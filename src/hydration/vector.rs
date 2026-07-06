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
/// module-top placement is marginally preferred — grep-findable; L3 may move
/// into the function body if preferred — both compile identically).
static ONNX_WARN_ONCE: Once = Once::new();

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
    // TODO(L3): deterministic dummy algorithm (DEVIL Q5 + L1 decision 3):
    //   1. Fold text bytes into a u64 seed:
    //      `text.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64))`
    //   2. Seed a hand-rolled SplitMix64 PRNG (~10 LOC, no `rand` dep).
    //      Reference: https://prng.di.unimi.it/splitmix64.c
    //      grafeo-engine has a `MockEmbeddingModel` at
    //      `grafeo-engine-0.5.42/src/embedding/mod.rs:62-93` with a similar
    //      fold-seed-derive pattern (`#[cfg(test)]`-private, algorithm
    //      reference only — NOT reusable from grafeo-loro).
    //   3. Emit `DEFAULT_EMBEDDING_DIM` f32 samples in `[0.0, 1.0)` via
    //      `(next_u64() >> 11) as f32 / (1u64 << 53) as f32`.
    //   4. Return `Ok(vec![...])`.
    //   Verify: same `text` → byte-identical `Vec<f32>`; empty `""` → valid
    //   `DEFAULT_EMBEDDING_DIM`-length vector (PRNG zero-seed must not panic).
    let _ = (text, DEFAULT_EMBEDDING_DIM);
    todo!("L3: deterministic dummy embedding via fold-seed-SplitMix64")
}
