# §3.17 Rolling Upgrades & Version Negotiation

> **Status:** Not started
> **Depends on:** §3.1 SWIM Gossip, §3.8 Cluster Bootstrap, §3.15 Admin API (DrainNode)
> **Crate(s):** `deriva-network`, `deriva-server`, `deriva-core`
> **Estimated effort:** 3 days

---

## 1. Problem Statement

A distributed system that requires downtime for upgrades is a distributed system that doesn't get upgraded. Operators delay upgrades, accumulating technical debt and security vulnerabilities. Deriva must support zero-downtime rolling upgrades where nodes are upgraded one at a time while the cluster continues serving requests.

Rolling upgrades introduce fundamental challenges:

1. **Protocol compatibility**: Node v2 sends a new RPC field. Node v1 doesn't understand it. Request fails.
2. **Mixed-version clusters**: During a rolling upgrade, nodes run different versions simultaneously. Every RPC must work across version boundaries.
3. **Feature gating**: New features can't be activated until all nodes support them. A single lagging node blocks the entire cluster.
4. **Rollback safety**: If v2 has a bug, operators must be able to roll back to v1 without data loss or corruption.
5. **Schema evolution**: Protobuf messages evolve. New fields, deprecated fields, renamed enums. Wire compatibility must be maintained.

### Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| R1 | Nodes advertise their protocol version via SWIM metadata | P0 |
| R2 | Cluster-wide minimum version tracked (min of all alive nodes) | P0 |
| R3 | Feature flags gated on cluster minimum version | P0 |
| R4 | Protobuf backward compatibility enforced (additive-only changes) | P0 |
| R5 | Version handshake on gRPC connection establishment | P1 |
| R6 | Graceful degradation: new node talks to old node using old protocol | P0 |
| R7 | Upgrade orchestrator: drain → upgrade → rejoin, one node at a time | P1 |
| R8 | Rollback: downgrade node without cluster disruption | P0 |
| R9 | Minimum supported version floor (reject nodes too old) | P1 |
| R10 | Upgrade status visible via Admin API | P1 |

### Non-Goals

- Blue-green deployment (separate clusters with traffic switching)
- Canary deployments (percentage-based traffic splitting)
- Automatic upgrade triggering (operator-initiated only)

---

## 2. Design

### 2.1 Version Model

```
  Deriva uses a 3-level version scheme:

  ┌─────────────────────────────────────────────────────┐
  │  Protocol Version: u32                              │
  │                                                     │
  │  Incremented when wire protocol changes:            │
  │    - New RPC added                                  │
  │    - New required field in existing message          │
  │    - Behavioral change in existing RPC              │
  │                                                     │
  │  NOT incremented for:                               │
  │    - Bug fixes                                      │
  │    - Performance improvements                       │
  │    - New optional fields (protobuf handles these)   │
  │                                                     │
  │  Current: PROTOCOL_VERSION = 1                      │
  │  Min supported: MIN_PROTOCOL_VERSION = 1            │
  └─────────────────────────────────────────────────────┘

  ┌─────────────────────────────────────────────────────┐
  │  Feature Version: u32                               │
  │                                                     │
  │  Incremented when new cluster-wide feature added:   │
  │    - New GC algorithm                               │
  │    - New rebalancing strategy                       │
  │    - New consistency mode                           │
  │                                                     │
  │  Features gated on: cluster_min_feature_version     │
  │  Feature only activates when ALL nodes support it.  │
  │                                                     │
  │  Current: FEATURE_VERSION = 1                       │
  └─────────────────────────────────────────────────────┘

  ┌─────────────────────────────────────────────────────┐
  │  Build Version: String (semver)                     │
  │                                                     │
  │  e.g., "0.3.2"                                      │
  │  Informational only. Not used for compatibility.    │
  │  Displayed in Admin API, logs, metrics.             │
  └─────────────────────────────────────────────────────┘
```

### 2.2 Architecture

```
  ┌──────────────────────────────────────────────────────────┐
  │                    Node Startup                           │
  │                                                           │
  │  1. Set local versions:                                   │
  │     protocol_version = 1                                  │
  │     feature_version = 1                                   │
  │     build_version = "0.3.2"                               │
  │                                                           │
  │  2. Advertise via SWIM metadata:                          │
  │     tags: ["pv:1", "fv:1", "bv:0.3.2"]                   │
  │                                                           │
  │  3. On join, check peers:                                 │
  │     If any peer.protocol_version < MIN_PROTOCOL_VERSION:  │
  │       warn (peer too old, may be evicted)                 │
  │     If local.protocol_version < cluster_min:              │
  │       refuse to join (we're too old)                      │
  └──────────────────────────────────────────────────────────┘

  ┌──────────────────────────────────────────────────────────┐
  │              Version Tracker (background)                 │
  │                                                           │
  │  Watches SWIM membership changes.                         │
  │  Computes:                                                │
  │    cluster_min_protocol = min(alive_nodes.protocol_ver)   │
  │    cluster_min_feature  = min(alive_nodes.feature_ver)    │
  │                                                           │
  │  Updates FeatureGate with new minimums.                   │
  │  Emits metrics: cluster_protocol_version,                 │
  │                 cluster_feature_version,                   │
  │                 nodes_by_version gauge.                    │
  └──────────────────────────────────────────────────────────┘

  ┌──────────────────────────────────────────────────────────┐
  │              Feature Gate                                  │
  │                                                           │
  │  is_enabled("streaming_gc") →                             │
  │    features["streaming_gc"].min_feature_version            │
  │    <= cluster_min_feature_version                          │
  │                                                           │
  │  Code paths check FeatureGate before using new features.  │
  │  Old code path used as fallback when feature not enabled. │
  └──────────────────────────────────────────────────────────┘
```

### 2.3 Wire Protocol — Version Handshake

```protobuf
// Added to proto/deriva.proto

message VersionInfo {
  uint32 protocol_version = 1;
  uint32 feature_version = 2;
  string build_version = 3;
  uint32 min_protocol_version = 4;
}

// Sent as gRPC metadata on first RPC to a peer
// Key: "x-deriva-version"
// Value: "<protocol_version>:<feature_version>:<build_version>"
//
// Receiver checks:
//   If sender.protocol_version < my.min_protocol_version → reject
//   If sender.protocol_version > my.protocol_version → use my version's behavior
```

### 2.4 Core Types

```rust
// deriva-core/src/version.rs

pub const PROTOCOL_VERSION: u32 = 1;
pub const FEATURE_VERSION: u32 = 1;
pub const MIN_PROTOCOL_VERSION: u32 = 1;
pub const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVersion {
    pub protocol: u32,
    pub feature: u32,
    pub build: String,
    pub min_protocol: u32,
}

impl NodeVersion {
    pub fn current() -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            feature: FEATURE_VERSION,
            build: BUILD_VERSION.to_string(),
            min_protocol: MIN_PROTOCOL_VERSION,
        }
    }

    pub fn to_swim_tags(&self) -> Vec<String> {
        vec![
            format!("pv:{}", self.protocol),
            format!("fv:{}", self.feature),
            format!("bv:{}", self.build),
        ]
    }

    pub fn from_swim_tags(tags: &[String]) -> Option<Self> {
        let pv = tags.iter().find_map(|t| t.strip_prefix("pv:"))?.parse().ok()?;
        let fv = tags.iter().find_map(|t| t.strip_prefix("fv:"))?.parse().ok()?;
        let bv = tags.iter().find_map(|t| t.strip_prefix("bv:"))?.to_string();
        Some(Self { protocol: pv, feature: fv, build: bv, min_protocol: MIN_PROTOCOL_VERSION })
    }

    pub fn is_compatible_with(&self, peer: &NodeVersion) -> bool {
        peer.protocol >= self.min_protocol && self.protocol >= peer.min_protocol
    }
}
```

```rust
// deriva-network/src/version_tracker.rs

pub struct VersionTracker {
    swim: Arc<SwimCluster>,
    cluster_min_protocol: AtomicU32,
    cluster_min_feature: AtomicU32,
    feature_gate: Arc<FeatureGate>,
}

impl VersionTracker {
    pub fn new(swim: Arc<SwimCluster>, feature_gate: Arc<FeatureGate>) -> Self {
        Self {
            swim,
            cluster_min_protocol: AtomicU32::new(PROTOCOL_VERSION),
            cluster_min_feature: AtomicU32::new(FEATURE_VERSION),
            feature_gate,
        }
    }

    pub fn start(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                self.recompute();
            }
        })
    }

    fn recompute(&self) {
        let members = self.swim.alive_members();
        if members.is_empty() { return; }

        let mut min_pv = u32::MAX;
        let mut min_fv = u32::MAX;

        for m in &members {
            if let Some(ver) = NodeVersion::from_swim_tags(&m.tags) {
                min_pv = min_pv.min(ver.protocol);
                min_fv = min_fv.min(ver.feature);
            }
            // Nodes without version tags treated as version 1
        }

        let min_pv = if min_pv == u32::MAX { 1 } else { min_pv };
        let min_fv = if min_fv == u32::MAX { 1 } else { min_fv };

        let old_pv = self.cluster_min_protocol.swap(min_pv, Ordering::Relaxed);
        let old_fv = self.cluster_min_feature.swap(min_fv, Ordering::Relaxed);

        if min_pv != old_pv || min_fv != old_fv {
            tracing::info!(
                min_protocol = min_pv,
                min_feature = min_fv,
                "cluster version changed"
            );
        }

        self.feature_gate.update_cluster_version(min_fv);
        metrics::gauge!("cluster_min_protocol_version").set(min_pv as f64);
        metrics::gauge!("cluster_min_feature_version").set(min_fv as f64);
    }

    pub fn cluster_min_protocol(&self) -> u32 {
        self.cluster_min_protocol.load(Ordering::Relaxed)
    }

    pub fn cluster_min_feature(&self) -> u32 {
        self.cluster_min_feature.load(Ordering::Relaxed)
    }
}
```

```rust
// deriva-network/src/feature_gate.rs

pub struct FeatureGate {
    features: Vec<FeatureDef>,
    cluster_min_feature: AtomicU32,
}

struct FeatureDef {
    name: &'static str,
    min_feature_version: u32,
    description: &'static str,
}

impl FeatureGate {
    pub fn new() -> Self {
        Self {
            features: vec![
                FeatureDef {
                    name: "streaming_gc",
                    min_feature_version: 2,
                    description: "Streaming garbage collection with incremental sweeps",
                },
                FeatureDef {
                    name: "batch_internal_rpc",
                    min_feature_version: 2,
                    description: "Peer-to-peer batch fetch RPC (reduces scatter RPCs)",
                },
                FeatureDef {
                    name: "adaptive_replication",
                    min_feature_version: 3,
                    description: "Dynamic replication factor based on access patterns",
                },
            ],
            cluster_min_feature: AtomicU32::new(1),
        }
    }

    pub fn update_cluster_version(&self, min_feature: u32) {
        self.cluster_min_feature.store(min_feature, Ordering::Relaxed);
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        let min = self.cluster_min_feature.load(Ordering::Relaxed);
        self.features.iter()
            .find(|f| f.name == name)
            .map(|f| min >= f.min_feature_version)
            .unwrap_or(false)
    }

    pub fn all_features(&self) -> Vec<(&str, bool, u32)> {
        let min = self.cluster_min_feature.load(Ordering::Relaxed);
        self.features.iter()
            .map(|f| (f.name, min >= f.min_feature_version, f.min_feature_version))
            .collect()
    }
}
```

---

## 3. Implementation

### 3.1 Version Interceptor (gRPC)

```rust
// deriva-server/src/version_interceptor.rs

pub fn version_interceptor(
    local_version: NodeVersion,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    move |req: Request<()>| {
        if let Some(peer_ver_str) = req.metadata().get("x-deriva-version") {
            let peer_ver_str = peer_ver_str.to_str()
                .map_err(|_| Status::invalid_argument("invalid version header"))?;

            let parts: Vec<&str> = peer_ver_str.split(':').collect();
            if parts.len() >= 2 {
                let peer_pv: u32 = parts[0].parse().unwrap_or(1);
                if peer_pv < local_version.min_protocol {
                    return Err(Status::failed_precondition(format!(
                        "peer protocol version {} below minimum {}",
                        peer_pv, local_version.min_protocol
                    )));
                }
            }
        }
        // No version header = legacy node, treat as version 1
        Ok(req)
    }
}

/// Outgoing interceptor: attach version to all RPCs
pub fn version_outgoing_interceptor(
    local_version: NodeVersion,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    let header_value = format!("{}:{}:{}",
        local_version.protocol, local_version.feature, local_version.build);
    move |mut req: Request<()>| {
        req.metadata_mut().insert(
            "x-deriva-version",
            header_value.parse().unwrap(),
        );
        Ok(req)
    }
}
```

### 3.2 Rolling Upgrade Orchestrator

```rust
// deriva-network/src/upgrade.rs

pub struct UpgradeOrchestrator {
    admin_clients: HashMap<NodeId, AdminServiceClient<Channel>>,
    target_version: String,
}

#[derive(Debug, Clone)]
pub struct UpgradeStatus {
    pub total_nodes: usize,
    pub upgraded: Vec<NodeId>,
    pub pending: Vec<NodeId>,
    pub current_node: Option<NodeId>,
    pub phase: UpgradePhase,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpgradePhase {
    NotStarted,
    Draining(NodeId),
    WaitingDrain(NodeId),
    Upgrading(NodeId),
    Rejoining(NodeId),
    WaitingHealthy(NodeId),
    Complete,
    Failed(String),
}

impl UpgradeOrchestrator {
    /// Upgrade one node. Called by operator for each node in sequence.
    pub async fn upgrade_node(
        &self,
        node_id: NodeId,
        coordinator: &AdminServiceClient<Channel>,
    ) -> Result<UpgradePhase, DerivaError> {
        // Phase 1: Drain
        tracing::info!(node_id = node_id.0, "starting node upgrade: drain");
        coordinator.drain_node(DrainNodeRequest { node_id: node_id.0 })
            .await.map_err(|e| DerivaError::Remote(e.to_string()))?;

        // Phase 2: Wait for drain to complete
        tracing::info!(node_id = node_id.0, "waiting for drain to complete");
        self.wait_for_drain(node_id, coordinator).await?;

        // Phase 3: Signal operator to upgrade the binary
        tracing::info!(node_id = node_id.0, "drain complete, ready for binary upgrade");
        Ok(UpgradePhase::Upgrading(node_id))

        // Phases 4-5 (rejoin + health check) happen automatically
        // when the node restarts with the new binary.
    }

    async fn wait_for_drain(
        &self,
        node_id: NodeId,
        coordinator: &AdminServiceClient<Channel>,
    ) -> Result<(), DerivaError> {
        let timeout = Duration::from_secs(300);
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err(DerivaError::Timeout("drain did not complete in 5 minutes".into()));
            }

            let stats = coordinator.get_node_stats(NodeStatsRequest {
                node_id: Some(node_id.0),
            }).await;

            match stats {
                Ok(resp) => {
                    let s = resp.into_inner().storage.unwrap_or_default();
                    if s.blob_count == 0 && s.recipe_count == 0 {
                        return Ok(());
                    }
                    tracing::info!(
                        node_id = node_id.0,
                        blobs = s.blob_count,
                        recipes = s.recipe_count,
                        "drain in progress"
                    );
                }
                Err(_) => {
                    // Node may have already shut down
                    return Ok(());
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
```

### 3.3 Protobuf Compatibility Rules

```rust
// deriva-core/src/version.rs (additions)

/// Protobuf compatibility rules enforced by code review + CI:
///
/// ALLOWED changes (backward compatible):
///   - Add new optional field (with new field number)
///   - Add new RPC to service
///   - Add new enum value (not reusing number)
///   - Add new message type
///   - Deprecate field (keep field number reserved)
///
/// FORBIDDEN changes (breaking):
///   - Remove or rename existing field
///   - Change field number
///   - Change field type
///   - Remove enum value
///   - Remove RPC
///   - Change RPC request/response type
///
/// When breaking change is unavoidable:
///   1. Increment PROTOCOL_VERSION
///   2. Increment MIN_PROTOCOL_VERSION (after all nodes upgraded)
///   3. Add new RPC alongside old one
///   4. Old RPC deprecated, removed after 2 versions
///
/// CI check: `buf breaking --against .git#branch=main`
```

### 3.4 Admin API Extensions

```rust
// Added to AdminService

async fn get_upgrade_status(
    &self, _req: Request<GetUpgradeStatusRequest>,
) -> Result<Response<UpgradeStatusResponse>, Status> {
    let members = self.state.swim.alive_members();
    let mut version_counts: HashMap<String, u32> = HashMap::new();

    for m in &members {
        let ver = NodeVersion::from_swim_tags(&m.tags)
            .map(|v| v.build)
            .unwrap_or_else(|| "unknown".to_string());
        *version_counts.entry(ver).or_default() += 1;
    }

    let features = self.state.feature_gate.all_features();
    let feature_status: Vec<FeatureStatus> = features.iter().map(|(name, enabled, min_ver)| {
        FeatureStatus {
            name: name.to_string(),
            enabled: *enabled,
            required_version: *min_ver,
        }
    }).collect();

    let cluster_min_pv = self.state.version_tracker.cluster_min_protocol();
    let cluster_min_fv = self.state.version_tracker.cluster_min_feature();

    Ok(Response::new(UpgradeStatusResponse {
        cluster_min_protocol_version: cluster_min_pv,
        cluster_min_feature_version: cluster_min_fv,
        node_versions: version_counts,
        features: feature_status,
        mixed_versions: version_counts.len() > 1,
    }))
}
```

```protobuf
// proto/admin.proto additions

message GetUpgradeStatusRequest {}

message UpgradeStatusResponse {
  uint32 cluster_min_protocol_version = 1;
  uint32 cluster_min_feature_version = 2;
  map<string, uint32> node_versions = 3;  // build_version → count
  repeated FeatureStatus features = 4;
  bool mixed_versions = 5;
}

message FeatureStatus {
  string name = 1;
  bool enabled = 2;
  uint32 required_version = 3;
}
```

### 3.5 Join-Time Version Check

```rust
// deriva-network/src/swim.rs (additions to join handler)

impl SwimCluster {
    pub fn handle_join_request(&self, peer: &SwimMember) -> Result<(), DerivaError> {
        let local = NodeVersion::current();

        if let Some(peer_ver) = NodeVersion::from_swim_tags(&peer.tags) {
            if !local.is_compatible_with(&peer_ver) {
                tracing::warn!(
                    peer_id = peer.node_id.0,
                    peer_protocol = peer_ver.protocol,
                    local_min = local.min_protocol,
                    "rejecting incompatible peer"
                );
                return Err(DerivaError::IncompatibleVersion(format!(
                    "peer protocol {} < minimum {}",
                    peer_ver.protocol, local.min_protocol
                )));
            }
        }
        // No version tags = legacy v1 node, allowed if MIN_PROTOCOL_VERSION == 1
        Ok(())
    }
}
```


---

## 4. Data Flow Diagrams

### 4.1 Rolling Upgrade — 3-Node Cluster (v1 → v2)

```
  Time    Node A (v1)         Node B (v1)         Node C (v1)
  ─────────────────────────────────────────────────────────────
  t=0     Alive (v1)          Alive (v1)          Alive (v1)
          cluster_min_pv=1    cluster_min_fv=1

  ── Upgrade Node C ──────────────────────────────────────────
  t=1     Alive               Alive               DRAINING
          (data migrating from C to A,B)

  t=2     Alive               Alive               DOWN (stopped)
          cluster_min_pv=1 (C excluded, only A,B counted)

  t=3     Alive               Alive               Alive (v2)
          cluster_min_pv=1    (v2 joins, min still 1)

  ── Upgrade Node B ──────────────────────────────────────────
  t=4     Alive               DRAINING            Alive (v2)

  t=5     Alive               DOWN (stopped)      Alive (v2)
          cluster_min_pv=1 (A=v1, C=v2, min=1)

  t=6     Alive               Alive (v2)          Alive (v2)
          cluster_min_pv=1 (A still v1)

  ── Upgrade Node A ──────────────────────────────────────────
  t=7     DRAINING            Alive (v2)          Alive (v2)

  t=8     DOWN (stopped)      Alive (v2)          Alive (v2)
          cluster_min_pv=2 (only v2 nodes)

  t=9     Alive (v2)          Alive (v2)          Alive (v2)
          cluster_min_pv=2    cluster_min_fv=2
          ✓ New features now enabled!
```

### 4.2 Version Handshake on RPC

```
  Node A (v1, pv=1)          Node B (v2, pv=2, min_pv=1)
       │                           │
       │── FetchValue ────────────►│
       │   metadata:               │
       │   x-deriva-version: 1:1:0.3.0
       │                           │
       │                     Check: peer_pv=1 >= my_min_pv=1? ✓
       │                     Process using v1-compatible behavior
       │                           │
       │◄── Response ──────────────┘
       │   (no new v2-only fields)
       │
  Later, after A upgraded to v2:
       │
       │── FetchValue ────────────►│
       │   x-deriva-version: 2:2:0.4.0
       │                           │
       │                     Check: peer_pv=2 >= my_min_pv=1? ✓
       │                     Process using v2 behavior (full features)
       │                           │
       │◄── Response ──────────────┘
       │   (includes new v2 fields)
```

### 4.3 Feature Gate Activation

```
  3-node cluster upgrading v1 → v2:

  ┌─────────────────────────────────────────────────────────┐
  │ Feature: "streaming_gc" (requires feature_version >= 2) │
  │                                                         │
  │ State 1: A=v1, B=v1, C=v1                              │
  │   cluster_min_fv = 1                                    │
  │   streaming_gc: DISABLED                                │
  │                                                         │
  │ State 2: A=v1, B=v1, C=v2                              │
  │   cluster_min_fv = 1 (A and B still v1)                │
  │   streaming_gc: DISABLED                                │
  │                                                         │
  │ State 3: A=v1, B=v2, C=v2                              │
  │   cluster_min_fv = 1 (A still v1)                      │
  │   streaming_gc: DISABLED                                │
  │                                                         │
  │ State 4: A=v2, B=v2, C=v2                              │
  │   cluster_min_fv = 2 (all v2!)                         │
  │   streaming_gc: ENABLED ✓                               │
  │                                                         │
  │ Feature activates ONLY when last node upgrades.         │
  │ Safe: no node receives messages it can't understand.    │
  └─────────────────────────────────────────────────────────┘
```

### 4.4 Rollback Scenario

```
  Cluster: A=v2, B=v2, C=v2 (all upgraded)
  Bug found in v2! Rollback C to v1.

  ┌─────────────────────────────────────────────────────────┐
  │ Before rollback:                                        │
  │   cluster_min_fv = 2                                    │
  │   streaming_gc: ENABLED                                 │
  │                                                         │
  │ After C rolls back to v1:                               │
  │   cluster_min_fv = 1 (C is v1 again)                   │
  │   streaming_gc: DISABLED (automatically!)               │
  │                                                         │
  │ Feature gate reacts within 5s (tracker interval).       │
  │ No manual intervention needed.                          │
  │ v2 nodes fall back to v1 code paths.                    │
  └─────────────────────────────────────────────────────────┘

  Requirement: v2 code must still support v1 behavior.
  Feature-gated code always has a fallback path.
```

### 4.5 Incompatible Node Rejected

```
  Cluster: A=v3, B=v3 (min_protocol=2)
  Old node C=v1 tries to join:

  Node C (v1)              Node A (v3, min_pv=2)
     │                           │
     │── SWIM Join ─────────────►│
     │   tags: [pv:1, fv:1]     │
     │                           │
     │                     Check: peer_pv=1 < min_pv=2
     │                     REJECT
     │                           │
     │◄── Join Rejected ────────┘
     │   "protocol version 1 below minimum 2"
     │
     │  Node C logs error and exits.
     │  Operator must upgrade C to at least v2 before joining.
```

---

## 5. Test Specification

### 5.1 NodeVersion Tests

```rust
#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn test_version_to_swim_tags() {
        let ver = NodeVersion {
            protocol: 2, feature: 3, build: "0.4.0".into(), min_protocol: 1,
        };
        let tags = ver.to_swim_tags();
        assert_eq!(tags, vec!["pv:2", "fv:3", "bv:0.4.0"]);
    }

    #[test]
    fn test_version_from_swim_tags() {
        let tags = vec!["pv:2".into(), "fv:3".into(), "bv:0.4.0".into()];
        let ver = NodeVersion::from_swim_tags(&tags).unwrap();
        assert_eq!(ver.protocol, 2);
        assert_eq!(ver.feature, 3);
        assert_eq!(ver.build, "0.4.0");
    }

    #[test]
    fn test_version_from_swim_tags_missing() {
        let tags = vec!["pv:2".into()]; // missing fv and bv
        assert!(NodeVersion::from_swim_tags(&tags).is_none());
    }

    #[test]
    fn test_version_compatibility_same() {
        let v1 = NodeVersion { protocol: 1, feature: 1, build: "0.1.0".into(), min_protocol: 1 };
        let v2 = NodeVersion { protocol: 1, feature: 1, build: "0.1.1".into(), min_protocol: 1 };
        assert!(v1.is_compatible_with(&v2));
        assert!(v2.is_compatible_with(&v1));
    }

    #[test]
    fn test_version_compatibility_newer_ok() {
        let v1 = NodeVersion { protocol: 1, feature: 1, build: "0.1.0".into(), min_protocol: 1 };
        let v2 = NodeVersion { protocol: 2, feature: 2, build: "0.2.0".into(), min_protocol: 1 };
        assert!(v1.is_compatible_with(&v2)); // v2.protocol >= v1.min
        assert!(v2.is_compatible_with(&v1)); // v1.protocol >= v2.min
    }

    #[test]
    fn test_version_incompatible() {
        let v1 = NodeVersion { protocol: 1, feature: 1, build: "0.1.0".into(), min_protocol: 1 };
        let v3 = NodeVersion { protocol: 3, feature: 3, build: "0.3.0".into(), min_protocol: 2 };
        assert!(v1.is_compatible_with(&v3));  // v3.protocol >= v1.min(1) ✓
        assert!(!v3.is_compatible_with(&v1)); // v1.protocol(1) < v3.min(2) ✗
    }
}
```

### 5.2 VersionTracker Tests

```rust
#[tokio::test]
async fn test_tracker_computes_min_versions() {
    let swim = mock_swim_with_members(vec![
        ("pv:2", "fv:3"),
        ("pv:1", "fv:2"),
        ("pv:3", "fv:2"),
    ]);
    let gate = Arc::new(FeatureGate::new());
    let tracker = VersionTracker::new(swim, gate.clone());

    tracker.recompute();

    assert_eq!(tracker.cluster_min_protocol(), 1);
    assert_eq!(tracker.cluster_min_feature(), 2);
}

#[tokio::test]
async fn test_tracker_ignores_down_nodes() {
    let swim = mock_swim_with_members_and_states(vec![
        ("pv:2", "fv:3", SwimState::Alive),
        ("pv:1", "fv:1", SwimState::Down),  // should be excluded
        ("pv:2", "fv:2", SwimState::Alive),
    ]);
    let gate = Arc::new(FeatureGate::new());
    let tracker = VersionTracker::new(swim, gate);

    tracker.recompute();

    // Down node excluded → min is 2, not 1
    assert_eq!(tracker.cluster_min_protocol(), 2);
    assert_eq!(tracker.cluster_min_feature(), 2);
}

#[tokio::test]
async fn test_tracker_empty_cluster() {
    let swim = mock_swim_with_members(vec![]);
    let gate = Arc::new(FeatureGate::new());
    let tracker = VersionTracker::new(swim, gate);

    tracker.recompute();

    // No change from initial values
    assert_eq!(tracker.cluster_min_protocol(), PROTOCOL_VERSION);
}
```

### 5.3 FeatureGate Tests

```rust
#[test]
fn test_feature_disabled_below_version() {
    let gate = FeatureGate::new();
    gate.update_cluster_version(1);
    assert!(!gate.is_enabled("streaming_gc")); // requires fv >= 2
}

#[test]
fn test_feature_enabled_at_version() {
    let gate = FeatureGate::new();
    gate.update_cluster_version(2);
    assert!(gate.is_enabled("streaming_gc")); // requires fv >= 2
}

#[test]
fn test_feature_enabled_above_version() {
    let gate = FeatureGate::new();
    gate.update_cluster_version(5);
    assert!(gate.is_enabled("streaming_gc"));
    assert!(gate.is_enabled("batch_internal_rpc"));
    assert!(gate.is_enabled("adaptive_replication")); // requires fv >= 3
}

#[test]
fn test_unknown_feature_disabled() {
    let gate = FeatureGate::new();
    gate.update_cluster_version(100);
    assert!(!gate.is_enabled("nonexistent_feature"));
}

#[test]
fn test_all_features_lists_status() {
    let gate = FeatureGate::new();
    gate.update_cluster_version(2);
    let features = gate.all_features();
    assert!(features.len() >= 3);

    let sg = features.iter().find(|(n, _, _)| *n == "streaming_gc").unwrap();
    assert!(sg.1); // enabled

    let ar = features.iter().find(|(n, _, _)| *n == "adaptive_replication").unwrap();
    assert!(!ar.1); // not enabled (requires 3)
}
```

### 5.4 Version Interceptor Tests

```rust
#[test]
fn test_interceptor_accepts_compatible_version() {
    let local = NodeVersion { protocol: 2, feature: 2, build: "0.2.0".into(), min_protocol: 1 };
    let interceptor = version_interceptor(local);

    let mut req = Request::new(());
    req.metadata_mut().insert("x-deriva-version", "1:1:0.1.0".parse().unwrap());

    assert!(interceptor(req).is_ok());
}

#[test]
fn test_interceptor_rejects_too_old() {
    let local = NodeVersion { protocol: 3, feature: 3, build: "0.3.0".into(), min_protocol: 2 };
    let interceptor = version_interceptor(local);

    let mut req = Request::new(());
    req.metadata_mut().insert("x-deriva-version", "1:1:0.1.0".parse().unwrap());

    let result = interceptor(req);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::FailedPrecondition);
}

#[test]
fn test_interceptor_allows_no_header() {
    let local = NodeVersion { protocol: 1, feature: 1, build: "0.1.0".into(), min_protocol: 1 };
    let interceptor = version_interceptor(local);

    let req = Request::new(());
    assert!(interceptor(req).is_ok()); // legacy node, treated as v1
}

#[test]
fn test_outgoing_interceptor_attaches_header() {
    let local = NodeVersion { protocol: 2, feature: 3, build: "0.4.0".into(), min_protocol: 1 };
    let interceptor = version_outgoing_interceptor(local);

    let req = Request::new(());
    let req = interceptor(req).unwrap();

    let val = req.metadata().get("x-deriva-version").unwrap().to_str().unwrap();
    assert_eq!(val, "2:3:0.4.0");
}
```

### 5.5 Integration Tests

```rust
#[tokio::test]
async fn test_mixed_version_cluster() {
    // Start 2 nodes at "v1"
    let (nodes, _handles) = start_cluster_with_version(2, 1, 1).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Verify cluster min = 1
    let mut admin = connect_admin(&nodes[0]).await;
    let status = admin.get_upgrade_status(GetUpgradeStatusRequest {})
        .await.unwrap().into_inner();
    assert_eq!(status.cluster_min_protocol_version, 1);
    assert!(!status.mixed_versions);

    // Add a "v2" node
    let v2_node = start_node_with_version(2, 2).await;
    v2_node.join_cluster(&nodes[0]).await;
    tokio::time::sleep(Duration::from_secs(10)).await;

    let status = admin.get_upgrade_status(GetUpgradeStatusRequest {})
        .await.unwrap().into_inner();
    assert_eq!(status.cluster_min_protocol_version, 1); // still 1
    assert!(status.mixed_versions);
}

#[tokio::test]
async fn test_feature_activates_after_full_upgrade() {
    let (nodes, _handles) = start_cluster_with_version(3, 1, 1).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    assert!(!nodes[0].feature_gate().is_enabled("streaming_gc"));

    // Upgrade all nodes to feature_version=2
    for node in &nodes {
        node.set_feature_version(2);
    }
    tokio::time::sleep(Duration::from_secs(10)).await;

    assert!(nodes[0].feature_gate().is_enabled("streaming_gc"));
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Node without version tags joins | Treated as protocol=1, feature=1 | Backward compat with pre-versioning nodes |
| 2 | All nodes same version | cluster_min = that version, no mixed flag | Normal steady state |
| 3 | Single node cluster upgrade | Drain has nothing to migrate, instant | Degenerate but valid |
| 4 | Node crashes during drain | Data on crashed node unavailable until replicas serve it | Replication handles this |
| 5 | Rollback to v1 after v2 features activated | Feature gate disables features within 5s | Automatic, no manual intervention |
| 6 | Two nodes upgrade simultaneously | Both drain at once → reduced capacity | Operator error; docs warn against this |
| 7 | Version tracker sees stale SWIM data | Min version may lag by up to 5s | Acceptable: features activate conservatively |
| 8 | Malformed version tag in SWIM | Node treated as v1 (from_swim_tags returns None) | Fail-safe: assume oldest |
| 9 | Protocol version overflow (u32::MAX) | Theoretical: 4 billion versions | Not a practical concern |
| 10 | Feature gate checked on hot path | AtomicU32 load: ~1ns | Negligible overhead |
| 11 | Operator forgets to upgrade one node | cluster_min stays at old version, features blocked | Safe: features don't activate prematurely |
| 12 | New node joins with higher min_protocol than cluster | Existing nodes with lower protocol rejected by new node | New node should use min_protocol ≤ cluster version |

### 6.1 Protobuf Forward Compatibility

```
  Node v1 receives a message with unknown fields (added in v2):

  Protobuf behavior:
    Unknown fields are preserved in the wire format.
    v1 node ignores them (doesn't crash).
    If v1 forwards the message, unknown fields are retained.

  This is why additive-only changes are safe:
    v2 adds optional field "priority" to FetchValueRequest.
    v1 receives it, ignores "priority", processes normally.
    v2 sends to v1: no priority-based behavior, but no error.

  Breaking change example (FORBIDDEN):
    v2 changes field 3 from string to int32.
    v1 tries to decode int32 as string → deserialization error.
    This is why we never change field types.
```

### 6.2 Upgrade Order Recommendations

```
  Recommended upgrade order for a 5-node cluster:

  1. Upgrade non-primary nodes first (nodes that own fewer ring tokens).
     Less data to migrate during drain.

  2. Upgrade the "coordinator" node last.
     Coordinator runs GC, rebalancing. Upgrading it last minimizes
     disruption to background operations.

  3. Wait for health check HEALTHY between each node.
     Ensures cluster is stable before proceeding.

  4. Never upgrade more than 1 node at a time.
     Maintains quorum and replication guarantees.

  CLI helper:
    $ deriva admin upgrade-plan
    Recommended order: [C, D, E, B, A]
    Reason: A is coordinator, B has most data
```

---

## 7. Performance Analysis

### 7.1 Overhead of Version System

```
┌──────────────────────────────┬──────────┬──────────────────────┐
│ Component                    │ Overhead │ Notes                │
├──────────────────────────────┼──────────┼──────────────────────┤
│ SWIM tags (3 extra strings)  │ ~50 bytes│ Per gossip message   │
│ Version interceptor          │ ~100ns   │ Parse header string  │
│ Outgoing interceptor         │ ~50ns    │ Insert pre-built str │
│ FeatureGate.is_enabled()     │ ~5ns     │ AtomicU32 load       │
│ VersionTracker.recompute()   │ ~10µs    │ Every 5s, iterate    │
│ Version metadata per node    │ ~100 bytes│ In-memory            │
└──────────────────────────────┴──────────┴──────────────────────┘

  Total per-request overhead: ~150ns (interceptors).
  Background overhead: ~10µs every 5 seconds.
  Negligible impact on throughput and latency.
```

### 7.2 Upgrade Duration Estimates

```
  Per-node upgrade time:
    Drain: depends on data volume
      1GB data, 50MB/s transfer → 20s
      10GB data, 50MB/s transfer → 200s (~3.5 min)
    Binary swap: ~5s (stop + start)
    Rejoin + health: ~10s

  3-node cluster, 5GB per node:
    Per node: ~100s drain + 5s swap + 10s health = ~2 min
    Total: 3 × 2 min = ~6 min

  10-node cluster, 20GB per node:
    Per node: ~400s drain + 5s swap + 10s health = ~7 min
    Total: 10 × 7 min = ~70 min

  Optimization: with replication, drain only migrates data
  where this node is the SOLE owner. Replicated data doesn't
  need migration (other replicas serve it).
```

### 7.3 Benchmarking Plan

```rust
/// Benchmark: FeatureGate.is_enabled() (target: <10ns)
#[bench]
fn bench_feature_gate_check(b: &mut Bencher) {
    let gate = FeatureGate::new();
    gate.update_cluster_version(2);
    b.iter(|| gate.is_enabled("streaming_gc"));
}

/// Benchmark: version interceptor (target: <200ns)
#[bench]
fn bench_version_interceptor(b: &mut Bencher) {
    let interceptor = version_interceptor(NodeVersion::current());
    b.iter(|| {
        let mut req = Request::new(());
        req.metadata_mut().insert("x-deriva-version", "2:2:0.4.0".parse().unwrap());
        interceptor(req).unwrap();
    });
}

/// Benchmark: VersionTracker.recompute() with 100 nodes
#[bench]
fn bench_tracker_recompute_100(b: &mut Bencher) {
    let swim = mock_swim_with_n_members(100);
    let tracker = VersionTracker::new(swim, Arc::new(FeatureGate::new()));
    b.iter(|| tracker.recompute());
    // Target: <100µs
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-core/src/version.rs` | **NEW** — NodeVersion, PROTOCOL_VERSION, FEATURE_VERSION constants |
| `deriva-core/src/error.rs` | Add `IncompatibleVersion` variant |
| `deriva-core/src/lib.rs` | Add `pub mod version` |
| `deriva-network/src/version_tracker.rs` | **NEW** — VersionTracker background task |
| `deriva-network/src/feature_gate.rs` | **NEW** — FeatureGate with feature definitions |
| `deriva-network/src/upgrade.rs` | **NEW** — UpgradeOrchestrator |
| `deriva-network/src/lib.rs` | Add `pub mod version_tracker, feature_gate, upgrade` |
| `deriva-server/src/version_interceptor.rs` | **NEW** — incoming + outgoing interceptors |
| `deriva-server/src/main.rs` | Wire interceptors, start VersionTracker |
| `deriva-server/src/state.rs` | Add `version_tracker`, `feature_gate` |
| `deriva-server/src/admin.rs` | Add `get_upgrade_status` RPC |
| `proto/admin.proto` | Add `GetUpgradeStatus` RPC + messages |
| `proto/deriva.proto` | Add `VersionInfo` message |
| `deriva-network/src/swim.rs` | Add version check in join handler |
| `deriva-network/tests/version.rs` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

| Crate | Version | Purpose |
|-------|---------|---------|
| `buf` (CI tool) | latest | Protobuf breaking change detection |

No new Rust crate dependencies. `buf` is a CI-only tool for proto linting.

---

## 10. Design Rationale

### 10.1 Why Two Version Numbers (Protocol + Feature)?

```
  Single version number:
    Increment for any change.
    Problem: a new feature (streaming_gc) bumps version.
    min_protocol must stay low (old nodes still compatible).
    Can't distinguish "wire-breaking change" from "new feature."

  Two version numbers:
    protocol_version: wire compatibility. Incremented rarely.
    feature_version: feature availability. Incremented per feature.

    This allows:
      - Old nodes to communicate with new nodes (protocol compatible)
      - New features to activate only when all nodes support them
      - Independent evolution of wire format and feature set

  Example:
    v1: protocol=1, feature=1
    v2: protocol=1, feature=2 (new GC, same wire format)
    v3: protocol=2, feature=3 (new RPC added, new feature)

    v1 and v2 can coexist (same protocol).
    v1 and v3 can coexist IF v3.min_protocol <= 1.
```

### 10.2 Why SWIM Tags Instead of Separate Version RPC?

```
  Separate VersionExchange RPC:
    Each node calls VersionExchange on every peer.
    N nodes → N² RPCs on startup.
    Must handle connection failures, retries.

  SWIM tags:
    Version piggybacked on existing gossip protocol.
    Zero extra RPCs. Zero extra connections.
    Propagates to all nodes within gossip convergence time (~2s).
    Already have SWIM infrastructure (§3.1).

  Trade-off: version info slightly delayed (gossip convergence).
  Acceptable: version changes are rare (upgrades), not latency-sensitive.
```

### 10.3 Why Conservative Feature Activation (All Nodes)?

```
  Option A: Majority activation (>50% of nodes at new version).
    Risk: minority nodes receive messages they can't process.
    Example: 2/3 nodes have streaming_gc. Node 3 gets a streaming
    GC message it doesn't understand → error.

  Option B: All-node activation (our choice).
    Feature only activates when EVERY alive node supports it.
    Guarantees: no node receives unsupported messages.
    Trade-off: one lagging node blocks feature activation.

  The trade-off is acceptable:
    - Upgrades are operator-controlled, not automatic.
    - Operators upgrade all nodes in sequence.
    - A lagging node is a bug in the upgrade process, not normal.
    - Safety > speed for feature activation.
```

### 10.4 Why Not Automatic Upgrades?

```
  Automatic rolling upgrade:
    System detects new binary available, upgrades nodes automatically.
    Convenient but dangerous:
      - No human verification between nodes.
      - Can't catch bugs that only appear under mixed versions.
      - Rollback requires automatic detection of "bad" (hard to define).

  Operator-initiated upgrade:
    Operator triggers each node upgrade explicitly.
    Can verify health between nodes.
    Can stop mid-upgrade if issues detected.
    Can rollback individual nodes.

  Deriva provides the TOOLS (drain, health check, version tracking)
  but leaves the DECISION to the operator.
  Automation can be built on top (scripts, CI/CD pipelines).
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `cluster_min_protocol_version` | Gauge | — | Current cluster minimum protocol version |
| `cluster_min_feature_version` | Gauge | — | Current cluster minimum feature version |
| `nodes_by_version` | Gauge | `build_version` | Count of nodes per build version |
| `version_check_rejections` | Counter | — | Nodes rejected for incompatible version |
| `feature_gate_checks` | Counter | `feature,result={enabled,disabled}` | Feature gate evaluations |
| `upgrade_drain_duration_secs` | Histogram | — | Time to drain a node during upgrade |
| `mixed_version_cluster` | Gauge | — | 1 if cluster has mixed versions, 0 otherwise |

### 11.2 Structured Logging

```rust
tracing::info!(
    protocol = PROTOCOL_VERSION,
    feature = FEATURE_VERSION,
    build = BUILD_VERSION,
    "node starting with version"
);

tracing::info!(
    min_protocol = min_pv,
    min_feature = min_fv,
    node_count = members.len(),
    "cluster version recomputed"
);

tracing::warn!(
    peer_id = peer.node_id.0,
    peer_protocol = peer_pv,
    local_min = local.min_protocol,
    "rejecting incompatible peer"
);

tracing::info!(
    feature = name,
    enabled = enabled,
    required_version = min_ver,
    cluster_version = cluster_min,
    "feature gate evaluated"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| Mixed version cluster | `mixed_version_cluster` == 1 for > 1 hour | Warning |
| Version rejection | `version_check_rejections` > 0 | Warning |
| Stale node version | Any node version < cluster_min - 1 for > 30min | Warning |
| Feature gate flapping | Feature enabled/disabled > 3 times in 10min | Critical |

---

## 12. Checklist

- [ ] Create `deriva-core/src/version.rs` with constants and NodeVersion
- [ ] Add `IncompatibleVersion` to DerivaError
- [ ] Create `deriva-network/src/version_tracker.rs`
- [ ] Create `deriva-network/src/feature_gate.rs` with feature definitions
- [ ] Create `deriva-network/src/upgrade.rs` with UpgradeOrchestrator
- [ ] Create `deriva-server/src/version_interceptor.rs` (incoming + outgoing)
- [ ] Wire version interceptors into tonic server and client channels
- [ ] Start VersionTracker background task in main.rs
- [ ] Add version tags to SWIM membership on join
- [ ] Add version check to SWIM join handler
- [ ] Add `get_upgrade_status` to AdminService
- [ ] Add `GetUpgradeStatus` RPC to admin.proto
- [ ] Add `VersionInfo` message to deriva.proto
- [ ] Add `version_tracker` and `feature_gate` to ServerState
- [ ] Write NodeVersion tests (5 tests)
- [ ] Write VersionTracker tests (3 tests)
- [ ] Write FeatureGate tests (5 tests)
- [ ] Write version interceptor tests (4 tests)
- [ ] Write integration tests (2 tests)
- [ ] Add metrics (7 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (4 alerts)
- [ ] Run benchmarks: feature gate, interceptor, tracker recompute
- [ ] Document protobuf compatibility rules in CONTRIBUTING.md
- [ ] Add `buf breaking` check to CI pipeline
- [ ] Document upgrade procedure in operator guide
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
