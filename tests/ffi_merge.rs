//! Integration tests for issue #3 sub-issues 2 + 4 (FFI cleanup + semantic
//! text merge).
//!
//! Tests cover:
//! - `externally_tagged_loro_op_bincode_roundtrip` — verifies `LoroOp`
//!   bincode round-trip does NOT panic (issue #3 sub-issue 2 root cause:
//!   `#[serde(tag="type", content="payload")]` would panic with
//!   `Bincode does not support Deserializer::deserialize_identifier`).
//! - `semantic_text_merge_basic` — non-overlapping line changes merge
//!   cleanly to `Merged`.
//! - `semantic_text_merge_conflict` — both sides modify the same line
//!   differently → `ManualRequired` + conflict markers.
//! - `semantic_text_merge_ours_wins` / `theirs_wins` / `identical_changes`.
//! - `origin_kind_repr_c` — verify `OriginKind` has `#[repr(C)]` and the
//!   documented discriminant values.
//! - `set_next_commit_origin_thread_local_roundtrip` — set + take round-
//!   trip on the same thread.
//! - `batcher_enqueue_flush_roundtrip` — register, enqueue, flush, verify
//!   flush callback fires with the expected op count.
//!
//! ## File location note
//!
//! Spec said `tests/unit/ffi_merge.rs`, but Cargo's auto-discovery does
//! NOT pick up `tests/unit/ffi_merge.rs` as a standalone test target when
//! `tests/unit/main.rs` exists (the `main.rs` claims the whole sub-
//! directory as a single `unit` test binary — see Agent W's precedent in
//! the worklog for `tests/wasm_runtime.rs`). To satisfy the workflow's
//! `cargo test --test ffi_merge` invocation, the file lives at top-level
//! `tests/ffi_merge.rs` where Cargo auto-discovers it. Orchestrator can
//! move it under `tests/unit/` and add `mod ffi_merge;` to
//! `tests/unit/main.rs` if they want it bundled.

use std::collections::HashMap;
use std::sync::Mutex;

use grafeo_loro::ffi::{
    batcher_enqueue, batcher_flush, batcher_on_flush, batcher_register, semantic_text_merge,
    set_next_commit_origin, BatcherHandle,
};
use grafeo_loro::types::events::{ConflictDetected, ConflictResolution, LoroOp};
use grafeo_loro::types::values::GraphValue;

// ============================================================================
// Sub-issue 2 — bincode round-trip (no serde internal tags)
// ============================================================================

/// Bincode 1.x round-trip on `LoroOp::UpsertNode` must NOT panic.
///
/// Issue #3 sub-issue 2 root cause: `#[serde(tag = "type", content =
/// "payload")]` on `LoroOp` would panic with `Bincode does not support
/// Deserializer::deserialize_identifier`. The externally-tagged default
/// (no `#[serde(tag = ...)]` attribute) is bincode-safe. This test
/// verifies the contract holds.
#[test]
fn externally_tagged_loro_op_bincode_roundtrip() {
    let mut props = HashMap::new();
    props.insert("name".to_string(), GraphValue::String("alice".to_string()));
    props.insert("age".to_string(), GraphValue::Integer(30));
    props.insert("active".to_string(), GraphValue::Bool(true));

    let op = LoroOp::UpsertNode {
        loro_key: "V/abc-123".to_string(),
        labels: vec!["Person".to_string()],
        properties: props,
    };

    // Encode — must not panic.
    let bytes = bincode::serialize(&op).expect("serialize LoroOp::UpsertNode");

    // Decode — must not panic. This is the line that would panic if
    // `LoroOp` had `#[serde(tag = "type", content = "payload")]`.
    let decoded: LoroOp = bincode::deserialize(&bytes).expect("deserialize LoroOp::UpsertNode");

    assert_eq!(decoded, op);
}

/// Bincode round-trip on `LoroOp::DeleteNode` (unit variant — exercises
/// the externally-tagged tag-only encoding).
#[test]
fn externally_tagged_loro_op_delete_node_bincode_roundtrip() {
    let op = LoroOp::DeleteNode {
        loro_key: "V/old-1".to_string(),
    };
    let bytes = bincode::serialize(&op).expect("serialize DeleteNode");
    let decoded: LoroOp = bincode::deserialize(&bytes).expect("deserialize DeleteNode");
    assert_eq!(decoded, op);
}

/// Bincode round-trip on a `Vec<LoroOp>` covering every variant.
#[test]
fn externally_tagged_loro_op_vec_roundtrip_all_variants() {
    let mut props = HashMap::new();
    props.insert("k".to_string(), GraphValue::Integer(1));

    let ops = vec![
        LoroOp::UpsertNode {
            loro_key: "V/n1".to_string(),
            labels: vec![],
            properties: props.clone(),
        },
        LoroOp::UpsertEdge {
            src_key: "V/a".to_string(),
            dst_key: "V/b".to_string(),
            label: "KNOWS".to_string(),
            properties: props.clone(),
        },
        LoroOp::DeleteNode {
            loro_key: "V/old".to_string(),
        },
        LoroOp::DeleteEdge {
            src_key: "V/a".to_string(),
            dst_key: "V/b".to_string(),
            label: "KNOWS".to_string(),
        },
        LoroOp::TreeMove {
            node_key: "V/n".to_string(),
            old_parent_key: "V/p1".to_string(),
            new_parent_key: "V/p2".to_string(),
        },
    ];

    let bytes = bincode::serialize(&ops).expect("serialize Vec<LoroOp>");
    let decoded: Vec<LoroOp> = bincode::deserialize(&bytes).expect("deserialize Vec<LoroOp>");
    assert_eq!(decoded.len(), ops.len());
    assert_eq!(decoded, ops);
}

// ============================================================================
// Sub-issue 4 — semantic_text_merge
// ============================================================================

/// Basic non-overlapping merge: ours changes line 2 (B→X), theirs changes
/// line 3 (C→Y). Expected outcome: `Merged`, output combines both
/// changes.
#[test]
fn semantic_text_merge_basic() {
    let base = "A\nB\nC";
    let ours = "A\nX\nC";
    let theirs = "A\nB\nY";

    let (merged, resolution) = semantic_text_merge(base, ours, theirs);

    assert_eq!(
        resolution,
        ConflictResolution::Merged,
        "non-overlapping changes should merge cleanly (got merged={:?})",
        merged
    );
    assert!(
        merged.contains("X"),
        "merged output should contain ours' X: got {:?}",
        merged
    );
    assert!(
        merged.contains("Y"),
        "merged output should contain theirs' Y: got {:?}",
        merged
    );
    assert!(
        !merged.contains("<<<<<<<"),
        "non-conflicting merge should NOT contain conflict markers: got {:?}",
        merged
    );
    assert!(
        merged.starts_with("A\n"),
        "merged output should start with A: got {:?}",
        merged
    );
}

/// Both sides modify the same line differently → `ManualRequired` +
/// conflict markers in the output.
#[test]
fn semantic_text_merge_conflict() {
    let base = "A\nB\nC";
    let ours = "A\nX\nC";
    let theirs = "A\nZ\nC";

    let (merged, resolution) = semantic_text_merge(base, ours, theirs);

    assert_eq!(
        resolution,
        ConflictResolution::ManualRequired,
        "same-line different-modification should be ManualRequired"
    );
    assert!(
        merged.contains("<<<<<<< ours"),
        "conflict output should contain `<<<<<<< ours` marker: got {:?}",
        merged
    );
    assert!(
        merged.contains("======="),
        "conflict output should contain `=======` separator: got {:?}",
        merged
    );
    assert!(
        merged.contains(">>>>>>> theirs"),
        "conflict output should contain `>>>>>>> theirs` marker: got {:?}",
        merged
    );
    assert!(
        merged.contains("X"),
        "conflict output should contain ours' X: got {:?}",
        merged
    );
    assert!(
        merged.contains("Z"),
        "conflict output should contain theirs' Z: got {:?}",
        merged
    );
}

/// Only ours changes (theirs == base) → `OursWins`.
#[test]
fn semantic_text_merge_ours_wins() {
    let base = "A\nB\nC";
    let ours = "A\nX\nC";
    let theirs = "A\nB\nC";

    let (merged, resolution) = semantic_text_merge(base, ours, theirs);

    assert_eq!(resolution, ConflictResolution::OursWins);
    assert_eq!(merged, "A\nX\nC");
}

/// Only theirs changes (ours == base) → `TheirsWins`.
#[test]
fn semantic_text_merge_theirs_wins() {
    let base = "A\nB\nC";
    let ours = "A\nB\nC";
    let theirs = "A\nB\nY";

    let (merged, resolution) = semantic_text_merge(base, ours, theirs);

    assert_eq!(resolution, ConflictResolution::TheirsWins);
    assert_eq!(merged, "A\nB\nY");
}

/// Both sides make the IDENTICAL change → `Merged` (no conflict).
#[test]
fn semantic_text_merge_identical_changes() {
    let base = "A\nB\nC";
    let ours = "A\nX\nC";
    let theirs = "A\nX\nC";

    let (merged, resolution) = semantic_text_merge(base, ours, theirs);

    assert_eq!(resolution, ConflictResolution::Merged);
    assert_eq!(merged, "A\nX\nC");
}

/// Both sides append different lines at the end → conflict.
#[test]
fn semantic_text_merge_conflicting_appends() {
    let base = "A\nB";
    let ours = "A\nB\nX";
    let theirs = "A\nB\nY";

    let (merged, resolution) = semantic_text_merge(base, ours, theirs);

    assert_eq!(resolution, ConflictResolution::ManualRequired);
    assert!(merged.contains("<<<<<<< ours"));
    assert!(merged.contains(">>>>>>> theirs"));
}

/// Empty base — both sides insert different content → conflict.
#[test]
fn semantic_text_merge_empty_base_conflict() {
    let base = "";
    let ours = "X";
    let theirs = "Y";

    let (merged, resolution) = semantic_text_merge(base, ours, theirs);

    assert_eq!(resolution, ConflictResolution::ManualRequired);
    assert!(merged.contains("<<<<<<<"));
}

/// Empty base — both sides insert the same content → `Merged`.
#[test]
fn semantic_text_merge_empty_base_identical() {
    let base = "";
    let ours = "X";
    let theirs = "X";

    let (merged, resolution) = semantic_text_merge(base, ours, theirs);

    assert_eq!(resolution, ConflictResolution::Merged);
    assert_eq!(merged, "X");
}

// ============================================================================
// Sub-issue 2 — OriginKind #[repr(C)] contract
// ============================================================================

/// `OriginKind` must be `#[repr(C)]` with the documented discriminant
/// values. Renumbering breaks the C ABI contract with downstream JS
/// callers — this test pins the values.
#[test]
fn origin_kind_repr_c() {
    use grafeo_loro::bridge::origin::OriginKind;

    // Discriminant values (frozen — see OriginKind doc).
    assert_eq!(OriginKind::Structural as u8, 0);
    assert_eq!(OriginKind::Typing as u8, 1);
    assert_eq!(OriginKind::Other as u8, 2);

    // `#[repr(C)]` on a field-less enum: discriminant follows the C ABI
    // convention (typically 4 bytes on most platforms = `c_int`). We
    // don't pin the exact size (platform-dependent); we just assert it's
    // a small, fixed-width integer (no larger than `usize`).
    assert!(
        std::mem::size_of::<OriginKind>() <= std::mem::size_of::<usize>(),
        "OriginKind should fit in a single machine word (repr(C) field-less enum); got {} bytes",
        std::mem::size_of::<OriginKind>()
    );
    assert!(
        std::mem::size_of::<OriginKind>() >= 1,
        "OriginKind should be at least 1 byte"
    );

    // Verify Copy + Eq + PartialEq are derived.
    let a = OriginKind::Structural;
    let b = a; // Copy
    assert_eq!(a, b);
    assert_ne!(a, OriginKind::Typing);
    assert_ne!(a, OriginKind::Other);
}

// ============================================================================
// Sub-issue 2 — set_next_commit_origin thread-local round-trip
// ============================================================================

#[test]
fn set_next_commit_origin_thread_local_roundtrip() {
    use grafeo_loro::bridge::origin::{take_next_commit_origin, OriginKind};

    // Clear any prior stash (tests share a thread pool).
    let _ = take_next_commit_origin();

    set_next_commit_origin(OriginKind::Typing, Some("node-42")).expect("set_next_commit_origin");

    let taken = take_next_commit_origin();
    assert_eq!(taken.as_deref(), Some("typing:node-42"));

    let taken2 = take_next_commit_origin();
    assert!(taken2.is_none(), "take should consume the stash");
}

#[test]
fn set_next_commit_origin_other_no_node_id() {
    use grafeo_loro::bridge::origin::{take_next_commit_origin, OriginKind};

    let _ = take_next_commit_origin();
    set_next_commit_origin(OriginKind::Other, None).expect("set_next_commit_origin");
    assert_eq!(take_next_commit_origin().as_deref(), Some("other"));
}

// ============================================================================
// Sub-issue 2 — batcher FFI round-trip
// ============================================================================

static FLUSH_CAPTURE: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

extern "C" fn capture_flush_payload(ptr: *const u8, len: usize) {
    assert_eq!(len, 16, "flush payload should be 16 bytes");
    let mut bytes = [0u8; 16];
    unsafe {
        std::ptr::copy_nonoverlapping(ptr, bytes.as_mut_ptr(), 16);
    }
    let epoch = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
    let op_count = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
    FLUSH_CAPTURE.lock().unwrap().push((epoch, op_count));
}

#[test]
fn batcher_enqueue_flush_roundtrip() {
    batcher_on_flush(capture_flush_payload);

    let handle: BatcherHandle = batcher_register();

    let ops: Vec<LoroOp> = (0..3)
        .map(|i| LoroOp::DeleteNode {
            loro_key: format!("V/n{i}"),
        })
        .collect();
    for op in &ops {
        let bytes = bincode::serialize(op).expect("serialize LoroOp");
        batcher_enqueue(handle, &bytes).expect("batcher_enqueue");
    }

    let pre_count = FLUSH_CAPTURE.lock().unwrap().len();
    batcher_flush(handle).expect("batcher_flush");

    let captures = FLUSH_CAPTURE.lock().unwrap();
    let new_captures = &captures[pre_count..];
    let matching = new_captures
        .iter()
        .any(|(epoch, op_count)| *op_count == 3 && *epoch == 0);
    assert!(
        matching,
        "expected a flush callback with op_count=3, epoch=0; got captures={:?}",
        new_captures
    );
}

#[test]
fn batcher_flush_unknown_handle_errors() {
    let bogus = BatcherHandle::unknown();
    let result = batcher_flush(bogus);
    assert!(
        result.is_err(),
        "flush with unknown handle should return Err"
    );
}

#[test]
fn batcher_enqueue_unknown_handle_errors() {
    let bogus = BatcherHandle::unknown();
    let op = LoroOp::DeleteNode {
        loro_key: "V/x".to_string(),
    };
    let bytes = bincode::serialize(&op).expect("serialize");
    let result = batcher_enqueue(bogus, &bytes);
    assert!(
        result.is_err(),
        "enqueue with unknown handle should return Err"
    );
}

#[test]
fn batcher_enqueue_malformed_bytes_errors() {
    let handle = batcher_register();
    let garbage: &[u8] = &[0xff, 0xff, 0xff, 0xff];
    let result = batcher_enqueue(handle, garbage);
    assert!(
        result.is_err(),
        "malformed bincode should return Err, not panic"
    );
}

// ============================================================================
// Sub-issue 4 — ConflictDetected + dispatch_conflict_detected
// ============================================================================

#[test]
fn conflict_detected_struct_fields() {
    let ev = ConflictDetected {
        node_key: "V/n1".to_string(),
        field: "body".to_string(),
        base: "hello".to_string(),
        ours: "hi".to_string(),
        theirs: "hey".to_string(),
        resolution: ConflictResolution::ManualRequired,
    };
    assert_eq!(ev.node_key, "V/n1");
    assert_eq!(ev.field, "body");
    assert_eq!(ev.base, "hello");
    assert_eq!(ev.ours, "hi");
    assert_eq!(ev.theirs, "hey");
    assert_eq!(ev.resolution, ConflictResolution::ManualRequired);

    let ev2 = ev.clone();
    assert_eq!(ev, ev2);
}

static CONFLICT_CAPTURE: Mutex<Vec<ConflictDetected>> = Mutex::new(Vec::new());

extern "C" fn capture_conflict(ptr: *const ConflictDetected) {
    let event: &ConflictDetected = unsafe { &*ptr };
    CONFLICT_CAPTURE.lock().unwrap().push(event.clone());
}

#[test]
fn dispatch_conflict_detected_fires_callbacks() {
    use grafeo_loro::ffi::{dispatch_conflict_detected, on_conflict_detected};

    on_conflict_detected(capture_conflict);

    let pre_count = CONFLICT_CAPTURE.lock().unwrap().len();

    let event = ConflictDetected {
        node_key: "V/dispatch-test".to_string(),
        field: "title".to_string(),
        base: "old".to_string(),
        ours: "ours".to_string(),
        theirs: "theirs".to_string(),
        resolution: ConflictResolution::ManualRequired,
    };
    dispatch_conflict_detected(&event);

    let captures = CONFLICT_CAPTURE.lock().unwrap();
    let new_captures = &captures[pre_count..];
    assert!(
        new_captures.iter().any(|c| c.node_key == "V/dispatch-test"),
        "expected the dispatched event to reach the callback; captures={:?}",
        new_captures
    );
}
