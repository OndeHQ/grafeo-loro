# Phase 7 — Gap Closure (Publish-Ready)

**Branch**: `phase-6` (continued from Phase 6 close at `13f19bf`)
**Scope**: Close ALL gaps from Phase 6 to publish-ready. Un-excluded T1 (`unimplemented!()` replacement).
**Methodology**: Plonga-Plongo-Loop (klemer L1→L2→L3 + Devil's advocate + plenger hunter)
**Session 2 commits**: 28 (since `13f19bf`)

## Gaps Closed

### Gap A — T1: Replace all 11 `unimplemented!()` calls
- **2 new error variants**: `NotYetImplemented(String)` + `InvalidEnvelope(String)` in `src/error.rs`
- **7 `Result`-returning fns**: replaced `unimplemented!()` with `Err(GrafeoLoroError::NotYetImplemented(...))`
  - `GrafeoLoroApp::query`, `update_text`, `generate_embedding`, `broadcast_presence`
  - `SsotMode::Grafeo` arms in `checkpoint` + `hydrate`
  - `PresenceManager::broadcast` (uses `room_id` in error message)
- **2 real implementations**: `parse_eph_envelope` + `build_eph_envelope` (`%EPH` wire format per arch §12)
- **1 real stub**: `PresenceManager::new` (stores `room_id`, no socket — future scope)
- **1 removal**: `impl Default for AppConfig` (0 callers; force builder — anti-plenger #11)
- **Result**: 0 `unimplemented!()` macro calls in `src/`

### Gap B — I12: MVCC snapshot isolation invariant
- Filled `check_i12_mvcc_snapshot_isolation` body using grafeo `set_viewing_epoch` + `clear_viewing_epoch` API
- Algorithm: write `Int64(1)` @ E1 → pin read session at E1 → write `Int64(2)` @ E2 → assert pinned session sees `Int64(1)` (snapshot isolation) → clear override → assert session sees `Int64(2)`
- Used `grafeo::Value::Int64` (NOT `Integer` — Devil B1 caught the hallucination)
- 3 non-trivial assertions (no Goodhart)

### Gap C — Deferred child-spans note updated
- Note now accurately states: T1 replaces panics with errors, NOT real implementations → child spans still depend on future-phase work

### Gap D — Structural `#[allow]` refactors
- **Removed 2 `too_many_arguments`**: `from_sync_engine_with_telemetry` → `AppTelemetryConfig` struct (7 fields, in `src/app.rs`); `MutationBatcher::new` → `BatcherConfig` struct (8 fields, in `src/bridge/batcher.rs`)
- **Removed 1 `dead_code`**: `PresenceManager::room_id` (now read in `broadcast` error message)
- **Kept 3 `async_yields_async`**: updated reasons to permanent design language (not TODO)
- All 4 remaining `#[allow]`s have `reason=` + are permanent design choices (0 TODOs)

### Gap E — EncFuzz consolidation
- Moved `FuzzOp` + `FuzzValue` + `convert_fuzz_op` from `consistency.rs` to `fuzz/fuzz_targets/lib.rs`
- Removed `EncFuzzOp`/`EncFuzzValue` mirror types + their `#[allow(dead_code)]`
- Both binaries (`consistency` + `gen_corpus`) now share the SSOT types

### Gap F — Stale doc-comments fixed (3)
- `telemetry/health.rs`: "All method bodies are unimplemented!()" → "Fully implemented in P5-L3"
- `telemetry/metrics.rs`: same
- `app.rs`: "All methods remain unimplemented!()" → "7 return Err(NotYetImplemented(...))"

### Gap G — Stale NOTE comments removed (7)
- All `// NOTE: body unimplemented!() — T1 excluded` comments removed from `app.rs` + `presence/socket.rs`

### Gap M2 — I15 tests rewritten
- I15 presence envelope tests now use production `build_eph_envelope`/`parse_eph_envelope` APIs (not hand-rolled envelopes)
- Positive path: round-trip `build → parse → assert_eq`
- Negative paths: bad magic, truncated, bad serde → `Err(InvalidEnvelope(...))`

## Plonga-Plongo-Loop Execution (Session 2)

| Step | Agent | Task ID | Outcome |
|------|-------|---------|---------|
| L1 | L1-scaffolding | G1 | 663-line gap-closure plan, 6 open questions |
| Devil | Devil-advocate | G2 | ACCEPTED-WITH-FIXES: 2 blockers (Int64, fuzz call site) + 3 majors + Q1-Q6 rulings |
| L2+L3 | L2-L3-implementer | G3 | 10 commits (shell hung after 3; orchestrator completed 4-10 directly) |
| L2+L3 (fuzz) | L3-fuzz | G3b | 4 commits (EncFuzz + I12 + I15 + fmt cleanup) |
| Doc fixes | Orchestrator | — | 1 commit (stale doc-comment mentions in app.rs) |
| Hunt | plenger-hunter | G4 | 10 incremental commits; verdict CLEAN-WITH-NITS — PUBLISH-READY |

**Max-turns hits**: 1 (G3 shell hung — orchestrator completed directly per traits #5)
**Total session 2 commits**: 28

## Final Gate Verification

| Gate | Result |
|------|--------|
| `cargo check --all` | PASS |
| `cargo fmt --all --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo test --all` | 82/82 pass (6 lib + 5 integration + 71 unit), 2 ignored (pre-existing) |
| `cd fuzz && cargo check` | PASS |
| `cd fuzz && cargo fmt --all --check` | PASS |
| `cd fuzz && cargo clippy --all-targets -- -D warnings` | PASS |
| `cd fuzz && cargo run --bin gen_corpus` | PASS (5 seed files: 12/76/192/141/12446 bytes) |

## Plenger Audit (final — Hunter G4)

- **Blockers**: 0
- **Majors**: 0
- **Minors**: 0
- **Nits**: 2 (non-blocking, well-documented)
  - N1: `FuzzState.db` field reserved-but-unused (dead_code allow with reason)
  - N2: I15 negative-path-3 hand-constructs prefix bytes (mitigated by positive-path dependency on production API)

All 8 publish-ready checklist items PASS.

## Publish-Ready State

- **0 `unimplemented!()` macro calls** in `src/` (was 11 at session start)
- **0 `#[allow]` with TODO/deferred language** (4 permanent-design allows remain, all with `reason=`)
- **0 stale NOTE comments** (was 7)
- **0 stale doc-comments** mentioning `unimplemented!()` (1 accurate reference in `error.rs` describing the new variant)
- **I12 does REAL snapshot isolation verification** (3 non-trivial assertions)
- **I15 tests use production APIs** (not hand-rolled envelopes)
- **No EncFuzz mirror types** (consolidated into `lib.rs`)
- **All 8 gates pass** (main + fuzz: check/fmt/clippy/test)
- **2 new error variants** (`NotYetImplemented`, `InvalidEnvelope`) for safe API surface
- **3 new config structs** (`AppTelemetryConfig`, `BatcherConfig`, `EphEnvelope`) replacing 17-arg signatures

## Remaining Future-Phase Work (NOT gaps — documented dependencies)

1. **7 `Err(NotYetImplemented(...))` returns**: these are intentional error returns for future-phase scope (query, update_text, generate_embedding, broadcast_presence, SsotMode::Grafeo checkpoint/hydrate, PresenceManager::broadcast). They are NOT gaps — they are safe API surfaces that return errors instead of panicking.
2. **13 deferred child spans**: depend on the actual function implementations (future phases), not on T1. T1 only replaced panics with errors.
3. **2 non-blocking nits**: `FuzzState.db` reserved field + I15 negative-path-3 hand-constructed prefix. Both documented with mitigation rationale.
