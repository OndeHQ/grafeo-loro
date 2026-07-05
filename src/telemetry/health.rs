use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use parking_lot::RwLock;
use loro::LoroDoc;
use grafeo::GrafeoDB;

pub struct HealthProbe {
    doc: Arc<RwLock<LoroDoc>>,
    db: Arc<GrafeoDB>,
    last_sync_ts: AtomicU64,
}

pub struct HealthStatus {
    pub overall: bool,
    pub components: Vec<(&'static str, bool)>,
}

impl HealthProbe {
    pub fn new(doc: Arc<RwLock<LoroDoc>>, db: Arc<GrafeoDB>) -> Self;
    pub fn update_sync_ts(&self);
    pub fn check(&self, max_staleness_ms: u64) -> HealthStatus;
}