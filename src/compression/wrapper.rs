//! Phase 3 Task 1 contracts: compression envelope + `LoroDoc` extension trait.
//!
//! L1 scaffold only — bodies are `unimplemented!()`. See `docs/implementation-plan.md`
//! Phase 3 Task 1 for the implementation contract.
//!
//! # Verified crate APIs (P3T1-L1)
//!
//! - `lz4_flex::compress_prepend_size(input: &[u8]) -> Vec<u8>` — infallible,
//!   `lz4_flex-0.11.6/src/block/compress.rs:713`.
//! - `lz4_flex::decompress_size_prepended(input: &[u8]) -> Result<Vec<u8>, DecompressError>`
//!   — `lz4_flex-0.11.6/src/block/decompress.rs:496`. `DecompressError: std::error::Error`
//!   — `lz4_flex-0.11.6/src/block/mod.rs:82-143` (non_exhaustive enum).
//! - `zstd::stream::write::Encoder::new(writer: W, level: i32) -> io::Result<Self>`
//!   — `zstd-0.13.3/src/stream/write/mod.rs:174` (W: Write, 'static lifetime).
//! - `zstd::stream::write::Encoder::finish(self) -> io::Result<W>` — `:287`.
//! - `zstd::stream::write::Decoder::new(writer: W) -> io::Result<Self>` — `:337`.
//! - `zstd::stream::write::Decoder` + `Encoder` impl `std::io::Write`.
//! - `zstd::stream::encode_all(src: impl Read, level: i32) -> io::Result<Vec<u8>>`
//!   — `zstd-0.13.3/src/stream/functions.rs:32` (convenience wrapper around Encoder).
//! - `zstd::stream::decode_all(src: impl Read) -> io::Result<Vec<u8>>` — `:8`.
//! - `LoroDoc::export(&self, mode: ExportMode) -> Result<Vec<u8>, LoroEncodeError>`
//!   — `loro-1.13.6/src/lib.rs:1306`. NOTE: error type is `LoroEncodeError`, NOT
//!   `LoroError`. `LoroEncodeError` is defined at `loro-common-1.13.1/src/error.rs:140`
//!   and `impl From<LoroEncodeError> for LoroError` at `:204` lets L3 chain
//!   `LoroEncodeError -> LoroError -> GrafeoLoroError::Loro` without a new variant
//!   (anti-plenger #5 Bloat — no new error variant needed).
//! - `LoroDoc::import(&self, bytes: &[u8]) -> Result<ImportStatus, LoroError>` —
//!   `loro-1.13.6/src/lib.rs:710`. NOTE: takes `&self`, NOT `&mut self` (interior
//!   mutability). The original architecture sketch (`docs/grafeo-loro.architecture.md:620`)
//!   used `&mut self`; corrected here (anti-plenger #1 backward-compat-slave).
//!
//! # Zstd level
//!
//! The Zstd level is sourced from `crate::constants::DEFAULT_ZSTD_LEVEL` (= 3,
//! zstd's own default per `zstd-0.13.3/src/lib.rs:36`). SSOT for Phase 4 storage.

use loro::{LoroDoc, ExportMode};

use crate::config::CompressionType;
use crate::error::Result;

/// Compressed payload envelope: codec tag + compressed bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedPayload {
    /// Codec used to produce `raw_data`.
    pub compression: CompressionType,
    /// Compressed bytes (or passthrough bytes when `compression == None`).
    pub raw_data: Vec<u8>,
}

impl CompressedPayload {
    /// Compress `raw_bytes` using `strategy`; fails on Zstd I/O errors.
    pub fn compress(raw_bytes: &[u8], strategy: CompressionType) -> Result<Self> {
        let _ = (raw_bytes, strategy);
        unimplemented!()
    }

    /// Decompress `raw_data` back to the original Loro bytes.
    pub fn decompress(&self) -> Result<Vec<u8>> {
        let _ = self;
        unimplemented!()
    }
}

/// Extension trait binding compression onto `LoroDoc` export/import.
pub trait LoroDocCompressionExt {
    /// Export the doc with `mode`, then compress under `strategy`.
    fn export_compressed(
        &self,
        mode: ExportMode,
        strategy: CompressionType,
    ) -> Result<CompressedPayload>;

    /// Decompress `payload`, then import into this doc.
    fn import_compressed(&self, payload: &CompressedPayload) -> Result<()>;
}

impl LoroDocCompressionExt for LoroDoc {
    fn export_compressed(
        &self,
        mode: ExportMode,
        strategy: CompressionType,
    ) -> Result<CompressedPayload> {
        let _ = (mode, strategy);
        unimplemented!()
    }

    fn import_compressed(&self, payload: &CompressedPayload) -> Result<()> {
        let _ = payload;
        unimplemented!()
    }
}
