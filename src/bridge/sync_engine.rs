use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use loro::LoroDoc;
use grafeo::GrafeoDB;
use crate::error::Result;

pub struct SyncEngine {
    db: Arc<GrafeoDB>,
    doc: Arc<RwLock<LoroDoc>>,
    inbound_tx: mpsc::Sender<loro::event::Event>,
}

impl SyncEngine {
    pub fn new(db: Arc<GrafeoDB>, doc: Arc<RwLock<LoroDoc>>) -> Arc<Self>;
    fn init_loro_subscriber(self: &Arc<Self>);
    fn spawn_inbound_worker(self: &Arc<Self>, rx: mpsc::Receiver<loro::event::Event>);
    pub fn spawn_outbound_worker(self: &Arc<Self>, cdc_rx: mpsc::Receiver<grafeo::cdc::CdcEvent>);
}