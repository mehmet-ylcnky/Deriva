# Requirements Document

## Introduction

Phase 2.7 of the Computation-Addressed Distributed File System (Deriva/CAS-DFS) adds chunked streaming pipeline execution with backpressure. Data flows through compute stages as 64KB chunks via tokio mpsc channels, enabling concurrent stage execution with bounded memory. This eliminates the requirement for all inputs to be fully materialized in memory before computation begins, reducing peak memory from O(total_input_size) to O(stages × channel_capacity × chunk_size) and enabling pipeline parallelism where downstream stages begin processing before upstream stages complete.

## Glossary

- **StreamChunk**: An enum representing a single unit of data in a streaming pipeline. Variants are `Data(Bytes)` (payload), `End` (stream termination), and `Error(DerivaError)` (failure propagation).
- **StreamingComputeFunction**: An async trait for compute functions that process data incrementally via chunked input/output receivers rather than requiring full materialization.
- **StreamPipeline**: A builder that constructs a DAG of streaming stages connected by bounded mpsc channels, then executes all stages concurrently as tokio tasks.
- **PipelineConfig**: Configuration struct controlling chunk_size (default 64KB), channel_capacity (default 8), cache_intermediates flag, and memory_budget.
- **PipelineNode**: An enum of node types in a StreamPipeline: Source (leaf data), Cached (previously computed), StreamingStage (streaming function), or BatchStage (batch function wrapped as streaming).
- **FunctionRegistry**: The registry of compute functions, extended to store both batch and streaming implementations indexed by function key.
- **StreamingExecutor**: The async executor that builds a StreamPipeline from a recipe DAG by resolving topological order, classifying nodes, and connecting stages with channels.
- **Backpressure**: Flow control mechanism where bounded channel capacity causes upstream producers to suspend (await) when downstream consumers are slow, preventing unbounded memory growth.
- **Adapter**: Utility functions that bridge between batch and streaming worlds: `batch_to_stream`, `value_to_stream`, `collect_stream`, and `tee_stream`.
- **CAddr**: A content address (hash) that uniquely identifies a computation result or leaf value.
- **Recipe**: A computation definition consisting of a function identifier, input addresses, and parameters.
- **DagReader**: A synchronous trait abstracting DAG access, providing `get_inputs` and `get_recipe` methods.
- **Channel_Capacity**: The maximum number of StreamChunk items buffered in an inter-stage mpsc channel before the sender suspends (default: 8).
- **Chunk_Size**: The target size in bytes for each Data chunk flowing through the pipeline (default: 65536 bytes / 64KB).
- **Tee**: A stream splitter that clones each chunk to two independent consumer receivers.

## Requirements

### Requirement 1: StreamChunk Protocol

**User Story:** As a system developer, I want a well-defined chunk protocol for streaming data, so that all pipeline stages agree on message ordering semantics and can reliably detect stream completion or failure.

#### Acceptance Criteria

1. THE StreamChunk enum SHALL provide exactly three variants: `Data(Bytes)` for payload chunks, `End` for stream termination, and `Error(DerivaError)` for failure propagation.
2. THE StreamChunk protocol SHALL enforce the ordering invariant: zero or more `Data` chunks followed by exactly one terminal (`End` or `Error`), with no chunks sent after the terminal.
3. WHEN a StreamChunk of variant `Data` is produced, THE StreamChunk SHALL contain a non-empty Bytes payload.
4. THE StreamChunk SHALL provide accessor methods: `is_data()`, `is_end()`, `is_error()`, `into_data()`, and `data_len()`.
5. THE StreamChunk SHALL derive `Clone` and `Debug`.

### Requirement 2: StreamingComputeFunction Trait

**User Story:** As a system developer, I want an async streaming trait for compute functions, so that functions can process data incrementally without buffering entire inputs in memory.

#### Acceptance Criteria

1. THE StreamingComputeFunction trait SHALL provide an async `stream_execute` method accepting `Vec<Receiver<StreamChunk>>` (one receiver per input) and `&HashMap<String, String>` (parameters), returning `Receiver<StreamChunk>`.
2. THE StreamingComputeFunction trait SHALL provide a `supports_streaming` method returning `bool` (default: `true`).
3. THE StreamingComputeFunction trait SHALL provide a `preferred_chunk_size` method returning `usize` (default: 65536 bytes).
4. THE StreamingComputeFunction trait SHALL provide a `channel_capacity` method returning `usize` (default: 8).
5. THE StreamingComputeFunction trait SHALL require `Send + Sync` bounds so that implementations can be shared across tokio tasks.
6. WHEN `stream_execute` is called, THE StreamingComputeFunction SHALL consume all input receivers and produce an output receiver that follows the StreamChunk protocol.
7. IF any input receiver yields an `Error` chunk, THEN THE StreamingComputeFunction SHALL propagate the error to the output receiver and terminate.

### Requirement 3: Batch-to-Stream Adapters

**User Story:** As a system developer, I want adapter functions that bridge batch and streaming execution, so that the pipeline can mix batch and streaming stages transparently.

#### Acceptance Criteria

1. THE `batch_to_stream` adapter SHALL accept a `Bytes` value and a `chunk_size`, split the value into chunks of at most `chunk_size` bytes, send each as `Data`, and terminate with `End`.
2. THE `value_to_stream` adapter SHALL accept a `Bytes` value, a `chunk_size`, and a `capacity`, create a bounded channel of the specified capacity, and stream the value as chunks followed by `End`.
3. THE `collect_stream` adapter SHALL consume a `Receiver<StreamChunk>`, accumulate all `Data` chunks, and return the concatenated `Bytes` on receiving `End`.
4. IF `collect_stream` receives an `Error` chunk, THEN THE `collect_stream` adapter SHALL return the contained `DerivaError`.
5. IF `collect_stream` receives a channel closure (None) without an `End` marker, THEN THE `collect_stream` adapter SHALL return an error indicating the stream closed prematurely.
6. THE `tee_stream` adapter SHALL accept a `Receiver<StreamChunk>` and produce two independent `Receiver<StreamChunk>` outputs, each receiving a clone of every chunk from the original stream.
7. WHEN the original stream in `tee_stream` yields a terminal chunk (`End` or `Error`), THE `tee_stream` adapter SHALL forward the terminal to both output receivers.

### Requirement 4: StreamPipeline Builder

**User Story:** As a system developer, I want a pipeline builder that constructs and executes a DAG of streaming stages, so that multi-stage computations run concurrently with backpressure-controlled memory usage.

#### Acceptance Criteria

1. THE StreamPipeline SHALL support four node types: Source (leaf data), Cached (previously materialized data), StreamingStage (streaming function), and BatchStage (batch function).
2. THE StreamPipeline SHALL accept a PipelineConfig specifying `chunk_size` (default 65536), `channel_capacity` (default 8), `cache_intermediates` (default true), and `memory_budget` (default 0, meaning unlimited).
3. WHEN `execute` is called, THE StreamPipeline SHALL spawn a tokio task for each stage, connecting stages via bounded mpsc channels of size `channel_capacity`.
4. WHEN a StreamingStage is executed, THE StreamPipeline SHALL pass the input receivers from upstream nodes to the stage's `stream_execute` method.
5. WHEN a BatchStage is executed, THE StreamPipeline SHALL collect all input streams to full Bytes values, execute the batch function synchronously, and wrap the result as a chunked stream via `batch_to_stream`.
6. WHEN a Source or Cached node is executed, THE StreamPipeline SHALL stream the stored Bytes data in chunks of `chunk_size` using `value_to_stream`.
7. THE StreamPipeline SHALL return the output receiver of the final (last-added) node.
8. IF the pipeline contains zero nodes, THEN THE StreamPipeline SHALL return an error.

### Requirement 5: Backpressure via Bounded Channels

**User Story:** As a system developer, I want bounded inter-stage channels that suspend fast producers when downstream is slow, so that pipeline memory usage remains predictable and bounded.

#### Acceptance Criteria

1. THE StreamPipeline SHALL use tokio mpsc channels with capacity equal to `PipelineConfig::channel_capacity` (default 8) for all inter-stage communication.
2. WHEN an inter-stage channel is full, THE producing stage's send operation SHALL suspend (async await) until the consumer drains at least one slot.
3. THE total memory consumption of a pipeline with D streaming stages SHALL be bounded by D × channel_capacity × chunk_size bytes for in-flight chunks.
4. WHEN a downstream receiver is dropped (consumer cancelled), THE upstream sender's send operation SHALL return an error, causing the upstream stage to terminate gracefully.

### Requirement 6: Streaming Executor (DAG to Pipeline)

**User Story:** As a system developer, I want an executor that automatically builds a streaming pipeline from a recipe DAG, so that the gRPC layer can invoke streaming materialization without manually constructing pipelines.

#### Acceptance Criteria

1. THE StreamingExecutor SHALL accept a CAddr and resolve the recipe DAG in topological order using the DagReader.
2. WHEN building the pipeline, THE StreamingExecutor SHALL classify each node: cached values become Cached nodes, leaf values become Source nodes, recipes with streaming functions become StreamingStage nodes, and recipes with batch-only functions become BatchStage nodes.
3. THE StreamingExecutor SHALL respect cache state: if a computed value is already cached, THE StreamingExecutor SHALL use it as a Cached source node rather than recomputing.
4. IF a function referenced by a recipe is not found in the FunctionRegistry (neither batch nor streaming), THEN THE StreamingExecutor SHALL return a `FunctionNotFound` error.
5. THE StreamingExecutor SHALL guarantee that all input indices for a stage refer to nodes already added to the pipeline (enforced by topological ordering).

### Requirement 7: FunctionRegistry Streaming Extension

**User Story:** As a system developer, I want the FunctionRegistry to store streaming function implementations alongside batch implementations, so that the executor can select the appropriate execution mode per function.

#### Acceptance Criteria

1. THE FunctionRegistry SHALL provide a `register_streaming` method that accepts a function name, version, and `Box<dyn StreamingComputeFunction>`.
2. THE FunctionRegistry SHALL provide a `get_streaming` method that returns the streaming implementation for a given function key, or None if no streaming version is registered.
3. THE FunctionRegistry SHALL provide a `has_streaming` method that returns true if a streaming implementation exists for a given function key.
4. THE FunctionRegistry SHALL store batch and streaming implementations in separate internal maps, allowing a function to have both a batch and a streaming implementation.
5. WHEN both a streaming and batch implementation exist for a function, THE system SHALL prefer the streaming implementation in pipeline execution.

### Requirement 8: Built-in Streaming Transform Functions

**User Story:** As a system developer, I want a library of built-in streaming transform functions, so that common data transformations can operate chunk-by-chunk without full input buffering.

#### Acceptance Criteria

1. THE system SHALL provide nine single-input chunk-by-chunk transform functions: Identity, Uppercase, Lowercase, Reverse, Base64Encode, Base64Decode, Xor, Compress, and Decompress.
2. WHEN a transform function receives a `Data` chunk, THE transform function SHALL process the chunk independently and emit the transformed chunk to the output.
3. THE Identity function SHALL pass input chunks through to the output unchanged.
4. THE Uppercase function SHALL convert each byte in a chunk to its ASCII uppercase equivalent.
5. THE Lowercase function SHALL convert each byte in a chunk to its ASCII lowercase equivalent.
6. THE Reverse function SHALL reverse the byte order within each individual chunk.
7. THE Base64Encode function SHALL encode each chunk independently using standard Base64.
8. THE Base64Decode function SHALL decode each chunk independently from standard Base64, emitting an `Error` chunk if the input is invalid Base64.
9. THE Xor function SHALL XOR each byte in a chunk with the key byte specified in `params["key"]`.
10. THE Compress function SHALL zlib-compress each chunk independently.
11. THE Decompress function SHALL zlib-decompress each chunk independently, emitting an `Error` chunk if the input is invalid compressed data.

### Requirement 9: Built-in Streaming Accumulator Functions

**User Story:** As a system developer, I want streaming accumulator functions that consume all input chunks and emit a single summary result, so that hashing, counting, and checksumming benefit from streaming input without full-value buffering.

#### Acceptance Criteria

1. THE system SHALL provide three accumulator functions: Sha256, ByteCount, and Checksum (CRC32).
2. THE Sha256 function SHALL feed each input `Data` chunk into an incremental SHA-256 hasher and emit the 32-byte digest as a single `Data` chunk upon receiving `End`.
3. THE ByteCount function SHALL count the total bytes across all input `Data` chunks and emit the count as an 8-byte big-endian u64 upon receiving `End`.
4. THE Checksum function SHALL compute a rolling CRC32 across all input `Data` chunks and emit the 4-byte checksum upon receiving `End`.
5. WHEN an accumulator function receives an `Error` chunk, THE accumulator function SHALL propagate the error to the output without emitting a result.

### Requirement 10: Built-in Streaming Combiner Functions

**User Story:** As a system developer, I want streaming combiner functions that merge multiple input streams into one output stream, so that multi-input recipes can operate in streaming mode.

#### Acceptance Criteria

1. THE system SHALL provide three multi-input combiner functions: Concat, Interleave, and ZipConcat.
2. THE Concat function SHALL consume inputs sequentially: all `Data` chunks from input 0 are emitted first, then all `Data` chunks from input 1, and so on, followed by a single `End`.
3. THE Interleave function SHALL alternate chunks across inputs in round-robin order (one chunk from input 0, one from input 1, etc.), skipping exhausted inputs, followed by `End` when all inputs are exhausted.
4. THE ZipConcat function SHALL read one chunk from each input in order, concatenate them into a single output chunk, and repeat until all inputs are exhausted, followed by `End`.
5. IF any input stream in a combiner function yields an `Error` chunk, THEN THE combiner function SHALL propagate the error to the output and terminate.

### Requirement 11: Built-in Streaming Utility Functions

**User Story:** As a system developer, I want streaming utility functions for chunk manipulation, so that pipelines can resize, limit, skip, repeat, or count chunks flowing through.

#### Acceptance Criteria

1. THE system SHALL provide five utility functions: ChunkResizer, Take, Skip, Repeat, and TeeCount.
2. THE ChunkResizer function SHALL re-chunk the input stream into chunks of a target size specified in `params["target_size"]`, buffering partial data as needed.
3. THE Take function SHALL forward at most N bytes from the input stream (where N is specified in `params["bytes"]`), then emit `End` and discard remaining input.
4. THE Skip function SHALL discard the first N bytes from the input stream (where N is specified in `params["bytes"]`), then forward remaining chunks to the output.
5. THE Repeat function SHALL replay the collected input stream a number of times specified in `params["count"]`, emitting all chunks for each repetition before the final `End`.
6. THE TeeCount function SHALL forward all input chunks unchanged to the output and also count the total number of chunks processed, emitting the count as a final `Data` chunk (8-byte big-endian u64) before `End`.

### Requirement 12: gRPC Integration with Automatic Mode Selection

**User Story:** As a system developer, I want the gRPC `get()` RPC to automatically select streaming vs batch execution based on function capability, so that clients benefit from streaming without API changes.

#### Acceptance Criteria

1. WHEN a `get()` RPC is received, THE service SHALL check if the root recipe's function has a streaming implementation via `registry.has_streaming()`.
2. WHEN a streaming implementation exists for the root function, THE service SHALL build and execute a StreamPipeline, tee the output stream to both the gRPC response and a background cache task.
3. WHEN no streaming implementation exists for the root function, THE service SHALL execute the existing batch materialization path unchanged.
4. THE background cache task SHALL collect the tee'd stream via `collect_stream` and cache the full result upon completion.
5. IF the streaming pipeline yields an `Error` chunk, THEN THE service SHALL propagate the error as a gRPC Status error to the client.
6. THE gRPC streaming response SHALL deliver chunks to the client as they are produced by the pipeline, without waiting for full materialization to complete.

### Requirement 13: Cache-After-Collect Strategy

**User Story:** As a system developer, I want streaming results to be cached as full Bytes values after stream completion, so that subsequent requests for the same address return immediately from cache without re-executing the pipeline.

#### Acceptance Criteria

1. WHEN a streaming pipeline completes successfully, THE system SHALL collect all output chunks into a single Bytes value and cache it under the recipe's CAddr.
2. THE caching strategy SHALL be compatible with the existing cache design (full Bytes keyed by CAddr).
3. WHEN a subsequent request arrives for a CAddr that is already cached, THE system SHALL return the cached value directly without building or executing a pipeline.
4. IF the background cache collection fails (stream error during tee), THEN THE system SHALL log the failure and not cache a partial result.

### Requirement 14: Streaming Observability Metrics

**User Story:** As a system developer, I want Prometheus metrics for streaming pipeline execution, so that pipeline throughput, latency, and resource usage can be monitored in production.

#### Acceptance Criteria

1. THE system SHALL expose a `deriva_stream_pipelines_total` counter metric tracking the total number of pipeline executions.
2. THE system SHALL expose a `deriva_stream_chunks_total` counter metric tracking the total number of chunks processed across all pipelines.
3. THE system SHALL expose a `deriva_stream_bytes_total` counter metric tracking the total number of bytes processed across all pipelines.
4. THE system SHALL expose a `deriva_stream_pipeline_duration_seconds` histogram metric tracking end-to-end pipeline execution duration.
5. WHEN a pipeline executes, THE metered output stream SHALL increment chunk and byte counters for each `Data` chunk that flows through the pipeline output.

### Requirement 15: Graceful Cancellation and Error Propagation

**User Story:** As a system developer, I want streaming pipelines to propagate cancellation and errors through the channel graph, so that resources are released promptly when a client disconnects or a stage fails.

#### Acceptance Criteria

1. WHEN a downstream receiver is dropped (client disconnects), THE upstream sender's next send operation SHALL return an error, causing the upstream task to terminate.
2. WHEN a stage task terminates due to a send error (receiver dropped), THE stage SHALL drop its input receivers, propagating cancellation upstream through the channel graph.
3. WHEN a streaming function emits an `Error` chunk, THE downstream consumer SHALL stop processing and propagate the error or terminate.
4. IF a channel is closed without sending an `End` or `Error` terminal, THEN THE downstream consumer SHALL treat this as a premature closure error.

### Requirement 16: Hybrid Pipeline Execution

**User Story:** As a system developer, I want pipelines to support mixed streaming and batch stages, so that functions without streaming implementations can participate in streaming pipelines without requiring all functions to implement the streaming trait.

#### Acceptance Criteria

1. WHEN a BatchStage is encountered in a pipeline, THE StreamPipeline SHALL collect all input streams to full Bytes values before executing the batch function.
2. WHEN a BatchStage completes execution, THE StreamPipeline SHALL wrap the batch result as a chunked stream using `batch_to_stream` with the configured `chunk_size`.
3. THE StreamPipeline SHALL support any interleaving of StreamingStage and BatchStage nodes within the same pipeline.
4. WHEN a StreamingStage follows a BatchStage, THE StreamingStage SHALL receive the batch result as a normally-chunked stream indistinguishable from a native streaming source.
