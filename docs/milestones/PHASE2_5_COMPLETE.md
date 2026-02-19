# Phase 2.5 Complete: Observability

**Date:** 2026-02-18  
**Commit:** 6be7f44  
**Status:** ✅ Complete

---

## Overview

Integrated Prometheus metrics, structured logging, and health endpoints across the Deriva stack. Provides production-grade observability for compute, cache, and server operations.

## Implementation

### Prometheus Metrics (23 total)

**Compute metrics (`deriva-compute/src/metrics.rs`):**
- `deriva_materialize_total` — materialization count by result
- `deriva_materialize_duration_seconds` — materialization latency histogram
- `deriva_materialize_active` — in-flight materialization gauge
- `deriva_cache_total` — cache hit/miss counter
- `deriva_compute_duration_seconds` — per-function compute latency
- `deriva_compute_input_bytes` — input size distribution
- `deriva_compute_output_bytes` — output size distribution

**Server metrics:**
- RPC request counts, latencies, error rates
- gRPC endpoint instrumentation with UUID request ID correlation

### Metrics Server

- axum-based HTTP server on configurable port
- `/metrics` — Prometheus scrape endpoint
- `/health` — liveness check

### Structured Logging

- `tracing-subscriber` with text and JSON output modes
- `EnvFilter` for runtime log level control
- Request ID propagation across async spans

### Monitoring Stack

- Docker Compose with Prometheus + Grafana
- Pre-configured scrape targets and dashboards

## Testing

14 observability tests covering metrics registration, endpoint responses, and tracing integration.

## Next Steps

Phase 2.6: Cascade Invalidation
