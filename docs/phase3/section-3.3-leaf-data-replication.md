# §3.3 Leaf Data Replication

> **Status**: Blueprint  
> **Depends on**: §3.1 SWIM Gossip, §3.2 Recipe Replication  
> **Crate(s)**: `deriva-network`, `deriva-storage`, `deriva-server`  
> **Estimated effort**: 5–6 days  

---

## 1. Problem Statement

### 1.1 Current Limitation

Leaf data (raw blobs stored via `put_leaf`) lives on a single node. If that
node crashes, the data is permanently lost. Unlike recipes (which are tiny
metadata replicated to ALL nodes per §3.2), leaf data can be arbitrarily
large — full all-node replication is impractical.

### 1.2 What Leaf Data Is

```
put_leaf(bytes) → CAddr

  CAddr = SHA-256(bytes)
  Stored in BlobStore: 2-level hex sharding
    base_dir/ab/cd/abcdef0123456789...

  Size range: 1 byte to multi-GB
  Typical: 1KB – 100MB (documents, images, datasets)
```

### 1.3 Why Configurable Replication Factor?

Different deployments have different durability needs:

| Scenario | Replication Factor | Rationale |
|----------|-------------------|-----------|
| Dev/test | 1 | Speed, no durability needed |
| Production (small) | 2 | Survive 1 node failure |
| Production (standard) | 3 | Survive 2 concurrent failures |
| Archival | 5 | Maximum durability |

### 1.4 Design Goals

1. **Configurable replication factor** (default N=3)
2. **Consistent hashing** for deterministic placement
3. **Write path**: replicate to N nodes before ACK
4. **Read path**: read from any replica, repair on read
5. **Hinted handoff**: buffer writes for temporarily-down nodes
6. **No conflicts**: content-addressed = same CAddr = same bytes

### 1.5 Comparison with Recipe Replication (§3.2)

```
┌──────────────┬─────────────────────┬─────────────────────┐
│              │ Recipes (§3.2)      │ Leaf Data (§3.3)    │
├──────────────┼─────────────────────┼─────────────────────┤
│ Size         │ ~300 bytes          │ 1B – multi-GB       │
│ Replication  │ ALL nodes           │ N nodes (default 3) │
│ Placement    │ Everywhere          │ Consistent hashing  │
│ Write model  │ Sync all-node       │ Sync N-replica      │
│ Read model   │ Local lookup        │ Any replica         │
│ Consistency  │ Strong              │ Tunable             │
│ Anti-entropy │ Fingerprint-based   │ Merkle tree per vnode│
│ Conflicts    │ Impossible (CAddr)  │ Impossible (CAddr)  │
└──────────────┴─────────────────────┴─────────────────────┘
```

---

## 2. Design

### 2.1 Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                  Leaf Data Replication                        │
│                                                               │
│  Client                                                       │
│    │ put_leaf(bytes)                                          │
│    ▼                                                          │
│  ┌──────────┐                                                │
│  │ Node A   │  1. Compute CAddr = SHA-256(bytes)             │
│  │ (coord.) │  2. Hash ring lookup → [B, C, D] (N=3)        │
│  └────┬─────┘  3. If A ∈ replicas: store locally             │
│       │        4. Forward to remaining replicas               │
│       │                                                       │
│       ├──── StoreLeaf(CAddr, bytes) ────▶ Node B  ──▶ ACK   │
│       ├──── StoreLeaf(CAddr, bytes) ────▶ Node C  ──▶ ACK   │
│       └──── StoreLeaf(CAddr, bytes) ────▶ Node D  ──▶ ACK   │
│                                                               │
│  5. W ACKs received → return success to client                │
│     (W = write quorum, default = 2 for N=3)                  │
│                                                               │
│  Read path:                                                   │
│  ┌──────────┐                                                │
│  │ Any node │  1. Hash ring → [B, C, D]                      │
│  │          │  2. Read from closest/fastest replica           │
│  │          │  3. If not found: try next replica              │
│  └──────────┘  4. Read-repair if inconsistency detected      │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Consistent Hash Ring

We use a virtual-node consistent hash ring for placement decisions.
Each physical node owns multiple virtual nodes (vnodes) on the ring,
ensuring even distribution even with few physical nodes.

```
Hash Ring (256 vnodes per physical node):

  0x0000 ─────────────────────────────────── 0xFFFF
    │                                           │
    ▼                                           ▼
  ┌─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┐
  │ B-3 │ A-1 │ C-2 │ B-1 │ A-3 │ C-1 │ B-2 │ A-2 │ ...
  └─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┘

  Placement for CAddr X:
    1. Hash X to position on ring
    2. Walk clockwise, collect N distinct physical nodes
    3. Those N nodes are the replica set for X

  Example (N=3):
    hash(X) lands between B-1 and A-3
    Walk clockwise: A-3 → C-1 → B-2
    Physical nodes: A, C, B → replica set = [A, C, B]
```

### 2.3 Ring Data Structure

```rust
/// A point on the consistent hash ring.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct VNode {
    position: u64,       // hash position on ring
    node_id: NodeId,     // owning physical node
    vnode_index: u16,    // which vnode of this physical node
}

/// Consistent hash ring for data placement.
pub struct HashRing {
    vnodes: Vec<VNode>,              // sorted by position
    vnodes_per_node: u16,            // default 256
    replication_factor: u8,          // default 3
}
```

### 2.4 Quorum Configuration

```
N = replication factor (total replicas)
W = write quorum (ACKs needed for write success)
R = read quorum (reads needed for read success)

Constraint: W + R > N  (guarantees overlap → strong consistency)

Default: N=3, W=2, R=2
  - Write survives 1 slow/failed replica
  - Read always hits at least 1 up-to-date replica
  - W + R = 4 > 3 = N ✓

Alternative presets:
  ┌──────────┬───┬───┬───┬──────────────────────┐
  │ Preset   │ N │ W │ R │ Guarantee             │
  ├──────────┼───┼───┼───┼──────────────────────┤
  │ ONE      │ 3 │ 1 │ 1 │ Fastest, weakest      │
  │ QUORUM   │ 3 │ 2 │ 2 │ Strong consistency     │
  │ ALL      │ 3 │ 3 │ 1 │ Strongest write        │
  │ LOCAL    │ 3 │ 1 │ 1 │ Single-node fast path  │
  └──────────┴───┴───┴───┴──────────────────────┘
```

### 2.5 Hinted Handoff

When a target replica is temporarily unreachable during a write:

```
Normal write (N=3, targets=[B,C,D]):
  B: ACK ✓
  C: ACK ✓
  D: TIMEOUT ✗

  W=2 satisfied → return success to client

  But D is missing the data. Solution: hinted handoff.

  Coordinator stores a "hint" locally:
    Hint { target: D, addr: CAddr, data: bytes, created: now }

  Background task periodically:
    1. Check if D is alive (via SWIM gossip)
    2. If alive → send StoreLeaf to D
    3. If D ACKs → delete hint
    4. If hint age > 24h → discard (anti-entropy will repair)
```

```
┌──────────────────────────────────────────────────┐
│              Hinted Handoff Flow                  │
│                                                    │
│  Write time:                                       │
│    Coord ──▶ B (ACK) ✓                            │
│    Coord ──▶ C (ACK) ✓                            │
│    Coord ──▶ D (FAIL) ✗ → store hint locally      │
│    Return success (W=2 met)                        │
│                                                    │
│  Later (D comes back online):                      │
│    Coord detects D alive via SWIM                  │
│    Coord ──▶ D (StoreLeaf from hint) → ACK ✓     │
│    Coord deletes hint                              │
│                                                    │
│  Much later (hint expired, D still missing data):  │
│    Anti-entropy Merkle sync detects gap            │
│    D fetches missing data from B or C              │
└──────────────────────────────────────────────────┘
```

### 2.6 Wire Protocol Additions

```protobuf
// Added to DerivaInternal service (from §3.2)

rpc StoreLeaf(StoreLeafRequest) returns (StoreLeafResponse);
rpc FetchLeaf(FetchLeafRequest) returns (stream FetchLeafChunk);
rpc SyncLeafFingerprint(LeafFingerprintRequest)
    returns (LeafFingerprintResponse);
rpc FetchLeafAddrs(FetchLeafAddrsRequest)
    returns (FetchLeafAddrsResponse);

message StoreLeafRequest {
    bytes addr = 1;
    bytes data = 2;          // for small blobs (<4MB)
    bool is_hint = 3;        // true if this is a hinted handoff
}

message StoreLeafResponse {
    bool success = 1;
    string error = 2;
}

message FetchLeafRequest {
    bytes addr = 1;
}

message FetchLeafChunk {
    bytes data = 1;          // 64KB chunks (matches existing get)
    bool is_last = 2;
}

message LeafFingerprintRequest {
    uint32 vnode_range_start = 1;
    uint32 vnode_range_end = 2;
    bytes merkle_root = 3;
}

message LeafFingerprintResponse {
    bool in_sync = 1;
    bytes merkle_root = 2;
    repeated bytes differing_addrs = 3;
}

message FetchLeafAddrsRequest {
    uint32 vnode_range_start = 1;
    uint32 vnode_range_end = 2;
}

message FetchLeafAddrsResponse {
    repeated bytes addrs = 1;
}
```

---

## 3. Implementation

### 3.1 Consistent Hash Ring

Location: `crates/deriva-network/src/hash_ring.rs`

```rust
use sha2::{Sha256, Digest};
use std::collections::BTreeMap;
use crate::types::NodeId;
use deriva_core::CAddr;

/// Configuration for the hash ring.
#[derive(Debug, Clone)]
pub struct HashRingConfig {
    pub vnodes_per_node: u16,
    pub replication_factor: u8,
}

impl Default for HashRingConfig {
    fn default() -> Self {
        Self {
            vnodes_per_node: 256,
            replication_factor: 3,
        }
    }
}

/// Consistent hash ring for leaf data placement.
pub struct HashRing {
    ring: BTreeMap<u64, NodeId>,  // position → physical node
    config: HashRingConfig,
    node_count: usize,
}

impl HashRing {
    pub fn new(config: HashRingConfig) -> Self {
        Self {
            ring: BTreeMap::new(),
            config,
            node_count: 0,
        }
    }

    /// Add a node to the ring with vnodes_per_node virtual nodes.
    pub fn add_node(&mut self, node: &NodeId) {
        for i in 0..self.config.vnodes_per_node {
            let position = self.hash_vnode(node, i);
            self.ring.insert(position, node.clone());
        }
        self.node_count += 1;
    }

    /// Remove a node and all its virtual nodes from the ring.
    pub fn remove_node(&mut self, node: &NodeId) {
        for i in 0..self.config.vnodes_per_node {
            let position = self.hash_vnode(node, i);
            self.ring.remove(&position);
        }
        self.node_count = self.node_count.saturating_sub(1);
    }

    /// Get the N replica nodes for a given CAddr.
    ///
    /// Walks clockwise from the hash position, collecting
    /// distinct physical nodes until N are found.
    pub fn get_replicas(&self, addr: &CAddr) -> Vec<NodeId> {
        if self.ring.is_empty() {
            return vec![];
        }

        let position = self.hash_addr(addr);
        let n = self.config.replication_factor as usize;
        let mut replicas = Vec::with_capacity(n);
        let mut seen = std::collections::HashSet::new();

        // Walk clockwise from position
        for (_, node) in self.ring.range(position..) {
            if seen.insert(node.clone()) {
                replicas.push(node.clone());
                if replicas.len() >= n { return replicas; }
            }
        }
        // Wrap around
        for (_, node) in self.ring.range(..position) {
            if seen.insert(node.clone()) {
                replicas.push(node.clone());
                if replicas.len() >= n { return replicas; }
            }
        }

        replicas // may be < N if fewer physical nodes than N
    }

    /// Check if a specific node is a replica for the given addr.
    pub fn is_replica(&self, addr: &CAddr, node: &NodeId) -> bool {
        self.get_replicas(addr).contains(node)
    }

    /// Hash a CAddr to a ring position.
    fn hash_addr(&self, addr: &CAddr) -> u64 {
        let bytes = addr.as_bytes();
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let result = hasher.finalize();
        u64::from_be_bytes(result[..8].try_into().unwrap())
    }

    /// Hash a virtual node to a ring position.
    fn hash_vnode(&self, node: &NodeId, index: u16) -> u64 {
        let mut hasher = Sha256::new();
        hasher.update(node.addr.to_string().as_bytes());
        hasher.update(b":");
        hasher.update(index.to_be_bytes());
        let result = hasher.finalize();
        u64::from_be_bytes(result[..8].try_into().unwrap())
    }

    pub fn node_count(&self) -> usize { self.node_count }
    pub fn vnode_count(&self) -> usize { self.ring.len() }
}
```

### 3.2 Quorum Configuration

Location: `crates/deriva-network/src/quorum.rs`

```rust
/// Consistency level for read/write operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsistencyLevel {
    One,
    Quorum,
    All,
}

/// Quorum configuration for leaf data operations.
#[derive(Debug, Clone)]
pub struct QuorumConfig {
    pub replication_factor: u8,  // N
    pub write_consistency: ConsistencyLevel,
    pub read_consistency: ConsistencyLevel,
}

impl Default for QuorumConfig {
    fn default() -> Self {
        Self {
            replication_factor: 3,
            write_consistency: ConsistencyLevel::Quorum,
            read_consistency: ConsistencyLevel::Quorum,
        }
    }
}

impl QuorumConfig {
    /// Number of ACKs needed for a write to succeed.
    pub fn write_quorum(&self) -> u8 {
        match self.write_consistency {
            ConsistencyLevel::One => 1,
            ConsistencyLevel::Quorum => self.replication_factor / 2 + 1,
            ConsistencyLevel::All => self.replication_factor,
        }
    }

    /// Number of reads needed for a read to succeed.
    pub fn read_quorum(&self) -> u8 {
        match self.read_consistency {
            ConsistencyLevel::One => 1,
            ConsistencyLevel::Quorum => self.replication_factor / 2 + 1,
            ConsistencyLevel::All => self.replication_factor,
        }
    }

    /// Check if W + R > N (strong consistency guarantee).
    pub fn is_strongly_consistent(&self) -> bool {
        (self.write_quorum() + self.read_quorum())
            > self.replication_factor
    }
}
```

### 3.3 Leaf Replicator — Write Path

Location: `crates/deriva-network/src/leaf_replication.rs`

```rust
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tonic::transport::Channel;
use deriva_core::{CAddr, DerivaError};
use crate::hash_ring::HashRing;
use crate::quorum::{QuorumConfig, ConsistencyLevel};
use crate::types::NodeId;
use crate::swim::SwimRuntime;

/// Configuration for leaf replication.
#[derive(Debug, Clone)]
pub struct LeafReplicationConfig {
    pub rpc_timeout: Duration,
    pub max_retries: u32,
    pub hint_ttl: Duration,
    pub hint_replay_interval: Duration,
    pub quorum: QuorumConfig,
}

impl Default for LeafReplicationConfig {
    fn default() -> Self {
        Self {
            rpc_timeout: Duration::from_secs(30),
            max_retries: 2,
            hint_ttl: Duration::from_secs(86400), // 24h
            hint_replay_interval: Duration::from_secs(60),
            quorum: QuorumConfig::default(),
        }
    }
}

/// Result of a leaf write operation.
#[derive(Debug)]
pub struct LeafWriteResult {
    pub addr: CAddr,
    pub acks: usize,
    pub required: usize,
    pub hints_stored: usize,
    pub failures: Vec<(NodeId, String)>,
}

impl LeafWriteResult {
    pub fn is_success(&self) -> bool {
        self.acks >= self.required
    }
}

/// Handles leaf data replication across the cluster.
pub struct LeafReplicator {
    config: LeafReplicationConfig,
    ring: Arc<RwLock<HashRing>>,
    local_id: NodeId,
}

impl LeafReplicator {
    pub fn new(
        config: LeafReplicationConfig,
        ring: Arc<RwLock<HashRing>>,
        local_id: NodeId,
    ) -> Self {
        Self { config, ring, local_id }
    }

    /// Replicate leaf data to the appropriate replica nodes.
    ///
    /// 1. Determine replica set from hash ring
    /// 2. Store locally if this node is a replica
    /// 3. Send to remote replicas concurrently
    /// 4. Wait for W ACKs (write quorum)
    /// 5. Store hints for failed replicas
    pub async fn replicate_leaf(
        &self,
        addr: &CAddr,
        data: &[u8],
        blob_store: &dyn StorageBackend,
        swim: &SwimRuntime,
        hint_store: &HintStore,
    ) -> Result<LeafWriteResult, DerivaError> {
        let ring = self.ring.read().await;
        let replicas = ring.get_replicas(addr);
        drop(ring);

        let w = self.config.quorum.write_quorum() as usize;
        let mut acks = 0usize;
        let mut failures = Vec::new();
        let mut hints_stored = 0usize;

        // Separate local vs remote replicas
        let is_local_replica = replicas.iter()
            .any(|n| *n == self.local_id);
        let remote_replicas: Vec<_> = replicas.iter()
            .filter(|n| **n != self.local_id)
            .cloned()
            .collect();

        // Store locally if we're a replica
        if is_local_replica {
            blob_store.put(addr, data).await
                .map_err(|e| DerivaError::Storage(e.to_string()))?;
            acks += 1;
        }

        // Send to remote replicas concurrently
        if !remote_replicas.is_empty() {
            let mut join_set = tokio::task::JoinSet::new();
            let timeout = self.config.rpc_timeout;

            for replica_id in &remote_replicas {
                let grpc_addr = match swim.get_grpc_addr(replica_id).await {
                    Some(a) => a,
                    None => {
                        failures.push((replica_id.clone(), "no grpc addr".into()));
                        continue;
                    }
                };
                let addr = *addr;
                let data = data.to_vec();
                let replica_id = replica_id.clone();

                join_set.spawn(async move {
                    match Self::send_store_leaf(&grpc_addr, &addr, &data, timeout).await {
                        Ok(()) => (replica_id, Ok(())),
                        Err(e) => (replica_id, Err(e.to_string())),
                    }
                });
            }

            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok((_, Ok(()))) => acks += 1,
                    Ok((id, Err(e))) => failures.push((id, e)),
                    Err(e) => failures.push((
                        NodeId::new("0.0.0.0:0".parse().unwrap(), "unknown"),
                        e.to_string(),
                    )),
                }
            }
        }

        // Store hints for failed replicas
        for (failed_id, _) in &failures {
            if data.len() <= 4 * 1024 * 1024 { // only hint for blobs ≤4MB
                hint_store.store(Hint {
                    target: failed_id.clone(),
                    addr: *addr,
                    data: data.to_vec(),
                    created: std::time::Instant::now(),
                }).await;
                hints_stored += 1;
            }
        }

        let result = LeafWriteResult {
            addr: *addr,
            acks,
            required: w,
            hints_stored,
            failures,
        };

        if result.is_success() {
            Ok(result)
        } else {
            Err(DerivaError::Replication(format!(
                "write quorum not met: {}/{} acks (need {})",
                acks, replicas.len(), w
            )))
        }
    }

    /// Send StoreLeaf RPC to a single peer.
    async fn send_store_leaf(
        grpc_addr: &std::net::SocketAddr,
        addr: &CAddr,
        data: &[u8],
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
        let request = tonic::Request::new(StoreLeafRequest {
            addr: addr.as_bytes().to_vec(),
            data: data.to_vec(),
            is_hint: false,
        });

        let resp = tokio::time::timeout(timeout, client.store_leaf(request))
            .await
            .map_err(|_| DerivaError::Network("store_leaf timeout".into()))?
            .map_err(|e| DerivaError::Network(e.to_string()))?;

        if resp.get_ref().success {
            Ok(())
        } else {
            Err(DerivaError::Network(resp.get_ref().error.clone()))
        }
    }
}
```

### 3.4 Leaf Replicator — Read Path

```rust
impl LeafReplicator {
    /// Read leaf data from the replica set.
    ///
    /// 1. Determine replica set from hash ring
    /// 2. Try local first (if this node is a replica)
    /// 3. Try remote replicas in order (closest first)
    /// 4. Read-repair: if found on one but missing on another
    pub async fn read_leaf(
        &self,
        addr: &CAddr,
        blob_store: &dyn StorageBackend,
        swim: &SwimRuntime,
    ) -> Result<Vec<u8>, DerivaError> {
        let ring = self.ring.read().await;
        let replicas = ring.get_replicas(addr);
        drop(ring);

        // Try local first
        let is_local = replicas.iter().any(|n| *n == self.local_id);
        if is_local {
            if let Ok(data) = blob_store.get(addr).await {
                return Ok(data);
            }
        }

        // Try remote replicas
        let mut last_err = String::from("no replicas available");
        for replica_id in &replicas {
            if *replica_id == self.local_id { continue; }

            let grpc_addr = match swim.get_grpc_addr(replica_id).await {
                Some(a) => a,
                None => continue,
            };

            match Self::fetch_leaf_from_peer(&grpc_addr, addr, self.config.rpc_timeout).await {
                Ok(data) => {
                    // Read-repair: store locally if we're a replica but missing it
                    if is_local {
                        let _ = blob_store.put(addr, &data).await;
                    }
                    return Ok(data);
                }
                Err(e) => { last_err = e.to_string(); }
            }
        }

        Err(DerivaError::NotFound(format!(
            "leaf {} not found on any replica: {}", addr, last_err
        )))
    }

    /// Fetch leaf data from a remote peer via streaming RPC.
    async fn fetch_leaf_from_peer(
        grpc_addr: &std::net::SocketAddr,
        addr: &CAddr,
        timeout: Duration,
    ) -> Result<Vec<u8>, DerivaError> {
        let endpoint = format!("http://{}", grpc_addr);
        let channel = Channel::from_shared(endpoint)
            .map_err(|e| DerivaError::Network(e.to_string()))?
            .connect_timeout(timeout)
            .connect()
            .await
            .map_err(|e| DerivaError::Network(e.to_string()))?;

        let mut client = DerivaInternalClient::new(channel);
        let request = tonic::Request::new(FetchLeafRequest {
            addr: addr.as_bytes().to_vec(),
        });

        let mut stream = client.fetch_leaf(request)
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

        if data.is_empty() {
            Err(DerivaError::NotFound("empty response".into()))
        } else {
            Ok(data)
        }
    }
}
```


### 3.5 Hint Store

Location: `crates/deriva-network/src/hint_store.rs`

```rust
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use deriva_core::CAddr;
use crate::types::NodeId;

/// A hinted handoff entry for a failed replica write.
#[derive(Debug, Clone)]
pub struct Hint {
    pub target: NodeId,
    pub addr: CAddr,
    pub data: Vec<u8>,
    pub created: Instant,
}

/// In-memory store for hinted handoffs.
///
/// Hints are stored when a replica write fails. A background task
/// replays hints when the target node comes back online.
pub struct HintStore {
    hints: Mutex<VecDeque<Hint>>,
    max_hints: usize,
    ttl: Duration,
}

impl HintStore {
    pub fn new(max_hints: usize, ttl: Duration) -> Self {
        Self {
            hints: Mutex::new(VecDeque::new()),
            max_hints,
            ttl,
        }
    }

    /// Store a hint for later replay.
    pub async fn store(&self, hint: Hint) {
        let mut hints = self.hints.lock().await;
        if hints.len() >= self.max_hints {
            hints.pop_front(); // evict oldest
        }
        hints.push_back(hint);
    }

    /// Get all hints for a specific target node.
    pub async fn hints_for(&self, target: &NodeId) -> Vec<Hint> {
        let hints = self.hints.lock().await;
        hints.iter()
            .filter(|h| h.target == *target && h.created.elapsed() < self.ttl)
            .cloned()
            .collect()
    }

    /// Remove a specific hint after successful replay.
    pub async fn remove(&self, target: &NodeId, addr: &CAddr) {
        let mut hints = self.hints.lock().await;
        hints.retain(|h| !(h.target == *target && h.addr == *addr));
    }

    /// Purge expired hints.
    pub async fn purge_expired(&self) {
        let mut hints = self.hints.lock().await;
        hints.retain(|h| h.created.elapsed() < self.ttl);
    }

    pub async fn len(&self) -> usize {
        self.hints.lock().await.len()
    }
}

/// Background task that replays hints to recovered nodes.
pub async fn hint_replay_loop(
    hint_store: Arc<HintStore>,
    swim: Arc<SwimRuntime>,
    config: LeafReplicationConfig,
) {
    let mut interval = tokio::time::interval(config.hint_replay_interval);
    loop {
        interval.tick().await;
        hint_store.purge_expired().await;

        let alive_nodes = swim.alive_nodes().await;
        for node_id in &alive_nodes {
            let hints = hint_store.hints_for(node_id).await;
            if hints.is_empty() { continue; }

            let grpc_addr = match swim.get_grpc_addr(node_id).await {
                Some(a) => a,
                None => continue,
            };

            for hint in &hints {
                match LeafReplicator::send_store_leaf(
                    &grpc_addr, &hint.addr, &hint.data, config.rpc_timeout,
                ).await {
                    Ok(()) => {
                        hint_store.remove(node_id, &hint.addr).await;
                        tracing::info!(
                            target = %node_id,
                            addr = %hint.addr,
                            "hinted handoff replayed successfully"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            target = %node_id,
                            addr = %hint.addr,
                            error = %e,
                            "hinted handoff replay failed, will retry"
                        );
                        break; // stop trying this node, retry next interval
                    }
                }
            }
        }
    }
}
```

### 3.6 Updated put_leaf Handler

Location: `crates/deriva-server/src/service.rs`

```rust
async fn put_leaf(
    &self,
    request: Request<PutLeafRequest>,
) -> Result<Response<PutLeafResponse>, Status> {
    let data = &request.get_ref().data;
    let addr = CAddr::from_bytes(data);

    // Single-node mode: store locally only
    if self.state.swim.is_none() {
        self.state.blob_store.put(&addr, data).await
            .map_err(|e| Status::internal(e.to_string()))?;
        return Ok(Response::new(PutLeafResponse {
            addr: addr.as_bytes().to_vec(),
        }));
    }

    // Cluster mode: replicate to hash ring replicas
    let swim = self.state.swim.as_ref().unwrap();
    let replicator = &self.state.leaf_replicator;
    let hint_store = &self.state.hint_store;

    let result = replicator.replicate_leaf(
        &addr, data, &*self.state.blob_store, swim, hint_store,
    ).await.map_err(|e| Status::unavailable(e.to_string()))?;

    tracing::info!(
        addr = %addr,
        acks = result.acks,
        required = result.required,
        hints = result.hints_stored,
        "leaf replicated"
    );

    Ok(Response::new(PutLeafResponse {
        addr: addr.as_bytes().to_vec(),
    }))
}
```

### 3.7 Ring Membership Updates

When SWIM detects a node join/leave, the hash ring must be updated:

```rust
/// Callback from SWIM when membership changes.
pub async fn on_membership_change(
    ring: Arc<RwLock<HashRing>>,
    event: MembershipEvent,
) {
    let mut ring = ring.write().await;
    match event {
        MembershipEvent::Join(node_id) => {
            ring.add_node(&node_id);
            tracing::info!(node = %node_id, "added node to hash ring");
        }
        MembershipEvent::Leave(node_id) | MembershipEvent::Dead(node_id) => {
            ring.remove_node(&node_id);
            tracing::info!(node = %node_id, "removed node from hash ring");
        }
        _ => {}
    }
}
```

---

## 4. Data Flow Diagrams

### 4.1 Write Path — Happy Path (N=3, W=2)

```
  Client          Node A (coord)      Node B           Node C           Node D
    │                  │                 │                │                │
    │ put_leaf(bytes)  │                 │                │                │
    │─────────────────▶│                 │                │                │
    │                  │                 │                │                │
    │                  │ CAddr = SHA-256(bytes)           │                │
    │                  │ ring.get_replicas(CAddr) → [B, C, D]             │
    │                  │                 │                │                │
    │                  │ StoreLeaf       │                │                │
    │                  │────────────────▶│ store in       │                │
    │                  │ StoreLeaf       │ BlobStore      │                │
    │                  │─────────────────────────────────▶│ store in       │
    │                  │ StoreLeaf       │                │ BlobStore      │
    │                  │──────────────────────────────────────────────────▶│
    │                  │                 │                │                │
    │                  │           ACK   │                │                │
    │                  │◀────────────────│                │                │
    │                  │                          ACK    │                │
    │                  │◀─────────────────────────────────│                │
    │                  │  W=2 met ✓      │                │           ACK │
    │  PutLeafResp     │  (don't wait    │                │                │
    │◀─────────────────│   for D's ACK)  │                │                │
    │                  │◀──────────────────────────────────────────────────│
```

### 4.2 Write Path — Partial Failure with Hinted Handoff

```
  Client          Node A (coord)      Node B           Node C (down)
    │                  │                 │                │ ✗
    │ put_leaf(bytes)  │                 │                │
    │─────────────────▶│                 │                │
    │                  │ replicas → [A, B, C]             │
    │                  │                 │                │
    │                  │ store locally ✓ │                │
    │                  │ (A is replica)  │                │
    │                  │                 │                │
    │                  │ StoreLeaf       │                │
    │                  │────────────────▶│ ACK ✓         │
    │                  │◀────────────────│                │
    │                  │                 │                │
    │                  │ StoreLeaf       │                │
    │                  │─────────────────────────────────▶│ TIMEOUT ✗
    │                  │                 │                │
    │                  │ acks=2, W=2 ✓   │                │
    │                  │ store hint for C │                │
    │  PutLeafResp     │                 │                │
    │◀─────────────────│                 │                │
    │                  │                 │                │
    │  ... later, C comes back ...       │                │
    │                  │                 │                │
    │                  │ hint replay:    │                │
    │                  │ StoreLeaf(hint) │                │
    │                  │─────────────────────────────────▶│ ACK ✓
    │                  │ delete hint     │                │
```

### 4.3 Read Path — Local Hit

```
  Client          Node B (has replica)
    │                  │
    │ get(CAddr)       │
    │─────────────────▶│
    │                  │ ring.get_replicas(CAddr) → [B, C, D]
    │                  │ B is local replica ✓
    │                  │ blob_store.get(CAddr) → data ✓
    │  stream data     │
    │◀─────────────────│
    │  (64KB chunks)   │
```

### 4.4 Read Path — Remote Fetch + Read Repair

```
  Client          Node A (not replica)   Node B (replica)   Node C (replica, missing)
    │                  │                    │                   │
    │ get(CAddr)       │                    │                   │
    │─────────────────▶│                    │                   │
    │                  │ replicas → [B, C, D]                   │
    │                  │ A not in replicas   │                   │
    │                  │                    │                   │
    │                  │ FetchLeaf(CAddr)   │                   │
    │                  │───────────────────▶│                   │
    │                  │ stream chunks      │                   │
    │                  │◀───────────────────│                   │
    │  stream data     │                    │                   │
    │◀─────────────────│                    │                   │
    │                  │                    │                   │
    │  (background: read-repair C)          │                   │
    │                  │ StoreLeaf(CAddr)   │                   │
    │                  │──────────────────────────────────────▶│
    │                  │                    │              ACK  │
```

### 4.5 Node Join — Data Rebalancing

```
  Before: 3 nodes [A, B, C], N=3 → every leaf on all 3 nodes

  Node D joins:
  ┌──────────────────────────────────────────────────────┐
  │ Ring rebalances: some CAddrs now map to D            │
  │                                                        │
  │ CAddr X: was [A, B, C] → now [A, B, D]              │
  │   D needs X, C no longer needs X                      │
  │                                                        │
  │ Anti-entropy detects D is missing X:                  │
  │   D syncs with A → fetches X                          │
  │                                                        │
  │ C still has X (no active deletion)                    │
  │   GC can reclaim later if storage pressure            │
  └──────────────────────────────────────────────────────┘
```

---

## 5. Test Specification

### 5.1 Unit Tests — Hash Ring

```rust
#[test]
fn test_ring_add_single_node() {
    let mut ring = HashRing::new(HashRingConfig {
        vnodes_per_node: 256,
        replication_factor: 3,
    });
    let node_a = test_node("A");
    ring.add_node(&node_a);

    assert_eq!(ring.node_count(), 1);
    assert_eq!(ring.vnode_count(), 256);

    // With 1 node, all addrs map to that node
    let replicas = ring.get_replicas(&test_addr("x"));
    assert_eq!(replicas.len(), 1);
    assert_eq!(replicas[0], node_a);
}

#[test]
fn test_ring_3_nodes_returns_3_replicas() {
    let mut ring = HashRing::new(HashRingConfig {
        vnodes_per_node: 256,
        replication_factor: 3,
    });
    ring.add_node(&test_node("A"));
    ring.add_node(&test_node("B"));
    ring.add_node(&test_node("C"));

    let replicas = ring.get_replicas(&test_addr("x"));
    assert_eq!(replicas.len(), 3);

    // All 3 are distinct physical nodes
    let unique: std::collections::HashSet<_> = replicas.iter().collect();
    assert_eq!(unique.len(), 3);
}

#[test]
fn test_ring_deterministic() {
    let mut ring = HashRing::new(HashRingConfig::default());
    ring.add_node(&test_node("A"));
    ring.add_node(&test_node("B"));
    ring.add_node(&test_node("C"));

    let addr = test_addr("deterministic");
    let r1 = ring.get_replicas(&addr);
    let r2 = ring.get_replicas(&addr);
    assert_eq!(r1, r2);
}

#[test]
fn test_ring_remove_node() {
    let mut ring = HashRing::new(HashRingConfig {
        vnodes_per_node: 256,
        replication_factor: 3,
    });
    let a = test_node("A");
    let b = test_node("B");
    let c = test_node("C");
    ring.add_node(&a);
    ring.add_node(&b);
    ring.add_node(&c);

    ring.remove_node(&c);
    assert_eq!(ring.node_count(), 2);

    let replicas = ring.get_replicas(&test_addr("x"));
    assert_eq!(replicas.len(), 2); // only 2 physical nodes left
    assert!(!replicas.contains(&c));
}

#[test]
fn test_ring_distribution_evenness() {
    let mut ring = HashRing::new(HashRingConfig {
        vnodes_per_node: 256,
        replication_factor: 1,
    });
    ring.add_node(&test_node("A"));
    ring.add_node(&test_node("B"));
    ring.add_node(&test_node("C"));

    // Generate 10000 random addrs, count primary assignments
    let mut counts = std::collections::HashMap::new();
    for i in 0..10000 {
        let addr = test_addr(&format!("key_{}", i));
        let primary = &ring.get_replicas(&addr)[0];
        *counts.entry(primary.clone()).or_insert(0) += 1;
    }

    // Each node should get ~3333 ± 500 (15% tolerance)
    for (_, count) in &counts {
        assert!(*count > 2500, "uneven: {}", count);
        assert!(*count < 4500, "uneven: {}", count);
    }
}

#[test]
fn test_ring_minimal_disruption_on_add() {
    let mut ring = HashRing::new(HashRingConfig {
        vnodes_per_node: 256,
        replication_factor: 1,
    });
    ring.add_node(&test_node("A"));
    ring.add_node(&test_node("B"));
    ring.add_node(&test_node("C"));

    // Record assignments before adding D
    let mut before = std::collections::HashMap::new();
    for i in 0..1000 {
        let addr = test_addr(&format!("key_{}", i));
        before.insert(addr, ring.get_replicas(&addr)[0].clone());
    }

    ring.add_node(&test_node("D"));

    // Count how many keys moved
    let mut moved = 0;
    for i in 0..1000 {
        let addr = test_addr(&format!("key_{}", i));
        let new_primary = &ring.get_replicas(&addr)[0];
        if *new_primary != before[&addr] { moved += 1; }
    }

    // Expect ~25% moved (1/4 of keys go to new node)
    assert!(moved < 400, "too many keys moved: {}", moved);
}
```

### 5.2 Unit Tests — Quorum Config

```rust
#[test]
fn test_quorum_defaults() {
    let q = QuorumConfig::default();
    assert_eq!(q.write_quorum(), 2); // N=3, QUORUM → 3/2+1=2
    assert_eq!(q.read_quorum(), 2);
    assert!(q.is_strongly_consistent()); // 2+2=4 > 3
}

#[test]
fn test_quorum_one() {
    let q = QuorumConfig {
        replication_factor: 3,
        write_consistency: ConsistencyLevel::One,
        read_consistency: ConsistencyLevel::One,
    };
    assert_eq!(q.write_quorum(), 1);
    assert_eq!(q.read_quorum(), 1);
    assert!(!q.is_strongly_consistent()); // 1+1=2 ≤ 3
}

#[test]
fn test_quorum_all() {
    let q = QuorumConfig {
        replication_factor: 3,
        write_consistency: ConsistencyLevel::All,
        read_consistency: ConsistencyLevel::One,
    };
    assert_eq!(q.write_quorum(), 3);
    assert_eq!(q.read_quorum(), 1);
    assert!(q.is_strongly_consistent()); // 3+1=4 > 3
}
```

### 5.3 Unit Tests — Hint Store

```rust
#[tokio::test]
async fn test_hint_store_and_retrieve() {
    let store = HintStore::new(100, Duration::from_secs(3600));
    let target = test_node("B");
    let addr = test_addr("x");

    store.store(Hint {
        target: target.clone(),
        addr,
        data: b"hello".to_vec(),
        created: Instant::now(),
    }).await;

    let hints = store.hints_for(&target).await;
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].data, b"hello");
}

#[tokio::test]
async fn test_hint_store_remove() {
    let store = HintStore::new(100, Duration::from_secs(3600));
    let target = test_node("B");
    let addr = test_addr("x");

    store.store(Hint {
        target: target.clone(), addr, data: vec![1], created: Instant::now(),
    }).await;

    store.remove(&target, &addr).await;
    assert_eq!(store.hints_for(&target).await.len(), 0);
}

#[tokio::test]
async fn test_hint_store_evicts_oldest_when_full() {
    let store = HintStore::new(2, Duration::from_secs(3600));
    let target = test_node("B");

    for i in 0..3u8 {
        store.store(Hint {
            target: target.clone(),
            addr: test_addr(&format!("x{}", i)),
            data: vec![i],
            created: Instant::now(),
        }).await;
    }

    assert_eq!(store.len().await, 2);
    let hints = store.hints_for(&target).await;
    // Oldest (x0) was evicted
    assert!(hints.iter().all(|h| h.data != vec![0u8]));
}

#[tokio::test]
async fn test_hint_store_purge_expired() {
    let store = HintStore::new(100, Duration::from_millis(50));
    let target = test_node("B");

    store.store(Hint {
        target: target.clone(),
        addr: test_addr("x"),
        data: vec![1],
        created: Instant::now(),
    }).await;

    tokio::time::sleep(Duration::from_millis(100)).await;
    store.purge_expired().await;
    assert_eq!(store.len().await, 0);
}
```

### 5.4 Integration Tests

```rust
#[tokio::test]
async fn test_leaf_replicated_to_correct_nodes() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (mut client_b, _sb) = start_cluster_node(1).await;
    let (mut client_c, _sc) = start_cluster_node(2).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    let addr = put_leaf(&mut client_a, b"replicated data").await;

    // At least W=2 nodes should have the data
    let mut found_count = 0;
    for client in [&mut client_a, &mut client_b, &mut client_c] {
        if get_leaf(client, &addr).await.is_ok() {
            found_count += 1;
        }
    }
    assert!(found_count >= 2, "expected >=2 replicas, found {}", found_count);
}

#[tokio::test]
async fn test_leaf_readable_after_one_node_dies() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (_, _sb) = start_cluster_node(1).await;
    let (mut client_c, server_c) = start_cluster_node(2).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    let addr = put_leaf(&mut client_a, b"durable data").await;

    // Kill node C
    drop(server_c);
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Data should still be readable from A
    let data = get_leaf(&mut client_a, &addr).await.unwrap();
    assert_eq!(data, b"durable data");
}

#[tokio::test]
async fn test_hinted_handoff_delivers_after_recovery() {
    let (mut client_a, _sa) = start_cluster_node(0).await;
    let (_, _sb) = start_cluster_node(1).await;
    let (mut client_c, server_c) = start_cluster_node(2).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Kill C before write
    drop(server_c);
    tokio::time::sleep(Duration::from_secs(1)).await;

    let addr = put_leaf(&mut client_a, b"hinted data").await;

    // Restart C
    let (mut client_c2, _sc2) = restart_cluster_node(2).await;
    tokio::time::sleep(Duration::from_secs(65)).await; // hint replay interval

    // C should now have the data via hinted handoff
    let data = get_leaf(&mut client_c2, &addr).await.unwrap();
    assert_eq!(data, b"hinted data");
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Cluster has fewer nodes than N | Replicate to all available nodes | Degrade gracefully |
| 2 | Single-node cluster | Store locally only, no replication | No peers |
| 3 | All replicas down during write | Return error, no hints (nobody to hint to) | Client retries |
| 4 | Large blob (>4MB) during hint | Skip hinting, rely on anti-entropy | Hints are in-memory, avoid OOM |
| 5 | Node joins mid-write | Not included in this write's replica set | Next write uses updated ring |
| 6 | Node leaves mid-write | Timeout + hint for that node | Hinted handoff covers it |
| 7 | Duplicate put_leaf (same bytes) | Idempotent — same CAddr, same data | Content-addressed dedup |
| 8 | Read from non-replica node | Forward to replica via FetchLeaf | Any node can serve reads |
| 9 | All replicas lost permanently | Data is lost (no recovery) | Increase N for critical data |
| 10 | Hash ring rebalance during read | May hit old or new replica, both valid | Consistency maintained |
| 11 | Hint store full | Evict oldest hint | Bounded memory usage |
| 12 | Hint replay to still-dead node | Fails, hint retained for next attempt | Retry until TTL expires |
| 13 | Concurrent writes of same CAddr | Both succeed, same data stored | Content-addressed = no conflict |
| 14 | Zero-byte leaf | Valid CAddr, replicated normally | Edge case but valid |

### 6.1 Failure Mode Decision Tree

```
put_leaf(bytes) called:
  │
  ├─ Compute CAddr, get replicas from ring
  │
  ├─ replicas.len() == 0?
  │   └─ YES → Error("no nodes in cluster")
  │
  ├─ Send to all replicas concurrently
  │
  ├─ acks >= W?
  │   ├─ YES → Success
  │   │   └─ Any failures? → Store hints for failed replicas
  │   │
  │   └─ NO → Wait for remaining (with timeout)
  │       ├─ acks >= W after timeout? → Success + hints
  │       └─ Still < W? → Error("write quorum not met")
  │           └─ Store hints for ALL failed replicas
  │               (data is on acks nodes, hints cover the rest)
```

---

## 7. Performance Analysis

### 7.1 Write Latency Model

```
put_leaf latency breakdown (N=3, W=2, 1MB blob):

  CAddr computation (SHA-256):     ~2ms
  Ring lookup:                     ~1μs
  Local store (if replica):        ~5ms (SSD)
  Network transfer (1MB, LAN):     ~8ms
  Remote store:                    ~5ms
  Total (parallel remotes):        ~15ms end-to-end

  Scaling with blob size:
  ┌───────────┬──────────┬──────────┬──────────┐
  │ Blob Size │ SHA-256  │ Network  │ Total    │
  ├───────────┼──────────┼──────────┼──────────┤
  │ 1 KB      │ <1ms     │ <1ms     │ ~6ms     │
  │ 1 MB      │ ~2ms     │ ~8ms     │ ~15ms    │
  │ 100 MB    │ ~200ms   │ ~800ms   │ ~1.2s    │
  │ 1 GB      │ ~2s      │ ~8s      │ ~12s     │
  └───────────┴──────────┴──────────┴──────────┘
```

### 7.2 Read Latency Model

```
get(CAddr) latency:

  Local hit (blob in local BlobStore):
    Ring lookup: ~1μs
    Disk read:   ~2ms (SSD)
    Total:       ~2ms

  Remote fetch (not local replica):
    Ring lookup:      ~1μs
    gRPC connect:     ~1ms (cached channel)
    Stream transfer:  ~8ms (1MB)
    Total:            ~10ms

  Read-repair overhead:
    Background store: ~5ms (async, not on critical path)
```

### 7.3 Storage Overhead

```
Replication factor impact:
  ┌───┬──────────────┬──────────────┐
  │ N │ Raw Data     │ Total Stored │
  ├───┼──────────────┼──────────────┤
  │ 1 │ 100 GB       │ 100 GB       │
  │ 2 │ 100 GB       │ 200 GB       │
  │ 3 │ 100 GB       │ 300 GB       │
  │ 5 │ 100 GB       │ 500 GB       │
  └───┴──────────────┴──────────────┘

  Per-node storage (3 nodes, N=3):
    Each node stores ALL data = 100 GB per node

  Per-node storage (10 nodes, N=3):
    Each node stores ~30% of data = 30 GB per node
```

### 7.4 Benchmarking Plan

```rust
/// Benchmark: hash ring lookup latency vs node count
#[bench]
fn bench_ring_lookup(b: &mut Bencher) {
    // Setup: rings with 3, 10, 50, 100 nodes (256 vnodes each)
    // Measure: get_replicas() for random CAddr
    // Expected: <5μs regardless of node count (BTreeMap range scan)
}

/// Benchmark: write throughput vs blob size
#[bench]
fn bench_write_throughput(b: &mut Bencher) {
    // Setup: 3-node cluster
    // Measure: put_leaf/sec for 1KB, 1MB, 100MB blobs
    // Expected: >1000/s for 1KB, >100/s for 1MB
}

/// Benchmark: ring rebalance disruption
#[bench]
fn bench_ring_rebalance(b: &mut Bencher) {
    // Setup: 10-node ring, add 11th node
    // Measure: % of keys that change primary replica
    // Expected: ~9% (1/11 of keys move to new node)
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/hash_ring.rs` | **NEW** — consistent hash ring |
| `deriva-network/src/quorum.rs` | **NEW** — quorum config + consistency levels |
| `deriva-network/src/leaf_replication.rs` | **NEW** — LeafReplicator write/read paths |
| `deriva-network/src/hint_store.rs` | **NEW** — HintStore + replay loop |
| `deriva-network/src/lib.rs` | Add `pub mod hash_ring, quorum, leaf_replication, hint_store` |
| `proto/deriva_internal.proto` | Add StoreLeaf, FetchLeaf, SyncLeafFingerprint RPCs |
| `deriva-server/src/service.rs` | Update `put_leaf` + `get` with replication |
| `deriva-server/src/state.rs` | Add `leaf_replicator`, `hint_store`, `hash_ring` to ServerState |
| `deriva-network/tests/hash_ring.rs` | **NEW** — ring unit tests |
| `deriva-network/tests/leaf_replication.rs` | **NEW** — integration tests |

---

## 9. Dependency Changes

| Crate | Dependency | Version | Reason |
|-------|-----------|---------|--------|
| `deriva-network` | `sha2` | `0.10` | Already added in §3.2 |
| `deriva-network` | `tonic` | `0.11` | Already added in §3.2 |

No new dependencies — reuses sha2 and tonic from §3.2.

---

## 10. Design Rationale

### 10.1 Why Consistent Hashing Instead of Random Placement?

| Strategy | Lookup | Rebalance on join/leave | Deterministic |
|----------|--------|------------------------|---------------|
| Random | O(N) broadcast | None | No |
| Modulo hash | O(1) | Massive (all keys move) | Yes |
| Consistent hash | O(1) ring walk | Minimal (~1/N keys move) | Yes |
| Rendezvous hash | O(N) per lookup | Minimal | Yes |

Consistent hashing gives O(1) lookup with minimal disruption when nodes
join or leave. With 256 vnodes per node, distribution is even (±15%).

### 10.2 Why Virtual Nodes?

Without vnodes, a 3-node ring has only 3 points — highly uneven distribution.
With 256 vnodes per node, the ring has 768 points — much more even.

```
3 nodes, no vnodes:     Node A gets ~60%, B gets ~25%, C gets ~15%
3 nodes, 256 vnodes:    Node A gets ~34%, B gets ~33%, C gets ~33%
```

### 10.3 Why Quorum Reads/Writes Instead of All-Node?

Leaf data can be large. All-node writes would:
- Multiply network bandwidth by N
- Increase write latency (wait for slowest node)
- Waste storage on nodes that don't need the data

Quorum (W=2, R=2 for N=3) provides strong consistency with:
- Tolerance for 1 slow/failed node on writes
- Tolerance for 1 slow/failed node on reads
- W + R = 4 > 3 = N → guaranteed overlap

### 10.4 Why Hinted Handoff Instead of Immediate Retry?

Immediate retry blocks the client. Hinted handoff:
- Returns success to client as soon as W is met
- Asynchronously delivers data to failed replica
- Bounded memory (max hints, TTL expiry)
- Anti-entropy as final safety net

### 10.5 Why Content-Addressing Eliminates Conflicts

Same argument as §3.2 for recipes, but even more important for data:

```
Node A: put_leaf(bytes_X) → CAddr = SHA-256(bytes_X)
Node B: put_leaf(bytes_X) → CAddr = SHA-256(bytes_X)

Same bytes → same CAddr → same blob → no conflict.
Replication is purely additive. No last-writer-wins needed.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `leaf_write_total` | Counter | `result={success,quorum_fail}` | Total leaf writes |
| `leaf_write_latency_ms` | Histogram | `blob_size_bucket` | Write latency |
| `leaf_write_acks` | Histogram | — | ACKs received per write |
| `leaf_read_total` | Counter | `source={local,remote}` | Read source |
| `leaf_read_repair_total` | Counter | — | Read repairs triggered |
| `hint_store_size` | Gauge | — | Current hint count |
| `hint_replay_total` | Counter | `result={success,failure}` | Hint replays |
| `hash_ring_nodes` | Gauge | — | Physical nodes in ring |
| `hash_ring_vnodes` | Gauge | — | Total vnodes in ring |

### 11.2 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| Write quorum failures | `leaf_write_total{result=quorum_fail}` > 5/min | Critical |
| Hint store growing | `hint_store_size` > 1000 for 10min | Warning |
| Read repair rate high | `leaf_read_repair_total` > 100/min | Warning |
| Ring has < N nodes | `hash_ring_nodes` < replication_factor | Critical |

---

## 12. Checklist

- [ ] Create `deriva-network/src/hash_ring.rs`
- [ ] Implement `HashRing` with add/remove/get_replicas
- [ ] Create `deriva-network/src/quorum.rs`
- [ ] Implement `QuorumConfig` with consistency levels
- [ ] Create `deriva-network/src/leaf_replication.rs`
- [ ] Implement `LeafReplicator::replicate_leaf` (write path)
- [ ] Implement `LeafReplicator::read_leaf` (read path + read-repair)
- [ ] Create `deriva-network/src/hint_store.rs`
- [ ] Implement `HintStore` with store/retrieve/purge
- [ ] Implement `hint_replay_loop` background task
- [ ] Add StoreLeaf/FetchLeaf RPCs to proto
- [ ] Update `put_leaf` handler with replication
- [ ] Update `get` handler with replica-aware reads
- [ ] Implement `on_membership_change` ring updates
- [ ] Write hash ring unit tests (~6 tests)
- [ ] Write quorum config unit tests (~3 tests)
- [ ] Write hint store unit tests (~4 tests)
- [ ] Write integration tests (~3 tests)
- [ ] Run benchmarks: ring lookup, write throughput, rebalance
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
