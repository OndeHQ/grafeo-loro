//! Phase 3 Task 1: compression envelope + `LoroDoc` extension trait.
//!
//! L3 deep implementation — codec calls filled; verified API citations inline.
//!
//! ## Issue #3 sub-issue 1 — pure-Rust codec swap
//!
//! Previously this module bound to `zstd-sys` (a C library) which:
//! 1. Broke `wasm32-unknown-unknown` builds (requires clang cross-compiler).
//! 2. Pulled a non-Rust dep into a crate that advertises WASM compatibility.
//!
//! The fix: `zstd` is replaced with `brotli` (pure-Rust) and `flate2` (with
//! `rust_backend` — also pure-Rust). Both compile cleanly on
//! `wasm32-unknown-unknown` without any C toolchain.
//!
//! **Variant-name caveat:** `CompressionType::Zstd` (in `src/config.rs`) is
//! kept for now because `config.rs` is orchestrator-owned. Internally it routes
//! to Brotli. The wire-format tag `0x02` is preserved for forward-compat with
//! existing on-disk snapshots.
//!
//! ```text
//! // TODO(orchestrator): rename Zstd variant → Brotli in src/config.rs.
//! //                   Once renamed, update the match arms + tag mapping here.
//! ```
//!
//! ## Codec helper API
//!
//! In addition to the [`CompressedPayload`] envelope (which dispatches via
//! [`CompressionType`]), this module exposes standalone pure-Rust codec helpers
//! — [`brotli_compress`], [`brotli_decompress`], [`deflate_compress`],
//! [`deflate_decompress`] — so callers (and tests) can exercise each codec
//! directly without going through the enum.

use std::io::{Read, Write};

use loro::{ExportMode, LoroDoc};
use tracing::instrument;

use crate::config::CompressionType;
use crate::error::{GrafeoLoroError, Result};

/// On-wire format version (P4-DEVIL m2 — `compress_to_wire`/`decompress_from_wire`).
///
/// Reserved for forward-compat with Phase 5+ codecs that may need extra header
/// bytes (e.g. per-codec level metadata, checksum). Bumped only on incompatible
/// layout changes; Phase 4 starts at `1`. `decompress_from_wire` rejects any
/// other version with `Compression(...)`.
const WIRE_FORMAT_VERSION: u8 = 1;

/// Compressed payload envelope: codec tag + compressed bytes. The wire format
/// (`to_wire`/`from_wire` — P4-L3, P4-DEVIL m2) wraps this struct into a
/// `[version:u8][codec_tag:u8][raw_data..]` byte sequence for storage.
// Debug: logging; Clone: caller reuse; PartialEq+Eq: roundtrip test assertions (DEVIL n1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedPayload {
    /// Codec used to produce `raw_data`.
    pub compression: CompressionType,
    /// Compressed bytes (or passthrough bytes when `compression == None`).
    pub raw_data: Vec<u8>,
}

impl CompressedPayload {
    /// Compress `raw_bytes` using `strategy`; fails on codec I/O errors.
    ///
    /// Codec dispatch:
    ///
    /// - `None`   → passthrough (`Vec::from`)
    /// - `Lz4`    → `lz4_flex::compress_prepend_size` (infallible, pure-Rust)
    /// - `Zstd`   → routes to **Brotli** (issue #3 sub-issue 1; variant-name
    ///              kept pending orchestrator rename — see module-level TODO).
    #[instrument(skip(raw_bytes), name = "payload_compress", level = "info")]
    pub fn compress(raw_bytes: &[u8], strategy: CompressionType) -> Result<Self> {
        // Dispatch on `strategy`; each arm produces a `raw_data: Vec<u8>`.
        let raw_data = match strategy {
            CompressionType::None => {
                // None: passthrough — pure clone, no header, no size prefix.
                // Idempotent: compress(decompress(x)) == x (DEVIL Q8 approved).
                raw_bytes.to_vec()
            }
            CompressionType::Lz4 => {
                // LZ4: `lz4_flex::compress_prepend_size` — INFALLIBLE, returns `Vec<u8>`.
                // verified at lz4_flex-0.11.6/src/block/compress.rs:713.
                lz4_flex::compress_prepend_size(raw_bytes)
            }
            CompressionType::Zstd => {
                // Issue #3 sub-issue 1: previously `zstd::stream::encode_all` (C dep,
                // broke WASM). Now routes to pure-Rust Brotli. Variant name kept
                // pending orchestrator rename (see module-level TODO).
                // TODO(orchestrator): rename Zstd variant → Brotli in src/config.rs.
                brotli_compress(raw_bytes, crate::constants::DEFAULT_BROTLI_QUALITY)?
            }
        };
        Ok(Self {
            compression: strategy,
            raw_data,
        })
    }

    /// Decompress `raw_data` back to the original Loro bytes.
    #[instrument(skip(self), name = "payload_decompress", level = "info")]
    pub fn decompress(&self) -> Result<Vec<u8>> {
        // Dispatch on `self.compression`; each arm produces `Result<Vec<u8>, GrafeoLoroError>`.
        match self.compression {
            CompressionType::None => {
                // None: passthrough — pure clone.
                Ok(self.raw_data.clone())
            }
            CompressionType::Lz4 => {
                // LZ4: `lz4_flex::decompress_size_prepended` — returns `Result<Vec<u8>, DecompressError>`.
                // verified at lz4_flex-0.11.6/src/block/decompress.rs:496.
                // `DecompressError` routed via `Compression(e.to_string())` (DEVIL Q1 approved).
                lz4_flex::decompress_size_prepended(&self.raw_data)
                    .map_err(|e| GrafeoLoroError::Compression(e.to_string()))
            }
            CompressionType::Zstd => {
                // Issue #3 sub-issue 1: previously `zstd::stream::decode_all` (C dep).
                // Now routes to pure-Rust Brotli. Variant name kept pending orchestrator rename.
                // TODO(orchestrator): rename Zstd variant → Brotli in src/config.rs.
                brotli_decompress(&self.raw_data)
            }
        }
    }

    /// Serialize this payload to the on-wire format (P4-DEVIL m2 — L3 scope):
    /// `[version:u8][codec_tag:u8][raw_data..]`.
    ///
    /// `codec_tag` matches `CompressionType` discriminant order
    /// (`None=0x00`, `Lz4=0x01`, `Zstd=0x02` — see [`compression_type_to_tag`]).
    /// Pre-allocates exactly `2 + raw_data.len()` so no reallocation occurs.
    /// Symmetric with [`Self::from_wire`].
    #[instrument(skip(self), name = "payload_to_wire", level = "debug")]
    pub fn to_wire(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(2 + self.raw_data.len());
        buf.push(WIRE_FORMAT_VERSION);
        buf.push(compression_type_to_tag(self.compression));
        buf.extend_from_slice(&self.raw_data);
        buf
    }

    /// Parse the on-wire format produced by [`Self::to_wire`].
    ///
    /// Rejects payloads shorter than the 2-byte header, unknown versions, and
    /// unknown codec tags with [`GrafeoLoroError::Compression`]. Symmetric with
    /// [`Self::to_wire`].
    #[instrument(skip(bytes), name = "payload_from_wire", level = "debug")]
    pub fn from_wire(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 2 {
            return Err(GrafeoLoroError::Compression(format!(
                "wire format: too few bytes (got {}; expected ≥2 for version+codec tag)",
                bytes.len()
            )));
        }
        let version = bytes[0];
        if version != WIRE_FORMAT_VERSION {
            return Err(GrafeoLoroError::Compression(format!(
                "wire format: unknown version {version} (expected {WIRE_FORMAT_VERSION})"
            )));
        }
        let compression = tag_to_compression_type(bytes[1])?;
        Ok(Self {
            compression,
            raw_data: bytes[2..].to_vec(),
        })
    }

    /// Convenience: compress raw bytes + serialize to wire format in one call.
    /// Used by `checkpoint` (architecture §4 Step D — persist shallow snapshot
    /// under the builder-configured codec).
    #[instrument(skip(raw_bytes), name = "compress_to_wire", level = "info")]
    pub fn compress_to_wire(raw_bytes: &[u8], strategy: CompressionType) -> Result<Vec<u8>> {
        let payload = Self::compress(raw_bytes, strategy)?;
        Ok(payload.to_wire())
    }

    /// Convenience: parse wire format + decompress in one call. Used by
    /// `hydrate` (architecture §4 Step A — restore cold-boot state from the
    /// storage backend).
    #[instrument(skip(bytes), name = "decompress_from_wire", level = "info")]
    pub fn decompress_from_wire(bytes: &[u8]) -> Result<Vec<u8>> {
        let payload = Self::from_wire(bytes)?;
        payload.decompress()
    }
}

/// Map a [`CompressionType`] to its 1-byte wire-format tag (P4-DEVIL m2).
///
/// Tag values mirror `CompressionType`'s `#[derive(Default)]` discriminant
/// order (`None=0`, `Lz4=1`, `Zstd=2` — `src/config.rs:8-14`). SSOT: any new
/// codec variant MUST be added here AND to [`tag_to_compression_type`].
const fn compression_type_to_tag(c: CompressionType) -> u8 {
    match c {
        CompressionType::None => 0x00,
        CompressionType::Lz4 => 0x01,
        CompressionType::Zstd => 0x02,
    }
}

/// Inverse of [`compression_type_to_tag`]. Rejects unknown tags with
/// [`GrafeoLoroError::Compression`] so a corrupt or future-format payload
/// surfaces as a deterministic error instead of panicking or silently
/// mis-decoding.
fn tag_to_compression_type(tag: u8) -> Result<CompressionType> {
    match tag {
        0x00 => Ok(CompressionType::None),
        0x01 => Ok(CompressionType::Lz4),
        0x02 => Ok(CompressionType::Zstd),
        _ => Err(GrafeoLoroError::Compression(format!(
            "wire format: unknown codec tag {tag:#04x} (expected 0x00=None, 0x01=Lz4, 0x02=Zstd/Brotli)"
        ))),
    }
}

// ============================================================================
// Issue #3 sub-issue 1 — pure-Rust codec helpers (Brotli + Deflate)
// ============================================================================
//
// These standalone helpers let callers (and tests) exercise each pure-Rust
// codec directly without going through `CompressionType` (which is currently
// fixed at three variants by the orchestrator-owned `src/config.rs`). Both
// `brotli` and `flate2` are pulled by the `compression` feature with their
// pure-Rust backends (see `Cargo.toml`).

/// Compress `input` using Brotli (pure-Rust, WASM-safe).
///
/// `quality` is the Brotli quality (0–11). Higher = better ratio + slower.
/// Use [`crate::constants::DEFAULT_BROTLI_QUALITY`] (= 5) for the SSOT default.
/// `lgwin` is fixed at 22 (the Brotli spec max for the standard 16 MiB window).
///
/// Errors are routed through [`GrafeoLoroError::Compression`] for symmetry with
/// the LZ4 codec path.
pub fn brotli_compress(input: &[u8], quality: u32) -> Result<Vec<u8>> {
    // `brotli::CompressorWriter::new(writer, buffer_size, q, lgwin)` — verified
    // at brotli-7.0.0/src/enc/writer.rs:84. `q` is quality (0–11), `lgwin` is
    // the window size (10–24). We pin `lgwin = 22` (spec default max).
    let mut writer = brotli::CompressorWriter::new(Vec::new(), 4096, quality, 22);
    writer
        .write_all(input)
        .map_err(|e| GrafeoLoroError::Compression(format!("brotli compress: {e}")))?;
    // Explicit flush before `into_inner` so any buffered compressed bytes are
    // drained into the inner `Vec<u8>`. `into_inner` returns `W` directly (not
    // `Result`) per brotli-7.0.0/src/enc/writer.rs:105 — the Drop impl would
    // also flush, but errors raised in Drop are silently swallowed.
    writer
        .flush()
        .map_err(|e| GrafeoLoroError::Compression(format!("brotli flush: {e}")))?;
    Ok(writer.into_inner())
}

/// Decompress a Brotli stream produced by [`brotli_compress`].
///
/// Uses `brotli::Decompressor` (re-exported from `brotli_decompressor::reader`
/// at `brotli-7.0.0/src/lib.rs:38`). Errors are routed via
/// [`GrafeoLoroError::Compression`].
pub fn brotli_decompress(input: &[u8]) -> Result<Vec<u8>> {
    let mut reader = brotli::Decompressor::new(input, 4096);
    let mut out = Vec::new();
    reader
        .read_to_end(&mut out)
        .map_err(|e| GrafeoLoroError::Compression(format!("brotli decompress: {e}")))?;
    Ok(out)
}

/// Compress `input` using raw DEFLATE (pure-Rust via `flate2`'s `rust_backend`).
///
/// `level` is 0–9 (0 = no compression, 9 = max). Use
/// [`crate::constants::DEFAULT_DEFLATE_LEVEL`] (= 6) for the SSOT default.
///
/// Output is raw DEFLATE (no zlib or gzip header) so the bytes are interoperable
/// with any RFC 1951 decoder (e.g. JS `pako.inflate`).
pub fn deflate_compress(input: &[u8], level: u32) -> Result<Vec<u8>> {
    // `flate2::Compression::new(level)` clamps to 0–10 internally; we cap at 9
    // to match the DEFLATE spec. `flate2::write::DeflateEncoder` produces raw
    // DEFLATE (no zlib header) per flate2-1.1.9/src/deflate/write.rs.
    let level = level.min(9);
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(level));
    encoder
        .write_all(input)
        .map_err(|e| GrafeoLoroError::Compression(format!("deflate compress: {e}")))?;
    encoder
        .finish()
        .map_err(|e| GrafeoLoroError::Compression(format!("deflate finish: {e}")))
}

/// Decompress a raw DEFLATE stream produced by [`deflate_compress`].
///
/// Uses `flate2::read::DeflateDecoder` (re-exported at flate2-1.1.9/src/lib.rs:156).
pub fn deflate_decompress(input: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::DeflateDecoder::new(input);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| GrafeoLoroError::Compression(format!("deflate decompress: {e}")))?;
    Ok(out)
}

/// Extension trait binding compression onto `LoroDoc` export/import.
pub trait LoroDocCompressionExt {
    /// Export the doc with `mode`, then compress under `strategy`.
    fn export_compressed(
        &self,
        mode: ExportMode,
        strategy: CompressionType,
    ) -> Result<CompressedPayload>;

    /// Decompress `payload`, then import into this doc. Returns `ImportStatus` so callers can detect pending dependencies (DEVIL M2).
    fn import_compressed(&self, payload: &CompressedPayload) -> Result<loro::ImportStatus>;
}

impl LoroDocCompressionExt for LoroDoc {
    #[instrument(skip(self, mode), name = "export_compressed", level = "info")]
    fn export_compressed(
        &self,
        mode: ExportMode,
        strategy: CompressionType,
    ) -> Result<CompressedPayload> {
        // Flow: export `LoroDoc` → bytes → compress.
        // `LoroDoc::export(&self, mode)` returns `Result<Vec<u8>, LoroEncodeError>` (verified at loro-1.13.6/src/lib.rs:1306).
        // `LoroEncodeError` → `LoroError` via `From` (loro-common-1.13.1/src/error.rs:204) → `GrafeoLoroError::Loro` via `#[from]` (src/error.rs:6).
        // Two-hop chain — `.map_err(|e| GrafeoLoroError::Loro(e.into()))` is required (single `?` won't auto-chain two `From`s).
        let bytes = self
            .export(mode)
            .map_err(|e| GrafeoLoroError::Loro(e.into()))?;
        CompressedPayload::compress(&bytes, strategy)
    }

    #[instrument(skip(self, payload), name = "import_compressed", level = "info")]
    fn import_compressed(&self, payload: &CompressedPayload) -> Result<loro::ImportStatus> {
        // Flow: decompress payload → bytes → import into `LoroDoc`.
        // `LoroDoc::import(&self, &[u8])` returns `Result<ImportStatus, LoroError>` (verified at loro-1.13.6/src/lib.rs:710).
        // `ImportStatus` SURFACED (DEVIL M2): Loro's own `import` doc (loro-1.13.6/src/lib.rs:705-708) warns about
        // pending dependencies — Phase 4 `hydrate()` cold-boot needs this to detect partial imports and fetch missing ranges.
        // No origin tag: compression module is origin-agnostic (DEVIL Q4 approved); Phase 4 wraps with `import_with` if needed.
        let bytes = payload.decompress()?;
        Ok(self.import(&bytes)?) // LoroError -> GrafeoLoroError::Loro via #[from]; ImportStatus returned
    }
}
