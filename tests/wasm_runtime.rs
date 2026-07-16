//! Issue #3 sub-issue 3 — WASM runtime tests.
//!
//! Tests the runtime fixes:
//!
//! 1. `runtime::now_ms()` returns a non-zero timestamp on native (and on
//!    `wasm32` with the `wasm` feature via `js_sys::Date::now()`; on `wasm32`
//!    without the `wasm` feature it returns `0` — documented fallback).
//! 2. `runtime::MemoryCeiling` enforces a byte budget with LRU eviction of
//!    idle docs under memory pressure.
//! 3. The RefCell borrow trap (`*d.borrow_mut() = d.borrow().saturating_sub(1)`)
//!    is demonstrated — the broken pattern panics, the fixed pattern works.
//!
//! **File location note:** The task spec said `tests/unit/wasm_runtime.rs`,
//! but Cargo's auto-discovery does NOT pick up `tests/unit/wasm_runtime.rs`
//! as a standalone test target when `tests/unit/main.rs` exists (the
//! `main.rs` filename claims the whole subdirectory as a single `unit` test
//! binary). To satisfy the workflow's `cargo test --test wasm_runtime`
//! invocation, this file lives at the top-level `tests/wasm_runtime.rs`
//! where Cargo auto-discovers it as the `wasm_runtime` test target. The
//! orchestrator can move it under `tests/unit/` and add a `mod wasm_runtime;`
//! line to `tests/unit/main.rs` if they want it bundled with the other
//! unit tests instead.
//!
//! Run with:
//!
//! ```sh
//! cargo test --no-default-features --features bridge,compression,wasm \
//!     --test wasm_runtime
//! ```

#![allow(missing_docs)]

/// `now_ms()` returns a non-zero wall-clock timestamp on native targets.
///
/// On wasm32 with the `wasm` feature it would also be non-zero via
/// `js_sys::Date::now()`. On wasm32 without the `wasm` feature it returns 0
/// (documented fallback — caller must enable `wasm` for a real clock).
#[cfg(feature = "bridge")]
#[test]
fn now_ms_returns_nonzero_on_native() {
    use grafeo_loro::runtime::now_ms;
    let t = now_ms();

    #[cfg(not(target_family = "wasm"))]
    {
        // Native: SystemTime::now() since UNIX_EPOCH — non-zero post-1970.
        assert!(t > 0, "native now_ms must be non-zero post-1970; got {t}");
        // Sanity: should be a reasonable 21st-century timestamp.
        // 1_704_067_200_000 = 2024-01-01 00:00:00 UTC.
        assert!(
            t >= 1_704_067_200_000,
            "now_ms sanity: expected ≥2024 timestamp, got {t}"
        );
    }
    #[cfg(all(target_family = "wasm", feature = "wasm"))]
    {
        assert!(t > 0, "wasm with wasm feature: now_ms must be non-zero; got {t}");
    }
    #[cfg(all(target_family = "wasm", not(feature = "wasm")))]
    {
        assert_eq!(
            t, 0,
            "wasm without wasm feature: now_ms must fall back to 0 (got {t})"
        );
    }
}

/// `now_ms()` is monotonic-ish: calling it twice in quick succession yields
/// a non-decreasing value (wall clock can leap forward, never backward —
/// modulo NTP adjustments, which we don't try to defend against here).
#[cfg(feature = "bridge")]
#[test]
fn now_ms_is_monotonic_non_decreasing() {
    use grafeo_loro::runtime::now_ms;
    let t1 = now_ms();
    // Spin briefly to ensure the clock advances (or stays the same).
    std::hint::black_box(t1);
    let t2 = now_ms();
    // On native this is essentially guaranteed. On wasm32 with `wasm`, the
    // JS `Date.now()` has 1ms resolution; same instant → equal is OK.
    assert!(t2 >= t1, "now_ms non-decreasing: t1={t1}, t2={t2}");
}

/// `MemoryCeiling::DEFAULT_BUDGET` is 80 MB (issue #3 sub-issue 3 mandate).
#[cfg(feature = "bridge")]
#[test]
fn memory_ceiling_default_budget_is_80mb() {
    use grafeo_loro::runtime::MemoryCeiling;
    assert_eq!(MemoryCeiling::DEFAULT_BUDGET, 80 * 1024 * 1024);
    assert_eq!(MemoryCeiling::default().budget_bytes(), 80 * 1024 * 1024);
}

/// `MemoryCeiling` basic alloc/release cycle: try_alloc succeeds within
/// budget, current_usage reflects the alloc, release brings it back to 0.
/// Over-budget alloc (with no idle docs) fails without mutating state.
#[cfg(feature = "bridge")]
#[test]
fn memory_ceiling_alloc_release_cycle() {
    use grafeo_loro::runtime::MemoryCeiling;
    let ceiling = MemoryCeiling::new(1024);
    assert_eq!(ceiling.budget_bytes(), 1024);
    assert_eq!(ceiling.current_usage(), 0);

    assert!(ceiling.try_alloc(512));
    assert_eq!(ceiling.current_usage(), 512);

    assert!(ceiling.try_alloc(512));
    assert_eq!(ceiling.current_usage(), 1024);

    // Over-budget alloc must fail (no idle docs to evict).
    assert!(!ceiling.try_alloc(1));
    assert_eq!(
        ceiling.current_usage(),
        1024,
        "failed alloc must not mutate state"
    );

    ceiling.release(512);
    assert_eq!(ceiling.current_usage(), 512);
    ceiling.release(512);
    assert_eq!(ceiling.current_usage(), 0);
}

/// `MemoryCeiling` eviction under pressure: register idle docs, then trigger
/// eviction by over-allocating. Verify the LRU policy (oldest `last_access_ms`
/// evicted first).
#[cfg(feature = "bridge")]
#[test]
fn memory_ceiling_evicts_idle_under_pressure_lru() {
    use grafeo_loro::runtime::MemoryCeiling;
    let ceiling = MemoryCeiling::new(1000);

    // Three docs allocated, totaling 900 bytes (under budget).
    assert!(ceiling.try_alloc(300));
    assert!(ceiling.try_alloc(300));
    assert!(ceiling.try_alloc(300));
    assert_eq!(ceiling.current_usage(), 900);

    // Register them as idle with distinct last-access timestamps.
    // Doc 1 is oldest (LRU candidate), doc 3 is newest.
    ceiling.register_idle(1, 300, 1_000);
    ceiling.register_idle(2, 300, 2_000);
    ceiling.register_idle(3, 300, 3_000);

    // Try to alloc 200 more — total would be 1100 > 1000 budget.
    // Eviction must remove doc 1 (LRU, 300 bytes) → 900-300=600 → 600+200=800 ≤ 1000. OK.
    let ok = ceiling.try_alloc(200);
    assert!(ok, "try_alloc(200) must succeed after evicting idle doc 1");
    assert_eq!(
        ceiling.current_usage(),
        800,
        "after eviction + alloc: 900 - 300 + 200 = 800"
    );
}

/// `MemoryCeiling::try_alloc` triggers multi-doc eviction in the slow path.
/// Verifies the internal `evict_to_fit` evicts enough LRU docs to fit the
/// requested allocation.
#[cfg(feature = "bridge")]
#[test]
fn memory_ceiling_try_alloc_evicts_multiple_to_fit() {
    use grafeo_loro::runtime::MemoryCeiling;
    let ceiling = MemoryCeiling::new(1000);

    // Allocate 3 * 300 = 900 bytes (under budget).
    assert!(ceiling.try_alloc(300));
    assert!(ceiling.try_alloc(300));
    assert!(ceiling.try_alloc(300));
    assert_eq!(ceiling.current_usage(), 900);

    // Register them as idle with distinct last-access timestamps.
    ceiling.register_idle(10, 300, 5_000); // oldest
    ceiling.register_idle(20, 300, 6_000);
    ceiling.register_idle(30, 300, 7_000); // newest

    // Try to alloc 700 more: 900 + 700 = 1600 > 1000 budget.
    // Need to free at least 600 bytes. LRU evicts doc 10 (300) → current=600,
    // 600+700=1300 > 1000, evict doc 20 (300) → current=300, 300+700=1000 ≤ 1000.
    // Stop. Now alloc 700 → current=1000.
    let ok = ceiling.try_alloc(700);
    assert!(ok, "try_alloc(700) must succeed after evicting 2 idle docs");
    assert_eq!(
        ceiling.current_usage(),
        1000,
        "after eviction + alloc: 900 - 600 + 700 = 1000"
    );
}

/// `MemoryCeiling::try_alloc` returns `false` when even evicting ALL idle docs
/// cannot free enough space for the requested allocation.
#[cfg(feature = "bridge")]
#[test]
fn memory_ceiling_try_alloc_fails_when_eviction_insufficient() {
    use grafeo_loro::runtime::MemoryCeiling;
    let ceiling = MemoryCeiling::new(1000);
    ceiling.try_alloc(500);
    ceiling.register_idle(99, 500, 1_000);

    // Try to alloc 600: 500+600=1100 > 1000. Evict doc 99 → current=0.
    // 0+600=600 ≤ 1000. OK — should succeed.
    assert!(ceiling.try_alloc(600));

    // Now try to alloc 1100 (more than budget): 600+1100=1700 > 1000.
    // No idle docs left → fail.
    assert!(
        !ceiling.try_alloc(1100),
        "try_alloc(>budget) must fail even after eviction"
    );
    assert_eq!(
        ceiling.current_usage(),
        600,
        "failed alloc must not mutate state"
    );
}

/// `MemoryCeiling::evict_idle` (public manual hook) evicts the SINGLE oldest
/// idle doc and returns its size. To evict multiple, the caller loops.
#[cfg(feature = "bridge")]
#[test]
fn memory_ceiling_evict_idle_single_lru() {
    use grafeo_loro::runtime::MemoryCeiling;
    let ceiling = MemoryCeiling::new(1000);
    ceiling.try_alloc(300);
    ceiling.try_alloc(300);
    ceiling.try_alloc(300);
    assert_eq!(ceiling.current_usage(), 900);

    // Register 3 idle docs with distinct last-access timestamps.
    ceiling.register_idle(10, 300, 5_000); // oldest (LRU)
    ceiling.register_idle(20, 300, 6_000);
    ceiling.register_idle(30, 300, 7_000); // newest

    // First evict_idle call: should evict doc 10 (oldest).
    let freed = ceiling.evict_idle();
    assert_eq!(freed, 300, "evict one doc (300 bytes)");
    assert_eq!(ceiling.current_usage(), 600, "900 - 300 = 600");

    // Second call: evicts doc 20.
    let freed = ceiling.evict_idle();
    assert_eq!(freed, 300, "evict second doc (300 bytes)");
    assert_eq!(ceiling.current_usage(), 300, "600 - 300 = 300");

    // Third call: evicts doc 30.
    let freed = ceiling.evict_idle();
    assert_eq!(freed, 300, "evict third doc (300 bytes)");
    assert_eq!(ceiling.current_usage(), 0, "300 - 300 = 0");

    // Fourth call: no more idle docs.
    let freed = ceiling.evict_idle();
    assert_eq!(freed, 0, "no more idle docs → 0 freed");
}

/// `MemoryCeiling::evict_idle` returns 0 when there are no idle docs.
#[cfg(feature = "bridge")]
#[test]
fn memory_ceiling_evict_idle_returns_zero_when_empty() {
    use grafeo_loro::runtime::MemoryCeiling;
    let ceiling = MemoryCeiling::new(1000);
    ceiling.try_alloc(500);
    let freed = ceiling.evict_idle();
    assert_eq!(freed, 0, "no idle docs registered → nothing to evict");
    assert_eq!(
        ceiling.current_usage(),
        500,
        "current_usage unchanged after no-op evict"
    );
}

/// `MemoryCeiling::register_idle` updates the entry when called with an
/// existing key (the "doc was idle, became active, became idle again" path).
#[cfg(feature = "bridge")]
#[test]
fn memory_ceiling_register_idle_updates_existing_key() {
    use grafeo_loro::runtime::MemoryCeiling;
    let ceiling = MemoryCeiling::new(1000);
    ceiling.try_alloc(200);
    ceiling.register_idle(42, 200, 1_000);

    // Re-register with updated size + last_access.
    ceiling.register_idle(42, 200, 9_000);

    // Evict: should evict the SINGLE doc with key=42 (200 bytes).
    let freed = ceiling.evict_idle();
    assert_eq!(freed, 200);
    assert_eq!(ceiling.current_usage(), 0);

    // No more idle docs.
    let freed = ceiling.evict_idle();
    assert_eq!(freed, 0, "re-register must replace, not append");
}

/// Issue #3 sub-issue 3: RefCell borrow trap fix demonstration.
///
/// The BROKEN pattern `*d.borrow_mut() = d.borrow().saturating_sub(1)` panics
/// at runtime because `borrow_mut()` is called WHILE `borrow()` is still
/// held (the temporary `Ref` is dropped at the END of the statement, AFTER
/// `borrow_mut` was already invoked — `RefCell` enforces borrow rules at
/// runtime and rejects the double-borrow).
#[test]
#[should_panic(expected = "already borrowed")]
fn refcell_borrow_trap_broken_pattern_panics() {
    use std::cell::RefCell;
    let d = RefCell::new(5u32);
    // BROKEN: borrow_mut while borrow is alive → runtime panic.
    *d.borrow_mut() = d.borrow().saturating_sub(1);
}

/// The FIXED pattern: extract the immutable borrow into a local variable
/// FIRST, drop it at the end of the statement, THEN call `borrow_mut()` in
/// a separate statement.
#[test]
fn refcell_borrow_trap_fixed_pattern_works() {
    use std::cell::RefCell;
    let d = RefCell::new(5u32);

    // FIXED: the immutable borrow ends at `;` BEFORE the mutable borrow starts.
    let cur = *d.borrow(); // `Ref<u32>` dropped here, releases the borrow.
    *d.borrow_mut() = cur.saturating_sub(1); // fresh mutable borrow.

    assert_eq!(*d.borrow(), 4, "fixed pattern must correctly decrement");
}
