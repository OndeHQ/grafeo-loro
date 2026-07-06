//! Phase 3 Task 1: compression envelope + `LoroDoc` extension trait.
//!
//! L2 wiring — bodies are `todo!("L3: ...")`; L3 fills in codec calls.
//! Codec API citations are inline `// verified at <path:line>` on each fn.
//! `zstd` binds to C zstd (no pure-Rust encoder exists in the ecosystem); `lz4_flex` is pure-Rust.

use loro::{LoroDoc, ExportMode};

use crate::config::CompressionType;
use crate::error::Result;

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
        // Wiring: dispatch on `strategy`; each arm produces a `raw_data: Vec<u8>`,
        // then the L3 body returns `Ok(Self { compression: strategy, raw_data })`.
        let _ = raw_bytes;  // L3 uses raw_bytes in each codec arm.
        match strategy {
            CompressionType::None => {
                // None: passthrough — pure clone, no header, no size prefix.
                // Idempotent: compress(decompress(x)) == x (DEVIL Q8 approved).
                // TODO(L3): raw_bytes.to_vec()
                todo!("L3: none passthrough")
            }
            CompressionType::Lz4 => {
                // LZ4: `lz4_flex::compress_prepend_size` — INFALLIBLE, returns `Vec<u8>`.
                // verified at lz4_flex-0.11.6/src/block/compress.rs:713.
                // TODO(L3): lz4_flex::compress_prepend_size(raw_bytes)
                todo!("L3: lz4 compress_prepend_size")
            }
            CompressionType::Zstd => {
                // Zstd: `zstd::stream::encode_all` — level `DEFAULT_ZSTD_LEVEL` (= 3).
                // verified at zstd-0.13.3/src/stream/functions.rs:32.
                // `io::Error` routed via `Compression(e.to_string())` (DEVIL M3 — symmetric with LZ4, NOT `StorageIo`).
                // TODO(L3): zstd::stream::encode_all(raw_bytes, crate::constants::DEFAULT_ZSTD_LEVEL).map_err(|e| crate::error::GrafeoLoroError::Compression(e.to_string()))?
                todo!("L3: zstd encode_all level 3")
            }
        }
    }

    /// Decompress `raw_data` back to the original Loro bytes.
    pub fn decompress(&self) -> Result<Vec<u8>> {
        // Wiring: dispatch on `self.compression`; each arm produces `Vec<u8>`.
        match self.compression {
            CompressionType::None => {
                // None: passthrough — pure clone.
                // TODO(L3): Ok(self.raw_data.clone())
                todo!("L3: none passthrough")
            }
            CompressionType::Lz4 => {
                // LZ4: `lz4_flex::decompress_size_prepended` — returns `Result<Vec<u8>, DecompressError>`.
                // verified at lz4_flex-0.11.6/src/block/decompress.rs:496.
                // `DecompressError` routed via `Compression(e.to_string())` (DEVIL Q1 approved).
                // TODO(L3): lz4_flex::decompress_size_prepended(&self.raw_data).map_err(|e| crate::error::GrafeoLoroError::Compression(e.to_string()))
                todo!("L3: lz4 decompress_size_prepended")
            }
            CompressionType::Zstd => {
                // Zstd: `zstd::stream::decode_all` — returns `Result<Vec<u8>, io::Error>`.
                // verified at zstd-0.13.3/src/stream/functions.rs:8.
                // `io::Error` routed via `Compression(e.to_string())` (DEVIL M3 — symmetric with LZ4, NOT `StorageIo`).
                // TODO(L3): zstd::stream::decode_all(&self.raw_data[..]).map_err(|e| crate::error::GrafeoLoroError::Compression(e.to_string()))
                todo!("L3: zstd decode_all")
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
        // Wiring: export `LoroDoc` → bytes → compress.
        // `LoroDoc::export(&self, mode)` returns `Result<Vec<u8>, LoroEncodeError>` (verified at loro-1.13.6/src/lib.rs:1306).
        // `LoroEncodeError` → `LoroError` via `From` (loro-common-1.13.1/src/error.rs:204) → `GrafeoLoroError::Loro` via `#[from]` (src/error.rs:6).
        // Two-hop chain — L3 must `.map_err(|e| GrafeoLoroError::Loro(e.into()))` (single `?` won't auto-chain two `From`s).
        // TODO(L3): let bytes = self.export(mode).map_err(|e| crate::error::GrafeoLoroError::Loro(e.into()))?;
        // TODO(L3): CompressedPayload::compress(&bytes, strategy)
        let _ = (mode, strategy);
        todo!("L3: export then compress")
    }

    fn import_compressed(&self, payload: &CompressedPayload) -> Result<loro::ImportStatus> {
        // Wiring: decompress payload → bytes → import into `LoroDoc`.
        // `LoroDoc::import(&self, &[u8])` returns `Result<ImportStatus, LoroError>` (verified at loro-1.13.6/src/lib.rs:710).
        // `ImportStatus` SURFACED (DEVIL M2): Loro's own `import` doc (loro-1.13.6/src/lib.rs:705-708) warns about
        // pending dependencies — Phase 4 `hydrate()` cold-boot needs this to detect partial imports and fetch missing ranges.
        // No origin tag: compression module is origin-agnostic (DEVIL Q4 approved); Phase 4 wraps with `import_with` if needed.
        // TODO(L3): let bytes = payload.decompress()?;
        // TODO(L3): Ok(self.import(&bytes)?)  // LoroError -> GrafeoLoroError::Loro via #[from]; ImportStatus returned
        let _ = payload;
        todo!("L3: decompress then import")
    }
}
