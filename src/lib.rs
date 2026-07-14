//! # grafeo-loro
//!
//! Local-first, in-process, dual-store graph database with CRDT consensus
//! (Loro) + execution (Grafeo). Issue #1 compliance: granular feature gates,
//! WASM target support, trait-abstracted runtime, tree-as-graph adapter.
//!
//! ## Feature matrix (issue #1 item 7)
//!
//! | Feature       | Pulls                                  | WASM-safe |
//! |---------------|----------------------------------------|-----------|
//! | `bridge`      | `lorosurgeon`, `parking_lot`           | yes       |
//! | `batcher`     | `bridge` + `tokio` (sync, time)        | no        |
//! | `compression` | `lz4_flex`, `zstd` (C lib)             | no*       |
//! | `tree`        | `bridge`                               | yes       |
//! | `storage`     | `async-trait`                          | yes       |
//! | `grafeo`      | `bridge` + `grafeo` (native only)      | no        |
//! | `onnx`        | `grafeo` + `ort` (native only)         | no        |
//! | `webrtc`      | `webrtc-rs` (native only)              | no        |
//! | `telemetry`   | `opentelemetry`, `tracing`             | yes       |
//! | `wasm`        | `wasm-bindgen`, `js-sys`, `web-sys`    | required  |
//! | `parallel`    | `rayon`                                | no        |
//! | `serde`       | `serde`, `serde_json`                  | yes       |
//! | `full`        | all native features                   | no        |
//!
//! `*` `compression` uses `zstd` which binds to a C library; do NOT enable
//! it in WASM builds. Use `lz4_flex` only via a future `compression-lz4`
//! sub-feature, or disable compression entirely in the browser.
//!
//! ## Onde's recommended feature set
//!
//! ```toml
//! grafeo-loro = { version = "0.2", default-features = false, features = ["bridge", "batcher", "compression", "tree"] }
//! ```
//!
//! ## Minimal WASM smoke test
//!
//! ```toml
//! grafeo-loro = { version = "0.2", default-features = false, features = ["bridge", "tree", "wasm"] }
//! ```
//!
//! ## MSRV
//!
//! Rust 1.80+ (verified via `cargo msrv` against the dep tree).

// ============================================================================
// Always-on core: error, constants, config, types
// ============================================================================
// These modules have no heavy deps; they form the shared vocabulary of the
// crate and are always available regardless of feature selection.

pub mod config;
pub mod constants;
pub mod error;
pub mod types;

// ============================================================================
// Feature-gated modules
// ============================================================================

/// Bridge: bidirectional Loro↔Grafeo sync types + id-mapping.
///
/// Required by `bridge` feature. The `MutationBatcher` requires `batcher`
/// (which pulls `tokio`). The `SyncEngine` itself requires `batcher` because
/// it owns the tokio MPSC channels.
#[cfg(feature = "bridge")]
pub mod bridge;

/// Compression envelope + LoroDoc extension trait.
///
/// Required by `compression` feature. Pulls `lz4_flex` + `zstd`.
#[cfg(feature = "compression")]
pub mod compression;

/// Hydration: cold-boot rebuild of Grafeo indexes from a Loro snapshot.
///
/// `parallel_hydrate_grafeo` requires the `parallel` feature (pulls `rayon`).
/// The serial fallback is always available when `grafeo` is on.
#[cfg(all(feature = "grafeo", not(target_family = "wasm")))]
pub mod hydration;

/// Ephemeral presence: WebSocket-channel %EPH envelope.
///
/// Independent of bridge/grafeo. `webrtc` feature wires a transport.
#[cfg(feature = "bridge")]
pub mod presence;

/// Grafeo schema entities: `VertexEntity`, `EdgeEntity`, `OrderedCollection`.
#[cfg(feature = "bridge")]
pub mod schema;

/// Storage backend trait + in-memory reference impl.
#[cfg(feature = "storage")]
pub mod storage;

/// Telemetry: metrics, traces, health probes.
#[cfg(feature = "telemetry")]
pub mod telemetry;

/// Tree-as-graph adapter (issue #1 item 8).
///
/// First-class `tree` module that turns `:CHILD` edges into ergonomic
/// `parent` / `children` / `indent` / `outdent` operations. Independent
/// of grafeo internals — works against the bridge surface only.
#[cfg(feature = "tree")]
pub mod tree_adapter;

/// FFI-friendly hot-path API (issue #1 item 6).
///
/// `NodeOp` is `#[repr(C)]` using `&str` not `String`. Bincode-only
/// `apply_loro_op_bytes` for sub-µs FFI.
#[cfg(feature = "bridge")]
pub mod ffi;

/// WASM bindings: `JsValue` error bridge + `wasm-bindgen` prelude.
///
/// Only available on `target_family = "wasm"`.
#[cfg(all(feature = "wasm", target_family = "wasm"))]
pub mod wasm;

/// Trait-abstracted async runtime (issue #1 item 2).
///
/// `Mailbox<T>` trait lets Onde plug in `wasm-bindgen-futures` +
/// `web-sys::MessageChannel` instead of `tokio::sync::mpsc`. The tokio
/// impl lives behind the `batcher` feature.
#[cfg(feature = "bridge")]
pub mod runtime;

// ============================================================================
// Top-level facade: `GrafeoLoroApp`
// ============================================================================
//
// The app facade requires `bridge` + `storage` + `telemetry` to be useful
// in production. Tests that exercise the facade enable `full`.

#[cfg(all(feature = "bridge", feature = "storage", feature = "grafeo", feature = "batcher"))]
pub mod app;

// ============================================================================
// Re-exports
// ============================================================================

#[cfg(all(feature = "bridge", feature = "storage", feature = "grafeo", feature = "batcher"))]
pub use app::GrafeoLoroApp;
#[cfg(all(feature = "batcher", feature = "grafeo"))]
pub use bridge::sync_engine::InboundMsg;
#[cfg(feature = "bridge")]
pub use bridge::BridgeMaps;
#[cfg(all(feature = "batcher", feature = "grafeo"))]
pub use bridge::SyncEngine;
pub use config::{CompressionType, SsotMode};
pub use error::GrafeoLoroError;
#[cfg(feature = "compression")]
pub use compression::{CompressedPayload, LoroDocCompressionExt};
#[cfg(all(feature = "grafeo", feature = "parallel", not(target_family = "wasm")))]
pub use hydration::parallel_hydrate_grafeo;
#[cfg(all(feature = "grafeo", not(target_family = "wasm")))]
pub use hydration::vector::generate_local_embedding;
#[cfg(all(feature = "grafeo", not(target_family = "wasm")))]
pub use hydration::VectorOffloadManager;
#[cfg(feature = "storage")]
pub use storage::StorageBackend;
// Re-exports for tree_adapter, ffi, wasm modules will be added by parallel
// agents (tasks 5, 6, 8) when they fill in their stub modules.

// Re-export native crates so raw handles are usable immediately (issue #1
// item 4: Onde receives the `LoroDoc` from `GrafeoLoroApp::doc()` and calls
// native Loro APIs directly via this re-export).
#[cfg(feature = "grafeo")]
pub use grafeo;
#[cfg(feature = "bridge")]
pub use loro;
