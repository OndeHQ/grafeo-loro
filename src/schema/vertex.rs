use std::collections::HashMap;
use lorosurgeon::{Hydrate, Reconcile};
use crate::types::values::LoroProperty;

#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct VertexEntity {
    pub labels: Vec<String>,
    pub properties: HashMap<String, LoroProperty>,
    
    #[loro(text)]
    pub description: String,
}