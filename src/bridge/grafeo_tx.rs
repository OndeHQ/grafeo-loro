//! Loro→Grafeo mutation translator + shared id-mapping state.
//!
//! `BridgeMaps` (Devil MAJOR M7 / orchestrator Gap 3 APPROVED) owns the four
//! lookup tables that translate between Loro-side string keys and grafeo
//! `NodeId`/`EdgeId`s. grafeo 0.5.42 has no upsert-by-external-id, so the
//! inbound path maintains the forward (`loro_key → grafeo id`) and inverse
//! (`grafeo id → loro_key`) maps itself. The outbound CDC poller reads the
//! inverse maps to translate `ChangeEvent.entity_id` back into Loro keys.
//!
//! Issue #1 compliance: `BridgeMaps` uses `crate::types::ids::{NodeId, EdgeId}`
//! which are `grafeo::{NodeId, EdgeId}` re-exports when `grafeo` is on, and
//! local `u64` newtypes when `grafeo` is off. This means `BridgeMaps` is
//! available in WASM builds without the grafeo execution layer — Onde can
//! construct one and wire its own runtime against it.
//!
//! # Native text bijection check (issue #3 sub-issue 7, invariant I11)
//!
//! [`validate_text_bijection`] verifies that every `loro_key ↔ NodeId` pair
//! in the bridge maps is bijective — both the forward (`loro_key → NodeId`)
//! and inverse (`NodeId → loro_key`) maps must agree, and no two distinct
//! `loro_key`s may map to the same `NodeId` (and vice versa). This is the
//! native enforcement of invariant I11 the issue body calls out as
//! "Bridge map drift causes silent data loss on text nodes."

use std::collections::HashMap;

use parking_lot::RwLock;
use tracing::instrument;

use crate::constants::TREE_EDGE_LABEL;
use crate::error::{GrafeoLoroError, Result};
use crate::types::events::LoroOp;
use crate::types::ids::{EdgeId, NodeId};
#[cfg(feature = "grafeo")]
use crate::types::values::gval_to_grafeo_value;

/// Composite key identifying a Loro-side edge: `(src_loro_key, dst_loro_key, label)`.
pub type EdgeKey = (String, String, String);

/// Shared bridge id-mapping state. Forward maps are read+written by the
/// inbound apply path; inverse maps are read by the outbound CDC poller.
/// All four maps are kept in lock-step by the helper methods below.
#[derive(Default)]
pub struct BridgeMaps {
    /// `loro_key → grafeo::NodeId` (inbound lookup-or-create).
    pub node_id_map: RwLock<HashMap<String, NodeId>>,
    /// `grafeo::NodeId → loro_key` (outbound reverse lookup).
    pub node_key_map: RwLock<HashMap<NodeId, String>>,
    /// `(src_key, dst_key, label) → grafeo::EdgeId` (inbound edge idempotency).
    pub edge_id_map: RwLock<HashMap<EdgeKey, EdgeId>>,
    /// `grafeo::EdgeId → (src_key, dst_key, label)` (outbound reverse lookup).
    pub edge_key_map: RwLock<HashMap<EdgeId, EdgeKey>>,
}

impl BridgeMaps {
    /// Construct fresh empty maps.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a `loro_key ↔ grafeo::NodeId` binding in both forward and
    /// inverse maps. Overwrites any prior binding for either side.
    #[instrument(skip(self), name = "bridge_insert_node", level = "trace")]
    pub fn insert_node(&self, loro_key: String, id: NodeId) {
        self.node_id_map.write().insert(loro_key.clone(), id);
        self.node_key_map.write().insert(id, loro_key);
    }

    /// Remove a `loro_key ↔ grafeo::NodeId` binding from both maps. Returns
    /// the grafeo id if the key was present (no-op otherwise).
    #[instrument(skip(self), name = "bridge_remove_node", level = "trace")]
    pub fn remove_node(&self, loro_key: &str) -> Option<NodeId> {
        let id = self.node_id_map.write().remove(loro_key)?;
        self.node_key_map.write().remove(&id);
        Some(id)
    }

    /// Record an `EdgeKey ↔ grafeo::EdgeId` binding in both maps.
    #[instrument(skip(self), name = "bridge_insert_edge", level = "trace")]
    pub fn insert_edge(&self, key: EdgeKey, id: EdgeId) {
        self.edge_id_map.write().insert(key.clone(), id);
        self.edge_key_map.write().insert(id, key);
    }

    /// Remove an edge binding by `EdgeKey`. Returns the grafeo id if present.
    #[instrument(skip(self), name = "bridge_remove_edge", level = "trace")]
    pub fn remove_edge(&self, key: &EdgeKey) -> Option<EdgeId> {
        let id = self.edge_id_map.write().remove(key)?;
        self.edge_key_map.write().remove(&id);
        Some(id)
    }

    /// Remove an edge binding by grafeo `EdgeId`. Returns the Loro-side key
    /// tuple if present (used when translating a CDC `EdgeDelete` event).
    #[instrument(skip(self), name = "bridge_remove_edge_by_id", level = "trace")]
    pub fn remove_edge_by_id(&self, id: EdgeId) -> Option<EdgeKey> {
        let key = self.edge_key_map.write().remove(&id)?;
        self.edge_id_map.write().remove(&key);
        Some(key)
    }
}

/// Apply a single `LoroOp` to a grafeo `Session` (already inside a
/// `begin_transaction()` / `prepare_commit()` pair). Looks up `loro_key` in
/// the forward maps; on hit, mutates the existing entity; on miss, creates a
/// new entity and inserts both forward and inverse bindings. Delete ops are
/// idempotent (missing key = no-op). `TreeMove` is implemented as
/// delete-old-CHILD-edge + insert-new-CHILD-edge per L3 mandate.
///
/// Issue #1: requires `grafeo` feature (calls `Session::create_node_with_props`).
#[cfg(feature = "grafeo")]
#[instrument(skip(session, op, maps), name = "apply_loro_op", level = "info")]
pub fn apply_loro_op(session: &grafeo::Session, op: &LoroOp, maps: &BridgeMaps) -> Result<()> {
    match op {
        LoroOp::UpsertNode {
            loro_key,
            labels,
            properties,
        } => apply_upsert_node(session, loro_key, labels, properties, maps),
        LoroOp::UpsertEdge {
            src_key,
            dst_key,
            label,
            properties,
        } => apply_upsert_edge(session, src_key, dst_key, label, properties, maps),
        LoroOp::DeleteNode { loro_key } => {
            if let Some(id) = maps.remove_node(loro_key) {
                session.delete_node(id);
            }
            Ok(())
        }
        LoroOp::DeleteEdge {
            src_key,
            dst_key,
            label,
        } => {
            let key = (src_key.clone(), dst_key.clone(), label.clone());
            if let Some(id) = maps.remove_edge(&key) {
                session.delete_edge(id);
            }
            Ok(())
        }
        LoroOp::TreeMove {
            node_key,
            old_parent_key,
            new_parent_key,
        } => apply_tree_move(session, node_key, old_parent_key, new_parent_key, maps),
    }
}

#[cfg(feature = "grafeo")]
fn apply_upsert_node(
    session: &grafeo::Session,
    loro_key: &str,
    labels: &[String],
    properties: &HashMap<String, crate::types::values::GraphValue>,
    maps: &BridgeMaps,
) -> Result<()> {
    if let Some(&id) = maps.node_id_map.read().get(loro_key) {
        for (k, v) in properties {
            session.set_node_property(id, k.as_str(), gval_to_grafeo_value(v.clone()))?;
        }
        return Ok(());
    }
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let props_iter = properties
        .iter()
        .map(|(k, v)| (k.as_str(), gval_to_grafeo_value(v.clone())));
    let id = session.create_node_with_props(&label_refs, props_iter)?;
    maps.insert_node(loro_key.to_string(), id);
    Ok(())
}

#[cfg(feature = "grafeo")]
fn apply_upsert_edge(
    session: &grafeo::Session,
    src_key: &str,
    dst_key: &str,
    label: &str,
    properties: &HashMap<String, crate::types::values::GraphValue>,
    maps: &BridgeMaps,
) -> Result<()> {
    let (src_id, dst_id) = match (
        maps.node_id_map.read().get(src_key),
        maps.node_id_map.read().get(dst_key),
    ) {
        (Some(&s), Some(&d)) => (s, d),
        _ => {
            return Err(GrafeoLoroError::Bridge(format!(
                "unknown node key(s): src={src_key:?} dst={dst_key:?}"
            )));
        }
    };
    let key: EdgeKey = (src_key.to_string(), dst_key.to_string(), label.to_string());
    if let Some(&eid) = maps.edge_id_map.read().get(&key) {
        for (k, v) in properties {
            session.set_edge_property(eid, k.as_str(), gval_to_grafeo_value(v.clone()))?;
        }
        return Ok(());
    }
    let props_iter = properties
        .iter()
        .map(|(k, v)| (k.as_str(), gval_to_grafeo_value(v.clone())));
    let eid = session.create_edge_with_props(src_id, dst_id, label, props_iter)?;
    maps.insert_edge(key, eid);
    Ok(())
}

/// Phase 1 tree move = delete old `CHILD` edge + insert new `CHILD` edge.
/// `old_parent_key`/`new_parent_key` may map to the same node (idempotent).
///
/// Edge direction is parent→child (src=parent, dst=child) per architecture
/// §7 line 265 (`(p)-[:CHILD]->(c)`) — P2T2-DEVIL R1; the pre-existing
/// child→parent direction was a Phase 1 bug, fixed in P2T2-L2.
#[cfg(feature = "grafeo")]
fn apply_tree_move(
    session: &grafeo::Session,
    node_key: &str,
    old_parent_key: &str,
    new_parent_key: &str,
    maps: &BridgeMaps,
) -> Result<()> {
    let node_id = match maps.node_id_map.read().get(node_key) {
        Some(&n) => n,
        None => return Ok(()),
    };
    let new_parent_id = match maps.node_id_map.read().get(new_parent_key) {
        Some(&n) => n,
        None => return Ok(()),
    };
    // parent→child: EdgeKey = (parent, child, label) — P2T2-DEVIL R1.
    let old_key: EdgeKey = (
        old_parent_key.to_string(),
        node_key.to_string(),
        TREE_EDGE_LABEL.to_string(),
    );
    if let Some(id) = maps.remove_edge(&old_key) {
        session.delete_edge(id);
    }
    let new_key: EdgeKey = (
        new_parent_key.to_string(),
        node_key.to_string(),
        TREE_EDGE_LABEL.to_string(),
    );
    if maps.edge_id_map.read().get(&new_key).is_none() {
        // parent→child: create_edge(parent, child, label) — P2T2-DEVIL R1.
        let eid = session.create_edge(new_parent_id, node_id, TREE_EDGE_LABEL);
        maps.insert_edge(new_key, eid);
    }
    Ok(())
}

// ============================================================================
// Issue #3 sub-issue 7, invariant I11: bijective bridge-map consistency
// ============================================================================
//
// The issue body calls out: "Bridge map drift causes silent data loss on
// text nodes." `validate_text_bijection` is the native enforcement: it
// walks both the forward (`loro_key → NodeId`) and inverse (`NodeId →
// loro_key`) maps and verifies they agree (no missing inverse, no missing
// forward, no two keys mapping to the same id, no two ids mapping to the
// same key).
//
// The check is pure-Rust + `bridge` feature only — no grafeo dep. The
// orchestrator wires it into `MutationBatcher::flush_inner`'s post-commit
// invariant-check hook (issue #3 sub-issue 10 territory) + the
// `observability` module's I11 assertion API.

/// Error returned by [`validate_text_bijection`] when the bridge maps
/// violate invariant I11 (bijective `loro_key ↔ NodeId` consistency).
///
/// Each variant carries enough context for the orchestrator to log a
/// structured alert + surface a repair hint to FFI (e.g. "delete the
/// orphaned loro_key X" or "re-bind NodeId Y to its last-known key").
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BijectionError {
    /// Forward map has `loro_key → id` but the inverse map lacks the
    /// corresponding `id → loro_key` entry. Bridge drift: an insert_node
    /// was interrupted between the forward write and the inverse write.
    #[error("bijection drift: forward map has loro_key {loro_key:?} → id {id:?} but inverse map lacks the entry")]
    MissingInverse {
        /// The orphaned forward-mapping key.
        loro_key: String,
        /// The id that should have an inverse entry but doesn't.
        id: crate::types::ids::NodeId,
    },
    /// Inverse map has `id → loro_key` but the forward map lacks the
    /// corresponding `loro_key → id` entry. Mirror of [`Self::MissingInverse`].
    #[error("bijection drift: inverse map has id {id:?} → loro_key {loro_key:?} but forward map lacks the entry")]
    MissingForward {
        /// The orphaned inverse-mapping key.
        loro_key: String,
        /// The id that has an inverse entry but no forward entry.
        id: crate::types::ids::NodeId,
    },
    /// Two distinct `loro_key`s map to the same `NodeId`. This silently
    /// loses the second insert_node's binding (the second `insert_node`
    /// overwrites the inverse map entry, but the forward map still has
    /// both keys pointing at the same id).
    #[error("bijection drift: two loro_keys map to same id {id:?} — keys: {key_a:?}, {key_b:?}")]
    DuplicateId {
        /// The id that is the target of two distinct key mappings.
        id: crate::types::ids::NodeId,
        /// First key.
        key_a: String,
        /// Second key (the one that overwrote key_a's inverse entry).
        key_b: String,
    },
    /// Two distinct `NodeId`s map to the same `loro_key`. Mirror of
    /// [`Self::DuplicateId`].
    #[error(
        "bijection drift: two ids map to same loro_key {loro_key:?} — ids: {id_a:?}, {id_b:?}"
    )]
    DuplicateKey {
        /// The key that is the target of two distinct id mappings.
        loro_key: String,
        /// First id.
        id_a: crate::types::ids::NodeId,
        /// Second id (the one that overwrote id_a's forward entry).
        id_b: crate::types::ids::NodeId,
    },
}

/// Verify bijective `loro_key ↔ NodeId` consistency on text nodes
/// (issue #3 sub-issue 7, invariant I11).
///
/// Walks both the forward (`node_id_map`) and inverse (`node_key_map`)
/// maps in [`BridgeMaps`] and verifies:
/// 1. Every forward entry has a corresponding inverse entry pointing back.
/// 2. Every inverse entry has a corresponding forward entry pointing back.
/// 3. No two distinct `loro_key`s map to the same `NodeId`.
/// 4. No two distinct `NodeId`s map to the same `loro_key`.
///
/// Returns `Ok(())` if all four invariants hold; `Err(BijectionError)` on
/// the first violation encountered.
///
/// # Cost
///
/// O(N) where N = max(node_id_map.len(), node_key_map.len()). Acquires
/// read locks on both maps. Suitable for periodic invariant checks (e.g.
/// post-flush) — NOT for hot-path per-op validation.
///
/// # Wire-up
///
/// The orchestrator wires this into:
/// - `MutationBatcher::flush_inner`'s post-commit invariant-check hook
///   (issue #3 sub-issue 10 territory).
/// - The `observability` module's I11 assertion API (issue #3 sub-issue 10).
/// - The FFI surface as `grafeo_loro_check_text_bijection(maps)` for
///   downstream debug builds.
pub fn validate_text_bijection(maps: &BridgeMaps) -> std::result::Result<(), BijectionError> {
    let forward = maps.node_id_map.read();
    let inverse = maps.node_key_map.read();

    // Check 1: every forward entry has a corresponding inverse entry.
    for (loro_key, id) in forward.iter() {
        match inverse.get(id) {
            Some(inv_key) if inv_key == loro_key => {
                // OK — bijective pair.
            }
            Some(_other_key) => {
                // Inverse points elsewhere — duplicate id (two keys → same id).
                return Err(BijectionError::DuplicateId {
                    id: *id,
                    key_a: loro_key.clone(),
                    key_b: _other_key.clone(),
                });
            }
            None => {
                return Err(BijectionError::MissingInverse {
                    loro_key: loro_key.clone(),
                    id: *id,
                });
            }
        }
    }

    // Check 2: every inverse entry has a corresponding forward entry.
    // (Duplicates among inverse values would also surface as DuplicateId
    // above when we encounter the second forward key, but a missing
    // forward entry is its own error.)
    for (id, loro_key) in inverse.iter() {
        match forward.get(loro_key) {
            Some(fwd_id) if fwd_id == id => {
                // OK — bijective pair.
            }
            Some(_other_id) => {
                // Forward points elsewhere — duplicate key (two ids → same key).
                return Err(BijectionError::DuplicateKey {
                    loro_key: loro_key.clone(),
                    id_a: *id,
                    id_b: *_other_id,
                });
            }
            None => {
                return Err(BijectionError::MissingForward {
                    loro_key: loro_key.clone(),
                    id: *id,
                });
            }
        }
    }

    Ok(())
}

#[cfg(all(test, feature = "grafeo"))]
mod bijection_tests {
    use super::*;
    use crate::types::ids::NodeId;

    #[test]
    fn empty_maps_pass() {
        let maps = BridgeMaps::new();
        assert!(validate_text_bijection(&maps).is_ok());
    }

    #[test]
    fn single_pair_passes() {
        let maps = BridgeMaps::new();
        maps.insert_node("k1".to_string(), NodeId::new(1));
        assert!(validate_text_bijection(&maps).is_ok());
    }

    #[test]
    fn missing_inverse_detected() {
        let maps = BridgeMaps::new();
        // Inject forward entry without the inverse by going through the
        // public API then surgically removing the inverse entry.
        maps.insert_node("k1".to_string(), NodeId::new(1));
        maps.node_key_map.write().remove(&NodeId::new(1));
        match validate_text_bijection(&maps) {
            Err(BijectionError::MissingInverse { loro_key, id }) => {
                assert_eq!(loro_key, "k1");
                assert_eq!(id, NodeId::new(1));
            }
            other => panic!("expected MissingInverse, got {other:?}"),
        }
    }

    #[test]
    fn missing_forward_detected() {
        let maps = BridgeMaps::new();
        maps.insert_node("k1".to_string(), NodeId::new(1));
        maps.node_id_map.write().remove("k1");
        match validate_text_bijection(&maps) {
            Err(BijectionError::MissingForward { loro_key, id }) => {
                assert_eq!(loro_key, "k1");
                assert_eq!(id, NodeId::new(1));
            }
            other => panic!("expected MissingForward, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_id_detected() {
        let maps = BridgeMaps::new();
        // First insert: k1 → id 1. Insert API keeps both forward and inverse.
        maps.insert_node("k1".to_string(), NodeId::new(1));
        // Second insert: k2 → id 1. insert_node overwrites the inverse
        // (id 1 → k2), but the forward still has BOTH k1 and k2 pointing
        // at id 1 — a DuplicateId violation.
        maps.insert_node("k2".to_string(), NodeId::new(1));
        match validate_text_bijection(&maps) {
            Err(BijectionError::DuplicateId { id, .. }) => {
                assert_eq!(id, NodeId::new(1));
            }
            other => panic!("expected DuplicateId, got {other:?}"),
        }
    }
}
