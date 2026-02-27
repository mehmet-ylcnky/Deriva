# §2.12 Memory Budget Enforcement

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 2 days

---

## 1. Problem Statement

### 1.1 Current Limitation

The streaming pipeline has a `memory_budget` field in `PipelineConfig`
but it is never enforced:

```rust
// crates/deriva-compute/src/pipeline.rs
pub struct PipelineConfig {
    pub chunk_size: usize,          // 64 KB default
    pub channel_capacity: usize,    // 8 default
    pub cache_intermediates: bool,
    pub memory_budget: usize,       // 0 = unlimited — NEVER CHECKED
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            cache_intermediates: true,
            memory_budget: 0,  // unlimited by default
        }
    }
}
```

Each streaming stage spawns a Tokio task with its own `mpsc::channel`
of capacity `channel_capacity`. The only memory bound is per-channel:
each channel can hold at most `channel_capacity` chunks. But there is
no global limit across all channels in a pipeline, and no limit across
concurrent pipelines sharing the same process.

This means:
1. **Per-pipeline memory is unbounded** — a pipeline with N stages
   allocates N channels, each buffering up to `capacity × chunk_size`
   bytes. Nothing prevents N from being large.
2. **Concurrent pipelines compound** — M concurrent pipelines each
   with N stages allocate `M × N × capacity × chunk_size` bytes of
   in-flight data with no coordination.
3. **No backpressure across the pipeline** — per-channel backpressure
   only stalls a single producer. A fast source can fill all downstream
   channels simultaneously before any consumer processes a chunk.
4. **OOM risk under load** — in a server processing many concurrent
   requests, each spawning a pipeline, total in-flight data can exceed
   available memory with no warning or throttling.

### 1.2 Worst-Case Memory Analysis

Per-pipeline in-flight memory = `stages × channel_capacity × chunk_size`:

| Stages | Capacity | Chunk Size | Per-Pipeline | 10 Concurrent | 100 Concurrent |
|--------|----------|-----------|-------------|---------------|----------------|
| 5 | 8 | 64 KB | 2.5 MB | 25 MB | 250 MB |
| 10 | 8 | 64 KB | 5.0 MB | 50 MB | 500 MB |
| 20 | 8 | 64 KB | 10.0 MB | 100 MB | 1,000 MB |
| 50 | 8 | 64 KB | 25.0 MB | 250 MB | 2,500 MB |
| 10 | 8 | 256 KB | 20.0 MB | 200 MB | 2,000 MB |
| 10 | 16 | 64 KB | 10.0 MB | 100 MB | 1,000 MB |

With §2.10 adaptive chunk sizing, chunk sizes can grow up to 1 MB per
stage, making the worst case even larger:

| Stages | Capacity | Chunk Size (adaptive) | Per-Pipeline | 10 Concurrent |
|--------|----------|-----------------------|-------------|---------------|
| 10 | 8 | 1 MB | 80 MB | 800 MB |
| 20 | 8 | 1 MB | 160 MB | 1,600 MB |

These numbers represent only in-flight channel data. Actual process
memory also includes stage task stacks (~8 KB each), intermediate
buffers within stages (e.g., compression dictionaries), and cached
results if `cache_intermediates` is enabled.

### 1.3 Real-World Impact

```
Scenario: Content processing server handling 50 concurrent requests
  Each request: 5-stage pipeline (decompress → decrypt → normalize → hash → compress)
  Channel capacity: 8, chunk size: 64 KB

  In-flight channel data: 50 × 5 × 8 × 64 KB = 125 MB
  Stage task stacks:      50 × 6 × 8 KB       = 2.4 MB
  Compression buffers:    50 × 2 × 256 KB      = 25 MB
  ─────────────────────────────────────────────────────
  Total:                                        ~152 MB

  If a traffic spike doubles concurrency to 100 requests:
  Total:                                        ~304 MB

  If adaptive chunking grows chunks to 256 KB:
  In-flight channel data: 100 × 5 × 8 × 256 KB = 1,000 MB
  Total:                                         ~1,027 MB

  On a 2 GB container, this leaves < 1 GB for the OS, runtime,
  and application logic — risking OOM kills.
```

With memory budget enforcement at 10 MB per pipeline:
- Each pipeline limited to 10 MB of in-flight data
- 100 concurrent pipelines: 100 × 10 MB = 1,000 MB (bounded)
- Excess producers block on semaphore until consumers drain — graceful
  degradation instead of OOM

### 1.4 Comparison

| Dimension | Current (per-channel only) | Budget Enforcement (§2.12) |
|-----------|--------------------------|---------------------------|
| Memory bound | Per-channel: `capacity × chunk_size` | Global: `memory_budget` across all channels |
| Cross-channel coordination | None | Semaphore-based permit system |
| Concurrent pipeline isolation | None (all share process memory) | Per-pipeline budget (configurable) |
| Backpressure scope | Single producer ↔ single consumer | All producers in pipeline |
| OOM protection | None | Producers block when budget exhausted |
| Overhead per chunk | 0 ns | ~40 ns (semaphore acquire + release) |
| Configuration | `channel_capacity` only | `memory_budget` + `channel_capacity` |
| Degradation mode | OOM kill | Throughput reduction (slower, not failing) |
| Observability | None | Budget utilization, wait time, exhaustion events |
| Adaptive chunking interaction | Chunks grow without limit | Chunk growth bounded by available permits |

### 1.5 What Exists vs What's Missing

**Exists today:**

- `PipelineConfig::memory_budget` field — declared, defaults to 0
  (unlimited), but never read during pipeline execution.
- Per-channel backpressure via bounded `mpsc::channel(capacity)` — a
  full channel blocks the producer's `send().await`. This provides
  local flow control between adjacent stages.
- `tokio::sync::Semaphore` — available in the `tokio` dependency
  already used by the pipeline. Provides async-native counting
  semaphore with `acquire()` / `release()` semantics.
- Metrics infrastructure in `metrics.rs` — `lazy_static!` +
  `prometheus` counters, histograms, and gauges already in use.

**Missing:**

1. **MemoryController** — no struct that wraps a semaphore and
   translates byte budgets into permit counts.
2. **Permit-aware channel wrappers** — no `BudgetedSender` /
   `BudgetedReceiver` that acquire/release permits on send/recv.
3. **Pipeline integration** — `StreamPipeline::execute()` does not
   create a memory controller or pass it to stage spawns.
4. **Budget validation** — no check that `memory_budget` is sensible
   relative to pipeline topology (stages × chunk_size).
5. **Observability** — no metrics for budget utilization, wait time,
   or exhaustion events.
6. **Interaction with §2.10** — adaptive chunk sizing can grow chunks
   without considering the global memory budget. A budget-aware resizer
   should consult available permits before growing.

---

## 2. Design

### 2.1 Architecture Overview

A per-pipeline `MemoryController` backed by a `tokio::sync::Semaphore`
limits total in-flight data across all channels. Each chunk in transit
holds one permit; producers block when the budget is exhausted.

```
  Pipeline with MemoryController (memory_budget = 10 MB, chunk_size = 64 KB)
  ─────────────────────────────────────────────────────────────────────────
  permits = memory_budget / chunk_size = 10 MB / 64 KB = 160 permits

  ┌────────────────────────────────────────────────────────────────────┐
  │                    MemoryController                                │
  │                    Semaphore(160 permits)                          │
  │                                                                    │
  │  acquire() ◄──── BudgetedSender.send()                            │
  │  release() ◄──── BudgetedReceiver.recv() after processing         │
  └────────────────────────────────────────────────────────────────────┘
        ▲               ▲               ▲               ▲
        │               │               │               │
  ┌─────┴─────┐   ┌────┴────┐   ┌─────┴─────┐   ┌────┴────┐
  │ Source    │──▶│ Stage 1 │──▶│ Stage 2   │──▶│ Stage 3 │
  │ (producer)│   │         │   │           │   │(consumer)│
  └───────────┘   └─────────┘   └───────────┘   └─────────┘
       ch₁            ch₂            ch₃

  Total in-flight chunks across ch₁ + ch₂ + ch₃ ≤ 160
  Each channel still has its own capacity (8) for local backpressure.
  Global semaphore is the tighter constraint when budget < stages × capacity × chunk_size.
```

When `memory_budget = 0` (default), no `MemoryController` is created
and raw `mpsc` channels are used — zero overhead, identical to current
behavior.

### 2.2 Permit Lifecycle

Each chunk follows a permit lifecycle from production to consumption:

```
  Producer task                          Consumer task
  ──────────────                         ──────────────
  1. Generate chunk (Bytes)
  2. controller.acquire().await ─────┐
     (blocks if no permits)          │
  3. budgeted_tx.send(chunk).await   │   
     (blocks if channel full)        │   
                                     │   4. budgeted_rx.recv().await
                                     │      (receives chunk + permit)
                                     │   5. Process chunk
                                     └── 6. drop(permit)
                                            (releases back to semaphore)
```

Key invariant: **every in-flight chunk holds exactly one permit**.
The permit is acquired before `send()` and released after the consumer
finishes processing (on `drop` of the `OwnedSemaphorePermit`).

For `End` and `Error` sentinel chunks, no permit is acquired — they
carry no data payload and don't contribute to memory pressure.

### 2.3 Interaction with Per-Channel Backpressure

Two independent backpressure layers operate simultaneously:

```
  Layer 1: Per-channel (existing)
  ────────────────────────────────
  mpsc::channel(capacity=8)
  → Producer blocks when THIS channel has 8 chunks buffered
  → Scope: single producer ↔ single consumer
  → Purpose: prevent fast stage from overwhelming slow neighbor

  Layer 2: Global budget (new)
  ────────────────────────────
  Semaphore(permits=160)
  → Producer blocks when ALL channels combined hold 160 chunks
  → Scope: all stages in the pipeline
  → Purpose: bound total pipeline memory
```

| Scenario | Channel full? | Budget exhausted? | Producer behavior |
|----------|:------------:|:-----------------:|-------------------|
| Normal operation | No | No | Sends immediately |
| Local congestion | Yes | No | Blocks on `channel.send()` |
| Global pressure | No | Yes | Blocks on `semaphore.acquire()` |
| Both | Yes | Yes | Blocks on `semaphore.acquire()` first, then `channel.send()` |

The global semaphore is always checked **before** the channel send.
This ensures that a producer never allocates a chunk it can't send.

### 2.4 Budget Calculation

```rust
effective_permits = if memory_budget == 0 {
    None  // unlimited — no semaphore created
} else {
    Some(memory_budget / chunk_size)  // integer division, minimum 1
};
```

| memory_budget | chunk_size | Permits | Max in-flight data |
|--------------|-----------|---------|-------------------|
| 0 | any | unlimited | unbounded |
| 1 MB | 64 KB | 16 | 1 MB |
| 10 MB | 64 KB | 160 | 10 MB |
| 10 MB | 256 KB | 40 | 10 MB |
| 64 KB | 64 KB | 1 | 64 KB |
| 32 KB | 64 KB | 1 (clamped) | 64 KB |

When `memory_budget < chunk_size`, permits are clamped to 1 and a
warning is logged. This ensures the pipeline can always make progress
(at least one chunk can be in flight).

### 2.5 Concurrent Pipeline Isolation

Each `StreamPipeline::execute()` call creates its own `MemoryController`
with its own semaphore. Pipelines do not share permits:

```
  Pipeline A (budget = 10 MB)          Pipeline B (budget = 10 MB)
  ┌──────────────────────────┐         ┌──────────────────────────┐
  │ MemoryController A       │         │ MemoryController B       │
  │ Semaphore(160 permits)   │         │ Semaphore(160 permits)   │
  │                          │         │                          │
  │ Stage₁ → Stage₂ → Stage₃│         │ Stage₁ → Stage₂         │
  └──────────────────────────┘         └──────────────────────────┘

  Total process memory bound: 10 MB + 10 MB = 20 MB (predictable)
```

Per-pipeline isolation is chosen over a shared global semaphore because:
1. **Predictable latency** — pipeline A's throughput is independent of
   pipeline B's behavior.
2. **No priority inversion** — a slow pipeline can't starve a fast one.
3. **Simple reasoning** — each pipeline's budget is self-contained.

A shared global semaphore could be added as a future extension for
system-wide memory limits, layered on top of per-pipeline budgets.

### 2.6 Interaction with Other Sections

| Section | Interaction |
|---------|-------------|
| §2.7 Streaming Materialization | `execute()` creates `MemoryController` and passes to stage spawns |
| §2.10 Adaptive Chunk Sizing | Resizer should consult available permits before growing chunk size; if budget is tight, prefer smaller chunks |
| §2.11 Pipeline Fusion | Fused stages use fewer channels → fewer permits needed → more headroom for remaining stages |
| §2.9 Size-Aware Mode Selection | Batch mode bypasses streaming channels entirely → no permits needed; mode selection can consider budget |

---

## 3. Implementation

### 3.1 MemoryController Struct

New file: `crates/deriva-compute/src/memory.rs`

```rust
use std::sync::Arc;
use tokio::sync::{Semaphore, OwnedSemaphorePermit};

/// Controls total in-flight memory across all channels in a pipeline.
///
/// Each permit represents one chunk's worth of memory (`chunk_size` bytes).
/// Producers acquire a permit before sending; consumers release on drop
/// after processing.
#[derive(Clone)]
pub struct MemoryController {
    semaphore: Arc<Semaphore>,
    total_permits: usize,
}

impl MemoryController {
    /// Create a new controller.
    ///
    /// `budget` = 0 is handled by the caller (no controller created).
    /// `permits` is clamped to at least 1.
    pub fn new(budget: usize, chunk_size: usize) -> Self {
        let permits = (budget / chunk_size).max(1);
        if budget > 0 && budget < chunk_size {
            tracing::warn!(
                budget,
                chunk_size,
                "memory_budget < chunk_size; clamped to 1 permit"
            );
        }
        Self {
            semaphore: Arc::new(Semaphore::new(permits)),
            total_permits: permits,
        }
    }

    /// Acquire one permit (blocks if budget exhausted).
    pub async fn acquire(&self) -> OwnedSemaphorePermit {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore closed unexpectedly")
    }

    /// Try to acquire without blocking. Returns `None` if no permits.
    pub fn try_acquire(&self) -> Option<OwnedSemaphorePermit> {
        self.semaphore.clone().try_acquire_owned().ok()
    }

    /// Number of permits currently available.
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Total permits this controller was created with.
    pub fn total_permits(&self) -> usize {
        self.total_permits
    }
}
```

### 3.2 BudgetedSender / BudgetedReceiver

Wrappers around `mpsc::Sender<StreamChunk>` / `mpsc::Receiver<StreamChunk>`
that integrate permit acquisition and release:

```rust
use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::sync::OwnedSemaphorePermit;
use deriva_core::streaming::StreamChunk;

/// A channel sender that acquires a memory permit before each data send.
pub struct BudgetedSender {
    tx: mpsc::Sender<(StreamChunk, Option<OwnedSemaphorePermit>)>,
    controller: MemoryController,
}

impl BudgetedSender {
    pub fn new(
        tx: mpsc::Sender<(StreamChunk, Option<OwnedSemaphorePermit>)>,
        controller: MemoryController,
    ) -> Self {
        Self { tx, controller }
    }

    /// Send a data chunk. Acquires a permit first (blocks if budget full).
    /// End/Error sentinels are sent without a permit.
    pub async fn send(&self, chunk: StreamChunk) -> Result<(), mpsc::error::SendError<StreamChunk>> {
        let permit = match &chunk {
            StreamChunk::Data(_) => Some(self.controller.acquire().await),
            StreamChunk::End | StreamChunk::Error(_) => None,
        };
        self.tx
            .send((chunk, permit))
            .await
            .map_err(|e| mpsc::error::SendError(e.0 .0))
    }
}

/// A channel receiver that releases the memory permit after processing.
pub struct BudgetedReceiver {
    rx: mpsc::Receiver<(StreamChunk, Option<OwnedSemaphorePermit>)>,
}

impl BudgetedReceiver {
    pub fn new(
        rx: mpsc::Receiver<(StreamChunk, Option<OwnedSemaphorePermit>)>,
    ) -> Self {
        Self { rx }
    }

    /// Receive the next chunk. The permit is released when the returned
    /// `ChunkGuard` is dropped (i.e., after the consumer processes it).
    pub async fn recv(&mut self) -> Option<ChunkGuard> {
        self.rx.recv().await.map(|(chunk, permit)| ChunkGuard { chunk, _permit: permit })
    }
}

/// Holds a chunk and its associated permit. The permit is released on drop.
pub struct ChunkGuard {
    pub chunk: StreamChunk,
    _permit: Option<OwnedSemaphorePermit>,
}
```

The `ChunkGuard` pattern ensures permits are released exactly when the
consumer is done with the chunk — not when it's dequeued from the
channel, but when the guard is dropped after processing. This gives
accurate memory accounting.

### 3.3 Budgeted Channel Constructor

Helper to create a budgeted channel pair:

```rust
/// Create a budgeted channel pair with the given capacity and controller.
pub fn budgeted_channel(
    capacity: usize,
    controller: MemoryController,
) -> (BudgetedSender, BudgetedReceiver) {
    let (tx, rx) = mpsc::channel(capacity);
    (
        BudgetedSender::new(tx, controller),
        BudgetedReceiver::new(rx),
    )
}
```

### 3.4 Pipeline Integration

Changes to `StreamPipeline::execute()` in `pipeline.rs` (~25 lines):

```rust
pub async fn execute(self) -> Result<mpsc::Receiver<StreamChunk>, DerivaError> {
    let start = std::time::Instant::now();
    metrics::STREAM_PIPELINES_TOTAL.inc();

    // --- NEW: create MemoryController if budget > 0 ---
    let controller = if self.config.memory_budget > 0 {
        let mc = MemoryController::new(
            self.config.memory_budget,
            self.config.chunk_size,
        );
        tracing::debug!(
            budget = self.config.memory_budget,
            chunk_size = self.config.chunk_size,
            permits = mc.total_permits(),
            "memory budget enforcement enabled"
        );
        metrics::record_memory_budget(
            self.config.memory_budget,
            mc.total_permits(),
        );
        Some(mc)
    } else {
        None
    };
    // --- END NEW ---

    let mut outputs: Vec<Option<ChannelOutput>> =
        Vec::with_capacity(self.nodes.len());

    for node in self.nodes {
        match node {
            PipelineNode::Source { data, .. }
            | PipelineNode::Cached { data, .. } => {
                // Source chunking: use budgeted sender if controller exists
                let rx = match &controller {
                    Some(mc) => value_to_stream_budgeted(
                        data,
                        self.config.chunk_size,
                        self.config.channel_capacity,
                        mc.clone(),
                    ),
                    None => value_to_stream(
                        data,
                        self.config.chunk_size,
                        self.config.channel_capacity,
                    ).into(),
                };
                outputs.push(Some(rx));
            }

            PipelineNode::StreamingStage { function, params, input_indices, .. } => {
                // Pass controller to stage spawn for budgeted output channel
                let inputs = take_inputs(&mut outputs, &input_indices)?;
                let rx = match &controller {
                    Some(mc) => function.stream_execute_budgeted(
                        inputs, &params, mc.clone(),
                    ).await,
                    None => function.stream_execute(inputs, &params).await.into(),
                };
                outputs.push(Some(rx));
            }

            // BatchStage unchanged — collects full stream, no budgeting needed
            PipelineNode::BatchStage { .. } => { /* existing code */ }
        }
    }

    // ... rest unchanged (output wrapping, metrics) ...
}
```

The key principle: **when `memory_budget == 0`, the code path is
identical to today** — no `MemoryController`, no `BudgetedSender`,
no overhead. The budgeted path only activates when a non-zero budget
is configured.

### 3.5 StreamingComputeFunction Trait Extension

A default method on the trait provides budgeted execution without
requiring changes to all 20 builtin implementations:

```rust
#[async_trait::async_trait]
pub trait StreamingComputeFunction: Send + Sync {
    // ... existing methods ...

    /// Execute with memory budget enforcement.
    ///
    /// Default implementation wraps the output of `stream_execute()` with
    /// a budgeted sender. Stages that create their own output channels
    /// can override this to use budgeted channels internally.
    async fn stream_execute_budgeted(
        &self,
        inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
        controller: MemoryController,
    ) -> BudgetedReceiver {
        let raw_rx = self.stream_execute(inputs, params).await;
        // Bridge: drain raw_rx, re-send through budgeted channel
        let (btx, brx) = budgeted_channel(
            self.channel_capacity(),
            controller,
        );
        tokio::spawn(async move {
            let mut raw_rx = raw_rx;
            while let Some(chunk) = raw_rx.recv().await {
                if btx.send(chunk).await.is_err() {
                    break;
                }
            }
        });
        brx
    }
}
```

This bridge approach means **zero changes to existing builtin
implementations**. Each stage's `stream_execute()` runs as before,
producing raw `mpsc::Receiver<StreamChunk>`. The bridge task drains
it and re-sends through a budgeted channel, acquiring permits on each
data chunk.

The bridge adds one extra Tokio task per stage. For stages that want
to avoid this overhead, they can override `stream_execute_budgeted()`
to use budgeted channels directly in their internal implementation.

### 3.6 PipelineConfig Validation

Validation at pipeline construction warns about problematic configurations:

```rust
impl StreamPipeline {
    pub fn new(config: PipelineConfig) -> Self {
        if config.memory_budget > 0 {
            let permits = config.memory_budget / config.chunk_size;
            if permits == 0 {
                tracing::warn!(
                    budget = config.memory_budget,
                    chunk_size = config.chunk_size,
                    "memory_budget < chunk_size; will clamp to 1 permit"
                );
            }
        }
        Self { nodes: Vec::new(), config }
    }
}
```

No hard errors — the pipeline always proceeds. Validation is advisory
to avoid surprising the caller with panics on misconfiguration.

---

## 4. Data Flow Diagrams

### 4.1 Normal Operation (Within Budget)

3-stage pipeline, budget = 640 KB, chunk_size = 64 KB → 10 permits.
Channel capacity = 4 per channel. Processing 8 chunks (512 KB input).

```
  Time →   t₁      t₂      t₃      t₄      t₅      t₆      t₇      t₈

  Permits: 10      10       9       8       7       8       9      10
           (idle)  (idle)  (src     (src    (src    (stg1   (stg2   (all
                           sends    sends   sends   done,   done,   done)
                           c₁)      c₂,c₃)  c₄)    recv    recv
                                                    frees)  frees)

  Source:  [c₁ c₂ c₃ c₄ c₅ c₆ c₇ c₈]
            │
            ▼ acquire permit, send on ch₁
  ch₁:    [c₁]──▶ Stage 1 processes c₁ ──▶ acquire permit, send on ch₂
  ch₂:            [c₁']──▶ Stage 2 processes c₁' ──▶ acquire permit, send on ch₃
  ch₃:                     [c₁'']──▶ Consumer receives c₁'' ──▶ drop guard (permit freed)

  Steady state: 3 chunks in flight (one per channel), 7 permits free.
  Pipeline never blocks — budget is sufficient for this topology.
```

Permit flow per chunk:
```
  acquire(1) → ch₁ → Stage 1 recv → drop guard → release(1)
                                   → acquire(1) → ch₂ → Stage 2 recv → drop guard → release(1)
                                                                      → acquire(1) → ch₃ → Consumer recv → drop guard → release(1)
```

Each chunk acquires 3 permits total (one per hop) but holds only 1 at
a time. Peak permits held = `min(stages × capacity, total_permits)`.

### 4.2 Budget Exhaustion and Recovery

Same pipeline, but budget = 192 KB → 3 permits. Source has 8 chunks.

```
  Time →   t₁       t₂       t₃       t₄       t₅       t₆

  Permits: 3        2        1        0        0→1      1→0
                                      ▲        ▲
                                      │        │
                                      BLOCKED  consumer drops
                                      (source  guard for c₁''
                                       waits)  → permit freed
                                               → source unblocks

  Source:  [c₁ c₂ c₃ c₄ c₅ c₆ c₇ c₈]
            │   │   │   │
            ▼   ▼   ▼   ✗ blocked at t₄
  ch₁:    c₁  c₂  c₃   ·  ← source cannot send c₄ (0 permits)
            │   │
  ch₂:    c₁' c₂'       ← Stage 1 forwarded c₁, c₂
            │
  ch₃:    c₁''          ← Stage 2 forwarded c₁

  At t₅: Consumer finishes c₁'' → drops ChunkGuard → permit released
          Source acquires permit → sends c₄ → pipeline resumes
```

The pipeline makes progress at the rate of the slowest consumer.
No data is lost, no errors — just throughput reduction. This is the
intended graceful degradation behavior.

### 4.3 End/Error Sentinels (No Permit)

```
  Source sends End marker:
  ─────────────────────────
  chunk = StreamChunk::End
  → match: not Data → permit = None
  → tx.send((End, None)).await
  → consumer receives (End, None)
  → ChunkGuard { chunk: End, _permit: None }
  → drop: no permit to release (no-op)

  Source sends Error:
  ───────────────────
  chunk = StreamChunk::Error(e)
  → match: not Data → permit = None
  → tx.send((Error(e), None)).await
  → consumer receives error, pipeline terminates
  → all in-flight ChunkGuards dropped → all permits released
```

### 4.4 Per-Pipeline Isolation Under Concurrent Load

Two pipelines running concurrently, each with budget = 5 MB:

```
  Pipeline A (budget=5 MB, 80 permits)     Pipeline B (budget=5 MB, 80 permits)
  ┌────────────────────────────────┐       ┌────────────────────────────────┐
  │ Semaphore A: 80 permits        │       │ Semaphore B: 80 permits        │
  │                                │       │                                │
  │ Src → Stg1 → Stg2 → Stg3     │       │ Src → Stg1 → Stg2             │
  │ (4 channels × cap 8 = 32 max) │       │ (3 channels × cap 8 = 24 max) │
  │                                │       │                                │
  │ Peak permits held: 32          │       │ Peak permits held: 24          │
  │ Permits free: 48               │       │ Permits free: 56              │
  └────────────────────────────────┘       └────────────────────────────────┘

  Pipeline A stalls (slow Stage 3):
  → Semaphore A: 0 permits, source blocked
  → Semaphore B: unaffected, still 56 free
  → Pipeline B throughput unchanged
```

### 4.5 Interaction with Adaptive Chunk Sizing (§2.10)

When adaptive chunking grows chunk size from 64 KB to 256 KB, each
chunk consumes 4× more memory but still holds only 1 permit:

```
  Budget = 10 MB, original chunk_size = 64 KB → 160 permits

  Without budget awareness:
    Resizer grows to 256 KB → actual memory per permit = 256 KB
    160 permits × 256 KB = 40 MB (4× over budget!)

  With budget awareness (future §2.10 integration):
    Resizer checks controller.available() before growing
    If available < threshold → hold current size or shrink
    Effective bound maintained at ~10 MB
```

This interaction is a known limitation of the current design. The
permit count is calculated from the original `chunk_size`, not the
actual chunk size after adaptive resizing. Full integration requires
§2.10 to consult the `MemoryController` — deferred to implementation
time.

---

## 5. Test Specification

### 5.1 MemoryController Core (6 tests)

**T1 — Permit count matches budget / chunk_size**
Create `MemoryController::new(1_048_576, 65_536)` (1 MB / 64 KB).
Assert `total_permits() == 16` and `available() == 16`.

**T2 — Budget smaller than chunk_size clamps to 1**
Create `MemoryController::new(32_768, 65_536)` (32 KB / 64 KB).
Assert `total_permits() == 1` and `available() == 1`.
Pipeline must still be able to make progress with a single permit.

**T3 — Acquire decrements, drop increments**
Create controller with 4 permits. Acquire 3 permits into a `Vec`.
Assert `available() == 1`. Drop one permit. Assert `available() == 2`.
Drop remaining two. Assert `available() == 4`.

**T4 — try_acquire returns None when exhausted**
Create controller with 2 permits. Acquire both via `acquire().await`.
Assert `try_acquire().is_none()`. Drop one. Assert `try_acquire().is_some()`.

**T5 — Acquire blocks when exhausted, unblocks on release**
Create controller with 1 permit. Acquire the permit. Spawn a task that
calls `acquire().await` and records the timestamp it unblocks. Sleep
50 ms, then drop the first permit. Assert the spawned task unblocked
within 10 ms of the drop (not before).

**T6 — Large budget produces correct permit count**
Create `MemoryController::new(1_073_741_824, 65_536)` (1 GB / 64 KB).
Assert `total_permits() == 16_384`. Verify no overflow or panic.

### 5.2 BudgetedSender / BudgetedReceiver (6 tests)

**T7 — Data chunk acquires permit, End does not**
Create budgeted channel with controller (4 permits). Send
`StreamChunk::Data(Bytes::from("hello"))`. Assert `available() == 3`.
Send `StreamChunk::End`. Assert `available() == 3` (unchanged).

**T8 — Error chunk does not acquire permit**
Create budgeted channel with controller (4 permits). Send
`StreamChunk::Error(...)`. Assert `available() == 4` (unchanged).

**T9 — ChunkGuard drop releases permit**
Send 3 data chunks through budgeted channel. Assert `available() == 1`.
Recv one chunk (returns `ChunkGuard`). Permit still held (guard alive).
Assert `available() == 1`. Drop the guard. Assert `available() == 2`.

**T10 — Multiple recv then drop in reverse order**
Send 4 data chunks (exhausts 4-permit controller). Recv all 4 into
guards. Assert `available() == 0`. Drop guards in reverse order
(last-received first). After each drop, assert available increments by 1.

**T11 — Sender dropped while receiver has outstanding guards**
Send 2 data chunks, recv both into guards. Drop the sender. Assert
`available() == 2` (guards still hold permits). Drop guards. Assert
`available() == 4`.

**T12 — Receiver dropped with chunks in channel**
Send 3 data chunks. Drop the receiver without recv'ing. The channel
drops its contents, which drops the `OwnedSemaphorePermit` inside each
tuple. Assert `available() == 4` (all permits returned).

### 5.3 Budget Exhaustion Behavior (5 tests)

**T13 — Producer blocks at budget limit, resumes on consumer drain**
3-permit controller, budgeted channel (capacity 8). Spawn producer
sending 10 data chunks. Spawn consumer that sleeps 100 ms between
recv's. Assert all 10 chunks received correctly. Assert producer was
blocked (total wall time > 300 ms due to 3-permit throttling).

**T14 — Two producers sharing one controller**
4-permit controller shared by two budgeted senders (two channels).
Each producer sends 5 chunks. Single consumer drains both channels
round-robin. Assert all 10 chunks received. Assert `available() == 4`
after completion.

**T15 — Budget = 1 permit serializes entire pipeline**
1-permit controller, 3-stage pipeline (Source → Upper → Lower).
Process 1 MB input. Assert output == expected (lowercase of input).
Assert pipeline completes (no deadlock). Measure wall time — should
be ~3× slower than unbounded (each chunk must fully traverse before
next enters).

**T16 — Tight budget with slow consumer**
2-permit controller. Producer generates 64 chunks at full speed.
Consumer sleeps 10 ms per chunk. Assert all 64 chunks received in
order. Assert peak permits held never exceeds 2 (verify via
`controller.available() >= 0` sampled during execution).

**T17 — Budget exhaustion with concurrent senders racing**
3-permit controller shared by 4 concurrent producer tasks, each
sending 10 chunks to separate channels. Single consumer drains all
4 channels. Assert total chunks received == 40. Assert no permit
leak (`available() == 3` after all guards dropped).

### 5.4 Pipeline Integration — Correctness (6 tests)

**T18 — Budgeted pipeline output matches unbounded**
Run 3-stage pipeline (Upper → Xor → Lower) on 2 MB input with
`memory_budget = 0` (unbounded). Run same pipeline with
`memory_budget = 512 KB`. Assert byte-identical output.

**T19 — Budgeted pipeline with single stage**
1-stage pipeline (Identity) with `memory_budget = 128 KB`. Process
1 MB input. Assert output == input. Verify permits are acquired and
released (controller `available() == total_permits()` after completion).

**T20 — Budgeted pipeline with batch stage in the middle**
Pipeline: Source → StreamingUpper → BatchSha256 → StreamingB64Encode.
Budget = 1 MB. BatchSha256 collects the full stream (no budgeting
needed for batch). Assert pipeline completes and output is correct
Base64-encoded SHA-256 digest.

**T21 — Budget with cache_intermediates enabled**
Pipeline: Source → Upper → Lower, `cache_intermediates = true`,
`memory_budget = 256 KB`. Assert pipeline completes. Cached
intermediates are separate from channel budgeting — verify both
caching and budgeting work simultaneously.

**T22 — Budget enforcement with §2.11 fused stages**
Pipeline: Source → Upper → Lower → Xor (all fusible), `enable_fusion
= true`, `memory_budget = 256 KB`. Fusion merges 3 stages into 1
FusedMapStage → only 1 channel instead of 3. Assert output correct.
Assert fewer permits needed (1 channel vs 3).

**T23 — Budget with heterogeneous chunk sizes (§2.10 interaction)**
Pipeline: Source → Compress → Decompress, `memory_budget = 2 MB`.
Compress output chunks are smaller than input (compression ratio).
Assert pipeline completes. Assert permits are tracked per chunk count,
not per byte count (permits based on original chunk_size).

### 5.5 Pipeline Integration — Edge Topologies (5 tests)

**T24 — Diamond DAG with shared budget**
```
  Source → Upper ──┐
                   ├──▶ Concat → Consumer
  Source → Lower ──┘
```
Budget = 1 MB. Both branches share the same `MemoryController`. Assert
output is correct concatenation. Assert no deadlock (both branches can
make progress with shared permits).

**T25 — Deep pipeline (20 stages) with tight budget**
20 × Identity stages, `memory_budget = 256 KB` (4 permits at 64 KB).
Process 512 KB input (8 chunks). Assert output == input. Assert
pipeline completes without deadlock despite only 4 permits for 20
channels. (Works because chunks flow through sequentially — at most
~20 permits needed at peak, but with 4 permits the pipeline serializes.)

**T26 — Single-chunk input with budget = chunk_size**
Budget = 64 KB (1 permit). Input = exactly 64 KB. 3-stage pipeline.
Assert output correct. Assert the single permit is sufficient (chunk
moves through one stage at a time).

**T27 — Empty input with budget enforcement**
Budget = 128 KB. Input = empty `Bytes`. Pipeline: Source → Upper.
Assert output is empty. Assert only `End` sentinel flows (no permits
acquired for data). Assert `available() == total_permits()` throughout.

**T28 — Fan-out topology with budget**
```
  Source → Upper → Consumer A
       └→ Lower → Consumer B
```
Budget = 512 KB. Source fans out to two branches. Assert both consumers
receive correct output. Assert total permits across both branches
bounded by budget.

### 5.6 Error Handling & Cancellation (5 tests)

**T29 — Upstream error releases all permits**
3-stage pipeline. Stage 2 injects `StreamChunk::Error` after 5 chunks.
Budget = 256 KB. Assert error propagates to consumer. Assert all
permits released after pipeline terminates (`available() == total_permits()`).

**T30 — Consumer dropped mid-stream releases permits**
Producer sends 100 chunks through budgeted channel. Consumer recvs 10,
then drops the receiver. Assert producer's send eventually fails
(channel closed). Assert all permits released (guards dropped when
channel contents dropped + consumer guards dropped).

**T31 — Panic in consumer task releases permits**
Spawn consumer that panics after processing 3 chunks. Assert permits
held by the 3 `ChunkGuard`s are released when the task unwinds (Rust
drop semantics). Assert `available()` returns to `total_permits()`.

**T32 — Timeout on budget acquisition**
Create controller with 1 permit, acquire it. Spawn task that uses
`tokio::time::timeout(Duration::from_millis(100), controller.acquire())`.
Assert timeout fires (returns `Err`). Drop original permit. Retry
acquire without timeout — assert succeeds immediately.

**T33 — Error in first stage with budget**
Pipeline: Source → ErrorStage → Upper. ErrorStage emits `Error` on
first chunk. Budget = 128 KB. Assert error reaches consumer. Assert
no permits leaked. Assert pipeline terminates cleanly.

### 5.7 Concurrency & Stress (5 tests)

**T34 — 20 concurrent pipelines with per-pipeline budgets**
Spawn 20 pipelines, each with 3 stages and `memory_budget = 512 KB`.
Each processes 1 MB input. Assert all 20 complete with correct output.
Assert no cross-pipeline interference (each has independent semaphore).

**T35 — 100 concurrent pipelines with 1-permit budget**
Spawn 100 pipelines, each with 2 stages and `memory_budget = 64 KB`
(1 permit). Each processes 256 KB input. Assert all complete (no
deadlock). Measure total wall time — should be ~4× slower than
unbounded due to serialization.

**T36 — Rapid pipeline creation and destruction**
Loop 500 iterations: create pipeline with budget, process 64 KB,
collect output, drop pipeline. Assert no permit leak across iterations
(semaphore is per-pipeline, dropped with pipeline). Assert no memory
growth (measure RSS at iteration 1 and 500, delta < 1 MB).

**T37 — Mixed budgeted and unbounded pipelines**
Spawn 10 pipelines with `memory_budget = 256 KB` and 10 with
`memory_budget = 0` (unbounded) concurrently. Assert all 20 complete
correctly. Assert budgeted pipelines don't interfere with unbounded.

**T38 — High contention: many producers, few permits**
8-permit controller shared by 8 producer tasks, each sending 100
chunks. Single consumer drains all 8 channels. Assert all 800 chunks
received. Assert peak permits held ≤ 8 at all times. Assert no
starvation (each producer sends all 100 chunks eventually).

### 5.8 Benchmarks & Performance (4 tests)

**T39 — Permit overhead ceiling: budgeted ≤ 1.05× unbounded**
Run 3-stage Identity pipeline on 8 MB with `memory_budget = 0` and
`memory_budget = 8 MB` (generous budget, no blocking). Measure
throughput. Assert budgeted throughput ≥ 95% of unbounded. The ~40 ns
semaphore overhead per chunk should be negligible vs ~100 ns channel
overhead.

**T40 — Tight budget throughput: graceful degradation**
Run 5-stage Identity pipeline on 8 MB with `memory_budget = 128 KB`
(2 permits). Measure throughput. Assert pipeline completes (no
deadlock). Assert throughput > 0 (pipeline makes progress). Compare
to unbounded — expect 3–10× slower due to serialization, but no
failure.

**T41 — Permit acquisition latency under contention**
4-permit controller. 8 concurrent tasks each acquiring and holding
permits for 1 ms, then releasing. Measure p50 and p99 acquisition
latency over 1000 iterations. Assert p50 < 5 ms, p99 < 50 ms.

**T42 — No overhead when budget = 0**
Run 3-stage Identity pipeline on 8 MB with `memory_budget = 0`.
Measure throughput. Compare to baseline (same pipeline, same code
path but before §2.12 changes). Assert within 1% — the budget=0
path must have zero overhead (no semaphore created, no permit checks).

### Test Summary

| # | Group | Tests | Focus |
|---|-------|-------|-------|
| 5.1 | MemoryController core | T1–T6 | Permit math, acquire/release, blocking, edge sizes |
| 5.2 | BudgetedSender/Receiver | T7–T12 | Permit lifecycle per chunk type, guard semantics, cleanup |
| 5.3 | Budget exhaustion | T13–T17 | Blocking, serialization, shared controllers, racing producers |
| 5.4 | Pipeline correctness | T18–T23 | Output equivalence, batch stages, fusion, adaptive chunking |
| 5.5 | Edge topologies | T24–T28 | Diamond DAG, deep pipeline, single-chunk, empty, fan-out |
| 5.6 | Error & cancellation | T29–T33 | Error propagation, dropped consumers, panic, timeout |
| 5.7 | Concurrency & stress | T34–T38 | 20–100 concurrent pipelines, rapid create/destroy, contention |
| 5.8 | Benchmarks & performance | T39–T42 | Overhead ceiling, degradation, latency, zero-cost budget=0 |
| | **Total** | **42** | |

---

## 6. Edge Cases & Error Handling

| # | Scenario | Expected Behavior | Test |
|---|----------|-------------------|------|
| 1 | `memory_budget = 0` (default) | No `MemoryController` created; raw channels used; zero overhead | T42 |
| 2 | `memory_budget < chunk_size` | Permits clamped to 1; warning logged; pipeline still works | T2, T15 |
| 3 | `memory_budget = chunk_size` (exactly 1 permit) | Pipeline serializes: one chunk in flight at a time; no deadlock | T26 |
| 4 | `memory_budget` not divisible by `chunk_size` | Integer division truncates; e.g., 100 KB / 64 KB = 1 permit | T2 |
| 5 | Empty input (`Bytes::new()`) | Only `End` sentinel flows; no permits acquired; budget untouched | T27 |
| 6 | Single-chunk input (exactly `chunk_size` bytes) | One permit acquired, released after consumer processes; clean | T26 |
| 7 | Very large input (1 GB) with tight budget (256 KB) | Pipeline completes slowly; producer blocks frequently; no OOM | T40 |
| 8 | Error in first stage | `StreamChunk::Error` propagates; no data permits acquired; clean exit | T33 |
| 9 | Error in middle stage after N chunks | N permits held by downstream guards; all released on error propagation + guard drops | T29 |
| 10 | Consumer dropped mid-stream | Channel closes; sender fails; in-channel chunks dropped → permits freed; guards dropped → permits freed | T30 |
| 11 | Consumer panics mid-stream | Tokio task unwinds; `ChunkGuard` drop runs → permits released | T31 |
| 12 | Producer panics after acquiring permit | `OwnedSemaphorePermit` dropped on unwind → permit released | — |
| 13 | Pipeline cancelled via `tokio::select!` | All tasks cancelled; all guards and channel contents dropped; permits freed | — |
| 14 | Diamond DAG: two branches share controller | Both branches compete for permits; no deadlock if budget ≥ 2 | T24 |
| 15 | Fan-out: one source feeds two consumers | Each branch acquires permits independently from shared pool | T28 |
| 16 | Deep pipeline (20+ stages) with few permits | Pipeline serializes heavily; chunks move one-at-a-time; completes eventually | T25 |
| 17 | Concurrent pipelines (100+) each with budget | Each has independent semaphore; no cross-pipeline interference | T35 |
| 18 | Rapid pipeline create/destroy (500 iterations) | No permit leak; semaphore dropped with pipeline; no memory growth | T36 |
| 19 | Batch stage in middle of budgeted pipeline | Batch stage collects full stream (no budgeting); downstream resumes with budgeted channels | T20 |
| 20 | Fused stages (§2.11) with budget | Fewer channels → fewer permits needed; budget more effective | T22 |
| 21 | Adaptive chunking (§2.10) grows chunks beyond budget assumption | Permits track chunk count, not bytes; actual memory may exceed budget; known limitation | T23 |
| 22 | `channel_capacity` > permits | Per-channel capacity is never fully used; global semaphore is tighter constraint | T15 |
| 23 | `channel_capacity = 1` with budget | Minimal buffering; each channel holds at most 1 chunk; budget still enforced on top | — |
| 24 | All stages are batch (no streaming) | No channels created; `MemoryController` created but unused; no overhead | — |
| 25 | Mixed streaming + batch stages with budget | Only streaming channels use budgeted wrappers; batch stages bypass | T20 |

---

## 7. Performance Analysis

### 7.1 Overhead of Semaphore Operations

`tokio::sync::Semaphore` is implemented as an atomic counter with a
wait list. Uncontended operations are lock-free:

| Operation | Cost (uncontended) | Cost (contended) | Notes |
|-----------|-------------------|------------------|-------|
| `acquire_owned()` | ~20 ns | ~1–50 μs | Contended = all permits held; blocks until release |
| `drop(OwnedSemaphorePermit)` | ~15 ns | ~20 ns | Wakes one waiter if any |
| **Per-chunk total** | **~35 ns** | **~35 ns + wait** | acquire + release |

Compared to existing per-chunk costs:

| Component | Cost per chunk | Semaphore overhead % |
|-----------|---------------|---------------------|
| Channel send + recv | ~100 ns | 35% of channel cost |
| Identity stage compute | ~0 ns | ∞ (but absolute cost tiny) |
| Uppercase compute (64 KB) | ~2 μs | 1.7% |
| Compress compute (64 KB) | ~200 μs | 0.02% |
| SHA-256 compute (64 KB) | ~15 μs | 0.23% |

The ~35 ns overhead is negligible for all practical workloads. Even
for the cheapest stages (Identity), the absolute cost is small compared
to channel overhead which already dominates.

### 7.2 Throughput Impact Under Various Budget Levels

For a 3-stage Identity pipeline processing 8 MB at 64 KB chunks (128 chunks):

| Budget | Permits | Behavior | Expected Throughput | vs Unbounded |
|--------|---------|----------|--------------------:|-------------:|
| 0 (unlimited) | ∞ | No semaphore | 667 MB/s | baseline |
| 8 MB | 128 | Never blocks (permits > chunks) | ~660 MB/s | ~99% |
| 1 MB | 16 | Rarely blocks | ~650 MB/s | ~97% |
| 256 KB | 4 | Frequent blocking | ~400 MB/s | ~60% |
| 128 KB | 2 | Heavy serialization | ~250 MB/s | ~37% |
| 64 KB | 1 | Full serialization | ~150 MB/s | ~22% |

Key insight: throughput degrades **gracefully and monotonically** with
tighter budgets. There is no cliff — just progressive slowdown as
producers spend more time waiting for permits.

### 7.3 Memory Savings

| Scenario | Without Budget | With Budget (10 MB/pipeline) | Savings |
|----------|---------------|------------------------------|---------|
| 1 pipeline, 5 stages | 2.5 MB | 2.5 MB (under budget) | 0% |
| 10 pipelines, 5 stages | 25 MB | 25 MB (under budget) | 0% |
| 10 pipelines, 20 stages | 100 MB | 100 MB (at budget) | 0% |
| 50 pipelines, 20 stages | 500 MB | 500 MB (at budget) | 0% |
| 100 pipelines, 20 stages | 1,000 MB | 1,000 MB (at budget) | 0% |
| 100 pipelines, 20 stages, adaptive 256 KB | 4,000 MB | 1,000 MB | 75% |
| 100 pipelines, 50 stages | 2,500 MB | 1,000 MB | 60% |

The budget provides value when actual in-flight data would exceed the
budget — i.e., deep pipelines, many concurrent pipelines, or adaptive
chunking growing chunk sizes. For shallow pipelines within budget,
there is no memory savings (and no overhead either, since permits
never block).

### 7.4 Contention Analysis

When the budget is tight, multiple stages compete for permits. The
semaphore's FIFO wait queue ensures fairness:

```
  3-permit budget, 5-stage pipeline, all stages ready to send:

  t₁: Stage 1, 2, 3 each acquire 1 permit (3/3 used)
  t₂: Stage 4 calls acquire() → blocks (FIFO position 1)
  t₃: Stage 5 calls acquire() → blocks (FIFO position 2)
  t₄: Consumer processes Stage 3's chunk → permit released
      → Stage 4 unblocks (FIFO head) → acquires permit
  t₅: Consumer processes next chunk → permit released
      → Stage 5 unblocks → acquires permit
```

No starvation: each stage eventually gets a permit. But throughput
is limited to the consumer's processing rate — the pipeline becomes
consumer-bound rather than producer-bound.

### 7.5 Benchmark Study Plan

Location: `deriva-compute/benches/memory_budget.rs`

Five benchmark groups measuring overhead, degradation, contention,
and scaling.

#### Benchmark 1: Semaphore Overhead — Budgeted vs Unbounded

Measures the per-chunk cost of semaphore acquire/release when the
budget is generous (never blocks).

```rust
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn bench_budgeted_vs_unbounded(c: &mut Criterion) {
    let mut group = c.benchmark_group("budgeted_vs_unbounded");
    let data = Bytes::from(vec![b'a'; 8 * 1024 * 1024]); // 8 MB
    group.throughput(Throughput::Bytes(8 * 1024 * 1024));

    let stages = [1, 3, 5, 10];

    for &n in &stages {
        // (a) Unbounded (budget = 0)
        group.bench_with_input(
            BenchmarkId::new("unbounded", n),
            &n,
            |b, &n| {
                b.to_async(&rt).iter(|| async {
                    run_n_stage_identity(data.clone(), n, PipelineConfig {
                        memory_budget: 0,
                        ..Default::default()
                    }).await
                });
            },
        );

        // (b) Budgeted (generous: 8 MB = never blocks)
        group.bench_with_input(
            BenchmarkId::new("budgeted_generous", n),
            &n,
            |b, &n| {
                b.to_async(&rt).iter(|| async {
                    run_n_stage_identity(data.clone(), n, PipelineConfig {
                        memory_budget: 8 * 1024 * 1024,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Budgeted within 5% of unbounded. The ~35 ns semaphore
overhead per chunk is small relative to ~100 ns channel cost.

#### Benchmark 2: Throughput vs Budget Level

Measures throughput degradation as budget tightens.

```rust
fn bench_throughput_vs_budget(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_vs_budget");
    let data = Bytes::from(vec![b'a'; 8 * 1024 * 1024]); // 8 MB
    group.throughput(Throughput::Bytes(8 * 1024 * 1024));

    // 3-stage identity pipeline at varying budgets
    let budgets_kb = [64, 128, 256, 512, 1024, 4096, 8192];

    for &budget_kb in &budgets_kb {
        group.bench_with_input(
            BenchmarkId::new("3_stage_identity", budget_kb),
            &budget_kb,
            |b, &budget_kb| {
                b.to_async(&rt).iter(|| async {
                    run_n_stage_identity(data.clone(), 3, PipelineConfig {
                        memory_budget: budget_kb * 1024,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Monotonic throughput increase with budget. 64 KB (1
permit) is ~5× slower than 8 MB (128 permits). 1 MB (16 permits)
is within 5% of unbounded.

#### Benchmark 3: Concurrent Pipeline Scaling

Measures total throughput with many concurrent budgeted pipelines.

```rust
fn bench_concurrent_pipelines(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_pipelines");
    let data = Bytes::from(vec![b'a'; 1024 * 1024]); // 1 MB each
    let concurrencies = [1, 5, 10, 25, 50];

    for &n in &concurrencies {
        group.throughput(Throughput::Bytes((n * 1024 * 1024) as u64));

        group.bench_with_input(
            BenchmarkId::new("budgeted_512kb", n),
            &n,
            |b, &n| {
                b.to_async(&rt).iter(|| async {
                    let futs: Vec<_> = (0..n).map(|_| {
                        run_n_stage_identity(data.clone(), 3, PipelineConfig {
                            memory_budget: 512 * 1024,
                            ..Default::default()
                        })
                    }).collect();
                    futures::future::join_all(futs).await
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("unbounded", n),
            &n,
            |b, &n| {
                b.to_async(&rt).iter(|| async {
                    let futs: Vec<_> = (0..n).map(|_| {
                        run_n_stage_identity(data.clone(), 3, PipelineConfig {
                            memory_budget: 0,
                            ..Default::default()
                        })
                    }).collect();
                    futures::future::join_all(futs).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** At low concurrency (1–5), budgeted and unbounded are
similar. At high concurrency (25–50), budgeted pipelines have lower
total memory and potentially better cache behavior, narrowing the
throughput gap.

#### Benchmark 4: Permit Acquisition Latency Under Contention

Measures raw semaphore performance under varying contention levels.

```rust
fn bench_permit_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("permit_contention");
    let contenders = [1, 2, 4, 8, 16];

    for &n in &contenders {
        group.bench_with_input(
            BenchmarkId::new("acquire_release", n),
            &n,
            |b, &n| {
                let controller = MemoryController::new(4 * 65536, 65536); // 4 permits
                b.to_async(&rt).iter(|| async {
                    let futs: Vec<_> = (0..n).map(|_| {
                        let c = controller.clone();
                        async move {
                            for _ in 0..100 {
                                let permit = c.acquire().await;
                                tokio::task::yield_now().await;
                                drop(permit);
                            }
                        }
                    }).collect();
                    futures::future::join_all(futs).await
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** 1 contender: ~20 ns/acquire. 4 contenders (= permits):
~50 ns/acquire. 16 contenders (4× oversubscribed): ~5 μs/acquire
(mostly wait time, not CPU cost).

#### Benchmark 5: Expensive Pipeline — Budget Overhead Negligible

Verifies that budget enforcement adds no measurable overhead on
compute-heavy pipelines.

```rust
fn bench_expensive_pipeline_budget(c: &mut Criterion) {
    let mut group = c.benchmark_group("expensive_pipeline_budget");
    let sizes = [1024 * 1024, 4 * 1024 * 1024, 16 * 1024 * 1024];

    for &size in &sizes {
        let data = Bytes::from(vec![0u8; size]);
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("unbounded_compress_decompress", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_compress_decompress(data.clone(), PipelineConfig {
                        memory_budget: 0,
                        ..Default::default()
                    }).await
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("budgeted_compress_decompress", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_compress_decompress(data.clone(), PipelineConfig {
                        memory_budget: 2 * 1024 * 1024,
                        ..Default::default()
                    }).await
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_budgeted_vs_unbounded,
    bench_throughput_vs_budget,
    bench_concurrent_pipelines,
    bench_permit_contention,
    bench_expensive_pipeline_budget,
);
criterion_main!(benches);
```

**Expected:** Within 1%. Compress at ~200 μs/chunk dwarfs the ~35 ns
semaphore overhead. Verifies budget enforcement doesn't regress
compute-heavy workloads.

#### Benchmark Summary

| # | Group | Parameters | Measures | Expected Result |
|---|-------|-----------|----------|-----------------|
| 1 | Budgeted vs unbounded | 4 depths × 2 modes | Per-chunk semaphore overhead | Budgeted ≤ 1.05× unbounded |
| 2 | Throughput vs budget | 7 budget levels | Degradation curve | Monotonic; 1 MB within 5% of unbounded |
| 3 | Concurrent pipelines | 5 concurrencies × 2 modes | Scaling under memory pressure | Budgeted competitive at high concurrency |
| 4 | Permit contention | 5 contender counts | Raw semaphore latency | ~20 ns uncontended, ~5 μs at 4× oversubscription |
| 5 | Expensive pipeline | 3 sizes × 2 modes | Overhead on compute-heavy stages | Within 1% (no regression) |

---

## 8. Files Changed

| File | Change | Lines |
|------|--------|-------|
| `deriva-compute/src/memory.rs` | **NEW** — `MemoryController`, `BudgetedSender`, `BudgetedReceiver`, `ChunkGuard`, `budgeted_channel()` | ~120 |
| `deriva-compute/src/pipeline.rs` | Create `MemoryController` in `execute()` when `memory_budget > 0`; pass to stage spawns; use budgeted channels | ~30 |
| `deriva-compute/src/streaming.rs` | Add `stream_execute_budgeted()` default method to `StreamingComputeFunction` trait | ~25 |
| `deriva-compute/src/lib.rs` | Add `mod memory;` | ~1 |
| `deriva-compute/src/metrics.rs` | Add `MEMORY_BUDGET_BYTES` gauge, `MEMORY_PERMITS_AVAILABLE` gauge, `MEMORY_BUDGET_WAITS` counter, `MEMORY_BUDGET_WAIT_SECONDS` histogram, `record_memory_budget()` helper | ~30 |
| `deriva-compute/tests/memory_budget.rs` | **NEW** — 42 tests: controller, sender/receiver, exhaustion, pipeline correctness, topologies, errors, concurrency, benchmarks | ~900 |
| `deriva-compute/benches/memory_budget.rs` | **NEW** — 5 benchmark groups: overhead, degradation curve, concurrent scaling, contention, expensive pipeline | ~200 |

---

## 9. Dependency Changes

No new crate dependencies. All functionality built on existing:

| Dependency | Already in | Used for |
|-----------|-----------|----------|
| `tokio::sync::Semaphore` | `tokio` (in `deriva-compute`) | `MemoryController` backing store |
| `tokio::sync::OwnedSemaphorePermit` | `tokio` | Permit held by `ChunkGuard`, released on drop |
| `tokio::sync::mpsc` | `tokio` | Inner channel inside `BudgetedSender`/`BudgetedReceiver` |
| `bytes::Bytes` | `deriva-core` | Chunk data type (existing) |
| `tracing` | `deriva-compute` | Warning/debug logs for budget clamping and enforcement |
| `prometheus` | `deriva-compute` | Metrics gauges and counters (existing pattern) |
| `criterion` | `deriva-compute` (dev) | Benchmark framework (existing) |
| `futures` | `deriva-compute` (dev) | `join_all` in concurrent benchmarks (existing) |

---

## 10. Design Rationale

### 10.1 Why Semaphore Instead of Custom Allocator?

| Approach | Pros | Cons |
|----------|------|------|
| `tokio::sync::Semaphore` | Async-native; well-tested; FIFO fairness; ~20 ns uncontended; zero unsafe code | Tracks chunk count, not exact bytes; permit granularity = chunk_size |
| Custom allocator (`GlobalAlloc` wrapper) | Tracks exact bytes; catches all allocations | Complex; error-prone; not async-aware; significant overhead on every allocation; requires `unsafe` |
| `jemalloc` arena limits | OS-level enforcement; hard limit | Not async-aware; kills process on OOM instead of backpressure; platform-specific |
| Manual reference counting | No external dependency | Reimplements semaphore poorly; no async blocking; error-prone |

The semaphore approach is chosen because:
1. It's already available in `tokio` (no new dependency).
2. It provides async-native blocking — producers `await` permits
   instead of busy-spinning or panicking.
3. FIFO wake ordering prevents starvation.
4. The ~20 ns uncontended cost is negligible.
5. The chunk-granularity approximation is acceptable — exact byte
   tracking would require wrapping every `Bytes` allocation, which
   is far more invasive.

### 10.2 Why Per-Pipeline Instead of Global?

| Approach | Pros | Cons |
|----------|------|------|
| Per-pipeline semaphore | Predictable; isolated; no cross-pipeline interference; simple reasoning | Total process memory = sum of all pipeline budgets (may exceed physical RAM) |
| Global shared semaphore | Hard system-wide limit; prevents OOM regardless of pipeline count | Unpredictable latency; priority inversion; slow pipeline starves fast ones; complex fairness |
| Hierarchical (global + per-pipeline) | Best of both; system-wide safety + per-pipeline predictability | Complex; two semaphore acquisitions per chunk; harder to reason about deadlocks |

Per-pipeline is chosen for simplicity and predictability. The caller
controls total memory by choosing appropriate per-pipeline budgets
and limiting concurrency. A global limit can be layered on top as a
future extension without changing the per-pipeline design.

### 10.3 Why Advisory Instead of Hard Limit?

The semaphore tracks permits (chunk count), not actual bytes. Several
factors cause actual memory to differ from the budget:

| Factor | Direction | Magnitude |
|--------|-----------|-----------|
| Chunk being processed by stage task (not in channel) | Over budget | 1 × chunk_size per stage |
| Compression output smaller than input | Under budget | Variable (2×–10× smaller) |
| Adaptive chunking grows chunks beyond original chunk_size | Over budget | Up to 16× if chunks grow to 1 MB |
| Stage-internal buffers (compression dictionaries, hash state) | Over budget | ~256 KB per compress stage |
| `cache_intermediates` storing results | Over budget | Unbounded (full output per cached stage) |

A hard limit would require tracking every allocation, which means
wrapping `Bytes::from()`, `Vec::with_capacity()`, and every internal
buffer — far too invasive. The advisory approach bounds the dominant
memory consumer (in-flight channel data) while accepting that actual
memory may be 10–30% above the budget due to processing overhead.

### 10.4 Why ChunkGuard Instead of Release-on-Recv?

Two options for when to release the permit:

| Approach | Release point | Pros | Cons |
|----------|--------------|------|------|
| Release on `recv()` | When chunk is dequeued from channel | Simple; fewer types | Undercounts: chunk is still in memory while being processed |
| Release on `drop(ChunkGuard)` | When consumer finishes processing | Accurate: permit held while chunk is alive | Extra wrapper type; consumer must handle `ChunkGuard` |

`ChunkGuard` is chosen because it gives more accurate memory accounting.
If we released on `recv()`, a slow consumer could dequeue many chunks
(releasing permits) while still holding them in memory — defeating the
budget. With `ChunkGuard`, the permit is held until the consumer is
truly done with the data.

---

## 11. Observability Integration

Four new metrics:

```rust
lazy_static! {
    /// Current memory budget configured for the most recent pipeline.
    static ref MEMORY_BUDGET_BYTES: Gauge = register_gauge!(
        "deriva_memory_budget_bytes",
        "Configured memory budget in bytes (0 = unlimited)"
    ).unwrap();

    /// Currently available permits across all active pipelines.
    /// Sampled at pipeline creation; not continuously updated.
    static ref MEMORY_PERMITS_AVAILABLE: Gauge = register_gauge!(
        "deriva_memory_permits_available",
        "Available memory permits at pipeline creation"
    ).unwrap();

    /// Total number of times a producer blocked waiting for a permit.
    static ref MEMORY_BUDGET_WAITS: IntCounter = register_int_counter!(
        "deriva_memory_budget_waits_total",
        "Number of times a producer blocked on memory budget"
    ).unwrap();

    /// Time spent waiting for memory permits.
    static ref MEMORY_BUDGET_WAIT_SECONDS: Histogram = register_histogram!(
        "deriva_memory_budget_wait_seconds",
        "Time spent waiting for memory budget permits",
        vec![0.0001, 0.001, 0.01, 0.05, 0.1, 0.5, 1.0]
    ).unwrap();
}

pub(crate) fn record_memory_budget(budget: usize, permits: usize) {
    MEMORY_BUDGET_BYTES.set(budget as f64);
    MEMORY_PERMITS_AVAILABLE.set(permits as f64);
}

pub(crate) fn record_budget_wait(duration_secs: f64) {
    MEMORY_BUDGET_WAITS.inc();
    MEMORY_BUDGET_WAIT_SECONDS.observe(duration_secs);
}
```

Wait tracking in `BudgetedSender::send()`:

```rust
pub async fn send(&self, chunk: StreamChunk) -> Result<(), ...> {
    let permit = match &chunk {
        StreamChunk::Data(_) => {
            let start = std::time::Instant::now();
            let p = self.controller.acquire().await;
            let elapsed = start.elapsed();
            if elapsed > Duration::from_micros(100) {
                metrics::record_budget_wait(elapsed.as_secs_f64());
            }
            Some(p)
        }
        _ => None,
    };
    // ... send ...
}
```

Dashboard queries:

```promql
# Budget utilization (lower = more headroom)
1 - (deriva_memory_permits_available / (deriva_memory_budget_bytes / 65536))

# Budget wait rate (should be near 0 for well-sized budgets)
rate(deriva_memory_budget_waits_total[5m])

# p99 wait latency (alert if > 100ms)
histogram_quantile(0.99, rate(deriva_memory_budget_wait_seconds_bucket[5m]))

# Pipelines running with budget enforcement
deriva_memory_budget_bytes > 0
```

---

## 12. Checklist

- [ ] Create `deriva-compute/src/memory.rs`
- [ ] Implement `MemoryController::new()` with budget/chunk_size → permits (clamp to 1)
- [ ] Implement `MemoryController::acquire()`, `try_acquire()`, `available()`, `total_permits()`
- [ ] Implement `BudgetedSender` with permit acquisition on `Data` chunks
- [ ] Implement `BudgetedReceiver` returning `ChunkGuard`
- [ ] Implement `ChunkGuard` with permit release on `drop`
- [ ] Implement `budgeted_channel()` constructor
- [ ] Add `stream_execute_budgeted()` default method to `StreamingComputeFunction` trait
- [ ] Add `mod memory;` to `lib.rs`
- [ ] Modify `StreamPipeline::execute()` to create `MemoryController` when `memory_budget > 0`
- [ ] Wire budgeted channels into `Source`/`Cached` node handling
- [ ] Wire budgeted channels into `StreamingStage` node handling
- [ ] Add budget validation warning in `StreamPipeline::new()`
- [ ] Add `MEMORY_BUDGET_BYTES` gauge to `metrics.rs`
- [ ] Add `MEMORY_PERMITS_AVAILABLE` gauge to `metrics.rs`
- [ ] Add `MEMORY_BUDGET_WAITS` counter to `metrics.rs`
- [ ] Add `MEMORY_BUDGET_WAIT_SECONDS` histogram to `metrics.rs`
- [ ] Add `record_memory_budget()` and `record_budget_wait()` helpers
- [ ] Add wait tracking in `BudgetedSender::send()` (log if > 100 μs)
- [ ] Unit tests: T1–T6 (MemoryController core)
- [ ] Unit tests: T7–T12 (BudgetedSender/Receiver)
- [ ] Unit tests: T13–T17 (budget exhaustion behavior)
- [ ] Integration tests: T18–T23 (pipeline correctness)
- [ ] Integration tests: T24–T28 (edge topologies)
- [ ] Error tests: T29–T33 (error handling & cancellation)
- [ ] Concurrency tests: T34–T38 (stress & scaling)
- [ ] Benchmark tests: T39–T42 (performance validation)
- [ ] Benchmark: budgeted vs unbounded overhead (4 depths × 2 modes)
- [ ] Benchmark: throughput vs budget level (7 budgets)
- [ ] Benchmark: concurrent pipeline scaling (5 concurrencies × 2 modes)
- [ ] Benchmark: permit contention (5 contender counts)
- [ ] Benchmark: expensive pipeline overhead (3 sizes × 2 modes)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing §2.7–§2.11 tests still pass
- [ ] Commit and push
