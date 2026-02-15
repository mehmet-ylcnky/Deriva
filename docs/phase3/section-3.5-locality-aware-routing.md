# §3.5 Locality-Aware Compute Routing

> **Status**: Blueprint  
> **Depends on**: §3.1 SWIM Gossip, §3.3 Leaf Data Replication, §3.4 Cache Placement  
> **Crate(s)**: `deriva-network`, `deriva-compute`, `deriva-server`  
> **Estimated effort**: 4–5 days  

---

## 1. Problem Statement

### 1.1 Current Limitation

When a node needs to materialize a recipe, it always computes locally —
even if all the inputs are cached on a different node. This means:

1. **Unnecessary data transfer**: Inputs must be fetched to the computing node
2. **Wasted bandwidth**: Large inputs traverse the network for every computation
3. **Suboptimal latency**: Transfer time dominates for large-input recipes

### 1.2 The Opportunity

SWIM gossip (§3.1) already propagates bloom filters of each node's cache
contents and metadata (cpu_load, memory stats). We can use this information
to route computation to the node that already has the most input data local.

```
Example — Recipe R = f(A, B, C) where A=500MB, B=200MB, C=100MB

Without routing (compute on requester Node X):
  Fetch A from Node 1: 500MB transfer (~4s)
  Fetch B from Node 2: 200MB transfer (~1.6s)
  Fetch C from Node 1: 100MB transfer (~0.8s)
  Compute f:           ~1s
  Total:               ~7.4s

With routing (compute on Node 1 which has A and C cached):
  Route request to Node 1: ~1ms
  Node 1 fetches B from Node 2: 200MB (~1.6s)
  Node 1 computes f: ~1s
  Stream result back to X: depends on result size
  Total:              ~2.7s (2.7× faster)
```

### 1.3 Design Goals

| Goal | Mechanism |
|------|-----------|
| Minimize data transfer | Route to node with most input bytes local |
| Avoid overloading nodes | Factor in cpu_load from gossip metadata |
| Fast routing decisions | Bloom filter lookup, no extra RPCs |
| Graceful fallback | Compute locally if routing fails or no benefit |
| No circular routing | Request carries hop count, max 1 forward |

### 1.4 What This Section Does NOT Cover

- **Distributed get protocol** (§3.6) — how a client request flows through the cluster
- **Cache placement after routing** (§3.4) — where to cache the result
- **Recipe replication** (§3.2) — recipes are already on all nodes

---

## 2. Design

### 2.1 Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│              Locality-Aware Compute Routing                      │
│                                                                   │
│  Node X receives get(R) where R = f(A, B, C)                    │
│                                                                   │
│  Step 1: Check local cache for R                                 │
│    cache.get(R) → miss                                           │
│                                                                   │
│  Step 2: Resolve recipe R → inputs [A, B, C]                    │
│                                                                   │
│  Step 3: Compute routing score for each alive node               │
│    ┌──────────────────────────────────────────────┐              │
│    │ Node   │ Has A? │ Has B? │ Has C? │ Score    │              │
│    ├────────┼────────┼────────┼────────┼──────────┤              │
│    │ X(self)│ ✗      │ ✗      │ ✓(100M)│ 100MB    │              │
│    │ Node 1 │ ✓(500M)│ ✗      │ ✓(100M)│ 600MB    │ ← best     │
│    │ Node 2 │ ✗      │ ✓(200M)│ ✗      │ 200MB    │              │
│    │ Node 3 │ ✗      │ ✗      │ ✗      │ 0MB      │              │
│    └──────────────────────────────────────────────┘              │
│                                                                   │
│  Step 4: Route to Node 1 (highest score, above threshold)        │
│    Forward ComputeRequest(R) → Node 1                            │
│    Node 1 materializes R locally (fetches only B from Node 2)    │
│    Node 1 streams result back to X                               │
│    X returns result to client                                     │
│                                                                   │
│  Step 5: Placement decision (§3.4)                               │
│    Node 1 may cache result (it computed it)                       │
│    Node X may cache result (it requested it)                      │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 Routing Cost Model

The routing decision compares the cost of local computation vs remote
computation. Cost is measured in estimated bytes transferred.

```
For recipe R with inputs [I₁, I₂, ..., Iₙ]:

  local_cost(node) = Σ size(Iₖ) for each Iₖ NOT local to node
  
  route_cost(target) = local_cost(target)     // target fetches missing inputs
                      + result_size            // stream result back
                      + routing_overhead       // ~1ms fixed cost

  Decision:
    best_target = argmin(route_cost(node)) for all alive nodes
    
    if route_cost(best_target) < local_cost(self) × savings_threshold:
      route to best_target
    else:
      compute locally
```

### 2.3 Savings Threshold

We only route if the savings exceed a threshold (default 0.3 = 30%):

```
savings = 1.0 - (route_cost(best) / local_cost(self))

if savings > 0.3:
  route to best_target
else:
  compute locally (not worth the routing overhead)

Examples:
  local_cost=800MB, route_cost=200MB → savings=75% → ROUTE ✓
  local_cost=100MB, route_cost=80MB  → savings=20% → LOCAL ✗
  local_cost=0 (all local)           → savings=N/A → LOCAL ✓
```

### 2.4 Load-Aware Adjustment

A node with high CPU load should be penalized in routing decisions:

```
adjusted_score(node) = locality_score(node) × load_factor(node)

load_factor(node):
  cpu_load < 0.5  → 1.0  (no penalty)
  cpu_load < 0.8  → 0.7  (mild penalty)
  cpu_load < 0.95 → 0.3  (heavy penalty)
  cpu_load >= 0.95 → 0.0  (don't route here)

This prevents routing all computation to one node that happens
to have everything cached but is already overloaded.
```

### 2.5 Anti-Circular Routing

To prevent request loops (A routes to B, B routes back to A):

```
ComputeRequest {
    addr: CAddr,
    hop_count: u8,     // starts at 0
    origin: NodeId,    // who originally received the client request
    max_hops: u8,      // default 1
}

Rule: if hop_count >= max_hops → compute locally, never forward.

With max_hops=1: at most 1 forward hop.
  Client → Node A → Node B (computes) → result back to A → client
  
  Node B will NOT forward to Node C even if C has better locality,
  because hop_count would be 2 > max_hops(1).
```

### 2.6 Wire Protocol

```protobuf
// Added to DerivaInternal service

rpc RouteCompute(RouteComputeRequest) returns (stream RouteComputeChunk);

message RouteComputeRequest {
    bytes addr = 1;           // CAddr to materialize
    uint32 hop_count = 2;     // current hop count
    uint32 max_hops = 3;      // max allowed hops (default 1)
    string origin_node = 4;   // node that received client request
}

message RouteComputeChunk {
    bytes data = 1;           // 64KB result chunks
    bool is_last = 2;
    bool cache_hit = 3;       // was this a cache hit on the remote?
    string computed_by = 4;   // which node actually computed it
}
```

---

## 3. Implementation

### 3.1 Routing Score Calculator

Location: `crates/deriva-network/src/routing.rs`

```rust
use deriva_core::{CAddr, Recipe};
use crate::swim::SwimRuntime;
use crate::types::NodeId;

/// Configuration for compute routing.
#[derive(Debug, Clone)]
pub struct RoutingConfig {
    /// Minimum savings ratio to justify routing (0.0–1.0).
    pub savings_threshold: f64,
    /// Maximum hops for forwarded requests.
    pub max_hops: u8,
    /// Whether to factor in CPU load.
    pub load_aware: bool,
    /// Estimated overhead per routing hop in bytes.
    pub routing_overhead_bytes: u64,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            savings_threshold: 0.3,
            max_hops: 1,
            load_aware: true,
            routing_overhead_bytes: 65536, // 64KB overhead estimate
        }
    }
}

/// Result of a routing decision.
#[derive(Debug, Clone)]
pub enum RoutingDecision {
    /// Compute locally on this node.
    Local,
    /// Route to a remote node for computation.
    Remote {
        target: NodeId,
        estimated_savings_pct: f64,
    },
}

/// Scores a node's locality for a given recipe's inputs.
#[derive(Debug, Clone)]
pub struct NodeScore {
    pub node_id: NodeId,
    pub local_bytes: u64,
    pub missing_bytes: u64,
    pub cpu_load: f64,
    pub adjusted_score: f64,
}

/// Compute routing engine.
pub struct ComputeRouter {
    config: RoutingConfig,
}

impl ComputeRouter {
    pub fn new(config: RoutingConfig) -> Self {
        Self { config }
    }

    /// Decide where to compute a recipe.
    ///
    /// Examines bloom filters and metadata from gossip to find
    /// the node with the best input locality.
    pub async fn route(
        &self,
        recipe: &Recipe,
        input_sizes: &[(CAddr, u64)], // (input_addr, estimated_size)
        local_id: &NodeId,
        swim: &SwimRuntime,
        hop_count: u8,
    ) -> RoutingDecision {
        // Never forward if we've hit max hops
        if hop_count >= self.config.max_hops {
            return RoutingDecision::Local;
        }

        let total_input_bytes: u64 = input_sizes.iter()
            .map(|(_, size)| size)
            .sum();

        // If no inputs or tiny inputs, compute locally
        if total_input_bytes < self.config.routing_overhead_bytes {
            return RoutingDecision::Local;
        }

        // Score all alive nodes
        let members = swim.alive_members_with_metadata().await;
        let mut scores: Vec<NodeScore> = Vec::new();

        for (node_id, metadata) in &members {
            let bloom = match &metadata.cache_bloom {
                Some(b) => b,
                None => continue,
            };

            let mut local_bytes = 0u64;
            let mut missing_bytes = 0u64;

            for (input_addr, size) in input_sizes {
                if *node_id == *local_id {
                    // For local node, check actual cache/store
                    // (bloom filter check is a simplification here)
                    if bloom.might_contain(input_addr) {
                        local_bytes += size;
                    } else {
                        missing_bytes += size;
                    }
                } else {
                    if bloom.might_contain(input_addr) {
                        local_bytes += size;
                    } else {
                        missing_bytes += size;
                    }
                }
            }

            let load_factor = if self.config.load_aware {
                Self::load_factor(metadata.cpu_load)
            } else {
                1.0
            };

            let adjusted_score = (local_bytes as f64) * load_factor;

            scores.push(NodeScore {
                node_id: node_id.clone(),
                local_bytes,
                missing_bytes,
                cpu_load: metadata.cpu_load,
                adjusted_score,
            });
        }

        // Find best remote node (excluding self)
        let local_score = scores.iter()
            .find(|s| s.node_id == *local_id)
            .map(|s| s.adjusted_score)
            .unwrap_or(0.0);

        let best_remote = scores.iter()
            .filter(|s| s.node_id != *local_id)
            .max_by(|a, b| a.adjusted_score.partial_cmp(&b.adjusted_score)
                .unwrap_or(std::cmp::Ordering::Equal));

        let best_remote = match best_remote {
            Some(r) if r.adjusted_score > 0.0 => r,
            _ => return RoutingDecision::Local,
        };

        // Calculate savings
        let local_cost = total_input_bytes as f64 - local_score;
        let remote_cost = best_remote.missing_bytes as f64
            + self.config.routing_overhead_bytes as f64;

        if local_cost <= 0.0 {
            return RoutingDecision::Local; // all inputs already local
        }

        let savings = 1.0 - (remote_cost / local_cost);

        if savings >= self.config.savings_threshold {
            RoutingDecision::Remote {
                target: best_remote.node_id.clone(),
                estimated_savings_pct: savings * 100.0,
            }
        } else {
            RoutingDecision::Local
        }
    }

    /// CPU load penalty factor.
    fn load_factor(cpu_load: f64) -> f64 {
        if cpu_load < 0.5 { 1.0 }
        else if cpu_load < 0.8 { 0.7 }
        else if cpu_load < 0.95 { 0.3 }
        else { 0.0 }
    }
}
```

### 3.2 Input Size Estimation

We need input sizes for the cost model. Since we don't always know exact
sizes, we use estimates from gossip metadata and local stores:

```rust
/// Estimate sizes of recipe inputs.
///
/// Uses local cache/blob store for known sizes,
/// falls back to a default estimate for unknown inputs.
pub async fn estimate_input_sizes(
    recipe: &Recipe,
    cache: &EvictableCache,
    blob_store: &dyn StorageBackend,
) -> Vec<(CAddr, u64)> {
    let default_size: u64 = 1024 * 1024; // 1MB default estimate
    let mut sizes = Vec::with_capacity(recipe.inputs.len());

    for input_addr in &recipe.inputs {
        let size = if let Some(entry) = cache.get_entry(input_addr) {
            entry.size as u64
        } else if let Ok(size) = blob_store.size(input_addr).await {
            size
        } else {
            default_size
        };
        sizes.push((*input_addr, size));
    }

    sizes
}
```

### 3.3 RouteCompute RPC Handler

Location: `crates/deriva-server/src/internal.rs`

```rust
/// Handle an incoming routed compute request from a peer.
async fn route_compute(
    &self,
    request: Request<RouteComputeRequest>,
) -> Result<Response<tonic::Streaming<RouteComputeChunk>>, Status> {
    let req = request.get_ref();
    let addr = parse_addr(&req.addr)?;
    let hop_count = req.hop_count;

    // Check local cache first
    if let Some(value) = self.state.cache.get(&addr) {
        let chunks = value_to_chunks(&value, &self.state.local_id, true);
        return Ok(Response::new(chunks));
    }

    // Materialize locally (this node was chosen for its input locality)
    let value = self.state.executor.materialize(
        &addr,
        hop_count + 1, // increment hop count for any sub-routing
    ).await.map_err(|e| Status::internal(e.to_string()))?;

    let chunks = value_to_chunks(&value, &self.state.local_id, false);
    Ok(Response::new(chunks))
}

/// Convert a value into a stream of 64KB chunks.
fn value_to_chunks(
    value: &Value,
    computed_by: &NodeId,
    cache_hit: bool,
) -> impl Stream<Item = Result<RouteComputeChunk, Status>> {
    let data = value.as_bytes().to_vec();
    let node_name = computed_by.to_string();
    let chunk_size = 65536; // 64KB

    let chunks: Vec<_> = data.chunks(chunk_size)
        .enumerate()
        .map(|(i, chunk)| {
            let is_last = (i + 1) * chunk_size >= data.len();
            Ok(RouteComputeChunk {
                data: chunk.to_vec(),
                is_last,
                cache_hit,
                computed_by: node_name.clone(),
            })
        })
        .collect();

    futures::stream::iter(chunks)
}
```

### 3.4 Integration with Executor

The executor's materialize path is updated to consult the router:

```rust
/// Materialize with routing awareness.
///
/// Before computing locally, check if another node would be
/// more efficient due to input locality.
pub async fn materialize_routed(
    &self,
    addr: &CAddr,
    hop_count: u8,
) -> Result<Value, DerivaError> {
    // Step 1: Check local cache
    if let Some(value) = self.cache.get(addr) {
        return Ok(value);
    }

    // Step 2: Resolve recipe (available locally per §3.2)
    let recipe = self.dag.read().await
        .get_recipe(addr)
        .ok_or_else(|| DerivaError::NotFound(format!("{}", addr)))?
        .clone();

    // Step 3: Routing decision
    if let Some(ref router) = self.router {
        let input_sizes = estimate_input_sizes(
            &recipe, &self.cache, &*self.blob_store,
        ).await;

        let decision = router.route(
            &recipe, &input_sizes, &self.local_id, &self.swim, hop_count,
        ).await;

        if let RoutingDecision::Remote { target, estimated_savings_pct } = &decision {
            tracing::info!(
                addr = %addr,
                target = %target,
                savings = %estimated_savings_pct,
                "routing computation to remote node"
            );

            match self.forward_compute(addr, target, hop_count).await {
                Ok(value) => return Ok(value),
                Err(e) => {
                    tracing::warn!(
                        addr = %addr,
                        target = %target,
                        error = %e,
                        "remote compute failed, falling back to local"
                    );
                    // Fall through to local computation
                }
            }
        }
    }

    // Step 4: Compute locally (original path)
    self.materialize_local(addr, &recipe, hop_count).await
}

/// Forward a compute request to a remote node.
async fn forward_compute(
    &self,
    addr: &CAddr,
    target: &NodeId,
    hop_count: u8,
) -> Result<Value, DerivaError> {
    let grpc_addr = self.swim.get_grpc_addr(target).await
        .ok_or_else(|| DerivaError::Network("no grpc addr for target".into()))?;

    let endpoint = format!("http://{}", grpc_addr);
    let channel = Channel::from_shared(endpoint)
        .map_err(|e| DerivaError::Network(e.to_string()))?
        .connect_timeout(Duration::from_secs(5))
        .connect()
        .await
        .map_err(|e| DerivaError::Network(e.to_string()))?;

    let mut client = DerivaInternalClient::new(channel);
    let request = tonic::Request::new(RouteComputeRequest {
        addr: addr.as_bytes().to_vec(),
        hop_count: (hop_count + 1) as u32,
        max_hops: self.router.as_ref()
            .map(|r| r.config.max_hops as u32)
            .unwrap_or(1),
        origin_node: self.local_id.to_string(),
    });

    let mut stream = client.route_compute(request)
        .await
        .map_err(|e| DerivaError::Network(e.to_string()))?
        .into_inner();

    let mut data = Vec::new();
    while let Some(chunk) = stream.message().await
        .map_err(|e| DerivaError::Network(e.to_string()))?
    {
        data.extend_from_slice(&chunk.data);
        if chunk.is_last { break; }
    }

    Value::from_bytes(&data)
}
```


---

## 4. Data Flow Diagrams

### 4.1 Routing Decision — Route to Remote

```
  Client          Node X (coord)         Node 1 (best)       Node 2
    │                  │                     │                  │
    │ get(R)           │                     │                  │
    │─────────────────▶│                     │                  │
    │                  │                     │                  │
    │                  │ cache.get(R) → miss  │                  │
    │                  │ resolve R → f(A,B,C) │                  │
    │                  │                     │                  │
    │                  │ Routing scores:      │                  │
    │                  │  X: A=✗ B=✗ C=✓     │                  │
    │                  │     local=100MB      │                  │
    │                  │  1: A=✓ B=✗ C=✓     │                  │
    │                  │     local=600MB ★    │                  │
    │                  │  2: A=✗ B=✓ C=✗     │                  │
    │                  │     local=200MB      │                  │
    │                  │                     │                  │
    │                  │ savings = 1-(264K/700MB) = 99.96%      │
    │                  │ threshold = 30% → ROUTE ✓              │
    │                  │                     │                  │
    │                  │ RouteCompute(R,hop=0)│                  │
    │                  │────────────────────▶│                  │
    │                  │                     │ cache.get(R)→miss│
    │                  │                     │ resolve R→f(A,B,C)
    │                  │                     │ A=local ✓        │
    │                  │                     │ C=local ✓        │
    │                  │                     │ B=need from 2    │
    │                  │                     │ FetchLeaf(B)     │
    │                  │                     │────────────────▶│
    │                  │                     │◀────────────────│
    │                  │                     │ compute f(A,B,C) │
    │                  │                     │ → result         │
    │                  │  stream result      │                  │
    │                  │◀────────────────────│                  │
    │  stream result   │                     │                  │
    │◀─────────────────│                     │                  │
```

### 4.2 Routing Decision — Compute Locally (No Benefit)

```
  Node X receives get(R) where R = f(A)
    │
    │ cache.get(R) → miss
    │ resolve R → f(A), A is 50KB
    │
    │ Routing scores:
    │   X: A=local ✓ → local_bytes=50KB
    │   1: A=✗       → local_bytes=0
    │   2: A=✗       → local_bytes=0
    │
    │ local_cost(X) = 0 (all inputs local)
    │ → RoutingDecision::Local
    │
    │ Compute f(A) locally → result
    │ Return to client
```

### 4.3 Routing Decision — Fallback on Remote Failure

```
  Node X          Node 1 (chosen)       Node 2
    │                  │                   │
    │ RouteCompute(R)  │                   │
    │─────────────────▶│                   │
    │                  │ ✗ CRASH / TIMEOUT │
    │                  │                   │
    │ Timeout (5s)     │                   │
    │                  │                   │
    │ Fallback: compute locally            │
    │ Fetch A from Node 2                  │
    │──────────────────────────────────────▶│
    │◀──────────────────────────────────────│
    │ Compute f(A) locally                 │
    │ Return to client                     │
```

### 4.4 Anti-Circular Routing (hop_count enforcement)

```
  Client → Node A → Node B → (would route to Node C, but hop_count=1 ≥ max_hops=1)
                              → compute locally on Node B

  Detailed:
    Client sends get(R) to Node A
    Node A: hop_count=0, route to Node B (best locality)
    Node A sends RouteCompute(R, hop_count=1) to Node B
    Node B: hop_count=1 >= max_hops=1 → MUST compute locally
    Node B materializes R, streams result to A
    Node A streams result to client

  Maximum chain length: Client → A → B (2 nodes, 1 hop)
  Never: Client → A → B → C (would be 2 hops)
```

### 4.5 Load-Aware Routing Avoids Hot Node

```
  Recipe R = f(A, B) — A=1GB, B=500MB

  Node scores (before load adjustment):
    Node 1: has A+B → local=1.5GB (best raw score)
    Node 2: has A   → local=1GB
    Node 3: has B   → local=500MB

  CPU loads from gossip:
    Node 1: cpu_load=0.97 → load_factor=0.0 (overloaded!)
    Node 2: cpu_load=0.30 → load_factor=1.0
    Node 3: cpu_load=0.45 → load_factor=1.0

  Adjusted scores:
    Node 1: 1.5GB × 0.0 = 0        ← penalized to zero
    Node 2: 1.0GB × 1.0 = 1.0GB    ← new best
    Node 3: 0.5GB × 1.0 = 0.5GB

  Route to Node 2 (not overloaded, has 1GB of inputs)
  Node 2 fetches only B (500MB) instead of X fetching A+B (1.5GB)
```

---

## 5. Test Specification

### 5.1 Unit Tests — Load Factor

```rust
#[test]
fn test_load_factor_low_load() {
    assert_eq!(ComputeRouter::load_factor(0.0), 1.0);
    assert_eq!(ComputeRouter::load_factor(0.3), 1.0);
    assert_eq!(ComputeRouter::load_factor(0.49), 1.0);
}

#[test]
fn test_load_factor_medium_load() {
    assert_eq!(ComputeRouter::load_factor(0.5), 0.7);
    assert_eq!(ComputeRouter::load_factor(0.7), 0.7);
    assert_eq!(ComputeRouter::load_factor(0.79), 0.7);
}

#[test]
fn test_load_factor_high_load() {
    assert_eq!(ComputeRouter::load_factor(0.8), 0.3);
    assert_eq!(ComputeRouter::load_factor(0.9), 0.3);
    assert_eq!(ComputeRouter::load_factor(0.94), 0.3);
}

#[test]
fn test_load_factor_overloaded() {
    assert_eq!(ComputeRouter::load_factor(0.95), 0.0);
    assert_eq!(ComputeRouter::load_factor(1.0), 0.0);
}
```

### 5.2 Unit Tests — Routing Decision

```rust
#[test]
fn test_route_local_when_all_inputs_local() {
    let router = ComputeRouter::new(RoutingConfig::default());
    // Setup: local node has all inputs in bloom filter
    // All other nodes also have some inputs
    // But local_cost(self) = 0 → always local
    let decision = block_on(router.route(
        &recipe_with_inputs(3),
        &[(addr("a"), 1000), (addr("b"), 2000), (addr("c"), 3000)],
        &local_id(),
        &swim_with_all_local(),
        0,
    ));
    assert!(matches!(decision, RoutingDecision::Local));
}

#[test]
fn test_route_local_when_tiny_inputs() {
    let router = ComputeRouter::new(RoutingConfig::default());
    // Inputs total < routing_overhead_bytes (64KB)
    let decision = block_on(router.route(
        &recipe_with_inputs(2),
        &[(addr("a"), 100), (addr("b"), 200)], // 300 bytes total
        &local_id(),
        &swim_with_remote_locality(),
        0,
    ));
    assert!(matches!(decision, RoutingDecision::Local));
}

#[test]
fn test_route_remote_when_significant_savings() {
    let router = ComputeRouter::new(RoutingConfig::default());
    // Local: 0 inputs cached → must fetch 100MB
    // Remote Node 1: has 90MB of inputs cached
    let decision = block_on(router.route(
        &recipe_with_inputs(2),
        &[(addr("a"), 90_000_000), (addr("b"), 10_000_000)],
        &local_id(),
        &swim_where_node1_has_input_a(),
        0,
    ));
    assert!(matches!(decision, RoutingDecision::Remote { .. }));
}

#[test]
fn test_route_local_when_below_savings_threshold() {
    let router = ComputeRouter::new(RoutingConfig {
        savings_threshold: 0.5, // require 50% savings
        ..Default::default()
    });
    // Local: missing 100MB, Remote: missing 80MB → savings=20% < 50%
    let decision = block_on(router.route(
        &recipe_with_inputs(2),
        &[(addr("a"), 50_000_000), (addr("b"), 50_000_000)],
        &local_id(),
        &swim_where_node1_has_input_a(), // node1 has 50MB local
        0,
    ));
    assert!(matches!(decision, RoutingDecision::Local));
}

#[test]
fn test_route_local_when_max_hops_reached() {
    let router = ComputeRouter::new(RoutingConfig::default());
    // Even with great remote locality, hop_count >= max_hops → local
    let decision = block_on(router.route(
        &recipe_with_inputs(2),
        &[(addr("a"), 90_000_000), (addr("b"), 10_000_000)],
        &local_id(),
        &swim_where_node1_has_input_a(),
        1, // hop_count=1 >= max_hops=1
    ));
    assert!(matches!(decision, RoutingDecision::Local));
}

#[test]
fn test_route_avoids_overloaded_node() {
    let router = ComputeRouter::new(RoutingConfig::default());
    // Node 1 has best locality but cpu_load=0.97
    // Node 2 has decent locality and cpu_load=0.3
    let decision = block_on(router.route(
        &recipe_with_inputs(2),
        &[(addr("a"), 90_000_000), (addr("b"), 10_000_000)],
        &local_id(),
        &swim_where_node1_overloaded_node2_ok(),
        0,
    ));
    match decision {
        RoutingDecision::Remote { target, .. } => {
            assert_ne!(target, node_id("node1"), "should not route to overloaded node");
        }
        RoutingDecision::Local => {} // also acceptable
    }
}
```

### 5.3 Unit Tests — RoutingConfig

```rust
#[test]
fn test_routing_config_defaults() {
    let config = RoutingConfig::default();
    assert!((config.savings_threshold - 0.3).abs() < f64::EPSILON);
    assert_eq!(config.max_hops, 1);
    assert!(config.load_aware);
    assert_eq!(config.routing_overhead_bytes, 65536);
}
```

### 5.4 Unit Tests — Input Size Estimation

```rust
#[tokio::test]
async fn test_estimate_sizes_from_cache() {
    let cache = mock_cache_with_entry(addr("a"), 5000);
    let blob = mock_blob_store();
    let recipe = recipe_with_inputs_addrs(vec![addr("a")]);

    let sizes = estimate_input_sizes(&recipe, &cache, &blob).await;
    assert_eq!(sizes.len(), 1);
    assert_eq!(sizes[0].1, 5000);
}

#[tokio::test]
async fn test_estimate_sizes_fallback_default() {
    let cache = empty_cache();
    let blob = empty_blob_store();
    let recipe = recipe_with_inputs_addrs(vec![addr("unknown")]);

    let sizes = estimate_input_sizes(&recipe, &cache, &blob).await;
    assert_eq!(sizes[0].1, 1024 * 1024); // 1MB default
}
```

### 5.5 Integration Tests

```rust
#[tokio::test]
async fn test_routed_compute_uses_remote_node() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (mut client_b, _sb) = start_cluster_node(1).await;
    let (mut client_c, _sc) = start_cluster_node(2).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Put large leaf on node B
    let big_data = vec![0u8; 10_000_000]; // 10MB
    let leaf = put_leaf(&mut client_b, &big_data).await;
    let recipe = put_recipe(&mut client_a, "identity", "v1", vec![leaf]).await;

    // Materialize on B so it's cached there
    let _ = get(&mut client_b, &recipe).await;

    // Wait for bloom filter gossip
    tokio::time::sleep(Duration::from_secs(15)).await;

    // Request from A — should route to B (has input cached)
    let result = get(&mut client_a, &recipe).await;
    assert_eq!(result.len(), 10_000_000);
}

#[tokio::test]
async fn test_routing_falls_back_on_remote_failure() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (_, server_b) = start_cluster_node(1).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    let leaf = put_leaf(&mut client_a, b"data").await;
    let recipe = put_recipe(&mut client_a, "identity", "v1", vec![leaf]).await;

    // Kill B (would-be routing target)
    drop(server_b);
    tokio::time::sleep(Duration::from_secs(1)).await;

    // A should fall back to local computation
    let result = get(&mut client_a, &recipe).await;
    assert_eq!(result, b"data");
}

#[tokio::test]
async fn test_hop_count_prevents_circular_routing() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (_, _sb) = start_cluster_node(1).await;
    let (_, _sc) = start_cluster_node(2).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    let leaf = put_leaf(&mut client_a, b"data").await;
    let recipe = put_recipe(&mut client_a, "identity", "v1", vec![leaf]).await;

    // Even with routing enabled, request completes (no infinite loop)
    let result = get(&mut client_a, &recipe).await;
    assert_eq!(result, b"data");
}
```


### 5.6 Integration Tests — Pipeline Routing

```rust
#[tokio::test]
async fn test_pipeline_routing_chains_correctly() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (mut client_b, _sb) = start_cluster_node(1).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Build pipeline: leaf → f → g → h
    let leaf = put_leaf(&mut client_a, &vec![0u8; 1_000_000]).await; // 1MB
    let step1 = put_recipe(&mut client_a, "f", "v1", vec![leaf]).await;
    let step2 = put_recipe(&mut client_a, "g", "v1", vec![step1]).await;
    let step3 = put_recipe(&mut client_a, "h", "v1", vec![step2]).await;

    // Materialize full pipeline on A (caches intermediates)
    let _ = get(&mut client_a, &step3).await;

    // Wait for bloom gossip
    tokio::time::sleep(Duration::from_secs(15)).await;

    // Request step3 from B — should route to A (has all intermediates)
    let result = get(&mut client_b, &step3).await;
    assert!(!result.is_empty());
}

#[tokio::test]
async fn test_routing_with_shared_inputs() {
    // Two recipes share the same large input
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (mut client_b, _sb) = start_cluster_node(1).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    let big_leaf = put_leaf(&mut client_a, &vec![42u8; 5_000_000]).await;
    let r1 = put_recipe(&mut client_a, "f", "v1", vec![big_leaf]).await;
    let r2 = put_recipe(&mut client_a, "g", "v1", vec![big_leaf]).await;

    // Materialize r1 on A (caches big_leaf and result)
    let _ = get(&mut client_a, &r1).await;
    tokio::time::sleep(Duration::from_secs(15)).await;

    // Request r2 from B — should route to A (has big_leaf cached)
    let result = get(&mut client_b, &r2).await;
    assert!(!result.is_empty());
}

#[tokio::test]
async fn test_routing_disabled_config() {
    // With savings_threshold=1.0, routing never triggers
    let config = RoutingConfig {
        savings_threshold: 1.0, // impossible to meet
        ..Default::default()
    };
    let (mut client_a, _sa) = start_cluster_node_with_routing(0, config).await;
    let (_, _sb) = start_cluster_node(1).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    let leaf = put_leaf(&mut client_a, b"data").await;
    let recipe = put_recipe(&mut client_a, "identity", "v1", vec![leaf]).await;

    // Should always compute locally
    let result = get(&mut client_a, &recipe).await;
    assert_eq!(result, b"data");
}
```

---

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Single-node cluster | Always local (no peers) | No routing possible |
| 2 | All peers overloaded (cpu>0.95) | Compute locally | load_factor=0 for all |
| 3 | Bloom filter false positive | Route to peer, peer doesn't have input, peer fetches it | Slightly suboptimal but correct |
| 4 | Remote node crashes mid-compute | Timeout → fallback to local | Graceful degradation |
| 5 | Recipe has 0 inputs (leaf lookup) | Always local (no routing benefit) | Nothing to route for |
| 6 | All inputs on local node | Always local (savings=0) | No transfer needed |
| 7 | Circular routing attempt | hop_count >= max_hops → local | Anti-loop protection |
| 8 | Input size unknown | Use 1MB default estimate | Conservative estimate |
| 9 | Very large result (>1GB) | Stream back in 64KB chunks | Existing streaming protocol |
| 10 | Routing target leaves cluster | gRPC connect fails → fallback local | SWIM detects eventually |
| 11 | Concurrent routing to same node | Multiple requests routed, node handles concurrently | No coordination needed |
| 12 | Gossip metadata stale (10s old) | Routing decision based on slightly old data | Acceptable — bloom rebuilt every 10s |
| 13 | Recipe inputs are other recipes | Recursive — each sub-recipe may also route | hop_count limits depth |
| 14 | Network partition | Can only route to reachable partition | SWIM marks unreachable as suspect/dead |

### 6.1 Fallback Chain

```
Routing attempt:
  1. Try remote node (best locality score)
     ├─ Success → stream result back
     └─ Failure (timeout/crash/error)
         │
  2. Compute locally (original path)
     ├─ Success → return result
     └─ Failure (missing inputs, compute error)
         │
  3. Return error to client

No retry of routing to a different remote node — too complex,
local fallback is always available and correct.
```

### 6.2 Bloom Filter False Positive Impact

```
Scenario: Route to Node 1 because bloom says it has input A.
  Node 1 actually doesn't have A (false positive, ~1% chance).
  Node 1 must fetch A from its replica set (§3.3).

Impact:
  - Extra hop: Client → X → Node 1 → fetch A → compute → stream back
  - vs optimal: Client → X → fetch A → compute locally
  - Overhead: ~2ms extra (one additional RPC hop)
  - Frequency: ~1% of routing decisions

Mitigation: None needed. 1% false positive rate with 2ms overhead
is negligible. The 99% of correct routing decisions save seconds.
```

### 6.3 Recursive Recipe Routing

```
Recipe R = f(S) where S = g(A, B) (S is itself a recipe)

Node X receives get(R):
  1. Resolve R → f(S)
  2. S is a recipe, not a leaf — need to materialize S first
  3. Routing decision for S: check bloom filters for S (cached result)
     - If S cached on Node 1 → fetch from Node 1 (no compute needed)
     - If S not cached → routing decision for g(A, B)
       - Route g(A,B) to node with A and B
       - That node computes S, caches it, returns it
  4. Now have S → compute f(S) locally or route

Key: hop_count increments at each routing hop, not at each
recursion level. Recursive materialization on the SAME node
does not increment hop_count.

  materialize(R, hop=0) on Node X
    → needs S → materialize(S, hop=0) on Node X
      → route S to Node 1 (hop=0 < max=1)
      → Node 1: materialize(S, hop=1) — computes locally (hop=1 >= max)
      → returns S to Node X
    → compute f(S) on Node X
```

---

### 7.1 Routing Decision Overhead

```
ComputeRouter.route() cost:
  ┌────────────────────────────────┬──────────┐
  │ Operation                      │ Cost     │
  ├────────────────────────────────┼──────────┤
  │ swim.alive_members()           │ ~500ns   │
  │ Bloom check per peer per input │ ~100ns   │
  │ Score calculation per peer     │ ~50ns    │
  │ Savings comparison             │ ~10ns    │
  │ Total (10 peers, 5 inputs)     │ ~6μs     │
  │ Total (50 peers, 5 inputs)     │ ~27μs    │
  └────────────────────────────────┴──────────┘

  Compared to materialization time (ms–s): negligible.
```

### 7.2 Bandwidth Savings Model

```
Scenario: 3-node cluster, recipe with 3 inputs (500MB, 200MB, 100MB)

Without routing (worst case — no inputs local):
  Transfer: 800MB
  Time: ~6.4s (at 1Gbps)

With routing (best case — route to node with 600MB local):
  Transfer: 200MB (missing input) + result streaming
  Time: ~1.6s + result time
  Savings: 75% bandwidth reduction

Average savings across workloads (simulated):
  ┌──────────────────────┬──────────────┐
  │ Workload             │ Avg Savings  │
  ├──────────────────────┼──────────────┤
  │ Random access         │ 15-25%       │
  │ Locality-heavy        │ 40-60%       │
  │ Pipeline (chained)    │ 50-70%       │
  │ Fan-out (shared input)│ 30-50%       │
  └──────────────────────┴──────────────┘
```

### 7.3 Latency Impact

```
Routing adds latency:
  Decision time: ~6μs (negligible)
  Extra RPC hop: ~1ms (gRPC connect, cached channel)
  Result streaming: same as local (64KB chunks)

Net effect:
  Small inputs (<64KB): routing adds ~1ms, saves nothing → don't route
  Medium inputs (1-100MB): routing adds ~1ms, saves 10-800ms → route ✓
  Large inputs (>100MB): routing adds ~1ms, saves seconds → route ✓

Break-even point: ~100KB of input savings justifies 1ms routing overhead
(at 1Gbps network: 100KB transfer = ~0.8ms)
```

### 7.5 Simulation: Routing Impact on Pipeline Workloads

```
Pipeline: A → f → B → g → C → h → D

  A = 100MB leaf (stored on Node 1)
  B = f(A) = 50MB (computed, cached on Node 1)
  C = g(B) = 25MB (computed, cached on Node 1)
  D = h(C) = 10MB (requested by client on Node 2)

Without routing (all computed on Node 2):
  Fetch A from Node 1:  100MB → 800ms
  Compute f(A):         200ms
  Compute g(B):         100ms
  Compute h(C):         50ms
  Total:                1150ms

With routing (h(C) routed to Node 1 which has C cached):
  Route request to Node 1:  1ms
  Node 1: C is local ✓
  Compute h(C):              50ms
  Stream D (10MB) back:      80ms
  Total:                     131ms (8.8× faster)

Pipeline workloads benefit enormously from routing because
intermediate results accumulate on the node that computed them.
```

### 7.6 Routing Decision Quality vs Gossip Staleness

```
Bloom filters are rebuilt every 10 seconds.
Between rebuilds, cache contents may change:
  - New entries added (not yet in bloom) → missed routing opportunity
  - Entries evicted (still in bloom) → false positive routing

Impact analysis:
  ┌──────────────────────┬──────────────┬──────────────┐
  │ Staleness            │ Missed routes│ False routes  │
  ├──────────────────────┼──────────────┼──────────────┤
  │ 0-2s (fresh)         │ ~1%          │ ~0.5%         │
  │ 2-5s                 │ ~3%          │ ~1%           │
  │ 5-10s (stale)        │ ~5%          │ ~2%           │
  │ >10s (very stale)    │ N/A (rebuilt)│ N/A           │
  └──────────────────────┴──────────────┴──────────────┘

  Missed routes: compute locally instead of routing → correct but slower
  False routes: route to node without input → node fetches input → correct but suboptimal

  Both cases are correct — routing is an optimization, not a requirement.
  Average quality degradation from staleness: <5%
```

### 7.7 Benchmarking Plan

```rust
/// Benchmark: routing decision latency vs cluster size
#[bench]
fn bench_routing_decision(b: &mut Bencher) {
    // Setup: clusters of 3, 10, 50 nodes with bloom filters
    // Recipe with 5 inputs of varying sizes
    // Measure: route() call duration
    // Expected: <10μs for 10 nodes, <30μs for 50 nodes
}

/// Benchmark: end-to-end materialization with routing
#[bench]
fn bench_routed_materialization(b: &mut Bencher) {
    // Setup: 3-node cluster, recipe with 100MB input
    // Input cached on remote node
    // Measure: total get() time with vs without routing
    // Expected: 2-3× speedup with routing
}

/// Benchmark: routing overhead for small inputs
#[bench]
fn bench_routing_overhead_small(b: &mut Bencher) {
    // Setup: recipe with 1KB input (all local)
    // Measure: overhead of routing decision (should be ~0)
    // Expected: <10μs overhead, decision = Local
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/routing.rs` | **NEW** — ComputeRouter, RoutingConfig, RoutingDecision |
| `deriva-network/src/lib.rs` | Add `pub mod routing` |
| `proto/deriva_internal.proto` | Add RouteCompute RPC |
| `deriva-compute/src/executor.rs` | Add `materialize_routed`, `forward_compute` |
| `deriva-server/src/internal.rs` | Add `route_compute` handler |
| `deriva-server/src/state.rs` | Add `router: Option<ComputeRouter>` to ServerState |
| `deriva-network/tests/routing.rs` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

No new external dependencies. Uses existing tonic, tokio, and futures crates.

---

## 10. Design Rationale

### 10.1 Why Bloom Filters Instead of Exact Cache Directory?

| Approach | Overhead | Accuracy | Staleness |
|----------|---------|----------|-----------|
| Bloom filter (gossip) | 12KB/node, piggybacked | ~99% (1% FP) | ~10s |
| Central directory | 1 RPC per lookup | 100% | Real-time |
| Distributed hash table | Complex protocol | 100% | Varies |

Bloom filters are already propagated via SWIM gossip (§3.1) at zero
additional network cost. The 1% false positive rate is acceptable
because routing is an optimization, not a correctness requirement.

### 10.2 Why max_hops=1 Instead of Multi-Hop?

Multi-hop routing (A→B→C→D) adds complexity:
- Harder to reason about latency
- More failure points in the chain
- Diminishing returns (most benefit from first hop)
- Risk of circular routing without careful tracking

Single-hop (A→B) captures 80%+ of the routing benefit with minimal
complexity. The router on B sees hop_count=1 and always computes locally.

### 10.3 Why Savings Threshold Instead of Always Routing?

```
Without threshold:
  Route even for 1% savings → overhead (1ms) may exceed savings
  Unnecessary network hops for marginal benefit

With threshold (30%):
  Only route when savings are significant
  Avoids routing for small inputs or marginal locality differences
  Reduces unnecessary inter-node traffic
```

### 10.4 Why Load-Aware Routing?

Without load awareness, the node with the best cache locality receives
ALL routed requests → becomes a hotspot → degrades performance.

Load-aware routing distributes computation across nodes:
- Lightly loaded nodes get more routed work
- Heavily loaded nodes are avoided even with good locality
- Prevents cascading overload

### 10.5 Why Fallback to Local Instead of Retry Another Remote?

```
Remote failure → try another remote:
  + Might find a better node
  - Adds latency (another RPC attempt)
  - May also fail (cascading failures)
  - Complex retry logic

Remote failure → compute locally:
  + Always works (inputs can be fetched)
  + Simple, predictable
  + No cascading failure risk
  - May be slower than optimal remote

Local fallback is simpler and always correct. The routing optimization
is best-effort — if it fails, the system degrades to pre-routing behavior.

### 10.6 Comparison with Other Systems

```
┌──────────────────┬──────────────────────────────────────────┐
│ System           │ Compute Routing Approach                  │
├──────────────────┼──────────────────────────────────────────┤
│ Spark            │ Data locality scheduling (prefer nodes    │
│                  │ with HDFS blocks, rack-aware fallback)    │
├──────────────────┼──────────────────────────────────────────┤
│ Dask             │ Worker stealing + data-aware scheduling   │
│                  │ (scheduler tracks data locations)         │
├──────────────────┼──────────────────────────────────────────┤
│ Ray              │ Object store locality hints               │
│                  │ (schedule on node with most objects)      │
├──────────────────┼──────────────────────────────────────────┤
│ Deriva           │ Bloom filter gossip + cost model          │
│                  │ (decentralized, no scheduler, 1-hop max)  │
└──────────────────┴──────────────────────────────────────────┘

Deriva's approach is unique in being fully decentralized:
  - No central scheduler (unlike Spark, Dask)
  - No object store directory (unlike Ray)
  - Routing decisions made locally using gossip metadata
  - Trade-off: slightly less optimal than centralized scheduling,
    but no single point of failure and zero coordination overhead
```
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `routing_decision_total` | Counter | `result={local,remote,fallback}` | Routing outcomes |
| `routing_savings_pct` | Histogram | — | Estimated savings when routing |
| `routing_decision_latency_us` | Histogram | — | Decision computation time |
| `routed_compute_total` | Counter | `result={success,failure}` | Remote compute outcomes |
| `routed_compute_latency_ms` | Histogram | — | End-to-end routed compute time |
| `routing_fallback_total` | Counter | `reason={timeout,error,crash}` | Fallback triggers |
| `routing_hop_count` | Histogram | — | Actual hops per request |
| `routing_input_bytes_saved` | Counter | — | Estimated bytes not transferred |

### 11.2 Log Events

```rust
tracing::info!(
    addr = %addr,
    target = %target,
    savings_pct = savings,
    hop_count = hop_count,
    input_count = recipe.inputs.len(),
    "routing computation to remote node"
);

tracing::warn!(
    addr = %addr,
    target = %target,
    error = %e,
    "remote compute failed, falling back to local"
);

tracing::debug!(
    addr = %addr,
    decision = "local",
    reason = %reason, // "all_local", "tiny_inputs", "max_hops", "no_savings"
    "routing decision: compute locally"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| High routing fallback rate | `routing_fallback_total` > 20% of routed | Warning |
| Routing decision slow | p99 `routing_decision_latency_us` > 100μs | Warning |
| Remote compute failures | `routed_compute_total{result=failure}` > 10/min | Warning |
| No routing happening | `routing_decision_total{result=remote}` = 0 for 1h | Info |

---

## 12. Checklist

- [ ] Create `deriva-network/src/routing.rs`
- [ ] Implement `ComputeRouter` with scoring and decision logic
- [ ] Implement `load_factor` CPU penalty
- [ ] Implement `estimate_input_sizes` helper
- [ ] Add `RouteCompute` RPC to proto
- [ ] Implement `route_compute` internal RPC handler
- [ ] Implement `materialize_routed` in executor
- [ ] Implement `forward_compute` with streaming response
- [ ] Add hop_count enforcement (anti-circular)
- [ ] Add fallback-to-local on remote failure
- [ ] Add routing metrics (8 metrics)
- [ ] Add structured log events
- [ ] Configure alerts
- [ ] Write load_factor unit tests (~4 tests)
- [ ] Write routing decision unit tests (~6 tests)
- [ ] Write config + input size tests (~3 tests)
- [ ] Write integration tests (~3 tests)
- [ ] Run benchmarks: decision latency, end-to-end, overhead
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
