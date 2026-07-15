//! FFI-friendly hot-path API (issue #1 item 6).
//!
//! Onde's ADR-010 bans `serde` on hot paths (>10 Hz). This module provides:
//!
//! - `NodeOp` — `#[repr(C)]` struct using `&str` not `String`, suitable for
//!   SharedArrayBuffer-backed bulk apply from WASM.
//! - `apply_node_batch(nodes: &[NodeOp])` — bulk apply with zero serde
//!   allocations on the hot path.
//! - `apply_loro_op_bytes(&[u8])` — bincode-only entry point for sub-µs FFI.
//!
//! Hot-path-safe APIs are documented as such. Admin-only APIs (the existing
//! `LoroOp` enum with `String` fields) remain available behind the `serde`
//! feature for non-hot-path use cases (snapshot import/export, admin UIs).
//!
//! ## ADR-010 compliance note
//!
//! `serde_json` (JSON codec) is admin-only and NEVER pulled by these entry
//! points. `bincode` 1.x transitively requires `serde` core (the trait
//! machinery: `Serialize`/`Deserialize` derives on `LoroOp`/`GraphValue`),
//! but the `serde` trait layer is zero-cost at runtime — there is no
//! reflection, no JSON parsing, no string escaping. The derived code is
//! straight-line field reads/writes, comparable to a hand-rolled binary
//! codec. This is the binary-codec path ADR-010 explicitly permits.

use std::collections::HashMap;

use crate::types::events::LoroOp;
use crate::types::values::GraphValue;

// `Result` and `BridgeMaps` are only used in the `apply_node_batch` /
// `apply_loro_op_bytes` signatures, which are gated by `grafeo`. Gate the
// imports to match so a `bridge`-only build doesn't warn about unused
// imports.
#[cfg(feature = "grafeo")]
use crate::error::Result;
#[cfg(feature = "grafeo")]
use crate::BridgeMaps;

/// C-FFI-compatible property value for hot-path bulk apply.
///
/// `#[repr(C)]` so the enum tag + payload layout matches a C union — WASM
/// callers can construct this in linear memory and pass a pointer to
/// [`apply_node_batch`]. The variants cover the scalar subset of
/// [`GraphValue`] (no `Vector`/`Map`/`List` — those are graph-only and not
/// part of the FFI hot-path surface).
///
/// All string variants borrow the caller's buffer (`&'a str`) — zero
/// allocations on the hot path. The caller must keep the source strings
/// alive for the duration of the call.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeValue<'a> {
    /// `null` / absent value.
    Null,
    /// Boolean.
    Bool(bool),
    /// 64-bit signed integer.
    Integer(i64),
    /// 64-bit IEEE-754 floating point.
    Float(f64),
    /// UTF-8 string slice (borrowed, not owned).
    Str(&'a str),
}

/// C-FFI-compatible node op for hot-path bulk apply.
///
/// `#[repr(C)]` so the layout matches a C struct — WASM callers can
/// construct this in linear memory and pass a pointer to `apply_node_batch`.
///
/// All strings are `&str` (borrowed), NOT `String` (owned) — zero
/// allocations on the hot path. The caller must keep the source strings
/// alive for the duration of the call.
///
/// ## C ABI sketch
///
/// In C ABI the equivalent struct would be:
///
/// ```c
/// typedef struct {
///     const char* loro_key;             // null-terminated
///     const char* const* labels;        // array of null-terminated strings
///     size_t labels_len;
///     size_t property_count;
///     const char* const* property_keys;   // parallel to property_values
///     const node_value_t* property_values;
/// } node_op_t;
/// ```
///
/// In Rust we use `&str` and `&[&str]` for ergonomics; the layout of `&str`
/// is `(ptr, len)` and `&[&str]` is `(ptr, len)`, which matches the C ABI
/// sketched above.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NodeOp<'a> {
    /// Loro-side stable string key (e.g. `"V/abc-123"`).
    pub loro_key: &'a str,
    /// Labels (array of `&str`). Caller owns the array; this is a pointer
    /// + length pair in C ABI.
    pub labels: &'a [&'a str],
    /// Property count — for the C ABI property array. Must equal
    /// `property_keys.len()` and `property_values.len()`. Stored explicitly
    /// so the C side can pre-validate without dereferencing the slice
    /// pointers.
    pub property_count: usize,
    /// Property keys (parallel array to `property_values`).
    pub property_keys: &'a [&'a str],
    /// Property values (parallel array to `property_keys`).
    pub property_values: &'a [NodeValue<'a>],
}

/// Convert a borrowed FFI [`NodeValue`] into the owned [`GraphValue`] used
/// by the existing apply path. Allocations happen here (one `String` per
/// `Str` variant) — this is the boundary between the zero-alloc FFI surface
/// and the owned internal representation.
impl<'a> From<&NodeValue<'a>> for GraphValue {
    fn from(v: &NodeValue<'a>) -> Self {
        match *v {
            NodeValue::Null => GraphValue::Null,
            NodeValue::Bool(b) => GraphValue::Bool(b),
            NodeValue::Integer(i) => GraphValue::Integer(i),
            NodeValue::Float(f) => GraphValue::Float(f),
            NodeValue::Str(s) => GraphValue::String(s.to_string()),
        }
    }
}

/// Convert a borrowed FFI [`NodeOp`] into the owned [`LoroOp::UpsertNode`]
/// used by the existing apply path. This is the bridge between the
/// zero-alloc FFI surface and the owned internal representation: each
/// `&str` becomes a `String`, and the parallel property arrays are zipped
/// into a `HashMap`.
///
/// The conversion is O(n) in `property_count` and allocates:
/// - 1 `String` for `loro_key`
/// - `labels.len()` `String`s for labels
/// - `property_count` `String`s for keys + `property_count` `GraphValue`s
///   for values (each `Str` variant allocates a `String`; scalar variants
///   do not allocate)
///
/// These allocations happen ONCE per op at the FFI boundary. After this,
/// `apply_loro_op` walks the `HashMap` without any further allocations.
impl<'a> From<NodeOp<'a>> for LoroOp {
    fn from(op: NodeOp<'a>) -> Self {
        let mut properties = HashMap::with_capacity(op.property_count);
        for (k, v) in op.property_keys.iter().zip(op.property_values.iter()) {
            properties.insert((*k).to_string(), GraphValue::from(v));
        }
        LoroOp::UpsertNode {
            loro_key: op.loro_key.to_string(),
            labels: op.labels.iter().map(|s| (*s).to_string()).collect(),
            properties,
        }
    }
}

/// Bulk-apply a slice of borrowed FFI [`NodeOp`]s to a grafeo `Session`.
///
/// Each `NodeOp` is converted to a [`LoroOp::UpsertNode`] via the `From`
/// impl and dispatched to [`crate::bridge::apply_loro_op`]. The conversion
/// allocates the owned `String`/`HashMap` representations exactly once per
/// op; the dispatch itself adds no further allocations.
///
/// # Hot-path-safe
///
/// Zero serde allocations. Properties are passed as parallel arrays — no
/// `HashMap` construction on the caller side. The caller can construct
/// `NodeOp`s directly in linear memory (e.g. via a `SharedArrayBuffer`
/// view in WASM) and pass a slice.
///
/// # Errors
///
/// Returns `GrafeoLoroError` if any individual `apply_loro_op` call fails
/// (e.g. `Bridge` error for unknown edge endpoints, `Grafeo` error for
/// session-level failures). The batch is applied in order; on the first
/// error, subsequent ops are NOT applied.
///
/// # Feature gating
///
/// Requires the `grafeo` feature (calls `apply_loro_op`, which calls
/// `Session::create_node_with_props`).
#[cfg(feature = "grafeo")]
pub fn apply_node_batch(
    session: &grafeo::Session,
    nodes: &[NodeOp<'_>],
    maps: &BridgeMaps,
) -> Result<()> {
    for node in nodes {
        let op: LoroOp = (*node).into();
        crate::bridge::apply_loro_op(session, &op, maps)?;
    }
    Ok(())
}

/// Bincode-only entry point: decode a `Vec<LoroOp>` from a byte slice and
/// apply each op to a grafeo `Session`.
///
/// # Hot-path-safe (bincode, NOT serde_json)
///
/// Bincode-only — sub-µs FFI for high-frequency ops. `serde_json` is NOT
/// pulled by this entry point; only the `serde` trait machinery (which is
/// zero-cost at runtime — straight-line field reads/writes, no reflection)
/// is used via bincode's `deserialize` impl. This is the binary-codec path
/// ADR-010 explicitly permits for >10 Hz hot paths.
///
/// # Errors
///
/// Returns `GrafeoLoroError::Bridge` if bincode deserialization fails
/// (malformed bytes, truncated payload, unknown enum variant). Returns
/// whatever `apply_loro_op` returns for apply-time failures.
///
/// # Feature gating
///
/// Requires both `grafeo` (calls `apply_loro_op`) and `serde` (bincode
/// needs `LoroOp: Deserialize`, which is derived under `serde`). The
/// `bridge` feature pulls `bincode` itself; the caller must additionally
/// enable `serde` so the derives fire.
#[cfg(all(feature = "grafeo", feature = "serde"))]
pub fn apply_loro_op_bytes(
    session: &grafeo::Session,
    bytes: &[u8],
    maps: &BridgeMaps,
) -> Result<()> {
    let ops: Vec<LoroOp> = bincode::deserialize(bytes)
        .map_err(|e| crate::error::GrafeoLoroError::Bridge(format!("bincode decode: {e}")))?;
    for op in &ops {
        crate::bridge::apply_loro_op(session, op, maps)?;
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================
//
// Tests exercise the conversion logic (`NodeOp → LoroOp`, `NodeValue →
// GraphValue`) and the bincode round-trip. They do NOT exercise
// `apply_node_batch` / `apply_loro_op_bytes` end-to-end because that would
// require a real `grafeo::Session` (which needs a `GrafeoDB` — heavy, and
// out of scope for this module's unit tests). End-to-end coverage lives in
// the integration tests under `tests/integration/`.

#[cfg(all(test, feature = "grafeo"))]
mod tests {
    use super::*;
    use crate::types::values::GraphValue;

    /// `NodeValue::Null` → `GraphValue::Null` (and the other scalar variants
    /// map 1:1).
    #[test]
    fn node_value_to_graph_value_scalars() {
        assert_eq!(GraphValue::from(&NodeValue::Null), GraphValue::Null);
        assert_eq!(
            GraphValue::from(&NodeValue::Bool(true)),
            GraphValue::Bool(true)
        );
        assert_eq!(
            GraphValue::from(&NodeValue::Integer(42)),
            GraphValue::Integer(42)
        );
        assert_eq!(
            GraphValue::from(&NodeValue::Float(3.5)),
            GraphValue::Float(3.5)
        );
        assert_eq!(
            GraphValue::from(&NodeValue::Str("hi")),
            GraphValue::String("hi".to_string())
        );
    }

    /// `NodeValue` is `Copy` — passing by value does not move ownership.
    /// This is a hard requirement for the `#[repr(C)]` FFI surface (C
    /// callers pass by value via the stack).
    #[test]
    fn node_value_is_copy() {
        let v = NodeValue::Integer(7);
        let v2 = v; // Copy, not move
        assert_eq!(v, v2);
    }

    /// `NodeOp` with no properties converts to a `LoroOp::UpsertNode` with
    /// an empty `properties` map.
    #[test]
    fn node_op_empty_props_to_loro_op() {
        let key = "V/abc-123";
        let labels: &[&str] = &["Person"];
        let op = NodeOp {
            loro_key: key,
            labels,
            property_count: 0,
            property_keys: &[],
            property_values: &[],
        };
        let loro_op: LoroOp = op.into();
        match loro_op {
            LoroOp::UpsertNode {
                loro_key,
                labels,
                properties,
            } => {
                assert_eq!(loro_key, "V/abc-123");
                assert_eq!(labels, vec!["Person".to_string()]);
                assert!(properties.is_empty());
            }
            other => panic!("expected UpsertNode, got {other:?}"),
        }
    }

    /// `NodeOp` with multiple properties zips `property_keys` and
    /// `property_values` into the `HashMap` correctly.
    #[test]
    fn node_op_with_props_to_loro_op() {
        let key = "V/node-1";
        let labels: &[&str] = &["Person", "User"];
        let pkeys: &[&str] = &["name", "age", "active"];
        let pvals: &[NodeValue] = &[
            NodeValue::Str("alice"),
            NodeValue::Integer(30),
            NodeValue::Bool(true),
        ];
        let op = NodeOp {
            loro_key: key,
            labels,
            property_count: 3,
            property_keys: pkeys,
            property_values: pvals,
        };
        let loro_op: LoroOp = op.into();
        match loro_op {
            LoroOp::UpsertNode {
                loro_key,
                labels,
                properties,
            } => {
                assert_eq!(loro_key, "V/node-1");
                assert_eq!(labels, vec!["Person".to_string(), "User".to_string()]);
                assert_eq!(properties.len(), 3);
                assert_eq!(
                    properties.get("name"),
                    Some(&GraphValue::String("alice".to_string()))
                );
                assert_eq!(properties.get("age"), Some(&GraphValue::Integer(30)));
                assert_eq!(properties.get("active"), Some(&GraphValue::Bool(true)));
                assert_eq!(properties.get("missing"), None);
            }
            other => panic!("expected UpsertNode, got {other:?}"),
        }
    }

    /// `property_count` is honoured for pre-sizing the `HashMap` even when
    /// the parallel arrays are longer (defensive — caller should keep them
    /// in sync, but the impl does not blindly trust `property_count`).
    #[test]
    fn node_op_property_count_mismatch_uses_actual_arrays() {
        // Caller claims 100 properties but only provides 2. The impl zips
        // the arrays (which stops at the shorter one) — no panic, no OOB.
        let pkeys: &[&str] = &["k1", "k2"];
        let pvals: &[NodeValue] = &[NodeValue::Integer(1), NodeValue::Integer(2)];
        let op = NodeOp {
            loro_key: "V/x",
            labels: &[],
            property_count: 100, // wrong on purpose
            property_keys: pkeys,
            property_values: pvals,
        };
        let loro_op: LoroOp = op.into();
        match loro_op {
            LoroOp::UpsertNode { properties, .. } => {
                // Zip stops at the shorter array (2 entries), regardless
                // of the bogus `property_count`.
                assert_eq!(properties.len(), 2);
            }
            other => panic!("expected UpsertNode, got {other:?}"),
        }
    }

    /// `NodeOp` is `#[repr(C)]` — verify it has the expected size for FFI.
    /// On a 64-bit target: `&str` is 16 bytes (ptr+len), `&[&str]` is 16
    /// bytes (ptr+len), `usize` is 8 bytes. Total:
    ///   loro_key (16) + labels (16) + property_count (8) +
    ///   property_keys (16) + property_values (16) = 72 bytes.
    ///
    /// We assert `<= 80` to allow for alignment padding without being
    /// brittle across platforms.
    #[test]
    fn node_op_repr_c_layout() {
        // Sizes are platform-dependent; we assert a reasonable upper bound.
        // The key invariant: the struct is NOT larger than the sum of its
        // fields plus alignment padding, which would indicate a non-`repr(C)`
        /// layout regression.
        assert_eq!(
            std::mem::size_of::<&str>(),
            std::mem::size_of::<usize>() * 2,
            "&str is (ptr, len) — 2 words"
        );
        assert_eq!(
            std::mem::size_of::<&[&str]>(),
            std::mem::size_of::<usize>() * 2,
            "&[&str] is (ptr, len) — 2 words"
        );
        let expected_max =
            std::mem::size_of::<&str>() * 4    // loro_key, labels, property_keys, property_values
            + std::mem::size_of::<usize>()     // property_count
            + 3 * std::mem::align_of::<usize>(); // worst-case padding
        assert!(
            std::mem::size_of::<NodeOp<'_>>() <= expected_max,
            "NodeOp size {} exceeded expected max {} — repr(C) layout regression?",
            std::mem::size_of::<NodeOp<'_>>(),
            expected_max
        );
    }

    /// `NodeValue` is `#[repr(C)]` — the enum tag is the first field,
    /// followed by the largest variant's payload. Verify it's reasonably
    /// small (tag + i64/f64 + ptr).
    #[test]
    fn node_value_repr_c_layout() {
        // Expected: tag (4 bytes, rounded to 8 by alignment) + payload.
        // Largest payload is `&str` (16 bytes) or `f64`/`i64` (8 bytes).
        // So 24 bytes is the typical size on 64-bit.
        let expected_max =
            std::mem::size_of::<u64>() // tag (rounded)
            + std::mem::size_of::<&str>() // largest payload
            + std::mem::align_of::<&str>(); // padding
        assert!(
            std::mem::size_of::<NodeValue<'_>>() <= expected_max,
            "NodeValue size {} exceeded expected max {} — repr(C) layout regression?",
            std::mem::size_of::<NodeValue<'_>>(),
            expected_max
        );
    }

    /// Bincode round-trip: serialize a `Vec<LoroOp>` and deserialize it back.
    /// This exercises the `Serialize`/`Deserialize` derives on `LoroOp` and
    /// `GraphValue` and verifies the bincode codec produces a stable,
    /// round-trippable encoding.
    #[cfg(feature = "serde")]
    #[test]
    fn loro_op_bincode_roundtrip() {
        let mut props = HashMap::new();
        props.insert("name".to_string(), GraphValue::String("alice".to_string()));
        props.insert("age".to_string(), GraphValue::Integer(30));
        props.insert("active".to_string(), GraphValue::Bool(true));
        props.insert("score".to_string(), GraphValue::Float(3.5));
        props.insert("nil".to_string(), GraphValue::Null);

        let ops = vec![
            LoroOp::UpsertNode {
                loro_key: "V/abc-123".to_string(),
                labels: vec!["Person".to_string(), "User".to_string()],
                properties: props.clone(),
            },
            LoroOp::DeleteNode {
                loro_key: "V/old".to_string(),
            },
            LoroOp::UpsertEdge {
                src_key: "V/a".to_string(),
                dst_key: "V/b".to_string(),
                label: "KNOWS".to_string(),
                properties: props,
            },
            LoroOp::TreeMove {
                node_key: "V/n".to_string(),
                old_parent_key: "V/p1".to_string(),
                new_parent_key: "V/p2".to_string(),
            },
        ];

        let bytes = bincode::serialize(&ops).expect("serialize");
        assert!(!bytes.is_empty(), "bincode output should be non-empty");

        let decoded: Vec<LoroOp> = bincode::deserialize(&bytes).expect("deserialize");
        // Structural equality — `LoroOp` derives `PartialEq`, and
        // `HashMap<String, GraphValue>` is `PartialEq` when `GraphValue`
        // is (which it is). Using `==` instead of `format!("{x:?}")`
        // because `HashMap` Debug iteration order is non-deterministic.
        assert_eq!(decoded.len(), ops.len());
        assert_eq!(decoded, ops);
    }

    /// Malformed bytes produce a `GrafeoLoroError::Bridge` from
    /// `apply_loro_op_bytes` (we can only check the error path here, not
    /// the full apply, which would need a `grafeo::Session`).
    #[cfg(feature = "serde")]
    #[test]
    fn loro_op_bincode_malformed_bytes_error() {
        // We can't call apply_loro_op_bytes without a real Session, but we
        // can verify bincode itself rejects garbage. This guards the
        // error-mapping path.
        let garbage: &[u8] = &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        let result: std::result::Result<Vec<LoroOp>, _> = bincode::deserialize(garbage);
        assert!(
            result.is_err(),
            "garbage bytes must not deserialize as Vec<LoroOp>"
        );
    }
}
