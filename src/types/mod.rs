//! Shared type vocabulary: ids, events, presence, values.
//!
//! These types are always available (no feature gate) because they form the
//! crate's public vocabulary. The `events` module references `grafeo::cdc`
//! types, so it is gated by `grafeo`.

#[cfg(feature = "bridge")]
pub mod events;
pub mod ids;
pub mod presence;
pub mod values;

#[cfg(all(feature = "bridge", feature = "grafeo"))]
pub use events::CdcEventWrapper;
#[cfg(feature = "bridge")]
pub use events::LoroOp;
pub use ids::{EdgeId, NodeId, PeerId};
pub use presence::PresencePayload;
pub use values::{GraphValue, LoroProperty};

/// Re-export of `grafeo_common::types::EpochId` for the epoch side-channel
/// echo-prevention set (`SyncEngine::bridge_origin_epochs`).
#[cfg(feature = "grafeo")]
pub use grafeo_common::types::EpochId;
