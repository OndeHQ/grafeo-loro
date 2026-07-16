//! Criterion benchmarks for grafeo-loro core operations (issue #1 item 11).
//!
//! Four benchmark groups measuring the four hot paths mandated by issue #1:
//!
//! 1. `cold_start` — construct a `GrafeoLoroApp` over `InMemoryStorage`
//!    (builder validation + `SyncEngine::with_telemetry` + `spawn_all`).
//! 2. `batch_flush` — push 100 ops through `apply_node_batch` (FFI hot path).
//! 3. `hydration` — hydrate 1000 vertices from a Loro doc via the serial
//!    `hydrate_grafeo` (no `parallel` feature → rayon-free path that is also
//!    the WASM-safe path).
//! 4. `tree_ops` — create 100 tree nodes via `LoroOp::UpsertNode` and move 50
//!    of them via `LoroOp::TreeMove`.
//!
//! # Run
//!
//! Native:
//! ```bash
//! cargo bench --features full
//! ```
//!
//! WASM (browser; see `benches/README.md` for the full runbook + known
//! limitations):
//! ```bash
//! cargo bench --target wasm32-unknown-unknown --features full,wasm
//! ```
//!
//! # Feature gating
//!
//! The benches are gated by `feature = "full"` (matches the `[[bench]]`
//! `required-features` in `Cargo.toml`). `full` pulls in `grafeo`, `batcher`,
//! `tree`, `telemetry`, `storage`, `serde`, `compression`, `parallel`.
//!
//! `grafeo` is native-only today (0.5.42 has not been ported to
//! `wasm32-unknown-unknown`). The bench file is therefore additionally gated
//! by `not(target_family = "wasm")` so `cargo build --target wasm32-unknown-
//! unknown --features full --benches` does not fail inside this file — it
//! fails earlier at the `grafeo` dependency (documented in
//! `benches/README.md` as a known limitation). When grafeo gains WASM
//! support, drop the `target_family = "wasm"` gate and the benches will
//! compile for WASM unchanged.

#![cfg(feature = "full")]
// Native-only gate — see module doc-comment. Removing this gate is the only
// change needed to enable WASM bench builds once grafeo supports WASM.
#![cfg(not(target_family = "wasm"))]

use std::collections::HashMap;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use loro::LoroDoc;
use lorosurgeon::{Reconcile, RootReconciler};
use tokio::runtime::Runtime;

use grafeo_loro::constants::ROOT_VERTICES;
use grafeo_loro::schema::VertexEntity;
use grafeo_loro::types::{LoroOp, LoroProperty};
use grafeo_loro::{
    apply_node_batch, hydrate_grafeo, BridgeMaps, CompressionType, GrafeoLoroApp, InMemoryStorage,
    NodeOp, NodeValue, SsotMode,
};

// ============================================================================
// Constants — keep the four bench groups deterministic + comparable across
// native vs WASM runs.
// ============================================================================

/// Number of ops pushed through `apply_node_batch` per iteration.
const BATCH_FLUSH_OPS: usize = 100;

/// Number of vertices reconciled into the LoroDoc fixture for `hydration`.
const HYDRATION_VERTEX_COUNT: usize = 1000;

/// Number of tree nodes created in `tree_ops`.
const TREE_CREATE_COUNT: usize = 100;

/// Number of tree nodes moved in `tree_ops` (subset of `TREE_CREATE_COUNT`).
const TREE_MOVE_COUNT: usize = 50;

// ============================================================================
// Shared helpers
// ============================================================================

/// Reconcile one `VertexEntity` into `doc.get_map("V")[loro_key]` as a
/// `Container::Map` (the shape `hydrate_grafeo` expects). Mirrors the
/// fixture builder in `tests/unit/parallel_hydrate.rs`.
fn reconcile_vertex(doc: &LoroDoc, loro_key: &str, entity: &VertexEntity) {
    let v_map = doc.get_map(ROOT_VERTICES);
    let node_map = v_map
        .ensure_mergeable_map(loro_key)
        .expect("ensure_mergeable_map");
    entity
        .reconcile(RootReconciler::new(node_map))
        .expect("reconcile VertexEntity");
}

/// Build a `VertexEntity` with 2 labels + 3 mixed-type properties + empty
/// description (the Loro-only field, kept empty to match the hydrate path).
fn sample_vertex(idx: usize) -> VertexEntity {
    let mut props = HashMap::new();
    props.insert(
        "name".to_string(),
        LoroProperty::String(format!("node-{idx}")),
    );
    props.insert("age".to_string(), LoroProperty::Integer(idx as i64));
    props.insert("active".to_string(), LoroProperty::Bool(idx % 2 == 0));
    VertexEntity {
        labels: vec!["Item".to_string(), "Bench".to_string()],
        properties: props,
        description: String::new(),
    }
}

// ============================================================================
// 1. cold_start — construct GrafeoLoroApp with InMemoryStorage
// ============================================================================

/// Measure the cost of building a fresh `GrafeoLoroApp` from scratch: builder
/// validation, `GrafeoDB::new_in_memory`, `LoroDoc::new`, telemetry handles,
/// `SyncEngine::with_telemetry`, and `spawn_all`.
///
/// Setup (excluded from timing): construct the `InMemoryStorage`.
/// Measurement: `GrafeoLoroAppBuilder::build().await` (drives the tokio
/// runtime via `block_on`).
fn bench_cold_start(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime for cold_start");
    c.bench_function("cold_start", |b| {
        b.iter_batched(
            || Arc::new(InMemoryStorage::new()),
            |storage| {
                let app = rt
                    .block_on(async {
                        GrafeoLoroApp::builder()
                            .ssot_mode(SsotMode::Loro)
                            .compression(CompressionType::Zstd)
                            .storage(storage)
                            .build()
                            .await
                    })
                    .expect("GrafeoLoroApp::build");
                // Drop the app synchronously inside the measurement closure
                // so the measured time includes the full build (spawn_all
                // returns when workers are spawned, not when they exit).
                black_box(app);
            },
            BatchSize::SmallInput,
        );
    });
}

// ============================================================================
// 2. batch_flush — push 100 ops through apply_node_batch
// ============================================================================

/// Measure the FFI hot-path cost of bulk-applying 100 `NodeOp`s through
/// `apply_node_batch`. Each `NodeOp` becomes a `LoroOp::UpsertNode` via the
/// `From<NodeOp> for LoroOp` impl, then dispatches through `apply_loro_op`
/// (which calls `Session::create_node_with_props`).
///
/// Setup (excluded): fresh `GrafeoDB`, fresh `BridgeMaps`, 100 `NodeOp`s
/// pre-built, session + `begin_transaction`.
/// Measurement: `apply_node_batch(&session, &ops, &maps)` + `prepare_commit`
/// + `commit` (the full "flush" semantics — apply + commit).
fn bench_batch_flush(c: &mut Criterion) {
    c.bench_function("batch_flush", |b| {
        b.iter_batched(
            || {
                // Pre-build the 100 NodeOps (zero-cost borrow of static
                // strings — keeps the FFI shape realistic).
                let labels_static: &'static [&'static str] =
                    Box::leak(vec!["Bench"].into_boxed_slice());
                let prop_keys_static: &'static [&'static str] =
                    Box::leak(vec!["idx"].into_boxed_slice());
                let ops: Vec<NodeOp<'static>> = (0..BATCH_FLUSH_OPS)
                    .map(|i| {
                        // Leak the format!'d string per iteration setup —
                        // criterion setup runs once per measurement, so this
                        // is bounded by the iteration count, not the op
                        // count. Keeps `NodeOp` borrows `'static` so the
                        // measurement closure can move them by value.
                        let key: &'static str =
                            Box::leak(format!("bench-node-{i}").into_boxed_str());
                        let vals: Vec<NodeValue<'static>> = vec![NodeValue::Integer(i as i64)];
                        NodeOp {
                            loro_key: key,
                            labels: labels_static,
                            property_count: 1,
                            property_keys: prop_keys_static,
                            property_values: Box::leak(vals.into_boxed_slice()),
                        }
                    })
                    .collect();

                // Fresh GrafeoDB + BridgeMaps so each iteration measures a
                // clean bulk-apply (no pre-existing node idempotency hits).
                let db = Arc::new(grafeo::GrafeoDB::new_in_memory());
                let maps = Arc::new(BridgeMaps::new());
                let mut session = db.session_with_cdc(false);
                session.begin_transaction().expect("begin_transaction");
                (db, maps, session, ops)
            },
            |(db, maps, mut session, ops)| {
                apply_node_batch(&session, &ops, &maps).expect("apply_node_batch");
                let prepared = session.prepare_commit().expect("prepare_commit");
                prepared.commit().expect("commit");
                // Keep handles alive for the duration of the measurement.
                black_box((db, maps, session, ops));
            },
            // Setup is moderately heavy (allocates 100 ops + opens a
            // GrafeoDB); LargeInput keeps the iteration count reasonable.
            BatchSize::LargeInput,
        );
    });
}

// ============================================================================
// 3. hydration — hydrate 1000 vertices from a Loro doc (serial)
// ============================================================================

/// Measure the serial cold-boot hydration path: rebuild Grafeo indexes from
/// a Loro doc containing 1000 vertices. Uses `hydrate_grafeo` (NOT
/// `parallel_hydrate_grafeo`) so the bench reflects the WASM-safe path.
///
/// Setup (excluded): build a `LoroDoc` with 1000 `VertexEntity`s reconciled
/// into the `V` root map; fresh `GrafeoDB` + fresh `BridgeMaps`.
/// Measurement: `hydrate_grafeo(&db, &doc, &maps, None, None)`.
fn bench_hydration(c: &mut Criterion) {
    c.bench_function("hydration", |b| {
        b.iter_batched(
            || {
                // Build the LoroDoc fixture with 1000 vertices. This is the
                // cold-boot input shape — `V` root map with each vertex as
                // a nested `LoroMap`.
                let doc = LoroDoc::new();
                for i in 0..HYDRATION_VERTEX_COUNT {
                    let entity = sample_vertex(i);
                    let key = format!("v-{i}");
                    reconcile_vertex(&doc, &key, &entity);
                }
                doc.commit();

                // Fresh DB + maps — `hydrate_grafeo` writes here.
                let db = Arc::new(grafeo::GrafeoDB::new_in_memory());
                let maps = Arc::new(BridgeMaps::new());
                (doc, db, maps)
            },
            |(doc, db, maps)| {
                // Serial hydrate — no `parallel` feature flag branch. Pass
                // `None` for metrics + tracer (the bench is not telemetry-
                // instrumented; the `Option` slots exist because `full`
                // enables the `telemetry` feature).
                hydrate_grafeo(&db, &doc, &maps, None, None).expect("hydrate_grafeo");
                black_box((doc, db, maps));
            },
            // Setup is very heavy (1000 reconciliations + a fresh GrafeoDB).
            // LargeInput is required — SmallInput would spend most of the
            // bench wall-clock in setup.
            BatchSize::LargeInput,
        );
    });
}

// ============================================================================
// 4. tree_ops — create 100 tree nodes + move 50 of them
// ============================================================================

/// Measure tree-as-graph operations through the bridge: create 100 nodes via
/// `LoroOp::UpsertNode`, attach all 100 under `root1` via `LoroOp::TreeMove`,
/// then move the first 50 to `root2` via another `LoroOp::TreeMove` (which
/// exercises the delete-old-edge + insert-new-edge path in `apply_tree_move`).
///
/// Setup (excluded): fresh `GrafeoDB` + fresh `BridgeMaps` + session with
/// `root1` and `root2` already created (so the measurement is purely the
/// 100 creates + 50 moves).
/// Measurement: `begin_transaction` + 100 `UpsertNode` + 100 `TreeMove`
/// (attach to root1) + 50 `TreeMove` (move to root2) + `commit`.
fn bench_tree_ops(c: &mut Criterion) {
    c.bench_function("tree_ops", |b| {
        b.iter_batched(
            || {
                let db = Arc::new(grafeo::GrafeoDB::new_in_memory());
                let maps = Arc::new(BridgeMaps::new());

                // Pre-create root1 + root2 so the measured path can attach
                // nodes under them (TreeMove looks up both endpoints in
                // `node_id_map`).
                {
                    let mut session = db.session_with_cdc(false);
                    session
                        .begin_transaction()
                        .expect("begin_transaction (setup)");
                    for root_key in &["root1", "root2"] {
                        let op = LoroOp::UpsertNode {
                            loro_key: root_key.to_string(),
                            labels: vec!["Root".to_string()],
                            properties: HashMap::new(),
                        };
                        grafeo_loro::bridge::apply_loro_op(&session, &op, &maps)
                            .expect("create root");
                    }
                    let prepared = session.prepare_commit().expect("prepare_commit (setup)");
                    prepared.commit().expect("commit (setup)");
                }

                let session = db.session_with_cdc(false);
                (db, maps, session)
            },
            |(db, maps, mut session)| {
                session.begin_transaction().expect("begin_transaction");

                // 100 creates — each becomes a tree node (no parent yet).
                for i in 0..TREE_CREATE_COUNT {
                    let op = LoroOp::UpsertNode {
                        loro_key: format!("n-{i}"),
                        labels: vec!["Node".to_string()],
                        properties: HashMap::new(),
                    };
                    grafeo_loro::bridge::apply_loro_op(&session, &op, &maps).expect("create node");
                }

                // 100 attaches — link each node under `root1`. This is the
                // initial `TreeMove` (delete-old-edge is a no-op because
                // the edge does not exist yet; insert-new-edge fires).
                for i in 0..TREE_CREATE_COUNT {
                    let op = LoroOp::TreeMove {
                        node_key: format!("n-{i}"),
                        old_parent_key: String::new(), // no prior parent
                        new_parent_key: "root1".to_string(),
                    };
                    grafeo_loro::bridge::apply_loro_op(&session, &op, &maps)
                        .expect("attach to root1");
                }

                // 50 moves — reparent the first 50 from `root1` to `root2`.
                // This exercises both the delete-old-edge AND insert-new-
                // edge branches of `apply_tree_move`.
                for i in 0..TREE_MOVE_COUNT {
                    let op = LoroOp::TreeMove {
                        node_key: format!("n-{i}"),
                        old_parent_key: "root1".to_string(),
                        new_parent_key: "root2".to_string(),
                    };
                    grafeo_loro::bridge::apply_loro_op(&session, &op, &maps)
                        .expect("move to root2");
                }

                let prepared = session.prepare_commit().expect("prepare_commit");
                prepared.commit().expect("commit");
                black_box((db, maps, session));
            },
            BatchSize::LargeInput,
        );
    });
}

// ============================================================================
// Criterion entry points
// ============================================================================

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = bench_cold_start, bench_batch_flush, bench_hydration, bench_tree_ops
}

criterion_main!(benches);
