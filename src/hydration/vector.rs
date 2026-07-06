use std::sync::Arc;
use grafeo::GrafeoDB;
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

/// Local ONNX inference stub. Phase 3 Task 3 owns ONLY the contract — body is
/// `unimplemented!()` until L3. Real ONNX wiring lands via
/// `grafeo_engine::embedding::OnnxEmbeddingModel` (verified at
/// `grafeo-engine-0.5.42/src/embedding/mod.rs:39` `trait EmbeddingModel` with
/// `fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>` and
/// `fn dimensions(&self) -> usize`; preset `MiniLmL6v2` is 384 dims per
/// `embedding/config.rs:18`).
///
/// # Contract (L3 implements)
///
/// - **Deterministic** — same `text` MUST yield byte-identical `Vec<f32>`
///   across calls. L3 algorithm: fold `text.bytes()` into a `u64` seed via
///   `wrapping_mul(31).wrapping_add(b as u64)`, seed a `SplitMix64`/`ChaCha8`
///   PRNG, emit `DEFAULT_EMBEDDING_DIM` `f32` samples in `[0.0, 1.0)`.
/// - **Input-sensitive** — different `text` MUST yield different `Vec<f32>`
///   (collision-resistant within PRNG seed space).
/// - **Dimension** — output length is always `DEFAULT_EMBEDDING_DIM` (384),
///   matching `all-MiniLM-L6-v2` so L3 can swap the dummy for real ONNX
///   without resizing downstream Grafeo HNSW state.
/// - **Warning** — emits `tracing::warn!` with message
///   `"ONNX not integrated; returning deterministic dummy embedding"` exactly
///   ONCE per process via `std::sync::Once` (informational, not actionable —
///   anti-plenger #8 Observability vs #10 fewest-LOC tension resolved in favour
///   of once-guard to avoid log spam under batch embedding loops).
/// - **Sync** — no I/O, no `await`. Real ONNX `EmbeddingModel::embed` is also
///   sync (`grafeo-engine-0.5.42/src/embedding/mod.rs:46`); if Task 4 needs it
///   in an async context, it will `tokio::task::spawn_blocking`.
/// - **Infallible stub** — the dummy never fails, but the signature is
///   `Result<Vec<f32>>` so future real-ONNX wiring (which CAN fail on model
///   load / tokenize / infer) does NOT break the call site (anti-plenger #14
///   never simplify basics; `GrafeoLoroError` reuses existing variants —
///   `Config(String)` for model-not-found, `Bridge(String)` for infer faults).
/// - **Empty input** — `""` MUST still produce a deterministic vector (folds
///   to the seed's zero-state, not a panic).
///
/// # Errors
///
/// Stub never returns `Err`. Real ONNX returns `Err` on tokenize/infer failure
/// (routed via `GrafeoLoroError::Bridge` or `Config` — L3 decides; no new
/// variant added at L1 per anti-plenger #5 Bloat).
pub fn generate_local_embedding(text: &str) -> Result<Vec<f32>> {
    let _ = (text, DEFAULT_EMBEDDING_DIM);
    unimplemented!()
}
