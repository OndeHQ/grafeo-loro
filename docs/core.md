# API Reference: Core Types

## `GrafeoLoroApp`

The primary application handle. Owns the sync engine, storage backend, and telemetry.

```rust
pub struct GrafeoLoroApp {
    pub(crate) sync_engine: Arc<SyncEngine>,
    pub(crate) ssot_mode: SsotMode,
    pub(crate) storage: Option<Arc<dyn StorageBackend>>,
    pub(crate) compression: CompressionType,
    pub(crate) metrics: Option<Arc<MetricsRegistry>>,
    pub(crate) health: Option<Arc<HealthProbe>>,
    pub(crate) tracer: Option<SharedTracer>,
    pub(crate) worker_handles: Option<Vec<JoinHandle<()>>>,
}
```

### Construction

```rust
// Minimal
let app = GrafeoLoroApp::from_sync_engine(engine);

// With storage
let app = GrafeoLoroApp::from_sync_engine_with_config(
    engine, SsotMode::Loro, Some(storage), CompressionType::Zstd
);

// Full telemetry
let app = GrafeoLoroApp::from_sync_engine_with_config(
    engine,
    AppTelemetryConfig {
        ssot_mode: SsotMode::Loro,
        storage: Some(storage),
        compression: CompressionType::Zstd,
        metrics: Some(metrics),
        health: Some(health),
        tracer: Some(tracer),
        worker_handles: Some(handles),
    }
);
```

### Lifecycle Methods

| Method | Signature | Purpose |
|---|---|---|
| `checkpoint` | `async fn(&self, graph_id: &str) -> Result<()>` | Persist Loro state to storage, clean deltas |
| `hydrate` | `async fn(&self, graph_id: &str) -> Result<()>` | Restore from storage, rebuild Grafeo view |
| `shutdown` | `async fn(self) -> Result<()>` | Graceful worker drain, tracer shutdown |

### Accessor Methods

```rust
app.sync_engine() -> &Arc<SyncEngine>;
app.grafeo_db() -> &Arc<GrafeoDB>;
app.loro_doc() -> &Arc<RwLock<LoroDoc>>;
app.maps() -> &Arc<BridgeMaps>;
app.ssot_mode() -> SsotMode;
app.compression() -> CompressionType;
app.metrics() -> Option<&Arc<MetricsRegistry>>;
app.health() -> Option<&Arc<HealthProbe>>;
app.tracer() -> Option<&SharedTracer>;
app.worker_handles() -> Option<&[JoinHandle<()>]>;
```

---

## `GrafeoLoroAppBuilder`

Fluent builder with validation at `build()`.

```rust
let app = GrafeoLoroApp::builder()
    .storage(Arc::new(s3))           // Required
    .ssot_mode(SsotMode::Loro)       // Default: Loro
    .compression(CompressionType::Zstd) // Default: Zstd
    .sync_compression(CompressionType::Lz4) // Default: Lz4
    .batch_interval_ms(100)          // Default: 100
    .batch_max_size(256)             // Default: 256
    .grafeo_dir(PathBuf::from("/data")) // Required for Grafeo-SSOT
    .with_metrics(metrics)           // Optional
    .with_health(health)             // Optional
    .with_tracer(tracer)             // Optional
    .build().await?;
```

### Validation Rules

| Rule | Error |
|---|---|
| `batch_interval_ms == 0` | `Config("batch_interval_ms must be > 0")` |
| `batch_max_size == 0` | `Config("batch_max_size must be > 0")` |
| `storage == None` | `Config("storage backend not set")` |
| `ssot_mode == Grafeo && grafeo_dir == None` | `Config("grafeo_dir required for SsotMode::Grafeo")` |

---

## `SyncEngine`

Orchestrates the three concurrent workers.

```rust
pub struct SyncEngine {
    pub(crate) grafeo_db: Arc<GrafeoDB>,
    pub(crate) loro_doc: Arc<RwLock<LoroDoc>>,
    pub(crate) inbound_tx: mpsc::Sender<InboundMsg>,
    pub(crate) outbound_tx: mpsc::Sender<OutboundMsg>,
    pub(crate) bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
    pub(crate) maps: Arc<BridgeMaps>,
    pub(crate) batcher: Arc<MutationBatcher>,
    pub(crate) inbound_event_count: Arc<AtomicU64>,
    pub(crate) inbound_filtered_count: Arc<AtomicU64>,
    pub(crate) shutdown_tx: broadcast::Sender<()>,
    pub(crate) metrics: Option<Arc<MetricsRegistry>>,
    pub(crate) tracer: Option<SharedTracer>,
    pub(crate) health: Option<Arc<HealthProbe>>,
}
```

### Construction

```rust
// Minimal
let (engine, inbound_rx, outbound_rx) = SyncEngine::new(db, doc);

// With batch tuning
let (engine, inbound_rx, outbound_rx) = SyncEngine::with_batch_config(db, doc, 512, 50);

// Full telemetry
let (engine, inbound_rx, outbound_rx) = SyncEngine::with_telemetry(
    db, doc, 256, 100, Some(metrics), Some(tracer), Some(health)
);
```

### Worker Spawning

```rust
// Individual workers
let inbound = engine.clone().spawn_inbound_worker(inbound_rx).await;
let outbound = engine.clone().spawn_outbound_worker(outbound_rx).await;
let poller = engine.clone().spawn_cdc_poller().await;

// All at once
let handles = engine.clone().spawn_all(inbound_rx, outbound_rx).await;
// Returns: vec![inbound, outbound, poller]
```

### Origin Filtering

```rust
engine.init_loro_subscriber()?;  // Subscribe to Loro diffs, filter bridge origin
engine.inbound_event_count();    // u64: accepted events
engine.inbound_filtered_count(); // u64: filtered events (echo prevention)
```

### Shutdown

```rust
engine.shutdown(); // broadcast::Sender sends () to all workers
```

---

## `MutationBatcher`

Buffers inbound ops and flushes to Grafeo transactions.

```rust
pub struct MutationBatcher {
    pub(crate) grafeo_db: Arc<GrafeoDB>,
    pub(crate) buffer: Mutex<Vec<LoroOp>>,
    pub(crate) batch_size: usize,
    pub(crate) batch_ms: u64,
    pub(crate) bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
    pub(crate) maps: Arc<BridgeMaps>,
    pub(crate) shutdown_tx: broadcast::Sender<()>,
    pub(crate) metrics: Option<Arc<MetricsRegistry>>,
    pub(crate) tracer: Option<SharedTracer>,
    pub(crate) health: Option<Arc<HealthProbe>>,
}
```

### Runtime Behavior

```rust
// Triggered by:
// 1. Buffer reaches batch_size
// 2. batch_ms interval fires
// 3. Shutdown signal (drain remaining)

// Flush semantics:
// - spawn_blocking for Grafeo transaction
// - Serializable isolation (tree moves)
// - 5-second timeout
// - Metrics recorded on success
// - Health probe updated on success
```

---

## `BridgeMaps`

Bijective key↔id mappings. All methods are `O(1)` and instrumented with `tracing`.

```rust
impl BridgeMaps {
    pub fn new() -> Self;
    pub fn insert_node(&self, loro_key: String, id: NodeId);
    pub fn remove_node(&self, loro_key: &str) -> Option<NodeId>;
    pub fn insert_edge(&self, key: EdgeKey, id: EdgeId);
    pub fn remove_edge(&self, key: &EdgeKey) -> Option<EdgeId>;
    pub fn remove_edge_by_id(&self, id: EdgeId) -> Option<EdgeKey>;
}
```

### Invariant

```rust
// After any operation:
node_id_map.len() == node_key_map.len()
edge_id_map.len() == edge_key_map.len()

// For all (k, v) in node_id_map:
node_key_map.get(&v) == Some(&k)

// For all (k, v) in edge_id_map:
edge_key_map.get(&v) == Some(&k)
```

Verified by `check_i11_bridge_maps_bijectivity` in fuzz target.

---

## `LoroOp`

The unified operation type. Serializable across the bridge.

```rust
pub enum LoroOp {
    UpsertNode {
        loro_key: String,
        labels: Vec<String>,
        properties: HashMap<String, GraphValue>,
    },
    UpsertEdge {
        src_key: String,
        dst_key: String,
        label: String,
        properties: HashMap<String, GraphValue>,
    },
    DeleteNode { loro_key: String },
    DeleteEdge { src_key, dst_key, label: String },
    TreeMove { node_key, old_parent_key, new_parent_key: String },
}
```

### GraphValue

```rust
pub enum GraphValue {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Vector(Arc<[f32]>),
    Map(HashMap<String, GraphValue>),
    List(Vec<GraphValue>),
}
```

Conversion to/from `LoroProperty` (scalar subset) and `grafeo::Value` (full set) via `TryFrom`/`From`.

---

## Error Types

```rust
pub enum GrafeoLoroError {
    Loro(loro::LoroError),           // CRDT operations
    Grafeo(grafeo::Error),           // Graph database
    StorageIo(std::io::Error),       // Backend I/O
    Compression(String),             // Codec failure
    ChannelClosed(String),           // mpsc/broadcast drop
    Config(String),                  // Builder validation
    UnsupportedLoroType(String),     // Binary/Container values
    Bridge(String),                  // Key mapping failure
    Hydrate(lorosurgeon::HydrateError), // Schema deserialization
    TreeMoveCreatesCycle { node_id, new_parent }, // Constraint violation
    NotYetImplemented(String),       // Phase-gated features
    InvalidEnvelope(String),         // Presence protocol
}
```

All variants implement `std::error::Error` via `thiserror`. `From` impls for `loro::LoroError`, `grafeo::Error`, `std::io::Error`, `lorosurgeon::HydrateError`.
