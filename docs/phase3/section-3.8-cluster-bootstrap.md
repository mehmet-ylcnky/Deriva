# §3.8 Cluster Bootstrap & Discovery

> **Status:** Not started
> **Depends on:** §3.1 (SWIM gossip), §3.2 (recipe replication), §3.3 (leaf replication), §3.7 (consistency)
> **Crate(s):** `deriva-network`, `deriva-server`
> **Estimated effort:** 2–3 days

---

## 1. Problem Statement

The current SWIM implementation (§3.1) assumes nodes already know at least one
peer to contact. There is no defined mechanism for:

1. **Initial cluster formation** — how do 3 nodes find each other on first boot?
2. **Node identity persistence** — if a node restarts, does it rejoin as the
   same identity or as a new member?
3. **Ordered startup** — a joining node needs recipes and ring state before it
   can serve requests. What's the sequence?
4. **Graceful shutdown** — a node leaving should drain requests, hand off hints,
   and notify peers before disappearing.
5. **Discovery methods** — operators need flexible ways to configure peer
   discovery (static seeds, DNS, environment variables).

Without a bootstrap protocol, every deployment requires manual orchestration
to wire nodes together, and restarts risk data loss or split-brain scenarios.

### What happens today without bootstrap

```
Node A starts → listens on :7947 → no peers → alone
Node B starts → listens on :7948 → no peers → alone
Node C starts → listens on :7949 → no peers → alone

Three independent single-node clusters. No replication, no routing.
Operator must manually call swim.join(peer_addr) on each node.
```

---

## 2. Design

### 2.1 Architecture Overview

```
                    ┌──────────────────────────────────┐
                    │       Bootstrap State Machine     │
                    └──────────────────────────────────┘

  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
  │ Init     │───►│ Discover │───►│ Joining  │───►│ Syncing  │───►│ Ready    │
  │          │    │          │    │          │    │          │    │          │
  │ Load ID  │    │ Find     │    │ SWIM     │    │ Pull     │    │ Serve    │
  │ from disk│    │ peers    │    │ join     │    │ recipes  │    │ requests │
  │ or gen   │    │ (seeds/  │    │ gossip   │    │ + ring   │    │          │
  │ new one  │    │  DNS)    │    │ handshake│    │ state    │    │          │
  └──────────┘    └──────────┘    └──────────┘    └──────────┘    └──────────┘
       │                                                               │
       │          On fatal error at any stage:                         │
       └──────────────────────┐                                        │
                         ┌────▼─────┐                             ┌────▼─────┐
                         │ Failed   │                             │ Draining │
                         │          │                             │          │
                         │ Log +    │                             │ Graceful │
                         │ exit(1)  │                             │ shutdown │
                         └──────────┘                             └──────────┘


  Graceful shutdown:
  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
  │ Ready    │───►│ Draining │───►│ Leaving  │───►│ Stopped  │
  │          │    │          │    │          │    │          │
  │ SIGTERM  │    │ Stop new │    │ SWIM     │    │ Process  │
  │ received │    │ requests │    │ leave +  │    │ exits    │
  │          │    │ Finish   │    │ transfer │    │          │
  │          │    │ in-flight│    │ hints    │    │          │
  └──────────┘    └──────────┘    └──────────┘    └──────────┘
```

### 2.2 Discovery Methods

```
┌─────────────────────────────────────────────────────────────┐
│                    Discovery Providers                       │
├──────────────┬──────────────────────────────────────────────┤
│ Static Seeds │ List of host:port pairs in config file       │
│              │ e.g., ["10.0.1.1:7947", "10.0.1.2:7947"]    │
│              │ Simplest, works for fixed infrastructure     │
├──────────────┼──────────────────────────────────────────────┤
│ DNS SRV      │ Resolve SRV record for service name          │
│              │ e.g., _deriva._tcp.cluster.local              │
│              │ Works with Kubernetes headless services       │
├──────────────┼──────────────────────────────────────────────┤
│ Environment  │ Read DERIVA_SEEDS=host1:port,host2:port      │
│              │ Convenient for Docker/container deployments   │
├──────────────┼──────────────────────────────────────────────┤
│ File         │ Read peers from a file, re-read on change    │
│              │ Useful with config management (Consul, etc.) │
└──────────────┴──────────────────────────────────────────────┘

  Priority: ENV > CLI flag > config file > DNS
  Multiple providers can be combined (union of discovered peers).
```

### 2.3 Node Identity

```rust
/// Persistent node identity, stored on disk across restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    /// Unique node ID (UUID v4, generated once on first boot).
    pub id: Uuid,
    /// Human-readable name (hostname or user-configured).
    pub name: String,
    /// Monotonically increasing incarnation number.
    /// Incremented on every restart to override stale SWIM state.
    pub incarnation: u64,
    /// When this identity was first created.
    pub created_at: DateTime<Utc>,
    /// When this node last started.
    pub last_started_at: DateTime<Utc>,
}

impl NodeIdentity {
    /// Load from disk or create new.
    pub fn load_or_create(data_dir: &Path, name: Option<String>) -> Result<Self, DerivaError> {
        let path = data_dir.join("node_identity.json");
        if path.exists() {
            let json = std::fs::read_to_string(&path)?;
            let mut identity: NodeIdentity = serde_json::from_str(&json)?;
            identity.incarnation += 1;
            identity.last_started_at = Utc::now();
            std::fs::write(&path, serde_json::to_string_pretty(&identity)?)?;
            Ok(identity)
        } else {
            let identity = NodeIdentity {
                id: Uuid::new_v4(),
                name: name.unwrap_or_else(|| hostname::get()
                    .map(|h| h.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "unknown".to_string())),
                incarnation: 0,
                created_at: Utc::now(),
                last_started_at: Utc::now(),
            };
            std::fs::create_dir_all(data_dir)?;
            std::fs::write(&path, serde_json::to_string_pretty(&identity)?)?;
            Ok(identity)
        }
    }
}
```

### 2.4 Bootstrap Configuration

```rust
#[derive(Debug, Clone)]
pub struct BootstrapConfig {
    /// Discovery method(s) to use.
    pub discovery: Vec<DiscoveryProvider>,
    /// Directory for persistent state (node identity, etc.).
    pub data_dir: PathBuf,
    /// Address this node listens on for SWIM gossip.
    pub listen_addr: SocketAddr,
    /// Address this node listens on for gRPC.
    pub grpc_addr: SocketAddr,
    /// Maximum time to wait for bootstrap to complete.
    pub bootstrap_timeout: Duration,          // default: 60s
    /// Time to wait for in-flight requests during drain.
    pub drain_timeout: Duration,              // default: 30s
    /// How long to wait between discovery retry attempts.
    pub discovery_retry_interval: Duration,   // default: 2s
    /// Maximum discovery attempts before giving up.
    pub max_discovery_attempts: u32,          // default: 15
    /// Optional: human-readable node name override.
    pub node_name: Option<String>,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            discovery: vec![DiscoveryProvider::Env],
            data_dir: PathBuf::from("./data"),
            listen_addr: "0.0.0.0:7947".parse().unwrap(),
            grpc_addr: "0.0.0.0:50051".parse().unwrap(),
            bootstrap_timeout: Duration::from_secs(60),
            drain_timeout: Duration::from_secs(30),
            discovery_retry_interval: Duration::from_secs(2),
            max_discovery_attempts: 15,
            node_name: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum DiscoveryProvider {
    /// Static seed list.
    Seeds(Vec<SocketAddr>),
    /// DNS SRV record lookup.
    Dns { service_name: String },
    /// Environment variable (DERIVA_SEEDS).
    Env,
    /// File containing one peer per line.
    File { path: PathBuf },
}
```

### 2.5 Bootstrap State Machine

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapState {
    /// Loading node identity from disk.
    Init,
    /// Discovering peers via configured providers.
    Discovering { attempts: u32 },
    /// Joining SWIM cluster.
    Joining,
    /// Syncing recipes and ring state from peers.
    Syncing { recipes_synced: bool, ring_ready: bool },
    /// Fully operational, serving requests.
    Ready,
    /// Draining in-flight requests before shutdown.
    Draining { since: Instant },
    /// Leaving SWIM cluster and transferring state.
    Leaving,
    /// Stopped.
    Stopped,
    /// Fatal error during bootstrap.
    Failed(String),
}
```

---

## 3. Implementation

### 3.1 Discovery Providers

```rust
// deriva-network/src/discovery.rs

use std::net::SocketAddr;

/// Resolve peers from a discovery provider.
pub async fn discover_peers(provider: &DiscoveryProvider) -> Result<Vec<SocketAddr>, DerivaError> {
    match provider {
        DiscoveryProvider::Seeds(addrs) => Ok(addrs.clone()),

        DiscoveryProvider::Env => {
            match std::env::var("DERIVA_SEEDS") {
                Ok(val) => {
                    let addrs: Result<Vec<SocketAddr>, _> = val
                        .split(',')
                        .map(|s| s.trim().parse())
                        .collect();
                    addrs.map_err(|e| DerivaError::Config(
                        format!("invalid DERIVA_SEEDS: {}", e)
                    ))
                }
                Err(_) => Ok(vec![]), // no env var = no seeds from this provider
            }
        }

        DiscoveryProvider::Dns { service_name } => {
            resolve_dns_srv(service_name).await
        }

        DiscoveryProvider::File { path } => {
            let content = tokio::fs::read_to_string(path).await
                .map_err(|e| DerivaError::Config(format!("seed file: {}", e)))?;
            let addrs: Result<Vec<SocketAddr>, _> = content
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                .map(|l| l.trim().parse())
                .collect();
            addrs.map_err(|e| DerivaError::Config(format!("seed file parse: {}", e)))
        }
    }
}

/// Resolve DNS SRV records to socket addresses.
async fn resolve_dns_srv(service_name: &str) -> Result<Vec<SocketAddr>, DerivaError> {
    use tokio::net::lookup_host;
    // SRV records return host:port pairs
    let addrs: Vec<SocketAddr> = lookup_host(service_name).await
        .map_err(|e| DerivaError::Network(format!("DNS SRV lookup failed: {}", e)))?
        .collect();
    Ok(addrs)
}

/// Combine results from multiple providers, deduplicate.
pub async fn discover_all(
    providers: &[DiscoveryProvider],
    local_addr: &SocketAddr,
) -> Result<Vec<SocketAddr>, DerivaError> {
    let mut all_peers = Vec::new();
    for provider in providers {
        match discover_peers(provider).await {
            Ok(peers) => all_peers.extend(peers),
            Err(e) => tracing::warn!(provider = ?provider, error = %e, "discovery failed"),
        }
    }
    // Deduplicate and remove self
    all_peers.sort();
    all_peers.dedup();
    all_peers.retain(|a| a != local_addr);
    Ok(all_peers)
}
```

### 3.2 Bootstrap Orchestrator

```rust
// deriva-network/src/bootstrap.rs

pub struct BootstrapOrchestrator {
    config: BootstrapConfig,
    state: Arc<RwLock<BootstrapState>>,
    identity: NodeIdentity,
    shutdown_tx: broadcast::Sender<()>,
}

impl BootstrapOrchestrator {
    pub async fn new(config: BootstrapConfig) -> Result<Self, DerivaError> {
        let identity = NodeIdentity::load_or_create(
            &config.data_dir,
            config.node_name.clone(),
        )?;

        tracing::info!(
            id = %identity.id,
            name = %identity.name,
            incarnation = identity.incarnation,
            "node identity loaded"
        );

        let (shutdown_tx, _) = broadcast::channel(1);

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(BootstrapState::Init)),
            identity,
            shutdown_tx,
        })
    }

    /// Run the full bootstrap sequence. Returns when node is Ready.
    pub async fn bootstrap(
        &self,
        swim: &SwimRuntime,
    ) -> Result<(), DerivaError> {
        let deadline = Instant::now() + self.config.bootstrap_timeout;

        // Phase 1: Discover peers
        self.set_state(BootstrapState::Discovering { attempts: 0 });
        let peers = self.discover_with_retry(deadline).await?;

        if peers.is_empty() {
            tracing::info!("no peers found — starting as single-node cluster");
            self.set_state(BootstrapState::Ready);
            return Ok(());
        }

        // Phase 2: Join SWIM cluster
        self.set_state(BootstrapState::Joining);
        tracing::info!(peer_count = peers.len(), "joining cluster");

        for peer in &peers {
            if let Err(e) = swim.join(*peer).await {
                tracing::warn!(peer = %peer, error = %e, "failed to join peer");
            } else {
                tracing::info!(peer = %peer, "joined peer successfully");
                break; // one successful join is enough — gossip propagates
            }
        }

        // Wait for membership to stabilize
        tokio::time::sleep(Duration::from_secs(3)).await;

        let members = swim.alive_members();
        if members.is_empty() {
            return Err(DerivaError::Bootstrap(
                "joined but no alive members detected".into()
            ));
        }

        // Phase 3: Sync state from cluster
        self.set_state(BootstrapState::Syncing {
            recipes_synced: false,
            ring_ready: false,
        });

        self.sync_recipes(swim, deadline).await?;
        self.set_state(BootstrapState::Syncing {
            recipes_synced: true,
            ring_ready: false,
        });

        // Ring is built automatically from SWIM membership (§3.3)
        // Just verify it has the expected number of nodes
        let ring_size = swim.ring().node_count();
        tracing::info!(ring_size = ring_size, "hash ring ready");

        self.set_state(BootstrapState::Syncing {
            recipes_synced: true,
            ring_ready: true,
        });

        // Phase 4: Ready
        self.set_state(BootstrapState::Ready);
        tracing::info!(
            id = %self.identity.id,
            members = members.len() + 1,
            "bootstrap complete — node is ready"
        );

        Ok(())
    }

    /// Discover peers with retry logic.
    async fn discover_with_retry(
        &self,
        deadline: Instant,
    ) -> Result<Vec<SocketAddr>, DerivaError> {
        for attempt in 0..self.config.max_discovery_attempts {
            if Instant::now() >= deadline {
                return Err(DerivaError::Timeout("bootstrap discovery timed out".into()));
            }

            self.set_state(BootstrapState::Discovering { attempts: attempt + 1 });

            let peers = discover_all(
                &self.config.discovery,
                &self.config.listen_addr,
            ).await?;

            if !peers.is_empty() {
                tracing::info!(
                    peers = peers.len(),
                    attempt = attempt + 1,
                    "discovered peers"
                );
                return Ok(peers);
            }

            tracing::debug!(
                attempt = attempt + 1,
                max = self.config.max_discovery_attempts,
                "no peers found, retrying"
            );
            tokio::time::sleep(self.config.discovery_retry_interval).await;
        }

        // No peers after all attempts — this is OK for first node
        tracing::info!("no peers discovered after all attempts");
        Ok(vec![])
    }

    /// Pull recipes from a peer to catch up.
    async fn sync_recipes(
        &self,
        swim: &SwimRuntime,
        deadline: Instant,
    ) -> Result<(), DerivaError> {
        let members = swim.alive_members();
        let Some((peer, _)) = members.iter().next() else {
            return Ok(()); // no peers to sync from
        };

        tracing::info!(peer = %peer, "syncing recipes from peer");

        let channel = swim.get_channel(peer).await?;
        let mut client = DerivaInternalClient::new(channel);

        let timeout_remaining = deadline.duration_since(Instant::now());
        let response = tokio::time::timeout(
            timeout_remaining,
            client.sync_recipes(tonic::Request::new(SyncRecipesRequest {})),
        ).await
            .map_err(|_| DerivaError::Timeout("recipe sync timed out".into()))?
            .map_err(|e| DerivaError::Network(format!("recipe sync failed: {}", e)))?;

        let recipes = response.into_inner();
        tracing::info!(count = recipes.recipes.len(), "received recipes from peer");

        // Recipes are content-addressed — just insert, no conflict possible
        for recipe_bytes in &recipes.recipes {
            // Deserialize and insert into local DAG store
            // (details depend on serialization format)
        }

        Ok(())
    }

    /// Initiate graceful shutdown.
    pub async fn shutdown(&self, swim: &SwimRuntime) -> Result<(), DerivaError> {
        tracing::info!("initiating graceful shutdown");

        // Phase 1: Drain — stop accepting new requests
        self.set_state(BootstrapState::Draining { since: Instant::now() });
        let _ = self.shutdown_tx.send(());

        // Wait for in-flight requests to complete
        tokio::time::sleep(self.config.drain_timeout).await;

        // Phase 2: Leave — notify SWIM cluster
        self.set_state(BootstrapState::Leaving);
        swim.leave().await?;

        // Transfer any pending hinted handoffs to other nodes
        swim.flush_hints().await?;

        // Phase 3: Stop
        self.set_state(BootstrapState::Stopped);
        tracing::info!("shutdown complete");
        Ok(())
    }

    /// Subscribe to shutdown signal.
    pub fn shutdown_receiver(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    pub fn state(&self) -> BootstrapState {
        self.state.read().unwrap().clone()
    }

    pub fn identity(&self) -> &NodeIdentity {
        &self.identity
    }

    fn set_state(&self, new_state: BootstrapState) {
        tracing::debug!(state = ?new_state, "bootstrap state transition");
        *self.state.write().unwrap() = new_state;
    }
}
```

### 3.3 Proto Changes

```protobuf
// Added to proto/deriva_internal.proto

service DerivaInternal {
    // Existing RPCs...

    // NEW: Sync all recipes from a peer (used during bootstrap)
    rpc SyncRecipes(SyncRecipesRequest) returns (SyncRecipesResponse);
}

message SyncRecipesRequest {
    // Empty — request all recipes.
    // Could add a fingerprint for delta sync in the future.
}

message SyncRecipesResponse {
    // Serialized recipes (bincode or protobuf).
    repeated bytes recipes = 1;
    // SHA-256 fingerprint of the full recipe set.
    bytes fingerprint = 2;
}
```

### 3.4 Server Integration

```rust
// Updated main() in deriva-server

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::init();

    let config = load_config()?;

    // 1. Bootstrap orchestrator
    let bootstrap = BootstrapOrchestrator::new(config.bootstrap.clone()).await?;

    // 2. Start SWIM runtime
    let swim = SwimRuntime::new(config.swim.clone()).await?;

    // 3. Run bootstrap sequence
    bootstrap.bootstrap(&swim).await?;

    // 4. Start gRPC server (only after bootstrap completes)
    let server_state = ServerState::new(
        swim.clone(),
        bootstrap.identity().clone(),
        config.clone(),
    );

    let mut shutdown_rx = bootstrap.shutdown_receiver();

    let grpc_server = tonic::transport::Server::builder()
        .add_service(DerivaServiceServer::new(
            DerivaService::new(server_state.clone())
        ))
        .add_service(DerivaInternalServer::new(
            InternalService::new(server_state.clone())
        ))
        .serve_with_shutdown(config.bootstrap.grpc_addr, async {
            let _ = shutdown_rx.recv().await;
        });

    // 5. Handle SIGTERM for graceful shutdown
    let bootstrap_clone = bootstrap.clone();
    let swim_clone = swim.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        bootstrap_clone.shutdown(&swim_clone).await.ok();
    });

    grpc_server.await?;
    Ok(())
}
```

### 3.5 SyncRecipes RPC Handler

```rust
// Added to deriva-server/src/internal.rs

async fn sync_recipes(
    &self,
    _request: Request<SyncRecipesRequest>,
) -> Result<Response<SyncRecipesResponse>, Status> {
    let dag = &self.state.dag_store;
    let all_recipes = dag.all_recipes();

    let serialized: Vec<Vec<u8>> = all_recipes.iter()
        .map(|(_, recipe)| bincode::serialize(recipe).unwrap())
        .collect();

    let fingerprint = dag.fingerprint();

    tracing::info!(
        count = serialized.len(),
        "serving recipe sync request"
    );

    Ok(Response::new(SyncRecipesResponse {
        recipes: serialized,
        fingerprint: fingerprint.to_vec(),
    }))
}
```


---

## 4. Data Flow Diagrams

### 4.1 Cold Start — 3-Node Cluster from Seeds

```
  Node A (seed)          Node B                  Node C
    │                      │                       │
    │ start                │ start                 │ start
    │ load identity        │ load identity         │ load identity
    │ (new, inc=0)         │ (new, inc=0)          │ (new, inc=0)
    │                      │                       │
    │ discover: seeds=     │ discover: seeds=      │ discover: seeds=
    │ [B,C]                │ [A,C]                 │ [A,B]
    │                      │                       │
    │ no peers alive yet   │                       │
    │ retry in 2s...       │                       │
    │                      │ join(A) ─────────────►│
    │◄──── join(B) ────────│                       │
    │                      │                       │
    │ SWIM: A↔B connected  │                       │
    │                      │                       │
    │                      │                       │ join(A) ──────►│
    │◄──── join(C) ────────────────────────────────│               │
    │                      │                       │               │
    │ SWIM: A↔B↔C          │ gossip propagates     │               │
    │                      │                       │               │
    │ sync_recipes(B) → 0  │                       │               │
    │ ring: [A,B,C]        │ ring: [A,B,C]         │ ring: [A,B,C] │
    │                      │                       │               │
    │ READY ✓              │ READY ✓               │ READY ✓       │

  Total time: ~5-8 seconds (discovery retries + gossip convergence)
```

### 4.2 Node Restart — Rejoin with Same Identity

```
  Node B (restarting)     Node A (existing)       Node C (existing)
    │                       │                       │
    │ start                 │                       │
    │ load identity         │                       │
    │ (existing, inc=3→4)   │                       │
    │                       │                       │
    │ discover: seeds=[A,C] │                       │
    │                       │                       │
    │── join(A) ───────────►│                       │
    │                       │ SWIM: B alive (inc=4) │
    │                       │ overrides stale B     │
    │                       │ (inc=3, was Dead)     │
    │                       │                       │
    │                       │── gossip(B alive) ───►│
    │                       │                       │ update B: alive
    │                       │                       │
    │── sync_recipes(A) ───►│                       │
    │◄── 150 recipes ──────│                       │
    │                       │                       │
    │ ring: [A,B,C]         │                       │
    │ READY ✓               │                       │

  Key: incarnation=4 > 3 tells SWIM this is a fresh restart,
  not a stale message about the old B.
```

### 4.3 Graceful Shutdown

```
  Node B (shutting down)  Node A                  Node C
    │                       │                       │
    │ SIGTERM received      │                       │
    │                       │                       │
    │ state → Draining      │                       │
    │ stop accepting new    │                       │
    │ requests              │                       │
    │                       │                       │
    │ wait 30s for          │                       │
    │ in-flight to finish   │                       │
    │ ...................... │                       │
    │                       │                       │
    │ state → Leaving       │                       │
    │                       │                       │
    │── SWIM leave() ──────►│                       │
    │── SWIM leave() ──────────────────────────────►│
    │                       │ mark B as Left        │ mark B as Left
    │                       │ ring: [A,C]           │ ring: [A,C]
    │                       │                       │
    │── flush hints ───────►│ (hinted handoffs      │
    │                       │  for B's data)        │
    │                       │                       │
    │ state → Stopped       │                       │
    │ exit(0)               │                       │

  After B leaves, A and C rebalance B's key ranges (§3.10).
```

### 4.4 DNS Discovery — Kubernetes Headless Service

```
  Kubernetes cluster:
    Service: deriva-headless (clusterIP: None)
    StatefulSet: deriva (replicas: 3)
      Pod: deriva-0 → 10.244.0.5
      Pod: deriva-1 → 10.244.0.6
      Pod: deriva-2 → 10.244.0.7

  DNS SRV record: _deriva._tcp.deriva-headless.default.svc.cluster.local
    → 10.244.0.5:7947
    → 10.244.0.6:7947
    → 10.244.0.7:7947

  Node config:
    discovery: [Dns { service_name: "deriva-headless.default.svc.cluster.local:7947" }]

  Bootstrap:
    1. Resolve DNS → [10.244.0.5, 10.244.0.6, 10.244.0.7]
    2. Remove self → [other two pods]
    3. Join first reachable peer
    4. Gossip propagates full membership
    5. Ready

  On pod restart: same flow, incarnation increments.
  On scale-up: new pod discovers existing pods via DNS.
```

### 4.5 First Node in Cluster (No Peers)

```
  Node A (first node)
    │
    │ start
    │ load identity (new, inc=0)
    │
    │ discover: seeds=[B:7947, C:7947]
    │ attempt 1: B unreachable, C unreachable
    │ attempt 2: B unreachable, C unreachable
    │ ...
    │ attempt 15: B unreachable, C unreachable
    │
    │ "no peers discovered after all attempts"
    │ "starting as single-node cluster"
    │
    │ state → Ready (single-node mode)
    │
    │ (later, when B starts and joins A,
    │  A transitions to multi-node automatically
    │  via SWIM membership callback)
```

---

## 5. Test Specification

### 5.1 Node Identity Tests

```rust
#[cfg(test)]
mod identity_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_new_identity() {
        let dir = TempDir::new().unwrap();
        let id = NodeIdentity::load_or_create(dir.path(), Some("test-node".into())).unwrap();
        assert_eq!(id.name, "test-node");
        assert_eq!(id.incarnation, 0);
        assert!(dir.path().join("node_identity.json").exists());
    }

    #[test]
    fn test_reload_increments_incarnation() {
        let dir = TempDir::new().unwrap();
        let id1 = NodeIdentity::load_or_create(dir.path(), Some("n".into())).unwrap();
        assert_eq!(id1.incarnation, 0);

        let id2 = NodeIdentity::load_or_create(dir.path(), None).unwrap();
        assert_eq!(id2.incarnation, 1);
        assert_eq!(id2.id, id1.id); // same UUID
        assert_eq!(id2.name, "n");  // name preserved

        let id3 = NodeIdentity::load_or_create(dir.path(), None).unwrap();
        assert_eq!(id3.incarnation, 2);
        assert_eq!(id3.id, id1.id);
    }

    #[test]
    fn test_identity_persists_across_loads() {
        let dir = TempDir::new().unwrap();
        let id1 = NodeIdentity::load_or_create(dir.path(), Some("node-a".into())).unwrap();

        // Simulate restart
        let id2 = NodeIdentity::load_or_create(dir.path(), None).unwrap();
        assert_eq!(id1.id, id2.id);
        assert_eq!(id1.name, id2.name);
        assert_eq!(id1.created_at, id2.created_at);
        assert!(id2.last_started_at > id1.last_started_at);
    }

    #[test]
    fn test_identity_creates_data_dir() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("deep").join("nested");
        let id = NodeIdentity::load_or_create(&nested, None).unwrap();
        assert!(nested.join("node_identity.json").exists());
        assert_eq!(id.incarnation, 0);
    }
}
```

### 5.2 Discovery Provider Tests

```rust
#[cfg(test)]
mod discovery_tests {
    use super::*;

    #[test]
    fn test_static_seeds() {
        let seeds = vec![
            "10.0.1.1:7947".parse().unwrap(),
            "10.0.1.2:7947".parse().unwrap(),
        ];
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(discover_peers(&DiscoveryProvider::Seeds(seeds.clone())));
        assert_eq!(result.unwrap(), seeds);
    }

    #[tokio::test]
    async fn test_env_discovery() {
        std::env::set_var("DERIVA_SEEDS", "127.0.0.1:7947,127.0.0.1:7948");
        let result = discover_peers(&DiscoveryProvider::Env).await.unwrap();
        assert_eq!(result.len(), 2);
        std::env::remove_var("DERIVA_SEEDS");
    }

    #[tokio::test]
    async fn test_env_discovery_missing() {
        std::env::remove_var("DERIVA_SEEDS");
        let result = discover_peers(&DiscoveryProvider::Env).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_file_discovery() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("seeds.txt");
        std::fs::write(&path, "127.0.0.1:7947\n127.0.0.1:7948\n# comment\n\n").unwrap();

        let result = discover_peers(&DiscoveryProvider::File { path }).await.unwrap();
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_file_discovery_with_comments_and_blanks() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("seeds.txt");
        std::fs::write(&path, "# cluster seeds\n\n127.0.0.1:7947\n\n# end\n").unwrap();

        let result = discover_peers(&DiscoveryProvider::File { path }).await.unwrap();
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn test_discover_all_deduplicates() {
        let local: SocketAddr = "127.0.0.1:7947".parse().unwrap();
        let peer: SocketAddr = "127.0.0.1:7948".parse().unwrap();

        let providers = vec![
            DiscoveryProvider::Seeds(vec![peer, local, peer]), // dup + self
        ];
        let result = discover_all(&providers, &local).await.unwrap();
        assert_eq!(result, vec![peer]); // deduped, self removed
    }

    #[tokio::test]
    async fn test_discover_all_combines_providers() {
        let local: SocketAddr = "127.0.0.1:7947".parse().unwrap();
        let providers = vec![
            DiscoveryProvider::Seeds(vec!["127.0.0.1:7948".parse().unwrap()]),
            DiscoveryProvider::Seeds(vec!["127.0.0.1:7949".parse().unwrap()]),
        ];
        let result = discover_all(&providers, &local).await.unwrap();
        assert_eq!(result.len(), 2);
    }
}
```

### 5.3 Bootstrap State Machine Tests

```rust
#[cfg(test)]
mod bootstrap_state_tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BootstrapConfig::default();
        assert_eq!(config.bootstrap_timeout, Duration::from_secs(60));
        assert_eq!(config.drain_timeout, Duration::from_secs(30));
        assert_eq!(config.max_discovery_attempts, 15);
    }

    #[test]
    fn test_state_transitions() {
        // Verify all valid state transitions compile
        let states = vec![
            BootstrapState::Init,
            BootstrapState::Discovering { attempts: 1 },
            BootstrapState::Joining,
            BootstrapState::Syncing { recipes_synced: false, ring_ready: false },
            BootstrapState::Syncing { recipes_synced: true, ring_ready: false },
            BootstrapState::Syncing { recipes_synced: true, ring_ready: true },
            BootstrapState::Ready,
            BootstrapState::Draining { since: Instant::now() },
            BootstrapState::Leaving,
            BootstrapState::Stopped,
            BootstrapState::Failed("test".into()),
        ];
        assert_eq!(states.len(), 11);
    }
}
```

### 5.4 Integration Tests

```rust
#[tokio::test]
async fn test_bootstrap_single_node() {
    let dir = TempDir::new().unwrap();
    let config = BootstrapConfig {
        discovery: vec![DiscoveryProvider::Seeds(vec![])], // no peers
        data_dir: dir.path().to_path_buf(),
        listen_addr: "127.0.0.1:17947".parse().unwrap(),
        max_discovery_attempts: 1, // don't retry
        ..Default::default()
    };

    let orchestrator = BootstrapOrchestrator::new(config).await.unwrap();
    let swim = SwimRuntime::new(SwimConfig::default()).await.unwrap();

    orchestrator.bootstrap(&swim).await.unwrap();
    assert_eq!(orchestrator.state(), BootstrapState::Ready);
}

#[tokio::test]
async fn test_bootstrap_two_nodes() {
    let dir_a = TempDir::new().unwrap();
    let dir_b = TempDir::new().unwrap();

    let addr_a: SocketAddr = "127.0.0.1:17950".parse().unwrap();
    let addr_b: SocketAddr = "127.0.0.1:17951".parse().unwrap();

    // Start node A (no seeds — becomes single node)
    let config_a = BootstrapConfig {
        discovery: vec![DiscoveryProvider::Seeds(vec![])],
        data_dir: dir_a.path().to_path_buf(),
        listen_addr: addr_a,
        max_discovery_attempts: 1,
        ..Default::default()
    };
    let orch_a = BootstrapOrchestrator::new(config_a).await.unwrap();
    let swim_a = SwimRuntime::new(SwimConfig { listen_addr: addr_a, ..Default::default() }).await.unwrap();
    orch_a.bootstrap(&swim_a).await.unwrap();

    // Start node B (seed = A)
    let config_b = BootstrapConfig {
        discovery: vec![DiscoveryProvider::Seeds(vec![addr_a])],
        data_dir: dir_b.path().to_path_buf(),
        listen_addr: addr_b,
        ..Default::default()
    };
    let orch_b = BootstrapOrchestrator::new(config_b).await.unwrap();
    let swim_b = SwimRuntime::new(SwimConfig { listen_addr: addr_b, ..Default::default() }).await.unwrap();
    orch_b.bootstrap(&swim_b).await.unwrap();

    assert_eq!(orch_b.state(), BootstrapState::Ready);

    // Both should see each other
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(swim_a.alive_members().len() >= 1);
    assert!(swim_b.alive_members().len() >= 1);
}

#[tokio::test]
async fn test_graceful_shutdown() {
    let dir = TempDir::new().unwrap();
    let config = BootstrapConfig {
        discovery: vec![DiscoveryProvider::Seeds(vec![])],
        data_dir: dir.path().to_path_buf(),
        listen_addr: "127.0.0.1:17952".parse().unwrap(),
        drain_timeout: Duration::from_millis(100), // fast for test
        max_discovery_attempts: 1,
        ..Default::default()
    };

    let orch = BootstrapOrchestrator::new(config).await.unwrap();
    let swim = SwimRuntime::new(SwimConfig::default()).await.unwrap();
    orch.bootstrap(&swim).await.unwrap();
    assert_eq!(orch.state(), BootstrapState::Ready);

    orch.shutdown(&swim).await.unwrap();
    assert_eq!(orch.state(), BootstrapState::Stopped);
}

#[tokio::test]
async fn test_node_restart_preserves_identity() {
    let dir = TempDir::new().unwrap();

    // First boot
    let config = BootstrapConfig {
        data_dir: dir.path().to_path_buf(),
        ..Default::default()
    };
    let orch1 = BootstrapOrchestrator::new(config.clone()).await.unwrap();
    let id1 = orch1.identity().clone();

    // Second boot (simulated restart)
    let orch2 = BootstrapOrchestrator::new(config).await.unwrap();
    let id2 = orch2.identity().clone();

    assert_eq!(id1.id, id2.id);
    assert_eq!(id1.name, id2.name);
    assert_eq!(id2.incarnation, id1.incarnation + 1);
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | All seeds unreachable | Start as single-node after max attempts | First node in cluster |
| 2 | DNS returns empty | Treat as no peers, retry | DNS may not be ready yet |
| 3 | Seed file missing | Log warning, skip provider | Non-fatal, other providers may work |
| 4 | Seed file has invalid entries | Skip invalid, use valid ones | Partial config is better than none |
| 5 | Identity file corrupted | Delete and create new identity | Fresh start, incarnation=0 |
| 6 | Data dir not writable | Fatal error, exit(1) | Cannot persist state |
| 7 | SWIM join fails for all peers | Retry with discovery | Peers may be starting up |
| 8 | Recipe sync peer crashes mid-transfer | Retry with different peer | Multiple peers available |
| 9 | Bootstrap timeout exceeded | Fatal error, exit(1) | Don't serve with incomplete state |
| 10 | SIGTERM during bootstrap | Skip to shutdown immediately | Clean exit |
| 11 | Two nodes start simultaneously | Both discover each other, both join | SWIM handles mutual join |
| 12 | Node restarts faster than SWIM detects death | Incarnation increment overrides stale Dead state | Correct by design |
| 13 | Drain timeout exceeded | Force shutdown, drop in-flight | Bounded shutdown time |
| 14 | Network partition during bootstrap | Node may start as single-node | Partition heals → SWIM merges |

### 6.1 Split-Brain Prevention

```
Scenario: Network partition during bootstrap.

  Partition 1: [A]     Partition 2: [B, C]

  A starts, discovers no peers → single-node mode
  B,C start, discover each other → 2-node cluster

  Result: two independent clusters (split-brain)

  When partition heals:
  - SWIM gossip merges membership lists
  - A discovers B,C, B,C discover A
  - Recipe anti-entropy (§3.2) syncs recipes
  - Ring rebalances (§3.10)

  Content-addressing prevents data conflicts:
  - Same data on both sides → same CAddr → no conflict
  - Different data → different CAddr → both preserved

  ∴ Split-brain is self-healing for Deriva. No manual intervention needed.
  The only risk is temporary inconsistency during the partition.
```

### 6.2 Thundering Herd on Startup

```
Scenario: 50-node cluster, all nodes restart simultaneously.

  All 50 nodes try to join seeds at the same time.
  Seeds receive 49 join requests each.

  Mitigation:
  - Jittered discovery retry: add random 0-1s to retry interval
  - SWIM join is lightweight (single UDP packet)
  - Only one successful join needed per node (gossip propagates)

  Expected behavior:
  - First few nodes form a core cluster in ~3s
  - Remaining nodes join via gossip in ~5-10s
  - Full cluster ready in ~15s
```

---

## 7. Performance Analysis

### 7.1 Bootstrap Latency Breakdown

```
┌──────────────────────────┬──────────┬──────────────────────────┐
│ Phase                    │ Latency  │ Determined by            │
├──────────────────────────┼──────────┼──────────────────────────┤
│ Load identity from disk  │ ~1ms     │ JSON parse               │
│ Discovery (seeds)        │ ~0ms     │ In-memory list           │
│ Discovery (DNS)          │ ~50ms    │ DNS resolution           │
│ Discovery (file)         │ ~1ms     │ File read                │
│ SWIM join                │ ~5ms     │ UDP round-trip           │
│ Gossip convergence       │ ~3s      │ SWIM protocol interval   │
│ Recipe sync (100 recipes)│ ~50ms    │ gRPC + serialization     │
│ Recipe sync (10K recipes)│ ~500ms   │ gRPC + serialization     │
│ Ring construction        │ ~1ms     │ In-memory hash ring      │
├──────────────────────────┼──────────┼──────────────────────────┤
│ Total (typical)          │ ~4s      │ Dominated by gossip      │
│ Total (large cluster)    │ ~8s      │ More gossip rounds       │
└──────────────────────────┴──────────┴──────────────────────────┘
```

### 7.2 Shutdown Latency

```
┌──────────────────────────┬──────────┬──────────────────────────┐
│ Phase                    │ Latency  │ Determined by            │
├──────────────────────────┼──────────┼──────────────────────────┤
│ Drain (wait for in-flight│ 0-30s    │ Configurable timeout     │
│ SWIM leave broadcast     │ ~5ms     │ UDP multicast            │
│ Hint flush               │ ~100ms   │ gRPC to peers            │
├──────────────────────────┼──────────┼──────────────────────────┤
│ Total (typical)          │ ~1s      │ Most requests finish fast│
│ Total (worst case)       │ ~30s     │ Drain timeout            │
└──────────────────────────┴──────────┴──────────────────────────┘
```

### 7.3 Benchmarking Plan

```rust
/// Benchmark: identity load/create
#[bench]
fn bench_identity_load(b: &mut Bencher) {
    // Pre-create identity file
    // Measure: load_or_create latency
    // Expected: <1ms
}

/// Benchmark: discovery from file with 100 entries
#[bench]
fn bench_file_discovery(b: &mut Bencher) {
    // 100-line seed file
    // Measure: discover_peers latency
    // Expected: <2ms
}

/// Benchmark: bootstrap to ready (3-node cluster)
#[bench]
fn bench_full_bootstrap(b: &mut Bencher) {
    // 3 nodes, static seeds
    // Measure: time from start to Ready state
    // Expected: <5s
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/bootstrap.rs` | **NEW** — BootstrapOrchestrator, BootstrapState, BootstrapConfig |
| `deriva-network/src/discovery.rs` | **NEW** — DiscoveryProvider, discover_peers, discover_all |
| `deriva-network/src/identity.rs` | **NEW** — NodeIdentity |
| `deriva-network/src/lib.rs` | Add `pub mod bootstrap, discovery, identity` |
| `proto/deriva_internal.proto` | Add `SyncRecipes` RPC |
| `deriva-server/src/internal.rs` | Add `sync_recipes` handler |
| `deriva-server/src/main.rs` | Rewrite startup to use BootstrapOrchestrator |
| `deriva-server/src/state.rs` | Add `bootstrap: BootstrapOrchestrator` |
| `deriva-network/tests/bootstrap.rs` | **NEW** — integration tests |
| `deriva-network/tests/discovery.rs` | **NEW** — unit tests |
| `deriva-network/tests/identity.rs` | **NEW** — unit tests |

---

## 9. Dependency Changes

| Crate | Version | Purpose |
|-------|---------|---------|
| `uuid` | `1.x` | Node identity UUID (already used in §3.6) |
| `chrono` | `0.4` | Timestamps in NodeIdentity |
| `hostname` | `0.4` | Auto-detect node name from OS hostname |
| `serde_json` | `1.x` | Serialize NodeIdentity to disk |
| `tempfile` | `3.x` | Test-only: temporary directories |

---

## 10. Design Rationale

### 10.1 Why Seed Nodes Instead of Multicast/Broadcast?

```
Multicast discovery:
  + Zero configuration
  - Doesn't work across subnets/VPCs
  - Blocked by many cloud network configurations
  - Unreliable in containerized environments

Seed nodes:
  + Works everywhere (any network topology)
  + Explicit, predictable behavior
  + Standard pattern (Cassandra, Elasticsearch, Consul)
  - Requires configuration

  Seed nodes are the industry standard for a reason.
  DNS discovery covers the Kubernetes use case.
```

### 10.2 Why Incarnation Numbers Instead of Timestamps?

```
Timestamps:
  - Require synchronized clocks (NTP)
  - Clock skew can cause stale state to win
  - Leap seconds, timezone issues

Incarnation numbers:
  - Monotonically increasing integer
  - No clock dependency
  - Node controls its own incarnation
  - Higher incarnation always wins (simple comparison)

  SWIM protocol already uses incarnation numbers.
  We persist them to survive restarts.
```

### 10.3 Why Bootstrap Before Serving?

```
Alternative: start serving immediately, sync in background.

  Risk: client sends Get(addr) before recipes are synced
  → node doesn't know the recipe → returns NotFound
  → client retries on another node → succeeds
  → inconsistent behavior, confusing errors

  By blocking until Ready:
  - Node has all recipes before first request
  - Ring is populated with all known members
  - No spurious NotFound errors
  - Clean, predictable startup behavior

  Trade-off: ~4s startup delay. Acceptable for a distributed system.
```

### 10.4 Why Graceful Shutdown Matters

```
Without graceful shutdown:
  - In-flight requests get connection reset
  - Hinted handoffs are lost (data loss risk)
  - SWIM detects death via timeout (~10s delay)
  - Ring rebalancing delayed

With graceful shutdown:
  - In-flight requests complete normally
  - Hints transferred to other nodes
  - SWIM notified immediately (leave message)
  - Ring rebalancing starts immediately

  Especially important for rolling upgrades (§3.17):
  drain → upgrade → restart → bootstrap → ready
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `bootstrap_state` | Gauge | `state` | Current bootstrap state (encoded as int) |
| `bootstrap_duration_ms` | Histogram | — | Time from Init to Ready |
| `bootstrap_discovery_attempts` | Counter | — | Total discovery attempts |
| `bootstrap_discovery_peers` | Gauge | — | Peers found in last discovery |
| `bootstrap_recipe_sync_count` | Counter | — | Recipes synced during bootstrap |
| `bootstrap_recipe_sync_ms` | Histogram | — | Recipe sync latency |
| `node_incarnation` | Gauge | — | Current incarnation number |
| `shutdown_drain_duration_ms` | Histogram | — | Time spent in drain phase |

### 11.2 Structured Logging

```rust
tracing::info!(
    id = %identity.id,
    name = %identity.name,
    incarnation = identity.incarnation,
    "node identity loaded"
);

tracing::info!(
    peers = peers.len(),
    attempt = attempt,
    providers = config.discovery.len(),
    "peers discovered"
);

tracing::info!(
    members = member_count,
    recipes = recipe_count,
    ring_size = ring_size,
    bootstrap_ms = elapsed.as_millis(),
    "bootstrap complete — node is ready"
);

tracing::info!(
    drain_ms = drain_elapsed.as_millis(),
    inflight = inflight_count,
    "graceful shutdown complete"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| Bootstrap failed | `bootstrap_state` = Failed for >1m | Critical |
| Bootstrap slow | `bootstrap_duration_ms` > 30s | Warning |
| No peers discovered | `bootstrap_discovery_peers` = 0 after Ready | Info |
| High incarnation | `node_incarnation` > 100 (frequent restarts) | Warning |

---

## 12. Checklist

- [ ] Create `deriva-network/src/identity.rs`
- [ ] Implement `NodeIdentity` with load/create/persist
- [ ] Create `deriva-network/src/discovery.rs`
- [ ] Implement `DiscoveryProvider` enum (Seeds, DNS, Env, File)
- [ ] Implement `discover_peers` and `discover_all`
- [ ] Create `deriva-network/src/bootstrap.rs`
- [ ] Implement `BootstrapOrchestrator` with state machine
- [ ] Implement bootstrap sequence (discover → join → sync → ready)
- [ ] Implement graceful shutdown (drain → leave → flush → stop)
- [ ] Add `SyncRecipes` RPC to proto
- [ ] Implement `sync_recipes` handler
- [ ] Rewrite `main()` to use bootstrap orchestrator
- [ ] Add SIGTERM handler for graceful shutdown
- [ ] Add jittered retry for discovery
- [ ] Write NodeIdentity unit tests (4 tests)
- [ ] Write discovery provider unit tests (6 tests)
- [ ] Write bootstrap state tests (2 tests)
- [ ] Write integration tests (4 tests)
- [ ] Add metrics (8 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (4 alerts)
- [ ] Run benchmarks: identity load, discovery, full bootstrap
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
