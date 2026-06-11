# Requirements Document

## Introduction

Phase 2.9 of the Computation-Addressed Distributed File System (Deriva/CAS-DFS) adds intelligent per-node selection between batch and streaming execution modes based on total input size. Below a configurable threshold (default 3MB), batch execution is preferred to avoid streaming overhead (task spawning, channel allocation, chunk framing). Above the threshold, streaming is preferred for bounded memory and pipeline parallelism. The selection is per-node within a pipeline, enabling hybrid pipelines where some stages run as batch and others as streaming within the same execution. This eliminates the 2.4–9.3× overhead that small inputs (75% of typical workload) currently pay under always-streaming execution, yielding an estimated 3× improvement for the median request with O(1) decision overhead per pipeline node.

## Glossary

- **StreamingExecutor**: The async executor that builds a StreamPipeline from a recipe DAG by resolving topological order, classifying nodes, and connecting stages with channels. Extended in this phase to perform size-aware mode selection.
- **PipelineConfig**: Configuration struct controlling chunk_size, channel_capacity, cache_intermediates, memory_budget, and the new streaming_threshold field.
- **StreamPipeline**: A builder that constructs a DAG of pipeline nodes (Source, Cached, StreamingStage, BatchStage) connected by bounded mpsc channels, then executes all stages concurrently as tokio tasks.
- **PipelineNode**: An enum of node types in a StreamPipeline: Source (leaf data), Cached (previously computed), StreamingStage (streaming function), or BatchStage (batch function wrapped as streaming).
- **FunctionRegistry**: The registry of compute functions storing both batch and streaming implementations indexed by function key.
- **Streaming_Threshold**: A per-pipeline configuration value in bytes. Nodes whose total input size is below this value prefer batch execution; nodes at or above prefer streaming. Default: 3145728 bytes (3MB).
- **Mode_Selection**: The per-node decision process that determines whether a pipeline node is constructed as a StreamingStage or BatchStage based on input size and implementation availability.
- **Fallback_Chain**: The logic that selects an alternate execution mode when the preferred mode's implementation does not exist for a given function.
- **Node_Data_Size**: A query method on StreamPipeline that returns the known data size for Source and Cached nodes, or None for computed nodes (StreamingStage, BatchStage).
- **Total_Input_Size**: A query method that sums the known data sizes of all input nodes for a given stage, returning None if any input has unknown size.
- **Batch_To_Stream**: An adapter function from §2.7 that wraps a batch result (Bytes) as a chunked stream, enabling BatchStage outputs to feed into downstream StreamingStage nodes.
- **CAddr**: A content address (hash) that uniquely identifies a computation result or leaf value.
- **MODE_SELECTION_Counter**: A Prometheus IntCounterVec metric tracking mode selection decisions by mode and reason labels.
- **STREAMING_THRESHOLD_Gauge**: A Prometheus IntGauge metric exposing the currently configured streaming threshold in bytes.

## Requirements

### Requirement 1: Streaming Threshold Configuration

**User Story:** As a pipeline operator, I want to configure the input size threshold that determines when batch execution is preferred over streaming, so that I can tune mode selection to match my workload characteristics.

#### Acceptance Criteria

1. THE PipelineConfig SHALL provide a `streaming_threshold` field of type `usize` with a default value of 3145728 bytes (3MB).
2. WHEN `streaming_threshold` is set to 0, THE Mode_Selection logic SHALL prefer streaming for all nodes regardless of input size.
3. WHEN `streaming_threshold` is set to `usize::MAX`, THE Mode_Selection logic SHALL prefer batch for all nodes regardless of input size.
4. THE PipelineConfig SHALL accept any `usize` value for `streaming_threshold` without additional validation.
5. THE `DEFAULT_STREAMING_THRESHOLD` constant SHALL be defined as `3 * 1024 * 1024` (3145728 bytes).

### Requirement 2: Node Data Size Query

**User Story:** As a system developer, I want to query the known data size of pipeline nodes, so that the mode selector can determine total input size for each stage.

#### Acceptance Criteria

1. WHEN `node_data_size` is called on a Source node, THE StreamPipeline SHALL return `Some(data.len())` where `data` is the stored Bytes value.
2. WHEN `node_data_size` is called on a Cached node, THE StreamPipeline SHALL return `Some(data.len())` where `data` is the stored Bytes value.
3. WHEN `node_data_size` is called on a StreamingStage node, THE StreamPipeline SHALL return `None`.
4. WHEN `node_data_size` is called on a BatchStage node, THE StreamPipeline SHALL return `None`.

### Requirement 3: Total Input Size Calculation

**User Story:** As a system developer, I want to calculate the total known input size for a set of input node indices, so that the mode selector can make threshold comparisons.

#### Acceptance Criteria

1. WHEN `total_input_size` is called with a set of input indices where all referenced nodes have known sizes, THE StreamPipeline SHALL return `Some(sum)` where sum is the total of all individual node data sizes.
2. WHEN `total_input_size` is called with a set of input indices where any referenced node has unknown size (returns None from `node_data_size`), THE StreamPipeline SHALL return `None`.
3. WHEN `total_input_size` is called with an empty set of input indices, THE StreamPipeline SHALL return `Some(0)`.

### Requirement 4: Per-Node Mode Selection Algorithm

**User Story:** As a system developer, I want the executor to select batch or streaming mode independently for each pipeline node based on its input size, so that small-input stages avoid streaming overhead while large-input stages retain pipeline parallelism.

#### Acceptance Criteria

1. WHEN the total input size for a node is `None` (unknown), THE Mode_Selection logic SHALL prefer streaming as the safe default.
2. WHEN the total input size for a node is `Some(0)` (empty), THE Mode_Selection logic SHALL prefer streaming.
3. WHEN the total input size for a node is `Some(size)` where `size` is greater than or equal to `streaming_threshold`, THE Mode_Selection logic SHALL prefer streaming.
4. WHEN the total input size for a node is `Some(size)` where `size` is less than `streaming_threshold` and greater than 0, THE Mode_Selection logic SHALL prefer batch.
5. THE Mode_Selection logic SHALL evaluate each node independently, allowing a single pipeline to contain both StreamingStage and BatchStage nodes.

### Requirement 5: Implementation Fallback Chain

**User Story:** As a system developer, I want the mode selector to fall back to an alternate execution mode when the preferred mode's implementation is unavailable, so that pipelines execute correctly regardless of which function implementations are registered.

#### Acceptance Criteria

1. WHEN batch mode is preferred and a batch implementation exists in the FunctionRegistry, THE Mode_Selection logic SHALL select BatchStage.
2. WHEN batch mode is preferred and no batch implementation exists but a streaming implementation exists, THE Mode_Selection logic SHALL select StreamingStage as a fallback.
3. WHEN streaming mode is preferred and a streaming implementation exists in the FunctionRegistry, THE Mode_Selection logic SHALL select StreamingStage.
4. WHEN streaming mode is preferred and no streaming implementation exists but a batch implementation exists, THE Mode_Selection logic SHALL select BatchStage as a fallback.
5. IF neither a batch nor a streaming implementation exists for a function, THEN THE Mode_Selection logic SHALL return a `FunctionNotFound` error.

### Requirement 6: Hybrid Pipeline Compatibility

**User Story:** As a system developer, I want size-aware mode selection to produce hybrid pipelines that mix batch and streaming stages, so that existing §2.7 hybrid execution infrastructure handles the mixed-mode output correctly.

#### Acceptance Criteria

1. WHEN a BatchStage is followed by a StreamingStage in the pipeline, THE StreamPipeline SHALL connect them using the `batch_to_stream` adapter from §2.7.
2. THE size-aware mode selection SHALL produce valid pipelines that the existing StreamPipeline execution logic can execute without modification.
3. WHEN multiple nodes in a pipeline have different mode selections (some batch, some streaming), THE StreamPipeline SHALL execute each node according to its assigned mode.

### Requirement 7: Mode Selection Telemetry

**User Story:** As a pipeline operator, I want Prometheus metrics tracking mode selection decisions, so that I can verify the threshold is effective and identify functions that would benefit from additional implementations.

#### Acceptance Criteria

1. THE system SHALL expose a `deriva_mode_selection_total` IntCounterVec metric with labels `mode` and `reason`.
2. WHEN a node is selected as batch due to input size below threshold, THE system SHALL increment the counter with labels `{mode="batch", reason="below_threshold"}`.
3. WHEN a node is selected as streaming due to input size at or above threshold, THE system SHALL increment the counter with labels `{mode="streaming", reason="above_threshold"}`.
4. WHEN a node is selected as streaming due to unknown input size, THE system SHALL increment the counter with labels `{mode="streaming", reason="unknown_size"}`.
5. WHEN a node falls back to streaming because no batch implementation exists, THE system SHALL increment the counter with labels `{mode="streaming", reason="no_batch_impl"}`.
6. WHEN a node falls back to batch because no streaming implementation exists, THE system SHALL increment the counter with labels `{mode="batch", reason="no_streaming_impl"}`.
7. THE system SHALL expose a `deriva_streaming_threshold_bytes` IntGauge metric reflecting the configured streaming_threshold value.
8. WHEN a StreamingExecutor is constructed, THE system SHALL set the `deriva_streaming_threshold_bytes` gauge to the configured threshold value.

### Requirement 8: Decision Overhead Bound

**User Story:** As a system developer, I want the mode selection decision to have constant-time overhead per node, so that the selection logic itself does not introduce measurable latency.

#### Acceptance Criteria

1. THE Mode_Selection logic SHALL execute in O(1) time per pipeline node, exclusive of the number of inputs which bounds the size summation to O(inputs_per_node).
2. THE `node_data_size` method SHALL execute in O(1) time by reading the length from the stored Bytes value directly.
3. THE `total_input_size` method SHALL execute in O(k) time where k is the number of input indices, performing one `node_data_size` lookup per input.

### Requirement 9: Backward Compatibility with §2.7 Behavior

**User Story:** As a system developer, I want the default configuration to remain compatible with existing §2.7 streaming behavior when explicitly requested, so that setting threshold to 0 produces identical behavior to the pre-§2.9 always-streaming logic.

#### Acceptance Criteria

1. WHEN `streaming_threshold` is set to 0, THE Mode_Selection logic SHALL select StreamingStage for every node that has a streaming implementation, matching the §2.7 behavior.
2. WHEN `streaming_threshold` is set to 0 and a function has only a batch implementation, THE Mode_Selection logic SHALL select BatchStage (matching §2.7 batch-only fallback behavior).
3. THE default `streaming_threshold` of 3MB SHALL differ from the §2.7 always-streaming behavior, representing the new optimized default.
