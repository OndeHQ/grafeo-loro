// Origin tags prevent echo feedback loops
pub const ORIGIN_GRAFEO_BRIDGE: &str = "grafeo-bridge";
pub const ORIGIN_LORO_BRIDGE: &str = "loro-bridge";

// Root LoroDoc container keys
pub const ROOT_VERTICES: &str = "V";
pub const ROOT_EDGES: &str = "E";
// Phase 2: tree container support — `ROOT_TREE: &str = "T_CHILD"` was deleted
// as YAGNI per Hunter NIT 11 (declared but never read in Phase 1). Re-add when
// the inbound subscriber grows a tree-container diff arm.

// Tree parent-child edge label. Direction is parent→child (src=parent,
// dst=child) per architecture §7 line 265 (`(p)-[:CHILD]->(c)`) and
// `sync_tree_move_to_grafeo`'s doc-comment (P2T2-DEVIL R1). This constant is
// the SSOT for the label literal; direction is enforced at call sites.
pub const TREE_EDGE_LABEL: &str = "CHILD";

// Ephemeral presence magic bytes
pub const EPH_MAGIC: &[u8; 4] = b"%EPH";

// Defaults
pub const DEFAULT_BATCH_MS: u64 = 100;
pub const DEFAULT_BATCH_SIZE: usize = 256;
pub const DEFAULT_CHUNK_SIZE: usize = 256;
pub const DEFAULT_STALENESS_MS: u64 = 5000;

/// Zstd compression level used by `compression::wrapper::CompressedPayload::compress`
/// for `CompressionType::Zstd` (Phase 3 Task 1). Level 3 is zstd's own default
/// (`zstd-0.13.3/src/lib.rs:36` re-exports `zstd_safe::CLEVEL_DEFAULT = 3`); named
/// here as SSOT so Phase 4 storage can reference the same constant without
/// reaching into the compression module (anti-plenger #2 DRY/SSOT).
pub const DEFAULT_ZSTD_LEVEL: i32 = 3;

/// Embedding dimension used by `hydration::vector::generate_local_embedding`
/// (Phase 3 Task 3) and required by Grafeo's HNSW vector index at index-creation
/// time. Value `384` matches `sentence-transformers/all-MiniLM-L6-v2` — the
/// preset `EmbeddingModelConfig::MiniLmL6v2` in `grafeo-engine-0.5.42`
/// (`src/embedding/config.rs:18`: `expected_dimensions: 384`). Named here as
/// SSOT so the stub, the real ONNX wiring (future loop), and the Grafeo HNSW
/// index-creation call site all agree on a single dimension source; the value
/// can be swapped in one place if Task 4 / Phase 5 moves to `MiniLmL12-v2`
/// (also 384) or `bge-small-en-v1.5` (also 384) — all-MiniLM/bge-small presets
/// share 384, so the SSOT is forward-compatible for the foreseeable roadmap.
pub const DEFAULT_EMBEDDING_DIM: usize = 384;

/// Grafeo node property key under which `VectorOffloadManager::handle_text_update`
/// (Phase 3 Task 4) stores the `Value::Vector(Arc<[f32]>)` produced by
/// `generate_local_embedding`. Named here as SSOT so the manager, the test
/// scaffolds, and any future `vector_search` call site (Phase 5+) agree on a
/// single key literal — anti-plenger #2 DRY/SSOT. Value `"embedding"` matches
/// grafeo-engine's own example docstring at
/// `grafeo-engine-0.5.42/src/database/index.rs:91` ("property containing vector
/// embeddings (e.g., `\"embedding\"`)"), so it is also the convention grafeo
/// tooling expects when auto-creating HNSW indexes.
pub const EMBEDDING_PROPERTY: &str = "embedding";

// Outbound CDC poller cadence (Grafeo→Loro path). Grafeo 0.5.42 CDC is
// poll-based: the outbound worker calls `session.changes_between(start, end)`
// on this interval. 50 ms ≈ 20 polls/sec — low latency without burning CPU.
pub const OUTBOUND_POLL_MS: u64 = 50;

// Epoch side-channel retention window. The `bridge_origin_epochs` set keeps
// epochs produced by Loro→Grafeo bridge writes so the outbound poller can
// filter them out (echo prevention). Pruning keeps only epochs newer than
// `last_polled_epoch - EPOCH_RETENTION` so the set does not grow unbounded.
pub const EPOCH_RETENTION: u64 = 10_000;