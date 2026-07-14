//! Storage backend trait + in-memory reference implementation.
//!
//! Issue #1 item 9: browser-friendly storage trait.
//! - `&self` not `&mut self` (matters for `Rc<RefCell<>>` wrapping in WASM)
//! - `async_trait` (works in WASM via `wasm-bindgen-futures`)
//! - No `tokio::fs` in default impls
//! - In-memory backend for unit tests

pub mod traits;
#[cfg(test)]
pub mod memory;

pub use traits::StorageBackend;
#[cfg(test)]
pub use memory::InMemoryStorage;
