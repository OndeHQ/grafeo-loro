use std::collections::HashMap;

#[cfg(feature = "grafeo")]
use grafeo_common::types::EpochId;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::GraphValue;

/// Translated Loro subscriber diff destined for the inbound batcher / worker.
///
/// Per orchestrator decision (Devil Gap 3): grafeo 0.5.42 has no upsert-by-
/// external-id, so `UpsertNode`/`DeleteNode` carry a Loro-side string key
/// (`loro_key`). The bridge maintains a `loro_key → grafeo::NodeId` map in
/// `SyncEngine` and translates at apply time.
///
/// `Serialize`/`Deserialize` derives are gated by `serde` so the bincode-only
/// FFI entry point `apply_loro_op_bytes` (issue #1 item 6) can round-trip a
/// `Vec<LoroOp>` through bincode without pulling `serde_json` (ADR-010).
///
/// `PartialEq` is derived so the bincode round-trip unit test can compare
/// `Vec<LoroOp>` structurally (not via `Debug` string — `HashMap` iteration
/// order is non-deterministic, which would make `format!("{x:?}")` flaky).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LoroOp {
    /// Insert or update a vertex identified by its Loro-side string key.
    UpsertNode {
        /// Loro-side stable string key (e.g. `"V/abc-123"`). Mapped to a
        /// grafeo `NodeId` via `SyncEngine::node_id_map`.
        loro_key: String,
        /// Grafeo labels (e.g. `["Person"]`).
        labels: Vec<String>,
        /// Full property map (already `lval_to_gval`-converted).
        properties: HashMap<String, GraphValue>,
    },
    /// Insert or update an edge identified by Loro-side string keys for
    /// both endpoints plus a label.
    UpsertEdge {
        src_key: String,
        dst_key: String,
        label: String,
        properties: HashMap<String, GraphValue>,
    },
    /// Remove a vertex by Loro-side string key.
    DeleteNode { loro_key: String },
    /// Remove an edge by (src, dst, label) Loro-side string keys.
    DeleteEdge {
        src_key: String,
        dst_key: String,
        label: String,
    },
    /// Tree reparenting: delete old `CHILD` edge, insert new one.
    TreeMove {
        node_key: String,
        old_parent_key: String,
        new_parent_key: String,
    },
}

/// Grafeo `ChangeEvent` paired with the MVCC `epoch` it was committed in.
///
/// The epoch is the echo-prevention side-channel (Devil Gap 1 / orchestrator
/// approval): inbound Loro→Grafeo writes record their commit epoch in
/// `SyncEngine::bridge_origin_epochs`; the outbound CDC poller filters any
/// `ChangeEvent` whose `epoch` is in that set.
#[cfg(feature = "grafeo")]
#[derive(Debug, Clone)]
pub struct CdcEventWrapper {
    /// MVCC epoch of the Grafeo transaction that produced this event.
    pub epoch: EpochId,
    /// The underlying Grafeo CDC change event.
    pub payload: grafeo::cdc::ChangeEvent,
}

#[cfg(feature = "grafeo")]
impl CdcEventWrapper {
    /// Construct a wrapper from its epoch and payload (L2 new issue #2 —
    /// ergonomic constructor for the type-alias `OutboundMsg = CdcEventWrapper`).
    pub fn new(epoch: EpochId, payload: grafeo::cdc::ChangeEvent) -> Self {
        Self { epoch, payload }
    }
}
