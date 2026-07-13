# P2-L1 Devil's Advocate Critique

**Task ID**: P2-DEVIL
**Agent**: Devil's Advocate
**Branch**: `p2-derives`
**Target**: P2-L1 Scaffolder output for Phase 2 Task 1 (Wire `lorosurgeon` derives)
**Critique artifact**: this file
**Method**: read-only verification against `lorosurgeon-0.2.1` source in `~/.cargo/registry/`, `loro-1.13.6` source, `grafeo-0.5.42` source, and `docs/grafeo-loro.architecture.md` §5 / §6 / §7 / §27.

---

## 0. Verification matrix (every L1 claim re-checked independently)

| L1 claim | Verification | Result | Citation |
|---|---|---|---|
| `cargo check --all-targets` exit 0 | Re-ran | ✅ PASS, 0 errors, 5 pre-existing lib warnings (`hydration/vector.rs`, `presence/socket.rs`, `telemetry/health.rs` — all Phase-1 dead_code) | `cargo check --all-targets 2>&1 \| tail -30` |
| `cargo test --no-run --all` compiles all 3 test binaries | Re-ran | ✅ PASS, 3 executables emitted (`unittests`, `integration-…`, `unit-…`) | `cargo test --no-run --all 2>&1 \| tail -30` |
| `Cargo.toml` pins `lorosurgeon = "0.2"` and resolves to `0.2.1` | `grep -n lorosurgeon Cargo.lock` + `cargo tree -i lorosurgeon` | ✅ `lorosurgeon v0.2.1` (Cargo.lock:1202-1213) and `lorosurgeon-derive v0.2.1` (Cargo.lock:1216-1224) | Cargo.lock:1202, 1216 |
| `0.3` does not exist on crates.io | `cargo search lorosurgeon` | ✅ latest published = `0.2.1`; `0.3` not published | `cargo search lorosurgeon` output |
| `#[key]` field attribute exists | `attrs.rs:19, 96, 102-105` | ✅ sets `FieldAttrs::is_key = true` | `lorosurgeon-derive-0.2.1/src/attrs.rs:19,96,102-105` |
| `#[loro(text)]` field attribute exists | `attrs.rs:24, 132-133` | ✅ sets `FieldAttrs::text = true` | `lorosurgeon-derive-0.2.1/src/attrs.rs:24,132-133` |
| `#[loro(movable)]` field attribute exists | `attrs.rs:23, 128-130` | ✅ sets `FieldAttrs::movable = true` | `lorosurgeon-derive-0.2.1/src/attrs.rs:23,128-130` |
| `RootReconciler::new(LoroMap)` exists | `reconcile.rs:297-300` | ✅ `pub fn new(map: LoroMap) -> Self` | `lorosurgeon-0.2.1/src/reconcile.rs:297-300` |
| `<T as Hydrate>::hydrate_map(&LoroMap)` exists | `hydrate.rs:64` (method), `:127` (free fn) | ✅ both exist | `lorosurgeon-0.2.1/src/hydrate.rs:64,127` |
| `Reconcile::key() -> LoadKey<Self::Key>` exists | `reconcile.rs:95` | ✅ default returns `LoadKey::NoKey`; derived `#[key]` overrides to `LoadKey::Found(...)` | `lorosurgeon-0.2.1/src/reconcile.rs:87-104` |
| `LoadKey::Found(K)` exists | `reconcile.rs:51-58` | ✅ enum has `NoKey / KeyNotFound / Found(K)` | `lorosurgeon-0.2.1/src/reconcile.rs:51-58` |
| The chosen roundtrip pattern (`RootReconciler::new(map)` + `T::hydrate_map(&map)`) is the canonical one | Cross-checked against lorosurgeon's own integration tests | ✅ identical pattern at `lorosurgeon-0.2.1/tests/integration.rs:151-162` | upstream test cite |
| `DocSync` trait requires `#[loro(root = "key")]` — unavailable on the 4 entities | `doc_sync.rs:13-29` | ✅ confirmed; `DocSync::from_doc/to_doc` only generated when `ContainerAttrs::root` is set; the 4 entities deliberately omit it | `lorosurgeon-0.2.1/src/doc_sync.rs:13-29` + `derive-0.2.1/src/reconcile/struct_impl.rs:21-29` |
| `Vec<T>` + `#[loro(movable)]` + items with `#[key]` triggers keyed diffing (mov ops, not delete+insert) | `reconcile/movable_list.rs:57-73` + `reconcile.rs:394-396` | ✅ `reconcile_movable_list` checks `has_keys` via `item.key()` and dispatches to `reconcile_keyed` (uses `mov()` + `set()`) or `reconcile_positional` (positional `set`/`insert`/`delete`) | `lorosurgeon-0.2.1/src/reconcile/movable_list.rs:57-202` |

**L1's verification bar**: HIGH. Every claim checked out. No hallucination, no Goodhart, no happy-path bias. The L1 worklog entry is among the most rigorous in the entire worklog and matches the Phase 1 Devil's depth standard.

---

## 1. Findings — by severity

### BLOCKER (1)

#### B1 — `LoroProperty` manual `Hydrate`/`Reconcile` impls are not isolated-tested; a silent regression to tagged-union encoding would only surface as a cryptic entity-roundtrip failure

**Context**: The Phase 1 orchestrator (Gap 2 decision, worklog.md:172-174) approved manual `Hydrate`/`Reconcile` impls on `LoroProperty` to emit **bare** `LoroValue`s (`Bool(true)` ↔ `LV::Bool(true)`), explicitly rejecting the derive-generated tagged-union encoding (`{ "Bool": true }`). The manual impls exist at `src/types/values.rs:39-71`. The Phase 1 hunter verified they exist (worklog.md:297). **No test in the codebase verifies the encoding shape directly.**

**Why this is a BLOCKER for Phase 2 Task 1**: L1's `vertex_entity_roundtrip` and `edge_entity_roundtrip` scaffolds transitively exercise `LoroProperty` (via `properties: HashMap<String, LoroProperty>`), but if a future L2/L3 re-introduces `#[derive(Hydrate, Reconcile)]` on `LoroProperty` (a 1-line regression), the entity roundtrip would fail with `HydrateError::unexpected("scalar", "inline collection")` (per `hydrate.rs:56-58`) — a cryptic error that doesn't pinpoint the regression. Worse, if the regression happens to align with the derived encoding, the test would still pass with the wrong wire format (the round-trip would succeed because both sides use the tagged-union form), violating the orchestrator's bare-value decision **silently**.

This is an anti-plenger **Goodhart's Law** violation: green tests, broken system. The Phase 2 task is explicitly "verify derives compile + test roundtrip" — the roundtrip must verify the **wire shape**, not just the Rust-side equality.

**Concrete solution**: Add a 5th scaffold `loro_property_encoding_roundtrip()` to `tests/unit/schema_roundtrip.rs`:

```rust
/// Locks in the Phase 1 orchestrator decision (Gap 2): `LoroProperty` must
/// encode as **bare** `LoroValue`s, NOT as derive-default tagged-union LoroMaps.
/// A regression to `#[derive(Hydrate, Reconcile)]` on `LoroProperty` would
/// flip `Bool(true)` → `LoroMap { "Bool": true }` and silently double the
/// wire size + break property lookups.
#[test]
#[ignore = "P2-L1 scaffold: L3 implements the body"]
fn loro_property_encoding_roundtrip() {
    use grafeo_loro::types::values::LoroProperty;
    use lorosurgeon::{Hydrate, Reconcile, RootReconciler};
    use std::collections::HashMap;

    // 1. Construct a HashMap with all 5 LoroProperty variants.
    let mut props = HashMap::new();
    props.insert("n".to_string(), LoroProperty::Null);
    props.insert("b".to_string(), LoroProperty::Bool(true));
    props.insert("i".to_string(), LoroProperty::Integer(42));
    props.insert("f".to_string(), LoroProperty::Float(3.14));
    props.insert("s".to_string(), LoroProperty::String("hi".into()));

    // 2. Reconcile into a fresh LoroDoc.
    let doc = loro::LoroDoc::new();
    let map = doc.get_map("props");
    let r = RootReconciler::new(map.clone());
    props.reconcile(r).unwrap();
    doc.commit();

    // 3. L3 MUST assert the bare-value wire shape (not just Rust equality):
    //    - props.b == LoroValue::Bool(true)         (bare)
    //    - props.b != LoroValue::Map({"Bool": true}) (tagged regression)
    //    - props.i == LoroValue::I64(42)
    //    - props.f == LoroValue::Double(3.14)
    //    - props.s == LoroValue::String("hi")
    //    - props.n == LoroValue::Null
    // 4. Hydrate back and assert_eq!(roundtripped, props).
    todo!()
}
```

**L2/L3 effort**: ~15 LOC for the body. Adds a 5th `#[ignore]` stub.

---

### MAJOR (3)

#### M1 — `OrderedCollection` identity-preservation (the entire point of `#[loro(movable)]` + `#[key]`) has no dedicated scaffold; only a side-comment

**Context**: Architecture §7 (line 227) says `OrderedCollection` exists to "model ordered structural lists ... without duplicate conflicts" and "Prevents interleaving during drag-drops". The mechanism is: `Vec<TreeNode>` + `#[loro(movable)]` + `TreeNode.node_id: #[key]` triggers `reconcile_keyed` (movable_list.rs:113-202) which emits `mov()` CRDT ops for reorders instead of delete+insert.

L1's `ordered_collection_roundtrip` scaffold doc says "L3 should also assert that reordering items produces `mov()` CRDT ops rather than delete+insert (identity preservation)" — but this is a side-comment, not a separate scaffold. There is no `#[test]` stub for the identity-preservation property. Without it, L3 could implement a green roundtrip that uses delete+insert (the LoroList path) and the test would pass — another Goodhart violation.

**Concrete solution**: Add a 6th scaffold `ordered_collection_reorder_preserves_identity()`:

```rust
/// Architecture §7 contract: reordering items in an `OrderedCollection`
/// MUST emit `mov()` CRDT ops (identity-preserving), NOT delete+insert.
/// A regression to `Vec<TreeNode>` without `#[loro(movable)]` would still
/// round-trip correctly in isolation (Myers LCS) but would lose CRDT element
/// identity under concurrent reorders from multiple peers.
#[test]
#[ignore = "P2-L1 scaffold: L3 implements the body — requires oplog diff inspection"]
fn ordered_collection_reorder_preserves_identity() {
    use grafeo_loro::schema::{OrderedCollection, TreeNode};
    use lorosurgeon::{Hydrate, Reconcile, RootReconciler};

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

    let doc = loro::LoroDoc::new();
    let map = doc.get_map("oc");

    // Reconcile abc, commit, capture oplog vv.
    abc.reconcile(RootReconciler::new(map.clone())).unwrap();
    doc.commit();
    let vv_before = doc.oplog_vv();

    // Reconcile cab (same items, reordered). L3 MUST assert:
    //   (a) doc.oplog_vv() changes (some op was emitted).
    //   (b) The oplog between vv_before and now contains TreeMove/Move ops
    //       on the LoroMovableList — NOT delete+insert pairs.
    //   (c) <TreeNode as Reconcile>::key() on each item returns
    //       LoadKey::Found(node_id) — verified by inspecting the keyed
    //       diffing path (movable_list.rs:113-202).
    //   (d) Re-hydrate and assert_eq!(roundtripped, cab).
    cab.reconcile(RootReconciler::new(map.clone())).unwrap();
    doc.commit();
    todo!()
}
```

**L2/L3 effort**: ~25 LOC for the body. The oplog-diff inspection is non-trivial (L3 will need `doc.export_from(vv_before)` + walk the `DiffBatch`). Leaving it as `todo!()` is fine — the scaffold locks in the contract.

---

#### M2 — Architecture §5 vs §7 conflation: `T_CHILD` (LoroTree) and `OrderedCollection` (LoroMovableList) are different concepts; `TreeNode` belongs to the latter, but the doc/code conflate them

**Context**:

- Architecture §5 (line 164): `"T_CHILD" (LoroTree) → Strict Spanning Tree (Prevents Move Cycles) → TreeNodes → Identifiers mapping to Vertex IDs`.
- Architecture §7 (lines 229-244): `OrderedCollection { #[loro(movable)] items: Vec<PlaylistItem> }` — a `LoroMovableList`, NOT a `LoroTree`.
- `src/schema/tree.rs:6-9`: `OrderedCollection { #[loro(movable)] items: Vec<TreeNode> }` — a `LoroMovableList`.
- `src/schema/tree.rs:11-16`: `TreeNode { #[key] node_id: String, title: String }` — flat, no `parent_id`.
- `src/schema/tree.rs:19-26`: `sync_tree_move_to_grafeo(db, node_id: NodeId, old_parent: NodeId, new_parent: NodeId)` — operates on raw `grafeo::NodeId`, NOT on `TreeNode` structs.

The `LoroTree` API (`loro-1.13.6/src/lib.rs:2871, 2933-3084`) is a **separate container type** with native `create(parent)`, `mov(target, parent)`, `get_parent(target)` semantics and `TreeID` (not `String`) as the identity. A `TreeNode` struct with `{ node_id: String, title: String }` has no parent field and cannot represent a node inside a `LoroTree` — the parent/child relationship is implicit in the `LoroTree` container itself, queried via `tree.get_parent(tree_id)`.

So the codebase has **two distinct "tree" concepts** that the architecture doc conflates under the word "tree":

1. `OrderedCollection` (`LoroMovableList`) — a flat ordered list of `TreeNode`s for drag-drop UI ordering (Phase 2 Task 1 territory).
2. `T_CHILD` (`LoroTree`) — a strict spanning tree that prevents cycles during parent moves (Phase 2 Task 2 territory).

The `TreeNode` struct serves concept (1), not concept (2). L1's `tree_node_roundtrip` scaffold tests concept (1) — correct — but the **name** `tree_node_roundtrip` and the file location `src/schema/tree.rs` both reinforce the conflation. Phase 2 Task 2 (`sync_tree_move_to_grafeo`) operates on concept (2) and has NO scaffold — L1 deferred it (correctly, out of scope), but didn't flag the conceptual split for the orchestrator.

**Concrete solution** (doc-level, no src/ changes per Devil's read-only rule):

1. Add a `KNOWN AMBIGUITY` note to `docs/grafeo-loro.architecture.md` §7 distinguishing the two concepts:

   ```markdown
   ### Known Ambiguity: `OrderedCollection` vs `T_CHILD`

   The codebase has two distinct "tree" concepts:
   - **`OrderedCollection`** (`LoroMovableList`, `src/schema/tree.rs:6-9`): a flat
     ordered list of `TreeNode`s for drag-drop UI ordering. Identity is preserved
     via `#[key] node_id` + `#[loro(movable)]`. No parent/child relationship.
   - **`T_CHILD`** (`LoroTree`, `src/constants.rs:8` comment): a strict spanning
     tree that prevents cycles during parent moves. Identity is `TreeID` (native
     Loro type, not `String`). Parent/child is managed by the `LoroTree` container
     itself, queried via `tree.get_parent(tree_id)`.

   `TreeNode` (this section) belongs to `OrderedCollection`. The `T_CHILD`
   `LoroTree` does not use `TreeNode` — its metadata (vertex_id mapping) lives
   in a separate container (TBD in Phase 2 Task 2).
   ```

2. Rename `tree_node_roundtrip` → `ordered_item_roundtrip` (or similar) in L1's scaffold to make the scope unambiguous. (L2 action; Devil cannot edit `tests/` per read-only rule — actually the rules say "DO NOT modify any `src/` files" but tests/ are not src/. Re-reading: "Devil's Advocate is read-only on source. You only write to `docs/critiques/` and `worklog.md`." So Devil cannot edit tests/ either. Action item for L2.)

3. Add a Phase 2 Task 2 placeholder scaffold `t_child_tree_move_roundtrip()` (deferred to Task 2's L1, not this loop).

**L2 effort**: ~10 LOC doc + 1 test rename.

---

#### M3 — `tree_node_roundtrip` as currently scaffolded does NOT actually exercise `#[key]`; only `ordered_collection_roundtrip` does (implicitly)

**Context**: L1's `tree_node_roundtrip` doc says "exercises the `#[key]` attribute on `node_id`". But `#[key]` only has observable behavior inside a `LoroMovableList` reconciliation (movable_list.rs:64-72 checks `item.key()` to dispatch to keyed diffing). A standalone `TreeNode` roundtrip via `RootReconciler::new(map)` calls `TreeNode::reconcile(r)` which calls `r.map()?` → writes `node_id` and `title` as plain map entries. The `key()` method exists on the trait but is never called during a flat-struct reconcile. So `tree_node_roundtrip` exercises **`TreeNode`'s map encoding**, NOT the `#[key]` attribute's behavior.

This is a tautology risk: a test named "exercises `#[key]`" that doesn't exercise `#[key]`. If L3 naively trusts the test name and removes `#[key]` from `TreeNode::node_id`, the test would still pass (the flat-struct roundtrip doesn't care about `#[key]`), but `OrderedCollection`'s keyed diffing would silently fall back to positional `set`/`insert`/`delete` (movable_list.rs:68-69) — losing the identity-preservation guarantee.

**Concrete solution** (L2 action): Restructure the two scaffolds so each test name matches what it actually tests:

```rust
/// Roundtrips a single `TreeNode` as a flat LoroMap. Does NOT exercise
/// `#[key]` (which only matters inside an `OrderedCollection`'s movable list).
/// Use `ordered_collection_reorder_preserves_identity` for the `#[key]` contract.
#[test]
#[ignore = "P2-L1 scaffold: L3 implements the body"]
fn tree_node_flat_roundtrip() {
    let _ = std::marker::PhantomData::<TreeNode>;
    todo!()
}

/// Directly asserts `<TreeNode as Reconcile>::key()` returns
/// `LoadKey::Found(node_id)` — the contract that `OrderedCollection`'s
/// movable-list diffing relies on.
#[test]
#[ignore = "P2-L1 scaffold: L3 implements the body"]
fn tree_node_key_extraction() {
    use lorosurgeon::{Reconcile, LoadKey};
    let tn = TreeNode { node_id: "n1".into(), title: "T".into() };
    // L3: assert!(matches!(tn.key(), LoadKey::Found(ref k) if k == "n1"));
    todo!()
}
```

The split makes the contract explicit: `tree_node_flat_roundtrip` tests the encoding, `tree_node_key_extraction` tests the key extraction in isolation, and `ordered_collection_reorder_preserves_identity` (M1 above) tests the keyed-diffing behavior end-to-end.

**L2 effort**: ~10 LOC (split + new stub).

---

### MINOR (5)

#### m1 — Architecture §27 line 1071 (`lorosurgeon = "0.3"`) is still wrong; L1 flagged but didn't fix the 1-character doc edit

L1's open question #1 (worklog.md:427) correctly identifies this as a doc-only fix but defers it. Devil agrees with the diagnosis but disagrees with the deferral — this is a 1-character edit (`"0.3"` → `"0.2"`) that should land in L2 to keep the doc as SSOT. Leaving the doc wrong invites the next agent to "fix" Cargo.toml to match the doc, reintroducing the Phase 1 L1 bug.

**Concrete solution**: `docs/grafeo-loro.architecture.md:1071` — change `lorosurgeon = "0.3"` to `lorosurgeon = "0.2"`. One-line edit.

---

#### m2 — `tests/unit/schema_roundtrip.rs:23` imports only `grafeo_loro::schema::{...}`; L3 will need `lorosurgeon::{Hydrate, Reconcile, RootReconciler}` to call the roundtrip pattern L1's doc describes

L1's test doc step 2 says "Reconcile it into a fresh `LoroDoc` root `LoroMap` via `lorosurgeon::RootReconciler`" and step 4 says "Hydrate the struct back via `<T as Hydrate>::hydrate_map(&map)`" — but the import block at line 23 doesn't bring `Hydrate`, `Reconcile`, or `RootReconciler` into scope. L3 will have to add the import manually, which is friction.

**Concrete solution**: Add `use lorosurgeon::{Hydrate, Reconcile, RootReconciler};` (and `use loro::LoroDoc;`) to the test imports. Two extra lines.

---

#### m3 — `PhantomData::<VertexEntity>` lines in each scaffold are dead code

Each test stub has `let _ = std::marker::PhantomData::<VertexEntity>;` to "exercise the imports". But the `use grafeo_loro::schema::{...};` statement at line 23 already exercises the imports — Rust will warn about unused imports if a type is never named, and `cargo check` would catch it. The `PhantomData` lines are noise that violates anti-plenger rule #13 ("oneline code first, fewest LOC").

**Concrete solution**: Remove the 4 `PhantomData` lines. The `use` statement alone is sufficient. The `todo!()` body is the only statement each test needs at scaffold time.

---

#### m4 — L1's module doc says "Reconcile it into a fresh `LoroDoc` root `LoroMap`" — ambiguous about whether `RootReconciler` takes the doc's actual root or a nested map

The phrase "fresh LoroDoc root LoroMap" is ambiguous. The upstream pattern (`lorosurgeon-0.2.1/tests/integration.rs:152-157`) uses `doc.get_map("root")` — a NESTED map accessed via the `"root"` string key, NOT the doc's actual root. The `RootReconciler::new(map)` constructor takes any `LoroMap` (reconcile.rs:297-300); it doesn't care whether it's the doc root or a nested map.

This matters because the production layout (architecture §5) puts vertices at `V.<NodeID>` — a NESTED map. L3 implementing the production path would use `RootReconciler::new(doc.get_map("V").get_map("k1"))` (or similar), NOT `RootReconciler::new(doc.get_map("V"))`. The test doc's "root LoroMap" phrasing could mislead L3 into reconciling a single vertex AT the V container (wrong) instead of UNDER it (right).

**Concrete solution**: Reword the module doc step 2:

```markdown
2. Reconcile it into a fresh `LoroMap` (e.g., `doc.get_map("root")`) via
   `lorosurgeon::RootReconciler::new(map)`. NOTE: this is the ISOLATED entity
   roundtrip pattern. The production path nests the entity under a registry
   key (`doc.get_map("V").get_map(<NodeID>)`) per architecture §5; L3 must
   NOT copy this test pattern directly into the bridge layer.
```

---

#### m5 — `#![allow(missing_docs)]` at `tests/unit/schema_roundtrip.rs:21` is unnecessary noise

Test files don't require doc comments on every item. The `#![allow(missing_docs)]` is defensive but adds a line of noise. Anti-plenger rule #11 ("deletion over addition").

**Concrete solution**: Remove the `#![allow(missing_docs)]` attribute.

---

### NIT (3)

#### n1 — Test module doc is 20 lines for 4 stubs; could be 5 lines

The current module doc (lines 1-19) explains the roundtrip pattern in detail. Per anti-plenger rule #13 ("oneline code first, oneline doc only"), this could be 3 lines: "Phase 2 Task 1 scaffolds. See architecture §6/§7 for the roundtrip pattern. Each `#[ignore]` stub is a contract for L3 to fill in."

**Concrete solution**: Trim the module doc to 3 lines + a `// Pattern: see lorosurgeon-0.2.1/tests/integration.rs:151-162` reference.

---

#### n2 — `docs/grafeo-loro.project-structure.md:71` still references `ROOT_TREE ("T_CHILD")` as a current constant; it was deleted in Phase 1 Hunter Fix 4 (worklog.md:334)

The project-structure doc was not updated when `ROOT_TREE` was deleted from `src/constants.rs:8` (now a comment). L1 didn't flag this drift.

**Concrete solution**: Update `docs/grafeo-loro.project-structure.md:71` to: `Container keys: ROOT_VERTICES ("V"), ROOT_EDGES ("E"). (ROOT_TREE was deleted as YAGNI in Phase 1 Hunter Fix 4; re-add in Phase 2 Task 2 when T_CHILD LoroTree is wired.)`.

---

#### n3 — L1's open question #5 (`sync_tree_move_to_grafeo` skeleton has `unimplemented!()` body) is correctly deferred — informational only

L1 noted this is out of scope for Task 1 (worklog.md:431). Devil agrees. This is a no-op for L2; the function will be implemented in Phase 2 Task 2's L3. Recording here for completeness so the hunter doesn't flag it as an oversight.

---

## 2. Cross-phase coupling analysis

### P2 Task 2 (`sync_tree_move_to_grafeo`) — does L1's scaffold block it?

**No**, but the architecture ambiguity (M2 above) makes Task 2's L1 harder. Task 2 will need:
- A `LoroTree` subscriber (currently no scaffold exists for tree events; the Phase 1 `init_loro_subscriber` only handles V/E root-container diffs, not T_CHILD — see `src/bridge/sync_engine.rs`).
- A `TreeID → grafeo::NodeId` mapping (the existing `node_id_map: HashMap<String, NodeId>` uses String keys, not TreeID; Task 2 L1 will need a separate map or extend the existing one).
- A new `LoroOp::TreeMove` variant carrying `TreeID` (not `String`) — the existing `LoroOp::TreeMove { node_key, old_parent_key, new_parent_key }` uses String keys, which is wrong for `LoroTree` (which uses `TreeID`).

**L1 Task 1 did NOT block Task 2**, but M2 (architecture ambiguity) should be resolved before Task 2 L1 starts, or Task 2 L1 will have to make the same conceptual split under time pressure.

### P2 Task 3 (`VertexBuilder`) — does L1's scaffold block it?

**No.** `VertexBuilder` is a fluent API on `GrafeoLoroApp` (`src/app.rs:122-143`) that accumulates labels/properties and commits atomically. It uses `VertexEntity` as the underlying type, so the derives verified by L1's scaffolds are the foundation. No scaffold is needed at the schema-derive level; Task 3's L1 will need its own test scaffolds in `tests/unit/` (or `tests/integration/`).

**One observation**: `VertexBuilder::commit()` returns `Result<NodeId>`, but `NodeId` is now `pub use grafeo::NodeId` (a `u64` newtype, `src/types/ids.rs:10`). The bridge's `node_id_map` uses `String` keys (loro_key). Task 3's L1 will need to decide how `VertexBuilder` allocates the loro_key — likely a UUID or counter-based string. L1 Task 1 didn't need to address this, but Task 3 L1 should be aware.

---

## 3. Anti-plenger audit of L1's own work

| Anti-plenger rule | L1 compliance | Notes |
|---|---|---|
| Backward compat slavery | ✅ | No legacy preservation; L1 explicitly chose the orchestrator-approved manual Hydrate/Reconcile path. |
| Tautology (green tests, broken system) | ⚠️ | B1 and M3 are tautology risks — tests that pass without verifying the contract. |
| Context blindness | ✅ | L1 read the architecture doc, prior worklog, and verified against actual crate sources. |
| Band-aids | ✅ | No symptom-patching; L1 deferred non-Task-1 concerns explicitly. |
| Bloat / DRY | ⚠️ | m3 (PhantomData lines) and n1 (verbose module doc) are mild bloat. |
| Hallucination | ✅ | Every API claim verified against `~/.cargo/registry/src/`. |
| Happy-path bias | ⚠️ | B1, M1, M3 are happy-path risks — the scaffolds verify the happy roundtrip but not the contract (wire shape, identity preservation, key extraction). |
| Goodhart's law | ⚠️ | B1 is a direct Goodhart risk — a regression to derive-default encoding would still pass the roundtrip test. |
| Pure functions | ✅ | L1 added no logic; scaffolds are pure `todo!()` bodies. |
| DRY/SRP/SSOT | ✅ | L1 didn't duplicate contracts; the scaffolds are the SSOT for what L3 must implement. |
| YAGNI | ✅ | L1 didn't add speculative scaffolds (no DocSync, no LoroTree test). |
| Performance & security | ✅ | N/A at scaffold level. |
| High cohesion / loose coupling | ✅ | Tests are isolated to schema derives; no bridge coupling. |
| Immutability | ✅ | Scaffolds don't mutate state. |
| Polymorphism over conditionals | ✅ | N/A at scaffold level. |
| Observability | ✅ | N/A at scaffold level. |
| Idempotency | ✅ | Scaffolds are `todo!()`; running them is a no-op (`#[ignore]`). |
| Fewest LOC | ⚠️ | m3 + n1: ~10 LOC of noise. |
| Deletion over addition | ⚠️ | m3 + m5: `PhantomData` + `#![allow(missing_docs)]` should be deleted. |
| Native-first | ✅ | L1 used the upstream `RootReconciler` pattern verbatim. |
| Oneline code/doc first | ⚠️ | n1: module doc is verbose. |
| Never simplify the basics | ✅ | L1 didn't shortcut any contract. |

**L1's anti-plenger score**: 17 ✅ / 6 ⚠️ / 0 ❌. The ⚠️ items are all addressed by the findings above.

---

## 4. Summary

### Severity counts
- **BLOCKER**: 1 (B1 — LoroProperty encoding regression risk)
- **MAJOR**: 3 (M1 — identity-preservation scaffold missing; M2 — architecture §5/§7 conflation; M3 — `tree_node_roundtrip` doesn't exercise `#[key]`)
- **MINOR**: 5 (m1 — doc version drift; m2 — missing lorosurgeon imports; m3 — PhantomData noise; m4 — ambiguous "root LoroMap" wording; m5 — unnecessary `#![allow]`)
- **NIT**: 3 (n1 — verbose module doc; n2 — project-structure doc drift; n3 — informational only)

### L2 must-address list (priority order)

1. **B1** — Add `loro_property_encoding_roundtrip` scaffold (~15 LOC stub) that locks in the bare-value wire shape (asserts `LoroValue::Bool(true)` not `LoroValue::Map({"Bool": true})`).
2. **M1** — Add `ordered_collection_reorder_preserves_identity` scaffold (~25 LOC stub) that verifies `mov()` ops on reorder (not delete+insert).
3. **M3** — Split `tree_node_roundtrip` into `tree_node_flat_roundtrip` (encoding) and `tree_node_key_extraction` (key contract) so each test name matches what it tests.
4. **M2** — Add a `Known Ambiguity` note to architecture §7 distinguishing `OrderedCollection` (`LoroMovableList`) from `T_CHILD` (`LoroTree`). Defer the `T_CHILD`-specific scaffold to Phase 2 Task 2's L1.
5. **m1** — Fix `docs/grafeo-loro.architecture.md:1071` (`lorosurgeon = "0.3"` → `"0.2"`).
6. **m2** — Add `use lorosurgeon::{Hydrate, Reconcile, RootReconciler}; use loro::LoroDoc;` to `tests/unit/schema_roundtrip.rs` imports.
7. **m3** — Remove the 4 `PhantomData` lines.
8. **m4** — Reword the module doc step 2 to clarify isolated-entity vs production-registry path.
9. **m5** — Remove `#![allow(missing_docs)]`.
10. **n1** — Trim the module doc to 3 lines + upstream-pattern reference.
11. **n2** — Update `docs/grafeo-loro.project-structure.md:71` to reflect `ROOT_TREE` deletion.

### Top findings (the 3 L2 must not skip)

1. **B1** — without the `LoroProperty` encoding test, a 1-line regression to `#[derive(Hydrate, Reconcile)]` would silently double the wire size and break property lookups, while ALL existing tests stay green. This is the textbook Goodhart's Law violation.
2. **M1** — without the identity-preservation test, `OrderedCollection` is just a `Vec` with extra steps. The entire point of `#[loro(movable)]` + `#[key]` is unverified.
3. **M2** — the architecture conflation between `OrderedCollection` (`LoroMovableList`) and `T_CHILD` (`LoroTree`) will cause Phase 2 Task 2's L1 to flounder. Resolving it now (a doc note) is cheap; resolving it under Task 2 time pressure is expensive.

### Critique artifact path
`docs/critiques/p2-l1-devil.md` (this file)
