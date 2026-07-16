#!/usr/bin/env bash
# Manual blackbox QA for grafeo-loro issue #4 (WASM consumer support).
#
# Builds the WASM bindings with wasm-pack, then runs a Node.js smoke test
# that imports the .wasm and exercises the full WasmOfflineOpQueue +
# WasmEpochTracker JS API.
#
# Usage:
#   bash scripts/wasm_blackbox_qa.sh
#
# Prerequisites:
#   - rustup target wasm32-unknown-unknown installed
#   - wasm-pack installed (cargo install wasm-pack --locked)
#   - Node.js 18+ installed
#
# Implementation note: the main `grafeo-loro` crate's `[lib]` section does
# not yet declare `crate-type = ["cdylib"]` (it's a plain rlib so it can be
# consumed as a normal Rust dependency). The orchestrator may add cdylib at
# release time. Until then, we build the `examples/wasm-offline-queue`
# crate instead — it declares `crate-type = ["cdylib", "rlib"]` AND its
# `src/lib.rs` re-exports `WasmOfflineOpQueue` + `WasmEpochTracker` via
# `pub use grafeo_loro::{...}`, so the resulting `pkg/*.d.ts` declares
# both JS classes. This gives us the same blackbox coverage as building
# the main crate directly would.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXAMPLE_DIR="$REPO_ROOT/examples/wasm-offline-queue"
PKG_DIR="$EXAMPLE_DIR/pkg"
# JS module name (and asset basename) produced by wasm-pack for the example
# crate. Derived from the lib name `wasm_offline_queue_example`.
JS_MODULE_NAME="wasm_offline_queue_example"

cd "$REPO_ROOT"

echo "=== [1/6] Verifying toolchain ==="
rustup target list --installed | grep -q wasm32-unknown-unknown || {
  echo "ERROR: wasm32-unknown-unknown target not installed"
  echo "  Run: rustup target add wasm32-unknown-unknown"
  exit 1
}
command -v wasm-pack >/dev/null || {
  echo "ERROR: wasm-pack not installed"
  echo "  Run: cargo install wasm-pack --locked"
  exit 1
}
command -v node >/dev/null || { echo "ERROR: node not installed"; exit 1; }
NODE_MAJOR=$(node -e "console.log(process.versions.node.split('.')[0])")
if [ "$NODE_MAJOR" -lt 18 ]; then
  echo "ERROR: Node.js 18+ required (got $(node --version))"
  exit 1
fi
echo "    ✅ rustup wasm32 target + wasm-pack $(wasm-pack --version | awk '{print $2}') + node $(node --version)"

echo "=== [2/6] Building example crate with wasm-pack (target=web) ==="
# Clean any previous pkg/ to ensure a fresh build
rm -rf "$PKG_DIR"
(
  cd "$EXAMPLE_DIR"
  wasm-pack build --target web --release 2>&1 | tail -15
)
test -f "$PKG_DIR/$JS_MODULE_NAME.js" || {
  echo "ERROR: $PKG_DIR/$JS_MODULE_NAME.js not produced"
  exit 1
}
test -f "$PKG_DIR/$JS_MODULE_NAME.d.ts" || {
  echo "ERROR: $PKG_DIR/$JS_MODULE_NAME.d.ts not produced"
  exit 1
}
test -f "$PKG_DIR/${JS_MODULE_NAME}_bg.wasm" || {
  echo "ERROR: $PKG_DIR/${JS_MODULE_NAME}_bg.wasm not produced"
  exit 1
}
echo "    ✅ $JS_MODULE_NAME.js + .d.ts + .wasm produced"

echo "=== [3/6] Verifying JS class exports in .d.ts ==="
grep -q "WasmOfflineOpQueue" "$PKG_DIR/$JS_MODULE_NAME.d.ts" || {
  echo "ERROR: WasmOfflineOpQueue not exported in .d.ts"
  exit 1
}
grep -q "WasmEpochTracker" "$PKG_DIR/$JS_MODULE_NAME.d.ts" || {
  echo "ERROR: WasmEpochTracker not exported in .d.ts"
  exit 1
}
echo "    ✅ WasmOfflineOpQueue + WasmEpochTracker exported"

echo "=== [4/6] Verifying JS class methods surface in .d.ts ==="
# Spot-check that key methods/properties from the issue #4 API contract
# appear in the generated TypeScript declarations.
for member in "enqueue" "drain" "depth" "bytesUsed" "capBytes" "retryBump" \
              "resetRetry" "retryCount" "isEmpty" "withCap" \
              "current" "checkMatch" "bump" "wipe"; do
  grep -q "\b$member\b" "$PKG_DIR/$JS_MODULE_NAME.d.ts" || {
    echo "ERROR: '$member' not found in .d.ts"
    exit 1
  }
done
echo "    ✅ All 14 queue + tracker members present in .d.ts"

echo "=== [5/6] Running Node.js smoke test ==="
# Node 18+ supports ES modules + top-level await. We write a small smoke
# test into a temp dir and import the built JS module via a `file://` URL
# (Node's "imports" package.json field requires relative `./` paths, which
# is awkward when the temp dir is in /tmp; an absolute `file://` URL is
# simpler and equally robust).
SMOKE_TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$SMOKE_TEST_DIR" "$PKG_DIR"' EXIT

cat > "$SMOKE_TEST_DIR/smoke.mjs" <<EOF
import init, { WasmOfflineOpQueue, WasmEpochTracker } from "file://$PKG_DIR/$JS_MODULE_NAME.js";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const wasmPath = join("$PKG_DIR", "${JS_MODULE_NAME}_bg.wasm");
const wasmBytes = readFileSync(wasmPath);

// wasm-pack --target web output expects to be loaded in a browser; in
// Node we have to instantiate the wasm module ourselves and feed it
// to \`init\`.
await init({ module_or_path: wasmBytes });

let pass = 0, fail = 0;
function assert(cond, msg) {
  if (cond) { pass++; console.log("  ✅", msg); }
  else { fail++; console.log("  ❌", msg); }
}

console.log("=== WasmOfflineOpQueue ===");

// 1. Construct with default cap
const q = new WasmOfflineOpQueue();
assert(q.depth === 0, "fresh queue: depth === 0");
assert(q.bytesUsed === 0, "fresh queue: bytesUsed === 0");
assert(q.capBytes === 10 * 1024 * 1024, "fresh queue: capBytes === 10MB");
assert(q.isEmpty === true, "fresh queue: isEmpty === true");

// 2. Enqueue 3 ops
q.enqueue(new Uint8Array([1, 2, 3]));
q.enqueue(new Uint8Array([4, 5, 6]));
q.enqueue(new Uint8Array([7, 8, 9, 10]));
assert(q.depth === 3, "after 3 enqueues: depth === 3");
assert(q.bytesUsed === 10, "after 3 enqueues: bytesUsed === 10");
assert(q.isEmpty === false, "after 3 enqueues: isEmpty === false");

// 3. Retry hooks
const r1 = q.retryBump();
const r2 = q.retryBump();
assert(r1 === 1, "retryBump() returns 1");
assert(r2 === 2, "retryBump() returns 2");
assert(q.retryCount === 2, "retryCount === 2");
q.resetRetry();
assert(q.retryCount === 0, "after resetRetry: retryCount === 0");

// 4. Drain returns FIFO order
const drained = q.drain();
assert(Array.isArray(drained), "drain() returns Array");
assert(drained.length === 3, "drain() returns 3 ops");
assert(drained[0] instanceof Uint8Array, "drain()[0] is Uint8Array");
assert(drained[0].length === 3 && drained[0][0] === 1, "drain()[0] === [1,2,3]");
assert(drained[1].length === 3 && drained[1][0] === 4, "drain()[1] === [4,5,6]");
assert(drained[2].length === 4 && drained[2][0] === 7, "drain()[2] === [7,8,9,10]");
assert(q.depth === 0, "after drain: depth === 0");
assert(q.bytesUsed === 0, "after drain: bytesUsed === 0");
assert(q.isEmpty === true, "after drain: isEmpty === true");

// 5. Cap enforcement
const smallQ = WasmOfflineOpQueue.withCap(10);
smallQ.enqueue(new Uint8Array([1, 2, 3, 4, 5]));   // 5 bytes
smallQ.enqueue(new Uint8Array([1, 2, 3, 4, 5]));   // 10 bytes total — at cap
let threw = false;
let thrownErr = null;
try {
  smallQ.enqueue(new Uint8Array([1]));             // would exceed cap
} catch (e) {
  threw = true;
  thrownErr = e;
}
assert(threw === true, "enqueue past cap throws");
if (thrownErr !== null) {
  assert(thrownErr.code === 1008, "cap overflow error code === 1008 (GrafeoLoroError::Bridge)");
  assert(typeof thrownErr.message === "string" && thrownErr.message.includes("offline queue overflow"), "cap overflow message mentions 'offline queue overflow'");
}
assert(smallQ.depth === 2, "after rejected enqueue: depth === 2 (unchanged)");

console.log("");
console.log("=== WasmEpochTracker ===");

// NOTE: u64 values surface as JS BigInt (per wasm-bindgen convention).
// \`current\`, \`bump()\`, \`wipe()\` return bigint; \`checkMatch(remote)\` and
// the \`error.local\`/\`error.remote\` fields on a mismatch error are also
// bigint.

const epoch = new WasmEpochTracker();
assert(epoch.current === 0n, "fresh tracker: current === 0n");

// checkMatch — match (passing BigInt)
epoch.checkMatch(0n); // should not throw

// checkMatch — mismatch
threw = false;
thrownErr = null;
try {
  epoch.checkMatch(1n);
} catch (e) {
  threw = true;
  thrownErr = e;
}
assert(threw === true, "checkMatch(1n) on epoch=0n throws");
if (thrownErr !== null) {
  assert(thrownErr.code === 1013, "epoch mismatch error code === 1013");
  assert(thrownErr.local === 0n, "epoch mismatch error.local === 0n");
  assert(thrownErr.remote === 1n, "epoch mismatch error.remote === 1n");
  assert(typeof thrownErr.message === "string" && thrownErr.message.includes("lineage epoch mismatch"), "epoch mismatch message mentions 'lineage epoch mismatch'");
}

// bump
const b1 = epoch.bump();
assert(b1 === 1n, "bump() returns 1n");
assert(epoch.current === 1n, "after bump: current === 1n");
const b2 = epoch.bump();
assert(b2 === 2n, "bump() returns 2n");
assert(epoch.current === 2n, "after 2 bumps: current === 2n");

// wipe (alias for bump)
const w1 = epoch.wipe();
assert(w1 === 3n, "wipe() returns 3n");
assert(epoch.current === 3n, "after wipe: current === 3n");

// checkMatch after wipe
epoch.checkMatch(3n); // should not throw

console.log("");
console.log(\`=== Smoke test complete: \${pass} passed, \${fail} failed ===\`);
if (fail > 0) process.exit(1);
EOF

node "$SMOKE_TEST_DIR/smoke.mjs"
SMOKE_EXIT=$?
echo "    smoke test exit code: $SMOKE_EXIT"
if [ "$SMOKE_EXIT" -ne 0 ]; then exit 1; fi

echo "=== [6/6] Cleanup ==="
rm -rf "$PKG_DIR"
echo "    ✅ pkg/ removed (was a build artifact)"

echo ""
echo "=== ALL BLACKBOX QA PASSED ==="
