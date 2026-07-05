//! Bidirectional Loro↔Grafeo sync engine.
//!
//! Owns the MPSC channels, the Loro subscriber, and the two async worker loops
//! (inbound = Loro→Grafeo, outbound = Grafeo→Loro). All bodies are
//! `unimplemented!()` at L1; L2/L3 fill in the wiring.

use std::sync::Arc;

use grafeo::GrafeoDB;
use loro::LoroDoc;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::error::Result;
use crate::types::events::{CdcEventWrapper, LoroOp};

/// Inbound channel payload: a Loro subscriber event translated to a graph op.
///
/// Variant kept minimal at L1; L2/L3 may add `RawDiff` for batched translation.
pub enum InboundMsg {
    /// A single translated graph mutation op destined for Grafeo.
    Op(LoroOp),
}

/// Outbound channel payload: a Grafeo CDC event destined for Loro.
pub enum OutboundMsg {
    /// Wrapped CDC event with origin metadata for echo filtering.
    Cdc(CdcEventWrapper),
}

/// Closure used by [`SyncEngine::init_loro_subscriber`] to filter Loro
/// `DiffEvent`s by origin before they are translated and pushed to the inbound
/// channel. Returns `true` to keep, `false` to drop (echo).
pub type LoroSubscriberFilter =
    Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Bidirectional sync engine. Holds shared handles to both stores, the two
/// MPSC channels, and a single cancellation token used to gracefully drain
/// both worker loops on shutdown.
pub struct SyncEngine {
    /// Grafeo execution-layer handle (internally thread-safe).
    pub(crate) grafeo_db: Arc<GrafeoDB>,
    /// Loro consensus-layer handle (mutations need a write lock).
    pub(crate) loro_doc: Arc<RwLock<LoroDoc>>,
    /// Inbound channel: Loro subscriber → inbound worker.
    pub(crate) inbound_tx: mpsc::Sender<InboundMsg>,
    /// Inbound channel receiver, owned by [`spawn_inbound_worker`].
    pub(crate) inbound_rx: tokio::sync::Mutex<mpsc::Receiver<InboundMsg>>,
    /// Outbound channel: Grafeo CDC source → outbound worker.
    pub(crate) outbound_tx: mpsc::Sender<OutboundMsg>,
    /// Outbound channel receiver, owned by [`spawn_outbound_worker`].
    pub(crate) outbound_rx: tokio::sync::Mutex<mpsc::Receiver<OutboundMsg>>,
    /// Cancellation token shared with both worker loops.
    pub(crate) shutdown: CancellationToken,
}

impl SyncEngine {
    /// Construct a new engine with fresh channels and a fresh shutdown token.
    /// Does NOT spawn workers or subscribe — call [`init_loro_subscriber`]
    /// and the `spawn_*_worker` methods explicitly.
    pub fn new(grafeo_db: Arc<GrafeoDB>, loro_doc: Arc<RwLock<LoroDoc>>) -> Self {
        let _ = (grafeo_db, loro_doc);
        unimplemented!()
    }

    /// Wire `loro_doc.subscribe_root` → origin filter → translate to `LoroOp`
    /// → `inbound_tx.send(InboundMsg::Op(...))`. Holds a strong ref to the
    /// returned [`loro::Subscription`] for the engine's lifetime.
    pub fn init_loro_subscriber(&self) -> Result<()> {
        unimplemented!()
    }

    /// Inbound worker: drain `inbound_rx`, batch ops, commit a single Grafeo
    /// transaction tagged with `ORIGIN_LORO_BRIDGE` per batch.
    pub async fn spawn_inbound_worker(self: Arc<Self>) -> JoinHandle<()> {
        unimplemented!()
    }

    /// Outbound worker: drain `outbound_rx`, filter echo by origin, transact
    /// Loro with `set_next_commit_origin(ORIGIN_GRAFEO_BRIDGE)`.
    pub async fn spawn_outbound_worker(self: Arc<Self>) -> JoinHandle<()> {
        unimplemented!()
    }

    /// Expose the outbound sender so external CDC sources (or a Grafeo CDC
    /// poller) can push [`OutboundMsg`]s into the engine.
    pub fn outbound_sender(&self) -> mpsc::Sender<OutboundMsg> {
        unimplemented!()
    }

    /// Signal both worker loops to drain and exit.
    pub fn shutdown(&self) {
        unimplemented!()
    }
}
