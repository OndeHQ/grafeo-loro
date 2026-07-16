//! Integration tests for `shadow`, `fts`, `sab`, `NodeIdTable`, and
//! `observability` modules (issue #3 sub-issues 6 + 10).
//!
//! NOTE: This file lives at top-level `tests/core_observability.rs` (NOT
//! `tests/unit/core_observability.rs`) because Cargo's auto-discovery does
//! NOT pick up files under `tests/unit/` as standalone test binaries when
//! `tests/unit/main.rs` exists (the latter claims the whole subdir as one
//! `unit` binary). The orchestrator can move this file under `tests/unit/`
//! and add `mod core_observability;` to `tests/unit/main.rs` if they want
//! it bundled. Same pattern as `tests/wasm_runtime.rs` (Agent W, Task ID 1).

use grafeo_loro::fts::{InvertedIndex, SearchHit};
use grafeo_loro::observability::{
    check_invariants, FaultInjector, FaultKind, InvariantCheckInput, InvariantViolation,
    QueueStateProbe,
};
use grafeo_loro::sab::{SabLayoutWriter, LAYOUT_ENTRY_BYTES};
use grafeo_loro::shadow::{ShadowError, ShadowRefStore};
use grafeo_loro::types::ids::NodeIdTable;

// =============================================================================
// Sub-issue 6: shadow commits
// =============================================================================

#[test]
fn shadow_commit_history() {
    let mut store = ShadowRefStore::new();
    let peer = "peer-A";
    let c1 = store.commit(peer, vec![], b"sv-1".to_vec());
    let c2 = store.commit(peer, vec![], b"sv-2".to_vec());
    let _c3 = store.commit(peer, vec![], b"sv-3".to_vec());
    assert_eq!(store.commit_count(), 3);
    // history(2) walks back from tip (c3 → c2), returning 2 commits.
    let h = store.history(peer, 2);
    assert_eq!(h.len(), 2);
    // Most-recent first.
    assert_eq!(h[0].state_vector, b"sv-3");
    assert_eq!(h[1].state_vector, b"sv-2");
    // history(10) returns all 3.
    let h_all = store.history(peer, 10);
    assert_eq!(h_all.len(), 3);
    // _c1 unused after this point — silence dead-code warning.
    let _ = c1;
    let _ = c2;
}

#[test]
fn shadow_commit_reset() {
    let mut store = ShadowRefStore::new();
    let peer = "peer-B";
    let c1 = store.commit(peer, vec![], b"sv-1".to_vec());
    let c2 = store.commit(peer, vec![], b"sv-2".to_vec());
    let c3 = store.commit(peer, vec![], b"sv-3".to_vec());
    assert_eq!(store.tip(peer), Some(c3));
    // Reset to c1 — should move tip back.
    store.reset_to(peer, c1).expect("reset_to c1");
    assert_eq!(store.tip(peer), Some(c1));
    // Reset to c2 — c2 is reachable from c1's prior tip (c3 → c2 → c1),
    // but now the tip is c1 (whose parents is []), so c2 is NOT reachable
    // from c1. Expect NotInPeerHistory.
    let err = store.reset_to(peer, c2).unwrap_err();
    match err {
        ShadowError::NotInPeerHistory { peer_id, .. } => {
            assert_eq!(peer_id, peer);
        }
        other => panic!("expected NotInPeerHistory, got {other:?}"),
    }
    // Reset to a non-existent commit.
    let bogus = [0xFFu8; 32];
    let err = store.reset_to(peer, bogus).unwrap_err();
    assert!(matches!(err, ShadowError::CommitNotFound { commit_id } if commit_id == bogus));
}

#[test]
fn shadow_commit_auto_appends_parent() {
    let mut store = ShadowRefStore::new();
    let peer = "peer-C";
    let c1 = store.commit(peer, vec![], b"sv-1".to_vec());
    let c2 = store.commit(peer, vec![], b"sv-2".to_vec()); // no explicit parents
    let commit2 = store.get_commit(&c2).expect("c2 exists");
    assert_eq!(commit2.parents, vec![c1]);
}

// =============================================================================
// Sub-issue 6: FTS
// =============================================================================

#[test]
fn fts_index_search() {
    let mut idx = InvertedIndex::new();
    idx.index_doc(1, "the quick brown fox jumps over the lazy dog");
    idx.index_doc(2, "quick brown dogs are playful");
    idx.index_doc(3, "the lazy cat sleeps all day");
    // "quick" appears in docs 1 and 2; not in doc 3.
    let hits = idx.search("quick", 10);
    assert!(!hits.is_empty());
    let ids: Vec<u32> = hits.iter().map(|h| h.doc_id).collect();
    assert!(ids.contains(&1));
    assert!(ids.contains(&2));
    assert!(!ids.contains(&3));
    // TF-IDF ranking: hits should be sorted by score descending. Verify
    // the top hit's score is >= the bottom hit's score.
    if hits.len() >= 2 {
        assert!(hits[0].score >= hits[hits.len() - 1].score);
    }
    // Multi-term query.
    let hits2 = idx.search("lazy dog", 10);
    let _ = hits2;
    // Smoke-check the SearchHit Debug impl is usable.
    let _dbg = format!(
        "{:?}",
        SearchHit {
            doc_id: 1,
            score: 1.5
        }
    );
}

#[test]
fn fts_memory_under_20mb() {
    // Index 10k docs of ~1KB each. Per issue spec, memory must stay <20MB.
    let mut idx = InvertedIndex::new();
    // Generate 10k docs of ~1KB text each. Use a small vocabulary so the
    // inverted index doesn't bloat to the full 10MB raw text size — real
    // corpora have heavy term repetition.
    let words = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
    ];
    let mut s = String::with_capacity(1024);
    for doc_id in 0..10_000u32 {
        s.clear();
        let seed = doc_id as usize;
        // ~128 words × ~8 bytes = ~1KB per doc.
        for i in 0..128usize {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(words[(seed + i) % words.len()]);
        }
        idx.index_doc(doc_id, &s);
    }
    let mem = idx.memory_usage_bytes();
    assert!(
        mem < 20_000_000,
        "memory_usage_bytes = {mem}, expected < 20_000_000 (20MB ceiling)"
    );
    // Sanity: search should still work.
    let hits = idx.search("alpha", 5);
    assert!(!hits.is_empty());
    assert!(hits.len() <= 5);
}

// =============================================================================
// Sub-issue 6: SAB layout
// =============================================================================

#[test]
fn sab_layout_recompute() {
    let mut w = SabLayoutWriter::new(8);
    // Set 3 entries with bogus y_offsets; recompute should cascade.
    w.set_layout(0, 0.0, 10.0);
    w.set_layout(1, 999.0, 20.0);
    w.set_layout(2, 999.0, 5.0);
    w.recompute_offsets(1);
    // After recompute: y[0]=0, y[1]=0+10=10, y[2]=10+20=30.
    assert_eq!(w.get_layout(0), Some((0.0, 10.0)));
    assert_eq!(w.get_layout(1), Some((10.0, 20.0)));
    assert_eq!(w.get_layout(2), Some((30.0, 5.0)));
    // total_height = max(0+10, 10+20, 30+5) = 35.
    assert_eq!(w.total_height(), 35.0);
    // Entry count + buffer length invariants.
    assert_eq!(w.entry_count(), 3);
    assert_eq!(w.as_bytes().len(), 8 * LAYOUT_ENTRY_BYTES);
}

#[test]
fn sab_layout_capacity_independent_of_entry_count() {
    let w = SabLayoutWriter::new(16);
    assert_eq!(w.capacity_entries(), 16);
    assert_eq!(w.entry_count(), 0);
    // Buffer length is capacity * entry_size, regardless of how many entries
    // have been written.
    assert_eq!(w.as_bytes().len(), 16 * LAYOUT_ENTRY_BYTES);
}

// =============================================================================
// Sub-issue 6: 64-bit IDs / NodeIdTable
// =============================================================================

#[test]
fn node_id_table_no_collision() {
    let mut t = NodeIdTable::new();
    let mut seen = std::collections::HashSet::new();
    for i in 0..10_000u64 {
        let key = format!("node-{i}");
        let id = t.intern(&key);
        assert!(seen.insert(id), "collision at i={i}, id={id}");
        // Ids should be 0..10_000 (monotonic).
        assert_eq!(id, i);
    }
    assert_eq!(t.len(), 10_000);
    // Idempotent: re-interning returns the same id.
    let id_again = t.intern("node-0");
    assert_eq!(id_again, 0);
    // Lookups both directions.
    assert_eq!(t.lookup(0), Some("node-0"));
    assert_eq!(t.lookup_by_str("node-9999"), Some(9999));
    assert_eq!(t.lookup(99_999), None);
    assert_eq!(t.lookup_by_str("missing"), None);
    assert!(!t.is_empty());
}

// =============================================================================
// Sub-issue 10: queue state probe
// =============================================================================

#[test]
fn queue_state_probe_snapshot() {
    let p = QueueStateProbe::new();
    p.set_depth(7);
    p.set_oldest_age_ms(4242);
    p.set_locked_nodes(2);
    let s = p.snapshot();
    assert_eq!(s.depth, 7);
    assert_eq!(s.oldest_age_ms, 4242);
    assert_eq!(s.locked_nodes, 2);
}

// =============================================================================
// Sub-issue 10: fault injector
// =============================================================================

#[test]
fn fault_injector_trigger() {
    let mut f = FaultInjector::new();
    // Disabled fault → Ok.
    assert!(f.trigger(FaultKind::NetworkTimeout).is_ok());
    // Enable NetworkTimeout → trigger returns Err.
    f.enable(FaultKind::NetworkTimeout);
    let err = f.trigger(FaultKind::NetworkTimeout).unwrap_err();
    assert_eq!(err.kind, FaultKind::NetworkTimeout);
    // Other faults still Ok (not armed).
    assert!(f.trigger(FaultKind::DiskFull).is_ok());
    // Disable → Ok again.
    f.disable(FaultKind::NetworkTimeout);
    assert!(f.trigger(FaultKind::NetworkTimeout).is_ok());
    // clear() disarms everything.
    f.enable(FaultKind::CorruptSnapshot);
    f.enable(FaultKind::ConcurrentSwitch);
    f.clear();
    assert!(!f.is_enabled(FaultKind::CorruptSnapshot));
    assert!(!f.is_enabled(FaultKind::ConcurrentSwitch));
}

// =============================================================================
// Sub-issue 10: invariant checks
// =============================================================================

#[test]
fn invariant_check_i14_violation() {
    // Construct a cycle: a → b → c → a.
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
fn invariant_check_all_pass_on_clean_input() {
    let keys = ["a", "b", "c", "d"];
    let epochs: [(&str, u64); 4] = [("a", 1), ("b", 2), ("c", 3), ("d", 4)];
    let edges: [(&str, &str); 3] = [("a", "b"), ("a", "c"), ("b", "d")];
    let g = ["n1", "n2", "n3"];
    let l = ["c1", "c2", "c3"];
    let epochs_pc: [(u64, u64); 3] = [(1, 2), (1, 3), (2, 4)];
    let input = InvariantCheckInput {
        node_keys: &keys,
        epochs: &epochs,
        parent_child_pairs: &edges,
        grafeo_nodes: &g,
        loro_containers: &l,
        parent_child_epochs: &epochs_pc,
    };
    assert!(check_invariants(&input).is_ok());
}

#[test]
fn invariant_check_empty_input_is_ok() {
    let input = InvariantCheckInput::default();
    assert!(check_invariants(&input).is_ok());
}
