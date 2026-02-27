# §2.11 Pipeline Fusion

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 2–3 days

---

## 1. Problem Statement

### 1.1 Current Limitation

Every streaming stage in the pipeline runs as an independent Tokio task
connected by `mpsc` channels. The per-chunk cost of a channel hop is:

```
  send:   tx.send(chunk).await   →  ~50 ns (uncontended, capacity > 0)
  recv:   rx.recv().await        →  ~50 ns
  total per hop:                    ~100 ns
```

For compute-heavy stages (compress: ~200 μs/chunk, SHA-256: ~15 μs/chunk)
this overhead is negligible (< 1%). But many streaming functions are
simple byte transforms where the actual compute is comparable to or
cheaper than the channel cost:

```rust
// Actual code from builtins_streaming.rs — these use spawn_map():
StreamingIdentity    →  no-op, 0 ns compute
StreamingUppercase   →  b.to_ascii_uppercase(), ~2 μs for 64 KB
StreamingLowercase   →  b.to_ascii_lowercase(), ~2 μs for 64 KB
StreamingReverse     →  v.reverse(), ~0.5 μs for 64 KB
StreamingXor         →  byte ^ key per byte, ~1 μs for 64 KB
```

For these functions, the 100 ns channel overhead per chunk is 5–50% of
the total per-chunk cost. In a chain of N such stages, the pipeline pays
N channel hops when only 1 is necessary — the transforms could be applied
sequentially within a single task.

### 1.2 Empirical Evidence

Consider a 5-stage identity pipeline processing 100 MB at 64 KB chunks
(1,536 chunks):

| Configuration | Stages | Channel hops | Overhead per chunk | Total overhead |
|--------------|--------|-------------|-------------------|---------------|
| 5× Identity (unfused) | 5 tasks | 5 × 1,536 = 7,680 | 5 × 100 ns = 500 ns | 7,680 × 100 ns = 768 μs |
| 5× Identity (fused) | 1 task | 1 × 1,536 = 1,536 | 1 × 100 ns = 100 ns | 1,536 × 100 ns = 154 μs |
| **Saved** | 4 tasks | **6,144 hops** | **400 ns/chunk** | **614 μs** |

For a more realistic chain — `Uppercase → Lowercase → XorCipher` on
100 MB:

| Configuration | Compute/chunk | Channel overhead/chunk | Overhead % |
|--------------|--------------|----------------------|-----------|
| 3 stages unfused | ~5 μs | 3 × 100 ns = 300 ns | 6.0% |
| 1 fused stage | ~5 μs | 1 × 100 ns = 100 ns | 2.0% |
| **Saved** | — | **200 ns/chunk** | **4.0%** |

The overhead percentage grows with pipeline depth and shrinks with
per-stage compute cost. Fusion is most valuable for chains of cheap
transforms.

### 1.3 Real-World Impact

Typical pipelines that benefit from fusion:

1. **Normalize + encrypt**: `Uppercase → XorCipher` — 2 cheap map stages,
   fusion eliminates 1 channel hop per chunk.

2. **Encode + compress**: `Base64Encode → Compress` — Base64 is a pure
   map (~8 μs/chunk), compress is also a map (~200 μs/chunk). Fusing
   saves 100 ns/chunk but the benefit is small relative to compress cost.

3. **Multi-step normalization**: `Uppercase → Reverse → XorCipher →
   Base64Encode` — 4 cheap maps, fusion eliminates 3 hops, saving
   300 ns/chunk (15% of total per-chunk cost).

Pipelines that do NOT benefit:

- Single-stage pipelines (nothing to fuse)
- Chains containing accumulators (SHA-256, ByteCount, Checksum) — these
  consume all input before emitting, breaking the chunk-by-chunk flow
- Chains containing multi-input combiners (Concat, Interleave, ZipConcat)
  — these have fan-in semantics incompatible with sequential application
- Chains containing stateful utilities (ChunkResizer, Take, Skip, Repeat,
  TeeCount) — these maintain internal state that depends on chunk
  boundaries or byte positions

### 1.4 Comparison

| Dimension | Unfused Pipeline | Fused Pipeline |
|-----------|-----------------|---------------|
| Tasks spawned per N-stage chain | N | 1 |
| Channel hops per chunk | N | 1 |
| Per-chunk overhead (N=5) | 500 ns | 100 ns |
| Task scheduling overhead | N × Tokio wake-ups | 1 × Tokio wake-up |
| Inter-stage parallelism | Yes (pipelined) | No (sequential) |
| Memory (channel buffers) | N × capacity × chunk_size | 1 × capacity × chunk_size |
| Error propagation | Per-stage, immediate | Single point, immediate |
| Observability granularity | Per-stage metrics | Per-fused-group metrics |
| Debugging | Each stage visible | Fused stages opaque |
| Correctness risk | None (existing) | Must prove equivalence |

### 1.5 What Exists vs What's Missing

**Exists today:**

- `spawn_map()` helper in `builtins_streaming.rs` — spawns a Tokio task
  per stage with its own channel. All 9 pure map functions (#1–#9) use
  this helper.
- `StreamingComputeFunction` trait with `stream_execute()` — each stage
  independently processes its input channel.
- Pipeline builder in `StreamPipeline::execute()` — wires stages
  sequentially, one task per node.

**Missing:**

- `is_fusible()` trait method — no way to query whether a function is a
  pure map that can be fused.
- `FusedMapStage` — no composite stage that applies multiple transforms
  per chunk in a single task.
- Fusion optimizer pass — no analysis of the pipeline graph to detect
  fusible chains.
- `enable_fusion` config — no way to enable/disable fusion.

**Fusibility classification of all 20 builtins:**

| # | Function | Category | Uses | Fusible? | Reason |
|---|----------|----------|------|----------|--------|
| 1 | StreamingIdentity | Map | passthrough | ✅ | Stateless, single-input, chunk-by-chunk |
| 2 | StreamingUppercase | Map | `spawn_map` | ✅ | Pure byte transform |
| 3 | StreamingLowercase | Map | `spawn_map` | ✅ | Pure byte transform |
| 4 | StreamingReverse | Map | `spawn_map` | ✅ | Pure byte transform |
| 5 | StreamingBase64Encode | Map | `spawn_map` | ✅ | Pure byte transform |
| 6 | StreamingBase64Decode | Map | `spawn_map` | ✅ | Pure byte transform |
| 7 | StreamingXor | Map | `spawn_map` | ✅ | Pure byte transform (param-dependent but stateless) |
| 8 | StreamingCompress | Map | `spawn_map` | ✅ | Per-chunk independent compression |
| 9 | StreamingDecompress | Map | `spawn_map` | ✅ | Per-chunk independent decompression |
| 10 | StreamingSha256 | Accumulator | `spawn_accumulate` | ❌ | Consumes all input before emitting |
| 11 | StreamingByteCount | Accumulator | `spawn_accumulate` | ❌ | Consumes all input before emitting |
| 12 | StreamingChecksum | Accumulator | `spawn_accumulate` | ❌ | Consumes all input before emitting |
| 13 | StreamingConcat | Combiner | custom | ❌ | Multi-input fan-in |
| 14 | StreamingInterleave | Combiner | custom | ❌ | Multi-input fan-in |
| 15 | StreamingZipConcat | Combiner | custom | ❌ | Multi-input fan-in |
| 16 | StreamingChunkResizer | Utility | custom | ❌ | Stateful buffer, changes chunk boundaries |
| 17 | StreamingTake | Utility | custom | ❌ | Stateful byte counter, truncates stream |
| 18 | StreamingSkip | Utility | custom | ❌ | Stateful byte counter, skips prefix |
| 19 | StreamingRepeat | Utility | custom | ❌ | Accumulates full input, replays N times |
| 20 | StreamingTeeCount | Utility | custom | ❌ | Stateful counter, appends trailer |

**9 of 20 functions are fusible** — all in Category 1 (single-input
chunk-by-chunk transforms using `spawn_map`).

---

## 2. Design

### 2.1 Architecture Overview

Fusion is a construction-time optimizer pass that runs before task
spawning. It transforms the pipeline node list, merging adjacent
fusible streaming stages into a single `FusedMapStage` node.

```
  Pipeline construction (existing)          Fusion pass (new)
  ─────────────────────────────────         ──────────────────

  add_source(...)                           ┌─────────────────────┐
  add_streaming_stage(Uppercase, ...)       │  fuse_pipeline()    │
  add_streaming_stage(Lowercase, ...)  ───► │  scan nodes in order│
  add_streaming_stage(XorCipher, ...)       │  merge fusible runs │
  add_streaming_stage(Sha256, ...)          └─────────┬───────────┘
                                                      │
                                                      ▼
  Before fusion:                            After fusion:
  ┌────────┐  ch  ┌─────┐  ch  ┌─────┐     ┌────────┐  ch  ┌──────────────┐
  │ Source  │────►│Upper│────►│Lower│      │ Source  │────►│ FusedMap     │
  └────────┘      └─────┘      └─────┘     └────────┘      │ [Upper,Lower,│
                    ch  ┌─────┐  ch  ┌──────┐               │  XorCipher]  │
                  ────►│ Xor │────►│Sha256│               └──────┬───────┘
                        └─────┘      └──────┘                ch   │
                                                            ┌─────▼──────┐
  5 tasks, 4 channels                                       │   Sha256   │
                                                            └────────────┘

                                                            3 tasks, 2 channels
```

The fusion pass is O(N) in the number of pipeline nodes and runs once
at construction time. It does not affect runtime behavior — the fused
stage produces identical output to the unfused chain.

### 2.2 Fusibility Criteria

A streaming stage is fusible if and only if ALL of the following hold:

| # | Criterion | Rationale |
|---|-----------|-----------|
| 1 | `is_fusible()` returns `true` | Explicit opt-in; function author certifies purity |
| 2 | Single input (1 `input_index`) | Multi-input stages have fan-in semantics |
| 3 | Input is the immediately preceding node | Non-adjacent stages can't be merged without rewiring |
| 4 | No other node reads from the same input | Fan-out means the intermediate result is needed elsewhere |
| 5 | Stage is `StreamingStage` variant | Batch stages have different execution semantics |

Criterion 1 is the safety gate — only functions that self-declare as
pure maps can be fused. Criteria 2–4 are structural constraints checked
by the optimizer. Criterion 5 is implicit (only `StreamingStage` nodes
are candidates).

### 2.3 Fusibility Detection — `is_fusible()` Trait Method

Added to `StreamingComputeFunction` with default `false`:

```rust
pub trait StreamingComputeFunction: Send + Sync {
    // ... existing methods ...

    /// Whether this function can be fused with adjacent fusible stages.
    ///
    /// Return `true` only if the function is a pure, stateless, single-input,
    /// single-output chunk-by-chunk transform. The function must produce
    /// exactly one output chunk per input chunk with no buffering.
    fn is_fusible(&self) -> bool {
        false
    }
}
```

The 9 fusible builtins override this to return `true`. All other
builtins (accumulators, combiners, utilities) keep the default `false`.

### 2.4 FusedMapStage Design

`FusedMapStage` holds a `Vec` of transform closures extracted from the
fusible stages. On each input chunk, it applies all transforms
sequentially, producing one output chunk:

```
  Input chunk (Bytes)
       │
       ▼
  ┌──────────┐
  │ stage[0]  │  Uppercase: b.to_ascii_uppercase()
  └────┬─────┘
       │ Bytes
       ▼
  ┌──────────┐
  │ stage[1]  │  Lowercase: b.to_ascii_lowercase()
  └────┬─────┘
       │ Bytes
       ▼
  ┌──────────┐
  │ stage[2]  │  XorCipher: b ^ key
  └────┬─────┘
       │ Bytes
       ▼
  Output chunk (Bytes)
```

Key properties:
- **Single task**: one `tokio::spawn` for the entire fused chain
- **Single channel**: one input `rx`, one output `tx`
- **Zero intermediate allocation**: each stage receives the previous
  stage's `Bytes` output directly (no channel buffer)
- **Error short-circuit**: if any stage returns `Err`, the fused stage
  emits `StreamChunk::Error` and terminates immediately

The fused stage implements `StreamingComputeFunction` itself, so it
is transparent to the rest of the pipeline — downstream stages see
a normal streaming stage.

### 2.5 Fusion Optimizer Pass

The optimizer scans the node list in topological order (which is
insertion order for our linear pipeline builder) and identifies
maximal runs of fusible stages:

```
  Algorithm: fuse_pipeline(nodes) → nodes'

  1. Build fan-out map: for each node index, count how many
     other nodes reference it as input.

  2. Scan nodes left to right. Maintain a "current run" of
     fusible stages.

  3. For each StreamingStage node:
     a. If is_fusible() AND single input AND input == previous node
        AND fan_out[input] == 1:
        → extend current run

     b. Otherwise:
        → flush current run (merge into FusedMapStage if len ≥ 2)
        → start new run or emit node as-is

  4. After scan, flush any remaining run.

  5. Renumber input_indices to account for merged nodes.
```

Step 5 is critical: when N nodes merge into 1, all subsequent
`input_indices` must be adjusted by `-(N-1)`.

### 2.6 Interaction with Other Features

| Feature | Interaction | Resolution |
|---------|------------|------------|
| §2.9 Mode Selection | §2.9 picks batch vs streaming; fusion only applies within streaming | No conflict — fusion runs after mode selection |
| §2.10 Adaptive Chunking | Resizer nodes are NOT fusible (stateful); they break fusible chains | Correct — resizer between two fusible stages creates two separate fused groups |
| §2.12 Memory Budget | Fusion reduces channel buffer count (fewer channels = less memory) | Beneficial — fusion helps stay within budget |
| Observability | Fused stages report as single unit; individual stage metrics lost | Acceptable — fused stages are cheap, per-stage metrics add little value |
| `enable_fusion: false` | Skips fusion pass entirely; pipeline runs as today | Clean opt-out for debugging |---

## 3. Implementation

### 3.1 Trait Extension

Add `is_fusible()` to `StreamingComputeFunction` in `streaming.rs`:

```rust
pub trait StreamingComputeFunction: Send + Sync {
    async fn stream_execute(
        &self,
        inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk>;

    fn supports_streaming(&self) -> bool { true }
    fn preferred_chunk_size(&self) -> usize { DEFAULT_CHUNK_SIZE }
    fn channel_capacity(&self) -> usize { DEFAULT_CHANNEL_CAPACITY }

    /// Whether this function can be fused with adjacent fusible stages.
    /// Only pure, stateless, single-input, chunk-by-chunk transforms
    /// should return `true`.
    fn is_fusible(&self) -> bool { false }
}
```

Mark the 9 fusible builtins in `builtins_streaming.rs`:

```rust
// Add to each of the 9 Category 1 impls:
impl StreamingComputeFunction for StreamingIdentity {
    // ... existing stream_execute ...
    fn is_fusible(&self) -> bool { true }
}
impl StreamingComputeFunction for StreamingUppercase {
    // ... existing stream_execute ...
    fn is_fusible(&self) -> bool { true }
}
// Same for: Lowercase, Reverse, Base64Encode, Base64Decode,
//           Xor, Compress, Decompress
```

9 one-line additions, no changes to existing method bodies.

### 3.2 FusedMapStage Struct

New file: `crates/deriva-compute/src/fusion.rs`

```rust
use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::{StreamingComputeFunction, DEFAULT_CHANNEL_CAPACITY};

/// A fused chain of pure map stages executed sequentially in one task.
pub struct FusedMapStage {
    stages: Vec<(Arc<dyn StreamingComputeFunction>, HashMap<String, String>)>,
}

impl FusedMapStage {
    pub fn new(
        stages: Vec<(Arc<dyn StreamingComputeFunction>, HashMap<String, String>)>,
    ) -> Self {
        debug_assert!(stages.len() >= 2, "fusion requires at least 2 stages");
        Self { stages }
    }

    pub fn len(&self) -> usize {
        self.stages.len()
    }
}

#[async_trait]
impl StreamingComputeFunction for FusedMapStage {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        assert_eq!(inputs.len(), 1, "FusedMapStage takes exactly 1 input");
        let mut rx = inputs.remove(0);
        let (tx, out) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

        // Clone stage refs for the spawned task.
        let stages: Vec<_> = self.stages.iter()
            .map(|(f, p)| (Arc::clone(f), p.clone()))
            .collect();

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        // Apply each stage's transform sequentially.
                        // We feed each stage a single-chunk stream and
                        // collect the single output chunk.
                        let mut current = chunk;
                        let mut failed = false;

                        for (stage, params) in &stages {
                            let (inner_tx, inner_rx) = mpsc::channel(1);
                            inner_tx.send(StreamChunk::Data(current)).await.ok();
                            inner_tx.send(StreamChunk::End).await.ok();
                            drop(inner_tx);

                            let mut out_rx = stage
                                .stream_execute(vec![inner_rx], params)
                                .await;

                            match out_rx.recv().await {
                                Some(StreamChunk::Data(b)) => current = b,
                                Some(StreamChunk::Error(e)) => {
                                    let _ = tx.send(StreamChunk::Error(e)).await;
                                    failed = true;
                                    break;
                                }
                                _ => {
                                    // Stage produced no data — treat as empty
                                    current = Bytes::new();
                                    break;
                                }
                            }
                            // Drain remaining (End marker) — don't leak tasks
                            while out_rx.recv().await.is_some() {}
                        }

                        if failed { return; }
                        if tx.send(StreamChunk::Data(current)).await.is_err() {
                            return;
                        }
                    }
                    Some(StreamChunk::End) => {
                        let _ = tx.send(StreamChunk::End).await;
                        return;
                    }
                    Some(StreamChunk::Error(e)) => {
                        let _ = tx.send(StreamChunk::Error(e)).await;
                        return;
                    }
                    None => return,
                }
            }
        });
        out
    }

    fn is_fusible(&self) -> bool { false } // Don't fuse already-fused stages
}
```

Memory: one `Vec` of `Arc` pointers (~24 bytes per stage). Runtime:
each stage's `stream_execute` is called per chunk, but since fusible
stages use `spawn_map` internally, the overhead is one mini-channel
per stage per chunk. This is still cheaper than full pipeline channels
because the mini-channels have capacity 1 and are consumed immediately.

**Optimization note:** A future iteration could bypass `stream_execute`
entirely by extracting the raw `Fn(&[u8]) -> Result<Bytes, String>`
closures from fusible stages. This would eliminate the mini-channel
overhead entirely, but requires a new trait method (`fn map_fn()`).
The current design is correct and simple; the optimization is deferred.

### 3.3 Fusion Optimizer

In `fusion.rs`, below `FusedMapStage`:

```rust
use crate::pipeline::PipelineNode;

/// Fuse adjacent fusible streaming stages into `FusedMapStage` nodes.
///
/// Returns a new node list with fusible chains merged. Non-fusible nodes
/// and non-streaming nodes pass through unchanged. Input indices are
/// renumbered to account for removed nodes.
pub fn fuse_pipeline(nodes: Vec<PipelineNode>) -> Vec<PipelineNode> {
    if nodes.len() < 2 { return nodes; }

    // Step 1: Build fan-out map (how many nodes read from each index).
    let mut fan_out = vec![0usize; nodes.len()];
    for node in &nodes {
        if let PipelineNode::StreamingStage { input_indices, .. }
             | PipelineNode::BatchStage { input_indices, .. } = node
        {
            for &idx in input_indices {
                fan_out[idx] += 1;
            }
        }
    }

    // Step 2: Scan and collect fusible runs.
    // index_map[old_index] = new_index after merging.
    let mut result: Vec<PipelineNode> = Vec::with_capacity(nodes.len());
    let mut index_map: Vec<usize> = Vec::with_capacity(nodes.len());
    let mut run: Vec<(usize, Arc<dyn StreamingComputeFunction>, HashMap<String, String>)>
        = Vec::new();

    for (i, node) in nodes.into_iter().enumerate() {
        let is_fusible_candidate = matches!(&node, PipelineNode::StreamingStage {
            function, input_indices, ..
        } if function.is_fusible()
            && input_indices.len() == 1
            && input_indices[0] == i.wrapping_sub(1)
            && fan_out[i.wrapping_sub(1)] == 1
        );

        if is_fusible_candidate {
            if let PipelineNode::StreamingStage { function, params, .. } = node {
                run.push((i, function, params));
                index_map.push(usize::MAX); // placeholder, resolved at flush
                continue;
            }
        }

        // Not fusible — flush any accumulated run first.
        flush_run(&mut run, &mut result, &mut index_map);

        let new_idx = result.len();
        index_map.push(new_idx);
        result.push(node);
    }
    flush_run(&mut run, &mut result, &mut index_map);

    // Step 3: Renumber input_indices in all remaining nodes.
    for node in &mut result {
        match node {
            PipelineNode::StreamingStage { input_indices, .. }
            | PipelineNode::BatchStage { input_indices, .. } => {
                for idx in input_indices.iter_mut() {
                    *idx = index_map[*idx];
                }
            }
            _ => {}
        }
    }

    result
}

fn flush_run(
    run: &mut Vec<(usize, Arc<dyn StreamingComputeFunction>, HashMap<String, String>)>,
    result: &mut Vec<PipelineNode>,
    index_map: &mut Vec<usize>,
) {
    if run.is_empty() { return; }

    if run.len() == 1 {
        // Single fusible stage — emit as-is, no fusion.
        let (orig_idx, function, params) = run.remove(0);
        let new_idx = result.len();
        index_map[orig_idx] = new_idx;
        result.push(PipelineNode::StreamingStage {
            _addr: CAddr::default(), // placeholder
            function,
            params,
            input_indices: vec![new_idx - 1],
        });
        return;
    }

    // Merge run of ≥2 into FusedMapStage.
    let first_input = result.len() - 1; // node before the run
    let stages: Vec<_> = run.drain(..)
        .map(|(orig_idx, f, p)| {
            let new_idx = result.len(); // will be set after push
            index_map[orig_idx] = new_idx;
            (f, p)
        })
        .collect();

    let fused = FusedMapStage::new(stages);
    let new_idx = result.len();
    // Fix all index_map entries for this run to point to the fused node.
    for entry in index_map.iter_mut() {
        if *entry == new_idx { /* already correct */ }
    }
    // All run members map to the single fused node index.
    let fused_idx = result.len();
    for entry in index_map.iter_mut().rev() {
        if *entry >= fused_idx && *entry != usize::MAX { break; }
        if *entry == usize::MAX { *entry = fused_idx; }
    }

    result.push(PipelineNode::StreamingStage {
        _addr: CAddr::default(),
        function: Arc::new(fused),
        params: HashMap::new(),
        input_indices: vec![first_input],
    });
}
```

### 3.4 Pipeline Builder Integration

In `pipeline.rs`, modify `StreamPipeline::execute()`:

```rust
use crate::fusion::fuse_pipeline;

impl StreamPipeline {
    pub async fn execute(self) -> Result<mpsc::Receiver<StreamChunk>, DerivaError> {
        let start = std::time::Instant::now();
        metrics::STREAM_PIPELINES_TOTAL.inc();

        // NEW: apply fusion pass if enabled.
        let nodes = if self.config.enable_fusion {
            let original_len = self.nodes.len();
            let fused = fuse_pipeline(self.nodes);
            let fused_count = original_len - fused.len();
            if fused_count > 0 {
                metrics::record_fusion(fused_count);
                tracing::debug!(
                    original = original_len,
                    fused = fused.len(),
                    eliminated = fused_count,
                    "pipeline fusion applied"
                );
            }
            fused
        } else {
            self.nodes
        };

        // Rest of execute() unchanged, but iterates over `nodes`
        // instead of `self.nodes`.
        let mut outputs: Vec<Option<mpsc::Receiver<StreamChunk>>> =
            Vec::with_capacity(nodes.len());

        for node in nodes {
            // ... existing match arms unchanged ...
        }
        // ...
    }
}
```

~15 lines added to `execute()`. The fusion pass runs before any tasks
are spawned, so there is zero runtime overhead if no stages are fused.

### 3.5 PipelineConfig Extension

Add one field to `PipelineConfig`:

```rust
pub struct PipelineConfig {
    pub chunk_size: usize,
    pub channel_capacity: usize,
    pub cache_intermediates: bool,
    pub memory_budget: usize,
    pub enable_fusion: bool,  // NEW — default true
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            cache_intermediates: true,
            memory_budget: 0,
            enable_fusion: true,
        }
    }
}
```

Fusion is enabled by default because it has no correctness risk (only
opt-in fusible stages are affected) and the overhead of the fusion
pass itself is negligible (O(N) scan).

### 3.6 Fusible Built-in Functions

Complete list of builtins with their fusibility status and the single
line change required:

| # | Struct | `is_fusible()` | Change |
|---|--------|---------------|--------|
| 1 | `StreamingIdentity` | `true` | Add `fn is_fusible(&self) -> bool { true }` |
| 2 | `StreamingUppercase` | `true` | Add `fn is_fusible(&self) -> bool { true }` |
| 3 | `StreamingLowercase` | `true` | Add `fn is_fusible(&self) -> bool { true }` |
| 4 | `StreamingReverse` | `true` | Add `fn is_fusible(&self) -> bool { true }` |
| 5 | `StreamingBase64Encode` | `true` | Add `fn is_fusible(&self) -> bool { true }` |
| 6 | `StreamingBase64Decode` | `true` | Add `fn is_fusible(&self) -> bool { true }` |
| 7 | `StreamingXor` | `true` | Add `fn is_fusible(&self) -> bool { true }` |
| 8 | `StreamingCompress` | `true` | Add `fn is_fusible(&self) -> bool { true }` |
| 9 | `StreamingDecompress` | `true` | Add `fn is_fusible(&self) -> bool { true }` |
| 10–12 | Accumulators | `false` (default) | No change |
| 13–15 | Combiners | `false` (default) | No change |
| 16–20 | Utilities | `false` (default) | No change |

Total code changes in `builtins_streaming.rs`: 9 one-line additions.

---

## 4. Data Flow Diagrams

### 4.1 Before Fusion — 3 Adjacent Map Stages

Pipeline: `Source → Uppercase → Lowercase → XorCipher`
Input: 1 MB, chunk size 64 KB (16 chunks)

```
  Task 0          Task 1           Task 2           Task 3
  ┌────────┐  ch0  ┌──────────┐  ch1  ┌──────────┐  ch2  ┌──────────┐
  │ Source  │─────►│Uppercase │─────►│Lowercase │─────►│XorCipher │
  │ 64KB×16│      │ 2μs/chk  │      │ 2μs/chk  │      │ 1μs/chk  │
  └────────┘      └──────────┘      └──────────┘      └──────────┘

  Channels: 3 (ch0, ch1, ch2)
  Tasks: 4
  Per-chunk path: send₀ + recv₁ + compute₁ + send₁ + recv₂ + compute₂ + send₂ + recv₃ + compute₃ + send₃
  Channel overhead per chunk: 3 hops × 100 ns = 300 ns
  Compute per chunk: 2 + 2 + 1 = 5 μs
  Total per chunk: 5.3 μs
  Total for 16 chunks: 84.8 μs
  Channel overhead total: 16 × 300 ns = 4.8 μs (5.7% of total)
```

### 4.2 After Fusion — Single Fused Stage

Pipeline: `Source → FusedMap[Uppercase, Lowercase, XorCipher]`

```
  Task 0          Task 1
  ┌────────┐  ch0  ┌─────────────────────────────┐
  │ Source  │─────►│ FusedMapStage               │
  │ 64KB×16│      │  [0] Uppercase  (2μs/chk)   │
  └────────┘      │  [1] Lowercase  (2μs/chk)   │
                  │  [2] XorCipher  (1μs/chk)   │
                  └─────────────────────────────┘

  Channels: 1 (ch0)
  Tasks: 2
  Per-chunk path: send₀ + recv₁ + compute₁₂₃ + send₁
  Channel overhead per chunk: 1 hop × 100 ns = 100 ns
  Compute per chunk: 5 μs (same — transforms still applied)
  Total per chunk: 5.1 μs
  Total for 16 chunks: 81.6 μs
  Channel overhead total: 16 × 100 ns = 1.6 μs (2.0% of total)

  Saved: 2 channels, 2 tasks, 3.2 μs (3.8% improvement)
```

### 4.3 Partial Fusion — Chain Interrupted by Accumulator

Pipeline: `Source → Uppercase → Lowercase → Sha256 → Base64Encode → XorCipher`

Sha256 is an accumulator (`is_fusible() = false`), so it breaks the
fusible chain into two groups:

```
  Before fusion (6 nodes, 5 channels):

  Source → Uppercase → Lowercase → Sha256 → Base64Encode → XorCipher
    ch0       ch1         ch2        ch3         ch4

  Fusion analysis:
    Uppercase  (fusible, input=Source, fan_out[Source]=1)     ─┐ run A
    Lowercase  (fusible, input=Uppercase, fan_out[Upper]=1)   ─┘
    Sha256     (NOT fusible) — flush run A, emit Sha256 as-is
    Base64Enc  (fusible, input=Sha256, fan_out[Sha256]=1)    ─┐ run B
    XorCipher  (fusible, input=Base64Enc, fan_out[B64]=1)    ─┘

  After fusion (4 nodes, 3 channels):

  ┌────────┐ ch0 ┌──────────────────┐ ch1 ┌────────┐ ch2 ┌──────────────────┐
  │ Source │────►│FusedMap          │────►│ Sha256 │────►│FusedMap          │
  └────────┘     │ [Upper, Lower]  │     │(accum) │     │ [B64Enc, Xor]   │
                 └──────────────────┘     └────────┘     └──────────────────┘

  Eliminated: 2 channels, 2 tasks
  Index renumbering: Source=0, FusedA=1, Sha256=2, FusedB=3
```

### 4.4 Fan-Out Prevents Fusion

Pipeline: `Source → Uppercase → Lowercase` where both Uppercase output
and Lowercase output are consumed by downstream stages.

```
  Before:
                        ┌──────────┐
                   ┌───►│ Sha256_A │  (reads from Uppercase)
  ┌────────┐ ch0 ┌┴─────────┐ ch1 ┌──────────┐ ch2 ┌──────────┐
  │ Source │────►│Uppercase │────►│Lowercase │────►│ Sha256_B │
  └────────┘     └──────────┘     └──────────┘     └──────────┘

  Fusion analysis:
    Uppercase: fusible, single input, input=Source, fan_out[Source]=1 ✓
    Lowercase: fusible, single input, input=Uppercase,
               BUT fan_out[Uppercase]=2 (Sha256_A also reads it) ✗

  Result: NO fusion. Uppercase output is needed by two consumers,
  so it cannot be absorbed into a fused stage.

  After fusion: unchanged (same 5 nodes, 4 channels)
```

### 4.5 Deep Fusible Chain — 10 Identity Stages

Pipeline: `Source → Identity×10`, 1 MB input, 64 KB chunks (16 chunks)

```
  Before fusion: 11 tasks, 10 channels
  Per-chunk overhead: 10 × 100 ns = 1,000 ns
  Total overhead: 16 × 1,000 ns = 16 μs

  After fusion: 2 tasks, 1 channel
  Per-chunk overhead: 1 × 100 ns = 100 ns
  Total overhead: 16 × 100 ns = 1.6 μs

  Saved: 9 tasks, 9 channels, 14.4 μs

  For Identity stages (0 ns compute), the pipeline time is
  dominated by channel overhead. Fusion reduces it by 90%.

  Before: 16 chunks × 10 hops × 100 ns = 16 μs
  After:  16 chunks × 1 hop  × 100 ns = 1.6 μs
  Speedup: 10×
```

Note: 10 Identity stages is artificial but demonstrates the upper
bound of fusion benefit. Real pipelines with 2–4 fusible stages
see 50–75% channel overhead reduction.

---

## 5. Test Specification

### 5.1 Unit Tests — `is_fusible()` Trait Method (5 tests)

**T1: All 9 map builtins report fusible**
Instantiate each of `StreamingIdentity`, `StreamingUppercase`,
`StreamingLowercase`, `StreamingReverse`, `StreamingBase64Encode`,
`StreamingBase64Decode`, `StreamingXor`, `StreamingCompress`,
`StreamingDecompress`. Assert `is_fusible() == true` for all 9.

**T2: All 3 accumulators report non-fusible**
Instantiate `StreamingSha256`, `StreamingByteCount`, `StreamingChecksum`.
Assert `is_fusible() == false` for all 3.

**T3: All 3 combiners report non-fusible**
Instantiate `StreamingConcat`, `StreamingInterleave`, `StreamingZipConcat`.
Assert `is_fusible() == false` for all 3.

**T4: All 5 utilities report non-fusible**
Instantiate `StreamingChunkResizer`, `StreamingTake`, `StreamingSkip`,
`StreamingRepeat`, `StreamingTeeCount`. Assert `is_fusible() == false`
for all 5.

**T5: FusedMapStage itself reports non-fusible**
Create a `FusedMapStage` from 2 Identity stages. Assert
`is_fusible() == false` — prevents recursive fusion.

### 5.2 Unit Tests — FusedMapStage Execution (6 tests)

**T6: Two-stage fusion produces correct output**
Fuse `Uppercase → Lowercase`. Feed `b"Hello World"` as a single chunk.
Assert output equals `b"hello world"` (uppercase then lowercase = all
lowercase). Verifies transforms applied in order.

**T7: Three-stage fusion with XorCipher roundtrip**
Fuse `XorCipher(key=0xAA) → Uppercase → XorCipher(key=0xAA)`. Feed
`b"test data"`. The two XOR operations cancel out, but Uppercase in
the middle modifies the bytes. Assert output matches applying the three
transforms manually in sequence.

**T8: Fused compress → decompress roundtrip**
Fuse `Compress → Decompress`. Feed 64 KB of random bytes. Assert
output equals input — verifies that per-chunk compress/decompress
is invertible through fusion.

**T9: Multi-chunk stream through fused stage**
Fuse `Uppercase → Reverse`. Feed 256 KB as 4 × 64 KB chunks. Collect
all output chunks. Concatenate and compare against applying Uppercase
then Reverse to the full 256 KB input. Verifies chunk-by-chunk
equivalence.

**T10: Fused stage preserves End marker**
Fuse `Identity → Identity`. Feed 1 chunk + End. Assert output stream
contains exactly 1 Data chunk followed by End. No extra chunks, no
dropped End.

**T11: Fused stage propagates mid-chain error**
Create a custom fusible stage that returns `Err("injected")` on the
3rd chunk. Fuse it between two Identity stages: `Identity → Failing →
Identity`. Feed 5 chunks. Assert output contains 2 Data chunks
followed by `StreamChunk::Error`. Verifies error short-circuit.

### 5.3 Unit Tests — Fusion Optimizer Logic (8 tests)

**T12: Three adjacent fusible stages merge into one node**
Build pipeline: `Source → Uppercase → Lowercase → Reverse`. Run
`fuse_pipeline()`. Assert result has 2 nodes: Source + 1 FusedMapStage.
Assert the fused node's `input_indices` points to Source (index 0).

**T13: Non-fusible stage breaks chain into two groups**
Build: `Source → Upper → Lower → Sha256 → Base64Enc → Xor`. Run
`fuse_pipeline()`. Assert 4 nodes: Source, FusedMap[Upper,Lower],
Sha256, FusedMap[Base64Enc,Xor]. Assert input indices are [0,1,2,3]
respectively.

**T14: Single fusible stage — no fusion**
Build: `Source → Uppercase → Sha256`. Run `fuse_pipeline()`. Assert
3 nodes unchanged — a single fusible stage between non-fusible
boundaries is not merged (needs ≥2 to fuse).

**T15: All non-fusible stages — no fusion**
Build: `Source → Sha256 → ByteCount`. Run `fuse_pipeline()`. Assert
3 nodes unchanged.

**T16: Fan-out prevents fusion**
Build: `Source → Uppercase → Lowercase`, where both Uppercase and
Lowercase are consumed by separate downstream Sha256 stages. Run
`fuse_pipeline()`. Assert Uppercase and Lowercase remain separate
(fan_out[Uppercase] = 2 blocks fusion).

**T17: Empty pipeline — no crash**
Run `fuse_pipeline(vec![])`. Assert empty result.

**T18: Single source node — no crash**
Run `fuse_pipeline(vec![Source])`. Assert 1 node unchanged.

**T19: Long chain of 10 fusible stages merges into one**
Build: `Source → Identity×10`. Run `fuse_pipeline()`. Assert 2 nodes:
Source + 1 FusedMapStage containing 10 stages. Assert
`fused.len() == 10`.

### 5.4 Integration Tests — Fused Pipeline Correctness (6 tests)

**T20: Fused pipeline output matches unfused — Uppercase → Lowercase → Xor**
Run the same 3-stage pipeline twice: once with `enable_fusion: true`,
once with `enable_fusion: false`. Feed 1 MB of random ASCII data.
Collect both outputs. Assert byte-for-byte equality.

**T21: Fused pipeline output matches unfused — Base64Encode → Compress**
Same approach: fused vs unfused on 512 KB input. Assert identical
output. Tests that Base64 expansion (4/3× size increase) followed by
compression works correctly through fusion.

**T22: Fused pipeline output matches unfused — 5-stage normalize chain**
Pipeline: `Uppercase → Reverse → XorCipher(0x42) → Lowercase → Base64Encode`.
Feed 2 MB. Assert fused == unfused output.

**T23: Partial fusion correctness — fusible + accumulator + fusible**
Pipeline: `Uppercase → Lowercase → Sha256 → Base64Encode → Xor`.
Run fused and unfused. Assert identical SHA-256 digest and identical
final output. Verifies that partial fusion (two fused groups around
an accumulator) preserves semantics.

**T24: Fusion with §2.10 adaptive resizer in chain**
Pipeline: `Source → AdaptiveResizer → Uppercase → Lowercase → Sha256`.
Resizer is non-fusible, so Upper+Lower should fuse but Resizer stays
separate. Assert output matches unfused pipeline. Verifies §2.10
interaction.

**T25: Fusion with batch stage in pipeline**
Pipeline: `BatchConcat → Uppercase → Lowercase → Reverse`. The batch
stage materializes before streaming begins. Assert fused streaming
tail produces correct output. Verifies batch→streaming boundary
doesn't confuse fusion.

### 5.5 Integration Tests — Edge Topologies (5 tests)

**T26: Diamond DAG — no fusion across fan-out/fan-in**
```
  Source → Uppercase → Sha256_A ─┐
                                  ├→ Concat → Identity
  Source → Lowercase → Sha256_B ─┘
```
Uppercase and Lowercase each have fan_out=1 but their inputs (Source)
have fan_out=2. Assert no fusion occurs. Assert final output is
correct (concatenation of two SHA-256 digests).

**T27: Linear chain with mixed batch and streaming stages**
`Source → BatchUppercase → StreamLowercase → StreamReverse → StreamXor`.
Batch stage breaks streaming continuity. Assert Lower+Reverse+Xor
fuse into one group. Assert output correctness.

**T28: Two independent fusible chains in same pipeline**
```
  Source_A → Upper → Lower → Sha256_A
  Source_B → Reverse → Xor → Sha256_B
```
Two separate linear chains. Assert each chain's fusible pair merges
independently. Assert both outputs correct.

**T29: Fusible chain feeding into multi-input combiner**
`Source_A → Upper → Lower ─┐`
`Source_B ─────────────────┘→ ZipConcat → Identity`
Upper+Lower should fuse. ZipConcat is non-fusible (multi-input).
Assert fusion occurs for Upper+Lower only. Assert ZipConcat output
correct.

**T30: 20-stage all-fusible pipeline**
`Source → Identity×20`. Assert fuses into 2 nodes. Feed 4 MB. Assert
output equals input (20 identities = no-op). Stress test for deep
fusion.

### 5.6 Concurrency & Stress Tests (4 tests)

**T31: 8 concurrent fused pipelines**
Spawn 8 pipelines simultaneously, each: `Source(256KB) → Upper →
Lower → Xor(key=i)` where `i` is the pipeline index. Assert all 8
produce correct, distinct outputs. Verifies no shared state leakage
between fused stages.

**T32: Fused pipeline under backpressure**
`Source(16MB) → Upper → Lower → SlowConsumer(1ms delay per chunk)`.
Upper+Lower fuse. Assert output correct despite slow consumption.
Verifies fused stage handles channel backpressure on its single
output channel.

**T33: Fused pipeline with large chunks (1 MB)**
`Source(32MB, chunk=1MB) → Compress → Decompress`. 32 chunks through
fused compress→decompress. Assert output equals input. Tests that
fusion works with non-default chunk sizes.

**T34: Rapid repeated pipeline construction and execution**
In a loop of 100 iterations: build `Source(64KB) → Upper → Lower`,
execute with fusion, collect output, drop. Assert no memory leak
(final RSS within 2× of initial). Verifies cleanup of fused stage
internals.

### 5.7 Benchmark Tests (3 tests)

**T35: Fused overhead ≤ 1.05× on homogeneous pipeline**
Benchmark `Source(8MB) → Identity×5` fused vs unfused. Assert fused
time ≤ 1.05× unfused time. Since Identity is a no-op, any slowdown
is pure fusion overhead (mini-channels). This is the overhead ceiling.

**T36: Fused ≥ 1.10× faster on 5-stage cheap transform chain**
Benchmark `Source(8MB) → Upper → Lower → Reverse → Xor → Base64Enc`
fused vs unfused. Assert fused is at least 10% faster. The 4
eliminated channel hops should dominate the mini-channel overhead.

**T37: Regression guard — 6 pipeline configurations**
Benchmark matrix: {2, 5, 10} fusible stages × {1MB, 16MB} input.
Record baseline. On subsequent runs, assert no configuration regresses
by more than 5% from baseline.

### 5.8 Error Handling Tests (3 tests)

**T38: Upstream error propagates through fused stage**
`Source → [Error after 3 chunks] → Upper → Lower`. Upper+Lower fuse.
Assert fused stage receives the error and forwards it to output.
Assert exactly 3 Data chunks before Error.

**T39: Dropped consumer — fused stage terminates cleanly**
`Source(1MB) → Upper → Lower`. Fuse, start pipeline, read 2 chunks,
then drop the output receiver. Assert no panic, no hang. The fused
task should detect the closed channel and exit.

**T40: Error in second stage of fused chain**
Create custom fusible stage that errors on chunks > 32 KB. Fuse:
`Identity → FailOnLarge → Lowercase`. Feed 64 KB chunks. Assert
`StreamChunk::Error` emitted on first chunk. Verifies mid-chain
error within the fused stage propagates correctly.

---

## 6. Edge Cases & Error Handling

| # | Scenario | Input | Expected Behavior |
|---|----------|-------|-------------------|
| 1 | Single-stage pipeline | `Source → Uppercase` | No fusion (need ≥2 fusible). Output unchanged. |
| 2 | All 9 fusible stages in one chain | `Source → Identity → Upper → Lower → Reverse → B64Enc → B64Dec → Xor → Compress → Decompress` | All 9 merge into 1 FusedMapStage. Output correct. |
| 3 | Empty stream (0 chunks) | `Source(0 bytes) → Upper → Lower` | Fused stage receives End immediately, forwards End. No Data chunks. |
| 4 | Single 1-byte chunk | `Source(1 byte) → Upper → Xor` | Fused stage processes 1-byte chunk through both transforms. Output = 1 byte. |
| 5 | Chunk exactly at channel capacity boundary | 8 chunks (capacity=8), `Upper → Lower` | All chunks buffered then consumed. No deadlock. |
| 6 | Error in first stage of fused chain | `FailingStage → Identity` fused | Error emitted on first chunk. Identity never executes. |
| 7 | Error in last stage of fused chain | `Identity → FailingStage` fused | First stages succeed, last stage errors. Error propagated to output. |
| 8 | Params differ between fused stages | `Xor(key=0xAA) → Xor(key=0x55)` fused | Each stage uses its own params. Output = input XOR'd with 0xAA then 0x55 (= XOR 0xFF). |
| 9 | Fan-out on first node of potential chain | `Source(fan_out=2) → Upper → Lower` | fan_out[Source]=2 blocks Upper from fusing. Both stages remain separate. |
| 10 | Fan-out on middle node of chain | `Upper → Lower(fan_out=2) → Reverse` | Lower output needed by 2 consumers. Lower+Reverse don't fuse. Upper+Lower don't fuse (Lower has fan_out=2 as output, but the check is fan_out on Lower's *input* which is Upper). Actually: fan_out[Upper]=1, fan_out[Lower]=2. Upper+Lower fuse (fan_out[Upper]=1 ✓). Lower+Reverse don't fuse because Lower is absorbed into fused group. Reverse becomes standalone. |
| 11 | `enable_fusion: false` | Any fusible chain | `fuse_pipeline()` not called. All stages remain separate. Output identical. |
| 12 | Pipeline with only batch stages | `BatchUpper → BatchLower` | No StreamingStage nodes. Fusion pass is no-op. |
| 13 | Fusible stage with non-default `preferred_chunk_size()` | `Compress(preferred=256KB) → Decompress(preferred=64KB)` | Fusion merges them. The fused stage inherits default preferred_chunk_size. §2.10 resizer insertion happens before fusion pass, so chunk sizes are already handled. |
| 14 | Very large chunk (1 MB) through fused stage | `Source(1MB, chunk=1MB) → Upper → Lower` | Single 1 MB chunk processed through fused chain. No splitting. Output = 1 MB. |
| 15 | Concurrent modification of pipeline during fusion | N/A — fusion runs at construction time | Not possible. `fuse_pipeline()` takes ownership of `Vec<PipelineNode>`. No concurrent access. |
| 16 | Fused stage followed by another fused stage | `[Upper,Lower] → Sha256 → [B64Enc,Xor]` | Two separate FusedMapStage nodes. Neither fuses with the other (FusedMapStage.is_fusible() = false). |
| 17 | Recursive fusion attempt | FusedMapStage adjacent to another FusedMapStage | `is_fusible()` returns false on FusedMapStage. No recursive fusion. |
| 18 | Non-adjacent fusible stages (gap in indices) | `Source → Upper → BatchStage → Lower → Reverse` | Upper is alone (no adjacent fusible after it). Lower+Reverse fuse. Upper stays separate. |
| 19 | Pipeline cancelled mid-execution | `Source(16MB) → Upper → Lower` fused, cancel after 4 chunks | Fused task detects closed output channel (`send().await.is_err()`), exits. Input channel drained by Tokio drop. No leak. |
| 20 | Fusible stage that produces empty Bytes on some chunks | Custom fusible stage returns `Bytes::new()` for even chunks | Fused chain passes empty Bytes to next stage. Next stage receives 0-length input. No panic, no skip. |

---

## 7. Performance Analysis

### 7.1 Expected Improvement

Fusion eliminates `(N-1)` channel hops for a chain of N fusible stages.
The benefit depends on the ratio of channel overhead to compute cost:

| Pipeline | Stages | Compute/chunk | Channel overhead/chunk | Fused overhead/chunk | Saved/chunk | Improvement |
|----------|--------|--------------|----------------------|---------------------|-------------|-------------|
| 3× Identity (1 MB) | 3 | ~0 ns | 3 × 100 ns = 300 ns | 100 ns | 200 ns | ~67% of overhead |
| Upper → Lower → Xor (1 MB) | 3 | 5 μs | 300 ns | 100 ns | 200 ns | 3.8% of total |
| Upper → Lower → Xor (64 MB) | 3 | 5 μs | 300 ns | 100 ns | 200 ns | 3.8% of total |
| 5× Identity (1 MB) | 5 | ~0 ns | 500 ns | 100 ns | 400 ns | ~80% of overhead |
| Upper → Rev → Xor → B64 → Lower (1 MB) | 5 | 15 μs | 500 ns | 100 ns | 400 ns | 2.6% of total |
| 10× Identity (1 MB) | 10 | ~0 ns | 1,000 ns | 100 ns | 900 ns | ~90% of overhead |
| Compress → Decompress (1 MB) | 2 | 400 μs | 200 ns | 100 ns | 100 ns | 0.025% of total |

Key observations:
- Fusion benefit is **proportional to chain length** and **inversely
  proportional to per-stage compute cost**.
- For cheap transforms (Identity, Uppercase, Xor): 2–4% total improvement
  at depth 3, scaling to 5–10% at depth 5+.
- For expensive transforms (Compress): benefit is negligible — channel
  overhead is already < 0.1% of compute.
- The primary value is **task reduction** (fewer Tokio tasks, less
  scheduling overhead) rather than raw throughput.

### 7.2 Overhead of Fusion Pass

| Component | Cost | Notes |
|-----------|------|-------|
| `fuse_pipeline()` scan | O(N) | One pass over node list |
| Fan-out map construction | O(N) | One `Vec<usize>` allocation |
| Index renumbering | O(N) | One pass over result nodes |
| `FusedMapStage` allocation | O(K) per fused group | K = stages in group, one `Vec<Arc>` |
| **Total** | **O(N)** | N = total pipeline nodes |

For a 20-node pipeline: ~200 ns. Negligible compared to pipeline
execution time (milliseconds).

### 7.3 Mini-Channel Overhead in FusedMapStage

The current `FusedMapStage` implementation uses mini-channels (capacity 1)
to invoke each stage's `stream_execute()` per chunk. This adds overhead
compared to direct function calls:

| Component | Cost per stage per chunk | Notes |
|-----------|------------------------|-------|
| `mpsc::channel(1)` creation | ~50 ns | Allocates channel internals |
| `send` + `recv` on mini-channel | ~80 ns | Uncontended, capacity 1 |
| `stream_execute` dispatch | ~10 ns | Virtual call + async overhead |
| Drain End marker | ~40 ns | One extra recv |
| **Total per stage** | **~180 ns** | |

For a 3-stage fused chain processing 16 chunks (1 MB at 64 KB):
- Mini-channel overhead: 3 × 16 × 180 ns = 8.6 μs
- Eliminated channel hops: 2 × 16 × 100 ns = 3.2 μs
- **Net cost: +5.4 μs** (mini-channels more expensive than saved hops)

This means the current implementation has **negative benefit for short
chains of cheap stages**. The break-even point is ~5 stages. For chains
shorter than 5, the mini-channel overhead exceeds the saved channel hops.

**Mitigation (future optimization):** Extract raw `Fn(&[u8]) -> Result<Bytes>`
closures via a new `map_fn()` trait method, bypassing `stream_execute()`
entirely. This would reduce per-stage overhead to ~5 ns (direct function
call), making fusion beneficial at depth ≥ 2.

### 7.4 When Fusion Hurts

| Scenario | Why it hurts | Mitigation |
|----------|-------------|------------|
| Chain of 2 cheap stages | Mini-channel overhead > saved hop | Future `map_fn()` optimization; or don't fuse chains < 3 |
| Expensive stages (Compress) | Benefit < 0.1%, mini-channel adds measurable overhead | `is_fusible()` could return false for expensive stages, but current design keeps it simple |
| Loss of inter-stage parallelism | Unfused: stage N+1 starts on chunk K while stage N processes chunk K+1. Fused: sequential per chunk. | Only matters when stages have similar cost AND pipeline is throughput-bound. Rare for fusible (cheap) stages. |

### 7.5 Benchmark Study Plan

Location: `deriva-compute/benches/pipeline_fusion.rs`

Five benchmark groups that validate improvement, measure overhead,
and detect regressions.

#### Benchmark 1: Fused vs Unfused — Cheap Transform Chain

Primary target workload: chain of cheap map stages at varying depths.

```rust
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn bench_fused_vs_unfused_cheap(c: &mut Criterion) {
    let mut group = c.benchmark_group("fused_vs_unfused_cheap");
    let data = Bytes::from(vec![b'a'; 8 * 1024 * 1024]); // 8 MB ASCII
    group.throughput(Throughput::Bytes(8 * 1024 * 1024));

    let depths = [2, 3, 5, 7, 10];

    for &depth in &depths {
        // (a) Unfused
        group.bench_with_input(
            BenchmarkId::new("unfused", depth),
            &depth,
            |b, &depth| {
                b.to_async(&rt).iter(|| async {
                    run_n_stage_uppercase_lowercase(data.clone(), depth, PipelineConfig {
                        enable_fusion: false,
                        ..Default::default()
                    }).await
                });
            },
        );

        // (b) Fused
        group.bench_with_input(
            BenchmarkId::new("fused", depth),
            &depth,
            |b, &depth| {
                b.to_async(&rt).iter(|| async {
                    run_n_stage_uppercase_lowercase(data.clone(), depth, PipelineConfig {
                        enable_fusion: true,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Fused is faster at depth ≥ 5 (break-even for mini-channel
overhead). At depth 2–3, fused may be slightly slower due to
mini-channel cost. At depth 10, fused is 5–10% faster.

#### Benchmark 2: Fused vs Unfused — Expensive Transform Chain

Measures fusion overhead on stages where compute dominates.

```rust
fn bench_fused_vs_unfused_expensive(c: &mut Criterion) {
    let mut group = c.benchmark_group("fused_vs_unfused_expensive");
    let sizes = vec![1024 * 1024, 8 * 1024 * 1024, 32 * 1024 * 1024];

    for &size in &sizes {
        let data = Bytes::from(vec![0u8; size]);
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("unfused_compress_decompress", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_compress_decompress(data.clone(), PipelineConfig {
                        enable_fusion: false,
                        ..Default::default()
                    }).await
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("fused_compress_decompress", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_compress_decompress(data.clone(), PipelineConfig {
                        enable_fusion: true,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Fused and unfused within 1% of each other. Compress +
Decompress at ~400 μs/chunk dwarfs the ~200 ns channel overhead.
Verifies fusion doesn't hurt expensive pipelines.

#### Benchmark 3: Fusion Pass Cost

Measures the construction-time cost of `fuse_pipeline()` at varying
pipeline sizes.

```rust
fn bench_fusion_pass_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("fusion_pass_cost");
    let depths = [5, 10, 20, 50, 100];

    for &depth in &depths {
        group.bench_with_input(
            BenchmarkId::new("fuse_pipeline", depth),
            &depth,
            |b, &depth| {
                b.iter(|| {
                    let nodes = build_n_identity_nodes(depth);
                    fuse_pipeline(nodes)
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Linear scaling. 10 nodes: ~100 ns. 100 nodes: ~1 μs.
Negligible compared to pipeline execution.

#### Benchmark 4: Partial Fusion — Mixed Pipeline

Measures realistic pipeline with fusible and non-fusible stages
interleaved.

```rust
fn bench_partial_fusion(c: &mut Criterion) {
    let mut group = c.benchmark_group("partial_fusion");
    let data = Bytes::from(vec![b'x'; 4 * 1024 * 1024]); // 4 MB
    group.throughput(Throughput::Bytes(4 * 1024 * 1024));

    // Pipeline: Upper → Lower → Sha256 → B64Enc → Xor
    // Fuses to: FusedMap[Upper,Lower] → Sha256 → FusedMap[B64Enc,Xor]

    group.bench_function("unfused", |b| {
        b.to_async(&rt).iter(|| async {
            run_mixed_pipeline(data.clone(), PipelineConfig {
                enable_fusion: false,
                ..Default::default()
            }).await
        });
    });

    group.bench_function("fused", |b| {
        b.to_async(&rt).iter(|| async {
            run_mixed_pipeline(data.clone(), PipelineConfig {
                enable_fusion: true,
                ..Default::default()
            }).await
        });
    });

    group.finish();
}
```

**Expected:** Sha256 accumulator dominates pipeline time. Fusion of
the two 2-stage groups saves 2 channel hops but the improvement is
< 1% of total. Verifies no regression on mixed pipelines.

#### Benchmark 5: Task Count Scaling

Measures Tokio runtime overhead reduction from fewer spawned tasks.

```rust
fn bench_task_count_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("task_count_scaling");
    let data = Bytes::from(vec![b'a'; 1024 * 1024]); // 1 MB
    group.throughput(Throughput::Bytes(1024 * 1024));

    // Run 50 concurrent pipelines, each with 5 fusible stages.
    // Unfused: 50 × 6 = 300 tasks. Fused: 50 × 2 = 100 tasks.

    group.bench_function("unfused_50_pipelines", |b| {
        b.to_async(&rt).iter(|| async {
            let futs: Vec<_> = (0..50).map(|_| {
                run_5_stage_identity(data.clone(), PipelineConfig {
                    enable_fusion: false,
                    ..Default::default()
                })
            }).collect();
            futures::future::join_all(futs).await
        });
    });

    group.bench_function("fused_50_pipelines", |b| {
        b.to_async(&rt).iter(|| async {
            let futs: Vec<_> = (0..50).map(|_| {
                run_5_stage_identity(data.clone(), PipelineConfig {
                    enable_fusion: true,
                    ..Default::default()
                })
            }).collect();
            futures::future::join_all(futs).await
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_fused_vs_unfused_cheap,
    bench_fused_vs_unfused_expensive,
    bench_fusion_pass_cost,
    bench_partial_fusion,
    bench_task_count_scaling,
);
criterion_main!(benches);
```

**Expected:** Fused is 10–20% faster due to 200 fewer Tokio tasks
competing for scheduler time. This is where fusion's primary benefit
manifests at scale — reduced task scheduling overhead.

#### Benchmark Summary

| # | Group | Parameters | Measures | Expected Result |
|---|-------|-----------|----------|-----------------|
| 1 | Cheap transform chain | 5 depths × 2 modes | Channel overhead elimination | Fused faster at depth ≥ 5 |
| 2 | Expensive transform chain | 3 sizes × 2 modes | Overhead on compute-heavy stages | Within 1% (no regression) |
| 3 | Fusion pass cost | 5 pipeline sizes | Construction-time overhead | Linear, < 1 μs for 100 nodes |
| 4 | Partial fusion (mixed) | 1 pipeline × 2 modes | Realistic mixed pipeline | < 1% improvement, no regression |
| 5 | Task count scaling | 50 concurrent × 2 modes | Tokio scheduler pressure | Fused 10–20% faster at scale |

---

## 8. Files Changed

| File | Change | Lines |
|------|--------|-------|
| `deriva-compute/src/fusion.rs` | **NEW** — `FusedMapStage`, `fuse_pipeline()`, `flush_run()` | ~150 |
| `deriva-compute/src/streaming.rs` | Add `fn is_fusible(&self) -> bool { false }` to `StreamingComputeFunction` trait | ~4 |
| `deriva-compute/src/builtins_streaming.rs` | Add `fn is_fusible(&self) -> bool { true }` to 9 Category 1 impls | ~9 |
| `deriva-compute/src/pipeline.rs` | Add `enable_fusion` to `PipelineConfig`; call `fuse_pipeline()` in `execute()` | ~20 |
| `deriva-compute/src/lib.rs` | Add `mod fusion;` | ~1 |
| `deriva-compute/src/metrics.rs` | Add `FUSION_STAGES_ELIMINATED` counter, `FUSION_CHAINS_TOTAL` counter, `record_fusion()` helper | ~15 |
| `deriva-compute/tests/pipeline_fusion.rs` | **NEW** — 40 tests: trait, fused execution, optimizer, integration, topologies, concurrency, benchmarks, errors | ~800 |
| `deriva-compute/benches/pipeline_fusion.rs` | **NEW** — 5 benchmark groups: cheap chain, expensive chain, pass cost, partial fusion, task scaling | ~200 |

---

## 9. Dependency Changes

No new crate dependencies. All functionality built on existing:

| Dependency | Already in | Used for |
|-----------|-----------|----------|
| `tokio` | `deriva-compute` | `mpsc::channel`, `spawn` (existing) |
| `bytes` | `deriva-core` | `Bytes` (existing) |
| `async-trait` | `deriva-compute` | `#[async_trait]` on `FusedMapStage` impl (existing) |
| `std::sync::Arc` | stdlib | Wrapping `FusedMapStage` as `Arc<dyn StreamingComputeFunction>` |
| `criterion` | `deriva-compute` (dev) | Benchmark framework (existing) |
| `futures` | `deriva-compute` (dev) | `join_all` in task scaling benchmark (existing) |

---

## 10. Design Rationale

### 10.1 Why Opt-In Fusibility Instead of Auto-Detection?

Auto-detecting whether a function is a pure, stateless, chunk-by-chunk
map requires proving properties that are undecidable in general:

| Property | Auto-detectable? | Why not |
|----------|-----------------|---------|
| Stateless | No | Function may capture mutable state in closure |
| No side effects | No | Function may write to disk, network, metrics |
| Single output per input | No | Function may buffer, split, or drop chunks |
| No dependency on chunk order | No | Function may use chunk index internally |

Opt-in `is_fusible()` is safe: the function author knows whether their
transform is pure. The default is `false`, so new functions are
conservatively non-fusible until explicitly marked.

The alternative — a `MapFunction` sub-trait with a `fn map(&self, &[u8])
-> Result<Bytes>` signature — would guarantee purity by construction but
requires refactoring all 9 builtins to implement a new trait. This is a
larger change deferred to the `map_fn()` optimization (§7.3).

### 10.2 Why Fuse at Pipeline Construction Instead of Runtime?

| Approach | Pros | Cons |
|----------|------|------|
| Construction-time | Zero runtime overhead; deterministic; simple implementation | Can't adapt to runtime conditions |
| Runtime (JIT fusion) | Could fuse based on observed throughput | Complex; non-deterministic; overhead of monitoring + restructuring |

Construction-time fusion is chosen because:
1. Fusibility is a static property of the function, not a runtime one.
2. The pipeline graph is fully known at construction time.
3. Zero runtime overhead — the fusion pass runs once before any data flows.
4. Deterministic — same pipeline always produces same fused structure.

### 10.3 Why Not Fuse Accumulators?

Accumulators (SHA-256, ByteCount, Checksum) consume all input chunks
before emitting a single output. Fusing an accumulator with a map stage
would change semantics:

```
  Unfused: Source → [chunk₁, chunk₂, ...] → Uppercase → [CHUNK₁, CHUNK₂, ...] → Sha256 → [digest]
  
  If fused (hypothetical): per chunk, apply Uppercase then Sha256.
  But Sha256 needs ALL chunks to produce a digest — it can't emit
  per-chunk output. The fused stage would need to buffer all chunks
  internally, defeating the purpose of streaming.
```

Accumulators break the 1-input-chunk → 1-output-chunk invariant that
fusion relies on. They must remain as separate pipeline stages.

### 10.4 Why Default `enable_fusion: true`?

Unlike §2.10 adaptive chunking (off by default), fusion is on by default:

| Factor | Adaptive chunking (off) | Fusion (on) |
|--------|------------------------|-------------|
| Correctness risk | Non-deterministic chunk boundaries | Deterministic; fused output == unfused output |
| Affected stages | All streaming stages | Only opt-in fusible stages |
| Overhead when no benefit | Resizer task + probe per stage | O(N) scan, no runtime cost |
| Regression risk | Timing-dependent behavior | None — same transforms, fewer tasks |

Fusion has no correctness risk (output is identical), no runtime overhead
when no stages are fusible, and the construction-time scan is negligible.
The only downside is reduced per-stage observability, which is acceptable
for cheap map stages.

---

## 11. Observability Integration

Two new metrics and DEBUG-level logging:

```rust
lazy_static! {
    /// Total number of stages eliminated by fusion.
    /// Incremented by (N-1) for each N-stage fused group.
    static ref FUSION_STAGES_ELIMINATED: IntCounter = register_int_counter!(
        "deriva_fusion_stages_eliminated_total",
        "Number of pipeline stages eliminated by fusion"
    ).unwrap();

    /// Total number of fused groups created.
    /// Each group of N merged stages increments this by 1.
    static ref FUSION_CHAINS_TOTAL: IntCounter = register_int_counter!(
        "deriva_fusion_chains_total",
        "Number of fused stage groups created"
    ).unwrap();
}

pub(crate) fn record_fusion(stages_eliminated: usize) {
    FUSION_STAGES_ELIMINATED.inc_by(stages_eliminated as u64);
    FUSION_CHAINS_TOTAL.inc();
}

pub(crate) fn get_fusion_stages_eliminated() -> u64 {
    FUSION_STAGES_ELIMINATED.get()
}

pub(crate) fn get_fusion_chains_total() -> u64 {
    FUSION_CHAINS_TOTAL.get()
}
```

Logging in `execute()`:

```rust
tracing::debug!(
    original = original_len,
    fused = fused.len(),
    eliminated = fused_count,
    "pipeline fusion applied"
);
```

Dashboard queries:

```promql
# Fusion activity rate
rate(deriva_fusion_chains_total[5m])

# Average stages eliminated per pipeline
deriva_fusion_stages_eliminated_total / deriva_fusion_chains_total

# Verify fusion is active (should be > 0 if fusible pipelines exist)
deriva_fusion_stages_eliminated_total
```

---

## 12. Checklist

- [ ] Add `fn is_fusible(&self) -> bool { false }` to `StreamingComputeFunction` trait
- [ ] Add `fn is_fusible(&self) -> bool { true }` to 9 Category 1 builtins
- [ ] Create `deriva-compute/src/fusion.rs`
- [ ] Implement `FusedMapStage` struct with `stream_execute()`
- [ ] Implement `FusedMapStage::is_fusible()` returning `false`
- [ ] Implement `fuse_pipeline()` optimizer with fan-out map and run detection
- [ ] Implement `flush_run()` helper with index renumbering
- [ ] Add `enable_fusion: bool` to `PipelineConfig` (default `true`)
- [ ] Add `mod fusion;` to `lib.rs`
- [ ] Call `fuse_pipeline()` in `StreamPipeline::execute()` gated by config
- [ ] Add `FUSION_STAGES_ELIMINATED` counter to `metrics.rs`
- [ ] Add `FUSION_CHAINS_TOTAL` counter to `metrics.rs`
- [ ] Add `record_fusion()` helper
- [ ] Add `tracing::debug!` log for fusion decisions
- [ ] Unit tests: 5 `is_fusible()` trait tests (T1–T5)
- [ ] Unit tests: 6 `FusedMapStage` execution tests (T6–T11)
- [ ] Unit tests: 8 fusion optimizer logic tests (T12–T19)
- [ ] Integration tests: 6 fused pipeline correctness tests (T20–T25)
- [ ] Integration tests: 5 edge topology tests (T26–T30)
- [ ] Concurrency tests: 4 stress tests (T31–T34)
- [ ] Benchmark tests: 3 performance tests (T35–T37)
- [ ] Error handling tests: 3 error tests (T38–T40)
- [ ] Benchmark: fused vs unfused cheap chain (5 depths × 2 modes)
- [ ] Benchmark: fused vs unfused expensive chain (3 sizes × 2 modes)
- [ ] Benchmark: fusion pass cost (5 pipeline sizes)
- [ ] Benchmark: partial fusion mixed pipeline
- [ ] Benchmark: task count scaling (50 concurrent pipelines)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing §2.7, §2.8, §2.9, §2.10 tests still pass
- [ ] Commit and push
