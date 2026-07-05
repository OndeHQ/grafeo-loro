//! Echo-loop prevention and bidirectional sync integration tests.
//!
//! L2 wiring: both tests construct a real `SyncEngine` over a fresh in-memory
//! `GrafeoDB` + `LoroDoc`, call `spawn_all`, and then `todo!()` out before
//! asserting anything. The construction path proves the wiring compiles
//! end-to-end (channel plumbing, batcher ownership, subscription storage,
//! shutdown broadcast). L3 fills in the mock traffic + assertions.

#![allow(missing_docs)]

use std::sync::Arc;

use grafeo::GrafeoDB;
use loro::LoroDoc;
use parking_lot::RwLock;

use grafeo_loro::bridge::SyncEngine;

/// Mock Loro + mock Grafeo wired through a real `SyncEngine`. Drive a single
/// user edit through one bridge, assert that the echo path does NOT produce
/// a second mutation in the originating store. Verifies origin filtering on
/// both inbound (grafeo-bridge tag) and outbound (loro-bridge tag) paths.
#[tokio::test]
#[ignore = "L2 wiring: construction compiles; L3 fills body"]
async fn echo_loop_prevention() {
    // Construct a real SyncEngine over fresh in-memory stores.
    let grafeo_db = Arc::new(GrafeoDB::new_in_memory());
    let loro_doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, inbound_rx, outbound_rx) = SyncEngine::new(grafeo_db, loro_doc);
    let engine = Arc::new(engine);

    // Spawn all workers (inbound + batcher, outbound, CDC poller) + init the
    // Loro subscriber. This proves the wiring compiles end-to-end.
    let _handles = engine.clone().spawn_all(inbound_rx, outbound_rx).await;

    // TODO L3: drive a single Loro edit, await the batcher's flush window,
    // assert that the outbound CDC poller filters out the echo via the
    // epoch side-channel (no second mutation lands in Loro).
    engine.shutdown();
    todo!()
}

/// Bidirectional sync with an artificial delay between bridges. Drive N
/// concurrent edits from both sides, wait for the batcher's flush window to
/// elapse, then assert both stores converge to the same state. Verifies
/// time-and-count flush behaviour under interleaved traffic.
#[tokio::test]
#[ignore = "L2 wiring: construction compiles; L3 fills body"]
async fn bidirectional_sync_with_delay() {
    let grafeo_db = Arc::new(GrafeoDB::new_in_memory());
    let loro_doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, inbound_rx, outbound_rx) = SyncEngine::new(grafeo_db, loro_doc);
    let engine = Arc::new(engine);

    let _handles = engine.clone().spawn_all(inbound_rx, outbound_rx).await;

    // TODO L3: drive N concurrent edits from both sides via
    // `engine.inbound_sender()` and direct Grafeo writes, await the batcher
    // flush window + CDC poll interval, assert convergence.
    engine.shutdown();
    todo!()
}
