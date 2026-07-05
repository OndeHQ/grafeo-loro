use std::collections::HashMap;
use lorosurgeon::{Hydrate, Reconcile};
use crate::types::values::LoroProperty;

#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct EdgeEntity {
    pub label: String,
    pub src: String,
    pub dst: String,
    pub properties: HashMap<String, LoroProperty>,
}