# grafeo-loro Benchmarks (issue #1 item 11)

Criterion benchmarks for the four hot paths mandated by issue #1:

| Group         | What it measures                                                |
|---------------|------------------------------------------------------------------|
| `cold_start`  | Construct a `GrafeoLoroApp` with `InMemoryStorage` (builder + `SyncEngine::with_telemetry` + `spawn_all`). |
| `batch_flush` | Push 100 ops through `apply_node_batch` (FFI hot path). |
| `hydration`   | Hydrate 1000 vertices from a Loro doc via serial `hydrate_grafeo` (WASM-safe path, no `parallel`/rayon). |
| `tree_ops`    | Create 100 tree nodes (`LoroOp::UpsertNode`) + move 50 of them (`LoroOp::TreeMove`). |

All four benchmarks use `criterion::black_box` to defeat optimizer
elision and `iter_batched` to split setup cost from measurement.

## Run on native

```bash
# From the crate root:
cargo bench --features full
```

The `[[bench]]` entry in `Cargo.toml` sets `required-features = ["full"]`
so the bench is only compiled when `full` is on. `full` pulls in `grafeo`,
`batcher`, `tree`, `telemetry`, `storage`, `serde`, `compression`,
`parallel` — everything the bench bodies touch.

Criterion writes its HTML + CSV reports under `target/criterion/`. Open
`target/criterion/cold_start/report/index.html` (etc.) in a browser for
the per-benchmark summary.

### Smoke check (compile only, no run)

```bash
cargo build --features full --benches
```

### Run a single group

```bash
cargo bench --features full -- cold_start
cargo bench --features full -- batch_flush
cargo bench --features full -- hydration
cargo bench --features full -- tree_ops
```

## Run on WASM (`wasm32-unknown-unknown`)

> **Known limitation — WASM bench build is blocked at the dependency level.**
>
> As of the current dep tree, `cargo build --target wasm32-unknown-unknown
> --features full --benches` fails before reaching `benches/core.rs`. The
> blockers are:
>
> 1. **`grafeo = "0.5.42"`** is native-only (not ported to
>    `wasm32-unknown-unknown`). The `full` feature requires `grafeo` because
>    every benchmark body (`apply_node_batch`, `hydrate_grafeo`,
>    `LoroOp::TreeMove`) calls into the grafeo execution layer. Tracked
>    upstream — when grafeo gains WASM support, drop the
>    `#![cfg(not(target_family = "wasm"))]` gate at the top of
>    `benches/core.rs` and the bench will compile for WASM unchanged.
> 2. **`ring v0.17`** (transitive dep of `webrtc-rs`, pulled by the `full`
>    feature) requires C compilation via `cc-rs`; the WASM build cannot find
>    `clang` configured for `wasm32-unknown-unknown` in this environment.
> 3. **`zstd-sys`** (pulled by `compression`, included in `full`) is a C
>    library with no pure-Rust encoder. WASM builds MUST NOT enable
>    `compression` (this is a hard constraint documented in the crate's
>    top-level feature matrix).
> 4. **`tokio` dev-dep with `rt-multi-thread`** does not compile on
>    `wasm32-unknown-unknown` (`compile_error!("Only features sync,macros,
>    io-util,rt,time are supported on wasm.")`). The dev-dep is used by
>    existing tests + by `bench_cold_start` (which uses
>    `tokio::runtime::Runtime::block_on` to drive the async
>    `GrafeoLoroApp::builder().build().await`).
>
> `benches/core.rs` is gated by `#![cfg(feature = "full")]` AND
> `#![cfg(not(target_family = "wasm"))]` so the bench file itself never
> causes a WASM build failure — the failures above are all at the
> dependency level (compile-time errors inside `grafeo`, `ring`,
> `zstd-sys`, or `tokio`).
>
> **Native bench is unaffected** — `cargo bench --features full` runs all
> four groups cleanly on the host (verified during this task: cold_start,
> batch_flush, hydration, tree_ops all produce measurements).

### Path to a WASM-runnable bench

Once `grafeo` ships WASM support, the steps are:

1. **Swap the criterion dev-dep line** in `Cargo.toml` to enable the
   `wasm-bindgen` engine:

   ```toml
   criterion = { version = "0.5", default-features = false, features = ["plotters", "cargo_bench_support", "wasm-bindgen"] }
   ```

   (This is already pre-staged as a comment in `Cargo.toml` — just
   uncomment + replace.)

2. **Drop the `target_family = "wasm"` gate** at the top of
   `benches/core.rs` (the `#![cfg(not(target_family = "wasm"))]` line).

3. **Exclude the C-library features** (`compression`, `webrtc`) from the
   WASM bench build — they will never compile to `wasm32-unknown-unknown`.
   The `full` feature currently includes both, so a future WASM-friendly
   `full-wasm` feature gate may be needed (or split `full` into
   `full-native` + `full-wasm`). For now, run with an explicit feature
   list:

   ```bash
   cargo bench --target wasm32-unknown-unknown \
     --features grafeo,batcher,tree,telemetry,storage,serde,parallel,wasm
   ```

   (Assumes `grafeo` supports WASM. If `parallel` (rayon) is also
   unavailable, drop it — the bench uses serial `hydrate_grafeo` only.)

4. **Replace `tokio::runtime::Runtime::block_on`** in `bench_cold_start`
   with a WASM-compatible async driver (`wasm-bindgen-futures::spawn_local`
   + a `Poll`-style loop, or restructure `cold_start` to use
   `GrafeoLoroApp::builder().build()` via a `Future`-driven criterion
   async executor). This is the only bench that needs the runtime —
   `batch_flush`, `hydration`, and `tree_ops` are purely synchronous.

5. **Run under a browser** (criterion's `wasm-bindgen` engine uses
   `web-sys::Performance::now()` for timing, which requires a browser
   context):

   ```bash
   # Build the bench as a wasm-bindgen binary:
   cargo build --target wasm32-unknown-unknown --features grafeo,batcher,tree,telemetry,storage,serde,wasm --benches
   wasm-bindgen --target web target/wasm32-unknown-unknown/debug/core.wasm --out-dir benches/wasm-output
   # Serve benches/wasm-output/ in a browser + open the HTML harness.
   ```

   (Exact browser-harness wiring depends on the runner; see
   [`criterion.rs/examples/wasm`](https://github.com/bheisler/criterion.rs/tree/wasm)
   for the upstream example.)

### Exact `cargo bench` command for M1 Chrome

When the above blockers are resolved, the bench command on M1 Chrome is:

```bash
# Native baseline (same M1 Mac, release):
cargo bench --features full

# WASM (M1 Chrome 120+, release):
cargo bench --target wasm32-unknown-unknown \
  --features grafeo,batcher,tree,telemetry,storage,serde,wasm
```

If the WASM bench needs to be run under a headless browser (e.g. in CI),
use [`criterion-wasm`](https://github.com/bheisler/criterion.rs/tree/wasm)
or wire `wasm-bindgen-test` + a custom runner that invokes
`criterion::BatchSize`'s `iter_batched` directly. The native bench file
is already engine-agnostic; only the `criterion` dev-dep feature flag
differs between native and WASM.

## Published numbers (TODO — M1 Chrome)

> **TODO(issue-1 follow-up):** the table below is a placeholder. Actual
> numbers require running the bench suite on M1 Chrome (native + WASM)
> with a fixed power profile + no background load. Fill in once the
> bench is run end-to-end on the target hardware. The native side can be
> filled in immediately by running `cargo bench --features full` on the
> bench author's machine; the WASM side requires the upstream `grafeo`
> WASM port (see "Known limitation" above).

| Benchmark      | Native (M1, release) | WASM (M1 Chrome, release) | Ratio (WASM/native) |
|----------------|----------------------|---------------------------|---------------------|
| `cold_start`   | TODO µs              | TODO µs                   | TODO ×              |
| `batch_flush`  | TODO µs              | TODO µs                   | TODO ×              |
| `hydration`    | TODO ms              | TODO ms                   | TODO ×              |
| `tree_ops`     | TODO µs              | TODO µs                   | TODO ×              |

### How to fill in the table

1. **Native** — on an M1 Mac:
   ```bash
   cargo bench --features full
   # Read the `time: [X µs Y µs Z µs]` line from each group's output.
   # Use the median (Y) for the table.
   ```
2. **WASM** — once `grafeo` supports WASM, on the same M1 Mac under
   Chrome 120+:
   ```bash
   cargo bench --target wasm32-unknown-unknown --features full,wasm
   # Criterion's wasm-bindgen engine writes the same `time: [...]` lines
   # to the JS console — copy the median into the table.
   ```
3. **Ratio** — `WASM_native / native`. Typical WASM slowdowns vs native
   for compute-heavy Rust workloads are 1.5×–4× (depends on how much
   the work touches `wasm-bindgen` boundary vs pure-WASM compute).

## Bench file layout

```
benches/
├── README.md   ← this file
└── core.rs     ← four benchmark groups + criterion_group! + criterion_main!
```

`core.rs` is the single bench binary (`[[bench]] name = "core"` in
`Cargo.toml`). Each group is a `fn bench_X(c: &mut Criterion)` registered
via `criterion_group!`. `harness = false` lets `criterion_main!` provide
`fn main()` (the default Rust bench harness does not understand
criterion's `--bench` flags).

## Iteration counts

The benches use `iter_batched` with `BatchSize::LargeInput` for
`batch_flush`, `hydration`, and `tree_ops` (their setup is heavy: builds
a `GrafeoDB`, reconciles 1000 vertices, etc.). `cold_start` uses
`BatchSize::SmallInput` because its setup (one `InMemoryStorage::new()`)
is trivial.

Criterion will auto-tune the iteration count to keep each measurement
within the configured warm-up + measurement windows (default 3 s + 5 s).
The `LargeInput` choice prevents criterion from spending most of the
window in setup; the trade-off is fewer independent samples (lower
statistical confidence). Tune via `Criterion::sample_size(n)` if needed.
