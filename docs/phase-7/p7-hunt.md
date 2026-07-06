# P7 Plenger Hunt Report (Gap-Closure — Publish-Ready Scan)

**Hunter**: plenger-hunter (Task ID G4)
**Scan range**: commits 13f19bf..a67fc1f (session 2 gap-closure: 16 commits — T1 + I12 + EncFuzz + config refactors + doc fixes)
**Date**: 2026-07-07
**Method**: incremental commits; rg-first; 2-query cap per anti-pattern.

## Scope Recap (16 gap-closure commits under review)

- `13b647b` P7-L2-A1: add `NotYetImplemented` + `InvalidEnvelope` error variants + serde_json dep
- `5bd5767` P7-L2-A4-D3: PresenceManager::new real stub + remove dead_code allow on room_id
- `29851c6` P7-L2-A3: implement parse_eph_envelope + build_eph_envelope (real %EPH wire format)
- `c1efa01` P7-L2-A2: replace 6 unimplemented!() with Err(NotYetImplemented) in app.rs
- `a2689c7` P7-L2-A2b: remove Default impl for AppConfig (force builder; 0 callers)
- `6f0bfc9` P7-L2-G: remove 7 stale NOTE comments (T1 no longer excluded)
- `a4ccbd2` P7-L2-F: fix 3 stale doc-comments (health.rs, metrics.rs, app.rs)
- `3cce1af` P7-L2-D1: refactor from_sync_engine_with_telemetry to AppTelemetryConfig struct
- `c6a449b` P7-L2-D2: refactor MutationBatcher::new to BatcherConfig struct
- `b31be3b` P7-L2-D4: update async_yields_async reasons to permanent design language
- `f5f0251` P7-L2-C: update deferred child-spans note
- `0fc1645` P7-L2-E: consolidate FuzzOp/FuzzValue into lib.rs, remove EncFuzz mirror types
- `5fa3886` P7-L2-B: implement I12 MVCC snapshot isolation invariant
- `6120275` P7-L2-M2: rewrite I15 tests for new %EPH wire format
- `646c2b2` P7-L2-fmt: apply rustfmt to prior P7-L2-A2/A3 commits
- `a67fc1f` P7-L2-F2: fix 3 stale doc-comment mentions of unimplemented!() in app.rs

## #1 Backward Compatibility Slaves — CLEAN

**Hunt 1**: `rg -n '#\[allow' src/ fuzz/` → 4 hits, all with `reason=`:
- `src/bridge/sync_engine.rs:461/555/663` — `clippy::async_yields_async`, reason: "spawn_*_worker returns tokio::task::JoinHandle by design — caller awaits the handle, not the spawn call. Permanent design choice, not a TODO." (3 sites, identical reason)
- `fuzz/fuzz_targets/consistency.rs:99` — `dead_code`, reason: "reserved for future invariant checks that need direct db access" (struct field `db: &'a Arc<GrafeoDB>`)

**Hunt 2**: `rg -n 'TODO.*(refactor|future|deprecated|legacy)' src/ fuzz/` → 0 hits.

**Verdict**: No backward-compat slavery. All 4 `#[allow]` use permanent design language, no TODO/deferred rot.
**Yellow flag (non-blocking)**: The `db` field on `FuzzState` has a `dead_code` allow reserved for "future invariant checks." I12 takes `db` as a function param, not via the struct field. This is reserved-space, not rot. Acceptable.
**Counts**: blockers 0, majors 0, minors 0, nits 1 (reserved `db` field — consider removing or wiring I12 through the struct).

## #2 Tautology — CLEAN

**Hunt 1**: `rg -n 'assert!\(true\)|assert_eq!\(true, true\)' fuzz/ src/` → 0 active hits. Only a comment at `fuzz/fuzz_targets/consistency.rs:871` noting "fn was a tautology (`assert!(true)`) — removed per anti-plenger #11" (historical marker, not a violation).

**Hunt 2**: I12 body (`fuzz/fuzz_targets/consistency.rs:570-638`) does REAL verification with 3 non-trivial assertions:
1. `assert!(e2.as_u64() > e1.as_u64())` — epoch must advance on commit (L610-615)
2. `assert_eq!(v_at_e1, Some(grafeo::Value::Int64(1)))` — pinned read sees old value (L620-626)
3. `assert_eq!(v_now, Some(grafeo::Value::Int64(2)))` — post-clear read sees new value (L631-637)

All asserts have descriptive failure messages with runtime context (epoch IDs, observed value). No tautology, no `.is_ok()` shortcuts, no hardcoded short-circuits.

**Verdict**: CLEAN.
**Counts**: blockers 0, majors 0, minors 0, nits 0.

