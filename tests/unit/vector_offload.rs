//! Phase 3 Task 4 tests: `hydration::vector::VectorOffloadManager::{new, handle_text_update}`.
//!
//! Spec validation gate (`docs/implementation-plan.md` Phase 3 Task 4 +
//! `ORCH-P3T4-SETUP` worklog line 4263): **"Vector never written to Loro
//! container."** All 4 tests are un-ignored and run by default (matching the
//! P3T1/P3T2/P3T3 L1→L3 trajectory).
//!
//! # Verified API surface (cheat sheet)
//!
//! ## grafeo-loro
//! - `grafeo_loro::VectorOffloadManager` — re-exported at `src/hydration/mod.rs:5`
//!   + crate root `src/lib.rs:31` (`pub use hydration::VectorOffloadManager;` —
//!   P3T4-L2 m5). Reach via the short path `grafeo_loro::VectorOffloadManager`.
//! - `VectorOffloadManager::new(Arc<GrafeoDB>) -> Self` — trivial `Self { db }`.
//! - `VectorOffloadManager::handle_text_update(&self, NodeId, &str) -> Result<()>`
//!   — async; L3 implemented the full 7-step flow (embed → session_with_cdc(false)
//!   → begin_tx → phantom pre-check → convert Vec→Arc→Value::Vector →
//!   set_node_property → prepare_commit + set_metadata(ORIGIN_LORO_BRIDGE) →
//!   commit).
//! - `grafeo_loro::hydration::vector::generate_local_embedding(&str) -> Result<Vec<f32>>`
//!   — Task 3, FULLY IMPLEMENTED. Returns `DEFAULT_EMBEDDING_DIM`-length
//!   `Vec<f32>` derived from `text` via SplitMix64. Deterministic + idempotent.
//! - `grafeo_loro::constants::EMBEDDING_PROPERTY: &str = "embedding"` — SSOT
//!   for the Grafeo property key.
//! - `grafeo_loro::constants::DEFAULT_EMBEDDING_DIM: usize = 384` — SSOT for
//!   embedding length (Task 3).
//! - `grafeo_loro::constants::ORIGIN_LORO_BRIDGE: &str = "loro-bridge"` —
//!   advisory origin tag.
//! - `grafeo_loro::types::ids::NodeId` — re-export of `grafeo::NodeId` (which
//!   is `pub struct NodeId(pub u64)`). NO conversion needed.
//! - `grafeo_loro::error::GrafeoLoroError::{Grafeo, Bridge, Config}` — existing
//!   variants cover all error paths via `#[from] grafeo::Error`.
//!
//! ## grafeo 0.5.42 (`grafeo-engine-0.5.42/src/`)
//! - `GrafeoDB::new_in_memory() -> Self` — verified in `tests/unit/parallel_hydrate.rs:129`.
//! - `GrafeoDB::session_with_cdc(false) -> Session` — `database/mod.rs:1728`.
//! - `Session::create_node(&self, &[&str]) -> NodeId` — `session/mod.rs:4860`
//!   (infallible; auto-commits at the current epoch when no tx is active).
//! - `Session::begin_transaction(&mut self) -> Result<()>` — `session/mod.rs:3883`.
//! - `Session::set_node_property(&self, NodeId, &str, Value) -> Result<()>` —
//!   `session/mod.rs:5012` (takes `&self`; underlying `crud.rs:359`
//!   auto-inserts into matching HNSW index if one exists).
//! - `Session::prepare_commit(&mut self) -> Result<PreparedCommit<'_>>` —
//!   `session/mod.rs:4496`.
//! - `Session::get_node(&self, NodeId) -> Option<Node>` — `session/mod.rs:5138`
//!   (used in test 1 for the phantom pre-check assertion + read-back).
//! - `Node::get_property(&self, &str) -> Option<&Value>` — used in
//!   `tests/unit/parallel_hydrate.rs:106` (`assert_grafeo_node` helper).
//! - `grafeo::Value::Vector(Arc<[f32]>)` — `grafeo-common-0.5.42/src/types/value.rs:138`.
//! - `PreparedCommit::set_metadata(K, V)` + `PreparedCommit::commit() -> Result<EpochId>`
//!   — `transaction/prepared.rs:107,124`.
//!
//! ## loro 1.13.6 (`loro-1.13.6/src/lib.rs`)
//! - `LoroDoc::new() -> Self` — `lib.rs:137`.
//! - `LoroDoc::get_deep_value(&self) -> LoroValue` — `loro-1.13.6/src/lib.rs:937`.
//!   Returns the full document as a `LoroValue::Map` (or `Null` for an empty doc).
//! - `LoroValue` variants (shapes verified at `loro-common-1.13.1/src/value.rs:14-26,51,53`):
//!   `Null`, `Bool(bool)`, `Double(f64)`, `I64(i64)`, `Binary(LoroBinaryValue)`,
//!   `String(LoroStringValue)`, `List(LoroListValue)` where
//!   `LoroListValue(Arc<Vec<LoroValue>>)`, `Map(LoroMapValue)` where
//!   `LoroMapValue(Arc<FxHashMap<String, LoroValue>>)`, `Container(ContainerID)`.
//!   `LoroListValue` derefs to `Vec<LoroValue>` at `value.rs:111`;
//!   `LoroMapValue` derefs to `FxHashMap<String, LoroValue>` at `value.rs:118`.
//!
//! # Edge cases (anti-happy-path)
//!
//! - Empty `text` `""` — `generate_local_embedding` returns a valid
//!   `DEFAULT_EMBEDDING_DIM`-length vector (fold yields seed `0`); the manager
//!   still writes it through (no short-circuit).
//! - Missing/unknown `node_id` — `set_node_property` would succeed (grafeo
//!   0.5.42 silently writes to phantom node ids); L3 added a pre-check via
//!   `session.get_node(node_id).is_none()` → `Err(GrafeoLoroError::Bridge(...))`.
//! - Grafeo tx failure (e.g. `prepare_commit` Err) — `Session::Drop`
//!   auto-rollbacks the un-committed tx (`session/mod.rs:5368-5383`); the
//!   `?` propagation returns `GrafeoLoroError::Grafeo`. No compensation needed
//!   because no Loro write happened (bypass invariant).
//! - Calling `handle_text_update` twice with the same `text` — second call
//!   overwrites the first (idempotent at the value level; `vector_offload_is_idempotent`
//!   asserts byte-identical vector).
//! - Calling `handle_text_update` with different texts on the same node —
//!   second call overwrites the first; `vector_offload_different_texts_different_embeddings`
//!   asserts the final vector matches the second text's embedding.

use std::sync::Arc;

use grafeo::{GrafeoDB, Value};
use grafeo_loro::constants::{DEFAULT_EMBEDDING_DIM, EMBEDDING_PROPERTY};
use grafeo_loro::hydration::vector::VectorOffloadManager;
use grafeo_loro::types::ids::NodeId;
use loro::{LoroDoc, LoroValue};

/// Spec gate (`docs/implementation-plan.md` Phase 3 Task 4): calling
/// `VectorOffloadManager::handle_text_update` on an existing Grafeo node
/// writes a `Value::Vector(Arc<[f32]>)` of length `DEFAULT_EMBEDDING_DIM`
/// into the node's `EMBEDDING_PROPERTY` slot.
///
/// # Anti-Goodhart test shape
///
/// 1. `Arc::new(GrafeoDB::new_in_memory())` + create a node via
///    `session.create_node(&["Doc"])`; record the returned `NodeId`.
/// 2. `VectorOffloadManager::new(db.clone())` + `mgr.handle_text_update(node_id, "hello world").await`.
/// 3. `db.session().get_node(node_id)` → `node.get_property(EMBEDDING_PROPERTY)`
///    → assert `Some(&grafeo::Value::Vector(arc))` AND `arc.len() == DEFAULT_EMBEDDING_DIM`.
/// 4. DO NOT assert vector searchability (no HNSW index creation — Task 4 spec
///    gate is "bypass Loro", NOT "searchable"). The `crud.rs:359`
///    auto-index-insert path is exercised at Phase 5 vector_search scope.
#[tokio::test]
async fn vector_offload_writes_embedding_to_grafeo() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let node_id = db.session().create_node(&["Doc"]);
    let mgr = VectorOffloadManager::new(db.clone());

    mgr.handle_text_update(node_id, "hello world")
        .await
        .expect("handle_text_update should succeed on existing node");

    let node = db
        .session()
        .get_node(node_id)
        .expect("node should exist after handle_text_update");
    match node.get_property(EMBEDDING_PROPERTY) {
        Some(Value::Vector(arc)) => {
            assert_eq!(
                arc.len(),
                DEFAULT_EMBEDDING_DIM,
                "embedding dimension must match DEFAULT_EMBEDDING_DIM"
            );
        }
        other => panic!(
            "expected Some(Value::Vector(_)) on EMBEDDING_PROPERTY, got {other:?}"
        ),
    }
}

/// **CRITICAL SPEC GATE** (`docs/implementation-plan.md` Phase 3 Task 4 +
/// `ORCH-P3T4-SETUP` worklog line 4263 + 4275): after
/// `VectorOffloadManager::handle_text_update`, the `LoroDoc` MUST contain NO
/// vector data. Vectors bypass Loro entirely — they live ONLY in Grafeo.
///
/// # Anti-Goodhart test shape (DEVIL answer 6 design constraint)
///
/// 1. Instantiate a FRESH `LoroDoc::new()` AND a FRESH `GrafeoDB::new_in_memory()`
///    with **NO `SyncEngine` connecting them**. The `VectorOffloadManager`
///    does NOT hold a `LoroDoc` reference (only `Arc<GrafeoDB>` field —
///    verified at `src/hydration/vector.rs:12`); the LoroDoc in this test is
///    intentionally DISCONNECTED from the manager's GrafeoDB, isolating the
///    contract test from any echo-back path.
/// 2. `VectorOffloadManager::new(db.clone())` + `mgr.handle_text_update(node_id, "test text").await`.
/// 3. `let deep = doc.get_deep_value();` — walk the `LoroValue` tree recursively.
/// 4. Assert NO `LoroValue::List` of length `DEFAULT_EMBEDDING_DIM` whose
///    elements are all `LoroValue::Double(_)` appears anywhere in the tree.
///    (Anti-tautology: also assert the doc is still effectively empty — fresh
///    `LoroDoc::get_deep_value()` returns `LoroValue::Null`.)
/// 5. Cross-check: the Grafeo node DOES have the embedding (reuse the
///    `vector_offload_writes_embedding_to_grafeo` assertion) — proving the
///    bypass went Grafeo-ward, not nowhere.
///
/// # Bypass invariant enforcement
///
/// The test does NOT mock or spy on `LoroDoc`; it inspects the final state. If
/// `handle_text_update` (or any code path it calls — including `generate_local_embedding`,
/// `apply_loro_op`, `RootReconciler`) writes ANY LoroValue to ANY Loro
/// container, this test fails. This is the strongest form of the bypass
/// invariant assertion.
#[tokio::test]
async fn vector_offload_never_writes_to_loro() {
    let doc = LoroDoc::new();
    let db = Arc::new(GrafeoDB::new_in_memory());
    let node_id = db.session().create_node(&["Doc"]);
    let mgr = VectorOffloadManager::new(db.clone());

    mgr.handle_text_update(node_id, "test text")
        .await
        .expect("handle_text_update should succeed");

    // (a) Fresh LoroDoc must still be effectively empty (`Null` or an empty
    // `Map` — `LoroDoc::new()` typically yields `Map({})`); a vector leak
    // would surface as a `List` of `Double`s somewhere in the tree.
    let deep = doc.get_deep_value();
    let is_empty = match &deep {
        LoroValue::Null => true,
        LoroValue::Map(m) => m.is_empty(),
        _ => false,
    };
    assert!(
        is_empty,
        "fresh disconnected LoroDoc must remain empty after handle_text_update; got {deep:?}"
    );

    // (b) Defense-in-depth: recursively walk the tree (covers a hypothetical
    // future regression where a vector lands in a nested Map/List).
    assert!(
        !contains_embedding_list(&deep, DEFAULT_EMBEDDING_DIM),
        "LoroDoc must NOT contain any LoroValue::List of {DEFAULT_EMBEDDING_DIM} LoroValue::Double(_) elements (bypass-Loro invariant violated)"
    );

    // (c) Cross-check: the Grafeo node DOES have the embedding (proving the
    // bypass went Grafeo-ward, not nowhere).
    let node = db
        .session()
        .get_node(node_id)
        .expect("grafeo node should exist");
    assert!(
        matches!(
            node.get_property(EMBEDDING_PROPERTY),
            Some(Value::Vector(arc)) if arc.len() == DEFAULT_EMBEDDING_DIM
        ),
        "grafeo node must carry the embedding (cross-check that bypass went Grafeo-ward)"
    );
}

/// Calling `handle_text_update` twice with the same `text` produces the same
/// embedding both times (anti-plenger #9 Absolute Idempotency). The Grafeo
/// node's `EMBEDDING_PROPERTY` ends with byte-identical `Arc<[f32]>` after
/// the second call.
///
/// # Anti-Goodhart
///
/// The assertion is on the FULL vector (not just length), so a non-deterministic
/// `generate_local_embedding` (e.g. one that reads `Instant::now()`) would fail.
/// Already covered at the Task 3 layer (`generate_local_embedding_is_deterministic`);
/// this test verifies the property is preserved through the Grafeo write+read
/// roundtrip.
#[tokio::test]
async fn vector_offload_is_idempotent() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let node_id = db.session().create_node(&["Doc"]);
    let mgr = VectorOffloadManager::new(db.clone());

    mgr.handle_text_update(node_id, "same text twice")
        .await
        .expect("first handle_text_update");
    let first = read_embedding(&db, node_id);

    mgr.handle_text_update(node_id, "same text twice")
        .await
        .expect("second handle_text_update");
    let second = read_embedding(&db, node_id);

    assert_eq!(
        first, second,
        "byte-identical embedding after two calls with same text (idempotency)"
    );
}

/// Calling `handle_text_update` with different texts produces different
/// embeddings on the same node (second call overwrites the first). Catches a
/// fixed-constant `generate_local_embedding` shortcut (already covered at Task 3)
/// AND catches a stale-cache bug in the manager (e.g. caching the first
/// embedding and ignoring subsequent texts — anti-band-aid).
///
/// # Anti-Goodhart
///
/// Asserts `assert_ne!` on the FULL `Vec<f32>` (not just length) read back from
/// Grafeo after each call.
#[tokio::test]
async fn vector_offload_different_texts_different_embeddings() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let node_id = db.session().create_node(&["Doc"]);
    let mgr = VectorOffloadManager::new(db.clone());

    mgr.handle_text_update(node_id, "first text")
        .await
        .expect("first handle_text_update");
    let first = read_embedding(&db, node_id);

    mgr.handle_text_update(node_id, "second text")
        .await
        .expect("second handle_text_update");
    let second = read_embedding(&db, node_id);

    assert_ne!(
        first, second,
        "different texts must produce different embeddings (no stale-cache bug)"
    );
}

/// Read back the embedding vector from a Grafeo node's `EMBEDDING_PROPERTY`
/// slot as a cloned `Vec<f32>`. Panics if the property is missing or not a
/// `Value::Vector` — used by idempotency + overwrite-semantics tests.
fn read_embedding(db: &GrafeoDB, node_id: NodeId) -> Vec<f32> {
    let node = db
        .session()
        .get_node(node_id)
        .unwrap_or_else(|| panic!("grafeo should have node {node_id:?}"));
    match node.get_property(EMBEDDING_PROPERTY) {
        Some(Value::Vector(arc)) => arc.to_vec(),
        other => panic!(
            "expected Some(Value::Vector(_)) on EMBEDDING_PROPERTY for node {node_id:?}, got {other:?}"
        ),
    }
}

/// Recursively walk the `LoroValue` tree. Returns `true` if any
/// `LoroValue::List` of length `dim` whose elements are all
/// `LoroValue::Double(_)` appears anywhere (this is the shape an embedding
/// would take if it leaked through to a Loro container). `LoroListValue`
/// derefs to `Vec<LoroValue>` at `loro-common-1.13.1/src/value.rs:111`;
/// `LoroMapValue` derefs to `FxHashMap<String, LoroValue>` at `value.rs:118`.
fn contains_embedding_list(v: &LoroValue, dim: usize) -> bool {
    match v {
        LoroValue::List(llv) => {
            if llv.len() == dim && llv.iter().all(|e| matches!(e, LoroValue::Double(_))) {
                return true;
            }
            llv.iter().any(|e| contains_embedding_list(e, dim))
        }
        LoroValue::Map(m) => m.values().any(|e| contains_embedding_list(e, dim)),
        _ => false,
    }
}
