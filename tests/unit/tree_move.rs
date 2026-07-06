//! Phase 2 Task 2 scaffolds: `sync_tree_move_to_grafeo` tree reparenting.
//!
//! All four scaffolds are `#[ignore]` with `todo!()` bodies — L3 (P2T2-L3)
//! fills them in. Tests use isolated `GrafeoDB::new_in_memory()` instances
//! (no bridge, no CDC, no Loro) so they exercise only the grafeo-side
//! reparenting contract.
//!
//! Grafeo Session API (verified P2T2-L1 against `grafeo-engine-0.5.42/src/`):
//! - `GrafeoDB::new_in_memory()` — in-memory constructor
//! - `db.session()` — `database/mod.rs:1663`
//! - `session.create_node(&[&str]) -> NodeId` — for fixture setup
//! - `session.create_edge(src, dst, &str) -> EdgeId` — `session/mod.rs:4935` (infallible)
//! - `session.delete_edge(EdgeId) -> bool` — `session/mod.rs:5092`
//! - `session.get_neighbors_outgoing_by_type(NodeId, &str) -> Vec<(NodeId, EdgeId)>` — `session/mod.rs`
//! - `session.begin_transaction() -> Result<()>` — `session/mod.rs:3883`
//! - `session.prepare_commit() -> Result<PreparedCommit<'_>>` — `session/mod.rs:4496`
//! - `PreparedCommit::commit(self) -> Result<EpochId>` — `transaction/prepared.rs:124`

#![allow(missing_docs)]

use grafeo::GrafeoDB;
use grafeo_loro::constants::TREE_EDGE_LABEL;
use grafeo_loro::error::GrafeoLoroError;
use grafeo_loro::schema::tree::sync_tree_move_to_grafeo;
use grafeo_loro::types::ids::NodeId;

/// Helper (L3 fills in): build a 3-node fixture `(root, mid, leaf)` wired as
/// `leaf --CHILD--> mid --CHILD--> root` (child→parent direction per
/// `apply_tree_move`). Returns `(root_id, mid_id, leaf_id)`.
fn build_chain_fixture(_db: &GrafeoDB) -> (NodeId, NodeId, NodeId) {
    todo!("P2T2-L3: create 3 nodes + 2 CHILD edges (leaf→mid, mid→root); return ids")
}

/// Move a leaf from parent A to parent B; assert the old `:CHILD` edge is
/// gone and the new `:CHILD` edge is present after `sync_tree_move_to_grafeo`
/// returns `Ok(())`. Acyclic invariant must hold (BFS up from leaf reaches
/// the new root, not the old one).
#[test]
#[ignore = "P2T2-L1 scaffold: L3 implements the body"]
fn tree_move_basic() {
    let _ = (TREE_EDGE_LABEL, build_chain_fixture, sync_tree_move_to_grafeo);
    todo!("P2T2-L3: build chain, move leaf mid→root, verify edges")
}

/// Reparenting a node under its own descendant must return
/// `Err(GrafeoLoroError::TreeMoveCreatesCycle { node_id, new_parent })`.
/// Grafeo 0.5.42 has no native acyclicity check (verified P2T2-L1), so the
/// bridge's `would_create_cycle` pre-check is the only defense.
#[test]
#[ignore = "P2T2-L1 scaffold: L3 implements the body"]
fn tree_move_cycle_rejected() {
    let _ = (TREE_EDGE_LABEL, build_chain_fixture, sync_tree_move_to_grafeo);
    // Anti-Goodhart: assert with `matches!` on the structured variant, NOT
    // on a substring of the error message.
    let _assert_cycle_variant = |err: &GrafeoLoroError| {
        assert!(
            matches!(err, GrafeoLoroError::TreeMoveCreatesCycle { .. }),
            "expected TreeMoveCreatesCycle, got {err:?}"
        );
    };
    todo!("P2T2-L3: build chain root→mid→leaf; try sync_tree_move_to_grafeo(root, root, mid) and assert Err(TreeMoveCreatesCycle)")
}

/// Moving the root (which has no parent edge to delete) must return `Err`.
/// Either `Bridge("no parent edge …")` or `TreeMoveCreatesCycle` is acceptable
/// depending on L3's choice — Devil should pin the exact variant.
#[test]
#[ignore = "P2T2-L1 scaffold: L3 implements the body"]
fn tree_move_root_to_leaf_rejected() {
    let _ = (TREE_EDGE_LABEL, build_chain_fixture, sync_tree_move_to_grafeo);
    todo!("P2T2-L3: build chain; call sync_tree_move_to_grafeo(root, root, leaf) — root has no parent edge — assert Err returned")
}

/// Idempotent move: calling `sync_tree_move_to_grafeo(db, node, A, A)` must
/// return `Ok(())` and leave the edge set unchanged (no duplicate edges,
/// no spurious deletion).
#[test]
#[ignore = "P2T2-L1 scaffold: L3 implements the body"]
fn tree_move_same_parent_noop() {
    let _ = (TREE_EDGE_LABEL, build_chain_fixture, sync_tree_move_to_grafeo);
    todo!("P2T2-L3: build chain leaf→A; call sync_tree_move_to_grafeo(db, leaf, A, A); assert Ok + edge set unchanged")
}
