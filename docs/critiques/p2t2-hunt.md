# P2T2 Hunter Critique

**Task ID**: P2T2-HUNT
**Agent**: Plenger Hunter
**Branch**: `p2-tree-move`
**Target**: Cumulative P2T2-L1 + P2T2-L2 + P2T2-L3 output for Phase 2 Task 2 (`schema::tree::sync_tree_move_to_grafeo` + `would_create_cycle_precheck` + 8 test bodies)
**Critique artifact**: this file
**Method**: read-only verification against `grafeo-engine-0.5.42` source in `~/.cargo/registry/src/index.crates.io-*/grafeo-engine-0.5.42/src/`, the `grafeo-0.5.42` umbrella crate, `grafeo-loro` `src/`+`tests/`, `docs/grafeo-loro.architecture.md` §7, `docs/implementation-plan.md`. Hunter touched NO `src/` or `tests/` files (read-only mandate); only this critique file and `worklog.md` were modified.

---

## 0. Verification matrix — every L3 claim re-checked independently

### 0.1 Compile / test status

| L3 claim | Verification command | Result | Citation |
|---|---|---|---|
| `cargo check --all-targets` exit 0, 5 pre-existing warnings | `cargo check --all-targets 2>&1 \| tail -30` | ✅ PASS — 5 warnings (`hydration/vector.rs:27`, `presence/socket.rs:6`, `telemetry/health.rs:9`, `app.rs` builder fields, `hydration/vector.rs:9`), 0 errors, 0 new warnings | local run |
| `cargo test --no-run --all` emits 3 test binaries | `cargo test --no-run --all 2>&1 \| tail -10` | ✅ PASS — `unittests`, `integration-13c51a3c9b7180c2`, `unit-2e155c9954744fca` | local run |
| `cargo test --all` → 25 PASS + 0 IGNORED + 0 FAIL | `cargo test --all 2>&1 \| tail -40` | ✅ PASS — `6 passed; 0 failed; 0 ignored` (lib) + `5 passed; 0 failed; 0 ignored` (integration) + `14 passed; 0 failed; 0 ignored` (unit) + `0 passed; 0 failed; 0 ignored` (doctests) = **25 PASS + 0 IGNORED + 0 FAIL** | local run |
| Integration test stable across 10+ runs | `for i in 1..5; do cargo test --test integration tree_move_concurrency; done` | ✅ PASS — 5/5 runs PASS, 0 flakiness observed (L3 claimed 10+; verified 5 per Hunter mandate) | local run |

L3's compile/test claims are 100% accurate.

### 0.2 Stub verification (Task 3)

| Command | Result |
|---|---|
| `grep -nE "TODO\|todo!\|unimplemented!\|unreachable!\|panic!\(\)" src/schema/tree.rs` | exit 1 — ZERO matches ✅ |
| `grep -nE "TODO\|todo!\|unimplemented!" tests/unit/tree_move.rs` | exit 1 — ZERO matches ✅ |
| `grep -nE "TODO\|todo!\|unimplemented!" tests/integration/tree_move_concurrency.rs` | exit 1 — ZERO matches ✅ |
| `grep -rn "#\[ignore" tests/` | exit 1 — ZERO matches ✅ |
| `grep -rn "L2 HACK" src/ tests/` | exit 1 — ZERO matches ✅ |

All stub greps clean. L3's "zero TODO / zero ignore / zero L2 HACK" claim CONFIRMED.

### 0.3 Grafeo Session API citations (Task 5 — anti-hallucination)

Every non-trivial grafeo API call in `src/schema/tree.rs` + test files re-verified against `~/.cargo/registry/src/index.crates.io-*/grafeo-engine-0.5.42/src/`:

| API call | Citation in code | Actual location | Status |
|---|---|---|---|
| `GrafeoDB::session()` | `database/mod.rs:1663` | `database/mod.rs:1663` (`pub fn session(&self) -> Session`) | ✅ exact |
| `GrafeoDB::session_with_cdc(false)` | `database/mod.rs:1728` | `database/mod.rs:1728` (`#[cfg(feature = "cdc")] pub fn session_with_cdc(&self, cdc_enabled: bool) -> Session`) | ✅ exact; `cdc` feature confirmed enabled transitively (grafeo default → `embedded` → `ai` → `cdc`; `grafeo-0.5.42/Cargo.toml:68-72`) |
| `Session::begin_transaction_with_isolation(Serializable)` | `session/mod.rs:3895` | `session/mod.rs:3895` (`#[cfg(feature = "lpg")] pub fn begin_transaction_with_isolation(&mut self, isolation_level: crate::transaction::IsolationLevel) -> Result<()>`) | ✅ exact; `lpg` feature confirmed in grafeo-engine default (`grafeo-engine-0.5.42/Cargo.toml:59-67`) AND pulled transitively via `grafeo` default → `embedded` → `grafeo-engine/lpg` |
| `grafeo_engine::transaction::IsolationLevel::Serializable` | `transaction/manager.rs:63` | `transaction/manager.rs:63` (`Serializable` variant of `IsolationLevel` enum, doc "Serializable Snapshot Isolation (SSI)") | ✅ exact |
| `Session::create_node(&["Folder"])` | `session/mod.rs:4860` | `session/mod.rs:4860` (`#[cfg(feature = "lpg")] pub fn create_node(&self, labels: &[&str]) -> NodeId` — infallible, returns `NodeId`) | ✅ exact |
| `Session::create_edge(new_parent, node_id, TREE_EDGE_LABEL)` | `session/mod.rs:4935` | `session/mod.rs:4935` (`#[cfg(feature = "lpg")] pub fn create_edge(&self, src: NodeId, dst: NodeId, edge_type: &str) -> grafeo_common::types::EdgeId` — infallible, returns `EdgeId`) | ✅ exact; signature `(src, dst, label)` matches L3's parent→child call |
| `Session::delete_edge(eid)` | `session/mod.rs:5092` | `session/mod.rs:5092` (`#[cfg(feature = "lpg")] pub fn delete_edge(&self, id: EdgeId) -> bool`) | ✅ exact; returns `false` if edge absent |
| `Session::get_neighbors_incoming(cur)` | `session/mod.rs:5237` | `session/mod.rs:5237` (`pub fn get_neighbors_incoming(&self, node: NodeId) -> Vec<(NodeId, EdgeId)>`) | ✅ exact |
| `Session::get_neighbors_outgoing_by_type(old_parent, TREE_EDGE_LABEL)` | `session/mod.rs:5256` | `session/mod.rs:5256` (`pub fn get_neighbors_outgoing_by_type(&self, node: NodeId, edge_type: &str) -> Vec<(NodeId, EdgeId)>`) | ✅ exact |
| `Session::node_exists(node_id)` | `session/mod.rs:5278` | `session/mod.rs:5278` (`pub fn node_exists(&self, id: NodeId) -> bool`) | ✅ exact |
| `Session::prepare_commit()` | `session/mod.rs:4496` | `session/mod.rs:4496` (`pub fn prepare_commit(&mut self) -> Result<crate::transaction::PreparedCommit<'_>>`) | ✅ exact |
| `PreparedCommit::set_metadata("origin", ORIGIN_LORO_BRIDGE)` | `transaction/prepared.rs:107` | `transaction/prepared.rs:107` (`pub fn set_metadata(&mut self, key: impl Into<String>, value: impl Into<String>)`) | ✅ exact |
| `PreparedCommit::commit()` | `transaction/prepared.rs:124` | `transaction/prepared.rs:124` (`pub fn commit(mut self) -> Result<EpochId>`) | ✅ exact |

**Zero hallucinations.** All 13 grafeo API citations are exact file:line matches against the actual crate source. Both required features (`lpg` + `cdc`) are confirmed active via feature-flag chain analysis.

### 0.4 Anti-Goodhart verification (Task 4)

| Test | Asserts NON-TRIVIAL? | Specific assertion shape |
|---|---|---|
| `tree_move_basic` | ✅ | TWO-SIDED: `!leaf_parents.contains(&mid)` (old gone) AND `leaf_parents.contains(&root)` (new present) + sanity `mid_parents.contains(&root)` (untouched edge intact) |
| `tree_move_cycle_rejected` | ✅ | `matches!(err, GrafeoLoroError::TreeMoveCreatesCycle { .. })` (NOT `err.is_err()`) + graph-unchanged invariant `leaf_parents.contains(&_mid) && leaf_parents.len() == 1` |
| `tree_move_root_to_descendant_rejected_as_cycle` | ✅ | `matches!(err, TreeMoveCreatesCycle { .. })` + 3-node unchanged invariant (root parentless, `mid→root` intact, `leaf→mid` intact) |
| `tree_move_same_parent_noop` | ✅ | `before == after` edge-set equality as `Vec<(NodeId, EdgeId)>` (catches edge-id rewrite churn, not just `Ok(())`) + `after.len() == 1` |
| `tree_move_unknown_node_rejected` | ✅ | `Bridge(ref msg) if msg.contains("unknown node_id")` — pinned substring match (NOT generic `is_err()`) |
| `tree_move_unknown_new_parent_rejected` | ✅ | `Bridge(ref msg) if msg.contains("unknown new_parent")` — pinned substring match |
| `tree_move_to_self_direct_cycle_rejected` | ✅ | `matches!(err, TreeMoveCreatesCycle { .. })` for direct self-loop case `node_id == new_parent` |
| `concurrent_tree_moves_three_peers_converge_acyclic` | ✅ | BFS the ACTUAL grafeo graph from each node (`for &start in &all_nodes`), assert `parent != start` (no node is its own ancestor). `visited` set per walk handles diamonds. NOT just `assert all results Ok` — accepts `Ok / Err(Grafeo) / Err(TreeMoveCreatesCycle)` and only panics on `Err(Bridge)` or task panic. |

All 8 tests assert non-trivial properties. No `assert!(true)`, no asserting-what-was-just-set, no `err.is_err()` shortcuts on cycle/error variants. **Anti-Goodhart PASS.**

### 0.5 Anti-bloat verification (Task 6 — DRY)

| Check | Result |
|---|---|
| `TREE_EDGE_LABEL` reused from `crate::constants` | ✅ — imported at `src/schema/tree.rs:20`, no hardcoded `"CHILD"` in src/schema/tree.rs or test files (grep exit 1) |
| `ORIGIN_LORO_BRIDGE` reused from `crate::constants` | ✅ — imported at `src/schema/tree.rs:20`, no hardcoded `"loro-bridge"` (grep exit 1) |
| `parents_of` helper deduplicates parent-collection across tests | ✅ — defined once at `tests/unit/tree_move.rs:44`, called 7 times across `tree_move_basic`, `tree_move_cycle_rejected`, `tree_move_root_to_descendant_rejected_as_cycle`, `tree_move_same_parent_noop` |
| `build_chain_fixture` deduplicates 3-node chain setup | ✅ — defined once at `tests/unit/tree_move.rs:33`, called 4 times |
| Did L3 reinvent `apply_loro_op` / `apply_tree_move` / `parse_edge_key` / `BridgeMaps` in `src/schema/tree.rs`? | ✅ NO — `sync_tree_move_to_grafeo` operates directly on `GrafeoDB` + `Session`, does NOT touch `src/bridge/grafeo_tx.rs` |
| Did L3 reinvent a graph-traversal helper? | ✅ NO — no pre-existing BFS/cycle helper in `src/` outside `schema/tree.rs`; `would_create_cycle_precheck` is the only BFS in the codebase (verified via `grep -rnE "VecDeque\|BFS\|cycle\|ancest" src/`) |

**Zero DRY violations.** No reinvented utilities, no hardcoded constants, no duplicated helpers.

### 0.6 Anti-context-blindness verification (Task 7)

| Check | Result |
|---|---|
| Phase 1 origin-filter invariant intact? | ✅ — `sync_tree_move_to_grafeo` does NOT write to Loro (no `set_next_commit_origin`, no `apply_op`); `set_metadata("origin", ORIGIN_LORO_BRIDGE)` is advisory (dropped on commit per Devil Gap 1); `session_with_cdc(false)` means no CDC events generated, so epoch side-channel irrelevant. Existing `src/bridge/batcher.rs:196` + `src/bridge/sync_engine.rs:201` paths unchanged. |
| Any code that writes to Loro without setting origin? | ✅ NO — `sync_tree_move_to_grafeo` doesn't write to Loro at all. |
| Does `sync_tree_move_to_grafeo` interact with existing bridge? | ✅ NO — `grep -rn "sync_tree_move_to_grafeo" src/bridge/` returns exit 1. Task 2 scope (schema-only) respected. |
| L3 known limitation #1 (TOCTOU tree-ness) acceptable for Phase 2? | ⚠️ See M1 below — the trade-off itself is ACCEPTABLE (acyclicity is the mandate, not tree-ness; diamonds are not cycles), BUT the doc-comment hallucinates a defense that doesn't exist. |
| L3 known limitation #2 (`set_metadata` advisory-only) acceptable? | ✅ ACCEPTABLE — matches Phase 1 `src/bridge/batcher.rs:196` pattern; epoch side-channel is the real echo-prevention. |
| L3 known limitation #3 (CDC disabled for tree moves) acceptable? | ✅ ACCEPTABLE — `session_with_cdc(false)` prevents echo loops; tree→Loro reverse path is unscheduled (no phase in implementation-plan.md covers it). |

### 0.7 Anti-happy-path-bias verification (Task 8)

| Edge case | Handled? | How |
|---|---|---|
| Old parent edge doesn't exist (root node) | ✅ | `old_edge: Option<EdgeId>` is `None` if `get_neighbors_outgoing_by_type` doesn't find `dst == node_id`; `debug!` logged, skip delete. No panic. |
| Both `node_id` AND `new_parent` unknown | ✅ | `node_id` checked FIRST (`src/schema/tree.rs:98`), `new_parent` checked SECOND (`:103`). `node_id` error wins (deterministic ordering). |
| Disconnected components in pre-check (no path from `new_parent` to `node_id`) | ✅ | BFS `visited: HashSet<NodeId>` prevents infinite loops; disconnected `node_id` simply never reached → returns `false` (no cycle). |
| Very deep trees (stack overflow risk) | ✅ | Iterative `VecDeque<NodeId>` BFS, NO recursion. No stack overflow regardless of tree depth. |

**Zero happy-path bias.** All 4 edge cases handled defensively with `debug!` observability.

### 0.8 Edge direction consistency (Task 9)

| Site | Direction | Status |
|---|---|---|
| `src/schema/tree.rs:151` `session.create_edge(new_parent, node_id, TREE_EDGE_LABEL)` | parent→child (`src=parent, dst=child`) | ✅ |
| `src/schema/tree.rs:203` `session.get_neighbors_incoming(cur)` in pre-check | incoming = parents in parent→child graph | ✅ walks UPWARD to ancestors |
| `src/bridge/grafeo_tx.rs:213` `session.create_edge(new_parent_id, node_id, TREE_EDGE_LABEL)` | parent→child | ✅ (P2T2-L2 fix per Devil R1) |
| `src/bridge/grafeo_tx.rs:206` `EdgeKey = (old_parent_key, node_key, TREE_EDGE_LABEL)` | parent→child | ✅ |
| `src/bridge/grafeo_tx.rs:210` `EdgeKey = (new_parent_key, node_key, TREE_EDGE_LABEL)` | parent→child | ✅ |
| `tests/unit/tree_move.rs:38` `session.create_edge(root, mid, TREE_EDGE_LABEL)` | parent→child | ✅ |
| `tests/unit/tree_move.rs:39` `session.create_edge(mid, leaf, TREE_EDGE_LABEL)` | parent→child | ✅ |
| `tests/integration/tree_move_concurrency.rs:63-65` `create_edge(root, a, ...)`, `create_edge(a, b, ...)`, `create_edge(b, c, ...)` | parent→child | ✅ |
| Architecture §7 line 265 `INSERT (p)-[:CHILD]->(c)` | parent→child | ✅ mandate source |

**Edge direction 100% consistent.** All 9 sites use parent→child per architecture §7 line 265 + Devil R1. Zero BLOCKERs.

### 0.9 TOCTOU analysis (Task 10)

| Question | Answer |
|---|---|
| Is Serializable isolation effective for cycle-prevention? | ❌ NO — see M1. The pre-check runs in a SEPARATE session (`db.session()` at `src/schema/tree.rs:114`), OUTSIDE the Serializable tx (`db.session_with_cdc(false)` + `begin_transaction_with_isolation(Serializable)` at `:128-131`). SSI tracks reads WITHIN a Serializable tx; since the pre-check reads are in a different session, SSI does NOT track them. The doc-comment at `:56-64` CLAIMS SSI defends, but it does NOT. |
| Can two concurrent moves create a diamond (node with 2 parents)? | ✅ YES — L3's worklog known limitation #1 explicitly acknowledges this. Two peers can BOTH pass pre-check against stale snapshots and BOTH commit (disjoint write sets = no SSI write-write conflict). |
| Does the integration test catch this if it happens? | ✅ YES — the BFS acyclicity assertion at `tests/integration/tree_move_concurrency.rs:121-136` uses a `visited` set per walk, which handles diamonds (nodes with multiple parents) without infinite loops. The assertion is "no node is its own ancestor" (cycle), NOT "each node has ≤1 parent" (tree). Diamonds are NOT cycles; the test correctly asserts acyclicity, not tree-ness. |
| Is the documented "acyclic but not tree" behavior acceptable for Phase 2? | ✅ ACCEPTABLE — `docs/implementation-plan.md:53` mandates "consistent acyclic result", NOT "tree invariant". L3's behavior meets the mandate. BUT the doc-comment hallucination (M1) must be corrected. |

---

## 1. Findings

### BLOCKER: 0

None.

### MAJOR: 1

#### M1 — Doc-comment hallucinates SSI defense that doesn't exist (anti-hallucination + anti-Goodhart)

**File**: `src/schema/tree.rs:56-64`

**Claim in doc-comment**:
```
/// # TOCTOU defense (P2T2-DEVIL R3)
///
/// The cycle pre-check is racy under default `SnapshotIsolation` (peer B can
/// commit a cycle-creating edge between peer A's pre-check and A's commit —
/// the textbook SI write-skew anomaly). We defend by opening the write tx
/// with `Serializable` isolation (SSI); grafeo's SSI tracker detects the
/// read-write conflict between A's cycle-check and B's edge write and aborts
/// one peer at commit time. No post-commit re-check needed (Devil rejected
/// option (b) as eventually-consistent).
```

**Reality**: The pre-check at `src/schema/tree.rs:114` (`would_create_cycle_precheck(db, node_id, new_parent)`) opens its OWN session via `db.session()` at `src/schema/tree.rs:196` — OUTSIDE the Serializable tx opened at `:128-131`. SSI tracks reads WITHIN a Serializable tx; since the pre-check reads are in a DIFFERENT session, SSI does NOT track them. The "read-write conflict between A's cycle-check and B's edge write" described in the doc-comment is NOT detected by SSI.

**Evidence**: L3's own worklog known limitation #1 (worklog.md:1072) explicitly acknowledges: _"would_create_cycle_precheck opens its own session (outside the Serializable tx). Under concurrent moves, peer A's pre-check can pass against a stale snapshot while peer B's commit changes the ancestor path. SSI catches write-write conflicts on the SAME edge but not on disjoint edges — so concurrent moves targeting the same node via different old_parents can create diamonds (node with 2 parents)."_

So L3 is AWARE the doc-comment is wrong, but the doc-comment denies it. This is a documentation hallucination that misleads future maintainers into believing SSI defends against the TOCTOU when it does not.

**Devil R3 deviation**: The Devil R3 (per worklog.md:851 + `docs/critiques/p2t2-l1-devil.md`) explicitly reasoned: _"Q3 resolution (c) adopted Serializable isolation, no inside-tx re-check helper is needed — SSI catches concurrent-cycle write-skew at commit time."_ This reasoning ASSUMES the pre-check is INSIDE the Serializable tx. L3's implementation places it OUTSIDE, breaking the Devil R3 contract.

**Impact**: Future maintainers reading the doc-comment would conclude _"we don't need to worry about concurrent cycles because SSI catches them"_ — which is FALSE. They might add concurrent callers relying on this defense, or skip adding retry logic, or fail to instrument for diamonds. The actual behavior (TOCTOU under concurrent moves, diamonds possible) is ACCEPTABLE for Phase 2 (acyclicity is the mandate, not tree-ness), but the doc-comment must NOT lie about the defense.

**Concrete fix (TWO options, fixer chooses)**:

- **Option (a) [PREFERRED — makes the defense actually work]**: Refactor `would_create_cycle_precheck` to take `&Session` instead of `&GrafeoDB`, and call it AFTER `begin_transaction_with_isolation(Serializable)` at `:128-131` (so pre-check reads are tracked by SSI). Restructure `sync_tree_move_to_grafeo`:
  1. Validate existence (probe session — outside tx, OK because existence is a stable property)
  2. Noop guard (BEFORE tx-open per Devil R4 — unchanged)
  3. Open tx (Serializable)
  4. Pre-check cycle (INSIDE tx, using `&Session`)
  5. Delete old edge + insert new edge + prepare + commit (unchanged)

  This makes the doc-comment TRUE and the defense ACTUALLY work. SSI will detect read-write conflicts between peer A's pre-check reads and peer B's edge writes, aborting one peer at commit time. Diamonds become impossible (not just acyclic).

- **Option (b) [MINIMAL — corrects the doc-comment only]**: Replace the `# TOCTOU defense` doc-comment block with an accurate description:
  ```
  /// # TOCTOU limitation (P2T2-DEVIL R3 deviation)
  ///
  /// The cycle pre-check opens its OWN session (`db.session()`) OUTSIDE the
  /// Serializable tx below. SSI therefore does NOT track pre-check reads, and
  /// cannot detect read-write conflicts between peer A's pre-check and peer
  /// B's concurrent edge write. Two concurrent moves can BOTH pass pre-check
  /// against stale snapshots and BOTH commit (disjoint write sets = no SSI
  /// write-write conflict), creating diamonds (node with 2 parents). The
  /// final graph is always ACYCLIC (each move is individually acyclic
  /// relative to its pre-check snapshot), but the TREE invariant (≤1 parent
  /// per node) can be violated. This is ACCEPTABLE for Phase 2 (mandate is
  /// acyclicity, not tree-ness). If tree-ness is required, move the pre-check
  /// INSIDE the Serializable tx (refactor to `&Session` signature).
  ```

Either option resolves M1. Option (a) is preferred because it fulfills Devil R3's original intent; option (b) is acceptable for Phase 2 push-readiness.

### MINOR: 4

#### m1 — Test redundancy: `tree_move_cycle_rejected` and `tree_move_root_to_descendant_rejected_as_cycle` use IDENTICAL call (anti-Goodhart + DRY)

**File**: `tests/unit/tree_move.rs:95` and `tests/unit/tree_move.rs:121`

Both tests invoke the EXACT SAME call:
```rust
let err = sync_tree_move_to_grafeo(&db, root, root, leaf).unwrap_err();
```

The Devil M5/R2 mandate (per worklog.md:967) explicitly distinguished these as two cases:
- `tree_move_cycle_rejected` — the GENERAL cycle case (any node moved under its descendant)
- `tree_move_root_to_descendant_rejected_as_cycle` — the SPECIFIC root case (root with no parent edge + descendant new_parent)

L3 implemented BOTH with the root case. The general case (non-root node WITH a real parent edge being moved under its descendant) is NOT explicitly tested.

**Impact**: Mild — the pre-check code path is identical for both cases (BFS from `new_parent` checking if `node_id` is reachable; the existence of an old_parent edge is irrelevant to the pre-check). So no actual code path is uncovered. But the test redundancy is real (two tests doing the same thing), and the Devil M5/R2 mandate is partially violated.

**Concrete fix**: Change `tree_move_cycle_rejected` to move `mid` (which has `root` as a real parent) under `leaf` (its descendant):
```rust
#[test]
fn tree_move_cycle_rejected() {
    let db = GrafeoDB::new_in_memory();
    let (root, mid, leaf) = build_chain_fixture(&db);
    // Move `mid` (which has `root` as a real parent) under `leaf` — `leaf` is
    // a descendant of `mid`, so the pre-check must reject with
    // `TreeMoveCreatesCycle`. Unlike `tree_move_root_to_descendant_rejected_as_cycle`,
    // this exercises the GENERAL case where the moved node has a real parent edge.
    let err = sync_tree_move_to_grafeo(&db, mid, root, leaf).unwrap_err();
    assert!(
        matches!(err, GrafeoLoroError::TreeMoveCreatesCycle { .. }),
        "expected TreeMoveCreatesCycle, got {err:?}"
    );
    // Anti-Goodhart: graph unchanged — root→mid and mid→leaf both intact.
    assert_eq!(parents_of(&db, mid), vec![root], "root→mid edge must be intact");
    assert_eq!(parents_of(&db, leaf), vec![mid], "mid→leaf edge must be intact");
}
```

This makes the two tests DISTINCT (general case vs root case) as Devil M5/R2 intended.

#### m2 — Dead test code: 3 LoroDoc peers created but never used (anti-bloat + anti-Goodhart)

**File**: `tests/integration/tree_move_concurrency.rs:48-53` + `:140`

The integration test creates 3 `LoroDoc` peers (`peer1`, `peer2`, `peer3`) with `set_peer_id(1/2/3)`, but NEVER uses them functionally. The only reference after creation is `let _ = (&peer1, &peer2, &peer3);` at `:140` — a no-op that exists solely to suppress unused-variable warnings.

The test comments explicitly acknowledge this: _"3 LoroDoc peers model the CRDT-side concurrency surface (peer_id 1, 2, 3). They are NOT wired into sync_tree_move_to_grafeo (which operates directly on grafeo NodeIds) — they exist to mirror a real 3-peer deployment where each peer's LoroTree would emit a TreeMove op concurrently."_

The test name `concurrent_tree_moves_three_peers_converge_acyclic` implies CRDT peer convergence, but no CRDT convergence is tested. The test actually tests _"3 concurrent sync_tree_move_to_grafeo calls on the same GrafeoDB"_.

**Impact**: Mild — the test passes and verifies acyclicity correctly. But the decorative LoroDoc peers are dead code, and the test name is misleading.

**Concrete fix (TWO options)**:
- **Option (a) [PREFERRED — remove bloat]**: Delete the 3 `LoroDoc` peer blocks (`:23` `use loro::LoroDoc;`, `:48-53` peer creation, `:140` no-op touch). Rename the test to `concurrent_sync_tree_move_calls_acyclic` to reflect what it actually tests. This removes ~10 lines of dead code and 1 misleading name.
- **Option (b) [DEFER — wire the peers]**: Out of scope for Phase 2 (Task 2 mandate is schema-only; bridge wiring is unscheduled). Defer to a future phase that wires `LoroTree` into the inbound subscriber.

#### m3 — Doc-staleness: `src/error.rs:38` references `would_create_cycle` (renamed to `would_create_cycle_precheck` in L2 per Devil M4)

**File**: `src/error.rs:38`

```
/// pre-check via `schema::tree::would_create_cycle` and reject with this
```

The actual function (renamed in P2T2-L2 per Devil M4) is `would_create_cycle_precheck` at `src/schema/tree.rs:186`. The doc-comment was not updated.

**Impact**: Mild — doc-only staleness. Future maintainers grepping for `would_create_cycle` won't find the actual function.

**Concrete fix**: Update `src/error.rs:38` to:
```
/// pre-check via `schema::tree::would_create_cycle_precheck` and reject with this
```

#### m4 — Integration test doesn't verify concurrency was actually exercised (anti-Goodhart)

**File**: `tests/integration/tree_move_concurrency.rs:96-108`

The test classifies each peer's `Result` and accepts ALL of `Ok(())`, `Err(Grafeo(_))`, `Err(TreeMoveCreatesCycle)` as acceptable outcomes — only `Err(Bridge(_))` or task panic fail the test. This means the test PASSES whether the 3 calls actually run concurrently (with SSI conflicts) or serialize (no conflicts). The test does NOT assert that concurrency was exercised.

**Impact**: Mild — the test verifies the FINAL state is acyclic, which is the mandate. But if grafeo's internal locking serializes the 3 calls, the test would pass without ever exercising the concurrent-cycle-prevention path. A future regression that breaks SSI conflict detection would NOT be caught by this test.

**Concrete fix**: Add an assertion that at least one of the following happened (proves concurrent scheduling actually raced):
```rust
// Anti-Goodhart: assert concurrency was actually exercised. If all 3 calls
// serialized (no SSI conflict, no concurrent cycle rejection), the test
// would pass without exercising the concurrent path. Require at least one
// SSI conflict OR at least one Ok + at least one TreeMoveCreatesCycle.
let oks = join_results.iter().filter(|r| matches!(r.as_ref().unwrap(), Ok(()))).count();
let ssi = join_results.iter().filter(|r| matches!(r.as_ref().unwrap(), Err(GrafeoLoroError::Grafeo(_)))).count();
let cyc = join_results.iter().filter(|r| matches!(r.as_ref().unwrap(), Err(GrafeoLoroError::TreeMoveCreatesCycle { .. }))).count();
assert!(
    ssi > 0 || (oks > 0 && cyc > 0),
    "expected at least one SSI conflict OR mixed Ok+Cycle (proves concurrency); got oks={oks} ssi={ssi} cyc={cyc}"
);
```

NOTE: This assertion may be flaky if grafeo's internal locking serializes the calls deterministically. If so, the fixer should instead instrument the test with mock delays (e.g., inject a `tokio::time::sleep` into `sync_tree_move_to_grafeo` behind a `#[cfg(test)]` hook) to force concurrency. Defer to fixer's judgment.

### NIT: 1

#### n1 — `let _ = (&peer1, &peer2, &peer3);` is a Band-Aid for unused-variable warnings (Band-Aid)

**File**: `tests/integration/tree_move_concurrency.rs:140`

This line exists solely to suppress unused-variable warnings caused by m2 (decorative LoroDoc peers). It is a Band-Aid for the bloat. Resolving m2 (remove the decorative peers) eliminates this line.

**Concrete fix**: Resolved by m2 fix (option a).

### ACCEPTABLE: 3

#### a1 — TOCTOU under concurrent moves creates diamonds (L3 known limitation #1)

**File**: `src/schema/tree.rs:114` (pre-check outside Serializable tx)

The pre-check opens its own session (`db.session()`) outside the Serializable tx. Under concurrent moves, two peers can BOTH pass pre-check against stale snapshots and BOTH commit (disjoint write sets = no SSI write-write conflict), creating diamonds (node with 2 parents). The final graph is always ACYCLIC (each move is individually acyclic relative to its pre-check snapshot).

**Acceptable for Phase 2**: `docs/implementation-plan.md:53` mandates _"Concurrent tree moves from 3 peers → consistent acyclic result"_, NOT _"tree invariant (≤1 parent per node)"_. L3's behavior meets the acyclicity mandate. The integration test (`tests/integration/tree_move_concurrency.rs:121-136`) handles diamonds correctly via `visited` set per BFS walk.

**Caveat**: The doc-comment hallucination (M1) must be corrected. The trade-off itself is acceptable; the lie about it is not.

#### a2 — `set_metadata` advisory-only (L3 known limitation #2)

**File**: `src/schema/tree.rs:157`

`PreparedCommit::set_metadata("origin", ORIGIN_LORO_BRIDGE)` is dropped on `commit()` per Devil Gap 1 — never reaches `ChangeEvent`. The epoch side-channel (`bridge_origin_epochs` set in `src/bridge/batcher.rs:187-196`) is the real echo-prevention mechanism.

**Acceptable**: Matches Phase 1 `src/bridge/batcher.rs:196` pattern exactly. Code comment at `src/schema/tree.rs:153-155` accurately documents this. For `sync_tree_move_to_grafeo`, `session_with_cdc(false)` means no CDC events are generated at all, so the epoch side-channel is irrelevant — but the `set_metadata` call is retained for advisory logging consistency.

#### a3 — CDC disabled for tree moves (L3 known limitation #3)

**File**: `src/schema/tree.rs:128`

`db.session_with_cdc(false)` disables CDC tracking for tree moves. This means tree-move mutations are invisible to the outbound CDC poller. If the outbound poller ever needs to translate tree structure back to Loro, this will need revisiting.

**Acceptable for Phase 2**: Tree moves are triggered by Loro events; echoing them back would create a loop. The tree→Loro reverse path is unscheduled (no phase in `docs/implementation-plan.md` covers it). Code comment at `src/schema/tree.rs:126-127` accurately documents this.

---

## 2. Anti-pattern tally

| Anti-pattern | Count | Findings |
|---|---|---|
| Backward compatibility slaves | 0 | — |
| Tautology | 0 | — |
| Context Blindness | 0 | (Phase 1 origin-filter intact; no Loro writes; scope respected) |
| Band-Aids | 1 | n1 (suppressed by m2 fix) |
| Bloat (DRY Violations) | 2 | m1 (test redundancy), m2 (dead LoroDoc peers) |
| Hallucination | 1 | M1 (doc-comment hallucinates SSI defense) |
| Happy-Path Bias | 0 | (all 4 edge cases handled) |
| Goodhart's Law in Action | 2 | m1 (redundant test passes without adding coverage), m4 (integration test passes without verifying concurrency) |

---

## 3. Push-readiness verdict

**LOOP BACK TO FIXER**

**Rationale**: 1 MAJOR (M1 — doc-comment hallucinates SSI defense that doesn't exist). The actual code behavior is ACCEPTABLE for Phase 2 (acyclicity is the mandate), but the doc-comment must NOT lie about the defense. Fix is small (option b: correct the doc-comment; ~15 lines) or medium (option a: refactor pre-check inside Serializable tx; ~20 lines + test updates). Either option resolves M1 and restores push-readiness.

The 4 MINORs (m1-m4) and 1 NIT (n1) should also be addressed in L2-R2 for cleanliness, but none are push-blockers on their own.

**Verification summary**:
- Compile: ✅ EXIT 0, 5 pre-existing warnings (unchanged), 0 new
- Test: ✅ 25/25 PASS, 0 ignored, 0 failed (verified 5 runs of integration test, 0 flakiness)
- Stubs: ✅ All greps clean (zero TODO/todo!/unimplemented!/unreachable!/panic! in src/schema/tree.rs; zero #[ignore]; zero L2 HACK)
- Anti-Goodhart: ✅ All 8 tests assert non-trivial properties
- Anti-hallucination: ✅ All 13 grafeo API citations verified against crate source; both required features (`lpg` + `cdc`) confirmed active
- Anti-bloat: ✅ No reinvented utilities, no hardcoded constants
- Anti-context-blindness: ✅ Phase 1 origin-filter intact; no Loro writes; scope respected
- Anti-happy-path: ✅ All 4 edge cases handled
- Edge direction: ✅ 100% consistent parent→child across all 9 sites
- TOCTOU: ⚠️ Trade-off acceptable for Phase 2, but doc-comment hallucinates (M1)
