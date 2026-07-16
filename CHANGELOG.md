# Changelog

All notable changes to grafeo-loro are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Pre-1.0 versions may break the API in minor bumps.

## [0.4.0] — 2026-07-16

### Issue #4 compliance — `OfflineOpQueue` + lineage epoch reachable from WASM

This release fixes [issue #4](https://github.com/OndeHQ/grafeo-loro/issues/4):
the `OfflineOpQueue` + `LineageEpoch` + `EpochMismatchError` types previously
lived in `src/bridge/sync_engine.rs`, gated behind `batcher + grafeo +
telemetry` — all three WASM-incompatible (tokio::sync::mpsc, native ONNX/ort,
opentelemetry native). Browser consumers on `wasm32-unknown-unknown` could
not name these types and had to re-implement the queue + epoch tracker
themselves.

### Added — `bridge::queue` module (WASM-accessible)

- New `src/bridge/queue.rs` module gated only by `feature = "bridge"`. No
  tokio, no grafeo, no telemetry deps. Compiles cleanly on
  `wasm32-unknown-unknown`.
- `OfflineOpQueue`, `LineageEpoch`, `EpochMismatchError` factored out of
  `sync_engine.rs` (no API changes — same methods, same semantics).
- **New** `EpochTracker` standalone type — pure `AtomicU64`, WASM-accessible
  lineage epoch tracker for browser consumers who don't want to go through
  `SyncEngine` at all. Methods: `current`, `check_match`, `bump`, `wipe`.
- Crate-root re-exports: `grafeo_loro::{OfflineOpQueue, EpochTracker,
  LineageEpoch, EpochMismatchError}` (gated by `bridge`).

### Added — `#[wasm_bindgen]` JS classes

- New `src/wasm/queue.rs` with `WasmOfflineOpQueue` + `WasmEpochTracker`
  `#[wasm_bindgen]` classes. JS API uses camelCase per wasm-bindgen
  convention.
- `WasmOfflineOpQueue`: `new()`, `withCap(bytes)`, `enqueue(bytes)`,
  `drain() -> Array<Uint8Array>`, `depth` (getter), `bytesUsed` (getter),
  `capBytes` (getter), `retryBump()`, `resetRetry()`, `retryCount` (getter),
  `isEmpty` (getter).
- `WasmEpochTracker`: `new()`, `current` (getter), `checkMatch(remote)`
  (throws on mismatch), `bump()`, `wipe()`.
- `EpochMismatchError` gets stable error code `1_013` (the existing
  `1_001`–`1_012` range is taken by `GrafeoLoroError` variants).

### Changed — `SyncEngine` delegates to factored-out types

- `SyncEngine::lineage_epoch: Arc<AtomicU64>` field replaced with
  `epoch_tracker: Arc<EpochTracker>` (shared via `Arc` so `EpochTracker::clone`
  is cheap).
- `SyncEngine::{lineage_epoch, check_epoch_match, wipe_cache}` delegate to
  the factored-out `EpochTracker`.
- `SyncEngine::{enqueue_offline_op, drain_offline_queue, offline_queue_depth,
  offline_queue_bytes, offline_queue_cap, offline_retry_bump,
  reset_offline_retry, offline_retry_count}` continue to delegate to the
  shared `Arc<Mutex<OfflineOpQueue>>` field — no API changes.

### Added — fail-loud on invented feature names (issue #4 secondary #2)

- Empty Cargo feature stubs `merge = []`, `awareness = []`,
  `persistence = []` declared.
- `compile_error!` guards in `src/lib.rs` fire with a helpful message
  pointing at the correct alternative (`doc.import()`, `presence` module,
  `storage` feature respectively) when any of these stubs is enabled.
- **Breaking**: consumers who relied on the silent no-op behavior of
  `--features merge` (etc.) in older Cargo will now get a hard compile
  error.

### Added — docs (issue #4 secondary #1 + #3)

- Crate-level rustdoc updated with:
  - Feature→sub-issue mapping table (issue #3's 10 sub-issues → feature gates)
  - "No `merge`/`awareness`/`persistence` feature" section with alternatives
  - WASM binary size note: ~2.26 MB raw, ~1.8 MB after `wasm-opt -Oz` for
    the full WASM-safe feature set; ~800 KB for the minimal `bridge,tree,wasm`
    smoke set.
- `README.md` "WASM Browser Consumer Usage" section with JS example.

### Added — tests + QA (issue #4 verification)

- `tests/queue_native.rs`: native tests for `OfflineOpQueue` + `EpochTracker`
  (cap enforcement, FIFO drain, retry hooks, epoch bump/wipe/check_match).
- `tests/queue_wasm.rs`: wasm-bindgen-test runner for the same tests on
  `wasm32-unknown-unknown`.
- `examples/wasm-offline-queue/`: end-to-end browser consumer example
  (HTML + JS + Rust wasm-bindgen wrapper showing the full queue + epoch
  lifecycle).
- `scripts/wasm_blackbox_qa.sh`: wasm-pack build + node smoke test that
  imports the `.wasm` and exercises the full JS API.

### Verification matrix (all green)

| Command | Result |
|---------|--------|
| `cargo build --no-default-features --features bridge` | ✅ queue reachable |
| `cargo build --target wasm32-unknown-unknown --no-default-features --features bridge,tree,wasm` | ✅ |
| `cargo build --target wasm32-unknown-unknown --no-default-features --features bridge,tree,compression,wasm,fts,sab,shadow,observability,serde` | ✅ full WASM-safe set |
| `cargo test --features full` | ✅ all existing + new tests pass |
| `cargo test --no-default-features --features bridge --test queue_native` | ✅ |
| `cargo test --target wasm32-unknown-unknown --no-default-features --features bridge,tree,wasm --test queue_wasm` | ✅ |
| `cargo clippy --features full -- -D warnings` | ✅ zero warnings |
| `cargo build --features merge` | ✅ fails with `compile_error!` (intentional) |
| `wasm-pack build --target web -- --features bridge,tree,wasm` | ✅ produces `pkg/` with JS classes |
| `node scripts/wasm_blackbox_qa.sh` | ✅ JS smoke test passes |

### Breaking changes

- `SyncEngine::lineage_epoch` field renamed to `epoch_tracker` (was
  `pub(crate)`, so no external API impact, but in-crate callers updated).
- `--features merge` / `--features awareness` / `--features persistence`
  now produce a hard `compile_error!` (was silent no-op in older Cargo).

### Migration guide

If you previously invented `merge` / `awareness` / `persistence` features:

```diff
# Cargo.toml
-grafeo-loro = { version = "0.3", features = ["bridge", "merge", "awareness"] }
+grafeo-loro = { version = "0.4", features = ["bridge"] }
```

Then:
- For `merge`: call `doc.import(other.export(loro::ExportFormat::Snapshot))`
  directly on the `LoroDoc` handle from `GrafeoLoroApp::doc()`.
- For `awareness`: use the `presence` module (always available with `bridge`).
- For `persistence`: enable `storage` and implement `StorageBackend`.

## [0.3.0] — 2026-07-15

### Issue #3 compliance — Browser WASM consumers Support

This release shipped [issue #3](https://github.com/OndeHQ/grafeo-loro/issues/3):
full browser WASM consumer support. The 10 sub-issues covered WASM target
compile + binary size, trait-abstracted runtime (Mailbox), LoroDoc ownership
API, merge semantics, offline op-queue + lineage epoch, FTS + SAB + shadow
commits, graph invariants, presence (ephemeral overlay), storage backend
trait, and observability hooks.

### Added — features (issue #3 sub-issues 6, 10)

- New features: `fts` (full-text-search inverted index), `sab` (SharedArrayBuffer
  layout writer), `shadow` (Git DAG shadow commits), `observability` (queue
  state, fault injection, invariant checks). All WASM-safe.

### Added — compression decoupled from grafeo (issue #3 sub-issue 1)

- `compression` feature now uses pure-Rust `lz4_flex` + `brotli` + `flate2`
  (drops `zstd-sys` C dep that broke WASM builds).
- `LoroDocCompressionExt` trait available whenever `compression` is on,
  independent of `grafeo`.

### Added — `GrafeoLoroError::Loro` always-on (issue #3 sub-issue 1)

- The `Loro(_)` variant is no longer gated behind `grafeo`; it is always
  available since `loro` itself is a non-optional dep.

### Changed — FFI `Result` collision fix (issue #3 sub-issues 2 + 4)

- `src/ffi/mod.rs` switched to `std::result::Result` for FFI entry points
  to avoid the type-alias collision with `crate::error::Result` under the
  `grafeo` feature.

### Added — FFI batcher/origin/conflict entry points (issue #3 sub-issues 2 + 4)

- New `apply_node_batch` + `apply_loro_op_bytes` (bincode-only) hot paths.
- `ConflictDetected` + `semantic_text_merge` for sub-issue 4 conflict
  resolution.

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

[Unreleased]: https://github.com/OndeHQ/grafeo-loro/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/OndeHQ/grafeo-loro/releases/tag/v0.4.0
[0.3.0]: https://github.com/OndeHQ/grafeo-loro/releases/tag/v0.3.0
[0.2.0]: https://github.com/OndeHQ/grafeo-loro/releases/tag/v0.2.0
[0.1.0]: https://github.com/OndeHQ/grafeo-loro/releases/tag/v0.1.0
