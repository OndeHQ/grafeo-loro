//! Bidirectional Loro↔Grafeo sync engine.
//!
//! Owns the MPSC channels, the Loro subscriber, the inbound batcher, and the
//! three async worker loops (inbound = Loro→Grafeo, outbound = Grafeo→Loro,
//! CDC poller = Grafeo CDC source). All algorithm bodies are `// TODO L3`;
//! the wiring (struct fields, channel plumbing, worker spawn signatures,
//! control-flow shape) is real and compiles.
//!
//! ## Loro concurrency model
//!
//! Loro 1.x uses an **auto-commit** model: there is no `transact_mut()`. The
//! `LoroDoc` is `Send + Sync` and exposes `set_next_commit_origin(&self, &str)`
//! + `commit(&self)` — both `&self`. We still wrap it in `Arc<RwLock<LoroDoc>>`
//! to **logically serialize** the `set_next_commit_origin + commit` pair on
//! the outbound path: holding the write lock across both calls prevents a
//! peer's commit from interleaving between our `set_next_commit_origin` and
//! our `commit`, which would tag the wrong origin on the wrong commit. This
//! is the only reason for the `RwLock`; it is NOT for thread safety.
//!
//! ## Grafeo concurrency model
//!
//! Grafeo 0.5.42 uses a `Session` API (not `begin_write_tx()`): call
//! `db.session_with_cdc(true)` → `session.begin_transaction()` → mutation
//! methods (`create_node_with_props`, `set_node_property`, `delete_node`,
//! ...) → `session.prepare_commit()` → `prepared.set_metadata(k, v)` (note:
//! metadata is **dropped on commit** — see epoch side-channel below) →
//! `prepared.commit() -> Result<EpochId>`.
//!
//! ## Grafeo→Loro echo prevention (Devil BLOCKER B2, orchestrator Gap 1)
//!
//! Grafeo's `ChangeEvent` has no `origin` field and `PreparedCommit::set_metadata`
//! is silently dropped on `commit()` (verified in grafeo-engine-0.5.42 source).
//! We work around this with an **epoch side-channel**: every inbound flush
//! records its commit `EpochId` in `bridge_origin_epochs`. The outbound CDC
//! poller filters any `ChangeEvent` whose `epoch` is in that set. The set is
//! pruned each poll cycle to keep only epochs newer than `last_polled_epoch
//! - EPOCH_RETENTION`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use grafeo::GrafeoDB;
use grafeo_common::types::EpochId;
use loro::LoroDoc;
use parking_lot::RwLock;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::bridge::batcher::MutationBatcher;
use crate::constants::{ORIGIN_GRAFEO_BRIDGE, OUTBOUND_POLL_MS};
use crate::error::Result;
use crate::types::events::{CdcEventWrapper, LoroOp};
// `EPOCH_RETENTION` is referenced in TODO comments inside `spawn_cdc_poller`;
// the L3 implementer will import it from `crate::constants` when filling in
// the prune step.

/// Inbound channel payload: a Loro subscriber event translated to a graph op.
///
/// Single variant at L2; the wrapping enum gives L3 room to add a `RawDiff`
/// variant if it wants to push translation work into the worker instead of
/// the subscriber callback.
pub enum InboundMsg {
    /// A single translated graph mutation op destined for Grafeo.
    Op(LoroOp),
}

/// Outbound channel payload: a Grafeo CDC event destined for Loro.
///
/// Per orchestrator Gap 5 (YAGNI): collapsed from a single-variant enum to a
/// type alias over `CdcEventWrapper`.
pub type OutboundMsg = CdcEventWrapper;

/// Channel capacity for both inbound and outbound MPSC channels.
///
/// 1024 matches the architecture doc §10 example. Backpressure: if the
/// inbound worker stalls, the Loro subscriber's `blocking_send` will block
/// on the Loro commit thread — acceptable for L2; L3 may switch to
/// `try_send` with drop-policy.
const CHANNEL_CAPACITY: usize = 1024;

/// Bidirectional sync engine. Holds shared handles to both stores, the two
/// MPSC channels (senders only — receivers are returned from [`Self::new`]
/// and passed back into the spawn_*_worker methods), the inbound batcher,
/// the Loro subscription handle, and the epoch side-channel set.
pub struct SyncEngine {
    /// Grafeo execution-layer handle (internally thread-safe).
    pub(crate) grafeo_db: Arc<GrafeoDB>,
    /// Loro consensus-layer handle. See module doc for why `RwLock`.
    pub(crate) loro_doc: Arc<RwLock<LoroDoc>>,
    /// Inbound channel: Loro subscriber → inbound worker.
    pub(crate) inbound_tx: mpsc::Sender<InboundMsg>,
    /// Outbound channel: CDC poller → outbound worker.
    pub(crate) outbound_tx: mpsc::Sender<OutboundMsg>,
    /// Holds the `loro::Subscription` returned by `subscribe_root`. Without
    /// this field the subscription would drop on return, immediately
    /// unsubscribing (Devil BLOCKER B3).
    pub(crate) loro_sub: parking_lot::Mutex<Option<loro::Subscription>>,
    /// Epoch side-channel (Devil BLOCKER B2 / orchestrator Gap 1 APPROVED):
    /// every inbound flush inserts its commit `EpochId` here; the outbound
    /// CDC poller filters any `ChangeEvent` whose `epoch` is in this set.
    pub(crate) bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
    /// `loro_key → grafeo::NodeId` mapping (Devil MAJOR M7 / orchestrator
    /// Gap 3 APPROVED). grafeo 0.5.42 has no upsert-by-external-id, so we
    /// look up on each `LoroOp::UpsertNode` and create+insert on miss.
    pub(crate) node_id_map: Arc<RwLock<HashMap<String, grafeo::NodeId>>>,
    /// Inbound mutation batcher. Owned by the engine so the inbound worker
    /// can spawn its `run` loop and forward `LoroOp`s into it.
    pub(crate) batcher: Arc<MutationBatcher>,
    /// Shutdown broadcast — workers subscribe and exit on `trigger()`
    /// (replaces `tokio_util::CancellationToken`, Devil NIT N16).
    pub(crate) shutdown_tx: broadcast::Sender<()>,
}

impl SyncEngine {
    /// Construct a new engine with fresh channels and a fresh shutdown
    /// broadcast. Does NOT subscribe or spawn workers — call
    /// [`Self::init_loro_subscriber`] and [`Self::spawn_all`] explicitly.
    ///
    /// Returns the engine plus the two channel receivers (the senders stay
    /// in the engine). The receivers are passed back into
    /// [`Self::spawn_inbound_worker`] / [`Self::spawn_outbound_worker`] (or
    /// [`Self::spawn_all`]) so each worker owns its receiver exclusively.
    pub fn new(
        grafeo_db: Arc<GrafeoDB>,
        loro_doc: Arc<RwLock<LoroDoc>>,
    ) -> (Self, mpsc::Receiver<InboundMsg>, mpsc::Receiver<OutboundMsg>) {
        let (inbound_tx, inbound_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (outbound_tx, outbound_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (shutdown_tx, _) = broadcast::channel(1);

        let bridge_origin_epochs = Arc::new(RwLock::new(HashSet::new()));
        let node_id_map = Arc::new(RwLock::new(HashMap::new()));

        let batcher = Arc::new(MutationBatcher::new(
            grafeo_db.clone(),
            crate::constants::DEFAULT_BATCH_SIZE,
            crate::constants::DEFAULT_BATCH_MS,
            bridge_origin_epochs.clone(),
            node_id_map.clone(),
            shutdown_tx.clone(),
        ));

        let engine = Self {
            grafeo_db,
            loro_doc,
            inbound_tx,
            outbound_tx,
            loro_sub: parking_lot::Mutex::new(None),
            bridge_origin_epochs,
            node_id_map,
            batcher,
            shutdown_tx,
        };
        (engine, inbound_rx, outbound_rx)
    }

    /// Wire `loro_doc.subscribe_root` → origin filter → translate to `LoroOp`
    /// → `inbound_tx.blocking_send(InboundMsg::Op(...))`. Stores the returned
    /// [`loro::Subscription`] in `self.loro_sub` so it lives as long as the
    /// engine (Devil BLOCKER B3 — without this, the sub drops on return).
    pub fn init_loro_subscriber(&self) -> Result<()> {
        let inbound_tx = self.inbound_tx.clone();
        // `subscribe_root(&self, Subscriber)` — read guard suffices.
        let doc = self.loro_doc.read();

        let handler: loro::event::Subscriber = Arc::new(move |event: loro::event::DiffEvent<'_>| {
            // Drop events generated by our own outbound bridge (echo).
            if event.origin == ORIGIN_GRAFEO_BRIDGE {
                return;
            }
            // TODO L3: translate `event` (DiffEvent) → Vec<LoroOp>.
            // For now, the translation is a no-op — L3 will walk
            // `event.events: Vec<ContainerDiff>` and project root-container
            // diffs into `LoroOp::UpsertNode { loro_key, labels, properties }`
            // etc.
            let _ops: Vec<LoroOp> = Vec::new();
            // TODO L3: for op in ops { let _ = inbound_tx.blocking_send(InboundMsg::Op(op)); }
            let _ = &inbound_tx;
        });

        let sub = doc.subscribe_root(handler);
        *self.loro_sub.lock() = Some(sub);
        Ok(())
    }

    /// Inbound worker: drain `rx`, extract `LoroOp` from each `InboundMsg`,
    /// forward to the internal batcher channel. The batcher's `run` loop is
    /// spawned as a child task and joined on shutdown.
    pub async fn spawn_inbound_worker(
        self: Arc<Self>,
        mut rx: mpsc::Receiver<InboundMsg>,
    ) -> JoinHandle<()> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let batcher = self.batcher.clone();
        let (batch_tx, batch_rx) = mpsc::channel::<LoroOp>(CHANNEL_CAPACITY);

        // Spawn the batcher's run loop as a child task.
        let batcher_handle = tokio::spawn(async move {
            let _ = batcher.run(batch_rx).await;
        });

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.recv() => break,
                    msg = rx.recv() => {
                        let Some(msg) = msg else { break };
                        match msg {
                            InboundMsg::Op(op) => {
                                // TODO L3: backpressure policy — currently
                                // `await` on full channel, which blocks the
                                // forwarder but does NOT block the Loro
                                // subscriber (which uses `blocking_send` on
                                // `inbound_tx`). L3 may switch to `try_send`
                                // with drop-on-full.
                                if batch_tx.send(op).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            // Signal the batcher to drain + flush on its own shutdown select.
            drop(batch_tx);
            let _ = batcher_handle.await;
        })
    }

    /// Outbound worker: drain `rx` (CDC events from the poller), filter via
    /// epoch side-channel (already filtered at poll time, but double-check),
    /// apply to Loro with `set_next_commit_origin(ORIGIN_GRAFEO_BRIDGE)` +
    /// `commit()` serialized under the Loro write lock.
    pub async fn spawn_outbound_worker(
        self: Arc<Self>,
        mut rx: mpsc::Receiver<OutboundMsg>,
    ) -> JoinHandle<()> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let loro_doc = self.loro_doc.clone();
        let bridge_epochs = self.bridge_origin_epochs.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.recv() => break,
                    msg = rx.recv() => {
                        let Some(msg) = msg else { break };
                        // Defensive double-check: poller already filters, but
                        // an epoch could in principle have been pruned between
                        // poll and apply. Skip if still in the set.
                        {
                            let epochs = bridge_epochs.read();
                            if epochs.contains(&msg.epoch) {
                                continue;
                            }
                        }

                        // TODO L3: translate `msg.payload: grafeo::cdc::ChangeEvent`
                        // into Loro mutations. Hold the Loro write lock
                        // across `set_next_commit_origin + commit` so the
                        // origin tag lands on OUR commit, not a peer's.
                        {
                            let doc = loro_doc.write();
                            doc.set_next_commit_origin(ORIGIN_GRAFEO_BRIDGE);
                            // TODO L3: project ChangeEvent → LoroMap/LoroList
                            // mutations on the appropriate root container
                            // (ROOT_VERTICES / ROOT_EDGES / ROOT_TREE).
                            let _ = &doc;
                            doc.commit();
                        }
                    }
                }
            }
        })
    }

    /// CDC poller (Devil MAJOR M6): poll `session.changes_between(start,
    /// end)` on a timer, filter out epochs in `bridge_origin_epochs`, push
    /// surviving events to the outbound channel. Prune the epoch set each
    /// cycle to keep only epochs newer than `last_epoch - EPOCH_RETENTION`.
    pub async fn spawn_cdc_poller(self: Arc<Self>) -> JoinHandle<()> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let grafeo_db = self.grafeo_db.clone();
        let outbound_tx = self.outbound_tx.clone();
        let bridge_epochs = self.bridge_origin_epochs.clone();

        tokio::spawn(async move {
            // TODO L3: read initial epoch from grafeo (e.g.
            // `grafeo_db.current_epoch()`). For L2 wiring we start at 0 so
            // the poll loop shape compiles; L3 will set this to the actual
            // last-polled epoch (possibly persisted across restarts).
            let mut last_epoch = grafeo_common::types::EpochId::new(0);
            let poll_interval = std::time::Duration::from_millis(OUTBOUND_POLL_MS);

            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.recv() => break,
                    _ = tokio::time::sleep(poll_interval) => {
                        // TODO L3: actual poll body:
                        //   let session = grafeo_db.session_with_cdc(true);
                        //   let current = grafeo_db.current_epoch();
                        //   if current <= last_epoch { continue; }
                        //   let events = session.changes_between(last_epoch, current)?;
                        //   let epochs_guard = bridge_epochs.read();
                        //   for ev in events {
                        //       if epochs_guard.contains(&ev.epoch) { continue; }
                        //       let wrapped = OutboundMsg {
                        //           epoch: ev.epoch,
                        //           payload: ev,
                        //       };
                        //       if outbound_tx.send(wrapped).await.is_err() { break; }
                        //   }
                        //   drop(epochs_guard);
                        //   // Prune: keep only epochs > last_epoch - EPOCH_RETENTION.
                        //   {
                        //       let mut set = bridge_epochs.write();
                        //       let cutoff = grafeo_common::types::EpochId::new(
                        //           last_epoch.as_u64().saturating_sub(EPOCH_RETENTION),
                        //       );
                        //       set.retain(|e| *e > cutoff);
                        //   }
                        //   last_epoch = current;
                        let _ = (&mut last_epoch, &grafeo_db, &outbound_tx, &bridge_epochs);
                    }
                }
            }
        })
    }

    /// Convenience: initialize the Loro subscriber and spawn all three
    /// worker tasks (inbound, outbound, CDC poller). Returns the three
    /// `JoinHandle`s in spawn order. The caller is responsible for
    /// awaiting them on shutdown.
    pub async fn spawn_all(
        self: Arc<Self>,
        inbound_rx: mpsc::Receiver<InboundMsg>,
        outbound_rx: mpsc::Receiver<OutboundMsg>,
    ) -> Vec<JoinHandle<()>> {
        // Subscribe before spawning workers so no Loro events are missed.
        // `init_loro_subscriber` stores the subscription in `self.loro_sub`.
        let _ = self.init_loro_subscriber();

        let inbound = self.clone().spawn_inbound_worker(inbound_rx).await;
        let outbound = self.clone().spawn_outbound_worker(outbound_rx).await;
        let poller = self.clone().spawn_cdc_poller().await;

        vec![inbound, outbound, poller]
    }

    /// Signal all worker loops to drain and exit.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Expose the inbound sender so external callers (e.g. tests) can push
    /// `InboundMsg`s directly without a real Loro subscriber.
    pub fn inbound_sender(&self) -> mpsc::Sender<InboundMsg> {
        self.inbound_tx.clone()
    }

    /// Expose the outbound sender so external callers (e.g. tests) can push
    /// `OutboundMsg`s directly without a real CDC poller.
    pub fn outbound_sender(&self) -> mpsc::Sender<OutboundMsg> {
        self.outbound_tx.clone()
    }
}
