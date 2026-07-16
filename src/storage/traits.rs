//! Storage backend trait — issue #1 item 9 + issue #3 sub-issue 9.
//!
//! Browser-friendly: `&self` not `&mut self`, `async_trait`, no `tokio::fs`.
//!
//! Issue #3 sub-issue 9 additions:
//! - `export_incremental_snapshot` — delta since a state vector.
//! - `stream_snapshot_to_opfs` — chunked streaming for OPFS.
//! - `diff_snapshots` — state-vector delta between two snapshots.

use async_trait::async_trait;

use crate::error::Result;

/// Persistent storage backend for cold-snapshot persistence.
#[async_trait(?Send)]
pub trait StorageBackend: 'static {
    async fn load(&self, key: &str) -> std::result::Result<Vec<u8>, std::io::Error>;
    async fn save(&self, key: &str, bytes: Vec<u8>) -> std::result::Result<(), std::io::Error>;
    async fn list(&self, prefix: &str) -> std::result::Result<Vec<String>, std::io::Error>;
    async fn delete(&self, key: &str) -> std::result::Result<(), std::io::Error>;

    /// Incremental snapshot export (issue #3 sub-issue 9). Returns only
    /// the delta since `since_state_vector`. Empty vec if no changes.
    async fn export_incremental_snapshot(&self, since_state_vector: &[u8]) -> Result<Vec<u8>>;

    /// Streaming snapshot write for OPFS (issue #3 sub-issue 9). Calls
    /// `chunk_callback` for each chunk (~64KB).
    ///
    /// The callback is `for<'a> Fn(&'a [u8])` (HRTB) — explicit because
    /// `async_trait`'s desugaring would otherwise tie the `&[u8]` argument
    /// lifetime to the `&self` borrow.
    async fn stream_snapshot_to_opfs(
        &self,
        chunk_callback: &(dyn for<'a> Fn(&'a [u8]) -> Result<()> + Send + Sync),
    ) -> Result<()>;

    /// Snapshot diffing API (issue #3 sub-issue 9).
    async fn diff_snapshots(&self, base: &[u8], head: &[u8]) -> Result<SnapshotDiff>;
}

/// Snapshot diff result (issue #3 sub-issue 9).
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotDiff {
    pub added_ops: u64,
    pub removed_ops: u64,
    pub state_vector_delta: Vec<u8>,
}

impl SnapshotDiff {
    pub fn empty() -> Self {
        Self {
            added_ops: 0,
            removed_ops: 0,
            state_vector_delta: Vec::new(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.added_ops == 0 && self.removed_ops == 0
    }
}

impl Default for SnapshotDiff {
    fn default() -> Self {
        Self::empty()
    }
}
