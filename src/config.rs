#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SsotMode {
    #[default]
    Loro,
    Grafeo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionType {
    None,
    Lz4,
    #[default]
    Zstd,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub ssot_mode: SsotMode,
    pub compression: CompressionType,
    pub sync_compression: CompressionType,
    pub batch_interval_ms: u64,
    pub batch_max_size: usize,
    pub hydration_chunk_size: usize,
    pub max_staleness_ms: u64,
    pub enable_presence: bool,
    pub presence_heartbeat_ms: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        unimplemented!()
    }
}