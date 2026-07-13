# P5 Plenger-Traits Hunt — Round 2 (Verification)

**Date**: 2026-07-06
**Hunter**: P5-HUNT-2 agent
**Audit range**: `17f9992..HEAD` (P5-L2-2 fixes only — commit `6cc144f`)
**Previous HUNT verdict**: L2_REENTRY (2 MAJOR + 1 MINOR + 2 NIT; 6/8 plenger categories clean)
**Verdict**: **PROCEED_TO_PUSH** — all 2 MAJOR + 1 MINOR verified fixed; 0 new plenger-traits introduced; 82/82 tests pass.

---

## 1. MAJOR 1 Fix Verification (double-count `inbound_events`)

**Finding (HUNT-1)**: `src/bridge/batcher.rs:317` bumped `m.inbound_events.add(op_count as u64, &[])` per-flush — but the same counter is also bumped per-op at `src/bridge/sync_engine.rs:477` (Devil Q12 per-op forward boundary). A 5-op batch would report 10 events (2× actual).

**Fix applied (P5-L2-2)**: Removed the per-flush `m.inbound_events.add(...)` line at `batcher.rs:317`, replaced with a 4-line explanatory comment citing Devil Q12 + HUNT-1 MAJOR 1.

**Verification evidence**:
- `rg -n "inbound_events\.add\(op_count" src/` → **0 matches** ✅
- `rg -n "inbound_events\.add" src/` → **1 match** (`src/bridge/sync_engine.rs:486` — the per-op forward boundary, Devil Q12 compliant) ✅
- `batcher.rs:316-321` confirmed: 4-line comment present, no counter call:
  ```rust
  if let Some(m) = &self.metrics {
      m.record_batch_flush(elapsed_ms, op_count as u64);
      // `inbound_events` is bumped per-op at the forward boundary
      // in `sync_engine.rs` (Devil Q12 — per-op forward, NOT
      // per-flush aggregate). Bumping it here too would double-
      // count: a 5-op batch would report 10 (P5-HUNT-1 MAJOR 1).
  }
  ```

**Verdict**: ✅ FIXED.

---

## 2. MAJOR 2 Fix Verification (missing arch §23.1 row 1/2 labels)

**Finding (HUNT-1)**: Both `inbound_events` (sync_engine.rs:477) and `outbound_events` (sync_engine.rs:569) were bumped with `&[]` (empty attribute set). Architecture §23.1 row 1/2 requires `origin` + `event_type` labels so operators can slice events by source + kind.

**Fix applied (P5-L2-2)**:
- Inbound (`sync_engine.rs:479-492`): 5-arm `match &op` on `LoroOp` → `vertex|edge|tree`; adds `KeyValue::new("origin", "loro")` + `KeyValue::new("event_type", event_type)`.
- Outbound (`sync_engine.rs:587-602`): 4-arm `match &msg.payload.entity_id` on `grafeo::cdc::EntityId` → `vertex|edge|triple|other`; adds `KeyValue::new("origin", "grafeo")` + `KeyValue::new("event_type", event_type)`. `_ => "other"` wildcard is justified because `EntityId` is `#[non_exhaustive]`.

**Verification evidence**:

### 2a. No empty attribute sets remain
- `rg -n "&\[\]" src/bridge/sync_engine.rs` → **0 matches** ✅

### 2b. `LoroOp` variant table (verified at `src/types/events.rs:14` — local enum)
| `LoroOp` variant (events.rs:14-49) | Match arm | `event_type` label | Semantic check |
|---|---|---|---|
| `UpsertNode { loro_key, labels, properties }` | `LoroOp::UpsertNode { .. }` | `"vertex"` | ✅ node = vertex |
| `DeleteNode { loro_key }` | `LoroOp::DeleteNode { .. }` | `"vertex"` | ✅ node = vertex |
| `UpsertEdge { src_key, dst_key, label, properties }` | `LoroOp::UpsertEdge { .. }` | `"edge"` | ✅ edge = edge |
| `DeleteEdge { src_key, dst_key, label }` | `LoroOp::DeleteEdge { .. }` | `"edge"` | ✅ edge = edge |
| `TreeMove { node_key, old_parent_key, new_parent_key }` | `LoroOp::TreeMove { .. }` | `"tree"` | ✅ tree move = tree |

**Match exhaustiveness**: 5 arms cover all 5 variants. `LoroOp` is locally defined and NOT `#[non_exhaustive]`, so no wildcard is needed (and none present — compiler would have caught any missing variant as `E0004`). ✅

### 2c. `grafeo::cdc::EntityId` variant table (verified at `grafeo-engine-0.5.42/src/cdc.rs:148`)
| `EntityId` variant (cdc.rs:148-155) | Match arm | `event_type` label | Semantic check |
|---|---|---|---|
| `Node(NodeId)` | `grafeo::cdc::EntityId::Node(_)` | `"vertex"` | ✅ node = vertex |
| `Edge(EdgeId)` | `grafeo::cdc::EntityId::Edge(_)` | `"edge"` | ✅ edge = edge |
| `Triple(u64)` | `grafeo::cdc::EntityId::Triple(_)` | `"triple"` | ✅ triple = triple |
| (future variants) | `_` | `"other"` | ✅ justified by `#[non_exhaustive]` at cdc.rs:147 |

**`#[non_exhaustive]` justification**: Confirmed by reading `grafeo-engine-0.5.42/src/cdc.rs:147` (`#[non_exhaustive]` directly above `pub enum EntityId {` at line 148). Without the `_ => "other"` wildcard, the compiler emits `E0004: non-exhaustive patterns` — P5-L2-2 worklog step 9 documents hitting + resolving exactly this. ✅

### 2d. Origin label
- Inbound: `KeyValue::new("origin", "loro")` — correct (Loro→Grafeo forward direction). ✅
- Outbound: `KeyValue::new("origin", "grafeo")` — correct (Grafeo→Loro CDC commit direction). ✅

### 2e. `KeyValue` import
- `use opentelemetry::KeyValue;` confirmed at `src/bridge/sync_engine.rs:57` (pre-existing — no new import added). ✅

### 2f. Anti-plenger #8 (polymorphism over conditionals)
- Both label derivations are pure `match` expressions — no `if/else` chains, no `matches!` + lookup-table indirection. ✅

**Verdict**: ✅ FIXED. Both LoroOp + EntityId variant tables verified against source; semantic mapping (vertex/edge/tree/triple/other) matches arch §23.1 row 1/2; `_ => "other"` wildcard is justified by `#[non_exhaustive]`; origin labels (loro/grafeo) correct.

---

## 3. MINOR 1 Fix Verification (parking_lot doc misrepresentation)

**Finding (HUNT-1)**: `src/telemetry/health.rs:20` module doc-comment + `:161-162` inline comment claimed the `try_read()` probe detects "LoroDoc lock poison". This is wrong — `parking_lot::RwLock` has NO poisoning (unlike `std::sync::RwLock`); `try_read()` returns `None` only when a writer currently holds the lock.

**Fix applied (P5-L2-2)**: Updated both comments to correctly describe parking_lot semantics. Doc-only, no behavioral change.

**Verification evidence**:

### 3a. Module doc-comment (`health.rs:20-23`)
```rust
//! 1. **Loro doc read accessibility** — `self.doc.try_read().is_some()`. `parking_lot::RwLock`
//!    has NO poisoning (unlike `std::sync::RwLock`); `try_read()` returns `None` only when a
//!    writer currently holds the lock, so this probe verifies the lock is not held by a writer
//!    (P5-HUNT-1 MINOR 1 — comment previously mis-described this as poison detection).
```
- Title renamed: "LoroDoc lock poison" → "Loro doc read accessibility". ✅
- Correctly states parking_lot has NO poisoning. ✅
- Correctly describes `try_read()` returning `None` only when writer holds the lock. ✅

### 3b. Inline comment (`health.rs:164-169`)
```rust
// 1. Loro doc read accessibility: `try_read()` returns `Option`
//    (parking_lot API) — `Some` if the lock is free or read-locked,
//    `None` if a writer currently holds it. `parking_lot::RwLock` has
//    NO poisoning (unlike `std::sync::RwLock`), so this probe verifies
//    the lock is not held by a writer — NOT poison detection
//    (P5-HUNT-1 MINOR 1).
```
- Correctly describes `Some`/`None` semantics for `parking_lot::RwLock::try_read()`. ✅
- Correctly states NO poisoning. ✅

### 3c. Behavioral unchanged
- The `check()` method body at `health.rs:160+` was not modified — `let loro_ok = self.doc.try_read().is_some();` (the actual probe) is unchanged. Pure doc fix. ✅

**Verdict**: ✅ FIXED. Doc-only, no behavioral change, comments now accurately describe parking_lot semantics.

---

## 4. New Plenger-Traits Introduced by Fixes?

Light sweeps on the diff (`git diff 17f9992..HEAD -- src/`):

### 4a. Band-aid sweep
`git diff 17f9992..HEAD -- src/ | grep "^+" | grep -E "unwrap\(\)|expect\(|panic!\(|\.ok\(\)|unwrap_or_default"` → **0 matches** ✅

### 4b. Conditional-over-polymorphism sweep
`git diff 17f9992..HEAD -- src/ | grep "^+" | grep -E "if let|if .*==|else"` → **0 new `if/else` chains** (the only `if let Some(m)` and `if let Some(h)` patterns were pre-existing in the surrounding code; only their bodies changed). ✅

### 4c. Hallucination sweep (LoroOp + EntityId variants)
- `LoroOp` is locally defined at `src/types/events.rs:14` (NOT from lorosurgeon as the workflow spec hypothesized — HUNT-1 worklog already noted this). All 5 variants verified line-by-line. ✅
- `EntityId` verified at `grafeo-engine-0.5.42/src/cdc.rs:148` with `#[non_exhaustive]` at line 147. All 3 named variants + wildcard arm justified. ✅
- **0 hallucinated variants.**

### 4d. Bloat sweep
- Inbound label derivation: 7 lines (`match` + 3 arms + `add` call) — under the 15-line threshold. ✅
- Outbound label derivation: 11 lines (`match` + 4 arms including `_` + comment + `add` call) — under the 15-line threshold. ✅
- Both fits inside the existing `if let Some(m) = metrics.as_ref() { ... }` block — no control-flow restructure. ✅

### 4e. Context Blindness sweep
- Both `match` expressions are pure value derivations (returning `&'static str`) inside a synchronous block — no `.await` crossed, no `RwLock` guard held. The `metrics.as_ref()` is an `Arc` clone, not a lock acquisition. ✅
- The `batch_tx.send(op).await` at sync_engine.rs:494 happens AFTER the `inbound_events.add(...)` call (line 486-492) — no guard leak. ✅
- The outbound worker's `apply_change_event_to_loro` + `doc.commit()` at lines 568-576 complete BEFORE the `outbound_events.add(...)` call at 587-602 — no guard leak. ✅

### 4f. Goodhart's Law sweep
- Labels are semantically meaningful (vertex/edge/tree/triple/other + origin loro/grafeo), not hardcoded to pass the HUNT-1 grep check. Mapping verified against arch §23.1 row 1/2 in §2b/§2c above. ✅

### 4g. Backward-compat slaves sweep
- N/A — these are new additions (label derivation), not refactors of legacy code. No legacy rot preserved. ✅

**Verdict**: **0 new plenger-traits introduced** by P5-L2-2 fixes.

---

## 5. Compile + Test Verification

- `cargo check --all-targets`: ✅ **0 errors, 1 pre-existing warning** (`presence::socket::room_id` field never read — unchanged from P5-L3 baseline, present since P5-L1).
- `cargo test --all`: ✅ **82 passed / 0 failed / 2 ignored** (6 unit lib + 5 integration + 71 unit tests + 0 doctests; the 2 ignored are pre-existing `vector_embedding` manual-smoke tests gated behind `--ignored`).
- `rg -n "inbound_events\.add\(op_count" src/` returns: **0 matches** ✅
- `rg -n "&\[\]" src/bridge/sync_engine.rs | grep -E "inbound|outbound"` returns: **0 matches** (in fact `rg "&\[\]" src/bridge/sync_engine.rs` returns 0 matches overall — no empty attribute sets remain anywhere in the file) ✅

---

## 6. Final Verdict

**PROCEED_TO_PUSH**.

**Rationale**: All 2 MAJOR + 1 MINOR findings from P5-HUNT-1 are verified fixed in commit `6cc144f`:
- MAJOR 1 (double-count): mechanically removed + 4-line comment; sole `inbound_events.add(...)` now lives at the per-op forward boundary per Devil Q12.
- MAJOR 2 (missing labels): both inbound + outbound counters now carry `origin` + `event_type` labels derived via pure `match` expressions; LoroOp + EntityId variant tables verified against source (0 hallucinations); `_ => "other"` wildcard on `EntityId` is justified by `#[non_exhaustive]`; semantic mapping matches arch §23.1 row 1/2.
- MINOR 1 (parking_lot doc): doc-only fix; comments now accurately describe `parking_lot::RwLock::try_read()` semantics; behavioral probe unchanged.

0 new plenger-traits introduced by the fixes (no new band-aids, no new `if/else` chains, no bloat >15 lines, no context blindness, no Goodhart hardcoding). 82/82 tests pass, 0 errors, 1 pre-existing warning. Phase 5 is ready to push.

The 2 NITs from HUNT-1 (OTel span hierarchy naming + meter provider global lifecycle) are correctly deferred to Phase 6 hardening per the HUNT-1 worklog — they are <5-line cosmetic items not worth a round-trip.

---

## 7. Phase 5 Loop Close Summary

- **Total commits** (P5-L1 → P5-L2 → P5-L3 → P5-HUNT-1 → P5-L2-2 → P5-HUNT-2): 6 substantive + 1 critique + this verification = 7 commits on `phase-5` branch (subject to final commit count verification by orchestrator).
- **Total tests**: 70 baseline (P5-L1) → **82 current** (+12 new in `tests/unit/telemetry.rs`).
- **cargo check**: 0 errors, 1 pre-existing warning (`presence::socket::room_id`).
- **Plenger categories clean**: 8/8 (HUNT-1 found 2 MAJOR + 1 MINOR in 2 categories — Goodhart §23.1 label compliance + Documentation accuracy; both now resolved. All 8 categories: Tautology/Hallucination/Bloat/Goodhart/Context-Blindness/Defensive/Backward-compat-slaves/Conditional-over-polymorphism now clean).
- **Telemetry surface**: OpenTelemetry counters (`inbound_events`, `outbound_events`, `batch_flush_duration_ms`, `inbound_event_count` test counter) + health probe (3-component: loro doc accessibility / grafeo dummy query / sync staleness) + traces (span hierarchy) all wired and tested.
- **Ready for Phase 6 (Hardening & Docs)**: deferred items — (a) NIT 1: OTel meter provider global lifecycle; (b) NIT 2: actual OTel parent-child span linking (currently logical-only, Jaeger reconstructs by name); (c) any architecture doc clarifications on `event_type="other"` fallback semantics for future `EntityId` variants.
