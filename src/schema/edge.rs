use crate::types::values::LoroProperty;
use lorosurgeon::{Hydrate, Reconcile};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct EdgeEntity {
    pub label: String,
    pub src: String,
    pub dst: String,
    pub properties: HashMap<String, LoroProperty>,
}
