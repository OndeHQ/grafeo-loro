//! Phase 2 Task 3 tests: `app::VertexBuilder` fluent API.
//!
//! All 9 tests assert the roundtrip contract under test — `VertexBuilder::commit()`
//! writes the vertex to **both** Loro (under the `"V"` root map at a fresh
//! `loro_key`) and Grafeo (via `Session::create_node_with_props` inside
//! `apply_loro_op`). The roundtrip tests read back from EACH store and assert
//! equality with the input (anti-Goodhart: assert BOTH stores, not just one).
//!
//! ## Test fixture (Q4 — P2T3-DEVIL resolution)
//!
//! Each test constructs a fresh `SyncEngine` (which holds a fresh `LoroDoc` +
//! fresh in-memory `GrafeoDB`) and wraps it in a `GrafeoLoroApp` via
//! [`GrafeoLoroApp::from_sync_engine`] — a non-test-y constructor intended
//! for tests and future embedding scenarios. There is NO prior test-fixture
//! pattern for `GrafeoLoroApp` (P2T2's `build_chain_fixture` constructs a
//! bare `GrafeoDB` chain, NOT a `GrafeoLoroApp` — P2T3-DEVIL m3).
//!
//! `GrafeoLoroAppBuilder::build` is Phase 4 scope (too heavy for unit tests).
//!
//! ## Reading back from Loro (M5 — P2T3-DEVIL API fix)
//!
//! `LoroMap::get_map` does NOT exist (only `LoroDoc::get_map` exists —
//! verified at `loro-1.13.6/src/lib.rs:489`). The correct API to read a
//! per-vertex nested map is `LoroMap::get(&str) -> Option<ValueOrContainer>`
//! (`loro-1.13.6/src/lib.rs:2150`) + `ValueOrContainer::Container(
//! Container::Map(m))` extraction (`loro-1.13.6/src/lib.rs:3813`,
//! `EnumAsInner` derive gives `into_container()`/`as_container()`):
//!
//! ```no_run
//! # use grafeo_loro::schema::VertexEntity;
//! # use lorosurgeon::Hydrate;
//! # use loro::{Container, LoroDoc, ValueOrContainer};
//! # use parking_lot::RwLock;
//! # use std::sync::Arc;
//! # let doc: Arc<RwLock<LoroDoc>> = unimplemented!();
//! # let loro_key: String = unimplemented!();
//! let v_map = doc.read().get_map("V");
//! let vertex_value = v_map.get(&loro_key).expect("V[loro_key] exists");
//! let node_map = match vertex_value {
//!     ValueOrContainer::Container(Container::Map(m)) => m,
//!     _ => panic!("expected LoroMap container for vertex {loro_key:?}"),
//! };
//! let hydrated = VertexEntity::hydrate_map(&node_map).unwrap();
//! // assert hydrated.labels, hydrated.properties
//! ```
//!
//! The `loro_key` is recovered from the grafeo `NodeId` via
//! `BridgeMaps::node_key_map` (since `commit()` returns the grafeo `NodeId`,
//! not the `loro_key` — Q5):
//!
//! ```no_run
//! # use grafeo_loro::bridge::SyncEngine;
//! # use std::sync::Arc;
//! # let app: grafeo_loro::GrafeoLoroApp = unimplemented!();
//! # let node_id: grafeo_loro::types::ids::NodeId = unimplemented!();
//! let loro_key = app.maps().node_key_map.read().get(&node_id)
//!     .cloned().expect("BridgeMaps has binding");
//! ```
//!
//! ## Reading back from Grafeo
//!
//! Use `session.get_node(NodeId) -> Option<Node>` (`session/mod.rs:5138`) and
//! inspect `node.labels` + `node.properties` (verified at
//! `grafeo-core-0.5.42/src/graph/lpg/node.rs:30-37`).
//!
//! # Grafeo Session API (verified against `grafeo-engine-0.5.42/src/`)
//!
//! - `GrafeoDB::new_in_memory() -> Self` — `database/mod.rs:267`
//! - `db.session() -> Session` — `database/mod.rs:1663`
//! - `db.session_with_cdc(bool) -> Session` — `database/mod.rs:1728`
//! - `session.get_node(NodeId) -> Option<Node>` — `session/mod.rs:5138`
//! - `Node::labels: SmallVec<[ArcStr; 2]>` — `grafeo-core-0.5.42/src/graph/lpg/node.rs:34`
//! - `Node::properties: PropertyMap` — `grafeo-core-0.5.42/src/graph/lpg/node.rs:36`
//! - `Node::has_label(&str) -> bool` — `grafeo-core-0.5.42/src/graph/lpg/node.rs:80`
//! - `Node::get_property(&str) -> Option<&Value>` — `grafeo-core-0.5.42/src/graph/lpg/node.rs:91`
//!
//! # Loro API (verified against `loro-1.13.6/src/lib.rs`)
//!
//! - `LoroDoc::new() -> Self` — `lib.rs:137`
//! - `LoroDoc::get_map<I: IntoContainerId>(&self, I) -> LoroMap` — `lib.rs:489`
//! - `LoroMap::get(&self, &str) -> Option<ValueOrContainer>` — `lib.rs:2150`
//! - `ValueOrContainer::Container(Container::Map(LoroMap))` — `lib.rs:3813` (`EnumAsInner`)
//!
//! # lorosurgeon API (verified against `lorosurgeon-0.2.1/src/`)
//!
//! - `<T as Hydrate>::hydrate_map(&LoroMap) -> Result<T, HydrateError>` — `hydrate.rs:64`

#![allow(missing_docs)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;

use grafeo::GrafeoDB;
use loro::{Container, LoroDoc, ValueOrContainer};
use lorosurgeon::Hydrate;
use parking_lot::RwLock;

use grafeo_loro::bridge::SyncEngine;
use grafeo_loro::constants::ROOT_VERTICES;
use grafeo_loro::error::GrafeoLoroError;
use grafeo_loro::schema::VertexEntity;
use grafeo_loro::types::values::gval_to_grafeo_value;
use grafeo_loro::types::{GraphValue, LoroProperty};
use grafeo_loro::GrafeoLoroApp;

/// Build a fresh `GrafeoLoroApp` over an in-memory `GrafeoDB` + `LoroDoc` for
/// unit tests. The returned `Arc<GrafeoDB>` + `Arc<RwLock<LoroDoc>>` let the
/// test read back state from each store via `db.session().get_node(...)` and
/// `doc.read().get_map("V").get(...)`.
fn build_app() -> (GrafeoLoroApp, Arc<GrafeoDB>, Arc<RwLock<LoroDoc>>) {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, _inbound_rx, _outbound_rx) = SyncEngine::new(db.clone(), doc.clone());
    let app = GrafeoLoroApp::from_sync_engine(Arc::new(engine));
    (app, db, doc)
}

/// Build a fresh `GrafeoLoroApp` over an in-memory `GrafeoDB` with a 1-byte
/// `max_property_size` limit, forcing `Session::create_node_with_props` to
/// reject any property value larger than 1 byte (Q6 — atomicity mock).
/// Verified: `Config::in_memory().with_max_property_size(1)` and
/// `GrafeoDB::with_config(config)` (`grafeo-engine-0.5.42/src/config.rs:425`
/// and `:559` and `database/mod.rs:346`); `check_property_size` at
/// `session/mod.rs:4631` rejects with `Err(Query::Execution(...))` when
/// `value.estimated_size_bytes() > limit`.
fn build_app_with_tiny_property_limit() -> (GrafeoLoroApp, Arc<GrafeoDB>, Arc<RwLock<LoroDoc>>) {
    let config = grafeo::Config::in_memory().with_max_property_size(1);
    let db = Arc::new(GrafeoDB::with_config(config).expect("db with tiny property limit"));
    let doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, _inbound_rx, _outbound_rx) = SyncEngine::new(db.clone(), doc.clone());
    let app = GrafeoLoroApp::from_sync_engine(Arc::new(engine));
    (app, db, doc)
}

/// Read a vertex back from Grafeo via `Session::get_node` and assert the
/// labels + properties match the expected snapshot. Anti-Goodhart: reads
/// from the actual store (`session/mod.rs:5138`), not from a stale cache.
fn assert_grafeo_has_vertex(
    db: &GrafeoDB,
    node_id: grafeo::NodeId,
    expected_labels: &[&str],
    expected_props: &[(&str, GraphValue)],
) {
    let session = db.session();
    let node = session.get_node(node_id).unwrap_or_else(|| {
        panic!(
            "grafeo should have node {node_id:?} (expected labels={expected_labels:?}, props={expected_props:?})"
        )
    });
    // Labels: every expected label is present AND label count matches (no
    // spurious extra labels).
    for lbl in expected_labels {
        assert!(
            node.has_label(lbl),
            "grafeo node {node_id:?} should have label {lbl:?}; actual labels={:?}",
            node.labels
        );
    }
    assert_eq!(
        node.labels.len(),
        expected_labels.len(),
        "grafeo node {node_id:?} label count mismatch; actual labels={:?}",
        node.labels
    );
    // Properties: every expected (key, value) matches the converted grafeo
    // value. `gval_to_grafeo_value` is the SSOT conversion (architecture §5).
    for (k, expected_v) in expected_props {
        let actual = node.get_property(k).unwrap_or_else(|| {
            panic!(
                "grafeo node {node_id:?} missing property {k:?}; actual properties={:?}",
                node.properties
            )
        });
        let expected_gv = gval_to_grafeo_value(expected_v.clone());
        assert_eq!(
            actual, &expected_gv,
            "grafeo node {node_id:?} property {k:?} mismatch; expected={expected_gv:?}, actual={actual:?}"
        );
    }
}

/// Read a vertex back from Loro via `LoroMap::get` + `VertexEntity::hydrate_map`
/// and assert the labels + properties match the expected snapshot. Anti-Goodhart:
/// hydrates the full `VertexEntity` (not just the property map) so the labels
/// roundtrip is also verified (catches the pre-existing translator bug pattern
/// where labels get dropped — P2T3-DEVIL M4).
fn assert_loro_has_vertex(
    doc: &Arc<RwLock<LoroDoc>>,
    loro_key: &str,
    expected_labels: &[&str],
    expected_props: &[(&str, GraphValue)],
) {
    let doc_guard = doc.read();
    let v_map = doc_guard.get_map(ROOT_VERTICES);
    let vertex = v_map
        .get(loro_key)
        .unwrap_or_else(|| panic!("Loro V[{loro_key:?}] should exist after commit()"));
    let node_map = match vertex {
        ValueOrContainer::Container(Container::Map(m)) => m,
        _ => panic!("Loro V[{loro_key:?}] should be a Map container, got {vertex:?}"),
    };
    let hydrated = VertexEntity::hydrate_map(&node_map)
        .unwrap_or_else(|e| panic!("hydrate_map failed for V[{loro_key:?}]: {e}"));
    drop(doc_guard);

    // Labels: sort both sides and compare (order is not significant —
    // `Vec<String>` reconcile preserves insertion order, but the test asserts
    // set-equality to avoid coupling to internal ordering).
    let mut expected_sorted: Vec<&str> = expected_labels.to_vec();
    expected_sorted.sort_unstable();
    let mut actual_sorted: Vec<&str> = hydrated.labels.iter().map(String::as_str).collect();
    actual_sorted.sort_unstable();
    assert_eq!(
        actual_sorted, expected_sorted,
        "Loro V[{loro_key:?}] labels mismatch; expected={expected_sorted:?}, actual={actual_sorted:?}"
    );

    // Properties: every expected (key, value) matches the converted LoroProperty.
    assert_eq!(
        hydrated.properties.len(),
        expected_props.len(),
        "Loro V[{loro_key:?}] property count mismatch; expected={}, actual={}",
        expected_props.len(),
        hydrated.properties.len()
    );
    for (k, expected_v) in expected_props {
        let actual = hydrated
            .properties
            .get(*k)
            .unwrap_or_else(|| panic!("Loro V[{loro_key:?}] missing property {k:?}"));
        let expected_loro = LoroProperty::try_from(expected_v.clone())
            .expect("expected_v should be a scalar GraphValue (Null/Bool/Integer/Float/String)");
        assert_eq!(
            actual, &expected_loro,
            "Loro V[{loro_key:?}] property {k:?} mismatch; expected={expected_loro:?}, actual={actual:?}"
        );
    }
}

/// Assert the Loro `"V"` root map does NOT contain `loro_key` (compensation
/// worked) and that `BridgeMaps::node_id_map` is empty (no binding recorded
/// on Grafeo failure). Used by the atomicity + strict-reject tests.
fn assert_no_side_effects(app: &GrafeoLoroApp, doc: &Arc<RwLock<LoroDoc>>, loro_key: &str) {
    let doc_guard = doc.read();
    let v_map = doc_guard.get_map(ROOT_VERTICES);
    assert!(
        v_map.get(loro_key).is_none(),
        "Loro V[{loro_key:?}] must NOT exist after commit() failure (compensation or pre-write reject)"
    );
    drop(doc_guard);
    assert!(
        app.maps().node_id_map.read().is_empty(),
        "BridgeMaps::node_id_map must be empty after commit() failure (no Grafeo write should have recorded a binding)"
    );
    assert!(
        app.maps().node_key_map.read().is_empty(),
        "BridgeMaps::node_key_map must be empty after commit() failure (forward+inverse maps in lock-step)"
    );
}

/// Create a vertex with 1 label (`"Person"`) + 1 property (`"name" →
/// "Alix"`), `commit()`, read back from BOTH Loro AND Grafeo, assert labels +
/// properties match in both stores. Anti-Goodhart: assert BOTH stores, not
/// just one (catches Loro-only or Grafeo-only regressions).
#[test]
fn vertex_builder_basic_roundtrip() {
    let (app, db, doc) = build_app();
    let node_id = app
        .create_vertex()
        .with_label("Person")
        .with_property("name", GraphValue::String("Alix".into()))
        .commit()
        .expect("commit succeeds");
    let loro_key = app
        .maps()
        .node_key_map
        .read()
        .get(&node_id)
        .cloned()
        .expect("BridgeMaps has binding for committed vertex");
    assert_grafeo_has_vertex(
        &db,
        node_id,
        &["Person"],
        &[("name", GraphValue::String("Alix".into()))],
    );
    assert_loro_has_vertex(
        &doc,
        &loro_key,
        &["Person"],
        &[("name", GraphValue::String("Alix".into()))],
    );
}

/// Create a vertex with 3 labels (`"Person"`, `"Admin"`, `"Engineer"`),
/// `commit()`, assert all 3 labels present in BOTH stores. Grafeo supports
/// multi-label nodes natively (`Node::labels: SmallVec<[ArcStr; 2]>` —
/// `grafeo-core-0.5.42/src/graph/lpg/node.rs:34`); Loro-side `VertexEntity`
/// stores labels as a `Vec<String>` (Phase 2 Task 1 verified).
#[test]
fn vertex_builder_multiple_labels() {
    let (app, db, doc) = build_app();
    let node_id = app
        .create_vertex()
        .with_label("Person")
        .with_label("Admin")
        .with_label("Engineer")
        .commit()
        .expect("commit succeeds");
    let loro_key = app
        .maps()
        .node_key_map
        .read()
        .get(&node_id)
        .cloned()
        .expect("BridgeMaps has binding for committed vertex");
    assert_grafeo_has_vertex(&db, node_id, &["Person", "Admin", "Engineer"], &[]);
    assert_loro_has_vertex(&doc, &loro_key, &["Person", "Admin", "Engineer"], &[]);
}

/// Create a vertex with 3 properties covering `Bool`, `Integer`, `String`
/// (`"active" → true`, `"age" → 30`, `"name" → "Alix"`), `commit()`, assert
/// all 3 properties present in BOTH stores with correct values. Exercises
/// the `GraphValue → LoroProperty` conversion paths (Bool/Integer/String are
/// shared variants; Float is exercised implicitly by Integer's i64 path).
#[test]
fn vertex_builder_multiple_properties() {
    let (app, db, doc) = build_app();
    let node_id = app
        .create_vertex()
        .with_property("active", GraphValue::Bool(true))
        .with_property("age", GraphValue::Integer(30))
        .with_property("name", GraphValue::String("Alix".into()))
        .commit()
        .expect("commit succeeds");
    let loro_key = app
        .maps()
        .node_key_map
        .read()
        .get(&node_id)
        .cloned()
        .expect("BridgeMaps has binding for committed vertex");
    let expected_props: &[(&str, GraphValue)] = &[
        ("active", GraphValue::Bool(true)),
        ("age", GraphValue::Integer(30)),
        ("name", GraphValue::String("Alix".into())),
    ];
    assert_grafeo_has_vertex(&db, node_id, &[], expected_props);
    assert_loro_has_vertex(&doc, &loro_key, &[], expected_props);
}

/// Create a vertex with NO labels and NO properties, `commit()`, assert it
/// succeeds (sensible default behavior). The grafeo side accepts an empty
/// label slice + empty props iter (`Session::create_node_with_props(&[], [])`
/// → `Ok(NodeId)`); the Loro side writes a `VertexEntity` with empty
/// `labels: Vec::new()` + empty `properties: HashMap::new()` + default
/// `description: String::new()` (the `#[loro(text)]` field — Phase 2 Task 1).
#[test]
fn vertex_builder_empty_vertex() {
    let (app, db, doc) = build_app();
    let node_id = app
        .create_vertex()
        .commit()
        .expect("empty vertex commits ok");
    let loro_key = app
        .maps()
        .node_key_map
        .read()
        .get(&node_id)
        .cloned()
        .expect("BridgeMaps has binding for committed empty vertex");
    assert_grafeo_has_vertex(&db, node_id, &[], &[]);
    assert_loro_has_vertex(&doc, &loro_key, &[], &[]);
}

/// Force a grafeo failure mid-`commit()` and assert Loro state is rolled back
/// (atomicity contract Option a — see `VertexBuilder` struct doc).
///
/// Mock strategy (Q6 — P2T3-DEVIL resolution): use
/// `Config::in_memory().with_max_property_size(1)` +
/// `GrafeoDB::with_config(config)` to force `check_property_size` rejection
/// at `session/mod.rs:4631`. The test then calls `commit()` with a property
/// value whose `estimated_size_bytes > 1` (e.g. `GraphValue::String("x".repeat(1024))`
/// → 1024 bytes > 1 byte limit). `create_node_with_props` returns
/// `Err(grafeo::Error::Query(...))` → mapped to `GrafeoLoroError::Grafeo(...)`
/// via the `#[from]` impl at `src/error.rs:9`. Deterministic — no mock
/// infrastructure, exercises a real grafeo code path.
///
/// Assertion: after `commit()` returns `Err`, the Loro `"V"` root map must
/// NOT contain the `loro_key` (compensation deleted it) and `BridgeMaps` must
/// NOT contain the binding (never inserted on Grafeo failure).
#[test]
fn vertex_builder_atomicity_rollback_on_grafeo_failure() {
    let (app, _db, doc) = build_app_with_tiny_property_limit();
    let result = app
        .create_vertex()
        .with_label("Person")
        .with_property("oversized", GraphValue::String("x".repeat(1024)))
        .commit();
    assert!(
        result.is_err(),
        "commit must fail when grafeo rejects the property size"
    );
    // The AtomicU64 counter starts at 0, so the failed commit's loro_key is
    // "V/0". Step 1 strict-reject let the String through (it's a scalar);
    // step 3 wrote V/0 to Loro; step 5 apply_loro_op failed on the
    // property-size check at session/mod.rs:4631; compensate_loro_vertex
    // deleted V/0. Final state: Loro V map empty + BridgeMaps empty.
    assert_no_side_effects(&app, &doc, "V/0");
}

/// Spawn 2 `VertexBuilder`s from the same `GrafeoLoroApp`, commit
/// concurrently, assert unique `NodeId`s + unique `loro_key`s + no
/// `BridgeMaps` corruption (Q8 — P2T3-DEVIL concurrency contract).
///
/// The `Arc<AtomicU64>` counter on `GrafeoLoroApp` (cloned into each
/// `VertexBuilder` via `Arc::clone`) guarantees unique `loro_key`s:
/// `fetch_add(1, Ordering::Relaxed)` is atomic and each call returns a
/// distinct value. The Loro `RwLock` write guard serializes
/// `set_next_commit_origin + commit` per call. Grafeo sessions run
/// concurrently (grafeo is internally thread-safe via MVCC); each
/// `create_node_with_props` assigns a distinct `NodeId` so no write-write
/// conflict (default isolation = `SnapshotIsolation`).
#[test]
fn vertex_builder_concurrent_commit() {
    let (app, _db, _doc) = build_app();
    let app = Arc::new(app);

    // Shared Mutex<Vec<...>> collects (NodeId, loro_key) pairs from both
    // threads. Using `std::sync::Mutex` (not `parking_lot::Mutex`) because
    // `std::sync::Mutex` is `Send` and can cross the thread::spawn boundary
    // without extra configuration.
    let collected: Arc<Mutex<Vec<(grafeo::NodeId, String)>>> = Arc::new(Mutex::new(Vec::new()));

    let threads: Vec<_> = (0..2)
        .map(|_| {
            let app = Arc::clone(&app);
            let collected = Arc::clone(&collected);
            std::thread::spawn(move || {
                for i in 0..10 {
                    let node_id = app
                        .create_vertex()
                        .with_label("N")
                        .with_property("idx", GraphValue::Integer(i))
                        .commit()
                        .expect("concurrent commit succeeds");
                    let loro_key = app
                        .maps()
                        .node_key_map
                        .read()
                        .get(&node_id)
                        .cloned()
                        .expect("BridgeMaps has binding for concurrent commit");
                    collected
                        .lock()
                        .expect("collected mutex poisoned")
                        .push((node_id, loro_key));
                }
            })
        })
        .collect();

    for t in threads {
        t.join().expect("worker thread panicked");
    }

    let pairs = Arc::try_unwrap(collected)
        .expect("both threads done; Arc has unique owner")
        .into_inner()
        .expect("collected mutex poisoned");

    // 20 commits total.
    assert_eq!(pairs.len(), 20, "expected 20 (NodeId, loro_key) pairs");

    // All 20 NodeIds are distinct (grafeo-assigned).
    let node_ids: HashSet<grafeo::NodeId> = pairs.iter().map(|(id, _)| *id).collect();
    assert_eq!(node_ids.len(), 20, "all 20 NodeIds must be distinct");

    // All 20 loro_keys are distinct (AtomicU64 counter — Q3 strategy).
    let loro_keys: HashSet<String> = pairs.iter().map(|(_, k)| k.clone()).collect();
    assert_eq!(loro_keys.len(), 20, "all 20 loro_keys must be distinct");

    // Forward + inverse BridgeMaps in lock-step (Q5 contract).
    assert_eq!(
        app.maps().node_id_map.read().len(),
        20,
        "BridgeMaps::node_id_map must have 20 entries (no corruption)"
    );
    assert_eq!(
        app.maps().node_key_map.read().len(),
        20,
        "BridgeMaps::node_key_map must have 20 entries (forward+inverse in lock-step)"
    );

    // Every (NodeId, loro_key) pair from the worker threads must round-trip
    // through BridgeMaps (forward AND inverse lookups agree).
    for (node_id, loro_key) in &pairs {
        let fwd = app
            .maps()
            .node_id_map
            .read()
            .get(loro_key)
            .copied()
            .expect("forward lookup");
        let inv = app
            .maps()
            .node_key_map
            .read()
            .get(node_id)
            .cloned()
            .expect("inverse lookup");
        assert_eq!(&fwd, node_id, "forward lookup mismatch for {loro_key:?}");
        assert_eq!(&inv, loro_key, "inverse lookup mismatch for {node_id:?}");
    }
}

/// `commit()` with a `GraphValue::Vector` property must return
/// `Err(UnsupportedLoroType(_))` BEFORE any Loro/Grafeo write (Q2 strict
/// reject — see `VertexBuilder` struct doc). The rejection happens at
/// `commit()` step 1, BEFORE the Loro write lock is acquired, so the Loro
/// `"V"` map must remain empty and `BridgeMaps` must be empty.
#[test]
fn vertex_builder_rejects_vector_property() {
    let (app, _db, doc) = build_app();
    let vec: std::sync::Arc<[f32]> = vec![1.0, 2.0, 3.0].into();
    let result = app
        .create_vertex()
        .with_label("Person")
        .with_property("embedding", GraphValue::Vector(vec))
        .commit();
    assert!(
        matches!(result, Err(GrafeoLoroError::UnsupportedLoroType(_))),
        "expected Err(UnsupportedLoroType) for Vector property, got {result:?}"
    );
    // The strict reject happened BEFORE any Loro/Grafeo write, so no
    // `loro_key` was generated. Assert the V map is empty (no key to check
    // against — just verify the root map is empty).
    let doc_guard = doc.read();
    let v_map = doc_guard.get_map(ROOT_VERTICES);
    assert!(
        v_map.is_empty(),
        "Loro V map must be empty after strict-reject (no write occurred)"
    );
    drop(doc_guard);
    assert!(
        app.maps().node_id_map.read().is_empty(),
        "BridgeMaps::node_id_map must be empty after strict-reject (no Grafeo write)"
    );
    assert!(
        app.maps().node_key_map.read().is_empty(),
        "BridgeMaps::node_key_map must be empty after strict-reject"
    );
}

/// `commit()` with a `GraphValue::Map` property must return
/// `Err(UnsupportedLoroType(_))` BEFORE any Loro/Grafeo write. See
/// `vertex_builder_rejects_vector_property` for the contract.
#[test]
fn vertex_builder_rejects_map_property() {
    let (app, _db, doc) = build_app();
    let mut map = HashMap::new();
    map.insert("k".to_string(), GraphValue::Integer(1));
    let result = app
        .create_vertex()
        .with_label("Person")
        .with_property("metadata", GraphValue::Map(map))
        .commit();
    assert!(
        matches!(result, Err(GrafeoLoroError::UnsupportedLoroType(_))),
        "expected Err(UnsupportedLoroType) for Map property, got {result:?}"
    );
    let doc_guard = doc.read();
    let v_map = doc_guard.get_map(ROOT_VERTICES);
    assert!(
        v_map.is_empty(),
        "Loro V map must be empty after strict-reject (no write occurred)"
    );
    drop(doc_guard);
    assert!(
        app.maps().node_id_map.read().is_empty(),
        "BridgeMaps::node_id_map must be empty after strict-reject (no Grafeo write)"
    );
    assert!(
        app.maps().node_key_map.read().is_empty(),
        "BridgeMaps::node_key_map must be empty after strict-reject"
    );
}

/// `commit()` with a `GraphValue::List` property must return
/// `Err(UnsupportedLoroType(_))` BEFORE any Loro/Grafeo write. See
/// `vertex_builder_rejects_vector_property` for the contract.
#[test]
fn vertex_builder_rejects_list_property() {
    let (app, _db, doc) = build_app();
    let list = vec![GraphValue::Integer(1), GraphValue::Integer(2)];
    let result = app
        .create_vertex()
        .with_label("Person")
        .with_property("tags", GraphValue::List(list))
        .commit();
    assert!(
        matches!(result, Err(GrafeoLoroError::UnsupportedLoroType(_))),
        "expected Err(UnsupportedLoroType) for List property, got {result:?}"
    );
    let doc_guard = doc.read();
    let v_map = doc_guard.get_map(ROOT_VERTICES);
    assert!(
        v_map.is_empty(),
        "Loro V map must be empty after strict-reject (no write occurred)"
    );
    drop(doc_guard);
    assert!(
        app.maps().node_id_map.read().is_empty(),
        "BridgeMaps::node_id_map must be empty after strict-reject (no Grafeo write)"
    );
    assert!(
        app.maps().node_key_map.read().is_empty(),
        "BridgeMaps::node_key_map must be empty after strict-reject"
    );
}

/// B1 inbound filter must prevent `commit()`'s Loro write from echoing
/// through the subscriber (P2T3-HUNT MAJOR 2 — previously the filter was
/// dead code in the test suite: unit tests didn't install a subscriber, and
/// integration tests didn't call `commit()`).
///
/// Test flow:
/// 1. Build `GrafeoLoroApp` + install the Loro subscriber (no workers — we
///    only need the synchronous filter, not the async drain path).
/// 2. Snapshot `inbound_event_count` + `inbound_filtered_count` BEFORE
///    `commit()`.
/// 3. Call `commit()` — this fires `doc.commit()` synchronously, which
///    invokes the subscriber handler. The handler MUST filter the event
///    (origin = `ORIGIN_LORO_BRIDGE`) and return early WITHOUT calling
///    `inbound_tx.try_send(...)`. The filter increments
///    `inbound_filtered_count` (P2T3-L2R2 MAJOR 2 — `inbound_event_count`
///    alone is insufficient because `translate_diff_event` also silently
///    skips Container-ref diffs, so a filter regression would NOT increment
///    `inbound_event_count`).
/// 4. Assert `inbound_filtered_count` INCREMENTED (filter actually fired —
///    this is the primary regression-catching assertion).
/// 5. Assert `inbound_event_count` is UNCHANGED (defense-in-depth — no echo
///    reached the inbound channel even if the translator ever learns to
///    handle Container refs).
/// 6. Assert `BridgeMaps::node_id_map.len() == 1` (defense-in-depth — only
///    the binding `commit()`'s step 5 inserted; no duplicate).
#[test]
fn vertex_builder_commit_does_not_echo_through_subscriber() {
    let (app, _db, _doc) = build_app();
    // Install the subscriber (no workers — the filter is synchronous).
    app.sync_engine()
        .init_loro_subscriber()
        .expect("subscriber installed");
    let event_count_before = app.sync_engine().inbound_event_count();
    let filtered_count_before = app.sync_engine().inbound_filtered_count();
    let _node_id = app
        .create_vertex()
        .with_label("Person")
        .with_property("name", GraphValue::String("Alix".into()))
        .commit()
        .expect("commit succeeds");
    let event_count_after = app.sync_engine().inbound_event_count();
    let filtered_count_after = app.sync_engine().inbound_filtered_count();

    // PRIMARY assertion: filter actually fired. If this fails with
    // `filtered_count_after == filtered_count_before`, the B1 filter
    // regression has occurred (the `|| event.origin == ORIGIN_LORO_BRIDGE`
    // clause was removed or broken). Verify by temporarily commenting out
    // the clause in `src/bridge/sync_engine.rs:init_loro_subscriber` — this
    // assertion will then fail.
    assert!(
        filtered_count_after > filtered_count_before,
        "B1 filter MUST fire on commit()'s ORIGIN_LORO_BRIDGE-tagged Loro write; \
         filtered_count_before={filtered_count_before}, \
         filtered_count_after={filtered_count_after} (filter regression — the \
         `|| event.origin == ORIGIN_LORO_BRIDGE` clause is missing or broken)"
    );

    // Defense-in-depth: no echo reached the inbound channel. (Note: this
    // assertion alone is INSUFFICIENT to catch a filter regression because
    // `translate_diff_event` also silently skips Container-ref diffs. The
    // `filtered_count` assertion above is the real regression catcher.)
    assert_eq!(
        event_count_after, event_count_before,
        "no echo should reach the inbound channel; event_count_before={event_count_before}, \
         event_count_after={event_count_after}"
    );

    // Defense-in-depth: exactly 1 binding recorded (commit's step 5). A
    // duplicate would indicate an echo re-created the node.
    assert_eq!(
        app.maps().node_id_map.read().len(),
        1,
        "BridgeMaps::node_id_map should have exactly 1 binding after commit (no echo re-creation)"
    );
    assert_eq!(
        app.maps().node_key_map.read().len(),
        1,
        "BridgeMaps::node_key_map should have exactly 1 binding (forward+inverse in lock-step)"
    );
}
