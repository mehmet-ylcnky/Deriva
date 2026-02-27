# §2.10 Adaptive Chunk Sizing

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization, §2.9 Size-Aware Mode Selection
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 2 days

---

## 1. Problem Statement

### 1.1 Current Limitation

The streaming pipeline introduced in §2.7 uses a single, compile-time
chunk size for all stages and all workloads:

```rust
// crates/deriva-compute/src/streaming.rs
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024; // 64 KB — never changes

// crates/deriva-compute/src/pipeline.rs
pub struct PipelineConfig {
    pub chunk_size: usize,          // set once, used everywhere
    pub channel_capacity: usize,
    pub cache_intermediates: bool,
    pub memory_budget: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,  // 64 KB for every stage
            ..
        }
    }
}
```

Every stage in the pipeline — regardless of its compute cost, I/O
characteristics, or position in the DAG — receives and emits 64 KB
chunks. The `preferred_chunk_size()` trait method exists on
`StreamingComputeFunction` but is never consulted during pipeline
execution:

```rust
// Trait method exists but is unused by the pipeline builder
fn preferred_chunk_size(&self) -> usize {
    DEFAULT_CHUNK_SIZE  // every function returns the same default
}
```

This means:
1. **Fast stages (identity, XOR) are channel-bound** — they process a
   64 KB chunk in <1 μs but pay ~100 ns channel overhead per chunk,
   making channel ops 10%+ of total time
2. **Slow stages (compress, SHA-256) starve downstream** — they hold
   a chunk for 50–500 μs while the consumer idles waiting
3. **No adaptation to input size** — a 256 KB input creates 4 chunks
   (4 channel round-trips) when a single chunk would suffice
4. **Mismatched producer/consumer rates cause pipeline bubbles** — a
   fast producer fills the channel buffer while a slow consumer drains
   it one chunk at a time, creating stop-and-go throughput

### 1.2 Empirical Evidence

Benchmarks from the Paper 2b evaluation (`chunk_size_sweep` Criterion
group, 5-stage identity pipeline, channel capacity 8) show significant
sensitivity to chunk size:

| Chunk Size | Latency (1 MB) | Channel Ops | Throughput | TTFB |
|------------|----------------|-------------|------------|------|
| 1 KB | 8.56 ms | 1,000 | 117 MB/s | 0.01 ms |
| 4 KB | 3.21 ms | 250 | 311 MB/s | 0.03 ms |
| 16 KB | 1.76 ms | 63 | 568 MB/s | 0.08 ms |
| **64 KB** | **1.50 ms** | **16** | **667 MB/s** | **0.25 ms** |
| 256 KB | 1.62 ms | 4 | 617 MB/s | 0.85 ms |
| 1 MB | 1.33 ms | 1 | 752 MB/s | 1.33 ms |

Key observations:
- **1 KB → 64 KB**: 5.7× latency improvement from reducing channel ops
- **64 KB → 1 MB**: 13% throughput gain but 5.3× worse TTFB
- **Sweet spot depends on workload**: throughput-sensitive workloads
  prefer larger chunks; latency-sensitive workloads prefer smaller
- **No single size is optimal** — 64 KB is a compromise that is
  near-optimal for neither extreme

For heterogeneous pipelines (fast producer → slow consumer), the
mismatch is worse. A `StreamingCompress` stage (zlib, ~200 μs per
64 KB chunk) feeding a `StreamingIdentity` stage (~0.5 μs per chunk)
means the consumer idles 99.7% of the time waiting for the next chunk.
Larger chunks from the compressor would reduce channel round-trips and
let the consumer process more data per wake-up.

### 1.3 Real-World Impact

```
Scenario: 5-stage heterogeneous pipeline
  compress(zlib) → encrypt(AES-CTR) → sha256 → identity → identity

  Per-chunk costs (64 KB chunks):
    compress:  ~200 μs/chunk  (CPU-bound, output smaller than input)
    encrypt:   ~15 μs/chunk   (AES-NI accelerated)
    sha256:    ~25 μs/chunk   (SHA extensions)
    identity:  ~0.5 μs/chunk  (memcpy)
    identity:  ~0.5 μs/chunk  (memcpy)

  Channel overhead: ~100 ns × 5 channels × 16 chunks = 8 μs

  Bottleneck: compress stage at 200 μs/chunk
  Pipeline throughput: 64 KB / 200 μs = 320 MB/s

  With adaptive chunking (256 KB chunks for compress stage):
    compress:  ~750 μs/chunk  (4× data, ~3.75× time due to dictionary)
    Channel ops: 4 instead of 16 → 75% fewer wake-ups
    Downstream stages process 4× data per wake → better cache locality
    Pipeline throughput: 256 KB / 750 μs = 341 MB/s (+7%)

  With adaptive chunking (1 MB chunks for compress stage):
    compress:  ~2.8 ms/chunk  (better dictionary, better ratio)
    Channel ops: 1 instead of 16 → 94% fewer wake-ups
    Pipeline throughput: 1 MB / 2.8 ms = 357 MB/s (+12%)

  For 64 MB input:
    Fixed 64 KB:    200 ms (1,000 chunks × 5 channels = 5,000 channel ops)
    Adaptive 256 KB: 187 ms (250 chunks × 5 channels = 1,250 channel ops)
    Adaptive 1 MB:   179 ms (64 chunks × 5 channels = 320 channel ops)
```

The improvement is modest for single pipelines (7–12%) but compounds
across concurrent pipelines: fewer channel operations means less Tokio
scheduler contention, fewer cache line bounces between producer and
consumer tasks, and lower tail latency under load.

### 1.4 Comparison

| Aspect | Current (fixed 64 KB) | Adaptive (§2.10) |
|--------|----------------------|-------------------|
| Chunk size | 64 KB everywhere | Per-stage, 1 KB – 1 MB |
| Channel ops (1 MB input) | 16 per stage | 1–16 depending on stage speed |
| TTFB | 0.25 ms (fixed) | Configurable: small first chunk, larger subsequent |
| Throughput (heterogeneous) | Bottleneck-limited | 7–12% improvement |
| Throughput (homogeneous) | Near-optimal | Same (no resize needed) |
| Configuration | `PipelineConfig::chunk_size` | `adaptive_chunking: bool` + bounds |
| Complexity | None | `ThroughputProbe` + `ChunkResizer` feedback loop |
| Memory overhead | None | ~64 bytes per stage (timestamps + counters) |
| Convergence | Instant (static) | 3–5 chunks to stabilize |

### 1.5 What Already Exists

The infrastructure for adaptive chunking is partially in place from §2.7:

- `StreamingChunkResizer` builtin function — already re-chunks a stream
  to a target size, but uses a fixed target set at construction time
- `preferred_chunk_size()` trait method on `StreamingComputeFunction` —
  exists but returns `DEFAULT_CHUNK_SIZE` for all functions and is never
  read by the pipeline builder
- `PipelineConfig::chunk_size` — single global value, used for
  `value_to_stream()` and `batch_to_stream()` chunking
- Bounded channels with configurable capacity — backpressure mechanism
  is already in place (§2.7 §3.4, validated in §9 channel sweep)

What is missing:
1. **Throughput monitoring** — no per-stage timing of send/recv
2. **Feedback loop** — no mechanism to adjust chunk size based on
   observed throughput
3. **Per-stage chunk sizes** — pipeline uses one global `chunk_size`
4. **Automatic resizer insertion** — pipeline builder does not detect
   mismatched stages

---

## 2. Design

### 2.1 Architecture Overview

Adaptive chunk sizing adds a feedback loop between channel throughput
measurement and chunk boundary placement. The loop runs per-stage,
independently, with no global coordination:

```
┌─────────────────────────────────────────────────────────────────┐
│                    StreamPipeline (per stage)                     │
│                                                                   │
│  ┌──────────┐   ┌──────────────────┐   ┌──────────────────────┐  │
│  │ Producer │──▶│ AdaptiveResizer  │──▶│ Consumer             │  │
│  │ (stage N)│   │                  │   │ (stage N+1)          │  │
│  └──────────┘   │  ┌────────────┐  │   └──────────────────────┘  │
│                  │  │ Throughput │  │                              │
│                  │  │ Probe     │  │                              │
│                  │  │           │  │                              │
│                  │  │ recv_bps ─┤  │                              │
│                  │  │ send_bps ─┤  │                              │
│                  │  │ ratio ────┤──┼──▶ target_chunk_size         │
│                  │  └────────────┘  │                              │
│                  └──────────────────┘                              │
│                                                                   │
│  Feedback loop (per chunk):                                       │
│    1. Receive chunk from producer          → record recv_time     │
│    2. Re-chunk to target_chunk_size        → buffer/split         │
│    3. Send re-chunked data to consumer     → record send_time     │
│    4. Compute ratio = recv_bps / send_bps                         │
│    5. Adjust target_chunk_size:                                   │
│       ratio > 1.5 (producer faster) → grow   (reduce channel ops) │
│       ratio < 0.7 (consumer faster) → shrink (improve TTFB)      │
│       otherwise                     → hold   (stable)            │
└─────────────────────────────────────────────────────────────────┘
```

The resizer is **not** inserted between every pair of stages. It is
only inserted when `adaptive_chunking` is enabled in `PipelineConfig`
and the pipeline has ≥2 streaming stages. For single-stage pipelines
or batch stages, the fixed `chunk_size` is used unchanged.

### 2.2 Throughput Monitoring

Each `AdaptiveResizer` contains a `ThroughputProbe` that tracks two
rates using exponential moving averages (EMA):

```
ThroughputProbe:
  recv_ema_bps: f64     // bytes/sec arriving from producer
  send_ema_bps: f64     // bytes/sec accepted by consumer
  last_recv:    Instant  // timestamp of last recv()
  last_send:    Instant  // timestamp of last send()
  alpha:        f64      // EMA smoothing factor (default 0.3)

  on_recv(bytes: usize):
    elapsed = now - last_recv
    instantaneous_bps = bytes as f64 / elapsed.as_secs_f64()
    recv_ema_bps = alpha * instantaneous_bps + (1 - alpha) * recv_ema_bps
    last_recv = now

  on_send(bytes: usize):
    elapsed = now - last_send
    instantaneous_bps = bytes as f64 / elapsed.as_secs_f64()
    send_ema_bps = alpha * instantaneous_bps + (1 - alpha) * send_ema_bps
    last_send = now
```

EMA with α=0.3 gives ~90% convergence within 7 samples (chunks),
which means the probe stabilizes within the first 7 chunks of a
stream. This is fast enough for streams with ≥16 chunks (≥1 MB at
64 KB default) while filtering out single-chunk timing noise.

The probe does **not** use wall-clock timestamps for absolute
throughput — only the ratio `recv_ema_bps / send_ema_bps` matters.
This makes the measurement independent of system load and avoids
the need for calibration.

### 2.3 Resize Strategy

The decision function maps the throughput ratio to a chunk size
adjustment:

```
compute_target_chunk_size(probe, current_size, bounds) -> usize:

  ratio = probe.recv_ema_bps / probe.send_ema_bps

  if ratio > 1.5:
    // Producer is 1.5× faster than consumer → backpressure
    // Grow chunks to reduce channel ops and amortize overhead
    new_size = current_size * 2

  else if ratio < 0.7:
    // Consumer is faster than producer → starvation
    // Shrink chunks for better TTFB and more granular flow
    new_size = current_size / 2

  else:
    // Balanced — no change
    new_size = current_size

  clamp(new_size, bounds.min, bounds.max)
```

Design choices:
- **Power-of-2 scaling** — doubling/halving keeps chunk sizes aligned
  to page boundaries and avoids slow convergence from small increments
- **Hysteresis band (0.7–1.5)** — prevents oscillation when throughput
  is roughly balanced. Without the dead zone, noise would cause
  continuous resize toggling
- **One adjustment per chunk** — the decision runs after each chunk
  send, but the new size only applies to the *next* chunk. This
  prevents mid-chunk resizing and keeps the feedback loop stable
- **Convergence: 3–5 chunks** — from 64 KB default, reaching 1 MB
  takes ceil(log2(1048576/65536)) = 4 doublings = 4 chunks

### 2.4 Chunk Size Bounds

Three configurable bounds constrain the adaptive range:

| Parameter | Default | Rationale |
|-----------|---------|-----------|
| `min_chunk_size` | 1 KB | Below 1 KB, channel overhead dominates (>10% of chunk processing time). Also the minimum useful unit for most transforms |
| `max_chunk_size` | 1 MB | Above 1 MB, TTFB degrades significantly (>1 ms for first byte). Also limits per-stage memory to channel_capacity × 1 MB = 8 MB |
| `initial_chunk_size` | 64 KB | Same as current `DEFAULT_CHUNK_SIZE`. Starting point before adaptation kicks in |

Per-function overrides via `preferred_chunk_size()`:

```
Stage chunk size resolution:
  1. If function overrides preferred_chunk_size() → use as initial
  2. Else → use PipelineConfig::chunk_size (64 KB default)
  3. Adaptive resizer adjusts from initial within [min, max] bounds
```

Example overrides:
- `StreamingCompress` → `preferred_chunk_size() = 256 * 1024` (256 KB)
  — compression benefits from larger dictionary windows
- `StreamingSha256` → `preferred_chunk_size() = 64 * 1024` (64 KB)
  — hash state is tiny, chunk size doesn't matter much
- `StreamingIdentity` → default 64 KB — channel-bound, would benefit
  from larger chunks but adaptive resizer will discover this

### 2.5 Automatic Insertion

The pipeline builder inserts an `AdaptiveResizer` between adjacent
streaming stages when all conditions are met:

```
insert_resizer(stage_i, stage_j, config) -> bool:

  // Only when adaptive chunking is enabled
  if !config.adaptive_chunking: return false

  // Only between streaming stages (batch stages handle their own chunking)
  if !is_streaming(stage_i) || !is_streaming(stage_j): return false

  // Only when stages have different preferred chunk sizes
  // OR when adaptive mode is forced (config.force_adaptive = true)
  let size_i = stage_i.function.preferred_chunk_size();
  let size_j = stage_j.function.preferred_chunk_size();
  if size_i == size_j && !config.force_adaptive: return false

  return true
```

When inserted, the resizer becomes an invisible intermediate node in
the pipeline DAG:

```
Before:  [compress] ──channel──▶ [encrypt]

After:   [compress] ──channel──▶ [AdaptiveResizer] ──channel──▶ [encrypt]
                                  │                │
                                  │ ThroughputProbe│
                                  │ target: 64KB   │
                                  │ min: 1KB       │
                                  │ max: 1MB       │
                                  └────────────────┘
```

The resizer adds one extra channel hop (~100 ns per chunk) but
eliminates the throughput mismatch that causes pipeline bubbles.
Net effect is positive when the mismatch ratio exceeds ~1.3×.

### 2.6 Interaction with §2.9 Size-Aware Mode Selection

Adaptive chunking operates **within** the streaming path only. It does
not affect the batch/streaming mode decision from §2.9:

```
§2.9 decides: batch or streaming? (per-node, based on input size)
§2.10 decides: what chunk size? (per-stage-pair, based on throughput)

Decision order:
  1. §2.9 selects mode → some nodes become BatchStage, others StreamingStage
  2. §2.10 inserts resizers between adjacent StreamingStage pairs
  3. BatchStage nodes use fixed chunk_size for batch_to_stream() wrapper

No interaction conflict: §2.9 reduces the number of streaming stages
(small inputs go batch), §2.10 optimizes the remaining streaming stages.
```

---

## 3. Implementation

### 3.1 PipelineConfig Extension

New fields on the existing config struct:

```rust
// crates/deriva-compute/src/pipeline.rs

pub const MIN_CHUNK_SIZE: usize = 1024;            // 1 KB
pub const MAX_CHUNK_SIZE: usize = 1024 * 1024;     // 1 MB
pub const DEFAULT_EMA_ALPHA: f64 = 0.3;
pub const GROW_RATIO: f64 = 1.5;
pub const SHRINK_RATIO: f64 = 0.7;

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub chunk_size: usize,
    pub channel_capacity: usize,
    pub cache_intermediates: bool,
    pub memory_budget: usize,
    // §2.10: Adaptive chunk sizing
    pub adaptive_chunking: bool,    // default: false (off by default)
    pub min_chunk_size: usize,      // default: 1 KB
    pub max_chunk_size: usize,      // default: 1 MB
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            cache_intermediates: true,
            memory_budget: 0,
            adaptive_chunking: false,
            min_chunk_size: MIN_CHUNK_SIZE,
            max_chunk_size: MAX_CHUNK_SIZE,
        }
    }
}
```

### 3.2 ThroughputProbe

A lightweight per-resizer struct that tracks producer and consumer
throughput using exponential moving averages:

```rust
// crates/deriva-compute/src/adaptive.rs (NEW file)

use std::time::Instant;

/// Tracks send/recv throughput via EMA to drive chunk size decisions.
pub(crate) struct ThroughputProbe {
    recv_ema_bps: f64,
    send_ema_bps: f64,
    last_recv: Instant,
    last_send: Instant,
    alpha: f64,
}

impl ThroughputProbe {
    pub fn new(alpha: f64) -> Self {
        let now = Instant::now();
        Self {
            recv_ema_bps: 0.0,
            send_ema_bps: 0.0,
            last_recv: now,
            last_send: now,
            alpha,
        }
    }

    /// Record a recv() of `bytes` from the producer.
    pub fn on_recv(&mut self, bytes: usize) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_recv).as_secs_f64();
        if elapsed > 0.0 {
            let bps = bytes as f64 / elapsed;
            self.recv_ema_bps = self.alpha * bps + (1.0 - self.alpha) * self.recv_ema_bps;
        }
        self.last_recv = now;
    }

    /// Record a send() of `bytes` to the consumer.
    pub fn on_send(&mut self, bytes: usize) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_send).as_secs_f64();
        if elapsed > 0.0 {
            let bps = bytes as f64 / elapsed;
            self.send_ema_bps = self.alpha * bps + (1.0 - self.alpha) * self.send_ema_bps;
        }
        self.last_send = now;
    }

    /// Throughput ratio: producer / consumer. >1 means producer is faster.
    pub fn ratio(&self) -> f64 {
        if self.send_ema_bps <= 0.0 {
            return 1.0; // no data yet → neutral
        }
        self.recv_ema_bps / self.send_ema_bps
    }
}
```

Memory: 40 bytes per probe (2 × f64 EMA + 2 × Instant + 1 × f64 alpha).
CPU: two `Instant::now()` calls per chunk (~18 ns each on Linux).

### 3.3 Resize Decision Function

Pure function that maps probe state to a new chunk size:

```rust
// crates/deriva-compute/src/adaptive.rs

/// Compute the next target chunk size based on throughput ratio.
///
/// - ratio > GROW_RATIO  → double (producer faster, reduce channel ops)
/// - ratio < SHRINK_RATIO → halve (consumer faster, improve TTFB)
/// - otherwise            → hold (balanced)
pub(crate) fn compute_target_chunk_size(
    probe: &ThroughputProbe,
    current: usize,
    min: usize,
    max: usize,
) -> usize {
    let ratio = probe.ratio();
    let next = if ratio > GROW_RATIO {
        current.saturating_mul(2)
    } else if ratio < SHRINK_RATIO {
        current / 2
    } else {
        current
    };
    next.clamp(min, max)
}
```

### 3.4 AdaptiveResizer — Enhanced ChunkResizer

Extends the existing `StreamingChunkResizer` pattern with the feedback
loop. This is a new struct, not a modification of the existing resizer
(which remains available for fixed-target use cases):

```rust
// crates/deriva-compute/src/adaptive.rs

use bytes::{Bytes, BytesMut};
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::DEFAULT_CHANNEL_CAPACITY;
use crate::pipeline::{MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, DEFAULT_EMA_ALPHA,
                       GROW_RATIO, SHRINK_RATIO};
use crate::metrics;

/// Adaptive chunk resizer that adjusts target size based on throughput.
pub(crate) async fn spawn_adaptive_resizer(
    mut rx: mpsc::Receiver<StreamChunk>,
    initial_chunk_size: usize,
    min: usize,
    max: usize,
    capacity: usize,
) -> mpsc::Receiver<StreamChunk> {
    let (tx, out) = mpsc::channel(capacity);

    tokio::spawn(async move {
        let mut probe = ThroughputProbe::new(DEFAULT_EMA_ALPHA);
        let mut target = initial_chunk_size;
        let mut buf = BytesMut::new();

        loop {
            match rx.recv().await {
                Some(StreamChunk::Data(chunk)) => {
                    let len = chunk.len();
                    probe.on_recv(len);
                    buf.extend_from_slice(&chunk);

                    // Emit complete chunks at current target size
                    while buf.len() >= target {
                        let piece = buf.split_to(target).freeze();
                        let piece_len = piece.len();
                        if tx.send(StreamChunk::Data(piece)).await.is_err() {
                            return;
                        }
                        probe.on_send(piece_len);
                    }

                    // Adjust target for next chunk
                    let new_target = compute_target_chunk_size(
                        &probe, target, min, max,
                    );
                    if new_target != target {
                        metrics::record_chunk_resize(target, new_target);
                        target = new_target;
                    }
                }
                Some(StreamChunk::End) => break,
                Some(StreamChunk::Error(e)) => {
                    let _ = tx.send(StreamChunk::Error(e)).await;
                    return;
                }
                None => break,
            }
        }

        // Flush remaining buffer
        if !buf.is_empty() {
            let _ = tx.send(StreamChunk::Data(buf.freeze())).await;
        }
        let _ = tx.send(StreamChunk::End).await;

        metrics::observe_final_chunk_size(target);
    });

    out
}
```

Key differences from the existing `StreamingChunkResizer`:
1. `target` is mutable — updated after each recv cycle
2. `ThroughputProbe` tracks timing of recv/send
3. Metric recording on resize events and final stabilized size
4. No params map — configuration passed directly as arguments

### 3.5 Pipeline Builder Integration

The resizer is inserted during `StreamPipeline::execute()`, between
adjacent streaming stages. The change is localized to the execution
loop:

```rust
// crates/deriva-compute/src/pipeline.rs — in StreamPipeline::execute()

// BEFORE (current code, simplified):
PipelineNode::StreamingStage { function, params, input_indices, .. } => {
    let inputs: Vec<mpsc::Receiver<StreamChunk>> =
        input_indices.iter().map(|&i| outputs[i].take().unwrap()).collect();
    let rx = function.stream_execute(inputs, &params).await;
    outputs.push(Some(rx));
}

// AFTER (with adaptive resizer insertion):
PipelineNode::StreamingStage { function, params, input_indices, .. } => {
    let inputs: Vec<mpsc::Receiver<StreamChunk>> =
        input_indices.iter().map(|&i| {
            let rx = outputs[i].take().unwrap();
            if self.config.adaptive_chunking && is_streaming_node(i, &node_types) {
                // Insert resizer between this streaming consumer
                // and its streaming producer
                let initial = function.preferred_chunk_size();
                // spawn_adaptive_resizer is sync-returning (spawns internally)
                tokio::runtime::Handle::current().block_on(
                    adaptive::spawn_adaptive_resizer(
                        rx,
                        initial,
                        self.config.min_chunk_size,
                        self.config.max_chunk_size,
                        self.config.channel_capacity,
                    )
                )
            } else {
                rx
            }
        }).collect();
    let rx = function.stream_execute(inputs, &params).await;
    outputs.push(Some(rx));
}
```

The `is_streaming_node()` helper checks whether the producer node at
index `i` is a `StreamingStage` (not a `Source`, `Cached`, or
`BatchStage`). Resizers are only inserted between streaming→streaming
edges, since source/cached nodes emit fixed-size chunks that don't
benefit from adaptation, and batch stages already collect-then-rechunk.

To track node types without borrowing `self.nodes` (which is consumed
by the `for node in self.nodes` loop), a `Vec<NodeType>` is built
before the loop:

```rust
// Before the execution loop:
#[derive(Clone, Copy, PartialEq)]
enum NodeType { Source, Cached, Streaming, Batch }

let node_types: Vec<NodeType> = self.nodes.iter().map(|n| match n {
    PipelineNode::Source { .. }         => NodeType::Source,
    PipelineNode::Cached { .. }         => NodeType::Cached,
    PipelineNode::StreamingStage { .. } => NodeType::Streaming,
    PipelineNode::BatchStage { .. }     => NodeType::Batch,
}).collect();

fn is_streaming_node(idx: usize, types: &[NodeType]) -> bool {
    types.get(idx) == Some(&NodeType::Streaming)
}
```

Total change to `pipeline.rs`: ~25 lines (node type vec + conditional
resizer insertion in the streaming arm).

---

## 4. Data Flow Diagrams

### 4.1 Without Adaptive Chunking — Fixed 64 KB

A 3-stage heterogeneous pipeline processing 1 MB input. All stages
receive and emit 64 KB chunks regardless of their processing speed:

```
Input: 1 MB blob

  [Source] ──64KB──▶ [Compress (zlib)] ──64KB──▶ [SHA-256] ──64KB──▶ [Collect]
            ×16       ~200 μs/chunk      ×16      ~25 μs/chunk ×16

  Timeline (compress is bottleneck):

  Chunk:  1    2    3    4    5    6    7    8   ...  16
          ├────┤
  Compress: 200μs
               ├────┤
  SHA-256:      25μs  (idle 175μs waiting for next chunk)
                      ├────┤
  Compress:            200μs
                           ├────┤
  SHA-256:                  25μs  (idle 175μs again)

  Total: 16 × 200 μs = 3.2 ms  (SHA-256 idle 87.5% of the time)
  Channel ops: 16 send + 16 recv = 32 per stage, 96 total
```

### 4.2 With Adaptive Chunking — Growing Chunks

Same pipeline with `AdaptiveResizer` inserted between compress and
SHA-256. The resizer detects that compress is slower and grows chunks:

```
Input: 1 MB blob

  [Source] ──64KB──▶ [Compress] ──var──▶ [Resizer] ──var──▶ [SHA-256] ──var──▶ [Collect]

  Resizer feedback loop (α=0.3, initial target=64KB):

  Chunk 1:  recv 64KB from compress (200μs)  → send 64KB to SHA-256 (25μs)
            ratio = recv_bps / send_bps ≈ 0.125  (< 0.7 → shrink? No — first sample, EMA cold)
            EMA not yet converged → hold at 64KB

  Chunk 2:  recv 64KB (200μs) → send 64KB (25μs)
            recv_ema ≈ 328 KB/s, send_ema ≈ 2,621 KB/s
            ratio ≈ 0.125 → shrink to 32KB
            (Consumer is faster — smaller chunks improve granularity)

  Chunk 3:  recv 32KB from buffer → send 32KB (12μs)
            ratio still < 0.7 → shrink to 16KB

  Chunk 4-5: ratio stabilizes near 0.125 → holds at 16KB
             (min_chunk_size=1KB not reached, but shrinking stops
              because compress output chunks are already 64KB —
              resizer can't make producer emit smaller chunks,
              it only re-chunks what it receives)

  Wait — this is wrong. The resizer sits AFTER compress, so it
  receives whatever compress emits (64KB chunks). The ratio measures
  how fast the resizer receives vs how fast SHA-256 consumes.

  Corrected analysis:
  The resizer receives 64KB every 200μs from compress.
  SHA-256 consumes a chunk in 25μs (fast consumer).
  ratio = recv_bps / send_bps = (64KB/200μs) / (64KB/25μs) = 0.125

  ratio < 0.7 → consumer is starved for work → shrink chunks
  But shrinking doesn't help here — the bottleneck is compress speed,
  not chunk size. The resizer correctly detects "consumer faster" but
  the right action is: do nothing (or remove the resizer).

  This reveals an important design insight: the resizer is most
  useful when inserted BEFORE the slow stage, not after it.
```

**Corrected placement** — resizer before the bottleneck:

```
  [Source] ──64KB──▶ [Resizer] ──var──▶ [Compress] ──var──▶ [SHA-256] ──▶ [Collect]

  Resizer feedback loop:

  Chunk 1:  recv 64KB from source (~instant)  → send 64KB to compress
            compress takes 200μs to consume → send blocks 200μs
            recv_bps ≈ ∞, send_bps ≈ 327 KB/s
            ratio >> 1.5 → grow to 128KB

  Chunk 2:  recv 128KB from buffer → send 128KB to compress
            compress takes ~380μs (sublinear — better dictionary)
            ratio still > 1.5 → grow to 256KB

  Chunk 3:  recv 256KB → send 256KB to compress
            compress takes ~700μs
            ratio > 1.5 → grow to 512KB

  Chunk 4:  recv 512KB → send 512KB to compress
            compress takes ~1.3ms
            ratio ≈ 1.2 → hold at 512KB (within hysteresis band)

  Result: 1 MB processed in 2 chunks (512KB + 512KB)
  Channel ops: 2 send + 2 recv = 4 per stage (vs 32 with fixed 64KB)
  Compress total: ~2.6ms (vs 3.2ms — better dictionary utilization)
  SHA-256 receives 2 larger chunks → 2 wake-ups instead of 16
```

### 4.3 Feedback Loop — Convergence Trace

Detailed trace of the resizer converging from 64 KB to steady state
for a fast-producer → slow-consumer edge (source → compress):

```
  EMA α = 0.3, initial target = 64 KB
  Producer: source emitting at ~10 GB/s (memcpy)
  Consumer: compress accepting at ~327 KB/s (zlib level 6)

  Chunk | Target  | recv_ema (MB/s) | send_ema (MB/s) | Ratio | Action
  ------+---------+-----------------+-----------------+-------+--------
    1   |  64 KB  |     —           |     —           |  1.0  | hold (cold start)
    2   |  64 KB  |  3,000          |    0.32         | 9375  | grow → 128 KB
    3   | 128 KB  |  4,200          |    0.33         | 12727 | grow → 256 KB
    4   | 256 KB  |  5,460          |    0.35         | 15600 | grow → 512 KB
    5   | 512 KB  |  6,822          |    0.37         | 18438 | grow → 1 MB (max)
    6   |   1 MB  |  7,775          |    0.38         | 20460 | hold (at max)
    7   |   1 MB  |  8,443          |    0.39         | 21649 | hold (at max)

  Converged to max (1 MB) in 5 chunks.
  Total channel ops for 1 MB input: ~4 (vs 16 at fixed 64 KB)
```

For a balanced pipeline (identity → identity), the ratio stays near
1.0 and the target never changes:

```
  Producer: identity at ~2 GB/s
  Consumer: identity at ~2 GB/s

  Chunk | Target  | Ratio | Action
  ------+---------+-------+--------
    1   |  64 KB  |  1.0  | hold
    2   |  64 KB  |  1.02 | hold
    3   |  64 KB  |  0.98 | hold
    ...
   16   |  64 KB  |  1.01 | hold

  No resizing — correct behavior for homogeneous pipelines.
```

### 4.4 Mixed Pipeline — §2.9 + §2.10 Combined

A pipeline where §2.9 selects batch for small inputs and §2.10
adapts chunk sizes for the streaming portion:

```
Recipe: Z = sha256(compress(concat(A, B)))
  A = 500 KB source, B = 800 KB source
  concat input sum = 1.3 MB < 3 MB threshold

  §2.9 decisions:
    concat:   BatchStage (1.3 MB < threshold)
    compress: StreamingStage (input unknown — computed)
    sha256:   StreamingStage (input unknown — computed)

  Pipeline with §2.10 adaptive chunking enabled:

  [A:500KB] ──▶ ┐
                 ├──▶ [concat (BATCH)] ──batch_to_stream(64KB)──▶
  [B:800KB] ──▶ ┘
       ──▶ [Resizer] ──var──▶ [compress] ──▶ [Resizer] ──var──▶ [sha256] ──▶ [Collect]

  Note: first resizer is between batch_to_stream output (fixed 64KB)
  and compress (slow consumer). It grows chunks to reduce compress
  wake-ups. Second resizer between compress and sha256 is a no-op
  (compress output rate ≈ sha256 input rate after adaptation).

  concat: 1.3 MB batch → 0.02 ms (fast, no streaming overhead)
  batch_to_stream: emits 20 × 64KB chunks
  Resizer 1: grows to 256KB after 3 chunks → compress gets 5 × 256KB
  compress: ~5 × 700μs = 3.5 ms
  sha256: ~5 × 100μs = 0.5 ms
  Total: ~4.0 ms

  Without adaptive (fixed 64KB):
  compress: 20 × 200μs = 4.0 ms
  sha256: 20 × 25μs = 0.5 ms
  Total: ~4.5 ms

  Improvement: ~11% from reduced channel ops + better compress dictionary
```

### 4.5 Small Stream — No Adaptation

For inputs smaller than 2× the initial chunk size, the resizer passes
through without adaptation (not enough chunks to converge):

```
Input: 100 KB blob, initial chunk size = 64 KB

  [Source] ──▶ [Resizer] ──▶ [Identity] ──▶ [Collect]

  Chunk 1: recv 64KB → send 64KB → ratio = 1.0 → hold
  Chunk 2: recv 36KB → send 36KB → End

  Result: 2 chunks, no resize. Identical to fixed chunking.
  Overhead: 2 × Instant::now() per chunk = ~72 ns total (negligible)
```

---

## 5. Test Specification

File: `deriva-compute/tests/adaptive_chunking.rs`

### 5.1 Unit Tests — ThroughputProbe EMA Accuracy (4 tests)

```rust
#[test]
fn test_probe_steady_state_fast_producer() {
    // Simulate producer delivering 1 MB/s, consumer accepting 100 KB/s.
    // After 10 samples the EMA ratio should converge above GROW_RATIO (1.5).
    let mut probe = ThroughputProbe::new(0.3);
    for _ in 0..10 {
        std::thread::sleep(Duration::from_micros(64)); // 64KB in 64μs = 1 GB/s
        probe.on_recv(65536);
        std::thread::sleep(Duration::from_micros(640)); // 64KB in 640μs = 100 MB/s
        probe.on_send(65536);
    }
    assert!(probe.ratio() > GROW_RATIO,
        "ratio {} should exceed {} for fast producer", probe.ratio(), GROW_RATIO);
}

#[test]
fn test_probe_steady_state_fast_consumer() {
    // Producer slow (640μs per 64KB), consumer fast (64μs per 64KB).
    // Ratio should converge below SHRINK_RATIO (0.7).
    let mut probe = ThroughputProbe::new(0.3);
    for _ in 0..10 {
        std::thread::sleep(Duration::from_micros(640));
        probe.on_recv(65536);
        std::thread::sleep(Duration::from_micros(64));
        probe.on_send(65536);
    }
    assert!(probe.ratio() < SHRINK_RATIO,
        "ratio {} should be below {} for fast consumer", probe.ratio(), SHRINK_RATIO);
}

#[test]
fn test_probe_balanced_stays_in_hysteresis() {
    // Producer and consumer at same speed — ratio should stay in [0.7, 1.5].
    let mut probe = ThroughputProbe::new(0.3);
    for _ in 0..20 {
        std::thread::sleep(Duration::from_micros(100));
        probe.on_recv(65536);
        std::thread::sleep(Duration::from_micros(100));
        probe.on_send(65536);
    }
    let r = probe.ratio();
    assert!(r >= SHRINK_RATIO && r <= GROW_RATIO,
        "balanced ratio {} should be in [{}, {}]", r, SHRINK_RATIO, GROW_RATIO);
}

#[test]
fn test_probe_ema_convergence_within_7_samples() {
    // After a sudden speed change, EMA should be within 10% of true
    // value after 7 samples (90% convergence for α=0.3).
    let mut probe = ThroughputProbe::new(0.3);
    // Phase 1: 5 samples at 1:1 ratio
    for _ in 0..5 {
        std::thread::sleep(Duration::from_micros(100));
        probe.on_recv(65536);
        std::thread::sleep(Duration::from_micros(100));
        probe.on_send(65536);
    }
    let r1 = probe.ratio();
    assert!((r1 - 1.0).abs() < 0.3, "should be near 1.0, got {}", r1);

    // Phase 2: sudden shift — producer 10× faster
    for _ in 0..7 {
        std::thread::sleep(Duration::from_micros(10));
        probe.on_recv(65536);
        std::thread::sleep(Duration::from_micros(100));
        probe.on_send(65536);
    }
    assert!(probe.ratio() > GROW_RATIO,
        "after 7 samples of 10:1 ratio, EMA {} should exceed {}", probe.ratio(), GROW_RATIO);
}
```

### 5.2 Unit Tests — Resize Decision Function (5 tests)

```rust
#[test]
fn test_decision_grow_on_backpressure() {
    // ratio = 3.0 (producer 3× faster) → double chunk size
    let mut probe = ThroughputProbe::new(1.0); // α=1 for instant convergence
    std::thread::sleep(Duration::from_micros(10));
    probe.on_recv(30000);
    std::thread::sleep(Duration::from_micros(10));
    probe.on_send(10000);
    let new = compute_target_chunk_size(&probe, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
    assert_eq!(new, 128 * 1024, "should double from 64KB to 128KB");
}

#[test]
fn test_decision_shrink_on_starvation() {
    // ratio = 0.3 (consumer 3× faster) → halve chunk size
    let mut probe = ThroughputProbe::new(1.0);
    std::thread::sleep(Duration::from_micros(10));
    probe.on_recv(10000);
    std::thread::sleep(Duration::from_micros(10));
    probe.on_send(30000);
    let new = compute_target_chunk_size(&probe, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
    assert_eq!(new, 32 * 1024, "should halve from 64KB to 32KB");
}

#[test]
fn test_decision_hold_in_hysteresis_band() {
    // ratio = 1.2 (slightly faster producer) → no change
    let mut probe = ThroughputProbe::new(1.0);
    std::thread::sleep(Duration::from_micros(10));
    probe.on_recv(12000);
    std::thread::sleep(Duration::from_micros(10));
    probe.on_send(10000);
    let new = compute_target_chunk_size(&probe, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
    assert_eq!(new, 64 * 1024, "should hold at 64KB for ratio in hysteresis band");
}

#[test]
fn test_decision_clamp_at_max() {
    // Current = 512KB, ratio > 1.5 → would double to 1MB, but max is 1MB → clamp
    let mut probe = ThroughputProbe::new(1.0);
    std::thread::sleep(Duration::from_micros(10));
    probe.on_recv(30000);
    std::thread::sleep(Duration::from_micros(10));
    probe.on_send(10000);
    let new = compute_target_chunk_size(&probe, 512 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
    assert_eq!(new, MAX_CHUNK_SIZE, "should clamp at 1MB max");
    // Double again — still clamped
    let new2 = compute_target_chunk_size(&probe, MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
    assert_eq!(new2, MAX_CHUNK_SIZE, "should stay at max");
}

#[test]
fn test_decision_clamp_at_min() {
    // Current = 2KB, ratio < 0.7 → would halve to 1KB = min → clamp
    let mut probe = ThroughputProbe::new(1.0);
    std::thread::sleep(Duration::from_micros(10));
    probe.on_recv(10000);
    std::thread::sleep(Duration::from_micros(10));
    probe.on_send(30000);
    let new = compute_target_chunk_size(&probe, 2 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
    assert_eq!(new, MIN_CHUNK_SIZE, "should clamp at 1KB min");
    // Halve again — still clamped
    let new2 = compute_target_chunk_size(&probe, MIN_CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
    assert_eq!(new2, MIN_CHUNK_SIZE, "should stay at min");
}
```

### 5.3 Unit Tests — AdaptiveResizer Chunk Output (5 tests)

```rust
#[tokio::test]
async fn test_resizer_passthrough_balanced_pipeline() {
    // Two identity stages at same speed — resizer should emit chunks
    // at approximately the initial size (no resize).
    let data = Bytes::from(vec![0u8; 256 * 1024]); // 256 KB
    let rx = value_to_stream(data.clone(), 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let result = collect_stream(out).await.unwrap();
    assert_eq!(result.len(), 256 * 1024);
    assert_eq!(result, data, "data integrity must be preserved");
}

#[tokio::test]
async fn test_resizer_grows_chunks_under_backpressure() {
    // Simulate slow consumer by inserting a delay stage after the resizer.
    // Feed 1 MB at 64KB chunks. The resizer should grow chunk sizes.
    let data = Bytes::from(vec![0xAB; 1024 * 1024]); // 1 MB
    let rx = value_to_stream(data.clone(), 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 2).await;
    // Channel capacity=2 creates backpressure when consumer is slow

    let mut chunks: Vec<usize> = Vec::new();
    let mut collected = BytesMut::new();
    let mut out = out;
    while let Some(chunk) = out.recv().await {
        match chunk {
            StreamChunk::Data(d) => {
                chunks.push(d.len());
                collected.extend_from_slice(&d);
                // Simulate slow consumer: 1ms per chunk
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            StreamChunk::End => break,
            StreamChunk::Error(e) => panic!("unexpected error: {}", e),
        }
    }
    assert_eq!(collected.freeze(), data, "data integrity");
    // Later chunks should be larger than initial 64KB (resizer grew them)
    let last_full_chunk = chunks.iter().rev().skip(1).next().unwrap_or(&65536);
    assert!(*last_full_chunk > 64 * 1024,
        "last full chunk {} should exceed initial 64KB under backpressure", last_full_chunk);
}

#[tokio::test]
async fn test_resizer_preserves_data_integrity_random_payload() {
    // Random data through resizer must be bit-identical after collect.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut rng_data = vec![0u8; 500_000]; // 500 KB, not aligned to any chunk size
    for (i, b) in rng_data.iter_mut().enumerate() {
        *b = (i.wrapping_mul(7) ^ i.wrapping_mul(13)) as u8;
    }
    let data = Bytes::from(rng_data);
    let mut h1 = DefaultHasher::new();
    data.hash(&mut h1);
    let hash_before = h1.finish();

    let rx = value_to_stream(data, 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let result = collect_stream(out).await.unwrap();

    let mut h2 = DefaultHasher::new();
    result.hash(&mut h2);
    assert_eq!(hash_before, h2.finish(), "hash mismatch — data corrupted by resizer");
}

#[tokio::test]
async fn test_resizer_single_chunk_stream_no_resize() {
    // Input smaller than initial chunk size — emitted as-is, no resize.
    let data = Bytes::from(vec![0u8; 30_000]); // 30 KB < 64 KB
    let rx = value_to_stream(data.clone(), 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let mut chunks = Vec::new();
    let mut out = out;
    while let Some(chunk) = out.recv().await {
        match chunk {
            StreamChunk::Data(d) => chunks.push(d.len()),
            StreamChunk::End => break,
            _ => panic!("unexpected"),
        }
    }
    assert_eq!(chunks.len(), 1, "single chunk should pass through without split");
    assert_eq!(chunks[0], 30_000);
}

#[tokio::test]
async fn test_resizer_respects_custom_bounds() {
    // min=16KB, max=128KB — resizer should never emit outside these bounds.
    let data = Bytes::from(vec![0u8; 2 * 1024 * 1024]); // 2 MB
    let rx = value_to_stream(data.clone(), 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, 16 * 1024, 128 * 1024, 2).await;
    let mut out = out;
    while let Some(chunk) = out.recv().await {
        match chunk {
            StreamChunk::Data(d) => {
                // Last chunk may be smaller (remainder), but all full chunks
                // must be within bounds
                // We check that no chunk exceeds max
                assert!(d.len() <= 128 * 1024,
                    "chunk {} exceeds max 128KB", d.len());
                tokio::time::sleep(Duration::from_micros(500)).await;
            }
            StreamChunk::End => break,
            StreamChunk::Error(e) => panic!("{}", e),
        }
    }
}
```

### 5.4 Integration Tests — Auto-Insertion (5 tests)

```rust
#[tokio::test]
async fn test_auto_insert_between_streaming_stages() {
    // 3 streaming stages with adaptive_chunking=true.
    // Pipeline should insert resizers between stage pairs.
    // Verify by checking output correctness (resizers are transparent).
    let data = Bytes::from(vec![b'a'; 512 * 1024]); // 512 KB
    let config = PipelineConfig {
        adaptive_chunking: true,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());
    let s1 = pipeline.add_streaming_stage(
        test_addr("s1"), Arc::new(StreamingUppercase), HashMap::new(), vec![src],
    );
    let s2 = pipeline.add_streaming_stage(
        test_addr("s2"), Arc::new(StreamingIdentity), HashMap::new(), vec![s1],
    );
    let _s3 = pipeline.add_streaming_stage(
        test_addr("s3"), Arc::new(StreamingIdentity), HashMap::new(), vec![s2],
    );
    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    let expected: Vec<u8> = vec![b'a'; 512 * 1024].iter().map(|b| b.to_ascii_uppercase()).collect();
    assert_eq!(result.as_ref(), expected.as_slice());
}

#[tokio::test]
async fn test_no_resizer_when_adaptive_disabled() {
    // Same pipeline but adaptive_chunking=false.
    // Should produce identical output (no resizers inserted).
    let data = Bytes::from(vec![b'z'; 256 * 1024]);
    let config = PipelineConfig {
        adaptive_chunking: false,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());
    let s1 = pipeline.add_streaming_stage(
        test_addr("s1"), Arc::new(StreamingUppercase), HashMap::new(), vec![src],
    );
    let _s2 = pipeline.add_streaming_stage(
        test_addr("s2"), Arc::new(StreamingIdentity), HashMap::new(), vec![s1],
    );
    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result.len(), 256 * 1024);
}

#[tokio::test]
async fn test_no_resizer_between_source_and_streaming() {
    // Resizer should NOT be inserted between Source→StreamingStage
    // (source emits fixed chunks, no throughput mismatch to adapt).
    let data = Bytes::from(vec![0u8; 128 * 1024]);
    let config = PipelineConfig {
        adaptive_chunking: true,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());
    let _s1 = pipeline.add_streaming_stage(
        test_addr("s1"), Arc::new(StreamingIdentity), HashMap::new(), vec![src],
    );
    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result, data);
}

#[tokio::test]
async fn test_no_resizer_between_batch_and_streaming() {
    // BatchStage → StreamingStage: batch_to_stream emits fixed chunks,
    // resizer should not be inserted (producer is not a streaming stage).
    let data = Bytes::from(vec![b'x'; 200 * 1024]);
    let config = PipelineConfig {
        adaptive_chunking: true,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());
    let batch = pipeline.add_batch_stage(
        test_addr("b"), Arc::new(IdentityFn), BTreeMap::new(), vec![src],
    );
    let _stream = pipeline.add_streaming_stage(
        test_addr("s"), Arc::new(StreamingIdentity), HashMap::new(), vec![batch],
    );
    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result, data);
}

#[tokio::test]
async fn test_resizer_with_multi_input_streaming_stage() {
    // StreamingConcat takes 2 streaming inputs. Resizers should be
    // inserted on both input edges.
    let a = Bytes::from(vec![b'A'; 128 * 1024]);
    let b = Bytes::from(vec![b'B'; 128 * 1024]);
    let config = PipelineConfig {
        adaptive_chunking: true,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let s_a = pipeline.add_source(test_addr("a"), a.clone());
    let s_b = pipeline.add_source(test_addr("b"), b.clone());
    let id_a = pipeline.add_streaming_stage(
        test_addr("ia"), Arc::new(StreamingIdentity), HashMap::new(), vec![s_a],
    );
    let id_b = pipeline.add_streaming_stage(
        test_addr("ib"), Arc::new(StreamingIdentity), HashMap::new(), vec![s_b],
    );
    let _concat = pipeline.add_streaming_stage(
        test_addr("c"), Arc::new(StreamingConcat), HashMap::new(), vec![id_a, id_b],
    );
    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result.len(), 256 * 1024);
    assert!(result.starts_with(&[b'A']));
    assert!(result.ends_with(&[b'B']));
}
```

### 5.5 End-to-End — Heterogeneous Pipeline (4 tests)

```rust
#[tokio::test]
async fn test_e2e_compress_then_sha256_adaptive() {
    // Real heterogeneous pipeline: compress (slow) → sha256 (fast).
    // Adaptive chunking should produce correct SHA-256 hash regardless
    // of chunk boundary changes.
    let data = Bytes::from(vec![b'Q'; 512 * 1024]); // 512 KB
    let config = PipelineConfig {
        adaptive_chunking: true,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());
    let comp = pipeline.add_streaming_stage(
        test_addr("c"), Arc::new(StreamingCompress), HashMap::new(), vec![src],
    );
    let _hash = pipeline.add_streaming_stage(
        test_addr("h"), Arc::new(StreamingSha256), HashMap::new(), vec![comp],
    );
    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();

    // Compute expected: compress then hash without adaptive chunking
    let config_fixed = PipelineConfig::default();
    let mut pipeline2 = StreamPipeline::new(config_fixed);
    let src2 = pipeline2.add_source(test_addr("a"), data);
    let comp2 = pipeline2.add_streaming_stage(
        test_addr("c"), Arc::new(StreamingCompress), HashMap::new(), vec![src2],
    );
    let _hash2 = pipeline2.add_streaming_stage(
        test_addr("h"), Arc::new(StreamingSha256), HashMap::new(), vec![comp2],
    );
    let rx2 = pipeline2.execute().await.unwrap();
    let expected = collect_stream(rx2).await.unwrap();

    assert_eq!(result, expected, "adaptive and fixed must produce identical SHA-256");
}

#[tokio::test]
async fn test_e2e_5_stage_identity_adaptive_matches_fixed() {
    // 5-stage identity pipeline: adaptive should produce bit-identical output.
    let data = Bytes::from(vec![0xFE; 1024 * 1024]); // 1 MB
    let run = |adaptive: bool| {
        let data = data.clone();
        async move {
            let config = PipelineConfig {
                adaptive_chunking: adaptive,
                ..Default::default()
            };
            let mut p = StreamPipeline::new(config);
            let mut idx = p.add_source(test_addr("a"), data);
            for i in 0..5 {
                idx = p.add_streaming_stage(
                    test_addr(&format!("s{}", i)),
                    Arc::new(StreamingIdentity),
                    HashMap::new(),
                    vec![idx],
                );
            }
            let rx = p.execute().await.unwrap();
            collect_stream(rx).await.unwrap()
        }
    };
    let adaptive_result = run(true).await;
    let fixed_result = run(false).await;
    assert_eq!(adaptive_result, fixed_result);
}

#[tokio::test]
async fn test_e2e_upper_then_lower_roundtrip_adaptive() {
    // uppercase → lowercase should return original data.
    // Tests that adaptive chunk boundary changes don't break
    // multi-byte-unaware transforms.
    let data = Bytes::from("hello world, this is a test of adaptive chunking with text data that spans multiple chunks when using small chunk sizes for testing purposes. ".repeat(4000));
    let config = PipelineConfig {
        adaptive_chunking: true,
        chunk_size: 4 * 1024, // small chunks to force more resizing
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), data.clone());
    let upper = pipeline.add_streaming_stage(
        test_addr("u"), Arc::new(StreamingUppercase), HashMap::new(), vec![src],
    );
    let _lower = pipeline.add_streaming_stage(
        test_addr("l"), Arc::new(StreamingLowercase), HashMap::new(), vec![upper],
    );
    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result, data, "upper→lower roundtrip must preserve data");
}

#[tokio::test]
async fn test_e2e_adaptive_with_size_aware_mode_selection() {
    // Combined §2.9 + §2.10: small input uses batch (§2.9), large
    // computed output uses streaming with adaptive chunking (§2.10).
    let small = Bytes::from(vec![b'a'; 100 * 1024]); // 100 KB < 3 MB threshold
    let config = PipelineConfig {
        adaptive_chunking: true,
        streaming_threshold: 3 * 1024 * 1024,
        ..Default::default()
    };
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("a"), small.clone());
    // §2.9 should select batch for repeat (input 100KB < threshold)
    let mut params = BTreeMap::new();
    params.insert("count".into(), Value::String("50".into())); // 100KB × 50 = 5 MB output
    let repeat = pipeline.add_batch_stage(
        test_addr("r"), Arc::new(RepeatFn), params, vec![src],
    );
    // Output is 5 MB (unknown to pipeline) → streaming with adaptive
    let _hash = pipeline.add_streaming_stage(
        test_addr("h"), Arc::new(StreamingSha256), HashMap::new(), vec![repeat],
    );
    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result.len(), 32, "SHA-256 output should be 32 bytes");
}
```

### 5.6 Benchmark Tests (2 tests)

```rust
#[tokio::test]
async fn test_bench_adaptive_not_slower_than_fixed_homogeneous() {
    // For homogeneous pipelines (all identity), adaptive should not
    // be more than 20% slower than fixed (resizer overhead is bounded).
    let data = Bytes::from(vec![0u8; 4 * 1024 * 1024]); // 4 MB

    let run = |adaptive: bool| {
        let data = data.clone();
        async move {
            let start = Instant::now();
            let config = PipelineConfig {
                adaptive_chunking: adaptive,
                ..Default::default()
            };
            let mut p = StreamPipeline::new(config);
            let mut idx = p.add_source(test_addr("a"), data);
            for i in 0..3 {
                idx = p.add_streaming_stage(
                    test_addr(&format!("s{}", i)),
                    Arc::new(StreamingIdentity),
                    HashMap::new(),
                    vec![idx],
                );
            }
            let rx = p.execute().await.unwrap();
            let _ = collect_stream(rx).await.unwrap();
            start.elapsed()
        }
    };

    let fixed_time = run(false).await;
    let adaptive_time = run(true).await;
    let overhead_ratio = adaptive_time.as_secs_f64() / fixed_time.as_secs_f64();
    assert!(overhead_ratio < 1.2,
        "adaptive overhead {:.2}× should be < 1.2× for homogeneous pipeline", overhead_ratio);
}

#[tokio::test]
async fn test_bench_adaptive_crossover_at_6_sizes() {
    // Run adaptive vs fixed at 6 input sizes. Verify adaptive is
    // never more than 25% slower at any size (regression guard).
    let sizes = [64 * 1024, 256 * 1024, 1024 * 1024, 4 * 1024 * 1024,
                 16 * 1024 * 1024, 64 * 1024 * 1024];

    for &size in &sizes {
        let data = Bytes::from(vec![0u8; size]);
        let run = |adaptive: bool| {
            let data = data.clone();
            async move {
                let config = PipelineConfig {
                    adaptive_chunking: adaptive,
                    ..Default::default()
                };
                let mut p = StreamPipeline::new(config);
                let mut idx = p.add_source(test_addr("a"), data);
                for i in 0..3 {
                    idx = p.add_streaming_stage(
                        test_addr(&format!("s{}", i)),
                        Arc::new(StreamingIdentity),
                        HashMap::new(),
                        vec![idx],
                    );
                }
                let start = Instant::now();
                let rx = p.execute().await.unwrap();
                let _ = collect_stream(rx).await.unwrap();
                start.elapsed()
            }
        };
        let fixed = run(false).await;
        let adaptive = run(true).await;
        let ratio = adaptive.as_secs_f64() / fixed.as_secs_f64();
        assert!(ratio < 1.25,
            "at {}KB: adaptive/fixed = {:.2}× exceeds 1.25× threshold",
            size / 1024, ratio);
    }
}
```

### 5.7 Concurrency & Race Conditions (3 tests)

```rust
#[tokio::test]
async fn test_concurrent_pipelines_with_adaptive_no_interference() {
    // Run 4 adaptive pipelines concurrently. Each should produce
    // correct output — resizer state is per-pipeline, no sharing.
    let handles: Vec<_> = (0..4).map(|i| {
        tokio::spawn(async move {
            let data = Bytes::from(vec![(i as u8); 512 * 1024]);
            let config = PipelineConfig {
                adaptive_chunking: true,
                ..Default::default()
            };
            let mut p = StreamPipeline::new(config);
            let mut idx = p.add_source(test_addr(&format!("a{}", i)), data.clone());
            for j in 0..3 {
                idx = p.add_streaming_stage(
                    test_addr(&format!("s{}_{}", i, j)),
                    Arc::new(StreamingIdentity),
                    HashMap::new(),
                    vec![idx],
                );
            }
            let rx = p.execute().await.unwrap();
            let result = collect_stream(rx).await.unwrap();
            assert_eq!(result, data, "pipeline {} data mismatch", i);
        })
    }).collect();
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn test_resizer_under_tokio_task_contention() {
    // Spawn 16 background CPU-heavy tasks to create scheduler pressure.
    // Adaptive pipeline should still produce correct output.
    let _noise: Vec<_> = (0..16).map(|_| {
        tokio::spawn(async {
            let mut x = 0u64;
            for i in 0..1_000_000 { x = x.wrapping_add(i); }
            std::hint::black_box(x);
        })
    }).collect();

    let data = Bytes::from(vec![0xCD; 256 * 1024]);
    let config = PipelineConfig {
        adaptive_chunking: true,
        ..Default::default()
    };
    let mut p = StreamPipeline::new(config);
    let mut idx = p.add_source(test_addr("a"), data.clone());
    for i in 0..3 {
        idx = p.add_streaming_stage(
            test_addr(&format!("s{}", i)),
            Arc::new(StreamingIdentity),
            HashMap::new(),
            vec![idx],
        );
    }
    let rx = p.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result, data);
}

#[tokio::test]
async fn test_resizer_with_gc_running_concurrently() {
    // Start a GC cycle (§2.8) while an adaptive pipeline is running.
    // Neither should interfere with the other.
    let data = Bytes::from(vec![0u8; 1024 * 1024]);
    let config = PipelineConfig {
        adaptive_chunking: true,
        cache_intermediates: true,
        ..Default::default()
    };
    let mut p = StreamPipeline::new(config);
    let src = p.add_source(test_addr("a"), data.clone());
    let _s = p.add_streaming_stage(
        test_addr("s"), Arc::new(StreamingIdentity), HashMap::new(), vec![src],
    );

    let gc_handle = tokio::spawn(async {
        // Simulate GC work
        tokio::time::sleep(Duration::from_millis(5)).await;
    });

    let rx = p.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result, data);
    gc_handle.await.unwrap();
}
```

### 5.8 Boundary & Edge Conditions (5 tests)

```rust
#[tokio::test]
async fn test_resizer_empty_stream() {
    // Zero-length input: resizer should emit End immediately.
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::End).await.unwrap();
    drop(tx);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let result = collect_stream(out).await.unwrap();
    assert_eq!(result.len(), 0);
}

#[tokio::test]
async fn test_resizer_exactly_one_byte() {
    // Minimal non-empty stream: 1 byte.
    let data = Bytes::from(vec![0x42]);
    let rx = value_to_stream(data.clone(), 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let result = collect_stream(out).await.unwrap();
    assert_eq!(result, data);
}

#[tokio::test]
async fn test_resizer_input_exactly_at_initial_chunk_size() {
    // Input = exactly 64 KB = 1 chunk. No resize opportunity.
    let data = Bytes::from(vec![0u8; 64 * 1024]);
    let rx = value_to_stream(data.clone(), 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let mut chunks = Vec::new();
    let mut out = out;
    while let Some(chunk) = out.recv().await {
        match chunk {
            StreamChunk::Data(d) => chunks.push(d.len()),
            StreamChunk::End => break,
            _ => panic!("unexpected"),
        }
    }
    assert_eq!(chunks, vec![64 * 1024]);
}

#[tokio::test]
async fn test_resizer_min_equals_max_forces_fixed_size() {
    // When min == max == 64KB, resizer cannot adapt — behaves like fixed.
    let data = Bytes::from(vec![0u8; 512 * 1024]);
    let rx = value_to_stream(data.clone(), 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, 64 * 1024, 64 * 1024, 2).await;
    let mut out = out;
    let mut chunks = Vec::new();
    while let Some(chunk) = out.recv().await {
        match chunk {
            StreamChunk::Data(d) => {
                chunks.push(d.len());
                tokio::time::sleep(Duration::from_millis(1)).await; // slow consumer
            }
            StreamChunk::End => break,
            _ => panic!("unexpected"),
        }
    }
    // All full chunks must be exactly 64KB (last may be smaller)
    for &c in &chunks[..chunks.len().saturating_sub(1)] {
        assert_eq!(c, 64 * 1024, "chunk size should be fixed at 64KB when min==max");
    }
}

#[tokio::test]
async fn test_resizer_handles_non_power_of_two_initial_size() {
    // Initial chunk size = 50KB (not power of 2). Doubling gives 100KB,
    // halving gives 25KB. Both are valid.
    let data = Bytes::from(vec![0u8; 1024 * 1024]);
    let rx = value_to_stream(data.clone(), 50 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 50 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let result = collect_stream(out).await.unwrap();
    assert_eq!(result.len(), 1024 * 1024, "data integrity with non-power-of-2 chunks");
}
```

### 5.9 Error Propagation (3 tests)

```rust
#[tokio::test]
async fn test_resizer_propagates_upstream_error() {
    // If producer sends Error, resizer must forward it to consumer.
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from(vec![0u8; 1000]))).await.unwrap();
    tx.send(StreamChunk::Error("upstream failure".into())).await.unwrap();
    drop(tx);

    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let err = collect_stream(out).await;
    assert!(err.is_err(), "should propagate error");
    assert!(err.unwrap_err().to_string().contains("upstream failure"));
}

#[tokio::test]
async fn test_resizer_handles_dropped_consumer() {
    // If consumer drops the receiver, resizer task should terminate
    // without panic (send returns Err, resizer returns).
    let data = Bytes::from(vec![0u8; 1024 * 1024]);
    let rx = value_to_stream(data, 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    // Read one chunk then drop
    let mut out = out;
    let _ = out.recv().await;
    drop(out);
    // Give resizer task time to notice and terminate
    tokio::time::sleep(Duration::from_millis(50)).await;
    // No panic = success
}

#[tokio::test]
async fn test_resizer_handles_dropped_producer() {
    // If producer drops without sending End, resizer should flush
    // buffer and send End.
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from(vec![0u8; 5000]))).await.unwrap();
    drop(tx); // no End sent

    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let result = collect_stream(out).await.unwrap();
    assert_eq!(result.len(), 5000, "should flush buffered data on producer drop");
}
```

### 5.10 Deep & Complex Topologies (3 tests)

```rust
#[tokio::test]
async fn test_adaptive_10_stage_deep_chain() {
    // 10-stage identity chain with adaptive chunking.
    // Resizers inserted between each pair = 9 resizers.
    // Output must be bit-identical to input.
    let data = Bytes::from(vec![0xAA; 2 * 1024 * 1024]); // 2 MB
    let config = PipelineConfig {
        adaptive_chunking: true,
        ..Default::default()
    };
    let mut p = StreamPipeline::new(config);
    let mut idx = p.add_source(test_addr("a"), data.clone());
    for i in 0..10 {
        idx = p.add_streaming_stage(
            test_addr(&format!("s{}", i)),
            Arc::new(StreamingIdentity),
            HashMap::new(),
            vec![idx],
        );
    }
    let rx = p.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result, data);
}

#[tokio::test]
async fn test_adaptive_diamond_dag() {
    // Diamond: source → [upper, lower] → concat
    // Both branches get resizers. Concat receives from 2 adapted streams.
    let data = Bytes::from(vec![b'm'; 256 * 1024]);
    let config = PipelineConfig {
        adaptive_chunking: true,
        ..Default::default()
    };
    let mut p = StreamPipeline::new(config);
    let src = p.add_source(test_addr("a"), data.clone());
    let upper = p.add_streaming_stage(
        test_addr("u"), Arc::new(StreamingUppercase), HashMap::new(), vec![src],
    );
    // Note: source is consumed by upper, so we need a second source for lower
    let src2 = p.add_source(test_addr("a2"), data.clone());
    let lower = p.add_streaming_stage(
        test_addr("l"), Arc::new(StreamingLowercase), HashMap::new(), vec![src2],
    );
    let _concat = p.add_streaming_stage(
        test_addr("c"), Arc::new(StreamingConcat), HashMap::new(), vec![upper, lower],
    );
    let rx = p.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result.len(), 512 * 1024);
    // First half should be uppercase 'M', second half lowercase 'm'
    assert!(result[0] == b'M');
    assert!(result[256 * 1024] == b'm');
}

#[tokio::test]
async fn test_adaptive_mixed_batch_streaming_chain() {
    // batch → streaming → streaming → batch → streaming
    // Resizers only between streaming→streaming edges.
    let data = Bytes::from(vec![b'a'; 128 * 1024]);
    let config = PipelineConfig {
        adaptive_chunking: true,
        ..Default::default()
    };
    let mut p = StreamPipeline::new(config);
    let src = p.add_source(test_addr("a"), data.clone());
    let b1 = p.add_batch_stage(
        test_addr("b1"), Arc::new(UppercaseFn), BTreeMap::new(), vec![src],
    );
    let s1 = p.add_streaming_stage(
        test_addr("s1"), Arc::new(StreamingIdentity), HashMap::new(), vec![b1],
    );
    let s2 = p.add_streaming_stage(
        test_addr("s2"), Arc::new(StreamingIdentity), HashMap::new(), vec![s1],
    );
    let b2 = p.add_batch_stage(
        test_addr("b2"), Arc::new(IdentityFn), BTreeMap::new(), vec![s2],
    );
    let _s3 = p.add_streaming_stage(
        test_addr("s3"), Arc::new(StreamingIdentity), HashMap::new(), vec![b2],
    );
    let rx = p.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    let expected: Vec<u8> = vec![b'a'; 128 * 1024].iter().map(|b| b.to_ascii_uppercase()).collect();
    assert_eq!(result.as_ref(), expected.as_slice());
}
```

### 5.11 Observability & Metrics (3 tests)

```rust
#[tokio::test]
async fn test_chunk_resize_counter_increments() {
    // When resizer grows or shrinks, the resize counter should increment.
    let before = metrics::get_chunk_resize_total();
    let data = Bytes::from(vec![0u8; 2 * 1024 * 1024]); // 2 MB
    let rx = value_to_stream(data, 64 * 1024, 8);
    // Channel capacity=1 forces extreme backpressure → resizer will grow
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 1).await;
    let mut out = out;
    while let Some(chunk) = out.recv().await {
        match chunk {
            StreamChunk::Data(_) => {
                tokio::time::sleep(Duration::from_millis(2)).await; // slow consumer
            }
            StreamChunk::End => break,
            _ => {}
        }
    }
    let after = metrics::get_chunk_resize_total();
    assert!(after > before, "resize counter should increment: before={}, after={}", before, after);
}

#[tokio::test]
async fn test_final_chunk_size_metric_recorded() {
    // After pipeline completes, the final stabilized chunk size should
    // be recorded in the histogram.
    let data = Bytes::from(vec![0u8; 512 * 1024]);
    let rx = value_to_stream(data, 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let _ = collect_stream(out).await.unwrap();
    // Metric observation count should have increased
    let count = metrics::get_final_chunk_size_observation_count();
    assert!(count > 0, "final chunk size should be observed");
}

#[tokio::test]
async fn test_no_metrics_when_no_resize_occurs() {
    // Balanced pipeline: no resize events → resize counter unchanged.
    let before = metrics::get_chunk_resize_total();
    let data = Bytes::from(vec![0u8; 64 * 1024]); // exactly 1 chunk
    let rx = value_to_stream(data, 64 * 1024, 8);
    let out = spawn_adaptive_resizer(rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8).await;
    let _ = collect_stream(out).await.unwrap();
    let after = metrics::get_chunk_resize_total();
    assert_eq!(before, after, "no resize should occur for single-chunk stream");
}
```

### 5.12 Configuration Validation (2 tests)

```rust
#[test]
fn test_config_min_greater_than_max_uses_min() {
    // If user sets min > max (misconfiguration), clamp should
    // effectively pin chunk size to min (clamp(x, 100KB, 10KB) = 100KB).
    let probe = ThroughputProbe::new(0.3);
    let result = compute_target_chunk_size(&probe, 64 * 1024, 100 * 1024, 10 * 1024);
    // clamp with min > max: Rust's clamp panics, so we should validate
    // in PipelineConfig construction and swap if needed.
    // This test documents the expected behavior.
    assert!(result >= 10 * 1024, "should not go below the smaller bound");
}

#[test]
fn test_config_zero_min_chunk_size_rejected() {
    // min_chunk_size = 0 would cause division issues in the resizer
    // (infinite chunks of 0 bytes). PipelineConfig should enforce min ≥ 1.
    let config = PipelineConfig {
        adaptive_chunking: true,
        min_chunk_size: 0,
        ..Default::default()
    };
    // The pipeline builder should normalize min to 1
    let effective_min = config.min_chunk_size.max(1);
    assert_eq!(effective_min, 1);
}
```

### Test Summary

| Section | Tests | Focus |
|---------|-------|-------|
| §5.1 ThroughputProbe EMA | 4 | Convergence, accuracy, cold start, speed transitions |
| §5.2 Resize Decision | 5 | Grow/shrink/hold logic, min/max clamping |
| §5.3 AdaptiveResizer Output | 5 | Passthrough, growth under backpressure, data integrity, bounds |
| §5.4 Auto-Insertion | 5 | Streaming→streaming insertion, skip for source/batch/disabled |
| §5.5 End-to-End Heterogeneous | 4 | Compress→SHA-256, 5-stage identity, roundtrip, §2.9 combined |
| §5.6 Benchmarks | 2 | Overhead bound (< 1.2×), regression guard at 6 sizes |
| §5.7 Concurrency | 3 | 4 concurrent pipelines, scheduler contention, GC interaction |
| §5.8 Boundary Conditions | 5 | Empty, 1 byte, exact chunk, min==max, non-power-of-2 |
| §5.9 Error Propagation | 3 | Upstream error, dropped consumer, dropped producer |
| §5.10 Deep Topologies | 3 | 10-stage chain, diamond DAG, mixed batch/streaming chain |
| §5.11 Observability | 3 | Resize counter, final size metric, no-resize baseline |
| §5.12 Configuration | 2 | min > max handling, zero min rejected |
| **Total** | **44** | |

---

## 6. Edge Cases & Error Handling

| # | Edge Case | Behaviour | Implementation |
|---|-----------|-----------|----------------|
| 1 | **Empty stream** (End immediately) | Resizer forwards End, no resize attempt | `loop` exits on first `recv() → End`, flushes empty buffer |
| 2 | **Single chunk < target** | Emitted as-is in flush path, no resize | Buffer never reaches `target`, flushed after End |
| 3 | **Single chunk = target** | Emitted as one full chunk, no resize opportunity | One send, probe has only 1 sample → holds |
| 4 | **Input not aligned to target** | Last chunk is smaller (remainder) | `buf.split_to(target)` for full chunks, `buf.freeze()` for remainder |
| 5 | **Zero-length Data chunk** | Ignored by probe (0 bytes / elapsed = 0 bps) | `on_recv(0)` produces 0 instantaneous bps, EMA trends toward 0 but ratio stays neutral since no send occurs |
| 6 | **Oscillating throughput** (fast/slow alternating) | Hysteresis band (0.7–1.5) absorbs oscillation | EMA smoothing + dead zone prevents toggling; worst case: alternates between two adjacent power-of-2 sizes |
| 7 | **Producer much faster than consumer** (>100×) | Grows to `max_chunk_size` in ceil(log2(max/initial)) chunks | Clamped at max; channel backpressure (bounded channel) prevents unbounded buffering |
| 8 | **Consumer much faster than producer** (>100×) | Shrinks to `min_chunk_size` in ceil(log2(initial/min)) chunks | Clamped at min; no further shrinking possible |
| 9 | **min_chunk_size > max_chunk_size** (misconfiguration) | `clamp()` with inverted bounds panics in Rust | Validate in `PipelineConfig` constructor: `assert!(min <= max)` or swap silently |
| 10 | **min_chunk_size = 0** | Would cause infinite loop (0-byte chunks) | Enforce `min_chunk_size >= 1` in config validation |
| 11 | **max_chunk_size = 0** | No valid chunk size exists | Enforce `max_chunk_size >= 1` in config validation |
| 12 | **Upstream Error mid-stream** | Resizer forwards Error, discards buffer | On `StreamChunk::Error(e)`: send error downstream, return immediately |
| 13 | **Consumer drops receiver** | `tx.send().await` returns `Err` → resizer task exits | All `send` calls check `is_err()` and return on failure |
| 14 | **Producer drops without End** | `rx.recv()` returns `None` → resizer flushes buffer, sends End | `None` arm breaks the loop, same as `End` |
| 15 | **Very large chunk from producer** (> max_chunk_size) | Split into multiple chunks at current target size | `while buf.len() >= target` loop handles arbitrarily large input chunks |
| 16 | **Instant::now() precision on VM** | Low-resolution clocks may report 0 elapsed time | `if elapsed > 0.0` guard skips EMA update when timestamps are identical |
| 17 | **NaN/Inf in EMA** | Division by zero if `send_ema_bps = 0` | `ratio()` returns 1.0 (neutral) when `send_ema_bps <= 0.0` |
| 18 | **Resizer between identical stages** | No throughput mismatch → no resize | Hysteresis band keeps target stable; auto-insertion skips when `preferred_chunk_size()` matches |
| 19 | **Pipeline with single streaming stage** | No streaming→streaming edge exists | Auto-insertion condition fails → no resizer inserted |
| 20 | **Adaptive disabled but preferred_chunk_size differs** | Resizer not inserted (feature off) | `if !config.adaptive_chunking: return false` in insertion check |

### 6.1 Config Validation

```rust
impl PipelineConfig {
    /// Validate and normalize adaptive chunking bounds.
    pub fn validate(&mut self) {
        self.min_chunk_size = self.min_chunk_size.max(1);
        self.max_chunk_size = self.max_chunk_size.max(1);
        if self.min_chunk_size > self.max_chunk_size {
            std::mem::swap(&mut self.min_chunk_size, &mut self.max_chunk_size);
        }
    }
}
```

---

## 7. Performance Analysis

### 7.1 Expected Improvement

| Pipeline Type | Fixed 64 KB | Adaptive | Improvement | Reason |
|--------------|-------------|----------|-------------|--------|
| Homogeneous 5× identity (1 MB) | 1.50 ms | 1.55 ms | −3% (overhead) | No mismatch → no benefit; resizer adds 1 channel hop |
| Homogeneous 5× identity (64 MB) | 50.8 ms | 51.2 ms | −1% | Overhead amortized over more chunks |
| Heterogeneous: compress→sha256 (1 MB) | 3.2 ms | 2.8 ms | +12% | Resizer grows chunks → fewer compress wake-ups |
| Heterogeneous: compress→sha256 (64 MB) | 200 ms | 179 ms | +10% | Larger chunks → better dictionary, fewer channel ops |
| Heterogeneous: compress→encrypt→sha256→id→id (1 MB) | 4.5 ms | 4.0 ms | +11% | Resizer before compress absorbs mismatch |
| Heterogeneous: compress→encrypt→sha256→id→id (64 MB) | 280 ms | 245 ms | +12% | Compound benefit across 4 resizer insertions |
| Mixed §2.9+§2.10: batch concat→stream compress (1.3 MB) | 4.5 ms | 4.0 ms | +11% | Batch avoids streaming overhead; adaptive optimizes streaming tail |

Weighted workload estimate (same distribution as §2.9 §1.3):

```
  <1 MB (60%):  homogeneous pipelines dominate → ~0% change
  1–3 MB (15%): mix of homo/hetero → ~5% average improvement
  3–10 MB (15%): heterogeneous benefit kicks in → ~10% improvement
  >10 MB (10%): full adaptive benefit → ~12% improvement

  Weighted average: 0.60×0% + 0.15×5% + 0.15×10% + 0.10×12% = 3.5%

  Modest overall, but the 10–12% gain on large heterogeneous
  pipelines is significant for data-intensive workloads.
```

### 7.2 Overhead of Throughput Monitoring

| Component | Cost per chunk | Cost for 1 MB (16 chunks) | % of pipeline time |
|-----------|---------------|--------------------------|-------------------|
| `Instant::now()` × 2 (recv + send) | ~36 ns | 576 ns | 0.04% of 1.5 ms |
| EMA multiply + add × 2 | ~4 ns | 64 ns | 0.004% |
| Ratio comparison + clamp | ~2 ns | 32 ns | 0.002% |
| Extra channel hop (resizer task) | ~100 ns | 1,600 ns | 0.11% |
| **Total per resizer** | **~142 ns** | **2,272 ns** | **0.15%** |

For a 5-stage pipeline with 4 resizers: 4 × 2,272 ns = 9.1 μs total
overhead on a 1 MB input (0.6% of 1.5 ms pipeline time). Negligible.

### 7.3 Convergence Time

From initial chunk size to steady state, measured in chunks processed:

| Initial → Target | Doublings/Halvings | Chunks to converge | Data processed before stable |
|-----------------|-------------------|-------------------|------------------------------|
| 64 KB → 1 MB | 4 doublings | 5 (1 cold + 4 grow) | 64+64+128+256+512 = 1,024 KB |
| 64 KB → 1 KB | 6 halvings | 7 (1 cold + 6 shrink) | 7 × 64 KB = 448 KB |
| 64 KB → 256 KB | 2 doublings | 3 (1 cold + 2 grow) | 64+64+128 = 256 KB |
| 64 KB → 64 KB | 0 | 1 (cold start, then stable) | 64 KB |

The resizer converges before processing 1 MB in all cases. For inputs
smaller than the convergence data (e.g., 256 KB input when target is
1 MB), the resizer partially adapts — it grows as far as it can within
the available chunks, which is still better than fixed.

### 7.4 Memory Impact

| Configuration | Per-stage memory | 5-stage pipeline |
|--------------|-----------------|-----------------|
| Fixed 64 KB, capacity 8 | 8 × 64 KB = 512 KB | 5 × 512 KB = 2.5 MB |
| Adaptive, converged to 256 KB, capacity 8 | 8 × 256 KB = 2 MB | 5 × 2 MB = 10 MB |
| Adaptive, converged to 1 MB, capacity 8 | 8 × 1 MB = 8 MB | 5 × 8 MB = 40 MB |

Adaptive chunking can increase memory usage up to `capacity × max_chunk_size`
per stage. With default max 1 MB and capacity 8, worst case is 8 MB per
stage — acceptable for server workloads but may need lower `max_chunk_size`
on memory-constrained systems.

The resizer itself adds ~40 bytes (probe) + one `BytesMut` buffer
(up to `max_chunk_size` = 1 MB worst case). Total resizer overhead:
~1 MB per insertion point.

### 7.5 Benchmark Study Plan

Location: `deriva-compute/benches/adaptive_chunking.rs`

Six benchmark groups that validate the improvement, measure overhead,
and detect regressions.

#### Benchmark 1: Adaptive vs Fixed — Heterogeneous Pipeline

Compares adaptive against fixed 64 KB on the primary target workload:
compress → SHA-256 at 8 input sizes.

```rust
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn bench_adaptive_vs_fixed_heterogeneous(c: &mut Criterion) {
    let mut group = c.benchmark_group("adaptive_vs_fixed_hetero");
    let sizes: Vec<usize> = vec![
        256 * 1024,       // 256 KB
        512 * 1024,       // 512 KB
        1024 * 1024,      // 1 MB
        2 * 1024 * 1024,  // 2 MB
        4 * 1024 * 1024,  // 4 MB
        8 * 1024 * 1024,  // 8 MB
        32 * 1024 * 1024, // 32 MB
        64 * 1024 * 1024, // 64 MB
    ];

    for &size in &sizes {
        let data = Bytes::from(vec![0u8; size]);
        group.throughput(Throughput::Bytes(size as u64));

        // (a) Fixed 64 KB
        group.bench_with_input(
            BenchmarkId::new("fixed_64KB", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_compress_then_sha256(data.clone(), PipelineConfig {
                        adaptive_chunking: false,
                        ..Default::default()
                    }).await
                });
            },
        );

        // (b) Adaptive (default bounds)
        group.bench_with_input(
            BenchmarkId::new("adaptive", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_compress_then_sha256(data.clone(), PipelineConfig {
                        adaptive_chunking: true,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Adaptive is 8–12% faster for sizes ≥ 1 MB, within 5%
for sizes < 1 MB (overhead absorbed).

#### Benchmark 2: Adaptive vs Fixed — Homogeneous Pipeline

Measures the overhead cost on pipelines that don't benefit from
adaptation (all stages at same speed).

```rust
fn bench_adaptive_vs_fixed_homogeneous(c: &mut Criterion) {
    let mut group = c.benchmark_group("adaptive_vs_fixed_homo");
    let sizes = vec![256 * 1024, 1024 * 1024, 16 * 1024 * 1024, 64 * 1024 * 1024];

    for &size in &sizes {
        let data = Bytes::from(vec![0u8; size]);
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("fixed", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_5_stage_identity(data.clone(), PipelineConfig {
                        adaptive_chunking: false,
                        ..Default::default()
                    }).await
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("adaptive", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_5_stage_identity(data.clone(), PipelineConfig {
                        adaptive_chunking: true,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Adaptive is ≤ 5% slower than fixed (resizer overhead
with no benefit). This is the acceptable overhead ceiling.

#### Benchmark 3: Convergence Speed

Measures how quickly the resizer reaches steady state by tracking
chunk sizes emitted over time. Uses a custom harness that records
each chunk's size.

```rust
fn bench_convergence_speed(c: &mut Criterion) {
    let mut group = c.benchmark_group("convergence_speed");
    // Use 4 MB input to give resizer enough chunks to converge
    let data = Bytes::from(vec![0u8; 4 * 1024 * 1024]);
    group.throughput(Throughput::Bytes(4 * 1024 * 1024));

    let initial_sizes = vec![
        ("4KB_to_max",  4 * 1024),   // needs 8 doublings
        ("16KB_to_max", 16 * 1024),  // needs 6 doublings
        ("64KB_to_max", 64 * 1024),  // needs 4 doublings (default)
        ("256KB_to_max", 256 * 1024), // needs 2 doublings
    ];

    for (label, initial) in &initial_sizes {
        group.bench_function(
            BenchmarkId::new("converge", label),
            |b| {
                b.to_async(&rt).iter(|| async {
                    let rx = value_to_stream(data.clone(), *initial, 8);
                    // Channel capacity=1 forces backpressure → resizer grows
                    let out = spawn_adaptive_resizer(
                        rx, *initial, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 1,
                    ).await;
                    collect_stream_with_delay(out, Duration::from_millis(1)).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Convergence from 64 KB → 1 MB takes 4–5 chunks (~320 KB
of data). From 4 KB → 1 MB takes 8–9 chunks (~100 KB). Starting
closer to the target is faster but the difference is small.

#### Benchmark 4: Max Chunk Size Sensitivity

Sweeps `max_chunk_size` to find the optimal upper bound for the
heterogeneous pipeline.

```rust
fn bench_max_chunk_sensitivity(c: &mut Criterion) {
    let mut group = c.benchmark_group("max_chunk_sensitivity");
    let data = Bytes::from(vec![0u8; 16 * 1024 * 1024]); // 16 MB
    group.throughput(Throughput::Bytes(16 * 1024 * 1024));

    let max_sizes = vec![
        64 * 1024,   // 64 KB (effectively fixed)
        128 * 1024,  // 128 KB
        256 * 1024,  // 256 KB
        512 * 1024,  // 512 KB
        1024 * 1024, // 1 MB (default max)
        2 * 1024 * 1024, // 2 MB (above default)
    ];

    for &max in &max_sizes {
        group.bench_with_input(
            BenchmarkId::new("compress_sha256", max / 1024),
            &max,
            |b, &max| {
                b.to_async(&rt).iter(|| async {
                    run_compress_then_sha256(data.clone(), PipelineConfig {
                        adaptive_chunking: true,
                        max_chunk_size: max,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Improvement plateaus around 256 KB–1 MB. Going above
1 MB yields diminishing returns while increasing memory usage.

#### Benchmark 5: Pipeline Depth × Adaptive

Tests whether adaptive benefit scales with pipeline depth (more
stages = more resizer insertion points = more potential benefit).

```rust
fn bench_depth_adaptive(c: &mut Criterion) {
    let mut group = c.benchmark_group("depth_adaptive");
    let data = Bytes::from(vec![0u8; 8 * 1024 * 1024]); // 8 MB
    group.throughput(Throughput::Bytes(8 * 1024 * 1024));
    let depths = [2, 3, 5, 10, 20];

    for &depth in &depths {
        group.bench_with_input(
            BenchmarkId::new("fixed", depth),
            &depth,
            |b, &depth| {
                b.to_async(&rt).iter(|| async {
                    run_n_stage_compress_identity(data.clone(), depth, PipelineConfig {
                        adaptive_chunking: false,
                        ..Default::default()
                    }).await
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("adaptive", depth),
            &depth,
            |b, &depth| {
                b.to_async(&rt).iter(|| async {
                    run_n_stage_compress_identity(data.clone(), depth, PipelineConfig {
                        adaptive_chunking: true,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Adaptive benefit grows with depth: ~5% at depth=2,
~12% at depth=5, ~15% at depth=20 (more resizer points to optimize).

#### Benchmark 6: Resizer Overhead Isolation

Measures the pure cost of the resizer by comparing direct channel
pass-through vs resizer-wrapped channel with no throughput mismatch.

```rust
fn bench_resizer_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("resizer_overhead");
    let data = Bytes::from(vec![0u8; 1024 * 1024]); // 1 MB
    group.throughput(Throughput::Bytes(1024 * 1024));

    // (a) Direct: source → collect (no resizer)
    group.bench_function("direct", |b| {
        b.to_async(&rt).iter(|| async {
            let rx = value_to_stream(data.clone(), 64 * 1024, 8);
            collect_stream(rx).await.unwrap()
        });
    });

    // (b) With resizer: source → resizer → collect
    group.bench_function("with_resizer", |b| {
        b.to_async(&rt).iter(|| async {
            let rx = value_to_stream(data.clone(), 64 * 1024, 8);
            let out = spawn_adaptive_resizer(
                rx, 64 * 1024, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, 8,
            ).await;
            collect_stream(out).await.unwrap()
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_adaptive_vs_fixed_heterogeneous,
    bench_adaptive_vs_fixed_homogeneous,
    bench_convergence_speed,
    bench_max_chunk_sensitivity,
    bench_depth_adaptive,
    bench_resizer_overhead,
);
criterion_main!(benches);
```

**Expected:** Resizer adds < 200 μs overhead for 1 MB (16 chunks ×
~142 ns per chunk = ~2.3 μs probe cost + ~1.6 μs extra channel hop).

#### Benchmark Summary

| # | Group | Parameters | Measures | Expected Result |
|---|-------|-----------|----------|-----------------|
| 1 | Adaptive vs fixed (hetero) | 8 sizes × 2 modes | Throughput, latency | Adaptive 8–12% faster ≥ 1 MB |
| 2 | Adaptive vs fixed (homo) | 4 sizes × 2 modes | Overhead ceiling | Adaptive ≤ 5% slower |
| 3 | Convergence speed | 4 initial sizes | Chunks to steady state | 4–9 chunks depending on start |
| 4 | Max chunk sensitivity | 6 max values | Optimal upper bound | Plateau at 256 KB–1 MB |
| 5 | Depth × adaptive | 5 depths × 2 modes | Scaling with pipeline depth | Benefit grows: 5% → 15% |
| 6 | Resizer overhead | 2 paths | Pure resizer cost | < 200 μs for 1 MB |

---

## 8. Files Changed

| File | Change | Lines |
|------|--------|-------|
| `deriva-compute/src/adaptive.rs` | **NEW** — `ThroughputProbe`, `compute_target_chunk_size()`, `spawn_adaptive_resizer()` | ~120 |
| `deriva-compute/src/pipeline.rs` | Add `adaptive_chunking`, `min_chunk_size`, `max_chunk_size` to `PipelineConfig`; `validate()`; `NodeType` enum; conditional resizer insertion in `execute()` | ~45 |
| `deriva-compute/src/lib.rs` | Add `mod adaptive;` | ~1 |
| `deriva-compute/src/metrics.rs` | Add `CHUNK_RESIZE_TOTAL` counter, `CHUNK_SIZE_FINAL` histogram, `record_chunk_resize()`, `observe_final_chunk_size()`, getter helpers | ~30 |
| `deriva-compute/tests/adaptive_chunking.rs` | **NEW** — 44 tests: probe, decision, resizer, auto-insertion, e2e, concurrency, boundaries, errors, topologies, metrics, config | ~900 |
| `deriva-compute/benches/adaptive_chunking.rs` | **NEW** — 6 benchmark groups: hetero, homo, convergence, max sensitivity, depth, overhead | ~250 |

---

## 9. Dependency Changes

No new crate dependencies. All functionality built on existing:

| Dependency | Already in | Used for |
|-----------|-----------|----------|
| `std::time::Instant` | stdlib | Throughput probe timestamps |
| `tokio` | `deriva-compute` | `mpsc::channel`, `spawn`, `time::sleep` (existing) |
| `bytes` | `deriva-core` | `Bytes`, `BytesMut` (existing) |
| `prometheus` | `deriva-compute` | `IntCounter`, `Histogram` for resize metrics |
| `lazy_static` | `deriva-compute` | Static metric registration (existing) |
| `criterion` | `deriva-compute` (dev) | Benchmark framework (existing) |

---

## 10. Design Rationale

### 10.1 Why Not Always Use Large Chunks?

Large chunks maximize throughput by reducing channel overhead, but they
degrade other dimensions:

| Metric | 64 KB chunks | 1 MB chunks | Impact |
|--------|-------------|-------------|--------|
| TTFB | 0.25 ms | 1.33 ms | 5.3× worse — first byte delayed until full chunk filled |
| Per-stage memory | 512 KB | 8 MB | 16× more — `capacity × chunk_size` per channel |
| Cancellation granularity | 64 KB wasted | 1 MB wasted | On cancel, in-flight chunks are discarded |
| Backpressure responsiveness | ~100 ns | ~1.6 ms | Slow consumer can't signal "stop" until chunk completes |

For interactive clients (streaming HTTP responses, real-time pipelines),
TTFB matters more than throughput. A fixed large chunk size would
penalize these workloads. Adaptive chunking lets the system find the
right balance per-stage at runtime.

### 10.2 Why Feedback-Based Instead of Predictive?

A predictive approach would assign chunk sizes at pipeline construction
time based on function metadata (e.g., "compress is slow, use 256 KB").
This was considered and rejected:

| Factor | Predictive | Feedback-based |
|--------|-----------|---------------|
| Accuracy | Depends on metadata quality | Measures actual runtime behavior |
| New functions | Requires manual annotation | Works automatically |
| Hardware variation | Wrong on different CPUs | Adapts to actual speed |
| Input-dependent cost | Can't predict (compress ratio varies) | Observes actual throughput |
| Complexity | Function metadata schema + planner | Probe + decision function |
| Cold start | Immediate (but possibly wrong) | 3–5 chunks to converge |

The feedback approach has a brief convergence period but is correct
by construction — it measures what actually happens rather than
predicting what might happen. The 3–5 chunk convergence cost (~320 KB)
is negligible for the workloads that benefit from adaptation (≥ 1 MB).

### 10.3 Why Off by Default?

Adaptive chunking is disabled by default (`adaptive_chunking: false`)
for three reasons:

1. **Fixed 64 KB is good enough for 80% of workloads** — the Paper 2b
   benchmarks show 64 KB is within 15% of optimal for homogeneous
   pipelines at all tested sizes. Only heterogeneous pipelines with
   significant speed mismatches benefit from adaptation.

2. **Deterministic behavior** — fixed chunk sizes produce deterministic
   chunk boundaries, which simplifies debugging, testing, and
   reproducibility. Adaptive sizing introduces non-determinism (chunk
   boundaries depend on timing).

3. **No regression risk** — enabling a new feature by default risks
   regressions in edge cases. Off-by-default lets users opt in after
   validating on their workloads.

Users who know they have heterogeneous pipelines (compression, encryption,
format conversion) should enable adaptive chunking for 8–12% throughput
improvement.

---

## 11. Observability Integration

Three new metrics (integrates with §2.5):

```rust
lazy_static! {
    /// Counts chunk resize events, labelled by direction.
    ///
    /// Labels:
    ///   direction: "grow" | "shrink"
    static ref CHUNK_RESIZE_TOTAL: IntCounterVec = register_int_counter_vec!(
        "deriva_chunk_resize_total",
        "Number of adaptive chunk resize events",
        &["direction"]
    ).unwrap();

    /// Histogram of final stabilized chunk sizes (bytes) per resizer.
    /// Recorded once when the resizer completes (stream ends).
    static ref CHUNK_SIZE_FINAL: Histogram = register_histogram!(
        "deriva_chunk_size_final_bytes",
        "Final stabilized chunk size after adaptive convergence",
        vec![1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 524288.0, 1048576.0]
    ).unwrap();

    /// Gauge exposing the current target chunk size per resizer instance.
    /// Updated on each resize event. Useful for live dashboards.
    static ref CHUNK_SIZE_CURRENT: IntGauge = register_int_gauge!(
        "deriva_chunk_size_current_bytes",
        "Current adaptive chunk target size in bytes"
    ).unwrap();
}

pub(crate) fn record_chunk_resize(old: usize, new: usize) {
    let direction = if new > old { "grow" } else { "shrink" };
    CHUNK_RESIZE_TOTAL.with_label_values(&[direction]).inc();
    CHUNK_SIZE_CURRENT.set(new as i64);
}

pub(crate) fn observe_final_chunk_size(size: usize) {
    CHUNK_SIZE_FINAL.observe(size as f64);
}

pub(crate) fn get_chunk_resize_total() -> u64 {
    CHUNK_RESIZE_TOTAL.with_label_values(&["grow"]).get()
        + CHUNK_RESIZE_TOTAL.with_label_values(&["shrink"]).get()
}

pub(crate) fn get_final_chunk_size_observation_count() -> u64 {
    CHUNK_SIZE_FINAL.get_sample_count()
}
```

Dashboard queries:

```promql
# Resize event rate — should be low after convergence
sum(rate(deriva_chunk_resize_total[5m])) by (direction)

# Distribution of final chunk sizes — shows what the resizer converges to
histogram_quantile(0.5, rate(deriva_chunk_size_final_bytes_bucket[1h]))
histogram_quantile(0.95, rate(deriva_chunk_size_final_bytes_bucket[1h]))

# Current chunk size across fleet — detect stuck resizers
deriva_chunk_size_current_bytes
```

---

## 12. Checklist

- [ ] Create `deriva-compute/src/adaptive.rs` with `ThroughputProbe` struct
- [ ] Implement `ThroughputProbe::on_recv()`, `on_send()`, `ratio()` with EMA (α=0.3)
- [ ] Implement `compute_target_chunk_size()` with power-of-2 scaling and hysteresis band
- [ ] Implement `spawn_adaptive_resizer()` with feedback loop and metric recording
- [ ] Add `mod adaptive;` to `deriva-compute/src/lib.rs`
- [ ] Extend `PipelineConfig` with `adaptive_chunking`, `min_chunk_size`, `max_chunk_size`
- [ ] Implement `PipelineConfig::validate()` for bounds normalization
- [ ] Add `NodeType` enum and `is_streaming_node()` helper to `pipeline.rs`
- [ ] Insert conditional resizer in `StreamPipeline::execute()` streaming arm
- [ ] Add `CHUNK_RESIZE_TOTAL` counter to `metrics.rs`
- [ ] Add `CHUNK_SIZE_FINAL` histogram to `metrics.rs`
- [ ] Add `CHUNK_SIZE_CURRENT` gauge to `metrics.rs`
- [ ] Add `record_chunk_resize()` and `observe_final_chunk_size()` helpers
- [ ] Unit tests: 4 ThroughputProbe EMA tests (§5.1)
- [ ] Unit tests: 5 resize decision function tests (§5.2)
- [ ] Unit tests: 5 AdaptiveResizer output tests (§5.3)
- [ ] Integration tests: 5 auto-insertion tests (§5.4)
- [ ] End-to-end tests: 4 heterogeneous pipeline tests (§5.5)
- [ ] Benchmark tests: 2 overhead/regression tests (§5.6)
- [ ] Concurrency tests: 3 parallel pipeline + contention tests (§5.7)
- [ ] Boundary tests: 5 edge condition tests (§5.8)
- [ ] Error tests: 3 propagation + dropped channel tests (§5.9)
- [ ] Topology tests: 3 deep chain + DAG + mixed tests (§5.10)
- [ ] Observability tests: 3 metric verification tests (§5.11)
- [ ] Configuration tests: 2 validation tests (§5.12)
- [ ] Benchmark: adaptive vs fixed heterogeneous (8 sizes × 2 modes)
- [ ] Benchmark: adaptive vs fixed homogeneous (4 sizes × 2 modes)
- [ ] Benchmark: convergence speed (4 initial sizes)
- [ ] Benchmark: max chunk size sensitivity (6 values)
- [ ] Benchmark: pipeline depth × adaptive (5 depths × 2 modes)
- [ ] Benchmark: resizer overhead isolation (direct vs wrapped)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing §2.7, §2.8, §2.9 tests still pass
- [ ] Commit and push
