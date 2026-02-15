# §3.1 Node Discovery — SWIM Gossip

> **Status**: Blueprint  
> **Depends on**: Phase 2 complete  
> **Crate(s)**: `deriva-network`, `deriva-core`, `deriva-server`  
> **Estimated effort**: 5–7 days  

---

## 1. Problem Statement

### 1.1 Current Limitation

Deriva Phase 1–2 is a single-node system. The `deriva-network` crate is a
stub with no implementation:

```rust
// crates/deriva-network/src/lib.rs (current)
// Distribution layer — implemented in Phase 3
```

There is no mechanism for:
1. **Node discovery** — finding other Deriva nodes on the network
2. **Membership tracking** — knowing which nodes are alive/dead
3. **Metadata exchange** — sharing cache contents, load, capacity
4. **Failure detection** — detecting crashed or partitioned nodes

### 1.2 Why SWIM?

Distributed systems need a membership protocol. Common options:

| Protocol | Consistency | Bandwidth | Failure Detection | Complexity |
|----------|------------|-----------|-------------------|------------|
| Centralized registry | Strong | Low | Slow (polling) | Low |
| Heartbeat (all-to-all) | Eventual | O(N²) | Fast | Low |
| SWIM gossip | Eventual | O(N log N) | Fast | Moderate |
| Raft/Paxos | Strong | O(N) | Fast | High |

SWIM (Scalable Weakly-consistent Infection-style Membership) is ideal because:
- **O(N log N) bandwidth** — scales to hundreds of nodes
- **Fast failure detection** — configurable probe interval (default 1s)
- **Piggybacked metadata** — gossip carries cache/load info for free
- **No single point of failure** — fully decentralized
- **Battle-tested** — used by Consul (HashiCorp), Memberlist, NATS

### 1.3 SWIM Protocol Overview

```
SWIM operates in rounds. Each round, a node:

  1. PING a random member
     ┌──────┐  ping   ┌──────┐
     │Node A│────────▶│Node B│
     └──────┘         └──────┘

  2. If ACK received → B is alive
     ┌──────┐  ack    ┌──────┐
     │Node A│◀────────│Node B│
     └──────┘         └──────┘

  3. If no ACK → indirect probe via K random members
     ┌──────┐ ping-req ┌──────┐  ping  ┌──────┐
     │Node A│─────────▶│Node C│───────▶│Node B│
     └──────┘          └──────┘        └──────┘
                       ┌──────┐  ack   ┌──────┐
                       │Node C│◀───────│Node B│
                       └──────┘        └──────┘
     ┌──────┐  ack     ┌──────┐
     │Node A│◀─────────│Node C│
     └──────┘          └──────┘

  4. If still no ACK → mark B as SUSPECT
  5. After suspicion timeout → mark B as DEAD
  6. Piggyback membership updates on all messages
```

### 1.4 What Deriva Needs Beyond Basic SWIM

Standard SWIM provides membership (alive/dead). Deriva also needs:

| Feature | Standard SWIM | Deriva Extension |
|---------|--------------|-----------------|
| Membership | ✓ | ✓ |
| Failure detection | ✓ | ✓ |
| Cache metadata | ✗ | Piggyback cache summary (bloom filter) |
| Compute load | ✗ | Piggyback CPU/memory utilization |
| Storage capacity | ✗ | Piggyback free disk space |
| Node role | ✗ | Piggyback role (compute/storage/hybrid) |
| Cluster epoch | ✗ | Monotonic epoch for view changes |

---

## 2. Design

### 2.1 Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     deriva-network                           │
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐   │
│  │  SwimRuntime  │  │ MemberList   │  │ MetadataStore    │   │
│  │              │  │              │  │                  │   │
│  │ probe loop   │  │ members[]    │  │ node_id → meta   │   │
│  │ gossip loop  │  │ alive/suspect│  │ cache bloom      │   │
│  │ failure det. │  │ dead         │  │ load, capacity   │   │
│  └──────┬───────┘  └──────┬───────┘  └────────┬─────────┘   │
│         │                  │                    │              │
│         ▼                  ▼                    ▼              │
│  ┌──────────────────────────────────────────────────────┐    │
│  │                   UDP Transport                       │    │
│  │  port: 7947 (configurable)                            │    │
│  │  messages: Ping, Ack, PingReq, Gossip                 │    │
│  └──────────────────────────────────────────────────────┘    │
│                                                               │
│  ┌──────────────────────────────────────────────────────┐    │
│  │                 Event Channel                         │    │
│  │  MemberJoined, MemberLeft, MemberSuspect,             │    │
│  │  MetadataUpdated                                      │    │
│  └──────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Node Identity

```rust
/// Unique identifier for a node in the cluster.
/// Composed of address + incarnation to handle restarts.
///
///   NodeId = (SocketAddr, incarnation_number)
///
/// Two nodes on the same address with different incarnations
/// are treated as different logical nodes (restart detection).

┌──────────────────────────────────┐
│            NodeId                 │
│                                   │
│  addr: SocketAddr                 │
│  incarnation: u64                 │
│  name: String (optional, human)   │
│                                   │
│  Serialized: addr + ":" + inc     │
│  Example: "10.0.1.5:7947:42"     │
└──────────────────────────────────┘
```

### 2.3 Member States

```
                    ┌─────────┐
         join       │  ALIVE  │◀──────── refute (higher incarnation)
        ────────▶  │         │
                    └────┬────┘
                         │ no ack after probe
                         ▼
                    ┌─────────┐
                    │ SUSPECT │
                    └────┬────┘
                         │ suspicion timeout
                         ▼
                    ┌─────────┐
                    │  DEAD   │
                    └────┬────┘
                         │ cleanup timeout
                         ▼
                    ┌─────────┐
                    │ REMOVED │ (purged from member list)
                    └─────────┘

  Transitions:
    ALIVE → SUSPECT:  failed probe (no ack, no indirect ack)
    SUSPECT → ALIVE:  ack received (with higher incarnation)
    SUSPECT → DEAD:   suspicion_timeout elapsed
    DEAD → REMOVED:   dead_cleanup_timeout elapsed
    Any → ALIVE:      node sends message with higher incarnation
```

### 2.4 Message Types

```
┌──────────────────────────────────────────────────────────┐
│                    SwimMessage                            │
│                                                           │
│  Header:                                                  │
│    msg_type: u8                                           │
│    sender: NodeId                                         │
│    sequence: u64                                          │
│                                                           │
│  Variants:                                                │
│                                                           │
│  Ping { target: NodeId }                                  │
│  Ack  { target: NodeId }                                  │
│  PingReq { target: NodeId, original: NodeId }             │
│  Gossip { updates: Vec<MemberUpdate> }                    │
│                                                           │
│  MemberUpdate:                                            │
│    node: NodeId                                           │
│    state: Alive | Suspect | Dead                          │
│    incarnation: u64                                       │
│    metadata: Option<NodeMetadata>                         │
│                                                           │
│  Piggybacking:                                            │
│    Every message carries up to MAX_PIGGYBACK (8)          │
│    pending MemberUpdates, most recent first.              │
└──────────────────────────────────────────────────────────┘
```

### 2.5 NodeMetadata

```
┌──────────────────────────────────────────────────────────┐
│                   NodeMetadata                            │
│                                                           │
│  grpc_addr: SocketAddr     (gRPC endpoint for RPCs)       │
│  role: NodeRole            (Compute | Storage | Hybrid)   │
│  cache_bloom: Vec<u8>      (bloom filter of cached addrs) │
│  cache_entry_count: u64                                   │
│  cache_size_bytes: u64                                    │
│  blob_count: u64                                          │
│  blob_size_bytes: u64                                     │
│  recipe_count: u64                                        │
│  cpu_load: f32             (0.0–1.0)                      │
│  memory_used_bytes: u64                                   │
│  memory_total_bytes: u64                                  │
│  uptime_secs: u64                                         │
│  version: String           (Deriva version)               │
│                                                           │
│  Serialized size: ~200 bytes + bloom filter               │
│  Bloom filter: 1KB for ~1000 addrs at 1% FPR             │
│  Total per-node metadata: ~1.2KB                          │
└──────────────────────────────────────────────────────────┘
```

### 2.6 Bloom Filter for Cache Contents

```
Purpose: Allow remote nodes to check "does node X likely have
addr Y cached?" without querying node X directly.

  Bloom filter parameters:
    Expected items: 10,000 (typical cache size)
    False positive rate: 1%
    Bits needed: ~96,000 (12KB)
    Hash functions: 7

  For smaller caches (1,000 items):
    Bits: ~9,600 (1.2KB)
    Hash functions: 7

  Trade-off:
    Larger bloom → fewer false positives → better routing
    Smaller bloom → less gossip bandwidth

  Update frequency:
    Rebuild bloom filter every 10 seconds (configurable)
    Or on significant cache change (>10% entries changed)

  Usage in compute routing (§3.5):
    "Which node likely has inputs [A, B, C] cached?"
    Check each node's bloom filter for A, B, C.
    Route to node with most hits.
```

### 2.7 Configuration

```rust
pub struct SwimConfig {
    /// UDP bind address for SWIM protocol
    pub bind_addr: SocketAddr,
    /// gRPC address advertised to other nodes
    pub grpc_addr: SocketAddr,
    /// Seed nodes to join on startup
    pub seeds: Vec<SocketAddr>,
    /// Probe interval (how often to ping a random member)
    pub probe_interval: Duration,     // default: 1s
    /// Probe timeout (how long to wait for ack)
    pub probe_timeout: Duration,      // default: 500ms
    /// Number of indirect probe targets
    pub indirect_probes: usize,       // default: 3
    /// Suspicion timeout multiplier (× probe_interval × log(N))
    pub suspicion_mult: u32,          // default: 4
    /// Dead node cleanup timeout
    pub dead_cleanup: Duration,       // default: 30s
    /// Max piggyback updates per message
    pub max_piggyback: usize,         // default: 8
    /// Metadata refresh interval
    pub metadata_interval: Duration,  // default: 10s
    /// Bloom filter size in bits
    pub bloom_bits: usize,            // default: 96_000
    /// Bloom filter hash count
    pub bloom_hashes: usize,          // default: 7
    /// Node role
    pub role: NodeRole,               // default: Hybrid
    /// Human-readable node name
    pub node_name: Option<String>,
}
```

### 2.8 Suspicion Timeout Scaling

```
SWIM uses logarithmic suspicion timeout to balance:
  - Fast detection (short timeout)
  - Avoiding false positives (long timeout)

  suspicion_timeout = suspicion_mult × probe_interval × log(N+1)

  Where N = number of members.

  Examples:
    N=3:   4 × 1s × log(4)  = 4 × 1.39 = 5.5s
    N=10:  4 × 1s × log(11) = 4 × 2.40 = 9.6s
    N=50:  4 × 1s × log(51) = 4 × 3.93 = 15.7s
    N=100: 4 × 1s × log(101)= 4 × 4.62 = 18.5s

  Larger clusters → longer suspicion timeout → fewer false positives
  This is correct: in larger clusters, network congestion is more
  likely, so we need more patience before declaring a node dead.
```

---

## 3. Implementation

### 3.1 Core Types

Location: `crates/deriva-network/src/types.rs`

```rust
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use serde::{Serialize, Deserialize};

/// Unique node identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId {
    pub addr: SocketAddr,
    pub incarnation: u64,
    pub name: String,
}

impl NodeId {
    pub fn new(addr: SocketAddr, name: impl Into<String>) -> Self {
        Self {
            addr,
            incarnation: 0,
            name: name.into(),
        }
    }

    /// Increment incarnation (used to refute suspicion).
    pub fn next_incarnation(&mut self) {
        self.incarnation += 1;
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.addr, self.incarnation, self.name)
    }
}

/// Node role in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    /// Primarily handles computation (large cache, fast CPU)
    Compute,
    /// Primarily stores data (large disk, high durability)
    Storage,
    /// Both compute and storage (default)
    Hybrid,
}

/// State of a member in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberState {
    Alive,
    Suspect,
    Dead,
}

/// A member in the cluster membership list.
#[derive(Debug, Clone)]
pub struct Member {
    pub id: NodeId,
    pub state: MemberState,
    pub state_change: Instant,
    pub metadata: Option<NodeMetadata>,
}

/// Metadata about a node, piggybacked on gossip messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetadata {
    pub grpc_addr: SocketAddr,
    pub role: NodeRole,
    pub cache_bloom: Vec<u8>,
    pub cache_entry_count: u64,
    pub cache_size_bytes: u64,
    pub blob_count: u64,
    pub blob_size_bytes: u64,
    pub recipe_count: u64,
    pub cpu_load: f32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub uptime_secs: u64,
    pub version: String,
}

/// A membership update, disseminated via gossip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberUpdate {
    pub node: NodeId,
    pub state: MemberState,
    pub incarnation: u64,
    pub metadata: Option<NodeMetadata>,
}

/// Events emitted by the SWIM runtime.
#[derive(Debug, Clone)]
pub enum SwimEvent {
    MemberJoined(NodeId),
    MemberLeft(NodeId),
    MemberSuspect(NodeId),
    MemberAlive(NodeId),
    MetadataUpdated(NodeId, NodeMetadata),
}
```

### 3.2 Message Serialization

Location: `crates/deriva-network/src/message.rs`

```rust
use serde::{Serialize, Deserialize};
use crate::types::{NodeId, MemberUpdate};

/// Wire format for SWIM messages.
///
/// All messages are serialized with bincode for compact binary encoding.
/// Max message size: 65507 bytes (UDP max payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SwimMessage {
    /// Direct probe: "are you alive?"
    Ping {
        sender: NodeId,
        sequence: u64,
        piggyback: Vec<MemberUpdate>,
    },
    /// Response to Ping: "yes, I'm alive"
    Ack {
        sender: NodeId,
        sequence: u64,
        piggyback: Vec<MemberUpdate>,
    },
    /// Indirect probe request: "please ping target for me"
    PingReq {
        sender: NodeId,
        target: NodeId,
        sequence: u64,
        piggyback: Vec<MemberUpdate>,
    },
    /// Bulk gossip (periodic full state sync for convergence)
    Sync {
        sender: NodeId,
        members: Vec<MemberUpdate>,
    },
}

impl SwimMessage {
    /// Serialize to bytes for UDP transmission.
    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize from UDP bytes.
    pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }

    /// Extract piggyback updates from any message type.
    pub fn piggyback(&self) -> &[MemberUpdate] {
        match self {
            SwimMessage::Ping { piggyback, .. } => piggyback,
            SwimMessage::Ack { piggyback, .. } => piggyback,
            SwimMessage::PingReq { piggyback, .. } => piggyback,
            SwimMessage::Sync { members, .. } => members,
        }
    }

    /// Get the sender of any message.
    pub fn sender(&self) -> &NodeId {
        match self {
            SwimMessage::Ping { sender, .. } => sender,
            SwimMessage::Ack { sender, .. } => sender,
            SwimMessage::PingReq { sender, .. } => sender,
            SwimMessage::Sync { sender, .. } => sender,
        }
    }
}
```

### 3.3 MemberList

Location: `crates/deriva-network/src/memberlist.rs`

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};
use crate::types::*;

/// Manages the cluster membership list.
///
/// Thread-safe: wrapped in RwLock by SwimRuntime.
pub struct MemberList {
    /// Our own node ID.
    local: NodeId,
    /// All known members (including self).
    members: HashMap<NodeId, Member>,
    /// Pending updates to piggyback on outgoing messages.
    /// Sorted by priority (most recent first).
    pending_updates: Vec<(MemberUpdate, u8)>, // (update, retransmit_count)
    /// Max retransmit count for piggyback updates.
    max_retransmit: u8,
}

impl MemberList {
    pub fn new(local: NodeId) -> Self {
        let mut members = HashMap::new();
        members.insert(local.clone(), Member {
            id: local.clone(),
            state: MemberState::Alive,
            state_change: Instant::now(),
            metadata: None,
        });
        Self {
            local,
            members,
            pending_updates: Vec::new(),
            max_retransmit: 4, // retransmit each update ~4 times
        }
    }

    /// Number of alive + suspect members (excluding dead).
    pub fn alive_count(&self) -> usize {
        self.members.values()
            .filter(|m| m.state != MemberState::Dead)
            .count()
    }

    /// Get all alive members (excluding self).
    pub fn alive_peers(&self) -> Vec<&Member> {
        self.members.values()
            .filter(|m| m.id != self.local && m.state == MemberState::Alive)
            .collect()
    }

    /// Pick a random alive peer for probing.
    pub fn random_peer(&self) -> Option<NodeId> {
        let peers = self.alive_peers();
        if peers.is_empty() {
            return None;
        }
        use rand::Rng;
        let idx = rand::thread_rng().gen_range(0..peers.len());
        Some(peers[idx].id.clone())
    }

    /// Pick K random alive peers for indirect probing (excluding target).
    pub fn random_peers_excluding(
        &self,
        exclude: &NodeId,
        count: usize,
    ) -> Vec<NodeId> {
        use rand::seq::SliceRandom;
        let mut peers: Vec<NodeId> = self.members.values()
            .filter(|m| {
                m.id != self.local
                && m.id != *exclude
                && m.state == MemberState::Alive
            })
            .map(|m| m.id.clone())
            .collect();
        peers.shuffle(&mut rand::thread_rng());
        peers.truncate(count);
        peers
    }

    /// Apply a membership update (from gossip or direct observation).
    ///
    /// Returns the event to emit, if any.
    pub fn apply_update(&mut self, update: &MemberUpdate) -> Option<SwimEvent> {
        if let Some(existing) = self.members.get_mut(&update.node) {
            // Incarnation-based conflict resolution:
            // Higher incarnation always wins.
            // Same incarnation: Alive < Suspect < Dead
            if update.incarnation < existing.id.incarnation {
                return None; // stale update
            }
            if update.incarnation == existing.id.incarnation {
                let dominated = match (&existing.state, &update.state) {
                    (MemberState::Alive, MemberState::Alive) => true,
                    (MemberState::Suspect, MemberState::Alive) => false,
                    (MemberState::Dead, _) => false,
                    _ => true,
                };
                if !dominated && update.state != MemberState::Dead {
                    return None;
                }
            }

            let old_state = existing.state;
            existing.state = update.state;
            existing.state_change = Instant::now();
            existing.id.incarnation = update.incarnation;
            if let Some(ref meta) = update.metadata {
                existing.metadata = Some(meta.clone());
            }

            match (old_state, update.state) {
                (MemberState::Alive, MemberState::Suspect) =>
                    Some(SwimEvent::MemberSuspect(update.node.clone())),
                (MemberState::Suspect, MemberState::Dead) |
                (MemberState::Alive, MemberState::Dead) =>
                    Some(SwimEvent::MemberLeft(update.node.clone())),
                (MemberState::Suspect, MemberState::Alive) =>
                    Some(SwimEvent::MemberAlive(update.node.clone())),
                _ => None,
            }
        } else {
            // New member
            self.members.insert(update.node.clone(), Member {
                id: NodeId {
                    addr: update.node.addr,
                    incarnation: update.incarnation,
                    name: update.node.name.clone(),
                },
                state: update.state,
                state_change: Instant::now(),
                metadata: update.metadata.clone(),
            });
            if update.state == MemberState::Alive {
                Some(SwimEvent::MemberJoined(update.node.clone()))
            } else {
                None
            }
        }
    }

    /// Mark a node as suspect (failed probe).
    pub fn suspect(&mut self, node: &NodeId) -> Option<SwimEvent> {
        self.apply_update(&MemberUpdate {
            node: node.clone(),
            state: MemberState::Suspect,
            incarnation: node.incarnation,
            metadata: None,
        })
    }

    /// Mark a node as dead (suspicion timeout expired).
    pub fn declare_dead(&mut self, node: &NodeId) -> Option<SwimEvent> {
        self.apply_update(&MemberUpdate {
            node: node.clone(),
            state: MemberState::Dead,
            incarnation: node.incarnation,
            metadata: None,
        })
    }

    /// Refute suspicion about ourselves by incrementing incarnation.
    pub fn refute(&mut self) -> MemberUpdate {
        self.local.next_incarnation();
        if let Some(me) = self.members.get_mut(&self.local) {
            me.id.incarnation = self.local.incarnation;
            me.state = MemberState::Alive;
            me.state_change = Instant::now();
        }
        MemberUpdate {
            node: self.local.clone(),
            state: MemberState::Alive,
            incarnation: self.local.incarnation,
            metadata: None,
        }
    }

    /// Get pending updates for piggybacking (up to max_count).
    pub fn drain_piggyback(&mut self, max_count: usize) -> Vec<MemberUpdate> {
        let mut result = Vec::with_capacity(max_count);
        let mut i = 0;
        while i < self.pending_updates.len() && result.len() < max_count {
            result.push(self.pending_updates[i].0.clone());
            self.pending_updates[i].1 -= 1;
            if self.pending_updates[i].1 == 0 {
                self.pending_updates.remove(i);
            } else {
                i += 1;
            }
        }
        result
    }

    /// Queue an update for piggybacking on future messages.
    pub fn queue_update(&mut self, update: MemberUpdate) {
        self.pending_updates.push((update, self.max_retransmit));
    }

    /// Remove dead members that have exceeded cleanup timeout.
    pub fn cleanup_dead(&mut self, timeout: Duration) -> Vec<NodeId> {
        let now = Instant::now();
        let mut removed = Vec::new();
        self.members.retain(|id, member| {
            if member.state == MemberState::Dead
                && now.duration_since(member.state_change) > timeout
            {
                removed.push(id.clone());
                false
            } else {
                true
            }
        });
        removed
    }

    /// Get all members with their states (for status/debugging).
    pub fn all_members(&self) -> Vec<(NodeId, MemberState)> {
        self.members.iter()
            .map(|(id, m)| (id.clone(), m.state))
            .collect()
    }

    /// Get metadata for a specific node.
    pub fn get_metadata(&self, node: &NodeId) -> Option<&NodeMetadata> {
        self.members.get(node).and_then(|m| m.metadata.as_ref())
    }

    /// Update local node's metadata.
    pub fn update_local_metadata(&mut self, metadata: NodeMetadata) {
        if let Some(me) = self.members.get_mut(&self.local) {
            me.metadata = Some(metadata);
        }
    }
}
```


### 3.4 SwimRuntime

Location: `crates/deriva-network/src/runtime.rs`

```rust
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::net::UdpSocket;
use crate::types::*;
use crate::memberlist::MemberList;
use crate::message::SwimMessage;
use crate::config::SwimConfig;

/// The main SWIM runtime. Manages the probe loop, gossip dissemination,
/// failure detection, and metadata exchange.
///
/// Spawns background tokio tasks for:
/// 1. Probe loop (periodic ping of random member)
/// 2. Receive loop (handle incoming UDP messages)
/// 3. Metadata refresh loop (update local metadata)
/// 4. Dead cleanup loop (purge long-dead members)
pub struct SwimRuntime {
    config: SwimConfig,
    local_id: NodeId,
    members: Arc<RwLock<MemberList>>,
    socket: Arc<UdpSocket>,
    event_tx: mpsc::Sender<SwimEvent>,
    sequence: Arc<std::sync::atomic::AtomicU64>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl SwimRuntime {
    /// Create and start the SWIM runtime.
    pub async fn start(
        config: SwimConfig,
    ) -> Result<(Self, mpsc::Receiver<SwimEvent>), std::io::Error> {
        let socket = UdpSocket::bind(&config.bind_addr).await?;
        let socket = Arc::new(socket);

        let local_id = NodeId::new(
            config.bind_addr,
            config.node_name.clone().unwrap_or_else(|| {
                format!("node-{}", config.bind_addr.port())
            }),
        );

        let members = Arc::new(RwLock::new(MemberList::new(local_id.clone())));
        let (event_tx, event_rx) = mpsc::channel(256);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);

        let runtime = Self {
            config,
            local_id,
            members,
            socket,
            event_tx,
            sequence: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            shutdown: shutdown_tx,
        };

        // Join seed nodes
        runtime.join_seeds().await;

        // Spawn background tasks
        runtime.spawn_receive_loop();
        runtime.spawn_probe_loop();
        runtime.spawn_cleanup_loop();

        Ok((runtime, event_rx))
    }

    /// Join seed nodes by sending initial Ping messages.
    async fn join_seeds(&self) {
        for seed_addr in &self.config.seeds {
            if *seed_addr == self.config.bind_addr {
                continue; // don't ping ourselves
            }
            let seq = self.next_sequence();
            let msg = SwimMessage::Ping {
                sender: self.local_id.clone(),
                sequence: seq,
                piggyback: vec![],
            };
            if let Ok(data) = msg.encode() {
                let _ = self.socket.send_to(&data, seed_addr).await;
            }
        }
    }

    fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Spawn the UDP receive loop.
    fn spawn_receive_loop(&self) {
        let socket = self.socket.clone();
        let members = self.members.clone();
        let event_tx = self.event_tx.clone();
        let local_id = self.local_id.clone();
        let config = self.config.clone();
        let mut shutdown_rx = self.shutdown.subscribe();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            loop {
                tokio::select! {
                    result = socket.recv_from(&mut buf) => {
                        if let Ok((len, src)) = result {
                            if let Ok(msg) = SwimMessage::decode(&buf[..len]) {
                                Self::handle_message(
                                    &msg, src, &socket, &members,
                                    &event_tx, &local_id, &config,
                                ).await;
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => break,
                }
            }
        });
    }

    /// Handle an incoming SWIM message.
    async fn handle_message(
        msg: &SwimMessage,
        src: std::net::SocketAddr,
        socket: &UdpSocket,
        members: &RwLock<MemberList>,
        event_tx: &mpsc::Sender<SwimEvent>,
        local_id: &NodeId,
        config: &SwimConfig,
    ) {
        // Process piggybacked updates
        {
            let mut ml = members.write().await;
            for update in msg.piggyback() {
                if let Some(event) = ml.apply_update(update) {
                    let _ = event_tx.send(event).await;
                }
            }
        }

        match msg {
            SwimMessage::Ping { sender, sequence, .. } => {
                // Respond with Ack
                let piggyback = {
                    let mut ml = members.write().await;
                    // Mark sender as alive (we just heard from them)
                    ml.apply_update(&MemberUpdate {
                        node: sender.clone(),
                        state: MemberState::Alive,
                        incarnation: sender.incarnation,
                        metadata: None,
                    });
                    ml.drain_piggyback(config.max_piggyback)
                };

                let ack = SwimMessage::Ack {
                    sender: local_id.clone(),
                    sequence: *sequence,
                    piggyback,
                };
                if let Ok(data) = ack.encode() {
                    let _ = socket.send_to(&data, src).await;
                }
            }

            SwimMessage::Ack { sender, .. } => {
                // Mark sender as alive
                let mut ml = members.write().await;
                if let Some(event) = ml.apply_update(&MemberUpdate {
                    node: sender.clone(),
                    state: MemberState::Alive,
                    incarnation: sender.incarnation,
                    metadata: None,
                }) {
                    let _ = event_tx.send(event).await;
                }
            }

            SwimMessage::PingReq { target, sender, sequence, .. } => {
                // Forward ping to target on behalf of sender
                let ping = SwimMessage::Ping {
                    sender: local_id.clone(),
                    sequence: *sequence,
                    piggyback: vec![],
                };
                if let Ok(data) = ping.encode() {
                    let _ = socket.send_to(&data, target.addr).await;
                }
                // When we get Ack from target, we forward it back to sender
                // (simplified: the Ack goes directly to us, we relay)
            }

            SwimMessage::Sync { members: remote_members, .. } => {
                let mut ml = members.write().await;
                for update in remote_members {
                    if let Some(event) = ml.apply_update(update) {
                        let _ = event_tx.send(event).await;
                    }
                }
            }
        }
    }

    /// Spawn the periodic probe loop.
    fn spawn_probe_loop(&self) {
        let socket = self.socket.clone();
        let members = self.members.clone();
        let event_tx = self.event_tx.clone();
        let local_id = self.local_id.clone();
        let config = self.config.clone();
        let sequence = self.sequence.clone();
        let mut shutdown_rx = self.shutdown.subscribe();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.probe_interval);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        Self::probe_cycle(
                            &socket, &members, &event_tx,
                            &local_id, &config, &sequence,
                        ).await;
                    }
                    _ = shutdown_rx.changed() => break,
                }
            }
        });
    }

    /// Execute one probe cycle: ping random member, handle timeout.
    async fn probe_cycle(
        socket: &UdpSocket,
        members: &RwLock<MemberList>,
        event_tx: &mpsc::Sender<SwimEvent>,
        local_id: &NodeId,
        config: &SwimConfig,
        sequence: &std::sync::atomic::AtomicU64,
    ) {
        // Pick random peer
        let target = {
            let ml = members.read().await;
            ml.random_peer()
        };
        let target = match target {
            Some(t) => t,
            None => return, // no peers
        };

        let seq = sequence.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let piggyback = {
            let mut ml = members.write().await;
            ml.drain_piggyback(config.max_piggyback)
        };

        // Send Ping
        let ping = SwimMessage::Ping {
            sender: local_id.clone(),
            sequence: seq,
            piggyback,
        };
        if let Ok(data) = ping.encode() {
            let _ = socket.send_to(&data, target.addr).await;
        }

        // Wait for Ack
        tokio::time::sleep(config.probe_timeout).await;

        // Check if target responded (simplified: check member state)
        let got_ack = {
            let ml = members.read().await;
            ml.all_members().iter().any(|(id, state)| {
                *id == target && *state == MemberState::Alive
            })
        };

        if got_ack {
            return; // target is alive
        }

        // Indirect probe: ask K random peers to ping target
        let indirect_targets = {
            let ml = members.read().await;
            ml.random_peers_excluding(&target, config.indirect_probes)
        };

        for relay in &indirect_targets {
            let ping_req = SwimMessage::PingReq {
                sender: local_id.clone(),
                target: target.clone(),
                sequence: seq,
                piggyback: vec![],
            };
            if let Ok(data) = ping_req.encode() {
                let _ = socket.send_to(&data, relay.addr).await;
            }
        }

        // Wait for indirect ack
        tokio::time::sleep(config.probe_timeout).await;

        // If still no ack → suspect
        let still_no_ack = {
            let ml = members.read().await;
            ml.all_members().iter().any(|(id, state)| {
                *id == target && *state != MemberState::Alive
            })
        };

        if still_no_ack || !got_ack {
            let mut ml = members.write().await;
            if let Some(event) = ml.suspect(&target) {
                let _ = event_tx.send(event).await;
                ml.queue_update(MemberUpdate {
                    node: target.clone(),
                    state: MemberState::Suspect,
                    incarnation: target.incarnation,
                    metadata: None,
                });
            }
        }
    }

    /// Spawn dead member cleanup loop.
    fn spawn_cleanup_loop(&self) {
        let members = self.members.clone();
        let config = self.config.clone();
        let mut shutdown_rx = self.shutdown.subscribe();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.dead_cleanup);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let mut ml = members.write().await;

                        // Promote suspects to dead if timeout expired
                        let n = ml.alive_count();
                        let suspicion_timeout = Duration::from_secs_f64(
                            config.suspicion_mult as f64
                            * config.probe_interval.as_secs_f64()
                            * ((n + 1) as f64).ln()
                        );

                        let suspects: Vec<NodeId> = ml.all_members().iter()
                            .filter(|(_, s)| *s == MemberState::Suspect)
                            .map(|(id, _)| id.clone())
                            .collect();

                        for suspect in suspects {
                            if let Some(member) = ml.members.get(&suspect) {
                                if member.state_change.elapsed() > suspicion_timeout {
                                    ml.declare_dead(&suspect);
                                }
                            }
                        }

                        // Purge long-dead members
                        ml.cleanup_dead(config.dead_cleanup);
                    }
                    _ = shutdown_rx.changed() => break,
                }
            }
        });
    }

    /// Get a snapshot of all members and their states.
    pub async fn members(&self) -> Vec<(NodeId, MemberState)> {
        let ml = self.members.read().await;
        ml.all_members()
    }

    /// Get metadata for a specific node.
    pub async fn get_metadata(&self, node: &NodeId) -> Option<NodeMetadata> {
        let ml = self.members.read().await;
        ml.get_metadata(node).cloned()
    }

    /// Update local node's metadata.
    pub async fn update_metadata(&self, metadata: NodeMetadata) {
        let mut ml = self.members.write().await;
        ml.update_local_metadata(metadata);
    }

    /// Gracefully shut down the SWIM runtime.
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    /// Get the local node ID.
    pub fn local_id(&self) -> &NodeId {
        &self.local_id
    }

    /// Get the number of alive members.
    pub async fn alive_count(&self) -> usize {
        let ml = self.members.read().await;
        ml.alive_count()
    }
}
```

### 3.5 Server Integration

Location: `crates/deriva-server/src/service.rs` — add cluster awareness

```rust
use deriva_network::runtime::SwimRuntime;
use deriva_network::types::{NodeMetadata, NodeRole, SwimEvent};

/// Extended ServerState with cluster membership.
pub struct ServerState {
    // ... existing fields ...
    pub swim: Option<Arc<SwimRuntime>>,
}

/// Background task: refresh local metadata and publish to SWIM.
async fn metadata_refresh_loop(
    swim: Arc<SwimRuntime>,
    state: Arc<ServerState>,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;

        let metadata = {
            let cache = state.cache.read().unwrap();
            let dag = state.dag.read().unwrap();
            NodeMetadata {
                grpc_addr: state.grpc_addr,
                role: NodeRole::Hybrid,
                cache_bloom: build_cache_bloom(&cache),
                cache_entry_count: cache.entry_count() as u64,
                cache_size_bytes: cache.current_size() as u64,
                blob_count: state.blob_store.stats()
                    .map(|(c, _)| c).unwrap_or(0),
                blob_size_bytes: state.blob_store.stats()
                    .map(|(_, b)| b).unwrap_or(0),
                recipe_count: dag.recipe_count() as u64,
                cpu_load: get_cpu_load(),
                memory_used_bytes: get_memory_used(),
                memory_total_bytes: get_memory_total(),
                uptime_secs: state.start_time.elapsed().as_secs(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            }
        };

        swim.update_metadata(metadata).await;
    }
}

/// Build a bloom filter from current cache contents.
fn build_cache_bloom(cache: &EvictableCache) -> Vec<u8> {
    // Use a simple bloom filter implementation
    // 96,000 bits = 12,000 bytes, 7 hash functions
    let mut bloom = vec![0u8; 12_000];
    for addr in cache.keys() {
        let hash_bytes = addr.as_bytes();
        for i in 0..7u64 {
            let h = hash_with_seed(hash_bytes, i);
            let bit_idx = (h as usize) % (12_000 * 8);
            bloom[bit_idx / 8] |= 1 << (bit_idx % 8);
        }
    }
    bloom
}

fn hash_with_seed(data: &[u8], seed: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hasher);
    data.hash(&mut hasher);
    hasher.finish()
}
```

### 3.6 Cluster Status RPC

```rust
// New proto messages:
message ClusterStatusRequest {}

message ClusterStatusResponse {
    repeated NodeInfo nodes = 1;
    uint64 alive_count = 2;
    uint64 suspect_count = 3;
    uint64 dead_count = 4;
}

message NodeInfo {
    string name = 1;
    string addr = 2;
    string state = 3;  // "alive", "suspect", "dead"
    uint64 incarnation = 4;
    string grpc_addr = 5;
    string role = 6;
    uint64 cache_entries = 7;
    uint64 cache_bytes = 8;
    uint64 blob_count = 9;
    float cpu_load = 10;
    uint64 uptime_secs = 11;
}

// Handler:
async fn cluster_status(
    &self,
    _request: Request<ClusterStatusRequest>,
) -> Result<Response<ClusterStatusResponse>, Status> {
    let swim = self.state.swim.as_ref()
        .ok_or_else(|| Status::unavailable("not in cluster mode"))?;

    let members = swim.members().await;
    let mut nodes = Vec::new();
    let mut alive = 0u64;
    let mut suspect = 0u64;
    let mut dead = 0u64;

    for (id, state) in &members {
        match state {
            MemberState::Alive => alive += 1,
            MemberState::Suspect => suspect += 1,
            MemberState::Dead => dead += 1,
        }

        let meta = swim.get_metadata(id).await;
        nodes.push(NodeInfo {
            name: id.name.clone(),
            addr: id.addr.to_string(),
            state: format!("{:?}", state).to_lowercase(),
            incarnation: id.incarnation,
            grpc_addr: meta.as_ref()
                .map(|m| m.grpc_addr.to_string())
                .unwrap_or_default(),
            role: meta.as_ref()
                .map(|m| format!("{:?}", m.role))
                .unwrap_or_default(),
            cache_entries: meta.as_ref()
                .map(|m| m.cache_entry_count).unwrap_or(0),
            cache_bytes: meta.as_ref()
                .map(|m| m.cache_size_bytes).unwrap_or(0),
            blob_count: meta.as_ref()
                .map(|m| m.blob_count).unwrap_or(0),
            cpu_load: meta.as_ref()
                .map(|m| m.cpu_load).unwrap_or(0.0),
            uptime_secs: meta.as_ref()
                .map(|m| m.uptime_secs).unwrap_or(0),
        });
    }

    Ok(Response::new(ClusterStatusResponse {
        nodes,
        alive_count: alive,
        suspect_count: suspect,
        dead_count: dead,
    }))
}
```

### 3.7 CLI Integration

```rust
#[derive(Subcommand)]
enum Commands {
    // ... existing ...

    /// Show cluster membership status
    Cluster,
}

Commands::Cluster => {
    let resp = client.cluster_status(ClusterStatusRequest {})
        .await?.into_inner();

    println!("Cluster: {} alive, {} suspect, {} dead",
        resp.alive_count, resp.suspect_count, resp.dead_count);
    println!();
    for node in &resp.nodes {
        let state_icon = match node.state.as_str() {
            "alive" => "●",
            "suspect" => "◐",
            "dead" => "○",
            _ => "?",
        };
        println!("  {} {} ({}) [{}]",
            state_icon, node.name, node.addr, node.state);
        println!("    gRPC: {}  role: {}  cpu: {:.0}%",
            node.grpc_addr, node.role, node.cpu_load * 100.0);
        println!("    cache: {} entries ({} bytes)  blobs: {}",
            node.cache_entries, node.cache_bytes, node.blob_count);
        println!("    uptime: {}s  incarnation: {}",
            node.uptime_secs, node.incarnation);
    }
}
```

CLI usage:

```bash
# Start node in cluster mode
deriva-server --bind 0.0.0.0:50051 \
  --swim-bind 0.0.0.0:7947 \
  --seeds 10.0.1.2:7947,10.0.1.3:7947

# Check cluster status
deriva cluster

# Example output:
# Cluster: 3 alive, 0 suspect, 0 dead
#
#   ● node-7947 (10.0.1.1:7947) [alive]
#     gRPC: 10.0.1.1:50051  role: Hybrid  cpu: 23%
#     cache: 1500 entries (48000000 bytes)  blobs: 800
#     uptime: 3600s  incarnation: 0
#
#   ● node-7947 (10.0.1.2:7947) [alive]
#     gRPC: 10.0.1.2:50051  role: Hybrid  cpu: 45%
#     cache: 2200 entries (72000000 bytes)  blobs: 1200
#     uptime: 7200s  incarnation: 1
```


---

## 4. Data Flow Diagrams

### 4.1 Three-Node Join Sequence

```
  Node A (seed)          Node B (joining)        Node C (joining)
       │                      │                       │
       │                      │  Ping(B→A)            │
       │◀─────────────────────│                       │
       │  Ack(A→B)            │                       │
       │─────────────────────▶│                       │
       │                      │                       │
       │  [A knows B]         │  [B knows A]          │
       │                      │                       │
       │                      │                  Ping(C→A)
       │◀─────────────────────│──────────────────────│
       │  Ack(A→C, piggyback:[B=alive])              │
       │─────────────────────────────────────────────▶│
       │                      │                       │
       │                      │  [C knows A and B]    │
       │                      │                       │
       │  Probe round: A pings B                      │
       │  Ping(A→B, piggyback:[C=alive])              │
       │─────────────────────▶│                       │
       │                      │  [B now knows C]      │
       │                      │                       │
       │  Convergence: all 3 nodes know each other    │
       │  Time: ~2 probe intervals (2 seconds)        │
```

### 4.2 Failure Detection — Indirect Probe

```
  Node A              Node B (slow)         Node C
    │                     │                    │
    │  Ping(A→B)          │                    │
    │────────────────────▶│ (no response)      │
    │                     │                    │
    │  ... timeout ...    │                    │
    │                     │                    │
    │  PingReq(A→C, target=B)                  │
    │─────────────────────────────────────────▶│
    │                     │                    │
    │                     │  Ping(C→B)         │
    │                     │◀───────────────────│
    │                     │  Ack(B→C)          │
    │                     │───────────────────▶│
    │                     │                    │
    │  Ack(C→A, "B is alive")                  │
    │◀─────────────────────────────────────────│
    │                     │                    │
    │  B is alive (indirect confirmation)      │
```

### 4.3 Suspicion and Death

```
  Node A              Node B (crashed)      Node C
    │                     ╳                    │
    │  Ping(A→B)          ╳                    │
    │──────────────────▶  ╳ (no response)      │
    │                     ╳                    │
    │  PingReq(A→C, target=B)                  │
    │─────────────────────────────────────────▶│
    │                     ╳  Ping(C→B)         │
    │                     ╳◀───────────────────│
    │                     ╳ (no response)      │
    │                     ╳                    │
    │  ... timeout ...    ╳                    │
    │                     ╳                    │
    │  A marks B as SUSPECT                    │
    │  Gossip: [B=suspect] piggybacked         │
    │─────────────────────────────────────────▶│
    │                     ╳                    │
    │  ... suspicion_timeout (5.5s for N=3)    │
    │                     ╳                    │
    │  A marks B as DEAD                       │
    │  Gossip: [B=dead] piggybacked            │
    │─────────────────────────────────────────▶│
    │                     ╳                    │
    │  ... dead_cleanup (30s) ...              │
    │                     ╳                    │
    │  A removes B from member list            │
```

### 4.4 Incarnation Refutation

```
  Node A              Node B              Node C
    │                     │                    │
    │  A suspects B (network glitch)           │
    │  Gossip: [B=suspect, inc=5]              │
    │─────────────────────────────────────────▶│
    │                     │                    │
    │                     │  B receives gossip │
    │                     │  "I'm suspected!"  │
    │                     │                    │
    │                     │  B increments incarnation: 5→6
    │                     │  Gossip: [B=alive, inc=6]
    │◀────────────────────│───────────────────▶│
    │                     │                    │
    │  inc=6 > inc=5 → B is ALIVE             │
    │  Suspicion refuted!                      │
```

### 4.5 Metadata Gossip Flow

```
  Node A updates cache (new entries)
    │
    │  Metadata refresh (every 10s):
    │  Rebuild bloom filter from cache
    │  Update local metadata
    │
    │  Next probe round:
    │  Ping(A→B, piggyback:[A.metadata_updated])
    │─────────────────────▶ Node B
    │                       │
    │                       │  B stores A's metadata
    │                       │  B can now check A's bloom filter
    │                       │  for cache routing decisions
    │                       │
    │                       │  Next probe: B→C
    │                       │  Ping(B→C, piggyback:[A.metadata])
    │                       │─────────────────────▶ Node C
    │                       │                       │
    │                       │  C now has A's metadata too
    │
    │  Full convergence: ~O(log N) probe rounds
    │  For N=10: ~3 rounds = 3 seconds
```

---

## 5. Test Specification

### 5.1 Unit Tests — NodeId

```rust
#[test]
fn test_node_id_equality() {
    let a = NodeId::new("127.0.0.1:7947".parse().unwrap(), "node-a");
    let b = NodeId::new("127.0.0.1:7947".parse().unwrap(), "node-a");
    assert_eq!(a, b);
}

#[test]
fn test_node_id_incarnation() {
    let mut a = NodeId::new("127.0.0.1:7947".parse().unwrap(), "node-a");
    assert_eq!(a.incarnation, 0);
    a.next_incarnation();
    assert_eq!(a.incarnation, 1);
}

#[test]
fn test_node_id_display() {
    let a = NodeId::new("127.0.0.1:7947".parse().unwrap(), "node-a");
    assert_eq!(a.to_string(), "127.0.0.1:7947:0:node-a");
}
```

### 5.2 Unit Tests — MemberList

```rust
#[test]
fn test_memberlist_new_contains_self() {
    let local = NodeId::new("127.0.0.1:7947".parse().unwrap(), "self");
    let ml = MemberList::new(local.clone());
    assert_eq!(ml.alive_count(), 1); // self
    assert!(ml.alive_peers().is_empty()); // no peers
}

#[test]
fn test_memberlist_apply_join() {
    let local = NodeId::new("127.0.0.1:7947".parse().unwrap(), "self");
    let mut ml = MemberList::new(local);

    let peer = NodeId::new("127.0.0.1:7948".parse().unwrap(), "peer");
    let event = ml.apply_update(&MemberUpdate {
        node: peer.clone(),
        state: MemberState::Alive,
        incarnation: 0,
        metadata: None,
    });

    assert_eq!(event, Some(SwimEvent::MemberJoined(peer)));
    assert_eq!(ml.alive_count(), 2);
}

#[test]
fn test_memberlist_suspect_then_dead() {
    let local = NodeId::new("127.0.0.1:7947".parse().unwrap(), "self");
    let mut ml = MemberList::new(local);

    let peer = NodeId::new("127.0.0.1:7948".parse().unwrap(), "peer");
    ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Alive,
        incarnation: 0, metadata: None,
    });

    let event = ml.suspect(&peer);
    assert!(matches!(event, Some(SwimEvent::MemberSuspect(_))));

    let event = ml.declare_dead(&peer);
    assert!(matches!(event, Some(SwimEvent::MemberLeft(_))));
}

#[test]
fn test_memberlist_refute() {
    let local = NodeId::new("127.0.0.1:7947".parse().unwrap(), "self");
    let mut ml = MemberList::new(local);

    let update = ml.refute();
    assert_eq!(update.incarnation, 1);
    assert_eq!(update.state, MemberState::Alive);
}

#[test]
fn test_memberlist_incarnation_wins() {
    let local = NodeId::new("127.0.0.1:7947".parse().unwrap(), "self");
    let mut ml = MemberList::new(local);

    let peer = NodeId::new("127.0.0.1:7948".parse().unwrap(), "peer");

    // Join at incarnation 5
    ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Alive,
        incarnation: 5, metadata: None,
    });

    // Stale suspect at incarnation 3 — should be ignored
    ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Suspect,
        incarnation: 3, metadata: None,
    });

    let members = ml.all_members();
    let (_, state) = members.iter().find(|(id, _)| *id == peer).unwrap();
    assert_eq!(*state, MemberState::Alive); // still alive
}

#[test]
fn test_memberlist_random_peer() {
    let local = NodeId::new("127.0.0.1:7947".parse().unwrap(), "self");
    let mut ml = MemberList::new(local);

    assert!(ml.random_peer().is_none()); // no peers

    let peer = NodeId::new("127.0.0.1:7948".parse().unwrap(), "peer");
    ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Alive,
        incarnation: 0, metadata: None,
    });

    assert_eq!(ml.random_peer(), Some(peer));
}

#[test]
fn test_memberlist_cleanup_dead() {
    let local = NodeId::new("127.0.0.1:7947".parse().unwrap(), "self");
    let mut ml = MemberList::new(local);

    let peer = NodeId::new("127.0.0.1:7948".parse().unwrap(), "peer");
    ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Dead,
        incarnation: 0, metadata: None,
    });

    // Cleanup with 0 timeout → immediate removal
    let removed = ml.cleanup_dead(Duration::from_secs(0));
    assert_eq!(removed.len(), 1);
    assert_eq!(ml.alive_count(), 1); // only self
}
```

### 5.3 Unit Tests — Message Serialization

```rust
#[test]
fn test_message_roundtrip_ping() {
    let msg = SwimMessage::Ping {
        sender: NodeId::new("127.0.0.1:7947".parse().unwrap(), "a"),
        sequence: 42,
        piggyback: vec![],
    };
    let data = msg.encode().unwrap();
    let decoded = SwimMessage::decode(&data).unwrap();
    assert_eq!(decoded.sender().addr, "127.0.0.1:7947".parse().unwrap());
}

#[test]
fn test_message_roundtrip_with_piggyback() {
    let update = MemberUpdate {
        node: NodeId::new("127.0.0.1:7948".parse().unwrap(), "b"),
        state: MemberState::Suspect,
        incarnation: 3,
        metadata: None,
    };
    let msg = SwimMessage::Ping {
        sender: NodeId::new("127.0.0.1:7947".parse().unwrap(), "a"),
        sequence: 1,
        piggyback: vec![update],
    };
    let data = msg.encode().unwrap();
    let decoded = SwimMessage::decode(&data).unwrap();
    assert_eq!(decoded.piggyback().len(), 1);
}

#[test]
fn test_message_size_within_udp_limit() {
    // 8 piggyback updates with metadata should fit in UDP
    let updates: Vec<MemberUpdate> = (0..8).map(|i| MemberUpdate {
        node: NodeId::new(
            format!("127.0.0.1:{}", 7947 + i).parse().unwrap(),
            format!("node-{}", i),
        ),
        state: MemberState::Alive,
        incarnation: i as u64,
        metadata: Some(NodeMetadata {
            grpc_addr: "127.0.0.1:50051".parse().unwrap(),
            role: NodeRole::Hybrid,
            cache_bloom: vec![0u8; 1200], // 1.2KB bloom
            cache_entry_count: 1000,
            cache_size_bytes: 50_000_000,
            blob_count: 500,
            blob_size_bytes: 100_000_000,
            recipe_count: 300,
            cpu_load: 0.45,
            memory_used_bytes: 2_000_000_000,
            memory_total_bytes: 8_000_000_000,
            uptime_secs: 3600,
            version: "0.1.0".to_string(),
        }),
    }).collect();

    let msg = SwimMessage::Ping {
        sender: NodeId::new("127.0.0.1:7947".parse().unwrap(), "a"),
        sequence: 1,
        piggyback: updates,
    };
    let data = msg.encode().unwrap();
    assert!(data.len() < 65507, "message too large: {} bytes", data.len());
}
```

### 5.4 Integration Tests

```rust
#[tokio::test]
async fn test_two_node_discovery() {
    let config_a = SwimConfig {
        bind_addr: "127.0.0.1:17947".parse().unwrap(),
        seeds: vec![],
        ..Default::default()
    };
    let config_b = SwimConfig {
        bind_addr: "127.0.0.1:17948".parse().unwrap(),
        seeds: vec!["127.0.0.1:17947".parse().unwrap()],
        ..Default::default()
    };

    let (runtime_a, mut events_a) = SwimRuntime::start(config_a).await.unwrap();
    let (runtime_b, mut events_b) = SwimRuntime::start(config_b).await.unwrap();

    // Wait for discovery
    tokio::time::sleep(Duration::from_secs(3)).await;

    assert_eq!(runtime_a.alive_count().await, 2);
    assert_eq!(runtime_b.alive_count().await, 2);

    runtime_a.shutdown();
    runtime_b.shutdown();
}

#[tokio::test]
async fn test_failure_detection() {
    let config_a = SwimConfig {
        bind_addr: "127.0.0.1:18947".parse().unwrap(),
        probe_interval: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(100),
        suspicion_mult: 2,
        ..Default::default()
    };
    let config_b = SwimConfig {
        bind_addr: "127.0.0.1:18948".parse().unwrap(),
        seeds: vec!["127.0.0.1:18947".parse().unwrap()],
        probe_interval: Duration::from_millis(200),
        ..Default::default()
    };

    let (runtime_a, _) = SwimRuntime::start(config_a).await.unwrap();
    let (runtime_b, _) = SwimRuntime::start(config_b).await.unwrap();

    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(runtime_a.alive_count().await, 2);

    // Kill node B
    runtime_b.shutdown();
    drop(runtime_b);

    // Wait for failure detection
    tokio::time::sleep(Duration::from_secs(3)).await;

    // A should detect B as dead
    assert_eq!(runtime_a.alive_count().await, 1);

    runtime_a.shutdown();
}
```

---

## 6. Edge Cases & Error Handling

| Case | Behavior | Rationale |
|------|----------|-----------|
| No seed nodes configured | Node starts alone, waits for others to join | Valid single-node mode |
| Seed node is down | Join fails silently, retried on next probe | Resilient to seed failures |
| All seeds are self | Ignored (don't ping self) | Defensive check |
| Network partition (A↔B but not A↔C) | C suspected by A, but B can relay | Indirect probes handle partial partitions |
| Full network partition | All remote nodes eventually marked dead | Correct — they're unreachable |
| Node restart (same addr) | New incarnation number distinguishes old/new | Incarnation-based identity |
| UDP packet loss | Retransmit via piggyback on next message | Gossip is inherently redundant |
| Message larger than UDP MTU | Bincode serialization should stay under 65KB | Bloom filter size is bounded |
| Clock skew between nodes | SWIM doesn't depend on wall clocks | Uses monotonic Instant for timeouts |
| Rapid join/leave cycling | Incarnation increments prevent confusion | Each restart is a new logical node |

---

## 7. Performance Analysis

### 7.1 Bandwidth

```
Per-probe message size:
  Ping/Ack header: ~100 bytes
  Piggyback (8 updates × ~50 bytes): ~400 bytes
  Total: ~500 bytes per probe

Probe rate: 1/second per node

Bandwidth per node:
  Outgoing: 1 ping + 1 ack = ~1KB/s
  Incoming: ~1KB/s (from being probed)
  Total: ~2KB/s per node

For 100-node cluster:
  Each node: ~2KB/s
  Total cluster: ~200KB/s
  Negligible compared to data transfer
```

### 7.2 Convergence Time

```
Time for all N nodes to learn about a new event:

  Expected rounds: O(log N)
  Each round: probe_interval (1s)

  N=3:   ~2 rounds = 2s
  N=10:  ~4 rounds = 4s
  N=50:  ~6 rounds = 6s
  N=100: ~7 rounds = 7s
```

### 7.3 Failure Detection Latency

```
Best case (direct probe detects failure):
  1 probe_interval + 1 probe_timeout = 1.5s

Worst case (indirect probe needed + suspicion timeout):
  1 probe_interval + 2 × probe_timeout + suspicion_timeout
  = 1s + 1s + 5.5s = 7.5s (for N=3)
  = 1s + 1s + 18.5s = 20.5s (for N=100)
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/lib.rs` | Replace stub with module declarations |
| `deriva-network/src/types.rs` | **NEW** — `NodeId`, `MemberState`, `Member`, `NodeMetadata`, `SwimEvent` |
| `deriva-network/src/message.rs` | **NEW** — `SwimMessage`, encode/decode |
| `deriva-network/src/memberlist.rs` | **NEW** — `MemberList` with SWIM state machine |
| `deriva-network/src/runtime.rs` | **NEW** — `SwimRuntime` with probe/receive/cleanup loops |
| `deriva-network/src/config.rs` | **NEW** — `SwimConfig` |
| `deriva-network/Cargo.toml` | Add dependencies |
| `deriva-server/src/service.rs` | Add `SwimRuntime` to `ServerState`, metadata refresh, `cluster_status` RPC |
| `proto/deriva.proto` | Add `ClusterStatus` RPC + messages |
| `deriva-cli/src/main.rs` | Add `cluster` command |
| `deriva-network/tests/` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

| Crate | Dependency | Version | Reason |
|-------|-----------|---------|--------|
| `deriva-network` | `tokio` | `1.x` | Async UDP, timers, tasks |
| `deriva-network` | `serde` | `1.x` | Message serialization |
| `deriva-network` | `bincode` | `1.x` | Compact binary encoding for UDP |
| `deriva-network` | `rand` | `0.8` | Random peer selection |

---

## 10. Design Rationale

### 10.1 Why UDP Instead of TCP for SWIM?

- SWIM messages are small (<1KB) — no need for TCP's stream semantics
- UDP is connectionless — no overhead for maintaining connections to all peers
- Packet loss is handled by gossip redundancy (piggyback retransmit)
- Lower latency — no TCP handshake for each probe

### 10.2 Why Bincode Instead of Protobuf for SWIM Messages?

- SWIM messages are internal (node-to-node), not client-facing
- Bincode is simpler (derive Serialize/Deserialize) and more compact
- No need for schema evolution — SWIM protocol is versioned with Deriva
- Protobuf is used for client-facing gRPC (different concern)

### 10.3 Why Bloom Filter Instead of Full Cache List?

- Full cache list: 10,000 addrs × 32 bytes = 320KB per node per gossip
- Bloom filter: 12KB for 10,000 addrs at 1% FPR
- 26× smaller — fits easily in UDP messages
- False positives (1%) are acceptable — worst case is a wasted remote check

---

## 11. Observability Integration

```rust
lazy_static! {
    static ref SWIM_PROBES_TOTAL: IntCounter = register_int_counter!(
        "deriva_swim_probes_total", "Total SWIM probes sent"
    ).unwrap();
    static ref SWIM_PROBE_FAILURES: IntCounter = register_int_counter!(
        "deriva_swim_probe_failures_total", "SWIM probes with no ack"
    ).unwrap();
    static ref SWIM_MEMBERS_ALIVE: Gauge = register_gauge!(
        "deriva_swim_members_alive", "Alive cluster members"
    ).unwrap();
    static ref SWIM_MEMBERS_SUSPECT: Gauge = register_gauge!(
        "deriva_swim_members_suspect", "Suspect cluster members"
    ).unwrap();
    static ref SWIM_GOSSIP_BYTES: IntCounter = register_int_counter!(
        "deriva_swim_gossip_bytes_total", "Total gossip bytes sent"
    ).unwrap();
}
```

---

## 12. Checklist

- [ ] Create `deriva-network/src/types.rs` with core types
- [ ] Create `deriva-network/src/message.rs` with wire format
- [ ] Create `deriva-network/src/memberlist.rs` with SWIM state machine
- [ ] Create `deriva-network/src/runtime.rs` with SwimRuntime
- [ ] Create `deriva-network/src/config.rs` with SwimConfig
- [ ] Implement probe loop with direct + indirect probing
- [ ] Implement suspicion timeout with logarithmic scaling
- [ ] Implement incarnation-based refutation
- [ ] Implement piggybacked gossip dissemination
- [ ] Implement bloom filter for cache metadata
- [ ] Add metadata refresh loop to server
- [ ] Add `ClusterStatus` RPC
- [ ] Add `cluster` CLI command
- [ ] Write unit tests for NodeId, MemberList, Message (~12 tests)
- [ ] Write integration tests for 2-node discovery + failure detection (~2 tests)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing tests still pass
- [ ] Commit and push
