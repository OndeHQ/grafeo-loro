use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use crate::bridge::{BridgeMaps, SyncEngine};
use crate::config::{CompressionType, SsotMode};
use crate::error::{GrafeoLoroError, Result};
use crate::storage::StorageBackend;
use crate::types::{GraphValue, NodeId, PresencePayload};

// L3 will need these when implementing `VertexBuilder::commit` (kept here as
// a hint; `#[allow(unused)]` silences the unused-import warning until then):
//   use std::sync::atomic::Ordering;
//   use crate::bridge::grafeo_tx::apply_loro_op;
//   use crate::constants::{ORIGIN_LORO_BRIDGE, ROOT_VERTICES};
//   use crate::types::events::LoroOp;

/// Top-level app facade.
///
/// # Phase 2 Task 3 scope (P2T3-L2)
///
/// Holds a single `Arc<SyncEngine>` handle plus a process-local
/// `loro_key_counter`. [`SyncEngine`] is the SSOT for `LoroDoc`, `GrafeoDB`,
/// `BridgeMaps`, and the epoch side-channel — `commit()` reaches them via the
/// engine's `pub(crate)` fields (`loro_doc`, `grafeo_db`) and the public
/// [`SyncEngine::maps`] accessor. No redundant `doc`/`db` Arc fields (DRY;
/// anti-plenger rule #2).
///
/// Production construction goes through [`GrafeoLoroAppBuilder::build`]
/// (Phase 4 scope — still `unimplemented!()`). Tests + future embedding
/// scenarios construct via [`Self::from_sync_engine`].
///
/// All methods other than [`Self::create_vertex`] + [`Self::maps`] remain
/// `unimplemented!()` (Phase 3-5 scope). See each method's doc-comment for
/// the owning phase.
pub struct GrafeoLoroApp {
    /// Bidirectional sync engine. SSOT for `LoroDoc` + `GrafeoDB` + `BridgeMaps`
    /// + epoch side-channel. `commit()` accesses them via `pub(crate)` fields.
    pub(crate) sync_engine: Arc<SyncEngine>,
    /// Process-local counter for fresh `loro_key` generation. NOT durable
    /// across cold boot — see [`VertexBuilder::commit`] doc.
    pub(crate) loro_key_counter: Arc<AtomicU64>,
}

/// Builder for [`GrafeoLoroApp`]. Fluent setters; call [`build`](Self::build)
/// to validate and spawn the runtime.
pub struct GrafeoLoroAppBuilder {
    storage: Option<Arc<dyn StorageBackend>>,
    ssot_mode: SsotMode,
    compression: CompressionType,
    sync_compression: CompressionType,
    batch_interval_ms: u64,
    batch_max_size: usize,
}

impl GrafeoLoroApp {
    /// Entry point for the fluent builder.
    pub fn builder() -> GrafeoLoroAppBuilder {
        unimplemented!("GrafeoLoroAppBuilder::build is Phase 4 scope")
    }

    /// Construct an app from a pre-built [`SyncEngine`]. Intended for tests
    /// and for future embedding scenarios (e.g. a `GrafeoLoroApp` constructed
    /// from an externally-managed engine). Production code should use
    /// [`Self::builder`] once Phase 4 lands. The `loro_key_counter` starts at
    /// 0 — cold-boot hydration (Phase 4) will re-seed it to
    /// `max(existing V/* keys) + 1`.
    pub fn from_sync_engine(sync_engine: Arc<SyncEngine>) -> Self {
        Self {
            sync_engine,
            loro_key_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Access the bridge id-mapping state. Used by tests to recover
    /// `loro_key ↔ grafeo::NodeId` bindings after [`VertexBuilder::commit`].
    pub fn maps(&self) -> &Arc<BridgeMaps> {
        self.sync_engine.maps()
    }

    /// Begin a fluent vertex-upsert transaction.
    ///
    /// Wiring only: clones the engine handle + the shared counter and returns
    /// a fresh empty [`VertexBuilder`]. No allocations beyond the empty
    /// `Vec`/`HashMap`.
    pub fn create_vertex(&self) -> VertexBuilder {
        VertexBuilder {
            sync_engine: Arc::clone(&self.sync_engine),
            loro_key_counter: Arc::clone(&self.loro_key_counter),
            labels: Vec::new(),
            properties: HashMap::new(),
        }
    }

    /// One-shot GQL query against the materialized Grafeo view.
    pub fn query(&self, gql: &str) -> Result<grafeo::QueryResult> {
        let _ = gql;
        unimplemented!("query is Phase 4+ scope")
    }

    /// Update a collaborative text field on a vertex.
    pub async fn update_text(&self, node_id: NodeId, field: &str, text: &str) -> Result<()> {
        let _ = (node_id, field, text);
        unimplemented!("update_text is Phase 3 scope")
    }

    /// Regenerate the embedding vector for a vertex's text field.
    pub async fn generate_embedding(&self, node_id: NodeId, field: &str) -> Result<()> {
        let _ = (node_id, field);
        unimplemented!("generate_embedding is Phase 3 scope")
    }

    /// Export a shallow snapshot and persist via the storage backend.
    pub async fn checkpoint(&self, graph_id: &str) -> Result<()> {
        let _ = graph_id;
        unimplemented!("checkpoint is Phase 4 scope")
    }

    /// Broadcast ephemeral presence over the WebSocket channel.
    pub async fn broadcast_presence(&self, payload: PresencePayload) -> Result<()> {
        let _ = payload;
        unimplemented!("broadcast_presence is Phase 5 scope")
    }

    /// Graceful shutdown: cancel workers, flush buffers, close stores.
    pub async fn shutdown(self) -> Result<()> {
        unimplemented!("shutdown is Phase 5 scope")
    }
}

impl GrafeoLoroAppBuilder {
    /// Provide a storage backend implementation (filesystem, S3, IPFS, ...).
    pub fn storage(self, storage: Arc<dyn StorageBackend>) -> Self {
        let _ = storage;
        unimplemented!("GrafeoLoroAppBuilder is Phase 4 scope")
    }

    /// Select Loro or Grafeo as the source of truth.
    pub fn ssot_mode(self, mode: SsotMode) -> Self {
        let _ = mode;
        unimplemented!("GrafeoLoroAppBuilder is Phase 4 scope")
    }

    /// Compression strategy for cold snapshots.
    pub fn compression(self, comp: CompressionType) -> Self {
        let _ = comp;
        unimplemented!("GrafeoLoroAppBuilder is Phase 4 scope")
    }

    /// Compression strategy for hot sync packets.
    pub fn sync_compression(self, comp: CompressionType) -> Self {
        let _ = comp;
        unimplemented!("GrafeoLoroAppBuilder is Phase 4 scope")
    }

    /// Batcher flush interval in milliseconds.
    pub fn batch_interval_ms(self, ms: u64) -> Self {
        let _ = ms;
        unimplemented!("GrafeoLoroAppBuilder is Phase 4 scope")
    }

    /// Batcher max ops per flush.
    pub fn batch_max_size(self, size: usize) -> Self {
        let _ = size;
        unimplemented!("GrafeoLoroAppBuilder is Phase 4 scope")
    }

    /// Validate config and spawn the runtime.
    pub async fn build(self) -> Result<GrafeoLoroApp> {
        unimplemented!("GrafeoLoroAppBuilder::build is Phase 4 scope")
    }
}

/// Fluent vertex-upsert builder returned by [`GrafeoLoroApp::create_vertex`].
///
/// # Phase 2 Task 3 contract (P2T3-L2)
///
/// Accumulates `labels` + `properties` via [`Self::with_label`] /
/// [`Self::with_property`]. [`Self::commit`] writes the vertex to **both**
/// Loro and Grafeo and returns the grafeo-assigned [`NodeId`].
///
/// `commit(self)` consumes `self` — one-shot (a compile-time guarantee that
/// the same builder cannot commit twice).
///
/// ## `VertexEntity::description` default
///
/// [`VertexEntity`](crate::schema::VertexEntity) has a `description: String`
/// field (`#[loro(text)]` — Phase 3 text-collaboration surface). Phase 2 does
/// NOT expose a `with_description` setter on this builder (YAGNI — Phase 3
/// will add it). `commit()` reconciles a `VertexEntity` with
/// `description: String::new()` (the `String` default), which the Loro side
/// stores as an empty `LoroText`. The Grafeo side has no `description`
/// property (it is a Loro-only field).
///
/// ## Atomicity contract (Option a — Loro-first with compensation)
///
/// `commit()` writes Loro first; if Loro fails, returns `Err` and Grafeo is
/// untouched. If Loro succeeds, writes Grafeo; if Grafeo fails, **compensates
/// by deleting the just-inserted Loro entry** under the same `loro_key`. The
/// final state on Grafeo failure is therefore: both stores clean (no partial
/// vertex).
///
/// Rationale: grafeo's `create_node_with_props` is the SSOT for `NodeId`
/// generation (it assigns the u64 id; the caller cannot pass one in — verified
/// `grafeo-engine-0.5.42/src/session/mod.rs:4885`). Option (b) (Grafeo-first)
/// would require populating `BridgeMaps` before the Loro write so the outbound
/// CDC poller can reverse-translate, but the Grafeo↔Loro echo window between
/// the two writes is wider under (b). Option (a) keeps the Loro write +
/// `set_next_commit_origin` + `commit` under a single `RwLock` write guard
/// (per `bridge::sync_engine` module doc) and lets the synchronous subscriber
/// fire+filter before the Grafeo session opens.
///
/// ## Echo prevention
///
/// The Loro commit is tagged with [`ORIGIN_LORO_BRIDGE`](crate::constants::ORIGIN_LORO_BRIDGE).
/// The Grafeo session is opened with `session_with_cdc(false)` so no CDC event
/// is emitted for the write (echo prevention on the Grafeo→Loro path).
///
/// The inbound subscriber filter at
/// `src/bridge/sync_engine.rs::init_loro_subscriber` skips BOTH
/// `ORIGIN_GRAFEO_BRIDGE` (outbound Grafeo→Loro echoes) AND `ORIGIN_LORO_BRIDGE`
/// (local RYOW `VertexBuilder::commit` echoes — added P2T3-L2 BLOCKER B1).
/// Without the `ORIGIN_LORO_BRIDGE` clause the synchronous subscriber would
/// re-apply the same vertex to Grafeo via the batcher, producing either a
/// duplicate label-less node (race case — see Pre-existing inbound translator
/// bug below) or a spurious no-op Grafeo commit polluting the epoch
/// side-channel (common case).
///
/// ## NodeId + loro_key generation strategy
///
/// The grafeo `NodeId` is assigned by `Session::create_node_with_props`
/// (cannot be passed in by the caller). `commit()` returns that
/// grafeo-assigned id. The Loro-side `loro_key` is generated freshly per
/// `commit()` call via an `Arc<AtomicU64>` counter held on [`GrafeoLoroApp`]
/// and cloned into each `VertexBuilder`: `format!("V/{}",
/// counter.fetch_add(1, Ordering::Relaxed))`. The `V/` prefix matches the
/// architecture §5 root map key convention and avoids collision with bare
/// integer keys. `AtomicU64: Send + Sync` (std), so concurrent `VertexBuilder`s
/// share the counter via `Arc::clone` and each gets a unique `loro_key` —
/// YAGNI on the `uuid` crate (not in `Cargo.toml`).
///
/// ### Multi-peer loro_key semantics
///
/// The counter is **process-local and NOT durable across cold boot**. The
/// `loro_key ↔ grafeo::NodeId` binding is rebuilt by the Phase 4 hydration
/// engine (which scans existing `V/*` keys and re-seeds the counter to
/// `max(existing) + 1`). The grafeo `NodeId` IS durable (grafeo assigns it;
/// the bridge mapping is in-memory). Multi-peer collision risk: two peers
/// generating `V/0`, `V/1` independently will collide on import. Future fix:
/// prefix with peer_id (Phase 4 scope). For Phase 2 (single-process), this is
/// a non-issue.
///
/// ## Pre-existing inbound translator bug (Phase 1, documented)
///
/// `translate_diff_event` at `src/bridge/sync_engine.rs:419-474` always
/// produces `LoroOp::UpsertNode { labels: Vec::new(), properties }` — labels
/// are silently dropped (the translator treats the `labels` key inside the
/// vertex map as a regular property rather than extracting it into the
/// `LoroOp::UpsertNode::labels` field). The B1 filter extension prevents
/// this bug from manifesting in `VertexBuilder::commit` (the echo from
/// `commit()` is filtered before reaching the translator). NO code change in
/// P2T3 — the fix (schema-aware translator) is out of scope. Future Phase
/// work should make `translate_diff_event` extract `labels` from the vertex
/// map's `labels: LoroValue::List` field.
///
/// ## Properties shape mismatch
///
/// `with_property` accepts [`GraphValue`] (full superset:
/// `Null/Bool/Integer/Float/String/Vector/Map/List`). The Loro-side
/// [`VertexEntity::properties`](crate::schema::VertexEntity) uses
/// [`LoroProperty`](crate::types::LoroProperty) which is the JSON-shaped subset
/// (`Null/Bool/Integer/Float/String`) — `Vector`/`Map`/`List` have no Loro
/// representation in the schema. `commit()` step 1 (BEFORE any Loro write)
/// strictly rejects `Vector`/`Map`/`List` with
/// [`GrafeoLoroError::UnsupportedLoroType`] (fail loud). Phase 3 §17 will
/// wire vector offloading; the strict reject now is forward-compatible.
pub struct VertexBuilder {
    /// Engine handle (cloned from `GrafeoLoroApp::sync_engine`). SSOT for
    /// `LoroDoc` + `GrafeoDB` + `BridgeMaps` + epoch side-channel.
    sync_engine: Arc<SyncEngine>,
    /// Process-local `loro_key` counter (cloned from
    /// `GrafeoLoroApp::loro_key_counter`). `fetch_add(1, Relaxed)` guarantees
    /// unique keys across concurrent `commit()` calls.
    loro_key_counter: Arc<AtomicU64>,
    /// Accumulated vertex labels (e.g. `["Person", "Admin"]`).
    labels: Vec<String>,
    /// Accumulated vertex properties (`key → GraphValue`).
    properties: HashMap<String, GraphValue>,
}

impl VertexBuilder {
    /// Attach a label to the vertex. Wiring only.
    pub fn with_label(mut self, label: &str) -> Self {
        self.labels.push(label.to_string());
        self
    }

    /// Attach a property to the vertex. Wiring only.
    pub fn with_property(mut self, key: &str, value: impl Into<GraphValue>) -> Self {
        self.properties.insert(key.to_string(), value.into());
        self
    }

    /// Generate a `NodeId`, write Loro + Grafeo atomically, return the id.
    ///
    /// See the [`VertexBuilder`] struct doc for the full atomicity contract,
    /// echo-prevention plan, NodeId + `loro_key` generation strategy,
    /// multi-peer semantics, pre-existing inbound translator bug, and
    /// properties shape mismatch policy. The skeleton body returns a
    /// placeholder error; L3 implements the 8-step algorithm below.
    ///
    /// # Errors
    ///
    /// - [`GrafeoLoroError::UnsupportedLoroType`] if any property value is a
    ///   `GraphValue::Vector`/`Map`/`List` (strict policy — see struct doc).
    ///   Returned BEFORE any Loro/Grafeo write.
    /// - [`GrafeoLoroError::Loro`] if the Loro write fails.
    /// - [`GrafeoLoroError::Grafeo`] if the Grafeo write fails (Loro
    ///   compensation has been attempted; if compensation also fails, the
    ///   error is logged at `error!` level with full context and the original
    ///   Grafeo error is returned — Q7).
    /// - [`GrafeoLoroError::Bridge`] if `apply_loro_op`'s binding insertion
    ///   cannot be observed post-call (engine dropped mid-commit — should not
    ///   happen since `self.sync_engine` holds an `Arc`).
    ///
    /// Grafeo Session API (verified against `grafeo-engine-0.5.42/src/`):
    /// - `GrafeoDB::session_with_cdc(bool)` — `database/mod.rs:1728` (`&self -> Session`)
    /// - `Session::begin_transaction()` — `session/mod.rs:3883` (`&mut self -> Result<()>`).
    ///   **Default isolation is `SnapshotIsolation`** (NOT `Serializable` —
    ///   the Devil's claim was incorrect; verified at
    ///   `transaction/manager.rs:41-56` where `#[default]` is on
    ///   `SnapshotIsolation`). `commit()` is write-only (single
    ///   `create_node_with_props` — no read-then-write race), so
    ///   SnapshotIsolation suffices and Serializable's SSI read-tracking
    ///   would add overhead for no benefit. P2T2's `sync_tree_move_to_grafeo`
    ///   DOES use explicit `Serializable` because its cycle pre-check reads
    ///   the graph inside the tx — leave that as-is.
    /// - `apply_loro_op(&Session, &LoroOp, &BridgeMaps) -> Result<()>` —
    ///   `src/bridge/grafeo_tx.rs:86` (SSOT for "lookup-or-create + insert
    ///   binding" — architecture §20). `commit()` reuses this instead of
    ///   inlining `create_node_with_props` + `BridgeMaps::insert_node` (DRY;
    ///   anti-plenger rule #2 + #9 idempotency).
    /// - `Session::prepare_commit` — `session/mod.rs:4496` (`&mut self -> Result<PreparedCommit<'_>>`)
    /// - `PreparedCommit::set_metadata(impl Into<String>, impl Into<String>)` — `transaction/prepared.rs:107` (advisory; dropped on commit per Devil Gap 1)
    /// - `PreparedCommit::commit(self) -> Result<EpochId>` — `transaction/prepared.rs:124`
    /// - `Session::Drop` auto-rollbacks an un-prepared-commit'd tx
    ///   (`session/mod.rs:5368` — compensation on Grafeo failure is therefore
    ///   just `drop(session)`).
    ///
    /// Loro API (verified against `loro-1.13.6/src/lib.rs`):
    /// - `LoroDoc::new() -> Self` — `lib.rs:137`
    /// - `LoroDoc::get_map<I: IntoContainerId>(&self, I) -> LoroMap` — `lib.rs:489`
    /// - `LoroMap::insert(&self, &str, impl Into<LoroValue>) -> LoroResult<()>` — `lib.rs:2135`
    /// - `LoroMap::delete(&self, &str) -> LoroResult<()>` — `lib.rs:2117` (compensation)
    /// - `LoroMap::get_or_create_container<C: ContainerTrait>(&self, &str, C) -> LoroResult<C>` — `lib.rs:2217` (deprecated in favor of `ensure_mergeable_map` but still functional; L3 may switch if convenient)
    /// - `LoroDoc::set_next_commit_origin(&self, &str)` — `lib.rs:626`
    /// - `LoroDoc::commit(&self)` — `lib.rs:593`
    ///
    /// lorosurgeon API (verified against `lorosurgeon-0.2.1/src/`):
    /// - `RootReconciler::new(LoroMap) -> Self` — `reconcile.rs:298`
    /// - `<VertexEntity as Reconcile>::reconcile<R: Reconciler>(&self, R) -> Result<(), ReconcileError>` — `reconcile.rs:92` (Phase 2 Task 1 verified)
    /// - `<VertexEntity as Hydrate>::hydrate_map(&LoroMap) -> Result<VertexEntity, HydrateError>` — `hydrate.rs:64` (Phase 2 Task 1 verified)
    pub fn commit(self) -> Result<NodeId> {
        // TODO(P2T3-L3): 1. Strict-reject `Vector`/`Map`/`List` properties
        //                  BEFORE any Loro/Grafeo write (Q2 — fail loud):
        //                    for (_k, v) in &self.properties {
        //                        if matches!(v, GraphValue::Vector(_) | GraphValue::Map(_) | GraphValue::List(_)) {
        //                            return Err(GrafeoLoroError::UnsupportedLoroType(format!(
        //                                "VertexBuilder::commit: property has unsupported GraphValue variant {:?} \
        //                                 (LoroProperty supports only Null/Bool/Integer/Float/String; Vector/Map/List \
        //                                 will be wired in Phase 3 §17 vector-offload)", v)));
        //                        }
        //                    }
        // TODO(P2T3-L3): 2. Generate fresh `loro_key` + build `VertexEntity`:
        //                    let loro_key = format!("V/{}", self.loro_key_counter.fetch_add(1, Ordering::Relaxed));
        //                    let entity = VertexEntity {
        //                        labels: self.labels.clone(),
        //                        properties: /* GraphValue → LoroProperty, all-strict-reject checked above */,
        //                        description: String::new(), // default — see struct doc (M3)
        //                    };
        // TODO(P2T3-L3): 3. Acquire Loro write lock + tag origin + reconcile + commit
        //                  (single `RwLock` write guard serializes
        //                  `set_next_commit_origin + commit` per
        //                  `bridge::sync_engine` module doc):
        //                    {
        //                        let doc = self.sync_engine.loro_doc.write();
        //                        doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE); // echo prevention — see B1 filter
        //                        let v_map = doc.get_map(ROOT_VERTICES);
        //                        let node_map = v_map.get_or_create_container(&loro_key, loro::LoroMap::new())?;
        //                        entity.reconcile(lorosurgeon::reconcile::RootReconciler::new(node_map.clone()))?;
        //                        doc.commit(); // fires subscriber synchronously; filtered by origin
        //                    } // release Loro write lock
        // TODO(P2T3-L3): 4. Open Grafeo session (CDC disabled — echo prevention)
        //                  + begin tx (default isolation = SnapshotIsolation; see method doc):
        //                    let mut session = self.sync_engine.grafeo_db.session_with_cdc(false);
        //                    session.begin_transaction()?;
        // TODO(P2T3-L3): 5. Apply via the SSOT apply path (architecture §20 — DRY):
        //                    let op = LoroOp::UpsertNode {
        //                        loro_key: loro_key.clone(),
        //                        labels: self.labels.clone(),
        //                        properties: self.properties.clone(),
        //                    };
        //                    apply_loro_op(&session, &op, self.sync_engine.maps())?;
        //                  - On Err: COMPENSATE Loro (re-acquire write lock,
        //                    `v_map.delete(&loro_key)?`, `doc.commit()` with
        //                    `ORIGIN_LORO_BRIDGE`). Drop `session` (auto-rollback).
        //                    Return the Grafeo error.
        // TODO(P2T3-L3): 6. Prepare + commit Grafeo tx (advisory metadata dropped on commit — Devil Gap 1):
        //                    let mut prepared = session.prepare_commit()?;
        //                    prepared.set_metadata("origin", ORIGIN_LORO_BRIDGE); // advisory only
        //                    let _epoch = prepared.commit()?;
        //                  - On Err: COMPENSATE Loro (same as step 5). Return
        //                    the Grafeo error.
        // TODO(P2T3-L3): 7. Recover the grafeo-assigned `NodeId` from
        //                  `BridgeMaps` (apply_loro_op's apply_upsert_node
        //                  inserted the binding via `maps.insert_node`):
        //                    let grafeo_node_id = self.sync_engine.maps()
        //                        .node_id_map.read().get(&loro_key)
        //                        .copied()
        //                        .expect("apply_loro_op inserted the binding");
        // TODO(P2T3-L3): 8. `Ok(grafeo_node_id)`
        //
        // NOTE: step 14 of the prior P2T3-L1 TODO (defensive epoch
        // side-channel insert) is DELETED — `session_with_cdc(false)` emits
        // no CDC event, so the side-channel is dead code (P2T3-DEVIL m1).
        let _ = (
            self.sync_engine,
            self.loro_key_counter,
            self.labels,
            self.properties,
        );
        Err(GrafeoLoroError::Bridge(
            "VertexBuilder::commit not yet implemented".into(),
        ))
    }
}
