use lorosurgeon::{Hydrate, Reconcile};
use grafeo::GrafeoDB;
use crate::types::ids::NodeId;

#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct OrderedCollection {
    #[loro(movable)]
    pub items: Vec<TreeNode>,
}

#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct TreeNode {
    #[key]
    pub node_id: String,
    pub title: String,
}

/// Translates Loro tree moves to Grafeo acyclic mutations.
pub fn sync_tree_move_to_grafeo(
    db: &GrafeoDB,
    node_id: NodeId,
    old_parent: NodeId,
    new_parent: NodeId,
) -> crate::error::Result<()> {
    let _ = (db, node_id, old_parent, new_parent);
    unimplemented!()
}