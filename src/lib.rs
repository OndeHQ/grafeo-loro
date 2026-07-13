pub mod app;
pub mod config;
pub mod constants;
pub mod error;

// Internal bridge logic is private to prevent API leak.
// Public types from the bridge are re-exported below for ergonomics.
mod bridge;
pub mod compression;
pub mod hydration;
pub mod presence;
pub mod schema;
pub mod storage;
pub mod telemetry;
pub mod types;

pub use app::GrafeoLoroApp;
pub use config::{CompressionType, SsotMode};
pub use error::GrafeoLoroError;
pub use storage::StorageBackend;

// Re-export native crates so raw handles are usable immediately.
// Users call native Grafeo / Loro APIs directly via `app.grafeo_db()` /
// `app.loro_doc()`; the bridge syncs transparently in the background.
pub use grafeo;
pub use loro;

// Bridge types re-exported for advanced introspection + embedded scenarios.
// The `bridge` module itself is private — users interact via raw handles and
// the bridge runs invisibly.
pub use bridge::{BridgeMaps, SyncEngine};
pub use bridge::sync_engine::InboundMsg;

// DEVIL m3: crate-root re-export of compression public API for Phase 4 storage ergonomics
// (`use grafeo_loro::{CompressedPayload, LoroDocCompressionExt}` vs the longer `compression::` path).
pub use compression::{CompressedPayload, LoroDocCompressionExt};
// DEVIL m1: crate-root re-export of parallel_hydrate_grafeo for Phase 4 storage ergonomics
// (`use grafeo_loro::parallel_hydrate_grafeo` vs the longer `hydration::parallel_hydrate_grafeo` path).
pub use hydration::parallel_hydrate_grafeo;
// P3T3-L2 m2: crate-root re-export of generate_local_embedding for external visibility
// (matches P3T1-L1 m3 + P3T2-L2 m1 precedent; `pub` stub reachable from `tests/unit/`).
pub use hydration::vector::generate_local_embedding;
// P3T4-L2 m5: crate-root re-export of VectorOffloadManager for Phase 4+ caller ergonomics
// (`use grafeo_loro::VectorOffloadManager` vs the longer `hydration::VectorOffloadManager` path;
// matches P3T1-L1 m3 + P3T2-L2 m1 + P3T3-L2 m2 precedent).
pub use hydration::VectorOffloadManager;
