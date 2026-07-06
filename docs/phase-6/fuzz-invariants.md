# Phase 6 — Fuzz Invariants (T5, L2 SSOT)

Checklist of consistency invariants the `consistency` fuzz target must verify after every random Loro op batch. Source: `docs/grafeo-loro.architecture.md` §§ 1, 8, 9, 11, 16, 19, 20, 21.

**Target**: `fuzz/fuzz_targets/consistency.rs`. L2 wires `FuzzInput` + op generator skeleton; L3 fills the invariant assertions.

---

## Invariants

- [ ] **I1 — Tree state parity**: After every op batch, the Grafeo tree (via `BridgeMaps` node map) contains exactly the same vertex set as the Loro `V` container. No missing, no extra vertices.

- [ ] **I2 — Edge state parity**: After every op batch, the Grafeo edge set (via `BridgeMaps` edge map) contains exactly the same edges as the Loro `E` container. Src/dst/label triple matches.

- [ ] **I3a — No panic in `apply_loro_op`**: Any `LoroOp` sequence (`UpsertNode`, `UpsertEdge`, `DeleteNode`, `DeleteEdge`, `TreeMove`) must not panic inside `bridge::grafeo_tx::apply_loro_op`. Per Devil C5.2 (split from I3 for finer failure attribution).

- [ ] **I3b — No panic in `MutationBatcher::run`**: Any `LoroOp` sequence drained through the batcher must not panic inside `run` (including `prepared.commit()`). Per Devil C5.2.

- [ ] **I3c — No panic in `parallel_hydrate_grafeo`**: Any Loro doc state hydrated via rayon chunks must not panic inside `parallel_hydrate_grafeo` (including `VertexEntity::hydrate_map` errors — those must be `Result`, not panic). Per Devil C5.2.

- [ ] **I4 — Echo loop bounded**: After applying a bridge-originated op, the epoch side-channel set (`SyncEngine::bridge_origin_epochs`) never grows beyond `EPOCH_RETENTION + 1` entries. Pruning must occur each CDC poll cycle (architecture §9).

- [ ] **I5 — Origin filter symmetry**: An op committed under `ORIGIN_GRAFEO_BRIDGE` must NOT be re-applied to Grafeo on the next inbound cycle. An op committed under `ORIGIN_LORO_BRIDGE` must NOT be re-broadcast to Loro on the next outbound cycle.

- [ ] **I6 — Read-your-own-writes**: A synchronous local write via `GrafeoLoroApp::update_text` must be visible to a subsequent `GrafeoLoroApp::query` before the batcher's 100ms flush window elapses (architecture §21).

- [ ] **I7 — Snapshot idempotency**: Calling `GrafeoLoroApp::checkpoint(graph_id)` twice in succession must produce byte-identical `CompressedPayload::to_wire` output for the same Loro doc state (architecture §11).

- [ ] **I8 — Compression round-trip**: For any random Loro doc state, `CompressedPayload::compress_to_wire(bytes, strategy)` followed by `decompress_from_wire(wire)` must yield the original `bytes` for all three `CompressionType` variants (`None`, `Lz4`, `Zstd`).

- [ ] **I9 — Hydration determinism**: `parallel_hydrate_grafeo(&doc, &db)` called twice on the same `LoroDoc` snapshot must produce byte-identical GrafeoDB state (CSR layout, HNSW index, BM25 index). Rayon chunk ordering must not leak non-determinism (architecture §16).

- [ ] **I10 — Vector offload bypass**: `VectorOffloadManager::handle_text_update` must NEVER write the embedding vector into the Loro doc (architecture §17, "Embedding Property SSOT"). The Loro doc must contain only the original text property; the embedding must appear only in the GrafeoDB property store.

- [ ] **I11 — BridgeMaps bijectivity**: For every `Node`/`Edge` in GrafeoDB, there exists exactly one `loro_key` in `BridgeMaps` (and vice versa). No orphaned map entries, no unmapped Grafeo entities.

- [x] **I12 — MVCC snapshot isolation** (P7-L2-B): A read session pinned to epoch `E` via grafeo's `set_viewing_epoch(E)` MUST continue to observe the DB state as of `E` even after a concurrent writer commits a newer epoch `E'`. Clearing the override MUST then expose the new state. Architecture §19. Implemented directly against grafeo's `set_viewing_epoch` time-travel API (NOT via `GrafeoLoroApp::query` — that remains `Err(NotYetImplemented(...))` per Gap A.2). See `check_i12_mvcc_snapshot_isolation` in `fuzz/fuzz_targets/consistency.rs` + L1 plan §Gap B in `docs/phase-7/gap-closure-l1-plan.md`. Uses `grafeo::Value::Int64` (not the hallucinated `Integer` — Devil B1/CA.1).

- [x] **I13 — Batcher count invariant** (COVERED BY I3b): After `MutationBatcher::run` flushes a batch of size `N`, `inbound_event_count` must increment by exactly `N`, and the batcher's internal queue must be empty.

  > **Covered by I3b (P6-L2-FIX, Hunter Task 5b)**: A standalone `check_i13_batcher_count` fn was a tautology — the call site hardcoded the `batcher_buffer_is_empty` parameter to `true`, making the fn's `assert!(batcher_buffer_is_empty, ...)` reduce to `assert!(true)`. The fn + call site were removed per anti-plenger #11 (Deletion over addition). I3b (`check_i3b_no_panic_in_batcher_run`) covers the underlying behavior: it spawns `MutationBatcher::run`, feeds ops via channel, triggers shutdown, and asserts `JoinHandle::await` is `Ok`. If the batcher failed to drain its buffer, it would either panic (caught by I3b's JoinError assert) or hang (test timeouts).

- [ ] **I14 — Tree move serializability**: `sync_tree_move_to_grafeo` under `IsolationLevel::Serializable` must never produce a cycle in the parent-child tree, regardless of concurrent move op ordering (architecture §7, §22).

- [ ] **I15 — Presence envelope integrity**: `build_eph_envelope(payload)` followed by `parse_eph_envelope(bytes)` must round-trip the `PresencePayload` exactly, AND must reject any non-`%EPH`-prefixed byte sequence with `GrafeoLoroError` (architecture §12).

---

## L2/L3 contract

- **L2**: Define `FuzzInput` (via `arbitrary::Arbitrary`) that yields a `Vec<FuzzOp>` plus a `SsotMode` and `CompressionType`. Define `FuzzOp` enum mirroring `LoroOp` variants (`UpsertNode`, `UpsertEdge`, `DeleteNode`, `DeleteEdge`, `TreeMove`). Define 15 invariant check fn skeletons (one per I1..I15, with I3 split into I3a/b/c). All bodies are `// TODO: L3`.

- **L3**: For each fuzz iteration, build a fresh `GrafeoLoroApp` (or reuse with reset), apply the op batch, then assert every invariant above. Failure → `panic!` (libfuzzer treats as crash).

- **L3 — Per-iteration vs periodic cadence** (per Devil C5.3 + C5.6):
  - **Checked every iteration** (cheap, O(1) or O(n) over current state): I1, I2, I3a/b/c, I4, I11, I12, I15. (I13 was removed in P6-L2-FIX — covered by I3b.)
  - **Checked periodically** (expensive I/O or full re-hydration): I7, I9.
    - Concrete cadence:
      - I7 (snapshot idempotency): every 1000 iterations OR on the final iteration of each fuzz run (whichever comes first). Cost: ~10-50ms per check.
      - I9 (hydration determinism): every 1000 iterations OR on the final iteration. Cost: ~50-200ms per check (full re-hydration + byte-compare).
  - **Checked only when the relevant op fires** (event-driven, not iteration-cadence): I5, I6, I8, I10, I14.

- **L3 — Non-trivial assertion guard** (per Devil M5):
  - Each invariant assertion must be NON-TRIVIAL — it must fail if the invariant is violated.
  - A `panic!` in the assertion is the only acceptable failure mode (libfuzzer treats as crash).
  - DO NOT use `assert!(result.is_ok())` as a substitute for invariant checks — that only catches `Result::Err`, not semantic violations (e.g., wrong vertex count).
  - Each `assert!` must compare two concrete values (e.g., `assert_eq!(grafeo_count, loro_count)`).

- **L3 — Malformed-input handling** (per Devil happy-path bias note):
  - If `FuzzInput::arbitrary` returns `Err` (malformed bytes), the fuzz target should `return` early (not panic) — libfuzzer treats early-return as a successful iteration, which is correct for malformed inputs.

- **L3 — Seed corpus** (per Devil M6):
  - `fuzz/corpus/consistency/` contains 5 seed files (populated by the `gen_corpus` binary at `fuzz/fuzz_targets/gen_corpus.rs`):
    1. `empty.bin` — empty op batch (tests I3a on no-op path)
    2. `single_upsert.bin` — one UpsertNode
    3. `all_variants.bin` — one of each LoroOp variant
    4. `cycle_attempt.bin` — TreeMove that would create a cycle (tests I14)
    5. `large_batch.bin` — 256 ops (tests I3b batcher-drain path — I13 was a tautology and removed in P6-L2-FIX; I3b covers the behavior)
  - **Regeneration** (idempotent per anti-plenger #9 — identical SHA256 on re-run):
    ```text,ignore
    cargo run --bin gen_corpus --manifest-path fuzz/Cargo.toml
    ```
  - **Encoding** (per orchestrator Q5 ruling — RAW ARBITRARY): bytes are written in the order `arbitrary::Arbitrary` reads them via `Unstructured` (u64/u32/u16 LE, u8 raw, `Vec<T>` with trailing length byte for `arbitrary_len`, `String` as u32-LE length + UTF-8). The decoded `FuzzInput` may differ slightly from the intended scenario if `arbitrary`'s internal encoding differs — the bytes are still valid fuzzer input (cargo-fuzz mutates from them regardless). See `gen_corpus.rs` module doc-comment for the full encoding rationale.
