//! Phase 3 Task 1 scaffolds: `compression::wrapper` roundtrips.
//!
//! All 5 tests assert the roundtrip contract under test — `CompressedPayload::compress`
//! followed by `CompressedPayload::decompress` returns the original bytes, AND (for
//! the Zstd-preserves-Loro-importability gate) that the decompressed bytes can be
//! fed back into a fresh `LoroDoc::import` without error. Anti-Goodhart: assert
//! BOTH byte-level equality AND Loro semantic equivalence where applicable.
//!
//! # Test fixture strategy
//!
//! Each test constructs a fresh `LoroDoc`, writes a small payload (text or map
//! mutation), commits, then calls `LoroDocCompressionExt::export_compressed` with
//! a chosen `CompressionType`. Decompression uses `CompressedPayload::decompress`
//! directly. For Loro-importability tests, the decompressed bytes are imported
//! into a SECOND fresh `LoroDoc` and `get_deep_value()` equality is asserted.
//!
//! `ExportMode::Snapshot` is the canonical "export everything" mode (verified
//! absent from `loro-1.13.6/src/lib.rs:1306` signature; the actual `ExportMode`
//! constructors live in `loro-1.13.6/src/encoding.rs` — L3 picks the right one,
//! likely `ExportMode::Snapshot`).
//!
//! # Verified crate APIs (P3T1-L1 — see `src/compression/wrapper.rs` module doc)
//!
//! - `lz4_flex::compress_prepend_size(input: &[u8]) -> Vec<u8>` — infallible
//!   (`lz4_flex-0.11.6/src/block/compress.rs:713`).
//! - `lz4_flex::decompress_size_prepended(input: &[u8]) -> Result<Vec<u8>, DecompressError>`
//!   (`lz4_flex-0.11.6/src/block/decompress.rs:496`).
//! - `zstd::stream::write::Encoder::new(writer: W, level: i32) -> io::Result<Self>`
//!   (`zstd-0.13.3/src/stream/write/mod.rs:174`); `Encoder::finish(self) -> io::Result<W>`
//!   (`:287`); impl `io::Write`.
//! - `zstd::stream::write::Decoder::new(writer: W) -> io::Result<Self>` (`:337`);
//!   impl `io::Write`.
//! - `LoroDoc::export(&self, mode: ExportMode) -> Result<Vec<u8>, LoroEncodeError>`
//!   (`loro-1.13.6/src/lib.rs:1306`).
//! - `LoroDoc::import(&self, bytes: &[u8]) -> Result<ImportStatus, LoroError>` —
//!   takes `&self`, NOT `&mut self` (`loro-1.13.6/src/lib.rs:710`).
//! - `LoroDoc::get_deep_value(&self) -> LoroValue` — semantic equality check.
//! - `LoroDoc::get_text<I: IntoContainerId>(&self, I) -> LoroText` — text mutation
//!   for the roundtrip fixture.
//! - `crate::constants::DEFAULT_ZSTD_LEVEL = 3` — zstd level SSOT.
//!
//! # Edge cases covered
//!
//! - Empty input: `compress(&[], _) -> decompress() == &[]` for all three codecs.
//! - `CompressionType::None` passthrough: bytes are stored unchanged.
//! - LZ4 roundtrip: `compress_prepend_size` + `decompress_size_prepended` cycle.
//! - Zstd roundtrip: stream Encoder + Decoder cycle at `DEFAULT_ZSTD_LEVEL`.
//! - Zstd preserves Loro importability: exported+compressed+decompressed+imported
//!   `LoroDoc` has `get_deep_value()` equal to the original doc.

#![allow(unused_imports)]

use grafeo_loro::compression::{CompressedPayload, LoroDocCompressionExt};
use grafeo_loro::config::CompressionType;
use grafeo_loro::constants::DEFAULT_ZSTD_LEVEL;
use loro::{ExportMode, LoroDoc};

/// LZ4 roundtrip: `CompressedPayload::compress(bytes, Lz4)` then `decompress()`
/// returns the original `bytes` exactly. Uses a non-trivial input (>64 KiB to
/// exceed LZ4's small-input fast path — verified at
/// `lz4_flex-0.11.6/src/block/mod.rs:77` `LZ4_64KLIMIT`).
#[test]
#[ignore = "P3T1-L1 scaffold: L3 implements the body"]
fn compression_lz4_roundtrip() {
    todo!()
}

/// Zstd roundtrip: `CompressedPayload::compress(bytes, Zstd)` then `decompress()`
/// returns the original `bytes` exactly. Uses `DEFAULT_ZSTD_LEVEL` (= 3). Input
/// should be large enough that compression actually shrinks it (otherwise the
/// test passes vacuously — anti-Goodhart).
#[test]
#[ignore = "P3T1-L1 scaffold: L3 implements the body"]
fn compression_zstd_roundtrip() {
    todo!()
}

/// Zstd preserves Loro importability: export `LoroDoc` A with
/// `ExportMode::Snapshot`, compress with Zstd, decompress, then import the
/// decompressed bytes into a fresh `LoroDoc` B. Assert
/// `A.get_deep_value() == B.get_deep_value()`. This is Phase 3 Task 1's
/// direct validation gate ("Zstd roundtrip preserves Loro importability").
#[test]
#[ignore = "P3T1-L1 scaffold: L3 implements the body"]
fn compression_zstd_preserves_loro_importability() {
    todo!()
}

/// None codec passthrough: `CompressedPayload::compress(bytes, None)` stores
/// `bytes` unchanged (no compression header, no size prefix). `decompress()`
/// returns the original bytes. Verifies the `None` arm is a pure clone, not
/// a tautological no-op that silently breaks on empty input.
#[test]
#[ignore = "P3T1-L1 scaffold: L3 implements the body"]
fn compression_none_passthrough() {
    todo!()
}

/// Empty input roundtrip: `CompressedPayload::compress(&[], strategy)` for each
/// of the three codecs produces a `CompressedPayload` whose `decompress()` is
/// also `&[]`. Anti-happy-path: empty input is a known edge case for Zstd
/// (the stream encoder still emits a frame header) and for LZ4
/// (`compress_prepend_size` prepends a 4-byte zero size).
#[test]
#[ignore = "P3T1-L1 scaffold: L3 implements the body"]
fn compression_empty_input_roundtrip() {
    todo!()
}
