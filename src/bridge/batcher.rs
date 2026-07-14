//! Inbound mutation batcher: collects `LoroOp`s and flushes them as a single
//! vectorized Grafeo transaction tagged with `ORIGIN_LORO_BRIDGE`.
//!
//! Issue #1 compliance: gated by `batcher` feature (pulls `tokio`). The
//! `apply_loro_op` calls inside `flush_inner` require `grafeo` (the
//! `Session` API). Both features must be on for the batcher to be useful.
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
use opentelemetry::trace::Tracer;
use parking_lot::Mutex;
use parking_lot::RwLock;
use tokio::sync::{broadcast, mpsc};
use tokio::time::Duration;
use tracing::instrument;

use crate::bridge::grafeo_tx::{apply_loro_op, BridgeMaps};
use crate::constants::{DEFAULT_BATCH_MS, DEFAULT_BATCH_SIZE, ORIGIN_LORO_BRIDGE};
use crate::error::{GrafeoLoroError, Result};
use crate::telemetry::{HealthProbe, MetricsRegistry, SharedTracer};
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
    /// Optional metrics registry (P5-L1 Task 4 wiring contact point).
    /// `Some` in production (threaded from `GrafeoLoroAppBuilder::build`);
    /// `None` in tests that do not configure telemetry. `flush_inner` records
    /// `batch_flush_duration` + bumps `inbound_events` counter via this
    /// handle (P5-L3 — `flush_inner` records metrics post-commit).
    pub(crate) metrics: Option<Arc<MetricsRegistry>>,
    /// Optional shared tracer (P5-L1 Task 4 wiring contact point). `Some` in
    /// production; `None` in tests. `flush_inner` opens a `batch_flush` child
    /// span via this handle (P5-L3 territory).
    pub(crate) tracer: Option<SharedTracer>,
    /// Optional health probe (P5-L2 Devil M3 / Q10 — symmetric with `metrics`
    /// + `tracer`). `Some` in production; `None` in tests. `flush_inner` calls
    ///   `health.update_sync_ts()` after each successful commit so the inbound
    ///   flush path stamps `last_sync_ts` (architecture §23.3 — "last sync" =
    ///   both inbound flush AND outbound commit).
    pub(crate) health: Option<Arc<HealthProbe>>,
}

/// Configuration bundle for [`MutationBatcher::new`]. Groups the 8 non-core
/// construction params into a single struct — replaces the prior 9-arg
/// signature (P7 `too_many_arguments` refactor, anti-plenger #5).
pub struct BatcherConfig {
    pub batch_size: usize,
    pub batch_ms: u64,
    pub bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
    pub maps: Arc<BridgeMaps>,
    pub shutdown_tx: broadcast::Sender<()>,
    pub metrics: Option<Arc<MetricsRegistry>>,
    pub tracer: Option<SharedTracer>,
    pub health: Option<Arc<HealthProbe>>,
}

impl MutationBatcher {
    /// Construct a batcher with explicit tuning. Shared `bridge_origin_epochs`
    /// and `maps` are owned by the parent `SyncEngine` and passed in by `Arc`
    /// clone so both the batcher and the engine/poller see the same state.
    ///
    /// # Phase 5 Task 4 wiring (P5-L1)
    ///
    /// `metrics` + `tracer` + `health` are P5-L1/L2 additions (Option so test
    /// constructors that do not configure telemetry can pass `None`). Production
    /// `GrafeoLoroAppBuilder::build` threads `Some(Arc::clone(&metrics))` +
    /// `Some(Arc::clone(&tracer))` + `Some(Arc::clone(&health))` here (P5-L2
    /// wired the parameter list — bodies filled P5-L3).
    pub fn new(grafeo_db: Arc<GrafeoDB>, config: BatcherConfig) -> Self {
        Self {
            grafeo_db,
            buffer: Mutex::new(Vec::new()),
            batch_size: config.batch_size,
            batch_ms: config.batch_ms,
            bridge_origin_epochs: config.bridge_origin_epochs,
            maps: config.maps,
            shutdown_tx: config.shutdown_tx,
            metrics: config.metrics,
            tracer: config.tracer,
            health: config.health,
        }
    }

    /// Construct a batcher using [`DEFAULT_BATCH_SIZE`] and [`DEFAULT_BATCH_MS`].
    ///
    /// Convenience wrapper around [`Self::new`] with default batch sizing.
    /// Callers that do not configure telemetry pass `None` for metrics/tracer/health.
    ///
    /// Currently unused — `SyncEngine::new_inner` calls `Self::new` directly
    /// with a `BatcherConfig` bundle. Kept for future ergonomic callers.
    #[allow(dead_code)]
    pub fn with_defaults(
        grafeo_db: Arc<GrafeoDB>,
        bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
        maps: Arc<BridgeMaps>,
        shutdown_tx: broadcast::Sender<()>,
        metrics: Option<Arc<MetricsRegistry>>,
        tracer: Option<SharedTracer>,
        health: Option<Arc<HealthProbe>>,
    ) -> Self {
        Self::new(
            grafeo_db,
            BatcherConfig {
                batch_size: DEFAULT_BATCH_SIZE,
                batch_ms: DEFAULT_BATCH_MS,
                bridge_origin_epochs,
                maps,
                shutdown_tx,
                metrics,
                tracer,
                health,
            },
        )
    }

    /// Access the optional metrics registry (P5-L1). Used by tests + future
    /// telemetry-aware callers to inspect the registered instruments.
    #[allow(dead_code)]
    pub fn metrics(&self) -> Option<&Arc<MetricsRegistry>> {
        self.metrics.as_ref()
    }

    /// Access the optional shared tracer (P5-L1). Used by tests + future
    /// telemetry-aware callers to inspect the configured tracer.
    #[allow(dead_code)]
    pub fn tracer(&self) -> Option<&SharedTracer> {
        self.tracer.as_ref()
    }

    /// Access the optional health probe (P5-L2 Devil M3 / Q10 — symmetric
    /// with `metrics()` + `tracer()`). `Some` in production; `None` in tests.
    /// `flush_inner` calls `health.update_sync_ts()` after each successful
    /// commit so the inbound flush path stamps `last_sync_ts` (architecture
    /// §23.3 — "last sync" = both inbound flush AND outbound commit).
    #[allow(dead_code)]
    pub fn health(&self) -> Option<&Arc<HealthProbe>> {
        self.health.as_ref()
    }

    /// Main loop: `tokio::select!` between (a) `rx.recv()` → push +
    /// size-threshold flush, (b) interval tick → flush, (c) shutdown →
    /// flush remaining + exit. Returns when shutdown fires AND the final
    /// flush completes.
    #[instrument(skip(self, rx), name = "batcher_run", level = "info")]
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
        // P5-L3: capture telemetry handles into the blocking closure so it
        // can emit the `grafeo_commit` grandchild span around `prepared.commit()`
        // (architecture §23.2 line 1048). Cloning `Option<Arc<...>>` is cheap
        // (Arc refcount bump).
        let blocking_tracer = self.tracer.clone();

        // P5-L3: open `batch_flush` parent span (architecture §23.2 tree row
        // 2.2). Held across the `spawn_blocking` await so the span covers the
        // entire Grafeo commit latency. Optional — `None` in tests / dev mode
        // without telemetry configured.
        let _batch_flush_span = self
            .tracer
            .as_ref()
            .map(|t| t.as_ref().span_builder("batch_flush").start(t.as_ref()));

        // P5-L3: capture start time BEFORE `spawn_blocking` so `elapsed_ms`
        // includes the blocking-pool scheduling + queue delay (the timeout
        // wrapping makes a simple `.await` duration insufficient). Use
        // `std::time::Instant` (NOT `tokio::time::Instant`) — we're in the
        // async context but measuring wall-clock, not runtime time.
        let started = std::time::Instant::now();

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
            // P5-L3 M2: emit `grafeo_commit` grandchild span around
            // `prepared.commit()` (architecture §23.2 line 1048 — Devil M2
            // overruled L1's YAGNI objection). The grandchild captures
            // commit-specific latency separately from the surrounding
            // `apply_loro_op` + `prepare_commit` work. Created inside the
            // `spawn_blocking` closure on the same thread as the commit call.
            // Note: parent-child linking requires OTel Context propagation
            // across `spawn_blocking` (out of scope for P5-L3 — the span name
            // is the contract; hierarchy is reconstructed in Jaeger by name
            // when no Context is propagated).
            let epoch: EpochId = {
                let _grafeo_commit_span = blocking_tracer
                    .as_ref()
                    .map(|t| t.as_ref().span_builder("grafeo_commit").start(t.as_ref()));
                prepared.commit()?
            };
            epochs.write().insert(epoch);
            Ok(())
        });

        // P5-L3: on successful flush, record metrics + stamp `last_sync_ts`.
        // Architecture §23.1 row 1 (inbound_events_total) + row 4
        // (batch_flush_duration_ms with `batch_size` label). Architecture
        // §23.3 — "last sync" = both inbound flush AND outbound commit, so
        // the batcher's flush path stamps `last_sync_ts` symmetric with the
        // outbound worker's post-Loro-commit stamp (Devil M3 / Q10).
        let outcome = tokio::time::timeout(FLUSH_TIMEOUT, blocking).await;
        if matches!(outcome, Ok(Ok(Ok(())))) {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if let Some(m) = &self.metrics {
                m.record_batch_flush(elapsed_ms, op_count as u64);
                // `inbound_events` is bumped per-op at the forward boundary
                // in `sync_engine.rs` (Devil Q12 — per-op forward, NOT
                // per-flush aggregate). Bumping it here too would double-
                // count: a 5-op batch would report 10 (P5-HUNT-1 MAJOR 1).
            }
            if let Some(h) = &self.health {
                h.update_sync_ts();
            }
        }
        // `_batch_flush_span` drops here on function return — after the
        // `spawn_blocking` await + metrics recording, so the span covers the
        // entire flush lifecycle.
        match outcome {
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
