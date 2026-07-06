//! Unit-test crate for `grafeo-loro` schema-level roundtrips.
//!
//! Submodules:
//! - [`schema_roundtrip`]: `lorosurgeon` derive roundtrip scaffolds (Phase 2 Task 1).
//! - [`tree_move`]: `sync_tree_move_to_grafeo` reparenting scaffolds (Phase 2 Task 2).
//! - [`vertex_builder`]: `app::VertexBuilder` fluent API scaffolds (Phase 2 Task 3).
//! - [`compression`]: `compression::wrapper` codec roundtrip scaffolds (Phase 3 Task 1).
//! - [`parallel_hydrate`]: `hydration::parallel::parallel_hydrate_grafeo` scaffolds (Phase 3 Task 2).
//! - [`vector_embedding`]: `hydration::vector::generate_local_embedding` stub scaffolds (Phase 3 Task 3).
//! - [`vector_offload`]: `hydration::vector::VectorOffloadManager::{new, handle_text_update}` scaffolds (Phase 3 Task 4).

mod compression;
mod parallel_hydrate;
mod schema_roundtrip;
mod tree_move;
mod vector_embedding;
mod vector_offload;
mod vertex_builder;
