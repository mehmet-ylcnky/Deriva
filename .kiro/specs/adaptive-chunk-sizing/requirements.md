# Requirements Document

## Introduction

Phase 2.10 of the Computation-Addressed Distributed File System (Deriva/CAS-DFS) replaces the fixed 64KB chunk size with per-stage adaptive chunk sizing. Each streaming stage selects its optimal chunk size based on throughput feedback using exponential moving averages. Fast stages use larger chunks to reduce channel overhead; slow stages use smaller chunks for better time-to-first-byte (TTFB) and pipeline balance. An AdaptiveResizer node is inserted between pipeline stages with mismatched throughput characteristics, measuring producer/consumer rate ratios and dynamically adjusting chunk boundaries within configurable bounds (4KB–1MB). This eliminates pipeline bubbles caused by uniform chunk sizing across heterogeneous stages, yielding 7–12% throughput improvements for mixed-speed pipelines while adding negligible overhead (~64 bytes per stage, ~36 ns per chunk for timing).

## Glossary

- **ThroughputProbe**: A lightweight struct that tracks producer and consumer throughput using exponential moving averages (EMA) to compute the producer/consumer rate ratio.
- **AdaptiveResizer**: A pipeline node inserted between adjacent streaming stages that re-chunks data to a dynamically-adjusted target size based on ThroughputProbe feedback.
- **EMA (Exponential Moving Average)**: A smoothing technique where `new_ema = α × sample + (1 - α) × old_ema`. Used to filter throughput measurement noise. With α=0.3, convergence to within 10% of the true value occurs within 7 samples.
- **Smoothing_Factor (α)**: The EMA coefficient controlling how quickly the average reacts to new measurements. Default: 0.3. Range: (0.0, 1.0).
- **Throughput_Ratio**: The ratio `recv_ema_bps / send_ema_bps` computed by ThroughputProbe. Values >1 indicate the producer is faster; values <1 indicate the consumer is faster.
- **Grow_Ratio**: The threshold above which the AdaptiveResizer doubles chunk size. Default: 1.5. Meaning: producer is at least 1.5× faster than consumer.
- **Shrink_Ratio**: The threshold below which the AdaptiveResizer halves chunk size. Default: 0.7. Meaning: consumer is at least 1.43× faster than producer.
- **Hysteresis_Band**: The range [Shrink_Ratio, Grow_Ratio] within which no chunk size adjustment occurs. Prevents oscillation when throughput is roughly balanced.
- **Min_Chunk_Size**: The minimum chunk size the AdaptiveResizer can shrink to. Default: 1024 bytes (1KB).
- **Max_Chunk_Size**: The maximum chunk size the AdaptiveResizer can grow to. Default: 1048576 bytes (1MB).
- **Initial_Chunk_Size**: The starting chunk size before adaptation begins. Default: 65536 bytes (64KB). Overridable per-function via `preferred_chunk_size()`.
- **AdaptiveChunkConfig**: Per-stage configuration struct holding target_latency_us, smoothing_factor, growth_factor, and shrink_factor.
- **StreamingComputeFunction**: The async trait for streaming compute functions (defined in §2.7). Extended here via its existing `preferred_chunk_size()` method.
- **StreamPipeline**: The pipeline builder that constructs and executes a DAG of streaming stages (defined in §2.7).
- **PipelineConfig**: Configuration struct for pipeline execution. Extended here with adaptive chunking fields.
- **StreamChunk**: The enum representing a unit of data in the streaming pipeline: `Data(Bytes)`, `End`, or `Error(DerivaError)`.
- **Pipeline_Bubble**: A period where a pipeline stage idles waiting for input, caused by throughput mismatch between adjacent stages with uniform chunk sizes.

## Requirements

### Requirement 1: ThroughputProbe EMA Measurement

**User Story:** As a system developer, I want per-resizer throughput measurement using exponential moving averages, so that the adaptive resizer can make informed chunk size decisions based on smoothed producer/consumer rate data.

#### Acceptance Criteria

1. THE ThroughputProbe SHALL track two independent EMA values: `recv_ema_bps` (bytes per second arriving from the producer) and `send_ema_bps` (bytes per second accepted by the consumer).
2. WHEN `on_recv` is called with a byte count, THE ThroughputProbe SHALL compute the instantaneous rate as `bytes / elapsed_seconds` and update `recv_ema_bps` using the formula `α × instantaneous + (1 - α) × recv_ema_bps`.
3. WHEN `on_send` is called with a byte count, THE ThroughputProbe SHALL compute the instantaneous rate as `bytes / elapsed_seconds` and update `send_ema_bps` using the formula `α × instantaneous + (1 - α) × send_ema_bps`.
4. IF the elapsed time since the previous measurement of the same type is zero, THEN THE ThroughputProbe SHALL skip the EMA update for that sample to avoid division by zero.
5. THE ThroughputProbe SHALL compute Throughput_Ratio as `recv_ema_bps / send_ema_bps`.
6. IF `send_ema_bps` is zero or negative, THEN THE ThroughputProbe SHALL return a neutral ratio of 1.0 (no data available for decision).
7. THE ThroughputProbe SHALL converge to within 10% of the true throughput ratio within 7 samples when Smoothing_Factor is 0.3.
8. THE ThroughputProbe SHALL consume at most 64 bytes of memory (two f64 EMA values, two timestamps, one f64 alpha).

### Requirement 2: Resize Decision Function

**User Story:** As a system developer, I want a pure decision function that maps throughput ratio to chunk size adjustments, so that the adaptation logic is testable in isolation and uses power-of-2 scaling with hysteresis to prevent oscillation.

#### Acceptance Criteria

1. WHEN Throughput_Ratio exceeds Grow_Ratio (default 1.5), THE resize decision function SHALL return `current_chunk_size × 2` (double).
2. WHEN Throughput_Ratio is below Shrink_Ratio (default 0.7), THE resize decision function SHALL return `current_chunk_size / 2` (halve).
3. WHEN Throughput_Ratio is within the Hysteresis_Band [Shrink_Ratio, Grow_Ratio], THE resize decision function SHALL return the current chunk size unchanged.
4. THE resize decision function SHALL clamp the computed chunk size to [Min_Chunk_Size, Max_Chunk_Size] before returning.
5. WHEN `current_chunk_size × 2` would overflow usize, THE resize decision function SHALL use saturating multiplication and clamp to Max_Chunk_Size.

### Requirement 3: AdaptiveResizer Node

**User Story:** As a system developer, I want an adaptive resizer node that buffers incoming chunks and re-emits them at a dynamically-adjusted target size, so that downstream stages receive optimally-sized chunks for their throughput characteristics.

#### Acceptance Criteria

1. THE AdaptiveResizer SHALL accept an input `Receiver<StreamChunk>`, an initial chunk size, min/max bounds, and a channel capacity, and return a new `Receiver<StreamChunk>` that emits re-chunked data.
2. WHEN the AdaptiveResizer receives a `Data` chunk, THE AdaptiveResizer SHALL buffer the data and emit complete chunks of the current target size to the output channel.
3. WHEN the AdaptiveResizer has emitted one or more chunks, THE AdaptiveResizer SHALL invoke the resize decision function and update its target chunk size for subsequent emissions.
4. WHEN the input stream terminates (`End` or channel closure), THE AdaptiveResizer SHALL flush any remaining buffered data as a final partial chunk and then emit `End`.
5. IF the input stream yields an `Error` chunk, THEN THE AdaptiveResizer SHALL forward the error to the output channel and terminate without flushing the buffer.
6. THE AdaptiveResizer SHALL preserve data integrity: the concatenation of all output `Data` chunks SHALL be byte-identical to the concatenation of all input `Data` chunks.
7. WHEN the output channel receiver is dropped (downstream cancellation), THE AdaptiveResizer SHALL detect the send error and terminate, releasing its input receiver to propagate cancellation upstream.
8. THE AdaptiveResizer SHALL record the ThroughputProbe `on_recv` timestamp/bytes for each received chunk and `on_send` timestamp/bytes for each emitted chunk.

### Requirement 4: PipelineConfig Adaptive Chunking Extension

**User Story:** As a system developer, I want PipelineConfig extended with adaptive chunking parameters, so that adaptive behavior is opt-in and configurable per pipeline.

#### Acceptance Criteria

1. THE PipelineConfig SHALL include an `adaptive_chunking` boolean field (default: false) that enables or disables adaptive chunk sizing for the pipeline.
2. THE PipelineConfig SHALL include a `min_chunk_size` field (default: 1024 bytes) specifying the minimum chunk size the AdaptiveResizer can produce.
3. THE PipelineConfig SHALL include a `max_chunk_size` field (default: 1048576 bytes) specifying the maximum chunk size the AdaptiveResizer can produce.
4. WHEN `adaptive_chunking` is false, THE StreamPipeline SHALL execute identically to pre-§2.10 behavior with no AdaptiveResizer nodes inserted.
5. THE PipelineConfig SHALL validate that `min_chunk_size <= chunk_size <= max_chunk_size` and that `min_chunk_size >= 1024` (1KB absolute floor).

### Requirement 5: Automatic Resizer Insertion by Pipeline Builder

**User Story:** As a system developer, I want the pipeline builder to automatically insert AdaptiveResizer nodes between streaming stages when adaptive chunking is enabled, so that developers do not need to manually manage chunk boundaries.

#### Acceptance Criteria

1. WHEN `adaptive_chunking` is true, THE StreamPipeline builder SHALL insert an AdaptiveResizer between each pair of adjacent StreamingStage nodes in the pipeline DAG.
2. THE StreamPipeline builder SHALL NOT insert an AdaptiveResizer between a Source node and a StreamingStage node.
3. THE StreamPipeline builder SHALL NOT insert an AdaptiveResizer between a BatchStage node and a StreamingStage node (BatchStage already re-chunks via `batch_to_stream`).
4. THE StreamPipeline builder SHALL NOT insert an AdaptiveResizer between a Cached node and a StreamingStage node.
5. WHEN an AdaptiveResizer is inserted, THE StreamPipeline builder SHALL initialize its target chunk size to the downstream function's `preferred_chunk_size()` return value.
6. THE StreamPipeline builder SHALL pass `PipelineConfig::min_chunk_size`, `PipelineConfig::max_chunk_size`, and `PipelineConfig::channel_capacity` to each inserted AdaptiveResizer.
7. THE AdaptiveResizer insertion SHALL be transparent to pipeline semantics: the pipeline output SHALL be byte-identical whether adaptive chunking is enabled or disabled.

### Requirement 6: Per-Function Preferred Chunk Size

**User Story:** As a system developer, I want each streaming function to declare its preferred initial chunk size via the existing `preferred_chunk_size()` trait method, so that compute-heavy functions start with larger chunks and the adaptive resizer converges faster.

#### Acceptance Criteria

1. THE `preferred_chunk_size()` method on StreamingComputeFunction SHALL be consulted by the pipeline builder when determining the initial chunk size for an AdaptiveResizer feeding that function.
2. WHEN a function does not override `preferred_chunk_size()`, THE default return value SHALL remain 65536 bytes (64KB), preserving backward compatibility.
3. THE `preferred_chunk_size()` return value SHALL be clamped to [Min_Chunk_Size, Max_Chunk_Size] by the pipeline builder before use as an initial target.
4. WHEN a streaming function overrides `preferred_chunk_size()` to return a value outside [Min_Chunk_Size, Max_Chunk_Size], THE pipeline builder SHALL clamp to the nearest bound without returning an error.

### Requirement 7: AdaptiveChunkConfig Per-Stage Configuration

**User Story:** As a system developer, I want per-stage adaptive chunk configuration, so that different stages can have different smoothing factors, growth/shrink ratios, and latency targets tuned to their workload characteristics.

#### Acceptance Criteria

1. THE AdaptiveChunkConfig struct SHALL contain the following fields: `smoothing_factor` (f64, default 0.3), `growth_factor` (f64, default 2.0), `shrink_factor` (f64, default 0.5), and `target_latency_us` (u64, default 0).
2. WHEN `smoothing_factor` is specified, THE ThroughputProbe SHALL use that value as its α parameter instead of the default 0.3.
3. WHEN `growth_factor` is specified, THE resize decision function SHALL multiply the current chunk size by `growth_factor` (instead of the default 2.0) when growing.
4. WHEN `shrink_factor` is specified, THE resize decision function SHALL multiply the current chunk size by `shrink_factor` (instead of the default 0.5) when shrinking.
5. THE AdaptiveChunkConfig SHALL validate that `smoothing_factor` is in the exclusive range (0.0, 1.0), that `growth_factor` is greater than 1.0, and that `shrink_factor` is in the exclusive range (0.0, 1.0).

### Requirement 8: Observability Metrics for Adaptive Chunking

**User Story:** As a system developer, I want per-stage metrics for chunk size and adaptation events, so that I can monitor convergence behavior and diagnose performance issues in production.

#### Acceptance Criteria

1. THE system SHALL expose a `deriva_adaptive_chunk_size_bytes` gauge metric, labeled by stage name, reporting the current target chunk size of each active AdaptiveResizer.
2. THE system SHALL expose a `deriva_adaptive_resize_events_total` counter metric, labeled by stage name and direction (`grow` or `shrink`), incrementing each time the AdaptiveResizer adjusts its target chunk size.
3. WHEN an AdaptiveResizer adjusts its target chunk size, THE system SHALL record a metric event with the old size, new size, and direction (grow/shrink).
4. WHEN a pipeline completes, THE system SHALL record the final stabilized chunk size for each AdaptiveResizer as an observation.

### Requirement 9: Data Integrity Preservation

**User Story:** As a system developer, I want the adaptive chunking system to guarantee byte-level data integrity regardless of chunk size changes, so that downstream consumers receive identical data whether or not adaptive chunking is enabled.

#### Acceptance Criteria

1. FOR ALL valid input streams, THE concatenation of output Data chunks from the AdaptiveResizer SHALL be byte-identical to the concatenation of input Data chunks (round-trip property).
2. FOR ALL valid pipeline configurations, THE final collected output of a pipeline with `adaptive_chunking = true` SHALL be byte-identical to the output with `adaptive_chunking = false` for the same inputs and functions.
3. WHEN the AdaptiveResizer changes its target chunk size mid-stream, THE AdaptiveResizer SHALL NOT drop, duplicate, or reorder any buffered bytes.
4. WHEN the input stream total size is not evenly divisible by the target chunk size, THE AdaptiveResizer SHALL emit the remainder as a final chunk smaller than the target size.

### Requirement 10: Convergence and Stability

**User Story:** As a system developer, I want the adaptive chunk sizing to converge within a bounded number of chunks and remain stable once converged, so that the system avoids perpetual oscillation and achieves steady-state throughput quickly.

#### Acceptance Criteria

1. THE AdaptiveResizer SHALL converge from Initial_Chunk_Size to steady-state within ceil(log2(Max_Chunk_Size / Min_Chunk_Size)) chunk cycles (at most 8 adjustments for 4KB–1MB range).
2. WHEN the throughput ratio is stable within the Hysteresis_Band for 3 consecutive chunks, THE AdaptiveResizer SHALL NOT adjust the chunk size (steady state).
3. WHEN the throughput ratio transitions from outside to inside the Hysteresis_Band, THE AdaptiveResizer SHALL stop adjusting immediately on the first chunk within the band.
4. FOR streams with fewer than 3 chunks, THE AdaptiveResizer SHALL pass data through at the initial target size without attempting adaptation (insufficient data for meaningful feedback).

### Requirement 11: Backward Compatibility

**User Story:** As a system developer, I want adaptive chunk sizing to be fully backward-compatible with existing pipelines, so that no existing behavior changes unless adaptive chunking is explicitly enabled.

#### Acceptance Criteria

1. WHEN `adaptive_chunking` is false (default), THE StreamPipeline SHALL produce identical execution behavior, timing characteristics, and output to pre-§2.10 code.
2. THE existing `StreamingChunkResizer` builtin function SHALL remain unchanged and available for fixed-target re-chunking use cases.
3. THE existing `preferred_chunk_size()` trait method signature and default return value SHALL remain unchanged.
4. THE PipelineConfig default values for all pre-existing fields SHALL remain unchanged.
