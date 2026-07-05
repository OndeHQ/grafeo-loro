//! Echo-prevention origin checks. Pure functions only.
//!
//! Two origin tags exist in the system: [`ORIGIN_GRAFEO_BRIDGE`] (set on Loro
//! transactions written by the Grafeo→Loro outbound worker) and
//! [`ORIGIN_LORO_BRIDGE`] (set on Grafeo transactions written by the
//! Loro→Grafeo inbound worker — advisory only; see module doc on
//! [`crate::bridge::sync_engine`] for the epoch side-channel that actually
//! prevents Grafeo→Loro echo).
//!
//! The Loro→Grafeo path uses `is_grafeo_bridge_origin` in the Loro
//! subscriber handler to filter echoes of our own outbound writes.
//! `is_loro_bridge_origin` is kept for symmetry / future use; the outbound
//! CDC poller currently uses the epoch side-channel instead of an origin
//! check (grafeo's `ChangeEvent` has no `origin` field — see Devil BLOCKER B2).

use crate::constants::{ORIGIN_GRAFEO_BRIDGE, ORIGIN_LORO_BRIDGE};

/// True iff `origin` was produced by the Grafeo→Loro outbound bridge.
pub fn is_grafeo_bridge_origin(origin: &str) -> bool {
    origin == ORIGIN_GRAFEO_BRIDGE
}

/// True iff `origin` was produced by the Loro→Grafeo inbound bridge.
///
/// Currently unused on the outbound path (the epoch side-channel replaces
/// it per Devil BLOCKER B2). Kept for symmetry; the Plenger hunter may flag
/// it as dead code — if so, delete it.
pub fn is_loro_bridge_origin(origin: Option<&str>) -> bool {
    origin == Some(ORIGIN_LORO_BRIDGE)
}
