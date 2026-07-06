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

## Anti-Pattern #8: Goodhart's Law in Action — HIGHEST RISK

**Hunt**: Read all 16 `check_iN_*` fn bodies (lines 235-775) + call sites (lines 891-972) + seed corpus.

**Per-fn verdict (16 total)**:
- **I1** (line 235): REAL — `assert_eq!(bridge_keys, loro_keys)` (set equality, two distinct HashSets).
- **I2** (line 250): WEAK — `assert_eq!(bridge_edges.len(), loro_edges.len())` (length only, NOT set equality like I1). Could miss same-count-different-keys drift. **MINOR**.
- **I3a** (line 270): REAL — `assert_eq!(state.op_count, requested_ops)` (concrete count comparison).
- **I3b** (line 283): REAL — `assert!(result.is_ok())` on `JoinHandle::await` (JoinError = panic detected; meaningful).
- **I3c** (line 338): REAL — `assert!(result.is_ok())` + `assert_eq!(fresh_db.node_count(), state.live_node_keys.len())` (1:1 hydration materialization).
- **I4** (line 363): REAL — `assert!(epoch_count <= max)` (concrete upper bound).
- **I5** (line 378): REAL — `assert_eq!(filtered_after, filtered_before + 1)` + `assert_eq!(events_after, events_before)` (concrete counter deltas).
- **I6** (line 438): REAL — `assert_eq!(prop, expected)` (write/read value equality).
- **I7** (line 475): REAL — `assert_eq!(wire1, wire2)` (byte-vector equality, two compression runs).
- **I8** (line 498): REAL — `assert_eq!(decompressed, sample)` (round-trip byte equality, all 3 strategies).
- **I9** (line 537): REAL-with-documented-limitation — `assert_eq!(db1.node_count(), db2.node_count())` + edge_count + BridgeMaps len. Doc-comment (line 535-536) explicitly notes "Full byte-identical comparison of GrafeoDB is not exposed by the public API". Honest limitation, NOT Goodhart.
- **I10** (line 575): REAL — `assert!(embedding.is_some())` + `assert!(matches!(embedding, Some(grafeo::Value::Vector(_))))` (presence + type check).
- **I11** (line 629): REAL — bijectivity: forward/inverse map length equality + per-entry `node_key_map.get(v).is_some_and(|inv| inv == k)` for BOTH node and edge maps.
- **I12** (line 672): NO-OP (empty body) — honestly deferred per Phase 6 T1 user exclusion. Doc-comment (lines 665-671) explicitly says "Intentionally empty — see doc-comment. NOT a stub; the check genuinely cannot be implemented until Phase 6 T1 fills `GrafeoLoroApp::query`." Acceptable known gap, NOT Goodhart (not pretending to verify).
- **I13** (line 682): **NO-OP / TAUTOLOGY — MAJOR GOODHART**. The fn body `assert!(batcher_buffer_is_empty, ...)` is fed a HARDCODED `true` at the call site (line 909): `check_i13_batcher_count(true, op_count);`. The call-site comment (lines 901-908) honestly admits "We can't access the batcher's private `buffer` field from here, so we pass `true`". This makes I13 a tautology — `assert!(true)` — exactly the Goodhart pattern. The honest comment saves it from being malicious, but the invariant is unverified. **MAJOR** (not blocker: I3b indirectly verifies the batcher drains via JoinHandle success, so the underlying behavior IS tested elsewhere; the I13 fn itself is dead weight that should either be removed or refactored to expose a real `buffer_is_empty()` accessor on `MutationBatcher`).
- **I14** (line 697): REAL — BFS cycle detection from every node, `panic!` on revisit. Concrete structural check.
- **I15** (line 728): REAL — magic-prefix `assert_eq!` + 5 per-field `assert_eq!` round-trip checks + negative test (`bad_bytes` rejection).

**Seed corpus** (`fuzz/corpus/consistency/`):
- 5 files, all different sizes (12B, 76B, 192B, 141B, 12446B) — good.
- `empty.bin` (12 bytes): `2a 00 00 00 00 00 00 00 01 64 00 00` — structured FuzzInput encoding (seed=0x2a=42, op_count=1, peer_count=0x64, bail=0x0000), NOT zero bytes. Valid empty-ops scenario.
- `single_upsert.bin` (76 bytes): contains real LoroKey `V/alice` + label `Person` + property `name=Alice` + `age=30` — genuinely different scenario.
- All 5 seeds encode distinct op batches (not duplicates).

**Summary**: 13/16 REAL; 1 WEAK (I2, MINOR); 1 honest NO-OP (I12, acceptable); 1 TAUTOLOGY (I13, MAJOR Goodhart).

## L3 Risk R1: T5 invariant checks Goodhart

**Resolution**: CONFIRMED (1 MAJOR) — cross-reference Anti-Pattern #8 above.

- I13 `check_i13_batcher_count(true, op_count)` (consistency.rs:909) hardcodes the `batcher_buffer_is_empty` parameter to `true`, making the fn's `assert!(batcher_buffer_is_empty, ...)` a tautology (`assert!(true)`).
- Honest call-site comment (lines 901-908) admits the limitation, so this is NOT malicious Goodhart — but the I13 invariant IS unverified by this fn.
- Mitigating factor: I3b (line 283) indirectly verifies the batcher drains via `JoinHandle::await` success (panic = JoinError). So the underlying behavior is tested, just not by I13.
- **Action (Fixer)**: either (a) remove `check_i13_batcher_count` entirely (I3b covers the behavior), or (b) add a `pub fn buffer_is_empty(&self) -> bool` accessor to `MutationBatcher` in `src/bridge/batcher.rs` and pass the real value. Option (a) is simpler (Deletion over addition — anti-plenger #11).

## L3 Risk R2: T5 #[allow] audit

**Resolution**: REFUTED — clean.

- 2 `#[allow]` attributes in fuzz crate, BOTH have `reason=`:
  - `fuzz/fuzz_targets/gen_corpus.rs:208` — `#[allow(dead_code, reason = "mirror of FuzzValue; all variants kept for parity")]` on `EncFuzzValue` enum. Doc-comment (line 207) explains "even if the 5 seed scenarios only exercise a subset". No TODO needed — the reason is structural (mirror-type parity), not a deferred fix.
  - `fuzz/fuzz_targets/consistency.rs:208` — `#[allow(dead_code, reason = "reserved for future invariant checks that need direct db access")]` on `FuzzState.db` field. No TODO needed — the field is reserved for future invariants (e.g., when T1 fills `GrafeoLoroApp::query`, I12 may use it).
- Neither has an associated TODO (neither needs one — both are intentional reservations, not deferred fixes).
- 0 `#[allow]` without `reason=` in fuzz crate.

## L3 Risk R3: T3 README hallucination

**Resolution**: REFUTED — clean.

- 15+ API references in README.md verified against `src/`:
  - `GrafeoLoroApp` (README:11,15,18,20,114) — `src/app.rs:64`.
  - `GrafeoLoroAppBuilder` (README:15,91,93,114) — `src/app.rs:112`.
  - `GrafeoLoroApp::builder()` (README:11,20) — `src/app.rs:169`.
  - `GrafeoLoroApp::hydrate` (README:21,114) — `src/app.rs:727` (`pub async fn hydrate`).
  - `GrafeoLoroApp::checkpoint` (README:23,114) — `src/app.rs:479` (`pub async fn checkpoint`).
  - `GrafeoLoroApp::create_vertex` (README:15,114) — `src/app.rs:348`.
  - `GrafeoLoroApp::query` (README:15) — `src/app.rs:360` (README honestly notes "currently `unimplemented!()` per user scope exclusion").
  - `VertexBuilder` (README:15,114) — `src/app.rs:1552`.
  - `AppConfig` (README:15,91) — `src/config.rs:17`.
  - `SsotMode` (README:18,20,91,97) — `src/config.rs:2`.
  - `CompressionType` (README:18,20,98,99) — `src/config.rs:9`.
  - `StorageBackend` (README:11,18,19,57,107) — `src/storage/traits.rs:2`.
  - `GrafeoLoroError::Config` (README:91,107) — `src/error.rs:21`.
  - `DEFAULT_CHUNK_SIZE` (README:102) — `src/constants.rs:24` (= 256, matches README default).
  - `parallel_hydrate_grafeo` (README:21,29,102) — used in fuzz crate, exists.
  - `MutationBatcher`, `SyncEngine`, `apply_loro_op`, `BridgeMaps`, `CompressedPayload`, `CompressedPayload::compress_to_wire`, `VectorOffloadManager`, `LoroOp`, `GraphValue`, `PresencePayload`, `PeerId` — all verified in #6 hunt.
- 0 hallucinated APIs. README prose is accurate to the codebase.

## L3 Risk R4: T2 deferred child spans

**Hunt**: `rg 'defer|Deferr' docs/phase-6/instrument-plan.md`

**Resolution**: REFUTED — actionable, not rot-hiding.

- The deferred-note (lines 251-272) has a SPECIFIC trigger condition: "Deferred until Phase 6 T1 (unimplemented!() replacement) is done — child spans on panicking bodies are observationally pointless." (line 251).
- Concrete reason: child spans require inline `tracing::info_span!(...)` calls inside method bodies (line 253); bodies are currently `unimplemented!()` (would panic before any child span could fire).
- Concrete L3 placement instructions provided (lines 255-270): a table mapping each of the 13 child spans to its host method (e.g., `decompress_snapshot` → `GrafeoLoroApp::hydrate` after `storage.load`).
- Line 272 explicitly states the L3 action: "L3 adds inline `info_span!` calls for the children when bodies are written."
- This is honest tech-debt documentation with a clear trigger (T1 completion), NOT permanent rot-hiding. The trigger is conditional on a future phase (T1 was user-excluded for P6), but the deferral is technically correct (can't instrument a panicking body).
- **NIT**: to guard against permanent rot, the note could add a "re-evaluate when T1 is scoped" reminder. Currently relies on the reader to follow up. Low-priority.

