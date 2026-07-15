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
}
