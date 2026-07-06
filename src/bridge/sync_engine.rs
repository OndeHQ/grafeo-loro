//! Bidirectional Loro↔Grafeo sync engine.
//!
//! Owns the MPSC channels, the Loro subscriber, the inbound batcher, and the
//! three async worker loops (inbound = Loro→Grafeo, outbound = Grafeo→Loro,
//! CDC poller = Grafeo CDC source). All algorithm bodies are real per L3.
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
//!
//! ## Backpressure policy (L2 new issue #3)
//!
//! - **Loro subscriber** (sync handler): `try_send` — non-blocking because
//!   the subscriber is invoked synchronously by `LoroDoc::commit`, which may
//!   itself run inside an async runtime (the outbound worker holds the Loro
//!   write lock across `set_next_commit_origin + commit`). `blocking_send`
//!   would panic in that case. On `Full`/`Closed`, log warn and drop the op.
//! - **CDC poller** (async): `send().await` — awaits space on the outbound
//!   channel. If `send` returns `Err` (channel closed), log warn and break
//!   the poll loop (workers are shutting down).
//! - **Inbound forwarder** (async): `batch_tx.send().await` — awaits space
//!   on the batcher channel. Same shutdown semantics.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use grafeo::GrafeoDB;
use grafeo_common::types::EpochId;
use loro::LoroDoc;
use parking_lot::RwLock;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::bridge::batcher::MutationBatcher;
use crate::bridge::grafeo_tx::{BridgeMaps, EdgeKey};
use crate::constants::{
    ORIGIN_GRAFEO_BRIDGE, ORIGIN_LORO_BRIDGE, OUTBOUND_POLL_MS, ROOT_EDGES, ROOT_VERTICES,
};
use crate::error::Result;
use crate::types::events::{CdcEventWrapper, LoroOp};
use crate::types::values::{grafeo_value_to_lval, lval_to_gval};
use crate::constants::EPOCH_RETENTION;

/// Inbound channel payload: a Loro subscriber event translated to a graph op.
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
/// 1024 matches the architecture doc §10 example. See module doc for
/// backpressure policy (subscriber uses `try_send` to avoid blocking the
/// Loro commit thread inside an async runtime).
const CHANNEL_CAPACITY: usize = 1024;

/// Bidirectional sync engine. Holds shared handles to both stores, the two
/// MPSC channels (senders only — receivers are returned from [`Self::new`]
/// and passed back into the spawn_*_worker methods), the inbound batcher,
/// the Loro subscription handle, the epoch side-channel set, and the shared
/// `BridgeMaps` (forward + inverse id lookups).
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
    /// Shared `loro_key ↔ grafeo::NodeId` and `EdgeKey ↔ grafeo::EdgeId`
    /// maps (Devil MAJOR M7 / orchestrator Gap 3 APPROVED + L3 inverse maps).
    pub(crate) maps: Arc<BridgeMaps>,
    /// Inbound mutation batcher. Owned by the engine so the inbound worker
    /// can spawn its `run` loop and forward `LoroOp`s into it.
    pub(crate) batcher: Arc<MutationBatcher>,
    /// Counter incremented by the Loro subscriber handler every time an op
    /// is successfully `try_send`-ed into the inbound channel (i.e. every
    /// non-echo event that survives the origin filter). Tests inspect this
    /// to deterministically assert no echo occurs during a settle window —
    /// the previous snapshot-comparison approach was timing-dependent
    /// (Hunter MAJOR 3).
    pub(crate) inbound_event_count: Arc<AtomicU64>,
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
        let maps = Arc::new(BridgeMaps::new());
        let inbound_event_count = Arc::new(AtomicU64::new(0));

        let batcher = Arc::new(MutationBatcher::new(
            grafeo_db.clone(),
            crate::constants::DEFAULT_BATCH_SIZE,
            crate::constants::DEFAULT_BATCH_MS,
            bridge_origin_epochs.clone(),
            maps.clone(),
            shutdown_tx.clone(),
        ));

        let engine = Self {
            grafeo_db,
            loro_doc,
            inbound_tx,
            outbound_tx,
            loro_sub: parking_lot::Mutex::new(None),
            bridge_origin_epochs,
            maps,
            batcher,
            inbound_event_count,
            shutdown_tx,
        };
        (engine, inbound_rx, outbound_rx)
    }

    /// Accessor for the shared id-mapping state (L2 new issue #1 —
    /// `node_id_map` was previously unread on `SyncEngine`; bundling into
    /// `BridgeMaps` and exposing via accessor resolves the warning and gives
    /// tests a hook to inspect state).
    pub fn maps(&self) -> &Arc<BridgeMaps> {
        &self.maps
    }

    /// Wire `loro_doc.subscribe_root` → origin filter → translate to `LoroOp`
    /// → `inbound_tx.try_send(InboundMsg::Op(...))`. Stores the returned
    /// [`loro::Subscription`] in `self.loro_sub` so it lives as long as the
    /// engine (Devil BLOCKER B3 — without this, the sub drops on return).
    ///
    /// Backpressure (L2 new issue #3): the subscriber callback is invoked
    /// synchronously by `LoroDoc::commit`, which may itself run inside an
    /// async runtime (e.g. when the outbound worker commits a Loro write).
    /// `blocking_send` would panic in that case, so we use `try_send`: if
    /// the channel is full or closed, we log a warning and drop the op.
    pub fn init_loro_subscriber(&self) -> Result<()> {
        let inbound_tx = self.inbound_tx.clone();
        let inbound_event_count = self.inbound_event_count.clone();
        // `subscribe_root(&self, Subscriber)` — read guard suffices.
        let doc = self.loro_doc.read();

        let handler: loro::event::Subscriber = Arc::new(move |event: loro::event::DiffEvent<'_>| {
            // Drop events generated by our own bridge (echo prevention).
            //
            // `ORIGIN_GRAFEO_BRIDGE` tags the outbound worker's Loro writes
            // (Grafeo→Loro direction). `ORIGIN_LORO_BRIDGE` tags the local
            // RYOW path in `VertexBuilder::commit` (P2T3-L2 BLOCKER B1) —
            // `commit()` writes Loro first, then Grafeo; without this filter
            // clause the synchronous subscriber would re-apply the same vertex
            // to Grafeo via the batcher, producing either a duplicate
            // label-less node (race case — `translate_diff_event` always
            // emits `labels: Vec::new()`, see P2T3-DEVIL M4) or a spurious
            // no-op Grafeo commit polluting the epoch side-channel (common
            // case). Phase 1 tests never set `ORIGIN_LORO_BRIDGE` as a Loro
            // commit origin (the constant is only used as advisory
            // `PreparedCommit::set_metadata`, which is dropped on commit),
            // so the extension is a no-op for existing tests.
            if event.origin == ORIGIN_GRAFEO_BRIDGE || event.origin == ORIGIN_LORO_BRIDGE {
                return;
            }
            let ops = translate_diff_event(&event);
            for op in ops {
                if let Err(e) = inbound_tx.try_send(InboundMsg::Op(op)) {
                    tracing::warn!(error = %e, "inbound channel full or closed; dropping LoroOp");
                    return;
                }
                // Count successful sends — gives tests a deterministic hook to
                // assert no echo occurred during a settle window (Hunter MAJOR 3).
                inbound_event_count.fetch_add(1, Ordering::Relaxed);
            }
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
        let maps = self.maps.clone();

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
                        if bridge_epochs.read().contains(&msg.epoch) {
                            continue;
                        }
                        let outcome = {
                            let doc = loro_doc.write();
                            apply_change_event_to_loro(&doc, &msg.payload, &maps)
                        };
                        if let Err(e) = outcome {
                            tracing::warn!(error = %e, "outbound translation skipped event");
                            continue;
                        }
                        {
                            let doc = loro_doc.write();
                            doc.set_next_commit_origin(ORIGIN_GRAFEO_BRIDGE);
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
            // L2 new issue #4: init from current_epoch so a restarted engine
            // does not re-replay the entire CDC history from epoch 0.
            let mut last_epoch = grafeo_db.current_epoch();
            let poll_interval = std::time::Duration::from_millis(OUTBOUND_POLL_MS);

            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.recv() => break,
                    _ = tokio::time::sleep(poll_interval) => {
                        let session = grafeo_db.session_with_cdc(true);
                        let current = grafeo_db.current_epoch();
                        if current <= last_epoch {
                            continue;
                        }
                        let events = match session.changes_between(last_epoch, current) {
                            Ok(ev) => ev,
                            Err(e) => {
                                tracing::warn!(error = %e, "cdc changes_between failed");
                                continue;
                            }
                        };
                        for ev in events {
                            if bridge_epochs.read().contains(&ev.epoch) {
                                continue;
                            }
                            let wrapped = OutboundMsg::new(ev.epoch, ev);
                            if outbound_tx.send(wrapped).await.is_err() {
                                tracing::warn!(
                                    "outbound channel closed; stopping CDC poller"
                                );
                                return;
                            }
                        }
                        // Prune: keep only epochs > last_epoch - EPOCH_RETENTION.
                        {
                            let mut set = bridge_epochs.write();
                            let cutoff = EpochId::new(
                                last_epoch.as_u64().saturating_sub(EPOCH_RETENTION),
                            );
                            set.retain(|e| *e > cutoff);
                        }
                        last_epoch = current;
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

    /// Number of ops the Loro subscriber has successfully pushed into the
    /// inbound channel since engine construction. Each successful `try_send`
    /// increments this counter; events filtered by the origin check (echoes)
    /// and ops dropped on a full/closed channel do NOT increment it. Tests
    /// use this to deterministically assert no echo occurred during a settle
    /// window (Hunter MAJOR 3 — replacing the timing-dependent Loro
    /// snapshot-comparison check).
    pub fn inbound_event_count(&self) -> u64 {
        self.inbound_event_count.load(Ordering::Relaxed)
    }
}

/// Pure projection from a Loro `DiffEvent` to a Vec of `LoroOp`s. Walks
/// `event.events: Vec<ContainerDiff>` and inspects each container diff. Root
/// map containers `V` (vertices) and `E` (edges) are projected to
/// `UpsertNode`/`DeleteNode` and `UpsertEdge`/`DeleteEdge` respectively.
/// Other diffs (Tree, Text, List, Counter, Unknown) are logged and skipped.
fn translate_diff_event(event: &loro::event::DiffEvent<'_>) -> Vec<LoroOp> {
    let mut ops = Vec::new();
    for cd in &event.events {
        let name = match cd.target {
            loro::ContainerID::Root { name, .. } => name.as_ref(),
            _ => continue,
        };
        match name {
            ROOT_VERTICES => {
                if let loro::event::Diff::Map(map_delta) = &cd.diff {
                    for (key, val) in map_delta.updated.iter() {
                        match val {
                            Some(loro::ValueOrContainer::Value(lv)) => match lv {
                                loro::LoroValue::Map(m) => {
                                    let mut props = std::collections::HashMap::new();
                                    let mut ok = true;
                                    for (k, v) in m.iter() {
                                        match lval_to_gval(v.clone()) {
                                            Ok(gv) => {
                                                props.insert(k.clone(), gv);
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    error = %e,
                                                    key = %k,
                                                    "skipping unsupported vertex property"
                                                );
                                                ok = false;
                                                break;
                                            }
                                        }
                                    }
                                    if ok {
                                        ops.push(LoroOp::UpsertNode {
                                            loro_key: key.to_string(),
                                            labels: Vec::new(),
                                            properties: props,
                                        });
                                    }
                                }
                                other => {
                                    tracing::warn!(
                                        key = %key,
                                        "vertex value is not a LoroValue::Map (got {:?}); skipping",
                                        std::mem::discriminant(other)
                                    );
                                }
                            },
                            None => ops.push(LoroOp::DeleteNode {
                                loro_key: key.to_string(),
                            }),
                            Some(loro::ValueOrContainer::Container(_)) => {
                                tracing::warn!(
                                    key = %key,
                                    "vertex value is a container ref; skipping"
                                );
                            }
                        }
                    }
                }
            }
            ROOT_EDGES => {
                if let loro::event::Diff::Map(map_delta) = &cd.diff {
                    for (key, val) in map_delta.updated.iter() {
                        let parsed = parse_edge_key(key);
                        let Some((src_key, dst_key, label)) = parsed else {
                            tracing::warn!(key = %key, "unparseable edge key; skipping");
                            continue;
                        };
                        match val {
                            Some(loro::ValueOrContainer::Value(loro::LoroValue::Map(m))) => {
                                let mut props = std::collections::HashMap::new();
                                let mut ok = true;
                                for (k, v) in m.iter() {
                                    match lval_to_gval(v.clone()) {
                                        Ok(gv) => {
                                            props.insert(k.clone(), gv);
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                error = %e,
                                                key = %k,
                                                "skipping unsupported edge property"
                                            );
                                            ok = false;
                                            break;
                                        }
                                    }
                                }
                                if ok {
                                    ops.push(LoroOp::UpsertEdge {
                                        src_key,
                                        dst_key,
                                        label,
                                        properties: props,
                                    });
                                }
                            }
                            None => ops.push(LoroOp::DeleteEdge {
                                src_key,
                                dst_key,
                                label,
                            }),
                            Some(_other) => {
                                tracing::warn!(
                                    key = %key,
                                    "edge value is not a LoroValue::Map; skipping"
                                );
                            }
                        }
                    }
                }
            }
            _ => {
                tracing::trace!(container = %name, "non V/E root container diff; skipping");
            }
        }
    }
    ops
}

/// Parse an edge Loro-map key `"src_key|dst_key|label"` into its tuple. Returns
/// `None` if the format is wrong (fewer than 2 `|` separators).
fn parse_edge_key(s: &str) -> Option<EdgeKey> {
    let mut parts = s.splitn(3, '|');
    let src = parts.next()?.to_string();
    let dst = parts.next()?.to_string();
    let label = parts.next()?.to_string();
    if label.is_empty() {
        return None;
    }
    Some((src, dst, label))
}

/// Encode an `EdgeKey` tuple back to `"src_key|dst_key|label"` for Loro map keys.
fn encode_edge_key(key: &EdgeKey) -> String {
    format!("{}|{}|{}", key.0, key.1, key.2)
}

/// Pure projection from a grafeo `ChangeEvent` to Loro mutations on the
/// appropriate root container (`V` for nodes, `E` for edges). Triple events
/// and unmapped entity ids are skipped with a warn log (no echo, no panic).
fn apply_change_event_to_loro(
    doc: &LoroDoc,
    event: &grafeo::cdc::ChangeEvent,
    maps: &BridgeMaps,
) -> Result<()> {
    use grafeo::cdc::{ChangeKind, EntityId};
    match (event.entity_id, event.kind.clone()) {
        (EntityId::Node(node_id), ChangeKind::Create | ChangeKind::Update) => {
            let node_key = match maps.node_key_map.read().get(&node_id) {
                Some(k) => k.clone(),
                None => {
                    tracing::warn!(
                        node_id = node_id.as_u64(),
                        "outbound node event skipped: no loro_key mapping (node not created via bridge)"
                    );
                    return Ok(());
                }
            };
            let v_map = doc.get_map(ROOT_VERTICES);
            // Read-modify-write: grafeo's CDC `after` for an Update is just
            // the changed keys (not the full property set). Merge into the
            // existing LoroMap value to avoid clobbering untouched props.
            let mut current = match v_map.get(&node_key) {
                Some(loro::ValueOrContainer::Value(loro::LoroValue::Map(m))) => {
                    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                }
                _ => std::collections::HashMap::new(),
            };
            for (k, v) in event.after.clone().unwrap_or_default() {
                current.insert(k, grafeo_value_to_lval(&v));
            }
            v_map.insert(&node_key, loro::LoroValue::Map(current.into()))?;
        }
        (EntityId::Node(node_id), ChangeKind::Delete) => {
            let node_key = match maps.node_key_map.read().get(&node_id) {
                Some(k) => k.clone(),
                None => {
                    tracing::warn!(
                        node_id = node_id.as_u64(),
                        "outbound node-delete event skipped: no loro_key mapping"
                    );
                    return Ok(());
                }
            };
            let v_map = doc.get_map(ROOT_VERTICES);
            let _ = v_map.delete(&node_key);
            // Best-effort map cleanup: the grafeo node is gone, so any stale
            // forward/inverse entry in our maps is now unreachable.
            maps.remove_node(&node_key);
        }
        (EntityId::Edge(edge_id), ChangeKind::Create) => {
            // Create events populate src_id/dst_id/edge_type — use
            // `lookup_edge_endpoints` to translate them to Loro keys.
            let (src_key, dst_key, label) = match lookup_edge_endpoints(event, &maps) {
                Some(t) => t,
                None => return Ok(()),
            };
            let key: EdgeKey = (src_key, dst_key, label);
            let e_map = doc.get_map(ROOT_EDGES);
            let loro_key = encode_edge_key(&key);
            let mut current = match e_map.get(&loro_key) {
                Some(loro::ValueOrContainer::Value(loro::LoroValue::Map(m))) => {
                    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                }
                _ => std::collections::HashMap::new(),
            };
            for (k, v) in event.after.clone().unwrap_or_default() {
                current.insert(k, grafeo_value_to_lval(&v));
            }
            e_map.insert(&loro_key, loro::LoroValue::Map(current.into()))?;
            // Record the grafeo EdgeId ↔ EdgeKey binding so a subsequent
            // EdgeDelete event (and EdgeUpdate events — see below) can be
            // reverse-translated.
            maps.insert_edge(key, edge_id);
        }
        (EntityId::Edge(edge_id), ChangeKind::Update) => {
            // Hunter MAJOR 2: grafeo's `record_update` sets `src_id`,
            // `dst_id`, and `edge_type` to `None` for ALL Update events
            // (verified in grafeo-engine-0.5.42/src/cdc.rs:~432). Calling
            // `lookup_edge_endpoints` here would always return `None` and
            // edge property updates from Grafeo→Loro would be silently
            // dropped. Instead, look up the EdgeKey via the binding recorded
            // at Create time. If the edge was created before the bridge
            // started (no binding), log + skip.
            let key = match maps.edge_key_map.read().get(&edge_id).cloned() {
                Some(k) => k,
                None => {
                    tracing::warn!(
                        edge_id = edge_id.as_u64(),
                        "outbound edge-update event skipped: no EdgeKey mapping (edge not created via bridge)"
                    );
                    return Ok(());
                }
            };
            let e_map = doc.get_map(ROOT_EDGES);
            let loro_key = encode_edge_key(&key);
            let mut current = match e_map.get(&loro_key) {
                Some(loro::ValueOrContainer::Value(loro::LoroValue::Map(m))) => {
                    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                }
                _ => std::collections::HashMap::new(),
            };
            for (k, v) in event.after.clone().unwrap_or_default() {
                current.insert(k, grafeo_value_to_lval(&v));
            }
            e_map.insert(&loro_key, loro::LoroValue::Map(current.into()))?;
        }
        (EntityId::Edge(edge_id), ChangeKind::Delete) => {
            let key = match maps.remove_edge_by_id(edge_id) {
                Some(k) => k,
                None => {
                    tracing::warn!(
                        edge_id = edge_id.as_u64(),
                        "outbound edge-delete event skipped: no EdgeKey mapping"
                    );
                    return Ok(());
                }
            };
            let e_map = doc.get_map(ROOT_EDGES);
            let _ = e_map.delete(&encode_edge_key(&key));
        }
        (EntityId::Triple(_), _) => {
            tracing::trace!("triple CDC events are not translated in Phase 1");
        }
        // `EntityId` and `ChangeKind` are both `#[non_exhaustive]`; future
        // variants are logged and skipped rather than panicking.
        _ => {
            tracing::warn!(
                entity_id = ?event.entity_id,
                kind = ?event.kind,
                "unmapped CDC event; skipping"
            );
        }
    }
    Ok(())
}

/// Resolve a `ChangeEvent`'s `src_id`/`dst_id`/`edge_type` into Loro-side
/// `(src_key, dst_key, label)` via `node_key_map`. Returns `None` if either
/// endpoint lacks an inverse mapping (the node was not created via the bridge).
fn lookup_edge_endpoints(
    event: &grafeo::cdc::ChangeEvent,
    maps: &BridgeMaps,
) -> Option<EdgeKey> {
    let src_id = event.src_id?;
    let dst_id = event.dst_id?;
    let label = event.edge_type.clone()?;
    let src_key = maps
        .node_key_map
        .read()
        .get(&grafeo::NodeId::new(src_id))
        .cloned()?;
    let dst_key = maps
        .node_key_map
        .read()
        .get(&grafeo::NodeId::new(dst_id))
        .cloned()?;
    Some((src_key, dst_key, label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_key_roundtrip() {
        let key: EdgeKey = ("a".to_string(), "b".to_string(), "KNOWS".to_string());
        let encoded = encode_edge_key(&key);
        assert_eq!(encoded, "a|b|KNOWS");
        let parsed = parse_edge_key(&encoded).unwrap();
        assert_eq!(parsed, key);
    }

    #[test]
    fn edge_key_parse_rejects_missing_separator() {
        assert!(parse_edge_key("only-one-segment").is_none());
        assert!(parse_edge_key("a|b|").is_none());
    }
}
