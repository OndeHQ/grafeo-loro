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

