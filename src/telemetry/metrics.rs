//! OpenTelemetry metrics registry (Phase 5 Task 2).
//!
//! # L1 contract layer (P5-L1)
//!
//! All method bodies are `unimplemented!()` — L2 wires the OpenTelemetry SDK
//! calls, L3 fills the algorithm bodies. The struct fields are the five
//! instruments specified in architecture §23.1:
//!
//! | Field | Instrument | Architecture ref |
//! |-------|-----------|-----------------|
//! | `inbound_events` | `Counter<u64>` | §23.1 row 1 |
//! | `outbound_events` | `Counter<u64>` | §23.1 row 2 |
//! | `echo_filtered` | `Counter<u64>` | §23.1 row 3 |
//! | `batch_flush_duration` | `Histogram<f64>` | §23.1 row 4 |
//! | `hydration_duration` | `Histogram<f64>` | §23.1 row 5 |
//!
//! ## Storage convention
//!
//! `MetricsRegistry` is stored as `Arc<MetricsRegistry>` on `GrafeoLoroApp`
//! and `Option<Arc<MetricsRegistry>>` on `SyncEngine` / `MutationBatcher`
//! (Option so test constructors that do not configure telemetry can pass
//! `None`). The registry itself is constructed once in
//! `GrafeoLoroAppBuilder::build` via [`Self::init`] from a `Meter` obtained
//! via `opentelemetry::global::meter("grafeo-loro")` (L2 territory).

use opentelemetry::metrics::{Counter, Histogram, Meter};

/// Registry of bridge/batcher/hydration counters and histograms.
///
/// Built once at app startup via [`Self::init`] from a [`Meter`]. Shared
/// (behind `Arc`) with `SyncEngine` + `MutationBatcher` so worker loops can
/// record without owning their own copy of the instruments.
pub struct MetricsRegistry {
    /// Total Loro events processed by the inbound worker
    /// (`grafeo_loro.sync.inbound_events_total`, §23.1 row 1).
    pub inbound_events: Counter<u64>,
    /// Total CDC events processed by the outbound worker
    /// (`grafeo_loro.sync.outbound_events_total`, §23.1 row 2).
    pub outbound_events: Counter<u64>,
    /// Events dropped by origin tracking (echo prevention)
    /// (`grafeo_loro.sync.echo_filtered_total`, §23.1 row 3).
    pub echo_filtered: Counter<u64>,
    /// Time to commit a batched Grafeo transaction in ms
    /// (`grafeo_loro.sync.batch_flush_duration_ms`, §23.1 row 4).
    pub batch_flush_duration: Histogram<f64>,
    /// Cold-start hydration wall-clock time in ms
    /// (`grafeo_loro.sync.hydration_duration_ms`, §23.1 row 5).
    pub hydration_duration: Histogram<f64>,
}

impl MetricsRegistry {
    /// Build all five instruments from a [`Meter`]. Called once in
    /// `GrafeoLoroAppBuilder::build` (L2 territory — needs
    /// `opentelemetry::global::meter("grafeo-loro")` + u64/f64 meter
    /// constructors).
    ///
    /// # L1 contract
    ///
    /// - Returns a fully-populated `MetricsRegistry` (all 5 fields).
    /// - Instrument names match architecture §23.1 exactly.
    /// - Idempotent over the input `Meter` (calling `init` twice on the
    ///   same `Meter` produces two independent registries — anti-plenger #9
    ///   Absolute Idempotency is about the *recordings*, not the registry
    ///   construction).
    pub fn init(meter: Meter) -> Self {
        let _ = meter;
        unimplemented!("P5-L2: wire `meter.u64_counter(...)` + `meter.f64_histogram(...)` per architecture §23.1")
    }

    /// Record a single batch flush. Called from `MutationBatcher::flush_inner`
    /// after `prepared.commit()` returns (L2 wiring).
    ///
    /// # L1 contract
    ///
    /// - `duration_ms` → `batch_flush_duration.record(duration_ms, [batch_size=N])`
    /// - `batch_size` → attribute set on the histogram record (architecture
    ///   §23.1 row 4 labels: `batch_size`).
    /// - No-op if the registry's instruments are no-ops (test mode).
    pub fn record_batch_flush(&self, duration_ms: f64, batch_size: u64) {
        let _ = (duration_ms, batch_size);
        unimplemented!("P5-L2: call self.batch_flush_duration.record(...) with [batch_size] attribute")
    }

    /// Record a hydration run. Called from `GrafeoLoroApp::hydrate` after
    /// `parallel_hydrate_grafeo` returns (L2 wiring).
    ///
    /// # L1 contract
    ///
    /// - `duration_ms` → `hydration_duration.record(duration_ms, [mode=...])`
    /// - `mode` → attribute set on the histogram record (architecture §23.1
    ///   row 5 labels: `mode` ∈ {`"loro"`, `"grafeo"`}).
    /// - `mode` is `&str` (not enum) to match the OTLP attribute model; the
    ///   caller is responsible for using one of the two architecture-defined
    ///   values. Devil Q6 — should this be a `HydrationMode` enum?
    pub fn record_hydration(&self, duration_ms: f64, mode: &str) {
        let _ = (duration_ms, mode);
        unimplemented!("P5-L2: call self.hydration_duration.record(...) with [mode] attribute")
    }
}
