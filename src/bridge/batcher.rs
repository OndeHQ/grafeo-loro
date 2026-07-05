//! Inbound mutation batcher: collects `LoroOp`s and flushes them as a single
//! vectorized Grafeo transaction tagged with `ORIGIN_LORO_BRIDGE`. All
//! algorithm bodies are `// TODO L3`; the wiring (struct fields, channel
//! plumbing, run-loop shape) is real and compiles.
//!
//! ## Wiring summary (Devil MAJOR M13)
//!
//! - `push(&mut self, ...)` removed — the batcher is driven entirely by its
//!   `run` loop consuming from a `Receiver<LoroOp>`.
//! - `flush_notify` field removed — size-threshold flush is checked inline
//!   after each `recv()` in the `run` loop.
//! - `run` takes `self: Arc<Self>` (interior mutability via
//!   `parking_lot::Mutex<Vec<LoroOp>>`) plus `mpsc::Receiver<LoroOp>`.
//! - `flush` skeleton uses the grafeo `Session` + `PreparedCommit` API and
//!   records the commit epoch in `bridge_origin_epochs` for echo prevention.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use grafeo::GrafeoDB;
use grafeo_common::types::EpochId;
use parking_lot::{Mutex, RwLock};
use tokio::sync::{broadcast, mpsc};
use tokio::time::Duration;

#[allow(unused_imports)] // Call site is `// TODO L3` in `flush_inner`; L3 uncomments it.
use crate::bridge::grafeo_tx::apply_loro_op;
use crate::constants::{DEFAULT_BATCH_MS, DEFAULT_BATCH_SIZE, ORIGIN_LORO_BRIDGE};
use crate::error::Result;
use crate::types::events::LoroOp;

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
    /// Shared `loro_key → grafeo::NodeId` map. Passed to `apply_loro_op` so
    /// the apply step can look up or create+insert nodes per
    /// `LoroOp::UpsertNode`.
    pub(crate) node_id_map: Arc<RwLock<HashMap<String, grafeo::NodeId>>>,
    /// Shutdown broadcast — `run` subscribes and exits on `trigger()`.
    pub(crate) shutdown_tx: broadcast::Sender<()>,
}

impl MutationBatcher {
    /// Construct a batcher with explicit tuning. Shared `bridge_origin_epochs`
    /// and `node_id_map` are owned by the parent `SyncEngine` and passed in
    /// by `Arc` clone so both the batcher and the engine/poller see the same
    /// state.
    pub fn new(
        grafeo_db: Arc<GrafeoDB>,
        batch_size: usize,
        batch_ms: u64,
        bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
        node_id_map: Arc<RwLock<HashMap<String, grafeo::NodeId>>>,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Self {
        Self {
            grafeo_db,
            buffer: Mutex::new(Vec::new()),
            batch_size,
            batch_ms,
            bridge_origin_epochs,
            node_id_map,
            shutdown_tx,
        }
    }

    /// Construct a batcher using [`DEFAULT_BATCH_SIZE`] and [`DEFAULT_BATCH_MS`].
    pub fn with_defaults(
        grafeo_db: Arc<GrafeoDB>,
        bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
        node_id_map: Arc<RwLock<HashMap<String, grafeo::NodeId>>>,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Self {
        Self::new(
            grafeo_db,
            DEFAULT_BATCH_SIZE,
            DEFAULT_BATCH_MS,
            bridge_origin_epochs,
            node_id_map,
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
                    // Drain remaining ops + final flush on shutdown.
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
                    // Size threshold hit — flush now.
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
    async fn flush_inner(&self, ops: Vec<LoroOp>) -> Result<()> {
        // TODO L3: implement actual flush body. Wiring below is real and
        // compiles; L3 uncomments the apply_loro_op call after the apply
        // function is filled in.
        let mut session = self.grafeo_db.session_with_cdc(true);
        session.begin_transaction()?;
        for op in &ops {
            // TODO L3: apply_loro_op(&session, op, &self.node_id_map)?;
            let _ = (op, &self.node_id_map);
        }
        let mut prepared = session.prepare_commit()?;
        // Note (Devil BLOCKER B2): `set_metadata` is dropped on `commit()`
        // — it never reaches `ChangeEvent`. Kept for advisory logging only;
        // the epoch side-channel is the real echo-prevention mechanism.
        prepared.set_metadata("origin", ORIGIN_LORO_BRIDGE);
        let epoch: EpochId = prepared.commit()?;
        {
            let mut set = self.bridge_origin_epochs.write();
            set.insert(epoch);
        }
        Ok(())
    }
}
