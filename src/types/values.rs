use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use lorosurgeon::{Hydrate, Reconcile};

/// Primitive property value bound to a LoroMap field via `lorosurgeon` (no containers).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Hydrate, Reconcile)]
#[serde(untagged)]
pub enum LoroProperty {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
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
pub fn lval_to_gval(val: loro::LoroValue) -> crate::error::Result<GraphValue> {
    let _ = val;
    unimplemented!()
}