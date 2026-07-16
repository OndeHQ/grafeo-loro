//! Grafeo edge entity + native acyclicity pre-commit hook (issue #3
//! sub-issue 7).
//!
//! # Native DAG validation (issue #3 sub-issue 7, invariant I14)
//!
//! [`validate_acyclic`] is the pre-commit hook the issue body calls for.
//! Given a batch of [`EdgeSpec`]s, it builds a temporary parent map and
//! runs a DFS cycle check before any of the edges are committed to grafeo.
//! This is distinct from [`crate::schema::tree::CycleGuard`] which is a
//! single-parent-tree guard (O(depth) incremental moves); `validate_acyclic`
//! handles multi-parent DAGs (any directed acyclic graph shape) in a single
//! batch — useful for bulk imports + cold-boot hydration.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::types::values::LoroProperty;
use lorosurgeon::{Hydrate, Reconcile};

/// Re-export of the schema-layer cycle error so callers can match against
/// a single type whether the cycle was detected by [`CycleGuard`] (single-
/// parent tree) or by [`validate_acyclic`] (multi-parent DAG batch).
///
/// Both code paths emit the same [`CycleError`] — the only difference is
/// whether `node` / `new_parent` carry the offending child/parent keys
/// (tree guard) or the offending src/dst keys of a back-edge (DAG batch).
pub use crate::schema::tree::CycleError;

#[derive(Debug, Clone, PartialEq, Hydrate, Reconcile)]
pub struct EdgeEntity {
    pub label: String,
    pub src: String,
    pub dst: String,
    pub properties: HashMap<String, LoroProperty>,
}

// ============================================================================
// Native acyclicity pre-commit hook (issue #3 sub-issue 7, invariant I14)
// ============================================================================

/// Specification of a directed edge for batch graph operations.
///
/// `src` is the parent/source, `dst` is the child/destination. Direction
/// is parent→child per architecture §7 line 265 (`(p)-[:CHILD]->(c)`),
/// matching [`crate::schema::tree::CycleGuard`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EdgeSpec {
    /// Source / parent node key.
    pub src: String,
    /// Destination / child node key.
    pub dst: String,
    /// Edge label (e.g. `CHILD`). Currently unused by cycle detection but
    /// retained for forward-compat with label-aware invariants (e.g.
    /// "only `:CHILD` edges participate in tree acyclicity; `:LINK` edges
    /// are exempt").
    pub label: String,
}

impl EdgeSpec {
    /// Construct a new edge spec.
    pub fn new(src: impl Into<String>, dst: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            src: src.into(),
            dst: dst.into(),
            label: label.into(),
        }
    }
}

impl fmt::Display for EdgeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -[{}]-> {}", self.src, self.label, self.dst)
    }
}

/// Pre-commit acyclicity check for a batch of edges (issue #3 sub-issue 7,
/// invariant I14).
///
/// Builds a temporary adjacency map from the edge batch and runs a DFS-based
/// cycle check. Returns `Err(CycleError)` on the first cycle found, with
/// `node` = the offending child key and `new_parent` = the offending parent
/// key of the back-edge.
///
/// # Cost
///
/// O(V + E) where V = unique nodes referenced by the batch, E = number of
/// edges. Suitable for pre-commit batch validation; for incremental single-
/// edge inserts use [`crate::schema::tree::CycleGuard::would_create_cycle`]
/// (O(depth) per move).
///
/// # Self-loops
///
/// An edge where `src == dst` is a trivial cycle and is rejected.
///
/// # Multi-parent DAGs
///
/// This function checks **acyclicity**, NOT tree-ness. A node may have
/// multiple parents (a diamond) and still pass. Use [`CycleGuard`] when
/// single-parent tree-ness is required.
pub fn validate_acyclic(edges: &[EdgeSpec]) -> Result<(), CycleError> {
    if edges.is_empty() {
        return Ok(());
    }

    // Build adjacency map: src → set of dsts.
    let mut adj: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut nodes: HashSet<&str> = HashSet::new();
    for e in edges {
        if e.src == e.dst {
            return Err(CycleError {
                node: e.dst.clone(),
                new_parent: e.src.clone(),
            });
        }
        adj.entry(e.src.as_str()).or_default().insert(e.dst.as_str());
        nodes.insert(e.src.as_str());
        nodes.insert(e.dst.as_str());
    }

    // Iterative DFS with three-color marking (white=unvisited, gray=in
    // progress, black=done). On encountering a gray node, we've found a
    // back-edge = cycle.
    let mut color: HashMap<&str, u8> = HashMap::new(); // 0=white, 1=gray, 2=black
    for &root in nodes.iter() {
        if color.get(root).copied().unwrap_or(0) != 0 {
            continue;
        }
        // Explicit stack: (node, neighbors_iter). Using `Vec<(&str, Vec<&str>)>`
        // — we materialize neighbor lists to avoid borrowing issues with the
        // HashMap iterator inside the DFS loop.
        let neighbors: Vec<&str> = adj.get(root).cloned().unwrap_or_default().into_iter().collect();
        let mut stack: Vec<(&str, std::vec::IntoIter<&str>)> =
            vec![(root, neighbors.into_iter())];
        color.insert(root, 1);
        while let Some((cur, mut iter)) = stack.pop() {
            if let Some(next) = iter.next() {
                // Push current back so we resume iterating its neighbors.
                stack.push((cur, iter));
                match color.get(next).copied().unwrap_or(0) {
                    0 => {
                        // White — descend.
                        color.insert(next, 1);
                        let next_neighbors: Vec<&str> =
                            adj.get(next).cloned().unwrap_or_default().into_iter().collect();
                        stack.push((next, next_neighbors.into_iter()));
                    }
                    1 => {
                        // Gray — back-edge = cycle.
                        return Err(CycleError {
                            node: next.to_string(),
                            new_parent: cur.to_string(),
                        });
                    }
                    // 2 = black — already fully processed, skip.
                    _ => {}
                }
            } else {
                // No more neighbors — mark black.
                color.insert(cur, 2);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_batch_ok() {
        assert!(validate_acyclic(&[]).is_ok());
    }

    #[test]
    fn self_loop_rejected() {
        let edges = vec![EdgeSpec::new("a", "a", "CHILD")];
        let err = validate_acyclic(&edges).unwrap_err();
        assert_eq!(err.node, "a");
        assert_eq!(err.new_parent, "a");
    }

    #[test]
    fn direct_cycle_rejected() {
        // A→B, B→A
        let edges = vec![
            EdgeSpec::new("a", "b", "CHILD"),
            EdgeSpec::new("b", "a", "CHILD"),
        ];
        assert!(validate_acyclic(&edges).is_err());
    }

    #[test]
    fn deep_cycle_rejected() {
        // A→B→C→A
        let edges = vec![
            EdgeSpec::new("a", "b", "CHILD"),
            EdgeSpec::new("b", "c", "CHILD"),
            EdgeSpec::new("c", "a", "CHILD"),
        ];
        assert!(validate_acyclic(&edges).is_err());
    }

    #[test]
    fn diamond_acyclic_ok() {
        // A→B, A→C, B→D, C→D (diamond — DAG, no cycle)
        let edges = vec![
            EdgeSpec::new("a", "b", "CHILD"),
            EdgeSpec::new("a", "c", "CHILD"),
            EdgeSpec::new("b", "d", "CHILD"),
            EdgeSpec::new("c", "d", "CHILD"),
        ];
        assert!(validate_acyclic(&edges).is_ok());
    }

    #[test]
    fn linear_chain_ok() {
        let edges = vec![
            EdgeSpec::new("a", "b", "CHILD"),
            EdgeSpec::new("b", "c", "CHILD"),
            EdgeSpec::new("c", "d", "CHILD"),
        ];
        assert!(validate_acyclic(&edges).is_ok());
    }
}
