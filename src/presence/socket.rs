use crate::error::Result;
use crate::types::presence::PresencePayload;
use tracing::instrument;

/// Ephemeral presence manager over a WebSocket channel. Never persists state.
pub struct PresenceManager {
    // TODO: Phase 6 T1 — wire `room_id` into broadcast() once body is implemented
    #[allow(
        dead_code,
        reason = "Phase 6 T1 excluded by user; field needed once broadcast_presence is implemented"
    )]
    room_id: String,
    // WebSocket connection state
}

impl PresenceManager {
    /// Construct for a given room id.
    pub fn new(room_id: String) -> Self {
        let _ = room_id;
        unimplemented!()
    }

    /// Broadcast a presence payload to all peers in the room.
    // NOTE: body unimplemented!() — T1 excluded per user; span fires then panics
    #[instrument(skip(self, payload), name = "presence_broadcast", level = "info")]
    pub async fn broadcast(&self, payload: PresencePayload) -> Result<()> {
        let _ = payload;
        unimplemented!()
    }

    /// Parse an `%EPH`-prefixed binary envelope into a payload.
    // NOTE: body unimplemented!() — T1 excluded per user; span fires then panics
    #[instrument(skip(bytes), name = "parse_eph_envelope", level = "debug")]
    pub fn parse_eph_envelope(bytes: &[u8]) -> Result<PresencePayload> {
        let _ = bytes;
        unimplemented!()
    }

    /// Build an `%EPH`-prefixed binary envelope from a payload.
    // NOTE: body unimplemented!() — T1 excluded per user; span fires then panics
    #[instrument(skip(payload), name = "build_eph_envelope", level = "debug")]
    pub fn build_eph_envelope(payload: &PresencePayload) -> Vec<u8> {
        let _ = payload;
        unimplemented!()
    }
}
