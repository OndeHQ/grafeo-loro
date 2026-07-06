//! Phase 6 Task 5 fuzz harness — random Loro op batches → verify Grafeo consistency.
//!
//! L2 wiring (per klemer-agents.md): define `FuzzInput` + `FuzzOp` enum + invariant
//! check fn skeletons. Complex algorithms (op generation, invariant assertions, batch
//! application) stay as `// TODO: L3`.
//!
//! # Build requirements
//!
//! `libfuzzer-sys` requires nightly Rust + `-Zsanitizer=address`. Use `cargo fuzz`:
//! ```text,ignore
//! rustup install nightly
//! cargo +nightly install cargo-fuzz
//! cargo +nightly fuzz run consistency
//! ```
//! `cargo fuzz` manages the nightly toolchain + `--cfg fuzzing` automatically.
//! Plain `cargo check` on this crate will fail because `fuzz_target!` requires
//! `cfg(fuzzing)` to expand to a libfuzzer-compatible entry point. To verify
//! syntax without nightly, run:
//! ```text,ignore
//! rustc --edition 2021 --crate-type lib --emit metadata fuzz/fuzz_targets/consistency.rs
//! ```
//! See `docs/phase-6/fuzz-invariants.md` for the 15-invariant checklist (I3 split
//! into I3a/b/c per Devil C5.2; I7/I9 cadence documented per C5.3).

// When built with `--cfg fuzzing` (via `cargo +nightly fuzz run`), libfuzzer provides
// `main`; the crate is compiled as `no_main`. When built without (e.g. `cargo check`
// for syntax verification), a fallback `main` is provided at the bottom of the file.
#![cfg_attr(fuzzing, no_main)]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

// TODO: L3 — define LoroOp enum variants mirroring grafeo_loro::types::events::LoroOp
//   (UpsertNode, UpsertEdge, DeleteNode, DeleteEdge, TreeMove).

/// Top-level fuzz input. Decoded once per iteration from the raw byte stream via
/// `arbitrary::Arbitrary`. L3 will fill in the full op generator + config knobs.
#[derive(Arbitrary, Debug, Clone)]
pub struct FuzzInput {
    /// Seed for any deterministic sub-choices (e.g. peer id, branch id).
    pub seed: u64,
    /// Ordered batch of ops to apply in sequence.
    pub ops: Vec<FuzzOp>,
    // TODO: L3 — add fields for grafeo config (SsotMode, CompressionType),
    // peer count (for echo-prevention I4/I5), batcher tuning (batch_interval_ms, batch_max_size),
    // and hydration chunk size.
}

/// Mirror of `grafeo_loro::types::events::LoroOp` with `Arbitrary`-derivable field
/// types (no `HashMap` — use `Vec<(String, FuzzValue)>` and convert at apply time).
/// L3 fills in the full variant shape + conversion to `grafeo_loro::LoroOp`.
#[derive(Arbitrary, Debug, Clone)]
pub enum FuzzOp {
    /// No-op marker — lets the fuzzer explore empty/short batches cheaply.
    NoOp,
    // TODO: L3 — UpsertNode { loro_key, labels, properties: Vec<(String, FuzzValue)> }
    // TODO: L3 — UpsertEdge { src_key, dst_key, label, properties: Vec<(String, FuzzValue)> }
    // TODO: L3 — DeleteNode { loro_key }
    // TODO: L3 — DeleteEdge { src_key, dst_key, label }
    // TODO: L3 — TreeMove { node_key, old_parent_key, new_parent_key }
}

// TODO: L3 — pub enum FuzzValue { Null, Bool(bool), Integer(i64), Float(f64), String(String) }
//   Mirror of grafeo_loro::types::values::GraphValue (JSON-shaped subset only — excludes
//   Vector/List/Map which are exotic for the fuzzer; L3 may extend if needed).

// =============================================================================
// Invariant check fn skeletons (one per I1..I15 per docs/phase-6/fuzz-invariants.md).
// L2 contract: skeleton signatures only; bodies are `// TODO: L3`.
// L3 contract: each assertion must be NON-TRIVIAL — it must fail if the invariant
// is violated (per Devil M5). Use `assert_eq!(a, b)` (concrete values), NOT
// `assert!(result.is_ok())` (only catches Result::Err, not semantic violations).
// =============================================================================

// TODO: L3 — fn check_i1_tree_state_parity(app: &GrafeoLoroApp, doc: &LoroDoc) -> bool
// TODO: L3 — fn check_i2_edge_state_parity(app: &GrafeoLoroApp, doc: &LoroDoc) -> bool
// TODO: L3 — fn check_i3a_no_panic_in_apply_loro_op(session: &Session, op: &LoroOp, maps: &BridgeMaps)
// TODO: L3 — fn check_i3b_no_panic_in_batcher_run(batcher: &MutationBatcher, rx: Receiver<LoroOp>)
// TODO: L3 — fn check_i3c_no_panic_in_parallel_hydrate(db: &GrafeoDB, doc: &LoroDoc, maps: &BridgeMaps)
// TODO: L3 — fn check_i4_echo_loop_bounded(engine: &SyncEngine) -> bool
// TODO: L3 — fn check_i5_origin_filter_symmetry(engine: &SyncEngine) -> bool
// TODO: L3 — fn check_i6_ryow(app: &GrafeoLoroApp, node_id: NodeId, text: &str) -> bool
// TODO: L3 — fn check_i7_snapshot_idempotency(app: &GrafeoLoroApp, graph_id: &str) -> bool
// TODO: L3 — fn check_i8_compression_round_trip(bytes: &[u8], strategy: CompressionType) -> bool
// TODO: L3 — fn check_i9_hydration_determinism(db: &GrafeoDB, doc: &LoroDoc, maps: &BridgeMaps) -> bool
// TODO: L3 — fn check_i10_vector_offload_bypass(mgr: &VectorOffloadManager, node_id: NodeId, text: &str) -> bool
// TODO: L3 — fn check_i11_bridge_maps_bijectivity(maps: &BridgeMaps) -> bool
// TODO: L3 — fn check_i12_mvcc_snapshot_isolation(app: &GrafeoLoroApp, epoch: EpochId) -> bool
// TODO: L3 — fn check_i13_batcher_count(batcher: &MutationBatcher, expected: u64) -> bool
// TODO: L3 — fn check_i14_tree_move_serializability(db: &GrafeoDB, ops: &[LoroOp]) -> bool
// TODO: L3 — fn check_i15_presence_envelope_integrity(payload: &PresencePayload) -> bool

// =============================================================================
// Fuzz target entry point.
// L2 contract: skeleton — decode bytes into FuzzInput via Arbitrary, apply ops,
// check invariants. L3 fills in the bodies.
// =============================================================================

fuzz_target!(|input: FuzzInput| {
    // L2 wiring: FuzzInput is decoded by `arbitrary::Arbitrary` (the macro reads
    // `&[u8]` and produces a `FuzzInput`). L3 fills in:
    //   1. Build a fresh GrafeoLoroApp (or reuse with reset).
    //   2. Apply each FuzzOp in `input.ops` (convert FuzzOp → grafeo_loro::LoroOp,
    //      push through the inbound batcher).
    //   3. After the batch flushes, assert every per-iteration invariant
    //      (I1, I2, I3a/b/c, I4, I11, I13, I15 per Devil C5.6).
    //   4. Every 1000 iterations OR on the final iteration of the run, assert
    //      the expensive periodic invariants (I7, I9 per Devil C5.3).
    //   5. If `FuzzInput::arbitrary` returns `Err` (malformed bytes), return
    //      early — libfuzzer treats early-return as a successful iteration
    //      (correct for malformed inputs per Devil happy-path bias note).
    let _ = input;
    // TODO: L3 — decode + apply + check invariants per the per-iteration / periodic cadence.
});

// Fallback `main` for non-fuzzing builds (enables `cargo check` to pass for syntax
// verification). When `--cfg fuzzing` is set (via `cargo +nightly fuzz run`), the
// `#![cfg_attr(fuzzing, no_main)]` attribute above suppresses crate-level main and
// libfuzzer's C runtime provides the real entry point that calls
// `rust_fuzzer_test_input` (generated by `fuzz_target!`).
#[cfg(not(fuzzing))]
fn main() {
    // Fuzz target is built via `cargo +nightly fuzz run consistency`.
    // This fallback main exists only so `cargo check` passes for syntax verification.
    eprintln!("grafeo-loro-fuzz: build with `cargo +nightly fuzz run consistency` to actually fuzz");
}
