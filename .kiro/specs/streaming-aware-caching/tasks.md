# Implementation Plan: Streaming-Aware Caching

## Overview

This plan implements Phase 2.13 — streaming-aware caching for the Deriva computation-addressed DFS. The implementation introduces chunk-indexed storage with manifest-based indexing, incremental cache writes/reads, byte-range reads, size-based path selection, GC integration, and observability. All work is in Rust within the `crates/deriva-compute` and `crates/deriva-storage` crates.

## Tasks

- [ ] 1. Define core data structures and interfaces
  - [ ] 1.1 Create `ChunkAddr`, `ChunkEntry`, `ChunkManifest`, and `ChunkManifestBuilder` types
    - Create a new file `crates/deriva-compute/src/chunk_cache.rs`
    - Define `ChunkAddr` with `caddr: CAddr` and `offset: u64`, implementing `blob_key()` as `"{caddr_hex}_chunk_{offset}"`
    - Define `ChunkEntry` with `offset: u64`, `length: u32`, `blob_key: String`
    - Define `ChunkManifest` with `caddr`, `total_size`, `chunk_size`, `chunks: Vec<ChunkEntry>`
    - Implement `ChunkManifest::to_bytes()`, `from_bytes()`, `validate()`, `chunks_for_range()`
    - Implement `ChunkManifestBuilder` with `add_chunk()`, `build()`, `blob_keys()`
    - Define `manifest_key(caddr: &CAddr) -> String` as `"{caddr_hex}_manifest"`
    - Export new module from `crates/deriva-compute/src/lib.rs`
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

  - [ ]* 1.2 Write property tests for ChunkManifest validity and ChunkAddr determinism
    - **Property 2: Manifest validity invariant** — For any sequence of `add_chunk` calls, the built manifest satisfies offset=0, ascending, contiguous, sum=total_size
    - **Property 11: ChunkAddr determinism** — Same (CAddr, offset) always produces same key; different pairs produce different keys
    - **Validates: Requirements 1.3, 1.4, 1.5**

  - [ ]* 1.3 Write unit tests for ChunkManifest and ChunkAddr
    - Test `ChunkManifestBuilder` produces correct manifests
    - Test `validate()` passes for valid manifests and fails for invalid ones
    - Test `chunks_for_range()` boundary cases (start of chunk, end of chunk, spanning multiple)
    - Test `blob_key()` and `manifest_key()` determinism and uniqueness
    - _Requirements: 1.3, 1.4, 1.5_

- [ ] 2. Implement ChunkCacheWriter for streaming cache writes
  - [ ] 2.1 Implement `ChunkCacheWriter` struct and `wrap()` method
    - Create `ChunkCacheWriter` in `crates/deriva-compute/src/chunk_cache.rs` (or a sub-module `chunk_cache_writer.rs`)
    - Accept `caddr`, `chunk_size`, and `BlobStore` reference
    - Implement `wrap(self, input: Receiver<StreamChunk>, capacity: usize) -> Receiver<StreamChunk>`
    - On `Data`: write chunk to BlobStore via `put(chunk_key, bytes)`, append to manifest builder, forward to consumer
    - On `End`: serialize and store ChunkManifest at manifest_key, forward End
    - On `Error` or consumer channel drop: clean up all previously written chunk blobs, do not write manifest
    - Ensure peak memory usage is one chunk + manifest builder overhead
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [ ]* 2.2 Write property test for chunk write round-trip
    - **Property 1: Chunk write round-trip** — Write random bytes through ChunkCacheWriter then stream read back; concatenation of reads equals original
    - **Validates: Requirements 1.1, 1.2, 3.1, 3.2**

  - [ ]* 2.3 Write property test for atomic commit semantics
    - **Property 4: Atomic commit semantics** — Inject failures at random chunk positions; verify no manifest exists, chunks cleaned up
    - **Validates: Requirements 2.3, 2.4**

  - [ ]* 2.4 Write property test for bounded memory during write
    - **Property 5: Bounded memory during write** — For large values, track peak allocations during write and verify bounded by chunk_size + manifest overhead
    - **Validates: Requirements 2.5, 10.1**

- [ ] 3. Implement ChunkCacheReader for streaming cache reads
  - [ ] 3.1 Implement `stream_from_cache()` and streaming read logic
    - Implement `stream_from_cache(manifest, blob_store, capacity) -> Receiver<StreamChunk>`
    - Read chunks sequentially from BlobStore based on manifest entries
    - Send `StreamChunk::Data` for each chunk, then `StreamChunk::End`
    - If any chunk blob is missing, treat as cache miss (return error/None)
    - Ensure at most `capacity` chunks are in flight at once
    - _Requirements: 3.1, 3.2, 3.3, 10.2_

  - [ ] 3.2 Implement partial cache miss detection and cleanup
    - When a chunk blob referenced by manifest is missing during read, treat as full cache miss
    - Remove the ChunkManifest and any remaining chunk blobs for that CAddr
    - Log a warning with CAddr and count of missing chunks
    - _Requirements: 8.1, 8.2, 8.3_

  - [ ]* 3.3 Write property test for partial miss cleanup
    - **Property 8: Partial miss cleanup** — Remove random chunks from a stored set; verify read returns None and cleanup removes manifest + remaining chunks
    - **Validates: Requirements 8.1, 8.2**

- [ ] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 5. Implement byte-range reads
  - [ ] 5.1 Implement `range_read()` function
    - Implement `range_read(manifest, blob_store, offset, length) -> Result<Bytes>`
    - Use `chunks_for_range()` to identify overlapping chunks
    - Read only the overlapping chunks from BlobStore
    - Trim leading bytes from first overlapping chunk, trailing bytes from last
    - Return exactly `(end - start)` bytes when range is within total_size
    - If range exceeds total_size, return bytes up to total_size and signal actual length
    - Allocate memory proportional to range size + at most two chunk sizes of overhead
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 10.3_

  - [ ]* 5.2 Write property test for range read correctness
    - **Property 3: Range read correctness** — For random values and random ranges within total_size, verify range_read returns the correct byte substring
    - **Validates: Requirements 5.1, 5.2, 5.3, 5.4, 5.5**

- [ ] 6. Extend AsyncMaterializationCache trait with streaming methods
  - [ ] 6.1 Add `get_stream`, `has_manifest`, and `range_read` methods to `AsyncMaterializationCache`
    - Extend the trait in `crates/deriva-compute/src/cache.rs` with new methods
    - `get_stream(&self, addr, capacity) -> Option<Receiver<StreamChunk>>`: read manifest, if present stream via ChunkCacheReader
    - `has_manifest(&self, addr) -> bool`: check if manifest_key exists in BlobStore
    - `range_read(&self, addr, offset, length) -> Option<Bytes>`: read manifest, delegate to range_read function
    - _Requirements: 3.4, 5.1_

  - [ ] 6.2 Implement full-value `get()` backward compatibility for chunk-cached entries
    - When `get(addr)` is called and manifest exists, read all chunks, concatenate in offset order, return assembled Bytes
    - When `get(addr)` is called with legacy single-blob entry, return directly
    - When `put(addr, bytes)` is called monolithically, store as single chunk with single-entry manifest
    - Handle both legacy and chunk-level entries transparently
    - _Requirements: 4.1, 4.2, 4.3, 4.4_

  - [ ]* 6.3 Write property tests for backward-compatible full read and legacy coexistence
    - **Property 6: Backward-compatible full read** — For chunk-cached values, `get(addr)` returns byte-identical full value
    - **Property 7: Legacy entry coexistence** — Mix of legacy + chunked entries both serve correctly
    - **Validates: Requirements 4.1, 4.2, 4.3, 4.4**

- [ ] 7. Implement size-based cache path selection
  - [ ] 7.1 Add `chunk_cache_threshold` to `PipelineConfig` and implement path selection logic
    - Add `chunk_cache_threshold: usize` field to `PipelineConfig` with default 1MB
    - In the streaming executor, when output size < threshold → use monolithic cache-after-collect
    - When output size >= threshold → use ChunkCacheWriter
    - When output size is unknown → use ChunkCacheWriter
    - _Requirements: 6.1, 6.2, 6.3, 6.4_

  - [ ] 7.2 Integrate ChunkCacheWriter into StreamingExecutor pipeline
    - Modify `crates/deriva-compute/src/streaming_executor.rs` to use ChunkCacheWriter when chunk-level path is selected
    - When cache hit detected, prefer `get_stream` over monolithic load for chunk-cached values
    - Wire up path selection based on known/unknown size and threshold
    - _Requirements: 3.5, 6.1, 6.2, 6.3_

  - [ ]* 7.3 Write property test for size-based path selection
    - **Property 10: Size-based path selection** — Random sizes around threshold; verify correct path chosen
    - **Validates: Requirements 6.1, 6.2, 6.3, 6.4**

- [ ] 8. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 9. Implement GC integration for chunk-cached entries
  - [ ] 9.1 Extend GC to handle chunk-manifested entries
    - Modify `crates/deriva-compute/src/gc.rs`
    - When GC determines a CAddr is unreachable: read its ChunkManifest, remove all referenced chunk blobs, remove the manifest blob
    - When no manifest exists: use legacy single-blob removal path
    - If a chunk removal fails: log the failure, continue removing remaining chunks
    - Report number of chunk blobs removed and bytes reclaimed via GcResult
    - _Requirements: 7.1, 7.2, 7.3, 7.5_

  - [ ] 9.2 Implement orphan chunk detection and cleanup during GC sweeps
    - Detect chunk blobs matching pattern `*_chunk_*` with no corresponding manifest
    - These are remnants of interrupted ChunkCacheWriter writes
    - Remove orphan chunks during periodic GC sweep
    - _Requirements: 7.4_

  - [ ]* 9.3 Write property test for GC atomicity
    - **Property 9: GC removes all chunks atomically** — Store chunks + manifest, run GC, verify all chunk blobs and manifest are removed
    - **Validates: Requirements 7.1, 7.2**

- [ ] 10. Implement observability metrics and structured logging
  - [ ] 10.1 Add chunk cache metrics and structured logging
    - Extend `crates/deriva-compute/src/metrics.rs` with new counters/gauges
    - Cache write metrics: bytes_written, chunks_stored, manifests_committed
    - Cache read metrics: bytes_read, chunks_served, stream_reads_initiated
    - Range read metrics: range_requests_served, chunks_read_per_range
    - Path selection metrics: monolithic_writes, chunk_level_writes
    - Partial miss counter: partial_miss_count
    - Emit structured warn-level logs on cache write/read failures with CAddr, operation type, error
    - Record bytes written and chunks stored in ChunkCacheWriter via metrics
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 2.6_

- [ ] 11. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The `chunk_cache.rs` module is new; all other modifications are to existing files (`cache.rs`, `gc.rs`, `streaming_executor.rs`, `metrics.rs`, `pipeline.rs`)
- The design's "manifest-last" commit strategy ensures that incomplete writes are never seen as cached (no manifest = not cached)

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "2.1"] },
    { "id": 2, "tasks": ["2.2", "2.3", "2.4", "3.1"] },
    { "id": 3, "tasks": ["3.2", "3.3", "5.1"] },
    { "id": 4, "tasks": ["5.2", "6.1"] },
    { "id": 5, "tasks": ["6.2", "6.3"] },
    { "id": 6, "tasks": ["7.1"] },
    { "id": 7, "tasks": ["7.2", "7.3"] },
    { "id": 8, "tasks": ["9.1"] },
    { "id": 9, "tasks": ["9.2", "9.3"] },
    { "id": 10, "tasks": ["10.1"] }
  ]
}
```
