//! Phase 3 Task 1: compression envelope + `LoroDoc` extension trait.
//!
//! L3 deep implementation — codec calls filled; verified API citations inline.
//! `zstd` binds to C zstd (no pure-Rust encoder exists in the ecosystem); `lz4_flex` is pure-Rust.

use loro::{LoroDoc, ExportMode};

use crate::config::CompressionType;
use crate::error::{GrafeoLoroError, Result};

/// Compressed payload envelope: codec tag + compressed bytes. In-memory only — Phase 4 `StorageBackend` adds the wire format (DEVIL M4).
// Debug: logging; Clone: caller reuse; PartialEq+Eq: roundtrip test assertions (DEVIL n1).
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
                // Zstd: `zstd::stream::encode_all` — level `DEFAULT_ZSTD_LEVEL` (= 3).
                // verified at zstd-0.13.3/src/stream/functions.rs:32.
                // `io::Error` routed via `Compression(e.to_string())` (DEVIL M3 — symmetric with LZ4, NOT `StorageIo`).
                zstd::stream::encode_all(raw_bytes, crate::constants::DEFAULT_ZSTD_LEVEL)
                    .map_err(|e| GrafeoLoroError::Compression(e.to_string()))?
            }
        };
        Ok(Self { compression: strategy, raw_data })
    }

    /// Decompress `raw_data` back to the original Loro bytes.
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
                // Zstd: `zstd::stream::decode_all` — returns `Result<Vec<u8>, io::Error>`.
                // verified at zstd-0.13.3/src/stream/functions.rs:8.
                // `io::Error` routed via `Compression(e.to_string())` (DEVIL M3 — symmetric with LZ4, NOT `StorageIo`).
                zstd::stream::decode_all(&self.raw_data[..])
                    .map_err(|e| GrafeoLoroError::Compression(e.to_string()))
            }
        }
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

    /// Decompress `payload`, then import into this doc. Returns `ImportStatus` so callers can detect pending dependencies (DEVIL M2).
    fn import_compressed(&self, payload: &CompressedPayload) -> Result<loro::ImportStatus>;
}

impl LoroDocCompressionExt for LoroDoc {
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
