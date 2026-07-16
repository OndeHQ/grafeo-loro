//! Observability hooks (issue #3 sub-issue 10).
//!
//! - Exposes internal queue state (`depth`, `oldest_age`, `locked_nodes`) to JS.
//! - Adds fault-injection hooks for testing edge cases.
//! - Provides invariant-check API for I4/I5/I11/I12/I14 post-mutation assertions.
//!
//! # Design
//!
//! All structs exposed to JS are `#[repr(C)]` and `Copy` so they round-trip
//! through `wasm-bindgen`'s memory boundary without serialization overhead.
//! The atomic probes use `Relaxed` ordering — observability reads don't
//! need happens-before guarantees, and the cost of acquire/release fences
//! would dominate the actual measurement.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Snapshot of internal queue state. FFI-friendly (all Copy types).
///
/// Fields are read by JS via a single `snapshot()` call into a `#[repr(C)]`
/// struct — no per-field getter round-trips across the WASM boundary.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct QueueState {
    /// Current depth of the inbound batcher queue (pending messages).
    pub depth: u32,
    /// Age (in ms) of the oldest unprocessed message in the queue. 0 if
    /// the queue is empty.
    pub oldest_age_ms: u64,
    /// Number of nodes currently locked by an in-flight op (e.g. tree-move
    /// reservations). Non-zero means a tree move is pending commit.
    pub locked_nodes: u32,
}

/// Atomic queue state tracker. Updated by the runtime, read by JS via FFI.
///
/// All setters take `&self` (not `&mut self`) because the runtime updates
/// these from multiple threads (or from reentrant WASM callbacks). Atomic
/// ops make this safe without locks.
pub struct QueueStateProbe {
    depth: AtomicU32,
    oldest_age_ms: AtomicU64,
    locked_nodes: AtomicU32,
}

impl QueueStateProbe {
    pub fn new() -> Self {
        Self {
            depth: AtomicU32::new(0),
            oldest_age_ms: AtomicU64::new(0),
            locked_nodes: AtomicU32::new(0),
        }
    }

    /// Atomically snapshot all three fields into a `QueueState`. The reads
    /// are not transactional — if the runtime updates `depth` between the
    /// `depth` read and the `oldest_age_ms` read, the snapshot may be
    /// slightly inconsistent. This is acceptable for observability (the JS
    /// side polls every ~100ms; a one-poll inconsistency is invisible).
    pub fn snapshot(&self) -> QueueState {
        QueueState {
            depth: self.depth.load(Ordering::Relaxed),
            oldest_age_ms: self.oldest_age_ms.load(Ordering::Relaxed),
            locked_nodes: self.locked_nodes.load(Ordering::Relaxed),
        }
    }

    pub fn set_depth(&self, d: u32) {
        self.depth.store(d, Ordering::Relaxed);
    }

    pub fn set_oldest_age_ms(&self, a: u64) {
        self.oldest_age_ms.store(a, Ordering::Relaxed);
    }

    pub fn set_locked_nodes(&self, n: u32) {
        self.locked_nodes.store(n, Ordering::Relaxed);
    }
}

impl Default for QueueStateProbe {
    fn default() -> Self {
        Self::new()
    }
}

/// Fault injection hooks for testing (issue #3 sub-issue 10).
///
/// `#[repr(C)]` so the enum can be passed across the FFI boundary as a
/// `u32` discriminant. Discriminant values are part of the wire format —
/// do NOT renumber existing variants (only append new ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum FaultKind {
    /// Simulate a network timeout on the next outbound op.
    NetworkTimeout = 0,
    /// Corrupt the next snapshot blob before deserialization.
    CorruptSnapshot = 1,
    /// Force a concurrent doc-switch race (two switches in flight).
    ConcurrentSwitch = 2,
    /// Simulate disk-full on the next persistence flush.
    DiskFull = 3,
}

/// Error returned by [`FaultInjector::trigger`] when a fault is enabled.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("fault injected: {kind:?}")]
pub struct FaultError {
    pub kind: FaultKind,
}

/// Fault injector — toggle faults on/off for testing. The runtime checks
/// `is_enabled(kind)` at the relevant call site and returns
/// `Err(FaultError)` if the fault is armed.
///
/// NOT thread-safe by design — fault injection is a test-only concern and
/// tests are single-threaded. If a future multi-threaded test needs it,
/// wrap in `Arc<Mutex<FaultInjector>>` at the call site.
pub struct FaultInjector {
    enabled_faults: Vec<FaultKind>,
}

impl FaultInjector {
    pub fn new() -> Self {
        Self {
            enabled_faults: Vec::new(),
        }
    }

    /// Arm `kind`. Idempotent — calling twice with the same kind is a no-op.
    pub fn enable(&mut self, kind: FaultKind) {
        if !self.is_enabled(kind) {
            self.enabled_faults.push(kind);
        }
    }

    /// Disarm `kind`. Idempotent.
    pub fn disable(&mut self, kind: FaultKind) {
        self.enabled_faults.retain(|&k| k != kind);
    }

    /// Whether `kind` is currently armed.
    pub fn is_enabled(&self, kind: FaultKind) -> bool {
        self.enabled_faults.contains(&kind)
    }

    /// If `kind` is armed, return `Err(FaultError)`. Otherwise `Ok(())`.
    ///
    /// This is the call site the runtime hooks into:
    /// ```ignore
    /// fn flush_snapshot(&self) -> Result<()> {
    ///     self.faults.trigger(FaultKind::CorruptSnapshot)?;
    ///     // ... normal flush logic ...
    /// }
    /// ```
    pub fn trigger(&self, kind: FaultKind) -> Result<(), FaultError> {
        if self.is_enabled(kind) {
            Err(FaultError { kind })
        } else {
            Ok(())
        }
    }

    /// Disarm ALL faults. Useful between test cases.
    pub fn clear(&mut self) {
        self.enabled_faults.clear();
    }
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

/// Invariant check API (issue #3 sub-issue 10). Returns the violated
/// invariant or `Ok(())` if all hold.
///
/// Each variant corresponds to a documented invariant in the grafeo-loro
/// architecture spec:
///
/// - **I4** — Uniqueness: node keys are unique within a doc.
/// - **I5** — Monotonicity: epoch numbers strictly increase over time.
/// - **I11** — Bridge bijection: every grafeo node maps to exactly one
///   Loro container and vice versa.
/// - **I12** — Epoch ordering: a child's epoch cannot precede its parent's.
/// - **I14** — Tree acyclicity: the `:CHILD` edge graph is a DAG (in
///   practice a forest).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvariantViolation {
    #[error("I4 violated: uniqueness")]
    I4Uniqueness,
    #[error("I5 violated: monotonicity")]
    I5Monotonicity,
    #[error("I11 violated: bridge bijection")]
    I11BridgeBijection,
    #[error("I12 violated: epoch ordering")]
    I12EpochOrdering,
    #[error("I14 violated: tree acyclicity")]
    I14TreeAcyclicity,
}

/// Input bundle for [`check_invariants`]. Holds borrowed references to the
/// runtime state the checker needs.
///
/// All fields are optional (`&[]` if not applicable) so the same call can
/// check a subset of invariants — e.g. a test that only cares about I14
/// (tree acyclicity) passes `node_keys=&[]`, `edges=[...]`, and leaves the
/// epoch fields empty.
///
/// # Field semantics
///
/// - `node_keys`: slice of node keys (strings). Checked for uniqueness (I4).
/// - `epochs`: slice of `(node_key, epoch)` pairs, ordered by insertion.
///   Checked for monotonicity (I5) and parent-child ordering (I12).
/// - `parent_child_pairs`: slice of `(parent_key, child_key)` edges.
///   Checked for acyclicity (I14).
/// - `grafeo_nodes` / `loro_containers`: parallel slices — checked for
///   bijection (I11). If either is empty, I11 is skipped.
#[derive(Debug, Clone, Default)]
pub struct InvariantCheckInput<'a> {
    /// Node keys to check for uniqueness (I4).
    pub node_keys: &'a [&'a str],
    /// Epoch pairs `(node_key, epoch_ms)` — checked for I5 monotonicity +
    /// I12 parent/child epoch ordering.
    pub epochs: &'a [(&'a str, u64)],
    /// `(parent_key, child_key)` edges — checked for I14 acyclicity.
    pub parent_child_pairs: &'a [(&'a str, &'a str)],
    /// Grafeo-side node keys — checked for I11 bijection with `loro_containers`.
    pub grafeo_nodes: &'a [&'a str],
    /// Loro-side container ids — checked for I11 bijection with `grafeo_nodes`.
    pub loro_containers: &'a [&'a str],
    /// Optional parent→child epoch pairs `(parent_epoch, child_epoch)` for
    /// I12. If empty, I12 is skipped.
    pub parent_child_epochs: &'a [(u64, u64)],
}

/// Run all invariant checks against `state`. Returns `Ok(())` if all hold,
/// or the first violated invariant (in I4 → I5 → I11 → I12 → I14 order).
///
/// # Invariants checked
///
/// - **I4 (uniqueness)**: `state.node_keys` has no duplicates.
/// - **I5 (monotonicity)**: `state.epochs` is non-decreasing in insertion
///   order (allows equal — strict monotonicity would reject ties).
/// - **I11 (bridge bijection)**: `state.grafeo_nodes` and
///   `state.loro_containers` have the same length and each grafeo node
///   appears exactly once (the caller is responsible for ensuring the
///   Loro side is similarly unique — we only check grafeo-side uniqueness
///   + length equality here).
/// - **I12 (epoch ordering)**: every `(parent_epoch, child_epoch)` pair in
///   `state.parent_child_epochs` satisfies `parent_epoch <= child_epoch`.
/// - **I14 (tree acyclicity)**: the graph induced by
///   `state.parent_child_pairs` is acyclic.
pub fn check_invariants(state: &InvariantCheckInput<'_>) -> Result<(), InvariantViolation> {
    // I4: uniqueness of node_keys.
    if !state.node_keys.is_empty() {
        let mut seen: HashSet<&str> = HashSet::with_capacity(state.node_keys.len());
        for &k in state.node_keys {
            if !seen.insert(k) {
                return Err(InvariantViolation::I4Uniqueness);
            }
        }
    }

    // I5: monotonicity of epochs (non-decreasing in insertion order).
    if state.epochs.len() > 1 {
        for w in state.epochs.windows(2) {
            if w[1].1 < w[0].1 {
                return Err(InvariantViolation::I5Monotonicity);
            }
        }
    }

    // I11: bridge bijection — equal lengths + unique grafeo keys.
    if !state.grafeo_nodes.is_empty() || !state.loro_containers.is_empty() {
        if state.grafeo_nodes.len() != state.loro_containers.len() {
            return Err(InvariantViolation::I11BridgeBijection);
        }
        let mut seen: HashSet<&str> = HashSet::with_capacity(state.grafeo_nodes.len());
        for &k in state.grafeo_nodes {
            if !seen.insert(k) {
                return Err(InvariantViolation::I11BridgeBijection);
            }
        }
    }

    // I12: parent epoch <= child epoch.
    for &(parent_epoch, child_epoch) in state.parent_child_epochs {
        if parent_epoch > child_epoch {
            return Err(InvariantViolation::I12EpochOrdering);
        }
    }

    // I14: tree acyclicity. Build adjacency + run DFS cycle-detect.
    if !state.parent_child_pairs.is_empty() && has_cycle(state.parent_child_pairs) {
        return Err(InvariantViolation::I14TreeAcyclicity);
    }

    Ok(())
}

/// DFS-based cycle detection on the parent→child edge graph.
///
/// Returns `true` if any cycle is reachable. Uses three-color DFS:
/// - `White` (unvisited) → not yet seen.
/// - `Gray` (in-progress) → on the current DFS stack.
/// - `Black` (done) → fully explored, no cycle through this node.
///
/// If we ever visit a `Gray` node, we've found a back-edge → cycle.
fn has_cycle(edges: &[(&str, &str)]) -> bool {
    use std::collections::HashMap;
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut color: HashMap<&str, Color> = HashMap::new();
    for &(p, c) in edges {
        adj.entry(p).or_default().push(c);
        color.insert(p, Color::White);
        color.insert(c, Color::White);
    }
    // Iterative DFS to avoid stack overflow on large graphs.
    // Stack holds (node, child_iter_index) — but since we can't easily
    // store an iterator index into a borrowed Vec, we use an explicit
    // recursion stack via a Vec of node refs and track child indices
    // separately.
    let nodes: Vec<&str> = color.keys().copied().collect();
    for &start in &nodes {
        if color.get(&start).copied().unwrap_or(Color::White) != Color::White {
            continue;
        }
        // Stack of (node, next_child_index).
        let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
        color.insert(start, Color::Gray);
        while let Some(&(node, idx)) = stack.last() {
            let children = match adj.get(node) {
                Some(c) => c.as_slice(),
                None => {
                    color.insert(node, Color::Black);
                    stack.pop();
                    continue;
                }
            };
            if idx >= children.len() {
                color.insert(node, Color::Black);
                stack.pop();
                continue;
            }
            // Advance the index for the current frame.
            stack.last_mut().unwrap().1 = idx + 1;
            let child = children[idx];
            match color.get(&child).copied().unwrap_or(Color::White) {
                Color::Gray => return true, // back-edge → cycle
                Color::Black => {}          // already explored — skip
                Color::White => {
                    color.insert(child, Color::Gray);
                    stack.push((child, 0));
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_state_probe_round_trip() {
        let p = QueueStateProbe::new();
        p.set_depth(42);
        p.set_oldest_age_ms(1234);
        p.set_locked_nodes(3);
        let s = p.snapshot();
        assert_eq!(s.depth, 42);
        assert_eq!(s.oldest_age_ms, 1234);
        assert_eq!(s.locked_nodes, 3);
    }

    #[test]
    fn fault_injector_enable_disable() {
        let mut f = FaultInjector::new();
        assert!(!f.is_enabled(FaultKind::NetworkTimeout));
        f.enable(FaultKind::NetworkTimeout);
        assert!(f.is_enabled(FaultKind::NetworkTimeout));
        assert!(f.trigger(FaultKind::NetworkTimeout).is_err());
        assert!(f.trigger(FaultKind::DiskFull).is_ok()); // not armed
        f.disable(FaultKind::NetworkTimeout);
        assert!(!f.is_enabled(FaultKind::NetworkTimeout));
        assert!(f.trigger(FaultKind::NetworkTimeout).is_ok());
    }

    #[test]
    fn invariant_i4_dup_detected() {
        let keys = ["a", "b", "a"];
        let input = InvariantCheckInput {
            node_keys: &keys,
            ..Default::default()
        };
        assert_eq!(
            check_invariants(&input),
            Err(InvariantViolation::I4Uniqueness)
        );
    }

    #[test]
    fn invariant_i14_cycle_detected() {
        // a → b → c → a (cycle)
        let edges: [(&str, &str); 3] = [("a", "b"), ("b", "c"), ("c", "a")];
        let input = InvariantCheckInput {
            parent_child_pairs: &edges,
            ..Default::default()
        };
        assert_eq!(
            check_invariants(&input),
            Err(InvariantViolation::I14TreeAcyclicity)
        );
    }

    #[test]
    fn invariant_i14_dag_passes() {
        // a → b, a → c, b → d (DAG, no cycle)
        let edges: [(&str, &str); 3] = [("a", "b"), ("a", "c"), ("b", "d")];
        let input = InvariantCheckInput {
            parent_child_pairs: &edges,
            ..Default::default()
        };
        assert!(check_invariants(&input).is_ok());
    }

    #[test]
    fn invariant_i5_non_monotonic_detected() {
        let epochs: [(&str, u64); 3] = [("a", 1), ("b", 5), ("c", 2)];
        let input = InvariantCheckInput {
            epochs: &epochs,
            ..Default::default()
        };
        assert_eq!(
            check_invariants(&input),
            Err(InvariantViolation::I5Monotonicity)
        );
    }

    #[test]
    fn invariant_i11_length_mismatch_detected() {
        let g = ["a", "b"];
        let l = ["x"];
        let input = InvariantCheckInput {
            grafeo_nodes: &g,
            loro_containers: &l,
            ..Default::default()
        };
        assert_eq!(
            check_invariants(&input),
            Err(InvariantViolation::I11BridgeBijection)
        );
    }

    #[test]
    fn invariant_i12_parent_after_child_detected() {
        let pairs: [(u64, u64); 1] = [(10, 5)]; // parent newer than child
        let input = InvariantCheckInput {
            parent_child_epochs: &pairs,
            ..Default::default()
        };
        assert_eq!(
            check_invariants(&input),
            Err(InvariantViolation::I12EpochOrdering)
        );
    }
}
