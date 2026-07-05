//! Inbound mutation batcher: collects `LoroOp`s and flushes them as a single
//! vectorized Grafeo transaction tagged with `ORIGIN_LORO_BRIDGE`.
//!
//! ## Wiring summary (Devil MAJOR M13)
//!
//! - `push(&mut self, ...)` removed — the batcher is driven entirely by its
//!   `run` loop consuming from a `Receiver<LoroOp>`.
//! - `flush_notify` field removed — size-threshold flush is checked inline
//!   after each `recv()` in the `run` loop.
//! - `run` takes `self: Arc<Self>` (interior mutability via
//!   `parking_lot::Mutex<Vec<LoroOp>>`) plus `mpsc::Receiver<LoroOp>`.
//! - `flush_inner` applies each op via `apply_loro_op(&session, op, &maps)`,
//!   then `prepare_commit` → `set_metadata` (advisory only) → `commit()`,
//!   recording the resulting `EpochId` in `bridge_origin_epochs` for echo
//!   prevention. The grafeo session calls run inside a
//!   `tokio::task::spawn_blocking` closure; the resulting `JoinHandle` is
//!   wrapped in `tokio::time::timeout(FLUSH_TIMEOUT)` so a stuck grafeo
//!   transaction cannot hang the inbound `JoinHandle` (Hunter MAJOR 1 —
//!   previously the timeout wrapped a zero-`.await` async block and could
//!   never fire). On timeout the blocking task keeps running in the
//!   background; if it eventually commits, its epoch lands in the side
//!   channel and is filtered by the outbound poller.

use std::collections::HashSet;
use std::sync::Arc;

use grafeo::GrafeoDB;
use grafeo_common::types::EpochId;
use parking_lot::Mutex;
use parking_lot::RwLock;
use tokio::sync::{broadcast, mpsc};
use tokio::time::Duration;

use crate::bridge::grafeo_tx::{apply_loro_op, BridgeMaps};
use crate::constants::{DEFAULT_BATCH_MS, DEFAULT_BATCH_SIZE, ORIGIN_LORO_BRIDGE};
use crate::error::{GrafeoLoroError, Result};
use crate::types::events::LoroOp;

/// Max wall-clock seconds for a single flush. If a grafeo transaction hangs,
/// the inbound worker logs an error and continues so its `JoinHandle` does
/// not block indefinitely (L2 new issue #6).
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// Time-and-count-based mutation batcher. Owns its `LoroOp` buffer behind a
/// `parking_lot::Mutex` (interior mutability — `run` takes `Arc<Self>`). The
/// `run` loop `tokio::select!`s between (a) `rx.recv()` → push + size-check
/// flush, (b) interval tick → flush, (c) shutdown → flush remaining + exit.
pub struct MutationBatcher {
    /// Grafeo execution-layer handle (internally thread-safe).
    pub(crate) grafeo_db: Arc<GrafeoDB>,
    /// Pending ops awaiting the next flush. Interior-mutable so `run` can
    /// take `self: Arc<Self>`.
    pub(crate) buffer: Mutex<Vec<LoroOp>>,
    /// Count threshold that triggers an immediate flush after `recv()`.
    pub(crate) batch_size: usize,
    /// Time threshold (ms) between automatic flushes in `run`.
    pub(crate) batch_ms: u64,
    /// Shared epoch side-channel set (echo prevention). After each flush's
    /// `prepared.commit()` returns the `EpochId`, the batcher inserts it
    /// here so the outbound CDC poller can filter same-epoch events.
    pub(crate) bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
    /// Shared id-mapping state (`loro_key ↔ grafeo::NodeId`,
    /// `EdgeKey ↔ grafeo::EdgeId`). Passed to `apply_loro_op`.
    pub(crate) maps: Arc<BridgeMaps>,
    /// Shutdown broadcast — `run` subscribes and exits on `trigger()`.
    pub(crate) shutdown_tx: broadcast::Sender<()>,
}

impl MutationBatcher {
    /// Construct a batcher with explicit tuning. Shared `bridge_origin_epochs`
    /// and `maps` are owned by the parent `SyncEngine` and passed in by `Arc`
    /// clone so both the batcher and the engine/poller see the same state.
    pub fn new(
        grafeo_db: Arc<GrafeoDB>,
        batch_size: usize,
        batch_ms: u64,
        bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
        maps: Arc<BridgeMaps>,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Self {
        Self {
            grafeo_db,
            buffer: Mutex::new(Vec::new()),
            batch_size,
            batch_ms,
            bridge_origin_epochs,
            maps,
            shutdown_tx,
        }
    }

    /// Construct a batcher using [`DEFAULT_BATCH_SIZE`] and [`DEFAULT_BATCH_MS`].
    pub fn with_defaults(
        grafeo_db: Arc<GrafeoDB>,
        bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
        maps: Arc<BridgeMaps>,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Self {
        Self::new(
            grafeo_db,
            DEFAULT_BATCH_SIZE,
            DEFAULT_BATCH_MS,
            bridge_origin_epochs,
            maps,
            shutdown_tx,
        )
    }

    /// Main loop: `tokio::select!` between (a) `rx.recv()` → push +
    /// size-threshold flush, (b) interval tick → flush, (c) shutdown →
    /// flush remaining + exit. Returns when shutdown fires AND the final
    /// flush completes.
    pub async fn run(self: Arc<Self>, mut rx: mpsc::Receiver<LoroOp>) -> Result<()> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let mut ticker = tokio::time::interval(Duration::from_millis(self.batch_ms));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => {
                    let drained: Vec<LoroOp> = {
                        let mut b = self.buffer.lock();
                        std::mem::take(&mut *b)
                    };
                    if !drained.is_empty() {
                        self.flush_inner(drained).await?;
                    }
                    break;
                }
                Some(op) = rx.recv() => {
                    {
                        let mut b = self.buffer.lock();
                        b.push(op);
                        if b.len() < self.batch_size {
                            continue;
                        }
                    }
                    self.flush().await?;
                }
                _ = ticker.tick() => {
                    self.flush().await?;
                }
            }
        }
        Ok(())
    }

    /// Drain `buffer` and apply all ops in a single Grafeo transaction tagged
    /// with `ORIGIN_LORO_BRIDGE`. The buffer is empty on return. Records the
    /// commit `EpochId` in `bridge_origin_epochs` for echo prevention.
    async fn flush(&self) -> Result<()> {
        let drained: Vec<LoroOp> = {
            let mut b = self.buffer.lock();
            std::mem::take(&mut *b)
        };
        if drained.is_empty() {
            return Ok(());
        }
        self.flush_inner(drained).await
    }

    /// Inner flush: takes a pre-drained buffer and applies it as a single
    /// Grafeo transaction. Split out so `run`'s shutdown path can drain +
    /// flush without re-acquiring the buffer lock twice.
    ///
    /// The grafeo session calls (`begin_transaction`/`apply_loro_op`/
    /// `prepare_commit`/`commit`) are synchronous and may block on disk IO
    /// or grafeo-internal locks. Running them on the async worker would
    /// starve the runtime; running them inside an async block with no
    /// `.await` points makes `tokio::time::timeout` a no-op (Hunter MAJOR 1).
    /// The fix: `spawn_blocking` runs the entire grafeo transaction on a
    /// dedicated blocking-pool thread, and `tokio::time::timeout` is applied
    /// to the resulting `JoinHandle`. If the timeout elapses, the worker
    /// reports a `GrafeoLoroError::Bridge(...)` and moves on; the orphaned
    /// blocking task continues to completion in the background — if it
    /// eventually commits, the resulting `EpochId` lands in the side-channel
    /// so the outbound poller still filters the corresponding CDC events.
    /// If it panics, the `JoinError` is mapped to `Bridge(...)` as well.
    async fn flush_inner(&self, ops: Vec<LoroOp>) -> Result<()> {
        let grafeo_db = self.grafeo_db.clone();
        let maps = self.maps.clone();
        let epochs = self.bridge_origin_epochs.clone();
        let op_count = ops.len();

        let blocking = tokio::task::spawn_blocking(move || -> Result<()> {
            let mut session = grafeo_db.session_with_cdc(true);
            session.begin_transaction()?;
            for op in &ops {
                apply_loro_op(&session, op, &maps)?;
            }
            let mut prepared = session.prepare_commit()?;
            // Note (Devil BLOCKER B2): `set_metadata` is dropped on `commit()`
            // — it never reaches `ChangeEvent`. Kept for advisory logging only;
            // the epoch side-channel is the real echo-prevention mechanism.
            prepared.set_metadata("origin", ORIGIN_LORO_BRIDGE);
            let epoch: EpochId = prepared.commit()?;
            epochs.write().insert(epoch);
            Ok(())
        });

        match tokio::time::timeout(FLUSH_TIMEOUT, blocking).await {
            Ok(Ok(res)) => res,
            Ok(Err(join_err)) => {
                tracing::error!(
                    error = %join_err,
                    "inbound flush blocking task panicked; {} ops in limbo",
                    op_count
                );
                Err(GrafeoLoroError::Bridge(format!(
                    "inbound flush blocking task panicked: {join_err}"
                )))
            }
            Err(_) => {
                tracing::error!(
                    "inbound flush exceeded {:?} timeout; {} ops dropped (task continues in background)",
                    FLUSH_TIMEOUT,
                    op_count
                );
                Err(GrafeoLoroError::Bridge(format!(
                    "inbound flush timeout after {:?}",
                    FLUSH_TIMEOUT
                )))
            }
        }
    }
}
