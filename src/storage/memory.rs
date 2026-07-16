//! In-memory storage backend — reference impl for unit tests.
//!
//! Issue #3 sub-issue 9: naive reference impls. The in-memory backend has
//! no incremental state-vector tracking — returns the full snapshot bytes
//! when a delta is requested and `base != head`. `diff_snapshots` uses a
//! byte-length heuristic (8 bytes per "op").

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use super::traits::SnapshotDiff;
use super::{SnapshotStreamer, StorageBackend};
use crate::error::Result;

/// Conventional key for the in-memory backend's "current snapshot" bytes.
pub const SNAPSHOT_KEY: &str = "snapshot";
const OP_BYTES_HEURISTIC: u64 = 8;

#[derive(Default)]
pub struct InMemoryStorage {
    inner: Mutex<HashMap<String, Vec<u8>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait(?Send)]
impl StorageBackend for InMemoryStorage {
    async fn load(&self, key: &str) -> std::result::Result<Vec<u8>, std::io::Error> {
        let inner = self.inner.lock().unwrap();
        inner.get(key).cloned().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("key not found: {key}"),
            )
        })
    }
    async fn save(&self, key: &str, bytes: Vec<u8>) -> std::result::Result<(), std::io::Error> {
        self.inner.lock().unwrap().insert(key.to_string(), bytes);
        Ok(())
    }
    async fn list(&self, prefix: &str) -> std::result::Result<Vec<String>, std::io::Error> {
        let inner = self.inner.lock().unwrap();
        let mut keys: Vec<String> = inner
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort();
        Ok(keys)
    }
    async fn delete(&self, key: &str) -> std::result::Result<(), std::io::Error> {
        self.inner.lock().unwrap().remove(key);
        Ok(())
    }

    async fn export_incremental_snapshot(&self, since_state_vector: &[u8]) -> Result<Vec<u8>> {
        let inner = self.inner.lock().unwrap();
        let Some(current) = inner.get(SNAPSHOT_KEY) else {
            return Ok(Vec::new());
        };
        if since_state_vector == current.as_slice() {
            return Ok(Vec::new());
        }
        Ok(current.clone())
    }

    async fn stream_snapshot_to_opfs(
        &self,
        chunk_callback: &(dyn for<'a> Fn(&'a [u8]) -> Result<()> + Send + Sync),
    ) -> Result<()> {
        let bytes = {
            let inner = self.inner.lock().unwrap();
            inner.get(SNAPSHOT_KEY).cloned().unwrap_or_default()
        };
        let chunk_size = SnapshotStreamer::default().chunk_size();
        for chunk in bytes.chunks(chunk_size) {
            chunk_callback(chunk)?;
        }
        Ok(())
    }

    async fn diff_snapshots(&self, base: &[u8], head: &[u8]) -> Result<SnapshotDiff> {
        if base == head {
            return Ok(SnapshotDiff::empty());
        }
        let base_ops = (base.len() as u64) / OP_BYTES_HEURISTIC;
        let head_ops = (head.len() as u64) / OP_BYTES_HEURISTIC;
        let added = head_ops.saturating_sub(base_ops);
        let removed = base_ops.saturating_sub(head_ops);
        let signed_delta = head_ops as i64 - base_ops as i64;
        Ok(SnapshotDiff {
            added_ops: added,
            removed_ops: removed,
            state_vector_delta: signed_delta.to_le_bytes().to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip() {
        let s = InMemoryStorage::new();
        s.save("k", vec![1, 2, 3]).await.unwrap();
        assert_eq!(s.load("k").await.unwrap(), vec![1, 2, 3]);
        assert!(
            matches!(s.load("missing").await, Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        );
        s.delete("k").await.unwrap();
        assert!(matches!(s.load("k").await, Err(e) if e.kind() == std::io::ErrorKind::NotFound));
    }

    #[tokio::test]
    async fn list_prefix() {
        let s = InMemoryStorage::new();
        s.save("graph_1/base.loro", vec![]).await.unwrap();
        s.save("graph_1/delta-1.loro", vec![]).await.unwrap();
        s.save("graph_2/base.loro", vec![]).await.unwrap();
        let keys = s.list("graph_1/").await.unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[tokio::test]
    async fn incremental_snapshot_no_changes_returns_empty() {
        let s = InMemoryStorage::new();
        let snap = b"hello-snapshot".to_vec();
        s.save(SNAPSHOT_KEY, snap.clone()).await.unwrap();
        assert!(s
            .export_incremental_snapshot(&snap)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn incremental_snapshot_no_snapshot_returns_empty() {
        let s = InMemoryStorage::new();
        assert!(s
            .export_incremental_snapshot(b"whatever")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn incremental_snapshot_changed_returns_full() {
        let s = InMemoryStorage::new();
        let snap = b"hello-snapshot".to_vec();
        s.save(SNAPSHOT_KEY, snap.clone()).await.unwrap();
        assert_eq!(
            s.export_incremental_snapshot(b"stale-sv").await.unwrap(),
            snap
        );
    }

    #[tokio::test]
    async fn diff_snapshots_identical_is_empty() {
        let s = InMemoryStorage::new();
        let bytes = vec![1u8; 16];
        assert_eq!(
            s.diff_snapshots(&bytes, &bytes).await.unwrap(),
            SnapshotDiff::empty()
        );
    }

    #[tokio::test]
    async fn diff_snapshots_growth() {
        let s = InMemoryStorage::new();
        let diff = s
            .diff_snapshots(&vec![0u8; 8], &vec![0u8; 24])
            .await
            .unwrap();
        assert_eq!(diff.added_ops, 2);
        assert_eq!(diff.removed_ops, 0);
    }
}
