//! Echo-prevention origin checks + FFI-friendly commit-origin tracking.
//!
//! Two origin tags exist in the system: [`ORIGIN_GRAFEO_BRIDGE`] (set on Loro
//! transactions written by the Grafeo→Loro outbound worker) and
//! [`ORIGIN_LORO_BRIDGE`] (set on Grafeo transactions written by the
//! Loro→Grafeo inbound worker — advisory only; see module doc on
//! [`crate::bridge::sync_engine`] for the epoch side-channel that actually
//! prevents Grafeo→Loro echo).
//!
//! The Loro→Grafeo path uses `is_grafeo_bridge_origin` in the Loro
//! subscriber handler to filter echoes of our own outbound writes.
//! `is_loro_bridge_origin` is kept for symmetry / future use; the outbound
//! CDC poller currently uses the epoch side-channel instead of an origin
//! check (grafeo's `ChangeEvent` has no `origin` field — see Devil BLOCKER B2).
//!
//! ## Issue #3 sub-issue 2 — FFI origin tracking
//!
//! [`OriginKind`] is a `#[repr(C)]` enum exposed to FFI so JS/WASM callers
//! can label each commit as `Structural`, `Typing`, or `Other` without
//! constructing ad-hoc origin strings on the JS side. The thread-local
//! `NEXT_COMMIT_ORIGIN` holds the next commit's origin; the bridge layer
//! (orchestrator-owned `src/bridge/batcher.rs::flush_inner`) calls
//! [`take_next_commit_origin()`] immediately before `prepared.commit()` and
//! passes the result to `doc.set_next_commit_origin(...)`.

use std::cell::RefCell;

use crate::constants::{ORIGIN_GRAFEO_BRIDGE, ORIGIN_LORO_BRIDGE};

/// C-FFI-compatible enum tagging the semantic kind of the next commit's
/// origin (issue #3 sub-issue 2).
///
/// `#[repr(C)]` + explicit discriminants so JS/WASM callers can construct
/// the value as a single `u8` in linear memory and pass it by value across
/// the FFI boundary. The bridge layer composes the actual origin string
/// (e.g. `"structural"`, `"typing:node-42"`, `"other"`) — callers do NOT
/// need to format strings on the JS side.
///
/// ## Discriminant values (frozen — DO NOT renumber)
///
/// | Variant      | Discriminant |
/// |--------------|--------------|
/// | `Structural` | `0`          |
/// | `Typing`     | `1`          |
/// | `Other`      | `2`          |
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginKind {
    /// Structural mutation: node/edge upsert, tree reparent, schema change.
    /// Composed origin string: `"structural"`.
    Structural = 0,
    /// Typing mutation on a text field bound to a specific node. Composed
    /// origin string: `"typing:{node_id}"` (or `"typing"` if `node_id` is `None`).
    Typing = 1,
    /// Anything else (cursor moves, ephemeral state, etc.). Composed origin
    /// string: `"other"` (or `"other:{node_id}"` if `node_id` is present).
    Other = 2,
}

impl OriginKind {
    /// Compose the origin string for this kind + optional node id. SSOT for
    /// origin-string composition — anti-plenger #2 (DRY/SSOT).
    pub fn compose_origin(self, node_id: Option<&str>) -> String {
        match (self, node_id) {
            (OriginKind::Structural, None) => "structural".to_string(),
            (OriginKind::Structural, Some(id)) => format!("structural:{id}"),
            (OriginKind::Typing, None) => "typing".to_string(),
            (OriginKind::Typing, Some(id)) => format!("typing:{id}"),
            (OriginKind::Other, None) => "other".to_string(),
            (OriginKind::Other, Some(id)) => format!("other:{id}"),
        }
    }
}

// Thread-local stash for the next commit's composed origin string (issue
// #3 sub-issue 2). Set via `set_next_commit_origin`; consumed via
// `take_next_commit_origin` by the bridge layer immediately before
// `prepared.commit()`. The stash is `Option` so "no origin set" is
// distinguishable from "origin set to empty string" (the former falls back
// to the bridge default `ORIGIN_LORO_BRIDGE`).
//
// TODO(orchestrator): if a future caller needs cross-thread origin
// propagation (e.g. a Grafeo→Loro flush queued from JS but executed on the
// blocking pool), replace this with an `Arc<Mutex<Option<String>>>`
// threaded into `BatcherConfig`.
thread_local! {
    pub(crate) static NEXT_COMMIT_ORIGIN: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Set the next commit's origin (issue #3 sub-issue 2 FFI entry point).
///
/// Composes an origin string from `kind` + optional `node_id` and stashes
/// it in the thread-local `NEXT_COMMIT_ORIGIN`. The bridge layer picks it
/// up via [`take_next_commit_origin`] before the next `prepared.commit()`.
pub fn set_next_commit_origin(kind: OriginKind, node_id: Option<&str>) {
    let composed = kind.compose_origin(node_id);
    NEXT_COMMIT_ORIGIN.with(|cell| {
        *cell.borrow_mut() = Some(composed);
    });
}

/// Take (consume) the stashed next-commit origin. Returns `None` if no
/// origin was set on the current thread since the last commit.
pub fn take_next_commit_origin() -> Option<String> {
    NEXT_COMMIT_ORIGIN.with(|cell| cell.borrow_mut().take())
}

/// Peek the stashed next-commit origin without consuming it. Diagnostic
/// use only — production code should use [`take_next_commit_origin`].
#[allow(dead_code)]
pub fn peek_next_commit_origin() -> Option<String> {
    NEXT_COMMIT_ORIGIN.with(|cell| cell.borrow().clone())
}

/// True iff `origin` was produced by the Grafeo→Loro outbound bridge.
#[allow(dead_code)]
pub fn is_grafeo_bridge_origin(origin: &str) -> bool {
    origin == ORIGIN_GRAFEO_BRIDGE
}

/// True iff `origin` was produced by the Loro→Grafeo inbound bridge.
#[allow(dead_code)]
pub fn is_loro_bridge_origin(origin: Option<&str>) -> bool {
    origin == Some(ORIGIN_LORO_BRIDGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_kind_compose_structural() {
        assert_eq!(OriginKind::Structural.compose_origin(None), "structural");
        assert_eq!(
            OriginKind::Structural.compose_origin(Some("n1")),
            "structural:n1"
        );
    }

    #[test]
    fn origin_kind_compose_typing() {
        assert_eq!(OriginKind::Typing.compose_origin(None), "typing");
        assert_eq!(
            OriginKind::Typing.compose_origin(Some("node-42")),
            "typing:node-42"
        );
    }

    #[test]
    fn origin_kind_compose_other() {
        assert_eq!(OriginKind::Other.compose_origin(None), "other");
        assert_eq!(OriginKind::Other.compose_origin(Some("x")), "other:x");
    }

    #[test]
    fn origin_kind_discriminants_are_stable() {
        assert_eq!(OriginKind::Structural as u8, 0);
        assert_eq!(OriginKind::Typing as u8, 1);
        assert_eq!(OriginKind::Other as u8, 2);
    }

    #[test]
    fn set_then_take_round_trips_composed_origin() {
        let _ = take_next_commit_origin();
        set_next_commit_origin(OriginKind::Typing, Some("node-7"));
        let taken = take_next_commit_origin();
        assert_eq!(taken.as_deref(), Some("typing:node-7"));
        let taken2 = take_next_commit_origin();
        assert!(taken2.is_none());
    }

    #[test]
    fn set_with_no_node_id_composes_bare_origin() {
        let _ = take_next_commit_origin();
        set_next_commit_origin(OriginKind::Structural, None);
        assert_eq!(take_next_commit_origin().as_deref(), Some("structural"));
    }
}
