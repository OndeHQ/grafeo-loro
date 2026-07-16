# Issue #4 Compliance Tracker

Branch: `fix/issue-4-wasm-queue`
Target version: `0.4.0`
Issue: https://github.com/OndeHQ/grafeo-loro/issues/4

## Sub-tasks

- [ ] **Q** — Factor `OfflineOpQueue` + `LineageEpoch` + `EpochMismatchError` out of `sync_engine.rs` into new `src/bridge/queue.rs`. Add standalone `EpochTracker`. Gate only by `bridge`. Update re-exports in `src/bridge/mod.rs` + `src/lib.rs`. Update `SyncEngine` to delegate.
- [ ] **W** — Add `src/wasm/queue.rs` with `#[wasm_bindgen]` `WasmOfflineOpQueue` + `WasmEpochTracker` classes. Wire into `src/wasm/mod.rs`.
- [ ] **D** — Update `src/lib.rs` rustdoc with feature→sub-issue mapping table + "no `merge`/`awareness`/`persistence` feature" note + WASM binary size note. Update `CHANGELOG.md` `[0.4.0]` section. Update `README.md` with WASM consumer usage. Add fail-loud `compile_error!` guards in `Cargo.toml`-discoverable location for invented feature names.
- [ ] **T** — Add `tests/queue_native.rs` + `tests/queue_wasm.rs` (wasm-bindgen-test). Add `examples/wasm-offline-queue/`. Write `scripts/wasm_blackbox_qa.sh`.

## Verification (Orchestrator runs)

- [ ] `cargo build --no-default-features --features bridge` ✅
- [ ] `cargo build --target wasm32-unknown-unknown --no-default-features --features bridge,tree,wasm` ✅
- [ ] `cargo test --features full` ✅
- [ ] `cargo clippy --features full -- -D warnings` ✅
- [ ] `wasm-pack build --target web -- --features bridge,tree,wasm` ✅
- [ ] `node scripts/wasm_blackbox_qa.sh` ✅
- [ ] `cargo publish --dry-run` ✅

## Release

- [ ] Bump `Cargo.toml` 0.3.0 → 0.4.0
- [ ] Push branch + open PR
- [ ] Tag `v0.4.0` + GitHub Release
- [ ] `cargo publish` to crates.io
- [ ] Close issue #4
