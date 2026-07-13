use crate::types::values::LoroProperty;
use lorosurgeon::{Hydrate, Reconcile};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct VertexEntity {
    pub labels: Vec<String>,
    pub properties: HashMap<String, LoroProperty>,

    #[loro(text)]
    pub description: String,
}
