# Implementation Plan: Size-Aware Mode Selection

## Overview

Implement per-node execution mode selection in `StreamingExecutor` based on total input size and a configurable threshold. The implementation extends `PipelineConfig` with a `streaming_threshold` field, adds size query methods to `StreamPipeline`, introduces mode selection logic with a 3-way fallback chain, and emits Prometheus telemetry for observability. The result is hybrid pipelines where small-input nodes use batch execution and large-input nodes use streaming, leveraging the existing §2.7 `batch_to_stream` adapter for interconnection.

## Tasks

- [ ] 1. Extend PipelineConfig with streaming threshold
  - [ ] 1.1 Add `streaming_threshold` field and `DEFAULT_STREAMING_THRESHOLD` constant to `crates/deriva-compute/src/pipeline.rs`
    - Define `pub const DEFAULT_STREAMING_THRESHOLD: usize = 3 * 1024 * 1024;`
    - Add `pub streaming_threshold: usize` field to `PipelineConfig`
    - Update `Default` impl to set `streaming_threshold: DEFAULT_STREAMING_THRESHOLD`
    - _Requirements: 1.1, 1.4, 1.5_

  - [ ]* 1.2 Write property test for threshold configuration (Property 4)
    - **Property 4: Threshold decision correctness**
    - Test that any `usize` value is accepted without panic
    - Test that `streaming_threshold = 0` produces always-stream preference for non-zero sizes
    - Test that `streaming_threshold = usize::MAX` produces always-batch preference
    - **Validates: Requirements 1.2, 1.3, 4.1, 4.2, 4.3, 4.4, 9.1**

- [ ] 2. Implement node data size query methods on StreamPipeline
  - [ ] 2.1 Add `node_data_size` method to `StreamPipeline` in `crates/deriva-compute/src/pipeline.rs`
    - Match on `PipelineNode` variant: `Source { data, .. } | Cached { data, .. }` → `Some(data.len())`
    - `StreamingStage { .. } | BatchStage { .. }` → `None`
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

  - [ ] 2.2 Add `total_input_size` method to `StreamPipeline` in `crates/deriva-compute/src/pipeline.rs`
    - Accept `&[usize]` input indices
    - Iterate indices, calling `node_data_size` for each
    - If any returns `None`, short-circuit and return `None`
    - Otherwise return `Some(sum)` with overflow protection via `checked_add` (return `None` on overflow)
    - Empty indices → `Some(0)`
    - _Requirements: 3.1, 3.2, 3.3_

  - [ ]* 2.3 Write property test for node data size (Property 1)
    - **Property 1: Node data size reflects stored data length**
    - Generate arbitrary `Bytes` values, store as Source/Cached, verify `node_data_size` returns `Some(data.len())`
    - **Validates: Requirements 2.1, 2.2**

  - [ ]* 2.4 Write property test for total input size summation (Property 2)
    - **Property 2: Total input size is the sum of known parts**
    - Generate multiple Source/Cached nodes with arbitrary sizes, verify `total_input_size` returns the sum
    - **Validates: Requirements 3.1**

  - [ ]* 2.5 Write property test for unknown input propagation (Property 3)
    - **Property 3: Unknown input propagates to unknown total**
    - Generate a mix of Source/Cached and StreamingStage/BatchStage nodes, verify `total_input_size` returns `None` when any computed node is included
    - **Validates: Requirements 3.2**

- [ ] 3. Implement mode selection logic in StreamingExecutor
  - [ ] 3.1 Add `select_mode` helper function in `crates/deriva-compute/src/streaming_executor.rs`
    - Implement the decision algorithm: `None` → prefer streaming, `Some(0)` → prefer streaming, `Some(s) >= threshold` → prefer streaming, `Some(s) < threshold && s > 0` → prefer batch
    - Implement 3-way fallback: if preferred mode impl exists → use it; else if alternate exists → fallback; else → `FunctionNotFound` error
    - Return a selection result indicating which stage type to build and the reason for metrics
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 5.1, 5.2, 5.3, 5.4, 5.5_

  - [ ] 3.2 Integrate `select_mode` into `materialize_streaming` in `crates/deriva-compute/src/streaming_executor.rs`
    - Replace the existing `if let Some(streaming_fn) = registry.get_streaming(...)` block with `select_mode` call
    - Call `pipeline.total_input_size(&input_indices)` before mode selection
    - Build `StreamingStage` or `BatchStage` based on selection result
    - Preserve existing input resolution and topo order logic unchanged
    - _Requirements: 4.5, 6.1, 6.2, 6.3_

  - [ ]* 3.3 Write property test for mode selection preference (Property 5)
    - **Property 5: Preferred mode selected when implementation exists**
    - Generate arbitrary threshold/size combinations with both impls available, verify preferred mode is selected
    - **Validates: Requirements 5.1, 5.3**

  - [ ]* 3.4 Write property test for mode selection fallback (Property 6)
    - **Property 6: Fallback to alternate mode when preferred unavailable**
    - Generate cases where preferred impl is missing but alternate exists, verify fallback is selected
    - **Validates: Requirements 5.2, 5.4**

- [ ] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 5. Add mode selection telemetry
  - [ ] 5.1 Add `MODE_SELECTION` counter and `STREAMING_THRESHOLD_GAUGE` to `crates/deriva-compute/src/metrics.rs`
    - Define `MODE_SELECTION: IntCounterVec` with labels `["mode", "reason"]`
    - Define `STREAMING_THRESHOLD_GAUGE: IntGauge`
    - Use existing `lazy_static!` pattern consistent with other metrics in the file
    - _Requirements: 7.1, 7.7_

  - [ ] 5.2 Emit metrics from mode selection logic in `crates/deriva-compute/src/streaming_executor.rs`
    - Increment `MODE_SELECTION` counter with appropriate `(mode, reason)` labels after each selection
    - Set `STREAMING_THRESHOLD_GAUGE` in `StreamingExecutor::new()`
    - Reason labels: `"below_threshold"`, `"above_threshold"`, `"unknown_size"`, `"no_batch_impl"`, `"no_streaming_impl"`
    - _Requirements: 7.2, 7.3, 7.4, 7.5, 7.6, 7.8_

  - [ ]* 5.3 Write unit tests for telemetry emission
    - Verify counter increments for each reason label
    - Verify gauge is set on executor construction
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.8_

- [ ] 6. Integration testing and backward compatibility
  - [ ] 6.1 Write integration tests for hybrid pipeline execution in `crates/deriva-compute/tests/`
    - Test pipeline with mixed small/large inputs producing correct results
    - Test that `batch_to_stream` adapter correctly connects BatchStage → StreamingStage
    - Test multi-node pipeline with independent mode selections per node
    - _Requirements: 6.1, 6.2, 6.3_

  - [ ]* 6.2 Write property test for pipeline execution correctness (Property 7)
    - **Property 7: Pipeline execution preserves computational correctness**
    - Generate pipelines with varying input sizes, execute with size-aware selection and compare output to all-streaming execution
    - **Validates: Requirements 6.2, 6.3**

  - [ ] 6.3 Write backward compatibility tests for threshold=0 in `crates/deriva-compute/tests/`
    - Verify `streaming_threshold = 0` produces identical stage selections to pre-§2.9 always-streaming logic
    - Verify batch-only functions still select BatchStage even with threshold=0
    - _Requirements: 9.1, 9.2, 9.3_

- [ ] 7. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- All implementation is in Rust, targeting the `crates/deriva-compute` crate
- The existing `batch_to_stream` adapter from §2.7 handles BatchStage→StreamingStage connections without modification
- The `FunctionRegistry` API is unchanged; mode selection uses existing `get()`, `get_streaming()`, and `has_streaming()` methods

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "5.1"] },
    { "id": 1, "tasks": ["1.2", "2.1"] },
    { "id": 2, "tasks": ["2.2", "2.3"] },
    { "id": 3, "tasks": ["2.4", "2.5", "3.1"] },
    { "id": 4, "tasks": ["3.2", "5.2"] },
    { "id": 5, "tasks": ["3.3", "3.4", "5.3"] },
    { "id": 6, "tasks": ["6.1", "6.3"] },
    { "id": 7, "tasks": ["6.2"] }
  ]
}
```
