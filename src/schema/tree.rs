//! Loro → Grafeo tree reparenting bridge.
//!
//! # Known Limitation (P2T2-DEVIL Q7/R7)
//!
//! `sync_tree_move_to_grafeo` has no production caller as of Phase 2 Task 2.
//! `translate_diff_event` (`src/bridge/sync_engine.rs:419`) only translates
//! `ROOT_VERTICES`/`ROOT_EDGES` diffs; the `LoroOp::TreeMove` variant in
//! `src/types/events.rs` is declared but NEVER generated. Wiring the
//! `T_CHILD` LoroTree container into the inbound subscriber is unscheduled
//! (no phase in `docs/implementation-plan.md` covers it). The function is
//! exercised only by `tests/unit/tree_move.rs` and
//! `tests/integration/tree_move_concurrency.rs` until bridge wiring lands.

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
/// Single-transaction reparent: delete the `(old_parent → node_id)` edge,
/// insert the `(new_parent → node_id)` edge (both with label
/// [`TREE_EDGE_LABEL`](crate::constants::TREE_EDGE_LABEL), direction
/// parent→child per architecture §7 line 265), commit atomically under
/// [`Serializable`](grafeo_engine::transaction::IsolationLevel::Serializable)
/// isolation. Cycles are rejected up-front via [`would_create_cycle_precheck`]
/// because Grafeo 0.5.42 has no native graph-edge acyclicity enforcement
/// (verified P2T2-L1).
///
/// # Edge direction (P2T2-DEVIL R1)
///
/// Direction is parent→child (`src=parent, dst=child`), matching the
/// architecture doc §7 lines 259, 265 (`(p)-[:CHILD]->(c)`) and Loro's
/// `LoroTree` semantics. The pre-existing child→parent direction in
/// `apply_tree_move` (`src/bridge/grafeo_tx.rs:200-206`) was a Phase 1 bug;
/// P2T2-L2 fixed it.
///
/// # TOCTOU defense (P2T2-DEVIL R3)
///
/// The cycle pre-check is racy under default `SnapshotIsolation` (peer B can
/// commit a cycle-creating edge between peer A's pre-check and A's commit —
/// the textbook SI write-skew anomaly). We defend by opening the write tx
/// with `Serializable` isolation (SSI); grafeo's SSI tracker detects the
/// read-write conflict between A's cycle-check and B's edge write and aborts
/// one peer at commit time. No post-commit re-check needed (Devil rejected
/// option (b) as eventually-consistent).
///
/// # Errors
///
/// - [`GrafeoLoroError::Bridge`] if `node_id` or `new_parent` does not exist
///   (verified via `Session::node_exists`, `session/mod.rs:5278`).
/// - [`GrafeoLoroError::TreeMoveCreatesCycle`] if `new_parent` is `node_id`
///   itself or a descendant of `node_id` (pre-check).
/// - [`GrafeoLoroError::Grafeo`] if the underlying session transaction fails
///   (write-write conflict, SSI violation, etc.).
///
/// Grafeo Session API (verified against `grafeo-engine-0.5.42/src/`):
/// - `GrafeoDB::session` — `database/mod.rs:1663` (`&self -> Session`)
/// - `Session::begin_transaction_with_isolation` — `session/mod.rs:3895` (`&mut self, IsolationLevel -> Result<()>`; `#[cfg(feature = "lpg")]`)
/// - `Session::create_edge` — `session/mod.rs:4935` (`&self, NodeId, NodeId, &str -> EdgeId`; infallible)
/// - `Session::delete_edge` — `session/mod.rs:5092` (`&self, EdgeId -> bool`; returns `false` if edge absent)
/// - `Session::get_neighbors_incoming` — `session/mod.rs:5237` (parent→child: incoming = parents of `cur`)
/// - `Session::node_exists` — `session/mod.rs:5278` (`&self, NodeId -> bool`)
/// - `Session::prepare_commit` — `session/mod.rs:4496` (`&mut self -> Result<PreparedCommit<'_>>`)
/// - `PreparedCommit::set_metadata` — `transaction/prepared.rs:107` (advisory; dropped on commit per Devil Gap 1)
/// - `PreparedCommit::commit` — `transaction/prepared.rs:124` (`self -> Result<EpochId>`)
pub fn sync_tree_move_to_grafeo(
    db: &GrafeoDB,
    node_id: NodeId,
    old_parent: NodeId,
    new_parent: NodeId,
) -> crate::error::Result<()> {
    // L2 HACK: silences dead_code warning until L3 implements the body.
    let _ = (db, node_id, old_parent, new_parent);

    // TODO(P2T2-L3): Validate existence: if !session.node_exists(node_id)
    //                 return Err(GrafeoLoroError::Bridge(format!("unknown node_id: {node_id:?}")));
    //                 if !session.node_exists(new_parent)
    //                 return Err(GrafeoLoroError::Bridge(format!("unknown new_parent: {new_parent:?}")));
    // TODO(P2T2-L3): Pre-check cycle: if would_create_cycle_precheck(db, node_id, new_parent)
    //                 return Err(GrafeoLoroError::TreeMoveCreatesCycle { node_id, new_parent }).
    // TODO(P2T2-L3): Noop guard (idempotent short-circuit; R4): if new_parent == old_parent
    //                 return Ok(()).
    // TODO(P2T2-L3): let mut session = db.session();
    // TODO(P2T2-L3): session.begin_transaction_with_isolation(
    //                     grafeo_engine::transaction::IsolationLevel::Serializable)?;
    // TODO(P2T2-L3): Resolve the existing (old_parent → node_id) EdgeId by walking
    //                 session.get_neighbors_incoming(node_id) (parent→child: incoming
    //                 neighbors of node_id are its parents) and matching src == old_parent;
    //                 if found, session.delete_edge(eid) — best-effort: log warn if absent
    //                 (Q2 best-effort delete semantics; root nodes have no parent edge).
    // TODO(P2T2-L3): session.create_edge(new_parent, node_id, TREE_EDGE_LABEL)
    //                 (parent→child direction per architecture §7 line 265).
    // TODO(P2T2-L3): let mut prepared = session.prepare_commit()?;
    // TODO(P2T2-L3): prepared.set_metadata("origin", ORIGIN_LORO_BRIDGE); // advisory, dropped on commit
    // TODO(P2T2-L3): prepared.commit()?; // -> EpochId; SSI may abort here on concurrent-cycle write-skew.
    Err(crate::error::GrafeoLoroError::Bridge(
        "sync_tree_move_to_grafeo not yet implemented".into(),
    ))
}

/// BFS upward from `new_parent` along `TREE_EDGE_LABEL` edges looking for
/// `node_id`. Returns `true` if `node_id` is reachable, meaning the proposed
/// move would create a cycle.
///
/// Edge direction is parent→child (src=parent, dst=child) per the
/// architecture doc §7 lines 259, 265 (P2T2-DEVIL R1); "upward" therefore
/// means following `Session::get_neighbors_incoming(cur)` (`session/mod.rs:5237`)
/// — incoming edges of `cur` point AT `cur` from its parents.
///
/// # Pre-check variant only (P2T2-DEVIL M4)
///
/// Because Q3 resolution (c) adopted Serializable isolation, no inside-tx
/// re-check helper is needed — SSI catches concurrent-cycle write-skew at
/// commit time. If the fallback (a) inside-tx re-check were ever needed,
/// split this into a `would_create_cycle_in_tx(session: &Session, ...)`
/// variant that takes a `&Session` reference (the `db: &GrafeoDB` signature
/// cannot be used inside an active tx: opening a nested session cannot see
/// the parent tx's uncommitted writes — `session/mod.rs:3911-3918`).
///
/// Grafeo 0.5.42 source verified (P2T2-L1) to have NO native graph-edge
/// acyclicity enforcement: only `catalog::resolved_node_type`
/// (`catalog/mod.rs:1349`) cycle-checks schema type inheritance, and
/// `procedures::has_negative_cycle` (`procedures.rs:831`) is a Bellman-Ford
/// query procedure — neither constrains user edges at commit time.
#[allow(dead_code)] // wired by P2T2-L3 in sync_tree_move_to_grafeo pre-check
fn would_create_cycle_precheck(db: &GrafeoDB, node_id: NodeId, new_parent: NodeId) -> bool {
    // L2 HACK: silences dead_code warning until L3 implements the body.
    let _ = (db, node_id, new_parent);
    // TODO(P2T2-L3): walk parent chain via session.get_neighbors_incoming(cur)
    //                 (parent→child: incoming = parents of cur); return true iff
    //                 node_id appears in the ancestor set of new_parent
    //                 (or iff new_parent == node_id — direct self-loop).
    todo!("P2T2-L3: implement cycle BFS upward via get_neighbors_incoming")
}

#[cfg(test)]
mod tests {
    // Unit tests for sync_tree_move_to_grafeo live in tests/unit/tree_move.rs
    // (separate test crate, matches Phase 2 Task 1 pattern).
}
