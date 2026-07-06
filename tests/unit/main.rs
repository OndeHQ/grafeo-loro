//! Unit-test crate for `grafeo-loro` schema-level roundtrips.
//!
//! Submodules:
//! - [`schema_roundtrip`]: `lorosurgeon` derive roundtrip scaffolds (Phase 2 Task 1).
//! - [`tree_move`]: `sync_tree_move_to_grafeo` reparenting scaffolds (Phase 2 Task 2).
//! - [`vertex_builder`]: `app::VertexBuilder` fluent API scaffolds (Phase 2 Task 3).

mod schema_roundtrip;
mod tree_move;
mod vertex_builder;
