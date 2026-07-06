//! Phase 2 Task 2 integration scaffold: 3-peer concurrent tree moves.
//!
//! Validates the implementation-plan §Phase 2 Task 2 integration requirement:
//! "Concurrent tree moves from 3 peers → consistent acyclic result."
//!
//! The scaffold is `#[ignore]` with a wired skeleton — L3 (P2T2-L3) fills in
//! the actual concurrent move sequence and final acyclicity assertion. The
//! test must verify:
//! 1. Three independent `LoroDoc` peers (each with a distinct `peer_id`)
//!    model three CRDT replicas. Each peer also holds a fresh `db.session()`
//!    handle against the SAME underlying `GrafeoDB` (MVCC isolation models
//!    concurrent peer transactions on the grafeo side).
//! 2. A shared tree fixture (≥4 nodes, e.g. `root → A → B → C`) is wired
//!    across the 3 peers.
//! 3. Each peer issues a `sync_tree_move_to_grafeo` call concurrently via
//!    `tokio::spawn` + `tokio::join!`.
//! 4. The final committed graph is acyclic (no `:CHILD`-edge cycle) — this
//!    is enforced by Serializable isolation (SSI), which catches the
//!    write-skew cycle at commit time (P2T2-DEVIL R3 option (c)).
//! 5. Convergence is deterministic regardless of peer interleaving
//!    (anti-Goodhart: test must NOT rely on a specific commit order).
//!
//! Scope note: this test exercises ONLY `sync_tree_move_to_grafeo` directly,
//! NOT the bridge subscriber (`init_loro_subscriber` does not generate
//! `LoroOp::TreeMove` events — wiring that is out of scope for Task 2 per
//! the L1 mandate). The 3 LoroDoc peers model the CRDT-side concurrency
//! surface; the 3 grafeo sessions model the grafeo-side MVCC/SSI concurrency.

#![allow(missing_docs)]

use std::sync::Arc;

use grafeo::GrafeoDB;
use grafeo_loro::error::Result;
use grafeo_loro::schema::tree::sync_tree_move_to_grafeo;
use grafeo_loro::types::ids::NodeId;
use loro::LoroDoc;

/// Three peers concurrently reparent nodes within a shared tree; the final
/// graph must be acyclic and converge regardless of commit order.
///
/// Grafeo Session API (verified P2T2-L1+L2): `db.session()` returns an
/// isolated MVCC session — three sessions on the same `GrafeoDB` model three
/// CRDT peers' concurrent write transactions. Sessions opened with
/// `Serializable` isolation (via `sync_tree_move_to_grafeo`'s
/// `begin_transaction_with_isolation(Serializable)`) catch write-skew cycles
/// at commit time; losing transactions return `Err(GrafeoLoroError::Grafeo(_))`
/// (SSI violation). The test must retry or assert graceful failure (NOT
/// silently drop the move).
#[tokio::test]
#[ignore = "P2T2-L2 scaffold: L3 implements the body"]
async fn concurrent_tree_moves_three_peers_converge_acyclic() {
    // ---- Fixture: shared GrafeoDB (Arc-shared across spawned tasks) + 3 LoroDoc peers ----
    let db = Arc::new(GrafeoDB::new_in_memory());

    let peer1 = LoroDoc::new();
    peer1.set_peer_id(1).unwrap();
    let peer2 = LoroDoc::new();
    peer2.set_peer_id(2).unwrap();
    let peer3 = LoroDoc::new();
    peer3.set_peer_id(3).unwrap();

    // TODO(P2T2-L3): build a shared tree fixture (e.g. root → A → B → C)
    //                 across the 3 LoroDoc peers via Loro CRDT sync
    //                 (export/import all_updates). Materialize the same tree
    //                 in grafeo via direct session.create_node + session.create_edge
    //                 calls (parent→child per P2T2-DEVIL R1) so the 3 concurrent
    //                 moves below operate on a non-empty graph.
    // TODO(P2T2-L3): capture the (n_i, old_p_i, new_p_i) triple each peer will
    //                 issue — design must NOT be adversarial in a way that
    //                 defeats SSI (see P2T2-DEVIL Q3 fallback (a) note).

    // ---- Concurrent moves via tokio::spawn + tokio::join! ----
    // Each task clones the Arc<GrafeoDB>, issues a `sync_tree_move_to_grafeo`
    // call, and returns the Result. Placeholder NodeIds (=0) are placeholders;
    // L3 must substitute real ids from the fixture above.
    let db1 = Arc::clone(&db);
    let h1: tokio::task::JoinHandle<Result<()>> = tokio::spawn(async move {
        // TODO(P2T2-L3): replace placeholders with real (n1, old_p1, new_p1) from the fixture.
        let n1: NodeId = NodeId::from(0);
        let old_p1: NodeId = NodeId::from(0);
        let new_p1: NodeId = NodeId::from(0);
        sync_tree_move_to_grafeo(&db1, n1, old_p1, new_p1)
    });
    let db2 = Arc::clone(&db);
    let h2: tokio::task::JoinHandle<Result<()>> = tokio::spawn(async move {
        // TODO(P2T2-L3): replace placeholders with real (n2, old_p2, new_p2) from the fixture.
        let n2: NodeId = NodeId::from(0);
        let old_p2: NodeId = NodeId::from(0);
        let new_p2: NodeId = NodeId::from(0);
        sync_tree_move_to_grafeo(&db2, n2, old_p2, new_p2)
    });
    let db3 = Arc::clone(&db);
    let h3: tokio::task::JoinHandle<Result<()>> = tokio::spawn(async move {
        // TODO(P2T2-L3): replace placeholders with real (n3, old_p3, new_p3) from the fixture.
        let n3: NodeId = NodeId::from(0);
        let old_p3: NodeId = NodeId::from(0);
        let new_p3: NodeId = NodeId::from(0);
        sync_tree_move_to_grafeo(&db3, n3, old_p3, new_p3)
    });

    // Await all three peer tasks. L3 must classify each Result:
    //   Ok(())                      → peer's move committed successfully
    //   Err(Grafeo(GrafeoError::*)) → peer lost an SSI / write-write conflict
    //                                 (acceptable retry-able failure; assert
    //                                 graceful, NOT silently dropped)
    //   Err(TreeMoveCreatesCycle)   → peer's pre-check rejected the move
    //                                 (only acceptable if the move would
    //                                 genuinely cycle against the final state)
    //   Err(Bridge(_))              → unexpected; fail the test
    let (r1, r2, r3) = tokio::join!(h1, h2, h3);
    let _ = (r1, r2, r3);

    // ---- Final acyclicity assertion ----
    // TODO(P2T2-L3): BFS up from every node via session.get_neighbors_incoming
    //                 (parent→child: incoming = parents of cur); assert each
    //                 walk terminates at a root, never revisits a node (cycle).
    // TODO(P2T2-L3): assert no peer silently dropped its move — every spawned
    //                 task's Result was either Ok OR a classified Err variant.
    // TODO(P2T2-L3): wire peer1/peer2/peer3 into the fixture + CRDT sync
    //                 (currently declared but not yet exercised — placeholder
    //                 for the Loro-side tree state replication).
    // L2 HACK: silences unused-variable warning until L3 implements the body.
    let _ = (&db, &peer1, &peer2, &peer3);
}
