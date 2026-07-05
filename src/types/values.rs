use serde::{Serialize, Deserialize};
use std::collections::{BTreeMap, HashMap};

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

/// Pure recursive `LoroValue → GraphValue` (architecture §5). Rejects `Binary`/`Container`.
pub fn lval_to_gval(val: loro::LoroValue) -> Result<GraphValue> {
    use loro::LoroValue as LV;
    Ok(match val {
        LV::Null => GraphValue::Null,
        LV::Bool(b) => GraphValue::Bool(b),
        LV::I64(i) => GraphValue::Integer(i),
        LV::Double(f) => GraphValue::Float(f),
        LV::String(s) => GraphValue::String(s.to_string()),
        LV::List(items) => {
            let v: Result<Vec<GraphValue>> = items.iter().cloned().map(lval_to_gval).collect();
            GraphValue::List(v?)
        }
        LV::Map(m) => {
            let mut out = HashMap::with_capacity(m.len());
            for (k, v) in m.iter() {
                out.insert(k.clone(), lval_to_gval(v.clone())?);
            }
            GraphValue::Map(out)
        }
        LV::Binary(_) => return Err(GrafeoLoroError::UnsupportedLoroType("Binary".into())),
        LV::Container(_) => return Err(GrafeoLoroError::UnsupportedLoroType("Container".into())),
    })
}

/// Pure `GraphValue → grafeo::Value` for the inbound apply path.
pub fn gval_to_grafeo_value(val: GraphValue) -> grafeo::Value {
    use grafeo::Value as GV;
    match val {
        GraphValue::Null => GV::Null,
        GraphValue::Bool(b) => GV::Bool(b),
        GraphValue::Integer(i) => GV::Int64(i),
        GraphValue::Float(f) => GV::Float64(f),
        GraphValue::String(s) => GV::String(s.into()),
        GraphValue::Vector(v) => GV::Vector(v),
        GraphValue::List(items) => {
            GV::List(items.into_iter().map(gval_to_grafeo_value).collect())
        }
        GraphValue::Map(m) => {
            let mut bt: BTreeMap<grafeo_common::types::PropertyKey, grafeo::Value> = BTreeMap::new();
            for (k, v) in m {
                bt.insert(k.into(), gval_to_grafeo_value(v));
            }
            GV::Map(std::sync::Arc::new(bt))
        }
    }
}

/// Pure `grafeo::Value → LoroValue` for the outbound worker. Exotic grafeo
/// variants (Timestamp/Date/Time/Duration/Path/Counter/Bytes) collapse to
/// `Null` — Phase 1 only round-trips the JSON-shaped subset shared with LoroValue.
pub fn grafeo_value_to_lval(val: &grafeo::Value) -> loro::LoroValue {
    use loro::LoroValue as LV;
    match val {
        grafeo::Value::Null => LV::Null,
        grafeo::Value::Bool(b) => LV::Bool(*b),
        grafeo::Value::Int64(i) => LV::I64(*i),
        grafeo::Value::Float64(f) => LV::Double(*f),
        grafeo::Value::String(s) => LV::String(s.as_str().into()),
        grafeo::Value::List(items) => {
            let v: Vec<loro::LoroValue> = items.iter().map(grafeo_value_to_lval).collect();
            LV::List(v.into())
        }
        grafeo::Value::Map(m) => {
            let mut out: HashMap<String, loro::LoroValue> = HashMap::with_capacity(m.len());
            for (k, v) in m.iter() {
                out.insert(k.as_str().to_string(), grafeo_value_to_lval(v));
            }
            LV::Map(out.into())
        }
        grafeo::Value::Vector(v) => {
            let list: Vec<loro::LoroValue> = v.iter().map(|f| LV::Double(f64::from(*f))).collect();
            LV::List(list.into())
        }
        // Exotic types collapse to Null for Phase 1 (YAGNI: no test exercises them).
        _ => LV::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lval_to_gval_scalars() {
        assert_eq!(lval_to_gval(loro::LoroValue::Null).unwrap(), GraphValue::Null);
        assert_eq!(
            lval_to_gval(loro::LoroValue::Bool(true)).unwrap(),
            GraphValue::Bool(true)
        );
        assert_eq!(
            lval_to_gval(loro::LoroValue::I64(42)).unwrap(),
            GraphValue::Integer(42)
        );
        assert_eq!(
            lval_to_gval(loro::LoroValue::Double(3.14)).unwrap(),
            GraphValue::Float(3.14)
        );
        assert_eq!(
            lval_to_gval(loro::LoroValue::String("hi".into())).unwrap(),
            GraphValue::String("hi".to_string())
        );
    }

    #[test]
    fn lval_to_gval_recursive() {
        let mut inner = HashMap::new();
        inner.insert("a".to_string(), loro::LoroValue::I64(1));
        let map = loro::LoroValue::Map(inner.into());
        let gv = lval_to_gval(map).unwrap();
        match gv {
            GraphValue::Map(m) => assert_eq!(m.get("a"), Some(&GraphValue::Integer(1))),
            other => panic!("expected Map, got {other:?}"),
        }

        let list = loro::LoroValue::List(vec![loro::LoroValue::Bool(false)].into());
        let gv = lval_to_gval(list).unwrap();
        match gv {
            GraphValue::List(v) => assert_eq!(v, vec![GraphValue::Bool(false)]),
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn lval_to_gval_rejects_binary_and_container() {
        let bin = loro::LoroValue::Binary(vec![1, 2, 3].into());
        assert!(matches!(
            lval_to_gval(bin),
            Err(GrafeoLoroError::UnsupportedLoroType(_))
        ));
    }

    #[test]
    fn gval_to_grafeo_roundtrip() {
        let gv = GraphValue::Integer(7);
        let grafeo_v = gval_to_grafeo_value(gv);
        assert_eq!(grafeo_v, grafeo::Value::Int64(7));
    }
}
