//! Phase 3 Task 3 tests: `hydration::vector::generate_local_embedding`.
//!
//! All 4 scaffolds are `#[ignore]`'d — L3 fills the bodies, then un-ignores
//! them. Spec validation gate: Phase 3 Task 3 has NO standalone spec test
//! (per ORCH-P3T3-SETUP worklog line 3662); the relevant gate is "Vector never
//! written to Loro container" which is Task 4's contract. These 4 scaffolds
//! cover the Task 3-specific properties: determinism, input-sensitivity,
//! dimension, and the ONNX-not-integrated warning log.
//!
//! # Verified API surface (cheat sheet for L3)
//!
//! - `grafeo_loro::hydration::vector::generate_local_embedding(&str) -> Result<Vec<f32>>`
//!   — sync, infallible at the stub layer; real ONNX may fail (P3T3-L1 decision
//!   1: future-proofed via `Result` so Task 4's call site never needs to be
//!   re-typed). Also re-exported at the crate root as
//!   `grafeo_loro::generate_local_embedding` (P3T3-L2 m2).
//! - `grafeo_loro::constants::DEFAULT_EMBEDDING_DIM: usize = 384` — SSOT for
//!   output length; matches `sentence-transformers/all-MiniLM-L6-v2` preset
//!   (`grafeo-engine-0.5.42/src/embedding/config.rs:18`).
//! - `tracing::warn!` macro — already used in 30+ call sites in the codebase
//!   (grep `tracing::warn!` `src/bridge/*.rs`); message constant:
//!   `"ONNX not integrated; returning deterministic dummy embedding"`.
//! - `std::sync::Once` — used to fire the warning exactly once per process
//!   (P3T3-L1 decision 6: rate-limit to avoid log-spam under batch loops).
//! - `grafeo_engine::embedding::EmbeddingModel` trait —
//!   `grafeo-engine-0.5.42/src/embedding/mod.rs:39` (sync, NOT async). `embed`
//!   at `mod.rs:47` (NOT `:46` — P3T3-DEVIL NIT 3 off-by-one correction),
//!   `dimensions` at `mod.rs:50`, `name` at `mod.rs:53`.
//! - grafeo-engine ships a `MockEmbeddingModel` at
//!   `grafeo-engine-0.5.42/src/embedding/mod.rs:62-93` (`#[cfg(test)]`-private)
//!   with algorithm `t.bytes().map(|b| b as f32).sum::<f32>()` → seed →
//!   `((seed + i as f32) * 0.01).sin()` per dim — almost identical shape to L3's
//!   fold-seed-PRNG algorithm. Algorithm reference only, NOT reusable from
//!   grafeo-loro (P3T3-DEVIL MINOR 4).
//!
//! # L3 algorithm hint (informational, not binding)
//!
//! 1. `let seed = text.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));`
//! 2. Seed a deterministic PRNG (`SplitMix64` from `rand` 0.8, or hand-rolled
//!    `SplitMix64` to avoid adding a `rand` dep — verify against `Cargo.toml`).
//! 3. Emit `DEFAULT_EMBEDDING_DIM` `f32` samples in `[0.0, 1.0)`.
//! 4. Wrap in `Once::call_once(|| tracing::warn!(...))`.
//!
//! # Edge cases (anti-happy-path)
//!
//! - Empty input `""` MUST still produce a `DEFAULT_EMBEDDING_DIM`-length vector
//!   (fold yields `0u64` as the seed — PRNG must handle a zero seed).
//! - Single-character input MUST differ from empty input (collision resistance).
//! - Two distinct long inputs MUST differ in at least one slot.
//! - Calling twice in the same process MUST yield byte-identical output
//!   (idempotency — anti-plenger #9).

#![allow(unused_imports)]
// TODO(L3): remove the silencer above when filling test bodies (matches the
// P3T1-L1 → P3T1-L3 trajectory per ORCH-P3T1-CLOSE: "grep -rn 'allow(unused_imports)'
// tests/unit/compression.rs → 0 matches (silencer removed when bodies filled)").

use grafeo_loro::constants::DEFAULT_EMBEDDING_DIM;
use grafeo_loro::hydration::vector::generate_local_embedding;

/// Hand-rolled tracing-subscriber `Layer` that counts `WARN` events (DEVIL Q3
/// — Option A decided at L2: adds `tracing-subscriber` to `[dev-dependencies]`
/// for the `Layer` trait; ~15 LOC vs ~50 LOC for direct `Subscriber` impl).
/// Anti-plenger #5 Bloat: do NOT reinvent `tracing_subscriber::Layer`.
///
/// # Once-global-state constraint (DEVIL Q3 caveat)
///
/// `generate_local_embedding` uses `std::sync::Once` to fire the ONNX-stub
/// warning exactly once per process. The FIRST test in the process that calls
/// `generate_local_embedding` will see the warning; subsequent tests in the
/// same process will see 0 warnings (Once already fired). L3 MUST either:
/// 1. Run with `cargo test -- --test-threads=1` AND ensure the warning test is
///    the first to call `generate_local_embedding` in the process; OR
/// 2. Move the warning test to its own integration-test binary (fresh process
///    per `cargo test` invocation).
///
/// L2 wires the infrastructure; L3 implements the test body.
mod test_capture {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing::Event;
    use tracing::Level;
    use tracing::Subscriber;
    use tracing_subscriber::layer::Context;
    use tracing_subscriber::Layer;

    /// Tracing layer that increments an `AtomicUsize` on every `WARN` event.
    /// L3 installs this via
    /// `tracing_subscriber::registry().with(WarnCounter::new()).set_default(|| ...)`.
    pub struct WarnCounter {
        count: AtomicUsize,
    }

    impl WarnCounter {
        pub fn new() -> Self {
            Self { count: AtomicUsize::new(0) }
        }

        pub fn get(&self) -> usize {
            self.count.load(Ordering::SeqCst)
        }
    }

    impl<S> Layer<S> for WarnCounter
    where
        S: Subscriber,
    {
        fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
            if event.metadata().level() == &Level::WARN {
                self.count.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

/// Same input → byte-identical `Vec<f32>` across two calls. Catches
/// non-deterministic PRNG seeding (e.g. `Instant::now()` as seed), mutable
/// global state, or accidental `&mut` capture in the closure.
#[test]
#[ignore = "P3T3-L1 scaffold: L3 implements the body"]
fn generate_local_embedding_is_deterministic() {
    let text = "grafeo-loro phase 3 task 3 determinism probe";
    let _ = (text, DEFAULT_EMBEDDING_DIM, generate_local_embedding);
    todo!()
}

/// Emits `tracing::warn!` with message `"ONNX not integrated; returning
/// deterministic dummy embedding"` exactly ONCE per process (via
/// `std::sync::Once`). Subsequent calls MUST NOT re-emit.
///
/// # Test capture (DEVIL Q3 — Option A decided at L2)
///
/// L2 wired `mod test_capture::WarnCounter` (a `tracing_subscriber::Layer`
/// that counts `WARN` events via `AtomicUsize`). L3 installs it via
/// `tracing_subscriber::registry().with(WarnCounter::new()).set_default(|| ...)`
/// inside the test body, calls `generate_local_embedding`, then asserts
/// `counter.get() == 1`.
///
/// `tracing-subscriber` was added to `[dev-dependencies]` at `Cargo.toml`
/// (1 line, standard companion to `tracing`). Alternative considered: hand-
/// rolled `Subscriber` impl (~50 LOC, no dep) — rejected as anti-plenger #5
/// Bloat (reinvents `tracing_subscriber::Layer`). Option C (skip the
/// warning-count test; manual `cargo test -- --nocapture` smoke) rejected
/// because it loses automated test coverage (anti-plenger #8 Observability).
///
/// ## Once-global-state constraint
///
/// `std::sync::Once` is process-global. See `mod test_capture` docstring: L3
/// MUST run with `--test-threads=1` AND ensure this test is the first to call
/// `generate_local_embedding` in the process, OR move this test to its own
/// integration-test binary (fresh process).
#[test]
#[ignore = "P3T3-L1 scaffold: L3 implements the body"]
fn generate_local_embedding_logs_onnx_warning() {
    // Reference WarnCounter to suppress dead-code warning at L2 (L3 will use
    // it for the actual assertion when implementing the body).
    let _ = test_capture::WarnCounter::new().get();
    let _ = (DEFAULT_EMBEDDING_DIM, generate_local_embedding);
    todo!()
}

/// Different inputs → different `Vec<f32>`. Catches a fixed-constant
/// implementation (e.g. `vec![0.0; N]` regardless of input). Anti-Goodhart:
/// `assert_ne!` on the full `Vec<f32>`, not just on length.
#[test]
#[ignore = "P3T3-L1 scaffold: L3 implements the body"]
fn generate_local_embedding_respects_input() {
    let a = "alice";
    let b = "bob";
    let _ = (a, b, DEFAULT_EMBEDDING_DIM, generate_local_embedding);
    todo!()
}

/// Output length is always `DEFAULT_EMBEDDING_DIM` (384). Catches an
/// off-by-one or a hardcoded literal drift between the function and the
/// `constants::DEFAULT_EMBEDDING_DIM` SSOT.
#[test]
#[ignore = "P3T3-L1 scaffold: L3 implements the body"]
fn generate_local_embedding_dimension_is_default() {
    let text = "dimension probe";
    let _ = (text, DEFAULT_EMBEDDING_DIM, generate_local_embedding);
    todo!()
}
