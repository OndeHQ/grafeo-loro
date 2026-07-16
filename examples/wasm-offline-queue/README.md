# WASM offline-queue example

End-to-end browser consumer demo for [grafeo-loro issue #4](https://github.com/OndeHQ/grafeo-loro/issues/4).

## Build

```sh
cd examples/wasm-offline-queue
wasm-pack build --target web --release
```

## Run

```sh
# Serve the directory (index.html needs a real HTTP server due to ES module CORS)
python3 -m http.server 8080
# Then open http://localhost:8080 in a browser
```

Click "Run demo" to exercise the full `WasmOfflineOpQueue` + `WasmEpochTracker` API.

## What it demonstrates

1. Constructing a `WasmOfflineOpQueue` with the default 10 MB cap.
2. Enqueueing 3 serialized LoroOps (simulated bytes).
3. Bumping the retry counter (simulating a failed flush).
4. Draining the queue on reconnect (FIFO order).
5. Resetting the retry counter after a successful flush.
6. Constructing a `WasmEpochTracker` at epoch 0.
7. First sync handshake — match (remote advertises epoch 0).
8. Server reset detection — mismatch (remote now advertises epoch 1).
9. Wiping local cache + bumping epoch via `wipe()`.
10. Re-handshake — match.

## Verifying the fail-loud feature stubs

The example's `Cargo.toml` declares 3 intentionally-broken feature stubs
(`merge`, `awareness`, `persistence`) that forward to grafeo-loro's
fail-loud stubs. To verify the `compile_error!` fires from a downstream
consumer perspective:

```sh
cd examples/wasm-offline-queue
# Uncomment the `merge = ["grafeo-loro/merge"]` line under [features] in Cargo.toml,
# then:
cargo build 2>&1 | head -10
# Expected: error: grafeo-loro has no `merge` feature. To merge two LoroDocs...
```
