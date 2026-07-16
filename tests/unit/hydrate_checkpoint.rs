//! Phase 4 Task (P4-L3) tests: `GrafeoLoroApp::hydrate`/`checkpoint` cold-boot
//! round-trip.
//!
//! Validates the `SsotMode::Loro` cold-boot round-trip end-to-end:
//! 1. Build a `GrafeoLoroApp` via `from_sync_engine_with_config` (NOT `build()` —
//!    `build()` requires `tokio::runtime`; the unit tests use a fresh
//!    in-memory `SyncEngine` directly — anti-plenger #11 native-first, no
//!    mockall).
//! 2. Install the Loro subscriber (so `inbound_event_count` /
//!    `inbound_filtered_count` are observable — P2T3-L2R2 MAJOR 2).
//! 3. Write a vertex to Loro using the native `LoroDoc` handle (raw API
//!    philosophy — no wrapper). The write is tagged with `ORIGIN_LORO_BRIDGE`
//!    so the inbound B1 filter skips the echo.
//! 4. `checkpoint("test-graph")` → verify storage has `test-graph/base.loro`
//!    with wire-format bytes (2-byte header + payload).
//! 5. Build a fresh app (cold boot) over the SAME storage backend.
//! 6. `hydrate("test-graph")` → verify the vertex is observable in the new
//!    LoroDoc + BridgeMaps (parallel_hydrate_grafeo materializes Grafeo).
//! 7. Assert no echo loops — `inbound_event_count` MUST NOT increment during
//!    `hydrate` (P1 lesson — the `ORIGIN_LORO_BRIDGE` import tag is filtered
//!    by the B1 filter at `src/bridge/sync_engine.rs:270`).
//!
//! Anti-Goodhart: the test round-trips REAL bytes through REAL compression
//! (Zstd) + REAL Loro `export(ExportMode::shallow_snapshot)` + REAL
//! `LoroDoc::import_with`. No mocks. If the production code regresses, the
//! test catches it (not the other way around).

#![allow(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use grafeo::GrafeoDB;
use loro::{Container, LoroDoc, ValueOrContainer};
use lorosurgeon::reconcile::RootReconciler;
use lorosurgeon::{Hydrate, Reconcile};
use parking_lot::RwLock;

use grafeo_loro::compression::CompressedPayload;
use grafeo_loro::config::{CompressionType, SsotMode};
use grafeo_loro::constants::{ORIGIN_LORO_BRIDGE, ROOT_VERTICES, STORAGE_KEY_BASE_LORO};
use grafeo_loro::schema::VertexEntity;
use grafeo_loro::storage::StorageBackend;
use grafeo_loro::types::LoroProperty;
use grafeo_loro::{GrafeoLoroApp, SyncEngine};

/// In-memory `StorageBackend` for cold-boot round-trip tests.
///
/// `Mutex<HashMap<String, Vec<u8>>>` is the simplest possible backend — no
/// mockall, no third-party crate (anti-plenger #5 Bloat + #11 native-first).
/// `list` returns sorted keys so `hydrate`'s delta-sort is deterministic.
struct InMemoryStorage {
    inner: Mutex<HashMap<String, Vec<u8>>>,
}

impl InMemoryStorage {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Snapshot accessor for test assertions. Returns all keys sorted.
    fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .inner
            .lock()
            .expect("storage mutex poisoned")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }
}

#[async_trait(?Send)]
impl StorageBackend for InMemoryStorage {
    async fn load(&self, key: &str) -> std::io::Result<Vec<u8>> {
        self.inner
            .lock()
            .expect("storage mutex poisoned")
            .get(key)
            .cloned()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("key not found: {key}"),
                )
            })
    }

    async fn save(&self, key: &str, bytes: Vec<u8>) -> std::io::Result<()> {
        self.inner
            .lock()
            .expect("storage mutex poisoned")
            .insert(key.to_string(), bytes);
        Ok(())
    }

    async fn list(&self, prefix: &str) -> std::io::Result<Vec<String>> {
        let mut keys: Vec<String> = self
            .inner
            .lock()
            .expect("storage mutex poisoned")
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort();
        Ok(keys)
    }

    async fn delete(&self, key: &str) -> std::io::Result<()> {
        self.inner
            .lock()
            .expect("storage mutex poisoned")
            .remove(key);
        Ok(())
    }

    // Issue #3 sub-issue 9 — stub impls for the storage trait extensions.
    // The hydrate-checkpoint tests do not exercise incremental snapshots,
    // streaming OPFS writes, or snapshot diffing; empty/Ok is sufficient.
    async fn export_incremental_snapshot(
        &self,
        _since: &[u8],
    ) -> grafeo_loro::error::Result<Vec<u8>> {
        Ok(Vec::new())
    }
    async fn stream_snapshot_to_opfs(
        &self,
        _cb: &(dyn for<'a> Fn(&'a [u8]) -> grafeo_loro::error::Result<()> + Send + Sync),
    ) -> grafeo_loro::error::Result<()> {
        Ok(())
    }
    async fn diff_snapshots(
        &self,
        _base: &[u8],
        _head: &[u8],
    ) -> grafeo_loro::error::Result<grafeo_loro::storage::SnapshotDiff> {
        Ok(grafeo_loro::storage::SnapshotDiff::empty())
    }
}

/// Build a fresh `GrafeoLoroApp` over an in-memory `GrafeoDB` + `LoroDoc`,
/// wired to `storage` with `SsotMode::Loro` + `CompressionType::Zstd`.
fn build_app_with_storage(
    storage: Arc<dyn StorageBackend>,
) -> (GrafeoLoroApp, Arc<GrafeoDB>, Arc<RwLock<LoroDoc>>) {
    let db = Arc::new(GrafeoDB::new_in_memory());
    let doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, _inbound_rx, _outbound_rx) = SyncEngine::new(db.clone(), doc.clone());
    let app = GrafeoLoroApp::from_sync_engine_with_config(
        Arc::new(engine),
        SsotMode::Loro,
        Some(storage),
        CompressionType::Zstd,
    );
    (app, db, doc)
}

/// Write a vertex directly to Loro using native APIs (raw-handle philosophy).
///
/// Uses `lorosurgeon::Reconcile` to materialize the `VertexEntity` schema into
/// the Loro doc under the `V/<key>` map. The commit is tagged with
/// `ORIGIN_LORO_BRIDGE` so the inbound B1 filter skips the echo — same pattern
/// the bridge uses internally for RYOW writes.
fn write_vertex_to_loro(
    app: &GrafeoLoroApp,
    loro_key: &str,
    labels: Vec<String>,
    properties: HashMap<String, LoroProperty>,
) {
    let entity = VertexEntity {
        labels,
        properties,
        description: String::new(),
    };
    let doc = app.loro_doc().write();
    doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE);
    let v_map = doc.get_map(ROOT_VERTICES);
    let node_map = v_map
        .ensure_mergeable_map(loro_key)
        .expect("ensure_mergeable_map for vertex");
    entity
        .reconcile(RootReconciler::new(node_map))
        .expect("reconcile vertex entity");
    doc.commit();
}

/// Full cold-boot round-trip: write vertex to Loro → checkpoint → cold boot →
/// hydrate → verify vertex recovered + no echo.
#[tokio::test]
async fn cold_boot_roundtrip_loro_mode() {
    // Hold the concrete `Arc<InMemoryStorage>` so the test can call
    // `keys()` for assertions; pass a clone (as `Arc<dyn StorageBackend>`)
    // into the app.
    let storage: Arc<InMemoryStorage> = Arc::new(InMemoryStorage::new());

    // --- Phase 1: build app, install subscriber, write vertex to Loro, checkpoint. ---
    let (app, _db, _doc) = build_app_with_storage(storage.clone());
    app.sync_engine()
        .init_loro_subscriber()
        .expect("subscriber installed");

    let loro_key = "V/0".to_string();
    let mut props = HashMap::new();
    props.insert("name".to_string(), LoroProperty::String("Alix".into()));
    write_vertex_to_loro(&app, &loro_key, vec!["Person".into()], props);

    // The checkpoint task writes the base snapshot through compress_to_wire.
    app.checkpoint("test-graph")
        .await
        .expect("checkpoint succeeds");

    // Verify storage has the expected key with wire-format bytes.
    let expected_key = format!("test-graph/{STORAGE_KEY_BASE_LORO}");
    let stored_keys = storage.keys();
    assert!(
        stored_keys.contains(&expected_key),
        "storage should contain {expected_key} after checkpoint; got {stored_keys:?}"
    );
    let stored_bytes = storage
        .load(&expected_key)
        .await
        .expect("load base snapshot");
    // Wire format = at least 2-byte header + body. Zstd produces a non-empty
    // frame for any input, so stored_bytes.len() > 2 for a non-empty doc.
    assert!(
        stored_bytes.len() >= 2,
        "wire-format payload must be at least 2 bytes (version + codec tag); got {}",
        stored_bytes.len()
    );
    // Verify the wire format actually decompresses back to Loro bytes.
    let _loro_bytes = CompressedPayload::decompress_from_wire(&stored_bytes)
        .expect("stored wire bytes decompress back to valid Loro bytes");

    // --- Phase 2: fresh app over the same storage (cold boot). ---
    let (app2, _db2, doc2) = build_app_with_storage(storage.clone());
    app2.sync_engine()
        .init_loro_subscriber()
        .expect("subscriber installed on fresh app");

    let event_count_before = app2.sync_engine().inbound_event_count();
    let filtered_count_before = app2.sync_engine().inbound_filtered_count();

    app2.hydrate("test-graph").await.expect("hydrate succeeds");

    // --- Phase 3: assertions on the hydrated state. ---

    // (a) The vertex is observable in the new LoroDoc.
    {
        let doc_guard = doc2.read();
        let v_map = doc_guard.get_map(ROOT_VERTICES);
        let vertex = v_map
            .get(&loro_key)
            .unwrap_or_else(|| panic!("hydrated LoroDoc should contain V/{loro_key:?}"));
        let node_map = match vertex {
            ValueOrContainer::Container(Container::Map(m)) => m,
            _ => panic!("V/{loro_key:?} should be a Map container, got {vertex:?}"),
        };
        let hydrated =
            VertexEntity::hydrate_map(&node_map).expect("hydrate_map succeeds on recovered vertex");
        assert!(
            hydrated.labels.iter().any(|l| l == "Person"),
            "recovered vertex should have label 'Person'; got {:?}",
            hydrated.labels
        );
        match hydrated.properties.get("name") {
            Some(grafeo_loro::types::LoroProperty::String(s)) if s == "Alix" => {}
            other => panic!("recovered vertex 'name' property mismatch; got {other:?}"),
        }
    }

    // (b) parallel_hydrate_grafeo ran during hydrate → BridgeMaps has the binding.
    // (The recovered vertex was materialized into Grafeo; `parallel_hydrate_grafeo`
    // inserts the binding directly via `BridgeMaps::insert_node`.)
    assert!(
        !app2.maps().node_id_map.read().is_empty(),
        "BridgeMaps::node_id_map should be non-empty after hydrate"
    );

    // (c) No echo loop: the import was tagged with ORIGIN_LORO_BRIDGE so the
    // B1 filter at sync_engine.rs:270 skipped it. inbound_event_count MUST
    // be unchanged; inbound_filtered_count MUST have incremented (proves the
    // filter actually fired — P2T3-L2R2 MAJOR 2).
    let event_count_after = app2.sync_engine().inbound_event_count();
    let filtered_count_after = app2.sync_engine().inbound_filtered_count();
    assert_eq!(
        event_count_after, event_count_before,
        "no echo should reach the inbound channel during hydrate; \
         event_count_before={event_count_before}, event_count_after={event_count_after}"
    );
    assert!(
        filtered_count_after > filtered_count_before,
        "B1 filter MUST fire on hydrate's ORIGIN_LORO_BRIDGE-tagged import; \
         filtered_count_before={filtered_count_before}, \
         filtered_count_after={filtered_count_after}"
    );
}

/// Cold-boot on an empty storage key (fresh graph) → `hydrate` succeeds,
/// initializes an empty `LoroDoc`, no panic.
#[tokio::test]
async fn cold_boot_fresh_graph_no_snapshot() {
    let storage: Arc<InMemoryStorage> = Arc::new(InMemoryStorage::new());

    let (app, _db, _doc) = build_app_with_storage(storage.clone());
    // No checkpoint — storage is empty.

    app.hydrate("never-checkpointed-graph")
        .await
        .expect("hydrate on fresh graph succeeds");

    // Fresh-graph path: no V/* keys to recover — BridgeMaps stays empty.
    assert!(
        app.maps().node_id_map.read().is_empty(),
        "BridgeMaps should be empty after fresh-graph hydrate"
    );
}

/// Double-checkpoint idempotency: calling `checkpoint` twice in succession
/// overwrites the base snapshot and clears no deltas (Phase 4 has no
/// delta-write path — `list(delta-)` returns empty, `delete` loop runs zero
/// times). The second call MUST NOT error.
#[tokio::test]
async fn checkpoint_idempotent_double_call() {
    let storage: Arc<InMemoryStorage> = Arc::new(InMemoryStorage::new());
    let (app, _db, _doc) = build_app_with_storage(storage.clone());
    app.sync_engine()
        .init_loro_subscriber()
        .expect("subscriber installed");

    // Write a vertex to Loro (raw handle) so the snapshot is non-empty.
    let mut props = HashMap::new();
    props.insert("name".to_string(), LoroProperty::String("Bob".into()));
    write_vertex_to_loro(&app, "V/0", vec!["Person".into()], props);

    app.checkpoint("g").await.expect("first checkpoint");
    app.checkpoint("g")
        .await
        .expect("second checkpoint (idempotent)");

    // Storage has exactly one base key (no duplicates).
    let keys = storage.keys();
    let base_count = keys
        .iter()
        .filter(|k| k.ends_with(STORAGE_KEY_BASE_LORO))
        .count();
    assert_eq!(
        base_count, 1,
        "exactly one base.loro key after double checkpoint"
    );
}
