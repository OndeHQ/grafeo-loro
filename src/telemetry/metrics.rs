use opentelemetry::metrics::{Counter, Histogram, Meter};

/// Registry of bridge/batcher/hydration counters and histograms.
pub struct MetricsRegistry {
    /// Total Loro events processed by the inbound worker.
    pub inbound_events: Counter<u64>,
    /// Total CDC events processed by the outbound worker.
    pub outbound_events: Counter<u64>,
    /// Events dropped by origin tracking (echo prevention).
    pub echo_filtered: Counter<u64>,
    /// Time to commit a batched Grafeo transaction (ms).
    pub batch_flush_duration: Histogram<f64>,
    /// Cold-start hydration wall-clock time (ms).
    pub hydration_duration: Histogram<f64>,
}

impl MetricsRegistry {
    /// Build all instruments from a [`Meter`].
    pub fn init(meter: Meter) -> Self {
        let _ = meter;
        unimplemented!()
    }

    /// Record a single batch flush.
    pub fn record_batch_flush(&self, duration_ms: f64, batch_size: u64) {
        let _ = (duration_ms, batch_size);
        unimplemented!()
    }

    /// Record a hydration run.
    pub fn record_hydration(&self, duration_ms: f64, mode: &str) {
        let _ = (duration_ms, mode);
        unimplemented!()
    }
}
