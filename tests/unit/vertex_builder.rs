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
//! ## Test fixture
//!
//! Each test constructs a fresh `SyncEngine` (which holds a fresh `LoroDoc` +
//! fresh in-memory `GrafeoDB`) and wraps it in a `GrafeoLoroApp`. L3 must
//! either:
//! - (a) add a `pub fn new_for_testing(sync_engine: Arc<SyncEngine>) -> GrafeoLoroApp`
//!   constructor on `GrafeoLoroApp`, OR
//! - (b) implement `GrafeoLoroAppBuilder::build` (Phase 4 scope) — overly heavy
//!   for unit tests.
//!
//! Option (a) is the recommended path (matches P2T2's `build_chain_fixture`
//! pattern of test-only construction).
//!
//! ## Reading back from Loro
//!
//! Use `<VertexEntity as Hydrate>::hydrate_map(&LoroMap)` (Phase 2 Task 1
//! verified at `lorosurgeon-0.2.1/src/hydrate.rs:64`) on the per-vertex nested
//! map at `doc.get_map("V").get_map(loro_key)`. The `loro_key` is recovered
//! from the grafeo `NodeId` via `BridgeMaps::node_key_map` (since `commit()`
//! returns the grafeo `NodeId`, not the `loro_key` — see open question #1 in
//! the worklog).
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
//! - `session.get_node(NodeId) -> Option<Node>` — `session/mod.rs:5138`
//! - `Node::labels: SmallVec<[ArcStr; 2]>` — `grafeo-core-0.5.42/src/graph/lpg/node.rs:34`
//! - `Node::properties: PropertyMap` — `grafeo-core-0.5.42/src/graph/lpg/node.rs:36`
//! - `Node::has_label(&str) -> bool` — `grafeo-core-0.5.42/src/graph/lpg/node.rs:80`
//! - `Node::get_property(&str) -> Option<&Value>` — `grafeo-core-0.5.42/src/graph/lpg/node.rs:91`
//!
//! # Loro API (verified against `loro-1.13.6/src/lib.rs`)
//!
//! - `LoroDoc::new() -> Self` — `lib.rs:137`
//! - `doc.get_map<I: IntoContainerId>(&self, I) -> LoroMap` — `lib.rs:489`
//!
//! # lorosurgeon API (verified against `lorosurgeon-0.2.1/src/`)
//!
//! - `<T as Hydrate>::hydrate_map(&LoroMap) -> Result<T, HydrateError>` — `hydrate.rs:64`

#![allow(missing_docs)]

/// Create a vertex with 1 label (`"Person"`) + 1 property (`"name" →
/// "Alix"`), `commit()`, read back from BOTH Loro AND Grafeo, assert labels +
/// properties match in both stores. Anti-Goodhart: assert BOTH stores, not
/// just one (catches Loro-only or Grafeo-only regressions).
#[test]
#[ignore = "P2T3-L1 scaffold: L3 implements the body"]
fn vertex_builder_basic_roundtrip() {
    // let app = build_app();
    // let node_id = app.create_vertex()
    //     .with_label("Person")
    //     .with_property("name", GraphValue::String("Alix".into()))
    //     .commit()
    //     .expect("commit succeeds");
    // let loro_key = app.sync_engine.maps().node_key_map.read().get(&node_id)
    //     .cloned().expect("BridgeMaps has binding");
    // assert_grafeo_has_vertex(&db, node_id, &["Person"], &[("name", GraphValue::String("Alix".into()))]);
    // assert_loro_has_vertex(&doc, &loro_key, &["Person".to_string()], &[("name".into(), LoroProperty::String("Alix".into()))]);
    todo!("P2T3-L3: implement basic roundtrip — see doc-comment + worklog for fixture strategy")
}

/// Create a vertex with 3 labels (`"Person"`, `"Admin"`, `"Engineer"`),
/// `commit()`, assert all 3 labels present in BOTH stores. Grafeo supports
/// multi-label nodes natively (`Node::labels: SmallVec<[ArcStr; 2]>` —
/// `grafeo-core-0.5.42/src/graph/lpg/node.rs:34`); Loro-side `VertexEntity`
/// stores labels as a `Vec<String>` (Phase 2 Task 1 verified).
#[test]
#[ignore = "P2T3-L1 scaffold: L3 implements the body"]
fn vertex_builder_multiple_labels() {
    // let app = build_app();
    // let node_id = app.create_vertex()
    //     .with_label("Person")
    //     .with_label("Admin")
    //     .with_label("Engineer")
    //     .commit()
    //     .expect("commit succeeds");
    // assert_grafeo_has_vertex(&db, node_id, &["Person", "Admin", "Engineer"], &[]);
    // assert_loro_has_vertex(&doc, &loro_key, &["Person", "Admin", "Engineer"], &[]);
    todo!("P2T3-L3: implement multi-label roundtrip")
}

/// Create a vertex with 3 properties covering `Bool`, `Integer`, `String`
/// (`"active" → true`, `"age" → 30`, `"name" → "Alix"`), `commit()`, assert
/// all 3 properties present in BOTH stores with correct values. Exercises
/// the `GraphValue → LoroProperty` conversion paths (Bool/Integer/String are
/// shared variants; Float is exercised implicitly by Integer's i64 path).
#[test]
#[ignore = "P2T3-L1 scaffold: L3 implements the body"]
fn vertex_builder_multiple_properties() {
    // let app = build_app();
    // let node_id = app.create_vertex()
    //     .with_property("active", GraphValue::Bool(true))
    //     .with_property("age", GraphValue::Integer(30))
    //     .with_property("name", GraphValue::String("Alix".into()))
    //     .commit()
    //     .expect("commit succeeds");
    // assert_grafeo_has_vertex(..., &[], &[("active", Bool(true)), ("age", Integer(30)), ("name", String("Alix"))]);
    // assert_loro_has_vertex(..., &[], &[("active", Bool(true)), ("age", Integer(30)), ("name", String("Alix"))]);
    todo!("P2T3-L3: implement multi-property roundtrip")
}

/// Create a vertex with NO labels and NO properties, `commit()`, assert it
/// succeeds (sensible default behavior). The grafeo side accepts an empty
/// label slice + empty props iter (`Session::create_node_with_props(&[], [])`
/// → `Ok(NodeId)`); the Loro side writes a `VertexEntity` with empty
/// `labels: Vec::new()` + empty `properties: HashMap::new()` + default
/// `description: String::new()` (the `#[loro(text)]` field — Phase 2 Task 1).
#[test]
#[ignore = "P2T3-L1 scaffold: L3 implements the body"]
fn vertex_builder_empty_vertex() {
    // let app = build_app();
    // let node_id = app.create_vertex().commit().expect("empty vertex commits ok");
    // assert_grafeo_has_vertex(..., &[], &[]);
    // assert_loro_has_vertex(..., &[], &[]);
    todo!("P2T3-L3: implement empty-vertex roundtrip")
}

/// Force a grafeo failure mid-`commit()` and assert Loro state is rolled back
/// (atomicity contract Option a — see `VertexBuilder` struct doc).
///
/// Mock strategy options for L3 (pick one):
/// - (1) Wrap `GrafeoDB` in a thin mock that returns `Err` on
///   `create_node_with_props`. Requires a trait abstraction grafeo-loro does
///   NOT currently have (YAGNI to add for one test).
/// - (2) Trigger a real grafeo failure by violating a constraint
///   (e.g. set `max_property_size` config to 1 byte and pass a property
///   value larger than that — `Session::create_node_with_props` calls
///   `check_property_size` at `session/mod.rs:4892` and returns
///   `Err` if exceeded). L3 must verify `GrafeoDB::with_config(Config::...)`
///   exposes this knob.
/// - (3) Drop the `GrafeoDB` Arc mid-commit (e.g. via `Arc::try_unwrap` or
///   `mem::forget` shenanigans). Brittle; not recommended.
///
/// Option (2) is preferred — no mock infrastructure, exercises a real grafeo
/// code path. L3 must verify `Config` exposes `max_property_size` (or similar)
/// and document the exact knob used.
///
/// Assertion: after `commit()` returns `Err`, the Loro `"V"` root map must
/// NOT contain the `loro_key` (compensation deleted it) and `BridgeMaps` must
/// NOT contain the binding (never inserted on Grafeo failure).
#[test]
#[ignore = "P2T3-L1 scaffold: L3 implements the body"]
fn vertex_builder_atomicity_rollback_on_grafeo_failure() {
    // let app = build_app();
    // let result = app.create_vertex()
    //     .with_label("Person")
    //     .with_property("oversized", GraphValue::String("x".repeat(1024)))
    //     .commit();
    // assert!(result.is_err(), "commit must fail when grafeo rejects the property size");
    // let v_map = doc.get_map("V");
    // assert!(v_map.len() == 0, "Loro V map must be empty after rollback (atomicity contract)");
    // assert!(sync_engine.maps().node_id_map.read().is_empty(), "BridgeMaps must be empty after rollback");
    todo!("P2T3-L3: implement atomicity rollback test (use Config::max_property_size to force grafeo failure — option 2)")
}
