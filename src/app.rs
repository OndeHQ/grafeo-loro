use std::sync::Arc;
use crate::config::{SsotMode, CompressionType};
use crate::error::Result;
use crate::storage::StorageBackend;
use crate::types::{NodeId, PresencePayload};

pub struct GrafeoLoroApp {
    // Internal Arc states for Doc, DB, SyncEngine, Batchers
}

pub struct GrafeoLoroAppBuilder {
    storage: Option<Arc<dyn StorageBackend>>,
    ssot_mode: SsotMode,
    compression: CompressionType,
    sync_compression: CompressionType,
    batch_interval_ms: u64,
    batch_max_size: usize,
}

impl GrafeoLoroApp {
    pub fn builder() -> GrafeoLoroAppBuilder;
    
    pub async fn hydrate(&self, graph_id: &str) -> Result<()>;
    pub fn create_vertex(&self) -> VertexBuilder;
    pub fn query(&self, gql: &str) -> Result<grafeo::QueryResult>;
    
    pub async fn update_text(&self, node_id: NodeId, field: &str, text: &str) -> Result<()>;
    pub async fn generate_embedding(&self, node_id: NodeId, field: &str) -> Result<()>;
    
    pub async fn checkpoint(&self, graph_id: &str) -> Result<()>;
    pub async fn broadcast_presence(&self, payload: PresencePayload) -> Result<()>;
    pub async fn shutdown(self) -> Result<()>;
}

impl GrafeoLoroAppBuilder {
    pub fn storage(mut self, storage: Arc<dyn StorageBackend>) -> Self;
    pub fn ssot_mode(mut self, mode: SsotMode) -> Self;
    pub fn compression(mut self, comp: CompressionType) -> Self;
    pub fn sync_compression(mut self, comp: CompressionType) -> Self;
    pub fn batch_interval_ms(mut self, ms: u64) -> Self;
    pub fn batch_max_size(mut self, size: usize) -> Self;
    
    pub async fn build(self) -> Result<GrafeoLoroApp>;
}

pub struct VertexBuilder {
    // Fluent API state
}

impl VertexBuilder {
    pub fn with_label(self, label: &str) -> Self;
    pub fn with_property(self, key: &str, value: impl Into<crate::types::GraphValue>) -> Self;
    pub fn commit(self) -> Result<NodeId>;
}