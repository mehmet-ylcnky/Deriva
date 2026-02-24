# Future Improvements: Streaming Materialization

Identified during the Paper 2b evaluation (benchmarks on `streaming_pipeline.rs` and `batch_vs_streaming` sweep). These are concrete, implementation-ready improvements informed by empirical data.

---

## 1. Size-Aware Execution Mode Selection

**Problem:** The `StreamingExecutor` currently selects streaming over batch whenever a `StreamingComputeFunction` is registered — regardless of input size. Benchmarks show batch is up to 9.3× faster below ~3 MB due to streaming's fixed overhead (channel creation, Tokio task spawning, chunk framing).

**Crossover data (5-stage pipeline, Criterion.rs median):**

| Input Size | Batch | Streaming | Winner |
|------------|-------|-----------|--------|
| 1 MB | 0.17 ms | 1.59 ms | Batch 9.3× |
| 2 MB | 0.41 ms | 2.36 ms | Batch 5.7× |
| 4 MB | 4.18 ms | 3.03 ms | Stream 1.4× |
| 8 MB | 9.84 ms | 4.72 ms | Stream 2.1× |

**Implementation:**

```rust
// In PipelineConfig:
pub streaming_threshold: usize, // default: 3 * 1024 * 1024 (3 MB)

// In StreamingExecutor::materialize_streaming(), at the selection point:
let input_size: usize = input_indices.iter()
    .filter_map(|&i| pipeline.node_data_size(i))
    .sum();

let use_streaming = input_size > self.config.streaming_threshold
    || input_size == 0; // unknown size → default to streaming

if use_streaming {
    if let Some(streaming_fn) = registry.get_streaming(func_id) {
        // streaming path
    }
} else if let Some(batch_fn) = registry.get(func_id) {
    // batch path — faster for small inputs
} else if let Some(streaming_fn) = registry.get_streaming(func_id) {
    // no batch impl, use streaming anyway
}
```

**Complexity:** ~20 lines. Requires adding `node_data_size(idx) -> Option<usize>` to `StreamPipeline` — trivial for `Source`/`Cached` nodes (return `bytes.len()`), returns `None` for computed nodes.

**Limitation:** Input size is only known for leaf/cached nodes. For intermediate computed nodes, the size is unknown until execution. Could be extended with size hints from function metadata.

---

## 2. Adaptive Chunk Sizing

**Problem:** The default 64 KB chunk size is near-optimal for the general case, but benchmarks show significant sensitivity:

| Chunk Size | Latency (1 MB) | Channel Ops |
|------------|----------------|-------------|
| 1 KB | 8.56 ms | 1,000 |
| 16 KB | 1.76 ms | 63 |
| 64 KB | 1.50 ms | 16 |
| 256 KB | 1.62 ms | 4 |
| 1 MB | 1.33 ms | 1 |

**Improvement:** Dynamically adjust chunk size based on observed throughput. If a stage processes chunks faster than its downstream can consume (backpressure detected), increase chunk size to reduce channel overhead. If a consumer is fast, decrease chunk size for better TTFB.

**Implementation:** Add a `ChunkResizer` stage that monitors send/recv timing and adjusts the `Bytes::slice()` boundaries. Could be inserted automatically by the pipeline builder between stages with mismatched throughput characteristics.

---

## 3. Pipeline Fusion

**Problem:** Adjacent map stages (e.g., `StreamingUppercase` → `StreamingLowercase`) each require a channel hop (~100 ns per chunk). For simple byte transforms, the channel overhead can exceed the compute cost.

**Improvement:** Detect fusible adjacent stages during pipeline construction and merge them into a single stage that applies both transforms per chunk. The optimizer must prove that fusion preserves semantics (both stages are pure map functions with no state).

**Implementation:** Add a `FusedMapStage` that holds a `Vec<Arc<dyn StreamingComputeFunction>>` and applies them sequentially per chunk within a single Tokio task. The pipeline builder identifies fusible chains during topological construction.

---

## 4. Memory Budget Enforcement

**Problem:** `PipelineConfig::memory_budget` exists but is not enforced. A pipeline with many stages could exceed system memory even with bounded channels.

**Improvement:** A global memory controller that tracks total buffer allocation across all pipeline channels. When total memory approaches the budget, the controller pauses producers by holding channel permits — providing system-wide backpressure beyond per-channel bounds.

**Implementation:** Use a `tokio::sync::Semaphore` with permits equal to `memory_budget / chunk_size`. Each `send()` acquires a permit, each `recv()` releases one. This bounds total in-flight data across the entire pipeline.

---

## 5. Streaming-Aware Caching

**Problem:** Cache-after-collect requires buffering the full result before caching. For very large outputs (>1 GB), this defeats the bounded-memory benefit of streaming.

**Improvement:** A chunk-level cache that stores individual chunks keyed by `(CAddr, offset)`. Enables partial cache hits (serve bytes 0–64KB from cache while computing the rest) and range reads without full materialization.

**Complexity:** High. Requires changes to the cache interface, CAddr model (sub-addressing), and GC (chunk-level reference counting). Deferred to Phase 3.
