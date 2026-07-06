//! Phase 3 Task 4 tests: `hydration::vector::VectorOffloadManager::{new, handle_text_update}`.
//!
//! Spec validation gate (`docs/implementation-plan.md` Phase 3 Task 4 +
//! `ORCH-P3T4-SETUP` worklog line 4263): **"Vector never written to Loro
//! container."** All 4 scaffolds are `#[ignore]`'d at L1 — L3 will fill the
//! bodies (matching the P3T1/P3T2/P3T3 L1→L3 trajectory).
//!
//! # L1 scope boundary
//!
//! L1 writes ONLY the contracts + `#[ignore]` test scaffolds (`todo!()` bodies
//! are L3). L1 does NOT implement `handle_text_update` body (klemer-agents.md:
//! "L1: Write ONLY compilable interfaces, types, skeletons, and empty method
//! signatures as cheatsheet"). The `VectorOffloadManager::new` body IS trivial
//! per L1 decision 1 (`Self { db }`).
//!
//! # Verified API surface (cheat sheet for L3)
//!
//! ## grafeo-loro
//! - `grafeo_loro::VectorOffloadManager` — re-exported at `src/hydration/mod.rs:5`
//!   + crate root `src/lib.rs:31` (`pub use hydration::VectorOffloadManager;` —
//!   P3T4-L2 m5). Reach via the short path `grafeo_loro::VectorOffloadManager`.
//! - `VectorOffloadManager::new(Arc<GrafeoDB>) -> Self` — L1 implements
//!   trivially as `Self { db }` (decision 1).
//! - `VectorOffloadManager::handle_text_update(&self, NodeId, &str) -> Result<()>`
//!   — async; L2 wired the execution path with `// TODO(L3):` markers per step
//!   (embed → session_with_cdc(false) → begin_tx → phantom pre-check →
//!   convert Vec→Arc→Value::Vector → set_node_property → prepare_commit +
//!   set_metadata(ORIGIN_LORO_BRIDGE) → commit). L3 fills the actual calls.
//! - `grafeo_loro::hydration::vector::generate_local_embedding(&str) -> Result<Vec<f32>>`
//!   — Task 3, FULLY IMPLEMENTED. Returns `DEFAULT_EMBEDDING_DIM`-length
//!   `Vec<f32>` derived from `text` via SplitMix64. Deterministic + idempotent.
//! - `grafeo_loro::constants::EMBEDDING_PROPERTY: &str = "embedding"` — SSOT
//!   for the Grafeo property key (L1 decision 4; cites
//!   `grafeo-engine-0.5.42/src/database/index.rs:91`).
//! - `grafeo_loro::constants::DEFAULT_EMBEDDING_DIM: usize = 384` — SSOT for
//!   embedding length (Task 3).
//! - `grafeo_loro::constants::ORIGIN_LORO_BRIDGE: &str = "loro-bridge"` —
//!   advisory origin tag (L1 decision 3 — no new `ORIGIN_VECTOR_OFFLOAD`).
//! - `grafeo_loro::types::ids::NodeId` — re-export of `grafeo::NodeId` (which
//!   is `pub struct NodeId(pub u64)`). NO conversion needed between
//!   grafeo-loro + grafeo NodeId (verified `src/types/ids.rs:10`).
//! - `grafeo_loro::error::GrafeoLoroError::{Grafeo, Bridge, Config}` — existing
//!   variants cover all error paths via `#[from] grafeo::Error` (no new variant
//!   per anti-plenger #5 Bloat).
//!
//! ## grafeo 0.5.42 (`grafeo-engine-0.5.42/src/`)
//! - `GrafeoDB::new_in_memory() -> Self` — verified in `tests/unit/parallel_hydrate.rs:129`.
//! - `GrafeoDB::session_with_cdc(false) -> Session` — `database/mod.rs:1728`.
//! - `GrafeoDB::node_count() -> usize` — `database/admin.rs:14`.
//! - `GrafeoDB::create_vector_index(label, property, dims, metric, m, ef_construction, quantization) -> Result<()>`
//!   — `database/index.rs:104` (7 args; ALL `Option` except label + property;
//!   needed ONLY if a test asserts searchability — Task 4's spec gate is
//!   "bypass Loro", NOT "searchable"; L3 may skip index creation in
//!   `vector_offload_writes_embedding_to_grafeo` and assert only the
//!   property's existence + type).
//! - `Session::begin_transaction(&mut self) -> Result<()>` — `session/mod.rs:3883`.
//! - `Session::set_node_property(&self, NodeId, &str, Value) -> Result<()>` —
//!   `session/mod.rs:5012` (takes `&self`; underlying `crud.rs:359`
//!   auto-inserts into matching HNSW index if one exists).
//! - `Session::prepare_commit(&mut self) -> Result<PreparedCommit<'_>>` —
//!   `session/mod.rs:4496`.
//! - `Session::get_node(&self, NodeId) -> Option<Node>` — `session/mod.rs:5138`
//!   (verified in `tests/unit/parallel_hydrate.rs:89`).
//! - `Node::get_property(&self, &str) -> Option<&Value>` — used in
//!   `tests/unit/parallel_hydrate.rs:106` (`assert_grafeo_node` helper).
//! - `grafeo::Value::Vector(Arc<[f32]>)` — `grafeo-common-0.5.42/src/types/value.rs:138`.
//! - `PreparedCommit::set_metadata(K, V)` + `PreparedCommit::commit() -> Result<EpochId>`
//!   — `transaction/prepared.rs:107,124`.
//!
//! ## loro 1.13.6 (`loro-1.13.6/src/lib.rs`)
//! - `LoroDoc::new() -> Self` — `lib.rs:137`.
//! - `LoroDoc::get_deep_value(&self) -> LoroValue` — `loro-1.13.6/src/lib.rs:937`
//!   (DEVIL MINOR 1 correction — L1 cited `:1064`, off-by-127). Returns the
//!   full document as a `LoroValue::Map` (or `Null` for an empty doc).
//! - `LoroValue` variants (for the bypass-Loro assertion — shapes verified at
//!   `loro-common-1.13.1/src/value.rs:14-26,51,53`): `Null`, `Bool(bool)`,
//!   `Double(f64)`, `I64(i64)`, `Binary(LoroBinaryValue)`, `String(LoroStringValue)`,
//!   `List(LoroListValue)` where `LoroListValue(Arc<Vec<LoroValue>>)`,
//!   `Map(LoroMapValue)` where `LoroMapValue(Arc<FxHashMap<String, LoroValue>>)`,
//!   `Container(ContainerID)`. The bypass-Loro test walks the `LoroValue` tree
//!   recursively and asserts NO `LoroValue::List` of `DEFAULT_EMBEDDING_DIM`
//!   `f32`-coerceable `Double`s appears anywhere (`LoroListValue` derefs to
//!   `Vec<LoroValue>` at `value.rs:111`).
//!
//! # L3 algorithm hint (informational, not binding)
//!
//! ```text
//! pub async fn handle_text_update(&self, node_id: NodeId, text: &str) -> Result<()> {
//!     let vec: Vec<f32> = generate_local_embedding(text)?;
//!     let arc: Arc<[f32]> = Arc::<[f32]>::from(vec);
//!     let mut session = self.db.session_with_cdc(false);
//!     session.begin_transaction()?;
//!     session.set_node_property(node_id, EMBEDDING_PROPERTY, grafeo::Value::Vector(arc))?;
//!     let mut prepared = session.prepare_commit()?;
//!     prepared.set_metadata(ORIGIN_LORO_BRIDGE, ORIGIN_LORO_BRIDGE);
//!     prepared.commit()?;
//!     Ok(())
//! }
//! ```
//!
//! # Edge cases (anti-happy-path)
//!
//! - Empty `text` `""` — `generate_local_embedding` returns a valid
//!   `DEFAULT_EMBEDDING_DIM`-length vector (fold yields seed `0`); the manager
//!   must still write it through (no short-circuit).
//! - Missing/unknown `node_id` — `set_node_property` succeeds (grafeo 0.5.42
//!   creates the property on the phantom node id; L3 should verify this is
//!   acceptable OR add a pre-check via `session.get_node(node_id).ok_or(...)`).
//!   L1 defers this to L3 (DEVIL Q4).
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

#![allow(unused_imports)] // imports are L3 cheat-sheet; bodies are todo!()
// TODO(L3): remove this silencer when filling test bodies

use std::sync::Arc;

use grafeo::GrafeoDB;
use grafeo_loro::constants::{DEFAULT_EMBEDDING_DIM, EMBEDDING_PROPERTY};
use grafeo_loro::hydration::vector::VectorOffloadManager;
use grafeo_loro::types::ids::NodeId;
use loro::LoroDoc;

/// Spec gate (`docs/implementation-plan.md` Phase 3 Task 4): calling
/// `VectorOffloadManager::handle_text_update` on an existing Grafeo node
/// writes a `Value::Vector(Arc<[f32]>)` of length `DEFAULT_EMBEDDING_DIM`
/// into the node's `EMBEDDING_PROPERTY` slot.
///
/// # Anti-Goodhart test shape
///
/// 1. `Arc::new(GrafeoDB::new_in_memory())` + create a node via
///    `session.create_node(["Doc"])` (or `create_node_with_props`); record the
///    returned `NodeId`.
/// 2. `VectorOffloadManager::new(db.clone())` + `mgr.handle_text_update(node_id, "hello world").await`.
/// 3. `db.session().get_node(node_id)` → `node.get_property(EMBEDDING_PROPERTY)`
///    → assert `Some(&grafeo::Value::Vector(arc))` AND `arc.len() == DEFAULT_EMBEDDING_DIM`.
/// 4. DO NOT assert vector searchability (no HNSW index creation — Task 4 spec
///    gate is "bypass Loro", NOT "searchable"). The `crud.rs:359`
///    auto-index-insert path is exercised at Phase 5 vector_search scope.
#[test]
#[ignore = "P3T4-L1 scaffold: L3 implements the body"]
fn vector_offload_writes_embedding_to_grafeo() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let _mgr = VectorOffloadManager::new(db.clone());
    let _node_id: NodeId = NodeId(0); // L3: replace with `create_node` result.
    let _text = "hello world";
    let _expected_dim = DEFAULT_EMBEDDING_DIM;
    let _prop = EMBEDDING_PROPERTY;
    todo!("L3: create node, call handle_text_update, assert Value::Vector(dim) on the node")
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
///    (Anti-tautology: also assert the doc is still effectively empty — at
///    most a `LoroValue::Map` with the standard root keys `V` / `E` each
///    mapping to an empty map. The vector must NOT have leaked through any
///    container.)
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
#[test]
#[ignore = "P3T4-L1 scaffold: L3 implements the body"]
fn vector_offload_never_writes_to_loro() {
    let _doc = LoroDoc::new();
    let db = Arc::new(GrafeoDB::new_in_memory());
    let _mgr = VectorOffloadManager::new(db.clone());
    let _node_id: NodeId = NodeId(0);
    let _text = "test text";
    let _dim = DEFAULT_EMBEDDING_DIM;
    todo!("L3: call handle_text_update, walk doc.get_deep_value(), assert no LoroValue::List<f32> of dim length")
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
#[test]
#[ignore = "P3T4-L1 scaffold: L3 implements the body"]
fn vector_offload_is_idempotent() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let _mgr = VectorOffloadManager::new(db.clone());
    let _node_id: NodeId = NodeId(0);
    let _text = "same text twice";
    let _dim = DEFAULT_EMBEDDING_DIM;
    todo!("L3: call handle_text_update twice with same text, assert Value::Vector equality on readback")
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
#[test]
#[ignore = "P3T4-L1 scaffold: L3 implements the body"]
fn vector_offload_different_texts_different_embeddings() {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let _mgr = VectorOffloadManager::new(db.clone());
    let _node_id: NodeId = NodeId(0);
    let _text_a = "first text";
    let _text_b = "second text";
    let _dim = DEFAULT_EMBEDDING_DIM;
    todo!("L3: call handle_text_update with text_a, read back; call with text_b, read back; assert_ne on the two vectors")
}
