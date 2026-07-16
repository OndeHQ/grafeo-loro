use super::PeerId;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Ephemeral presence payload — never persisted.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct PresencePayload {
    pub peer_id: PeerId,
    pub active_node: Option<String>,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub last_active_ts: u64,
}

// Issue #3 sub-issue 8: node-level presence (bound to VertexId, not just doc).

/// Cursor position attached to a specific vertex (issue #3 sub-issue 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CursorPos {
    pub offset: u32,
    pub line: u32,
    pub col: u32,
}

/// Selection range attached to a specific vertex (issue #3 sub-issue 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SelectionRange {
    pub start: u32,
    pub end: u32,
}

/// Node-level presence (issue #3 sub-issue 8). Bound to a specific
/// `vertex_id`, not just the doc. Replaces JS-polled awareness state.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodePresence {
    pub peer_id: String,
    pub vertex_id: String,
    pub cursor: Option<CursorPos>,
    pub selection: Option<SelectionRange>,
    pub last_seen_ms: u64,
}
