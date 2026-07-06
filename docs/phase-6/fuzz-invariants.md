# Phase 6 — Fuzz Invariants (T5, L1 cheatsheet)

Checklist of consistency invariants the `consistency` fuzz target must verify after every random Loro op batch. Source: `docs/grafeo-loro.architecture.md` §§ 1, 8, 9, 11, 16, 19, 20, 21.

**Target**: `fuzz/fuzz_targets/consistency.rs`. L2 wires `FuzzInput` + op generator; L3 fills the invariant assertions.

---

## Invariants

- [ ] **I1 — Tree state parity**: After every op batch, the Grafeo tree (via `BridgeMaps` node map) contains exactly the same vertex set as the Loro `V` container. No missing, no extra vertices.

- [ ] **I2 — Edge state parity**: After every op batch, the Grafeo edge set (via `BridgeMaps` edge map) contains exactly the same edges as the Loro `E` container. Src/dst/label triple matches.

- [ ] **I3 — No panic on any op sequence**: Any sequence of valid Loro ops (insert, delete, move, update text, update property) must not cause `panic!`, `unwrap` failure, or `unreachable!` in `apply_loro_op`, `MutationBatcher::run`, or `parallel_hydrate_grafeo`.

- [ ] **I4 — Echo loop bounded**: After applying a bridge-originated op, the epoch side-channel set (`SyncEngine::bridge_origin_epochs`) never grows beyond `EPOCH_RETENTION + 1` entries. Pruning must occur each CDC poll cycle (architecture §9).

- [ ] **I5 — Origin filter symmetry**: An op committed under `ORIGIN_GRAFEO_BRIDGE` must NOT be re-applied to Grafeo on the next inbound cycle. An op committed under `ORIGIN_LORO_BRIDGE` must NOT be re-broadcast to Loro on the next outbound cycle.

- [ ] **I6 — Read-your-own-writes**: A synchronous local write via `GrafeoLoroApp::update_text` must be visible to a subsequent `GrafeoLoroApp::query` before the batcher's 100ms flush window elapses (architecture §21).

- [ ] **I7 — Snapshot idempotency**: Calling `GrafeoLoroApp::checkpoint(graph_id)` twice in succession must produce byte-identical `CompressedPayload::to_wire` output for the same Loro doc state (architecture §11).

- [ ] **I8 — Compression round-trip**: For any random Loro doc state, `CompressedPayload::compress_to_wire(bytes, strategy)` followed by `decompress_from_wire(wire)` must yield the original `bytes` for all three `CompressionType` variants (`None`, `Lz4`, `Zstd`).

- [ ] **I9 — Hydration determinism**: `parallel_hydrate_grafeo(&doc, &db)` called twice on the same `LoroDoc` snapshot must produce byte-identical GrafeoDB state (CSR layout, HNSW index, BM25 index). Rayon chunk ordering must not leak non-determinism (architecture §16).

- [ ] **I10 — Vector offload bypass**: `VectorOffloadManager::handle_text_update` must NEVER write the embedding vector into the Loro doc (architecture §17, "Embedding Property SSOT"). The Loro doc must contain only the original text property; the embedding must appear only in the GrafeoDB property store.

- [ ] **I11 — BridgeMaps bijectivity**: For every `Node`/`Edge` in GrafeoDB, there exists exactly one `loro_key` in `BridgeMaps` (and vice versa). No orphaned map entries, no unmapped Grafeo entities.

- [ ] **I12 — MVCC snapshot isolation**: A `GrafeoLoroApp::query` started at epoch `E` must observe a consistent snapshot even if concurrent inbound writes advance the epoch mid-query (architecture §19). No torn reads.

- [ ] **I13 — Batcher count invariant**: After `MutationBatcher::run` flushes a batch of size `N`, `inbound_event_count` must increment by exactly `N`, and the batcher's internal queue must be empty.

- [ ] **I14 — Tree move serializability**: `sync_tree_move_to_grafeo` under `IsolationLevel::Serializable` must never produce a cycle in the parent-child tree, regardless of concurrent move op ordering (architecture §7, §22).

- [ ] **I15 — Presence envelope integrity**: `build_eph_envelope(payload)` followed by `parse_eph_envelope(bytes)` must round-trip the `PresencePayload` exactly, AND must reject any non-`%EPH`-prefixed byte sequence with `GrafeoLoroError` (architecture §12).

---

## L2/L3 contract

- L2: Define `FuzzInput` (via `arbitrary::Arbitrary`) that yields a `Vec<LoroOp>` plus a `SsotMode` and `CompressionType`.
- L3: For each fuzz iteration, build a fresh `GrafeoLoroApp` (or reuse with reset), apply the op batch, then assert every invariant above. Failure → `panic!` (libfuzzer treats as crash).
- L3: Document which invariants are **checked every iteration** (I3, I11) vs **checked periodically** (I4, I7, I9) to keep per-iteration cost bounded.
