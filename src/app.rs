use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use lorosurgeon::reconcile::RootReconciler;
use lorosurgeon::Reconcile;

use crate::bridge::{apply_loro_op, BridgeMaps, SyncEngine};
use crate::config::{CompressionType, SsotMode};
use crate::constants::{ORIGIN_LORO_BRIDGE, ROOT_VERTICES};
use crate::error::{GrafeoLoroError, Result};
use crate::schema::VertexEntity;
use crate::storage::StorageBackend;
use crate::types::events::LoroOp;
use crate::types::{GraphValue, LoroProperty, NodeId, PresencePayload};

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

    /// Access the underlying [`SyncEngine`]. Exposed (P2T3-L2R2 MAJOR 2) so
    /// integration tests can install the Loro subscriber + inspect
    /// `inbound_event_count` to verify the B1 echo-prevention filter —
    /// previously the filter was dead code in the test suite (P2T3-HUNT
    /// MAJOR 2). Future embedding scenarios may also use this to drive the
    /// engine directly.
    pub fn sync_engine(&self) -> &Arc<SyncEngine> {
        &self.sync_engine
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

    /// Regenerate the embedding vector for a vertex's text field. App-level
    /// wrapper: reads text from Loro, then delegates to
    /// `VectorOffloadManager::handle_text_update` (Task 4) which calls
    /// `generate_local_embedding` (Task 3). NOT Task 3 scope (Task 3 owns only
    /// the leaf `generate_local_embedding` stub); NOT Task 4 scope (Task 4 owns
    /// `VectorOffloadManager::handle_text_update` + `new`). This is a separate
    /// app-facade concern that composes both — Phase 4+ scope (P3T3-DEVIL M2).
    pub async fn generate_embedding(&self, node_id: NodeId, field: &str) -> Result<()> {
        let _ = (node_id, field);
        unimplemented!("generate_embedding is Phase 4+ scope (depends on Task 4's VectorOffloadManager::handle_text_update)")
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
        // 1. Strict-reject `Vector`/`Map`/`List` properties BEFORE any Loro/
        //    Grafeo write (Q2 — fail loud). LoroProperty supports only the
        //    JSON-shaped subset (Null/Bool/Integer/Float/String); the other
        //    GraphValue variants will be wired in Phase 3 §17 vector-offload.
        for v in self.properties.values() {
            if matches!(
                v,
                GraphValue::Vector(_) | GraphValue::Map(_) | GraphValue::List(_)
            ) {
                return Err(GrafeoLoroError::UnsupportedLoroType(format!(
                    "VertexBuilder::commit: property has unsupported GraphValue variant {v:?} \
                     (LoroProperty supports only Null/Bool/Integer/Float/String; \
                     Vector/Map/List will be wired in Phase 3 §17 vector-offload)"
                )));
            }
        }

        // 2. Generate fresh `loro_key` (AtomicU64 counter — see struct doc)
        //    and build the Loro-side `VertexEntity`. The strict reject above
        //    makes the `GraphValue → LoroProperty` conversion total.
        let loro_key = format!("V/{}", self.loro_key_counter.fetch_add(1, Ordering::Relaxed));
        let mut loro_props = HashMap::with_capacity(self.properties.len());
        for (k, v) in &self.properties {
            loro_props.insert(k.clone(), LoroProperty::try_from(v.clone())?);
        }
        let entity = VertexEntity {
            labels: self.labels.clone(),
            properties: loro_props,
            description: String::new(), // default — see struct doc (M3)
        };
        tracing::debug!(
            loro_key = %loro_key,
            labels = ?self.labels,
            property_count = self.properties.len(),
            "VertexBuilder::commit: starting Loro-first atomic write"
        );

        // 3. Acquire Loro write lock + tag origin + reconcile + commit. The
        //    single `RwLock` write guard serializes `set_next_commit_origin +
        //    commit` per the `bridge::sync_engine` module doc (so a peer's
        //    commit cannot interleave and pick up our origin tag).
        //    `ensure_mergeable_map` (loro-1.13.6/src/lib.rs:2247) is the
        //    non-deprecated successor to `get_or_create_container`.
        {
            let doc = self.sync_engine.loro_doc.write();
            doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE); // echo prevention — see B1 filter
            let v_map = doc.get_map(ROOT_VERTICES);
            let node_map = v_map.ensure_mergeable_map(&loro_key)?;
            entity
                .reconcile(RootReconciler::new(node_map))
                .map_err(|e| GrafeoLoroError::Bridge(format!("Loro reconcile failed: {e}")))?;
            doc.commit(); // fires subscriber synchronously; filtered by origin (B1)
        } // release Loro write lock

        // 4. Open Grafeo session (CDC disabled — echo prevention on the
        //    Grafeo→Loro path) + begin tx. Default isolation is
        //    `SnapshotIsolation` (grafeo-engine-0.5.42/src/transaction/manager.rs:55)
        //    — `commit()` is write-only (single `create_node_with_props`),
        //    no read-then-write race, so SnapshotIsolation suffices.
        //
        //    On `begin_transaction()` Err (theoretical — fresh session has
        //    no active tx, so `InvalidState` is impossible), compensate Loro
        //    (step 5 hasn't run yet, so NO BridgeMaps cleanup needed — L2-R2
        //    MAJOR 4 + atomicity contract).
        let mut session = self.sync_engine.grafeo_db.session_with_cdc(false);
        if let Err(raw_err) = session.begin_transaction() {
            let grafeo_err: GrafeoLoroError = raw_err.into();
            compensate_loro_vertex(&self.sync_engine, &loro_key, &grafeo_err, &self.labels, &self.properties);
            return Err(grafeo_err);
        }

        // 5. Apply via the SSOT apply path (architecture §20 — DRY).
        //    `apply_loro_op` looks up `loro_key` in `node_id_map`; on miss,
        //    `create_node_with_props` + `maps.insert_node` (grafeo_tx.rs:124-144).
        //    On Err: COMPENSATE Loro (delete the just-inserted entry); drop
        //    `session` (Drop auto-rollbacks the active tx per
        //    session/mod.rs:5368-5383). Return the ORIGINAL Grafeo error.
        let op = LoroOp::UpsertNode {
            loro_key: loro_key.clone(),
            labels: self.labels.clone(),
            properties: self.properties.clone(),
        };
        if let Err(grafeo_err) = apply_loro_op(&session, &op, self.sync_engine.maps()) {
            compensate_loro_vertex(&self.sync_engine, &loro_key, &grafeo_err, &self.labels, &self.properties);
            drop(session); // auto-rollback Grafeo tx
            return Err(grafeo_err);
        }

        // 6. Prepare + commit Grafeo tx. `set_metadata` is advisory only —
        //    Devil Gap 1 established that grafeo drops it on commit, so the
        //    epoch side-channel (Phase 1) is the real echo-prevention
        //    mechanism on the outbound path (not exercised here since
        //    `session_with_cdc(false)` emits no CDC event).
        //
        //    On `prepare_commit()` Err (theoretical — only fails on
        //    `InvalidState` which is impossible after `begin_transaction`
        //    succeeded), compensate Loro AND remove the BridgeMaps binding
        //    that step 5's `apply_loro_op` inserted, then drop `session` to
        //    auto-rollback the Grafeo tx (L2-R2 MAJOR 3 + atomicity contract).
        let mut prepared = match session.prepare_commit() {
            Ok(p) => p,
            Err(raw_err) => {
                let grafeo_err: GrafeoLoroError = raw_err.into();
                compensate_loro_vertex(&self.sync_engine, &loro_key, &grafeo_err, &self.labels, &self.properties);
                self.sync_engine.maps().remove_node(&loro_key);
                // `session` is dropped on `return` (auto-rollback Grafeo tx
                // via Session::Drop at session/mod.rs:5372-5383). Explicit
                // `drop(session)` here would conflict with the `&mut` borrow
                // held by `PreparedCommit<'_>` in the Ok arm.
                return Err(grafeo_err);
            }
        };
        prepared.set_metadata("origin", ORIGIN_LORO_BRIDGE);
        if let Err(raw_err) = prepared.commit() {
            // `prepared.commit()` sets `finalized = true` BEFORE calling
            // `session.commit()` (transaction/prepared.rs:124-129), so
            // `PreparedCommit::Drop` is a NO-OP. The actual Grafeo rollback
            // happens inside `session.commit()` → `commit_inner()`'s catch
            // block (session/mod.rs:4014-4036), which calls
            // `store.rollback_transaction_properties(transaction_id)` for
            // each touched graph. The session tx is no longer active
            // (`current_transaction` was `take()`'d), so `Session::Drop` is
            // also a no-op. (L2-R2 MINOR 4: corrected misleading comment.)
            let grafeo_err: GrafeoLoroError = raw_err.into();
            compensate_loro_vertex(&self.sync_engine, &loro_key, &grafeo_err, &self.labels, &self.properties);
            // Step 5's `apply_loro_op` inserted a `BridgeMaps` binding for
            // `loro_key → grafeo_node_id`. The Grafeo node was just rolled
            // back, so the binding now points to a phantom NodeId — remove
            // it from BOTH maps atomically (`BridgeMaps::remove_node` at
            // grafeo_tx.rs:52) to honor the atomicity contract (L2-R2 MAJOR 1).
            self.sync_engine.maps().remove_node(&loro_key);
            return Err(grafeo_err);
        }

        // 7. Recover the grafeo-assigned `NodeId` from `BridgeMaps`
        //    (`apply_loro_op`'s `apply_upsert_node` inserted the binding via
        //    `maps.insert_node` at grafeo_tx.rs:142).
        let grafeo_node_id = self
            .sync_engine
            .maps()
            .node_id_map
            .read()
            .get(&loro_key)
            .copied()
            .ok_or_else(|| {
                GrafeoLoroError::Bridge(format!(
                    "BridgeMaps missing binding for {loro_key} after apply_loro_op"
                ))
            })?;

        tracing::debug!(
            loro_key = %loro_key,
            node_id = ?grafeo_node_id,
            "VertexBuilder::commit: atomic write complete"
        );

        // 8. Return the grafeo-assigned NodeId.
        Ok(grafeo_node_id)
    }
}

/// Compensate a `commit()` Loro write by deleting the just-inserted vertex
/// entry under `loro_key` and committing with `ORIGIN_LORO_BRIDGE` (so the
/// delete also bypasses the inbound subscriber filter — P2T3-L2 B1).
///
/// Q7 compensation-failure contract: if the Loro compensation ALSO fails,
/// log at `error!` with full context (loro_key, labels, properties, both
/// errors) and return — the caller returns the ORIGINAL Grafeo error (not
/// the Loro compensation error). The system may be inconsistent (Loro has
/// the vertex, Grafeo does not) — flagged for caller-side retry.
fn compensate_loro_vertex(
    sync_engine: &SyncEngine,
    loro_key: &str,
    grafeo_err: &GrafeoLoroError,
    labels: &[String],
    properties: &HashMap<String, GraphValue>,
) {
    // Hold the Loro write guard across `set_next_commit_origin + delete +
    // commit` per the `bridge::sync_engine` module doc (so no peer commit can
    // interleave and pick up our origin tag).
    let comp_result: std::result::Result<(), loro::LoroError> = {
        let doc = sync_engine.loro_doc.write();
        doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE);
        let v_map = doc.get_map(ROOT_VERTICES);
        match v_map.delete(loro_key) {
            Ok(()) => {
                doc.commit();
                Ok(())
            }
            Err(e) => {
                // Clear the pending `ORIGIN_LORO_BRIDGE` origin tag (P2T3-L2R2
                // MINOR 3). Without this `commit()`, a subsequent Loro write
                // that doesn't call `set_next_commit_origin` would inherit
                // `ORIGIN_LORO_BRIDGE` and be silently filtered by the B1
                // inbound filter. In Phase 2 all Loro writes go through
                // `commit()` or `apply_change_event_to_loro`, both of which
                // set their own origin, so this is defensive — but the cost
                // is one extra `commit()` call (no-op on the doc state since
                // `delete` failed before mutating anything).
                doc.commit();
                Err(e)
            }
        }
    };
    match comp_result {
        Ok(()) => {
            tracing::debug!(
                loro_key = %loro_key,
                grafeo_error = %grafeo_err,
                "VertexBuilder::commit: Loro compensation succeeded (vertex entry deleted)"
            );
        }
        Err(e) => {
            tracing::error!(
                loro_key = %loro_key,
                labels = ?labels,
                properties = ?properties,
                grafeo_error = %grafeo_err,
                loro_compensation_error = %e,
                "VertexBuilder::commit: Loro compensation FAILED after Grafeo error; \
                 system may be inconsistent (Loro has the vertex, Grafeo does not). \
                 Returning original Grafeo error."
            );
        }
    }
}
