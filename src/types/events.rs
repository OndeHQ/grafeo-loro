use std::collections::HashMap;

#[cfg(feature = "grafeo")]
use grafeo_common::types::EpochId;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::GraphValue;

// ============================================================================
// Issue #3 sub-issue 2 — bincode 1.x enum contract
// ============================================================================
//
// `LoroOp` + `GraphValue` MUST remain externally-tagged (serde default) —
// do NOT add `#[serde(tag = "type", content = "payload")]` (or any other
// internally/adjacently-tagged attribute) to either enum. Bincode 1.x
// panics on internally-tagged enums with
// `Bincode does not support Deserializer::deserialize_identifier` (issue
// #3 sub-issue 2 root cause). The `apply_loro_op_bytes` FFI entry point
// and the new `batcher_enqueue` FFI entry point both bincode-encode
// `LoroOp`; internally-tagged attributes would break both.
//
// `LoroProperty` uses `#[serde(untagged)]`, which IS bincode-safe (untagged
// delegates to the variant's own `Deserialize` impl — no identifier lookup).
// Do NOT change `LoroProperty` to internally-tagged either.

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

// ============================================================================
// Issue #3 sub-issue 4 — semantic merge conflict events
// ============================================================================
//
// `ConflictDetected` replaces silent Last-Writer-Wins on text divergence.
// The bridge emits this event to FFI when text merge detects conflicting
// edits on the same field; downstream JS no longer needs to implement
// manual diff3 per node (issue #3 sub-issue 4 root cause).

/// Emitted when text divergence is detected during merge (issue #3
/// sub-issue 4).
///
/// Replaces silent LWW — downstream no longer needs JS diff3. The bridge
/// layer calls `crate::ffi::semantic_text_merge` on every conflicting
/// triple; if the result is `ConflictResolution::ManualRequired`, the
/// bridge constructs a `ConflictDetected` and dispatches it to all
/// callbacks registered via `crate::ffi::on_conflict_detected`.
///
/// ## Field semantics
///
/// - `node_key`: Loro-side stable string key (e.g. `"V/abc-123"`) of the
///   node whose text field diverged.
/// - `field`: property name on `node_key` whose text value diverged
///   (e.g. `"body"`, `"title"`).
/// - `base`: the common-ancestor text (from the last shared snapshot).
/// - `ours`: the local peer's version of the field at merge time.
/// - `theirs`: the remote peer's version of the field at merge time.
/// - `resolution`: outcome of `crate::ffi::semantic_text_merge` on the
///   triple. `ManualRequired` means the merge produced conflict markers
///   (`<<<<<<<` / `=======` / `>>>>>>>`) inside `ours` + `theirs` —
///   downstream MUST surface a UI for the user to resolve.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ConflictDetected {
    /// Loro-side stable string key of the diverging node.
    pub node_key: String,
    /// Property name on `node_key` whose text value diverged.
    pub field: String,
    /// Common-ancestor text (last shared snapshot value).
    pub base: String,
    /// Local peer's version of the field at merge time.
    pub ours: String,
    /// Remote peer's version of the field at merge time.
    pub theirs: String,
    /// Outcome of `semantic_text_merge` on the triple.
    pub resolution: ConflictResolution,
}

/// Outcome of a 3-way text merge (issue #3 sub-issue 4).
///
/// `Copy` + `Eq` so it can be stored in atomic flags / lock-free structures
/// for batch conflict accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ConflictResolution {
    /// Merge took `ours` verbatim (theirs == base; no remote change).
    OursWins,
    /// Merge took `theirs` verbatim (ours == base; no local change).
    TheirsWins,
    /// Merge combined non-overlapping changes from both sides. No conflict
    /// markers in the output.
    Merged,
    /// Both sides modified the same line(s) differently. Output contains
    /// conflict markers (`<<<<<<<` / `=======` / `>>>>>>>`); downstream
    /// MUST surface a UI for the user to resolve.
    ManualRequired,
}
