pub mod app;
pub mod config;
pub mod error;
pub mod constants;

pub mod types;
pub mod bridge;
pub mod schema;
pub mod compression;
pub mod hydration;
pub mod storage;
pub mod presence;
pub mod telemetry;

pub use app::GrafeoLoroApp;
pub use config::{SsotMode, CompressionType, AppConfig};
pub use error::GrafeoLoroError;
pub use storage::StorageBackend;
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