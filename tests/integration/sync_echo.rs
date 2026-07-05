//! Echo-loop prevention and bidirectional sync integration tests.
//!
//! Bodies remain `todo!()` at L1; L2/L3 fill in mock Loro + mock Grafeo
//! fixtures and assertions.

#![allow(missing_docs)]

/// Mock Loro + mock Grafeo wired through a real `SyncEngine`. Drive a single
/// user edit through one bridge, assert that the echo path does NOT produce
/// a second mutation in the originating store. Verifies origin filtering on
/// both inbound (grafeo-bridge tag) and outbound (loro-bridge tag) paths.
#[tokio::test]
#[ignore = "L1 contract: body filled by L3"]
async fn echo_loop_prevention() {
    todo!()
}

/// Bidirectional sync with an artificial delay between bridges. Drive N
/// concurrent edits from both sides, wait for the batcher's flush window to
/// elapse, then assert both stores converge to the same state. Verifies
/// time-and-count flush behaviour under interleaved traffic.
#[tokio::test]
#[ignore = "L1 contract: body filled by L3"]
async fn bidirectional_sync_with_delay() {
    todo!()
}
