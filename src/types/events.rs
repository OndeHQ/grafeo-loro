use std::collections::HashMap;
use super::{NodeId, GraphValue};

/// Translated Loro subscriber diff destined for the inbound batcher / worker.
#[derive(Debug, Clone)]
pub enum LoroOp {
    /// Insert or update a vertex with the given property map.
    UpsertNode {
        id: NodeId,
        properties: HashMap<String, GraphValue>,
    },
    /// Insert or update an edge with label + property map.
    UpsertEdge {
        src: NodeId,
        dst: NodeId,
        label: String,
        properties: HashMap<String, GraphValue>,
    },
    /// Remove a vertex by id.
    DeleteNode {
        id: NodeId,
    },
    /// Remove an edge by (src, dst, label).
    DeleteEdge {
        src: NodeId,
        dst: NodeId,
        label: String,
    },
    /// Tree reparenting: delete old `CHILD` edge, insert new one.
    TreeMove {
        node_id: NodeId,
        old_parent: NodeId,
        new_parent: NodeId,
    },
}

/// Grafeo `ChangeEvent` paired with its origin string (extracted from tx
/// metadata) for echo filtering on the outbound path.
#[derive(Debug, Clone)]
pub struct CdcEventWrapper {
    /// Origin tag from the Grafeo transaction metadata, if present.
    pub origin: Option<String>,
    /// The underlying Grafeo CDC change event.
    pub payload: grafeo::cdc::ChangeEvent,
}