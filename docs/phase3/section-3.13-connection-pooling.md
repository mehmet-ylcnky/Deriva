# §3.13 Connection Pooling & Channel Management

> **Status:** Not started
> **Depends on:** §3.1 (SWIM gossip), §3.12 (mTLS)
> **Crate(s):** `deriva-network`, `deriva-server`
> **Estimated effort:** 2 days

---

## 1. Problem Statement

Every inter-node RPC in Deriva (FetchValue, Replicate, GcCollectRoots, MigrateTransfer,
etc.) requires a gRPC channel to the target peer. Without connection pooling:

1. **Connection storm on startup**: a 16-node cluster where each node connects to
   all 15 peers creates 240 TCP connections simultaneously. With mTLS, each requires
   a TLS handshake (~1.5ms). Burst of 240 handshakes saturates CPU for ~360ms and
   may trigger SYN flood protection on firewalls.

2. **Per-request connection overhead**: creating a new TCP + TLS connection per RPC
   adds ~2ms latency. For a FetchValue that takes 5ms of actual work, connection
   setup doubles total latency.

3. **File descriptor exhaustion**: without reuse, a busy node may open thousands of
   connections. Linux default ulimit is 1024 FDs. Each TCP connection = 1 FD.

4. **Stale connections**: a peer crashes and restarts. Old connections are broken but
   the caller doesn't know until the next RPC times out (30s default). Meanwhile,
   all RPCs to that peer fail slowly.

5. **Uneven load**: without channel management, all RPCs to a peer funnel through
   one HTTP/2 connection. HTTP/2 multiplexing handles this well up to ~100 concurrent
   streams, but beyond that, head-of-line blocking at the TCP level degrades throughput.

### Goals

- **Persistent channel pool**: one gRPC channel per peer, reused across all RPCs.
  Channels are created lazily on first use and kept alive.
- **Health checking**: periodic pings detect dead connections. Broken channels are
  replaced transparently.
- **Warm-up on join**: when SWIM detects a new peer, pre-establish the channel
  before any RPC needs it.
- **Idle timeout**: channels unused for 5 minutes are closed to free resources.
- **Exponential backoff reconnection**: failed connection attempts back off
  (100ms → 200ms → 400ms → ... → 30s cap) to avoid hammering a recovering peer.
- **Multi-channel option**: for high-throughput peers, allow N channels (default 1,
  configurable up to 4) to overcome HTTP/2 stream limits.
- **Graceful drain**: on shutdown, finish in-flight RPCs before closing channels.

### Non-Goals

- Connection pooling for client-facing gRPC (clients manage their own connections).
- Load balancing across multiple channels to same peer (future optimization).

---

## 2. Design

### 2.1 Architecture Overview

```
  ┌─────────────────────────────────────────────────────────────┐
  │                      ChannelPool                            │
  │                                                             │
  │  ┌──────────────────────────────────────────────────────┐   │
  │  │  peer_channels: DashMap<NodeId, PeerChannel>         │   │
  │  │                                                      │   │
  │  │  NodeA ──► PeerChannel {                             │   │
  │  │              channels: [Channel; 1],                 │   │
  │  │              state: Connected,                       │   │
  │  │              last_used: Instant,                     │   │
  │  │              health: Healthy,                        │   │
  │  │            }                                         │   │
  │  │                                                      │   │
  │  │  NodeB ──► PeerChannel {                             │   │
  │  │              channels: [Channel; 1],                 │   │
  │  │              state: Connected,                       │   │
  │  │              health: Healthy,                        │   │
  │  │            }                                         │   │
  │  │                                                      │   │
  │  │  NodeC ──► PeerChannel {                             │   │
  │  │              channels: [Channel; 1],                 │   │
  │  │              state: Reconnecting { attempt: 2 },     │   │
  │  │              health: Unhealthy,                      │   │
  │  │            }                                         │   │
  │  └──────────────────────────────────────────────────────┘   │
  │                                                             │
  │  Background tasks:                                          │
  │  ├── health_check_loop (every 10s)                          │
  │  ├── idle_reaper_loop (every 60s)                           │
  │  └── warm_up on SWIM join events                            │
  └─────────────────────────────────────────────────────────────┘
```

### 2.2 Channel Lifecycle

```
  SWIM: peer joined
       │
       ▼
  ┌──────────┐    connect()    ┌───────────┐
  │  Empty   │ ──────────────► │ Connected │◄──────────────┐
  └──────────┘                 └─────┬─────┘               │
                                     │                     │
                              health check fail            │
                              or RPC error                 │
                                     │                     │
                                     ▼                     │
                              ┌──────────────┐   success   │
                              │ Reconnecting │─────────────┘
                              │ (backoff)    │
                              └──────┬───────┘
                                     │
                              max retries or
                              SWIM: peer left
                                     │
                                     ▼
                              ┌──────────┐
                              │ Removed  │
                              └──────────┘

  Idle timeout:
  Connected ──(unused 5min)──► Idle ──(close)──► Empty
  Next RPC to this peer: reconnect on demand.
```

### 2.3 Core Types

```rust
/// Pool of gRPC channels to cluster peers.
pub struct ChannelPool {
    config: ChannelPoolConfig,
    tls_manager: Option<Arc<TlsManager>>,
    channels: Arc<DashMap<NodeId, PeerChannel>>,
    /// Maps NodeId → socket address for connection.
    peer_addrs: Arc<DashMap<NodeId, SocketAddr>>,
    cancel: CancellationToken,
}

#[derive(Debug, Clone)]
pub struct ChannelPoolConfig {
    /// Number of gRPC channels per peer.
    pub channels_per_peer: usize,        // default: 1
    /// Idle timeout before closing unused channel.
    pub idle_timeout: Duration,          // default: 5 min
    /// Health check interval.
    pub health_check_interval: Duration, // default: 10s
    /// Health check timeout.
    pub health_check_timeout: Duration,  // default: 3s
    /// Max consecutive health check failures before marking unhealthy.
    pub max_health_failures: u32,        // default: 3
    /// Initial reconnect delay.
    pub reconnect_base_delay: Duration,  // default: 100ms
    /// Max reconnect delay.
    pub reconnect_max_delay: Duration,   // default: 30s
    /// Connect timeout for new channels.
    pub connect_timeout: Duration,       // default: 5s
    /// HTTP/2 keep-alive interval.
    pub http2_keepalive: Duration,       // default: 30s
    /// Max concurrent RPCs per channel (HTTP/2 streams).
    pub max_concurrent_streams: u32,     // default: 100
    /// Idle reaper check interval.
    pub reaper_interval: Duration,       // default: 60s
    /// Pre-warm channels on SWIM join.
    pub warm_on_join: bool,              // default: true
}

impl Default for ChannelPoolConfig {
    fn default() -> Self {
        Self {
            channels_per_peer: 1,
            idle_timeout: Duration::from_secs(300),
            health_check_interval: Duration::from_secs(10),
            health_check_timeout: Duration::from_secs(3),
            max_health_failures: 3,
            reconnect_base_delay: Duration::from_millis(100),
            reconnect_max_delay: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            http2_keepalive: Duration::from_secs(30),
            max_concurrent_streams: 100,
            reaper_interval: Duration::from_secs(60),
            warm_on_join: true,
        }
    }
}

/// State for a single peer's channel(s).
pub struct PeerChannel {
    pub node_id: NodeId,
    pub addr: SocketAddr,
    pub channels: Vec<Channel>,
    pub state: PeerChannelState,
    pub health: HealthState,
    pub last_used: Instant,
    pub last_health_check: Instant,
    /// Round-robin index for multi-channel.
    pub rr_index: AtomicUsize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PeerChannelState {
    Connected,
    Reconnecting {
        attempt: u32,
        next_retry: Instant,
    },
    Draining,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HealthState {
    Healthy,
    Degraded { consecutive_failures: u32 },
    Unhealthy,
    Unknown,
}
```

### 2.4 Backoff Strategy

```
  Attempt  Delay       Cumulative
  1        100ms       100ms
  2        200ms       300ms
  3        400ms       700ms
  4        800ms       1.5s
  5        1.6s        3.1s
  6        3.2s        6.3s
  7        6.4s        12.7s
  8        12.8s       25.5s
  9        25.6s       51.1s
  10       30s (cap)   81.1s
  11+      30s (cap)   ...

  With ±20% jitter to prevent thundering herd:
  Attempt 1: 80ms–120ms
  Attempt 5: 1.28s–1.92s
```

---

## 3. Implementation

### 3.1 ChannelPool Core

```rust
impl ChannelPool {
    pub fn new(
        config: ChannelPoolConfig,
        tls_manager: Option<Arc<TlsManager>>,
    ) -> Self {
        let pool = Self {
            config,
            tls_manager,
            channels: Arc::new(DashMap::new()),
            peer_addrs: Arc::new(DashMap::new()),
            cancel: CancellationToken::new(),
        };
        pool.spawn_background_tasks();
        pool
    }

    /// Get a channel to the specified peer. Creates one if needed.
    pub async fn get_channel(&self, peer: &NodeId) -> Result<Channel, DerivaError> {
        // Fast path: existing healthy channel
        if let Some(mut entry) = self.channels.get_mut(peer) {
            let pc = entry.value_mut();
            if pc.state == PeerChannelState::Connected
                && pc.health != HealthState::Unhealthy
            {
                pc.last_used = Instant::now();
                let idx = pc.rr_index.fetch_add(1, Ordering::Relaxed) % pc.channels.len();
                return Ok(pc.channels[idx].clone());
            }
        }

        // Slow path: create or reconnect
        self.ensure_channel(peer).await
    }

    /// Ensure a healthy channel exists for the peer.
    async fn ensure_channel(&self, peer: &NodeId) -> Result<Channel, DerivaError> {
        let addr = self.peer_addrs.get(peer)
            .map(|e| *e.value())
            .ok_or_else(|| DerivaError::Network(format!(
                "unknown peer: {}", peer
            )))?;

        let channels = self.create_channels(&addr).await?;

        let pc = PeerChannel {
            node_id: *peer,
            addr,
            channels: channels.clone(),
            state: PeerChannelState::Connected,
            health: HealthState::Healthy,
            last_used: Instant::now(),
            last_health_check: Instant::now(),
            rr_index: AtomicUsize::new(0),
        };

        self.channels.insert(*peer, pc);
        Ok(channels[0].clone())
    }

    /// Create N channels to a peer address.
    async fn create_channels(
        &self,
        addr: &SocketAddr,
    ) -> Result<Vec<Channel>, DerivaError> {
        let mut channels = Vec::with_capacity(self.config.channels_per_peer);

        for _ in 0..self.config.channels_per_peer {
            let channel = self.build_endpoint(addr).await?;
            channels.push(channel);
        }

        Ok(channels)
    }

    /// Build a single tonic endpoint with all settings.
    async fn build_endpoint(
        &self,
        addr: &SocketAddr,
    ) -> Result<Channel, DerivaError> {
        let uri = if self.tls_manager.is_some() {
            format!("https://{}", addr)
        } else {
            format!("http://{}", addr)
        };

        let mut endpoint = Channel::from_shared(uri)
            .map_err(|e| DerivaError::Network(format!("bad uri: {}", e)))?
            .connect_timeout(self.config.connect_timeout)
            .timeout(Duration::from_secs(30))
            .http2_keep_alive_interval(self.config.http2_keepalive)
            .keep_alive_timeout(Duration::from_secs(10))
            .keep_alive_while_idle(true)
            .initial_stream_window_size(1 << 20)  // 1MB
            .initial_connection_window_size(1 << 22); // 4MB

        if let Some(ref tls) = self.tls_manager {
            endpoint = endpoint.tls_config(tls.client_tls())
                .map_err(|e| DerivaError::Network(format!("tls: {}", e)))?;
        }

        let channel = endpoint.connect().await
            .map_err(|e| DerivaError::Network(format!(
                "connect to {}: {}", addr, e
            )))?;

        Ok(channel)
    }

    /// Register a peer's address (called from SWIM on join).
    pub fn register_peer(&self, peer: NodeId, addr: SocketAddr) {
        self.peer_addrs.insert(peer, addr);
        if self.config.warm_on_join {
            let pool = self.clone_inner();
            tokio::spawn(async move {
                if let Err(e) = pool.ensure_channel(&peer).await {
                    tracing::warn!(
                        peer = %peer, error = %e,
                        "warm-up connection failed"
                    );
                }
            });
        }
    }

    /// Remove a peer (called from SWIM on leave/fail).
    pub fn remove_peer(&self, peer: &NodeId) {
        self.channels.remove(peer);
        self.peer_addrs.remove(peer);
        tracing::debug!(peer = %peer, "peer channel removed");
    }

    /// Drain all channels for graceful shutdown.
    pub async fn drain(&self, timeout: Duration) {
        for mut entry in self.channels.iter_mut() {
            entry.value_mut().state = PeerChannelState::Draining;
        }
        // Wait for in-flight RPCs to complete (or timeout)
        tokio::time::sleep(timeout).await;
        self.channels.clear();
        self.cancel.cancel();
    }
}
```

### 3.2 Background Tasks

```rust
impl ChannelPool {
    fn spawn_background_tasks(&self) {
        self.spawn_health_checker();
        self.spawn_idle_reaper();
    }

    fn spawn_health_checker(&self) {
        let channels = self.channels.clone();
        let config = self.config.clone();
        let cancel = self.cancel.clone();
        let pool = self.clone_inner();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.health_check_interval);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = interval.tick() => {}
                }

                let peers: Vec<NodeId> = channels.iter()
                    .map(|e| *e.key())
                    .collect();

                for peer in peers {
                    if let Some(mut entry) = channels.get_mut(&peer) {
                        let pc = entry.value_mut();
                        if pc.state == PeerChannelState::Draining {
                            continue;
                        }

                        let healthy = pool.check_health(&pc.channels[0], &config).await;

                        if healthy {
                            pc.health = HealthState::Healthy;
                            pc.last_health_check = Instant::now();
                        } else {
                            let failures = match &pc.health {
                                HealthState::Degraded { consecutive_failures } => {
                                    consecutive_failures + 1
                                }
                                _ => 1,
                            };

                            if failures >= config.max_health_failures {
                                pc.health = HealthState::Unhealthy;
                                pc.state = PeerChannelState::Reconnecting {
                                    attempt: 0,
                                    next_retry: Instant::now(),
                                };
                                tracing::warn!(
                                    peer = %pc.node_id,
                                    "peer marked unhealthy, reconnecting"
                                );
                            } else {
                                pc.health = HealthState::Degraded {
                                    consecutive_failures: failures,
                                };
                            }
                        }
                    }
                }

                // Attempt reconnections
                pool.attempt_reconnections().await;
            }
        });
    }

    async fn check_health(&self, channel: &Channel, config: &ChannelPoolConfig) -> bool {
        let mut client = DerivaInternalClient::new(channel.clone());
        let result = tokio::time::timeout(
            config.health_check_timeout,
            client.health_check(HealthCheckRequest {}),
        ).await;

        matches!(result, Ok(Ok(_)))
    }

    async fn attempt_reconnections(&self) {
        let reconnect_peers: Vec<(NodeId, u32)> = self.channels.iter()
            .filter_map(|entry| {
                let pc = entry.value();
                if let PeerChannelState::Reconnecting { attempt, next_retry } = &pc.state {
                    if Instant::now() >= *next_retry {
                        Some((*entry.key(), *attempt))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        for (peer, attempt) in reconnect_peers {
            match self.ensure_channel(&peer).await {
                Ok(_) => {
                    tracing::info!(peer = %peer, attempt, "reconnected to peer");
                }
                Err(e) => {
                    let next_delay = self.backoff_delay(attempt);
                    if let Some(mut entry) = self.channels.get_mut(&peer) {
                        entry.value_mut().state = PeerChannelState::Reconnecting {
                            attempt: attempt + 1,
                            next_retry: Instant::now() + next_delay,
                        };
                    }
                    tracing::debug!(
                        peer = %peer, attempt, delay_ms = next_delay.as_millis(),
                        error = %e, "reconnect failed, backing off"
                    );
                }
            }
        }
    }

    fn backoff_delay(&self, attempt: u32) -> Duration {
        let base = self.config.reconnect_base_delay.as_millis() as u64;
        let delay = base.saturating_mul(1u64 << attempt.min(20));
        let capped = delay.min(self.config.reconnect_max_delay.as_millis() as u64);
        // Add ±20% jitter
        let jitter = (capped as f64 * 0.2 * (rand::random::<f64>() * 2.0 - 1.0)) as i64;
        Duration::from_millis((capped as i64 + jitter).max(1) as u64)
    }

    fn spawn_idle_reaper(&self) {
        let channels = self.channels.clone();
        let idle_timeout = self.config.idle_timeout;
        let reaper_interval = self.config.reaper_interval;
        let cancel = self.cancel.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(reaper_interval);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = interval.tick() => {}
                }

                let now = Instant::now();
                let idle_peers: Vec<NodeId> = channels.iter()
                    .filter(|e| {
                        let pc = e.value();
                        pc.state == PeerChannelState::Connected
                            && now.duration_since(pc.last_used) > idle_timeout
                    })
                    .map(|e| *e.key())
                    .collect();

                for peer in idle_peers {
                    channels.remove(&peer);
                    tracing::debug!(peer = %peer, "idle channel reaped");
                }
            }
        });
    }
}
```

### 3.3 SWIM Integration

```rust
/// Hook ChannelPool into SWIM membership events.
impl ChannelPool {
    pub fn subscribe_swim(&self, swim: &Swim) {
        let pool = self.clone_inner();
        swim.on_member_join(move |node_id, addr| {
            pool.register_peer(node_id, addr);
        });

        let pool = self.clone_inner();
        swim.on_member_leave(move |node_id| {
            pool.remove_peer(&node_id);
        });

        let pool = self.clone_inner();
        swim.on_member_fail(move |node_id| {
            if let Some(mut entry) = pool.channels.get_mut(&node_id) {
                entry.value_mut().state = PeerChannelState::Reconnecting {
                    attempt: 0,
                    next_retry: Instant::now(),
                };
                entry.value_mut().health = HealthState::Unhealthy;
            }
        });
    }
}
```

### 3.4 Convenience Client Factory

```rust
/// Create a typed gRPC client for a peer, using the pool.
impl ChannelPool {
    pub async fn internal_client(
        &self,
        peer: &NodeId,
    ) -> Result<DerivaInternalClient<Channel>, DerivaError> {
        let channel = self.get_channel(peer).await?;
        Ok(DerivaInternalClient::new(channel))
    }

    /// Get channels to multiple peers (for fan-out RPCs).
    pub async fn get_channels_for(
        &self,
        peers: &[NodeId],
    ) -> Vec<(NodeId, Result<Channel, DerivaError>)> {
        let mut results = Vec::with_capacity(peers.len());
        for peer in peers {
            results.push((*peer, self.get_channel(peer).await));
        }
        results
    }
}
```


---

## 4. Data Flow Diagrams

### 4.1 Channel Lifecycle — Happy Path

```
  SWIM: Node B joined at 10.0.0.2:9000
    │
    ▼
  ChannelPool.register_peer(B, 10.0.0.2:9000)
    │
    ├── warm_on_join=true
    │   └── spawn: ensure_channel(B)
    │       ├── build_endpoint("https://10.0.0.2:9000")
    │       ├── TLS handshake (~1.5ms)
    │       ├── HTTP/2 connection established
    │       └── PeerChannel { state: Connected, health: Healthy }
    │
    ▼
  RPC: FetchValue to Node B
    │
    ├── get_channel(B) → fast path: existing Connected channel
    ├── last_used = now
    └── return channel → caller makes RPC

  ... 5 minutes of no RPCs to B ...

  Idle reaper:
    │
    ├── last_used was 5 min ago > idle_timeout
    └── remove channel for B
        (peer_addrs kept — reconnect on next RPC)

  Next RPC to B:
    │
    ├── get_channel(B) → no entry → ensure_channel(B)
    ├── reconnect (new TLS handshake)
    └── return fresh channel
```

### 4.2 Peer Failure and Reconnection

```
  Time ──────────────────────────────────────────────────────►

  t=0:   Channel to B: Connected, Healthy
  t=10s: Health check → ping B → timeout → Degraded(1)
  t=20s: Health check → ping B → timeout → Degraded(2)
  t=30s: Health check → ping B → timeout → Degraded(3) ≥ max(3)
         → Unhealthy, state=Reconnecting(attempt=0)

  t=30s: Reconnect attempt 0 → connect to B → FAIL
         next_retry = now + 100ms (±20% jitter)

  t=30.1s: Reconnect attempt 1 → connect to B → FAIL
           next_retry = now + 200ms

  t=30.3s: Reconnect attempt 2 → connect to B → FAIL
           next_retry = now + 400ms

  ... B restarts at t=35s ...

  t=35.7s: Reconnect attempt 5 → connect to B → SUCCESS!
           state=Connected, health=Healthy
           Log: "reconnected to peer B (attempt 5)"

  Meanwhile, RPCs to B between t=30s and t=35.7s:
    get_channel(B) → state=Reconnecting → ensure_channel(B)
    → may succeed (if B is back) or fail (DerivaError::Network)
    → caller handles error (retry, route elsewhere)
```

### 4.3 Multi-Channel Round-Robin

```
  Config: channels_per_peer = 3

  PeerChannel for Node B:
    channels = [ch_0, ch_1, ch_2]
    rr_index = AtomicUsize(0)

  RPC 1: get_channel(B) → rr_index=0 → ch_0, rr_index=1
  RPC 2: get_channel(B) → rr_index=1 → ch_1, rr_index=2
  RPC 3: get_channel(B) → rr_index=2 → ch_2, rr_index=3
  RPC 4: get_channel(B) → rr_index=3 % 3 = 0 → ch_0, rr_index=4
  ...

  Each channel handles up to 100 concurrent HTTP/2 streams.
  3 channels → 300 concurrent RPCs to one peer.
```

### 4.4 Warm-Up on Cluster Bootstrap

```
  Node A starts, discovers peers via SWIM:

  t=0:   SWIM: peer B alive at 10.0.0.2:9000
         → register_peer(B) → spawn warm-up → connect to B
  t=0:   SWIM: peer C alive at 10.0.0.3:9000
         → register_peer(C) → spawn warm-up → connect to C
  t=0:   SWIM: peer D alive at 10.0.0.4:9000
         → register_peer(D) → spawn warm-up → connect to D

  t=1.5ms: All 3 warm-up connections complete (parallel)

  t=2ms: First client request arrives
         → get_channel(B) → already connected → 0ms overhead
         → RPC proceeds immediately

  Without warm-up:
  t=2ms: First client request → get_channel(B) → connect → 1.5ms
         → total latency = request_time + 1.5ms (cold start penalty)
```

### 4.5 Graceful Shutdown with Drain

```
  SIGTERM received → shutdown sequence starts

  1. ChannelPool.drain(timeout=10s)
     │
     ├── Set all PeerChannels to state=Draining
     │   (health checker skips draining channels)
     │
     ├── In-flight RPCs continue on existing channels
     │   (tonic channels are cloned Arcs — still valid)
     │
     ├── New get_channel() calls:
     │   state=Draining → return existing channel
     │   (allow final RPCs to complete)
     │
     ├── Wait 10 seconds
     │
     └── channels.clear() → all channels dropped
         → TCP connections closed (FIN sent)
         → cancel token → background tasks stop

  2. Server stops accepting new connections
  3. Process exits
```

### 4.6 Connection Storm Prevention

```
  Without warm-up staggering (16-node cluster):
    t=0: All 15 peers discovered simultaneously
    → 15 concurrent connect() calls
    → 15 TLS handshakes in parallel
    → CPU spike, possible timeout

  With staggered warm-up:
    t=0:    connect to peer 1
    t=50ms: connect to peer 2
    t=100ms: connect to peer 3
    ...
    t=750ms: connect to peer 15

    Total: 750ms to warm all peers, no CPU spike.

  Implementation: warm-up tasks use a semaphore(4)
  to limit concurrent connection attempts.
```

---

## 5. Test Specification

### 5.1 Channel Pool Core Tests

```rust
#[cfg(test)]
mod pool_tests {
    use super::*;

    async fn setup_pool_with_mock_peers(n: usize) -> (ChannelPool, Vec<NodeId>) {
        let config = ChannelPoolConfig {
            warm_on_join: false, // manual control in tests
            health_check_interval: Duration::from_secs(3600), // disable
            ..Default::default()
        };
        let pool = ChannelPool::new(config, None);
        let mut peers = Vec::new();
        for i in 0..n {
            let id = NodeId::from(format!("node_{}", i));
            let addr: SocketAddr = format!("127.0.0.1:{}", 10000 + i).parse().unwrap();
            pool.peer_addrs.insert(id, addr);
            peers.push(id);
        }
        (pool, peers)
    }

    #[tokio::test]
    async fn test_get_channel_creates_on_first_use() {
        let server = start_mock_grpc_server().await;
        let pool = ChannelPool::new(ChannelPoolConfig::default(), None);
        let peer = NodeId::from("peer_1");
        pool.peer_addrs.insert(peer, server.addr());

        let channel = pool.get_channel(&peer).await;
        assert!(channel.is_ok());
        assert!(pool.channels.contains_key(&peer));
    }

    #[tokio::test]
    async fn test_get_channel_reuses_existing() {
        let server = start_mock_grpc_server().await;
        let pool = ChannelPool::new(ChannelPoolConfig::default(), None);
        let peer = NodeId::from("peer_1");
        pool.peer_addrs.insert(peer, server.addr());

        let ch1 = pool.get_channel(&peer).await.unwrap();
        let ch2 = pool.get_channel(&peer).await.unwrap();
        // Same channel object (cloned Arc internally)
        assert_eq!(pool.channels.len(), 1);
    }

    #[tokio::test]
    async fn test_unknown_peer_returns_error() {
        let pool = ChannelPool::new(ChannelPoolConfig::default(), None);
        let peer = NodeId::from("unknown");
        let result = pool.get_channel(&peer).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_remove_peer_clears_channel() {
        let server = start_mock_grpc_server().await;
        let pool = ChannelPool::new(ChannelPoolConfig::default(), None);
        let peer = NodeId::from("peer_1");
        pool.peer_addrs.insert(peer, server.addr());

        pool.get_channel(&peer).await.unwrap();
        assert!(pool.channels.contains_key(&peer));

        pool.remove_peer(&peer);
        assert!(!pool.channels.contains_key(&peer));
    }
}
```

### 5.2 Idle Reaper Tests

```rust
#[tokio::test]
async fn test_idle_channel_reaped() {
    let server = start_mock_grpc_server().await;
    let pool = ChannelPool::new(ChannelPoolConfig {
        idle_timeout: Duration::from_millis(200),
        reaper_interval: Duration::from_millis(100),
        warm_on_join: false,
        health_check_interval: Duration::from_secs(3600),
        ..Default::default()
    }, None);

    let peer = NodeId::from("peer_1");
    pool.peer_addrs.insert(peer, server.addr());
    pool.get_channel(&peer).await.unwrap();

    assert!(pool.channels.contains_key(&peer));

    // Wait for idle timeout + reaper cycle
    tokio::time::sleep(Duration::from_millis(400)).await;

    assert!(!pool.channels.contains_key(&peer));
}

#[tokio::test]
async fn test_active_channel_not_reaped() {
    let server = start_mock_grpc_server().await;
    let pool = ChannelPool::new(ChannelPoolConfig {
        idle_timeout: Duration::from_millis(300),
        reaper_interval: Duration::from_millis(100),
        warm_on_join: false,
        health_check_interval: Duration::from_secs(3600),
        ..Default::default()
    }, None);

    let peer = NodeId::from("peer_1");
    pool.peer_addrs.insert(peer, server.addr());
    pool.get_channel(&peer).await.unwrap();

    // Keep using it
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        pool.get_channel(&peer).await.unwrap(); // refreshes last_used
    }

    // Should still be there
    assert!(pool.channels.contains_key(&peer));
}
```

### 5.3 Backoff Tests

```rust
#[test]
fn test_backoff_exponential() {
    let config = ChannelPoolConfig {
        reconnect_base_delay: Duration::from_millis(100),
        reconnect_max_delay: Duration::from_secs(30),
        ..Default::default()
    };
    let pool = ChannelPool::new(config, None);

    // Without jitter, delays should be approximately:
    // attempt 0: 100ms, 1: 200ms, 2: 400ms, 3: 800ms
    let d0 = pool.backoff_delay(0);
    let d1 = pool.backoff_delay(1);
    let d2 = pool.backoff_delay(2);

    // With ±20% jitter, check ranges
    assert!(d0 >= Duration::from_millis(80) && d0 <= Duration::from_millis(120));
    assert!(d1 >= Duration::from_millis(160) && d1 <= Duration::from_millis(240));
    assert!(d2 >= Duration::from_millis(320) && d2 <= Duration::from_millis(480));
}

#[test]
fn test_backoff_caps_at_max() {
    let config = ChannelPoolConfig {
        reconnect_base_delay: Duration::from_millis(100),
        reconnect_max_delay: Duration::from_secs(30),
        ..Default::default()
    };
    let pool = ChannelPool::new(config, None);

    let d20 = pool.backoff_delay(20);
    // Should be capped at 30s (±20% jitter)
    assert!(d20 <= Duration::from_secs(36));
    assert!(d20 >= Duration::from_secs(24));
}
```

### 5.4 Health Check Tests

```rust
#[tokio::test]
async fn test_healthy_peer_stays_healthy() {
    let server = start_mock_grpc_server().await; // responds to health checks
    let pool = ChannelPool::new(ChannelPoolConfig {
        health_check_interval: Duration::from_millis(100),
        max_health_failures: 3,
        warm_on_join: false,
        ..Default::default()
    }, None);

    let peer = NodeId::from("peer_1");
    pool.peer_addrs.insert(peer, server.addr());
    pool.get_channel(&peer).await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let entry = pool.channels.get(&peer).unwrap();
    assert_eq!(entry.value().health, HealthState::Healthy);
}

#[tokio::test]
async fn test_dead_peer_marked_unhealthy() {
    let server = start_mock_grpc_server().await;
    let addr = server.addr();
    let pool = ChannelPool::new(ChannelPoolConfig {
        health_check_interval: Duration::from_millis(50),
        health_check_timeout: Duration::from_millis(20),
        max_health_failures: 2,
        warm_on_join: false,
        ..Default::default()
    }, None);

    let peer = NodeId::from("peer_1");
    pool.peer_addrs.insert(peer, addr);
    pool.get_channel(&peer).await.unwrap();

    // Kill server
    drop(server);

    // Wait for health checks to detect failure
    tokio::time::sleep(Duration::from_millis(300)).await;

    let entry = pool.channels.get(&peer).unwrap();
    assert_eq!(entry.value().health, HealthState::Unhealthy);
}
```

### 5.5 Multi-Channel Tests

```rust
#[tokio::test]
async fn test_multi_channel_round_robin() {
    let server = start_mock_grpc_server().await;
    let pool = ChannelPool::new(ChannelPoolConfig {
        channels_per_peer: 3,
        warm_on_join: false,
        health_check_interval: Duration::from_secs(3600),
        ..Default::default()
    }, None);

    let peer = NodeId::from("peer_1");
    pool.peer_addrs.insert(peer, server.addr());
    pool.ensure_channel(&peer).await.unwrap();

    let entry = pool.channels.get(&peer).unwrap();
    assert_eq!(entry.value().channels.len(), 3);

    // Verify round-robin index advances
    let _ = pool.get_channel(&peer).await.unwrap(); // idx 0
    let _ = pool.get_channel(&peer).await.unwrap(); // idx 1
    let _ = pool.get_channel(&peer).await.unwrap(); // idx 2
    let _ = pool.get_channel(&peer).await.unwrap(); // idx 0 again

    let entry = pool.channels.get(&peer).unwrap();
    assert_eq!(entry.value().rr_index.load(Ordering::Relaxed), 4);
}
```

### 5.6 Integration Tests

```rust
#[tokio::test]
async fn test_pool_with_real_cluster() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    let pool = &nodes[0].channel_pool;

    // Should have channels to both peers
    for i in 1..3 {
        let channel = pool.get_channel(&nodes[i].id()).await;
        assert!(channel.is_ok());
    }

    // Make actual RPCs through pooled channels
    let mut client = pool.internal_client(&nodes[1].id()).await.unwrap();
    let result = client.health_check(HealthCheckRequest {}).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_pool_reconnects_after_peer_restart() {
    let (nodes, mut handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    let pool = &nodes[0].channel_pool;
    let peer_id = nodes[1].id();

    // Verify connection works
    let ch = pool.get_channel(&peer_id).await.unwrap();
    assert!(DerivaInternalClient::new(ch).health_check(HealthCheckRequest {}).await.is_ok());

    // Kill and restart node 1
    let addr = nodes[1].addr();
    drop(handles.remove(1));
    tokio::time::sleep(Duration::from_secs(5)).await;

    let _new_node = start_node_at(addr).await;
    tokio::time::sleep(Duration::from_secs(10)).await;

    // Pool should have reconnected
    let ch = pool.get_channel(&peer_id).await.unwrap();
    let result = DerivaInternalClient::new(ch).health_check(HealthCheckRequest {}).await;
    assert!(result.is_ok());
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Peer address changes (new IP) | SWIM provides updated addr → remove old channel, register new | Dynamic cloud IPs |
| 2 | DNS resolution failure | connect() fails → backoff retry | Transient DNS |
| 3 | TLS cert mismatch on connect | connect() fails → backoff retry | Cert rotation in progress |
| 4 | Channel broken mid-RPC | Caller gets tonic::Status → retry via pool (gets new channel) | Pool detects on next health check |
| 5 | All channels to peer fail | HealthState::Unhealthy → RPCs return error immediately | Don't queue behind dead peer |
| 6 | Pool.get_channel during drain | Returns existing channel (allow final RPCs) | Graceful completion |
| 7 | Concurrent ensure_channel for same peer | DashMap insert is last-writer-wins | Extra channel created then dropped — harmless |
| 8 | channels_per_peer = 0 | Config validation rejects | Must be ≥ 1 |
| 9 | Peer rejoins with same NodeId | remove_peer + register_peer → fresh channel | Clean slate |
| 10 | HTTP/2 GOAWAY from peer | tonic detects, channel becomes unusable | Health check catches it, reconnect |
| 11 | System FD limit reached | connect() fails with "too many open files" | Backoff retry; alert operator |
| 12 | Clock jump (NTP correction) | Instant-based timers unaffected (monotonic) | Correct by design |

### 6.1 Concurrent ensure_channel Race

```
Problem: two RPCs to peer B arrive simultaneously, both find no channel.

  Thread 1: get_channel(B) → miss → ensure_channel(B)
  Thread 2: get_channel(B) → miss → ensure_channel(B)

  Both call create_channels() → two TCP connections created.
  Both call channels.insert(B, ...) → DashMap: second insert overwrites first.
  First channel is dropped (TCP connection closed).

  Result: one extra connection attempt. Harmless.
  The second channel is used going forward.

  Optimization (not implemented): use DashMap::entry() API
  with or_try_insert_with() to avoid the race. But the race
  is benign and rare, so simplicity wins.
```

### 6.2 Handling HTTP/2 Stream Exhaustion

```
  Config: max_concurrent_streams = 100
  Scenario: 150 concurrent RPCs to peer B with 1 channel.

  RPCs 1–100: multiplexed on the HTTP/2 connection → OK
  RPCs 101–150: HTTP/2 flow control queues them
    → tonic internally waits for a stream slot
    → adds latency but doesn't fail

  With channels_per_peer = 2:
    RPCs round-robin across 2 channels
    Each handles ~75 concurrent → well within 100 limit
    No queuing, lower latency

  Detection: monitor admission_in_flight per peer.
  If consistently > max_concurrent_streams × channels_per_peer,
  consider increasing channels_per_peer.
```

### 6.3 Warm-Up Failure Handling

```
  SWIM: peer D joined → warm-up spawn → connect fails

  Behavior:
    - Log warning: "warm-up connection failed"
    - Do NOT add to channels map (no broken entry)
    - Do NOT retry warm-up (next get_channel() will connect)

  Rationale: warm-up is best-effort optimization.
  If it fails, the first RPC to D pays the connection cost.
  No need for complex warm-up retry logic.
```

---

## 7. Performance Analysis

### 7.1 Connection Overhead

```
┌──────────────────────────────┬──────────┬──────────────────────┐
│ Operation                    │ Latency  │ Notes                │
├──────────────────────────────┼──────────┼──────────────────────┤
│ TCP connect (same AZ)        │ ~0.3ms   │ SYN/SYN-ACK/ACK     │
│ TLS 1.3 handshake            │ ~1.5ms   │ 1-RTT with mTLS     │
│ HTTP/2 negotiation           │ ~0.1ms   │ ALPN in TLS          │
│ Total cold connect           │ ~2ms     │ First RPC to peer    │
├──────────────────────────────┼──────────┼──────────────────────┤
│ get_channel() fast path      │ ~100ns   │ DashMap lookup       │
│ RPC on warm channel          │ ~0.5ms   │ HTTP/2 stream setup  │
│ Health check ping            │ ~1ms     │ Unary RPC            │
├──────────────────────────────┼──────────┼──────────────────────┤
│ Amortized per-RPC overhead   │ ~100ns   │ Pool lookup only     │
└──────────────────────────────┴──────────┴──────────────────────┘
```

### 7.2 Resource Usage

```
Per peer channel:
  - 1 TCP connection: 1 FD, ~20KB kernel buffer
  - 1 TLS session: ~20KB rustls state
  - 1 HTTP/2 connection: ~5KB nghttp2 state
  - PeerChannel struct: ~200 bytes
  Total: ~45KB per peer

16-node cluster (15 peers):
  - 15 × 45KB = 675KB
  - 15 FDs

With channels_per_peer=3:
  - 45 × 45KB = 2MB
  - 45 FDs

  Well within typical limits (ulimit -n = 65536 on servers).
```

### 7.3 Health Check Overhead

```
  15 peers × 1 health check / 10s = 1.5 RPCs/sec
  Each health check: ~1ms CPU, ~200 bytes on wire
  Total: 1.5ms CPU/sec, 300 bytes/sec

  Negligible. Even at 100 peers: 10 RPCs/sec = 10ms CPU/sec.
```

### 7.4 Benchmarking Plan

```rust
/// Benchmark: get_channel fast path (existing channel)
#[bench]
fn bench_get_channel_fast_path(b: &mut Bencher) {
    // Pre-populated pool, measure DashMap lookup
    // Expected: <200ns
}

/// Benchmark: get_channel cold path (new connection)
#[bench]
fn bench_get_channel_cold(b: &mut Bencher) {
    // Connect to loopback mock server
    // Expected: <3ms
}

/// Benchmark: round-robin channel selection
#[bench]
fn bench_round_robin_selection(b: &mut Bencher) {
    // 3 channels, measure atomic fetch_add + modulo
    // Expected: <50ns
}

/// Benchmark: health check cycle (15 peers)
#[bench]
fn bench_health_check_cycle(b: &mut Bencher) {
    // 15 mock peers, measure full health check round
    // Expected: <50ms (parallel pings)
}

/// Benchmark: backoff_delay computation
#[bench]
fn bench_backoff_delay(b: &mut Bencher) {
    // Compute delay for attempt 0–20
    // Expected: <100ns per call
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/channel_pool.rs` | **NEW** — ChannelPool, PeerChannel, ChannelPoolConfig |
| `deriva-network/src/lib.rs` | Add `pub mod channel_pool` |
| `deriva-network/src/swim.rs` | Add `on_member_join`, `on_member_leave`, `on_member_fail` callbacks |
| `deriva-server/src/state.rs` | Add `channel_pool: Arc<ChannelPool>` |
| `deriva-server/src/main.rs` | Initialize ChannelPool, subscribe to SWIM events |
| `deriva-network/src/distributed_get.rs` | Use `channel_pool.get_channel()` instead of ad-hoc connects |
| `deriva-network/src/replication.rs` | Use `channel_pool.get_channel()` |
| `deriva-network/src/cluster_gc.rs` | Use `channel_pool.get_channel()` |
| `deriva-network/src/migration.rs` | Use `channel_pool.get_channel()` |
| `deriva-network/tests/channel_pool.rs` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

| Crate | Version | Purpose |
|-------|---------|---------|
| `rand` | 0.8.x | Jitter for backoff (already in workspace) |

No new external dependencies.

---

## 10. Design Rationale

### 10.1 Why Lazy + Warm-Up Instead of Eager Connect-All?

```
Eager connect-all:
  On startup, connect to every known peer immediately.
  + All channels ready before first RPC
  - Slow startup if many peers (16 peers × 2ms = 32ms)
  - Wasted connections to peers we never talk to
  - Connection storm on cluster-wide restart

Lazy (connect on first use):
  + Fast startup
  + Only connect to peers we actually need
  - First RPC to each peer pays connection cost

Lazy + warm-up (our approach):
  + SWIM join event triggers background connect
  + By the time first RPC arrives, channel is usually ready
  + No connection storm (staggered by SWIM discovery timing)
  + Unused peers never get connections
  Best of both worlds.
```

### 10.2 Why DashMap Instead of RwLock<HashMap>?

```
RwLock<HashMap>:
  - Write lock during insert blocks all readers
  - Health checker holds read lock → blocks new connections
  - Contention under high RPC rates

DashMap:
  - Sharded concurrent map (16 shards by default)
  - Readers and writers to different shards don't block
  - get_mut() locks only one shard
  - Perfect for "many readers, occasional writers" pattern

  ChannelPool access pattern:
    get_channel(): ~10,000/sec (hot path, read-mostly)
    register_peer(): ~1/min (rare, write)
    remove_peer(): ~1/min (rare, write)

  DashMap is ideal for this access pattern.
```

### 10.3 Why HTTP/2 Keep-Alive Instead of Application-Level Pings?

```
Application-level pings:
  - Custom HealthCheck RPC every 10s
  - Detects application-level issues (server overloaded, handler panic)
  - Higher overhead (full RPC round-trip)

HTTP/2 keep-alive (PING frames):
  - Built into HTTP/2 protocol
  - Detects transport-level issues (TCP broken, firewall timeout)
  - Very low overhead (8-byte frame)
  - Keeps NAT/firewall mappings alive

We use BOTH:
  - HTTP/2 keep-alive (30s): prevents idle connection closure by firewalls
  - Application health check (10s): detects server-level issues

  The health check is more aggressive (10s) because we need to detect
  application failures quickly for routing decisions.
  HTTP/2 keep-alive is less aggressive (30s) because it's just for
  transport liveness.
```

### 10.4 Why Single Channel Per Peer by Default?

```
HTTP/2 multiplexes up to 100+ concurrent streams on one TCP connection.
For most Deriva workloads (reads + writes, not bulk transfer),
one channel handles the load easily.

Multiple channels help when:
  - Concurrent RPCs > 100 (HTTP/2 stream limit)
  - Large streaming RPCs (MigrateTransfer) block the connection
  - TCP-level head-of-line blocking on lossy networks

Default: 1 channel (simple, sufficient for most deployments).
Configurable: up to 4 for high-throughput scenarios.
Operator can tune based on observed HTTP/2 stream utilization.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `pool_channels_active` | Gauge | — | Total active channels across all peers |
| `pool_peers_connected` | Gauge | — | Peers with at least one healthy channel |
| `pool_peers_unhealthy` | Gauge | — | Peers in Unhealthy state |
| `pool_connects_total` | Counter | `result={success,failed}` | Connection attempts |
| `pool_connect_duration_ms` | Histogram | — | Time to establish new channel |
| `pool_reconnects_total` | Counter | — | Reconnection attempts |
| `pool_idle_reaped` | Counter | — | Channels closed due to idle timeout |
| `pool_health_checks_total` | Counter | `result={healthy,failed}` | Health check outcomes |
| `pool_get_channel_duration_ns` | Histogram | `path={fast,slow}` | get_channel() latency |
| `pool_channels_per_peer` | Gauge | `peer` | Channels to each peer (top-10) |

### 11.2 Structured Logging

```rust
tracing::info!(
    peer = %peer, addr = %addr,
    "channel established"
);

tracing::warn!(
    peer = %peer, attempt = attempt,
    delay_ms = next_delay.as_millis(),
    error = %e,
    "reconnect failed, backing off"
);

tracing::info!(
    peer = %peer, attempt = attempt,
    "reconnected to peer"
);

tracing::debug!(
    peer = %peer,
    idle_secs = idle_duration.as_secs(),
    "idle channel reaped"
);

tracing::warn!(
    peer = %peer,
    consecutive_failures = failures,
    "peer marked unhealthy"
);

tracing::info!(
    peers = pool.channels.len(),
    total_channels = total,
    "channel pool status"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| Peer unreachable | `pool_peers_unhealthy` > 0 for 5 min | Warning |
| Many peers unreachable | `pool_peers_unhealthy` > 30% of cluster | Critical |
| Connection failures | `pool_connects_total{result=failed}` > 10/min | Warning |
| Slow connections | `pool_connect_duration_ms` p99 > 5s | Warning |
| Pool empty | `pool_peers_connected` = 0 (multi-node cluster) | Critical |
| High reconnect rate | `pool_reconnects_total` > 50/min | Warning |

---

## 12. Checklist

- [ ] Create `deriva-network/src/channel_pool.rs`
- [ ] Implement `ChannelPool` with DashMap-based storage
- [ ] Implement `get_channel()` fast path (DashMap lookup + round-robin)
- [ ] Implement `ensure_channel()` slow path (connect + TLS)
- [ ] Implement `build_endpoint()` with HTTP/2 settings and TLS
- [ ] Implement `register_peer()` with optional warm-up
- [ ] Implement `remove_peer()` cleanup
- [ ] Implement health checker background task
- [ ] Implement `check_health()` via HealthCheck RPC
- [ ] Implement `attempt_reconnections()` with backoff
- [ ] Implement `backoff_delay()` with exponential + jitter
- [ ] Implement idle reaper background task
- [ ] Implement `drain()` for graceful shutdown
- [ ] Implement SWIM event subscription (`subscribe_swim`)
- [ ] Implement `internal_client()` convenience factory
- [ ] Implement multi-channel round-robin selection
- [ ] Refactor distributed_get to use ChannelPool
- [ ] Refactor replication to use ChannelPool
- [ ] Refactor cluster_gc to use ChannelPool
- [ ] Refactor migration to use ChannelPool
- [ ] Write pool core tests (4 tests)
- [ ] Write idle reaper tests (2 tests)
- [ ] Write backoff tests (2 tests)
- [ ] Write health check tests (2 tests)
- [ ] Write multi-channel tests (1 test)
- [ ] Write integration tests (2 tests)
- [ ] Add metrics (10 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (6 alerts)
- [ ] Run benchmarks: fast path, cold path, round-robin, health cycle, backoff
- [ ] Config validation: channels_per_peer ≥ 1, idle_timeout > health_check_interval
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
