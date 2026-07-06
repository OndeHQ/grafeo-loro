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

## Anti-Pattern #3: Context Blindness

**Hunt**: `rg 'tokio::runtime|block_on|spawn' fuzz/fuzz_targets/consistency.rs` + `rg '^use ' fuzz/fuzz_targets/consistency.rs`

**Verdict**: NOT FOUND — clean by inspection.

- Fuzz harness uses `tokio::runtime::Builder::new_current_thread()` (lines 287, 382, 601) — CORRECT for fuzzing (deterministic, lower overhead than multi_thread).
- `rt.block_on(async move {...})` (lines 299, 387, 604) is the ONLY way to enter tokio from libfuzzer's synchronous entry point — NOT a context violation.
- `tokio::spawn` (line 320) used correctly inside runtime to run `MutationBatcher::run` concurrently.
- Imports use real `grafeo_loro` crate APIs: `bridge::{apply_loro_op, BridgeMaps}`, `compression::CompressedPayload`, `config::CompressionType`, `constants::*`, `types::{EpochId, PresencePayload, ...}`, `VectorOffloadManager`. No reinvented logic — the harness respects the global async architecture.

## Anti-Pattern #4: Band-Aids

**Hunt**: `rg 'unwrap\(\)|expect\(' fuzz/fuzz_targets/consistency.rs` + `rg 'TODO.*(refactor|fix)' src/`

**Verdict**: NOT FOUND — clean by inspection.

- 19 `expect()` calls in `consistency.rs`, ALL with invariant-labeled messages (e.g., `"I5: init_loro_subscriber failed"`, `"I6: apply_loro_op failed"`, `"I10: tokio runtime construction failed"`).
- These are INTENTIONAL crash-on-failure semantics for the libfuzzer harness — if an underlying API returns Err, the fuzzer SHOULD panic (libfuzzer treats panic as a crash to investigate). This is correct design, not a band-aid. No `unwrap()` calls (which would be context-free); all are `.expect("I<n>: ...")` with diagnostic messages.
- 0 `unwrap()` calls in consistency.rs.
- 2 `TODO refactor` in src/ (`app.rs:251`, `batcher.rs:104`) — pre-existing Phase 5 wiring tech debt with documented "future phase" plan. NOT Phase 6 band-aids, NOT masking broken behavior. (Already noted in #1.)

## Anti-Pattern #5: Bloat (DRY Violations)

**Hunt**: `rg 'fn enc_|fn decode_' fuzz/fuzz_targets/gen_corpus.rs` + `rg 'fn convert_fuzz_op' fuzz/fuzz_targets/consistency.rs` + `rg 'fn (encode|decode|compress_to_wire|decompress)' src/`

**Verdict**: NOT FOUND — clean by inspection (1 NIT noted).

- `gen_corpus.rs` has 7 `enc_*` helpers (`enc_u64`, `enc_u16`, `enc_u8`, `enc_string`, `enc_fuzz_op`, `enc_fuzz_value`, `enc_fuzz_input`). These are bespoke binary writers for the seed corpus — NOT duplicating any `src/` logic (src/ has only `compress_to_wire`/`decompress`/`encode_edge_key`, all distinct purposes).
- `EncFuzzValue`/`EncFuzzOp` in gen_corpus.rs are acknowledged mirror types of `FuzzValue`/`FuzzOp` (per `#[allow]` reason "mirror of FuzzValue; all variants kept for parity"). Justified by asymmetric needs: `FuzzValue` derives `Arbitrary` (decoder for libfuzzer input); `EncFuzzValue` is writer-only (for deterministic generator). **NIT**: could potentially be unified via a single type deriving both `Arbitrary` + a serialization trait, but the `Arbitrary` derive's byte format usually differs from a hand-written encoder, so the split is pragmatic.
- `convert_fuzz_op` (consistency.rs:148) converts `FuzzOp` (fuzz-internal enum with `peer_count`/`bail_after_ops` fields) → `LoroOp` (production type). Legitimate adapter — NOT a DRY violation. `src/bridge::apply_loro_op` takes `LoroOp`, not `FuzzOp`, so the fuzz harness needs this adapter.

## Anti-Pattern #6: Hallucination

**Hunt**: `rg '^use (grafeo|loro|libfuzzer|grafeo_loro)' fuzz/fuzz_targets/consistency.rs` + `rg 'pub (fn|struct|enum) (...)' src/` + `rg 'GrafeoLoroApp::|AppConfig::|GrafeoLoroAppBuilder::|GrafeoDB::' README.md`

**Verdict**: NOT FOUND — clean by inspection.

- 10 fuzz imports verified against `src/`:
  - `grafeo::GrafeoDB`, `loro::LoroDoc`, `libfuzzer_sys::fuzz_target` — external crates.
  - `grafeo_loro::bridge::{apply_loro_op, BridgeMaps}` — exists at `src/bridge/grafeo_tx.rs:27,93`.
  - `grafeo_loro::compression::CompressedPayload` — `src/compression/wrapper.rs:25`.
  - `grafeo_loro::config::CompressionType` — `src/config.rs:9`.
  - `grafeo_loro::types::events::LoroOp` — `src/types/events.rs:14`.
  - `grafeo_loro::types::values::GraphValue` — `src/types/values.rs:72`.
  - `grafeo_loro::types::{EpochId, PresencePayload}` — PresencePayload at `src/types/presence.rs:5`; EpochId compiles (fuzz cargo check PASS per worklog line 265).
  - `grafeo_loro::VectorOffloadManager` — `src/hydration/vector.rs:11`, re-exported at `src/lib.rs:31`.
- README API references verified:
  - `GrafeoLoroApp::builder()` (README:11, 20) — `src/app.rs:169`.
  - `GrafeoDB::new_in_memory()` (README:106) — external `grafeo` crate API, used throughout tests.
  - `GrafeoLoroAppBuilder::build` (README:114) — impl at `src/app.rs:1073`.
- Compilation itself is the strongest proof: fuzz crate `cargo check PASS` (worklog line 265) means every imported symbol resolves. No hallucinated APIs.

## Anti-Pattern #7: Happy-Path Bias

**Hunt**: `rg 'Arbitrary::Err|if let Err|match .*Err\(|let _ =' fuzz/fuzz_targets/consistency.rs` + `rg 'fuzz_target!|arbitrary' fuzz/fuzz_targets/consistency.rs`

**Verdict**: NOT FOUND — clean by inspection.

- `fuzz_target!` entry (lines 781-789) explicitly handles `Arbitrary::Err` via `match FuzzInput::arbitrary(&mut u) { Ok(i) => i, Err(_) => return }` — early-return on malformed input, with comment citing "Devil happy-path bias note" from `docs/phase-6/fuzz-invariants.md`. NOT happy-path bias; this IS the defensive pattern.
- 7 `let _ = ...` calls — all deliberate fire-and-forget with contextual justification:
  - Line 315, 324: channel sends where receiver may be gone (legitimate).
  - Line 407, 832, 838: Loro map ops on potentially-malformed keys — defensive testing of error paths.
  - Line 818: `apply_loro_op(...)` with explicit comment "we log via `let _ =` and continue" — deliberate error-path testing.
- No bare `.unwrap()` in the harness; all fallible ops use `expect("I<n>: ...")` or `let _ =`.

