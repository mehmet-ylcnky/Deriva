# §3.14 Batch Operations

> **Status:** Not started
> **Depends on:** §3.6 (distributed get), §3.3 (leaf replication), §3.13 (connection pooling)
> **Crate(s):** `deriva-network`, `deriva-server`, `deriva-core`
> **Estimated effort:** 2–3 days

---

## 1. Problem Statement

Many workloads require fetching or storing multiple values in a single logical
operation. Without batch support:

1. **Round-trip amplification**: fetching 100 leaves requires 100 sequential RPCs.
   At 2ms per RPC (including network), that's 200ms total. A single batch RPC
   with scatter-gather can complete in ~10ms (one round-trip + parallel fan-out).

2. **Connection overhead**: 100 individual RPCs create 100 HTTP/2 streams
   sequentially. A batch creates 1 stream with internal parallelism.

3. **Partial failure complexity**: without batch semantics, the caller must
   implement retry logic for each individual operation. A batch API provides
   unified partial-failure reporting.

4. **Pipeline stalls**: a computation that needs inputs A, B, C must wait for
   each sequentially. BatchGet returns all three in one call, enabling the
   computation to start immediately.

### Goals

- **BatchGet**: fetch multiple values by CAddr in a single RPC. Scatter to
  owning nodes, gather results. Return partial results on partial failure.
- **BatchPutLeaf**: store multiple leaf values in a single RPC. Scatter to
  replica sets, gather acknowledgments.
- **Bounded concurrency**: max 32 concurrent sub-requests per batch to prevent
  resource exhaustion.
- **Streaming response**: results stream back as they complete, not all-at-once.
  Caller can start processing early results while later ones are still in flight.
- **Size limits**: max 1000 items per batch, max 64MB total payload.
- **Partial failure**: each item in the batch has its own status. The batch
  succeeds even if some items fail.

### Non-Goals

- BatchPutRecipe (recipes are small and infrequent — individual puts suffice).
- Transactional batches (all-or-nothing semantics).
- Cross-batch ordering guarantees.

---

## 2. Design

### 2.1 Architecture Overview

```
  Client
    │
    │ BatchGet([A, B, C, D, E])
    ▼
  ┌──────────────────────────────────────────────┐
  │  Coordinator Node (receives batch RPC)       │
  │                                              │
  │  1. Partition by owner:                      │
  │     Node X owns: [A, C]                      │
  │     Node Y owns: [B, E]                      │
  │     Local:       [D]                         │
  │                                              │
  │  2. Scatter (parallel, bounded):             │
  │     ├── FetchValue(A) → X ──┐               │
  │     ├── FetchValue(C) → X ──┤               │
  │     ├── FetchValue(B) → Y ──┤  concurrency  │
  │     ├── FetchValue(E) → Y ──┤  semaphore(32)│
  │     └── local_get(D) ──────┤               │
  │                              │               │
  │  3. Gather + stream back:    │               │
  │     ◄── result(D) ──────────┘               │
  │     ◄── result(A) ──────────                │
  │     ◄── result(B) ──────────                │
  │     ◄── result(C) ──────────                │
  │     ◄── result(E) ──────────                │
  └──────────────────────────────────────────────┘
    │
    │ Stream<BatchGetResponse>
    ▼
  Client receives results as they arrive
```

### 2.2 Wire Protocol

```protobuf
// In deriva.proto (client-facing)

message BatchGetRequest {
    repeated bytes addrs = 1;           // CAddrs to fetch
    uint32 max_concurrency = 2;         // optional override (default: 32)
}

message BatchGetResponse {
    bytes addr = 1;                     // which CAddr this result is for
    oneof result {
        bytes data = 2;                 // success: the value bytes
        BatchItemError error = 3;       // failure for this item
    }
    uint32 index = 4;                   // position in original request
}

message BatchItemError {
    BatchErrorCode code = 1;
    string message = 2;
}

enum BatchErrorCode {
    NOT_FOUND = 0;
    TIMEOUT = 1;
    UNAVAILABLE = 2;
    INTERNAL = 3;
}

message BatchPutLeafRequest {
    repeated BatchPutItem items = 1;
    uint32 max_concurrency = 2;
}

message BatchPutItem {
    bytes data = 1;                     // leaf value bytes
    string client_id = 2;              // caller-assigned ID for correlation
}

message BatchPutLeafResponse {
    string client_id = 1;              // correlates to request item
    oneof result {
        bytes addr = 2;                // success: computed CAddr
        BatchItemError error = 3;      // failure for this item
    }
    uint32 index = 4;
}

service Deriva {
    // ... existing RPCs ...
    rpc BatchGet(BatchGetRequest) returns (stream BatchGetResponse);
    rpc BatchPutLeaf(BatchPutLeafRequest) returns (stream BatchPutLeafResponse);
}
```

### 2.3 Core Types

```rust
/// Orchestrates batch operations with scatter-gather.
pub struct BatchExecutor {
    channel_pool: Arc<ChannelPool>,
    ring: Arc<RwLock<HashRing>>,
    local_store: Arc<BlobStore>,
    recipe_store: Arc<SledRecipeStore>,
    config: BatchConfig,
}

#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Max items per batch request.
    pub max_batch_size: usize,          // default: 1000
    /// Max total payload bytes per batch.
    pub max_batch_bytes: u64,           // default: 64 MB
    /// Default concurrent sub-requests.
    pub default_concurrency: usize,     // default: 32
    /// Max concurrent sub-requests (hard cap).
    pub max_concurrency: usize,         // default: 64
    /// Per-item timeout.
    pub item_timeout: Duration,         // default: 10s
    /// Overall batch timeout.
    pub batch_timeout: Duration,        // default: 60s
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 1000,
            max_batch_bytes: 64 * 1024 * 1024,
            default_concurrency: 32,
            max_concurrency: 64,
            item_timeout: Duration::from_secs(10),
            batch_timeout: Duration::from_secs(60),
        }
    }
}

/// Result for a single item in a batch.
#[derive(Debug)]
pub enum BatchItemResult {
    Success { addr: CAddr, data: Vec<u8> },
    NotFound { addr: CAddr },
    Error { addr: CAddr, code: BatchErrorCode, message: String },
}

/// Tracks batch progress.
#[derive(Debug)]
pub struct BatchProgress {
    pub total: usize,
    pub completed: AtomicUsize,
    pub succeeded: AtomicUsize,
    pub failed: AtomicUsize,
}
```

### 2.4 Partitioning Strategy

```
  Given addrs = [A, B, C, D, E] and ring with nodes {X, Y, local}:

  Step 1: Compute primary owner for each addr
    ring.primary(A) = X
    ring.primary(B) = Y
    ring.primary(C) = X
    ring.primary(D) = local
    ring.primary(E) = Y

  Step 2: Group by owner
    local:  [(D, 3)]           // (addr, original_index)
    X:      [(A, 0), (C, 2)]
    Y:      [(B, 1), (E, 4)]

  Step 3: For each group, issue sub-requests in parallel
    Local items: resolve directly (no RPC)
    Remote items: FetchValue RPC via channel pool

  Optimization: if multiple items go to same peer,
  we could batch them in a single RPC. But FetchValue
  is per-item, so we issue parallel individual RPCs.
  (Future: add BatchFetchValue internal RPC for peer-to-peer batching)
```

---

## 3. Implementation

### 3.1 BatchExecutor — BatchGet

```rust
impl BatchExecutor {
    pub fn new(
        channel_pool: Arc<ChannelPool>,
        ring: Arc<RwLock<HashRing>>,
        local_store: Arc<BlobStore>,
        recipe_store: Arc<SledRecipeStore>,
        config: BatchConfig,
    ) -> Self {
        Self { channel_pool, ring, local_store, recipe_store, config }
    }

    /// Execute a BatchGet, streaming results via the sender.
    pub async fn batch_get(
        &self,
        addrs: Vec<CAddr>,
        concurrency: usize,
        tx: mpsc::Sender<BatchGetResponse>,
    ) -> Result<(), DerivaError> {
        self.validate_batch_size(addrs.len())?;

        let concurrency = concurrency.min(self.config.max_concurrency);
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let progress = Arc::new(BatchProgress::new(addrs.len()));

        let ring = self.ring.read().await;
        let local_id = self.channel_pool.local_id();

        let mut handles = Vec::with_capacity(addrs.len());

        for (index, addr) in addrs.into_iter().enumerate() {
            let sem = semaphore.clone();
            let tx = tx.clone();
            let progress = progress.clone();
            let owner = ring.primary(&addr);

            if owner == local_id {
                let store = self.local_store.clone();
                let recipe_store = self.recipe_store.clone();
                let timeout = self.config.item_timeout;
                handles.push(tokio::spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    let result = tokio::time::timeout(
                        timeout,
                        Self::resolve_local(&store, &recipe_store, &addr),
                    ).await;
                    let response = match result {
                        Ok(Ok(data)) => {
                            progress.succeeded.fetch_add(1, Ordering::Relaxed);
                            BatchGetResponse {
                                addr: addr.as_bytes().to_vec(),
                                result: Some(batch_get_response::Result::Data(data)),
                                index: index as u32,
                            }
                        }
                        Ok(Err(e)) => {
                            progress.failed.fetch_add(1, Ordering::Relaxed);
                            Self::error_response(&addr, index, e)
                        }
                        Err(_) => {
                            progress.failed.fetch_add(1, Ordering::Relaxed);
                            Self::timeout_response(&addr, index)
                        }
                    };
                    progress.completed.fetch_add(1, Ordering::Relaxed);
                    let _ = tx.send(response).await;
                }));
            } else {
                let pool = self.channel_pool.clone();
                let timeout = self.config.item_timeout;
                handles.push(tokio::spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    let result = tokio::time::timeout(
                        timeout,
                        Self::fetch_remote(&pool, &owner, &addr),
                    ).await;
                    let response = match result {
                        Ok(Ok(data)) => {
                            progress.succeeded.fetch_add(1, Ordering::Relaxed);
                            BatchGetResponse {
                                addr: addr.as_bytes().to_vec(),
                                result: Some(batch_get_response::Result::Data(data)),
                                index: index as u32,
                            }
                        }
                        Ok(Err(e)) => {
                            progress.failed.fetch_add(1, Ordering::Relaxed);
                            Self::error_response(&addr, index, e)
                        }
                        Err(_) => {
                            progress.failed.fetch_add(1, Ordering::Relaxed);
                            Self::timeout_response(&addr, index)
                        }
                    };
                    progress.completed.fetch_add(1, Ordering::Relaxed);
                    let _ = tx.send(response).await;
                }));
            }
        }

        // Wait for all items (with overall batch timeout)
        let _ = tokio::time::timeout(
            self.config.batch_timeout,
            futures::future::join_all(handles),
        ).await;

        Ok(())
    }

    async fn resolve_local(
        store: &BlobStore,
        recipe_store: &SledRecipeStore,
        addr: &CAddr,
    ) -> Result<Vec<u8>, DerivaError> {
        // Try blob store first (leaf data)
        if let Some(data) = store.get_raw(addr).await? {
            return Ok(data);
        }
        // Try recipe store (materialized value)
        if let Some(recipe) = recipe_store.get(addr)? {
            // Return recipe bytes (caller can materialize if needed)
            return Ok(bincode::serialize(&recipe)?);
        }
        Err(DerivaError::NotFound(addr.to_string()))
    }

    async fn fetch_remote(
        pool: &ChannelPool,
        owner: &NodeId,
        addr: &CAddr,
    ) -> Result<Vec<u8>, DerivaError> {
        let mut client = pool.internal_client(owner).await?;
        let response = client.fetch_value(FetchValueRequest {
            addr: addr.as_bytes().to_vec(),
        }).await?;
        Ok(response.into_inner().data)
    }

    fn error_response(addr: &CAddr, index: usize, e: DerivaError) -> BatchGetResponse {
        let (code, msg) = match &e {
            DerivaError::NotFound(_) => (BatchErrorCode::NotFound, e.to_string()),
            DerivaError::Network(_) => (BatchErrorCode::Unavailable, e.to_string()),
            _ => (BatchErrorCode::Internal, e.to_string()),
        };
        BatchGetResponse {
            addr: addr.as_bytes().to_vec(),
            result: Some(batch_get_response::Result::Error(BatchItemError {
                code: code as i32,
                message: msg,
            })),
            index: index as u32,
        }
    }

    fn timeout_response(addr: &CAddr, index: usize) -> BatchGetResponse {
        BatchGetResponse {
            addr: addr.as_bytes().to_vec(),
            result: Some(batch_get_response::Result::Error(BatchItemError {
                code: BatchErrorCode::Timeout as i32,
                message: "item timeout".into(),
            })),
            index: index as u32,
        }
    }

    fn validate_batch_size(&self, count: usize) -> Result<(), DerivaError> {
        if count == 0 {
            return Err(DerivaError::InvalidArgument("empty batch".into()));
        }
        if count > self.config.max_batch_size {
            return Err(DerivaError::InvalidArgument(format!(
                "batch size {} exceeds max {}", count, self.config.max_batch_size
            )));
        }
        Ok(())
    }
}
```

### 3.2 BatchExecutor — BatchPutLeaf

```rust
impl BatchExecutor {
    /// Execute a BatchPutLeaf, streaming results via the sender.
    pub async fn batch_put_leaf(
        &self,
        items: Vec<BatchPutItem>,
        concurrency: usize,
        tx: mpsc::Sender<BatchPutLeafResponse>,
    ) -> Result<(), DerivaError> {
        self.validate_batch_size(items.len())?;
        self.validate_batch_bytes(&items)?;

        let concurrency = concurrency.min(self.config.max_concurrency);
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let progress = Arc::new(BatchProgress::new(items.len()));

        let mut handles = Vec::with_capacity(items.len());

        for (index, item) in items.into_iter().enumerate() {
            let sem = semaphore.clone();
            let tx = tx.clone();
            let progress = progress.clone();
            let store = self.local_store.clone();
            let ring = self.ring.clone();
            let pool = self.channel_pool.clone();
            let timeout = self.config.item_timeout;

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let result = tokio::time::timeout(
                    timeout,
                    Self::put_single_leaf(&store, &ring, &pool, &item.data),
                ).await;

                let response = match result {
                    Ok(Ok(addr)) => {
                        progress.succeeded.fetch_add(1, Ordering::Relaxed);
                        BatchPutLeafResponse {
                            client_id: item.client_id,
                            result: Some(batch_put_leaf_response::Result::Addr(
                                addr.as_bytes().to_vec(),
                            )),
                            index: index as u32,
                        }
                    }
                    Ok(Err(e)) => {
                        progress.failed.fetch_add(1, Ordering::Relaxed);
                        BatchPutLeafResponse {
                            client_id: item.client_id,
                            result: Some(batch_put_leaf_response::Result::Error(
                                BatchItemError {
                                    code: BatchErrorCode::Internal as i32,
                                    message: e.to_string(),
                                },
                            )),
                            index: index as u32,
                        }
                    }
                    Err(_) => {
                        progress.failed.fetch_add(1, Ordering::Relaxed);
                        BatchPutLeafResponse {
                            client_id: item.client_id,
                            result: Some(batch_put_leaf_response::Result::Error(
                                BatchItemError {
                                    code: BatchErrorCode::Timeout as i32,
                                    message: "item timeout".into(),
                                },
                            )),
                            index: index as u32,
                        }
                    }
                };
                progress.completed.fetch_add(1, Ordering::Relaxed);
                let _ = tx.send(response).await;
            }));
        }

        let _ = tokio::time::timeout(
            self.config.batch_timeout,
            futures::future::join_all(handles),
        ).await;

        Ok(())
    }

    async fn put_single_leaf(
        store: &BlobStore,
        ring: &RwLock<HashRing>,
        pool: &ChannelPool,
        data: &[u8],
    ) -> Result<CAddr, DerivaError> {
        let addr = CAddr::hash(data);
        store.put_raw_with_addr(&addr, data).await?;

        // Replicate to replica set
        let ring = ring.read().await;
        let replicas = ring.replicas(&addr);
        let local_id = pool.local_id();

        for replica in replicas {
            if replica != local_id {
                let mut client = pool.internal_client(&replica).await?;
                client.replicate_leaf(ReplicateLeafRequest {
                    addr: addr.as_bytes().to_vec(),
                    data: data.to_vec(),
                }).await?;
            }
        }

        Ok(addr)
    }

    fn validate_batch_bytes(&self, items: &[BatchPutItem]) -> Result<(), DerivaError> {
        let total: u64 = items.iter().map(|i| i.data.len() as u64).sum();
        if total > self.config.max_batch_bytes {
            return Err(DerivaError::InvalidArgument(format!(
                "batch payload {} bytes exceeds max {} bytes",
                total, self.config.max_batch_bytes
            )));
        }
        Ok(())
    }
}

impl BatchProgress {
    pub fn new(total: usize) -> Self {
        Self {
            total,
            completed: AtomicUsize::new(0),
            succeeded: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
        }
    }
}
```

### 3.3 gRPC Handler

```rust
/// Server-side handler for BatchGet.
pub async fn batch_get(
    &self,
    request: Request<BatchGetRequest>,
) -> Result<Response<ResponseStream<BatchGetResponse>>, Status> {
    let req = request.into_inner();
    let addrs: Vec<CAddr> = req.addrs.iter()
        .map(|b| CAddr::from_bytes(b))
        .collect();

    let concurrency = if req.max_concurrency > 0 {
        req.max_concurrency as usize
    } else {
        self.batch_executor.config.default_concurrency
    };

    let (tx, rx) = mpsc::channel(64);

    let executor = self.batch_executor.clone();
    tokio::spawn(async move {
        if let Err(e) = executor.batch_get(addrs, concurrency, tx).await {
            tracing::error!(error = %e, "batch_get failed");
        }
    });

    let stream = ReceiverStream::new(rx);
    Ok(Response::new(Box::pin(stream)))
}

/// Server-side handler for BatchPutLeaf.
pub async fn batch_put_leaf(
    &self,
    request: Request<BatchPutLeafRequest>,
) -> Result<Response<ResponseStream<BatchPutLeafResponse>>, Status> {
    let req = request.into_inner();

    let concurrency = if req.max_concurrency > 0 {
        req.max_concurrency as usize
    } else {
        self.batch_executor.config.default_concurrency
    };

    let (tx, rx) = mpsc::channel(64);

    let executor = self.batch_executor.clone();
    tokio::spawn(async move {
        if let Err(e) = executor.batch_put_leaf(req.items, concurrency, tx).await {
            tracing::error!(error = %e, "batch_put_leaf failed");
        }
    });

    let stream = ReceiverStream::new(rx);
    Ok(Response::new(Box::pin(stream)))
}
```


---

## 4. Data Flow Diagrams

### 4.1 BatchGet — Happy Path (5 items, 3 nodes)

```
  Client                  Coordinator (Node A)        Node B          Node C
    │                           │                       │               │
    │── BatchGet([1,2,3,4,5]) ─►│                       │               │
    │                           │                       │               │
    │                     Partition:                     │               │
    │                       local: [3]                  │               │
    │                       B: [1, 4]                   │               │
    │                       C: [2, 5]                   │               │
    │                           │                       │               │
    │                     Scatter (semaphore=32):        │               │
    │                     ├── local_get(3) ─────────┐   │               │
    │                     ├── FetchValue(1) ────────┼──►│               │
    │                     ├── FetchValue(4) ────────┼──►│               │
    │                     ├── FetchValue(2) ────────┼───┼──────────────►│
    │                     └── FetchValue(5) ────────┼───┼──────────────►│
    │                           │                   │   │               │
    │◄── result(3, data) ──────┘ (local, fastest)  │   │               │
    │◄── result(1, data) ──────────────────────────┘   │               │
    │◄── result(2, data) ──────────────────────────────┘               │
    │◄── result(4, data) ──────────────────────────────                │
    │◄── result(5, data) ──────────────────────────────────────────────┘
    │                           │
    │  (stream complete)        │
```

### 4.2 BatchGet — Partial Failure

```
  Client                  Coordinator              Node B (slow)    Node C (down)
    │                           │                       │               │
    │── BatchGet([1,2,3]) ─────►│                       │               │
    │                           │                       │               │
    │                     Partition:                     │               │
    │                       B: [1]                      │               │
    │                       C: [2]                      │               │
    │                       local: [3]                  │               │
    │                           │                       │               │
    │◄── result(3, data) ──────┘ (local, instant)      │               │
    │                           │                       │               │
    │                     FetchValue(1) → B ───────────►│               │
    │                     FetchValue(2) → C ────────────┼──── X timeout │
    │                           │                       │               │
    │                     ... 10s timeout for item 2 ...│               │
    │                           │                       │               │
    │◄── result(1, data) ──────┘ (B responded)         │               │
    │◄── result(2, TIMEOUT) ───┘ (C unreachable)       │               │
    │                           │                       │               │
    │  Batch complete: 2/3 succeeded, 1/3 failed       │               │
    │  Client decides: retry item 2, or proceed without│               │
```

### 4.3 BatchPutLeaf — Scatter to Replica Sets

```
  Client                  Coordinator              Node B          Node C
    │                           │                       │               │
    │── BatchPutLeaf(          │                       │               │
    │     [data_X, data_Y])  ──►│                       │               │
    │                           │                       │               │
    │                     For data_X:                   │               │
    │                       addr_X = hash(data_X)       │               │
    │                       store locally               │               │
    │                       replicas = [A, B]           │               │
    │                       Replicate(X) → B ──────────►│               │
    │                           │                       │               │
    │                     For data_Y:                   │               │
    │                       addr_Y = hash(data_Y)       │               │
    │                       store locally               │               │
    │                       replicas = [A, C]           │               │
    │                       Replicate(Y) → C ──────────────────────────►│
    │                           │                       │               │
    │◄── result(X, addr_X) ────┘                       │               │
    │◄── result(Y, addr_Y) ────┘                       │               │
    │                           │                       │               │
    │  Both items stored + replicated                   │               │
```

### 4.4 Concurrency Semaphore Throttling

```
  BatchGet with 100 items, concurrency=32:

  ┌─────────────────────────────────────────────────────────┐
  │ Semaphore(32)                                           │
  │                                                         │
  │ t=0ms:   items 1–32 acquire permits → start fetching    │
  │ t=5ms:   item 3 completes → permit released             │
  │          item 33 acquires permit → starts                │
  │ t=6ms:   item 7 completes → item 34 starts              │
  │ ...                                                     │
  │ t=50ms:  items 1–64 complete, items 65–96 in flight     │
  │ t=100ms: all 100 items complete                         │
  │                                                         │
  │ Without semaphore: 100 concurrent RPCs → overwhelm peer │
  │ With semaphore: max 32 concurrent → controlled load     │
  └─────────────────────────────────────────────────────────┘
```

### 4.5 Batch Timeout vs Item Timeout

```
  BatchGet with 50 items:
    item_timeout = 10s
    batch_timeout = 60s

  Scenario 1: All items fast (5ms each)
    Total: ~50ms (parallel). Well within both timeouts.

  Scenario 2: One item slow (15s), rest fast
    Fast items: complete in ~50ms, streamed to client
    Slow item: exceeds item_timeout (10s) → TIMEOUT error
    Batch completes at t=10s. 49 success, 1 timeout.

  Scenario 3: Many items slow (network partition)
    Items trickle in slowly, 1/sec
    At t=60s: batch_timeout fires
    Items completed so far: streamed to client
    Remaining items: cancelled (tasks dropped)
    Client gets partial results + knows batch timed out.
```

---

## 5. Test Specification

### 5.1 BatchGet Tests

```rust
#[cfg(test)]
mod batch_get_tests {
    use super::*;

    async fn setup_executor(blobs: &[(&str, &[u8])]) -> BatchExecutor {
        let store = Arc::new(BlobStore::in_memory());
        for (addr_str, data) in blobs {
            let addr = CAddr::from_hex(addr_str);
            store.put_raw_with_addr(&addr, data).await.unwrap();
        }
        BatchExecutor::new(
            mock_channel_pool(),
            mock_ring_local_only(),
            store,
            mock_recipe_store(),
            BatchConfig::default(),
        )
    }

    #[tokio::test]
    async fn test_batch_get_all_local() {
        let addr_a = CAddr::hash(b"aaa");
        let addr_b = CAddr::hash(b"bbb");
        let exec = setup_executor(&[
            (&addr_a.to_hex(), b"aaa"),
            (&addr_b.to_hex(), b"bbb"),
        ]).await;

        let (tx, mut rx) = mpsc::channel(16);
        exec.batch_get(vec![addr_a, addr_b], 32, tx).await.unwrap();

        let mut results = Vec::new();
        while let Some(resp) = rx.recv().await {
            results.push(resp);
        }

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.result.as_ref().unwrap().is_data()));
    }

    #[tokio::test]
    async fn test_batch_get_not_found() {
        let exec = setup_executor(&[]).await;
        let missing = CAddr::hash(b"missing");

        let (tx, mut rx) = mpsc::channel(16);
        exec.batch_get(vec![missing], 32, tx).await.unwrap();

        let resp = rx.recv().await.unwrap();
        match resp.result.unwrap() {
            batch_get_response::Result::Error(e) => {
                assert_eq!(e.code, BatchErrorCode::NotFound as i32);
            }
            _ => panic!("expected error"),
        }
    }

    #[tokio::test]
    async fn test_batch_get_empty_rejected() {
        let exec = setup_executor(&[]).await;
        let (tx, _rx) = mpsc::channel(16);
        let result = exec.batch_get(vec![], 32, tx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_batch_get_exceeds_max_size() {
        let exec = BatchExecutor::new(
            mock_channel_pool(),
            mock_ring_local_only(),
            Arc::new(BlobStore::in_memory()),
            mock_recipe_store(),
            BatchConfig { max_batch_size: 5, ..Default::default() },
        );

        let addrs: Vec<CAddr> = (0..10).map(|i| CAddr::hash(&[i])).collect();
        let (tx, _rx) = mpsc::channel(16);
        let result = exec.batch_get(addrs, 32, tx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_batch_get_preserves_index() {
        let addr_a = CAddr::hash(b"first");
        let addr_b = CAddr::hash(b"second");
        let exec = setup_executor(&[
            (&addr_a.to_hex(), b"first"),
            (&addr_b.to_hex(), b"second"),
        ]).await;

        let (tx, mut rx) = mpsc::channel(16);
        exec.batch_get(vec![addr_a, addr_b], 32, tx).await.unwrap();

        let mut results = Vec::new();
        while let Some(resp) = rx.recv().await {
            results.push(resp);
        }

        // Results may arrive out of order, but index field preserves position
        for r in &results {
            assert!(r.index == 0 || r.index == 1);
        }
        let indices: HashSet<u32> = results.iter().map(|r| r.index).collect();
        assert_eq!(indices.len(), 2);
    }
}
```

### 5.2 BatchPutLeaf Tests

```rust
#[tokio::test]
async fn test_batch_put_leaf_stores_all() {
    let store = Arc::new(BlobStore::in_memory());
    let exec = BatchExecutor::new(
        mock_channel_pool(),
        mock_ring_local_only(),
        store.clone(),
        mock_recipe_store(),
        BatchConfig::default(),
    );

    let items = vec![
        BatchPutItem { data: b"leaf_1".to_vec(), client_id: "c1".into() },
        BatchPutItem { data: b"leaf_2".to_vec(), client_id: "c2".into() },
    ];

    let (tx, mut rx) = mpsc::channel(16);
    exec.batch_put_leaf(items, 32, tx).await.unwrap();

    let mut results = Vec::new();
    while let Some(resp) = rx.recv().await {
        results.push(resp);
    }

    assert_eq!(results.len(), 2);
    for r in &results {
        let addr_bytes = match r.result.as_ref().unwrap() {
            batch_put_leaf_response::Result::Addr(a) => a,
            _ => panic!("expected addr"),
        };
        let addr = CAddr::from_bytes(addr_bytes);
        assert!(store.exists(&addr).await);
    }
}

#[tokio::test]
async fn test_batch_put_leaf_payload_limit() {
    let exec = BatchExecutor::new(
        mock_channel_pool(),
        mock_ring_local_only(),
        Arc::new(BlobStore::in_memory()),
        mock_recipe_store(),
        BatchConfig { max_batch_bytes: 100, ..Default::default() },
    );

    let items = vec![
        BatchPutItem { data: vec![0u8; 60], client_id: "c1".into() },
        BatchPutItem { data: vec![0u8; 60], client_id: "c2".into() },
    ]; // total 120 > max 100

    let (tx, _rx) = mpsc::channel(16);
    let result = exec.batch_put_leaf(items, 32, tx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_batch_put_leaf_client_id_preserved() {
    let exec = setup_default_executor().await;
    let items = vec![
        BatchPutItem { data: b"x".to_vec(), client_id: "my-id-42".into() },
    ];

    let (tx, mut rx) = mpsc::channel(16);
    exec.batch_put_leaf(items, 32, tx).await.unwrap();

    let resp = rx.recv().await.unwrap();
    assert_eq!(resp.client_id, "my-id-42");
}
```

### 5.3 Concurrency Tests

```rust
#[tokio::test]
async fn test_concurrency_bounded() {
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let current = Arc::new(AtomicUsize::new(0));

    // Mock store that tracks concurrency
    let store = MockBlobStore::new({
        let max_concurrent = max_concurrent.clone();
        let current = current.clone();
        move |_addr| {
            let c = current.fetch_add(1, Ordering::SeqCst) + 1;
            max_concurrent.fetch_max(c, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(10));
            current.fetch_sub(1, Ordering::SeqCst);
            Ok(vec![1, 2, 3])
        }
    });

    let exec = BatchExecutor::new(
        mock_channel_pool(),
        mock_ring_local_only(),
        Arc::new(store),
        mock_recipe_store(),
        BatchConfig { default_concurrency: 4, ..Default::default() },
    );

    let addrs: Vec<CAddr> = (0..20).map(|i| CAddr::hash(&[i])).collect();
    let (tx, mut rx) = mpsc::channel(64);
    exec.batch_get(addrs, 4, tx).await.unwrap();

    while rx.recv().await.is_some() {}

    assert!(max_concurrent.load(Ordering::SeqCst) <= 4);
}
```

### 5.4 Integration Tests

```rust
#[tokio::test]
async fn test_batch_get_distributed() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Put leaves across the cluster
    let mut addrs = Vec::new();
    for i in 0..20 {
        let addr = put_leaf(&nodes[i % 3], format!("data_{}", i).as_bytes()).await;
        addrs.push(addr);
    }
    tokio::time::sleep(Duration::from_secs(3)).await;

    // BatchGet from node 0
    let mut client = connect_client(&nodes[0]).await;
    let response_stream = client.batch_get(BatchGetRequest {
        addrs: addrs.iter().map(|a| a.as_bytes().to_vec()).collect(),
        max_concurrency: 16,
    }).await.unwrap();

    let mut results = Vec::new();
    let mut stream = response_stream.into_inner();
    while let Some(resp) = stream.next().await {
        results.push(resp.unwrap());
    }

    assert_eq!(results.len(), 20);
    let successes = results.iter()
        .filter(|r| matches!(r.result, Some(batch_get_response::Result::Data(_))))
        .count();
    assert_eq!(successes, 20);
}

#[tokio::test]
async fn test_batch_put_leaf_distributed() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    let mut client = connect_client(&nodes[0]).await;
    let items: Vec<BatchPutItem> = (0..10).map(|i| BatchPutItem {
        data: format!("leaf_{}", i).into_bytes(),
        client_id: format!("id_{}", i),
    }).collect();

    let response_stream = client.batch_put_leaf(BatchPutLeafRequest {
        items,
        max_concurrency: 8,
    }).await.unwrap();

    let mut results = Vec::new();
    let mut stream = response_stream.into_inner();
    while let Some(resp) = stream.next().await {
        results.push(resp.unwrap());
    }

    assert_eq!(results.len(), 10);

    // Verify all leaves are retrievable
    for r in &results {
        let addr = match r.result.as_ref().unwrap() {
            batch_put_leaf_response::Result::Addr(a) => CAddr::from_bytes(a),
            _ => panic!("expected addr"),
        };
        let get_result = client.get_value(GetValueRequest {
            addr: addr.as_bytes().to_vec(),
        }).await;
        assert!(get_result.is_ok());
    }
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Empty batch | Reject with InvalidArgument | No work to do |
| 2 | Batch exceeds max_batch_size | Reject with InvalidArgument | Prevent resource exhaustion |
| 3 | Batch exceeds max_batch_bytes | Reject with InvalidArgument (PutLeaf only) | Memory protection |
| 4 | Duplicate addrs in BatchGet | Each resolved independently (may hit cache) | Idempotent, no dedup needed |
| 5 | All items fail | Batch succeeds, all items have error status | Partial failure semantics |
| 6 | Coordinator crashes mid-batch | Client stream ends, client retries | gRPC stream broken |
| 7 | One peer very slow | That item times out (item_timeout), rest succeed | Per-item isolation |
| 8 | batch_timeout before all items | Completed items streamed, rest cancelled | Bounded total time |
| 9 | Client disconnects mid-stream | Sender detects closed channel, tasks cancelled | Tokio drop semantics |
| 10 | max_concurrency = 0 in request | Use default_concurrency | Treat 0 as "use default" |
| 11 | Same data put twice in BatchPutLeaf | Same CAddr computed, idempotent store | Content-addressing dedup |
| 12 | Item data is empty (0 bytes) | Valid: CAddr of empty = hash("") | Edge case but allowed |

### 6.1 Partial Failure Semantics

```
Design choice: batch operations use "best-effort" semantics.

  Option A: All-or-nothing (transactional)
    If any item fails, roll back all items.
    Complex: need distributed rollback for PutLeaf.
    Wasteful: 99 successful items discarded because 1 failed.

  Option B: Partial success (our choice)
    Each item independent. Failures reported per-item.
    Client decides how to handle partial results.
    Simple: no rollback needed.

  The client_id field in BatchPutLeaf lets callers correlate
  responses to requests without relying on ordering.
  The index field provides positional correlation for BatchGet.
```

### 6.2 Streaming Backpressure

```
  mpsc::channel(64) between executor and gRPC stream.

  If client reads slowly:
    Channel fills to 64 items.
    Executor tasks block on tx.send().await.
    Semaphore permits not released → new items don't start.
    Natural backpressure: slow client → slow batch.

  If client disconnects:
    tx.send() returns Err (receiver dropped).
    Task logs warning and exits.
    Remaining tasks see closed channel → exit.
    Semaphore permits released → no leak.
```

### 6.3 Deduplication Opportunity

```
  BatchGet([A, A, A, B]):
    Naive: 4 fetches (3 for A, 1 for B).
    Optimized: dedup → fetch A once, B once, return 4 results.

  Not implemented in v1 (simplicity).
  Content-addressing means fetching A twice returns same data.
  The extra fetches hit local cache after first fetch → fast.

  Future optimization: dedup addrs, fan-out results.
  Saves network for remote fetches.
```

---

## 7. Performance Analysis

### 7.1 Latency Comparison

```
┌──────────────────────────────┬──────────┬──────────────────────┐
│ Operation                    │ Latency  │ Notes                │
├──────────────────────────────┼──────────┼──────────────────────┤
│ 100 sequential GetValue      │ ~200ms   │ 100 × 2ms per RPC   │
│ BatchGet(100), concurrency=32│ ~12ms    │ ~4 rounds of 32     │
│ BatchGet(100), concurrency=8 │ ~30ms    │ ~13 rounds of 8     │
│ BatchGet(100), all local     │ ~5ms     │ No network, parallel │
├──────────────────────────────┼──────────┼──────────────────────┤
│ 100 sequential PutLeaf       │ ~500ms   │ 100 × 5ms (+ repl)  │
│ BatchPutLeaf(100), conc=32   │ ~25ms    │ ~4 rounds of 32     │
└──────────────────────────────┴──────────┴──────────────────────┘

  Speedup: 16–20x for BatchGet, 20x for BatchPutLeaf.
```

### 7.2 Resource Usage

```
Per batch request:
  - 1 tokio task per item (lightweight: ~256 bytes stack)
  - 1 semaphore permit per active item
  - mpsc channel: 64 slots × ~1KB per response = 64KB buffer

  BatchGet(1000) peak:
    1000 tasks × 256 bytes = 256KB task overhead
    32 concurrent × ~64KB response = 2MB in-flight data
    Total: ~3MB peak memory

  Acceptable for server handling a few concurrent batches.
```

### 7.3 Network Amplification

```
  BatchGet(100) on 3-node cluster:
    ~33 items local (no network)
    ~33 items → Node B (33 FetchValue RPCs)
    ~33 items → Node C (33 FetchValue RPCs)
    Total: 66 RPCs (vs 100 without batching)

  With future peer-to-peer batching (BatchFetchValue internal RPC):
    1 RPC to Node B (33 items)
    1 RPC to Node C (33 items)
    Total: 2 RPCs. 50x reduction.

  v1 uses individual FetchValue RPCs per item.
  Optimization deferred to avoid protocol complexity.
```

### 7.4 Benchmarking Plan

```rust
/// Benchmark: BatchGet 100 items, all local
#[bench]
fn bench_batch_get_100_local(b: &mut Bencher) {
    // Pre-populated store, 100 addrs
    // Expected: <10ms
}

/// Benchmark: BatchGet 100 items, distributed (3 nodes)
#[bench]
fn bench_batch_get_100_distributed(b: &mut Bencher) {
    // 3-node cluster, 100 items
    // Expected: <20ms
}

/// Benchmark: BatchPutLeaf 100 items
#[bench]
fn bench_batch_put_100(b: &mut Bencher) {
    // 100 × 1KB leaves
    // Expected: <30ms
}

/// Benchmark: BatchGet vs sequential GetValue
#[bench]
fn bench_batch_vs_sequential(b: &mut Bencher) {
    // Compare 100 sequential gets vs BatchGet(100)
    // Expected: 15-20x speedup
}

/// Benchmark: concurrency impact
#[bench]
fn bench_concurrency_sweep(b: &mut Bencher) {
    // BatchGet(100) with concurrency 4, 8, 16, 32, 64
    // Find optimal concurrency for this hardware
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/batch.rs` | **NEW** — BatchExecutor, BatchConfig, BatchProgress |
| `deriva-network/src/lib.rs` | Add `pub mod batch` |
| `proto/deriva.proto` | Add `BatchGet`, `BatchPutLeaf` RPCs + messages |
| `deriva-server/src/service.rs` | Add `batch_get`, `batch_put_leaf` handlers |
| `deriva-server/src/state.rs` | Add `batch_executor: Arc<BatchExecutor>` |
| `deriva-server/src/main.rs` | Initialize BatchExecutor |
| `deriva-core/src/error.rs` | Add `InvalidArgument` variant to DerivaError |
| `deriva-cli/src/commands.rs` | Add `batch-get`, `batch-put` CLI commands |
| `deriva-network/tests/batch.rs` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

| Crate | Version | Purpose |
|-------|---------|---------|
| `futures` | 0.3.x | `join_all` for task collection (already in workspace) |

No new external dependencies.

---

## 10. Design Rationale

### 10.1 Why Streaming Response Instead of Unary?

```
Unary response:
  Server collects ALL results, sends one big response.
  Client waits for slowest item before seeing any results.
  Memory: server holds all results in memory simultaneously.

Streaming response:
  Server sends each result as it completes.
  Client can start processing immediately.
  Memory: only channel buffer (64 items) in memory.

  For BatchGet(1000) where 999 items are fast and 1 is slow:
    Unary: client waits 10s (slow item timeout) for all results.
    Streaming: client gets 999 results in ~50ms, last one at 10s.

  Streaming is strictly better for latency-sensitive workloads.
```

### 10.2 Why Semaphore Instead of Fixed Thread Pool?

```
Fixed thread pool:
  N worker threads pull from a queue.
  Threads are OS-level → expensive (8KB stack each).
  Blocking: if a worker blocks on I/O, thread is wasted.

Tokio semaphore + spawn:
  N permits control concurrency.
  Tasks are lightweight (~256 bytes).
  Non-blocking: tasks yield on .await, no thread waste.
  Dynamic: semaphore permits can be adjusted at runtime.

  Tokio semaphore is the idiomatic Rust async approach.
```

### 10.3 Why Per-Item Timeout AND Batch Timeout?

```
Per-item timeout only:
  If all 1000 items are slow (5s each), batch takes 1000 × 5s / 32 = 156s.
  Client waits 2.5 minutes. Unacceptable.

Batch timeout only:
  If batch timeout = 60s and one item is slow (59s),
  all other items complete in 50ms but batch doesn't "finish" until 59s.
  (Actually it does with streaming, but the batch_get() call blocks.)

Both:
  Per-item timeout (10s): caps individual slow items.
  Batch timeout (60s): caps total batch duration.
  Belt and suspenders. Guarantees bounded execution time.
```

### 10.4 Why client_id in BatchPutLeaf?

```
Problem: BatchPutLeaf items don't have a CAddr yet (it's computed from data).
  Client needs to correlate "which response is for which input?"

  Option A: Use index field only.
    Works, but fragile if client reorders items.

  Option B: Client-assigned ID (our choice).
    Client sets client_id = "order_123_attachment_2".
    Response echoes client_id back.
    Client correlates by its own meaningful identifier.
    More robust than positional index.

  We provide BOTH index and client_id for maximum flexibility.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `batch_requests_total` | Counter | `op={get,put_leaf}` | Batch requests received |
| `batch_items_total` | Counter | `op,result={success,error}` | Per-item outcomes |
| `batch_size` | Histogram | `op` | Items per batch |
| `batch_duration_ms` | Histogram | `op` | Total batch latency |
| `batch_item_duration_ms` | Histogram | `op` | Per-item latency |
| `batch_concurrency` | Gauge | — | Current active batch sub-requests |
| `batch_payload_bytes` | Histogram | `op` | Total payload size |
| `batch_partial_failures` | Counter | `op` | Batches with at least one failed item |

### 11.2 Structured Logging

```rust
tracing::info!(
    op = "batch_get",
    items = addrs.len(),
    concurrency = concurrency,
    "batch operation started"
);

tracing::info!(
    op = "batch_get",
    total = progress.total,
    succeeded = progress.succeeded.load(Ordering::Relaxed),
    failed = progress.failed.load(Ordering::Relaxed),
    duration_ms = elapsed.as_millis(),
    "batch operation complete"
);

tracing::warn!(
    op = "batch_get",
    addr = %addr,
    index = index,
    error = %e,
    "batch item failed"
);

tracing::warn!(
    op = "batch_get",
    items = addrs.len(),
    "batch timed out, partial results returned"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| High batch failure rate | `batch_partial_failures` > 10% of `batch_requests_total` | Warning |
| Batch timeout | `batch_duration_ms` p99 > batch_timeout × 0.8 | Warning |
| Large batches | `batch_size` p99 > max_batch_size × 0.8 | Info |
| Batch item slow | `batch_item_duration_ms` p99 > item_timeout × 0.5 | Info |

---

## 12. Checklist

- [ ] Create `deriva-network/src/batch.rs`
- [ ] Implement `BatchExecutor` with scatter-gather pattern
- [ ] Implement `batch_get` with ring-based partitioning
- [ ] Implement `batch_put_leaf` with replication
- [ ] Implement per-item timeout and batch timeout
- [ ] Implement concurrency control via Semaphore
- [ ] Implement streaming response via mpsc channel
- [ ] Implement batch validation (size, bytes)
- [ ] Add `BatchGet` RPC to proto (server streaming)
- [ ] Add `BatchPutLeaf` RPC to proto (server streaming)
- [ ] Add `BatchErrorCode` enum to proto
- [ ] Implement gRPC handlers in service
- [ ] Add `InvalidArgument` to DerivaError
- [ ] Add CLI commands: `batch-get`, `batch-put`
- [ ] Write BatchGet tests (5 tests)
- [ ] Write BatchPutLeaf tests (3 tests)
- [ ] Write concurrency tests (1 test)
- [ ] Write integration tests (2 tests)
- [ ] Add metrics (8 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (4 alerts)
- [ ] Run benchmarks: local batch, distributed batch, put batch, concurrency sweep
- [ ] Config validation: max_batch_size ≥ 1, max_concurrency ≥ 1
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
