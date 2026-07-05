use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LoroProperty {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum GraphValue {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Vector(std::sync::Arc<[f32]>),
    Map(HashMap<String, GraphValue>),
}

pub fn lval_to_gval(val: loro::LoroValue) -> GraphValue {
    // Maps LoroValue -> GraphValue
    unimplemented!()
}