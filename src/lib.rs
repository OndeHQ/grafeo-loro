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
//! ### Issue #4: `OfflineOpQueue` + `EpochTracker` (WASM-accessible)
//!
//! Issue #4 factored `OfflineOpQueue`, `LineageEpoch`, `EpochMismatchError`,
//! and the new standalone `EpochTracker` out of `src/bridge/sync_engine.rs`
//! into `src/bridge/queue.rs`. The new module is gated **only** by
//! `feature = "bridge"` — no `batcher` (tokio), no `grafeo` (native
//! ONNX/ort), no `telemetry` (opentelemetry native). Browser consumers on
//! `wasm32-unknown-unknown` can name these types directly via
//! `grafeo_loro::OfflineOpQueue` / `grafeo_loro::EpochTracker`.
//!
//! ### `bridge` queue + epoch tracker sub-API
//!
//! | Type                 | Feature gate | Purpose                                  |
//! |----------------------|--------------|------------------------------------------|
//! | `OfflineOpQueue`     | `bridge`     | Cap-bounded FIFO of serialized LoroOps  |
//! | `LineageEpoch`       | `bridge`     | `u64` type alias for the epoch counter   |
//! | `EpochMismatchError` | `bridge`     | Error raised on remote/local epoch drift |
//! | `EpochTracker`       | `bridge`     | Standalone `Arc<AtomicU64>` epoch store  |
//!
//! ## Onde's recommended feature set
//!
//! ```toml
//! grafeo-loro = { version = "0.4", default-features = false, features = ["bridge", "batcher", "compression", "tree"] }
//! ```
//!
//! ## Minimal WASM smoke test
//!
//! ```toml
//! grafeo-loro = { version = "0.4", default-features = false, features = ["bridge", "tree", "wasm"] }
//! ```
//!
//! ## MSRV
//!
//! Rust 1.80+ (verified via `cargo msrv` against the dep tree).
//!
//! ## Issue #3 sub-issue → feature mapping
//!
//! Issue #3 ("Browser WASM consumers Support") has 10 sub-issues. Their
//! feature gates are:
//!
//! | Sub-issue | Topic                              | Feature gate                              |
//! |-----------|------------------------------------|-------------------------------------------|
//! | #1        | WASM target compile + binary size  | `wasm` + `bridge,tree,compression`        |
//! | #2        | Trait-abstracted runtime (Mailbox) | `bridge` (trait); `batcher` (Tokio impl)  |
//! | #3        | LoroDoc ownership API              | `bridge,storage,grafeo,batcher`           |
//! | #4        | Merge semantics                    | (no feature — use `doc.import()`)         |
//! | #5        | Offline op-queue + lineage epoch   | `bridge` (factored out in issue #4)       |
//! | #6        | FTS + SAB + shadow commits         | `fts`, `sab`, `shadow`                    |
//! | #7        | Graph invariants (cycle, root)     | `bridge,tree`                             |
//! | #8        | Presence (ephemeral overlay)       | `bridge` (always available with bridge)   |
//! | #9        | Storage backend trait              | `storage`                                 |
//! | #10       | Observability hooks                | `observability`                           |
//!
//! ## No `merge` / `awareness` / `persistence` feature
//!
//! If you saw a feature flag named `merge`, `awareness`, or `persistence`
//! in a downstream consumer's Cargo.toml, it was invented — grafeo-loro
//! has never declared these. Enabling them now produces a `compile_error!`
//! pointing at the correct alternative:
//!
//! | Invented name   | Correct alternative                                            |
//! |-----------------|----------------------------------------------------------------|
//! | `merge`         | `doc.import(other.export(loro::ExportFormat::Snapshot))`       |
//! | `awareness`     | `presence` module (always available with `bridge`)             |
//! | `persistence`   | `storage` feature (`StorageBackend` trait + `InMemoryStorage`) |
//!
//! ## WASM binary size
//!
//! With the full WASM-safe feature set (`bridge,tree,compression,wasm,serde,
//! fts,sab,shadow,observability`) and `opt-level="z"` + `lto=true` +
//! `codegen-units=1` + `strip=true`, the raw `.wasm` (before `wasm-opt -Oz`)
//! is approximately **2.26 MB**. After `wasm-opt -Oz` (configured in
//! `[package.metadata.wasm-pack.profile.release]`), it shrinks ~15-20%.
//!
//! For size-constrained consumers (e.g. <1.8 MB budget), drop `fts` (inverted
//! index) and `shadow` (Git DAG shadow commits) first — they together add
//! ~400 KB. The minimal smoke feature set `bridge,tree,wasm` produces a
//! ~800 KB `.wasm` after `wasm-opt -Oz`.

// ============================================================================
// Always-on core: error, constants, config, types
// ============================================================================
// These modules have no heavy deps; they form the shared vocabulary of the
// crate and are always available regardless of feature selection.

// ============================================================================
// Issue #4 secondary finding #2: fail-loud on invented feature names
// ============================================================================
//
// Issue #4 reports that downstream consumers have been observed inventing
// feature names like `merge`, `awareness`, `persistence` in their Cargo.toml
// and shipping stubs referencing them. The crate has never declared these
// features — older Cargo silently accepted unknown `--features` flags (no-op),
// newer Cargo warns but does not error.
//
// To fail loud and clear, we declare them as empty feature stubs in
// Cargo.toml and emit a `compile_error!` here pointing the consumer at the
// correct alternative. This is a breaking change (consumers who relied on
// the silent no-op behavior will now get a hard error) — intentional per
// issue #4's "no backward compat" mandate.

#[cfg(feature = "merge")]
compile_error!(
    "grafeo-loro has no `merge` feature. To merge two LoroDocs, call \
     `doc.import(other_doc.export(loro::ExportFormat::Snapshot))` — see \
     the Loro CRDT docs. The `merge` feature name was invented by a \
     downstream consumer stub; remove it from your Cargo.toml."
);

#[cfg(feature = "awareness")]
compile_error!(
    "grafeo-loro has no `awareness` feature. Presence/awareness is provided \
     by the `presence` module, which is always available when the `bridge` \
     feature is enabled — no separate feature flag needed. The `awareness` \
     feature name was invented by a downstream consumer stub; remove it \
     from your Cargo.toml."
);

#[cfg(feature = "persistence")]
compile_error!(
    "grafeo-loro has no `persistence` feature. Persistence is provided by \
     the `storage` feature (StorageBackend trait + InMemoryStorage reference \
     impl). Enable `storage` in your Cargo.toml. The `persistence` feature \
     name was invented by a downstream consumer stub; remove it."
);

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

#[cfg(all(
    feature = "bridge",
    feature = "storage",
    feature = "grafeo",
    feature = "batcher"
))]
pub mod app;

// ============================================================================
// Re-exports
// ============================================================================

#[cfg(all(
    feature = "bridge",
    feature = "storage",
    feature = "grafeo",
    feature = "batcher"
))]
pub use app::GrafeoLoroApp;
#[cfg(all(feature = "batcher", feature = "grafeo", feature = "telemetry"))]
pub use bridge::sync_engine::InboundMsg;
#[cfg(feature = "bridge")]
pub use bridge::BridgeMaps;
#[cfg(all(feature = "batcher", feature = "grafeo", feature = "telemetry"))]
pub use bridge::SyncEngine;

// Issue #4: OfflineOpQueue + EpochTracker are reachable from WASM with
// `feature = "bridge"` only (no batcher/grafeo/telemetry needed).
// Factored out of `src/bridge/sync_engine.rs` in issue #4 so browser
// consumers on `wasm32-unknown-unknown` can name these types directly
// from the crate root.
#[cfg(feature = "bridge")]
pub use bridge::queue::{EpochMismatchError, EpochTracker, LineageEpoch, OfflineOpQueue};

// Issue #4: #[wasm_bindgen] JS-facing wrappers. Available only on actual
// WASM targets (the wrappers themselves are `#![cfg(target_family = "wasm")]`
// inside src/wasm/queue.rs).
#[cfg(all(feature = "wasm", target_family = "wasm"))]
pub use wasm::queue::{WasmEpochTracker, WasmOfflineOpQueue};

#[cfg(feature = "compression")]
pub use compression::{CompressedPayload, LoroDocCompressionExt};
pub use config::{CompressionType, SsotMode};
pub use error::GrafeoLoroError;
pub use error::Result;
#[cfg(all(feature = "grafeo", not(target_family = "wasm")))]
pub use hydration::hydrate_grafeo;
#[cfg(all(feature = "grafeo", feature = "parallel", not(target_family = "wasm")))]
pub use hydration::parallel_hydrate_grafeo;
#[cfg(all(feature = "grafeo", not(target_family = "wasm")))]
pub use hydration::vector::generate_local_embedding;
#[cfg(all(feature = "grafeo", not(target_family = "wasm")))]
pub use hydration::VectorOffloadManager;
#[cfg(feature = "storage")]
pub use storage::InMemoryStorage;
#[cfg(feature = "storage")]
pub use storage::StorageBackend;
// Trait-abstracted async runtime (issue #1 item 2). The `Mailbox` trait +
// `MailboxClosed` error are available with `bridge` alone; the tokio-backed
// `TokioMailbox` impl requires `batcher` (which pulls `tokio::sync::mpsc`).
#[cfg(feature = "batcher")]
pub use runtime::TokioMailbox;
#[cfg(feature = "bridge")]
pub use runtime::{Mailbox, MailboxClosed};
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
#[cfg(all(feature = "bridge", feature = "grafeo", feature = "serde"))]
pub use ffi::apply_loro_op_bytes;
#[cfg(all(feature = "bridge", feature = "grafeo"))]
pub use ffi::apply_node_batch;
#[cfg(feature = "bridge")]
pub use ffi::{NodeOp, NodeValue};

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
