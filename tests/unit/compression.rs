//! Phase 3 Task 1 tests: `compression::wrapper` roundtrips.
//!
//! All 5 tests assert the roundtrip contract — `CompressedPayload::compress`
//! followed by `CompressedPayload::decompress` returns the original bytes, AND
//! (for the Zstd-preserves-Loro-importability gate) the decompressed bytes import
//! into a fresh `LoroDoc` without error. Anti-Goodhart: assert BOTH byte-level
//! equality AND Loro semantic equivalence where applicable.
//!
//! Codec API citations live inline on `src/compression/wrapper.rs` fns.

use grafeo_loro::compression::{CompressedPayload, LoroDocCompressionExt};
use grafeo_loro::config::CompressionType;
use loro::{ExportMode, LoroDoc};

/// LZ4 roundtrip: `CompressedPayload::compress(bytes, Lz4)` then `decompress()`
/// returns the original `bytes` exactly. Uses a repeated pattern ≥32 bytes — LZ4
/// has poor compression ratio on tiny inputs, so a small input would pass vacuously.
#[test]
fn compression_lz4_roundtrip() {
    let input = b"hello compression world hello compression world";
    let payload = CompressedPayload::compress(input, CompressionType::Lz4)
        .expect("LZ4 compress is infallible in practice");
    assert_eq!(payload.compression, CompressionType::Lz4);
    assert_ne!(payload.raw_data.as_slice(), &input[..], "LZ4 must transform input (else test is vacuous)");
    let recovered = payload.decompress().expect("LZ4 decompress succeeds for valid payload");
    assert_eq!(recovered.as_slice(), &input[..]);
}

/// Zstd roundtrip: `CompressedPayload::compress(bytes, Zstd)` then `decompress()`
/// returns the original `bytes` exactly. Uses `DEFAULT_ZSTD_LEVEL` (= 3). Input
/// is large enough that compression actually shrinks it (anti-Goodhart — otherwise
/// the test passes vacuously).
#[test]
fn compression_zstd_roundtrip() {
    let input = b"hello compression world hello compression world";
    let payload = CompressedPayload::compress(input, CompressionType::Zstd)
        .expect("Zstd compress at level 3 succeeds for valid input");
    assert_eq!(payload.compression, CompressionType::Zstd);
    assert_ne!(payload.raw_data.as_slice(), &input[..], "Zstd must transform input (else test is vacuous)");
    let recovered = payload.decompress().expect("Zstd decompress succeeds for valid payload");
    assert_eq!(recovered.as_slice(), &input[..]);
}

/// Zstd preserves Loro importability: export `LoroDoc` A with `ExportMode::Snapshot`
/// (unit variant, `loro-internal-1.13.6/src/encoding.rs:55`, re-exported at
/// `loro-1.13.6/src/lib.rs:56`), compress with Zstd, decompress, then import the
/// decompressed bytes into a fresh `LoroDoc` B. Assert
/// `A.get_deep_value() == B.get_deep_value()` (Phase 3 Task 1's direct validation gate).
#[test]
fn compression_zstd_preserves_loro_importability() {
    let doc_a = LoroDoc::new();
    doc_a
        .get_text("text")
        .insert(0, "hello world")
        .expect("insert into LoroText succeeds");

    let payload = doc_a
        .export_compressed(ExportMode::Snapshot, CompressionType::Zstd)
        .expect("export + Zstd compress succeeds");
    assert_eq!(payload.compression, CompressionType::Zstd);

    let doc_b = LoroDoc::new();
    let status = doc_b
        .import_compressed(&payload)
        .expect("decompress + LoroDoc::import succeeds");
    // ImportStatus returned but no missing dependencies expected for a self-contained snapshot.
    let _ = status;

    assert_eq!(
        doc_a.get_deep_value(),
        doc_b.get_deep_value(),
        "export → Zstd compress → decompress → import must preserve CRDT state"
    );
}

/// None codec passthrough: `CompressedPayload::compress(bytes, None)` stores
/// `bytes` unchanged (no compression header, no size prefix). `decompress()`
/// returns the original bytes. Verifies the `None` arm is a pure clone, not a
/// tautological no-op that silently breaks on empty input.
#[test]
fn compression_none_passthrough() {
    let input = b"uncompressed payload";
    let payload = CompressedPayload::compress(input, CompressionType::None)
        .expect("None compress is infallible (pure clone)");
    assert_eq!(payload.compression, CompressionType::None);
    assert_eq!(payload.raw_data.as_slice(), &input[..], "None arm stores bytes verbatim (no header)");
    let recovered = payload.decompress().expect("None decompress is infallible (pure clone)");
    assert_eq!(recovered.as_slice(), &input[..]);
}

/// Empty-input roundtrip: iterate over `[CompressionType::None, Lz4, Zstd]` and
/// assert `CompressedPayload::compress(&[], t).decompress() == &[]` for each
/// (anti-Goodhart — pins the test shape so the test can't trivially pass by
/// testing only one codec). Anti-happy-path: Zstd produces a non-empty frame
/// header even for empty input; roundtrip must still yield empty `Vec<u8>`. LZ4
/// prepends a 4-byte zero size.
#[test]
fn compression_empty_input_roundtrip() {
    for strategy in [CompressionType::None, CompressionType::Lz4, CompressionType::Zstd] {
        let payload = CompressedPayload::compress(b"", strategy)
            .unwrap_or_else(|e| panic!("compress(&[], {strategy:?}) failed: {e}"));
        assert_eq!(payload.compression, strategy, "codec tag preserved for empty input");
        let recovered = payload
            .decompress()
            .unwrap_or_else(|e| panic!("decompress() failed for {strategy:?}: {e}"));
        assert_eq!(recovered, b"", "empty-input roundtrip must yield empty Vec for {strategy:?}");
    }
}
