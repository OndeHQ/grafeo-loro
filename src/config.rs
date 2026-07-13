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

// `AppConfig` was removed — `GrafeoLoroAppBuilder` is the sole construction
// path. Anti-plenger #11 (deletion over addition): the plain-data struct had
// zero callers (verified via `rg 'AppConfig' src/ tests/ fuzz/`).
