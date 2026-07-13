# Advanced Topics

## Custom Schema Derives

### The `loro(text)` Attribute

```rust
#[derive(Hydrate, Reconcile)]
pub struct VertexEntity {
    pub labels: Vec<String>,
    pub properties: HashMap<String, LoroProperty>,

    #[loro(text)]  // Maps to LoroText container
    pub description: String,
}
```

Behavior:
- `Hydrate`: Reads `LoroText::to_string()`
- `Reconcile`: Creates/updates `LoroText` container with diff
- Enables collaborative rich-text editing on the field

### The `loro(movable)` Attribute

```rust
#[derive(Hydrate, Reconcile)]
pub struct OrderedCollection {
    #[loro(movable)]  // Enables drag-and-drop reordering
    pub items: Vec<TreeNode>,
}
```

Behavior:
- Uses Loro's movable list semantics
- Reorder operations are CRDT-merged without conflict
- TreeNode's `#[key]` field determines identity across moves

### Custom Key Fields

```rust
#[derive(Hydrate, Reconcile)]
pub struct TreeNode {
    #[key]  // Used for identity in movable collections
    pub node_id: String,
    pub title: String,
}
```

The `#[key]` field must be unique within its container. Duplicate keys → last-write-wins.

---

## Vector Embeddings Pipeline

### Current: Deterministic Dummy

```rust
pub fn generate_local_embedding(text: &str) -> Result<Vec<f32>> {
    // SplitMix64 PRNG seeded by text bytes
    // Returns 384-dim vector (DEFAULT_EMBEDDING_DIM)
    // Deterministic: same text → same vector
}
```

Useful for:
- Testing vector search without ONNX dependency
- Reproducible fuzz targets
- CI/CD without GPU

### Future: ONNX Runtime

```rust
#[cfg(feature = "onnx")]
pub async fn generate_onnx_embedding(text: &str) -> Result<Vec<f32>> {
    let session = ort::Session::builder()
        .commit_from_file("model.onnx")?;
    let tokens = tokenize(text);
    let outputs = session.run(inputs![tokens]?)?;
    Ok(outputs["embedding"].try_extract::<f32>()?.to_vec())
}
```

### VectorOffloadManager

```rust
let mgr = VectorOffloadManager::new(db.clone());

// Triggered by text property update
mgr.handle_text_update(node_id, "new text content").await?;

// Writes to Grafeo:
// SET node.embedding = Vector([0.1, 0.2, ...]) WHERE id = node_id
```

The embedding is stored as `grafeo::Value::Vector`, queryable via:
```rust
db.session().execute("
    MATCH (n:Document)
    WHERE vector_similarity(n.embedding, $query_vec) > 0.8
    RETURN n.title
")?;
```

---

## CDC Event Translation

### Inbound (Loro Diff → LoroOp)

```rust
fn translate_diff_event(event: &DiffEvent) -> Vec<LoroOp> {
    // Container name determines target:
    // "V" → vertex operations (UpsertNode/DeleteNode)
    // "E" → edge operations (UpsertEdge/DeleteEdge)
    // other → skipped with trace log

    // Map diff: key = loro_key, value = LoroValue::Map(properties)
    // None value → DeleteNode/DeleteEdge
}
```

### Outbound (CDC → Loro Update)

```rust
fn apply_change_event_to_loro(doc, event, maps) -> Result<()> {
    // EntityId::Node + Create/Update → Upsert into V map
    // EntityId::Node + Delete → Delete from V map
    // EntityId::Edge + Create → Upsert into E map (with endpoint lookup)
    // EntityId::Edge + Update → Update E map (key from edge_key_map)
    // EntityId::Edge + Delete → Delete from E map (remove from maps)
    // EntityId::Triple → skipped (Phase 1)
}
```

### Edge Key Encoding

```rust
// Loro map key format: "src_key|dst_key|label"
fn encode_edge_key((src, dst, label): &EdgeKey) -> String {
    format!("{src}|{dst}|{label}")
}

// Parse: splitn(3, '|'), reject if < 3 parts or empty label
fn parse_edge_key(s: &str) -> Option<EdgeKey>;
```

---

## Epoch Management

### The Echo Prevention Set

```rust
pub(crate) bridge_origin_epochs: Arc<RwLock<HashSet<EpochId>>>;
```

Purpose: Track epochs created by the bridge itself, so outbound CDC events from bridge-originated transactions are filtered (not re-applied to Loro).

Lifecycle:
```
1. Bridge writes to Grafeo → generates Epoch E
2. E inserted into bridge_origin_epochs
3. CDC poller sees E in changes_between
4. E found in bridge_origin_epochs → filtered (echo prevented)
5. Retention: epochs older than EPOCH_RETENTION (10,000) are pruned
```

### MVCC Snapshots

```rust
let session = db.session();
session.set_viewing_epoch(epoch);  // Pin to historical state

// All reads return state as of 'epoch'
let prop = session.get_node_property(node_id, "name");

session.clear_viewing_epoch();  // Return to latest
```

Use cases:
- Time-travel queries
- Consistent reads across multiple properties
- Audit snapshots

---

## Compression Internals

### Wire Format

```
Byte 0: Version (0x01)
Byte 1: Codec tag
  0x00 = None
  0x01 = Lz4
  0x02 = Zstd
Bytes 2..: Payload
```

### Codec Characteristics

| Codec | Ratio | Speed | Memory | Use Case |
|---|---|---|---|---|
| None | 1.0x | ∞ | 0 | LAN, testing |
| Lz4 | ~2x | 500 MB/s | Low | Real-time sync |
| Zstd(3) | ~5x | 100 MB/s | Medium | Storage, backup |

### Streaming API

```rust
// For large documents: compress in chunks
let mut encoder = zstd::stream::Encoder::new(writer, 3)?;
for chunk in doc.export_stream() {
    encoder.write_all(&chunk)?;
}
encoder.finish()?;
```

Not yet implemented. Current API materializes full `Vec<u8>`.

---

## Presence Protocol

### Binary Format

```
Offset    Size    Content
0         4       Magic: %EPH (0x25 0x45 0x50 0x48)
4         2       room_id length (little-endian u16)
6         N       room_id bytes (UTF-8)
6+N       1       Message type (0x01 = Presence)
7+N       M       JSON payload (serde_json)
```

Total size: 7 + N + M bytes

### Overhead Analysis

| Component | Fixed | Variable |
|---|---|---|
| Magic | 4 | — |
| Room length | 2 | — |
| Room ID | — | N |
| Type | 1 | — |
| Payload | — | M |
| **Total** | **7** | **N + M** |

Typical: room_id=16, payload=128 → 151 bytes per message
At 30fps cursor tracking: ~4.5 KB/s per peer

### Future Extensions

| Type | Value | Purpose |
|---|---|---|
| Presence | 0x01 | Current implementation |
| Selection | 0x02 | Shared text selection ranges |
| Awareness | 0x03 | Viewport/fold state |
| Custom | 0x80-0xFF | Application-specific |
