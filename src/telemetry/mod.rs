//! Telemetry surface: metrics, tracing, and health checks.
//!
//! # Phase 5 Tasks 2/3/4 (P5-L1 contract layer)
//!
//! This module groups the three observability pillars required by Phase 5
//! (architecture §23):
//!
//! - **metrics** — [`MetricsRegistry`] (Task 2): OpenTelemetry counters +
//!   histograms for inbound/outbound events, echo filters, batch flush
//!   duration, hydration duration.
//! - **health** — [`HealthProbe`] / [`HealthStatus`] (Task 3): probes LoroDoc
//!   lock poison, Grafeo dummy query, and `last_sync_ts` staleness.
//! - **traces** — span-creation helpers in [`traces`] (Task 4): parents for
//!   the cold-start hydration, inbound sync loop, and hybrid-query span
//!   hierarchies defined in architecture §23.2.
//!
//! ## `SharedTracer` (P5-L1)
//!
//! The concrete tracer handle stored on `GrafeoLoroApp` / `SyncEngine` /
//! `MutationBatcher`. [`opentelemetry::global::BoxedTracer`] is `Send + Sync`
//! but NOT `Clone`; wrapping it in [`Arc`] allows the same tracer to be
//! cheaply shared across the three owners without each one re-calling
//! `global::tracer(name)` (which would create three distinct `BoxedTracer`
//! wrappers around the same underlying provider — wasteful + confusing for
//! span attribution). See Devil Q3 — alternative is `Option<BoxedTracer>`
//! per owner (no `Arc`).
//!
//! L1 contract: type alias + `Option<SharedTracer>` field type everywhere.
//! L2 will populate via `global::tracer("grafeo-loro")` in `build()`.

use std::sync::Arc;

use opentelemetry::global::BoxedTracer;

pub mod metrics;
pub mod traces;
pub mod health;

pub use health::{HealthProbe, HealthStatus};
pub use metrics::MetricsRegistry;

/// Shared tracer handle. `Arc<BoxedTracer>` so the same tracer can be cloned
/// into `SyncEngine`, `MutationBatcher`, and `GrafeoLoroApp` without each
/// owner re-calling `global::tracer(name)`. `BoxedTracer` itself is `Send +
/// Sync` but not `Clone` (it wraps `Box<dyn ObjectSafeTracer + Send +
/// Sync>` — verified at `opentelemetry-0.23.0/src/global/trace.rs:244`).
///
/// Stored as `Option<SharedTracer>` on each owner so L2/L3 can detect the
/// "no tracer configured" case (tests + dev mode) and skip span creation
/// without panicking.
pub type SharedTracer = Arc<BoxedTracer>;
