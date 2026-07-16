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

/// WASM bindings: `JsValue` error bridge + `wasm-bindgen` prelude (issue #1
/// item 12).
///
/// The module itself is gated by `feature = "wasm"` only — the target-
/// agnostic `error_code` function is available on native (for testing) too.
/// The JsValue-using pieces (`js_error`, `From<GrafeoLoroError> for JsValue`,
/// `init_panic_hook`) are internally gated by `target_family = "wasm"` so
/// they only compile on actual WASM targets.
#[cfg(feature = "wasm")]
pub mod wasm;

/// Trait-abstracted async runtime (issue #1 item 2).
///
/// `Mailbox<T>` trait lets Onde plug in `wasm-bindgen-futures` +
/// `web-sys::MessageChannel` instead of `tokio::sync::mpsc`. The tokio
/// impl lives behind the `batcher` feature.
#[cfg(feature = "bridge")]
pub mod runtime;

// ============================================================================
// Issue #3 sub-issue 6: native shadow commits, FTS, SAB layout
// ============================================================================

/// Native Git DAG / shadow commit API (issue #3 sub-issue 6).
///
/// Exposes per-writer WIP ref namespace (`refs/wip/<peerId>`) so downstream
/// no longer needs `isomorphic-git` (80KB) for bounded undo.
#[cfg(feature = "shadow")]
pub mod shadow;

/// Lightweight native full-text-search inverted index (issue #3 sub-issue 6).
///
/// <20MB WASM memory ceiling. Replaces heavy ONNX/HNSW runtimes for
/// text-search workloads.
#[cfg(feature = "fts")]
pub mod fts;

/// Native SAB (SharedArrayBuffer) layout writer (issue #3 sub-issue 6).
///
/// Pushes Y-offset / height math to Rust and writes directly to a SAB
/// pointer so the JS virtualizer doesn't have to.
#[cfg(feature = "sab")]
pub mod sab;

// ============================================================================
// Issue #3 sub-issue 10: observability
// ============================================================================

/// Observability hooks: queue state, fault injection, invariant checks
/// (issue #3 sub-issue 10).
///
/// Exposes internal queue state (`depth`, `oldest_age`, `locked_nodes`) to
/// JS. Adds fault-injection hooks for testing. Provides invariant-check
/// API for I4/I5/I11/I12/I14 post-mutation assertions.
#[cfg(feature = "observability")]
pub mod observability;

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
#[cfg(all(feature = "batcher", feature = "grafeo", feature = "telemetry"))]
pub use bridge::sync_engine::InboundMsg;
#[cfg(feature = "bridge")]
pub use bridge::BridgeMaps;
#[cfg(all(feature = "batcher", feature = "grafeo", feature = "telemetry"))]
pub use bridge::SyncEngine;
pub use config::{CompressionType, SsotMode};
pub use error::GrafeoLoroError;
#[cfg(feature = "compression")]
pub use compression::{CompressedPayload, LoroDocCompressionExt};
#[cfg(all(feature = "grafeo", feature = "parallel", not(target_family = "wasm")))]
pub use hydration::parallel_hydrate_grafeo;
#[cfg(all(feature = "grafeo", not(target_family = "wasm")))]
pub use hydration::hydrate_grafeo;
#[cfg(all(feature = "grafeo", not(target_family = "wasm")))]
pub use hydration::vector::generate_local_embedding;
#[cfg(all(feature = "grafeo", not(target_family = "wasm")))]
pub use hydration::VectorOffloadManager;
#[cfg(feature = "storage")]
pub use storage::StorageBackend;
#[cfg(feature = "storage")]
pub use storage::InMemoryStorage;
pub use error::Result;
// Trait-abstracted async runtime (issue #1 item 2). The `Mailbox` trait +
// `MailboxClosed` error are available with `bridge` alone; the tokio-backed
// `TokioMailbox` impl requires `batcher` (which pulls `tokio::sync::mpsc`).
#[cfg(feature = "bridge")]
pub use runtime::{Mailbox, MailboxClosed};
#[cfg(feature = "batcher")]
pub use runtime::TokioMailbox;
// Re-exports for tree_adapter (issue #1 item 8). The `tree` feature gates
// the module itself; the re-exports follow the same gate. `CycleError`
// derives `thiserror::Error` (a non-optional dep), so it is always available
// when `tree` is on.
#[cfg(feature = "tree")]
pub use tree_adapter::{CycleError, TreeAdapter, TreeNode};
// Re-exports for ffi, wasm modules will be added by parallel agents (tasks
// 6, 8) when they fill in their stub modules.

// Re-exports for the FFI hot-path API (issue #1 item 6). `NodeOp` and
// `NodeValue` are available with `bridge` alone (they have no grafeo deps);
// `apply_node_batch` additionally needs `grafeo` (calls `apply_loro_op`);
// `apply_loro_op_bytes` additionally needs `serde` (bincode 1.x requires
// `LoroOp: Deserialize`, which is derived under `serde`).
#[cfg(feature = "bridge")]
pub use ffi::{NodeOp, NodeValue};
#[cfg(all(feature = "bridge", feature = "grafeo"))]
pub use ffi::apply_node_batch;
#[cfg(all(feature = "bridge", feature = "grafeo", feature = "serde"))]
pub use ffi::apply_loro_op_bytes;

// Re-exports for the WASM JsValue error bridge (issue #1 item 12).
// `error_code` is target-agnostic (testable on native); `js_error` +
// `init_panic_hook` only exist on `target_family = "wasm"`. The
// `From<GrafeoLoroError> for JsValue` impl is brought into scope by
// `use crate::wasm::*` on WASM targets — it cannot be re-exported via
// `pub use` because trait impls are not nameable items; callers that
// want `?` to auto-convert in `#[wasm_bindgen]` fns must add
// `use grafeo_loro::wasm;` (the impl is in scope via the module).
#[cfg(feature = "wasm")]
pub use wasm::error_code;
#[cfg(all(feature = "wasm", target_family = "wasm"))]
pub use wasm::{init_panic_hook, js_error};

// Re-export native crates so raw handles are usable immediately (issue #1
// item 4: Onde receives the `LoroDoc` from `GrafeoLoroApp::doc()` and calls
// native Loro APIs directly via this re-export).
#[cfg(feature = "grafeo")]
pub use grafeo;
#[cfg(feature = "bridge")]
pub use loro;
