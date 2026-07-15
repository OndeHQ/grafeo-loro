# Changelog

All notable changes to grafeo-loro are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Pre-1.0 versions may break the API in minor bumps.

## [Unreleased]

## [0.2.0] — 2026-07-15

### Issue #1 compliance — 100% complete

This release makes grafeo-loro dependable as a native Cargo dep from
`wasm32-unknown-unknown` (Onde's WASM bundle) and native server builds.
Every hard blocker, soft blocker, and nice-to-have in
[issue #1](https://github.com/OndeHQ/grafeo-loro/issues/1) is resolved.

### Added — features (issue #1 item 7)

- **`default = []`** — a fresh `cargo add grafeo-loro` now pulls zero
  heavy native deps. The user opts in explicitly.
- New features: `bridge`, `batcher`, `compression`, `tree`, `storage`,
  `grafeo`, `onnx`, `webrtc`, `telemetry`, `wasm`, `parallel`, `serde`,
  `full`.
- Recommended Onde feature set: `["bridge", "batcher", "compression", "tree"]`
  for native, `["bridge", "tree", "wasm"]` for WASM.
- CI matrix documented in `Cargo.toml` comments: `--no-default-features`,
  `--features bridge`, `--features bridge,batcher,compression,tree`,
  `--features full`, plus wasm32 targets.

### Added — WASM target support (issue #1 items 1, 3)

- `cargo build --target wasm32-unknown-unknown --no-default-features
  --features bridge,tree,wasm` succeeds.
- `parallel_hydrate_grafeo` gated by `parallel` feature (default-off).
- New serial `hydrate_grafeo` is the default impl — WASM-safe, no rayon dep.

### Added — trait-abstracted runtime (issue #1 item 2)

- New `Mailbox<T>` trait (`async_trait(?Send)` — WASM-compatible).
- `TokioMailbox<T>` impl gated by `batcher` feature.
- WASM users provide their own impl (e.g. `web-sys::MessageChannel`).

### Added — LoroDoc ownership API (issue #1 item 4)

- `GrafeoLoroApp::doc() -> RwLockReadGuard<LoroDoc>` — shared borrow.
- `GrafeoLoroApp::subscribe<F>(handler) -> loro::Subscription`.
- Lifecycle contract documented: ownership, drop order, explicit close,
  snapshot trigger.
- Multiple `subscribe()` calls coexist (no exclusive `take_event_handler`).

### Added — Loro version alignment (issue #1 item 5)

- `loro` pinned to `1.13` (matches Onde's current version).
- Long-term: loosen to `>=1.13, <2.0` and document MSRV (1.80+).

### Added — FFI hot-path API (issue #1 item 6)

- `NodeOp` `#[repr(C)]` using `&str` not `String` (zero alloc on hot path).
- `NodeValue` C-friendly enum.
- `apply_node_batch(&[NodeOp])` — SAB-backed bulk apply.
- `apply_loro_op_bytes(&[u8])` — bincode-only entry point (no serde_json).
- Documented: hot-path-safe vs admin-only APIs.

### Added — tree-as-graph adapter (issue #1 item 8)

- New `tree_adapter` module (feature: `tree`).
- `TreeAdapter` operating over `BridgeMaps` (no direct grafeo calls).
- `TreeNode` view with `parent` / `children`.
- `parent()`, `children()`, `descendants()` (DFS), `ancestors()` (root-last).
- `create_child_op`, `move_op`, `indent_op`, `outdent_op` — return `LoroOp`.
- Documented equivalence: `node.parent()` (old Onde) ≡ `tree::parent(node)`.

### Added — browser-friendly storage trait (issue #1 item 9)

- `StorageBackend` now takes `&self` not `&mut self` (Rc<RefCell<>>-friendly).
- `async_trait(?Send)` — works in WASM via `wasm-bindgen-futures`.
- No `tokio::fs` in default impls.
- New `InMemoryStorage` reference impl for unit tests + examples + ephemeral
  graphs (publicly exported under the `storage` feature).

### Added — JsValue error bridge (issue #1 item 12)

- `impl From<GrafeoLoroError> for JsValue` (auto-convert in `#[wasm_bindgen]`).
- `js_error(err) -> JsValue` wrapper producing `{code, message}` JS object.
- `error_code(err) -> u32` stable numeric mapping (codes 1001–1012).
- `init_panic_hook()` `#[wasm_bindgen]` prelude.
- `console_error_panic_hook` dep added (optional, gated by `wasm` feature).

### Added — package metadata (issue #1 item 13)

- MSRV: Rust 1.80+ (declared in `Cargo.toml`).
- `documentation`, `readme`, `keywords`, `categories` fields added.
- Tagged release: `v0.2.0`.

### Changed — BREAKING (no backward compatibility per issue mandate)

- `Cargo.toml`: every dep except `loro`/`lorosurgeon`/`parking_lot`/
  `async-trait`/`thiserror`/`tracing` is now `optional = true`. Existing
  users MUST add `--features full` (or pick specific features) to keep
  the old behavior.
- `StorageBackend` trait: methods now `&self` (was `&self` before too, but
  bound changed from `Send + Sync` to `?Send`). Existing impls MUST update
  `#[async_trait]` to `#[async_trait(?Send)]`.
- `GrafeoLoroError`: variants `Loro`, `Grafeo`, `Hydrate`, `Compression`
  are now cfg-gated by their feature. Existing match arms on these
  variants MUST be feature-gated too.
- `apply_loro_op` now requires `grafeo` feature (was always-on).
- `parallel_hydrate_grafeo` now requires `parallel` feature (was always-on).
  Use `hydrate_grafeo` (serial) for the default path.
- `BridgeMaps.node_id_map` / `node_key_map` / `edge_id_map` / `edge_key_map`
  now use `crate::types::ids::{NodeId, EdgeId}` (which are `grafeo::{NodeId,
  EdgeId}` re-exports when `grafeo` is on, local `u64` newtypes when off).
- `InMemoryStorage` is now publicly exported (previously `#[cfg(test)]`
  only). External users may now depend on it for examples / ephemeral
  graphs / unit-test scaffolding; production use is still discouraged
  (use a real backend for durability).

### Removed

- Direct `tokio` dep when `batcher` is off — WASM users no longer pull tokio.
- Direct `rayon` dep when `parallel` is off — WASM users no longer pull rayon.
- Direct `grafeo` / `grafeo-common` / `grafeo-engine` deps when `grafeo`
  is off — useful for bridge-only / tree-only WASM builds.

## [0.1.0] — 2026-07-14

Initial public release. Single-crate, all features always-on. No WASM
target support. No feature gates.

[Unreleased]: https://github.com/OndeHQ/grafeo-loro/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/OndeHQ/grafeo-loro/releases/tag/v0.2.0
[0.1.0]: https://github.com/OndeHQ/grafeo-loro/releases/tag/v0.1.0
