//! WASM bindings: JsValue error bridge + prelude (issue #1 item 12).
//!
//! Available whenever the `wasm` feature is on. The JsValue glue (and the
//! `#[wasm_bindgen]` panic hook) is further gated by `target_family = "wasm"`
//! so the target-agnostic pieces — in particular `error_code` — can be
//! unit-tested on native without standing up a JS runtime.
//!
//! Provides:
//!
//! - `impl From<GrafeoLoroError> for JsValue` — error auto-conversion for
//!   `#[wasm_bindgen]` functions that return `Result<T, JsValue>` (enables
//!   `?` to just work).
//! - `js_error(err: GrafeoLoroError) -> JsValue` — explicit wrapper that
//!   produces a plain JS object `{ code, message }`.
//! - `error_code(err: &GrafeoLoroError) -> u32` — stable numeric mapping
//!   (target-agnostic; the source of truth for the code table below).
//! - `init_panic_hook()` — `#[wasm_bindgen]` prelude; routes Rust panics to
//!   `console.error` via `console_error_panic_hook`.
//!
//! # Error code mapping
//!
//! Each `GrafeoLoroError` variant maps to a stable numeric code. JS callers
//! should branch on `error.code` (the `.message` field is for humans and may
//! change between versions).
//!
//! | Code | Variant                | Meaning                            | Feature gate         |
//! |------|------------------------|------------------------------------|----------------------|
//! | 1001 | `Loro`                 | Loro CRDT error                    | `grafeo`             |
//! | 1002 | `Grafeo`               | Grafeo DB error                    | `grafeo`             |
//! | 1003 | `StorageIo`            | Storage backend I/O error          | always               |
//! | 1004 | `Compression`          | Compression codec failure          | `compression`        |
//! | 1005 | `ChannelClosed`        | Bridge channel closed              | always               |
//! | 1006 | `Config`               | Configuration invalid              | always               |
//! | 1007 | `UnsupportedLoroType`  | Unsupported LoroValue type         | always               |
//! | 1008 | `Bridge`               | Runtime bridge error               | always               |
//! | 1009 | `Hydrate`              | Cold-boot hydration failure        | `bridge`             |
//! | 1010 | `TreeMoveCreatesCycle` | Tree reparenting would cycle       | always               |
//! | 1011 | `NotYetImplemented`    | Feature not yet implemented        | always               |
//! | 1012 | `InvalidEnvelope`      | Malformed %EPH envelope            | always               |
//!
//! Codes are 1_001–1_012 to leave room below for HTTP-style category codes
//! and above for future variants. Codes are NEVER renumbered once assigned;
//! deprecation removes the variant on the Rust side and the JS code table
//! keeps the slot as "reserved".

#![cfg(feature = "wasm")]

// ============================================================================
// Issue #3 sub-issue 1 — `batcher` + WASM compile_error guard
// ============================================================================
//
// The `batcher` feature pulls `tokio/sync` + `tokio/time`. `tokio::sync::mpsc`
// cannot run in browser WASM (no multi-threaded runtime, no `mio`-backed I/O
// reactor, no system clock). Fail fast with a clear `compile_error!` so the
// user knows to leave `batcher` off and plug in a `Mailbox<T>` impl backed by
// `wasm-bindgen-futures` + `web-sys::MessageChannel` instead.
//
// **Note:** This guard fires only when the `wasm` feature is also enabled
// (because the surrounding `#![cfg(feature = "wasm")]` attribute above skips
// the whole file otherwise). For the case where a user enables `batcher` on a
// `wasm32-unknown-unknown` target WITHOUT the `wasm` feature, a sister guard
// lives in `src/runtime/mod.rs` (which is always compiled when `batcher` is
// on, since `batcher` requires `bridge` and `bridge` exposes `runtime`).
#[cfg(all(feature = "batcher", target_family = "wasm"))]
compile_error!(
    "`batcher` feature pulls tokio::sync::mpsc which cannot run in browser WASM. \
     Use the `Mailbox` trait with a wasm-bindgen-futures impl instead."
);

use crate::error::GrafeoLoroError;

// JsValue glue is only compiled on actual WASM targets. The target-agnostic
// `error_code` (and the test module) compile on any target so the mapping
// can be unit-tested without a JS runtime.
#[cfg(target_family = "wasm")]
use js_sys::Object;
#[cfg(target_family = "wasm")]
use wasm_bindgen::prelude::*;

/// Stable error code for each `GrafeoLoroError` variant. JS callers branch
/// on this; the `.message` field is for humans.
///
/// See the module-level docs for the full code table. Codes are stable
/// across releases (never renumbered); new variants append a new code in
/// the 1_0XX range.
///
/// # Example
///
/// ```
/// # use grafeo_loro::error::GrafeoLoroError;
/// # #[cfg(feature = "wasm")]
/// # {
/// use grafeo_loro::wasm::error_code;
/// let err = GrafeoLoroError::Config("bad port".into());
/// assert_eq!(error_code(&err), 1006);
/// # }
/// ```
pub fn error_code(err: &GrafeoLoroError) -> u32 {
    match err {
        #[cfg(feature = "grafeo")]
        GrafeoLoroError::Loro(_) => 1001,
        #[cfg(feature = "grafeo")]
        GrafeoLoroError::Grafeo(_) => 1002,
        GrafeoLoroError::StorageIo(_) => 1003,
        #[cfg(feature = "compression")]
        GrafeoLoroError::Compression(_) => 1004,
        GrafeoLoroError::ChannelClosed(_) => 1005,
        GrafeoLoroError::Config(_) => 1006,
        GrafeoLoroError::UnsupportedLoroType(_) => 1007,
        GrafeoLoroError::Bridge(_) => 1008,
        #[cfg(feature = "bridge")]
        GrafeoLoroError::Hydrate(_) => 1009,
        GrafeoLoroError::TreeMoveCreatesCycle { .. } => 1010,
        GrafeoLoroError::NotYetImplemented(_) => 1011,
        GrafeoLoroError::InvalidEnvelope(_) => 1012,
    }
}

/// Wrap a `GrafeoLoroError` into a `JsValue` suitable for returning from a
/// `#[wasm_bindgen]` function. The JsValue is a plain JS object with two
/// fields:
///
/// ```js
/// { code: 1008, message: "Bridge error: unknown node key" }
/// ```
///
/// Callers can pattern-match on `code` for stable behavior across versions.
///
/// `Reflect::set` is used (rather than `js_sys::Object::set`) because the
/// latter indexes into the object via `[]`, which can be shadowed by
/// `Object.prototype` properties (e.g. a key named `"constructor"`).
/// `Reflect::set` bypasses the prototype chain. Both calls use `.expect()`
/// — on a fresh `Object::new()` they cannot fail (no prototype interception,
/// no read-only property, sufficient memory).
#[cfg(target_family = "wasm")]
pub fn js_error(err: GrafeoLoroError) -> JsValue {
    let code = error_code(&err);
    let message = err.to_string();
    let obj = Object::new();
    js_sys::Reflect::set(&obj, &JsValue::from("code"), &JsValue::from(code))
        .expect("Reflect::set must succeed on a fresh Object");
    js_sys::Reflect::set(&obj, &JsValue::from("message"), &JsValue::from_str(&message))
        .expect("Reflect::set must succeed on a fresh Object");
    JsValue::from(obj)
}

/// `From` impl so `?` auto-converts in `#[wasm_bindgen]` functions that
/// return `Result<T, JsValue>`.
///
/// # Example
///
/// ```no_run
/// # #[cfg(target_family = "wasm")]
/// # {
/// use grafeo_loro::error::{GrafeoLoroError, Result};
/// use grafeo_loro::wasm; // brings `From<GrafeoLoroError> for JsValue` into scope
/// use wasm_bindgen::prelude::*;
///
/// #[wasm_bindgen]
/// pub fn risky_op() -> Result<(), JsValue> {
///     do_something()?; // GrafeoLoroError → JsValue via this From impl
///     Ok(())
/// }
/// # fn do_something() -> Result<()> { Ok(()) }
/// # }
/// ```
#[cfg(target_family = "wasm")]
impl From<GrafeoLoroError> for JsValue {
    fn from(err: GrafeoLoroError) -> Self {
        js_error(err)
    }
}

/// `#[wasm_bindgen]` prelude — call from your JS-facing init function:
///
/// ```rust,no_run
/// # #[cfg(target_family = "wasm")]
/// # {
/// #[wasm_bindgen]
/// pub fn init() {
///     grafeo_loro::wasm::init_panic_hook();
/// }
/// # }
/// ```
///
/// Installs `console_error_panic_hook::set_once` so uncaught Rust panics
/// surface as `console.error` traces in the browser devtools instead of
/// silently aborting the WASM module.
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ids::NodeId;

    /// Helper: construct a `NodeId` from a `u64` regardless of whether the
    /// `grafeo` feature is on (mirrors the pattern in `tree_adapter::tests`).
    fn nid(n: u64) -> NodeId {
        #[cfg(not(feature = "grafeo"))]
        {
            NodeId(n)
        }
        #[cfg(feature = "grafeo")]
        {
            NodeId::from(n)
        }
    }

    /// Verify every always-available variant maps to its documented code.
    /// This locks the code-to-variant contract for variants that don't need
    /// an extra feature gate.
    #[test]
    fn error_codes_always_available_are_stable() {
        assert_eq!(
            error_code(&GrafeoLoroError::StorageIo(std::io::Error::new(
                std::io::ErrorKind::Other,
                "x"
            ))),
            1003
        );
        assert_eq!(
            error_code(&GrafeoLoroError::ChannelClosed("x".into())),
            1005
        );
        assert_eq!(error_code(&GrafeoLoroError::Config("x".into())), 1006);
        assert_eq!(
            error_code(&GrafeoLoroError::UnsupportedLoroType("x".into())),
            1007
        );
        assert_eq!(error_code(&GrafeoLoroError::Bridge("x".into())), 1008);
        assert_eq!(
            error_code(&GrafeoLoroError::TreeMoveCreatesCycle {
                node_id: nid(1),
                new_parent: nid(2),
            }),
            1010
        );
        assert_eq!(
            error_code(&GrafeoLoroError::NotYetImplemented("x".into())),
            1011
        );
        assert_eq!(
            error_code(&GrafeoLoroError::InvalidEnvelope("x".into())),
            1012
        );
    }

    /// `error_code` is total: iterating the always-available variants must
    /// never panic. (Sanity check that the match arms cover every variant
    /// when feature gates are off.)
    #[test]
    fn error_code_does_not_panic_on_always_available_variants() {
        let _ = error_code(&GrafeoLoroError::Config("a".into()));
        let _ = error_code(&GrafeoLoroError::Bridge("b".into()));
        let _ = error_code(&GrafeoLoroError::ChannelClosed("c".into()));
        let _ = error_code(&GrafeoLoroError::UnsupportedLoroType("d".into()));
        let _ = error_code(&GrafeoLoroError::NotYetImplemented("e".into()));
        let _ = error_code(&GrafeoLoroError::InvalidEnvelope("f".into()));
        let _ = error_code(&GrafeoLoroError::StorageIo(std::io::Error::new(
            std::io::ErrorKind::Other,
            "g",
        )));
        let _ = error_code(&GrafeoLoroError::TreeMoveCreatesCycle {
            node_id: nid(1),
            new_parent: nid(2),
        });
    }

    /// Compression variant → 1004 (only when `compression` is on).
    #[cfg(feature = "compression")]
    #[test]
    fn error_code_compression_is_1004() {
        assert_eq!(
            error_code(&GrafeoLoroError::Compression("lz4 fail".into())),
            1004
        );
    }

    /// Loro variant → 1001 (only when `grafeo` is on; that's the feature
    /// gate on the variant in `error.rs`). `loro::LoroError` is always-on
    /// (loro is not optional), but the variant only exists when `grafeo`
    /// is enabled. We construct via `loro::LoroError::DecodeError`, a
    /// stable variant present since loro 1.0.
    #[cfg(feature = "grafeo")]
    #[test]
    fn error_code_loro_is_1001() {
        let loro_err = loro::LoroError::DecodeError("bad bytes".into());
        let err = GrafeoLoroError::from(loro_err);
        assert_eq!(error_code(&err), 1001);
    }

    /// Hydrate variant → 1009 (only when `bridge` is on, which pulls
    /// `lorosurgeon::error::HydrateError`). Constructed via
    /// `HydrateError::missing` (a stable constructor that's been present
    /// since lorosurgeon 0.2).
    #[cfg(feature = "bridge")]
    #[test]
    fn error_code_hydrate_is_1009() {
        let hydrate_err = lorosurgeon::error::HydrateError::missing("missing_key");
        let err = GrafeoLoroError::from(hydrate_err);
        assert_eq!(error_code(&err), 1009);
    }
}
