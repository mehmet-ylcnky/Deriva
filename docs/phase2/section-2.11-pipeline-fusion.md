# §2.11 Pipeline Fusion

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 2–3 days

---

## 1. Problem Statement

### 1.1 Current Limitation

TODO — each streaming stage requires a channel hop (~100 ns per chunk); for simple byte transforms the channel overhead can exceed compute cost

### 1.2 Empirical Evidence

TODO — adjacent map stages (e.g., `StreamingUppercase` → `StreamingLowercase`) each pay full channel cost; quantify overhead for N-stage identity-like pipelines

### 1.3 Comparison

TODO — table: unfused vs fused pipeline (stages, channel hops, overhead per chunk, total overhead for 100 MB)

---

## 2. Design

### 2.1 Architecture Overview

TODO — optimizer pass during pipeline construction that detects fusible chains and merges them into `FusedMapStage`

### 2.2 Fusibility Criteria

TODO — both stages must be pure map functions (stateless, single-input, single-output, no side effects); accumulator/combiner/utility stages are NOT fusible

### 2.3 Fusibility Detection

TODO — `StreamingComputeFunction::is_fusible() -> bool` trait method (default false); only functions that opt in are candidates

### 2.4 FusedMapStage

TODO — holds `Vec<Arc<dyn StreamingComputeFunction>>`, applies all transforms sequentially per chunk within a single Tokio task

### 2.5 Fusion Optimizer Pass

TODO — walk pipeline nodes in topo order; when adjacent fusible stages found, merge into single `FusedMapStage` node, rewire edges

---

## 3. Implementation

### 3.1 Trait Extension

TODO — add `fn is_fusible(&self) -> bool` to `StreamingComputeFunction` with default `false`; mark pure map builtins as fusible

### 3.2 FusedMapStage Struct

TODO — `struct FusedMapStage { stages: Vec<Arc<dyn StreamingComputeFunction>> }` implementing `StreamingComputeFunction`; `stream_execute` applies each stage's transform in sequence per chunk

### 3.3 Fusion Optimizer

TODO — `fn fuse_pipeline(nodes: Vec<PipelineNode>) -> Vec<PipelineNode>` — scan for adjacent fusible StreamingStage nodes, merge chains, update input_indices

### 3.4 Pipeline Builder Integration

TODO — call `fuse_pipeline()` in `StreamPipeline::execute()` before spawning tasks; gated by `PipelineConfig::enable_fusion` (default true)

### 3.5 PipelineConfig Extension

TODO — `enable_fusion: bool` field

### 3.6 Fusible Built-in Functions

TODO — list which of the 20 existing builtins are fusible: Identity, Uppercase, Lowercase, Reverse, Base64Encode/Decode, XorCipher, Compress, Decompress (9 of 20)

---

## 4. Data Flow Diagrams

### 4.1 Before Fusion

TODO — 3 adjacent map stages with 3 channels

### 4.2 After Fusion

TODO — single fused stage with 1 channel, applying 3 transforms per chunk

### 4.3 Partial Fusion

TODO — fusible chain interrupted by non-fusible stage (accumulator); two separate fused groups

---

## 5. Test Specification

### 5.1 Unit Tests — is_fusible Trait Method

TODO — verify fusible builtins return true, non-fusible return false

### 5.2 Unit Tests — FusedMapStage

TODO — fuse Uppercase + Lowercase, verify per-chunk application; fuse 3+ stages

### 5.3 Unit Tests — Fusion Optimizer

TODO — chain of 3 fusible → merged to 1; fusible-nonfusible-fusible → 3 nodes; single stage → unchanged; no fusible → unchanged

### 5.4 Integration Tests — Fused Pipeline Correctness

TODO — fused pipeline produces identical output to unfused pipeline

### 5.5 Benchmark — Fused vs Unfused

TODO — 5-stage identity pipeline, measure channel overhead elimination

---

## 6. Edge Cases & Error Handling

TODO — table: single-stage pipeline (no fusion), all stages fusible (one mega-stage), error in middle of fused chain, params differ between fused stages, fan-out before fusible chain

---

## 7. Performance Analysis

### 7.1 Expected Improvement

TODO — eliminate (N-1) channel hops for N-stage fusible chain; ~100 ns × chunks saved per eliminated hop

### 7.2 Overhead of Fusion Pass

TODO — O(N) scan of pipeline nodes, negligible

### 7.3 When Fusion Hurts

TODO — fused stage runs in single task (no inter-stage parallelism); only beneficial when per-chunk compute < channel overhead

---

## 8. Files Changed

TODO — table of affected files

---

## 9. Dependency Changes

TODO — none expected

---

## 10. Design Rationale

### 10.1 Why Opt-In Fusibility Instead of Auto-Detection?

TODO — proving purity statically is impossible; opt-in is safe and explicit

### 10.2 Why Fuse at Pipeline Construction Instead of Runtime?

TODO — construction-time fusion is simpler, deterministic, and has zero runtime overhead

### 10.3 Why Not Fuse Accumulators?

TODO — accumulators consume all input before emitting; fusing them with map stages would change semantics

---

## 11. Observability Integration

TODO — `deriva_fused_stages_total` counter, `deriva_fusion_chains_total` counter, log fusion decisions at DEBUG level

---

## 12. Checklist

- [ ] Add `is_fusible()` to `StreamingComputeFunction` trait
- [ ] Mark fusible builtins (9 of 20)
- [ ] Implement `FusedMapStage` struct
- [ ] Implement `fuse_pipeline()` optimizer pass
- [ ] Add `enable_fusion` to `PipelineConfig`
- [ ] Integrate fusion pass into `StreamPipeline::execute()`
- [ ] Unit tests for `is_fusible`, `FusedMapStage`, optimizer
- [ ] Integration test: fused == unfused output
- [ ] Benchmark fused vs unfused
- [ ] Add observability metrics
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing tests still pass
- [ ] Commit and push
