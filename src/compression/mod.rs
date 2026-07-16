//! Compression envelope + `LoroDoc` extension trait.
//!
//! Issue #1 item 7: gated by `compression` feature. Pulls `lz4_flex` +
//! `brotli` + `flate2` — **all pure-Rust, all WASM-safe** (issue #3 sub-issue 1
//! replaced `zstd-sys` with `brotli` + `flate2`; the previous C dep broke
//! `wasm32-unknown-unknown` builds because it required a clang cross-compiler).
//!
//! ## Codec helper API
//!
//! In addition to the [`CompressedPayload`] envelope (which dispatches via
//! [`CompressionType`]), this module exposes standalone pure-Rust codec helpers
//! — [`brotli_compress`], [`brotli_decompress`], [`deflate_compress`],
//! [`deflate_decompress`] — so callers (and tests) can exercise each codec
//! directly without going through the enum (which is orchestrator-owned and
//! currently fixed at three variants: `None`, `Lz4`, `Zstd`).
//!
//! ```text
//! // TODO(orchestrator): rename CompressionType::Zstd variant → Brotli in
//! // src/config.rs. Once renamed, update src/compression/wrapper.rs match
//! // arms + tag mapping. Variant name is kept for now to avoid breaking the
//! // orchestrator's in-flight config refactor.
//! ```

pub mod wrapper;

pub use wrapper::{
    brotli_compress, brotli_decompress, deflate_compress, deflate_decompress, CompressedPayload,
    LoroDocCompressionExt,
};
