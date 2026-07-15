# Migration Guide: Raw Loro → grafeo-loro-wrapped Loro

This guide walks Onde (and any other consumer) through migrating from
direct `loro-crdt` usage to `grafeo-loro` as the native data layer.
Issue #1 reference: <https://github.com/OndeHQ/grafeo-loro/issues/1>.

## Why migrate?

Raw `loro-crdt` is an excellent CRDT library, but using it directly as
the persistence layer for a graph application leaves three structural
problems unsolved. First, there is no single source of truth (SSOT) for
graph state — Onde previously kept vertex / edge metadata in ad-hoc
side tables indexed by Loro `TreeID`, which drifts from the Loro doc
whenever a peer mutates state through a path the side tables do not
observe. Second, there is no execution layer — Loro gives you CRDT
consensus but no Cypher, no vector indexes, no schema; Onde ends up
re-implementing those ad-hoc on top of Loro maps. Third, there is no
bridge: the moment you want a transactional graph database (grafeo)
backed by a CRDT for sync (Loro), you need a bidirectional translator
that drains Loro diffs into grafeo mutations and drains grafeo CDC
events back into Loro ops — with echo-prevention so the two stores do
not infinitely re-replicate each other's writes.

`grafeo-loro` solves all three by making Loro the SSOT, owning the
`LoroDoc`, exposing a thin wrapper API (`GrafeoLoroApp`), and running
the bridge as background workers. Onde migrates from "raw Loro" to
"grafeo-loro-wrapped Loro" so that vertex / edge state has one home
(the Loro doc), one query path (grafeo), and one sync path (the bridge).
The migration is mechanical for most callers — the bulk of the work is
swapping `LoroTree` ops for `tree::` adapter ops and `doc.subscribe()`
for `app.subscribe()`.

## Prerequisites

- **Rust 1.80+** (the MSRV; declared in `Cargo.toml` `rust-version`).
  `loro = "1.13"` requires it and grafeo-loro uses `std::sync::OnceLock`
  internally (stabilized 1.70; the 1.80 floor is loro's, not ours).
- **`loro = "1.13"`** — grafeo-loro pins the same version (issue #1
  item 5). You no longer need a direct `loro-crdt` dep in your
  `Cargo.toml`; grafeo-loro re-exports the crate as
  `grafeo_loro::loro` so you can call native Loro APIs (`LoroDoc`,
  `LoroMap`, `LoroTree`, `UndoManager`, `ExportMode`, …) through it.
- **For WASM**: install the `wasm32-unknown-unknown` target via
  `rustup target add wasm32-unknown-unknown`. Do NOT enable the
  `batcher`, `compression`, `parallel`, `grafeo`, `onnx`, or `webrtc`
  features in WASM builds — they pull native deps (tokio runtime, zstd
  C lib, rayon, ort, webrtc-rs) that do not compile for wasm32.

## 1. Cargo.toml

Swap the direct `loro-crdt` dep for `grafeo-loro`. Use
`default-features = false` and opt into the feature set you need:

```toml
[dependencies]
# Before:
# loro = "1.0"

# After — native (Onde server / Electron main process):
grafeo-loro = { version = "0.2", default-features = false, features = ["bridge", "batcher", "compression", "tree"] }

# After — WASM (Onde browser bundle):
grafeo-loro = { version = "0.2", default-features = false, features = ["bridge", "tree", "wasm"] }
```

You no longer need a direct `loro` dep. grafeo-loro re-exports it as
`grafeo_loro::loro`, so call sites change from `use loro::LoroDoc;` to
`use grafeo_loro::loro::LoroDoc;` (or, more idiomatically, you reach
the doc through `app.doc()` and never construct a `LoroDoc` yourself —
see §3).

## 2. LoroTree ops → tree:: ops

Raw Loro exposes tree operations on `LoroTree` directly:

```rust
// Before — raw Loro:
let tree = doc.get_tree("outline");
let child = tree.create(parent_id)?;          // create + attach
tree.move_to(child, new_parent_id)?;          // reparent
tree.delete(child)?;
let parent = tree.parent(child);              // Option<TreeParentId>
let kids = tree.children(parent_id)?;         // Vec<TreeID>
```

grafeo-loro does NOT use `LoroTree` containers. Tree structure is
modelled as `:CHILD` edges in the graph (`(parent)-[:CHILD]->(child)`)
so that the same vertex / edge schema serves both graph queries and
tree queries. The `tree` feature module (`tree_adapter`) provides
ergonomic wrappers that emit `LoroOp::TreeMove` through the bridge —
they do NOT bypass the bridge to call grafeo directly. This keeps the
SSOT contract intact (Loro remains the source of truth; grafeo is the
read-optimized materialized view).

```rust
// After — grafeo-loro (with feature = "tree"):
use grafeo_loro::TreeAdapter;

let maps = app.bridge_maps();
let tree = TreeAdapter::new(maps);

// Reads (mirror the old LoroTree API):
let parent: Option<NodeId> = tree.parent(child_id)?;
let kids: Vec<NodeId> = tree.children(parent_id)?;
let all_kids: Vec<NodeId> = tree.descendants(root_id);   // DFS pre-order
let chain: Vec<NodeId> = tree.ancestors(child_id);       // immediate parent first, root last
let view = tree.view(child_id)?;                          // TreeNode { id, parent, children }

// Writes (emit LoroOp; feed through apply_loro_op or the inbound batcher):
let op = tree.create_child_op(parent_id, "V/new-child".to_string(), "CHILD".to_string());
let op = tree.move_op(child_id, old_parent_id, new_parent_id);
let op = tree.indent_op(child_id, parent_id, previous_sibling_id);
let op = tree.outdent_op(child_id, parent_id, grandparent_id);
```

Equivalence table (issue #1 item 14 mandate):

| Old Onde (raw Loro)        | New Onde (grafeo-loro)             |
|----------------------------|------------------------------------|
| `node.parent()`            | `tree::parent(node)`               |
| `node.children()`          | `tree::children(node)`             |
| `node.descendants()`       | `tree::descendants(node)`          |
| `node.ancestors()`         | `tree::ancestors(node)`            |
| `node.move_to(new_parent)` | `tree::move_op(node, old, new)`    |
| `node.indent()`            | `tree::indent_op(node, p, prev)`   |
| `node.outdent()`           | `tree::outdent_op(node, p, gp)`    |

The adapter does NOT perform the moves itself — it constructs `LoroOp`
values. The caller feeds those into `apply_loro_op` (single op) or the
inbound batcher (high-frequency stream). This indirection is what lets
the same adapter work in pure-WASM builds where grafeo is off; the
adapter only knows about `BridgeMaps`, not the grafeo execution layer.

## 3. doc.subscribe() → app.subscribe()

In raw Loro, you subscribe to doc events directly:

```rust
// Before — raw Loro:
let _sub = doc.subscribe_root(Arc::new(move |event| {
    // handle diff event
}));
```

In grafeo-loro, the app owns the `LoroDoc`. You no longer construct or
subscribe to the doc directly — `GrafeoLoroApp::build()` attaches its
own internal subscriber (the bridge's inbound path) during `spawn_all`.
You attach YOUR subscriber through `app.subscribe(handler)`, which
returns a `loro::Subscription` you keep alive for as long as you want
events:

```rust
// After — grafeo-loro:
let _sub = app.subscribe(|ev: grafeo_loro::loro::event::DiffEvent| {
    // Onde's orchestrator forwards to its UI store / reactive graph.
});
```

Three things to know about event ordering and ownership:

1. **Multiple subscribers coexist** (issue #1 item 4). grafeo-loro's
   internal bridge subscriber and Onde's external subscriber both fire
   on every commit. There is no exclusive `take_event_handler` — call
   `subscribe` as many times as you need.
2. **Registration order matters**. grafeo-loro's bridge subscribes
   first (during `build()`), so the bridge's translation runs before
   Onde's handler. This means Onde's handler sees post-bridge state —
   useful for orchestrator bookkeeping but NOT for intercepting ops
   (use the inbound MPSC channel for that).
3. **Handler MUST be fast and non-blocking**. It runs synchronously
   inside `LoroDoc::commit`. For expensive work, forward the event to
   a channel and process it on a separate task.

The `LoroDoc` itself is reached through `app.doc()` which returns a
`parking_lot::RwLockReadGuard<'_, LoroDoc>`. Hold the guard only for
the duration of a single Loro API call — do NOT hold it across an
`.await` point or a `shutdown()` call. The bridge's outbound worker
needs the write lock to commit its origin-tagged commit pairs, so a
long-held read guard stalls the bridge.

## 4. UndoManager integration

`loro::UndoManager` continues to work unchanged against the `LoroDoc`
returned by `app.doc()`. Construct it once per peer, keep the peer ID
stable while the manager is alive (Loro's undo stack is per-peer), and
call `undo()` / `redo()` as before:

```rust
use grafeo_loro::loro::UndoManager;

let mut undo = {
    let doc = app.doc();
    UndoManager::new(&*doc)
};
// ... user performs an action via app.doc().get_map(...).insert(...) ...
undo.undo()?;   // rolls back the last local commit
undo.redo()?;
```

The only behavioural difference is that the bridge's inbound worker
re-applies the rolled-back state to grafeo as a fresh `LoroOp` (the
bridge sees an undo as just another diff event). Echo-prevention via
the `ORIGIN_LORO_BRIDGE` tag on internal commits means the bridge's
own write-back does not feedback-loop. You do not need to coordinate
undo with the bridge — just call `UndoManager::undo()` and let the
subscriber propagate the diff.

## 5. Snapshot bytes

Raw Loro uses `LoroDoc::export(ExportMode::Snapshot)` /
`LoroDoc::import(&bytes)` for full snapshots, and
`ExportMode::updates(&vv)` for incremental deltas. grafeo-loro wraps
this in `app.checkpoint(graph_id)` and `app.hydrate(graph_id)`, which
add a compression envelope and a stable storage key layout:

```rust
// Before — raw Loro:
let bytes = doc.export(loro::ExportMode::Snapshot)?;
std::fs::write("snapshot.loro", &bytes)?;

// After — grafeo-loro (SsotMode::Loro, CompressionType::Zstd):
app.checkpoint("graph_1").await?;   // export + compress + save
// ... process restarts ...
let app2 = GrafeoLoroApp::builder()
    .ssot_mode(SsotMode::Loro)
    .compression(CompressionType::Zstd)
    .storage(storage_clone)
    .build().await?;
app2.hydrate("graph_1").await?;     // load + decompress + import + parallel_hydrate_grafeo
```

Internally, `checkpoint` does:

1. `LoroDoc::oplog_frontiers()` — capture the current frontiers.
2. `LoroDoc::export(ExportMode::shallow_snapshot(&frontiers))` —
   shallow snapshot (current state + partial history since frontiers;
   history-trimmed per architecture §4 Step D to prevent storage bloat).
3. `CompressedPayload::compress_to_wire(&bytes, self.compression)` —
   wrap under the configured codec (zstd / lz4 / none) and serialize
   to the on-wire format.
4. `StorageBackend::save("{graph_id}/base.loro", wire_bytes)` —
   overwrite the base snapshot key.

`hydrate` is the inverse: `load` → `decompress_from_wire` →
`LoroDoc::import_with(&bytes, ORIGIN_LORO_BRIDGE)` (the origin tag
makes the bridge's echo filter skip the import) → enumerate and
re-import any delta keys → `parallel_hydrate_grafeo` to rebuild the
grafeo indexes from the restored Loro state.

The on-wire format is `[version:u8][codec_tag:u8][raw_data..]` where
`codec_tag` is `0x00=None`, `0x01=Lz4`, `0x02=Zstd` (matches the
`CompressionType` discriminant order). The version byte is reserved
for future format changes (currently `0x01`). The codec tag lets
`hydrate` decompress without out-of-band metadata — the snapshot is
self-describing.

Do NOT call `hydrate` on an app whose `GrafeoDB` / `BridgeMaps` is
already warm — `parallel_hydrate_grafeo` assumes a cold start and will
create duplicate vertices. The canonical pattern is
`builder().build().await` + `hydrate(graph_id).await` exactly once at
cold boot.

## 6. Worked example: 50-line outliner

See `examples/minimal_outliner.rs` in the repo root. Build with:

```bash
cargo build --features full --example minimal_outliner
```

Run with:

```bash
cargo run --features full --example minimal_outliner
```

The example demonstrates: app construction (`builder().storage().ssot_mode()
.compression().build().await`), event subscription
(`app.subscribe(|ev| ...)`), cold-boot hydrate (`app.hydrate(graph_id)`),
mutation via the wrapped `LoroDoc` (`app.doc().get_map(...).insert(...)`),
checkpoint (`app.checkpoint(graph_id)`), and graceful shutdown
(`app.shutdown()`). The whole example is 67 lines including comments —
real code body is under 50.

## 7. Feature flags

| Feature       | Pulls                                  | WASM-safe | When to enable                              |
|---------------|----------------------------------------|-----------|---------------------------------------------|
| `bridge`      | `lorosurgeon`, `parking_lot`, `bincode`| yes       | Always (minimal hot-path contract).         |
| `batcher`     | `bridge` + `tokio` (sync, time)        | no        | Native server / Electron main.              |
| `compression` | `lz4_flex`, `zstd` (C lib), `grafeo`   | no        | Native; checkpoint/hydrate need a codec.    |
| `tree`        | `bridge`                               | yes       | Any app that models trees as `:CHILD` edges.|
| `storage`     | `async-trait`                          | yes       | Any app that checkpoints / hydrates.        |
| `grafeo`      | `bridge` + `grafeo` (native only)      | no        | Native; required by `compression` + `onnx`. |
| `onnx`        | `grafeo` + `ort` (native only)         | no        | Native vector embeddings via ONNX Runtime.  |
| `webrtc`      | `webrtc-rs` (native only)              | no        | Native WebRTC transport for presence.       |
| `telemetry`   | `opentelemetry`, `tracing`             | yes       | Production (metrics + traces + health).     |
| `wasm`        | `wasm-bindgen`, `js-sys`, `web-sys`     | required  | Browser WASM bundle only.                   |
| `parallel`    | `rayon`                                | no        | Native; parallel hydration speedup.         |
| `serde`       | `serde`, `serde_json`                  | yes       | Admin APIs + bincode FFI entry point.       |
| `full`        | all native features (no `wasm`)        | no        | Dev / CI / examples.                        |

Recommended bundles:

- **Onde native**: `["bridge", "batcher", "compression", "tree"]`
- **Onde WASM**: `["bridge", "tree", "wasm"]`
- **CI / dev**: `["full"]` (one-shot, pulls everything native)

## 8. Common pitfalls

**Forgetting to enable `tree`**. If you call `TreeAdapter::new(...)` or
`use grafeo_loro::TreeAdapter;` without `--features tree`, you get a
compile error, not a runtime failure — but the error message ("cannot
find type `TreeAdapter`") is not obviously tied to the feature gate.
Always include `tree` in your feature set when migrating tree code.

**Calling `hydrate` on a warm `GrafeoDB`**. `parallel_hydrate_grafeo`
assumes a cold start. If your process restarts without re-creating
the `GrafeoLoroApp` (e.g. you call `hydrate` twice on the same app
instance, or you construct a new app pointing at the same in-memory
`GrafeoDB`), you will get duplicate vertices. The fix is structural:
call `builder().build().await` + `hydrate(graph_id).await` exactly
once per process cold start. After that, all writes go through the
bridge.

**Holding `app.doc()` across an `.await`**. `app.doc()` returns a
`parking_lot::RwLockReadGuard`. Holding it across an `.await` point
blocks the bridge's outbound worker (which needs the write lock to
commit) and can deadlock under load. The pattern is: scope the doc
borrow tightly, drop it before awaiting. Same for `app.subscribe()` —
the handler closure must not hold any borrow of the app.

**Mixing raw `loro::LoroDoc` mutation with grafeo-loro**. grafeo-loro
owns the `LoroDoc`. If you construct a separate `LoroDoc::new()` and
mutate it, the bridge will never see your writes (it subscribes only
to the doc owned by `GrafeoLoroApp`). Always mutate via `app.doc()`.
The re-export `grafeo_loro::loro` is for type access only
(`LoroValue`, `ExportMode`, `UndoManager`, …), not for constructing
rival docs.

**Enabling `compression` in WASM**. The `zstd` crate binds to a C
library that does not compile for `wasm32-unknown-unknown`. If you
enable `compression` in a WASM build, you will get a C compile error
deep in `zstd-sys`. The fix is to NOT enable `compression` in WASM
feature sets — use `CompressionType::None` or `Lz4` (lz4_flex is
pure Rust) via a future `compression-lz4` sub-feature, or skip
checkpoint compression entirely in the browser and let the server
compress on its side of the storage backend.

**Trusting `subscribe` handler ordering for correctness**. The bridge
subscribes first; your handler runs second. If you write a handler
that MUST see events before the bridge translates them (e.g. to
intercept and rewrite ops), `app.subscribe` is the wrong tool — use
the inbound MPSC channel exposed via `Mailbox<T>` (issue #1 item 2).
The subscribe handler is for observation / bookkeeping, not
interception.

## 9. Need help?

- File issues at <https://github.com/OndeHQ/grafeo-loro/issues>.
- Architecture reference: `docs/grafeo-loro.architecture.md` in the
  repo.
- Worklog for issue #1 compliance: `worklog.md` at the repo root
  (per-task entries from all 12 agents).
