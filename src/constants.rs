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

// ============================================================================
// Phase 4 — Storage key SSOT (P4-L1)
// ============================================================================
//
// Architecture §2 + §4 Step D + §24.3 (`StorageBackend::load/save/list/delete`
// take opaque `&str` keys; the application owns the key-space convention).
// These constants are the SSOT for the suffix portion of every storage key
// produced/consumed by `app::GrafeoLoroApp::hydrate` / `checkpoint`
// (P4T2-L2 / P4T3-L2). Call sites compose with a caller-supplied `graph_id`
// via `format!("{graph_id}/{STORAGE_KEY_*}")` — see each method's doc-comment
// for the exact composition pattern. Anti-plenger #2 DRY/SSOT: no call site
// re-literalizes `"base.loro"`, `"snapshot.tar.zst"`, etc.

/// Suffix of the Loro-SSOT mode base snapshot storage key. Full key:
/// `format!("{graph_id}/{STORAGE_KEY_BASE_LORO}")` — e.g. `graph_123/base.loro`.
///
/// Contents: bytes from `LoroDoc::export(ExportMode::Snapshot)` (full state +
/// history) on `checkpoint`, optionally wrapped via
/// `CompressedPayload::compress(&bytes, CompressionType::Zstd)` (P3T1-L3 codec
/// envelope). On `hydrate`, decompressed + `LoroDoc::import_with(&bytes,
/// ORIGIN_LORO_BRIDGE)` (verified at `loro-1.13.6/src/lib.rs:721` — P4-DEVIL
/// n1 + M10: `import_with` tags the import for the B1 echo filter at
/// `src/bridge/sync_engine.rs:234`, which skips events whose origin matches
/// `ORIGIN_LORO_BRIDGE`. The untagged `LoroDoc::import` at `:710` is NOT
/// used — it would re-fire the subscriber on cold-boot import).
///
/// `// TODO(P4-L3)`: P4-DEVIL m2 wire format — 1-byte codec tag (0=None, 1=Lz4,
/// 2=Zstd — matches `CompressionType` discriminant order) + N bytes payload.
/// L3 adds `compress_to_wire` / `decompress_from_wire` helpers in
/// `src/compression/wrapper.rs`; Phase 4 L2 writes `payload.raw_data` directly
/// assuming codec matches `self.compression` (single-codec deployment).
///
/// `StorageBackend::load` returning `io::ErrorKind::NotFound` on this key is
/// the "fresh graph" cold-boot path — `hydrate` initializes an empty `LoroDoc`
/// and proceeds (P4T2-L2 contract).
pub const STORAGE_KEY_BASE_LORO: &str = "base.loro";

/// Prefix of the Loro-SSOT mode delta storage keys. Full key:
/// `format!("{graph_id}/{STORAGE_KEY_DELTA_PREFIX}{epoch}{STORAGE_KEY_DELTA_SUFFIX}")`
/// — e.g. `graph_123/delta-42.loro`.
///
/// Used as the `prefix` arg to `StorageBackend::list` for delta enumeration
/// during `hydrate` (architecture §24.3 — "List keys matching a prefix (for
/// delta enumeration in Loro SSOT mode)"). Each delta's body is
/// `LoroDoc::export(ExportMode::updates(&from_vv))` (verified at
/// `loro-internal-1.13.6/src/encoding.rs:80`), optionally compressed via
/// `CompressedPayload::compress`.
///
/// The `{epoch}` slot is reserved for the grafeo-side `EpochId`
/// (`GrafeoDB::current_epoch().as_u64()` — verified PUBLIC at
/// `grafeo-engine-0.5.42/src/database/crud.rs:258`). `P4-DEVIL Q1` RESOLVED:
/// deferred — Phase 4 has NO delta-write path (P4-DEVIL M1); the slot is
/// unused in Phase 4 (`hydrate`'s delta-listing returns `Ok(vec![])` and the
/// import loop runs zero times). Phase 5+ Loro sync wire will populate it.
pub const STORAGE_KEY_DELTA_PREFIX: &str = "delta-";

/// Suffix of the Loro-SSOT mode delta storage keys — see
/// [`STORAGE_KEY_DELTA_PREFIX`].
pub const STORAGE_KEY_DELTA_SUFFIX: &str = ".loro";

/// Suffix of the Grafeo-SSOT mode tarball snapshot storage key. Full key:
/// `format!("{graph_id}/{STORAGE_KEY_GRAFEO_TAR_ZST}")` — e.g.
/// `graph_123/snapshot.tar.zst`.
///
/// Contents: tarball of the directory-backed `GrafeoDB` folder (post-flush),
/// zstd-compressed via `CompressedPayload::compress(&tar_bytes,
/// CompressionType::Zstd)`. On `hydrate`, `zstd::stream::decode_all`
/// (verified at `zstd-0.13.3/src/stream/functions.rs:8`) + tar extraction +
/// `GrafeoDB::with_config(Config::persistent(extracted_dir))` (verified at
/// `grafeo-engine-0.5.42/src/database/mod.rs:346` — NOT `GrafeoDB::open`
/// which is `#[cfg(feature = "wal")]`-gated at `:289`; P4-DEVIL B1).
///
/// `StorageBackend::load` returning `io::ErrorKind::NotFound` on this key is
/// the "fresh graph" cold-boot path — `hydrate` initializes an empty
/// `GrafeoDB` (P4T2-L2 contract).
///
/// `P4-DEVIL Q2/B1/B2` RESOLVED (option (d)): `SsotMode::Grafeo` is deferred
/// to Phase 5. The `tar` crate is NOT in `Cargo.toml` (P4-DEVIL M5 — Phase 5
/// adds `tar = "0.4"`). `GrafeoDB::backup_full` / `restore_to_epoch` (at
/// `grafeo-engine-0.5.42/src/database/mod.rs:2743` / `:2813`) are gated
/// behind the `wal` feature which is NOT in grafeo-0.5.42's default
/// `embedded` feature set. P5 plan: enable `wal` + add `tar` + refactor
/// `SyncEngine.grafeo_db` to `ArcSwap<GrafeoDB>` (B2) + use non-destructive
/// `backup_full(&backup_dir)` (P4-DEVIL M3).
pub const STORAGE_KEY_GRAFEO_TAR_ZST: &str = "snapshot.tar.zst";