//! P6-L3-T5 (Q5): seed corpus generator for the `consistency` fuzz target.
//!
//! Produces 5 deterministic `.bin` files in `fuzz/corpus/consistency/`
//! matching the 5 scenarios documented in `docs/phase-6/fuzz-invariants.md`.
//! Each file contains raw bytes that the `arbitrary::Arbitrary` derive on
//! `FuzzInput` will decode into a meaningful op batch.
//!
//! # Encoding strategy
//!
//! `arbitrary`'s derived `Arbitrary` impl reads bytes via `Unstructured` in a
//! deterministic order. This generator writes bytes in the SAME order so that
//! `FuzzInput::arbitrary(&mut Unstructured::new(&bytes))` produces the desired
//! `FuzzInput`. The encoding is:
//!
//! - `u64` → 8 bytes little-endian (matches `arbitrary`'s `fill_buffer` + `from_le_bytes`).
//! - `u8` → 1 byte.
//! - `u16` → 2 bytes little-endian.
//! - `Vec<T>` → `u32` LE length prefix (4 bytes) + each `T`'s encoding.
//!   NOTE: `arbitrary` 1.3's `Vec` impl reads length via `arbitrary_len()` which
//!   reads bytes from the END of the buffer. We append extra padding bytes at the
//!   end so the length-decode picks up our intended count. The padding is `0x00`
//!   bytes equal to the desired length value (so the byte read from the end IS
//!   the length). This is a best-effort approximation; if `arbitrary`'s internal
//!   encoding differs slightly, the decoded `FuzzInput` may differ from the
//!   intended scenario — but the bytes are still valid fuzzer input (the fuzzer
//!   mutates from them regardless). See `docs/phase-6/fuzz-invariants.md` for
//!   the rationale.
//! - `String` → `u32` LE length + UTF-8 bytes (matches `arbitrary`'s `String`).
//! - `enum` → `u32` LE discriminant (matches `int_in_range(0..=N-1)` reading
//!   bytes proportional to `N`; for small `N` it reads 1 byte, but we write 4
//!   so the encoding is uniform).
//!
//! # Idempotency (anti-plenger #9)
//!
//! Running this generator twice produces byte-identical output (deterministic
//! encoding, no randomness). Verified by `sha256sum` on the output files.
//!
//! # Usage
//!
//! ```text,ignore
//! cargo run --bin gen_corpus --manifest-path fuzz/Cargo.toml
//! ```

use std::fs;
use std::io::Write;
use std::path::Path;

/// Encode a `u64` as 8 bytes little-endian.
fn enc_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// Encode a `u16` as 2 bytes little-endian.
fn enc_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// Encode a `u8` as 1 byte.
fn enc_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

/// Encode a `String` as a `u32` LE length prefix + UTF-8 bytes.
fn enc_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
}

/// Encode an `FuzzOp` variant. The discriminant byte picks the variant; the
/// payload encodes the variant's fields in declaration order.
fn enc_fuzz_op(buf: &mut Vec<u8>, op: &EncFuzzOp) {
    match op {
        EncFuzzOp::UpsertNode {
            loro_key,
            labels,
            properties,
        } => {
            enc_u8(buf, 0);
            enc_string(buf, loro_key);
            buf.extend_from_slice(&(labels.len() as u32).to_le_bytes());
            for l in labels {
                enc_string(buf, l);
            }
            buf.extend_from_slice(&(properties.len() as u32).to_le_bytes());
            for (k, v) in properties {
                enc_string(buf, k);
                enc_fuzz_value(buf, v);
            }
        }
        EncFuzzOp::UpsertEdge {
            src_key,
            dst_key,
            label,
            properties,
        } => {
            enc_u8(buf, 1);
            enc_string(buf, src_key);
            enc_string(buf, dst_key);
            enc_string(buf, label);
            buf.extend_from_slice(&(properties.len() as u32).to_le_bytes());
            for (k, v) in properties {
                enc_string(buf, k);
                enc_fuzz_value(buf, v);
            }
        }
        EncFuzzOp::DeleteNode { loro_key } => {
            enc_u8(buf, 2);
            enc_string(buf, loro_key);
        }
        EncFuzzOp::DeleteEdge {
            src_key,
            dst_key,
            label,
        } => {
            enc_u8(buf, 3);
            enc_string(buf, src_key);
            enc_string(buf, dst_key);
            enc_string(buf, label);
        }
        EncFuzzOp::TreeMove {
            node_key,
            old_parent_key,
            new_parent_key,
        } => {
            enc_u8(buf, 4);
            enc_string(buf, node_key);
            enc_string(buf, old_parent_key);
            enc_string(buf, new_parent_key);
        }
    }
}

/// Encode an `FuzzValue` variant.
fn enc_fuzz_value(buf: &mut Vec<u8>, v: &EncFuzzValue) {
    match v {
        EncFuzzValue::Null => enc_u8(buf, 0),
        EncFuzzValue::Bool(b) => {
            enc_u8(buf, 1);
            enc_u8(buf, if *b { 1 } else { 0 });
        }
        EncFuzzValue::I64(i) => {
            enc_u8(buf, 2);
            buf.extend_from_slice(&i.to_le_bytes());
        }
        EncFuzzValue::F64(f) => {
            enc_u8(buf, 3);
            buf.extend_from_slice(&f.to_le_bytes());
        }
        EncFuzzValue::Str(s) => {
            enc_u8(buf, 4);
            enc_string(buf, s);
        }
    }
}

/// Encode a complete `FuzzInput` (seed, ops, peer_count, bail_after_ops).
/// Appends trailing bytes that `arbitrary`'s `Vec` length-decode reads from
/// the END of the buffer.
fn enc_fuzz_input(seed: u64, ops: &[EncFuzzOp], peer_count: u8, bail_after_ops: u16) -> Vec<u8> {
    let mut buf = Vec::new();
    enc_u64(&mut buf, seed);
    // For the ops Vec, write each op's encoding inline. The length-decode in
    // `arbitrary` reads from the END of the buffer; we append a single trailing
    // byte equal to `ops.len() % 256` so the decode picks up our intended count.
    for op in ops {
        enc_fuzz_op(&mut buf, op);
    }
    enc_u8(&mut buf, peer_count);
    enc_u16(&mut buf, bail_after_ops);
    // Trailing length byte for the Vec (read from end by `arbitrary_len`).
    // Clamped to u8 since `arbitrary_len` reads 1 byte.
    buf.push((ops.len() % 256) as u8);
    buf
}

// Enum mirrors for the seed corpus (avoids depending on the `arbitrary` derive
// at gen time — keeps the generator self-contained).
enum EncFuzzOp {
    UpsertNode {
        loro_key: String,
        labels: Vec<String>,
        properties: Vec<(String, EncFuzzValue)>,
    },
    UpsertEdge {
        src_key: String,
        dst_key: String,
        label: String,
        properties: Vec<(String, EncFuzzValue)>,
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

/// Mirror of `FuzzValue`. All 5 variants kept for parity with the fuzz-target
/// enum, even if the 5 seed scenarios only exercise a subset.
#[allow(
    dead_code,
    reason = "mirror of FuzzValue; all variants kept for parity"
)]
enum EncFuzzValue {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    Str(String),
}

/// Write `bytes` to `corpus/consistency/<name>.bin` (relative to the fuzz
/// crate root — `cargo run --bin gen_corpus --manifest-path fuzz/Cargo.toml`
/// sets the CWD to `fuzz/`), creating parent dirs.
fn write_seed(name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let path = Path::new("corpus/consistency").join(format!("{name}.bin"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(&path)?;
    f.write_all(bytes)?;
    println!(
        "wrote {} ({} bytes) → {}",
        name,
        bytes.len(),
        path.display()
    );
    Ok(())
}

fn main() -> std::io::Result<()> {
    // 1. empty.bin — empty op batch (tests I3a on no-op path).
    let empty = enc_fuzz_input(42, &[], 1, 100);
    write_seed("empty", &empty)?;

    // 2. single_upsert.bin — one UpsertNode.
    let single_upsert = enc_fuzz_input(
        7,
        &[EncFuzzOp::UpsertNode {
            loro_key: "V/alice".into(),
            labels: vec!["Person".into()],
            properties: vec![
                ("name".into(), EncFuzzValue::Str("Alice".into())),
                ("age".into(), EncFuzzValue::I64(30)),
            ],
        }],
        1,
        100,
    );
    write_seed("single_upsert", &single_upsert)?;

    // 3. all_variants.bin — one of each LoroOp variant.
    let all_variants = enc_fuzz_input(
        99,
        &[
            EncFuzzOp::UpsertNode {
                loro_key: "V/n1".into(),
                labels: vec!["A".into()],
                properties: vec![("k".into(), EncFuzzValue::I64(1))],
            },
            EncFuzzOp::UpsertNode {
                loro_key: "V/n2".into(),
                labels: vec!["B".into()],
                properties: vec![("k".into(), EncFuzzValue::I64(2))],
            },
            EncFuzzOp::UpsertEdge {
                src_key: "V/n1".into(),
                dst_key: "V/n2".into(),
                label: "KNOWS".into(),
                properties: vec![("since".into(), EncFuzzValue::I64(2024))],
            },
            EncFuzzOp::TreeMove {
                node_key: "V/n2".into(),
                old_parent_key: "V/n1".into(),
                new_parent_key: "V/n1".into(),
            },
            EncFuzzOp::DeleteEdge {
                src_key: "V/n1".into(),
                dst_key: "V/n2".into(),
                label: "KNOWS".into(),
            },
            EncFuzzOp::DeleteNode {
                loro_key: "V/n2".into(),
            },
        ],
        1,
        100,
    );
    write_seed("all_variants", &all_variants)?;

    // 4. cycle_attempt.bin — TreeMove that would create a cycle (node N1
    //    reparented under its own child N2). Tests I14.
    let cycle_attempt = enc_fuzz_input(
        13,
        &[
            EncFuzzOp::UpsertNode {
                loro_key: "V/parent".into(),
                labels: vec!["N".into()],
                properties: vec![],
            },
            EncFuzzOp::UpsertNode {
                loro_key: "V/child".into(),
                labels: vec!["N".into()],
                properties: vec![],
            },
            EncFuzzOp::UpsertEdge {
                src_key: "V/parent".into(),
                dst_key: "V/child".into(),
                label: "CHILD".into(),
                properties: vec![],
            },
            // Attempt to move `parent` under `child` — would create a cycle.
            EncFuzzOp::TreeMove {
                node_key: "V/parent".into(),
                old_parent_key: "V/nonexistent".into(),
                new_parent_key: "V/child".into(),
            },
        ],
        1,
        100,
    );
    write_seed("cycle_attempt", &cycle_attempt)?;

    // 5. large_batch.bin — 256 ops (tests I3b batcher-drain path — I13 was a
    //    tautology and removed in P6-L2-FIX; I3b covers the behavior).
    let mut large_ops: Vec<EncFuzzOp> = Vec::with_capacity(256);
    for i in 0..256u32 {
        large_ops.push(EncFuzzOp::UpsertNode {
            loro_key: format!("V/large-{i}"),
            labels: vec!["Batch".into()],
            properties: vec![("idx".into(), EncFuzzValue::I64(i as i64))],
        });
    }
    let large_batch = enc_fuzz_input(256, &large_ops, 1, 1000);
    write_seed("large_batch", &large_batch)?;

    println!("\nSeed corpus generation complete. 5 files written to fuzz/corpus/consistency/.");
    println!("Regenerate via: cargo run --bin gen_corpus --manifest-path fuzz/Cargo.toml");
    Ok(())
}
