//! Phase 2 Task 1 scaffolds: roundtrip tests for the `lorosurgeon` derives
//! on `VertexEntity`, `EdgeEntity`, `OrderedCollection`, and `TreeNode`.
//!
//! **L1 contract only** — function signatures are defined here as a
//! cheatsheet for L3; bodies are `todo!()` and tests carry `#[ignore]` so
//! `cargo test` does not panic on them. L3 must drop in real bodies and
//! remove the `#[ignore]` attributes.
//!
//! Each test exercises the same roundtrip pattern (architecture §6 + §7):
//!   1. Construct a Rust struct.
//!   2. Reconcile it into a fresh `LoroDoc` root `LoroMap` via
//!      `lorosurgeon::RootReconciler`.
//!   3. `doc.commit()`.
//!   4. Hydrate the struct back via `<T as Hydrate>::hydrate_map(&map)`.
//!   5. `assert_eq!(roundtripped, original)`.
//!
//! The `#[loro(text)]` (on `VertexEntity::description`), `#[loro(movable)]`
//! (on `OrderedCollection::items`), and `#[key]` (on `TreeNode::node_id`)
//! attributes are exercised by the corresponding tests below.

#![allow(missing_docs)]

use grafeo_loro::schema::{EdgeEntity, OrderedCollection, TreeNode, VertexEntity};

/// `VertexEntity` roundtrips through a Loro `LoroMap`, exercising the
/// `#[loro(text)]` attribute on `description` (stored as `LoroText`, not a
/// scalar string).
#[test]
#[ignore = "P2-L1 scaffold: L3 implements the body"]
fn vertex_entity_roundtrip() {
    let _ = std::marker::PhantomData::<VertexEntity>;
    todo!()
}

/// `EdgeEntity` roundtrips through a Loro `LoroMap`. No special field
/// attributes — plain `Vec<String>` labels + `HashMap<String, LoroProperty>`.
#[test]
#[ignore = "P2-L1 scaffold: L3 implements the body"]
fn edge_entity_roundtrip() {
    let _ = std::marker::PhantomData::<EdgeEntity>;
    todo!()
}

/// `OrderedCollection` roundtrips through a Loro `LoroMap`, exercising the
/// `#[loro(movable)]` attribute on `items` (stored as `LoroMovableList`,
/// not `LoroList`). L3 should also assert that reordering items produces
/// `mov()` CRDT ops rather than delete+insert (identity preservation).
#[test]
#[ignore = "P2-L1 scaffold: L3 implements the body"]
fn ordered_collection_roundtrip() {
    let _ = std::marker::PhantomData::<OrderedCollection>;
    todo!()
}

/// `TreeNode` roundtrips through a Loro `LoroMap`, exercising the `#[key]`
/// attribute on `node_id`. L3 should also assert that
/// `<TreeNode as Reconcile>::key()` returns `LoadKey::Found(node_id)` so
/// the movable-list diffing in `OrderedCollection` can match items by id.
#[test]
#[ignore = "P2-L1 scaffold: L3 implements the body"]
fn tree_node_roundtrip() {
    let _ = std::marker::PhantomData::<TreeNode>;
    todo!()
}
