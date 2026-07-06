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
///
/// Single-transaction reparent: delete the `(node_id → old_parent)` edge,
/// insert the `(node_id → new_parent)` edge (both with label
/// [`TREE_EDGE_LABEL`](crate::constants::TREE_EDGE_LABEL)), commit atomically.
/// Cycles are rejected up-front via [`would_create_cycle`] because Grafeo
/// 0.5.42 has no native graph-edge acyclicity enforcement (verified P2T2-L1).
///
/// # Errors
///
/// - [`GrafeoLoroError::TreeMoveCreatesCycle`] if `new_parent` is `node_id`
///   itself or a descendant of `node_id`.
/// - [`GrafeoLoroError::Grafeo`] if the underlying session transaction fails
///   (write-write conflict, SSI violation, etc.).
///
/// Grafeo Session API (verified against `grafeo-engine-0.5.42/src/`):
/// - `GrafeoDB::session` — `database/mod.rs:1663` (`&self -> Session`)
/// - `Session::begin_transaction` — `session/mod.rs:3883` (`&mut self -> Result<()>`)
/// - `Session::create_edge` — `session/mod.rs:4935` (`&self, NodeId, NodeId, &str -> EdgeId`; infallible)
/// - `Session::delete_edge` — `session/mod.rs:5092` (`&self, EdgeId -> bool`; returns `false` if edge absent)
/// - `Session::get_neighbors_outgoing_by_type` — `session/mod.rs` (after 5237; for cycle BFS)
/// - `Session::prepare_commit` — `session/mod.rs:4496` (`&mut self -> Result<PreparedCommit<'_>>`)
/// - `PreparedCommit::set_metadata` — `transaction/prepared.rs:108` (advisory; dropped on commit per Devil Gap 1)
/// - `PreparedCommit::commit` — `transaction/prepared.rs:124` (`self -> Result<EpochId>`)
pub fn sync_tree_move_to_grafeo(
    db: &GrafeoDB,
    node_id: NodeId,
    old_parent: NodeId,
    new_parent: NodeId,
) -> crate::error::Result<()> {
    let _ = (db, node_id, old_parent, new_parent);
    // TODO(P2T2-L3): Pre-check cycle: if would_create_cycle(db, node_id, new_parent)
    //                 return Err(GrafeoLoroError::TreeMoveCreatesCycle { node_id, new_parent }).
    // TODO(P2T2-L3): let mut session = db.session();
    // TODO(P2T2-L3): session.begin_transaction()?;
    // TODO(P2T2-L3): Resolve the existing (node_id → old_parent) EdgeId by walking
    //                 session.get_neighbors_outgoing_by_type(node_id, TREE_EDGE_LABEL)
    //                 and matching dst == old_parent; if found, session.delete_edge(eid).
    //                 (delete_edge returns bool — log false as "edge already absent" warning.)
    // TODO(P2T2-L3): If new_parent != old_parent (idempotent-noop guard),
    //                 session.create_edge(node_id, new_parent, TREE_EDGE_LABEL).
    //                 Direction is child→parent per apply_tree_move (src/bridge/grafeo_tx.rs:200-206).
    // TODO(P2T2-L3): let mut prepared = session.prepare_commit()?;
    // TODO(P2T2-L3): prepared.set_metadata("origin", ORIGIN_LORO_BRIDGE); // advisory, dropped on commit
    // TODO(P2T2-L3): prepared.commit()?; // -> EpochId
    // TODO(P2T2-L3): Re-verify acyclicity post-commit (defensive) — see Devil open question about
    //                 concurrent peer moves invalidating the pre-check.
    Err(crate::error::GrafeoLoroError::Bridge(
        "sync_tree_move_to_grafeo not yet implemented".into(),
    ))
}

/// BFS upward from `new_parent` along `TREE_EDGE_LABEL` edges looking for
/// `node_id`. Returns `true` if `node_id` is reachable, meaning the proposed
/// move would create a cycle.
///
/// Edge direction is child→parent (src=child, dst=parent) per
/// `apply_tree_move` (`src/bridge/grafeo_tx.rs:200-206`); "upward" therefore
/// means following `get_neighbors_outgoing_by_type(cur, TREE_EDGE_LABEL)`.
///
/// Grafeo 0.5.42 source verified (P2T2-L1) to have NO native graph-edge
/// acyclicity enforcement: only `catalog::resolved_node_type`
/// (`catalog/mod.rs:1349`) cycle-checks schema type inheritance, and
/// `procedures::has_negative_cycle` (`procedures.rs:831`) is a Bellman-Ford
/// query procedure — neither constrains user edges at commit time.
#[allow(dead_code)] // wired by P2T2-L3 in sync_tree_move_to_grafeo pre-check
fn would_create_cycle(db: &GrafeoDB, node_id: NodeId, new_parent: NodeId) -> bool {
    let _ = (db, node_id, new_parent);
    // TODO(P2T2-L3): walk parent chain via session.get_neighbors_outgoing_by_type;
    //                 return true iff node_id appears in the ancestor set of new_parent
    //                 (or iff new_parent == node_id — direct self-loop).
    todo!("P2T2-L3: implement cycle BFS")
}

#[cfg(test)]
mod tests {
    // Unit tests for sync_tree_move_to_grafeo live in tests/unit/tree_move.rs
    // (separate test crate, matches Phase 2 Task 1 pattern).
}
