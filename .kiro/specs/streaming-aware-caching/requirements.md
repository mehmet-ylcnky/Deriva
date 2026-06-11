# Requirements Document

## Introduction

Phase 2.13 extends the Deriva caching layer to be streaming-aware. The current cache stores computed values as monolithic `Bytes` blobs keyed by `CAddr`, requiring full-value buffering for both cache writes and reads. This defeats the bounded-memory streaming benefits established in §2.7 and §2.12. Streaming-aware caching introduces chunk-level storage with a manifest-based index, enabling incremental cache writes as chunks flow through the pipeline, streaming cache reads without full-value loading, and byte-range reads that fetch only the relevant chunks.

## Glossary

- **Cache**: The storage subsystem (in-memory `EvictableCache` and on-disk `BlobStore`) that retains computed values to avoid redundant recomputation.
- **CAddr**: Computation Address — a content-addressed hash that uniquely identifies a computed value based on its recipe and inputs.
- **ChunkAddr**: A sub-addressing scheme that identifies an individual chunk within a cached value, composed of a CAddr and a byte offset.
- **ChunkManifest**: A metadata structure recording the ordered sequence of chunks (offsets, sizes, blob keys) that compose a cached value.
- **ChunkEntry**: A single entry within a ChunkManifest describing one chunk's offset, length, and blob storage key.
- **ChunkCacheWriter**: A pipeline component that tees streaming chunks to both the consumer channel and the blob store incrementally.
- **ChunkCacheReader**: A component that streams cached chunks from blob storage without loading the full value into memory.
- **BlobStore**: On-disk blob storage that stores data keyed by string blob keys in a sharded directory structure.
- **StreamChunk**: The unit of data flowing through a streaming pipeline — either Data(Bytes), End, or Error.
- **StreamPipeline**: The streaming execution pipeline that processes data through function stages in bounded memory.
- **Manifest_Key**: A deterministic blob key derived from a CAddr that locates the ChunkManifest in BlobStore.
- **Chunk_Cache_Threshold**: A configurable size boundary; values below this threshold use monolithic caching and values above use chunk-level caching.
- **AsyncMaterializationCache**: The async trait providing get/put/contains operations used by the StreamingExecutor.
- **GC**: Garbage Collection — the process that removes orphaned blobs and cache entries not reachable from the live DAG.

## Requirements

### Requirement 1: Chunk-Indexed Storage

**User Story:** As a system operator, I want computed values stored as ordered chunks with a manifest index, so that individual chunks can be retrieved without loading the full value.

#### Acceptance Criteria

1. WHEN a computed value is cached via the chunk-level path, THE BlobStore SHALL store each chunk as a separate blob keyed by a deterministic ChunkAddr-derived key.
2. WHEN all chunks of a value are stored, THE BlobStore SHALL store a ChunkManifest blob at the Manifest_Key for that CAddr.
3. THE ChunkManifest SHALL contain an ordered, contiguous, non-overlapping sequence of ChunkEntry records where the first entry has offset zero and the sum of all entry lengths equals total_size.
4. WHEN a ChunkManifest is deserialized and validated, THE Cache SHALL verify that chunks are sorted by ascending offset, contiguous without gaps, and sum to total_size.
5. THE ChunkAddr SHALL derive a deterministic blob key from the CAddr and byte offset such that the same CAddr and offset always produce the same key.

### Requirement 2: Streaming Cache Writes

**User Story:** As a system operator, I want cache writes to happen incrementally as chunks flow through the pipeline, so that caching does not require buffering the full output value in memory.

#### Acceptance Criteria

1. WHEN a StreamChunk::Data arrives at the ChunkCacheWriter, THE ChunkCacheWriter SHALL write the chunk to BlobStore and forward the chunk to the consumer within a single pipeline step without accumulating other chunks in memory.
2. WHEN all chunks have been written and a StreamChunk::End arrives, THE ChunkCacheWriter SHALL serialize and store the ChunkManifest at the Manifest_Key.
3. IF a BlobStore write fails during streaming cache write, THEN THE ChunkCacheWriter SHALL clean up all previously written chunks for that value, forward an error to the consumer, and refrain from writing a manifest.
4. IF the consumer drops the receiving channel during cache write, THEN THE ChunkCacheWriter SHALL clean up all previously written chunks for that value and stop processing.
5. WHILE the ChunkCacheWriter is active, THE ChunkCacheWriter SHALL maintain peak memory usage of one chunk size plus manifest builder overhead regardless of total output size.
6. THE ChunkCacheWriter SHALL record the number of bytes written and number of chunks stored via observability metrics.

### Requirement 3: Streaming Cache Reads

**User Story:** As a system operator, I want cache hits to stream chunks directly from storage rather than loading the full value into memory, so that serving cached values respects bounded-memory constraints.

#### Acceptance Criteria

1. WHEN a streaming cache read is requested for a CAddr with a valid ChunkManifest, THE ChunkCacheReader SHALL produce a stream of StreamChunk::Data items by reading chunks sequentially from BlobStore without loading all chunks simultaneously.
2. WHEN all chunks have been sent, THE ChunkCacheReader SHALL send a StreamChunk::End to signal completion.
3. IF any chunk blob is missing during a streaming cache read, THEN THE ChunkCacheReader SHALL treat the value as a cache miss and signal an error.
4. THE AsyncMaterializationCache trait SHALL expose a get_stream method that returns a streaming Receiver of StreamChunk on cache hit and None on cache miss.
5. WHEN the StreamingExecutor detects a cache hit, THE StreamingExecutor SHALL prefer streaming cache reads over monolithic full-value loads when the value was stored as chunks.

### Requirement 4: Full-Value Cache Read Backward Compatibility

**User Story:** As a developer using the existing cache API, I want existing `cache.get(addr)` calls to continue returning full `Bytes`, so that code not yet migrated to streaming reads continues to function.

#### Acceptance Criteria

1. WHEN cache.get(addr) is called and the value is stored as chunks with a ChunkManifest, THE Cache SHALL read all chunks, concatenate them in offset order, and return the assembled Bytes.
2. WHEN cache.get(addr) is called and the value is stored as a legacy monolithic blob, THE Cache SHALL return the blob directly as Bytes.
3. WHEN cache.put(addr, bytes) is called with a monolithic Bytes value, THE Cache SHALL store it as a single chunk with a corresponding single-entry ChunkManifest.
4. THE Cache SHALL transparently handle both legacy single-blob entries and new chunk-level entries without requiring callers to distinguish between the two.

### Requirement 5: Byte-Range Read Support

**User Story:** As a client requesting a partial read of a cached value, I want to retrieve only the bytes within a specified range, so that large values can be partially consumed without loading the entire content.

#### Acceptance Criteria

1. WHEN a range read is requested for bytes [start, end) on a CAddr with a ChunkManifest, THE Cache SHALL identify the minimal set of chunks overlapping the requested range using the manifest offsets.
2. THE Cache SHALL read only the identified overlapping chunks from BlobStore rather than all chunks.
3. WHEN the first overlapping chunk contains leading bytes before the requested start, THE Cache SHALL trim the leading bytes from the response.
4. WHEN the last overlapping chunk extends beyond the requested end, THE Cache SHALL trim the trailing bytes from the response.
5. THE Cache SHALL return exactly (end - start) bytes when the requested range falls within the value's total_size.
6. IF the requested range exceeds the value's total_size, THEN THE Cache SHALL return bytes up to total_size and signal the actual number of bytes returned.

### Requirement 6: Size-Based Cache Path Selection

**User Story:** As a system operator, I want small values cached monolithically and large values cached as chunks, so that the overhead of manifest management is avoided for trivially small values.

#### Acceptance Criteria

1. WHEN a streaming pipeline output has a known size below the Chunk_Cache_Threshold, THE Cache SHALL use the monolithic cache-after-collect path.
2. WHEN a streaming pipeline output has a known size at or above the Chunk_Cache_Threshold, THE Cache SHALL use the chunk-level cache path with ChunkCacheWriter.
3. WHEN a streaming pipeline output has an unknown size, THE Cache SHALL use the chunk-level cache path.
4. THE Chunk_Cache_Threshold SHALL be configurable via PipelineConfig with a default value of 1 MB.

### Requirement 7: Garbage Collection Integration

**User Story:** As a system operator, I want garbage collection to atomically remove all chunks and the manifest for a value when it is evicted, so that no orphan chunks accumulate on disk.

#### Acceptance Criteria

1. WHEN GC determines a CAddr is unreachable, THE GC SHALL read the ChunkManifest for that CAddr and remove all referenced chunk blobs and the manifest blob.
2. WHEN GC encounters a CAddr without a manifest, THE GC SHALL remove the legacy single-blob entry using the existing removal path.
3. IF a chunk removal fails during GC, THEN THE GC SHALL log the failure and continue removing remaining chunks for that CAddr.
4. WHEN the ChunkCacheWriter is interrupted before writing a manifest, THE GC SHALL detect orphan chunk blobs (chunks with no corresponding manifest) during periodic sweeps and remove them.
5. THE GC SHALL report the number of chunk blobs removed and bytes reclaimed via GcResult metrics.

### Requirement 8: Partial Cache Hit Detection

**User Story:** As a system operator, I want the cache to detect when some chunks of a value are missing and treat it as a full cache miss, so that corrupted or partially evicted entries do not produce incorrect results.

#### Acceptance Criteria

1. WHEN a cache read encounters a ChunkManifest but one or more referenced chunk blobs are missing from BlobStore, THE Cache SHALL treat the value as a full cache miss.
2. WHEN a partial cache miss is detected, THE Cache SHALL remove the ChunkManifest and any remaining chunk blobs for that CAddr to allow clean re-caching.
3. THE Cache SHALL log a warning when a partial cache miss is detected including the CAddr and count of missing chunks.

### Requirement 9: Observability

**User Story:** As a system operator, I want metrics and structured logs for chunk-level cache operations, so that I can monitor cache behavior and diagnose performance issues.

#### Acceptance Criteria

1. THE Cache SHALL expose metrics for chunk cache write operations including bytes written, chunks stored, and manifests committed.
2. THE Cache SHALL expose metrics for chunk cache read operations including bytes read, chunks served, and stream reads initiated.
3. THE Cache SHALL expose metrics for range read operations including range requests served and chunks read per range request.
4. THE Cache SHALL expose metrics for cache path selection including count of monolithic writes versus chunk-level writes.
5. WHEN a chunk cache write or read fails, THE Cache SHALL emit a structured log at warn level with the CAddr, operation type, and error description.
6. WHEN a partial cache miss is detected, THE Cache SHALL increment a partial_miss counter metric.

### Requirement 10: Memory Bounds

**User Story:** As a system operator, I want streaming cache operations to respect bounded memory usage proportional to chunk size rather than total value size, so that caching a 4 GB value does not require 4 GB of memory.

#### Acceptance Criteria

1. WHILE performing a streaming cache write, THE ChunkCacheWriter SHALL hold at most one chunk plus manifest builder metadata in memory at any point.
2. WHILE performing a streaming cache read, THE ChunkCacheReader SHALL hold at most a bounded number of chunks in flight determined by channel capacity.
3. WHEN serving a range read, THE Cache SHALL allocate memory proportional to the range size plus at most two chunk sizes of overhead rather than the full value size.
4. THE Cache SHALL support caching values larger than available system memory by writing chunks incrementally to disk-backed BlobStore.
