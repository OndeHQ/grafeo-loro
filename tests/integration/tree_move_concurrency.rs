//! Phase 2 Task 2 integration scaffold: 3-peer concurrent tree moves.
//!
//! Validates the implementation-plan §Phase 2 Task 2 integration requirement:
//! "Concurrent tree moves from 3 peers → consistent acyclic result."
//!
//! The scaffold is `#[ignore]` with a `todo!()` body — L3 (P2T2-L3) fills it
//! in. The test must verify:
//! 1. Three independent sessions each issue `sync_tree_move_to_grafeo` calls
//!    concurrently (e.g. reparenting siblings under each other).
//! 2. The final committed graph is acyclic (no `:CHILD` edge cycle).
//! 3. The convergence is deterministic regardless of peer interleaving
//!    (anti-Goodhart: test must NOT rely on a specific commit order).
//!
//! Scope note: this test exercises ONLY `sync_tree_move_to_grafeo` directly,
//! NOT the bridge subscriber (`init_loro_subscriber` does not generate
//! `LoroOp::TreeMove` events — wiring that is out of scope for Task 2 per
//! the L1 mandate). The 3-peer simulation uses three `GrafeoDB::session()`
//! handles against the SAME underlying `GrafeoDB` (MVCC isolation) to model
//! concurrent peer transactions.

#![allow(missing_docs)]

use grafeo::GrafeoDB;
use grafeo_loro::schema::tree::sync_tree_move_to_grafeo;

/// Three peers concurrently reparent nodes within a shared tree; the final
/// graph must be acyclic and converge regardless of commit order.
///
/// Grafeo Session API (verified P2T2-L1): `db.session()` returns an isolated
/// MVCC session — three sessions on the same `GrafeoDB` model three CRDT
/// peers. Sessions that lose a write-write conflict at `prepare_commit` /
/// `commit` time return `Err(GrafeoLoroError::Grafeo(_))`; the test must
/// retry or assert graceful failure (NOT silently drop the move).
#[tokio::test]
#[ignore = "P2T2-L1 scaffold: L3 implements the body"]
async fn concurrent_tree_moves_three_peers_converge_acyclic() {
    let _ = (sync_tree_move_to_grafeo, GrafeoDB::new_in_memory);
    // TODO(P2T2-L3): build a shared fixture tree (≥4 nodes), spawn 3 tasks
    //                 each holding a fresh `db.session()` and issuing
    //                 `sync_tree_move_to_grafeo` calls in parallel; await all;
    //                 then assert (a) all returned Ok OR returned
    //                 Grafeo(GrafeoError::WriteWriteConflict) (acceptable
    //                 retry-able failure), (b) the final graph has no
    //                 `:CHILD`-edge cycle (BFS up from every node reaches a
    //                 root, never itself), (c) the cycle-pre-check
    //                 invariant held (no peer succeeded in creating a cycle).
    todo!("P2T2-L3: spawn 3 peer sessions, issue concurrent moves, verify acyclic convergence")
}
