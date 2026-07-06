# grafeo-loro

> Local-first, in-process, dual-store graph database with CRDT consensus.

## Overview

<!-- TODO: L3 — 1-paragraph summary: grafeo-loro bridges LoroDoc (CRDT consensus SSOT) and GrafeoDB (execution SSOT) in-process. Zero cloud servers. See docs/grafeo-loro.architecture.md for full design. -->

## Quickstart

<!-- TODO: L3 — prose introduction -->

- Add `grafeo-loro = "0.1"` to `Cargo.toml` `[dependencies]`.
- Import: `use grafeo_loro::GrafeoLoroApp;`.
- Build an app via `GrafeoLoroApp::builder().storage(...).ssot_mode(...).compression(...).build().await?`.
- Hydrate cold-boot state from storage: `app.hydrate(graph_id).await?`.
- Sync Loro ops into Grafeo via `SyncEngine::spawn_all(...)` (inbound + outbound + CDC poller workers).
- Mutate vertices via `app.create_vertex().with_label(...).with_property(...).commit()?`.
- Checkpoint state to storage: `app.checkpoint(graph_id).await?`.
- Shutdown gracefully: `app.shutdown().await?`.
- See `tests/integration/main.rs` + `tests/unit/main.rs` for working examples.

## Architecture

<!-- TODO: L3 — 1-paragraph summary of the 12-module structure (app, bridge, schema, compression, hydration, storage, presence, telemetry, types, config, constants, error) and the dual-SSOT philosophy (arch §1-2). -->

- **Dual-SSOT design** — LoroDoc is the CRDT consensus SSOT; GrafeoDB is the execution SSOT (architecture §1-2). Bridge module translates ops bidirectionally.
- **12 modules** — see `docs/grafeo-loro.architecture.md` for full design and `docs/grafeo-loro.project-structure.md` for module responsibilities.
- **Module dependency graph** (verified against `rg -n '^use crate::' src/`):

```mermaid
%% grafeo-loro module dependency graph (L2 — verified against `rg -n '^use crate::' src/`)
%% 12 modules per `src/lib.rs` + `docs/grafeo-loro.project-structure.md`.
graph TD
    app
    bridge
    schema
    compression
    hydration
    storage
    presence
    telemetry
    types
    config
    constants
    error

    app --> bridge [label="SyncEngine, BridgeMaps, apply_loro_op"]
    app --> compression [label="CompressedPayload"]
    app --> config [label="CompressionType, SsotMode"]
    app --> constants [label="ORIGIN_LORO_BRIDGE, ROOT_VERTICES, STORAGE_KEY_*"]
    app --> error [label="GrafeoLoroError, Result"]
    app --> hydration [label="parallel_hydrate_grafeo"]
    app --> schema [label="VertexEntity"]
    app --> storage [label="StorageBackend"]
    app --> telemetry [label="HealthProbe, MetricsRegistry, SharedTracer, HydrationMode"]
    app --> types [label="GraphValue, LoroProperty, NodeId, PresencePayload, LoroOp"]
    %% app --> presence: deferred until Phase 6 T1 (broadcast_presence unimplemented!())

    bridge --> constants [label="EPOCH_RETENTION, ORIGIN_*, DEFAULT_BATCH_*, TREE_EDGE_LABEL, ROOT_*"]
    bridge --> error [label="GrafeoLoroError, Result"]
    bridge --> telemetry [label="HealthProbe, MetricsRegistry, SharedTracer"]
    bridge --> types [label="LoroOp, CdcEventWrapper, GraphValue, gval_to_grafeo_value, lval_to_gval, grafeo_value_to_lval"]

    hydration --> bridge [label="apply_loro_op, BridgeMaps"]
    hydration --> constants [label="DEFAULT_CHUNK_SIZE, ORIGIN_LORO_BRIDGE, ROOT_VERTICES, DEFAULT_EMBEDDING_DIM, EMBEDDING_PROPERTY"]
    hydration --> error [label="GrafeoLoroError, Result"]
    hydration --> schema [label="VertexEntity"]
    hydration --> telemetry [label="MetricsRegistry, SharedTracer"]
    hydration --> types [label="LoroOp, GraphValue, NodeId"]

    schema --> constants [label="ORIGIN_LORO_BRIDGE, TREE_EDGE_LABEL"]
    schema --> error [label="GrafeoLoroError"]
    schema --> types [label="LoroProperty, NodeId"]

    compression --> config [label="CompressionType"]
    compression --> error [label="GrafeoLoroError, Result"]

    presence --> error [label="Result"]
    presence --> types [label="PresencePayload"]

    types --> error [label="GrafeoLoroError, Result"]
```

- **Full design doc**: `docs/grafeo-loro.architecture.md` (1384 lines, 25 sections).

## Configuration

<!-- TODO: L3 — prose introduction + per-knob explanation -->

- `SsotMode` (`config.rs`) — `Loro` (default) or `Grafeo`. Selects which store is the consensus SSOT for cold-boot hydration.
- `CompressionType` (`config.rs`) — `None`, `Lz4`, or `Zstd` (default). Used for snapshot export/import via `LoroDocCompressionExt`.
- `AppConfig` (`config.rs`) — top-level config struct:
  - `ssot_mode: SsotMode`
  - `compression: CompressionType` (snapshot codec)
  - `sync_compression: CompressionType` (sync wire codec)
  - `batch_interval_ms: u64` (batcher flush cadence)
  - `batch_max_size: usize` (batcher flush threshold)
  - `hydration_chunk_size: usize` (rayon parallel chunk size)
  - `max_staleness_ms: u64` (health probe threshold)
  - `enable_presence: bool` (WebSocket presence channel)
  - `presence_heartbeat_ms: u64` (presence broadcast cadence)
- `GrafeoLoroAppBuilder` (`app.rs`) — fluent builder for `GrafeoLoroApp`:
  - `.storage(Arc<dyn StorageBackend>)` — pluggable storage backend.
  - `.ssot_mode(SsotMode)` — select SSOT.
  - `.compression(CompressionType)` — snapshot codec.
  - `.sync_compression(CompressionType)` — sync wire codec.
  - `.batch_interval_ms(u64)` / `.batch_max_size(usize)` — batcher tuning.
  - `.grafeo_dir(impl Into<PathBuf>)` — required when `SsotMode::Grafeo`.
  - `.with_metrics(Arc<MetricsRegistry>)` / `.with_health(Arc<HealthProbe>)` / `.with_tracer(SharedTracer)` — optional telemetry.
  - `.build().await?` — finalize + spawn workers.

## Testing

<!-- TODO: L3 — prose introduction -->

- `cargo test --all` — run all 82 tests (lib + integration + unit).
- `cargo test --test integration` — integration tests (`tests/integration/main.rs`): sync echo, tree-move concurrency.
- `cargo test --test unit` — unit tests (`tests/unit/main.rs`): 71 tests across 13 modules (compression, hydration, parallel_hydrate, schema_roundtrip, telemetry, tree_move, vector_offload, vertex_builder, etc.).
- `cargo test -- --ignored` — run tests marked `#[ignore]` (e.g. ONNX smoke test).
- Fuzz harness: `cd fuzz && cargo +nightly fuzz run consistency` (requires nightly Rust + `-Zsanitizer=address`; see `docs/phase-6/fuzz-invariants.md` for the 15 invariants).
- `cargo clippy --all-targets -- -D warnings` — lint gate (CI-enforced).
- `cargo fmt --all --check` — formatting gate (CI-enforced).

## License

<!-- TODO: L3 — pick license (current `LICENSE` file content placeholder) -->
