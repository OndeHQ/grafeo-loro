//! Phase 2 Task 1 scaffolds: `lorosurgeon` derive roundtrips.
//! Pattern: `lorosurgeon-0.2.1/tests/integration.rs:151-162`.
//! Each `#[ignore]` stub is a contract for L3 to fill in.

use std::collections::HashMap;

use grafeo_loro::schema::{EdgeEntity, OrderedCollection, TreeNode, VertexEntity};
use grafeo_loro::types::LoroProperty;
use lorosurgeon::{Hydrate, Reconcile, RootReconciler};
use loro::LoroDoc;

// Isolated-entity pattern: `doc.get_map("root")` is the test fixture
// (matches upstream `lorosurgeon-0.2.1/tests/integration.rs:151-162`).
// Production path nests entities under registry keys (`doc.get_map("V").get_map(<NodeID>)`)
// per architecture §5; L3 must NOT copy this test pattern into the bridge.

/// `VertexEntity` roundtrips through a Loro `LoroMap`, exercising the
/// `#[loro(text)]` attribute on `description` (stored as `LoroText`, not a
/// scalar string).
#[test]
#[ignore = "P2-L2 scaffold: L3 adds deeper assertions (rich-text LCS)"]
fn vertex_entity_roundtrip() {
    let original = VertexEntity {
        labels: vec!["person".into()],
        properties: HashMap::from([("active".into(), LoroProperty::Bool(true))]),
        description: "hello".into(),
    };
    let doc = LoroDoc::new();
    let map = doc.get_map("root");
    original
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    let hydrated = VertexEntity::hydrate_map(&map).unwrap();
    assert_eq!(hydrated, original);
    // TODO(P2-L3): exercise char-level LCS on `description` (insert/delete
    // mid-string) and assert the diff is char-wise, not whole-string replace.
}

/// `EdgeEntity` roundtrips through a Loro `LoroMap`. Plain `String` fields +
/// `HashMap<String, LoroProperty>` properties (no special field attributes).
#[test]
#[ignore = "P2-L2 scaffold: L3 adds deeper assertions"]
fn edge_entity_roundtrip() {
    let original = EdgeEntity {
        label: "knows".into(),
        src: "n1".into(),
        dst: "n2".into(),
        properties: HashMap::from([("weight".into(), LoroProperty::Float(0.5))]),
    };
    let doc = LoroDoc::new();
    let map = doc.get_map("root");
    original
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    let hydrated = EdgeEntity::hydrate_map(&map).unwrap();
    assert_eq!(hydrated, original);
}

/// `OrderedCollection` roundtrips through a Loro `LoroMap`, exercising the
/// `#[loro(movable)]` attribute on `items` (stored as `LoroMovableList`).
/// Identity-preserving reorder behavior is verified separately by
/// `ordered_collection_reorder_preserves_identity`.
#[test]
#[ignore = "P2-L2 scaffold: L3 adds deeper assertions"]
fn ordered_collection_roundtrip() {
    let original = OrderedCollection {
        items: vec![
            TreeNode { node_id: "n1".into(), title: "Alpha".into() },
            TreeNode { node_id: "n2".into(), title: "Beta".into() },
        ],
    };
    let doc = LoroDoc::new();
    let map = doc.get_map("root");
    original
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    let hydrated = OrderedCollection::hydrate_map(&map).unwrap();
    assert_eq!(hydrated, original);
}

/// Architecture §7 contract: reordering items in an `OrderedCollection`
/// MUST emit `mov()` CRDT ops (identity-preserving), NOT delete+insert.
/// A regression to plain `Vec<TreeNode>` (no `#[loro(movable)]`) would
/// still round-trip correctly in isolation (Myers LCS) but would lose CRDT
/// element identity under concurrent reorders from multiple peers.
#[test]
#[ignore = "P2-L2 scaffold: L3 implements oplog diff inspection"]
fn ordered_collection_reorder_preserves_identity() {
    let abc = OrderedCollection {
        items: vec![
            TreeNode { node_id: "A".into(), title: "Alpha".into() },
            TreeNode { node_id: "B".into(), title: "Beta".into() },
            TreeNode { node_id: "C".into(), title: "Gamma".into() },
        ],
    };
    let cab = OrderedCollection {
        items: vec![
            TreeNode { node_id: "C".into(), title: "Gamma".into() },
            TreeNode { node_id: "A".into(), title: "Alpha".into() },
            TreeNode { node_id: "B".into(), title: "Beta".into() },
        ],
    };
    let doc = LoroDoc::new();
    let map = doc.get_map("root");
    abc.reconcile(RootReconciler::new(map.clone())).unwrap();
    doc.commit();
    let vv_before = doc.oplog_vv();
    cab.reconcile(RootReconciler::new(map.clone())).unwrap();
    doc.commit();
    // TODO(P2-L3): assert
    //   (a) `doc.oplog_vv()` advances (some op was emitted by the reorder).
    //   (b) `doc.export_from(vv_before)` yields a DiffBatch whose list-ops
    //       contain Move ops on the LoroMovableList — NOT delete+insert
    //       pairs (see `lorosurgeon-0.2.1/src/reconcile/movable_list.rs`).
    //   (c) Re-hydrate via `OrderedCollection::hydrate_map(&map)` and
    //       assert_eq!(hydrated, cab).
    drop(vv_before); // keep wiring live until L3 fills in (b)
}

/// Roundtrips a single `TreeNode` as a flat LoroMap. Does NOT exercise
/// `#[key]` (which only matters inside an `OrderedCollection`'s movable
/// list). Use `tree_node_key_extraction` and
/// `ordered_collection_reorder_preserves_identity` for the `#[key]`
/// contract.
#[test]
#[ignore = "P2-L2 scaffold: L3 adds deeper assertions"]
fn tree_node_flat_roundtrip() {
    let original = TreeNode { node_id: "n1".into(), title: "Alpha".into() };
    let doc = LoroDoc::new();
    let map = doc.get_map("root");
    original
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    let hydrated = TreeNode::hydrate_map(&map).unwrap();
    assert_eq!(hydrated, original);
}

/// Directly asserts `<TreeNode as Reconcile>::key()` returns
/// `LoadKey::Found(node_id)` — the contract that `OrderedCollection`'s
/// movable-list keyed diffing relies on.
#[test]
#[ignore = "P2-L2 scaffold: L3 verifies hydrate_key from a LoroMap source"]
fn tree_node_key_extraction() {
    use lorosurgeon::LoadKey;
    let tn = TreeNode { node_id: "n1".into(), title: "T".into() };
    assert_eq!(tn.key(), LoadKey::Found("n1".to_string()));
    // TODO(P2-L3): also verify `TreeNode::hydrate_key` extracts the key
    // from a LoroMap source (the Loro-side extraction used by the keyed
    // diffing path — see `lorosurgeon-0.2.1/src/reconcile/movable_list.rs`).
}

/// B1 (P2-DEVIL): `LoroProperty` MUST encode as a bare `LoroValue`, NOT a
/// tagged-union `LoroValue::Map({"Bool": true, ...})`. A regression to
/// `#[derive(Hydrate, Reconcile)]` would silently flip the wire shape while
/// all entity-roundtrip tests stay green (Goodhart's Law violation).
#[test]
#[ignore = "P2-L2 scaffold: L3 implements the multi-variant assertion loop"]
fn loro_property_encoding_roundtrip() {
    use lorosurgeon::PropReconciler;
    let doc = LoroDoc::new();
    let map = doc.get_map("root");
    // Wire one variant as the contract example.
    let prop = LoroProperty::Bool(true);
    let reconciler = PropReconciler::map_put(map.clone(), "k".to_string());
    prop.reconcile(reconciler).unwrap();
    doc.commit();
    let value = map.get("k").expect("wire: key written").get_deep_value();
    assert_eq!(value, loro::LoroValue::Bool(true));
    // TODO(P2-L3): extend to all 5 variants and assert each bare wire shape
    //   (NOT a tagged-union LoroValue::Map):
    //   Null       → LoroValue::Null
    //   Bool(b)    → LoroValue::Bool(b)
    //   Integer(i) → LoroValue::I64(i)
    //   Float(f)   → LoroValue::Double(f)
    //   String(s)  → LoroValue::String(s)
}
