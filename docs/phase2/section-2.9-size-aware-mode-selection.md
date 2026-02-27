# §2.9 Size-Aware Execution Mode Selection

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 1 day

---

## 1. Problem Statement

### 1.1 Current Limitation

The `StreamingExecutor` introduced in §2.7 selects streaming over batch
whenever a `StreamingComputeFunction` is registered — regardless of input
size. The selection logic in `materialize_streaming()` is binary:

```rust
// Current selection (from crates/deriva-compute/src/executor.rs)
if let Some(streaming_fn) = registry.get_streaming(&func_key) {
    // always takes this path when a streaming impl exists
    let idx = pipeline.add_streaming_stage(
        *topo_addr, streaming_fn, recipe.params.clone(), input_indices,
    );
    addr_to_idx.insert(*topo_addr, idx);
} else if let Some(batch_fn) = registry.get(&func_key) {
    // batch path — only used when NO streaming impl exists
    ...
}
```

This means:
1. **Small values pay streaming overhead for no benefit** — channel
   creation, Tokio task spawning, chunk framing, and inter-task
   synchronization add fixed costs that dominate for small inputs
2. **No way to prefer batch even when both implementations exist** —
   the registry always favours the streaming path
3. **Pipeline construction cost is constant** — a 5-stage pipeline
   spawns 5 Tokio tasks and creates 5 bounded channels even for a
   1 KB input that would complete in microseconds via batch

### 1.2 Empirical Evidence

Benchmarks from the Paper 2b evaluation (`batch_vs_streaming` Criterion
sweep, 5-stage identity pipeline, 64 KB chunks, channel capacity 8)
reveal a clear crossover point:

| Input Size | Batch (median) | Streaming (median) | Winner | Ratio |
|------------|----------------|-------------------|--------|-------|
| 1 MB | 0.17 ms | 1.59 ms | Batch | 9.3× |
| 2 MB | 0.41 ms | 2.36 ms | Batch | 5.7× |
| 3 MB | 1.12 ms | 2.68 ms | Batch | 2.4× |
| 4 MB | 4.18 ms | 3.03 ms | **Streaming** | 1.4× |
| 8 MB | 9.84 ms | 4.72 ms | **Streaming** | 2.1× |
| 16 MB | 22.1 ms | 7.75 ms | **Streaming** | 2.9× |
| 64 MB | 130.3 ms | 50.8 ms | **Streaming** | 2.6× |

The crossover occurs at approximately **3 MB** of total input size.
Below this threshold, batch execution avoids:
- 5 `tokio::spawn` calls (~2 μs each)
- 5 `mpsc::channel(8)` allocations (~1 μs each)
- 16 chunk send/recv round-trips for 1 MB at 64 KB chunks (~100 ns each)
- Task scheduler overhead for 5 concurrent tasks

Above the threshold, streaming's pipeline parallelism and bounded memory
dominate, delivering 1.4–2.9× speedup that grows with input size.

### 1.3 Real-World Impact

```
Workload: Mixed-size CAS pipeline (typical data lake)

  Distribution of materialization requests by input size:
    <1 MB:   60% of requests  (metadata, configs, small files)
    1–3 MB:  15% of requests  (documents, small datasets)
    3–10 MB: 15% of requests  (images, medium datasets)
    >10 MB:  10% of requests  (videos, large datasets, archives)

  Current behavior (always streaming):
    75% of requests (< 3 MB) pay 2.4–9.3× overhead
    Weighted average penalty: ~4.5× for the majority of requests

  With size-aware selection:
    <3 MB requests use batch → 2.4–9.3× faster
    ≥3 MB requests use streaming → same performance
    Net improvement: ~3× faster for the median request
```

### 1.4 Comparison

| Aspect | Current (always streaming) | Size-aware (§2.9) |
|--------|---------------------------|-------------------|
| Small inputs (<3 MB) | 2.4–9.3× slower than batch | Batch speed |
| Large inputs (≥3 MB) | Optimal | Optimal (unchanged) |
| Unknown-size inputs | Streaming (correct) | Streaming (safe default) |
| Decision overhead | None | O(1) per pipeline node |
| Configuration | None | Single threshold (default 3 MB) |
| Implementation complexity | N/A | ~30 lines changed |
| Memory for small inputs | 5 × 8 × 64 KB = 2.5 MB | O(input size) |

### 1.5 What Already Exists

The infrastructure for both execution modes is fully implemented in §2.7:

- `StreamPipeline` supports both `StreamingStage` and `BatchStage` nodes
- `FunctionRegistry` stores both `streaming_functions` and `functions` maps
- `batch_to_stream()` adapter wraps batch results as single-chunk streams
- `collect_stream()` collects streaming output back to `Bytes`

What is missing is the **decision logic** — the code that chooses which
path to take based on input characteristics. Currently the choice is
hardcoded: streaming if available, batch otherwise.

---

## 2. Design

### 2.1 Architecture Overview

The size-aware selector sits between recipe resolution and pipeline
construction — it inspects each node's input sizes before choosing
the execution mode:

```
┌─────────────────────────────────────────────────────────────┐
│                  StreamingExecutor                            │
│                                                               │
│  ┌──────────┐    ┌──────────────┐    ┌───────────────────┐   │
│  │ Resolve  │───▶│ Size-Aware   │───▶│ Build Pipeline    │   │
│  │ topo     │    │ Mode Selector│    │ (StreamingStage   │   │
│  │ order    │    │              │    │  or BatchStage)   │   │
│  └──────────┘    └──────┬───────┘    └───────────────────┘   │
│                         │                                     │
│                  ┌──────▼───────┐                             │
│                  │ Per-node     │                             │
│                  │ decision:    │                             │
│                  │              │                             │
│                  │ sum(inputs)  │                             │
│                  │   known?     │                             │
│                  │  ┌───┴───┐   │                             │
│                  │  yes     no  │                             │
│                  │  │       │   │                             │
│                  │  │    streaming                            │
│                  │  │    (safe default)                       │
│                  │  ▼                                         │
│                  │ sum ≥ threshold?                           │
│                  │  ┌───┴───┐                                │
│                  │  yes     no                                │
│                  │  │       │                                 │
│                  │ streaming batch                            │
│                  └──────────┘                                 │
└─────────────────────────────────────────────────────────────┘
```

The decision is **per-node**, not per-pipeline. A single pipeline may
contain both streaming and batch stages — the existing hybrid mode
from §2.7 already handles this via `batch_to_stream()` adapters.

### 2.2 Threshold Configuration

A single field added to the existing `PipelineConfig`:

```
┌──────────────────────────────────────┐
│          PipelineConfig              │
│                                      │
│  chunk_size: usize          (64 KB)  │
│  channel_capacity: usize    (8)      │
│  cache_intermediates: bool  (true)   │
│  memory_budget: usize       (0)      │
│                                      │
│  + streaming_threshold: usize (3 MB) │  ← NEW
│                                      │
└──────────────────────────────────────┘
```

Semantics:
- `streaming_threshold = 0` → always streaming (current behaviour)
- `streaming_threshold = usize::MAX` → always batch (force batch)
- `streaming_threshold = 3 * 1024 * 1024` → default: crossover at 3 MB

The threshold is configurable per-pipeline, allowing callers to tune
based on workload characteristics (e.g., a metadata-heavy workload
might raise the threshold to 8 MB).

### 2.3 Size Estimation Strategy

Input size is only knowable for certain node types:

| Node Type | Size Known? | Source |
|-----------|-------------|--------|
| `Source` (leaf blob) | ✅ Yes | `data.len()` |
| `Cached` (materialized) | ✅ Yes | `data.len()` |
| `StreamingStage` (computed) | ❌ No | Output size unknown until execution |
| `BatchStage` (computed) | ❌ No | Output size unknown until execution |

The selector sums the **known** input sizes for each node. If any
input has unknown size, the total is treated as unknown.

```
node_input_size(node) → Option<usize>

  For each input index of the node:
    size_i = pipeline.node_data_size(input_index)
    if size_i is None → return None  (unknown → streaming)

  return Some(sum of all size_i)
```

This is conservative: if even one input is unknown, we default to
streaming. This is safe because streaming always works (it's the
universal fallback), while batch requires all inputs to fit in memory.

**Future extension:** Functions could declare `fn estimated_output_size(&self, input_sizes: &[usize]) -> Option<usize>` to propagate size estimates through the DAG. For example, `identity` preserves size, `compress` estimates 0.5×, `base64_encode` estimates 1.33×. This would allow size-aware selection even for intermediate nodes. Deferred — the leaf/cached case covers the majority of real workloads.

### 2.4 Selection Algorithm

For each recipe node during pipeline construction, the 3-way fallback:

```
select_mode(func_key, input_size: Option<usize>, config, registry):

  1. Compute input_size for this node
     input_size = sum of node_data_size(i) for each input i
                  → None if any input is unknown

  2. Decide preferred mode
     prefer_streaming = match input_size {
         None        → true   // unknown size → streaming (safe)
         Some(0)     → true   // empty input → streaming (negligible either way)
         Some(size)  → size >= config.streaming_threshold
     }

  3. Select implementation with fallback
     if prefer_streaming {
         if registry.has_streaming(func_key) → StreamingStage
         else if registry.has_batch(func_key) → BatchStage
         else → Error("function not found")
     } else {
         // prefer batch for small inputs
         if registry.has_batch(func_key) → BatchStage
         else if registry.has_streaming(func_key) → StreamingStage  // fallback
         else → Error("function not found")
     }
```

The fallback chain ensures correctness:
- If we prefer batch but only streaming exists → use streaming (works, just slower)
- If we prefer streaming but only batch exists → use batch (existing §2.7 hybrid mode)
- Both exist → use the preferred mode based on size

### 2.5 Interaction with Existing Pipeline Modes

The size-aware selector does **not** change how `StreamPipeline` executes.
It only changes which `PipelineNode` variant is inserted per recipe:

```
Before (§2.7):
  Recipe node → always StreamingStage (if streaming impl exists)

After (§2.9):
  Recipe node → StreamingStage OR BatchStage (based on input size)

Pipeline execution is unchanged:
  - StreamingStage: spawns task, connects channels
  - BatchStage: collects inputs, calls execute(), wraps as stream
  - Mixed pipelines work via batch_to_stream() adapter (§2.7 §2.5)
```

Example mixed pipeline after size-aware selection:

```
Recipe: Z = compress(concat(A, B))
  A = 500 KB leaf, B = 800 KB leaf
  Total input to concat: 1.3 MB < 3 MB threshold

  concat: BatchStage (1.3 MB < threshold, batch impl exists)
  compress: input is concat output (unknown size) → StreamingStage

  Pipeline:
    Source(A) ──┐
                ├──▶ BatchStage(concat) ──▶ batch_to_stream ──▶ StreamingStage(compress) ──▶ output
    Source(B) ──┘

  concat runs as batch (fast for 1.3 MB), result wrapped as stream,
  compress runs as streaming (handles potentially large output).
```

### 2.6 Telemetry Design

A Prometheus counter tracks mode selection decisions for operational
visibility and threshold tuning:

```
┌──────────────────────────────────────────────────────┐
│  deriva_mode_selection_total                          │
│                                                       │
│  Labels:                                              │
│    mode = "batch" | "streaming" | "streaming_fallback"│
│    reason = "below_threshold" | "above_threshold"     │
│             | "unknown_size" | "no_batch_impl"        │
│             | "no_streaming_impl"                     │
│                                                       │
│  Examples:                                            │
│    {mode="batch", reason="below_threshold"}      +1   │
│    {mode="streaming", reason="above_threshold"}  +1   │
│    {mode="streaming", reason="unknown_size"}     +1   │
│    {mode="streaming", reason="no_batch_impl"}    +1   │
│    {mode="batch", reason="no_streaming_impl"}    +1   │
└──────────────────────────────────────────────────────┘

  Additionally, a gauge exposes the configured threshold:

  deriva_streaming_threshold_bytes  gauge  3145728
```

This enables operators to:
1. Verify the threshold is effective (ratio of batch vs streaming selections)
2. Detect workloads that would benefit from threshold adjustment
3. Identify functions missing batch implementations (high `no_batch_impl` count)

---

## 3. Implementation

### 3.1 PipelineConfig Extension

Location: `crates/deriva-compute/src/pipeline.rs`

```rust
/// Default streaming threshold: 3 MB.
/// Below this, batch execution is preferred for functions that support both modes.
pub const DEFAULT_STREAMING_THRESHOLD: usize = 3 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub chunk_size: usize,
    pub channel_capacity: usize,
    pub cache_intermediates: bool,
    pub memory_budget: usize,
    /// Input size threshold in bytes. Nodes whose total input size
    /// is below this value prefer batch execution when available.
    /// Set to 0 to always prefer streaming (§2.7 behaviour).
    /// Set to usize::MAX to always prefer batch.
    pub streaming_threshold: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            cache_intermediates: true,
            memory_budget: 0,
            streaming_threshold: DEFAULT_STREAMING_THRESHOLD,
        }
    }
}
```

### 3.2 Node Data Size Query

Location: `crates/deriva-compute/src/pipeline.rs`

```rust
impl StreamPipeline {
    /// Returns the known data size for a pipeline node, or `None`
    /// if the size cannot be determined (computed nodes).
    pub fn node_data_size(&self, idx: usize) -> Option<usize> {
        match &self.nodes[idx] {
            PipelineNode::Source { data, .. } => Some(data.len()),
            PipelineNode::Cached { data, .. } => Some(data.len()),
            PipelineNode::StreamingStage { .. } => None,
            PipelineNode::BatchStage { .. } => None,
        }
    }

    /// Returns the total known input size for a set of input node indices.
    /// Returns `None` if any input has unknown size.
    pub fn total_input_size(&self, input_indices: &[usize]) -> Option<usize> {
        let mut total = 0usize;
        for &idx in input_indices {
            total += self.node_data_size(idx)?;
        }
        Some(total)
    }
}
```

### 3.3 Selection Logic in StreamingExecutor

Location: `crates/deriva-compute/src/executor.rs`

The existing loop body in `materialize_streaming()` that unconditionally
picks streaming is replaced with size-aware selection:

```rust
// --- BEFORE (§2.7) ---
if let Some(streaming_fn) = registry.get_streaming(&func_key) {
    let idx = pipeline.add_streaming_stage(
        *topo_addr, streaming_fn, recipe.params.clone(), input_indices,
    );
    addr_to_idx.insert(*topo_addr, idx);
} else if let Some(batch_fn) = registry.get(&func_key) {
    let idx = pipeline.add_batch_stage(
        *topo_addr, batch_fn, recipe.params.clone(), input_indices,
    );
    addr_to_idx.insert(*topo_addr, idx);
} else {
    return Err(DerivaError::Compute(
        format!("function not found: {}", func_key)
    ));
}

// --- AFTER (§2.9) ---
let input_size = pipeline.total_input_size(&input_indices);
let prefer_streaming = match input_size {
    None => true,        // unknown → streaming (safe default)
    Some(0) => true,     // empty → streaming (negligible)
    Some(s) => s >= self.config.streaming_threshold,
};

let has_streaming = registry.has_streaming(&func_key);
let has_batch = registry.get(&func_key).is_some();

let idx = if prefer_streaming {
    if has_streaming {
        MODE_SELECTION.with_label_values(&["streaming", "above_threshold"]).inc();
        pipeline.add_streaming_stage(
            *topo_addr,
            registry.get_streaming(&func_key).unwrap(),
            recipe.params.clone(),
            input_indices,
        )
    } else if has_batch {
        MODE_SELECTION.with_label_values(&["batch", "no_streaming_impl"]).inc();
        pipeline.add_batch_stage(
            *topo_addr,
            registry.get(&func_key).unwrap(),
            recipe.params.clone(),
            input_indices,
        )
    } else {
        return Err(DerivaError::Compute(
            format!("function not found: {}", func_key),
        ));
    }
} else {
    // prefer batch for small inputs
    if has_batch {
        MODE_SELECTION.with_label_values(&["batch", "below_threshold"]).inc();
        pipeline.add_batch_stage(
            *topo_addr,
            registry.get(&func_key).unwrap(),
            recipe.params.clone(),
            input_indices,
        )
    } else if has_streaming {
        // no batch impl — fall back to streaming
        MODE_SELECTION.with_label_values(&["streaming", "no_batch_impl"]).inc();
        pipeline.add_streaming_stage(
            *topo_addr,
            registry.get_streaming(&func_key).unwrap(),
            recipe.params.clone(),
            input_indices,
        )
    } else {
        return Err(DerivaError::Compute(
            format!("function not found: {}", func_key),
        ));
    }
};

addr_to_idx.insert(*topo_addr, idx);
```

### 3.4 Telemetry: Mode Selection Counter

Location: `crates/deriva-compute/src/executor.rs`

```rust
use prometheus::{IntCounterVec, IntGauge, register_int_counter_vec, register_int_gauge};

lazy_static! {
    static ref MODE_SELECTION: IntCounterVec = register_int_counter_vec!(
        "deriva_mode_selection_total",
        "Execution mode selections by mode and reason",
        &["mode", "reason"]
    ).unwrap();

    static ref STREAMING_THRESHOLD_GAUGE: IntGauge = register_int_gauge!(
        "deriva_streaming_threshold_bytes",
        "Configured streaming threshold in bytes"
    ).unwrap();
}

impl StreamingExecutor {
    pub fn new(config: PipelineConfig) -> Self {
        STREAMING_THRESHOLD_GAUGE.set(config.streaming_threshold as i64);
        Self { config }
    }
}
```

---

## 4. Data Flow Diagrams

### 4.1 Small Input Path (< threshold)

```
Recipe: Z = sha256(uppercase(A))
  A = 500 KB leaf
  threshold = 3 MB

  Selection:
    uppercase: input = A (500 KB, known) → 500 KB < 3 MB → prefer batch
               batch impl exists → BatchStage
    sha256:    input = uppercase output (unknown size) → prefer streaming
               streaming impl exists → StreamingStage

  Pipeline:

  ┌──────────┐  collect   ┌────────────┐  Bytes   ┌──────────────┐
  │ Source(A) │──────────▶│ BatchStage  │────────▶│ batch_to_    │
  │ 500 KB   │  (fast,   │ uppercase   │         │ stream       │
  └──────────┘  no tasks) │ execute()   │         │ (wrap result)│
                           └────────────┘         └──────┬───────┘
                                                          │ chunks
                                                          ▼
                                                  ┌──────────────┐
                                                  │StreamingStage│
                                                  │ sha256       │──▶ 32-byte hash
                                                  │ (fold)       │
                                                  └──────────────┘

  Timing:
    BatchStage(uppercase): ~0.05 ms (direct execute, no channels)
    StreamingStage(sha256): ~0.02 ms (single chunk through fold)
    Total: ~0.07 ms

  vs always-streaming (§2.7):
    2 tasks, 2 channels, 8 chunks at 64 KB → ~0.8 ms
    Improvement: ~11× faster
```

### 4.2 Large Input Path (≥ threshold)

```
Recipe: Z = sha256(uppercase(A))
  A = 64 MB leaf
  threshold = 3 MB

  Selection:
    uppercase: input = A (64 MB, known) → 64 MB ≥ 3 MB → prefer streaming
               streaming impl exists → StreamingStage
    sha256:    input = uppercase output (unknown) → prefer streaming
               streaming impl exists → StreamingStage

  Pipeline (identical to §2.7 behaviour):

  ┌──────────┐  chunks   ┌──────────────┐  chunks   ┌──────────────┐
  │ Source(A) │─────────▶│StreamingStage│─────────▶│StreamingStage│──▶ hash
  │ 64 MB    │  1000×    │ uppercase    │  1000×    │ sha256       │
  └──────────┘  64 KB    │ (map)       │  64 KB    │ (fold)       │
                          └──────────────┘           └──────────────┘

  Memory: 2 channels × 8 × 64 KB = 1 MB (vs 128 MB for batch)
  Timing: ~50 ms (pipelined)
```

### 4.3 Unknown Size Path

```
Recipe: Z = compress(transform(A))
  A = 2 MB leaf
  threshold = 3 MB

  Selection:
    transform: input = A (2 MB, known) → 2 MB < 3 MB → prefer batch
               batch impl exists → BatchStage ✓
    compress:  input = transform output (UNKNOWN — computed node)
               → prefer streaming (safe default)
               streaming impl exists → StreamingStage ✓

  Pipeline:

  ┌──────────┐  collect   ┌────────────┐  batch_to_stream   ┌──────────────┐
  │ Source(A) │──────────▶│ BatchStage  │──────────────────▶│StreamingStage│──▶ out
  │ 2 MB     │           │ transform   │                    │ compress     │
  └──────────┘           └────────────┘                    └──────────────┘

  transform runs as batch (fast for 2 MB).
  compress runs as streaming (output size unknown, could be large).
  Safe: if transform output were 100 MB, streaming handles it gracefully.
```

### 4.4 Mixed-Size Multi-Input Node

```
Recipe: Z = concat(A, B, C)
  A = 100 KB, B = 200 KB, C = 50 MB
  threshold = 3 MB

  Selection:
    concat: inputs = [A, B, C], all known sizes
            total = 100 KB + 200 KB + 50 MB = 50.3 MB ≥ 3 MB
            → prefer streaming
            streaming impl exists → StreamingStage ✓

  Pipeline:

  ┌──────────┐  chunks
  │Source(A)  │────────┐
  │ 100 KB   │        │
  └──────────┘        │
  ┌──────────┐  chunks│   ┌──────────────┐
  │Source(B)  │────────┼──▶│StreamingStage│──▶ output
  │ 200 KB   │        │   │ concat       │
  └──────────┘        │   └──────────────┘
  ┌──────────┐  chunks│
  │Source(C)  │────────┘
  │ 50 MB    │
  └──────────┘

  Even though A and B are small, the total input exceeds the threshold.
  Streaming is correct here — C dominates and benefits from bounded memory.
```

### 4.5 Fallback: Streaming-Only Function Below Threshold

```
Recipe: Z = rate_limit(A)
  A = 500 KB leaf
  threshold = 3 MB

  Selection:
    rate_limit: input = A (500 KB) → 500 KB < 3 MB → prefer batch
                batch impl exists? NO (rate_limit is streaming-only)
                streaming impl exists? YES → StreamingStage (fallback)

  Metric emitted: {mode="streaming", reason="no_batch_impl"}

  Pipeline:

  ┌──────────┐  chunks   ┌──────────────┐
  │ Source(A) │─────────▶│StreamingStage│──▶ output
  │ 500 KB   │  8×64KB   │ rate_limit   │
  └──────────┘           └──────────────┘

  Correct but suboptimal — streaming overhead paid for small input.
  Acceptable: rate_limit has no batch equivalent by design.
```

---

## 5. Test Specification

### 5.1 Unit Tests — node_data_size

```rust
#[test]
fn test_node_data_size_source() {
    let mut pipeline = StreamPipeline::new(PipelineConfig::default());
    let idx = pipeline.add_source(test_addr("a"), Bytes::from(vec![0u8; 1000]));
    assert_eq!(pipeline.node_data_size(idx), Some(1000));
}

#[test]
fn test_node_data_size_cached() {
    let mut pipeline = StreamPipeline::new(PipelineConfig::default());
    let idx = pipeline.add_cached(test_addr("c"), Bytes::from(vec![0u8; 5000]));
    assert_eq!(pipeline.node_data_size(idx), Some(5000));
}

#[test]
fn test_node_data_size_streaming_stage_unknown() {
    let mut pipeline = StreamPipeline::new(PipelineConfig::default());
    let src = pipeline.add_source(test_addr("a"), Bytes::from("x"));
    let stage = pipeline.add_streaming_stage(
        test_addr("s"), Box::new(StreamingIdentity),
        HashMap::new(), vec![src],
    );
    assert_eq!(pipeline.node_data_size(stage), None);
}

#[test]
fn test_node_data_size_batch_stage_unknown() {
    let mut pipeline = StreamPipeline::new(PipelineConfig::default());
    let src = pipeline.add_source(test_addr("a"), Bytes::from("x"));
    let stage = pipeline.add_batch_stage(
        test_addr("b"), Box::new(IdentityFn),
        HashMap::new(), vec![src],
    );
    assert_eq!(pipeline.node_data_size(stage), None);
}
```

### 5.2 Unit Tests — total_input_size

```rust
#[test]
fn test_total_input_size_all_known() {
    let mut pipeline = StreamPipeline::new(PipelineConfig::default());
    let a = pipeline.add_source(test_addr("a"), Bytes::from(vec![0u8; 1000]));
    let b = pipeline.add_source(test_addr("b"), Bytes::from(vec![0u8; 2000]));
    assert_eq!(pipeline.total_input_size(&[a, b]), Some(3000));
}

#[test]
fn test_total_input_size_one_unknown() {
    let mut pipeline = StreamPipeline::new(PipelineConfig::default());
    let a = pipeline.add_source(test_addr("a"), Bytes::from(vec![0u8; 1000]));
    let b = pipeline.add_streaming_stage(
        test_addr("s"), Box::new(StreamingIdentity),
        HashMap::new(), vec![a],
    );
    // a is known, but b (computed) is unknown → total is None
    let c = pipeline.add_source(test_addr("c"), Bytes::from(vec![0u8; 500]));
    assert_eq!(pipeline.total_input_size(&[b, c]), None);
}

#[test]
fn test_total_input_size_empty() {
    let pipeline = StreamPipeline::new(PipelineConfig::default());
    assert_eq!(pipeline.total_input_size(&[]), Some(0));
}
```

### 5.3 Unit Tests — Threshold Selection Logic

```rust
/// Helper: build a pipeline with one recipe node and return which
/// PipelineNode variant was selected (Streaming or Batch).
fn select_mode_for_size(
    input_bytes: usize,
    threshold: usize,
    has_streaming: bool,
    has_batch: bool,
) -> &'static str { /* returns "streaming" or "batch" */ }

#[test]
fn test_below_threshold_selects_batch() {
    // 1 MB input, 3 MB threshold, both impls available
    assert_eq!(
        select_mode_for_size(1_000_000, 3 * 1024 * 1024, true, true),
        "batch"
    );
}

#[test]
fn test_above_threshold_selects_streaming() {
    // 4 MB input, 3 MB threshold, both impls available
    assert_eq!(
        select_mode_for_size(4 * 1024 * 1024, 3 * 1024 * 1024, true, true),
        "streaming"
    );
}

#[test]
fn test_at_threshold_selects_streaming() {
    // Exactly 3 MB → >= threshold → streaming
    assert_eq!(
        select_mode_for_size(3 * 1024 * 1024, 3 * 1024 * 1024, true, true),
        "streaming"
    );
}

#[test]
fn test_below_threshold_no_batch_impl_falls_back_to_streaming() {
    // 1 MB, prefer batch, but no batch impl → streaming fallback
    assert_eq!(
        select_mode_for_size(1_000_000, 3 * 1024 * 1024, true, false),
        "streaming"
    );
}

#[test]
fn test_above_threshold_no_streaming_impl_falls_back_to_batch() {
    // 4 MB, prefer streaming, but no streaming impl → batch fallback
    assert_eq!(
        select_mode_for_size(4 * 1024 * 1024, 3 * 1024 * 1024, false, true),
        "batch"
    );
}

#[test]
fn test_threshold_zero_always_streaming() {
    // threshold=0 → always prefer streaming regardless of size
    assert_eq!(
        select_mode_for_size(100, 0, true, true),
        "streaming"
    );
}

#[test]
fn test_threshold_max_always_batch() {
    // threshold=usize::MAX → always prefer batch
    assert_eq!(
        select_mode_for_size(100 * 1024 * 1024, usize::MAX, true, true),
        "batch"
    );
}

#[test]
fn test_empty_input_selects_streaming() {
    // 0 bytes → streaming (negligible either way, streaming is safe)
    assert_eq!(
        select_mode_for_size(0, 3 * 1024 * 1024, true, true),
        "streaming"
    );
}
```

### 5.4 Unit Tests — Unknown Size Defaults to Streaming

```rust
#[tokio::test]
async fn test_unknown_size_input_selects_streaming() {
    // Build pipeline: Source(A) → StreamingStage(f) → recipe_node(g)
    // recipe_node(g) has computed input (unknown size) → must select streaming
    let mut registry = FunctionRegistry::new();
    registry.register("g", "v1", Box::new(IdentityFn));
    registry.register_streaming("g", "v1", Box::new(StreamingIdentity));

    let config = PipelineConfig {
        streaming_threshold: 3 * 1024 * 1024,
        ..Default::default()
    };

    // g's input is output of f (computed, unknown size)
    // Even though f's output might be small, unknown → streaming
    let executor = StreamingExecutor::new(config);
    // ... build DAG with A → f → g, materialize g
    // Assert g was added as StreamingStage (not BatchStage)
}
```

### 5.5 Integration Tests — End-to-End Mode Selection

```rust
#[tokio::test]
async fn test_e2e_small_input_uses_batch_path() {
    // 100 KB input through identity function (both impls registered)
    // threshold = 3 MB → should use batch
    let data = Bytes::from(vec![b'x'; 100 * 1024]);
    let config = PipelineConfig {
        streaming_threshold: 3 * 1024 * 1024,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());

    // Simulate selection: input size 100 KB < 3 MB → batch
    let idx = pipeline.add_batch_stage(
        test_addr("id"), Box::new(IdentityFn),
        HashMap::new(), vec![src],
    );

    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result, data);
}

#[tokio::test]
async fn test_e2e_large_input_uses_streaming_path() {
    // 8 MB input through identity function
    // threshold = 3 MB → should use streaming
    let data = Bytes::from(vec![b'x'; 8 * 1024 * 1024]);
    let config = PipelineConfig {
        streaming_threshold: 3 * 1024 * 1024,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());

    let idx = pipeline.add_streaming_stage(
        test_addr("id"), Box::new(StreamingIdentity),
        HashMap::new(), vec![src],
    );

    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result, data);
}

#[tokio::test]
async fn test_e2e_mixed_pipeline_batch_then_streaming() {
    // A = 500 KB → uppercase (batch) → sha256 (streaming, unknown input)
    let data = Bytes::from(vec![b'a'; 500 * 1024]);
    let config = PipelineConfig {
        streaming_threshold: 3 * 1024 * 1024,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data);

    // uppercase: 500 KB < 3 MB → batch
    let upper_idx = pipeline.add_batch_stage(
        test_addr("upper"), Box::new(UppercaseFn),
        HashMap::new(), vec![src],
    );
    // sha256: input is computed (unknown) → streaming
    let _hash_idx = pipeline.add_streaming_stage(
        test_addr("hash"), Box::new(StreamingSha256),
        HashMap::new(), vec![upper_idx],
    );

    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result.len(), 32); // SHA-256 digest

    // Verify correctness: hash of uppercased input
    use sha2::{Sha256, Digest};
    let expected = Sha256::digest(&vec![b'A'; 500 * 1024]);
    assert_eq!(&result[..], &expected[..]);
}
```

### 5.6 Benchmark — Verify Crossover Point

```rust
/// Parameterized benchmark: run identity pipeline at various input sizes
/// with both forced-batch and forced-streaming, verify crossover.
#[bench]
fn bench_mode_selection_crossover(b: &mut Bencher) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let sizes = [
        512 * 1024,       // 512 KB
        1024 * 1024,      // 1 MB
        2 * 1024 * 1024,  // 2 MB
        3 * 1024 * 1024,  // 3 MB
        4 * 1024 * 1024,  // 4 MB
        8 * 1024 * 1024,  // 8 MB
    ];

    for &size in &sizes {
        let data = Bytes::from(vec![0u8; size]);

        // Batch path
        let batch_time = rt.block_on(async {
            let mut pipeline = StreamPipeline::new(PipelineConfig {
                streaming_threshold: usize::MAX, // force batch
                ..Default::default()
            });
            let src = pipeline.add_source(test_addr("a"), data.clone());
            pipeline.add_batch_stage(
                test_addr("id"), Box::new(IdentityFn),
                HashMap::new(), vec![src],
            );
            let start = Instant::now();
            let rx = pipeline.execute().await.unwrap();
            let _ = collect_stream(rx).await.unwrap();
            start.elapsed()
        });

        // Streaming path
        let stream_time = rt.block_on(async {
            let mut pipeline = StreamPipeline::new(PipelineConfig {
                streaming_threshold: 0, // force streaming
                ..Default::default()
            });
            let src = pipeline.add_source(test_addr("a"), data.clone());
            pipeline.add_streaming_stage(
                test_addr("id"), Box::new(StreamingIdentity),
                HashMap::new(), vec![src],
            );
            let start = Instant::now();
            let rx = pipeline.execute().await.unwrap();
            let _ = collect_stream(rx).await.unwrap();
            start.elapsed()
        });

        println!(
            "size={:>7} KB  batch={:>8.2?}  stream={:>8.2?}  winner={}",
            size / 1024, batch_time, stream_time,
            if batch_time < stream_time { "batch" } else { "stream" },
        );
    }
}
```

### 5.7 Concurrency — Mode Selection Under Parallel Materialization

```rust
#[tokio::test]
async fn test_concurrent_materializations_different_modes() {
    // Two concurrent materializations: one small (batch), one large (streaming).
    // Verify they don't interfere — each picks the correct mode independently.
    let registry = make_dual_registry(); // identity registered as both batch+streaming
    let config = PipelineConfig {
        streaming_threshold: 3 * 1024 * 1024,
        ..Default::default()
    };

    let small_data = Bytes::from(vec![b'a'; 500 * 1024]);  // 500 KB → batch
    let large_data = Bytes::from(vec![b'b'; 8 * 1024 * 1024]); // 8 MB → streaming

    let (small_result, large_result) = tokio::join!(
        materialize_with_config(&registry, small_data.clone(), config.clone()),
        materialize_with_config(&registry, large_data.clone(), config.clone()),
    );

    assert_eq!(small_result.unwrap(), small_data);
    assert_eq!(large_result.unwrap(), large_data);
}

#[tokio::test]
async fn test_concurrent_gc_does_not_corrupt_mode_selection() {
    // Start a materialization, run GC concurrently.
    // Mode selection reads DAG under lock — GC holds write lock.
    // Verify materialization completes correctly after GC releases lock.
    let (dag, blob_store, cache, pins, registry) = setup_full_state();
    let addr_a = blob_store.put(&Bytes::from(vec![0u8; 1_000_000])).unwrap();
    let recipe_addr = insert_recipe(&dag, "identity", vec![addr_a]);

    let config = PipelineConfig {
        streaming_threshold: 3 * 1024 * 1024,
        ..Default::default()
    };

    let gc_handle = tokio::spawn({
        let dag = dag.clone();
        async move {
            run_gc_async(&dag, &blob_store, &cache, &pins, &GcConfig::default()).await
        }
    });

    let mat_handle = tokio::spawn(async move {
        let executor = StreamingExecutor::new(config);
        executor.materialize_streaming(&recipe_addr, &dag, &cache, &blob_store, &registry).await
    });

    let (gc_res, mat_res) = tokio::join!(gc_handle, mat_handle);
    assert!(gc_res.unwrap().is_ok());
    // Materialization succeeds — addr_a is live (referenced by recipe), not GC'd
    assert!(mat_res.unwrap().is_ok());
}
```

### 5.8 Regression — Streaming Correctness Preserved

```rust
#[tokio::test]
async fn test_backpressure_still_works_when_streaming_selected() {
    // 16 MB through 5-stage pipeline with channel_capacity=2.
    // Verify no data loss despite aggressive backpressure.
    // This ensures size-aware selection doesn't break §2.7 backpressure.
    let data = Bytes::from(vec![b'x'; 16 * 1024 * 1024]);
    let config = PipelineConfig {
        streaming_threshold: 3 * 1024 * 1024, // 16 MB > threshold → streaming
        channel_capacity: 2,                    // aggressive backpressure
        chunk_size: 16 * 1024,                  // 16 KB chunks → 1024 chunks
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());
    let mut prev = src;
    for i in 0..5 {
        prev = pipeline.add_streaming_stage(
            test_addr(&format!("stage_{}", i)),
            Box::new(StreamingIdentity),
            HashMap::new(),
            vec![prev],
        );
    }
    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result.len(), 16 * 1024 * 1024);
    assert_eq!(result, data);
}

#[tokio::test]
async fn test_cache_after_collect_works_for_batch_selected_node() {
    // When batch is selected, the result is wrapped via batch_to_stream().
    // Verify the tee+cache-after-collect path (§2.7 §3.8) still works.
    let data = Bytes::from(vec![b'z'; 200 * 1024]); // 200 KB → batch
    let config = PipelineConfig {
        streaming_threshold: 3 * 1024 * 1024,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());
    pipeline.add_batch_stage(
        test_addr("upper"), Box::new(UppercaseFn),
        HashMap::new(), vec![src],
    );
    let rx = pipeline.execute().await.unwrap();
    let (client_rx, cache_rx) = tee_stream(rx, 8);

    let (client_result, cache_result) = tokio::join!(
        collect_stream(client_rx),
        collect_stream(cache_rx),
    );

    let expected = data.iter().map(|b| b.to_ascii_uppercase()).collect::<Vec<_>>();
    assert_eq!(client_result.unwrap(), Bytes::from(expected.clone()));
    assert_eq!(cache_result.unwrap(), Bytes::from(expected));
}
```

### 5.9 Edge Case — Threshold Boundary Behaviour

```rust
#[tokio::test]
async fn test_one_byte_below_threshold_selects_batch() {
    let threshold = 3 * 1024 * 1024;
    let data = Bytes::from(vec![0u8; threshold - 1]);
    let mode = determine_mode_for_leaf_input(&data, threshold, true, true);
    assert_eq!(mode, "batch");
}

#[tokio::test]
async fn test_one_byte_at_threshold_selects_streaming() {
    let threshold = 3 * 1024 * 1024;
    let data = Bytes::from(vec![0u8; threshold]);
    let mode = determine_mode_for_leaf_input(&data, threshold, true, true);
    assert_eq!(mode, "streaming");
}

#[tokio::test]
async fn test_multi_input_sum_crosses_threshold() {
    // Each input is below threshold individually, but sum exceeds it.
    // A = 2 MB, B = 2 MB → total 4 MB > 3 MB → streaming
    let threshold = 3 * 1024 * 1024;
    let mut pipeline = StreamPipeline::new(PipelineConfig {
        streaming_threshold: threshold,
        ..Default::default()
    });
    let a = pipeline.add_source(test_addr("a"), Bytes::from(vec![0u8; 2 * 1024 * 1024]));
    let b = pipeline.add_source(test_addr("b"), Bytes::from(vec![0u8; 2 * 1024 * 1024]));
    assert_eq!(pipeline.total_input_size(&[a, b]), Some(4 * 1024 * 1024));
    // 4 MB >= 3 MB → streaming
}

#[tokio::test]
async fn test_single_unknown_among_many_known_forces_streaming() {
    // 5 inputs: 4 known (each 100 KB), 1 computed (unknown).
    // Even though known total is only 400 KB, unknown → None → streaming.
    let mut pipeline = StreamPipeline::new(PipelineConfig::default());
    let mut indices = Vec::new();
    for i in 0..4 {
        indices.push(pipeline.add_source(
            test_addr(&format!("s{}", i)),
            Bytes::from(vec![0u8; 100 * 1024]),
        ));
    }
    let computed = pipeline.add_streaming_stage(
        test_addr("comp"), Box::new(StreamingIdentity),
        HashMap::new(), vec![indices[0]],
    );
    // Replace last index with computed node
    indices.push(computed);
    assert_eq!(pipeline.total_input_size(&indices), None);
}
```

### 5.10 Pipeline Topology — Deep Chain Mode Selection

```rust
#[tokio::test]
async fn test_deep_chain_first_node_batch_rest_streaming() {
    // Chain: A → f1 → f2 → f3 → f4 → f5
    // A = 1 MB leaf. f1 sees known input (1 MB < 3 MB) → batch.
    // f2..f5 see computed inputs (unknown) → streaming.
    // Verify entire chain produces correct result.
    let data = Bytes::from(vec![b'a'; 1024 * 1024]);
    let config = PipelineConfig {
        streaming_threshold: 3 * 1024 * 1024,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());

    // f1: batch (known input 1 MB < threshold)
    let mut prev = pipeline.add_batch_stage(
        test_addr("f1"), Box::new(UppercaseFn),
        HashMap::new(), vec![src],
    );
    // f2..f5: streaming (unknown input from computed nodes)
    for i in 2..=5 {
        prev = pipeline.add_streaming_stage(
            test_addr(&format!("f{}", i)),
            Box::new(StreamingIdentity),
            HashMap::new(), vec![prev],
        );
    }

    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    // uppercase of 'a' repeated 1M times
    assert_eq!(result.len(), 1024 * 1024);
    assert!(result.iter().all(|&b| b == b'A'));
}

#[tokio::test]
async fn test_diamond_dag_mixed_modes() {
    // Diamond: A → f1 → f3
    //          A → f2 → f3
    // A = 2 MB. f1 and f2 both see 2 MB (< 3 MB) → batch.
    // f3 sees two computed inputs (unknown) → streaming.
    // Note: requires fan-out support or two separate Source nodes.
    let data = Bytes::from(vec![b'x'; 2 * 1024 * 1024]);
    let config = PipelineConfig {
        streaming_threshold: 3 * 1024 * 1024,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src_a1 = pipeline.add_source(test_addr("a1"), data.clone());
    let src_a2 = pipeline.add_source(test_addr("a2"), data.clone());

    let f1 = pipeline.add_batch_stage(
        test_addr("f1"), Box::new(UppercaseFn),
        HashMap::new(), vec![src_a1],
    );
    let f2 = pipeline.add_batch_stage(
        test_addr("f2"), Box::new(IdentityFn),
        HashMap::new(), vec![src_a2],
    );
    // f3: concat two computed inputs (both unknown) → streaming
    let _f3 = pipeline.add_streaming_stage(
        test_addr("f3"), Box::new(StreamingConcat),
        HashMap::new(), vec![f1, f2],
    );

    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    // uppercase(A) ++ identity(A) = 2 MB of 'X' + 2 MB of 'x'
    assert_eq!(result.len(), 4 * 1024 * 1024);
    assert!(result[..2 * 1024 * 1024].iter().all(|&b| b == b'X'));
    assert!(result[2 * 1024 * 1024..].iter().all(|&b| b == b'x'));
}
```

### 5.11 Metric Verification

```rust
#[tokio::test]
async fn test_metrics_batch_below_threshold() {
    let before = MODE_SELECTION
        .with_label_values(&["batch", "below_threshold"])
        .get();

    // Materialize 500 KB input with both impls available, threshold 3 MB
    run_materialization(500 * 1024, 3 * 1024 * 1024).await;

    let after = MODE_SELECTION
        .with_label_values(&["batch", "below_threshold"])
        .get();
    assert_eq!(after - before, 1);
}

#[tokio::test]
async fn test_metrics_streaming_no_batch_impl() {
    let before = MODE_SELECTION
        .with_label_values(&["streaming", "no_batch_impl"])
        .get();

    // Materialize 500 KB with streaming-only function, threshold 3 MB
    // Prefers batch but no batch impl → fallback to streaming
    run_materialization_streaming_only(500 * 1024, 3 * 1024 * 1024).await;

    let after = MODE_SELECTION
        .with_label_values(&["streaming", "no_batch_impl"])
        .get();
    assert_eq!(after - before, 1);
}

#[tokio::test]
async fn test_threshold_gauge_reflects_config() {
    let config = PipelineConfig {
        streaming_threshold: 5 * 1024 * 1024,
        ..Default::default()
    };
    let _executor = StreamingExecutor::new(config);
    assert_eq!(
        STREAMING_THRESHOLD_GAUGE.get(),
        5 * 1024 * 1024_i64,
    );
}
```

### 5.12 Error Paths

```rust
#[tokio::test]
async fn test_no_impl_at_all_returns_error() {
    // Function key has neither batch nor streaming impl → error
    let mut registry = FunctionRegistry::new();
    // Don't register "missing_fn"
    let config = PipelineConfig::default();
    let executor = StreamingExecutor::new(config);

    let result = executor.materialize_streaming(
        &test_addr("recipe_using_missing_fn"),
        &dag_with_recipe("missing_fn", "v1"),
        &empty_cache(),
        &empty_blob_store(),
        &registry,
    ).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("function not found"));
}

#[tokio::test]
async fn test_batch_error_propagates_through_stream_wrapper() {
    // Batch function returns error → batch_to_stream should propagate.
    // Verify the error surfaces to the pipeline consumer.
    let mut pipeline = StreamPipeline::new(PipelineConfig {
        streaming_threshold: usize::MAX, // force batch
        ..Default::default()
    });
    let src = pipeline.add_source(test_addr("a"), Bytes::from("input"));
    pipeline.add_batch_stage(
        test_addr("fail"), Box::new(AlwaysFailFn),
        HashMap::new(), vec![src],
    );

    let result = pipeline.execute().await;
    assert!(result.is_err());
}
```

### Test Summary Table

| Section | # | Tests | What is verified |
|---------|---|-------|------------------|
| §5.1 | 4 | `node_data_size` | Source/Cached return known size, computed nodes return None |
| §5.2 | 3 | `total_input_size` | Sum of known sizes, None propagation, empty input list |
| §5.3 | 8 | Threshold selection | Below/above/at threshold, fallbacks, threshold=0/MAX, empty input |
| §5.4 | 1 | Unknown size | Computed input defaults to streaming |
| §5.5 | 3 | End-to-end | Small→batch, large→streaming, mixed pipeline correctness |
| §5.6 | 1 | Benchmark | Crossover verification at 6 input sizes |
| §5.7 | 2 | Concurrency | Parallel materializations, GC + materialization race |
| §5.8 | 2 | Regression | Backpressure preserved, cache-after-collect with batch node |
| §5.9 | 4 | Boundary | threshold±1 byte, multi-input sum crossing, unknown among known |
| §5.10 | 2 | Topology | Deep 5-stage chain mixed modes, diamond DAG mixed modes |
| §5.11 | 3 | Metrics | batch counter, streaming fallback counter, threshold gauge |
| §5.12 | 2 | Errors | Missing function, batch error propagation |
| **Total** | **35** | | |

---

## 6. Edge Cases & Error Handling

| Case | Behaviour | Rationale |
|------|-----------|-----------|
| Input size = 0 bytes | Streaming selected | Empty input is negligible either way; streaming is the safe universal path |
| Input size = 1 byte | Batch selected (below threshold) | Batch avoids channel/task overhead for trivially small data |
| Input size = threshold − 1 | Batch selected | Strict `>=` comparison; one byte below is still "small" |
| Input size = threshold exactly | Streaming selected | `>=` threshold means at-threshold gets streaming benefit |
| `streaming_threshold = 0` | Always streaming | Disables size-aware selection; restores §2.7 behaviour |
| `streaming_threshold = usize::MAX` | Always batch | Forces batch for all sizes; useful for debugging or memory-constrained batch-only deployments |
| Multi-input: all small, sum exceeds threshold | Streaming selected | Sum-based check catches cases where individual inputs are small but aggregate is large (e.g., concat of 100 × 50 KB = 5 MB) |
| Multi-input: all small, sum below threshold | Batch selected | Aggregate is genuinely small; batch is faster |
| Multi-input: one input is computed (unknown size) | Streaming selected | Conservative: any unknown → `None` → streaming. Cannot guarantee aggregate fits in memory |
| Only streaming impl registered | Streaming regardless of size | Fallback: no batch to prefer, streaming is the only option. Metric: `{streaming, no_batch_impl}` |
| Only batch impl registered | Batch regardless of size | Fallback: no streaming to prefer, batch is the only option. Metric: `{batch, no_streaming_impl}` |
| Neither impl registered | `DerivaError::Compute` | Hard error — function key not found in registry |
| Cached intermediate node as input | Size known from cache | `Cached` variant returns `data.len()`; enables batch selection for cache-hit subtrees |
| Pipeline with all batch stages | Works — no streaming overhead | Every stage collects inputs and calls `execute()` directly; `batch_to_stream()` wraps each result for the next stage's input channel |
| Pipeline with single node | Selection still applies | Even a 1-stage pipeline benefits: batch avoids 1 task spawn + 1 channel for small inputs |
| Threshold changed between materializations | New threshold applies to next pipeline | `PipelineConfig` is per-pipeline; changing `StreamingExecutor.config` affects subsequent calls only |
| Very large number of inputs (100+) | Sum may overflow | Use `checked_add` in `total_input_size()`; on overflow return `None` (treat as unknown → streaming) |
| Race: blob deleted between size check and execution | Batch `execute()` fails with missing input | Same as §2.7 — not a new failure mode. GC grace period (§2.8) prevents this in practice |

### 6.1 Overflow Protection in total_input_size

```rust
pub fn total_input_size(&self, input_indices: &[usize]) -> Option<usize> {
    let mut total = 0usize;
    for &idx in input_indices {
        let size = self.node_data_size(idx)?;
        total = total.checked_add(size)?; // None on overflow → streaming
    }
    Some(total)
}
```

If a node has 100 inputs each claiming `usize::MAX / 50` bytes, the sum
overflows. `checked_add` returns `None`, which the selector treats as
unknown size → streaming. This is correct: if the aggregate is that large,
streaming is the right choice anyway.

---

## 7. Performance Analysis

### 7.1 Expected Improvement

Based on the Paper 2b crossover data (5-stage identity pipeline, 64 KB
chunks, channel capacity 8):

```
Input Size   Batch     Streaming   Batch Speedup   Size-Aware Picks
─────────────────────────────────────────────────────────────────────
  512 KB     0.09 ms    1.21 ms       13.4×         batch ✓
    1 MB     0.17 ms    1.59 ms        9.3×         batch ✓
    2 MB     0.41 ms    2.36 ms        5.7×         batch ✓
    3 MB     1.12 ms    2.68 ms        2.4×         streaming (at threshold)
    4 MB     4.18 ms    3.03 ms        0.7×         streaming ✓
    8 MB     9.84 ms    4.72 ms        0.5×         streaming ✓
   16 MB    22.1  ms    7.75 ms        0.4×         streaming ✓
   64 MB   130.3  ms   50.8  ms        0.4×         streaming ✓

Size-aware result: optimal or near-optimal at every size.
Worst case: at exactly 3 MB, streaming is 2.4× slower than batch.
  This is the threshold boundary — could be tuned to 3.5 MB to
  capture this case, but the improvement is marginal.
```

Weighted improvement for a typical mixed workload:

```
Workload distribution (from §1.3):
  60% requests < 1 MB:   was 9.3× slower → now batch speed
  15% requests 1–3 MB:   was 2.4–5.7× slower → now batch speed
  15% requests 3–10 MB:  was optimal → unchanged
  10% requests > 10 MB:  was optimal → unchanged

Weighted latency improvement:
  Before: 0.60 × 1.59 + 0.15 × 2.36 + 0.15 × 3.03 + 0.10 × 4.72 = 2.23 ms avg
  After:  0.60 × 0.17 + 0.15 × 0.41 + 0.15 × 3.03 + 0.10 × 4.72 = 1.09 ms avg
  Improvement: 2.05× faster for the average request
```

### 7.2 Overhead of Size Check

The size-aware selection adds per-node overhead:

```
Per-node cost:
  node_data_size():     1 match arm → ~2 ns
  total_input_size():   N calls to node_data_size + N additions
                        For N=3 inputs: ~10 ns
  Comparison:           1 branch → ~1 ns
  Metric increment:     1 atomic add → ~5 ns
  ─────────────────────────────────────────
  Total per node:       ~18 ns

For a 5-stage pipeline:  5 × 18 ns = 90 ns
For a 20-stage pipeline: 20 × 18 ns = 360 ns

Context: streaming pipeline execution for 1 MB takes ~1.59 ms.
  90 ns overhead = 0.006% of execution time.
  Negligible.
```

### 7.3 Memory Comparison

```
Scenario: 1 MB input through 5-stage pipeline

  Always-streaming (§2.7):
    5 channels × 8 capacity × 64 KB = 2.5 MB buffered
    + 5 Tokio task stacks (~8 KB each) = 40 KB
    + input data in flight = ~64 KB
    Total: ~2.6 MB

  Size-aware batch (§2.9):
    Input: 1 MB (collected once)
    Per-stage: input + output = 2 MB peak (released after execute)
    No channels, no task stacks
    Total peak: ~2 MB

  Savings: ~0.6 MB (23% less)
  More importantly: no Tokio task scheduling overhead.

Scenario: 64 MB input through 5-stage pipeline

  Both modes select streaming → identical memory profile:
    5 × 8 × 64 KB = 2.5 MB (bounded, regardless of input size)
```

### 7.4 Threshold Sensitivity Analysis

```
What happens if the threshold is set incorrectly?

  Threshold too low (e.g., 512 KB):
    Inputs 512 KB–3 MB use streaming unnecessarily.
    Penalty: 2.4–9.3× slower for those inputs.
    But: no correctness issue, just suboptimal performance.

  Threshold too high (e.g., 16 MB):
    Inputs 3–16 MB use batch unnecessarily.
    Penalty: batch is slower above 3 MB (1.4–2.9× at 4–16 MB).
    Also: batch requires O(input) memory vs streaming's O(chunk).
    Risk: OOM for very large inputs forced into batch.

  Threshold at 0 (disabled):
    Equivalent to §2.7 — always streaming.
    No regression, just misses the batch optimization.

  Conclusion: the default 3 MB is conservative. Setting it anywhere
  in the 2–4 MB range produces similar results. The penalty for
  being wrong is performance, not correctness.
```

### 7.5 Benchmark Study Plan

Location: `deriva-compute/benches/mode_crossover.rs`

Five benchmark groups that together validate the crossover point, quantify
the improvement, and detect regressions.

#### Benchmark 1: Crossover Sweep — Batch vs Streaming vs Size-Aware

Isolates the mode selection benefit by running the same pipeline under
three configurations at 10 input sizes.

```rust
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn bench_crossover_sweep(c: &mut Criterion) {
    let mut group = c.benchmark_group("crossover_sweep");
    let sizes: Vec<usize> = vec![
        256 * 1024,       // 256 KB
        512 * 1024,       // 512 KB
        1024 * 1024,      // 1 MB
        2 * 1024 * 1024,  // 2 MB
        3 * 1024 * 1024,  // 3 MB (threshold)
        4 * 1024 * 1024,  // 4 MB
        6 * 1024 * 1024,  // 6 MB
        8 * 1024 * 1024,  // 8 MB
        16 * 1024 * 1024, // 16 MB
        32 * 1024 * 1024, // 32 MB
    ];

    for &size in &sizes {
        let data = Bytes::from(vec![0u8; size]);
        group.throughput(Throughput::Bytes(size as u64));

        // (a) Force batch: threshold = usize::MAX
        group.bench_with_input(
            BenchmarkId::new("batch", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_5_stage_identity(data.clone(), PipelineConfig {
                        streaming_threshold: usize::MAX,
                        ..Default::default()
                    }).await
                });
            },
        );

        // (b) Force streaming: threshold = 0
        group.bench_with_input(
            BenchmarkId::new("streaming", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_5_stage_identity(data.clone(), PipelineConfig {
                        streaming_threshold: 0,
                        ..Default::default()
                    }).await
                });
            },
        );

        // (c) Size-aware: threshold = 3 MB (default)
        group.bench_with_input(
            BenchmarkId::new("size_aware", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_5_stage_identity(data.clone(), PipelineConfig {
                        streaming_threshold: DEFAULT_STREAMING_THRESHOLD,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected output:** Size-aware matches batch below 3 MB and matches
streaming above 3 MB — always within 5% of the faster mode.

#### Benchmark 2: Pipeline Depth Sensitivity

Tests whether the crossover point shifts with pipeline depth (1, 3, 5,
10, 20 stages). Deeper pipelines have more fixed overhead from task
spawning, which should push the crossover higher.

```rust
fn bench_depth_sensitivity(c: &mut Criterion) {
    let mut group = c.benchmark_group("depth_sensitivity");
    let depths = [1, 3, 5, 10, 20];
    let size = 2 * 1024 * 1024; // 2 MB — near crossover
    let data = Bytes::from(vec![0u8; size]);
    group.throughput(Throughput::Bytes(size as u64));

    for &depth in &depths {
        group.bench_with_input(
            BenchmarkId::new("batch", depth),
            &depth,
            |b, &depth| {
                b.to_async(&rt).iter(|| async {
                    run_n_stage_identity(data.clone(), depth, PipelineConfig {
                        streaming_threshold: usize::MAX,
                        ..Default::default()
                    }).await
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("streaming", depth),
            &depth,
            |b, &depth| {
                b.to_async(&rt).iter(|| async {
                    run_n_stage_identity(data.clone(), depth, PipelineConfig {
                        streaming_threshold: 0,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected output:** Crossover shifts from ~2 MB at depth=1 to ~4 MB at
depth=20 as streaming's fixed overhead grows linearly with depth.

#### Benchmark 3: Mixed Pipeline — Batch Stage Followed by Streaming

Measures the real-world scenario from §4.3: first stage selected as batch
(small known input), subsequent stages as streaming (unknown computed input).
Compares against all-streaming and all-batch baselines.

```rust
fn bench_mixed_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_pipeline");
    let sizes = [512 * 1024, 1024 * 1024, 2 * 1024 * 1024];

    for &size in &sizes {
        let data = Bytes::from(vec![b'a'; size]);
        group.throughput(Throughput::Bytes(size as u64));

        // All-streaming baseline
        group.bench_with_input(
            BenchmarkId::new("all_streaming", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    // uppercase(streaming) → sha256(streaming)
                    run_upper_then_hash_streaming(data.clone()).await
                });
            },
        );

        // Mixed: uppercase(batch) → sha256(streaming)
        group.bench_with_input(
            BenchmarkId::new("mixed_batch_stream", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_upper_batch_then_hash_streaming(data.clone()).await
                });
            },
        );

        // All-batch baseline
        group.bench_with_input(
            BenchmarkId::new("all_batch", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_upper_then_hash_batch(data.clone()).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected output:** Mixed mode is within 10% of all-batch for small
inputs (batch_to_stream wrapper overhead is minimal) and faster than
all-batch for large inputs.

#### Benchmark 4: Multi-Input Sum Threshold

Tests the sum-based threshold with varying input counts and sizes.
Validates that `total_input_size` correctly triggers streaming when
aggregate exceeds threshold even though individual inputs are small.

```rust
fn bench_multi_input_sum(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_input_sum");

    // Case A: 2 inputs × 1 MB = 2 MB < 3 MB → batch
    // Case B: 4 inputs × 1 MB = 4 MB > 3 MB → streaming
    // Case C: 10 inputs × 500 KB = 5 MB > 3 MB → streaming
    let cases: Vec<(&str, usize, usize)> = vec![
        ("2x1MB_batch",    2,  1024 * 1024),
        ("4x1MB_stream",   4,  1024 * 1024),
        ("10x500KB_stream", 10, 512 * 1024),
    ];

    for (label, count, per_input_size) in &cases {
        let inputs: Vec<Bytes> = (0..*count)
            .map(|_| Bytes::from(vec![0u8; *per_input_size]))
            .collect();
        let total = count * per_input_size;
        group.throughput(Throughput::Bytes(total as u64));

        group.bench_function(
            BenchmarkId::new("size_aware", label),
            |b| {
                b.to_async(&rt).iter(|| async {
                    run_concat_n_inputs(inputs.clone(), PipelineConfig {
                        streaming_threshold: DEFAULT_STREAMING_THRESHOLD,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected output:** 2×1MB uses batch (fast), 4×1MB and 10×500KB use
streaming (correct for aggregate size).

#### Benchmark 5: Selection Overhead Isolation

Measures the pure overhead of the size-aware decision logic by comparing
a no-op pipeline (source → collect, no compute stages) with and without
the selection code path. This isolates the cost of `node_data_size()`,
`total_input_size()`, comparison, and metric increment.

```rust
fn bench_selection_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("selection_overhead");
    let data = Bytes::from(vec![0u8; 1024]); // 1 KB — trivial payload
    group.throughput(Throughput::Bytes(1024));

    // (a) Direct batch execute — no selection logic
    group.bench_function("direct_batch", |b| {
        b.to_async(&rt).iter(|| async {
            let identity = IdentityFn;
            identity.execute(&[data.clone()], &HashMap::new()).unwrap()
        });
    });

    // (b) Through pipeline with size-aware selection
    group.bench_function("pipeline_size_aware", |b| {
        b.to_async(&rt).iter(|| async {
            run_1_stage_identity(data.clone(), PipelineConfig {
                streaming_threshold: DEFAULT_STREAMING_THRESHOLD,
                ..Default::default()
            }).await
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_crossover_sweep,
    bench_depth_sensitivity,
    bench_mixed_pipeline,
    bench_multi_input_sum,
    bench_selection_overhead,
);
criterion_main!(benches);
```

**Expected output:** Overhead of pipeline + selection is <1 μs above
direct batch call for 1 KB input, confirming negligible decision cost.

#### Benchmark Summary

| # | Group | Parameters | Measures | Expected Result |
|---|-------|-----------|----------|-----------------|
| 1 | Crossover sweep | 10 sizes × 3 modes | Latency, throughput | Size-aware ≤ min(batch, streaming) + 5% |
| 2 | Depth sensitivity | 5 depths × 2 modes | Crossover shift | Crossover rises ~0.1 MB per additional stage |
| 3 | Mixed pipeline | 3 sizes × 3 modes | Batch→streaming transition cost | Mixed within 10% of all-batch for small inputs |
| 4 | Multi-input sum | 3 input configurations | Sum-based threshold correctness | Correct mode selected per aggregate size |
| 5 | Selection overhead | 2 paths | Pure decision cost | <1 μs overhead |

Total: ~575 lines changed/added across 4 files.

---

## 9. Dependency Changes

No new crate dependencies. All functionality built on existing:

| Dependency | Already in | Used for |
|-----------|-----------|----------|
| `prometheus` | `deriva-compute` | `IntCounterVec`, `IntGauge` for mode selection metrics |
| `lazy_static` | `deriva-compute` | Static metric registration |
| `tokio` | `deriva-compute` | Existing async runtime (no new features) |
| `bytes` | `deriva-core` | Existing `Bytes` type (no new features) |

---

## 10. Design Rationale

### 10.1 Why a Static Threshold Instead of Adaptive?

An adaptive threshold would observe batch vs streaming latency at runtime
and adjust the crossover point. This was considered and rejected:

| Factor | Static threshold | Adaptive threshold |
|--------|-----------------|-------------------|
| Complexity | 1 comparison | Online learning algorithm, EMA, hysteresis |
| Predictability | Deterministic | Non-deterministic (depends on history) |
| Cold start | Works immediately | Needs warmup period with suboptimal decisions |
| Debugging | Trivial (check config) | Hard (why did it pick batch for 5 MB?) |
| Correctness risk | None | Feedback loop instability, oscillation |
| Improvement potential | Fixed at crossover | Could adapt to hardware/workload changes |

The static threshold captures 95%+ of the benefit with zero complexity.
Adaptive selection is deferred to §2.10 (Adaptive Chunk Sizing), which
addresses a related but more impactful tuning problem.

### 10.2 Why Default to Streaming for Unknown Sizes?

When input size is unknown (computed node), two options exist:

1. **Default to streaming** (chosen) — safe because streaming works for
   any size with bounded memory. If the actual size is small, we pay
   streaming overhead (~1.5 ms for a 5-stage pipeline). Worst case is
   performance, not correctness.

2. **Default to batch** — risky because if the actual size is large
   (e.g., 1 GB computed output), batch requires O(input) memory and
   may OOM. Worst case is a crash.

Streaming is the universally safe default. The performance penalty for
small computed outputs (~1 ms) is acceptable given the OOM risk of the
alternative.

### 10.3 Why Sum Input Sizes Instead of Max?

The threshold comparison uses `sum(input_sizes)` rather than
`max(input_sizes)`. Rationale:

```
Scenario: concat(A, B, C) where A=1MB, B=1MB, C=1MB
  max = 1 MB < 3 MB → would select batch
  sum = 3 MB ≥ 3 MB → selects streaming

Batch concat must hold all 3 inputs + output in memory:
  3 × 1 MB inputs + 3 MB output = 6 MB peak

Streaming concat processes sequentially with bounded buffers:
  1 channel × 8 × 64 KB = 512 KB peak

Sum correctly reflects the total memory pressure on the batch path.
Max would underestimate for multi-input functions, leading to
batch selection for workloads that benefit from streaming.
```

---

## 8. Files Changed

| File | Change | Lines |
|------|--------|-------|
| `deriva-compute/src/pipeline.rs` | Add `streaming_threshold` to `PipelineConfig`, `DEFAULT_STREAMING_THRESHOLD` constant, `node_data_size()`, `total_input_size()` | ~25 |
| `deriva-compute/src/executor.rs` | Replace unconditional streaming selection with size-aware 3-way fallback; add `MODE_SELECTION` counter and `STREAMING_THRESHOLD_GAUGE` | ~40 |
| `deriva-compute/tests/mode_selection.rs` | **NEW** — 35 tests: node size queries, threshold logic, concurrency, regression, boundary, topology, metrics, errors | ~450 |
| `deriva-compute/benches/mode_crossover.rs` | **NEW** — 5 benchmark groups: crossover sweep (10 sizes × 3 modes), depth sensitivity (5 depths), mixed pipeline, multi-input sum, selection overhead | ~250 |

---

## 9. Dependency Changes

No new crate dependencies. All functionality built on existing:

| Dependency | Already in | Used for |
|-----------|-----------|----------|
| `prometheus` | `deriva-compute` | `IntCounterVec`, `IntGauge` for mode selection metrics |
| `lazy_static` | `deriva-compute` | Static metric registration |
| `tokio` | `deriva-compute` | Existing async runtime (no new features) |
| `bytes` | `deriva-core` | Existing `Bytes` type (no new features) |
| `criterion` | `deriva-compute` (dev) | Benchmark framework (already used for §2.7 benchmarks) |

---

## 10. Design Rationale

### 10.1 Why a Static Threshold Instead of Adaptive?

An adaptive threshold would observe batch vs streaming latency at runtime
and adjust the crossover point. This was considered and rejected:

| Factor | Static threshold | Adaptive threshold |
|--------|-----------------|-------------------|
| Complexity | 1 comparison | Online learning algorithm, EMA, hysteresis |
| Predictability | Deterministic | Non-deterministic (depends on history) |
| Cold start | Works immediately | Needs warmup period with suboptimal decisions |
| Debugging | Trivial (check config) | Hard (why did it pick batch for 5 MB?) |
| Correctness risk | None | Feedback loop instability, oscillation |
| Improvement potential | Fixed at crossover | Could adapt to hardware/workload changes |

The static threshold captures 95%+ of the benefit with zero complexity.
Adaptive selection is deferred to §2.10 (Adaptive Chunk Sizing), which
addresses a related but more impactful tuning problem.

### 10.2 Why Default to Streaming for Unknown Sizes?

When input size is unknown (computed node), two options exist:

1. **Default to streaming** (chosen) — safe because streaming works for
   any size with bounded memory. If the actual size is small, we pay
   streaming overhead (~1.5 ms for a 5-stage pipeline). Worst case is
   performance, not correctness.

2. **Default to batch** — risky because if the actual size is large
   (e.g., 1 GB computed output), batch requires O(input) memory and
   may OOM. Worst case is a crash.

Streaming is the universally safe default. The performance penalty for
small computed outputs (~1 ms) is acceptable given the OOM risk of the
alternative.

### 10.3 Why Sum Input Sizes Instead of Max?

The threshold comparison uses `sum(input_sizes)` rather than
`max(input_sizes)`. Rationale:

```
Scenario: concat(A, B, C) where A=1MB, B=1MB, C=1MB
  max = 1 MB < 3 MB → would select batch
  sum = 3 MB ≥ 3 MB → selects streaming

Batch concat must hold all 3 inputs + output in memory:
  3 × 1 MB inputs + 3 MB output = 6 MB peak

Streaming concat processes sequentially with bounded buffers:
  1 channel × 8 × 64 KB = 512 KB peak

Sum correctly reflects the total memory pressure on the batch path.
Max would underestimate for multi-input functions, leading to
batch selection for workloads that benefit from streaming.
```

---

## 11. Observability Integration

Two new metrics (integrates with §2.5):

```rust
lazy_static! {
    /// Counts mode selection decisions, labelled by chosen mode and reason.
    ///
    /// Labels:
    ///   mode:   "batch" | "streaming"
    ///   reason: "below_threshold" | "above_threshold" | "unknown_size"
    ///           | "no_batch_impl" | "no_streaming_impl"
    static ref MODE_SELECTION: IntCounterVec = register_int_counter_vec!(
        "deriva_mode_selection_total",
        "Execution mode selections by mode and reason",
        &["mode", "reason"]
    ).unwrap();

    /// Exposes the configured streaming threshold for dashboard correlation.
    static ref STREAMING_THRESHOLD_GAUGE: IntGauge = register_int_gauge!(
        "deriva_streaming_threshold_bytes",
        "Configured streaming threshold in bytes"
    ).unwrap();
}
```

Dashboard queries:

```promql
# Ratio of batch vs streaming selections (should be ~75% batch for typical workloads)
sum(rate(deriva_mode_selection_total{mode="batch"}[5m]))
/
sum(rate(deriva_mode_selection_total[5m]))

# Functions frequently falling back due to missing batch impl
# (candidates for adding a batch implementation)
topk(5,
  sum by (reason) (rate(deriva_mode_selection_total{reason="no_batch_impl"}[1h]))
)

# Verify threshold is set correctly across fleet
deriva_streaming_threshold_bytes
```

---

## 12. Checklist

- [ ] Add `streaming_threshold` to `PipelineConfig` with default 3 MB
- [ ] Implement `node_data_size()` on `StreamPipeline`
- [ ] Implement `total_input_size()` with `checked_add` overflow protection
- [ ] Replace unconditional streaming selection in `StreamingExecutor::materialize_streaming()` with 3-way fallback
- [ ] Add `MODE_SELECTION` Prometheus counter with `[mode, reason]` labels
- [ ] Add `STREAMING_THRESHOLD_GAUGE` Prometheus gauge
- [ ] Unit tests: `node_data_size` (4 tests)
- [ ] Unit tests: `total_input_size` (3 tests)
- [ ] Unit tests: threshold selection logic (8 tests)
- [ ] Unit tests: unknown size defaults to streaming (1 test)
- [ ] Integration tests: end-to-end mode selection (3 tests)
- [ ] Concurrency tests: parallel materializations, GC race (2 tests)
- [ ] Regression tests: backpressure, cache-after-collect (2 tests)
- [ ] Boundary tests: threshold ±1, sum crossing, unknown among known (4 tests)
- [ ] Topology tests: deep chain, diamond DAG (2 tests)
- [ ] Metric tests: counter increments, gauge value (3 tests)
- [ ] Error tests: missing function, batch error propagation (2 tests)
- [ ] Benchmark: crossover verification at 6 input sizes (1 bench)
- [ ] Benchmark: crossover sweep — 10 sizes × 3 modes (batch/streaming/size-aware)
- [ ] Benchmark: depth sensitivity — 5 depths × 2 modes at 2 MB
- [ ] Benchmark: mixed pipeline — batch→streaming transition cost at 3 sizes
- [ ] Benchmark: multi-input sum — 3 input configurations validating aggregate threshold
- [ ] Benchmark: selection overhead isolation — direct batch vs pipeline path
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing tests still pass
- [ ] Commit and push
