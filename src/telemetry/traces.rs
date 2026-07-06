//! Span-creation helpers (Phase 5 Task 4).
//!
//! # L1 contract layer (P5-L1)
//!
//! Three top-level span creators matching architecture §23.2 span hierarchy:
//!
//! - [`create_cold_start_span`] — parent of `decompress_snapshot` /
//!   `import_loro_doc` / `parallel_hydrate_grafeo` (architecture §23.2 tree
//!   row 1).
//! - [`create_inbound_sync_span`] — parent of `receive_loro_event` /
//!   `batch_flush` / `index_rebuild` (architecture §23.2 tree row 2).
//! - [`create_hybrid_query_span`] — parent of `hnsw_search` /
//!   `graph_traversal` (architecture §23.2 tree row 5).
//!
//! Each function takes a `&T where T: Tracer` and returns a `BoxedSpan`.
//! The caller owns the returned span; dropping it ends the span. L2 wires
//! the call sites:
//!
//! - `create_cold_start_span` → called at the top of
//!   `GrafeoLoroApp::hydrate` (wraps the whole cold-start sequence).
//! - `create_inbound_sync_span` → called at the top of
//!   `SyncEngine::spawn_inbound_worker`'s `tokio::spawn` closure (wraps the
//!   entire inbound loop — NOT one per op; one per worker lifetime).
//! - `create_hybrid_query_span` → called at the top of
//!   `GrafeoLoroApp::query` (wraps the GQL + HNSW + traversal sequence).
//!
//! ## Devil questions
//!
//! - Q7: Should these functions take `&T where T: Tracer` (current generic
//!   form) or `&SharedTracer` (concrete `&Arc<BoxedTracer>`)? The generic
//!   form allows tests to pass a `noop::NoopTracer`; the concrete form
//!   matches what production stores. Recommendation: keep generic — tests
//!   benefit, production auto-derefs `Arc<T>` to `&T`.
//! - Q8: Span names — architecture §23.2 uses `cold_start_hydration`,
//!   `inbound_sync_loop`, `hybrid_query`. The function names match
//!   (`create_cold_start_span` → span name `"cold_start_hydration"`). L2
//!   should hardcode these names in the SpanBuilder.

use opentelemetry::global::BoxedSpan;
use opentelemetry::trace::Tracer;

/// Open a top-level cold-start hydration span (parent of decompress / import
/// / hydrate per architecture §23.2 tree row 1).
///
/// # L1 contract
///
/// - Calls `tracer.span_builder("cold_start_hydration").start(...)` (or
///   equivalent `tracer.build(...)` API).
/// - Returns the started `BoxedSpan`; caller owns + drops to end the span.
///
/// # L2 wiring
///
/// Called at the top of `GrafeoLoroApp::hydrate`. The span wraps the entire
/// cold-start sequence: storage load → decompress → `doc.import_with` →
/// `parallel_hydrate_grafeo` → `loro_key_counter` re-seed.
pub fn create_cold_start_span<T: Tracer>(tracer: &T) -> BoxedSpan {
    let _ = tracer;
    unimplemented!("P5-L2: tracer.span_builder(\"cold_start_hydration\").start(tracer)")
}

/// Open an inbound-sync loop span (parent of receive / batch / commit spans
/// per architecture §23.2 tree row 2).
///
/// # L1 contract
///
/// - Span name: `"inbound_sync_loop"`.
/// - One per worker lifetime (NOT one per op).
///
/// # L2 wiring
///
/// Called at the top of `SyncEngine::spawn_inbound_worker`'s `tokio::spawn`
/// closure. The span wraps the entire inbound loop lifetime. Child spans
/// (`receive_loro_event`, `batch_flush`) are created inside the loop by L2.
pub fn create_inbound_sync_span<T: Tracer>(tracer: &T) -> BoxedSpan {
    let _ = tracer;
    unimplemented!("P5-L2: tracer.span_builder(\"inbound_sync_loop\").start(tracer)")
}

/// Open a hybrid-query span (parent of HNSW + traversal spans per
/// architecture §23.2 tree row 5).
///
/// # L1 contract
///
/// - Span name: `"hybrid_query"`.
/// - One per `GrafeoLoroApp::query` call.
///
/// # L2 wiring
///
/// Called at the top of `GrafeoLoroApp::query`. The span wraps the GQL
/// parse + HNSW search + graph traversal sequence.
pub fn create_hybrid_query_span<T: Tracer>(tracer: &T) -> BoxedSpan {
    let _ = tracer;
    unimplemented!("P5-L2: tracer.span_builder(\"hybrid_query\").start(tracer)")
}
