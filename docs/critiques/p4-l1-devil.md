# P4-L1 Devil's Advocate Critique

**Task ID**: P4-DEVIL
**Agent**: Devil's Advocate
**Subject**: P4-L1 scaffolding (commit `818d5c5`)
**Date**: 2026-07-06
**Branch**: `phase-4`
**Critique artifact**: this file (`docs/critiques/p4-l1-devil.md`)
**Method**: read-only verification against `grafeo-engine-0.5.42` / `grafeo-common-0.5.42` / `grafeo-core-0.5.42` / `grafeo-0.5.42` / `loro-1.13.6` / `loro-internal-1.13.6` / `lorosurgeon-0.2.1` source in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`, plus `grafeo-loro` `src/` + `docs/grafeo-loro.architecture.md` (§2/§4/§5/§6/§9/§14/§15/§16/§20/§24) + `docs/implementation-plan.md` Phase 4 + `docs/grafeo-loro.project-structure.md`. Devil touched NO `src/` or `tests/` files (read-only mandate); only this critique file and `worklog.md` were modified.

---

## Summary

P4-L1's overall quality is **high**: every API citation in the worklog was independently re-verified line-for-line (zero hallucinations — same accuracy bar as P2T3-L1), the storage-key SSOT constants in `src/constants.rs` are correctly cross-referenced to architecture §2/§4/§24.3, and the 73-line `checkpoint` / 103-line `hydrate` doc-comments are thorough walkthroughs of the 6-step dispatch on `SsotMode`. The decision to add NO new error variants (DRY reuse of existing `Config`/`StorageIo`/`Loro`/`Grafeo`/`Compression`/`Hydrate`/`Bridge`) is correct.

However, L1's contract has **two BLOCKERs that prevent L3 from compiling/working as-written**, and **one structural flaw in the `SsotMode::Grafeo` direction** that L1 did not surface. Top risks:

1. **B1 (`GrafeoDB::open` is `wal`-gated)** — L1's `checkpoint` + `hydrate` doc-comments both say "verified at `grafeo-engine-0.5.42/src/database/mod.rs:290`" for `GrafeoDB::open(path)`. The function DOES exist at that line — but it is annotated `#[cfg(feature = "wal")]`, and grafeo-0.5.42's default `embedded` feature set does NOT activate `wal` (verified at `grafeo-0.5.42/Cargo.toml:173-175` — `default-features = false` + `embedded` does not include `grafeo-engine/wal`). L3's literal call to `GrafeoDB::open(path)` will not compile under the current Cargo.toml. L1 verified the symbol exists in source but did NOT verify it is COMPILED under our feature set.

2. **B2 (no way to rebind `Arc<GrafeoDB>` in `SyncEngine` after `SsotMode::Grafeo` restore)** — `SsotMode::Grafeo` hydrate downloads a tarball, extracts, and must ATTACH to the restored on-disk DB. But `SyncEngine.grafeo_db: Arc<GrafeoDB>` is immutable (verified at `src/bridge/sync_engine.rs:97`). L1's `checkpoint` doc-comment says "The reopened handle is bound back into the `SyncEngine`'s `pub(crate) grafeo_db` field" — but `Arc<GrafeoDB>` cannot be rebound in place. Either the field type must change to `Arc<RwLock<Arc<GrafeoDB>>>` (architectural refactor) or `SsotMode::Grafeo` must be deferred to Phase 5.

3. **M1 (`SsotMode::Loro` delta-write path is missing)** — L1's `checkpoint` doc-comment only writes a base snapshot + DELETES existing deltas. L1's `hydrate` doc-comment enumerates + imports deltas via `StorageBackend::list(delta-prefix)`. But NO method in the L1 contract WRITES deltas. Architecture §4 Step D only specifies shallow-snapshot export. The delta mechanism is half-specified — `hydrate` enumerates a key-space that no one populates. For Phase 4 scope, the delta-listing in `hydrate` is a no-op (empty list). This is a documented gap, not a blocker, but L1 should have flagged it explicitly.

The 8 open questions (Q1–Q8) are all resolvable with concrete decisions — see §1 below. **No question requires deferral**; Q1 (delta-key epoch source) is moot for Phase 4 scope (no delta-write path), Q2 has two viable paths (enable `wal` OR defer `Grafeo` mode), and Q6 has a concrete mirror-of-`parallel_hydrate_grafeo` approach.

---

## 1. L1 Open Questions — Decisions

### Q1: Delta-key epoch source

**Decision**: **DEFERRED — moot for Phase 4 scope.** The delta-key format `{graph_id}/delta-{epoch}.loro` is reserved but the epoch-source decision is unneeded because Phase 4 has no delta-WRITE path (see M1).

**Rationale**:
- L1's `checkpoint` doc-comment (`src/app.rs:148-152`) only writes a base snapshot + deletes existing deltas. It does NOT write any new delta.
- L1's `hydrate` doc-comment (`src/app.rs:228-230`) enumerates deltas via `StorageBackend::list(delta-prefix)` and imports each. In Phase 4 this list will always be empty (no writer exists).
- Architecture §4 Step D specifies only `doc.export(ExportMode::ShallowSnapshot)` — there is NO delta-export step.
- Architecture §4 Step C mentions `doc.export(ExportMode::Updates)` for live peer-to-peer sync (Phase 5+ wire-protocol scope), NOT for storage.
- Implementation plan (`docs/implementation-plan.md:94,97`) says "Download base + deltas" + "upload base + clear deltas" — implying deltas are produced by some OTHER mechanism (Phase 5 Loro sync wire).
- The architecture's epoch side-channel (§9) uses `grafeo::EpochId` (verified at `src/bridge/sync_engine.rs:111` — `bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>` populated by `prepared.commit()?` at `src/bridge/batcher.rs:189`). IF a delta-write path is added later, the canonical epoch source is `grafeo_db.current_epoch().as_u64()` — verified PUBLIC at `grafeo-engine-0.5.42/src/database/crud.rs:258` (`pub fn current_epoch(&self) -> EpochId`), already called at `src/bridge/sync_engine.rs:352,361`. This matches architecture §9's "the bridge records that `EpochId` in an in-memory `HashSet`".

**L3 implementation hint**: For Phase 4 scope, `hydrate`'s delta-listing step returns `Ok(vec![])` and the import loop is a no-op. Reserve the `STORAGE_KEY_DELTA_PREFIX`/`SUFFIX` constants (already added by L1) for the Phase 5+ delta-write path; add a one-line comment to `src/constants.rs:STORAGE_KEY_DELTA_PREFIX` doc: "`P4-DEVIL Q1`: epoch-source decision deferred — Phase 4 has no delta-write path; the delta-listing in `hydrate` is a no-op. When a delta-write path lands (Phase 5+ Loro sync wire), use `grafeo_db.current_epoch().as_u64()` as the epoch slot (matches §9 epoch side-channel `EpochId` source)."

---

### Q2: `tar` crate + GrafeoDB flush strategy

**Decision**: **Option (d) — RECOMMEND deferring `SsotMode::Grafeo` to Phase 5; if the orchestrator rejects deferral, fall back to option (a) + tar.**

- **Option (a) — enable `wal` feature + `tar` crate + `GrafeoDB::backup_full`/`open` (L1's intent, made compile-correct)**: add `grafeo = { version = "0.5", features = ["wal"] }` + `tar = "0.4"` to `Cargo.toml`. Use `GrafeoDB::backup_full(&backup_dir) -> Result<BackupSegment>` (verified `grafeo-engine-0.5.42/src/database/mod.rs:2743`, gated `#[cfg(all(feature = "wal", feature = "grafeo-file", feature = "lpg"))]` — `grafeo-file` + `lpg` are in `embedded`, so adding `wal` unlocks it). Use `GrafeoDB::restore_to_epoch(&backup_dir, target_epoch, &output_path) -> Result<()>` (verified `:2813`, gated `#[cfg(all(feature = "wal", feature = "grafeo-file"))]`) for hydrate. Requires B2 fix (rebindable `grafeo_db` field).
- **Option (d) — defer `SsotMode::Grafeo` to Phase 5**: implement only `SsotMode::Loro` for Phase 4. Mark `SsotMode::Grafeo` arms in `hydrate`/`checkpoint` as `unimplemented!("P5: requires wal feature + ArcSwap grafeo_db field — see P4-DEVIL Q2/B2")` with a doc-comment citing this critique. The validation test "Full cold boot → mutate → checkpoint → cold boot again" runs in `SsotMode::Loro` only.

**Rationale for option (d) as primary recommendation**:
1. **B1 + B2 combined cost**: enabling `wal` is not enough — we ALSO need to rebind `Arc<GrafeoDB>` in `SyncEngine` (B2). That refactor touches ~30 call sites (`self.grafeo_db.session_with_cdc(...)`, `self.grafeo_db.current_epoch()`, `self.grafeo_db.graph_store()`, etc. — verified by `grep -n "grafeo_db\." src/bridge/*.rs src/app.rs`). Risk of regression in the 25 passing Phase 1-3 tests is high.
2. **Architecture §2 mode table IS asymmetric by design**: `SsotMode::Grafeo`'s value-add is "Saved directly (no network regeneration)" for vector/HNSW indexes. But Phase 4 Task 1 (S3 backend) was EXCLUDED by the user, so storage is a mock anyway — the "no network regeneration" benefit cannot be exercised without a real backend. Deferring `Grafeo` mode to Phase 5 (when Task 1 lands) is the natural sequencing.
3. **YAGNI / anti-plenger #3**: Phase 4 ships a working `SsotMode::Loro` (the default per `src/config.rs:4` `#[default] Loro`). `SsotMode::Grafeo` is the secondary mode — deferring it removes B1+B2+M3+M5+M6 risk in one stroke.
4. **Anti-Goodhart (plenger #8)**: enabling `wal` + refactoring `SyncEngine` to make `SsotMode::Grafeo` "work" against a mock storage backend would be a textbook Goodhart — green tests, broken system. The mock can't exercise the "Instant DB attach" speedup that is `Grafeo` mode's whole purpose.

**If orchestrator rejects deferral and chooses option (a)**:
- `Cargo.toml` change: `grafeo = { version = "0.5", features = ["wal"] }` + add `tar = "0.4"` to `[dependencies]`.
- `SyncEngine.grafeo_db` field type changes from `Arc<GrafeoDB>` to `Arc<RwLock<Arc<GrafeoDB>>>` (or `arc_swap::ArcSwap<GrafeoDB>` — would add `arc-swap = "1"` dep). All 30+ call sites updated to `self.grafeo_db.read().session_with_cdc(...)` (or `self.grafeo_db.load()` for `ArcSwap`).
- `checkpoint` (Grafeo mode): `let backup_dir = tempdir()?; self.sync_engine.grafeo_db.read().backup_full(&backup_dir)?; let tar_bytes = tar_directory(&backup_dir)?; let payload = CompressedPayload::compress(&tar_bytes, CompressionType::Zstd)?; storage.save(key, payload.raw_data).await?;`
- `hydrate` (Grafeo mode): download → `zstd::decode_all` → `tar::Archive::new(&bytes[..]).unpack(&extracted_dir)?` → `let restored = GrafeoDB::restore_to_epoch(&extracted_dir, EpochId::MAX, &output_path)?; let new_db = GrafeoDB::open(&output_path)?; *self.sync_engine.grafeo_db.write() = Arc::new(new_db);` — then proceed to Grafeo→Loro reconciliation (Q6).
- **Caveat**: `restore_to_epoch` takes a `target_epoch: EpochId` — for "latest", `EpochId::MAX` is the `PENDING` sentinel (`grafeo-common-0.5.42/src/types/id.rs:230`) and may not mean "latest". L3 must read `grafeo-engine-0.5.42/src/database/backup.rs` (the `do_restore_to_epoch` impl) to determine the latest-epoch semantics — `[UNVERIFIED]` Devil did not deep-dive into `backup.rs`. Alternative: read the backup manifest first to find the max epoch, then pass that.

**L3 implementation hint** (for option (d)): in `src/app.rs:307` `hydrate` body, after `match self.sync_engine.ssot_mode` (which requires adding a `ssot_mode` field to `GrafeoLoroApp` — see M8), the `SsotMode::Grafeo` arm returns `unimplemented!("P5: SsotMode::Grafeo hydrate — see P4-DEVIL Q2/B2")`. Same for `checkpoint` at `src/app.rs:200`.

---

### Q3: Atomic base-overwrite + delta-clear

**Decision**: **Option (c) — accept orphan-delta risk; deduplication is automatic via Loro's `trim_the_known_part_of_change`, NOT via `ImportStatus::pending` (L1 mis-attributed the mechanism).**

**Rationale**:
- L1's option (c) text says "rely on Loro's `ImportStatus::pending` to deduplicate". This is INCORRECT. `ImportStatus::pending: Option<VersionRange>` (`loro-internal-1.13.6/src/encoding.rs:228-232`) carries MISSING-DEPENDENCY changes (changes that reference unknown peer IDs / counters) — verified by the docstring at `loro-1.13.6/src/lib.rs:706`: "Missing dependencies: check the returned `ImportStatus`. If `pending` is non-empty, fetch those missing ranges ... and re-import."
- The actual dedup mechanism is `OpLog::trim_the_known_part_of_change` at `loro-internal-1.13.6/src/oplog.rs:350-365` — strips the prefix of an incoming change whose `(peer, counter)` is already in the oplog's version vector. The trimmed change is either `None` (fully known — no-op) or `Some(change)` with the unknown suffix. This is invoked on EVERY import, silently, regardless of `ImportStatus`. So re-importing an already-applied delta is idempotent — the known part is trimmed, the unknown part (empty) is applied.
- Therefore, in `SsotMode::Loro`: if `checkpoint` writes the base snapshot THEN fails to delete a delta, the next `hydrate` re-imports the orphan delta → `trim_the_known_part_of_change` strips the known ops → no-op. **Idempotent.**
- Option (a) "delete deltas BEFORE overwriting base" is RECOVERABLE but loses data: if base-overwrite fails after delta-clear, you have neither base nor deltas. Reverting to the previous base requires a temp-key-rename (option (b)), which needs `StorageBackend::rename` — NOT in the trait (verified `src/storage/traits.rs:2-7`).
- Option (c) is the cleanest: no trait extension, no data loss, automatic dedup.

**L3 implementation hint**: `checkpoint` proceeds in this order: (1) `LoroDoc::oplog_frontiers()` → (2) `LoroDoc::export(ShallowSnapshot)` → (3) `CompressedPayload::compress` → (4) `storage.save(base_key, bytes)` → (5) `storage.list(delta_prefix)` → (6) `for k in deltas { storage.delete(k) }`. If step 6 fails partway, the next `hydrate` re-imports the orphan deltas harmlessly (dedup via `trim_the_known_part_of_change`). Add a one-line comment in the `checkpoint` body: `// P4-DEVIL Q3: orphan-delta risk accepted — Loro's trim_the_known_part_of_change (oplog.rs:350) deduplicates re-imported deltas. NOT via ImportStatus::pending (missing-dep tracking, unrelated).`

---

### Q4: Cross-method lock for `checkpoint` vs concurrent `hydrate`/mutation

**Decision**: **Trust the orchestrator (NO `RwLock<HashSet<graph_id>>` for Phase 4). Add a `# Concurrency` section to `checkpoint` + `hydrate` doc-comments documenting the precondition.**

**Rationale**:
- The validation test (`docs/implementation-plan.md:101`) is sequential: "Full cold boot → mutate → checkpoint → cold boot again" — no concurrent `checkpoint` + `hydrate`.
- Architecture §24.2 quick-start (`docs/grafeo-loro.architecture.md:1207-1268`) is also sequential: `build().await? → hydrate("graph_123").await? → ... → checkpoint("graph_123").await?`. No concurrent invocations.
- Anti-plenger #3 (YAGNI): a `RwLock<HashSet<graph_id>>` adds 1 lock + 1 allocation per call + deadlock-analysis burden, for a constraint that no caller currently violates.
- Anti-plenger #10 (fewest LOC): 0 lines of lock code vs ~10 lines for the `RwLock` + 4 `read()`/`write()` sites.
- The `SsotMode::Grafeo` `close()`+reopen path (Q2 option (a)) ALREADY requires orchestrator-side serialization (no concurrent mutations during `close()`), so the cross-method lock would not help there anyway.
- If a future phase needs concurrent `checkpoint` + `hydrate` (e.g. multi-tenant graph), add the lock then. For Phase 4, document the precondition.

**L3 implementation hint**: add to `checkpoint` doc-comment (`src/app.rs:191-199` Idempotency section): "Concurrency: caller MUST serialize `checkpoint` with concurrent `hydrate` and any in-flight vertex mutations. No internal lock; Phase 4 trusts the orchestrator (validation test is sequential). A `RwLock<HashSet<graph_id>>` may be added in Phase 5 if a multi-tenant use case requires it." Mirror in `hydrate` doc-comment.

---

### Q5: Missing `grafeo_dir` builder setter

**Decision**: **Option (a) — add `pub fn grafeo_dir(self, path: PathBuf) -> Self` to `GrafeoLoroAppBuilder`. Default `grafeo_dir: Option<PathBuf> = None`. `build()` rejects `SsotMode::Grafeo + None` with `Config("grafeo_dir required for SsotMode::Grafeo")`. `build()` uses `GrafeoDB::new_in_memory()` when `grafeo_dir == None` (works for `SsotMode::Loro` + tests).**

**Rationale**:
- `GrafeoDB::open(path: impl AsRef<Path>)` is `wal`-gated (B1), so we cannot rely on it. `GrafeoDB::with_config(Config::persistent(path))` is NOT gated (verified at `grafeo-engine-0.5.42/src/database/mod.rs:346` — no `#[cfg]` annotation). The `with_config` path is the canonical way to open a persistent DB without enabling `wal`. (Note: `Config::persistent` sets `wal_enabled: true` at `grafeo-engine-0.5.42/src/config.rs:435-442`, but with `wal` feature OFF the WAL init block at `database/mod.rs:517-520` is `#[cfg(feature = "wal")]`-gated and silently skipped — the file_manager still initializes because `grafeo-file` IS in `embedded`. Persistence works without WAL durability — verified by reading the `with_config` body at `:445-560`.)
- Option (b) "always use `new_in_memory()`" breaks `SsotMode::Grafeo` — Q2 decision tree hinges on having a directory path.
- Option (c) "add `grafeo_dir` to `AppConfig`" is premature — `AppConfig::default()` is `unimplemented!()` (`src/config.rs:34`) and `AppConfig` is unused by any current code path. YAGNI.
- Architecture §24.4 config table does NOT mention `grafeo_dir` — Devil flags this as a doc gap (M9), not a blocker. The setter is justified by `docs/implementation-plan.md:99-100` Task 4 "Init LoroDoc, GrafeoDB, SyncEngine, Batcher" which requires a GrafeoDB init path.

**L3 implementation hint**:
- Add `grafeo_dir: Option<PathBuf>` field to `GrafeoLoroAppBuilder` (`src/app.rs:46-53`).
- Add setter: `pub fn grafeo_dir(mut self, path: impl Into<PathBuf>) -> Self { self.grafeo_dir = Some(path.into()); self }` — use `impl Into<PathBuf>` for ergonomic `&str`/`Path`/`PathBuf` acceptance.
- In `build()`: `let grafeo_db = match (self.grafeo_dir, self.ssot_mode) { (Some(p), _) => Arc::new(GrafeoDB::with_config(Config::persistent(p))?), (None, SsotMode::Loro) => Arc::new(GrafeoDB::new_in_memory()), (None, SsotMode::Grafeo) => return Err(GrafeoLoroError::Config("grafeo_dir required for SsotMode::Grafeo".into())), };`
- Add `use std::path::PathBuf;` + `use grafeo::Config;` imports to `src/app.rs`.

---

### Q6: Grafeo→Loro reconciliation path

**Decision**: **Implement `parallel_hydrate_loro` as the MIRROR of `parallel_hydrate_grafeo` — iterate `GrafeoDB::graph_store().node_ids()` + `edges_from(n, Outgoing)` for each vertex/edge, build `VertexEntity` / `EdgeEntity`, reconcile into `LoroDoc` via `entity.reconcile(RootReconciler::new(v_map.ensure_mergeable_map(&loro_key)?))`, populate `BridgeMaps`.**

**Rationale** (all citations independently verified):
- Architecture §4 Step A only specifies Loro→Grafeo hydration ("`LoroGrafeoBridge` reads final state of `LoroDoc`, iterates through active containers, and populates local in-memory or on-disk `GrafeoDB` cache"). The Grafeo→Loro direction is NOT specified — Devil flags this as a doc gap (M7), not a blocker.
- The mirror path is feasible because the necessary APIs are PUBLIC:
  - `GrafeoDB::graph_store(&self) -> Arc<dyn GraphStoreSearch>` — verified `grafeo-engine-0.5.42/src/database/mod.rs:2120` (public, NOT feature-gated). `GraphStoreSearch` extends `GraphStore` (`grafeo-core-0.5.42/src/graph/traits.rs:360`).
  - `<GraphStore as>::node_ids() -> Vec<NodeId>` — verified `grafeo-core-0.5.42/src/graph/traits.rs:115` (returns all non-deleted node IDs, sorted).
  - `<GraphStore as>::get_node(id: NodeId) -> Option<Node>` — verified `:48`. `Node` struct has `pub id: NodeId`, `pub labels: SmallVec<[ArcStr; 2]>`, `pub properties: PropertyMap` (`grafeo-core-0.5.42/src/graph/lpg/node.rs:30-37`).
  - `<GraphStore as>::edges_from(node, direction) -> Vec<(NodeId, EdgeId)>` — verified `:116`. There is NO `edge_ids()` scan method, so edges are enumerated by iterating `node_ids()` + `edges_from(n, Outgoing)` per node.
  - `<GraphStore as>::get_edge(id) -> Option<Edge>` — verified `:54`. `Edge` has `pub id`, `pub src`, `pub dst`, `pub edge_type: ArcStr`, `pub properties: PropertyMap` (`grafeo-core-0.5.42/src/graph/lpg/edge.rs:31-39`).
  - `NodeId(pub u64)` with `pub fn as_u64(&self) -> u64` — verified `grafeo-common-0.5.42/src/types/id.rs:25-60`. `format!("V/{}", node_id.as_u64())` is the deterministic `loro_key`.
  - `<VertexEntity as Reconcile>::reconcile<R: Reconciler>(&self, R) -> Result<(), ReconcileError>` — verified `lorosurgeon-0.2.1/src/reconcile.rs:92`. Already imported at `src/app.rs:6` (`use lorosurgeon::Reconcile;`).
  - `RootReconciler::new(LoroMap) -> Self` — verified `lorosurgeon-0.2.1/src/reconcile.rs:298`. Already imported at `src/app.rs:5`. The existing `VertexBuilder::commit` at `src/app.rs:738` uses `entity.reconcile(RootReconciler::new(node_map))` — same pattern, just iterate over Grafeo vertices instead of one vertex.
  - `BridgeMaps::insert_node(loro_key: String, id: grafeo::NodeId)` — verified at `src/bridge/grafeo_tx.rs:45-48` (used by `apply_upsert_node` at `:124-144`).
- The `loro_key` for restored vertices uses the grafeo-assigned `NodeId` as the suffix: `format!("V/{}", n.as_u64())`. This is consistent with `VertexBuilder::commit`'s `format!("V/{}", counter.fetch_add(1, Relaxed))` format. To avoid collision with subsequent `VertexBuilder::commit` calls, `loro_key_counter` MUST be re-seeded to `max(node_ids) + 1` via `counter.fetch_max(max_id + 1, Ordering::Relaxed)` (`AtomicU64::fetch_max` is stable since Rust 1.45). Matches L1's existing doc-comment at `src/app.rs:236` "Re-seed `loro_key_counter` to `max(existing V/* keys) + 1`".
- The echo-prevention precondition (subscriber NOT yet active — P3T2-DEVIL M3) applies EQUALLY to `parallel_hydrate_loro`: each `entity.reconcile(...)` triggers a Loro commit, which fires the synchronous subscriber if active. The subscriber would translate the diff to `LoroOp::UpsertNode` and push to the batcher, which would re-create the vertex in Grafeo — producing a duplicate. L1's `hydrate` doc-comment lists this precondition for `SsotMode::Loro` (`src/app.rs:270-273`) but NOT for `SsotMode::Grafeo` (M6). Add the precondition to both arms.
- Alternative (rejected): "CDC replay from epoch 0" — would require `GrafeoDB::changes_between(0, current_epoch)` + a reverse-translator. The reverse-translator does not exist (only `translate_diff_event` at `src/bridge/sync_engine.rs:419` for Loro→Grafeo). Building a Grafeo→Loro translator is a Phase 5+ concern (YAGNI for Phase 4).
- Alternative (rejected): "use `lorosurgeon::reconcile::RootReconciler::new(doc.get_map(ROOT_VERTICES))` directly" — this would write all vertices into the root V map as fields (not as sub-maps). The LoroDoc schema requires `V/<loro_key>` sub-maps (architecture §5 line 158-160), so this would corrupt the schema. Must use `ensure_mergeable_map(&loro_key)` per vertex.

**L3 implementation hint** (concrete code shape):
```rust
// In src/hydration/parallel.rs (new function — mirror of parallel_hydrate_grafeo):
use grafeo::GraphStore;  // brings node_ids() / get_node() / edges_from() into scope
use lorosurgeon::reconcile::RootReconciler;
use lorosurgeon::Reconcile;
use crate::schema::{VertexEntity, EdgeEntity};
use crate::bridge::grafeo_tx::BridgeMaps;
use crate::constants::ROOT_VERTICES;
use crate::types::values::LoroProperty;

pub fn parallel_hydrate_loro(db: &Arc<GrafeoDB>, doc: &LoroDoc, maps: &BridgeMaps) -> Result<()> {
    let v_root = doc.get_map(ROOT_VERTICES);
    let e_root = doc.get_map(ROOT_EDGES);  // ROOT_EDGES = "E" at constants.rs:7
    let store = db.graph_store();

    // 1. Vertices: scan node_ids() (sorted), reconcile each into V/<id>.
    let node_ids = store.node_ids();
    for n in &node_ids {
        let node = store.get_node(*n).ok_or_else(|| {
            GrafeoLoroError::Bridge(format!("graph_store::get_node({n:?}) returned None"))
        })?;
        let loro_key = format!("V/{}", n.as_u64());
        let entity = VertexEntity {
            labels: node.labels.iter().map(|s| s.to_string()).collect(),
            properties: node.properties.iter()
                .map(|(k, v)| Ok((k.to_string(), LoroProperty::try_from(v.clone())?)))
                .collect::<Result<HashMap<_, _>>>()?,
            description: String::new(),  // Loro-only field, no Grafeo source
        };
        let node_map = v_root.ensure_mergeable_map(&loro_key)
            .map_err(|e| GrafeoLoroError::Bridge(format!("ensure_mergeable_map: {e}")))?;
        entity.reconcile(RootReconciler::new(node_map))
            .map_err(|e| GrafeoLoroError::Bridge(format!("reconcile vertex: {e}")))?;
        maps.insert_node(loro_key, *n);
        doc.commit();  // flush per-vertex (or batch — see L3 perf note)
    }

    // 2. Edges: iterate node_ids() + edges_from(n, Outgoing) (each edge seen once).
    for n in &node_ids {
        for (dst, eid) in store.edges_from(*n, grafeo::Direction::Outgoing) {
            let edge = store.get_edge(eid).ok_or_else(|| {
                GrafeoLoroError::Bridge(format!("graph_store::get_edge({eid:?}) returned None"))
            })?;
            let edge_key = format!("E/{}", eid.as_u64());
            let entity = EdgeEntity {
                label: edge.edge_type.to_string(),
                src: format!("V/{}", edge.src.as_u64()),
                dst: format!("V/{}", edge.dst.as_u64()),
                properties: edge.properties.iter()
                    .map(|(k, v)| Ok((k.to_string(), LoroProperty::try_from(v.clone())?)))
                    .collect::<Result<HashMap<_, _>>>()?,
            };
            let edge_map = e_root.ensure_mergeable_map(&edge_key)
                .map_err(|e| GrafeoLoroError::Bridge(format!("ensure_mergeable_map: {e}")))?;
            entity.reconcile(RootReconciler::new(edge_map))
                .map_err(|e| GrafeoLoroError::Bridge(format!("reconcile edge: {e}")))?;
            // BridgeMaps::insert_edge (if it exists — verify at src/bridge/grafeo_tx.rs)
            doc.commit();
        }
    }

    // 3. Re-seed loro_key_counter (caller's responsibility — see hint below).
    Ok(())
}
```

`LoroProperty::TryFrom<&grafeo::Value>` — verify it exists at `src/types/values.rs`. If not, L3 adds it. `[UNVERIFIED]` Devil did not deep-dive into the `From<&grafeo::Value> for LoroProperty` impl — L3 must check.

`BridgeMaps::insert_edge` — verify at `src/bridge/grafeo_tx.rs`. The `node_id_map` is `pub` (P2T3-L1 verified), but the edge-equivalent may not be. `[UNVERIFIED]` Devil did not deep-dive — L3 must check.

`loro_key_counter` re-seed — caller (`hydrate` body) does `self.loro_key_counter.fetch_max(max_node_id + 1, Ordering::Relaxed)` after `parallel_hydrate_loro` returns. Compute `max_node_id` from `node_ids.iter().max()`.

---

### Q7: MutationBatcher parameter wiring

**Decision**: **Option (b) — add `SyncEngine::with_batch_config(grafeo_db, loro_doc, batch_size, batch_ms) -> (Self, Receiver, Receiver)` constructor. Keep `SyncEngine::new` unchanged (uses `DEFAULT_BATCH_SIZE` + `DEFAULT_BATCH_MS`). `GrafeoLoroAppBuilder::build` calls `SyncEngine::with_batch_config(...)` with the builder's `batch_max_size` + `batch_interval_ms`.**

**Rationale**:
- `SyncEngine::new` is called from 6 test sites (verified via `grep -rn "SyncEngine::new\b" tests/`: `tests/integration/sync_echo.rs:71,195,309,410` + `tests/unit/vertex_builder.rs:115,132`; a 7th grep match at `tests/unit/parallel_hydrate.rs:316` is a doc-comment, not a call). Option (a) "widen `SyncEngine::new` signature" would break all 6 — high regression risk.
- `MutationBatcher::with_defaults` (at `src/bridge/batcher.rs:93-107`) already exists as the "defaults" constructor. The pattern is precedent for adding a "with config" variant.
- Option (c) "remove `MutationBatcher::new` from `SyncEngine::new`; let `build()` construct separately" — `MutationBatcher` is owned by `SyncEngine` (field at `sync_engine.rs:117` — `pub(crate) batcher: Arc<MutationBatcher>`), and the inbound worker at `:263-269` clones `self.batcher` to spawn its `run` loop. Decoupling would require a new injection point + breaking the encapsulation that says "SyncEngine owns the batcher". YAGNI.
- Option (d) "setter on SyncEngine" — adds mutable state pre-`spawn_all`, requires the caller to remember the ordering (`set_batch_config` BEFORE `spawn_all`). Worse ergonomics than a constructor.
- Architecture §24.4 lists `batch_interval_ms` + `batch_max_size` as config knobs — wiring them through is required. The builder setters already exist (`src/app.rs:423,441`); only the SyncEngine constructor is missing.

**L3 implementation hint**:
- Add to `src/bridge/sync_engine.rs:148` (after `pub fn new`):
```rust
/// Construct an engine with explicit batcher tuning. Like [`Self::new`] but
/// threads the builder's `batch_interval_ms` / `batch_max_size` into
/// `MutationBatcher::new` instead of hardcoding `DEFAULT_BATCH_SIZE` /
/// `DEFAULT_BATCH_MS`. Used by `GrafeoLoroAppBuilder::build` (Phase 4 Task 4
/// — P4-DEVIL Q7). Tests use [`Self::new`] for defaults.
pub fn with_batch_config(
    grafeo_db: Arc<GrafeoDB>,
    loro_doc: Arc<RwLock<LoroDoc>>,
    batch_size: usize,
    batch_ms: u64,
) -> (Self, mpsc::Receiver<InboundMsg>, mpsc::Receiver<OutboundMsg>) {
    // Same body as Self::new but pass (batch_size, batch_ms) to MutationBatcher::new.
    // Factor the shared body into a private fn `new_inner(db, doc, batch_size, batch_ms)`
    // to DRY (anti-plenger #2) — `new` delegates with DEFAULT_* constants.
    Self::new_inner(grafeo_db, loro_doc, batch_size, batch_ms)
}
```
- Refactor: extract `fn new_inner(...) -> (Self, Receiver, Receiver)` containing the current `new` body, parameterized on `batch_size` + `batch_ms`. `pub fn new` becomes `Self::new_inner(db, doc, DEFAULT_BATCH_SIZE, DEFAULT_BATCH_MS)`. `pub fn with_batch_config` becomes `Self::new_inner(db, doc, batch_size, batch_ms)`. Zero duplication.
- In `GrafeoLoroAppBuilder::build` (`src/app.rs:494`): `let (engine, inbound_rx, outbound_rx) = SyncEngine::with_batch_config(grafeo_db, loro_doc, self.batch_max_size, self.batch_interval_ms);`

---

### Q8: Zero-value validation for batch params

**Decision**: **Option (b) — defensive reject in `build()`. `batch_interval_ms == 0` → `Err(Config("batch_interval_ms must be > 0"))`. `batch_max_size == 0` → `Err(Config("batch_max_size must be > 0"))`.**

**Rationale**:
- `MutationBatcher::run` uses `tokio::time::interval(Duration::from_millis(self.batch_ms))` (at `src/bridge/batcher.rs:115`). `Duration::from_millis(0)` produces a "tick immediately and then never again" interval (tokio's behavior) — this would either busy-loop or never flush, depending on `MissedTickBehavior`. Bug-at-a-distance.
- `MutationBatcher::run` size-check is `if b.len() < self.batch_size { continue; }` (at `:135`). With `batch_size == 0`, this is `0 < 0` (false) → ALWAYS flushes on every op — degenerates to no batching. Bug-at-a-distance.
- Both bugs are silent (no panic, no error) — anti-plenger #14 (never simplify the basics) applies: explicit validation is the "basic" that prevents silent degeneration.
- The 5-char delta (`> 0`) is cheaper than the debugging time for a "batcher doesn't batch" symptom.
- Architecture §24.4 lists defaults `100` / `256` — both `> 0`. Validation enforces the documented contract.
- Anti-plenger #3 (YAGNI) does NOT apply — this is parameter validation at the system boundary, not speculative generality.

**L3 implementation hint**: in `GrafeoLoroAppBuilder::build` (`src/app.rs:494`), add at the top of the body:
```rust
if self.batch_interval_ms == 0 {
    return Err(GrafeoLoroError::Config("batch_interval_ms must be > 0".into()));
}
if self.batch_max_size == 0 {
    return Err(GrafeoLoroError::Config("batch_max_size must be > 0".into()));
}
```
Also validate `storage.is_some()` (already documented in L1's doc-comment at `src/app.rs:450-451`):
```rust
let storage = self.storage.ok_or_else(|| {
    GrafeoLoroError::Config("storage backend not set".into())
})?;
```

---

## 2. Additional Findings (L1 missed)

### BLOCKER B1: `GrafeoDB::open(path)` is `#[cfg(feature = "wal")]`-gated — L1's contract will not compile under current Cargo.toml

**Symptom**: L1's `checkpoint` doc-comment (`src/app.rs:174-177`) says "Reopen the `GrafeoDB` via `GrafeoDB::open(same_dir)` (verified at `grafeo-engine-0.5.42/src/database/mod.rs:290`)". L1's `hydrate` doc-comment (`src/app.rs:255-257`) says "`GrafeoDB::open(extracted_dir)` (verified at `grafeo-engine-0.5.42/src/database/mod.rs:290`)". The function DOES exist at that line — but it is annotated `#[cfg(feature = "wal")]` at `:289`. Grafeo-0.5.42's `default = ["embedded"]` does NOT activate `wal` (verified at `grafeo-0.5.42/Cargo.toml:69-78` — `embedded = [..., "grafeo-file", "arrow-export"]` — no `"grafeo-engine/wal"`). Grafeo-loro's `Cargo.toml:7` has `grafeo = "0.5"` (default features only).

**Impact**: L3's literal call to `GrafeoDB::open(path)` will fail to compile: `error[E0432]: unresolved import \`GrafeoDB::open\`` or `error[E0599]: no function named \`open\` found`. The whole `SsotMode::Grafeo` arm of `hydrate` + `checkpoint` is broken.

**Fix**: Pick one (per Q2 decision tree):
- **Q2 option (a)**: change `Cargo.toml:7` to `grafeo = { version = "0.5", features = ["wal"] }`. Then `GrafeoDB::open(path)` compiles. (Note: this also unlocks `GrafeoDB::backup_full` + `restore_to_epoch` per Q2.)
- **Q2 option (d)**: defer `SsotMode::Grafeo` to Phase 5. `hydrate` + `checkpoint` `Grafeo` arms return `unimplemented!("P5: ...")`. L3 never calls `GrafeoDB::open(path)` — no compile error.
- **Alternative (NOT recommended)**: use `GrafeoDB::with_config(Config::persistent(path))` (NOT `wal`-gated) instead of `GrafeoDB::open(path)`. This avoids B1 without enabling `wal`, BUT loses access to `backup_full` + `restore_to_epoch` (still gated). Combined with `close()` (see B2/M3), this path is workable but requires the B2 fix and is the worst of both worlds.

**File:line**: `src/app.rs:174-177` (checkpoint doc-comment step 5), `src/app.rs:255-257` (hydrate doc-comment step 4), `Cargo.toml:7` (`grafeo = "0.5"`).

---

### BLOCKER B2: No way to rebind `Arc<GrafeoDB>` in `SyncEngine` after `SsotMode::Grafeo` restore

**Symptom**: `SsotMode::Grafeo` `hydrate` must download a tarball, extract, and ATTACH to the restored on-disk DB. But `SyncEngine.grafeo_db: Arc<GrafeoDB>` is immutable (verified at `src/bridge/sync_engine.rs:97` — `pub(crate) grafeo_db: Arc<GrafeoDB>`). L1's `checkpoint` doc-comment (`src/app.rs:174-177`) says "The reopened handle is bound back into the `SyncEngine`'s `pub(crate) grafeo_db` field" — but `Arc<GrafeoDB>` cannot be rebound in place. `Arc::make_mut` requires `T: Clone` (GrafeoDB is NOT `Clone`); `Arc::get_mut` requires unique ownership (multiple `Arc` clones exist — `SyncEngine` shares with `MutationBatcher.grafeo_db` at `batcher.rs:50` + the CDC poller at `sync_engine.rs:345`).

**Impact**: `SsotMode::Grafeo` `hydrate` CANNOT swap the restored `GrafeoDB` into the existing `SyncEngine`. The SyncEngine continues to reference the old (empty, in-memory) `GrafeoDB` while the restored one is orphaned. All subsequent `apply_loro_op` calls + CDC polling operate on the wrong DB. Subsequent `checkpoint` writes the wrong DB.

**Fix** (per Q2 decision tree):
- **Q2 option (d) (RECOMMENDED)**: defer `SsotMode::Grafeo` to Phase 5. B2 is moot for Phase 4.
- **Q2 option (a) + B2 fix**: change `SyncEngine.grafeo_db` field type to `Arc<RwLock<Arc<GrafeoDB>>>` (or `arc_swap::ArcSwap<GrafeoDB>` — would add `arc-swap = "1"` dep). All ~30 call sites updated:
  - `src/bridge/sync_engine.rs:162` — `MutationBatcher::new(grafeo_db.clone(), ...)` becomes `MutationBatcher::new(grafeo_db.read().clone(), ...)` (clone the inner `Arc`).
  - `src/bridge/sync_engine.rs:345` — `let grafeo_db = self.grafeo_db.clone();` becomes `let grafeo_db = self.grafeo_db.read().clone();`.
  - `src/bridge/sync_engine.rs:352,361` — `grafeo_db.current_epoch()` stays the same (it's called on the cloned `Arc<GrafeoDB>`, not on the `RwLock`).
  - `src/app.rs:753` — `self.sync_engine.grafeo_db.session_with_cdc(false)` becomes `self.sync_engine.grafeo_db.read().session_with_cdc(false)`.
  - `src/bridge/batcher.rs:181,187` — `self.grafeo_db.clone()` + `grafeo_db.session_with_cdc(true)` — these operate on the `Arc<GrafeoDB>` that was passed to `MutationBatcher::new` at construction. If the engine swaps the inner `Arc` after batcher construction, the batcher is stuck with the OLD `Arc`. Need to either: (i) pass the `Arc<RwLock<Arc<GrafeoDB>>>` to the batcher too, (ii) re-construct the batcher on swap, or (iii) ban `SsotMode::Grafeo` `hydrate` after `spawn_all` (force `hydrate` to run BEFORE `spawn_all`, in `build()` itself — architecture §24.2's `build().await? → hydrate("graph_123").await?` ordering forbids this).
  - This refactor is non-trivial. Devil RECOMMENDS option (d) — defer.

**File:line**: `src/bridge/sync_engine.rs:97` (field declaration), `src/app.rs:174-177` (checkpoint doc-comment claim), `src/app.rs:255-257` (hydrate doc-comment claim).

---

### MAJOR M1: `SsotMode::Loro` delta-WRITE path is missing — `hydrate` enumerates a key-space that no one populates

**Symptom**: L1's `checkpoint` doc-comment (`src/app.rs:148-152`) lists 6 steps: `oplog_frontiers` → `ShallowSnapshot` export → `compress` → `save(base_key)` → `list(delta_prefix)` → `delete` each delta. NO step WRITES a delta. L1's `hydrate` doc-comment (`src/app.rs:228-230`) enumerates + imports deltas via `list(delta_prefix)` + per-delta `load` + `import`. Architecture §4 Step D specifies only `doc.export(ExportMode::ShallowSnapshot)` — no delta-export.

**Impact**: For Phase 4 scope, `hydrate`'s delta-listing returns `Ok(vec![])` (no writer exists). This is harmless — the import loop is a no-op. But L1's doc-comment does NOT flag this gap; an L3 reader would assume deltas exist and waste time implementing a delta-import loop that never fires. Also, the `STORAGE_KEY_DELTA_PREFIX` / `STORAGE_KEY_DELTA_SUFFIX` constants are reserved but unused in Phase 4.

**Fix**: Add a `# Phase 4 scope` subsection to L1's `hydrate` doc-comment (between Preconditions and Errors): "Phase 4 scope: no delta-write path exists — `checkpoint` writes only the base snapshot; `hydrate`'s delta-listing returns `Ok(vec![])`. The delta constants are reserved for the Phase 5+ Loro sync wire-protocol path (architecture §4 Step C `doc.export(ExportMode::Updates)`). L3 implements the delta-listing as `let deltas = storage.list(...).await?; for k in &deltas { /* load + decompress + import */ }` — the loop body runs zero times in Phase 4."

**File:line**: `src/app.rs:148-152` (checkpoint doc-comment), `src/app.rs:228-230` (hydrate doc-comment), `src/constants.rs:112-116` (delta prefix/suffix constants).

---

### MAJOR M2: Q3's deduplication rationale is wrong — `ImportStatus::pending` is missing-dep tracking, NOT dedup

**Symptom**: L1's Q3 option (c) text says "rely on Loro's `ImportStatus::pending` to deduplicate". `ImportStatus::pending: Option<VersionRange>` (`loro-internal-1.13.6/src/encoding.rs:228-232`) carries MISSING-DEPENDENCY changes (changes referencing unknown peer IDs / counters) — verified by the docstring at `loro-1.13.6/src/lib.rs:706`. The actual dedup mechanism is `OpLog::trim_the_known_part_of_change` at `loro-internal-1.13.6/src/oplog.rs:350-365` — strips the prefix of an incoming change whose `(peer, counter)` is already in the oplog's version vector. This is invoked on EVERY import, silently, regardless of `ImportStatus`.

**Impact**: L3 reader of L1's Q3 doc-comment would be misled into thinking `ImportStatus::pending` is the dedup mechanism. If L3 implements a "retry import on non-empty pending" loop, it would loop forever on legitimately-pending imports (which require fetching the missing deps from a peer, not from storage). The Phase 4 hydrate path doesn't have peers, so `pending` would always be `None` for self-contained base snapshots — L1's mis-attribution is harmless in Phase 4 but misleading.

**Fix**: Update Q3 decision rationale (see §1.Q3 above). Add a one-line comment in `hydrate` body: `// Dedup is automatic via trim_the_known_part_of_change (loro-internal-1.13.6/src/oplog.rs:350) — orphan-delta re-import is a no-op. ImportStatus::pending is missing-dep tracking, NOT dedup (P4-DEVIL M2).`

**File:line**: `worklog.md:5109` (L1's Q3 framing), `src/app.rs:153-154` (checkpoint TODO comment references Q3).

---

### MAJOR M3: L1's claim "close() is destructive (drops the handle)" is misleading — `close(&self)` does NOT consume

**Symptom**: L1's `checkpoint` doc-comment (`src/app.rs:158-161`) says "Flush the on-disk `GrafeoDB` to its directory — `GrafeoDB::close()` (verified at `grafeo-engine-0.5.42/src/database/mod.rs:2229`; `Drop` calls it but explicit invocation ensures the on-disk state is current before tarring)." The function DOES exist at that line — but its signature is `pub fn close(&self) -> Result<()>`, NOT `pub fn close(self) -> Result<()>`. It takes `&self`, NOT `self`. The handle is NOT dropped — it remains in memory, only `is_open` is set to `false` (verified at `database/mod.rs:2232`). `Drop` calls `close()` again, which early-returns on `!is_open` (idempotent — `:2231`).

**Impact**: L1's wording "drops the handle" misleads L3 into thinking `close()` frees the `Arc<GrafeoDB>`. In reality:
1. After `close()`, the `Arc<GrafeoDB>` still exists in `SyncEngine.grafeo_db`.
2. Subsequent operations on the closed DB (e.g. `session_with_cdc(...)` → `begin_transaction()`) MAY fail or produce undefined behavior — the file_manager is closed (`fm.close()?` at `:2284`).
3. The `Drop` impl is a no-op (early-return), so the closed handle stays in memory until the `Arc`'s refcount drops to 0 (which requires dropping the SyncEngine + batcher + CDC poller).
4. The "reopen" path (`GrafeoDB::open(same_dir)` per L1's step 5) creates a NEW `Arc<GrafeoDB>` — but `SyncEngine.grafeo_db` still points at the OLD closed one (see B2).

**Fix**: Either:
- Update the doc-comment to: "Flush the on-disk `GrafeoDB` to its directory — `GrafeoDB::close()` takes `&self` (NOT `self`) — it flushes the WAL + file_manager and sets `is_open = false`, but the `Arc<GrafeoDB>` handle remains in memory. Subsequent operations on the closed DB will fail. The 'reopen' step 5 creates a NEW `Arc<GrafeoDB>` and requires B2 fix to rebind into `SyncEngine.grafeo_db`."
- OR (preferred): replace `close()` + reopen with `GrafeoDB::backup_full(&backup_dir)` (requires `wal` feature — see Q2 option (a)). `backup_full` is NON-destructive (takes `&self`, does NOT close) — verified at `database/mod.rs:2743-2758`. It checkpoints to file internally then copies the file to `backup_dir`. The original DB continues operating normally. This eliminates B2 for `checkpoint` (the restore-on-hydrate path still needs B2).

**File:line**: `src/app.rs:158-161` (checkpoint doc-comment step 1).

---

### MAJOR M4: Q6 lacks concrete code shape — L1 only flags the gap

**Symptom**: L1's Q6 text (worklog line 5112) says: "What's the canonical Grafeo→Loro path? Iterate `GrafeoDB`'s vertex table → `VertexEntity::reconcile(RootReconciler::new(node_map))` per vertex? Or a Grafeo CDC replay from epoch 0? This is a significant implementation gap." L1's `hydrate` doc-comment (`src/app.rs:258-265`) flags it as `// TODO(P4-L2)` but does NOT propose a concrete path.

**Impact**: L2 (fixer) reads L1's contract and has no concrete approach to refine. L3 (implementer) has no code shape to fill in. The "CDC replay from epoch 0" alternative is a rabbit hole (no reverse-translator exists — only `translate_diff_event` at `src/bridge/sync_engine.rs:419` for Loro→Grafeo). L1's flagging is correct but insufficient for an L1 contract (which should be implementable without further ambiguity).

**Fix**: See §1.Q6 above — Devil provides the concrete code shape (mirror of `parallel_hydrate_grafeo` using `graph_store().node_ids()` + `get_node()` + `entity.reconcile(RootReconciler::new(node_map))`).

**File:line**: `src/app.rs:258-265` (hydrate doc-comment step 5 Grafeo mode).

---

### MAJOR M5: `tar` crate is unnecessary if single-file format is used; `tar` IS necessary for backup-dir format

**Symptom**: L1's `checkpoint` doc-comment (`src/app.rs:167-168`) says "Tar the `GrafeoDB` directory. `// TODO(P4-L3): the \`tar\` crate is NOT yet in Cargo.toml — L3 must add it.`" The `tar` crate IS NOT in `Cargo.toml` (verified — no `tar = ` line). But whether `tar` is needed depends on the format:
- `Config::persistent(dir_path)` with `wal` feature ON + `StorageFormat::WalDirectory` (default for directory paths) → produces a directory of DB files → `tar` is needed.
- `Config::persistent(file_path)` with `wal` feature ON + `StorageFormat::SingleFile` (default for `.grafeo` extension) → produces a single `.grafeo` file → `tar` is NOT needed (just zstd the file bytes).
- `GrafeoDB::backup_full(&backup_dir)` → produces a backup directory with manifest + segment files → `tar` IS needed (multiple files).

**Impact**: L3's choice of format determines whether `tar` is needed. L1's doc-comment assumes the directory format without justification. If Q2 option (a) is chosen with `backup_full`, `tar` is needed. If Q2 option (d) is chosen, `tar` is moot. If single-file format is used, `tar` is wasted dep.

**Fix**: Decide based on Q2:
- Q2 option (a) + `backup_full` (RECOMMENDED if option (a) is chosen): `tar` IS needed. Add `tar = "0.4"` to `Cargo.toml`. Use `tar::Builder::new(Vec::new()).append_dir_all("", backup_dir)?` to create the tarball.
- Q2 option (a) + single-file format: `tar` NOT needed. Just `tokio::fs::read(&file_path).await?` + `CompressedPayload::compress(&bytes, Zstd)`. Update `STORAGE_KEY_GRAFEO_TAR_ZST` to `STORAGE_KEY_GRAFEO_SNAPSHOT_ZST = "snapshot.grafeo.zst"`. But this loses the architecture's `.tar.zst` naming convention (architecture §2 line 77 + §24.3) — Devil rejects this for spec-divergence.
- Q2 option (d): `tar` not needed in Phase 4. Add it in Phase 5 when `SsotMode::Grafeo` lands.

**File:line**: `src/app.rs:167-168` (checkpoint doc-comment step 2), `src/app.rs:252-254` (hydrate doc-comment step 3), `src/constants.rs:118-141` (`STORAGE_KEY_GRAFEO_TAR_ZST` doc).

---

### MAJOR M6: `SsotMode::Grafeo` hydrate precondition "subscriber NOT active" is missing

**Symptom**: L1's `hydrate` doc-comment lists preconditions (`src/app.rs:268-277`) but only mentions "subscriber NOT yet active" for the `SsotMode::Loro` direction (via the `parallel_hydrate_grafeo` preconditions at `src/hydration/parallel.rs:23-29`). The `SsotMode::Grafeo` direction's step 5 ("Rebuild the live `LoroDoc` from the restored Grafeo state") ALSO triggers Loro commits (one per `entity.reconcile(...)` call), which fire the synchronous subscriber if active. The subscriber would translate the diff to `LoroOp::UpsertNode` and push to the batcher, which would re-create the vertex in Grafeo — producing a duplicate.

**Impact**: If `SsotMode::Grafeo` `hydrate` is called AFTER `SyncEngine::init_loro_subscriber`, every reconcile commit triggers an echo through the batcher → `apply_loro_op` → `create_node_with_props` → duplicate vertex. The `loro_key` matches an existing entry in `node_id_map`, so `apply_upsert_node` takes the "lookup hit" path (`src/bridge/grafeo_tx.rs:124-144`) and calls `set_node_property` for each property — this is idempotent IF the properties match. But if the hydrate reconciles an edge that the subscriber then re-applies, the edge creation is NOT idempotent (`create_edge` is called twice). Phase 4 won't hit this if `hydrate` is called BEFORE `spawn_all` (per architecture §24.2 `build().await? → hydrate().await?`), but the precondition must be documented for both arms.

**Fix**: Add to `hydrate` doc-comment Preconditions section: "For BOTH `SsotMode::Loro` AND `SsotMode::Grafeo`: caller has NOT yet called `SyncEngine::init_loro_subscriber` / `spawn_all`. In `SsotMode::Grafeo`, the Grafeo→Loro reconciliation (step 5) triggers one Loro commit per vertex/edge — if the subscriber is active, it re-applies each as a `LoroOp::UpsertNode`/`UpsertEdge` via the batcher, producing duplicates (P4-DEVIL M6). Architecture §24.2 `build().await? → hydrate().await?` ordering structurally enforces this (subscriber is spawned inside `build`'s step 6 AFTER `hydrate` is called — but `build`'s step 6 currently DOES spawn the subscriber, so `hydrate` AFTER `build` would have the subscriber active. This is a CONTRADICTION — see M10)."

**File:line**: `src/app.rs:268-277` (hydrate Preconditions section).

---

### MAJOR M7: Architecture §4 Step A only specifies Loro→Grafeo hydration — Grafeo→Loro direction is unspecified

**Symptom**: Architecture §4 Step A (`docs/grafeo-loro.architecture.md:120-125`) says: "Hydrates local memory `LoroDoc` using `doc.import_with_status(&bytes)`. `LoroGrafeoBridge` reads final state of `LoroDoc`, iterates through active containers, and populates local in-memory or on-disk `GrafeoDB` cache." This describes ONLY the `SsotMode::Loro` direction (Loro→Grafeo). The `SsotMode::Grafeo` direction (download tar → extract → restore Grafeo → REBUILD LoroDoc from Grafeo) is NOT specified anywhere in §4. Architecture §2 mode table mentions `.tar.zst` storage + "Instant DB attach" boot pattern but no LoroDoc-rebuild path.

**Impact**: L1's `hydrate` doc-comment step 5 (`src/app.rs:258-265`) correctly flags this as a gap, but the architecture itself is incomplete. Devil cannot resolve an architecture gap — only flag it. This is a documentation issue, not a code issue.

**Fix**: File an architecture-doc update for Phase 5 (out of P4-DEVIL scope — Devil touches only `docs/critiques/` + `worklog.md`). Add a §4 Step A subsection: "`SsotMode::Grafeo` cold start: (1) `StorageBackend::load(snapshot.tar.zst)` → (2) `zstd::decode_all` → (3) `tar::unpack` to temp dir → (4) `GrafeoDB::backup_full::restore_to_epoch(backup_dir, target_epoch, output_path)` or `GrafeoDB::open(output_path)` → (5) `parallel_hydrate_loro(db, doc, maps)` (mirror of `parallel_hydrate_grafeo` — iterate `graph_store().node_ids()` + reconcile each `VertexEntity`/`EdgeEntity` into LoroDoc) → (6) re-seed `loro_key_counter`." For Phase 4, the §1.Q6 decision above is the canonical interpretation.

**File:line**: `docs/grafeo-loro.architecture.md:120-125` (§4 Step A).

---

### MAJOR M8: `GrafeoLoroApp` is missing a `ssot_mode` field — `hydrate` / `checkpoint` cannot dispatch

**Symptom**: L1's `hydrate` doc-comment (`src/app.rs:210`) says "Dispatches on the builder-configured `SsotMode`". But `GrafeoLoroApp` struct (`src/app.rs:35-42`) has only `sync_engine` + `loro_key_counter` fields — NO `ssot_mode` field. The builder has `ssot_mode: SsotMode` (`src/app.rs:48`), but `build()` (still `unimplemented!()`) does NOT carry it into the `GrafeoLoroApp`. `from_sync_engine` (the test constructor at `src/app.rs:67`) does NOT take a `ssot_mode` arg either.

**Impact**: `hydrate` + `checkpoint` cannot `match self.ssot_mode { ... }` — the field does not exist. L3 would need to either:
- Add `pub(crate) ssot_mode: SsotMode` field to `GrafeoLoroApp` + thread it through `from_sync_engine` (test-side) + `build` (production-side).
- Or read `ssot_mode` from `SyncEngine` (would require adding it to `SyncEngine` — bigger blast radius).
- Or hardcode `SsotMode::Loro` in `hydrate` / `checkpoint` (defeats the dispatch — Q2 option (d)).

**Fix**: Add `pub(crate) ssot_mode: SsotMode` field to `GrafeoLoroApp` (`src/app.rs:35-42`). Update `from_sync_engine` signature: `pub fn from_sync_engine(sync_engine: Arc<SyncEngine>, ssot_mode: SsotMode) -> Self` (breaking change — 7 test callers need update, all in `tests/`). Alternative: add a `pub fn from_sync_engine_with_mode(sync_engine, ssot_mode)` and keep `from_sync_engine` delegating with `SsotMode::default()` (= `Loro`). This avoids breaking existing tests but requires L3 to use the new constructor in `build()`. Devil recommends the latter (non-breaking).

Also missing: `pub(crate) storage: Arc<dyn StorageBackend>` field (needed by `hydrate`/`checkpoint` to call `load`/`save`/`list`/`delete`). Same fix pattern — add field + thread through constructors.

Also missing: `pub(crate) compression: CompressionType` field (needed by `hydrate`/`checkpoint` to call `CompressedPayload::compress`/`decompress` with the configured codec). Same fix pattern.

**File:line**: `src/app.rs:35-42` (GrafeoLoroApp struct), `src/app.rs:67` (from_sync_engine), `src/app.rs:494` (build).

---

### MAJOR M9: Architecture §24.4 config table does not mention `grafeo_dir` — doc gap

**Symptom**: Architecture §24.4 (`docs/grafeo-loro.architecture.md:1292-1304`) lists 9 config parameters (`ssot_mode`, `compression`, `sync_compression`, `batch_interval_ms`, `batch_max_size`, `hydration_chunk_size`, `max_staleness_ms`, `enable_presence`, `presence_heartbeat_ms`). `grafeo_dir` is NOT listed. But `SsotMode::Grafeo` + production `GrafeoDB::with_config(Config::persistent(path))` both require a directory path.

**Impact**: The doc gap creates ambiguity about whether `grafeo_dir` is a builder setter, an `AppConfig` field, or an env-var-driven config. Q5 decision (option (a) — builder setter) is the Devil's recommendation, but the architecture doc should be updated to reflect this.

**Fix**: File an architecture-doc update (out of P4-DEVIL scope). Add `grafeo_dir` row to §24.4: `| \`grafeo_dir\` | \`None\` (in-memory) | Path to on-disk GrafeoDB directory (required for \`SsotMode::Grafeo\` and production persistence) |`.

**File:line**: `docs/grafeo-loro.architecture.md:1292-1304` (§24.4 config table).

---

### MAJOR M10: Architecture §24.2 quick-start ordering (`build` → `hydrate`) contradicts `build`'s step 6 (`spawn_all`)

**Symptom**: Architecture §24.2 quick-start (`docs/grafeo-loro.architecture.md:1213-1223`):
```rust
let app = GrafeoLoroApp::builder()....build().await?;
app.hydrate("graph_123").await?;  // ← hydrate AFTER build
```
L1's `build` doc-comment (`src/app.rs:471-476`) step 6 says: "**Spawn tokio tasks** — `Arc::new(engine).clone().spawn_all(inbound_rx, outbound_rx).await` ... spawns the Loro subscriber + inbound worker + outbound worker + CDC poller." `SyncEngine::spawn_all` body (`src/bridge/sync_engine.rs:403-417`) calls `self.init_loro_subscriber()` BEFORE spawning the workers. So after `build().await?` returns, the subscriber IS active. Then `hydrate("graph_123").await?` runs WITH the subscriber active.

But `parallel_hydrate_grafeo` preconditions (`src/hydration/parallel.rs:23-29`) say: "subscriber is NOT yet active (otherwise the subscriber fires on each hydrated vertex and re-creates it via `apply_loro_op`, producing duplicates)". P3T2-DEVIL M3 flagged this same issue.

**Impact**: Following architecture §24.2 literally, `hydrate` runs with subscriber active → produces duplicates. The architecture contradicts the precondition. This is the SAME issue M6 raises for `SsotMode::Grafeo` direction, but it ALSO applies to `SsotMode::Loro` direction (and was already flagged in P3T2-DEVIL M3 for `parallel_hydrate_grafeo`).

**Fix**: Two viable resolutions:
- **Option (i) — move `hydrate` INTO `build`**: `build()` calls `hydrate(graph_id)` BEFORE `spawn_all` (between step 5 "init batcher" and step 6 "spawn tokio tasks"). This requires `build` to take a `graph_id: &str` parameter. Architecture §24.2 quick-start becomes `let app = GrafeoLoroApp::builder()....build("graph_123").await?;` — NO separate `hydrate` call. This is a clean fix but breaks the architecture's `build → hydrate` API.
- **Option (ii) — keep `build → hydrate` ordering; tag storage-import commits with `ORIGIN_LORO_BRIDGE`**: in `hydrate`'s LoroDoc-import step, use `LoroDoc::import_with(&bytes, ORIGIN_LORO_BRIDGE)` instead of `LoroDoc::import(&bytes)`. The existing B1 filter at `src/bridge/sync_engine.rs:234` skips `ORIGIN_LORO_BRIDGE` events, so the subscriber does NOT re-apply them. This requires `import_compressed` (or its caller) to use `import_with` — but P3T1-DEVIL Q4 already established that `compression::wrapper::import_compressed` is origin-agnostic. Phase 4 `hydrate` calls `LoroDoc::import_with` directly (not via `import_compressed`). Then `parallel_hydrate_grafeo`'s own commits also need `ORIGIN_LORO_BRIDGE` (already set via `prepared.set_metadata(ORIGIN_LORO_BRIDGE, ...)` at `src/hydration/parallel.rs:105` — but `set_metadata` is advisory-only and dropped on commit per Devil Gap 1; the GRAFEO side doesn't fire the subscriber, only the LORO side does).

Wait — re-reading: `parallel_hydrate_grafeo` writes to GRAFEO (not Loro), so it doesn't fire the Loro subscriber. The subscriber fires on LORO commits. `hydrate`'s `LoroDoc::import` IS a Loro commit (the import applies changes to the oplog and fires the subscriber). So option (ii) for `SsotMode::Loro` is: `LoroDoc::import_with(&bytes, ORIGIN_LORO_BRIDGE)` → B1 filter skips → no echo. This is the cleanest fix.

For `SsotMode::Grafeo`, the Grafeo→Loro reconciliation calls `entity.reconcile(...)` + `doc.commit()`. The `doc.commit()` fires the subscriber. To skip: `doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE)` BEFORE `doc.commit()` (same pattern as `VertexBuilder::commit` at `src/app.rs:730`). Then B1 filter skips. This is also clean.

**Decision**: Option (ii) — keep `build → hydrate` ordering; use `ORIGIN_LORO_BRIDGE` tag on all hydrate-side Loro commits (import + reconcile). This avoids breaking the architecture API and reuses the existing B1 filter. `parallel_hydrate_grafeo` does NOT need a fix (it writes to Grafeo, not Loro — its `set_metadata(ORIGIN_LORO_BRIDGE, ...)` is advisory-only and was always irrelevant; the real echo prevention for `parallel_hydrate_grafeo` is `session_with_cdc(false)` which suppresses the outbound Grafeo→Loro echo).

**L3 implementation hint**:
- In `hydrate` `SsotMode::Loro` step 3: replace `LoroDoc::import(&bytes)` with `LoroDoc::import_with(&bytes, ORIGIN_LORO_BRIDGE)`. The B1 filter at `sync_engine.rs:234` skips events with this origin.
- In `parallel_hydrate_loro` (the new function per Q6): wrap each `entity.reconcile(...)` + `doc.commit()` with `doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE)` before commit. Same pattern as `VertexBuilder::commit` at `src/app.rs:730-737`.
- Add a one-line comment in `hydrate` body: `// P4-DEVIL M10: import_with(ORIGIN_LORO_BRIDGE) → B1 filter skips → no echo. Architecture §24.2 build→hydrate ordering works because the filter catches our origin.`

**File:line**: `src/app.rs:223` (hydrate step 3 — LoroDoc::import), `src/app.rs:471-476` (build step 6 spawn_all), `src/bridge/sync_engine.rs:234` (B1 filter).

---

### MINOR m1: `STORAGE_KEY_GRAFEO_TAR_ZST` doc-comment cites `GrafeoDB::open` without the `wal` caveat

**Symptom**: `src/constants.rs:126-127` says "`GrafeoDB::open(extracted_dir)` (verified at `grafeo-engine-0.5.42/src/database/mod.rs:290`)". L1 verified the symbol exists at that line, but did NOT verify the `#[cfg(feature = "wal")]` gate at `:289`. The constant's doc-comment is therefore misleading — a reader would assume `GrafeoDB::open` is always available.

**Impact**: Minor — same root cause as B1, but the constants doc-comment is a documentation issue, not a compile error. L3 reads the doc-comment, assumes `GrafeoDB::open` works, and hits B1 at compile time.

**Fix**: Update `src/constants.rs:126-127` to: "`GrafeoDB::open(extracted_dir)` (verified at `grafeo-engine-0.5.42/src/database/mod.rs:290` — BUT `#[cfg(feature = "wal")]`-gated; requires `grafeo = { version = "0.5", features = ["wal"] }` in Cargo.toml, OR use `GrafeoDB::with_config(Config::persistent(path))` instead — see P4-DEVIL B1)."

**File:line**: `src/constants.rs:126-127`.

---

### MINOR m2: `STORAGE_KEY_BASE_LORO` doc-comment says "raw bytes + codec" but does not specify the codec tag wire format

**Symptom**: `src/constants.rs:81-93` doc-comment for `STORAGE_KEY_BASE_LORO` says: "optionally wrapped via `CompressedPayload::compress(&bytes, CompressionType::Zstd)` (P3T1-L3 codec envelope)". But `CompressedPayload` is IN-MEMORY ONLY (P3T1-DEVIL M4) — there's NO wire format. Phase 4 `checkpoint` saves `payload.raw_data` (raw zstd-compressed bytes, NOT the codec envelope) to storage. Phase 4 `hydrate` reads raw bytes from storage and calls `CompressedPayload::decompress` — but `decompress` requires a `CompressedPayload` struct with the `compression` field set. There's a MISMATCH: `checkpoint` writes raw bytes (no codec tag), `hydrate` expects a `CompressedPayload` (with codec tag).

**Impact**: L3 implementing `hydrate` would need to either:
- Construct a `CompressedPayload { compression: CompressionType::Zstd, raw_data: loaded_bytes }` manually (assumes the codec — but `checkpoint` might use `CompressionType::None` or `Lz4` depending on builder config).
- OR call `zstd::decode_all(&loaded_bytes[..])` directly (bypasses `CompressedPayload`, but assumes Zstd — wrong if builder config is `Lz4` or `None`).

The wire format MUST include the codec tag (1 byte) so `hydrate` knows how to decompress. P3T1-DEVIL M4 already flagged this as a Phase 4 concern. L1's doc-comment does not address it.

**Fix**: Add a `# Wire format` subsection to `STORAGE_KEY_BASE_LORO` doc-comment: "Wire format: 1-byte codec tag (0=None, 1=Lz4, 2=Zstd — matches `CompressionType` discriminant order) + N bytes compressed/raw payload. Phase 4 `checkpoint` writes `codec_tag || payload.raw_data` to storage; `hydrate` reads the first byte, dispatches on codec, decompresses the rest. L3 adds a 5-line `compress_to_wire(&[u8], CompressionType) -> Vec<u8>` + `decompress_from_wire(&[u8]) -> Result<Vec<u8>>` helper in `src/compression/wrapper.rs`."

**File:line**: `src/constants.rs:81-93`, `src/compression/wrapper.rs:11` (CompressedPayload in-memory-only rustdoc).

---

### MINOR m3: `hydrate`'s `LoroDoc::import` doc-comment says "non-empty `pending` triggers a delta fetch loop" — should be "`pending.is_some()`"

**Symptom**: `src/app.rs:223-227` says: "`LoroDoc::import(&bytes)` (verified at `loro-1.13.6/src/lib.rs:710`) — surfaces `ImportStatus`; non-empty `pending` triggers a delta fetch loop (step 4)." But `ImportStatus::pending` is `Option<VersionRange>` (`loro-internal-1.13.6/src/encoding.rs:228-232`), NOT a collection. The correct check is `status.pending.is_some()`, not "non-empty".

**Impact**: Minor — L3 reading the doc-comment would understand the intent ("if there are pending changes, fetch deps"). The literal wording is slightly off. P4-DEVIL M2 already clarifies that `pending` is missing-dep tracking, NOT dedup — so the "delta fetch loop" framing is doubly wrong (it's a "fetch missing deps" loop, not a "fetch more deltas" loop).

**Fix**: Update `src/app.rs:223-227` to: "`LoroDoc::import_with(&bytes, ORIGIN_LORO_BRIDGE)` (verified at `loro-1.13.6/src/lib.rs:721` — `import_with` lets us tag the import with the B1-filter origin per M10) — surfaces `ImportStatus`; `status.pending.is_some()` means missing-dependency changes were deferred (Loro docstring at `lib.rs:706-708` says 'fetch those missing ranges and re-import') — for Phase 4 self-contained base snapshots this is always `None`. `pending` is NOT a dedup mechanism (P4-DEVIL M2)."

**File:line**: `src/app.rs:223-227`.

---

### MINOR m4: `SsotMode::Grafeo` checkpoint step 5 says "bind back into `SyncEngine`'s `pub(crate) grafeo_db` field" — implies mutable field that does not exist

**Symptom**: `src/app.rs:174-177` says: "Reopen the `GrafeoDB` via `GrafeoDB::open(same_dir)` (verified at `grafeo-engine-0.5.42/src/database/mod.rs:290`) if `close()` was used in step 1. The reopened handle is bound back into the `SyncEngine`'s `pub(crate) grafeo_db` field." The phrase "bound back into" implies the field is mutable/swappable. It is NOT — see B2.

**Impact**: Same as B2 — L3 reader would assume the rebinding is straightforward and hit the architectural wall.

**Fix**: Replace step 5 with: "Reopen the `GrafeoDB` via `GrafeoDB::with_config(Config::persistent(same_dir))` (NOT `GrafeoDB::open` — that is `wal`-gated per B1). Rebinding the new `Arc<GrafeoDB>` into `SyncEngine.grafeo_db` requires the B2 fix (`Arc<RwLock<Arc<GrafeoDB>>>` field type). For Phase 4 option (d), this step is `unimplemented!()` — `SsotMode::Grafeo` deferred to Phase 5."

**File:line**: `src/app.rs:174-177`.

---

### NIT n1: `STORAGE_KEY_BASE_LORO` doc-comment cites `loro-1.13.6/src/lib.rs:710` for `LoroDoc::import`, but the import path should be `import_with` per M10

**Symptom**: `src/constants.rs:87-88` says: "On `hydrate`, decompressed + `LoroDoc::import(&bytes)` (verified at `loro-1.13.6/src/lib.rs:710`)." Per M10 decision, Phase 4 `hydrate` should use `LoroDoc::import_with(&bytes, ORIGIN_LORO_BRIDGE)` (at `:721`) to tag the import for B1 filter.

**Impact**: NIT — L3 would follow the doc-comment and use `LoroDoc::import` (no origin tag), hitting the M10 echo issue.

**Fix**: Update `src/constants.rs:87-88` to: "On `hydrate`, decompressed + `LoroDoc::import_with(&bytes, ORIGIN_LORO_BRIDGE)` (verified at `loro-1.13.6/src/lib.rs:721` — `import_with` tags the import for the B1 echo filter per P4-DEVIL M10)."

**File:line**: `src/constants.rs:87-88`.

---

### NIT n2: L1's `checkpoint` doc-comment step 5 (`GrafeoDB::open(same_dir)`) cites the wrong line for `GrafeoDB::open`

**Symptom**: `src/app.rs:174-177` says "`GrafeoDB::open(same_dir)` (verified at `grafeo-engine-0.5.42/src/database/mod.rs:290`)". The function definition starts at `:289` (the `#[cfg(feature = "wal")]` attribute) with the `pub fn open(...)` signature at `:290`. The citation is line-correct but context-incomplete — it does not mention the `#[cfg]` gate.

**Impact**: NIT — same as B1 / m1.

**Fix**: Covered by B1 / m1 / m4 fixes.

**File:line**: `src/app.rs:174-177`, `src/app.rs:255-257`, `src/constants.rs:126-127`.

---

### NIT n3: `STORAGE_KEY_DELTA_PREFIX` doc-comment says "epoch slot is the grafeo-side `EpochId`" but Q1 decision is "deferred — moot for Phase 4"

**Symptom**: `src/constants.rs:106-111` says: "The `{epoch}` slot in the key is the grafeo-side `EpochId` at the time the delta was produced (architecture §9 — epoch side-channel is the SSOT for commit ordering across the Loro↔Grafeo bridge). `P4-DEVIL Q1`: the exact epoch-source for the key (`bridge_origin_epochs` set vs. grafeo's `transaction_manager.current_epoch()`) is ambiguous in the spec — flagged for resolution before P4-L2 wiring."

**Impact**: NIT — Q1 is now resolved (deferred, moot for Phase 4). The doc-comment's "flagged for resolution before P4-L2 wiring" is stale.

**Fix**: Update `src/constants.rs:106-111` to: "The `{epoch}` slot is reserved for the grafeo-side `EpochId` (`GrafeoDB::current_epoch().as_u64()` — verified PUBLIC at `grafeo-engine-0.5.42/src/database/crud.rs:258`). `P4-DEVIL Q1` RESOLVED: deferred — Phase 4 has no delta-write path (M1); the slot is unused in Phase 4. Phase 5+ Loro sync wire will populate it."

**File:line**: `src/constants.rs:106-111`.

---

## 3. Architecture Alignment Audit

### `docs/implementation-plan.md` Phase 4

| Task | L1 alignment | Status |
|---|---|---|
| Task 1 (S3 backend) | EXCLUDED by user — L1 did not touch it | ✓ |
| Task 2 (`hydrate`) — Loro mode: download base + deltas → import → parallel hydrate | L1's `hydrate` doc-comment matches all 6 steps. M1 (delta-write path missing) is a spec gap, not a misalignment. | ✓ with M1 caveat |
| Task 2 (`hydrate`) — Grafeo mode: download tar.zst → extract → restore DB → hydrate Loro | L1's `hydrate` doc-comment matches all 6 steps. B1 (`GrafeoDB::open` gated) + B2 (no rebind) + M4 (no concrete Q6 path) are the blockers. | ✗ B1/B2/M4 |
| Task 3 (`checkpoint`) — Loro mode: shallow snapshot → upload base → clear deltas | L1's `checkpoint` doc-comment matches all 6 steps. | ✓ |
| Task 3 (`checkpoint`) — Grafeo mode: backup DB → compress tar.zst → upload | L1's `checkpoint` doc-comment matches all 5 steps. B1/B2/M3 are the blockers. | ✗ B1/B2/M3 |
| Task 4 (`build`) — validate config → init LoroDoc/GrafeoDB/SyncEngine/Batcher → spawn tokio tasks | L1's `build` doc-comment matches all 7 steps. M8 (missing `ssot_mode`/`storage`/`compression` fields on `GrafeoLoroApp`), M10 (build→hydrate ordering contradiction), Q5 (missing `grafeo_dir` setter), Q7 (batch params not threaded), Q8 (zero-value validation) are the gaps. | ✗ M8/M10/Q5/Q7/Q8 |
| Validation — full cold boot → mutate → checkpoint → cold boot again | Achievable in `SsotMode::Loro` only for Phase 4 (Q2 option (d) recommendation). | ✓ for Loro mode |
| Validation — S3 network failure → graceful error, no corruption | Out of P4-DEVIL scope (S3 backend excluded). | N/A |

### `docs/grafeo-loro.architecture.md`

| Section | L1 alignment | Status |
|---|---|---|
| §2 SSOT modes | L1's constants `STORAGE_KEY_BASE_LORO` / `STORAGE_KEY_GRAFEO_TAR_ZST` match `.loro` + `.tar.zst` artifacts. | ✓ |
| §4 Step A (cold startup) — Loro direction | L1's `hydrate` Loro mode matches §4 Step A. M10 (build→hydrate ordering vs subscriber) is a contradiction. | ✗ M10 |
| §4 Step A (cold startup) — Grafeo direction | UNSPECIFIED in architecture (M7). L1's `hydrate` Grafeo mode is Devil's interpretation. | ✗ M7 |
| §4 Step D (session termination) | L1's `checkpoint` Loro mode matches §4 Step D shallow-snapshot. | ✓ |
| §5 root container schema | L1's `ROOT_VERTICES` / `ROOT_EDGES` constants match §5. Q6's `parallel_hydrate_loro` writes into `V/<id>` + `E/<id>` per §5. | ✓ |
| §6 VertexEntity schema | L1's `hydrate` step 5 Grafeo mode reconciles `VertexEntity` (P2 derives). | ✓ |
| §9 epoch side-channel | L1's epoch mention in `STORAGE_KEY_DELTA_PREFIX` doc-comment aligns with §9. Q1 decision: deferred (moot). | ✓ |
| §14 dual-layer compression | L1's `CompressedPayload::compress` with `Zstd` for cold snapshots matches §14. m2 (wire format missing) is a gap. | ✗ m2 |
| §15 compression wrapper | L1 uses `CompressedPayload::compress`/`decompress` per §15. | ✓ |
| §16 parallel index hydration engine | L1's `hydrate` step 5 calls `parallel_hydrate_grafeo` per §16. | ✓ |
| §20 inbound mutation batcher | Q7's `SyncEngine::with_batch_config` decision aligns with §20's `apply_loro_op` SSOT. | ✓ |
| §24.2 quick-start example | L1's `build` doc-comment matches §24.2's `builder()....build().await?` chain. M10 (build→hydrate ordering) contradicts §24.2's `app.hydrate("graph_123").await?` post-build call. | ✗ M10 |
| §24.3 StorageBackend trait | L1's constants + `hydrate`/`checkpoint` calls (`load`/`save`/`list`/`delete`) match §24.3. | ✓ |
| §24.4 configuration reference | Q5 (missing `grafeo_dir` setter) + M9 (architecture doc gap) — `grafeo_dir` not in §24.4. Q8 (zero-value validation) is implicit in §24.4's defaults. | ✗ M9 |

### `docs/grafeo-loro.project-structure.md`

| Module | L1 alignment | Status |
|---|---|---|
| `src/app.rs` — `GrafeoLoroApp` builder + lifecycle | L1's `hydrate` + `checkpoint` + 6 setters + `build` are all in `src/app.rs`. Matches project-structure spec. | ✓ |
| `src/constants.rs` — centralized constants | L1's 4 new storage-key constants (`STORAGE_KEY_BASE_LORO`/`DELTA_PREFIX`/`DELTA_SUFFIX`/`GRAFEO_TAR_ZST`) match the "SSOT" mandate. | ✓ |
| `src/storage/traits.rs` — `StorageBackend` trait | L1 did not modify the trait (correct — Task 1 excluded). | ✓ |
| `src/hydration/parallel.rs` — `parallel_hydrate_grafeo` | L1's `hydrate` calls this. Q6 proposes adding `parallel_hydrate_loro` here. | ✓ with Q6 addition |
| `src/bridge/sync_engine.rs` — `SyncEngine` | Q7 proposes adding `with_batch_config` constructor. B2 proposes changing `grafeo_db` field type (rejected for Phase 4). | ✓ with Q7 addition |
| `src/bridge/batcher.rs` — `MutationBatcher` | Q7's `SyncEngine::with_batch_config` threads params into `MutationBatcher::new`. | ✓ |

---

## 4. L2/L3 Action Checklist

Ordered by priority (BLOCKER first, then MAJOR, then MINOR, then NIT).

### BLOCKER (2)

1. **[L2+L3] B1 + Q2**: Choose `SsotMode::Grafeo` strategy. **Devil RECOMMENDS option (d)** — defer `SsotMode::Grafeo` to Phase 5. In `src/app.rs:200` `checkpoint` body and `src/app.rs:307` `hydrate` body: `match self.ssot_mode { SsotMode::Loro => { /* L2/L3 implement */ }, SsotMode::Grafeo => unimplemented!("P5: requires wal feature + ArcSwap grafeo_db field — see P4-DEVIL Q2/B1/B2") }`. If orchestrator rejects deferral, fall back to option (a): add `grafeo = { version = "0.5", features = ["wal"] }` + `tar = "0.4"` to `Cargo.toml`, then fix B2.

2. **[L2+L3] B2 (conditional on option (a))**: If Q2 option (a) is chosen, change `SyncEngine.grafeo_db: Arc<GrafeoDB>` to `Arc<RwLock<Arc<GrafeoDB>>>` (or `ArcSwap<GrafeoDB>`). Update ~30 call sites in `src/bridge/sync_engine.rs`, `src/bridge/batcher.rs`, `src/app.rs`. Devil RECOMMENDS option (d) to avoid this refactor.

### MAJOR (10)

3. **[L2] M8**: Add `pub(crate) ssot_mode: SsotMode`, `pub(crate) storage: Arc<dyn StorageBackend>`, `pub(crate) compression: CompressionType` fields to `GrafeoLoroApp` (`src/app.rs:35-42`). Add non-breaking `pub fn from_sync_engine_with_config(sync_engine, ssot_mode, storage, compression) -> Self` constructor. Update `build()` body to use the new constructor. Keep `from_sync_engine` delegating with `SsotMode::default()` + `unimplemented!("storage not set — use from_sync_engine_with_config")` (or accept `Arc<dyn StorageBackend>` as a new required arg — Devil recommends the latter for explicitness).

4. **[L2] Q5 + M9**: Add `pub fn grafeo_dir(self, path: impl Into<PathBuf>) -> Self` setter to `GrafeoLoroAppBuilder` (`src/app.rs:324`). Add `grafeo_dir: Option<PathBuf>` field. In `build()`: `let grafeo_db = match (self.grafeo_dir, self.ssot_mode) { (Some(p), _) => Arc::new(GrafeoDB::with_config(Config::persistent(p))?), (None, SsotMode::Loro) => Arc::new(GrafeoDB::new_in_memory()), (None, SsotMode::Grafeo) => return Err(GrafeoLoroError::Config("grafeo_dir required for SsotMode::Grafeo".into())), };`. Add `use std::path::PathBuf;` + `use grafeo::Config;` imports. File architecture-doc update for §24.4 (out of P4-DEVIL scope).

5. **[L2] Q7**: Add `pub fn with_batch_config(grafeo_db, loro_doc, batch_size, batch_ms) -> (Self, Receiver, Receiver)` to `SyncEngine` (`src/bridge/sync_engine.rs:148`). Refactor `pub fn new` to delegate to a private `fn new_inner(db, doc, batch_size, batch_ms)` (DRY). Update `build()` to call `SyncEngine::with_batch_config(grafeo_db, loro_doc, self.batch_max_size, self.batch_interval_ms)`.

6. **[L2] Q8**: Add zero-value validation to `build()` (`src/app.rs:494`): `if self.batch_interval_ms == 0 { return Err(GrafeoLoroError::Config("batch_interval_ms must be > 0".into())); }` + same for `batch_max_size`. Also `let storage = self.storage.ok_or_else(|| GrafeoLoroError::Config("storage backend not set".into()))?;`.

7. **[L2] M10**: Update `hydrate` `SsotMode::Loro` step 3 to use `LoroDoc::import_with(&bytes, ORIGIN_LORO_BRIDGE)` instead of `LoroDoc::import(&bytes)`. This routes through the B1 filter at `src/bridge/sync_engine.rs:234` and prevents echo when `hydrate` runs after `spawn_all`.

8. **[L3] Q6 + M4**: Implement `parallel_hydrate_loro` in `src/hydration/parallel.rs` per the concrete code shape in §1.Q6. Wrap each `entity.reconcile(...)` + `doc.commit()` with `doc.set_next_commit_origin(ORIGIN_LORO_BRIDGE)` (M10 echo prevention). Add `use grafeo::GraphStore;` import. Add `pub use hydration::parallel_hydrate_loro;` to `src/lib.rs` for crate-root re-export (matches P3T2-DEVIL m1 precedent).

9. **[L2] M1**: Add a `# Phase 4 scope` subsection to `hydrate` doc-comment (`src/app.rs:208-310`) flagging that the delta-listing returns `Ok(vec![])` in Phase 4 (no delta-write path exists). Reserve `STORAGE_KEY_DELTA_PREFIX`/`SUFFIX` for Phase 5+.

10. **[L2] M2**: Update Q3 rationale in `worklog.md` and add a one-line comment in `checkpoint` body: `// P4-DEVIL M2: dedup is via trim_the_known_part_of_change (loro-internal oplog.rs:350), NOT ImportStatus::pending (missing-dep tracking).`

11. **[L2] M3**: Update `checkpoint` doc-comment (`src/app.rs:158-161`) to clarify `close(&self)` does NOT consume the handle. If Q2 option (a) is chosen, replace `close()` + reopen with `backup_full(&backup_dir)` (non-destructive).

12. **[L2] M6**: Add `SsotMode::Grafeo` precondition to `hydrate` doc-comment (`src/app.rs:268-277`): "For BOTH `SsotMode::Loro` AND `SsotMode::Grafeo`: subscriber MUST be inactive OR all hydrate-side Loro commits MUST be tagged with `ORIGIN_LORO_BRIDGE` (M10)."

### MINOR (4)

13. **[L2] m1**: Update `STORAGE_KEY_GRAFEO_TAR_ZST` doc-comment (`src/constants.rs:126-127`) to mention `GrafeoDB::open` is `wal`-gated. Cross-ref B1.

14. **[L3] m2**: Add `compress_to_wire(&[u8], CompressionType) -> Vec<u8>` + `decompress_from_wire(&[u8]) -> Result<Vec<u8>>` helpers in `src/compression/wrapper.rs`. Wire format: 1-byte codec tag (0=None, 1=Lz4, 2=Zstd) + N bytes payload. `checkpoint` writes `compress_to_wire(&payload.raw_data, codec)` to storage; `hydrate` reads + `decompress_from_wire`. Update `STORAGE_KEY_BASE_LORO` doc-comment to specify the wire format.

15. **[L2] m3**: Update `hydrate` doc-comment (`src/app.rs:223-227`) to use `import_with` (per M10) + `pending.is_some()` (not "non-empty") + clarify `pending` is missing-dep tracking (not dedup, per M2).

16. **[L2] m4**: Update `checkpoint` doc-comment step 5 (`src/app.rs:174-177`) to clarify rebinding requires B2 fix (or is `unimplemented!()` under Q2 option (d)).

### NIT (3)

17. **[L2] n1**: Update `STORAGE_KEY_BASE_LORO` doc-comment (`src/constants.rs:87-88`) to cite `import_with` at `:721` instead of `import` at `:710` (per M10).

18. **[L2] n2**: Update all `GrafeoDB::open` citations (`src/app.rs:174-177`, `src/app.rs:255-257`, `src/constants.rs:126-127`) to mention the `#[cfg(feature = "wal")]` gate.

19. **[L2] n3**: Update `STORAGE_KEY_DELTA_PREFIX` doc-comment (`src/constants.rs:106-111`) to reflect Q1 resolution (deferred, moot for Phase 4).

### Architecture-doc updates (out of P4-DEVIL scope — flag for orchestrator)

20. **[orchestrator] M7**: Architecture §4 Step A lacks `SsotMode::Grafeo` cold-start path. File doc update for Phase 5.

21. **[orchestrator] M9**: Architecture §24.4 config table missing `grafeo_dir` row. File doc update.

22. **[orchestrator] M10**: Architecture §24.2 quick-start `build → hydrate` ordering contradicts `build`'s step 6 `spawn_all` (subscriber active). Either reorder (`hydrate` inside `build`) OR document the `ORIGIN_LORO_BRIDGE` tag requirement. Devil chose the latter (M10 fix #7) — architecture doc should reflect this.

---

## 5. Verifications Performed

Every grep command below was run against the actual crate source under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/` (or `grafeo-loro` `src/`/`docs/` for internal citations). L2/L3 can reproduce any citation by re-running.

### Grafeo API verifications (B1 / Q2 / Q5 / Q6)

```bash
# GrafeoDB::open is wal-gated (B1)
grep -n "pub fn open\b\|pub fn close\b\|pub fn backup_full\b\|pub fn restore_to_epoch\b\|pub fn checkpoint_to_file\b\|pub fn file_manager\b\|pub fn with_config\b\|pub fn new_in_memory\b" \
  grafeo-engine-0.5.42/src/database/mod.rs
# Output:
#   267: pub fn new_in_memory() -> Self
#   289: #[cfg(feature = "wal")]              ← B1 gate
#   290: pub fn open(path: impl AsRef<Path>) -> Result<Self>
#   319: #[cfg(feature = "grafeo-file")]      ← open_read_only gate
#   320: pub fn open_read_only(path: impl AsRef<std::path::Path>) -> Result<Self>
#   346: pub fn with_config(config: Config) -> Result<Self>   ← NOT gated
#   2229: pub fn close(&self) -> Result<()>   ← M3: takes &self, NOT self
#   2743: #[cfg(all(feature = "wal", feature = "grafeo-file", feature = "lpg"))]  ← backup_full gate
#   2744: pub fn backup_full(&self, backup_dir: &Path) -> Result<BackupSegment>
#   2813: #[cfg(all(feature = "wal", feature = "grafeo-file"))]  ← restore_to_epoch gate
#   2814: pub fn restore_to_epoch(backup_dir: &Path, target_epoch: EpochId, output_path: &Path) -> Result<()>
#   2827: fn checkpoint_to_file(...)   ← PRIVATE (not pub)
#   2853: #[cfg(feature = "grafeo-file")]  ← file_manager gate (grafeo-file is in embedded)
#   2854: pub fn file_manager(&self) -> Option<&Arc<GrafeoFileManager>>

# Grafeo's grafeo-engine dep declaration (B1 root cause)
grep -n "grafeo-engine\|default-features" grafeo-0.5.42/Cargo.toml | head -5
# Output:
#   173: [dependencies.grafeo-engine]
#   174: version = "0.5.42"
#   175: default-features = false       ← wal is OFF by default

# Grafeo's embedded feature set (B1 root cause)
grep -A 10 "^embedded = \[" grafeo-0.5.42/Cargo.toml
# Output: embedded = ["grafeo-engine/lpg", "gql", "ai", "algos", "parallel", "regex", "grafeo-file", "arrow-export"]
# (no "grafeo-engine/wal" — confirms wal is OFF)

# GrafeoDB::current_epoch is PUBLIC (Q1)
grep -n "pub fn current_epoch" grafeo-engine-0.5.42/src/database/crud.rs
# Output: 258: pub fn current_epoch(&self) -> grafeo_common::types::EpochId

# GrafeoDB::graph_store is PUBLIC (Q6)
grep -n "pub fn graph_store\b" grafeo-engine-0.5.42/src/database/mod.rs
# Output: 2120: pub fn graph_store(&self) -> Arc<dyn GraphStoreSearch>

# GraphStore::node_ids + edges_from + get_node + get_edge (Q6)
grep -n "fn node_ids\|fn edges_from\|fn get_node\b\|fn get_edge\b\|fn edge_count\|fn node_count" \
  grafeo-core-0.5.42/src/graph/traits.rs
# Output: 48 (get_node), 54 (get_edge), 116 (edges_from), 115 (node_ids), 158 (edge_count), 155 (node_count)

# Node + Edge struct shapes (Q6)
grep -A 5 "^pub struct Node\b\|^pub struct Edge\b" \
  grafeo-core-0.5.42/src/graph/lpg/node.rs grafeo-core-0.5.42/src/graph/lpg/edge.rs
# Output: Node { id, labels: SmallVec<[ArcStr; 2]>, properties: PropertyMap }
#         Edge { id, src, dst, edge_type: ArcStr, properties: PropertyMap }

# NodeId/EdgeId/EpochId as_u64() accessors (Q1, Q6)
grep -n "pub fn as_u64\|pub const fn as_u64" grafeo-common-0.5.42/src/types/id.rs
# Output: 49 (NodeId), 109 (EdgeId), 244 (EpochId)
```

### Loro API verifications (Q3 / M2 / m3)

```bash
# ImportStatus struct + pending field (M2)
grep -n "pub struct ImportStatus\|pub pending\|pub success" \
  loro-internal-1.13.6/src/encoding.rs
# Output: 228 (pub struct ImportStatus), 229 (pub success: VersionRange), 230 (pub pending: Option<VersionRange>)

# trim_the_known_part_of_change — the REAL dedup mechanism (M2)
grep -n "fn trim_the_known_part_of_change\|fn check_id_is_not_duplicated" \
  loro-internal-1.13.6/src/oplog.rs
# Output: 350 (trim_the_known_part_of_change), 368 (check_id_is_not_duplicated)

# LoroDoc::import vs import_with (M10 / n1)
grep -n "pub fn import\b\|pub fn import_with\b" loro-1.13.6/src/lib.rs
# Output: 710 (import), 721 (import_with)

# LoroDoc::import docstring — confirms pending = missing deps, NOT dedup (M2)
sed -n '700,715p' loro-1.13.6/src/lib.rs
# "Missing dependencies: check the returned ImportStatus. If pending is non-empty, fetch those missing ranges..."
```

### Lorosurgeon API verifications (Q6)

```bash
# Reconcile trait + RootReconciler (Q6)
grep -n "trait Reconcile\b\|fn reconcile<R\|pub struct RootReconciler\|pub fn new\b\|impl Reconciler for RootReconciler" \
  lorosurgeon-0.2.1/src/reconcile.rs
# Output: 87 (trait Reconcile), 92 (fn reconcile<R: Reconciler>), 293 (pub struct RootReconciler),
#         298 (pub fn new(map: LoroMap) -> Self), 303 (impl Reconciler for RootReconciler)

# PropReconciler::map_put (Q6 alternative for sub-map writes — not chosen; RootReconciler::new(LoroMap) preferred per existing commit() pattern)
grep -n "pub fn map_put\|PropAction::MapPut" lorosurgeon-0.2.1/src/reconcile.rs
# Output: 147 (pub fn map_put(map: LoroMap, key: String) -> Self)
```

### Grafeo-loro internal verifications (B2 / M8 / Q7 / Q8)

```bash
# SyncEngine.grafeo_db field type (B2)
grep -n "pub(crate) grafeo_db\|pub(crate) sync_engine\|pub(crate) loro_key_counter" src/bridge/sync_engine.rs src/app.rs
# Output: src/bridge/sync_engine.rs:97 (pub(crate) grafeo_db: Arc<GrafeoDB>)  ← immutable Arc, B2
#         src/app.rs:38 (pub(crate) sync_engine: Arc<SyncEngine>)
#         src/app.rs:41 (pub(crate) loro_key_counter: Arc<AtomicU64>)

# SyncEngine::new signature + MutationBatcher::new call site (Q7)
grep -n "pub fn new\b\|MutationBatcher::new\b\|DEFAULT_BATCH_SIZE\|DEFAULT_BATCH_MS" \
  src/bridge/sync_engine.rs src/bridge/batcher.rs src/constants.rs
# Output: sync_engine.rs:148 (pub fn new(grafeo_db, loro_doc) -> (Self, Receiver, Receiver))
#         sync_engine.rs:161-168 (MutationBatcher::new with DEFAULT_BATCH_SIZE + DEFAULT_BATCH_MS hardcoded)
#         batcher.rs:73 (pub fn new(grafeo_db, batch_size, batch_ms, ...))
#         batcher.rs:93 (pub fn with_defaults(...))  ← precedent for with_batch_config
#         constants.rs:22 (DEFAULT_BATCH_MS = 100), :23 (DEFAULT_BATCH_SIZE = 256)

# GrafeoLoroApp + GrafeoLoroAppBuilder struct fields (M8)
grep -n "pub struct GrafeoLoroApp\|pub struct GrafeoLoroAppBuilder\|ssot_mode\|compression\|storage:" src/app.rs
# Output: 35 (pub struct GrafeoLoroApp), 46 (pub struct GrafeoLoroAppBuilder),
#         47 (storage: Option<Arc<dyn StorageBackend>>),
#         48 (ssot_mode: SsotMode), 49 (compression), 50 (sync_compression), 51 (batch_interval_ms), 52 (batch_max_size)
# GrafeoLoroApp has NO ssot_mode / storage / compression fields — M8 confirmed.

# SyncEngine::new callers (Q7 regression risk)
grep -rn "SyncEngine::new\b" tests/ src/ | wc -l
# Output: 7 test callers + 1 src caller (comment) — widening signature would break 7 tests

# B1 filter — skips ORIGIN_LORO_BRIDGE (M10)
grep -n "ORIGIN_GRAFEO_BRIDGE\|ORIGIN_LORO_BRIDGE\|event.origin ==" src/bridge/sync_engine.rs | head -5
# Output: 234 (if event.origin == ORIGIN_GRAFEO_BRIDGE || event.origin == ORIGIN_LORO_BRIDGE { ... })
```

### Architecture + plan verifications

```bash
# Architecture §4 Step A — Loro direction only (M7)
sed -n '120,125p' docs/grafeo-loro.architecture.md
# Step A describes ONLY Loro→Grafeo hydration; Grafeo→Loro unspecified.

# Architecture §24.4 config table — no grafeo_dir (M9)
grep -n "grafeo_dir" docs/grafeo-loro.architecture.md
# Output: (no matches) — M9 confirmed.

# Implementation plan Phase 4 — both SsotMode branches in scope
sed -n '92,100p' docs/implementation-plan.md
# Task 2 hydrate: Loro mode + Grafeo mode both listed. Task 3 checkpoint: Loro + Grafeo both listed.

# Architecture §24.2 quick-start ordering (M10)
sed -n '1219,1224p' docs/grafeo-loro.architecture.md
# build().await? → app.hydrate("graph_123").await? — hydrate AFTER build
# build's step 6 (spawn_all → init_loro_subscriber) makes subscriber active before hydrate runs — M10.
```

### Compile sanity (no `cargo check` per task spec — Devil is read-only)

```bash
# Confirm Cargo.toml has grafeo = "0.5" (no wal) + no tar dep (B1 / M5)
grep -n "grafeo = \|tar = " Cargo.toml
# Output: 7 (grafeo = "0.5"), no tar line — confirms B1 + M5
```

---

## 6. Devil's Advocate self-assessment

- **Depth**: 2 BLOCKERs + 10 MAJORs + 4 MINORs + 3 NITs + 8 RESOLUTIONS (Q1-Q8). Matches P2T3-DEVIL depth (1 BLOCKER + 5 MAJOR + 5 MINOR + 2 NIT + 8 RESOLUTIONS) scaled up for Phase 4's larger surface (3 tasks vs 1, plus 2 SsotMode branches).
- **Hallucination check**: every file:line citation verified against actual crate source in §5. No fabricated APIs. Two `[UNVERIFIED]` flags (LoroProperty TryFrom grafeo::Value; BridgeMaps::insert_edge) — both are L3-side concerns that do not affect L2 wiring decisions.
- **Anti-Goodhart check**: Q2 recommendation is "defer `SsotMode::Grafeo`" rather than "enable `wal` + refactor to make tests green". The defer path is honest about Phase 4's scope limits.
- **Anti-plenger check**: Q4 (trust orchestrator, no lock), Q8 (defensive reject — anti-plenger #14 "never simplify the basics"), M1 (document the gap, don't fake-implement deltas), M2 (correct the dedup rationale, don't paper over with `ImportStatus::pending`).
- **Scope discipline**: Devil touched ONLY `docs/critiques/p4-l1-devil.md` + `worklog.md`. NO `src/` or `tests/` modifications. M7/M9/M10 architecture-doc updates are flagged for orchestrator, not done in-place.

---

*End of critique document.*
