//! Identity types used across the bridge.
//!
//! `NodeId` and `EdgeId` are re-exported from `grafeo` so the bridge talks to
//! the execution layer using its native id types — no `From`/`Into` shims, no
//! conversion overhead, no risk of two `u64` newtypes drifting. `PeerId`
//! stays local because grafeo has no CRDT peer concept.
//!
//! When the `grafeo` feature is OFF (e.g. WASM-only builds that don't need
//! the execution layer), `NodeId` / `EdgeId` fall back to local `u64`
//! newtypes so the rest of the crate still compiles.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "grafeo")]
pub use grafeo::{EdgeId, NodeId};

/// Fallback `NodeId` when `grafeo` is disabled. Same in-memory layout as
/// `grafeo::NodeId` (`u64`) so callers can `transmute` if needed, but we
/// don't expose that — re-enable `grafeo` instead.
#[cfg(not(feature = "grafeo"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeId(pub u64);

/// Fallback `EdgeId` when `grafeo` is disabled.
#[cfg(not(feature = "grafeo"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EdgeId(pub u64);

/// CRDT peer identifier. No grafeo equivalent — Loro-only concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PeerId(pub u64);
