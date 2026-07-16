//! Browser consumer example for issue #4.
//!
//! Demonstrates the WASM-accessible `WasmOfflineOpQueue` + `WasmEpochTracker`
//! JS classes. Build with:
//!
//! ```sh
//! cd examples/wasm-offline-queue
//! wasm-pack build --target web --release
//! ```
//!
//! Then open `index.html` in a browser (or serve via `python -m http.server`).
//!
//! The example simulates a browser consumer's offline op-queue + sync
//! handshake flow:
//! 1. User writes ops while offline — they're enqueued in `WasmOfflineOpQueue`
//! 2. On reconnect, drain the queue + flush to the remote
//! 3. Sync handshake validates lineage epoch via `WasmEpochTracker`
//!
//! This crate is **WASM-only** — `cargo check` on the host (e.g. native
//! x86_64) compiles an empty crate because the upstream
//! `grafeo_loro::WasmOfflineOpQueue` / `WasmEpochTracker` re-exports are
//! gated by `target_family = "wasm"`. Use `wasm-pack build --target web` to
//! produce the actual JS bindings; `cargo check` is a smoke check only.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

// Re-export the `#[wasm_bindgen]` JS classes so they appear in this crate's
// `pkg/*.d.ts` output. wasm-bindgen honors `pub use` of `#[wasm_bindgen]`
// types from upstream crates and emits the corresponding JS class bindings
// in the downstream crate's pkg output. This also brings them into scope
// for the `run_demo()` function below.
pub use grafeo_loro::{WasmEpochTracker, WasmOfflineOpQueue};

/// JS-facing demo entry point. Called from `index.html`'s button click.
/// Returns the demo log as a string for the JS side to render.
#[wasm_bindgen]
pub fn run_demo() -> String {
    let mut log = String::new();
    log.push_str("=== grafeo-loro issue #4 demo ===\n\n");

    // ---- Offline op-queue ----
    log.push_str("[1] Constructing WasmOfflineOpQueue with default 10 MB cap...\n");
    let mut queue = WasmOfflineOpQueue::new();
    log.push_str(&format!(
        "    depth={}, bytesUsed={}, capBytes={}, isEmpty={}\n",
        queue.depth(),
        queue.bytes_used(),
        queue.cap_bytes(),
        queue.is_empty(),
    ));

    log.push_str("[2] Enqueueing 3 serialized LoroOps (simulated)...\n");
    let _ = queue.enqueue(&[1, 2, 3]);
    let _ = queue.enqueue(&[4, 5, 6]);
    let _ = queue.enqueue(&[7, 8, 9, 10]);
    log.push_str(&format!(
        "    depth={}, bytesUsed={}, isEmpty={}\n",
        queue.depth(),
        queue.bytes_used(),
        queue.is_empty(),
    ));

    log.push_str("[3] Bumping retry counter (simulating failed flush)...\n");
    let r1 = queue.retry_bump();
    let r2 = queue.retry_bump();
    log.push_str(&format!(
        "    retryBump()={}, retryBump()={}, retryCount={}\n",
        r1, r2, queue.retry_count(),
    ));

    log.push_str("[4] On reconnect: drain + flush (simulated)...\n");
    let drained = queue.drain();
    let drained_len = drained.len();
    log.push_str(&format!(
        "    drained {} ops, depth={}, bytesUsed={}\n",
        drained_len, queue.depth(), queue.bytes_used(),
    ));
    queue.reset_retry();
    log.push_str(&format!(
        "    resetRetry(), retryCount={}\n",
        queue.retry_count(),
    ));

    // ---- Epoch tracker ----
    log.push_str("\n[5] Constructing WasmEpochTracker...\n");
    let epoch = WasmEpochTracker::new();
    log.push_str(&format!("    current={}\n", epoch.current()));

    log.push_str("[6] First sync handshake — remote advertises epoch 0...\n");
    match epoch.check_match(0) {
        Ok(_) => log.push_str("    ✅ match — proceed with sync\n"),
        Err(_) => log.push_str("    ❌ unexpected mismatch\n"),
    }

    log.push_str("[7] Server reset detected — remote now advertises epoch 1...\n");
    match epoch.check_match(1) {
        Ok(_) => log.push_str("    ❌ unexpected match\n"),
        Err(e) => log.push_str(&format!(
            "    ✅ mismatch (expected): {}\n",
            js_error_to_string(&e)
        )),
    }

    log.push_str("[8] Wipe local cache + bump epoch...\n");
    let new_epoch = epoch.wipe();
    log.push_str(&format!(
        "    wipe()={}, current={}\n",
        new_epoch,
        epoch.current(),
    ));

    log.push_str("[9] Re-handshake — remote advertises epoch 1...\n");
    match epoch.check_match(1) {
        Ok(_) => log.push_str("    ✅ match — proceed with sync\n"),
        Err(_) => log.push_str("    ❌ unexpected mismatch\n"),
    }

    log.push_str("\n=== demo complete ===\n");
    log
}

/// Helper to extract the `.message` field from a JS error object.
fn js_error_to_string(e: &JsValue) -> String {
    if let Some(obj) = e.as_ref().dyn_ref::<js_sys::Object>() {
        if let Ok(message) = js_sys::Reflect::get(obj, &JsValue::from("message")) {
            if let Some(s) = message.as_string() {
                return s;
            }
        }
    }
    format!("{:?}", e)
}
