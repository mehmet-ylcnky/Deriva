# Implementation Plan: Streaming Materialization

## Overview

Implement Phase 2.7 chunked streaming pipeline execution with backpressure for the Deriva compute engine. Data flows through compute stages as 64KB chunks via tokio mpsc channels, enabling concurrent stage execution with bounded memory. The implementation builds from core protocol types up through adapters, pipeline builder, executor, built-in functions, gRPC integration, and observability.

## Tasks

- [ ] 1. Define StreamChunk protocol and core types
  - [ ] 1.1 Create `StreamChunk` enum in `deriva-core/src/streaming.rs`
    - Define `StreamChunk` enum with `Data(Bytes)`, `End`, and `Error(DerivaError)` variants
    - Derive `Clone` and `Debug`
    - Implement accessor methods: `is_data()`, `is_end()`, `is_error()`, `into_data()`, `data_len()`
    - Enforce non-empty Data invariant in constructors
    - Export from `deriva-core` crate root
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

  - [ ] 1.2 Create `PipelineConfig` struct in `deriva-compute/src/pipeline.rs`
    - Define `PipelineConfig` with fields: `chunk_size` (default 65536), `channel_capacity` (default 8), `cache_intermediates` (default true), `memory_budget` (default 0)
    - Implement `Default` trait with specified defaults
    - _Requirements: 4.2_

  - [ ]* 1.3 Write property tests for StreamChunk protocol (Property 1, 2)
    - **Property 1: Stream Protocol Ordering Invariant** — any output stream produces zero or more Data chunks followed by exactly one terminal
    - **Property 2: Non-Empty Data Chunks** — any Data variant has length > 0
    - **Validates: Requirements 1.2, 1.3, 2.6**

- [ ] 2. Implement StreamingComputeFunction trait and adapters
  - [ ] 2.1 Define `StreamingComputeFunction` trait in `deriva-compute/src/streaming.rs`
    - Define async trait with `stream_execute`, `supports_streaming`, `preferred_chunk_size`, `channel_capacity` methods
    - Require `Send + Sync` bounds
    - Provide default implementations for `supports_streaming` (true), `preferred_chunk_size` (65536), `channel_capacity` (8)
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [ ] 2.2 Implement batch-to-stream adapters in `deriva-compute/src/streaming.rs`
    - Implement `batch_to_stream(data: Bytes, chunk_size: usize) -> Receiver<StreamChunk>` — spawn task to split and send chunks
    - Implement `value_to_stream(data: Bytes, chunk_size: usize, capacity: usize) -> Receiver<StreamChunk>` — same with custom capacity
    - Implement `collect_stream(rx: Receiver<StreamChunk>) -> Result<Bytes, DerivaError>` — accumulate Data, return on End, error on Error or premature closure
    - Implement `tee_stream(rx: Receiver<StreamChunk>, capacity: usize) -> (Receiver, Receiver)` — clone each chunk to two outputs
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7_

  - [ ]* 2.3 Write property tests for adapter round-trips (Property 3, 4)
    - **Property 3: Adapter Round-Trip** — `collect_stream(batch_to_stream(data, chunk_size))` returns original data, each chunk ≤ chunk_size
    - **Property 4: Tee Stream Correctness** — both tee outputs collect to same Bytes as original
    - **Validates: Requirements 3.1, 3.2, 3.3, 3.6, 3.7, 4.6**

  - [ ]* 2.4 Write property test for error propagation at function level (Property 5)
    - **Property 5: Error Propagation — Function Level** — if any input yields Error, output yields Error with no subsequent Data
    - **Validates: Requirements 2.7, 9.5, 10.5**

- [ ] 3. Implement StreamPipeline builder and execution
  - [ ] 3.1 Implement `PipelineNode` enum and `StreamPipeline` struct in `deriva-compute/src/pipeline.rs`
    - Define `PipelineNode` enum with Source, Cached, StreamingStage, BatchStage variants
    - Implement `StreamPipeline` with `new(config)`, `add_node(node)`, and `execute()` methods
    - Execute spawns tokio tasks per node connected via bounded mpsc channels
    - Source/Cached nodes use `value_to_stream`; StreamingStage passes receivers to `stream_execute`; BatchStage collects inputs, runs batch, wraps via `batch_to_stream`
    - Return last node's output receiver; error on empty pipeline
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 4.7, 4.8, 5.1, 5.2, 5.3, 5.4_

  - [ ]* 3.2 Write property test for pipeline equivalence (Property 7)
    - **Property 7: Pipeline Equivalence with Batch Execution** — streaming pipeline output == batch materialization result for any valid recipe DAG
    - **Validates: Requirements 4.5, 6.1, 16.1, 16.3, 16.4**

  - [ ]* 3.3 Write property test for error propagation at pipeline level (Property 6)
    - **Property 6: Error Propagation — Pipeline Level** — if a stage emits Error, pipeline output contains Error
    - **Validates: Requirements 15.3**

  - [ ]* 3.4 Write property test for cancellation propagation (Property 19)
    - **Property 19: Cancellation Propagation** — dropping output receiver causes all upstream tasks to terminate
    - **Validates: Requirements 5.4, 15.1, 15.2**

- [ ] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 5. Extend FunctionRegistry for streaming
  - [ ] 5.1 Add streaming function map to `FunctionRegistry` in `deriva-compute/src/registry.rs`
    - Add `streaming_functions: HashMap<String, Arc<dyn StreamingComputeFunction>>` field
    - Implement `register_streaming(name, version, f)` method
    - Implement `get_streaming(key) -> Option<Arc<dyn StreamingComputeFunction>>` method
    - Implement `has_streaming(key) -> bool` method
    - Ensure batch and streaming maps are independent (function can have both)
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5_

  - [ ]* 5.2 Write property test for FunctionRegistry round-trip (Property 8)
    - **Property 8: FunctionRegistry Round-Trip** — after register_streaming, get_streaming returns Some and has_streaming returns true
    - **Validates: Requirements 7.1, 7.2, 7.3, 7.4**

- [ ] 6. Implement StreamingExecutor (DAG to Pipeline)
  - [ ] 6.1 Create `StreamingExecutor` in `deriva-compute/src/streaming_executor.rs`
    - Implement `materialize_streaming` method that resolves recipe DAG in topological order
    - Classify each node: cached → Cached, leaf → Source, streaming fn → StreamingStage, batch-only fn → BatchStage
    - Return `FunctionNotFound` error if function is missing from registry
    - Respect cache state: already-cached values become Cached source nodes
    - Guarantee topological ordering (inputs precede consumers)
    - Execute constructed pipeline and return output receiver
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

  - [ ]* 6.2 Write unit tests for StreamingExecutor
    - Test cache hit bypass (cached value returned without pipeline)
    - Test function not found error
    - Test streaming preference (both exist → streaming used)
    - Test hybrid DAG (mix of batch and streaming nodes)
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 7.5_

- [ ] 7. Implement built-in streaming transform functions
  - [ ] 7.1 Create `deriva-compute/src/builtins_streaming/transforms.rs` with 9 transform functions
    - Implement Identity, Uppercase, Lowercase, Reverse, Base64Encode, Base64Decode, Xor, Compress, Decompress
    - Each implements `StreamingComputeFunction` trait
    - Each processes Data chunks independently and propagates Error
    - Xor reads key from `params["key"]`; Base64Decode and Decompress emit Error on invalid input
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8, 8.9, 8.10, 8.11_

  - [ ]* 7.2 Write property tests for transforms (Property 9, 10, 11)
    - **Property 9: Transform Model Equivalence** — streaming transform result == batch transform on full input
    - **Property 10: XOR Involution** — xor(xor(data, key), key) == data
    - **Property 11: Compress/Decompress Round-Trip** — decompress(compress(data)) == data
    - **Validates: Requirements 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.9, 8.10, 8.11**

- [ ] 8. Implement built-in streaming accumulator functions
  - [ ] 8.1 Create `deriva-compute/src/builtins_streaming/accumulators.rs` with 3 accumulator functions
    - Implement Sha256 (incremental SHA-256, emit 32-byte digest on End)
    - Implement ByteCount (count bytes, emit 8-byte BE u64 on End)
    - Implement Checksum (rolling CRC32, emit 4-byte BE u32 on End)
    - All propagate Error chunks without emitting a result
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5_

  - [ ]* 8.2 Write property tests for accumulators (Property 12)
    - **Property 12: Accumulator Model Equivalence** — streaming digest/count/checksum == batch computation on full value regardless of chunk boundaries
    - **Validates: Requirements 9.2, 9.3, 9.4**

- [ ] 9. Implement built-in streaming combiner functions
  - [ ] 9.1 Create `deriva-compute/src/builtins_streaming/combiners.rs` with 3 combiner functions
    - Implement Concat (consume inputs sequentially, emit all Data from input 0 first, then input 1, etc.)
    - Implement Interleave (round-robin one chunk from each input, skip exhausted)
    - Implement ZipConcat (read one chunk from each, concatenate into single output chunk)
    - All propagate Error from any input and terminate
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5_

  - [ ]* 9.2 Write property tests for combiners (Property 13, 14)
    - **Property 13: Concat Sequential Correctness** — collected Concat output == concatenation of all inputs in order
    - **Property 14: Combiner Byte Preservation** — Interleave/ZipConcat output byte count == sum of input byte lengths
    - **Validates: Requirements 10.2, 10.3, 10.4**

- [ ] 10. Implement built-in streaming utility functions
  - [ ] 10.1 Create `deriva-compute/src/builtins_streaming/utilities.rs` with 5 utility functions
    - Implement ChunkResizer (re-chunk to target_size from params, buffer partial data)
    - Implement Take (forward first N bytes from params["bytes"], then End)
    - Implement Skip (discard first N bytes from params["bytes"], forward rest)
    - Implement Repeat (collect input, replay count times from params["count"])
    - Implement TeeCount (forward all chunks, emit chunk count as final 8-byte BE u64 Data before End)
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5, 11.6_

  - [ ]* 10.2 Write property tests for utilities (Property 15, 16, 17, 18)
    - **Property 15: ChunkResizer Correctness** — all output chunks except last have exactly target_size bytes; collected output == original input
    - **Property 16: Take/Skip Complementarity** — Take(N) ++ Skip(N) == original input
    - **Property 17: Repeat Correctness** — output == input repeated count times
    - **Property 18: TeeCount Data Preservation** — output (minus count chunk) == input; count chunk is correct u64
    - **Validates: Requirements 11.2, 11.3, 11.4, 11.5, 11.6**

- [ ] 11. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 12. Register built-in streaming functions
  - [ ] 12.1 Create `deriva-compute/src/builtins_streaming/mod.rs` module and registration function
    - Create module file re-exporting all streaming function structs
    - Implement `register_streaming_builtins(registry: &mut FunctionRegistry)` that registers all 20 streaming functions
    - Call registration in the existing function registry initialization path
    - _Requirements: 7.1, 8.1, 9.1, 10.1, 11.1_

- [ ] 13. Implement streaming observability metrics
  - [ ] 13.1 Add streaming metrics to `deriva-compute/src/metrics.rs`
    - Register `deriva_stream_pipelines_total` counter
    - Register `deriva_stream_chunks_total` counter
    - Register `deriva_stream_bytes_total` counter
    - Register `deriva_stream_pipeline_duration_seconds` histogram
    - Implement a metered output wrapper that intercepts chunks and increments counters
    - Wire metrics into StreamPipeline execute path
    - _Requirements: 14.1, 14.2, 14.3, 14.4, 14.5_

  - [ ]* 13.2 Write property test for metrics counting accuracy (Property 20)
    - **Property 20: Metrics Counting Accuracy** — chunks_total increment == number of Data chunks emitted; bytes_total increment == total bytes emitted
    - **Validates: Requirements 14.5**

- [ ] 14. Implement gRPC integration with streaming mode selection
  - [ ] 14.1 Update `get()` RPC handler in `deriva-server/src/service.rs` for streaming mode
    - Check cache first — if hit, stream cached value to client via `value_to_stream`
    - Check `registry.has_streaming(root_function)` — if true, build StreamPipeline via StreamingExecutor
    - Tee pipeline output: one receiver for gRPC response streaming, one for background cache collector
    - If no streaming implementation, fall back to existing batch materialization path
    - Deliver chunks to client as they are produced (no waiting for full materialization)
    - Propagate Error chunks as gRPC Status errors
    - _Requirements: 12.1, 12.2, 12.3, 12.5, 12.6_

  - [ ] 14.2 Implement cache-after-collect background task
    - Spawn background task that collects tee'd stream via `collect_stream`
    - On success, cache full Bytes value under recipe's CAddr
    - On failure (stream error), log the failure and do not cache partial result
    - _Requirements: 13.1, 13.2, 13.3, 13.4, 12.4_

  - [ ]* 14.3 Write integration tests for gRPC streaming and caching
    - Test streaming response delivers chunks incrementally
    - Test batch fallback when no streaming implementation exists
    - Test cache-after-collect: first request streams + caches, second request hits cache
    - Test error propagation to gRPC Status
    - _Requirements: 12.1, 12.2, 12.3, 12.5, 13.1, 13.3_

- [ ] 15. Implement graceful cancellation and error propagation wiring
  - [ ] 15.1 Ensure cancellation propagation in pipeline execution
    - Verify that when output receiver is dropped, upstream senders get SendError and terminate
    - Verify that stage tasks drop their input receivers on termination (cascade upstream)
    - Ensure premature channel closure (None without End) is detected as error by `collect_stream`
    - Add defensive handling in all stage task spawns for send errors
    - _Requirements: 15.1, 15.2, 15.3, 15.4, 5.4_

  - [ ]* 15.2 Write integration tests for cancellation and hybrid execution
    - Test cancellation cascade: drop output receiver, verify all tasks terminate within timeout
    - Test hybrid pipeline: mix of streaming and batch stages produces correct output
    - Test batch stage following streaming stage and vice versa
    - _Requirements: 15.1, 15.2, 16.1, 16.2, 16.3, 16.4_

- [ ] 16. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties defined in the design (20 properties)
- Unit tests validate specific examples and edge cases
- The implementation language is Rust (matching the existing codebase and design)
- Dependencies needed: `base64`, `flate2`, `sha2`, `crc32fast` (for built-in streaming functions)
- All streaming functions use `proptest` for property-based testing (already in workspace)

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2"] },
    { "id": 1, "tasks": ["1.3", "2.1"] },
    { "id": 2, "tasks": ["2.2", "5.1"] },
    { "id": 3, "tasks": ["2.3", "2.4", "5.2", "3.1"] },
    { "id": 4, "tasks": ["3.2", "3.3", "3.4", "6.1"] },
    { "id": 5, "tasks": ["6.2", "7.1", "8.1", "9.1", "10.1"] },
    { "id": 6, "tasks": ["7.2", "8.2", "9.2", "10.2", "12.1"] },
    { "id": 7, "tasks": ["13.1"] },
    { "id": 8, "tasks": ["13.2", "14.1"] },
    { "id": 9, "tasks": ["14.2", "15.1"] },
    { "id": 10, "tasks": ["14.3", "15.2"] }
  ]
}
```
