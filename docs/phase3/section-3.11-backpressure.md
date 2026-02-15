# §3.11 Backpressure & Admission Control

> **Status:** Not started
> **Depends on:** §3.6 (distributed get), §3.10 (migration)
> **Crate(s):** `deriva-network`, `deriva-server`
> **Estimated effort:** 2–3 days

---

## 1. Problem Statement

A distributed system without admission control degrades catastrophically under
overload. When request rate exceeds capacity, unbounded queuing causes:

1. **Memory exhaustion**: each queued request holds buffers, futures, and channel
   slots. At 10K queued requests × 64KB avg = 640MB of dead weight.

2. **Latency amplification**: requests queued for 30s are useless to callers who
   already timed out, yet the server still processes them, wasting CPU on stale work.

3. **Cascade failure**: Node A overloaded → slow responses to Node B → B's queues
   grow → B overloaded → entire cluster collapses.

4. **Unfair degradation**: without priority, a batch analytics job starves
   interactive user reads.

### Goals

- **Bounded queuing**: hard limit on in-flight requests (default 1024).
- **Fast rejection**: shed load with `RESOURCE_EXHAUSTED` within 1ms, not after
  queuing for 30s.
- **Priority levels**: interactive reads > batch operations > background (GC, migration).
- **Adaptive concurrency**: dynamically adjust concurrency limit based on observed
  latency (TCP Vegas-style).
- **Memory pressure**: detect system memory usage and shed load before OOM.
- **Per-peer fairness**: no single peer can monopolize the request queue.

### Non-Goals

- Rate limiting per API key (future §4.x REST API concern).
- Cost-based admission (e.g., reject expensive computations first).

---

## 2. Design

### 2.1 Architecture Overview

```
  Incoming gRPC request
         │
         ▼
  ┌──────────────────────┐
  │  Memory Pressure     │──► REJECT if memory > 90%
  │  Check               │
  └──────┬───────────────┘
         │ OK
         ▼
  ┌──────────────────────┐
  │  Per-Peer Fairness   │──► REJECT if peer > per_peer_limit
  │  Check               │
  └──────┬───────────────┘
         │ OK
         ▼
  ┌──────────────────────┐
  │  Priority Queue      │
  │  ┌────────────────┐  │
  │  │ HIGH (reads)   │  │──► Semaphore acquire
  │  │ NORMAL (writes)│  │    (adaptive limit)
  │  │ LOW (bg tasks) │  │
  │  └────────────────┘  │
  └──────┬───────────────┘
         │ acquired
         ▼
  ┌──────────────────────┐
  │  Request Handler     │
  │  (actual work)       │
  └──────────────────────┘
```

### 2.2 Adaptive Concurrency — TCP Vegas Model

```
  The concurrency limit adjusts based on observed latency:

  Every window (1 second):
    1. Measure: avg_latency over last window
    2. Compare to baseline: min_latency (best observed)
    3. Compute queue estimate: in_flight × (1 - min_latency/avg_latency)
    4. Adjust:
       if queue_estimate < alpha (2):  limit += 1  (under-utilized)
       if queue_estimate > beta (8):   limit -= 1  (overloaded)
       else:                           limit unchanged

  Bounds: min_limit=8, max_limit=1024

  ┌─────────────────────────────────────────────────────┐
  │ Concurrency Limit Over Time                         │
  │                                                     │
  │  1024 ┤                                             │
  │       │         ╭──────╮                            │
  │   512 ┤    ╭───╯      ╰───╮     load spike         │
  │       │   ╯                ╰──╮                     │
  │   256 ┤──╯                    ╰──────────           │
  │       │                                             │
  │     8 ┤                                             │
  │       └─────────────────────────────────── time     │
  └─────────────────────────────────────────────────────┘
```

### 2.3 Core Types

```rust
/// Priority levels for request admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Background: GC, migration, replication
    Low = 0,
    /// Normal: writes, recipe puts
    Normal = 1,
    /// High: interactive reads, health checks
    High = 2,
}

/// Admission controller — gates all incoming requests.
pub struct AdmissionController {
    config: AdmissionConfig,
    /// Adaptive concurrency limiter.
    limiter: Arc<AdaptiveLimiter>,
    /// Per-peer request counters.
    peer_counts: Arc<DashMap<NodeId, AtomicU32>>,
    /// Memory pressure monitor.
    memory_monitor: Arc<MemoryMonitor>,
    /// Metrics.
    metrics: AdmissionMetrics,
}

#[derive(Debug, Clone)]
pub struct AdmissionConfig {
    /// Hard ceiling on concurrent requests.
    pub max_concurrent: usize,          // default: 1024
    /// Initial concurrency limit (adaptive adjusts from here).
    pub initial_limit: usize,           // default: 128
    /// Minimum concurrency limit.
    pub min_limit: usize,               // default: 8
    /// Per-peer max concurrent requests.
    pub per_peer_limit: usize,          // default: 64
    /// Memory pressure threshold (fraction 0.0–1.0).
    pub memory_pressure_threshold: f64, // default: 0.85
    /// Memory critical threshold — reject all non-HIGH.
    pub memory_critical_threshold: f64, // default: 0.95
    /// Vegas alpha (queue too small → increase limit).
    pub vegas_alpha: f64,               // default: 2.0
    /// Vegas beta (queue too large → decrease limit).
    pub vegas_beta: f64,                // default: 8.0
    /// Window duration for latency sampling.
    pub window_duration: Duration,      // default: 1s
    /// Fraction of LOW priority to shed under pressure.
    pub low_priority_shed_ratio: f64,   // default: 0.5
}

impl Default for AdmissionConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 1024,
            initial_limit: 128,
            min_limit: 8,
            per_peer_limit: 64,
            memory_pressure_threshold: 0.85,
            memory_critical_threshold: 0.95,
            vegas_alpha: 2.0,
            vegas_beta: 8.0,
            window_duration: Duration::from_secs(1),
            low_priority_shed_ratio: 0.5,
        }
    }
}

/// RAII guard — released when request completes.
pub struct AdmissionPermit {
    limiter: Arc<AdaptiveLimiter>,
    peer_id: Option<NodeId>,
    peer_counts: Arc<DashMap<NodeId, AtomicU32>>,
    start: Instant,
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        self.limiter.release(self.start.elapsed());
        if let Some(ref peer) = self.peer_id {
            if let Some(counter) = self.peer_counts.get(peer) {
                counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}

/// Adaptive concurrency limiter (Vegas-style).
pub struct AdaptiveLimiter {
    current_limit: AtomicUsize,
    in_flight: AtomicUsize,
    semaphore: Arc<Semaphore>,
    min_latency: Mutex<Duration>,
    window_latencies: Mutex<Vec<Duration>>,
    config: AdmissionConfig,
}

/// Monitors system memory usage.
pub struct MemoryMonitor {
    threshold: f64,
    critical: f64,
    cached_usage: AtomicU64, // updated every 500ms
}
```

### 2.4 Priority Mapping

```
┌─────────────────────────────┬──────────┐
│ Operation                   │ Priority │
├─────────────────────────────┼──────────┤
│ GetValue (read)             │ High     │
│ FetchValue (remote read)    │ High     │
│ HealthCheck                 │ High     │
│ PutLeaf (write)             │ Normal   │
│ PutRecipe (write)           │ Normal   │
│ BatchGet                    │ Normal   │
│ BatchPutLeaf               │ Normal   │
│ GcCollectRoots             │ Low      │
│ GcSweep                    │ Low      │
│ MigrateTransfer            │ Low      │
│ SyncRecipes                │ Low      │
│ ReplicateLeaf              │ Low      │
└─────────────────────────────┴──────────┘
```

---

## 3. Implementation

### 3.1 AdmissionController

```rust
impl AdmissionController {
    pub fn new(config: AdmissionConfig) -> Self {
        let limiter = Arc::new(AdaptiveLimiter::new(&config));
        let memory_monitor = Arc::new(MemoryMonitor::new(
            config.memory_pressure_threshold,
            config.memory_critical_threshold,
        ));

        let mm = memory_monitor.clone();
        tokio::spawn(async move { mm.poll_loop().await });

        let lim = limiter.clone();
        let cfg = config.clone();
        tokio::spawn(async move { lim.adjustment_loop(&cfg).await });

        Self {
            config,
            limiter,
            peer_counts: Arc::new(DashMap::new()),
            memory_monitor,
            metrics: AdmissionMetrics::new(),
        }
    }

    /// Try to admit a request. Returns permit on success.
    pub async fn try_admit(
        &self,
        priority: Priority,
        peer_id: Option<NodeId>,
    ) -> Result<AdmissionPermit, AdmissionError> {
        // 1. Memory pressure check
        let mem_usage = self.memory_monitor.usage();
        if mem_usage > self.config.memory_critical_threshold {
            if priority != Priority::High {
                self.metrics.rejected_memory.fetch_add(1, Ordering::Relaxed);
                return Err(AdmissionError::MemoryPressure(mem_usage));
            }
        } else if mem_usage > self.config.memory_pressure_threshold {
            if priority == Priority::Low {
                self.metrics.rejected_memory.fetch_add(1, Ordering::Relaxed);
                return Err(AdmissionError::MemoryPressure(mem_usage));
            }
        }

        // 2. Per-peer fairness check
        if let Some(ref pid) = peer_id {
            let count = self.peer_counts
                .entry(*pid)
                .or_insert_with(|| AtomicU32::new(0));
            let current = count.load(Ordering::Relaxed);
            if current >= self.config.per_peer_limit as u32 {
                self.metrics.rejected_peer_limit.fetch_add(1, Ordering::Relaxed);
                return Err(AdmissionError::PeerLimitExceeded(*pid));
            }
            count.fetch_add(1, Ordering::Relaxed);
        }

        // 3. Concurrency limiter (priority-aware)
        match priority {
            Priority::High => {
                // High priority: try immediately, short timeout
                match tokio::time::timeout(
                    Duration::from_millis(100),
                    self.limiter.acquire(),
                ).await {
                    Ok(()) => {}
                    Err(_) => {
                        self.dec_peer(peer_id.as_ref());
                        self.metrics.rejected_overload.fetch_add(1, Ordering::Relaxed);
                        return Err(AdmissionError::Overloaded);
                    }
                }
            }
            Priority::Normal => {
                match tokio::time::timeout(
                    Duration::from_millis(50),
                    self.limiter.acquire(),
                ).await {
                    Ok(()) => {}
                    Err(_) => {
                        self.dec_peer(peer_id.as_ref());
                        self.metrics.rejected_overload.fetch_add(1, Ordering::Relaxed);
                        return Err(AdmissionError::Overloaded);
                    }
                }
            }
            Priority::Low => {
                // Low priority: try_acquire only, no waiting
                if !self.limiter.try_acquire() {
                    self.dec_peer(peer_id.as_ref());
                    self.metrics.rejected_overload.fetch_add(1, Ordering::Relaxed);
                    return Err(AdmissionError::Overloaded);
                }
            }
        }

        self.metrics.admitted.fetch_add(1, Ordering::Relaxed);
        Ok(AdmissionPermit {
            limiter: self.limiter.clone(),
            peer_id,
            peer_counts: self.peer_counts.clone(),
            start: Instant::now(),
        })
    }

    fn dec_peer(&self, peer_id: Option<&NodeId>) {
        if let Some(pid) = peer_id {
            if let Some(counter) = self.peer_counts.get(pid) {
                counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AdmissionError {
    #[error("memory pressure: {0:.1}% used")]
    MemoryPressure(f64),
    #[error("peer {0} exceeded per-peer limit")]
    PeerLimitExceeded(NodeId),
    #[error("server overloaded")]
    Overloaded,
}

impl From<AdmissionError> for tonic::Status {
    fn from(e: AdmissionError) -> Self {
        tonic::Status::resource_exhausted(e.to_string())
    }
}
```

### 3.2 Adaptive Limiter (Vegas)

```rust
impl AdaptiveLimiter {
    pub fn new(config: &AdmissionConfig) -> Self {
        let limit = config.initial_limit;
        Self {
            current_limit: AtomicUsize::new(limit),
            in_flight: AtomicUsize::new(0),
            semaphore: Arc::new(Semaphore::new(limit)),
            min_latency: Mutex::new(Duration::from_secs(60)),
            window_latencies: Mutex::new(Vec::with_capacity(1024)),
            config: config.clone(),
        }
    }

    pub async fn acquire(&self) {
        let _ = self.semaphore.acquire().await.unwrap();
        self.in_flight.fetch_add(1, Ordering::Relaxed);
    }

    pub fn try_acquire(&self) -> bool {
        match self.semaphore.try_acquire() {
            Ok(_permit) => {
                self.in_flight.fetch_add(1, Ordering::Relaxed);
                std::mem::forget(_permit); // managed manually
                true
            }
            Err(_) => false,
        }
    }

    pub fn release(&self, latency: Duration) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
        self.semaphore.add_permits(1);
        if let Ok(mut window) = self.window_latencies.try_lock() {
            window.push(latency);
        }
    }

    /// Runs every window_duration, adjusts limit.
    pub async fn adjustment_loop(&self, config: &AdmissionConfig) {
        let mut interval = tokio::time::interval(config.window_duration);
        loop {
            interval.tick().await;
            self.adjust(config);
        }
    }

    fn adjust(&self, config: &AdmissionConfig) {
        let latencies = {
            let mut window = self.window_latencies.lock().unwrap();
            let taken = std::mem::take(&mut *window);
            taken
        };

        if latencies.is_empty() {
            return;
        }

        let avg: Duration = latencies.iter().sum::<Duration>() / latencies.len() as u32;

        // Update min_latency
        {
            let mut min = self.min_latency.lock().unwrap();
            if avg < *min {
                *min = avg;
            }
        }

        let min_lat = *self.min_latency.lock().unwrap();
        if min_lat.is_zero() {
            return;
        }

        let in_flight = self.in_flight.load(Ordering::Relaxed) as f64;
        let queue_estimate = in_flight * (1.0 - min_lat.as_secs_f64() / avg.as_secs_f64());

        let current = self.current_limit.load(Ordering::Relaxed);
        let new_limit = if queue_estimate < config.vegas_alpha {
            (current + 1).min(config.max_concurrent)
        } else if queue_estimate > config.vegas_beta {
            (current - 1).max(config.min_limit)
        } else {
            current
        };

        if new_limit != current {
            self.current_limit.store(new_limit, Ordering::Relaxed);
            // Adjust semaphore permits
            if new_limit > current {
                self.semaphore.add_permits(new_limit - current);
            }
            // For decrease: permits naturally drain as they're not returned
            tracing::debug!(
                old = current, new = new_limit,
                queue_est = format!("{:.1}", queue_estimate),
                "concurrency limit adjusted"
            );
        }
    }
}
```

### 3.3 Memory Monitor

```rust
impl MemoryMonitor {
    pub fn new(threshold: f64, critical: f64) -> Self {
        Self {
            threshold,
            critical,
            cached_usage: AtomicU64::new(0),
        }
    }

    /// Returns memory usage as fraction (0.0–1.0).
    pub fn usage(&self) -> f64 {
        f64::from_bits(self.cached_usage.load(Ordering::Relaxed))
    }

    pub fn is_pressured(&self) -> bool {
        self.usage() > self.threshold
    }

    /// Poll /proc/meminfo every 500ms.
    pub async fn poll_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            interval.tick().await;
            if let Ok(usage) = Self::read_memory_usage() {
                self.cached_usage.store(usage.to_bits(), Ordering::Relaxed);
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn read_memory_usage() -> Result<f64, std::io::Error> {
        let contents = std::fs::read_to_string("/proc/meminfo")?;
        let mut total = 0u64;
        let mut available = 0u64;
        for line in contents.lines() {
            if line.starts_with("MemTotal:") {
                total = Self::parse_kb(line);
            } else if line.starts_with("MemAvailable:") {
                available = Self::parse_kb(line);
            }
        }
        if total == 0 { return Ok(0.0); }
        Ok(1.0 - (available as f64 / total as f64))
    }

    #[cfg(not(target_os = "linux"))]
    fn read_memory_usage() -> Result<f64, std::io::Error> {
        Ok(0.0) // non-Linux: assume no pressure
    }

    fn parse_kb(line: &str) -> u64 {
        line.split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }
}
```

### 3.4 gRPC Interceptor Integration

```rust
/// Tonic interceptor that enforces admission control.
pub fn admission_interceptor(
    controller: Arc<AdmissionController>,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    move |mut req: Request<()>| {
        let priority = extract_priority(&req);
        let peer_id = extract_peer_id(&req);

        // Use block_on for sync interceptor context
        // (In practice, use tonic's async interceptor or middleware layer)
        let permit = tokio::runtime::Handle::current()
            .block_on(controller.try_admit(priority, peer_id))
            .map_err(tonic::Status::from)?;

        // Attach permit to request extensions so it lives until response
        req.extensions_mut().insert(permit);
        Ok(req)
    }
}

fn extract_priority(req: &Request<()>) -> Priority {
    req.metadata()
        .get("x-deriva-priority")
        .and_then(|v| v.to_str().ok())
        .map(|s| match s {
            "high" => Priority::High,
            "low" => Priority::Low,
            _ => Priority::Normal,
        })
        .unwrap_or(Priority::Normal)
}

fn extract_peer_id(req: &Request<()>) -> Option<NodeId> {
    req.metadata()
        .get("x-deriva-node-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| NodeId::parse(s).ok())
}
```


---

## 4. Data Flow Diagrams

### 4.1 Normal Request — Admitted

```
  Client ──GetValue──► Server
                         │
                    MemoryMonitor: 45% → OK
                    PeerFairness: 3/64 → OK
                    AdaptiveLimiter: 50/128 → acquire() → OK
                         │
                    ┌────▼────┐
                    │ Handler │ process request
                    └────┬────┘
                         │
                    Permit dropped → release()
                    latency recorded → Vegas window
                         │
                    ◄── Response ── Client
```

### 4.2 Overload — Request Rejected

```
  Client ──PutLeaf──► Server
                         │
                    MemoryMonitor: 72% → OK
                    PeerFairness: 12/64 → OK
                    AdaptiveLimiter: 128/128 → timeout(50ms) → FAIL
                         │
                    ◄── RESOURCE_EXHAUSTED ── Client
                         │
                    Client retries with backoff
```

### 4.3 Memory Pressure — Tiered Shedding

```
  Memory at 87% (above threshold 85%, below critical 95%):

  ┌─────────────────────────────────────────────────┐
  │ Incoming requests:                              │
  │                                                 │
  │ GetValue (HIGH)    → ADMITTED ✓                 │
  │ PutLeaf (NORMAL)   → ADMITTED ✓                 │
  │ GcSweep (LOW)      → REJECTED (memory pressure)│
  │ MigrateTransfer(LOW)→ REJECTED (memory pressure)│
  │ GetValue (HIGH)    → ADMITTED ✓                 │
  └─────────────────────────────────────────────────┘

  Memory at 96% (above critical 95%):

  ┌─────────────────────────────────────────────────┐
  │ GetValue (HIGH)    → ADMITTED ✓ (only HIGH)     │
  │ PutLeaf (NORMAL)   → REJECTED (critical memory) │
  │ GcSweep (LOW)      → REJECTED (critical memory) │
  └─────────────────────────────────────────────────┘
```

### 4.4 Per-Peer Fairness

```
  Node B sends 65 concurrent requests to Node A:

  Request 1–64:  PeerFairness: B=1..64/64 → ADMITTED ✓
  Request 65:    PeerFairness: B=64/64 → REJECTED
                 ◄── RESOURCE_EXHAUSTED("peer B exceeded limit")

  Meanwhile, Node C's requests still admitted:
  Request from C: PeerFairness: C=1/64 → ADMITTED ✓

  Effect: B's flood doesn't block C's requests.
```

### 4.5 Vegas Adaptive Concurrency

```
  Time ──────────────────────────────────────────────►

  Window 1: avg_latency=5ms, min_latency=5ms
    queue_est = 50 × (1 - 5/5) = 0 < alpha(2)
    limit: 128 → 129 (increase)

  Window 2: avg_latency=5ms
    queue_est = 0 < alpha(2)
    limit: 129 → 130

  ... (steady increase during low load)

  Window 50: limit=178, load spike arrives
    avg_latency=50ms, min_latency=5ms
    queue_est = 178 × (1 - 5/50) = 178 × 0.9 = 160 > beta(8)
    limit: 178 → 177 (decrease)

  Window 51: avg_latency=45ms
    queue_est = 177 × (1 - 5/45) = 157 > beta(8)
    limit: 177 → 176

  ... (gradual decrease until latency stabilizes)

  Window 80: avg_latency=8ms, limit=64
    queue_est = 64 × (1 - 5/8) = 24 > beta(8)
    limit: 64 → 63

  Window 100: avg_latency=6ms, limit=45
    queue_est = 45 × (1 - 5/6) = 7.5 → between alpha and beta
    limit: 45 (stable)
```

### 4.6 Cascade Prevention

```
  Without backpressure:
  ┌───────┐     ┌───────┐     ┌───────┐
  │ Node A│────►│ Node B│────►│ Node C│
  │ queue │     │ queue │     │ queue │
  │ grows │     │ grows │     │ SLOW  │
  │ OOM!  │     │ OOM!  │     │       │
  └───────┘     └───────┘     └───────┘

  With backpressure:
  ┌───────┐     ┌───────┐     ┌───────┐
  │ Node A│────►│ Node B│────►│ Node C│
  │       │     │       │     │ SLOW  │
  │       │  ◄──│REJECT │  ◄──│REJECT │
  │ retry │     │       │     │       │
  │ later │     │       │     │       │
  └───────┘     └───────┘     └───────┘

  A gets RESOURCE_EXHAUSTED from B within 50ms.
  A can retry, route elsewhere, or return error to client.
  No cascading queue buildup.
```

---

## 5. Test Specification

### 5.1 Admission Controller Tests

```rust
#[cfg(test)]
mod admission_tests {
    use super::*;

    fn default_controller() -> AdmissionController {
        AdmissionController::new(AdmissionConfig {
            max_concurrent: 16,
            initial_limit: 8,
            min_limit: 2,
            per_peer_limit: 4,
            memory_pressure_threshold: 0.85,
            memory_critical_threshold: 0.95,
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn test_admit_under_limit() {
        let ctrl = default_controller();
        let permit = ctrl.try_admit(Priority::Normal, None).await;
        assert!(permit.is_ok());
    }

    #[tokio::test]
    async fn test_reject_when_full() {
        let ctrl = default_controller();
        let mut permits = Vec::new();

        // Fill up to limit (8)
        for _ in 0..8 {
            permits.push(ctrl.try_admit(Priority::Normal, None).await.unwrap());
        }

        // 9th should be rejected (Normal has 50ms timeout)
        let result = ctrl.try_admit(Priority::Normal, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_low_priority_rejected_immediately_when_full() {
        let ctrl = default_controller();
        let mut permits = Vec::new();
        for _ in 0..8 {
            permits.push(ctrl.try_admit(Priority::Normal, None).await.unwrap());
        }

        // LOW uses try_acquire — instant rejection
        let start = Instant::now();
        let result = ctrl.try_admit(Priority::Low, None).await;
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_millis(5));
    }

    #[tokio::test]
    async fn test_permit_release_allows_next() {
        let ctrl = default_controller();
        let mut permits = Vec::new();
        for _ in 0..8 {
            permits.push(ctrl.try_admit(Priority::Normal, None).await.unwrap());
        }

        // Drop one permit
        permits.pop();

        // Now one slot available
        let result = ctrl.try_admit(Priority::Normal, None).await;
        assert!(result.is_ok());
    }
}
```

### 5.2 Per-Peer Fairness Tests

```rust
#[tokio::test]
async fn test_per_peer_limit() {
    let ctrl = AdmissionController::new(AdmissionConfig {
        max_concurrent: 100,
        initial_limit: 100,
        per_peer_limit: 3,
        ..Default::default()
    });

    let peer = NodeId::from("peer_A");
    let mut permits = Vec::new();

    for _ in 0..3 {
        permits.push(ctrl.try_admit(Priority::Normal, Some(peer)).await.unwrap());
    }

    // 4th from same peer → rejected
    let result = ctrl.try_admit(Priority::Normal, Some(peer)).await;
    assert!(matches!(result, Err(AdmissionError::PeerLimitExceeded(_))));

    // Different peer → admitted
    let other = NodeId::from("peer_B");
    let result = ctrl.try_admit(Priority::Normal, Some(other)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_peer_count_decrements_on_drop() {
    let ctrl = AdmissionController::new(AdmissionConfig {
        max_concurrent: 100,
        initial_limit: 100,
        per_peer_limit: 2,
        ..Default::default()
    });

    let peer = NodeId::from("peer_A");
    let p1 = ctrl.try_admit(Priority::Normal, Some(peer)).await.unwrap();
    let p2 = ctrl.try_admit(Priority::Normal, Some(peer)).await.unwrap();

    // Full for this peer
    assert!(ctrl.try_admit(Priority::Normal, Some(peer)).await.is_err());

    drop(p1);
    // Slot freed
    assert!(ctrl.try_admit(Priority::Normal, Some(peer)).await.is_ok());
}
```

### 5.3 Memory Pressure Tests

```rust
#[tokio::test]
async fn test_memory_pressure_sheds_low_priority() {
    let ctrl = default_controller();
    // Simulate 90% memory usage
    ctrl.memory_monitor.cached_usage.store(
        (0.90_f64).to_bits(), Ordering::Relaxed,
    );

    // HIGH → admitted
    assert!(ctrl.try_admit(Priority::High, None).await.is_ok());
    // NORMAL → admitted (above threshold but below critical)
    assert!(ctrl.try_admit(Priority::Normal, None).await.is_ok());
    // LOW → rejected
    assert!(ctrl.try_admit(Priority::Low, None).await.is_err());
}

#[tokio::test]
async fn test_memory_critical_only_high() {
    let ctrl = default_controller();
    ctrl.memory_monitor.cached_usage.store(
        (0.96_f64).to_bits(), Ordering::Relaxed,
    );

    assert!(ctrl.try_admit(Priority::High, None).await.is_ok());
    assert!(ctrl.try_admit(Priority::Normal, None).await.is_err());
    assert!(ctrl.try_admit(Priority::Low, None).await.is_err());
}
```

### 5.4 Adaptive Limiter Tests

```rust
#[test]
fn test_vegas_increase_on_low_queue() {
    let config = AdmissionConfig {
        initial_limit: 100,
        vegas_alpha: 2.0,
        vegas_beta: 8.0,
        max_concurrent: 1024,
        min_limit: 8,
        ..Default::default()
    };
    let limiter = AdaptiveLimiter::new(&config);
    *limiter.min_latency.lock().unwrap() = Duration::from_millis(5);

    // Simulate low queue: avg ≈ min
    {
        let mut w = limiter.window_latencies.lock().unwrap();
        for _ in 0..100 {
            w.push(Duration::from_millis(5));
        }
    }
    limiter.in_flight.store(10, Ordering::Relaxed);

    limiter.adjust(&config);
    assert_eq!(limiter.current_limit.load(Ordering::Relaxed), 101);
}

#[test]
fn test_vegas_decrease_on_high_queue() {
    let config = AdmissionConfig {
        initial_limit: 100,
        vegas_alpha: 2.0,
        vegas_beta: 8.0,
        max_concurrent: 1024,
        min_limit: 8,
        ..Default::default()
    };
    let limiter = AdaptiveLimiter::new(&config);
    *limiter.min_latency.lock().unwrap() = Duration::from_millis(5);

    // Simulate high queue: avg >> min
    {
        let mut w = limiter.window_latencies.lock().unwrap();
        for _ in 0..100 {
            w.push(Duration::from_millis(50));
        }
    }
    limiter.in_flight.store(100, Ordering::Relaxed);

    limiter.adjust(&config);
    assert_eq!(limiter.current_limit.load(Ordering::Relaxed), 99);
}

#[test]
fn test_vegas_respects_min_limit() {
    let config = AdmissionConfig {
        initial_limit: 9,
        min_limit: 8,
        vegas_beta: 8.0,
        ..Default::default()
    };
    let limiter = AdaptiveLimiter::new(&config);
    *limiter.min_latency.lock().unwrap() = Duration::from_millis(1);

    {
        let mut w = limiter.window_latencies.lock().unwrap();
        for _ in 0..100 { w.push(Duration::from_millis(100)); }
    }
    limiter.in_flight.store(100, Ordering::Relaxed);

    limiter.adjust(&config);
    assert_eq!(limiter.current_limit.load(Ordering::Relaxed), 8); // min

    limiter.adjust(&config);
    assert_eq!(limiter.current_limit.load(Ordering::Relaxed), 8); // stays at min
}
```

### 5.5 Integration Tests

```rust
#[tokio::test]
async fn test_server_returns_resource_exhausted() {
    let (node, _handle) = start_node_with_config(AdmissionConfig {
        max_concurrent: 4,
        initial_limit: 4,
        ..Default::default()
    }).await;

    let mut client = connect_client(&node).await;

    // Saturate with slow requests
    let mut handles = Vec::new();
    for _ in 0..4 {
        let mut c = client.clone();
        handles.push(tokio::spawn(async move {
            c.get_value(slow_request()).await
        }));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 5th request should get RESOURCE_EXHAUSTED
    let result = client.get_value(fast_request()).await;
    assert_eq!(result.unwrap_err().code(), tonic::Code::ResourceExhausted);
}

#[tokio::test]
async fn test_high_priority_admitted_when_normal_rejected() {
    let (node, _handle) = start_node_with_config(AdmissionConfig {
        max_concurrent: 4,
        initial_limit: 4,
        ..Default::default()
    }).await;

    let mut client = connect_client(&node).await;

    // Fill with LOW priority
    let mut handles = Vec::new();
    for _ in 0..4 {
        let mut c = client.clone();
        handles.push(tokio::spawn(async move {
            c.migrate_transfer(slow_stream()).await
        }));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;

    // HIGH priority gets 100ms timeout to wait for a slot
    // (one of the LOW tasks may complete, or timeout → rejected)
    // This tests the priority timeout differentiation
    let mut high_client = connect_client_with_priority(&node, "high").await;
    let _result = high_client.get_value(fast_request()).await;
    // Result depends on timing — test validates no panic/hang
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | All requests are HIGH priority | Degrades to FIFO with 100ms timeout | No starvation, bounded wait |
| 2 | Memory reads fail (/proc unavailable) | Assume 0% usage, no shedding | Safe: over-admit rather than block |
| 3 | Vegas min_latency never updates | Stays at initial 60s → queue_est always 0 → limit grows | Converges once real traffic arrives |
| 4 | Zero requests in window | Skip adjustment | No data to decide on |
| 5 | Permit leaked (handler panics) | Drop impl releases permit | RAII guarantees cleanup |
| 6 | Peer ID not in metadata | peer_id=None, skip fairness check | Client requests have no peer ID |
| 7 | Semaphore permits go negative (decrease) | Permits drain naturally, no forced revocation | Avoids cancelling in-flight work |
| 8 | Config: per_peer_limit > max_concurrent | Effective limit is max_concurrent | Peer can't exceed global limit |
| 9 | Rapid priority changes on same connection | Each request evaluated independently | Stateless per-request |
| 10 | Memory oscillates around threshold | Rapid admit/reject for LOW | Acceptable: LOW is best-effort |

### 6.1 Semaphore Decrease Mechanics

```
Problem: Vegas says decrease limit from 100 to 99.
  But Semaphore has no "remove_permits" API.

Solution: track current_limit separately.
  - On decrease: just update current_limit counter.
  - Don't add permits back when releasing if in_flight > current_limit.
  - Permits naturally drain as requests complete.

  Example:
    limit=100, in_flight=95, semaphore_permits=5
    Vegas: limit → 99
    current_limit=99
    Next release: in_flight=94, but we only add permit if
      semaphore_permits < current_limit - in_flight
    Eventually semaphore stabilizes at new limit.

  Simpler alternative (what we implement):
    On decrease, just update the counter.
    Semaphore may temporarily have more permits than limit.
    This is fine — the overshoot is at most 1 per window (1/sec).
    Within a few seconds, it converges.
```

### 6.2 Client Retry Guidance

```
When client receives RESOURCE_EXHAUSTED:

  Recommended retry strategy:
  1. Wait: random(50ms, 200ms) × 2^attempt
  2. Max attempts: 3
  3. Include jitter to avoid thundering herd
  4. If all retries fail: return error to caller

  Server includes Retry-After hint in metadata:
    grpc-retry-after-ms: 100

  Client libraries should respect this hint.
```

---

## 7. Performance Analysis

### 7.1 Admission Overhead

```
┌──────────────────────────────┬──────────┬──────────────────────┐
│ Check                        │ Latency  │ Notes                │
├──────────────────────────────┼──────────┼──────────────────────┤
│ Memory pressure read         │ ~10ns    │ Atomic load          │
│ Per-peer counter check       │ ~50ns    │ DashMap lookup       │
│ Semaphore acquire (no wait)  │ ~100ns   │ Atomic CAS           │
│ Semaphore acquire (wait)     │ 0–100ms  │ Depends on priority  │
├──────────────────────────────┼──────────┼──────────────────────┤
│ Total (fast path)            │ ~200ns   │ Negligible           │
│ Total (rejection)            │ <1ms     │ Fast fail            │
└──────────────────────────────┴──────────┴──────────────────────┘
```

### 7.2 Vegas Convergence

```
Starting from initial_limit=128:

  Under-loaded (10 req/s, 5ms latency):
    Limit grows by 1/sec → reaches max_concurrent in ~15 min
    (In practice, stays well below max since queue_est stays low)

  Overloaded (10K req/s, latency climbing):
    Limit decreases by 1/sec → slow convergence
    But: rejection happens at semaphore level immediately
    Vegas adjusts the CEILING, not the immediate behavior

  Spike recovery:
    Load drops → latency drops → queue_est drops → limit grows
    Recovery: ~30 seconds to regain 30 permits
```

### 7.3 Memory Monitor Overhead

```
/proc/meminfo read: ~5μs (kernel provides cached values)
Frequency: every 500ms
CPU overhead: 0.001% of one core
Memory: 64 bytes (two AtomicU64)
```

### 7.4 Benchmarking Plan

```rust
/// Benchmark: admission fast path (no contention)
#[bench]
fn bench_admit_no_contention(b: &mut Bencher) {
    // Single thread, limit=1024, 0 in-flight
    // Expected: <500ns
}

/// Benchmark: admission with contention (8 threads)
#[bench]
fn bench_admit_contention(b: &mut Bencher) {
    // 8 threads, limit=64
    // Expected: <5μs avg
}

/// Benchmark: Vegas adjustment cycle
#[bench]
fn bench_vegas_adjust(b: &mut Bencher) {
    // 1000 latency samples, compute adjustment
    // Expected: <100μs
}

/// Benchmark: memory monitor read
#[bench]
fn bench_memory_read(b: &mut Bencher) {
    // Read /proc/meminfo and parse
    // Expected: <10μs
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/admission.rs` | **NEW** — AdmissionController, AdmissionConfig, Priority |
| `deriva-network/src/adaptive_limiter.rs` | **NEW** — AdaptiveLimiter (Vegas) |
| `deriva-network/src/memory_monitor.rs` | **NEW** — MemoryMonitor |
| `deriva-network/src/lib.rs` | Add `pub mod admission`, `adaptive_limiter`, `memory_monitor` |
| `deriva-server/src/main.rs` | Create AdmissionController, wire into tonic server |
| `deriva-server/src/internal.rs` | Add priority extraction to each RPC handler |
| `deriva-server/src/state.rs` | Add `admission: Arc<AdmissionController>` |
| `deriva-network/tests/admission.rs` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

No new external dependencies. Uses existing `tokio`, `dashmap`, `thiserror`.

---

## 10. Design Rationale

### 10.1 Why Vegas Instead of Fixed Concurrency Limit?

```
Fixed limit:
  - Set too low → under-utilizes hardware during normal load
  - Set too high → doesn't protect during overload
  - Requires manual tuning per deployment

Vegas adaptive:
  - Starts at reasonable default (128)
  - Grows during low load → full utilization
  - Shrinks during overload → protection
  - Self-tuning, no operator intervention

  Trade-off: Vegas converges slowly (1/sec adjustment).
  But: semaphore provides immediate protection.
  Vegas adjusts the ceiling; semaphore enforces it instantly.
```

### 10.2 Why Per-Peer Limits Instead of Global Only?

```
Global limit only:
  One misbehaving peer sends 1000 requests.
  All 1000 admitted (under global limit of 1024).
  Other peers' requests rejected.
  → Unfair: one peer monopolizes the server.

Per-peer limit (64):
  Misbehaving peer: 64 admitted, rest rejected.
  Other peers: still have 960 slots available.
  → Fair: no single peer can starve others.

  Per-peer limit should be < max_concurrent / expected_peers.
  For 16-node cluster: 1024 / 16 = 64 per peer.
```

### 10.3 Why Priority Timeout Tiers Instead of Strict Preemption?

```
Strict preemption:
  HIGH request arrives → cancel a LOW request mid-execution.
  Complex: need cancellation points in every handler.
  Wasteful: partially-completed work is thrown away.

Timeout tiers:
  HIGH: wait up to 100ms for a slot (likely gets one as others complete)
  NORMAL: wait up to 50ms
  LOW: try_acquire only (instant reject if full)

  Effect: under overload, LOW is shed first (instant reject),
  NORMAL next (short timeout), HIGH last (longer timeout).
  No cancellation complexity. Simple and effective.
```

### 10.4 Why Memory Monitoring via /proc Instead of Allocator Hooks?

```
Allocator hooks (jemalloc stats):
  + Precise: exact bytes allocated by this process
  - Doesn't account for page cache, other processes
  - Requires jemalloc dependency

/proc/meminfo:
  + System-wide view: accounts for all memory pressure
  + No dependencies
  + Includes page cache pressure
  - Less precise for this process specifically

  For admission control, system-wide pressure matters more.
  If the OS is under memory pressure (from any cause),
  we should shed load regardless of our own allocation.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `admission_total` | Counter | `result={admitted,rejected}` | Request outcomes |
| `admission_rejected_reason` | Counter | `reason={overload,memory,peer_limit}` | Rejection breakdown |
| `admission_in_flight` | Gauge | — | Current in-flight requests |
| `admission_concurrency_limit` | Gauge | — | Current Vegas limit |
| `admission_queue_wait_ms` | Histogram | `priority` | Time waiting for permit |
| `admission_memory_usage` | Gauge | — | System memory fraction |
| `admission_peer_in_flight` | Gauge | `peer` | Per-peer in-flight (top-10) |
| `admission_vegas_queue_estimate` | Gauge | — | Vegas queue estimate |

### 11.2 Structured Logging

```rust
tracing::warn!(
    priority = ?priority,
    peer = ?peer_id,
    memory = format!("{:.1}%", mem_usage * 100.0),
    "request rejected: memory pressure"
);

tracing::warn!(
    peer = %pid,
    current = current,
    limit = self.config.per_peer_limit,
    "request rejected: peer limit exceeded"
);

tracing::debug!(
    old_limit = current,
    new_limit = new_limit,
    queue_estimate = format!("{:.1}", queue_estimate),
    avg_latency_ms = avg.as_millis(),
    min_latency_ms = min_lat.as_millis(),
    "vegas concurrency adjustment"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| High rejection rate | `admission_rejected` > 10% of total for 5 min | Warning |
| Memory pressure | `admission_memory_usage` > 0.85 for 2 min | Warning |
| Memory critical | `admission_memory_usage` > 0.95 | Critical |
| Concurrency limit bottomed | `admission_concurrency_limit` = min_limit for 5 min | Warning |
| Peer flooding | `admission_rejected_reason{reason=peer_limit}` > 100/min | Info |

---

## 12. Checklist

- [ ] Create `deriva-network/src/admission.rs`
- [ ] Implement `AdmissionController` with 3-stage check
- [ ] Implement `AdmissionPermit` with RAII release
- [ ] Implement priority-aware timeout tiers
- [ ] Create `deriva-network/src/adaptive_limiter.rs`
- [ ] Implement Vegas-style adaptive concurrency
- [ ] Implement adjustment loop (1s window)
- [ ] Create `deriva-network/src/memory_monitor.rs`
- [ ] Implement `/proc/meminfo` polling (Linux)
- [ ] Implement tiered memory shedding (threshold + critical)
- [ ] Implement per-peer fairness with DashMap counters
- [ ] Wire admission controller into tonic server
- [ ] Add priority extraction from gRPC metadata
- [ ] Map all RPCs to priority levels
- [ ] Write admission controller tests (4 tests)
- [ ] Write per-peer fairness tests (2 tests)
- [ ] Write memory pressure tests (2 tests)
- [ ] Write adaptive limiter tests (3 tests)
- [ ] Write integration tests (2 tests)
- [ ] Add metrics (8 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (5 alerts)
- [ ] Run benchmarks: fast path, contention, Vegas, memory read
- [ ] Config validation: min_limit >= 1, per_peer_limit >= 1
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
