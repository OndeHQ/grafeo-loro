//! Phase 4 Task (P4-L3) tests: `CompressedPayload` on-wire format.
//!
//! Validates the P4-DEVIL m2 wire format implemented in
//! `src/compression/wrapper.rs`:
//!
//! - `compress_to_wire(&[u8], CompressionType) -> Vec<u8>` produces
//!   `[version:u8=1][codec_tag:u8][raw_data..]`.
//! - `decompress_from_wire(&[u8]) -> Vec<u8>` parses the wire format and
//!   dispatches to the matching codec.
//! - Round-trip equality for all 3 codecs (`None`, `Lz4`, `Zstd`).
//! - Unknown codec tag → `Err(Compression(...))`.
//! - Unknown version byte → `Err(Compression(...))`.
//! - Too-short payload (<2 bytes) → `Err(Compression(...))`.
//!
//! Anti-Goodhart: each codec case uses an input large enough that compression
//! actually transforms the bytes (verified via `assert_ne!` on the wire bytes
//! vs. the raw input for `Lz4`/`Zstd`). Anti-happy-path: each error case uses
//! a distinct invalid payload shape (too-short, bad-version, bad-tag).
//!
//! Codec API citations live inline on `src/compression/wrapper.rs` fns.

#![allow(missing_docs)]

use grafeo_loro::compression::CompressedPayload;
use grafeo_loro::config::CompressionType;
use grafeo_loro::error::GrafeoLoroError;

/// Repeated-pattern input large enough that LZ4 + Zstd actually compress it
/// (anti-Goodhart — a tiny input would pass roundtrip vacuously).
const INPUT: &[u8] = b"hello compression world hello compression world hello compression world";

/// Wire-format constants (mirror `src/compression/wrapper.rs`).
const WIRE_VERSION: u8 = 1;
const TAG_NONE: u8 = 0x00;
const TAG_LZ4: u8 = 0x01;
const TAG_ZSTD: u8 = 0x02;

/// `None` codec round-trip: `compress_to_wire(bytes, None)` then
/// `decompress_from_wire` returns the original `bytes` exactly. The wire bytes
/// are `[1, 0x00, ...bytes]` — version + None-tag + raw passthrough.
#[test]
fn compression_wire_roundtrip_none() {
    let wire = CompressedPayload::compress_to_wire(INPUT, CompressionType::None)
        .expect("None compress_to_wire is infallible");
    // Header: version=1, tag=0x00 (None).
    assert_eq!(wire[0], WIRE_VERSION, "version byte");
    assert_eq!(wire[1], TAG_NONE, "codec tag byte for None");
    // Body: raw passthrough (no compression header from lz4/zstd).
    assert_eq!(
        &wire[2..],
        INPUT,
        "None arm stores raw bytes verbatim after the 2-byte header"
    );
    let recovered = CompressedPayload::decompress_from_wire(&wire)
        .expect("None decompress_from_wire is infallible");
    assert_eq!(recovered.as_slice(), INPUT);
}

/// `Lz4` codec round-trip. Wire bytes start with `[1, 0x01, ...lz4_bytes]` —
/// the LZ4 frame is `compress_prepend_size` output (4-byte size + compressed
/// body). Anti-Goodhart: assert the wire body differs from the raw input.
#[test]
fn compression_wire_roundtrip_lz4() {
    let wire = CompressedPayload::compress_to_wire(INPUT, CompressionType::Lz4)
        .expect("LZ4 compress_to_wire is infallible (lz4_flex is infallible)");
    assert_eq!(wire[0], WIRE_VERSION, "version byte");
    assert_eq!(wire[1], TAG_LZ4, "codec tag byte for Lz4");
    assert_ne!(
        &wire[2..],
        INPUT,
        "LZ4 must transform input (else test is vacuous)"
    );
    let recovered = CompressedPayload::decompress_from_wire(&wire)
        .expect("LZ4 decompress_from_wire succeeds for valid payload");
    assert_eq!(recovered.as_slice(), INPUT);
}

/// `Zstd` codec round-trip. Wire bytes start with `[1, 0x02, ...zstd_bytes]` —
/// the Zstd frame is `encode_all(level=3)` output. Anti-Goodhart: assert the
/// wire body differs from the raw input.
#[test]
fn compression_wire_roundtrip_zstd() {
    let wire = CompressedPayload::compress_to_wire(INPUT, CompressionType::Zstd)
        .expect("Zstd compress_to_wire at level 3 succeeds for valid input");
    assert_eq!(wire[0], WIRE_VERSION, "version byte");
    assert_eq!(wire[1], TAG_ZSTD, "codec tag byte for Zstd");
    assert_ne!(
        &wire[2..],
        INPUT,
        "Zstd must transform input (else test is vacuous)"
    );
    let recovered = CompressedPayload::decompress_from_wire(&wire)
        .expect("Zstd decompress_from_wire succeeds for valid payload");
    assert_eq!(recovered.as_slice(), INPUT);
}

/// Empty-input round-trip across all 3 codecs. Anti-happy-path: Zstd produces
/// a non-empty frame header even for empty input; LZ4 prepends a 4-byte zero
/// size; None stores the empty body verbatim. Round-trip must still yield the
/// empty `Vec<u8>` for each.
#[test]
fn compression_wire_empty_input_roundtrip() {
    for strategy in [CompressionType::None, CompressionType::Lz4, CompressionType::Zstd] {
        let wire = CompressedPayload::compress_to_wire(b"", strategy)
            .unwrap_or_else(|e| panic!("compress_to_wire(&[], {strategy:?}) failed: {e}"));
        // Header is always 2 bytes regardless of body length.
        assert_eq!(wire[0], WIRE_VERSION, "version byte for {strategy:?}");
        let expected_tag = match strategy {
            CompressionType::None => TAG_NONE,
            CompressionType::Lz4 => TAG_LZ4,
            CompressionType::Zstd => TAG_ZSTD,
        };
        assert_eq!(wire[1], expected_tag, "codec tag byte for {strategy:?}");
        let recovered = CompressedPayload::decompress_from_wire(&wire)
            .unwrap_or_else(|e| panic!("decompress_from_wire failed for {strategy:?}: {e}"));
        assert_eq!(
            recovered,
            b"",
            "empty-input roundtrip must yield empty Vec for {strategy:?}"
        );
    }
}

/// Unknown codec tag (e.g. `0xFF`) → `Err(Compression(...))`. Validates that
/// `tag_to_compression_type` rejects out-of-range tags instead of silently
/// mis-decoding (anti-hallucination — no invented codec).
#[test]
fn compression_wire_unknown_codec_tag_rejected() {
    // Build a synthetic wire payload with an invalid tag byte: version=1, tag=0xFF,
    // body = passthrough of INPUT (any bytes work — the parser must reject at the
    // tag, not at the body).
    let mut bad = Vec::with_capacity(2 + INPUT.len());
    bad.push(WIRE_VERSION);
    bad.push(0xFF);
    bad.extend_from_slice(INPUT);
    let err = CompressedPayload::decompress_from_wire(&bad)
        .expect_err("unknown codec tag 0xFF must be rejected");
    assert!(
        matches!(err, GrafeoLoroError::Compression(ref msg) if msg.contains("unknown codec tag")),
        "expected Compression(unknown codec tag ...), got {err:?}"
    );
}

/// Unknown wire-format version (e.g. `0x42`) → `Err(Compression(...))`.
/// Validates that `from_wire` rejects future versions instead of silently
/// mis-parsing (forward-compat gate — bumped only on incompatible layout
/// changes per the WIRE_FORMAT_VERSION doc-comment).
#[test]
fn compression_wire_unknown_version_rejected() {
    let mut bad = Vec::with_capacity(2 + INPUT.len());
    bad.push(0x42); // unknown future version
    bad.push(TAG_ZSTD);
    bad.extend_from_slice(INPUT);
    let err = CompressedPayload::decompress_from_wire(&bad)
        .expect_err("unknown version byte must be rejected");
    assert!(
        matches!(err, GrafeoLoroError::Compression(ref msg) if msg.contains("unknown version")),
        "expected Compression(unknown version ...), got {err:?}"
    );
}

/// Too-short payload (0 bytes or 1 byte) → `Err(Compression(...))`. The
/// 2-byte header (version + codec tag) is the minimum; `from_wire` rejects
/// payloads shorter than that with a deterministic error (anti-hallucination
/// — no panic on empty slice, no silent accept of partial header).
#[test]
fn compression_wire_too_short_rejected() {
    for bad_len in [0, 1] {
        let bad = vec![0u8; bad_len];
        let err = CompressedPayload::decompress_from_wire(&bad)
            .expect_err("too-short payload must be rejected");
        assert!(
            matches!(err, GrafeoLoroError::Compression(ref msg) if msg.contains("too few bytes")),
            "expected Compression(too few bytes ...) for len={bad_len}, got {err:?}"
        );
    }
}

/// Symmetry: `to_wire` followed by `from_wire` recovers the original
/// `CompressedPayload` struct (codec + raw_data) byte-for-byte. Validates the
/// two helpers are exact inverses (anti-tautology — the previous tests only
/// validated the decompressed bytes, not the parsed struct shape).
#[test]
fn compression_wire_to_wire_from_wire_symmetric() {
    for strategy in [CompressionType::None, CompressionType::Lz4, CompressionType::Zstd] {
        let payload = CompressedPayload::compress(INPUT, strategy)
            .unwrap_or_else(|e| panic!("compress({strategy:?}) failed: {e}"));
        let wire = payload.to_wire();
        let recovered = CompressedPayload::from_wire(&wire)
            .unwrap_or_else(|e| panic!("from_wire failed for {strategy:?}: {e}"));
        assert_eq!(
            recovered, payload,
            "to_wire/from_wire must round-trip the CompressedPayload struct for {strategy:?}"
        );
    }
}
