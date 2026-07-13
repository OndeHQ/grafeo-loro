# Architecture Overview

## The Dual-Store Problem

Most systems choose one consistency model. We run two simultaneously:

| Store | Consistency | Query Pattern | Use Case |
|---|---|---|---|
| **Loro** | CRDT (eventual, mathematically proven) | Document-level, real-time sync | Collaborative state |
| **Grafeo** | ACID (serializable transactions) | Graph traversal, analytics | Structured queries |

The bridge makes them appear as one system.

---

## Data Flow Diagrams

### Inbound Path: Loro → Grafeo

```
┌─────────────┐    ┌─────────────────┐    ┌──────────────────┐    ┌─────────────┐
│  User edits │───►│  LoroDoc (CRDT) │───►│  DiffEvent fired │───►│  Subscriber   │
│  (local)    │    │  (source)       │    │  (origin check)  │    │  (translate)  │
└─────────────┘    └─────────────────┘    └──────────────────┘    └──────┬──────┘
                                                                         │
                    ┌────────────────────────────────────────────────────┘
                    ▼
┌─────────────┐    ┌─────────────────┐    ┌──────────────────┐    ┌─────────────┐
│  Grafeo DB  │◄───│  Batch flush    │◄───│  MutationBatcher │◄───│  mpsc::chan │
│  (materialized│   │  (tx per batch) │    │  (buffer/timeout)│    │  (bounded)  │
│   view)     │    └─────────────────┘    └──────────────────┘    └─────────────┘
└─────────────┘
```

**Key invariant**: Every Loro mutation becomes exactly one Grafeo transaction. No coalescing, no splitting.

### Outbound Path: Grafeo → Loro

```
┌─────────────┐    ┌─────────────────┐    ┌──────────────────┐    ┌─────────────┐
│  Grafeo CDC │───►│  changes_between│───►│  Epoch filter    │───►│  Outbound   │
│  (WAL tail) │    │  (epoch range)  │    │  (echo prevent)  │    │  channel    │
└─────────────┘    └─────────────────┘    └──────────────────┘    └──────┬──────┘
                                                                         │
                    ┌────────────────────────────────────────────────────┘
                    ▼
┌─────────────┐    ┌─────────────────┐    ┌──────────────────┐    ┌─────────────┐
│  LoroDoc    │◄───│  set_next_origin│◄───│  apply_change_   │◄───│  translate  │
│  (replica)  │    │  (bridge tag)    │    │  event_to_loro   │    │  (maps lookup)│
└─────────────┘    └─────────────────┘    └──────────────────┘    └─────────────┘
```

**Key invariant**: Bridge-tagged commits are filtered by the subscriber, preventing echo loops (I4, I5).

### Tree Move: The Hard Case

```
Before:                    After:
    A                         A
    │                         │
    B ──► C                   C
    │    /│\                 │
    └──► D  E                 B
                              │
                              D  E

Operation: TreeMove { node: B, old_parent: A, new_parent: C }
```

```rust
// Cycle detection runs BEFORE commit
fn would_create_cycle_in_tx(session, node_id, new_parent) -> bool {
    // BFS from new_parent up; if node_id found → cycle
}

// Old edge deleted, new edge created atomically
session.begin_transaction_with_isolation(Serializable)?;
if would_create_cycle_in_tx(...) { return Err(TreeMoveCreatesCycle); }
session.delete_edge(old_edge)?;
session.create_edge(new_parent, node_id, TREE_EDGE_LABEL)?;
session.prepare_commit()?.set_metadata(ORIGIN_LORO_BRIDGE).commit()?;
```

---

## Concurrency Model

### Thread Map

```
Main Thread (Tokio RT):
├── spawn_inbound_worker     (async, mpsc consumer)
├── spawn_outbound_worker    (async, mpsc consumer)  
├── spawn_cdc_poller         (async, sleep loop)
└── user code

Blocking Pool (spawn_blocking):
└── flush_inner              (Grafeo transaction, CPU-intensive)

Rayon Pool (global):
└── parallel_hydrate_grafeo  (chunked vertex hydration)
```

### Lock Hierarchy

```
BridgeMaps:
  node_id_map  : RwLock (read-heavy, write on upsert/delete)
  node_key_map : RwLock (inverse, always paired with node_id_map)
  edge_id_map  : RwLock (read-heavy, write on upsert/delete)
  edge_key_map : RwLock (inverse, always paired with edge_id_map)

LoroDoc:
  parking_lot::RwLock (write on import/commit, read on query)

Epoch Set:
  parking_lot::RwLock<HashSet> (write on commit, read on filter)
```

**Deadlock prevention**: BridgeMaps writes always acquire in order: node maps → edge maps. Never reverse.

---

## Consistency Guarantees

### Within a Single Process

| Property | Mechanism | Verifiable |
|---|---|---|
| Linearizable reads | Grafeo `Session` + `prepare_commit` | I6 (RYOW) |
| Snapshot isolation | `set_viewing_epoch` / `clear_viewing_epoch` | I12 |
| Serializable tree moves | `IsolationLevel::Serializable` + cycle check | I14 |
| Bijective bridge | Dual-map invariant | I11 |

### Across Processes (Sync)

| Property | Mechanism | Verifiable |
|---|---|---|
| Eventual consistency | CRDT merge (Loro) | I1, I2 |
| Echo suppression | Origin metadata + epoch filtering | I4, I5 |
| At-least-once delivery | mpsc bounded channel + retry | I3a, I3b |

---

## Memory Layout

### Hot Path (per op)

```
Stack:
  apply_loro_op(session, op, maps)
  ├── node_id_map.read()     [RwLock read guard]
  ├── grafeo::Session::set_node_property()
  └── (no heap alloc on read path)

Heap (BridgeMaps):
  HashMap<String, NodeId>     ~48 bytes per entry
  HashMap<NodeId, String>     ~48 bytes per entry  
  HashMap<EdgeKey, EdgeId>    ~72 bytes per entry
  HashMap<EdgeId, EdgeKey>    ~72 bytes per entry
```

### Cold Path (checkpoint)

```
LoroDoc::export(shallow_snapshot) → Vec<u8>
  └── CompressedPayload::compress_to_wire()
      ├── None:  memcpy
      ├── Lz4:   ~2x compression, ~1ms/MB
      └── Zstd:  ~5x compression, ~10ms/MB (level 3)
```

---

## Failure Modes

| Scenario | Behavior | Recovery |
|---|---|---|
| Inbound channel full | Drop + warn log | Backpressure to Loro (natural) |
| Outbound channel full | Drop + stop poller | Restart poller on next epoch |
| Flush timeout (5s) | Return Err, task continues | Caller retries or drains |
| Storage load fail | Skip delta, continue hydrate | Next hydrate retries |
| Storage delete fail | Warn log, continue | Next checkpoint retries |
| Cycle in tree move | Err before commit | Transaction rolled back |
| Bad wire format | Err on parse | Caller handles |
