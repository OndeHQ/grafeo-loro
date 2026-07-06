# Phase 6 — `#[instrument]` Inventory (T2, L1 cheatsheet)

**Single Source of Truth** (anti-plenger SSOT). Every public API in `src/` that requires a `#[instrument]` span is listed here. L2 adds the actual attributes; L1 lists the contract only.

**Enumeration method**: `rg -n 'pub (async )?fn' src/` against the codebase (47 files, see `repomix-output.xml`). Private fns (`fn`, `async fn` without `pub`) are out of scope and excluded by definition.

**Default span name** = fn name. `skip` fields = `self` for large structs is implicit (`tracing::instrument` skips `self` only when listed); we list only the *additional* fields to skip.

---

## Inventory

### `src/app.rs` — `GrafeoLoroApp`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `GrafeoLoroApp::builder` | 168 | — | — | — | **EXCLUDED**: trivial factory (1 LOC). |
| `GrafeoLoroApp::from_sync_engine` | 184 | — | — | — | **EXCLUDED**: field-init constructor. |
| `GrafeoLoroApp::from_sync_engine_with_config` | 210 | — | — | — | **EXCLUDED**: field-init constructor. |
| `GrafeoLoroApp::from_sync_engine_with_telemetry` | 246 | — | — | — | **EXCLUDED**: field-init constructor. |
| `GrafeoLoroApp::maps` | 271 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::sync_engine` | 281 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::loro_key_counter` | 290 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::ssot_mode` | 296 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::compression` | 302 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::metrics` | 309 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::health` | 316 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::tracer` | 324 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::worker_handles` | 332 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::create_vertex` | 341 | `create_vertex` | `self` | debug | Builder factory; cheap but marks a vertex creation flow. |
| `GrafeoLoroApp::query` | 351 | `query` | `self`, `gql` (potentially large) | info | GQL execution path; surface in traces. |
| `GrafeoLoroApp::update_text` | 357 | `update_text` | `self`, `text` (unbounded) | info | Async mutation; hot sync path. |
| `GrafeoLoroApp::generate_embedding` | 369 | `generate_embedding` | `self` | info | Async vector offload. |
| `GrafeoLoroApp::checkpoint` | 465 | `checkpoint` | `self` | info | Persistence op; bound to storage latency. |
| `GrafeoLoroApp::hydrate` | 711 | `hydrate` | `self` | info | Cold-start hydration; matches `create_cold_start_span`. |
| `GrafeoLoroApp::broadcast_presence` | 975 | `broadcast_presence` | `self`, `payload` (ephemeral) | info | Network send. |
| `GrafeoLoroApp::shutdown` | 999 | `shutdown` | `self` | info | Lifecycle termination; observe join errors. |

### `src/app.rs` — `GrafeoLoroAppBuilder`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `storage` | 1071 | — | — | — | **EXCLUDED**: builder setter (1 LOC). |
| `ssot_mode` | 1090 | — | — | — | **EXCLUDED**: builder setter. |
| `compression` | 1111 | — | — | — | **EXCLUDED**: builder setter. |
| `sync_compression` | 1131 | — | — | — | **EXCLUDED**: builder setter. |
| `batch_interval_ms` | 1153 | — | — | — | **EXCLUDED**: builder setter. |
| `batch_max_size` | 1171 | — | — | — | **EXCLUDED**: builder setter. |
| `grafeo_dir` | 1187 | — | — | — | **EXCLUDED**: builder setter. |
| `with_metrics` | 1208 | — | — | — | **EXCLUDED**: builder setter. |
| `with_health` | 1231 | — | — | — | **EXCLUDED**: builder setter. |
| `with_tracer` | 1252 | — | — | — | **EXCLUDED**: builder setter. |
| `build` | 1319 | `build` | `self` | info | Async lifecycle; spawns workers — high value. |

### `src/app.rs` — `VertexBuilder`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `with_label` | 1550 | — | — | — | **EXCLUDED**: builder setter. |
| `with_property` | 1556 | — | — | — | **EXCLUDED**: builder setter. |
| `commit` | 1620 | `vertex_commit` | `self`, `value` (unbounded) | info | Mutation; surfaces Grafeo transaction latency. |

### `src/bridge/sync_engine.rs` — `SyncEngine`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `new` | 181 | — | — | — | **EXCLUDED**: constructor (struct init). |
| `with_batch_config` | 208 | — | — | — | **EXCLUDED**: constructor. |
| `with_telemetry` | 238 | — | — | — | **EXCLUDED**: constructor. |
| `maps` | 313 | — | — | — | **EXCLUDED**: trivial accessor. |
| `metrics` | 321 | — | — | — | **EXCLUDED**: trivial accessor. |
| `tracer` | 329 | — | — | — | **EXCLUDED**: trivial accessor. |
| `health` | 338 | — | — | — | **EXCLUDED**: trivial accessor. |
| `init_loro_subscriber` | 352 | `init_loro_subscriber` | `self` | info | Lifecycle; wires subscription — observe failures. |
| `spawn_inbound_worker` | 435 | `spawn_inbound_worker` | `self` | info | Async loop start; mark in traces. |
| `spawn_outbound_worker` | 524 | `spawn_outbound_worker` | `self` | info | Async loop start. |
| `spawn_cdc_poller` | 627 | `spawn_cdc_poller` | `self` | info | Async loop start. |
| `spawn_all` | 714 | `spawn_all` | `self` | info | Orchestrates the 3 spawns. |
| `shutdown` | 731 | `shutdown` | `self` | info | Lifecycle. |
| `inbound_sender` | 737 | — | — | — | **EXCLUDED**: trivial accessor. |
| `outbound_sender` | 743 | — | — | — | **EXCLUDED**: trivial accessor. |
| `inbound_event_count` | 754 | — | — | — | **EXCLUDED**: trivial counter accessor. |
| `inbound_filtered_count` | 767 | — | — | — | **EXCLUDED**: trivial counter accessor. |

### `src/bridge/batcher.rs` — `MutationBatcher`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `new` | 99 | — | — | — | **EXCLUDED**: constructor. |
| `with_defaults` | 131 | — | — | — | **EXCLUDED**: constructor. |
| `metrics` | 155 | — | — | — | **EXCLUDED**: trivial accessor. |
| `tracer` | 161 | — | — | — | **EXCLUDED**: trivial accessor. |
| `health` | 170 | — | — | — | **EXCLUDED**: trivial accessor. |
| `run` | 178 | `batcher_run` | `self`, `rx` (receiver) | info | Async event loop; primary sync drain. |

### `src/bridge/grafeo_tx.rs` — `BridgeMaps` + `apply_loro_op`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `BridgeMaps::new` | 39 | — | — | — | **EXCLUDED**: constructor. |
| `insert_node` | 45 | `bridge_insert_node` | `self` | trace | Map mutation; hot path — trace only. |
| `remove_node` | 52 | `bridge_remove_node` | `self` | trace | Map mutation; trace. |
| `insert_edge` | 59 | `bridge_insert_edge` | `self` | trace | Map mutation; trace. |
| `remove_edge` | 65 | `bridge_remove_edge` | `self` | trace | Map mutation; trace. |
| `remove_edge_by_id` | 73 | `bridge_remove_edge_by_id` | `self` | trace | Map mutation; trace. |
| `apply_loro_op` | 86 | `apply_loro_op` | `session`, `op` (may be large), `maps` | info | Core sync translator; surfaces per-op latency. |

### `src/bridge/origin.rs`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `is_grafeo_bridge_origin` | 19 | — | — | — | **EXCLUDED**: 1-LOC pure comparison. |
| `is_loro_bridge_origin` | 28 | — | — | — | **EXCLUDED**: 1-LOC pure comparison. |

### `src/compression/wrapper.rs` — `CompressedPayload` + `LoroDocCompressionExt`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `compress` | 33 | `payload_compress` | `raw_bytes` (large) | info | CPU-bound codec work. |
| `decompress` | 58 | `payload_decompress` | `self` | info | CPU-bound codec work. |
| `to_wire` | 89 | `payload_to_wire` | `self` | debug | Serialization; cheap, but tagged for wire-format debugging. |
| `from_wire` | 102 | `payload_from_wire` | `bytes` (large) | debug | Deserialization. |
| `compress_to_wire` | 125 | `compress_to_wire` | `raw_bytes` | info | Composite hot-sync entry. |
| `decompress_from_wire` | 133 | `decompress_from_wire` | `bytes` | info | Composite cold-start entry. |
| `export_compressed` (trait) | 170 | `export_compressed` | `self`, `mode` | info | Loro export + compress. |
| `import_compressed` (trait) | 177 | `import_compressed` | `self`, `payload` | info | Decompress + Loro import. |

### `src/hydration/parallel.rs`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `parallel_hydrate_grafeo` | 42 | `parallel_hydrate_grafeo` | `doc` (large), `db` (large) | info | Cold-start critical path. |

### `src/hydration/vector.rs` — `VectorOffloadManager` + `generate_local_embedding`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `new` | 23 | — | — | — | **EXCLUDED**: constructor. |
| `handle_text_update` | 36 | `handle_text_update` | `self`, `text` (unbounded) | info | Async vector pipeline. |
| `generate_local_embedding` | 122 | `generate_local_embedding` | `text` (unbounded) | info | ONNX stub; observe latency. |

### `src/schema/tree.rs`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `sync_tree_move_to_grafeo` | 116 | `sync_tree_move_to_grafeo` | `session`, `maps` | info | Tree move op; Serializable isolation. |

### `src/storage/traits.rs` — `StorageBackend` trait

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `StorageBackend` trait decl | 2 | — | — | — | **EXCLUDED**: trait declarations cannot carry `#[instrument]`; only impls can. No in-tree impls exist (app provides impls). L2 may instrument concrete impls if any are added. |

### `src/telemetry/metrics.rs` — `MetricsRegistry`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `init` | 98 | — | — | — | **EXCLUDED**: constructor (OTel meter init). |
| `record_batch_flush` | 121 | `record_batch_flush` | `self` | trace | Pure counter write; trace only. |
| `record_hydration` | 141 | `record_hydration` | `self` | trace | Pure histogram write; trace only. |

### `src/telemetry/health.rs` — `HealthProbe`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `new` | 105 | — | — | — | **EXCLUDED**: constructor. |
| `update_sync_ts` | 128 | `update_sync_ts` | `self` | debug | Atomic store; cheap but marks sync heartbeat. |
| `check` | 160 | `health_check` | `self` | info | Liveness probe; must surface in traces. |
| `_last_sync_ts_for_test` | 204 | — | — | — | **EXCLUDED**: test-only accessor. |
| `_set_last_sync_ts_for_test` | 213 | — | — | — | **EXCLUDED**: test-only accessor. |

### `src/telemetry/traces.rs` — span factories

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `create_cold_start_span` | 63 | — | — | — | **EXCLUDED**: span factory — instrumenting it is recursive. |
| `create_inbound_sync_span` | 84 | — | — | — | **EXCLUDED**: span factory. |
| `create_outbound_sync_span` | 107 | — | — | — | **EXCLUDED**: span factory. |
| `create_hybrid_query_span` | 127 | — | — | — | **EXCLUDED**: span factory. |

### `src/types/values.rs` — pure conversion fns

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `lval_to_gval` | 163 | — | — | — | **EXCLUDED**: pure conversion, hot path; span overhead dominates. |
| `gval_to_grafeo_value` | 188 | — | — | — | **EXCLUDED**: pure conversion. |
| `grafeo_value_to_lval` | 213 | — | — | — | **EXCLUDED**: pure conversion. |

### `src/types/events.rs` — `CdcEventWrapper`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `new` | 68 | — | — | — | **EXCLUDED**: 1-LOC struct constructor. |

### `src/presence/socket.rs` — `PresenceManager`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `new` | 12 | — | — | — | **EXCLUDED**: constructor. |
| `broadcast` | 18 | `presence_broadcast` | `self`, `payload` | info | Network send; surfaces WS latency. |
| `parse_eph_envelope` | 24 | `parse_eph_envelope` | `bytes` | debug | Pure parse; tagged for malformed-input debugging. |
| `build_eph_envelope` | 30 | `build_eph_envelope` | `payload` | debug | Pure serialize; tagged for envelope-format debugging. |

---

## Exclusion Rationale (YAGNI)

The following categories are **excluded** from instrumentation because `#[instrument]` overhead (span enter/exit + attribute capture) would exceed the work performed, producing noise without observability value:

1. **Trivial accessors** (`maps`, `metrics`, `tracer`, `health`, `worker_handles`, `*count`, etc.) — 1-LOC field returns; span overhead > work.
2. **Constructors** (`new`, `with_defaults`, `with_*`, `from_sync_engine*`) — struct initialization; no I/O, no failure modes worth tracing.
3. **Builder setters** (`storage`, `ssot_mode`, `compression`, `with_label`, `with_property`, `batch_*`, `grafeo_dir`, etc.) — 1-LOC field writes.
4. **Pure comparison / conversion fns** (`is_grafeo_bridge_origin`, `is_loro_bridge_origin`, `lval_to_gval`, `gval_to_grafeo_value`, `grafeo_value_to_lval`) — pure CPU, hot path; span overhead dominates.
5. **Span factories** (`create_*_span` in `telemetry/traces.rs`) — they *create* spans; instrumenting them is recursive (anti-plenger #4 Performance).
6. **Trait declarations** (`StorageBackend`) — `#[instrument]` applies to impls, not declarations. No in-tree impls exist.
7. **Test-only accessors** (`_last_sync_ts_for_test`, `_set_last_sync_ts_for_test`) — never on production paths.

## Inclusion Rationale

Public APIs that perform one or more of: **async work**, **I/O** (network, disk, codec), **mutation of shared state**, or **non-trivial computation** are included at `info` (default) or `debug`/`trace` where the path is hot enough that `info` would be noisy. Large or unbounded parameters (`text`, `payload`, `bytes`, `raw_bytes`, `gql`, `value`, `op`) are listed in `skip` to prevent span attribute bloat.

## Summary counts

- **Total public fns/methods enumerated**: 88
- **Included (to be instrumented in L2)**: 33
- **Excluded (YAGNI)**: 55

## L2 contract

L2 must add `#[tracing::instrument(skip(...), level = "...")]` (or `#[instrument(name = "...")]` where span name != fn name) to every **Included** row above. No new rows may be added without updating this SSOT doc (anti-plenger SSOT).
