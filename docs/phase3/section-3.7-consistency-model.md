# §3.7 Consistency Model

> **Status:** Not started
> **Depends on:** §3.2 (recipe replication), §3.3 (leaf data replication), §3.4 (cache placement), §3.6 (distributed get)
> **Crate(s):** `deriva-network`, `deriva-core`, `deriva-server`
> **Estimated effort:** 2–3 days

---

## 1. Problem Statement

Deriva is a distributed system with three distinct data tiers — recipes, leaf
data, and cached computed values — each with fundamentally different
characteristics. A one-size-fits-all consistency model would either sacrifice
performance (strong consistency everywhere) or correctness (eventual
consistency everywhere).

The challenge is to define and implement a consistency model that:

1. **Guarantees recipe consistency** — recipes are the source of truth for the
   computation DAG. A stale or missing recipe means incorrect computation.
   All nodes must agree on the recipe set.

2. **Provides tunable leaf consistency** — leaf data is user-supplied and
   replicated. Different workloads need different trade-offs between latency
   and consistency (e.g., analytics can tolerate stale reads, financial data
   cannot).

3. **Accepts eventual cache consistency** — cached computed values are
   derivable from recipes + leaves. Stale caches are suboptimal but never
   incorrect — recomputation always produces the correct result.

4. **Provides read-after-write guarantees** — a client that writes data should
   immediately be able to read it back, regardless of which node handles the
   read.

### Content-Addressing Simplifies Consistency

```
Key insight: Deriva is content-addressed.

  CAddr = SHA-256(content)

  This means:
  - Values are immutable (same addr = same content, always)
  - No write conflicts (two writes of same content = same addr)
  - No lost updates (you can't "overwrite" a content-addressed value)
  - Replication is idempotent (receiving the same value twice is harmless)

  What CAN be inconsistent:
  - Visibility: Node A has value X, Node B doesn't (yet)
  - Availability: Value X exists but the node you're asking doesn't have it
  - Staleness: Cache has old computed value, recipe has been updated
    (but old CAddr still maps to old value — new recipe = new CAddr)

  What CANNOT be inconsistent:
  - Value corruption: CAddr verification catches any bit-flip
  - Write conflicts: impossible by construction
  - Divergent values for same addr: impossible (content-addressed)
```

---

## 2. Design

### 2.1 Three-Tier Consistency Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Consistency Tiers                             │
├─────────────────┬──────────────────┬────────────────────────────┤
│   RECIPES       │   LEAF DATA      │   CACHED VALUES            │
│   (Strong)      │   (Tunable)      │   (Eventual)               │
├─────────────────┼──────────────────┼────────────────────────────┤
│ All-node sync   │ Quorum-based     │ Best-effort                │
│ replication     │ replication      │ No replication             │
│ (§3.2)          │ (§3.3)           │ (§3.4)                     │
│                 │                  │                            │
│ W = ALL         │ W = configurable │ W = 1 (local only)         │
│ R = 1           │ R = configurable │ R = 1 (local only)         │
│                 │                  │                            │
│ Consistency:    │ Consistency:     │ Consistency:               │
│ Linearizable    │ ONE / QUORUM /   │ Eventual                   │
│                 │ ALL              │ (recomputable)             │
│                 │                  │                            │
│ Anti-entropy:   │ Read-repair +    │ Invalidation               │
│ Fingerprint     │ hinted handoff   │ cascade (§2.6)             │
│ sync (30s)      │                  │                            │
└─────────────────┴──────────────────┴────────────────────────────┘
```

### 2.2 Consistency Levels

```rust
/// Consistency level for read and write operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsistencyLevel {
    /// Read/write from a single node. Fastest, weakest guarantee.
    /// Sufficient when: stale reads are acceptable, or data is immutable.
    One,

    /// Read/write from a quorum of replicas (⌊N/2⌋ + 1).
    /// Provides strong consistency when W + R > N.
    /// Default for leaf data operations.
    Quorum,

    /// Read/write from all replicas. Strongest guarantee, highest latency.
    /// Used for recipes (write path) and critical reads.
    All,
}

impl ConsistencyLevel {
    /// Number of nodes required for this level given replication factor N.
    pub fn required_nodes(&self, replication_factor: usize) -> usize {
        match self {
            ConsistencyLevel::One => 1,
            ConsistencyLevel::Quorum => replication_factor / 2 + 1,
            ConsistencyLevel::All => replication_factor,
        }
    }

    /// Check if W + R > N (strong consistency condition).
    pub fn is_strongly_consistent(
        write_level: ConsistencyLevel,
        read_level: ConsistencyLevel,
        replication_factor: usize,
    ) -> bool {
        let w = write_level.required_nodes(replication_factor);
        let r = read_level.required_nodes(replication_factor);
        w + r > replication_factor
    }
}
```

### 2.3 Consistency Configuration

```rust
/// Per-operation consistency configuration.
#[derive(Debug, Clone)]
pub struct ConsistencyConfig {
    // --- Recipe tier ---
    /// Write consistency for recipes. Always ALL (enforced).
    pub recipe_write: ConsistencyLevel,      // ALL (immutable)
    /// Read consistency for recipes. Always ONE (all nodes have all recipes).
    pub recipe_read: ConsistencyLevel,        // ONE (sufficient)

    // --- Leaf tier ---
    /// Write consistency for leaf data.
    pub leaf_write: ConsistencyLevel,         // default: QUORUM
    /// Read consistency for leaf data.
    pub leaf_read: ConsistencyLevel,          // default: QUORUM
    /// Replication factor for leaf data.
    pub leaf_replication_factor: usize,       // default: 3

    // --- Cache tier ---
    /// Cache is always local-only (ONE/ONE), not configurable.
    /// Included for documentation completeness.
    pub cache_consistency: ConsistencyLevel,  // ONE (fixed)

    // --- Timeouts ---
    /// Maximum time to wait for write acknowledgments.
    pub write_timeout: Duration,              // default: 5s
    /// Maximum time to wait for read responses.
    pub read_timeout: Duration,               // default: 5s
}

impl Default for ConsistencyConfig {
    fn default() -> Self {
        Self {
            recipe_write: ConsistencyLevel::All,
            recipe_read: ConsistencyLevel::One,
            leaf_write: ConsistencyLevel::Quorum,
            leaf_read: ConsistencyLevel::Quorum,
            leaf_replication_factor: 3,
            cache_consistency: ConsistencyLevel::One,
            write_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_secs(5),
        }
    }
}

impl ConsistencyConfig {
    /// Returns true if the current config provides strong consistency for leaves.
    pub fn leaf_strongly_consistent(&self) -> bool {
        ConsistencyLevel::is_strongly_consistent(
            self.leaf_write,
            self.leaf_read,
            self.leaf_replication_factor,
        )
    }
}
```

### 2.4 Consistency Guarantees Summary

```
┌──────────┬───────────┬───────────┬──────────────────────────────┐
│ Tier     │ Write     │ Read      │ Guarantee                    │
├──────────┼───────────┼───────────┼──────────────────────────────┤
│ Recipe   │ ALL       │ ONE       │ Linearizable                 │
│          │           │           │ (all nodes have all recipes) │
├──────────┼───────────┼───────────┼──────────────────────────────┤
│ Leaf     │ QUORUM(2) │ QUORUM(2) │ Strong (W+R=4 > N=3)        │
│ (default)│           │           │ Read-after-write ✓           │
├──────────┼───────────┼───────────┼──────────────────────────────┤
│ Leaf     │ ONE       │ ONE       │ Eventual                     │
│ (fast)   │           │           │ May read stale               │
├──────────┼───────────┼───────────┼──────────────────────────────┤
│ Leaf     │ ALL       │ ONE       │ Strong                       │
│ (durable)│           │           │ Highest write latency        │
├──────────┼───────────┼───────────┼──────────────────────────────┤
│ Cache    │ ONE       │ ONE       │ Eventual (best-effort)       │
│          │ (local)   │ (local)   │ Recomputable on miss         │
└──────────┴───────────┴───────────┴──────────────────────────────┘
```

### 2.5 Read-After-Write Guarantee

```
Scenario: Client writes leaf L to Node A, then reads L from Node B.

With QUORUM write (W=2, N=3):
  Write to Node A → replicate to Node B and Node C
  Wait for 2 acks (A + one of {B, C})

With QUORUM read (R=2, N=3):
  Read from 2 of {A, B, C}
  At least 1 node has the latest write (pigeonhole: W+R=4 > N=3)

  ∴ Read-after-write guaranteed for QUORUM/QUORUM.

With ONE write, ONE read:
  Write to Node A only → read from Node B → B may not have it yet
  Read-after-write NOT guaranteed.

  Mitigation: client can read from the same node it wrote to.
  Or: use QUORUM for operations requiring read-after-write.
```

---

## 3. Implementation

### 3.1 Quorum Coordinator

```rust
// deriva-network/src/quorum.rs

use crate::swim::SwimRuntime;
use deriva_core::{CAddr, DerivaError};
use std::sync::Arc;
use tokio::time::timeout;

/// Coordinates quorum reads and writes across replicas.
pub struct QuorumCoordinator {
    swim: Arc<SwimRuntime>,
    config: ConsistencyConfig,
}

impl QuorumCoordinator {
    pub fn new(swim: Arc<SwimRuntime>, config: ConsistencyConfig) -> Self {
        Self { swim, config }
    }

    /// Write data to replicas with the configured consistency level.
    /// Returns Ok(ack_count) when enough replicas acknowledge.
    pub async fn write_leaf(
        &self,
        addr: &CAddr,
        data: &[u8],
        level: ConsistencyLevel,
    ) -> Result<usize, DerivaError> {
        let replicas = self.swim.ring().replicas_for(addr);
        let required = level.required_nodes(self.config.leaf_replication_factor)
            .min(replicas.len());

        if replicas.is_empty() {
            return Err(DerivaError::Internal("no replicas available".into()));
        }

        // Fan-out writes to all replicas concurrently
        let mut futures = Vec::new();
        for replica in &replicas {
            let fut = self.write_to_replica(replica, addr, data);
            futures.push(fut);
        }

        // Wait for required number of acks
        let mut acks = 0usize;
        let mut errors = Vec::new();
        let mut remaining = futures::stream::FuturesUnordered::from_iter(futures);

        let deadline = timeout(self.config.write_timeout, async {
            use futures::StreamExt;
            while let Some(result) = remaining.next().await {
                match result {
                    Ok(()) => {
                        acks += 1;
                        if acks >= required {
                            return Ok(acks);
                        }
                    }
                    Err(e) => errors.push(e),
                }
            }
            Err(DerivaError::Consistency(format!(
                "write quorum not met: got {}/{} acks, errors: {:?}",
                acks, required, errors
            )))
        }).await;

        match deadline {
            Ok(result) => result,
            Err(_) => Err(DerivaError::Timeout(format!(
                "write timeout: got {}/{} acks", acks, required
            ))),
        }
    }

    /// Read data from replicas with the configured consistency level.
    /// Returns the first successful response once quorum is met.
    pub async fn read_leaf(
        &self,
        addr: &CAddr,
        level: ConsistencyLevel,
    ) -> Result<Vec<u8>, DerivaError> {
        let replicas = self.swim.ring().replicas_for(addr);
        let required = level.required_nodes(self.config.leaf_replication_factor)
            .min(replicas.len());

        if replicas.is_empty() {
            return Err(DerivaError::NotFound("no replicas for addr".into()));
        }

        // Fan-out reads to all replicas concurrently
        let mut futures = Vec::new();
        for replica in &replicas {
            let fut = self.read_from_replica(replica, addr);
            futures.push(fut);
        }

        let mut successes: Vec<Vec<u8>> = Vec::new();
        let mut errors = Vec::new();
        let mut remaining = futures::stream::FuturesUnordered::from_iter(futures);

        let deadline = timeout(self.config.read_timeout, async {
            use futures::StreamExt;
            while let Some(result) = remaining.next().await {
                match result {
                    Ok(data) => {
                        successes.push(data);
                        if successes.len() >= required {
                            // All responses should be identical (content-addressed)
                            // Return the first one
                            return Ok(successes.into_iter().next().unwrap());
                        }
                    }
                    Err(e) => errors.push(e),
                }
            }
            Err(DerivaError::Consistency(format!(
                "read quorum not met: got {}/{} responses",
                successes.len(), required
            )))
        }).await;

        match deadline {
            Ok(result) => result,
            Err(_) => Err(DerivaError::Timeout(format!(
                "read timeout: got {}/{} responses", successes.len(), required
            ))),
        }
    }

    /// Write to a single replica via internal RPC.
    async fn write_to_replica(
        &self,
        replica: &NodeId,
        addr: &CAddr,
        data: &[u8],
    ) -> Result<(), DerivaError> {
        if *replica == self.swim.local_id() {
            // Local write — direct store access
            return Ok(()); // caller handles local write
        }
        let channel = self.swim.get_channel(replica).await?;
        let mut client = DerivaInternalClient::new(channel);
        let request = tonic::Request::new(ReplicateLeafRequest {
            addr: addr.as_bytes().to_vec(),
            data: data.to_vec(),
        });
        client.replicate_leaf(request).await
            .map_err(|e| DerivaError::Network(format!("replica write failed: {}", e)))?;
        Ok(())
    }

    /// Read from a single replica via internal RPC.
    async fn read_from_replica(
        &self,
        replica: &NodeId,
        addr: &CAddr,
    ) -> Result<Vec<u8>, DerivaError> {
        if *replica == self.swim.local_id() {
            return Err(DerivaError::Internal("use local read path".into()));
        }
        let channel = self.swim.get_channel(replica).await?;
        let mut client = DerivaInternalClient::new(channel);
        let request = tonic::Request::new(FetchValueRequest {
            addr: addr.as_bytes().to_vec(),
            hop_count: 0,
            request_id: String::new(),
        });
        let response = client.fetch_value(request).await
            .map_err(|e| DerivaError::Network(format!("replica read failed: {}", e)))?;

        // Collect stream
        let mut data = Vec::new();
        let mut stream = response.into_inner();
        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| DerivaError::Network(e.to_string()))?;
            if chunk.is_last && chunk.data.is_empty() {
                break;
            }
            data.extend_from_slice(&chunk.data);
        }
        Ok(data)
    }
}
```

### 3.2 Consistency-Aware Put Leaf

```rust
// Updated put_leaf in deriva-server/src/service.rs

async fn put_leaf(
    &self,
    request: Request<PutLeafRequest>,
) -> Result<Response<PutLeafResponse>, Status> {
    let req = request.into_inner();
    let data = &req.data;

    // Compute content address
    let addr = CAddr::hash(data);

    // Store locally
    self.state.leaf_store.put(&addr, data)
        .map_err(|e| Status::internal(e.to_string()))?;

    // Replicate with configured consistency
    if let Some(quorum) = &self.state.quorum_coordinator {
        let level = self.state.consistency_config.leaf_write;
        let ack_count = quorum.write_leaf(&addr, data, level).await
            .map_err(|e| Status::internal(format!("replication failed: {}", e)))?;

        tracing::info!(
            addr = %addr,
            level = ?level,
            acks = ack_count,
            "leaf written with quorum"
        );
    }

    Ok(Response::new(PutLeafResponse {
        addr: addr.as_bytes().to_vec(),
    }))
}
```

### 3.3 Consistency-Aware Get (Leaf Path)

```rust
// Integration with §3.6 DistributedGetResolver — leaf read path

/// Read a leaf with the configured consistency level.
async fn read_leaf_consistent(
    &self,
    addr: &CAddr,
) -> Result<Option<Vec<u8>>, DerivaError> {
    // Always check local first
    if let Some(data) = self.local_leaf_store.get(addr)? {
        match self.config.leaf_read {
            ConsistencyLevel::One => {
                // Local hit is sufficient for ONE
                return Ok(Some(data));
            }
            ConsistencyLevel::Quorum | ConsistencyLevel::All => {
                // Need to verify with quorum — but content-addressed
                // means if we have it locally, it's correct.
                // Quorum read is only needed when local doesn't have it.
                return Ok(Some(data));
            }
        }
    }

    // Not local — need to fetch from replicas
    if let Some(quorum) = &self.quorum_coordinator {
        let level = self.config.leaf_read;
        match quorum.read_leaf(addr, level).await {
            Ok(data) => {
                // Read-repair: store locally for future reads
                self.local_leaf_store.put(addr, &data)?;
                Ok(Some(data))
            }
            Err(DerivaError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    } else {
        Ok(None) // single-node mode, not found
    }
}
```

### 3.4 Consistency Verification

```rust
// deriva-network/src/consistency.rs

/// Verifies consistency guarantees are met for the current configuration.
pub struct ConsistencyVerifier {
    config: ConsistencyConfig,
}

impl ConsistencyVerifier {
    pub fn new(config: ConsistencyConfig) -> Self {
        Self { config }
    }

    /// Validate that the configuration provides the expected guarantees.
    pub fn validate(&self) -> Vec<ConsistencyWarning> {
        let mut warnings = Vec::new();

        // Recipe tier: must be ALL/ONE
        if self.config.recipe_write != ConsistencyLevel::All {
            warnings.push(ConsistencyWarning::RecipeNotStrong {
                actual: self.config.recipe_write,
            });
        }

        // Leaf tier: check W+R>N
        if !self.config.leaf_strongly_consistent() {
            warnings.push(ConsistencyWarning::LeafNotStrong {
                write: self.config.leaf_write,
                read: self.config.leaf_read,
                n: self.config.leaf_replication_factor,
            });
        }

        // Replication factor sanity
        if self.config.leaf_replication_factor < 2 {
            warnings.push(ConsistencyWarning::LowReplicationFactor {
                n: self.config.leaf_replication_factor,
            });
        }

        warnings
    }

    /// Check if a specific operation meets its consistency requirement.
    pub fn check_write_quorum(
        &self,
        acks: usize,
        level: ConsistencyLevel,
    ) -> bool {
        acks >= level.required_nodes(self.config.leaf_replication_factor)
    }
}

#[derive(Debug)]
pub enum ConsistencyWarning {
    RecipeNotStrong { actual: ConsistencyLevel },
    LeafNotStrong {
        write: ConsistencyLevel,
        read: ConsistencyLevel,
        n: usize,
    },
    LowReplicationFactor { n: usize },
}

impl std::fmt::Display for ConsistencyWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RecipeNotStrong { actual } => write!(
                f, "Recipe write consistency is {:?}, expected ALL", actual
            ),
            Self::LeafNotStrong { write, read, n } => write!(
                f, "Leaf W({:?})+R({:?}) <= N({}): not strongly consistent",
                write, read, n
            ),
            Self::LowReplicationFactor { n } => write!(
                f, "Replication factor {} < 2: no redundancy", n
            ),
        }
    }
}
```

### 3.5 Proto Changes

```protobuf
// Added to proto/deriva.proto (public API)

message PutLeafRequest {
    bytes data = 1;
    // NEW: optional consistency level override
    ConsistencyLevel consistency = 2;
}

message GetRequest {
    bytes addr = 1;
    // NEW: optional consistency level override
    ConsistencyLevel consistency = 2;
}

enum ConsistencyLevel {
    DEFAULT = 0;   // Use server-configured default
    ONE = 1;
    QUORUM = 2;
    ALL = 3;
}
```


---

## 4. Data Flow Diagrams

### 4.1 Recipe Write — Strong Consistency (ALL)

```
Client              Node A              Node B              Node C
  │                   │                   │                   │
  │── PutRecipe(r) ──►│                   │                   │
  │                   │ store locally     │                   │
  │                   │                   │                   │
  │                   │── Replicate(r) ──►│                   │
  │                   │── Replicate(r) ──────────────────────►│
  │                   │                   │                   │
  │                   │                   │ store + verify    │
  │                   │◄── ACK ──────────│ CAddr             │
  │                   │                   │                   │ store + verify
  │                   │◄── ACK ──────────────────────────────│ CAddr
  │                   │                   │                   │
  │                   │ acks=3 (ALL met)  │                   │
  │◄── OK(addr) ─────│                   │                   │

  Guarantee: After OK, all nodes have recipe r.
  Any subsequent read on any node returns r.
```

### 4.2 Leaf Write — Quorum Consistency (W=2, N=3)

```
Client              Node A              Node B              Node C
  │                   │                   │                   │
  │── PutLeaf(data) ─►│                   │                   │
  │                   │ store locally     │                   │
  │                   │ (ack 1/2)        │                   │
  │                   │                   │                   │
  │                   │── Replicate ─────►│                   │
  │                   │── Replicate ──────────────────────────►│
  │                   │                   │                   │
  │                   │◄── ACK ──────────│ (ack 2/2 ✓)       │
  │                   │                   │                   │
  │◄── OK(addr) ─────│  QUORUM MET       │                   │
  │                   │                   │                   │
  │                   │                   │                   │ (ack arrives
  │                   │◄── ACK ──────────────────────────────│  later, ignored)

  Guarantee: After OK, at least 2 of 3 nodes have the leaf.
  A QUORUM read (R=2) will find it (W+R=4 > N=3).
```

### 4.3 Leaf Write — ONE Consistency (Fast Path)

```
Client              Node A              Node B              Node C
  │                   │                   │                   │
  │── PutLeaf(data,  ►│                   │                   │
  │   level=ONE)      │ store locally     │                   │
  │                   │ (ack 1/1 ✓)      │                   │
  │◄── OK(addr) ─────│                   │                   │
  │                   │                   │                   │
  │                   │── Replicate ─────►│ (async, best-effort)
  │                   │── Replicate ──────────────────────────►│
  │                   │                   │                   │

  Guarantee: After OK, only Node A has the leaf.
  A ONE read from Node A succeeds. From B or C: may fail.
  Replication happens asynchronously — eventually consistent.
```

### 4.4 Read-After-Write with QUORUM

```
Client writes to Node A, reads from Node B:

  Write (W=QUORUM=2):
    Node A: has data ✓
    Node B: has data ✓ (ack'd before write returned)
    Node C: may or may not have data

  Read (R=QUORUM=2):
    Query Node A: has data ✓
    Query Node B: has data ✓
    → 2 responses, quorum met → return data

  OR:
    Query Node B: has data ✓
    Query Node C: doesn't have data ✗
    Query Node A: has data ✓
    → 2 responses, quorum met → return data

  In all cases: at least one node in the read quorum
  overlaps with the write quorum → data found.

  Proof: W + R = 2 + 2 = 4 > N = 3
  ∴ read set ∩ write set ≠ ∅ (pigeonhole principle)
```

### 4.5 Cache Consistency — Eventual with Recomputation

```
Node A                              Node B
  │                                   │
  │ cache.put(addr, computed_value)   │
  │                                   │
  │ (bloom filter updated in ~10s)    │
  │                                   │
  │                                   │ get(addr)
  │                                   │ cache → miss
  │                                   │ bloom → Node A has it
  │                                   │
  │◄── FetchValue(addr) ─────────────│
  │ cache.get(addr) → Some            │
  │── stream value ──────────────────►│
  │                                   │ cache.put(addr, value)
  │                                   │

  If Node A evicts before Node B fetches:
  │                                   │
  │◄── FetchValue(addr) ─────────────│
  │ cache.get(addr) → None            │
  │── NotFound ──────────────────────►│
  │                                   │ fallback: recompute locally
  │                                   │ (always correct)

  Cache inconsistency is harmless — recomputation is the fallback.
```

### 4.6 Consistency During Node Failure

```
3-node cluster, N=3, W=QUORUM(2), R=QUORUM(2)

  Node C goes down:

  Write path:
    Write to A (local) + replicate to B → 2 acks → QUORUM met ✓
    C gets hinted handoff when it recovers (§3.3)

  Read path:
    Read from A + B → 2 responses → QUORUM met ✓
    C is unavailable but not needed

  ∴ System remains available for reads and writes with 1 node down.

  If 2 nodes go down (only A remains):
    Write: 1 ack < QUORUM(2) → write fails (or downgrade to ONE)
    Read: 1 response < QUORUM(2) → read fails (or downgrade to ONE)

  Trade-off: QUORUM tolerates ⌊(N-1)/2⌋ failures = 1 for N=3.
```

---

## 5. Test Specification

### 5.1 ConsistencyLevel Unit Tests

```rust
#[cfg(test)]
mod consistency_level_tests {
    use super::*;

    #[test]
    fn test_required_nodes_one() {
        assert_eq!(ConsistencyLevel::One.required_nodes(3), 1);
        assert_eq!(ConsistencyLevel::One.required_nodes(5), 1);
        assert_eq!(ConsistencyLevel::One.required_nodes(1), 1);
    }

    #[test]
    fn test_required_nodes_quorum() {
        assert_eq!(ConsistencyLevel::Quorum.required_nodes(3), 2); // 3/2+1
        assert_eq!(ConsistencyLevel::Quorum.required_nodes(5), 3); // 5/2+1
        assert_eq!(ConsistencyLevel::Quorum.required_nodes(1), 1); // 1/2+1
        assert_eq!(ConsistencyLevel::Quorum.required_nodes(4), 3); // 4/2+1
    }

    #[test]
    fn test_required_nodes_all() {
        assert_eq!(ConsistencyLevel::All.required_nodes(3), 3);
        assert_eq!(ConsistencyLevel::All.required_nodes(5), 5);
    }

    #[test]
    fn test_strong_consistency_quorum_quorum() {
        // W=2 + R=2 = 4 > N=3 → strong
        assert!(ConsistencyLevel::is_strongly_consistent(
            ConsistencyLevel::Quorum,
            ConsistencyLevel::Quorum,
            3,
        ));
    }

    #[test]
    fn test_not_strong_one_one() {
        // W=1 + R=1 = 2 ≤ N=3 → not strong
        assert!(!ConsistencyLevel::is_strongly_consistent(
            ConsistencyLevel::One,
            ConsistencyLevel::One,
            3,
        ));
    }

    #[test]
    fn test_strong_all_one() {
        // W=3 + R=1 = 4 > N=3 → strong
        assert!(ConsistencyLevel::is_strongly_consistent(
            ConsistencyLevel::All,
            ConsistencyLevel::One,
            3,
        ));
    }

    #[test]
    fn test_strong_one_all() {
        // W=1 + R=3 = 4 > N=3 → strong
        assert!(ConsistencyLevel::is_strongly_consistent(
            ConsistencyLevel::One,
            ConsistencyLevel::All,
            3,
        ));
    }

    #[test]
    fn test_edge_case_n_equals_1() {
        // W=1 + R=1 = 2 > N=1 → strong (trivially)
        assert!(ConsistencyLevel::is_strongly_consistent(
            ConsistencyLevel::One,
            ConsistencyLevel::One,
            1,
        ));
    }
}
```

### 5.2 ConsistencyConfig Tests

```rust
#[test]
fn test_default_config_is_strongly_consistent() {
    let config = ConsistencyConfig::default();
    assert!(config.leaf_strongly_consistent());
    assert_eq!(config.recipe_write, ConsistencyLevel::All);
    assert_eq!(config.recipe_read, ConsistencyLevel::One);
    assert_eq!(config.leaf_write, ConsistencyLevel::Quorum);
    assert_eq!(config.leaf_read, ConsistencyLevel::Quorum);
    assert_eq!(config.leaf_replication_factor, 3);
}

#[test]
fn test_one_one_not_strongly_consistent() {
    let config = ConsistencyConfig {
        leaf_write: ConsistencyLevel::One,
        leaf_read: ConsistencyLevel::One,
        ..Default::default()
    };
    assert!(!config.leaf_strongly_consistent());
}
```

### 5.3 ConsistencyVerifier Tests

```rust
#[test]
fn test_verifier_default_no_warnings() {
    let config = ConsistencyConfig::default();
    let verifier = ConsistencyVerifier::new(config);
    let warnings = verifier.validate();
    assert!(warnings.is_empty());
}

#[test]
fn test_verifier_warns_weak_recipe() {
    let config = ConsistencyConfig {
        recipe_write: ConsistencyLevel::One, // bad!
        ..Default::default()
    };
    let verifier = ConsistencyVerifier::new(config);
    let warnings = verifier.validate();
    assert!(warnings.iter().any(|w| matches!(w, ConsistencyWarning::RecipeNotStrong { .. })));
}

#[test]
fn test_verifier_warns_weak_leaf() {
    let config = ConsistencyConfig {
        leaf_write: ConsistencyLevel::One,
        leaf_read: ConsistencyLevel::One,
        ..Default::default()
    };
    let verifier = ConsistencyVerifier::new(config);
    let warnings = verifier.validate();
    assert!(warnings.iter().any(|w| matches!(w, ConsistencyWarning::LeafNotStrong { .. })));
}

#[test]
fn test_verifier_warns_low_replication() {
    let config = ConsistencyConfig {
        leaf_replication_factor: 1,
        ..Default::default()
    };
    let verifier = ConsistencyVerifier::new(config);
    let warnings = verifier.validate();
    assert!(warnings.iter().any(|w| matches!(w, ConsistencyWarning::LowReplicationFactor { .. })));
}

#[test]
fn test_check_write_quorum() {
    let config = ConsistencyConfig::default(); // N=3
    let verifier = ConsistencyVerifier::new(config);

    assert!(verifier.check_write_quorum(2, ConsistencyLevel::Quorum));  // 2 >= 2
    assert!(!verifier.check_write_quorum(1, ConsistencyLevel::Quorum)); // 1 < 2
    assert!(verifier.check_write_quorum(1, ConsistencyLevel::One));     // 1 >= 1
    assert!(verifier.check_write_quorum(3, ConsistencyLevel::All));     // 3 >= 3
    assert!(!verifier.check_write_quorum(2, ConsistencyLevel::All));    // 2 < 3
}
```

### 5.4 Quorum Coordinator Integration Tests

```rust
#[tokio::test]
async fn test_quorum_write_succeeds() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    let coordinator = &nodes[0].quorum_coordinator;
    let data = b"test data";
    let addr = CAddr::hash(data);

    let acks = coordinator.write_leaf(&addr, data, ConsistencyLevel::Quorum).await.unwrap();
    assert!(acks >= 2); // quorum for N=3
}

#[tokio::test]
async fn test_quorum_write_with_one_node_down() {
    let (nodes, mut handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Kill node 2
    drop(handles.remove(2));
    tokio::time::sleep(Duration::from_secs(5)).await;

    let coordinator = &nodes[0].quorum_coordinator;
    let data = b"test data";
    let addr = CAddr::hash(data);

    // QUORUM(2) should still succeed with 2 of 3 nodes
    let acks = coordinator.write_leaf(&addr, data, ConsistencyLevel::Quorum).await.unwrap();
    assert!(acks >= 2);
}

#[tokio::test]
async fn test_quorum_write_fails_with_two_nodes_down() {
    let (nodes, mut handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Kill nodes 1 and 2
    drop(handles.remove(2));
    drop(handles.remove(1));
    tokio::time::sleep(Duration::from_secs(5)).await;

    let coordinator = &nodes[0].quorum_coordinator;
    let data = b"test data";
    let addr = CAddr::hash(data);

    // QUORUM(2) fails — only 1 node available
    let result = coordinator.write_leaf(&addr, data, ConsistencyLevel::Quorum).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_read_after_write_quorum() {
    let (mut clients, _handles) = start_cluster_with_clients(3).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Write via node 0 with QUORUM
    let addr = put_leaf_with_consistency(&mut clients[0], b"hello", ConsistencyLevel::Quorum).await;

    // Read from node 1 with QUORUM — should find it
    let result = get_with_consistency(&mut clients[1], &addr, ConsistencyLevel::Quorum).await;
    assert_eq!(result, b"hello");
}

#[tokio::test]
async fn test_read_after_write_one_may_miss() {
    let (mut clients, _handles) = start_cluster_with_clients(3).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Write via node 0 with ONE (only local)
    let addr = put_leaf_with_consistency(&mut clients[0], b"hello", ConsistencyLevel::One).await;

    // Immediate read from node 2 with ONE — may not find it
    // (replication hasn't happened yet)
    let result = try_get_with_consistency(&mut clients[2], &addr, ConsistencyLevel::One).await;
    // This may or may not succeed depending on timing — that's the point of ONE consistency
    // We just verify it doesn't crash
    let _ = result;
}

#[tokio::test]
async fn test_all_write_all_read() {
    let (mut clients, _handles) = start_cluster_with_clients(3).await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    let addr = put_leaf_with_consistency(&mut clients[0], b"critical", ConsistencyLevel::All).await;

    // Read from any node with ONE — guaranteed to find it
    for client in &mut clients {
        let result = get_with_consistency(client, &addr, ConsistencyLevel::One).await;
        assert_eq!(result, b"critical");
    }
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Write with ALL, one node down | Write fails (cannot reach all) | ALL requires all nodes |
| 2 | Write with QUORUM, one node down | Write succeeds (2/3 acks) | Quorum tolerates 1 failure |
| 3 | Read with QUORUM, one node down | Read succeeds (2/3 responses) | Quorum tolerates 1 failure |
| 4 | Write timeout before quorum met | Return Timeout error | Bounded latency |
| 5 | Replica returns different data | Impossible (content-addressed) | CAddr = SHA-256(content) |
| 6 | Network partition (A | B,C) | A: QUORUM fails (alone). B,C: QUORUM succeeds | Partition-intolerant for minority |
| 7 | Stale bloom filter + cache eviction | FetchValue returns NotFound, fallback to compute | Correct but suboptimal |
| 8 | Recipe write partially fails | Retry or return error — no partial state | Recipes are idempotent |
| 9 | Client specifies invalid level | Map DEFAULT to server config | Graceful handling |
| 10 | N=1 cluster | All levels equivalent to ONE | Trivially consistent |
| 11 | Concurrent writes of same leaf | Both succeed (same CAddr, same content) | Content-addressing = idempotent |
| 12 | Read-repair during quorum read | Store locally if missing | Improves future reads |

### 6.1 Content-Addressing Eliminates Write Conflicts

```
Traditional databases:
  Client A writes key=X, value=1
  Client B writes key=X, value=2
  → Conflict! Which value wins? Need conflict resolution.

Deriva (content-addressed):
  Client A writes data=[1,2,3] → addr=SHA256([1,2,3]) = 0xABC
  Client B writes data=[4,5,6] → addr=SHA256([4,5,6]) = 0xDEF
  → Different data = different addresses. No conflict.

  Client A writes data=[1,2,3] → addr=0xABC
  Client B writes data=[1,2,3] → addr=0xABC (same!)
  → Same data = same address. Idempotent. No conflict.

  ∴ Deriva never has write conflicts. Replication is always safe.
  The only consistency concern is visibility (does the reader's
  node have the data yet?), not correctness (which value is right?).
```

### 6.2 Partition Behavior

```
Network partition: {A} | {B, C}

  Partition A (minority):
    - Recipes: A has all recipes (ALL replication) ✓
    - Leaf write QUORUM: A alone = 1 ack < 2 → FAILS
    - Leaf write ONE: A alone = 1 ack → succeeds (but not replicated)
    - Leaf read QUORUM: A alone = 1 response < 2 → FAILS
    - Leaf read ONE: A has local data → succeeds for local data
    - Cache: works normally (local only)

  Partition B,C (majority):
    - Recipes: B,C have all recipes ✓
    - Leaf write QUORUM: B+C = 2 acks → succeeds ✓
    - Leaf read QUORUM: B+C = 2 responses → succeeds ✓
    - Cache: works normally

  After partition heals:
    - Recipes: anti-entropy sync reconciles (§3.2)
    - Leaves: hinted handoff delivers missed writes (§3.3)
    - Caches: no reconciliation needed (recomputable)

  Deriva is CP-leaning for QUORUM operations (consistency over availability)
  and AP-leaning for ONE operations (availability over consistency).
```

---

## 7. Performance Analysis

### 7.1 Write Latency by Consistency Level

```
┌──────────────┬──────────────┬──────────────────────────────┐
│ Level        │ Latency      │ Determined by                │
├──────────────┼──────────────┼──────────────────────────────┤
│ ONE          │ ~1ms         │ Local disk write only        │
│ QUORUM (N=3) │ ~3ms         │ Local + fastest remote ack   │
│ ALL (N=3)    │ ~5ms         │ Local + slowest remote ack   │
└──────────────┴──────────────┴──────────────────────────────┘

  ONE: fire-and-forget replication (async)
  QUORUM: wait for 2nd ack (parallel fan-out, take 2nd fastest)
  ALL: wait for all acks (limited by slowest replica)

  For 1MB leaf data:
  ┌──────────────┬──────────────┐
  │ Level        │ Latency      │
  ├──────────────┼──────────────┤
  │ ONE          │ ~5ms         │
  │ QUORUM       │ ~12ms        │
  │ ALL          │ ~18ms        │
  └──────────────┴──────────────┘
```

### 7.2 Read Latency by Consistency Level

```
┌──────────────┬──────────────┬──────────────────────────────┐
│ Level        │ Latency      │ Determined by                │
├──────────────┼──────────────┼──────────────────────────────┤
│ ONE (local)  │ ~0.5ms       │ Local cache/leaf lookup      │
│ ONE (remote) │ ~3ms         │ Single RPC to any replica    │
│ QUORUM       │ ~4ms         │ Fan-out, wait for 2nd resp   │
│ ALL          │ ~6ms         │ Fan-out, wait for all        │
└──────────────┴──────────────┴──────────────────────────────┘

  Content-addressing optimization: if local has the data,
  QUORUM/ALL reads can short-circuit (local data is always correct).
  Only need remote reads when local doesn't have the data.
```

### 7.3 Availability vs Consistency Trade-off

```
  Availability (% of requests that succeed) vs node failures:

  N=3 cluster:
  ┌──────────────┬──────────┬──────────┬──────────┐
  │ Failures     │ ONE      │ QUORUM   │ ALL      │
  ├──────────────┼──────────┼──────────┼──────────┤
  │ 0            │ 100%     │ 100%     │ 100%     │
  │ 1            │ 100%     │ 100%     │ 0%       │
  │ 2            │ 33%*     │ 0%       │ 0%       │
  └──────────────┴──────────┴──────────┴──────────┘
  * 33% = requests that hit the surviving node

  N=5 cluster:
  ┌──────────────┬──────────┬──────────┬──────────┐
  │ Failures     │ ONE      │ QUORUM   │ ALL      │
  ├──────────────┼──────────┼──────────┼──────────┤
  │ 0            │ 100%     │ 100%     │ 100%     │
  │ 1            │ 100%     │ 100%     │ 0%       │
  │ 2            │ 100%     │ 100%     │ 0%       │
  │ 3            │ 40%*     │ 0%       │ 0%       │
  └──────────────┴──────────┴──────────┴──────────┘

  Recommendation: QUORUM/QUORUM for most workloads.
  ONE/ONE only for latency-critical, loss-tolerant workloads.
  ALL only for recipe writes (small, infrequent).
```

### 7.4 Benchmarking Plan

```rust
/// Benchmark: write latency by consistency level
#[bench]
fn bench_write_consistency_levels(b: &mut Bencher) {
    // 3-node cluster, 1KB leaf
    // Measure: put_leaf latency for ONE, QUORUM, ALL
    // Expected: ONE ~1ms, QUORUM ~3ms, ALL ~5ms
}

/// Benchmark: read latency by consistency level
#[bench]
fn bench_read_consistency_levels(b: &mut Bencher) {
    // 3-node cluster, pre-populated 1KB leaf
    // Measure: get latency for ONE, QUORUM, ALL
    // Expected: ONE ~0.5ms (local), QUORUM ~4ms, ALL ~6ms
}

/// Benchmark: quorum write throughput
#[bench]
fn bench_quorum_write_throughput(b: &mut Bencher) {
    // 3-node cluster, concurrent QUORUM writes
    // Measure: writes/sec
    // Expected: >5K writes/sec for 1KB leaves
}

/// Benchmark: consistency verification overhead
#[bench]
fn bench_verifier_validate(b: &mut Bencher) {
    // Measure: ConsistencyVerifier::validate() latency
    // Expected: <1μs (pure computation, no I/O)
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/quorum.rs` | **NEW** — QuorumCoordinator |
| `deriva-network/src/consistency.rs` | **NEW** — ConsistencyLevel, ConsistencyConfig, ConsistencyVerifier |
| `deriva-network/src/lib.rs` | Add `pub mod quorum, consistency` |
| `deriva-core/src/error.rs` | Add `DerivaError::Consistency(String)` variant |
| `proto/deriva.proto` | Add `ConsistencyLevel` enum, update PutLeaf/Get requests |
| `deriva-server/src/service.rs` | Update `put_leaf` and `get` for consistency levels |
| `deriva-server/src/state.rs` | Add `quorum_coordinator`, `consistency_config` |
| `deriva-network/tests/consistency.rs` | **NEW** — unit tests |
| `deriva-network/tests/quorum.rs` | **NEW** — integration tests |

---

## 9. Dependency Changes

No new external dependencies. Uses existing `tokio`, `tonic`, `futures`, `dashmap`.

---

## 10. Design Rationale

### 10.1 Why Three Tiers Instead of Uniform Consistency?

```
Uniform strong consistency:
  - Recipes: ✓ correct (need strong)
  - Leaves: ✓ correct but slow (QUORUM for every read)
  - Cache: ✗ wasteful (replicating recomputable data)

Uniform eventual consistency:
  - Recipes: ✗ dangerous (stale recipe → wrong computation)
  - Leaves: ✓ fast but may read stale
  - Cache: ✓ fine (already recomputable)

Three tiers:
  - Recipes: strong (correctness critical, data is tiny ~300B)
  - Leaves: tunable (user chooses latency vs consistency)
  - Cache: eventual (always recomputable, no replication cost)

  Each tier gets the consistency level that matches its semantics.
```

### 10.2 Why Content-Addressing Makes This Simpler

```
In a traditional KV store:
  - Write conflicts require resolution (LWW, vector clocks, CRDTs)
  - Read-repair must handle divergent values
  - Anti-entropy must detect and resolve conflicts
  - Consistency levels affect correctness

In Deriva:
  - No write conflicts (same content = same address)
  - Read-repair is trivial (any copy is correct)
  - Anti-entropy only checks presence, not value
  - Consistency levels only affect visibility, not correctness

  This means:
  - Quorum reads don't need to compare values (all identical)
  - Read-repair just copies data (no merge logic)
  - Eventual consistency is safe (stale = missing, not wrong)
```

### 10.3 Why QUORUM/QUORUM as Default?

```
Options considered:
  ONE/ONE:    Fast but no read-after-write guarantee
  ONE/ALL:    Fast writes, slow reads, strong reads
  ALL/ONE:    Slow writes, fast reads, strong reads
  QUORUM/QUORUM: Balanced latency, strong consistency

  QUORUM/QUORUM is the standard choice because:
  - W+R > N guarantees overlap → strong consistency
  - Tolerates ⌊(N-1)/2⌋ failures → good availability
  - Balanced latency (neither write nor read is bottleneck)
  - Well-understood by operators (Cassandra, DynamoDB precedent)
```

### 10.4 Why Per-Request Consistency Override?

```
Server-wide config is the default, but per-request override allows:

  - Batch import: use ONE for speed, then verify with QUORUM read
  - Critical data: use ALL for important writes
  - Analytics: use ONE for reads (stale is fine)
  - Interactive: use QUORUM for read-after-write

  The proto ConsistencyLevel enum includes DEFAULT (0) which
  means "use server config". Clients that don't care about
  consistency just omit the field.
```

### 10.5 Why No Vector Clocks or CRDTs?

```
Vector clocks / CRDTs solve write conflicts in mutable KV stores.

Deriva has no write conflicts (content-addressed):
  - Same data → same CAddr → idempotent write
  - Different data → different CAddr → no conflict

Therefore:
  - Vector clocks: unnecessary (no concurrent conflicting writes)
  - CRDTs: unnecessary (no state to merge)
  - Lamport timestamps: unnecessary (no causal ordering needed)

  The only ordering that matters is recipe dependency order,
  which is encoded in the DAG structure, not in timestamps.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `quorum_write_total` | Counter | `level={one,quorum,all}` | Write operations by level |
| `quorum_write_latency_ms` | Histogram | `level` | Write latency |
| `quorum_write_acks` | Histogram | `level` | Acks received per write |
| `quorum_write_failures` | Counter | `level`, `reason={timeout,insufficient_acks}` | Failed writes |
| `quorum_read_total` | Counter | `level` | Read operations by level |
| `quorum_read_latency_ms` | Histogram | `level` | Read latency |
| `quorum_read_responses` | Histogram | `level` | Responses received per read |
| `quorum_read_failures` | Counter | `level`, `reason` | Failed reads |
| `consistency_warnings` | Gauge | `type` | Active config warnings |
| `read_repair_total` | Counter | — | Read-repair operations triggered |

### 11.2 Structured Logging

```rust
tracing::info!(
    addr = %addr,
    level = ?level,
    acks = ack_count,
    required = required,
    latency_ms = elapsed.as_millis(),
    "quorum write completed"
);

tracing::warn!(
    addr = %addr,
    level = ?level,
    acks = ack_count,
    required = required,
    errors = ?errors,
    "quorum write failed: insufficient acks"
);

tracing::debug!(
    addr = %addr,
    "read-repair: storing locally after remote fetch"
);

// Startup validation
for warning in verifier.validate() {
    tracing::warn!(warning = %warning, "consistency configuration warning");
}
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| Quorum write failures | `quorum_write_failures` > 1% of writes | Critical |
| Quorum read failures | `quorum_read_failures` > 1% of reads | Critical |
| Write latency spike | p99 `quorum_write_latency_ms` > 1s | Warning |
| Read latency spike | p99 `quorum_read_latency_ms` > 500ms | Warning |
| Consistency warnings active | `consistency_warnings` > 0 | Info |
| High read-repair rate | `read_repair_total` > 100/min | Info |

---

## 12. Checklist

- [ ] Create `deriva-network/src/consistency.rs`
- [ ] Implement `ConsistencyLevel` enum with `required_nodes`, `is_strongly_consistent`
- [ ] Implement `ConsistencyConfig` with defaults
- [ ] Implement `ConsistencyVerifier` with validation warnings
- [ ] Create `deriva-network/src/quorum.rs`
- [ ] Implement `QuorumCoordinator` with fan-out write
- [ ] Implement `QuorumCoordinator` with fan-out read
- [ ] Add `DerivaError::Consistency` variant
- [ ] Add `ConsistencyLevel` enum to proto
- [ ] Update `PutLeafRequest` with optional consistency field
- [ ] Update `GetRequest` with optional consistency field
- [ ] Update `put_leaf` handler for quorum writes
- [ ] Integrate consistency-aware reads into distributed get (§3.6)
- [ ] Add read-repair on remote leaf fetch
- [ ] Add startup config validation with warnings
- [ ] Write ConsistencyLevel unit tests (8 tests)
- [ ] Write ConsistencyConfig tests (2 tests)
- [ ] Write ConsistencyVerifier tests (5 tests)
- [ ] Write QuorumCoordinator integration tests (6 tests)
- [ ] Add metrics (10 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (6 alerts)
- [ ] Run benchmarks: write/read latency by level, throughput, verifier overhead
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
