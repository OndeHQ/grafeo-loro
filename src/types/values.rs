use serde::{Serialize, Deserialize};
use std::collections::HashMap;

use lorosurgeon::{
    Hydrate, Reconcile, Reconciler,
    reconcile::NoKey,
    error::{HydrateError, ReconcileError},
};

use crate::error::{GrafeoLoroError, Result};

/// Primitive property value bound to a LoroMap field.
///
/// Manual `Hydrate`/`Reconcile` impls (orchestrator decision, Devil Gap 2)
/// produce **bare** `LoroValue`s — `Bool(true)` ↔ `LoroValue::Bool(true)`,
/// `Float(3.14)` ↔ `LoroValue::Double(3.14)`, etc. The default
/// `#[derive(Hydrate, Reconcile)]` would emit a tagged-union LoroMap
/// (`{ "Bool": true }`) which doubles the wire size and breaks property
/// lookups. The manual impls are ~30 LOC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LoroProperty {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
}

impl Default for LoroProperty {
    fn default() -> Self {
        Self::Null
    }
}

// Manual `Hydrate` overrides each scalar dispatcher so the default
// `hydrate_value` dispatch falls through to bare-value construction.
// No tagged-union wrapping; no nested `LoroMap`.
impl Hydrate for LoroProperty {
    fn hydrate_null() -> std::result::Result<Self, HydrateError> {
        Ok(Self::Null)
    }
    fn hydrate_bool(b: bool) -> std::result::Result<Self, HydrateError> {
        Ok(Self::Bool(b))
    }
    fn hydrate_i64(i: i64) -> std::result::Result<Self, HydrateError> {
        Ok(Self::Integer(i))
    }
    fn hydrate_f64(f: f64) -> std::result::Result<Self, HydrateError> {
        Ok(Self::Float(f))
    }
    fn hydrate_string(s: &str) -> std::result::Result<Self, HydrateError> {
        Ok(Self::String(s.to_string()))
    }
}

// Manual `Reconcile` writes each variant as a bare scalar via the
// `Reconciler` API (no map wrapping).
impl Reconcile for LoroProperty {
    type Key = NoKey;

    fn reconcile<R: Reconciler>(&self, r: R) -> std::result::Result<(), ReconcileError> {
        match self {
            Self::Null => r.null(),
            Self::Bool(b) => r.boolean(*b),
            Self::Integer(i) => r.i64(*i),
            Self::Float(f) => r.f64(*f),
            Self::String(s) => r.str(s),
        }
    }
}

/// Grafeo-side value: superset of `LoroProperty` plus recursive `Map`/`List` and offloaded `Vector`.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphValue {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Vector(std::sync::Arc<[f32]>),
    Map(HashMap<String, GraphValue>),
    List(Vec<GraphValue>),
}

/// Converts a raw `LoroValue` into a `GraphValue`, recursing into Map/List and rejecting Binary/Container.
pub fn lval_to_gval(val: loro::LoroValue) -> Result<GraphValue> {
    let _ = val;
    // TODO L3: recursive mapping per architecture doc §5 —
    //   Null → GraphValue::Null,
    //   Bool(b) → GraphValue::Bool(b),
    //   I64(i) → GraphValue::Integer(i),
    //   Double(f) → GraphValue::Float(f),
    //   String(s) → GraphValue::String(s),
    //   Map(m) → GraphValue::Map(HashMap from entries, recursing),
    //   List(l) → GraphValue::List(Vec, recursing),
    //   Binary | Container → Err(UnsupportedLoroType).
    Err(GrafeoLoroError::UnsupportedLoroType(
        "lval_to_gval body pending L3".to_string(),
    ))
}
