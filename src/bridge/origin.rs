//! Echo-prevention origin checks. Pure functions only.
//!
//! Two origin tags exist in the system: [`ORIGIN_GRAFEO_BRIDGE`] (set on Loro
//! transactions written by the Grafeo→Loro outbound worker) and
//! [`ORIGIN_LORO_BRIDGE`] (set on Grafeo transactions written by the
//! Loro→Grafeo inbound worker). An echo occurs when a mutation from one bridge
//! cycles back through the other bridge — these helpers detect that case.

use crate::constants::{ORIGIN_GRAFEO_BRIDGE, ORIGIN_LORO_BRIDGE};

/// True iff `origin` was produced by the Grafeo→Loro outbound bridge.
pub fn is_grafeo_bridge_origin(origin: &str) -> bool {
    let _ = (origin, ORIGIN_GRAFEO_BRIDGE);
    unimplemented!()
}

/// True iff `origin` was produced by the Loro→Grafeo inbound bridge.
pub fn is_loro_bridge_origin(origin: Option<&str>) -> bool {
    let _ = (origin, ORIGIN_LORO_BRIDGE);
    unimplemented!()
}

/// True iff `origin` is either `ORIGIN_GRAFEO_BRIDGE` or `ORIGIN_LORO_BRIDGE`.
pub fn is_bridge_origin(origin: &str) -> bool {
    let _ = (origin, ORIGIN_GRAFEO_BRIDGE, ORIGIN_LORO_BRIDGE);
    unimplemented!()
}

/// True iff `origin_a` and `origin_b` are both bridge origins AND differ —
/// i.e. one is `grafeo-bridge` and the other is `loro-bridge`. This is the
/// echo fingerprint: a mutation cycled from one bridge back through the other.
pub fn is_echo(origin_a: &str, origin_b: &str) -> bool {
    let _ = (origin_a, origin_b, ORIGIN_GRAFEO_BRIDGE, ORIGIN_LORO_BRIDGE);
    unimplemented!()
}
