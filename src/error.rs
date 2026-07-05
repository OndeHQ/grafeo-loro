use thiserror::Error;

#[derive(Error, Debug)]
pub enum GrafeoLoroError {
    #[error("Loro CRDT error: {0}")]
    Loro(#[from] loro::LoroError),

    #[error("Grafeo DB error: {0}")]
    Grafeo(#[from] grafeo::Error),

    #[error("Storage backend I/O error: {0}")]
    StorageIo(#[from] std::io::Error),

    #[error("Compression codec failure: {0}")]
    Compression(String),

    #[error("Channel closed: {0}")]
    ChannelClosed(String),

    #[error("Configuration invalid: {0}")]
    Config(String),

    /// LoroValue variant has no GraphValue mapping (e.g. Binary, Container).
    #[error("Unsupported LoroValue type: {0}")]
    UnsupportedLoroType(String),

    /// Runtime bridge error: unknown id-mapping keys, flush timeouts, blocking
    /// pool panics — anything that surfaces from the live bridge machinery
    /// (Hunter NIT 12: previously misrouted to `Config`).
    #[error("Bridge error: {0}")]
    Bridge(String),
}

pub type Result<T> = std::result::Result<T, GrafeoLoroError>;
