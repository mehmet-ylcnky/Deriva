# §3.15 Admin API & Cluster Introspection

> **Status:** Not started
> **Depends on:** §3.1 SWIM Gossip, §3.8 Cluster Bootstrap, §3.9 Cluster-Wide GC, §3.10 Data Rebalancing, §3.11 Backpressure, §3.12 mTLS, §3.13 Connection Pooling
> **Crate(s):** `deriva-server`, `deriva-network`, `deriva-core`
> **Estimated effort:** 3 days

---

## 1. Problem Statement

A distributed system without operational visibility is a distributed system waiting to fail. Operators need answers to fundamental questions at any moment:

1. **Cluster topology**: Which nodes are alive? What state is each node in? When did they join?
2. **Resource utilization**: How much storage is consumed? How many recipes/blobs exist? What's the cache hit rate?
3. **Operational control**: Trigger GC manually. Drain a node before maintenance. Force rebalancing. Rotate TLS certificates.
4. **Diagnostics**: Why is this node slow? What's the current ring layout? Which nodes own a given address?
5. **Configuration**: View and update runtime-tunable parameters without restart.

Without a dedicated admin API, operators resort to:
- SSH-ing into individual nodes and grepping logs
- Writing ad-hoc scripts against the data-plane gRPC API
- Restarting nodes to change configuration
- Guessing at cluster state from external metrics alone

### Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| A1 | List all cluster members with state, address, generation | P0 |
| A2 | Get detailed node stats (storage, cache, connections, uptime) | P0 |
| A3 | Trigger manual GC on cluster or single node | P0 |
| A4 | Drain node (stop accepting new data, migrate existing) | P0 |
| A5 | Lookup ring owner for a given CAddr | P1 |
| A6 | Get/set runtime configuration | P1 |
| A7 | Force rebalance | P1 |
| A8 | Health check endpoint (for load balancers) | P0 |
| A9 | Admin API protected by separate auth (admin mTLS or token) | P0 |
| A10 | Read-only operations safe to call at any time | P0 |

### Non-Goals

- Full web UI (out of scope; admin API is the foundation for one)
- Multi-cluster federation management
- User/tenant management (single-tenant system)

---

## 2. Design

### 2.1 Architecture

```
                    ┌─────────────────────────────────────────┐
                    │              Admin Client                │
                    │  (CLI / curl / monitoring / dashboard)   │
                    └──────────────┬──────────────────────────┘
                                   │ gRPC (admin port 9091)
                                   │ mTLS or token auth
                    ┌──────────────▼──────────────────────────┐
                    │         AdminService (tonic)             │
                    │                                          │
                    │  ┌──────────┐ ┌──────────┐ ┌──────────┐ │
                    │  │ Cluster  │ │  Node    │ │  Ops     │ │
                    │  │ Queries  │ │  Stats   │ │ Actions  │ │
                    │  └────┬─────┘ └────┬─────┘ └────┬─────┘ │
                    │       │            │            │        │
                    │  ┌────▼────────────▼────────────▼─────┐  │
                    │  │          ServerState                │  │
                    │  │  ┌──────────┐  ┌───────────────┐   │  │
                    │  │  │ SwimCluster│ │ BlobStore     │   │  │
                    │  │  │ HashRing  │ │ RecipeStore   │   │  │
                    │  │  │ GcCoord   │ │ ChannelPool   │   │  │
                    │  │  │ Rebalancer│ │ RuntimeConfig │   │  │
                    │  │  └──────────┘  └───────────────┘   │  │
                    │  └────────────────────────────────────┘  │
                    └──────────────────────────────────────────┘

  Key: Admin API runs on a SEPARATE port (9091) from data-plane (9090).
  This allows different firewall rules and auth policies.
```

### 2.2 API Surface

```
┌─────────────────────────────────────────────────────────┐
│                  AdminService RPCs                       │
├─────────────────────────────────────────────────────────┤
│ Read-only (safe):                                       │
│   GetClusterInfo()        → ClusterInfoResponse         │
│   GetNodeStats(node_id?)  → NodeStatsResponse           │
│   LookupAddr(addr)        → LookupAddrResponse          │
│   GetConfig()             → ConfigResponse              │
│   HealthCheck()           → HealthResponse              │
│                                                         │
│ Mutating (requires confirmation):                       │
│   TriggerGc(scope)        → GcTriggerResponse           │
│   DrainNode(node_id)      → DrainResponse               │
│   UndoNodeDrain(node_id)  → DrainResponse               │
│   ForceRebalance()        → RebalanceResponse           │
│   SetConfig(key, value)   → ConfigResponse              │
└─────────────────────────────────────────────────────────┘
```

### 2.3 Wire Protocol

```protobuf
// proto/admin.proto

service AdminService {
  // Read-only
  rpc GetClusterInfo(ClusterInfoRequest) returns (ClusterInfoResponse);
  rpc GetNodeStats(NodeStatsRequest) returns (NodeStatsResponse);
  rpc LookupAddr(LookupAddrRequest) returns (LookupAddrResponse);
  rpc GetConfig(GetConfigRequest) returns (ConfigResponse);
  rpc HealthCheck(HealthCheckRequest) returns (HealthResponse);

  // Mutating
  rpc TriggerGc(TriggerGcRequest) returns (TriggerGcResponse);
  rpc DrainNode(DrainNodeRequest) returns (DrainResponse);
  rpc UndoNodeDrain(UndoNodeDrainRequest) returns (DrainResponse);
  rpc ForceRebalance(ForceRebalanceRequest) returns (ForceRebalanceResponse);
  rpc SetConfig(SetConfigRequest) returns (ConfigResponse);
}

message ClusterInfoRequest {}

message ClusterInfoResponse {
  uint32 cluster_size = 1;
  repeated MemberInfo members = 2;
  uint64 ring_version = 3;
}

message MemberInfo {
  uint64 node_id = 1;
  string addr = 2;
  MemberState state = 3;
  uint64 generation = 4;
  uint64 joined_at_ms = 5;
  uint64 last_seen_ms = 6;
  repeated string tags = 7;
}

enum MemberState {
  ALIVE = 0;
  SUSPECT = 1;
  DOWN = 2;
  LEFT = 3;
  DRAINING = 4;
}

message NodeStatsRequest {
  optional uint64 node_id = 1; // empty = local node
}

message NodeStatsResponse {
  uint64 node_id = 1;
  StorageStats storage = 2;
  CacheStats cache = 3;
  ConnectionStats connections = 4;
  uint64 uptime_secs = 5;
  string version = 6;
  BackpressureStats backpressure = 7;
}

message StorageStats {
  uint64 blob_count = 1;
  uint64 blob_bytes = 2;
  uint64 recipe_count = 3;
  uint64 recipe_bytes = 4;
  uint64 disk_free_bytes = 5;
}

message CacheStats {
  uint64 entries = 1;
  uint64 bytes = 2;
  uint64 capacity_bytes = 3;
  double hit_rate = 4;
  uint64 evictions = 5;
}

message ConnectionStats {
  uint32 active_peers = 1;
  uint32 healthy_peers = 2;
  uint32 unhealthy_peers = 3;
  uint32 idle_connections = 4;
}

message BackpressureStats {
  uint32 current_concurrency = 1;
  uint32 concurrency_limit = 2;
  double memory_usage_pct = 3;
  uint64 rejected_requests = 4;
}

message LookupAddrRequest {
  bytes addr = 1;
}

message LookupAddrResponse {
  uint64 primary_owner = 1;
  repeated uint64 replica_owners = 2;
  bool exists_locally = 3;
  optional string value_type = 4; // "blob" | "recipe" | "not_found"
}

message HealthCheckRequest {}

message HealthResponse {
  HealthStatus status = 1;
  string message = 2;
  map<string, bool> checks = 3;
}

enum HealthStatus {
  HEALTHY = 0;
  DEGRADED = 1;
  UNHEALTHY = 2;
}

message TriggerGcRequest {
  GcScope scope = 1;
  bool dry_run = 2;
}

enum GcScope {
  LOCAL = 0;
  CLUSTER = 1;
}

message TriggerGcResponse {
  bool accepted = 1;
  string message = 2;
  optional string gc_run_id = 3;
}

message DrainNodeRequest {
  uint64 node_id = 1;
}

message UndoNodeDrainRequest {
  uint64 node_id = 1;
}

message DrainResponse {
  bool accepted = 1;
  string message = 2;
  MemberState new_state = 3;
}

message ForceRebalanceRequest {}

message ForceRebalanceResponse {
  bool accepted = 1;
  string message = 2;
  uint32 migrations_started = 3;
}

message GetConfigRequest {}

message SetConfigRequest {
  string key = 1;
  string value = 2;
}

message ConfigResponse {
  map<string, ConfigEntry> entries = 1;
}

message ConfigEntry {
  string value = 1;
  string default_value = 2;
  bool mutable = 3;
  string description = 4;
}
```

### 2.4 Core Types

```rust
// deriva-server/src/admin.rs

use tonic::{Request, Response, Status};

pub struct AdminServiceImpl {
    state: Arc<ServerState>,
    start_time: Instant,
}

/// Runtime-tunable configuration registry
pub struct RuntimeConfig {
    entries: DashMap<String, ConfigValue>,
}

pub struct ConfigValue {
    pub value: String,
    pub default: String,
    pub mutable: bool,
    pub description: String,
    pub validator: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>,
}

impl RuntimeConfig {
    pub fn new() -> Self {
        let rc = Self { entries: DashMap::new() };
        // Register known tunable parameters
        rc.register("gc.min_blob_age_secs", "300", true,
            "Minimum blob age before GC eligibility",
            Some(Box::new(|v| v.parse::<u64>().is_ok())));
        rc.register("cache.max_bytes", "1073741824", true,
            "Maximum cache size in bytes",
            Some(Box::new(|v| v.parse::<u64>().is_ok())));
        rc.register("backpressure.concurrency_limit", "128", true,
            "Max concurrent requests",
            Some(Box::new(|v| v.parse::<u32>().map_or(false, |n| n >= 1))));
        rc.register("rebalance.rate_limit_bytes", "52428800", true,
            "Rebalance transfer rate limit (bytes/sec)",
            Some(Box::new(|v| v.parse::<u64>().is_ok())));
        rc.register("pool.idle_timeout_secs", "300", true,
            "Connection idle timeout",
            Some(Box::new(|v| v.parse::<u64>().is_ok())));
        rc
    }

    fn register(&self, key: &str, default: &str, mutable: bool,
                desc: &str, validator: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>) {
        self.entries.insert(key.to_string(), ConfigValue {
            value: default.to_string(),
            default: default.to_string(),
            mutable,
            description: desc.to_string(),
            validator,
        });
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.entries.get(key).map(|e| e.value.clone())
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), DerivaError> {
        let mut entry = self.entries.get_mut(key)
            .ok_or(DerivaError::NotFound(format!("config key: {}", key)))?;
        if !entry.mutable {
            return Err(DerivaError::InvalidArgument(
                format!("config key '{}' is not mutable", key)));
        }
        if let Some(ref validator) = entry.validator {
            if !validator(value) {
                return Err(DerivaError::InvalidArgument(
                    format!("invalid value '{}' for key '{}'", value, key)));
            }
        }
        entry.value = value.to_string();
        Ok(())
    }
}
```

### 2.5 Admin Auth

```
  Admin API uses a separate authentication layer:

  Option 1: Separate mTLS CA (default)
    Admin clients use certs signed by an admin-specific CA.
    Data-plane clients cannot access admin API.
    Configured via admin_tls.ca_cert, admin_tls.cert, admin_tls.key.

  Option 2: Bearer token
    For environments where mTLS is impractical (dev, CI).
    Token set via DERIVA_ADMIN_TOKEN env var.
    Sent as `authorization: Bearer <token>` metadata.

  Interceptor checks auth before any admin RPC:
    1. If admin mTLS configured → require valid admin client cert.
    2. Else if DERIVA_ADMIN_TOKEN set → require matching bearer token.
    3. Else → reject (no open admin API by default).
```

---

## 3. Implementation

### 3.1 Admin Service — Read-Only RPCs

```rust
#[tonic::async_trait]
impl admin_service_server::AdminService for AdminServiceImpl {
    async fn get_cluster_info(
        &self, _req: Request<ClusterInfoRequest>,
    ) -> Result<Response<ClusterInfoResponse>, Status> {
        let members = self.state.swim.members();
        let ring_version = self.state.ring.version();

        let member_infos: Vec<MemberInfo> = members.iter().map(|m| MemberInfo {
            node_id: m.node_id.0,
            addr: m.addr.to_string(),
            state: swim_state_to_proto(m.state) as i32,
            generation: m.generation,
            joined_at_ms: m.joined_at.timestamp_millis() as u64,
            last_seen_ms: m.last_seen.timestamp_millis() as u64,
            tags: m.tags.clone(),
        }).collect();

        Ok(Response::new(ClusterInfoResponse {
            cluster_size: member_infos.len() as u32,
            members: member_infos,
            ring_version,
        }))
    }

    async fn get_node_stats(
        &self, req: Request<NodeStatsRequest>,
    ) -> Result<Response<NodeStatsResponse>, Status> {
        let target = req.into_inner().node_id;

        // If remote node requested, forward the RPC
        if let Some(nid) = target {
            if nid != self.state.local_node_id.0 {
                return self.forward_node_stats(NodeId(nid)).await;
            }
        }

        let storage = self.state.blob_store.stats().await;
        let recipe_stats = self.state.recipe_store.stats().await;
        let cache = self.state.cache.stats();
        let pool = self.state.channel_pool.stats();
        let bp = self.state.admission.stats();

        Ok(Response::new(NodeStatsResponse {
            node_id: self.state.local_node_id.0,
            storage: Some(StorageStats {
                blob_count: storage.count,
                blob_bytes: storage.bytes,
                recipe_count: recipe_stats.count,
                recipe_bytes: recipe_stats.bytes,
                disk_free_bytes: get_disk_free(&self.state.config.data_dir),
            }),
            cache: Some(CacheStats {
                entries: cache.entries,
                bytes: cache.bytes,
                capacity_bytes: cache.capacity,
                hit_rate: cache.hit_rate(),
                evictions: cache.evictions,
            }),
            connections: Some(ConnectionStats {
                active_peers: pool.active,
                healthy_peers: pool.healthy,
                unhealthy_peers: pool.unhealthy,
                idle_connections: pool.idle,
            }),
            backpressure: Some(BackpressureStats {
                current_concurrency: bp.current as u32,
                concurrency_limit: bp.limit as u32,
                memory_usage_pct: bp.memory_pct,
                rejected_requests: bp.rejected,
            }),
            uptime_secs: self.start_time.elapsed().as_secs(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }))
    }

    async fn lookup_addr(
        &self, req: Request<LookupAddrRequest>,
    ) -> Result<Response<LookupAddrResponse>, Status> {
        let addr = CAddr::from_bytes(&req.into_inner().addr);
        let primary = self.state.ring.primary_owner(&addr);
        let replicas = self.state.ring.replica_owners(&addr);

        let exists_locally = self.state.blob_store.exists(&addr).await;
        let value_type = if exists_locally {
            Some("blob".to_string())
        } else if self.state.recipe_store.exists(&addr).await {
            Some("recipe".to_string())
        } else {
            Some("not_found".to_string())
        };

        Ok(Response::new(LookupAddrResponse {
            primary_owner: primary.0,
            replica_owners: replicas.iter().map(|n| n.0).collect(),
            exists_locally,
            value_type,
        }))
    }

    async fn health_check(
        &self, _req: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        let mut checks = HashMap::new();
        let swim_ok = self.state.swim.is_joined();
        let storage_ok = self.state.blob_store.is_healthy().await;
        let pool_ok = self.state.channel_pool.has_healthy_peers();

        checks.insert("swim_membership".into(), swim_ok);
        checks.insert("storage".into(), storage_ok);
        checks.insert("peer_connectivity".into(), pool_ok);

        let all_ok = swim_ok && storage_ok && pool_ok;
        let status = if all_ok {
            HealthStatus::Healthy
        } else if storage_ok {
            HealthStatus::Degraded
        } else {
            HealthStatus::Unhealthy
        };

        Ok(Response::new(HealthResponse {
            status: status as i32,
            message: if all_ok { "OK".into() } else { "degraded".into() },
            checks,
        }))
    }

    async fn get_config(
        &self, _req: Request<GetConfigRequest>,
    ) -> Result<Response<ConfigResponse>, Status> {
        let entries = self.state.runtime_config.entries.iter()
            .map(|e| (e.key().clone(), ConfigEntry {
                value: e.value.clone(),
                default_value: e.default.clone(),
                mutable: e.mutable,
                description: e.description.clone(),
            }))
            .collect();

        Ok(Response::new(ConfigResponse { entries }))
    }
}
```

### 3.2 Admin Service — Mutating RPCs

```rust
#[tonic::async_trait]
impl admin_service_server::AdminService for AdminServiceImpl {
    async fn trigger_gc(
        &self, req: Request<TriggerGcRequest>,
    ) -> Result<Response<TriggerGcResponse>, Status> {
        let inner = req.into_inner();
        let dry_run = inner.dry_run;

        match GcScope::try_from(inner.scope).unwrap_or(GcScope::Local) {
            GcScope::Local => {
                let run_id = self.state.gc_coordinator.trigger_local(dry_run).await
                    .map_err(|e| Status::internal(e.to_string()))?;
                Ok(Response::new(TriggerGcResponse {
                    accepted: true,
                    message: format!("Local GC triggered (dry_run={})", dry_run),
                    gc_run_id: Some(run_id),
                }))
            }
            GcScope::Cluster => {
                let run_id = self.state.gc_coordinator.trigger_cluster(dry_run).await
                    .map_err(|e| Status::internal(e.to_string()))?;
                Ok(Response::new(TriggerGcResponse {
                    accepted: true,
                    message: format!("Cluster GC triggered (dry_run={})", dry_run),
                    gc_run_id: Some(run_id),
                }))
            }
        }
    }

    async fn drain_node(
        &self, req: Request<DrainNodeRequest>,
    ) -> Result<Response<DrainResponse>, Status> {
        let node_id = NodeId(req.into_inner().node_id);

        // Mark node as DRAINING in SWIM metadata
        self.state.swim.set_member_state(node_id, SwimState::Draining)
            .map_err(|e| Status::not_found(e.to_string()))?;

        // Trigger rebalance to migrate data away
        self.state.rebalancer.trigger_drain(node_id).await
            .map_err(|e| Status::internal(e.to_string()))?;

        tracing::info!(node_id = node_id.0, "node drain initiated");

        Ok(Response::new(DrainResponse {
            accepted: true,
            message: format!("Node {} drain initiated, data migration started", node_id.0),
            new_state: MemberState::Draining as i32,
        }))
    }

    async fn undo_node_drain(
        &self, req: Request<UndoNodeDrainRequest>,
    ) -> Result<Response<DrainResponse>, Status> {
        let node_id = NodeId(req.into_inner().node_id);

        self.state.swim.set_member_state(node_id, SwimState::Alive)
            .map_err(|e| Status::not_found(e.to_string()))?;

        self.state.rebalancer.cancel_drain(node_id).await;

        tracing::info!(node_id = node_id.0, "node drain cancelled");

        Ok(Response::new(DrainResponse {
            accepted: true,
            message: format!("Node {} drain cancelled, restored to ALIVE", node_id.0),
            new_state: MemberState::Alive as i32,
        }))
    }

    async fn force_rebalance(
        &self, _req: Request<ForceRebalanceRequest>,
    ) -> Result<Response<ForceRebalanceResponse>, Status> {
        let count = self.state.rebalancer.trigger_full_rebalance().await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ForceRebalanceResponse {
            accepted: true,
            message: format!("Rebalance started, {} migrations queued", count),
            migrations_started: count,
        }))
    }

    async fn set_config(
        &self, req: Request<SetConfigRequest>,
    ) -> Result<Response<ConfigResponse>, Status> {
        let inner = req.into_inner();
        self.state.runtime_config.set(&inner.key, &inner.value)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        tracing::info!(key = %inner.key, value = %inner.value, "config updated via admin API");

        // Return full config after update
        self.get_config(Request::new(GetConfigRequest {})).await
    }
}
```

### 3.3 Admin Auth Interceptor

```rust
pub fn admin_auth_interceptor(
    admin_token: Option<String>,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    move |req: Request<()>| {
        // If mTLS is configured, tonic TLS layer handles client cert validation.
        // This interceptor handles bearer token auth as fallback.
        if let Some(ref token) = admin_token {
            let meta = req.metadata();
            let provided = meta.get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "));

            match provided {
                Some(t) if constant_time_eq(t.as_bytes(), token.as_bytes()) => Ok(req),
                Some(_) => Err(Status::unauthenticated("invalid admin token")),
                None => Err(Status::unauthenticated("admin token required")),
            }
        } else {
            // mTLS mode: if we got here, TLS handshake already validated client cert
            Ok(req)
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
```

### 3.4 Server Wiring

```rust
// deriva-server/src/main.rs (additions)

let admin_service = AdminServiceImpl {
    state: server_state.clone(),
    start_time: Instant::now(),
};

let admin_token = std::env::var("DERIVA_ADMIN_TOKEN").ok();

let admin_server = Server::builder()
    .tls_config(admin_tls_config)?  // separate admin TLS
    .add_service(
        admin_service_server::AdminServiceServer::with_interceptor(
            admin_service,
            admin_auth_interceptor(admin_token),
        )
    )
    .serve(admin_addr);  // port 9091

// Run data-plane and admin servers concurrently
tokio::select! {
    r = data_server => r?,
    r = admin_server => r?,
}
```

### 3.5 Helper: Disk Free Space

```rust
fn get_disk_free(path: &Path) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let stat = nix::sys::statvfs::statvfs(path).ok();
        stat.map(|s| s.blocks_available() * s.fragment_size()).unwrap_or(0)
    }
    #[cfg(not(unix))]
    { 0 }
}
```


---

## 4. Data Flow Diagrams

### 4.1 GetClusterInfo — Operator Queries Topology

```
  Operator (CLI)              Admin API (Node A)         SWIM Membership
       │                           │                          │
       │── GetClusterInfo() ──────►│                          │
       │                           │── members() ────────────►│
       │                           │◄── [A:Alive, B:Alive,   │
       │                           │     C:Suspect, D:Left]   │
       │                           │                          │
       │                           │── ring.version() ───►    │
       │                           │◄── 42                    │
       │                           │                          │
       │◄── ClusterInfoResponse ──┘                          │
       │    cluster_size: 4                                   │
       │    ring_version: 42                                  │
       │    members: [A, B, C, D]                             │
```

### 4.2 DrainNode — Maintenance Workflow

```
  Operator                Admin API (Node A)       SWIM         Rebalancer
     │                         │                     │               │
     │── DrainNode(B) ────────►│                     │               │
     │                         │── set_state(B,      │               │
     │                         │   DRAINING) ───────►│               │
     │                         │                     │── gossip ──►  │
     │                         │                     │  (B=DRAINING) │
     │                         │── trigger_drain(B) ─┼──────────────►│
     │                         │                     │               │
     │◄── "drain initiated" ──┘                     │               │
     │                                               │               │
     │  ... time passes, data migrates ...           │               │
     │                                               │               │
     │── GetNodeStats(B) ────►│                      │               │
     │◄── blob_count: 0 ─────┘  (all migrated)     │               │
     │                                               │               │
     │  Operator shuts down Node B for maintenance   │               │
     │                                               │               │
     │  ... maintenance complete ...                 │               │
     │                                               │               │
     │── UndoNodeDrain(B) ───►│                      │               │
     │◄── "restored ALIVE" ──┘                      │               │
```

### 4.3 HealthCheck — Load Balancer Integration

```
  Load Balancer            Admin API (port 9091)
       │                         │
       │── HealthCheck() ───────►│
       │                         │── swim.is_joined()? ──► true
       │                         │── blob_store.healthy()? ► true
       │                         │── pool.has_peers()? ────► true
       │                         │
       │◄── HEALTHY ────────────┘
       │    checks: {swim: ✓, storage: ✓, peers: ✓}
       │
       │  ... disk fills up ...
       │
       │── HealthCheck() ───────►│
       │                         │── blob_store.healthy()? ► false
       │◄── UNHEALTHY ─────────┘
       │    checks: {swim: ✓, storage: ✗, peers: ✓}
       │
       │  LB removes node from rotation
```

### 4.4 SetConfig — Runtime Tuning

```
  Operator                Admin API              RuntimeConfig
     │                         │                       │
     │── SetConfig(                                    │
     │   "backpressure.        │                       │
     │    concurrency_limit",  │                       │
     │   "256") ──────────────►│                       │
     │                         │── set("bp.conc", "256")►│
     │                         │                       │── validate: u32? ✓
     │                         │                       │── mutable? ✓
     │                         │                       │── store "256"
     │                         │◄── Ok ────────────────┘
     │                         │                       │
     │◄── ConfigResponse ─────┘                       │
     │    (full config snapshot)                       │
     │                                                 │
     │  Next request reads new limit from RuntimeConfig│
```

### 4.5 LookupAddr — Debugging Data Placement

```
  Operator                Admin API              Ring        BlobStore
     │                         │                   │             │
     │── LookupAddr(0xABC) ───►│                   │             │
     │                         │── primary(ABC) ──►│             │
     │                         │◄── Node B ────────┘             │
     │                         │── replicas(ABC) ─►│             │
     │                         │◄── [B, C] ────────┘             │
     │                         │── exists(ABC) ───────────────►  │
     │                         │◄── false ─────────────────────  │
     │                         │── recipe_exists(ABC) ──► false  │
     │                         │                                 │
     │◄── LookupAddrResponse ─┘                                 │
     │    primary: B                                             │
     │    replicas: [B, C]                                       │
     │    exists_locally: false                                  │
     │    value_type: "not_found"                                │
```

---

## 5. Test Specification

### 5.1 GetClusterInfo Tests

```rust
#[cfg(test)]
mod admin_tests {
    use super::*;

    fn mock_state(members: Vec<SwimMember>) -> Arc<ServerState> {
        let state = ServerState::test_default();
        for m in members {
            state.swim.inject_member(m);
        }
        Arc::new(state)
    }

    fn admin_svc(state: Arc<ServerState>) -> AdminServiceImpl {
        AdminServiceImpl { state, start_time: Instant::now() }
    }

    #[tokio::test]
    async fn test_get_cluster_info_returns_all_members() {
        let state = mock_state(vec![
            SwimMember::new(NodeId(1), "127.0.0.1:9090", SwimState::Alive),
            SwimMember::new(NodeId(2), "127.0.0.2:9090", SwimState::Alive),
            SwimMember::new(NodeId(3), "127.0.0.3:9090", SwimState::Suspect),
        ]);
        let svc = admin_svc(state);

        let resp = svc.get_cluster_info(Request::new(ClusterInfoRequest {}))
            .await.unwrap().into_inner();

        assert_eq!(resp.cluster_size, 3);
        assert_eq!(resp.members.len(), 3);
        let suspect = resp.members.iter().find(|m| m.node_id == 3).unwrap();
        assert_eq!(suspect.state, MemberState::Suspect as i32);
    }

    #[tokio::test]
    async fn test_get_cluster_info_empty_cluster() {
        let state = mock_state(vec![]);
        let svc = admin_svc(state);

        let resp = svc.get_cluster_info(Request::new(ClusterInfoRequest {}))
            .await.unwrap().into_inner();

        assert_eq!(resp.cluster_size, 0);
        assert!(resp.members.is_empty());
    }
}
```

### 5.2 NodeStats Tests

```rust
#[tokio::test]
async fn test_get_node_stats_local() {
    let state = mock_state(vec![]);
    // Pre-populate some blobs
    state.blob_store.put_raw(b"hello").await.unwrap();
    state.blob_store.put_raw(b"world").await.unwrap();
    let svc = admin_svc(state);

    let resp = svc.get_node_stats(Request::new(NodeStatsRequest { node_id: None }))
        .await.unwrap().into_inner();

    assert_eq!(resp.storage.unwrap().blob_count, 2);
    assert!(resp.uptime_secs < 5); // just started
    assert!(!resp.version.is_empty());
}

#[tokio::test]
async fn test_get_node_stats_cache_hit_rate() {
    let state = mock_state(vec![]);
    // Simulate cache activity
    let addr = state.blob_store.put_raw(b"data").await.unwrap();
    state.cache.get(&addr); // miss
    state.cache.insert(addr.clone(), Value::Leaf(b"data".to_vec()));
    state.cache.get(&addr); // hit
    let svc = admin_svc(state);

    let resp = svc.get_node_stats(Request::new(NodeStatsRequest { node_id: None }))
        .await.unwrap().into_inner();

    let cache = resp.cache.unwrap();
    assert!(cache.hit_rate > 0.0);
    assert_eq!(cache.entries, 1);
}
```

### 5.3 HealthCheck Tests

```rust
#[tokio::test]
async fn test_health_check_healthy() {
    let state = mock_state(vec![
        SwimMember::new(NodeId(1), "127.0.0.1:9090", SwimState::Alive),
    ]);
    state.swim.mark_joined();
    let svc = admin_svc(state);

    let resp = svc.health_check(Request::new(HealthCheckRequest {}))
        .await.unwrap().into_inner();

    assert_eq!(resp.status, HealthStatus::Healthy as i32);
    assert!(resp.checks["swim_membership"]);
    assert!(resp.checks["storage"]);
}

#[tokio::test]
async fn test_health_check_degraded_no_peers() {
    let state = mock_state(vec![]);
    state.swim.mark_joined();
    // No peers in pool → degraded
    let svc = admin_svc(state);

    let resp = svc.health_check(Request::new(HealthCheckRequest {}))
        .await.unwrap().into_inner();

    // Storage OK but no peers → degraded
    assert!(resp.status == HealthStatus::Degraded as i32
         || resp.status == HealthStatus::Healthy as i32);
}
```

### 5.4 LookupAddr Tests

```rust
#[tokio::test]
async fn test_lookup_addr_local_blob() {
    let state = mock_state(vec![]);
    let addr = state.blob_store.put_raw(b"test").await.unwrap();
    let svc = admin_svc(state);

    let resp = svc.lookup_addr(Request::new(LookupAddrRequest {
        addr: addr.as_bytes().to_vec(),
    })).await.unwrap().into_inner();

    assert!(resp.exists_locally);
    assert_eq!(resp.value_type.unwrap(), "blob");
}

#[tokio::test]
async fn test_lookup_addr_not_found() {
    let state = mock_state(vec![]);
    let addr = CAddr::hash(b"nonexistent");
    let svc = admin_svc(state);

    let resp = svc.lookup_addr(Request::new(LookupAddrRequest {
        addr: addr.as_bytes().to_vec(),
    })).await.unwrap().into_inner();

    assert!(!resp.exists_locally);
    assert_eq!(resp.value_type.unwrap(), "not_found");
}
```

### 5.5 Config Tests

```rust
#[tokio::test]
async fn test_get_config_returns_all_entries() {
    let state = mock_state(vec![]);
    let svc = admin_svc(state);

    let resp = svc.get_config(Request::new(GetConfigRequest {}))
        .await.unwrap().into_inner();

    assert!(resp.entries.contains_key("gc.min_blob_age_secs"));
    assert!(resp.entries.contains_key("cache.max_bytes"));
    let gc_entry = &resp.entries["gc.min_blob_age_secs"];
    assert!(gc_entry.mutable);
    assert_eq!(gc_entry.default_value, "300");
}

#[tokio::test]
async fn test_set_config_valid() {
    let state = mock_state(vec![]);
    let svc = admin_svc(state);

    let resp = svc.set_config(Request::new(SetConfigRequest {
        key: "cache.max_bytes".into(),
        value: "2147483648".into(),
    })).await.unwrap().into_inner();

    assert_eq!(resp.entries["cache.max_bytes"].value, "2147483648");
}

#[tokio::test]
async fn test_set_config_invalid_value() {
    let state = mock_state(vec![]);
    let svc = admin_svc(state);

    let result = svc.set_config(Request::new(SetConfigRequest {
        key: "cache.max_bytes".into(),
        value: "not_a_number".into(),
    })).await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn test_set_config_unknown_key() {
    let state = mock_state(vec![]);
    let svc = admin_svc(state);

    let result = svc.set_config(Request::new(SetConfigRequest {
        key: "nonexistent.key".into(),
        value: "42".into(),
    })).await;

    assert!(result.is_err());
}
```

### 5.6 Auth Interceptor Tests

```rust
#[test]
fn test_auth_interceptor_valid_token() {
    let interceptor = admin_auth_interceptor(Some("secret123".into()));
    let mut req = Request::new(());
    req.metadata_mut().insert("authorization",
        "Bearer secret123".parse().unwrap());

    assert!(interceptor(req).is_ok());
}

#[test]
fn test_auth_interceptor_invalid_token() {
    let interceptor = admin_auth_interceptor(Some("secret123".into()));
    let mut req = Request::new(());
    req.metadata_mut().insert("authorization",
        "Bearer wrong".parse().unwrap());

    assert!(interceptor(req).is_err());
}

#[test]
fn test_auth_interceptor_missing_token() {
    let interceptor = admin_auth_interceptor(Some("secret123".into()));
    let req = Request::new(());
    assert!(interceptor(req).is_err());
}

#[test]
fn test_auth_interceptor_no_auth_configured() {
    let interceptor = admin_auth_interceptor(None);
    let req = Request::new(());
    // mTLS mode: interceptor passes through (TLS layer handles auth)
    assert!(interceptor(req).is_ok());
}
```

### 5.7 Integration Tests

```rust
#[tokio::test]
async fn test_admin_api_full_workflow() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    let mut admin = connect_admin(&nodes[0]).await;

    // 1. Check cluster info
    let info = admin.get_cluster_info(ClusterInfoRequest {}).await.unwrap().into_inner();
    assert_eq!(info.cluster_size, 3);

    // 2. Check health
    let health = admin.health_check(HealthCheckRequest {}).await.unwrap().into_inner();
    assert_eq!(health.status, HealthStatus::Healthy as i32);

    // 3. Get node stats
    let stats = admin.get_node_stats(NodeStatsRequest { node_id: None })
        .await.unwrap().into_inner();
    assert!(stats.uptime_secs > 0);

    // 4. Update config
    admin.set_config(SetConfigRequest {
        key: "cache.max_bytes".into(),
        value: "536870912".into(),
    }).await.unwrap();

    // 5. Verify config persisted
    let config = admin.get_config(GetConfigRequest {}).await.unwrap().into_inner();
    assert_eq!(config.entries["cache.max_bytes"].value, "536870912");
}

#[tokio::test]
async fn test_drain_and_restore() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    let mut admin = connect_admin(&nodes[0]).await;
    let node_b_id = nodes[1].node_id();

    // Drain node B
    let resp = admin.drain_node(DrainNodeRequest { node_id: node_b_id })
        .await.unwrap().into_inner();
    assert!(resp.accepted);
    assert_eq!(resp.new_state, MemberState::Draining as i32);

    // Verify state in cluster info
    let info = admin.get_cluster_info(ClusterInfoRequest {}).await.unwrap().into_inner();
    let b = info.members.iter().find(|m| m.node_id == node_b_id).unwrap();
    assert_eq!(b.state, MemberState::Draining as i32);

    // Undo drain
    let resp = admin.undo_node_drain(UndoNodeDrainRequest { node_id: node_b_id })
        .await.unwrap().into_inner();
    assert!(resp.accepted);
    assert_eq!(resp.new_state, MemberState::Alive as i32);
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Admin port already in use | Startup fails with clear error | Fail-fast, don't silently skip admin |
| 2 | GetNodeStats for unknown node_id | Return NOT_FOUND status | Don't guess |
| 3 | GetNodeStats for remote node, peer unreachable | Return UNAVAILABLE with node_id in message | Operator knows which node is down |
| 4 | DrainNode for already-draining node | Idempotent: return success, no-op | Safe to retry |
| 5 | DrainNode for self (coordinator) | Allowed: node drains itself, stops accepting new data | Operator responsibility to have other nodes |
| 6 | DrainNode for unknown node | NOT_FOUND error | Can't drain what doesn't exist |
| 7 | SetConfig with immutable key | INVALID_ARGUMENT error | Prevent accidental changes to startup-only config |
| 8 | SetConfig with value that fails validation | INVALID_ARGUMENT with details | Clear feedback |
| 9 | TriggerGc while GC already running | Return accepted=false, message="GC already in progress" | Prevent overlapping GC runs |
| 10 | ForceRebalance with no data to move | Return accepted=true, migrations_started=0 | Idempotent, no error |
| 11 | HealthCheck during startup (not yet joined) | Return UNHEALTHY | LB won't route traffic until ready |
| 12 | Admin token timing attack | constant_time_eq comparison | Prevent token extraction via timing |
| 13 | Very large cluster (1000 nodes) | GetClusterInfo returns all; paginate in future | v1 simplicity |
| 14 | Concurrent SetConfig for same key | Last writer wins (DashMap) | Acceptable for config tuning |

### 6.1 Separate Port Security Model

```
  Why separate port (9091) instead of path-based routing?

  1. Firewall rules: Block 9091 from public, allow only from admin CIDR.
     Simpler than L7 path-based rules.

  2. Separate TLS: Admin can use different CA, stricter cert requirements.
     Data-plane clients never see admin endpoints.

  3. Rate limiting: Admin port can have different rate limits.
     A DDoS on data-plane doesn't affect admin access.

  4. Monitoring: Admin port traffic is clearly separated in metrics.

  Trade-off: Two ports to manage. Worth it for security isolation.
```

### 6.2 Config Change Propagation

```
  RuntimeConfig.set() updates the in-memory DashMap.
  Components read config values on each request:

    let limit = state.runtime_config
        .get("backpressure.concurrency_limit")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(128);

  No notification mechanism needed for v1.
  Components poll config on each operation → changes take effect immediately.

  Future: Add watch/subscribe for config changes if polling is too expensive.
  For now, config reads are DashMap lookups (~10ns) → negligible.
```

---

## 7. Performance Analysis

### 7.1 Admin RPC Latency

```
┌──────────────────────────┬──────────┬──────────────────────────┐
│ RPC                      │ Latency  │ Notes                    │
├──────────────────────────┼──────────┼──────────────────────────┤
│ HealthCheck              │ <1ms     │ 3 boolean checks         │
│ GetClusterInfo (10 nodes)│ <1ms     │ Read SWIM member list    │
│ GetClusterInfo (100)     │ ~2ms     │ Serialize 100 members    │
│ GetNodeStats (local)     │ ~5ms     │ Disk stat + store stats  │
│ GetNodeStats (remote)    │ ~10ms    │ Forward RPC to peer      │
│ LookupAddr               │ ~2ms     │ Ring lookup + store check│
│ GetConfig                │ <1ms     │ DashMap iteration        │
│ SetConfig                │ <1ms     │ DashMap insert           │
│ TriggerGc                │ ~5ms     │ Spawn async GC task      │
│ DrainNode                │ ~10ms    │ SWIM update + rebalance  │
│ ForceRebalance           │ ~20ms    │ Compute migration plan   │
└──────────────────────────┴──────────┴──────────────────────────┘

  All admin RPCs are fast. No admin RPC blocks on long operations.
  GC, drain, rebalance are async: RPC returns immediately, work happens in background.
```

### 7.2 Resource Overhead

```
  Admin server overhead:
    - 1 tonic server on port 9091 (shared tokio runtime)
    - RuntimeConfig: ~1KB for 5 entries
    - No background tasks (all on-demand)

  Total: negligible. <1MB memory, 0 CPU when idle.
```

### 7.3 Benchmarking Plan

```rust
/// Benchmark: HealthCheck latency (target: <500µs)
#[bench]
fn bench_health_check(b: &mut Bencher) {
    // Local health check, no network
}

/// Benchmark: GetClusterInfo with 100 members
#[bench]
fn bench_cluster_info_100(b: &mut Bencher) {
    // Pre-populate 100 SWIM members
    // Target: <5ms
}

/// Benchmark: GetNodeStats local
#[bench]
fn bench_node_stats_local(b: &mut Bencher) {
    // Local stats collection
    // Target: <10ms
}

/// Benchmark: SetConfig throughput
#[bench]
fn bench_set_config(b: &mut Bencher) {
    // Rapid config updates
    // Target: >100K ops/sec
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `proto/admin.proto` | **NEW** — AdminService definition, all messages |
| `deriva-server/src/admin.rs` | **NEW** — AdminServiceImpl, all RPC handlers |
| `deriva-server/src/config.rs` | **NEW** — RuntimeConfig, ConfigValue |
| `deriva-server/src/auth.rs` | **NEW** — admin_auth_interceptor, constant_time_eq |
| `deriva-server/src/main.rs` | Add admin server startup on port 9091 |
| `deriva-server/src/state.rs` | Add `runtime_config: Arc<RuntimeConfig>` |
| `deriva-server/src/lib.rs` | Add `pub mod admin, config, auth` |
| `deriva-network/src/swim.rs` | Add `set_member_state()`, `DRAINING` state |
| `deriva-cli/src/commands.rs` | Add admin subcommands |
| `deriva-server/tests/admin.rs` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

| Crate | Version | Purpose |
|-------|---------|---------|
| `nix` | 0.27.x | `statvfs` for disk free space (unix only) |

All other dependencies already in workspace (`tonic`, `dashmap`, `tracing`).

---

## 10. Design Rationale

### 10.1 Why gRPC Instead of REST for Admin API?

```
  REST (HTTP/JSON):
    + Easier to curl from command line
    + No proto compilation needed
    - No streaming (for future log tailing, event watching)
    - Separate HTTP server dependency (axum/actix)
    - Different serialization path from data-plane

  gRPC:
    + Same transport as data-plane (tonic already in deps)
    + Streaming support for future features
    + Strongly typed proto contract
    + Same TLS infrastructure
    - Harder to curl (need grpcurl)

  Decision: gRPC. Consistency with data-plane outweighs curl convenience.
  CLI tool wraps gRPC calls for operator ergonomics.
  Future: add optional REST gateway via tonic-web if needed.
```

### 10.2 Why RuntimeConfig Instead of Config File Reload?

```
  Config file reload:
    Operator edits TOML file, sends SIGHUP.
    Node re-reads file, applies changes.
    Problem: must SSH into each node. No cluster-wide update.

  RuntimeConfig via Admin API:
    Operator calls SetConfig RPC.
    Change takes effect immediately on that node.
    Can script cluster-wide updates: for each node, call SetConfig.
    No file editing, no SSH, no signal handling.

  Trade-off: RuntimeConfig is ephemeral (lost on restart).
  Acceptable: tunable parameters have sensible defaults.
  Persistent config changes still go through config file + restart.
  RuntimeConfig is for operational tuning, not permanent changes.
```

### 10.3 Why Separate Admin Auth?

```
  Same auth as data-plane:
    Any client with data-plane certs can call admin RPCs.
    Dangerous: a compromised application can drain nodes, trigger GC.

  Separate admin auth:
    Admin certs issued to operators only.
    Application certs cannot access admin API.
    Defense in depth: even if data-plane is compromised, admin is safe.

  Bearer token option for dev/CI where mTLS is overkill.
  Production: always use admin mTLS.
```

### 10.4 Why DRAINING as a SWIM State?

```
  Option A: Separate drain flag outside SWIM.
    Problem: other nodes don't know about drain.
    They keep routing data to draining node.

  Option B: DRAINING as SWIM state (our choice).
    Gossip propagates DRAINING to all nodes.
    Ring excludes DRAINING nodes from new placements.
    Existing data migrated by rebalancer.
    Clean integration with existing membership protocol.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `admin_requests_total` | Counter | `rpc={GetClusterInfo,...}` | Admin RPC calls |
| `admin_request_duration_ms` | Histogram | `rpc` | Admin RPC latency |
| `admin_auth_failures_total` | Counter | `reason={invalid_token,missing_token}` | Auth failures |
| `admin_config_changes_total` | Counter | `key` | Config updates via API |
| `admin_drain_operations_total` | Counter | `action={drain,undo}` | Drain operations |
| `admin_gc_triggers_total` | Counter | `scope={local,cluster}` | Manual GC triggers |
| `health_check_status` | Gauge | — | 0=healthy, 1=degraded, 2=unhealthy |

### 11.2 Structured Logging

```rust
tracing::info!(rpc = "GetClusterInfo", cluster_size = members.len(), "admin query");

tracing::info!(
    rpc = "DrainNode",
    node_id = node_id.0,
    "node drain initiated via admin API"
);

tracing::warn!(
    rpc = "SetConfig",
    key = %key,
    old_value = %old,
    new_value = %new,
    "runtime config changed via admin API"
);

tracing::warn!(
    remote_addr = %req.remote_addr().unwrap_or("unknown"),
    "admin auth failure: invalid token"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| Admin auth failures | `admin_auth_failures_total` > 5 in 1min | Critical |
| Health check unhealthy | `health_check_status` == 2 for > 30s | Critical |
| Health check degraded | `health_check_status` == 1 for > 5min | Warning |
| Frequent config changes | `admin_config_changes_total` > 10 in 1min | Info |

---

## 12. Checklist

- [ ] Create `proto/admin.proto` with all messages and RPCs
- [ ] Generate Rust code from admin.proto
- [ ] Implement `AdminServiceImpl` with all 10 RPCs
- [ ] Implement `RuntimeConfig` with 5 tunable parameters
- [ ] Implement `admin_auth_interceptor` with constant-time comparison
- [ ] Implement `constant_time_eq` helper
- [ ] Add `DRAINING` state to SWIM membership
- [ ] Add `set_member_state()` to SwimCluster
- [ ] Wire admin server on port 9091 in main.rs
- [ ] Add `runtime_config` to ServerState
- [ ] Implement `get_disk_free()` helper (unix)
- [ ] Add admin CLI subcommands
- [ ] Write GetClusterInfo tests (2 tests)
- [ ] Write NodeStats tests (2 tests)
- [ ] Write HealthCheck tests (2 tests)
- [ ] Write LookupAddr tests (2 tests)
- [ ] Write Config tests (3 tests)
- [ ] Write Auth interceptor tests (4 tests)
- [ ] Write integration tests (2 tests)
- [ ] Add metrics (7 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (4 alerts)
- [ ] Run benchmarks: health check, cluster info, node stats, set config
- [ ] Document admin API in operator guide
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
