//! Ephemeral presence: WebSocket-channel %EPH envelope.
//!
//! Independent of bridge/grafeo. The `webrtc` feature wires a transport
//! (native only). The `PresenceManager::broadcast` method returns
//! `NotYetImplemented` until a transport is wired.

pub mod socket;
pub use socket::{EphEnvelope, PresenceManager};
