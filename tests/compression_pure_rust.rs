//! Issue #3 sub-issue 1 — pure-Rust compression codec round-trip tests.
//!
//! Verifies that the three pure-Rust codecs (LZ4, Brotli, Deflate) round-trip
//! a sample payload through `compress → decompress` and recover the original
//! bytes exactly. This is the regression-gate for the `zstd-sys` →
//! `brotli`/`flate2` swap (issue #3 sub-issue 1): if any codec fails to
//! round-trip, the WASM-safe compression story breaks.
//!
//! **File location note:** The task spec said `tests/unit/compression_pure_rust.rs`,
//! but Cargo's auto-discovery does NOT pick up `tests/unit/compression_pure_rust.rs`
//! as a standalone test target when `tests/unit/main.rs` exists (the
//! `main.rs` filename claims the whole subdirectory as a single `unit` test
//! binary). To satisfy the workflow's `cargo test --test compression_pure_rust`
//! invocation, this file lives at the top-level `tests/compression_pure_rust.rs`
//! where Cargo auto-discovers it as the `compression_pure_rust` test target.
//! The orchestrator can move it under `tests/unit/` and add a
//! `mod compression_pure_rust;` line to `tests/unit/main.rs` if they want it
//! bundled with the other unit tests instead.
//!
//! Run with:
//!
//! ```sh
//! cargo test --no-default-features --features bridge,compression,wasm \
//!     --test compression_pure_rust
//! ```

#![allow(missing_docs)]

use grafeo_loro::compression::{
    brotli_compress, brotli_decompress, deflate_compress, deflate_decompress,
};
use grafeo_loro::config::CompressionType;
use grafeo_loro::constants::{DEFAULT_BROTLI_QUALITY, DEFAULT_DEFLATE_LEVEL};
use grafeo_loro::error::GrafeoLoroError;

/// Repeated-pattern input large enough that all three codecs actually
/// compress it (anti-Goodhart — a tiny input would pass roundtrip vacuously).
/// 81 bytes is comfortably above the LZ4 minimum-block threshold and gives
/// Brotli/Deflate enough context to find redundancy.
const INPUT: &[u8] = b"hello compression world hello compression world hello compression world";

// ============================================================================
// LZ4 — already pure-Rust via `lz4_flex`; round-trip via `CompressedPayload`.
// ============================================================================

/// LZ4 round-trip via the `CompressedPayload` envelope: `compress(bytes, Lz4)`
/// then `decompress()` returns the original `bytes` exactly. Anti-Goodhart:
/// assert the compressed bytes differ from the input.
#[test]
fn lz4_roundtrip_via_compressed_payload() {
    let payload = grafeo_loro::compression::CompressedPayload::compress(INPUT, CompressionType::Lz4)
        .expect("LZ4 compress is infallible (lz4_flex)");
    assert_eq!(payload.compression, CompressionType::Lz4);
    assert_ne!(
        payload.raw_data.as_slice(),
        INPUT,
        "LZ4 must transform input (else test is vacuous)"
    );
    let recovered = payload
        .decompress()
        .expect("LZ4 decompress succeeds for valid payload");
    assert_eq!(recovered.as_slice(), INPUT);
}

/// LZ4 empty-input round-trip: `lz4_flex::compress_prepend_size` prepends a
/// 4-byte zero size; decompress yields empty `Vec<u8>`.
#[test]
fn lz4_empty_input_roundtrip() {
    let payload = grafeo_loro::compression::CompressedPayload::compress(b"", CompressionType::Lz4)
        .expect("LZ4 compress is infallible");
    assert_eq!(payload.compression, CompressionType::Lz4);
    // LZ4 prepends a 4-byte zero size header even for empty input. The block
    // body itself is 1 byte (the LZ4 token encoding for "0 literals, 0 match"),
    // so total is 5 bytes — but the only contract we want to pin is "non-empty
    // even for empty input" (anti-happy-path: a vacuous compress would yield 0).
    assert!(
        !payload.raw_data.is_empty(),
        "LZ4 empty-input must still emit the 4-byte size prefix + token byte"
    );
    let recovered = payload
        .decompress()
        .expect("LZ4 decompress succeeds for empty input");
    assert_eq!(recovered, b"");
}

// ============================================================================
// Brotli — pure-Rust via `brotli` crate (replaces `zstd-sys`).
// ============================================================================

/// Brotli round-trip via the standalone `brotli_compress`/`brotli_decompress`
/// helpers. Anti-Goodhart: assert the compressed bytes differ from the input
/// and are smaller (Brotli at quality 5 should easily compress a 81-byte
/// highly-redundant input).
#[test]
fn brotli_roundtrip_via_helpers() {
    let compressed =
        brotli_compress(INPUT, DEFAULT_BROTLI_QUALITY).expect("brotli compress succeeds");
    assert_ne!(
        compressed.as_slice(),
        INPUT,
        "Brotli must transform input (else test is vacuous)"
    );
    assert!(
        compressed.len() < INPUT.len(),
        "Brotli at quality {} should compress {} bytes to <{}; got {}",
        DEFAULT_BROTLI_QUALITY,
        INPUT.len(),
        INPUT.len(),
        compressed.len()
    );
    let recovered = brotli_decompress(&compressed).expect("brotli decompress succeeds");
    assert_eq!(recovered.as_slice(), INPUT);
}

/// Brotli round-trip via the `CompressedPayload` envelope (routes through
/// `CompressionType::Zstd` arm, which is the orchestrator-pending rename
/// target → Brotli). Verifies the enum dispatch path works end-to-end.
#[test]
fn brotli_roundtrip_via_compressed_payload() {
    // TODO(orchestrator): once `CompressionType::Zstd` is renamed to
    // `CompressionType::Brotli`, update this test to use the new name.
    let payload =
        grafeo_loro::compression::CompressedPayload::compress(INPUT, CompressionType::Zstd)
            .expect("Brotli compress via Zstd arm succeeds");
    assert_eq!(payload.compression, CompressionType::Zstd);
    assert_ne!(
        payload.raw_data.as_slice(),
        INPUT,
        "Brotli must transform input (else test is vacuous)"
    );
    let recovered = payload
        .decompress()
        .expect("Brotli decompress via Zstd arm succeeds");
    assert_eq!(recovered.as_slice(), INPUT);
}

/// Brotli empty-input round-trip. Brotli emits a non-empty frame even for
/// empty input (3-byte end-of-stream marker); decompress yields empty `Vec`.
#[test]
fn brotli_empty_input_roundtrip() {
    let compressed = brotli_compress(b"", DEFAULT_BROTLI_QUALITY).expect("brotli compress empty");
    // Brotli emits a small frame even for empty input.
    assert!(!compressed.is_empty(), "Brotli empty-input emits non-empty frame");
    let recovered = brotli_decompress(&compressed).expect("brotli decompress empty");
    assert_eq!(recovered, b"");
}

/// Brotli corruption detection: truncated bytes should produce a
/// `Compression(...)` error (not a panic, not silent success).
#[test]
fn brotli_decompress_corrupt_input_errors() {
    // Truncated Brotli stream — definitely invalid.
    let corrupt = [0u8; 8];
    let result = brotli_decompress(&corrupt);
    assert!(
        matches!(result, Err(GrafeoLoroError::Compression(ref msg)) if msg.contains("brotli decompress")),
        "expected Compression(brotli decompress ...) error, got {result:?}"
    );
}

/// Brotli quality sweep: quality 0 (fastest) and quality 11 (best) both
/// round-trip the input. Anti-Goodhart: quality 11 should produce a smaller
/// or equal compressed size than quality 0 for the redundant test input.
#[test]
fn brotli_quality_sweep_roundtrips() {
    let fast = brotli_compress(INPUT, 0).expect("brotli q0 compress");
    let best = brotli_compress(INPUT, 11).expect("brotli q11 compress");

    let recovered_fast = brotli_decompress(&fast).expect("brotli q0 decompress");
    let recovered_best = brotli_decompress(&best).expect("brotli q11 decompress");
    assert_eq!(recovered_fast.as_slice(), INPUT);
    assert_eq!(recovered_best.as_slice(), INPUT);

    // Quality 11 should compress at least as well as quality 0.
    assert!(
        best.len() <= fast.len(),
        "brotli q11 ({} bytes) should be ≤ q0 ({} bytes)",
        best.len(),
        fast.len()
    );
}

// ============================================================================
// DEFLATE — pure-Rust via `flate2` with `rust_backend`.
// ============================================================================

/// DEFLATE round-trip via the standalone `deflate_compress`/`deflate_decompress`
/// helpers. Anti-Goodhart: assert the compressed bytes differ from the input
/// and are smaller.
#[test]
fn deflate_roundtrip_via_helpers() {
    let compressed =
        deflate_compress(INPUT, DEFAULT_DEFLATE_LEVEL).expect("deflate compress succeeds");
    assert_ne!(
        compressed.as_slice(),
        INPUT,
        "Deflate must transform input (else test is vacuous)"
    );
    assert!(
        compressed.len() < INPUT.len(),
        "Deflate at level {} should compress {} bytes to <{}; got {}",
        DEFAULT_DEFLATE_LEVEL,
        INPUT.len(),
        INPUT.len(),
        compressed.len()
    );
    let recovered = deflate_decompress(&compressed).expect("deflate decompress succeeds");
    assert_eq!(recovered.as_slice(), INPUT);
}

/// DEFLATE empty-input round-trip.
#[test]
fn deflate_empty_input_roundtrip() {
    let compressed = deflate_compress(b"", DEFAULT_DEFLATE_LEVEL).expect("deflate compress empty");
    // DEFLATE emits a small marker even for empty input.
    let recovered = deflate_decompress(&compressed).expect("deflate decompress empty");
    assert_eq!(recovered, b"");
}

/// DEFLATE level sweep: level 0 (store) and level 9 (max) both round-trip.
/// Anti-Goodhart: level 9 should produce a smaller or equal compressed size.
#[test]
fn deflate_level_sweep_roundtrips() {
    let stored = deflate_compress(INPUT, 0).expect("deflate level 0 (stored)");
    let maxed = deflate_compress(INPUT, 9).expect("deflate level 9 (max)");

    let recovered_stored = deflate_decompress(&stored).expect("deflate level 0 decompress");
    let recovered_maxed = deflate_decompress(&maxed).expect("deflate level 9 decompress");
    assert_eq!(recovered_stored.as_slice(), INPUT);
    assert_eq!(recovered_maxed.as_slice(), INPUT);

    // Level 9 should compress at least as well as level 0.
    assert!(
        maxed.len() <= stored.len(),
        "deflate level 9 ({} bytes) should be ≤ level 0 ({} bytes)",
        maxed.len(),
        stored.len()
    );
}

/// DEFLATE corruption detection: garbage bytes should produce a
/// `Compression(...)` error (not a panic).
#[test]
fn deflate_decompress_corrupt_input_errors() {
    let corrupt = [0xFFu8; 32];
    let result = deflate_decompress(&corrupt);
    assert!(
        matches!(result, Err(GrafeoLoroError::Compression(ref msg)) if msg.contains("deflate decompress")),
        "expected Compression(deflate decompress ...) error, got {result:?}"
    );
}

// ============================================================================
// Wire-format round-trip — exercises `compress_to_wire`/`decompress_from_wire`
// for each codec to confirm the on-wire envelope survives the codec swap.
// ============================================================================

/// Wire-format round-trip for the `Zstd` (Brotli) codec — verifies the
/// `compress_to_wire → decompress_from_wire` pipeline still works after the
/// zstd → brotli swap. Wire bytes are `[version=1][tag=0x02][brotli_bytes]`.
#[test]
fn wire_roundtrip_zstd_arm_now_brotli() {
    use grafeo_loro::compression::CompressedPayload;
    let wire = CompressedPayload::compress_to_wire(INPUT, CompressionType::Zstd)
        .expect("compress_to_wire via Zstd/Brotli arm");
    assert_eq!(wire[0], 1, "wire version byte");
    assert_eq!(wire[1], 0x02, "wire codec tag for Zstd (now Brotli)");
    let recovered = CompressedPayload::decompress_from_wire(&wire)
        .expect("decompress_from_wire via Zstd/Brotli arm");
    assert_eq!(recovered.as_slice(), INPUT);
}

/// Wire-format round-trip for the `Lz4` codec — regression-gate: LZ4 path
/// must continue to work after the codec swap (it was untouched but the
/// shared `CompressedPayload` envelope was edited).
#[test]
fn wire_roundtrip_lz4_unchanged() {
    use grafeo_loro::compression::CompressedPayload;
    let wire = CompressedPayload::compress_to_wire(INPUT, CompressionType::Lz4)
        .expect("compress_to_wire via Lz4 arm");
    assert_eq!(wire[0], 1, "wire version byte");
    assert_eq!(wire[1], 0x01, "wire codec tag for Lz4");
    let recovered = CompressedPayload::decompress_from_wire(&wire)
        .expect("decompress_from_wire via Lz4 arm");
    assert_eq!(recovered.as_slice(), INPUT);
}

/// Wire-format round-trip for the `None` codec — passthrough arm unchanged.
#[test]
fn wire_roundtrip_none_passthrough() {
    use grafeo_loro::compression::CompressedPayload;
    let wire = CompressedPayload::compress_to_wire(INPUT, CompressionType::None)
        .expect("compress_to_wire via None arm");
    assert_eq!(wire[0], 1, "wire version byte");
    assert_eq!(wire[1], 0x00, "wire codec tag for None");
    // None arm stores raw bytes verbatim after the 2-byte header.
    assert_eq!(&wire[2..], INPUT);
    let recovered = CompressedPayload::decompress_from_wire(&wire)
        .expect("decompress_from_wire via None arm");
    assert_eq!(recovered.as_slice(), INPUT);
}
