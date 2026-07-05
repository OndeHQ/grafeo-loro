use opentelemetry::trace::{Span, Tracer};

pub fn create_cold_start_span(tracer: &impl Tracer) -> Span;
pub fn create_inbound_sync_span(tracer: &impl Tracer) -> Span;
pub fn create_hybrid_query_span(tracer: &impl Tracer) -> Span;