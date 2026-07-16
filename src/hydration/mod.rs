//! Hydration: cold-boot rebuild of Grafeo indexes from a Loro snapshot.
//!
//! Issue #1 item 3 compliance:
//! - `parallel_hydrate_grafeo` is gated by the `parallel` feature (pulls `rayon`).
//! - The serial fallback `hydrate_grafeo` is the default implementation (always
//!   available when `grafeo` is on, no rayon dep).
//! - WASM builds use the serial path (`parallel` is off in WASM by default).

#[cfg(feature = "parallel")]
pub mod parallel;
pub mod serial;
pub mod vector;

#[cfg(feature = "parallel")]
pub use parallel::parallel_hydrate_grafeo;
pub use serial::hydrate_grafeo;
pub use vector::VectorOffloadManager;
