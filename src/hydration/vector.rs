use std::sync::Arc;
use std::sync::Once;
use grafeo::GrafeoDB;
use tracing::warn;
use crate::types::ids::NodeId;
use crate::error::Result;
use crate::constants::{DEFAULT_EMBEDDING_DIM, EMBEDDING_PROPERTY, ORIGIN_LORO_BRIDGE};

/// Manages offloaded float-vector embeddings. Vectors are never written to
/// Loro; they go direct to Grafeo's HNSW index.
pub struct VectorOffloadManager {
    db: Arc<GrafeoDB>,
}

impl VectorOffloadManager {
    /// Construct with a shared Grafeo handle.
    ///
    /// L1 decision 1: trivial `Self { db }`. ONNX model preload is Phase 6
    /// (stub stays on the deterministic SplitMix64 dummy). Vector index
    /// creation is a schema concern (`GrafeoDB::create_vector_index` at
    /// `database/index.rs:104`), not a manager concern — callers/index-owner
    /// create the HNSW index BEFORE calling `handle_text_update`.
    pub fn new(db: Arc<GrafeoDB>) -> Self {
        Self { db }
    }

    /// Generate embedding for `text` via `generate_local_embedding` (Task 3),
    /// then write it direct to Grafeo as `Value::Vector(Arc<[f32]>)` on the
    /// node's `EMBEDDING_PROPERTY` slot — bypassing Loro entirely.
    ///
    /// # Loro bypass invariant (spec validation gate)
    ///
    /// This function NEVER calls `LoroDoc::*`, `LoroMap::*`, `RootReconciler`,
    /// or any other Loro write API. The embedding lives ONLY in Grafeo. The
    /// test `vector_offload_never_writes_to_loro` asserts this by inspecting
    /// `LoroDoc::get_deep_value()` post-call and verifying no `LoroValue::List`
    /// of `DEFAULT_EMBEDDING_DIM` floats is present anywhere.
    ///
    /// # Transaction lifecycle (L1 decision 7 — matches `parallel_hydrate_grafeo` + `VertexBuilder::commit`)
    ///
    /// 1. `db.session_with_cdc(false)` — CDC off suppresses outbound echoes
    ///    (matches `parallel_hydrate_grafeo:57` + `app.rs:443`).
    /// 2. `session.begin_transaction()` — SI isolation (single property write,
    ///    no read-then-write race — Serializable not needed).
    /// 3. `session.set_node_property(node_id, EMBEDDING_PROPERTY, Value::Vector(arc))`
    ///    — verified at `grafeo-engine-0.5.42/src/session/mod.rs:5012` (takes
    ///    `&self`); the underlying `crud.rs:359` extracts vector data and
    ///    auto-inserts into any matching HNSW index (created by the caller).
    /// 4. `session.prepare_commit()?.set_metadata(ORIGIN_LORO_BRIDGE, ORIGIN_LORO_BRIDGE).commit()?`
    ///    — origin tag is advisory-only per Devil Gap 1 (dropped on `commit()`
    ///    per `transaction/prepared.rs:124-128`); retained for advisory
    ///    logging consistency with `parallel_hydrate_grafeo:104-106`.
    ///
    /// # `Vec<f32>` → `Arc<[f32]>` conversion (L1 decision 5)
    ///
    /// `Arc::<[f32]>::from(vec)` where `vec: Vec<f32>` — `From<Vec<T>> for
    /// Arc<[T]>` is in `std::sync` (stable since 1.54). L3 may also use
    /// `vec.into()` (inferred) — equivalent.
    ///
    /// # Errors
    ///
    /// - `GrafeoLoroError::Grafeo` if `begin_transaction` / `set_node_property`
    ///   / `prepare_commit` / `commit` fails (existing `#[from] grafeo::Error`
    ///   impl handles all — no new variant per anti-plenger #5 Bloat).
    /// - `generate_local_embedding` error (stub returns `Ok`; real ONNX may
    ///   fail via `Config`/`Bridge` per Task 3 contract) propagates via `?`.
    ///
    /// # Origin tag (L1 decision 3 — `ORIGIN_LORO_BRIDGE`)
    ///
    /// The vector write IS conceptually a Loro-side update routed directly to
    /// Grafeo (bypassing Loro's CRDT). Using `ORIGIN_LORO_BRIDGE` keeps the
    /// origin vocabulary at 2 tags (`grafeo-bridge` / `loro-bridge`) — no new
    /// `ORIGIN_VECTOR_OFFLOAD` constant, no `bridge::origin` change (anti-
    /// plenger #11 deletion over addition). The outbound worker filters via
    /// the epoch side-channel (Devil Gap 1), not origin strings, so the tag is
    /// advisory — but consistency with `parallel_hydrate_grafeo` matters for
    /// log-readability.
    pub async fn handle_text_update(&self, node_id: NodeId, text: &str) -> Result<()> {
        let _ = (node_id, text, EMBEDDING_PROPERTY, ORIGIN_LORO_BRIDGE, self.db.clone());
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
