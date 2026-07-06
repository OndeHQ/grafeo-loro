// Origin tags prevent echo feedback loops
pub const ORIGIN_GRAFEO_BRIDGE: &str = "grafeo-bridge";
pub const ORIGIN_LORO_BRIDGE: &str = "loro-bridge";

// Root LoroDoc container keys
pub const ROOT_VERTICES: &str = "V";
pub const ROOT_EDGES: &str = "E";
// Phase 2: tree container support — `ROOT_TREE: &str = "T_CHILD"` was deleted
// as YAGNI per Hunter NIT 11 (declared but never read in Phase 1). Re-add when
// the inbound subscriber grows a tree-container diff arm.

// Tree parent-child edge label. Existing `apply_tree_move`
// (`src/bridge/grafeo_tx.rs:200-206`) hardcodes `"CHILD"` and uses
// child→parent direction (src=child, dst=parent) — this constant is the SSOT
// for the label literal; direction is enforced at call sites.
pub const TREE_EDGE_LABEL: &str = "CHILD";

// Ephemeral presence magic bytes
pub const EPH_MAGIC: &[u8; 4] = b"%EPH";

// Defaults
pub const DEFAULT_BATCH_MS: u64 = 100;
pub const DEFAULT_BATCH_SIZE: usize = 256;
pub const DEFAULT_CHUNK_SIZE: usize = 256;
pub const DEFAULT_STALENESS_MS: u64 = 5000;

// Outbound CDC poller cadence (Grafeo→Loro path). Grafeo 0.5.42 CDC is
// poll-based: the outbound worker calls `session.changes_between(start, end)`
// on this interval. 50 ms ≈ 20 polls/sec — low latency without burning CPU.
pub const OUTBOUND_POLL_MS: u64 = 50;

// Epoch side-channel retention window. The `bridge_origin_epochs` set keeps
// epochs produced by Loro→Grafeo bridge writes so the outbound poller can
// filter them out (echo prevention). Pruning keeps only epochs newer than
// `last_polled_epoch - EPOCH_RETENTION` so the set does not grow unbounded.
pub const EPOCH_RETENTION: u64 = 10_000;