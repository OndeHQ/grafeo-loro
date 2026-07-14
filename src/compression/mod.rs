//! Compression envelope + `LoroDoc` extension trait.
//!
//! Issue #1 item 7: gated by `compression` feature. Pulls `lz4_flex` +
//! `zstd`. **NOT WASM-safe** when both codecs are enabled (zstd binds to
//! C lib). For WASM, enable only `lz4_flex` by forking the feature into
//! `compression-lz4` and `compression-zstd` — that split is deferred to a
//! follow-up; for now, do NOT enable `compression` in WASM builds.

pub mod wrapper;

pub use wrapper::{CompressedPayload, LoroDocCompressionExt};
