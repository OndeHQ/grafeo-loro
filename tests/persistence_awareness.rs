//! Integration tests for issue #3 sub-issues 8 + 9.
//!
//! **Sub-issue 8 (Awareness)** — node-level presence registry + GC + FFI callback.
//! **Sub-issue 9 (Persistence)** — incremental snapshots, streaming OPFS writes,
//! snapshot diffing.
//!
//! # File placement note
//!
//! The task spec lists `tests/unit/persistence_awareness.rs` as the target
//! file, but Cargo auto-discovery does NOT pick up files under
//! `tests/unit/` as standalone `--test` targets when `tests/unit/main.rs`
//! exists (the `main.rs` filename claims the whole subdirectory as a
//! single `unit` test binary). The workflow's `cargo test --test
//! persistence_awareness` invocation requires top-level placement, so this
//! file lives at `tests/persistence_awareness.rs`. The orchestrator can
//! move it under `tests/unit/` and add `mod persistence_awareness;` to
//! `tests/unit/main.rs` if they prefer bundled discovery (matches the
//! precedent set by Agent W's `tests/wasm_runtime.rs` +
//! `tests/compression_pure_rust.rs`).

use grafeo_loro::presence::{NodePresenceRegistry, presence_register_callback};
use grafeo_loro::storage::{
    InMemoryStorage, SnapshotDiff, SnapshotStreamer, StorageBackend,
    DEFAULT_SNAPSHOT_CHUNK_SIZE,
};
use grafeo_loro::types::presence::{CursorPos, NodePresence, SelectionRange};

// ============================================================================
// Sub-issue 8: node-level presence
// ============================================================================

#[test]
fn node_presence_upsert_query() {
    let mut reg = NodePresenceRegistry::new(30_000);
    reg.upsert(NodePresence {
        peer_id: "peer-A".into(),
        vertex_id: "v1".into(),
        cursor: Some(CursorPos {
            offset: 5,
            line: 1,
            col: 5,
        }),
        selection: None,
        last_seen_ms: 1_000,
    });
    reg.upsert(NodePresence {
        peer_id: "peer-B".into(),
        vertex_id: "v1".into(),
        cursor: None,
        selection: Some(SelectionRange { start: 0, end: 10 }),
        last_seen_ms: 1_000,
    });
    reg.upsert(NodePresence {
        peer_id: "peer-A".into(),
        vertex_id: "v2".into(),
        cursor: None,
        selection: None,
        last_seen_ms: 1_000,
    });

    // Two distinct peers on v1.
    let v1 = reg.for_node("v1");
    assert_eq!(v1.len(), 2, "expected 2 peers on v1, got {}", v1.len());

    // peer-A is also on v2.
    let v2 = reg.for_node("v2");
    assert_eq!(v2.len(), 1);

    // No presence on v3.
    assert_eq!(reg.for_node("v3").len(), 0);

    // Upsert replaces — peer-A on v1 should now have the new cursor.
    reg.upsert(NodePresence {
        peer_id: "peer-A".into(),
        vertex_id: "v1".into(),
        cursor: Some(CursorPos {
            offset: 99,
            line: 9,
            col: 9,
        }),
        selection: None,
        last_seen_ms: 2_000,
    });
    let v1_after = reg.for_node("v1");
    assert_eq!(
        v1_after.len(),
        2,
        "upsert must replace, not append — still 2 peers on v1"
    );
    let peer_a_v1 = v1_after
        .iter()
        .find(|p| p.peer_id == "peer-A")
        .expect("peer-A still on v1 after upsert");
    assert_eq!(peer_a_v1.cursor.unwrap().offset, 99);
    assert_eq!(peer_a_v1.last_seen_ms, 2_000);
}

#[test]
fn node_presence_gc_stale() {
    let mut reg = NodePresenceRegistry::new(1_000); // 1s timeout
    reg.upsert(NodePresence {
        peer_id: "fresh".into(),
        vertex_id: "v1".into(),
        cursor: None,
        selection: None,
        last_seen_ms: 5_000,
    });
    reg.upsert(NodePresence {
        peer_id: "stale".into(),
        vertex_id: "v1".into(),
        cursor: None,
        selection: None,
        last_seen_ms: 1_000,
    });
    // Also add a stale peer on a different node to verify GC is global.
    reg.upsert(NodePresence {
        peer_id: "stale-2".into(),
        vertex_id: "v2".into(),
        cursor: None,
        selection: None,
        last_seen_ms: 1_500,
    });

    // now = 5_500; cutoff = 5_500 - 1_000 = 4_500.
    // fresh (5_000 >= 4_500) survives. stale (1_000 < 4_500) drops.
    // stale-2 (1_500 < 4_500) drops.
    let removed = reg.gc_stale(5_500);
    assert_eq!(removed, 2, "expected 2 stale peers dropped, got {removed}");

    let v1 = reg.for_node("v1");
    assert_eq!(v1.len(), 1);
    assert_eq!(v1[0].peer_id, "fresh");

    let v2 = reg.for_node("v2");
    assert_eq!(v2.len(), 0, "v2 should be empty after GC");
}

#[test]
fn node_presence_default_timeout_is_30s() {
    let reg = NodePresenceRegistry::default();
    assert_eq!(
        reg.stale_timeout_ms(),
        30_000,
        "default stale timeout must be 30s per issue spec"
    );
}

#[test]
fn node_presence_ffi_callback_fires_on_upsert() {
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
    reg.upsert(NodePresence {
        peer_id: "p2".into(),
        vertex_id: "v".into(),
        cursor: None,
        selection: None,
        last_seen_ms: 0,
    });

    assert_eq!(
        FIRES.load(Ordering::SeqCst),
        2,
        "FFI callback must fire once per upsert"
    );
}

#[test]
fn node_presence_rust_callback_sees_new_state() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let last_seen = Arc::new(AtomicU64::new(0));
    let captured = last_seen.clone();
    let mut reg = NodePresenceRegistry::new(1_000);
    reg.on_change(move |p| {
        captured.store(p.last_seen_ms, Ordering::SeqCst);
    });
    reg.upsert(NodePresence {
        peer_id: "p".into(),
        vertex_id: "v".into(),
        cursor: None,
        selection: None,
        last_seen_ms: 4242,
    });
    // The callback must fire AFTER the upsert lands, so it sees the NEW
    // last_seen_ms (4242), not the prior value (0).
    assert_eq!(last_seen.load(Ordering::SeqCst), 4242);
}

// ============================================================================
// Sub-issue 9: persistence — incremental snapshots, streaming OPFS, diffing
// ============================================================================

#[tokio::test]
async fn incremental_snapshot_empty_when_no_changes() {
    let s = InMemoryStorage::new();
    let snapshot = b"hello-snapshot-bytes".to_vec();
    s.save("snapshot", snapshot.clone()).await.unwrap();

    // Same "full state vector" (= snapshot bytes in this naive impl) → no
    // changes → empty delta.
    let delta = s.export_incremental_snapshot(&snapshot).await.unwrap();
    assert!(
        delta.is_empty(),
        "incremental snapshot must be empty when since_state_vector matches head"
    );
}

#[tokio::test]
async fn incremental_snapshot_returns_full_when_changed() {
    let s = InMemoryStorage::new();
    let snapshot = b"current-snapshot".to_vec();
    s.save("snapshot", snapshot.clone()).await.unwrap();

    let delta = s.export_incremental_snapshot(b"stale-state-vector").await.unwrap();
    assert_eq!(
        delta, snapshot,
        "naive in-memory backend returns full snapshot when SV differs"
    );
}

#[tokio::test]
async fn incremental_snapshot_empty_when_no_snapshot_stored() {
    let s = InMemoryStorage::new();
    let delta = s.export_incremental_snapshot(b"any-sv").await.unwrap();
    assert!(delta.is_empty(), "no snapshot stored → empty delta");
}

#[test]
fn stream_snapshot_chunks_correct_size() {
    // 200 bytes with chunk_size=64 → 4 chunks: 64, 64, 64, 8.
    let streamer = SnapshotStreamer::new(64);
    let data = vec![0xABu8; 200];

    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let n = streamer
        .stream(&data, |c| {
            chunks.push(c.to_vec());
            Ok(())
        })
        .unwrap();

    assert_eq!(n, 4, "expected 4 chunks for 200 bytes @ 64B chunk_size");
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0].len(), 64);
    assert_eq!(chunks[1].len(), 64);
    assert_eq!(chunks[2].len(), 64);
    assert_eq!(chunks[3].len(), 8, "last chunk must be the remainder");

    // Reconstruction: concatenating all chunks must reproduce the input.
    let reconstructed: Vec<u8> = chunks.into_iter().flatten().collect();
    assert_eq!(reconstructed, data);
}

#[test]
fn stream_snapshot_default_chunk_size_is_64kb() {
    let streamer = SnapshotStreamer::default();
    assert_eq!(
        streamer.chunk_size(),
        DEFAULT_SNAPSHOT_CHUNK_SIZE,
        "default chunk size must be 64KB"
    );
    assert_eq!(streamer.chunk_size(), 64 * 1024);
}

#[test]
fn stream_snapshot_exact_multiple_no_partial_chunk() {
    // 256 bytes with chunk_size=64 → exactly 4 chunks, no partial.
    let streamer = SnapshotStreamer::new(64);
    let data = vec![0u8; 256];
    let mut sizes = Vec::new();
    let n = streamer
        .stream(&data, |c| {
            sizes.push(c.len());
            Ok(())
        })
        .unwrap();
    assert_eq!(n, 4);
    assert_eq!(sizes, vec![64, 64, 64, 64]);
}

#[tokio::test]
async fn stream_snapshot_to_opfs_via_in_memory_backend() {
    use std::sync::{Arc, Mutex};
    let s = InMemoryStorage::new();
    let snapshot = vec![0xCDu8; 150]; // 150 bytes → 3 chunks @ 64B + remainder
    s.save("snapshot", snapshot.clone()).await.unwrap();

    // The chunk_callback must be `Fn + Send + Sync` (per the trait). Use
    // `Arc<Mutex<Vec<u8>>>` for interior mutability — a plain `&mut Vec<u8>`
    // capture would make the closure `FnMut` only, which the trait rejects.
    let collected = Arc::new(Mutex::new(Vec::<u8>::new()));
    {
        let collected_clone = collected.clone();
        let chunk_callback = move |chunk: &[u8]| -> grafeo_loro::Result<()> {
            collected_clone.lock().unwrap().extend_from_slice(chunk);
            Ok(())
        };
        s.stream_snapshot_to_opfs(&chunk_callback).await.unwrap();
    }
    let collected: Vec<u8> = collected.lock().unwrap().clone();
    assert_eq!(collected, snapshot, "streamed bytes must equal stored snapshot");
}

#[tokio::test]
async fn snapshot_diff_basic() {
    let s = InMemoryStorage::new();
    // 8 bytes per "op" heuristic (see InMemoryStorage::diff_snapshots).
    // base = 8 bytes (1 op), head = 24 bytes (3 ops) → 2 ops added.
    let base = vec![0u8; 8];
    let head = vec![0u8; 24];

    let diff: SnapshotDiff = s.diff_snapshots(&base, &head).await.unwrap();
    assert_eq!(diff.added_ops, 2, "expected 2 added ops (3 - 1)");
    assert_eq!(diff.removed_ops, 0, "no ops removed in append-only growth");
    assert!(
        !diff.state_vector_delta.is_empty(),
        "state_vector_delta must be non-empty when ops differ"
    );

    // Sanity: the opaque delta encodes the signed op difference (i64 LE).
    let signed_delta = i64::from_le_bytes(
        diff.state_vector_delta[..8]
            .try_into()
            .expect("state_vector_delta has at least 8 bytes"),
    );
    assert_eq!(signed_delta, 2, "signed delta = head_ops - base_ops = 3 - 1");
}

#[tokio::test]
async fn snapshot_diff_identical_is_empty() {
    let s = InMemoryStorage::new();
    let bytes = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
    let diff = s.diff_snapshots(&bytes, &bytes).await.unwrap();
    assert_eq!(diff, SnapshotDiff::empty());
    assert!(diff.is_empty());
}

#[tokio::test]
async fn snapshot_diff_shrink_reports_removed() {
    let s = InMemoryStorage::new();
    // base = 24 bytes (3 ops), head = 8 bytes (1 op) → 2 removed.
    let base = vec![0u8; 24];
    let head = vec![0u8; 8];
    let diff = s.diff_snapshots(&base, &head).await.unwrap();
    assert_eq!(diff.added_ops, 0);
    assert_eq!(diff.removed_ops, 2);
}
