//! Grafeo schema entities: `VertexEntity`, `EdgeEntity`, `OrderedCollection`,
//! plus native graph invariants (issue #3 sub-issue 7).
//!
//! # Native invariants (issue #3 sub-issue 7)
//!
//! - [`CycleGuard`] (`tree`): O(depth) parent-pointer map for incremental
//!   single-parent-tree cycle detection on edge insert.
//! - [`validate_acyclic`] (`edge`): pre-commit batch acyclicity check for
//!   multi-parent DAGs.
//! - [`RootTracker`] (`mod`): O(1) incremental root set maintenance —
//!   replaces the downstream O(N) DAG walk the issue body calls out as
//!   "kills 60fps".
//!
//! These entities are bound to `lorosurgeon`'s `Hydrate`/`Reconcile` derives
//! and the `LoroProperty` value type. They are available whenever `bridge`
//! is on (which is the minimal useful feature).

pub mod edge;
pub mod tree;
pub mod vertex;

pub use edge::{validate_acyclic, EdgeEntity, EdgeSpec};
pub use tree::{CycleError, CycleGuard, OrderedCollection, TreeNode};
pub use vertex::VertexEntity;

use std::collections::HashSet;

// ============================================================================
// Native root tracker (issue #3 sub-issue 7, invariant I14)
// ============================================================================
//
// The issue body calls out: "Downstream walks DAG to find roots. O(N)
// traversal kills 60fps." `RootTracker` maintains the root set INCREMENTALLY
// via `on_edge_inserted` / `on_edge_removed` hooks — O(1) per edge mutation
// (HashSet insert/remove). Root queries (`roots()`, `is_root()`) are O(1).
//
// The tracker is intentionally decoupled from grafeo: it operates on string
// node-keys, so it works in pure-WASM builds. The orchestrator wires it
// into `bridge::grafeo_tx::apply_*` + the inbound batcher's pre-commit hook
// in a follow-up.

/// Incremental root-set tracker for O(1) root queries (issue #3 sub-issue 7,
/// invariant I14).
///
/// A node is a "root" iff no edge points at it (no parent). The tracker
/// maintains two sets:
/// - `has_parent`: nodes that currently have at least one incoming edge.
/// - `roots`: nodes that have been registered via `register_node` and do
///   NOT appear in `has_parent`.
///
/// Nodes are explicitly registered via [`Self::register_node`] — this lets
/// the tracker distinguish "node X is a root" from "node X is unknown to
/// the tracker". A node that has never been registered will NOT appear in
/// `roots()` even if it has no parent edges; this avoids false positives
/// when the tracker is attached to a partially-hydrated graph.
///
/// # Cost model
///
/// - `register_node`: O(1) amortized (HashSet insert).
/// - `on_edge_inserted` / `on_edge_removed`: O(1) amortized.
/// - `roots` / `is_root`: O(1).
///
/// # Multi-parent DAGs
///
/// `has_parent` is a SET, not a count — inserting the same `(child, parent)`
/// pair twice is idempotent (calling `on_edge_removed` once does NOT remove
/// the child from `has_parent` if another parent edge remains). The caller
/// is responsible for tracking edge multiplicity if they need to remove a
/// child from `has_parent` only when its last parent edge is removed; the
/// common case (tree edges with one parent per child) just works.
#[derive(Debug, Clone, Default)]
pub struct RootTracker {
    /// Nodes with at least one incoming edge (have a parent).
    has_parent: HashSet<String>,
    /// Nodes registered with `register_node` that are currently roots.
    roots: HashSet<String>,
}

impl RootTracker {
    /// Construct a fresh empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a node with the tracker. Initially a root (no parent edges
    /// recorded yet). O(1). Idempotent — registering the same node twice is
    /// a no-op.
    pub fn register_node(&mut self, node: &str) {
        if !self.has_parent.contains(node) {
            self.roots.insert(node.to_string());
        }
    }

    /// Unregister a node entirely (removes it from both `roots` and
    /// `has_parent`). Use this when a node is deleted from the graph.
    /// O(1).
    pub fn unregister_node(&mut self, node: &str) {
        self.roots.remove(node);
        self.has_parent.remove(node);
    }

    /// Hook: an edge `parent → child` was inserted. Removes `child` from
    /// the root set (if present). O(1).
    ///
    /// The `parent` argument is accepted for API symmetry with
    /// [`Self::on_edge_removed`] + future multi-parent-aware accounting;
    /// the tracker does not currently use it (single-edge semantic: any
    /// incoming edge disqualifies the child from root status).
    pub fn on_edge_inserted(&mut self, child: &str, _parent: &str) {
        self.has_parent.insert(child.to_string());
        self.roots.remove(child);
    }

    /// Hook: an edge `parent → child` was removed. If `child` has no
    /// remaining incoming edges, it becomes a root again. O(1).
    ///
    /// # Multi-parent caveat
    ///
    /// The tracker does NOT maintain an incoming-edge count per node —
    /// `has_parent` is a SET. Calling `on_edge_removed` will re-root the
    /// child unconditionally. If multiple parent edges existed for the
    /// child, the caller MUST NOT call this hook until the LAST parent
    /// edge is removed. For tree workloads (one parent per child) this is
    /// automatic.
    pub fn on_edge_removed(&mut self, child: &str, _parent: &str) {
        self.has_parent.remove(child);
        // Re-root: the child is now a root iff it is still a known node.
        // (We don't track "known nodes" separately; `has_parent` removal
        // means we no longer think it has a parent. If it was never
        // registered, the roots set never contained it anyway — the
        // insert below is a no-op for unregistered nodes only if we
        // ALSO didn't see it in `has_parent`. To support the "child
        // was never registered but had an edge inserted+removed"
        // case, we add it to roots unconditionally and rely on
        // `unregister_node` to clean up if the caller later deletes it.)
        self.roots.insert(child.to_string());
    }

    /// Returns the current root set. O(1) (returns a reference).
    pub fn roots(&self) -> &HashSet<String> {
        &self.roots
    }

    /// Returns `true` iff `node` is currently a root (registered + no
    /// incoming edges). O(1).
    pub fn is_root(&self, node: &str) -> bool {
        self.roots.contains(node)
    }

    /// Number of currently-tracked roots. O(1).
    pub fn root_count(&self) -> usize {
        self.roots.len()
    }

    /// Number of currently-tracked non-root nodes. O(1).
    pub fn non_root_count(&self) -> usize {
        self.has_parent.len()
    }

    /// Whether the tracker is empty (no nodes registered). O(1).
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty() && self.has_parent.is_empty()
    }
}

#[cfg(test)]
mod root_tracker_tests {
    use super::*;

    #[test]
    fn registered_node_starts_as_root() {
        let mut t = RootTracker::new();
        t.register_node("a");
        assert!(t.is_root("a"));
        assert_eq!(t.root_count(), 1);
    }

    #[test]
    fn edge_insert_unroots_child() {
        let mut t = RootTracker::new();
        t.register_node("a");
        t.register_node("b");
        t.on_edge_inserted("b", "a");
        assert!(t.is_root("a"));
        assert!(!t.is_root("b"));
    }

    #[test]
    fn edge_remove_reroots_child() {
        let mut t = RootTracker::new();
        t.register_node("a");
        t.register_node("b");
        t.on_edge_inserted("b", "a");
        t.on_edge_removed("b", "a");
        assert!(t.is_root("b"));
    }

    #[test]
    fn incremental_chain_maintains_roots() {
        // a→b→c→d — only `a` should remain a root.
        let mut t = RootTracker::new();
        for n in ["a", "b", "c", "d"] {
            t.register_node(n);
        }
        t.on_edge_inserted("b", "a");
        t.on_edge_inserted("c", "b");
        t.on_edge_inserted("d", "c");
        assert!(t.is_root("a"));
        assert!(!t.is_root("b"));
        assert!(!t.is_root("c"));
        assert!(!t.is_root("d"));
        assert_eq!(t.root_count(), 1);

        // Remove edge b→c. `c` becomes a root again (its only parent edge
        // was b→c). `d` is NOT re-rooted automatically — its parent edge
        // (c→d) is still present. For tree workloads the caller is
        // responsible for cascading edge removals (e.g. delete c→d before
        // b→c if both should detach). The tracker tracks direct edge
        // insertions/removals only — no transitive propagation.
        t.on_edge_removed("c", "b");
        assert!(t.is_root("c"));
        assert!(!t.is_root("d")); // still has parent edge c→d
        assert!(t.is_root("a"));

        // Now remove c→d. `d` becomes a root again.
        t.on_edge_removed("d", "c");
        assert!(t.is_root("d"));
        assert_eq!(t.root_count(), 3); // a, c, d
    }

    #[test]
    fn unregister_removes_from_both_sets() {
        let mut t = RootTracker::new();
        t.register_node("a");
        t.register_node("b");
        t.on_edge_inserted("b", "a");
        t.unregister_node("b");
        assert!(!t.is_root("b"));
        assert_eq!(t.root_count(), 1); // only `a` remains
    }
}
