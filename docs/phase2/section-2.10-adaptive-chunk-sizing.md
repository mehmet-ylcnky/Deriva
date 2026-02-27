# §2.10 Adaptive Chunk Sizing

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization, §2.9 Size-Aware Mode Selection
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 2 days

---

## 1. Problem Statement

### 1.1 Current Limitation

TODO — fixed 64 KB chunk size is near-optimal for general case but suboptimal for specific workloads

### 1.2 Empirical Evidence

TODO — chunk size sensitivity table (1 KB → 1 MB, latency + channel ops)

### 1.3 Throughput vs Latency Tradeoff

TODO — small chunks = better TTFB, large chunks = better throughput; no single size is optimal

---

## 2. Design

### 2.1 Architecture Overview

TODO — `ChunkResizer` stage inserted between mismatched-throughput stages

### 2.2 Throughput Monitoring

TODO — measure send/recv timing per stage to detect backpressure vs starvation

### 2.3 Resize Strategy

TODO — if producer faster than consumer (backpressure), increase chunk size to reduce channel ops; if consumer faster (starvation), decrease chunk size for better TTFB

### 2.4 Chunk Size Bounds

TODO — min 1 KB, max 1 MB, default 64 KB; configurable per-function via `preferred_chunk_size()`

### 2.5 Automatic Insertion

TODO — pipeline builder detects adjacent stages with mismatched throughput characteristics and inserts `ChunkResizer` automatically

---

## 3. Implementation

### 3.1 ChunkResizer Enhancement

TODO — extend existing `StreamingChunkResizer` with adaptive target size based on timing feedback

### 3.2 Throughput Probe

TODO — `ThroughputProbe` struct tracking send/recv timestamps, computing bytes/sec per stage

### 3.3 Resize Decision Function

TODO — `fn compute_target_chunk_size(producer_bps: f64, consumer_bps: f64, current_size: usize) -> usize`

### 3.4 Pipeline Builder Integration

TODO — auto-insert resizer in `StreamPipeline::execute()` when adjacent stages have different `preferred_chunk_size()`

### 3.5 PipelineConfig Extension

TODO — `adaptive_chunking: bool` (default false), `min_chunk_size`, `max_chunk_size`

---

## 4. Data Flow Diagrams

### 4.1 Without Adaptive Chunking

TODO — fixed 64 KB chunks through all stages

### 4.2 With Adaptive Chunking

TODO — resizer between fast producer and slow consumer, chunk size growing

### 4.3 Feedback Loop

TODO — timing measurement → resize decision → new chunk boundaries

---

## 5. Test Specification

### 5.1 Unit Tests — ThroughputProbe

TODO — probe accuracy with known send/recv intervals

### 5.2 Unit Tests — Resize Decision Function

TODO — backpressure → larger chunks, starvation → smaller chunks, balanced → no change

### 5.3 Unit Tests — ChunkResizer Adaptive Mode

TODO — verify chunk sizes change over time based on simulated throughput

### 5.4 Integration Tests — Auto-Insertion

TODO — pipeline with mismatched stages gets resizer inserted

### 5.5 Benchmark — Adaptive vs Fixed

TODO — compare adaptive chunking against fixed 64 KB on heterogeneous pipelines

---

## 6. Edge Cases & Error Handling

TODO — table: single-chunk stream, all chunks same size, oscillating throughput, zero-length chunks, resizer at pipeline start/end

---

## 7. Performance Analysis

### 7.1 Expected Improvement

TODO — 10–30% throughput gain on heterogeneous pipelines (fast compress → slow encrypt)

### 7.2 Overhead of Throughput Monitoring

TODO — one timestamp per send/recv, negligible

### 7.3 Convergence Time

TODO — how many chunks before adaptive sizing stabilizes

---

## 8. Files Changed

TODO — table of affected files

---

## 9. Dependency Changes

TODO — none expected (uses `std::time::Instant`)

---

## 10. Design Rationale

### 10.1 Why Not Always Use Large Chunks?

TODO — large chunks increase TTFB and memory; poor for interactive/streaming clients

### 10.2 Why Feedback-Based Instead of Predictive?

TODO — workload characteristics unknown ahead of time; feedback adapts to actual runtime behavior

### 10.3 Why Off by Default?

TODO — fixed 64 KB is good enough for 80% of workloads; adaptive adds complexity and potential instability

---

## 11. Observability Integration

TODO — `deriva_chunk_size_bytes` histogram, `deriva_chunk_resize_total` counter, `deriva_stage_throughput_bytes_per_sec` gauge

---

## 12. Checklist

- [ ] Implement `ThroughputProbe` with send/recv timing
- [ ] Implement `compute_target_chunk_size()` decision function
- [ ] Extend `StreamingChunkResizer` with adaptive mode
- [ ] Add `adaptive_chunking` to `PipelineConfig`
- [ ] Auto-insert resizer in pipeline builder when enabled
- [ ] Unit tests for probe, decision function, resizer
- [ ] Integration test for auto-insertion
- [ ] Benchmark adaptive vs fixed on heterogeneous pipeline
- [ ] Add observability metrics (chunk size histogram, resize counter)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing tests still pass
- [ ] Commit and push
