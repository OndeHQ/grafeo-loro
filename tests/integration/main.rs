//! Integration test crate for the `grafeo-loro` bridge.
//!
//! Submodules:
//! - [`sync_echo`]: echo loop prevention and bidirectional sync tests.
//! - [`tree_move_concurrency`]: 3-peer concurrent tree-move convergence (Phase 2 Task 2).

mod sync_echo;
mod tree_move_concurrency;
