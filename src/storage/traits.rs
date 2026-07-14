//! Storage backend trait — issue #1 item 9.
//!
//! Browser-friendly: `&self` not `&mut self` (matters for `Rc<RefCell<>>`
//! wrapping in WASM), `async_trait` (works in WASM via
//! `wasm-bindgen-futures`), no `tokio::fs` in default impls.
//!
//! Onde will write its own IDB/OPFS adapter implementing this trait. We do
//! NOT ship browser storage — Onde owns it (reciprocal commitment per
//! issue #1).

use async_trait::async_trait;

/// Persistent storage backend for cold-snapshot persistence.
///
/// All methods take `&self` (not `&mut self`) so the trait can be wrapped
/// in `Rc<RefCell<dyn StorageBackend>>` or `Arc<dyn StorageBackend>` in
/// both native and WASM environments. Methods are `async` via
/// [`async_trait`] — works in WASM via `wasm-bindgen-futures`.
///
/// # Implementor notes
///
/// - **No `tokio::fs`** in default impls — that would pull `tokio`'s
///   runtime, which is unavailable in browser WASM. Use
///   `wasm-bindgen-futures` + `web-sys` for IDB/OPFS in browser impls.
/// - **No `Send` bound required** — WASM futures are single-threaded.
///   Native impls that need `Send` can add it locally; the trait itself
///   stays WASM-compatible.
/// - **`'static` bound** retained so the trait object can be stored in
///   `Arc<dyn StorageBackend>` (the production GrafeoLoroApp shape).
///
/// # Errors
///
/// Implementations return `std::io::Error` to keep the trait signature
/// simple. Wrapping backend-specific errors into `io::Error` via
/// `io::Error::new(io::ErrorKind::Other, ...)` is the expected pattern.
#[async_trait(?Send)]
pub trait StorageBackend: 'static {
    /// Load the bytes previously saved under `key`.
    ///
    /// Returns `io::ErrorKind::NotFound` when the key is absent — this is
    /// the "fresh graph" cold-boot signal consumed by `GrafeoLoroApp::hydrate`.
    async fn load(&self, key: &str) -> std::result::Result<Vec<u8>, std::io::Error>;

    /// Persist `bytes` under `key`. Subsequent `load(key)` MUST return the
    /// same bytes (modulo in-place mutation by a concurrent writer — backends
    /// with weaker guarantees should document that).
    async fn save(&self, key: &str, bytes: Vec<u8>) -> std::result::Result<(), std::io::Error>;

    /// Enumerate all stored keys beginning with `prefix`. Used by
    /// `GrafeoLoroApp::hydrate` to enumerate delta snapshots during
    /// cold-boot replay.
    async fn list(&self, prefix: &str) -> std::result::Result<Vec<String>, std::io::Error>;

    /// Remove the entry at `key`. Idempotent: deleting a missing key is OK.
    async fn delete(&self, key: &str) -> std::result::Result<(), std::io::Error>;
}
