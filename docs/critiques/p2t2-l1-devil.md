# P2T2-L1 Devil's Advocate Critique

**Task ID**: P2T2-DEVIL
**Agent**: Devil's Advocate
**Branch**: `p2-tree-move`
**Target**: P2T2-L1 Scaffolder output for Phase 2 Task 2 (`schema::tree::sync_tree_move_to_grafeo`)
**Critique artifact**: this file
**Method**: read-only verification against `grafeo-engine-0.5.42` source in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/grafeo-engine-0.5.42/src/`, the `grafeo-0.5.42` umbrella crate, `grafeo-loro` `src/`+`tests/`, `docs/grafeo-loro.architecture.md` §5/§7, `docs/implementation-plan.md`. Devil touched NO `src/` or `tests/` files (read-only mandate); only this critique file and `worklog.md` were modified.

---

## 0. Verification matrix — every L1 claim re-checked independently

### 0.1 Compile / test status

| L1 claim | Verification command | Result | Citation |
|---|---|---|---|
| `cargo check --all-targets` exit 0, 5 pre-existing warnings | `cargo check --all-targets 2>&1 \| tail -30` | ✅ PASS — 5 warnings (`hydration/vector.rs:27`, `presence/socket.rs:6`, `telemetry/health.rs:9`, `app.rs` builder fields), 0 errors, 0 new warnings | local run |
| `cargo test --no-run --all` emits 3 test binaries | `cargo test --no-run --all 2>&1 \| tail -10` | ✅ PASS — `unittests`, `integration-7ec22efdf3e4b2a6`, `unit-5438de560f4f75d3` | local run |
| `cargo test --all` → 17 PASS + 5 IGNORED + 0 FAIL | `cargo test --all 2>&1 \| rg "test result"` | ✅ PASS — `6 passed; 0 failed; 0 ignored` (lib unittests) + `4 passed; 0 failed; 1 ignored` (integration) + `7 passed; 0 failed; 4 ignored` (unit) + `0 passed; 0 failed; 0 ignored` (doctests) = **17 PASS + 5 IGNORED + 0 FAIL** | local run |

L1's compile/test claims are 100% accurate.

### 0.2 Grafeo Session API citations (every line:line claim re-checked)

| L1 claim | Verification | Result | Actual citation |
|---|---|---|---|
| `GrafeoDB::session` — `database/mod.rs:1663` | `rg -n "pub fn session\b" database/mod.rs` | ✅ exact match | `database/mod.rs:1663` `pub fn session(&self) -> Session` |
| `GrafeoDB::session_with_cdc` — `database/mod.rs:1728` | `rg -n "pub fn session_with_cdc" database/mod.rs` | ✅ exact match | `database/mod.rs:1728` `pub fn session_with_cdc(&self, cdc_enabled: bool) -> Session` |
| `Session::begin_transaction` — `session/mod.rs:3883` | `rg -n "pub fn begin_transaction\b" session/mod.rs` | ✅ exact match | `session/mod.rs:3883` `pub fn begin_transaction(&mut self) -> Result<()>` |
| `Session::commit` — `session/mod.rs:3961` | `rg -n "pub fn commit\b" session/mod.rs` | ✅ exact match | `session/mod.rs:3961` `pub fn commit(&mut self) -> Result<()>` |
| `Session::prepare_commit` — `session/mod.rs:4496` | `rg -n "pub fn prepare_commit" session/mod.rs` | ✅ exact match | `session/mod.rs:4496` `pub fn prepare_commit(&mut self) -> Result<PreparedCommit<'_>>` |
| `Session::create_edge` — `session/mod.rs:4935` (infallible) | direct read | ✅ exact match — returns `grafeo_common::types::EdgeId` (NOT `Result<EdgeId>`); infallible confirmed | `session/mod.rs:4935-4940` |
| `Session::delete_edge` — `session/mod.rs:5092` (returns bool) | direct read | ✅ exact match — `pub fn delete_edge(&self, id: EdgeId) -> bool`; returns `false` if edge absent (no-op) | `session/mod.rs:5092-5107` |
| `Session::get_neighbors_outgoing_by_type` — "session/mod.rs (after 5237)" | direct read | ⚠️ vague but correct — actual location is `session/mod.rs:5256` (signature line); "after 5237" is true but imprecise | `session/mod.rs:5256-5268` |
| `Session::get_neighbors_incoming` — `session/mod.rs:5237` | direct read | ✅ exact match | `session/mod.rs:5237-5241` |
| `Session::node_exists` — "around 5280" | direct read | ⚠️ off by 2 — actual is `session/mod.rs:5278` | `session/mod.rs:5278-5280` |
| `PreparedCommit::set_metadata` — `transaction/prepared.rs:108` | direct read | ❌ **off by 1** — actual signature is at `prepared.rs:107`; line 108 is the function body (`self.metadata.insert(...)`) | `transaction/prepared.rs:107-109` |
| `PreparedCommit::commit` — `transaction/prepared.rs:124` | direct read | ✅ exact match | `transaction/prepared.rs:124-128` |
| `PreparedCommit::abort` — `transaction/prepared.rs:135` | direct read | ✅ exact match | `transaction/prepared.rs:135-138` |
| `grafeo` umbrella re-exports `Session` at top level | `rg "pub use.*Session" grafeo-0.5.42/src/lib.rs` | ✅ confirmed at `grafeo-0.5.42/src/lib.rs:68` (`pub use grafeo_engine::{... Session ...}`) | umbrella lib.rs:68 |

**L1 hallucination score: 0**. No fabricated APIs. Two minor citation drift issues (set_metadata off-by-1; node_exists ~2 lines off) — both NIT-level (see §3.N1, §3.N2).

### 0.3 Cycle-detection claim (Task 3 verification)

L1 claims grafeo 0.5.42 has NO native graph-edge acyclicity enforcement at commit time.

Independent verification (`rg -n "cycle|acyclic|Cycle" grafeo-engine-0.5.42/src/ | rg -v test`):

| Match | What it actually checks | Commit-time edge-cycle? |
|---|---|---|
| `procedures.rs:831` `has_negative_cycle` | Bellman-Ford query procedure (negative-weight cycle detection in weighted graphs) | NO — query-time algorithm, not commit-time constraint |
| `query/optimizer/join_order.rs:1048,1312` | Query-planner cycle detection in join graphs (a→b→c→a join pattern) | NO — query-planning only |
| `query/optimizer/mod.rs:2393,2449` | Same — query-planner acyclic-pattern detection | NO |
| `query/translators/gql/pattern.rs:607-628` | GQL pattern cycle (`target_var == current_source`) — for self-referential MATCH patterns like `(a)-[:KNOWS]->(a)` | NO — query-translation only |
| `query/translators/cypher.rs:793-814` | Same as above for Cypher | NO |

**Verdict**: L1's claim is **CORRECT**. Grafeo 0.5.42 has zero commit-time enforcement of user-edge acyclicity. The bridge MUST implement its own pre-check via `would_create_cycle`. The architecture doc §7 line 249 ("Loro's LoroTree enforces an acyclic graph internally") applies to the Loro-side `LoroTree` container, NOT to grafeo edges. The implementation-plan.md line 46 stale claim ("Grafeo enforces acyclic") is FALSE — see finding §3.N5.

### 0.4 `lpg` feature confirmation

L1 implicitly relies on `lpg` feature being enabled (because `create_edge`, `delete_edge`, `get_neighbors_*`, `node_exists`, `begin_transaction_with_isolation` are ALL `#[cfg(feature = "lpg")]` at their definition sites).

Verification: `grafeo-0.5.42/Cargo.toml` declares `default = ["embedded"]` and `embedded = ["grafeo-engine/lpg", "gql", "ai", "algos", "parallel", "regex", "grafeo-file", "arrow-export"]`. `grafeo-loro/Cargo.toml` declares `grafeo = "0.5"` with default features. → `lpg` is enabled transitively. L1's skeleton compiles because of this. ✅

**Fragility note**: if a future maintainer adds `default-features = false` to the grafeo dep, the entire tree-move path (and several other bridge paths) would fail to compile. Worth a comment in `Cargo.toml` (defer to hunter).

### 0.5 `IsolationLevel` reachability audit (NEW — L1 did not verify)

L1's open question #3 mentions `begin_transaction_with_isolation(Serializable)` as option (c). Verification:

- `IsolationLevel` is `pub enum IsolationLevel` at `grafeo-engine-0.5.42/src/transaction/manager.rs:43`
- Re-exported as `grafeo_engine::transaction::IsolationLevel` via `pub use manager::{... IsolationLevel ...}` at `transaction/mod.rs:200-202`
- **`grafeo` umbrella crate does NOT re-export `transaction` module** — confirmed by `rg "pub use grafeo_engine::" grafeo-0.5.42/src/lib.rs` (only `admin`, `auth`, `cdc`, `database`, `memory_usage`, `session` are re-exported as modules; `transaction` is absent)
- `grafeo-loro/Cargo.toml` does NOT declare `grafeo-engine` as a direct dep — only `grafeo = "0.5"` and `grafeo-common = "0.5"`

**Conclusion**: To call `begin_transaction_with_isolation(grafeo_engine::transaction::IsolationLevel::Serializable)`, grafeo-loro MUST add `grafeo-engine = "0.5"` as a direct dep. This is the hidden cost of option (c). See finding §3.M3 and resolution §1.Q3.

### 0.6 `apply_tree_move` direction verification (Task 4)

| Source | Direction | Citation |
|---|---|---|
| Architecture doc §7 line 259 | parent→child: `MATCH (p:Folder {id: $old_p})-[r:CHILD]->(c:Folder {id: $cid}) DELETE r` | `docs/grafeo-loro.architecture.md:259` |
| Architecture doc §7 line 265 | parent→child: `MATCH (p:Folder {id: $new_p}), (c:Folder {id: $cid}) INSERT (p)-[:CHILD]->(c)` | `docs/grafeo-loro.architecture.md:265` |
| Existing `apply_tree_move` | **child→parent**: `EdgeKey = (node_key, parent_key, "CHILD")` — i.e. `src=child, dst=parent` | `src/bridge/grafeo_tx.rs:200, 204, 206` |
| L1 skeleton (P2T2-L1) | **child→parent** (matches existing `apply_tree_move`, defers to DRY/SSOT) | `src/schema/tree.rs:54-59, 74-76` |

**Verdict**: There IS a real contradiction. The architecture doc is internally consistent (parent→child in both DELETE and INSERT pseudocode). The existing `apply_tree_move` is the outlier. L1 chose to follow the existing code rather than the spec — this propagates the bug into the new skeleton. See resolution §1.Q1 and finding §3.M1.

---

## 1. Resolutions for L1's 7 open questions

### Q1 — Edge direction contradiction → **MAJOR (fix in L2)**

**Recommendation**: **Parent→child is canonical** (architecture doc §7 lines 259, 265). The existing `apply_tree_move` at `src/bridge/grafeo_tx.rs:200-206` has the direction REVERSED (child→parent). L1 chose to follow the broken code rather than the spec — this propagates the bug into the new `sync_tree_move_to_grafeo` skeleton (`src/schema/tree.rs:54-59`).

**Rationale**:
1. The architecture doc is the canonical spec; both its DELETE and INSERT pseudocode use `parent-[:CHILD]->child`. This is the conventional reading ("parent is parent of child").
2. The existing `apply_tree_move` is DEAD CODE — `translate_diff_event` at `src/bridge/sync_engine.rs:419-538` only handles `ROOT_VERTICES`/`ROOT_EDGES` containers, never generating `LoroOp::TreeMove`. Fixing its direction costs nothing (no production caller).
3. Loro's `LoroTree` (the upstream CRDT container behind `T_CHILD`) also uses parent→child semantics: `tree.get_parent(tree_id)` returns the parent, and the doc §7 line 278 explicitly says "Parent/child is managed by the `LoroTree` container itself, queried via `tree.get_parent(tree_id)`". The bridge should mirror this convention.
4. Fixing the direction now prevents future contributors from copying the wrong direction into VertexBuilder or other consumers (Task 9 cross-phase coupling check — VertexBuilder doesn't currently use `TREE_EDGE_LABEL` per `src/app.rs:122-143`, but a future `.with_parent()` method would inherit the convention).

**Concrete fix (L2)**:
- `src/bridge/grafeo_tx.rs:200`: `let old_key: EdgeKey = (old_parent_key.to_string(), node_key.to_string(), "CHILD".to_string());` (swap first two tuple positions)
- `src/bridge/grafeo_tx.rs:204`: `let new_key: EdgeKey = (new_parent_key.to_string(), node_key.to_string(), "CHILD".to_string());` (swap)
- `src/bridge/grafeo_tx.rs:206`: `let eid = session.create_edge(new_parent_id, node_id, "CHILD");` (swap src/dst args)
- `src/schema/tree.rs:54-59` (skeleton TODO comments): rewrite to use parent→child: "delete `(old_parent → node_id)` edge, insert `(new_parent → node_id)` edge"
- `src/schema/tree.rs:74-76` (`would_create_cycle` doc-comment): "Edge direction is parent→child (src=parent, dst=child); 'upward' therefore means following `get_neighbors_incoming(cur)` (edges pointing TO `cur` lead to `cur`'s parents)"
- `src/schema/tree.rs:86-87` (TODO comment in `would_create_cycle` body): "walk parent chain via `session.get_neighbors_incoming(cur)`; return true iff `node_id` appears in the ancestor set of `new_parent` (or iff `new_parent == node_id` — direct self-loop)"
- `tests/unit/tree_move.rs:29` (helper doc-comment): rewrite fixture description: "build a 3-node fixture `(root, mid, leaf)` wired as `root --CHILD--> mid --CHILD--> leaf` (parent→child direction per architecture §7 line 265)"

### Q2 — Root-move error variant → **MINOR (fix in L2)**

**Recommendation**: Pin the variant as **`TreeMoveCreatesCycle`** (NOT `Bridge("no parent edge for root …")`). The function should use **best-effort delete** semantics: if the `(node_id → old_parent)` edge doesn't exist (e.g., node is a root), log a warning and continue. The ONLY rejection reason is `TreeMoveCreatesCycle` (from pre-check or post-insert re-check).

**Rationale**:
1. **Consistency with existing `apply_tree_move`** (`src/bridge/grafeo_tx.rs:192-198`): the existing code uses `if let Some(id) = maps.remove_edge(&old_key) { ... }` — silently no-ops if the edge is absent. The new `sync_tree_move_to_grafeo` should match this semantics.
2. **SRP**: "node has no current parent" is not an error condition — it's a valid state (root nodes have no parent). The function's job is "make `new_parent` be `node_id`'s parent after this call"; whether `old_parent` was already correct, was wrong, or was absent is the caller's concern.
3. **Anti-Goodhart**: a structured `TreeMoveCreatesCycle { node_id, new_parent }` variant is matchable via `matches!` (the test scaffold already uses this pattern at `tests/unit/tree_move.rs:55-60`). A `Bridge("no parent edge for root …")` substring-match is fragile (stringly-typed).
4. **Test impact**: `tree_move_root_to_leaf_rejected` as scaffolded (`sync_tree_move_to_grafeo(root, root, leaf)` where `leaf` is a descendant of `root`) is actually a CYCLE test under best-effort semantics — `new_parent (leaf)` is a descendant of `node_id (root)`, so the pre-check rejects with `TreeMoveCreatesCycle`. The test name is misleading; see finding §3.M4.

**Concrete fix (L2)**:
- Update the test scaffold `tree_move_root_to_leaf_rejected` (`tests/unit/tree_move.rs:67-72`) to assert `matches!(err, GrafeoLoroError::TreeMoveCreatesCycle { .. })` (matching `tree_move_cycle_rejected`'s pattern at `tests/unit/tree_move.rs:55-60`).
- OR rename the test to `tree_move_root_to_descendant_rejected_as_cycle` for clarity (preferred — the current name suggests "no parent edge" rejection, which contradicts the recommended best-effort semantics).
- Update the L1 skeleton TODO at `src/schema/tree.rs:53-56` to make the best-effort semantics explicit: "Resolve the existing `(old_parent → node_id)` EdgeId by walking `session.get_neighbors_incoming(node_id)` and matching src == old_parent; **if no edge found (node is currently a root), log warn and continue (best-effort delete)**; otherwise `session.delete_edge(eid)`."

### Q3 — Concurrent-cycle race (TOCTOU) → **MAJOR (fix in L2)**

**Recommendation**: **Option (c) `begin_transaction_with_isolation(IsolationLevel::Serializable)`** is the correct defense, with the prerequisite of adding `grafeo-engine = "0.5"` as a direct dep in `Cargo.toml`. As a fallback if the dep addition is rejected, use **option (a) re-check inside tx post-insert pre-commit + rollback** with documented "best-effort, not write-skew-safe" semantics.

**Rationale (why (c) is preferred)**:
1. **SI is insufficient for write-skew cycles**. Concrete failure mode under default `SnapshotIsolation`:
   - Initial tree: `A → B`, `A → C` (A is parent of siblings B and C).
   - Peer X: move B under C (`new_parent=C`). Cycle-check walks C's ancestors = {A}. B not in {A}. Pass. Insert `C → B`.
   - Peer Y: move C under B (`new_parent=B`). Cycle-check walks B's ancestors = {A}. C not in {A}. Pass. Insert `B → C`.
   - Both transactions commit (no write-write conflict — different edges).
   - **Final state has a cycle**: `A → B → C → B → C → …` (B's parent is C, C's parent is B).
   - This is the textbook SI write-skew anomaly.
2. **Serializable (SSI) catches it**: grafeo's SSI tracks read-write conflicts. Peer X's cycle-check reads edges involving C (and node C itself); Peer Y writes to C (modifying C's outgoing edges in parent→child convention). SSI detects the read-write conflict and aborts one peer. Verified at `grafeo-engine-0.5.42/src/transaction/manager.rs:313-322` (`if our_isolation == IsolationLevel::Serializable && !our_read_set.is_empty()`).
3. **Why (a) is insufficient**: under SI, A's tx-internal re-check walks ancestors of `new_parent` AS VISIBLE TO A'S SNAPSHOT. If B commits between A's tx-begin and A's insert, A's snapshot doesn't see B's write. A's re-check passes. Both commit. Cycle.
4. **Why (b) is insufficient**: post-commit audit detects the cycle AFTER it's persisted. Undoing requires another tx (move the node back to `old_parent`), which itself can race with concurrent moves. Eventually-consistent, but ugly and not what the integration test asserts (`tests/integration/tree_move_concurrency.rs:10` asserts "The final committed graph is acyclic" — this must hold AT THE END OF THE TEST, not eventually).

**Cost of (c)**:
- Add `grafeo-engine = "0.5"` to `[dependencies]` in `Cargo.toml`. The crate is already loaded transitively via `grafeo`, so no new code enters the build graph — only a direct path to `grafeo_engine::transaction::IsolationLevel`.
- Use `session.begin_transaction_with_isolation(grafeo_engine::transaction::IsolationLevel::Serializable)` instead of `session.begin_transaction()`.
- The cycle-check's read-set MUST walk the full ancestor chain (not just direct parent) for SSI to detect cross-ancestor conflicts. `would_create_cycle` already does this (BFS, not depth-1).
- L3 must handle `Err(GrafeoLoroError::Grafeo(_))` from `prepare_commit`/`commit` as a retryable serialization conflict (the integration test scaffold already acknowledges this at `tests/integration/tree_move_concurrency.rs:41-43`).

**Fallback (a) — if grafeo-engine direct dep is rejected**:
- Use `session.begin_transaction()` (default SI).
- Inside the tx, AFTER `create_edge(new_parent, node_id, TREE_EDGE_LABEL)` and BEFORE `prepare_commit`, walk `get_neighbors_incoming(new_parent)` looking for `node_id`. If found, `session.rollback()` and return `Err(TreeMoveCreatesCycle)`.
- This catches SINGLE-PEER cycles (the move itself creates a cycle) but NOT concurrent-peer write-skew cycles. Document this limitation in the function doc-comment.
- For the integration test, design the 3-peer moves to NOT trigger write-skew (e.g., 3 peers reparenting 3 different leaves under a common ancestor — no overlap). The test then asserts acyclicity under "non-adversarial concurrency", not "arbitrary concurrency".

**Concrete fix (L2)**:
- Update `src/schema/tree.rs:51-52` TODO to: `let mut session = db.session(); session.begin_transaction_with_isolation(grafeo_engine::transaction::IsolationLevel::Serializable)?;`
- Add `grafeo-engine = "0.5"` to `Cargo.toml` `[dependencies]`.
- Update `src/schema/tree.rs:63-64` TODO to remove "Re-verify acyclicity post-commit" — Serializable makes this unnecessary. Replace with: "Serializable isolation (SSI) catches concurrent-cycle write-skew at commit time; no post-commit re-check needed."
- Update `tests/integration/tree_move_concurrency.rs:32-33` doc-comment to reflect that the test relies on Serializable isolation (not "write-write conflict" — that's SI, not SSI).

### Q4 — Same-parent noop semantics → **MINOR (fix in L2)**

**Recommendation**: **Short-circuit BEFORE opening a tx**, but AFTER the pre-check. The noop check is `if new_parent == old_parent { return Ok(()); }` placed between the cycle pre-check and `session.begin_transaction(...)`.

**Rationale**:
1. **YAGNI / Deletion-over-addition**: if the move is a noop, no tx is needed. Opening a tx for a noop is wasted work (session allocation, snapshot capture, commit epoch allocation).
2. **Avoids false write-write conflicts**: under SI/SSI, opening a tx and writing the same edge that already exists still counts as a write to that edge. If a concurrent peer is genuinely moving the edge, the noop-peer's "write" can trigger a spurious conflict abort. Short-circuiting avoids this.
3. **Absolute Idempotency** (anti-plenger rule #9): calling `sync_tree_move_to_grafeo(db, n, A, A)` N times must have the same effect as calling it once. Short-circuit guarantees this without relying on `delete_edge` + `create_edge` being perfectly idempotent (which they are, but the tx overhead is real).
4. **Order matters**: the noop check MUST come AFTER the cycle pre-check. If `new_parent == node_id` AND `old_parent == node_id`, that's a self-loop cycle — reject with `TreeMoveCreatesCycle`, NOT a noop Ok. The pre-check catches this.

**Concrete fix (L2)**:
- Update `src/schema/tree.rs:49-50` (first TODO) to add the noop guard BEFORE the session-open TODO:
  ```rust
  // TODO(P2T2-L3): Pre-check cycle: if would_create_cycle(db, node_id, new_parent)
  //                 return Err(GrafeoLoroError::TreeMoveCreatesCycle { node_id, new_parent }).
  // TODO(P2T2-L3): Noop guard: if new_parent == old_parent { return Ok(()); } // idempotent, no tx
  // TODO(P2T2-L3): let mut session = db.session();
  // TODO(P2T2-L3): session.begin_transaction_with_isolation(Serializable)?;
  ```
- Remove the existing "If new_parent != old_parent (idempotent-noop guard)" clause from `src/schema/tree.rs:57-58` TODO — that guard was inside the tx, which is the wrong place per this resolution.

### Q5 — `apply_tree_move` literal hardcoding → **MINOR (fix in L2, in-scope)**

**Recommendation**: **IN-SCOPE for L2**. Refactor `apply_tree_move` at `src/bridge/grafeo_tx.rs:200, 204, 206` to use `TREE_EDGE_LABEL` instead of the literal `"CHILD"`. This is a 3-line mechanical change.

**Rationale**:
1. **DRY/SSOT violation**: L1 declared `TREE_EDGE_LABEL: &str = "CHILD"` in `src/constants.rs:16` specifically as the SSOT for this literal. Leaving 3 literal uses in the only other call-site that produces the same edge label is the textbook DRY violation. The constant exists; not using it everywhere is worse than not having the constant.
2. **Cost is trivial**: 3 lines of `s/"CHILD"/TREE_EDGE_LABEL/`. The function is dead code (no caller), so refactor risk is zero.
3. **YAGNI-compliant**: this isn't scope creep — the constant was created precisely to be the SSOT, and SSOT means "used everywhere", not "used in new code only".
4. **Prevents drift**: if a future maintainer renames the edge label (e.g., from `"CHILD"` to `"PARENT_OF"`), the literal uses would silently diverge from the constant. Refactoring now locks them together.
5. **Caveat**: there's a `use crate::constants::TREE_EDGE_LABEL;` import needed at the top of `src/bridge/grafeo_tx.rs`. Verify it doesn't conflict with existing imports.

**Concrete fix (L2)**:
- `src/bridge/grafeo_tx.rs:200`: `"CHILD".to_string()` → `TREE_EDGE_LABEL.to_string()`
- `src/bridge/grafeo_tx.rs:204`: same
- `src/bridge/grafeo_tx.rs:206`: `session.create_edge(node_id, new_parent_id, "CHILD")` → `session.create_edge(node_id, new_parent_id, TREE_EDGE_LABEL)` (NOTE: if Q1 resolution is also applied, this becomes `session.create_edge(new_parent_id, node_id, TREE_EDGE_LABEL)` — parent→child direction)
- Add `use crate::constants::TREE_EDGE_LABEL;` to the imports at top of file (verify no existing import).

### Q6 — `ORIGIN_LORO_BRIDGE` metadata on tree-move commit → **NIT (no action)**

**Recommendation**: **KEEP the `set_metadata` call** in the L1 skeleton (already present as a TODO at `src/schema/tree.rs:61`). No action needed.

**Rationale**:
1. **Defensive consistency**: the call matches the established pattern in `src/bridge/batcher.rs:193-196` (with the explicit "Devil BLOCKER B2: set_metadata is dropped on commit()" comment). Keeping the pattern consistent across all Loro→Grafeo commit sites is more valuable than removing one advisory call.
2. **Documents intent**: the call signals "this tx originated from the Loro bridge" even if grafeo 0.5.42 doesn't propagate metadata to CDC events. A future grafeo patch that propagates metadata would make this call functional with zero code changes.
3. **Cost is negligible**: 1 LOC, no runtime cost (HashMap insert into a transient `PreparedCommit`).
4. **Already commented**: the skeleton TODO at `src/schema/tree.rs:61` already has `// advisory, dropped on commit` — the rationale is documented inline.
5. **Anti-plenger "Deletion over addition"**: the call is already there; removing it would be a change, not a deletion. Keep.

**Concrete fix (L2)**: none. Optionally, L2 can tighten the comment to reference the batcher precedent: `// advisory, dropped on commit (see src/bridge/batcher.rs:193-196 for pattern)`.

### Q7 — Bridge wiring scope boundary → **MINOR (file follow-up, NOT a Task 2 blocker)**

**Recommendation**: **L1's scope boundary IS correct per the implementation plan**, but the implementation plan has a hidden gap: bridge wiring for `LoroOp::TreeMove` is not listed in ANY phase (2 through 6). File a follow-up to address this in Phase 6 hardening (or a future Phase 7).

**Rationale**:
1. **Implementation plan verification**: `docs/implementation-plan.md:42-46` lists Phase 2 Task 2 as "Implement `schema::tree::sync_tree_move_to_grafeo`" with 4 sub-bullets (delete old parent edge, insert new parent edge, wrap in single Grafeo tx, return error if cycle detected). NONE of the sub-bullets mention bridge wiring. Task 3 is `VertexBuilder`. Phase 3-6 don't mention tree-move bridge wiring either.
2. **Architecture doc verification**: §7 line 250 says "LoroGrafeoBridge catches tree move events, translates them to Grafeo-compliant transaction mutations" — this implies bridge wiring IS eventually needed, but the implementation plan doesn't schedule it.
3. **Current state verification**: `src/bridge/sync_engine.rs:419-538` `translate_diff_event` only handles `ROOT_VERTICES` and `ROOT_EDGES` containers. The `_ => { tracing::trace!(... "non V/E root container diff; skipping"); }` arm at `sync_engine.rs:532-534` catches everything else (including any future `T_CHILD` LoroTree container). `LoroOp::TreeMove` is therefore NEVER generated in production.
4. **L1's skeleton is correct in scope**: `sync_tree_move_to_grafeo` is callable directly (via tests and future bridge wiring), but no production caller exists. This mirrors `apply_tree_move`'s situation (dead path per Hunter MINOR 8 in P2 Task 1's hunt). The function exists; wiring is a separate concern.
5. **The gap is in the implementation plan, not in L1's work**: filing a follow-up to wire `LoroOp::TreeMove` generation into `translate_diff_event` (when the `T_CHILD` LoroTree container is wired into the LoroDoc schema) is an ORCH-level decision, not a Task 2 L2 fix.

**Concrete fix (L2)**:
- Add a "Known Limitation" note to `src/schema/tree.rs` module doc-comment (NOT to the function doc-comment) referencing this gap:
  ```rust
  //! Note (P2T2-DEVIL Q7): `sync_tree_move_to_grafeo` has no production caller
  //! as of Phase 2 Task 2. `translate_diff_event` (`src/bridge/sync_engine.rs:419`)
  //! only translates `ROOT_VERTICES`/`ROOT_EDGES` diffs; `LoroOp::TreeMove` is
  //! declared but never generated. Wiring the `T_CHILD` LoroTree container into
  //! the inbound subscriber is unscheduled (no phase in `docs/implementation-plan.md`
  //! covers it). The function is exercised only by `tests/unit/tree_move.rs` and
  //! `tests/integration/tree_move_concurrency.rs` until bridge wiring lands.
  ```
- File a follow-up note in the worklog OR a TODO in `docs/implementation-plan.md` for Phase 6 hardening.

---

## 2. Top NEW findings (issues L1 missed)

### M1 — `would_create_cycle` helper signature cannot be used for inside-tx re-check (Q3 fallback path) → **MAJOR**

**Context**: L1 declared `fn would_create_cycle(db: &GrafeoDB, node_id: NodeId, new_parent: NodeId) -> bool` at `src/schema/tree.rs:84`. This signature takes `&GrafeoDB` (immutable), which means the helper must open its OWN `Session` to walk the graph.

**Problem**: grafeo's `Session::begin_transaction` is documented as "Returns an error if a transaction is already active" (`session/mod.rs:3891-3893`). If the helper is called from inside an active tx (e.g., for the post-insert pre-commit re-check prescribed by Q3 fallback option (a)), opening a new session would either fail OR create a nested-tx savepoint (`session/mod.rs:3911-3918` auto-savepoint behavior) that does NOT see the parent tx's uncommitted writes. The helper would walk the OLD tree (pre-insert state), missing the cycle that the just-inserted edge creates.

**Why L1 missed this**: L1's skeleton prescribes "Re-verify acyclicity post-commit (defensive)" at `src/schema/tree.rs:63-64` — i.e., the re-check is OUTSIDE the tx, after commit. That sidesteps the issue. But under Q3 resolution (c) Serializable, no inside-tx re-check is needed; and under Q3 fallback (a) inside-tx re-check, the helper signature is wrong.

**Concrete fix (L2)**: Split the helper into two functions:
- `fn would_create_cycle_precheck(db: &GrafeoDB, node_id: NodeId, new_parent: NodeId) -> bool` — opens its own session (read-only), walks the parent chain. Used OUTSIDE the main tx (pre-check).
- `fn would_create_cycle_in_tx(session: &grafeo::Session, node_id: NodeId, new_parent: NodeId) -> bool` — takes a `&Session` reference, walks the parent chain WITHIN the active tx (sees uncommitted writes). Used INSIDE the main tx (post-insert re-check, only needed if Q3 fallback (a) is chosen).

If Q3 resolution (c) Serializable is adopted, only `would_create_cycle_precheck` is needed (Serializable handles concurrent-cycle detection via SSI; no inside-tx re-check needed). The skeleton should declare ONLY the precheck variant in that case.

### M2 — Test scaffold `tree_move_root_to_leaf_rejected` is mis-named and asserts the wrong contract → **MAJOR** (downgraded from BLOCKER because the test is `#[ignore]` and L3 can fix at fill-time)

**Context**: `tests/unit/tree_move.rs:67-72` declares:
```rust
fn tree_move_root_to_leaf_rejected() {
    let _ = (TREE_EDGE_LABEL, build_chain_fixture, sync_tree_move_to_grafeo);
    todo!("P2T2-L3: build chain; call sync_tree_move_to_grafeo(root, root, leaf) — root has no parent edge — assert Err returned")
}
```

**Problem**: The test name "root_to_leaf_rejected" suggests the rejection reason is "root has no parent edge" (a structural invariant violation). But under Q2's recommended best-effort-delete semantics, "no parent edge" is NOT a rejection reason — the function continues and inserts the new edge. The actual rejection (if `leaf` is a descendant of `root`) is `TreeMoveCreatesCycle`, which is the SAME variant tested by `tree_move_cycle_rejected` at `tests/unit/tree_move.rs:49-62`. The two tests are redundant under Q2 semantics.

If Q2's resolution is NOT adopted (i.e., L3 chooses strict "reject if no parent edge" semantics), then the test name matches the assertion, but the test overlaps conceptually with `tree_move_cycle_rejected` only if `leaf` is a non-descendant (different tree's leaf). The L1 scaffold doesn't specify which.

**Why L1 missed this**: L1 left the variant choice to Devil (Q2). L1 didn't realize that the test setup (`root, root, leaf`) is a CYCLE under any reasonable tree structure (root is ancestor of leaf), making the test a duplicate of `tree_move_cycle_rejected` regardless of Q2's resolution.

**Concrete fix (L2)**: Either:
- **Option A (preferred)**: Rename `tree_move_root_to_leaf_rejected` → `tree_move_root_to_descendant_rejected_as_cycle` and assert `matches!(err, GrafeoLoroError::TreeMoveCreatesCycle { .. })`. The test then covers the case "node_id is currently a root (no parent edge) AND new_parent is a descendant" — a specific edge case of cycle rejection where the pre-check must catch the cycle WITHOUT relying on a delete-then-recheck pattern (since there's no edge to delete).
- **Option B**: Repurpose the test to use a DIFFERENT tree's leaf (not a descendant of root): `sync_tree_move_to_grafeo(root, ROOT_SENTINEL, OTHER_TREE_ROOT)`. Under Q2 best-effort semantics, this returns `Ok(())` (root gains a parent). The test name becomes `tree_move_root_reparented_to_other_tree_ok` and asserts Ok. This tests the "no parent edge to delete" path with a non-cyclic outcome.

Either option resolves the ambiguity. Option A is preferred because it adds a distinct cycle-rejection case (root with no parent + descendant new_parent) that `tree_move_cycle_rejected` doesn't explicitly cover.

### M3 — Skeleton TODO at `src/schema/tree.rs:63-64` prescribes "Re-verify acyclicity post-commit" — this is option (b), which Devil rejects → **MAJOR**

**Context**: `src/schema/tree.rs:63-64`:
```rust
// TODO(P2T2-L3): Re-verify acyclicity post-commit (defensive) — see Devil open question about
//                 concurrent peer moves invalidating the pre-check.
```

**Problem**: Post-commit re-verify is too late — the cycle is already persisted. Undoing requires another tx (move the node back to `old_parent`), which itself can race with concurrent moves. This is the "eventually consistent" approach, which contradicts the integration test's assertion at `tests/integration/tree_move_concurrency.rs:10`: "The final committed graph is acyclic" — this must hold AT THE END OF THE TEST, not eventually.

**Why L1 missed this**: L1 listed option (b) as one of three options in open question #3 but didn't take a position. The skeleton TODO implicitly endorses (b) by prescribing "Re-verify acyclicity post-commit" without mentioning Serializable or inside-tx re-check.

**Concrete fix (L2)**: Per Q3 resolution, replace the TODO:
- If Q3 (c) Serializable adopted: remove the post-commit re-verify TODO entirely. Replace with a comment: "Serializable isolation (SSI) catches concurrent-cycle write-skew at commit time; no post-commit re-check needed."
- If Q3 fallback (a) adopted: replace with an INSIDE-tx re-check TODO: "Re-verify acyclicity INSIDE tx post-insert pre-commit: walk `session.get_neighbors_incoming(new_parent)` looking for `node_id`; if found, `session.rollback()` and return `Err(TreeMoveCreatesCycle)`. NOTE: this catches single-peer cycles but NOT concurrent-peer write-skew cycles under SI — document this limitation."

### M4 — Missing test scaffolds for "node doesn't exist" and "new_parent doesn't exist" contracts → **MINOR**

**Context**: L1's 4 unit test scaffolds cover: basic move, cycle rejection, root-to-leaf (cycle), same-parent noop. None cover:
- `sync_tree_move_to_grafeo(db, INVALID_NODE, A, B)` — what happens when `node_id` doesn't exist in grafeo?
- `sync_tree_move_to_grafeo(db, VALID_NODE, A, INVALID_B)` — what happens when `new_parent` doesn't exist?
- `sync_tree_move_to_grafeo(db, X, A, X)` — direct self-loop (X reparented under itself).

**Why this matters**: `apply_tree_move` at `src/bridge/grafeo_tx.rs:192-198` silently returns `Ok(())` when `node_key` or `new_parent_key` isn't in `node_id_map` (the bridge-side Loro→Grafeo id map). But `sync_tree_move_to_grafeo` takes raw `NodeId` (not loro_key), so the "missing from map" path doesn't apply — the question is whether grafeo's `session.create_edge(invalid_id, ...)` panics, returns a sentinel, or silently creates a dangling edge.

Verified: `Session::create_edge` at `session/mod.rs:4935-4959` is infallible — it calls `self.active_lpg_store().create_edge_versioned(src, dst, ...)` without checking existence. Grafeo may or may not create a dangling edge (depends on store-level validation). The contract for `sync_tree_move_to_grafeo` must be pinned.

**Concrete fix (L2)**: Add 3 scaffolds to `tests/unit/tree_move.rs`:
```rust
/// Moving a non-existent node must return Err (NOT silently noop).
/// Anti-plenger: silent-noop would hide caller bugs.
#[test]
#[ignore = "P2T2-L1 scaffold: L3 implements the body"]
fn tree_move_unknown_node_rejected() {
    let _ = (TREE_EDGE_LABEL, build_chain_fixture, sync_tree_move_to_grafeo);
    todo!("P2T2-L3: build chain; call sync_tree_move_to_grafeo(db, INVALID_NODE, A, B); assert Err returned (variant: Devil pins — recommend Bridge(\"unknown node_id …\"))")
}

/// Moving under a non-existent new_parent must return Err.
#[test]
#[ignore = "P2T2-L1 scaffold: L3 implements the body"]
fn tree_move_unknown_new_parent_rejected() {
    let _ = (TREE_EDGE_LABEL, build_chain_fixture, sync_tree_move_to_grafeo);
    todo!("P2T2-L3: build chain; call sync_tree_move_to_grafeo(db, VALID_NODE, A, INVALID_B); assert Err returned")
}

/// Direct self-loop: sync_tree_move_to_grafeo(db, X, A, X) must return
/// Err(TreeMoveCreatesCycle) — new_parent == node_id is a trivial cycle.
#[test]
#[ignore = "P2T2-L1 scaffold: L3 implements the body"]
fn tree_move_to_self_direct_cycle_rejected() {
    let _ = (TREE_EDGE_LABEL, build_chain_fixture, sync_tree_move_to_grafeo);
    todo!("P2T2-L3: build chain X→A; call sync_tree_move_to_grafeo(db, X, A, X); assert matches!(err, TreeMoveCreatesCycle { .. })")
}
```

**Devil pins the contract**: `sync_tree_move_to_grafeo` MUST validate `node_exists(node_id)` AND `node_exists(new_parent)` BEFORE opening a tx, returning `Err(GrafeoLoroError::Bridge("unknown node_id: …".into()))` if either is absent. This matches the bridge-side pattern at `src/bridge/grafeo_tx.rs:155-159` (`unknown node key(s): src=… dst=…`).

### M5 — `Cargo.toml` lacks `grafeo-engine` direct dep, blocking Q3 option (c) → **MAJOR** (only if Q3 (c) is adopted)

**Context**: `Cargo.toml:7` declares `grafeo = "0.5"` and `Cargo.toml:11` declares `grafeo-common = "0.5"`, but NOT `grafeo-engine`. The grafeo umbrella crate (`grafeo-0.5.42/src/lib.rs`) does NOT re-export the `transaction` module, so `grafeo_engine::transaction::IsolationLevel::Serializable` is unreachable from grafeo-loro.

**Why this matters**: If Q3 resolution (c) Serializable is adopted, L3 cannot call `session.begin_transaction_with_isolation(grafeo_engine::transaction::IsolationLevel::Serializable)` without adding `grafeo-engine = "0.5"` to `Cargo.toml`.

**Concrete fix (L2)**: Add to `Cargo.toml` `[dependencies]`:
```toml
# Direct dep for `grafeo_engine::transaction::IsolationLevel` (used by
# `sync_tree_move_to_grafeo` for Serializable isolation — see P2T2-DEVIL Q3).
# Already loaded transitively via grafeo, so this adds no new code to the build graph.
grafeo-engine = "0.5"
```

If Q3 fallback (a) is adopted instead, this dep is NOT needed — skip this fix.

---

## 3. Findings — full severity list

### BLOCKER (0)

None. L1's scaffolding is structurally sound; all 13 API citations verify (with 2 NIT-level off-by-one drift); compile/test claims are 100% accurate; the skeleton compiles and 17/17 tests pass. No hallucination, no Goodhart, no happy-path bias. L1's verification bar is HIGH — comparable to the Phase 1 Devil's depth standard.

### MAJOR (5)

- **M1** (Q1 + Q3): Edge direction contradiction — L1 followed the broken `apply_tree_move` (child→parent) instead of the architecture doc spec (parent→child). The skeleton's TODO comments at `src/schema/tree.rs:54-59` and `would_create_cycle` doc-comment at `src/schema/tree.rs:74-76` propagate the wrong direction. L2 must update both to parent→child AND flip `would_create_cycle` to walk `get_neighbors_incoming` instead of `get_neighbors_outgoing_by_type`.
- **M2** (Q3): Skeleton prescribes "Re-verify acyclicity post-commit" (option b) at `src/schema/tree.rs:63-64` — Devil rejects this as too late. L2 must replace with either Serializable isolation (option c) or inside-tx re-check (option a).
- **M3** (Q3): `Cargo.toml` lacks `grafeo-engine` direct dep, blocking option (c). L2 must add it IF (c) is adopted.
- **M4** (NEW M1): `would_create_cycle` helper signature `db: &GrafeoDB` cannot be used for inside-tx re-check (opens nested tx / can't see uncommitted writes). L2 must split into precheck + in-tx variants OR adopt Q3 (c) Serializable (which needs only the precheck variant).
- **M5** (NEW M2): Test scaffold `tree_move_root_to_leaf_rejected` is mis-named and asserts the wrong contract under Q2's best-effort semantics. L2 must rename or repurpose per §2.M2's Option A or B.

### MINOR (6)

- **m1** (Q2): Test scaffold `tree_move_root_to_leaf_rejected` body comment at `tests/unit/tree_move.rs:71` says "root has no parent edge — assert Err returned" — under Q2 best-effort semantics, this is wrong (no-parent-edge is NOT a rejection). L2 must update the comment to assert `TreeMoveCreatesCycle`.
- **m2** (Q4): Skeleton TODO at `src/schema/tree.rs:57-58` puts the noop guard INSIDE the tx ("If new_parent != old_parent (idempotent-noop guard), session.create_edge(...)"). Per Q4 resolution, the noop guard should be BEFORE the tx is opened. L2 must move the guard.
- **m3** (Q5): `apply_tree_move` at `src/bridge/grafeo_tx.rs:200,204,206` uses literal `"CHILD"` instead of `TREE_EDGE_LABEL`. In-scope for L2 — 3-line refactor.
- **m4** (NEW M4): Missing test scaffolds for "unknown node_id", "unknown new_parent", and "direct self-loop" contracts. L2 must add 3 scaffolds per §2.M4.
- **m5** (Q7): Implementation plan hidden gap — `LoroOp::TreeMove` bridge wiring is not scheduled in any phase. L2 must add a "Known Limitation" note to `src/schema/tree.rs` module doc-comment.
- **m6** (NEW): Implementation-plan.md line 46 stale claim "Grafeo enforces acyclic" — false per L1's verification. L2 must update to "Grafeo does NOT enforce acyclic; the bridge pre-checks via `would_create_cycle` (verified P2T2-L1)".

### NIT (5)

- **n1**: `PreparedCommit::set_metadata` citation drift — L1 cited `transaction/prepared.rs:108`, actual is line 107 (the `pub fn` signature line). Line 108 is the function body. L1's doc-comment at `src/schema/tree.rs:40` and worklog:717 carry the wrong citation. Fix: change `108` → `107`.
- **n2**: `Session::node_exists` citation drift — L1 worklog:716 says "around 5280", actual is `session/mod.rs:5278`. Acceptable approximation but worth tightening.
- **n3**: `Session::get_neighbors_outgoing_by_type` citation vague — L1 worklog:714 says "session/mod.rs (after 5237)", actual is `session/mod.rs:5256`. Replace "after 5237" with the precise `:5256`.
- **n4**: `tests/integration/tree_move_concurrency.rs:37` uses `let _ = (sync_tree_move_to_grafeo, GrafeoDB::new_in_memory);` to silence unused-import warnings — a hack. L3 should remove this line when filling in the test body. Document in the test's doc-comment.
- **n5**: `src/schema/tree.rs:48` uses `let _ = (db, node_id, old_parent, new_parent);` to silence unused-variable warnings — same hack. L3 should remove when filling in the body.

### RESOLUTION (7) — one per L1 open question

- **R1 (Q1)**: Parent→child is canonical. Fix both `apply_tree_move` and the L1 skeleton to use parent→child. Update `would_create_cycle` to walk `get_neighbors_incoming`.
- **R2 (Q2)**: Pin error variant as `TreeMoveCreatesCycle`. Best-effort delete semantics (no rejection for "no parent edge"). Rename/repurpose `tree_move_root_to_leaf_rejected` test.
- **R3 (Q3)**: Option (c) `begin_transaction_with_isolation(Serializable)` — requires adding `grafeo-engine = "0.5"` direct dep. Fallback (a) inside-tx re-check if dep rejected. Reject (b) post-commit audit.
- **R4 (Q4)**: Short-circuit noop BEFORE opening tx, AFTER cycle pre-check. Order: pre-check → noop guard → open tx → delete → insert → commit.
- **R5 (Q5)**: IN-SCOPE for L2. Refactor `apply_tree_move` 3 literal `"CHILD"` → `TREE_EDGE_LABEL`.
- **R6 (Q6)**: KEEP the `set_metadata` call. Defensive consistency with `batcher.rs:193-196`. No action needed.
- **R7 (Q7)**: Scope boundary IS correct. Implementation plan has hidden gap (no phase schedules `LoroOp::TreeMove` bridge wiring). File follow-up note in `src/schema/tree.rs` module doc-comment.

---

## 4. Cross-phase coupling check (Task 9)

**Question**: Does L1's P2T2-L1 skeleton accidentally block or conflict with P2 Task 3 (`VertexBuilder`)?

**Answer**: **No conflict.**

**Verification**:
- `VertexBuilder` is declared at `src/app.rs:122-143` with 3 methods: `with_label`, `with_property`, `commit`. None reference `TREE_EDGE_LABEL` or any tree-related concept.
- `VertexBuilder::commit` returns `Result<NodeId>` — it allocates a new node via `session.create_node(...)` (verified at `session/mod.rs:4860` — `pub fn create_node(&self, labels: &[&str]) -> NodeId`, infallible). No edge creation in the current VertexBuilder contract.
- A FUTURE `.with_parent(parent_key)` or `.with_child(child_key)` method on VertexBuilder would use `TREE_EDGE_LABEL` — but that's not in the current `src/app.rs:122-143` contract, and the implementation plan §Phase 2 Task 3 (`docs/implementation-plan.md:47-49`) lists only "Accumulate labels/properties" and "commit(): Generate NodeId, write Loro + Grafeo atomically". No edge creation in scope.
- The `TREE_EDGE_LABEL` constant is in `src/constants.rs:16`, accessible from any module. VertexBuilder can use it without coupling to `schema::tree`.

**Conclusion**: L1's `TREE_EDGE_LABEL` constant, `TreeMoveCreatesCycle` error variant, and `would_create_cycle` helper are all local to `schema::tree` and don't touch `app::VertexBuilder`. Task 3 L1 is unblocked.

**One observation (informational, not a finding)**: if Q1 resolution is adopted (flip direction to parent→child), AND a future VertexBuilder `.with_parent()` method is added, that method should use the same parent→child direction. The `TREE_EDGE_LABEL` constant is direction-agnostic (just the label string); the direction is enforced at call sites. L1's doc-comment at `src/constants.rs:12-15` correctly notes this: "direction is enforced at call sites".

---

## 5. Architecture doc alignment check (Task 8)

**Question**: Did L1's changes (new constant, new error variant, new helper signature) align with `docs/grafeo-loro.architecture.md` §7 and §5? Or did L1 introduce new concepts not in the doc that need to be documented?

**Answer**: **Mostly aligned, with 3 doc-drift issues for L2 to fix.**

### Aligned (no action)

- `TREE_EDGE_LABEL: &str = "CHILD"` (`src/constants.rs:16`) — matches arch doc §7 lines 259, 265 (`[:CHILD]`). ✅
- `GrafeoLoroError::TreeMoveCreatesCycle { node_id, new_parent }` (`src/error.rs:33-44`) — arch doc §7 doesn't specify an error variant, but the variant is consistent with the §7 line 249 claim that "Loro's LoroTree enforces an acyclic graph internally" (the bridge mirrors this on the grafeo side). ✅
- `would_create_cycle(db, node_id, new_parent) -> bool` (`src/schema/tree.rs:84`) — arch doc §7 doesn't mention a cycle-check helper, but the function is implied by §7 line 249's acyclicity claim (the bridge must enforce what LoroTree enforces natively). ✅
- `sync_tree_move_to_grafeo(db, node_id, old_parent, new_parent) -> Result<()>` signature — matches arch doc §7 line 254 pseudocode signature `fn sync_tree_move_to_grafeo(db: &grafeo::GrafeoDB, node_id: u64, old_parent: u64, new_parent: u64)`. The `NodeId` vs `u64` difference is fine — `NodeId` is a `u64` newtype (`src/types/ids.rs:10` re-exports `grafeo::NodeId`). ✅

### Doc-drift issues for L2 to fix

- **D1 (MAJOR, tied to M1)**: Arch doc §7 lines 259, 265 use parent→child direction; L1 skeleton uses child→parent. L2 must EITHER update the skeleton to parent→child (preferred per Q1) OR update the arch doc to child→parent (NOT preferred — breaks conventional reading and Loro's `LoroTree` semantics).
- **D2 (MINOR, tied to m6)**: `docs/implementation-plan.md:46` says "Return error if cycle detected (Grafeo enforces acyclic)" — false per L1's verification. L2 must update to "Return error if cycle detected (Grafeo does NOT enforce acyclic — bridge pre-checks via `would_create_cycle`; verified P2T2-L1)".
- **D3 (MINOR, tied to m5)**: Arch doc §7 line 250 says "LoroGrafeoBridge catches tree move events, translates them to Grafeo-compliant transaction mutations" — but `translate_diff_event` doesn't generate `LoroOp::TreeMove`. The arch doc implies bridge wiring exists; the code doesn't have it. L2 must add a "Known Limitation" note to §7 (or to `src/schema/tree.rs` module doc-comment per Q7 resolution) noting that bridge wiring is unscheduled.

---

## 6. L2 must-address list (actionable)

Ordered by priority (BLOCKER → MAJOR → MINOR → NIT). L2 (Fixer) should address all BLOCKERs and MAJORs; MINORs are recommended; NITs are defer-able to hunter.

### BLOCKER (0)

None.

### MAJOR (5) — L2 must fix

1. **M1 / R1 (Q1)**: Flip edge direction to parent→child in BOTH `apply_tree_move` (`src/bridge/grafeo_tx.rs:200,204,206`) AND the L1 skeleton TODO comments (`src/schema/tree.rs:54-59, 74-76, 86-87`). Update `would_create_cycle` to walk `get_neighbors_incoming` instead of `get_neighbors_outgoing_by_type`.
2. **M2 / R3 (Q3)**: Replace "Re-verify acyclicity post-commit" TODO at `src/schema/tree.rs:63-64` with either (preferred) `session.begin_transaction_with_isolation(grafeo_engine::transaction::IsolationLevel::Serializable)?;` or (fallback) inside-tx post-insert pre-commit re-check via `would_create_cycle_in_tx(session, ...)`. Reject option (b).
3. **M3 / R3 (Q3)**: If Q3 (c) Serializable adopted, add `grafeo-engine = "0.5"` to `Cargo.toml` `[dependencies]` with the comment in §2.M5.
4. **M4 (NEW M1)**: Split `would_create_cycle` into precheck (`db: &GrafeoDB`) + in-tx (`session: &Session`) variants. If Q3 (c) adopted, only the precheck variant is needed.
5. **M5 (NEW M2) / R2 (Q2)**: Rename or repurpose `tree_move_root_to_leaf_rejected` test scaffold per §2.M2 Option A (rename to `tree_move_root_to_descendant_rejected_as_cycle`) or Option B (repurpose to `tree_move_root_reparented_to_other_tree_ok`).

### MINOR (6) — L2 should fix

6. **m1**: Update `tests/unit/tree_move.rs:71` body comment to assert `matches!(err, GrafeoLoroError::TreeMoveCreatesCycle { .. })` (matching `tree_move_cycle_rejected` pattern).
7. **m2 / R4 (Q4)**: Move noop guard `if new_parent == old_parent { return Ok(()); }` to BEFORE the tx-open TODO in `src/schema/tree.rs:49-52`. Remove the in-tx noop guard clause from `src/schema/tree.rs:57-58`.
8. **m3 / R5 (Q5)**: Refactor `src/bridge/grafeo_tx.rs:200,204,206` literal `"CHILD"` → `TREE_EDGE_LABEL`. Add `use crate::constants::TREE_EDGE_LABEL;` import.
9. **m4 (NEW M4)**: Add 3 test scaffolds: `tree_move_unknown_node_rejected`, `tree_move_unknown_new_parent_rejected`, `tree_move_to_self_direct_cycle_rejected`. Per §2.M4.
10. **m5 / R7 (Q7)**: Add "Known Limitation" note to `src/schema/tree.rs` module doc-comment about no production caller / unscheduled bridge wiring.
11. **m6 (NEW)**: Update `docs/implementation-plan.md:46` to reflect grafeo's lack of native acyclicity enforcement.

### NIT (5) — L2 may fix or defer to hunter

12. **n1**: Fix `PreparedCommit::set_metadata` citation in `src/schema/tree.rs:40` from `transaction/prepared.rs:108` → `transaction/prepared.rs:107`.
13. **n2**: Tighten `Session::node_exists` citation in worklog (informational — worklog is append-only, no fix needed in source).
14. **n3**: Tighten `Session::get_neighbors_outgoing_by_type` citation in `src/schema/tree.rs:38` from "session/mod.rs (after 5237; for cycle BFS)" → "session/mod.rs:5256".
15. **n4**: Add a doc-comment line to `tests/integration/tree_move_concurrency.rs:36-37` noting the `let _ = (...)` line is a warning-silencer that L3 must remove when filling the body.
16. **n5**: Same for `src/schema/tree.rs:48` `let _ = (db, node_id, old_parent, new_parent);`.

---

## 7. Anti-plenger audit (self-applied to this critique)

- **Pure Functions (Zero Side Effects)**: this critique is reproducible — every claim cites a file:line, every verification command is listed, every fix is concrete code.
- **DRY/SSOT**: cited architecture doc as the SSOT for edge direction (§0.6); cited grafeo source as SSOT for API signatures (§0.2). No re-derivation.
- **YAGNI**: did NOT propose scope expansions (e.g., did NOT propose a new `TreeMoveValidator` trait, did NOT propose a generic `CycleDetector` interface). All fixes are minimal.
- **Performance & Security**: identified SI write-skew as a security/correctness issue (§1.Q3); recommended Serializable as the robust defense.
- **High Cohesion, Loose Coupling**: `would_create_cycle` split (§2.M1) keeps the precheck (db-level) and in-tx (session-level) concerns separated.
- **Immutability**: no mutable global state proposed.
- **Polymorphism Over Conditionals**: N/A (no new conditionals introduced).
- **Observability**: recommended log-warn for "edge already absent" in best-effort delete (§1.Q2).
- **Absolute Idempotency**: short-circuit noop guard (§1.Q4) guarantees idempotency without relying on delete+insert being perfectly idempotent.
- **Same logic, fewest code/LOC possible**: all proposed fixes are minimal-line.
- **Deletion over addition**: where possible, recommended deletion (e.g., remove "Re-verify acyclicity post-commit" TODO rather than adding a new audit step).
- **Native-first**: recommended grafeo's native `IsolationLevel::Serializable` over a custom cycle-lock.
- **Oneline code first, oneline doc only**: cited file:line, not block-quoted source.
- **Never simplify the basics and explicit requests**: addressed all 7 L1 open questions + 9 verification tasks + cross-phase coupling + arch doc alignment.

---

## 8. Final verdict

L1's P2T2-L1 scaffolding is **structurally sound and verification-rigorous** — zero hallucinations, zero Goodhart risks, compile/test claims 100% accurate. The 7 open questions are well-formed and correctly deferred to Devil. The 5 new findings (M1-M5) are mostly consequences of the 7 open questions (edge direction, race semantics, helper signature) rather than independent oversights.

**L2 priority order**:
1. Resolve edge direction (M1/R1) — affects 4 files, foundational.
2. Resolve isolation strategy (M2/M3/R3) — determines whether grafeo-engine dep is needed.
3. Split cycle-check helper (M4) — depends on (2).
4. Fix test scaffold naming/assertion (M5/R2/m1) — depends on (1) and (2).
5. Add missing test scaffolds (m4) — independent.
6. Refactor `apply_tree_move` literal (m3/R5) — independent, trivial.
7. Update doc-drift (m5/m6/D2/D3) — independent.
8. Tighten citations (n1/n3) — independent, cosmetic.

**Critique artifact**: `docs/critiques/p2t2-l1-devil.md` (this file).
