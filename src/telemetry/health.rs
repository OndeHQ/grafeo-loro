//! Health probe (Phase 5 Task 3).
//!
//! # L1 contract layer (P5-L1)
//!
//! All method bodies are `unimplemented!()`. The struct shape mirrors
//! architecture §23.3 exactly:
//!
//! ```text,ignore
//! pub struct HealthProbe {
//!     doc: Arc<RwLock<LoroDoc>>,
//!     db: Arc<GrafeoDB>,
//!     last_sync_ts: AtomicU64,
//! }
//! ```
//!
//! L2 wires `update_sync_ts` into the outbound worker (after each successful
//! Loro commit) and the inbound batcher (after each flush). L3 fills the
//! `check` body with the three probes specified in architecture §23.3:
//!
//! 1. **Loro doc read accessibility** — `self.doc.try_read().is_some()`. `parking_lot::RwLock`
//!    has NO poisoning (unlike `std::sync::RwLock`); `try_read()` returns `None` only when a
//!    writer currently holds the lock, so this probe verifies the lock is not held by a writer
//!    (P5-HUNT-1 MINOR 1 — comment previously mis-described this as poison detection).
//! 2. **Grafeo dummy query** — `self.db.session().execute("MATCH (n) RETURN count(n) LIMIT 1").is_ok()`
//!    (Devil M1 — correct API per grafeo-engine-0.5.42; `GrafeoDB::execute(&str)` does NOT
//!    exist; the actual API is `db.session() -> Session` + `Session::execute(&self, query:
//!    &str) -> Result<QueryResult>` verified at `grafeo-engine-0.5.42/src/database/mod.rs:1663`
//!    + `session/mod.rs:2636`).
//! 3. **Sync staleness** — `now - last_sync_ts < max_staleness_ms`.
//!
//! ## Storage convention
//!
//! `HealthProbe` is stored as `Arc<HealthProbe>` on `GrafeoLoroApp` (top-level
//! owner, always present in production). Tests can omit it via the
//! `Option<Arc<HealthProbe>>` field type — Devil Q2 to confirm.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use grafeo::GrafeoDB;
use loro::LoroDoc;
use parking_lot::RwLock;

/// Wall-clock milliseconds since UNIX epoch.
///
/// Returns 0 if the system clock is set before `UNIX_EPOCH` (defensive —
/// `duration_since` errors only on time going backwards; in practice this
/// is unreachable on commodity OSes, but the fallback keeps `check()`
/// non-panicking). Used to stamp `last_sync_ts` and to compute staleness
/// in [`HealthProbe::check`].
fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Health probe: checks Loro lock poison, Grafeo dummy query, sync staleness.
///
/// Per architecture §23.3: "Returns 200 OK if: LoroDoc is not poisoned
/// (can acquire read lock) AND GrafeoDB can execute a dummy query AND last
/// sync occurred within `max_staleness_ms`."
///
/// `update_sync_ts` is called by the bridge after every successful
/// inbound flush + outbound commit (L2 wiring). `check` is exposed via an
/// HTTP endpoint (out of scope — Phase 6 hardening).
pub struct HealthProbe {
    /// Loro consensus-layer handle. `try_read` detects poison.
    doc: Arc<RwLock<LoroDoc>>,
    /// Grafeo execution-layer handle. Dummy query detects storage failure.
    db: Arc<GrafeoDB>,
    /// Wall-clock ms of the last successful sync (inbound flush OR outbound
    /// commit). Loaded with `Ordering::Relaxed` per architecture §23.3 —
    /// staleness is a soft signal, not a synchronization primitive.
    last_sync_ts: AtomicU64,
}

/// Aggregate health status with per-component breakdown.
///
/// Returned by [`HealthProbe::check`]. The `overall` flag is the AND of all
/// `components` — a single failing component marks the app unhealthy.
/// `components` preserves order: `[("loro_doc", _), ("grafeo_db", _),
/// ("sync_freshness", _)]` (architecture §23.3).
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// Overall OK flag (AND of all components).
    pub overall: bool,
    /// Per-component `(name, ok)` pairs. Order matches architecture §23.3.
    pub components: Vec<(&'static str, bool)>,
}

impl HealthProbe {
    /// Construct with shared handles. `last_sync_ts` initializes to the
    /// current wall-clock ms so a freshly-constructed probe does NOT
    /// immediately fail the staleness check (L2 territory — needs
    /// `SystemTime::now().duration_since(UNIX_EPOCH)`).
    ///
    /// # L1 contract
    ///
    /// - Stores `doc` + `db` Arc clones.
    /// - Initializes `last_sync_ts` to `now_ms` (NOT 0 — a 0 init would
    ///   make `check` always fail staleness on first call).
    pub fn new(doc: Arc<RwLock<LoroDoc>>, db: Arc<GrafeoDB>) -> Self {
        // P5-L3: store Arc clones; init `last_sync_ts` to current wall-clock
        // ms so a freshly-constructed probe does NOT immediately fail the
        // staleness check (Devil L1 contract — `0` init would fail `check`
        // on first call since `now - 0` always exceeds `max_staleness_ms`).
        Self {
            doc,
            db,
            last_sync_ts: AtomicU64::new(unix_timestamp_ms()),
        }
    }

    /// Stamp `last_sync_ts` with the current wall-clock ms.
    ///
    /// Called by the bridge after every successful inbound flush + outbound
    /// commit (L2 wiring contact points: `MutationBatcher::flush_inner`
    /// post-commit + `SyncEngine::spawn_outbound_worker` post-Loro-commit).
    /// Uses `Ordering::Relaxed` — staleness is a soft signal.
    ///
    /// # L1 contract
    ///
    /// - `self.last_sync_ts.store(now_ms, Ordering::Relaxed)`.
    /// - `now_ms` from `SystemTime::now().duration_since(UNIX_EPOCH)`.
    pub fn update_sync_ts(&self) {
        // P5-L3: stamp current wall-clock ms. `Ordering::Relaxed` per
        // architecture §23.3 — staleness is a soft signal, not a sync
        // primitive; no accompanying memory payload needs stronger ordering.
        self.last_sync_ts
            .store(unix_timestamp_ms(), Ordering::Relaxed);
    }

    /// Probe all three components; returns [`HealthStatus`] with
    /// `overall=false` on any failure (poisoned lock, dummy query error,
    /// stale sync).
    ///
    /// # L1 contract
    ///
    /// Per architecture §23.3 (Devil M1 — correct Grafeo API):
    /// - `loro_ok = self.doc.try_read().is_some()`
    /// - `grafeo_ok = self.db.session().execute("MATCH (n) RETURN count(n) LIMIT 1").is_ok()`
    ///   (NOT `self.db.execute(...)` — `GrafeoDB::execute(&str)` does NOT exist in
    ///   grafeo 0.5.42; correct API is `db.session() -> Session` + `Session::execute(&self,
    ///   query: &str) -> Result<QueryResult>`, verified at
    ///   `grafeo-engine-0.5.42/src/database/mod.rs:1663` + `session/mod.rs:2636`.)
    /// - `sync_ok = now - self.last_sync_ts.load(Relaxed) < max_staleness_ms`
    /// - `overall = loro_ok && grafeo_ok && sync_ok`
    /// - `components = vec![("loro_doc", loro_ok), ("grafeo_db", grafeo_ok), ("sync_freshness", sync_ok)]`
    ///
    /// # Devil questions resolved
    ///
    /// - Q4 (Devil M1): API verified — `GrafeoDB::execute(&str)` does NOT exist;
    ///   correct API is `db.session().execute(query: &str) -> Result<QueryResult>`.
    ///   Architecture §23.3 line 1080 was patched in P5-L2 to use `db.session().execute(...)`.
    /// - Q5: Silent return on failure (no `WARN` log) per architecture §23.4 — WARN list
    ///   covers echo loops + batch flush backpressure, NOT health checks.
    pub fn check(&self, max_staleness_ms: u64) -> HealthStatus {
        // P5-L3: three-component probe per architecture §23.3 (Devil M1 —
        // correct Grafeo API is `db.session().execute(...)`, NOT `db.execute(...)`).
        //
        // 1. Loro doc read accessibility: `try_read()` returns `Option`
        //    (parking_lot API) — `Some` if the lock is free or read-locked,
        //    `None` if a writer currently holds it. `parking_lot::RwLock` has
        //    NO poisoning (unlike `std::sync::RwLock`), so this probe verifies
        //    the lock is not held by a writer — NOT poison detection
        //    (P5-HUNT-1 MINOR 1).
        // 2. Grafeo dummy query: `MATCH (n) RETURN count(n) LIMIT 1` is the
        //    lightest possible probe that still exercises the storage layer
        //    (parse + plan + execute + return). `is_ok()` collapses any
        //    error (panic, IO, schema, query-parse) into a single `false`.
        // 3. Sync staleness: `now - last_sync_ts.load(Relaxed) <=
        //    max_staleness_ms`. Uses `saturating_sub` to handle the
        //    time-went-backwards edge case (clock skew, NTP step) — yields 0
        //    which is always `<= max_staleness_ms` (treats backwards clock
        //    as "just synced" rather than "infinitely stale"; anti-plenger
        //    #7 defensive programming).
        let loro_ok = self.doc.try_read().is_some();
        let grafeo_ok = self
            .db
            .session()
            .execute("MATCH (n) RETURN count(n) LIMIT 1")
            .is_ok();
        let now = unix_timestamp_ms();
        let last = self.last_sync_ts.load(Ordering::Relaxed);
        let sync_ok = now.saturating_sub(last) <= max_staleness_ms;
        HealthStatus {
            overall: loro_ok && grafeo_ok && sync_ok,
            components: vec![
                ("loro_doc", loro_ok),
                ("grafeo_db", grafeo_ok),
                ("sync_freshness", sync_ok),
            ],
        }
    }

    /// Test-only accessor for the `last_sync_ts` field (P5-L3). Used by
    /// `tests/unit/telemetry.rs` to deterministically construct stale-sync
    /// scenarios without relying on `tokio::time::sleep` (which is slow +
    /// flaky on CI). Hidden from docs to discourage production use.
    #[doc(hidden)]
    pub fn _last_sync_ts_for_test(&self) -> u64 {
        self.last_sync_ts.load(Ordering::Relaxed)
    }

    /// Test-only setter for the `last_sync_ts` field (P5-L3). Used by
    /// `tests/unit/telemetry.rs` to deterministically simulate stale sync
    /// (e.g., set to `now - 10_000` then call `check(5_000)` to assert
    /// `sync_ok=false`). Hidden from docs to discourage production use.
    #[doc(hidden)]
    pub fn _set_last_sync_ts_for_test(&self, ts: u64) {
        self.last_sync_ts.store(ts, Ordering::Relaxed);
    }
}
