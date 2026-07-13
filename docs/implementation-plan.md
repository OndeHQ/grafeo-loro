# Implementation Phase: `grafeo-loro`

## Current State Assessment
Skeleton complete. Modules defined. Types centralized. Constants isolated.
**Missing**: All function bodies. Logic stubs (`unimplemented!()`). No tests. No S3 backend. No ONNX integration.

---

## Phase 1: Core Glue & Echo Prevention (Week 1)
Foundation. Must work before hydration or storage.

### Tasks
1.  **Implement `types::values::lval_to_gval`**
    -   Map `LoroValue::Map/List/String/I64/F64/Bool/Null` → `GraphValue`.
    -   Handle nested maps recursively.
    -   Panic/error on unsupported types (e.g., Binary, Container).
2.  **Implement `bridge::origin` checks**
    -   Wire into `sync_engine.rs` subscriber filter.
    -   Wire into `batcher.rs` CDC listener.
3.  **Implement `bridge::sync_engine` MPSC loops**
    -   `init_loro_subscriber`: Filter origin, push to channel.
    -   `spawn_inbound_worker`: Recv loop, batch ops, commit Grafeo tx.
    -   `spawn_outbound_worker`: Recv CDC, filter origin, transact Loro.
4.  **Implement `bridge::batcher` flush logic**
    -   Time/count trigger via `tokio::select!`.
    -   Vectorized upsert/delete in single Grafeo tx.
    -   Set `ORIGIN_LORO_BRIDGE` metadata on tx.

### Validation
-   Unit test: Echo loop prevention (mock Loro+Grafeo, verify no infinite recursion).
-   Integration test: Bidirectional sync with artificial delay.

---

## Phase 2: Schema Mapping & Tree Safety (Week 2)
Declarative CRDT ↔ Graph translation.

### Tasks
1.  **Wire `lorosurgeon` derives**
    -   Verify `VertexEntity`, `EdgeEntity`, `OrderedCollection` compile.
    -   Test roundtrip: Rust struct → Loro container → Rust struct.
2.  **Implement `schema::tree::sync_tree_move_to_grafeo`**
    -   Delete old parent edge.
    -   Insert new parent edge.
    -   Wrap in single Grafeo tx (Serializable isolation; P2T2-DEVIL R3).
    -   Return error if cycle detected (Grafeo does NOT enforce acyclic — bridge pre-checks via `would_create_cycle_precheck`; verified P2T2-L1).
3.  **Implement `app::VertexBuilder` fluent API**
    -   Accumulate labels/properties.
    -   `commit()`: Generate NodeId, write Loro + Grafeo atomically.

### Validation
-   Unit test: Tree move cycle rejection.
-   Integration test: Concurrent tree moves from 3 peers → consistent acyclic result.

---

## Phase 3: Compression & Hydration (Week 3)
Cold boot performance. Storage efficiency.

### Tasks
1.  **Implement `compression::wrapper`**
    -   LZ4: `lz4_flex::compress_prepend_size` / `decompress_size_prepended`.
    -   Zstd: Stream encoder/decoder level 3.
    -   `LoroDocCompressionExt` trait impl.
2.  **Implement `hydration::parallel::parallel_hydrate_grafeo`**
    -   Extract node IDs from Loro map.
    -   `rayon::par_chunks(DEFAULT_CHUNK_SIZE)`.
    -   Per-chunk Grafeo tx with `ORIGIN_LORO_BRIDGE`.
    -   Call `lval_to_gval` for properties.
3.  **Stub `hydration::vector::generate_local_embedding`**
    -   Return deterministic dummy vector for now.
    -   Log warning: "ONNX not integrated".
4.  **Implement `VectorOffloadManager::handle_text_update`**
    -   Generate embedding.
    -   Direct Grafeo upsert (bypass Loro).

### Validation
-   Benchmark: Hydration 10k nodes < 500ms on 8-core.
-   Test: Zstd roundtrip preserves Loro importability.
-   Test: Vector never written to Loro container.

---

## Phase 4: Storage Backend & Lifecycle (Week 4)
Persistence. SSOT mode switching.

### Tasks
1.  **Implement `storage::traits::StorageBackend` for S3**
    -   Use `aws-sdk-s3` or `object_store` crate.
    -   Async load/save/list/delete.
    -   Retry with exponential backoff.
2.  **Implement `app::GrafeoLoroApp::hydrate`**
    -   Match on `SsotMode::Loro` vs `Grafeo`.
    -   Loro mode: Download base + deltas → import → parallel hydrate.
    -   Grafeo mode: Download tar.zst → extract → restore DB → hydrate Loro.
3.  **Implement `app::GrafeoLoroApp::checkpoint`**
    -   Loro mode: Export shallow snapshot → upload base → clear deltas.
    -   Grafeo mode: Backup DB → compress tar.zst → upload.
4.  **Implement `app::GrafeoLoroAppBuilder::build`**
    -   Validate config.
    -   Init LoroDoc, GrafeoDB, SyncEngine, Batcher.
    -   Spawn tokio tasks.

### Validation
-   Integration test: Full cold boot → mutate → checkpoint → cold boot again.
-   Test: S3 network failure → graceful error, no corruption.

---

## Phase 5: Presence & Telemetry (Week 5)
Real-time UX. Observability.

### Tasks
1.  **Implement `presence::socket::PresenceManager`**
    -   Parse/build `%EPH` envelope.
    -   WebSocket broadcast via `tokio-tungstenite`.
    -   Heartbeat timeout cleanup.
2.  **Implement `telemetry::metrics::MetricsRegistry`**
    -   OpenTelemetry counters/histograms.
    -   Record batch flush duration, hydration time, echo filters.
3.  **Implement `telemetry::health::HealthProbe::check`**
    -   Verify RwLock not poisoned.
    -   Dummy Grafeo query.
    -   Check `last_sync_ts` staleness.
4.  **Wire telemetry into bridge/batcher/hydration**
    -   Spans for cold start, inbound sync, hybrid query.

### Validation
-   Test: Presence payload < 256 bytes.
-   Test: Health probe detects stale sync > 5s.
-   Manual: Jaeger trace shows full sync pipeline.

---

## Phase 6: Hardening & Docs (Week 6)
Production readiness.

### Tasks
1.  **Replace all `unimplemented!()` with proper errors**
2.  **Add `#[instrument]` spans to all public APIs**
3.  **Write README with quickstart + architecture diagram**
4.  **CI: `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo test --all`**
5.  **Fuzz test: Random Loro ops → verify Grafeo consistency**

---

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| Deadlock in bidirectional sync | Decoupled MPSC + origin filtering (Phase 1) |
| Hydration OOM on large graphs | Rayon chunks + streaming Loro iteration (Phase 3) |
| S3 partial upload corrupts state | Atomic put + ETag validation (Phase 4) |
| Echo loop bypasses origin check | Integration test with 3-peer mesh (Phase 1) |
| Vector bloat in Loro | Compile-time type safety + runtime assertion (Phase 3) |

---

## Success Criteria
-   Cold boot 10k nodes < 1s (8-core).
-   Sync latency p99 < 50ms (local loopback).
-   Zero echo loops under concurrent edit stress test.
-   S3 storage < 5MB for 10k-node graph (Zstd).
-   Health probe detects failures < 10s.