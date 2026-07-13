# grafeo-loro

> Local-first, in-process, dual-store graph database. CRDT consensus meets native execution.

## Stop Fighting Wrappers

Reimplementing native APIs kills momentum. Upstream updates break custom wrappers. State drifts. Echo loops burn CPU. Boilerplate multiplies.

`grafeo-loro` rejects the wrapper pattern. The crate exposes **raw handles** to the two underlying stores — [LoroDoc](https://loro.dev) for CRDT consensus and [GrafeoDB](https://crates.io/crates/grafeo) for graph + vector + BM25 execution — and runs a **transparent background bridge** that keeps them in lock-step. Users call native Grafeo and Loro APIs directly. The bridge syncs state silently.

- **Raw Grafeo Handle:** Execute native Cypher. Manage transactions. Use native vector indexing.
- **Raw Loro Handle:** Manipulate native Maps, Text, Trees. Leverage full CRDT power.
- **Invisible Bridge:** Background workers sync state. Epoch side-channel prevents echo loops. Origin filtering kills infinite recursion.
- **Zero Tradeoffs:** Full native speed. Full native features. Zero reimplemented APIs.

## Architecture

```text
[Client Code]
   |
   +---> app.grafeo_db()  -->  Native Grafeo Session / Cypher / Vector
   |       | (CDC events)
   |       v
   |     [Outbound Worker]  -->  Translates  -->  app.loro_doc()
   |
   +---> app.loro_doc()    -->  Native Loro Map / Text / Tree
           | (Diff events)
           v
         [Inbound Worker]  -->  Batches  -->  app.grafeo_db()
```

Users call native APIs. The bridge syncs state silently.

### What the Bridge Does (Background Magic)

1. **Inbound worker** — subscribes to Loro doc changes → translates to `LoroOp` → batches → applies to Grafeo.
2. **Outbound worker** — polls Grafeo CDC → translates to Loro mutations → writes to the Loro doc.
3. **Echo prevention** — an epoch side-channel + origin tagging prevents infinite loops: when the inbound worker commits to Grafeo, the resulting CDC event is tagged with the same epoch the worker just recorded, so the outbound worker skips it; when the outbound worker writes to Loro, the commit is tagged with `ORIGIN_LORO_BRIDGE` so the inbound subscriber filters it out.

The `bridge` module is **private**. Users never import from it directly — they interact via the raw handles (`grafeo_db()`, `loro_doc()`, `bridge_maps()`) and the bridge runs invisibly. A small number of bridge types (`BridgeMaps`, `SyncEngine`, `InboundMsg`) are re-exported at the crate root for advanced introspection and embedded scenarios.

### Module Dependency Graph

```mermaid
%% grafeo-loro module dependency graph
%% `bridge` is private (mod bridge; in src/lib.rs); types re-exported at crate root.
graph TD
    app
    bridge[bridge *private*]
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

    app --> bridge [label="SyncEngine, BridgeMaps"]
    app --> compression [label="CompressedPayload"]
    app --> config [label="CompressionType, SsotMode"]
    app --> constants [label="ORIGIN_LORO_BRIDGE, STORAGE_KEY_*"]
    app --> error [label="GrafeoLoroError, Result"]
    app --> hydration [label="parallel_hydrate_grafeo"]
    app --> storage [label="StorageBackend"]
    app --> telemetry [label="HealthProbe, MetricsRegistry, SharedTracer, HydrationMode"]

    bridge --> constants [label="EPOCH_RETENTION, ORIGIN_*, DEFAULT_BATCH_*, TREE_EDGE_LABEL, ROOT_*"]
    bridge --> error [label="GrafeoLoroError, Result"]
    bridge --> telemetry [label="HealthProbe, MetricsRegistry, SharedTracer"]
    bridge --> types [label="LoroOp, CdcEventWrapper, GraphValue, ..."]

    hydration --> bridge [label="apply_loro_op, BridgeMaps"]
    hydration --> constants [label="DEFAULT_CHUNK_SIZE, ORIGIN_LORO_BRIDGE, ROOT_VERTICES, ..."]
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

## Quick Start

```rust
use std::sync::Arc;
use grafeo_loro::{GrafeoLoroApp, SsotMode, CompressionType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let storage = Arc::new(MyStorageBackend);

    let app = GrafeoLoroApp::builder()
        .storage(storage)
        .ssot_mode(SsotMode::Loro)
        .compression(CompressionType::Zstd)
        .build()
        .await?;

    // 1. Native Grafeo execution — raw handle, no wrappers
    let db = app.grafeo_db();
    let mut session = db.session_with_cdc(true);
    session.begin_transaction()?;
    let node_id = session.create_node_with_props(
        &["Person"],
        [("name", grafeo::Value::String("Alice".into()))].into_iter()
    )?;
    let mut prepared = session.prepare_commit()?;
    prepared.set_metadata("origin", grafeo_loro::constants::ORIGIN_LORO_BRIDGE); // tag for bridge
    prepared.commit()?;
    // Outbound worker auto-syncs to Loro in the background.

    // 2. Native Cypher query
    let read_session = db.session();
    let results = read_session.execute("MATCH (p:Person) RETURN p.name")?;

    // 3. Native Loro CRDT manipulation — raw handle, no wrappers
    let doc = app.loro_doc().write();
    let custom_map = doc.get_map("my_state");
    custom_map.insert("key", "value")?;
    doc.commit();
    // Inbound worker auto-syncs to Grafeo in the background.

    app.shutdown().await?;
    Ok(())
}
```

The full flow:

- **Add the dep**: `cargo add grafeo-loro` (or add `grafeo-loro = "0.1"` to `Cargo.toml` `[dependencies]`).
- **Import the facade + native crates**: `use grafeo_loro::{GrafeoLoroApp, StorageBackend, CompressionType, SsotMode, grafeo, loro};`. The native `grafeo` and `loro` crates are re-exported at the `grafeo_loro` crate root so you never need a second dependency declaration.
- **Implement storage**: implement the `StorageBackend` trait (`async fn load/save/list/delete`) for your backend (filesystem, S3, IPFS — see `src/storage/traits.rs`).
- **Build the app**: `let app = GrafeoLoroApp::builder().storage(Arc::new(MyStorage)).ssot_mode(SsotMode::Loro).compression(CompressionType::Zstd).build().await?;` — this constructs a `GrafeoDB` + `LoroDoc` + `SyncEngine`, spawns the inbound batcher + outbound CDC poller + Loro subscriber as tokio tasks, and returns a ready-to-use app with raw handles.
- **Hydrate cold-boot state**: `app.hydrate("graph_123").await?` — restores the Loro snapshot from storage and rebuilds Grafeo indexes in parallel via rayon chunks.
- **Mutate via native APIs**: write to `app.grafeo_db()` or `app.loro_doc()` directly — the bridge syncs the other store in the background. Tag your Grafeo commits with `ORIGIN_LORO_BRIDGE` (metadata) and your Loro commits with `set_next_commit_origin(ORIGIN_LORO_BRIDGE)` so the bridge can identify and filter echoes.
- **Checkpoint**: `app.checkpoint("graph_123").await?` — exports a shallow Loro snapshot, compresses via `CompressedPayload::compress_to_wire(.., CompressionType::Zstd)`, and persists via `StorageBackend::save`.
- **Shutdown**: `app.shutdown().await?` — drains the inbound batcher, joins the three sync workers, releases the Loro subscription, and flushes telemetry exporters.
- **See working examples**: `tests/integration/sync_echo.rs` (sync echo + tree-move concurrency), `tests/unit/hydrate_checkpoint.rs` (cold-boot round-trip using native Loro APIs), `tests/unit/parallel_hydrate.rs` (Grafeo hydration).

## Public API Surface

The `GrafeoLoroApp` struct is intentionally tiny. It holds the `SyncEngine` (which owns the raw `GrafeoDB` + `LoroDoc` handles + bridge state) and exposes them via three accessors:

| Method | Returns | Purpose |
|---|---|---|
| `grafeo_db()` | `&Arc<grafeo::GrafeoDB>` | Raw Grafeo handle. Use native session / Cypher / vector APIs directly. |
| `loro_doc()` | `&Arc<parking_lot::RwLock<loro::LoroDoc>>` | Raw Loro handle. Use native CRDT types (Map / Text / Tree) directly. |
| `bridge_maps()` | `&Arc<BridgeMaps>` | Bridge id-mapping state for advanced introspection (`loro_key ↔ NodeId`, `EdgeKey ↔ EdgeId`). Optional — most users never need this. |
| `sync_engine()` | `&Arc<SyncEngine>` | Underlying engine handle. Used by integration tests to install the Loro subscriber and inspect `inbound_event_count` for echo-prevention verification. |
| `metrics()` | `Option<&Arc<MetricsRegistry>>` | OpenTelemetry metrics registry. `Some` in production, `None` in tests. |
| `health()` | `Option<&Arc<HealthProbe>>` | Health probe (Loro doc lock, Grafeo session, sync freshness). |
| `tracer()` | `Option<&SharedTracer>` | OpenTelemetry tracer. |
| `ssot_mode()` | `SsotMode` | Builder-configured SSOT mode. |
| `compression()` | `CompressionType` | Builder-configured snapshot codec. |
| `worker_handles()` | `Option<&[JoinHandle<()>]>` | Worker `JoinHandle`s, consumed by `shutdown()`. |
| `checkpoint(graph_id)` | `Result<()>` | Snapshot the Loro doc + persist via `StorageBackend`. |
| `hydrate(graph_id)` | `Result<()>` | Cold-boot: restore snapshot + rebuild Grafeo indexes. |
| `shutdown(self)` | `Result<()>` | Graceful shutdown: drain workers, flush telemetry. |

The crate root re-exports `grafeo` and `loro` (the native crates) plus `BridgeMaps`, `SyncEngine`, and `InboundMsg` so embedded scenarios and tooling can construct a `SyncEngine` directly without going through the builder.

### What was Deleted

The following wrappers were removed in favor of raw native handles (anti-plenger #11: deletion over addition):

- `create_vertex()` / `VertexBuilder` — use `app.grafeo_db().session_with_cdc(true).create_node_with_props(...)` or write to `app.loro_doc().write().get_map("V")` directly.
- `query(gql)` — use `app.grafeo_db().session().execute("MATCH (n) RETURN n")` directly.
- `update_text(node_id, field, text)` — use `app.loro_doc().write().get_text(...)` directly.
- `generate_embedding(node_id, field)` — use `VectorOffloadManager::handle_text_update` directly.
- `broadcast_presence(payload)` — Phase 5 WebSocket transport scope (still `unimplemented!()`; route around via the `presence` module).
- `loro_key_counter()` — was an artifact of the `VertexBuilder` key-generation strategy; users now generate their own Loro keys.
- `AppConfig` struct — zero callers; the `GrafeoLoroAppBuilder` is the sole construction path.

## Configuration

`grafeo-loro` is configured entirely via the fluent `GrafeoLoroAppBuilder` (in `src/app.rs`). There is no separate `AppConfig` struct — the builder IS the configuration surface. Its setters validate inputs eagerly and reject invalid combos (e.g. `SsotMode::Grafeo` without `grafeo_dir`) at `.build().await?` time with `GrafeoLoroError::Config`.

| Knob | Type | Default | Description |
|---|---|---|---|
| `ssot_mode` | `SsotMode` | `Loro` | Selects consensus SSOT. `Loro` = `.loro` snapshots + time-travel; `Grafeo` = `.tar.zst` snapshots + native indexes (Phase 5+ wal-feature scope). |
| `compression` | `CompressionType` | `Zstd` | Cold-snapshot codec. `None`/`Lz4`/`Zstd` — used by `checkpoint` + `hydrate` via `CompressedPayload`. |
| `sync_compression` | `CompressionType` | `Lz4` | Hot-sync wire codec (peer-to-peer Loro bytes). `Lz4` for low-latency decompression. |
| `batch_interval_ms` | `u64` | `100` | `MutationBatcher` flush cadence. Must be > 0 (validated in `build()`). |
| `batch_max_size` | `usize` | `256` | `MutationBatcher` flush threshold (op count). Must be > 0 (validated in `build()`). |
| `grafeo_dir` | `Option<PathBuf>` | `None` | Required when `SsotMode::Grafeo`. `None` → in-memory `GrafeoDB::new_in_memory()`. |
| `storage` | `Option<Arc<dyn StorageBackend>>` | `None` | Required for production. `None` rejected by `build()` with `Config("storage backend not set")`. |
| `with_metrics(..)` | `Option<Arc<MetricsRegistry>>` | auto-constructed | Telemetry handle. `build()` auto-constructs from `opentelemetry::global::meter("grafeo-loro")` if unset. |
| `with_health(..)` | `Option<Arc<HealthProbe>>` | auto-constructed | Health probe. `build()` auto-constructs from the freshly-built `loro_doc` + `grafeo_db` if unset. |
| `with_tracer(..)` | `Option<SharedTracer>` | auto-constructed | Tracer handle. `build()` auto-constructs from `opentelemetry::global::tracer("grafeo-loro")` if unset. |

Defaults match architecture §24.4 (`SsotMode::Loro` for time-travel + minimal storage, `CompressionType::Zstd` for cold snapshots, `CompressionType::Lz4` for hot sync wire, 100 ms / 256 ops batcher).

## Bulletproof Invariants

15 invariants are checked via the libFuzzer harness in `fuzz/fuzz_targets/consistency.rs`. State parity. Echo loops bounded. RYOW. Snapshot idempotency. MVCC snapshot isolation. Tree move serializability. Bridge map bijectivity.

```bash
cargo +nightly fuzz run consistency
```

Generate seed corpus:

```bash
cargo run --bin gen_corpus --manifest-path fuzz/Cargo.toml
```

See `docs/phase-6/fuzz-invariants.md` for the full invariant list.

## Deep Telemetry

OpenTelemetry metrics are built-in. `MetricsRegistry` (constructed in `build()` from `opentelemetry::global::meter("grafeo-loro")`) records:

- `inbound_events_total` — Loro subscriber events that survived the origin filter.
- `outbound_events_total` — Grafeo CDC events that survived the epoch side-channel filter.
- `echo_filtered_total` — events filtered out by origin / epoch checks.
- `batch_flush_duration_ms` — inbound batcher flush latency histogram.
- `hydration_duration_ms` — cold-boot hydration latency histogram (labelled by `HydrationMode::{Loro, Grafeo}`).

`HealthProbe` (constructed in `build()` from the freshly-built `loro_doc` + `grafeo_db`) tracks Loro doc lock contention, Grafeo DB session availability, and sync freshness (`last_sync_ts` stamped by both the inbound batcher after each flush and the outbound worker after each Loro commit — architecture §23.3).

`SharedTracer` (constructed in `build()` from `opentelemetry::global::tracer("grafeo-loro")`) opens spans for `cold_start_hydration`, `parallel_hydrate_grafeo`, `inbound_sync_loop`, `outbound_sync_loop`, and (Phase 6+) `hybrid_query`.

## Testing

The test suite is split into unit tests (`tests/unit/`) and integration tests (`tests/integration/`), totaling 72 tests (6 lib + 5 integration + 61 unit + 0 doctest; 2 ignored pre-existing). Unit tests cover individual modules in isolation; integration tests exercise the full `SyncEngine` pipeline including the Loro subscriber, inbound batcher, outbound CDC poller, and the epoch side-channel echo-prevention filter. All tests run on stable Rust 1.97+ via `cargo test --all`; the fuzz harness requires nightly + cargo-fuzz (see `docs/phase-6/fuzz-invariants.md`).

The unit-test crate (`tests/unit/main.rs`) wires 9 submodules, each focused on one component: `compression` (codec round-trips for None/Lz4/Zstd), `compression_payload` (on-wire format + version/codec-tag validation), `parallel_hydrate` (rayon chunking + property-type preservation + malformed-shape rejection), `schema_roundtrip` (lorosurgeon `Hydrate`/`Reconcile` derive round-trips), `tree_move` (`sync_tree_move_to_grafeo` parent→child edge direction + Serializable isolation), `vector_embedding` (`generate_local_embedding` stub + WARN-counter observability), `vector_offload` (`VectorOffloadManager` embedding-property bypass), `hydrate_checkpoint` (`GrafeoLoroApp::hydrate`/`checkpoint` cold-boot round-trip using **native Loro APIs** via the raw `loro_doc()` handle), `builder_validation` (`GrafeoLoroAppBuilder::build` config-rejection paths), `telemetry` (`MetricsRegistry` + `HealthProbe` + span factories). The integration-test crate covers `sync_echo` (B1 origin-filter + B2 epoch side-channel) and `tree_move_concurrency` (concurrent TreeMove ops under Serializable isolation).

- `cargo test --all` — run all 72 tests (lib + integration + unit). 2 ignored (pre-existing — ONNX smoke test + benchmark).
- `cargo test --test integration` — integration tests: sync echo, tree-move concurrency (5 tests).
- `cargo test --test unit` — unit tests: 61 tests across 9 submodules.
- `cargo test -- --ignored` — run tests marked `#[ignore]` (ONNX smoke test, perf benchmark).
- Fuzz harness: `cd fuzz && cargo +nightly fuzz run consistency` (requires nightly Rust + cargo-fuzz; see `docs/phase-6/fuzz-invariants.md` for the 16 invariants).
- `cargo clippy --all-targets -- -D warnings` — lint gate (CI-enforced via `.github/workflows/ci.yml` `clippy` job).
- `cargo fmt --all --check` — formatting gate (CI-enforced via `.github/workflows/ci.yml` `fmt` job).

## Roadmap

- **Phase 1:** Core bridge, raw handles, background sync, echo prevention. *(Complete)*
- **Phase 2:** Telemetry, fuzzing harness, 15 invariant proofs. *(Complete)*
- **Phase 3:** Native Grafeo vector indexing integration. Persistent storage backends (S3/RocksDB).
- **Phase 4:** Distributed P2P sync transport. Multi-peer conflict resolution tuning.
- **Phase 5:** Native Loro Tree optimizations. Advanced CRDT move semantics. `SsotMode::Grafeo` checkpoint/hydrate (requires `wal` feature + `ArcSwap<GrafeoDB>` field). WebSocket presence transport.

## License

Licensed under either of <a href="LICENSE-APACHE">Apache License, Version 2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option. The SPDX expression is `MIT OR Apache-2.0` (standard Rust dual-license per crates.io convention — allows downstream consumers to pick either terms). The full license texts are in `LICENSE-MIT` and `LICENSE-APACHE` at the repository root.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
