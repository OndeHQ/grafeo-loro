# P7 Plenger Hunt Report (Gap-Closure — Publish-Ready Scan)

**Hunter**: plenger-hunter (Task ID G4)
**Scan range**: commits 13f19bf..a67fc1f (session 2 gap-closure: 16 commits — T1 + I12 + EncFuzz + config refactors + doc fixes)
**Date**: 2026-07-07
**Method**: incremental commits; rg-first; 2-query cap per anti-pattern.

## Scope Recap (16 gap-closure commits under review)

- `13b647b` P7-L2-A1: add `NotYetImplemented` + `InvalidEnvelope` error variants + serde_json dep
- `5bd5767` P7-L2-A4-D3: PresenceManager::new real stub + remove dead_code allow on room_id
- `29851c6` P7-L2-A3: implement parse_eph_envelope + build_eph_envelope (real %EPH wire format)
- `c1efa01` P7-L2-A2: replace 6 unimplemented!() with Err(NotYetImplemented) in app.rs
- `a2689c7` P7-L2-A2b: remove Default impl for AppConfig (force builder; 0 callers)
- `6f0bfc9` P7-L2-G: remove 7 stale NOTE comments (T1 no longer excluded)
- `a4ccbd2` P7-L2-F: fix 3 stale doc-comments (health.rs, metrics.rs, app.rs)
- `3cce1af` P7-L2-D1: refactor from_sync_engine_with_telemetry to AppTelemetryConfig struct
- `c6a449b` P7-L2-D2: refactor MutationBatcher::new to BatcherConfig struct
- `b31be3b` P7-L2-D4: update async_yields_async reasons to permanent design language
- `f5f0251` P7-L2-C: update deferred child-spans note
- `0fc1645` P7-L2-E: consolidate FuzzOp/FuzzValue into lib.rs, remove EncFuzz mirror types
- `5fa3886` P7-L2-B: implement I12 MVCC snapshot isolation invariant
- `6120275` P7-L2-M2: rewrite I15 tests for new %EPH wire format
- `646c2b2` P7-L2-fmt: apply rustfmt to prior P7-L2-A2/A3 commits
- `a67fc1f` P7-L2-F2: fix 3 stale doc-comment mentions of unimplemented!() in app.rs

