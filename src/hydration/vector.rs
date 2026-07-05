use std::sync::Arc;
use grafeo::GrafeoDB;
use crate::types::ids::NodeId;
use crate::error::Result;

pub struct VectorOffloadManager {
    db: Arc<GrafeoDB>,
}

impl VectorOffloadManager {
    pub fn new(db: Arc<GrafeoDB>) -> Self;
    
    /// Detects text update, generates embedding, writes direct to Grafeo.
    pub async fn handle_text_update(&self, node_id: NodeId, text: &str) -> Result<()>;
}

/// Local ONNX inference pipeline stub.
async fn generate_local_embedding(text: &str) -> Vec<f32>;