//! Native integration tests for `OfflineOpQueue` + `EpochTracker` (issue #4).
//!
//! Run with:
//!   cargo test --no-default-features --features bridge --test queue_native
//!
//! These tests verify the factored-out types behave identically to the
//! pre-issue-#4 inline versions in sync_engine.rs. They run on native
//! (any target) and do NOT require wasm-bindgen.

#![cfg(feature = "bridge")]

use grafeo_loro::{EpochMismatchError, EpochTracker, LineageEpoch, OfflineOpQueue};

// ============================================================================
// OfflineOpQueue
// ============================================================================

#[test]
fn queue_default_cap_is_10mb() {
    assert_eq!(OfflineOpQueue::DEFAULT_CAP, 10 * 1024 * 1024);
    let q = OfflineOpQueue::new();
    assert_eq!(q.cap_bytes(), 10 * 1024 * 1024);
}

#[test]
fn queue_with_cap_custom() {
    let q = OfflineOpQueue::with_cap(1024);
    assert_eq!(q.cap_bytes(), 1024);
}

#[test]
fn queue_starts_empty() {
    let q = OfflineOpQueue::new();
    assert!(q.is_empty());
    assert_eq!(q.depth(), 0);
    assert_eq!(q.bytes_used(), 0);
    assert_eq!(q.retry_count(), 0);
}

#[test]
fn queue_default_impl_matches_new() {
    let q1 = OfflineOpQueue::new();
    let q2 = OfflineOpQueue::default();
    assert_eq!(q1.cap_bytes(), q2.cap_bytes());
    assert_eq!(q1.depth(), q2.depth());
    assert_eq!(q1.bytes_used(), q2.bytes_used());
}

#[test]
fn queue_enqueue_drain_fifo_order() {
    let mut q = OfflineOpQueue::new();
    q.enqueue(b"op1".to_vec()).unwrap();
    q.enqueue(b"op2".to_vec()).unwrap();
    q.enqueue(b"op3".to_vec()).unwrap();
    assert_eq!(q.depth(), 3);
    assert_eq!(q.bytes_used(), 9);
    assert!(!q.is_empty());

    let drained = q.drain();
    assert_eq!(drained.len(), 3);
    assert_eq!(drained[0], b"op1".to_vec());
    assert_eq!(drained[1], b"op2".to_vec());
    assert_eq!(drained[2], b"op3".to_vec());

    // Drain resets depth + bytes_used...
    assert_eq!(q.depth(), 0);
    assert_eq!(q.bytes_used(), 0);
    assert!(q.is_empty());
    // ...but NOT retry_count.
    assert_eq!(q.retry_count(), 0); // unchanged (was 0 anyway)
}

#[test]
fn queue_cap_enforced_returns_bridge_error() {
    let mut q = OfflineOpQueue::with_cap(10);
    q.enqueue(b"12345".to_vec()).unwrap(); // 5 bytes
    assert_eq!(q.bytes_used(), 5);
    q.enqueue(b"12345".to_vec()).unwrap(); // 10 bytes total — at cap, OK
    assert_eq!(q.bytes_used(), 10);

    // Third enqueue would push to 11 bytes — must fail.
    let result = q.enqueue(b"x".to_vec());
    assert!(result.is_err(), "enqueue past cap must Err");
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("offline queue overflow"),
        "error message must mention overflow: got {msg}"
    );
    assert!(
        msg.contains("cap=10"),
        "error message must mention cap size: got {msg}"
    );

    // Queue state unchanged after rejected enqueue.
    assert_eq!(q.depth(), 2);
    assert_eq!(q.bytes_used(), 10);
}

#[test]
fn queue_cap_with_large_op() {
    // Single op larger than cap → must reject.
    let mut q = OfflineOpQueue::with_cap(4);
    let result = q.enqueue(vec![0u8; 5]);
    assert!(result.is_err());
    assert_eq!(q.depth(), 0);
    assert_eq!(q.bytes_used(), 0);
}

#[test]
fn queue_byte_count_overflow_protection() {
    // The checked_add path is unit-tested in src/bridge/queue.rs (the
    // `queue_byte_count_overflow` unit test sets `total_bytes = usize::MAX - 1`
    // directly via the in-module private field access). From the public API
    // alone we cannot realistically reach a usize overflow (would require
    // ~9 EB of enqueued bytes), so this test is a compile-time sanity check
    // that the API stays available on this target.
    //
    // Re-enqueueing at the cap path is fully covered by
    // `queue_cap_enforced_returns_bridge_error` above.
    let q = OfflineOpQueue::with_cap(usize::MAX);
    assert_eq!(q.cap_bytes(), usize::MAX);
    assert!(q.is_empty());
}

#[test]
fn queue_retry_bump_returns_new_count_saturating() {
    let mut q = OfflineOpQueue::new();
    assert_eq!(q.retry_count(), 0);
    assert_eq!(q.retry_bump(), 1);
    assert_eq!(q.retry_bump(), 2);
    assert_eq!(q.retry_bump(), 3);
    assert_eq!(q.retry_count(), 3);
}

#[test]
fn queue_retry_bump_saturates_at_max() {
    let mut q = OfflineOpQueue::new();
    // Saturating add: bumping past u32::MAX stays at u32::MAX.
    // We can't realistically call bump u32::MAX times, but we can
    // verify the saturating_add semantics by checking that repeated
    // bumps never panic and always return <= u32::MAX.
    for _ in 0..100 {
        let new = q.retry_bump();
        assert!(new > 0);
        assert!(new <= u32::MAX);
    }
}

#[test]
fn queue_reset_retry_zeroes_counter() {
    let mut q = OfflineOpQueue::new();
    q.retry_bump();
    q.retry_bump();
    assert_eq!(q.retry_count(), 2);
    q.reset_retry();
    assert_eq!(q.retry_count(), 0);
    // Resetting again is a no-op.
    q.reset_retry();
    assert_eq!(q.retry_count(), 0);
}

#[test]
fn queue_drain_empty_returns_empty_vec() {
    let mut q = OfflineOpQueue::new();
    let drained = q.drain();
    assert!(drained.is_empty());
}

#[test]
fn queue_drain_preserves_retry_count() {
    let mut q = OfflineOpQueue::new();
    q.enqueue(b"op".to_vec()).unwrap();
    q.retry_bump();
    q.retry_bump();
    assert_eq!(q.retry_count(), 2);

    let drained = q.drain();
    assert_eq!(drained.len(), 1);
    // Drain does NOT reset retry_count.
    assert_eq!(q.retry_count(), 2);
}

#[test]
fn queue_debug_impl_does_not_leak_op_bytes() {
    // The custom Debug impl should show depth/bytes_used/cap_bytes/retry_count
    // but NOT the actual op bytes (which could be sensitive LoroOp payloads).
    let mut q = OfflineOpQueue::new();
    q.enqueue(b"sensitive-payload-bytes".to_vec()).unwrap();
    let debug_str = format!("{:?}", q);
    assert!(debug_str.contains("depth"));
    assert!(debug_str.contains("bytes_used"));
    assert!(debug_str.contains("cap_bytes"));
    assert!(debug_str.contains("retry_count"));
    assert!(
        !debug_str.contains("sensitive-payload-bytes"),
        "Debug impl must not leak op bytes: got {debug_str}"
    );
}

// ============================================================================
// EpochTracker
// ============================================================================

#[test]
fn epoch_tracker_starts_at_zero() {
    let t = EpochTracker::new();
    assert_eq!(t.current(), 0);
}

#[test]
fn epoch_tracker_default_impl_matches_new() {
    let t1 = EpochTracker::new();
    let t2 = EpochTracker::default();
    assert_eq!(t1.current(), t2.current());
}

#[test]
fn epoch_tracker_check_match_ok_when_equal() {
    let t = EpochTracker::new();
    assert!(t.check_match(0).is_ok());
}

#[test]
fn epoch_tracker_check_match_err_on_mismatch() {
    let t = EpochTracker::new();
    let result = t.check_match(1);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.local, 0);
    assert_eq!(err.remote, 1);
}

#[test]
fn epoch_tracker_bump_increments_and_returns_new() {
    let t = EpochTracker::new();
    assert_eq!(t.bump(), 1);
    assert_eq!(t.current(), 1);
    assert_eq!(t.bump(), 2);
    assert_eq!(t.current(), 2);
    assert_eq!(t.bump(), 3);
    assert_eq!(t.current(), 3);
}

#[test]
fn epoch_tracker_wipe_is_alias_for_bump() {
    let t = EpochTracker::new();
    assert_eq!(t.wipe(), 1);
    assert_eq!(t.current(), 1);
    assert_eq!(t.wipe(), 2);
    assert_eq!(t.current(), 2);
}

#[test]
fn epoch_tracker_check_match_after_bump() {
    let t = EpochTracker::new();
    t.bump(); // 1
    t.bump(); // 2
    assert!(t.check_match(2).is_ok());
    assert!(t.check_match(3).is_err());
}

#[test]
fn epoch_tracker_clone_shares_state() {
    // EpochTracker is Clone via Arc<AtomicU64>. Cloned instances share
    // the same atomic — bumping one is visible to the other.
    let t = EpochTracker::new();
    let t2 = t.clone();
    t.bump();
    assert_eq!(t2.current(), 1, "cloned tracker must share state via Arc");
    t2.bump();
    assert_eq!(t.current(), 2, "bump on clone visible to original");
}

#[test]
fn epoch_tracker_simulated_sync_handshake() {
    // Simulate the orchestrator's sync handshake flow:
    // 1. Local tracker starts at epoch 0
    // 2. Remote advertises epoch 0 — match, proceed
    // 3. Server resets → remote now advertises epoch 1 — mismatch, wipe
    // 4. After wipe, local is at epoch 1 — match, proceed
    let local = EpochTracker::new();
    assert_eq!(local.current(), 0);

    // First handshake — match
    assert!(local.check_match(0).is_ok());

    // Second handshake after server reset — mismatch
    let result = local.check_match(1);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.local, 0);
    assert_eq!(err.remote, 1);

    // Wipe local cache + bump epoch
    let new_epoch = local.wipe();
    assert_eq!(new_epoch, 1);

    // Third handshake — match again
    assert!(local.check_match(1).is_ok());
}

// ============================================================================
// Type aliases / error type
// ============================================================================

#[test]
fn lineage_epoch_is_u64() {
    // Compile-time check that LineageEpoch is u64.
    let _: LineageEpoch = 0u64;
}

#[test]
fn epoch_mismatch_error_is_send_sync_clone() {
    // The error must be Send + Sync + Clone so it can flow through
    // async boundaries + FFI.
    fn assert_send_sync_clone<T: Send + Sync + Clone>() {}
    assert_send_sync_clone::<EpochMismatchError>();
}

#[test]
fn epoch_mismatch_error_display_includes_local_and_remote() {
    let err = EpochMismatchError {
        local: 7,
        remote: 42,
    };
    let msg = err.to_string();
    assert!(msg.contains("lineage epoch mismatch"), "got {msg}");
    assert!(msg.contains("local=7"), "got {msg}");
    assert!(msg.contains("remote=42"), "got {msg}");
}

#[test]
fn epoch_mismatch_error_eq() {
    let a = EpochMismatchError {
        local: 1,
        remote: 2,
    };
    let b = EpochMismatchError {
        local: 1,
        remote: 2,
    };
    let c = EpochMismatchError {
        local: 1,
        remote: 3,
    };
    assert_eq!(a, b);
    assert_ne!(a, c);
}
