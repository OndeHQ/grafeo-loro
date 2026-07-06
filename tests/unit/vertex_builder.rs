//! Phase 2 Task 3 scaffolds: `app::VertexBuilder` fluent API.
//!
//! All scaffolds are `#[ignore]` + `todo!()` per L1 convention; L3 implements
//! the bodies and removes the `#[ignore]` attributes.
//!
//! # Roundtrip contract under test
//!
//! `VertexBuilder::commit()` writes the vertex to **both** Loro (under the
//! `"V"` root map at a fresh `loro_key`) and Grafeo (via
//! `Session::create_node_with_props`). The roundtrip tests read back from each
//! store and assert equality with the input.
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
//! # let doc: LoroDoc = unimplemented!();
//! # let loro_key: String = unimplemented!();
//! let v_map = doc.get_map("V");
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

use std::sync::Arc;

use grafeo::GrafeoDB;
use loro::LoroDoc;
use parking_lot::RwLock;

use grafeo_loro::bridge::SyncEngine;
use grafeo_loro::error::GrafeoLoroError;
use grafeo_loro::types::GraphValue;
use grafeo_loro::GrafeoLoroApp;

/// Build a fresh `GrafeoLoroApp` over an in-memory `GrafeoDB` + `LoroDoc` for
/// unit tests. The returned `Arc<GrafeoDB>` lets the test read back grafeo
/// state via `db.session().get_node(...)`.
fn build_app() -> (GrafeoLoroApp, Arc<GrafeoDB>) {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, _inbound_rx, _outbound_rx) = SyncEngine::new(db.clone(), doc);
    let app = GrafeoLoroApp::from_sync_engine(Arc::new(engine));
    (app, db)
}

/// Build a fresh `GrafeoLoroApp` over an in-memory `GrafeoDB` with a 1-byte
/// `max_property_size` limit, forcing `Session::create_node_with_props` to
/// reject any property value larger than 1 byte (Q6 — atomicity mock).
/// Verified: `Config::in_memory().with_max_property_size(1)` +
/// `GrafeoDB::with_config(config)` (`grafeo-engine-0.5.42/src/config.rs:425`
/// + `:559` + `database/mod.rs:346`); `check_property_size` at
/// `session/mod.rs:4631` rejects with `Err(Query::Execution(...))` when
/// `value.estimated_size_bytes() > limit`.
fn build_app_with_tiny_property_limit() -> (GrafeoLoroApp, Arc<GrafeoDB>) {
    let config = grafeo::Config::in_memory().with_max_property_size(1);
    let db = Arc::new(GrafeoDB::with_config(config).expect("db with tiny property limit"));
    let doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, _inbound_rx, _outbound_rx) = SyncEngine::new(db.clone(), doc);
    let app = GrafeoLoroApp::from_sync_engine(Arc::new(engine));
    (app, db)
}

/// Create a vertex with 1 label (`"Person"`) + 1 property (`"name" →
/// "Alix"`), `commit()`, read back from BOTH Loro AND Grafeo, assert labels +
/// properties match in both stores. Anti-Goodhart: assert BOTH stores, not
/// just one (catches Loro-only or Grafeo-only regressions).
#[test]
#[ignore = "P2T3-L2 scaffold: L3 implements the body"]
fn vertex_builder_basic_roundtrip() {
    let (app, db) = build_app();
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
        .expect("BridgeMaps has binding");
    // TODO(P2T3-L3): assert_grafeo_has_vertex(&db, node_id, &["Person"], &[("name", GraphValue::String("Alix".into()))]);
    // TODO(P2T3-L3): assert_loro_has_vertex(&doc, &loro_key, &["Person".to_string()], &[("name".into(), LoroProperty::String("Alix".into()))]);
    let _ = (db, loro_key, node_id);
    todo!("P2T3-L3: implement basic roundtrip — see doc-comment + worklog for fixture strategy")
}

/// Create a vertex with 3 labels (`"Person"`, `"Admin"`, `"Engineer"`),
/// `commit()`, assert all 3 labels present in BOTH stores. Grafeo supports
/// multi-label nodes natively (`Node::labels: SmallVec<[ArcStr; 2]>` —
/// `grafeo-core-0.5.42/src/graph/lpg/node.rs:34`); Loro-side `VertexEntity`
/// stores labels as a `Vec<String>` (Phase 2 Task 1 verified).
#[test]
#[ignore = "P2T3-L2 scaffold: L3 implements the body"]
fn vertex_builder_multiple_labels() {
    let (app, db) = build_app();
    let node_id = app
        .create_vertex()
        .with_label("Person")
        .with_label("Admin")
        .with_label("Engineer")
        .commit()
        .expect("commit succeeds");
    // TODO(P2T3-L3): assert_grafeo_has_vertex(&db, node_id, &["Person", "Admin", "Engineer"], &[]);
    // TODO(P2T3-L3): assert_loro_has_vertex(&doc, &loro_key, &["Person", "Admin", "Engineer"], &[]);
    let _ = (db, node_id);
    todo!("P2T3-L3: implement multi-label roundtrip")
}

/// Create a vertex with 3 properties covering `Bool`, `Integer`, `String`
/// (`"active" → true`, `"age" → 30`, `"name" → "Alix"`), `commit()`, assert
/// all 3 properties present in BOTH stores with correct values. Exercises
/// the `GraphValue → LoroProperty` conversion paths (Bool/Integer/String are
/// shared variants; Float is exercised implicitly by Integer's i64 path).
#[test]
#[ignore = "P2T3-L2 scaffold: L3 implements the body"]
fn vertex_builder_multiple_properties() {
    let (app, db) = build_app();
    let node_id = app
        .create_vertex()
        .with_property("active", GraphValue::Bool(true))
        .with_property("age", GraphValue::Integer(30))
        .with_property("name", GraphValue::String("Alix".into()))
        .commit()
        .expect("commit succeeds");
    // TODO(P2T3-L3): assert_grafeo_has_vertex(&db, node_id, &[], &[("active", Bool(true)), ("age", Integer(30)), ("name", String("Alix"))]);
    // TODO(P2T3-L3): assert_loro_has_vertex(&doc, &loro_key, &[], &[("active", Bool(true)), ("age", Integer(30)), ("name", String("Alix"))]);
    let _ = (db, node_id);
    todo!("P2T3-L3: implement multi-property roundtrip")
}

/// Create a vertex with NO labels and NO properties, `commit()`, assert it
/// succeeds (sensible default behavior). The grafeo side accepts an empty
/// label slice + empty props iter (`Session::create_node_with_props(&[], [])`
/// → `Ok(NodeId)`); the Loro side writes a `VertexEntity` with empty
/// `labels: Vec::new()` + empty `properties: HashMap::new()` + default
/// `description: String::new()` (the `#[loro(text)]` field — Phase 2 Task 1).
#[test]
#[ignore = "P2T3-L2 scaffold: L3 implements the body"]
fn vertex_builder_empty_vertex() {
    let (app, db) = build_app();
    let node_id = app.create_vertex().commit().expect("empty vertex commits ok");
    // TODO(P2T3-L3): assert_grafeo_has_vertex(&db, node_id, &[], &[]);
    // TODO(P2T3-L3): assert_loro_has_vertex(&doc, &loro_key, &[], &[]);
    let _ = (db, node_id);
    todo!("P2T3-L3: implement empty-vertex roundtrip")
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
#[ignore = "P2T3-L2 scaffold: L3 implements the body"]
fn vertex_builder_atomicity_rollback_on_grafeo_failure() {
    let (app, _db) = build_app_with_tiny_property_limit();
    let result = app
        .create_vertex()
        .with_label("Person")
        .with_property("oversized", GraphValue::String("x".repeat(1024)))
        .commit();
    assert!(
        result.is_err(),
        "commit must fail when grafeo rejects the property size"
    );
    // TODO(P2T3-L3): re-acquire Loro read lock, assert V map is empty
    //                  (compensation deleted the loro_key entry).
    // TODO(P2T3-L3): assert app.maps().node_id_map.read().is_empty()
    //                  (BridgeMaps never inserted on Grafeo failure).
    todo!("P2T3-L3: implement atomicity rollback assertions")
}

/// Spawn 2+ `VertexBuilder`s from the same `GrafeoLoroApp`, commit
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
#[ignore = "P2T3-L2 scaffold: L3 implements the body"]
fn vertex_builder_concurrent_commit() {
    let (app, db) = build_app();
    // TODO(P2T3-L3): spawn 2 threads (or tokio tasks), each calls
    //                  `app.create_vertex().with_label("N").commit()` 10 times
    //                  (20 commits total). Collect (NodeId, loro_key) pairs.
    // TODO(P2T3-L3): assert 20 distinct NodeIds (grafeo-assigned).
    // TODO(P2T3-L3): assert 20 distinct loro_keys (AtomicU64 counter).
    // TODO(P2T3-L3): assert app.maps().node_id_map.read().len() == 20
    //                  (no BridgeMaps corruption).
    // TODO(P2T3-L3): assert app.maps().node_key_map.read().len() == 20
    //                  (forward and inverse maps in lock-step).
    let _ = (app, db);
    todo!("P2T3-L3: implement concurrent commit (2 threads × 10 commits = 20 unique pairs)")
}

/// `commit()` with a `GraphValue::Vector` property must return
/// `Err(UnsupportedLoroType(_))` BEFORE any Loro/Grafeo write (Q2 strict
/// reject — see `VertexBuilder` struct doc). The rejection happens at
/// `commit()` step 1, BEFORE the Loro write lock is acquired, so the Loro
/// `"V"` map must remain empty and `BridgeMaps` must be empty.
#[test]
#[ignore = "P2T3-L2 scaffold: L3 implements the body"]
fn vertex_builder_rejects_vector_property() {
    let (app, _db) = build_app();
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
    // TODO(P2T3-L3): assert Loro V map is empty (no write occurred).
    // TODO(P2T3-L3): assert app.maps().node_id_map.read().is_empty()
    //                  (no Grafeo write either).
    todo!("P2T3-L3: implement strict-reject assertions for Vector property")
}

/// `commit()` with a `GraphValue::Map` property must return
/// `Err(UnsupportedLoroType(_))` BEFORE any Loro/Grafeo write. See
/// `vertex_builder_rejects_vector_property` for the contract.
#[test]
#[ignore = "P2T3-L2 scaffold: L3 implements the body"]
fn vertex_builder_rejects_map_property() {
    let (app, _db) = build_app();
    let mut map = std::collections::HashMap::new();
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
    // TODO(P2T3-L3): assert Loro V map is empty (no write occurred).
    // TODO(P2T3-L3): assert app.maps().node_id_map.read().is_empty()
    //                  (no Grafeo write either).
    todo!("P2T3-L3: implement strict-reject assertions for Map property")
}

/// `commit()` with a `GraphValue::List` property must return
/// `Err(UnsupportedLoroType(_))` BEFORE any Loro/Grafeo write. See
/// `vertex_builder_rejects_vector_property` for the contract.
#[test]
#[ignore = "P2T3-L2 scaffold: L3 implements the body"]
fn vertex_builder_rejects_list_property() {
    let (app, _db) = build_app();
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
    // TODO(P2T3-L3): assert Loro V map is empty (no write occurred).
    // TODO(P2T3-L3): assert app.maps().node_id_map.read().is_empty()
    //                  (no Grafeo write either).
    todo!("P2T3-L3: implement strict-reject assertions for List property")
}
