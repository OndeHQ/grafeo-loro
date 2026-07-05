# Plonga-Plongo-Loop Worklog

**Repository**: grafeo-loro (cloned from https://github.com/OndeHQ/grafeo-loro)
**Phase**: Phase 1 — Core Glue & Echo Prevention (FULL phase, all 4 tasks + validation)
**$stn (current loop scope)**: `phase-1` (user override: bypassed the "pick ONE task" rule)
**Branch**: `phase-1`
**Base commit**: `1ce13e0 Update grafeo-loro.architecture.md`

## Phase 1 Scope (all tasks)

Per `docs/implementation-plan.md`:

1. **`types::values::lval_to_gval`**
   - Map `LoroValue::{Map, List, String, I64, F64, Bool, Null}` → `GraphValue`
   - Handle nested maps recursively
   - Panic/error on unsupported types (Binary, Container)

2. **`bridge::origin` checks**
   - Wire into `sync_engine.rs` subscriber filter
   - Wire into `batcher.rs` CDC listener

3. **`bridge::sync_engine` MPSC loops**
   - `init_loro_subscriber`: Filter origin, push to channel
   - `spawn_inbound_worker`: Recv loop, batch ops, commit Grafeo tx
   - `spawn_outbound_worker`: Recv CDC, filter origin, transact Loro

4. **`bridge::batcher` flush logic**
   - Time/count trigger via `tokio::select!`
   - Vectorized upsert/delete in single Grafeo tx
   - Set `ORIGIN_LORO_BRIDGE` metadata on tx

### Validation
- Unit test: Echo loop prevention (mock Loro+Grafeo, verify no infinite recursion)
- Integration test: Bidirectional sync with artificial delay

## Loop Plan

Per `plonga-plongo-loop.md` (one $stn loop covers ALL Phase 1 tasks):
1. L1 scaffolding (contracts only — no logic)  ← `Task ID: L1`
2. Devil's advocate critique + solution          ← `Task ID: DEVIL`
3. Fixer (L2 evolving/reducing scaffolds)         ← `Task ID: FIX-L2`
4. L3 deep implementation (zero TODOs)            ← `Task ID: L3`
5. Plenger hunter (find anti-patterns)            ← `Task ID: HUNT`
6. Back to step 3 if issues found, else push

## Shared Rules (all sub-agents must comply)

- Read `repomix.md` first (signature-based read-only context). Update with `cd /home/z/my-project/repos/grafeo-loro && repomix --output repomix.md` if needed.
- Prefer `grep -n` over individual file reads for context efficiency.
- ALWAYS `cd /home/z/my-project/repos/grafeo-loro &&` before any git/cargo/repomix command — bash session resets cwd between calls.
- Push to `phase-1` branch on significant progress.
- Never create another branch.
- Never rebase in a way that drops/overwrites others' commits.
- Comply with `klemer-agents.md` layer rules (L1: contracts only; L2: wiring+skeleton; L3: meat).
- Comply with `anti-plenger.md` decisions (pure functions, DRY, YAGNI, immutability, polymorphism over conditionals, observability, idempotency, fewest LOC, deletion over addition, native-first, oneline code/doc first, never simplify basics).
- Avoid `plenger-traits.md` anti-patterns: backward-compat slaves, tautology, context blindness, band-aids, bloat/DRY violations, hallucination, happy-path bias, Goodhart's law.

## Key Codebase Facts (from repomix scan)

- `src/types/values.rs`: `LoroProperty` enum (Null/Bool/I64/F64/String), `GraphValue` enum (Null/Bool/I64/F64/String/Vector/Map), `lval_to_gval` stub.
- `src/constants.rs`: `ORIGIN_GRAFEO_BRIDGE="grafeo-bridge"`, `ORIGIN_LORO_BRIDGE="loro-bridge"`, root keys, defaults.
- `src/error.rs`: `GrafeoLoroError` with variants Loro/Grafeo/StorageIo/Compression/ChannelClosed/Config. No `UnsupportedLoroType` yet.
- `src/bridge/{sync_engine.rs, batcher.rs, origin.rs}`: skeletons exist (need inspection by L1 agent).
- Dependencies: `loro=1.0`, `grafeo=0.5`, `lorosurgeon=0.3`, `tokio`, `parking_lot`, `rayon`, `async-trait`, `thiserror`, `opentelemetry`.

---

## Task L1 — Scaffolding (contracts only)

---
Task ID: L1
Agent: L1 Scaffolding
Task: Write contracts (interfaces/types/signatures only) for all 4 Phase 1 tasks

Work Log:
- Read `worklog.md`, `repomix.md`, and inspected existing skeletons via `grep -n` / file reads.
- Inspected grafeo 0.5.42 source (vendored in `~/.cargo/registry`): confirmed `grafeo::cdc::CdcEvent` does NOT exist — actual type is `grafeo::cdc::ChangeEvent`. Fixed `CdcEventWrapper.payload` type accordingly.
- Inspected loro 1.13.6 source: confirmed `LoroValue` variants are `Null / Bool / Double / I64 / Binary / String / List / Map / Container`. The spec's `F64` is actually `Double`; `lval_to_gval` body (L3) will need to map `Double → GraphValue::Float`.
- Inspected grafeo `Value` enum (grafeo-common): it has `Int64`/`Float64`/`List`/`Map`/`Vector`/... — confirms that adding `GraphValue::List` is consistent with the Grafeo execution layer.
- **Task 1 (values)**: `GraphValue::List(Vec<GraphValue>)` added. Rationale: Loro's `LoroValue::List` exists, Grafeo's `Value::List` exists, and the implementation plan explicitly lists `List` as supported. Rejecting it would lose data on round-trip. Decision (a) over (b).
- **Task 1 (error story)**: `GrafeoLoroError::UnsupportedLoroType(String)` added with `#[error("Unsupported LoroValue type: {0}")]`. `lval_to_gval` signature changed to `pub fn lval_to_gval(val: loro::LoroValue) -> crate::error::Result<GraphValue>` (returns `Result`, no panic).
- **Task 1 (docs)**: one-line rustdoc added to `LoroProperty`, `GraphValue`, `lval_to_gval`. Also added `#[derive(Hydrate, Reconcile)]` to `LoroProperty` so the existing schema skeletons compile.
- **Task 2 (origin.rs)**: kept existing `is_grafeo_bridge_origin`/`is_loro_bridge_origin` (still useful — they answer single-bridge-specific questions for inbound vs outbound workers) and added `is_bridge_origin(&str) -> bool` and `is_echo(&str, &str) -> bool`. All bodies `unimplemented!()`.
- **Task 2 (filter types)**: declared `pub type LoroSubscriberFilter = Arc<dyn Fn(&str) -> bool + Send + Sync>` in `sync_engine.rs`; declared `pub type CdcEventFilter = Arc<dyn Fn(Option<&str>) -> bool + Send + Sync>` in `batcher.rs` (per spec). No wiring.
- **Task 3 (sync_engine)**: declared `pub enum InboundMsg { Op(LoroOp) }` and `pub enum OutboundMsg { Cdc(CdcEventWrapper) }`. `SyncEngine` fields: `grafeo_db: Arc<GrafeoDB>`, `loro_doc: Arc<RwLock<LoroDoc>>`, `inbound_tx`/`inbound_rx: tokio::sync::Mutex<mpsc::Receiver<...>>`, `outbound_tx`/`outbound_rx: tokio::sync::Mutex<mpsc::Receiver<...>>`, `shutdown: CancellationToken`. Methods: `new`, `init_loro_subscriber`, `spawn_inbound_worker(self: Arc<Self>) -> JoinHandle<()>`, `spawn_outbound_worker(self: Arc<Self>) -> JoinHandle<()>`, plus `outbound_sender()` and `shutdown()` helpers.
- **Task 3 (grafeo handle type)**: kept `Arc<GrafeoDB>` — GrafeoDB manages internal locks (RwLock + Arc fields), so external `Mutex` is unnecessary. Noted in worklog for L2 verification.
- **Task 3 (loro handle type)**: kept `Arc<RwLock<LoroDoc>>` per architecture doc §8. Loro's `subscribe_root` takes `&self` so a read guard suffices for subscribe; mutations need a write guard.
- **Task 4 (batcher)**: `MutationBatcher` fields: `grafeo_db: Arc<GrafeoDB>`, `buffer: Vec<LoroOp>`, `batch_size: usize`, `batch_ms: u64`, `flush_notify: Arc<tokio::sync::Notify>` (for size-threshold wake), `shutdown: CancellationToken`. Methods: `new(grafeo_db, batch_size, batch_ms)`, `with_defaults(grafeo_db)`, `push(&mut self, op) -> Result<()>`, `run(self) -> Result<()>`, `flush(&mut self) -> Result<()>` (private). Declared `BatchedOp` helper enum for vectorized flush grouping.
- **Task 4 (buffer type)**: chose `Vec<LoroOp>` over `Vec<OutboundMsg>` — the batcher is inbound-only (Loro→Grafeo per architecture §20). `OutboundMsg` is for the Grafeo→Loro path which doesn't go through this batcher.
- **Validation scaffolding**: created `tests/integration/main.rs` (modern Rust 2018+ layout) with `mod sync_echo;`, and `tests/integration/sync_echo.rs` with two `#[tokio::test] #[ignore]` functions: `echo_loop_prevention` and `bidirectional_sync_with_delay`. Bodies are `todo!()`.
- **Cargo.toml changes**: (1) `lorosurgeon = "0.3"` → `"0.2"` (0.3 doesn't exist on crates.io, 0.2.1 is latest). (2) Added `tokio-util = { version = "0.7", features = ["rt"] }` for `CancellationToken`. (3) Added `features = ["metrics", "trace"]` to `opentelemetry` (metrics is not default-enabled in 0.23). (4) Added `[dev-dependencies] tokio = { ..., "test-util" }` for integration tests.
- **Non-Phase-1 skeleton fixes** (required for `cargo check` to pass — pre-existing skeletons had `pub fn foo();` bodies which are invalid Rust in impls/free functions, plus other type errors): added `unimplemented!()` bodies and one-line docs to `src/app.rs`, `src/compression/wrapper.rs`, `src/config.rs`, `src/hydration/{parallel,vector}.rs`, `src/presence/socket.rs`, `src/schema/tree.rs`, `src/telemetry/{health,metrics,traces}.rs`. Fixed `&impl Tracer` → `<T: Tracer>(&T)` and `Span` (trait) → `BoxedSpan` (concrete struct from `opentelemetry::global`) in `traces.rs`.
- Ran `cargo check --all-targets`: **0 errors, 15 warnings** (all `unused_variable`/`unused_mut`/`field never read` — expected since bodies are `unimplemented!()`).

Stage Summary:
- **Error story**: `Result<GraphValue, GrafeoLoroError>` with new `UnsupportedLoroType(String)` variant. No panics.
- **`GraphValue::List`**: ADDED. Rationale: Loro `List` + Grafeo `List` both exist; rejecting would lose round-trip data.
- **Channel types**: `InboundMsg { Op(LoroOp) }`, `OutboundMsg { Cdc(CdcEventWrapper) }`. Both use `tokio::sync::mpsc` with capacity chosen by L2 (suggested 1024 per architecture §10 example).
- **`SyncEngine` fields**: `grafeo_db: Arc<GrafeoDB>`, `loro_doc: Arc<RwLock<LoroDoc>>`, `inbound_tx`, `inbound_rx: tokio::sync::Mutex<Receiver<InboundMsg>>`, `outbound_tx`, `outbound_rx: tokio::sync::Mutex<Receiver<OutboundMsg>>`, `shutdown: CancellationToken`.
- **`MutationBatcher` fields**: `grafeo_db: Arc<GrafeoDB>`, `buffer: Vec<LoroOp>`, `batch_size: usize`, `batch_ms: u64`, `flush_notify: Arc<Notify>`, `shutdown: CancellationToken`.
- **Grafeo handle type**: `Arc<GrafeoDB>` (internally thread-safe, no external Mutex needed).
- **Files touched** (Phase 1): `Cargo.toml`, `src/error.rs`, `src/types/{values,events}.rs`, `src/bridge/{origin,sync_engine,batcher}.rs`, `tests/integration/{main,sync_echo}.rs`.
- **Files touched** (non-Phase-1, needed for compile): `src/app.rs`, `src/config.rs`, `src/compression/wrapper.rs`, `src/hydration/{parallel,vector}.rs`, `src/presence/socket.rs`, `src/schema/tree.rs`, `src/telemetry/{health,metrics,traces}.rs`.
- **`cargo check` result**: ✅ PASS (0 errors, 15 warnings — all expected from `unimplemented!()` bodies).
- **Open questions for Devil's advocate**:
  1. `InboundMsg` has a single variant `Op(LoroOp)`. Should we add a `RawDiff { origin, batch: loro::event::DiffBatch }` variant to push translation work into the worker instead of the sync subscriber callback? (Trade-off: subscriber handler does less work but the worker needs to know about Loro internals.)
  2. `push(&mut self, op)` + `run(self)` API on `MutationBatcher` is awkward — `run` consumes `self` so external `push` calls can't happen concurrently. L2 should probably restructure as `Arc<Mutex<Self>>` + `run(self: Arc<Self>)`, OR add a separate `Sender<LoroOp>` returned from `new`. Current L1 contract follows the spec literally.
  3. `CdcEventFilter` declared in `batcher.rs` per spec, but the batcher is inbound (Loro→Grafeo). The CDC filter is actually consumed by the outbound worker in `sync_engine.rs`. Should the alias be moved to `origin.rs` (its natural home) or `sync_engine.rs` (where it's used)? Currently deferred to L2.
  4. `LoroProperty` now derives `Hydrate + Reconcile` to satisfy schema skeletons. The `#[serde(untagged)]` attr is preserved but lorosurgeon ignores it. L2 should verify the derive-generated LoroMap shape round-trips correctly with the schema's `HashMap<String, LoroProperty>` fields.
  5. `loro::event::DiffEvent<'a>` has a lifetime — the subscriber callback receives a borrowed event. The L1 contract uses `InboundMsg::Op(LoroOp)` (owned) which sidesteps the lifetime issue, but L2 must verify the subscriber handler can synchronously translate `DiffEvent<'a>` → `LoroOp` without holding borrows across `.await`.
  6. `grafeo::cdc::ChangeEvent` has many fields (entity_id, kind, epoch, before, after, labels, edge_type, ...). L2 must decide which fields the outbound worker reads to construct Loro mutations — the L1 `CdcEventWrapper` wraps the entire event without projection.
  7. Added `tokio-util` as a new dep for `CancellationToken`. Acceptable per spec ("preferred"), but adds a transitive dep tree. Alternative: `tokio::sync::broadcast::Sender<()>` (no new dep, less ergonomic).
  8. `OutboundMsg::Cdc(CdcEventWrapper)` adds a layer of indirection — could simplify to `pub type OutboundMsg = CdcEventWrapper`. Kept the enum per spec to allow L2 to add variants (e.g. `Flush`, `Tick`) without breaking changes.
