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

use std::fmt;

use opentelemetry::metrics::{Counter, Histogram, Meter};
use opentelemetry::KeyValue;
use tracing::instrument;

/// Hydration mode for `record_hydration` attribute labelling (architecture
/// §23.1 row 5 label `mode` ∈ {`"loro"`, `"grafeo"}`).
///
/// Type-safe replacement for the `&str` form per Devil m1 (Q6 ruling): the
/// enum prevents typos at compile time; [`Display`](fmt::Display) renders the
/// OTLP attribute value (`"loro"` / `"grafeo"`) for the histogram record.
///
/// # L2 contract (P5-L2 — Devil m1)
///
/// - `Loro` → `"loro"` (architecture §23.1 row 5).
/// - `Grafeo` → `"grafeo"`.
/// - Callers (`GrafeoLoroApp::hydrate`) map `SsotMode::Loro →
///   HydrationMode::Loro` / `SsotMode::Grafeo → HydrationMode::Grafeo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HydrationMode {
    /// Loro SSOT hydration (snapshot → Loro → Grafeo indexes).
    Loro,
    /// Grafeo SSOT hydration (Grafeo graph → Loro mirror).
    Grafeo,
}

impl fmt::Display for HydrationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HydrationMode::Loro => f.write_str("loro"),
            HydrationMode::Grafeo => f.write_str("grafeo"),
        }
    }
}

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
        // P5-L3: build all five instruments from the meter. Names match
        // architecture §23.1 exactly. The `init()` call on each builder
        // returns a fully-constructed instrument (no-op if `meter` came from
        // `opentelemetry::global::meter(...)` with no SDK installed — tests).
        Self {
            inbound_events: meter.u64_counter("inbound_events_total").init(),
            outbound_events: meter.u64_counter("outbound_events_total").init(),
            echo_filtered: meter.u64_counter("echo_filtered_total").init(),
            batch_flush_duration: meter.f64_histogram("batch_flush_duration_ms").init(),
            hydration_duration: meter.f64_histogram("hydration_duration_ms").init(),
        }
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
    #[instrument(skip(self), name = "record_batch_flush", level = "trace")]
    pub fn record_batch_flush(&self, duration_ms: f64, batch_size: u64) {
        // P5-L3: architecture §23.1 row 4 — `batch_flush_duration_ms` with
        // label `batch_size`. `u64` → `i64` cast because OTel `Value` does
        // not implement `From<u64>` (only `i64` / `f64` / `bool` / strings);
        // `batch_size` realistically stays well below `i64::MAX`.
        self.batch_flush_duration.record(
            duration_ms,
            &[KeyValue::new("batch_size", batch_size as i64)],
        );
    }

    /// Record a hydration run. Called from `GrafeoLoroApp::hydrate` after
    /// `parallel_hydrate_grafeo` returns (L2 wiring).
    ///
    /// # L1 contract
    ///
    /// - `duration_ms` → `hydration_duration.record(duration_ms, &[mode=...])`
    /// - `mode` → attribute set on the histogram record (architecture §23.1
    ///   row 5 labels: `mode` ∈ {`"loro"`, `"grafeo"`}). Type-safe
    ///   [`HydrationMode`] enum per Devil m1 (Q6 ruling) — `&str` form
    ///   replaced by the enum to prevent typos at compile time; the enum's
    ///   `Display` impl renders the OTLP attribute value (`"loro"` / `"grafeo"`).
    #[instrument(skip(self), name = "record_hydration", level = "trace")]
    pub fn record_hydration(&self, duration_ms: f64, mode: HydrationMode) {
        // P5-L3: architecture §23.1 row 5 — `hydration_duration_ms` with
        // label `mode` ∈ {`"loro"`, `"grafeo"`}. `mode.to_string()` is
        // pre-computed once (not on every `record` call hot-path for the
        // Display impl itself; the impl is a `match` + `write_str` — cheap).
        self.hydration_duration
            .record(duration_ms, &[KeyValue::new("mode", mode.to_string())]);
    }
}
