# Phase 2.7 Complete: Streaming Materialization

**Date:** 2026-02-19  
**Commits:** 9 commits (4fa7fcf through 4f33fe8)  
**Status:** ✅ Complete

---

## Overview

Implemented chunked streaming pipeline execution with backpressure. Data flows through compute stages as 64KB chunks via tokio mpsc channels, enabling concurrent stage execution with bounded memory. The gRPC `get()` RPC automatically selects streaming vs batch mode per function.

## Implementation

### Core Types

**StreamChunk** (`deriva-core/src/streaming.rs`):
- `Data(Bytes)` — payload chunk
- `End` — stream termination
- `Error(DerivaError)` — error propagation

**StreamingComputeFunction trait** (`deriva-compute/src/streaming.rs`):
- `stream_execute(inputs, params) -> Receiver<StreamChunk>`
- Adapters: `batch_to_stream`, `value_to_stream`, `collect_stream`
- `tee_stream` — clone a stream to two consumers

### Pipeline Builder (`deriva-compute/src/pipeline.rs`)

- `StreamPipeline` with `PipelineConfig` (chunk_size, channel_capacity, memory_budget)
- Node types: Source, Cached, StreamingStage, BatchStage
- Hybrid execution — streaming and batch stages in same pipeline
- Metered output stream for observability

### 20 Built-in Streaming Functions (`deriva-compute/src/builtins_streaming.rs`)

| Category | Functions |
|----------|-----------|
| Transforms (9) | Identity, Uppercase, Lowercase, Reverse, Base64Encode, Base64Decode, Xor, Compress, Decompress |
| Accumulators (3) | Sha256, ByteCount, Checksum (CRC32) |
| Combiners (3) | Concat, Interleave, ZipConcat |
| Utilities (5) | ChunkResizer, Take, Skip, Repeat, TeeCount |

### FunctionRegistry Extension

- `register_streaming()` — register streaming-capable functions
- `has_streaming()` — check if a function supports streaming
- `get_streaming()` — retrieve streaming function by FunctionId

### Streaming Executor (`deriva-compute/src/streaming_executor.rs`)

- Takes `&dyn DagReader` for DAG traversal
- Builds `StreamPipeline` from recipe DAG with cache-aware stage selection

### gRPC Integration (`deriva-server/src/service.rs`)

- `get()` RPC checks `registry.has_streaming()` for root recipe
- Streaming path: builds pipeline, tees output to client + background cache task
- Batch path: unchanged existing behavior

### Observability (4 metrics)

- `deriva_stream_pipelines_total` — pipeline execution count
- `deriva_stream_chunks_total` — chunks processed
- `deriva_stream_bytes_total` — bytes processed
- `deriva_stream_pipeline_duration_seconds` — end-to-end latency histogram

### Files Modified

- `deriva-core/src/streaming.rs` — StreamChunk type
- `deriva-compute/src/streaming.rs` — trait, adapters, tee_stream
- `deriva-compute/src/builtins_streaming.rs` — 20 built-in functions
- `deriva-compute/src/pipeline.rs` — StreamPipeline builder
- `deriva-compute/src/streaming_executor.rs` — DAG-to-pipeline executor
- `deriva-compute/src/registry.rs` — streaming function registry
- `deriva-compute/src/metrics.rs` — streaming metrics
- `deriva-server/src/service.rs` — gRPC streaming/batch routing
- `deriva-server/src/state.rs` — BlobStore field for leaf access

### Dependencies Added

```toml
sha2 = "0.10"
base64 = "0.22"
flate2 = "1"
crc32fast = "1"
```

## Testing

### Unit Tests (52 tests in `deriva-compute/tests/streaming.rs`)

| Section | Tests | Coverage |
|---------|-------|----------|
| §5.1–5.3 | 10 | StreamChunk, batch_to_stream, collect_stream |
| §5.4–5.6 | 6 | Concat, Sha256, Uppercase |
| §5.7–5.8 | 4 | tee_stream, pipeline integration |
| §5.9–5.24 | 32 | All 16 new builtin functions + pipeline compositions |

**Results:** ✅ 52 passed, 0 failed

## Benchmark Results (`streaming_pipeline.rs`)

| Benchmark | Time |
|-----------|------|
| Identity 1MB | ~1.8 ms |
| Identity 100MB | ~170 ms |
| 3-stage pipeline 1MB | ~2.15 ms |
| Backpressure (1ms/chunk) | ~10 ms |
| **Chunk size comparison (1MB):** | |
| 1KB chunks | ~8.1 ms |
| 16KB chunks | ~1.86 ms |
| 64KB chunks (default) | ~1.89 ms |
| 256KB chunks | ~1.85 ms |
| 1MB chunks | ~1.64 ms |

**Key insight:** 1KB chunks are ~4× slower due to channel overhead. 16KB–64KB is the sweet spot, confirming spec §7.4 analysis.

## Next Steps

Phase 2.8: Garbage Collection (final Phase 2 section)
