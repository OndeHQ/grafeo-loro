# P5 Plenger-Traits Hunt

**Date**: 2026-07-06
**Hunter**: P5-HUNT agent
**Audit range**: `9cdc60b..HEAD` (P5-L1 `5a03a14` + P5-L1 worklog `8baf278` + P5-DEVIL `289450b` + P5-DEVIL worklog `dcf95fc` + P5-L2 `8e965f1` + P5-L3 `1b5e7a6`)
**Total commits**: 6
**Files touched**: 14 (5 source modules in `src/telemetry/`, 2 in `src/bridge/`, 1 in `src/hydration/`, 1 in `src/app.rs`, 2 test files, 1 architecture doc, 1 worklog, 1 critique `p5-l1-devil.md`)
**Test count**: 70 baseline → 82 current (+12 new in `tests/unit/telemetry.rs`)
**Verdict**: **L2_REENTRY** — 0 BLOCKER, 2 MAJOR, 1 MINOR, 2 NIT; 6/8 plenger categories clean

---

## Summary

Phase 5's overall quality is **high**: 34/34 TODO(P5-L3) markers filled, 12 new tests added, 82/82 pass, 0 new `unimplemented!()` introduced, 0 production `unwrap()`/`expect()`/`panic!()`, 0 cargo check errors. All OpenTelemetry + Grafeo + parking_lot API calls were independently re-verified line-for-line against the actual crate source under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/` — **zero hallucinations** (plenger-traits #6: 0 findings). The 12 new tests in `tests/unit/telemetry.rs` use real GrafeoDB + real OTel API dispatch (no mocks), exercise edge values (0 ms, MAX values, stale sync), and assert real invariants (Display strings, components order, `overall` AND-of-components) — **0 tautology** + **0 Goodhart** findings. Concurrency hygiene is clean: no `parking_lot::RwLock` read/write guard is held across any `.await` in `src/telemetry/` + `src/bridge/` + `src/hydration/` + `src/app.rs`; all 6 `loro_doc.read()`/`write()` sites in `hydrate`/`checkpoint` are scoped in `{}` blocks that close BEFORE any `storage.*.await` call; the `spawn_blocking` closure in `flush_inner` is sync (no `.await` inside); all `AtomicU64` operations use `Ordering::Relaxed` (correct for bare counters with no accompanying memory payload). No backward-compat slaves — `from_sync_engine` + `from_sync_engine_with_config` + `with_batch_config` are legitimate test/production separation shims (Devil Q11 ruling), all delegate to the production constructors with `None` telemetry slots, no `#[deprecated]` evasion. No Band-Aids — 5 `let _ = ...` suppressors are either pre-existing (presence/socket) or noop-borrow guards for newly-wired-but-not-yet-used handles (cleanly documented).

The remaining findings are **2 MAJOR + 1 MINOR + 2 NIT**, none of which are BLOCKERs but the 2 MAJORs warrant a small L2 reentry to fix before push:

- **MAJOR 1**: `inbound_events` counter is **double-counted** in production. `spawn_inbound_worker` bumps it per-op at `src/bridge/sync_engine.rs:477` (`m.inbound_events.add(1, &[])`) AND `MutationBatcher::flush_inner` bumps it again per-flush at `src/bridge/batcher.rs:317` (`m.inbound_events.add(op_count as u64, &[])`). For a 5-op batch, the counter increments by 10 (5 per-op + 5 aggregate). Devil Q12 explicitly ruled the OTel counter lives at the per-op forward boundary (in `spawn_inbound_worker`); the L3 batcher addition deviates from this ruling + causes the metric to report 2x the actual count.
- **MAJOR 2**: `inbound_events` + `outbound_events` counters omit the architecture-specified labels. Architecture §23.1 row 1 specifies labels `origin, event_type` for `inbound_events_total`; row 2 specifies the same for `outbound_events_total`. The L3 implementation records both with `&[]` empty attribute sets (`src/bridge/sync_engine.rs:477` + `:569` + `src/bridge/batcher.rs:317`). The OTLP consumer cannot slice inbound/outbound events by type — observability granularity is lost. The `echo_filtered` counter (row 3, label `direction`) + `batch_flush_duration` (row 4, label `batch_size`) + `hydration_duration` (row 5, label `mode`) are all correctly labelled; only the two event counters omit labels.

**Recommendation: L2_REENTRY.** Both MAJORs are mechanical fixes (1-line removal for MAJOR 1; ~10 lines added for MAJOR 2 to derive `event_type` from `LoroOp` variant). The MINOR + NITs can be deferred to Phase 6 hardening.

---

## Findings by Plenger-Trait

### 1. Backward Compatibility Slaves

**0 findings.**

The prime suspects — `from_sync_engine` (4-arg shim), `from_sync_engine_with_config` (legacy 4-arg production), `with_batch_config` (4-arg test form) — were all inspected. None are backward-compat slaves; all are legitimate test/production separation per Devil Q11:

- **`from_sync_engine(sync_engine)`** at `src/app.rs:184-194`: 5-line shim that delegates to `from_sync_engine_with_config(sync_engine, SsotMode::default(), None, CompressionType::default())`. Used only by 2 `tests/unit/vertex_builder.rs` sites (lines 116, 133) that don't exercise `hydrate`/`checkpoint`. Doc-comment at `:180-184` explicitly warns "Callers that exercise `hydrate`/`checkpoint` MUST use the explicit constructor (storage `None` will fail at first dispatch)". Failure surfaces as `GrafeoLoroError::Config("storage backend not set")` at first dispatch — no silent legacy rot.
- **`from_sync_engine_with_config`** at `src/app.rs:210-232`: legacy 4-arg production constructor (P4-era). Still used by `tests/unit/hydrate_checkpoint.rs:131` (the cold-boot round-trip test). Initializes the 4 P5-L1 telemetry fields to `None` (preserves the 4-arg test API per Devil Q1).
- **`with_batch_config`** at `src/bridge/sync_engine.rs:208-215`: 4-arg test form. Used by 0 production sites (production uses `with_telemetry` exclusively at `src/app.rs:1385`). Used by 0 test sites directly (tests use the even-shorter `SyncEngine::new` 2-arg form). Devil Q11 explicitly approved keeping both indefinitely.

No `#[deprecated]` markers in `src/` (`rg -n "#\[deprecated" src/` returns 0 hits) — Devil Q11 ruling honored. No code paths route around new code to preserve old behavior; both test-form constructors delegate to the production constructors with `None` telemetry slots.

### 2. Tautology (Green tests, broken system)

**0 findings.**

Read all 12 new tests in `tests/unit/telemetry.rs` (277 lines). For each test, asked: "Does this verify REAL behavior through a REAL pipeline, or does it just verify the test setup?"

| Test | Verifies | Real? |
|---|---|---|
| `test_metrics_registry_init_construction_no_panic` | Constructs `MetricsRegistry::init(global::meter("test"))` + binds to `_`. Type system guarantees `Counter<u64>` / `Histogram<f64>` (not `()`). Comment correctly notes "we call `add`/`record` below to verify dispatch" (in tests 2/3). | ✅ Borderline — pure no-panic constructor test, but acceptable as an edge-case companion to tests 2/3. The 5 instruments really ARE constructed via real `meter.u64_counter(...).init()` + `meter.f64_histogram(...).init()` dispatch. |
| `test_record_batch_flush_no_panic_on_edge_values` | Calls `record_batch_flush(0.0, 0)` + `(f64::MAX, u64::MAX)` + `(12.5, 256)`. Real `Histogram::record` dispatch through noop instrument vtable. | ✅ Anti-plenger #7 — edge values tested (0, MAX, realistic). |
| `test_record_hydration_no_panic_with_both_modes` | Calls `record_hydration` 4x with both `HydrationMode::Loro` + `Grafeo`. Exercises the real `Display` impl on the enum (called inside `record_hydration` to render the OTLP attribute). | ✅ Real Display dispatch. |
| `test_hydration_mode_display_renders_correct_strings` | `assert_eq!(HydrationMode::Loro.to_string(), "loro")` + `Grafeo` → `"grafeo"`. Round-trip via `format!("mode={}", ...)`. | ✅ Real Display output assertion. |
| `test_health_probe_new_initializes_last_sync_ts_nonzero` | `assert!(probe._last_sync_ts_for_test() > 0)`. Verifies `new()` actually ran the `unix_timestamp_ms()` init (NOT 0). | ✅ Real invariant. |
| `test_health_probe_check_returns_false_when_sync_stale` | Computes real `now` from `SystemTime::now()`, sets `last_sync_ts` to `now - 10_000` via test-helper setter (calls same `last_sync_ts.store(...)` code path as production `update_sync_ts()`), then `check(5_000)`. Asserts `sync_ok == false` AND `overall == false`. | ✅ **Phase 5 Task 3 validation requirement** — real stale-sync path with real timestamp arithmetic. Anti-flaky (avoids `tokio::time::sleep`). |
| `test_health_probe_check_sync_ok_when_fresh` | `update_sync_ts()` then `check(5_000)` — asserts `sync_ok == true`. Only asserts the component we control (sync_freshness), NOT `overall` (which depends on Grafeo state — defensive). | ✅ Anti-happy-path-bias — narrow assertion scope. |
| `test_health_probe_check_overall_true_when_all_components_healthy` | Calls `check(5_000)` against REAL in-memory `GrafeoDB` + REAL `LoroDoc`. Asserts `overall == true`. | ✅ **Anti-tautology gold standard** — real GrafeoDB executes a real Cypher `MATCH (n) RETURN count(n) LIMIT 1` query. |
| `test_health_probe_check_components_order_matches_architecture` | `assert_eq!(names, vec!["loro_doc", "grafeo_db", "sync_freshness"])`. Verifies REAL ordering of the `components` Vec. | ✅ Real ordering contract. |
| `test_health_probe_update_sync_ts_advances_timestamp` | Forces `last_sync_ts = 0`, calls `update_sync_ts()`, asserts `after > 0` AND `after >= before`. | ✅ Real stamp advancement. |
| `test_span_creators_return_boxed_span_without_panic` | Calls all 4 `create_*_span` helpers + binds to `_`. Type system guarantees `BoxedSpan` return. Drop path also exercised. | ✅ Borderline no-panic, but type system + drop path provide real verification. |
| `test_span_creators_callable_multiple_times` | Calls each helper 3x in a loop. Anti-plenger #9 (idempotency). | ✅ Real repeated-call safety. |

No mocks (real OTel API dispatch + real GrafeoDB). No `#[cfg(test)]` re-definitions of production types. No assertions on test setup state. The test-helper accessors `_last_sync_ts_for_test` + `_set_last_sync_ts_for_test` are `#[doc(hidden)] pub` (visible to tests but hidden from docs) + use the same atomic `load`/`store` code paths as production — not a mock, just a deterministic test hook.

### 3. Context Blindness

**0 findings.**

**Lock-across-`.await` sweep** — verified all 6 `doc.read()`/`doc.write()` sites in `hydrate`/`checkpoint`:
- `src/app.rs:485-487` (`checkpoint` `oplog_frontiers`): scoped in `{}` block, no `.await` while held. ✅
- `src/app.rs:503-506` (`checkpoint` `export`): scoped in `{}` block, no `.await` while held. ✅
- `src/app.rs:776-799` (`hydrate` `import_with`): scoped in `{}` block, no `.await` while held. ✅
- `src/app.rs:875-890` (`hydrate` `parallel_hydrate_grafeo`): scoped in `{}` block. `parallel_hydrate_grafeo` is sync (NOT async — returns `Result<()>`). No `.await` while held. ✅
- `src/app.rs:924-952` (`hydrate` `get_map().keys()` scan): scoped in `{}` block, all sync. No `.await` while held. ✅
- `src/bridge/sync_engine.rs:363` (`init_loro_subscriber` `doc.read()`): function is `pub fn` (NOT async). `subscribe_root(handler)` + `*self.loro_sub.lock() = Some(sub)` are sync. No `.await` while held. ✅

**Worker-loop write guards** — `src/bridge/sync_engine.rs:551-553` (`spawn_outbound_worker` apply_change_event) + `:558-562` (`set_next_commit_origin + commit`): both scoped in `{}` blocks, all sync calls. No `.await` while held. ✅

**CDC poller read guard** — `src/bridge/sync_engine.rs:642` (`bridge_epochs.read().contains(&ev.epoch)`): guard in `if` condition, drops at end of `if`. The `outbound_tx.send(wrapped).await` at `:655` runs AFTER the guard drops. ✅

**Batcher epochs write** — `src/bridge/batcher.rs:302` (`epochs.write().insert(epoch)`): INSIDE the `spawn_blocking` closure on a blocking-pool thread (not the async task thread). No `.await` while held (closure is sync). ✅

**AtomicU64 Ordering** — all atomic operations use `Ordering::Relaxed`:
- `inbound_event_count.fetch_add(1, Relaxed)` at `sync_engine.rs:412` — bare counter, no payload, Relaxed correct.
- `inbound_filtered_count.fetch_add(1, Relaxed)` at `sync_engine.rs:394` — same.
- `last_sync_ts.store(now_ms, Relaxed)` at `health.rs:130` — soft signal, no payload, Relaxed correct (architecture §23.3 explicitly says "staleness is a soft signal, not a synchronization primitive").
- `last_sync_ts.load(Relaxed)` at `health.rs:180` + `:198` — same.
- `loro_key_counter.fetch_max(max + 1, Relaxed)` at `app.rs:938` — pre-existing P4 pattern, P4-HUNT verified correct (no accompanying memory payload; each VertexBuilder::commit does its own fetch_add + reads its own result). ✅

**Span lifecycle** — child span drops BEFORE parent span:
- `src/bridge/batcher.rs:294-301`: `_grafeo_commit_span` drops at end of `{}` block at `:301` (inside `spawn_blocking` closure). `_batch_flush_span` drops at end of `flush_inner` function (after `tokio::time::timeout(...).await` resolves). Child drops first. ✅
- `src/hydration/parallel.rs:90-92`: `_chunk_span` drops at end of `par_chunks` closure body. `_parallel_span` drops at end of `parallel_hydrate_grafeo` function. Child drops first. ✅
- `src/app.rs:729-731`: `_cold_start_span` drops at end of `hydrate` function. `parallel_hydrate_grafeo`'s `_parallel_span` (child) drops when `parallel_hydrate_grafeo` returns (before `_cold_start_span` drops). ✅

**Caveat (not a finding)** — parent-child linking across `tokio::spawn` / `spawn_blocking` boundaries is NOT propagated via OTel `Context`. Spans emitted across these boundaries appear as root spans in Jaeger (not nested children). L3 worklog explicitly documents this as out-of-scope (span names are the contract; Jaeger "search by name" reconstructs the hierarchy for human inspection). See NIT 2 below for the architecture-implications angle.

### 4. Band-Aids

**0 findings.**

**Error swallowing sweep** — `rg -n "\.ok\(\)|unwrap_or_default|unwrap_or\(|if let Err.*continue|let _ = " src/telemetry/ src/bridge/ src/hydration/ src/app.rs`:
- `src/telemetry/health.rs:54` (`unix_timestamp_ms().unwrap_or(0)`): defensive fallback for `SystemTime::now().duration_since(UNIX_EPOCH)` erroring on time-went-backwards. Documented as "unreachable on commodity OSes, but the fallback keeps `check()` non-panicking". Anti-plenger #7 (defensive programming). ✅
- `src/hydration/parallel.rs:116-117` (`.ok().and_then(|c| c.into_map().ok())`): collapses two `Result`s to `Option` before `ok_or_else`. The original enums are diagnostic only — the `ok_or_else` produces a `Bridge` error with the key name. Pre-existing P3T2 pattern (not introduced by P5). ✅
- `src/app.rs:931` (`strip_prefix("V/").and_then(|n| n.parse::<u64>().ok())`): pre-existing P4 `loro_key_counter` re-seed. ✅
- `src/bridge/sync_engine.rs:370` (`let _ = &metrics;`): noop-borrow guard for the metrics handle captured by `init_loro_subscriber`'s closure. The closure DOES use `metrics` at `:399-401` (`m.echo_filtered.add(1, &[...])`) — the `let _ = &metrics;` line is redundant (the variable IS used). **MINOR cleanup opportunity** but not a Band-Aid — no error being swallowed, just a leftover from L2's wiring phase. The `let _ = &metrics;` line could be removed without behavioral change.
- `src/hydration/parallel.rs:67` (`let _ = metrics;`): noop-borrow for the `metrics` param in `parallel_hydrate_grafeo`. Documented as "the param stays in the signature for forward-compat (per-chunk metrics in a future phase) + for the L2 contract that threads it through." Acceptable YAGNI deferral — not a Band-Aid.
- `src/bridge/sync_engine.rs:450` (`let _ = batcher.run(batch_rx).await;`): discards `Result<()>` from the batcher's `run` loop. Logged inside `run` itself (any `Err` returned is from the final flush; previous flushes already logged). Acceptable.
- `src/bridge/sync_engine.rs:489` (`let _ = batcher_handle.await;`): discards `JoinResult` from the batcher task. The batcher's `run` already logs its own errors. Acceptable.
- `src/bridge/sync_engine.rs:688` (`let _ = self.init_loro_subscriber();`): discards `Result<()>` from subscriber init. `init_loro_subscriber` returns `Result<()>` but currently has no failure path (the `let sub = doc.subscribe_root(handler)` cannot fail). Acceptable.
- `src/bridge/sync_engine.rs:699` (`let _ = self.shutdown_tx.send(());`): discards `SendError` — shutdown broadcast has 0 subscribers means the workers already exited, which is fine. Acceptable.

**TODO/FIXME/HACK/XXX sweep** — `rg -n "TODO|FIXME|HACK|XXX" src/telemetry/ src/bridge/ src/hydration/ src/app.rs`:
- 0 `TODO(P5-L3)` markers remain (L3 cleaned up all 34). ✅
- 4 stale `TODO(P4-L2)` / `TODO(P5)` references in `src/app.rs` doc-comments (`:429`, `:648`, `:1143`, `:1164`) — all in doc-comment text describing future-phase work, not in active code. Pre-existing from P4. Not Band-Aids.
- 0 `FIXME` / `HACK` / `XXX` markers in P5-touched code. ✅

**Span Context deferral (P5-L3 worklog note)** — the L3 worklog explicitly documents: "Real parent-child linking requires explicit `Context::current()` capture + `with_context(...)` propagation across spawn boundaries (out of P5-L3 scope — YAGNI check: span names + Jaeger's 'search by name' reconstructs the hierarchy for human inspection)." This is a YAGNI deferral, not a Band-Aid — the span NAMES are correct (architecture §23.2 contract honored), and the L3 worklog transparently discloses the limitation. See NIT 2 for the architecture-implications angle.

### 5. Bloat (DRY Violations)

**0 findings.**

**Display impl sweep** — `rg -n "fn fmt|impl Display|impl fmt::Display" src/telemetry/`:
- `src/telemetry/metrics.rs:52-59` (`impl fmt::Display for HydrationMode`): 7-line impl using `f.write_str("loro")` / `f.write_str("grafeo")`. This is the standard `Display` pattern (NOT reinventing a utility). Devil m1 (Q6 ruling) explicitly approved the `HydrationMode` enum for type safety vs `&str`. ✅

**KeyValue/AttributeValue sweep** — `rg -n "KeyValue::new|AttributeValue" src/telemetry/`:
- 5 `KeyValue::new(key, value)` call sites across `src/telemetry/metrics.rs:127,147` + `src/bridge/sync_engine.rs:400,546,650`. All use the standard OTel `KeyValue::new` API (verified at `opentelemetry-0.23.0/src/common.rs:413`). No reinvented attribute construction. ✅

**Time-reading consistency sweep** — `rg -n "now\(\)|SystemTime|Instant" src/telemetry/ src/bridge/ src/app.rs`:
- `src/telemetry/health.rs:51` (`SystemTime::now().duration_since(UNIX_EPOCH)`): wall-clock ms for `last_sync_ts` + `check()` staleness computation.
- `src/bridge/batcher.rs:271` (`std::time::Instant::now()`): elapsed measurement for `batch_flush_duration`. Comment at `:269-270` explicitly justifies `std::time::Instant` (NOT `tokio::time::Instant`) — wall-clock measurement, not runtime time.
- `src/app.rs:738` (`std::time::Instant::now()`): elapsed measurement for `hydration_duration`.
- `src/bridge/batcher.rs:33,44,312` (`tokio::time::Duration` for `FLUSH_TIMEOUT`): `tokio::time::Duration` is a re-export of `std::time::Duration` (same type, no mixing). ✅
- No mixing of `std::time` + `tokio::time` for the same logical purpose. Wall-clock ms uses `SystemTime`; elapsed uses `std::time::Instant`; timeout uses `tokio::time::Duration` (== `std::time::Duration`). ✅

**`MetricsRegistry::init` pattern** — compared to OTel cargo-registry examples: the `meter.u64_counter(name).init()` + `meter.f64_histogram(name).init()` chain is the canonical OTel 0.23 meter-factory pattern (verified at `opentelemetry-0.23.0/src/metrics/meter.rs:273,385` + `instruments/mod.rs:82`). No reinvention. ✅

### 6. Hallucination

**0 findings.**

Every P5-L3 API call was physically verified against the actual crate source under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`:

| API call | Crate | File:line | Verified? |
|---|---|---|---|
| `meter.u64_counter(name)` | opentelemetry-0.23.0 | `src/metrics/meter.rs:273` | ✅ |
| `meter.f64_histogram(name)` | opentelemetry-0.23.0 | `src/metrics/meter.rs:385` | ✅ |
| `Counter::add(value, &[KeyValue])` | opentelemetry-0.23.0 | `src/metrics/instruments/counter.rs:35` (noop at `metrics/noop.rs:96`) | ✅ |
| `Histogram::record(value, &[KeyValue])` | opentelemetry-0.23.0 | `src/metrics/instruments/histogram.rs:34` (noop at `metrics/noop.rs:108`) | ✅ |
| `Tracer::span_builder(name)` | opentelemetry-0.23.0 | `src/trace/tracer.rs:162` | ✅ |
| `SpanBuilder::start(tracer) -> T::Span` | opentelemetry-0.23.0 | `src/trace/tracer.rs:374` | ✅ (returns `T::Span`, justifying L3's `T: Tracer<Span = BoxedSpan>` constraint) |
| `KeyValue::new(K, V)` | opentelemetry-0.23.0 | `src/common.rs:413` | ✅ |
| `Value: From<i64>` (NOT `From<u64>`) | opentelemetry-0.23.0 | `src/common.rs:349-362` (macro `from_values!` only impls `bool, i64, f64, StringValue`) | ✅ (L3's `batch_size as i64` cast is correct) |
| `Value: From<String>` | opentelemetry-0.23.0 | `src/common.rs:371` | ✅ (L3's `mode.to_string()` works) |
| `global::meter(name) -> Meter` | opentelemetry-0.23.0 | `src/global/metrics.rs:115` | ✅ |
| `global::tracer(name) -> BoxedTracer` | opentelemetry-0.23.0 | `src/global/trace.rs:394` | ✅ |
| `global::shutdown_tracer_provider()` | opentelemetry-0.23.0 | `src/global/trace.rs:421` | ✅ |
| `global::shutdown_meter_provider()` | opentelemetry-0.23.0 | (does NOT exist in 0.23) | ✅ (L3 worklog correctly notes this + justifies calling `shutdown_tracer_provider` only — meter provider flushed on drop) |
| `parking_lot::RwLock::try_read() -> Option<...>` | lock_api-0.4.14 (re-exported by parking_lot-0.12.5) | `src/rwlock.rs:472-479` | ✅ returns `Option` (NOT `Result`); `Some` if acquired, `None` if contention |
| `AtomicU64::load(Ordering::Relaxed)` | std | stdlib | ✅ |
| `AtomicU64::store(value, Ordering::Relaxed)` | std | stdlib | ✅ |
| `GrafeoDB::session() -> Session` | grafeo-engine-0.5.42 | `src/database/mod.rs:1663` | ✅ (Devil M1 verified) |
| `GrafeoDB::session_with_cdc(bool) -> Session` | grafeo-engine-0.5.42 | `src/database/mod.rs:1728` | ✅ |
| `Session::execute(&str) -> Result<QueryResult>` | grafeo-engine-0.5.42 | `src/session/mod.rs:2636` | ✅ (Devil M1 verified) |
| `GrafeoDB::execute(&str)` | grafeo-engine-0.5.42 | (does NOT exist) | ✅ (Devil M1 caught this; arch §23.3 patched in P5-L2) |

**API verification table — every P5-L3 call site confirmed.** Zero hallucinations.

### 7. Happy-Path Bias

**0 findings.**

**Production unwrap sweep** — `rg -n "\.unwrap\(\)|\.expect\(|panic!\(" src/telemetry/ src/bridge/ src/hydration/ src/app.rs`:
- 0 production `unwrap()` / `expect()` / `panic!()` calls. All 10 matches are inside `#[cfg(test)]` modules (`src/types/values.rs:257-291` + `src/bridge/sync_engine.rs:1055`). ✅

**`HealthProbe::check` error-path coverage** — verified at `src/telemetry/health.rs:157-190`:
- RwLock poisoned → `try_read()` returns `None` → `loro_ok = false`. ✅ (NOTE: parking_lot::RwLock has NO poisoning — see MINOR 1 below — but the code path handles `None` correctly regardless of why `try_read()` fails.)
- `db.session().execute(...)` returns `Err` → `.is_ok()` returns `false` → `grafeo_ok = false`. ✅
- Time went backwards (`last_sync_ts > now`) → `now.saturating_sub(last)` returns 0 → `0 <= max_staleness_ms` → `sync_ok = true`. ✅ Defensive — `saturating_sub` prevents arithmetic underflow. The L3 comment at `:167-172` documents the rationale ("treats backwards clock as 'just synced' rather than 'infinitely stale'; anti-plenger #7 defensive programming").
- `overall = loro_ok && grafeo_ok && sync_ok` — single failing component marks app unhealthy. ✅

**`MetricsRegistry::init` failure handling** — verified at `src/telemetry/metrics.rs:98-110`:
- `meter.u64_counter(name).init()` + `meter.f64_histogram(name).init()` are infallible (the `init()` consumes the builder + returns a fully-constructed instrument; the noop Meter returns noop instruments — no failure path).
- If a real SDK is installed + the meter is broken, the OTel SDK would panic internally — but this is an OTel contract violation, not a P5 issue. No defensive handling needed at the P5 layer. ✅

**`MutationBatcher::flush_inner` failure handling** — verified at `src/bridge/batcher.rs:245-350`:
- If `prepared.commit()` fails inside `spawn_blocking`, the closure returns `Err`, `tokio::time::timeout(...).await` returns `Ok(Ok(Err(...)))`, `matches!(outcome, Ok(Ok(Ok(()))))` is false → metrics NOT recorded (correct — don't bump `inbound_events` on failed flush).
- The outcome is mapped at `:327` (`Ok(Ok(res)) => res`) which propagates the `Err` to the caller.
- If the blocking task panics: `Ok(Err(join_err))` arm at `:328-337` logs `tracing::error!` + returns `GrafeoLoroError::Bridge(...)`.
- If the timeout elapses: `Err(_)` arm at `:338-348` logs `tracing::error!` + returns `GrafeoLoroError::Bridge(...)` + notes "task continues in background".
- Health probe `update_sync_ts()` only called on `Ok(Ok(Ok(())))` — failed flush does NOT stamp `last_sync_ts`. ✅

**`GrafeoLoroApp::shutdown` failure handling** — verified at `src/app.rs:999-1051`:
- `worker_handles` is `None` → skipped silently (test path, `:1015` `if let Some(handles)`). ✅
- `handle.await` returns `Err(JoinError)` → logged via `tracing::warn!(worker_idx, error, "shutdown: worker join failed (continuing)")` at `:1018-1023` + continues to next handle. ✅
- `sync_engine.shutdown()` is non-async (returns `()`); `shutdown_tracer_provider()` returns `()` — neither has a failure path. ✅
- `shutdown` always returns `Ok(())` (best-effort). ✅

### 8. Goodhart's Law in Action

**0 findings.**

Read each of the 12 tests + asked: "Is this testing REAL behavior, or did the author write the test to match a hardcoded output?"

- **No hardcoded counter values** — tests assert invariants (`> 0`, `>= before`, `== false`, `== true`), not specific counter readings. The `inbound_events`/`outbound_events`/`echo_filtered` counters are NEVER inspected in tests (correctly — the noop SDK doesn't accumulate state).
- **No mocked system-under-test** — `fresh_probe()` at `:120-124` builds a REAL `GrafeoDB::new_in_memory()` + REAL `LoroDoc::new()` + REAL `HealthProbe::new(doc, db)`. The `MetricsRegistry::init` at `:63-64` builds from the REAL `global::meter("grafeo-loro-test")`. The `tracer` at `:257` is the REAL `global::tracer("grafeo-loro-test")`. No `MockMeter` / `MockTracer` / `MockGrafeoDB`.
- **No `#[cfg(test)]` re-definitions of production types** — `rg "#\[cfg\(test\)\]" tests/unit/telemetry.rs` returns 0 hits. The `HealthProbe` + `MetricsRegistry` + `HydrationMode` + `BoxedSpan` types used in tests are the SAME production types.
- **Real Display verification** — `test_hydration_mode_display_renders_correct_strings` asserts `to_string()` output, which exercises the real `fmt::Display` impl. ✅
- **Real staleness arithmetic** — `test_health_probe_check_returns_false_when_sync_stale` computes real `now` from `SystemTime`, sets `last_sync_ts` to `now - 10_000`, then asserts the real `check(5_000)` computation returns `sync_ok == false`. ✅
- **Real Grafeo query** — `test_health_probe_check_overall_true_when_all_components_healthy` runs a REAL `MATCH (n) RETURN count(n) LIMIT 1` against a REAL in-memory `GrafeoDB`. ✅

No test was written to match a hardcoded output. All assertions are on observable invariants that don't depend on a specific instrument reading.

---

## 9. Findings Summary

### BLOCKERs: 0

### MAJORs: 2

**MAJOR 1 — `inbound_events` counter double-counted**

- **Location**: `src/bridge/batcher.rs:317` + `src/bridge/sync_engine.rs:477`
- **Symptom**: The `inbound_events` OTel counter is bumped at TWO boundaries: (a) per-op forward in `spawn_inbound_worker` (`m.inbound_events.add(1, &[])` at `sync_engine.rs:477`) and (b) per-flush aggregate in `flush_inner` (`m.inbound_events.add(op_count as u64, &[])` at `batcher.rs:317`). For a 5-op batch, the counter increments by 10 (5 per-op + 5 aggregate) — production reports 2x the actual op count.
- **Devil ruling deviation**: Devil Q12 explicitly specified the OTel counter lives at the **per-op forward boundary** in `spawn_inbound_worker` (distinct from the test-only `inbound_event_count` AtomicU64 at the subscriber boundary). Devil did NOT authorize a third call site in `flush_inner`. The L3 worklog conflates Q10 (health probe `update_sync_ts` in batcher — correctly authorized) with Q12 (inbound_events counter — NOT authorized for batcher).
- **Fix**: Remove the `m.inbound_events.add(op_count as u64, &[])` line at `batcher.rs:317`. Keep the per-op forward count at `sync_engine.rs:477` (per Devil Q12). The `m.record_batch_flush(elapsed_ms, op_count as u64)` line above it (batcher.rs:316) is correct + stays — it records `batch_flush_duration` which is a different instrument.
- **Test impact**: No test currently inspects `inbound_events` readings (the noop SDK doesn't accumulate state). Removing the line breaks 0 tests.

**MAJOR 2 — `inbound_events` + `outbound_events` omit architecture-specified labels**

- **Location**: `src/bridge/sync_engine.rs:477` (`m.inbound_events.add(1, &[])`) + `:569` (`m.outbound_events.add(1, &[])`) + `src/bridge/batcher.rs:317` (the line called out in MAJOR 1, also empty labels)
- **Symptom**: Architecture §23.1 row 1 specifies labels `origin, event_type` for `inbound_events_total`; row 2 specifies the same for `outbound_events_total`. The L3 implementation records both with `&[]` empty attribute sets. The OTLP consumer cannot slice inbound/outbound events by type — observability granularity is lost.
- **Contract mismatch**: 3 of 5 instruments correctly honor their arch-specified labels:
  - `echo_filtered_total` (row 3, label `direction`) — ✅ recorded as `KeyValue::new("direction", "inbound")` / `"outbound"` at 3 call sites.
  - `batch_flush_duration_ms` (row 4, label `batch_size`) — ✅ recorded as `KeyValue::new("batch_size", batch_size as i64)`.
  - `hydration_duration_ms` (row 5, label `mode`) — ✅ recorded as `KeyValue::new("mode", mode.to_string())`.
  - Only `inbound_events_total` + `outbound_events_total` (rows 1 + 2) omit labels.
- **Fix**: Derive `event_type` from the `LoroOp` variant at the per-op forward site (`spawn_inbound_worker`). The `LoroOp` enum has 4 variants (`UpsertNode`, `DeleteNode`, `UpsertEdge`, `DeleteEdge`) — derive `event_type_str` via a `match` + pass `&[KeyValue::new("event_type", event_type_str)]`. The `origin` label is more nuanced (the Loro event origin has already been filtered to non-`ORIGIN_GRAFEO_BRIDGE` / non-`ORIGIN_LORO_BRIDGE` at the subscriber boundary, so the surviving origin is typically the user's commit origin — usually empty or `"default"`). Plumb the origin through `InboundMsg::Op(op, origin)` (requires extending `InboundMsg` to carry the origin — a small API change), OR document that `origin` is deferred to Phase 6 + record only `event_type` for now. For `outbound_events` at `sync_engine.rs:569`, derive `event_type` from the `ChangeEvent` payload (`apply_change_event_to_loro` already inspects the event type — pass it through).
- **Test impact**: Add 2 new tests asserting that the labels are non-empty (requires a real OTel SDK + a test exporter that captures recorded attributes — out of P5-HUNT scope; recommend Phase 6 hardening).

### MINORs: 1

**MINOR 1 — HealthProbe `loro_doc` component source comment misrepresents parking_lot poisoning**

- **Location**: `src/telemetry/health.rs:21` (module doc) + `:161` (inline comment in `check`)
- **Symptom**: The L3 source comment says: "LoroDoc lock poison: `try_read()` returns `Option` (parking_lot API) — `Some` if the lock is unpoisoned, `None` if poisoned." This is **factually wrong** about parking_lot semantics. parking_lot::RwLock has **no poisoning** (verified at `parking_lot-0.12.5/src/rwlock.rs:32`: "No poisoning, the lock is released normally on panic"). `try_read()` returns `None` when the write lock is currently held by another thread (contention), NOT when poisoned.
- **Behavioral impact**: None — the code path handles `None` correctly regardless of why `try_read()` fails. In production, the LoroDoc write lock is held only for tiny scoped blocks (no `.await` while held, per Plenger #3 sweep), so `try_read()` essentially always returns `Some` in practice — the `loro_doc` health component is a near-constant `true`.
- **Architecture impact**: Architecture §23.3 line 1080 says "LoroDoc is not poisoned (can acquire read lock)" — the parenthetical "(can acquire read lock)" is the operational definition, which IS what `try_read().is_some()` verifies. The "not poisoned" wording is a leftover from `std::sync::RwLock` mental model and doesn't apply to parking_lot.
- **Fix**: Update the source comment + architecture §23.3 to say "LoroDoc lock acquirable" (the operational definition). No code change needed (the implementation is fine). Alternatively, switch to `std::sync::RwLock` for the LoroDoc wrapper if true poison detection is desired — but this would lose parking_lot's no-poison property + require re-evaluating all 6 lock sites for poison handling. Recommend the doc-fix path.
- **Test impact**: 0 — the test `test_health_probe_check_overall_true_when_all_components_healthy` passes because no writer holds the lock at check time, which is the correct operational behavior.

### NITs: 2

**NIT 1 — L3 worklog summary `unimplemented!()` count off by 3**

- **Location**: `worklog.md` P5-L3 entry Stage Summary (line ~5685): "Pre-existing `unimplemented!()` in production code (out of P5-L3 scope): 8"
- **Actual count**: 11 (verified via `rg -n "unimplemented!\(\)" src/` excluding doc-comments). The L3 worklog's enumeration is complete (it lists all 11 sites correctly: `config.rs:31`, `presence/socket.rs:14,20,26,32`, `app.rs:353,359,371,571,966,977`) but the summary count says "8" instead of "11".
- **Impact**: None — the L3 worklog's "New `unimplemented!()` introduced by P5-L3: 0" claim is correct (verified). All 11 sites are pre-existing from earlier phases (Phase 4 deferred, Phase 5 Task 1 skipped, Phase 3/4+ scope, SsotMode::Grafeo wal-feature deferred). This is purely a counting inaccuracy in the worklog summary, not a production regression.
- **Fix**: Update the worklog summary "8" → "11" (cosmetic).

**NIT 2 — Span hierarchy is logical (Jaeger reconstructs by name), not actual OTel parent-child**

- **Location**: `src/bridge/batcher.rs:285-301` (grafeo_commit grandchild inside spawn_blocking) + `src/bridge/sync_engine.rs:453-490` (inbound_sync_loop parent in tokio::spawn) + `src/hydration/parallel.rs:68-92` (parallel_hydrate_grafeo + hydrate_chunk in rayon closure)
- **Symptom**: P5-L3 emits the correct span NAMES per architecture §23.2 but does NOT propagate OTel `Context` across `tokio::spawn` / `spawn_blocking` / `rayon::par_chunks` boundaries. This means `grafeo_commit`, `batch_flush`, `receive_cdc_event`, `hydrate_chunk`, etc. appear as **root spans** in Jaeger, not as nested children of their architecture-specified parents.
- **Documentation**: L3 worklog transparently documents this as out-of-scope: "If HUNT finds Jaeger showing flat span list, that's the cause — not a bug, just absent Context plumbing."
- **Architecture implication**: Architecture §23.2 shows the span hierarchy as a TREE (with row numbers 1, 1.3, 1.3.1, 2, 2.2, 2.2.1, etc.). The tree is a **logical grouping** for human understanding — Jaeger reconstruction by name is a valid alternative. The architecture does NOT explicitly require OTel `Context` propagation across spawn boundaries.
- **Fix (optional, Phase 6)**: Either (a) update architecture §23.2 to explicitly note "tree is logical; parent-child linking requires Context propagation, deferred to Phase 6 hardening" OR (b) implement Context propagation in Phase 6 via `opentelemetry::Context::current()` capture before spawn + `with_context(...)` restoration inside the closure. Not a P5 blocker.
- **Test impact**: 0 — no test asserts parent-child linking.

---

## 10. L2 Re-entry Recommendation

**Verdict: L2_REENTRY.**

**Rationale**: The 2 MAJORs are real production correctness issues that warrant a small L2 reentry before push:

1. **MAJOR 1 (double-counting)** is a 1-line removal in `batcher.rs:317`. The fix is trivial, the test impact is 0 (no test inspects the counter), and the production behavior change is significant (counter reports actual count instead of 2x). Pushing this as-is would mean operators see 2x inbound event counts in their OTLP consumer — a real observability bug.

2. **MAJOR 2 (missing labels)** is a ~10-line addition to derive `event_type` from `LoroOp` variant. The fix is mechanical (one `match` statement + `KeyValue::new("event_type", event_type_str)`). The architecture §23.1 row 1/2 label spec is a contract — pushing an implementation that doesn't honor it would mean operators can't slice inbound/outbound events by type, which is the whole point of the labels.

Both fixes are scoped to L2 (state + execution-path wiring, no algorithm rework). Neither requires L1 contract changes (the `MetricsRegistry::record_*` method signatures already take the right types — `record_batch_flush(duration_ms, batch_size)` already has the `batch_size` parameter; `inbound_events.add(value, attrs)` is the standard OTel API that already accepts attributes). The fix is purely "remove one line + add labels at the existing call sites".

**MINOR 1 + NITs 1-2 can be deferred to Phase 6 hardening** — they are documentation/accuracy issues with no behavioral impact.

**L2 reentry scope (top 3)**:
1. **[MAJOR 1]** Remove `m.inbound_events.add(op_count as u64, &[])` at `src/bridge/batcher.rs:317` (1-line removal).
2. **[MAJOR 2]** Add `event_type` label to `m.inbound_events.add(1, &[...])` at `src/bridge/sync_engine.rs:477` (derive from `LoroOp` variant: `UpsertNode → "upsert_node"`, `DeleteNode → "delete_node"`, `UpsertEdge → "upsert_edge"`, `DeleteEdge → "delete_edge"`). Same for `m.outbound_events.add(1, &[...])` at `:569` (derive from `ChangeEvent` payload). The `origin` label may be deferred to Phase 6 OR recorded as a constant (e.g., `"user"`) since the echo filter already removed `ORIGIN_GRAFEO_BRIDGE` / `ORIGIN_LORO_BRIDGE`.
3. **[MAJOR 2 test]** Add 1 test in `tests/unit/telemetry.rs` that constructs a `MetricsRegistry` from a real SDK meter (use `opentelemetry_sdk::metrics::SdkMeterProvider` + a test `MetricReader` that captures recorded attributes) + asserts that `record_*` calls record the correct labels. This is a Phase 5+ test improvement — may be deferred to Phase 6 if the SDK setup is too heavy.

---

## 11. API Verification Table

| API call | Crate | File:line | Verified? |
|---|---|---|---|
| `meter.u64_counter(name)` | opentelemetry-0.23.0 | `src/metrics/meter.rs:273` | ✅ |
| `meter.f64_histogram(name)` | opentelemetry-0.23.0 | `src/metrics/meter.rs:385` | ✅ |
| `Counter::add(value, &[KeyValue])` | opentelemetry-0.23.0 | `src/metrics/instruments/counter.rs:35` | ✅ |
| `Histogram::record(value, &[KeyValue])` | opentelemetry-0.23.0 | `src/metrics/instruments/histogram.rs:34` | ✅ |
| `Tracer::span_builder(name)` | opentelemetry-0.23.0 | `src/trace/tracer.rs:162` | ✅ |
| `SpanBuilder::start(tracer) -> T::Span` | opentelemetry-0.23.0 | `src/trace/tracer.rs:374` | ✅ |
| `KeyValue::new(K, V)` | opentelemetry-0.23.0 | `src/common.rs:413` | ✅ |
| `Value: From<i64>` (NOT `From<u64>`) | opentelemetry-0.23.0 | `src/common.rs:349-362` (`from_values!` macro) | ✅ |
| `Value: From<String>` | opentelemetry-0.23.0 | `src/common.rs:371` | ✅ |
| `global::meter(name) -> Meter` | opentelemetry-0.23.0 | `src/global/metrics.rs:115` | ✅ |
| `global::tracer(name) -> BoxedTracer` | opentelemetry-0.23.0 | `src/global/trace.rs:394` | ✅ |
| `global::shutdown_tracer_provider()` | opentelemetry-0.23.0 | `src/global/trace.rs:421` | ✅ |
| `global::shutdown_meter_provider()` | opentelemetry-0.23.0 | (does NOT exist in 0.23) | ✅ (L3 correctly notes + justifies) |
| `parking_lot::RwLock::try_read() -> Option<...>` | lock_api-0.4.14 (via parking_lot-0.12.5) | `src/rwlock.rs:472-479` | ✅ returns `Option` (Some=acquired, None=contention) |
| `AtomicU64::load(Ordering::Relaxed)` | std | stdlib | ✅ |
| `AtomicU64::store(value, Ordering::Relaxed)` | std | stdlib | ✅ |
| `AtomicU64::fetch_max(value, Ordering::Relaxed)` | std | stdlib | ✅ |
| `GrafeoDB::session() -> Session` | grafeo-engine-0.5.42 | `src/database/mod.rs:1663` | ✅ |
| `GrafeoDB::session_with_cdc(bool) -> Session` | grafeo-engine-0.5.42 | `src/database/mod.rs:1728` | ✅ |
| `Session::execute(&str) -> Result<QueryResult>` | grafeo-engine-0.5.42 | `src/session/mod.rs:2636` | ✅ |
| `GrafeoDB::execute(&str)` | grafeo-engine-0.5.42 | (does NOT exist) | ✅ (Devil M1 caught; arch §23.3 patched in P5-L2) |
| `SystemTime::now().duration_since(UNIX_EPOCH)` | std | stdlib | ✅ |
| `std::time::Instant::now()` | std | stdlib | ✅ |
| `tokio::time::timeout(duration, future)` | tokio | tokio-1.x stdlib | ✅ |
| `tokio::task::spawn_blocking(closure)` | tokio | tokio-1.x stdlib | ✅ |
| `tokio::spawn(future)` | tokio | tokio-1.x stdlib | ✅ |
| `parking_lot::Mutex::lock()` | parking_lot-0.12.5 | `src/mutex.rs` | ✅ |
| `parking_lot::RwLock::read()` / `.write()` | parking_lot-0.12.5 | `src/rwlock.rs` | ✅ |
| `loro::LoroDoc::new()` | loro-1.13.6 | `src/lib.rs:137` (P4-HUNT verified) | ✅ |
| `loro::LoroDoc::import_with(bytes, origin)` | loro-1.13.6 | `src/lib.rs:721` (P4-HUNT verified) | ✅ |
| `loro::LoroDoc::export(ExportMode)` | loro-1.13.6 | `src/lib.rs:1306` (P4-HUNT verified) | ✅ |
| `loro::LoroDoc::oplog_frontiers()` | loro-1.13.6 | `src/lib.rs:948` (P4-HUNT verified) | ✅ |
| `loro::LoroDoc::get_map(name)` | loro-1.13.6 | `src/lib.rs:489` (P4-HUNT verified) | ✅ |
| `loro::LoroDoc::subscribe_root(handler)` | loro-1.13.6 | (P2T3-HUNT verified) | ✅ |
| `loro::LoroDoc::set_next_commit_origin(origin)` | loro-1.13.6 | (P2T3-HUNT verified) | ✅ |
| `loro::LoroDoc::commit()` | loro-1.13.6 | (P2T3-HUNT verified) | ✅ |
| `lorosurgeon::Hydrate::hydrate_map(&LoroMap)` | lorosurgeon-0.2.1 | `src/hydrate.rs:64` (P3T2-HUNT verified) | ✅ |
| `grafeo::GrafeoDB::new_in_memory()` | grafeo-engine-0.5.42 | `src/database/mod.rs:267` (P4-HUNT verified) | ✅ |
| `grafeo::GrafeoDB::with_config(Config::persistent(p))` | grafeo-engine-0.5.42 | `src/database/mod.rs:346` (P4-HUNT verified) | ✅ |
| `grafeo::GrafeoDB::current_epoch()` | grafeo-engine-0.5.42 | (P2T3-HUNT verified) | ✅ |

**Verification method**: `rg -n "<api_name>" ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/<crate>-<version>/src/` for each API call cited in P5-L3 worklog + source code. Zero hallucinations found.

---

## 12. Verification Commands Run

```bash
# Compile + test verification
cargo check --all-targets       # exit 0, 1 pre-existing warning (presence::socket::room_id never read)
cargo test --all                 # 6 lib + 5 integration + 71 unit + 2 ignored = 82 passed, 0 failed

# Plenger sweeps
rg -n "from_sync_engine_with_config|with_batch_config" src/ tests/   # 0 backward-compat slaves
rg -n "#\[deprecated" src/                                          # 0 deprecation evasion
rg -n "\.await" src/telemetry/ src/bridge/ src/hydration/ src/app.rs # 0 lock-across-.await
rg -n "tokio::spawn|spawn_blocking" src/                            # 0 non-Send captures
rg -n "AtomicU64|AtomicUsize" src/telemetry/                       # Relaxed ordering correct
rg -n "\.ok\(\)|unwrap_or_default|unwrap_or\(|if let Err.*continue|let _ = " src/telemetry/ src/bridge/ src/hydration/ src/app.rs
rg -n "TODO|FIXME|HACK|XXX" src/telemetry/ src/bridge/ src/hydration/ src/app.rs
rg -n "fn fmt|impl Display" src/telemetry/
rg -n "KeyValue::new|AttributeValue" src/telemetry/
rg -n "now\(\)|SystemTime|Instant" src/telemetry/ src/bridge/ src/app.rs
rg -n "\.unwrap\(\)|\.expect\(|panic!\(" src/telemetry/ src/bridge/ src/hydration/ src/app.rs
rg -n "TODO\(P5-L3\)" src/                                          # 0 remaining (all 34 filled)
rg -n "unimplemented!\(\)" src/                                     # 11 pre-existing (L3 worklog said 8 — NIT 1)

# API verification (each verified against cargo registry source)
rg -n "pub fn u64_counter|pub fn f64_histogram" ~/.cargo/registry/src/index.crates.io-*/opentelemetry-0.23.0/src/metrics/
rg -n "fn record\b|fn add\b" ~/.cargo/registry/src/index.crates.io-*/opentelemetry-0.23.0/src/metrics/instruments/
rg -n "fn span_builder|fn start\b" ~/.cargo/registry/src/index.crates.io-*/opentelemetry-0.23.0/src/trace/
rg -n "pub fn shutdown_tracer_provider|pub fn tracer\b|pub fn meter\b" ~/.cargo/registry/src/index.crates.io-*/opentelemetry-0.23.0/src/global/
rg -n "pub fn new\b" ~/.cargo/registry/src/index.crates.io-*/opentelemetry-0.23.0/src/common.rs
rg -n "impl From" ~/.cargo/registry/src/index.crates.io-*/opentelemetry-0.23.0/src/common.rs
rg -n "pub fn try_read" ~/.cargo/registry/src/index.crates.io-*/lock_api-0.4.14/src/rwlock.rs
rg -n "pub fn session\b|pub fn session_with_cdc|pub fn execute\b" ~/.cargo/registry/src/index.crates.io-*/grafeo-engine-0.5.42/src/database/mod.rs
rg -n "pub fn execute\b" ~/.cargo/registry/src/index.crates.io-*/grafeo-engine-0.5.42/src/session/mod.rs
```

---

## 13. PAT Safety Check

```bash
$ rg -n "ghp_" docs/critiques/p5-hunt.md worklog.md
# exit 1 (no matches) — no PAT leakage in committed files
```

✅ PAT safety check passed.
