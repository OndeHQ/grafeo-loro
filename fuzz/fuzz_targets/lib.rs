//! Fuzz target library root.
//!
//! Hosts the shared `FuzzOp` + `FuzzValue` enums + `to_graph_value` impl +
//! `convert_fuzz_op` free function consumed by BOTH the `consistency` fuzz
//! target and the `gen_corpus` seed-corpus generator. Defined here (not in
//! either binary) to enforce DRY/SSOT (anti-plenger #5 — Bloat) — the prior
//! `EncFuzzOp`/`EncFuzzValue` mirrors in `gen_corpus.rs` were a 1:1 duplicate
//! of these types.

use arbitrary::Arbitrary;
use grafeo_loro::types::events::LoroOp;
use grafeo_loro::types::values::GraphValue;

/// Mirror of `grafeo_loro::types::events::LoroOp` with `Arbitrary`-derivable
/// field types. Uses `Vec<(String, FuzzValue)>` for properties (no `HashMap` —
/// `HashMap` does not derive `Arbitrary`) and converts to `HashMap<String,
/// GraphValue>` at apply time via `convert_fuzz_op`.
#[derive(Arbitrary, Debug, Clone)]
pub enum FuzzOp {
    UpsertNode {
        loro_key: String,
        labels: Vec<String>,
        properties: Vec<(String, FuzzValue)>,
    },
    UpsertEdge {
        src_key: String,
        dst_key: String,
        label: String,
        properties: Vec<(String, FuzzValue)>,
    },
    DeleteNode {
        loro_key: String,
    },
    DeleteEdge {
        src_key: String,
        dst_key: String,
        label: String,
    },
    TreeMove {
        node_key: String,
        old_parent_key: String,
        new_parent_key: String,
    },
}

/// Mirror of the scalar subset of `grafeo_loro::types::values::GraphValue`.
/// Excludes `Vector`/`Map`/`List` (exotic for the fuzzer — they require
/// recursive `Arbitrary` impls that produce deep structures unsuitable for
/// fuzz iteration speed). The 5 variants map 1:1 to `GraphValue` variants via
/// `FuzzValue::to_graph_value`.
#[derive(Arbitrary, Debug, Clone)]
pub enum FuzzValue {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    Str(String),
}

impl FuzzValue {
    /// Convert `FuzzValue` → `GraphValue` (scalar subset only).
    pub fn to_graph_value(&self) -> GraphValue {
        match self {
            FuzzValue::Null => GraphValue::Null,
            FuzzValue::Bool(b) => GraphValue::Bool(*b),
            FuzzValue::I64(i) => GraphValue::Integer(*i),
            FuzzValue::F64(f) => GraphValue::Float(*f),
            FuzzValue::Str(s) => GraphValue::String(s.clone()),
        }
    }
}

/// Convert a `FuzzOp` into a `LoroOp` by collecting the property vec into a
/// `HashMap`. Duplicate keys in the vec are last-wins (matches `HashMap::from`
/// semantics). This is the bridge between the `Arbitrary`-derivable fuzz shape
/// and the production `LoroOp` enum.
pub fn convert_fuzz_op(op: &FuzzOp) -> LoroOp {
    match op {
        FuzzOp::UpsertNode {
            loro_key,
            labels,
            properties,
        } => LoroOp::UpsertNode {
            loro_key: loro_key.clone(),
            labels: labels.clone(),
            properties: properties
                .iter()
                .map(|(k, v)| (k.clone(), v.to_graph_value()))
                .collect(),
        },
        FuzzOp::UpsertEdge {
            src_key,
            dst_key,
            label,
            properties,
        } => LoroOp::UpsertEdge {
            src_key: src_key.clone(),
            dst_key: dst_key.clone(),
            label: label.clone(),
            properties: properties
                .iter()
                .map(|(k, v)| (k.clone(), v.to_graph_value()))
                .collect(),
        },
        FuzzOp::DeleteNode { loro_key } => LoroOp::DeleteNode {
            loro_key: loro_key.clone(),
        },
        FuzzOp::DeleteEdge {
            src_key,
            dst_key,
            label,
        } => LoroOp::DeleteEdge {
            src_key: src_key.clone(),
            dst_key: dst_key.clone(),
            label: label.clone(),
        },
        FuzzOp::TreeMove {
            node_key,
            old_parent_key,
            new_parent_key,
        } => LoroOp::TreeMove {
            node_key: node_key.clone(),
            old_parent_key: old_parent_key.clone(),
            new_parent_key: new_parent_key.clone(),
        },
    }
}
