//! `#[wasm_bindgen]` wrappers around the offline op-queue + lineage epoch
//! tracker (issue #4).
//!
//! Browser consumers on `wasm32-unknown-unknown` use these JS classes directly:
//!
//! ```js
//! import { WasmOfflineOpQueue, WasmEpochTracker } from "grafeo-loro";
//!
//! const queue = new WasmOfflineOpQueue();
//! queue.enqueue(new Uint8Array([1, 2, 3]));
//! console.log(queue.depth);          // 1
//! console.log(queue.bytesUsed);      // 3
//! console.log(queue.capBytes);       // 10485760 (10 MB default)
//! const ops = queue.drain();          // [Uint8Array(3)]
//! console.log(queue.isEmpty);        // true
//!
//! const epoch = new WasmEpochTracker();
//! console.log(epoch.current);        // 0
//! epoch.bump();                      // returns 1
//! epoch.checkMatch(1);               // ok
//! epoch.checkMatch(2);               // throws { code: 1013, message: "..." }
//! ```
//!
//! Gated by `feature = "wasm"` + `target_family = "wasm"`.

#![cfg(feature = "wasm")]
#![cfg(target_family = "wasm")]

use crate::bridge::queue::{EpochMismatchError, EpochTracker, OfflineOpQueue};
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

/// Error code for `EpochMismatchError` (issue #4 — new in 0.4.0).
///
/// Codes 1_001–1_012 are taken by `GrafeoLoroError` variants (see
/// `src/wasm/mod.rs` code table). `EpochMismatchError` is a separate error
/// type returned by `EpochTracker::check_match`; we assign it code 1_013
/// to leave the existing 1_001–1_012 range untouched.
const EPOCH_MISMATCH_CODE: u32 = 1013;

/// Wrap an `EpochMismatchError` into a JS object:
/// `{ code: 1013, message: "...", local: <u64>, remote: <u64> }`.
///
/// The `code` field is the stable numeric branch key (JS callers should
/// pattern-match on this — `message` is for humans and may change between
/// versions). The `local`/`remote` fields let the JS side surface a
/// "your cache is at epoch X, server is at epoch Y — wipe required"
/// message without re-parsing the human string.
fn epoch_mismatch_to_jsvalue(err: EpochMismatchError) -> JsValue {
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(
        &obj,
        &JsValue::from("code"),
        &JsValue::from(EPOCH_MISMATCH_CODE),
    )
    .expect("Reflect::set must succeed on a fresh Object");
    js_sys::Reflect::set(
        &obj,
        &JsValue::from("message"),
        &JsValue::from_str(&err.to_string()),
    )
    .expect("Reflect::set must succeed on a fresh Object");
    js_sys::Reflect::set(&obj, &JsValue::from("local"), &JsValue::from(err.local))
        .expect("Reflect::set must succeed on a fresh Object");
    js_sys::Reflect::set(
        &obj,
        &JsValue::from("remote"),
        &JsValue::from(err.remote),
    )
    .expect("Reflect::set must succeed on a fresh Object");
    JsValue::from(obj)
}

/// JS-facing wrapper around [`crate::bridge::queue::OfflineOpQueue`].
///
/// Browser consumers use this class to manage a pending offline op-queue
/// without enabling `batcher` (tokio::sync::mpsc) + `grafeo` (native
/// ONNX/ort) + `telemetry` (opentelemetry native) — all WASM-incompatible.
///
/// Issue #4.
#[wasm_bindgen]
pub struct WasmOfflineOpQueue {
    inner: OfflineOpQueue,
}

#[wasm_bindgen]
impl WasmOfflineOpQueue {
    /// Construct a fresh empty queue with the default 10 MB cap.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: OfflineOpQueue::new(),
        }
    }

    /// Construct a fresh empty queue with a custom cap (in bytes).
    #[wasm_bindgen(js_name = withCap)]
    pub fn with_cap(cap_bytes: usize) -> Self {
        Self {
            inner: OfflineOpQueue::with_cap(cap_bytes),
        }
    }

    /// Enqueue a serialized LoroOp. Returns `Err(JsValue)` if adding the
    /// op would exceed the cap — the JS value is a `{ code, message }`
    /// object (code 1008 = `GrafeoLoroError::Bridge`).
    #[wasm_bindgen(js_name = enqueue)]
    pub fn enqueue(&mut self, op_bytes: &[u8]) -> Result<(), JsValue> {
        self.inner
            .enqueue(op_bytes.to_vec())
            .map_err(crate::wasm::js_error)
    }

    /// Drain all queued ops in FIFO order. Returns a JS `Array<Uint8Array>`.
    /// Resets `bytesUsed` to 0 but does NOT reset `retryCount` (call
    /// `resetRetry()` separately after a successful flush).
    #[wasm_bindgen(js_name = drain)]
    pub fn drain(&mut self) -> Vec<Uint8Array> {
        self.inner
            .drain()
            .into_iter()
            .map(|op| Uint8Array::from(&op[..]))
            .collect()
    }

    /// Number of ops currently queued.
    #[wasm_bindgen(js_name = depth, getter)]
    pub fn depth(&self) -> usize {
        self.inner.depth()
    }

    /// Total bytes currently held (sum of `op_bytes.length`).
    #[wasm_bindgen(js_name = bytesUsed, getter)]
    pub fn bytes_used(&self) -> usize {
        self.inner.bytes_used()
    }

    /// Cap in bytes (10 MB default).
    #[wasm_bindgen(js_name = capBytes, getter)]
    pub fn cap_bytes(&self) -> usize {
        self.inner.cap_bytes()
    }

    /// Bump the retry counter; returns the new count. Saturating add.
    #[wasm_bindgen(js_name = retryBump)]
    pub fn retry_bump(&mut self) -> u32 {
        self.inner.retry_bump()
    }

    /// Reset the retry counter to 0.
    #[wasm_bindgen(js_name = resetRetry)]
    pub fn reset_retry(&mut self) {
        self.inner.reset_retry()
    }

    /// Current retry count.
    #[wasm_bindgen(js_name = retryCount, getter)]
    pub fn retry_count(&self) -> u32 {
        self.inner.retry_count()
    }

    /// Whether the queue is empty.
    #[wasm_bindgen(js_name = isEmpty, getter)]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl Default for WasmOfflineOpQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// JS-facing wrapper around [`crate::bridge::queue::EpochTracker`].
///
/// Browser consumers use this class to track lineage epochs for sync
/// handshake validation without going through `SyncEngine` (which is
/// WASM-incompatible).
///
/// Issue #4.
#[wasm_bindgen]
pub struct WasmEpochTracker {
    inner: EpochTracker,
}

#[wasm_bindgen]
impl WasmEpochTracker {
    /// Construct a fresh tracker starting at epoch 0.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: EpochTracker::new(),
        }
    }

    /// Current lineage epoch.
    #[wasm_bindgen(js_name = current, getter)]
    pub fn current(&self) -> u64 {
        self.inner.current()
    }

    /// Check whether the remote's advertised epoch matches the local one.
    /// Returns `Err(JsValue)` on mismatch — the JS value is a
    /// `{ code: 1013, message: "...", local, remote }` object.
    #[wasm_bindgen(js_name = checkMatch)]
    pub fn check_match(&self, remote_epoch: u64) -> Result<(), JsValue> {
        self.inner
            .check_match(remote_epoch)
            .map_err(epoch_mismatch_to_jsvalue)
    }

    /// Atomically bump the epoch; returns the new value.
    #[wasm_bindgen(js_name = bump)]
    pub fn bump(&self) -> u64 {
        self.inner.bump()
    }

    /// Semantic alias for `bump()` — bumps the epoch to signal a cache wipe
    /// event (server reset, manual reset, lineage break). Returns the new
    /// epoch.
    #[wasm_bindgen(js_name = wipe)]
    pub fn wipe(&self) -> u64 {
        self.inner.wipe()
    }
}

impl Default for WasmEpochTracker {
    fn default() -> Self {
        Self::new()
    }
}
