# Architecture: `grafeo-loro`

> **System**: Local-first, in-process, dual-store graph database with CRDT consensus.
> **Philosophy**: Separate consensus (Loro) from execution (Grafeo). Zero cloud servers. Zero coordination overhead.

---

## Table of Contents

1. [System Topology & Architectural Philosophy](#1-system-topology--architectural-philosophy)
2. [Switchable SSOT Configurations](#2-switchable-ssot-configurations)
3. [Component Roles & Boundaries](#3-component-roles--boundaries)
4. [Local-First Lifecycle Flow](#4-local-first-lifecycle-flow)
5. [Root Container Schema](#5-root-container-schema)
6. [Declarative Mapping via `lorosurgeon`](#6-declarative-mapping-via-lorosurgeon)
7. [Ordered Sequences & Movable Trees](#7-ordered-sequences--movable-trees)
8. [Concurrency & Deadlock Prevention](#8-concurrency--deadlock-prevention)
9. [Echo Feedback Loop Prevention](#9-echo-feedback-loop-prevention)
10. [Rust Event Loop & Origin Processing](#10-rust-event-loop--origin-processing)
11. [Shallow Snapshotting](#11-shallow-snapshotting)
12. [In-Memory Ephemeral Presence](#12-in-memory-ephemeral-presence)
13. [Loro 1.0 Document Size Trade-Off](#13-loro-10-document-size-trade-off)
14. [Dual-Layer Compression Pipeline](#14-dual-layer-compression-pipeline)
15. [Compression Wrapper Implementation](#15-compression-wrapper-implementation)
16. [Parallel Index Hydration Engine](#16-parallel-index-hydration-engine)
17. [Asynchronous Vector Generation & Offloading](#17-asynchronous-vector-generation--offloading)
18. [Post-Sync Hybrid Query](#18-post-sync-hybrid-query)
19. [Non-Blocking MVCC & Snapshot Isolation](#19-non-blocking-mvcc--snapshot-isolation)
20. [Inbound Mutation Batcher](#20-inbound-mutation-batcher)
21. [Read-Your-Own-Writes Consistency](#21-read-your-own-writes-consistency)
22. [Concurrent Write Scaling via Block-STM](#22-concurrent-write-scaling-via-block-stm)
23. [Observability](#23-observability)
24. [Installation & Usage](#24-installation--usage)

---

## 1. System Topology & Architectural Philosophy

Local-first, in-process, dual-store architecture. Separates consensus (Loro) from execution (Grafeo).

```text
+-----------------------------------------------------------------------------------+
|                                 Local Client                                      |
|                                                                                   |
|  +--------------------+                                   +--------------------+  |
|  |     LoroDoc        |                                   |     GrafeoDB       |  |
|  |  (SSOT: Consensus) |                                   | (SSOT: Execution)  |  |
|  +---------+----------+                                   +---------^----------+  |
|            |                                                        |             |
|            | (LoroEvent)                                            | (Direct API)|
|            v                                                        |             |
|  +---------+--------------------------------------------------------+----------+  |
|  |                              LoroGrafeoBridge                                  |  |
|  +---------^--------------------------------------------------------+----------+  |
|            |                                                        |             |
|            | (Loro Bytes)                                           | (CDC Event) |
|            v                                                        v             |
+------------+--------------------------------------------------------+-------------+
             ^                                                        |
             | (Import / Export if Loro SSOT)                         | (If Grafeo SSOT)
             v                                                        v
+------------+----------------------------------------------------------------------+
|                                Storage Backend                                    |
|                                                                                   |
|   The application provides its own storage layer (filesystem, S3, IPFS, etc.)     |
|   via the `StorageBackend` trait. The architecture handles compression only.        |
+-----------------------------------------------------------------------------------+
```

---

## 2. Switchable SSOT Configurations

```rust
pub enum SsotMode {
    Loro,   // Storage stores .loro. Grafeo is ephemeral query cache.
    Grafeo, // Storage stores backup.tar.zst. Loro is ephemeral merge engine.
}
```

### Mode Selection Guide

| Feature / Constraint | `SsotMode::Loro` | `SsotMode::Grafeo` |
| :--- | :--- | :--- |
| **Primary Storage Artifact** | `.loro` (Binary CRDT snapshot) | `.tar.zst` (Compressed database folder) |
| **Time Travel Capabilities** | Yes (via Loro native frontiers) | No (CRDT history discarded on session teardown) |
| **Vector / HNSW Indexes** | Regenerated locally on cold boot | Saved directly (no network regeneration) |
| **Storage Size** | Minimal (RLE compressed, history-trimmed) | Heavy (Contains binary indices, zone maps) |
| **Boot Speed Pattern** | Slow DB index rebuild, fast download | Instant DB attach, slow download |
| **Schema Evolution** | Decoupled client-side migrations | Fast-path native Grafeo transactions |

---

## 3. Component Roles & Boundaries

### LoroDoc (Consensus Layer)
*   **Role**: Authoritative single source of truth (SSOT) for document state, history, and network merges.
*   **Memory**: High-efficiency RLE (Run-Length Encoding) operations log DAG.
*   **Attributes**: Manages Lamport clocks, peer IDs, frontiers, and conflict-free concurrent editing resolution.

### GrafeoDB (Execution Layer)
*   **Role**: Materialized view optimized for local runtime queries, analytics, and indexing.
*   **Memory**: Columnar blocks, Compressed Sparse Row (CSR) adjacency indexes, HNSW vector indexes, and BM25 text inverted index.
*   **Attributes**: Parallel push-based vectorized execution, morsel-driven thread scaling.

### LoroGrafeoBridge (Glue Layer)
*   **Role**: In-process bidirectional sync manager.
*   **Memory**: Multi-thread ownership locks and synchronous transaction buffers.
*   **Attributes**: Converts `LoroEvent` diffs to Grafeo direct database updates. Converts Grafeo `CdcEvent` streams to Loro mutations.

### Storage Backend (Pluggable)
*   **Role**: The application provides its own storage layer via the `StorageBackend` trait.
*   **Interface**: `load(key)`, `save(key, bytes)`, `list(prefix)`, `delete(key)`.
*   **Attributes**: The architecture is storage-agnostic. Implement S3, filesystem, IPFS, or any custom backend.

---

## 4. Local-First Lifecycle Flow

### Step A: Cold Startup
1.  Client process launches.
2.  Fetches compressed snapshot from storage backend via `StorageBackend::load()`.
3.  Decompresses payload using the dual-layer compression pipeline.
4.  Hydrates local memory `LoroDoc` using `doc.import_with_status(&bytes)`.
5.  `LoroGrafeoBridge` reads final state of `LoroDoc`, iterates through active containers, and populates local in-memory or on-disk `GrafeoDB` cache.

### Step B: Offline Mutation
1.  User modifies graph offline (e.g., adds node, changes property).
2.  Grafeo local database applies change instantly (<1ms). UI redraws.
3.  Grafeo emits `CdcEvent` (polled by the bridge's CDC poller worker — grafeo 0.5.42 CDC is poll-based, not push-based).
4.  `LoroGrafeoBridge` consumes the `CdcEvent`, takes the `LoroDoc` write lock, calls `set_next_commit_origin("grafeo-bridge")`, applies the equivalent mutation, and calls `commit()`. (Loro 1.x is auto-commit — there is no `transact_mut()`.)
5.  Updates stored in local Loro oplog.

### Step C: Reconciliation (Online Sync)
1.  Network connection restores.
2.  Local client exchanges Loro version vectors with peers or central coordinator.
3.  Calculates delta binary updates via `doc.export(ExportMode::Updates)`.
4.  Imports incoming remote updates via `doc.import_with_status()`.
5.  Loro resolves structural conflicts using LWW and Fugue algorithm automatically.
6.  `LoroDoc` emits `LoroEvent` diffs.
7.  `LoroGrafeoBridge` processes diffs, writes updates to `GrafeoDB`.
8.  `GrafeoDB` rebuilds indexes. UI updates.

### Step D: Session Termination
1.  Session ends. Last client disconnects.
2.  Client exports finalized state as single shallow snapshot via `doc.export(ExportMode::ShallowSnapshot)`.
3.  Compresses payload using the dual-layer compression pipeline.
4.  Saves compressed payload to storage backend via `StorageBackend::save()`. History discarded to prevent storage bloat.

---

## 5. Root Container Schema

The root level of `LoroDoc` holds three main partition containers. This layout separates generic graph mutations, cyclic links, and hierarchical tree graphs.

```text
LoroDoc
 ├── "V" (LoroMap) ────────────────> Registry of all Vertices
 │    └── <NodeID: String> (LoroMap) -> Single Vertex Entity
 │
 ├── "E" (LoroMap) ────────────────> Registry of Cyclic/Generic Edges
 │    └── <EdgeID: String> (LoroMap) -> Directed Edge Link
 │
 └── "T_CHILD" (LoroTree) ─────────> Strict Spanning Tree (Prevents Move Cycles)
      └── TreeNodes ────────────────> Identifiers mapping to Vertex IDs
```

---

## 6. Declarative Mapping via `lorosurgeon`

Using `lorosurgeon` avoids manual `LoroValue` parsing. It provides declarative bidirectional conversion between native Rust structures and Loro CRDT containers using `#[derive(Hydrate, Reconcile)]`.

### Vertex Entity Schema

Vertices hold arrays, primitive properties, and cooperative rich-text properties.

```rust
use std::collections::HashMap;
use lorosurgeon::{Hydrate, Reconcile};

#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct VertexEntity {
    // Labels array mapped to LoroList
    pub labels: Vec<String>,

    // Primitive properties mapped to LoroMap
    pub properties: HashMap<String, LoroProperty>,

    // Collaborative text mapped to LoroText (Fugue-managed)
    #[loro(text)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
#[serde(untagged)]
pub enum LoroProperty {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
}
```

### Generic Edge Entity Schema

Edges hold connections and edge-specific weights or relationships.

```rust
#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct EdgeEntity {
    pub label: String,

    // Explicit NodeID string boundaries
    pub src: String,
    pub dst: String,

    pub properties: HashMap<String, LoroProperty>,
}
```

---

## 7. Ordered Sequences & Movable Trees

To model ordered structural lists (like card positions or child node indices) without duplicate conflicts, `grafeo-loro` leverages identity-preserving lists and native tree movements.

```rust
#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct OrderedCollection {
    // Identity-preserving movable list. Prevents interleaving during drag-drops.
    #[loro(movable)]
    pub items: Vec<PlaylistItem>,
}

#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct PlaylistItem {
    // Unique ID used to identify element across concurrent moves
    #[key]
    pub track_id: String,
    pub title: String,
}
```

### Tree Movement Mapping (`T_CHILD`)

*   **Operation**: Moving node `X` from parent `A` to parent `B`.
*   **Safety**: Loro's `LoroTree` enforces an acyclic graph internally.
*   **Grafeo Translation**: `LoroGrafeoBridge` catches tree move events, translates them to Grafeo-compliant transaction mutations.

```rust
// Inbound sync translation layer converts Loro tree moves:
fn sync_tree_move_to_grafeo(db: &grafeo::GrafeoDB, node_id: u64, old_parent: u64, new_parent: u64) {
    let mut tx = db.begin_write_tx();

    // Remove stale parent relationship
    tx.execute(
        "MATCH (p:Folder {id: $old_p})-[r:CHILD]->(c:Folder {id: $cid}) DELETE r",
        vec![("old_p", old_parent.into()), ("cid", node_id.into())]
    ).unwrap();

    // Write validated non-cyclic parent relationship
    tx.execute(
        "MATCH (p:Folder {id: $new_p}), (c:Folder {id: $cid}) INSERT (p)-[:CHILD]->(c)",
        vec![("new_p", new_parent.into()), ("cid", node_id.into())]
    ).unwrap();

    tx.commit().unwrap();
}
```

### Known Ambiguity: `OrderedCollection` (LoroMovableList) vs `T_CHILD` (LoroTree)

The codebase has two distinct "tree" concepts that share the word "tree" but use different Loro containers and serve different phases:

- **`OrderedCollection`** (`LoroMovableList`, `src/schema/tree.rs:6-9`): a flat ordered list of `TreeNode`s for drag-drop UI ordering. Identity is preserved via `#[key] node_id` + `#[loro(movable)]`. No parent/child relationship. Phase 2 Task 1 territory (this section).
- **`T_CHILD`** (`LoroTree`, `src/constants.rs:8` comment): a strict spanning tree that prevents cycles during parent moves. Identity is `TreeID` (native Loro type, not `String`). Parent/child is managed by the `LoroTree` container itself, queried via `tree.get_parent(tree_id)`. Phase 2 Task 2 territory (`sync_tree_move_to_grafeo`).

`TreeNode` (this section, `src/schema/tree.rs:11-16`) belongs to `OrderedCollection` only. The `T_CHILD` `LoroTree` does not use `TreeNode` — its metadata (vertex_id mapping) lives in a separate container to be wired in Phase 2 Task 2.

---

## 8. Concurrency & Deadlock Prevention

Both engines share one OS process. To prevent deadlocks during bidirectional synchronization:
*   **Decoupled Writing**: Do not perform synchronous write loops inside event callbacks.
*   **Execution Locks**: `LoroDoc` runs inside `parking_lot::RwLock`. `GrafeoDB` manages internal lock-free reader threads and parallel writer queues.
*   **Async Buffering**: Use thread-safe `tokio::sync::mpsc` channels to offload mutations from synchronous callbacks into async worker loops.

```text
[Loro Thread] ──(Sync Callback)──> Push to MPSC ──> [Tokio Thread Pool] ──> Write to GrafeoDB
[Grafeo Worker] ──(CDC Event)─────> Push to MPSC ──> [Tokio Thread Pool] ──> Write to LoroDoc
```

---

## 9. Echo Feedback Loop Prevention

Bidirectional sync creates feedback loops where an update echoed back replicates infinitely.

```text
Loro Update ──> Bridge ──> Grafeo Write ──> Grafeo CDC ──> Bridge ──> Loro Write (Loop!)
```

### Solution: Origin Tracking + Epoch Side-Channel

1.  **Loro-to-Grafeo Skip** (origin tag on Loro commit, visible in subscriber):
    *   Set Loro transaction origin during bridge mutations using `doc.set_next_commit_origin("grafeo-bridge")`.
    *   In the Loro subscription handler, inspect `event.origin`. If it equals `"grafeo-bridge"`, discard the event.
2.  **Grafeo-to-Loro Skip** (epoch side-channel — see Known Limitation below):
    *   When the bridge commits a Grafeo transaction, `prepared.commit()` returns the `EpochId`.
    *   The bridge records that `EpochId` in an in-memory `HashSet<EpochId>` (`SyncEngine::bridge_origin_epochs`).
    *   The outbound CDC poller calls `session.changes_between(start, end)`, filters out any `ChangeEvent` whose `.epoch` is in the set, and forwards survivors to the outbound worker.
    *   The set is pruned each poll cycle to keep only epochs newer than `last_polled_epoch - EPOCH_RETENTION` (default 10_000).

### Known Limitation: Grafeo CDC has no origin field (Devil BLOCKER B2)

Grafeo 0.5.42's `ChangeEvent` carries `entity_id / kind / epoch / timestamp / before / after / labels / edge_type / src_id / dst_id / triple_*` — **no `origin` field**. `PreparedCommit::set_metadata(k, v)` is dropped on `commit()` (verified in grafeo-engine source: `commit()` calls `session.commit()` and never propagates `metadata` to `CdcLog`). The architecture's original design ("inspect the transaction origin in the CDC listener") therefore cannot be implemented as written.

**Workaround (orchestrator-approved)**: the epoch side-channel above. An upstream grafeo patch adding an `origin: Option<String>` field to `ChangeEvent` (and propagating `PreparedCommit::metadata` through the commit path) would let us delete the side-channel and return to the simpler origin-tag design. Out of scope for this loop.

---

## 10. Rust Event Loop & Origin Processing

Below is the concrete, thread-safe Rust synchronization engine.

> **Note (Devil BLOCKER B1/B2)**: The pseudocode below is **illustrative** —
> it shows the intended control flow, not literal grafeo 0.5.42 / loro 1.13.6
> API calls. The actual implementation in `src/bridge/sync_engine.rs` uses the
> grafeo `Session` + `PreparedCommit` API (`db.session_with_cdc(true)` →
> `session.begin_transaction()` → `session.create_node_with_props(...)` /
> `session.set_node_property(...)` / `session.delete_node(...)` →
> `session.prepare_commit()` → `prepared.set_metadata(...)` (advisory only —
> dropped on commit) → `prepared.commit() -> Result<EpochId>`), and the loro
> auto-commit model (`set_next_commit_origin` + `commit` — there is no
> `transact_mut()`). Echo prevention on the Grafeo→Loro path uses the epoch
> side-channel (§9) because grafeo's `ChangeEvent` has no `origin` field.

```rust
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use loro::{LoroDoc, LoroValue};
use grafeo::{GrafeoDB, cdc::ChangeEvent};

pub struct SyncEngine {
    db: Arc<GrafeoDB>,
    doc: Arc<RwLock<LoroDoc>>,
    // Bridge-internal worker channel
    inbound_tx: mpsc::Sender<loro::event::DiffEvent<'static>>, // illustrative
}

impl SyncEngine {
    pub fn new(db: Arc<GrafeoDB>, doc: Arc<RwLock<LoroDoc>>) -> Arc<Self> {
        let (inbound_tx, inbound_rx) = mpsc::channel(1024);

        let engine = Arc::new(Self {
            db,
            doc,
            inbound_tx,
        });

        // Start loops
        engine.spawn_inbound_worker(inbound_rx);
        engine.init_loro_subscriber();

        engine
    }

    /// 1. Synchronous Loro event handler. Converts events to async channel updates.
    fn init_loro_subscriber(self: &Arc<Self>) {
        let tx = self.inbound_tx.clone();
        let doc = self.doc.read(); // subscribe_root takes &self

        let _sub = doc.subscribe_root(Arc::new(move |event| {
            // Drop events generated by our own bridge mutations
            if event.origin == "grafeo-bridge" {
                return;
            }
            // Push valid remote/user edits to async processing thread
            let _ = tx.try_send(event.clone());
        }));
    }

    /// 2. Inbound Worker (Loro -> Grafeo)
    fn spawn_inbound_worker(self: &Arc<Self>, mut rx: mpsc::Receiver<DiffEvent>) {
        let db = self.db.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                // Begin a Grafeo Session transaction (Session API, not begin_write_tx).
                let mut session = db.session_with_cdc(true);
                session.begin_transaction().unwrap();

                // Set origin metadata (advisory only — dropped on commit;
                // the epoch side-channel is the real echo prevention).
                // session.set_metadata(...) lives on PreparedCommit, below.

                for diff in &event.events {
                    // session.create_node_with_props(labels, props_iter)
                    // session.set_node_property(id, key, value)
                    // session.delete_node(id)
                }

                let mut prepared = session.prepare_commit().unwrap();
                prepared.set_metadata("origin", "loro-bridge"); // advisory
                let _epoch = prepared.commit().unwrap(); // -> EpochId
                // TODO: insert epoch into bridge_origin_epochs set.
            }
        });
    }

    /// 3. Outbound Worker (Grafeo -> Loro)
    pub fn spawn_outbound_worker(self: &Arc<Self>, mut cdc_rx: mpsc::Receiver<ChangeEvent>) {
        let doc_lock = self.doc.clone();
        tokio::spawn(async move {
            while let Some(event) = cdc_rx.recv().await {
                // Filter via epoch side-channel (done at poll time in
                // spawn_cdc_poller; defensive double-check here).
                // if bridge_origin_epochs.contains(&event.epoch) { continue; }

                let doc = doc_lock.write();

                // Identify origin to prevent echo
                doc.set_next_commit_origin("grafeo-bridge");
                // Apply equivalent mutation to Loro (auto-commit model:
                // no transact_mut — call container mutators directly).
                // ...doc.get_map("V").insert(...)...;
                doc.commit();
            }
        });
    }

    /// 4. CDC Poller (Grafeo CDC is poll-based in 0.5.42, not push-based).
    pub fn spawn_cdc_poller(self: &Arc<Self>) {
        let db = self.db.clone();
        let tx = self.outbound_tx.clone();
        tokio::spawn(async move {
            let mut last_epoch = db.current_epoch();
            loop {
                tokio::time::sleep(Duration::from_millis(OUTBOUND_POLL_MS)).await;
                let current = db.current_epoch();
                if current <= last_epoch { continue; }
                let session = db.session_with_cdc(true);
                let events = session.changes_between(last_epoch, current).unwrap();
                for ev in events {
                    if bridge_origin_epochs.read().contains(&ev.epoch) { continue; }
                    let _ = tx.send(OutboundMsg { epoch: ev.epoch, payload: ev }).await;
                }
                // Prune: keep only epochs > last_epoch - EPOCH_RETENTION.
                last_epoch = current;
            }
        });
    }
}
```

---

## 11. Shallow Snapshotting

To prevent storage and network payload bloat from long-running collaborative histories, `grafeo-loro` truncates old history using **Shallow Snapshot Encoding**.

```text
Full History:  Op1 ──> Op2 ──> Op3 ──> Op4 ──> Op5 (Current State)
                                         │
                        [Truncate older than Op4]
                                         │
                                         ▼
Shallow Snapshot:                      [Op4] ──> Op5 (Current State with minimal clocks)
```

This preserves the current state and the minimum version vector/clock metadata required to continue peer-to-peer merges safely, but completely discards older operation logs.

---

## 12. In-Memory Ephemeral Presence

Real-time presence (active nodes, select highlights, mouse cursors) is kept ephemeral. It is never written to the CRDT document or saved to storage.

Clients use `%EPH` (Ephemeral Store) message envelopes over a WebSocket channel:

```text
+--------------------------------------------------------------+
|                   WebSocket Binary Message                   |
|                                                              |
|  [Magic Bytes]  [Room ID VarString]  [Msg Type]  [Payload]   |
|   "%EPH" (4B)     "graph_123" (9B)     0x01       Message    |
+--------------------------------------------------------------+
```

### Presence Struct (Rust)

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PresencePayload {
    pub peer_id: u64,
    pub active_node: Option<String>,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub last_active_ts: u64,
}
```

---

## 13. Loro 1.0 Document Size Trade-Off

Loro 1.0 optimizes for raw importing and parsing speed (10x-100x faster than traditional CRDTs).

Trade-off:
*   Without compression, a Loro 1.0 snapshot is roughly **twice the size** of alternative CRDT formats.
*   It encodes both historical operations and current document states explicitly within the binary layout, avoiding runtime reconstruction decompression inside the CRDT core.
*   Loro delegates payload compression to the host application.

To minimize storage costs and network transit, `grafeo-loro` implements a dual-layer compression pipeline in Rust.

---

## 14. Dual-Layer Compression Pipeline

```text
                  +----------------------------------------+
                  |         Exported Loro Payload          |
                  +-------------------+--------------------+
                                      |
                    [Is Hot Sync Packet or Cold Snapshot?]
                                      |
                     +----------------+----------------+
                     | Hot Sync                        | Cold Snapshot
                     v                                 v
          +--------------------+            +--------------------+
          |    LZ4 Encoder     |            |    ZSTD Encoder    |
          | (High Throughput)  |            |  (High Compression)|
          +--------------------+            +--------------------+
```

### Hot Sync Packet (LZ4 Block)
*   **Target**: In-flight synchronization packets (`.loro.delta`).
*   **Performance**: Fast-path compression. Negligible CPU overhead. Keeps latency low.

### Cold Snapshot (Zstd Level 3)
*   **Target**: Checkpointed shallow snapshots (`.loro` stored in storage).
*   **Performance**: High compression ratio. Shrinks document size by >60%, neutralizing Loro 1.0's state-duplication storage penalty.

---

## 15. Compression Wrapper Implementation

`CompressionType` is defined in `src/config.rs` (SSOT — Phase 4 storage references it via `crate::config::CompressionType`). Error type is `GrafeoLoroError` (see `src/error.rs`); `Compression(String)` is the symmetric codec-failure variant for BOTH LZ4 (`DecompressError`) and Zstd (`io::Error`) — `StorageIo` is reserved for storage backend I/O. Zstd level is sourced from `crate::constants::DEFAULT_ZSTD_LEVEL` (= 3, zstd's own `CLEVEL_DEFAULT`).

```rust
use loro::{LoroDoc, ExportMode};

use crate::config::CompressionType;
use crate::constants::DEFAULT_ZSTD_LEVEL;
use crate::error::{GrafeoLoroError, Result};

// Cargo.toml dependencies:
// zstd = "0.13"     // binds to C zstd (no pure-Rust encoder exists in the ecosystem)
// lz4_flex = "0.11" // pure-Rust

/// Compressed payload envelope. In-memory only — Phase 4 `StorageBackend` adds
/// the wire format (codec byte + raw bytes) for `save`/`load`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedPayload {
    pub compression: CompressionType,
    pub raw_data: Vec<u8>,
}

impl CompressedPayload {
    /// Compress `raw_bytes` using `strategy`. Fails on Zstd I/O errors
    /// (routed via `Compression(String)`, symmetric with LZ4 — NOT `StorageIo`).
    pub fn compress(raw_bytes: &[u8], strategy: CompressionType) -> Result<Self> {
        let raw_data = match strategy {
            CompressionType::None => raw_bytes.to_vec(),
            CompressionType::Lz4 => lz4_flex::compress_prepend_size(raw_bytes),
            CompressionType::Zstd => zstd::stream::encode_all(raw_bytes, DEFAULT_ZSTD_LEVEL)
                .map_err(|e| GrafeoLoroError::Compression(e.to_string()))?,
        };
        Ok(Self { compression: strategy, raw_data })
    }

    /// Decompress `raw_data` back to the original Loro bytes.
    pub fn decompress(&self) -> Result<Vec<u8>> {
        match self.compression {
            CompressionType::None => Ok(self.raw_data.clone()),
            CompressionType::Lz4 => lz4_flex::decompress_size_prepended(&self.raw_data)
                .map_err(|e| GrafeoLoroError::Compression(e.to_string())),
            CompressionType::Zstd => zstd::stream::decode_all(&self.raw_data[..])
                .map_err(|e| GrafeoLoroError::Compression(e.to_string())),
        }
    }
}

pub trait LoroDocCompressionExt {
    fn export_compressed(&self, mode: ExportMode, strategy: CompressionType) -> Result<CompressedPayload>;
    /// Returns `ImportStatus` so Phase 4 `hydrate()` can detect pending
    /// dependencies (Loro's `import` doc warns about missing dependency ranges).
    fn import_compressed(&self, payload: &CompressedPayload) -> Result<loro::ImportStatus>;
}

impl LoroDocCompressionExt for LoroDoc {
    fn export_compressed(&self, mode: ExportMode, strategy: CompressionType) -> Result<CompressedPayload> {
        // `LoroDoc::export` returns `Result<Vec<u8>, LoroEncodeError>`; `LoroEncodeError`
        // chains to `LoroError` via `From` (loro-common error.rs:204), then to
        // `GrafeoLoroError::Loro` via `#[from]`. Two-hop chain requires explicit
        // `.map_err` (single `?` won't auto-chain two `From`s).
        let bytes = self.export(mode).map_err(|e| GrafeoLoroError::Loro(e.into()))?;
        CompressedPayload::compress(&bytes, strategy)
    }

    fn import_compressed(&self, payload: &CompressedPayload) -> Result<loro::ImportStatus> {
        // `LoroDoc::import(&self, &[u8])` returns `Result<ImportStatus, LoroError>`
        // (takes `&self` — interior mutability; NOT `&mut self`). `ImportStatus` is
        // surfaced to the caller (DEVIL M2 — Phase 4 hydrate() inspects `pending`).
        // No origin tag: compression module is origin-agnostic (Phase 4 wraps with `import_with` if needed).
        let bytes = payload.decompress()?;
        Ok(self.import(&bytes)?)  // LoroError -> GrafeoLoroError::Loro via #[from]
    }
}
```

---

## 16. Parallel Index Hydration Engine

When `LoroDoc` imports a compressed snapshot from storage, the local `GrafeoDB` cache begins empty. Rebuilding Grafeo's structural indexes (CSR Adjacency, HNSW Vector, and BM25 Text) from Loro raw containers must be parallelized to prevent UI thread lockups.

```text
                       +---------------------------+
                       |   Loro State (Hydrated)   |
                       +-------------+-------------+
                                     |
                          [Split into CPU chunks]
                                     |
                     +---------------+---------------+
                     | Chunk 1       | Chunk 2       |
                     v               v               v
            +----------------+----------------+----------------+
            |  Worker Thread |  Worker Thread |  Worker Thread | (Rayon)
            +--------+-------+--------+-------+--------+-------+
                     |                |                |
                     v                v                v
            +--------------------------------------------------+
            |      Grafeo Vectorized Write Transaction        | (Block-STM)
            +------------------------+-------------------------+
                                     |
               +---------------------+---------------------+
               |                     |                     |
               v                     v                     v
      +------------------+  +------------------+  +------------------+
      |  CSR Adjacency   |  |    HNSW Index    |  |    BM25 Index    |
      +------------------+  +------------------+  +------------------+
```

### Chunked Parallel Processing (Rust)

Use `rayon` to chunk Loro map collections and parallelize Grafeo transaction insertions. Constants (`DEFAULT_CHUNK_SIZE`, `ORIGIN_LORO_BRIDGE`, `ROOT_VERTICES`) are sourced from `crate::constants` (SSOT — no literals). Error type is `GrafeoLoroError` (see `src/error.rs`); per-vertex unwrap failures route via `Bridge(String)` (vertex missing or not a `Container::Map`), and `VertexEntity::hydrate_map` errors route via `Hydrate(#[from] lorosurgeon::error::HydrateError)` (structured vertex-shape mismatch — P3T2-L2R2 M2 replaces the prior `Bridge(format!(...))` band-aid). The read-path SSOT is `VertexEntity::hydrate_map(&LoroMap)` (`lorosurgeon-0.2.1/src/hydrate.rs:127`) — DO NOT manually iterate the vertex sub-map's fields. `VertexEntity::description` (`LoroText`) is Loro-only (`src/app.rs:201`) and is naturally isolated from `properties` by the SSOT read path — hydration skips it.

> **Preconditions** (DEVIL M3): hydration MUST run BEFORE `bridge::sync_engine`'s Loro subscriber starts (or Phase 4 must tag the storage-import commit with `ORIGIN_LORO_BRIDGE` and rely on the B1 filter). `session_with_cdc(false)` only suppresses the outbound Grafeo→Loro echo; it does NOT suppress the inbound Loro→Grafeo echo from the live subscriber. See §9 echo prevention.

```rust
use std::sync::Arc;
use loro::LoroDoc;
use grafeo::GrafeoDB;
use rayon::prelude::*;
use lorosurgeon::Hydrate; // VertexEntity::hydrate_map SSOT (lorosurgeon-0.2.1/src/hydrate.rs:127)

use grafeo_loro::bridge::{apply_loro_op, BridgeMaps};
use grafeo_loro::constants::{DEFAULT_CHUNK_SIZE, ORIGIN_LORO_BRIDGE, ROOT_VERTICES};
use grafeo_loro::error::{GrafeoLoroError, Result};
use grafeo_loro::schema::vertex::VertexEntity;
use grafeo_loro::types::events::LoroOp;
use grafeo_loro::types::values::GraphValue;

/// Rebuilds Grafeo indexes from Loro state using Rayon chunks of
/// `DEFAULT_CHUNK_SIZE`. Each chunk runs in its own Grafeo `Session`
/// transaction tagged with `ORIGIN_LORO_BRIDGE`; the `loro_key ↔ NodeId`
/// mapping is recorded in `maps`. Fail-fast: first chunk error aborts the
/// whole call (anti-plenger #9 Absolute Idempotency).
pub fn parallel_hydrate_grafeo(
    db: &Arc<GrafeoDB>,
    doc: &LoroDoc,
    maps: &BridgeMaps,
) -> Result<()> {
    // 1. Extract vertex keys from Loro root map "V".
    //    LoroDoc::get_map verified at loro-1.13.6/src/lib.rs:489 (no `txn` arg —
    //    LoroDoc uses interior mutability, NOT `doc.transact()` which does NOT exist).
    //    LoroMap::keys verified at lib.rs:2315 → impl Iterator<Item = InternalString>;
    //    InternalString→String via Display (loro-common-1.13.1/src/internal_string.rs:194).
    let v_root = doc.get_map(ROOT_VERTICES);
    let keys: Vec<String> = v_root.keys().map(|s| s.to_string()).collect();

    // 2. Parallel chunk processing — Session is single-threaded
    //    (grafeo-engine-0.5.42/src/session/mod.rs) so each chunk owns its own.
    keys.par_chunks(DEFAULT_CHUNK_SIZE).try_for_each(|chunk| -> Result<()> {
        // cdc=false suppresses outbound Grafeo→Loro echoes (matches app.rs:437).
        let mut session = db.session_with_cdc(false); // verified at database/mod.rs:1728
        session.begin_transaction()?; // verified at session/mod.rs:3883

        // 3. Per-vertex hydration via SSOT (DEVIL M2 — DO NOT manually iterate).
        for key in chunk {
            let voc = v_root.get(key) // verified at lib.rs:2150 → Option<ValueOrContainer>
                .ok_or_else(|| GrafeoLoroError::Bridge(format!("vertex {key} missing")))?;
            // `into_container()` returns `Result<Container, Self>` and
            // `into_map()` returns `Result<LoroMap, Self>` (both via
            // `EnumAsInner` at lib.rs:3813 / :3636); the two error types
            // differ, so collapse both to `Option` via `.ok()` before
            // `and_then` + `ok_or_else` (the original enum variants are
            // diagnostic-only — the user-facing message is "not a Container::Map").
            let vertex_map = voc.into_container() // lib.rs:3813 (EnumAsInner)
                .ok()
                .and_then(|c| c.into_map().ok()) // lib.rs:3636 (EnumAsInner on Container)
                .ok_or_else(|| GrafeoLoroError::Bridge(format!("vertex {key} not a Container::Map")))?;
            // `hydrate_map` errors route via `From<HydrateError> for GrafeoLoroError`
            // (P3T2-L2R2 M2) — `?` preserves the structured `HydrateError`
            // (Missing/Unexpected/Overflow/Json variants carry property-level
            // context). `From<LoroProperty> for GraphValue` impl lives at
            // `src/types/values.rs:120-135` (P3T2-L3 added).
            let entity: VertexEntity = VertexEntity::hydrate_map(&vertex_map)?; // SSOT

            // 4. Build LoroOp::UpsertNode + apply via SSOT (grafeo_tx.rs:86).
            let op = LoroOp::UpsertNode {
                loro_key: key.clone(),
                labels: entity.labels,
                properties: entity.properties
                    .into_iter()
                    .map(|(k, v)| (k, GraphValue::from(v)))
                    .collect(),
            };
            apply_loro_op(&session, &op, maps)?;
        }

        // 5. Prepare + commit with origin tag. Metadata is advisory-only per
        //    Devil Gap 1 (dropped on commit — see §9 echo prevention for the
        //    `bridge_origin_epochs` side-channel that actually filters echoes).
        let mut prepared = session.prepare_commit()?; // verified at session/mod.rs:4496
        prepared.set_metadata(ORIGIN_LORO_BRIDGE, ORIGIN_LORO_BRIDGE); // prepared.rs:107
        prepared.commit()?; // verified at prepared.rs:124 — consumes self
        Ok(())
    })
}
```

Loro 1.13.6 verified API surface (DEVIL M1 — replaces pre-verification sketch):
- `LoroDoc::get_map<I: IntoContainerId>(&self, I) -> LoroMap` — `loro-1.13.6/src/lib.rs:489` (no `txn` arg; LoroDoc uses interior mutability).
- `LoroMap::keys(&self) -> impl Iterator<Item = InternalString> + '_` — `:2315` (collect `Vec<String>` via `Display`).
- `LoroMap::get(&self, &str) -> Option<ValueOrContainer>` — `:2150` (NOT `Option<LoroValue>`).
- `ValueOrContainer::into_container() -> Result<Container, Self>` + `Container::into_map() -> Result<LoroMap, Self>` — `:3813`, `:3636` (both derive `EnumAsInner`; return `Result<T, Self>` NOT `Option<T>` — collapse via `.ok().and_then(|c| c.into_map().ok())` before `ok_or_else`).
- `VertexEntity::hydrate_map(&LoroMap) -> Result<VertexEntity, HydrateError>` — `lorosurgeon-0.2.1/src/hydrate.rs:127` (via `#[derive(Hydrate)]` at `src/schema/vertex.rs:5`); errors route into `GrafeoLoroError::Hydrate(#[from] HydrateError)` via `From` impl at `src/error.rs` (P3T2-L2R2 M2).
- `GrafeoDB::session_with_cdc(false) -> Session` — `grafeo-engine-0.5.42/src/database/mod.rs:1728` (CDC off — outbound echoes suppressed).
- `Session::begin_transaction(&mut self) -> Result<()>` — `session/mod.rs:3883` (default isolation = `SnapshotIsolation`; write-only chunk has no read-then-write race).
- `Session::prepare_commit(&mut self) -> Result<PreparedCommit<'_>>` — `:4496`.
- `PreparedCommit::set_metadata(&mut self, impl Into<String>, impl Into<String>)` — `transaction/prepared.rs:107` (advisory only — dropped on `commit()`).
- `PreparedCommit::commit(self) -> Result<EpochId>` — `prepared.rs:124` (consumes self; `Session::Drop` auto-rollbacks un-prepared-commit'd tx at `session/mod.rs:5368-5383`).

---

## 17. Asynchronous Vector Generation & Offloading

Grafeo stores vector embeddings as `Value::Vector(Arc<[f32]>)` natively. **Never write these float vectors into the Loro CRDT.** They bloat storage and cannot be combined meaningfully (taking the union of two concurrent vector changes is mathematically nonsensical).

### The Offloaded Vector Pipeline

1.  **Loro Text Edit**: Peer edits text description in Loro container.
2.  **Network Broadcast**: Loro syncs text edits (Fugue) between nodes.
3.  **Bridge Intercept**: Bridge detects updated `description` text property on Node `X`.
4.  **Local Embedding Generation**: Run an in-process local ONNX model (e.g., MiniLM-L6) to generate a 384-dimensional float vector.
5.  **Grafeo Direct Insertion**: Insert float vector directly into Grafeo's localized HNSW index. Never push the resulting vector back to Loro.

```rust
use std::sync::Arc;
use grafeo::{GrafeoDB, Value as GValue};
use grafeo_loro::types::ids::NodeId;
use grafeo_loro::error::Result;
use grafeo_loro::constants::{DEFAULT_EMBEDDING_DIM, EMBEDDING_PROPERTY, ORIGIN_LORO_BRIDGE};

pub struct VectorOffloadManager {
    db: Arc<GrafeoDB>,
}

impl VectorOffloadManager {
    /// Detects updated text and generates local-only embeddings. Writes the
    /// resulting vector directly to Grafeo (never to Loro). Task 4 owns the
    /// body — calls `generate_local_embedding`, then upserts via Session +
    /// PreparedCommit with `session_with_cdc(false)` (CDC off suppresses
    /// outbound Grafeo→Loro echoes — bypass-Loro invariant).
    pub async fn handle_text_update(&self, node_id: NodeId, text: &str) -> Result<()> {
        // 1. Generate local float vector (deterministic dummy until ONNX lands).
        let embedding_vector: Vec<f32> = generate_local_embedding(text)?;

        // 2. Insert directly into Grafeo column and update HNSW index.
        // CDC OFF (`false`) — outbound worker must NOT see this write (bypass-Loro).
        let mut session = self.db.session_with_cdc(false);
        session.begin_transaction()?;
        // `node_id` is already `grafeo::NodeId` (re-exported at
        // `src/types/ids.rs:10`); NO wrap. `EMBEDDING_PROPERTY` is the SSOT
        // constant for the `"embedding"` literal (Phase 3 Task 4).
        session.set_node_property(
            node_id,
            EMBEDDING_PROPERTY,
            GValue::Vector(Arc::from(embedding_vector)),
        )?;
        let mut prepared = session.prepare_commit()?;
        prepared.set_metadata(ORIGIN_LORO_BRIDGE, ORIGIN_LORO_BRIDGE); // advisory only
        let _epoch = prepared.commit()?;
        // Grafeo rebuilds local HNSW index incrementally on commit.
        Ok(())
    }
}

/// Deterministic dummy embedding generator (ONNX stub). Returns a
/// `DEFAULT_EMBEDDING_DIM`-dimensional vector derived deterministically from
/// `text` (same input → byte-identical output; empty `""` → valid vector).
/// Logs `tracing::warn!("ONNX not integrated; returning deterministic dummy
/// embedding")` once per process via `std::sync::Once`. Real ONNX lands via
/// `grafeo_engine::embedding::OnnxEmbeddingModel` (Phase 6).
//
// Fully implemented in Phase 3 Task 3 — see `src/hydration/vector.rs:129-149`
// for the source. Algorithm: fold `text.bytes()` into a `u64` seed → hand-rolled
// SplitMix64 PRNG (~10 LOC, no `rand` dep) → `DEFAULT_EMBEDDING_DIM` `f32`
// samples in `[0.0, 1.0)` via the top-24-bits `u64_to_f01` formula. Deterministic,
// zero-seed-safe (empty `""` folds to seed `0`, first sample is non-zero),
// idempotent (anti-plenger #9). Once-warn guarded.
pub fn generate_local_embedding(text: &str) -> Result<Vec<f32>> {
    // … body at src/hydration/vector.rs:129-149 (Phase 3 Task 3) …
    Ok(Vec::new())
}
```

### Embedding Property SSOT (Phase 3 Task 4)

`crate::constants::EMBEDDING_PROPERTY: &str = "embedding"` (`src/constants.rs:46-55`) is the single source of truth for the Grafeo node property key under which `VectorOffloadManager::handle_text_update` stores `Value::Vector(Arc<[f32]>)`. The manager, the `tests/unit/vector_offload.rs` scaffolds, and any future `vector_search` call site (Phase 5+) all reference the SSOT constant — no inline `"embedding"` literals. Value `"embedding"` matches grafeo-engine's own example docstring at `grafeo-engine-0.5.42/src/database/index.rs:91` ("property containing vector embeddings (e.g., `\"embedding\"`)"), so it is also the convention grafeo tooling expects when auto-creating HNSW indexes. The auto-index-insert path at `crud.rs:413-426` (gated `#[cfg(feature = "vector-index")]` — transitively enabled via `grafeo` default → `embedded` → `vector-index`) fires only if an index ALREADY EXISTS for the label+property combo; callers create the index BEFORE calling `handle_text_update` (not a manager concern — anti-plenger #3 YAGNI).

### ONNX Stub Contract (Phase 3 Task 3)

Until real ONNX integration lands (Phase 6 hardening), `generate_local_embedding` is a deterministic dummy:

- **Dimension SSOT**: `crate::constants::DEFAULT_EMBEDDING_DIM: usize = 384` (`src/constants.rs:44`). Value matches `sentence-transformers/all-MiniLM-L6-v2` preset (`grafeo-engine-0.5.42/src/embedding/config.rs:18` `expected_dimensions: 384`). Forward-compatible across `MiniLmL12-v2` and `bge-small-en-v1.5` presets (also 384) — no HNSW index resize when Task 4 / Phase 5 swaps presets.
- **Determinism**: same `text` MUST yield byte-identical `Vec<f32>` across calls (anti-plenger #9 Absolute Idempotency). L3 algorithm: fold `text.bytes()` into a `u64` seed, seed a hand-rolled `SplitMix64` PRNG (~10 LOC, no `rand` dep — DEVIL Q5), emit `DEFAULT_EMBEDDING_DIM` `f32` samples in `[0.0, 1.0)`.
- **Warning log**: emits `tracing::warn!("ONNX not integrated; returning deterministic dummy embedding")` exactly ONCE per process via `std::sync::Once` (anti-plenger #8 Observability vs #10 fewest-LOC — once-guard prevents log-spam under batch embedding loops in Task 4). Once-guard placement (module-top vs function-body) is L3's call (NIT 1).
- **Sync, fallible**: `pub fn(&str) -> Result<Vec<f32>>` — no `async` (real ONNX `EmbeddingModel::embed` is sync at `grafeo-engine-0.5.42/src/embedding/mod.rs:47`); `Result`-wrapped for future ONNX-fail safety (anti-plenger #14 never simplify basics — real ONNX can fail on tokenize/infer/model-load; routing via existing `GrafeoLoroError::Config`/`Bridge` variants, no new variant).
- **Empty input**: `""` MUST still produce a valid `DEFAULT_EMBEDDING_DIM`-length vector (fold-seed of empty byte sequence → PRNG zero-state; no panic).
- **Re-export**: `pub use hydration::vector::generate_local_embedding;` at `src/lib.rs` exposes the stub at the crate root for external visibility (matches P3T1-L1 m3 + P3T2-L2 m1 precedent).

Real ONNX wiring (Phase 6) uses `grafeo_engine::embedding::{EmbeddingModel, OnnxEmbeddingModel}` (`grafeo-engine-0.5.42/src/embedding/mod.rs:39` `pub trait EmbeddingModel: Send + Sync` — sync; `mod.rs:47` `fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>` — sync, batched, fallible; `mod.rs:50` `fn dimensions(&self) -> usize`). grafeo-engine also ships a `MockEmbeddingModel` at `mod.rs:62-93` (`#[cfg(test)]`-private) with a similar fold-seed-derive pattern — algorithm reference only, not reusable from grafeo-loro.

### Validation (Phase 3 Task 4 — bypass-Loro test contract)

The spec validation gate (`docs/implementation-plan.md` Phase 3 Task 4): **"Vector never written to Loro container."** Four `#[ignore]`'d test scaffolds live at `tests/unit/vector_offload.rs` (registered in `tests/unit/main.rs`):

1. **`vector_offload_writes_embedding_to_grafeo`** — happy-path: create a node via `Session::create_node`, call `handle_text_update(node_id, "hello world")`, assert `node.get_property(EMBEDDING_PROPERTY)` returns `Some(&Value::Vector(arc))` with `arc.len() == DEFAULT_EMBEDDING_DIM`. Does NOT assert searchability (no `create_vector_index` call — Task 4 spec gate is "bypass Loro", NOT "searchable").
2. **`vector_offload_never_writes_to_loro`** — **CRITICAL SPEC GATE**: instantiate a FRESH `LoroDoc::new()` AND a FRESH `GrafeoDB::new_in_memory()` with **NO `SyncEngine` connecting them** (the manager does NOT hold a `LoroDoc` reference — verified; only `Arc<GrafeoDB>` field). Call `handle_text_update`, walk `doc.get_deep_value()` recursively, assert NO `LoroValue::List` of `DEFAULT_EMBEDDING_DIM` `LoroValue::Double(_)` elements appears anywhere in the tree. Cross-check: the Grafeo node DOES have the embedding (proves the bypass went Grafeo-ward, not nowhere). Anti-tautology: also asserts the doc is still effectively empty.
3. **`vector_offload_is_idempotent`** — same `text` twice → byte-identical `Vec<f32>` on readback (`assert_eq!` on the FULL vector, not just length).
4. **`vector_offload_different_texts_different_embeddings`** — different texts on the same node → different vectors on readback (`assert_ne!` on the FULL `Vec<f32>`).

All four bodies are `todo!()` at L1; L3 fills them. Scaffolds match P3T1-L1 + P3T2-L1 + P3T3-L1 precedent (`#[ignore]` until L3).

---

## 18. Post-Sync Hybrid Query

Once the sync pipeline and parallel indexing finish, Grafeo can execute complex hybrid query plans (combining text, vector, and graph structure) instantly. Loro remains completely oblivious to this high-performance query execution layer.

```rust
// Run a GQL hybrid search query directly against the materialized GrafeoDB
let result = db.execute(r#"
    MATCH (d:Document)
    WHERE cosine_similarity(d.embedding, vector([0.15, 0.75, 0.35, 0.55])) > 0.85
    MATCH (d)-[:KNOWS*1..3]->(recipient:Person)
    RETURN d.title, recipient.name
"#).unwrap();

for row in result.rows() {
    println!("{:?}", row);
}
```

---

## 19. Non-Blocking MVCC & Snapshot Isolation

Grafeo utilizes Multi-Version Concurrency Control (MVCC) with Snapshot Isolation (SI).

*   **Zero Reader Blocking**: Long-running read queries (e.g., GQL traversals, Louvain algorithms, HNSW searches) do not lock database tables. Readers acquire a snapshot corresponding to a specific epoch.
*   **Zero Writer Blocking**: Inbound collaboration updates are committed as new epochs. Writers run concurrently using Block-STM without waiting for active read queries to finish.
*   **Consistency Guarantee**: Active queries see a frozen, consistent snapshot of the collaborative graph. Subsequent queries instantly acquire the newly merged epoch.

```text
Time Line ------------------------------------------------------------------------------------>

[Reader 1 (PageRank)]  |=== Active Epoch 42 ===| (No Locks held)
[Loro Sync Thread]       |-- Commit Epoch 43 (Merge Remote Edits) --| (Block-STM Write)
[Reader 2 (GQL Query)]                           |=== Active Epoch 43 ===|
```

---

## 20. Inbound Mutation Batcher

Applying every single keystroke or cursor move as a persistent Grafeo write transaction murders throughput.

`LoroGrafeoBridge` uses a **time-and-count-based batcher** to collect incoming Loro changes, committing them in optimized, vectorized blocks.

> **Note (Devil BLOCKER B1, MAJOR M7)**: The pseudocode below is
> illustrative. The actual grafeo 0.5.42 API is `Session`-based (no
> `begin_write_tx()`), and `LoroOp::UpsertNode` carries a Loro-side string
> key (`loro_key`) rather than a numeric `id` because grafeo has no
> upsert-by-external-id. The bridge maintains a `loro_key → grafeo::NodeId`
> map in `SyncEngine` and translates at apply time via
> `bridge::grafeo_tx::apply_loro_op`.

```rust
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration, Instant};
use grafeo::GrafeoDB;

pub struct MutationBatcher {
    db: Arc<GrafeoDB>,
}

impl MutationBatcher {
    pub async fn start_batch_loop(&self, mut rx: mpsc::Receiver<LoroOp>, batch_ms: u64, max_batch_size: usize) {
        let mut buffer = Vec::with_capacity(max_batch_size);
        let mut ticker = interval(Duration::from_millis(batch_ms));

        loop {
            tokio::select! {
                // 1. Consume incoming Loro mutation events
                Some(op) = rx.recv() => {
                    buffer.push(op);
                    if buffer.len() >= max_batch_size {
                        self.flush_batch(&mut buffer);
                    }
                }
                // 2. Timeout interval reached (e.g., 100ms passed since last write)
                _ = ticker.tick() => {
                    if !buffer.is_empty() {
                        self.flush_batch(&mut buffer);
                    }
                }
            }
        }
    }

    fn flush_batch(&self, buffer: &mut Vec<LoroOp>) {
        // Session + PreparedCommit API (illustrative).
        let mut session = self.db.session_with_cdc(true);
        session.begin_transaction().unwrap();

        // Execute batch vectorized insertion via apply_loro_op, which
        // consults the loro_key → grafeo::NodeId map for identity.
        for op in buffer.drain(..) {
            match op {
                LoroOp::UpsertNode { loro_key, labels, properties } => {
                    // apply_loro_op(&session, &op, &node_id_map)
                    //   → if loro_key in map: set_node_property for each prop
                    //   → else: create_node_with_props + insert into map
                }
                LoroOp::DeleteNode { loro_key } => {
                    // apply_loro_op → look up NodeId, session.delete_node(id)
                }
            }
        }

        let mut prepared = session.prepare_commit().unwrap();
        prepared.set_metadata("origin", "loro-bridge"); // advisory only
        let epoch = prepared.commit().unwrap(); // -> EpochId
        // Insert `epoch` into bridge_origin_epochs (echo prevention).
    }
}

pub enum LoroOp {
    UpsertNode {
        loro_key: String,
        labels: Vec<String>,
        properties: std::collections::HashMap<String, grafeo::Value>,
    },
    DeleteNode { loro_key: String },
}
```

---

## 21. Read-Your-Own-Writes Consistency

While remote updates are batched to preserve performance, local user edits must reflect in local queries immediately.

*   **The Path**:
    1.  Local user types character.
    2.  Local UI intercepts keystroke. Writes to Loro *and* spawns a synchronous, lightweight local-only Grafeo write transaction.
    3.  Local transaction bypasses the async batcher, immediately incrementing the local Grafeo epoch.
    4.  Local read queries instantly reflect the user's input.
*   **Remote Path**:
    1.  Remote peer edits broadcast over WebSocket.
    2.  These updates enter the `MutationBatcher` queue, merging asynchronously into Grafeo every 100ms.

---

## 22. Concurrent Write Scaling via Block-STM

When multiple remote updates arrive concurrently, Grafeo's **Block-STM** execution engine partitions the transaction execution.

1.  Updates are executed speculatively in parallel across the Thread Pool.
2.  If two operations mutate the same memory block (dependency conflict), the conflict is auto-detected.
3.  The lower-priority transaction is aborted, rolled back, and re-executed instantly.
4.  Provides multi-threaded writing speed during high-concurrency collaborative editing spikes without risking database locking bottlenecks.

---

## 23. Observability

`grafeo-loro` exposes structured telemetry across three pillars: **metrics**, **tracing**, and **health checks**. All signals are emitted via the `opentelemetry` Rust SDK and can be exported to Prometheus, Jaeger, or any OTLP-compatible backend.

### 23.1 Metric Dimensions

| Metric Name | Type | Labels | Description |
|-------------|------|--------|-------------|
| `grafeo_loro.sync.inbound_events_total` | Counter | `origin`, `event_type` | Total Loro events processed by the inbound worker |
| `grafeo_loro.sync.outbound_events_total` | Counter | `origin`, `event_type` | Total CDC events processed by the outbound worker |
| `grafeo_loro.sync.echo_filtered_total` | Counter | `direction` | Events dropped by origin tracking (echo prevention) |
| `grafeo_loro.sync.batch_flush_duration_ms` | Histogram | `batch_size` | Time to commit a batched Grafeo transaction |
| `grafeo_loro.sync.hydration_duration_ms` | Histogram | `mode` (`loro`/`grafeo`) | Cold-start hydration wall-clock time |
| `grafeo_loro.grafeo.query_duration_ms` | Histogram | `query_type` | GQL / HNSW / traversal execution time |
| `grafeo_loro.grafeo.epoch_number` | Gauge | — | Current Grafeo MVCC epoch |
| `grafeo_loro.grafeo.active_readers` | Gauge | — | Number of concurrent snapshot readers |
| `grafeo_loro.compression.ratio` | Gauge | `algorithm` (`lz4`/`zstd`) | Compression ratio for last snapshot |
| `grafeo_loro.bridge.queue_depth` | Gauge | `direction` (`inbound`/`outbound`) | Current MPSC channel depth |
| `grafeo_loro.presence.peers_connected` | Gauge | `room_id` | Active ephemeral WebSocket peers |

### 23.2 Span Hierarchy

```text
grafeo_loro_session
├── span: cold_start_hydration
│   ├── span: decompress_snapshot
│   ├── span: import_loro_doc
│   └── span: parallel_hydrate_grafeo
│       └── span: hydrate_chunk (one per rayon chunk)
├── span: inbound_sync_loop
│   ├── span: receive_loro_event
│   ├── span: batch_flush (every N ms or max_batch_size)
│   │   └── span: grafeo_commit
│   └── span: index_rebuild
├── span: outbound_sync_loop
│   ├── span: receive_cdc_event
│   └── span: loro_commit
├── span: user_mutation
│   ├── span: local_grafeo_write (RYOW path)
│   └── span: local_loro_commit
└── span: hybrid_query
    ├── span: hnsw_search
    └── span: graph_traversal
```

### 23.3 Health Check Endpoint

```rust
use std::sync::Arc;
use parking_lot::RwLock;

pub struct HealthProbe {
    doc: Arc<RwLock<LoroDoc>>,
    db: Arc<GrafeoDB>,
    last_sync_ts: AtomicU64,
}

impl HealthProbe {
    /// Returns 200 OK if:
    /// - LoroDoc is not poisoned (can acquire read lock)
    /// - GrafeoDB can execute a dummy query
    /// - Last sync occurred within `max_staleness_ms`
    pub fn check(&self, max_staleness_ms: u64) -> HealthStatus {
        let loro_ok = self.doc.try_read().is_some();
        let grafeo_ok = self.db.execute("MATCH (n) RETURN count(n) LIMIT 1").is_ok();
        let now = unix_timestamp_ms();
        let sync_ok = now - self.last_sync_ts.load(Ordering::Relaxed) < max_staleness_ms;

        HealthStatus {
            overall: loro_ok && grafeo_ok && sync_ok,
            components: vec![
                ("loro_doc", loro_ok),
                ("grafeo_db", grafeo_ok),
                ("sync_freshness", sync_ok),
            ],
        }
    }
}
```

### 23.4 Structured Logging

All bridge events log at `INFO` level with structured JSON:

```json
{
  "timestamp": "2026-07-05T19:37:00Z",
  "level": "INFO",
  "target": "grafeo_loro::bridge",
  "event": "batch_flush",
  "batch_size": 47,
  "duration_ms": 12,
  "epoch_advanced_to": 1284,
  "origin_skipped": 0
}
```

Critical warnings:
*   `WARN` — Echo loop detected despite origin tracking (indicates metadata corruption).
*   `WARN` — Batch flush exceeded `batch_ms` threshold (backpressure signal).
*   `ERROR` — Block-STM abort rate > 10% (contention spike).
*   `ERROR` — Loro import failed (potential CRDT corruption).

### 23.5 Alerting Rules (Prometheus)

```yaml
- alert: GrafeoLoroHighEchoFilterRate
  expr: rate(grafeo_loro_sync_echo_filtered_total[5m]) > 100
  for: 2m
  labels:
    severity: warning
  annotations:
    summary: "High echo filter rate — possible origin tracking bug"

- alert: GrafeoLoroHydrationStall
  expr: grafio_loro_sync_hydration_duration_ms > 30000
  for: 0m
  labels:
    severity: critical
  annotations:
    summary: "Cold-start hydration exceeded 30s"

- alert: GrafeoLoroBlockStmContention
  expr: rate(grafeo_loro_grafeo_blockstm_aborts_total[5m]) / rate(grafeo_loro_grafeo_blockstm_commits_total[5m]) > 0.1
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "Block-STM abort rate > 10% — consider reducing batch concurrency"
```

---

## 24. Installation & Usage

### 24.1 Cargo.toml

```toml
[dependencies]
grafeo-loro = "0.1"
grafeo = "0.5"
loro = "1.0"
lorosurgeon = "0.2"
tokio = { version = "1", features = ["full"] }
parking_lot = "0.12"
rayon = "1.8"
lz4_flex = "0.11"
zstd = "0.13"

# Optional: observability
opentelemetry = "0.23"
opentelemetry-prometheus = "0.16"

# Optional: local embeddings
ort = "2.0"  # or tract-onnx
```

### 24.2 Quick Start Example

```rust
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use grafeo_loro::{GrafeoLoroApp, SsotMode, StorageBackend, CompressionType};
use loro::LoroDoc;
use grafeo::GrafeoDB;

// 1. Implement your own storage backend (filesystem, S3, etc.)
struct FileStorage {
    dir: String,
}

impl StorageBackend for FileStorage {
    async fn load(&self, key: &str) -> Result<Vec<u8>, std::io::Error> {
        tokio::fs::read(format!("{}/{}", self.dir, key)).await
    }

    async fn save(&self, key: &str, bytes: Vec<u8>) -> Result<(), std::io::Error> {
        tokio::fs::write(format!("{}/{}", self.dir, key), bytes).await
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>, std::io::Error> {
        // Return keys matching prefix
        Ok(vec![])
    }

    async fn delete(&self, key: &str) -> Result<(), std::io::Error> {
        tokio::fs::remove_file(format!("{}/{}", self.dir, key)).await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 2. Initialize storage
    let storage = Arc::new(FileStorage { dir: "./data".to_string() });

    // 3. Create the dual-store application
    let app = GrafeoLoroApp::builder()
        .storage(storage)
        .ssot_mode(SsotMode::Loro)          // or SsotMode::Grafeo
        .compression(CompressionType::Zstd)   // Zstd for snapshots, LZ4 for sync
        .batch_interval_ms(100)
        .batch_max_size(256)
        .build()
        .await?;

    // 4. Cold-start hydration (loads from storage, decompresses, hydrates both stores)
    app.hydrate("graph_123").await?;

    // 5. Insert a vertex (local-first: Grafeo instantly, Loro async via bridge)
    let node_id = app.create_vertex()
        .with_label("Person")
        .with_property("name", "Alice")
        .with_property("age", 30)
        .commit()?;

    // 6. The vertex is immediately queryable (RYOW)
    let result = app.query(r#"
        MATCH (p:Person {name: "Alice"})
        RETURN p.age
    "#)?;
    println!("Alice's age: {:?}", result.rows().next());

    // 7. Collaborative text edit (Fugue-managed via Loro)
    app.update_text(node_id, "description", "Alice is a software engineer.").await?;

    // 8. Generate local embedding (offloaded to Grafeo, never touches Loro)
    app.generate_embedding(node_id, "description").await?;

    // 9. Hybrid vector + graph query
    let similar = app.query(r#"
        MATCH (d:Document)
        WHERE cosine_similarity(d.embedding, vector([0.15, 0.75, 0.35, 0.55])) > 0.85
        MATCH (d)-[:KNOWS*1..3]->(recipient:Person)
        RETURN d.title, recipient.name
    "#)?;

    // 10. Export compressed snapshot for storage
    app.checkpoint("graph_123").await?;

    // 11. Real-time presence (ephemeral, not persisted)
    app.broadcast_presence(PresencePayload {
        peer_id: 42,
        active_node: Some(node_id.to_string()),
        cursor_x: 120.5,
        cursor_y: 340.0,
        last_active_ts: unix_timestamp_ms(),
    }).await?;

    // 12. Graceful shutdown
    app.shutdown().await?;
    Ok(())
}
```

### 24.3 Storage Backend Trait

```rust
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync + 'static {
    /// Load raw bytes from storage. The caller handles decompression.
    async fn load(&self, key: &str) -> Result<Vec<u8>, std::io::Error>;

    /// Save raw bytes to storage. The caller handles compression.
    async fn save(&self, key: &str, bytes: Vec<u8>) -> Result<(), std::io::Error>;

    /// List keys matching a prefix (for delta enumeration in Loro SSOT mode).
    async fn list(&self, prefix: &str) -> Result<Vec<String>, std::io::Error>;

    /// Delete a key from storage.
    async fn delete(&self, key: &str) -> Result<(), std::io::Error>;
}
```

Implement this trait for S3, GCS, Azure Blob, IPFS, or any custom backend. The architecture is fully storage-agnostic.

### 24.4 Configuration Reference

| Parameter | Default | Description |
|-----------|---------|-------------|
| `ssot_mode` | `SsotMode::Loro` | Which engine owns the canonical state |
| `compression` | `CompressionType::Zstd` | Compression for cold snapshots |
| `sync_compression` | `CompressionType::Lz4` | Compression for hot sync packets |
| `batch_interval_ms` | `100` | Max time to buffer inbound Loro events |
| `batch_max_size` | `256` | Max events per Grafeo batch flush |
| `hydration_chunk_size` | `256` | Rayon chunk size for parallel index rebuild |
| `max_staleness_ms` | `5000` | Health check threshold for sync freshness |
| `enable_presence` | `true` | Enable ephemeral WebSocket presence |
| `presence_heartbeat_ms` | `30000` | Presence heartbeat interval |
| `grafeo_dir` | `None` (in-memory) | Path to on-disk GrafeoDB directory (required for `SsotMode::Grafeo` and production persistence). `None` → in-memory `GrafeoDB::new_in_memory()`. Added in Phase 4 (P4-DEVIL Q5/M9). |

---

## 25. Phase 4 Deviations

This section records deviations from the original Phase 4 plan documented in
`docs/implementation-plan.md`. Phase 4 ships a working `SsotMode::Loro` cold
boot → mutate → checkpoint → cold boot cycle. `SsotMode::Grafeo` is deferred
to Phase 5.

### 25.1 `SsotMode::Grafeo` cold-start path deferred to Phase 5 (P4-DEVIL M7)

Architecture §4 Step A only specifies the Loro direction (Loro → Grafeo via
`parallel_hydrate_grafeo`). The Grafeo direction (download tar.zst → extract
→ restore DB → rebuild LoroDoc from Grafeo state via `parallel_hydrate_loro`)
is unspecified in §4. P4-DEVIL M7 flagged this as a doc gap; P4-DEVIL Q2
decision (option (d)) recommends deferring `SsotMode::Grafeo` to Phase 5
based on three combined costs:

1. **B1**: `GrafeoDB::open(path)` is `#[cfg(feature = "wal")]`-gated
   (`grafeo-engine-0.5.42/src/database/mod.rs:289`). Phase 4's
   `Cargo.toml` uses `grafeo = "0.5"` with default features only
   (`embedded` does NOT activate `wal`). The literal `GrafeoDB::open(path)`
   call would not compile.
2. **B2**: `SyncEngine.grafeo_db: Arc<GrafeoDB>` is immutable
   (`src/bridge/sync_engine.rs:97`). After `SsotMode::Grafeo` hydrate
   restores an on-disk DB, there is no way to rebind the new `Arc<GrafeoDB>`
   into the existing `SyncEngine`. Phase 5 must refactor the field type to
   `Arc<RwLock<Arc<GrafeoDB>>>` or `arc_swap::ArcSwap<GrafeoDB>` (~30 call
   sites).
3. **M3**: `GrafeoDB::close(&self)` takes `&self` (NOT `self`) — it flushes
   the WAL + file_manager and sets `is_open = false`, but the `Arc<GrafeoDB>`
   handle remains in memory. The tar-of-directory + `close()` + reopen path
   is therefore broken without B2.

Phase 4 implementation: `hydrate`/`checkpoint` `SsotMode::Grafeo` arms
return `unimplemented!("P5: requires wal feature + ArcSwap grafeo_db field
— see P4-DEVIL Q2/B1/B2")`. The validation test runs in `SsotMode::Loro`
only.

Phase 5 plan (when S3 backend from Task 1 lands):

1. `Cargo.toml`: `grafeo = { version = "0.5", features = ["wal"] }` + add
   `tar = "0.4"`.
2. `SyncEngine.grafeo_db` field type → `ArcSwap<GrafeoDB>` (or
   `Arc<RwLock<Arc<GrafeoDB>>>`). Update ~30 call sites.
3. `checkpoint` (Grafeo arm): use non-destructive `GrafeoDB::backup_full(
   &backup_dir)` (takes `&self`, does NOT close — verified at
   `grafeo-engine-0.5.42/src/database/mod.rs:2743`, gated
   `#[cfg(all(feature = "wal", feature = "grafeo-file", feature = "lpg"))]`
   — all three features are unlocked by adding `wal` since `embedded`
   already activates `grafeo-file` + `lpg`).
4. `hydrate` (Grafeo arm): download → `zstd::decode_all` → `tar::unpack` →
   `GrafeoDB::restore_to_epoch(&extracted_dir, EpochId::MAX, &output_path)`
   (verified at `:2813`, gated `#[cfg(all(feature = "wal", feature =
   "grafeo-file"))]`) → `GrafeoDB::with_config(Config::persistent(
   output_path))` → `ArcSwap::store` the new `Arc<GrafeoDB>` into
   `SyncEngine.grafeo_db`.
5. Implement `parallel_hydrate_loro` in `src/hydration/parallel.rs` — mirror
   of `parallel_hydrate_grafeo` using `graph_store().node_ids()` +
   `entity.reconcile(RootReconciler::new(node_map))` per vertex. Wrap each
   `doc.commit()` with `doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE)` to
   route through the B1 filter (echo prevention — P4-DEVIL M6).
6. Architecture §4 Step A: add a `SsotMode::Grafeo` subsection documenting
   the cold-start path (out of P4-L2 scope — file doc update for Phase 5).

### 25.2 `grafeo_dir` config row added to §24.4 (P4-DEVIL M9)

Architecture §24.4 originally listed 9 config parameters. P4-DEVIL Q5/M9
flagged the absence of `grafeo_dir` — production `GrafeoDB::with_config(
Config::persistent(path))` requires a directory path, and the builder needs
a setter for it. Phase 4 adds the row above. The `grafeo_dir` setter is
`pub fn grafeo_dir(self, path: impl Into<PathBuf>) -> Self` on
`GrafeoLoroAppBuilder`. `build()` rejects `SsotMode::Grafeo + grafeo_dir ==
None` with `Config("grafeo_dir required for SsotMode::Grafeo")`.

---

*End of Architecture Document*
