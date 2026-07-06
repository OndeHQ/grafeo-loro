use crate::error::{GrafeoLoroError, Result};
use crate::types::presence::PresencePayload;
use tracing::instrument;

/// `%EPH` magic bytes (architecture §12).
const EPH_MAGIC: &[u8; 4] = b"%EPH";

/// Message type for presence payloads (architecture §12; future msg_types reserved).
const EPH_MSG_TYPE_PRESENCE: u8 = 0x01;

/// Decoded `%EPH` envelope (architecture §12).
#[derive(Debug, Clone, PartialEq)]
pub struct EphEnvelope {
    pub room_id: String,
    pub payload: PresencePayload,
}

/// Ephemeral presence manager over a WebSocket channel. Never persists state.
pub struct PresenceManager {
    room_id: String,
    // WebSocket connection state — added in Phase 5+ when broadcast is implemented
}

impl PresenceManager {
    /// Construct for a given room id. Pure constructor — stores the room_id for
    /// later use by `broadcast` (no socket connection in this phase).
    pub fn new(room_id: String) -> Self {
        Self { room_id }
    }

    /// Broadcast a presence payload to all peers in the room.
    // NOTE: body unimplemented!() — T1 excluded per user; span fires then panics
    #[instrument(skip(self, payload), name = "presence_broadcast", level = "info")]
    pub async fn broadcast(&self, payload: PresencePayload) -> Result<()> {
        let _ = payload;
        Err(GrafeoLoroError::NotYetImplemented(format!(
            "PresenceManager::broadcast: no WebSocket transport wired for room {}",
            self.room_id
        )))
    }

    /// Parse an `%EPH`-prefixed binary envelope into an [`EphEnvelope`].
    ///
    /// Wire format (architecture §12; `VarString = u16 LE length + UTF-8 bytes`):
    /// `[magic:4][room_id_len:u16 LE][room_id:UTF-8][msg_type:u8][serde_json payload]`.
    // NOTE: body unimplemented!() — T1 excluded per user; span fires then panics
    #[instrument(skip(bytes), name = "parse_eph_envelope", level = "debug")]
    pub fn parse_eph_envelope(bytes: &[u8]) -> Result<EphEnvelope> {
        if bytes.len() < EPH_MAGIC.len() {
            return Err(GrafeoLoroError::InvalidEnvelope(format!(
                "buffer too short: {} bytes",
                bytes.len()
            )));
        }
        if &bytes[0..EPH_MAGIC.len()] != EPH_MAGIC {
            return Err(GrafeoLoroError::InvalidEnvelope(format!(
                "bad magic: {:?}",
                &bytes[0..EPH_MAGIC.len()]
            )));
        }
        if bytes.len() < EPH_MAGIC.len() + 2 {
            return Err(GrafeoLoroError::InvalidEnvelope(
                "truncated: missing room_id_len".into(),
            ));
        }
        let room_id_len = u16::from_le_bytes([
            bytes[EPH_MAGIC.len()],
            bytes[EPH_MAGIC.len() + 1],
        ]) as usize;
        let room_id_start = EPH_MAGIC.len() + 2;
        let room_id_end = room_id_start + room_id_len;
        if bytes.len() < room_id_end + 1 {
            return Err(GrafeoLoroError::InvalidEnvelope(format!(
                "room_id segment truncated: need {} bytes, have {}",
                room_id_end + 1,
                bytes.len()
            )));
        }
        let room_id = std::str::from_utf8(&bytes[room_id_start..room_id_end])
            .map_err(|e| GrafeoLoroError::InvalidEnvelope(format!("bad room_id utf8: {e}")))?
            .to_string();
        let msg_type = bytes[room_id_end];
        if msg_type != EPH_MSG_TYPE_PRESENCE {
            return Err(GrafeoLoroError::InvalidEnvelope(format!(
                "unsupported msg_type: 0x{msg_type:02x}"
            )));
        }
        let payload_bytes = &bytes[room_id_end + 1..];
        let payload: PresencePayload = serde_json::from_slice(payload_bytes).map_err(|e| {
            GrafeoLoroError::InvalidEnvelope(format!("serde: {e}"))
        })?;
        Ok(EphEnvelope { room_id, payload })
    }

    /// Build an `%EPH`-prefixed binary envelope from a room_id + payload.
    ///
    /// Wire format (architecture §12; `VarString = u16 LE length + UTF-8 bytes`):
    /// `[magic:4][room_id_len:u16 LE][room_id:UTF-8][msg_type:u8][serde_json payload]`.
    // NOTE: body unimplemented!() — T1 excluded per user; span fires then panics
    #[instrument(skip(payload), fields(room_id = %room_id), name = "build_eph_envelope", level = "debug")]
    pub fn build_eph_envelope(room_id: &str, payload: &PresencePayload) -> Result<Vec<u8>> {
        let room_id_bytes = room_id.as_bytes();
        if room_id_bytes.len() > u16::MAX as usize {
            return Err(GrafeoLoroError::InvalidEnvelope(format!(
                "room_id too long: {} bytes (max {})",
                room_id_bytes.len(),
                u16::MAX
            )));
        }
        let json = serde_json::to_vec(payload).map_err(|e| {
            GrafeoLoroError::InvalidEnvelope(format!("serde_json encode: {e}"))
        })?;
        let mut buf =
            Vec::with_capacity(EPH_MAGIC.len() + 2 + room_id_bytes.len() + 1 + json.len());
        buf.extend_from_slice(EPH_MAGIC);
        buf.extend_from_slice(&(room_id_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(room_id_bytes);
        buf.push(EPH_MSG_TYPE_PRESENCE);
        buf.extend_from_slice(&json);
        Ok(buf)
    }
}
