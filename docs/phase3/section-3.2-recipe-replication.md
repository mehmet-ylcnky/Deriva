# §3.2 Recipe Replication

> **Status**: Blueprint  
> **Depends on**: §3.1 SWIM Gossip  
> **Crate(s)**: `deriva-network`, `deriva-core`, `deriva-server`  
> **Estimated effort**: 3–4 days  

---

## 1. Problem Statement

### 1.1 Current Limitation

Recipes exist only on the node where `put_recipe` was called. If that node
crashes, all recipes are lost. Other nodes cannot resolve or materialize
any recipe they don't have locally.

### 1.2 Why Replicate ALL Recipes to ALL Nodes?

Recipes are tiny (function name + version + input addrs + params ≈ 200–500 bytes).
Even 100,000 recipes = ~50MB — trivially fits in memory on every node.

| Strategy | Consistency | Lookup latency | Complexity |
|----------|------------|---------------|------------|
| No replication | None | Local only | None |
| Quorum-based | Tunable | 1 RTT (quorum read) | High |
| All-node replication | Strong | Local (0 RTT) | Moderate |
| Gossip-based eventual | Eventual | Local after convergence | Low |

All-node replication is chosen because:
1. Recipes are small — replication cost is negligible
2. Every node needs recipes for materialization — local lookup is critical
3. Strong consistency prevents "recipe not found" errors during routing
4. Simplifies distributed `get` protocol (§3.6) — any node can resolve

### 1.3 Replication Guarantee

```
put_recipe(f, [A, B]) → addr_X

  Guarantee: After put_recipe returns success, addr_X is stored
  on ALL alive nodes in the cluster.

  If any node fails to acknowledge:
    - Retry up to 3 times
    - If still failing: return error to client
    - Client can retry or accept partial replication

  Consistency model: synchronous all-node replication
  (strongest possible for recipes)
```

---

## 2. Design

### 2.1 Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                Recipe Replication                         │
│                                                           │
│  Client                                                   │
│    │ put_recipe(f, [A,B])                                │
│    ▼                                                      │
│  ┌──────────┐                                            │
│  │ Node A   │  1. Insert locally                         │
│  │ (coord.) │  2. Replicate to all peers                 │
│  └────┬─────┘                                            │
│       │                                                   │
│       ├──── ReplicateRecipe(X) ────▶ Node B  ──▶ ACK     │
│       ├──── ReplicateRecipe(X) ────▶ Node C  ──▶ ACK     │
│       └──── ReplicateRecipe(X) ────▶ Node D  ──▶ ACK     │
│                                                           │
│  3. All ACKs received → return success to client          │
│                                                           │
│  Anti-entropy: periodic full sync for consistency         │
│  ┌──────┐  SyncRecipes  ┌──────┐                         │
│  │Node A│───────────────▶│Node B│                         │
│  │      │◀───────────────│      │                         │
│  └──────┘  MissingRecipes└──────┘                         │
└─────────────────────────────────────────────────────────┘
```

### 2.2 Replication Protocol

```
Phase 1: Synchronous replication on put_recipe

  1. Coordinator inserts recipe into local DagStore
  2. Coordinator sends ReplicateRecipe RPC to all alive peers
  3. Each peer inserts into their local DagStore, returns ACK
  4. Coordinator waits for ALL ACKs (with timeout)
  5. If all ACK → return success
  6. If any NACK/timeout → retry up to 3 times
  7. If still failing → return error (partial replication)

Phase 2: Anti-entropy (background)

  Every 30 seconds, each node:
  1. Compute recipe fingerprint: hash of sorted recipe addrs
  2. Exchange fingerprints with a random peer
  3. If fingerprints differ:
     a. Exchange full recipe addr lists
     b. Identify missing recipes on each side
     c. Transfer missing recipes
  4. Convergence: O(log N) rounds for full consistency
```

### 2.3 Wire Protocol

```protobuf
service DerivaInternal {
    // Node-to-node recipe replication
    rpc ReplicateRecipe(ReplicateRecipeRequest)
        returns (ReplicateRecipeResponse);

    // Anti-entropy: exchange recipe fingerprints
    rpc SyncRecipes(SyncRecipesRequest)
        returns (SyncRecipesResponse);

    // Fetch specific recipes by addr
    rpc FetchRecipes(FetchRecipesRequest)
        returns (FetchRecipesResponse);
}

message ReplicateRecipeRequest {
    bytes addr = 1;
    string function_name = 2;
    string function_version = 3;
    repeated bytes inputs = 4;
    map<string, string> params = 5;
}

message ReplicateRecipeResponse {
    bool success = 1;
    string error = 2;
}

message SyncRecipesRequest {
    bytes fingerprint = 1;  // SHA-256 of sorted recipe addrs
    uint64 recipe_count = 2;
}

message SyncRecipesResponse {
    bool in_sync = 1;
    bytes fingerprint = 2;
    repeated bytes recipe_addrs = 3;  // all recipe addrs (if not in sync)
}

message FetchRecipesRequest {
    repeated bytes addrs = 1;
}

message FetchRecipesResponse {
    repeated RecipeData recipes = 1;
}

message RecipeData {
    bytes addr = 1;
    string function_name = 2;
    string function_version = 3;
    repeated bytes inputs = 4;
    map<string, string> params = 5;
}
```

---

## 3. Implementation

### 3.1 RecipeReplicator

Location: `crates/deriva-network/src/replication.rs`

```rust
use std::time::Duration;
use tonic::transport::Channel;
use deriva_core::{CAddr, Recipe, DerivaError};
use crate::types::NodeId;

/// Configuration for recipe replication.
#[derive(Debug, Clone)]
pub struct ReplicationConfig {
    /// Timeout for each replication RPC.
    pub rpc_timeout: Duration,
    /// Max retries per peer.
    pub max_retries: u32,
    /// Anti-entropy sync interval.
    pub sync_interval: Duration,
    /// Whether to require all-node ACK (true) or majority (false).
    pub require_all: bool,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            rpc_timeout: Duration::from_secs(5),
            max_retries: 3,
            sync_interval: Duration::from_secs(30),
            require_all: true,
        }
    }
}

/// Result of a replication attempt.
#[derive(Debug)]
pub struct ReplicationResult {
    pub success_count: usize,
    pub failure_count: usize,
    pub total_peers: usize,
    pub failures: Vec<(NodeId, String)>,
}

impl ReplicationResult {
    pub fn is_fully_replicated(&self) -> bool {
        self.failure_count == 0
    }
}

/// Handles recipe replication to cluster peers.
pub struct RecipeReplicator {
    config: ReplicationConfig,
}

impl RecipeReplicator {
    pub fn new(config: ReplicationConfig) -> Self {
        Self { config }
    }

    /// Replicate a recipe to all alive peers.
    ///
    /// Sends ReplicateRecipe RPC to each peer concurrently.
    /// Retries failed peers up to max_retries times.
    pub async fn replicate_to_all(
        &self,
        recipe: &Recipe,
        addr: &CAddr,
        peers: &[(NodeId, std::net::SocketAddr)], // (id, grpc_addr)
    ) -> ReplicationResult {
        use tokio::task::JoinSet;

        let mut join_set = JoinSet::new();
        let timeout = self.config.rpc_timeout;
        let max_retries = self.config.max_retries;

        for (peer_id, grpc_addr) in peers {
            let peer_id = peer_id.clone();
            let grpc_addr = *grpc_addr;
            let addr = *addr;
            let recipe = recipe.clone();

            join_set.spawn(async move {
                let mut last_err = String::new();
                for attempt in 0..=max_retries {
                    match Self::send_replicate(
                        &grpc_addr, &addr, &recipe, timeout,
                    ).await {
                        Ok(()) => return (peer_id, Ok(())),
                        Err(e) => {
                            last_err = e.to_string();
                            if attempt < max_retries {
                                tokio::time::sleep(
                                    Duration::from_millis(100 * (attempt as u64 + 1))
                                ).await;
                            }
                        }
                    }
                }
                (peer_id, Err(last_err))
            });
        }

        let mut success_count = 0;
        let mut failures = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((peer_id, Ok(()))) => success_count += 1,
                Ok((peer_id, Err(e))) => failures.push((peer_id, e)),
                Err(e) => failures.push((
                    NodeId::new("0.0.0.0:0".parse().unwrap(), "unknown"),
                    e.to_string(),
                )),
            }
        }

        ReplicationResult {
            success_count,
            failure_count: failures.len(),
            total_peers: peers.len(),
            failures,
        }
    }

    /// Send a single ReplicateRecipe RPC to a peer.
    async fn send_replicate(
        grpc_addr: &std::net::SocketAddr,
        addr: &CAddr,
        recipe: &Recipe,
        timeout: Duration,
    ) -> Result<(), DerivaError> {
        let endpoint = format!("http://{}", grpc_addr);
        let channel = Channel::from_shared(endpoint)
            .map_err(|e| DerivaError::Network(e.to_string()))?
            .connect_timeout(timeout)
            .connect()
            .await
            .map_err(|e| DerivaError::Network(e.to_string()))?;

        let mut client = DerivaInternalClient::new(channel);

        let request = tonic::Request::new(ReplicateRecipeRequest {
            addr: addr.as_bytes().to_vec(),
            function_name: recipe.function_name.clone(),
            function_version: recipe.function_version.clone(),
            inputs: recipe.inputs.iter()
                .map(|a| a.as_bytes().to_vec())
                .collect(),
            params: recipe.params.clone(),
        });

        let response = tokio::time::timeout(timeout, client.replicate_recipe(request))
            .await
            .map_err(|_| DerivaError::Network("replication timeout".into()))?
            .map_err(|e| DerivaError::Network(e.to_string()))?;

        if response.get_ref().success {
            Ok(())
        } else {
            Err(DerivaError::Network(response.get_ref().error.clone()))
        }
    }
}
```

### 3.2 Anti-Entropy Sync

Location: `crates/deriva-network/src/replication.rs`

```rust
use sha2::{Sha256, Digest};

/// Compute a fingerprint of all recipes in the DAG.
///
/// Fingerprint = SHA-256 of sorted recipe addrs.
/// Two nodes with identical recipe sets have identical fingerprints.
pub fn compute_recipe_fingerprint(dag: &DagStore) -> Vec<u8> {
    let mut addrs: Vec<CAddr> = dag.iter_recipes()
        .map(|(addr, _)| *addr)
        .collect();
    addrs.sort();

    let mut hasher = Sha256::new();
    for addr in &addrs {
        hasher.update(addr.as_bytes());
    }
    hasher.finalize().to_vec()
}

/// Identify recipes that exist on remote but not locally.
pub fn find_missing_recipes(
    local_addrs: &[CAddr],
    remote_addrs: &[CAddr],
) -> Vec<CAddr> {
    let local_set: std::collections::HashSet<CAddr> =
        local_addrs.iter().copied().collect();
    remote_addrs.iter()
        .filter(|addr| !local_set.contains(addr))
        .copied()
        .collect()
}

/// Anti-entropy sync loop.
///
/// Periodically picks a random peer and synchronizes recipe sets.
pub async fn anti_entropy_loop(
    dag: Arc<RwLock<DagStore>>,
    swim: Arc<SwimRuntime>,
    config: ReplicationConfig,
) {
    let mut interval = tokio::time::interval(config.sync_interval);
    loop {
        interval.tick().await;

        // Pick random alive peer
        let peers = swim.members().await;
        let alive_peers: Vec<_> = peers.iter()
            .filter(|(id, state)| {
                *state == MemberState::Alive && *id != *swim.local_id()
            })
            .collect();

        if alive_peers.is_empty() { continue; }

        use rand::Rng;
        let idx = rand::thread_rng().gen_range(0..alive_peers.len());
        let (peer_id, _) = &alive_peers[idx];

        let peer_meta = swim.get_metadata(peer_id).await;
        let grpc_addr = match peer_meta {
            Some(m) => m.grpc_addr,
            None => continue,
        };

        // Compute local fingerprint
        let local_fp = {
            let dag_guard = dag.read().await;
            compute_recipe_fingerprint(&dag_guard)
        };

        // Exchange fingerprints with peer
        let endpoint = format!("http://{}", grpc_addr);
        let channel = match Channel::from_shared(endpoint)
            .unwrap().connect().await
        {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut client = DerivaInternalClient::new(channel);

        let sync_resp = match client.sync_recipes(SyncRecipesRequest {
            fingerprint: local_fp.clone(),
            recipe_count: dag.read().await.recipe_count() as u64,
        }).await {
            Ok(r) => r.into_inner(),
            Err(_) => continue,
        };

        if sync_resp.in_sync { continue; } // already synchronized

        // Find missing recipes
        let local_addrs: Vec<CAddr> = {
            let dag_guard = dag.read().await;
            dag_guard.iter_recipes().map(|(a, _)| *a).collect()
        };
        let remote_addrs: Vec<CAddr> = sync_resp.recipe_addrs.iter()
            .filter_map(|bytes| CAddr::try_from(bytes.as_slice()).ok())
            .collect();

        let missing = find_missing_recipes(&local_addrs, &remote_addrs);
        if missing.is_empty() { continue; }

        // Fetch missing recipes
        let fetch_resp = match client.fetch_recipes(FetchRecipesRequest {
            addrs: missing.iter().map(|a| a.as_bytes().to_vec()).collect(),
        }).await {
            Ok(r) => r.into_inner(),
            Err(_) => continue,
        };

        // Insert missing recipes locally
        let mut dag_guard = dag.write().await;
        for recipe_data in &fetch_resp.recipes {
            if let Ok(addr) = CAddr::try_from(recipe_data.addr.as_slice()) {
                let recipe = Recipe {
                    function_name: recipe_data.function_name.clone(),
                    function_version: recipe_data.function_version.clone(),
                    inputs: recipe_data.inputs.iter()
                        .filter_map(|b| CAddr::try_from(b.as_slice()).ok())
                        .collect(),
                    params: recipe_data.params.clone(),
                };
                let _ = dag_guard.insert(recipe);
            }
        }
    }
}
```

### 3.3 Updated put_recipe Handler

Location: `crates/deriva-server/src/service.rs`

```rust
async fn put_recipe(
    &self,
    request: Request<PutRecipeRequest>,
) -> Result<Response<PutRecipeResponse>, Status> {
    let req = request.get_ref();

    // Build recipe
    let inputs: Vec<CAddr> = req.inputs.iter()
        .map(|b| parse_addr(b))
        .collect::<Result<Vec<_>, _>>()?;
    let recipe = Recipe {
        function_name: req.function_name.clone(),
        function_version: req.function_version.clone(),
        inputs,
        params: req.params.clone(),
    };
    let addr = recipe.compute_addr();

    // Insert locally
    {
        let mut dag = self.state.dag.write()
            .map_err(|_| Status::internal("dag lock"))?;
        dag.insert(recipe.clone())
            .map_err(|e| Status::internal(e.to_string()))?;
    }

    // Replicate to all peers (if in cluster mode)
    if let Some(ref swim) = self.state.swim {
        let peers = self.get_alive_peer_grpc_addrs(swim).await;
        if !peers.is_empty() {
            let replicator = &self.state.replicator;
            let result = replicator.replicate_to_all(
                &recipe, &addr, &peers,
            ).await;

            if !result.is_fully_replicated()
                && self.state.replication_config.require_all
            {
                return Err(Status::unavailable(format!(
                    "replication failed: {}/{} peers",
                    result.failure_count, result.total_peers
                )));
            }
        }
    }

    Ok(Response::new(PutRecipeResponse {
        addr: addr.as_bytes().to_vec(),
    }))
}
```

### 3.4 Internal RPC Handlers

```rust
/// Handle incoming recipe replication from a peer.
async fn replicate_recipe(
    &self,
    request: Request<ReplicateRecipeRequest>,
) -> Result<Response<ReplicateRecipeResponse>, Status> {
    let req = request.get_ref();
    let addr = parse_addr(&req.addr)?;
    let inputs: Vec<CAddr> = req.inputs.iter()
        .map(|b| parse_addr(b))
        .collect::<Result<Vec<_>, _>>()?;

    let recipe = Recipe {
        function_name: req.function_name.clone(),
        function_version: req.function_version.clone(),
        inputs,
        params: req.params.clone(),
    };

    let mut dag = self.state.dag.write()
        .map_err(|_| Status::internal("dag lock"))?;

    match dag.insert(recipe) {
        Ok(_) | Err(DerivaError::AlreadyExists(_)) => {
            Ok(Response::new(ReplicateRecipeResponse {
                success: true,
                error: String::new(),
            }))
        }
        Err(e) => {
            Ok(Response::new(ReplicateRecipeResponse {
                success: false,
                error: e.to_string(),
            }))
        }
    }
}

/// Handle anti-entropy sync request.
async fn sync_recipes(
    &self,
    request: Request<SyncRecipesRequest>,
) -> Result<Response<SyncRecipesResponse>, Status> {
    let req = request.get_ref();
    let dag = self.state.dag.read()
        .map_err(|_| Status::internal("dag lock"))?;

    let local_fp = compute_recipe_fingerprint(&dag);
    let in_sync = local_fp == req.fingerprint;

    let recipe_addrs = if in_sync {
        vec![]
    } else {
        dag.iter_recipes()
            .map(|(addr, _)| addr.as_bytes().to_vec())
            .collect()
    };

    Ok(Response::new(SyncRecipesResponse {
        in_sync,
        fingerprint: local_fp,
        recipe_addrs,
    }))
}

/// Handle fetch recipes request.
async fn fetch_recipes(
    &self,
    request: Request<FetchRecipesRequest>,
) -> Result<Response<FetchRecipesResponse>, Status> {
    let req = request.get_ref();
    let dag = self.state.dag.read()
        .map_err(|_| Status::internal("dag lock"))?;

    let recipes: Vec<RecipeData> = req.addrs.iter()
        .filter_map(|bytes| {
            let addr = CAddr::try_from(bytes.as_slice()).ok()?;
            let recipe = dag.get_recipe(&addr)?;
            Some(RecipeData {
                addr: bytes.clone(),
                function_name: recipe.function_name.clone(),
                function_version: recipe.function_version.clone(),
                inputs: recipe.inputs.iter()
                    .map(|a| a.as_bytes().to_vec())
                    .collect(),
                params: recipe.params.clone(),
            })
        })
        .collect();

    Ok(Response::new(FetchRecipesResponse { recipes }))
}
```


---

## 4. Data Flow Diagrams

### 4.1 Synchronous Replication — Happy Path

```
  Client          Node A (coord)      Node B           Node C
    │                  │                 │                │
    │ put_recipe(f,[X])│                 │                │
    │─────────────────▶│                 │                │
    │                  │ insert locally  │                │
    │                  │                 │                │
    │                  │ ReplicateRecipe │                │
    │                  │────────────────▶│                │
    │                  │ ReplicateRecipe │                │
    │                  │─────────────────────────────────▶│
    │                  │                 │                │
    │                  │           ACK   │                │
    │                  │◀────────────────│                │
    │                  │                          ACK    │
    │                  │◀─────────────────────────────────│
    │                  │                 │                │
    │  PutRecipeResp   │  all ACKs ✓    │                │
    │◀─────────────────│                 │                │
    │                  │                 │                │
    │  Recipe on ALL 3 nodes             │                │
```

### 4.2 Replication with Retry

```
  Node A (coord)      Node B           Node C (slow)
    │                   │                │
    │ ReplicateRecipe   │                │
    │──────────────────▶│  ACK           │
    │◀──────────────────│                │
    │                   │                │
    │ ReplicateRecipe   │                │
    │───────────────────────────────────▶│ (timeout)
    │                   │                │
    │ Retry #1 (100ms delay)             │
    │───────────────────────────────────▶│ (timeout)
    │                   │                │
    │ Retry #2 (200ms delay)             │
    │───────────────────────────────────▶│
    │                   │           ACK  │
    │◀───────────────────────────────────│
    │                   │                │
    │ All ACKs ✓        │                │
```

### 4.3 Anti-Entropy Sync

```
  Node A                    Node B
    │                         │
    │  SyncRecipes            │
    │  fp=0xabc, count=100    │
    │────────────────────────▶│
    │                         │  local fp=0xdef (different!)
    │  SyncRecipesResponse    │
    │  in_sync=false          │
    │  recipe_addrs=[...]     │
    │◀────────────────────────│
    │                         │
    │  Compute missing:       │
    │  A has [1..100]         │
    │  B has [1..95, 101..105]│
    │  A missing: [101..105]  │
    │                         │
    │  FetchRecipes([101..105])│
    │────────────────────────▶│
    │  FetchRecipesResponse   │
    │  recipes=[101..105]     │
    │◀────────────────────────│
    │                         │
    │  Insert 101..105 locally│
    │  Now A has [1..105]     │
    │                         │
    │  Next sync: B fetches   │
    │  [96..100] from A       │
```

### 4.4 New Node Joins — Full Sync

```
  Node A (existing)     Node D (new, empty DAG)
    │                         │
    │  D joins cluster via SWIM│
    │                         │
    │  Anti-entropy triggers: │
    │  SyncRecipes            │
    │  fp=0x000, count=0      │
    │◀────────────────────────│
    │                         │
    │  SyncRecipesResponse    │
    │  in_sync=false          │
    │  recipe_addrs=[1..500]  │
    │────────────────────────▶│
    │                         │
    │  FetchRecipes([1..500]) │
    │◀────────────────────────│
    │  FetchRecipesResponse   │
    │  recipes=[1..500]       │
    │────────────────────────▶│
    │                         │
    │  D now has all 500 recipes
    │  Full sync in 1 round   │
```

---

## 5. Test Specification

### 5.1 Unit Tests — Fingerprint

```rust
#[test]
fn test_fingerprint_deterministic() {
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    insert_recipe(&mut dag, "f", vec![a]);
    insert_recipe(&mut dag, "g", vec![a]);

    let fp1 = compute_recipe_fingerprint(&dag);
    let fp2 = compute_recipe_fingerprint(&dag);
    assert_eq!(fp1, fp2);
}

#[test]
fn test_fingerprint_changes_on_insert() {
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    insert_recipe(&mut dag, "f", vec![a]);
    let fp1 = compute_recipe_fingerprint(&dag);

    insert_recipe(&mut dag, "g", vec![a]);
    let fp2 = compute_recipe_fingerprint(&dag);
    assert_ne!(fp1, fp2);
}

#[test]
fn test_fingerprint_same_for_same_recipes() {
    let a = leaf_addr("a");

    let mut dag1 = DagStore::new();
    insert_recipe(&mut dag1, "f", vec![a]);
    insert_recipe(&mut dag1, "g", vec![a]);

    let mut dag2 = DagStore::new();
    // Insert in different order
    insert_recipe(&mut dag2, "g", vec![a]);
    insert_recipe(&mut dag2, "f", vec![a]);

    // Fingerprints should match (sorted)
    assert_eq!(
        compute_recipe_fingerprint(&dag1),
        compute_recipe_fingerprint(&dag2),
    );
}
```

### 5.2 Unit Tests — find_missing_recipes

```rust
#[test]
fn test_find_missing_none() {
    let local = vec![addr("a"), addr("b"), addr("c")];
    let remote = vec![addr("a"), addr("b")];
    let missing = find_missing_recipes(&local, &remote);
    assert!(missing.is_empty());
}

#[test]
fn test_find_missing_some() {
    let local = vec![addr("a"), addr("b")];
    let remote = vec![addr("a"), addr("b"), addr("c"), addr("d")];
    let missing = find_missing_recipes(&local, &remote);
    assert_eq!(missing.len(), 2);
    assert!(missing.contains(&addr("c")));
    assert!(missing.contains(&addr("d")));
}

#[test]
fn test_find_missing_all() {
    let local = vec![];
    let remote = vec![addr("a"), addr("b")];
    let missing = find_missing_recipes(&local, &remote);
    assert_eq!(missing.len(), 2);
}
```

### 5.3 Unit Tests — ReplicationResult

```rust
#[test]
fn test_replication_result_fully_replicated() {
    let result = ReplicationResult {
        success_count: 3,
        failure_count: 0,
        total_peers: 3,
        failures: vec![],
    };
    assert!(result.is_fully_replicated());
}

#[test]
fn test_replication_result_partial() {
    let result = ReplicationResult {
        success_count: 2,
        failure_count: 1,
        total_peers: 3,
        failures: vec![(test_node_id("c"), "timeout".into())],
    };
    assert!(!result.is_fully_replicated());
}
```

### 5.4 Integration Tests

```rust
#[tokio::test]
async fn test_recipe_replicated_to_all_nodes() {
    let (mut client_a, _server_a) = start_cluster_node(0).await;
    let (_client_b, _server_b) = start_cluster_node(1).await;
    let (mut client_c, _server_c) = start_cluster_node(2).await;

    // Wait for cluster formation
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Put recipe on node A
    let leaf = put_leaf(&mut client_a, b"data").await;
    let recipe_addr = put_recipe(
        &mut client_a, "identity", "v1", vec![leaf],
    ).await;

    // Verify recipe exists on node C
    let resolve = client_c.resolve(ResolveRequest {
        addr: recipe_addr.clone(),
    }).await.unwrap().into_inner();

    assert!(resolve.found);
    assert_eq!(resolve.function_name, "identity");
}

#[tokio::test]
async fn test_anti_entropy_recovers_missing_recipe() {
    let (mut client_a, _server_a) = start_cluster_node(0).await;
    let (mut client_b, server_b) = start_cluster_node(1).await;

    tokio::time::sleep(Duration::from_secs(2)).await;

    // Put recipe on A (replicates to B)
    let leaf = put_leaf(&mut client_a, b"data").await;
    let recipe_addr = put_recipe(
        &mut client_a, "identity", "v1", vec![leaf],
    ).await;

    // Simulate B losing the recipe (restart with empty DAG)
    // ... restart node B ...

    // Wait for anti-entropy sync
    tokio::time::sleep(Duration::from_secs(35)).await;

    // B should have the recipe again
    let resolve = client_b.resolve(ResolveRequest {
        addr: recipe_addr,
    }).await.unwrap().into_inner();
    assert!(resolve.found);
}

#[tokio::test]
async fn test_put_recipe_survives_single_node_failure() {
    let (mut client_a, _) = start_cluster_node(0).await;
    let (_, _server_b) = start_cluster_node(1).await;
    let (mut client_c, _) = start_cluster_node(2).await;

    tokio::time::sleep(Duration::from_secs(2)).await;

    let leaf = put_leaf(&mut client_a, b"data").await;
    let recipe_addr = put_recipe(
        &mut client_a, "identity", "v1", vec![leaf],
    ).await;

    // Kill node B
    drop(_server_b);
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Recipe still available on C
    let resolve = client_c.resolve(ResolveRequest {
        addr: recipe_addr,
    }).await.unwrap().into_inner();
    assert!(resolve.found);
}
```

### 5.5 Unit Tests — ReplicationConfig Validation

```rust
#[test]
fn test_default_config_values() {
    let config = ReplicationConfig::default();
    assert_eq!(config.rpc_timeout, Duration::from_secs(5));
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.sync_interval, Duration::from_secs(30));
    assert!(config.require_all);
}

#[test]
fn test_custom_config() {
    let config = ReplicationConfig {
        rpc_timeout: Duration::from_secs(10),
        max_retries: 5,
        sync_interval: Duration::from_secs(60),
        require_all: false,
    };
    assert_eq!(config.max_retries, 5);
    assert!(!config.require_all);
}
```

### 5.6 Unit Tests — Idempotent Replication

```rust
#[test]
fn test_replicate_same_recipe_twice_is_idempotent() {
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    let recipe = Recipe {
        function_name: "f".into(),
        function_version: "v1".into(),
        inputs: vec![a],
        params: Default::default(),
    };
    let addr1 = dag.insert(recipe.clone()).unwrap();

    // Second insert of identical recipe should not error
    // (content-addressed: same recipe = same addr)
    let addr2 = dag.insert(recipe.clone());
    match addr2 {
        Ok(a) => assert_eq!(a, addr1),
        Err(DerivaError::AlreadyExists(_)) => {} // also acceptable
        Err(e) => panic!("unexpected error: {}", e),
    }
}

#[test]
fn test_replicate_handler_accepts_duplicate() {
    // Simulates the replicate_recipe handler receiving a recipe
    // that already exists locally — should return success.
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    let recipe = Recipe {
        function_name: "f".into(),
        function_version: "v1".into(),
        inputs: vec![a],
        params: Default::default(),
    };
    let _ = dag.insert(recipe.clone()).unwrap();

    // Handler logic: insert returns AlreadyExists → still success
    let result = dag.insert(recipe);
    let success = match result {
        Ok(_) => true,
        Err(DerivaError::AlreadyExists(_)) => true,
        Err(_) => false,
    };
    assert!(success);
}
```

### 5.7 Unit Tests — Fingerprint Edge Cases

```rust
#[test]
fn test_fingerprint_empty_dag() {
    let dag = DagStore::new();
    let fp = compute_recipe_fingerprint(&dag);
    // SHA-256 of empty input
    assert_eq!(fp.len(), 32);
}

#[test]
fn test_fingerprint_single_recipe() {
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    insert_recipe(&mut dag, "f", vec![a]);
    let fp = compute_recipe_fingerprint(&dag);
    assert_eq!(fp.len(), 32);
}

#[test]
fn test_fingerprint_1000_recipes() {
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    for i in 0..1000 {
        insert_recipe(&mut dag, &format!("f_{}", i), vec![a]);
    }
    let fp = compute_recipe_fingerprint(&dag);
    assert_eq!(fp.len(), 32);

    // Fingerprint is stable across calls
    let fp2 = compute_recipe_fingerprint(&dag);
    assert_eq!(fp, fp2);
}
```

### 5.8 Integration Tests — Concurrent Replication

```rust
#[tokio::test]
async fn test_concurrent_put_recipe_from_different_nodes() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (mut client_b, _sb) = start_cluster_node(1).await;
    let (mut client_c, _sc) = start_cluster_node(2).await;

    tokio::time::sleep(Duration::from_secs(3)).await;

    // Put different recipes concurrently from A and B
    let leaf_a = put_leaf(&mut client_a, b"data_a").await;
    let leaf_b = put_leaf(&mut client_b, b"data_b").await;

    let (addr_a, addr_b) = tokio::join!(
        put_recipe(&mut client_a, "func_a", "v1", vec![leaf_a]),
        put_recipe(&mut client_b, "func_b", "v1", vec![leaf_b]),
    );

    // Both recipes should exist on node C
    tokio::time::sleep(Duration::from_millis(500)).await;

    let resolve_a = client_c.resolve(ResolveRequest {
        addr: addr_a.clone(),
    }).await.unwrap().into_inner();
    assert!(resolve_a.found);

    let resolve_b = client_c.resolve(ResolveRequest {
        addr: addr_b.clone(),
    }).await.unwrap().into_inner();
    assert!(resolve_b.found);
}

#[tokio::test]
async fn test_replication_to_5_node_cluster() {
    let mut nodes = Vec::new();
    for i in 0..5 {
        nodes.push(start_cluster_node(i).await);
    }
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Put recipe on node 0
    let leaf = put_leaf(&mut nodes[0].0, b"data").await;
    let recipe_addr = put_recipe(
        &mut nodes[0].0, "identity", "v1", vec![leaf],
    ).await;

    // Verify on all other nodes
    for i in 1..5 {
        let resolve = nodes[i].0.resolve(ResolveRequest {
            addr: recipe_addr.clone(),
        }).await.unwrap().into_inner();
        assert!(resolve.found, "recipe missing on node {}", i);
    }
}

#[tokio::test]
async fn test_new_node_gets_recipes_via_anti_entropy() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (_, _sb) = start_cluster_node(1).await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Insert 50 recipes on node A
    let leaf = put_leaf(&mut client_a, b"data").await;
    for i in 0..50 {
        put_recipe(
            &mut client_a,
            &format!("func_{}", i), "v1", vec![leaf],
        ).await;
    }

    // Start node C (joins late)
    let (mut client_c, _sc) = start_cluster_node(2).await;

    // Wait for anti-entropy to sync (30s interval + buffer)
    tokio::time::sleep(Duration::from_secs(40)).await;

    // Node C should have all 50 recipes
    let status = client_c.status(StatusRequest {}).await
        .unwrap().into_inner();
    assert!(status.recipe_count >= 50,
        "expected >=50 recipes, got {}", status.recipe_count);
}
```

### 5.9 Integration Tests — Failure Scenarios

```rust
#[tokio::test]
async fn test_put_recipe_fails_when_all_peers_down() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (_, server_b) = start_cluster_node(1).await;
    let (_, server_c) = start_cluster_node(2).await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Kill both peers
    drop(server_b);
    drop(server_c);
    tokio::time::sleep(Duration::from_secs(1)).await;

    // put_recipe should fail (require_all=true, no peers ACK)
    let leaf = put_leaf(&mut client_a, b"data").await;
    let result = client_a.put_recipe(PutRecipeRequest {
        function_name: "f".into(),
        function_version: "v1".into(),
        inputs: vec![leaf],
        params: Default::default(),
    }).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_put_recipe_succeeds_with_require_all_false() {
    // Start cluster with require_all=false config
    let config = ReplicationConfig {
        require_all: false,
        ..Default::default()
    };
    let (mut client_a, _sa) = start_cluster_node_with_config(0, config).await;
    let (_, server_b) = start_cluster_node(1).await;
    let (_, server_c) = start_cluster_node(2).await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Kill one peer
    drop(server_c);
    tokio::time::sleep(Duration::from_secs(1)).await;

    // put_recipe should succeed (only 1 of 2 peers failed)
    let leaf = put_leaf(&mut client_a, b"data").await;
    let result = client_a.put_recipe(PutRecipeRequest {
        function_name: "f".into(),
        function_version: "v1".into(),
        inputs: vec![leaf],
        params: Default::default(),
    }).await;

    assert!(result.is_ok());
}
```

---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Single-node cluster | No replication, local insert only | No peers to replicate to |
| 2 | Peer joins during replication | New peer gets recipe via anti-entropy | Sync catches up within 30s |
| 3 | Peer dies during replication | Retry 3x, then error if require_all | Client can retry |
| 4 | Duplicate recipe (already exists) | Idempotent insert, return success | Content-addressed = same addr = same recipe |
| 5 | Very large recipe (many inputs) | Still small (~1KB max) | Recipes are metadata, not data |
| 6 | Network partition during put | Partial replication, error returned | Client retries after partition heals |
| 7 | Anti-entropy with 100K recipes | Full addr list ~3.2MB, fits in gRPC | Acceptable for infrequent sync |
| 8 | Concurrent put_recipe on different nodes | Both replicate, both succeed (idempotent) | Content-addressed dedup |
| 9 | Node restarts with empty DAG | Anti-entropy restores all recipes | Full sync in 1 round |
| 10 | gRPC channel fails mid-replication | Retry with backoff (100ms, 200ms, 300ms) | Exponential backoff prevents thundering herd |
| 11 | Fingerprint collision (SHA-256) | Probability ~2^-256, ignored | Astronomically unlikely |
| 12 | Recipe with 0 inputs | Valid recipe, replicated normally | Edge case in recipe construction |
| 13 | Anti-entropy during high write load | Sync may find stale diff, next round corrects | Convergence guaranteed over multiple rounds |
| 14 | Clock skew between nodes | No impact — no timestamps in protocol | Content-addressed, no ordering needed |

### 6.1 Error Recovery Matrix

```
┌─────────────────────────┬──────────────────────────────────┐
│ Failure Mode            │ Recovery Path                     │
├─────────────────────────┼──────────────────────────────────┤
│ 1 peer timeout          │ Retry 3x → error if require_all  │
│ All peers timeout       │ Error → client retries            │
│ Coordinator crash       │ Anti-entropy syncs from peers     │
│ Network partition       │ Anti-entropy after heal           │
│ Corrupted recipe data   │ CAddr verification on receive     │
│ gRPC max message size   │ Batch FetchRecipes (100 per RPC)  │
│ OOM during full sync    │ Paginated FetchRecipes            │
└─────────────────────────┴──────────────────────────────────┘
```

### 6.2 CAddr Verification on Receive

When a node receives a replicated recipe, it should verify the CAddr:

```rust
/// Verify that the received recipe matches its claimed address.
fn verify_recipe_addr(claimed_addr: &CAddr, recipe: &Recipe) -> bool {
    let computed = recipe.compute_addr();
    computed == *claimed_addr
}
```

This prevents a malicious or buggy peer from injecting recipes with
incorrect addresses. If verification fails, the recipe is rejected
and the peer is flagged for investigation.

```
Receive ReplicateRecipe(addr=X, recipe=R):
  1. Compute R.compute_addr() → Y
  2. If X != Y → reject, log warning, increment bad_replication counter
  3. If X == Y → insert into DagStore
```

---

## 7. Performance Analysis

### 7.1 Latency Model

```
Replication latency (per put_recipe):
  Local insert:                ~1μs
  gRPC channel setup (cached): ~50μs
  Serialization (bincode):     ~5μs
  Network RTT (LAN):           ~0.5ms
  Peer insert:                 ~1μs
  Total (3 peers, parallel):   ~1ms end-to-end

  With retries (1 peer slow):
    Attempt 1: timeout at 5s
    Retry 1 (100ms backoff): +5.1s
    Retry 2 (200ms backoff): +5.3s
    Worst case: ~15.4s (3 retries, all timeout)
```

### 7.2 Bandwidth Model

```
Anti-entropy bandwidth per sync round:
  ┌──────────────────────────────────────────────────┐
  │ Recipe Count │ Fingerprint │ Addr List │ Fetch   │
  ├──────────────┼─────────────┼───────────┼─────────┤
  │ 1,000        │ 100 B       │ 32 KB     │ ~5 KB   │
  │ 10,000       │ 100 B       │ 320 KB    │ ~50 KB  │
  │ 100,000      │ 100 B       │ 3.2 MB    │ ~500 KB │
  │ 1,000,000    │ 100 B       │ 32 MB     │ ~5 MB   │
  └──────────────────────────────────────────────────┘

  Sync frequency: every 30s, 1 random peer
  Average bandwidth (10K recipes, in-sync): ~200 B/round
  Average bandwidth (10K recipes, 1% drift): ~3.5 KB/round
  Steady-state: <1 KB/s per node
```

### 7.3 Memory Footprint

```
Recipe storage per node:
  ┌──────────────────────────────────────────────────┐
  │ Recipe Count │ Avg Size │ Total Memory │ % of 8GB│
  ├──────────────┼──────────┼──────────────┼─────────┤
  │ 1,000        │ 300 B    │ 300 KB       │ 0.004%  │
  │ 10,000       │ 300 B    │ 3 MB         │ 0.037%  │
  │ 100,000      │ 300 B    │ 30 MB        │ 0.37%   │
  │ 1,000,000    │ 300 B    │ 300 MB       │ 3.7%    │
  └──────────────────────────────────────────────────┘

  Fingerprint computation (SHA-256):
    10K recipes: ~0.5ms
    100K recipes: ~5ms
    1M recipes: ~50ms
```

### 7.4 Benchmarking Plan

```rust
/// Benchmark: put_recipe replication latency vs cluster size
#[bench]
fn bench_replication_latency(b: &mut Bencher) {
    // Setup: 3-node, 5-node, 10-node clusters
    // Measure: time from put_recipe call to all ACKs received
    // Expected: <2ms for 3 nodes, <5ms for 10 nodes (LAN)
}

/// Benchmark: fingerprint computation vs recipe count
#[bench]
fn bench_fingerprint_computation(b: &mut Bencher) {
    // Setup: DagStore with 1K, 10K, 100K recipes
    // Measure: compute_recipe_fingerprint() duration
    // Expected: O(N) where N = recipe count
}

/// Benchmark: anti-entropy sync with varying drift
#[bench]
fn bench_anti_entropy_sync(b: &mut Bencher) {
    // Setup: two nodes, one with N extra recipes
    // Measure: time to full sync (fingerprint + fetch + insert)
    // Vary: N = 10, 100, 1000 missing recipes
}

/// Benchmark: concurrent put_recipe throughput
#[bench]
fn bench_concurrent_put_recipe(b: &mut Bencher) {
    // Setup: 3-node cluster
    // Measure: recipes/sec with 1, 4, 16 concurrent writers
    // Expected: >1000 recipes/sec (bottleneck is network RTT)
}
```

### 7.5 Scalability Limits

```
Maximum cluster size for all-node replication:
  put_recipe latency = max(peer_rtt) across all peers
  With 100 nodes: still ~1ms (parallel RPCs)
  Bandwidth per put: 100 × 300B = 30KB (negligible)

  Anti-entropy with 100 nodes:
    Each node syncs with 1 random peer every 30s
    Full cluster convergence: O(log 100) × 30s ≈ 3.5 minutes
    Acceptable for background repair

  Recommendation: all-node replication works up to ~500 nodes.
  Beyond that, consider hierarchical replication or gossip-based.
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/replication.rs` | **NEW** — `RecipeReplicator`, anti-entropy, fingerprint |
| `deriva-network/src/lib.rs` | Add `pub mod replication` |
| `proto/deriva_internal.proto` | **NEW** — internal node-to-node RPCs |
| `deriva-server/src/service.rs` | Update `put_recipe`, add internal RPC handlers |
| `deriva-server/src/internal.rs` | **NEW** — internal gRPC service implementation |
| `deriva-network/tests/replication.rs` | **NEW** — unit + integration tests |

## 9. Dependency Changes

| Crate | Dependency | Version | Reason |
|-------|-----------|---------|--------|
| `deriva-network` | `sha2` | `0.10` | Recipe fingerprint computation |
| `deriva-network` | `tonic` | `0.11` | gRPC client for peer communication |

---

## 10. Design Rationale

### 10.1 Why Synchronous Replication Instead of Async Gossip?

Recipes are critical metadata. If a client puts a recipe and immediately
calls `get`, any node must be able to resolve it. Async gossip would create
a window where some nodes don't have the recipe yet.

| Approach | Read-after-write | Latency | Complexity |
|----------|-----------------|---------|------------|
| Sync all-node | ✅ Guaranteed | ~1ms (LAN) | Moderate |
| Async gossip | ❌ Window of inconsistency | ~0ms (fire-and-forget) | Low |
| Quorum write | ✅ If read quorum matches | ~1ms | High |
| Chain replication | ✅ Guaranteed | ~N×RTT | Moderate |

Synchronous replication ensures read-after-write consistency for recipes.
The cost (~1ms per put_recipe) is acceptable because put_recipe is not
a hot path (recipes are created infrequently compared to get).

### 10.2 Why Anti-Entropy in Addition to Synchronous Replication?

Synchronous replication can fail (network issues, node restarts). Anti-entropy
is the safety net that ensures eventual consistency even after failures.
Belt-and-suspenders approach: sync replication for normal operation,
anti-entropy for recovery.

```
Without anti-entropy:
  Node A puts recipe X → replicates to B, C
  Node C crashes, restarts with empty DAG
  Recipe X is lost on C forever (unless re-put)

With anti-entropy:
  Node C restarts → anti-entropy syncs with random peer
  Within 30s, C has recipe X again
  No manual intervention needed
```

### 10.3 Why Fingerprint-Based Sync Instead of Vector Clocks?

| Approach | Complexity | Overhead | Suitability |
|----------|-----------|----------|-------------|
| Fingerprint (SHA-256) | O(N) compute | 32 bytes wire | Immutable data ✅ |
| Vector clocks | O(1) compare | O(N_nodes) per entry | Mutable data |
| Merkle tree | O(log N) compare | O(log N) wire | Large datasets |
| Version vectors | O(1) compare | O(N_nodes) per entry | Mutable data |

Fingerprint comparison is simpler and sufficient:
- If fingerprints match → in sync (no further work)
- If different → exchange full addr lists and compute diff
- No need for causal ordering (recipes are immutable, content-addressed)

Vector clocks would add complexity without benefit for immutable data.

### 10.4 Why Not Merkle Trees for Anti-Entropy?

Merkle trees (as used in Dynamo, Cassandra) are optimal when:
- Dataset is very large (millions of entries)
- Only a small fraction differs between nodes
- You want O(log N) diff instead of O(N)

For Deriva recipes:
- Even 100K recipes = 3.2MB addr list (fits in one gRPC message)
- Full addr exchange is simple and fast enough
- Merkle tree adds implementation complexity (tree construction, traversal)
- Benefit only appears at >1M recipes

Decision: start with fingerprint + full list. Add Merkle tree if recipe
count exceeds 1M (unlikely in near term).

### 10.5 Why Content-Addressed Recipes Simplify Replication

Traditional replication must handle conflicts (two nodes write different
values for the same key). Content-addressed recipes eliminate this:

```
Node A: put_recipe("f", [X]) → addr = SHA256("f" + "v1" + X) = 0xabc
Node B: put_recipe("f", [X]) → addr = SHA256("f" + "v1" + X) = 0xabc

Same inputs → same addr → same recipe → no conflict possible.

Node A: put_recipe("f", [X]) → 0xabc
Node B: put_recipe("g", [Y]) → 0xdef

Different recipes → different addrs → no conflict possible.
```

This means:
- No conflict resolution needed
- No last-writer-wins semantics
- No vector clocks or CRDTs
- Replication is purely additive (insert-only)
- Idempotent by construction

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `recipe_replication_total` | Counter | `result={success,failure}` | Total replication attempts |
| `recipe_replication_latency_ms` | Histogram | `peer_count` | End-to-end replication time |
| `recipe_replication_retry_total` | Counter | `peer` | Retries per peer |
| `anti_entropy_sync_total` | Counter | `result={in_sync,diverged}` | Sync round outcomes |
| `anti_entropy_recipes_fetched` | Counter | — | Recipes fetched during sync |
| `anti_entropy_latency_ms` | Histogram | — | Full sync round duration |
| `recipe_fingerprint_compute_ms` | Histogram | — | Fingerprint computation time |
| `recipe_replication_peers_failed` | Gauge | — | Current count of unreachable peers |

### 11.2 Log Events

```rust
// On successful replication
tracing::info!(
    addr = %addr,
    peers = peers.len(),
    latency_ms = elapsed.as_millis(),
    "recipe replicated to all peers"
);

// On replication failure
tracing::warn!(
    addr = %addr,
    failed_peers = ?result.failures,
    success = result.success_count,
    total = result.total_peers,
    "recipe replication partially failed"
);

// On anti-entropy sync
tracing::debug!(
    peer = %peer_id,
    in_sync = sync_resp.in_sync,
    missing = missing.len(),
    "anti-entropy sync round"
);

// On CAddr verification failure
tracing::error!(
    claimed_addr = %claimed,
    computed_addr = %computed,
    peer = %peer_id,
    "recipe CAddr verification failed — rejecting"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| Replication failure rate high | `recipe_replication_total{result=failure}` > 10% over 5m | Warning |
| Anti-entropy divergence persistent | `anti_entropy_sync_total{result=diverged}` for same peer > 5 consecutive rounds | Warning |
| CAddr verification failure | Any `bad_replication` counter increment | Critical |
| Replication latency spike | p99 `recipe_replication_latency_ms` > 10s | Warning |

---

## 12. Checklist

- [ ] Create `deriva-network/src/replication.rs`
- [ ] Implement `RecipeReplicator` with concurrent RPC + retry
- [ ] Implement `compute_recipe_fingerprint`
- [ ] Implement `find_missing_recipes`
- [ ] Implement `anti_entropy_loop`
- [ ] Implement `verify_recipe_addr` on receive path
- [ ] Create `proto/deriva_internal.proto` with internal RPCs
- [ ] Update `put_recipe` handler with replication
- [ ] Implement `replicate_recipe` internal handler
- [ ] Implement `sync_recipes` internal handler
- [ ] Implement `fetch_recipes` internal handler
- [ ] Add replication metrics (8 metrics)
- [ ] Add structured log events for replication
- [ ] Configure alerts for replication failures
- [ ] Write unit tests — fingerprint (~4 tests)
- [ ] Write unit tests — find_missing (~3 tests)
- [ ] Write unit tests — ReplicationResult (~2 tests)
- [ ] Write unit tests — config + idempotency (~4 tests)
- [ ] Write integration tests — happy path (~3 tests)
- [ ] Write integration tests — failure scenarios (~2 tests)
- [ ] Run benchmarks: replication latency, fingerprint, throughput
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
