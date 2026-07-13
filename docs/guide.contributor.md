# Contributing

## Code Structure

```
src/
├── app.rs              # Builder + lifecycle (checkpoint/hydrate/shutdown)
├── bridge/
│   ├── batcher.rs      # MutationBatcher: buffer → Grafeo tx
│   ├── grafeo_tx.rs    # apply_loro_op + BridgeMaps
│   ├── mod.rs          # Public re-exports
│   ├── origin.rs       # Origin string constants + predicates
│   └── sync_engine.rs  # Three workers + CDC translation
├── compression/
│   ├── mod.rs          # Public re-exports
│   └── wrapper.rs      # CompressedPayload + LoroDocCompressionExt
├── config.rs           # SsotMode, CompressionType enums
├── constants.rs        # All string/byte constants
├── error.rs            # GrafeoLoroError + Result<T>
├── hydration/
│   ├── mod.rs          # Public re-exports
│   ├── parallel.rs     # parallel_hydrate_grafeo (rayon)
│   └── vector.rs       # VectorOffloadManager + generate_local_embedding
├── lib.rs              # Crate root: public API surface
├── presence/
│   ├── mod.rs          # Public re-exports
│   └── socket.rs       # PresenceManager + EphEnvelope protocol
├── schema/
│   ├── edge.rs         # EdgeEntity (Hydrate + Reconcile)
│   ├── mod.rs          # Public re-exports
│   ├── tree.rs         # TreeNode + OrderedCollection + cycle detection
│   └── vertex.rs       # VertexEntity (Hydrate + Reconcile)
├── storage/
│   ├── mod.rs          # Public re-exports
│   └── traits.rs       # StorageBackend async trait
├── telemetry/
│   ├── health.rs       # HealthProbe + HealthStatus
│   ├── metrics.rs      # MetricsRegistry + HydrationMode
│   ├── mod.rs          # SharedTracer type alias
│   └── traces.rs       # Span constructors
└── types/
    ├── events.rs       # LoroOp + CdcEventWrapper
    ├── ids.rs          # PeerId + re-exports
    ├── mod.rs          # Public re-exports
    ├── presence.rs     # PresencePayload
    └── values.rs       # GraphValue + LoroProperty + conversions

fuzz/
├── fuzz_targets/
│   ├── consistency.rs  # Main invariant fuzzer (I1..I15)
│   ├── gen_corpus.rs   # Seed corpus generator
│   └── lib.rs          # FuzzOp + FuzzValue + convert_fuzz_op
└── Cargo.toml
```

---

## Adding an Invariant

### 1. Define the Check Function

```rust
// In fuzz/fuzz_targets/consistency.rs

/// I16 — Your new invariant
fn check_i16_your_invariant(state: &FuzzState) {
    // Concrete assert comparing two values
    assert_eq!(
        actual, expected,
        "I16: description — got {:?}, expected {:?}", actual, expected
    );
}
```

### 2. Wire Into Fuzz Target

```rust
// In the fuzz_target! macro body:
check_i16_your_invariant(&state);
```

### 3. Update Checklist

```markdown
| I16 | Your invariant description | consistency.rs | 🚧 |
```

### 4. Verify

```bash
cargo +nightly fuzz run consistency -- -max_total_time=60
```

---

## Adding a Storage Backend

```rust
use async_trait::async_trait;
use grafeo_loro::StorageBackend;

pub struct RedisStorage {
    client: redis::Client,
    prefix: String,
}

#[async_trait]
impl StorageBackend for RedisStorage {
    async fn load(&self, key: &str) -> Result<Vec<u8>, std::io::Error> {
        let mut conn = self.client.get_async_connection().await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let full_key = format!("{}{}", self.prefix, key);
        redis::cmd("GET").arg(&full_key).query_async(&mut conn).await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    async fn save(&self, key: &str, bytes: Vec<u8>) -> Result<(), std::io::Error> {
        // ...
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>, std::io::Error> {
        // ...
    }

    async fn delete(&self, key: &str) -> Result<(), std::io::Error> {
        // ...
    }
}
```

---

## Testing Strategy

### Unit Tests

```rust
// In-module, #[cfg(test)]
#[test]
fn edge_key_roundtrip() {
    let key = ("a".into(), "b".into(), "KNOWS".into());
    let encoded = encode_edge_key(&key);
    assert_eq!(parse_edge_key(&encoded), Some(key));
}
```

### Integration Tests

```rust
// tests/integration_checkpoint.rs
#[tokio::test]
async fn checkpoint_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = Arc::new(LocalFsStorage::new(tmp.path()));
    let app = GrafeoLoroApp::builder()
        .storage(storage)
        .build().await.unwrap();

    // ... write data ...
    app.checkpoint("test").await.unwrap();

    // ... new app instance ...
    let app2 = /* rebuild */;
    app2.hydrate("test").await.unwrap();

    // Assert parity
    assert_eq!(app.grafeo_db().node_count(), app2.grafeo_db().node_count());
}
```

### Fuzz Tests

```bash
# Generate corpus
cargo run --bin gen_corpus --manifest-path fuzz/Cargo.toml

# Run fuzzer (indefinite)
cargo +nightly fuzz run consistency

# Run with coverage
cargo +nightly fuzz coverage consistency
```

---

## Code Style

### Tracing

```rust
// Always instrument public methods
#[instrument(skip(self, rx), name = "batcher_run", level = "info")]
pub async fn run(self: Arc<Self>, mut rx: mpsc::Receiver<LoroOp>) -> Result<()> {
    // ...
}

// Use structured fields
#[instrument(skip(bytes), fields(room_id = %room_id), name = "build_eph_envelope")]
```

### Error Handling

```rust
// Prefer ? over match for propagation
let prepared = session.prepare_commit()?;
prepared.commit()?;

// Contextual errors for bridge operations
return Err(GrafeoLoroError::Bridge(format!(
    "unknown node key(s): src={src_key:?} dst={dst_key:?}"
)));
```

### Lock Ordering

```rust
// Correct: node maps before edge maps
let node_id = maps.node_id_map.write().remove(key)?;
maps.node_key_map.write().remove(&node_id);

// Correct: read locks for lookups
let (src_id, dst_id) = match (
    maps.node_id_map.read().get(src_key),
    maps.node_id_map.read().get(dst_key),
) { ... };
```

---

## Release Checklist

- [ ] All invariants pass 1-hour fuzz run
- [ ] `cargo test` clean
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo doc` builds without warnings
- [ ] Version bumped in `Cargo.toml`
- [ ] `CHANGELOG.md` updated
- [ ] Feature matrix in README updated
