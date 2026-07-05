//! Loro→Grafeo mutation translator. Pure-function-shaped per-variant
//! dispatcher from `LoroOp` to grafeo `Session` mutation calls.
//!
//! This module exists (Devil MAJOR M7 / orchestrator Gap 3 APPROVED) so the
//! batcher's flush path can call a single `apply_loro_op` entry point that
//! owns the `loro_key → grafeo::NodeId` lookup-or-create dance. Per-variant
//! bodies are `// TODO L3`.

use std::collections::HashMap;

use parking_lot::RwLock;

use crate::error::Result;
use crate::types::events::LoroOp;

/// Apply a single `LoroOp` to a grafeo `Session` (already inside a
/// `begin_transaction()` / `prepare_commit()` pair). Looks up `loro_key` in
/// `node_id_map`; on hit, mutates the existing node; on miss, creates a new
/// node and inserts the mapping. Edges and tree moves follow the same
/// lookup-or-create pattern against `src_key` / `dst_key` / `node_key`.
///
/// # Arguments
///
/// - `session` — a `grafeo::Session` with an active transaction. Mutation
///   methods on `Session` (`create_node_with_props`, `set_node_property`,
///   `delete_node`, ...) take `&self`, so passing `&Session` is sufficient.
/// - `op` — the translated Loro mutation.
/// - `node_id_map` — shared `loro_key → grafeo::NodeId` mapping. Updated
///   in-place when a new node is created.
pub fn apply_loro_op(
    session: &grafeo::Session,
    op: &LoroOp,
    node_id_map: &RwLock<HashMap<String, grafeo::NodeId>>,
) -> Result<()> {
    let _ = (session, op, node_id_map);
    match op {
        LoroOp::UpsertNode {
            loro_key,
            labels,
            properties,
        } => {
            // TODO L3:
            //   let map_guard = node_id_map.read();
            //   if let Some(&id) = map_guard.get(loro_key) {
            //       // Existing node — set each property.
            //       for (k, v) in properties {
            //           session.set_node_property(id, k, graph_value_to_grafeo_value(v))?;
            //       }
            //   } else {
            //       drop(map_guard);
            //       // New node — create with labels + props, insert into map.
            //       let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            //       let props_iter = properties.iter().map(|(k, v)| (k.as_str(), graph_value_to_grafeo_value(v)));
            //       let id = session.create_node_with_props(&label_refs, props_iter)?;
            //       node_id_map.write().insert(loro_key.clone(), id);
            //   }
            let _ = (loro_key, labels, properties);
            Ok(())
        }
        LoroOp::UpsertEdge {
            src_key,
            dst_key,
            label,
            properties,
        } => {
            // TODO L3: look up src + dst NodeIds via node_id_map (both must
            // exist — if either is missing, log + skip or error per L3
            // policy), then `session.create_edge_with_props(src, dst, label,
            // props)`. Edge `loro_key` mapping is not yet maintained; L3
            // may extend the engine to keep a separate edge map.
            let _ = (src_key, dst_key, label, properties);
            Ok(())
        }
        LoroOp::DeleteNode { loro_key } => {
            // TODO L3:
            //   let mut map_guard = node_id_map.write();
            //   if let Some(id) = map_guard.remove(loro_key) {
            //       session.delete_node(id);
            //   }
            let _ = loro_key;
            Ok(())
        }
        LoroOp::DeleteEdge {
            src_key,
            dst_key,
            label,
        } => {
            // TODO L3: look up edge by (src, dst, label) — requires either
            // a maintained edge map or a query. L3 decides.
            let _ = (src_key, dst_key, label);
            Ok(())
        }
        LoroOp::TreeMove {
            node_key,
            old_parent_key,
            new_parent_key,
        } => {
            // TODO L3: translate to delete-old-CHILD-edge +
            //   insert-new-CHILD-edge. See `schema::tree::sync_tree_move_to_grafeo`
            //   for the legacy entry point (L3 may consolidate).
            let _ = (node_key, old_parent_key, new_parent_key);
            Ok(())
        }
    }
}
