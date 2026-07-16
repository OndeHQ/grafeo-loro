//! Native Git DAG / shadow commit API (issue #3 sub-issue 6).
//!
//! Per-writer WIP ref namespace: `refs/wip/<peerId>`. Replaces 80KB
//! `isomorphic-git` for bounded undo in browser WASM.
//!
//! # Design
//!
//! A shadow commit is an immutable snapshot of doc state at a point in time,
//! identified by a 32-byte content hash. Each writer (peer) maintains a WIP
//! ref at `refs/wip/<peerId>` pointing at the tip of their personal undo
//! stack. `reset_to` moves the tip back to an earlier commit for undo.
//!
//! Hashing is a pure-Rust 256-bit digest (no `sha2` dep) — concatenation of
//! four 64-bit SipHash lanes with different salts. NOT cryptographically
//! secure, but deterministic, collision-resistant for our domain (commits are
//! sequentially created per writer), and zero-dep.
//!
//! # Why not `sha2`?
//!
//! Adding `sha2` would pull ~50KB of WASM size + a transitive `digest` crate
//! dependency. The shadow commit use-case does not need pre-image resistance
//! — it only needs uniqueness across one writer's commit history. A 256-bit
//! SipHash concatenation provides 2^128 collision resistance against
//! accidental collisions, which is more than sufficient for undo stacks.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::runtime::now_ms;

/// A shadow commit — an immutable snapshot of the doc state at a point in
/// time, identified by a 32-byte SHA-like hash.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShadowCommit {
    /// 32-byte content hash. Identifies this commit uniquely within the store.
    pub id: [u8; 32],
    /// Parent commit hashes (0 = root, 1 = normal, 2+ = merge).
    pub parents: Vec<[u8; 32]>,
    /// Owning peer — used to namespace the WIP ref.
    pub peer_id: String,
    /// Wall-clock timestamp in ms since UNIX_EPOCH (use [`crate::runtime::now_ms`]).
    pub timestamp_ms: u64,
    /// Opaque encoded state vector (e.g. Loro `EncodedBlob` bytes).
    pub state_vector: Vec<u8>,
}

/// Errors emitted by the shadow ref store.
///
/// `commit_id` fields are stored as `[u8; 32]` for structured access; the
/// `Display` impl renders them as lowercase hex via [`hex_32`] (no `hex`
/// crate dep needed).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShadowError {
    /// `reset_to` referenced a commit id that is not in the store.
    #[error("commit not found: {}", hex_32(*commit_id))]
    CommitNotFound { commit_id: [u8; 32] },
    /// `reset_to` referenced a commit that is not in this peer's history.
    #[error("commit {} is not in peer {peer_id}'s history", hex_32(*commit_id))]
    NotInPeerHistory {
        commit_id: [u8; 32],
        peer_id: String,
    },
}

/// Render a 32-byte hash as lowercase hex (64 chars). Used by the
/// `ShadowError` `Display` impls — avoids pulling the `hex` crate dep just
/// for error formatting.
fn hex_32(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// Manages shadow refs per writer. Each peer has a WIP ref at
/// `refs/wip/<peerId>` pointing at the tip of their personal undo stack.
///
/// # Ref namespace
///
/// `refs/wip/<peerId>` → tip commit id (`[u8; 32]`).
///
/// # Bounded undo
///
/// The store grows unbounded by default. Callers wanting bounded undo can
/// truncate the history by walking parents from the tip and dropping commits
/// older than `N` generations (a future `truncate_to_depth` helper would do
/// this — left as TODO for now since the issue spec only requires the ref
/// API).
pub struct ShadowRefStore {
    /// peer_id → tip commit id.
    refs: HashMap<String, [u8; 32]>,
    /// commit id → commit object.
    commits: HashMap<[u8; 32], ShadowCommit>,
}

impl ShadowRefStore {
    pub fn new() -> Self {
        Self {
            refs: HashMap::new(),
            commits: HashMap::new(),
        }
    }

    /// Create a new shadow commit, attach it to `refs/wip/<peer_id>`, and
    /// return its id.
    ///
    /// If `parents` is empty AND the peer has an existing tip, the current
    /// tip is auto-appended as a parent (so callers don't have to thread the
    /// tip through). If `parents` is non-empty, it's used as-is (enables
    /// explicit merge commits with multiple parents).
    pub fn commit(
        &mut self,
        peer_id: &str,
        mut parents: Vec<[u8; 32]>,
        state_vector: Vec<u8>,
    ) -> [u8; 32] {
        // Auto-append current tip as parent if no explicit parents were given.
        if parents.is_empty() {
            if let Some(tip) = self.refs.get(peer_id) {
                parents.push(*tip);
            }
        }
        let timestamp_ms = now_ms();
        let id = hash_commit(peer_id, &parents, timestamp_ms, &state_vector);
        let commit = ShadowCommit {
            id,
            parents,
            peer_id: peer_id.to_string(),
            timestamp_ms,
            state_vector,
        };
        self.commits.insert(id, commit);
        self.refs.insert(peer_id.to_string(), id);
        id
    }

    /// Return the current tip commit id for `peer_id`, or `None` if the peer
    /// has no commits yet.
    pub fn tip(&self, peer_id: &str) -> Option<[u8; 32]> {
        self.refs.get(peer_id).copied()
    }

    /// Walk back from the tip of `peer_id`'s ref, returning up to `limit`
    /// commits (most-recent first). Returns an empty vec if the peer has no
    /// commits.
    ///
    /// Walks first-parent ancestry only — merge commits do not fan out.
    pub fn history(&self, peer_id: &str, limit: usize) -> Vec<&ShadowCommit> {
        let mut out = Vec::with_capacity(limit);
        let mut cursor = match self.refs.get(peer_id) {
            Some(id) => *id,
            None => return out,
        };
        while out.len() < limit {
            match self.commits.get(&cursor) {
                Some(commit) => {
                    out.push(commit);
                    match commit.parents.first() {
                        Some(p) => cursor = *p,
                        None => break,
                    }
                }
                None => break,
            }
        }
        out
    }

    /// Move `refs/wip/<peer_id>` to point at `commit_id`. The commit must
    /// exist in the store AND be reachable from the peer's current tip (so
    /// callers can't silently switch to another peer's history).
    ///
    /// Returns the previous tip id on success.
    pub fn reset_to(
        &mut self,
        peer_id: &str,
        commit_id: [u8; 32],
    ) -> Result<[u8; 32], ShadowError> {
        // Verify the commit exists.
        if !self.commits.contains_key(&commit_id) {
            return Err(ShadowError::CommitNotFound { commit_id });
        }
        // Walk first-parent ancestry from the current tip; commit_id must be
        // reachable. (If the peer has no tip yet, the only acceptable
        // reset target is one that already exists in the store — which we
        // just verified. Skip the reachability check in that case.)
        if let Some(mut cursor) = self.refs.get(peer_id).copied() {
            let mut found = cursor == commit_id;
            while !found {
                match self.commits.get(&cursor) {
                    Some(c) => match c.parents.first() {
                        Some(p) => {
                            cursor = *p;
                            if cursor == commit_id {
                                found = true;
                            }
                        }
                        None => break,
                    },
                    None => break,
                }
            }
            if !found {
                return Err(ShadowError::NotInPeerHistory {
                    commit_id,
                    peer_id: peer_id.to_string(),
                });
            }
        }
        let prev = self.refs.insert(peer_id.to_string(), commit_id);
        Ok(prev.unwrap_or(commit_id))
    }

    /// Number of commits in the store (across all peers).
    pub fn commit_count(&self) -> usize {
        self.commits.len()
    }

    /// Number of writers (peers with at least one commit).
    pub fn peer_count(&self) -> usize {
        self.refs.len()
    }

    /// Borrow a commit by id (for inspection by tools/tests).
    pub fn get_commit(&self, id: &[u8; 32]) -> Option<&ShadowCommit> {
        self.commits.get(id)
    }
}

impl Default for ShadowRefStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a 32-byte content hash for a shadow commit.
///
/// Combines four 64-bit SipHash lanes (each with a distinct salt mixed into
/// the hasher state before the payload) into a 256-bit digest. The salts
/// ensure the four lanes don't trivially collide with each other.
fn hash_commit(
    peer_id: &str,
    parents: &[[u8; 32]],
    timestamp_ms: u64,
    state_vector: &[u8],
) -> [u8; 32] {
    let salts: [u64; 4] = [
        0x5173_6861_646f_7753, // "ShadowS"
        0x6861_646f_7753_3147, // "owS1Gha"
        0x646f_7753_3247_6861, // "doS2Gha"
        0x6f77_5333_4768_6164, // "owS3Ghad"
    ];
    let mut out = [0u8; 32];
    for (i, salt) in salts.iter().enumerate() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        salt.hash(&mut h);
        peer_id.hash(&mut h);
        parents.hash(&mut h);
        timestamp_ms.hash(&mut h);
        state_vector.hash(&mut h);
        let lane = h.finish();
        out[i * 8..(i + 1) * 8].copy_from_slice(&lane.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_commit_is_deterministic() {
        let a = hash_commit("peer-a", &[], 1234, &[1, 2, 3]);
        let b = hash_commit("peer-a", &[], 1234, &[1, 2, 3]);
        assert_eq!(a, b);
    }

    #[test]
    fn hash_commit_differs_on_peer() {
        let a = hash_commit("peer-a", &[], 1234, &[1, 2, 3]);
        let b = hash_commit("peer-b", &[], 1234, &[1, 2, 3]);
        assert_ne!(a, b);
    }

    #[test]
    fn hash_commit_differs_on_state_vector() {
        let a = hash_commit("peer-a", &[], 1234, &[1, 2, 3]);
        let b = hash_commit("peer-a", &[], 1234, &[1, 2, 4]);
        assert_ne!(a, b);
    }
}
