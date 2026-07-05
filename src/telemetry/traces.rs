use opentelemetry::global::BoxedSpan;
use opentelemetry::trace::Tracer;

/// Open a top-level cold-start hydration span (parent of decompress/import/hydrate).
pub fn create_cold_start_span<T: Tracer>(tracer: &T) -> BoxedSpan {
    let _ = tracer;
    unimplemented!()
}

/// Open an inbound-sync loop span (parent of receive/batch/commit spans).
pub fn create_inbound_sync_span<T: Tracer>(tracer: &T) -> BoxedSpan {
    let _ = tracer;
    unimplemented!()
}

/// Open a hybrid-query span (parent of HNSW + traversal spans).
pub fn create_hybrid_query_span<T: Tracer>(tracer: &T) -> BoxedSpan {
    let _ = tracer;
    unimplemented!()
}
