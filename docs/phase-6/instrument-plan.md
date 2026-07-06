# Phase 6 — `#[instrument]` Inventory (T2, L2 SSOT)

**Single Source of Truth** (anti-plenger SSOT). Every public API in `src/` that requires a `#[instrument]` span is listed here. L2 adds the actual attributes; L1 lists the contract only.

**Enumeration method**: `rg -n 'pub (async )?fn' src/` against the codebase (47 files, see `repomix-output.xml`). Private fns (`fn`, `async fn` without `pub`) are out of scope and excluded by definition.

**Default span name** = fn name. `skip` fields = `self` for large structs is implicit (`tracing::instrument` skips `self` only when listed); we list only the *additional* fields to skip.

**Trait-method rows**: `#[instrument]` applies to **impl-block methods** only (it cannot be placed on trait method declarations — no body). The `Line` column for trait methods points to the impl-block method line.

---

## Inventory

### `src/app.rs` — `GrafeoLoroApp`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `GrafeoLoroApp::builder` | 168 | — | — | — | **EXCLUDED**: trivial factory (1 LOC). |
| `GrafeoLoroApp::from_sync_engine` | 184 | — | — | — | **EXCLUDED**: field-init constructor. |
| `GrafeoLoroApp::from_sync_engine_with_config` | 210 | — | — | — | **EXCLUDED**: field-init constructor. |
| `GrafeoLoroApp::from_sync_engine_with_telemetry` | 251 | — | — | — | **EXCLUDED**: field-init constructor (8 args; `#[allow(clippy::too_many_arguments)]` applied per C4.1 — refactor to `AppConfig` struct deferred to future phase). |
| `GrafeoLoroApp::maps` | 276 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::sync_engine` | 286 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::loro_key_counter` | 295 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::ssot_mode` | 301 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::compression` | 307 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::metrics` | 314 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::health` | 321 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::tracer` | 329 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::worker_handles` | 337 | — | — | — | **EXCLUDED**: trivial accessor. |
| `GrafeoLoroApp::create_vertex` | 346 | `create_vertex` | `self` | debug | Builder factory; cheap but marks a vertex creation flow. |
| `GrafeoLoroApp::query` | 356 | `query` | `self`, `gql` (potentially large) | info | GQL execution path; surface in traces. |
| `GrafeoLoroApp::update_text` | 362 | `update_text` | `self`, `text` (unbounded) | info | Async mutation; hot sync path. |
| `GrafeoLoroApp::generate_embedding` | 374 | `generate_embedding` | `self` | info | Async vector offload. |
| `GrafeoLoroApp::checkpoint` | 470 | `checkpoint` | `self` | info | Persistence op; bound to storage latency. |
| `GrafeoLoroApp::hydrate` | 717 | `hydrate` | `self` | info | Cold-start hydration; matches `create_cold_start_span`. |
| `GrafeoLoroApp::broadcast_presence` | 981 | `broadcast_presence` | `self`, `payload` (ephemeral) | info | Network send. |
| `GrafeoLoroApp::shutdown` | 1005 | `shutdown` | `self` | info | Lifecycle termination; observe join errors. |

### `src/app.rs` — `GrafeoLoroAppBuilder`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `storage` | 1077 | — | — | — | **EXCLUDED**: builder setter (1 LOC). |
| `ssot_mode` | 1096 | — | — | — | **EXCLUDED**: builder setter. |
| `compression` | 1117 | — | — | — | **EXCLUDED**: builder setter. |
| `sync_compression` | 1137 | — | — | — | **EXCLUDED**: builder setter. |
| `batch_interval_ms` | 1159 | — | — | — | **EXCLUDED**: builder setter. |
| `batch_max_size` | 1177 | — | — | — | **EXCLUDED**: builder setter. |
| `grafeo_dir` | 1193 | — | — | — | **EXCLUDED**: builder setter. |
| `with_metrics` | 1214 | — | — | — | **EXCLUDED**: builder setter. |
| `with_health` | 1237 | — | — | — | **EXCLUDED**: builder setter. |
| `with_tracer` | 1258 | — | — | — | **EXCLUDED**: builder setter. |
| `build` | 1325 | `build` | `self` | info | Async lifecycle; spawns workers — high value. |

### `src/app.rs` — `VertexBuilder`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `with_label` | 1554 | — | — | — | **EXCLUDED**: builder setter. |
| `with_property` | 1560 | — | — | — | **EXCLUDED**: builder setter. |
| `commit` | 1624 | `vertex_commit` | `self`, `value` (unbounded) | info | Mutation; surfaces Grafeo transaction latency. |

### `src/bridge/sync_engine.rs` — `SyncEngine`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `new` | 181 | — | — | — | **EXCLUDED**: delegating constructor (forwards to `Self::new_inner`); no I/O, no failure modes worth tracing. `new_inner` is private (out of inventory scope). |
| `with_batch_config` | 212 | — | — | — | **EXCLUDED**: constructor. |
| `with_telemetry` | 246 | — | — | — | **EXCLUDED**: constructor. |
| `maps` | 331 | — | — | — | **EXCLUDED**: trivial accessor. |
| `metrics` | 339 | — | — | — | **EXCLUDED**: trivial accessor. |
| `tracer` | 347 | — | — | — | **EXCLUDED**: trivial accessor. |
| `health` | 356 | — | — | — | **EXCLUDED**: trivial accessor. |
| `init_loro_subscriber` | 370 | `init_loro_subscriber` | `self` | info | Lifecycle; wires subscription — observe failures. |
| `spawn_inbound_worker` | 456 | `spawn_inbound_worker` | `self` | info | Async loop start; mark in traces. |
| `spawn_outbound_worker` | 545 | `spawn_outbound_worker` | `self` | info | Async loop start. |
| `spawn_cdc_poller` | 648 | `spawn_cdc_poller` | `self` | info | Async loop start. |
| `spawn_all` | 735 | `spawn_all` | `self` | info | Orchestrates the 3 spawns. |
| `shutdown` | 752 | `shutdown` | `self` | info | Lifecycle. |
| `inbound_sender` | 758 | — | — | — | **EXCLUDED**: trivial accessor. |
| `outbound_sender` | 764 | — | — | — | **EXCLUDED**: trivial accessor. |
| `inbound_event_count` | 775 | — | — | — | **EXCLUDED**: trivial counter accessor. |
| `inbound_filtered_count` | 788 | — | — | — | **EXCLUDED**: trivial counter accessor. |

### `src/bridge/batcher.rs` — `MutationBatcher`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `new` | 104 | — | — | — | **EXCLUDED**: constructor (9 args; `#[allow(clippy::too_many_arguments)]` applied per C4.1 — refactor to `BatcherConfig` struct deferred to future phase). |
| `with_defaults` | 136 | — | — | — | **EXCLUDED**: constructor. |
| `metrics` | 160 | — | — | — | **EXCLUDED**: trivial accessor. |
| `tracer` | 166 | — | — | — | **EXCLUDED**: trivial accessor. |
| `health` | 175 | — | — | — | **EXCLUDED**: trivial accessor. |
| `run` | 183 | `batcher_run` | `self`, `rx` (receiver) | info | Async event loop; primary sync drain. |

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
| `decompress` | 61 | `payload_decompress` | `self` | info | CPU-bound codec work. |
| `to_wire` | 92 | `payload_to_wire` | `self` | debug | Serialization; cheap, but tagged for wire-format debugging. |
| `from_wire` | 105 | `payload_from_wire` | `bytes` (large) | debug | Deserialization. |
| `compress_to_wire` | 128 | `compress_to_wire` | `raw_bytes` | info | Composite hot-sync entry. |
| `decompress_from_wire` | 136 | `decompress_from_wire` | `bytes` | info | Composite cold-start entry. |
| `export_compressed` (impl) | 184 | `export_compressed` | `self`, `mode` | info | Loro export + compress. `#[instrument]` goes on the `impl LoroDocCompressionExt for LoroDoc` method at line 184, NOT the trait decl at line 173. |
| `import_compressed` (impl) | 199 | `import_compressed` | `self`, `payload` | info | Decompress + Loro import. `#[instrument]` goes on the `impl LoroDocCompressionExt for LoroDoc` method at line 199, NOT the trait decl at line 180. |

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
| `StorageBackend` trait decl | 2 | — | — | — | **EXCLUDED**: trait declarations cannot carry `#[instrument]`; only impls can. Two test-only impls exist (`InMemoryStorage` in `tests/unit/builder_validation.rs:56` and `tests/unit/hydrate_checkpoint.rs:78`); production impls are app-provided out-of-tree and not instrumented by this crate. |

### `src/telemetry/metrics.rs` — `MetricsRegistry`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `init` | 98 | — | — | — | **EXCLUDED**: constructor (OTel meter init). |
| `record_batch_flush` | 121 | `record_batch_flush` | `self` | trace | Pure counter write; trace only. |
| `record_hydration` | 143 | `record_hydration` | `self` | trace | Pure histogram write; trace only. |

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

Per Devil C2.3 / Q1: INCLUDED at `trace` level. `tracing` skips trace-level spans entirely when the subscriber doesn't accept `TRACE` (zero-cost in production). With trace enabled, the span IS the observability value: you see every value-conversion call site, which is the SSOT Loro↔Grafeo type-translation boundary (arch §5/§6).

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `lval_to_gval` | 159 | `lval_to_gval` | `val` (potentially large) | trace | Loro→Grafeo value-translation SSOT (arch §5/§6). |
| `gval_to_grafeo_value` | 184 | `gval_to_grafeo_value` | `val` | trace | Grafeo-value→grafeo::Value for inbound apply path. |
| `grafeo_value_to_lval` | 208 | `grafeo_value_to_lval` | `val` | trace | grafeo::Value→LoroValue for outbound worker. |

### `src/types/events.rs` — `CdcEventWrapper`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `new` | 66 | — | — | — | **EXCLUDED**: 1-LOC struct constructor. |

### `src/presence/socket.rs` — `PresenceManager`

| Symbol | Line | Span name | `skip` | Level | Notes |
|---|---|---|---|---|---|
| `new` | 17 | — | — | — | **EXCLUDED**: constructor. |
| `broadcast` | 23 | `presence_broadcast` | `self`, `payload` | info | Network send; surfaces WS latency. |
| `parse_eph_envelope` | 29 | `parse_eph_envelope` | `bytes` | debug | Pure parse; tagged for malformed-input debugging. |
| `build_eph_envelope` | 35 | `build_eph_envelope` | `payload` | debug | Pure serialize; tagged for envelope-format debugging. |

---

## Exclusion Rationale (YAGNI)

The following categories are **excluded** from instrumentation because `#[instrument]` overhead (span enter/exit + attribute capture) would exceed the work performed, producing noise without observability value:

1. **Trivial accessors** (`maps`, `metrics`, `tracer`, `health`, `worker_handles`, `*count`, etc.) — 1-LOC field returns; span overhead > work.
2. **Constructors** (`new`, `with_defaults`, `with_*`, `from_sync_engine*`) — struct initialization; no I/O, no failure modes worth tracing. (Delegating constructors that forward to private helpers like `SyncEngine::new_inner` are also excluded — the helper is private and out of inventory scope.)
3. **Builder setters** (`storage`, `ssot_mode`, `compression`, `with_label`, `with_property`, `batch_*`, `grafeo_dir`, etc.) — 1-LOC field writes.
4. **Pure comparison fns** (`is_grafeo_bridge_origin`, `is_loro_bridge_origin`) — 1-LOC pure CPU; span overhead dominates. (Conversion fns `lval_to_gval`, `gval_to_grafeo_value`, `grafeo_value_to_lval` are INCLUDED at `trace` per C2.3 — zero-cost when TRACE disabled.)
5. **Span factories** (`create_*_span` in `telemetry/traces.rs`) — they *create* spans; instrumenting them is recursive (anti-plenger #4 Performance).
6. **Trait declarations** (`StorageBackend`, `LoroDocCompressionExt`) — `#[instrument]` applies to impls, not declarations. Trait method rows above point at impl-block lines.
7. **Test-only accessors** (`_last_sync_ts_for_test`, `_set_last_sync_ts_for_test`) — never on production paths.

## Inclusion Rationale

Public APIs that perform one or more of: **async work**, **I/O** (network, disk, codec), **mutation of shared state**, or **non-trivial computation** are included at `info` (default) or `debug`/`trace` where the path is hot enough that `info` would be noisy. Large or unbounded parameters (`text`, `payload`, `bytes`, `raw_bytes`, `gql`, `value`, `op`) are listed in `skip` to prevent span attribute bloat.

## Summary counts

- **Total public fns/methods (per `rg -n 'pub (async )?fn' src/`)**: 98
- **Total entries in this SSOT**: 101 (98 pub fns + 2 trait-method decls in `LoroDocCompressionExt` + 1 `StorageBackend` trait-decl row)
- **Included (to be instrumented in L2)**: 45 (42 originally INCLUDED + 3 pure-conversion fns moved from EXCLUDED per Devil C2.3/Q1)
- **Excluded (YAGNI)**: 56

## Stubbed APIs (Phase 6 T1 — user-excluded)

The following INCLUDED pub fns currently have `unimplemented!()` bodies (T1 was excluded by user). L2 still adds `#[instrument]` per spec — the span will fire on entry, then the body panics. This is acceptable: the span surfaces "stub hit" in traces, which is itself useful during the post-T1 transition.

- `GrafeoLoroApp::query` (app.rs:356)
- `GrafeoLoroApp::update_text` (app.rs:362)
- `GrafeoLoroApp::generate_embedding` (app.rs:374)
- `GrafeoLoroApp::broadcast_presence` (app.rs:981)
- `PresenceManager::broadcast` (presence/socket.rs:23)
- `PresenceManager::parse_eph_envelope` (presence/socket.rs:29)
- `PresenceManager::build_eph_envelope` (presence/socket.rs:35)

Each gets a one-line comment above the `#[instrument]` attribute: `// NOTE: body unimplemented!() — T1 excluded per user; span fires then panics`.

## Span hierarchy (arch §23.2)

Architecture §23.2 defines a 5-parent span hierarchy with 13 child spans. The parent spans are created via `create_*_span` factories in `src/telemetry/traces.rs` (correctly EXCLUDED above as span factories). The **child spans** do NOT map to any existing `pub fn` — they require inline `tracing::info_span!(...)` or `#[instrument]` calls inside method bodies (L3 placement, deferred until T1 fills the bodies).

| Child span | Parent | Host method (L3 placement) |
|---|---|---|
| `decompress_snapshot` | `cold_start_hydration` | `GrafeoLoroApp::hydrate` (after storage.load, before import) |
| `import_loro_doc` | `cold_start_hydration` | `GrafeoLoroApp::hydrate` (around `LoroDoc::import_with_status`) |
| `parallel_hydrate_grafeo` | `cold_start_hydration` | already a pub fn (INCLUDED above) — wraps the rayon parallel hydration |
| `hydrate_chunk` | `parallel_hydrate_grafeo` | `parallel_hydrate_grafeo` (per rayon chunk) |
| `receive_loro_event` | `inbound_sync_loop` | `SyncEngine::spawn_inbound_worker` (per event) |
| `batch_flush` | `inbound_sync_loop` | `MutationBatcher::run` (per flush) |
| `grafeo_commit` | `batch_flush` | `MutationBatcher::run` (around `prepared.commit()`) |
| `index_rebuild` | `inbound_sync_loop` | `SyncEngine::spawn_inbound_worker` (post-batch) |
| `receive_cdc_event` | `outbound_sync_loop` | `SyncEngine::spawn_cdc_poller` (per poll) |
| `loro_commit` | `outbound_sync_loop` | `SyncEngine::spawn_outbound_worker` (per commit) |
| `local_grafeo_write` | `user_mutation` | `GrafeoLoroApp::update_text` (RYOW path) |
| `local_loro_commit` | `user_mutation` | `GrafeoLoroApp::update_text` (post-write) |
| `hnsw_search` | `hybrid_query` | `GrafeoLoroApp::query` (vector arm) |
| `graph_traversal` | `hybrid_query` | `GrafeoLoroApp::query` (GQL arm) |

Note: arch §23.2 row 4 (`user_mutation`) has no `create_user_mutation_span` factory in `telemetry/traces.rs`. L3 should either add it OR fold `local_grafeo_write`/`local_loro_commit` into the `update_text` `#[instrument]` span as inner `info_span!` calls. Most host methods currently have `unimplemented!()` bodies (see "Stubbed APIs" above), so L3 placement is deferred until T1 fills them. L2 adds `#[instrument]` on the parent pub fns (already in inventory); L3 adds inline `info_span!` calls for the children when bodies are written.

## L2 contract

L2 must add `#[tracing::instrument(skip(...), level = "...")]` (or `#[instrument(name = "...")]` where span name != fn name) to every **Included** row above. No new rows may be added without updating this SSOT doc (anti-plenger SSOT).

- For L2: minimal `#[instrument]` (no fields/skip yet — L3 refines).
- Place `#[instrument]` on the **impl-block method**, not the trait declaration (per C2.2).
- For async fns: `#[instrument]` works on async fns (tracing handles the future).
- For fns with `unimplemented!()` body: add `// NOTE: body unimplemented!() — T1 excluded per user; span fires then panics` above the attribute.
- Add `use tracing::instrument;` to each modified file (or extend existing `use tracing::{...}` import).
