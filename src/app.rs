use std::path::PathBuf;
use std::sync::Arc;

use tokio::task::JoinHandle;
use tracing::instrument;

use crate::bridge::{BridgeMaps, SyncEngine};
use crate::compression::wrapper::CompressedPayload;
use crate::config::{CompressionType, SsotMode};
use crate::constants::{
    ORIGIN_LORO_BRIDGE, STORAGE_KEY_BASE_LORO, STORAGE_KEY_DELTA_PREFIX,
};
use crate::error::{GrafeoLoroError, Result};
use crate::hydration::parallel_hydrate_grafeo;
use crate::storage::StorageBackend;
use crate::telemetry::{HealthProbe, HydrationMode, MetricsRegistry, SharedTracer};

/// Top-level app facade. Minimal wrapper philosophy: this struct holds the
/// `SyncEngine` (which owns the raw `GrafeoDB` + `LoroDoc` handles + bridge
/// state) and exposes them via [`Self::grafeo_db`] / [`Self::loro_doc`] /
/// [`Self::bridge_maps`]. Users call native Grafeo / Loro APIs directly; the
/// bridge's background workers sync state transparently.
///
/// # Construction
///
/// Production construction goes through [`GrafeoLoroAppBuilder::build`],
/// which threads the builder slots into [`Self::from_sync_engine_with_telemetry`].
/// Tests use the non-breaking [`Self::from_sync_engine`] shim (delegates with
/// defaults — `SsotMode::Loro`, no storage, `CompressionType::default()`, no
/// telemetry).
///
/// # Persistence + telemetry
///
/// Phase 4 adds three dispatch fields: `ssot_mode`, `storage`, `compression`.
/// `hydrate`/`checkpoint` match on `ssot_mode` and call `storage.load/save/
/// list/delete` + `CompressedPayload::compress`/`decompress` with the
/// configured `compression`.
///
/// Phase 5 adds four telemetry fields:
/// - `metrics: Option<Arc<MetricsRegistry>>` — None in tests, Some in
///   production (built from `opentelemetry::global::meter(...)` in `build()`).
/// - `health: Option<Arc<HealthProbe>>` — None in tests, Some in production.
/// - `tracer: Option<SharedTracer>` — None in tests, Some in production
///   (built from `opentelemetry::global::tracer(...)` in `build()`).
/// - `worker_handles: Option<Vec<JoinHandle<()>>>` — preserves the handles
///   returned by `SyncEngine::spawn_all` (CB-1 forward-compat from P4-HUNT).
///   Consumed by [`Self::shutdown`] in L2/L3 to await worker termination.
///
/// All four default to `None` in [`Self::from_sync_engine`] +
/// [`Self::from_sync_engine_with_config`] (backward compat with existing
/// tests). Production `build()` populates them via
/// [`Self::from_sync_engine_with_telemetry`].
///
/// The remaining `Err(GrafeoLoroError::NotYetImplemented(...))` arms are
/// `SsotMode::Grafeo` checkpoint/hydrate (Phase 5 wal-feature scope). See
/// `GrafeoLoroError::NotYetImplemented`.
pub struct GrafeoLoroApp {
    /// Bidirectional sync engine. SSOT for `LoroDoc`, `GrafeoDB`, `BridgeMaps`,
    /// and the epoch side-channel. Raw handles are exposed via `grafeo_db()`,
    /// `loro_doc()` and `bridge_maps()`.
    pub(crate) sync_engine: Arc<SyncEngine>,
    /// Builder-configured SSOT mode (P4-DEVIL M8). `hydrate`/`checkpoint`
    /// dispatch on this field.
    pub(crate) ssot_mode: SsotMode,
    /// Storage backend for cold-snapshot persistence (P4-DEVIL M8). `None`
    /// for tests that do not exercise `hydrate`/`checkpoint` (the
    /// non-breaking [`Self::from_sync_engine`] constructor passes `None`).
    /// Production `build()` rejects `None` with `Config("storage backend not set")`.
    pub(crate) storage: Option<Arc<dyn StorageBackend>>,
    /// Compression codec for cold snapshots (P4-DEVIL M8). Used by
    /// `checkpoint` to wrap the snapshot bytes and by `hydrate` to decompress.
    /// Defaults to `CompressionType::Zstd` per architecture §24.4.
    pub(crate) compression: CompressionType,
    /// Optional metrics registry (P5-L1 Task 4). `Some` in production
    /// (built from `opentelemetry::global::meter("grafeo-loro")` in `build()`);
    /// `None` in tests. `hydrate` records `hydration_duration` via this handle
    /// (P5-L2 territory).
    pub(crate) metrics: Option<Arc<MetricsRegistry>>,
    /// Optional health probe (P5-L1 Task 3). `Some` in production; `None` in
    /// tests. Exposed via [`Self::health`] for HTTP endpoint wiring (Phase 6).
    pub(crate) health: Option<Arc<HealthProbe>>,
    /// Optional shared tracer (P5-L1 Task 4). `Some` in production (built
    /// from `opentelemetry::global::tracer("grafeo-loro")` in `build()`);
    /// `None` in tests. `hydrate` opens a `cold_start_hydration` parent span
    /// via this handle (P5-L2 territory).
    pub(crate) tracer: Option<SharedTracer>,
    /// Worker `JoinHandle`s preserved from `SyncEngine::spawn_all` (P5-L1
    /// forward-compat with P4-HUNT CB-1). Consumed by [`Self::shutdown`] to
    /// await worker termination + flush telemetry before drop. `None` in
    /// tests / `from_sync_engine*` constructors that do not call `spawn_all`.
    pub(crate) worker_handles: Option<Vec<JoinHandle<()>>>,
}

/// Builder for [`GrafeoLoroApp`]. Fluent setters; call [`build`](Self::build)
/// to validate and spawn the runtime.
///
/// # Phase 5 Task 4 telemetry slots (P5-L1)
///
/// Three new optional slots added: `metrics`, `health`, `tracer`. All
/// default to `None`. Production callers set them via [`.with_metrics(...)`]
/// / [`.with_health(...)`] / [`.with_tracer(...)`] before `build()`. Tests
/// that do not configure telemetry leave them at `None`.
pub struct GrafeoLoroAppBuilder {
    storage: Option<Arc<dyn StorageBackend>>,
    ssot_mode: SsotMode,
    compression: CompressionType,
    sync_compression: CompressionType,
    batch_interval_ms: u64,
    batch_max_size: usize,
    /// Optional on-disk directory for `GrafeoDB` (P4-DEVIL Q5). `None` →
    /// in-memory `GrafeoDB::new_in_memory()` (works for `SsotMode::Loro` +
    /// tests). `Some(p)` → `GrafeoDB::with_config(Config::persistent(p))`
    /// (NOT `GrafeoDB::open` — that is `#[cfg(feature = "wal")]`-gated per
    /// P4-DEVIL B1). `build()` rejects `SsotMode::Grafeo + None` with
    /// `Config("grafeo_dir required for SsotMode::Grafeo")`.
    grafeo_dir: Option<PathBuf>,
    /// Optional metrics registry (P5-L1 Task 4). `Some` in production
    /// (caller pre-builds via `MetricsRegistry::init(global::meter(...))`);
    /// `None` in tests. `build()` threads `Some` into `SyncEngine::with_telemetry`
    /// + `GrafeoLoroApp::metrics`. Devil Q14 — should `build()` construct the
    ///   registry itself from `global::meter("grafeo-loro")`, or require the
    ///   caller to pre-build via `.with_metrics(...)`? L1 leaves the decision
    ///   open; L2 implements per Devil ruling.
    metrics: Option<Arc<MetricsRegistry>>,
    /// Optional health probe (P5-L1 Task 3). `Some` in production (caller
    /// pre-builds via `HealthProbe::new(doc_clone, db_clone)`); `None` in
    /// tests. `build()` threads `Some` into `GrafeoLoroApp::health`.
    health: Option<Arc<HealthProbe>>,
    /// Optional shared tracer (P5-L1 Task 4). `Some` in production (caller
    /// pre-builds via `Arc::new(global::tracer("grafeo-loro"))`); `None` in
    /// tests. `build()` threads `Some` into `SyncEngine::with_telemetry` +
    /// `GrafeoLoroApp::tracer`.
    tracer: Option<SharedTracer>,
}

impl Default for GrafeoLoroAppBuilder {
    /// Defaults match architecture §24.4 (`SsotMode::Loro`,
    /// `CompressionType::Zstd`, `CompressionType::Lz4` for sync, 100 ms /
    /// 256 ops batcher). `storage` + `grafeo_dir` + `metrics` + `health` +
    /// `tracer` default to `None` — `build()` rejects a missing `storage`
    /// for production use (telemetry slots remain `None` if not set).
    fn default() -> Self {
        Self {
            storage: None,
            ssot_mode: SsotMode::default(),
            compression: CompressionType::default(),
            sync_compression: CompressionType::Lz4,
            batch_interval_ms: crate::constants::DEFAULT_BATCH_MS,
            batch_max_size: crate::constants::DEFAULT_BATCH_SIZE,
            grafeo_dir: None,
            metrics: None,
            health: None,
            tracer: None,
        }
    }
}

/// Runtime-resource bundle for [`GrafeoLoroApp::from_sync_engine_with_telemetry`].
/// Groups the 7 non-core construction params (SSOT mode, storage, compression,
/// telemetry handles, worker handles) into a single struct — replaces the
/// prior 8-arg signature (P7 `too_many_arguments` refactor, anti-plenger #5).
pub struct AppTelemetryConfig {
    pub ssot_mode: SsotMode,
    pub storage: Option<Arc<dyn StorageBackend>>,
    pub compression: CompressionType,
    pub metrics: Option<Arc<MetricsRegistry>>,
    pub health: Option<Arc<HealthProbe>>,
    pub tracer: Option<SharedTracer>,
    pub worker_handles: Option<Vec<JoinHandle<()>>>,
}

impl GrafeoLoroApp {
    /// Entry point for the fluent builder.
    pub fn builder() -> GrafeoLoroAppBuilder {
        GrafeoLoroAppBuilder::default()
    }

    /// Construct an app from a pre-built [`SyncEngine`]. Intended for tests
    /// and for future embedding scenarios (e.g. a `GrafeoLoroApp` constructed
    /// from an externally-managed engine). Production code should use
    /// [`Self::builder`].
    ///
    /// Non-breaking shim (P4-DEVIL M8): delegates to
    /// [`Self::from_sync_engine_with_config`] with `SsotMode::default()`
    /// (= `Loro`), `storage = None`, `compression = CompressionType::default()`.
    /// Callers that exercise `hydrate`/`checkpoint` MUST use the explicit
    /// constructor (storage `None` will fail at first dispatch).
    pub fn from_sync_engine(sync_engine: Arc<SyncEngine>) -> Self {
        Self::from_sync_engine_with_config(
            sync_engine,
            SsotMode::default(),
            None,
            CompressionType::default(),
        )
    }

    /// Construct an app from a pre-built [`SyncEngine`] with explicit Phase 4
    /// dispatch fields (P4-DEVIL M8). Production `build()` calls this with
    /// the builder's `ssot_mode` + `storage` + `compression` slots. Tests
    /// that exercise `hydrate`/`checkpoint` dispatch also use this directly.
    ///
    /// `storage` is `Option` so test scenarios that do not exercise the cold
    /// snapshot path can pass `None` without constructing a mock backend
    /// (matches the builder's `storage: Option<Arc<dyn StorageBackend>>` slot).
    /// `hydrate`/`checkpoint` reject `None` at dispatch time with
    /// `Config("storage backend not set")` (defensive — same as `build()`).
    ///
    /// # Phase 5 Task 4 (P5-L1)
    ///
    /// Telemetry fields (`metrics`, `health`, `tracer`, `worker_handles`)
    /// all default to `None` here — this constructor preserves backward
    /// compat with existing tests (no signature change). Production code
    /// that needs telemetry uses [`Self::from_sync_engine_with_telemetry`].
    pub fn from_sync_engine_with_config(
        sync_engine: Arc<SyncEngine>,
        ssot_mode: SsotMode,
        storage: Option<Arc<dyn StorageBackend>>,
        compression: CompressionType,
    ) -> Self {
        Self {
            sync_engine,
            ssot_mode,
            storage,
            compression,
            metrics: None,
            health: None,
            tracer: None,
            worker_handles: None,
        }
    }

    /// Construct an app from a pre-built [`SyncEngine`] with explicit Phase 4
    /// dispatch fields AND Phase 5 telemetry fields (P5-L1 Task 4).
    ///
    /// # L1 contract
    ///
    /// Like [`Self::from_sync_engine_with_config`] but also takes the four
    /// telemetry/lifecycle slots added in P5-L1:
    /// - `metrics: Option<Arc<MetricsRegistry>>` — `Some` in production, `None`
    ///   in tests / dev mode without telemetry configured.
    /// - `health: Option<Arc<HealthProbe>>` — same.
    /// - `tracer: Option<SharedTracer>` — same.
    /// - `worker_handles: Option<Vec<JoinHandle<()>>>` — `Some(handles)` when
    ///   the caller has invoked `SyncEngine::spawn_all`; `None` otherwise.
    ///   Consumed by [`Self::shutdown`] in L2/L3 (CB-1 forward-compat).
    ///
    /// Production `build()` is the sole caller (P5-L2 territory — replaces
    /// the prior `from_sync_engine_with_config` call at the end of `build`).
    pub fn from_sync_engine_with_telemetry(
        sync_engine: Arc<SyncEngine>,
        config: AppTelemetryConfig,
    ) -> Self {
        Self {
            sync_engine,
            ssot_mode: config.ssot_mode,
            storage: config.storage,
            compression: config.compression,
            metrics: config.metrics,
            health: config.health,
            tracer: config.tracer,
            worker_handles: config.worker_handles,
        }
    }

    /// Access the bridge id-mapping state. Alias for [`Self::bridge_maps`].
    /// Kept for backward compat with tests that call `app.maps()`.
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

    /// Raw Grafeo DB. Use native session/Cypher/vector APIs directly.
    ///
    /// The bridge's outbound worker polls Grafeo CDC events and translates
    /// them to Loro mutations — users call native Grafeo APIs and the bridge
    /// syncs transparently in the background.
    pub fn grafeo_db(&self) -> &Arc<grafeo::GrafeoDB> {
        &self.sync_engine.grafeo_db
    }

    /// Raw Loro Doc. Use native CRDT types (Map/Text/Tree) directly.
    ///
    /// The bridge's inbound worker subscribes to Loro doc changes and
    /// translates them to Grafeo mutations — users call native Loro APIs and
    /// the bridge syncs transparently in the background.
    pub fn loro_doc(&self) -> &Arc<parking_lot::RwLock<loro::LoroDoc>> {
        &self.sync_engine.loro_doc
    }

    /// Borrow the underlying `LoroDoc` directly (issue #1 item 4).
    ///
    /// This is the Loro-SSOT ownership accessor: Onde receives a shared
    /// borrow of the doc managed by `grafeo-loro` and calls native Loro
    /// APIs (`doc.get_map(...)`, `doc.commit()`, etc.) directly. Onde must
    /// NOT construct its own `LoroDoc` — that breaks the SSOT contract.
    ///
    /// # Returns
    ///
    /// `&LoroDoc` — a shared borrow. The `parking_lot::RwLock` is held
    /// internally for the duration of the borrow so that the outbound
    /// worker cannot interleave a `set_next_commit_origin + commit` pair
    /// while you're mid-call. (Loro 1.x is `Send + Sync` and most APIs
    /// take `&self`, but the bridge needs the lock to logically serialize
    /// the origin-tagged commit pair — see `bridge::sync_engine` module
    /// doc.)
    ///
    /// # Lifecycle contract (issue #1 item 4)
    ///
    /// - **Ownership**: `grafeo-loro` owns the `LoroDoc`. Onde receives a
    ///   shared borrow only. The doc is dropped when `GrafeoLoroApp::drop`
    ///   runs (after `shutdown()` if called).
    /// - **Drop order**: workers are joined first via `shutdown()`, then
    ///   the `LoroDoc` is dropped. Onde must NOT hold a `&LoroDoc` across
    ///   an `await` point or a `shutdown()` call — use `doc()` per-call.
    /// - **Explicit close**: call `app.shutdown().await` to flush + tear
    ///   down workers before drop. Skipping `shutdown()` leaks workers
    ///   (they'll be killed when the runtime drops, but in-flight flushes
    ///   are lost).
    /// - **Snapshot trigger**: call `app.checkpoint(graph_id).await` to
    ///   write a compressed snapshot to the configured storage backend.
    ///   This does NOT close the doc — the doc remains live and workers
    ///   continue syncing.
    pub fn doc(&self) -> parking_lot::RwLockReadGuard<'_, loro::LoroDoc> {
        self.sync_engine.loro_doc.read()
    }

    /// Subscribe to inbound + outbound bridge events (issue #1 item 4).
    ///
    /// Multiple `subscribe()` calls coexist — Onde's orchestrator and
    /// grafeo-loro's internal subscriber both attach without excluding
    /// each other. The handler is invoked from the Loro subscriber thread
    /// (synchronously inside `LoroDoc::commit`), so it MUST be fast and
    /// non-blocking. For expensive work, forward to a channel.
    ///
    /// # Event ordering
    ///
    /// When both Onde's handler and grafeo-loro's bridge subscribe to the
    /// same Loro doc, Loro invokes subscribers in registration order.
    /// grafeo-loro's bridge subscribes first (during `GrafeoLoroApp::build`),
    /// so the bridge's translation runs before Onde's handler. This means
    /// Onde's handler sees the post-bridge state — useful for orchestrator
    /// bookkeeping but NOT for intercepting ops (use the inbound MPSC
    /// channel for that).
    ///
    /// # Handler signature
    ///
    /// `Fn(&loro::diff::DiffBatch) + Send + Sync + 'static`. The handler
    /// receives the raw Loro diff batch; grafeo-loro does NOT re-shape it.
    /// Onde filters by origin if it needs to (the diff's `origin` field
    /// tells you whether the commit came from the user, the bridge, or a
    /// remote peer).
    ///
    /// # Returns
    ///
    /// A `loro::Subscription` — drop it to unsubscribe. Keep it alive for
    /// as long as you want events; if it drops, your handler stops being
    /// called.
    pub fn subscribe<F>(&self, handler: F) -> loro::Subscription
    where
        F: Fn(&loro::diff::DiffBatch) + Send + Sync + 'static,
    {
        // Loro's `subscribe` is on the root container. We expose the same
        // API so Onde's orchestrator can attach without knowing the
        // internal container layout.
        let doc = self.sync_engine.loro_doc.read();
        doc.subscribe_root(handler)
    }

    /// Bridge maps for advanced introspection (loro_key ↔ grafeo::NodeId
    /// and EdgeKey ↔ grafeo::EdgeId bindings).
    ///
    /// Most users do not need this — it's exposed for tooling that needs to
    /// inspect the bridge's id-mapping state (e.g. debug tooling, admin UIs).
    pub fn bridge_maps(&self) -> &Arc<BridgeMaps> {
        &self.sync_engine.maps
    }

    /// Snapshot of the builder-configured SSOT mode (P4-L3 — test hook for
    /// verifying `GrafeoLoroAppBuilder::build` threads the slot through).
    pub fn ssot_mode(&self) -> SsotMode {
        self.ssot_mode
    }

    /// Snapshot of the builder-configured compression codec (P4-L3 — test hook
    /// for verifying `GrafeoLoroAppBuilder::build` threads the slot through).
    pub fn compression(&self) -> CompressionType {
        self.compression
    }

    /// Access the optional metrics registry (P5-L1). `Some` in production,
    /// `None` in tests. Used by `hydrate` (P5-L2 will call
    /// `metrics.record_hydration(...)` after `parallel_hydrate_grafeo`).
    pub fn metrics(&self) -> Option<&Arc<MetricsRegistry>> {
        self.metrics.as_ref()
    }

    /// Access the optional health probe (P5-L1). `Some` in production, `None`
    /// in tests. Used by an HTTP endpoint (Phase 6 hardening) + by `shutdown`
    /// (P5-L2/L3 will call `health.check(...)` before tearing down workers).
    pub fn health(&self) -> Option<&Arc<HealthProbe>> {
        self.health.as_ref()
    }

    /// Access the optional shared tracer (P5-L1). `Some` in production, `None`
    /// in tests. Used by `hydrate` (P5-L2 will open a `cold_start_hydration`
    /// parent span via `crate::telemetry::traces::create_cold_start_span`) +
    /// `query` (P5-L2 will open a `hybrid_query` parent span).
    pub fn tracer(&self) -> Option<&SharedTracer> {
        self.tracer.as_ref()
    }

    /// Access the worker `JoinHandle`s preserved from `SyncEngine::spawn_all`
    /// (P5-L1 CB-1 forward-compat). `Some` in production (populated by
    /// `build()`); `None` in tests / `from_sync_engine*` constructors.
    /// Consumed by [`Self::shutdown`] in L2/L3 to await worker termination.
    pub fn worker_handles(&self) -> Option<&[JoinHandle<()>]> {
        self.worker_handles.as_deref()
    }

    /// Export a shallow snapshot and persist via the storage backend.
    ///
    /// # Phase 4 Task 3 scope (P4T3-L2)
    ///
    /// Dispatches on `self.ssot_mode` (P4-DEVIL M8 — the field is now on
    /// `GrafeoLoroApp`, threaded through `from_sync_engine_with_config` by
    /// `build()`).
    ///
    /// ## `SsotMode::Loro` (architecture §4 Step D — "History discarded to prevent storage bloat")
    ///
    /// 1. `LoroDoc::oplog_frontiers()` (verified at `loro-1.13.6/src/lib.rs:948`)
    ///    — capture the current frontiers for the shallow snapshot.
    /// 2. `LoroDoc::export(ExportMode::shallow_snapshot(&frontiers))`
    ///    (verified at `loro-internal-1.13.6/src/encoding.rs:108`) — produces
    ///    a shallow snapshot: current state + partial history since frontiers
    ///    (history-trimmed, per architecture §4 Step D).
    /// 3. `CompressedPayload::compress_to_wire(&bytes, self.compression)`
    ///    (verified at `src/compression/wrapper.rs:125`) — wrap under the
    ///    builder-configured codec + serialize to the on-wire format
    ///    `[version:u8][codec_tag:u8][raw_data..]` (P4-DEVIL m2 — L3 scope).
    /// 4. `StorageBackend::save(format!("{graph_id}/{STORAGE_KEY_BASE_LORO}"),
    ///    wire_bytes)` — overwrite the base snapshot with the wire-format bytes.
    /// 5. `StorageBackend::list(format!("{graph_id}/{STORAGE_KEY_DELTA_PREFIX}"))`
    ///    — enumerate existing delta keys.
    /// 6. For each delta key, `StorageBackend::delete(key)` — clear deltas
    ///    now folded into the base snapshot. Delete failures are logged as a
    ///    warn and swallowed (anti-plenger #9 idempotent retry — the next
    ///    checkpoint retries; orphan deltas are re-imported harmlessly by
    ///    `hydrate` via `trim_the_known_part_of_change`).
    ///
    ///    # Atomicity (P4-DEVIL Q3)
    ///
    ///    Orphan-delta risk accepted (option (c)): if step 4 succeeds but
    ///    step 6 fails partway, the next `hydrate` re-imports the orphan
    ///    deltas harmlessly. Deduplication is automatic via Loro's
    ///    `OpLog::trim_the_known_part_of_change`
    ///    (`loro-internal-1.13.6/src/oplog.rs:350`) — NOT via
    ///    `ImportStatus::pending` (P4-DEVIL M2: `pending` is missing-dep
    ///    tracking, NOT dedup).
    ///
    /// ## `SsotMode::Grafeo` (architecture §4 Step D) — **deferred to Phase 5**
    ///
    /// P4-DEVIL Q2 decision (option (d)): the `SsotMode::Grafeo` arm returns
    /// `Err(GrafeoLoroError::NotYetImplemented("requires wal feature + ArcSwap
    ///    grafeo_db field"))` for Phase 4/7. The future-phase plan:
    ///
    /// 1. Flush the on-disk `GrafeoDB` to its directory — `GrafeoDB::close()`
    ///    takes `&self` (NOT `self` — verified at
    ///    `grafeo-engine-0.5.42/src/database/mod.rs:2229`; P4-DEVIL M3).
    ///    `close()` flushes the WAL + file_manager and sets `is_open = false`,
    ///    but the `Arc<GrafeoDB>` handle remains in memory. Subsequent
    ///    operations on the closed DB will fail. P5 should prefer
    ///    `GrafeoDB::backup_full(&backup_dir)` (non-destructive — takes
    ///    `&self`, does NOT close) when the `wal` feature is enabled.
    /// 2. Tar the `GrafeoDB` directory (or `backup_full`'s output dir).
    ///    `// TODO(P5): add `tar = "0.4"` to Cargo.toml.`
    /// 3. `CompressedPayload::compress(&tar_bytes, CompressionType::Zstd)`.
    /// 4. `StorageBackend::save(format!("{graph_id}/{STORAGE_KEY_GRAFEO_TAR_ZST}"),
    ///    payload.raw_data)`.
    /// 5. Reopen the `GrafeoDB` via `GrafeoDB::with_config(Config::persistent(
    ///    same_dir))` (NOT `GrafeoDB::open` — that is `#[cfg(feature = "wal")]`-
    ///    gated per P4-DEVIL B1; `with_config` at
    ///    `grafeo-engine-0.5.42/src/database/mod.rs:346` is unconditionally
    ///    compiled). Rebinding the new `Arc<GrafeoDB>` into
    ///    `SyncEngine.grafeo_db` requires the B2 fix (`Arc<RwLock<Arc<GrafeoDB>>>`
    ///    or `ArcSwap<GrafeoDB>` field type — P4-DEVIL B2).
    ///
    /// # Concurrency (P4-DEVIL Q4)
    ///
    /// Caller MUST serialize `checkpoint` with concurrent `hydrate` and any
    /// in-flight vertex mutations. No internal lock; Phase 4 trusts the
    /// orchestrator (validation test is sequential). A `RwLock<HashSet<graph_id>>`
    /// may be added in Phase 5 if a multi-tenant use case requires it.
    ///
    /// # Errors
    ///
    /// - `GrafeoLoroError::Config("storage backend not set")` if `self.storage`
    ///   is `None` (defensive — `build()` also rejects this).
    /// - `GrafeoLoroError::Loro` for `LoroDoc::export` failures (Loro encode
    ///   errors routed via `#[from] loro::LoroError` at `src/error.rs:6`).
    /// - `GrafeoLoroError::Compression` for `CompressedPayload::compress`
    ///   failures (zstd/lz4 codec errors).
    /// - `GrafeoLoroError::StorageIo` for `StorageBackend::save` / `list` /
    ///   `delete` failures (routed via `#[from] std::io::Error` at
    ///   `src/error.rs:12`).
    ///
    /// # Idempotency
    ///
    /// Calling `checkpoint(graph_id)` twice in succession is a no-op on the
    /// second call IF the Loro doc has not been mutated between calls — the
    /// storage key is overwritten unconditionally (last writer wins).
    #[instrument(skip(self), level = "info")]
    pub async fn checkpoint(&self, graph_id: &str) -> Result<()> {
        // Manual span (P4-DEVIL Q4 observability) — equivalent to
        // `#[instrument(skip(self), fields(graph_id = %graph_id))]` but without
        // enabling the `attributes` feature on `tracing` (anti-plenger #10 —
        // fewest LOC, no Cargo.toml change).
        let span = tracing::info_span!(
            "checkpoint",
            graph_id = %graph_id,
            ssot_mode = ?self.ssot_mode
        );
        let _enter = span.enter();

        let storage = self
            .storage
            .as_ref()
            .ok_or_else(|| GrafeoLoroError::Config("storage backend not set".into()))?;

        match self.ssot_mode {
            SsotMode::Loro => {
                // Step 1: oplog_frontiers for shallow snapshot.
                let frontiers = {
                    let doc = self.sync_engine.loro_doc.read();
                    doc.oplog_frontiers()
                };
                tracing::debug!(?frontiers, "checkpoint: oplog_frontiers");

                // Step 2: export shallow snapshot.
                //
                // Verified API: `ExportMode::shallow_snapshot(&Frontiers)` at
                // `loro-internal-1.13.6/src/encoding.rs:108` (re-exported as
                // `loro::ExportMode::shallow_snapshot` at `loro-1.13.6/src/lib.rs:56`).
                // P4-DEVIL m2 (architecture §4 Step D "History discarded to
                // prevent storage bloat"): `shallow_snapshot` is the right
                // variant — produces current state + partial history since
                // `frontiers` (history-trimmed). NOT the deep `ExportMode::Snapshot`
                // variant (full history) — would re-bloat storage on each
                // checkpoint. NOT `ExportMode::StateOnly` either — that drops
                // too much and would break `import_with` on `hydrate`.
                let snapshot_bytes = {
                    let doc = self.sync_engine.loro_doc.read();
                    doc.export(loro::ExportMode::shallow_snapshot(&frontiers))
                        .map_err(|e| GrafeoLoroError::Loro(e.into()))?
                };
                tracing::debug!(
                    bytes = snapshot_bytes.len(),
                    "checkpoint: shallow snapshot exported"
                );

                // Step 3: compress under the configured codec + serialize to
                // the wire format (P4-DEVIL m2 — `compress_to_wire` produces
                // `[version:u8][codec_tag:u8][raw_data..]` so `hydrate`'s
                // `decompress_from_wire` knows which codec to use).
                let wire_bytes =
                    CompressedPayload::compress_to_wire(&snapshot_bytes, self.compression)?;

                // Step 4: save base snapshot (overwrites any prior).
                let base_key = format!("{graph_id}/{STORAGE_KEY_BASE_LORO}");
                tracing::debug!(key = %base_key, "checkpoint: saving base snapshot");
                storage.save(&base_key, wire_bytes).await?;

                // Step 5+6: list + delete delta keys.
                //
                // P4-DEVIL Q3: orphan-delta risk accepted — if step 6 fails
                // partway, the next hydrate re-imports the orphan deltas
                // harmlessly (dedup via `trim_the_known_part_of_change`, NOT
                // `ImportStatus::pending` per P4-DEVIL M2).
                //
                // P4-DEVIL M1: Phase 4 has no delta-write path — the list is
                // always empty. The loop runs zero times.
                let delta_prefix = format!("{graph_id}/{STORAGE_KEY_DELTA_PREFIX}");
                tracing::debug!(
                    prefix = %delta_prefix,
                    "checkpoint: listing delta keys for deletion"
                );
                let delta_keys = storage.list(&delta_prefix).await?;
                // P4-DEVIL Q3 + anti-plenger #9 idempotent retry: log + continue
                // on delete failure. The next `checkpoint` retries; orphan
                // deltas are re-imported harmlessly by `hydrate` (dedup via
                // `trim_the_known_part_of_change` at loro-internal-1.13.6/src/oplog.rs:350,
                // NOT via `ImportStatus::pending` per P4-DEVIL M2).
                for k in &delta_keys {
                    tracing::debug!(key = %k, "checkpoint: deleting delta");
                    if let Err(e) = storage.delete(k).await {
                        tracing::warn!(
                            key = %k,
                            error = %e,
                            "checkpoint: delta delete failed; will retry next checkpoint"
                        );
                    }
                }

                tracing::info!(
                    delta_count = delta_keys.len(),
                    "checkpoint: complete (Loro mode)"
                );
                Ok(())
            }
            SsotMode::Grafeo => {
                // P4-DEVIL Q2/B1/B2/M3: deferred to Phase 5.
                // B1: GrafeoDB::backup_full is `#[cfg(all(feature = "wal",
                //     feature = "grafeo-file", feature = "lpg"))]`-gated.
                // B2: SyncEngine.grafeo_db: Arc<GrafeoDB> cannot be rebound
                //     after close+reopen.
                // M3: GrafeoDB::close(&self) does NOT drop the Arc handle —
                //     would leave SyncEngine with a closed handle.
                // P5 needs: wal feature + tar crate + ArcSwap grafeo_db field
                //           + non-destructive backup_full.
                return Err(GrafeoLoroError::NotYetImplemented(
                    "SsotMode::Grafeo checkpoint: requires wal feature + ArcSwap grafeo_db field"
                        .into(),
                ));
            }
        }
    }

    /// Cold-boot hydration: download + restore graph state from the storage
    /// backend into both `LoroDoc` and `GrafeoDB`.
    ///
    /// # Phase 4 Task 2 scope (P4T2-L2)
    ///
    /// Dispatches on `self.ssot_mode` (P4-DEVIL M8 — the field is now on
    /// `GrafeoLoroApp`, threaded through `from_sync_engine_with_config` by
    /// `build()`).
    ///
    /// ## `SsotMode::Loro` (architecture §4 Step A)
    ///
    /// 1. `StorageBackend::load(format!("{graph_id}/{STORAGE_KEY_BASE_LORO}"))`
    ///    — download the base snapshot (`LoroDoc::export(ExportMode::Snapshot)`
    ///    bytes). `StorageIo(io::ErrorKind::NotFound)` is the "fresh graph"
    ///    case — initialize an empty `LoroDoc` and skip ahead to step 5
    ///    (parallel hydrate over an empty doc is a no-op).
    /// 2. `CompressedPayload::decompress_from_wire(&bytes)` (verified at
    ///    `src/compression/wrapper.rs:133`) — parse the on-wire format
    ///    `[version:u8][codec_tag:u8][raw_data..]` and decompress under the
    ///    tagged codec (P4-DEVIL m2 — L3 scope). Rejects unknown versions /
    ///    codec tags with `GrafeoLoroError::Compression(...)`.
    /// 3. `LoroDoc::import_with(&bytes, ORIGIN_LORO_BRIDGE)` (verified at
    ///    `loro-1.13.6/src/lib.rs:721` — P4-DEVIL M10 + n1: `import_with`
    ///    tags the import for the B1 echo filter at
    ///    `src/bridge/sync_engine.rs:234`, which skips events whose origin
    ///    matches `ORIGIN_LORO_BRIDGE`. This is what makes the architecture
    ///    §24.2 `build → hydrate` ordering safe — the subscriber is active
    ///    when `hydrate` runs, but the import's events are filtered out and
    ///    do not re-trigger `apply_loro_op` on the inbound batcher.) —
    ///    surfaces `ImportStatus`. `status.pending.is_some()` (P4-DEVIL m3 —
    ///    NOT "non-empty `pending`") means missing-dependency changes were
    ///    deferred; for Phase 4 self-contained base snapshots this is always
    ///    `None`. `pending` is NOT a dedup mechanism (P4-DEVIL M2 — dedup is
    ///    automatic via `trim_the_known_part_of_change` at
    ///    `loro-internal-1.13.6/src/oplog.rs:350`).
    /// 4. `StorageBackend::list(format!("{graph_id}/{STORAGE_KEY_DELTA_PREFIX}"))`
    ///    — enumerate delta keys; for each, `load` + `decompress` +
    ///    `import_with(ORIGIN_LORO_BRIDGE)`.
    ///
    ///    # Phase 4 scope (P4-DEVIL M1)
    ///
    ///    No delta-WRITE path exists in Phase 4 — `checkpoint` writes only
    ///    the base snapshot. The delta-listing returns `Ok(vec![])` and the
    ///    import loop runs zero times. The delta constants
    ///    (`STORAGE_KEY_DELTA_PREFIX` / `_SUFFIX`) are reserved for the
    ///    Phase 5+ Loro sync wire-protocol path (architecture §4 Step C
    ///    `doc.export(ExportMode::updates)`).
    /// 5. `parallel_hydrate_grafeo(&grafeo_db, &loro_doc, &bridge_maps)`
    ///    (verified at `src/hydration/parallel.rs:40`) — rebuilds Grafeo
    ///    indexes from Loro state in rayon chunks. Writes to Grafeo (NOT
    ///    Loro) + uses `session_with_cdc(false)` — no echo through the Loro
    ///    subscriber even when the subscriber is active (P4-DEVIL M10).
    ///
    /// ## `SsotMode::Grafeo` (architecture §4 Step A) — **deferred to Phase 5**
    ///
    /// P4-DEVIL Q2 decision (option (d)): the `SsotMode::Grafeo` arm returns
    /// `Err(GrafeoLoroError::NotYetImplemented("requires wal feature + ArcSwap
    ///    grafeo_db field"))` for Phase 4/7. The future-phase plan:
    ///
    /// 1. `StorageBackend::load(format!("{graph_id}/{STORAGE_KEY_GRAFEO_TAR_ZST}"))`
    ///    — download the compressed tarball. `NotFound` = fresh graph.
    /// 2. `zstd::stream::decode_all` (verified at
    ///    `zstd-0.13.3/src/stream/functions.rs:8`) — decompress.
    /// 3. Extract the tar stream to a temporary directory. `// TODO(P5): add
    ///    `tar = "0.4"` to Cargo.toml.`
    /// 4. `GrafeoDB::with_config(Config::persistent(extracted_dir))` (NOT
    ///    `GrafeoDB::open` — that is `#[cfg(feature = "wal")]`-gated per
    ///    P4-DEVIL B1) — attach to the restored on-disk DB. Rebinding the
    ///    new `Arc<GrafeoDB>` into `SyncEngine.grafeo_db` requires the B2
    ///    fix (`Arc<RwLock<Arc<GrafeoDB>>>` or `ArcSwap<GrafeoDB>`).
    /// 5. Rebuild the live `LoroDoc` from the restored Grafeo state via
    ///    `parallel_hydrate_loro` (P4-DEVIL Q6/M4 — L3 scope; mirror of
    ///    `parallel_hydrate_grafeo` using `graph_store().node_ids()` +
    ///    `entity.reconcile(RootReconciler::new(node_map))` per vertex).
    ///
    ///    # Echo-prevention precondition (P4-DEVIL M6)
    ///
    ///    The Grafeo→Loro reconciliation in step 5 triggers one Loro commit
    ///    per vertex/edge (`entity.reconcile(...)` + `doc.commit()`). P5
    ///    MUST wrap each commit with `doc.set_next_commit_origin(
    ///    ORIGIN_LORO_BRIDGE)` BEFORE `doc.commit()`. Otherwise the active
    ///    subscriber translates each diff to `LoroOp::UpsertNode` and pushes
    ///    to the batcher, which re-creates the vertex in Grafeo (duplicate).
    ///    The B1 filter at `src/bridge/sync_engine.rs:234` skips events
    ///    tagged with `ORIGIN_LORO_BRIDGE`, so the echo is suppressed.
    ///
    /// # Preconditions
    ///
    /// - For BOTH `SsotMode::Loro` AND `SsotMode::Grafeo` (P4-DEVIL M6):
    ///   either the subscriber is inactive OR all hydrate-side Loro commits
    ///   are tagged with `ORIGIN_LORO_BRIDGE` (M10) so the B1 filter skips
    ///   them. `SsotMode::Loro` uses `import_with(ORIGIN_LORO_BRIDGE)`;
    ///   `SsotMode::Grafeo` (P5) will use `set_next_commit_origin` per commit.
    /// - `GrafeoDB` is empty (cold) — `parallel_hydrate_grafeo` will create
    ///   duplicates otherwise (per its idempotency assumption at
    ///   `src/hydration/parallel.rs:39`).
    /// - `BridgeMaps` is empty (cold) — same reason.
    ///
    /// # Errors
    ///
    /// - `GrafeoLoroError::Config("storage backend not set")` if `self.storage`
    ///   is `None`.
    /// - `GrafeoLoroError::StorageIo` for backend I/O failures (except
    ///   `io::ErrorKind::NotFound` on the base/tarball key, which is the
    ///   "fresh graph" path).
    /// - `GrafeoLoroError::Compression` for `CompressedPayload::decompress` /
    ///   `zstd::stream::decode_all` failures.
    /// - `GrafeoLoroError::Loro` for `LoroDoc::import_with` failures.
    /// - `GrafeoLoroError::Grafeo` for per-chunk tx failures during
    ///   `parallel_hydrate_grafeo`.
    /// - `GrafeoLoroError::Hydrate` for `VertexEntity::hydrate_map` field-shape
    ///   mismatches during `parallel_hydrate_grafeo`.
    /// - `GrafeoLoroError::Bridge` for vertex missing from LoroMap / wrong
    ///   container type during `parallel_hydrate_grafeo`.
    ///
    /// # Idempotency
    ///
    /// Calling `hydrate(graph_id)` twice on a non-cold `GrafeoDB` /
    /// `BridgeMaps` produces duplicate vertices (per
    /// `parallel_hydrate_grafeo`'s idempotency assumption). Caller
    /// responsibility: only call once at cold boot. The orchestrator's
    /// `builder().build().await` + `hydrate()` sequence (architecture §24.2)
    /// is the canonical pattern.
    #[instrument(skip(self), level = "info")]
    pub async fn hydrate(&self, graph_id: &str) -> Result<()> {
        // Manual span (P4-DEVIL M6/M10 observability) — equivalent to
        // `#[instrument(skip(self), fields(graph_id = %graph_id))]` but without
        // enabling the `attributes` feature on `tracing` (anti-plenger #10 —
        // fewest LOC, no Cargo.toml change).
        let span = tracing::info_span!(
            "hydrate",
            graph_id = %graph_id,
            ssot_mode = ?self.ssot_mode
        );
        let _enter = span.enter();

        // P5-L3: open `cold_start_hydration` OTel parent span (architecture
        // §23.2 tree row 1) — wraps the entire cold-start sequence (storage
        // load → decompress → import → parallel_hydrate → re-seed counter).
        // `parallel_hydrate_grafeo` opens the `parallel_hydrate_grafeo` child
        // span (row 1.3); `cold_start_hydration` is held for the whole
        // function body. Drops on function return.
        let _cold_start_span = self
            .tracer
            .as_ref()
            .map(|t| crate::telemetry::traces::create_cold_start_span(t.as_ref()));

        // P5-L3: capture start time for the `hydration_duration` histogram
        // (architecture §23.1 row 5). Recorded AFTER `parallel_hydrate_grafeo`
        // returns — measures the parallel Grafeo hydration wall-clock. The
        // cold-start sequence as a whole is timed by the `_cold_start_span`
        // span lifetime (separate observability axis).
        let hydrate_started = std::time::Instant::now();

        let storage = self
            .storage
            .as_ref()
            .ok_or_else(|| GrafeoLoroError::Config("storage backend not set".into()))?;

        match self.ssot_mode {
            SsotMode::Loro => {
                let base_key = format!("{graph_id}/{STORAGE_KEY_BASE_LORO}");

                // Step 1: load base snapshot (NotFound = fresh graph).
                tracing::debug!(key = %base_key, "hydrate: loading base snapshot");
                let base_bytes = match storage.load(&base_key).await {
                    Ok(b) => b,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        tracing::info!(
                            key = %base_key,
                            "hydrate: base snapshot not found — fresh graph"
                        );
                        Vec::new()
                    }
                    Err(e) => return Err(GrafeoLoroError::from(e)),
                };

                if !base_bytes.is_empty() {
                    // Step 2: decompress the base snapshot via the wire-format
                    // helper (P4-DEVIL m2 — `decompress_from_wire` parses
                    // `[version:u8][codec_tag:u8][raw_data..]` and dispatches
                    // to the matching codec). Rejects unknown versions / tags.
                    let loro_bytes = CompressedPayload::decompress_from_wire(&base_bytes)?;

                    // Step 3: import into LoroDoc with ORIGIN_LORO_BRIDGE tag
                    // so the B1 filter at sync_engine.rs:270 skips the echo
                    // (P4-DEVIL M10).
                    tracing::debug!(
                        bytes = loro_bytes.len(),
                        "hydrate: importing base into LoroDoc"
                    );
                    {
                        let doc = self.sync_engine.loro_doc.write();
                        let status = doc.import_with(&loro_bytes, ORIGIN_LORO_BRIDGE)?;
                        // P4-DEVIL M2/m3: pending.is_some() = missing-dep
                        // tracking, NOT dedup. Dedup is automatic via
                        // trim_the_known_part_of_change (oplog.rs:350).
                        if status.pending.is_some() {
                            // Phase 4 self-contained snapshots should always
                            // have `pending == None`. A non-None value means
                            // the snapshot was exported with a frontier the
                            // hydrated doc cannot yet resolve. Phase 5+ Loro
                            // sync wire will fetch the missing ranges via
                            // `doc.export(ExportMode::updates(&oplog_vv()))`
                            // and re-import; for Phase 4 we surface this as a
                            // warn so the operator knows the cold-boot is
                            // incomplete (anti-plenger #10 observability).
                            tracing::warn!(
                                ?status.pending,
                                "hydrate: ImportStatus.pending.is_some() — \
                                 missing dependencies (Phase 4 self-contained \
                                 snapshots should always be None)"
                            );
                        }
                    }

                    // Step 4: enumerate + import delta keys.
                    //
                    // P4-DEVIL M1: Phase 4 has no delta-write path — the list
                    // is always empty. The loop runs zero times. L3 implements
                    // the real loop for forward-compat with Phase 5's Loro sync
                    // wire-protocol path (architecture §4 Step C
                    // `doc.export(ExportMode::updates)`).
                    let delta_prefix = format!("{graph_id}/{STORAGE_KEY_DELTA_PREFIX}");
                    tracing::debug!(
                        prefix = %delta_prefix,
                        "hydrate: listing delta keys"
                    );
                    let mut delta_keys = storage.list(&delta_prefix).await?;
                    // Lexicographic sort matches numeric epoch order IF the
                    // epoch slot is zero-padded to a fixed width (e.g. 20
                    // digits for u64::MAX). `STORAGE_KEY_DELTA_PREFIX`'s
                    // doc-comment does NOT mandate padding, so this is a
                    // forward-compat assumption: Phase 5+ Loro sync wire MUST
                    // zero-pad the `{epoch}` slot — see `src/constants.rs:122`.
                    // Phase 4 has no delta-write path so the assumption is
                    // vacuous in practice (the list is empty).
                    delta_keys.sort_unstable();
                    let mut imported = 0usize;
                    for k in &delta_keys {
                        tracing::debug!(key = %k, "hydrate: loading delta");
                        let delta_bytes = match storage.load(k).await {
                            Ok(b) => b,
                            Err(e) => {
                                // Idempotent retry: a missing delta between
                                // `list` and `load` (e.g. another writer
                                // checkpointed) is recoverable on the next
                                // hydrate — log + continue (anti-plenger #9).
                                tracing::warn!(
                                    key = %k,
                                    error = %e,
                                    "hydrate: delta load failed; skipping (next hydrate retries)"
                                );
                                continue;
                            }
                        };
                        let delta_loro_bytes =
                            CompressedPayload::decompress_from_wire(&delta_bytes)?;
                        let status = {
                            let doc = self.sync_engine.loro_doc.write();
                            doc.import_with(&delta_loro_bytes, ORIGIN_LORO_BRIDGE)?
                        };
                        if status.pending.is_some() {
                            tracing::warn!(
                                key = %k,
                                ?status.pending,
                                "hydrate: delta ImportStatus.pending.is_some() — \
                                 missing dependencies (Phase 4 self-contained \
                                 deltas should always be None)"
                            );
                        }
                        imported += 1;
                    }
                    tracing::info!(
                        delta_count = delta_keys.len(),
                        imported,
                        "hydrate: delta import complete"
                    );
                }

                // Step 5: parallel_hydrate_grafeo from Loro state.
                //
                // Precondition (src/hydration/parallel.rs:23-29): subscriber
                // NOT yet active. P4-DEVIL M10: hydrate runs AFTER build() →
                // spawn_all, so subscriber IS active. `parallel_hydrate_grafeo`
                // writes to Grafeo (not Loro) + uses `session_with_cdc(false)`
                // — no echo through the Loro subscriber. The `LoroDoc::import_with`
                // above is the only Loro write and it is tagged with
                // ORIGIN_LORO_BRIDGE so the B1 filter skips it.
                tracing::info!("hydrate: parallel_hydrate_grafeo (Loro → Grafeo)");
                {
                    let doc = self.sync_engine.loro_doc.read();
                    // P5-L2 wiring (Devil M3): thread telemetry handles into
                    // `parallel_hydrate_grafeo` so L3 can emit the
                    // `parallel_hydrate_grafeo` child span (architecture §23.2
                    // tree row 1.3) + record `hydration_duration` histogram.
                    // `None, None` only fires when the app was built without
                    // telemetry (test mode); production builds thread `Some`.
                    parallel_hydrate_grafeo(
                        &self.sync_engine.grafeo_db,
                        &doc,
                        self.sync_engine.maps(),
                        self.metrics.as_ref(),
                        self.tracer.as_ref(),
                    )?;
                }
                // P5-L3: record `hydration_duration` histogram (architecture
                // §23.1 row 5) with the `mode` label mapped from `SsotMode`.
                // `SsotMode::Loro → HydrationMode::Loro` (this arm);
                // `SsotMode::Grafeo → HydrationMode::Grafeo` (the Grafeo arm
                // below is unimplemented — P5 wal-feature scope). The mapping
                // is inline (NOT a `From<SsotMode>` impl) — fewer LOC + no
                // new trait impl to maintain (anti-plenger #5 Bloat).
                if let Some(m) = &self.metrics {
                    let mode = match self.ssot_mode {
                        SsotMode::Loro => HydrationMode::Loro,
                        SsotMode::Grafeo => HydrationMode::Grafeo,
                    };
                    let elapsed_ms = hydrate_started.elapsed().as_secs_f64() * 1000.0;
                    m.record_hydration(elapsed_ms, mode);
                }

                tracing::info!("hydrate: complete (Loro mode)");
                Ok(())
            }
            SsotMode::Grafeo => {
                // P4-DEVIL Q2/B1/B2: deferred to Phase 5.
                // B1: GrafeoDB::open is #[cfg(feature = "wal")]-gated.
                // B2: SyncEngine.grafeo_db: Arc<GrafeoDB> cannot be rebound
                //     after restore.
                // M6: P5 needs `set_next_commit_origin(ORIGIN_LORO_BRIDGE)`
                //     before each `doc.commit()` in `parallel_hydrate_loro`.
                // Q6/M4: P5 needs `parallel_hydrate_loro` (mirror of
                //     `parallel_hydrate_grafeo` using `graph_store().node_ids()`
                //     + `entity.reconcile(RootReconciler::new(node_map))`).
                return Err(GrafeoLoroError::NotYetImplemented(
                    "SsotMode::Grafeo hydrate: requires wal feature + ArcSwap grafeo_db field"
                        .into(),
                ));
            }
        }
    }

    /// Graceful shutdown: cancel workers, flush buffers, close stores.
    ///
    /// # Phase 5 Task 4 (P5-L1)
    ///
    /// Fully implemented in P5-L3 per architecture §4 Step D:
    ///
    /// 1. `self.sync_engine.shutdown()` — trigger the broadcast (already
    ///    implemented at `src/bridge/sync_engine.rs:611`).
    /// 2. `if let Some(handles) = self.worker_handles { for h in handles {
    ///    let _ = h.await; } }` — drain the inbound + outbound + CDC poller
    ///    tasks (CB-1 forward-compat — `worker_handles` populated by `build()`).
    /// 3. Final `checkpoint` to flush pending state (optional — Devil Q15:
    ///    should `shutdown` auto-checkpoint, or is that the caller's job?).
    /// 4. Flush telemetry exporters (P5-L2 territory — needs
    ///    `opentelemetry::global::shutdown_tracer_provider()` if a tracer was
    ///    configured).
    /// 5. Close `GrafeoDB` (currently no-op — `GrafeoDB::close` is wal-gated
    ///    per P4-DEVIL M3; deferred to Phase 5 wal feature work).
    #[instrument(skip(self), level = "info")]
    pub async fn shutdown(self) -> Result<()> {
        // P5-L3: 5-step graceful shutdown per architecture §4 Step D +
        // Devil Q15 (no auto-checkpoint) + Devil M3 (flush telemetry).
        //
        // Step 1: signal all worker loops to drain + exit via the broadcast.
        // `SyncEngine::shutdown` is non-async (just sends on the broadcast
        // channel) — safe to call from this async context without `.await`.
        self.sync_engine.shutdown();

        // Step 2: drain `worker_handles` — await each `JoinHandle<()>` so
        // the workers have fully exited (drained their buffers + dropped
        // their spans) before we flush telemetry. `None` if the app was
        // constructed via `from_sync_engine*` (test path) — skip silently.
        // Errors are logged + discarded: a worker that panicked during
        // shutdown should NOT abort the rest of the shutdown sequence
        // (anti-plenger #7 defensive programming — best-effort drain).
        if let Some(handles) = self.worker_handles {
            for (idx, handle) in handles.into_iter().enumerate() {
                if let Err(err) = handle.await {
                    tracing::warn!(
                        worker_idx = idx,
                        error = %err,
                        "shutdown: worker join failed (continuing)"
                    );
                }
            }
        }

        // Step 3: NO auto-checkpoint (Devil Q15 ruling — checkpoint is the
        // caller's responsibility; separation of concerns). The shutdown
        // sequence is purely about graceful worker drain + telemetry flush,
        // NOT durable state persistence. Callers that need a final checkpoint
        // must call `app.checkpoint(...)` BEFORE `app.shutdown().await`.

        // Step 4: flush telemetry exporters. `shutdown_tracer_provider()`
        // flushes any buffered spans to the configured exporter (no-op if
        // no SDK is installed — tests). Verified API:
        // `opentelemetry-0.23.0/src/global/trace.rs:421`. The meter provider
        // in OTel 0.23 does NOT have a public `shutdown_meter_provider` —
        // meters are flushed implicitly on drop of the SDK-owned provider.
        // We call `shutdown_tracer_provider` only (correct + sufficient).
        // Anti-plenger #6 (Performance & Security): graceful flush prevents
        // span loss on process exit.
        opentelemetry::global::shutdown_tracer_provider();

        // Step 5: GrafeoDB closes on drop — no explicit close needed (the
        // `Arc<GrafeoDB>` is released when `self` drops at function return;
        // if other Arc holders exist they keep the DB alive — correct
        // shared-ownership semantics). Devil Q15 confirmed: no auto-close
        // hook needed.

        Ok(())
    }
}

impl GrafeoLoroAppBuilder {
    /// Provide a storage backend implementation (filesystem, S3, IPFS, ...).
    ///
    /// # Phase 4 Task 4 scope (P4T4-L2)
    ///
    /// Stores the `Arc<dyn StorageBackend>` into the builder's `storage`
    /// slot. The same handle is later reachable from `build()`'s spawned
    /// `GrafeoLoroApp` so `hydrate` / `checkpoint` can call `load` / `save` /
    /// `list` / `delete` (architecture §24.3).
    ///
    /// # Contract (P4T4-L2 wires the body)
    ///
    /// - Consumes `self`, returns `Self` with `self.storage = Some(storage)`.
    /// - Idempotent over the slot: a second call overwrites the first (no
    ///   accumulation; anti-plenger #9).
    /// - No validation here — `build()` rejects a missing `storage` with
    ///   `GrafeoLoroError::Config("storage backend not set")`.
    pub fn storage(mut self, storage: Arc<dyn StorageBackend>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Select Loro or Grafeo as the source of truth.
    ///
    /// # Phase 4 Task 4 scope (P4T4-L2)
    ///
    /// Stores the `SsotMode` into the builder's `ssot_mode` slot. The
    /// selected mode dispatches `hydrate` / `checkpoint` (architecture §2 —
    /// `SsotMode::Loro` stores `.loro`; `SsotMode::Grafeo` stores
    /// `backup.tar.zst`). Defaults to `SsotMode::Loro` per `SsotMode::Default`
    /// at `src/config.rs:3`.
    ///
    /// # Contract (P4T4-L2 wires the body)
    ///
    /// - Consumes `self`, returns `Self` with `self.ssot_mode = mode`.
    /// - Idempotent over the slot.
    pub fn ssot_mode(mut self, mode: SsotMode) -> Self {
        self.ssot_mode = mode;
        self
    }

    /// Compression strategy for cold snapshots.
    ///
    /// # Phase 4 Task 4 scope (P4T4-L2)
    ///
    /// Stores the `CompressionType` into the builder's `compression` slot.
    /// Used by `checkpoint` to wrap the base snapshot / tarball via
    /// `CompressedPayload::compress` (verified at
    /// `src/compression/wrapper.rs:23`), and by `hydrate` to decompress via
    /// `CompressedPayload::decompress` (verified at
    /// `src/compression/wrapper.rs:48`). Defaults to `CompressionType::Zstd`
    /// per architecture §24.4 line 1297.
    ///
    /// # Contract (P4T4-L2 wires the body)
    ///
    /// - Consumes `self`, returns `Self` with `self.compression = comp`.
    /// - Idempotent over the slot.
    pub fn compression(mut self, comp: CompressionType) -> Self {
        self.compression = comp;
        self
    }

    /// Compression strategy for hot sync packets.
    ///
    /// # Phase 4 Task 4 scope (P4T4-L2)
    ///
    /// Stores the `CompressionType` into the builder's `sync_compression`
    /// slot. Reserved for the Loro wire-protocol path
    /// (`LoroDoc::export_compressed(ExportMode::updates(&vv), sync_compression)`
    /// — `LoroDocCompressionExt` at `src/compression/wrapper.rs:74`) used by
    /// the delta-storage arm of `checkpoint` / `hydrate`. Defaults to
    /// `CompressionType::Lz4` per architecture §24.4 line 1298.
    ///
    /// # Contract (P4T4-L2 wires the body)
    ///
    /// - Consumes `self`, returns `Self` with `self.sync_compression = comp`.
    /// - Idempotent over the slot.
    pub fn sync_compression(mut self, comp: CompressionType) -> Self {
        self.sync_compression = comp;
        self
    }

    /// Batcher flush interval in milliseconds.
    ///
    /// # Phase 4 Task 4 scope (P4T4-L2)
    ///
    /// Stores the `u64` into the builder's `batch_interval_ms` slot. Flows
    /// into `MutationBatcher::new` (called from `SyncEngine::new` at
    /// `src/bridge/sync_engine.rs:161`) — currently the batcher hard-codes
    /// `DEFAULT_BATCH_MS`; `// TODO(P4-L2): thread the builder's value
    /// through `SyncEngine::new` (signature change at `src/bridge/sync_engine.rs:148`).
    /// Flagged for P4-DEVIL Q7 — the `MutationBatcher::new` API takes the
    /// batch params positionally; widening it is a cross-module wiring
    /// concern.` Defaults to `DEFAULT_BATCH_MS` (100) per `src/constants.rs:22`.
    ///
    /// # Contract (P4T4-L2 wires the body)
    ///
    /// - Consumes `self`, returns `Self` with `self.batch_interval_ms = ms`.
    /// - Idempotent over the slot.
    pub fn batch_interval_ms(mut self, ms: u64) -> Self {
        self.batch_interval_ms = ms;
        self
    }

    /// Batcher max ops per flush.
    ///
    /// # Phase 4 Task 4 scope (P4T4-L2)
    ///
    /// Stores the `usize` into the builder's `batch_max_size` slot. Flows
    /// into `MutationBatcher::new` (same caveat as `batch_interval_ms` —
    /// `// TODO(P4-L2)` cross-module wiring). Defaults to `DEFAULT_BATCH_SIZE`
    /// (256) per `src/constants.rs:23`.
    ///
    /// # Contract (P4T4-L2 wires the body)
    ///
    /// - Consumes `self`, returns `Self` with `self.batch_max_size = size`.
    /// - Idempotent over the slot.
    pub fn batch_max_size(mut self, size: usize) -> Self {
        self.batch_max_size = size;
        self
    }

    /// Set the on-disk directory for `GrafeoDB` (P4-DEVIL Q5).
    ///
    /// `None` (default) → `GrafeoDB::new_in_memory()` (works for
    /// `SsotMode::Loro` + tests). `Some(p)` →
    /// `GrafeoDB::with_config(Config::persistent(p))` (NOT `GrafeoDB::open`
    /// — that is `#[cfg(feature = "wal")]`-gated per P4-DEVIL B1).
    /// `build()` rejects `SsotMode::Grafeo + None` with
    /// `Config("grafeo_dir required for SsotMode::Grafeo")`.
    ///
    /// Uses `impl Into<PathBuf>` so callers can pass `&str` / `&Path` /
    /// `PathBuf` ergonomically (P4-DEVIL Q5 L3 hint).
    pub fn grafeo_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.grafeo_dir = Some(path.into());
        self
    }

    /// Provide a pre-built metrics registry (P5-L1 Task 4).
    ///
    /// # Phase 5 Task 4 wiring (P5-L1)
    ///
    /// Stores the `Arc<MetricsRegistry>` into the builder's `metrics` slot.
    /// `build()` threads `Some(Arc::clone(&metrics))` into
    /// `SyncEngine::with_telemetry` + `GrafeoLoroApp::metrics`.
    ///
    /// # Contract (P5-L2 wires the body — already trivial field assignment)
    ///
    /// - Consumes `self`, returns `Self` with `self.metrics = Some(metrics)`.
    /// - Idempotent over the slot.
    /// - Caller responsibility: construct `MetricsRegistry::init(meter)` from
    ///   `opentelemetry::global::meter("grafeo-loro")` BEFORE calling this
    ///   setter. Devil Q14 — should `build()` auto-construct the registry if
    ///   this slot is `None`? L1 leaves the decision open.
    pub fn with_metrics(mut self, metrics: Arc<MetricsRegistry>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Provide a pre-built health probe (P5-L1 Task 3).
    ///
    /// # Phase 5 Task 3 wiring (P5-L1)
    ///
    /// Stores the `Arc<HealthProbe>` into the builder's `health` slot.
    /// `build()` threads `Some(Arc::clone(&health))` into
    /// `GrafeoLoroApp::health`.
    ///
    /// # Contract
    ///
    /// - Consumes `self`, returns `Self` with `self.health = Some(health)`.
    /// - Idempotent over the slot.
    /// - Caller responsibility: construct `HealthProbe::new(doc_clone,
    ///   db_clone)` from clones of the `Arc<RwLock<LoroDoc>>` + `Arc<GrafeoDB>`
    ///   that `build()` will create. This is awkward — caller must construct
    ///   the doc + db BEFORE the builder. Devil Q16 — should `build()`
    ///   construct `HealthProbe` internally after creating `loro_doc` +
    ///   `grafeo_db`, taking only `last_sync_ts` initial value as a param?
    pub fn with_health(mut self, health: Arc<HealthProbe>) -> Self {
        self.health = Some(health);
        self
    }

    /// Provide a pre-built shared tracer (P5-L1 Task 4).
    ///
    /// # Phase 5 Task 4 wiring (P5-L1)
    ///
    /// Stores the `SharedTracer` (alias for `Arc<BoxedTracer>`) into the
    /// builder's `tracer` slot. `build()` threads `Some(Arc::clone(&tracer))`
    /// into `SyncEngine::with_telemetry` + `GrafeoLoroApp::tracer`.
    ///
    /// # Contract
    ///
    /// - Consumes `self`, returns `Self` with `self.tracer = Some(tracer)`.
    /// - Idempotent over the slot.
    /// - Caller responsibility: construct
    ///   `Arc::new(opentelemetry::global::tracer("grafeo-loro"))` BEFORE
    ///   calling this setter. Devil Q17 — should `build()` auto-construct
    ///   the tracer from `global::tracer(...)` if this slot is `None`?
    pub fn with_tracer(mut self, tracer: SharedTracer) -> Self {
        self.tracer = Some(tracer);
        self
    }

    /// Validate config and spawn the runtime.
    ///
    /// # Phase 4 Task 4 scope (P4T4-L2)
    ///
    /// 1. **Validate config** (P4-DEVIL Q5/Q8) — reject:
    ///    - `storage == None` → `Config("storage backend not set")`.
    ///    - `batch_interval_ms == 0` → `Config("batch_interval_ms must be > 0")`
    ///      (`Duration::from_millis(0)` would degenerate the batcher ticker
    ///      — P4-DEVIL Q8 anti-plenger #14 "never simplify the basics").
    ///    - `batch_max_size == 0` → `Config("batch_max_size must be > 0")`
    ///      (`if b.len() < 0` is always false → degenerate no-batching).
    ///    - `SsotMode::Grafeo` + `grafeo_dir == None` →
    ///      `Config("grafeo_dir required for SsotMode::Grafeo")` (P4-DEVIL Q5).
    /// 2. **Init `GrafeoDB`** (P4-DEVIL Q5) — dispatch on `grafeo_dir`:
    ///    - `Some(p)` → `GrafeoDB::with_config(Config::persistent(p))`
    ///      (NOT `GrafeoDB::open` — that is `#[cfg(feature = "wal")]`-gated
    ///      per P4-DEVIL B1; `with_config` is unconditionally compiled at
    ///      `grafeo-engine-0.5.42/src/database/mod.rs:346`).
    ///    - `None` → `GrafeoDB::new_in_memory()`
    ///      (`grafeo-engine-0.5.42/src/database/mod.rs:267`).
    /// 3. **Init `LoroDoc`** — `LoroDoc::new()` (`loro-1.13.6/src/lib.rs:137`)
    ///    wrapped in `Arc<RwLock<LoroDoc>>` per `SyncEngine::new`'s signature.
    /// 4. **Init `SyncEngine`** (P4-DEVIL Q7 + P5-L1 Task 4) —
    ///    `SyncEngine::with_telemetry(grafeo_db, loro_doc, batch_max_size,
    ///    batch_interval_ms, metrics, tracer)` (added P5-L1; replaces the
    ///    prior `with_batch_config` call) returns the engine + the two
    ///    channel receivers. The `MutationBatcher` is owned by
    ///    `SyncEngine::new_inner` (no separate init step). `metrics` +
    ///    `tracer` are `Option` so test builds without telemetry configured
    ///    pass `None`.
    /// 5. **Spawn tokio tasks** — `Arc::new(engine).clone().spawn_all(
    ///    inbound_rx, outbound_rx).await` — spawns the Loro subscriber
    ///    (`init_loro_subscriber` is called inside `spawn_all`) + inbound
    ///    worker + outbound worker + CDC poller. Returns the three
    ///    `JoinHandle`s; P5-L1 CB-1 forward-compat preserves them in
    ///    `worker_handles` for `GrafeoLoroApp::shutdown` to drain.
    /// 6. **Wrap into `GrafeoLoroApp`** —
    ///    `GrafeoLoroApp::from_sync_engine_with_telemetry(Arc::new(engine),
    ///    ssot_mode, Some(storage), compression, metrics, health, tracer,
    ///    Some(worker_handles))` (P4-DEVIL M8 + P5-L1 Task 4).
    ///
    /// # Concurrency (P4-DEVIL M10)
    ///
    /// `build()` activates the Loro subscriber inside `spawn_all` (step 5).
    /// `hydrate()` called AFTER `build()` therefore runs with the subscriber
    /// active. This is safe because `hydrate`'s `LoroDoc::import_with` uses
    /// `ORIGIN_LORO_BRIDGE` (P4-DEVIL M10) which the B1 filter at
    /// `src/bridge/sync_engine.rs:234` skips — no echo. `parallel_hydrate_grafeo`
    /// writes to Grafeo (not Loro) and uses `session_with_cdc(false)` — no
    /// outbound echo. The subscriber active window is therefore safe.
    ///
    /// # Errors
    ///
    /// - `GrafeoLoroError::Config` for the four validation failures above.
    /// - `GrafeoLoroError::Grafeo` if `GrafeoDB::with_config(...)` fails.
    ///
    /// # Idempotency
    ///
    /// `build()` consumes `self` — calling it twice on the same builder is a
    /// compile-time error (move). The returned `GrafeoLoroApp` owns the
    /// `Arc<SyncEngine>` exclusively; orchestrator may `Arc::clone` for child
    /// tasks but cannot `build()` twice.
    #[instrument(skip(self), level = "info")]
    pub async fn build(self) -> Result<GrafeoLoroApp> {
        // 1. Validate config (P4-DEVIL Q5/Q8).
        if self.batch_interval_ms == 0 {
            return Err(GrafeoLoroError::Config(
                "batch_interval_ms must be > 0".into(),
            ));
        }
        if self.batch_max_size == 0 {
            return Err(GrafeoLoroError::Config("batch_max_size must be > 0".into()));
        }
        let storage = self
            .storage
            .ok_or_else(|| GrafeoLoroError::Config("storage backend not set".into()))?;
        if matches!(self.ssot_mode, SsotMode::Grafeo) && self.grafeo_dir.is_none() {
            return Err(GrafeoLoroError::Config(
                "grafeo_dir required for SsotMode::Grafeo".into(),
            ));
        }

        // 2. Init GrafeoDB (P4-DEVIL Q5 — NOT `GrafeoDB::open` (wal-gated)).
        let grafeo_db: Arc<grafeo::GrafeoDB> = match self.grafeo_dir {
            Some(p) => Arc::new(grafeo::GrafeoDB::with_config(grafeo::Config::persistent(
                p,
            ))?),
            None => Arc::new(grafeo::GrafeoDB::new_in_memory()),
        };

        // 3. Init LoroDoc.
        let loro_doc = Arc::new(parking_lot::RwLock::new(loro::LoroDoc::new()));

        // 4. Auto-construct telemetry handles if their builder slots are
        //    `None` (Devil m2 — Q14/Q16/Q17 rulings). `opentelemetry::global`
        //    is verified to expose `meter(name)` + `tracer(name)` (Devil
        //    step 3). P5-L3 fills the bodies: production auto-construction
        //    fires whenever the builder slots are unset; tests that do not
        //    configure telemetry (e.g. `build_accepts_valid_loro_config`)
        //    get real no-op instruments from `global::meter` / `global::tracer`
        //    (without an SDK installed, these are no-op — no behavior change
        //    vs the prior `None` returns).
        let metrics = self.metrics.clone().or_else(|| {
            // Verified API (Devil step 3): opentelemetry-0.23.0/src/global/metrics.rs:115
            Some(Arc::new(MetricsRegistry::init(
                opentelemetry::global::meter("grafeo-loro"),
            )))
        });
        let tracer = self.tracer.clone().or_else(|| {
            // Verified API (Devil step 3): opentelemetry-0.23.0/src/global/trace.rs:394
            Some(Arc::new(opentelemetry::global::tracer("grafeo-loro")))
        });
        let health = self.health.clone().or_else(|| {
            // HealthProbe auto-constructed from the just-created loro_doc +
            // grafeo_db (Devil Q16 ruling — both handles exist at this point
            // in `build()`). `HealthProbe::new` initializes `last_sync_ts`
            // to current wall-clock ms (P5-L3) so a freshly-constructed probe
            // does NOT immediately fail the staleness check.
            Some(Arc::new(HealthProbe::new(
                loro_doc.clone(),
                grafeo_db.clone(),
            )))
        });

        // 5. Init SyncEngine (P4-DEVIL Q7 + P5-L1 Task 4 + P5-L2 Devil M3 —
        //    `with_telemetry` threads builder batch params + metrics + tracer
        //    + health into the MutationBatcher).
        let (engine, inbound_rx, outbound_rx) = SyncEngine::with_telemetry(
            grafeo_db,
            loro_doc,
            self.batch_max_size,
            self.batch_interval_ms,
            metrics.clone(),
            tracer.clone(),
            health.clone(),
        );
        let engine = Arc::new(engine);

        // 6. Spawn tokio tasks (init_loro_subscriber is called inside
        //    spawn_all — subscriber is active when build() returns; hydrate()
        //    handles this via ORIGIN_LORO_BRIDGE per P4-DEVIL M10).
        //
        // P5-L1 CB-1 forward-compat: preserve the returned `Vec<JoinHandle<()>>`
        // in `worker_handles` so `GrafeoLoroApp::shutdown` can drain workers
        // in L2/L3 (P4-HUNT CB-1 — previously discarded as `_join_handles`).
        let worker_handles = engine.clone().spawn_all(inbound_rx, outbound_rx).await;

        tracing::info!(
            ssot_mode = ?self.ssot_mode,
            compression = ?self.compression,
            metrics_configured = metrics.is_some(),
            health_configured = health.is_some(),
            tracer_configured = tracer.is_some(),
            "GrafeoLoroAppBuilder::build: runtime spawned"
        );

        // 7. Wrap into GrafeoLoroApp (P4-DEVIL M8 + P5-L1 Task 4 —
        //    from_sync_engine_with_telemetry threads ssot_mode + storage +
        //    compression + metrics + health + tracer + worker_handles into
        //    the app struct).
        Ok(GrafeoLoroApp::from_sync_engine_with_telemetry(
            engine,
            AppTelemetryConfig {
                ssot_mode: self.ssot_mode,
                storage: Some(storage),
                compression: self.compression,
                metrics,
                health,
                tracer,
                worker_handles: Some(worker_handles),
            },
        ))
    }
}
