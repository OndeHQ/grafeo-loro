//! Phase 2 Task 2: `sync_tree_move_to_grafeo` tree reparenting.
//!
//! Tests use isolated `GrafeoDB::new_in_memory()` instances (no bridge, no CDC, no
//! Loro) so they exercise only the grafeo-side reparenting contract.
//!
//! Edge direction is parent→child per architecture §7 line 265 (P2T2-DEVIL R1).
//!
//! Grafeo Session API (verified P2T2-L1 + P2T2-L2 against `grafeo-engine-0.5.42/src/`):
//! - `GrafeoDB::new_in_memory()` — in-memory constructor
//! - `db.session()` — `database/mod.rs:1663`
//! - `session.create_node(&[&str]) -> NodeId` — `session/mod.rs:4860` (infallible)
//! - `session.create_edge(src, dst, &str) -> EdgeId` — `session/mod.rs:4935` (infallible)
//! - `session.delete_edge(EdgeId) -> bool` — `session/mod.rs:5092`
//! - `session.get_neighbors_incoming(NodeId) -> Vec<(NodeId, EdgeId)>` — `session/mod.rs:5237`
//! - `session.get_neighbors_outgoing_by_type(NodeId, &str) -> Vec<(NodeId, EdgeId)>` — `session/mod.rs:5256`
//! - `session.node_exists(NodeId) -> bool` — `session/mod.rs:5278`

#![allow(missing_docs)]

use grafeo::{EdgeId, GrafeoDB};
use grafeo_loro::constants::TREE_EDGE_LABEL;
use grafeo_loro::error::GrafeoLoroError;
use grafeo_loro::schema::tree::sync_tree_move_to_grafeo;
use grafeo_loro::types::ids::NodeId;

/// Build a 3-node fixture `(root, mid, leaf)` wired as
/// `root --CHILD--> mid --CHILD--> leaf` (parent→child direction per
/// architecture §7 line 265; P2T2-DEVIL R1). Returns `(root_id, mid_id, leaf_id)`.
///
/// Uses `session.create_node(&["Folder"])` (`session/mod.rs:4860`) +
/// `session.create_edge(parent, child, TREE_EDGE_LABEL)` (`session/mod.rs:4935`)
/// — both infallible and auto-committed at the current epoch when no tx is open.
fn build_chain_fixture(db: &GrafeoDB) -> (NodeId, NodeId, NodeId) {
    let session = db.session();
    let root = session.create_node(&["Folder"]);
    let mid = session.create_node(&["Folder"]);
    let leaf = session.create_node(&["Folder"]);
    session.create_edge(root, mid, TREE_EDGE_LABEL);
    session.create_edge(mid, leaf, TREE_EDGE_LABEL);
    (root, mid, leaf)
}

/// Collect the parent NodeIds of `node` (incoming CHILD neighbors) for assertions.
fn parents_of(db: &GrafeoDB, node: NodeId) -> Vec<NodeId> {
    db.session()
        .get_neighbors_incoming(node)
        .into_iter()
        .map(|(n, _)| n)
        .collect()
}

/// Move a leaf from parent A to parent B; assert the old `:CHILD` edge is
/// gone and the new `:CHILD` edge is present after `sync_tree_move_to_grafeo`
/// returns `Ok(())`. Acyclic invariant must hold (BFS up from leaf reaches
/// the new root, not the old one).
#[test]
fn tree_move_basic() {
    let db = GrafeoDB::new_in_memory();
    let (root, mid, leaf) = build_chain_fixture(&db);

    // Move `leaf` from `mid` to `root` (parent→child: new edge root→leaf).
    let result = sync_tree_move_to_grafeo(&db, leaf, mid, root);
    assert!(result.is_ok(), "expected Ok, got {result:?}");

    // Two-sided assertion: old edge gone AND new edge present (anti-Goodhart).
    let leaf_parents = parents_of(&db, leaf);
    assert!(
        !leaf_parents.contains(&mid),
        "old mid→leaf edge should be gone, leaf parents = {leaf_parents:?}"
    );
    assert!(
        leaf_parents.contains(&root),
        "new root→leaf edge should be present, leaf parents = {leaf_parents:?}"
    );
    // Sanity: root→mid edge unchanged (the move must not have touched it).
    let mid_parents = parents_of(&db, mid);
    assert!(
        mid_parents.contains(&root),
        "root→mid edge should be unchanged, mid parents = {mid_parents:?}"
    );
}

/// Reparenting a non-root node under its own descendant must return
/// `Err(GrafeoLoroError::TreeMoveCreatesCycle { node_id, new_parent })`.
/// Grafeo 0.5.42 has no native acyclicity check (verified P2T2-L1), so the
/// bridge's `would_create_cycle_in_tx` pre-check is the only defense. This
/// exercises the GENERAL cycle case (non-root `mid` with a real parent edge
/// being moved under its descendant `leaf`), distinct from
/// `tree_move_root_to_descendant_rejected_as_cycle` which covers the
/// root-specific case (P2T2-HUNT m1).
#[test]
fn tree_move_cycle_rejected() {
    let db = GrafeoDB::new_in_memory();
    let (root, mid, leaf) = build_chain_fixture(&db);
    // Move `mid` (which has `root` as a real parent) under `leaf` — `leaf` is
    // a descendant of `mid`, so the pre-check must reject with
    // `TreeMoveCreatesCycle`. Unlike `tree_move_root_to_descendant_rejected_as_cycle`,
    // this exercises the case where the moved node has a real parent edge
    // (`root→mid`), so the best-effort delete step WOULD have an edge to
    // delete if the pre-check didn't fire first.
    let err = sync_tree_move_to_grafeo(&db, mid, root, leaf).unwrap_err();
    assert!(
        matches!(err, GrafeoLoroError::TreeMoveCreatesCycle { .. }),
        "expected TreeMoveCreatesCycle, got {err:?}"
    );
    // Anti-Goodhart: graph unchanged — root→mid and mid→leaf both intact.
    assert_eq!(parents_of(&db, mid), vec![root], "root→mid edge must be intact");
    assert_eq!(parents_of(&db, leaf), vec![mid], "mid→leaf edge must be intact");
}

/// Moving a root (no parent edge) under its own descendant must return
/// `Err(TreeMoveCreatesCycle)` — a specific edge case where the cycle
/// pre-check must catch the cycle WITHOUT relying on a delete-then-recheck
/// pattern (there is no parent edge to delete). Renamed from
/// `tree_move_root_to_leaf_rejected` per P2T2-DEVIL M5/R2/m1.
#[test]
fn tree_move_root_to_descendant_rejected_as_cycle() {
    let db = GrafeoDB::new_in_memory();
    let (root, mid, leaf) = build_chain_fixture(&db);
    // `root` has no incoming `:CHILD` edge (it's a root); `leaf` is its
    // descendant. Best-effort delete (Q2) is a no-op; the cycle pre-check
    // (Q1/R1 walks `get_neighbors_incoming`) catches `leaf` is reachable
    // from `root`, so the move is rejected as `TreeMoveCreatesCycle`.
    let err = sync_tree_move_to_grafeo(&db, root, root, leaf).unwrap_err();
    assert!(
        matches!(err, GrafeoLoroError::TreeMoveCreatesCycle { .. }),
        "expected TreeMoveCreatesCycle (root → descendant is a cycle), got {err:?}"
    );
    // Anti-Goodhart: graph unchanged — root still has no parent, mid still
    // has root as its only parent, leaf still has mid as its only parent.
    assert!(parents_of(&db, root).is_empty(), "root must remain parentless");
    assert_eq!(parents_of(&db, mid), vec![root], "mid→root edge must be intact");
    assert_eq!(parents_of(&db, leaf), vec![mid], "mid→leaf edge must be intact");
}

/// Idempotent move: calling `sync_tree_move_to_grafeo(db, node, A, A)` must
/// return `Ok(())` and leave the edge set unchanged (no duplicate edges,
/// no spurious deletion). The noop guard short-circuits BEFORE the tx is
/// opened (P2T2-DEVIL R4/m2).
#[test]
fn tree_move_same_parent_noop() {
    let db = GrafeoDB::new_in_memory();
    let (_root, mid, leaf) = build_chain_fixture(&db);

    // Capture edge set BEFORE the call (full (parent, edge_id) pairs to detect
    // any churn — including edge id rewrite with same parent set).
    let before: Vec<(NodeId, EdgeId)> = db.session().get_neighbors_incoming(leaf);

    // `leaf` is currently under `mid`; calling sync with old_parent==new_parent==mid
    // is a noop and must return Ok without opening a tx.
    let result = sync_tree_move_to_grafeo(&db, leaf, mid, mid);
    assert!(result.is_ok(), "expected Ok, got {result:?}");

    // Assert edge set unchanged (no duplicate mid→leaf edges, no spurious deletion).
    let after: Vec<(NodeId, EdgeId)> = db.session().get_neighbors_incoming(leaf);
    assert_eq!(before, after, "edge set must be unchanged after noop move");
    assert_eq!(after.len(), 1, "exactly one mid→leaf edge must remain");
}

/// Moving a non-existent `node_id` must return `Err(Bridge("unknown node_id: …"))`
/// — silently succeeding would hide caller bugs (anti-plenger rule #1, #9).
/// Contract pinned by P2T2-DEVIL M4/m4.
#[test]
fn tree_move_unknown_node_rejected() {
    let db = GrafeoDB::new_in_memory();
    let session = db.session();
    let a: NodeId = session.create_node(&["Folder"]);
    let b: NodeId = session.create_node(&["Folder"]);
    let nonexistent: NodeId = NodeId::from(999_999);
    let err = sync_tree_move_to_grafeo(&db, nonexistent, a, b).unwrap_err();
    assert!(
        matches!(err, GrafeoLoroError::Bridge(ref msg) if msg.contains("unknown node_id")),
        "expected Bridge(\"unknown node_id: …\"), got {err:?}"
    );
}

/// Moving under a non-existent `new_parent` must return
/// `Err(Bridge("unknown new_parent: …"))`. Contract pinned by P2T2-DEVIL M4/m4.
#[test]
fn tree_move_unknown_new_parent_rejected() {
    let db = GrafeoDB::new_in_memory();
    let session = db.session();
    let a: NodeId = session.create_node(&["Folder"]);
    let b: NodeId = session.create_node(&["Folder"]);
    let nonexistent: NodeId = NodeId::from(999_999);
    let err = sync_tree_move_to_grafeo(&db, b, a, nonexistent).unwrap_err();
    assert!(
        matches!(err, GrafeoLoroError::Bridge(ref msg) if msg.contains("unknown new_parent")),
        "expected Bridge(\"unknown new_parent: …\"), got {err:?}"
    );
}

/// Direct self-loop: `sync_tree_move_to_grafeo(db, n, A, n)` must return
/// `Err(TreeMoveCreatesCycle)` — `new_parent == node_id` is a trivial cycle
/// caught by the pre-check's `new_parent == node_id` short-circuit (or by
/// BFS returning true on the first iteration). Contract pinned by
/// P2T2-DEVIL M4/m4.
#[test]
fn tree_move_to_self_direct_cycle_rejected() {
    let db = GrafeoDB::new_in_memory();
    let session = db.session();
    let a: NodeId = session.create_node(&["Folder"]);
    let x: NodeId = session.create_node(&["Folder"]);
    let err = sync_tree_move_to_grafeo(&db, x, a, x).unwrap_err();
    assert!(
        matches!(err, GrafeoLoroError::TreeMoveCreatesCycle { .. }),
        "expected TreeMoveCreatesCycle (self-loop), got {err:?}"
    );
}
