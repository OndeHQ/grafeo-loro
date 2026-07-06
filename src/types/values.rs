use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use lorosurgeon::{
    error::{HydrateError, ReconcileError},
    reconcile::NoKey,
    Hydrate, Reconcile, Reconciler,
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
#[derive(Default)]
pub enum LoroProperty {
    #[default]
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
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

// Ergonomic `From` impls so `with_property(key, value)` accepts bare
// primitives (P2T3-DEVIL m4). The 5 covered variants match `LoroProperty`'s
// subset — the strict-reject path in `VertexBuilder::commit` keeps the
// `Vector`/`Map`/`List` variants graph-only.
impl From<bool> for GraphValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<i64> for GraphValue {
    fn from(i: i64) -> Self {
        Self::Integer(i)
    }
}

impl From<f64> for GraphValue {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<String> for GraphValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for GraphValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

/// `LoroProperty` → `GraphValue` (scalar subset only; `LoroProperty` has no
/// `Vector`/`Map`/`List` variants). Used by `parallel_hydrate_grafeo` to convert
/// `VertexEntity::properties` (`HashMap<String, LoroProperty>`) into
/// `LoroOp::UpsertNode::properties` (`HashMap<String, GraphValue>`) — the 5
/// variants map 1:1. Inverse of `TryFrom<GraphValue> for LoroProperty` above.
impl From<LoroProperty> for GraphValue {
    fn from(p: LoroProperty) -> Self {
        match p {
            LoroProperty::Null => GraphValue::Null,
            LoroProperty::Bool(b) => GraphValue::Bool(b),
            LoroProperty::Integer(i) => GraphValue::Integer(i),
            LoroProperty::Float(f) => GraphValue::Float(f),
            LoroProperty::String(s) => GraphValue::String(s),
        }
    }
}

/// Total-on-scalar-subset conversion used by `VertexBuilder::commit` step 2 to
/// build a Loro-side `VertexEntity`. The scalar variants map 1:1; the
/// `Vector`/`Map`/`List` variants (which have no `LoroProperty` representation)
/// are rejected with [`GrafeoLoroError::UnsupportedLoroType`]. `commit()` step
/// 1 strictly rejects those variants BEFORE this call, so the `Err` arm is
/// defensive — but kept total so the conversion cannot silently drop data.
impl std::convert::TryFrom<GraphValue> for LoroProperty {
    type Error = GrafeoLoroError;

    fn try_from(v: GraphValue) -> std::result::Result<Self, Self::Error> {
        match v {
            GraphValue::Null => Ok(LoroProperty::Null),
            GraphValue::Bool(b) => Ok(LoroProperty::Bool(b)),
            GraphValue::Integer(i) => Ok(LoroProperty::Integer(i)),
            GraphValue::Float(f) => Ok(LoroProperty::Float(f)),
            GraphValue::String(s) => Ok(LoroProperty::String(s)),
            GraphValue::Vector(_) | GraphValue::Map(_) | GraphValue::List(_) => {
                Err(GrafeoLoroError::UnsupportedLoroType(format!(
                    "{v:?} has no LoroProperty representation (Vector/Map/List are graph-only)"
                )))
            }
        }
    }
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
        GraphValue::List(items) => GV::List(items.into_iter().map(gval_to_grafeo_value).collect()),
        GraphValue::Map(m) => {
            let mut bt: BTreeMap<grafeo_common::types::PropertyKey, grafeo::Value> =
                BTreeMap::new();
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
        // Exotic grafeo types (Timestamp/Date/Time/Duration/ZonedDatetime/Path/
        // GCounter/Bytes) collapse to Null for Phase 1 (YAGNI: no test
        // exercises them, no LoroValue equivalent exists for most). The
        // collapse is intentional — flagged by Hunter MINOR 9 — but a warn
        // log gives observability so silent data loss is at least visible.
        exotic => {
            tracing::warn!(
                grafeo_ty = ?exotic,
                "exotic grafeo type collapses to LoroValue::Null for Phase 1"
            );
            LV::Null
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lval_to_gval_scalars() {
        assert_eq!(
            lval_to_gval(loro::LoroValue::Null).unwrap(),
            GraphValue::Null
        );
        assert_eq!(
            lval_to_gval(loro::LoroValue::Bool(true)).unwrap(),
            GraphValue::Bool(true)
        );
        assert_eq!(
            lval_to_gval(loro::LoroValue::I64(42)).unwrap(),
            GraphValue::Integer(42)
        );
        assert_eq!(
            lval_to_gval(loro::LoroValue::Double(3.5)).unwrap(),
            GraphValue::Float(3.5)
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

        // Hunter MINOR 6: the test name promises Container coverage but the
        // original body only exercised Binary. Construct a root-Map container
        // id (the cheapest ContainerID variant) and verify the same rejection.
        let cid = loro::ContainerID::Root {
            name: "test_container".into(),
            container_type: loro::ContainerType::Map,
        };
        let ctr = loro::LoroValue::Container(cid);
        assert!(matches!(
            lval_to_gval(ctr),
            Err(GrafeoLoroError::UnsupportedLoroType(_))
        ));
    }

    #[test]
    fn gval_to_grafeo_maps_all_variants() {
        // Hunter MINOR 5: the original `gval_to_grafeo_roundtrip` test was
        // misnamed (it tested a 1-way mapping, not a roundtrip) and only
        // exercised 1 of 8 GraphValue variants. Renamed and expanded.
        assert_eq!(gval_to_grafeo_value(GraphValue::Null), grafeo::Value::Null);
        assert_eq!(
            gval_to_grafeo_value(GraphValue::Bool(true)),
            grafeo::Value::Bool(true)
        );
        assert_eq!(
            gval_to_grafeo_value(GraphValue::Integer(7)),
            grafeo::Value::Int64(7)
        );
        assert_eq!(
            gval_to_grafeo_value(GraphValue::Float(3.5)),
            grafeo::Value::Float64(3.5)
        );
        assert_eq!(
            gval_to_grafeo_value(GraphValue::String("hi".to_string())),
            grafeo::Value::String("hi".into())
        );
        let vec: std::sync::Arc<[f32]> = vec![1.0, 2.5, 3.75].into();
        assert_eq!(
            gval_to_grafeo_value(GraphValue::Vector(vec.clone())),
            grafeo::Value::Vector(vec)
        );
        // Nested list (recursive): GraphValue::List([Bool, Integer]) →
        // grafeo::Value::List([Bool, Int64]).
        let list_in = GraphValue::List(vec![GraphValue::Bool(false), GraphValue::Integer(11)]);
        let list_out =
            grafeo::Value::List(vec![grafeo::Value::Bool(false), grafeo::Value::Int64(11)].into());
        assert_eq!(gval_to_grafeo_value(list_in), list_out);
        // Nested map (recursive): {"k": Bool} → grafeo::Value::Map{"k": Bool}.
        let mut map_in = HashMap::new();
        map_in.insert("k".to_string(), GraphValue::Bool(true));
        let map_out = gval_to_grafeo_value(GraphValue::Map(map_in));
        let expected = grafeo::Value::Map(std::sync::Arc::new({
            let mut bt: BTreeMap<grafeo_common::types::PropertyKey, grafeo::Value> =
                BTreeMap::new();
            bt.insert("k".into(), grafeo::Value::Bool(true));
            bt
        }));
        assert_eq!(map_out, expected);
    }
}
