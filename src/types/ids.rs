//! Identity types used across the bridge.
//!
//! `NodeId` and `EdgeId` are re-exported from `grafeo` so the bridge talks to
//! the execution layer using its native id types — no `From`/`Into` shims, no
//! conversion overhead, no risk of two `u64` newtypes drifting. `PeerId`
//! stays local because grafeo has no CRDT peer concept.

use serde::{Serialize, Deserialize};

pub use grafeo::{NodeId, EdgeId};

/// CRDT peer identifier. No grafeo equivalent — Loro-only concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerId(pub u64);
