//! Storage backend trait + in-memory reference implementation.
//!
//! Issue #1 item 9: browser-friendly storage trait.
//! Issue #3 sub-issue 9: `SnapshotStreamer` + `SnapshotDiff`.

pub mod memory;
pub mod traits;

pub use memory::InMemoryStorage;
pub use traits::{SnapshotDiff, StorageBackend};

use crate::error::Result;

/// Default chunk size for `SnapshotStreamer` (64 KB).
pub const DEFAULT_SNAPSHOT_CHUNK_SIZE: usize = 64 * 1024;

/// Streams a snapshot in fixed-size chunks. Used by `stream_snapshot_to_opfs`.
pub struct SnapshotStreamer {
    chunk_size: usize,
}

impl SnapshotStreamer {
    pub fn new(chunk_size: usize) -> Self {
        Self {
            chunk_size: chunk_size.max(1),
        }
    }

    /// Stream `data` to `sink` in chunks. Returns chunk count.
    pub fn stream<F: FnMut(&[u8]) -> Result<()>>(&self, data: &[u8], mut sink: F) -> Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let mut n = 0;
        for chunk in data.chunks(self.chunk_size) {
            sink(chunk)?;
            n += 1;
        }
        Ok(n)
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }
}

impl Default for SnapshotStreamer {
    fn default() -> Self {
        Self::new(DEFAULT_SNAPSHOT_CHUNK_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_empty_input_no_calls() {
        let s = SnapshotStreamer::new(64);
        let mut calls = 0;
        assert_eq!(
            s.stream(&[], |_| {
                calls += 1;
                Ok(())
            })
            .unwrap(),
            0
        );
        assert_eq!(calls, 0);
    }

    #[test]
    fn stream_exact_multiple() {
        let s = SnapshotStreamer::new(4);
        let data = vec![0u8; 16];
        let mut sizes = Vec::new();
        let n = s
            .stream(&data, |c| {
                sizes.push(c.len());
                Ok(())
            })
            .unwrap();
        assert_eq!(n, 4);
        assert_eq!(sizes, vec![4, 4, 4, 4]);
    }

    #[test]
    fn stream_partial_last_chunk() {
        let s = SnapshotStreamer::new(8);
        let data = vec![0u8; 20];
        let mut sizes = Vec::new();
        let n = s
            .stream(&data, |c| {
                sizes.push(c.len());
                Ok(())
            })
            .unwrap();
        assert_eq!(n, 3);
        assert_eq!(sizes, vec![8, 8, 4]);
    }

    #[test]
    fn stream_propagates_error() {
        let s = SnapshotStreamer::new(4);
        let data = vec![0u8; 16];
        assert!(s
            .stream(&data, |_| Err(crate::error::GrafeoLoroError::Config(
                "x".into()
            )))
            .is_err());
    }

    #[test]
    fn zero_chunk_size_clamped_to_one() {
        let s = SnapshotStreamer::new(0);
        assert_eq!(s.chunk_size(), 1);
        let data = vec![0u8; 3];
        let mut sizes = Vec::new();
        let n = s
            .stream(&data, |c| {
                sizes.push(c.len());
                Ok(())
            })
            .unwrap();
        assert_eq!(n, 3);
    }
}
