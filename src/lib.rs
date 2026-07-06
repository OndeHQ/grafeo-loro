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