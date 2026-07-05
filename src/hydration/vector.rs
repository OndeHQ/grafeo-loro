use std::sync::Arc;
use grafeo::GrafeoDB;
use crate::types::ids::NodeId;
use crate::error::Result;

/// Manages offloaded float-vector embeddings. Vectors are never written to
/// Loro; they go direct to Grafeo's HNSW index.
pub struct VectorOffloadManager {
    db: Arc<GrafeoDB>,
}

impl VectorOffloadManager {
    /// Construct with a shared Grafeo handle.
    pub fn new(db: Arc<GrafeoDB>) -> Self {
        let _ = db;
        unimplemented!()
    }

    /// Detects text update, generates embedding, writes direct to Grafeo.
    pub async fn handle_text_update(&self, node_id: NodeId, text: &str) -> Result<()> {
        let _ = (node_id, text);
        unimplemented!()
    }
}

/// Local ONNX inference pipeline stub.
async fn generate_local_embedding(text: &str) -> Vec<f32> {
    let _ = text;
    unimplemented!()
}
