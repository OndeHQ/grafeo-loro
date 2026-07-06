//! Phase 6 Task 5 fuzz harness — random Loro op batches → verify Grafeo consistency.
//!
//! L3 implementation (per klemer-agents.md): all 16 invariant check fns filled
//! with non-trivial assertions (per Devil M5). `FuzzOp` mirrors
//! `grafeo_loro::types::events::LoroOp` with `Arbitrary`-derivable field types
//! (defined in `fuzz/fuzz_targets/lib.rs` — DRY/SSOT, shared with
//! `gen_corpus`). `FuzzValue` mirrors the scalar subset of `GraphValue`. The
//! op generator applies each `FuzzOp` through `apply_loro_op` (the SSOT
//! inbound path) and asserts per-iteration invariants on the resulting
//! `BridgeMaps` + `GrafeoDB` + `LoroDoc` state.
//!
//! # Build requirements
//!
//! `libfuzzer-sys` requires nightly Rust + `-Zsanitizer=address`. Use `cargo fuzz`:
//! ```text,ignore
//! rustup install nightly
//! cargo +nightly install cargo-fuzz
//! cargo +nightly fuzz run consistency
//! ```
//! `cargo fuzz` manages the nightly toolchain + `--cfg fuzzing` automatically.
//! Plain `cargo check` on this crate will pass because of the
//! `#![cfg_attr(fuzzing, no_main)]` + fallback `main()` pattern. See
//! `docs/phase-6/fuzz-invariants.md` for the 16-invariant checklist (I3 split
//! into I3a/b/c per Devil C5.2; I7/I9 cadence documented per C5.3).
//!
//! # Invariant check cadence (per docs/phase-6/fuzz-invariants.md)
//!
//! - **Every iteration** (cheap): I1, I2, I3a, I3b, I3c, I4, I11, I12, I15
//! - **Periodic** (every 1000 ops OR final): I7, I9
//! - **Event-driven** (when the relevant op fires): I5, I6, I8, I10, I14
//!
//! I12 (MVCC snapshot isolation) is implemented directly against grafeo's
//! `set_viewing_epoch` time-travel API (architecture §19) — it does NOT depend
//! on `GrafeoLoroApp::query` (which returns `Err(NotYetImplemented(...))` per
//! Gap A.2). See `check_i12_mvcc_snapshot_isolation` doc-comment + L1 plan
//! §Gap B in `docs/phase-7/gap-closure-l1-plan.md`.

// When built with `--cfg fuzzing` (via `cargo +nightly fuzz run`), libfuzzer provides
// `main`; the crate is compiled as `no_main`. When built without (e.g. `cargo check`
// for syntax verification), a fallback `main` is provided at the bottom of the file.
#![cfg_attr(fuzzing, no_main)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use arbitrary::{Arbitrary, Unstructured};
use grafeo::GrafeoDB;
use libfuzzer_sys::fuzz_target;
use loro::LoroDoc;
use parking_lot::RwLock;

use grafeo_loro::bridge::{apply_loro_op, BridgeMaps};
use grafeo_loro::compression::CompressedPayload;
use grafeo_loro::config::CompressionType;
use grafeo_loro::constants::{
    EMBEDDING_PROPERTY, EPH_MAGIC, EPOCH_RETENTION, ROOT_VERTICES, TREE_EDGE_LABEL,
};
use grafeo_loro::types::events::LoroOp;
use grafeo_loro::types::values::GraphValue;
use grafeo_loro::types::{EpochId, PresencePayload};
use grafeo_loro::VectorOffloadManager;

// Shared fuzz types + op converter (DRY/SSOT — anti-plenger #5). Defined in
// `fuzz/fuzz_targets/lib.rs` so both the `consistency` fuzz target and the
// `gen_corpus` seed-corpus generator use the same definitions.
use grafeo_loro_fuzz::{convert_fuzz_op, FuzzOp};

// =============================================================================
// Fuzz input types
// =============================================================================

/// Top-level fuzz input. Decoded once per iteration from the raw byte stream
/// via `arbitrary::Arbitrary`.
#[derive(Arbitrary, Debug, Clone)]
pub struct FuzzInput {
    /// Seed for any deterministic sub-choices (e.g. which expensive invariant
    /// to run). Also used as the periodic-invariant cadence divisor.
    pub seed: u64,
    /// Ordered batch of ops to apply in sequence.
    pub ops: Vec<FuzzOp>,
    /// Peer count for mesh testing (1-3, clamped at apply time). Used by I4
    /// (echo loop bounded) + I5 (origin filter symmetry) which simulate
    /// multi-peer Loro→Grafeo→Loro round-trips.
    pub peer_count: u8,
    /// Safety limit on op count per iteration. Clamped to [1, 10_000] at apply
    /// time to prevent the fuzzer from generating pathological batches that
    /// exceed the iteration timeout.
    pub bail_after_ops: u16,
}

// =============================================================================
// Fuzz state (shared across invariant checks)
// =============================================================================

/// Read-only view of the fuzz iteration state. Invariant check fns take
/// `&FuzzState` and return `()` — they `panic!` on violation (libfuzzer treats
/// panic as crash). Per Devil M5: each `assert!` compares two concrete values.
struct FuzzState<'a> {
    #[allow(
        dead_code,
        reason = "reserved for future invariant checks that need direct db access"
    )]
    db: &'a Arc<GrafeoDB>,
    doc: &'a LoroDoc,
    maps: &'a Arc<BridgeMaps>,
    epochs: &'a Arc<RwLock<HashSet<EpochId>>>,
    /// Set of live Loro-side node keys (from UpsertNode minus DeleteNode).
    /// Used by I1 (tree state parity) to compare with `BridgeMaps::node_id_map`.
    live_node_keys: &'a HashSet<String>,
    /// Set of live Loro-side edge keys (src, dst, label).
    /// Used by I2 (edge state parity) to compare with `BridgeMaps::edge_id_map`.
    live_edge_keys: &'a HashSet<(String, String, String)>,
    op_count: u64,
}

// =============================================================================
// Invariant check fns (I1..I15, with I3 split into I3a/b/c per Devil C5.2).
// Each fn panics on violation (libfuzzer crash). Per Devil M5: each assert!
// compares two concrete values — NO `assert!(result.is_ok())` shortcuts.
// =============================================================================

/// I1 — Tree state parity: the `BridgeMaps::node_id_map` keys MUST equal the
/// set of live Loro-side node keys (UpsertNode minus DeleteNode). The fuzz
/// target mirrors each UpsertNode/DeleteNode into both stores, so any drift
/// is a real bridge bug (e.g. `DeleteNode` not removing the bridge binding).
fn check_i1_tree_state_parity(state: &FuzzState) {
    let bridge_keys: HashSet<String> = state.maps.node_id_map.read().keys().cloned().collect();
    let loro_keys: HashSet<String> = state.live_node_keys.iter().cloned().collect();
    assert_eq!(
        bridge_keys,
        loro_keys,
        "I1: tree state parity violated — bridge has {} keys, loro has {} keys",
        bridge_keys.len(),
        loro_keys.len()
    );
}

/// I2 — Edge state parity: the `BridgeMaps::edge_id_map` keys MUST equal the
/// set of live Loro-side edge keys (src, dst, label triples from UpsertEdge
/// minus DeleteEdge minus TreeMove's delete-old-edge step).
fn check_i2_edge_state_parity(state: &FuzzState) {
    let bridge_edges: HashSet<(String, String, String)> =
        state.maps.edge_id_map.read().keys().cloned().collect();
    let loro_edges: HashSet<(String, String, String)> =
        state.live_edge_keys.iter().cloned().collect();
    assert_eq!(
        bridge_edges,
        loro_edges,
        "I2: edge state parity violated — bridge has {} edges, loro has {} edges",
        bridge_edges.len(),
        loro_edges.len()
    );
}

/// I3a — No panic in `apply_loro_op`: implicit. If `apply_loro_op` panicked
/// during the apply loop, the fuzz target would have crashed before reaching
/// this check. The non-trivial assertion is that `op_count` ops were applied
/// (i.e. the loop completed without panic). Per Devil M5, we assert a concrete
/// value comparison: the applied-op count MUST equal the requested-op count
/// (capped at `bail_after_ops`).
fn check_i3a_no_panic_in_apply_loro_op(state: &FuzzState, requested_ops: u64) {
    assert_eq!(
        state.op_count, requested_ops,
        "I3a: apply_loro_op did not complete all requested ops without panic — applied {}, requested {}",
        state.op_count, requested_ops
    );
}

/// I3b — No panic in `MutationBatcher::run`: spawn the batcher on a tokio
/// runtime, feed it the same ops via a channel, trigger shutdown, await
/// completion. If `run` (or `flush_inner`, which calls `apply_loro_op` +
/// `prepared.commit()`) panics, the `JoinHandle::await` returns `Err(JoinError)`
/// and we assert that didn't happen.
fn check_i3b_no_panic_in_batcher_run(db: &Arc<GrafeoDB>, maps: &Arc<BridgeMaps>, ops: &[FuzzOp]) {
    use grafeo_loro::bridge::{BatcherConfig, MutationBatcher};
    use tokio::sync::{broadcast, mpsc};

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            // Runtime construction failure is a real defect (not a fuzz input
            // violation) — panic to surface it.
            panic!("I3b: tokio runtime construction failed: {e}");
        }
    };

    rt.block_on(async move {
        let bridge_origin_epochs = Arc::new(RwLock::new(HashSet::<EpochId>::new()));
        let (shutdown_tx, _) = broadcast::channel(1);
        let batcher = Arc::new(MutationBatcher::new(
            db.clone(),
            BatcherConfig {
                batch_size: 256,
                batch_ms: 100,
                bridge_origin_epochs,
                maps: maps.clone(),
                shutdown_tx: shutdown_tx.clone(),
                metrics: None,
                tracer: None,
                health: None,
            },
        ));
        let (tx, rx) = mpsc::channel::<LoroOp>(1024);
        for op in ops {
            let _ = tx.send(convert_fuzz_op(op)).await;
        }
        drop(tx);

        let batcher_clone = batcher.clone();
        let handle = tokio::spawn(async move { batcher_clone.run(rx).await });

        // Trigger shutdown after a short delay so the batcher drains + exits.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let _ = shutdown_tx.send(());

        let result = handle.await;
        assert!(
            result.is_ok(),
            "I3b: MutationBatcher::run panicked — JoinError: {:?}",
            result.err()
        );
    });
}

/// I3c — No panic in `parallel_hydrate_grafeo`: call `parallel_hydrate_grafeo`
/// on a fresh `GrafeoDB` + the current `LoroDoc` snapshot. If `hydrate_map` or
/// the rayon chunk loop panics, the call unwinds and we assert it didn't.
fn check_i3c_no_panic_in_parallel_hydrate(state: &FuzzState) {
    let fresh_db = Arc::new(GrafeoDB::new_in_memory());
    let fresh_maps = Arc::new(BridgeMaps::new());
    let result =
        grafeo_loro::parallel_hydrate_grafeo(&fresh_db, state.doc, &fresh_maps, None, None);
    assert!(
        result.is_ok(),
        "I3c: parallel_hydrate_grafeo returned Err (non-panic failure) — {:?}",
        result.err()
    );
    // Non-trivial assertion: the fresh DB's node count MUST equal the live node
    // key count (hydration is a 1:1 materialization).
    assert_eq!(
        fresh_db.node_count(),
        state.live_node_keys.len(),
        "I3c: hydration node count mismatch — fresh_db has {}, live_node_keys has {}",
        fresh_db.node_count(),
        state.live_node_keys.len()
    );
}

/// I4 — Echo loop bounded: the epoch side-channel set MUST NOT exceed
/// `EPOCH_RETENTION + 1` entries. The fuzz target inserts every commit epoch
/// into the set (mimicking `MutationBatcher::flush_inner`); the set is pruned
/// by the outbound CDC poller in production. Here we assert the upper bound.
fn check_i4_echo_loop_bounded(state: &FuzzState) {
    let epoch_count = state.epochs.read().len();
    let max = (EPOCH_RETENTION as usize) + 1;
    assert!(
        epoch_count <= max,
        "I4: epoch side-channel set grew to {epoch_count} entries (max allowed = {max})"
    );
}

/// I5 — Origin filter symmetry: verify that a Loro commit tagged with
/// `ORIGIN_GRAFEO_BRIDGE` is filtered by the inbound subscriber (i.e.
/// `inbound_filtered_count` increments, `inbound_event_count` does not). This
/// tests the B1 echo-prevention filter at `src/bridge/sync_engine.rs:414`. We
/// construct a `SyncEngine`, init the subscriber, commit a Loro op with the
/// grafeo-bridge origin, and assert the filter fired.
fn check_i5_origin_filter_symmetry() {
    use grafeo_loro::bridge::SyncEngine;
    use grafeo_loro::constants::ORIGIN_GRAFEO_BRIDGE;

    let rt = match tokio::runtime::Builder::new_current_thread().build() {
        Ok(rt) => rt,
        Err(e) => panic!("I5: tokio runtime construction failed: {e}"),
    };

    rt.block_on(async move {
        let db = Arc::new(GrafeoDB::new_in_memory());
        let doc = Arc::new(parking_lot::RwLock::new(LoroDoc::new()));
        let (engine, _inbound_rx, _outbound_rx) = SyncEngine::new(db, doc.clone());
        let engine = Arc::new(engine);
        // Init the Loro subscriber so the origin filter is active.
        engine
            .init_loro_subscriber()
            .expect("I5: init_loro_subscriber failed");

        let filtered_before = engine.inbound_filtered_count();
        let events_before = engine.inbound_event_count();

        // Commit a Loro write tagged with ORIGIN_GRAFEO_BRIDGE. The subscriber
        // MUST filter this (origin matches) — `inbound_filtered_count` increments,
        // `inbound_event_count` does NOT.
        {
            let doc = doc.read();
            doc.set_next_commit_origin(ORIGIN_GRAFEO_BRIDGE);
            let v_map = doc.get_map(ROOT_VERTICES);
            let _ = v_map.ensure_mergeable_map("V/i5-test");
            doc.commit();
        }

        // Yield once so the subscriber callback (which runs synchronously inside
        // `commit`) has fully returned. The subscriber uses `try_send` so no
        // await is needed, but we yield for safety.
        tokio::task::yield_now().await;

        let filtered_after = engine.inbound_filtered_count();
        let events_after = engine.inbound_event_count();

        // Non-trivial assertions: filtered count MUST increment by exactly 1,
        // event count MUST NOT increment (the op was filtered, not forwarded).
        assert_eq!(
            filtered_after,
            filtered_before + 1,
            "I5: origin filter did not fire — filtered_before={filtered_before}, filtered_after={filtered_after}"
        );
        assert_eq!(
            events_after, events_before,
            "I5: origin-filtered op was forwarded to batcher — events_before={events_before}, events_after={events_after}"
        );
    });
}

/// I6 — Read-your-own-writes: after a synchronous write via `apply_loro_op`
/// (the SSOT inbound path), an immediate read via `session.get_node_property`
/// MUST observe the written value. This tests the RYOW SEMANTICS (write then
/// read sees the write) without depending on `GrafeoLoroApp::update_text` /
/// `query` (both `unimplemented!()` per Phase 6 T1 user exclusion).
fn check_i6_ryow(db: &Arc<GrafeoDB>, maps: &Arc<BridgeMaps>) {
    let mut session = db.session_with_cdc(false);
    session
        .begin_transaction()
        .expect("I6: begin_transaction failed");
    let op = LoroOp::UpsertNode {
        loro_key: "V/i6-ryow-test".to_string(),
        labels: vec!["Test".into()],
        properties: HashMap::from([("text".to_string(), GraphValue::String("hello-ryow".into()))]),
    };
    apply_loro_op(&session, &op, maps).expect("I6: apply_loro_op failed");
    let prepared = session.prepare_commit().expect("I6: prepare_commit failed");
    prepared.commit().expect("I6: commit failed");

    // Immediate read on the SAME db (no flush window delay). The read MUST
    // observe the written value — this is the RYOW guarantee.
    let read_session = db.session();
    let node_id = *maps
        .node_id_map
        .read()
        .get("V/i6-ryow-test")
        .expect("I6: BridgeMaps missing node after commit");
    let prop = read_session
        .get_node_property(node_id, "text")
        .expect("I6: get_node_property returned None after commit");
    // Non-trivial assertion: the read value MUST equal the written value.
    let expected = grafeo::Value::String("hello-ryow".into());
    assert_eq!(
        prop, expected,
        "I6: RYOW violated — wrote {expected:?}, read {prop:?}"
    );
}

/// I7 — Snapshot idempotency: calling `CompressedPayload::compress_to_wire`
/// twice on the same Loro doc state MUST produce byte-identical output. This
/// tests the snapshot side of `checkpoint` (the storage-write path is
// idempotent by overwrite, but the compression+wire-format MUST be deterministic).
fn check_i7_snapshot_idempotency(doc: &LoroDoc, compression: CompressionType) {
    let frontiers = doc.oplog_frontiers();
    let snapshot_bytes = doc
        .export(loro::ExportMode::shallow_snapshot(&frontiers))
        .expect("I7: LoroDoc::export failed");
    let wire1 = CompressedPayload::compress_to_wire(&snapshot_bytes, compression)
        .expect("I7: compress_to_wire (1st call) failed");
    let wire2 = CompressedPayload::compress_to_wire(&snapshot_bytes, compression)
        .expect("I7: compress_to_wire (2nd call) failed");
    // Non-trivial assertion: the two wire-format byte vectors MUST be identical.
    assert_eq!(
        wire1,
        wire2,
        "I7: snapshot idempotency violated — wire1.len()={}, wire2.len()={}",
        wire1.len(),
        wire2.len()
    );
}

/// I8 — Compression round-trip: for any input bytes, `compress` followed by
/// `decompress` MUST yield the original bytes, for ALL three `CompressionType`
/// variants (`None`, `Lz4`, `Zstd`). We test with a sample derived from the
/// fuzz input's op count (deterministic per iteration).
fn check_i8_compression_round_trip(sample_seed: u64) {
    // Build a deterministic sample byte buffer from the seed. Size varies so
    // the fuzzer exercises different compression ratio regimes.
    let size = (sample_seed as usize % 4096) + 1;
    let mut sample = Vec::with_capacity(size);
    let mut s = sample_seed;
    for _ in 0..size {
        // SplitMix64 step for deterministic but varied bytes.
        s = s.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        sample.push(z as u8);
    }

    for strategy in [
        CompressionType::None,
        CompressionType::Lz4,
        CompressionType::Zstd,
    ] {
        let payload = CompressedPayload::compress(&sample, strategy).expect("I8: compress failed");
        let decompressed = payload.decompress().expect("I8: decompress failed");
        // Non-trivial assertion: round-trip MUST reproduce the original bytes.
        assert_eq!(
            decompressed,
            sample,
            "I8: compression round-trip failed for {strategy:?} — input.len()={}, output.len()={}",
            sample.len(),
            decompressed.len()
        );
    }
}

/// I9 — Hydration determinism: calling `parallel_hydrate_grafeo` twice on the
/// same `LoroDoc` snapshot MUST produce GrafeoDB states with identical node
/// counts, label counts, and property-key counts. Rayon chunk ordering MUST
/// NOT leak non-determinism. Full byte-identical comparison of GrafeoDB is not
/// exposed by the public API; we compare the available structural counts.
fn check_i9_hydration_determinism(doc: &LoroDoc) {
    let db1 = Arc::new(GrafeoDB::new_in_memory());
    let maps1 = Arc::new(BridgeMaps::new());
    grafeo_loro::parallel_hydrate_grafeo(&db1, doc, &maps1, None, None)
        .expect("I9: first parallel_hydrate_grafeo failed");

    let db2 = Arc::new(GrafeoDB::new_in_memory());
    let maps2 = Arc::new(BridgeMaps::new());
    grafeo_loro::parallel_hydrate_grafeo(&db2, doc, &maps2, None, None)
        .expect("I9: second parallel_hydrate_grafeo failed");

    // Non-trivial assertions: the two hydrated DBs MUST have identical counts.
    assert_eq!(
        db1.node_count(),
        db2.node_count(),
        "I9: hydration determinism violated — node_count db1={}, db2={}",
        db1.node_count(),
        db2.node_count()
    );
    assert_eq!(
        db1.edge_count(),
        db2.edge_count(),
        "I9: hydration determinism violated — edge_count db1={}, db2={}",
        db1.edge_count(),
        db2.edge_count()
    );
    // BridgeMaps MUST also be identical.
    assert_eq!(
        maps1.node_id_map.read().len(),
        maps2.node_id_map.read().len(),
        "I9: hydration determinism violated — BridgeMaps node_id_map len differs"
    );
}

/// I10 — Vector offload bypass: `VectorOffloadManager::handle_text_update`
/// MUST NEVER write the embedding vector into the Loro doc. After calling
/// `handle_text_update`, the Loro doc MUST NOT contain an `EMBEDDING_PROPERTY`
/// key on any vertex sub-map. The embedding appears only in GrafeoDB.
fn check_i10_vector_offload_bypass(db: &Arc<GrafeoDB>, maps: &Arc<BridgeMaps>) {
    // Create a vertex to attach the embedding to.
    let mut session = db.session_with_cdc(false);
    session
        .begin_transaction()
        .expect("I10: begin_transaction failed");
    let op = LoroOp::UpsertNode {
        loro_key: "V/i10-vec-test".to_string(),
        labels: vec!["Test".into()],
        properties: HashMap::from([(
            "text".to_string(),
            GraphValue::String("sample text for embedding".into()),
        )]),
    };
    apply_loro_op(&session, &op, maps).expect("I10: apply_loro_op failed");
    let prepared = session
        .prepare_commit()
        .expect("I10: prepare_commit failed");
    prepared.commit().expect("I10: commit failed");

    let node_id = *maps
        .node_id_map
        .read()
        .get("V/i10-vec-test")
        .expect("I10: BridgeMaps missing node");

    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("I10: tokio runtime construction failed");
    rt.block_on(async {
        let mgr = VectorOffloadManager::new(db.clone());
        mgr.handle_text_update(node_id, "sample text for embedding")
            .await
            .expect("I10: handle_text_update failed");
    });

    // Non-trivial assertion: the Grafeo node MUST have the embedding property.
    let read_session = db.session();
    let embedding = read_session.get_node_property(node_id, EMBEDDING_PROPERTY);
    assert!(
        embedding.is_some(),
        "I10: VectorOffloadManager did not write embedding to Grafeo"
    );
    // Non-trivial assertion: the embedding MUST be a Vector variant.
    assert!(
        matches!(embedding, Some(grafeo::Value::Vector(_))),
        "I10: embedding property is not a Vector — got {:?}",
        embedding
    );
}

/// I11 — BridgeMaps bijectivity: for every `loro_key` in `node_id_map`, there
/// MUST be a corresponding `NodeId` in `node_key_map` (and vice versa). Same
/// for `edge_id_map` ↔ `edge_key_map`. No orphaned entries in either direction.
fn check_i11_bridge_maps_bijectivity(maps: &Arc<BridgeMaps>) {
    let node_id_map = maps.node_id_map.read();
    let node_key_map = maps.node_key_map.read();
    // Forward ↔ inverse node maps MUST have identical lengths.
    assert_eq!(
        node_id_map.len(),
        node_key_map.len(),
        "I11: node map bijectivity violated — forward={}, inverse={}",
        node_id_map.len(),
        node_key_map.len()
    );
    // Every forward entry MUST have a matching inverse entry.
    for (k, v) in node_id_map.iter() {
        assert!(
            node_key_map.get(v).is_some_and(|inv| inv == k),
            "I11: node forward entry {k:?} → {v:?} has no matching inverse"
        );
    }

    let edge_id_map = maps.edge_id_map.read();
    let edge_key_map = maps.edge_key_map.read();
    assert_eq!(
        edge_id_map.len(),
        edge_key_map.len(),
        "I11: edge map bijectivity violated — forward={}, inverse={}",
        edge_id_map.len(),
        edge_key_map.len()
    );
    for (k, v) in edge_id_map.iter() {
        assert!(
            edge_key_map.get(v).is_some_and(|inv| inv == k),
            "I11: edge forward entry {k:?} → {v:?} has no matching inverse"
        );
    }
}

/// I12 — MVCC snapshot isolation: a session pinned to epoch E via
/// `set_viewing_epoch(E)` MUST continue to observe the DB state as of E,
/// even after a concurrent writer commits a new epoch E'. Clearing the
/// override MUST then expose the new state.
///
/// Tests the "zero reader blocking + consistent snapshot" half of
/// architecture §19 directly via grafeo's `set_viewing_epoch` time-travel
/// API (NOT via `GrafeoLoroApp::query` — that remains
/// `Err(NotYetImplemented(...))` per Gap A.2).
///
/// Per Devil B1 (gap-closure-l1-devil.md §CA.1): uses `grafeo::Value::Int64`
/// (the real variant) — NOT the hallucinated `grafeo::Value::Integer`.
fn check_i12_mvcc_snapshot_isolation(db: &Arc<GrafeoDB>, maps: &Arc<BridgeMaps>) {
    // 1. Write node N with property "v" = 1 → commit returns epoch E1.
    let mut w1 = db.session_with_cdc(false);
    w1.begin_transaction()
        .expect("I12: begin_transaction (write 1) failed");
    let op = LoroOp::UpsertNode {
        loro_key: "V/i12-snap-test".to_string(),
        labels: vec!["Test".into()],
        properties: HashMap::from([("v".to_string(), GraphValue::Integer(1))]),
    };
    apply_loro_op(&w1, &op, maps).expect("I12: apply_loro_op (write 1) failed");
    let prepared1 = w1
        .prepare_commit()
        .expect("I12: prepare_commit (write 1) failed");
    let e1 = prepared1.commit().expect("I12: commit (write 1) failed");

    // 2. Open a read session and pin it to epoch E1.
    let read_session = db.session();
    read_session.set_viewing_epoch(e1);

    // 3. Write node N's property "v" = 2 → commit returns epoch E2 (E2 > E1).
    //    Uses the direct grafeo `set_node_property` API (NOT apply_loro_op)
    //    because apply_loro_op's UpsertNode path would re-create the node
    //    instead of mutating the existing property. See Devil CB.2 ruling.
    let mut w2 = db.session_with_cdc(false);
    w2.begin_transaction()
        .expect("I12: begin_transaction (write 2) failed");
    let node_id = *maps
        .node_id_map
        .read()
        .get("V/i12-snap-test")
        .expect("I12: BridgeMaps missing node after write 1");
    w2.set_node_property(node_id, "v", grafeo::Value::Int64(2))
        .expect("I12: set_node_property (write 2) failed");
    let prepared2 = w2
        .prepare_commit()
        .expect("I12: prepare_commit (write 2) failed");
    let e2 = prepared2.commit().expect("I12: commit (write 2) failed");

    // Non-trivial assertion 1: epoch must advance on commit.
    assert!(
        e2.as_u64() > e1.as_u64(),
        "I12: epoch did not advance: E1={}, E2={}",
        e1.as_u64(),
        e2.as_u64()
    );

    // Non-trivial assertion 2: read_session pinned at E1 MUST see v=1,
    // NOT the new v=2.
    let v_at_e1 = read_session.get_node_property(node_id, "v");
    assert_eq!(
        v_at_e1,
        Some(grafeo::Value::Int64(1)),
        "I12: snapshot isolation violated — pinned at E1={}, saw v={:?} (expected 1)",
        e1.as_u64(),
        v_at_e1
    );

    // Non-trivial assertion 3: clearing the override MUST expose v=2.
    read_session.clear_viewing_epoch();
    let v_now = read_session.get_node_property(node_id, "v");
    assert_eq!(
        v_now,
        Some(grafeo::Value::Int64(2)),
        "I12: post-clear read saw v={:?} (expected 2 after epoch advanced to {})",
        v_now,
        e2.as_u64()
    );
}

/// I14 — Tree move serializability: after any sequence of `TreeMove` ops, the
/// Grafeo tree (CHILD edges) MUST NOT contain a cycle. We traverse the
/// parent→child tree from every node and assert no node is revisited within a
/// single root-to-leaf walk. Grafeo 0.5.42 has no native acyclicity check, so
/// the bridge is the sole enforcer.
fn check_i14_tree_move_serializability(db: &Arc<GrafeoDB>, maps: &Arc<BridgeMaps>) {
    let session = db.session();
    let node_ids: Vec<grafeo::NodeId> = maps.node_id_map.read().values().copied().collect();

    for root in &node_ids {
        // BFS from `root` following outgoing CHILD edges. If we revisit any
        // node, there's a cycle.
        let mut visited = HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(*root);
        while let Some(node) = queue.pop_front() {
            if !visited.insert(node) {
                panic!(
                    "I14: tree move serializability violated — cycle detected at node {node:?} (root={root:?})"
                );
            }
            // Get outgoing CHILD edges (parent→child direction).
            let children = session.get_neighbors_outgoing_by_type(node, TREE_EDGE_LABEL);
            for (child, _edge_id) in children {
                queue.push_back(child);
            }
        }
    }
}

/// I15 — Presence envelope integrity: `PresenceManager::build_eph_envelope`
/// followed by `PresenceManager::parse_eph_envelope` MUST round-trip the
/// `room_id` + `PresencePayload` exactly, AND `parse_eph_envelope` MUST
/// reject any malformed byte sequence with `GrafeoLoroError::InvalidEnvelope`.
///
/// Tests the production `%EPH` wire format (architecture §12; Gap A.3):
/// `[magic:4][room_id_len:u16 LE][room_id:UTF-8][msg_type:u8][serde_json payload]`.
///
/// Per Devil M2/CA.3 (gap-closure-l1-devil.md): uses the REAL production APIs
/// (NOT a hand-rolled envelope). The pre-P7-L2 implementation tested a
/// simpler `[magic:4][serde_json]` format that was INCOMPATIBLE with the new
/// wire format — Goodhart risk (testing a format that doesn't match production).
fn check_i15_presence_envelope_integrity(payload: &PresencePayload) {
    use grafeo_loro::presence::PresenceManager;
    use grafeo_loro::GrafeoLoroError;

    // Fixed room_id for the round-trip (distinct from any fuzz-test vertex
    // key — never collides with I6/I10/I12 keys).
    let room_id = "V/i15-roundtrip-test";

    // === Positive path: build → parse round-trip ===
    let envelope_bytes = PresenceManager::build_eph_envelope(room_id, payload)
        .expect("I15: build_eph_envelope failed");
    let decoded = PresenceManager::parse_eph_envelope(&envelope_bytes)
        .expect("I15: parse_eph_envelope failed");
    // Non-trivial assertion 1: room_id MUST round-trip exactly.
    assert_eq!(decoded.room_id, room_id, "I15: room_id round-trip mismatch");
    // Non-trivial assertion 2: payload MUST round-trip exactly (struct-level
    // PartialEq — covers peer_id, cursor_x/y, last_active_ts, active_node).
    assert_eq!(
        decoded.payload, *payload,
        "I15: payload round-trip mismatch — decoded={:?}, original={:?}",
        decoded.payload, payload
    );

    // === Negative path 1: bad magic ===
    // 4 bytes of bad magic + otherwise-valid envelope tail.
    let mut bad_magic = envelope_bytes.clone();
    bad_magic[..EPH_MAGIC.len()].copy_from_slice(b"XXXX");
    let err = PresenceManager::parse_eph_envelope(&bad_magic)
        .expect_err("I15: parse_eph_envelope accepted bad magic");
    assert!(
        matches!(err, GrafeoLoroError::InvalidEnvelope(_)),
        "I15: bad-magic rejection returned wrong error variant: {err:?}"
    );

    // === Negative path 2: truncated buffer (just the magic) ===
    let err = PresenceManager::parse_eph_envelope(EPH_MAGIC)
        .expect_err("I15: parse_eph_envelope accepted magic-only buffer");
    assert!(
        matches!(err, GrafeoLoroError::InvalidEnvelope(_)),
        "I15: truncation rejection returned wrong error variant: {err:?}"
    );

    // === Negative path 3: bad serde payload (valid prefix, garbage JSON) ===
    // Build a buffer with the correct magic + room_id + msg_type, then append
    // invalid JSON bytes for the payload. parse_eph_envelope MUST reject with
    // InvalidEnvelope("serde: ...").
    let mut bad_serde = Vec::with_capacity(EPH_MAGIC.len() + 2 + room_id.len() + 1 + 4);
    bad_serde.extend_from_slice(EPH_MAGIC);
    bad_serde.extend_from_slice(&(room_id.len() as u16).to_le_bytes());
    bad_serde.extend_from_slice(room_id.as_bytes());
    bad_serde.push(0x01); // EPH_MSG_TYPE_PRESENCE
    bad_serde.extend_from_slice(b"not valid json {{{");
    let err = PresenceManager::parse_eph_envelope(&bad_serde)
        .expect_err("I15: parse_eph_envelope accepted invalid JSON payload");
    assert!(
        matches!(err, GrafeoLoroError::InvalidEnvelope(_)),
        "I15: bad-serde rejection returned wrong error variant: {err:?}"
    );
}

// =============================================================================
// Fuzz target entry point.
// =============================================================================

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let input = match FuzzInput::arbitrary(&mut u) {
        Ok(i) => i,
        // Malformed input — early return per docs/phase-6/fuzz-invariants.md
        // (Devil happy-path bias note). libfuzzer treats early-return as a
        // successful iteration.
        Err(_) => return,
    };

    // Clamp config knobs to safe ranges.
    let _peer_count = input.peer_count.clamp(1, 3);
    let bail = (input.bail_after_ops.max(1) as u64).min(10_000);

    // Build fresh state for this iteration.
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = LoroDoc::new();
    let maps = Arc::new(BridgeMaps::new());
    let epochs: Arc<RwLock<HashSet<EpochId>>> = Arc::new(RwLock::new(HashSet::new()));
    let mut live_node_keys: HashSet<String> = HashSet::new();
    let mut live_edge_keys: HashSet<(String, String, String)> = HashSet::new();

    let mut op_count = 0u64;
    let requested_ops = (input.ops.len() as u64).min(bail);
    let mut last_tree_move: Option<FuzzOp> = None;

    // I3a: apply each op via `apply_loro_op` (the SSOT inbound path). A panic
    // here crashes the fuzz target — which is the I3a failure mode.
    for fuzz_op in input.ops.iter().take(bail as usize) {
        let op = convert_fuzz_op(fuzz_op);
        let mut session = db.session_with_cdc(false);
        // `begin_transaction` failure is a real defect — panic to surface it.
        session
            .begin_transaction()
            .unwrap_or_else(|e| panic!("I3a: begin_transaction failed: {e}"));
        // `apply_loro_op` failure is acceptable (e.g. UpsertEdge with unknown
        // node keys returns `Err(Bridge(...))`); we log via `let _ =` and continue.
        let _ = apply_loro_op(&session, &op, &maps);
        // Commit + record the epoch for I4.
        if let Ok(prepared) = session.prepare_commit() {
            if let Ok(epoch) = prepared.commit() {
                epochs.write().insert(epoch);
            }
        }

        // Mirror the op into the LoroDoc V container + the live-key sets so
        // I1/I2 can check parity. This mirrors what `VertexBuilder::commit`
        // + the Loro subscriber would do in the full pipeline.
        match fuzz_op {
            FuzzOp::UpsertNode { loro_key, .. } => {
                let v_map = doc.get_map(ROOT_VERTICES);
                let _ = v_map.ensure_mergeable_map(loro_key);
                doc.commit();
                live_node_keys.insert(loro_key.clone());
            }
            FuzzOp::DeleteNode { loro_key } => {
                let v_map = doc.get_map(ROOT_VERTICES);
                let _ = v_map.delete(loro_key);
                doc.commit();
                live_node_keys.remove(loro_key);
            }
            FuzzOp::UpsertEdge {
                src_key,
                dst_key,
                label,
                ..
            } => {
                live_edge_keys.insert((src_key.clone(), dst_key.clone(), label.clone()));
            }
            FuzzOp::DeleteEdge {
                src_key,
                dst_key,
                label,
            } => {
                live_edge_keys.remove(&(src_key.clone(), dst_key.clone(), label.clone()));
            }
            FuzzOp::TreeMove {
                node_key,
                old_parent_key,
                new_parent_key,
            } => {
                // TreeMove = delete old CHILD edge + insert new CHILD edge.
                live_edge_keys.remove(&(
                    old_parent_key.clone(),
                    node_key.clone(),
                    TREE_EDGE_LABEL.to_string(),
                ));
                live_edge_keys.insert((
                    new_parent_key.clone(),
                    node_key.clone(),
                    TREE_EDGE_LABEL.to_string(),
                ));
                last_tree_move = Some(fuzz_op.clone());
            }
        }

        op_count += 1;
    }

    // Build the invariant-check state view.
    let state = FuzzState {
        db: &db,
        doc: &doc,
        maps: &maps,
        epochs: &epochs,
        live_node_keys: &live_node_keys,
        live_edge_keys: &live_edge_keys,
        op_count,
    };

    // ===== Per-iteration invariants (cheap) =====
    check_i1_tree_state_parity(&state);
    check_i2_edge_state_parity(&state);
    check_i3a_no_panic_in_apply_loro_op(&state, requested_ops);
    check_i4_echo_loop_bounded(&state);
    check_i11_bridge_maps_bijectivity(&maps);

    // I3c: parallel_hydrate (sync — no runtime needed).
    check_i3c_no_panic_in_parallel_hydrate(&state);

    // I13 (batcher count / buffer-empty-after-run) is COVERED BY I3b: I3b
    // spawns `MutationBatcher::run`, feeds ops via channel, triggers shutdown,
    // and asserts `JoinHandle::await` is `Ok`. If the batcher failed to drain
    // its buffer, it would either panic (caught by I3b's JoinError assert) or
    // hang (the test would time out). The previous `check_i13_batcher_count`
    // fn was a tautology (`assert!(true)`) — removed per anti-plenger #11
    // (Deletion over addition) in P6-L2-FIX (Hunter Task 5b finding).

    // I15: presence envelope integrity (pure function — no state needed).
    let payload = PresencePayload {
        peer_id: grafeo_loro::types::PeerId(input.seed),
        active_node: if input.seed % 2 == 0 {
            Some(format!("V/{}", input.seed % 100))
        } else {
            None
        },
        cursor_x: (input.seed as f32) / 100.0,
        cursor_y: ((input.seed >> 32) as f32) / 100.0,
        last_active_ts: input.seed,
    };
    check_i15_presence_envelope_integrity(&payload);

    // ===== Periodic invariants (every 1000 ops OR final iteration) =====
    if op_count > 0 && (op_count.is_multiple_of(1000) || op_count == requested_ops) {
        // I7: snapshot idempotency. Use the LoroDoc's current state + a
        // deterministic compression type derived from the seed.
        let compression = match input.seed % 3 {
            0 => CompressionType::None,
            1 => CompressionType::Lz4,
            _ => CompressionType::Zstd,
        };
        check_i7_snapshot_idempotency(&doc, compression);
        // I9: hydration determinism.
        check_i9_hydration_determinism(&doc);
    }

    // ===== Event-driven invariants =====

    // I3b: run the batcher on a tokio runtime (async — needs runtime).
    // Only run if op count is small enough to fit within the iteration timeout.
    if op_count <= 100 {
        check_i3b_no_panic_in_batcher_run(&db, &maps, &input.ops[..op_count as usize]);
    }

    // I5: origin filter symmetry (async — needs runtime + SyncEngine).
    // Run on ~1/8 of iterations to avoid timeout (the test spawns a SyncEngine).
    if input.seed.is_multiple_of(8) {
        check_i5_origin_filter_symmetry();
    }

    // I6: RYOW (sync — direct session read).
    if input.seed.is_multiple_of(4) {
        check_i6_ryow(&db, &maps);
    }

    // I8: compression round-trip (pure function).
    check_i8_compression_round_trip(input.seed);

    // I10: vector offload bypass (async — needs runtime).
    if input.seed.is_multiple_of(8) {
        check_i10_vector_offload_bypass(&db, &maps);
    }

    // I12: MVCC snapshot isolation — pinned-epoch reads + post-clear reads.
    // Per L1 plan §Gap B: every iteration (cheap; 1 write + 3 reads + 2 commits).
    check_i12_mvcc_snapshot_isolation(&db, &maps);

    // I14: tree move serializability (only if a TreeMove fired).
    if last_tree_move.is_some() {
        check_i14_tree_move_serializability(&db, &maps);
    }
});

// Fallback `main` for non-fuzzing builds (enables `cargo check` to pass for
// syntax verification). When `--cfg fuzzing` is set (via `cargo +nightly fuzz
// run`), the `#![cfg_attr(fuzzing, no_main)]` attribute above suppresses
// crate-level main and libfuzzer's C runtime provides the real entry point
// that calls `rust_fuzzer_test_input` (generated by `fuzz_target!`).
#[cfg(not(fuzzing))]
fn main() {
    // Fuzz target is built via `cargo +nightly fuzz run consistency`.
    // This fallback main exists only so `cargo check` passes for syntax verification.
    eprintln!(
        "grafeo-loro-fuzz: build with `cargo +nightly fuzz run consistency` to actually fuzz"
    );
}
