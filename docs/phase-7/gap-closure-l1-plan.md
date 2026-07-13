# Phase 7 Gap-Closure L1 Plan (Publish-Ready)

**Agent**: L1-scaffolding (Task ID G1)
**Scope**: Close all gaps from Phase 6 to publish-ready.
**Method**: Plonga-Plongo-Loop (klemer L1→L2→L3 + Devil + Hunter).
**Branch**: `phase-6` (continue — no new branch).
**Base commit**: `13f19bf` (Phase 6 close).

This is the **single source of truth** for the gap-closure work (anti-plenger #2 — SSOT). L2 implements per these contracts; L3 fills the meat; Devil + Hunter audit afterward.

---

## Gap A — T1: Replace 11 `unimplemented!()` calls

### A.1: New error variant `NotYetImplemented`

**File**: `src/error.rs`
**Variant**: `NotYetImplemented(String)`
**Display**: `#[error("not yet implemented: {0}")]`

**Rationale** (DRY + YAGNI review):
- Existing variants (`Config`, `Bridge`, `Compression`) are domain-specific; none semantically fits "feature deferred to a future phase".
- `Config` is **rejected**: it currently carries "configuration invalid" semantics (e.g. `Config("storage backend not set")`, `Config("grafeo_dir required for SsotMode::Grafeo")`). Routing deferred-feature errors through `Config` would conflate user-actionable config errors with not-yet-shipped capability gaps — breaking the diagnostic contract of `Config`.
- `Bridge` is **rejected**: same conflation risk (Bridge = "runtime bridge error", not "feature not shipped").
- Adding a dedicated variant is the **fewest-LOC + highest-observability** path (anti-plenger #10 + #8): operators can filter `NotYetImplemented` out of incident alerts (it's expected in this release) without filtering out real `Config`/`Bridge` errors.

**No `From` impl** needed — the variant is constructed explicitly at 9 call sites (A.2 rows 2–11 below use `Err(NotYetImplemented(...))` directly; row 1 is the `Default`-removal case with no error construction).

**Callers using `?`**: none — every call site already returns `Result<T>` and the current `unimplemented!()` already terminates before any `?`-propagation path. No `From` impl required.

### A.2: Per-function treatment table

| # | File:Line | Function | Current | New | Notes |
|---|----------|----------|---------|-----|-------|
| 1 | `src/config.rs:31` | `AppConfig::default()` | `unimplemented!()` | **REMOVE the `impl Default for AppConfig` block (rows 1a below)** | Evidence: `rg -n 'AppConfig::default\|Default for AppConfig' src/ tests/ fuzz/` returns ZERO call sites (only the impl itself at `src/config.rs:29` + a doc reference at `docs/critiques/p4-l1-devil.md:109` and `docs/phase-6/p6-l1-devil.md:258`). Production code uses `GrafeoLoroAppBuilder` (`src/app.rs:169` `pub fn builder()`) — never `AppConfig::default()`. The `AppConfig` struct itself stays (it's re-exported at `src/lib.rs:16` + documented in `README.md:91`). Anti-plenger #11 (Deletion over addition) → delete the impl; do NOT add a real `Default`. The `#[derive(Default)]` option is rejected: `AppConfig` has 9 fields whose safe defaults (`batch_max_size`, `hydration_chunk_size`, etc.) live in `crate::constants`; a `derive(Default)` would silently produce `0` for `usize` fields, masking misconfiguration. |
| 2 | `src/app.rs:362` | `GrafeoLoroApp::query(gql)` | `unimplemented!("query is Phase 4+ scope")` | `Err(NotYetImplemented("GrafeoLoroApp::query — Phase 4+ scope (GQL engine integration)".into()))` | Drop the `let _ = gql;` line — `gql` becomes a `_gql` parameter OR keep `gql` + the underscore assign (L2 picks). `#[instrument(skip(self, gql), level = "info")]` stays. |
| 3 | `src/app.rs:370` | `GrafeoLoroApp::update_text(...)` | `unimplemented!("update_text is Phase 3 scope")` | `Err(NotYetImplemented("GrafeoLoroApp::update_text — Phase 3 scope (collaborative text fields)".into()))` | `async` signature unchanged. `#[instrument(skip(self, text), level = "info")]` stays. |
| 4 | `src/app.rs:384` | `GrafeoLoroApp::generate_embedding(...)` | `unimplemented!("generate_embedding is Phase 4+ scope (depends on Task 4's VectorOffloadManager::handle_text_update)")` | `Err(NotYetImplemented("GrafeoLoroApp::generate_embedding — Phase 4+ scope (depends on VectorOffloadManager::handle_text_update)".into()))` | `#[instrument(skip(self), level = "info")]` stays. |
| 5 | `src/app.rs:586` | `checkpoint()` `SsotMode::Grafeo` arm | `unimplemented!("P5: SsotMode::Grafeo checkpoint — requires wal feature + ArcSwap grafeo_db field — see P4-DEVIL Q2/B1/B2/M3")` | `Err(NotYetImplemented("SsotMode::Grafeo checkpoint — requires wal feature + ArcSwap grafeo_db field (P4-DEVIL Q2/B1/B2/M3)".into()))` | Match arm currently `=> unimplemented!(...)`; new shape: `=> Err(NotYetImplemented(...))`. The `?` operator in the outer `pub async fn checkpoint` will propagate via `From` (already `GrafeoLoroError`); no new `From` needed. |
| 6 | `src/app.rs:982` | `hydrate()` `SsotMode::Grafeo` arm | `unimplemented!("P5: SsotMode::Grafeo hydrate — requires wal feature + ArcSwap grafeo_db field — see P4-DEVIL Q2/B1/B2")` | `Err(NotYetImplemented("SsotMode::Grafeo hydrate — requires wal feature + ArcSwap grafeo_db field (P4-DEVIL Q2/B1/B2)".into()))` | Same shape as row 5. |
| 7 | `src/app.rs:995` | `GrafeoLoroApp::broadcast_presence(...)` | `unimplemented!("broadcast_presence is Phase 5 scope")` | `Err(NotYetImplemented("GrafeoLoroApp::broadcast_presence — Phase 5 scope (WebSocket integration)".into()))` | `async` signature unchanged. The `let _ = payload;` line is dropped (the error doesn't need the payload). |
| 8 | `src/presence/socket.rs:20` | `PresenceManager::new(room_id)` | `unimplemented!()` | **IMPLEMENT real stub (rows 8a–8b below)** — store `room_id`, return `Self` | **No socket connection** (future scope). See A.4 for the new struct shape. |
| 9 | `src/presence/socket.rs:28` | `PresenceManager::broadcast(payload)` | `unimplemented!()` | `Err(NotYetImplemented(format!("PresenceManager::broadcast — Phase 5 scope (WebSocket wiring); room_id = {}", self.room_id)))` | Includes `self.room_id` in the message (justifies removing `#[allow(dead_code)]` per Gap D.3). `#[instrument(skip(self, payload), name = "presence_broadcast", level = "info")]` stays. |
| 10 | `src/presence/socket.rs:36` | `PresenceManager::parse_eph_envelope(bytes)` | `unimplemented!()` | **IMPLEMENT (Gap A.3 wire format)** | Pure function — no I/O. L2 implements per A.3 contract below. |
| 11 | `src/presence/socket.rs:44` | `PresenceManager::build_eph_envelope(payload)` | `unimplemented!()` | **IMPLEMENT (Gap A.3 wire format)** | Pure function — returns `Vec<u8>`. L2 implements per A.3. |

**Row 1a — Removal of `impl Default for AppConfig`**:

```rust
// DELETE these 4 lines from src/config.rs:
impl Default for AppConfig {
    fn default() -> Self {
        unimplemented!()
    }
}
```

The struct definition (rows 16–27) stays unchanged. No callers, no test breakage (verified via `rg -n 'AppConfig::default\|Default for AppConfig' src/ tests/ fuzz/` = ZERO call sites).

### A.3: `parse_eph_envelope` / `build_eph_envelope` wire format contract

**Architecture ref**: `docs/grafeo-loro.architecture.md` §12 (lines 478–506). Architecture specifies the wire layout as `[Magic Bytes "%EPH" (4B)] [Room ID VarString] [Msg Type u8] [Payload]` — the "VarString" length-prefix scheme is unspecified. This L1 contract **defines** the scheme as `u16 LE length + UTF-8 bytes` (room IDs in this codebase are short graph identifiers like `"graph_123"` — 9 bytes — so `u16` length is more than sufficient and keeps the envelope at 4 + 2 + N + 1 + M bytes; `u32` would be wasteful, `u8` would cap at 255B which is too tight for path-style room ids).

**`PresencePayload` serde check** (verified): `src/types/presence.rs:4` has `#[derive(Serialize, Deserialize, Debug, Clone)]` — matches architecture §12 spec exactly. No new derive needed.

**Wire format (single source of truth)**:

```text
+--------------------------------------------------------------------+
|  Offset | Size | Field        | Encoding                           |
+---------+------+--------------+------------------------------------+
|  0      | 4    | magic        | b"%EPH" (literal ASCII bytes)      |
|  4      | 2    | room_id_len  | u16 little-endian                  |
|  6      | N    | room_id      | UTF-8 bytes (N = room_id_len)      |
|  6+N    | 1    | msg_type     | u8 (0x01 = presence; future-proof) |
|  7+N    | M    | payload      | serde_json::to_vec(&PresencePayload)|
+--------------------------------------------------------------------+
```

**Total envelope size** = `7 + room_id.len() + payload_json_len`.

**`build_eph_envelope(payload: &PresencePayload) -> Vec<u8>`** contract:
1. `let mut buf = Vec::with_capacity(64);` (pre-allocate for typical payload size — anti-plenger #4 Performance).
2. `buf.extend_from_slice(b"%EPH");`
3. `let room_id = ???;` — **NOTE**: `build_eph_envelope` is a `pub fn(payload: &PresencePayload) -> Vec<u8>` (no `&self`); it has no access to `self.room_id`. Two options for L2 to choose:
   - **Option A (recommended — DRY)**: `build_eph_envelope` writes the magic + msg_type + payload only (NO room_id segment); the caller `broadcast` prepends `room_id_len + room_id`. The fn signature stays `pub fn build_eph_envelope(payload: &PresencePayload) -> Vec<u8>` and produces `[magic:4][msg_type:1][payload:M]`. Then `parse_eph_envelope` parses the same prefix. This violates the architecture §12 diagram (which shows room_id between magic and msg_type).
   - **Option B (matches arch §12 literally)**: change signature to `pub fn build_eph_envelope(room_id: &str, payload: &PresencePayload) -> Vec<u8>` — adds `room_id` parameter. `parse_eph_envelope` returns `(String /* room_id */, PresencePayload)` OR a new struct `EphEnvelope { room_id, payload }`. **Devil Q1**: which return shape — tuple or struct? L1 recommends struct (named fields — anti-plenger #5 High Cohesion).
   - **L1 ruling**: **Option B** with a new struct `EphEnvelope { pub room_id: String, pub payload: PresencePayload }`. Reasons: (1) matches architecture §12 diagram literally (no L1-level deviation from spec); (2) the caller `broadcast` already has `&self.room_id` to pass in; (3) the struct return type documents the envelope contents more clearly than a tuple.
   - **Updated signatures**:
     ```rust
     pub fn parse_eph_envelope(bytes: &[u8]) -> Result<EphEnvelope>
     pub fn build_eph_envelope(room_id: &str, payload: &PresencePayload) -> Vec<u8>
     ```
   - **New struct** (define in `src/presence/socket.rs`):
     ```rust
     /// Decoded `%EPH` envelope (architecture §12).
     #[derive(Debug, Clone, PartialEq)]
     pub struct EphEnvelope {
         pub room_id: String,
         pub payload: PresencePayload,
     }
     ```
4. `buf.extend_from_slice(b"%EPH");`
5. `let room_id_bytes = room_id.as_bytes();`
   `buf.extend_from_slice(&(room_id_bytes.len() as u16).to_le_bytes());`
   `buf.extend_from_slice(room_id_bytes);`
   **Defensive**: `room_id_bytes.len() > u16::MAX as usize` → return `Vec::new()` OR panic? `build_eph_envelope` returns `Vec<u8>` (no `Result`). L1 contract: **debug_assert + truncate** is wrong (silent corruption). Better: change return type to `Result<Vec<u8>>` so over-long room_id surfaces as `Err(NotYetImplemented(...))`? NO — that's misuse of `NotYetImplemented`. Better: introduce a new error variant `InvalidEnvelope(String)` for both parse and build failures. **L1 ruling**: change `build_eph_envelope` return type to `Result<Vec<u8>>` and add error variant `GrafeoLoroError::InvalidEnvelope(String)` with `#[error("invalid %EPH envelope: {0}")]`. `InvalidEnvelope` covers: build-side room_id too long, parse-side magic mismatch, parse-side truncated buffer, parse-side msg_type unknown, parse-side serde_json failure.
6. `buf.push(0x01u8);` (msg_type = presence)
7. `let json = serde_json::to_vec(payload).map_err(|e| InvalidEnvelope(format!("serde_json encode: {e}")))?;`
8. `buf.extend_from_slice(&json);`
9. `Ok(buf)`

**`parse_eph_envelope(bytes: &[u8]) -> Result<EphEnvelope>`** contract:
1. `if bytes.len() < 7 { return Err(InvalidEnvelope(format!("buffer too short: {} bytes", bytes.len()))); }` (minimum = magic + u16 + 0-byte room + msg_type + 0-byte payload; serde_json empty PresencePayload is non-empty, so this is a lower bound).
2. `if &bytes[0..4] != b"%EPH" { return Err(InvalidEnvelope(format!("bad magic: {:?}", &bytes[0..4]))); }`
3. `let room_id_len = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;`
4. `if bytes.len() < 6 + room_id_len + 1 { return Err(InvalidEnvelope(format!("room_id segment truncated: need {} bytes, have {}", 6 + room_id_len + 1, bytes.len()))); }`
5. `let room_id = std::str::from_utf8(&bytes[6..6+room_id_len]).map_err(|e| InvalidEnvelope(format!("room_id not UTF-8: {e}")))?.to_string();`
6. `let msg_type = bytes[6+room_id_len];`
   `if msg_type != 0x01 { return Err(InvalidEnvelope(format!("unsupported msg_type: 0x{:02x}", msg_type))); }` (only presence supported; future msg_types reserved)
7. `let payload_bytes = &bytes[7+room_id_len..];`
8. `let payload: PresencePayload = serde_json::from_slice(payload_bytes).map_err(|e| InvalidEnvelope(format!("serde_json decode: {e}")))?;`
9. `Ok(EphEnvelope { room_id, payload })`

**`#[instrument]` on parse/build**: the current `#[instrument(skip(bytes), name = "parse_eph_envelope", level = "debug")]` / `#[instrument(skip(payload), name = "build_eph_envelope", level = "debug")]` stay. For `build_eph_envelope`'s new `room_id` param, change to `#[instrument(skip(payload), fields(room_id = %room_id), name = "build_eph_envelope", level = "debug")]` (L2 picks — keeps payload out of the span attributes but surfaces room_id for correlation).

**Fuzz I15 already exercises this format** (per `p6-hunt.md` Anti-Pattern #8: I15 = "magic-prefix `assert_eq!` + 5 per-field `assert_eq!` round-trip checks + negative test (`bad_bytes` rejection)"). Once `parse_eph_envelope`/`build_eph_envelope` are real, I15's existing assertions become live instead of stubbed — but the L1 fuzz-target check at `fuzz/fuzz_targets/consistency.rs:728` already has the structure. **L2 action**: verify I15 still aligns with the new `EphEnvelope` return shape; update I15 if it currently calls the old `unimplemented!()` API.

### A.4: `PresenceManager::new` real stub

**File**: `src/presence/socket.rs`
**Current struct**:
```rust
pub struct PresenceManager {
    #[allow(dead_code, reason = "...")]  // ← removed in Gap D.3
    room_id: String,
    // WebSocket connection state
}
```

**New struct** (L1 contract):
```rust
pub struct PresenceManager {
    room_id: String,
    // WebSocket connection state — added in Phase 5+ when broadcast is implemented
}
```

**New `new`**:
```rust
pub fn new(room_id: String) -> Self {
    Self { room_id }
}
```

**Rationale**: this is a pure constructor — stores the room_id for later use by `broadcast` (which includes it in error messages and would include it in real WebSocket frames once implemented). No socket connection in this phase (the architecture §12 WebSocket channel is "future scope" per task instructions). The `#[allow(dead_code)]` on `room_id` is then removed (see Gap D.3).

---

## Gap B — I12 MVCC snapshot isolation invariant check contract

**File**: `fuzz/fuzz_targets/consistency.rs`
**Function**: `fn check_i12_mvcc_snapshot_isolation(db: &Arc<GrafeoDB>, maps: &Arc<BridgeMaps>)`

**Architecture ref**: `docs/grafeo-loro.architecture.md` §19 (lines 885–899). Grafeo uses MVCC with Snapshot Isolation (SI). Sessions acquire a snapshot at a specific epoch; writers commit as new epochs via Block-STM; active queries see a frozen, consistent snapshot.

**Grafeo API surface** (verified via `rg -n` against `grafeo-engine-0.5.42/src/session/mod.rs` + `transaction/`):
- `db.session() -> Session` — opens a session at the current epoch (default isolation = `SnapshotIsolation`).
- `db.session_with_cdc(bool) -> Session` — same but with CDC override.
- `session.begin_transaction() -> Result<()>` — pins the session to the current epoch.
- `session.set_viewing_epoch(epoch: EpochId) -> ()` — overrides all reads to see the DB at epoch E (time-travel API).
- `session.viewing_epoch() -> Option<EpochId>` — accessor.
- `session.clear_viewing_epoch() -> ()` — clears override.
- `session.get_node_property(node_id, prop) -> Option<Value>` — reads via current or overridden epoch.
- `prepared.commit() -> Result<EpochId>` — returns the new epoch after commit.
- `IsolationLevel::SnapshotIsolation` is the default; `IsolationLevel::Serializable` is opt-in via `begin_transaction_with_isolation`.

**Why I12 was deferred**: the original doc-comment (lines 665–671) claims I12 "requires `GrafeoLoroApp::query`" — this is **over-conservative**. `query` is a GQL convenience wrapper; the underlying grafeo API exposes everything I12 needs (`set_viewing_epoch` + `get_node_property`). The deferral reasoning was correct for Phase 6 (T1 was excluded → bodies still `unimplemented!()`), but in Phase 7 I12 can be implemented directly against the grafeo API without depending on `query`.

**I12 contract**:

```rust
/// I12 — MVCC snapshot isolation: a session pinned to epoch E via
/// `set_viewing_epoch(E)` MUST continue to observe the DB state as of E,
/// even after a concurrent writer commits a new epoch E'. Clearing the
/// override MUST then expose the new state.
///
/// Tests the "zero reader blocking + consistent snapshot" half of
/// architecture §19 directly via grafeo's `set_viewing_epoch` time-travel
/// API (NOT via `GrafeoLoroApp::query` — that remains
/// `Err(NotYetImplemented(...))` per Gap A.2 row 2).
fn check_i12_mvcc_snapshot_isolation(db: &Arc<GrafeoDB>, maps: &Arc<BridgeMaps>) {
    // 1. Write node N with property "v" = 1 → commit returns epoch E1.
    let mut w1 = db.session_with_cdc(false);
    w1.begin_transaction().expect("I12: begin_transaction (write 1) failed");
    let op = LoroOp::UpsertNode {
        loro_key: "V/i12-snap-test".to_string(),
        labels: vec!["Test".into()],
        properties: HashMap::from([(
            "v".to_string(),
            GraphValue::Integer(1),
        )]),
    };
    apply_loro_op(&w1, &op, maps).expect("I12: apply_loro_op (write 1) failed");
    let prepared1 = w1.prepare_commit().expect("I12: prepare_commit (write 1) failed");
    let e1 = prepared1.commit().expect("I12: commit (write 1) failed");

    // 2. Open a read session and pin it to epoch E1.
    let read_session = db.session();
    read_session.set_viewing_epoch(e1);

    // 3. Write node N's property "v" = 2 → commit returns epoch E2 (E2 > E1).
    let mut w2 = db.session_with_cdc(false);
    w2.begin_transaction().expect("I12: begin_transaction (write 2) failed");
    let node_id = *maps
        .node_id_map
        .read()
        .get("V/i12-snap-test")
        .expect("I12: BridgeMaps missing node after write 1");
    w2.set_node_property(node_id, "v", grafeo::Value::Integer(2))
        .expect("I12: set_node_property (write 2) failed");
    let prepared2 = w2.prepare_commit().expect("I12: prepare_commit (write 2) failed");
    let e2 = prepared2.commit().expect("I12: commit (write 2) failed");

    // Non-trivial assertion 1: epoch must advance on commit.
    assert!(
        e2.as_u64() > e1.as_u64(),
        "I12: epoch did not advance: E1={}, E2={}",
        e1.as_u64(),
        e2.as_u64()
    );

    // Non-trivial assertion 2: read_session pinned at E1 MUST see v=1,
    // NOT the new v=2.
    let v_at_e1 = read_session.get_node_property(node_id, "v");
    assert_eq!(
        v_at_e1,
        Some(grafeo::Value::Integer(1)),
        "I12: snapshot isolation violated — pinned at E1={}, saw v={:?} (expected 1)",
        e1.as_u64(),
        v_at_e1
    );

    // Non-trivial assertion 3: clearing the override MUST expose v=2.
    read_session.clear_viewing_epoch();
    let v_now = read_session.get_node_property(node_id, "v");
    assert_eq!(
        v_now,
        Some(grafeo::Value::Integer(2)),
        "I12: post-clear read saw v={:?} (expected 2 after epoch advanced to {})",
        v_now,
        e2.as_u64()
    );
}
```

**Call-site update** (currently `fuzz/fuzz_targets/consistency.rs:949–950`):
```rust
// OLD:
// I12: DEFERRED — see check_i12_mvcc_snapshot_isolation doc-comment.
check_i12_mvcc_snapshot_isolation();

// NEW:
// I12: MVCC snapshot isolation — pinned-epoch reads + post-clear reads.
check_i12_mvcc_snapshot_isolation(&db, &maps);
```

**Cadence**: every iteration (cheap; one extra write + three reads + two commits). Per `docs/phase-6/fuzz-invariants.md` cadence rules — this is a "per-iter, invariants about the live state" check.

**Devil Q2 (open question for Devil)**: the existing `check_i6_ryow` writes `V/i6-ryow-test`; `check_i10_vector_offload_bypass` writes `V/i10-vec-test`. The I12 contract uses `V/i12-snap-test`. Are there collision risks if I12 runs before I6/I10 on the same DB? Likely no (distinct keys), but Devil should rule. Alternative: use a fresh `GrafeoDB::new_in_memory()` inside I12 to fully isolate — but that loses the "live state" framing.

**L2/L3 scope**: L2 writes the function body + call-site update per this contract. L3 verifies the assertions fire correctly under fuzz (no false positives on the empty-ops seed, etc.).

---

## Gap C — Deferred child spans note update

**File**: `docs/phase-6/instrument-plan.md`
**Section**: "Span hierarchy (arch §23.2)" (lines 249–272).

### C.1: Updated note text

**OLD** (line 251):
> Deferred until Phase 6 T1 (unimplemented!() replacement) is done — child spans on panicking bodies are observationally pointless.

**NEW**:
> Deferred until the actual function implementations land in a future phase. Phase 7 (gap-closure) T1 replaces `unimplemented!()` panics with `Err(NotYetImplemented(...))` returns — it does NOT implement the real logic, so the sub-operations these child spans would instrument do not yet exist. Adding `info_span!("decompress_snapshot")` inside a body that just returns `Err(NotYetImplemented(...))` would be pure noise. Re-evaluate when `GrafeoLoroApp::query`/`hydrate`/`checkpoint`/`update_text`/`generate_embedding`/`broadcast_presence` get real implementations (Phase 4+ for `query`/`generate_embedding`, Phase 3+ for `update_text`, Phase 5+ for `broadcast_presence`, future-phase wal-feature work for the `SsotMode::Grafeo` arms).

### C.2: Also update the closing note (line 272)

**OLD**:
> Note: arch §23.2 row 4 (`user_mutation`) has no `create_user_mutation_span` factory in `telemetry/traces.rs`. L3 should either add it OR fold `local_grafeo_write`/`local_loro_commit` into the `update_text` `#[instrument]` span as inner `info_span!` calls. Most host methods currently have `unimplemented!()` bodies (see "Stubbed APIs" above), so L3 placement is deferred until T1 fills them. L2 adds `#[instrument]` on the parent pub fns (already in inventory); L3 adds inline `info_span!` calls for the children when bodies are written.

**NEW**:
> Note: arch §23.2 row 4 (`user_mutation`) has no `create_user_mutation_span` factory in `telemetry/traces.rs`. L3 should either add it OR fold `local_grafeo_write`/`local_loro_commit` into the `update_text` `#[instrument]` span as inner `info_span!` calls — but only once `update_text` has a real body (Phase 3+ scope; not Phase 7 — see updated deferral note above). L2 has already added `#[instrument]` on the parent pub fns (per inventory); L3 adds inline `info_span!` calls for the children when real bodies are written in a future phase.

### C.3: Rationale

This is **NOT a gap** — it's an accurate dependency statement. The original Phase 6 note said "deferred until T1" which was wrong: T1 (Phase 7) replaces panics with errors, not real implementations. The child spans (e.g., `decompress_snapshot` inside `hydrate`) wrap sub-operations of real implementations; with `Err(NotYetImplemented(...))` bodies, there are no sub-operations to span. The updated note makes the dependency chain explicit so future readers don't expect Phase 7 T1 to enable the child spans.

The "re-evaluate" trigger is concrete: when any of the 6 host methods (`query`, `hydrate`, `checkpoint`, `update_text`, `generate_embedding`, `broadcast_presence`) gets a real implementation, revisit the corresponding row(s) in the child-span table.

---

## Gap D — Structural `#[allow]` refactors

### D.1: `AppTelemetryConfig` struct (replaces `from_sync_engine_with_telemetry` 8-arg)

**File**: `src/app.rs` (struct + constructor); 1 call site at `src/app.rs:1436` (inside `GrafeoLoroAppBuilder::build`).

**Current signature** (`src/app.rs:252–261`):
```rust
pub fn from_sync_engine_with_telemetry(
    sync_engine: Arc<SyncEngine>,
    ssot_mode: SsotMode,
    storage: Option<Arc<dyn StorageBackend>>,
    compression: CompressionType,
    metrics: Option<Arc<MetricsRegistry>>,
    health: Option<Arc<HealthProbe>>,
    tracer: Option<SharedTracer>,
    worker_handles: Option<Vec<JoinHandle<()>>>,
) -> Self
```

`sync_engine` is **NOT** a config field — it's the engine being wrapped; keep it as the first positional arg. The other 7 args collapse into `AppTelemetryConfig`:

```rust
/// Configuration bundle for `GrafeoLoroApp::from_sync_engine_with_telemetry`.
/// Replaces the prior 8-arg constructor (anti-plenger #5 — High Cohesion).
#[derive(Debug, Clone)]
pub struct AppTelemetryConfig {
    pub ssot_mode: SsotMode,
    pub storage: Option<Arc<dyn StorageBackend>>,
    pub compression: CompressionType,
    pub metrics: Option<Arc<MetricsRegistry>>,
    pub health: Option<Arc<HealthProbe>>,
    pub tracer: Option<SharedTracer>,
    pub worker_handles: Option<Vec<JoinHandle<()>>>,
}
```

**New signature**:
```rust
pub fn from_sync_engine_with_telemetry(
    sync_engine: Arc<SyncEngine>,
    config: AppTelemetryConfig,
) -> Self
```

**Body**: identical field-init logic, just sourced from `config.*` instead of named params.

**Call-site migration** (`src/app.rs:1436`, inside `build()`):
```rust
// OLD (8 args):
Ok(GrafeoLoroApp::from_sync_engine_with_telemetry(
    Arc::new(engine),
    self.ssot_mode,
    self.storage.clone(),
    self.compression,
    metrics,
    health,
    tracer,
    Some(worker_handles),
))

// NEW (2 args):
Ok(GrafeoLoroApp::from_sync_engine_with_telemetry(
    Arc::new(engine),
    AppTelemetryConfig {
        ssot_mode: self.ssot_mode,
        storage: self.storage.clone(),
        compression: self.compression,
        metrics,
        health,
        tracer,
        worker_handles: Some(worker_handles),
    },
))
```

**Other call sites**: ZERO (verified via `rg -n 'from_sync_engine_with_telemetry' src/ tests/ fuzz/` = 1 match at the definition + 1 at the call site + doc-comment mentions only).

**Removal**: the `#[allow(clippy::too_many_arguments, reason = "...")]` at `src/app.rs:247` and the `// TODO: refactor to AppConfig struct in future phase` at `src/app.rs:251` are both removed.

**Devil Q3**: should `AppTelemetryConfig` live in `src/config.rs` (alongside `AppConfig`/`SsotMode`/`CompressionType`) or in `src/app.rs` (next to the constructor)? L1 recommends `src/config.rs` — it's the crate's config SSOT; colocating avoids a future drift where someone adds a new config struct in the wrong file.

### D.2: `BatcherConfig` struct (replaces `MutationBatcher::new` 9-arg)

**File**: `src/bridge/batcher.rs` (struct + constructor); 1 call site at `src/bridge/sync_engine.rs:297` (inside `SyncEngine::new_inner`); also `MutationBatcher::with_defaults` at `src/bridge/batcher.rs:137` delegates to `new` so it's affected.

**Current signature** (`src/bridge/batcher.rs:105–115`):
```rust
pub fn new(
    grafeo_db: Arc<GrafeoDB>,
    batch_size: usize,
    batch_ms: u64,
    bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
    maps: Arc<BridgeMaps>,
    shutdown_tx: broadcast::Sender<()>,
    metrics: Option<Arc<MetricsRegistry>>,
    tracer: Option<SharedTracer>,
    health: Option<Arc<HealthProbe>>,
) -> Self
```

`grafeo_db` is the **owned resource** being wrapped; keep it as the first positional arg. The other 8 args collapse into `BatcherConfig`:

```rust
/// Configuration bundle for `MutationBatcher::new`.
/// Replaces the prior 9-arg constructor (anti-plenger #5).
#[derive(Clone)]
pub struct BatcherConfig {
    pub batch_size: usize,
    pub batch_ms: u64,
    pub bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>,
    pub maps: Arc<BridgeMaps>,
    pub shutdown_tx: broadcast::Sender<()>,
    pub metrics: Option<Arc<MetricsRegistry>>,
    pub tracer: Option<SharedTracer>,
    pub health: Option<Arc<HealthProbe>>,
}
```

**New signatures**:
```rust
pub fn new(grafeo_db: Arc<GrafeoDB>, config: BatcherConfig) -> Self

pub fn with_defaults(grafeo_db: Arc<GrafeoDB>, config: BatcherConfig) -> Self
```

`with_defaults` previously took 7 args (grafeo_db + 6 channel/state args, hardcoded `DEFAULT_BATCH_SIZE`/`DEFAULT_BATCH_MS`). New shape: `with_defaults` overrides `config.batch_size`/`config.batch_ms` with the defaults and delegates to `new`:

```rust
pub fn with_defaults(grafeo_db: Arc<GrafeoDB>, mut config: BatcherConfig) -> Self {
    config.batch_size = DEFAULT_BATCH_SIZE;
    config.batch_ms = DEFAULT_BATCH_MS;
    Self::new(grafeo_db, config)
}
```

**Call-site migration** (`src/bridge/sync_engine.rs:297–307`):
```rust
// OLD (9 args):
let batcher = Arc::new(MutationBatcher::new(
    grafeo_db.clone(),
    batch_size,
    batch_ms,
    bridge_origin_epochs.clone(),
    maps.clone(),
    shutdown_tx.clone(),
    metrics.clone(),
    tracer.clone(),
    health.clone(),
));

// NEW (2 args):
let batcher = Arc::new(MutationBatcher::new(
    grafeo_db.clone(),
    BatcherConfig {
        batch_size,
        batch_ms,
        bridge_origin_epochs: bridge_origin_epochs.clone(),
        maps: maps.clone(),
        shutdown_tx: shutdown_tx.clone(),
        metrics: metrics.clone(),
        tracer: tracer.clone(),
        health: health.clone(),
    },
));
```

**Other call sites**: ZERO outside `src/bridge/sync_engine.rs:297` (verified via `rg -n 'MutationBatcher::new\b' src/ tests/ fuzz/`). `MutationBatcher::with_defaults` has ZERO call sites (verified — declared but unused in production; the `with_defaults` API is reserved for future test fixtures per its doc-comment, but currently no test calls it). Devil Q4: should `with_defaults` be removed (anti-plenger #11 — Deletion over addition)? L1 recommends **KEEP** — it's a documented API alternative, removal would be premature; but if Devil rules "remove", the deletion is one fn + doc-comment, no other changes.

**Removal**: the `#[allow(clippy::too_many_arguments, reason = "...")]` at `src/bridge/batcher.rs:100` and the `// TODO: refactor to BatcherConfig struct in future phase` at `src/bridge/batcher.rs:104` are both removed.

**Devil Q3 (same as D.1)**: where does `BatcherConfig` live? L1 recommends `src/bridge/batcher.rs` (next to `MutationBatcher`) — `BatcherConfig` is bridge-internal, not a public-facing config struct like `AppConfig`. Different SSOT scope.

### D.3: `room_id` `dead_code` allow removal

**File**: `src/presence/socket.rs:8–11`

**Evidence the field becomes read**: after Gap A.2 row 8 (`new` stores `room_id`) + row 9 (`broadcast` uses `self.room_id` in the `NotYetImplemented` error message), the field is **read** at `src/presence/socket.rs:28` (inside `broadcast`).

**Action**:
1. Remove the `#[allow(dead_code, reason = "Phase 6 T1 excluded by user; field needed once broadcast_presence is implemented")]` attribute (lines 8–11).
2. Remove the `// TODO: Phase 6 T1 — wire room_id into broadcast() once body is implemented` comment (line 7).

The `room_id: String,` field definition (line 12) stays.

### D.4: `async_yields_async` reason update (permanent, not a TODO)

**Files**: `src/bridge/sync_engine.rs:459`, `:553`, `:661` (3 occurrences on `spawn_inbound_worker`/`spawn_outbound_worker`/`spawn_cdc_poller`).

**OLD reason** (all 3 sites):
```rust
reason = "spawn_*_worker fns return tokio::task::JoinHandle by design — caller awaits the handle, not the spawn call"
```

This reason is **already correct** — but the surrounding context (P5-L2 doc-comments + the P5-L1/L2/L3 history) frames these as "design choices wired in P5-L2". The reason text itself is fine. The Hunter flagged the "deferred" language elsewhere (e.g. the `#[allow]` reason strings on rows 1–2 of D.1/D.2 used "deferred to future phase"); `async_yields_async` does NOT use "deferred" language.

**L1 ruling**: **NO TEXT CHANGE needed** on the `async_yields_async` reasons. They are already permanent-design phrasing, not TODOs. The Hunter's "update reason to remove 'deferred' language" note applies only if such language existed here — it doesn't. **Action: confirm via `rg -n 'async_yields_async' src/bridge/sync_engine.rs` that no `deferred`/`TODO`/`future phase` language appears in the reason strings (or in the immediately preceding comments).** If found, remove; if not, no edit.

Verification to perform at L2 time:
```
rg -n -B2 -A4 'async_yields_async' src/bridge/sync_engine.rs
```
Look for any adjacent `// TODO` or `deferred` comment. The current `reason = "spawn_*_worker fns return ..."` is already permanent.

---

## Gap E — Hunter nit resolutions

### E.1: `EncFuzzValue`/`EncFuzzOp` consolidation

**Files**: `fuzz/fuzz_targets/gen_corpus.rs:179–218` (mirror types) + `fuzz/fuzz_targets/consistency.rs:85–115` (canonical types).

**Investigation**:
- `FuzzOp` + `FuzzValue` are defined in `fuzz/fuzz_targets/consistency.rs:85–115` with `#[derive(Arbitrary, Debug, Clone)]`.
- `EncFuzzOp` + `EncFuzzValue` are defined in `fuzz/fuzz_targets/gen_corpus.rs:179–218` with NO derives (writer-side only).
- The `gen_corpus.rs:177–178` comment: "Enum mirrors for the seed corpus (avoids depending on the `arbitrary` derive at gen time — keeps the generator self-contained)."
- Both binaries are in the same crate (`grafeo-loro-fuzz`); the crate's `lib.rs` (`fuzz/fuzz_targets/lib.rs:1`) is currently empty (`//! Fuzz target library root.`).
- `arbitrary` is already in `Cargo.toml` deps — so "self-contained" is weak rationale (the dep is loaded regardless).

**Decision**: **CONSOLIDATE** (anti-plenger #5 Bloat — DRY Violations).

**Plan**:
1. Move `FuzzOp` + `FuzzValue` (with `#[derive(Arbitrary, Debug, Clone)]` + `impl FuzzValue { fn to_graph_value(...) }` + `From<FuzzOp> for LoroOp`) from `fuzz/fuzz_targets/consistency.rs:85–155` into `fuzz/fuzz_targets/lib.rs`.
2. Make them `pub` in `lib.rs` so both binaries can `use` them.
3. In `fuzz/fuzz_targets/consistency.rs`: replace the local definitions with `use grafeo_loro_fuzz::{FuzzOp, FuzzValue};` (the crate name from `Cargo.toml`). Keep `FuzzInput` (struct with seed/ops/peer_count/bail_after_ops) in `consistency.rs` — it's fuzz-target-specific.
4. In `fuzz/fuzz_targets/gen_corpus.rs`: replace `EncFuzzOp` + `EncFuzzValue` with `use grafeo_loro_fuzz::{FuzzOp, FuzzValue};`. Update `enc_fuzz_op(buf, op: &FuzzOp)` + `enc_fuzz_value(buf, v: &FuzzValue)` signatures. Delete the `EncFuzzOp`/`EncFuzzValue` enums + the `#[allow(dead_code, ...)]` attribute.
5. Verify `cargo check -p grafeo-loro-fuzz` + `cargo +nightly check -p grafeo-loro-fuzz` + `cargo run --bin gen_corpus --manifest-path fuzz/Cargo.toml` (idempotent — `sha256sum` of corpus files unchanged).

**Risk**: the `Arbitrary` derive requires the type to be in scope of the derive macro at definition site — moving to `lib.rs` doesn't break this. The `From<FuzzOp> for LoroOp` impl lives next to the type (in `lib.rs`); both binaries `use` it via the re-export.

**Devil Q5**: should the `From<FuzzOp> for LoroOp` impl also move to `lib.rs`? Yes — it's tightly coupled to `FuzzOp` (it pattern-matches every variant). Keeping it in `consistency.rs` while moving `FuzzOp` to `lib.rs` would split the type's logic across two files (anti-plenger #5 — High Cohesion violation).

**Hunter status after fix**: NIT #1 (`EncFuzzValue`/`EncFuzzOp` mirror types) — **RESOLVED** via consolidation.

---

## Gap F — Stale doc-comment corrections

### F.1: `src/telemetry/health.rs:5`

**OLD** (line 5):
```
//! All method bodies are `unimplemented!()`. The struct shape mirrors
```

**NEW**:
```
//! All method bodies are fully implemented (P5-L3). The struct shape mirrors
```

### F.2: `src/telemetry/metrics.rs:5`

**OLD** (line 5):
```
//! All method bodies are `unimplemented!()` — L2 wires the OpenTelemetry SDK
```

**NEW**:
```
//! All method bodies are fully implemented (P5-L3) — the OpenTelemetry SDK
```

The original sentence continues "calls, L3 fills the algorithm bodies." — this clause is now also stale. L2 should rewrite the full sentence:

**Full new sentence (lines 5–7)**:
```
//! All method bodies are fully implemented (P5-L3) — the OpenTelemetry SDK
//! calls land in `MetricsRegistry::init` + `record_batch_flush` + `record_hydration`.
//! The struct fields are the five instruments specified in architecture §23.1:
```

### F.3: `src/app.rs:62`

**OLD** (lines 61–63):
```
/// All methods other than [`Self::create_vertex`] + [`Self::maps`] remain
/// `unimplemented!()` (Phase 3-5 scope). See each method's doc-comment for
/// the owning phase.
```

**NEW**:
```
/// Most methods are implemented; 6 remaining `unimplemented!()` bodies are
/// replaced with `Err(NotYetImplemented(...))` returns in Phase 7 (see Gap A
/// in `docs/phase-7/gap-closure-l1-plan.md`). See each method's doc-comment
/// for the owning phase.
```

**Devil Q6**: the original sentence says "All methods other than `create_vertex` + `maps`" — that was always an overstatement (Phase 4/5 implemented `hydrate`/`checkpoint`/`create_vertex`/`maps`/accessors). The new wording ("Most methods are implemented") is accurate. Devil should rule on whether the count "6" is correct: per Gap A.2, the 6 pub-fn `unimplemented!()`s being replaced are `query` (row 2), `update_text` (row 3), `generate_embedding` (row 4), `broadcast_presence` (row 7), `PresenceManager::broadcast` (row 9), `PresenceManager::parse_eph_envelope` (row 10, but this is being implemented — not just error-wrapped), `PresenceManager::build_eph_envelope` (row 11, also being implemented), plus `PresenceManager::new` (row 8, also being implemented). After Gap A: rows 2/3/4/7 return `Err(NotYetImplemented(...))`; rows 8/10/11 are real implementations; row 9 returns `Err(NotYetImplemented(...))`; rows 5/6 (`SsotMode::Grafeo` arms inside already-implemented `checkpoint`/`hydrate`) return `Err(NotYetImplemented(...))` but are match arms, not standalone pub fns. The "6 remaining" refers to: `query`, `update_text`, `generate_embedding`, `broadcast_presence`, `PresenceManager::broadcast`, plus the 2 `SsotMode::Grafeo` arms (rows 5+6 count as 2 since they're in 2 different pub fns). Devil should verify count.

---

## Gap G — Stale NOTE comment removal

**Files + lines** (verified via `rg -n '// NOTE: body unimplemented!()' src/`):

| # | File:Line | Associated function | Action |
|---|-----------|---------------------|--------|
| 1 | `src/app.rs:358` | `GrafeoLoroApp::query` | REMOVE the NOTE comment (line 358). Keep the `#[instrument(skip(self, gql), level = "info")]` attribute (line 359). |
| 2 | `src/app.rs:366` | `GrafeoLoroApp::update_text` | REMOVE line 366. Keep `#[instrument(skip(self, text), level = "info")]` (line 367). |
| 3 | `src/app.rs:380` | `GrafeoLoroApp::generate_embedding` | REMOVE line 380. Keep `#[instrument(skip(self), level = "info")]` (line 381). |
| 4 | `src/app.rs:991` | `GrafeoLoroApp::broadcast_presence` | REMOVE line 991. Keep `#[instrument(skip(self, payload), level = "info")]` (line 992). |
| 5 | `src/presence/socket.rs:24` | `PresenceManager::broadcast` | REMOVE line 24. Keep `#[instrument(skip(self, payload), name = "presence_broadcast", level = "info")]` (line 25). |
| 6 | `src/presence/socket.rs:32` | `PresenceManager::parse_eph_envelope` | REMOVE line 32. Keep `#[instrument(skip(bytes), name = "parse_eph_envelope", level = "debug")]` (line 33). |
| 7 | `src/presence/socket.rs:40` | `PresenceManager::build_eph_envelope` | REMOVE line 40. Keep `#[instrument(skip(payload), name = "build_eph_envelope", level = "debug")]` (line 41). Update to `skip(payload), fields(room_id = %room_id)` per Gap A.3 contract if Devil Q1 picks Option B (recommended). |

**Total**: 7 NOTE comments removed (the task brief said "5" — actual count via `rg` is 7; the task brief undercounted). All 7 follow the same pattern: the NOTE was a Phase 6 marker for "T1 excluded per user; span fires then panics" — now that T1 is being un-excluded in Phase 7, the NOTE is stale.

**No new comments added** — the `#[instrument]` attributes already document that the fn is traced; the body's `Err(NotYetImplemented(...))` makes the deferral explicit at the call site. Anti-plenger #13 (oneline doc only) — no extra commentary.

---

## Summary

| Metric | Count |
|--------|-------|
| New error variants | 2 (`NotYetImplemented(String)` + `InvalidEnvelope(String)`) |
| New structs | 3 (`AppTelemetryConfig`, `BatcherConfig`, `EphEnvelope`) |
| New `pub fn`s | 0 (all changes are to existing pub fns) |
| Functions modified (unimplemented!() replacement) | 11 (rows 1–11 in Gap A.2; row 1 is impl-removal, rows 2–9 are Err-wraps, rows 8/10/11 are real implementations) |
| Functions modified (config struct refactor) | 2 (`from_sync_engine_with_telemetry`, `MutationBatcher::new` + `with_defaults`) |
| Fuzz target changes | 2 (`check_i12_mvcc_snapshot_isolation` body + call-site; `EncFuzzValue`/`EncFuzzOp` consolidation) |
| Doc-comment fixes | 3 (stale `unimplemented!()` claims in `health.rs:5`, `metrics.rs:5`, `app.rs:62`) |
| NOTE comment removals | 7 (Gap G) |
| Deferred-spans note update | 1 (`docs/phase-6/instrument-plan.md` lines 251 + 272) |
| `#[allow]` attributes removed | 3 (`app.rs:247` too_many_arguments; `batcher.rs:100` too_many_arguments; `presence/socket.rs:8` dead_code) |
| `#[allow]` attributes kept (verified permanent) | 3 (`sync_engine.rs:459/553/661` async_yields_async) |
| `// TODO` comments removed | 2 (`app.rs:251`, `batcher.rs:104`) |
| Hunter nits resolved | 3/3 (EncFuzz consolidation E.1; I12 fill B; deferred-spans note C) |
| Phase 6 known gaps closed | 4/4 (T1 unimplemented!(); I12; child spans note; structural #[allow] TODOs) |

## Open Questions for Devil's Advocate

| # | Question | L1 Recommendation |
|---|----------|-------------------|
| Q1 | `build_eph_envelope` signature: Option A (no room_id arg, caller prepends) vs Option B (room_id arg, returns full envelope)? | **Option B** — matches architecture §12 diagram literally + caller already has room_id. |
| Q2 | I12 collision risk with I6/I10 on shared DB (`V/i12-snap-test` vs `V/i6-ryow-test` vs `V/i10-vec-test`)? | Accept distinct keys (no collision); alternative is fresh `GrafeoDB::new_in_memory()` inside I12 (loses "live state" framing). |
| Q3 | `AppTelemetryConfig` location: `src/config.rs` vs `src/app.rs`? | `src/config.rs` (crate config SSOT). `BatcherConfig` → `src/bridge/batcher.rs` (bridge-internal). |
| Q4 | `MutationBatcher::with_defaults` has 0 call sites — remove (anti-plenger #11) or keep (documented API)? | **KEEP** — reserved for future test fixtures; removal is premature. |
| Q5 | `From<FuzzOp> for LoroOp` impl location: `consistency.rs` vs `lib.rs` (with `FuzzOp`)? | **`lib.rs`** — tightly coupled to `FuzzOp`, keep co-located (anti-plenger #5). |
| Q6 | Phase 7 `app.rs:62` doc-comment count "6 remaining" — correct? | Devil to verify: count `unimplemented!()`/`Err(NotYetImplemented(...))` post-Phase-7 in `src/app.rs` + `src/presence/socket.rs` + `SsotMode::Grafeo` arms. |

## Process

1. **L2 (Fixer)**: implement per this plan. Order:
   - Gap A.1 (add `NotYetImplemented` + `InvalidEnvelope` variants to `src/error.rs`).
   - Gap A.4 + Gap D.3 (PresenceManager struct + new + remove dead_code allow).
   - Gap A.3 (EphEnvelope struct + parse_eph_envelope + build_eph_envelope real impl).
   - Gap A.2 rows 2–7 (replace 6 `unimplemented!()` with `Err(NotYetImplemented(...))`).
   - Gap A.2 row 1 (remove `impl Default for AppConfig`).
   - Gap G (remove 7 NOTE comments).
   - Gap F (rewrite 3 stale doc-comments).
   - Gap D.1 + D.2 (refactor `from_sync_engine_with_telemetry` + `MutationBatcher::new` to config structs).
   - Gap E.1 (consolidate EncFuzz → shared lib).
   - Gap B (implement I12 body + update call site).
   - Gap C (update deferred-spans note).
2. **L3 (Meat)**: verify all gates: `cargo check --all`, `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all` (expect 82+ pass; I12 adds no new test, just a live fuzz body). Fuzz: `cd fuzz && cargo check`, `cd fuzz && cargo clippy --all-targets -- -D warnings`, `cd fuzz && cargo run --bin gen_corpus` + `sha256sum` corpus (must match committed files).
3. **Devil (advocate)**: rule on Q1–Q6.
4. **Hunter (plenger)**: re-scan all 8 anti-patterns.
