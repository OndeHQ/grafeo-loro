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

// `Default` is intentionally NOT implemented for `AppConfig` — callers must
// construct it explicitly via `GrafeoLoroAppBuilder` (anti-plenger #11:
// deletion over addition; panicking in `Default::default()` is a footgun).
