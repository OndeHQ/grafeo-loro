//! Phase 2 Task 1 scaffolds: `lorosurgeon` derive roundtrips.
//! Pattern: `lorosurgeon-0.2.1/tests/integration.rs:151-162`.

use std::collections::HashMap;

use grafeo_loro::schema::{EdgeEntity, OrderedCollection, TreeNode, VertexEntity};
use grafeo_loro::types::LoroProperty;
use lorosurgeon::{Hydrate, LoadKey, PropReconciler, Reconcile, RootReconciler};
use loro::event::{Diff, ListDiffItem};
use loro::{Container, ExportMode, LoroDoc, LoroValue, TextDelta, ValueOrContainer};

// Isolated-entity pattern: `doc.get_map("root")` is the test fixture
// (matches upstream `lorosurgeon-0.2.1/tests/integration.rs:151-162`).
// Production path nests entities under registry keys (`doc.get_map("V").get_map(<NodeID>)`)
// per architecture §5; L3 must NOT copy this test pattern into the bridge.

/// `VertexEntity` roundtrips through a Loro `LoroMap`, exercising the
/// `#[loro(text)]` attribute on `description` (stored as `LoroText`, not a
/// scalar string). The follow-up mid-string mutation verifies that
/// `TextReconciler::update` (`lorosurgeon-0.2.1/src/reconcile.rs:408-416`)
/// emits char-level retain+insert deltas, NOT a whole-string replace.
#[test]
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

    // Char-level LCS on `description`: insert "x" mid-string ("hello" → "hexllo"),
    // re-reconcile, and inspect the oplog diff for a Retain-prefixed text delta.
    // A whole-string replace would emit `Delete{5}, Insert{"hexllo"}` with no Retain.
    let mutated = VertexEntity {
        description: "hexllo".into(),
        ..original
    };
    let before = doc.oplog_frontiers();
    mutated
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    let after = doc.oplog_frontiers();
    assert_ne!(before, after, "oplog frontiers must advance after mid-string text edit");

    let batch = doc.diff(&before, &after).expect("diff between adjacent frontiers");
    let mut saw_text_retain = false;
    for (_cid, diff) in batch.iter() {
        if let Diff::Text(deltas) = diff {
            assert!(
                deltas
                    .iter()
                    .any(|d| matches!(d, TextDelta::Retain { .. })),
                "text delta must retain chars (char-level LCS), got {deltas:?}",
            );
            assert!(
                !deltas
                    .iter()
                    .any(|d| matches!(d, TextDelta::Delete { delete } if *delete >= 5)),
                "text delta must not delete the entire original string (whole-string replace), got {deltas:?}",
            );
            saw_text_retain = true;
        }
    }
    assert!(saw_text_retain, "expected at least one Text diff in {batch:?}");

    let hydrated_mutated = VertexEntity::hydrate_map(&map).unwrap();
    assert_eq!(hydrated_mutated, mutated);
}

/// `EdgeEntity` roundtrips through a Loro `LoroMap`. Plain `String` fields +
/// `HashMap<String, LoroProperty>` properties (no special field attributes).
/// Adds a property-mutation case: insert edge → mutate property → re-reconcile
/// → assert hydrate equals mutated original.
#[test]
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

    // Mutate one property and re-reconcile. The no-op detection on
    // unchanged fields (`PropReconciler::put_value`, reconcile.rs:179-194)
    // skips label/src/dst; only the changed `weight` emits an op.
    let mutated = EdgeEntity {
        properties: {
            let mut p = original.properties.clone();
            p.insert("weight".into(), LoroProperty::Float(0.9));
            p.insert("since".into(), LoroProperty::Integer(2024));
            p
        },
        ..original.clone()
    };
    mutated
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    let hydrated_mutated = EdgeEntity::hydrate_map(&map).unwrap();
    assert_eq!(hydrated_mutated, mutated);
    assert_ne!(hydrated_mutated, original);
}

/// `OrderedCollection` roundtrips through a Loro `LoroMap`, exercising the
/// `#[loro(movable)]` attribute on `items` (stored as `LoroMovableList`).
/// Non-trivial case: 3+ items, append + prepend + middle insert in sequence.
/// Identity-preserving reorder behavior is verified separately by
/// `ordered_collection_reorder_preserves_identity`.
#[test]
fn ordered_collection_roundtrip() {
    let doc = LoroDoc::new();
    let map = doc.get_map("root");

    // 1) Append: empty → [n1, n2].
    let initial = OrderedCollection {
        items: vec![
            TreeNode { node_id: "n1".into(), title: "Alpha".into() },
            TreeNode { node_id: "n2".into(), title: "Beta".into() },
        ],
    };
    initial
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    assert_eq!(OrderedCollection::hydrate_map(&map).unwrap(), initial);

    // 2) Append n3: [n1, n2] → [n1, n2, n3].
    let appended = OrderedCollection {
        items: initial
            .items
            .iter()
            .cloned()
            .chain([TreeNode { node_id: "n3".into(), title: "Gamma".into() }])
            .collect(),
    };
    appended
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    assert_eq!(OrderedCollection::hydrate_map(&map).unwrap(), appended);

    // 3) Prepend n0: [n1, n2, n3] → [n0, n1, n2, n3].
    let prepended = OrderedCollection {
        items: [TreeNode { node_id: "n0".into(), title: "Zero".into() }]
            .into_iter()
            .chain(appended.items.iter().cloned())
            .collect(),
    };
    prepended
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    assert_eq!(OrderedCollection::hydrate_map(&map).unwrap(), prepended);

    // 4) Middle insert n1a at index 1: [n0, n1, n2, n3] → [n0, n1a, n1, n2, n3].
    let middle_inserted = OrderedCollection {
        items: prepended
            .items
            .iter()
            .take(1)
            .cloned()
            .chain([TreeNode { node_id: "n1a".into(), title: "Alpha-prime".into() }])
            .chain(prepended.items.iter().skip(1).cloned())
            .collect(),
    };
    middle_inserted
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    assert_eq!(OrderedCollection::hydrate_map(&map).unwrap(), middle_inserted);
    assert_eq!(middle_inserted.items.len(), 5);
}

/// Architecture §7 contract: reordering items in an `OrderedCollection`
/// MUST emit `mov()` CRDT ops (identity-preserving), NOT delete+insert.
/// A regression to plain `Vec<TreeNode>` (no `#[loro(movable)]`) would
/// still round-trip correctly in isolation (Myers LCS) but would lose CRDT
/// element identity under concurrent reorders from multiple peers.
#[test]
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

    // (a) oplog_vv advances; (b) DiffBatch contains Move ops, not delete+insert.
    let vv_before = doc.oplog_vv();
    let f_before = doc.oplog_frontiers();
    cab.reconcile(RootReconciler::new(map.clone())).unwrap();
    doc.commit();
    let vv_after = doc.oplog_vv();
    let f_after = doc.oplog_frontiers();
    assert_ne!(vv_before, vv_after, "oplog vv must advance after reorder");

    let batch = doc.diff(&f_before, &f_after).expect("diff between adjacent frontiers");
    // Walk every container delta in the batch. For the LoroMovableList, expect
    // at least one `ListDiffItem::Insert { is_move: true, .. }` (a Move op)
    // and zero `ListDiffItem::Insert { is_move: false, .. }` (a non-move
    // insert, which together with a Delete would constitute a delete+insert
    // pattern that loses CRDT element identity).
    // See `lorosurgeon-0.2.1/src/reconcile/movable_list.rs:113-202` for the
    // keyed-diffing dispatch that emits `mov()` ops for matched items.
    let mut saw_move = false;
    let mut saw_non_move_insert = false;
    for (_cid, diff) in batch.iter() {
        if let Diff::List(items) = diff {
            for item in items {
                match item {
                    ListDiffItem::Insert { is_move: true, .. } => saw_move = true,
                    ListDiffItem::Insert { is_move: false, .. } => saw_non_move_insert = true,
                    ListDiffItem::Delete { .. } | ListDiffItem::Retain { .. } => {}
                }
            }
        }
    }
    assert!(saw_move, "expected at least one Move op (is_move=true), got batch: {batch:?}");
    assert!(
        !saw_non_move_insert,
        "expected zero non-move Inserts (delete+insert pattern), got batch: {batch:?}",
    );

    // (c) Re-hydrate + assert_eq to the reordered collection.
    let hydrated = OrderedCollection::hydrate_map(&map).unwrap();
    assert_eq!(hydrated, cab);
}

/// Roundtrips a single `TreeNode` as a flat LoroMap. Does NOT exercise
/// `#[key]` (which only matters inside an `OrderedCollection`'s movable
/// list). Use `tree_node_key_extraction` and
/// `ordered_collection_reorder_preserves_identity` for the `#[key]`
/// contract.
///
/// Also exercises a field-level concurrent merge across two `LoroDoc` peers
/// (the Loro CRDT core use case). Two peers diverge by mutating DIFFERENT
/// fields of the same flat LoroMap; after both-way import/export, both
/// converge to the union of both field changes.
#[test]
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

    // Field-level concurrent merge: peer A mutates `node_id`, peer B mutates
    // `title`. The reconcile re-writes the whole struct, but no-op detection
    // (reconcile.rs:179-194) skips the unchanged field, so each peer emits
    // only one field-level op. After both-way sync, both peers have the
    // union of both field changes — the defining property of Loro's CRDT.
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let map_a = doc_a.get_map("root");
    original
        .clone()
        .reconcile(RootReconciler::new(map_a.clone()))
        .unwrap();
    doc_a.commit();
    let a_bytes = doc_a.export(ExportMode::all_updates()).unwrap();

    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    doc_b.import(&a_bytes).unwrap();
    let map_b = doc_b.get_map("root");
    assert_eq!(
        TreeNode::hydrate_map(&map_b).unwrap(),
        original,
        "peer B must match peer A after initial sync",
    );

    // Peer A mutates `node_id`; `title` unchanged → no-op skip.
    let a_mut = TreeNode { node_id: "n1A".into(), title: original.title.clone() };
    a_mut
        .clone()
        .reconcile(RootReconciler::new(map_a.clone()))
        .unwrap();
    doc_a.commit();

    // Peer B mutates `title`; `node_id` unchanged → no-op skip.
    let b_mut = TreeNode { node_id: original.node_id.clone(), title: "Bravo".into() };
    b_mut
        .clone()
        .reconcile(RootReconciler::new(map_b.clone()))
        .unwrap();
    doc_b.commit();

    // Both-way sync: each peer imports the other's updates.
    let a_to_b = doc_a.export(ExportMode::all_updates()).unwrap();
    let b_to_a = doc_b.export(ExportMode::all_updates()).unwrap();
    doc_a.import(&b_to_a).unwrap();
    doc_b.import(&a_to_b).unwrap();

    // Both peers converge to the union of field-level changes.
    let merged = TreeNode { node_id: "n1A".into(), title: "Bravo".into() };
    assert_eq!(TreeNode::hydrate_map(&map_a).unwrap(), merged, "peer A must converge");
    assert_eq!(TreeNode::hydrate_map(&map_b).unwrap(), merged, "peer B must converge");
}

/// Directly asserts `<TreeNode as Reconcile>::key()` returns
/// `LoadKey::Found(node_id)` (Rust-side extraction) AND
/// `TreeNode::hydrate_key` extracts the same key from a LoroMap source
/// (the Loro-side extraction used by keyed diffing — see
/// `lorosurgeon-0.2.1/src/reconcile/movable_list.rs:113-127`).
#[test]
fn tree_node_key_extraction() {
    let tn = TreeNode { node_id: "n1".into(), title: "T".into() };
    assert_eq!(tn.key(), LoadKey::Found("n1".to_string()));

    // Reconcile into a LoroMap, then exercise the derived `hydrate_key`
    // (lorosurgeon-derive-0.2.1/src/reconcile/struct_impl.rs:136-156) — it
    // matches `ValueOrContainer::Container(Container::Map(map))` and reads
    // the `#[key]` field. This is the exact path used by `reconcile_keyed`
    // to match old/new list items by identity.
    let doc = LoroDoc::new();
    let map = doc.get_map("root");
    tn.clone()
        .reconcile(RootReconciler::new(map.clone()))
        .unwrap();
    doc.commit();
    let voc = ValueOrContainer::Container(Container::Map(map));
    let loaded = TreeNode::hydrate_key(&voc).unwrap();
    assert_eq!(loaded, LoadKey::Found("n1".to_string()));
}

/// B1 (P2-DEVIL): `LoroProperty` MUST encode as a bare `LoroValue`, NOT a
/// tagged-union `LoroValue::Map({"Bool": true, ...})`. A regression to
/// `#[derive(Hydrate, Reconcile)]` would silently flip the wire shape while
/// all entity-roundtrip tests stay green (Goodhart's Law violation).
///
/// Exercises all 5 variants (`Null`/`Bool`/`Integer`/`Float`/`String`) and
/// asserts each bare wire shape — NOT a tagged-union `LoroValue::Map`.
#[test]
fn loro_property_encoding_roundtrip() {
    // (variant_name, LoroProperty, expected bare LoroValue)
    let cases: [(&str, LoroProperty, LoroValue); 5] = [
        ("Null", LoroProperty::Null, LoroValue::Null),
        ("Bool", LoroProperty::Bool(true), LoroValue::Bool(true)),
        ("Integer", LoroProperty::Integer(42), LoroValue::I64(42)),
        ("Float", LoroProperty::Float(3.14), LoroValue::Double(3.14)),
        (
            "String",
            LoroProperty::String("hi".into()),
            LoroValue::String("hi".into()),
        ),
    ];

    for (name, prop, expected) in cases {
        let doc = LoroDoc::new();
        let map = doc.get_map("root");
        let reconciler = PropReconciler::map_put(map.clone(), "k".to_string());
        prop.reconcile(reconciler).unwrap();
        doc.commit();
        let value = map
            .get("k")
            .unwrap_or_else(|| panic!("wire: {name} key written"))
            .get_deep_value();
        assert_eq!(
            value, expected,
            "{name}: wire shape must be the bare scalar",
        );
        assert!(
            !matches!(value, LoroValue::Map(_)),
            "{name}: wire shape must NOT be a tagged-union LoroValue::Map (Goodhart defense)",
        );
    }
}
