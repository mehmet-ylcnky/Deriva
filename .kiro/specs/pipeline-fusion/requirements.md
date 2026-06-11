# Requirements Document

## Introduction

Phase 2.11 of the Computation-Addressed Distributed File System (Deriva/CAS-DFS) adds pipeline fusion — an optimization that collapses adjacent compatible streaming stages into a single fused task to eliminate inter-stage channel overhead. For chains of cheap byte transforms (Identity, Uppercase, XOR), channel hops constitute 5–50% of per-chunk cost. Fusing N stages into 1 eliminates N-1 channel hops, reducing task count, channel buffer memory, and Tokio scheduling overhead while preserving byte-for-byte output equivalence with the unfused pipeline.

## Glossary

- **Fusible_Stage**: A streaming compute function that is a pure, stateless, single-input, single-output, chunk-by-chunk transform producing exactly one output chunk per input chunk with no buffering or size change.
- **FusedMapStage**: A composite streaming stage that holds a vector of fusible transform functions and applies them sequentially per chunk within a single Tokio task, eliminating intermediate channels.
- **ChunkTransform**: The logical per-chunk operation performed by a fusible stage: a synchronous function from input Bytes and parameters to output Bytes.
- **Fusion_Optimizer**: A construction-time pass that scans the pipeline node list in topological order, identifies maximal runs of adjacent fusible stages, and replaces each run with a single FusedMapStage node.
- **Fusible_Run**: A maximal contiguous sequence of two or more adjacent fusible stages where each stage has a single input from the immediately preceding node and that preceding node has fan-out of exactly one.
- **Fan_Out**: The number of downstream nodes that read from a given node's output. A fan-out greater than one prevents fusion because the intermediate result is consumed by multiple consumers.
- **Channel_Hop**: A single send/receive pair across a tokio mpsc channel between two pipeline stages, costing approximately 100ns per chunk.
- **StreamingComputeFunction**: The existing async trait for streaming compute functions (defined in §2.7).
- **StreamPipeline**: The existing builder that constructs and executes a DAG of streaming stages (defined in §2.7).
- **PipelineConfig**: The existing configuration struct for pipeline execution parameters (defined in §2.7).
- **PipelineNode**: The existing enum of node types in a StreamPipeline (defined in §2.7).
- **StreamChunk**: The existing enum representing a unit of data in a streaming pipeline: `Data(Bytes)`, `End`, or `Error(DerivaError)` (defined in §2.7).

## Requirements

### Requirement 1: Fusibility Metadata on StreamingComputeFunction

**User Story:** As a system developer, I want each streaming compute function to declare whether it is fusible, so that the optimizer can identify which stages are safe to merge without changing pipeline semantics.

#### Acceptance Criteria

1. THE StreamingComputeFunction trait SHALL provide an `is_fusible` method returning `bool` with a default implementation returning `false`.
2. WHEN a streaming compute function is a pure, stateless, single-input, single-output, chunk-by-chunk transform that produces exactly one output chunk per input chunk with no internal buffering, THE function's `is_fusible` method SHALL return `true`.
3. THE nine built-in map functions (Identity, Uppercase, Lowercase, Reverse, Base64Encode, Base64Decode, Xor, Compress, Decompress) SHALL return `true` from `is_fusible`.
4. THE three built-in accumulator functions (Sha256, ByteCount, Checksum) SHALL return `false` from `is_fusible`.
5. THE three built-in combiner functions (Concat, Interleave, ZipConcat) SHALL return `false` from `is_fusible`.
6. THE five built-in utility functions (ChunkResizer, Take, Skip, Repeat, TeeCount) SHALL return `false` from `is_fusible`.

### Requirement 2: FusedMapStage Composite Execution

**User Story:** As a system developer, I want a composite stage that applies a chain of fusible transforms sequentially per chunk within a single task, so that intermediate channel hops are eliminated while preserving correct transform ordering.

#### Acceptance Criteria

1. THE FusedMapStage SHALL accept a vector of two or more fusible streaming compute functions with their associated parameters.
2. WHEN a `Data` chunk is received, THE FusedMapStage SHALL apply each contained transform function sequentially in vector order to the chunk, passing the output of each transform as input to the next.
3. WHEN all transforms in the chain complete successfully for a chunk, THE FusedMapStage SHALL emit exactly one `Data` chunk containing the final transformed bytes.
4. WHEN the input stream yields an `End` chunk, THE FusedMapStage SHALL forward `End` to the output stream.
5. WHEN the input stream yields an `Error` chunk, THE FusedMapStage SHALL forward the `Error` to the output stream without applying transforms.
6. IF any transform function within the fused chain returns an error during chunk processing, THEN THE FusedMapStage SHALL emit a `StreamChunk::Error` to the output and terminate processing.
7. THE FusedMapStage SHALL execute within a single Tokio task with a single input receiver and a single output sender.
8. THE FusedMapStage SHALL return `false` from `is_fusible` to prevent recursive fusion of already-fused stages.

### Requirement 3: Fusion Optimizer Pass

**User Story:** As a system developer, I want an optimizer pass that automatically detects and merges fusible chains at pipeline construction time, so that the performance benefit is transparent to pipeline callers without manual stage composition.

#### Acceptance Criteria

1. THE Fusion_Optimizer SHALL scan the pipeline node list in topological order and identify maximal fusible runs.
2. THE Fusion_Optimizer SHALL determine a stage is a fusion candidate only when all of the following hold: the stage's function reports `is_fusible` as true, the stage has exactly one input, the input is the immediately preceding node, and the preceding node's fan-out is exactly one.
3. WHEN a fusible run of two or more consecutive stages is identified, THE Fusion_Optimizer SHALL replace the run with a single FusedMapStage node containing the run's functions in their original order.
4. WHEN a fusible run contains only one stage, THE Fusion_Optimizer SHALL leave the stage unchanged (fusion requires at least two stages).
5. AFTER merging fusible runs, THE Fusion_Optimizer SHALL renumber all input indices in remaining nodes to account for removed nodes.
6. THE Fusion_Optimizer SHALL execute in O(N) time where N is the number of pipeline nodes.
7. THE Fusion_Optimizer SHALL produce a pipeline that is output-equivalent to the unfused pipeline for all inputs.

### Requirement 4: Fusion Configuration

**User Story:** As a system developer, I want a configuration flag to enable or disable pipeline fusion, so that I can bypass fusion for debugging or correctness verification without code changes.

#### Acceptance Criteria

1. THE PipelineConfig SHALL provide an `enable_fusion` field of type `bool` with a default value of `true`.
2. WHEN `enable_fusion` is `true`, THE StreamPipeline execution SHALL run the Fusion_Optimizer pass before spawning tasks.
3. WHEN `enable_fusion` is `false`, THE StreamPipeline execution SHALL skip the Fusion_Optimizer pass entirely, executing the pipeline identically to §2.7 unfused behavior.
4. THE fusion configuration SHALL have no effect on pipeline correctness: the output SHALL be byte-for-byte identical regardless of the `enable_fusion` setting.

### Requirement 5: Fusion Correctness — Output Equivalence

**User Story:** As a system developer, I want fused pipelines to produce byte-for-byte identical output to unfused pipelines, so that fusion is a safe optimization with zero semantic impact.

#### Acceptance Criteria

1. FOR ALL valid inputs, THE FusedMapStage containing transforms [T1, T2, ..., TN] SHALL produce output identical to executing T1, T2, ..., TN as separate stages connected by channels.
2. FOR ALL valid inputs, THE fused pipeline SHALL produce the same stream of `Data` chunks in the same order and with the same byte content as the unfused pipeline.
3. WHEN a non-fusible stage separates two fusible groups, THE Fusion_Optimizer SHALL create two independent FusedMapStage nodes with the non-fusible stage between them, preserving the original data flow.
4. WHEN fan-out on an intermediate node is greater than one, THE Fusion_Optimizer SHALL not absorb that node's downstream into a fused group, preserving the intermediate output for all consumers.

### Requirement 6: Fusion with Heterogeneous Pipelines

**User Story:** As a system developer, I want fusion to operate correctly alongside non-fusible stages, batch stages, and multi-input combiners, so that mixed pipelines benefit from partial fusion where applicable.

#### Acceptance Criteria

1. WHEN a pipeline contains a mix of fusible and non-fusible streaming stages, THE Fusion_Optimizer SHALL fuse only the maximal fusible runs, leaving non-fusible stages intact.
2. WHEN a BatchStage appears in a pipeline, THE Fusion_Optimizer SHALL treat it as a fusion boundary (batch stages are never fusible).
3. WHEN a multi-input combiner follows a fusible chain, THE Fusion_Optimizer SHALL fuse the preceding chain and leave the combiner as a separate stage.
4. WHEN a ChunkResizer (non-fusible utility) appears between two fusible stages, THE Fusion_Optimizer SHALL produce two separate fused groups with the ChunkResizer between them.
5. WHEN a fusible chain feeds into a multi-input combiner, THE fusible chain preceding the combiner input SHALL be fused independently.

### Requirement 7: Fused Stage Error Propagation and Cancellation

**User Story:** As a system developer, I want fused stages to propagate errors and handle cancellation correctly, so that the fused pipeline maintains the same failure semantics as the unfused pipeline.

#### Acceptance Criteria

1. WHEN the upstream source emits a `StreamChunk::Error`, THE FusedMapStage SHALL forward the error to the output without attempting to apply transforms.
2. IF a transform within the fused chain produces an error on a specific chunk, THEN THE FusedMapStage SHALL emit the error immediately and stop processing subsequent chunks.
3. WHEN the output receiver is dropped (downstream cancellation), THE FusedMapStage task SHALL detect the send failure and terminate, dropping the input receiver to propagate cancellation upstream.
4. WHEN the input channel closes without sending `End` or `Error`, THE FusedMapStage SHALL treat this as a premature closure and terminate.

### Requirement 8: Per-Stage Parameter Preservation in Fusion

**User Story:** As a system developer, I want each fused transform to retain its own parameter set, so that parameterized functions like XorCipher use their individual keys even when fused with other stages.

#### Acceptance Criteria

1. THE FusedMapStage SHALL store a separate parameter HashMap for each contained transform function.
2. WHEN applying transform function Ti within the fused chain, THE FusedMapStage SHALL pass Ti's associated parameters to Ti.
3. WHEN two XorCipher stages with different keys are fused (key=0xAA, key=0x55), THE FusedMapStage SHALL apply the first XOR with key 0xAA and the second XOR with key 0x55, producing output equivalent to XOR with 0xFF.

### Requirement 9: Fusion Observability

**User Story:** As a system developer, I want observability into fusion decisions, so that I can verify fusion is being applied and diagnose performance characteristics of fused pipelines.

#### Acceptance Criteria

1. WHEN the Fusion_Optimizer merges stages, THE system SHALL emit a debug-level log message containing the original node count, the fused node count, and the number of eliminated stages.
2. WHEN fusion is applied, THE system SHALL record a metric indicating the number of stages eliminated by fusion.
3. THE FusedMapStage SHALL report pipeline metrics (chunks processed, bytes processed) as a single unit for the entire fused group.

### Requirement 10: Fusion Performance Characteristics

**User Story:** As a system developer, I want pipeline fusion to reduce channel overhead proportional to the number of eliminated hops, so that chains of cheap transforms execute measurably faster.

#### Acceptance Criteria

1. FOR a pipeline of N fusible stages, THE fused pipeline SHALL use exactly 1 channel hop per chunk instead of N channel hops per chunk.
2. FOR a pipeline of N fusible stages, THE fused pipeline SHALL spawn 1 Tokio task for the fused group instead of N tasks.
3. THE Fusion_Optimizer pass SHALL add negligible construction-time overhead (O(N) scan of pipeline nodes).
4. THE fused execution overhead (sequential transform application) SHALL not exceed 1.05x the compute cost of applying the same transforms in separate tasks for the same input.
