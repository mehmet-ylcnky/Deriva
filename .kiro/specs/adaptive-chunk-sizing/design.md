# Design Document: Adaptive Chunk Sizing

## Overview

Adaptive chunk sizing (§2.10) replaces the fixed 64KB chunk size in the streaming pipeline with per-stage dynamic chunk sizing driven by throughput feedback. Each `AdaptiveResizer` node, inserted between adjacent streaming stages, measures the producer/consumer throughput ratio using exponential moving averages (EMA) and adjusts chunk boundaries using power-of-2 scaling with hysteresis to prevent oscillation.

The design introduces three core components:
1. **ThroughputProbe** — lightweight EMA-based throughput measurement (~64 bytes per stage)
2. **Resize Decision Function** — pure function mapping throughput ratio to chunk size adjustment
3. **AdaptiveResizer** — buffering node that re-chunks data at a dynamically-adjusted target size

The system is opt-in via `PipelineConfig::adaptive_chunking` (default: false), ensuring full backward compatibility. When enabled, the pipeline builder automatically inserts `AdaptiveResizer` nodes between adjacent streaming stages, using each downstream function's `preferred_chunk_size()` as the initial target.

**Key design goals:**
- 7–12% throughput improvement for heterogeneous pipelines
- Convergence within 4–8 chunk cycles (log2(max/min) doublings)
- Byte-level data integrity preservation (output identical with/without adaptive chunking)
- Negligible overhead (~64 bytes memory, ~36 ns timing per chunk)

## Architecture

```mermaid
graph TD
    subgraph "StreamPipeline (adaptive_chunking = true)"
        S[Source Node] -->|"64KB chunks"| AR1[AdaptiveResizer 1]
        AR1 -->|"var-size chunks"| F1[StreamingStage: Compress]
        F1 -->|"var-size chunks"| AR2[AdaptiveResizer 2]
        AR2 -->|"var-size chunks"| F2[StreamingStage: SHA-256]
        F2 -->|"var-size chunks"| C[Collect]
    end

    subgraph "AdaptiveResizer Internal"
        TP[ThroughputProbe] --> RD[Resize Decision]
        RD --> BUF[Buffer + Re-chunk]
        BUF --> TP
    end
```

**Insertion rules:**
- Only between adjacent `StreamingStage → StreamingStage` edges
- NOT after Source, Cached, or BatchStage nodes (they handle their own chunking)
- Only when `PipelineConfig::adaptive_chunking == true`

**Feedback loop (per chunk cycle):**
1. Receive chunk from producer → record `on_recv(bytes)` timestamp
2. Buffer and re-chunk to `target_chunk_size`
3. Send re-chunked data to consumer → record `on_send(bytes)` timestamp
4. Compute `ratio = recv_ema_bps / send_ema_bps`
5. Apply resize decision: grow (ratio > 1.5), shrink (ratio < 0.7), or hold

## Components and Interfaces

### ThroughputProbe

```rust
pub(crate) struct ThroughputProbe {
    recv_ema_bps: f64,       // EMA of producer throughput (bytes/sec)
    send_ema_bps: f64,       // EMA of consumer throughput (bytes/sec)
    last_recv: Instant,      // Timestamp of last on_recv call
    last_send: Instant,      // Timestamp of last on_send call
    alpha: f64,              // Smoothing factor (default 0.3)
}

impl ThroughputProbe {
    pub fn new(alpha: f64) -> Self;
    pub fn on_recv(&mut self, bytes: usize);  // Record producer data arrival
    pub fn on_send(&mut self, bytes: usize);  // Record consumer data emission
    pub fn ratio(&self) -> f64;               // recv_ema / send_ema (1.0 if no data)
}
```

**Invariants:**
- `alpha` ∈ (0.0, 1.0)
- Zero-elapsed samples are skipped (no division by zero)
- Returns 1.0 (neutral) when `send_ema_bps <= 0.0`
- Memory: 40 bytes (2×f64 + 2×Instant(16 bytes on Linux) + 1×f64) — within 64-byte budget

### Resize Decision Function

```rust
pub(crate) fn compute_target_chunk_size(
    ratio: f64,
    current: usize,
    min: usize,
    max: usize,
    growth_factor: f64,   // default 2.0
    shrink_factor: f64,   // default 0.5
    grow_ratio: f64,      // default 1.5
    shrink_ratio: f64,    // default 0.7
) -> usize;
```

**Logic:**
- `ratio > grow_ratio` → `(current as f64 * growth_factor) as usize`, clamped to `[min, max]`
- `ratio < shrink_ratio` → `(current as f64 * shrink_factor) as usize`, clamped to `[min, max]`
- Otherwise → `current` (no change)
- Uses `saturating_mul` equivalent to prevent overflow before clamping

**Properties:**
- Pure function — no side effects, fully deterministic
- Output always ∈ `[min, max]`
- Hysteresis band `[shrink_ratio, grow_ratio]` prevents oscillation

### AdaptiveResizer

```rust
pub(crate) async fn spawn_adaptive_resizer(
    rx: mpsc::Receiver<StreamChunk>,
    initial_chunk_size: usize,
    min: usize,
    max: usize,
    capacity: usize,
    config: AdaptiveChunkConfig,
    stage_name: &str,
) -> mpsc::Receiver<StreamChunk>;
```

**Behavior:**
- Spawns a Tokio task that runs the feedback loop
- Buffers incoming `Data` chunks in a `BytesMut`
- Emits complete chunks at `target_chunk_size` to the output channel
- On `End` or channel closure: flushes remaining buffer as partial chunk, sends `End`
- On `Error`: forwards error immediately, terminates without flushing
- On downstream drop (send error): terminates, releasing input receiver for upstream cancellation propagation
- Records metrics on each resize event and final stabilized size

### AdaptiveChunkConfig

```rust
#[derive(Debug, Clone)]
pub struct AdaptiveChunkConfig {
    pub smoothing_factor: f64,   // α for EMA, default 0.3, range (0.0, 1.0)
    pub growth_factor: f64,      // Multiplier when growing, default 2.0, must be > 1.0
    pub shrink_factor: f64,      // Multiplier when shrinking, default 0.5, range (0.0, 1.0)
    pub target_latency_us: u64,  // Reserved for future use, default 0
    pub grow_ratio: f64,         // Threshold to grow, default 1.5
    pub shrink_ratio: f64,       // Threshold to shrink, default 0.7
}
```

### PipelineConfig Extension

```rust
pub struct PipelineConfig {
    // Existing fields (unchanged):
    pub chunk_size: usize,           // default: 64KB
    pub channel_capacity: usize,     // default: 8
    pub cache_intermediates: bool,   // default: true
    pub memory_budget: usize,        // default: 0

    // New fields (§2.10):
    pub adaptive_chunking: bool,     // default: false (opt-in)
    pub min_chunk_size: usize,       // default: 1024 (1KB)
    pub max_chunk_size: usize,       // default: 1048576 (1MB)
}
```

**Validation:** `min_chunk_size <= chunk_size <= max_chunk_size` and `min_chunk_size >= 1024`

### Pipeline Builder Integration

The `StreamPipeline::execute()` method conditionally wraps input receivers with an `AdaptiveResizer` before passing them to streaming stage functions. A `Vec<NodeType>` is pre-computed before the execution loop to determine which upstream nodes are streaming stages:

```rust
#[derive(Clone, Copy, PartialEq)]
enum NodeType { Source, Cached, Streaming, Batch }
```

Only `Streaming → Streaming` edges get a resizer. Source, Cached, and Batch nodes do not.

### Observability Metrics

New metrics added to `crates/deriva-compute/src/metrics.rs`:

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `deriva_adaptive_chunk_size_bytes` | Gauge | `stage` | Current target chunk size per resizer |
| `deriva_adaptive_resize_events_total` | Counter | `stage`, `direction` | Resize event count (grow/shrink) |

Events recorded:
- On each resize: old size, new size, direction
- On pipeline completion: final stabilized chunk size observation

## Data Models

### Core Types

```rust
/// StreamChunk — existing (unchanged)
pub enum StreamChunk {
    Data(Bytes),
    End,
    Error(DerivaError),
}

/// ThroughputProbe — new, internal to adaptive module
pub(crate) struct ThroughputProbe {
    recv_ema_bps: f64,
    send_ema_bps: f64,
    last_recv: Instant,
    last_send: Instant,
    alpha: f64,
}

/// AdaptiveChunkConfig — new, public configuration
#[derive(Debug, Clone)]
pub struct AdaptiveChunkConfig {
    pub smoothing_factor: f64,
    pub growth_factor: f64,
    pub shrink_factor: f64,
    pub target_latency_us: u64,
    pub grow_ratio: f64,
    pub shrink_ratio: f64,
}

impl Default for AdaptiveChunkConfig {
    fn default() -> Self {
        Self {
            smoothing_factor: 0.3,
            growth_factor: 2.0,
            shrink_factor: 0.5,
            target_latency_us: 0,
            grow_ratio: 1.5,
            shrink_ratio: 0.7,
        }
    }
}
```

### Constants

```rust
pub const MIN_CHUNK_SIZE: usize = 1024;           // 1 KB absolute floor
pub const MAX_CHUNK_SIZE: usize = 1024 * 1024;    // 1 MB ceiling
pub const DEFAULT_EMA_ALPHA: f64 = 0.3;
pub const DEFAULT_GROW_RATIO: f64 = 1.5;
pub const DEFAULT_SHRINK_RATIO: f64 = 0.7;
pub const DEFAULT_GROWTH_FACTOR: f64 = 2.0;
pub const DEFAULT_SHRINK_FACTOR: f64 = 0.5;
```

### File Layout

```
crates/deriva-compute/src/
├── adaptive.rs          (NEW — ThroughputProbe, compute_target_chunk_size, spawn_adaptive_resizer)
├── pipeline.rs          (MODIFIED — PipelineConfig extension, resizer insertion logic)
├── streaming.rs         (UNCHANGED)
├── metrics.rs           (MODIFIED — new adaptive chunking metrics)
└── builtins_streaming/
    └── core.rs          (UNCHANGED — StreamingChunkResizer remains for fixed-target use)
```


## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system — essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: EMA Update Correctness

*For any* sequence of `(bytes, elapsed)` measurement pairs and any valid smoothing factor α ∈ (0.0, 1.0), the ThroughputProbe's EMA values after processing the sequence SHALL equal the result of iteratively applying `new_ema = α × (bytes/elapsed) + (1-α) × old_ema` for each sample with elapsed > 0, starting from 0.0. Samples with elapsed = 0 SHALL leave the EMA unchanged.

**Validates: Requirements 1.2, 1.3, 1.4, 1.5, 7.2**

### Property 2: EMA Convergence

*For any* constant throughput rate R > 0 and smoothing factor α = 0.3, after feeding 7 or more samples at rate R to the ThroughputProbe, the EMA value SHALL be within 10% of R (i.e., |ema - R| / R < 0.1).

**Validates: Requirements 1.7**

### Property 3: Resize Decision Correctness

*For any* throughput ratio, current chunk size, growth factor > 1.0, shrink factor ∈ (0.0, 1.0), grow ratio threshold, and shrink ratio threshold where grow_ratio > shrink_ratio: when ratio > grow_ratio the result SHALL be `current × growth_factor` (clamped); when ratio < shrink_ratio the result SHALL be `current × shrink_factor` (clamped); when ratio ∈ [shrink_ratio, grow_ratio] the result SHALL equal current (clamped).

**Validates: Requirements 2.1, 2.2, 2.3, 7.3, 7.4**

### Property 4: Resize Output Always In Bounds

*For any* throughput ratio (including extreme values), any current chunk size (including values near usize::MAX), and any valid bounds [min, max] where min ≤ max, the resize decision function output SHALL always be within [min, max].

**Validates: Requirements 2.4, 2.5**

### Property 5: Data Integrity Round-Trip

*For any* input byte stream (arbitrary content, arbitrary chunking boundaries), the concatenation of all output `Data` chunks from the AdaptiveResizer SHALL be byte-identical to the concatenation of all input `Data` chunks. No bytes are dropped, duplicated, or reordered regardless of target chunk size changes during processing.

**Validates: Requirements 3.6, 9.1, 9.3, 9.4**

### Property 6: Pipeline Semantic Transparency

*For any* valid pipeline DAG with streaming stages and any input data, the final collected output with `adaptive_chunking = true` SHALL be byte-identical to the output with `adaptive_chunking = false`. Adaptive chunking affects only internal chunk boundaries, never the semantic content.

**Validates: Requirements 4.4, 5.7, 9.2, 11.1**

### Property 7: Configuration Validation

*For any* AdaptiveChunkConfig values, validation SHALL accept if and only if: `smoothing_factor ∈ (0.0, 1.0)` AND `growth_factor > 1.0` AND `shrink_factor ∈ (0.0, 1.0)`. For PipelineConfig, validation SHALL accept if and only if: `min_chunk_size ≤ chunk_size ≤ max_chunk_size` AND `min_chunk_size ≥ 1024`.

**Validates: Requirements 4.5, 7.5**

### Property 8: Resizer Insertion Rules

*For any* pipeline DAG with `adaptive_chunking = true`, an AdaptiveResizer SHALL be inserted between two nodes if and only if both are `StreamingStage` nodes. Source, Cached, and BatchStage nodes feeding a StreamingStage SHALL NOT have a resizer inserted on that edge.

**Validates: Requirements 5.1, 5.2, 5.3, 5.4**

### Property 9: Preferred Chunk Size Clamping

*For any* value returned by `preferred_chunk_size()` (including values outside [min_chunk_size, max_chunk_size]), the pipeline builder SHALL clamp it to [min_chunk_size, max_chunk_size] without error when using it as the initial target for an AdaptiveResizer.

**Validates: Requirements 6.3, 6.4**

### Property 10: Bounded Convergence

*For any* initial chunk size within [min, max] and any stable throughput ratio outside the hysteresis band, the AdaptiveResizer SHALL reach a bound (min or max) within at most `ceil(log2(max / min))` resize decisions. With default bounds [1KB, 1MB], this is at most 10 adjustments.

**Validates: Requirements 10.1**

### Property 11: Short Streams Pass-Through

*For any* input stream whose total data size results in fewer than 3 chunks at the initial target size, the AdaptiveResizer SHALL emit chunks at the initial target size only (no resize decisions are applied).

**Validates: Requirements 10.4**

## Error Handling

### ThroughputProbe Errors

| Condition | Handling | Rationale |
|-----------|----------|-----------|
| Zero elapsed time between measurements | Skip EMA update, retain previous value | Prevents division by zero; occurs on very fast consecutive calls |
| `send_ema_bps` is zero (cold start) | Return neutral ratio 1.0 | No data to make a decision; default to "hold" |
| NaN/Inf from floating-point arithmetic | Treat as neutral ratio 1.0 | Defensive; shouldn't occur with the elapsed > 0 guard |

### Resize Decision Errors

| Condition | Handling | Rationale |
|-----------|----------|-----------|
| `current_size × growth_factor` overflows | Saturating multiply, then clamp to max | Prevents panic on extreme values |
| `current_size × shrink_factor` rounds to 0 | Clamp to min_chunk_size | Ensures output is always ≥ min |
| `min > max` in bounds | Validation rejects config at construction | Invalid configuration caught early |

### AdaptiveResizer Errors

| Condition | Handling | Rationale |
|-----------|----------|-----------|
| Input stream yields `Error(e)` | Forward error to output, terminate task (no flush) | Error semantics: downstream learns immediately, buffer may contain partial/corrupt data |
| Output channel receiver dropped | Detect `send().is_err()`, terminate task | Cancellation propagation: releasing input receiver signals upstream |
| Input channel closed without `End` | Flush buffer, emit `End` | Treat as graceful close (Tokio channel semantics) |
| Empty input stream (immediate `End`) | Emit `End` immediately (no data chunks) | No data to process is valid |

### PipelineConfig Validation Errors

| Condition | Handling | Rationale |
|-----------|----------|-----------|
| `min_chunk_size > chunk_size` | Return `Err(InvalidConfig)` | Inconsistent bounds detected at build time |
| `chunk_size > max_chunk_size` | Return `Err(InvalidConfig)` | Inconsistent bounds detected at build time |
| `min_chunk_size < 1024` | Return `Err(InvalidConfig)` | Below 1KB, channel overhead dominates |
| `smoothing_factor` ≤ 0.0 or ≥ 1.0 | Return `Err(InvalidConfig)` | EMA formula requires α ∈ (0, 1) |
| `growth_factor` ≤ 1.0 | Return `Err(InvalidConfig)` | Growth must increase chunk size |
| `shrink_factor` ≤ 0.0 or ≥ 1.0 | Return `Err(InvalidConfig)` | Shrink must decrease chunk size |

## Testing Strategy

### Unit Tests (Example-Based)

Unit tests cover specific scenarios, edge cases, and integration points:

- **ThroughputProbe cold start**: Verify ratio returns 1.0 before any measurements
- **ThroughputProbe zero-elapsed skip**: Verify EMA is unchanged when two calls occur at same instant
- **Resize decision boundary values**: Verify exact behavior at grow_ratio and shrink_ratio boundaries
- **AdaptiveResizer error forwarding**: Verify Error chunks are forwarded without flushing buffer
- **AdaptiveResizer cancellation**: Verify task terminates when downstream drops receiver
- **Pipeline builder insertion**: Verify resizer inserted only on Streaming→Streaming edges with specific pipeline topologies
- **Config validation rejection**: Verify invalid configs produce appropriate errors
- **Backward compatibility**: Verify PipelineConfig defaults unchanged, StreamingChunkResizer still works

### Property-Based Tests

Property-based tests verify universal properties across randomized inputs. Each property test runs a minimum of 100 iterations.

**Library**: `proptest` (Rust PBT framework, already available in the workspace)

| Property | Generator Strategy | Validation |
|----------|-------------------|------------|
| Property 1: EMA Update | Random Vec<(usize, Duration)> pairs + random α | Manual EMA computation matches probe |
| Property 2: EMA Convergence | Random constant rate R, α=0.3, 7+ samples | |ema - R| / R < 0.1 |
| Property 3: Resize Decision | Random (ratio, current, growth_factor, shrink_factor, bounds) | Correct branch taken + multiplication |
| Property 4: Bounds Invariant | Random (ratio, current up to usize::MAX, bounds) | Output ∈ [min, max] |
| Property 5: Data Integrity | Random Vec<Bytes> input stream, random initial target | concat(output) == concat(input) |
| Property 6: Pipeline Transparency | Random pipeline + input data | output(adaptive=true) == output(adaptive=false) |
| Property 7: Config Validation | Random config field values | Accept iff all fields in valid ranges |
| Property 8: Insertion Rules | Random pipeline DAG topology | Resizer count == Streaming→Streaming edge count |
| Property 9: Clamping | Random preferred sizes + random bounds | Result ∈ [min, max] |
| Property 10: Bounded Convergence | Random initial_size, stable ratio outside band | Adjustments ≤ ceil(log2(max/min)) |
| Property 11: Short Streams | Random data < 3×initial_target bytes | No resize decisions applied |

**Property test tag format**: `Feature: adaptive-chunk-sizing, Property {N}: {title}`

### Integration Tests

- **End-to-end pipeline with adaptive chunking**: Run a multi-stage pipeline (compress → encrypt → hash) with adaptive chunking enabled, verify correct output and observe metric emissions
- **Metric recording**: Verify `deriva_adaptive_chunk_size_bytes` gauge and `deriva_adaptive_resize_events_total` counter are updated during pipeline execution
- **Convergence under load**: Run a heterogeneous pipeline and verify chunk sizes stabilize within expected bounds

### Benchmarks

- **Throughput comparison**: Fixed 64KB vs adaptive chunking on heterogeneous 5-stage pipeline
- **Overhead measurement**: Timing overhead of ThroughputProbe per chunk (~36 ns target)
- **Convergence speed**: Chunks to reach steady state for various producer/consumer rate ratios
