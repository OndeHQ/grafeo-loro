//! WASM integration tests for `OfflineOpQueue` + `EpochTracker` (issue #4).
//!
//! Run with:
//!   cargo test --target wasm32-unknown-unknown --no-default-features \
//!     --features bridge,tree,wasm --test queue_wasm
//!
//! These tests verify the factored-out types are reachable and behave
//! correctly on `wasm32-unknown-unknown` — the whole point of issue #4.

#![cfg(feature = "bridge")]
#![cfg(target_arch = "wasm32")]

use grafeo_loro::{EpochMismatchError, EpochTracker, OfflineOpQueue};

// Initialize the panic hook so test failures surface in the browser console
// (or node test runner) instead of silently aborting.
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

// ============================================================================
// OfflineOpQueue — WASM reachability + behavior
// ============================================================================

#[wasm_bindgen_test::wasm_bindgen_test]
fn wasm_queue_constructible() {
    let q = OfflineOpQueue::new();
    assert_eq!(q.depth(), 0);
    assert_eq!(q.bytes_used(), 0);
    assert_eq!(q.cap_bytes(), 10 * 1024 * 1024);
    assert!(q.is_empty());
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn wasm_queue_with_cap_custom() {
    let q = OfflineOpQueue::with_cap(2048);
    assert_eq!(q.cap_bytes(), 2048);
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn wasm_queue_enqueue_drain_fifo() {
    let mut q = OfflineOpQueue::new();
    q.enqueue(b"op1".to_vec()).unwrap();
    q.enqueue(b"op2".to_vec()).unwrap();
    q.enqueue(b"op3".to_vec()).unwrap();
    assert_eq!(q.depth(), 3);
    assert_eq!(q.bytes_used(), 9);

    let drained = q.drain();
    assert_eq!(drained.len(), 3);
    assert_eq!(drained[0], b"op1".to_vec());
    assert_eq!(drained[1], b"op2".to_vec());
    assert_eq!(drained[2], b"op3".to_vec());

    assert_eq!(q.depth(), 0);
    assert_eq!(q.bytes_used(), 0);
    assert!(q.is_empty());
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn wasm_queue_cap_enforced() {
    let mut q = OfflineOpQueue::with_cap(10);
    q.enqueue(b"12345".to_vec()).unwrap();
    q.enqueue(b"12345".to_vec()).unwrap();
    let result = q.enqueue(b"x".to_vec());
    assert!(result.is_err());
    assert_eq!(q.depth(), 2);
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn wasm_queue_retry_hooks() {
    let mut q = OfflineOpQueue::new();
    assert_eq!(q.retry_bump(), 1);
    assert_eq!(q.retry_bump(), 2);
    assert_eq!(q.retry_count(), 2);
    q.reset_retry();
    assert_eq!(q.retry_count(), 0);
}

// ============================================================================
// EpochTracker — WASM reachability + behavior
// ============================================================================

#[wasm_bindgen_test::wasm_bindgen_test]
fn wasm_epoch_tracker_constructible() {
    let t = EpochTracker::new();
    assert_eq!(t.current(), 0);
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn wasm_epoch_tracker_bump_wipe() {
    let t = EpochTracker::new();
    assert_eq!(t.bump(), 1);
    assert_eq!(t.wipe(), 2);
    assert_eq!(t.current(), 2);
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn wasm_epoch_tracker_check_match() {
    let t = EpochTracker::new();
    assert!(t.check_match(0).is_ok());
    assert!(t.check_match(1).is_err());
    let err = t.check_match(1).unwrap_err();
    assert_eq!(err.local, 0);
    assert_eq!(err.remote, 1);
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn wasm_epoch_tracker_clone_shares_state() {
    let t = EpochTracker::new();
    let t2 = t.clone();
    t.bump();
    assert_eq!(t2.current(), 1);
}

// ============================================================================
// Sync handshake simulation on WASM (the issue #4 use case)
// ============================================================================

#[wasm_bindgen_test::wasm_bindgen_test]
fn wasm_simulated_sync_handshake() {
    // Browser consumer flow:
    // 1. Local epoch tracker starts at 0
    // 2. Remote advertises epoch 0 — match, proceed with sync
    // 3. Server resets → remote advertises epoch 1 — mismatch
    // 4. Browser wipes local cache, bumps epoch
    // 5. Re-handshake — match
    let local = EpochTracker::new();
    assert!(local.check_match(0).is_ok());

    // Server reset detected:
    let err = local.check_match(1).unwrap_err();
    assert_eq!(err.local, 0);
    assert_eq!(err.remote, 1);

    // Wipe + bump:
    let new_epoch = local.wipe();
    assert_eq!(new_epoch, 1);

    // Re-handshake succeeds:
    assert!(local.check_match(1).is_ok());
}

// Keep the import lint-clean if EpochMismatchError is otherwise unused
// (some feature combinations might trigger a dead-code notice).
#[allow(dead_code)]
const _: fn() = || {
    let _ = EpochMismatchError {
        local: 0,
        remote: 0,
    };
};
