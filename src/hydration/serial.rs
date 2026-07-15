//! Serial cold-boot hydration — the default impl (issue #1 item 3).
//!
//! Available whenever `grafeo` is on; no `rayon` dep. WASM-safe.
//!
//! `parallel_hydrate_grafeo` (in `parallel.rs`, gated by `parallel` feature)
//! is the multi-threaded alternative for native builds.

#![cfg(feature = "grafeo")]

use std::sync::Arc;

use grafeo::GrafeoDB;
use loro::LoroDoc;
use lorosurgeon::Hydrate;
#[cfg(feature = "telemetry")]
use opentelemetry::trace::Tracer;
#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::bridge::apply_loro_op;
use crate::bridge::grafeo_tx::BridgeMaps;
use crate::constants::{DEFAULT_CHUNK_SIZE, ORIGIN_LORO_BRIDGE, ROOT_VERTICES};
use crate::error::{GrafeoLoroError, Result};
use crate::schema::vertex::VertexEntity;
#[cfg(feature = "telemetry")]
use crate::telemetry::{MetricsRegistry, SharedTracer};
use crate::types::events::LoroOp;
use crate::types::values::GraphValue;

/// Serial cold-boot hydration: rebuilds Grafeo indexes from Loro state
/// using a single-threaded loop. This is the default impl (issue #1 item 3)
/// and the WASM-safe path.
///
/// # Preconditions
///
/// Same as `parallel_hydrate_grafeo` — see that function's docs.
///
/// # Errors
///
/// Same as `parallel_hydrate_grafeo` — see that function's docs.
#[cfg_attr(feature = "telemetry", instrument(
    skip(db, doc, maps, metrics, tracer),
    name = "hydrate_grafeo",
    level = "info"
))]
pub fn hydrate_grafeo(
    db: &Arc<GrafeoDB>,
    doc: &LoroDoc,
    maps: &BridgeMaps,
    #[cfg(feature = "telemetry")] metrics: Option<&Arc<MetricsRegistry>>,
    #[cfg(feature = "telemetry")] tracer: Option<&SharedTracer>,
) -> Result<()> {
    #[cfg(feature = "telemetry")]
    let _ = metrics;
    #[cfg(feature = "telemetry")]
    let _serial_span = tracer.map(|t| {
        t.as_ref()
            .span_builder("hydrate_grafeo")
            .start(t.as_ref())
    });

    let v_root = doc.get_map(ROOT_VERTICES);
    let keys: Vec<String> = v_root.keys().map(|s| s.to_string()).collect();

    // Serial chunk processing — same logic as parallel_hydrate_grafeo but
    // using `chunks` instead of `par_chunks`.
    for chunk in keys.chunks(DEFAULT_CHUNK_SIZE) {
        #[cfg(feature = "telemetry")]
        let _chunk_span = tracer
            .as_ref()
            .map(|t| t.span_builder("hydrate_chunk").start(t.as_ref()));

        let mut session = db.session_with_cdc(false);
        session.begin_transaction()?;

        for key in chunk {
            let voc = v_root.get(key).ok_or_else(|| {
                GrafeoLoroError::Bridge(format!("vertex {key} missing from LoroMap"))
            })?;
            let vertex_map = voc
                .into_container()
                .ok()
                .and_then(|c| c.into_map().ok())
                .ok_or_else(|| {
                    GrafeoLoroError::Bridge(format!("vertex {key} is not a Container::Map"))
                })?;
            let entity: VertexEntity = VertexEntity::hydrate_map(&vertex_map)?;

            let op = LoroOp::UpsertNode {
                loro_key: key.clone(),
                labels: entity.labels,
                properties: entity
                    .properties
                    .into_iter()
                    .map(|(k, v)| (k, GraphValue::from(v)))
                    .collect(),
            };
            apply_loro_op(&session, &op, maps)?;
        }

        let mut prepared = session.prepare_commit()?;
        prepared.set_metadata(ORIGIN_LORO_BRIDGE, ORIGIN_LORO_BRIDGE);
        prepared.commit()?;
    }
    Ok(())
}
