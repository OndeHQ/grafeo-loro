//! Phase 4 Task (P4-L3) tests: `GrafeoLoroAppBuilder::build` config validation.
//!
//! Validates the four P4-DEVIL Q5/Q8 rejection paths in
//! `GrafeoLoroAppBuilder::build` (`src/app.rs:873-891`):
//!
//! - `batch_interval_ms == 0` → `Config("batch_interval_ms must be > 0")`
//!   (Q8 — `Duration::from_millis(0)` would degenerate the batcher ticker).
//! - `batch_max_size == 0` → `Config("batch_max_size must be > 0")`
//!   (Q8 — `if b.len() < 0` is always false → degenerate no-batching).
//! - `storage == None` → `Config("storage backend not set")` (M8 — defensive;
//!   `hydrate`/`checkpoint` also reject `None` at dispatch time).
//! - `SsotMode::Grafeo + grafeo_dir == None` →
//!   `Config("grafeo_dir required for SsotMode::Grafeo")` (Q5).
//!
//! Each rejection path is exercised in isolation — the other fields are kept
//! at their `Default` values so the rejection is unambiguous (anti-Goodhart —
//! no test sets two invalid fields and asserts the first rejection).
//!
//! Anti-hallucination: tests use a real `InMemoryStorage` (the same impl as
//! `hydrate_checkpoint.rs`) so the `storage.is_none()` check is the only
//! field under test. `build()` is async — uses `#[tokio::test]`.
//!
//! `GrafeoLoroApp` does NOT implement `Debug` (its `Arc<SyncEngine>` field
//! owns non-`Debug` types), so the rejection tests use `match` instead of
//! `expect_err` (which would require `Self: Debug` on the Ok type).

#![allow(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;

use grafeo_loro::config::{CompressionType, SsotMode};
use grafeo_loro::error::GrafeoLoroError;
use grafeo_loro::storage::StorageBackend;
use grafeo_loro::GrafeoLoroApp;

/// Minimal in-memory `StorageBackend` for builder-validation tests.
/// (Mirrors `hydrate_checkpoint::InMemoryStorage` — duplicated here to keep
/// each test module self-contained per the existing test-suite convention.)
struct InMemoryStorage {
    inner: Mutex<HashMap<String, Vec<u8>>>,
}

impl InMemoryStorage {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
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
    // The builder-validation tests do not exercise incremental snapshots,
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

/// Assert `result` is `Err(Config(msg))` where `msg` contains `needle`.
fn assert_config_err<T>(result: Result<T, GrafeoLoroError>, needle: &str) {
    match result {
        Err(GrafeoLoroError::Config(msg)) => {
            assert!(
                msg.contains(needle),
                "Config error message {msg:?} does not contain {needle:?}"
            );
        }
        Err(other) => panic!("expected Config({needle:?}), got other error: {other:?}"),
        Ok(_) => panic!("expected Config({needle:?}), got Ok"),
    }
}

/// `batch_interval_ms == 0` → `Err(Config("batch_interval_ms must be > 0"))`.
#[tokio::test]
async fn build_rejects_zero_batch_interval_ms() {
    let storage: Arc<dyn StorageBackend> = Arc::new(InMemoryStorage::new());
    let result = GrafeoLoroApp::builder()
        .storage(storage)
        .batch_interval_ms(0)
        .build()
        .await;
    assert_config_err(result, "batch_interval_ms");
}

/// `batch_max_size == 0` → `Err(Config("batch_max_size must be > 0"))`.
#[tokio::test]
async fn build_rejects_zero_batch_max_size() {
    let storage: Arc<dyn StorageBackend> = Arc::new(InMemoryStorage::new());
    let result = GrafeoLoroApp::builder()
        .storage(storage)
        .batch_max_size(0)
        .build()
        .await;
    assert_config_err(result, "batch_max_size");
}

/// `storage == None` (the `Default`) → `Err(Config("storage backend not set"))`.
/// No `.storage(...)` call — the slot stays at its `Default::default()` `None`.
#[tokio::test]
async fn build_rejects_missing_storage() {
    let result = GrafeoLoroApp::builder().build().await;
    assert_config_err(result, "storage backend not set");
}

/// `SsotMode::Grafeo + grafeo_dir == None` →
/// `Err(Config("grafeo_dir required for SsotMode::Grafeo"))`. B1 fix:
/// `GrafeoDB::open` is `#[cfg(feature = "wal")]`-gated; without `grafeo_dir`
/// the only valid mode is `SsotMode::Loro` (in-memory GrafeoDB).
#[tokio::test]
async fn build_rejects_grafeo_mode_without_grafeo_dir() {
    let storage: Arc<dyn StorageBackend> = Arc::new(InMemoryStorage::new());
    let result = GrafeoLoroApp::builder()
        .storage(storage)
        .ssot_mode(SsotMode::Grafeo)
        .build()
        .await;
    assert_config_err(result, "grafeo_dir required for SsotMode::Grafeo");
}

/// Positive control: a valid `SsotMode::Loro` config with all defaults (other
/// than `storage`) builds successfully. Anti-tautology — proves the rejection
/// tests above are not just rejecting EVERY config (would be a tautology if
/// `build` always errored).
#[tokio::test]
async fn build_accepts_valid_loro_config() {
    let storage: Arc<dyn StorageBackend> = Arc::new(InMemoryStorage::new());
    let app = GrafeoLoroApp::builder()
        .storage(storage)
        .ssot_mode(SsotMode::Loro)
        .compression(CompressionType::Zstd)
        .build()
        .await
        .expect("valid Loro config builds");
    // Sanity: the app exposes its dispatch fields via the public accessors
    // added in P4-L3 (`ssot_mode()` + `compression()`).
    assert_eq!(app.ssot_mode(), SsotMode::Loro);
    assert_eq!(app.compression(), CompressionType::Zstd);
    // The sync engine should be reachable.
    let _ = app.sync_engine();
}
