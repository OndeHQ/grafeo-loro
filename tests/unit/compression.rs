//! Phase 3 Task 1 scaffolds: `compression::wrapper` roundtrips.
//!
//! All 5 tests assert the roundtrip contract — `CompressedPayload::compress`
//! followed by `CompressedPayload::decompress` returns the original bytes, AND
//! (for the Zstd-preserves-Loro-importability gate) the decompressed bytes import
//! into a fresh `LoroDoc` without error. Anti-Goodhart: assert BOTH byte-level
//! equality AND Loro semantic equivalence where applicable. Bodies are `todo!()`
//! (L3 implements; L2 wiring-only).
//!
//! Codec API citations live inline on `src/compression/wrapper.rs` fns.

// DEVIL n3: silencer retained because test bodies are `todo!()` (L3 work);
// deleting it would produce 3+ unused-import warnings (DEFAULT_ZSTD_LEVEL,
// ExportMode, LoroDoc, CompressedPayload, LoroDocCompressionExt). L3 removes
// this when bodies are filled. Matches P2T2-L1/P2T3-L1 precedent.
#![allow(unused_imports)]

use grafeo_loro::compression::{CompressedPayload, LoroDocCompressionExt};
use grafeo_loro::config::CompressionType;
use grafeo_loro::constants::DEFAULT_ZSTD_LEVEL;
use loro::{ExportMode, LoroDoc};

/// LZ4 roundtrip: `CompressedPayload::compress(bytes, Lz4)` then `decompress()`
/// returns the original `bytes` exactly. Uses a non-trivial input (>64 KiB) — LZ4
/// has poor compression ratio on tiny inputs, so a small input would pass vacuously.
#[test]
#[ignore = "P3T1-L1 scaffold: L3 implements the body"]
fn compression_lz4_roundtrip() {
    todo!()
}

/// Zstd roundtrip: `CompressedPayload::compress(bytes, Zstd)` then `decompress()`
/// returns the original `bytes` exactly. Uses `DEFAULT_ZSTD_LEVEL` (= 3). Input
/// is large enough that compression actually shrinks it (anti-Goodhart — otherwise
/// the test passes vacuously).
#[test]
#[ignore = "P3T1-L1 scaffold: L3 implements the body"]
fn compression_zstd_roundtrip() {
    todo!()
}

/// Zstd preserves Loro importability: export `LoroDoc` A with `ExportMode::Snapshot`
/// (unit variant, `loro-internal-1.13.6/src/encoding.rs:55`, re-exported at
/// `loro-1.13.6/src/lib.rs:56`), compress with Zstd, decompress, then import the
/// decompressed bytes into a fresh `LoroDoc` B. Assert
/// `A.get_deep_value() == B.get_deep_value()` (Phase 3 Task 1's direct validation gate).
#[test]
#[ignore = "P3T1-L1 scaffold: L3 implements the body"]
fn compression_zstd_preserves_loro_importability() {
    todo!()
}

/// None codec passthrough: `CompressedPayload::compress(bytes, None)` stores
/// `bytes` unchanged (no compression header, no size prefix). `decompress()`
/// returns the original bytes. Verifies the `None` arm is a pure clone, not a
/// tautological no-op that silently breaks on empty input.
#[test]
#[ignore = "P3T1-L1 scaffold: L3 implements the body"]
fn compression_none_passthrough() {
    todo!()
}

/// Empty-input roundtrip: iterate over `[CompressionType::None, Lz4, Zstd]` and
/// assert `CompressedPayload::compress(&[], t).decompress() == &[]` for each
/// (anti-Goodhart — pins the test shape so L3 can't trivially pass by testing
/// only one codec). Anti-happy-path: Zstd produces a non-empty frame header even
/// for empty input; roundtrip must still yield empty `Vec<u8>`. LZ4 prepends a
/// 4-byte zero size.
#[test]
#[ignore = "P3T1-L1 scaffold: L3 implements the body"]
fn compression_empty_input_roundtrip() {
    todo!()
}
