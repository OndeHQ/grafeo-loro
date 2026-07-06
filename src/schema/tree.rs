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

use std::collections::{HashSet, VecDeque};

use grafeo::GrafeoDB;
use lorosurgeon::{Hydrate, Reconcile};
use tracing::debug;

use crate::constants::{ORIGIN_LORO_BRIDGE, TREE_EDGE_LABEL};
use crate::error::GrafeoLoroError;
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
/// - `GrafeoDB::session_with_cdc` — `database/mod.rs:1728` (`&self, bool -> Session`; `#[cfg(feature = "cdc")]`; `cdc` feature enabled transitively via `grafeo = "0.5"` default → `embedded` → `ai` → `cdc`)
/// - `grafeo_engine::transaction::IsolationLevel::Serializable` — `transaction/manager.rs:63`, re-exported at `transaction/mod.rs:201` (umbrella `grafeo` does NOT re-export `transaction`; direct `grafeo-engine = "0.5"` dep required — P2T2-DEVIL R3)
/// - `Session::begin_transaction_with_isolation` — `session/mod.rs:3895` (`&mut self, IsolationLevel -> Result<()>`; `#[cfg(feature = "lpg")]`)
/// - `Session::create_edge` — `session/mod.rs:4935` (`&self, NodeId, NodeId, &str -> EdgeId`; infallible)
/// - `Session::delete_edge` — `session/mod.rs:5092` (`&self, EdgeId -> bool`; returns `false` if edge absent)
/// - `Session::get_neighbors_incoming` — `session/mod.rs:5237` (parent→child: incoming = parents of `cur`)
/// - `Session::get_neighbors_outgoing_by_type` — `session/mod.rs:5256` (`&self, NodeId, &str -> Vec<(NodeId, EdgeId)>`)
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
    // 1. Validate existence (`Session::node_exists`, session/mod.rs:5278). A fresh
    //    session reads the latest committed state; existence is checked BEFORE
    //    the pre-check so unknown ids surface as `Bridge` rather than `Grafeo`.
    let probe = db.session();
    if !probe.node_exists(node_id) {
        return Err(GrafeoLoroError::Bridge(format!(
            "unknown node_id: {node_id:?}"
        )));
    }
    if !probe.node_exists(new_parent) {
        return Err(GrafeoLoroError::Bridge(format!(
            "unknown new_parent: {new_parent:?}"
        )));
    }
    drop(probe);

    // 2. Pre-check cycle (P2T2-DEVIL R1/R3). Grafeo 0.5.42 has no native
    //    acyclicity enforcement, so the bridge must reject cycle-creating
    //    moves up-front. Serializable isolation (step 4) catches concurrent
    //    write-skew at commit time.
    if would_create_cycle_precheck(db, node_id, new_parent) {
        return Err(GrafeoLoroError::TreeMoveCreatesCycle { node_id, new_parent });
    }

    // 3. Noop guard (idempotent short-circuit; P2T2-DEVIL R4/m2). Placed AFTER
    //    cycle pre-check so `sync_tree_move_to_grafeo(db, n, A, A)` returns
    //    `Ok(())` without opening a tx (true no-op, no edge churn).
    if old_parent == new_parent {
        debug!(?node_id, ?new_parent, "tree move noop: old_parent == new_parent");
        return Ok(());
    }

    // 4. Open tx (Serializable). `session_with_cdc(false)` disables CDC tracking
    //    for tree moves so they don't echo back through the outbound poller.
    let mut session = db.session_with_cdc(false);
    session.begin_transaction_with_isolation(
        grafeo_engine::transaction::IsolationLevel::Serializable,
    )?;

    // 5. Resolve + delete old edge (best-effort; Q2). Walk `old_parent`'s
    //    outgoing `:CHILD` edges and match `dst == node_id`. Root nodes have
    //    no parent edge — the best-effort delete is a no-op (P2T2-DEVIL Q2).
    let old_edge: Option<grafeo::EdgeId> = session
        .get_neighbors_outgoing_by_type(old_parent, TREE_EDGE_LABEL)
        .into_iter()
        .find(|(dst, _)| *dst == node_id)
        .map(|(_, eid)| eid);
    if let Some(eid) = old_edge {
        let deleted = session.delete_edge(eid);
        if !deleted {
            debug!(?eid, ?old_parent, ?node_id, "old_parent→node_id edge already absent during delete");
        }
    } else {
        debug!(?old_parent, ?node_id, "no old_parent→node_id CHILD edge to delete (root or stale)");
    }

    // 6. Insert new edge (parent→child per architecture §7 line 265).
    session.create_edge(new_parent, node_id, TREE_EDGE_LABEL);

    // 7. Prepare + commit. `set_metadata` is advisory (dropped on commit per
    //    Devil Gap 1); the epoch side-channel is the real echo-prevention
    //    mechanism. `commit()` may return `Err(Grafeo(_))` on SSI conflict.
    let mut prepared = session.prepare_commit()?;
    prepared.set_metadata("origin", ORIGIN_LORO_BRIDGE);
    prepared.commit()?;
    Ok(())
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
fn would_create_cycle_precheck(db: &GrafeoDB, node_id: NodeId, new_parent: NodeId) -> bool {
    // Direct self-loop (trivial cycle).
    if node_id == new_parent {
        debug!(?node_id, "cycle pre-check: node_id == new_parent (self-loop)");
        return true;
    }

    // BFS upward from `new_parent` along incoming edges (parent→child: incoming
    // of `cur` = parents of `cur`). If `node_id` is reachable, the proposed move
    // would close a cycle (`node_id → ... → new_parent → node_id`).
    let session = db.session();
    let mut queue: VecDeque<NodeId> = VecDeque::new();
    let mut visited: HashSet<NodeId> = HashSet::new();
    queue.push_back(new_parent);
    visited.insert(new_parent);

    while let Some(cur) = queue.pop_front() {
        for (parent_id, _edge_id) in session.get_neighbors_incoming(cur) {
            if parent_id == node_id {
                debug!(?node_id, ?new_parent, ?cur, "cycle pre-check: node_id is ancestor of new_parent");
                return true;
            }
            if visited.insert(parent_id) {
                queue.push_back(parent_id);
            }
        }
    }
    debug!(?node_id, ?new_parent, "cycle pre-check: no cycle detected");
    false
}

#[cfg(test)]
mod tests {
    // Unit tests for sync_tree_move_to_grafeo live in tests/unit/tree_move.rs
    // (separate test crate, matches Phase 2 Task 1 pattern).
}
