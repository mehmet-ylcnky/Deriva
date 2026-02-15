# §3.9 Cluster-Wide Garbage Collection

> **Status:** Not started
> **Depends on:** §2.8 (single-node GC), §3.1 (SWIM gossip), §3.2 (recipe replication), §3.3 (leaf replication), §3.8 (bootstrap)
> **Crate(s):** `deriva-network`, `deriva-storage`, `deriva-server`
> **Estimated effort:** 3–4 days

---

## 1. Problem Statement

Single-node GC (§2.8) performs mark-and-sweep locally: find all reachable
CAddrs from the DAG, delete everything else. In a distributed cluster, this
is unsafe:

1. **Node A deletes a blob that Node B still references** — Node B has a recipe
   pointing to a leaf stored on A. A's local GC sees no local reference and
   deletes it. B's next computation fails.

2. **In-flight operations** — a materialization in progress on Node C may need
   a blob that appears unreferenced at GC scan time.

3. **Hinted handoffs** — hints queued for a recovering node reference blobs
   that must not be collected.

4. **Replication lag** — a newly written leaf may not have reached all replicas
   yet. GC on a replica that hasn't received it sees no reference.

The cluster needs a coordinated GC protocol where **no blob is deleted unless
ALL nodes agree it is unreferenced**.

### Why this is hard

```
Node A: has blob X (leaf), no local recipe references X
Node B: has recipe R that uses X as input
Node C: has nothing related to X

Single-node GC on A: "X is unreferenced" → DELETE X → WRONG!
  B's recipe R still needs X.

Correct approach:
  1. A asks all nodes: "who references X?"
  2. B responds: "I have recipe R that uses X"
  3. A: "X is referenced" → KEEP X

  This requires a global view of references.
```

---

## 2. Design

### 2.1 Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│              Distributed GC Protocol (3-Phase)                  │
└─────────────────────────────────────────────────────────────────┘

  Phase 1: ELECT COORDINATOR
  ┌──────────────────────────────────────────────────────────────┐
  │ Lowest NodeId among alive members becomes GC coordinator.    │
  │ Coordinator acquires distributed GC lock (leader lease).     │
  └──────────────────────────────────────────────────────────────┘
                              │
                              ▼
  Phase 2: COLLECT ROOT SETS
  ┌──────────────────────────────────────────────────────────────┐
  │ Coordinator broadcasts GcMarkRequest to all nodes.           │
  │ Each node responds with its local root set:                  │
  │   - All CAddrs referenced by local recipes (inputs + outputs)│
  │   - All CAddrs in pending hinted handoffs                    │
  │   - All CAddrs in active materializations                    │
  │ Coordinator computes GLOBAL root set = union of all locals.  │
  └──────────────────────────────────────────────────────────────┘
                              │
                              ▼
  Phase 3: SWEEP
  ┌──────────────────────────────────────────────────────────────┐
  │ Coordinator broadcasts GcSweepRequest with sweep list:       │
  │   sweep_list = local_blobs - global_root_set                 │
  │ Each node deletes blobs in sweep_list from local storage.    │
  │ Nodes report bytes reclaimed.                                │
  │ Coordinator releases GC lock.                                │
  └──────────────────────────────────────────────────────────────┘


  Timeline:
  ┌────────┐  ┌──────────────┐  ┌───────────┐  ┌────────┐
  │ Elect  │─►│ Collect Roots│─►│  Sweep    │─►│ Done   │
  │ ~10ms  │  │  ~100ms      │  │  ~500ms   │  │        │
  └────────┘  └──────────────┘  └───────────┘  └────────┘
```

### 2.2 GC Coordinator Election

```
Simple leader election: lowest NodeId wins.

  Alive members: [Node-A (id=0x1a..), Node-B (id=0x3f..), Node-C (id=0x7c..)]
  Coordinator: Node-A (lowest UUID)

  Why not Raft/Paxos?
  - GC is infrequent (every 5-30 minutes)
  - Coordinator failure just means GC doesn't run this cycle
  - No state to replicate (GC is stateless)
  - Simplicity >> correctness of leader election for GC

  If coordinator crashes mid-GC:
  - GC lock expires (TTL = 2 × gc_interval)
  - Next cycle, new lowest NodeId becomes coordinator
  - No partial deletes: sweep only happens after all roots collected
```

### 2.3 Core Types

```rust
/// Distributed GC coordinator.
pub struct ClusterGc {
    swim: Arc<SwimRuntime>,
    config: ClusterGcConfig,
    /// Prevents concurrent GC runs.
    gc_lock: Arc<Mutex<()>>,
    /// Current GC state for observability.
    state: Arc<RwLock<GcState>>,
}

#[derive(Debug, Clone)]
pub struct ClusterGcConfig {
    /// How often to run GC.
    pub interval: Duration,                   // default: 10 minutes
    /// Maximum time for the entire GC cycle.
    pub cycle_timeout: Duration,              // default: 60s
    /// Maximum time to wait for a single node's root set.
    pub node_timeout: Duration,               // default: 10s
    /// Minimum number of nodes that must respond for GC to proceed.
    pub quorum: QuorumRequirement,            // default: Majority
    /// Enable dry-run mode (report but don't delete).
    pub dry_run: bool,                        // default: false
    /// Minimum age of a blob before it can be collected.
    /// Protects in-flight writes from being swept.
    pub min_blob_age: Duration,               // default: 5 minutes
}

#[derive(Debug, Clone)]
pub enum QuorumRequirement {
    /// Majority of alive nodes must respond.
    Majority,
    /// All alive nodes must respond.
    All,
    /// At least N nodes must respond.
    AtLeast(usize),
}

#[derive(Debug, Clone)]
pub enum GcState {
    Idle,
    Electing,
    CollectingRoots { responded: usize, total: usize },
    Sweeping { sweep_count: usize },
    Done(GcResult),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct GcResult {
    pub blobs_swept: usize,
    pub bytes_reclaimed: u64,
    pub nodes_participated: usize,
    pub global_root_size: usize,
    pub duration: Duration,
    pub dry_run: bool,
}

impl Default for ClusterGcConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(600),
            cycle_timeout: Duration::from_secs(60),
            node_timeout: Duration::from_secs(10),
            quorum: QuorumRequirement::Majority,
            dry_run: false,
            min_blob_age: Duration::from_secs(300),
        }
    }
}
```

### 2.4 Wire Protocol

```protobuf
// Added to proto/deriva_internal.proto

service DerivaInternal {
    // Existing RPCs...

    // GC Phase 2: Request local root set from a node.
    rpc GcCollectRoots(GcCollectRootsRequest) returns (GcCollectRootsResponse);

    // GC Phase 3: Instruct a node to sweep specified blobs.
    rpc GcSweep(GcSweepRequest) returns (GcSweepResponse);
}

message GcCollectRootsRequest {
    string gc_cycle_id = 1;     // Unique ID for this GC cycle
    uint64 min_blob_age_secs = 2; // Only consider blobs older than this
}

message GcCollectRootsResponse {
    repeated bytes root_addrs = 1;  // CAddrs in local root set
    uint64 total_blobs = 2;         // Total blobs on this node
    uint64 total_bytes = 3;         // Total bytes on this node
}

message GcSweepRequest {
    string gc_cycle_id = 1;
    repeated bytes sweep_addrs = 2; // CAddrs to delete
    bool dry_run = 3;
}

message GcSweepResponse {
    uint64 blobs_deleted = 1;
    uint64 bytes_reclaimed = 2;
}
```

---

## 3. Implementation

### 3.1 GC Coordinator

```rust
// deriva-network/src/cluster_gc.rs

impl ClusterGc {
    pub fn new(swim: Arc<SwimRuntime>, config: ClusterGcConfig) -> Self {
        Self {
            swim,
            config,
            gc_lock: Arc::new(Mutex::new(())),
            state: Arc::new(RwLock::new(GcState::Idle)),
        }
    }

    /// Start the periodic GC background task.
    pub fn spawn_periodic(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.config.interval);
            loop {
                interval.tick().await;
                if self.is_coordinator() {
                    match self.run_gc_cycle().await {
                        Ok(result) => {
                            tracing::info!(
                                swept = result.blobs_swept,
                                reclaimed_bytes = result.bytes_reclaimed,
                                nodes = result.nodes_participated,
                                dry_run = result.dry_run,
                                duration_ms = result.duration.as_millis(),
                                "GC cycle complete"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "GC cycle failed");
                        }
                    }
                }
            }
        })
    }

    /// Am I the coordinator? (lowest NodeId among alive members)
    fn is_coordinator(&self) -> bool {
        let local = self.swim.local_id();
        let members = self.swim.alive_members();
        members.keys().all(|peer| local.id < peer.id)
    }

    /// Run a single GC cycle.
    pub async fn run_gc_cycle(&self) -> Result<GcResult, DerivaError> {
        let _lock = self.gc_lock.try_lock()
            .map_err(|_| DerivaError::Internal("GC already running".into()))?;

        let started = Instant::now();
        let cycle_id = Uuid::new_v4().to_string();

        tracing::info!(cycle_id = %cycle_id, "starting GC cycle");

        // Phase 2: Collect root sets from all nodes
        self.set_state(GcState::Electing);
        let members = self.swim.alive_members();
        let total_nodes = members.len() + 1; // +1 for self

        self.set_state(GcState::CollectingRoots { responded: 0, total: total_nodes });

        let global_roots = self.collect_global_roots(&cycle_id, &members).await?;

        tracing::info!(
            global_root_size = global_roots.len(),
            "global root set computed"
        );

        // Phase 3: Compute sweep list and execute
        let local_blobs = self.swim.local_blob_addrs().await?;
        let sweep_list: Vec<CAddr> = local_blobs.into_iter()
            .filter(|addr| !global_roots.contains(addr))
            .collect();

        self.set_state(GcState::Sweeping { sweep_count: sweep_list.len() });

        let (blobs_swept, bytes_reclaimed) = self.execute_sweep(
            &cycle_id, &sweep_list, &members
        ).await?;

        let result = GcResult {
            blobs_swept,
            bytes_reclaimed,
            nodes_participated: total_nodes,
            global_root_size: global_roots.len(),
            duration: started.elapsed(),
            dry_run: self.config.dry_run,
        };

        self.set_state(GcState::Done(result.clone()));
        Ok(result)
    }

    /// Collect root sets from all nodes (including self).
    async fn collect_global_roots(
        &self,
        cycle_id: &str,
        members: &HashMap<NodeId, NodeMetadata>,
    ) -> Result<HashSet<CAddr>, DerivaError> {
        let mut global_roots = HashSet::new();

        // Local root set
        let local_roots = self.collect_local_roots().await?;
        global_roots.extend(local_roots);

        // Remote root sets (fan-out)
        let mut futures = Vec::new();
        for (peer, _) in members {
            let fut = self.request_roots(peer, cycle_id);
            futures.push((peer.clone(), fut));
        }

        let mut responded = 1usize; // self already responded
        let required = self.quorum_required(members.len() + 1);
        let mut errors = Vec::new();

        for (peer, fut) in futures {
            match tokio::time::timeout(self.config.node_timeout, fut).await {
                Ok(Ok(roots)) => {
                    global_roots.extend(roots);
                    responded += 1;
                    self.set_state(GcState::CollectingRoots {
                        responded,
                        total: members.len() + 1,
                    });
                }
                Ok(Err(e)) => {
                    tracing::warn!(peer = %peer, error = %e, "root collection failed");
                    errors.push(e);
                }
                Err(_) => {
                    tracing::warn!(peer = %peer, "root collection timed out");
                    errors.push(DerivaError::Timeout("root collection".into()));
                }
            }
        }

        if responded < required {
            return Err(DerivaError::Consistency(format!(
                "GC quorum not met: {}/{} responded, {} required",
                responded, members.len() + 1, required
            )));
        }

        Ok(global_roots)
    }

    /// Collect local root set: all CAddrs referenced by recipes, hints, active ops.
    async fn collect_local_roots(&self) -> Result<Vec<CAddr>, DerivaError> {
        let mut roots = Vec::new();

        // All recipe inputs and outputs
        let dag = self.swim.local_dag_store();
        for (addr, recipe) in dag.all_recipes() {
            roots.push(addr.clone());
            for input in &recipe.inputs {
                roots.push(input.addr().clone());
            }
        }

        // All pending hinted handoffs
        let hints = self.swim.local_hint_store();
        for hint in hints.all_pending() {
            roots.push(hint.addr.clone());
        }

        // All CAddrs in active materializations
        let active = self.swim.active_materializations();
        for addr in active {
            roots.push(addr);
        }

        Ok(roots)
    }

    /// Request root set from a remote node.
    async fn request_roots(
        &self,
        peer: &NodeId,
        cycle_id: &str,
    ) -> Result<Vec<CAddr>, DerivaError> {
        let channel = self.swim.get_channel(peer).await?;
        let mut client = DerivaInternalClient::new(channel);

        let response = client.gc_collect_roots(tonic::Request::new(
            GcCollectRootsRequest {
                gc_cycle_id: cycle_id.to_string(),
                min_blob_age_secs: self.config.min_blob_age.as_secs(),
            }
        )).await
            .map_err(|e| DerivaError::Network(format!("gc roots: {}", e)))?;

        let roots = response.into_inner().root_addrs.iter()
            .filter_map(|bytes| CAddr::from_bytes(bytes).ok())
            .collect();

        Ok(roots)
    }

    /// Execute sweep on local node and broadcast to peers.
    async fn execute_sweep(
        &self,
        cycle_id: &str,
        sweep_list: &[CAddr],
        members: &HashMap<NodeId, NodeMetadata>,
    ) -> Result<(usize, u64), DerivaError> {
        if sweep_list.is_empty() {
            return Ok((0, 0));
        }

        let sweep_bytes: Vec<Vec<u8>> = sweep_list.iter()
            .map(|a| a.as_bytes().to_vec())
            .collect();

        let mut total_deleted = 0usize;
        let mut total_reclaimed = 0u64;

        // Sweep locally
        let local_result = self.sweep_local(sweep_list).await?;
        total_deleted += local_result.0;
        total_reclaimed += local_result.1;

        // Broadcast sweep to peers
        for (peer, _) in members {
            match self.request_sweep(peer, cycle_id, &sweep_bytes).await {
                Ok((deleted, reclaimed)) => {
                    total_deleted += deleted;
                    total_reclaimed += reclaimed;
                }
                Err(e) => {
                    tracing::warn!(peer = %peer, error = %e, "remote sweep failed");
                    // Non-fatal: peer will clean up next cycle
                }
            }
        }

        Ok((total_deleted, total_reclaimed))
    }

    /// Sweep blobs from local storage.
    async fn sweep_local(&self, sweep_list: &[CAddr]) -> Result<(usize, u64), DerivaError> {
        if self.config.dry_run {
            let estimated_bytes: u64 = sweep_list.len() as u64 * 1024; // rough estimate
            tracing::info!(
                count = sweep_list.len(),
                estimated_bytes = estimated_bytes,
                "[DRY RUN] would sweep blobs"
            );
            return Ok((0, 0));
        }

        let blob_store = self.swim.local_blob_store();
        let mut deleted = 0usize;
        let mut reclaimed = 0u64;

        for addr in sweep_list {
            match blob_store.delete(addr) {
                Ok(Some(size)) => {
                    deleted += 1;
                    reclaimed += size;
                }
                Ok(None) => {} // already gone
                Err(e) => {
                    tracing::warn!(addr = %addr, error = %e, "failed to delete blob");
                }
            }
        }

        Ok((deleted, reclaimed))
    }

    /// Request sweep on a remote node.
    async fn request_sweep(
        &self,
        peer: &NodeId,
        cycle_id: &str,
        sweep_bytes: &[Vec<u8>],
    ) -> Result<(usize, u64), DerivaError> {
        let channel = self.swim.get_channel(peer).await?;
        let mut client = DerivaInternalClient::new(channel);

        let response = client.gc_sweep(tonic::Request::new(GcSweepRequest {
            gc_cycle_id: cycle_id.to_string(),
            sweep_addrs: sweep_bytes.to_vec(),
            dry_run: self.config.dry_run,
        })).await
            .map_err(|e| DerivaError::Network(format!("gc sweep: {}", e)))?;

        let resp = response.into_inner();
        Ok((resp.blobs_deleted as usize, resp.bytes_reclaimed))
    }

    fn quorum_required(&self, total: usize) -> usize {
        match &self.config.quorum {
            QuorumRequirement::Majority => total / 2 + 1,
            QuorumRequirement::All => total,
            QuorumRequirement::AtLeast(n) => (*n).min(total),
        }
    }

    fn set_state(&self, new_state: GcState) {
        *self.state.write().unwrap() = new_state;
    }

    pub fn state(&self) -> GcState {
        self.state.read().unwrap().clone()
    }
}
```

### 3.2 GC RPC Handlers

```rust
// Added to deriva-server/src/internal.rs

async fn gc_collect_roots(
    &self,
    request: Request<GcCollectRootsRequest>,
) -> Result<Response<GcCollectRootsResponse>, Status> {
    let req = request.into_inner();
    let min_age = Duration::from_secs(req.min_blob_age_secs);

    tracing::debug!(cycle_id = %req.gc_cycle_id, "collecting local GC roots");

    let mut roots = Vec::new();

    // Recipe references
    for (addr, recipe) in self.state.dag_store.all_recipes() {
        roots.push(addr.as_bytes().to_vec());
        for input in &recipe.inputs {
            roots.push(input.addr().as_bytes().to_vec());
        }
    }

    // Hinted handoff references
    if let Some(hints) = &self.state.hint_store {
        for hint in hints.all_pending() {
            roots.push(hint.addr.as_bytes().to_vec());
        }
    }

    // Active materialization references
    for addr in self.state.executor.active_addrs() {
        roots.push(addr.as_bytes().to_vec());
    }

    let (total_blobs, total_bytes) = self.state.blob_store.stats();

    Ok(Response::new(GcCollectRootsResponse {
        root_addrs: roots,
        total_blobs,
        total_bytes,
    }))
}

async fn gc_sweep(
    &self,
    request: Request<GcSweepRequest>,
) -> Result<Response<GcSweepResponse>, Status> {
    let req = request.into_inner();

    tracing::info!(
        cycle_id = %req.gc_cycle_id,
        count = req.sweep_addrs.len(),
        dry_run = req.dry_run,
        "received GC sweep request"
    );

    if req.dry_run {
        return Ok(Response::new(GcSweepResponse {
            blobs_deleted: 0,
            bytes_reclaimed: 0,
        }));
    }

    let mut deleted = 0u64;
    let mut reclaimed = 0u64;

    for addr_bytes in &req.sweep_addrs {
        if let Ok(addr) = CAddr::from_bytes(addr_bytes) {
            match self.state.blob_store.delete(&addr) {
                Ok(Some(size)) => {
                    deleted += 1;
                    reclaimed += size;
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(addr = %addr, error = %e, "sweep delete failed");
                }
            }
            // Also evict from cache
            self.state.cache.evict(&addr);
        }
    }

    Ok(Response::new(GcSweepResponse {
        blobs_deleted: deleted,
        bytes_reclaimed: reclaimed,
    }))
}
```


---

## 4. Data Flow Diagrams

### 4.1 Normal GC Cycle — 3-Node Cluster

```
  Node A (coordinator)      Node B                  Node C
    │                         │                       │
    │ is_coordinator() = true │                       │
    │ acquire gc_lock         │                       │
    │                         │                       │
    │── GcCollectRoots ──────►│                       │
    │── GcCollectRoots ──────────────────────────────►│
    │                         │                       │
    │ collect local roots     │ collect local roots   │ collect local roots
    │ roots_A = {X,Y,Z}      │ roots_B = {Y,W}      │ roots_C = {Z,V}
    │                         │                       │
    │◄── roots_B = {Y,W} ───│                       │
    │◄── roots_C = {Z,V} ──────────────────────────│
    │                         │                       │
    │ global = {X,Y,Z,W,V}   │                       │
    │                         │                       │
    │ local blobs = {X,Y,Z,Q,R}                      │
    │ sweep = {Q,R} (not in global)                   │
    │                         │                       │
    │ delete Q,R locally      │                       │
    │── GcSweep({Q,R}) ─────►│ delete Q,R if present │
    │── GcSweep({Q,R}) ─────────────────────────────►│ delete Q,R if present
    │                         │                       │
    │ result: 2 swept, 50KB   │                       │
    │ release gc_lock         │                       │
```

### 4.2 GC with Quorum Failure

```
  Node A (coordinator)      Node B                  Node C (down)
    │                         │                       │
    │── GcCollectRoots ──────►│                       │
    │── GcCollectRoots ──────────────────────────── X │ (unreachable)
    │                         │                       │
    │◄── roots_B ────────────│                       │
    │                         │ timeout (10s)         │
    │                         │                       │
    │ responded: 2/3 (A + B)  │                       │
    │ quorum = Majority = 2   │                       │
    │ 2 >= 2 → quorum met ✓  │                       │
    │                         │                       │
    │ PROCEED with sweep      │                       │
    │ (conservative: C's roots│                       │
    │  not included, so we    │                       │
    │  may keep blobs C refs) │                       │
    │                         │                       │

  Safety: missing C's roots means we KEEP more blobs than necessary
  (false negatives in sweep list). Never delete something C needs.
  Worst case: some garbage survives until next cycle when C is back.
```

### 4.3 GC Coordinator Failure Mid-Cycle

```
  Node A (coordinator)      Node B                  Node C
    │                         │                       │
    │── GcCollectRoots ──────►│                       │
    │── GcCollectRoots ──────────────────────────────►│
    │                         │                       │
    │◄── roots_B ────────────│                       │
    │                         │                       │
    │ ╳ CRASH                 │                       │
    │                         │                       │
    │                         │ (no sweep received)   │ (no sweep received)
    │                         │                       │
    │                         │ Nothing deleted.      │ Nothing deleted.
    │                         │ GC lock expires.      │
    │                         │                       │
    │                         │ Next cycle:           │
    │                         │ Node B is now lowest  │
    │                         │ → becomes coordinator │
    │                         │ → runs full GC cycle  │

  Safety: coordinator crash before sweep = no deletions = safe.
  The only risk is if crash happens DURING sweep broadcast:
  some nodes swept, others didn't. This is fine — next cycle
  catches the rest. No data loss possible.
```

### 4.4 GC with In-Flight Materialization

```
  Node A (coordinator)      Node B (computing)
    │                         │
    │── GcCollectRoots ──────►│
    │                         │
    │                         │ active_materializations = {X}
    │                         │ (currently computing recipe that needs blob X)
    │                         │
    │                         │ roots_B = {Y, W, X}  ← X included!
    │◄── roots_B ────────────│
    │                         │
    │ global roots include X  │
    │ → X NOT in sweep list   │
    │ → X preserved ✓         │
    │                         │
    │                         │ materialization completes
    │                         │ X no longer in active set
    │                         │ (next GC cycle may collect X
    │                         │  if no recipe references it)
```

### 4.5 Dry-Run GC

```
  Node A (coordinator, dry_run=true)
    │
    │ collect global roots → {X,Y,Z}
    │ local blobs → {X,Y,Z,Q,R,S}
    │ sweep list → {Q,R,S}
    │
    │ [DRY RUN] would sweep 3 blobs (~150KB)
    │
    │── GcSweep(dry_run=true) → Node B: 0 deleted
    │── GcSweep(dry_run=true) → Node C: 0 deleted
    │
    │ Result: blobs_swept=0, bytes_reclaimed=0
    │ (but logs show what WOULD be collected)
```

---

## 5. Test Specification

### 5.1 Coordinator Election Tests

```rust
#[cfg(test)]
mod election_tests {
    use super::*;

    fn make_gc(local_id: &str, peers: &[&str]) -> ClusterGc {
        // Helper: create ClusterGc with mock SWIM
        // local_id and peers are UUID strings
        todo!()
    }

    #[test]
    fn test_lowest_id_is_coordinator() {
        let gc = make_gc("00000001", &["00000002", "00000003"]);
        assert!(gc.is_coordinator());
    }

    #[test]
    fn test_not_coordinator_if_higher_id() {
        let gc = make_gc("00000003", &["00000001", "00000002"]);
        assert!(!gc.is_coordinator());
    }

    #[test]
    fn test_single_node_is_coordinator() {
        let gc = make_gc("00000001", &[]);
        assert!(gc.is_coordinator());
    }

    #[test]
    fn test_coordinator_changes_on_member_leave() {
        // Node 1 was coordinator, leaves cluster
        // Node 2 (next lowest) becomes coordinator
        let gc = make_gc("00000002", &["00000003"]);
        assert!(gc.is_coordinator());
    }
}
```

### 5.2 Quorum Requirement Tests

```rust
#[test]
fn test_majority_quorum() {
    let gc = ClusterGc::new(mock_swim(), ClusterGcConfig {
        quorum: QuorumRequirement::Majority,
        ..Default::default()
    });
    assert_eq!(gc.quorum_required(3), 2);
    assert_eq!(gc.quorum_required(5), 3);
    assert_eq!(gc.quorum_required(1), 1);
}

#[test]
fn test_all_quorum() {
    let gc = ClusterGc::new(mock_swim(), ClusterGcConfig {
        quorum: QuorumRequirement::All,
        ..Default::default()
    });
    assert_eq!(gc.quorum_required(3), 3);
}

#[test]
fn test_at_least_quorum() {
    let gc = ClusterGc::new(mock_swim(), ClusterGcConfig {
        quorum: QuorumRequirement::AtLeast(2),
        ..Default::default()
    });
    assert_eq!(gc.quorum_required(3), 2);
    assert_eq!(gc.quorum_required(1), 1); // capped at total
}
```

### 5.3 Root Set Collection Tests

```rust
#[tokio::test]
async fn test_local_roots_include_recipe_inputs() {
    let gc = setup_gc_with_recipes(vec![
        recipe("R1", vec!["leaf_A", "leaf_B"]),
        recipe("R2", vec!["leaf_B", "leaf_C"]),
    ]).await;

    let roots = gc.collect_local_roots().await.unwrap();
    let root_set: HashSet<_> = roots.into_iter().collect();

    // Should include R1, R2 (recipe addrs) + leaf_A, leaf_B, leaf_C (inputs)
    assert!(root_set.contains(&addr("R1")));
    assert!(root_set.contains(&addr("R2")));
    assert!(root_set.contains(&addr("leaf_A")));
    assert!(root_set.contains(&addr("leaf_B")));
    assert!(root_set.contains(&addr("leaf_C")));
}

#[tokio::test]
async fn test_local_roots_include_hints() {
    let gc = setup_gc_with_hints(vec![
        hint("hint_blob_1"),
        hint("hint_blob_2"),
    ]).await;

    let roots = gc.collect_local_roots().await.unwrap();
    let root_set: HashSet<_> = roots.into_iter().collect();

    assert!(root_set.contains(&addr("hint_blob_1")));
    assert!(root_set.contains(&addr("hint_blob_2")));
}

#[tokio::test]
async fn test_local_roots_include_active_materializations() {
    let gc = setup_gc_with_active(vec!["computing_X"]).await;

    let roots = gc.collect_local_roots().await.unwrap();
    assert!(roots.contains(&addr("computing_X")));
}
```

### 5.4 Sweep Tests

```rust
#[tokio::test]
async fn test_sweep_deletes_unreferenced_blobs() {
    let gc = setup_gc_with_blobs(vec!["A", "B", "C", "D"]).await;
    // Global roots = {A, B}
    // Sweep list = {C, D}

    let (deleted, _) = gc.sweep_local(&[addr("C"), addr("D")]).await.unwrap();
    assert_eq!(deleted, 2);

    // Verify A and B still exist
    assert!(gc.blob_exists(&addr("A")).await);
    assert!(gc.blob_exists(&addr("B")).await);
    assert!(!gc.blob_exists(&addr("C")).await);
    assert!(!gc.blob_exists(&addr("D")).await);
}

#[tokio::test]
async fn test_dry_run_deletes_nothing() {
    let gc = setup_gc_with_config(ClusterGcConfig {
        dry_run: true,
        ..Default::default()
    }).await;

    let (deleted, reclaimed) = gc.sweep_local(&[addr("X")]).await.unwrap();
    assert_eq!(deleted, 0);
    assert_eq!(reclaimed, 0);
    assert!(gc.blob_exists(&addr("X")).await); // still there
}

#[tokio::test]
async fn test_sweep_nonexistent_blob_is_noop() {
    let gc = setup_gc_with_blobs(vec!["A"]).await;
    let (deleted, _) = gc.sweep_local(&[addr("Z")]).await.unwrap(); // Z doesn't exist
    assert_eq!(deleted, 0);
}
```

### 5.5 Integration Tests

```rust
#[tokio::test]
async fn test_full_gc_cycle_3_nodes() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Put some leaves and recipes
    let leaf_a = put_leaf(&nodes[0], b"data_a").await;
    let leaf_b = put_leaf(&nodes[0], b"data_b").await;
    let recipe = put_recipe(&nodes[0], "f", "v1", vec![leaf_a.clone()]).await;
    tokio::time::sleep(Duration::from_secs(3)).await; // replication

    // Put an orphan blob directly (no recipe references it)
    let orphan = nodes[0].blob_store.put_raw(b"orphan_data").await.unwrap();
    tokio::time::sleep(Duration::from_secs(301)).await; // wait for min_blob_age

    // Run GC
    let coordinator = &nodes[0].cluster_gc;
    let result = coordinator.run_gc_cycle().await.unwrap();

    // Orphan should be swept, leaf_a, leaf_b, recipe preserved
    assert!(result.blobs_swept >= 1);
    assert!(nodes[0].blob_store.exists(&leaf_a).await);
    assert!(nodes[0].blob_store.exists(&leaf_b).await);
}

#[tokio::test]
async fn test_gc_quorum_failure_aborts() {
    let (nodes, mut handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Kill 2 nodes — only coordinator remains
    drop(handles.remove(2));
    drop(handles.remove(1));
    tokio::time::sleep(Duration::from_secs(5)).await;

    let gc = ClusterGc::new(nodes[0].swim.clone(), ClusterGcConfig {
        quorum: QuorumRequirement::Majority, // needs 2/3
        ..Default::default()
    });

    let result = gc.run_gc_cycle().await;
    assert!(result.is_err()); // quorum not met
}

#[tokio::test]
async fn test_gc_concurrent_lock() {
    let gc = Arc::new(setup_single_node_gc().await);

    let gc1 = gc.clone();
    let gc2 = gc.clone();

    let h1 = tokio::spawn(async move { gc1.run_gc_cycle().await });
    let h2 = tokio::spawn(async move { gc2.run_gc_cycle().await });

    let (r1, r2) = tokio::join!(h1, h2);
    // One should succeed, one should fail with "already running"
    let results = vec![r1.unwrap(), r2.unwrap()];
    assert!(results.iter().any(|r| r.is_ok()));
    assert!(results.iter().any(|r| r.is_err()));
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | No blobs to sweep | GC completes instantly, result=0 | No-op is valid |
| 2 | Coordinator crashes mid-collect | No sweep happens, lock expires | Safe: no deletions |
| 3 | Coordinator crashes mid-sweep | Partial sweep, rest caught next cycle | Safe: idempotent deletes |
| 4 | Node joins during GC | Not included in this cycle | Caught next cycle |
| 5 | Node leaves during GC | Root collection may timeout for it | Quorum handles this |
| 6 | Blob written during GC collect phase | min_blob_age protects it (5min) | Young blobs excluded |
| 7 | Recipe deleted during GC | Inputs may become unreferenced | Caught this or next cycle |
| 8 | Concurrent GC attempts | Mutex prevents double-run | Only one GC at a time |
| 9 | Very large sweep list (>100K blobs) | Batched sweep RPCs | Prevent message size limits |
| 10 | All nodes are coordinator candidates | Lowest ID wins deterministically | No ambiguity |
| 11 | Single-node cluster | GC runs locally only, no RPCs | Degrades to §2.8 behavior |
| 12 | GC cycle exceeds timeout | Abort, release lock, log warning | Bounded cycle time |

### 6.1 The min_blob_age Safety Window

```
Problem: blob written at t=0, GC collect at t=1, sweep at t=2.

  t=0: Node A writes blob X (put_leaf)
  t=1: GC coordinator collects roots from all nodes
       Node A's replication hasn't completed yet
       Node B doesn't know about X → not in B's root set
       But Node A's recipe references X → X in A's root set → SAFE

  But what if X is written AFTER root collection starts?
  t=0: GC starts collecting roots
  t=1: Node A writes blob X (new leaf, no recipe yet)
  t=2: GC sweep: X not in any root set → DELETE X → WRONG!

  Solution: min_blob_age = 5 minutes
  Only blobs older than 5 minutes are eligible for sweep.
  Since GC cycle takes <60s, any blob written during the cycle
  is protected by the age threshold.

  Timeline:
  ├── blob written ──── 5 min ──── eligible for GC ──►
  ├── GC cycle ── <60s ──►
  No overlap possible if min_blob_age > cycle_timeout.
```

### 6.2 Large Sweep List Batching

```
If sweep list > 10,000 blobs:
  - Split into batches of 1,000
  - Send each batch as separate GcSweep RPC
  - Aggregate results

  Reason: gRPC message size limit (default 4MB).
  10,000 CAddrs × 32 bytes = 320KB (fits in one message).
  100,000 CAddrs × 32 bytes = 3.2MB (close to limit).

  Batching threshold: 50,000 addrs per message (~1.6MB).
```

---

## 7. Performance Analysis

### 7.1 GC Cycle Latency

```
┌──────────────────────────┬──────────┬──────────────────────────┐
│ Phase                    │ Latency  │ Determined by            │
├──────────────────────────┼──────────┼──────────────────────────┤
│ Coordinator election     │ ~0ms     │ Local comparison         │
│ Root collection (3 nodes)│ ~100ms   │ Fan-out RPC + DAG scan   │
│ Root collection (50 nodes│ ~500ms   │ Slowest node response    │
│ Global root set union    │ ~10ms    │ HashSet merge            │
│ Sweep list computation   │ ~5ms     │ Set difference           │
│ Local sweep (1K blobs)   │ ~50ms    │ Disk deletes             │
│ Remote sweep broadcast   │ ~100ms   │ Fan-out RPC              │
├──────────────────────────┼──────────┼──────────────────────────┤
│ Total (3 nodes, 1K sweep)│ ~300ms   │                          │
│ Total (50 nodes, 10K)    │ ~1.5s    │                          │
└──────────────────────────┴──────────┴──────────────────────────┘
```

### 7.2 Memory Overhead

```
Root set size per node:
  - 10K recipes × 3 inputs avg = 40K CAddrs × 32 bytes = 1.28MB
  - 100 hints × 32 bytes = 3.2KB
  - 10 active materializations × 32 bytes = 320 bytes

Global root set (3 nodes):
  - ~120K CAddrs (with overlap) → ~3.8MB in memory
  - Held only during GC cycle (~1s)

Sweep list:
  - Typically <1% of total blobs → small

  Memory impact: negligible (<10MB peak during GC).
```

### 7.3 Disk I/O During Sweep

```
Sweep deletes blobs from BlobStore (§1.6: 2-level hex sharding).

  Each delete:
  - 1 filesystem unlink() call
  - ~0.1ms on SSD

  1,000 blob sweep: ~100ms
  10,000 blob sweep: ~1s

  Throttling: not needed for typical sweep sizes.
  For very large sweeps (>100K), consider rate-limiting
  deletes to avoid I/O spikes affecting read latency.
```

### 7.4 Benchmarking Plan

```rust
/// Benchmark: root set collection latency
#[bench]
fn bench_collect_local_roots(b: &mut Bencher) {
    // 10K recipes, 3 inputs each
    // Measure: collect_local_roots() latency
    // Expected: <50ms
}

/// Benchmark: sweep throughput
#[bench]
fn bench_sweep_local(b: &mut Bencher) {
    // 1K blobs to delete
    // Measure: sweep_local() latency
    // Expected: <100ms on SSD
}

/// Benchmark: global root set merge
#[bench]
fn bench_root_set_merge(b: &mut Bencher) {
    // 3 root sets of 40K CAddrs each
    // Measure: HashSet union time
    // Expected: <10ms
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/cluster_gc.rs` | **NEW** — ClusterGc, ClusterGcConfig, GcState, GcResult |
| `deriva-network/src/lib.rs` | Add `pub mod cluster_gc` |
| `proto/deriva_internal.proto` | Add `GcCollectRoots`, `GcSweep` RPCs |
| `deriva-server/src/internal.rs` | Add `gc_collect_roots`, `gc_sweep` handlers |
| `deriva-server/src/state.rs` | Add `cluster_gc: Option<Arc<ClusterGc>>` |
| `deriva-server/src/main.rs` | Spawn periodic GC task after bootstrap |
| `deriva-storage/src/blob_store.rs` | Add `delete()`, `stats()`, `all_addrs()` methods |
| `deriva-compute/src/executor.rs` | Add `active_addrs()` for in-flight tracking |
| `deriva-network/tests/cluster_gc.rs` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

No new external dependencies. Uses existing `uuid`, `tokio`, `tonic`, `dashmap`.

---

## 10. Design Rationale

### 10.1 Why Coordinator-Based Instead of Peer-to-Peer GC?

```
Peer-to-peer GC:
  Each node independently decides what to delete.
  Risk: Node A deletes blob X, Node B still needs X.
  Requires distributed consensus on every delete decision.

Coordinator-based GC:
  One node collects global view, makes all decisions.
  Simple, correct, single point of coordination.
  Coordinator failure = GC skipped (safe, not stuck).

  Trade-off: coordinator is a bottleneck for GC.
  But GC runs every 10 minutes and takes <2s.
  Not a scalability concern.
```

### 10.2 Why Quorum Instead of ALL for Root Collection?

```
ALL: every node must respond.
  One slow/dead node blocks GC entirely.
  In a 50-node cluster, probability of all responding = low.

Majority: >50% must respond.
  Missing nodes' roots are NOT included.
  This means we may KEEP blobs that missing nodes reference.
  We will NOT delete blobs that missing nodes reference
  (because those blobs are also referenced by responding nodes
   via recipe replication — all nodes have all recipes).

  Wait — what about leaf-only references?
  A leaf on Node X, referenced by no recipe, only stored on X.
  If X doesn't respond, we don't know about this leaf.
  But: leaves without recipe references are already orphans!
  (No recipe points to them → they're garbage.)

  ∴ Majority quorum is safe for GC root collection.
```

### 10.3 Why min_blob_age Instead of Write Barriers?

```
Write barrier approach:
  - Before GC: set a "GC in progress" flag
  - All writes during GC are added to root set
  - After GC: clear flag
  - Complex: distributed flag, race conditions

min_blob_age approach:
  - Only sweep blobs older than 5 minutes
  - Any blob written during GC cycle is <60s old → protected
  - Simple: no distributed coordination needed
  - Trade-off: garbage survives for 5 extra minutes

  Simplicity wins. 5 minutes of extra garbage is negligible.
```

### 10.4 Why Sweep Broadcast Instead of Per-Node Sweep Lists?

```
Per-node sweep lists:
  Coordinator computes separate sweep list for each node
  based on what blobs each node has.
  More precise, less network traffic.
  But: coordinator needs to know every node's blob inventory.

Broadcast single sweep list:
  Coordinator sends same list to all nodes.
  Each node deletes what it has from the list.
  Simpler, no need for per-node blob inventory.
  Slightly more network traffic (sending addrs that node doesn't have).

  For typical sweep sizes (<10K addrs × 32 bytes = 320KB),
  the broadcast overhead is negligible.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `gc_cycles_total` | Counter | `result={success,failed,skipped}` | GC cycle outcomes |
| `gc_cycle_duration_ms` | Histogram | — | Full cycle latency |
| `gc_blobs_swept` | Counter | — | Total blobs deleted by GC |
| `gc_bytes_reclaimed` | Counter | — | Total bytes freed by GC |
| `gc_root_set_size` | Gauge | — | Size of last global root set |
| `gc_sweep_list_size` | Gauge | — | Size of last sweep list |
| `gc_nodes_responded` | Gauge | — | Nodes that responded in last cycle |
| `gc_is_coordinator` | Gauge | — | 1 if this node is GC coordinator |
| `gc_state` | Gauge | `state` | Current GC state |
| `gc_lock_held` | Gauge | — | 1 if GC lock is held |

### 11.2 Structured Logging

```rust
tracing::info!(
    cycle_id = %cycle_id,
    coordinator = %self.swim.local_id(),
    alive_nodes = members.len() + 1,
    "starting GC cycle"
);

tracing::info!(
    cycle_id = %cycle_id,
    global_roots = global_roots.len(),
    responded = responded,
    total = total_nodes,
    "root collection complete"
);

tracing::info!(
    cycle_id = %cycle_id,
    swept = result.blobs_swept,
    reclaimed_bytes = result.bytes_reclaimed,
    duration_ms = result.duration.as_millis(),
    dry_run = result.dry_run,
    "GC cycle complete"
);

tracing::warn!(
    cycle_id = %cycle_id,
    responded = responded,
    required = required,
    "GC quorum not met — aborting cycle"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| GC cycle failed | `gc_cycles_total{result=failed}` > 3 consecutive | Warning |
| GC not running | `gc_cycles_total` = 0 for 30 minutes | Warning |
| Large sweep | `gc_sweep_list_size` > 100K | Info |
| GC cycle slow | `gc_cycle_duration_ms` > 30s | Warning |
| No coordinator | `gc_is_coordinator` = 0 on all nodes | Critical |

---

## 12. Checklist

- [ ] Create `deriva-network/src/cluster_gc.rs`
- [ ] Implement `ClusterGc` with coordinator election
- [ ] Implement 3-phase GC cycle (elect → collect → sweep)
- [ ] Implement `collect_local_roots` (recipes + hints + active ops)
- [ ] Implement `collect_global_roots` with fan-out and quorum
- [ ] Implement `sweep_local` with dry-run support
- [ ] Implement `execute_sweep` with broadcast to peers
- [ ] Add `GcCollectRoots` RPC to proto
- [ ] Add `GcSweep` RPC to proto
- [ ] Implement RPC handlers in internal service
- [ ] Add `delete()` and `stats()` to BlobStore
- [ ] Add `active_addrs()` to Executor for in-flight tracking
- [ ] Add min_blob_age protection for young blobs
- [ ] Add GC lock (Mutex) to prevent concurrent runs
- [ ] Spawn periodic GC task in server main
- [ ] Write coordinator election tests (4 tests)
- [ ] Write quorum requirement tests (3 tests)
- [ ] Write root set collection tests (3 tests)
- [ ] Write sweep tests (3 tests)
- [ ] Write integration tests (3 tests)
- [ ] Add metrics (10 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (5 alerts)
- [ ] Run benchmarks: root collection, sweep throughput, root merge
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
