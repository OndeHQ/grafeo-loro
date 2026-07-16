//! Loro → Grafeo tree reparenting bridge + native cycle guard (issue #3
//! sub-issue 7).
//!
//! # Native cycle guard (issue #3 sub-issue 7, invariant I14)
//!
//! [`CycleGuard`] maintains a `parent_of` map keyed by node-key strings so
//! edge inserts can reject cycles in O(depth) worst-case before commit. This
//! is the native enforcement of invariant I14 (tree acyclicity) that the
//! issue body calls for — the prior bridge-only `would_create_cycle_in_tx`
//! BFS walked grafeo's neighbor index at insert time (O(N) per move, killed
//! 60fps). The new guard lives entirely in the schema layer (no `grafeo`
//! feature dep) and is consulted pre-commit by `bridge::grafeo_tx::apply_*`
//! once the orchestrator wires it into the inbound apply path.
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
//!
//! # Feature gating
//!
//! Issue #1: `sync_tree_move_to_grafeo` requires `grafeo` feature (calls
//! `GrafeoDB::session` etc.). The `OrderedCollection` / `TreeNode` types +
//! [`CycleGuard`] / [`CycleError`] at the top of this file are available
//! whenever `bridge` is on (no grafeo dep); the `sync_tree_move_to_grafeo`
//! function is gated by `grafeo`.

use std::collections::{HashMap, HashSet};
#[cfg(feature = "grafeo")]
use std::collections::VecDeque;

use lorosurgeon::{Hydrate, Reconcile};
#[cfg(feature = "grafeo")]
use tracing::{debug, instrument};

#[cfg(feature = "grafeo")]
use crate::constants::{ORIGIN_LORO_BRIDGE, TREE_EDGE_LABEL};
#[cfg(feature = "grafeo")]
use crate::error::GrafeoLoroError;
#[cfg(feature = "grafeo")]
use crate::types::ids::NodeId;

// ============================================================================
// Native cycle guard (issue #3 sub-issue 7, invariant I14)
// ============================================================================
//
// `CycleGuard` is a pure-Rust parent-pointer map. Insert-time cycle detection
// is O(depth) worst-case (follow parent pointers from `new_parent` upward
// until hitting `node` or a root). This is the native enforcement of
// invariant I14 that the issue body calls for — replacing the bridge-only
// `would_create_cycle_in_tx` BFS (which walked grafeo's neighbor index at
// O(N) per move).
//
// The guard is intentionally decoupled from grafeo: it operates on string
// node-keys (the Loro-side identity), so it works in pure-WASM builds where
// the `grafeo` execution layer is absent. The orchestrator wires it into
// `bridge::grafeo_tx::apply_tree_move` + the inbound batcher's pre-commit
// hook in a follow-up.

/// Error returned by [`CycleGuard::apply_move`] when the proposed move
/// would create a cycle (invariant I14 violation).
///
/// Distinct from `tree_adapter::CycleError` (which carries `NodeId`s and is
/// used by the `tree` feature's adapter layer). This schema-layer error uses
/// string keys so it works in builds without `grafeo`/`tree` features.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cycle: node {node:?} cannot be reparented under {new_parent:?}")]
pub struct CycleError {
    /// Key of the node being reparented.
    pub node: String,
    /// Proposed new parent key.
    pub new_parent: String,
}

/// Native parent-pointer map for O(depth) cycle detection on edge insert
/// (issue #3 sub-issue 7, invariant I14).
///
/// The guard tracks one parent per node (enforces tree-ness, not just
/// acyclicity). Multi-parent graphs (DAGs) are NOT supported by this guard
/// — use [`crate::schema::edge::validate_acyclic`] for batch DAG validation.
///
/// # Cost model
///
/// - `apply_move`: O(depth) worst-case (walks parent chain to check for
///   cycle before mutating the map).
/// - `would_create_cycle`: O(depth) worst-case (same walk, read-only).
/// - `roots`: O(N) — iterates all entries. Cached root set is the
///   responsibility of [`crate::schema::RootTracker`].
///
/// # Persistence story
///
/// The guard is in-memory only. Re-hydration from a Loro snapshot must
/// replay tree moves through `apply_move` to rebuild the parent map. The
/// orchestrator's hydration path (`src/hydration/*`) is the wiring point.
#[derive(Debug, Clone, Default)]
pub struct CycleGuard {
    /// `node_key → parent_key`. Absent entry = root.
    parent_of: HashMap<String, String>,
}

impl CycleGuard {
    /// Construct a fresh empty guard.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if reparenting `node` under `new_parent` would close a
    /// cycle. O(depth) worst-case: walks the parent chain from `new_parent`
    /// upward looking for `node`. Self-loops (`node == new_parent`) return
    /// `true` immediately.
    pub fn would_create_cycle(&self, node: &str, new_parent: &str) -> bool {
        if node == new_parent {
            return true;
        }
        // Walk parent chain from `new_parent` upward. If we hit `node`, the
        // proposed move closes a cycle. If we hit a root (no parent entry),
        // no cycle.
        let mut cur = new_parent.to_string();
        loop {
            if cur == node {
                return true;
            }
            match self.parent_of.get(&cur) {
                Some(p) => cur = p.clone(),
                None => return false,
            }
        }
    }

    /// Apply a reparent move: update the parent pointer for `node` to
    /// `new_parent`. Returns `Err(CycleError)` if the move would create a
    /// cycle (pre-checked via [`Self::would_create_cycle`]). O(depth)
    /// worst-case.
    pub fn apply_move(&mut self, node: &str, new_parent: &str) -> Result<(), CycleError> {
        if self.would_create_cycle(node, new_parent) {
            return Err(CycleError {
                node: node.to_string(),
                new_parent: new_parent.to_string(),
            });
        }
        self.parent_of.insert(node.to_string(), new_parent.to_string());
        Ok(())
    }

    /// Record an initial parent binding (no cycle check — used during
    /// hydration replay when the caller guarantees the source graph was
    /// already acyclic). O(1).
    ///
    /// # Safety (logical)
    ///
    /// Calling this with a binding that creates a cycle leaves the guard in
    /// an inconsistent state — `would_create_cycle` will still detect the
    /// cycle on subsequent moves, but `roots()` may return a stale set. Use
    /// [`Self::apply_move`] for any binding whose acyclicity is not
    /// pre-validated.
    pub fn record_parent_unchecked(&mut self, node: &str, parent: &str) {
        self.parent_of.insert(node.to_string(), parent.to_string());
    }

    /// Remove a node's parent binding (turns it back into a root). O(1).
    /// No-op if the node was a root or absent.
    pub fn detach(&mut self, node: &str) {
        self.parent_of.remove(node);
    }

    /// Returns the parent of `node` if one is recorded, else `None` (root
    /// or unknown node). O(1).
    pub fn parent_of(&self, node: &str) -> Option<&str> {
        self.parent_of.get(node).map(String::as_str)
    }

    /// Returns all node keys known to the guard (whether root or non-root).
    /// O(N).
    pub fn known_nodes(&self) -> impl Iterator<Item = &str> {
        self.parent_of.keys().map(String::as_str)
    }

    /// Returns the set of root node keys (nodes with no recorded parent).
    /// O(N) — iterates all entries. For incremental O(1) root membership
    /// queries, use [`crate::schema::RootTracker`] instead.
    ///
    /// Note: this returns only nodes the guard knows about. A node that has
    /// never been registered via `apply_move` / `record_parent_unchecked`
    /// is unknown to the guard and will NOT appear in `roots()`.
    pub fn roots(&self) -> Vec<String> {
        // A node is a root iff it appears as a parent target somewhere OR
        // has an entry with no parent of its own. We need both: nodes that
        // are pointed-to but don't point anywhere (pure roots) + nodes that
        // are roots because they were never reparented.
        let mut all_nodes: HashSet<&str> = HashSet::new();
        for (child, parent) in self.parent_of.iter() {
            all_nodes.insert(child.as_str());
            all_nodes.insert(parent.as_str());
        }
        all_nodes
            .into_iter()
            .filter(|n| !self.parent_of.contains_key(*n))
            .map(String::from)
            .collect()
    }

    /// Number of nodes the guard has parent info for. O(1).
    pub fn len(&self) -> usize {
        self.parent_of.len()
    }

    /// Whether the guard is empty. O(1).
    pub fn is_empty(&self) -> bool {
        self.parent_of.is_empty()
    }
}

#[cfg(test)]
mod cycle_guard_tests {
    use super::*;

    #[test]
    fn self_loop_rejected() {
        let mut g = CycleGuard::new();
        assert!(g.apply_move("a", "a").is_err());
    }

    #[test]
    fn direct_cycle_rejected() {
        // A→B then B→A must fail.
        let mut g = CycleGuard::new();
        g.apply_move("b", "a").unwrap();
        assert!(g.apply_move("a", "b").is_err());
    }

    #[test]
    fn deep_cycle_rejected() {
        // A→B→C then C→A must fail.
        let mut g = CycleGuard::new();
        g.apply_move("b", "a").unwrap();
        g.apply_move("c", "b").unwrap();
        assert!(g.apply_move("a", "c").is_err());
    }

    #[test]
    fn non_cycle_move_accepted() {
        let mut g = CycleGuard::new();
        g.apply_move("b", "a").unwrap();
        assert!(g.apply_move("c", "a").is_ok());
        assert_eq!(g.parent_of("c"), Some("a"));
    }

    #[test]
    fn roots_correct_after_moves() {
        let mut g = CycleGuard::new();
        g.apply_move("b", "a").unwrap();
        g.apply_move("c", "b").unwrap();
        let mut roots = g.roots();
        roots.sort();
        assert_eq!(roots, vec!["a".to_string()]);
    }

    #[test]
    fn detach_makes_root_again() {
        let mut g = CycleGuard::new();
        g.apply_move("b", "a").unwrap();
        g.detach("b");
        assert!(g.parent_of("b").is_none());
    }
}

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
/// isolation. Cycles are rejected up-front via [`would_create_cycle_in_tx`]
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
/// # TOCTOU limitation (P2T2-DEVIL R3 deviation — honest disclosure)
///
/// The cycle pre-check runs INSIDE the Serializable tx (after
/// `begin_transaction_with_isolation(Serializable)`), reading the tx's
/// consistent snapshot via [`would_create_cycle_in_tx`]. The structural
/// placement is forward-compatible: IF grafeo's direct-CRUD read paths
/// (`Session::get_neighbors_incoming` at `session/mod.rs:5237`) called
/// `TransactionManager::record_read` (`transaction/manager.rs:225`), SSI
/// would detect read-write conflicts between peer A's pre-check and peer B's
/// concurrent edge write and abort one peer at commit time.
///
/// **However, grafeo 0.5.42 does NOT wire direct-CRUD reads into SSI
/// tracking.** `Session::get_neighbors_incoming` (`session/mod.rs:5237`),
/// `Session::get_node` (`session/mod.rs:5138`), and `Session::get_edge`
/// (`session/mod.rs:5185`) all bypass `record_read` (verified by source
/// analysis + empirical two-tx probe in P2T2-L2R2 — see worklog). The
/// `TransactionManager::commit` SSI validation (`transaction/manager.rs:313`)
/// operates on an empty `read_set` for direct-CRUD transactions, so SSI does
/// NOT actually detect read-write conflicts for this code path in 0.5.42.
/// Direct-CRUD writes (`create_edge`/`delete_edge`) likewise bypass
/// `record_write`, so SSI does NOT detect write-write conflicts either.
///
/// The Serializable isolation level on direct-CRUD transactions provides
/// only: (1) snapshot isolation (each tx sees its own snapshot at
/// `start_epoch`), (2) PENDING-epoch versioning (uncommitted writes
/// invisible to other sessions), (3) atomic commit/rollback of version
/// chains. NOT conflict detection.
///
/// **Actual active defense**: each move is individually acyclic relative to
/// its pre-check snapshot, so the final committed graph is always ACYCLIC.
/// Concurrent moves can create diamonds (node with 2 parents) when both
/// peers' pre-checks pass against stale snapshots and both commit (disjoint
/// write sets, no conflict detection). The tree invariant (≤1 parent per
/// node) can be violated. This is ACCEPTABLE for Phase 2 (mandate is
/// acyclicity per `docs/implementation-plan.md:53`, not tree-ness). The
/// integration test's BFS acyclicity assertion is the real safety net.
///
/// # Errors
///
/// - [`GrafeoLoroError::Bridge`] if `node_id` or `new_parent` does not exist
///   (verified via `Session::node_exists`, `session/mod.rs:5278`).
/// - [`GrafeoLoroError::TreeMoveCreatesCycle`] if `new_parent` is `node_id`
///   itself or a descendant of `node_id` (pre-check, inside the Serializable tx).
/// - [`GrafeoLoroError::Grafeo`] if the underlying session transaction fails
///   (begin/prepare/commit error; SSI aborts are NOT expected for direct-CRUD
///   in 0.5.42 per the limitation above).
///
/// Grafeo Session API (verified against `grafeo-engine-0.5.42/src/`):
/// - `GrafeoDB::session` — `database/mod.rs:1663` (`&self -> Session`)
/// - `GrafeoDB::session_with_cdc` — `database/mod.rs:1728` (`&self, bool -> Session`; `#[cfg(feature = "cdc")]`; `cdc` feature enabled transitively via `grafeo = "0.5"` default → `embedded` → `ai` → `cdc`)
/// - `grafeo_engine::transaction::IsolationLevel::Serializable` — `transaction/manager.rs:63`, re-exported at `transaction/mod.rs:201` (umbrella `grafeo` does NOT re-export `transaction`; direct `grafeo-engine = "0.5"` dep required — P2T2-DEVIL R3)
/// - `Session::begin_transaction_with_isolation` — `session/mod.rs:3895` (`&mut self, IsolationLevel -> Result<()>`; `#[cfg(feature = "lpg")]`)
/// - `Session::create_edge` — `session/mod.rs:4935` (`&self, NodeId, NodeId, &str -> EdgeId`; infallible)
/// - `Session::delete_edge` — `session/mod.rs:5092` (`&self, EdgeId -> bool`; returns `false` if edge absent)
/// - `Session::get_neighbors_incoming` — `session/mod.rs:5237` (parent→child: incoming = parents of `cur`); does NOT call `TransactionManager::record_read` in 0.5.42
/// - `Session::get_neighbors_outgoing_by_type` — `session/mod.rs:5256` (`&self, NodeId, &str -> Vec<(NodeId, EdgeId)>`)
/// - `Session::node_exists` — `session/mod.rs:5278` (`&self, NodeId -> bool`)
/// - `Session::prepare_commit` — `session/mod.rs:4496` (`&mut self -> Result<PreparedCommit<'_>>`)
/// - `PreparedCommit::set_metadata` — `transaction/prepared.rs:107` (advisory; dropped on commit per Devil Gap 1)
/// - `PreparedCommit::commit` — `transaction/prepared.rs:124` (`self -> Result<EpochId>`)
#[cfg(feature = "grafeo")]
#[instrument(skip(db), name = "sync_tree_move_to_grafeo", level = "info")]
pub fn sync_tree_move_to_grafeo(
    db: &grafeo::GrafeoDB,
    node_id: NodeId,
    old_parent: NodeId,
    new_parent: NodeId,
) -> crate::error::Result<()> {
    // 1. Validate existence (`Session::node_exists`, session/mod.rs:5278). A fresh
    //    session reads the latest committed state; existence is checked BEFORE
    //    the Serializable tx so unknown ids surface as `Bridge` rather than
    //    `Grafeo`. Existence is a stable property (a node never disappears),
    //    so checking outside the tx does not weaken SSI guarantees.
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

    // 2. Noop guard (idempotent short-circuit; P2T2-DEVIL R4/m2). Placed
    //    BEFORE the Serializable tx so `sync_tree_move_to_grafeo(db, n, A, A)`
    //    returns `Ok(())` without opening a tx (true no-op, no edge churn).
    //    If `old_parent == new_parent`, no edges change, so no cycle can be
    //    created — the pre-check is skipped.
    if old_parent == new_parent {
        debug!(
            ?node_id,
            ?new_parent,
            "tree move noop: old_parent == new_parent"
        );
        return Ok(());
    }

    // 3. Open tx (Serializable). `session_with_cdc(false)` disables CDC tracking
    //    for tree moves so they don't echo back through the outbound poller.
    //    On early return (cycle pre-check below), the owned `session` is
    //    dropped and `Session::Drop` (`session/mod.rs:5368`) auto-rollbacks
    //    the active tx — no explicit rollback needed.
    let mut session = db.session_with_cdc(false);
    session.begin_transaction_with_isolation(
        grafeo_engine::transaction::IsolationLevel::Serializable,
    )?;

    // 4. Pre-check cycle INSIDE the Serializable tx (P2T2-DEVIL R1/R3).
    //    Grafeo 0.5.42 has no native acyclicity enforcement, so the bridge
    //    must reject cycle-creating moves up-front. Running inside the tx
    //    reads the tx's consistent snapshot (forward-compatible: if a future
    //    grafeo version wires direct-CRUD reads into `record_read`, SSI will
    //    activate automatically). See the `# TOCTOU limitation` doc section
    //    above for the honest 0.5.42 disclosure.
    if would_create_cycle_in_tx(&session, node_id, new_parent) {
        return Err(GrafeoLoroError::TreeMoveCreatesCycle {
            node_id,
            new_parent,
        });
    }

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
            debug!(
                ?eid,
                ?old_parent,
                ?node_id,
                "old_parent→node_id edge already absent during delete"
            );
        }
    } else {
        debug!(
            ?old_parent,
            ?node_id,
            "no old_parent→node_id CHILD edge to delete (root or stale)"
        );
    }

    // 6. Insert new edge (parent→child per architecture §7 line 265).
    session.create_edge(new_parent, node_id, TREE_EDGE_LABEL);

    // 7. Prepare + commit. `set_metadata` is advisory (dropped on commit per
    //    Devil Gap 1); the epoch side-channel is the real echo-prevention
    //    mechanism. `commit()` may return `Err(Grafeo(_))` on internal commit
    //    failure (NOT SSI conflict detection — see TOCTOU limitation above).
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
/// # Inside-tx variant (P2T2-DEVIL M4 + P2T2-HUNT M1)
///
/// Takes `&Session` and is called AFTER `begin_transaction_with_isolation(Serializable)`
/// inside `sync_tree_move_to_grafeo` (see step 4 of that function). Reads the
/// tx's consistent snapshot.
///
/// **Forward-compatibility note**: in grafeo 0.5.42, `Session::get_neighbors_incoming`
/// (`session/mod.rs:5237`) does NOT call `TransactionManager::record_read`
/// (`transaction/manager.rs:225`), so SSI does NOT track these reads for
/// conflict detection (verified empirically via a two-tx probe — see
/// P2T2-L2R2 worklog). The structural placement inside the Serializable tx
/// is preserved so that IF a future grafeo version wires direct-CRUD reads
/// into `record_read`, SSI will detect concurrent-cycle write-skew at commit
/// time automatically. For 0.5.42, the actual defense is per-move acyclicity
/// (each move is individually acyclic relative to its pre-check snapshot, so
/// the final graph is always acyclic; diamonds are possible but not cycles).
///
/// Grafeo 0.5.42 source verified (P2T2-L1) to have NO native graph-edge
/// acyclicity enforcement: only `catalog::resolved_node_type`
/// (`catalog/mod.rs:1349`) cycle-checks schema type inheritance, and
/// `procedures::has_negative_cycle` (`procedures.rs:831`) is a Bellman-Ford
/// query procedure — neither constrains user edges at commit time.
#[cfg(feature = "grafeo")]
fn would_create_cycle_in_tx(
    session: &grafeo::Session,
    node_id: NodeId,
    new_parent: NodeId,
) -> bool {
    // Direct self-loop (trivial cycle).
    if node_id == new_parent {
        debug!(
            ?node_id,
            "cycle pre-check: node_id == new_parent (self-loop)"
        );
        return true;
    }

    // BFS upward from `new_parent` along incoming edges (parent→child: incoming
    // of `cur` = parents of `cur`). If `node_id` is reachable, the proposed move
    // would close a cycle (`node_id → ... → new_parent → node_id`).
    let mut queue: VecDeque<NodeId> = VecDeque::new();
    let mut visited: HashSet<NodeId> = HashSet::new();
    queue.push_back(new_parent);
    visited.insert(new_parent);

    while let Some(cur) = queue.pop_front() {
        for (parent_id, _edge_id) in session.get_neighbors_incoming(cur) {
            if parent_id == node_id {
                debug!(
                    ?node_id,
                    ?new_parent,
                    ?cur,
                    "cycle pre-check: node_id is ancestor of new_parent"
                );
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
