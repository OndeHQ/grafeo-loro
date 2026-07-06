use std::sync::Arc;
use std::sync::Once;
use grafeo::GrafeoDB;
use tracing::warn;
use crate::types::ids::NodeId;
use crate::error::Result;
use crate::constants::DEFAULT_EMBEDDING_DIM;

/// Manages offloaded float-vector embeddings. Vectors are never written to
/// Loro; they go direct to Grafeo's HNSW index.
pub struct VectorOffloadManager {
    db: Arc<GrafeoDB>,
}

impl VectorOffloadManager {
    /// Construct with a shared Grafeo handle.
    ///
    /// Stub — Task 4 owns the body (decides whether to spin up a background
    /// ONNX worker, preload `OnnxEmbeddingModel`, register with the inbound
    /// subscriber, etc.).
    pub fn new(db: Arc<GrafeoDB>) -> Self {
        let _ = db;
        unimplemented!()
    }

    /// Detects text update, generates embedding, writes direct to Grafeo.
    ///
    /// Stub — Task 4 owns the body (calls `generate_local_embedding`, writes
    /// the resulting `Vec<f32>` to Grafeo's vector index, never to Loro).
    pub async fn handle_text_update(&self, node_id: NodeId, text: &str) -> Result<()> {
        let _ = (node_id, text);
        unimplemented!()
    }
}

/// Module-level once-guard for the ONNX stub warning (DEVIL NIT 1 + Q2:
/// module-top placement is marginally preferred — grep-findable).
static ONNX_WARN_ONCE: Once = Once::new();

/// Hand-rolled SplitMix64 PRNG (DEVIL Q5 — no `rand` dep). Reference:
/// <https://prng.di.unimi.it/splitmix64.c>. Algorithm: increment state by the
/// golden-ratio constant, then mix via xor-shift-multiply. ~10 LOC. Zero-seed
/// safe (the `wrapping_add(0x9E3779B97F4A7C15)` on the first `next_u64` call
/// produces a non-zero state, so the empty-input `""` case folds to seed `0u64`
/// and the first sample is deterministic — anti-happy-path).
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
}

/// Map a `u64` to `f32` in `[0.0, 1.0)` using the top 24 bits (float32 has a
/// 24-bit mantissa — anti-plenger #10 fewest LOC, no precision loss vs the
/// 53-bit `f64` formula `(x >> 11) as f32 / (1u64 << 53) as f32`).
fn u64_to_f01(x: u64) -> f32 {
    ((x >> 40) as f32) / ((1u64 << 24) as f32)
}

/// Deterministic dummy embedding generator (ONNX stub). Returns a
/// `DEFAULT_EMBEDDING_DIM`-dimensional vector derived deterministically from
/// `text` (same input → byte-identical output; empty `""` → valid vector).
/// Logs `tracing::warn!("ONNX not integrated; returning deterministic dummy
/// embedding")` once per process via `std::sync::Once`. Real ONNX lands via
/// `grafeo_engine::embedding::OnnxEmbeddingModel` (Phase 6).
///
/// # Errors
///
/// Stub never returns `Err`; real ONNX can fail (tokenize/infer/model-load),
/// routed via existing `GrafeoLoroError::Config`/`Bridge` variants (no new
/// variant — anti-plenger #5 Bloat).
pub fn generate_local_embedding(text: &str) -> Result<Vec<f32>> {
    ONNX_WARN_ONCE.call_once(|| {
        warn!("ONNX not integrated; returning deterministic dummy embedding");
    });

    // Fold text bytes into a u64 seed (deterministic, input-sensitive).
    let seed: u64 = text
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));

    // Hand-rolled SplitMix64 PRNG (no `rand` dep — DEVIL Q5). Reference:
    // https://prng.di.unimi.it/splitmix64.c. grafeo-engine's `MockEmbeddingModel`
    // at `grafeo-engine-0.5.42/src/embedding/mod.rs:62-93` uses a similar
    // fold-seed-derive pattern (`#[cfg(test)]`-private, algorithm reference only).
    let mut rng = SplitMix64::new(seed);
    let mut vec = Vec::with_capacity(DEFAULT_EMBEDDING_DIM);
    for _ in 0..DEFAULT_EMBEDDING_DIM {
        vec.push(u64_to_f01(rng.next_u64()));
    }
    Ok(vec)
}
