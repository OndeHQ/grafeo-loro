//! Phase 3 Task 2 tests: `hydration::parallel::parallel_hydrate_grafeo`.
//!
//! All 7 functional tests are un-ignored and run by default. The 1 benchmark
//! (`parallel_hydrate_10k_nodes_under_500ms`) stays `#[ignore]`'d — run it
//! manually with `--release --ignored`. The spec validation gate is
//! `docs/implementation-plan.md:78`.
//!
//! # Verified API surface (cheat sheet)
//!
//! - `LoroDoc::get_map("V") -> LoroMap` — root vertices map
//!   (`loro-1.13.6/src/lib.rs:489`).
//! - `LoroMap::keys() -> impl Iterator<Item = InternalString>` — collect into
//!   `Vec<String>` for `rayon::par_chunks` (`loro-1.13.6/src/lib.rs:2315`).
//! - `LoroMap::get(&str) -> Option<ValueOrContainer>` — read each vertex
//!   sub-map; unwrap via `ValueOrContainer::Container(Container::Map(map))`
//!   (`loro-1.13.6/src/lib.rs:2150`, `:3813`).
//! - `LoroMap::ensure_mergeable_map(&str) -> LoroResult<LoroMap>` — get-or-create
//!   a nested map child at the given key (`loro-1.13.6/src/lib.rs:2247`).
//! - `VertexEntity::hydrate_map(&LoroMap) -> Result<VertexEntity, HydrateError>` —
//!   SSOT read path (`lorosurgeon-0.2.1/src/hydrate.rs:127`).
//! - `RootReconciler::new(LoroMap) -> Self` (`lorosurgeon-0.2.1/src/reconcile.rs:298`)
//!   + `entity.reconcile(reconciler)` writes the entity into the LoroMap.
//! - `GrafeoDB::node_count() -> usize` — total live node count
//!   (`grafeo-engine-0.5.42/src/database/admin.rs:14`).
//! - `db.session().get_node(NodeId) -> Option<Node>` — read a node back
//!   (`grafeo-engine-0.5.42/src/session/mod.rs:5138`); `Node::labels` +
//!   `Node::properties` for assertions.
//! - `apply_loro_op(&Session, &LoroOp, &BridgeMaps) -> Result<()>` — SSOT
//!   "lookup-or-create + insert binding" (`src/bridge/grafeo_tx.rs:86`).
//! - `BridgeMaps::node_id_map: RwLock<HashMap<String, NodeId>>` — read-back
//!   for `loro_key → NodeId` binding (`src/bridge/grafeo_tx.rs:28`).
//!
//! # Edge cases (anti-happy-path)
//!
//! - Empty `LoroDoc` (no `V` map entries) → Ok, zero nodes created.
//! - Single vertex with no properties → Ok, node created with empty prop map.
//! - Vertex sub-map is not a `Container::Map` (e.g. a `Container::List`) →
//!   `Err(GrafeoLoroError::Bridge(...))` (originally specified as Binary
//!   rejection; changed per L3 — see `parallel_hydrate_rejects_non_map_container`).
//! - 300 vertices with `DEFAULT_CHUNK_SIZE = 256` → 2 chunks (256 + 44); all
//!   300 must commit (no chunk lost on Rayon split).
//! - `VertexEntity::description` is Loro-only (`src/app.rs:201`) — MUST NOT
//!   appear in Grafeo properties post-hydrate (DEVIL M4).

use std::collections::HashMap;
use std::sync::Arc;

use grafeo::GrafeoDB;
use grafeo_loro::constants::ROOT_VERTICES;
use grafeo_loro::hydration::parallel_hydrate_grafeo;
use grafeo_loro::schema::VertexEntity;
use grafeo_loro::types::values::GraphValue;
use grafeo_loro::types::LoroProperty;
use grafeo_loro::BridgeMaps;
use loro::LoroDoc;
use lorosurgeon::{Reconcile, RootReconciler};

/// Insert `entity` into `doc.get_map("V")[loro_key]` as a `Container::Map`
/// (the cold-boot read path expects `Container::Map`, not `LoroValue::Map`).
/// Mirrors `VertexBuilder::commit` steps 3 (`app.rs:416-425`) without the
/// Grafeo write — hydration tests need pure-Loro fixtures so the SUT
/// (`parallel_hydrate_grafeo`) owns the Grafeo write.
fn reconcile_vertex_into_loro(doc: &LoroDoc, loro_key: &str, entity: &VertexEntity) {
    let v_map = doc.get_map(ROOT_VERTICES);
    let node_map = v_map
        .ensure_mergeable_map(loro_key)
        .expect("ensure_mergeable_map");
    entity
        .reconcile(RootReconciler::new(node_map))
        .expect("reconcile VertexEntity");
    doc.commit();
}

/// Build a `VertexEntity` with the given labels + properties (no description).
fn vertex(labels: Vec<String>, properties: HashMap<String, LoroProperty>) -> VertexEntity {
    VertexEntity {
        labels,
        properties,
        description: String::new(),
    }
}

/// Read a single node back from Grafeo and assert its labels + properties.
fn assert_grafeo_node(
    db: &GrafeoDB,
    node_id: grafeo::NodeId,
    expected_labels: &[&str],
    expected_props: &[(&str, GraphValue)],
) {
    let session = db.session();
    let node = session
        .get_node(node_id)
        .unwrap_or_else(|| panic!("grafeo should have node {node_id:?}"));
    for lbl in expected_labels {
        assert!(
            node.has_label(lbl),
            "node {node_id:?} missing label {lbl:?}; actual={:?}",
            node.labels
        );
    }
    assert_eq!(
        node.labels.len(),
        expected_labels.len(),
        "node {node_id:?} label count mismatch; actual={:?}",
        node.labels
    );
    for (k, expected_v) in expected_props {
        let actual = node
            .get_property(k)
            .unwrap_or_else(|| panic!("node {node_id:?} missing property {k:?}"));
        let expected_gv = grafeo_loro::types::values::gval_to_grafeo_value(expected_v.clone());
        assert_eq!(
            actual, &expected_gv,
            "node {node_id:?} property {k:?} mismatch"
        );
    }
    assert_eq!(
        node.properties.len(),
        expected_props.len(),
        "node {node_id:?} property count mismatch; actual={:?}",
        node.properties
    );
}

/// Empty `LoroDoc` (no `ROOT_VERTICES` map entries) → `parallel_hydrate_grafeo`
/// returns `Ok(())` and creates zero Grafeo nodes. Anti-happy-path baseline:
/// the empty-chunk edge case must not panic or no-op silently with stale
/// `BridgeMaps` state.
#[test]
fn parallel_hydrate_empty_doc_no_op() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = LoroDoc::new();
    let maps = BridgeMaps::new();

    let result = parallel_hydrate_grafeo(&db, &doc, &maps, None, None);
    assert!(
        result.is_ok(),
        "empty-doc hydrate should be Ok, got {result:?}"
    );
    assert_eq!(
        db.node_count(),
        0,
        "no nodes should exist after empty hydrate"
    );
    assert!(
        maps.node_id_map.read().is_empty(),
        "BridgeMaps should have zero bindings after empty hydrate"
    );
}

/// Single-vertex roundtrip: reconcile one `VertexEntity` into `doc.get_map("V")`
/// via `lorosurgeon::RootReconciler`, then call `parallel_hydrate_grafeo` and
/// verify exactly one Grafeo node exists with matching labels + properties
/// AND that `BridgeMaps::node_id_map` contains the `loro_key → NodeId` binding.
#[test]
fn parallel_hydrate_single_vertex_roundtrip() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = LoroDoc::new();
    let maps = BridgeMaps::new();

    let entity = vertex(
        vec!["Person".into()],
        HashMap::from([
            ("name".into(), LoroProperty::String("Alice".into())),
            ("age".into(), LoroProperty::Integer(30)),
        ]),
    );
    let loro_key = "V/1";
    reconcile_vertex_into_loro(&doc, loro_key, &entity);

    let result = parallel_hydrate_grafeo(&db, &doc, &maps, None, None);
    assert!(
        result.is_ok(),
        "single-vertex hydrate should be Ok, got {result:?}"
    );

    assert_eq!(db.node_count(), 1, "exactly 1 node should exist");
    let node_id = *maps
        .node_id_map
        .read()
        .get(loro_key)
        .unwrap_or_else(|| panic!("BridgeMaps missing binding for {loro_key:?}"));
    assert_grafeo_node(
        &db,
        node_id,
        &["Person"],
        &[
            ("name", GraphValue::String("Alice".into())),
            ("age", GraphValue::Integer(30)),
        ],
    );
}

/// Chunk-size boundary: 300 vertices with `DEFAULT_CHUNK_SIZE = 256` must
/// produce 2 chunks (256 + 44). All 300 nodes must be created (no chunk lost
/// on Rayon split). Asserts both the chunk-count boundary (256/300 split) and
/// the total node count (300, not 256).
#[test]
fn parallel_hydrate_multi_chunk_respects_chunk_size() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = LoroDoc::new();
    let maps = BridgeMaps::new();

    for i in 0..300 {
        let entity = vertex(
            vec!["Node".into()],
            HashMap::from([("idx".into(), LoroProperty::Integer(i as i64))]),
        );
        let key = format!("V/{i}");
        reconcile_vertex_into_loro(&doc, &key, &entity);
    }

    let result = parallel_hydrate_grafeo(&db, &doc, &maps, None, None);
    assert!(
        result.is_ok(),
        "300-vertex hydrate should be Ok, got {result:?}"
    );

    assert_eq!(
        db.node_count(),
        300,
        "all 300 nodes must commit across 2 chunks (256 + 44)"
    );
    assert_eq!(
        maps.node_id_map.read().len(),
        300,
        "BridgeMaps must have 300 bindings"
    );
}

/// Property-type preservation: a vertex carrying `Bool`/`Integer`/`Float`/
/// `String`/`Null` `LoroProperty` variants hydrates into a Grafeo node with
/// matching `Value::Bool`/`Int64`/`Float64`/`String`/`Null` properties.
/// Covers the 5 `LoroProperty` variants wired through `From<LoroProperty>
/// for GraphValue` → `gval_to_grafeo_value`.
#[test]
fn parallel_hydrate_preserves_property_types() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = LoroDoc::new();
    let maps = BridgeMaps::new();

    let entity = vertex(
        vec!["Typed".into()],
        HashMap::from([
            ("b".into(), LoroProperty::Bool(true)),
            ("i".into(), LoroProperty::Integer(42)),
            ("f".into(), LoroProperty::Float(3.5)),
            ("s".into(), LoroProperty::String("hello".into())),
            ("n".into(), LoroProperty::Null),
        ]),
    );
    let loro_key = "V/typed";
    reconcile_vertex_into_loro(&doc, loro_key, &entity);

    let result = parallel_hydrate_grafeo(&db, &doc, &maps, None, None);
    assert!(
        result.is_ok(),
        "typed-props hydrate should be Ok, got {result:?}"
    );

    let node_id = *maps
        .node_id_map
        .read()
        .get(loro_key)
        .expect("BridgeMaps binding");
    assert_grafeo_node(
        &db,
        node_id,
        &["Typed"],
        &[
            ("b", GraphValue::Bool(true)),
            ("i", GraphValue::Integer(42)),
            ("f", GraphValue::Float(3.5)),
            ("s", GraphValue::String("hello".into())),
            ("n", GraphValue::Null),
        ],
    );
}

/// Malformed-shape rejection: a Loro `V/<key>` entry that is NOT a
/// `Container::Map` (here, a `Container::List`) causes
/// `parallel_hydrate_grafeo` to return `Err(GrafeoLoroError::Bridge(...))`.
///
/// # L3 deviation from original spec
///
/// The original P3T2-L1 scaffold promised "Binary rejection" via
/// `lval_to_gval`'s `LoroValue::Binary` arm. But the hydration read-path
/// SSOT is `VertexEntity::hydrate_map` (`lorosurgeon-0.2.1/src/hydrate.rs:127`),
/// which uses the `Hydrate` derive — the derive rejects `LoroValue::Binary`
/// at the field-extraction level (the `Binary` arm is reserved for
/// `Vec<u8>`/`ByteArray` fields; `LoroProperty` has no such variant). The
/// `lval_to_gval` rejection is therefore unreachable through this code path.
/// Per the L3 task spec, this test was CHANGED to assert the malformed-shape
/// rejection arm in `parallel_hydrate_grafeo` itself (`voc.into_container()`
/// and `c.into_map()` collapse to `None` when the sub-map is a `Container::List`).
/// P3T2-L2R2 m1: renamed from `parallel_hydrate_rejects_binary_property` to
/// `parallel_hydrate_rejects_non_map_container` to reflect what the test
/// actually asserts (the original Binary-rejection coverage is preserved at
/// `src/types/values.rs:296` `lval_to_gval_rejects_binary_and_container`).
#[test]
fn parallel_hydrate_rejects_non_map_container() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = LoroDoc::new();
    let maps = BridgeMaps::new();

    // Insert a `Container::List` at `V/malformed` (not the expected `Container::Map`).
    let v_root = doc.get_map(ROOT_VERTICES);
    let _list = v_root
        .ensure_mergeable_list("V/malformed")
        .expect("ensure_mergeable_list");
    doc.commit();

    let err =
        parallel_hydrate_grafeo(&db, &doc, &maps, None, None).expect_err("expected Bridge error");
    assert!(
        matches!(err, grafeo_loro::error::GrafeoLoroError::Bridge(_)),
        "expected Bridge error for malformed vertex shape, got {err:?}"
    );
    // Anti-Goodhart: the malformed vertex was the only entry, so no partial
    // commit occurred (the failing chunk's session auto-rolled back).
    assert_eq!(
        db.node_count(),
        0,
        "no nodes should exist after failed hydrate"
    );
}

/// Side-effect contract: `parallel_hydrate_grafeo` populates `BridgeMaps`
/// with bidirectional `loro_key ↔ NodeId` bindings — every hydrated vertex
/// must have a forward entry in `node_id_map` AND a matching inverse entry in
/// `node_key_map` pointing back to the same `loro_key` (P3T2-L2R2 M3 —
/// replaces the prior `parallel_hydrate_tags_origin_loro_bridge` tautology,
/// which was a subset of Test 3 with no new coverage; this test verifies a
/// real contract that Test 3's count-only assertion does NOT cover).
///
/// Rationale: `apply_loro_op` → `apply_upsert_node` → `BridgeMaps::insert_node`
/// writes BOTH `node_id_map` and `node_key_map` (verified at
/// `src/bridge/grafeo_tx.rs:45-48`); the inverse-consistency contract is a
/// real precondition for Phase 4's `SyncEngine::new(db, doc, maps)` to
/// correctly translate outbound Grafeo CDC events back into Loro writes.
#[test]
fn parallel_hydrate_populates_bridge_maps() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = LoroDoc::new();
    let maps = BridgeMaps::new();

    let n: usize = 5;
    let keys: Vec<String> = (0..n).map(|i| format!("V/{i}")).collect();
    for (i, key) in keys.iter().enumerate() {
        let entity = vertex(
            vec!["Tagged".into()],
            HashMap::from([("idx".into(), LoroProperty::Integer(i as i64))]),
        );
        reconcile_vertex_into_loro(&doc, key, &entity);
    }

    let result = parallel_hydrate_grafeo(&db, &doc, &maps, None, None);
    assert!(result.is_ok(), "hydrate should succeed, got {result:?}");

    // Forward map (loro_key → NodeId) + inverse map (NodeId → loro_key) must
    // both have exactly `n` entries and be consistent inverses of each other.
    let id_map = maps.node_id_map.read();
    let key_map = maps.node_key_map.read();
    assert_eq!(id_map.len(), n, "node_id_map must have {n} entries");
    assert_eq!(key_map.len(), n, "node_key_map must have {n} entries");
    for key in &keys {
        let id = id_map
            .get(key)
            .unwrap_or_else(|| panic!("node_id_map missing binding for {key:?}"));
        let inverse = key_map
            .get(id)
            .unwrap_or_else(|| panic!("node_key_map missing inverse for NodeId {id:?}"));
        assert_eq!(
            inverse, key,
            "node_key_map[{id:?}] = {inverse:?}, expected {key:?} (inverse mismatch)"
        );
    }
}

/// Spec validation gate (`docs/implementation-plan.md:78`): hydrating 10,000
/// vertices into Grafeo completes in <500 ms wall-clock on an 8-core machine.
///
/// # Test shape (anti-Goodhart — L3 MUST follow this, NOT short-circuit)
///
/// 1. **Generate 10,000 vertices in a fresh `LoroDoc`** via a builder loop
///    that calls `lorosurgeon::RootReconciler::new(doc.get_map("V"))` (the
///    SSOT write path also used by `VertexBuilder::commit` at `src/app.rs:422`)
///    for each `VertexEntity` with **2 labels + 3 properties of mixed types**
///    (`Bool`, `I64`, `String`). DO NOT write via `LoroMap::insert` directly —
///    that produces a `LoroValue::Map` snapshot, exercising the wrong unwrap
///    path (cold-boot read uses `Container::Map`, not `LoroValue::Map` — DEVIL M5).
/// 2. **Time ONLY the hydration call** — `std::time::Instant::now()` is
///    started AFTER the 10k-vertex Loro fixture setup completes, and
///    `.elapsed()` is captured immediately after `parallel_hydrate_grafeo`
///    returns. The 10k Loro doc setup is NOT timed (only the hydration is).
/// 3. Assert `elapsed < 500ms` (use `std::time::Instant` — NOT `tokio::time`;
///    hydration is sync per L1 decision 2). The 500ms threshold is the
///    documented spec gate — anti-Goodhart: do NOT relax to "CI tolerance".
/// 4. Assert `db` has exactly 10,000 nodes (Grafeo `node_count()` API).
///
/// Marked `#[ignore]` so it doesn't run in CI by default — benchmark: run
/// manually with `--release --ignored parallel_hydrate_10k_nodes_under_500ms`.
#[test]
#[ignore = "benchmark: run manually with `--release --ignored parallel_hydrate_10k_nodes_under_500ms`"]
fn parallel_hydrate_10k_nodes_under_500ms() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = LoroDoc::new();
    let maps = BridgeMaps::new();

    // Fixture setup (10k Loro vertices) — NOT timed; only the hydration call
    // below is the subject of the 500ms spec gate.
    for i in 0..10_000 {
        let entity = vertex(
            vec!["LabelA".into(), "LabelB".into()],
            HashMap::from([
                ("flag".into(), LoroProperty::Bool(i % 2 == 0)),
                ("idx".into(), LoroProperty::Integer(i as i64)),
                ("name".into(), LoroProperty::String(format!("n{i}"))),
            ]),
        );
        let key = format!("V/{i}");
        reconcile_vertex_into_loro(&doc, &key, &entity);
    }

    // Time ONLY the hydration call (per HUNT C1: spec gate measures hydration,
    // not Loro doc fixture setup). `--release` is required for the spec gate.
    let start = std::time::Instant::now();
    let result = parallel_hydrate_grafeo(&db, &doc, &maps, None, None);
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "10k hydrate should succeed, got {result:?}");
    // Spec gate: 500ms — anti-Goodhart (do NOT relax to "CI tolerance").
    assert!(
        elapsed.as_millis() < 500,
        "hydration of 10k nodes took {elapsed:?}; spec gate is 500ms"
    );
    assert_eq!(db.node_count(), 10_000, "all 10k nodes must commit");
    assert_eq!(
        maps.node_id_map.read().len(),
        10_000,
        "BridgeMaps must have 10k bindings"
    );
}

/// Anti-happy-path: a `VertexEntity` with `properties: HashMap::new()` (empty)
/// hydrates into a Grafeo node with 0 properties. L3 reconciles one vertex
/// with empty `properties`, calls `parallel_hydrate_grafeo`, and asserts (a)
/// exactly 1 Grafeo node exists, (b) the node has 0 properties, (c) `BridgeMaps`
/// contains the `loro_key → NodeId` binding. Pins the empty-props edge case
/// (DEVIL m2) so L3 cannot trivially pass by always using a vertex with ≥1
/// property.
#[test]
fn parallel_hydrate_vertex_with_no_properties() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = LoroDoc::new();
    let maps = BridgeMaps::new();

    let entity = vertex(vec!["Thing".into()], HashMap::new());
    let loro_key = "V/empty";
    reconcile_vertex_into_loro(&doc, loro_key, &entity);

    let result = parallel_hydrate_grafeo(&db, &doc, &maps, None, None);
    assert!(
        result.is_ok(),
        "empty-props hydrate should be Ok, got {result:?}"
    );

    assert_eq!(db.node_count(), 1, "exactly 1 node should exist");
    let node_id = *maps
        .node_id_map
        .read()
        .get(loro_key)
        .expect("BridgeMaps binding for empty-props vertex");
    assert_grafeo_node(&db, node_id, &["Thing"], &[]);
}
