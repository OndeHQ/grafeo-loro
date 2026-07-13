# P5-L1 Devil Advocate Critique

**Date**: 2026-07-06
**Reviewer**: P5-DEVIL agent
**L1 commit range**: `9cdc60b..5a03a14` (worklog hash `8baf278`)
**L1 questions resolved**: 17/17
**Verdict**: PROCEED_TO_L2

## 1. Scope Alignment Audit

Phase 5 Task 2/3/4 deliverables (Task 1 Presence SKIPPED per user) verified against `docs/implementation-plan.md` Phase 5 + `docs/grafeo-loro.architecture.md` §23:

- **Task 2 — `telemetry::metrics::MetricsRegistry`**: ✅ 5 instrument fields present (`inbound_events`, `outbound_events`, `echo_filtered` Counters; `batch_flush_duration`, `hydration_duration` Histograms) at `src/telemetry/metrics.rs`. Names match §23.1 rows 1–5. **GAP**: §23.1 rows 6–11 (`grafeo.query_duration_ms`, `grafeo.epoch_number`, `grafeo.active_readers`, `compression.ratio`, `bridge.queue_depth`, `presence.peers_connected`) are NOT scaffolded. These are GRAFEO-internal/compression/presence concerns, out of Phase 5 Task 2 scope (Task 2 = "sync.*" namespace only). **Acceptable** — confirm out-of-scope, defer to future phases.
- **Task 3 — `telemetry::health::HealthProbe::check`**: ✅ struct + `check` + `update_sync_ts` + `new` signatures present at `src/telemetry/health.rs:45+`. **GAP**: arch §23.3 line 1080 uses `self.db.execute(...)` which does NOT exist in grafeo 0.5.42 — see §5 API Verification (M1).
- **Task 4 — wire telemetry into bridge/batcher/hydration/app**: ✅ contact points present in `src/bridge/sync_engine.rs` (4 worker methods, 9 TODOs), `src/bridge/batcher.rs` (`flush_inner` 2 TODO sites), `src/hydration/parallel.rs` (2 TODOs), `src/app.rs` (hydrate + build + shutdown call sites). 22 `// TODO(P5-L2):` markers — accurate count per L1 worklog step 17.

**Scope conclusion**: All Task 2/3/4 deliverables scaffolded. No scope gaps requiring L1 rework.

## 2. Architecture Alignment Audit

| Arch §X | Requirement | L1 Status | Notes |
|---|---|---|---|
| §23.1 rows 1–5 | 5 sync.* metrics | ✅ Present | `metrics.rs` fields match names exactly |
| §23.2 tree row 1 | `cold_start_hydration` parent + `decompress_snapshot`/`import_loro_doc`/`parallel_hydrate_grafeo` children + `hydrate_chunk` grandchild | ⚠️ Partial | Only parent `cold_start_hydration` has helper; children/grandchildren deferred to L2 — acceptable per L1 contract-only mandate |
| §23.2 tree row 2 | `inbound_sync_loop` parent + `receive_loro_event` + `batch_flush` child + **`grafeo_commit` grandchild** | ⚠️ Partial | Helper for parent present; **`grafeo_commit` grandchild NOT scaffolded in batcher TODO** — see Q9 (M2) |
| §23.2 tree row 3 | `outbound_sync_loop` parent + `receive_cdc_event` + `loro_commit` children | ❌ Missing helper | **No `create_outbound_sync_span` helper** — see Q13 (M3) |
| §23.2 tree row 4 | `user_mutation` + children | N/A | Out of Phase 5 scope (no user-mutation API in grafeo-loro yet) |
| §23.2 tree row 5 | `hybrid_query` + children | ⚠️ Partial | Parent helper present; children deferred to L2 — acceptable |
| §23.3 | `HealthProbe` struct + `check` 3-component probe | ⚠️ Partial | Struct + check signature present; **API call wrong** — see §5 (M1) |
| §23.4 | Structured logging (INFO + WARN list) | ✅ Documented | Q5 ruling: health check silent (WARN list excludes health) |
| §23.5 | Prometheus alerting rules | N/A | Out of L1 scope (alert manager config, not code) |

## 3. Open Question Rulings (Q1–Q17)

### Q1: `GrafeoLoroApp.metrics` is `Option<Arc<MetricsRegistry>>` (L1) vs spec `Arc<MetricsRegistry>` (non-Option)
**Ruling**: **Accept `Option<Arc<MetricsRegistry>>`** (L1 decision upheld).
**Rationale**: Backward compat with 4-arg `from_sync_engine_with_config` test API (7+ test call sites). The constructor choice (`from_sync_engine_with_config` vs `from_sync_engine_with_telemetry`) IS the telemetry-enabled flag — no separate flag needed.
**Source**: `src/app.rs:86` (field def) + `src/app.rs:252` (`from_sync_engine_with_telemetry` 8-arg signature).

### Q2: `HealthProbe.doc` field type — `Arc<RwLock<LoroDoc>>` (L1) vs `Weak<RwLock<LoroDoc>>`
**Ruling**: **`Arc<RwLock<LoroDoc>>`** (L1 upheld).
**Rationale**: App owns `loro_doc` via `SyncEngine.loro_doc` for the app's entire lifetime; `Weak` would always upgrade successfully in practice (no real ownership cycle risk). Arc matches arch §23.3 line 1068 verbatim + is simpler.
**Source**: `docs/grafeo-loro.architecture.md:1068` + `src/telemetry/health.rs:45`.

### Q3: `SharedTracer = Arc<BoxedTracer>` (L1) vs `Option<BoxedTracer>` per-owner
**Ruling**: **`Arc<BoxedTracer>`** (L1 upheld).
**Rationale**: Verified `BoxedTracer` is `Send + Sync` but NOT `Clone` (`opentelemetry-0.23.0/src/global/trace.rs:244`). Arc gives shared ownership across SyncEngine + MutationBatcher + GrafeoLoroApp without forcing each owner to call `global::tracer("grafeo-loro")` independently (which would allocate 3 separate BoxedTracer instances).
**Source**: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/opentelemetry-0.23.0/src/global/trace.rs:244` (BoxedTracer impl block — no Clone derive) + `src/telemetry/mod.rs` (`SharedTracer` alias).

### Q4: `HealthProbe::check` calls `self.db.execute("MATCH (n) RETURN count(n) LIMIT 1")` per arch §23.3 line 1080 — does `GrafeoDB::execute(gql: &str)` exist in grafeo 0.5.42?
**Ruling**: **`GrafeoDB::execute(&str)` does NOT exist.** Correct API is `self.db.session().execute("MATCH (n) RETURN count(n) LIMIT 1")`.
**Rationale**: Physical inspection of grafeo-engine-0.5.42 `src/database/mod.rs` shows `GrafeoDB` has `session() -> Session` (line 1663), `session_with_cdc(bool)` (line 1728), `session_read_only()` (line 1745) — but NO `execute(&str)` method on `GrafeoDB`. `Session::execute(&self, query: &str) -> Result<QueryResult>` is at `src/session/mod.rs:2636`. The architecture §23.3 line 1080 is **out of sync with the actual grafeo 0.5.42 API**.
**Source**: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/grafeo-engine-0.5.42/src/database/mod.rs:1663` (`pub fn session(&self) -> Session`) + `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/grafeo-engine-0.5.42/src/session/mod.rs:2636` (`pub fn execute(&self, query: &str) -> Result<QueryResult>`).
**L2 action**: Implement `HealthProbe::check` using `self.db.session().execute("MATCH (n) RETURN count(n) LIMIT 1").is_ok()`. NO need for `session_with_cdc(false)` (read-only COUNT query doesn't trigger CDC writes). Also: file an arch-doc follow-up to correct §23.3 line 1080.

### Q5: `HealthProbe::check` — log WARN per failing component, or silent return?
**Ruling**: **Silent return** (L1 recommendation upheld).
**Rationale**: Arch §23.4 line 1113–1117 lists WARN/ERROR events: echo loops (WARN), batch flush backpressure (WARN), Block-STM abort rate >10% (ERROR), Loro import failure (ERROR). Health-check polling is NOT in this list — health is a passive read endpoint; if it logged WARN on every failing component, a single broken component would spam logs every poll interval. The HTTP endpoint wrapper (Phase 6+) decides log level based on `HealthStatus.overall`.
**Source**: `docs/grafeo-loro.architecture.md:1113-1117` (WARN/ERROR list — health absent).

### Q6: `MetricsRegistry::record_hydration(duration_ms, mode: &str)` vs `HydrationMode` enum
**Ruling**: **`HydrationMode` enum** (`Loro` / `Grafeo`) with `impl Display` rendering `"loro"` / `"grafeo"`.
**Rationale**: Enum prevents typos at compile time; `Display` impl renders the OTLP attribute value. Matches anti-plenger "Polymorphism over Conditionals" + "Observability" (no stringly-typed APIs).
**Source**: `docs/grafeo-loro.architecture.md:1028` (row 5 labels `mode` (`loro`/`grafeo`)) + `anti-plenger.md` Polymorphism-over-Conditionals.
**L2 action**: Add `pub enum HydrationMode { Loro, Grafeo }` + `impl Display` to `metrics.rs`; change `record_hydration` signature to `(duration_ms: f64, mode: HydrationMode)`.

### Q7: `traces::create_*_span<T: Tracer>(tracer: &T)` generic vs concrete `&SharedTracer`
**Ruling**: **Keep generic** `<T: Tracer>(tracer: &T)`.
**Rationale**: Tests can inject `opentelemetry::trace::NoopTracer` (verified exists in opentelemetry 0.23 — `opentelemetry::trace::NoopTracer`). Production auto-derefs `Arc<BoxedTracer>` to `&BoxedTracer` which satisfies `Tracer`. Generic is strictly more flexible with zero runtime cost (monomorphization).
**Source**: `src/telemetry/traces.rs` (3 generic fns) + `opentelemetry 0.23` `trace::NoopTracer`.

### Q8: Span names `cold_start_hydration`, `inbound_sync_loop`, `hybrid_query` — exact match to arch §23.2?
**Ruling**: **Confirmed exact match** — no renames.
**Rationale**: Arch §23.2 lines 1040, 1045, 1056 use these names verbatim. L1 doc-comments pin them correctly. Additional names L2 must use: `outbound_sync_loop` (line 1050), `batch_flush` (line 1047), `grafeo_commit` (line 1048), `receive_loro_event` (line 1046), `receive_cdc_event` (line 1051), `loro_commit` (line 1052), `decompress_snapshot` (line 1041), `import_loro_doc` (line 1042), `parallel_hydrate_grafeo` (line 1043), `hydrate_chunk` (line 1044), `hnsw_search` (line 1057), `graph_traversal` (line 1058), `local_grafeo_write` (line 1054), `local_loro_commit` (line 1055).
**Source**: `docs/grafeo-loro.architecture.md:1038-1059`.

### Q9: Should batcher emit `grafeo_commit` grandchild span under `batch_flush`?
**Ruling**: **REWORK — `grafeo_commit` grandchild IS required** (L1's YAGNI suggestion rejected).
**Rationale**: Arch §23.2 line 1048 explicitly shows `│   │   └── span: grafeo_commit` as a grandchild of `batch_flush`. L1's `batcher.rs:233` TODO already flags this as conditional ("Record `grafeo_commit` as a grandchild if architecture") — Devil confirms arch requires it. YAGNI does NOT apply when the spec explicitly lists the span.
**Source**: `docs/grafeo-loro.architecture.md:1048` + `src/bridge/batcher.rs:233`.
**L2 action**: In `flush_inner`, after `batch_flush` parent span opens, create `grafeo_commit` grandchild span around the actual `commit()` call.

### Q10: Does `MutationBatcher` need a `health: Option<Arc<HealthProbe>>` field?
**Ruling**: **YES — both `MutationBatcher` AND `SyncEngine` need a `health` field** (L1 partial — SyncEngine field missing).
**Rationale**: Arch §23.3 "Last sync" semantic covers BOTH inbound (batcher flush → Grafeo commit) AND outbound (CDC poll → Loro commit). L1 added `health` TODO at `sync_engine.rs:467` (`let health = self.health.clone();`) but **did NOT add a `health` field to `SyncEngine`** struct (lines 96-151 list only `metrics` + `tracer`). This is a contract bug — L2 will hit a compile error or be confused. Symmetric with `metrics` + `tracer` pattern.
**Source**: `src/bridge/sync_engine.rs:96-151` (struct — NO `health` field) vs `src/bridge/sync_engine.rs:467` (TODO references `self.health`). See M2.
**L2 action**: Add `pub(crate) health: Option<Arc<HealthProbe>>` to `SyncEngine` struct + `MutationBatcher` struct. Thread through `new_inner` + `with_telemetry` (extend signature from 6 args to 7 args: add `health: Option<Arc<HealthProbe>>`). Add accessor `health()`.

### Q11: Deprecate `SyncEngine::with_batch_config` now that `with_telemetry` exists?
**Ruling**: **Keep both** (L1 recommendation upheld).
**Rationale**: `with_batch_config` is the 4-arg test-friendly form (no telemetry construction needed). `with_telemetry` is the 6-arg production form. Per anti-plenger "Same logic, fewest LOC" — adding `#[deprecated]` would force 7+ test sites to migrate to `with_telemetry(None, None)` which is MORE LOC for no benefit. Backward-compat slave trap avoided (the API is not actually deprecated — both have legitimate use cases).
**Source**: `src/bridge/sync_engine.rs:198` (`with_batch_config`) + `src/bridge/sync_engine.rs:225` (`with_telemetry`).

### Q12: `inbound_event_count: Arc<AtomicU64>` (subscriber boundary, test-only) vs OTel `inbound_events` counter (per-op batcher boundary) — redundant?
**Ruling**: **Both coexist** (L1 analysis upheld).
**Rationale**: Different boundaries, different purposes: (a) `inbound_event_count` at `init_loro_subscriber` handler — counts every non-echo event that survives origin filter + enters the mpsc channel. Used by tests for deterministic echo assertions (Hunter MAJOR 3 fix). (b) OTel `inbound_events` counter at `spawn_inbound_worker` per-op-forward-to-batcher — counts ops with labels `origin` + `event_type` (per §23.1 row 1). Labels are NOT available at subscriber boundary (subscriber only sees raw Loro events, not translated op metadata). Removing either breaks its consumer.
**Source**: `src/bridge/sync_engine.rs:125` (`inbound_event_count` field) + `docs/grafeo-loro.architecture.md:1024` (row 1 labels) + `src/bridge/sync_engine.rs:424` (TODO for OTel counter in inbound worker).

### Q13: Should `traces.rs` add `create_outbound_sync_span` for symmetry?
**Ruling**: **YES — add it** (L1 recommendation upheld, MAJOR gap).
**Rationale**: Arch §23.2 lines 1050-1052 explicitly define `outbound_sync_loop` parent + `receive_cdc_event` + `loro_commit` children. L1's `spawn_outbound_worker` TODO at `sync_engine.rs:470-471` references `/* create_outbound_sync_span(t.as_ref()) — Devil Q13 */` — i.e., L1 explicitly deferred this helper pending Devil ruling. Devil rules: add it. Without it, the outbound worker has no parent span, breaking the §23.2 tree row 3 contract.
**Source**: `docs/grafeo-loro.architecture.md:1050-1052` + `src/bridge/sync_engine.rs:451-452` (L1 comment: "note: NO `create_outbound_sync_span`") + `src/bridge/sync_engine.rs:471` (TODO marker).
**L2 action**: Add `pub fn create_outbound_sync_span<T: Tracer>(tracer: &T) -> BoxedSpan` to `src/telemetry/traces.rs`. Use in `spawn_outbound_worker` parent + create `receive_cdc_event` + `loro_commit` children inline.

### Q14: Auto-construct `MetricsRegistry::init(global::meter("grafeo-loro"))` in `build()` if `.with_metrics(...)` not called?
**Ruling**: **YES — auto-construct in `build()` if `None`**.
**Rationale**: Verified `opentelemetry::global::meter(name)` exists at `opentelemetry-0.23.0/src/global/metrics.rs:115` (signature `pub fn meter(name: impl Into<Cow<'static, str>>) -> Meter`). Production ergonomics: caller shouldn't have to construct OTel boilerplate just to get default metrics. Tests use `from_sync_engine_with_config` (4-arg, no telemetry) — bypasses `build()` entirely, so no test breakage. The constructor choice (`build()` vs `from_sync_engine_with_config`) IS the telemetry-enabled flag.
**Source**: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/opentelemetry-0.23.0/src/global/metrics.rs:115` + `src/app.rs:160` (Builder Default — `metrics: None`).
**L2 action**: In `build()`, after constructing `loro_doc` + `grafeo_db`, if `self.metrics.is_none()` then `self.metrics = Some(Arc::new(MetricsRegistry::init(opentelemetry::global::meter("grafeo-loro"))));`.

### Q15: Should `shutdown()` auto-checkpoint before draining workers?
**Ruling**: **NO auto-checkpoint** (L1 recommendation upheld).
**Rationale**: Arch §4 Step D is silent on shutdown semantics. Separation of concerns: `shutdown()` cancels workers + flushes telemetry + closes GrafeoDB; caller decides if a final `checkpoint()` is needed (depends on whether the caller is a graceful HTTP shutdown vs a panic-recovery path). Auto-checkpointing would couple shutdown to storage I/O — bad for fast-fail paths. Matches anti-plenger "High Cohesion / Loose Coupling".
**Source**: `docs/grafeo-loro.architecture.md` §4 Step D (silent — no checkpoint mandate) + `anti-plenger.md` High Cohesion.
**L2 action**: `shutdown()` body: (1) `sync_engine.shutdown_tx.send(());` (2) `join_all(worker_handles)` with timeout; (3) `drop(sync_engine)` (drops `grafeo_db` Arc — last ref closes DB); (4) NO checkpoint call.

### Q16: Auto-construct `HealthProbe::new(loro_doc.clone(), grafeo_db.clone())` in `build()`?
**Ruling**: **YES — auto-construct in `build()` if `None`** (L1 recommendation upheld).
**Rationale**: At the point `build()` constructs `HealthProbe`, both `loro_doc` and `grafeo_db` already exist as `Arc` handles in the builder's local scope — no caller-side construction awkwardness. Tests using `from_sync_engine_with_config` bypass `build()` so no test breakage. Pre-building via `.with_health(...)` remains available for callers wanting custom probes (e.g., custom `max_staleness_ms` default).
**Source**: `src/app.rs:136` (Builder `health` field) + `src/telemetry/health.rs:70` (`HealthProbe::new`).
**L2 action**: In `build()`, after `loro_doc` + `grafeo_db` construction, if `self.health.is_none()` then `self.health = Some(Arc::new(HealthProbe::new(loro_doc.clone(), grafeo_db.clone())));`.

### Q17: Auto-construct `Arc::new(global::tracer("grafeo-loro"))` in `build()` if `.with_tracer(...)` not called?
**Ruling**: **YES — auto-construct in `build()` if `None`** (same as Q14).
**Rationale**: Verified `opentelemetry::global::tracer(name)` exists at `opentelemetry-0.23.0/src/global/trace.rs:394` (signature `pub fn tracer(name: impl Into<Cow<'static, str>>) -> BoxedTracer`). Same trade-off as Q14: production ergonomics win; tests bypass via `from_sync_engine_with_config`. Wrapping in `Arc` per Q3 ruling (`SharedTracer = Arc<BoxedTracer>`).
**Source**: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/opentelemetry-0.23.0/src/global/trace.rs:394` + `src/telemetry/mod.rs` (`SharedTracer` alias).
**L2 action**: In `build()`, if `self.tracer.is_none()` then `self.tracer = Some(Arc::new(opentelemetry::global::tracer("grafeo-loro")));`.

## 4. Anti-Plenger Audit

- **Bloat (DRY)**: ✅ Clean. No telemetry-related code duplicated across `SyncEngine`/`MutationBatcher`/`GrafeoLoroApp` — each owns its `Option<Arc<...>>` slot, accessed via accessor. Span helpers centralized in `traces.rs`.
- **Hallucination**: ⚠️ **M1 — arch §23.3 line 1080 hallucinates `GrafeoDB::execute(&str)`**. Architecture doc itself is out of sync with grafeo 0.5.42 API (correct: `db.session().execute(...)`). L1 inherited this hallucination from the spec — Devil caught it via physical API inspection. NOT L1's fault, but L2 must not propagate it.
- **YAGNI**: ⚠️ **M2 — Q9 `grafeo_commit` grandchild span**. L1 suggested YAGNI for the grandchild span, but arch §23.2 line 1048 explicitly requires it. YAGNI does not override an explicit spec requirement. L2 must add it.
- **Backward-compat slaves**: ✅ Clean. L1 added NEW constructors (`with_telemetry`, `from_sync_engine_with_telemetry`) rather than mutating existing 4-arg test APIs. Old constructors preserved unchanged. Q11 ruling keeps both indefinitely (not deprecated) — legitimate dual-use, not slave behavior.
- **Context Blindness**: ⚠️ **M3 — Q10 SyncEngine missing `health` field**. L1 added `let health = self.health.clone();` TODO at `sync_engine.rs:467` but did NOT add the `health` field to the `SyncEngine` struct (lines 96-151). This is a contract inconsistency that will confuse L2 (TODO references nonexistent field). Fix: add field in L2.
- **Tautology**: ✅ Clean. No `let x = x;` patterns, no trivially-true doc comments.
- **Band-Aids**: ✅ Clean. No `// FIXME later` / `// HACK` markers; all TODOs are explicit `// TODO(P5-L2):` with concrete next-step descriptions.
- **Happy-Path Bias**: ✅ N/A for L1 (no impl logic — all bodies `unimplemented!()`).
- **Goodhart**: ✅ N/A for L1 (no test gaming — 70/70 baseline preserved, no new tests added to inflate count).

## 5. API Verification (Q4) — Physical Inspection Result

**Verdict**: `GrafeoDB::execute(&str)` does **NOT exist** in grafeo 0.5.42. Architecture §23.3 line 1080 is out of sync with the actual API.

**Evidence chain**:
1. `grafeo 0.5.42` crate root (`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/grafeo-0.5.42/src/lib.rs`) is a 94-line re-export shim — re-exports `GrafeoDB` from `grafeo_engine`.
2. `grafeo-engine 0.5.42` source at `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/grafeo-engine-0.5.42/src/`:
   - `src/database/mod.rs:1663` — `pub fn session(&self) -> Session`
   - `src/database/mod.rs:1728` — `pub fn session_with_cdc(&self, cdc_enabled: bool) -> Session`
   - `src/database/mod.rs:1745` — `pub fn session_read_only(&self) -> Session`
   - `src/database/mod.rs:267` — `pub fn new_in_memory() -> Self`
   - `src/database/mod.rs:290` — `pub fn open(path: impl AsRef<Path>) -> Result<Self>`
   - `src/database/mod.rs:346` — `pub fn with_config(config: Config) -> Result<Self>`
   - `src/session/mod.rs:2636` — `pub fn execute(&self, query: &str) -> Result<QueryResult>` (on `Session`, not `GrafeoDB`)
   - **NO `impl GrafeoDB { pub fn execute(...) }` anywhere** (rg returned zero matches for `pub fn execute` on GrafeoDB directly).
3. Crate-level doc example at `grafeo-0.5.42/src/lib.rs:30-38` confirms the correct usage: `let mut session = db.session(); session.execute("INSERT (:Person {...})")?;`.

**L2 implementation guidance**:
```rust
// In HealthProbe::check (src/telemetry/health.rs):
let grafeo_ok = self.db.session().execute("MATCH (n) RETURN count(n) LIMIT 1").is_ok();
```
- NO `session_with_cdc(false)` needed — read-only COUNT query doesn't write, doesn't trigger CDC.
- NO `begin_transaction` / `prepare_commit` / `commit` no-op tx needed — `Session::execute` already wraps in an implicit tx for single-statement queries.
- File a follow-up to correct `docs/grafeo-loro.architecture.md:1080` from `self.db.execute(...)` → `self.db.session().execute(...)`.

## 6. Findings (BLOCKER / MAJOR / MINOR / NIT)

### BLOCKERs
None. L1 scaffolding compiles (`cargo check --all-targets` exit 0), 70/70 tests pass, all 17 questions resolvable in L2 without L1 rework.

### MAJORs

#### M1: Architecture §23.3 line 1080 has incorrect GrafeoDB API (`db.execute` → should be `db.session().execute`)
- **Source**: `docs/grafeo-loro.architecture.md:1080` + physical grafeo-engine-0.5.42 inspection (see §5).
- **Impact**: L2 implementing `HealthProbe::check` body will hit a compile error if following the arch doc verbatim. Health check endpoint will be non-functional.
- **L2 fix**: Use `self.db.session().execute("MATCH (n) RETURN count(n) LIMIT 1").is_ok()`. Also patch the arch doc.

#### M2: `grafeo_commit` grandchild span not scaffolded in batcher (arch §23.2 line 1048 requires it)
- **Source**: `src/bridge/batcher.rs:229-233` (TODO conditional) + `docs/grafeo-loro.architecture.md:1048`.
- **Impact**: Span hierarchy incomplete — per-flush commit timing invisible in traces. L1's YAGNI suggestion rejected (spec explicitly lists the span).
- **L2 fix**: In `MutationBatcher::flush_inner`, after opening `batch_flush` parent span, open `grafeo_commit` grandchild span around the actual `commit()` call.

#### M3: `SyncEngine` struct missing `health: Option<Arc<HealthProbe>>` field (TODO at sync_engine.rs:467 references `self.health` which doesn't exist)
- **Source**: `src/bridge/sync_engine.rs:96-151` (struct fields — no `health`) vs `src/bridge/sync_engine.rs:467` (`// TODO(P5-L2): let health = self.health.clone();`).
- **Impact**: L2 implementing `spawn_outbound_worker` body will hit a compile error or be confused by the contradictory contract. Also: `MutationBatcher` lacks the field too (per Q10 ruling, both need it).
- **L2 fix**: (a) Add `pub(crate) health: Option<Arc<HealthProbe>>` to `SyncEngine` struct. (b) Add same field to `MutationBatcher` struct. (c) Extend `SyncEngine::with_telemetry` signature from 6 args → 7 args (add `health: Option<Arc<HealthProbe>>`). (d) Thread `health.clone()` into `MutationBatcher::new` + `with_defaults` (extend from 2 new params → 3). (e) Add `health()` accessor on both. (f) Update `GrafeoLoroAppBuilder::build` to pass `health.clone()` into `with_telemetry`.

#### M4: `create_outbound_sync_span` helper missing from `traces.rs` (arch §23.2 lines 1050-1052 require outbound_sync_loop parent span)
- **Source**: `src/telemetry/traces.rs` (only 3 helpers: cold_start / inbound / hybrid) + `src/bridge/sync_engine.rs:451-452` (L1 comment: "note: NO `create_outbound_sync_span`") + `docs/grafeo-loro.architecture.md:1050-1052`.
- **Impact**: Outbound worker has no parent span — breaks §23.2 tree row 3 contract. Traces from CDC poll → Loro commit path will be orphaned.
- **L2 fix**: Add `pub fn create_outbound_sync_span<T: Tracer>(tracer: &T) -> BoxedSpan` to `src/telemetry/traces.rs`. Use in `spawn_outbound_worker`. Inline-create `receive_cdc_event` + `loro_commit` children.

### MINORs

#### m1: `MetricsRegistry::record_hydration` uses `&str` mode (Q6 ruling: switch to `HydrationMode` enum)
- **Source**: `src/telemetry/metrics.rs` (record_hydration signature) + `docs/grafeo-loro.architecture.md:1028`.
- **L2 fix**: Add `pub enum HydrationMode { Loro, Grafeo }` + `impl Display` (renders `"loro"` / `"grafeo"`); change signature to `(duration_ms: f64, mode: HydrationMode)`.

#### m2: Q14/Q16/Q17 auto-construction in `build()` not yet implemented (L1 left TODOs at app.rs build path)
- **Source**: `src/app.rs` `build()` body — L1 TODOs at the 3 auto-construction sites.
- **L2 fix**: Implement auto-construction of `metrics` (via `global::meter("grafeo-loro")`), `health` (via `HealthProbe::new(doc, db)`), `tracer` (via `global::tracer("grafeo-loro")`) when each `Option` is `None` at the corresponding point in `build()`.

### NITs

#### n1: `GrafeoLoroAppBuilder::build` still has `// TODO(P5-L2):` markers for auto-construction (Q14/Q16/Q17)
- Already covered by m2; called out separately because the markers are documentation, not code defects.

#### n2: Architecture doc line 1131 typo `grafio_loro_sync_hydration_duration_ms` (should be `grafeo_loro_sync_...`)
- **Source**: `docs/grafeo-loro.architecture.md:1131`.
- **Impact**: Prometheus alert rule would silently fail to match the metric. Not L1's job to fix, but Devil flags for arch-doc follow-up.

## 7. L2 Implementation Guide (ranked, top 8 items)

1. **[M3] Add `health: Option<Arc<HealthProbe>>` field to both `SyncEngine` + `MutationBatcher` structs**; extend `SyncEngine::with_telemetry` to 7 args + `MutationBatcher::new`/`with_defaults` to accept it; add `health()` accessors; thread `health.clone()` from `GrafeoLoroAppBuilder::build` → `with_telemetry` → `MutationBatcher::new`. Without this, `spawn_outbound_worker` TODO at sync_engine.rs:467 won't compile.

2. **[M1] Implement `HealthProbe::check` body** using `self.db.session().execute("MATCH (n) RETURN count(n) LIMIT 1").is_ok()` (NOT `self.db.execute(...)` per arch §23.3 line 1080 which is wrong). Also implement `HealthProbe::new` + `update_sync_ts` bodies. Patch arch doc.

3. **[M4] Add `create_outbound_sync_span<T: Tracer>(tracer: &T) -> BoxedSpan` to `src/telemetry/traces.rs`**. Use in `spawn_outbound_worker` to open parent span. Inline-create `receive_cdc_event` + `loro_commit` children at the appropriate call sites.

4. **[M2] Add `grafeo_commit` grandchild span** in `MutationBatcher::flush_inner` around the actual `commit()` call, nested under the `batch_flush` parent span. Use span name `"grafeo_commit"` per arch §23.2 line 1048.

5. **[m2] Implement Q14/Q16/Q17 auto-construction in `GrafeoLoroAppBuilder::build`**: if `self.metrics.is_none()` → `Some(Arc::new(MetricsRegistry::init(global::meter("grafeo-loro"))))`; if `self.health.is_none()` → `Some(Arc::new(HealthProbe::new(loro_doc.clone(), grafeo_db.clone())))`; if `self.tracer.is_none()` → `Some(Arc::new(global::tracer("grafeo-loro")))`. Order matters: health needs loro_doc + grafeo_db (already constructed earlier in `build`).

6. **[m1] Add `HydrationMode` enum** (`Loro` / `Grafeo`) with `impl Display` to `src/telemetry/metrics.rs`; change `record_hydration` signature to `(duration_ms: f64, mode: HydrationMode)`. Update call sites in `src/app.rs::hydrate` (both `SsotMode::Loro` + `SsotMode::Grafeo` arms).

7. **Implement `MetricsRegistry` method bodies**: `record_batch_flush(duration_ms, batch_size)` + `record_hydration(duration_ms, mode)` + `init(Meter)` constructor. Wire call sites: `MutationBatcher::flush_inner` (record_batch_flush), `GrafeoLoroApp::hydrate` start/end (record_hydration with cold_start_hydration span), `spawn_inbound_worker` (inbound_events.add with origin+event_type labels), `spawn_outbound_worker` (outbound_events.add), `init_loro_subscriber` origin-filter path + `spawn_cdc_poller` (echo_filtered.add with direction label).

8. **Implement `create_*_span` bodies** in `traces.rs`: each opens a `BoxedSpan` via `tracer.build().with_name(name).start(&mut span_context)`. Wire: `create_cold_start_span` → `GrafeoLoroApp::hydrate` top; `create_inbound_sync_span` → `spawn_inbound_worker` top; `create_outbound_sync_span` (new) → `spawn_outbound_worker` top; `create_hybrid_query_span` → query path top (Phase 5 may stub query path if not yet implemented).

## 8. Verification Matrix

| Q# | Ruling | L2 Action |
|---|---|---|
| Q1 | Accept `Option<Arc<MetricsRegistry>>` | None — L1 contract upheld |
| Q2 | Accept `Arc<RwLock<LoroDoc>>` | None — L1 contract upheld |
| Q3 | Accept `SharedTracer = Arc<BoxedTracer>` | None — L1 contract upheld |
| Q4 | `db.execute` does NOT exist; use `db.session().execute(...)` | Implement `HealthProbe::check` body with corrected API; patch arch doc §23.3 line 1080 |
| Q5 | Silent return on failing component | None — implement `check` without WARN logging |
| Q6 | Use `HydrationMode` enum | Add enum + `Display` impl; change `record_hydration` signature |
| Q7 | Keep generic `<T: Tracer>` | None — L1 contract upheld |
| Q8 | Span names confirmed exact | None — use names verbatim per arch §23.2 lines 1040-1058 |
| Q9 | REWORK — `grafeo_commit` grandchild required | Add grandchild span in `flush_inner` around `commit()` |
| Q10 | YES — both `SyncEngine` + `MutationBatcher` need `health` field | Add field to both structs; extend `with_telemetry` + `MutationBatcher::new`/`with_defaults`; add accessors |
| Q11 | Keep both constructors | None — L1 contract upheld |
| Q12 | Both counters coexist | None — L1 contract upheld |
| Q13 | YES — add `create_outbound_sync_span` helper | Add helper to `traces.rs`; use in `spawn_outbound_worker` |
| Q14 | YES — auto-construct metrics in `build()` if `None` | Implement auto-construction via `global::meter("grafeo-loro")` |
| Q15 | NO auto-checkpoint in `shutdown()` | Implement `shutdown` as: cancel workers → join with timeout → drop engine (no checkpoint) |
| Q16 | YES — auto-construct health in `build()` if `None` | Implement auto-construction via `HealthProbe::new(doc, db)` |
| Q17 | YES — auto-construct tracer in `build()` if `None` | Implement auto-construction via `global::tracer("grafeo-loro")` wrapped in `Arc` |

---

**Verdict**: PROCEED_TO_L2. 0 BLOCKERs, 4 MAJORs (M1–M4), 2 MINORs (m1–m2), 2 NITs (n1–n2). All 17 L1 open questions resolved with explicit rulings. L2 has a clear 8-item ranked implementation guide.
