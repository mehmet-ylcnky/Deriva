# Requirements Document

## Introduction

Phase 2.5 adds production-grade observability to the Computation-Addressed Distributed File System (Deriva/CAS-DFS). The feature integrates structured tracing via `tokio-tracing` and Prometheus metrics across all critical paths: materialization, cache, DAG operations, and gRPC RPCs. It provides dashboards, alerting support, and health endpoints for operational monitoring.

## Glossary

- **Metrics_Server**: An axum-based HTTP server running on a configurable port (default 9090) that exposes Prometheus metrics and health check endpoints.
- **Tracing_Subscriber**: The `tracing-subscriber` component that formats and outputs structured log events in text or JSON format with configurable log levels.
- **Prometheus_Registry**: The global Prometheus metric registry that holds all metric collectors (counters, histograms, gauges) and encodes them for scraping.
- **Request_ID**: A UUID generated per inbound RPC request, propagated across async spans for end-to-end correlation.
- **Metrics_Endpoint**: The HTTP `/metrics` path served by the Metrics_Server that returns all registered metrics in Prometheus text exposition format.
- **Health_Endpoint**: The HTTP `/health` path served by the Metrics_Server that reports liveness status of the server.
- **EnvFilter**: The `tracing-subscriber` runtime log level filter controlled via the `RUST_LOG` environment variable.
- **Instrument_RPC_Macro**: A Rust macro (`instrument_rpc!`) that wraps RPC handler bodies to automatically record request count, latency, status, and active gauge for each method.
- **Structured_Span**: A `tracing` span that carries structured fields (e.g., addr, request_id) and tracks duration of an async operation.
- **Counter**: A Prometheus metric type that only increases, used for counting events (e.g., total RPCs, cache hits).
- **Histogram**: A Prometheus metric type that records value distributions in configurable buckets (e.g., latency, byte sizes).
- **Gauge**: A Prometheus metric type that can increase or decrease, representing a current value (e.g., active materializations, cache size).

## Requirements

### Requirement 1: Prometheus Metric Registration

**User Story:** As a system operator, I want all observability metrics to be statically registered at startup, so that the metrics endpoint is immediately available for scraping without requiring traffic to initialize metrics.

#### Acceptance Criteria

1. THE Prometheus_Registry SHALL register 23 metrics at process startup covering RPC, materialization, cache, compute, verification, DAG, and storage categories.
2. WHEN a metric is accessed for the first time, THE Prometheus_Registry SHALL return the pre-registered metric without error or panic.
3. THE Prometheus_Registry SHALL define RPC metrics with labels: `deriva_rpc_total` (method, status), `deriva_rpc_duration_seconds` (method), `deriva_rpc_active` (method).
4. THE Prometheus_Registry SHALL define materialization metrics: `deriva_materialize_total` (result: hit/miss/error), `deriva_materialize_duration_seconds`, `deriva_materialize_active`, `deriva_materialize_depth`.
5. THE Prometheus_Registry SHALL define cache metrics: `deriva_cache_total` (result: hit/miss), `deriva_cache_eviction_total`, `deriva_cache_size_bytes`, `deriva_cache_entries`, `deriva_cache_hit_rate`.
6. THE Prometheus_Registry SHALL define compute metrics: `deriva_compute_duration_seconds` (function), `deriva_compute_input_bytes` (function), `deriva_compute_output_bytes` (function).
7. THE Prometheus_Registry SHALL define verification metrics: `deriva_verification_total` (result: pass/fail), `deriva_verification_failure_rate`.
8. THE Prometheus_Registry SHALL define DAG metrics: `deriva_dag_recipes`, `deriva_dag_insert_duration_seconds`.
9. THE Prometheus_Registry SHALL define storage metrics: `deriva_storage_blobs`, `deriva_storage_blob_bytes`.

### Requirement 2: Metrics HTTP Endpoint

**User Story:** As a system operator, I want an HTTP endpoint that exposes metrics in Prometheus text format, so that Prometheus can scrape and store time-series data for dashboards and alerting.

#### Acceptance Criteria

1. WHEN the server starts, THE Metrics_Server SHALL bind to a configurable port (default 9090) separate from the gRPC port.
2. WHEN a GET request is received at `/metrics`, THE Metrics_Server SHALL respond with HTTP 200 and a body in Prometheus text exposition format containing all registered metrics.
3. WHEN a GET request is received at `/health`, THE Metrics_Server SHALL respond with HTTP 200 and a JSON body containing server liveness status.
4. IF the configured metrics port is already in use, THEN THE Metrics_Server SHALL log an error and terminate the process with a descriptive message.
5. WHILE the Metrics_Server is running, THE Metrics_Server SHALL handle concurrent scrape requests without blocking gRPC traffic.

### Requirement 3: RPC Instrumentation

**User Story:** As a system operator, I want every gRPC RPC to be automatically instrumented with metrics, so that I can monitor request rates, error rates, and latencies per method.

#### Acceptance Criteria

1. WHEN an RPC handler begins execution, THE Instrument_RPC_Macro SHALL increment the `deriva_rpc_active` gauge for the corresponding method label.
2. WHEN an RPC handler completes execution, THE Instrument_RPC_Macro SHALL decrement the `deriva_rpc_active` gauge, increment `deriva_rpc_total` with method and status (ok/error) labels, and observe the elapsed duration in `deriva_rpc_duration_seconds`.
3. THE Instrument_RPC_Macro SHALL apply to all gRPC methods: Get, PutLeaf, PutRecipe, Verify, and Status.
4. WHEN an RPC returns an error, THE Instrument_RPC_Macro SHALL record the status label as "error".
5. WHEN an RPC returns successfully, THE Instrument_RPC_Macro SHALL record the status label as "ok".

### Requirement 4: Materialization Metrics

**User Story:** As a system operator, I want materialization operations to emit metrics for cache hits, misses, errors, active count, duration, and DAG depth, so that I can identify performance bottlenecks and monitor cache effectiveness.

#### Acceptance Criteria

1. WHEN a materialization resolves from cache, THE Prometheus_Registry SHALL increment `deriva_materialize_total` with result="hit".
2. WHEN a materialization requires computation, THE Prometheus_Registry SHALL increment `deriva_materialize_total` with result="miss".
3. WHEN a materialization fails with an error, THE Prometheus_Registry SHALL increment `deriva_materialize_total` with result="error".
4. WHEN a materialization begins, THE Prometheus_Registry SHALL increment `deriva_materialize_active`, and decrement it upon completion.
5. WHEN a materialization completes, THE Prometheus_Registry SHALL observe the total elapsed time in `deriva_materialize_duration_seconds`.
6. WHEN a materialization completes, THE Prometheus_Registry SHALL observe the DAG depth traversed in `deriva_materialize_depth`.

### Requirement 5: Cache Metrics

**User Story:** As a system operator, I want real-time cache metrics, so that I can monitor hit rates, eviction pressure, and capacity utilization.

#### Acceptance Criteria

1. WHEN a cache lookup succeeds, THE Prometheus_Registry SHALL increment `deriva_cache_total` with result="hit".
2. WHEN a cache lookup fails (miss), THE Prometheus_Registry SHALL increment `deriva_cache_total` with result="miss".
3. WHEN a cache eviction occurs, THE Prometheus_Registry SHALL increment `deriva_cache_eviction_total`.
4. WHEN the cache size changes, THE Prometheus_Registry SHALL update `deriva_cache_size_bytes` to reflect the current total byte size.
5. WHEN the cache entry count changes, THE Prometheus_Registry SHALL update `deriva_cache_entries` to reflect the current entry count.
6. THE Prometheus_Registry SHALL maintain `deriva_cache_hit_rate` as a gauge reflecting the rolling cache hit ratio.

### Requirement 6: Compute Metrics

**User Story:** As a system operator, I want per-function compute metrics, so that I can identify expensive functions and monitor input/output size distributions.

#### Acceptance Criteria

1. WHEN a compute function execution completes, THE Prometheus_Registry SHALL observe the elapsed duration in `deriva_compute_duration_seconds` with the function name as label.
2. WHEN a compute function execution begins, THE Prometheus_Registry SHALL observe the total input byte size in `deriva_compute_input_bytes` with the function name as label.
3. WHEN a compute function execution completes, THE Prometheus_Registry SHALL observe the output byte size in `deriva_compute_output_bytes` with the function name as label.

### Requirement 7: Verification and DAG Metrics

**User Story:** As a system operator, I want verification outcome and DAG structure metrics, so that I can detect determinism violations and monitor DAG growth.

#### Acceptance Criteria

1. WHEN a verification check passes, THE Prometheus_Registry SHALL increment `deriva_verification_total` with result="pass".
2. WHEN a verification check fails, THE Prometheus_Registry SHALL increment `deriva_verification_total` with result="fail".
3. THE Prometheus_Registry SHALL maintain `deriva_verification_failure_rate` as a gauge reflecting the rolling verification failure ratio.
4. WHEN a recipe is inserted into the DAG, THE Prometheus_Registry SHALL update `deriva_dag_recipes` to reflect the current recipe count.
5. WHEN a recipe is inserted into the DAG, THE Prometheus_Registry SHALL observe the insertion duration in `deriva_dag_insert_duration_seconds`.

### Requirement 8: Storage Metrics

**User Story:** As a system operator, I want storage metrics, so that I can monitor blob count and total storage consumption.

#### Acceptance Criteria

1. WHEN blob storage state changes, THE Prometheus_Registry SHALL update `deriva_storage_blobs` to reflect the current total blob count.
2. WHEN blob storage state changes, THE Prometheus_Registry SHALL update `deriva_storage_blob_bytes` to reflect the current total byte size of all stored blobs.

### Requirement 9: Structured Tracing

**User Story:** As a developer, I want structured log output with configurable format and level, so that I can debug issues in development (text format) and integrate with log aggregation in production (JSON format).

#### Acceptance Criteria

1. WHEN the server starts with `--log-format text`, THE Tracing_Subscriber SHALL output human-readable logs with uptime timestamps, target module, and span context.
2. WHEN the server starts with `--log-format json`, THE Tracing_Subscriber SHALL output JSON-formatted log events with timestamp, level, target, span, and structured fields.
3. THE Tracing_Subscriber SHALL respect the `RUST_LOG` environment variable for runtime log level filtering via EnvFilter.
4. WHEN `RUST_LOG` is not set, THE Tracing_Subscriber SHALL default to the `deriva=info` filter level.
5. THE Tracing_Subscriber SHALL support five log levels: ERROR (unrecoverable failures), WARN (recoverable issues), INFO (request lifecycle), DEBUG (internal decisions), TRACE (individual operations).

### Requirement 10: Request ID Correlation

**User Story:** As a developer, I want a unique request ID propagated across all async spans within a single RPC, so that I can trace end-to-end request flows in logs.

#### Acceptance Criteria

1. WHEN an RPC request is received, THE Instrument_RPC_Macro SHALL generate a UUID request ID and attach it as a field on the root tracing span.
2. WHILE processing an RPC request, THE Tracing_Subscriber SHALL include the request ID in all child spans and log events within that request's async context.
3. THE Request_ID SHALL be a version-4 UUID formatted as a standard hyphenated string.

### Requirement 11: Tracing Spans for Materialization

**User Story:** As a developer, I want each materialization to create a tracing span with the content address, so that I can correlate log events to specific materializations.

#### Acceptance Criteria

1. WHEN a materialization begins, THE Structured_Span SHALL be created at INFO level with span name "materialize" and field `addr` set to the content address.
2. WHEN a cache check occurs within a materialization, THE Structured_Span SHALL emit a DEBUG-level event indicating hit or miss.
3. WHEN a compute function executes within a materialization, THE Structured_Span SHALL emit an INFO-level event with function name, input bytes, output bytes, and duration.

### Requirement 12: Performance Overhead

**User Story:** As a system architect, I want observability overhead to remain negligible relative to compute time, so that instrumentation does not degrade throughput.

#### Acceptance Criteria

1. THE Prometheus_Registry SHALL add no more than 500 nanoseconds of total per-request overhead for counter increments, histogram observations, and gauge updates combined.
2. THE Tracing_Subscriber SHALL add no more than 150 nanoseconds of overhead per span creation and close.
3. THE Metrics_Server SHALL encode all registered metrics in under 5 milliseconds per scrape request.

### Requirement 13: Server CLI Configuration

**User Story:** As a system operator, I want CLI flags and environment variables to control observability behavior, so that I can tune logging and metrics for different deployment environments.

#### Acceptance Criteria

1. THE Metrics_Server SHALL accept a `--metrics-port` CLI flag to configure the metrics HTTP port, defaulting to 9090.
2. THE Tracing_Subscriber SHALL accept a `--log-format` CLI flag with values `text` (default) or `json`.
3. THE Tracing_Subscriber SHALL accept the `RUST_LOG` environment variable for log level configuration.

### Requirement 14: Monitoring Stack Integration

**User Story:** As a developer, I want a Docker Compose configuration with Prometheus and Grafana pre-configured to scrape the Deriva metrics endpoint, so that I can visualize metrics locally during development.

#### Acceptance Criteria

1. THE Monitoring_Stack SHALL provide a Docker Compose file that starts Prometheus and Grafana services.
2. THE Monitoring_Stack SHALL pre-configure Prometheus to scrape the Deriva metrics endpoint at the configured port and 15-second interval.
3. THE Monitoring_Stack SHALL provide a Grafana dashboard with panels for RPC rate, RPC latency percentiles, cache hit rate, cache size, active materializations, and compute duration by function.
