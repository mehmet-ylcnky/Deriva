# §3.6 Distributed Get Protocol

> **Status:** Not started
> **Depends on:** §3.1 (SWIM gossip), §3.3 (leaf replication), §3.4 (cache placement), §3.5 (locality-aware routing)
> **Crate(s):** `deriva-network`, `deriva-server`, `deriva-compute`
> **Estimated effort:** 3–4 days

---

## 1. Problem Statement

In a single-node Deriva, `Get(addr)` is straightforward: check cache → check
leaf store → resolve recipe → materialize → return. In a distributed cluster,
the value may be cached on a remote node, stored as a leaf on a different
replica, or require computation that should be routed elsewhere for locality.

The Distributed Get Protocol must:

1. **Accept a Get request on any node** — clients should not need to know
   which node holds the data.
2. **Resolve the address** — determine if it's a leaf, cached value, or recipe.
3. **Locate the data** — check local stores, then query the cluster via bloom
   filters (§3.4) and the replica ring (§3.3).
4. **Route computation if needed** — use locality-aware routing (§3.5) to
   compute on the optimal node.
5. **Stream the result back** — regardless of where the data was found or
   computed, stream 64KB chunks to the client via the existing gRPC protocol.
6. **Handle failures gracefully** — timeout, retry, circuit breaker, fallback.

### What makes this hard

```
Client → Node A (entry point)
  Node A: not in cache, not a local leaf
  Node A: bloom filter says Node B has it cached
  Node A: forward to Node B
  Node B: cache entry expired between bloom rebuild and request
  Node B: it's a recipe, route computation to Node C (has inputs)
  Node C: computes, streams result back to Node B
  Node B: proxies stream to Node A
  Node A: proxies stream to Client

  3 hops, 2 proxy layers, multiple failure points.
```

The protocol must handle this multi-hop resolution while maintaining streaming
semantics, bounded latency, and correct error propagation.

---

## 2. Design

### 2.1 Architecture Overview

```
                          ┌─────────────────────────────────────────┐
                          │           Distributed Get Flow          │
                          └─────────────────────────────────────────┘

  Client                   Node A (entry)         Node B            Node C
    │                        │                      │                 │
    │── Get(addr) ──────────►│                      │                 │
    │                        │ 1. Check local       │                 │
    │                        │    cache/leaf/recipe  │                 │
    │                        │    → not found local  │                 │
    │                        │                      │                 │
    │                        │ 2. Check bloom       │                 │
    │                        │    filters (gossip)  │                 │
    │                        │    → Node B may have │                 │
    │                        │                      │                 │
    │                        │── FetchValue(addr) ─►│                 │
    │                        │                      │ 3. Check local  │
    │                        │                      │    → found!     │
    │                        │◄── Stream chunks ────│                 │
    │◄── Stream chunks ─────│                      │                 │
    │                        │                      │                 │

  Alternative: value requires computation
    │                        │                      │                 │
    │                        │ 2. It's a recipe     │                 │
    │                        │    Route to Node C   │                 │
    │                        │    (§3.5 routing)    │                 │
    │                        │                      │                 │
    │                        │── RouteCompute ──────────────────────►│
    │                        │                      │                 │ 4. Compute
    │                        │                      │                 │    locally
    │◄── Stream chunks ─────│◄─────────────── Stream chunks ────────│
    │                        │                      │                 │
```

### 2.2 Request State Machine

```
                    ┌──────────┐
                    │ Received │
                    └────┬─────┘
                         │
                    ┌────▼─────┐
              ┌─────│ CheckLocal│─────┐
              │     └──────────┘     │
           Found                  Not found
              │                      │
              ▼                 ┌────▼──────┐
         ┌────────┐       ┌────│ Classify   │────┐
         │ Stream │       │    └───────────┘    │
         │ Local  │     Leaf                  Recipe
         └────────┘       │                      │
                     ┌────▼──────┐         ┌────▼──────┐
                     │ FetchLeaf │         │ RouteOrMat │
                     │ (replica) │         │ (§3.5)     │
                     └────┬──────┘         └────┬──────┘
                          │                     │
                     ┌────▼──┐            ┌────▼──────┐
                     │Stream │            │ Stream    │
                     │Fetched│            │ Computed  │
                     └───┬───┘            └────┬──────┘
                         │                     │
                         └──────────┬──────────┘
                               ┌───▼───┐
                               │ Done  │
                               └───────┘

  Any state can transition to Error → fallback or return error.
```

### 2.3 Resolution Strategy

The entry node follows this priority order:

```
1. LOCAL CACHE HIT     — fastest, return immediately
2. LOCAL LEAF STORE    — local disk read, fast
3. REMOTE CACHE HIT   — bloom filter lookup, fetch from peer
4. REMOTE LEAF FETCH   — fetch from replica ring (§3.3)
5. COMPUTE (routed)    — route to optimal node (§3.5)
6. COMPUTE (local)     — materialize locally as fallback
```

### 2.4 Core Types

```rust
/// Tracks the lifecycle of a distributed get request.
#[derive(Debug)]
pub struct DistributedGet {
    /// The address being resolved.
    addr: CAddr,
    /// Unique request ID for tracing.
    request_id: Uuid,
    /// Current state of the request.
    state: GetState,
    /// When the request was received.
    started_at: Instant,
    /// Maximum time allowed for the entire operation.
    deadline: Instant,
    /// Number of remote hops taken so far.
    hop_count: u32,
}

#[derive(Debug)]
enum GetState {
    /// Initial state — checking local stores.
    CheckingLocal,
    /// Fetching from a remote node (cache hit or leaf replica).
    FetchingRemote { target: NodeId },
    /// Computing (locally or routed to remote).
    Computing { target: Option<NodeId> },
    /// Streaming result back to caller.
    Streaming,
    /// Terminal — completed or failed.
    Done(GetOutcome),
}

#[derive(Debug)]
enum GetOutcome {
    /// Value found/computed, total bytes streamed.
    Success { source: ValueSource, bytes: u64 },
    /// Request failed after all fallbacks.
    Failed(DerivaError),
}

#[derive(Debug, Clone, Copy)]
enum ValueSource {
    LocalCache,
    LocalLeaf,
    RemoteCache(NodeId),
    RemoteLeaf(NodeId),
    ComputedLocal,
    ComputedRemote(NodeId),
}
```

### 2.5 Configuration

```rust
#[derive(Debug, Clone)]
pub struct DistributedGetConfig {
    /// Maximum time for the entire get operation.
    pub timeout: Duration,                    // default: 30s
    /// Maximum time to wait for a remote fetch.
    pub remote_fetch_timeout: Duration,       // default: 10s
    /// Maximum time to wait for remote computation.
    pub remote_compute_timeout: Duration,     // default: 20s
    /// Maximum hops for cache/leaf fetching.
    pub max_fetch_hops: u32,                  // default: 1
    /// Circuit breaker: failures before opening.
    pub circuit_breaker_threshold: u32,       // default: 5
    /// Circuit breaker: time in open state before half-open.
    pub circuit_breaker_reset: Duration,      // default: 30s
    /// Maximum concurrent remote fetches per node.
    pub max_concurrent_fetches: usize,        // default: 64
}

impl Default for DistributedGetConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            remote_fetch_timeout: Duration::from_secs(10),
            remote_compute_timeout: Duration::from_secs(20),
            max_fetch_hops: 1,
            circuit_breaker_threshold: 5,
            circuit_breaker_reset: Duration::from_secs(30),
            max_concurrent_fetches: 64,
        }
    }
}
```

### 2.6 Circuit Breaker

```rust
/// Per-peer circuit breaker to avoid hammering failing nodes.
#[derive(Debug)]
pub struct CircuitBreaker {
    states: DashMap<NodeId, CircuitState>,
    threshold: u32,
    reset_duration: Duration,
}

#[derive(Debug)]
enum CircuitState {
    Closed { failures: u32 },
    Open { since: Instant },
    HalfOpen,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, reset_duration: Duration) -> Self {
        Self {
            states: DashMap::new(),
            threshold,
            reset_duration,
        }
    }

    /// Returns true if requests to this peer are allowed.
    pub fn is_allowed(&self, peer: &NodeId) -> bool {
        match self.states.get(peer) {
            None => true,
            Some(entry) => match entry.value() {
                CircuitState::Closed { .. } => true,
                CircuitState::Open { since } => {
                    if since.elapsed() >= self.reset_duration {
                        true // transition to half-open on next call
                    } else {
                        false
                    }
                }
                CircuitState::HalfOpen => true, // allow one probe
            },
        }
    }

    /// Record a successful request to a peer.
    pub fn record_success(&self, peer: &NodeId) {
        self.states.insert(peer.clone(), CircuitState::Closed { failures: 0 });
    }

    /// Record a failed request to a peer.
    pub fn record_failure(&self, peer: &NodeId) {
        let mut entry = self.states.entry(peer.clone())
            .or_insert(CircuitState::Closed { failures: 0 });
        match entry.value_mut() {
            CircuitState::Closed { failures } => {
                *failures += 1;
                if *failures >= self.threshold {
                    *entry = CircuitState::Open { since: Instant::now() };
                }
            }
            CircuitState::HalfOpen => {
                *entry = CircuitState::Open { since: Instant::now() };
            }
            CircuitState::Open { .. } => {} // already open
        }
    }
}
```

### 2.7 Wire Protocol

```protobuf
// Added to proto/deriva_internal.proto

service DerivaInternal {
    // Existing RPCs...
    rpc RouteCompute(RouteComputeRequest) returns (stream DataChunk);

    // NEW: Fetch a value from a peer (cache or leaf)
    rpc FetchValue(FetchValueRequest) returns (stream DataChunk);
}

message FetchValueRequest {
    bytes addr = 1;          // CAddr (32 bytes)
    uint32 hop_count = 2;    // anti-circular
    string request_id = 3;   // tracing correlation
}

// DataChunk already exists in the public API — reuse for internal streaming.
```

---

## 3. Implementation

### 3.1 Distributed Get Resolver

```rust
// deriva-network/src/distributed_get.rs

use crate::routing::ComputeRouter;
use crate::swim::SwimRuntime;
use deriva_core::{CAddr, Value, DerivaError};
use std::sync::Arc;
use tokio::sync::Semaphore;
use uuid::Uuid;

pub struct DistributedGetResolver {
    config: DistributedGetConfig,
    swim: Arc<SwimRuntime>,
    router: Arc<ComputeRouter>,
    circuit_breaker: CircuitBreaker,
    fetch_semaphore: Arc<Semaphore>,
}

impl DistributedGetResolver {
    pub fn new(
        config: DistributedGetConfig,
        swim: Arc<SwimRuntime>,
        router: Arc<ComputeRouter>,
    ) -> Self {
        let cb = CircuitBreaker::new(
            config.circuit_breaker_threshold,
            config.circuit_breaker_reset,
        );
        let sem = Arc::new(Semaphore::new(config.max_concurrent_fetches));
        Self {
            config,
            swim,
            router,
            circuit_breaker: cb,
            fetch_semaphore: sem,
        }
    }

    /// Main entry point: resolve an address across the cluster.
    /// Returns a stream of DataChunks.
    pub async fn resolve(
        &self,
        addr: &CAddr,
        local_cache: &EvictableCache,
        local_leaf_store: &LeafStore,
        dag_store: &DagStore,
        executor: &Executor,
    ) -> Result<ValueStream, DerivaError> {
        let request_id = Uuid::new_v4();
        let deadline = Instant::now() + self.config.timeout;

        tracing::info!(
            addr = %addr,
            request_id = %request_id,
            "distributed get started"
        );

        // 1. Check local cache
        if let Some(value) = local_cache.get(addr) {
            tracing::debug!(addr = %addr, "local cache hit");
            metrics::counter!("distributed_get_source", "source" => "local_cache").increment(1);
            return Ok(ValueStream::from_value(value));
        }

        // 2. Check local leaf store
        if let Some(data) = local_leaf_store.get(addr)? {
            tracing::debug!(addr = %addr, "local leaf hit");
            metrics::counter!("distributed_get_source", "source" => "local_leaf").increment(1);
            return Ok(ValueStream::from_bytes(data));
        }

        // 3. Check if it's a recipe
        let recipe = dag_store.get(addr);

        // 4. Try remote cache (bloom filter lookup)
        if let Some(stream) = self.try_remote_cache(addr, deadline).await? {
            metrics::counter!("distributed_get_source", "source" => "remote_cache").increment(1);
            return Ok(stream);
        }

        // 5. Try remote leaf (replica ring)
        if recipe.is_none() {
            // Not a recipe — must be a leaf on another replica
            if let Some(stream) = self.try_remote_leaf(addr, deadline).await? {
                metrics::counter!("distributed_get_source", "source" => "remote_leaf").increment(1);
                return Ok(stream);
            }
        }

        // 6. Compute (recipe must exist at this point)
        let recipe = recipe.ok_or_else(|| {
            DerivaError::NotFound(format!("address {} not found in cluster", addr))
        })?;

        // 7. Route or compute locally (§3.5)
        let stream = self.compute_or_route(
            addr, &recipe, executor, dag_store, local_cache, deadline
        ).await?;
        Ok(stream)
    }

    /// Check remote nodes for a cached copy using bloom filters.
    async fn try_remote_cache(
        &self,
        addr: &CAddr,
        deadline: Instant,
    ) -> Result<Option<ValueStream>, DerivaError> {
        let peers = self.swim.alive_members();
        let mut candidates: Vec<&NodeId> = peers.iter()
            .filter(|(_, meta)| meta.bloom_filter.check(addr.as_bytes()))
            .map(|(id, _)| id)
            .collect();

        // Sort by lowest load
        candidates.sort_by(|a, b| {
            let la = self.swim.get_metadata(a).map(|m| m.cpu_load).unwrap_or(1.0);
            let lb = self.swim.get_metadata(b).map(|m| m.cpu_load).unwrap_or(1.0);
            la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
        });

        for peer in candidates.into_iter().take(3) {
            if !self.circuit_breaker.is_allowed(peer) {
                continue;
            }
            if Instant::now() >= deadline {
                return Err(DerivaError::Timeout("distributed get deadline exceeded".into()));
            }

            let _permit = self.fetch_semaphore.acquire().await
                .map_err(|_| DerivaError::Internal("semaphore closed".into()))?;

            match self.fetch_from_peer(peer, addr).await {
                Ok(stream) => {
                    self.circuit_breaker.record_success(peer);
                    tracing::debug!(addr = %addr, peer = %peer, "remote cache hit");
                    return Ok(Some(stream));
                }
                Err(e) => {
                    self.circuit_breaker.record_failure(peer);
                    tracing::warn!(
                        addr = %addr, peer = %peer, error = %e,
                        "remote cache fetch failed, trying next"
                    );
                }
            }
        }
        Ok(None)
    }

    /// Fetch a leaf from its replica set (§3.3 consistent hash ring).
    async fn try_remote_leaf(
        &self,
        addr: &CAddr,
        deadline: Instant,
    ) -> Result<Option<ValueStream>, DerivaError> {
        let replicas = self.swim.ring().replicas_for(addr);

        for replica in replicas {
            if replica == self.swim.local_id() {
                continue; // already checked local
            }
            if !self.circuit_breaker.is_allowed(&replica) {
                continue;
            }
            if Instant::now() >= deadline {
                return Err(DerivaError::Timeout("distributed get deadline exceeded".into()));
            }

            match self.fetch_from_peer(&replica, addr).await {
                Ok(stream) => {
                    self.circuit_breaker.record_success(&replica);
                    tracing::debug!(addr = %addr, replica = %replica, "remote leaf hit");
                    return Ok(Some(stream));
                }
                Err(e) => {
                    self.circuit_breaker.record_failure(&replica);
                    tracing::warn!(
                        addr = %addr, replica = %replica, error = %e,
                        "remote leaf fetch failed, trying next replica"
                    );
                }
            }
        }
        Ok(None)
    }

    /// Fetch value from a specific peer via FetchValue RPC.
    async fn fetch_from_peer(
        &self,
        peer: &NodeId,
        addr: &CAddr,
    ) -> Result<ValueStream, DerivaError> {
        let timeout = self.config.remote_fetch_timeout;
        let channel = self.swim.get_channel(peer).await?;
        let mut client = DerivaInternalClient::new(channel);

        let request = tonic::Request::new(FetchValueRequest {
            addr: addr.as_bytes().to_vec(),
            hop_count: 1,
            request_id: Uuid::new_v4().to_string(),
        });

        let response = tokio::time::timeout(timeout, client.fetch_value(request))
            .await
            .map_err(|_| DerivaError::Timeout("remote fetch timed out".into()))?
            .map_err(|e| DerivaError::Network(format!("fetch RPC failed: {}", e)))?;

        Ok(ValueStream::from_tonic_stream(response.into_inner()))
    }

    /// Compute via routing (§3.5) or locally.
    async fn compute_or_route(
        &self,
        addr: &CAddr,
        recipe: &Recipe,
        executor: &Executor,
        dag_store: &DagStore,
        cache: &EvictableCache,
        deadline: Instant,
    ) -> Result<ValueStream, DerivaError> {
        let remaining = deadline.duration_since(Instant::now());
        let compute_timeout = remaining.min(self.config.remote_compute_timeout);

        // Use §3.5 routing decision
        match self.router.route(addr, recipe, dag_store, cache, 0).await? {
            RoutingDecision::Local { .. } => {
                tracing::debug!(addr = %addr, "computing locally");
                metrics::counter!("distributed_get_source", "source" => "computed_local").increment(1);
                let value = executor.materialize(addr, dag_store, cache).await?;
                Ok(ValueStream::from_value(value))
            }
            RoutingDecision::Remote { target, .. } => {
                tracing::info!(addr = %addr, target = %target, "routing computation");
                match tokio::time::timeout(
                    compute_timeout,
                    self.forward_compute(&target, addr)
                ).await {
                    Ok(Ok(stream)) => {
                        self.circuit_breaker.record_success(&target);
                        metrics::counter!("distributed_get_source", "source" => "computed_remote").increment(1);
                        Ok(stream)
                    }
                    Ok(Err(e)) | Err(e) => {
                        self.circuit_breaker.record_failure(&target);
                        tracing::warn!(
                            addr = %addr, target = %target, error = %e,
                            "remote compute failed, falling back to local"
                        );
                        metrics::counter!("distributed_get_source", "source" => "computed_local_fallback").increment(1);
                        let value = executor.materialize(addr, dag_store, cache).await?;
                        Ok(ValueStream::from_value(value))
                    }
                }
            }
        }
    }

    /// Forward computation to a remote node via RouteCompute RPC.
    async fn forward_compute(
        &self,
        target: &NodeId,
        addr: &CAddr,
    ) -> Result<ValueStream, DerivaError> {
        let channel = self.swim.get_channel(target).await?;
        let mut client = DerivaInternalClient::new(channel);

        let request = tonic::Request::new(RouteComputeRequest {
            addr: addr.as_bytes().to_vec(),
            hop_count: 1,
        });

        let response = client.route_compute(request).await
            .map_err(|e| DerivaError::Network(format!("route compute failed: {}", e)))?;

        Ok(ValueStream::from_tonic_stream(response.into_inner()))
    }
}
```

### 3.2 ValueStream — Unified Streaming Abstraction

```rust
// deriva-network/src/value_stream.rs

use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;

/// Unified stream that can wrap local values or remote gRPC streams.
pub struct ValueStream {
    inner: ValueStreamInner,
}

enum ValueStreamInner {
    /// Single value, chunked into 64KB pieces on iteration.
    Local { data: Bytes, offset: usize },
    /// Remote gRPC stream of DataChunks.
    Remote {
        stream: Pin<Box<dyn Stream<Item = Result<DataChunk, tonic::Status>> + Send>>,
    },
}

impl ValueStream {
    pub fn from_value(value: Value) -> Self {
        Self {
            inner: ValueStreamInner::Local {
                data: Bytes::from(value.into_bytes()),
                offset: 0,
            },
        }
    }

    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self {
            inner: ValueStreamInner::Local {
                data: Bytes::from(data),
                offset: 0,
            },
        }
    }

    pub fn from_tonic_stream(
        stream: tonic::Streaming<DataChunk>,
    ) -> Self {
        Self {
            inner: ValueStreamInner::Remote {
                stream: Box::pin(stream),
            },
        }
    }

    /// Yield next chunk (up to 64KB).
    pub async fn next_chunk(&mut self) -> Option<Result<Bytes, DerivaError>> {
        const CHUNK_SIZE: usize = 65_536;
        match &mut self.inner {
            ValueStreamInner::Local { data, offset } => {
                if *offset >= data.len() {
                    return None;
                }
                let end = (*offset + CHUNK_SIZE).min(data.len());
                let chunk = data.slice(*offset..end);
                *offset = end;
                Some(Ok(chunk))
            }
            ValueStreamInner::Remote { stream } => {
                use futures::StreamExt;
                match stream.next().await {
                    Some(Ok(chunk)) => Some(Ok(Bytes::from(chunk.data))),
                    Some(Err(status)) => Some(Err(DerivaError::Network(
                        format!("stream error: {}", status)
                    ))),
                    None => None,
                }
            }
        }
    }

    /// Collect entire stream into a single Vec<u8>.
    pub async fn collect_all(&mut self) -> Result<Vec<u8>, DerivaError> {
        let mut result = Vec::new();
        while let Some(chunk) = self.next_chunk().await {
            result.extend_from_slice(&chunk?);
        }
        Ok(result)
    }
}
```

### 3.3 FetchValue RPC Handler

```rust
// Added to deriva-server/src/internal.rs

#[tonic::async_trait]
impl DerivaInternal for InternalService {
    type FetchValueStream = ReceiverStream<Result<DataChunk, Status>>;

    async fn fetch_value(
        &self,
        request: Request<FetchValueRequest>,
    ) -> Result<Response<Self::FetchValueStream>, Status> {
        let req = request.into_inner();
        let addr = CAddr::from_bytes(&req.addr)
            .map_err(|e| Status::invalid_argument(format!("bad addr: {}", e)))?;

        tracing::debug!(
            addr = %addr,
            hop_count = req.hop_count,
            request_id = %req.request_id,
            "fetch_value received"
        );

        // Only check local stores — do NOT recurse into distributed get
        // to prevent infinite forwarding loops.
        let data = if let Some(value) = self.state.cache.get(&addr) {
            value.into_bytes()
        } else if let Some(data) = self.state.leaf_store.get(&addr)
            .map_err(|e| Status::internal(e.to_string()))? {
            data
        } else {
            return Err(Status::not_found(format!("addr {} not found locally", addr)));
        };

        // Stream in 64KB chunks
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            for chunk in data.chunks(65_536) {
                if tx.send(Ok(DataChunk {
                    data: chunk.to_vec(),
                    offset: 0,
                    is_last: false,
                })).await.is_err() {
                    break;
                }
            }
            let _ = tx.send(Ok(DataChunk {
                data: vec![],
                offset: 0,
                is_last: true,
            })).await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
```

### 3.4 Server Integration — Updated Get Handler

```rust
// Updated get() in deriva-server/src/service.rs

async fn get(
    &self,
    request: Request<GetRequest>,
) -> Result<Response<Self::GetStream>, Status> {
    let addr = CAddr::from_bytes(&request.into_inner().addr)
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let state = self.state.clone();
    tokio::spawn(async move {
        let result = if let Some(resolver) = &state.distributed_get {
            // Cluster mode: use distributed resolution
            resolver.resolve(
                &addr,
                &state.cache,
                &state.leaf_store,
                &state.dag_store,
                &state.executor,
            ).await
        } else {
            // Single-node mode: original path
            match state.executor.materialize(&addr, &state.dag_store, &state.cache).await {
                Ok(value) => Ok(ValueStream::from_value(value)),
                Err(e) => Err(e),
            }
        };

        match result {
            Ok(mut stream) => {
                while let Some(chunk_result) = stream.next_chunk().await {
                    match chunk_result {
                        Ok(data) => {
                            if tx.send(Ok(DataChunk {
                                data: data.to_vec(),
                                offset: 0,
                                is_last: false,
                            })).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(Status::internal(e.to_string()))).await;
                            return;
                        }
                    }
                }
                let _ = tx.send(Ok(DataChunk {
                    data: vec![],
                    offset: 0,
                    is_last: true,
                })).await;
            }
            Err(e) => {
                let _ = tx.send(Err(Status::internal(e.to_string()))).await;
            }
        }
    });

    Ok(Response::new(ReceiverStream::new(rx)))
}
```


---

## 4. Data Flow Diagrams

### 4.1 Happy Path — Local Cache Hit

```
Client              Node A
  │                   │
  │── Get(addr) ─────►│
  │                   │ cache.get(addr) → Some(value)
  │                   │
  │◄── chunk 1 ──────│  (64KB)
  │◄── chunk 2 ──────│  (64KB)
  │◄── chunk N ──────│  (remainder)
  │◄── is_last=true ─│
  │                   │

  Latency: ~1ms (cache lookup + streaming)
  Hops: 0
```

### 4.2 Remote Cache Hit via Bloom Filter

```
Client              Node A                    Node B
  │                   │                         │
  │── Get(addr) ─────►│                         │
  │                   │ cache.get → None         │
  │                   │ leaf_store.get → None    │
  │                   │                         │
  │                   │ bloom_check(peers, addr) │
  │                   │ → Node B matches         │
  │                   │                         │
  │                   │── FetchValue(addr) ────►│
  │                   │                         │ cache.get → Some!
  │                   │◄── chunk 1 ────────────│
  │◄── chunk 1 ──────│                         │
  │                   │◄── chunk 2 ────────────│
  │◄── chunk 2 ──────│                         │
  │                   │◄── is_last ────────────│
  │◄── is_last ──────│                         │

  Latency: ~3ms (bloom check + RPC + streaming)
  Hops: 1
```

### 4.3 Remote Leaf Fetch from Replica Ring

```
Client              Node A                    Node B (replica)
  │                   │                         │
  │── Get(addr) ─────►│                         │
  │                   │ cache → None             │
  │                   │ leaf_store → None         │
  │                   │ dag_store → None (not recipe)│
  │                   │ bloom → no matches       │
  │                   │                         │
  │                   │ ring.replicas_for(addr)  │
  │                   │ → [Node B, Node C]       │
  │                   │                         │
  │                   │── FetchValue(addr) ────►│
  │                   │                         │ leaf_store.get → Some!
  │                   │◄── stream chunks ──────│
  │◄── stream chunks ─│                         │

  Latency: ~5ms (ring lookup + RPC + disk read + streaming)
  Hops: 1
```

### 4.4 Routed Computation

```
Client              Node A                    Node C (has inputs)
  │                   │                         │
  │── Get(addr) ─────►│                         │
  │                   │ cache → None             │
  │                   │ leaf → None              │
  │                   │ dag → Recipe(f, [i1,i2]) │
  │                   │ bloom → no cache hit     │
  │                   │                         │
  │                   │ router.route(addr)       │
  │                   │ → Remote(Node C, 65%)    │
  │                   │                         │
  │                   │── RouteCompute(addr) ──►│
  │                   │                         │ materialize(addr)
  │                   │                         │ i1 local ✓
  │                   │                         │ i2 local ✓
  │                   │                         │ compute f(i1,i2)
  │                   │◄── stream result ──────│
  │◄── stream result ─│                         │

  Latency: ~50ms+ (routing + RPC + compute + streaming)
  Hops: 1 (routing hop)
```

### 4.5 Fallback Chain — Remote Fails, Local Succeeds

```
Client              Node A                    Node B (down)
  │                   │                         │
  │── Get(addr) ─────►│                         │
  │                   │ cache/leaf → None        │
  │                   │ bloom → Node B           │
  │                   │                         │
  │                   │── FetchValue(addr) ──X  │ (connection refused)
  │                   │                         │
  │                   │ circuit_breaker.record_failure(B)
  │                   │                         │
  │                   │ dag → Recipe(f, [input]) │
  │                   │ router.route → Remote(B) │
  │                   │ but B circuit open!      │
  │                   │ → fallback: compute local│
  │                   │                         │
  │                   │ materialize(addr) locally│
  │                   │ fetch input from replica │
  │                   │ compute f(input)         │
  │                   │                         │
  │◄── stream result ─│                         │

  Latency: ~200ms (failed attempt + local compute)
  Hops: 0 (after fallback)
```

### 4.6 Circuit Breaker Prevents Repeated Failures

```
Timeline for Node B failures:

  t=0s   Request 1 → Node B → timeout (10s)     failures=1
  t=12s  Request 2 → Node B → connection refused  failures=2
  t=13s  Request 3 → Node B → connection refused  failures=3
  t=14s  Request 4 → Node B → connection refused  failures=4
  t=15s  Request 5 → Node B → connection refused  failures=5
         ──── CIRCUIT OPENS ────
  t=16s  Request 6 → skip Node B (circuit open) → local
  t=20s  Request 7 → skip Node B (circuit open) → local
         ...
  t=45s  ──── 30s elapsed, HALF-OPEN ────
  t=46s  Request N → try Node B (probe)
         → success → CIRCUIT CLOSES
         → failure → CIRCUIT RE-OPENS for 30s
```

---

## 5. Test Specification

### 5.1 Circuit Breaker Unit Tests

```rust
#[cfg(test)]
mod circuit_breaker_tests {
    use super::*;

    fn test_node(name: &str) -> NodeId {
        NodeId {
            addr: format!("127.0.0.1:{}", 7000 + name.len()).parse().unwrap(),
            incarnation: 0,
            name: name.to_string(),
        }
    }

    #[test]
    fn test_initially_closed() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        let node = test_node("a");
        assert!(cb.is_allowed(&node));
    }

    #[test]
    fn test_opens_after_threshold() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        let node = test_node("a");

        cb.record_failure(&node);
        assert!(cb.is_allowed(&node)); // 1 < 3
        cb.record_failure(&node);
        assert!(cb.is_allowed(&node)); // 2 < 3
        cb.record_failure(&node);
        assert!(!cb.is_allowed(&node)); // 3 >= 3 → open
    }

    #[test]
    fn test_success_resets() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        let node = test_node("a");

        cb.record_failure(&node);
        cb.record_failure(&node);
        cb.record_success(&node); // reset
        cb.record_failure(&node);
        assert!(cb.is_allowed(&node)); // only 1 failure since reset
    }

    #[test]
    fn test_half_open_after_reset_duration() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(50));
        let node = test_node("a");

        cb.record_failure(&node);
        cb.record_failure(&node);
        assert!(!cb.is_allowed(&node)); // open

        std::thread::sleep(Duration::from_millis(60));
        assert!(cb.is_allowed(&node)); // half-open (reset elapsed)
    }

    #[test]
    fn test_half_open_failure_reopens() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(50));
        let node = test_node("a");

        cb.record_failure(&node);
        cb.record_failure(&node); // opens
        std::thread::sleep(Duration::from_millis(60)); // half-open

        cb.record_failure(&node); // probe failed → re-open
        assert!(!cb.is_allowed(&node));
    }

    #[test]
    fn test_independent_per_node() {
        let cb = CircuitBreaker::new(2, Duration::from_secs(30));
        let a = test_node("a");
        let b = test_node("b");

        cb.record_failure(&a);
        cb.record_failure(&a); // a opens
        assert!(!cb.is_allowed(&a));
        assert!(cb.is_allowed(&b)); // b unaffected
    }
}
```

### 5.2 ValueStream Unit Tests

```rust
#[cfg(test)]
mod value_stream_tests {
    use super::*;

    #[tokio::test]
    async fn test_local_small_value() {
        let mut stream = ValueStream::from_bytes(vec![1, 2, 3, 4, 5]);
        let chunk = stream.next_chunk().await.unwrap().unwrap();
        assert_eq!(&chunk[..], &[1, 2, 3, 4, 5]);
        assert!(stream.next_chunk().await.is_none());
    }

    #[tokio::test]
    async fn test_local_chunking() {
        // 100KB → should produce 2 chunks (64KB + 36KB)
        let data = vec![0xAB; 100_000];
        let mut stream = ValueStream::from_bytes(data.clone());

        let c1 = stream.next_chunk().await.unwrap().unwrap();
        assert_eq!(c1.len(), 65_536);

        let c2 = stream.next_chunk().await.unwrap().unwrap();
        assert_eq!(c2.len(), 100_000 - 65_536);

        assert!(stream.next_chunk().await.is_none());
    }

    #[tokio::test]
    async fn test_collect_all() {
        let data = vec![0xFF; 200_000];
        let mut stream = ValueStream::from_bytes(data.clone());
        let collected = stream.collect_all().await.unwrap();
        assert_eq!(collected, data);
    }

    #[tokio::test]
    async fn test_empty_value() {
        let mut stream = ValueStream::from_bytes(vec![]);
        assert!(stream.next_chunk().await.is_none());
    }
}
```

### 5.3 DistributedGetConfig Tests

```rust
#[test]
fn test_default_config() {
    let config = DistributedGetConfig::default();
    assert_eq!(config.timeout, Duration::from_secs(30));
    assert_eq!(config.remote_fetch_timeout, Duration::from_secs(10));
    assert_eq!(config.remote_compute_timeout, Duration::from_secs(20));
    assert_eq!(config.max_fetch_hops, 1);
    assert_eq!(config.circuit_breaker_threshold, 5);
    assert_eq!(config.circuit_breaker_reset, Duration::from_secs(30));
    assert_eq!(config.max_concurrent_fetches, 64);
}
```

### 5.4 Integration Tests — Multi-Node Get

```rust
#[tokio::test]
async fn test_get_local_cache_hit() {
    let (mut client, state) = start_single_node().await;

    // Put a leaf and materialize to cache it
    let addr = put_leaf(&mut client, b"hello world").await;
    let result = get(&mut client, &addr).await;
    assert_eq!(result, b"hello world");

    // Second get should hit cache
    let result2 = get(&mut client, &addr).await;
    assert_eq!(result2, b"hello world");
}

#[tokio::test]
async fn test_get_remote_cache_hit() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (mut client_b, _sb) = start_cluster_node(1).await;
    tokio::time::sleep(Duration::from_secs(3)).await; // gossip convergence

    // Put and materialize on A (caches result)
    let leaf = put_leaf(&mut client_a, b"data").await;
    let recipe = put_recipe(&mut client_a, "identity", "v1", vec![leaf]).await;
    let _ = get(&mut client_a, &recipe).await;

    // Wait for bloom filter gossip
    tokio::time::sleep(Duration::from_secs(15)).await;

    // Get from B — should find via bloom filter on A
    let result = get(&mut client_b, &recipe).await;
    assert_eq!(result, b"data");
}

#[tokio::test]
async fn test_get_remote_leaf_from_replica() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (mut client_b, _sb) = start_cluster_node(1).await;
    let (mut client_c, _sc) = start_cluster_node(2).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Put leaf on A — replicated to B and C via §3.3
    let addr = put_leaf(&mut client_a, &vec![0u8; 1_000_000]).await;
    tokio::time::sleep(Duration::from_secs(2)).await; // replication

    // Get from C — should fetch from replica ring
    let result = get(&mut client_c, &addr).await;
    assert_eq!(result.len(), 1_000_000);
}

#[tokio::test]
async fn test_get_with_computation_routing() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (mut client_b, _sb) = start_cluster_node(1).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Large leaf on A
    let leaf = put_leaf(&mut client_a, &vec![42u8; 5_000_000]).await;
    let recipe = put_recipe(&mut client_a, "sha256", "v1", vec![leaf]).await;

    // Wait for bloom gossip
    tokio::time::sleep(Duration::from_secs(15)).await;

    // Get from B — should route computation to A (has the 5MB input)
    let result = get(&mut client_b, &recipe).await;
    assert!(!result.is_empty());
}

#[tokio::test]
async fn test_get_fallback_on_node_failure() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (mut client_b, sb) = start_cluster_node(1).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    let leaf = put_leaf(&mut client_a, b"important").await;
    let recipe = put_recipe(&mut client_a, "identity", "v1", vec![leaf]).await;

    // Materialize on B to cache it
    let _ = get(&mut client_b, &recipe).await;
    tokio::time::sleep(Duration::from_secs(15)).await;

    // Kill B
    drop(sb);
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Get from A — bloom says B has it, but B is down
    // Should fallback to local computation
    let result = get(&mut client_a, &recipe).await;
    assert_eq!(result, b"important");
}

#[tokio::test]
async fn test_get_not_found() {
    let (mut client, _s) = start_single_node().await;
    let fake_addr = CAddr::from_bytes(&[0u8; 32]).unwrap();

    let result = try_get(&mut client, &fake_addr).await;
    assert!(result.is_err());
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Address not found anywhere | Return `NotFound` error | No data, no recipe, no replicas |
| 2 | All replicas down | Return error after trying all | Cannot fetch leaf |
| 3 | Bloom false positive (peer lacks value) | Peer returns `NotFound`, try next candidate | Graceful fallback |
| 4 | Remote fetch timeout | Circuit breaker records failure, try next | Bounded latency |
| 5 | Remote compute timeout | Fallback to local computation | Always-correct fallback |
| 6 | Client disconnects mid-stream | Sender detects closed channel, stops | Resource cleanup |
| 7 | Entry node is also the best compute target | Router returns `Local` | No unnecessary RPC |
| 8 | Concurrent gets for same address | Each resolves independently | No dedup (simplicity) |
| 9 | Value evicted between bloom check and fetch | Peer returns `NotFound` | Try next or compute |
| 10 | Recipe exists but inputs missing everywhere | Compute fails with `NotFound` for input | Propagate error |
| 11 | Deadline exceeded during resolution | Return `Timeout` error | Bounded total latency |
| 12 | Semaphore exhausted (too many concurrent fetches) | Await permit (backpressure) | Prevents connection storm |
| 13 | Single-node mode (no SWIM) | Skip distributed resolution, use local path | Backward compatible |
| 14 | Fetch returns corrupted data | CAddr verification fails on cache store | Content-addressed integrity |

### 6.1 Request Deduplication (Not Implemented)

```
Considered: coalescing concurrent gets for the same address.

  Request 1: Get(X) → starts resolution
  Request 2: Get(X) → waits for Request 1's result

Rejected because:
  - Adds complexity (waiters map, notification)
  - Cache already handles this (second get hits cache after first completes)
  - Concurrent gets for same addr are rare in practice
  - Risk of head-of-line blocking if first request is slow

Future optimization if profiling shows duplicate work.
```

### 6.2 Streaming Proxy Correctness

```
When Node A proxies a stream from Node B to the client:

  Node B → [chunk1, chunk2, ..., chunkN, is_last] → Node A → Client

  Invariants:
  1. Chunks arrive in order (TCP guarantees, gRPC preserves)
  2. No chunks are duplicated (single stream, single consumer)
  3. is_last is forwarded exactly once
  4. If Node B stream errors, Node A propagates error to client
  5. If client disconnects, Node A drops the proxy stream
     (Node B detects broken pipe on next send)

  Memory: Node A buffers at most 16 chunks (channel capacity)
  before backpressure slows Node B's sending.
```

---

## 7. Performance Analysis

### 7.1 Latency Breakdown by Resolution Path

```
┌─────────────────────────┬──────────┬───────────────────────────┐
│ Path                    │ Latency  │ Breakdown                 │
├─────────────────────────┼──────────┼───────────────────────────┤
│ Local cache hit         │ ~0.5ms   │ HashMap lookup + stream   │
│ Local leaf hit          │ ~2ms     │ Disk read + stream        │
│ Remote cache hit        │ ~4ms     │ Bloom + RPC + stream      │
│ Remote leaf (replica)   │ ~6ms     │ Ring + RPC + disk + stream│
│ Routed compute (small)  │ ~15ms   │ Route + RPC + compute     │
│ Routed compute (large)  │ ~100ms+ │ Route + RPC + compute     │
│ Local compute (fallback)│ ~50ms+  │ Fetch inputs + compute    │
│ Fallback after failure  │ ~12s    │ Timeout(10s) + local      │
└─────────────────────────┴──────────┴───────────────────────────┘
```

### 7.2 Throughput — Concurrent Gets

```
Semaphore limits concurrent remote fetches to 64 per node.
Each fetch uses one gRPC stream (~1 TCP connection from pool).

Theoretical max throughput per node:
  Local cache hits: ~100K gets/sec (limited by HashMap contention)
  Remote fetches: ~64 concurrent × ~4ms each = ~16K gets/sec
  Computations: depends on compute function complexity

Bottleneck analysis:
  ┌──────────────────┬──────────────┬──────────────────┐
  │ Component        │ Limit        │ Mitigation       │
  ├──────────────────┼──────────────┼──────────────────┤
  │ gRPC connections │ 64 concurrent│ Connection pool   │
  │ Cache HashMap    │ Lock contention│ DashMap sharding│
  │ Disk I/O (leaf)  │ ~10K IOPS   │ SSD, read-ahead  │
  │ Network bandwidth│ 1-10 Gbps   │ Chunk streaming   │
  │ CPU (compute)    │ Core count   │ Routing spreads   │
  └──────────────────┴──────────────┴──────────────────┘
```

### 7.3 Circuit Breaker Impact

```
Without circuit breaker:
  Node B down → every get that bloom-matches B waits 10s timeout
  10 gets/sec × 10s timeout = 100 blocked requests before SWIM detects

With circuit breaker (threshold=5, reset=30s):
  First 5 requests: each waits up to 10s → 50s total wasted
  Request 6+: skip B immediately → 0 additional latency
  After 30s: probe B once → if still down, re-open for 30s

  Improvement: 50s wasted vs potentially minutes of blocked requests.
```

### 7.4 Streaming Proxy Overhead

```
Proxy adds per-chunk overhead:
  - Receive from upstream: ~50μs (channel recv)
  - Send to downstream: ~50μs (channel send)
  - Total per chunk: ~100μs
  - For 1GB value (16384 chunks): ~1.6s overhead

  vs direct streaming (no proxy): 0s overhead

  Proxy overhead is ~1.6s / total_time. For a 1GB value at 1Gbps:
  Transfer time: ~8s
  Proxy overhead: ~1.6s (20%)

  For typical values (<100MB): proxy overhead < 200ms (<5%)
```

### 7.5 Benchmarking Plan

```rust
/// Benchmark: local cache hit throughput
#[bench]
fn bench_get_local_cache(b: &mut Bencher) {
    // Pre-populate cache with 1000 entries
    // Measure: get() calls per second
    // Expected: >50K gets/sec
}

/// Benchmark: remote fetch latency (2-node cluster)
#[bench]
fn bench_get_remote_fetch(b: &mut Bencher) {
    // Value cached on Node B, get from Node A
    // Measure: end-to-end latency
    // Expected: <5ms for 1KB value
}

/// Benchmark: streaming proxy throughput
#[bench]
fn bench_streaming_proxy(b: &mut Bencher) {
    // 100MB value on Node B, get from Node A
    // Measure: total throughput (MB/s)
    // Expected: >500MB/s (limited by network, not proxy)
}

/// Benchmark: circuit breaker overhead
#[bench]
fn bench_circuit_breaker_check(b: &mut Bencher) {
    // 100 nodes in circuit breaker map
    // Measure: is_allowed() latency
    // Expected: <100ns per check
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/distributed_get.rs` | **NEW** — DistributedGetResolver, DistributedGetConfig |
| `deriva-network/src/value_stream.rs` | **NEW** — ValueStream abstraction |
| `deriva-network/src/circuit_breaker.rs` | **NEW** — CircuitBreaker |
| `deriva-network/src/lib.rs` | Add `pub mod distributed_get, value_stream, circuit_breaker` |
| `proto/deriva_internal.proto` | Add `FetchValue` RPC |
| `deriva-server/src/internal.rs` | Add `fetch_value` handler |
| `deriva-server/src/service.rs` | Update `get()` to use DistributedGetResolver |
| `deriva-server/src/state.rs` | Add `distributed_get: Option<DistributedGetResolver>` |
| `deriva-network/tests/distributed_get.rs` | **NEW** — integration tests |
| `deriva-network/tests/circuit_breaker.rs` | **NEW** — unit tests |

---

## 9. Dependency Changes

| Crate | Version | Purpose |
|-------|---------|---------|
| `uuid` | `1.x` | Request ID generation for tracing correlation |
| `dashmap` | `5.x` | Already used — circuit breaker concurrent map |

No new dependencies beyond `uuid` (if not already present).

---

## 10. Design Rationale

### 10.1 Why a Unified ValueStream?

```
Without ValueStream:
  - Local values: Vec<u8> → chunk manually in get() handler
  - Remote values: tonic::Streaming<DataChunk> → different code path
  - Each resolution path has its own streaming logic
  - Proxy code duplicated for each source type

With ValueStream:
  - Single abstraction: next_chunk() → Option<Result<Bytes>>
  - get() handler doesn't care where data came from
  - Proxy is trivial: read from upstream ValueStream, write to channel
  - Easy to add new sources (e.g., disk-backed streaming)
```

### 10.2 Why FetchValue is Local-Only (No Recursion)?

```
If FetchValue recursed into distributed get:
  Node A → FetchValue(B) → B doesn't have it → B → FetchValue(C) → ...

  Risk: infinite forwarding loops
  Risk: unbounded latency
  Risk: hard to reason about correctness

Instead: FetchValue checks ONLY local cache and leaf store.
  If not found locally → returns NotFound → caller tries next strategy.

  The entry node (Node A) owns the resolution strategy.
  Peers are passive data sources, not active resolvers.
```

### 10.3 Why No Request Deduplication?

```
Cost of dedup:
  - Waiters map: DashMap<CAddr, Vec<oneshot::Sender>>
  - Lock contention on hot addresses
  - Complexity: cleanup on timeout, error propagation to all waiters
  - Risk: head-of-line blocking

Benefit of dedup:
  - Avoid duplicate computation for concurrent gets of same addr
  - Save network bandwidth for duplicate remote fetches

Analysis:
  - Cache makes dedup mostly unnecessary (second get hits cache)
  - Concurrent gets for same addr are rare (different clients, different addrs)
  - Computation is idempotent (duplicate work is wasteful but correct)

  Decision: skip dedup. Add later if profiling shows duplicate work.
```

### 10.4 Why Circuit Breaker Instead of Retry with Backoff?

```
Retry with backoff:
  + Eventually succeeds if node recovers
  - Each request independently retries → N requests × M retries
  - Slow: exponential backoff adds seconds per request
  - No shared state between requests

Circuit breaker:
  + Shared state: one node's failure benefits all requests
  + Fast: skip failing node immediately after threshold
  + Self-healing: half-open state probes recovery
  - May skip a node that just recovered (30s delay)

  For distributed get, fast failure is more important than
  guaranteed delivery. The system has fallbacks (other replicas,
  local computation) that don't require the failing node.
```

### 10.5 Why Priority Order: Cache → Leaf → Remote Cache → Remote Leaf → Compute?

```
Ordered by cost (cheapest first):

  1. Local cache:   ~0.5ms, 0 network, 0 disk
  2. Local leaf:    ~2ms, 0 network, 1 disk read
  3. Remote cache:  ~4ms, 1 RPC, 0 disk (peer's memory)
  4. Remote leaf:   ~6ms, 1 RPC, 1 disk (peer's disk)
  5. Compute:       ~50ms+, possibly 1+ RPC, CPU-bound

  Each step is strictly more expensive than the previous.
  Short-circuiting at the earliest hit minimizes latency.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `distributed_get_total` | Counter | — | Total get requests |
| `distributed_get_source` | Counter | `source={local_cache,local_leaf,remote_cache,remote_leaf,computed_local,computed_remote,computed_local_fallback}` | Resolution source |
| `distributed_get_latency_ms` | Histogram | `source` | End-to-end latency |
| `distributed_get_errors` | Counter | `error={not_found,timeout,network,internal}` | Error types |
| `distributed_get_hops` | Histogram | — | Number of remote hops |
| `distributed_get_bytes` | Counter | — | Total bytes streamed |
| `circuit_breaker_state` | Gauge | `peer`, `state={closed,open,half_open}` | Per-peer state |
| `circuit_breaker_trips` | Counter | `peer` | Times circuit opened |
| `fetch_value_received` | Counter | `result={hit,miss}` | FetchValue RPC outcomes |
| `remote_fetch_concurrent` | Gauge | — | Current concurrent fetches |

### 11.2 Structured Logging

```rust
// Request lifecycle logging
tracing::info!(
    addr = %addr,
    request_id = %request_id,
    "distributed get started"
);

tracing::debug!(
    addr = %addr,
    source = "remote_cache",
    peer = %peer,
    latency_ms = elapsed.as_millis(),
    "distributed get resolved"
);

tracing::warn!(
    addr = %addr,
    peer = %peer,
    error = %e,
    attempt = attempt_num,
    "remote fetch failed, trying next candidate"
);

tracing::error!(
    addr = %addr,
    request_id = %request_id,
    error = %e,
    elapsed_ms = started.elapsed().as_millis(),
    "distributed get failed after all fallbacks"
);
```

### 11.3 Distributed Tracing

```
Each distributed get creates a trace span:

  [distributed_get addr=X request_id=abc]
    ├── [check_local] → miss
    ├── [try_remote_cache]
    │   ├── [fetch_from_peer peer=B] → not_found (bloom FP)
    │   └── [fetch_from_peer peer=C] → success, 4ms
    └── [stream_to_client] → 1.2MB, 3 chunks

  Span attributes:
    - addr, request_id
    - source (final resolution source)
    - hops (number of remote calls)
    - bytes_streamed
    - latency_ms
```

### 11.4 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| High get error rate | `distributed_get_errors` > 5% of total | Critical |
| Get latency spike | p99 `distributed_get_latency_ms` > 5s | Warning |
| Circuit breaker open | Any `circuit_breaker_state{state=open}` > 0 | Warning |
| Fetch semaphore saturated | `remote_fetch_concurrent` = 64 for >30s | Warning |
| High fallback rate | `computed_local_fallback` > 20% of computes | Info |

---

## 12. Checklist

- [ ] Create `deriva-network/src/distributed_get.rs`
- [ ] Implement `DistributedGetResolver` with 6-step resolution
- [ ] Create `deriva-network/src/value_stream.rs`
- [ ] Implement `ValueStream` with local and remote variants
- [ ] Create `deriva-network/src/circuit_breaker.rs`
- [ ] Implement `CircuitBreaker` with closed/open/half-open states
- [ ] Add `FetchValue` RPC to `proto/deriva_internal.proto`
- [ ] Implement `fetch_value` handler (local-only, no recursion)
- [ ] Update `get()` in `deriva-server` to use distributed resolver
- [ ] Add `DistributedGetResolver` to `ServerState`
- [ ] Integrate with §3.5 routing for compute decisions
- [ ] Integrate with §3.3 replica ring for leaf fetching
- [ ] Integrate with §3.4 bloom filters for cache discovery
- [ ] Add fetch semaphore for backpressure (64 concurrent)
- [ ] Add request_id for tracing correlation
- [ ] Add deadline enforcement (30s total timeout)
- [ ] Write circuit breaker unit tests (6 tests)
- [ ] Write ValueStream unit tests (4 tests)
- [ ] Write config test (1 test)
- [ ] Write integration tests (6 tests)
- [ ] Add metrics (10 metrics)
- [ ] Add structured log events
- [ ] Add distributed tracing spans
- [ ] Configure alerts (5 alerts)
- [ ] Run benchmarks: cache hit throughput, remote fetch latency, proxy throughput
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
