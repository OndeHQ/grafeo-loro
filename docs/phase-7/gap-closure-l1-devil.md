# P7 L1 Devil's Advocate Critique

**Reviewer**: Devil-advocate (Task ID G2)
**L1 commit**: a35b1da
**Date**: 2026-07-07

## Summary Verdict

**ACCEPTED-WITH-FIXES** — The L1 plan is structurally sound: the per-gap scope is correct, the wire-format contract (Gap A.3) is well-specified, the grafeo 0.5.42 API surface for I12 (Gap B) is REAL (not hallucinated at the API level), and Gap C/D/E/F/G are all reasonable. However, L2 CANNOT proceed without resolving **2 BLOCKERS + 3 MAJORS**: (1) the I12 contract body hallucinates `grafeo::Value::Integer` (3 occurrences) when the real variant is `grafeo::Value::Int64` (plenger #6 Hallucination); (2) Gap D.2 misses a second `MutationBatcher::new` call site at `fuzz/fuzz_targets/consistency.rs:302` (I3b fuzz target) — L1's "ZERO outside src/bridge/sync_engine.rs:297" claim is factually wrong, and the refactor will break the fuzz crate build; (3) `serde_json` is not a direct dependency of the main `grafeo-loro` crate, so L1's parse/build_eph_envelope will not compile without a Cargo.toml edit; (4) I15 currently tests a SIMPLER envelope format that is INCOMPATIBLE with L1's new wire format, so I15 MUST be rewritten (not just "verified"); (5) Q5 is built on a false premise — there is no `impl From<FuzzOp> for LoroOp` in the codebase. All 5 issues have concrete fixes below.

## Per-Gap Critiques

### Gap A (T1 — 11 `unimplemented!()`)

#### CA.1: I12 contract hallucinates `grafeo::Value::Integer` (plenger #6)
- **Issue**: L1 plan lines 220, 238, 249 use `grafeo::Value::Integer(N)` for the I12 write + 2 assertions. This variant DOES NOT EXIST in grafeo 0.5.42. The real enum (verified at `~/.cargo/registry/src/.../grafeo-common-0.5.42/src/types/value.rs:94`) has `Int64(i64)`, NOT `Integer(i64)`. The codebase's OWN `GraphValue::Integer` (in `src/types/values.rs:24`) IS named `Integer`, and `gval_to_grafeo_value` maps it to `grafeo::Value::Int64` (line 192) — so L1 likely confused the two enums.
- **Evidence**:
  - `rg -n 'pub enum Value' ~/.cargo/registry/src/*/grafeo-common-0.5.42/src/types/value.rs` → `value.rs:94: pub enum Value { ... Int64(i64), Float64(f64), ... }` — no `Integer` variant.
  - `rg -n 'GraphValue::Integer.*=>.*GV::' src/types/values.rs` → `values.rs:192: GraphValue::Integer(i) => GV::Int64(i)` — confirms the canonical mapping.
  - L1 plan line 220: `w2.set_node_property(node_id, "v", grafeo::Value::Integer(2))` — won't compile.
  - L1 plan line 238: `Some(grafeo::Value::Integer(1))` — won't compile.
  - L1 plan line 249: `Some(grafeo::Value::Integer(2))` — won't compile.
- **Severity**: blocker
- **Solution for L2**: Replace all 3 occurrences of `grafeo::Value::Integer(N)` with `grafeo::Value::Int64(N)` in the I12 contract. L1 plan line 201 stays as `GraphValue::Integer(1)` (codebase enum — correct). The change is mechanical: 3 sed-equivalent edits.

#### CA.2: `serde_json` not declared as a direct dependency
- **Issue**: L1's Gap A.3 contract uses `serde_json::to_vec(payload)` (line 109) + `serde_json::from_slice(payload_bytes)` (line 122) in `src/presence/socket.rs`. The main `grafeo-loro` crate's `Cargo.toml` does NOT list `serde_json` under `[dependencies]` — only `serde = { version = "1", features = ["derive"] }`. `serde_json` IS in `Cargo.lock` (transitively via grafeo/loro/opentelemetry) and IS a direct dep of the fuzz crate (`fuzz/Cargo.toml:28`), but Rust 2018+ requires explicit declaration for `use serde_json::...` in `src/`.
- **Evidence**:
  - `rg -n 'serde_json' Cargo.toml` → exit code 1 (no matches).
  - `rg -n 'serde_json' fuzz/Cargo.toml` → `fuzz/Cargo.toml:28: serde_json = "1"` (only in fuzz crate).
  - `rg -n 'serde_json' src/` → no matches (no existing usage to grandfather the dep).
- **Severity**: major
- **Solution for L2**: Add `serde_json = "1"` to `[dependencies]` in `Cargo.toml` BEFORE implementing Gap A.3. One line. The version `1` matches the fuzz crate's pin (no version drift).

#### CA.3: I15 tests an INCOMPATIBLE wire format — must be rewritten, not just "verified"
- **Issue**: L1 plan line 127 says "L2 action: verify I15 still aligns with the new `EphEnvelope` return shape; update I15 if it currently calls the old `unimplemented!()` API." This is too vague. The actual situation: I15 (`fuzz/fuzz_targets/consistency.rs:707-760`) currently constructs its OWN envelope as `[magic:4][serde_json_payload:M]` (lines 715-718) — NO `room_id_len`, NO `room_id`, NO `msg_type`. After L1's Gap A.3 lands, `parse_eph_envelope` will expect `[magic:4][u16 room_id_len][room_id][msg_type:1][serde_json:M]`. If I15 is not rewritten, it either (a) fails to call the new APIs at all (Goodhart — testing a format that doesn't match production), or (b) calls `parse_eph_envelope` on its old-shape envelope and gets `Err(InvalidEnvelope("room_id segment truncated"))` (fuzz crash).
- **Evidence**:
  - Read `fuzz/fuzz_targets/consistency.rs:714-731`:
    ```
    let json = serde_json::to_vec(payload).expect("I15: serde_json::to_vec failed");
    let mut envelope = Vec::with_capacity(EPH_MAGIC.len() + json.len());
    envelope.extend_from_slice(EPH_MAGIC);
    envelope.extend_from_slice(&json);
    // ... decode via serde_json::from_slice(&envelope[EPH_MAGIC.len()..])
    ```
  - L1 plan lines 66-78: the new wire format has 4 + 2 + N + 1 + M bytes (magic + u16 + room + msg_type + payload). I15's envelope has 4 + M bytes. They are incompatible.
- **Severity**: major
- **Solution for L2**: Rewrite I15 to call the new production APIs:
  ```rust
  fn check_i15_presence_envelope_integrity(payload: &PresencePayload) {
      let room_id = "V/i15-roundtrip-test";
      let envelope = PresenceManager::build_eph_envelope(room_id, payload)
          .expect("I15: build_eph_envelope failed");
      let decoded = PresenceManager::parse_eph_envelope(&envelope)
          .expect("I15: parse_eph_envelope failed");
      assert_eq!(decoded.room_id, room_id, "I15: room_id round-trip mismatch");
      assert_eq!(decoded.payload.peer_id.0, payload.peer_id.0, "I15: peer_id mismatch");
      // ... 4 more field assertions on decoded.payload (cursor_x, cursor_y, last_active_ts, active_node)
      // Negative test: non-%EPH bytes MUST be rejected.
      let bad_bytes = b"NOT-EPH some random bytes";
      assert!(PresenceManager::parse_eph_envelope(bad_bytes).is_err(),
              "I15: parse_eph_envelope accepted non-%EPH bytes");
  }
  ```
  This replaces the manual envelope construction with the production API and tests both positive (round-trip) + negative (rejection) paths. Update the I15 doc-comment to reflect the new contract.

#### CA.4: `PeerId` serde format diverges from arch §12 (pre-existing, inherited)
- **Issue**: Architecture §12 (`docs/grafeo-loro.architecture.md:499`) specifies `pub peer_id: u64` (bare integer) in `PresencePayload`. The codebase has `pub peer_id: PeerId(pub u64)` (`src/types/presence.rs:6` + `src/types/ids.rs:14`). `PeerId(pub u64)` lacks `#[serde(transparent)]`, so serde_json serializes it as `[42]` (1-element JSON array), not bare `42`. L1's wire format inherits this drift — the `serde_json::to_vec(payload)` call produces `{"peer_id":[42],...}` not `{"peer_id":42,...}`. Doesn't break I15 round-trip (both encode + decode use the same serde format), but diverges from arch §12 spec.
- **Evidence**:
  - `rg -n 'pub struct PeerId' src/types/ids.rs` → `ids.rs:14: pub struct PeerId(pub u64);` — no `#[serde(transparent)]`.
  - `rg -n 'peer_id' docs/grafeo-loro.architecture.md` → line 499-500: `pub peer_id: u64,` (bare).
- **Severity**: minor (pre-existing, doesn't block Phase 7)
- **Solution for L2** (optional): Add `#[serde(transparent)]` to `PeerId(pub u64)` in `src/types/ids.rs:14` so the wire format matches arch §12. This is a 1-line edit, no behavior change for the codebase (PeerId is still a distinct type), but the JSON wire form becomes `42` instead of `[42]`. If skipped, document the divergence in a NOTE on `PresencePayload` so future maintainers know.

#### CA.5: `VarString` length-prefix scheme undocumented in arch §12
- **Issue**: Architecture §12 (`docs/grafeo-loro.architecture.md:488`) specifies `Room ID VarString` without defining the length-prefix scheme. L1 chose `u16 LE length + UTF-8 bytes` (Gap A.3 lines 73-74). Reasonable choice (room IDs like `"graph_123"` are 9 bytes; u16 max=65535 is plenty; u8 max=255 might be tight for path-style IDs; u32 wasteful). But L1's choice is undocumented in the architecture spec — future maintainers reading arch §12 won't know whether it's u8/u16/u32/LE/BE/varint.
- **Evidence**:
  - `rg -n 'VarString' docs/grafeo-loro.architecture.md` → only at line 488 (no length-prefix definition).
  - L1 plan line 73: `| 4 | 2 | room_id_len | u16 little-endian |` — L1's choice.
- **Severity**: nit
- **Solution for L2**: After implementing Gap A.3, add a one-liner to arch §12: `// VarString = u16 LE length prefix + UTF-8 bytes`. This pins the wire format in the architecture SSOT so future readers don't have to grep `src/presence/socket.rs` to discover it.

### Gap B (I12)

#### CB.1: Covered by CA.1 (BLOCKER — `grafeo::Value::Integer` hallucination)
- **Issue**: Same as CA.1. The I12 contract body cannot compile as written.
- **Severity**: blocker
- **Solution**: See CA.1.

#### CB.2: I12 write-2 path bypasses `apply_loro_op` — confirm this is intentional
- **Issue**: L1 plan lines 213-221 use `w2.set_node_property(node_id, "v", ...)` directly on the grafeo session (NOT via `apply_loro_op`). This is correct for I12's purpose (it needs to test `set_viewing_epoch` directly, which `apply_loro_op` doesn't expose), but it means I12 doesn't exercise the bridge's `apply_loro_op` code path on write-2. If a future change to `apply_loro_op` breaks the bridge → grafeo mapping, I12 won't catch it on write-2.
- **Evidence**: L1 plan line 220: `w2.set_node_property(node_id, "v", grafeo::Value::Integer(2))` — direct grafeo API, not `apply_loro_op(&w2, &op, maps)`.
- **Severity**: nit (acceptable — I12's purpose is grafeo-level snapshot isolation, not bridge integration; I6 already covers `apply_loro_op` round-trip)
- **Solution**: Add a 1-line comment in the I12 body: `// Direct grafeo API (not apply_loro_op) — I12 tests set_viewing_epoch semantics, not bridge integration.`

### Gap C (deferred spans note)

#### CC.1: L1's update is accurate — no critique
- **Issue**: None. L1's Gap C correctly identifies that Phase 7 T1 replaces panics with errors (not real implementations), so child spans remain observationally pointless until the bodies get real implementations. The updated note text makes the dependency chain explicit.
- **Severity**: nit (no action needed)
- **Solution**: ACCEPT as written.

### Gap D (`#[allow]` refactors)

#### CD.1: BLOCKER — Gap D.2 MISSES the second `MutationBatcher::new` call site
- **Issue**: L1 plan line 472 claims: "**Other call sites**: ZERO outside `src/bridge/sync_engine.rs:297` (verified via `rg -n 'MutationBatcher::new\b' src/ tests/ fuzz/`)." This is FALSE. There IS a second call site at `fuzz/fuzz_targets/consistency.rs:302` (I3b "MutationBatcher::run does not panic" check). The fuzz I3b target constructs a `MutationBatcher` with 9 args (matching the current `new` signature) to test the batcher-drain behavior. L1's BatcherConfig refactor would BREAK the fuzz crate build.
- **Evidence**:
  - `rg -n 'MutationBatcher::new\b' fuzz/` → `fuzz/fuzz_targets/consistency.rs:302: let batcher = Arc::new(MutationBatcher::new(`.
  - Read `fuzz/fuzz_targets/consistency.rs:299-312`:
    ```
    rt.block_on(async move {
        let bridge_origin_epochs = Arc::new(RwLock::new(HashSet::<EpochId>::new()));
        let (shutdown_tx, _) = broadcast::channel(1);
        let batcher = Arc::new(MutationBatcher::new(
            db.clone(),
            256, 100,
            bridge_origin_epochs,
            maps.clone(),
            shutdown_tx.clone(),
            None, None, None,
        ));
    ```
  - L1 plan line 472 explicitly says "ZERO outside src/bridge/sync_engine.rs:297" — contradicts the rg result.
- **Severity**: blocker
- **Solution for L2**: Add a "D.2a — fuzz I3b call-site migration" subsection to Gap D.2:
  ```rust
  // fuzz/fuzz_targets/consistency.rs:302 — NEW (2 args):
  let batcher = Arc::new(MutationBatcher::new(
      db.clone(),
      BatcherConfig {
          batch_size: 256,
          batch_ms: 100,
          bridge_origin_epochs,
          maps: maps.clone(),
          shutdown_tx: shutdown_tx.clone(),
          metrics: None,
          tracer: None,
          health: None,
      },
  ));
  ```
  Also: the `with_defaults` migration (L1 plan lines 434-438) is fine — `with_defaults` has 0 callers, so no fuzz/src migration needed.

#### CD.2: Q3 — `AppTelemetryConfig` should live in `src/app.rs`, NOT `src/config.rs`
- **Issue**: L1 plan Q3 recommends placing `AppTelemetryConfig` in `src/config.rs` ("crate config SSOT"). This is wrong for 3 reasons:
  1. **`src/config.rs` is currently a leaf module with ZERO `use` imports** (verified: `rg -n '^use ' src/config.rs` → no matches). Adding `AppTelemetryConfig` requires 5+ new imports (`StorageBackend`, `MetricsRegistry`, `HealthProbe`, `SharedTracer`, `JoinHandle`) — converting a clean leaf into a hub module, violating anti-plenger #5 (High Cohesion).
  2. **`AppTelemetryConfig` is constructor-specific** to `GrafeoLoroApp::from_sync_engine_with_telemetry` — its 7 fields exactly match the 7 non-positional args of that constructor. Putting it in config.rs decouples it from its only consumer, breaking cohesion.
  3. **`AppTelemetryConfig` is a runtime-resource-bundle** (`Option<Arc<dyn StorageBackend>>`, `Option<Arc<MetricsRegistry>>`, `Option<SharedTracer>`, `Option<Vec<JoinHandle<()>>>`) — these are runtime resource handles, not plain-data config values like `AppConfig`'s `usize`/`u64`/`bool`/`SsotMode`/`CompressionType` fields. Different lifecycle, different semantics.
- **Evidence**:
  - `rg -n '^use ' src/config.rs` → no matches (leaf module).
  - `rg -n 'pub struct AppConfig' src/config.rs` → `config.rs:17` — all Copy + plain-data fields (SsotMode, CompressionType, usize, u64, bool). No Arc<dyn Trait>, no JoinHandle.
  - L1 plan lines 327-339 — AppTelemetryConfig has 4 `Option<Arc<...>>` + 1 `Option<Vec<JoinHandle>>` + 1 `Option<Arc<dyn StorageBackend>>`. Fundamentally different shape from AppConfig.
- **Severity**: minor
- **Solution for L2**: Place `AppTelemetryConfig` in `src/app.rs` (next to `from_sync_engine_with_telemetry`). Keep `BatcherConfig` in `src/bridge/batcher.rs` (L1's recommendation here is correct — batcher.rs is the only consumer). Asymmetry is justified: `AppTelemetryConfig` is constructor-bundled (not a "config" in the data sense); `BatcherConfig` is constructor-bundled to `MutationBatcher` (also not a "config" in the data sense).

#### CD.3: Q4 — `with_defaults` keep ruling
- **Issue**: L1 Q4 recommends KEEP `MutationBatcher::with_defaults` despite 0 callers. Devil ruling: AGREE. The function is documented as "reserved for future test fixtures" per its doc-comment (verified: `rg -n 'with_defaults' src/bridge/batcher.rs` → line 137). Removing it now would require re-adding it later when test fixtures want a "defaults" path. Anti-plenger #11 (Deletion over addition) does NOT apply when the API is documented as a future-use alternative (the criterion is "no documented future need", not "zero current callers").
- **Evidence**: `rg -n 'with_defaults' src/ tests/ fuzz/` → only `src/bridge/batcher.rs:137` (the declaration). 0 callers.
- **Severity**: nit (no action — L1 ruling accepted)
- **Solution**: ACCEPT L1's "KEEP" ruling. L2 should leave `with_defaults` alone but adapt its signature to take `BatcherConfig` (L1 plan lines 434-438) for API consistency with `new`.

#### CD.4: Gap D.4 `async_yields_async` — no change needed (L1 ruling correct)
- **Issue**: L1 ruling is "NO TEXT CHANGE needed" — Devil agrees. The 3 `async_yields_async` reasons at `src/bridge/sync_engine.rs:459/553/661` are already permanent-design phrasing (verified by L1's own `rg` step recommendation).
- **Severity**: nit (no action — L1 ruling accepted)
- **Solution**: ACCEPT. L2 should run the verification `rg -n -B2 -A4 'async_yields_async' src/bridge/sync_engine.rs` and confirm no "deferred"/"TODO"/"future phase" language in adjacent comments.

### Gap E (EncFuzz consolidation)

#### CE.1: BLOCKER (MAJOR) — Q5 is built on a FALSE premise; no `From<FuzzOp> for LoroOp` exists
- **Issue**: L1 plan Q5 (line 644) asks: "should the `From<FuzzOp> for LoroOp` impl also move to `lib.rs`?" — implying such an impl exists. It does NOT. The actual adapter is a FREE FUNCTION `fn convert_fuzz_op(op: &FuzzOp) -> LoroOp` at `fuzz/fuzz_targets/consistency.rs:148`. L1 hallucinated the `From` impl. Also: L1 plan line 527 says "Move `FuzzOp` + `FuzzValue` (with `#[derive(Arbitrary, Debug, Clone)]` + `impl FuzzValue { fn to_graph_value(...) }` + `From<FuzzOp> for LoroOp`) from `fuzz/fuzz_targets/consistency.rs:85–155` into `fuzz/fuzz_targets/lib.rs`." — the `From<FuzzOp> for LoroOp` part is fabricated.
- **Evidence**:
  - `rg -n 'impl From<FuzzOp>|impl From<FuzzValue>|impl ::core::convert::From' fuzz/` → no matches.
  - `rg -n 'convert_fuzz_op' fuzz/fuzz_targets/consistency.rs` → line 148 (`fn convert_fuzz_op(op: &FuzzOp) -> LoroOp`), called at lines 315 + 795.
- **Severity**: major
- **Solution for L2**:
  1. Q5 Devil ruling: **DISAGREE with L1's premise** — there is no `From<FuzzOp> for LoroOp` impl. Move the FREE FUNCTION `convert_fuzz_op` + `FuzzOp` + `FuzzValue` + `impl FuzzValue { fn to_graph_value(...) }` to `fuzz/fuzz_targets/lib.rs`. Make all 4 `pub` (or `pub(crate)`).
  2. Bump `to_graph_value` visibility from private (`fn to_graph_value(&self)`) to `pub(crate)` (it's currently private — moving to lib.rs requires visibility bump so consistency.rs can call it via `use grafeo_loro_fuzz::FuzzValue`).
  3. Update L1 plan line 527 to remove `+ From<FuzzOp> for LoroOp` and replace with `+ convert_fuzz_op free function` + visibility note.
  4. The consolidation itself (replacing `EncFuzzOp`/`EncFuzzValue` in `gen_corpus.rs` with `use grafeo_loro_fuzz::{FuzzOp, FuzzValue}`) is straightforward per L1 plan steps 4-5.

#### CE.2: `to_graph_value` visibility bump required
- **Issue**: `impl FuzzValue { fn to_graph_value(&self) -> GraphValue }` is currently private (`fuzz/fuzz_targets/consistency.rs:133`). After moving to `lib.rs`, it must be at least `pub(crate)` for consistency.rs (a separate binary in the same crate) to call it via `use grafeo_loro_fuzz::FuzzValue`.
- **Evidence**: `rg -n 'fn to_graph_value' fuzz/fuzz_targets/consistency.rs` → `consistency.rs:133: fn to_graph_value(&self) -> GraphValue {` — no `pub`.
- **Severity**: nit
- **Solution for L2**: Bump to `pub(crate)` (or `pub` if grafeo-loro-fuzz crate wants it externally visible — `pub(crate)` is sufficient since both binaries are in the same crate).

### Gap F (stale docs)

#### CF.1: Q6 count "6 remaining" is wrong — should be 7
- **Issue**: L1 plan Q6 (line 593) proposes updating `src/app.rs:62` doc-comment to say "6 remaining `unimplemented!()` bodies are replaced with `Err(NotYetImplemented(...))`". But L1's own Q6 explanation counts 5 standalone pub fns + 2 match arms = 7 sites. The "6" in the proposed text is internally inconsistent with L1's own analysis.
- **Evidence**: Post-Phase-7 `Err(NotYetImplemented(...))` sites:
  1. `src/app.rs:362` `query` (row 2)
  2. `src/app.rs:370` `update_text` (row 3)
  3. `src/app.rs:384` `generate_embedding` (row 4)
  4. `src/app.rs:586` `checkpoint` `SsotMode::Grafeo` arm (row 5)
  5. `src/app.rs:982` `hydrate` `SsotMode::Grafeo` arm (row 6)
  6. `src/app.rs:995` `broadcast_presence` (row 7)
  7. `src/presence/socket.rs:28` `PresenceManager::broadcast` (row 9)
  Total: **7** sites (5 standalone pub fns + 2 match arms). L1's "6" is wrong.
- **Severity**: minor
- **Solution for L2**: Rewrite the `src/app.rs:62` doc-comment to say "7 deferred-feature call sites return `Err(NotYetImplemented(...))` (5 standalone pub methods + 2 `SsotMode::Grafeo` match arms in `checkpoint`/`hydrate`)" — or, for brevity: "Most methods are implemented; 7 deferred-feature call sites return `Err(NotYetImplemented(...))` (see Gap A in `docs/phase-7/gap-closure-l1-plan.md`)."

### Gap G (NOTE removals)

#### CG.1: 7 NOTE comments verified — removal is correct
- **Issue**: None. L1's Gap G correctly identifies all 7 `// NOTE: body unimplemented!()` comments (4 in `app.rs` lines 358/366/380/991, 3 in `presence/socket.rs` lines 24/32/40). Removal is the right action (the NOTE was a Phase 6 marker for "T1 excluded per user" — now stale since Phase 7 un-excludes T1).
- **Evidence**: `rg -n '// NOTE: body unimplemented' src/` → exactly 7 matches as L1 claims.
- **Severity**: nit (no action — L1 ruling accepted)
- **Solution**: ACCEPT. L2 removes all 7 NOTEs per L1 plan table.

## L1 Open Questions — Resolutions

### Q1: `build_eph_envelope` signature (Option A vs B)
- **L1 recommendation**: Option B (room_id arg, returns full envelope)
- **Devil ruling**: **AGREE**. Option B matches arch §12 diagram literally (`[magic][room_id_varstring][msg_type][payload]`). The caller `broadcast` already has `&self.room_id`. The new `EphEnvelope { room_id, payload }` struct return documents the envelope contents more clearly than a tuple.
- **Action for L2**: Implement per L1 plan lines 87-102 (Option B). Use signature `pub fn build_eph_envelope(room_id: &str, payload: &PresencePayload) -> Result<Vec<u8>>` + `pub fn parse_eph_envelope(bytes: &[u8]) -> Result<EphEnvelope>`. Add `InvalidEnvelope(String)` error variant to `GrafeoLoroError` per L1 plan line 107. Also document the `VarString = u16 LE + UTF-8` scheme in arch §12 (see CA.5).

### Q2: I12 collision risk on shared DB
- **L1 recommendation**: Accept distinct keys (no collision)
- **Devil ruling**: **AGREE**. Verified: `V/i6-ryow-test` (consistency.rs:444), `V/i10-vec-test` (consistency.rs:582), `V/i12-snap-test` (L1 plan line 197). Three distinct keys, no collision risk even if all three checks run on the same `db`/`maps`. The `BridgeMaps::node_id_map` is keyed by `loro_key` string — distinct strings = distinct entries.
- **Action for L2**: ACCEPT L1 ruling. No isolation changes needed. I12 stays on the shared `db: &Arc<GrafeoDB>` + `maps: &Arc<BridgeMaps>` from the fuzz entry point (preserves "live state" framing per L1 plan line 270).

### Q3: `AppTelemetryConfig` location
- **L1 recommendation**: `src/config.rs` (crate config SSOT)
- **Devil ruling**: **DISAGREE**. `AppTelemetryConfig` should live in `src/app.rs` (next to `from_sync_engine_with_telemetry`). See critique CD.2 for full rationale: (1) config.rs is a leaf module with 0 imports — adding 5+ imports breaks high cohesion; (2) AppTelemetryConfig is constructor-specific (7 fields = 7 constructor args); (3) AppTelemetryConfig is a runtime-resource-bundle (Arc<dyn Trait> handles), not plain-data config. `BatcherConfig` → `src/bridge/batcher.rs` (L1's recommendation here is correct).
- **Action for L2**: Place `AppTelemetryConfig` struct in `src/app.rs` (next to the `from_sync_engine_with_telemetry` constructor at line 252). Place `BatcherConfig` struct in `src/bridge/batcher.rs` (next to `MutationBatcher::new` at line 105). Both structs derive `Debug, Clone` (verified Clone compiles — Arc<T>: Clone for any T: ?Sized, no T: Clone requirement).

### Q4: `MutationBatcher::with_defaults` (0 callers — remove/keep?)
- **L1 recommendation**: KEEP — reserved for future test fixtures
- **Devil ruling**: **AGREE**. The function is documented (line 137) as a future-use API alternative. Anti-plenger #11 (Deletion over addition) does NOT apply when the API has a documented future need. 0 current callers ≠ 0 future callers; removing now creates re-add churn later.
- **Action for L2**: KEEP `with_defaults`. Adapt its signature to take `BatcherConfig` per L1 plan lines 434-438 (override `batch_size`/`batch_ms` with `DEFAULT_BATCH_SIZE`/`DEFAULT_BATCH_MS`, delegate to `new`). L2 should add a `// Reserved for future test fixtures — see Q4 ruling in gap-closure-l1-devil.md` comment to guard against future Hunter nit re-raise.

### Q5: `From<FuzzOp> for LoroOp` impl location
- **L1 recommendation**: `lib.rs` (tightly coupled to FuzzOp)
- **Devil ruling**: **DISAGREE with L1's premise** — there is no `impl From<FuzzOp> for LoroOp` in the codebase (verified via `rg -n 'impl From<FuzzOp>' fuzz/` → no matches). The actual adapter is a FREE FUNCTION `fn convert_fuzz_op(op: &FuzzOp) -> LoroOp` at `fuzz/fuzz_targets/consistency.rs:148`. L1 hallucinated the From impl (plenger #6).
- **Action for L2**: Move `FuzzOp` + `FuzzValue` + `impl FuzzValue { pub(crate) fn to_graph_value(...) }` + `fn convert_fuzz_op(op: &FuzzOp) -> LoroOp` (free function, no From impl) to `fuzz/fuzz_targets/lib.rs`. Bump `to_graph_value` visibility from private to `pub(crate)`. Both binaries `use grafeo_loro_fuzz::{FuzzOp, FuzzValue, convert_fuzz_op};`. Update L1 plan line 527 to remove the `From<FuzzOp> for LoroOp` mention and replace with `convert_fuzz_op` free function.

### Q6: Post-Phase-7 `unimplemented!()` count
- **L1 recommendation**: Devil to verify count "6 remaining"
- **Devil ruling**: L1's "6" is wrong. Correct count: **7 `Err(NotYetImplemented(...))` returns** post-Phase-7 (5 standalone pub fns + 2 `SsotMode::Grafeo` match arms). L1's own Q6 explanation (line 593) counts 5+2=7, contradicting its own "6" in the proposed doc-comment text. The correct post-Phase-7 count of `unimplemented!()` macro calls in src/ is **0** (all 11 are eliminated: 1 removed via Default-impl deletion, 6 replaced with `Err(NotYetImplemented(...))`, 4 real-implemented).
- **Action for L2**: Update the `src/app.rs:62` doc-comment per L1 plan lines 585-591, but change "6 remaining" to "7 deferred-feature call sites" (or "5 pub methods + 2 `SsotMode::Grafeo` match arms"). See critique CF.1 for the exact wording.

## Arc Alignment Audit

Cross-check L1 plan against `docs/grafeo-loro.architecture.md`:

- **§12 (In-Memory Ephemeral Presence)**: L1's wire format (Gap A.3) matches the diagram literally (`[magic:4][room_id_varstring][msg_type:1][payload:M]`). TWO divergences:
  1. `VarString` length-prefix scheme is L1's choice (u16 LE) — undocumented in arch §12. Fix: add a one-liner to arch §12 (see CA.5).
  2. `PresencePayload.peer_id` is `PeerId(pub u64)` (no `#[serde(transparent)]`) — serializes as `[42]` instead of bare `42` per arch §12's `peer_id: u64`. Pre-existing drift, inherited by L1 (see CA.4).

- **§19 (MVCC Snapshot Isolation)**: L1's I12 plan correctly invokes grafeo's `set_viewing_epoch`/`clear_viewing_epoch`/`get_node_property` time-travel API. ALL grafeo APIs verified against grafeo-engine-0.5.42 source:
  - `db.session()` → `database/mod.rs:1663` ✓
  - `db.session_with_cdc(bool)` → `database/mod.rs:1728` ✓
  - `session.begin_transaction(&mut self) -> Result<()>` → `session/mod.rs:3883` (default isolation = `SnapshotIsolation`, per doc-comment at line 3877) ✓
  - `session.set_viewing_epoch(&self, epoch: EpochId) -> ()` → `session/mod.rs:730` ✓
  - `session.clear_viewing_epoch(&self) -> ()` → `session/mod.rs:735` ✓
  - `session.viewing_epoch(&self) -> Option<EpochId>` → `session/mod.rs:741` ✓
  - `session.set_node_property(&self, id, key, value) -> Result<()>` → `session/mod.rs:5012` ✓
  - `session.get_node_property(&self, id, key) -> Option<Value>` → `session/mod.rs:5172` ✓
  - `prepared.commit() -> Result<EpochId>` → `transaction/prepared.rs:124` ✓
  - `EpochId(pub u64)` with `as_u64()` → `grafeo-common-0.5.42/src/types/id.rs:223+247` ✓
  Architecture §19 alignment is SOUND. No misalignment. The hallucination in I12 (CA.1) is at the `Value` variant level, not the API surface level.

- **§23 (Observability)**: L1's `AppTelemetryConfig` refactor doesn't change telemetry semantics — only bundles 7 args into a struct. No architecture divergence. The 3 `async_yields_async` allows on `spawn_*_worker` (Gap D.4) are correctly KEPT (already permanent-design rationale).

## Anti-Plenger Early Scan

- **Bloat risk** in new structs (`AppTelemetryConfig`, `BatcherConfig`, `EphEnvelope`): **LOW**. All 3 are config-bundles / value-structs with 2-7 plain fields. No duplicated logic, no reinvented utilities. `EphEnvelope` is a 2-field value type returned by `parse_eph_envelope` — minimal + fit-for-purpose. ✓

- **Hallucination risk** in I12: **CONFIRMED** — `grafeo::Value::Integer` does not exist (BLOCKER CA.1). The grafeo API surface (db.session, set_viewing_epoch, etc.) IS real (verified against grafeo-engine source). Also confirmed in Q5 — `From<FuzzOp> for LoroOp` doesn't exist (CE.1). L1 hallucinated at the variant/method-impl level, not at the API-path level.

- **Hallucination risk** in Q1 architecture §12: **CLEAN** — magic bytes "%EPH" (4B), VarString, msg_type=0x01 all match arch §12 diagram literally. No hallucination at the wire-format level.

- **Happy-path bias** in `parse_eph_envelope`: **LOW**. L1's contract (Gap A.3 lines 113-123) explicitly handles 5 failure modes: (1) buffer too short, (2) bad magic, (3) room_id segment truncated, (4) non-UTF-8 room_id, (5) unsupported msg_type, (6) serde_json decode failure. All return `Err(InvalidEnvelope(...))`. No happy-path bias. The negative test in I15 (`bad_bytes` rejection) exercises failure-mode (2). ✓

- **Goodhart risk** in I15 after L1's changes: **HIGH if I15 is not rewritten** (see CA.3). I15 currently tests a format that doesn't match the production wire format. L2 MUST rewrite I15 to use the new `build_eph_envelope`/`parse_eph_envelope` APIs.

- **Band-Aid risk** in `NotYetImplemented` variant: **LOW**. L1's A.1 rationale (lines 21-25) correctly argues against reusing `Config`/`Bridge` for deferred-feature semantics — adding a dedicated variant is the cleanest path. The 9 call sites use it explicitly (not via `?`-propagation), so no `From` impl needed. ✓

- **Backward Compatibility Slaves** risk: **LOW**. L1's Gap A.2 row 1 (REMOVE `impl Default for AppConfig`) is a clean structural break (anti-plenger #1). Gap D refactors also clean (no legacy adapters). ✓

## Recommendations for L2

Numbered list of concrete actions. Each is specific (file + change), actionable (L2 can do without further research), and traced to a critique or Q-resolution.

1. **[BLOCKER — traces to CA.1]** Fix the I12 contract: replace 3 occurrences of `grafeo::Value::Integer(N)` with `grafeo::Value::Int64(N)` in the L1 plan body (lines 220, 238, 249). When implementing in `fuzz/fuzz_targets/consistency.rs::check_i12_mvcc_snapshot_isolation`, use `grafeo::Value::Int64(2)` for the write-2 value + `Some(grafeo::Value::Int64(1))` / `Some(grafeo::Value::Int64(2))` for the assertions. The L1 plan's `GraphValue::Integer(1)` at line 201 stays unchanged (codebase enum, correct).

2. **[BLOCKER — traces to CD.1]** Add fuzz I3b call-site migration to Gap D.2. Rewrite `fuzz/fuzz_targets/consistency.rs:302-312` to construct `BatcherConfig { batch_size: 256, batch_ms: 100, bridge_origin_epochs, maps: maps.clone(), shutdown_tx: shutdown_tx.clone(), metrics: None, tracer: None, health: None }` and pass it as the second arg to `MutationBatcher::new(db.clone(), config)`. Update the L1 plan Gap D.2 "Other call sites" claim from "ZERO outside src/bridge/sync_engine.rs:297" to "ONE additional call site at fuzz/fuzz_targets/consistency.rs:302 (I3b fuzz target)".

3. **[MAJOR — traces to CA.2]** Add `serde_json = "1"` to `[dependencies]` in `Cargo.toml` (between line 31 `serde` and line 32 `async-trait`). One line. Required before L2 implements Gap A.3 (parse/build_eph_envelope) — otherwise `serde_json::to_vec` / `serde_json::from_slice` in `src/presence/socket.rs` won't resolve.

4. **[MAJOR — traces to CA.3]** Rewrite I15 (`fuzz/fuzz_targets/consistency.rs:707-760`) to use the new production APIs. Replace the manual envelope construction with `PresenceManager::build_eph_envelope(room_id, payload)` + `PresenceManager::parse_eph_envelope(&envelope)`. Use a fixed `room_id = "V/i15-roundtrip-test"` for the round-trip. Update the 5 per-field assertions to read from `decoded.payload.*` (not `decoded.*`). Update the negative test to call `PresenceManager::parse_eph_envelope(bad_bytes)` and assert `.is_err()`.

5. **[MAJOR — traces to CE.1]** Drop L1's false premise about `impl From<FuzzOp> for LoroOp`. Move `FuzzOp` + `FuzzValue` + `impl FuzzValue { pub(crate) fn to_graph_value(&self) -> GraphValue }` + `fn convert_fuzz_op(op: &FuzzOp) -> LoroOp` (FREE FUNCTION, no From impl) from `fuzz/fuzz_targets/consistency.rs:85-155` to `fuzz/fuzz_targets/lib.rs`. Make all 4 items `pub` (or `pub(crate)`). Replace `EncFuzzOp`/`EncFuzzValue` in `fuzz/fuzz_targets/gen_corpus.rs:179-218` with `use grafeo_loro_fuzz::{FuzzOp, FuzzValue};`. Update `enc_fuzz_op`/`enc_fuzz_value` signatures to take `&FuzzOp`/`&FuzzValue`.

6. **[MINOR — traces to Q3]** Place `AppTelemetryConfig` in `src/app.rs` (next to `from_sync_engine_with_telemetry` at line 252), NOT `src/config.rs`. Keep `BatcherConfig` in `src/bridge/batcher.rs` (L1's recommendation here is correct). Both structs derive `Debug, Clone` — verified Clone compiles (Arc<T>: Clone for any T: ?Sized).

7. **[MINOR — traces to Q6/CF.1]** Update the `src/app.rs:62` doc-comment to say "7 deferred-feature call sites return `Err(NotYetImplemented(...))`" (or "5 pub methods + 2 `SsotMode::Grafeo` match arms"). L1's "6 remaining" is internally inconsistent.

8. **[NIT — traces to CA.5]** After L2 lands Gap A.3, add a one-liner to `docs/grafeo-loro.architecture.md` §12 (after line 491) documenting the `VarString = u16 LE length + UTF-8 bytes` scheme. This pins the wire format in the architecture SSOT.

9. **[NIT — traces to CA.4]** Optional: add `#[serde(transparent)]` to `PeerId(pub u64)` in `src/types/ids.rs:14` so the JSON wire format matches arch §12's `peer_id: u64` (bare integer, not `[42]` array). Pre-existing drift, inherited by L1. If skipped, add a `// NOTE: peer_id serializes as [u64] not bare u64 — see arch §12 drift` comment on `PresencePayload`.

10. **[ACCEPT — no action]** Gaps C, D.3, D.4, G: ACCEPT L1's plan as written. No fixes needed. L2 implements verbatim.
