//! Phase 3 Task 2: parallel cold-boot hydration of Grafeo indexes from a Loro snapshot.
//!
//! Read-path SSOT: `VertexEntity::hydrate_map(&LoroMap)` (`lorosurgeon-0.2.1/src/hydrate.rs:127`)
//! — DO NOT manually iterate the vertex sub-map's keys (DEVIL M2 DRY).

use std::sync::Arc;

use grafeo::GrafeoDB;
use lorosurgeon::Hydrate;
use loro::LoroDoc;
use rayon::prelude::*;

use crate::bridge::apply_loro_op;
use crate::bridge::grafeo_tx::BridgeMaps;
use crate::constants::{DEFAULT_CHUNK_SIZE, ORIGIN_LORO_BRIDGE, ROOT_VERTICES};
use crate::error::{GrafeoLoroError, Result};
use crate::schema::vertex::VertexEntity;
use crate::types::events::LoroOp;
use crate::types::values::GraphValue;

/// Rebuilds Grafeo indexes from Loro state using Rayon chunks of `DEFAULT_CHUNK_SIZE`; each chunk runs in its own Grafeo `Session` transaction tagged with `ORIGIN_LORO_BRIDGE`, and the `loro_key ↔ NodeId` mapping is recorded in `maps`. Fail-fast: the first chunk error aborts the whole call (anti-plenger #9 Absolute Idempotency — no partial success, no inconsistency).
///
/// # Preconditions
///
/// - `GrafeoDB` is empty (cold boot) or its state is consistent with a prior snapshot.
/// - `bridge::sync_engine` subscriber is NOT yet active (otherwise the subscriber fires on each hydrated vertex and re-creates it via `apply_loro_op`, producing duplicates — `session_with_cdc(false)` only suppresses the outbound Grafeo→Loro echo, NOT the inbound Loro→Grafeo echo).
/// - `BridgeMaps` is empty (cold boot) or matches the prior Grafeo state.
///
/// `VertexEntity::description` (`LoroText`) is Loro-only — NOT written to Grafeo (per `src/app.rs:201`). Hydration skips it; only `labels` + `properties` are materialized in Grafeo. The read-path SSOT is `VertexEntity::hydrate_map(&LoroMap)` (`lorosurgeon-0.2.1/src/hydrate.rs:127`), which naturally isolates `description` from `properties` — DO NOT manually iterate the vertex sub-map's keys (DEVIL M2 DRY).
///
/// # Errors
///
/// - [`GrafeoLoroError::Grafeo`][crate::error::GrafeoLoroError::Grafeo] if any per-chunk tx fails (begin / mutate / prepare / commit). The failing chunk's `Session::Drop` auto-rollbacks its tx; previously-committed chunks remain committed.
/// - [`GrafeoLoroError::Bridge`][crate::error::GrafeoLoroError::Bridge] if a vertex sub-map is not a `Container::Map` (vertex shape mismatch at the Loro container level).
/// - [`GrafeoLoroError::Hydrate`][crate::error::GrafeoLoroError::Hydrate] if `VertexEntity::hydrate_map` fails (vertex field-shape mismatch — missing required property, type mismatch, overflow, JSON failure). Structured `lorosurgeon::error::HydrateError` is preserved (P3T2-L2R2 M2 — replaces the prior `Bridge(format!(...))` band-aid).
///
/// # Idempotency assumption
///
/// Caller guarantees `GrafeoDB` + `BridgeMaps` are cold. Re-running on a warm DB will create duplicate nodes (no upsert check). Phase 4 `hydrate()` enforces this.
pub fn parallel_hydrate_grafeo(db: &Arc<GrafeoDB>, doc: &LoroDoc, maps: &BridgeMaps) -> Result<()> {
    // 1. Extract vertex keys from Loro root map "V". `LoroDoc::get_map` returns
    //    an empty LoroMap if the key is absent (cold-boot empty-doc edge case).
    //    `LoroMap::keys` yields `InternalString` (`loro-1.13.6/src/lib.rs:2315`),
    //    which `Display`s as `&str` (`loro-common-1.13.1/src/internal_string.rs:194`).
    let v_root = doc.get_map(ROOT_VERTICES);
    let keys: Vec<String> = v_root.keys().map(|s| s.to_string()).collect();

    // 2. Parallel chunk processing via rayon::par_chunks. Session is
    //    single-threaded (grafeo-engine-0.5.42/src/session/mod.rs), so each
    //    chunk owns its own Session. `try_for_each` propagates the first `Err`
    //    and short-circuits remaining chunks (fail-fast anti-plenger #9).
    keys.par_chunks(DEFAULT_CHUNK_SIZE).try_for_each(|chunk| -> Result<()> {
        // 3. Per-chunk Grafeo session: CDC off suppresses outbound echoes
        //    (matches `VertexBuilder::commit` at `src/app.rs:437`). On any
        //    error below, `Session::Drop` auto-rollbacks the un-prepared-commit'd
        //    tx (`session/mod.rs:5368-5383`) — compensation is just `drop(session)`.
        let mut session = db.session_with_cdc(false);
        session.begin_transaction()?;

        // 4. Per-vertex hydration via SSOT (DEVIL M2 — DO NOT manually iterate
        //    fields). `v_root.get(key)` returns `Option<ValueOrContainer>`
        //    (`loro-1.13.6/src/lib.rs:2150`); `ValueOrContainer::into_container`
        //    + `Container::into_map` extract the `LoroMap` (`EnumAsInner` at
        //    `:3813` / `:3636`). `VertexEntity::hydrate_map` is the trait
        //    method (`lorosurgeon-0.2.1/src/hydrate.rs:64`) on `Hydrate`.
        for key in chunk {
            let voc = v_root.get(key).ok_or_else(|| {
                GrafeoLoroError::Bridge(format!("vertex {key} missing from LoroMap"))
            })?;
            // `ValueOrContainer::into_container` returns `Result<Container, Self>`
            // and `Container::into_map` returns `Result<LoroMap, Self>` (both via
            // `EnumAsInner`); collapse the two `Result`s to a single `Option`
            // before `ok_or_else` (the original enums are diagnostic only).
            let vertex_map = voc
                .into_container()
                .ok()
                .and_then(|c| c.into_map().ok())
                .ok_or_else(|| {
                    GrafeoLoroError::Bridge(format!("vertex {key} is not a Container::Map"))
                })?;
            let entity: VertexEntity = VertexEntity::hydrate_map(&vertex_map)?;

            // 5. Build `LoroOp::UpsertNode` and reuse the SSOT apply path
            //    (`src/bridge/grafeo_tx.rs:86`) — `apply_upsert_node` handles
            //    the `node_id_map` lookup + `create_node_with_props` +
            //    `maps.insert_node` triplet (DRY; anti-plenger #2 + #5).
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

        // 6. Prepare + commit with origin tag. `set_metadata` is advisory-only
        //    per Devil Gap 1 (dropped on commit per `src/app.rs:461-465`); the
        //    real echo-prevention side-channel is `bridge_origin_epochs` in
        //    `SyncEngine` (§9). `prepare_commit` borrows `&mut session`;
        //    `prepared.commit()` consumes `prepared` and releases the borrow.
        let mut prepared = session.prepare_commit()?;
        prepared.set_metadata(ORIGIN_LORO_BRIDGE, ORIGIN_LORO_BRIDGE);
        prepared.commit()?;
        Ok(())
    })
}
