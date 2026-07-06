//! Phase 3 Task 1: compression envelope + `LoroDoc` extension trait.
//!
//! L3 deep implementation — codec calls filled; verified API citations inline.
//! `zstd` binds to C zstd (no pure-Rust encoder exists in the ecosystem); `lz4_flex` is pure-Rust.

use loro::{ExportMode, LoroDoc};

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
        Ok(Self {
            compression: strategy,
            raw_data,
        })
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

    /// Serialize this payload to the on-wire format (P4-DEVIL m2 — L3 scope):
    /// `[version:u8][codec_tag:u8][raw_data..]`.
    ///
    /// `codec_tag` matches `CompressionType` discriminant order
    /// (`None=0x00`, `Lz4=0x01`, `Zstd=0x02` — see [`compression_type_to_tag`]).
    /// Pre-allocates exactly `2 + raw_data.len()` so no reallocation occurs.
    /// Symmetric with [`Self::from_wire`].
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
    pub fn compress_to_wire(raw_bytes: &[u8], strategy: CompressionType) -> Result<Vec<u8>> {
        let payload = Self::compress(raw_bytes, strategy)?;
        Ok(payload.to_wire())
    }

    /// Convenience: parse wire format + decompress in one call. Used by
    /// `hydrate` (architecture §4 Step A — restore cold-boot state from the
    /// storage backend).
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
            "wire format: unknown codec tag {tag:#04x} (expected 0x00=None, 0x01=Lz4, 0x02=Zstd)"
        ))),
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
