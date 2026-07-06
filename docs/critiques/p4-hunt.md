# P4 Plenger-Traits Hunt

**Hunter**: P4-HUNT agent
**Subject**: Phase 4 work (P4-L1 → P4-DEVIL → P4-L2 → P4-L3)
**Date**: 2026-07-06
**Branch**: `phase-4`
**Range**: `3272017..HEAD` (Phase 3 close → `3352164` P4-L3 worklog entry)
**Critique artifact**: this file (`docs/critiques/p4-hunt.md`)
**Method**: read-only verification against `grafeo-engine-0.5.42` / `grafeo-0.5.42` / `loro-1.13.6` / `loro-internal-1.13.6` / `loro-common-1.13.1` / `lorosurgeon-0.2.1` / `lz4_flex-0.11.6` / `zstd-0.13.3` source in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`, plus `grafeo-loro` `src/` + `tests/` + `docs/critiques/p4-l1-devil.md` + `docs/critiques/p2t3-hunt.md` (voice/format reference) + `worklog.md` Phase 4 entries (lines 4995–5380). Hunter touched NO `src/` or `tests/` files (read-only mandate); only this critique file and `worklog.md` were modified.

---

## Summary

Phase 4's overall quality is **high** and the loop is **ready to push**. Every API citation in the L1/L2/L3 worklog entries was independently re-verified line-for-line against the actual crate source under `~/.cargo/registry/src/index.crates.io-*/` — zero hallucinations (plenger-traits #6: 0 findings). The cold-boot round-trip test in `tests/unit/hydrate_checkpoint.rs:142-268` is a **real** integration test: it creates a vertex with properties, checkpoints REAL bytes through REAL Zstd compression + REAL `LoroDoc::export(ExportMode::shallow_snapshot)`, builds a FRESH app over the same storage, hydrates via REAL `LoroDoc::import_with(ORIGIN_LORO_BRIDGE)`, and asserts (a) `loro_key_counter == 1` (re-seed), (b) the vertex re-appears in the new `LoroDoc` with its `Person` label + `name: "Alix"` property, (c) `BridgeMaps::node_id_map` is non-empty, (d) `inbound_event_count` is UNCHANGED while `inbound_filtered_count` INCREMENTED — proving the B1 echo-filter fired. This is the anti-tautology gold standard; plenger-traits #2 (tautology) and #8 (Goodhart's Law) both have **0 findings**.

The `from_sync_engine` shim (Devil M8's "non-breaking constructor") is NOT a backward-compat slave — it delegates to `from_sync_engine_with_config` with `SsotMode::default()` / `None` / `CompressionType::default()` and is used only by 2 `tests/unit/vertex_builder.rs` sites that don't exercise `hydrate`/`checkpoint`. The doc-comment explicitly warns "Callers that exercise `hydrate`/`checkpoint` MUST use the explicit constructor (storage `None` will fail at first dispatch)" — the failure is detected at first dispatch via `Config("storage backend not set")`. Plenger-traits #1 (backward-compat slaves): **0 findings**.

Concurrency hygiene is clean: no `parking_lot::RwLock` or `std::sync::RwLock` write/read guard is held across any `.await` in `src/app.rs`. The `loro_key_counter.fetch_max(max + 1, Ordering::Relaxed)` (plenger-traits #3 context-blindness flagged item) is correct — `Relaxed` is sufficient because the counter carries no accompanying memory payload that requires Release/Acquire synchronization; each `VertexBuilder::commit` does its own `fetch_add(1, Relaxed)` and reads its own result. Plenger-traits #3 (context blindness): **0 findings**. Production code has **zero** `unwrap()`/`expect()`/`panic!()` outside `#[cfg(test)]` modules and `unimplemented!()` arms for P5-deferred paths — plenger-traits #7 (happy-path bias): **0 findings** on the panic axis.

The remaining findings are **3 MINOR + 1 NIT**, none of which block the loop. Top 3 risks (all MINOR): (1) `build()` discards the `Vec<JoinHandle<()>>` from `spawn_all` while its doc-comment claims "the caller (orchestrator) is responsible for awaiting them on shutdown" — a doc/code mismatch that P5's `shutdown().await` will need to reconcile; (2) `hydrate`'s delta-load `continue` swallows ALL `storage.load` errors instead of just `NotFound` — harmless in Phase 4 (no delta-write path) but a forward-compat risk for Phase 5+ Loro sync wire; (3) `InMemoryStorage` impl duplicated across 2 test files (~70 lines each). **Recommendation: PROCEED TO PUSH.**

---

## Findings by Plenger-Trait

### 1. Backward-compat slaves

**0 findings.**

The `from_sync_engine` shim at `src/app.rs:120-127` was the prime suspect (Devil M8 wanted state fields added; L2 kept the old constructor as a "non-breaking shim"). Verified:

- **Delegates, does not duplicate**: the shim body is 5 lines that delegate to `from_sync_engine_with_config(sync_engine, SsotMode::default(), None, CompressionType::default())`. No legacy code path preserved.
- **Test-only usage**: `rg -n "from_sync_engine\b" src/ tests/` shows 2 call sites, both in `tests/unit/vertex_builder.rs` (lines 116, 133). These tests don't exercise `hydrate`/`checkpoint` — they only test `VertexBuilder::commit`. The `None` storage is therefore correct for their scope.
- **Defensive failure**: production callers that accidentally use the shim hit `Config("storage backend not set")` at the first `hydrate`/`checkpoint` dispatch — no silent legacy rot.
- **Doc-warning**: the shim's doc-comment at `:115-119` explicitly says "Callers that exercise `hydrate`/`checkpoint` MUST use the explicit constructor (storage `None` will fail at first dispatch)."

No `// DEPRECATED` or `// LEGACY` comments in P4-touched code (`rg -n "DEPRECATED|LEGACY" src/ tests/` returns only one hit in `docs/critiques/p2t3-hunt.md` — unrelated). No code paths route around new code to keep old behavior.

### 2. Tautology

**0 findings.**

The cold-boot round-trip test at `tests/unit/hydrate_checkpoint.rs:142-268` (`cold_boot_roundtrip_loro_mode`) was the prime suspect. Verified end-to-end:

| Test step | Verification | Real? |
|---|---|---|
| Creates a vertex with `with_label("Person").with_property("name", GraphValue::String("Alix".into()))` | Lines 154-159 | ✅ Real vertex with real properties |
| `checkpoint("test-graph")` writes REAL bytes through `CompressedPayload::compress_to_wire` (Zstd) + `LoroDoc::export(ExportMode::shallow_snapshot)` | Line 169-171; production code at `src/app.rs:357-377` | ✅ Real export + real compression |
| Storage actually receives the wire-format bytes (`stored_keys.contains(&expected_key)` + `stored_bytes.len() >= 2` + `decompress_from_wire` succeeds) | Lines 173-193 | ✅ Bytes hit the storage backend |
| Fresh app over the SAME storage (`build_app_with_storage(storage.clone())`) — NOT reusing `app`'s state | Line 196 | ✅ Real cold boot |
| `app2.hydrate("test-graph")` runs | Lines 204-206 | ✅ Real hydrate |
| `loro_key_counter() == 1` (re-seed from `V/0` key) | Lines 211-215 | ✅ Real re-seed verification |
| Vertex re-appears in new `LoroDoc`: `v_map.get(&loro_key)` + `VertexEntity::hydrate_map(&node_map)` + `labels.iter().any(\|l\| l == "Person")` + `properties.get("name") == Some(LoroProperty::String("Alix"))` | Lines 217-239 | ✅ Real vertex recovery — labels + properties verified |
| `BridgeMaps::node_id_map` non-empty after hydrate | Lines 246-249 | ✅ Real binding check |
| `inbound_event_count` UNCHANGED (no echo reached inbound channel) | Lines 257-261 | ✅ Real no-echo assertion |
| `inbound_filtered_count` INCREMENTED (B1 filter fired) | Lines 262-267 | ✅ Real filter-firing assertion |

The test is the **anti-tautology gold standard**: it exercises REAL production code (REAL Zstd compression, REAL Loro export/import, REAL `parallel_hydrate_grafeo`), and the assertions verify observable state (vertex labels + properties + counter + filter counts), not just "no error returned."

The builder-validation tests at `tests/unit/builder_validation.rs` use `assert_config_err(result, needle)` which asserts the error MESSAGE contains the needle (e.g. `"batch_interval_ms"`, `"storage backend not set"`) — not just that `Err` is returned. The positive control `build_accepts_valid_loro_config` proves the rejection tests aren't just rejecting EVERY config (anti-tautology).

The compression-payload tests at `tests/unit/compression_payload.rs` include `compression_wire_to_wire_from_wire_symmetric` which verifies `to_wire` followed by `from_wire` recovers the original `CompressedPayload` struct byte-for-byte (codec + raw_data) — NOT just the decompressed bytes. The LZ4/Zstd round-trip tests assert `assert_ne!(&wire[2..], INPUT)` to prove the codec actually transformed the input (anti-Goodhart — a no-op codec would pass a naive round-trip test).

### 3. Context Blindness

**0 findings.**

Verified concurrency hygiene in `src/app.rs` + `src/bridge/sync_engine.rs`:

- **No RwLock guard held across `.await`**: all 6 `doc.read()`/`doc.write()` sites in `hydrate`/`checkpoint` are scoped in `{}` blocks that close BEFORE any `storage.load/save/list/delete.await`. Specifically:
  - `checkpoint` `:339-342` — `doc.read()` for `oplog_frontiers()`, scoped.
  - `checkpoint` `:357-361` — `doc.read()` for `export(...)`, scoped.
  - `hydrate` `:614-637` — `doc.write()` for `import_with(...)`, scoped.
  - `hydrate` `:682-684` — `doc.write()` for delta `import_with(...)`, scoped.
  - `hydrate` `:713-720` — `doc.read()` for `parallel_hydrate_grafeo`, scoped.
  - `hydrate` `:739-767` — `doc.read()` for `get_map().keys()` scan, scoped.
- **`loro_key_counter.fetch_max(max + 1, Ordering::Relaxed)`** at `src/app.rs:753`: `Relaxed` is correct. The counter is a bare `u64` with no accompanying memory payload — `Relaxed` guarantees atomicity (the read-modify-write is indivisible) without imposing a memory-ordering fence. Each `VertexBuilder::commit` does its own `fetch_add(1, Relaxed)` and reads its own return value, so cross-thread visibility of the counter value is not required (each thread observes its own increment). `Release`/`Acquire` would add fences for no benefit. Pre-existing precedent: `src/bridge/sync_engine.rs:282` uses `Relaxed` for `inbound_event_count.fetch_add` — same pattern.
- **`init_loro_subscriber`** at `src/bridge/sync_engine.rs:240-289` holds `doc.read()` (parking_lot) while constructing the handler closure + calling `doc.subscribe_root(handler)` + storing the subscription in `self.loro_sub` (parking_lot::Mutex). All synchronous — no `.await` while holding the guard. The handler closure itself uses `inbound_tx.try_send(...)` (NOT async send) so it cannot block the runtime.
- **Storage backend calls all awaited**: `storage.load(k).await`, `storage.save(k, b).await`, `storage.list(p).await`, `storage.delete(k).await` — all 6 storage call sites in `hydrate`/`checkpoint` use `.await`. No fire-and-forget.
- **No `std::sync::RwLock` in async context**: the codebase uses `parking_lot::RwLock` (which is non-poisoning and never blocks the runtime — its read/write methods are sync). `std::sync::Mutex` is used only in the test `InMemoryStorage` impl (`tests/unit/hydrate_checkpoint.rs:53` + `tests/unit/builder_validation.rs:44`) inside `async fn` methods — but `Mutex::lock()` on a `std::sync::Mutex` held briefly (HashMap lookup/insert) is acceptable for in-memory test backends. Production storage backends (S3, filesystem) will be async-native.

### 4. Band-Aids

**1 finding (MINOR).**

#### BA-1: `hydrate` delta-load `continue` swallows ALL `storage.load` errors, not just `NotFound`

**Symptom**: The delta-import loop in `hydrate` has a `match storage.load(k).await { Ok(b) => b, Err(e) => { tracing::warn!(...); continue; } }` arm that swallows every `io::Error` variant — including permanent failures like `PermissionDenied`, `ConnectionReset`, `CorruptedData` — not just the `NotFound` race the comment justifies.

**Location**: `src/app.rs:664-678`

**Impact**: In Phase 4 this is harmless — Devil M1 established that Phase 4 has no delta-write path, so `storage.list(delta_prefix)` always returns `Ok(vec![])` and the loop body never runs. The band-aid becomes a real defect in Phase 5+ when the Loro sync wire-protocol path populates delta keys: a permanent storage failure on a delta would be silently skipped, producing a silently-incomplete hydrate. The `tracing::warn!` log entry would be the only signal — easy to miss in production logs.

**Fix**: Distinguish `NotFound` (the documented race — another writer checkpointed between `list` and `load`) from other errors (propagate):

```rust
let delta_bytes = match storage.load(k).await {
    Ok(b) => b,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
        tracing::warn!(
            key = %k,
            "hydrate: delta vanished between list and load (concurrent checkpoint); skipping"
        );
        continue;
    }
    Err(e) => return Err(GrafeoLoroError::from(e)),
};
```

**Severity**: MINOR (no Phase 4 impact; forward-compat risk for Phase 5+).

**Note**: The `checkpoint` delta-delete at `src/app.rs:401-407` (`if let Err(e) = storage.delete(k).await { tracing::warn!(...); }`) is NOT a band-aid — it is explicitly justified by Devil Q3 (orphan-delta risk accepted; next checkpoint retries; orphan deltas are re-imported harmlessly by `hydrate` via `trim_the_known_part_of_change`). The asymmetric treatment (delete-failure recoverable, load-failure not) is correct: a failed delete leaves an orphan delta that the next checkpoint cleans up; a failed load (non-NotFound) means the delta exists but is unreadable — silently skipping it produces a state-divergent hydrate.

### 5. Bloat (DRY Violations)

**2 findings (1 MINOR, 1 NIT).**

#### DRY-1: `InMemoryStorage` impl duplicated across 2 test files

**Symptom**: The `InMemoryStorage` struct + `StorageBackend` impl (~70 lines: `Mutex<HashMap<String, Vec<u8>>>` + `load`/`save`/`list`/`delete` methods) is copy-pasted verbatim between `tests/unit/hydrate_checkpoint.rs:47-121` and `tests/unit/builder_validation.rs:40-96`. The `builder_validation.rs` copy even acknowledges the duplication in its doc-comment: "Mirrors `hydrate_checkpoint::InMemoryStorage` — duplicated here to keep each test module self-contained per the existing test-suite convention."

**Location**: `tests/unit/hydrate_checkpoint.rs:47-121` + `tests/unit/builder_validation.rs:40-96`

**Impact**: A bug fix or feature addition (e.g. adding a `clear()` method, or fixing a `Mutex` poisoning handling difference) must be applied in both files. The two copies have already diverged slightly: `hydrate_checkpoint.rs` has a `keys()` accessor (line 64-74) for test assertions; `builder_validation.rs` does not.

**Fix**: Extract to `tests/common/mod.rs` (or `tests/common/in_memory_storage.rs`) and `mod common;` from both test files. The "self-contained test module" convention is a real consideration but does not justify ~70 lines of verbatim duplication — the convention can be preserved by re-exporting from a shared module.

**Severity**: MINOR.

#### DRY-2: `format!("{graph_id}/{STORAGE_KEY_*}")` composed inline at 4 sites

**Symptom**: The storage-key composition `format!("{graph_id}/{STORAGE_KEY_BASE_LORO}")` (and the `STORAGE_KEY_DELTA_PREFIX` variant) is written inline at 4 call sites in `src/app.rs` (lines 375, 388, 584, 646) plus several doc-comment references. The `STORAGE_KEY_*` constants are the SSOT for the suffix portion (good), but the `{graph_id}/` prefix composition is repeated.

**Location**: `src/app.rs:375, 388, 584, 646` (call sites); `src/constants.rs:77-79` (doc-comment establishing the convention).

**Impact**: Low. The composition is a 1-line `format!` and the SSOT for the suffix is already a named constant. A bug in the composition (e.g. accidental `format!("{graph_id}{STORAGE_KEY_BASE_LORO}")` missing the `/`) would need to be caught at 4 sites. A helper `fn storage_key(graph_id: &str, suffix: &str) -> String { format!("{graph_id}/{suffix}") }` would centralize the convention.

**Fix**: Optional. Add a `pub(crate) fn storage_key(graph_id: &str, suffix: &str) -> String` helper in `src/constants.rs` or `src/storage/mod.rs` and use it at the 4 sites. Acceptable as-is given the 1-line clarity.

**Severity**: NIT.

**Note**: `CompressedPayload` does NOT duplicate logic from elsewhere — `compress`/`decompress`/`to_wire`/`from_wire`/`compress_to_wire`/`decompress_from_wire` are all defined in `src/compression/wrapper.rs` and are the SSOT for the compression envelope. The `LoroDocCompressionExt` trait (`wrapper.rs:168-205`) is unused by Phase 4 work (it was a Phase 3 scaffold for a different code path) but is not a DRY violation — it composes `CompressedPayload::compress`/`decompress` rather than duplicating codec dispatch.

### 6. Hallucination

**0 findings.**

Every API call in the P4-L1/L2/L3 diff was independently verified against the actual crate source in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`. Method: `git diff 3272017..HEAD -- src/ | grep "^+" | grep -E "\.[a-z_]+\(" | grep -oE "\.[a-z_]+\(" | sort -u` produced 39 unique method-call tokens; each was checked against the relevant crate source.

Verified API citations (file:line in crate source):

| Call | Crate source location |
|---|---|
| `LoroDoc::oplog_frontiers(&self) -> Frontiers` | `loro-1.13.6/src/lib.rs:948` ✅ |
| `LoroDoc::import_with(&self, &[u8], &str) -> Result<ImportStatus, LoroError>` | `loro-1.13.6/src/lib.rs:721` ✅ |
| `LoroDoc::export(&self, ExportMode) -> Result<Vec<u8>, LoroEncodeError>` | `loro-1.13.6/src/lib.rs:1306` ✅ |
| `LoroDoc::get_map<I: IntoContainerId>(&self, I) -> LoroMap` | `loro-1.13.6/src/lib.rs:489` ✅ |
| `LoroDoc::set_next_commit_origin(&self, &str)` | `loro-1.13.6/src/lib.rs:626` ✅ |
| `LoroDoc::commit(&self)` | `loro-1.13.6/src/lib.rs:593` ✅ |
| `LoroMap::keys(&self) -> impl Iterator<Item = InternalString>` | `loro-1.13.6/src/lib.rs:2315` ✅ |
| `LoroMap::ensure_mergeable_map(&self, &str) -> LoroResult<LoroMap>` | `loro-1.13.6/src/lib.rs:2247` ✅ |
| `ExportMode::shallow_snapshot(&Frontiers) -> Self` | `loro-internal-1.13.6/src/encoding.rs:108` ✅ |
| `InternalString: AsRef<str>` | `loro-common-1.13.1/src/internal_string.rs:127` ✅ |
| `GrafeoDB::new_in_memory() -> Self` | `grafeo-engine-0.5.42/src/database/mod.rs:267` ✅ |
| `GrafeoDB::open(path)` — `#[cfg(feature = "wal")]`-gated | `grafeo-engine-0.5.42/src/database/mod.rs:289-291` ✅ (correctly avoided by L2 per Devil B1) |
| `GrafeoDB::with_config(Config) -> Result<Self>` | `grafeo-engine-0.5.42/src/database/mod.rs:346` ✅ |
| `GrafeoDB::session_with_cdc(bool) -> Session` | `grafeo-engine-0.5.42/src/database/mod.rs:1728` ✅ |
| `grafeo::Config::persistent(impl Into<PathBuf>) -> Self` | `grafeo-engine-0.5.42/src/config.rs:435` ✅ |
| `grafeo::Config` + `grafeo::GrafeoDB` re-exports | `grafeo-0.5.42/src/lib.rs:69` ✅ |
| `RootReconciler::new(LoroMap) -> Self` | `lorosurgeon-0.2.1/src/reconcile.rs:298` ✅ |
| `<VertexEntity as Reconcile>::reconcile<R: Reconciler>` | `lorosurgeon-0.2.1/src/reconcile.rs:92` ✅ |
| `<VertexEntity as Hydrate>::hydrate_map(&LoroMap)` | `lorosurgeon-0.2.1/src/hydrate.rs:64` ✅ |
| `lz4_flex::compress_prepend_size(&[u8]) -> Vec<u8>` | `lz4_flex-0.11.6/src/block/compress.rs:713` ✅ |
| `lz4_flex::decompress_size_prepended(&[u8]) -> Result<Vec<u8>, DecompressError>` | `lz4_flex-0.11.6/src/block/decompress.rs:496` ✅ |
| `zstd::stream::encode_all<R: io::Read>(R, i32) -> io::Result<Vec<u8>>` | `zstd-0.13.3/src/stream/functions.rs:32` ✅ |
| `zstd::stream::decode_all<R: io::Read>(R) -> io::Result<Vec<u8>>` | `zstd-0.13.3/src/stream/functions.rs:8` ✅ |
| `AtomicU64::fetch_max(&self, u64, Ordering) -> u64` | std (verified via existing usage at `src/bridge/sync_engine.rs:282` + `grafeo-engine-0.5.42/src/transaction/manager.rs:461`) ✅ |
| `SyncEngine::with_batch_config(db, doc, batch_size, batch_ms)` | `src/bridge/sync_engine.rs:170` ✅ (P4-L2 addition; `new` delegates to private `new_inner` per DRY) |
| `SyncEngine::spawn_all(self: Arc<Self>, inbound_rx, outbound_rx) -> Vec<JoinHandle<()>>` | `src/bridge/sync_engine.rs:439` ✅ |
| `SyncEngine::init_loro_subscriber(&self) -> Result<()>` | `src/bridge/sync_engine.rs:240` ✅ (infallible in practice — returns `Ok(())` unconditionally) |

No `unsafe` blocks, no `transmute`, no fabricated types, no speculative trait impls. The `CompressedPayload` wire format (`[version:u8][codec_tag:u8][raw_data..]`) is a real implementation backed by real codec calls — not a hallucinated envelope.

### 7. Happy-Path Bias

**0 findings on the panic axis. 1 finding (MINOR) reclassified under Context Blindness (CB-1 below).**

`rg -n "\.unwrap\(\)|\.expect\(|panic!\(" src/` returns 9 hits, ALL inside `#[cfg(test)]` modules (`src/types/values.rs:257-291`, `src/bridge/sync_engine.rs:813`). Production code has zero `unwrap()`/`expect()`/`panic!()` outside the `unimplemented!("P5: ...")` arms for deferred `SsotMode::Grafeo` paths (which are explicit deferrals, not happy-path bias).

Defensive programming is present:
- `build()` validates 4 config conditions (`batch_interval_ms == 0`, `batch_max_size == 0`, `storage == None`, `SsotMode::Grafeo + grafeo_dir == None`) with `Config(...)` errors before any allocation.
- `hydrate`/`checkpoint` reject `None` storage at dispatch time (defensive — `build()` also rejects this).
- `hydrate`'s `storage.load` distinguishes `NotFound` (fresh-graph path, returns `Vec::new()`) from other errors (propagated via `?`).
- `CompressedPayload::from_wire` rejects `bytes.len() < 2`, unknown versions, and unknown codec tags with `Compression(...)` errors — no silent mis-decode.
- `VertexBuilder::commit` strict-rejects `Vector`/`Map`/`List` properties BEFORE any Loro/Grafeo write.
- `compensate_loro_vertex` handles the Loro-compensation-failure case with `tracing::error!` + full context (loro_key, labels, properties, both errors).

#### CB-1 (reclassified from Happy-Path Bias): `build()` discards `Vec<JoinHandle<()>>` — doc/code mismatch

**Symptom**: `GrafeoLoroAppBuilder::build` at `src/app.rs:1044` does `let _join_handles = engine.clone().spawn_all(inbound_rx, outbound_rx).await;` — the `Vec<JoinHandle<()>>` is bound to `_join_handles` and dropped at the end of `build()`. The doc-comment at `:967-973` explicitly says: "Returns the three `JoinHandle`s; the caller (orchestrator) is responsible for awaiting them on shutdown." But `build()` returns `Result<GrafeoLoroApp>`, NOT `Result<(GrafeoLoroApp, Vec<JoinHandle<()>>)>` — the orchestrator never receives the handles.

**Location**: `src/app.rs:1044` (code); `src/app.rs:967-973` (misleading doc-comment).

**Impact**: The 3 spawned tasks (inbound worker, outbound worker, CDC poller) are detached immediately after `build()` returns. They continue running until either (a) the tokio runtime shuts down, or (b) `engine.shutdown_tx.send(())` is called via `app.sync_engine().shutdown()`. The orchestrator CANNOT `await` the handles to confirm graceful shutdown completion — only fire-and-forget. This is a forward-compat issue for P5's `GrafeoLoroApp::shutdown(self).await` (currently `unimplemented!` at `src/app.rs:796-798`): to implement graceful shutdown with completion confirmation, P5 will need to either store the handles on `GrafeoLoroApp` (e.g. `pub(crate) join_handles: Vec<JoinHandle<()>>`) or refactor `build()` to return them.

**Comparison to existing pattern**: `tests/integration/sync_echo.rs:73,197,311,412` use `let handles = engine.clone().spawn_all(...)` and keep `handles` in scope for the test function duration (preventing detach). The P4-L2 `build()` does NOT follow this pattern — it explicitly discards.

**Fix** (P5 scope): Either (a) store the handles on `GrafeoLoroApp` and add a `pub async fn shutdown(self) -> Result<()>` that calls `engine.shutdown()` + `await`s all handles; OR (b) update the `:967-973` doc-comment to acknowledge the detach pattern ("JoinHandles are detached; the orchestrator signals shutdown via `engine.shutdown()` and the runtime cancels the tasks on drop"). Option (a) is the cleaner fix; option (b) is the minimal doc-fix.

**Severity**: MINOR (no Phase 4 functional impact — the validation test is sequential and the runtime cancels tasks on test teardown; the issue is a doc/code mismatch + P5 forward-compat).

### 8. Goodhart's Law in Action

**0 findings.**

The Phase 4 work does NOT take the shortest/laziest path to green tests. Evidence:

- **Cold-boot round-trip uses REAL codecs + REAL Loro export/import** (not mocked): the test at `tests/unit/hydrate_checkpoint.rs:142-268` runs `CompressedPayload::compress_to_wire(&snapshot_bytes, CompressionType::Zstd)` (REAL Zstd compression at level 3) + `LoroDoc::export(ExportMode::shallow_snapshot(&frontiers))` (REAL Loro shallow-snapshot encoding) + `LoroDoc::import_with(&loro_bytes, ORIGIN_LORO_BRIDGE)` (REAL Loro import). A regression in any of these production code paths would break the test.
- **Anti-Goodhart assertions in compression tests**: `compression_wire_roundtrip_lz4` and `compression_wire_roundtrip_zstd` at `tests/unit/compression_payload.rs:62-95` assert `assert_ne!(&wire[2..], INPUT)` — a no-op codec (e.g. accidentally falling through to the `None` arm) would FAIL this assertion. The test forces the codec to actually transform the input.
- **Anti-tautology symmetry test**: `compression_wire_to_wire_from_wire_symmetric` at `tests/unit/compression_payload.rs:184-196` verifies `to_wire` followed by `from_wire` recovers the original `CompressedPayload` struct (codec + raw_data) byte-for-byte — NOT just the decompressed bytes. A bug in `to_wire` (e.g. wrong tag mapping) or `from_wire` (e.g. wrong slice range) would fail this.
- **Builder validation has a positive control**: `build_accepts_valid_loro_config` at `tests/unit/builder_validation.rs:163-179` proves the rejection tests aren't just rejecting EVERY config (which would be a tautology — `build()` always errors). The positive control asserts `app.ssot_mode() == SsotMode::Loro` and `app.compression() == CompressionType::Zstd` — verifying the builder actually threads the slots through.
- **No hardcoded expected values that mirror production logic 1:1**: the cold-boot test asserts `loro_key_counter() == 1` based on the test's own setup (1 vertex committed → max V/* key = 0 → `fetch_max(0+1)` → counter = 1). The expected value `1` is computed from the test's setup, not copy-pasted from the production formula. If the production re-seed algorithm regressed to `fetch_max(max, Relaxed)` (off-by-one), the test would catch it (counter would be 0, not 1).
- **Error message verification, not just `Err`**: `assert_config_err(result, needle)` at `tests/unit/builder_validation.rs:99-110` asserts the error message CONTAINS the needle (e.g. `"batch_interval_ms"`, `"storage backend not set"`, `"grafeo_dir required for SsotMode::Grafeo"`). A regression that returned `Config("unknown error")` would fail. (Minor weakness: the needle is a substring, not the full message — but sufficient to catch the documented error variants.)

---

## Verifications Performed

### 1. Compile + test status

```bash
cd /home/z/my-project/workspace/grafeo-loro && . "$HOME/.cargo/env" && cargo check --all-targets 2>&1 | tail -10
```
Result: ✅ PASS — 2 pre-existing lib warnings (`presence::socket::room_id` never read; `telemetry::health` fields `doc`/`db`/`last_sync_ts` never read — both unchanged from P4-L1 baseline), 0 errors, 0 new warnings from P4 work.

```bash
cd /home/z/my-project/workspace/grafeo-loro && . "$HOME/.cargo/env" && cargo test --all 2>&1 | grep "test result"
```
Result: ✅ PASS — `6 passed; 0 failed; 0 ignored` (lib) + `5 passed; 0 failed; 0 ignored` (integration) + `59 passed; 0 failed; 2 ignored` (unit — 2 ignored are pre-existing: `generate_local_embedding_logs_onnx_warning` smoke + `parallel_hydrate_10k_nodes_under_500ms` benchmark) + `0 passed; 0 failed; 0 ignored` (doc-tests) = **70 PASS + 0 FAIL + 2 IGNORED**. Matches the P4-L3 worklog claim exactly.

### 2. Hallucination sweep — every API call in P4 diff

```bash
cd /home/z/my-project/workspace/grafeo-loro && git diff 3272017..HEAD -- src/ | grep "^+" | grep -E "\.[a-z_]+\(" | grep -oE "\.[a-z_]+\(" | sort -u
```
Result: 39 unique method-call tokens. Each verified against `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/` source via `rg -n "fn <name>"`. See §6 (Hallucination) for the full citation table. **Zero hallucinations.**

### 3. Band-aid sweep — silent error swallowing

```bash
cd /home/z/my-project/workspace/grafeo-loro && rg -n "\.ok\(\)|unwrap_or_default|unwrap_or\(|if let Err.*continue|let _ = " src/ | grep -v test
```
Result: 26 hits. Classification:
- **P4-introduced (1 hit — BA-1)**: `src/app.rs:664-677` `Err(e) => { tracing::warn!(...); continue; }` in `hydrate` delta-load — swallows ALL load errors, not just NotFound. Flagged MINOR.
- **P4-introduced (1 hit — justified)**: `src/app.rs:401-407` `if let Err(e) = storage.delete(k).await { tracing::warn!(...); }` in `checkpoint` delta-delete — justified per Devil Q3 (orphan-delta risk accepted; next checkpoint retries). NOT a band-aid.
- **Pre-existing (24 hits)**: `src/app.rs:207,213,225,791` (`let _ = gql;` etc. on `unimplemented!()` stubs — placeholder arg suppression), `src/presence/socket.rs:13,19,25,31`, `src/telemetry/traces.rs:6,12,18`, `src/telemetry/metrics.rs:20,26,32`, `src/telemetry/health.rs:25,37`, `src/bridge/sync_engine.rs:304,326,446,457,672,689,710,746,763`, `src/hydration/parallel.rs:76-77`. None introduced by P4-L1/L2/L3; not in scope.

### 4. Happy-path bias sweep — panics in production

```bash
cd /home/z/my-project/workspace/grafeo-loro && rg -n "\.unwrap\(\)|\.expect\(|panic!\(" src/ | grep -v test | grep -v "unimplemented!"
```
Result: 0 hits in production code. All 9 `unwrap()`/`expect()`/`panic!()` occurrences in `src/` are inside `#[cfg(test)]` modules (`src/types/values.rs:257-291`, `src/bridge/sync_engine.rs:813`).

### 5. DRY sweep — compress/decompress + storage key

```bash
cd /home/z/my-project/workspace/grafeo-loro && rg -n "fn compress|fn decompress" src/ tests/
```
Result: `CompressedPayload::{compress, decompress, compress_to_wire, decompress_from_wire}` all defined in `src/compression/wrapper.rs:33, 58, 125, 133` (SSOT). No duplication. The `LoroDocCompressionExt` trait at `wrapper.rs:168-205` composes `CompressedPayload::compress`/`decompress` — NOT duplicating codec dispatch.

```bash
cd /home/z/my-project/workspace/grafeo-loro && rg -n "format!\(\"\{graph_id\}" src/app.rs
```
Result: 4 inline compositions (`src/app.rs:375, 388, 584, 646`). Flagged as DRY-2 NIT — acceptable as-is (1-line `format!` with SSOT suffix constant).

### 6. Tautology sweep — cold-boot round-trip test

```bash
cd /home/z/my-project/workspace/grafeo-loro && cat tests/unit/hydrate_checkpoint.rs
```
Result: Test `cold_boot_roundtrip_loro_mode` (lines 142-268) verified end-to-end. See §2 (Tautology) for the full step-by-step verification table. **REAL test — no tautology.**

### 7. Context blindness sweep — async/concurrency

```bash
cd /home/z/my-project/workspace/grafeo-loro && rg -n "\.await" src/app.rs src/bridge/sync_engine.rs src/bridge/batcher.rs
```
Result: 16 `.await` sites in `src/app.rs` (all on `storage.load/save/list/delete` — no RwLock guard held across any of them). Verified each `doc.read()`/`doc.write()` site is scoped in `{}` blocks that close before any `.await`. `loro_key_counter.fetch_max(max + 1, Ordering::Relaxed)` — `Relaxed` ordering correct (bare u64, no accompanying memory payload). See §3 (Context Blindness) for the full analysis.

### 8. Backward-compat slave sweep

```bash
cd /home/z/my-project/workspace/grafeo-loro && rg -n "from_sync_engine\b" src/ tests/
```
Result: 2 call sites, both in `tests/unit/vertex_builder.rs` (lines 116, 133). The shim delegates to `from_sync_engine_with_config` with defaults (not duplicating). Doc-comment warns production callers MUST use the explicit constructor. **NOT a backward-compat slave.**

### 9. Branch + commit verification

```bash
cd /home/z/my-project/workspace/grafeo-loro && git branch --show-current && git log --oneline -5
```
Result: ✅ on `phase-4` branch. Latest commit `3352164` (P4-L3 worklog entry). Range `3272017..HEAD` covers P4-L1 (`818d5c5`) + P4-DEVIL (`a49b892`) + P4-L2 (`d528e47`) + P4-L3 (`49331d7` + `61c9bad` + `3352164`).

### 10. No PAT leakage

Verified: this file contains no GitHub personal-access token literal. (The `git push` command uses the PAT via the `x-access-token:` URL scheme on the CLI, not committed to the file.)

---

## L2 Re-Entry Recommendation

Per `plonga-plongo-loop.md` step 6: "Back to 3 if issues found, else push $stn".

**Recommendation**: **PROCEED TO PUSH**

**Rationale**: The Phase 4 work is high quality across all 8 plenger-traits categories:

- **0 BLOCKERs**, **0 MAJORs**, **3 MINORs** (BA-1, DRY-1, CB-1), **1 NIT** (DRY-2).
- 4 of 8 categories have **0 findings** (Backward-compat slaves, Tautology, Context Blindness, Hallucination, Goodhart's Law — 5 of 8 actually).
- The cold-boot round-trip test is the anti-tautology gold standard — REAL bytes through REAL codecs + REAL Loro export/import, with REAL assertions on observable state (vertex labels + properties + counter + filter counts).
- Every API call in the P4 diff was independently re-verified against the actual crate source — zero hallucinations.
- All 70 tests pass; 0 regressions; 0 new warnings from P4 work.
- Production code has zero `unwrap()`/`expect()`/`panic!()` outside `unimplemented!()` P5-deferred arms.

The 3 MINOR findings are all forward-compat issues for Phase 5 (not Phase 4 defects):

1. **BA-1** (delta-load `continue` swallows non-NotFound errors): harmless in Phase 4 because the delta-listing is always empty (Devil M1 — no delta-write path). Becomes a real defect in Phase 5+ when the Loro sync wire path populates delta keys.
2. **DRY-1** (`InMemoryStorage` duplicated across 2 test files): test-code duplication, not production code. ~70 lines copy-pasted. Cosmetic.
3. **CB-1** (`build()` discards `Vec<JoinHandle<()>>`): doc/code mismatch. The 3 spawned tasks are detached but still running — they exit on `engine.shutdown()` or runtime drop. Phase 4 validation test is sequential and unaffected. P5's `shutdown().await` will need to reconcile (either store handles on `GrafeoLoroApp` or update the doc-comment).

None of these MINORs block the loop close. They can be addressed in a Phase 5 L1 scaffolding pass (when `SsotMode::Grafeo` is unblocked and the delta-write path + graceful shutdown are implemented) or as a Phase 4 follow-up patch if the orchestrator prefers.

The 1 NIT (DRY-2: inline `format!` for storage keys) is acceptable as-is — the SSOT for the suffix portion is already a named constant, and the 1-line `format!` composition is clearer than a helper function.

**Push `$stn`** (Phase 4 close).
