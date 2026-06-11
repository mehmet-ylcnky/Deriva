# Implementation Plan: Observability

## Overview

This plan completes the production-grade observability implementation for Deriva by filling gaps in metric instrumentation, adding missing metric definitions, wiring gauge updates into cache/DAG/storage mutation paths, implementing the monitoring stack (Docker Compose + Grafana), and covering all 12 correctness properties with property-based tests. The project uses Rust with `prometheus`, `tokio-tracing`, and `proptest`.

## Tasks

- [ ] 1. Complete metric registration and cache eviction instrumentation
  - [ ] 1.1 Register missing `CACHE_EVICTION_TOTAL` counter in `crates/deriva-server/src/metrics.rs`
    - Add `register_int_counter!("deriva_cache_eviction_total", "Total cache evictions")` to the `lazy_static!` block
    - Re-export from `crates/deriva-compute/src/metrics.rs` if instrumentation lives in compute crate
    - _Requirements: 1.5, 5.3_

  - [ ] 1.2 Instrument cache eviction path to increment `CACHE_EVICTION_TOTAL`
    - In `crates/deriva-compute/src/cache.rs`, increment the counter whenever an entry is evicted
    - Update `CACHE_SIZE` and `CACHE_ENTRIES` gauges on every put/evict operation
    - _Requirements: 5.3, 5.4, 5.5_

  - [ ] 1.3 Implement rolling cache hit rate gauge update
    - Track total hits and misses in `SharedCache` and update `CACHE_HIT_RATE` gauge after each lookup
    - Formula: `hits / (hits + misses)` using atomics for thread safety
    - _Requirements: 5.6_

  - [ ]* 1.4 Write property test for cache gauge consistency (Property 4)
    - **Property 4: Cache gauge consistency**
    - For any sequence of cache put and eviction operations, `CACHE_SIZE` equals actual total bytes and `CACHE_ENTRIES` equals actual entry count
    - **Validates: Requirements 5.4, 5.5**

  - [ ]* 1.5 Write property test for cache hit rate consistency (Property 5)
    - **Property 5: Cache hit rate consistency**
    - For any sequence of cache lookups with H hits and M misses (H+M > 0), `CACHE_HIT_RATE` equals H/(H+M)
    - **Validates: Requirements 5.6**

- [ ] 2. Wire DAG, storage, and verification gauge updates
  - [ ] 2.1 Instrument DAG recipe insertion with gauge and duration
    - In the DAG `put_recipe` path, update `DAG_RECIPES` gauge to `dag.len()` after insertion
    - Observe insertion duration in `DAG_INSERT_DURATION`
    - _Requirements: 7.4, 7.5_

  - [ ] 2.2 Instrument storage blob operations with gauge updates
    - After blob put/delete in `crates/deriva-storage`, update `STORAGE_BLOBS` and `STORAGE_BLOB_BYTES` gauges
    - _Requirements: 8.1, 8.2_

  - [ ] 2.3 Instrument verification pass/fail counters and failure rate
    - In the verification path within `AsyncExecutor`, increment `VERIFY_TOTAL` with result="pass" or "fail"
    - Update `VERIFY_FAILURE_RATE` gauge as rolling ratio
    - _Requirements: 7.1, 7.2, 7.3_

  - [ ]* 2.4 Write property test for DAG recipe gauge consistency (Property 7)
    - **Property 7: DAG recipe gauge consistency**
    - For any sequence of recipe insertions, `DAG_RECIPES` gauge equals actual `dag.len()`
    - **Validates: Requirements 7.4**

  - [ ]* 2.5 Write property test for storage gauge consistency (Property 8)
    - **Property 8: Storage gauge consistency**
    - For any sequence of blob operations, `STORAGE_BLOBS` equals actual count and `STORAGE_BLOB_BYTES` equals actual total size
    - **Validates: Requirements 8.1, 8.2**

- [ ] 3. Complete materialization metrics instrumentation
  - [ ] 3.1 Instrument materialization depth recording
    - In `AsyncExecutor::materialize`, track DAG depth during recursive resolution
    - Observe depth in `MAT_DEPTH` histogram upon materialization completion
    - _Requirements: 4.6_

  - [ ] 3.2 Add structured tracing spans and events to materialization
    - Ensure `info_span!("materialize", addr = %addr)` is created at entry
    - Emit DEBUG events for cache hit/miss decisions
    - Emit INFO event on compute completion with function name, input_bytes, output_bytes, duration
    - _Requirements: 11.1, 11.2, 11.3_

  - [ ]* 3.3 Write property test for materialization outcome counter correctness (Property 2)
    - **Property 2: Materialization outcome counter correctness**
    - For any materialization, `MAT_TOTAL` is incremented exactly once with correct result label (hit/miss/error)
    - **Validates: Requirements 4.1, 4.2, 4.3**

  - [ ]* 3.4 Write property test for materialization active gauge balance (Property 3)
    - **Property 3: Materialization active gauge balance**
    - For any materialization (hit, compute, or error), `MAT_ACTIVE` returns to pre-invocation value after completion
    - **Validates: Requirements 4.4**

  - [ ]* 3.5 Write property test for materialization span structure (Property 12)
    - **Property 12: Materialization span structure**
    - For any materialization of address A, a span "materialize" is created with field `addr` and a DEBUG event for cache hit/miss is emitted
    - **Validates: Requirements 11.1, 11.2**

- [ ] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 5. Complete RPC instrumentation and request ID generation
  - [ ] 5.1 Ensure all RPC methods use `begin_rpc`/`record_rpc` pattern
    - Verify that `resolve`, `invalidate`, `cascade_invalidate`, `garbage_collect`, `pin`, `unpin`, `list_pins` all call `begin_rpc`/`record_rpc`
    - Add instrumentation to any methods that are missing it
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

  - [ ]* 5.2 Write property test for RPC instrumentation invariant (Property 1)
    - **Property 1: RPC instrumentation invariant**
    - For any RPC invocation, `begin_rpc`/`record_rpc` increments `rpc_total` exactly once, observes duration, and leaves `rpc_active` balanced
    - **Validates: Requirements 3.1, 3.2, 3.4, 3.5**

  - [ ]* 5.3 Write property test for request ID UUID v4 format (Property 10)
    - **Property 10: Request ID is valid UUID v4**
    - For any RPC invocation, the `request_id` is a valid v4 UUID matching `^[0-9a-f]{8}-...-[0-9a-f]{12}$`
    - **Validates: Requirements 10.1, 10.3**

- [ ] 6. Compute metrics instrumentation and property tests
  - [ ] 6.1 Verify compute function metrics are fully wired
    - Confirm `COMPUTE_DURATION`, `COMPUTE_INPUT_BYTES`, `COMPUTE_OUTPUT_BYTES` are observed with correct function label on every compute execution
    - Fix any gaps in the instrumentation path
    - _Requirements: 6.1, 6.2, 6.3_

  - [ ]* 6.2 Write property test for compute metrics completeness (Property 6)
    - **Property 6: Compute metrics completeness**
    - For any compute execution with function F, input size I, output size O, the system observes I in `compute_input_bytes[F]`, O in `compute_output_bytes[F]`, and duration in `compute_duration_seconds[F]`
    - **Validates: Requirements 6.1, 6.2, 6.3**

- [ ] 7. Metrics endpoint and tracing property tests
  - [ ] 7.1 Add error handling for metrics port conflict
    - In `start_metrics_server`, catch `TcpListener::bind` failure, log a descriptive error, and terminate the process
    - _Requirements: 2.4_

  - [ ]* 7.2 Write property test for metrics encode round-trip (Property 9)
    - **Property 9: Metrics encode round-trip**
    - For any sequence of metric operations, `encode_metrics()` produces valid Prometheus text exposition format containing all registered metric family names
    - **Validates: Requirements 2.2**

  - [ ]* 7.3 Write property test for JSON log structural validity (Property 11)
    - **Property 11: JSON log structural validity**
    - For any log event emitted with JSON format, the output is valid JSON containing "timestamp", "level", and "target" fields
    - **Validates: Requirements 9.2**

- [ ] 8. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 9. Monitoring stack and Docker Compose setup
  - [ ] 9.1 Create Docker Compose monitoring configuration
    - Create `docker-compose.monitoring.yml` with Prometheus and Grafana services
    - Create/update `monitoring/prometheus.yml` with scrape config targeting `host.docker.internal:9090` at 15s interval
    - _Requirements: 14.1, 14.2_

  - [ ] 9.2 Create Grafana dashboard and provisioning configuration
    - Create `monitoring/grafana/provisioning/datasources/prometheus.yml` for auto-provisioned Prometheus datasource
    - Create `monitoring/grafana/provisioning/dashboards/dashboard.yml` for auto-provisioned dashboard
    - Create `monitoring/grafana/dashboards/deriva.json` with panels for: RPC rate, RPC latency percentiles, cache hit rate, cache size, active materializations, compute duration by function
    - _Requirements: 14.3_

- [ ] 10. Unit and integration tests
  - [ ]* 10.1 Write unit tests for metrics registration and health endpoint
    - `test_metrics_registry_initializes`: all 23 metrics accessible without panic
    - `test_health_endpoint_returns_200`: /health returns JSON with status field
    - `test_metrics_port_conflict`: proper error on port-in-use
    - `test_env_filter_default`: default filter is `deriva=info`
    - _Requirements: 1.1, 1.2, 2.3, 2.4, 9.4_

  - [ ]* 10.2 Write integration tests for full observability flow
    - `test_full_observability_flow`: put leaf + recipe, get, scrape /metrics, verify all metric families present
    - `test_json_log_integration`: full request produces valid JSON log stream
    - `test_all_rpcs_instrumented`: each RPC method increments rpc_total
    - _Requirements: 2.2, 3.3, 9.2_

- [ ] 11. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The project already has substantial metric definitions in `crates/deriva-server/src/metrics.rs` and `crates/deriva-compute/src/metrics.rs`; tasks focus on completing instrumentation and adding missing definitions
- `proptest` is already a workspace dependency

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "5.1", "6.1", "7.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "2.1", "2.2", "2.3", "3.1", "3.2"] },
    { "id": 2, "tasks": ["1.4", "1.5", "2.4", "2.5", "3.3", "3.4", "3.5", "5.2", "5.3", "6.2", "7.2", "7.3"] },
    { "id": 3, "tasks": ["9.1"] },
    { "id": 4, "tasks": ["9.2"] },
    { "id": 5, "tasks": ["10.1", "10.2"] }
  ]
}
```
