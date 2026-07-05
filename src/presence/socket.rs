use crate::types::presence::PresencePayload;
use crate::error::Result;

pub struct PresenceManager {
    room_id: String,
    // WebSocket connection state
}

impl PresenceManager {
    pub fn new(room_id: String) -> Self;
    pub async fn broadcast(&self, payload: PresencePayload) -> Result<()>;
    pub fn parse_eph_envelope(bytes: &[u8]) -> Result<PresencePayload>;
    pub fn build_eph_envelope(payload: &PresencePayload) -> Vec<u8>;
}