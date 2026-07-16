# Grafeo-Loro

> **Local-first graph database with invisible consensus.**
>
> Your data lives on-device. Your team stays in sync. The network is an implementation detail.

---

## What You Get in 30 Seconds

```rust
use grafeo_loro::{GrafeoLoroApp, SsotMode, CompressionType};

// One line. Fully distributed. No servers to configure.
let app = GrafeoLoroApp::builder()
    .storage(s3_backend)
    .ssot_mode(SsotMode::Loro)
    .compression(CompressionType::Zstd)
    .build().await?;

// Writes are local. Sync is automatic. Conflicts are mathematically impossible.
app.sync_engine().inbound_sender()
    .send(LoroOp::UpsertNode { 
        loro_key: "V/alice".into(),
        labels: vec!["Person"],
        properties: [("name", "Alice".into())].into(),
    }).await?;
```

| You Want | You Do | It Handles |
|---|---|---|
| Offline work | Write normally | CRDT merge on reconnect |
| Real-time collab | Start the app | WebRTC/data-channel sync |
| Audit trail | Read the log | Every change, every peer, every millisecond |
| Time travel | Set epoch | MVCC snapshot isolation |
| Embedded search | Tag a node | ONNX vector generation + similarity |

---

## The Three Modes

### 1. Loro-SSOT (Default)
```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Client A  │◄───►│   Loro Doc  │◄───►│  Grafeo DB  │
│  (writes)   │     │  (source)   │     │  (query)    │
└─────────────┘     └─────────────┘     └─────────────┘
         │                                    │
         └───────────── Delta Sync ───────────┘
```
- Loro owns the document. Grafeo materializes views.
- Best for: collaborative editors, knowledge graphs, design tools.

### 2. Grafeo-SSOT (Enterprise)
```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Grafeo DB  │◄───►│   WAL Tail  │◄───►│   Loro Doc  │
│  (source)   │     │  (streaming)│     │  (replica)  │
└─────────────┘     └─────────────┘     └─────────────┘
```
- Grafeo owns the transaction log. Loro is a read replica.
- Best for: compliance, audit-heavy, existing graph infrastructure.

### 3. Hybrid Query (Automatic)
```rust
// Runs on Grafeo (fast graph traversal)
let neighbors = db.session()
    .execute("MATCH (n:Person)-[:KNOWS]->(m) RETURN m.name")?;

// Runs on Loro (live collaborative state)
let live_cursor = doc.get_map("presence")
    .get("cursor_positions");
```

---

## Documentation Map

| Document | Audience | Purpose |
|---|---|---|
| [`docs/grafeo-loro.architecture.md`](docs/grafeo-loro.architecture.md) | System designers | Data flow, concurrency model, consistency guarantees |
| [`docs/core.md`](docs/core.md) | Application developers | Type signatures, trait contracts, error handling |
| [`docs/guide.md`](docs/guide.md) | Platform engineers | Deployment, monitoring, disaster recovery |
| [`docs/guide.contributor.md`](docs/guide.contributor.md) | Contributors | Code structure, testing, fuzz targets |

---

## Core Abstractions

### The Bridge Maps
```rust
// Invisible bookkeeping. You never touch this.
pub struct BridgeMaps {
    node_id_map: RwLock<HashMap<String, NodeId>>,   // loro_key → grafeo_id
    node_key_map: RwLock<HashMap<NodeId, String>>,   // grafeo_id → loro_key
    edge_id_map:  RwLock<HashMap<EdgeKey, EdgeId>>,  // (src,dst,label) → grafeo_id
    edge_key_map: RwLock<HashMap<EdgeId, EdgeKey>>,  // grafeo_id → (src,dst,label)
}
```
Every Loro mutation → Grafeo transaction. Every CDC event → Loro update. The maps guarantee bijective consistency (invariant I11).

### The Sync Engine
```rust
let (engine, inbound_rx, outbound_rx) = SyncEngine::new(db, doc);

// Three concurrent workers, one channel contract:
// inbound_rx  : LoroDiff → GrafeoBatch (with echo suppression)
// outbound_rx : CDCEvent → LoroUpdate (with epoch filtering)
// poller      : WAL tail → outbound_rx (50ms heartbeat)
```

### The Mutation Batcher
```rust
// Configurable flush semantics
BatcherConfig {
    batch_size: 256,        // ops
    batch_ms: 100,          // max latency
    bridge_origin_epochs,   // echo prevention set
    maps,                   // bijective bridge
    shutdown_tx,            // graceful drain
    metrics,                // OpenTelemetry hooks
    tracer,                 // distributed tracing
    health,                 // staleness probe
}
```

---

## Invariant Checklist (Fuzz-Verified)

| ID | Invariant | Fuzz Target | Status |
|---|---|---|---|
| I1 | Tree state parity: `BridgeMaps.node_id_map` ≡ live Loro keys | `consistency.rs` | ✅ |
| I2 | Edge state parity: `BridgeMaps.edge_id_map` ≡ live Loro edges | `consistency.rs` | ✅ |
| I3a | `apply_loro_op` never panics | `consistency.rs` | ✅ |
| I3b | `MutationBatcher::run` never panics | `consistency.rs` | ✅ |
| I3c | `parallel_hydrate_grafeo` never panics | `consistency.rs` | ✅ |
| I4 | Echo loop bounded: epoch set ≤ `EPOCH_RETENTION + 1` | `consistency.rs` | ✅ |
| I5 | Origin filter symmetry: bridge-tagged ops roundtrip as filtered | `consistency.rs` | ✅ |
| I6 | Read-your-own-writes: commit → immediate visibility | `consistency.rs` | ✅ |
| I7 | Snapshot idempotency: same frontiers → same wire bytes | `consistency.rs` | ✅ |
| I8 | Compression round-trip: `compress ∘ decompress = id` | `consistency.rs` | ✅ |
| I9 | Hydration determinism: same doc → same node/edge counts | `consistency.rs` | ✅ |
| I10 | Vector offload: text update → embedding property | `consistency.rs` | ✅ |
| I11 | Bridge bijectivity: forward/inverse map lengths equal | `consistency.rs` | ✅ |
| I12 | MVCC snapshot isolation: pinned epoch → stable read | `consistency.rs` | ✅ |
| I14 | Tree acyclicity: no `CHILD` edge cycles post-move | `consistency.rs` | ✅ |
| I15 | Presence envelope: `build ∘ parse = id`, bad magic rejected | `consistency.rs` | ✅ |

Run: `cargo +nightly fuzz run consistency`

---

## Telemetry & Observability

### Metrics (OpenTelemetry)
```rust
// All instruments auto-registered on build
m.inbound_events.add(1, &[KeyValue::new("event_type", "vertex")]);
m.outbound_events.add(1, &[KeyValue::new("origin", "grafeo")]);
m.echo_filtered.add(1, &[KeyValue::new("direction", "inbound")]);
m.record_batch_flush(duration_ms, batch_size);
m.record_hydration(duration_ms, HydrationMode::Loro);
```

### Health Probe
```rust
let status = health.check(max_staleness_ms);
// status.overall      : bool
// status.components   : [("loro_doc", true), ("grafeo_db", true), ("sync_freshness", true)]
```

### Distributed Tracing
```rust
// Spans emitted automatically:
// - cold_start_hydration
// - inbound_sync_loop / outbound_sync_loop
// - hybrid_query
// - batch_flush (with grafeo_commit child)
// - hydrate_chunk
```

---

## Storage & Checkpointing

### Wire Format
```
┌────────┬────────┬─────────────────────┐
│ 0x01   │ 0x00   │ <raw bytes>         │  = Uncompressed
│ 0x01   │ 0x01   │ <lz4 payload>       │  = LZ4
│ 0x01   │ 0x02   │ <zstd payload>      │  = Zstd (default, level 3)
└────────┴────────┴─────────────────────┘
 version   codec
```

### Checkpoint Lifecycle
```rust
// Loro-SSOT: snapshot + delta cleanup
app.checkpoint("graph-42").await?;
// → writes: graph-42/base.loro (zstd snapshot)
// → deletes: graph-42/delta-*.loro (idempotent)

// Cold start: base + deltas → parallel hydrate
app.hydrate("graph-42").await?;
// → loads base.loro, applies deltas in order, runs parallel_hydrate_grafeo
```

---

## Schema: Graph as Code

```rust
#[derive(Hydrate, Reconcile)]
pub struct VertexEntity {
    pub labels: Vec<String>,
    pub properties: HashMap<String, LoroProperty>,
    #[loro(text)]
    pub description: String,  // Collaborative rich text
}

#[derive(Hydrate, Reconcile)]
pub struct EdgeEntity {
    pub label: String,
    pub src: String,
    pub dst: String,
    pub properties: HashMap<String, LoroProperty>,
}

#[derive(Hydrate, Reconcile)]
pub struct TreeNode {
    #[key]
    pub node_id: String,
    pub title: String,
    #[loro(movable)]
    pub items: Vec<TreeNode>,  // Drag-and-drop ordering
}
```

---

## Presence (Ephemeral Overlay)

```rust
let mgr = PresenceManager::new("room-42".into());

// Broadcast: 4-byte magic + 2-byte room_len + room + 1-byte type + JSON
let bytes = PresenceManager::build_eph_envelope(&room_id, &payload)?;
// → %EPH + le_u16(room.len()) + room + 0x01 + serde_json(payload)

// Parse: strict validation, no panic paths
let envelope = PresenceManager::parse_eph_envelope(&bytes)?;
```

---

## Quick Reference

### Building
```bash
cargo build --release

# Fuzzing
cargo +nightly fuzz run consistency

# Corpus generation
cargo run --bin gen_corpus --manifest-path fuzz/Cargo.toml
```

### Feature Matrix
| Feature | Status | Gate |
|---|---|---|
| Loro-SSOT checkpoint | ✅ | default |
| Grafeo-SSOT checkpoint | 🚧 | `wal` feature |
| ONNX embeddings | 🚧 | `onnx` feature |
| WebRTC sync transport | 🚧 | `webrtc` feature |
| Persistent Grafeo | ✅ | `grafeo_dir` in builder |

---

## WASM Browser Consumer Usage

Browser consumers on `wasm32-unknown-unknown` can use the offline op-queue
and lineage epoch tracker directly without enabling `batcher` (tokio) or
`grafeo` (native ONNX/ort) — both are WASM-incompatible.

### Cargo.toml

```toml
grafeo-loro = { version = "0.4", default-features = false, features = ["bridge", "tree", "wasm"] }
```

### JavaScript

```js
import init, { WasmOfflineOpQueue, WasmEpochTracker } from "grafeo-loro";

await init();  // one-time wasm bootstrap

// Offline op-queue: serialize LoroOps and replay on reconnect
const queue = new WasmOfflineOpQueue();
queue.enqueue(new Uint8Array([1, 2, 3]));  // serialized LoroOp bytes
queue.enqueue(new Uint8Array([4, 5, 6]));
console.log(queue.depth);        // 2
console.log(queue.bytesUsed);    // 6
console.log(queue.capBytes);     // 10485760 (10 MB default)

// On reconnect: drain + flush
const ops = queue.drain();        // Array<Uint8Array>
for (const op of ops) {
  await sendToRemote(op);
}
queue.resetRetry();

// Lineage epoch: validate sync handshake
const epoch = new WasmEpochTracker();
console.log(epoch.current);      // 0
const newEpoch = epoch.bump();   // 1
try {
  epoch.checkMatch(remoteEpoch); // throws on mismatch
  // ... proceed with sync
} catch (err) {
  if (err.code === 1013) {
    // EpochMismatchError — wipe local cache before retrying
    epoch.wipe();
    await wipeLocalCache();
  }
}
```

### No `merge` / `awareness` / `persistence` feature

If you saw these feature names in a downstream Cargo.toml, they were invented.
Enabling them now produces a `compile_error!`:

- `merge` → use `doc.import(other.export(loro::ExportFormat::Snapshot))`
- `awareness` → use the `presence` module (always available with `bridge`)
- `persistence` → use the `storage` feature (`StorageBackend` trait)

---

## License

MIT OR Apache-2.0
