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

    /// Cold-boot hydration failure surfaced by `VertexEntity::hydrate_map`
    /// (lorosurgeon). Structured `HydrateError` is preserved (P3T2-L2R2 M2 —
    /// replaces the prior `Bridge(format!(...))` band-aid at the call site).
    #[error("Hydrate error: {0}")]
    Hydrate(#[from] lorosurgeon::error::HydrateError),

    /// Tree reparenting would create a cycle. Grafeo 0.5.42 has NO native
    /// graph-edge acyclicity enforcement (verified P2T2-L1: only
    /// `catalog::resolved_node_type` cycle-checks schema type inheritance;
    /// `procedures::has_negative_cycle` is a Bellman-Ford query procedure —
    /// neither constrains user edges at commit time), so the bridge must
    /// pre-check via `schema::tree::would_create_cycle_in_tx` (run INSIDE the
    /// Serializable tx, before edge mutations) and reject with this variant.
    #[error("Tree move cycle: node {node_id:?} cannot be reparented under {new_parent:?}")]
    TreeMoveCreatesCycle {
        node_id: crate::types::ids::NodeId,
        new_parent: crate::types::ids::NodeId,
    },

    /// Feature/method is planned for a future phase but not yet implemented.
    /// Returned instead of panicking via `unimplemented!()` (Phase 6 T1).
    #[error("not yet implemented: {0}")]
    NotYetImplemented(String),

    /// Malformed `%EPH` presence envelope (bad magic, truncated, serde failure).
    #[error("invalid presence envelope: {0}")]
    InvalidEnvelope(String),
}

pub type Result<T> = std::result::Result<T, GrafeoLoroError>;
