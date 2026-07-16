//! Integration tests for issue #3 sub-issues 5 + 7.
//!
//! Tests:
//! - `cycle_detection_direct` — A→B→A rejected (invariant I14).
//! - `cycle_detection_deep` — A→B→C→A rejected (invariant I14).
//! - `root_tracker_incremental` — insertions + removals maintain correct
//!   root set (replaces O(N) DAG walk).
//! - `offline_queue_cap` — enqueue past 10 MB returns Err (sub-issue 5).
//! - `epoch_mismatch_detected` — `check_epoch_match` returns Err on
//!   mismatch (sub-issue 5).
//!
//! # Feature gating
//!
//! The graph-side tests (`cycle_*`, `root_tracker_*`) run with `bridge,tree`
//! features — the minimal useful combo. The sync-side tests
//! (`offline_queue_cap`, `epoch_mismatch_detected`) require the full
//! `batcher + grafeo + telemetry` feature set (because `SyncEngine` lives
//! in the gated `bridge::sync_engine` module). They are conditionally
//! compiled so `cargo test --no-default-features --features bridge,tree
//! --test graph_sync` still produces a working test binary.
//!
//! To run ALL tests:
//! ```sh
//! cargo test --no-default-features --features full --test graph_sync
//! ```

#![cfg(feature = "bridge")]

use grafeo_loro::schema::{validate_acyclic, CycleGuard, EdgeSpec, RootTracker};

// ============================================================================
// Sub-issue 7: Graph invariants — acyclicity, root-tracking
// ============================================================================

#[test]
fn cycle_detection_direct() {
    // A→B then B→A must be rejected.
    let mut g = CycleGuard::new();
    // B is a child of A.
    assert!(g.apply_move("b", "a").is_ok());
    // Trying to make A a child of B (closing A→B→A cycle) must fail.
    let err = g.apply_move("a", "b").unwrap_err();
    assert_eq!(err.node, "a");
    assert_eq!(err.new_parent, "b");
}

#[test]
fn cycle_detection_deep() {
    // A→B→C then C→A must be rejected (depth-3 cycle).
    let mut g = CycleGuard::new();
    assert!(g.apply_move("b", "a").is_ok());
    assert!(g.apply_move("c", "b").is_ok());
    // Now C is a child of B which is a child of A. Trying to make A a
    // child of C would close the A→B→C→A cycle.
    let err = g.apply_move("a", "c").unwrap_err();
    assert_eq!(err.node, "a");
    assert_eq!(err.new_parent, "c");

    // Also exercise the batch `validate_acyclic` API on the same shape —
    // pre-commit hook for graph operations.
    let cycle_edges = vec![
        EdgeSpec::new("a", "b", "CHILD"),
        EdgeSpec::new("b", "c", "CHILD"),
        EdgeSpec::new("c", "a", "CHILD"),
    ];
    let err = validate_acyclic(&cycle_edges).unwrap_err();
    // The offending back-edge identifies a node + parent pair where the
    // cycle closes. The exact pair depends on which root DFS starts from
    // (HashSet iteration order is non-deterministic) — but both must be
    // members of {a, b, c} and they must be distinct.
    let members = ["a", "b", "c"];
    assert!(
        members.contains(&err.node.as_str()),
        "node {} should be a member of the cycle",
        err.node
    );
    assert!(
        members.contains(&err.new_parent.as_str()),
        "new_parent {} should be a member of the cycle",
        err.new_parent
    );
    assert_ne!(err.node, err.new_parent);
}

#[test]
fn root_tracker_incremental() {
    // Insertions + removals maintain correct root set — O(1) per
    // mutation, replacing the downstream O(N) DAG walk the issue body
    // calls out as "kills 60fps".
    let mut t = RootTracker::new();

    // Register 4 nodes — all start as roots.
    for n in ["a", "b", "c", "d"] {
        t.register_node(n);
    }
    assert_eq!(t.root_count(), 4);

    // a→b→c→d — only `a` should remain a root.
    t.on_edge_inserted("b", "a");
    t.on_edge_inserted("c", "b");
    t.on_edge_inserted("d", "c");
    assert!(t.is_root("a"));
    assert!(!t.is_root("b"));
    assert!(!t.is_root("c"));
    assert!(!t.is_root("d"));
    assert_eq!(t.root_count(), 1);

    // Remove edge b→c — `c` becomes a root again (in tree mode, removing
    // b→c disconnects c's subtree from a).
    t.on_edge_removed("c", "b");
    assert!(t.is_root("c"));

    // Re-insert edge b→c — `c` is no longer a root.
    t.on_edge_inserted("c", "b");
    assert!(!t.is_root("c"));

    // Unregister `a` — `a` is removed from the tracker.
    t.unregister_node("a");
    assert!(!t.is_root("a"));
}

// ============================================================================
// Sub-issue 7: Text bijection (invariant I11)
// ============================================================================

#[test]
fn text_bijection_drift_detected() {
    // Every loro_key ↔ NodeId pair must be bijective (invariant I11).
    use grafeo_loro::bridge::grafeo_tx::{validate_text_bijection, BijectionError};
    use grafeo_loro::bridge::BridgeMaps;
    use grafeo_loro::types::ids::NodeId;

    // Constructor helper — `grafeo::NodeId` has `new(u64)` while the
    // no-grafeo fallback `NodeId(pub u64)` uses tuple construction. This
    // lets the test run under either feature combo.
    #[cfg(feature = "grafeo")]
    fn nid(n: u64) -> NodeId {
        NodeId::new(n)
    }
    #[cfg(not(feature = "grafeo"))]
    fn nid(n: u64) -> NodeId {
        NodeId(n)
    }

    let maps = BridgeMaps::new();
    // Healthy bijection: k1 → id 1, k2 → id 2.
    maps.insert_node("k1".to_string(), nid(1));
    maps.insert_node("k2".to_string(), nid(2));
    assert!(validate_text_bijection(&maps).is_ok());

    // Drift: k3 maps to id 2 in the forward direction, but the inverse
    // (id 2 → k3) overwrites the prior (id 2 → k2) entry — DuplicateId.
    maps.insert_node("k3".to_string(), nid(2));
    match validate_text_bijection(&maps) {
        Err(BijectionError::DuplicateId { id, .. }) => {
            assert_eq!(id, nid(2));
        }
        other => panic!("expected DuplicateId, got {other:?}"),
    }
}

// ============================================================================
// Sub-issue 5: Sync — lineage epochs + offline op-queue
// ============================================================================
//
// These tests require the full `SyncEngine` machinery (gated by
// `batcher + grafeo + telemetry`). They are conditionally compiled so the
// `--features bridge,tree` workflow command still builds a working test
// binary (running only the graph-side tests above).
//
// TODO(orchestrator): re-export `EpochMismatchError`, `LineageEpoch`,
// `OfflineOpQueue` from `src/bridge/mod.rs` so callers can use
// `grafeo_loro::bridge::*` instead of `grafeo_loro::bridge::sync_engine::*`.

#[cfg(all(feature = "batcher", feature = "grafeo", feature = "telemetry"))]
mod sync_engine_tests {
    use grafeo::GrafeoDB;
    use grafeo_loro::bridge::sync_engine::{
        EpochMismatchError, LineageEpoch, OfflineOpQueue, SyncEngine,
    };
    use grafeo_loro::error::GrafeoLoroError;
    use loro::LoroDoc;
    use parking_lot::RwLock;
    use std::sync::Arc;

    fn fresh_engine() -> SyncEngine {
        // Fresh in-memory grafeo + loro doc + engine.
        let grafeo_db = Arc::new(GrafeoDB::new_in_memory());
        let loro_doc = Arc::new(RwLock::new(LoroDoc::new()));
        let (engine, _inbound_rx, _outbound_rx) = SyncEngine::new(grafeo_db, loro_doc);
        engine
    }

    #[test]
    fn offline_queue_cap() {
        // Enqueue past the 10 MB cap returns Err.
        let mut q = OfflineOpQueue::new();
        assert_eq!(q.cap_bytes(), 10 * 1024 * 1024);

        // Enqueue a 5 MB op — OK.
        let five_mb = vec![0u8; 5 * 1024 * 1024];
        assert!(q.enqueue(five_mb).is_ok());
        assert_eq!(q.depth(), 1);
        assert_eq!(q.bytes_used(), 5 * 1024 * 1024);

        // Enqueue another 5 MB op — OK (exactly at cap).
        let five_mb_2 = vec![0u8; 5 * 1024 * 1024];
        assert!(q.enqueue(five_mb_2).is_ok());
        assert_eq!(q.depth(), 2);
        assert_eq!(q.bytes_used(), 10 * 1024 * 1024);

        // Enqueue 1 more byte — Err (cap exceeded).
        let one_byte = vec![0u8; 1];
        let err = q.enqueue(one_byte).unwrap_err();
        assert!(matches!(err, GrafeoLoroError::Bridge(_)));
        assert_eq!(q.depth(), 2); // unchanged
        assert_eq!(q.bytes_used(), 10 * 1024 * 1024);

        // Drain returns all ops in FIFO order + resets bytes.
        let drained = q.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(q.depth(), 0);
        assert_eq!(q.bytes_used(), 0);
    }

    #[test]
    fn offline_queue_retry_hooks() {
        // retry_bump increments, reset_retry zeroes.
        let mut q = OfflineOpQueue::new();
        assert_eq!(q.retry_count(), 0);
        assert_eq!(q.retry_bump(), 1);
        assert_eq!(q.retry_bump(), 2);
        assert_eq!(q.retry_bump(), 3);
        assert_eq!(q.retry_count(), 3);
        q.reset_retry();
        assert_eq!(q.retry_count(), 0);
    }

    #[test]
    fn epoch_mismatch_detected() {
        let engine = fresh_engine();

        // Initial epoch is 0.
        let initial: LineageEpoch = engine.lineage_epoch();
        assert_eq!(initial, 0);

        // Remote advertises epoch 0 — match.
        assert!(engine.check_epoch_match(0).is_ok());

        // Remote advertises epoch 1 — mismatch.
        let err = engine.check_epoch_match(1).unwrap_err();
        assert_eq!(err.local, 0);
        assert_eq!(err.remote, 1);
        // Verify the error matches the spec format (local, remote fields).
        assert_eq!(
            err,
            EpochMismatchError {
                local: 0,
                remote: 1,
            }
        );

        // wipe_cache bumps local epoch to 1 + drains the offline queue.
        let new_epoch = engine.wipe_cache();
        assert_eq!(new_epoch, 1);
        assert_eq!(engine.lineage_epoch(), 1);

        // After wipe, epoch 1 matches.
        assert!(engine.check_epoch_match(1).is_ok());
        // Epoch 2 still mismatches.
        assert!(engine.check_epoch_match(2).is_err());
    }

    #[test]
    fn offline_queue_accessors_on_engine() {
        let engine = fresh_engine();

        // Fresh engine has empty queue.
        assert_eq!(engine.offline_queue_depth(), 0);
        assert_eq!(engine.offline_queue_bytes(), 0);
        assert_eq!(engine.offline_queue_cap(), 10 * 1024 * 1024);

        // Enqueue via engine method.
        let op = vec![1, 2, 3, 4];
        assert!(engine.enqueue_offline_op(op.clone()).is_ok());
        assert_eq!(engine.offline_queue_depth(), 1);
        assert_eq!(engine.offline_queue_bytes(), 4);

        // Drain via engine method.
        let drained = engine.drain_offline_queue();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0], op);
        assert_eq!(engine.offline_queue_depth(), 0);
        assert_eq!(engine.offline_queue_bytes(), 0);

        // Retry hooks on engine.
        assert_eq!(engine.offline_retry_bump(), 1);
        assert_eq!(engine.offline_retry_count(), 1);
        engine.reset_offline_retry();
        assert_eq!(engine.offline_retry_count(), 0);
    }

    #[test]
    fn wipe_cache_drains_offline_queue() {
        let engine = fresh_engine();
        engine.enqueue_offline_op(vec![1, 2, 3]).unwrap();
        engine.enqueue_offline_op(vec![4, 5, 6]).unwrap();
        assert_eq!(engine.offline_queue_depth(), 2);

        engine.offline_retry_bump();
        assert_eq!(engine.offline_retry_count(), 1);

        // wipe_cache drains the queue + resets retry.
        let new_epoch = engine.wipe_cache();
        assert_eq!(new_epoch, 1);
        assert_eq!(engine.offline_queue_depth(), 0);
        assert_eq!(engine.offline_queue_bytes(), 0);
        assert_eq!(engine.offline_retry_count(), 0);
    }
}
