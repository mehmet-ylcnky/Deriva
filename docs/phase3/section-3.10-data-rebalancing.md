# §3.10 Data Migration & Rebalancing

> **Status:** Not started
> **Depends on:** §3.1 (SWIM gossip), §3.3 (leaf data replication), §3.4 (cache placement policy)
> **Crate(s):** `deriva-network`, `deriva-storage`, `deriva-server`
> **Estimated effort:** 3–4 days

---

## 1. Problem Statement

When nodes join or leave a Deriva cluster, the consistent hash ring shifts ownership
boundaries. Blobs that were assigned to one set of replica nodes now belong to a
different set. Without active data migration:

1. **Join without rebalancing**: new node owns a ring segment but has zero data.
   All reads for that segment miss locally and must be fetched remotely, creating
   a sustained hot-spot on the previous owner until cache warming completes.

2. **Leave without rebalancing**: departed node's data is gone. If replication
   factor was met before departure, surviving replicas still hold copies. But if
   RF=1 or multiple nodes fail simultaneously, data is lost.

3. **Gradual drift**: over many join/leave events, data distribution skews.
   Some nodes hold far more data than their ring share warrants, wasting disk
   and creating uneven read latency.

### Goals

- **Correctness**: after rebalancing, every blob is stored on its designated
  replica set according to the current ring.
- **Availability**: reads and writes continue during migration (no downtime).
- **Throttling**: migration traffic does not starve normal client requests.
- **Progress tracking**: operators can observe migration state and ETA.
- **Resumability**: if a migration is interrupted (node crash), it resumes
  from where it left off rather than restarting.

### Non-Goals

- Cross-datacenter migration (future §4.x).
- Automatic RF adjustment (RF is static config).

---

## 2. Design

### 2.1 Architecture Overview

```
  Ring BEFORE Node D joins          Ring AFTER Node D joins
  ┌───────────────────┐             ┌───────────────────┐
  │     A (0–120)     │             │     A (0–90)      │
  │     B (121–200)   │             │     D (91–120)    │  ← new
  │     C (201–360)   │             │     B (121–200)   │
  └───────────────────┘             │     C (201–360)   │
                                    └───────────────────┘

  Blobs in range 91–120 must migrate from A → D.
  A is the "source", D is the "target".

  Migration flow:
  ┌──────┐  StreamTransfer  ┌──────┐
  │  A   │ ───────────────► │  D   │
  │(src) │  blob chunks     │(tgt) │
  │      │ ◄─────────────── │      │
  │      │  Ack per batch   │      │
  └──────┘                  └──────┘
```

### 2.2 Migration Trigger

```
  SWIM membership change
         │
         ▼
  Ring recalculated (§3.4)
         │
         ▼
  For each local blob:
    old_owners = ring_before.replicas(blob.addr)
    new_owners = ring_after.replicas(blob.addr)
    if self ∈ old_owners && self ∉ new_owners:
      → schedule SEND to new_owners
    if self ∈ new_owners && self ∉ old_owners:
      → expect RECEIVE from old_owners
```

### 2.3 Core Types

```rust
/// Tracks a single migration task: one blob from source to target.
#[derive(Debug, Clone)]
pub struct MigrationTask {
    pub blob_addr: CAddr,
    pub blob_size: u64,
    pub source: NodeId,
    pub target: NodeId,
    pub state: MigrationTaskState,
    pub created_at: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MigrationTaskState {
    Pending,
    Transferring,
    Completed,
    Failed { reason: String, retries: u32 },
}

/// Orchestrates all migrations for this node.
pub struct MigrationManager {
    swim: Arc<Swim>,
    blob_store: Arc<BlobStore>,
    ring: Arc<RwLock<HashRing>>,
    config: MigrationConfig,
    /// Outbound tasks: blobs this node must send away.
    outbound: Arc<Mutex<VecDeque<MigrationTask>>>,
    /// Inbound tasks: blobs this node expects to receive.
    inbound: Arc<DashMap<CAddr, MigrationTaskState>>,
    /// Progress tracking.
    progress: Arc<MigrationProgress>,
    /// Throttle: max bytes/sec for migration traffic.
    rate_limiter: Arc<RateLimiter>,
    /// Cancellation.
    cancel: CancellationToken,
}

#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Max bytes per second for outbound migration traffic.
    pub max_transfer_rate: u64,          // default: 50 MB/s
    /// Max concurrent outbound streams.
    pub max_concurrent_streams: usize,   // default: 4
    /// Batch size for streaming transfer (bytes).
    pub stream_chunk_size: usize,        // default: 256 KB
    /// Retry limit per task.
    pub max_retries: u32,                // default: 3
    /// Delay between retries.
    pub retry_delay: Duration,           // default: 5s
    /// Whether to delete source copy after confirmed transfer.
    pub delete_after_transfer: bool,     // default: true
    /// Checkpoint interval for resumability.
    pub checkpoint_interval: Duration,   // default: 30s
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            max_transfer_rate: 50 * 1024 * 1024,
            max_concurrent_streams: 4,
            stream_chunk_size: 256 * 1024,
            max_retries: 3,
            retry_delay: Duration::from_secs(5),
            delete_after_transfer: true,
            checkpoint_interval: Duration::from_secs(30),
        }
    }
}

/// Tracks overall migration progress.
#[derive(Debug, Default)]
pub struct MigrationProgress {
    pub total_tasks: AtomicU64,
    pub completed_tasks: AtomicU64,
    pub failed_tasks: AtomicU64,
    pub bytes_transferred: AtomicU64,
    pub start_time: Mutex<Option<Instant>>,
    pub estimated_completion: Mutex<Option<Instant>>,
}
```

### 2.4 Wire Protocol

```protobuf
// In deriva_internal.proto

message MigrateTransferRequest {
    bytes blob_addr = 1;
    uint64 blob_size = 2;
    bytes chunk_data = 3;       // one chunk of the blob
    uint32 chunk_index = 4;
    uint32 total_chunks = 5;
    bool is_last = 6;
}

message MigrateTransferResponse {
    bool accepted = 1;
    string error = 2;
}

message MigrateStatusRequest {
    // empty — returns this node's migration status
}

message MigrateStatusResponse {
    uint64 total_tasks = 1;
    uint64 completed_tasks = 2;
    uint64 failed_tasks = 3;
    uint64 bytes_transferred = 4;
    uint64 bytes_remaining = 5;
    uint32 eta_seconds = 6;
    MigrateState state = 7;
}

enum MigrateState {
    IDLE = 0;
    PLANNING = 1;
    TRANSFERRING = 2;
    COMPLETING = 3;
}

service DerivaInternal {
    // ... existing RPCs ...
    rpc MigrateTransfer(stream MigrateTransferRequest) returns (MigrateTransferResponse);
    rpc MigrateStatus(MigrateStatusRequest) returns (MigrateStatusResponse);
}
```

### 2.5 Checkpoint / Resumability

```
Checkpoint file: <data_dir>/migration_checkpoint.json

{
  "ring_version": 42,
  "completed": ["<addr1>", "<addr2>", ...],
  "failed": {"<addr3>": {"retries": 2, "reason": "timeout"}},
  "pending": ["<addr4>", "<addr5>", ...]
}

On restart:
  1. Load checkpoint
  2. If ring_version matches current ring → resume pending tasks
  3. If ring_version differs → discard checkpoint, recompute plan
```

---

## 3. Implementation

### 3.1 MigrationManager Core

```rust
impl MigrationManager {
    pub fn new(
        swim: Arc<Swim>,
        blob_store: Arc<BlobStore>,
        ring: Arc<RwLock<HashRing>>,
        config: MigrationConfig,
    ) -> Self {
        Self {
            swim,
            blob_store,
            ring,
            config,
            outbound: Arc::new(Mutex::new(VecDeque::new())),
            inbound: Arc::new(DashMap::new()),
            progress: Arc::new(MigrationProgress::default()),
            rate_limiter: Arc::new(RateLimiter::new(config.max_transfer_rate)),
            cancel: CancellationToken::new(),
        }
    }

    /// Called when SWIM detects membership change.
    pub async fn on_ring_change(
        &self,
        old_ring: &HashRing,
        new_ring: &HashRing,
    ) -> Result<(), DerivaError> {
        let plan = self.compute_plan(old_ring, new_ring).await?;
        if plan.is_empty() {
            tracing::info!("ring change: no migration needed");
            return Ok(());
        }

        tracing::info!(
            outbound = plan.len(),
            total_bytes = plan.iter().map(|t| t.blob_size).sum::<u64>(),
            "migration plan computed"
        );

        let mut queue = self.outbound.lock().await;
        queue.extend(plan);
        self.progress.total_tasks.fetch_add(
            queue.len() as u64,
            Ordering::Relaxed,
        );
        *self.progress.start_time.lock().await = Some(Instant::now());

        self.spawn_workers();
        Ok(())
    }

    /// Compute which blobs need to move.
    async fn compute_plan(
        &self,
        old_ring: &HashRing,
        new_ring: &HashRing,
    ) -> Result<Vec<MigrationTask>, DerivaError> {
        let local_id = self.swim.local_id();
        let all_addrs = self.blob_store.all_addrs().await?;
        let mut tasks = Vec::new();

        for addr in all_addrs {
            let old_owners = old_ring.replicas(&addr);
            let new_owners = new_ring.replicas(&addr);

            // We owned it before but not anymore → send to new primary
            if old_owners.contains(&local_id) && !new_owners.contains(&local_id) {
                let target = new_owners[0]; // primary of new replica set
                let size = self.blob_store.blob_size(&addr).await?;
                tasks.push(MigrationTask {
                    blob_addr: addr,
                    blob_size: size,
                    source: local_id,
                    target,
                    state: MigrationTaskState::Pending,
                    created_at: Instant::now(),
                });
            }
        }

        Ok(tasks)
    }

    /// Spawn concurrent transfer workers.
    fn spawn_workers(&self) {
        let concurrency = self.config.max_concurrent_streams;
        for _ in 0..concurrency {
            let mgr = self.clone_inner();
            tokio::spawn(async move {
                mgr.transfer_loop().await;
            });
        }
    }

    /// Worker loop: pull tasks from queue, transfer each.
    async fn transfer_loop(&self) {
        loop {
            let task = {
                let mut queue = self.outbound.lock().await;
                queue.pop_front()
            };

            let Some(mut task) = task else {
                break; // queue empty
            };

            task.state = MigrationTaskState::Transferring;
            match self.transfer_blob(&task).await {
                Ok(()) => {
                    task.state = MigrationTaskState::Completed;
                    self.progress.completed_tasks.fetch_add(1, Ordering::Relaxed);
                    self.progress.bytes_transferred.fetch_add(
                        task.blob_size, Ordering::Relaxed,
                    );

                    if self.config.delete_after_transfer {
                        let _ = self.blob_store.delete(&task.blob_addr).await;
                    }
                }
                Err(e) => {
                    let retries = match &task.state {
                        MigrationTaskState::Failed { retries, .. } => *retries,
                        _ => 0,
                    };
                    if retries < self.config.max_retries {
                        task.state = MigrationTaskState::Failed {
                            reason: e.to_string(),
                            retries: retries + 1,
                        };
                        tokio::time::sleep(self.config.retry_delay).await;
                        self.outbound.lock().await.push_back(task);
                    } else {
                        tracing::error!(
                            addr = %task.blob_addr,
                            error = %e,
                            "migration task exhausted retries"
                        );
                        self.progress.failed_tasks.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    /// Stream a single blob to the target node.
    async fn transfer_blob(&self, task: &MigrationTask) -> Result<(), DerivaError> {
        let data = self.blob_store.get_raw(&task.blob_addr).await?;
        let chunks: Vec<_> = data
            .chunks(self.config.stream_chunk_size)
            .enumerate()
            .collect();
        let total_chunks = chunks.len() as u32;

        let channel = self.swim.channel_for(&task.target)?;
        let mut client = DerivaInternalClient::new(channel);

        let (tx, rx) = tokio::sync::mpsc::channel(4);

        let response_fut = client.migrate_transfer(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        );

        for (i, chunk) in chunks {
            // Rate limiting
            self.rate_limiter.acquire(chunk.len() as u64).await;

            tx.send(MigrateTransferRequest {
                blob_addr: task.blob_addr.as_bytes().to_vec(),
                blob_size: task.blob_size,
                chunk_data: chunk.to_vec(),
                chunk_index: i as u32,
                total_chunks,
                is_last: i as u32 == total_chunks - 1,
            }).await.map_err(|_| DerivaError::Internal("stream closed".into()))?;
        }
        drop(tx);

        let response = response_fut.await?.into_inner();
        if !response.accepted {
            return Err(DerivaError::Internal(response.error));
        }

        Ok(())
    }
}
```

### 3.2 Rate Limiter

```rust
/// Token-bucket rate limiter for migration traffic.
pub struct RateLimiter {
    tokens: AtomicU64,
    max_tokens: u64,
    refill_rate: u64, // tokens per second
}

impl RateLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        Self {
            tokens: AtomicU64::new(bytes_per_sec),
            max_tokens: bytes_per_sec,
            refill_rate: bytes_per_sec,
        }
    }

    pub async fn acquire(&self, bytes: u64) {
        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            if current >= bytes {
                if self.tokens.compare_exchange(
                    current, current - bytes,
                    Ordering::Relaxed, Ordering::Relaxed,
                ).is_ok() {
                    return;
                }
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }

    /// Called periodically (every 100ms) to refill tokens.
    pub fn refill(&self) {
        let add = self.refill_rate / 10; // 100ms worth
        let current = self.tokens.load(Ordering::Relaxed);
        let new = (current + add).min(self.max_tokens);
        self.tokens.store(new, Ordering::Relaxed);
    }
}
```

### 3.3 Inbound Transfer Handler

```rust
/// RPC handler: receive streamed blob from source node.
pub async fn migrate_transfer(
    &self,
    request: Request<Streaming<MigrateTransferRequest>>,
) -> Result<Response<MigrateTransferResponse>, Status> {
    let mut stream = request.into_inner();
    let mut assembler: Option<BlobAssembler> = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;

        let asm = assembler.get_or_insert_with(|| {
            BlobAssembler::new(
                CAddr::from_bytes(&chunk.blob_addr),
                chunk.blob_size as usize,
                chunk.total_chunks,
            )
        });

        asm.add_chunk(chunk.chunk_index, &chunk.chunk_data);

        if chunk.is_last {
            let (addr, data) = asm.finalize()?;

            // Verify content address
            let computed = CAddr::hash(&data);
            if computed != addr {
                return Ok(Response::new(MigrateTransferResponse {
                    accepted: false,
                    error: "content address mismatch".into(),
                }));
            }

            self.blob_store.put_raw_with_addr(&addr, &data).await
                .map_err(|e| Status::internal(e.to_string()))?;

            return Ok(Response::new(MigrateTransferResponse {
                accepted: true,
                error: String::new(),
            }));
        }
    }

    Ok(Response::new(MigrateTransferResponse {
        accepted: false,
        error: "stream ended without final chunk".into(),
    }))
}
```

### 3.4 Blob Assembler

```rust
/// Reassembles a blob from streamed chunks.
pub struct BlobAssembler {
    addr: CAddr,
    expected_size: usize,
    total_chunks: u32,
    chunks: Vec<Option<Vec<u8>>>,
    received: u32,
}

impl BlobAssembler {
    pub fn new(addr: CAddr, expected_size: usize, total_chunks: u32) -> Self {
        Self {
            addr,
            expected_size,
            total_chunks,
            chunks: vec![None; total_chunks as usize],
            received: 0,
        }
    }

    pub fn add_chunk(&mut self, index: u32, data: &[u8]) {
        if (index as usize) < self.chunks.len() && self.chunks[index as usize].is_none() {
            self.chunks[index as usize] = Some(data.to_vec());
            self.received += 1;
        }
    }

    pub fn finalize(self) -> Result<(CAddr, Vec<u8>), DerivaError> {
        if self.received != self.total_chunks {
            return Err(DerivaError::Internal(format!(
                "incomplete: {}/{} chunks", self.received, self.total_chunks
            )));
        }
        let data: Vec<u8> = self.chunks
            .into_iter()
            .filter_map(|c| c)
            .flatten()
            .collect();
        Ok((self.addr, data))
    }
}
```

### 3.5 Checkpoint Persistence

```rust
impl MigrationManager {
    async fn save_checkpoint(&self) -> Result<(), DerivaError> {
        let ring_version = self.ring.read().await.version();
        let queue = self.outbound.lock().await;

        let checkpoint = MigrationCheckpoint {
            ring_version,
            completed: self.completed_addrs().await,
            failed: self.failed_addrs().await,
            pending: queue.iter().map(|t| t.blob_addr).collect(),
        };

        let path = self.data_dir().join("migration_checkpoint.json");
        let json = serde_json::to_string_pretty(&checkpoint)?;
        tokio::fs::write(&path, json).await?;
        Ok(())
    }

    async fn load_checkpoint(&self) -> Result<Option<MigrationCheckpoint>, DerivaError> {
        let path = self.data_dir().join("migration_checkpoint.json");
        if !path.exists() {
            return Ok(None);
        }
        let json = tokio::fs::read_to_string(&path).await?;
        let cp: MigrationCheckpoint = serde_json::from_str(&json)?;
        Ok(Some(cp))
    }

    pub async fn resume_if_needed(&self) -> Result<(), DerivaError> {
        let Some(cp) = self.load_checkpoint().await? else {
            return Ok(());
        };
        let current_version = self.ring.read().await.version();
        if cp.ring_version != current_version {
            tracing::info!("checkpoint ring version mismatch, recomputing");
            return Ok(());
        }

        let pending: Vec<_> = cp.pending.into_iter()
            .filter(|a| !cp.completed.contains(a))
            .collect();

        tracing::info!(
            resuming = pending.len(),
            previously_completed = cp.completed.len(),
            "resuming migration from checkpoint"
        );

        let mut queue = self.outbound.lock().await;
        for addr in pending {
            let size = self.blob_store.blob_size(&addr).await.unwrap_or(0);
            let target = self.ring.read().await.replicas(&addr)[0];
            queue.push_back(MigrationTask {
                blob_addr: addr,
                blob_size: size,
                source: self.swim.local_id(),
                target,
                state: MigrationTaskState::Pending,
                created_at: Instant::now(),
            });
        }

        if !queue.is_empty() {
            self.spawn_workers();
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct MigrationCheckpoint {
    ring_version: u64,
    completed: Vec<CAddr>,
    failed: HashMap<CAddr, FailedInfo>,
    pending: Vec<CAddr>,
}

#[derive(Serialize, Deserialize)]
struct FailedInfo {
    retries: u32,
    reason: String,
}
```


---

## 4. Data Flow Diagrams

### 4.1 Node Join — Full Migration Lifecycle

```
  Time ──────────────────────────────────────────────────────►

  SWIM: Node D joins cluster {A, B, C}
    │
    ▼
  Ring recalculated on ALL nodes
    │
    ▼
  Each node computes migration plan:
    Node A: "I own blobs in range 91–120, D now owns them → SEND"
    Node B: "No ownership changes for my blobs → IDLE"
    Node C: "No ownership changes for my blobs → IDLE"
    Node D: "I'm new, I own range 91–120 → EXPECT receives"
    │
    ▼
  Node A starts transfer workers (4 concurrent)
    │
    ├── Worker 1: stream blob_1 → D ──── ACK ✓
    ├── Worker 2: stream blob_2 → D ──── ACK ✓
    ├── Worker 3: stream blob_3 → D ──── ACK ✓
    ├── Worker 4: stream blob_4 → D ──── ACK ✓
    │   (workers pull next from queue)
    ├── Worker 1: stream blob_5 → D ──── ACK ✓
    │   ...
    ▼
  Queue empty → workers exit
    │
    ▼
  Node A: delete_after_transfer → remove local copies
    │
    ▼
  Migration complete. D now serves range 91–120.
```

### 4.2 Node Leave — Graceful Departure

```
  Node B announces graceful leave (§3.8 shutdown)
    │
    ▼
  Ring recalculated: B's range redistributed to A and C
    │
    ▼
  Node B computes plan:
    "I own blobs in range 121–200"
    "Range 121–160 → now owned by A"
    "Range 161–200 → now owned by C"
    │
    ├── Transfer 121–160 blobs → A
    └── Transfer 161–200 blobs → C
    │
    ▼
  B waits for all transfers to complete
    │
    ▼
  B confirms leave to SWIM
    │
    ▼
  B shuts down
```

### 4.3 Transfer with Rate Limiting

```
  Rate limit: 50 MB/s, chunk size: 256 KB

  ┌─────────────────────────────────────────────────┐
  │ Token bucket: 50MB capacity, refills 5MB/100ms  │
  └─────────────────────────────────────────────────┘

  Worker 1: acquire(256KB) → granted → send chunk
  Worker 2: acquire(256KB) → granted → send chunk
  Worker 3: acquire(256KB) → granted → send chunk
  Worker 4: acquire(256KB) → granted → send chunk
  ...
  (200 chunks/sec × 256KB = 50 MB/s aggregate)

  If client traffic spikes:
    Migration workers still limited to 50 MB/s.
    Client traffic gets remaining bandwidth.
```

### 4.4 Interrupted Migration — Crash and Resume

```
  Node A transferring to D:
    blob_1 ✓ (completed)
    blob_2 ✓ (completed)
    blob_3 ✓ (completed)
    blob_4 ← in progress
    blob_5   (pending)
    blob_6   (pending)
    │
    ▼ CHECKPOINT saved: completed=[1,2,3], pending=[4,5,6]
    │
    ╳ Node A crashes
    │
    ▼ Node A restarts
    │
    ▼ load_checkpoint()
    │ ring_version matches? YES
    │ resume pending = [4,5,6] (blob_4 retried since incomplete)
    │
    ▼ spawn workers → transfer blob_4, blob_5, blob_6
    │
    ▼ Migration completes
```

### 4.5 Concurrent Read During Migration

```
  Client GET(blob_X) where blob_X is migrating A → D

  Case 1: blob_X still on A (not yet transferred)
    Client → D (new owner) → miss → forward to A → hit → return
    (§3.6 distributed get handles this transparently)

  Case 2: blob_X already on D (transfer complete)
    Client → D → hit → return

  Case 3: blob_X in transit (being streamed)
    Client → D → miss (not yet fully received)
    → forward to A → hit (still has copy) → return

  No read failures during migration. Worst case: extra hop.
```

### 4.6 Multiple Simultaneous Ring Changes

```
  t=0: Node D joins → migration A→D starts
  t=5: Node E joins → ring changes again

  Approach: CANCEL current migration, recompute plan.

  t=0: plan_v1 = {blob_1→D, blob_2→D, blob_3→D}
  t=5: ring changes → plan_v1 invalidated
       plan_v2 = {blob_1→D, blob_2→E, blob_3→D}
       (blob_2 now goes to E instead of D)

  Already-completed transfers in plan_v1:
    blob_1 already on D → still correct in plan_v2 → no action
    blob_2 already on D → wrong! should be on E
      → plan_v2 adds: blob_2 D→E (D will send to E)

  Implementation: on ring change, cancel workers, recompute,
  skip already-correctly-placed blobs, add corrective transfers.
```

---

## 5. Test Specification

### 5.1 Migration Plan Computation Tests

```rust
#[cfg(test)]
mod plan_tests {
    use super::*;

    fn ring_with_nodes(nodes: &[&str]) -> HashRing {
        let mut ring = HashRing::new(64); // 64 vnodes
        for n in nodes {
            ring.add_node(NodeId::from(*n));
        }
        ring
    }

    #[tokio::test]
    async fn test_node_join_creates_outbound_tasks() {
        let old_ring = ring_with_nodes(&["A", "B", "C"]);
        let new_ring = ring_with_nodes(&["A", "B", "C", "D"]);

        let mgr = setup_migration_manager("A", &old_ring).await;
        // Populate blobs that A owns in old ring but D owns in new ring
        let migrating = populate_blobs_in_range(&mgr, &old_ring, &new_ring, "A").await;

        let plan = mgr.compute_plan(&old_ring, &new_ring).await.unwrap();

        assert!(!plan.is_empty());
        for task in &plan {
            assert_eq!(task.source, NodeId::from("A"));
            assert_eq!(task.target, NodeId::from("D"));
            assert!(migrating.contains(&task.blob_addr));
        }
    }

    #[tokio::test]
    async fn test_no_migration_if_ownership_unchanged() {
        let old_ring = ring_with_nodes(&["A", "B", "C"]);
        let new_ring = old_ring.clone(); // same ring

        let mgr = setup_migration_manager("A", &old_ring).await;
        let plan = mgr.compute_plan(&old_ring, &new_ring).await.unwrap();
        assert!(plan.is_empty());
    }

    #[tokio::test]
    async fn test_node_leave_redistributes() {
        let old_ring = ring_with_nodes(&["A", "B", "C"]);
        let new_ring = ring_with_nodes(&["A", "C"]); // B leaves

        let mgr = setup_migration_manager("B", &old_ring).await;
        populate_blobs_owned_by(&mgr, &old_ring, "B").await;

        let plan = mgr.compute_plan(&old_ring, &new_ring).await.unwrap();

        assert!(!plan.is_empty());
        for task in &plan {
            assert_eq!(task.source, NodeId::from("B"));
            // target should be A or C
            assert!(
                task.target == NodeId::from("A") ||
                task.target == NodeId::from("C")
            );
        }
    }
}
```

### 5.2 Rate Limiter Tests

```rust
#[tokio::test]
async fn test_rate_limiter_throttles() {
    let limiter = RateLimiter::new(1_000_000); // 1 MB/s

    let start = Instant::now();
    // Acquire 1MB — should be instant (bucket starts full)
    limiter.acquire(1_000_000).await;
    assert!(start.elapsed() < Duration::from_millis(50));

    // Acquire another 1MB — should wait ~1s for refill
    // (In test, we manually refill to avoid real-time waits)
}

#[test]
fn test_rate_limiter_refill() {
    let limiter = RateLimiter::new(1_000_000);
    limiter.tokens.store(0, Ordering::Relaxed);
    limiter.refill(); // adds 100KB (1MB/10)
    assert_eq!(limiter.tokens.load(Ordering::Relaxed), 100_000);
}

#[test]
fn test_rate_limiter_caps_at_max() {
    let limiter = RateLimiter::new(1_000_000);
    limiter.refill();
    // Already at max, refill shouldn't exceed
    assert_eq!(limiter.tokens.load(Ordering::Relaxed), 1_000_000);
}
```

### 5.3 Blob Assembler Tests

```rust
#[test]
fn test_assembler_happy_path() {
    let addr = CAddr::hash(b"hello world");
    let mut asm = BlobAssembler::new(addr, 11, 2);
    asm.add_chunk(0, b"hello ");
    asm.add_chunk(1, b"world");
    let (result_addr, data) = asm.finalize().unwrap();
    assert_eq!(result_addr, addr);
    assert_eq!(data, b"hello world");
}

#[test]
fn test_assembler_missing_chunk() {
    let addr = CAddr::hash(b"test");
    let mut asm = BlobAssembler::new(addr, 4, 2);
    asm.add_chunk(0, b"te");
    // chunk 1 missing
    let result = asm.finalize();
    assert!(result.is_err());
}

#[test]
fn test_assembler_duplicate_chunk_ignored() {
    let addr = CAddr::hash(b"ab");
    let mut asm = BlobAssembler::new(addr, 2, 2);
    asm.add_chunk(0, b"a");
    asm.add_chunk(0, b"X"); // duplicate — ignored
    asm.add_chunk(1, b"b");
    let (_, data) = asm.finalize().unwrap();
    assert_eq!(data, b"ab"); // first chunk wins
}

#[test]
fn test_assembler_out_of_bounds_chunk() {
    let addr = CAddr::hash(b"x");
    let mut asm = BlobAssembler::new(addr, 1, 1);
    asm.add_chunk(5, b"bad"); // index 5, but total_chunks=1
    assert_eq!(asm.received, 0); // ignored
}
```

### 5.4 Checkpoint Tests

```rust
#[tokio::test]
async fn test_checkpoint_save_and_load() {
    let mgr = setup_migration_manager_with_tmpdir().await;

    // Simulate some completed and pending tasks
    mgr.outbound.lock().await.push_back(make_task("addr_1", Pending));
    mgr.outbound.lock().await.push_back(make_task("addr_2", Pending));

    mgr.save_checkpoint().await.unwrap();
    let cp = mgr.load_checkpoint().await.unwrap().unwrap();

    assert_eq!(cp.pending.len(), 2);
    assert!(cp.completed.is_empty());
}

#[tokio::test]
async fn test_checkpoint_resume_skips_completed() {
    let mgr = setup_migration_manager_with_tmpdir().await;

    // Save checkpoint with 1 completed, 2 pending
    let cp = MigrationCheckpoint {
        ring_version: mgr.ring.read().await.version(),
        completed: vec![addr("addr_1")],
        failed: HashMap::new(),
        pending: vec![addr("addr_1"), addr("addr_2"), addr("addr_3")],
    };
    save_checkpoint_raw(&mgr, &cp).await;

    mgr.resume_if_needed().await.unwrap();
    let queue = mgr.outbound.lock().await;
    // addr_1 already completed → only addr_2, addr_3 queued
    assert_eq!(queue.len(), 2);
}

#[tokio::test]
async fn test_checkpoint_discarded_on_ring_version_mismatch() {
    let mgr = setup_migration_manager_with_tmpdir().await;

    let cp = MigrationCheckpoint {
        ring_version: 999, // doesn't match current
        completed: vec![],
        failed: HashMap::new(),
        pending: vec![addr("addr_1")],
    };
    save_checkpoint_raw(&mgr, &cp).await;

    mgr.resume_if_needed().await.unwrap();
    let queue = mgr.outbound.lock().await;
    assert!(queue.is_empty()); // discarded, not resumed
}
```

### 5.5 Integration Tests

```rust
#[tokio::test]
async fn test_end_to_end_migration_on_join() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Put blobs on the 3-node cluster
    let addrs: Vec<_> = (0..20).map(|i| {
        let data = format!("blob_{}", i);
        put_leaf(&nodes[0], data.as_bytes())
    }).collect();
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Add 4th node
    let node_d = start_node("D").await;
    tokio::time::sleep(Duration::from_secs(15)).await; // migration time

    // Verify: all blobs accessible via node D
    for addr in &addrs {
        let result = get_blob(&node_d, addr).await;
        assert!(result.is_ok(), "blob {} not accessible via D", addr);
    }

    // Verify: blobs that D now owns are stored locally on D
    let d_ring = node_d.ring.read().await;
    for addr in &addrs {
        let owners = d_ring.replicas(addr);
        if owners.contains(&node_d.id()) {
            assert!(node_d.blob_store.exists(addr).await);
        }
    }
}

#[tokio::test]
async fn test_migration_does_not_lose_data() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    let addrs: Vec<_> = (0..50).map(|i| {
        put_leaf(&nodes[0], format!("data_{}", i).as_bytes())
    }).collect();
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Trigger multiple ring changes
    let node_d = start_node("D").await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    let node_e = start_node("E").await;
    tokio::time::sleep(Duration::from_secs(15)).await;

    // ALL blobs must still be accessible
    for addr in &addrs {
        let result = get_blob_from_any(&[&nodes[0], &nodes[1], &nodes[2],
                                         &node_d, &node_e], addr).await;
        assert!(result.is_ok(), "blob {} lost after rebalancing", addr);
    }
}

#[tokio::test]
async fn test_reads_succeed_during_migration() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    let addr = put_leaf(&nodes[0], b"important_data").await;

    // Start migration by adding node
    let node_d = start_node("D").await;

    // Immediately read — should succeed even during migration
    for _ in 0..10 {
        let result = get_blob(&nodes[0], &addr).await;
        assert!(result.is_ok());
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Target node dies mid-transfer | Retry with exponential backoff; after max_retries, mark failed | Transient failures common |
| 2 | Source node dies mid-transfer | Target receives partial blob → discards | Assembler detects incomplete |
| 3 | Content address mismatch on receive | Reject transfer, source retries | Corruption detection |
| 4 | Ring changes during migration | Cancel current plan, recompute | Stale plan may send to wrong node |
| 5 | Blob already exists on target | Target returns accepted (idempotent) | Duplicate transfer is harmless |
| 6 | Empty blob (0 bytes) | Single chunk with empty data | Edge case but valid |
| 7 | Very large blob (>1GB) | Streamed in 256KB chunks, ~4000 chunks | Rate limiter prevents saturation |
| 8 | All blobs already correctly placed | Empty plan, no workers spawned | No-op is valid |
| 9 | Node joins and immediately leaves | Migration starts then cancels | Cancel token stops workers |
| 10 | Checkpoint file corrupted | Discard, recompute plan from scratch | Safe fallback |
| 11 | Disk full on target | Transfer rejected, source retries later | Target returns error |
| 12 | Rate limiter set to 0 | Workers block forever | Config validation: min 1 MB/s |

### 6.1 Handling Rapid Ring Changes

```
Problem: Node D joins at t=0, Node E joins at t=2s.
  Migration plan for D's join is still running.

  Approach: "generation" counter on migration plans.

  on_ring_change():
    1. Increment generation
    2. Cancel all workers (via CancellationToken)
    3. Wait for workers to drain (with 5s timeout)
    4. Recompute plan against latest ring
    5. Spawn new workers

  Workers check generation before each transfer:
    if current_generation != my_generation { return; }

  This ensures stale workers don't send to wrong targets.
```

### 6.2 Delete-After-Transfer Safety

```
Problem: A transfers blob_X to D, then deletes local copy.
  But what if D crashes before persisting blob_X?

  Solution: two-phase confirmation.
  1. A streams blob_X to D
  2. D persists to disk, verifies CAddr, returns accepted=true
  3. A receives accepted=true → deletes local copy

  If D crashes between receive and persist:
    D never sends accepted → A keeps local copy → safe.

  If A crashes after D's accepted but before local delete:
    A restarts, checkpoint shows blob_X completed.
    A still has local copy (delete didn't happen).
    Next ring check: A no longer owns blob_X → GC will clean it.
    Or: resume logic re-checks and deletes.
```

---

## 7. Performance Analysis

### 7.1 Migration Throughput

```
┌──────────────────────────────┬──────────────────────────────┐
│ Configuration                │ Throughput                   │
├──────────────────────────────┼──────────────────────────────┤
│ 4 streams × 50 MB/s limit   │ 50 MB/s (rate limited)       │
│ 4 streams × unlimited       │ ~400 MB/s (network bound)    │
│ 1 stream × 50 MB/s limit    │ 50 MB/s (single stream)      │
├──────────────────────────────┼──────────────────────────────┤
│ 10 GB migration @ 50 MB/s   │ ~200 seconds (3.3 min)       │
│ 100 GB migration @ 50 MB/s  │ ~2000 seconds (33 min)       │
│ 1 TB migration @ 50 MB/s    │ ~20000 seconds (5.5 hours)   │
└──────────────────────────────┴──────────────────────────────┘
```

### 7.2 Impact on Client Latency

```
Without rate limiting:
  Migration saturates network → client p99 latency spikes 10x

With 50 MB/s limit on 1 Gbps link:
  Migration uses 40% bandwidth → client latency increase ~20%

With 10 MB/s limit on 1 Gbps link:
  Migration uses 8% bandwidth → client latency increase <5%

  Recommendation: default 50 MB/s, configurable per deployment.
  Operators with dedicated migration NICs can increase.
```

### 7.3 Memory Usage During Migration

```
Per outbound worker:
  - 1 blob in memory at a time (streaming)
  - Chunk buffer: 256 KB
  - 4 workers: 1 MB total

Per inbound transfer:
  - BlobAssembler: accumulates full blob
  - Worst case: 1 GB blob in memory
  - Mitigation: reject blobs > max_blob_size (configurable, default 256 MB)

  Typical: <10 MB memory overhead for migration.
```

### 7.4 Benchmarking Plan

```rust
/// Benchmark: migration plan computation
#[bench]
fn bench_compute_plan_10k_blobs(b: &mut Bencher) {
    // 10K blobs, 3→4 node ring change
    // Expected: <100ms
}

/// Benchmark: single blob transfer (1MB)
#[bench]
fn bench_transfer_1mb_blob(b: &mut Bencher) {
    // Loopback transfer, no rate limit
    // Expected: <10ms
}

/// Benchmark: rate limiter overhead
#[bench]
fn bench_rate_limiter_acquire(b: &mut Bencher) {
    // 1000 acquire(256KB) calls
    // Expected: <1μs per call (when tokens available)
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/migration.rs` | **NEW** — MigrationManager, MigrationTask, MigrationConfig |
| `deriva-network/src/rate_limiter.rs` | **NEW** — RateLimiter (token bucket) |
| `deriva-network/src/blob_assembler.rs` | **NEW** — BlobAssembler for inbound streams |
| `deriva-network/src/lib.rs` | Add `pub mod migration`, `pub mod rate_limiter`, `pub mod blob_assembler` |
| `proto/deriva_internal.proto` | Add `MigrateTransfer`, `MigrateStatus` RPCs |
| `deriva-server/src/internal.rs` | Add `migrate_transfer`, `migrate_status` handlers |
| `deriva-server/src/state.rs` | Add `migration_manager: Arc<MigrationManager>` |
| `deriva-server/src/main.rs` | Initialize MigrationManager, hook into SWIM membership changes |
| `deriva-storage/src/blob_store.rs` | Add `all_addrs()`, `blob_size()`, `put_raw_with_addr()` |
| `deriva-network/src/hash_ring.rs` | Add `version()` method, ring change callback |
| `deriva-network/tests/migration.rs` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

| Crate | Version | Purpose |
|-------|---------|---------|
| `serde_json` | 1.x | Checkpoint serialization (already in workspace) |

No new external dependencies.

---

## 10. Design Rationale

### 10.1 Why Push-Based Instead of Pull-Based Migration?

```
Pull-based: new node scans ring, requests blobs from old owners.
  + New node controls its own onboarding
  - New node doesn't know what blobs exist on old owners
  - Requires "list all blobs in range" RPC (expensive)

Push-based: old owner scans local blobs, sends to new owner.
  + Old owner knows exactly what it has
  + No expensive listing RPC needed
  + Natural: the node losing ownership initiates transfer
  - Old owner must be alive (handled by replication)

  Push-based wins for simplicity and efficiency.
```

### 10.2 Why Streaming Instead of Bulk Transfer?

```
Bulk: serialize all blobs into one large message.
  - gRPC message size limits (4MB default)
  - All-or-nothing: one failure = retransmit everything
  - Memory: entire batch in memory

Streaming: send blobs one at a time, chunked.
  + No message size limits
  + Per-blob retry granularity
  + Bounded memory (one blob at a time)
  + Rate limiting per chunk

  Streaming is strictly better for large migrations.
```

### 10.3 Why Checkpoint to Disk Instead of In-Memory Only?

```
In-memory: fast, but lost on crash.
  After crash, must recompute entire plan and re-transfer
  everything (including already-transferred blobs).

Disk checkpoint: survives crash.
  After restart, skip already-completed transfers.
  For a 100GB migration that's 80% done, saves re-transferring 80GB.

  Cost: one JSON write every 30 seconds. Negligible.
```

### 10.4 Why Not Use Consistent Hashing Virtual Nodes to Minimize Movement?

```
We DO use virtual nodes (§3.4: 64 vnodes per physical node).
This already minimizes data movement on ring changes:
  - Adding 1 node to a 3-node cluster moves ~25% of data (optimal).
  - Without vnodes, movement could be up to 100% for one node.

The migration manager handles whatever movement the ring dictates.
Virtual nodes reduce the AMOUNT; migration handles the MECHANICS.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `migration_tasks_total` | Counter | `state={completed,failed}` | Task outcomes |
| `migration_bytes_transferred` | Counter | `direction={outbound,inbound}` | Bytes moved |
| `migration_active_streams` | Gauge | — | Current concurrent transfers |
| `migration_queue_depth` | Gauge | — | Pending tasks in queue |
| `migration_task_duration_ms` | Histogram | — | Per-blob transfer latency |
| `migration_state` | Gauge | `state` | Current manager state |
| `migration_rate_limiter_wait_ms` | Histogram | — | Time waiting for rate limit tokens |
| `migration_checkpoints_saved` | Counter | — | Checkpoint writes |
| `migration_blobs_deleted_after_transfer` | Counter | — | Source copies removed |

### 11.2 Structured Logging

```rust
tracing::info!(
    ring_version = new_ring.version(),
    outbound_tasks = plan.len(),
    total_bytes = plan.iter().map(|t| t.blob_size).sum::<u64>(),
    "migration plan computed"
);

tracing::info!(
    blob = %task.blob_addr,
    target = %task.target,
    size = task.blob_size,
    "blob transfer complete"
);

tracing::warn!(
    blob = %task.blob_addr,
    target = %task.target,
    error = %e,
    retries = retries,
    "blob transfer failed, retrying"
);

tracing::info!(
    completed = progress.completed_tasks.load(Ordering::Relaxed),
    total = progress.total_tasks.load(Ordering::Relaxed),
    bytes = progress.bytes_transferred.load(Ordering::Relaxed),
    "migration progress update"
);
```

### 11.3 Admin API Integration (§3.15)

```
GET /cluster/migration
{
  "state": "transferring",
  "total_tasks": 1500,
  "completed_tasks": 1200,
  "failed_tasks": 3,
  "bytes_transferred": 12884901888,
  "bytes_remaining": 3221225472,
  "eta_seconds": 64,
  "rate_bytes_per_sec": 52428800,
  "active_streams": 4
}
```

### 11.4 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| Migration stalled | `migration_tasks_total` unchanged for 5 min while queue > 0 | Warning |
| High failure rate | `migration_tasks_total{state=failed}` > 10% of total | Warning |
| Migration slow | ETA > 1 hour | Info |
| Rate limiter saturated | `migration_rate_limiter_wait_ms` p99 > 1s | Info |

---

## 12. Checklist

- [ ] Create `deriva-network/src/migration.rs`
- [ ] Implement `MigrationManager` with plan computation
- [ ] Implement `compute_plan` (diff old/new ring ownership)
- [ ] Implement `transfer_blob` (streaming gRPC client)
- [ ] Implement `transfer_loop` (worker with retry logic)
- [ ] Create `deriva-network/src/rate_limiter.rs`
- [ ] Implement token-bucket `RateLimiter`
- [ ] Create `deriva-network/src/blob_assembler.rs`
- [ ] Implement `BlobAssembler` for inbound chunk reassembly
- [ ] Add `MigrateTransfer` streaming RPC to proto
- [ ] Add `MigrateStatus` RPC to proto
- [ ] Implement `migrate_transfer` handler (inbound)
- [ ] Implement `migrate_status` handler
- [ ] Add `all_addrs()`, `blob_size()`, `put_raw_with_addr()` to BlobStore
- [ ] Add ring `version()` and change callback
- [ ] Implement checkpoint save/load/resume
- [ ] Hook `on_ring_change` into SWIM membership events
- [ ] Add CancellationToken for plan invalidation on rapid ring changes
- [ ] Write plan computation tests (3 tests)
- [ ] Write rate limiter tests (3 tests)
- [ ] Write blob assembler tests (4 tests)
- [ ] Write checkpoint tests (3 tests)
- [ ] Write integration tests (3 tests)
- [ ] Add metrics (9 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (4 alerts)
- [ ] Run benchmarks: plan computation, transfer throughput, rate limiter
- [ ] Config validation: min rate 1 MB/s, max_retries >= 1
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
