# Implementation Plan: Adaptive Chunk Sizing

## Overview

This plan implements per-stage adaptive chunk sizing (§2.10) for the streaming pipeline. The implementation introduces `ThroughputProbe` for EMA-based throughput measurement, a pure `compute_target_chunk_size` resize decision function, and the `AdaptiveResizer` buffering node. The pipeline builder is extended to automatically insert resizers between adjacent streaming stages when `adaptive_chunking` is enabled. All work is in the `crates/deriva-compute` crate, primarily in a new `adaptive.rs` module with modifications to `pipeline.rs` and `metrics.rs`.

## Tasks

- [ ] 1. Create adaptive module with core types and constants
  - [ ] 1.1 Create `crates/deriva-compute/src/adaptive.rs` with module structure, constants, and `AdaptiveChunkConfig`
    - Define constants: `MIN_CHUNK_SIZE` (1024), `MAX_CHUNK_SIZE` (1048576), `DEFAULT_EMA_ALPHA` (0.3), `DEFAULT_GROW_RATIO` (1.5), `DEFAULT_SHRINK_RATIO` (0.7), `DEFAULT_GROWTH_FACTOR` (2.0), `DEFAULT_SHRINK_FACTOR` (0.5)
    - Implement `AdaptiveChunkConfig` struct with `smoothing_factor`, `growth_factor`, `shrink_factor`, `target_latency_us`, `grow_ratio`, `shrink_ratio` fields
    - Implement `Default` for `AdaptiveChunkConfig` with specified defaults
    - Implement validation method that rejects `smoothing_factor` not in (0.0, 1.0), `growth_factor` ≤ 1.0, `shrink_factor` not in (0.0, 1.0)
    - Register the module in `crates/deriva-compute/src/lib.rs`
    - _Requirements: 7.1, 7.5_

  - [ ] 1.2 Extend `PipelineConfig` in `crates/deriva-compute/src/pipeline.rs` with adaptive chunking fields
    - Add `adaptive_chunking: bool` (default: false), `min_chunk_size: usize` (default: 1024), `max_chunk_size: usize` (default: 1048576) fields
    - Update `Default` impl to include new field defaults
    - Implement validation: `min_chunk_size <= chunk_size <= max_chunk_size` and `min_chunk_size >= 1024`
    - _Requirements: 4.1, 4.2, 4.3, 4.5, 11.4_

- [ ] 2. Implement ThroughputProbe
  - [ ] 2.1 Implement `ThroughputProbe` struct in `crates/deriva-compute/src/adaptive.rs`
    - Define struct with `recv_ema_bps: f64`, `send_ema_bps: f64`, `last_recv: Instant`, `last_send: Instant`, `alpha: f64`
    - Implement `new(alpha: f64) -> Self` initializing EMAs to 0.0 and timestamps to `Instant::now()`
    - Implement `on_recv(&mut self, bytes: usize)` computing instantaneous rate and updating EMA; skip if elapsed is zero
    - Implement `on_send(&mut self, bytes: usize)` computing instantaneous rate and updating EMA; skip if elapsed is zero
    - Implement `ratio(&self) -> f64` returning `recv_ema_bps / send_ema_bps` or 1.0 if `send_ema_bps <= 0.0`
    - Handle NaN/Inf defensively by returning 1.0
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8_

  - [ ]* 2.2 Write property test for ThroughputProbe EMA Update (Property 1)
    - **Property 1: EMA Update Correctness**
    - Generate random `Vec<(usize, Duration)>` measurement pairs and random α ∈ (0.0, 1.0)
    - Verify ThroughputProbe EMA values match manual iterative EMA computation
    - **Validates: Requirements 1.2, 1.3, 1.4, 1.5, 7.2**

  - [ ]* 2.3 Write property test for ThroughputProbe convergence (Property 2)
    - **Property 2: EMA Convergence**
    - Generate random constant rate R > 0, feed 7+ samples at rate R with α=0.3
    - Verify `|ema - R| / R < 0.1` after 7 samples
    - **Validates: Requirements 1.7**

- [ ] 3. Implement resize decision function
  - [ ] 3.1 Implement `compute_target_chunk_size` pure function in `crates/deriva-compute/src/adaptive.rs`
    - Accept `ratio`, `current`, `min`, `max`, `growth_factor`, `shrink_factor`, `grow_ratio`, `shrink_ratio`
    - When `ratio > grow_ratio`: return `(current as f64 * growth_factor) as usize` clamped to [min, max]
    - When `ratio < shrink_ratio`: return `(current as f64 * shrink_factor) as usize` clamped to [min, max]
    - Otherwise: return `current` clamped to [min, max]
    - Use saturating arithmetic to prevent overflow
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [ ]* 3.2 Write property test for resize decision correctness (Property 3)
    - **Property 3: Resize Decision Correctness**
    - Generate random `(ratio, current, growth_factor, shrink_factor, grow_ratio, shrink_ratio, min, max)`
    - Verify correct branch taken and multiplication applied with clamping
    - **Validates: Requirements 2.1, 2.2, 2.3, 7.3, 7.4**

  - [ ]* 3.3 Write property test for resize output bounds (Property 4)
    - **Property 4: Resize Output Always In Bounds**
    - Generate random ratio (including extremes), current (including near usize::MAX), and valid bounds [min, max]
    - Verify output is always within [min, max]
    - **Validates: Requirements 2.4, 2.5**

- [ ] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 5. Implement AdaptiveResizer node
  - [ ] 5.1 Implement `spawn_adaptive_resizer` async function in `crates/deriva-compute/src/adaptive.rs`
    - Accept `rx: mpsc::Receiver<StreamChunk>`, `initial_chunk_size`, `min`, `max`, `capacity`, `config: AdaptiveChunkConfig`, `stage_name: &str`
    - Return a new `mpsc::Receiver<StreamChunk>` emitting re-chunked data
    - Spawn a Tokio task that buffers incoming `Data` chunks in a `BytesMut`
    - Emit complete chunks at `target_chunk_size` to the output channel
    - On each emission cycle: call `probe.on_recv()` for received bytes, `probe.on_send()` for emitted bytes, invoke `compute_target_chunk_size` and update target
    - On `End` or channel closure: flush remaining buffer as partial chunk, emit `End`
    - On `Error`: forward error immediately, terminate without flushing
    - On downstream drop (send error): terminate task, release input receiver for cancellation propagation
    - For streams with fewer than 3 chunks at initial target size: skip resize decisions (pass through at initial size)
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7, 3.8, 9.1, 9.3, 9.4, 10.4_

  - [ ]* 5.2 Write property test for data integrity round-trip (Property 5)
    - **Property 5: Data Integrity Round-Trip**
    - Generate random `Vec<Bytes>` input stream with arbitrary chunking and random initial target size
    - Run through `spawn_adaptive_resizer` and collect output
    - Verify concatenation of output Data chunks is byte-identical to concatenation of input Data chunks
    - **Validates: Requirements 3.6, 9.1, 9.3, 9.4**

  - [ ]* 5.3 Write property test for bounded convergence (Property 10)
    - **Property 10: Bounded Convergence**
    - Generate random initial_size within [min, max] and a stable throughput ratio outside hysteresis band
    - Verify resizer reaches a bound within `ceil(log2(max / min))` resize decisions
    - **Validates: Requirements 10.1**

  - [ ]* 5.4 Write property test for short streams pass-through (Property 11)
    - **Property 11: Short Streams Pass-Through**
    - Generate random data whose total size results in fewer than 3 chunks at initial target
    - Verify no resize decisions are applied (output at initial target size only)
    - **Validates: Requirements 10.4**

- [ ] 6. Implement pipeline builder integration
  - [ ] 6.1 Add `NodeType` enum and resizer insertion logic in `crates/deriva-compute/src/pipeline.rs`
    - Define `NodeType` enum: `Source`, `Cached`, `Streaming`, `Batch`
    - In `StreamPipeline::execute()`, pre-compute a `Vec<NodeType>` from the nodes
    - When `adaptive_chunking` is true, before passing a receiver to a StreamingStage, check if the upstream node is also a StreamingStage
    - If Streaming→Streaming: wrap the receiver with `spawn_adaptive_resizer`
    - Do NOT insert resizer after Source, Cached, or BatchStage nodes
    - Initialize resizer's target to downstream function's `preferred_chunk_size()` clamped to [min_chunk_size, max_chunk_size]
    - Pass `min_chunk_size`, `max_chunk_size`, `channel_capacity` from PipelineConfig to each resizer
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 5.7, 6.1, 6.2, 6.3, 6.4, 4.4, 11.1_

  - [ ]* 6.2 Write property test for resizer insertion rules (Property 8)
    - **Property 8: Resizer Insertion Rules**
    - Generate random pipeline DAG topologies with mixed node types
    - Verify resizer count equals the number of Streaming→Streaming edges
    - **Validates: Requirements 5.1, 5.2, 5.3, 5.4**

  - [ ]* 6.3 Write property test for preferred chunk size clamping (Property 9)
    - **Property 9: Preferred Chunk Size Clamping**
    - Generate random preferred sizes (including out-of-bounds) and random min/max bounds
    - Verify the clamped result is always in [min, max] and no error is returned
    - **Validates: Requirements 6.3, 6.4**

  - [ ]* 6.4 Write property test for configuration validation (Property 7)
    - **Property 7: Configuration Validation**
    - Generate random AdaptiveChunkConfig and PipelineConfig field values
    - Verify validation accepts iff all fields are in valid ranges
    - **Validates: Requirements 4.5, 7.5**

- [ ] 7. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 8. Implement observability metrics
  - [ ] 8.1 Add adaptive chunking metrics to `crates/deriva-compute/src/metrics.rs`
    - Add `deriva_adaptive_chunk_size_bytes` gauge metric with `stage` label
    - Add `deriva_adaptive_resize_events_total` counter metric with `stage` and `direction` labels
    - _Requirements: 8.1, 8.2_

  - [ ] 8.2 Wire metrics into `spawn_adaptive_resizer` in `crates/deriva-compute/src/adaptive.rs`
    - On each resize event: increment `deriva_adaptive_resize_events_total` with direction (`grow`/`shrink`), record old size and new size
    - Update `deriva_adaptive_chunk_size_bytes` gauge with the current target chunk size after each adjustment
    - On pipeline completion (task exit): record final stabilized chunk size as observation
    - _Requirements: 8.3, 8.4_

- [ ] 9. Integration testing and pipeline semantic transparency
  - [ ]* 9.1 Write property test for pipeline semantic transparency (Property 6)
    - **Property 6: Pipeline Semantic Transparency**
    - Generate random pipeline DAG with streaming stages and random input data
    - Execute with `adaptive_chunking = true` and `adaptive_chunking = false`
    - Verify outputs are byte-identical
    - **Validates: Requirements 4.4, 5.7, 9.2, 11.1**

  - [ ]* 9.2 Write integration tests for end-to-end adaptive chunking behavior
    - Test multi-stage pipeline (compress → hash) with adaptive chunking enabled, verify correct output
    - Verify `deriva_adaptive_chunk_size_bytes` and `deriva_adaptive_resize_events_total` metrics are updated
    - Verify backward compatibility: PipelineConfig defaults unchanged, `StreamingChunkResizer` still works
    - _Requirements: 8.1, 8.2, 11.1, 11.2, 11.3, 11.4_

- [ ] 10. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The implementation language is Rust, matching the existing codebase and design document
- `proptest` is already available as a workspace dependency
- The `adaptive.rs` module is new; `pipeline.rs` and `metrics.rs` are modified
- Existing `StreamingChunkResizer` builtin remains unchanged for fixed-target re-chunking

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2"] },
    { "id": 1, "tasks": ["2.1", "3.1"] },
    { "id": 2, "tasks": ["2.2", "2.3", "3.2", "3.3"] },
    { "id": 3, "tasks": ["5.1", "8.1"] },
    { "id": 4, "tasks": ["5.2", "5.3", "5.4", "8.2"] },
    { "id": 5, "tasks": ["6.1"] },
    { "id": 6, "tasks": ["6.2", "6.3", "6.4"] },
    { "id": 7, "tasks": ["9.1", "9.2"] }
  ]
}
```
