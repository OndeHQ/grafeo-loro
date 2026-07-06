//! Unit-test crate for `grafeo-loro` schema-level roundtrips.
//!
//! Submodules:
//! - [`schema_roundtrip`]: `lorosurgeon` derive roundtrip scaffolds (Phase 2 Task 1).
//! - [`tree_move`]: `sync_tree_move_to_grafeo` reparenting scaffolds (Phase 2 Task 2).
//! - [`vertex_builder`]: `app::VertexBuilder` fluent API scaffolds (Phase 2 Task 3).
//! - [`compression`]: `compression::wrapper` codec roundtrip scaffolds (Phase 3 Task 1).
//! - [`compression_payload`]: `CompressedPayload` on-wire format roundtrips (Phase 4 P4-L3).
//! - [`parallel_hydrate`]: `hydration::parallel::parallel_hydrate_grafeo` scaffolds (Phase 3 Task 2).
//! - [`vector_embedding`]: `hydration::vector::generate_local_embedding` stub scaffolds (Phase 3 Task 3).
//! - [`vector_offload`]: `hydration::vector::VectorOffloadManager::{new, handle_text_update}` scaffolds (Phase 3 Task 4).
//! - [`hydrate_checkpoint`]: `GrafeoLoroApp::hydrate`/`checkpoint` cold-boot round-trip (Phase 4 P4-L3).
//! - [`builder_validation`]: `GrafeoLoroAppBuilder::build` config-validation rejection paths (Phase 4 P4-L3).

mod compression;
mod compression_payload;
mod parallel_hydrate;
mod schema_roundtrip;
mod tree_move;
mod vector_embedding;
mod vector_offload;
mod vertex_builder;
mod builder_validation;
mod hydrate_checkpoint;
