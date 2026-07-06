# P2T3 Hunter Critique

**Task ID**: P2T3-HUNT
**Agent**: Plenger Hunter
**Branch**: `p2-vertex-builder`
**Target**: Cumulative P2T3-L1 + P2T3-L2 + P2T3-L3 work (Phase 2 Task 3 — `app::VertexBuilder` fluent API)
**Critique artifact**: this file (`docs/critiques/p2t3-hunt.md`)
**Method**: read-only verification against `grafeo-engine-0.5.42` / `loro-1.13.6` / `lorosurgeon-0.2.1` / `grafeo-core-0.5.42` / `grafeo-common-0.5.42` source in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`, plus `grafeo-loro` `src/`+`tests/`, `docs/critiques/p2t3-l1-devil.md`, `worklog.md`. Hunter touched NO `src/` or `tests/` files (read-only mandate); only this critique file and `worklog.md` were modified.

---

## 0. Verification matrix — every L3 claim re-checked independently

### 0.1 Compile / test status (Task 1 + Task 2)

| L3 claim | Verification command | Result | Citation |
|---|---|---|---|
| `cargo check --all-targets` exit 0, 5 pre-existing warnings | `cargo check --all-targets 2>&1 \| tail -30` | ✅ PASS — 5 warnings (`hydration/vector.rs:9+27`, `presence/socket.rs:6`, `telemetry/health.rs:9`, `app.rs:46` builder fields), 0 errors, 0 new warnings | local run |
| `cargo test --no-run --all` emits 3 test binaries | `cargo test --no-run --all 2>&1 \| tail -10` | ✅ PASS — `grafeo_loro-e6e174b4d8b88039` (lib), `integration-13c51a3c9b7180c2`, `unit-2e155c9954744fca` | local run |
| `cargo test --all` → 34 PASS + 0 IGNORED + 0 FAIL | `cargo test --all 2>&1 \| rg "test result"` | ✅ PASS — `6 passed; 0 failed; 0 ignored` (lib) + `5 passed; 0 failed; 0 ignored` (integration) + `23 passed; 0 failed; 0 ignored` (unit) + `0 passed; 0 failed; 0 ignored` (doctests) = **34 PASS + 0 IGNORED + 0 FAIL** | local run |
| `cargo test --test unit vertex_builder_concurrent_commit` runs 5× with 0 failures | ran 5 times | ✅ PASS — 5/5 runs `1 passed; 0 failed; 0 ignored` | local run |

L3's compile/test claims are 100% accurate.

### 0.2 Stub verification (Task 3)

| grep | L3 claim | Result |
|---|---|---|
| `grep -nE "TODO\|todo!\|unimplemented!" src/app.rs` | All remaining `unimplemented!()` in Phase 3-5 scope methods | ✅ PASS — 14 matches, ALL in `GrafeoLoroApp::{builder, query, update_text, generate_embedding, checkpoint, broadcast_presence, shutdown}` (Phase 3-5) + `GrafeoLoroAppBuilder::{storage, ssot_mode, compression, sync_compression, batch_interval_ms, batch_max_size, build}` (Phase 4). Each has a phase-scope doc-comment. Zero `unimplemented!()` in `commit()` body. |
| `grep -nE "TODO\|todo!\|unimplemented!" tests/unit/vertex_builder.rs` | Zero matches (except doc-comment examples) | ✅ PASS — 4 matches at lines 35, 36, 54, 55, ALL inside `#![allow(missing_docs)]` doc-comment code examples (`# let doc: Arc<RwLock<LoroDoc>> = unimplemented!();`). Zero in test bodies. |
| `grep -rn "#\[ignore" tests/` | Zero matches | ✅ PASS — Zero matches |
| `grep -rn "L2 HACK" src/ tests/` | Zero matches | ✅ PASS — Zero matches in `src/` + `tests/` (only historical references in `worklog.md` + `docs/critiques/p2t2-hunt.md`) |

L3's stub-verification claims are 100% accurate.

### 0.3 Anti-Goodhart verification (Task 4) — 9 test bodies

| Test | Non-trivial assertion? | BOTH stores? | Verdict |
|---|---|---|---|
| `vertex_builder_basic_roundtrip` | `assert_grafeo_has_vertex(&db, node_id, &["Person"], &[("name", String("Alix"))])` + `assert_loro_has_vertex(&doc, &loro_key, &["Person"], &[("name", String("Alix"))])` | ✅ BOTH | ✅ PASS |
| `vertex_builder_multiple_labels` | 3 labels asserted in BOTH stores via `assert_grafeo_has_vertex` (count + each `has_label`) + `assert_loro_has_vertex` (sorted set equality) | ✅ BOTH, ALL 3 | ✅ PASS |
| `vertex_builder_multiple_properties` | 3 properties (Bool/Integer/String) asserted in BOTH stores with value equality | ✅ BOTH, ALL 3 | ✅ PASS |
| `vertex_builder_empty_vertex` | `commit()` returns `Ok(NodeId)` + `assert_grafeo_has_vertex(&db, node_id, &[], &[])` + `assert_loro_has_vertex(&doc, &loro_key, &[], &[])` (empty labels vec + empty properties map) | ✅ BOTH, empty | ✅ PASS |
| `vertex_builder_atomicity_rollback_on_grafeo_failure` | `result.is_err()` + `assert_no_side_effects(&app, &doc, "V/0")` (Loro V map empty + BridgeMaps `node_id_map` + `node_key_map` both empty) | ✅ NO side effects | ✅ PASS |
| `vertex_builder_concurrent_commit` | 20 distinct NodeIds (HashSet) + 20 distinct loro_keys (HashSet) + `BridgeMaps::node_id_map.len() == 20` + `BridgeMaps::node_key_map.len() == 20` + each pair round-trips forward+inverse | ✅ 20 DISTINCT pairs | ✅ PASS |
| `vertex_builder_rejects_vector_property` | `matches!(result, Err(GrafeoLoroError::UnsupportedLoroType(_)))` + Loro V map empty + BridgeMaps empty | ✅ Err + no side effects | ✅ PASS |
| `vertex_builder_rejects_map_property` | Same as Vector | ✅ Err + no side effects | ✅ PASS |
| `vertex_builder_rejects_list_property` | Same as Vector | ✅ Err + no side effects | ✅ PASS |

All 9 tests assert non-trivial conditions. L3's anti-Goodhart claims are 100% accurate.

### 0.4 Anti-hallucination verification (Task 5) — API citations

| API call | L3 citation | Verified location | Status |
|---|---|---|---|
| `LoroDoc::set_next_commit_origin(&self, &str)` | `loro-1.13.6/src/lib.rs:626` | `lib.rs:626` ✅ | PASS |
| `LoroDoc::commit(&self)` | `loro-1.13.6/src/lib.rs:593` | `lib.rs:593` ✅ (returns `()`, NOT `Result`) | PASS |
| `LoroDoc::get_map<I: IntoContainerId>(&self, I) -> LoroMap` | `loro-1.13.6/src/lib.rs:489` | `lib.rs:489` ✅ | PASS |
| `LoroMap::ensure_mergeable_map(&self, &str) -> LoroResult<LoroMap>` | `loro-1.13.6/src/lib.rs:2247` | `lib.rs:2247` ✅ (NON-DEPRECATED successor; `get_or_create_container` at `:2217` is `#[deprecated]` ✅) | PASS |
| `LoroMap::get(&self, &str) -> Option<ValueOrContainer>` | `loro-1.13.6/src/lib.rs:2150` | `lib.rs:2150` ✅ | PASS |
| `LoroMap::delete(&self, &str) -> LoroResult<()>` | `loro-1.13.6/src/lib.rs:2117` | `lib.rs:2117` ✅ | PASS |
| `ValueOrContainer::Container(Container::Map(LoroMap))` pattern | `loro-1.13.6/src/lib.rs:3813` + `:3636` | `ValueOrContainer` at `:3813` ✅; `Container` enum at `:3637` (L3 cited `:3636` which is the `use enum_as_inner::EnumAsInner;` import — NIT off-by-one) | PASS |
| `RootReconciler::new(LoroMap) -> Self` | `lorosurgeon-0.2.1/src/reconcile.rs:298` | `reconcile.rs:297` ✅ (L3 cited `:298`, actual `pub fn new` at `:297` — NIT off-by-one) | PASS |
| `<T as Reconcile>::reconcile<R: Reconciler>(&self, R) -> Result<(), ReconcileError>` | `lorosurgeon-0.2.1/src/reconcile.rs:92` | `reconcile.rs:95` ✅ (trait decl at `:91`, `fn reconcile` at `:95` — NIT off-by-one) | PASS |
| `<T as Hydrate>::hydrate_map(&LoroMap) -> Result<Self, HydrateError>` | `lorosurgeon-0.2.1/src/hydrate.rs:64` | `hydrate.rs:64` ✅ | PASS |
| `GrafeoDB::session_with_cdc(&self, bool) -> Session` | `grafeo-engine-0.5.42/src/database/mod.rs:1728` | `database/mod.rs:1728` ✅ (CDC feature enabled by default in `grafeo-0.5.42/Cargo.toml:44`) | PASS |
| `Session::begin_transaction(&mut self) -> Result<()>` | `grafeo-engine-0.5.42/src/session/mod.rs:3883` | `session/mod.rs:3883` ✅ (default = `SnapshotIsolation` per `transaction/manager.rs:55` `#[default]`) | PASS |
| `Session::create_node_with_props(&self, &[&str], impl IntoIterator<Item = (&str, Value)>) -> Result<NodeId>` | `grafeo-engine-0.5.42/src/session/mod.rs:4885` | `session/mod.rs:4885` ✅ (calls `check_property_size` at `:4892`) | PASS |
| `Session::prepare_commit(&mut self) -> Result<PreparedCommit<'_>>` | `grafeo-engine-0.5.42/src/session/mod.rs:4496` | `session/mod.rs:4496` ✅ | PASS |
| `Session::get_node(&self, NodeId) -> Option<Node>` | `grafeo-engine-0.5.42/src/session/mod.rs:5138` | `session/mod.rs:5138` ✅ | PASS |
| `Session::Drop` auto-rollback | `grafeo-engine-0.5.42/src/session/mod.rs:5368-5383` | `session/mod.rs:5372-5383` ✅ (`impl Drop for Session` at `:5372`; checks `self.in_transaction()` at `:5375`) | PASS |
| `PreparedCommit::set_metadata(impl Into<String>, impl Into<String>)` | `grafeo-engine-0.5.42/src/transaction/prepared.rs:107` | `prepared.rs:107` ✅ | PASS |
| `PreparedCommit::commit(self) -> Result<EpochId>` | `grafeo-engine-0.5.42/src/transaction/prepared.rs:124` | `prepared.rs:124` ✅ | PASS |
| `PreparedCommit::Drop` auto-rollback | `grafeo-engine-0.5.42/src/transaction/prepared.rs:141-148` | `prepared.rs:144-149` ✅ (`impl Drop for PreparedCommit` at `:144`; checks `if !self.finalized` at `:145`) | PASS |
| `Node::has_label(&str) -> bool` | `grafeo-core-0.5.42/src/graph/lpg/node.rs:80` | `node.rs:80` ✅ | PASS |
| `Node::get_property(&str) -> Option<&Value>` | `grafeo-core-0.5.42/src/graph/lpg/node.rs:91` | `node.rs:91` ✅ | PASS |
| `Config::in_memory() -> Self` | `grafeo-engine-0.5.42/src/config.rs:425` | `config.rs:425` ✅ | PASS |
| `Config::with_max_property_size(self, usize) -> Self` | `grafeo-engine-0.5.42/src/config.rs:559` | `config.rs:559` ✅ | PASS |
| `Value::estimated_size_bytes(&self) -> usize` (for `String(s)` returns `s.len()`) | `grafeo-common-0.5.42/src/types/value.rs:391` | `value.rs:391` ✅ | PASS |
| `apply_loro_op(&Session, &LoroOp, &BridgeMaps) -> Result<()>` | `src/bridge/grafeo_tx.rs:86` | `grafeo_tx.rs:86` ✅ | PASS |
| `BridgeMaps::node_id_map: RwLock<HashMap<String, grafeo::NodeId>>` (public) | `src/bridge/grafeo_tx.rs:28` | `grafeo_tx.rs:28` ✅ | PASS |
| `BridgeMaps::node_key_map: RwLock<HashMap<grafeo::NodeId, String>>` (public) | `src/bridge/grafeo_tx.rs:30` | `grafeo_tx.rs:30` ✅ | PASS |
| `SyncEngine::grafeo_db: Arc<GrafeoDB>` (pub(crate)) | `src/bridge/sync_engine.rs:97` | `sync_engine.rs:97` ✅ | PASS |
| `SyncEngine::loro_doc: Arc<RwLock<LoroDoc>>` (pub(crate)) | `src/bridge/sync_engine.rs:99` | `sync_engine.rs:99` ✅ | PASS |
| `SyncEngine::maps(&self) -> &Arc<BridgeMaps>` | `src/bridge/sync_engine.rs:179` | `sync_engine.rs:179` ✅ | PASS |
| `SyncEngine::inbound_event_count(&self) -> u64` (pub) | (not cited by L3 — discovered by hunter) | `sync_engine.rs:423` ✅ | PASS |
| `GrafeoLoroError::Loro(#[from] loro::LoroError)` | `src/error.rs:6` | `error.rs:6` ✅ (enables `?` on `ensure_mergeable_map`) | PASS |
| `GrafeoLoroError::Grafeo(#[from] grafeo::Error)` | `src/error.rs:9` | `error.rs:9` ✅ (enables `.into()` on `prepared.commit()` Err) | PASS |
| `GrafeoLoroError::UnsupportedLoroType(String)` | `src/error.rs:25` | `error.rs:25` ✅ | PASS |
| `GrafeoLoroError::Bridge(String)` | `src/error.rs:31` | `error.rs:31` ✅ | PASS |
| `ORIGIN_LORO_BRIDGE: &str = "loro-bridge"` | `src/constants.rs:3` | `constants.rs:3` ✅ | PASS |
| `ROOT_VERTICES: &str = "V"` | `src/constants.rs:6` | `constants.rs:6` ✅ | PASS |
| `VertexEntity { labels: Vec<String>, properties: HashMap<String, LoroProperty>, description: String }` with `#[derive(Hydrate, Reconcile)]` + `#[loro(text)] description` | `src/schema/vertex.rs:5-12` | `vertex.rs:5-12` ✅ | PASS |
| `TryFrom<GraphValue> for LoroProperty` (new impl added by L3) | `src/types/values.rs:126-143` | `values.rs:126-143` ✅ (scalar 1:1; Vector/Map/List → `Err(UnsupportedLoroType)`) | PASS |
| `gval_to_grafeo_value(GraphValue) -> grafeo::Value` | `src/types/values.rs:146` (L3 worklog citation) | `values.rs:171` ✅ (L3 worklog cited `:146` which is `lval_to_gval` — NIT citation error) | PASS |

**Zero hallucinations.** Every API call cited by L3 exists at (or within 1 line of) the cited location. The 3 NIT off-by-one citation errors (`Container` at `:3636` vs `:3637`, `RootReconciler::new` at `:298` vs `:297`, `Reconcile::reconcile` at `:92` vs `:95`) are cosmetic and don't affect correctness.

---

## 1. Findings

### MAJOR 1 — Step 6 `prepared.commit()` failure does NOT clean up `BridgeMaps` (atomicity contract violation)

**Location**: `src/app.rs:449-458`

**Code**:
```rust
let mut prepared = session.prepare_commit()?;
prepared.set_metadata("origin", ORIGIN_LORO_BRIDGE);
if let Err(raw_err) = prepared.commit() {
    let grafeo_err: GrafeoLoroError = raw_err.into();
    compensate_loro_vertex(&self.sync_engine, &loro_key, &grafeo_err, &self.labels, &self.properties);
    return Err(grafeo_err);
}
```

**Problem**: On `prepared.commit()` failure (e.g. grafeo write-write conflict, SSI violation, internal grafeo error), the code:
1. ✅ Calls `compensate_loro_vertex` (deletes Loro V/{loro_key})
2. ❌ Does NOT call `self.sync_engine.maps().remove_node(&loro_key)` to remove the stale `BridgeMaps` binding

Step 5 (`apply_loro_op`) succeeded → `maps.insert_node(loro_key, id)` was called → `BridgeMaps::node_id_map` and `BridgeMaps::node_key_map` now contain `{loro_key → NodeId}` and `{NodeId → loro_key}`. On step 6 failure, the Grafeo node was rolled back (via `session.commit()` → `commit_inner()` → `store.rollback_transaction_properties` at `grafeo-engine-0.5.42/src/session/mod.rs:4023-4035`), so the `NodeId` no longer exists in Grafeo. But `BridgeMaps` still has the binding, pointing to a phantom NodeId.

**Impact**: The system is left in an inconsistent state:
- Loro: clean (V/{loro_key} deleted by compensation)
- Grafeo: clean (tx rolled back by `session.commit()` internal catch block)
- BridgeMaps: **STALE** (`loro_key → NodeId` for a non-existent node)

A subsequent outbound CDC poll (if CDC were enabled) or a `apply_loro_op` for `DeleteNode { loro_key }` would look up the stale binding and try to operate on the phantom NodeId, causing undefined behavior (grafeo's `session.delete_node(phantom_id)` returns `false` silently; `session.get_node(phantom_id)` returns `None`).

**Atomicity contract violation**: The L1 doc-comment at `src/app.rs:194-200` says: "if either write fails, roll back the other (or fail before committing the second). The final state on Grafeo failure is therefore: both stores clean (no partial vertex)." The BridgeMaps is a third store that is NOT cleaned up on step 6 failure.

**Test coverage gap**: `vertex_builder_atomicity_rollback_on_grafeo_failure` triggers step 5 failure (via `check_property_size` rejection in `create_node_with_props`), NOT step 6 failure. So this bug is not caught by the test suite. The test asserts `assert_no_side_effects` which checks `BridgeMaps::node_id_map.read().is_empty()` — but this passes only because step 5 failure means `insert_node` was never called. A step 6 failure test would catch this bug.

**Proposed fix**:
```rust
if let Err(raw_err) = prepared.commit() {
    let grafeo_err: GrafeoLoroError = raw_err.into();
    compensate_loro_vertex(&self.sync_engine, &loro_key, &grafeo_err, &self.labels, &self.properties);
    // Remove the stale BridgeMaps binding (step 5 inserted it; step 6 failure
    // rolled back the Grafeo node, so the binding now points to a phantom).
    self.sync_engine.maps().remove_node(&loro_key);
    return Err(grafeo_err);
}
```

**Severity**: MAJOR — real bug, violates atomicity contract, but only fires on grafeo commit conflict (rare for write-only `create_node_with_props` at `SnapshotIsolation` since there's no read-then-write race).

---

### MAJOR 2 — B1 inbound filter extension (`ORIGIN_LORO_BRIDGE` skip) is NOT exercised by any test

**Location**: `src/bridge/sync_engine.rs:215` (filter clause added by P2T3-L2 commit `870a124`)

**Filter code**:
```rust
if event.origin == ORIGIN_GRAFEO_BRIDGE || event.origin == ORIGIN_LORO_BRIDGE {
    return;
}
```

**Problem**: The B1 filter is a P2T3-L2 BLOCKER fix that prevents `VertexBuilder::commit()`'s Loro write from echoing back through the inbound subscriber → batcher → Grafeo apply path. However:

1. **P2T3-L3 unit tests do NOT install the subscriber.** `build_app()` and `build_app_with_tiny_property_limit()` call `SyncEngine::new(...)` which does NOT call `init_loro_subscriber()`. The subscriber is only installed inside `spawn_all()` (at `src/bridge/sync_engine.rs:390`), which the unit tests never call.

2. **No integration test uses `VertexBuilder::commit()`.** The Phase 1 integration tests (`tests/integration/sync_echo.rs`) use `spawn_all()` (so the subscriber IS installed), but they drive edits through `session.execute(...)` and direct Loro writes — NONE of them call `GrafeoLoroApp::create_vertex().commit()`. So `ORIGIN_LORO_BRIDGE` is never set as a Loro commit origin in any integration test.

3. **Result**: The B1 filter clause `|| event.origin == ORIGIN_LORO_BRIDGE` is **dead code in the test suite**. A regression that removes this clause would pass ALL 34 tests. The filter is logically correct (verified by reading the code + tracing the flow), but it has ZERO test coverage.

**Flow trace (correct case, untested)**:
1. `commit()` step 3: `doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE);` + `doc.commit();`
2. `doc.commit()` fires the subscriber synchronously
3. Subscriber handler checks `event.origin == ORIGIN_GRAFEO_BRIDGE || event.origin == ORIGIN_LORO_BRIDGE`
4. Since `event.origin == "loro-bridge"`, the filter returns early
5. No `LoroOp` is sent to the inbound channel
6. `inbound_event_count` does NOT increment
7. No echo

**Flow trace (regression case, also untested)**: If the `|| event.origin == ORIGIN_LORO_BRIDGE` clause is removed:
1. `commit()` step 3: `doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE);` + `doc.commit();`
2. `doc.commit()` fires the subscriber
3. Subscriber handler checks `event.origin == ORIGIN_GRAFEO_BRIDGE` — FALSE (origin is `loro-bridge`)
4. Filter does NOT skip → `translate_diff_event(&event)` runs
5. `translate_diff_event` produces `LoroOp::UpsertNode { loro_key, labels: Vec::new(), properties }` (pre-existing Phase 1 bug M4 — labels always empty)
6. `inbound_tx.try_send(InboundMsg::Op(op))` sends to inbound channel
7. `inbound_event_count.fetch_add(1, ...)` increments
8. Inbound worker (if running) would later call `apply_loro_op` for the UpsertNode → UPDATE path (loro_key already in BridgeMaps) → sets empty properties on existing node → spurious no-op Grafeo commit polluting the epoch side-channel

**Test fixture gap**: `build_app()` should optionally install the subscriber. The `SyncEngine::inbound_event_count()` accessor (public, at `src/bridge/sync_engine.rs:423`) gives a deterministic hook to assert no echo.

**Proposed fix** — add a new test:
```rust
/// B1 filter must prevent `commit()`'s Loro write from echoing through the
/// inbound subscriber. Installs the subscriber (no workers), calls `commit()`,
/// asserts `inbound_event_count` did NOT increment.
#[test]
fn vertex_builder_commit_does_not_echo_through_subscriber() {
    let (app, _db, _doc) = build_app();
    // Install the subscriber (but NOT the workers — we only care that the
    // filter prevents the echo from reaching the inbound channel).
    app.sync_engine.init_loro_subscriber().expect("subscriber installed");
    let count_before = app.sync_engine.inbound_event_count();
    let _node_id = app
        .create_vertex()
        .with_label("Person")
        .with_property("name", GraphValue::String("Alix".into()))
        .commit()
        .expect("commit succeeds");
    let count_after = app.sync_engine.inbound_event_count();
    assert_eq!(
        count_before, count_after,
        "B1 filter must prevent commit() write from echoing through the subscriber \
         (ORIGIN_LORO_BRIDGE must be filtered)"
    );
}
```

**Note**: `app.sync_engine` is `pub(crate)`, so the test (in `tests/unit/`, a separate crate) cannot access it directly. The fix requires EITHER exposing a public accessor on `GrafeoLoroApp` like `pub fn sync_engine(&self) -> &SyncEngine` OR adding the test as a `#[cfg(test)] mod tests` inside `src/app.rs` (lib internal test).

**Severity**: MAJOR — Tautology/Goodhart risk. The B1 filter is a critical safety mechanism with zero test coverage. A regression could silently break echo prevention in production.

---

### MAJOR 3 — Step 6 `prepare_commit()?` does NOT compensate Loro (atomicity contract violation)

**Location**: `src/app.rs:447`

**Code**:
```rust
let mut prepared = session.prepare_commit()?;
```

**Problem**: If `prepare_commit()` returns Err, the `?` propagates immediately. Loro was already written in step 3 (`doc.commit()` at line 414), but `compensate_loro_vertex` is NOT called. The function returns Err with Loro still containing V/{loro_key}.

**Atomicity contract violation**: Same as MAJOR 1 — the L1 doc-comment says "if either write fails, roll back the other". Step 6 is part of the Grafeo write; if it fails, Loro must be rolled back.

**Likelihood**: LOW. `Session::prepare_commit()` → `PreparedCommit::new(session)` (at `grafeo-engine-0.5.42/src/transaction/prepared.rs:78`) returns Err ONLY if `session.current_transaction_id()` returns None (i.e. no active transaction). But L3's `commit()` calls `session.begin_transaction()?;` at step 4 BEFORE `prepare_commit()`, so the transaction IS active. The only way `prepare_commit()` could fail is if grafeo internals are corrupted (e.g. `current_transaction` was cleared by a concurrent thread — but `Session` is not `Sync` in the sense that it's typically used from one thread at a time; L3's `commit()` creates a fresh session per call).

**Proposed fix**:
```rust
let mut prepared = match session.prepare_commit() {
    Ok(p) => p,
    Err(raw_err) => {
        let grafeo_err: GrafeoLoroError = raw_err.into();
        compensate_loro_vertex(&self.sync_engine, &loro_key, &grafeo_err, &self.labels, &self.properties);
        drop(session); // auto-rollback Grafeo tx
        return Err(grafeo_err);
    }
};
```

**Severity**: MAJOR — contract violation, but theoretical failure path (requires grafeo internals to be in invalid state).

---

### MAJOR 4 — Step 4 `begin_transaction()?` does NOT compensate Loro (atomicity contract violation)

**Location**: `src/app.rs:423`

**Code**:
```rust
let mut session = self.sync_engine.grafeo_db.session_with_cdc(false);
session.begin_transaction()?;
```

**Problem**: If `begin_transaction()` returns Err, the `?` propagates immediately. Loro was already written in step 3, but `compensate_loro_vertex` is NOT called.

**Atomicity contract violation**: Same as MAJOR 1 + 3.

**Likelihood**: VERY LOW. `Session::begin_transaction()` returns Err ONLY if a transaction is already active (`TransactionError::InvalidState("transaction already active")`). But `session_with_cdc(false)` creates a FRESH session, so no transaction is active. The only way `begin_transaction()` could fail is if grafeo internals are corrupted.

**Proposed fix**:
```rust
let mut session = self.sync_engine.grafeo_db.session_with_cdc(false);
if let Err(raw_err) = session.begin_transaction() {
    let grafeo_err: GrafeoLoroError = raw_err.into();
    compensate_loro_vertex(&self.sync_engine, &loro_key, &grafeo_err, &self.labels, &self.properties);
    return Err(grafeo_err);
}
```

**Severity**: MAJOR — contract violation, but theoretical failure path (requires fresh session to have active tx, which is impossible under normal grafeo semantics).

---

### MINOR 1 — Test file uses literal `"V"` instead of `ROOT_VERTICES` constant (DRY violation)

**Location**: `tests/unit/vertex_builder.rs:195, 245, 533, 567, 600` (5 occurrences in code paths; lines 4, 37, 110, 240, 386, 515 are in doc-comments/assertion messages)

**Code** (example at line 195):
```rust
let v_map = doc_guard.get_map("V");
```

**Problem**: The constant `ROOT_VERTICES = "V"` exists at `src/constants.rs:6` and is `pub`. The source code (`src/app.rs:409`) correctly uses `ROOT_VERTICES`. The test file uses the literal `"V"` 5 times in code paths. If `ROOT_VERTICES` ever changes (e.g. from `"V"` to `"vertices"`), the source would use the new value but the tests would still use `"V"` and fail (or worse, silently test the wrong root map).

**Proposed fix**:
```rust
use grafeo_loro::constants::ROOT_VERTICES;
// ...
let v_map = doc_guard.get_map(ROOT_VERTICES);
```

**Severity**: MINOR — DRY violation; tests are slightly brittle to refactors of the constant.

---

### MINOR 2 — TOCTOU between `apply_loro_op` (step 5) and `node_id_map.get` (step 7)

**Location**: `src/app.rs:436` (step 5 insert) → `src/app.rs:463-474` (step 7 read)

**Problem**: Step 5 calls `apply_loro_op` which (via `apply_upsert_node` at `src/bridge/grafeo_tx.rs:141-142`) calls `session.create_node_with_props(...)` then `maps.insert_node(loro_key, id)`. Step 7 reads `self.sync_engine.maps().node_id_map.read().get(&loro_key)`. Between these two operations, another thread (e.g. the inbound worker processing a remote `LoroOp::DeleteNode { loro_key }`) could call `maps.remove_node(&loro_key)`, causing step 7 to return `Err(Bridge(...))`.

**Likelihood**: VERY LOW. Requires:
1. A remote peer to know about `loro_key` (just generated locally by `AtomicU64::fetch_add`)
2. The remote peer to send a `DeleteNode` for `loro_key`
3. The local inbound worker to process the delete
4. All between step 5 and step 7 (microsecond window)

**Impact**: The error is propagated (not silently dropped). The user sees `Err(Bridge("BridgeMaps missing binding for V/N after apply_loro_op"))`. The Loro and Grafeo states are still consistent (Loro has V/N, Grafeo has the node, BridgeMaps no longer has the binding). The user can retry.

**Proposed fix** (optional, larger refactor): Change `apply_loro_op` to return the `NodeId` for `UpsertNode` ops, eliminating the BridgeMaps read in step 7. This is a larger refactor (changes the `apply_loro_op` signature) and may not be worth the complexity for a theoretical race.

**Severity**: MINOR — theoretical TOCTOU; error is propagated safely.

---

### MINOR 3 — `compensate_loro_vertex` does not call `doc.commit()` on `v_map.delete()` failure (pending origin tag leak)

**Location**: `src/app.rs:506-517`

**Code**:
```rust
let comp_result: std::result::Result<(), loro::LoroError> = {
    let doc = sync_engine.loro_doc.write();
    doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE);
    let v_map = doc.get_map(ROOT_VERTICES);
    match v_map.delete(loro_key) {
        Ok(()) => {
            doc.commit();
            Ok(())
        }
        Err(e) => Err(e),  // ← doc.commit() NOT called
    }
};
```

**Problem**: If `v_map.delete(loro_key)` returns Err, `doc.commit()` is NOT called. The `set_next_commit_origin(ORIGIN_LORO_BRIDGE)` flag remains pending on the doc. When the next thread acquires the Loro write guard and calls `doc.commit()` (without first calling `set_next_commit_origin` with its own origin), that commit would use `ORIGIN_LORO_BRIDGE` as its origin — which would then be filtered by the B1 filter, causing the next commit's echo to be incorrectly skipped.

**Mitigating factor**: In the grafeo-loro architecture, ALL Loro writes go through either `VertexBuilder::commit()` (sets `ORIGIN_LORO_BRIDGE`) or `apply_change_event_to_loro` (sets `ORIGIN_GRAFEO_BRIDGE`). Both paths call `set_next_commit_origin` BEFORE `commit()`, overwriting the pending tag. So the pending tag is overwritten before the next commit fires. ✅ For Phase 2.

**Phase 4+ concern**: If user code writes directly to Loro without setting an origin, the pending `ORIGIN_LORO_BRIDGE` could cause an incorrect echo filter. But that's a Phase 4+ concern.

**Likelihood**: VERY LOW. `LoroMap::delete` returns Err only if the map is detached or the key holds a non-mergeable value. The V map is attached (root map); the key holds a LoroMap (mergeable). So `delete` should succeed.

**Proposed fix** (defensive): Always call `doc.commit()` to clear the pending origin tag, even on delete failure:
```rust
match v_map.delete(loro_key) {
    Ok(()) => {
        doc.commit();
        Ok(())
    }
    Err(e) => {
        doc.commit();  // ← clear the pending origin tag
        Err(e)
    }
}
```

**Severity**: MINOR — theoretical origin-tag leak; mitigated by Phase 2 architecture (all Loro writes set their own origin).

---

### MINOR 4 — Misleading comment at `src/app.rs:450-452` (Drop is a no-op on `prepared.commit()` Err)

**Location**: `src/app.rs:449-454`

**Code**:
```rust
if let Err(raw_err) = prepared.commit() {
    // `prepared` was consumed by `commit()`; on Err it auto-rolled
    // back via `Drop` (transaction/prepared.rs:141-148). The session
    // tx is also auto-rolled back when `session` drops below.
    let grafeo_err: GrafeoLoroError = raw_err.into();
    ...
}
```

**Problem**: The comment says "on Err it auto-rolled back via `Drop`". This is INCORRECT. Looking at `PreparedCommit::commit` (`grafeo-engine-0.5.42/src/transaction/prepared.rs:124-129`):
```rust
pub fn commit(mut self) -> Result<EpochId> {
    self.finalized = true;            // ← set BEFORE actual commit
    self.session.commit()?;           // ← can fail; on Err, returns to caller
    Ok(self.session.transaction_manager().current_epoch())
}
```

`self.finalized = true` is set BEFORE `self.session.commit()`. So when `prepared.commit()` returns Err, `prepared.finalized` is already `true`. The `Drop` impl checks `if !self.finalized` — but `finalized` is true, so `Drop` is a NO-OP.

The ACTUAL rollback happens inside `session.commit()` → `commit_inner()` at `grafeo-engine-0.5.42/src/session/mod.rs:4014-4036`: on `transaction_manager.commit(transaction_id)` Err, the catch block calls `store.rollback_transaction_properties(transaction_id)` for each touched graph, clears CDC events, savepoints, touched graphs. So the Grafeo state IS rolled back, but NOT via `PreparedCommit::Drop` — rather via `session.commit()`'s internal catch block.

**Proposed fix**: Update the comment to reflect the actual mechanism:
```rust
if let Err(raw_err) = prepared.commit() {
    // `prepared.commit()` set `finalized = true` before calling
    // `session.commit()`, so `PreparedCommit::Drop` is a no-op. The
    // actual Grafeo rollback happens inside `session.commit()` →
    // `commit_inner()`'s catch block (session/mod.rs:4014-4036), which
    // calls `store.rollback_transaction_properties(transaction_id)` for
    // each touched graph. The session tx is no longer active
    // (`current_transaction` was `take()`'d), so `Session::Drop` is
    // also a no-op.
    let grafeo_err: GrafeoLoroError = raw_err.into();
    ...
}
```

**Severity**: MINOR — comment is misleading but code is correct.

---

### NIT 1 — Worklog citation `gval_to_grafeo_value` at `values.rs:146` is inaccurate

**Location**: `worklog.md:1692` (P2T3-L3 stage summary)

**Problem**: The worklog cites `gval_to_grafeo_value` at `src/types/values.rs:146`, but the actual location is `src/types/values.rs:171`. Line 146 is `lval_to_gval` (a different function).

**Proposed fix**: Update the citation to `src/types/values.rs:171`.

**Severity**: NIT — cosmetic citation error; function exists, just wrong line number.

---

### NIT 2 — L3 doc-comment cites `Container` enum at `lib.rs:3636` (off-by-one)

**Location**: `worklog.md:1570` (P2T3-L3 work log)

**Problem**: L3 cited `Container` enum at `loro-1.13.6/src/lib.rs:3636`, but the actual `pub enum Container {` is at line 3637. Line 3636 is `use enum_as_inner::EnumAsInner;` (the derive import).

**Proposed fix**: Update citation to `:3637`.

**Severity**: NIT — off-by-one citation; enum exists at the cited location ± 1 line.

---

### ACCEPTABLE 1 — `AtomicU64::fetch_add(1, Ordering::Relaxed)` is sufficient for unique key generation

**Location**: `src/app.rs:383`

**Verification**: `Relaxed` ordering guarantees atomicity of the `fetch_add` itself (each call returns a distinct value). For a counter used ONLY to generate unique IDs (no other memory operations need to be synchronized with the counter increment), `Relaxed` is the correct ordering. `Acquire`/`Release` would add unnecessary memory barriers. The grafeo `NodeId` is generated independently by `create_node_with_props` (which has its own internal synchronization via MVCC). The Loro `RwLock` serializes the Loro write critical section. So `Relaxed` is sufficient.

**Severity**: ACCEPTABLE — correct use of `Relaxed` ordering; no fix needed.

---

### ACCEPTABLE 2 — `TryFrom<GraphValue> for LoroProperty` is a NEW conversion (not bloat)

**Location**: `src/types/values.rs:126-143`

**Verification**: The existing conversions in `src/types/values.rs` are:
- `lval_to_gval` (`LoroValue → GraphValue`) at line 146
- `gval_to_grafeo_value` (`GraphValue → grafeo::Value`) at line 171
- `grafeo_value_to_lval` (`grafeo::Value → LoroValue`) at line 196

There is NO existing `GraphValue → LoroProperty` conversion. The new `TryFrom` impl is necessary for `commit()` step 2 (building `VertexEntity.properties: HashMap<String, LoroProperty>`). It's a pure function (no side effects), total on the scalar subset (Null/Bool/Integer/Float/String), and rejects Vector/Map/List with `UnsupportedLoroType` (defensive — `commit()` step 1 strictly rejects these BEFORE this call). Not bloat.

**Severity**: ACCEPTABLE — DRY-compliant new conversion; no fix needed.

---

### ACCEPTABLE 3 — `compensate_loro_vertex` helper is DRY (shared by step 5 + step 6 error arms)

**Location**: `src/app.rs:496-539`

**Verification**: The helper is called from BOTH step 5 error arm (line 437) AND step 6 error arm (line 456). It correctly:
- Re-acquires the Loro write lock (`let doc = sync_engine.loro_doc.write();`)
- Sets `ORIGIN_LORO_BRIDGE` BEFORE delete (`doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE);`)
- Holds the lock across `set_next_commit_origin + delete + commit` (single block scope)
- On Loro compensation failure: logs at `error!` with full context (loro_key, labels, properties, both errors) and returns — caller returns the ORIGINAL Grafeo error (Q7 contract)

**Severity**: ACCEPTABLE — DRY-compliant helper; correct Q7 contract implementation.

---

### ACCEPTABLE 4 — `apply_loro_op` reuse is DRY (no inlined `create_node_with_props + insert_node`)

**Location**: `src/app.rs:436`

**Verification**: L3's `commit()` calls `apply_loro_op(&session, &op, self.sync_engine.maps())` instead of inlining `session.create_node_with_props(...)` + `maps.insert_node(...)`. This reuses the SSOT apply path (architecture §20) and ensures idempotency (lookup-or-create semantics). Devil M1 mandated this; L3 complies.

**Severity**: ACCEPTABLE — DRY-compliant reuse; no fix needed.

---

### ACCEPTABLE 5 — All 9 tests assert non-trivial conditions (anti-Goodhart compliant)

**Verification**: See Section 0.3 above. All 9 tests assert BOTH stores (where applicable), verify ALL labels/properties (not just one), check empty-state correctness, verify no side effects on failure, and verify 20 distinct pairs in the concurrent test.

**Severity**: ACCEPTABLE — anti-Goodhart compliant; no fix needed.

---

### ACCEPTABLE 6 — Concurrency test uses real `std::thread::spawn` (not faked)

**Location**: `tests/unit/vertex_builder.rs:432-455`

**Verification**: The test spawns 2 real OS threads via `std::thread::spawn`, each doing 10 `commit()` calls. The threads share `Arc<GrafeoLoroApp>` and `Arc<Mutex<Vec<...>>>`. This exercises real concurrency (not a simulated sequential approximation). The test is deterministic across 5 runs (verified by hunter).

**Severity**: ACCEPTABLE — real concurrency test; no fix needed.

---

### ACCEPTABLE 7 — Test fixtures construct fresh `GrafeoLoroApp` per test (no state leaks)

**Location**: `tests/unit/vertex_builder.rs:111-134`

**Verification**: Both `build_app()` and `build_app_with_tiny_property_limit()` construct a fresh `GrafeoDB::new_in_memory()` + fresh `LoroDoc::new()` + fresh `SyncEngine::new(...)` + fresh `GrafeoLoroApp::from_sync_engine(...)`. Each test gets its own app, doc, db, and BridgeMaps. No state leaks between tests.

**Severity**: ACCEPTABLE — fresh fixtures per test; no fix needed.

---

### ACCEPTABLE 8 — `commit()` does not break any Phase 1 invariant

**Verification**:
1. **Echo prevention**: `commit()` sets `ORIGIN_LORO_BRIDGE` on the Loro commit (line 408) + uses `session_with_cdc(false)` (line 422). The B1 filter (line 215) skips `ORIGIN_LORO_BRIDGE`. ✅ (Filter logic correct, but see MAJOR 2 for test coverage gap.)
2. **BridgeMaps consistency**: `commit()` inserts via `apply_loro_op` → `maps.insert_node` which updates BOTH `node_id_map` and `node_key_map` in lock-step. ✅ (But see MAJOR 1 for step 6 failure cleanup gap.)
3. **Epoch side-channel**: `commit()` uses `session_with_cdc(false)`, so no CDC event is emitted, no epoch is added to `bridge_origin_epochs`. ✅
4. **Phase 1 tests**: All 5 integration tests + 6 lib tests pass. ✅

**Severity**: ACCEPTABLE — Phase 1 invariants preserved; no fix needed (but MAJOR 1 + MAJOR 2 should be fixed to make the invariants ROBUST).

---

## 2. Anti-plenger pattern coverage

| Pattern | Found? | Severity | Notes |
|---|---|---|---|
| Backward compatibility slaves | ❌ No | N/A | No backward compat concerns in P2T3 |
| Tautology (green tests, broken system) | ⚠️ Yes | MAJOR 2 | B1 filter is untested; tests pass without exercising it |
| Context Blindness | ⚠️ Partial | MAJOR 2 | Unit tests don't install subscriber; global echo-prevention architecture is not verified end-to-end |
| Band-Aids | ❌ No | N/A | L3 implemented the full 8-step algorithm, not a band-aid |
| Bloat (DRY Violations) | ⚠️ Minor | MINOR 1 | Test file uses literal `"V"` instead of `ROOT_VERTICES` |
| Hallucination | ❌ No | N/A | All API calls verified to exist (3 NIT off-by-one citations) |
| Happy-Path Bias | ⚠️ Yes | MAJOR 1, 3, 4 | Step 6 failure doesn't clean up BridgeMaps; `prepare_commit()?` and `begin_transaction()?` don't compensate Loro |
| Goodhart's Law | ⚠️ Partial | MAJOR 2 | Tests assert non-trivial conditions, but B1 filter is gamed (untested) |

---

## 3. Push-readiness verdict

**Finding count by severity**:
- **BLOCKER**: 0
- **MAJOR**: 4 (MAJOR 1: step 6 BridgeMaps cleanup; MAJOR 2: B1 filter untested; MAJOR 3: `prepare_commit()?` no compensate; MAJOR 4: `begin_transaction()?` no compensate)
- **MINOR**: 4 (MINOR 1: literal `"V"`; MINOR 2: TOCTOU; MINOR 3: pending origin tag; MINOR 4: misleading comment)
- **NIT**: 2 (NIT 1: worklog citation; NIT 2: off-by-one citation)
- **ACCEPTABLE**: 8

**Verdict**: **LOOP BACK TO FIXER**

The 4 MAJOR findings all relate to the atomicity contract:
- MAJOR 1 is a real bug (step 6 failure leaves stale BridgeMaps binding) — fix is 1 line (`maps.remove_node(&loro_key)`)
- MAJOR 2 is a real test coverage gap (B1 filter is dead code in the test suite) — fix is a new test (~15 LOC) + a public accessor on `GrafeoLoroApp` for `sync_engine` OR move the test to `src/app.rs` as a `#[cfg(test)] mod tests`
- MAJOR 3 + 4 are theoretical failure paths (`prepare_commit` / `begin_transaction` failing) — fix is `match` instead of `?` (~5 LOC each)

Total fix size: ~30-40 LOC + 1 new test. All fixes are surgical and well-scoped.

**Top 5 findings** (priority order for L2-R2 fixer):
1. **MAJOR 1**: Add `self.sync_engine.maps().remove_node(&loro_key);` to step 6 error arm (`src/app.rs:456`)
2. **MAJOR 2**: Add `vertex_builder_commit_does_not_echo_through_subscriber` test + expose `sync_engine` accessor on `GrafeoLoroApp` (or move test to lib internal)
3. **MAJOR 3**: Convert `session.prepare_commit()?` to `match` with `compensate_loro_vertex` on Err (`src/app.rs:447`)
4. **MAJOR 4**: Convert `session.begin_transaction()?` to `match` with `compensate_loro_vertex` on Err (`src/app.rs:423`)
5. **MINOR 1**: Replace literal `"V"` with `ROOT_VERTICES` constant in test file (5 occurrences)

---

## 4. Files touched by this critique

- `docs/critiques/p2t3-hunt.md` (NEW — this file)
- `worklog.md` (APPEND — P2T3-HUNT entry)

**No `src/` or `tests/` files modified** (Hunter read-only mandate).
