use std::sync::Arc;
use tokio::sync::mpsc;
use grafeo::GrafeoDB;
use crate::types::events::LoroOp;

pub struct MutationBatcher {
    db: Arc<GrafeoDB>,
}

impl MutationBatcher {
    pub fn new(db: Arc<GrafeoDB>) -> Self;
    pub async fn start_batch_loop(
        &self, 
        rx: mpsc::Receiver<LoroOp>, 
        batch_ms: u64, 
        max_batch_size: usize
    );
    fn flush_batch(&self, buffer: &mut Vec<LoroOp>);
}