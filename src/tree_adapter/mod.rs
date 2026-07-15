//! Tree-as-graph adapter (issue #1 item 8).
//!
//! Turns `:CHILD` edges into ergonomic tree operations. All ops emit
//! `LoroOp::TreeMove` through the bridge — no direct graph API bypass.
//!
//! # Equivalence with old Onde
//!
//! `node.parent()` in old Onde ≡ `tree::parent(node)` in new Onde.
//!
//! `node.children()` ≡ `tree::children(node)`.
//! `node.descendants()` ≡ `tree::descendants(node)`.
//! `node.ancestors()` ≡ `tree::ancestors(node)`.
//! `node.move_to(new_parent)` ≡ `tree::move_op(node, old_parent, new_parent)`.
//! `node.indent()` ≡ `tree::indent_op(node, parent, previous_sibling)`.
//! `node.outdent()` ≡ `tree::outdent_op(node, parent, grandparent)`.
//!
//! # Design
//!
//! The adapter is a thin view over a `BridgeMaps` reference. It performs no
//! mutation on its own — every state-changing helper returns a `LoroOp` for
//! the caller to feed into the bridge (`apply_loro_op` / the inbound batcher).
//! This keeps the tree module decoupled from grafeo internals: it works in
//! pure-WASM builds (where `grafeo` is off and `BridgeMaps` falls back to the
//! local `u64` newtype ids).
//!
//! Edge direction is parent→child (src=parent, dst=child) per architecture
//! §7 line 265 (`(p)-[:CHILD]->(c)`) — `BridgeMaps::edge_id_map` keys are
//! `(src_key, dst_key, label)` tuples, so "parent of N" = the EdgeKey whose
//! `dst_key == N` and `label == "CHILD"`, and "children of N" = the EdgeKeys
//! whose `src_key == N` and `label == "CHILD"`.

use std::collections::{HashMap, HashSet};

use crate::bridge::BridgeMaps;
use crate::constants::TREE_EDGE_LABEL;
use crate::error::{GrafeoLoroError, Result};
use crate::types::events::LoroOp;
use crate::types::ids::NodeId;

/// Error returned when a tree move would create a cycle.
///
/// The adapter itself does not perform moves — it only constructs `LoroOp`s.
/// This type is part of the public tree API so callers (or future
/// higher-level helpers) can surface a structured cycle error instead of
/// stringly-typed `GrafeoLoroError::Bridge`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cycle: node {node:?} cannot be reparented under {new_parent:?}")]
pub struct CycleError {
    pub node: NodeId,
    pub new_parent: NodeId,
}

/// Tree node view: a vertex with `parent: Option<VertexId>` and
/// `children: Vec<VertexId>` derived from `:CHILD` edges.
///
/// Constructed via [`TreeAdapter::view`] for callers that want a snapshot
/// of a node's immediate tree neighbourhood without walking the maps
/// themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

/// Tree adapter — operates over a `BridgeMaps` reference.
///
/// All ops emit `LoroOp::TreeMove` (or `UpsertNode` / `DeleteNode`) for the
/// bridge to apply. The adapter does NOT touch grafeo directly.
///
/// `'a` ties the adapter to the lifetime of the borrowed `BridgeMaps` — the
/// adapter is zero-cost (a single reference field) and not `Clone` (it would
/// be a misleading alias for the same underlying maps).
pub struct TreeAdapter<'a> {
    maps: &'a BridgeMaps,
}

impl<'a> TreeAdapter<'a> {
    pub fn new(maps: &'a BridgeMaps) -> Self {
        Self { maps }
    }

    /// Borrow the underlying `BridgeMaps` (for callers that want to chain
    /// into other bridge APIs).
    pub fn maps(&self) -> &BridgeMaps {
        self.maps
    }

    /// Snapshot a node's immediate tree neighbourhood: itself, its parent,
    /// and its children. Returns `Err` if `node` is not in `node_key_map`.
    ///
    /// Equivalent to inspecting `node.parent()` + `node.children()` in old
    /// Onde — collapsed into one struct for ergonomic call sites.
    pub fn view(&self, node: NodeId) -> Result<TreeNode> {
        Ok(TreeNode {
            id: node,
            parent: self.parent(node)?,
            children: self.children(node)?,
        })
    }

    /// Look up the parent of `node` by walking the inverse of `edge_key_map`.
    ///
    /// Returns `Ok(None)` for root nodes (no incoming `:CHILD` edge).
    /// Returns `Ok(Some(parent_id))` for non-root nodes.
    /// Returns `Err(GrafeoLoroError::Bridge)` if `node` is not in
    /// `node_key_map` (unknown node).
    ///
    /// Equivalent to `node.parent()` in old Onde.
    pub fn parent(&self, node: NodeId) -> Result<Option<NodeId>> {
        let node_key = self.node_key(&node)?;
        // Walk the inverse: find an EdgeKey whose `dst_key == node_key` and
        // `label == TREE_EDGE_LABEL`. The `src_key` of that edge is the
        // parent's loro_key. Drop the read guard before looking up the
        // parent's NodeId (avoids holding two locks simultaneously).
        let parent_key_opt: Option<String> = {
            let edge_id_map = self.maps.edge_id_map.read();
            edge_id_map
                .keys()
                .find(|(_, dst_key, label)| {
                    label == TREE_EDGE_LABEL && dst_key == &node_key
                })
                .map(|(src_key, _, _)| src_key.clone())
        };
        match parent_key_opt {
            Some(pk) => Ok(Some(self.node_id(&pk)?)),
            None => Ok(None),
        }
    }

    /// Look up all children of `node` by walking `edge_id_map` forward.
    ///
    /// Returns `Ok(vec![])` for leaf nodes (no outgoing `:CHILD` edges).
    /// Returns `Ok(vec![child1, child2, ...])` for non-leaf nodes. Order
    /// follows `HashMap` iteration and is NOT stable across runs — callers
    /// needing deterministic sibling order must sort the result themselves.
    /// Returns `Err(GrafeoLoroError::Bridge)` if `node` is not in
    /// `node_key_map`.
    ///
    /// Equivalent to `node.children()` in old Onde.
    pub fn children(&self, node: NodeId) -> Result<Vec<NodeId>> {
        let node_key = self.node_key(&node)?;
        // Collect all dst_keys whose src_key == node_key and label == CHILD.
        let child_keys: Vec<String> = {
            let edge_id_map = self.maps.edge_id_map.read();
            edge_id_map
                .keys()
                .filter_map(|(src_key, dst_key, label)| {
                    if label == TREE_EDGE_LABEL && src_key == &node_key {
                        Some(dst_key.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };
        // Translate each child loro_key back to its NodeId. Missing bindings
        // are skipped (the maps are out of sync — caller bug, but we don't
        // fail the whole call for one missing child).
        let mut out = Vec::with_capacity(child_keys.len());
        for ck in child_keys {
            if let Ok(cid) = self.node_id(&ck) {
                out.push(cid);
            }
        }
        Ok(out)
    }

    /// Depth-first iterator collecting all descendants of `node`.
    ///
    /// Returns in DFS pre-order (a node is visited before its children).
    /// Does NOT include `node` itself. Cycle-safe: a `visited` set guards
    /// against corrupted map state that would otherwise infinite-loop.
    /// Returns `vec![]` if `node` is not in `node_key_map` (treated as a
    /// leaf with no children).
    ///
    /// Equivalent to `node.descendants()` in old Onde.
    pub fn descendants(&self, node: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut visited: HashSet<NodeId> = HashSet::new();
        visited.insert(node);
        // Stack-based DFS pre-order. Push children in reverse so they pop
        // in iteration order (preserves "parent before children" invariant
        // across sibling reordering).
        let mut stack: Vec<NodeId> = match self.children(node) {
            Ok(c) => c,
            Err(_) => return out,
        };
        // Reverse the initial siblings so the first child is on top of the
        // stack and pops first.
        stack.reverse();
        while let Some(n) = stack.pop() {
            if !visited.insert(n) {
                continue; // cycle guard — already visited
            }
            out.push(n);
            // Push this node's children (in reverse) so they are visited
            // before any remaining siblings of `n` on the stack.
            if let Ok(cs) = self.children(n) {
                for c in cs.into_iter().rev() {
                    stack.push(c);
                }
            }
        }
        out
    }

    /// Walk up the parent chain from `node`.
    ///
    /// Returns in order: immediate parent first, root last. Does NOT include
    /// `node` itself. Cycle-safe: a `visited` set guards against corrupted
    /// map state. Returns `vec![]` if `node` is a root or not in
    /// `node_key_map`.
    ///
    /// Equivalent to `node.ancestors()` in old Onde.
    pub fn ancestors(&self, node: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut visited: HashSet<NodeId> = HashSet::new();
        visited.insert(node);
        let mut current = node;
        loop {
            let parent = match self.parent(current) {
                Ok(Some(p)) => p,
                _ => break,
            };
            if !visited.insert(parent) {
                break; // cycle guard
            }
            out.push(parent);
            current = parent;
        }
        out
    }

    /// Build an `LoroOp::UpsertNode` for a new child node.
    ///
    /// Does NOT actually link the child to the parent — the caller must also
    /// emit a [`move_op`](Self::move_op) (or `UpsertEdge`) to establish the
    /// `:CHILD` edge. The `parent` arg is accepted for ergonomic symmetry
    /// with `move_op` / `indent_op` / `outdent_op` and is NOT embedded in
    /// the returned op (the bridge establishes the edge via `TreeMove`).
    ///
    /// The `label` arg is the grafeo vertex label (e.g. `"Folder"`,
    /// `"Item"`); the `:CHILD` edge label is added by the bridge's
    /// `apply_tree_move` translator and is NOT the caller's concern here.
    ///
    /// Equivalent to `node.create_child(label)` in old Onde — but split
    /// into two ops (UpsertNode + TreeMove) so the bridge can batch them.
    pub fn create_child_op(&self, _parent: NodeId, child_loro_key: &str, label: &str) -> LoroOp {
        LoroOp::UpsertNode {
            loro_key: child_loro_key.to_string(),
            labels: vec![label.to_string()],
            properties: HashMap::new(),
        }
    }

    /// Build an `LoroOp::TreeMove` reparenting `node` from `old_parent` to
    /// `new_parent`. The bridge translates this to delete-old-CHILD-edge +
    /// insert-new-CHILD-edge (per `apply_tree_move` in
    /// `src/bridge/grafeo_tx.rs`).
    ///
    /// Equivalent to `node.move_to(new_parent)` in old Onde.
    pub fn move_op(
        &self,
        node_loro_key: &str,
        old_parent_loro_key: &str,
        new_parent_loro_key: &str,
    ) -> LoroOp {
        LoroOp::TreeMove {
            node_key: node_loro_key.to_string(),
            old_parent_key: old_parent_loro_key.to_string(),
            new_parent_key: new_parent_loro_key.to_string(),
        }
    }

    /// Indent: reparent `node` under its previous sibling.
    ///
    /// Since the adapter does not track sibling order (the underlying
    /// `:CHILD` edges are an unordered `HashMap`-keyed set), the caller must
    /// compute the previous sibling's loro_key and pass it as
    /// `previous_sibling_loro_key`. In other words, `indent` is exactly
    /// [`move_op(node, parent, previous_sibling)`](Self::move_op).
    ///
    /// Equivalent to `node.indent()` in old Onde — but the sibling-order
    /// computation is the caller's responsibility (e.g. by sorting the
    /// result of [`children`](Self::children) on a Loro-side insertion
    /// order the caller maintains).
    pub fn indent_op(
        &self,
        node_loro_key: &str,
        parent_loro_key: &str,
        previous_sibling_loro_key: &str,
    ) -> LoroOp {
        self.move_op(node_loro_key, parent_loro_key, previous_sibling_loro_key)
    }

    /// Outdent: reparent `node` under its grandparent (the parent of its
    /// current parent).
    ///
    /// The caller passes the grandparent's loro_key directly. In other
    /// words, `outdent` is exactly
    /// [`move_op(node, parent, grandparent)`](Self::move_op).
    ///
    /// Equivalent to `node.outdent()` in old Onde.
    pub fn outdent_op(
        &self,
        node_loro_key: &str,
        parent_loro_key: &str,
        grandparent_loro_key: &str,
    ) -> LoroOp {
        self.move_op(node_loro_key, parent_loro_key, grandparent_loro_key)
    }

    // ---- private helpers -------------------------------------------------

    /// Look up the loro_key for a `NodeId`. Errors if the node is not in
    /// `node_key_map` (unknown node).
    fn node_key(&self, node: &NodeId) -> Result<String> {
        self.maps
            .node_key_map
            .read()
            .get(node)
            .cloned()
            .ok_or_else(|| GrafeoLoroError::Bridge(format!("unknown node id: {node:?}")))
    }

    /// Look up the `NodeId` for a loro_key. Errors if the key is not in
    /// `node_id_map` (unknown key).
    fn node_id(&self, loro_key: &str) -> Result<NodeId> {
        self.maps
            .node_id_map
            .read()
            .get(loro_key)
            .copied()
            .ok_or_else(|| GrafeoLoroError::Bridge(format!("unknown node key: {loro_key:?}")))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::BridgeMaps;
    use crate::constants::TREE_EDGE_LABEL;
    use crate::types::ids::{EdgeId, NodeId};

    /// Test helper: construct a `NodeId` from a `u64` regardless of whether
    /// the `grafeo` feature is on. grafeo's `NodeId` impls `From<u64>` (used
    /// by `tests/unit/tree_move.rs`); the fallback `NodeId(pub u64)` is
    /// constructed directly via tuple-struct syntax.
    fn nid(n: u64) -> NodeId {
        #[cfg(not(feature = "grafeo"))]
        {
            NodeId(n)
        }
        #[cfg(feature = "grafeo")]
        {
            NodeId::from(n)
        }
    }

    /// Test helper: construct an `EdgeId` from a `u64` (same cfg split as
    /// `nid`).
    fn eid(n: u64) -> EdgeId {
        #[cfg(not(feature = "grafeo"))]
        {
            EdgeId(n)
        }
        #[cfg(feature = "grafeo")]
        {
            EdgeId::from(n)
        }
    }

    /// Insert a node binding `loro_key ↔ NodeId(u64)` into `maps`. Returns
    /// the constructed `NodeId` for downstream assertions.
    fn add_node(maps: &BridgeMaps, loro_key: &str, id: u64) -> NodeId {
        let n = nid(id);
        maps.insert_node(loro_key.to_string(), n);
        n
    }

    /// Insert a `:CHILD` edge binding `(parent_key, child_key, "CHILD") ↔
    /// EdgeId(u64)` into `maps`.
    fn add_child_edge(maps: &BridgeMaps, parent_key: &str, child_key: &str, edge_id: u64) {
        maps.insert_edge(
            (
                parent_key.to_string(),
                child_key.to_string(),
                TREE_EDGE_LABEL.to_string(),
            ),
            eid(edge_id),
        );
    }

    // ---- parent() --------------------------------------------------------

    #[test]
    fn parent_returns_none_for_root() {
        let maps = BridgeMaps::new();
        let root = add_node(&maps, "root", 1);
        let adapter = TreeAdapter::new(&maps);
        assert_eq!(adapter.parent(root).unwrap(), None);
    }

    #[test]
    fn parent_returns_parent_for_child() {
        let maps = BridgeMaps::new();
        let root = add_node(&maps, "root", 1);
        let child = add_node(&maps, "child", 2);
        add_child_edge(&maps, "root", "child", 100);
        let adapter = TreeAdapter::new(&maps);
        assert_eq!(adapter.parent(child).unwrap(), Some(root));
    }

    #[test]
    fn parent_ignores_non_child_edges() {
        // The inverse walk must filter by label == "CHILD" — a non-CHILD
        // edge pointing at `node` does NOT make its endpoint a parent.
        let maps = BridgeMaps::new();
        let root = add_node(&maps, "root", 1);
        let child = add_node(&maps, "child", 2);
        maps.insert_edge(
            ("root".to_string(), "child".to_string(), "OTHER".to_string()),
            eid(100),
        );
        let adapter = TreeAdapter::new(&maps);
        assert_eq!(adapter.parent(child).unwrap(), None);
        // Adding the real CHILD edge flips the answer.
        add_child_edge(&maps, "root", "child", 101);
        assert_eq!(adapter.parent(child).unwrap(), Some(root));
    }

    #[test]
    fn parent_errors_for_unknown_node() {
        let maps = BridgeMaps::new();
        let adapter = TreeAdapter::new(&maps);
        let unknown = nid(999);
        assert!(adapter.parent(unknown).is_err());
    }

    // ---- children() ------------------------------------------------------

    #[test]
    fn children_returns_all_children() {
        let maps = BridgeMaps::new();
        let root = add_node(&maps, "root", 1);
        let c1 = add_node(&maps, "c1", 2);
        let c2 = add_node(&maps, "c2", 3);
        let c3 = add_node(&maps, "c3", 4);
        add_child_edge(&maps, "root", "c1", 100);
        add_child_edge(&maps, "root", "c2", 101);
        add_child_edge(&maps, "root", "c3", 102);
        // Add a non-CHILD edge to verify the label filter works.
        maps.insert_edge(
            ("root".to_string(), "x".to_string(), "OTHER".to_string()),
            eid(200),
        );
        let adapter = TreeAdapter::new(&maps);
        let children = adapter.children(root).unwrap();
        assert_eq!(children.len(), 3, "children = {children:?}");
        assert!(children.contains(&c1));
        assert!(children.contains(&c2));
        assert!(children.contains(&c3));
    }

    #[test]
    fn children_returns_empty_for_leaf() {
        let maps = BridgeMaps::new();
        let leaf = add_node(&maps, "leaf", 1);
        let adapter = TreeAdapter::new(&maps);
        assert_eq!(adapter.children(leaf).unwrap(), vec![]);
    }

    #[test]
    fn children_errors_for_unknown_node() {
        let maps = BridgeMaps::new();
        let adapter = TreeAdapter::new(&maps);
        let unknown = nid(999);
        assert!(adapter.children(unknown).is_err());
    }

    // ---- descendants() ---------------------------------------------------

    #[test]
    fn descendants_linear_chain_dfs_pre_order() {
        // Linear chain: root → a → b → c. DFS pre-order is unambiguous.
        let maps = BridgeMaps::new();
        let root = add_node(&maps, "root", 1);
        let a = add_node(&maps, "a", 2);
        let b = add_node(&maps, "b", 3);
        let c = add_node(&maps, "c", 4);
        add_child_edge(&maps, "root", "a", 100);
        add_child_edge(&maps, "a", "b", 101);
        add_child_edge(&maps, "b", "c", 102);
        let adapter = TreeAdapter::new(&maps);
        let desc = adapter.descendants(root);
        assert_eq!(desc, vec![a, b, c]);
    }

    #[test]
    fn descendants_branched_tree_set_membership() {
        // Tree:
        //        root
        //       /    \
        //      a      b
        //     / \     |
        //    a1  a2   b1
        let maps = BridgeMaps::new();
        let root = add_node(&maps, "root", 1);
        let a = add_node(&maps, "a", 2);
        let b = add_node(&maps, "b", 3);
        let a1 = add_node(&maps, "a1", 4);
        let a2 = add_node(&maps, "a2", 5);
        let b1 = add_node(&maps, "b1", 6);
        add_child_edge(&maps, "root", "a", 100);
        add_child_edge(&maps, "root", "b", 101);
        add_child_edge(&maps, "a", "a1", 102);
        add_child_edge(&maps, "a", "a2", 103);
        add_child_edge(&maps, "b", "b1", 104);
        let adapter = TreeAdapter::new(&maps);
        let desc = adapter.descendants(root);
        // HashMap iteration is non-deterministic; verify set + count.
        assert_eq!(desc.len(), 5, "descendants = {desc:?}");
        assert!(desc.contains(&a));
        assert!(desc.contains(&b));
        assert!(desc.contains(&a1));
        assert!(desc.contains(&a2));
        assert!(desc.contains(&b1));
        // The starting node is NOT included.
        assert!(!desc.contains(&root));
        // DFS pre-order invariant: each node appears AFTER its parent.
        // For `a` and `b` (whose parent is `root`, not in the output), they
        // appear before their own children.
        let pos_a = desc.iter().position(|&x| x == a).unwrap();
        let pos_a1 = desc.iter().position(|&x| x == a1).unwrap();
        let pos_a2 = desc.iter().position(|&x| x == a2).unwrap();
        assert!(pos_a < pos_a1, "a must precede a1");
        assert!(pos_a < pos_a2, "a must precede a2");
        let pos_b = desc.iter().position(|&x| x == b).unwrap();
        let pos_b1 = desc.iter().position(|&x| x == b1).unwrap();
        assert!(pos_b < pos_b1, "b must precede b1");
    }

    #[test]
    fn descendants_empty_for_leaf() {
        let maps = BridgeMaps::new();
        let leaf = add_node(&maps, "leaf", 1);
        let adapter = TreeAdapter::new(&maps);
        assert_eq!(adapter.descendants(leaf), vec![]);
    }

    #[test]
    fn descendants_cycle_safe() {
        // Corrupted map: a → b → a (cycle). The visited set must break the
        // loop; we get each node once.
        let maps = BridgeMaps::new();
        let a = add_node(&maps, "a", 1);
        let b = add_node(&maps, "b", 2);
        add_child_edge(&maps, "a", "b", 100);
        add_child_edge(&maps, "b", "a", 101); // back-edge: cycle
        let adapter = TreeAdapter::new(&maps);
        let desc = adapter.descendants(a);
        // b is reachable; a is the start (not included) but also re-reachable
        // via the cycle — the visited guard must prevent it from being
        // pushed twice.
        assert_eq!(desc.len(), 1, "descendants = {desc:?}");
        assert!(desc.contains(&b));
    }

    // ---- ancestors() -----------------------------------------------------

    #[test]
    fn ancestors_immediate_parent_first_root_last() {
        // Chain: root → mid → leaf
        let maps = BridgeMaps::new();
        let root = add_node(&maps, "root", 1);
        let mid = add_node(&maps, "mid", 2);
        let leaf = add_node(&maps, "leaf", 3);
        add_child_edge(&maps, "root", "mid", 100);
        add_child_edge(&maps, "mid", "leaf", 101);
        let adapter = TreeAdapter::new(&maps);
        let anc = adapter.ancestors(leaf);
        assert_eq!(anc, vec![mid, root]);
    }

    #[test]
    fn ancestors_empty_for_root() {
        let maps = BridgeMaps::new();
        let root = add_node(&maps, "root", 1);
        let adapter = TreeAdapter::new(&maps);
        assert_eq!(adapter.ancestors(root), vec![]);
    }

    #[test]
    fn ancestors_cycle_safe() {
        // Corrupted map: a → b → a (cycle).
        let maps = BridgeMaps::new();
        let a = add_node(&maps, "a", 1);
        let b = add_node(&maps, "b", 2);
        add_child_edge(&maps, "a", "b", 100);
        add_child_edge(&maps, "b", "a", 101); // cycle
        let adapter = TreeAdapter::new(&maps);
        let anc = adapter.ancestors(a);
        // Walk: parent(a) = b, parent(b) = a (already visited → break).
        assert_eq!(anc.len(), 1, "ancestors = {anc:?}");
        assert_eq!(anc[0], b);
    }

    // ---- view() ----------------------------------------------------------

    #[test]
    fn view_captures_parent_and_children() {
        let maps = BridgeMaps::new();
        let root = add_node(&maps, "root", 1);
        let c1 = add_node(&maps, "c1", 2);
        let c2 = add_node(&maps, "c2", 3);
        add_child_edge(&maps, "root", "c1", 100);
        add_child_edge(&maps, "root", "c2", 101);
        let adapter = TreeAdapter::new(&maps);
        let view = adapter.view(root).unwrap();
        assert_eq!(view.id, root);
        assert_eq!(view.parent, None);
        assert_eq!(view.children.len(), 2);
        assert!(view.children.contains(&c1));
        assert!(view.children.contains(&c2));
    }

    // ---- op constructors -------------------------------------------------

    #[test]
    fn move_op_produces_tree_move_variant() {
        let maps = BridgeMaps::new();
        let adapter = TreeAdapter::new(&maps);
        let op = adapter.move_op("leaf", "mid", "root");
        match op {
            LoroOp::TreeMove {
                node_key,
                old_parent_key,
                new_parent_key,
            } => {
                assert_eq!(node_key, "leaf");
                assert_eq!(old_parent_key, "mid");
                assert_eq!(new_parent_key, "root");
            }
            other => panic!("expected TreeMove, got {other:?}"),
        }
    }

    #[test]
    fn create_child_op_produces_upsert_node() {
        let maps = BridgeMaps::new();
        let adapter = TreeAdapter::new(&maps);
        let op = adapter.create_child_op(nid(1), "child-key", "Item");
        match op {
            LoroOp::UpsertNode {
                loro_key,
                labels,
                properties,
            } => {
                assert_eq!(loro_key, "child-key");
                assert_eq!(labels, vec!["Item".to_string()]);
                assert!(properties.is_empty());
            }
            other => panic!("expected UpsertNode, got {other:?}"),
        }
    }

    #[test]
    fn indent_op_targets_previous_sibling() {
        let maps = BridgeMaps::new();
        let adapter = TreeAdapter::new(&maps);
        let op = adapter.indent_op("leaf", "parent", "prev_sibling");
        match op {
            LoroOp::TreeMove {
                node_key,
                old_parent_key,
                new_parent_key,
            } => {
                assert_eq!(node_key, "leaf");
                assert_eq!(old_parent_key, "parent");
                assert_eq!(new_parent_key, "prev_sibling");
            }
            other => panic!("expected TreeMove, got {other:?}"),
        }
    }

    #[test]
    fn outdent_op_targets_grandparent() {
        let maps = BridgeMaps::new();
        let adapter = TreeAdapter::new(&maps);
        let op = adapter.outdent_op("leaf", "parent", "grandparent");
        match op {
            LoroOp::TreeMove {
                node_key,
                old_parent_key,
                new_parent_key,
            } => {
                assert_eq!(node_key, "leaf");
                assert_eq!(old_parent_key, "parent");
                assert_eq!(new_parent_key, "grandparent");
            }
            other => panic!("expected TreeMove, got {other:?}"),
        }
    }

    #[test]
    fn indent_op_equals_move_op_with_previous_sibling() {
        // indent is documented as `move_op(node, parent, previous_sibling)`.
        let maps = BridgeMaps::new();
        let adapter = TreeAdapter::new(&maps);
        let indent = adapter.indent_op("n", "p", "ps");
        let mv = adapter.move_op("n", "p", "ps");
        // LoroOp does not derive PartialEq; compare via debug repr.
        assert_eq!(format!("{indent:?}"), format!("{mv:?}"));
    }

    #[test]
    fn outdent_op_equals_move_op_with_grandparent() {
        let maps = BridgeMaps::new();
        let adapter = TreeAdapter::new(&maps);
        let outdent = adapter.outdent_op("n", "p", "gp");
        let mv = adapter.move_op("n", "p", "gp");
        assert_eq!(format!("{outdent:?}"), format!("{mv:?}"));
    }

    // ---- end-to-end traversal sanity -------------------------------------

    #[test]
    fn end_to_end_traversal_of_three_level_tree() {
        // Three-level tree:
        //     root
        //     / \
        //    a   b
        //   /|   |
        //  a1 a2 b1
        let maps = BridgeMaps::new();
        let root = add_node(&maps, "root", 1);
        let a = add_node(&maps, "a", 2);
        let b = add_node(&maps, "b", 3);
        let a1 = add_node(&maps, "a1", 4);
        let a2 = add_node(&maps, "a2", 5);
        let b1 = add_node(&maps, "b1", 6);
        add_child_edge(&maps, "root", "a", 100);
        add_child_edge(&maps, "root", "b", 101);
        add_child_edge(&maps, "a", "a1", 102);
        add_child_edge(&maps, "a", "a2", 103);
        add_child_edge(&maps, "b", "b1", 104);
        let adapter = TreeAdapter::new(&maps);

        // parent() correctness.
        assert_eq!(adapter.parent(a).unwrap(), Some(root));
        assert_eq!(adapter.parent(a1).unwrap(), Some(a));
        assert_eq!(adapter.parent(b1).unwrap(), Some(b));
        assert_eq!(adapter.parent(root).unwrap(), None);

        // ancestors(b1) = [b, root] (immediate parent first, root last).
        assert_eq!(adapter.ancestors(b1), vec![b, root]);
        // ancestors(a1) = [a, root].
        assert_eq!(adapter.ancestors(a1), vec![a, root]);

        // descendants(a) = {a1, a2} (DFS pre-order; set is unambiguous here
        // only because both are leaves — but the count + membership is the
        // contract).
        let desc_a = adapter.descendants(a);
        assert_eq!(desc_a.len(), 2);
        assert!(desc_a.contains(&a1));
        assert!(desc_a.contains(&a2));

        // descendants(root) = all 5 non-root nodes.
        let desc_root = adapter.descendants(root);
        assert_eq!(desc_root.len(), 5);
    }
}
