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

## #3 Context Blindness — CLEAN

**Hunt 1**: `rg -n 'set_viewing_epoch|clear_viewing_epoch' fuzz/fuzz_targets/consistency.rs` → I12 correctly:
- L588: `read_session.set_viewing_epoch(e1)` (pin to old epoch)
- L629: `read_session.clear_viewing_epoch()` (release override before final read)
The override is scoped (4 lines of pinned reads between set and clear). No global state mutation leak.

**Hunt 2**: `rg -n 'InvalidEnvelope' src/presence/socket.rs` → 9 distinct error paths:
- L48: bad magic prefix
- L54: buffer too short (truncated)
- L60: insufficient bytes for room_id_len
- L69: insufficient bytes for room_id
- L76: room_id not valid UTF-8 (map_err)
- L80: insufficient bytes for msg_type
- L86: serde_json decode fail (map_err)
- L98: build_eph_envelope payload encode failure path
- L105: serde_json encode fail in build (map_err)

Architecture §12 wire format (magic + u16 room_id_len + room_id + u8 msg_type + serde_json) is fully validated. No skipped error paths. No silent `.unwrap_or_default()` swallows.

**Verdict**: CLEAN. grafeo MVCC model respected; production error surface comprehensive.
**Counts**: blockers 0, majors 0, minors 0, nits 0.

## #4 Band-Aids — CLEAN

**Hunt 1**: `rg -n 'unwrap\(\)|expect\(' src/presence/socket.rs` → 0 hits. Pure `?` operator + `map_err` for error propagation. No symptom-patching unwraps hiding real failure modes.

**Hunt 2**: All `#[allow]` blocks (re-verified from #1):
- 3× `clippy::async_yields_async` in `src/bridge/sync_engine.rs` — permanent design choice (worker spawners returning JoinHandle). Each `reason=` explicitly says "Permanent design choice, not a TODO."
- 1× `dead_code` on `FuzzState.db` field — reserved for future invariant checks (structural reservation, not rot-masking).

No `#[allow]` masks deferred rot. None use TODO/deferred language. No band-aids detected.

**Verdict**: CLEAN.
**Counts**: blockers 0, majors 0, minors 0, nits 0.

## #5 Bloat (DRY Violations) — CLEAN

**Hunt 1**: `rg -n 'fn enc_|fn decode_' fuzz/fuzz_targets/gen_corpus.rs` → 7 fns:
- `enc_u64/u16/u8/string` — primitive byte encoders (seed-corpus file format only; NOT production wire format)
- `enc_fuzz_op`/`enc_fuzz_value`/`enc_fuzz_input` — fuzz seed serializers operating on `FuzzOp`/`FuzzValue` (the shared types from lib.rs)

These are FUZZ FILE FORMAT serializers (deterministic seed .bin layout) — NOT reinventions of the production `%EPH` envelope (which lives in `src/presence/socket.rs` and uses `magic + u16 room_id_len + room_id + u8 msg_type + serde_json`). Distinct concerns: file format ≠ wire format.

**Hunt 2**: `rg -n 'EncFuzz' fuzz/ src/` → 2 hits, both historical comments:
- `fuzz/fuzz_targets/lib.rs:7` — module doc explaining the removal rationale
- `fuzz/fuzz_targets/gen_corpus.rs:51` — comment marking the prior anti-plenger #5 violation site (removed in P7-L2-E)

No `EncFuzzOp`/`EncFuzzValue` mirror types remain. Gap E consolidation verified.

**Verdict**: CLEAN.
**Counts**: blockers 0, majors 0, minors 0, nits 0.

## #6 Hallucination — CLEAN

**Hunt 1**: `rg -n 'grafeo::Value::Int64|grafeo::Value::Integer' fuzz/fuzz_targets/consistency.rs` → 5 hits:
- L568-569: doc-comment explicitly calling out that I12 uses `Int64` NOT hallucinated `Integer` (Devil B1/CA.1)
- L602: `grafeo::Value::Int64(2)` (write 2 — production API)
- L622: `Some(grafeo::Value::Int64(1))` (assertion 2)
- L633: `Some(grafeo::Value::Int64(2))` (assertion 3)

All grafeo API uses reference the REAL `Int64` variant. The codebase's `GraphValue::Integer(1)` at L578 is a DIFFERENT enum (codebase-internal type, not grafeo's). Distinct types — no shadowing.

**Hunt 2**: `rg -n 'pub fn set_viewing_epoch' ~/.cargo/registry/src/*/grafeo-engine-*/src/` → 1 hit:
- `grafeo-engine-0.5.42/src/session/mod.rs:730` — `pub fn set_viewing_epoch(&self, epoch: EpochId)` exists for real.

API surface verified. No fabricated methods.

**Verdict**: CLEAN. Devil B1 critical hallucination risk eliminated.
**Counts**: blockers 0, majors 0, minors 0, nits 0.

## #7 Happy-Path Bias — CLEAN

**Hunt 1**: `parse_eph_envelope` (`src/presence/socket.rs:46-88`) handles ALL 7 malformed cases:
1. L47-52: buffer too short for magic
2. L53-58: bad magic prefix (`XXXX` ≠ `agic`)
3. L59-63: missing room_id_len (truncated after magic)
4. L68-74: room_id segment truncated
5. L75-77: room_id not valid UTF-8 (map_err)
6. L79-83: unsupported msg_type (≠ `EPH_MSG_TYPE_PRESENCE`)
7. L85-86: serde_json decode failure (map_err)

Only 1 ok path (L87). All error paths return `GrafeoLoroError::InvalidEnvelope(...)` with specific reason strings. No silent `.unwrap_or_default()` swallows, no happy-path-only logic.

**Hunt 2**: I12 vertex-existence edge case:
- L600-601: `expect("I12: BridgeMaps missing node after write 1")` — explicit panic on missing node. Appropriate because the prior `apply_loro_op(...).expect("I12: apply_loro_op (write 1) failed")` (L580) already asserted the write succeeded; BridgeMaps missing node at this point is a genuine invariant violation (panic = correct response in fuzz target).
- `set_viewing_epoch` returns `()` per grafeo 0.5.42 (verified #6) — no Result to handle, no silent failure possible.

**Verdict**: CLEAN.
**Counts**: blockers 0, majors 0, minors 0, nits 0.

