# P6 Plenger Hunt Report

**Hunter**: plenger-hunter agent (Task ID 5b — re-spawn of 5)
**Scan range**: commits 9d0cac2..4165d3f (12 commits, Phase 6 T2/T3/T4/T5)
**Date**: 2026-07-07
**Method**: incremental commits; rg-first investigation; 2-query cap per anti-pattern.

## Anti-Pattern #1: Backward Compatibility Slaves

**Hunt**: `rg '#\[allow' src/ fuzz/` + `git show 568ea7e --stat`

**Verdict**: NOT FOUND — clean by inspection.

- 8 `#[allow]` attributes total; ALL 8 have `reason=` field with documented justifications.
- Commit 568ea7e ("P6-L2-BASELINE: cargo fmt + clippy --fix + manual #[allow] cleanup (C4.1 blocker fix)") was a structural cleanup (42 files, +661/-406, mostly fmt + test lint), NOT a band-aid — it removed redundant allows before L2 wiring.
- 2 allows (`src/app.rs:247`, `src/bridge/batcher.rs:100`) defer refactor to `AppConfig`/`BatcherConfig` struct "in future phase" — pre-date Phase 6, tracked with TODOs, NOT new rot introduced by P6.
- 3 allows in `src/bridge/sync_engine.rs:459/553/661` suppress `clippy::async_yields_async` with correct design rationale (spawn_*_worker returns JoinHandle by design).

## Anti-Pattern #2: Tautology

**Hunt**: `rg 'assert!\(true\)|assert_eq!\(\w+,\s*\1\)' fuzz/` + `rg 'is_ok\(\)' fuzz/fuzz_targets/consistency.rs`

**Verdict**: NOT FOUND — clean by inspection.

- 0 `assert!(true)` in fuzz crate.
- 2 `result.is_ok()` calls in `consistency.rs`:
  - Line 327 (I3b): `JoinHandle::await` JoinError check — real assertion that the spawned batcher task didn't panic. NOT tautological.
  - Line 343 (I3c): `parallel_hydrate_grafeo` API error check, immediately followed (lines 350-356) by `assert_eq!(fresh_db.node_count(), state.live_node_keys.len())` — a real 1:1 hydration materialization comparison. The `is_ok()` is a precondition, not the actual invariant.
- Module header (line 225-229) explicitly states the non-tautology contract: "NO `assert!(result.is_ok())` shortcuts" — and the code honors it.

