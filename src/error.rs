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

    /// Tree reparenting would create a cycle. Grafeo 0.5.42 has NO native
    /// graph-edge acyclicity enforcement (verified P2T2-L1: only
    /// `catalog::resolved_node_type` cycle-checks schema type inheritance;
    /// `procedures::has_negative_cycle` is a Bellman-Ford query procedure —
    /// neither constrains user edges at commit time), so the bridge must
    /// pre-check via `schema::tree::would_create_cycle` and reject with this
    /// variant before opening the write transaction.
    #[error("Tree move cycle: node {node_id:?} cannot be reparented under {new_parent:?}")]
    TreeMoveCreatesCycle {
        node_id: crate::types::ids::NodeId,
        new_parent: crate::types::ids::NodeId,
    },
}

pub type Result<T> = std::result::Result<T, GrafeoLoroError>;
