//! Minimal outliner built on grafeo-loro (issue #1 item 14 worked example).
//!
//! Demonstrates the raw-Loro → grafeo-loro migration path: app construction,
//! tree mutation via the wrapped `LoroDoc`, event subscription, checkpoint,
//! cold-boot hydrate, and graceful shutdown. Build with:
//!
//! ```bash
//! cargo build --features full --example minimal_outliner
//! ```

use std::sync::Arc;
use std::time::Duration;

use grafeo_loro::{CompressionType, GrafeoLoroApp, InMemoryStorage, Result, SsotMode};
use loro::LoroValue;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Build the app — Loro SSOT, zstd compression, in-memory storage.
    let storage = Arc::new(InMemoryStorage::new());
    let app = GrafeoLoroApp::builder()
        .ssot_mode(SsotMode::Loro)
        .compression(CompressionType::Zstd)
        .storage(storage)
        .build()
        .await?;

    // 2. Subscribe to bridge events — multiple subscribers coexist (issue #1
    //    item 4). Onde's orchestrator attaches alongside the bridge's
    //    internal subscriber; both fire in registration order.
    let _sub = app.subscribe(|ev: loro::event::DiffEvent| {
        let _ = ev; // Onde would forward to its UI store here.
    });

    // 3. Cold-boot hydrate — fresh graph (storage empty) returns Ok(())
    //    without work. Warm boot would load + decompress + import the base
    //    snapshot and replay any deltas.
    app.hydrate("graph_1").await?;

    // 4. Mutate via the wrapped LoroDoc. Outliner: two bullets under a root.
    //    The bridge's inbound subscriber translates each commit to a
    //    `LoroOp::UpsertNode` for the Grafeo write path.
    {
        let doc = app.doc();
        let root = doc.get_map("outliner");
        let child1 = doc.get_map("V/child-1");
        child1.insert("text", "First bullet")?;
        let child2 = doc.get_map("V/child-2");
        child2.insert("text", "Second bullet")?;
        root.insert("child-1", LoroValue::String("V/child-1".into()))?;
        root.insert("child-2", LoroValue::String("V/child-2".into()))?;
        doc.commit();
    }
    // Allow the inbound batcher to flush (100ms tick by default).
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 5. Checkpoint — exports a shallow Loro snapshot, compresses it under
    //    zstd, writes to storage under `graph_1/base.loro`. On-wire format
    //    is `[version:u8][codec_tag:u8][raw_data..]`.
    app.checkpoint("graph_1").await?;

    // 6. Graceful shutdown — drains workers + flushes telemetry. Does NOT
    //    auto-checkpoint (Devil Q15); callers that need a final snapshot
    //    call `checkpoint` BEFORE `shutdown`.
    app.shutdown().await?;
    Ok(())
}
