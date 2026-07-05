# Architecture: `grafeo-loro` (Part 1/5)

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
|                                Cloud S3 Bucket                                    |
|                                                                                   |
|   s3://bucket/graphs/<graph_id>/state.loro (Loro SSOT mode)                      |
|   s3://bucket/graphs/<graph_id>/backup.tar.zst (Grafeo SSOT mode)                 |
+-----------------------------------------------------------------------------------+
 
## 1.1 Switchable SSOT Configurations

```rust
pub enum SsotMode {
    Loro,   // S3 stores .loro. Grafeo is ephemeral query cache.
    Grafeo, // S3 stores backup.tar.zst. Loro is ephemeral merge engine.
}
```

### Mode Selection Guide

| Feature / Constraint | `SsotMode::Loro` | `SsotMode::Grafeo` |
| :--- | :--- | :--- |
| **Primary S3 Artifact** | `.loro` (Binary CRDT snapshot) | `.tar.zst` (Compressed database folder) |
| **Time Travel Capabilities** | Yes (via Loro native frontiers) | No (CRDT history discarded on session teardown) |
| **Vector / HNSW Indexes** | Regenerated locally on cold boot | Saved directly (no network regeneration) |
| **Cloud Storage Size** | Minimal (RLE compressed, history-trimmed) | Heavy (Contains binary indices, zone maps) |
| **Boot Speed Pattern** | Slow DB index rebuild, fast S3 download | Instant DB attach, slow S3 download |
| **Schema Evolution** | Decoupled client-side migrations | Fast-path native Grafeo transactions |

## 2. Component Roles & Boundaries

### LoroDoc (Consensus Layer)
*   **Role**: Authoritative single source of truth (SSOT) for document state, history, and network merges [1.1.1].
*   **Memory**: High-efficiency RLE (Run-Length Encoding) operations log DAG.
*   **Attributes**: Manages Lamport clocks, peer IDs, frontiers, and conflict-free concurrent editing resolution [1.1.1, 1.2.1].

### GrafeoDB (Execution Layer)
*   **Role**: Materialized view optimized for local runtime queries, analytics, and indexing.
*   **Memory**: Columnar blocks, Compressed Sparse Row (CSR) adjacency indexes, HNSW vector indexes, and BM25 text inverted index.
*   **Attributes**: Parallel push-based vectorized execution, morsel-driven thread scaling.

### LoroGrafeoBridge (Glue Layer)
*   **Role**: In-process bidirectional sync manager.
*   **Memory**: Multi-thread ownership locks and synchronous transaction buffers.
*   **Attributes**: Converts `LoroEvent` diffs to Grafeo direct database updates. Converts Grafeo `CdcEvent` streams to Loro mutations.

### S3 Storage (Backup Layer)
*   **Role**: Static, append-only, serverless coordination storage.
*   **Memory**: Cloud object storage storing only platform-agnostic `.loro` binary blobs.
*   **Attributes**: No running servers, zero databases in cloud, zero CPU overhead.

---

## 3. Local-First Lifecycle Flow

### Step A: Cold Startup
1.  Client process launches.
2.  Fetches `state.loro` (shallow snapshot with zero redundant history) from S3.
3.  Hydrates local memory `LoroDoc` using `doc.import_with_status(&bytes)`.
4.  `LoroGrafeoBridge` reads final state of `LoroDoc`, iterates through active containers, and populates local in-memory or on-disk `GrafeoDB` cache.

### Step B: Offline Mutation
1.  User modifies graph offline (e.g., adds node, changes property).
2.  Grafeo local database applies change instantly (<1ms). UI redraws.
3.  Grafeo emits `CdcEvent`.
4.  `LoroGrafeoBridge` consumes `CdcEvent`, locks `LoroDoc`, and applies equivalent mutation within `LoroDoc::transact_mut()`.
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
3.  Uploads `.loro` payload to S3, overwriting old file. History discarded to prevent storage bloat.


# Architecture: `grafeo-loro` (Part 2/5)

## 1. Root Container Schema

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

## 2. Declarative Mapping via `lorosurgeon`

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

## 3. Ordered Sequences & Movable Trees

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

# Architecture: `grafeo-loro` (Part 3/5)

## 1. Concurrency & Deadlock Prevention

Both engines share one OS process. To prevent deadlocks during bidirectional synchronization:
*   **Decoupled Writing**: Do not perform synchronous write loops inside event callbacks. 
*   **Execution Locks**: `LoroDoc` runs inside `parking_lot::RwLock`. `GrafeoDB` manages internal lock-free reader threads and parallel writer queues.
*   **Async Buffering**: Use thread-safe `tokio::sync::mpsc` channels to offload mutations from synchronous callbacks into async worker loops.

```text
[Loro Thread] ──(Sync Callback)──> Push to MPSC ──> [Tokio Thread Pool] ──> Write to GrafeoDB
[Grafeo Worker] ──(CDC Event)─────> Push to MPSC ──> [Tokio Thread Pool] ──> Write to LoroDoc
```

---

## 2. Echo Feedback Loop Prevention

Bidirectional sync creates feedback loops where an update echoed back replicates infinitely.

```text
Loro Update ──> Bridge ──> Grafeo Write ──> Grafeo CDC ──> Bridge ──> Loro Write (Loop!)
```

### Solution: Origin Tracking

1.  **Loro-to-Grafeo Skip**:
    *   Set Loro transaction origin during bridge mutations using `doc.set_next_commit_origin("grafeo-bridge")`.
    *   In the Loro subscription handler, inspect `event.origin`. If it equals `"grafeo-bridge"`, discard the event.
2.  **Grafeo-to-Loro Skip**:
    *   When the bridge executes queries in Grafeo, attach transaction metadata: `tx.set_metadata("origin", "loro-bridge")`.
    *   In the Grafeo CDC listener loop, inspect the transaction origin. If it equals `"loro-bridge"`, ignore the event.

---

## 3. Rust Event Loop & Origin Processing

Below is the concrete, thread-safe Rust synchronization engine.

```rust
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use loro::{LoroDoc, LoroValue};
use grafeo::{GrafeoDB, cdc::CdcEvent};

pub struct SyncEngine {
    db: Arc<GrafeoDB>,
    doc: Arc<RwLock<LoroDoc>>,
    // Bridge-internal worker channel
    inbound_tx: mpsc::Sender<loro::event::Event>,
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
        let mut doc = self.doc.write();

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
    fn spawn_inbound_worker(self: &Arc<Self>, mut rx: mpsc::Receiver<loro::event::Event>) {
        let db = self.db.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                // Begin high-speed Grafeo transaction
                let mut db_tx = db.begin_write_tx();
                
                // Set origin metadata to prevent echo loops
                db_tx.set_metadata("origin", "loro-bridge");

                for diff in &event.events {
                    // Extract diff target container and map values
                    // db_tx.upsert_node(...) / db_tx.delete_node(...)
                }

                db_tx.commit().unwrap();
            }
        });
    }

    /// 3. Outbound Worker (Grafeo -> Loro)
    pub fn spawn_outbound_worker(self: &Arc<Self>, mut cdc_rx: mpsc::Receiver<CdcEvent>) {
        let doc_lock = self.doc.clone();
        tokio::spawn(async move {
            while let Some(event) = cdc_rx.recv().await {
                // Drop CDC events originating from our own inbound worker
                if event.transaction_metadata.get("origin").map(|s| s.as_str()) == Some("loro-bridge") {
                    continue;
                }

                let mut doc = doc_lock.write();
                
                // Identify origin to prevent echo
                doc.set_next_commit_origin("grafeo-bridge");
                let mut txn = doc.transact_mut();

                match event {
                    CdcEvent::NodeInserted { id, label, properties } => {
                        let v_root = doc.get_map("V");
                        if let Ok(node_map) = v_root.insert_map(&mut txn, id.to_string()) {
                            // Map values into transaction
                        }
                    }
                    _ => {}
                }
                txn.commit().unwrap();
            }
        });
    }
}
```

# Architecture: `grafeo-loro` (Part 4/5)

## 1. Shallow Snapshotting (Git-style Truncation)

To prevent S3 storage and network payload bloat from long-running collaborative histories, `grafeo-loro` truncates old history using **Shallow Snapshot Encoding**.

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

## 2. In-Memory Ephemeral Presence (WebSocket Wire Format)

Real-time presence (active nodes, select highlights, mouse cursors) is kept ephemeral. It is never written to the CRDT document or saved to S3.

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

## 3. S3 Persistence Protocol

The S3 sync lifecycle balances lightweight saves and cold starts using snapshots and append-only logs.

```rust
use std::sync::Arc;
use parking_lot::RwLock;
use loro::{LoroDoc, ExportMode, VersionVector};

pub struct S3SyncManager {
    doc: Arc<RwLock<LoroDoc>>,
    graph_id: String,
    db: Arc<GrafeoDB>,
}

impl S3SyncManager {
    /// 1. Switchable Cold Start Hydration
    pub async fn hydrate_from_s3(&self, mode: SsotMode) -> Result<(), Box<dyn std::error::Error>> {
        match mode {
            SsotMode::Loro => {
                let mut doc = self.doc.write();
                if let Ok(base_bytes) = download_s3_object(&format!("{}/base.loro", self.graph_id)).await {
                    doc.import_with_status(&base_bytes).unwrap();
                }
                let deltas = list_s3_deltas(&format!("{}/deltas/", self.graph_id)).await?;
                for delta_key in deltas {
                    let delta_bytes = download_s3_object(&delta_key).await?;
                    doc.import_with_status(&delta_bytes).unwrap();
                }
                // Rebuild Grafeo indexes locally from CRDT state
                parallel_hydrate_grafeo(&self.db, &doc);
            }
            SsotMode::Grafeo => {
                let tar_path = format!("/tmp/{}.tar.zst", self.graph_id);
                let backup_bytes = download_s3_object(&format!("{}/backup.tar.zst", self.graph_id)).await?;
                std::fs::write(&tar_path, backup_bytes)?;
                
                extract_tar_zst(&tar_path, "/tmp/db");
                GrafeoDB::restore_to_epoch("/tmp/db", 0, "./mydb").unwrap();
                
                // Hydrate blank ephemeral Loro doc from local Grafeo state
                hydrate_loro_from_grafeo(&self.db, &self.doc.read());
            }
        }
        Ok(())
    }

    /// 2. Session End: Switchable teardown of ephemeral engine
    pub async fn checkpoint_session(&self, mode: SsotMode) {
        match mode {
            SsotMode::Loro => {
                let doc = self.doc.read();
                let current_frontiers = doc.state_frontiers();
                let shallow_snapshot_bytes = doc.export(ExportMode::ShallowSnapshot(&current_frontiers)).unwrap();
                upload_s3_object(&format!("{}/base.loro", self.graph_id), shallow_snapshot_bytes).await;
                clear_s3_folder(&format!("{}/deltas/", self.graph_id)).await;
            }
            SsotMode::Grafeo => {
                self.db.backup_full("/tmp/grafeo_backup").unwrap();
                let tar_path = format!("/tmp/{}.tar.zst", self.graph_id);
                compress_tar_zst("/tmp/grafeo_backup", &tar_path);
                upload_s3_object(&format!("{}/backup.tar.zst", self.graph_id), std::fs::read(&tar_path).unwrap()).await;
            }
        }
    }
}

// Dummy definitions for S3 mock boundaries
async fn download_s3_object(_key: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> { Ok(vec![]) }
async fn list_s3_deltas(_prefix: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> { Ok(vec![]) }
async fn upload_s3_object(_key: &str, _bytes: Vec<u8>) {}
async fn clear_s3_folder(_prefix: &str) {}
```

# Architecture: `grafeo-loro` (Part 5/5)

## 1. Loro 1.0 Document Size Trade-Off

Loro 1.0 optimizes for raw importing and parsing speed (10x-100x faster than traditional CRDTs). 

Trade-off:
*   Without compression, a Loro 1.0 snapshot is roughly **twice the size** of alternative CRDT formats.
*   It encodes both historical operations and current document states explicitly within the binary layout, avoiding runtime reconstruction decompression inside the CRDT core.
*   Loro delegates payload compression to the host application.

To minimize S3 storage costs and network transit, `grafeo-loro` implements a dual-layer compression pipeline in Rust.

---

## 2. Dual-Layer Compression Pipeline

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
*   **Target**: Checkpointed shallow snapshots (`.loro` stored in S3).
*   **Performance**: High compression ratio. Shrinks document size by >60%, neutralizing Loro 1.0's state-duplication storage penalty.

---

## 3. Compression Wrapper Implementation (Rust)

```rust
use std::io::{Read, Write};
use loro::{LoroDoc, ExportMode};

// Cargo.toml dependencies:
// zstd = "0.13"
// lz4_flex = "0.11"

pub enum CompressionType {
    None,
    Lz4,
    Zstd,
}

pub struct CompressedPayload {
    pub compression: CompressionType,
    pub raw_data: Vec<u8>,
}

impl CompressedPayload {
    /// 1. Compress raw exported Loro bytes
    pub fn compress(raw_bytes: &[u8], strategy: CompressionType) -> Self {
        match strategy {
            CompressionType::None => Self {
                compression: CompressionType::None,
                raw_data: raw_bytes.to_vec(),
            },
            CompressionType::Lz4 => {
                let compressed = lz4_flex::compress_prepend_size(raw_bytes);
                Self {
                    compression: CompressionType::Lz4,
                    raw_data: compressed,
                }
            }
            CompressionType::Zstd => {
                let mut encoder = zstd::stream::Encoder::new(Vec::new(), 3).unwrap();
                encoder.write_all(raw_bytes).unwrap();
                let compressed = encoder.finish().unwrap();
                Self {
                    compression: CompressionType::Zstd,
                    raw_data: compressed,
                }
            }
        }
    }

    /// 2. Decompress bytes back to standard Loro binary format
    pub fn decompress(&self) -> Result<Vec<u8>, std::io::Error> {
        match self.compression {
            CompressionType::None => Ok(self.raw_data.clone()),
            CompressionType::Lz4 => {
                let decompressed = lz4_flex::decompress_size_prepended(&self.raw_data)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                Ok(decompressed)
            }
            CompressionType::Zstd => {
                let mut decoder = zstd::stream::Decoder::new(&self.raw_data[..]).unwrap();
                let mut decompressed = Vec::new();
                decoder.read_to_end(&mut decompressed).unwrap();
                Ok(decompressed)
            }
        }
    }
}

pub trait LoroDocCompressionExt {
    fn export_compressed(&self, mode: ExportMode, strategy: CompressionType) -> CompressedPayload;
    fn import_compressed(&mut self, payload: &CompressedPayload) -> Result<(), loro::LoroError>;
}

impl LoroDocCompressionExt for LoroDoc {
    fn export_compressed(&self, mode: ExportMode, strategy: CompressionType) -> CompressedPayload {
        let raw_bytes = self.export(mode).unwrap();
        CompressedPayload::compress(&raw_bytes, strategy)
    }

    fn import_compressed(&mut self, payload: &CompressedPayload) -> Result<(), loro::LoroError> {
        let decompressed_bytes = payload.decompress()
            .map_err(|_| loro::LoroError::DecodeError("Compression failure".into()))?;
        self.import_with_status(&decompressed_bytes)?;
        Ok(())
    }
}
```

# Architecture: `grafeo-loro` (Part 6/6)

## 1. Parallel Index Hydration Engine

When `LoroDoc` imports a compressed snapshot from S3, the local `GrafeoDB` cache begins empty. Rebuilding Grafeo's structural indexes (CSR Adjacency, HNSW Vector, and BM25 Text) from Loro raw containers must be parallelized to prevent UI thread lockups.

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
Use `rayon` to chunk Loro map collections and parallelize Grafeo transaction insertions.

```rust
use rayon::prelude::*;
use std::sync::Arc;
use loro::{LoroDoc, LoroValue};
use grafeo::{GrafeoDB, Value as GValue};

pub fn parallel_hydrate_grafeo(db: &Arc<GrafeoDB>, doc: &LoroDoc) {
    let v_root = doc.get_map("V");
    let txn = doc.transact();
    
    // Extract raw keys (Node IDs)
    let node_ids: Vec<String> = v_root.keys(&txn).collect();
    
    // Execute in parallel chunks via Rayon
    node_ids.par_chunks(256).for_each(|chunk| {
        let mut db_tx = db.begin_write_tx();
        db_tx.set_metadata("origin", "loro-bridge"); // Prevent echo loops
        
        for id_str in chunk {
            let node_id: u64 = id_str.parse().unwrap();
            
            if let Some(LoroValue::Map(node_data)) = v_root.get(&txn, id_str) {
                let mut properties = std::collections::HashMap::new();
                
                // Hydrate generic properties
                if let Some(LoroValue::Map(props)) = node_data.get("prop") {
                    for (k, v) in props.iter() {
                        properties.insert(k.to_string(), lval_to_gval(v.clone()));
                    }
                }
                
                // Hydrate collaborative description text
                if let Some(LoroValue::String(desc)) = node_data.get("description") {
                    properties.insert("description".to_string(), GValue::String(desc.to_string()));
                }

                db_tx.upsert_node(node_id, properties);
            }
        }
        
        // Block-STM parallel transaction execution
        db_tx.commit().unwrap();
    });
}
```

---

## 2. Asynchronous Vector Generation & Offloading (HNSW)

Grafeo stores vector embeddings as `Value::Vector(Arc<[f32]>)` natively [1]. Never write these float vectors into the Loro CRDT. They bloat storage and cannot be combined meaningfully (taking the union of two concurrent vector changes is mathematically nonsensical).

### The Offloaded Vector Pipeline
1.  **Loro Text Edit**: Peer edits text description in Loro container.
2.  **Network Broadcast**: Loro syncs text edits (Fugue) between nodes.
3.  **Bridge Intercept**: Bridge detects updated `description` text property on Node `X`.
4.  **Local Embedding Generation**: Run an in-process local ONNX model (e.g., MiniLM-L6) to generate a 384-dimensional float vector.
5.  **Grafeo Direct Insertion**: Insert float vector directly into Grafeo's localized HNSW index. Never push the resulting vector back to Loro.

```rust
use std::sync::Arc;
use grafeo::{GrafeoDB, Value as GValue};

pub struct VectorOffloadManager {
    db: Arc<GrafeoDB>,
}

impl VectorOffloadManager {
    /// Detects updated text and generates local-only embeddings
    pub async fn handle_text_update(&self, node_id: u64, text: &str) {
        // 1. Generate local float vector (ONNX / API)
        let embedding_vector: Vec<f32> = generate_local_embedding(text).await;
        
        // 2. Insert directly into Grafeo column and update HNSW index
        let mut tx = self.db.begin_write_tx();
        tx.set_metadata("origin", "loro-bridge");
        
        let mut props = std::collections::HashMap::new();
        props.insert("embedding".to_string(), GValue::Vector(Arc::from(embedding_vector)));
        
        tx.update_node_properties(node_id, props);
        tx.commit().unwrap(); // Grafeo rebuilds local HNSW index incrementally
    }
}

async fn generate_local_embedding(_text: &str) -> Vec<f32> {
    // Local ONNX inference pipeline (e.g., via tract or ort crate)
    vec![0.15, 0.72, -0.05, 0.33] // Placeholder
}
```

---

## 3. Post-Sync Hybrid Query (HNSW + Graph Traversals)

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

# Architecture: `grafeo-loro` (Concurrency & Live Execution)

Balancing heavy analytical queries with continuous, keystroke-level collaborative updates in a single process.

---

## 1. Non-Blocking MVCC & Snapshot Isolation

Grafeo utilizes Multi-Version Concurrency Control (MVCC) with Snapshot Isolation (SI) [1]. 

*   **Zero Reader Blocking**: Long-running read queries (e.g., GQL traversals, Louvain algorithms, HNSW searches) do not lock database tables. Readers acquire a snapshot corresponding to a specific epoch [1].
*   **Zero Writer Blocking**: Inbound collaboration updates are committed as new epochs. Writers run concurrently using Block-STM without waiting for active read queries to finish [1].
*   **Consistency Guarantee**: Active queries see a frozen, consistent snapshot of the collaborative graph. Subsequent queries instantly acquire the newly merged epoch.

```text
Time Line ------------------------------------------------------------------------------------>

[Reader 1 (PageRank)]  |=== Active Epoch 42 ===| (No Locks held)
[Loro Sync Thread]       |-- Commit Epoch 43 (Merge Remote Edits) --| (Block-STM Write)
[Reader 2 (GQL Query)]                           |=== Active Epoch 43 ===|
```

---

## 2. Inbound Mutation Batcher (Buffer & Commit)

Applying every single keystroke or cursor move as a persistent Grafeo write transaction murders throughput. 

`LoroGrafeoBridge` uses a **time-and-count-based batcher** to collect incoming Loro changes, committing them in optimized, vectorized blocks.

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
        let mut tx = self.db.begin_write_tx();
        tx.set_metadata("origin", "loro-bridge");
        
        // Execute batch vectorized insertion
        for op in buffer.drain(..) {
            match op {
                LoroOp::UpsertNode { id, properties } => {
                    tx.upsert_node(id, properties);
                }
                LoroOp::DeleteNode { id } => {
                    tx.delete_node(id);
                }
            }
        }
        
        tx.commit().unwrap(); // Advances Grafeo epoch in atomic transaction
    }
}

pub enum LoroOp {
    UpsertNode { id: u64, properties: std::collections::HashMap<String, grafeo::Value> },
    DeleteNode { id: u64 },
}
```

---

## 3. Read-Your-Own-Writes (RYOW) Consistency

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

## 4. Concurrent Write Scaling via Block-STM

When multiple remote updates arrive concurrently, Grafeo's **Block-STM** execution engine partitions the transaction execution [1].

1.  Updates are executed speculatively in parallel across the Thread Pool.
2.  If two operations mutate the same memory block (dependency conflict), the conflict is auto-detected.
3.  The lower-priority transaction is aborted, rolled back, and re-executed instantly [1].
4.  Provides multi-threaded writing speed during high-concurrency collaborative editing spikes without risking database locking bottlenecks.

