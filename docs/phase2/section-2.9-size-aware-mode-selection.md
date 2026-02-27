# §2.9 Size-Aware Execution Mode Selection

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 1 day

---

## 1. Problem Statement

### 1.1 Current Limitation

TODO

### 1.2 Empirical Evidence

TODO — crossover data table from benchmarks

### 1.3 Comparison

TODO — table: current (always-streaming) vs size-aware

---

## 2. Design

### 2.1 Architecture Overview

TODO — decision flow diagram

### 2.2 Threshold Configuration

TODO — `PipelineConfig::streaming_threshold` field

### 2.3 Size Estimation Strategy

TODO — known-size (Source/Cached) vs unknown-size (Computed) nodes

### 2.4 Selection Algorithm

TODO — pseudocode for the 3-way fallback: streaming → batch → streaming-anyway

---

## 3. Implementation

### 3.1 PipelineConfig Extension

TODO — `streaming_threshold: usize` field, default 3 MB

### 3.2 Node Data Size Query

TODO — `StreamPipeline::node_data_size(idx) -> Option<usize>`

### 3.3 Selection Logic in StreamingExecutor

TODO — modified `materialize_streaming()` with size check

### 3.4 Telemetry: Mode Selection Counter

TODO — Prometheus counter `deriva_mode_selection_total{mode="streaming"|"batch"|"streaming_fallback"}`

---

## 4. Data Flow Diagrams

### 4.1 Small Input Path (< threshold)

TODO

### 4.2 Large Input Path (≥ threshold)

TODO

### 4.3 Unknown Size Path

TODO

---

## 5. Test Specification

### 5.1 Unit Tests — node_data_size

TODO

### 5.2 Unit Tests — Threshold Selection Logic

TODO — below threshold → batch, above → streaming, unknown → streaming

### 5.3 Integration Tests — End-to-End Mode Selection

TODO — pipeline with mixed sizes

### 5.4 Benchmark — Verify Crossover Point

TODO — parameterized bench at 1/2/3/4/8 MB

---

## 6. Edge Cases & Error Handling

TODO — table: zero-size input, unknown size, threshold=0, threshold=usize::MAX, multi-input mixed sizes

---

## 7. Performance Analysis

### 7.1 Expected Improvement

TODO — quantify: 9.3× faster for 1 MB inputs, neutral for >4 MB

### 7.2 Overhead of Size Check

TODO — O(1) per node, negligible

---

## 8. Files Changed

TODO — table of affected files

---

## 9. Dependency Changes

TODO — none expected

---

## 10. Design Rationale

### 10.1 Why a Static Threshold Instead of Adaptive?

TODO

### 10.2 Why Default to Streaming for Unknown Sizes?

TODO

### 10.3 Why Sum Input Sizes Instead of Max?

TODO

---

## 11. Observability Integration

TODO — mode selection counter, threshold config gauge

---

## 12. Checklist

- [ ] Add `streaming_threshold` to `PipelineConfig`
- [ ] Implement `node_data_size()` on `StreamPipeline`
- [ ] Add size-aware selection in `StreamingExecutor::materialize_streaming()`
- [ ] Add mode selection Prometheus counter
- [ ] Unit tests for threshold logic
- [ ] Integration test for end-to-end mode selection
- [ ] Benchmark verifying crossover improvement
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing tests still pass
- [ ] Commit and push
