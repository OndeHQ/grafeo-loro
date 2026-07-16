//! Ephemeral presence: WebSocket-channel %EPH envelope + node-level presence.
//!
//! Issue #3 sub-issue 8: `NodePresenceRegistry` holds `NodePresence` states
//! keyed by `(peer_id, vertex_id)`. Stale peers are dropped after a
//! configurable timeout (default 30s) via `gc_stale`.

pub mod socket;
pub use socket::{EphEnvelope, PresenceManager};

use std::collections::HashMap;

use crate::types::presence::NodePresence;

/// Default stale timeout for node-level presence: 30 seconds.
pub const DEFAULT_NODE_PRESENCE_STALE_TIMEOUT_MS: u64 = 30_000;

/// Registry of node-level presence states. GCs stale peers after timeout.
///
/// Keyed by `(peer_id, vertex_id)` so the same peer can be present on
/// multiple nodes simultaneously. Mutating methods take `&mut self` —
/// wrap in `Mutex` / `RefCell` for shared mutability.
pub struct NodePresenceRegistry {
    states: HashMap<(String, String), NodePresence>,
    stale_timeout_ms: u64,
    #[allow(clippy::type_complexity)]
    on_change: Option<Box<dyn Fn(&NodePresence) + Send + Sync>>,
}

impl NodePresenceRegistry {
    /// Construct with a custom stale timeout (in milliseconds).
    pub fn new(stale_timeout_ms: u64) -> Self {
        Self {
            states: HashMap::new(),
            stale_timeout_ms,
            on_change: None,
        }
    }

    /// Insert or replace a presence entry. The `on_change` callback (if
    /// registered) is fired after the insert.
    pub fn upsert(&mut self, presence: NodePresence) {
        let key = (presence.peer_id.clone(), presence.vertex_id.clone());
        self.states.insert(key, presence.clone());
        if let Some(cb) = &self.on_change {
            cb(&presence);
        }
    }

    /// Remove the presence entry for `(peer_id, vertex_id)`. Idempotent.
    pub fn remove(&mut self, peer_id: &str, vertex_id: &str) {
        self.states
            .remove(&(peer_id.to_string(), vertex_id.to_string()));
    }

    /// Return all presence entries bound to `vertex_id` (borrowed refs).
    pub fn for_node(&self, vertex_id: &str) -> Vec<&NodePresence> {
        self.states
            .values()
            .filter(|p| p.vertex_id == vertex_id)
            .collect()
    }

    /// Drop presence entries whose `last_seen_ms` is older than
    /// `now_ms - stale_timeout_ms`. Returns the number removed.
    pub fn gc_stale(&mut self, now_ms: u64) -> usize {
        let cutoff = now_ms.saturating_sub(self.stale_timeout_ms);
        let before = self.states.len();
        self.states.retain(|_, p| p.last_seen_ms >= cutoff);
        before - self.states.len()
    }

    /// Register a callback fired after every `upsert`.
    pub fn on_change(&mut self, callback: impl Fn(&NodePresence) + Send + Sync + 'static) {
        self.on_change = Some(Box::new(callback));
    }

    /// Read-only accessor for the configured stale timeout.
    pub fn stale_timeout_ms(&self) -> u64 {
        self.stale_timeout_ms
    }

    /// Total number of presence entries.
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// True iff `len() == 0`.
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

impl Default for NodePresenceRegistry {
    fn default() -> Self {
        Self::new(DEFAULT_NODE_PRESENCE_STALE_TIMEOUT_MS)
    }
}

/// Register a C-ABI callback fired on every presence change (issue #3
/// sub-issue 8 FFI entry point). The callback receives a `*const
/// NodePresence` valid only for the duration of the call.
///
/// # Safety
///
/// This function is safe to call. The unsafety is on the C side: the
/// callee MUST NOT dereference the pointer after returning.
pub fn presence_register_callback(
    reg: &mut NodePresenceRegistry,
    cb: extern "C" fn(*const NodePresence),
) {
    reg.on_change(move |p: &NodePresence| cb(p as *const NodePresence));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::presence::{CursorPos, SelectionRange};

    #[test]
    fn smoke_upsert_query_remove() {
        let mut reg = NodePresenceRegistry::new(1_000);
        reg.upsert(NodePresence {
            peer_id: "p1".into(),
            vertex_id: "v1".into(),
            cursor: Some(CursorPos {
                offset: 1,
                line: 0,
                col: 1,
            }),
            selection: None,
            last_seen_ms: 100,
        });
        reg.upsert(NodePresence {
            peer_id: "p2".into(),
            vertex_id: "v1".into(),
            cursor: None,
            selection: Some(SelectionRange { start: 0, end: 5 }),
            last_seen_ms: 100,
        });
        assert_eq!(reg.for_node("v1").len(), 2);
        assert_eq!(reg.for_node("v2").len(), 0);
        reg.remove("p1", "v1");
        assert_eq!(reg.for_node("v1").len(), 1);
    }

    #[test]
    fn smoke_gc_stale() {
        let mut reg = NodePresenceRegistry::new(500);
        reg.upsert(NodePresence {
            peer_id: "fresh".into(),
            vertex_id: "v".into(),
            cursor: None,
            selection: None,
            last_seen_ms: 1_000,
        });
        reg.upsert(NodePresence {
            peer_id: "stale".into(),
            vertex_id: "v".into(),
            cursor: None,
            selection: None,
            last_seen_ms: 100,
        });
        let removed = reg.gc_stale(1_200);
        assert_eq!(removed, 1);
        assert_eq!(reg.for_node("v").len(), 1);
    }

    #[test]
    fn smoke_change_callback_fires() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let mut reg = NodePresenceRegistry::new(1_000);
        reg.on_change(move |_p| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });
        reg.upsert(NodePresence {
            peer_id: "p".into(),
            vertex_id: "v".into(),
            cursor: None,
            selection: None,
            last_seen_ms: 0,
        });
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn smoke_ffi_callback_fires() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static FIRES: AtomicUsize = AtomicUsize::new(0);
        extern "C" fn cb(_p: *const NodePresence) {
            FIRES.fetch_add(1, Ordering::SeqCst);
        }
        let mut reg = NodePresenceRegistry::new(1_000);
        presence_register_callback(&mut reg, cb);
        reg.upsert(NodePresence {
            peer_id: "p".into(),
            vertex_id: "v".into(),
            cursor: None,
            selection: None,
            last_seen_ms: 0,
        });
        assert_eq!(FIRES.load(Ordering::SeqCst), 1);
    }
}
