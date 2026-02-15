# Section 2.5: Observability

> **Goal:** Add structured tracing (tokio-tracing) and Prometheus metrics to all
> critical paths: materialization, cache, DAG operations, gRPC RPCs.
>
> **Crate:** `deriva-server` (metrics endpoint, tracing subscriber), `deriva-compute` (instrumentation)
>
> **Estimated tests:** ~12
>
> **Commit message pattern:** `2.5: Observability — tracing spans + Prometheus metrics`

---

## 1. Problem Statement

Phase 1 has zero observability. The only insight into system behavior is the `Status` RPC
which returns 5 aggregate counters. There is no way to:

- Trace a single Get request through cache → DAG → compute → response
- Identify slow materializations or hot recipes
- Monitor cache hit rates over time (only instantaneous snapshot)
- Alert on error rates or latency spikes
- Profile which compute functions are expensive
- Understand concurrency behavior (how many parallel materializations?)

### What We Need

| Category | Tool | Purpose |
|----------|------|---------|
| **Tracing** | `tracing` + `tracing-subscriber` | Structured logs with spans, request correlation |
| **Metrics** | `prometheus` + `metrics` crate | Counters, histograms, gauges for dashboards/alerts |
| **Endpoint** | HTTP `/metrics` | Prometheus scrape target |

### Observability Layers

```
Layer 1: Tracing (per-request detail)
┌──────────────────────────────────────────────────────┐
│ GET caddr=0xabc123 [request_id=req-42]               │
│  ├─ cache_lookup: MISS (2μs)                         │
│  ├─ dag_resolve: 3 nodes (15μs)                      │
│  ├─ materialize input_1: cache HIT (1μs)             │
│  ├─ materialize input_2: compute (45ms)              │
│  │   ├─ function: transform/v1                       │
│  │   └─ output: 1024 bytes                           │
│  ├─ compute final: concat/v1 (2ms)                   │
│  └─ cache_put: 2048 bytes (5μs)                      │
│ Total: 47ms                                          │
└──────────────────────────────────────────────────────┘

Layer 2: Metrics (aggregate dashboards)
┌──────────────────────────────────────────────────────┐
│ deriva_get_duration_seconds{quantile="0.99"} 0.12    │
│ deriva_cache_hit_total 45231                         │
│ deriva_cache_miss_total 8921                         │
│ deriva_compute_duration_seconds{fn="transform/v1"}   │
│ deriva_dag_depth{quantile="0.50"} 3                  │
│ deriva_active_materializations 7                     │
└──────────────────────────────────────────────────────┘
```

---

## 2. Design

### 2.5.1 Tracing Architecture

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
│ gRPC Request │────▶│ tracing::span!   │────▶│ tracing-subscriber│
│ (tonic)      │     │ "rpc.get"        │     │                   │
└─────────────┘     │  ├─ "materialize" │     │ ┌───────────────┐ │
                     │  │  ├─ "cache"    │     │ │ fmt (stdout)  │ │
                     │  │  ├─ "compute"  │     │ ├───────────────┤ │
                     │  │  └─ "cache_put"│     │ │ json (file)   │ │
                     │  └─ "stream"      │     │ ├───────────────┤ │
                     └──────────────────┘     │ │ opentelemetry  │ │
                                               │ └───────────────┘ │
                                               └─────────────────┘
```

Use `tracing` crate with structured fields:

```rust
use tracing::{info_span, instrument, Instrument};

#[instrument(skip(self), fields(addr = %addr))]
pub fn materialize(&self, addr: CAddr) -> BoxFuture<'_, Result<Bytes>> {
    Box::pin(async move {
        let span = info_span!("cache_check", addr = %addr);
        let cached = self.cache.get(&addr).instrument(span).await;
        // ...
    })
}
```

### 2.5.2 Metrics Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────┐
│ AsyncExecutor   │────▶│ metrics crate    │────▶│ prometheus  │
│ SharedCache     │     │ (counters,       │     │ registry    │
│ DerivaService   │     │  histograms,     │     │             │
│ PersistentDag   │     │  gauges)         │     │ /metrics    │
└─────────────────┘     └──────────────────┘     │ endpoint    │
                                                  └─────────────┘
```

### 2.5.3 Metric Definitions

#### RPC Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `deriva_rpc_total` | Counter | `method`, `status` | Total RPC calls |
| `deriva_rpc_duration_seconds` | Histogram | `method` | RPC latency |
| `deriva_rpc_active` | Gauge | `method` | In-flight RPCs |

#### Materialization Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `deriva_materialize_total` | Counter | `result` (hit/miss/error) | Total materializations |
| `deriva_materialize_duration_seconds` | Histogram | — | End-to-end materialization time |
| `deriva_materialize_active` | Gauge | — | In-flight materializations |
| `deriva_materialize_depth` | Histogram | — | DAG depth of materialized addrs |

#### Cache Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `deriva_cache_hit_total` | Counter | — | Cache hits |
| `deriva_cache_miss_total` | Counter | — | Cache misses |
| `deriva_cache_eviction_total` | Counter | — | Evictions |
| `deriva_cache_size_bytes` | Gauge | — | Current cache size |
| `deriva_cache_entries` | Gauge | — | Current entry count |
| `deriva_cache_hit_rate` | Gauge | — | Rolling hit rate |

#### Compute Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `deriva_compute_duration_seconds` | Histogram | `function` | Per-function compute time |
| `deriva_compute_input_bytes` | Histogram | `function` | Input size per compute |
| `deriva_compute_output_bytes` | Histogram | `function` | Output size per compute |

#### Verification Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `deriva_verification_total` | Counter | `result` (pass/fail) | Verification outcomes |
| `deriva_verification_failure_rate` | Gauge | — | Rolling failure rate |

#### DAG Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `deriva_dag_recipes` | Gauge | — | Total recipes in DAG |
| `deriva_dag_insert_duration_seconds` | Histogram | — | DAG insert latency |

#### Storage Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `deriva_storage_blobs` | Gauge | — | Total blobs |
| `deriva_storage_blob_bytes` | Gauge | — | Total blob storage |

---

## 3. Implementation

### 3.1 Metrics Registry: `deriva-server/src/metrics.rs`

```rust
use prometheus::{
    register_counter_vec, register_gauge, register_gauge_vec,
    register_histogram, register_histogram_vec,
    CounterVec, Gauge, GaugeVec, Histogram, HistogramVec,
    Encoder, TextEncoder,
};
use lazy_static::lazy_static;

lazy_static! {
    // RPC metrics
    pub static ref RPC_TOTAL: CounterVec = register_counter_vec!(
        "deriva_rpc_total", "Total RPC calls", &["method", "status"]
    ).unwrap();
    pub static ref RPC_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_rpc_duration_seconds", "RPC latency",
        &["method"],
        vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]
    ).unwrap();
    pub static ref RPC_ACTIVE: GaugeVec = register_gauge_vec!(
        "deriva_rpc_active", "In-flight RPCs", &["method"]
    ).unwrap();

    // Materialization metrics
    pub static ref MAT_TOTAL: CounterVec = register_counter_vec!(
        "deriva_materialize_total", "Total materializations", &["result"]
    ).unwrap();
    pub static ref MAT_DURATION: Histogram = register_histogram!(
        "deriva_materialize_duration_seconds", "Materialization latency",
        vec![0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 30.0]
    ).unwrap();
    pub static ref MAT_ACTIVE: Gauge = register_gauge!(
        "deriva_materialize_active", "In-flight materializations"
    ).unwrap();

    // Cache metrics
    pub static ref CACHE_HIT: CounterVec = register_counter_vec!(
        "deriva_cache_total", "Cache operations", &["result"]
    ).unwrap();
    pub static ref CACHE_SIZE: Gauge = register_gauge!(
        "deriva_cache_size_bytes", "Current cache size"
    ).unwrap();
    pub static ref CACHE_ENTRIES: Gauge = register_gauge!(
        "deriva_cache_entries", "Current cache entries"
    ).unwrap();

    // Compute metrics
    pub static ref COMPUTE_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_compute_duration_seconds", "Compute function latency",
        &["function"],
        vec![0.0001, 0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]
    ).unwrap();

    // Verification metrics
    pub static ref VERIFY_TOTAL: CounterVec = register_counter_vec!(
        "deriva_verification_total", "Verification outcomes", &["result"]
    ).unwrap();

    // DAG metrics
    pub static ref DAG_RECIPES: Gauge = register_gauge!(
        "deriva_dag_recipes", "Total recipes in DAG"
    ).unwrap();
}

pub fn encode_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}
```

### 3.2 Metrics HTTP Endpoint

Expose `/metrics` on a separate HTTP port (default: 9090) for Prometheus scraping:

```rust
// deriva-server/src/metrics_server.rs

use axum::{routing::get, Router};
use std::net::SocketAddr;

async fn metrics_handler() -> String {
    super::metrics::encode_metrics()
}

pub async fn start_metrics_server(port: u16) {
    let app = Router::new().route("/metrics", get(metrics_handler));
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Metrics server listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
```

### 3.3 Tracing Subscriber Setup

```rust
// deriva-server/src/main.rs — add tracing init

use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("deriva=info")))
        .with(fmt::layer()
            .with_target(true)
            .with_span_events(fmt::format::FmtSpan::CLOSE)
            .with_timer(fmt::time::uptime()))
        .init();
}
```

Log output example:

```
0.001s INFO  deriva_server::service: rpc.put_leaf addr=caddr(0xabc123...)
0.002s INFO  deriva_server::service: rpc.put_recipe addr=caddr(0xdef456...) inputs=2
0.015s INFO  deriva_compute::async_executor: materialize addr=caddr(0xdef456...)
0.015s DEBUG deriva_compute::async_executor:   cache_check: MISS
0.016s DEBUG deriva_compute::async_executor:   resolve_inputs: 2 inputs
0.016s INFO  deriva_compute::async_executor:   materialize addr=caddr(0x111...) [input 1/2]
0.016s DEBUG deriva_compute::async_executor:     cache_check: HIT
0.017s INFO  deriva_compute::async_executor:   materialize addr=caddr(0x222...) [input 2/2]
0.017s DEBUG deriva_compute::async_executor:     cache_check: MISS
0.017s DEBUG deriva_compute::async_executor:     leaf_check: HIT
0.018s INFO  deriva_compute::async_executor:   compute fn=concat/v1 inputs=2048B output=4096B (1.2ms)
0.019s INFO  deriva_compute::async_executor:   cache_put: 4096 bytes
0.019s INFO  deriva_server::service: rpc.get complete addr=caddr(0xdef456...) (4.1ms)
```

### 3.4 Instrumenting AsyncExecutor

```rust
// In async_executor.rs — add instrumentation to materialize()

pub fn materialize(&self, addr: CAddr) -> BoxFuture<'_, Result<Bytes>> {
    Box::pin(async move {
        let _active = MAT_ACTIVE_GUARD::new(); // increment on create, decrement on drop
        let start = std::time::Instant::now();
        let span = tracing::info_span!("materialize", addr = %addr);

        let result = async {
            // 1. Cache check
            if let Some(bytes) = self.cache.get(&addr).await {
                CACHE_HIT.with_label_values(&["hit"]).inc();
                MAT_TOTAL.with_label_values(&["cache_hit"]).inc();
                tracing::debug!("cache hit");
                return Ok(bytes);
            }
            CACHE_HIT.with_label_values(&["miss"]).inc();

            // 2. Leaf check
            if let Some(bytes) = self.leaf_store.get_leaf(&addr).await {
                MAT_TOTAL.with_label_values(&["leaf"]).inc();
                tracing::debug!("leaf hit");
                return Ok(bytes);
            }

            // 3-8. Recipe lookup, parallel resolve, compute, cache put
            // ... (same as §2.3, with tracing::debug! at each step)

            // After compute:
            COMPUTE_DURATION
                .with_label_values(&[&recipe.function_id.to_string()])
                .observe(compute_elapsed.as_secs_f64());
            MAT_TOTAL.with_label_values(&["computed"]).inc();

            Ok(output)
        }
        .instrument(span)
        .await;

        MAT_DURATION.observe(start.elapsed().as_secs_f64());
        result
    })
}
```

### 3.5 Instrumenting DerivaService RPCs

```rust
// Macro to reduce boilerplate for RPC instrumentation
macro_rules! instrument_rpc {
    ($method:expr, $body:expr) => {{
        let method = $method;
        RPC_ACTIVE.with_label_values(&[method]).inc();
        let start = std::time::Instant::now();
        let result = $body;
        let status = if result.is_ok() { "ok" } else { "error" };
        RPC_TOTAL.with_label_values(&[method, status]).inc();
        RPC_DURATION.with_label_values(&[method]).observe(start.elapsed().as_secs_f64());
        RPC_ACTIVE.with_label_values(&[method]).dec();
        result
    }};
}

// Usage in service.rs:
async fn get(&self, request: Request<GetRequest>) -> Result<Response<Self::GetStream>, Status> {
    instrument_rpc!("get", {
        // ... existing implementation
    })
}
```

---

## 4. Data Flow Diagrams

### 4.1 Request Tracing Flow

```
Client: deriva get 0xabc123
    │
    ▼
┌─────────────────────────────────────────────────────────┐
│ DerivaService::get()                                     │
│ span: rpc.get { method="get", addr="0xabc123" }         │
│                                                          │
│  ├─ RPC_ACTIVE["get"].inc()                              │
│  │                                                       │
│  ├─ AsyncExecutor::materialize(0xabc123)                 │
│  │  span: materialize { addr="0xabc123" }                │
│  │  │                                                    │
│  │  ├─ cache.get() → MISS                                │
│  │  │  CACHE_HIT["miss"].inc()                           │
│  │  │                                                    │
│  │  ├─ dag.get_recipe() → Recipe(concat, [0x111, 0x222]) │
│  │  │                                                    │
│  │  ├─ try_join_all:                                     │
│  │  │  ├─ materialize(0x111) → cache HIT (1μs)           │
│  │  │  │  CACHE_HIT["hit"].inc()                         │
│  │  │  └─ materialize(0x222) → leaf HIT (3μs)            │
│  │  │                                                    │
│  │  ├─ spawn_blocking(concat.execute(...))               │
│  │  │  COMPUTE_DURATION["concat/v1"].observe(0.002)      │
│  │  │                                                    │
│  │  └─ cache.put(0xabc123, result)                       │
│  │     CACHE_SIZE.set(new_size)                          │
│  │                                                       │
│  ├─ stream 64KB chunks                                   │
│  │                                                       │
│  ├─ RPC_TOTAL["get","ok"].inc()                          │
│  ├─ RPC_DURATION["get"].observe(0.047)                   │
│  └─ RPC_ACTIVE["get"].dec()                              │
└─────────────────────────────────────────────────────────┘
```

### 4.2 Prometheus Scrape Flow

```
┌──────────────┐     GET /metrics      ┌──────────────────┐
│  Prometheus  │──────────────────────▶│ Metrics Server   │
│  (scraper)   │                       │ (axum, port 9090)│
│              │◀──────────────────────│                  │
│              │  text/plain response   │ prometheus::gather│
└──────────────┘                       └──────────────────┘
       │
       ▼
┌──────────────┐
│  Grafana     │  Dashboard with panels:
│  Dashboard   │  - RPC latency p50/p95/p99
│              │  - Cache hit rate over time
│              │  - Active materializations
│              │  - Compute time by function
│              │  - Verification failure rate
└──────────────┘
```

### 4.3 Grafana Dashboard Layout

```
┌─────────────────────────────────────────────────────────────┐
│                    Deriva Dashboard                          │
├──────────────────────┬──────────────────────────────────────┤
│ RPC Rate             │ RPC Latency (p50/p95/p99)            │
│ ┌──────────────────┐ │ ┌──────────────────────────────────┐ │
│ │ ▄▄▄▄▄▄▄▄▄▄▄▄▄▄  │ │ │ p99: ─── 120ms                 │ │
│ │ get: 150 req/s   │ │ │ p95: ─── 45ms                  │ │
│ │ put: 20 req/s    │ │ │ p50: ─── 12ms                  │ │
│ └──────────────────┘ │ └──────────────────────────────────┘ │
├──────────────────────┼──────────────────────────────────────┤
│ Cache Hit Rate       │ Cache Size                           │
│ ┌──────────────────┐ │ ┌──────────────────────────────────┐ │
│ │ ████████████░░░  │ │ │ 450MB / 1024MB                  │ │
│ │ 83.5%            │ │ │ 12,345 entries                  │ │
│ └──────────────────┘ │ └──────────────────────────────────┘ │
├──────────────────────┼──────────────────────────────────────┤
│ Active Materializations │ Compute Time by Function          │
│ ┌──────────────────┐ │ ┌──────────────────────────────────┐ │
│ │ ▂▃▅▇▅▃▂▁▂▃▅▇▅▃  │ │ │ transform/v1: p99=120ms        │ │
│ │ current: 7       │ │ │ concat/v1:    p99=2ms          │ │
│ │ peak: 23         │ │ │ identity/v1:  p99=0.1ms        │ │
│ └──────────────────┘ │ └──────────────────────────────────┘ │
├──────────────────────┴──────────────────────────────────────┤
│ Verification: 10,234 verified | 0 failures | rate: 0.00%   │
└─────────────────────────────────────────────────────────────┘
```

---

## 5. Test Specification

### 5.1 Unit Tests

```
#[test]
test_metrics_registry_initializes
    Access all lazy_static metrics
    Assert no panic (registration succeeds)

#[test]
test_metrics_encode_produces_valid_prometheus_format
    Increment some counters
    let output = encode_metrics();
    assert!(output.contains("deriva_rpc_total"));
    assert!(output.contains("deriva_cache_total"));

#[tokio::test]
test_materialize_increments_cache_hit_counter
    Pre-populate cache, materialize (cache hit)
    Assert CACHE_HIT["hit"] incremented

#[tokio::test]
test_materialize_increments_cache_miss_counter
    Materialize uncached recipe
    Assert CACHE_HIT["miss"] incremented

#[tokio::test]
test_materialize_records_duration_histogram
    Materialize a recipe
    Assert MAT_DURATION has at least 1 observation

#[tokio::test]
test_compute_duration_labeled_by_function
    Materialize recipe using "concat/v1"
    Assert COMPUTE_DURATION["concat/v1"] has observation

#[tokio::test]
test_rpc_total_incremented_on_success
    Call get RPC successfully
    Assert RPC_TOTAL["get","ok"] incremented

#[tokio::test]
test_rpc_total_incremented_on_error
    Call get RPC with invalid addr
    Assert RPC_TOTAL["get","error"] incremented

#[tokio::test]
test_active_materializations_gauge
    Start long materialization (SlowFunction)
    Assert MAT_ACTIVE > 0 during compute
    Wait for completion
    Assert MAT_ACTIVE == 0

#[tokio::test]
test_tracing_span_created
    Use tracing_test::traced_test or tracing-subscriber with in-memory layer
    Materialize a recipe
    Assert span "materialize" was created with addr field

#[tokio::test]
test_metrics_endpoint_returns_200
    Start metrics server on random port
    HTTP GET /metrics
    Assert status 200
    Assert body contains "deriva_"
```

### 5.2 Integration Tests

```
#[tokio::test]
test_full_observability_flow
    Start server with metrics endpoint
    Put leaf + recipe, Get recipe
    Scrape /metrics
    Assert all expected metric families present
    Assert rpc_total > 0
    Assert cache counters > 0
```

---

## 6. Edge Cases & Error Handling

| Case | Expected Behavior |
|------|-------------------|
| Prometheus registry full | Should not happen (static metrics), but log warning |
| Metrics endpoint under heavy load | axum handles concurrently, no blocking |
| Tracing subscriber not initialized | `tracing` macros are no-ops — no crash |
| Very high cardinality labels | Only `function` label varies — bounded by registry size |
| Metrics server port conflict | Error on startup with clear message |
| Long-running materialization | `MAT_ACTIVE` gauge stays elevated — visible in dashboard |

---

## 7. Performance Impact

| Component | Overhead | Notes |
|-----------|----------|-------|
| Counter increment | ~20ns | Atomic operation |
| Histogram observe | ~50ns | Atomic + bucket search |
| Gauge set | ~20ns | Atomic operation |
| tracing span create | ~100ns | Allocation + field formatting |
| tracing span close | ~50ns | Duration calculation |
| /metrics encode | ~1ms for 50 metrics | Only on scrape (every 15s) |
| Total per-request | ~500ns | Negligible vs compute time |

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-server/src/metrics.rs` | **NEW** — Prometheus metric definitions + encode |
| `deriva-server/src/metrics_server.rs` | **NEW** — axum HTTP server for /metrics |
| `deriva-server/src/main.rs` | Init tracing subscriber, start metrics server |
| `deriva-server/src/service.rs` | Add `instrument_rpc!` macro, instrument all RPCs |
| `deriva-compute/src/async_executor.rs` | Add tracing spans + metric increments |
| `deriva-compute/src/cache.rs` | Add cache metric updates in SharedCache |
| `deriva-server/Cargo.toml` | Add `prometheus`, `lazy_static`, `axum`, `tracing`, `tracing-subscriber` |
| `deriva-compute/Cargo.toml` | Add `tracing` |
| `deriva-server/tests/observability.rs` | **NEW** — ~12 tests |

---

## 9. Dependency Changes

```toml
# deriva-server/Cargo.toml
[dependencies]
prometheus = "0.13"
lazy_static = "1"
axum = "0.7"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "json"] }

# deriva-compute/Cargo.toml
[dependencies]
tracing = "0.1"
```

---

## 10. Design Rationale

### 10.1 Why Prometheus Instead of OpenTelemetry Metrics?

| Factor | Prometheus | OpenTelemetry |
|--------|-----------|---------------|
| Maturity | Battle-tested | Newer, still evolving |
| Rust ecosystem | `prometheus` crate is stable | `opentelemetry` crate works but more complex |
| Pull vs Push | Pull (simpler infra) | Push (needs collector) |
| Grafana integration | Native | Via OTLP → Prometheus adapter |
| Dependency weight | ~2 crates | ~10+ crates |

Prometheus is simpler for a single-node system. When we go distributed (Phase 3),
we can add OpenTelemetry as an additional exporter without removing Prometheus.

### 10.2 Why Separate Metrics Port?

Running metrics on a separate port (9090) from gRPC (50051):
- Prometheus scraper doesn't interfere with gRPC traffic
- Can firewall metrics port separately (internal-only)
- Standard convention (Prometheus default port is 9090)
- No need to add HTTP routing to the gRPC server

### 10.3 Why `lazy_static` Instead of `once_cell`?

The `prometheus` crate's `register_*` macros work naturally with `lazy_static`.
`once_cell::sync::Lazy` would also work but requires slightly more boilerplate.
Either is fine — `lazy_static` is more conventional with `prometheus`.

### 10.4 Why Not Instrument Every sled Read?

Sled reads are ~1-5μs. Adding a histogram observation (~50ns) to every sled read
would add ~5% overhead to sled operations. Instead, we instrument at the
materialization level (which includes sled reads) and at the DAG insert level.

If sled performance becomes a concern, add a `SLED_READ_DURATION` histogram
behind a feature flag.

---

## 11. Example Prometheus Queries

```promql
# Request rate by method
rate(deriva_rpc_total[5m])

# p99 latency for Get RPCs
histogram_quantile(0.99, rate(deriva_rpc_duration_seconds_bucket{method="get"}[5m]))

# Cache hit rate over time
rate(deriva_cache_total{result="hit"}[5m]) /
(rate(deriva_cache_total{result="hit"}[5m]) + rate(deriva_cache_total{result="miss"}[5m]))

# Slowest compute functions (p95)
histogram_quantile(0.95, rate(deriva_compute_duration_seconds_bucket[5m]))

# Active materializations (current)
deriva_materialize_active

# Verification failure rate
rate(deriva_verification_total{result="fail"}[5m]) /
rate(deriva_verification_total[5m])
```

---

## 12. Alerting Rules

```yaml
# prometheus/alerts.yml
groups:
  - name: deriva
    rules:
      - alert: HighErrorRate
        expr: rate(deriva_rpc_total{status="error"}[5m]) > 0.05
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Deriva error rate above 5%"

      - alert: CacheHitRateLow
        expr: |
          rate(deriva_cache_total{result="hit"}[10m]) /
          (rate(deriva_cache_total{result="hit"}[10m]) + rate(deriva_cache_total{result="miss"}[10m]))
          < 0.5
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Cache hit rate below 50%"

      - alert: DeterminismViolation
        expr: increase(deriva_verification_total{result="fail"}[1h]) > 0
        labels:
          severity: critical
        annotations:
          summary: "Non-deterministic compute function detected!"

      - alert: HighLatency
        expr: histogram_quantile(0.99, rate(deriva_rpc_duration_seconds_bucket{method="get"}[5m])) > 5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "p99 Get latency above 5 seconds"
```

---

## 13. Structured JSON Logging

For production deployments, switch from human-readable to JSON logs:

```rust
fn init_tracing(json: bool) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("deriva=info"));

    if json {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json()
                .with_target(true)
                .with_span_events(fmt::format::FmtSpan::CLOSE))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer()
                .with_target(true)
                .with_span_events(fmt::format::FmtSpan::CLOSE)
                .with_timer(fmt::time::uptime()))
            .init();
    }
}
```

JSON log output:

```json
{
  "timestamp": "2026-02-14T01:43:46.351Z",
  "level": "INFO",
  "target": "deriva_compute::async_executor",
  "span": {
    "name": "materialize",
    "addr": "caddr(0xabc123...)"
  },
  "fields": {
    "message": "compute complete",
    "function": "concat/v1",
    "input_bytes": 2048,
    "output_bytes": 4096,
    "duration_ms": 1.2
  }
}
```

This integrates with log aggregation systems (CloudWatch Logs, ELK, Loki).

### 13.1 Log Levels Guide

| Level | What to log | Example |
|-------|-------------|---------|
| ERROR | Unrecoverable failures | `DeterminismViolation`, sled corruption |
| WARN | Recoverable issues | Cache eviction under pressure, slow compute (>1s) |
| INFO | Request lifecycle | RPC start/end, materialization complete |
| DEBUG | Internal decisions | Cache hit/miss, DAG traversal steps |
| TRACE | Everything | Individual sled reads, byte counts |

### 13.2 Server CLI Flags

```bash
# Human-readable logs (default)
deriva-server --log-format text

# JSON logs (production)
deriva-server --log-format json

# Verbose debugging
RUST_LOG=deriva=debug deriva-server

# Trace a specific crate
RUST_LOG=deriva_compute=trace,deriva_server=info deriva-server

# Metrics on custom port
deriva-server --metrics-port 9091
```

---

## 14. Docker Compose Monitoring Stack

For local development, provide a docker-compose that runs Prometheus + Grafana:

```yaml
# docker-compose.monitoring.yml
version: "3.8"

services:
  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./monitoring/prometheus.yml:/etc/prometheus/prometheus.yml
    extra_hosts:
      - "host.docker.internal:host-gateway"

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    volumes:
      - ./monitoring/grafana/dashboards:/var/lib/grafana/dashboards
      - ./monitoring/grafana/provisioning:/etc/grafana/provisioning
```

```yaml
# monitoring/prometheus.yml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: "deriva"
    static_configs:
      - targets: ["host.docker.internal:9090"]
```

Usage:

```bash
# Start Deriva server
deriva-server --metrics-port 9090 &

# Start monitoring stack
docker-compose -f docker-compose.monitoring.yml up -d

# Open Grafana at http://localhost:3000 (admin/admin)
# Import Deriva dashboard from monitoring/grafana/dashboards/deriva.json
```

---

## 15. Health Check Endpoint

Add a `/health` endpoint alongside `/metrics` for load balancer integration:

```rust
// In metrics_server.rs

async fn health_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let cache_ok = state.cache.entry_count().await >= 0; // always true, but exercises the lock
    let dag_ok = state.dag.len() >= 0; // exercises sled

    if cache_ok && dag_ok {
        (StatusCode::OK, Json(serde_json::json!({
            "status": "healthy",
            "cache_entries": state.cache.entry_count().await,
            "dag_recipes": state.dag.len(),
            "uptime_seconds": state.start_time.elapsed().as_secs(),
        })))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
            "status": "unhealthy",
            "cache_ok": cache_ok,
            "dag_ok": dag_ok,
        })))
    }
}

pub async fn start_metrics_server(port: u16, state: Arc<ServerState>) {
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .with_state(state);
    // ...
}
```

Usage:

```bash
# Kubernetes liveness probe
livenessProbe:
  httpGet:
    path: /health
    port: 9090
  initialDelaySeconds: 5
  periodSeconds: 10

# Docker healthcheck
HEALTHCHECK --interval=10s --timeout=3s \
  CMD curl -f http://localhost:9090/health || exit 1
```

Response:

```json
{
  "status": "healthy",
  "cache_entries": 1234,
  "dag_recipes": 5678,
  "uptime_seconds": 3600
}
```

---

## 16. Request ID Correlation

Add a unique request ID to each RPC for end-to-end tracing:

```rust
use uuid::Uuid;

async fn get(&self, request: Request<GetRequest>) -> Result<Response<Self::GetStream>, Status> {
    let request_id = Uuid::new_v4().to_string();
    let span = tracing::info_span!("rpc.get",
        request_id = %request_id,
        addr = %hex::encode(&request.get_ref().addr),
    );

    async move {
        tracing::info!("request started");
        // ... handle request ...
        tracing::info!("request complete");
    }
    .instrument(span)
    .await
}
```

The `request_id` propagates through all child spans, allowing correlation of
all log lines for a single request:

```
grep "request_id=abc-123" logs.json | jq .
```

This is essential for debugging in production when multiple requests are interleaved.

---

## 16. Checklist

- [ ] Create `metrics.rs` with all Prometheus metric definitions
- [ ] Create `metrics_server.rs` with axum `/metrics` endpoint
- [ ] Initialize `tracing-subscriber` in `main.rs` (text + JSON modes)
- [ ] Add `instrument_rpc!` macro to `service.rs`
- [ ] Instrument all 6 RPCs with metrics
- [ ] Add tracing spans to `AsyncExecutor::materialize`
- [ ] Add cache hit/miss counters to `SharedCache`
- [ ] Add compute duration histogram with function label
- [ ] Add verification counters
- [ ] Add request ID correlation via UUID
- [ ] Add `--metrics-port` and `--log-format` CLI flags
- [ ] Create `docker-compose.monitoring.yml`
- [ ] Create `monitoring/prometheus.yml`
- [ ] Write unit tests (~10)
- [ ] Write integration test (~2)
- [ ] Create example Grafana dashboard JSON (optional)
- [ ] Create example alerting rules (optional)
- [ ] Run full test suite
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
