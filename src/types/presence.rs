use super::PeerId;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Ephemeral presence payload — never persisted. Serialized via `serde_json`
/// when the `serde` feature is on; otherwise the caller serializes manually.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct PresencePayload {
    pub peer_id: PeerId,
    pub active_node: Option<String>,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub last_active_ts: u64,
}
