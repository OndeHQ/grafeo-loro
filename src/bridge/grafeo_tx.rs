//! Loro→Grafeo mutation translator + shared id-mapping state.
//!
//! `BridgeMaps` (Devil MAJOR M7 / orchestrator Gap 3 APPROVED) owns the four
//! lookup tables that translate between Loro-side string keys and grafeo
//! `NodeId`/`EdgeId`s. grafeo 0.5.42 has no upsert-by-external-id, so the
//! inbound path maintains the forward (`loro_key → grafeo id`) and inverse
//! (`grafeo id → loro_key`) maps itself. The outbound CDC poller reads the
//! inverse maps to translate `ChangeEvent.entity_id` back into Loro keys.

use std::collections::HashMap;

use parking_lot::RwLock;

use crate::error::{GrafeoLoroError, Result};
use crate::types::events::LoroOp;
use crate::types::values::gval_to_grafeo_value;

/// Composite key identifying a Loro-side edge: `(src_loro_key, dst_loro_key, label)`.
pub type EdgeKey = (String, String, String);

/// Shared bridge id-mapping state. Forward maps are read+written by the
/// inbound apply path; inverse maps are read by the outbound CDC poller.
/// All four maps are kept in lock-step by the helper methods below.
#[derive(Default)]
pub struct BridgeMaps {
    /// `loro_key → grafeo::NodeId` (inbound lookup-or-create).
    pub node_id_map: RwLock<HashMap<String, grafeo::NodeId>>,
    /// `grafeo::NodeId → loro_key` (outbound reverse lookup).
    pub node_key_map: RwLock<HashMap<grafeo::NodeId, String>>,
    /// `(src_key, dst_key, label) → grafeo::EdgeId` (inbound edge idempotency).
    pub edge_id_map: RwLock<HashMap<EdgeKey, grafeo::EdgeId>>,
    /// `grafeo::EdgeId → (src_key, dst_key, label)` (outbound reverse lookup).
    pub edge_key_map: RwLock<HashMap<grafeo::EdgeId, EdgeKey>>,
}

impl BridgeMaps {
    /// Construct fresh empty maps.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a `loro_key ↔ grafeo::NodeId` binding in both forward and
    /// inverse maps. Overwrites any prior binding for either side.
    pub fn insert_node(&self, loro_key: String, id: grafeo::NodeId) {
        self.node_id_map.write().insert(loro_key.clone(), id);
        self.node_key_map.write().insert(id, loro_key);
    }

    /// Remove a `loro_key ↔ grafeo::NodeId` binding from both maps. Returns
    /// the grafeo id if the key was present (no-op otherwise).
    pub fn remove_node(&self, loro_key: &str) -> Option<grafeo::NodeId> {
        let id = self.node_id_map.write().remove(loro_key)?;
        self.node_key_map.write().remove(&id);
        Some(id)
    }

    /// Record an `EdgeKey ↔ grafeo::EdgeId` binding in both maps.
    pub fn insert_edge(&self, key: EdgeKey, id: grafeo::EdgeId) {
        self.edge_id_map.write().insert(key.clone(), id);
        self.edge_key_map.write().insert(id, key);
    }

    /// Remove an edge binding by `EdgeKey`. Returns the grafeo id if present.
    pub fn remove_edge(&self, key: &EdgeKey) -> Option<grafeo::EdgeId> {
        let id = self.edge_id_map.write().remove(key)?;
        self.edge_key_map.write().remove(&id);
        Some(id)
    }

    /// Remove an edge binding by grafeo `EdgeId`. Returns the Loro-side key
    /// tuple if present (used when translating a CDC `EdgeDelete` event).
    pub fn remove_edge_by_id(&self, id: grafeo::EdgeId) -> Option<EdgeKey> {
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

fn apply_upsert_edge(
    session: &grafeo::Session,
    src_key: &str,
    dst_key: &str,
    label: &str,
    properties: &HashMap<String, crate::types::values::GraphValue>,
    maps: &BridgeMaps,
) -> Result<()> {
    let (src_id, dst_id) = match (maps.node_id_map.read().get(src_key), maps.node_id_map.read().get(dst_key)) {
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
/// Phase 2: tree container support — handler exists because `LoroOp::TreeMove`
/// is part of the L1 contract, but no production caller exists in Phase 1
/// (the inbound subscriber only translates `ROOT_VERTICES`/`ROOT_EDGES`
/// diffs; `ROOT_TREE` was deleted as YAGNI per Hunter NIT 11). The variant
/// and this handler are retained so Phase 2 can wire tree-container diffs
/// without re-shaping the enum. Hunter MINOR 8 flagged this as a dead path.
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
    let old_key: EdgeKey = (node_key.to_string(), old_parent_key.to_string(), "CHILD".to_string());
    if let Some(id) = maps.remove_edge(&old_key) {
        session.delete_edge(id);
    }
    let new_key: EdgeKey = (node_key.to_string(), new_parent_key.to_string(), "CHILD".to_string());
    if maps.edge_id_map.read().get(&new_key).is_none() {
        let eid = session.create_edge(node_id, new_parent_id, "CHILD");
        maps.insert_edge(new_key, eid);
    }
    Ok(())
}
