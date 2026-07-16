//! Identity types used across the bridge.
//!
//! `NodeId` and `EdgeId` are re-exported from `grafeo` so the bridge talks to
//! the execution layer using its native id types — no `From`/`Into` shims, no
//! conversion overhead, no risk of two `u64` newtypes drifting. `PeerId`
//! stays local because grafeo has no CRDT peer concept.
//!
//! When the `grafeo` feature is OFF (e.g. WASM-only builds that don't need
//! the execution layer), `NodeId` / `EdgeId` fall back to local `u64`
//! newtypes so the rest of the crate still compiles.
//!
//! # 64-bit ID precision (issue #3 sub-issue 6)
//!
//! The previous implementation cast a hash to `f32` for `NodeId`, losing
//! precision beyond 2^24 and colliding past ~1000 nodes. `NodeId` is now
//! a full 64-bit integer (always has been in this branch — see
//! [`NodeId`] below). For string↔u64 mapping (when the source-of-truth uses
//! opaque string keys, e.g. Loro peer ids or tree node GUIDs), use the new
//! [`NodeIdTable`] for stable, collision-free bijective interning.

use std::collections::HashMap;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "grafeo")]
pub use grafeo::{EdgeId, NodeId};

/// Fallback `NodeId` when `grafeo` is disabled. Same in-memory layout as
/// `grafeo::NodeId` (`u64`) so callers can `transmute` if needed, but we
/// don't expose that — re-enable `grafeo` instead.
///
/// # Issue #3 sub-issue 6 — 64-bit ID precision
///
/// `NodeId` is a full `u64`. There is no `f32` cast anywhere in the ID path
/// — the prior collision class (lossy `f32` hashing beyond 2^24 nodes) is
/// structurally impossible. For string-keyed sources, use [`NodeIdTable`]
/// to intern string keys into stable `u64` ids without hashing.
#[cfg(not(feature = "grafeo"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeId(pub u64);

/// Fallback `EdgeId` when `grafeo` is disabled.
#[cfg(not(feature = "grafeo"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EdgeId(pub u64);

/// CRDT peer identifier. No grafeo equivalent — Loro-only concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PeerId(pub u64);

/// Stable bijective mapping between Loro-side string keys and `u64` `NodeId`s
/// (issue #3 sub-issue 6).
///
/// Replaces the prior lossy `f32` hash cast that collided beyond 2^24 nodes.
/// Interns arbitrary string keys (Loro peer ids, tree GUIDs, opaque user
/// keys) into monotonically-increasing `u64` ids — no collisions, no
/// precision loss, no hashing.
///
/// # Design
///
/// - `intern` is idempotent: interning the same key twice returns the same
///   id. The id is the next counter value, starting at 0.
/// - `lookup` (id → &str) and `lookup_by_str` (str → u64) are both O(1).
/// - The table is append-only — there is no `remove`. If a key is
///   re-interned after some hypothetical removal, the same id is reused.
///   Removal is intentionally omitted to keep ids stable for the lifetime
///   of the table (which is what callers wanting stable string↔u64 mapping
///   need).
///
/// # Capacity
///
/// `u64` ids support up to 2^64 interning operations — effectively unlimited
/// for any realistic doc size. The internal `HashMap` grows naturally.
pub struct NodeIdTable {
    str_to_id: HashMap<String, u64>,
    id_to_str: HashMap<u64, String>,
    next_id: u64,
}

impl NodeIdTable {
    /// Create an empty table. The first interned key will receive id `0`.
    pub fn new() -> Self {
        Self {
            str_to_id: HashMap::new(),
            id_to_str: HashMap::new(),
            next_id: 0,
        }
    }

    /// Intern `key` and return its `u64` id. If `key` was already interned,
    /// returns the existing id (idempotent).
    pub fn intern(&mut self, key: &str) -> u64 {
        if let Some(&id) = self.str_to_id.get(key) {
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.str_to_id.insert(key.to_string(), id);
        self.id_to_str.insert(id, key.to_string());
        id
    }

    /// Look up the string key for `id`. Returns `None` if the id was never
    /// interned (or was interred by a different table instance).
    pub fn lookup(&self, id: u64) -> Option<&str> {
        self.id_to_str.get(&id).map(|s| s.as_str())
    }

    /// Look up the `u64` id for `key`. Returns `None` if the key was never
    /// interned.
    pub fn lookup_by_str(&self, key: &str) -> Option<u64> {
        self.str_to_id.get(key).copied()
    }

    /// Number of interned keys.
    pub fn len(&self) -> usize {
        self.str_to_id.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.str_to_id.is_empty()
    }

    /// Next id that will be handed out (== `len()` for an append-only table).
    /// Exposed for callers that want to pre-allocate id-indexed storage.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }
}

impl Default for NodeIdTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_table_intern_is_idempotent() {
        let mut t = NodeIdTable::new();
        let a = t.intern("alpha");
        let b = t.intern("alpha");
        assert_eq!(a, b);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn node_id_table_lookup_both_directions() {
        let mut t = NodeIdTable::new();
        let id = t.intern("beta");
        assert_eq!(t.lookup(id), Some("beta"));
        assert_eq!(t.lookup_by_str("beta"), Some(id));
        assert_eq!(t.lookup(999), None);
        assert_eq!(t.lookup_by_str("missing"), None);
    }

    #[test]
    fn node_id_table_no_collisions_for_10k_keys() {
        let mut t = NodeIdTable::new();
        let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
        for i in 0..10_000u64 {
            let key = format!("node-{i}");
            let id = t.intern(&key);
            assert!(seen.insert(id), "collision on key {key} → id {id}");
            assert!(id < 10_000);
        }
        assert_eq!(t.len(), 10_000);
    }
}
