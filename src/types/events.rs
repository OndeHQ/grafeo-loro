use std::collections::HashMap;
use super::{NodeId, GraphValue};

#[derive(Debug, Clone)]
pub enum LoroOp {
    UpsertNode { 
        id: NodeId, 
        properties: HashMap<String, GraphValue> 
    },
    UpsertEdge { 
        src: NodeId, 
        dst: NodeId, 
        label: String,
        properties: HashMap<String, GraphValue>
    },
    DeleteNode { 
        id: NodeId 
    },
    DeleteEdge { 
        src: NodeId, 
        dst: NodeId, 
        label: String 
    },
    TreeMove {
        node_id: NodeId,
        old_parent: NodeId,
        new_parent: NodeId,
    },
}

#[derive(Debug, Clone)]
pub struct CdcEventWrapper {
    pub origin: Option<String>,
    pub payload: grafeo::cdc::CdcEvent,
}