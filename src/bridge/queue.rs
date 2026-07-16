//! Offline op-queue + lineage epoch — WASM-accessible (issue #4).
//!
//! Factored out of `sync_engine.rs` so browser consumers on
//! `wasm32-unknown-unknown` can use the real grafeo-loro queue without
//! enabling `batcher` (tokio::sync::mpsc) + `grafeo` (native ONNX/ort) +
//! `telemetry` (opentelemetry native) — all of which are WASM-incompatible.
//!
//! Gated by `feature = "bridge"` only. No tokio, no grafeo, no telemetry deps.

use crate::error::{GrafeoLoroError, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Lineage epoch key — bumped on every "cache wipe" event (server reset,
/// manual `wipe_cache()` call, or any lineage break the orchestrator
/// detects). Exchanged at sync handshake: if `local != remote`, the client
/// MUST wipe its cache before syncing.
///
/// Issue #3 sub-issue 5 / issue #4 (WASM accessibility).
pub type LineageEpoch = u64;

/// Error returned when the local and remote lineage epochs differ. Fatal-
/// by-design — the client MUST wipe its cache before retrying the handshake.
///
/// Issue #3 sub-issue 5 / issue #4 (WASM accessibility).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("lineage epoch mismatch: local={local} remote={remote}; client cache wipe required")]
pub struct EpochMismatchError {
    /// Local lineage epoch at handshake time.
    pub local: u64,
    /// Remote lineage epoch at handshake time.
    pub remote: u64,
}

/// Native offline op-queue with 10 MB cap (issue #3 sub-issue 5).
///
/// Pure in-memory FIFO (`Vec<Vec<u8>>` + byte-cap + retry counter). No tokio,
/// no grafeo, no telemetry deps. Factored out of `sync_engine.rs` in issue #4
/// so browser WASM consumers get the real grafeo-loro queue.
///
/// # Cap policy
///
/// 10 MB default cap ([`Self::DEFAULT_CAP`]). On enqueue past cap, returns
/// `Err(GrafeoLoroError::Bridge)` with a descriptive message — the
/// orchestrator's FFI layer surfaces this as a backpressure signal so the JS
/// side can flush the queue (or drop oldest) before enqueuing more.
///
/// # Retry hooks
///
/// `retry_bump` increments the internal counter; `reset_retry` zeroes it.
/// The orchestrator wires these to its exponential-backoff scheduler — the
/// queue itself does NOT implement backoff (it's a pure data structure).
pub struct OfflineOpQueue {
    ops: Vec<Vec<u8>>,
    total_bytes: usize,
    cap_bytes: usize,
    retry_count: u32,
}

impl OfflineOpQueue {
    /// Default cap: 10 MB (issue #3 sub-issue 5 mandate).
    pub const DEFAULT_CAP: usize = 10 * 1024 * 1024;

    /// Construct a fresh empty queue with the default 10 MB cap.
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            total_bytes: 0,
            cap_bytes: Self::DEFAULT_CAP,
            retry_count: 0,
        }
    }

    /// Construct a fresh empty queue with a custom cap.
    pub fn with_cap(cap_bytes: usize) -> Self {
        Self {
            ops: Vec::new(),
            total_bytes: 0,
            cap_bytes,
            retry_count: 0,
        }
    }

    /// Enqueue a serialized LoroOp. Returns `Err(GrafeoLoroError::Bridge)`
    /// if adding `op_bytes` would exceed the cap.
    pub fn enqueue(&mut self, op_bytes: Vec<u8>) -> Result<()> {
        let new_total = self
            .total_bytes
            .checked_add(op_bytes.len())
            .ok_or_else(|| {
                GrafeoLoroError::Bridge(format!(
                    "offline queue overflow: byte count overflow (current={}, adding={})",
                    self.total_bytes,
                    op_bytes.len()
                ))
            })?;
        if new_total > self.cap_bytes {
            return Err(GrafeoLoroError::Bridge(format!(
                "offline queue overflow: cap={} bytes, current={}, adding={} bytes",
                self.cap_bytes,
                self.total_bytes,
                op_bytes.len()
            )));
        }
        self.total_bytes = new_total;
        self.ops.push(op_bytes);
        Ok(())
    }

    /// Drain all queued ops in FIFO order. Resets `total_bytes` to 0 but
    /// does NOT reset `retry_count` (the orchestrator calls `reset_retry`
    /// separately after a successful flush).
    pub fn drain(&mut self) -> Vec<Vec<u8>> {
        let drained = std::mem::take(&mut self.ops);
        self.total_bytes = 0;
        drained
    }

    /// Number of ops currently queued.
    pub fn depth(&self) -> usize {
        self.ops.len()
    }

    /// Total bytes currently held (sum of `ops[i].len()`).
    pub fn bytes_used(&self) -> usize {
        self.total_bytes
    }

    /// Cap in bytes (10 MB default).
    pub fn cap_bytes(&self) -> usize {
        self.cap_bytes
    }

    /// Bump the retry counter, returning the new value. Saturating add.
    pub fn retry_bump(&mut self) -> u32 {
        self.retry_count = self.retry_count.saturating_add(1);
        self.retry_count
    }

    /// Reset the retry counter to 0. Called after a successful flush.
    pub fn reset_retry(&mut self) {
        self.retry_count = 0;
    }

    /// Current retry count (read accessor).
    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

impl Default for OfflineOpQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for OfflineOpQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OfflineOpQueue")
            .field("depth", &self.ops.len())
            .field("bytes_used", &self.total_bytes)
            .field("cap_bytes", &self.cap_bytes)
            .field("retry_count", &self.retry_count)
            .finish()
    }
}

/// Standalone WASM-accessible lineage epoch tracker (issue #4).
///
/// Pure `AtomicU64` — no tokio, no grafeo, no telemetry. Browser consumers
/// on `wasm32-unknown-unknown` use this directly; native `SyncEngine` holds
/// an `Arc<EpochTracker>` internally and delegates its `lineage_epoch` /
/// `check_epoch_match` / `wipe_cache` methods to this type.
///
/// # Semantics
///
/// - `current()` reads the epoch atomically.
/// - `check_match(remote)` returns `Err(EpochMismatchError)` if local != remote.
/// - `bump()` atomically increments and returns the new value.
/// - `wipe()` is a semantic alias for `bump()` — same effect, named for the
///   "cache wipe" use case (server reset, manual reset, lineage break).
pub struct EpochTracker {
    epoch: Arc<AtomicU64>,
}

impl EpochTracker {
    /// Construct a fresh tracker starting at epoch 0.
    pub fn new() -> Self {
        Self {
            epoch: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Current lineage epoch.
    pub fn current(&self) -> LineageEpoch {
        self.epoch.load(Ordering::Relaxed)
    }

    /// Check whether the remote's advertised epoch matches the local one.
    /// Returns `Err(EpochMismatchError)` on mismatch.
    pub fn check_match(&self, remote_epoch: LineageEpoch) -> std::result::Result<(), EpochMismatchError> {
        let local = self.epoch.load(Ordering::Relaxed);
        if local == remote_epoch {
            Ok(())
        } else {
            Err(EpochMismatchError {
                local,
                remote: remote_epoch,
            })
        }
    }

    /// Atomically bump the epoch and return the new value.
    pub fn bump(&self) -> LineageEpoch {
        self.epoch.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Semantic alias for `bump()` — bumps the epoch to signal a cache wipe
    /// event (server reset, manual reset, lineage break). Returns the new
    /// epoch.
    pub fn wipe(&self) -> LineageEpoch {
        self.bump()
    }
}

impl Default for EpochTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EpochTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EpochTracker")
            .field("current", &self.current())
            .finish()
    }
}

impl Clone for EpochTracker {
    fn clone(&self) -> Self {
        Self {
            epoch: Arc::clone(&self.epoch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_default_cap_is_10mb() {
        assert_eq!(OfflineOpQueue::DEFAULT_CAP, 10 * 1024 * 1024);
        let q = OfflineOpQueue::new();
        assert_eq!(q.cap_bytes(), 10 * 1024 * 1024);
    }

    #[test]
    fn queue_enqueue_drain_fifo() {
        let mut q = OfflineOpQueue::new();
        q.enqueue(b"op1".to_vec()).unwrap();
        q.enqueue(b"op2".to_vec()).unwrap();
        q.enqueue(b"op3".to_vec()).unwrap();
        assert_eq!(q.depth(), 3);
        assert_eq!(q.bytes_used(), 9);
        let drained = q.drain();
        assert_eq!(drained, vec![b"op1".to_vec(), b"op2".to_vec(), b"op3".to_vec()]);
        assert_eq!(q.depth(), 0);
        assert_eq!(q.bytes_used(), 0);
        assert!(q.is_empty());
    }

    #[test]
    fn queue_cap_enforced() {
        let mut q = OfflineOpQueue::with_cap(10);
        q.enqueue(b"12345".to_vec()).unwrap(); // 5 bytes
        q.enqueue(b"12345".to_vec()).unwrap(); // 10 bytes total — at cap, OK
        let result = q.enqueue(b"x".to_vec()); // would exceed cap
        assert!(result.is_err());
        assert_eq!(q.depth(), 2); // third op rejected
    }

    #[test]
    fn queue_retry_hooks() {
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
    fn queue_retry_saturates() {
        let mut q = OfflineOpQueue::new();
        q.retry_count = u32::MAX;
        assert_eq!(q.retry_bump(), u32::MAX); // saturating
    }

    #[test]
    fn epoch_tracker_basic() {
        let t = EpochTracker::new();
        assert_eq!(t.current(), 0);
        assert!(t.check_match(0).is_ok());
        assert!(t.check_match(1).is_err());
    }

    #[test]
    fn epoch_tracker_bump_and_wipe() {
        let t = EpochTracker::new();
        assert_eq!(t.bump(), 1);
        assert_eq!(t.current(), 1);
        assert_eq!(t.wipe(), 2);
        assert_eq!(t.current(), 2);
    }

    #[test]
    fn epoch_tracker_check_match_after_bump() {
        let t = EpochTracker::new();
        t.bump(); // now 1
        t.bump(); // now 2
        assert!(t.check_match(2).is_ok());
        assert!(t.check_match(3).is_err());
        let err = t.check_match(3).unwrap_err();
        assert_eq!(err.local, 2);
        assert_eq!(err.remote, 3);
    }

    #[test]
    fn epoch_tracker_clone_shares_state() {
        let t = EpochTracker::new();
        let t2 = t.clone();
        t.bump();
        assert_eq!(t2.current(), 1); // shared via Arc
    }
}
