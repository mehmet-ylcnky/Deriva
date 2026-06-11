# Implementation Plan: Pipeline Fusion

## Overview

Pipeline Fusion is a construction-time optimizer that collapses adjacent fusible streaming stages into a single `FusedMapStage`, eliminating inter-stage channel hops. The implementation adds the `enable_fusion` config flag, creates the `FusedMapStage` composite, implements the O(N) fusion optimizer pass, integrates it into `StreamPipeline::execute()`, and adds observability metrics. All work lives in the `deriva-compute` crate.

## Tasks

- [ ] 1. Add `enable_fusion` field to PipelineConfig and verify fusibility declarations
  - [ ] 1.1 Add `enable_fusion: bool` field to `PipelineConfig` with default `true`
    - Modify `crates/deriva-compute/src/pipeline.rs` to add the field to `PipelineConfig` struct and its `Default` impl
    - Verify the field compiles and existing tests still pass
    - _Requirements: 4.1_

  - [ ] 1.2 Audit and verify `is_fusible()` returns on all streaming builtins
    - Confirm the 9 core map builtins (Identity, Uppercase, Lowercase, Reverse, Base64Encode, Base64Decode, Xor, Compress, Decompress) return `true` from `is_fusible()`
    - Confirm accumulators (Sha256, ByteCount, Checksum), combiners (Concat, Interleave, ZipConcat), and utilities (ChunkResizer, Take, Skip, Repeat, TeeCount) return `false`
    - Add `is_fusible() -> true` overrides to additional builtins listed in design (HexEncode, HexDecode, Base32Encode, Base32Decode, LineEnding, Pad, Trim, BitwiseAnd, BitwiseOr, BitwiseNot, ByteSwap, ChunkHash) in their respective files under `crates/deriva-compute/src/builtins_streaming/`
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6_

  - [ ]* 1.3 Write unit tests for `is_fusible()` declarations
    - Create test in `crates/deriva-compute/tests/` that instantiates each builtin and asserts expected `is_fusible()` value
    - Cover all 21 fusible and all non-fusible builtins
    - _Requirements: 1.3, 1.4, 1.5, 1.6_

- [ ] 2. Implement FusedMapStage composite
  - [ ] 2.1 Create `FusedMapStage` struct and `StreamingComputeFunction` impl
    - Create new file `crates/deriva-compute/src/fusion.rs`
    - Define `FusedMapStage` struct with `stages: Vec<(Arc<dyn StreamingComputeFunction>, HashMap<String, String>)>`
    - Implement `new()` with `debug_assert!(stages.len() >= 2)`
    - Implement `len()` and `stage_names()` helper methods
    - Implement `StreamingComputeFunction` for `FusedMapStage`:
      - `stream_execute`: single task loop — receive chunk, apply each transform sequentially via `stream_execute` with single-chunk synthetic input, forward result
      - `is_fusible() -> false`
      - `metric_name() -> "FusedMapStage"`
    - Handle `StreamChunk::End` forwarding, `StreamChunk::Error` passthrough, and mid-chain error propagation
    - Register module in `crates/deriva-compute/src/lib.rs`
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8_

  - [ ]* 2.2 Write unit tests for FusedMapStage basic execution
    - Test construction panics with fewer than 2 stages
    - Test single chunk through 2-3 transforms produces correct output
    - Test multi-chunk stream produces correct sequential output
    - Test `End` chunk is forwarded
    - Test input `Error` chunk is forwarded without applying transforms
    - Test mid-chain transform error emits `Error` and stops processing
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

  - [ ]* 2.3 Write property test for output equivalence (Property 1)
    - **Property 1: Output equivalence (fused vs unfused)**
    - Use `proptest` to generate random byte vectors (1–256 KB) and random subsets of fusible transforms (2–8)
    - Run fused execution (via FusedMapStage) and unfused execution (via separate stages with channels)
    - Assert byte-for-byte output identity
    - **Validates: Requirements 3.7, 4.4, 5.1, 5.2**

  - [ ]* 2.4 Write property test for per-stage parameter preservation (Property 6)
    - **Property 6: Per-stage parameter preservation**
    - Use `proptest` to generate random parameter maps for N parameterized XorCipher stages (2–6)
    - Verify fused output matches sequential application with per-stage params
    - **Validates: Requirements 8.1, 8.2, 8.3**

- [ ] 3. Implement fusion optimizer pass
  - [ ] 3.1 Implement `fuse_pipeline()` function and `FusionStats`
    - Add `fuse_pipeline(nodes: Vec<PipelineNode>) -> (Vec<PipelineNode>, FusionStats)` to `crates/deriva-compute/src/fusion.rs`
    - Define `FusionStats { original_count, fused_count, stages_eliminated, fused_groups }`
    - Implement the O(N) algorithm:
      1. Build fan-out map
      2. Scan nodes left-to-right collecting fusible runs
      3. Flush runs ≥ 2 into `FusedMapStage` nodes
      4. Renumber `input_indices` via index map
    - Handle edge cases: empty node list, single node, all-fusible, no fusible stages
    - Make `PipelineNode` accessible (adjust visibility as needed)
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 6.1, 6.2, 6.3, 6.4, 6.5_

  - [ ]* 3.2 Write unit tests for fusion optimizer
    - Test all-fusible linear chain → single FusedMapStage
    - Test single fusible stage → no fusion
    - Test mixed pipeline (fusible + non-fusible + fusible) → two fused groups
    - Test fan-out > 1 prevents fusion
    - Test BatchStage as fusion boundary
    - Test index renumbering correctness
    - Test empty pipeline → no-op
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 6.1, 6.2, 6.3, 6.4_

  - [ ]* 3.3 Write property test for optimizer maximal groups (Property 2)
    - **Property 2: Optimizer produces maximal fusible groups**
    - Use `proptest` to generate random pipeline topologies (3–30 nodes) with random fusibility flags and fan-out patterns
    - Verify post-conditions: all fusible runs merged, no invalid stages in fused groups, maximality (no two adjacent fusible stages remain unfused), singletons unchanged
    - **Validates: Requirements 3.1, 3.2, 3.3, 3.4, 5.3, 5.4, 6.1, 6.2**

  - [ ]* 3.4 Write property test for index renumbering (Property 3)
    - **Property 3: Index renumbering preserves data flow**
    - Use `proptest` to generate random pipelines (5–50 nodes)
    - Run optimizer, verify all `input_indices` are valid (0 ≤ idx < len) and logical data flow is preserved
    - **Validates: Requirements 3.5**

  - [ ]* 3.5 Write property test for stage elimination count (Property 7)
    - **Property 7: Stage elimination count**
    - Use `proptest` to generate random N (2–20), create all-fusible chain of N stages
    - Verify fused result has exactly 1 FusedMapStage node, stats show N-1 eliminated
    - **Validates: Requirements 10.1, 10.2**

- [ ] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 5. Integrate fusion into StreamPipeline::execute and add observability
  - [ ] 5.1 Integrate fusion optimizer call into `StreamPipeline::execute()`
    - Modify `crates/deriva-compute/src/pipeline.rs` `execute()` method
    - Add conditional fusion pass: if `self.config.enable_fusion`, call `fuse_pipeline(self.nodes)` and use resulting nodes
    - If `enable_fusion` is false, skip the optimizer pass entirely
    - Add `tracing::debug!` log with original count, fused count, eliminated stages, and group count
    - _Requirements: 4.2, 4.3, 4.4, 9.1_

  - [ ] 5.2 Add fusion metrics to observability
    - Add `record_fusion(stages_eliminated: usize)` function to `crates/deriva-compute/src/metrics.rs`
    - Define a counter or histogram metric for stages eliminated
    - Call `metrics::record_fusion()` from `execute()` when fusion is applied
    - _Requirements: 9.2, 9.3_

  - [ ]* 5.3 Write property test for error passthrough (Property 4)
    - **Property 4: Error passthrough from input stream**
    - Use `proptest` to generate random byte sequences with random error injection points
    - Verify errors appear in fused output stream unchanged, no transforms applied after error
    - **Validates: Requirements 2.5, 7.1**

  - [ ]* 5.4 Write property test for internal transform error propagation (Property 5)
    - **Property 5: Internal transform error propagation**
    - Use `proptest` to generate random data and random failing transform position
    - Verify error emitted immediately, no subsequent transforms applied, no further chunks processed
    - **Validates: Requirements 2.6, 7.2**

- [ ] 6. Integration tests and end-to-end validation
  - [ ] 6.1 Write integration tests for fused pipeline execution
    - Create `crates/deriva-compute/tests/pipeline_fusion.rs`
    - Test end-to-end pipeline execution with fusion enabled vs disabled, byte-compare outputs
    - Test mixed pipeline (fusible + accumulator + fusible) producing correct final result
    - Test cancellation (drop output receiver mid-stream, verify upstream cleanup)
    - Test large pipeline (20+ stages) with multiple fused groups
    - Test `enable_fusion=false` skips optimizer entirely
    - _Requirements: 4.3, 4.4, 5.1, 5.2, 5.3, 5.4, 7.1, 7.2, 7.3, 7.4, 10.1, 10.2_

  - [ ]* 6.2 Write integration tests for fusion observability
    - Verify `stages_eliminated` metric is incremented after fusion
    - Verify debug log message emitted with correct counts
    - Verify FusedMapStage reports metrics as single unit
    - _Requirements: 9.1, 9.2, 9.3_

- [ ] 7. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties defined in the design document using `proptest` crate
- Unit tests validate specific examples and edge cases
- The design uses Rust throughout — all implementation in `crates/deriva-compute/`
- `FusedMapStage` is stored as a regular `StreamingStage` in `PipelineNode` (no enum changes needed)
- The fusion optimizer modifies the node list before the existing executor loop runs, keeping the runtime path unchanged

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2"] },
    { "id": 1, "tasks": ["1.3", "2.1"] },
    { "id": 2, "tasks": ["2.2", "2.3", "2.4", "3.1"] },
    { "id": 3, "tasks": ["3.2", "3.3", "3.4", "3.5"] },
    { "id": 4, "tasks": ["5.1", "5.2"] },
    { "id": 5, "tasks": ["5.3", "5.4", "6.1"] },
    { "id": 6, "tasks": ["6.2"] }
  ]
}
```
