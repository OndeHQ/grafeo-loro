# Operations Guide

## Deployment Patterns

### Single-Process (Desktop App)

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let storage = Arc::new(LocalFsStorage::new("./data"));
    let app = GrafeoLoroApp::builder()
        .storage(storage)
        .ssot_mode(SsotMode::Loro)
        .grafeo_dir(PathBuf::from("./data/graph"))
        .build().await?;

    // Hydrate on startup
    app.hydrate("my-graph").await?;

    // ... application logic ...

    // Checkpoint on graceful exit
    app.checkpoint("my-graph").await?;
    app.shutdown().await?;
    Ok(())
}
```

### Multi-Process (Server)

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   API Pod   │◄───►│   Sync Pod  │◄───►│  Object Store│
│  (GraphQL)  │     │  (grafeo-   │     │  (S3/GCS)   │
│             │     │   loro)      │     │             │
└─────────────┘     └─────────────┘     └─────────────┘
       │                   │
       └──── WebSocket ────┘
              (presence)
```

```rust
// Sync Pod: owns the graph, serves CDC to API pods
let app = GrafeoLoroApp::builder()
    .storage(s3_backend)
    .ssot_mode(SsotMode::Grafeo)  // Grafeo owns truth
    .build().await?;

// API Pod: read replica via Loro sync
let replica = LoroDoc::new();
replica.import(&sync_payload)?;  // From sync pod
```

---

## Monitoring

### Health Checks

```rust
// Kubernetes liveness probe
let status = app.health().unwrap().check(5000); // 5s max staleness
if !status.overall {
    // Return HTTP 503, trigger pod restart
}

// Component breakdown
for (name, ok) in status.components {
    metrics.gauge(format!("health.{name}"), ok as u8);
}
```

### Key Metrics

| Metric | Type | Labels | Alert Threshold |
|---|---|---|---|
| `inbound_events_total` | Counter | `origin`, `event_type` | — |
| `outbound_events_total` | Counter | `origin`, `event_type` | — |
| `echo_filtered_total` | Counter | `direction` | > 100/min (stuck loop) |
| `batch_flush_duration_ms` | Histogram | `batch_size` | p99 > 1000ms |
| `hydration_duration_ms` | Histogram | `mode` | p99 > 30000ms |

### Log Levels

| Level | Module | Content |
|---|---|---|
| ERROR | `batcher_run` | Flush timeout, task panic |
| WARN | `sync_engine` | Channel full, CDC failure, unmapped event |
| INFO | `apply_loro_op` | Operation applied |
| DEBUG | `checkpoint` | Key operations, delta listing |
| TRACE | `bridge_*` | Map insert/remove |

---

## Disaster Recovery

### Scenario: Corrupted Loro Document

```bash
# 1. Stop application
# 2. Delete local Loro state
rm -rf ./data/graph/*.loro

# 3. Restart — hydrate rebuilds from storage
#    (base.loro + delta-*.loro)
```

### Scenario: Corrupted Grafeo Database

```bash
# 1. Stop application  
# 2. Delete Grafeo files
rm -rf ./data/graph/grafeo/*

# 3. Restart — parallel_hydrate_grafeo rebuilds from Loro
#    (slower than checkpoint restore, but guaranteed consistent)
```

### Scenario: Split Brain (Multi-Writer)

```rust
// Loro CRDTs merge automatically. No action needed.
// Grafeo sees merged history via CDC.
// If Grafeo-SSOT: last-writer-wins on conflicting properties.
```

---

## Performance Tuning

### Batch Sizing

| Workload | batch_size | batch_ms | Rationale |
|---|---|---|---|
| Real-time collab | 64 | 50 | Low latency, small batches |
| Bulk import | 1024 | 500 | Throughput over latency |
| Mixed | 256 | 100 | Default balance |

### Compression

| Network | Storage | Recommended |
|---|---|---|
| LAN / localhost | None | `CompressionType::None` |
| WAN / cloud | SSD | `CompressionType::Lz4` |
| WAN / cloud | HDD / S3 | `CompressionType::Zstd` |

### Hydration Chunk Size

```rust
// DEFAULT_CHUNK_SIZE = 256 vertices per rayon task
// Tune for: L3 cache size / vertex size
// Typical: 128-512
```

---

## Security

### Storage Encryption

```rust
// Wrap any StorageBackend with encryption
struct EncryptedStorage<S: StorageBackend> {
    inner: S,
    cipher: Aes256Gcm,
}

#[async_trait]
impl<S: StorageBackend> StorageBackend for EncryptedStorage<S> {
    async fn save(&self, key: &str, bytes: Vec<u8>) -> Result<()> {
        let encrypted = self.cipher.encrypt(&bytes);
        self.inner.save(key, encrypted).await
    }
    // ... load, list, delete similarly
}
```

### Presence Authentication

```rust
// EphEnvelope carries no auth. Add layer:
struct AuthenticatedPresence {
    envelope: EphEnvelope,
    hmac: [u8; 32],  // HMAC-SHA256(room_id || payload || timestamp)
    timestamp: u64,
}

// Verify: reject if |now - timestamp| > 30s or HMAC invalid
```

---

## Backup Strategy

### Continuous (Loro-SSOT)

```
S3 Bucket:
  graph-42/
    base.loro          (weekly full snapshot)
    delta-0001.loro    (hourly incremental)
    delta-0002.loro
    ...
```

Retention: Keep `base.loro` + last 168 deltas (7 days). Older: archive to Glacier.

### Point-in-Time (Grafeo-SSOT)

```rust
// Grafeo WAL supports epoch-based PITR
let epoch = EpochId::new(1_000_000);
db.session().set_viewing_epoch(epoch)?;
// Query returns state as of that epoch
```
