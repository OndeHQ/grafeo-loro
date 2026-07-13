# `grafeo-loro` Project Structure

Rust crate layout. Centralized types. Centralized constants. Dual-store graph DB. CRDT consensus.

```text
grafeo-loro/
├── Cargo.toml                 # Deps: loro, grafeo, tokio, rayon, zstd, lz4_flex
├── src/
│   ├── lib.rs                 # Crate root. Re-exports.
│   ├── app.rs                 # `GrafeoLoroApp` builder. Lifecycle.
│   ├── config.rs              # `SsotMode` enum. Tuning knobs.
│   ├── error.rs               # Unified `GrafeoLoroError`.
│   ├── constants.rs           # Centralized constants. Origins, keys, magic bytes, defaults.
│   │
│   ├── types/                 # Centralized types. Shared across all modules.
│   │   ├── mod.rs             # Module exports.
│   │   ├── ids.rs             # `NodeId`, `EdgeId`, `PeerId` newtypes.
│   │   ├── values.rs          # `LoroProperty`, `GraphValue` enums.
│   │   ├── events.rs          # `LoroOp`, `CdcEvent` wrappers.
│   │   └── presence.rs        # `PresencePayload` struct.
│   │
│   ├── bridge/                # LoroGrafeoBridge. Bidirectional sync.
│   │   ├── mod.rs             
│   │   ├── sync_engine.rs     # `SyncEngine`. MPSC channels. Tokio loops.
│   │   ├── batcher.rs         # `MutationBatcher`. Time/count flush.
│   │   └── origin.rs          # Echo prevention. Origin tag checks.
│   │
│   ├── schema/                # Loro CRDT container mapping.
│   │   ├── mod.rs             
│   │   ├── vertex.rs          # `VertexEntity`. Uses `types::values`.
│   │   ├── edge.rs            # `EdgeEntity`. Src/dst bounds.
│   │   └── tree.rs            # `OrderedCollection`. `T_CHILD` moves.
│   │
│   ├── compression/           # Dual-layer payload shrinking.
│   │   ├── mod.rs             
│   │   └── wrapper.rs         # `CompressedPayload`. `LoroDocCompressionExt`.
│   │
│   ├── hydration/             # Cold boot index rebuild.
│   │   ├── mod.rs             
│   │   ├── parallel.rs        # Rayon chunks. Block-STM.
│   │   └── vector.rs          # `VectorOffloadManager`. Local ONNX offload.
│   │
│   ├── storage/               # Pluggable persistence.
│   │   ├── mod.rs             
│   │   └── traits.rs          # `StorageBackend` async trait.
│   │
│   ├── presence/              # Ephemeral real-time state.
│   │   ├── mod.rs             
│   │   └── socket.rs          # WebSocket handling. Uses `types::presence`.
│   │
│   └── telemetry/             # Observability.
│       ├── mod.rs             
│       ├── metrics.rs         # OpenTelemetry counters, histograms.
│       ├── traces.rs          # Span hierarchy.
│       └── health.rs          # `HealthProbe`. Lock checks, staleness.
│
└── tests/
    ├── integration/
    │   ├── sync_echo.rs       # Echo loop prevention tests.
    │   ├── hydration.rs       # Parallel index rebuild tests.
    │   └── snapshot.rs        # Shallow snapshot + Zstd/LZ4 roundtrip.
    └── fixtures/
        └── mock_storage.rs    # Memory `StorageBackend` mock.
```

## Module Responsibilities

### `constants`
Single source of truth for magic strings. Prevents typo bugs. 
*   Origins: `ORIGIN_GRAFEO_BRIDGE` (`"grafeo-bridge"`), `ORIGIN_LORO_BRIDGE` (`"loro-bridge"`).
*   Container keys: `ROOT_VERTICES` (`"V"`), `ROOT_EDGES` (`"E"`). (`ROOT_TREE` (`"T_CHILD"`) was deleted as YAGNI in Phase 1 Hunter Fix 4; re-add in Phase 2 Task 2 when the `T_CHILD` `LoroTree` is wired.)
*   Magic bytes: `EPH_MAGIC` (`b"%EPH"`).
*   Defaults: `DEFAULT_BATCH_MS`, `DEFAULT_CHUNK_SIZE`.

### `types`
Centralized structs/enums. Prevents circular dependencies. Shared by `bridge`, `schema`, `hydration`.
*   `ids`: Strongly typed IDs. Prevents mixing node/edge/peer IDs.
*   `values`: `LoroProperty` enum. Maps Loro values to Grafeo values.
*   `events`: Unified `LoroOp` enum for batcher.
*   `presence`: `PresencePayload` for WebSocket envelopes.

### `bridge`
Glue layer. Converts diffs. Decoupled writing via `tokio::sync::mpsc`. Prevents deadlocks. Uses `constants` for origin tagging.

### `schema`
Declarative mapping via `lorosurgeon`. `#[derive(Hydrate, Reconcile)]`. Maps Rust structs to Loro containers. References `types::values`.

### `compression`
LZ4 hot sync. Zstd level 3 cold snapshots. Wraps `LoroDoc` export/import.

### `hydration`
Rayon parallel chunks. Rebuilds CSR, HNSW, BM25 indexes. Offloads float vectors direct to Grafeo. Never writes vectors to Loro.

### `storage`
Storage-agnostic. App provides S3, filesystem, IPFS via `StorageBackend` trait.

### `telemetry`
OpenTelemetry SDK. Prometheus metrics. Jaeger traces. `HealthProbe` checks `parking_lot::RwLock` poison state, dummy queries, sync staleness.