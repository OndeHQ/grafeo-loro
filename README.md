# grafeo-loro

> Local-first, in-process, dual-store graph database with CRDT consensus.

## Overview

`grafeo-loro` is a Rust library that fuses two storage engines into a single coherent graph database: [LoroDoc](https://loro.dev) serves as the conflict-free consensus source of truth (CRDT layer), and [GrafeoDB](https://crates.io/crates/grafeo) serves as the execution source of truth (graph + vector + BM25 indexes). The bidirectional `bridge` module translates Loro operations into Grafeo transactions on the inbound path and Grafeo CDC events into Loro commits on the outbound path, with origin tagging plus an MVCC-epoch side-channel that together prevent the echo feedback loops which CRDT↔mutable-store bridges are otherwise prone to.

The library solves the bidirectional sync problem inherent to "CRDT for consensus + native index for queries" architectures: how to keep two stores that have different consistency models, different operation granularities, and different concurrency primitives in lock-step without diverging, duplicating writes, or panicking under adversarial op orderings. The architecture (see `docs/grafeo-loro.architecture.md` §1) intentionally rejects the cloud-server coordination pattern — there is no central authoritative service, no network round-trips for commits, and no coordination overhead beyond the in-process `SyncEngine`. Peers sync peer-to-peer by exchanging Loro bytes (the CRDT wire format); local queries hit GrafeoDB directly and observe a consistent MVCC snapshot.

Production usage is via `GrafeoLoroApp::builder()...build().await?`, which constructs a `SyncEngine`, spawns the inbound batcher + outbound CDC poller + Loro subscriber as tokio tasks, and exposes a fluent vertex/edge mutation API. Cold-boot hydration rebuilds Grafeo indexes from a Loro snapshot in parallel via rayon; checkpoint persists a compressed snapshot through a pluggable `StorageBackend` trait (filesystem, S3, IPFS — caller-supplied). The crate exposes 98 public functions across 14 modules, all instrumented with `tracing` spans at `info`/`debug`/`trace` levels per the per-API inventory in `docs/phase-6/instrument-plan.md`.

## Quickstart

The fastest path from `cargo new` to a running graph is the fluent `GrafeoLoroAppBuilder`. Each builder setter corresponds to one `AppConfig` knob (see [Configuration](#configuration) below); `.build().await?` validates the config, constructs a `GrafeoDB` + `LoroDoc` + `SyncEngine`, spawns the three sync workers, and returns a ready-to-use `GrafeoLoroApp`. Mutations flow through `create_vertex()` → `VertexBuilder::commit()`; reads flow through `query()` (Phase 6+ — currently `unimplemented!()` per user scope exclusion). Cold-boot hydration + checkpoint round-trip the state through your `StorageBackend` impl.

- **Add the dep**: `cargo add grafeo-loro` (or add `grafeo-loro = "0.1"` to `Cargo.toml` `[dependencies]`).
- **Import the facade**: `use grafeo_loro::{GrafeoLoroApp, StorageBackend, CompressionType, SsotMode};`.
- **Implement storage**: implement the `StorageBackend` trait (`async fn load/save/list/delete`) for your backend (filesystem, S3, IPFS — see `src/storage/traits.rs`).
- **Build the app**: `let app = GrafeoLoroApp::builder().storage(Arc::new(MyStorage)).ssot_mode(SsotMode::Loro).compression(CompressionType::Zstd).build().await?;`
- **Hydrate cold-boot state**: `app.hydrate("graph_123").await?` — restores Loro snapshot from storage + rebuilds Grafeo indexes via `parallel_hydrate_grafeo` (rayon chunks).
- **Mutate a vertex**: `let id = app.create_vertex().with_label("Person").with_property("name", "Alice").commit()?;`
- **Checkpoint**: `app.checkpoint("graph_123").await?` — exports a shallow Loro snapshot + compresses via `CompressedPayload::compress_to_wire(.., CompressionType::Zstd)` + persists via `StorageBackend::save`.
- **Shutdown**: `app.shutdown().await?` — drains the inbound batcher, joins the three sync workers, releases the Loro subscription.
- **See working examples**: `tests/integration/main.rs` (sync echo, tree-move concurrency) + `tests/unit/vertex_builder.rs` (fluent API) + `tests/unit/hydrate_checkpoint.rs` (cold-boot round-trip).

## Architecture

`grafeo-loro` is structured as 12 modules with a strict dual-SSOT philosophy (architecture §1-2): `LoroDoc` is the consensus SSOT (authoritative for state + history + network merges); `GrafeoDB` is the execution SSOT (authoritative for queries + vector + BM25 indexes). The `bridge` module is the glue — it owns the four id-mapping tables (`loro_key ↔ NodeId`, `EdgeKey ↔ EdgeId` in both directions) and the `apply_loro_op` translator that converts each `LoroOp` variant (`UpsertNode`, `UpsertEdge`, `DeleteNode`, `DeleteEdge`, `TreeMove`) into a Grafeo `Session` transaction. The `SyncEngine` (in `bridge/`) wires three async workers — inbound (Loro→Grafeo via `MutationBatcher`), outbound (Grafeo→Loro via CDC poller), and the Loro subscriber — plus the epoch side-channel set that prevents echo loops.

The module dependency graph (verified against `rg -n '^use crate::' src/`) is below; the full design doc is `docs/grafeo-loro.architecture.md` (1384 lines, 25 sections). The `app` module sits at the top as the user-facing facade; `bridge` + `hydration` + `schema` form the sync core; `compression` wraps snapshot codec; `telemetry` provides metrics + health + spans; `config`/`constants`/`error` are leaf utilities. Note the deferred `app --> presence` edge — `broadcast_presence` is `unimplemented!()` per Phase 6 T1 user exclusion.

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

`grafeo-loro` is configured at build time via the fluent `GrafeoLoroAppBuilder` (in `src/app.rs`) and at runtime via the `AppConfig` struct (in `src/config.rs`). The builder is the production entry point — its setters validate inputs eagerly and reject invalid combos (e.g. `SsotMode::Grafeo` without `grafeo_dir`) at `.build().await?` time with `GrafeoLoroError::Config`. `AppConfig` is the plain-data struct that the builder would ideally thread through `from_sync_engine_with_telemetry` once the 8-arg constructor is refactored to a single config param (deferred to a future phase per the `#[allow(clippy::too_many_arguments)]` reason string).

The table below enumerates every configurable knob, its type, default value, and the architectural rationale. Defaults match architecture §24.4 (`SsotMode::Loro` for time-travel + minimal storage, `CompressionType::Zstd` for cold snapshots, `CompressionType::Lz4` for hot sync wire, 100 ms / 256 ops batcher). The `GrafeoLoroAppBuilder` setters mirror these fields one-for-one (`.ssot_mode(..)`, `.compression(..)`, `.sync_compression(..)`, `.batch_interval_ms(..)`, `.batch_max_size(..)`, `.grafeo_dir(..)`, `.with_metrics(..)`, `.with_health(..)`, `.with_tracer(..)`).

| Knob | Type | Default | Description |
|---|---|---|---|
| `ssot_mode` | `SsotMode` | `Loro` | Selects consensus SSOT. `Loro` = `.loro` snapshots + time-travel; `Grafeo` = `.tar.zst` snapshots + native indexes (Phase 5+). |
| `compression` | `CompressionType` | `Zstd` | Cold-snapshot codec. `None`/`Lz4`/`Zstd` — used by `checkpoint` + `hydrate` via `CompressedPayload`. |
| `sync_compression` | `CompressionType` | `Lz4` | Hot-sync wire codec (peer-to-peer Loro bytes). `Lz4` for low-latency decompression. |
| `batch_interval_ms` | `u64` | `100` | `MutationBatcher` flush cadence. Must be > 0 (validated in `build()`). |
| `batch_max_size` | `usize` | `256` | `MutationBatcher` flush threshold (op count). Must be > 0 (validated in `build()`). |
| `hydration_chunk_size` | `usize` | `256` (`DEFAULT_CHUNK_SIZE`) | Rayon parallel chunk size for `parallel_hydrate_grafeo`. |
| `max_staleness_ms` | `u64` | `5000` (`DEFAULT_STALENESS_MS`) | `HealthProbe::check` staleness threshold. |
| `enable_presence` | `bool` | `false` | WebSocket presence channel enable (Phase 6 T1 — currently `unimplemented!()`). |
| `presence_heartbeat_ms` | `u64` | — | Presence broadcast cadence (Phase 6 T1). |
| `grafeo_dir` | `Option<PathBuf>` | `None` | Required when `SsotMode::Grafeo`. `None` → in-memory `GrafeoDB::new_in_memory()`. |
| `storage` | `Option<Arc<dyn StorageBackend>>` | `None` | Required for production. `None` rejected by `build()` with `Config("storage backend not set")`. |
| `metrics` / `health` / `tracer` | `Option<Arc<...>>` | auto-constructed | Telemetry handles. `build()` auto-constructs from `opentelemetry::global` if unset. |

## Testing

The test suite is split into unit tests (`tests/unit/`) and integration tests (`tests/integration/`), totaling 82 tests (6 lib + 5 integration + 71 unit + 0 doctest, 2 ignored pre-existing). Unit tests cover individual modules in isolation; integration tests exercise the full `SyncEngine` pipeline including the Loro subscriber, inbound batcher, outbound CDC poller, and the epoch side-channel echo-prevention filter. All tests run on stable Rust 1.96+ via `cargo test --all`; the fuzz harness requires nightly + cargo-fuzz (see `docs/phase-6/fuzz-invariants.md`).

The unit-test crate (`tests/unit/main.rs`) wires 12 submodules, each focused on one component: `compression` (codec round-trips for None/Lz4/Zstd), `compression_payload` (on-wire format + version/codec-tag validation), `parallel_hydrate` (rayon chunking + property-type preservation + malformed-shape rejection), `schema_roundtrip` (lorosurgeon `Hydrate`/`Reconcile` derive round-trips), `tree_move` (`sync_tree_move_to_grafeo` parent→child edge direction + Serializable isolation), `vertex_builder` (fluent `create_vertex().with_label().with_property().commit()`), `vector_embedding` (`generate_local_embedding` stub + WARN-counter observability), `vector_offload` (`VectorOffloadManager` embedding-property bypass), `hydrate_checkpoint` (`GrafeoLoroApp::hydrate`/`checkpoint` cold-boot round-trip), `builder_validation` (`GrafeoLoroAppBuilder::build` config-rejection paths), `telemetry` (`MetricsRegistry` + `HealthProbe` + span factories). The integration-test crate covers `sync_echo` (B1 origin-filter + B2 epoch side-channel) and `tree_move_concurrency` (concurrent TreeMove ops under Serializable isolation).

- `cargo test --all` — run all 82 tests (lib + integration + unit). 2 ignored (pre-existing — ONNX smoke test + benchmark).
- `cargo test --test integration` — integration tests: sync echo, tree-move concurrency (5 tests).
- `cargo test --test unit` — unit tests: 71 tests across 12 submodules.
- `cargo test -- --ignored` — run tests marked `#[ignore]` (ONNX smoke test, perf benchmark).
- Fuzz harness: `cd fuzz && cargo +nightly fuzz run consistency` (requires nightly Rust + cargo-fuzz; see `docs/phase-6/fuzz-invariants.md` for the 16 invariants).
- `cargo clippy --all-targets -- -D warnings` — lint gate (CI-enforced via `.github/workflows/ci.yml` `clippy` job).
- `cargo fmt --all --check` — formatting gate (CI-enforced via `.github/workflows/ci.yml` `fmt` job).

## License

Licensed under either of <a href="LICENSE-MIT">Apache License, Version 2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option. The SPDX expression is `MIT OR Apache-2.0` (standard Rust dual-license per crates.io convention — allows downstream consumers to pick either terms). The full license texts are in `LICENSE-MIT` and `LICENSE-APACHE` at the repository root.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
