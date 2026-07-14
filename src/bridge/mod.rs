//! Bridge: bidirectional Loro↔Grafeo sync types + id-mapping.
//!
//! Issue #1: bridge module is gated by the `bridge` feature. The
//! `MutationBatcher` requires `batcher` (pulls `tokio`). The `SyncEngine`
//! itself requires `batcher` because it owns the tokio MPSC channels. The
//! `apply_loro_op` translator requires `grafeo` because it calls
//! `Session::create_node_with_props` etc.
//!
//! When `bridge` is on but `grafeo`/`batcher` are off, only `BridgeMaps`
//! and the `origin` helpers are available — this is the WASM-friendly
//! surface for Onde to plug its own runtime into.

pub mod grafeo_tx;
pub mod origin;

#[cfg(feature = "batcher")]
pub mod batcher;
#[cfg(feature = "batcher")]
pub mod sync_engine;

// Re-exports for in-crate ergonomic access (`use crate::bridge::SyncEngine`
// instead of `use crate::bridge::sync_engine::SyncEngine`). The `bridge`
// module itself is gated by the `bridge` feature in `src/lib.rs`; these
// re-exports are reachable whenever `bridge` is on.
#[cfg(feature = "grafeo")]
pub use grafeo_tx::apply_loro_op;
pub use grafeo_tx::BridgeMaps;
#[cfg(feature = "batcher")]
pub use sync_engine::SyncEngine;
