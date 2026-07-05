use opentelemetry::metrics::{Counter, Histogram, Meter};

pub struct MetricsRegistry {
    pub inbound_events: Counter<u64>,
    pub outbound_events: Counter<u64>,
    pub echo_filtered: Counter<u64>,
    pub batch_flush_duration: Histogram<f64>,
    pub hydration_duration: Histogram<f64>,
}

impl MetricsRegistry {
    pub fn init(meter: Meter) -> Self;
    pub fn record_batch_flush(&self, duration_ms: f64, batch_size: u64);
    pub fn record_hydration(&self, duration_ms: f64, mode: &str);
}