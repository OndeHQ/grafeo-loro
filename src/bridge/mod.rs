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
// Issue #4: `queue` is gated by `feature = "bridge"` only (inherited from
// the parent `bridge` module's gate in `src/lib.rs`). It holds the
// WASM-accessible `OfflineOpQueue` + `EpochTracker` + `LineageEpoch` +
// `EpochMismatchError` types — no tokio, no grafeo, no telemetry deps.
// Browser consumers on `wasm32-unknown-unknown` use this directly without
// enabling `batcher` (tokio mpsc) + `grafeo` (native ONNX/ort) +
// `telemetry` (opentelemetry native).
pub mod queue;

// `batcher` + `sync_engine` modules pull `grafeo` (GrafeoDB, Session API),
// `telemetry` (HealthProbe, MetricsRegistry, SharedTracer), and the full
// tokio runtime (`spawn`, `spawn_blocking`, `select!`). They are therefore
// gated by `batcher + grafeo + telemetry` — enabling `batcher` alone gives
// you the trait-abstracted `Mailbox<T>` + `TokioMailbox<T>` (in
// `crate::runtime`) without dragging in the grafeo execution layer.
// Issue #1 item 2 compliance: the Mailbox trait itself is available with
// `bridge` alone.
#[cfg(all(feature = "batcher", feature = "grafeo", feature = "telemetry"))]
pub mod batcher;
#[cfg(all(feature = "batcher", feature = "grafeo", feature = "telemetry"))]
pub mod sync_engine;

// Re-exports for in-crate ergonomic access (`use crate::bridge::SyncEngine`
// instead of `use crate::bridge::sync_engine::SyncEngine`). The `bridge`
// module itself is gated by the `bridge` feature in `src/lib.rs`; these
// re-exports are reachable whenever `bridge` is on.
#[cfg(feature = "grafeo")]
pub use grafeo_tx::apply_loro_op;
pub use grafeo_tx::BridgeMaps;
// Issue #4: re-export the WASM-accessible queue types from `bridge::queue`
// so callers can `use grafeo_loro::bridge::{OfflineOpQueue, EpochTracker,
// ...}` without reaching into the submodule. Reachable whenever `bridge`
// is on — no `batcher`/`grafeo`/`telemetry` gate (that's the whole point
// of issue #4).
pub use queue::{EpochMismatchError, EpochTracker, LineageEpoch, OfflineOpQueue};
#[cfg(all(feature = "batcher", feature = "grafeo", feature = "telemetry"))]
pub use sync_engine::SyncEngine;
