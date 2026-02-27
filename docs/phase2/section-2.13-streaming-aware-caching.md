# §2.13 Streaming-Aware Caching

> **Status**: Blueprint (Phase 3 — deferred)
> **Depends on**: §2.7 Streaming Materialization, §2.8 Garbage Collection
> **Crate(s)**: `deriva-core`, `deriva-compute`, `deriva-storage`
> **Estimated effort**: 5–7 days

---

## 1. Problem Statement

### 1.1 Current Limitation

TODO — cache-after-collect (§2.7 design decision) requires buffering the full result before caching; for >1 GB outputs this defeats the bounded-memory benefit of streaming

### 1.2 Real-World Impact

TODO — example: 4 GB video transcode pipeline streams to client in 2.5 MB peak memory, but cache-after-collect requires 4 GB buffer; system OOMs or skips caching

### 1.3 Comparison

TODO — table: cache-after-collect vs chunk-level cache (memory, complexity, partial cache hits, range reads, GC impact)

---

## 2. Design

### 2.1 Architecture Overview

TODO — chunk-level cache keyed by `(CAddr, offset, length)`; chunks stored individually in blob store; manifest tracks chunk sequence per CAddr

### 2.2 Sub-Addressing Model

TODO — `ChunkAddr = (CAddr, u64 offset)` as cache key; chunks are contiguous, non-overlapping byte ranges of the full value

### 2.3 Chunk Manifest

TODO — per-CAddr manifest: ordered list of `(offset, length, chunk_blob_key)`; stored as a small metadata blob alongside the chunks

### 2.4 Cache Write Path

TODO — as each chunk arrives from pipeline, write to blob store immediately with `ChunkAddr` key; on stream End, write manifest; no full-result buffering

### 2.5 Cache Read Path — Full Value

TODO — read manifest, concatenate all chunks in order; equivalent to current cache hit behavior

### 2.6 Cache Read Path — Range Read

TODO — read manifest, identify overlapping chunks, read only those; enables byte-range requests without full materialization

### 2.7 Cache Read Path — Partial Hit

TODO — some chunks cached, others missing (e.g., after partial GC); re-execute pipeline but skip cached chunks; requires chunk-aware pipeline execution

---

## 3. Implementation

### 3.1 ChunkAddr Type

TODO — `struct ChunkAddr { caddr: CAddr, offset: u64 }` in `deriva-core`

### 3.2 ChunkManifest Type

TODO — `struct ChunkManifest { caddr: CAddr, total_size: u64, chunks: Vec<ChunkEntry> }` where `ChunkEntry = { offset: u64, length: u32, blob_key: BlobKey }`

### 3.3 ChunkCacheWriter

TODO — implements streaming cache write: receives `StreamChunk`s, writes each to blob store, builds manifest, writes manifest on End

### 3.4 ChunkCacheReader

TODO — reads manifest, returns either full `Bytes` (concatenated) or `Receiver<StreamChunk>` (re-streamed from cache)

### 3.5 Range Read Support

TODO — `fn read_range(caddr: &CAddr, offset: u64, length: u64) -> Result<Bytes>` — reads only necessary chunks from manifest

### 3.6 GC Integration

TODO — chunk-level reference counting; GC must evict all chunks + manifest atomically; partial eviction creates inconsistency

### 3.7 Migration Path

TODO — coexistence with existing full-value cache; new entries use chunk cache, old entries served from full-value cache; gradual migration

---

## 4. Data Flow Diagrams

### 4.1 Chunk Cache Write Path

TODO — pipeline → ChunkCacheWriter → blob store (per chunk) + manifest

### 4.2 Chunk Cache Full Read

TODO — manifest → read all chunks → concatenate → return Bytes

### 4.3 Chunk Cache Range Read

TODO — manifest → identify overlapping chunks → read subset → slice → return

### 4.4 Partial Cache Hit

TODO — manifest → some chunks present, some missing → re-execute for missing → merge

---

## 5. Test Specification

### 5.1 Unit Tests — ChunkAddr / ChunkManifest

TODO — construction, serialization/deserialization, ordering

### 5.2 Unit Tests — ChunkCacheWriter

TODO — write stream of chunks, verify manifest and blob entries created

### 5.3 Unit Tests — ChunkCacheReader Full Read

TODO — read all chunks, verify concatenation matches original

### 5.4 Unit Tests — ChunkCacheReader Range Read

TODO — range within single chunk, range spanning multiple chunks, range at boundaries

### 5.5 Unit Tests — GC Eviction

TODO — evict CAddr, verify all chunks + manifest removed atomically

### 5.6 Integration Tests — End-to-End Chunk Caching

TODO — pipeline → chunk cache write → chunk cache read → verify identical output

### 5.7 Integration Tests — Range Read After Streaming

TODO — stream large value, then range-read middle portion

### 5.8 Benchmark — Chunk Cache vs Full-Value Cache

TODO — write/read latency comparison; memory usage during write

---

## 6. Edge Cases & Error Handling

TODO — table: empty stream (0 chunks), single-chunk stream (degenerate case), blob store write failure mid-stream (partial manifest), concurrent read during write, manifest corruption, chunk size mismatch with manifest, GC during active write

---

## 7. Performance Analysis

### 7.1 Write Path Overhead

TODO — per-chunk blob store write vs single full-value write; more I/O ops but bounded memory

### 7.2 Read Path Overhead

TODO — manifest lookup + N chunk reads vs single read; ~1 ms overhead for manifest; parallelizable chunk reads

### 7.3 Range Read Savings

TODO — quantify: 1 KB range from 4 GB value reads ~1 chunk (64 KB) instead of 4 GB

### 7.4 Storage Overhead

TODO — manifest size: ~20 bytes per chunk; 4 GB / 64 KB = 65K chunks × 20 B = 1.3 MB manifest; negligible

---

## 8. Files Changed

TODO — table of affected files

---

## 9. Dependency Changes

TODO — none expected (uses existing blob store interface)

---

## 10. Design Rationale

### 10.1 Why Deferred to Phase 3?

TODO — cache-after-collect works for most workloads (<1 GB); chunk caching adds significant complexity to cache, GC, and invalidation; only justified for very large values

### 10.2 Why Not Compress Chunks Before Caching?

TODO — compression is a separate pipeline stage; cache stores raw chunks to avoid double-compression; users can add compression stage if needed

### 10.3 Why Atomic Manifest + Chunks Instead of Append-Only Log?

TODO — manifest enables random access (range reads); append-only log requires sequential scan

### 10.4 Why Not Use the Existing Blob Store Directly?

TODO — blob store is for leaf data; chunk cache is for computed intermediate/output data; different lifecycle (GC vs user-managed)

---

## 11. Observability Integration

TODO — `deriva_chunk_cache_writes_total`, `deriva_chunk_cache_reads_total`, `deriva_chunk_cache_range_reads_total`, `deriva_chunk_cache_bytes_written`, `deriva_chunk_cache_manifest_size_bytes` histogram

---

## 12. Checklist

- [ ] Define `ChunkAddr` and `ChunkManifest` types in `deriva-core`
- [ ] Implement `ChunkCacheWriter` (streaming write to blob store)
- [ ] Implement `ChunkCacheReader` (full read + range read)
- [ ] Implement manifest serialization/deserialization
- [ ] Integrate chunk cache write into pipeline execution path
- [ ] Integrate chunk cache read into `StreamingExecutor` cache lookup
- [ ] Implement range read support
- [ ] Update GC to handle chunk-level eviction atomically
- [ ] Migration path: coexist with full-value cache
- [ ] Unit tests for types, writer, reader, range read, GC
- [ ] Integration tests for end-to-end chunk caching
- [ ] Benchmark chunk cache vs full-value cache
- [ ] Add observability metrics
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing tests still pass
- [ ] Commit and push
