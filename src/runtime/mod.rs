//! Trait-abstracted async runtime (issue #1 item 2).
//!
//! `tokio::sync::mpsc` cannot run in browser WASM (no multi-threaded runtime,
//! no reactor, no `mio`-backed I/O). To let Onde depend on a single crate
//! across native + WASM targets, we abstract the channel behind a
//! [`Mailbox<T>`] trait.
//!
//! - **Native**: enable the `batcher` feature → get [`TokioMailbox<T>`], a
//!   thin wrapper over `tokio::sync::mpsc::channel`.
//! - **WASM**: leave `batcher` off and provide your own impl — e.g.
//!   `WasmMessageChannelMailbox` backed by `wasm-bindgen-futures` +
//!   `web-sys::MessageChannel` (two `MessagePort`s, one per direction).
//!
//! # Why `async_trait(?Send)`
//!
//! The `?Send` attribute makes the returned futures `LocalBoxFuture` (not
//! `Send`). This is required for WASM, where futures are single-threaded and
//! `Send` bounds would be unsatisfiable for `web-sys` handles. The trait
//! itself remains `Send + Sync + 'static` so it can live in
//! `Arc<dyn Mailbox<T>>` on native multi-threaded runtimes; only the returned
//! futures are `!Send`. Native callers that need to `tokio::spawn` a mailbox
//! future should use `tokio::task::spawn_local` inside a `LocalSet`, or
//! re-wrap the future in `send_wrapper::SendWrapper`.
//!
//! # Module availability
//!
//! This module is available whenever the `bridge` feature is on — no extra
//! feature gate on the trait itself. The [`TokioMailbox<T>`] impl is gated by
//! `batcher` because it pulls `tokio::sync::mpsc`.

use async_trait::async_trait;

// ============================================================================
// Issue #3 sub-issue 1 — sister `batcher` + WASM compile_error guard
// ============================================================================
//
// The primary guard lives in `src/wasm/mod.rs` (per the issue spec) but is
// skipped entirely when the `wasm` feature is off (the surrounding
// `#![cfg(feature = "wasm")]` skips the whole file). This sister guard fires
// for the OTHER case: a user enables `batcher` on a `wasm32-unknown-unknown`
// target WITHOUT enabling the `wasm` feature. Since `batcher` requires
// `bridge` and `bridge` exposes `runtime`, this module is always compiled
// when `batcher` is on — so this guard is guaranteed to fire.
//
// The two guards are mutually exclusive (the `not(feature = "wasm")` here
// vs. the implicit `feature = "wasm"` from the file-level cfg in
// `src/wasm/mod.rs`) so only one fires per scenario — no double error spam.
#[cfg(all(feature = "batcher", target_family = "wasm", not(feature = "wasm")))]
compile_error!(
    "`batcher` feature pulls tokio::sync::mpsc which cannot run in browser WASM. \
     Enable the `wasm` feature for a clearer error chain OR use the `Mailbox` \
     trait with a wasm-bindgen-futures impl instead."
);

// ============================================================================
// Issue #3 sub-issue 3 — WASM-safe wall clock (`now_ms`)
// ============================================================================
//
// `SystemTime::now()` panics on `wasm32-unknown-unknown` (no system clock
// syscall — the spec leaves timekeeping to the embedder). The fix:
// route through `js_sys::Date::now()` on wasm32 (when the `wasm` feature is
// on) so we get a real wall-clock via the browser's `Date.now()`.
//
// Callers MUST prefer this over `SystemTime::now()` / `Instant::now()` so the
// crate is panicking-safe on WASM. Existing call sites that still use
// `SystemTime::now()` / `Instant::now()` directly are flagged via TODO
// comments — see the worklog for the audit results.

/// Wall-clock milliseconds since the UNIX epoch. WASM-safe.
///
/// - **Native** (`not(target_family = "wasm")`): `SystemTime::now()
///   .duration_since(UNIX_EPOCH).as_millis()`. Returns `0` if the system
///   clock is before the epoch (shouldn't happen post-1970, but defensive).
/// - **wasm32 with `wasm` feature**: `js_sys::Date::now() as u64` — the
///   browser's `Date.now()` returns wall-clock ms since UNIX epoch.
/// - **wasm32 without `wasm` feature**: `0`. Callers MUST enable the `wasm`
///   feature for a real clock; the fallback exists so a no-features WASM
///   build compiles without panicking at call time.
///
/// # Example
///
/// ```rust
/// # #[cfg(feature = "bridge")]
/// # {
/// use grafeo_loro::runtime::now_ms;
/// let t = now_ms();
/// // On native + on wasm32 with `wasm` feature, `t` is non-zero post-1970.
/// // On wasm32 without `wasm` feature, `t == 0` (caller must opt in).
/// # }
/// ```
pub fn now_ms() -> u64 {
    #[cfg(not(target_family = "wasm"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
    #[cfg(all(target_family = "wasm", feature = "wasm"))]
    {
        // `js_sys::Date::now()` returns `f64` (millis since UNIX epoch, same as
        // JS `Date.now()`). Cast to `u64` is safe for any post-1970 timestamp
        // (the `f64` mantissa has 52 bits of precision → ±8.6e15 ms = ±275k years).
        js_sys::Date::now() as u64
    }
    #[cfg(all(target_family = "wasm", not(feature = "wasm")))]
    {
        0 // fallback — caller must enable `wasm` feature for real clock
    }
}

// ============================================================================
// Issue #3 sub-issue 3 — native memory ceiling (`MemoryCeiling`)
// ============================================================================
//
// Browser WASM has a strict memory ceiling (default 2 GB, but Onde targets
// <80 MB per active graph). Without native enforcement, doc allocations can
// OOM the WASM heap. `MemoryCeiling` is a tracking struct that:
//
// 1. Counts the current bytes allocated (atomic, lock-free).
// 2. Rejects allocations that would exceed the configured budget (after
//    attempting eviction of idle docs).
// 3. Evicts idle docs in LRU order under pressure.
//
// This struct is target-agnostic — it works on both native (where the ceiling
// is a soft cap before the OS OOM-killer fires) and WASM (where the ceiling
// is a hard cap before the browser aborts the module).

/// Native memory ceiling enforcement (issue #3 sub-issue 3).
///
/// Evicts idle docs when the RAM budget is exceeded. Tracks the current
/// allocation count via a lock-free `AtomicUsize`; idle docs are registered
/// in a `parking_lot::Mutex<Vec<IdleDoc>>` and evicted in LRU order (oldest
/// `last_access_ms` first) when [`Self::try_alloc`] needs to free space.
///
/// # Example
///
/// ```rust
/// # #[cfg(feature = "bridge")]
/// # {
/// use grafeo_loro::runtime::MemoryCeiling;
///
/// let ceiling = MemoryCeiling::new(MemoryCeiling::DEFAULT_BUDGET);
/// assert!(ceiling.try_alloc(1024));
/// assert_eq!(ceiling.current_usage(), 1024);
/// ceiling.release(1024);
/// assert_eq!(ceiling.current_usage(), 0);
/// # }
/// ```
pub struct MemoryCeiling {
    /// Hard cap on `current_bytes`. Allocations that would exceed this fire
    /// eviction; if eviction cannot free enough, [`try_alloc`] returns `false`.
    budget_bytes: usize,
    /// Lock-free counter of currently-allocated bytes.
    current_bytes: std::sync::atomic::AtomicUsize,
    /// Idle docs registered for eviction. LRU-sorted at eviction time.
    /// Held behind `parking_lot::Mutex` (issue #1 mandate: parking_lot is the
    /// only always-on sync primitive). Held only briefly during
    /// register/evict — NOT held across any `.await` point.
    idle: parking_lot::Mutex<Vec<IdleDoc>>,
}

/// One registered idle doc — eligible for eviction under memory pressure.
#[derive(Debug, Clone, Copy)]
struct IdleDoc {
    /// Caller-supplied identifier (e.g. doc handle key). Opaque to `MemoryCeiling`.
    key: u64,
    /// Size in bytes that this doc occupies in the heap. Subtract from
    /// `current_bytes` on eviction.
    size_bytes: usize,
    /// Last-access timestamp in ms (via [`now_ms`]). LRU eviction picks the
    /// smallest `last_access_ms` first.
    last_access_ms: u64,
}

impl MemoryCeiling {
    /// Default budget: 80 MB. Matches the Onde target ceiling for active
    /// graph state in browser WASM (issue #3 sub-issue 3).
    pub const DEFAULT_BUDGET: usize = 80 * 1024 * 1024; // 80 MB

    /// Construct a new ceiling with the given byte budget.
    ///
    /// `current_bytes` starts at 0; the idle-doc registry starts empty.
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            current_bytes: std::sync::atomic::AtomicUsize::new(0),
            idle: parking_lot::Mutex::new(Vec::new()),
        }
    }

    /// The configured budget (in bytes). Read-only — never changes after
    /// construction.
    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    /// Current bytes tracked as allocated. Lock-free atomic load.
    pub fn current_usage(&self) -> usize {
        self.current_bytes.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Try to allocate `bytes` against the ceiling.
    ///
    /// - If `current_usage() + bytes <= budget`, increments the counter and
    ///   returns `true`.
    /// - Otherwise, evicts idle docs (LRU first) until either the allocation
    ///   fits (returns `true`) or the idle registry is exhausted (returns
    ///   `false`).
    ///
    /// The atomic increment uses `Relaxed` ordering — we don't need a
    /// happens-before relationship with other threads' allocations because
    /// overcommitting by a few bytes between the check and the increment is
    /// harmless (the next `try_alloc` will see the new total and trigger
    /// eviction if needed).
    pub fn try_alloc(&self, bytes: usize) -> bool {
        // Fast path: try to allocate without eviction.
        if self.try_alloc_inner(bytes) {
            return true;
        }
        // Slow path: evict LRU docs until we can fit `bytes` (or no more idle).
        let _freed = self.evict_to_fit(bytes);
        self.try_alloc_inner(bytes)
    }

    /// Internal: check + increment, no eviction. Returns `true` on success.
    fn try_alloc_inner(&self, bytes: usize) -> bool {
        let current = self.current_usage();
        if current + bytes <= self.budget_bytes {
            self.current_bytes
                .fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Release `bytes` from the counter. Called when a doc is dropped by the
    /// caller (NOT via eviction — eviction releases its own bytes).
    pub fn release(&self, bytes: usize) {
        // `fetch_sub` wraps on underflow; clamp to 0 explicitly to prevent
        // a bug from cascading into a `usize::MAX` current_usage.
        let prev = self
            .current_bytes
            .fetch_sub(bytes, std::sync::atomic::Ordering::Relaxed);
        if prev < bytes {
            // Underflow — reset to 0 (defensive; should not happen if callers
            // balance alloc/release correctly).
            self.current_bytes
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Register a doc as idle (eligible for eviction).
    ///
    /// `key` is any caller-supplied u64 identifier (e.g. the doc handle's
    /// internal id). `size_bytes` is the doc's heap footprint — subtracted
    /// from `current_usage()` on eviction. `last_access_ms` is the doc's
    /// last-access timestamp (via [`now_ms`]); LRU eviction picks the
    /// smallest first.
    ///
    /// If a doc with the same `key` is already registered, it is updated
    /// (size + last-access) — this is the "doc was idle, became active, became
    /// idle again" path.
    pub fn register_idle(&self, key: u64, size_bytes: usize, last_access_ms: u64) {
        let mut idle = self.idle.lock();
        if let Some(slot) = idle.iter_mut().find(|d| d.key == key) {
            *slot = IdleDoc {
                key,
                size_bytes,
                last_access_ms,
            };
        } else {
            idle.push(IdleDoc {
                key,
                size_bytes,
                last_access_ms,
            });
        }
    }

    /// Evict the single oldest (LRU) idle doc. Returns the bytes freed
    /// (`0` if the idle registry is empty).
    ///
    /// This is the **public manual hook** for callers that want to react to
    /// external memory pressure signals (e.g. JS `memory.pressure` event,
    /// `performance.memory` growth) without going through [`try_alloc`].
    ///
    /// The internal `try_alloc` path uses the private [`evict_to_fit`]
    /// helper which evicts multiple docs in one lock acquisition to fit a
    /// target allocation. This public method evicts exactly one — callers
    /// that need to free N bytes should call this in a loop.
    pub fn evict_idle(&self) -> usize {
        let mut idle = self.idle.lock();
        if idle.is_empty() {
            return 0;
        }
        // LRU: sort ascending by `last_access_ms` (oldest first).
        idle.sort_by_key(|d| d.last_access_ms);
        let evicted = idle.remove(0);
        // Saturating subtract: the atomic counter should never go below the
        // doc's size, but `fetch_sub` wraps on underflow — clamp defensively.
        let prev = self
            .current_bytes
            .fetch_sub(evicted.size_bytes, std::sync::atomic::Ordering::Relaxed);
        prev.min(evicted.size_bytes)
    }

    /// Private: evict LRU docs in a loop until `current + bytes <= budget`
    /// (or no more idle docs). Used by [`try_alloc`] in the slow path.
    /// Returns total bytes freed.
    fn evict_to_fit(&self, bytes: usize) -> usize {
        let mut idle = self.idle.lock();
        if idle.is_empty() {
            return 0;
        }
        idle.sort_by_key(|d| d.last_access_ms);
        let mut freed = 0usize;
        while !idle.is_empty() && self.current_usage() + bytes > self.budget_bytes {
            let evicted = idle.remove(0);
            let prev = self.current_bytes.fetch_sub(
                evicted.size_bytes,
                std::sync::atomic::Ordering::Relaxed,
            );
            let actually_freed = prev.min(evicted.size_bytes);
            freed += actually_freed;
        }
        freed
    }
}

impl std::fmt::Debug for MemoryCeiling {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryCeiling")
            .field("budget_bytes", &self.budget_bytes)
            .field("current_bytes", &self.current_usage())
            .field("idle_count", &self.idle.lock().len())
            .finish()
    }
}

impl Default for MemoryCeiling {
    fn default() -> Self {
        Self::new(Self::DEFAULT_BUDGET)
    }
}

/// Error returned by [`Mailbox::send`] when the channel is closed (all
/// receivers have been dropped, or the underlying transport has been torn
/// down).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("mailbox channel is closed")]
pub struct MailboxClosed;

/// MPSC mailbox abstracted over the async runtime.
///
/// Onde plugs in `wasm-bindgen-futures` + `web-sys::MessageChannel` in the
/// browser. The default tokio-backed impl ([`TokioMailbox<T>`]) lives behind
/// the `batcher` feature.
///
/// # When `batcher` is off (WASM mode)
///
/// Users provide their own impl — e.g. `WasmMessageChannelMailbox` backed by
/// `web-sys::MessageChannel`. The `Send + Sync + 'static` supertraits are
/// satisfiable in WASM by either storing raw JS handle indices (the
/// `MessagePort`'s internal `u32`) or by wrapping the JS handle in
/// `send_wrapper::SendWrapper`.
///
/// # Why `?Send` futures
///
/// See the module-level docs: the trait is `Send + Sync + 'static` (so it
/// can live in `Arc<dyn Mailbox<T>>`), but the futures returned by `send`
/// and `recv` are `LocalBoxFuture` (not `Send`) so the trait is usable from
/// single-threaded WASM runtimes.
///
/// # Errors
///
/// [`send`](Mailbox::send) returns [`MailboxClosed`] when the receive half
/// has been dropped. [`recv`](Mailbox::recv) returns `None` when the send
/// half has been dropped (standard MPSC close semantics).
#[async_trait(?Send)]
pub trait Mailbox<T>: Send + Sync + 'static {
    /// Send a message. Returns `Err(MailboxClosed)` if all receivers have
    /// been dropped (channel closed from the receive side).
    async fn send(&self, msg: T) -> Result<(), MailboxClosed>;

    /// Receive a message. Returns `None` if all senders have been dropped
    /// (channel closed from the send side).
    async fn recv(&self) -> Option<T>;
}

// ============================================================================
// Tokio-backed default impl — only available with the `batcher` feature.
// ============================================================================
//
// The `batcher` feature pulls `tokio` (with the `sync` feature, which
// provides `mpsc`). WASM builds MUST NOT enable `batcher`; they provide
// their own `Mailbox<T>` impl instead.

#[cfg(feature = "batcher")]
use std::sync::Arc;

/// Tokio-backed default impl of [`Mailbox<T>`].
///
/// Wraps `tokio::sync::mpsc::Sender<T>` + `tokio::sync::mpsc::Receiver<T>`
/// behind `parking_lot::Mutex`. The two halves returned by [`Self::new`]
/// share a single `Arc` to the channel state so they can be moved
/// independently (e.g. tx_side into a sync engine, rx_side into a worker
/// task).
///
/// # Drop semantics
///
/// Dropping the tx_side drops the `Sender<T>` → subsequent `recv` on the
/// rx_side returns `None`. Dropping the rx_side drops the `Receiver<T>` →
/// subsequent `send` on the tx_side returns `Err(MailboxClosed)`. This
/// mirrors `tokio::sync::mpsc` close semantics.
///
/// # Concurrency note
///
/// The `Receiver<T>` is stored behind a single `parking_lot::Mutex` (not
/// `tokio::sync::Mutex`) per the issue #1 mandate that `parking_lot` is the
/// only always-on sync primitive. The mutex is held only for the duration of
/// the `take()` / `recv().await` / `put_back` sequence — it is NOT held
/// across the `.await` point, so senders are never blocked by an in-flight
/// recv. Concurrent `recv` calls from multiple tasks are discouraged
/// (standard MPSC contract: one consumer); a second concurrent recv will
/// return `None` immediately rather than block.
#[cfg(feature = "batcher")]
pub struct TokioMailbox<T> {
    inner: Arc<parking_lot::Mutex<TokioMailboxInner<T>>>,
    role: MailboxRole,
}

#[cfg(feature = "batcher")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum MailboxRole {
    Tx,
    Rx,
}

#[cfg(feature = "batcher")]
struct TokioMailboxInner<T> {
    tx: Option<tokio::sync::mpsc::Sender<T>>,
    rx: Option<tokio::sync::mpsc::Receiver<T>>,
}

#[cfg(feature = "batcher")]
impl<T> TokioMailbox<T> {
    /// Create a new bounded tokio-backed channel with the given capacity.
    ///
    /// Returns `(tx_side, rx_side)` — two [`TokioMailbox<T>`] handles that
    /// share the same underlying channel via `Arc`. The `tx_side` is used to
    /// send messages; the `rx_side` is used to receive them. Either side
    /// can be dropped independently; dropping one closes the channel from
    /// the other side's perspective (see type-level docs).
    ///
    /// # Capacity
    ///
    /// Bounded capacity matches `tokio::sync::mpsc::channel`. Use a small
    /// power of two (e.g. 1024 for the inbound bridge channel).
    pub fn new(capacity: usize) -> (Self, Self) {
        let (tx, rx) = tokio::sync::mpsc::channel::<T>(capacity);
        let inner = Arc::new(parking_lot::Mutex::new(TokioMailboxInner {
            tx: Some(tx),
            rx: Some(rx),
        }));
        let tx_side = Self {
            inner: inner.clone(),
            role: MailboxRole::Tx,
        };
        let rx_side = Self {
            inner,
            role: MailboxRole::Rx,
        };
        (tx_side, rx_side)
    }
}

#[cfg(feature = "batcher")]
impl<T> Drop for TokioMailbox<T> {
    fn drop(&mut self) {
        // On drop, take our half out of the shared inner so the other half
        // observes channel-closed semantics. Without this, dropping the
        // tx_side would merely decrement the Arc refcount and the Receiver
        // would block forever on recv().
        let mut guard = self.inner.lock();
        match self.role {
            MailboxRole::Tx => {
                guard.tx.take();
            }
            MailboxRole::Rx => {
                guard.rx.take();
            }
        }
    }
}

/// RAII guard that puts the `Receiver<T>` back into the shared inner when
/// dropped, even if the `recv().await` future is cancelled mid-flight.
///
/// Without this guard, cancelling a `recv()` between `take()` and `put_back`
/// would permanently lose the `Receiver<T>`, silently breaking the channel.
#[cfg(feature = "batcher")]
struct ReceiverGuard<T> {
    inner: Arc<parking_lot::Mutex<TokioMailboxInner<T>>>,
    rx: Option<tokio::sync::mpsc::Receiver<T>>,
}

#[cfg(feature = "batcher")]
impl<T> ReceiverGuard<T> {
    /// Poll the guarded receiver. Returns `None` if the receiver was absent
    /// (another task is currently in `recv`) or if all senders have been
    /// dropped (channel closed).
    async fn recv(&mut self) -> Option<T> {
        match self.rx.as_mut() {
            Some(rx) => rx.recv().await,
            None => None,
        }
    }
}

#[cfg(feature = "batcher")]
impl<T> Drop for ReceiverGuard<T> {
    fn drop(&mut self) {
        // Only put the receiver back if we actually hold one. If `rx` is
        // None (e.g. another task had the receiver), leave the inner
        // untouched.
        if let Some(rx) = self.rx.take() {
            let mut guard = self.inner.lock();
            guard.rx = Some(rx);
        }
    }
}

#[cfg(feature = "batcher")]
#[async_trait(?Send)]
impl<T: Send + 'static> Mailbox<T> for TokioMailbox<T> {
    async fn send(&self, msg: T) -> Result<(), MailboxClosed> {
        // Clone the Sender out of the mutex, then release the lock before
        // awaiting. `tokio::sync::mpsc::Sender` is cheaply `Clone` (just an
        // Arc bump internally).
        let tx = {
            let guard = self.inner.lock();
            guard.tx.clone()
        };
        match tx {
            Some(tx) => tx.send(msg).await.map_err(|_| MailboxClosed),
            // Tx half was dropped (or never existed) — channel is closed.
            None => Err(MailboxClosed),
        }
    }

    async fn recv(&self) -> Option<T> {
        // Take the Receiver out of the mutex, then release the lock before
        // awaiting. parking_lot::Mutex is NOT safe to hold across .await.
        // The ReceiverGuard puts the receiver back when dropped, including
        // if the future is cancelled.
        let rx = {
            let mut guard = self.inner.lock();
            guard.rx.take()
        };
        let mut guard = ReceiverGuard {
            inner: self.inner.clone(),
            rx,
        };
        guard.recv().await
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Basic round-trip: send a message, recv it, assert equal.
    #[cfg(feature = "batcher")]
    #[tokio::test]
    async fn tokio_mailbox_send_recv_roundtrip() {
        let (tx, rx) = TokioMailbox::<u32>::new(8);
        tx.send(42).await.unwrap();
        let msg = rx.recv().await;
        assert_eq!(msg, Some(42));
    }

    /// Dropping the tx_side closes the channel; subsequent recv returns None.
    #[cfg(feature = "batcher")]
    #[tokio::test]
    async fn tokio_mailbox_closed_returns_none() {
        let (tx, rx) = TokioMailbox::<u32>::new(8);
        drop(tx);
        let msg = rx.recv().await;
        assert_eq!(msg, None);
    }

    /// Dropping the rx_side causes subsequent send to return Err(MailboxClosed).
    #[cfg(feature = "batcher")]
    #[tokio::test]
    async fn tokio_mailbox_send_after_rx_dropped_returns_err() {
        let (tx, rx) = TokioMailbox::<u32>::new(8);
        drop(rx);
        let result = tx.send(99).await;
        assert_eq!(result, Err(MailboxClosed));
    }

    /// Multiple messages preserve FIFO order.
    #[cfg(feature = "batcher")]
    #[tokio::test]
    async fn tokio_mailbox_fifo_order() {
        let (tx, rx) = TokioMailbox::<&'static str>::new(8);
        tx.send("first").await.unwrap();
        tx.send("second").await.unwrap();
        tx.send("third").await.unwrap();
        assert_eq!(rx.recv().await, Some("first"));
        assert_eq!(rx.recv().await, Some("second"));
        assert_eq!(rx.recv().await, Some("third"));
    }

    // ========================================================================
    // Issue #3 sub-issue 3 — `now_ms` + `MemoryCeiling` + RefCell fix tests
    // ========================================================================

    /// `now_ms()` returns non-zero on native (post-1970). On wasm32 with `wasm`
    /// feature it would also be non-zero via `js_sys::Date::now()`; on wasm32
    /// without `wasm` feature it returns 0 (documented fallback).
    #[test]
    fn now_ms_returns_nonzero_on_native() {
        let t = now_ms();
        // Native: SystemTime::now() since UNIX_EPOCH — non-zero post-1970.
        // We can't assert `> 0` on wasm32-without-wasm-feature; check cfg.
        #[cfg(not(target_family = "wasm"))]
        {
            assert!(t > 0, "native now_ms must be non-zero post-1970; got {t}");
            // Sanity: should be a reasonable 21st-century timestamp
            // (between 2024-01-01 and 2100-01-01 in ms).
            assert!(
                t >= 1_704_067_200_000, // 2024-01-01
                "now_ms sanity: expected ≥2024 timestamp, got {t}"
            );
        }
        #[cfg(all(target_family = "wasm", feature = "wasm"))]
        {
            assert!(t > 0, "wasm with wasm feature: now_ms must be non-zero; got {t}");
        }
        #[cfg(all(target_family = "wasm", not(feature = "wasm")))]
        {
            assert_eq!(t, 0, "wasm without wasm feature: now_ms must fall back to 0");
        }
    }

    /// `MemoryCeiling` basic alloc/release cycle: try_alloc succeeds within
    /// budget, current_usage reflects the alloc, release brings it back to 0.
    #[test]
    fn memory_ceiling_alloc_release_cycle() {
        let ceiling = MemoryCeiling::new(1024);
        assert_eq!(ceiling.budget_bytes(), 1024);
        assert_eq!(ceiling.current_usage(), 0);

        assert!(ceiling.try_alloc(512));
        assert_eq!(ceiling.current_usage(), 512);

        assert!(ceiling.try_alloc(512));
        assert_eq!(ceiling.current_usage(), 1024);

        // Over-budget alloc must fail (no idle docs to evict).
        assert!(!ceiling.try_alloc(1));
        assert_eq!(ceiling.current_usage(), 1024, "failed alloc must not mutate state");

        ceiling.release(512);
        assert_eq!(ceiling.current_usage(), 512);
        ceiling.release(512);
        assert_eq!(ceiling.current_usage(), 0);
    }

    /// `MemoryCeiling` eviction under pressure: register idle docs, then
    /// trigger eviction by over-allocating. Verify the LRU policy (oldest
    /// `last_access_ms` evicted first) and the bytes-freed return value.
    #[test]
    fn memory_ceiling_evicts_idle_under_pressure_lru() {
        let ceiling = MemoryCeiling::new(1000);

        // Three docs allocated, totaling 900 bytes (under budget).
        assert!(ceiling.try_alloc(300));
        assert!(ceiling.try_alloc(300));
        assert!(ceiling.try_alloc(300));
        assert_eq!(ceiling.current_usage(), 900);

        // Register them as idle with distinct last-access timestamps.
        // Doc 1 is oldest (LRU candidate), doc 3 is newest.
        ceiling.register_idle(/*key=*/ 1, /*size=*/ 300, /*last_access=*/ 1_000);
        ceiling.register_idle(/*key=*/ 2, /*size=*/ 300, /*last_access=*/ 2_000);
        ceiling.register_idle(/*key=*/ 3, /*size=*/ 300, /*last_access=*/ 3_000);

        // Now try to alloc 200 more — total would be 1100 > 1000 budget.
        // Eviction must remove doc 1 (LRU, 300 bytes) → 900-300=600 → 600+200=800 ≤ 1000. OK.
        let ok = ceiling.try_alloc(200);
        assert!(ok, "try_alloc(200) must succeed after evicting idle doc 1");
        assert_eq!(
            ceiling.current_usage(),
            800,
            "after eviction + alloc: 900 - 300 + 200 = 800"
        );
    }

    /// `MemoryCeiling::evict_idle` returns 0 when there are no idle docs
    /// (e.g. fresh ceiling, or all docs are active).
    #[test]
    fn memory_ceiling_evict_idle_returns_zero_when_empty() {
        let ceiling = MemoryCeiling::new(1000);
        ceiling.try_alloc(500);
        let freed = ceiling.evict_idle();
        assert_eq!(freed, 0, "no idle docs registered → nothing to evict");
        assert_eq!(ceiling.current_usage(), 500, "current_usage unchanged");
    }

    /// `MemoryCeiling::evict_idle` (public manual hook) evicts the SINGLE
    /// oldest idle doc and returns its size. To evict multiple, the caller
    /// loops. This test verifies the single-evict semantics + LRU order.
    #[test]
    fn memory_ceiling_evict_idle_single_lru() {
        let ceiling = MemoryCeiling::new(1000);
        ceiling.try_alloc(300);
        ceiling.try_alloc(300);
        ceiling.try_alloc(300);
        assert_eq!(ceiling.current_usage(), 900);

        // Register 3 idle docs with distinct last-access timestamps.
        ceiling.register_idle(10, 300, 5_000); // oldest (LRU)
        ceiling.register_idle(20, 300, 6_000);
        ceiling.register_idle(30, 300, 7_000); // newest

        // First evict_idle call: should evict doc 10 (oldest).
        let freed = ceiling.evict_idle();
        assert_eq!(freed, 300, "evict one doc (300 bytes)");
        assert_eq!(ceiling.current_usage(), 600, "900 - 300 = 600");

        // Second call: evicts doc 20.
        let freed = ceiling.evict_idle();
        assert_eq!(freed, 300, "evict second doc (300 bytes)");
        assert_eq!(ceiling.current_usage(), 300, "600 - 300 = 300");

        // Third call: evicts doc 30.
        let freed = ceiling.evict_idle();
        assert_eq!(freed, 300, "evict third doc (300 bytes)");
        assert_eq!(ceiling.current_usage(), 0, "300 - 300 = 0");

        // Fourth call: no more idle docs.
        let freed = ceiling.evict_idle();
        assert_eq!(freed, 0, "no more idle docs → 0 freed");
    }

    /// `MemoryCeiling::try_alloc` triggers multi-doc eviction in the slow path.
    /// Verifies the internal `evict_to_fit` evicts enough LRU docs to fit the
    /// requested allocation.
    #[test]
    fn memory_ceiling_try_alloc_evicts_multiple_to_fit() {
        let ceiling = MemoryCeiling::new(1000);

        // Allocate 3 * 300 = 900 bytes (under budget).
        assert!(ceiling.try_alloc(300));
        assert!(ceiling.try_alloc(300));
        assert!(ceiling.try_alloc(300));
        assert_eq!(ceiling.current_usage(), 900);

        // Register them as idle with distinct last-access timestamps.
        ceiling.register_idle(10, 300, 5_000); // oldest
        ceiling.register_idle(20, 300, 6_000);
        ceiling.register_idle(30, 300, 7_000); // newest

        // Try to alloc 700 more: 900 + 700 = 1600 > 1000 budget.
        // Need to free at least 600 bytes. LRU evicts doc 10 (300) → current=600,
        // 600+700=1300 > 1000, evict doc 20 (300) → current=300, 300+700=1000 ≤ 1000.
        // Stop. Now alloc 700 → current=1000.
        let ok = ceiling.try_alloc(700);
        assert!(ok, "try_alloc(700) must succeed after evicting 2 idle docs");
        assert_eq!(
            ceiling.current_usage(),
            1000,
            "after eviction + alloc: 900 - 600 + 700 = 1000"
        );
    }

    /// `MemoryCeiling::try_alloc` returns `false` when even evicting ALL idle
    /// docs cannot free enough space for the requested allocation.
    #[test]
    fn memory_ceiling_try_alloc_fails_when_eviction_insufficient() {
        let ceiling = MemoryCeiling::new(1000);
        ceiling.try_alloc(500);
        ceiling.register_idle(99, 500, 1_000);

        // Try to alloc 600: 500+600=1100 > 1000. Evict doc 99 → current=0.
        // 0+600=600 ≤ 1000. OK — should succeed.
        assert!(ceiling.try_alloc(600));

        // Now try to alloc 1100 (more than budget): 600+1100=1700 > 1000.
        // No idle docs left → fail.
        assert!(
            !ceiling.try_alloc(1100),
            "try_alloc(>budget) must fail even after eviction"
        );
        assert_eq!(ceiling.current_usage(), 600, "failed alloc must not mutate state");
    }

    /// Issue #3 sub-issue 3: RefCell borrow trap fix demonstration.
    ///
    /// The BROKEN pattern `*d.borrow_mut() = d.borrow().saturating_sub(1)`
    /// panics at runtime because `borrow_mut()` is called WHILE `borrow()` is
    /// still held (the temporary `Ref` is dropped at the END of the statement,
    /// AFTER `borrow_mut` was already invoked — `RefCell` enforces borrow
    /// rules at runtime and rejects the double-borrow).
    ///
    /// The FIX: extract the immutable borrow into a local variable FIRST,
    /// drop it explicitly at the end of the statement (the local's lifetime
    /// ends at `;`), THEN call `borrow_mut()` in a separate statement.
    ///
    /// This test verifies BOTH the broken pattern panics AND the fix works.
    #[test]
    #[should_panic(expected = "already borrowed")]
    fn refcell_borrow_trap_broken_pattern_panics() {
        use std::cell::RefCell;
        let d = RefCell::new(5u32);
        // BROKEN: borrow_mut while borrow is alive → runtime panic.
        // The `d.borrow()` returns a `Ref<u32>` whose lifetime extends to the
        // end of the statement, but `d.borrow_mut()` is called BEFORE that
        // lifetime ends, so `RefCell` sees two simultaneous borrows.
        *d.borrow_mut() = d.borrow().saturating_sub(1);
    }

    /// The FIXED pattern: separate the immutable read from the mutable write.
    /// No panic; the value is correctly updated.
    #[test]
    fn refcell_borrow_trap_fixed_pattern_works() {
        use std::cell::RefCell;
        let d = RefCell::new(5u32);

        // FIXED: the immutable borrow ends at `;` BEFORE the mutable borrow starts.
        let cur = *d.borrow(); // `Ref<u32>` dropped here, releases the borrow.
        *d.borrow_mut() = cur.saturating_sub(1); // fresh mutable borrow.

        assert_eq!(*d.borrow(), 4, "fixed pattern must correctly decrement");
    }
}
