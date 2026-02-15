# §3.16 Request Hedging & Speculative Execution

> **Status:** Not started
> **Depends on:** §3.5 Locality-Aware Routing, §3.6 Distributed Get, §3.13 Connection Pooling, §3.11 Backpressure
> **Crate(s):** `deriva-network`, `deriva-server`
> **Estimated effort:** 2.5 days

---

## 1. Problem Statement

In a distributed system, tail latency is the enemy. Even when median latency is 2ms, the p99 can spike to 200ms due to:

1. **GC pauses**: A node running garbage collection stalls for 50–200ms.
2. **Disk I/O contention**: A node serving a large sequential read blocks other reads.
3. **Network jitter**: A single packet retransmit adds 200ms+ (TCP RTO).
4. **CPU scheduling**: A node under load delays processing by tens of milliseconds.
5. **Cold cache**: One replica has the data cached, another must read from disk.

The standard approach — send request, wait for response, retry on timeout — guarantees that every slow node directly impacts client-visible latency. With replication factor ≥ 2, the same data exists on multiple nodes. We can exploit this redundancy.

### The Hedging Strategy

Instead of waiting for a slow response, send a speculative second request to another replica after a short delay. Use whichever response arrives first, cancel the other.

```
  Without hedging:          With hedging (delay=5ms):
  ┌──────────────────┐      ┌──────────────────┐
  │ Request → Node A │      │ Request → Node A  │
  │ ... 150ms (slow) │      │ ... 5ms (no reply)│
  │ Response ← A     │      │ Hedge → Node B    │
  │                  │      │ ... 3ms            │
  │ Total: 150ms     │      │ Response ← B      │
  └──────────────────┘      │ Cancel A           │
                            │ Total: 8ms         │
                            └──────────────────┘
```

### Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| H1 | Hedge read requests (GetValue, FetchValue) after configurable delay | P0 |
| H2 | Cancel losing request to avoid wasted work | P0 |
| H3 | Adaptive hedge delay based on observed latency (p50/p90) | P1 |
| H4 | Budget-based hedging: limit hedge rate to N% of total requests | P0 |
| H5 | Exclude unhealthy/overloaded nodes from hedge targets | P1 |
| H6 | Metrics: hedge rate, hedge win rate, latency improvement | P0 |
| H7 | Disable hedging under backpressure (don't amplify overload) | P0 |
| H8 | Read-only: never hedge writes (non-idempotent side effects) | P0 |

### Non-Goals

- Hedging write operations (PutLeaf, StoreRecipe)
- Hedging across data centers (cross-region latency makes hedging counterproductive)
- Request cloning (sending to all replicas simultaneously — too expensive)

---

## 2. Design

### 2.1 Architecture

```
  ┌──────────────────────────────────────────────────────┐
  │                   HedgedRequest                       │
  │                                                       │
  │  ┌─────────────┐    ┌──────────────┐   ┌───────────┐ │
  │  │ LatencyTracker│   │ HedgeBudget  │   │ NodeFilter│ │
  │  │ (per-peer    │   │ (token bucket│   │ (exclude  │ │
  │  │  EWMA p50/p90│   │  10% default)│   │  suspect/ │ │
  │  │  histograms) │   │              │   │  draining)│ │
  │  └──────┬───────┘   └──────┬───────┘   └─────┬─────┘ │
  │         │                  │                  │       │
  │  ┌──────▼──────────────────▼──────────────────▼─────┐ │
  │  │              HedgeExecutor                        │ │
  │  │                                                   │ │
  │  │  1. Send primary request to best replica          │ │
  │  │  2. Start hedge timer (adaptive delay)            │ │
  │  │  3. If timer fires before response:               │ │
  │  │     a. Check budget (token available?)            │ │
  │  │     b. Pick next-best replica                     │ │
  │  │     c. Send hedge request                         │ │
  │  │  4. Return first successful response              │ │
  │  │  5. Cancel the other via CancellationToken        │ │
  │  └──────────────────────────────────────────────────┘ │
  └──────────────────────────────────────────────────────┘

  Integration point: wraps the existing FetchValue RPC path
  in distributed_get (§3.6). No changes to the RPC protocol.
```

### 2.2 Adaptive Latency Tracking

```
  Per-peer latency tracker using exponentially weighted moving average:

  ┌─────────────────────────────────────────────┐
  │ LatencyTracker                              │
  │                                             │
  │  peer_stats: DashMap<NodeId, PeerLatency>   │
  │                                             │
  │  PeerLatency {                              │
  │    ewma_ms: f64,       // smoothed average  │
  │    variance: f64,      // smoothed variance  │
  │    sample_count: u64,                       │
  │  }                                          │
  │                                             │
  │  Hedge delay = ewma + 2 * sqrt(variance)   │
  │  (≈ p95 of observed latency)               │
  │                                             │
  │  Alpha = 0.1 (smooth, slow to react)       │
  │  Min delay = 1ms (floor)                   │
  │  Max delay = 50ms (cap)                    │
  │  Default (no data) = 5ms                   │
  └─────────────────────────────────────────────┘

  Why EWMA instead of histogram?
    - O(1) memory per peer (2 floats vs thousands of buckets)
    - O(1) update (vs histogram insertion + percentile scan)
    - Good enough: we need approximate p95, not exact percentiles
```

### 2.3 Hedge Budget

```
  Token-bucket rate limiter for hedge requests:

  ┌─────────────────────────────────────────────┐
  │ HedgeBudget                                 │
  │                                             │
  │  budget_pct: f64 = 0.10  (10% of requests) │
  │  tokens: AtomicU64                          │
  │  max_tokens: u64 = 100                      │
  │  refill_interval: Duration = 1s             │
  │                                             │
  │  Every second: tokens = min(max, total_reqs │
  │                              * budget_pct)  │
  │                                             │
  │  try_acquire() → bool:                      │
  │    tokens.fetch_sub(1) > 0                  │
  │                                             │
  │  If budget exhausted: skip hedging, wait    │
  │  for primary response normally.             │
  └─────────────────────────────────────────────┘

  Why 10% default?
    Hedging doubles load for hedged requests.
    10% budget → at most 10% extra load on cluster.
    Enough to cover tail latency (typically <5% of requests are slow).
```

### 2.4 Core Types

```rust
// deriva-network/src/hedge.rs

pub struct HedgeConfig {
    /// Enable hedging (default: true for reads)
    pub enabled: bool,
    /// Percentage of requests eligible for hedging (0.0–1.0)
    pub budget_pct: f64,
    /// Maximum hedge budget tokens
    pub max_budget_tokens: u64,
    /// Minimum hedge delay (floor)
    pub min_delay: Duration,
    /// Maximum hedge delay (cap)
    pub max_delay: Duration,
    /// Default delay when no latency data available
    pub default_delay: Duration,
    /// EWMA smoothing factor (0.0–1.0)
    pub alpha: f64,
}

impl Default for HedgeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            budget_pct: 0.10,
            max_budget_tokens: 100,
            min_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(50),
            default_delay: Duration::from_millis(5),
            alpha: 0.1,
        }
    }
}

pub struct HedgeExecutor {
    config: HedgeConfig,
    latency: LatencyTracker,
    budget: HedgeBudget,
    pool: Arc<ChannelPool>,
    admission: Arc<AdmissionController>,
}

struct LatencyTracker {
    peers: DashMap<NodeId, PeerLatency>,
    alpha: f64,
}

struct PeerLatency {
    ewma_ms: f64,
    variance: f64,
    sample_count: u64,
}

struct HedgeBudget {
    tokens: AtomicI64,
    max_tokens: u64,
    budget_pct: f64,
    request_count: AtomicU64,
}
```

### 2.5 Configuration

```toml
# deriva.toml
[hedge]
enabled = true
budget_pct = 0.10
max_budget_tokens = 100
min_delay_ms = 1
max_delay_ms = 50
default_delay_ms = 5
alpha = 0.1
```

---

## 3. Implementation

### 3.1 Latency Tracker

```rust
impl LatencyTracker {
    pub fn new(alpha: f64) -> Self {
        Self { peers: DashMap::new(), alpha }
    }

    pub fn record(&self, peer: NodeId, latency: Duration) {
        let ms = latency.as_secs_f64() * 1000.0;
        let mut entry = self.peers.entry(peer).or_insert(PeerLatency {
            ewma_ms: ms,
            variance: 0.0,
            sample_count: 0,
        });

        let diff = ms - entry.ewma_ms;
        entry.ewma_ms += self.alpha * diff;
        entry.variance = (1.0 - self.alpha) * (entry.variance + self.alpha * diff * diff);
        entry.sample_count += 1;
    }

    /// Returns adaptive hedge delay for a peer: ewma + 2*stddev, clamped.
    pub fn hedge_delay(&self, peer: NodeId, config: &HedgeConfig) -> Duration {
        match self.peers.get(&peer) {
            Some(stats) if stats.sample_count >= 10 => {
                let delay_ms = stats.ewma_ms + 2.0 * stats.variance.sqrt();
                let delay = Duration::from_secs_f64(delay_ms / 1000.0);
                delay.clamp(config.min_delay, config.max_delay)
            }
            _ => config.default_delay,
        }
    }
}
```

### 3.2 Hedge Budget

```rust
impl HedgeBudget {
    pub fn new(budget_pct: f64, max_tokens: u64) -> Self {
        Self {
            tokens: AtomicI64::new(max_tokens as i64),
            max_tokens,
            budget_pct,
            request_count: AtomicU64::new(0),
        }
    }

    pub fn record_request(&self) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn try_acquire(&self) -> bool {
        self.tokens.fetch_sub(1, Ordering::Relaxed) > 0
    }

    /// Called periodically (every 1s) to refill tokens.
    pub fn refill(&self) {
        let reqs = self.request_count.swap(0, Ordering::Relaxed);
        let new_tokens = ((reqs as f64) * self.budget_pct).ceil() as i64;
        let capped = new_tokens.min(self.max_tokens as i64);
        self.tokens.store(capped, Ordering::Relaxed);
    }
}
```

### 3.3 Hedge Executor

```rust
impl HedgeExecutor {
    pub fn new(
        config: HedgeConfig,
        pool: Arc<ChannelPool>,
        admission: Arc<AdmissionController>,
    ) -> Self {
        let latency = LatencyTracker::new(config.alpha);
        let budget = HedgeBudget::new(config.budget_pct, config.max_budget_tokens);
        Self { config, latency, budget, pool, admission }
    }

    /// Start background budget refill task.
    pub fn start_refill_task(&self) -> JoinHandle<()> {
        let budget = self.budget.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                budget.refill();
            }
        })
    }

    /// Execute a hedged read. Sends primary request, optionally hedges.
    pub async fn hedged_fetch(
        &self,
        addr: &CAddr,
        primary: NodeId,
        replicas: &[NodeId],
    ) -> Result<Value, DerivaError> {
        self.budget.record_request();

        if !self.config.enabled || replicas.is_empty() {
            return self.fetch_from(primary, addr).await;
        }

        // Pick best hedge target (first healthy replica that isn't primary)
        let hedge_target = replicas.iter()
            .filter(|n| **n != primary)
            .filter(|n| self.pool.is_healthy(**n))
            .next()
            .copied();

        let hedge_target = match hedge_target {
            Some(t) => t,
            None => return self.fetch_from(primary, addr).await,
        };

        let delay = self.latency.hedge_delay(primary, &self.config);
        let token = CancellationToken::new();

        // Primary request
        let primary_token = token.child_token();
        let primary_fut = {
            let pool = self.pool.clone();
            let addr = addr.clone();
            let tracker = &self.latency;
            async move {
                let start = Instant::now();
                let result = Self::fetch_with_cancel(&pool, primary, &addr, primary_token).await;
                if result.is_ok() {
                    tracker.record(primary, start.elapsed());
                }
                (primary, result)
            }
        };

        // Hedge request (delayed)
        let hedge_token = token.child_token();
        let can_hedge = !self.admission.is_overloaded();
        let hedge_fut = {
            let pool = self.pool.clone();
            let addr = addr.clone();
            let budget = &self.budget;
            let tracker = &self.latency;
            async move {
                tokio::time::sleep(delay).await;
                if !can_hedge || !budget.try_acquire() {
                    // Budget exhausted or system overloaded: don't hedge
                    return (hedge_target, Err(DerivaError::Cancelled));
                }
                metrics::counter!("hedge_requests_total").increment(1);
                let start = Instant::now();
                let result = Self::fetch_with_cancel(&pool, hedge_target, &addr, hedge_token).await;
                if result.is_ok() {
                    tracker.record(hedge_target, start.elapsed());
                }
                (hedge_target, result)
            }
        };

        // Race: first success wins
        tokio::pin!(primary_fut);
        tokio::pin!(hedge_fut);

        let (winner, result) = tokio::select! {
            (node, res) = &mut primary_fut => {
                token.cancel(); // cancel hedge
                if res.is_ok() {
                    metrics::counter!("hedge_primary_wins").increment(1);
                }
                (node, res)
            }
            (node, res) = &mut hedge_fut => {
                if res.is_ok() {
                    token.cancel(); // cancel primary
                    metrics::counter!("hedge_wins_total").increment(1);
                    (node, res)
                } else {
                    // Hedge failed/cancelled, wait for primary
                    let (node, res) = primary_fut.await;
                    token.cancel();
                    (node, res)
                }
            }
        };

        result
    }

    async fn fetch_with_cancel(
        pool: &ChannelPool,
        node: NodeId,
        addr: &CAddr,
        cancel: CancellationToken,
    ) -> Result<Value, DerivaError> {
        tokio::select! {
            result = Self::do_fetch(pool, node, addr) => result,
            _ = cancel.cancelled() => Err(DerivaError::Cancelled),
        }
    }

    async fn do_fetch(
        pool: &ChannelPool,
        node: NodeId,
        addr: &CAddr,
    ) -> Result<Value, DerivaError> {
        let mut client = pool.get_client(node).await?;
        let resp = client.fetch_value(FetchValueRequest {
            addr: addr.as_bytes().to_vec(),
        }).await.map_err(|e| DerivaError::Remote(e.to_string()))?;
        Value::from_proto(resp.into_inner())
    }

    async fn fetch_from(&self, node: NodeId, addr: &CAddr) -> Result<Value, DerivaError> {
        let start = Instant::now();
        let result = Self::do_fetch(&self.pool, node, addr).await;
        if result.is_ok() {
            self.latency.record(node, start.elapsed());
        }
        result
    }
}
```

### 3.4 Integration with Distributed Get

```rust
// deriva-network/src/distributed_get.rs (modified)

impl DistributedGet {
    pub async fn get(&self, addr: &CAddr) -> Result<Value, DerivaError> {
        // 1. Check local cache
        if let Some(val) = self.cache.get(addr) {
            return Ok(val);
        }

        // 2. Check local stores
        if let Some(val) = self.local_lookup(addr).await? {
            self.cache.insert(addr.clone(), val.clone());
            return Ok(val);
        }

        // 3. Remote fetch with hedging
        let primary = self.ring.primary_owner(addr);
        let replicas = self.ring.replica_owners(addr);

        let val = self.hedge_executor
            .hedged_fetch(addr, primary, &replicas)
            .await?;

        self.cache.insert(addr.clone(), val.clone());
        Ok(val)
    }
}
```


---

## 4. Data Flow Diagrams

### 4.1 Hedged Read — Hedge Wins

```
  Client              Coordinator           Node A (primary)    Node B (replica)
    │                      │                      │                   │
    │── GetValue(X) ──────►│                      │                   │
    │                      │                      │                   │
    │                      │── FetchValue(X) ────►│ (GC pause)        │
    │                      │                      │ ...stalled...     │
    │                      │                      │                   │
    │                      │  delay=5ms expires   │                   │
    │                      │  budget.try_acquire() → true             │
    │                      │                      │                   │
    │                      │── FetchValue(X) ─────┼──────────────────►│
    │                      │                      │                   │
    │                      │◄── Value(X) ─────────┼───────────────────┘ (3ms)
    │                      │                      │                   │
    │                      │  cancel(primary) ────►│ (cancelled)      │
    │                      │                      │                   │
    │◄── Value(X) ────────┘                      │                   │
    │                      │                      │                   │
    │  Total: 8ms (5ms delay + 3ms hedge)        │                   │
    │  Without hedging: 150ms (GC pause)         │                   │
```

### 4.2 Hedged Read — Primary Wins (No Hedge Sent)

```
  Client              Coordinator           Node A (primary)
    │                      │                      │
    │── GetValue(X) ──────►│                      │
    │                      │── FetchValue(X) ────►│
    │                      │                      │
    │                      │◄── Value(X) ─────────┘ (2ms, before delay)
    │                      │                      │
    │                      │  cancel hedge timer  │
    │                      │                      │
    │◄── Value(X) ────────┘                      │
    │                      │                      │
    │  Total: 2ms. Hedge never sent.             │
    │  Zero overhead for fast responses.         │
```

### 4.3 Budget Exhausted — No Hedge

```
  Client              Coordinator           Node A (slow)
    │                      │                      │
    │── GetValue(X) ──────►│                      │
    │                      │── FetchValue(X) ────►│ (slow)
    │                      │                      │
    │                      │  delay=5ms expires   │
    │                      │  budget.try_acquire() → false (exhausted)
    │                      │                      │
    │                      │  (no hedge sent)     │
    │                      │                      │
    │                      │◄── Value(X) ─────────┘ (80ms)
    │                      │                      │
    │◄── Value(X) ────────┘                      │
    │                      │                      │
    │  Total: 80ms. Budget prevented amplification.
    │  Budget refills next second.               │
```

### 4.4 Backpressure Suppresses Hedging

```
  Client              Coordinator           Node A
    │                      │                      │
    │── GetValue(X) ──────►│                      │
    │                      │                      │
    │                      │  admission.is_overloaded() → true
    │                      │  (memory pressure high)
    │                      │                      │
    │                      │── FetchValue(X) ────►│ (primary only)
    │                      │                      │
    │                      │  delay expires, but  │
    │                      │  can_hedge = false   │
    │                      │  → skip hedge        │
    │                      │                      │
    │                      │◄── Value(X) ─────────┘
    │◄── Value(X) ────────┘
    │
    │  Hedging disabled during overload to avoid amplifying pressure.
```

### 4.5 Adaptive Delay Convergence

```
  Peer A latency samples over time:

  Sample:  2ms  3ms  2ms  15ms  2ms  3ms  2ms  2ms  3ms  2ms
  EWMA:    2.0  2.1  2.1  3.4   3.3  3.2  3.1  3.0  3.0  2.9
  StdDev:  0    0.3  0.3  3.8   3.6  3.4  3.2  3.0  2.9  2.7

  Hedge delay = EWMA + 2*StdDev:
           2.0  2.7  2.7  11.0  10.5 10.0 9.5  9.0  8.8  8.3

  The 15ms outlier raises the delay temporarily.
  EWMA smoothing (alpha=0.1) means it decays slowly.
  After ~20 normal samples, delay returns to ~3ms.

  This prevents hedging on normal variance while catching true slowdowns.
```

---

## 5. Test Specification

### 5.1 LatencyTracker Tests

```rust
#[cfg(test)]
mod latency_tests {
    use super::*;

    #[test]
    fn test_tracker_default_delay_no_data() {
        let tracker = LatencyTracker::new(0.1);
        let config = HedgeConfig::default();
        let delay = tracker.hedge_delay(NodeId(1), &config);
        assert_eq!(delay, config.default_delay);
    }

    #[test]
    fn test_tracker_needs_minimum_samples() {
        let tracker = LatencyTracker::new(0.1);
        let config = HedgeConfig::default();

        // Record 5 samples (below threshold of 10)
        for _ in 0..5 {
            tracker.record(NodeId(1), Duration::from_millis(2));
        }

        // Still returns default (not enough data)
        assert_eq!(tracker.hedge_delay(NodeId(1), &config), config.default_delay);
    }

    #[test]
    fn test_tracker_adapts_after_enough_samples() {
        let tracker = LatencyTracker::new(0.1);
        let config = HedgeConfig::default();

        for _ in 0..20 {
            tracker.record(NodeId(1), Duration::from_millis(2));
        }

        let delay = tracker.hedge_delay(NodeId(1), &config);
        // With consistent 2ms, EWMA ≈ 2ms, variance ≈ 0
        // Delay ≈ 2ms, clamped to min_delay if below
        assert!(delay >= config.min_delay);
        assert!(delay <= Duration::from_millis(10));
    }

    #[test]
    fn test_tracker_outlier_raises_delay() {
        let tracker = LatencyTracker::new(0.1);
        let config = HedgeConfig::default();

        for _ in 0..15 {
            tracker.record(NodeId(1), Duration::from_millis(2));
        }
        let delay_before = tracker.hedge_delay(NodeId(1), &config);

        // Inject outlier
        tracker.record(NodeId(1), Duration::from_millis(100));
        let delay_after = tracker.hedge_delay(NodeId(1), &config);

        assert!(delay_after > delay_before);
    }

    #[test]
    fn test_tracker_delay_clamped_to_max() {
        let tracker = LatencyTracker::new(0.5); // high alpha = fast react
        let config = HedgeConfig { max_delay: Duration::from_millis(50), ..Default::default() };

        for _ in 0..20 {
            tracker.record(NodeId(1), Duration::from_millis(200));
        }

        let delay = tracker.hedge_delay(NodeId(1), &config);
        assert!(delay <= config.max_delay);
    }

    #[test]
    fn test_tracker_per_peer_isolation() {
        let tracker = LatencyTracker::new(0.1);
        let config = HedgeConfig::default();

        for _ in 0..20 {
            tracker.record(NodeId(1), Duration::from_millis(2));
            tracker.record(NodeId(2), Duration::from_millis(50));
        }

        let delay_1 = tracker.hedge_delay(NodeId(1), &config);
        let delay_2 = tracker.hedge_delay(NodeId(2), &config);
        assert!(delay_2 > delay_1);
    }
}
```

### 5.2 HedgeBudget Tests

```rust
#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn test_budget_acquire_within_limit() {
        let budget = HedgeBudget::new(0.10, 100);
        // Initial tokens = max_tokens = 100
        for _ in 0..100 {
            assert!(budget.try_acquire());
        }
        // 101st should fail
        assert!(!budget.try_acquire());
    }

    #[test]
    fn test_budget_refill() {
        let budget = HedgeBudget::new(0.10, 100);
        // Exhaust all tokens
        for _ in 0..100 { budget.try_acquire(); }
        assert!(!budget.try_acquire());

        // Simulate 1000 requests in the last second
        for _ in 0..1000 { budget.record_request(); }
        budget.refill();

        // 10% of 1000 = 100 tokens
        assert!(budget.try_acquire());
    }

    #[test]
    fn test_budget_refill_capped() {
        let budget = HedgeBudget::new(0.10, 50);
        for _ in 0..10000 { budget.record_request(); }
        budget.refill();

        // 10% of 10000 = 1000, but capped at max_tokens=50
        let mut acquired = 0;
        while budget.try_acquire() { acquired += 1; }
        assert_eq!(acquired, 50);
    }

    #[test]
    fn test_budget_zero_requests_zero_tokens() {
        let budget = HedgeBudget::new(0.10, 100);
        for _ in 0..100 { budget.try_acquire(); }
        // No requests recorded
        budget.refill();
        // 10% of 0 = 0 tokens (ceil(0) = 0)
        assert!(!budget.try_acquire());
    }
}
```

### 5.3 HedgeExecutor Tests

```rust
#[tokio::test]
async fn test_hedge_disabled_goes_direct() {
    let config = HedgeConfig { enabled: false, ..Default::default() };
    let exec = HedgeExecutor::new(config, mock_pool(), mock_admission());

    let addr = CAddr::hash(b"test");
    // Should fetch from primary only, no hedging
    let result = exec.hedged_fetch(&addr, NodeId(1), &[NodeId(2)]).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_hedge_no_replicas_goes_direct() {
    let exec = HedgeExecutor::new(HedgeConfig::default(), mock_pool(), mock_admission());
    let addr = CAddr::hash(b"test");
    let result = exec.hedged_fetch(&addr, NodeId(1), &[]).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_hedge_wins_when_primary_slow() {
    let pool = MockChannelPool::new();
    pool.set_latency(NodeId(1), Duration::from_millis(200)); // slow primary
    pool.set_latency(NodeId(2), Duration::from_millis(2));   // fast replica

    let config = HedgeConfig {
        default_delay: Duration::from_millis(5),
        ..Default::default()
    };
    let exec = HedgeExecutor::new(config, Arc::new(pool), mock_admission());

    let addr = CAddr::hash(b"test");
    let start = Instant::now();
    let result = exec.hedged_fetch(&addr, NodeId(1), &[NodeId(2)]).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    assert!(elapsed < Duration::from_millis(50)); // hedge won, not 200ms
}

#[tokio::test]
async fn test_primary_wins_before_delay() {
    let pool = MockChannelPool::new();
    pool.set_latency(NodeId(1), Duration::from_millis(1)); // fast primary
    pool.set_latency(NodeId(2), Duration::from_millis(1));

    let config = HedgeConfig {
        default_delay: Duration::from_millis(50), // long delay
        ..Default::default()
    };
    let exec = HedgeExecutor::new(config, Arc::new(pool), mock_admission());

    let addr = CAddr::hash(b"test");
    let start = Instant::now();
    let result = exec.hedged_fetch(&addr, NodeId(1), &[NodeId(2)]).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    assert!(elapsed < Duration::from_millis(10)); // primary won before hedge delay
}

#[tokio::test]
async fn test_hedge_skipped_under_backpressure() {
    let pool = MockChannelPool::new();
    pool.set_latency(NodeId(1), Duration::from_millis(100));
    pool.set_latency(NodeId(2), Duration::from_millis(2));

    let admission = MockAdmission::new();
    admission.set_overloaded(true);

    let config = HedgeConfig {
        default_delay: Duration::from_millis(5),
        ..Default::default()
    };
    let exec = HedgeExecutor::new(config, Arc::new(pool), Arc::new(admission));

    let addr = CAddr::hash(b"test");
    let start = Instant::now();
    let result = exec.hedged_fetch(&addr, NodeId(1), &[NodeId(2)]).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    // Should NOT have hedged, so waited for slow primary
    assert!(elapsed >= Duration::from_millis(90));
}
```

### 5.4 Integration Tests

```rust
#[tokio::test]
async fn test_hedging_reduces_tail_latency() {
    let (nodes, _handles) = start_cluster(3).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Store data (replicated to 2 nodes)
    let addr = put_leaf(&nodes[0], b"hedging_test").await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Inject artificial delay on primary owner
    let primary = nodes[0].ring().primary_owner(&addr);
    inject_delay(&nodes, primary, Duration::from_millis(100));

    // Fetch with hedging enabled
    let mut client = connect_client(&nodes[0]).await;
    let start = Instant::now();
    let result = client.get_value(GetValueRequest {
        addr: addr.as_bytes().to_vec(),
    }).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    // With hedging, should complete much faster than 100ms
    assert!(elapsed < Duration::from_millis(50));
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Primary and hedge both fail | Return primary's error | Hedge failure is secondary |
| 2 | Hedge succeeds, primary eventually succeeds | Primary response discarded (task cancelled) | First wins |
| 3 | Only one replica (RF=1) | No hedging possible, direct fetch | Need ≥2 replicas to hedge |
| 4 | All replicas unhealthy | Fetch from primary only (no hedge target) | Don't send to known-bad nodes |
| 5 | Hedge target same as primary | Skip hedging (no alternative) | Filtered in replica selection |
| 6 | Budget at zero, slow primary | Wait for primary (no hedge) | Budget prevents amplification |
| 7 | Hedge delay > primary response time | Hedge never sent, primary wins | Zero overhead for fast requests |
| 8 | Clock skew between nodes | No impact: delays are local timers | No cross-node time dependency |
| 9 | Hedging during node drain | Draining node excluded from hedge targets | NodeFilter checks DRAINING state |
| 10 | Very high request rate (100K/s) | Budget caps hedges at 10K/s | Linear scaling of hedge overhead |
| 11 | EWMA with all identical latencies | Variance → 0, delay → ewma (≈ min_delay) | Correct: no variance = no need for buffer |
| 12 | First 9 requests (below sample threshold) | Use default_delay | Don't adapt on insufficient data |

### 6.1 Write Safety

```
  CRITICAL: Hedging is ONLY for reads.

  Why not hedge writes?
    PutLeaf: Content-addressed → idempotent → safe to retry.
    BUT: replication side-effects. Two concurrent PutLeaf to different
    nodes could cause inconsistent replica sets.

    StoreRecipe: Same concern with replication.

  The HedgeExecutor is only called from distributed_get path.
  Write paths (put_leaf, store_recipe) never invoke hedging.
  This is enforced by API design, not runtime checks.
```

### 6.2 Cancellation Semantics

```
  When the winning response arrives:
    1. CancellationToken is cancelled.
    2. The losing task's select! branch fires cancel.cancelled().
    3. Task returns Err(Cancelled).
    4. The gRPC channel is NOT closed (connection pooled).
    5. The remote node may still process the request.

  Remote node impact:
    The FetchValue RPC on the losing node completes normally.
    Its response is sent but the client stream is already done.
    tonic handles this gracefully (response discarded).

  No wasted work on the remote node? Not quite:
    The remote node does the full read. We can't cancel remote work.
    This is why budget limiting is essential: caps wasted work at 10%.

  Future: gRPC cancellation propagation (client cancels → server aborts).
  Requires tonic CancellationToken integration on server side.
```

---

## 7. Performance Analysis

### 7.1 Latency Impact

```
┌──────────────────────────────┬──────────┬──────────┬──────────┐
│ Scenario                     │ No Hedge │ Hedged   │ Savings  │
├──────────────────────────────┼──────────┼──────────┼──────────┤
│ p50 (normal)                 │ 2ms      │ 2ms      │ 0%       │
│ p90 (slight delay)           │ 5ms      │ 5ms      │ 0%       │
│ p99 (GC pause on primary)   │ 150ms    │ 8ms      │ 95%      │
│ p99.9 (network retransmit)  │ 300ms    │ 10ms     │ 97%      │
└──────────────────────────────┴──────────┴──────────┴──────────┘

  Hedging has near-zero impact on median latency.
  Dramatic improvement on tail latency (p99+).
  This is the primary value proposition.
```

### 7.2 Load Amplification

```
  With 10% budget and 5% of requests being slow:

  Total requests: 10,000/sec
  Slow requests (>delay): ~500/sec
  Hedge requests sent: min(500, budget=1000) = 500/sec
  Load amplification: 10,500/10,000 = 5% extra load

  Worst case (all requests slow):
  Hedge requests: min(10000, budget=1000) = 1000/sec
  Load amplification: 11,000/10,000 = 10% extra load

  Budget guarantees: max 10% extra load regardless of conditions.
```

### 7.3 Memory Overhead

```
  Per HedgeExecutor:
    LatencyTracker: 24 bytes per peer (2 floats + 1 counter)
    HedgeBudget: 24 bytes (2 atomics + 1 float)
    Config: 64 bytes

  100-node cluster: 24 * 100 + 24 + 64 = 2,488 bytes ≈ 2.5KB
  Negligible.
```

### 7.4 Benchmarking Plan

```rust
/// Benchmark: hedged_fetch with fast primary (hedge not triggered)
#[bench]
fn bench_hedge_fast_primary(b: &mut Bencher) {
    // Primary responds in 1ms, delay=5ms → no hedge
    // Target: <2ms (same as non-hedged)
}

/// Benchmark: hedged_fetch with slow primary (hedge wins)
#[bench]
fn bench_hedge_slow_primary(b: &mut Bencher) {
    // Primary 100ms, replica 2ms, delay=5ms
    // Target: <10ms
}

/// Benchmark: LatencyTracker.record() throughput
#[bench]
fn bench_latency_record(b: &mut Bencher) {
    // Target: >1M records/sec
}

/// Benchmark: HedgeBudget.try_acquire() throughput
#[bench]
fn bench_budget_acquire(b: &mut Bencher) {
    // Atomic operation, target: >10M ops/sec
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/hedge.rs` | **NEW** — HedgeExecutor, LatencyTracker, HedgeBudget, HedgeConfig |
| `deriva-network/src/lib.rs` | Add `pub mod hedge` |
| `deriva-network/src/distributed_get.rs` | Integrate HedgeExecutor into remote fetch path |
| `deriva-server/src/state.rs` | Add `hedge_executor: Arc<HedgeExecutor>` |
| `deriva-server/src/main.rs` | Initialize HedgeExecutor, start refill task |
| `deriva-network/tests/hedge.rs` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

| Crate | Version | Purpose |
|-------|---------|---------|
| `tokio-util` | 0.7.x | `CancellationToken` (already in workspace) |

No new external dependencies.

---

## 10. Design Rationale

### 10.1 Why EWMA Instead of Histogram for Latency?

```
  Histogram (e.g., HDR Histogram):
    + Exact percentiles (p50, p90, p99)
    + Rich statistical analysis
    - O(N) memory (thousands of buckets)
    - Percentile computation is O(buckets)
    - Need to decide bucket boundaries upfront

  EWMA + variance:
    + O(1) memory (2 floats per peer)
    + O(1) update
    + Naturally decays old data (recent samples weighted more)
    - Only approximate percentiles
    - Single "delay" value, not full distribution

  For hedge delay, we need one number: "when should I give up waiting?"
  EWMA + 2*stddev ≈ p95 is good enough for this purpose.
  Exact percentiles would be wasted precision.
```

### 10.2 Why Token Bucket Instead of Percentage Counter?

```
  Percentage counter:
    Track: hedges_sent / total_requests.
    If ratio > 10%, stop hedging.
    Problem: bursty. 100 slow requests in 1ms → 100 hedges → 100% ratio.
    Then no hedging for the rest of the second.

  Token bucket:
    Smooth: tokens refill every second based on last second's traffic.
    Burst-tolerant: up to max_tokens hedges in a burst.
    Self-regulating: high traffic → more tokens. Low traffic → fewer tokens.
    Standard pattern for rate limiting.
```

### 10.3 Why Not Hedge All Requests (No Delay)?

```
  "Tied requests" (send to 2 replicas simultaneously):
    + Lowest possible tail latency
    - 2x load on every request (100% amplification)
    - Doubles network traffic
    - Doubles CPU usage on remote nodes

  Delayed hedging (our approach):
    + Fast requests (majority) never trigger hedge → 0% overhead
    + Only slow requests (tail) trigger hedge → ~5-10% overhead
    + Budget caps worst case at 10%
    - Slightly higher tail latency than tied requests (delay added)

  The delay is the key insight: most requests are fast.
  Only hedge when we have evidence the primary is slow.
```

### 10.4 Why Disable Hedging Under Backpressure?

```
  Scenario: cluster under heavy load, all nodes slow.
  Without check: hedging sends MORE requests → more load → slower → more hedging.
  Positive feedback loop → cascading failure.

  With backpressure check:
    admission.is_overloaded() → true → skip hedge.
    Reduces load by 10% (no hedge traffic).
    Helps cluster recover.

  Hedging is a fair-weather optimization.
  Under stress, it becomes harmful. Disable it.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `hedge_requests_total` | Counter | — | Hedge requests actually sent |
| `hedge_wins_total` | Counter | — | Times hedge response arrived first |
| `hedge_primary_wins` | Counter | — | Times primary won (hedge cancelled) |
| `hedge_budget_exhausted` | Counter | — | Times hedge skipped due to budget |
| `hedge_backpressure_skipped` | Counter | — | Times hedge skipped due to overload |
| `hedge_delay_ms` | Histogram | — | Computed hedge delay values |
| `hedge_latency_saved_ms` | Histogram | — | Latency saved when hedge wins |
| `peer_latency_ewma_ms` | Gauge | `peer` | Current EWMA per peer |

### 11.2 Structured Logging

```rust
tracing::debug!(
    addr = %addr,
    primary = primary.0,
    hedge_target = hedge_target.0,
    delay_ms = delay.as_millis(),
    "sending hedged request"
);

tracing::debug!(
    addr = %addr,
    winner = %winner.0,
    elapsed_ms = elapsed.as_millis(),
    hedge_sent = hedge_was_sent,
    "hedged fetch complete"
);

tracing::debug!(
    reason = "budget_exhausted",
    "hedge skipped"
);

tracing::debug!(
    reason = "backpressure",
    "hedge skipped"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| High hedge rate | `hedge_requests_total` / total_requests > 20% | Warning |
| Hedge never wins | `hedge_wins_total` == 0 for 10min (while hedging) | Info |
| Budget always exhausted | `hedge_budget_exhausted` > 50% of hedge-eligible | Warning |
| Peer latency spike | `peer_latency_ewma_ms` > 100ms for any peer | Warning |

---

## 12. Checklist

- [ ] Create `deriva-network/src/hedge.rs`
- [ ] Implement `LatencyTracker` with EWMA + variance
- [ ] Implement `HedgeBudget` with token bucket
- [ ] Implement `HedgeExecutor::hedged_fetch` with select! racing
- [ ] Implement cancellation via `CancellationToken`
- [ ] Implement budget refill background task
- [ ] Implement backpressure check before hedging
- [ ] Integrate into `DistributedGet::get()` remote fetch path
- [ ] Filter unhealthy/draining nodes from hedge targets
- [ ] Add `HedgeConfig` with defaults
- [ ] Add config to `deriva.toml`
- [ ] Wire HedgeExecutor in ServerState and main.rs
- [ ] Write LatencyTracker tests (5 tests)
- [ ] Write HedgeBudget tests (4 tests)
- [ ] Write HedgeExecutor tests (4 tests)
- [ ] Write integration test (1 test)
- [ ] Add metrics (8 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (4 alerts)
- [ ] Run benchmarks: fast primary, slow primary, tracker throughput, budget throughput
- [ ] Verify writes are never hedged (code review)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
