# §2.13 Streaming-Aware Caching

> **Status**: Blueprint (Phase 3 — deferred)
> **Depends on**: §2.7 Streaming Materialization, §2.8 Garbage Collection
> **Crate(s)**: `deriva-core`, `deriva-compute`, `deriva-storage`
> **Estimated effort**: 5–7 days

---

## 1. Problem Statement

### 1.1 Current Limitation

The caching layer stores computed values as monolithic `Bytes` blobs
keyed by `CAddr`. Both the in-memory `EvictableCache` and the on-disk
`BlobStore` operate on full values:

```rust
// crates/deriva-core/src/cache.rs
pub struct EvictableCache {
    entries: HashMap<CAddr, CacheEntry>,  // CacheEntry.data: Bytes (full value)
    current_size: u64,
    // ...
}

impl EvictableCache {
    pub fn get(&mut self, addr: &CAddr) -> Option<Bytes> { ... }      // returns full value
    pub fn put(&mut self, addr: CAddr, data: Bytes, ...) -> u64 { ... } // stores full value
}

// crates/deriva-storage/src/blob_store.rs
impl BlobStore {
    pub fn put(&self, addr: &CAddr, data: &[u8]) -> Result<u64> { ... } // writes full blob
    pub fn get(&self, addr: &CAddr) -> Result<Option<Bytes>> { ... }    // reads full blob
}
```

The streaming executor (`streaming_executor.rs`) checks the cache
before building the pipeline. On a cache hit, the full `Bytes` value
is loaded and fed into the pipeline as a `Cached` source node:

```rust
// crates/deriva-compute/src/streaming_executor.rs
if let Some(cached_data) = cache.get(topo_addr).await {
    let idx = pipeline.add_cached(*topo_addr, cached_data);  // full Bytes in memory
    // ...
}
```

After pipeline execution, the output stream must be fully collected
into a single `Bytes` before it can be cached — this is the
"cache-after-collect" pattern. The streaming pipeline produces chunks
via `mpsc::Receiver<StreamChunk>`, but the cache API requires a
monolithic `Bytes`:

```
  Pipeline output (streaming)          Cache write (monolithic)
  ─────────────────────────            ────────────────────────
  chunk₁ (64 KB) ─┐
  chunk₂ (64 KB)  ├─► collect_stream() ─► Bytes (full value) ─► cache.put(addr, bytes)
  chunk₃ (64 KB)  │
  ...             ─┘
  End
```

This creates four problems:

1. **Full buffering defeats streaming** — the entire output must be
   held in memory before caching. A 4 GB pipeline output that streams
   to the client in 2.5 MB peak memory requires a 4 GB buffer just
   for caching.

2. **Cache reads are monolithic** — a cache hit loads the entire value
   into memory even if the consumer only needs a byte range. A 4 GB
   cached value requires 4 GB of memory to serve any request.

3. **No partial cache hits** — if GC evicts a cached value, the entire
   computation must be re-executed. There is no way to retain some
   chunks and recompute only the missing ones.

4. **No range-read support** — HTTP byte-range requests or partial
   reads require loading the full value and slicing in memory. There
   is no way to read only the relevant portion from cache.

### 1.2 Worst-Case Memory Analysis

Cache-after-collect memory cost = full output size held in memory:

| Output Size | Pipeline Peak Memory | Cache-After-Collect Buffer | Total Peak | Streaming Benefit Lost |
|------------|---------------------|---------------------------|-----------|----------------------|
| 64 KB | 512 KB | 64 KB | 576 KB | 0% (trivial) |
| 1 MB | 512 KB | 1 MB | 1.5 MB | 66% of peak is cache buffer |
| 100 MB | 512 KB | 100 MB | 100.5 MB | 99.5% of peak is cache buffer |
| 1 GB | 512 KB | 1 GB | 1.0005 GB | 99.95% of peak is cache buffer |
| 4 GB | 512 KB | 4 GB | 4.0005 GB | 99.99% of peak is cache buffer |

Pipeline peak memory assumes §2.12 budget enforcement at 512 KB. The
cache buffer completely dominates for outputs > 1 MB, negating the
bounded-memory benefit of streaming.

### 1.3 Real-World Impact

```
Scenario: Video transcoding pipeline on a 2 GB container

  Pipeline: Source(4 GB raw) → Compress → Encrypt → Checksum
  Streaming peak memory: ~2.5 MB (§2.12 budget = 2 MB + overhead)
  Client receives output as a stream — bounded memory, works fine.

  Now cache the result for future requests:
  Option A: cache-after-collect
    → collect_stream() buffers 4 GB → OOM on 2 GB container
    → Must skip caching entirely for large outputs

  Option B: skip caching for large values
    → Every request re-executes the full pipeline
    → 4 GB × 200 μs/chunk × 65,536 chunks = ~13 seconds per request
    → No benefit from repeated access patterns

  Option C: chunk-level cache (§2.13)
    → Each 64 KB chunk written to blob store as it arrives
    → Peak memory: 2.5 MB (unchanged from streaming)
    → Cache hit: stream chunks from blob store (bounded memory)
    → Range read: read only needed chunks (~64 KB for a 64 KB range)
```

The threshold where cache-after-collect becomes problematic depends on
the container's available memory. On a 2 GB container with ~1 GB
available for caching, outputs > 500 MB cannot be cached without
risking OOM. On a 512 MB container, the threshold drops to ~200 MB.

### 1.4 Comparison

| Dimension | Cache-After-Collect (current) | Chunk-Level Cache (§2.13) |
|-----------|------------------------------|--------------------------|
| Cache write memory | O(output_size) — full value buffered | O(chunk_size) — one chunk at a time |
| Cache read memory | O(output_size) — full value loaded | O(chunk_size) per chunk, or O(range) for range reads |
| Cache write latency | Blocked until stream completes | Incremental — first chunk cached immediately |
| Cache read latency | Single blob read | Manifest read + N chunk reads (parallelizable) |
| Range read support | No — must load full value and slice | Yes — read only overlapping chunks |
| Partial cache hit | No — all or nothing | Yes — re-execute only missing chunks |
| GC granularity | Per-CAddr (all or nothing) | Per-CAddr (all chunks + manifest atomically) |
| Storage overhead | 0 (single blob) | ~20 bytes/chunk manifest entry (0.03% for 64 KB chunks) |
| Complexity | Simple — existing `put()`/`get()` | Moderate — new types, manifest, chunk addressing |
| Max cacheable size | Limited by available memory | Limited by available disk (unbounded in practice) |
| Interaction with §2.12 | Budget irrelevant during cache write (full buffer) | Budget respected — cache write is part of streaming |
| Interaction with §2.8 GC | Simple — remove one blob per CAddr | Must remove all chunks + manifest atomically |

### 1.5 What Exists vs What's Missing

**Exists today:**

- `EvictableCache` in `deriva-core` — in-memory LRU cache with
  cost-aware eviction, keyed by `CAddr`, stores full `Bytes` values.
  Supports `get()`, `put()`, `remove()`, `remove_batch()`, `pin()`.
- `BlobStore` in `deriva-storage` — on-disk blob storage keyed by
  `CAddr`. Stores full files at `root/{hex[0:2]}/{hex[2:4]}/{hex}`.
  Supports `put()`, `get()`, `remove()`, `list_addrs()`.
- `SharedCache` in `deriva-compute` — async wrapper around
  `EvictableCache` with `RwLock` for concurrent access.
- `AsyncMaterializationCache` trait — async `get()`/`put()`/`contains()`
  interface used by `StreamingExecutor`.
- `StreamingExecutor::materialize_streaming()` — checks cache before
  building pipeline; loads full `Bytes` on cache hit.
- GC in `deriva-server` — sweeps orphaned blobs and cache entries
  based on live set from DAG.
- `collect_stream()` helper — drains `Receiver<StreamChunk>` into
  a single `Bytes` for cache-after-collect.

**Missing:**

1. **ChunkAddr type** — no sub-addressing model for individual chunks
   within a cached value. Current cache key is `CAddr` (full value).
2. **ChunkManifest type** — no metadata structure tracking the ordered
   sequence of chunks for a cached value.
3. **ChunkCacheWriter** — no streaming cache writer that stores chunks
   incrementally as they arrive from the pipeline.
4. **ChunkCacheReader** — no streaming cache reader that re-streams
   chunks from storage, or reads a byte range from cached chunks.
5. **Range read support** — no way to read a byte range from a cached
   value without loading the entire value.
6. **Partial cache hit** — no mechanism to detect which chunks are
   cached and re-execute only for missing chunks.
7. **GC integration** — GC must be updated to evict all chunks +
   manifest atomically when a `CAddr` is garbage collected.
8. **Migration path** — no coexistence strategy for old full-value
   cache entries and new chunk-level cache entries.
9. **Observability** — no metrics for chunk cache writes, reads,
   range reads, or manifest sizes.
10. **`AsyncMaterializationCache` extension** — the trait has no
    streaming `put_stream()` or `get_stream()` methods; only
    monolithic `Bytes` operations.

---

## 2. Design

### 2.1 Architecture Overview

Chunk-level caching introduces a two-tier storage model: individual
chunks are stored as separate blobs, and a manifest tracks the ordered
sequence of chunks for each `CAddr`. The cache write happens
incrementally as chunks flow through the pipeline — no full-value
buffering required.

```
  Pipeline output                     Chunk-Level Cache
  ──────────────                      ─────────────────

  chunk₁ (64 KB) ──► ChunkCacheWriter ──► BlobStore.put(chunk_key₁, chunk₁)
  chunk₂ (64 KB) ──►                  ──► BlobStore.put(chunk_key₂, chunk₂)
  chunk₃ (64 KB) ──►                  ──► BlobStore.put(chunk_key₃, chunk₃)
  End             ──►                  ──► BlobStore.put(manifest_key, manifest)

  manifest = {
    caddr: CAddr,
    total_size: 192 KB,
    chunks: [
      { offset: 0,      length: 65536, blob_key: chunk_key₁ },
      { offset: 65536,  length: 65536, blob_key: chunk_key₂ },
      { offset: 131072, length: 65536, blob_key: chunk_key₃ },
    ]
  }
```

Cache reads can operate in three modes:

```
  Full read:    manifest → read all chunks → concatenate → Bytes
  Stream read:  manifest → read chunks sequentially → Receiver<StreamChunk>
  Range read:   manifest → identify overlapping chunks → read subset → slice
```

When `cache_intermediates` is enabled and the output is below a
configurable threshold (`chunk_cache_threshold`, default 1 MB), the
existing cache-after-collect path is used. Above the threshold, the
chunk-level cache path activates. This avoids the overhead of manifest
management for small values where monolithic caching is efficient.

### 2.2 Sub-Addressing Model

Each chunk within a cached value is identified by a `ChunkAddr`:

```
  ChunkAddr = (CAddr, u64 offset)

  Examples for a 192 KB value cached as 3 × 64 KB chunks:
    (CAddr("abc123..."), 0)       → chunk₁ (bytes 0–65535)
    (CAddr("abc123..."), 65536)   → chunk₂ (bytes 65536–131071)
    (CAddr("abc123..."), 131072)  → chunk₃ (bytes 131072–196607)
```

The `ChunkAddr` is deterministic: given a `CAddr` and an offset, the
chunk key is always the same. This enables:
- Direct chunk lookup without reading the manifest
- Deduplication if two values share identical chunk content (future)
- Stable blob keys across cache rebuilds

The blob key for storage is derived from the `ChunkAddr`:

```
  blob_key = hash(caddr || offset)   // or caddr.to_hex() + "_" + offset.to_string()
```

The manifest itself is stored at a well-known key derived from the
`CAddr`:

```
  manifest_key = hash(caddr || "manifest")  // or caddr.to_hex() + "_manifest"
```

### 2.3 Chunk Manifest

The manifest is a small metadata blob that describes the chunk
sequence for a cached value:

```
  ChunkManifest {
    caddr: CAddr,                    // the value this manifest describes
    total_size: u64,                 // total bytes across all chunks
    chunk_size: u32,                 // chunk size used during caching
    chunks: Vec<ChunkEntry>,         // ordered, contiguous, non-overlapping
  }

  ChunkEntry {
    offset: u64,                     // byte offset within the full value
    length: u32,                     // chunk size in bytes (last chunk may be smaller)
    blob_key: BlobKey,               // key in BlobStore for this chunk
  }
```

Invariants:
- `chunks` is sorted by `offset` (ascending)
- Chunks are contiguous: `chunks[i+1].offset == chunks[i].offset + chunks[i].length`
- `sum(chunks[i].length) == total_size`
- `chunks[0].offset == 0`

Manifest size: ~20 bytes per chunk entry (8 offset + 4 length + 8 blob_key).
For a 4 GB value at 64 KB chunks: 65,536 entries × 20 bytes = 1.3 MB.
For a 100 MB value at 64 KB chunks: 1,600 entries × 20 bytes = 32 KB.

Serialization: bincode or MessagePack for compact binary encoding.
The manifest is small enough to store as a single blob.

### 2.4 Cache Write Path

The `ChunkCacheWriter` sits between the pipeline output and the
downstream consumer. It tees each chunk to both the consumer and
the blob store:

```
  Pipeline output                ChunkCacheWriter              Consumer
  ──────────────                 ────────────────               ────────
  rx: Receiver<StreamChunk>  →   for each Data(chunk):         tx: Sender<StreamChunk>
                                   1. Compute blob_key from (caddr, offset)
                                   2. BlobStore.put(blob_key, chunk)
                                   3. Append ChunkEntry to manifest builder
                                   4. Forward chunk to consumer tx
                                   5. offset += chunk.len()
                                 on End:
                                   6. Serialize manifest
                                   7. BlobStore.put(manifest_key, manifest)
                                   8. Forward End to consumer tx
                                 on Error:
                                   9. Discard partial manifest (no commit)
                                   10. Clean up any chunks already written
                                   11. Forward Error to consumer tx
```

Key properties:
- **No buffering** — each chunk is written to blob store immediately
  and forwarded to the consumer. Peak memory = 1 chunk.
- **Atomic commit** — the manifest is written only after all chunks
  succeed. If the stream errors mid-way, partial chunks are cleaned
  up and no manifest is written. A missing manifest means the value
  is not cached (chunks without manifest are orphans for GC).
- **Transparent** — the consumer sees the same stream as without
  caching. The writer is a pass-through tee.

### 2.5 Cache Read Path — Full Value

For backward compatibility, a full-value read concatenates all chunks:

```
  1. Read manifest from BlobStore.get(manifest_key)
  2. If manifest missing → cache miss
  3. Allocate BytesMut with capacity = manifest.total_size
  4. For each ChunkEntry in manifest.chunks:
       a. BlobStore.get(entry.blob_key)
       b. If missing → partial corruption → treat as cache miss
       c. Append to BytesMut
  5. Return BytesMut.freeze() as Bytes
```

This path is used by the existing `AsyncMaterializationCache::get()`
interface for backward compatibility. It loads the full value into
memory — same as current behavior, but the data is stored as chunks
on disk.

### 2.6 Cache Read Path — Stream Read

The streaming read path avoids loading the full value into memory:

```
  1. Read manifest from BlobStore.get(manifest_key)
  2. If manifest missing → cache miss
  3. Create mpsc::channel(capacity)
  4. Spawn task:
       for each ChunkEntry in manifest.chunks:
         a. BlobStore.get(entry.blob_key)
         b. If missing → send Error → return
         c. tx.send(StreamChunk::Data(chunk))
       tx.send(StreamChunk::End)
  5. Return Receiver<StreamChunk>
```

This is the preferred path for `StreamingExecutor`. Instead of loading
the full cached value and feeding it through `add_cached()` →
`value_to_stream()`, the executor can directly stream from the chunk
cache with bounded memory.

New trait method on `AsyncMaterializationCache`:

```rust
async fn get_stream(
    &self,
    addr: &CAddr,
    chunk_size: usize,
    capacity: usize,
) -> Option<mpsc::Receiver<StreamChunk>>;
```

### 2.7 Cache Read Path — Range Read

Range reads identify the minimal set of chunks that overlap the
requested byte range:

```
  Request: read bytes [offset, offset + length) from CAddr

  1. Read manifest
  2. Binary search for first chunk where chunk.offset + chunk.length > offset
  3. Iterate chunks until chunk.offset >= offset + length
  4. Read only those chunks from BlobStore
  5. Slice first chunk: skip (offset - first_chunk.offset) leading bytes
  6. Slice last chunk: truncate to (offset + length - last_chunk.offset) bytes
  7. Concatenate sliced chunks → return Bytes

  Example: range [100_000, 200_000) from a value with 64 KB chunks:
    chunk₁: [0, 65536)       — overlaps (contains byte 100_000)
    chunk₂: [65536, 131072)  — overlaps (contains bytes up to 131072)
    chunk₃: [131072, 196608) — overlaps (contains bytes up to 196608)
    chunk₄: [196608, 262144) — overlaps (contains byte 200_000)

    Read 4 chunks (256 KB) instead of full value.
    Slice: skip 100_000 bytes from chunk₁, truncate chunk₄ at byte 200_000.
    Return: 100_000 bytes.
```

For large values, range reads are dramatically more efficient:

| Value Size | Range Size | Chunks Read | Data Read | vs Full Read |
|-----------|-----------|-------------|-----------|-------------|
| 100 MB | 64 KB | 1–2 | 64–128 KB | 0.06–0.13% |
| 1 GB | 1 MB | 16–17 | 1–1.1 MB | 0.1% |
| 4 GB | 64 KB | 1–2 | 64–128 KB | 0.002% |

### 2.8 Partial Cache Hit (Future Extension)

When GC evicts some chunks but not all (e.g., due to a bug or
interrupted eviction), the manifest may reference missing chunks.
A partial cache hit detects this and re-executes only for the
missing portions:

```
  1. Read manifest
  2. For each ChunkEntry, check BlobStore.contains(blob_key)
  3. Partition into present_chunks and missing_chunks
  4. If all present → full cache hit (stream from cache)
  5. If all missing → full cache miss (re-execute pipeline)
  6. If mixed → partial hit:
       a. Re-execute pipeline
       b. For present chunks: skip pipeline output, serve from cache
       c. For missing chunks: use pipeline output, write to cache
       d. Merge into output stream in offset order
```

This is complex and deferred to a future iteration. The initial
implementation treats any missing chunk as a full cache miss (step 5).
The manifest is deleted and the value is re-cached from scratch.

### 2.9 GC Integration

GC must evict all chunks + manifest atomically for a given `CAddr`:

```
  Current GC (per-CAddr):
    1. cache.remove(addr)        → removes EvictableCache entry
    2. blobs.remove(addr)        → removes single blob file

  Chunk-level GC (per-CAddr):
    1. cache.remove(addr)        → removes EvictableCache entry (if any)
    2. Read manifest from BlobStore.get(manifest_key(addr))
    3. If manifest exists:
         a. For each ChunkEntry: BlobStore.remove(entry.blob_key)
         b. BlobStore.remove(manifest_key)
    4. Else: BlobStore.remove(addr)  → legacy single-blob removal
```

Step 4 handles backward compatibility: old entries stored as single
blobs are removed by the existing path. New entries stored as chunks
are removed via the manifest.

Orphan cleanup: chunks without a manifest (from interrupted writes)
are cleaned up by a periodic sweep that identifies blob keys matching
the chunk key pattern but lacking a corresponding manifest.

### 2.10 Migration Path

Old full-value cache entries and new chunk-level entries coexist:

```
  Cache lookup order:
    1. Check EvictableCache (in-memory) → full Bytes hit (existing path)
    2. Check BlobStore for manifest_key(addr) → chunk-level hit
    3. Check BlobStore for addr → legacy full-blob hit
    4. Cache miss → execute pipeline

  Cache write:
    - output_size < chunk_cache_threshold → cache-after-collect (existing)
    - output_size ≥ chunk_cache_threshold → chunk-level cache (new)
    - output_size unknown (streaming) → always chunk-level cache
```

The threshold is configurable via `PipelineConfig::chunk_cache_threshold`
(default 1 MB). Values below the threshold use the simpler monolithic
path. Values above (or streaming outputs where size is unknown upfront)
use chunk-level caching.

Over time, as old entries are evicted and new entries are written,
the cache naturally migrates to chunk-level storage. No explicit
migration step is needed.

### 2.11 Interaction with Other Sections

| Section | Interaction |
|---------|-------------|
| §2.7 Streaming Materialization | `ChunkCacheWriter` integrates into the pipeline output path; `get_stream()` provides streaming cache reads |
| §2.8 Garbage Collection | GC must read manifest to enumerate chunk blob keys; atomic eviction of all chunks + manifest |
| §2.10 Adaptive Chunk Sizing | Chunks may vary in size if adaptive chunking is active; manifest records actual `length` per chunk, not assumed uniform size |
| §2.11 Pipeline Fusion | No direct interaction — fusion operates on pipeline stages, caching operates on pipeline output |
| §2.12 Memory Budget | Chunk cache write respects budget — `ChunkCacheWriter` is a pass-through tee, no additional buffering beyond one chunk |
| §2.9 Size-Aware Mode Selection | Batch mode produces full `Bytes` — uses existing monolithic cache path; streaming mode uses chunk-level cache |

---

## 3. Implementation

### 3.1 ChunkAddr Type

New in `crates/deriva-core/src/chunk_addr.rs`:

```rust
use crate::address::CAddr;
use serde::{Serialize, Deserialize};

/// Identifies a single chunk within a cached value.
///
/// The combination of `caddr` (which value) and `offset` (which byte
/// position) uniquely identifies a chunk. The blob key for storage
/// is derived deterministically from these two fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkAddr {
    pub caddr: CAddr,
    pub offset: u64,
}

impl ChunkAddr {
    pub fn new(caddr: CAddr, offset: u64) -> Self {
        Self { caddr, offset }
    }

    /// Derive a blob key for storing this chunk.
    /// Format: "{caddr_hex}_chunk_{offset}" — deterministic and human-readable.
    pub fn blob_key(&self) -> String {
        format!("{}_chunk_{}", self.caddr.to_hex(), self.offset)
    }
}

/// Derive the manifest blob key for a CAddr.
pub fn manifest_key(caddr: &CAddr) -> String {
    format!("{}_manifest", caddr.to_hex())
}
```

### 3.2 ChunkManifest Type

New in `crates/deriva-core/src/chunk_manifest.rs`:

```rust
use crate::address::CAddr;
use serde::{Serialize, Deserialize};

/// Describes one chunk in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkEntry {
    pub offset: u64,
    pub length: u32,
    pub blob_key: String,
}

/// Ordered manifest of all chunks for a cached value.
///
/// Invariants:
/// - `chunks` sorted by `offset` ascending
/// - Contiguous: `chunks[i+1].offset == chunks[i].offset + chunks[i].length`
/// - `chunks[0].offset == 0`
/// - `sum(length) == total_size`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkManifest {
    pub caddr: CAddr,
    pub total_size: u64,
    pub chunk_size: u32,
    pub chunks: Vec<ChunkEntry>,
}

impl ChunkManifest {
    /// Create a new empty manifest builder.
    pub fn builder(caddr: CAddr, chunk_size: u32) -> ChunkManifestBuilder {
        ChunkManifestBuilder {
            caddr,
            chunk_size,
            total_size: 0,
            chunks: Vec::new(),
        }
    }

    /// Serialize to bytes (bincode).
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self).map_err(|e| e.to_string())
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        bincode::deserialize(data).map_err(|e| e.to_string())
    }

    /// Find chunks overlapping the byte range [offset, offset + length).
    /// Returns a slice of ChunkEntry references.
    pub fn chunks_for_range(&self, offset: u64, length: u64) -> &[ChunkEntry] {
        if self.chunks.is_empty() || length == 0 {
            return &[];
        }
        let end = offset + length;

        // Binary search for first chunk that ends after `offset`.
        let start_idx = self.chunks.partition_point(|c| c.offset + c.length as u64 <= offset);

        // Linear scan for last chunk that starts before `end`.
        let end_idx = self.chunks[start_idx..]
            .partition_point(|c| c.offset < end)
            + start_idx;

        &self.chunks[start_idx..end_idx]
    }

    /// Validate manifest invariants. Returns Ok(()) or error description.
    pub fn validate(&self) -> Result<(), String> {
        if self.chunks.is_empty() && self.total_size == 0 {
            return Ok(());
        }
        if self.chunks.is_empty() {
            return Err("non-zero total_size with empty chunks".into());
        }
        if self.chunks[0].offset != 0 {
            return Err(format!("first chunk offset {} != 0", self.chunks[0].offset));
        }
        let mut expected_offset = 0u64;
        let mut sum = 0u64;
        for (i, chunk) in self.chunks.iter().enumerate() {
            if chunk.offset != expected_offset {
                return Err(format!(
                    "chunk[{}] offset {} != expected {}",
                    i, chunk.offset, expected_offset
                ));
            }
            expected_offset += chunk.length as u64;
            sum += chunk.length as u64;
        }
        if sum != self.total_size {
            return Err(format!("sum {} != total_size {}", sum, self.total_size));
        }
        Ok(())
    }
}

/// Incrementally builds a ChunkManifest as chunks arrive.
pub struct ChunkManifestBuilder {
    caddr: CAddr,
    chunk_size: u32,
    total_size: u64,
    chunks: Vec<ChunkEntry>,
}

impl ChunkManifestBuilder {
    /// Record a chunk that was written to blob store.
    pub fn add_chunk(&mut self, length: u32, blob_key: String) {
        let offset = self.total_size;
        self.chunks.push(ChunkEntry { offset, length, blob_key });
        self.total_size += length as u64;
    }

    /// Finalize into a ChunkManifest.
    pub fn build(self) -> ChunkManifest {
        ChunkManifest {
            caddr: self.caddr,
            total_size: self.total_size,
            chunk_size: self.chunk_size,
            chunks: self.chunks,
        }
    }

    /// Number of chunks recorded so far.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Blob keys recorded so far (for cleanup on error).
    pub fn blob_keys(&self) -> Vec<String> {
        self.chunks.iter().map(|c| c.blob_key.clone()).collect()
    }
}
```

### 3.3 ChunkCacheWriter

New in `crates/deriva-compute/src/chunk_cache.rs`:

```rust
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::address::CAddr;
use deriva_core::chunk_addr::{ChunkAddr, manifest_key};
use deriva_core::chunk_manifest::ChunkManifest;
use deriva_core::streaming::StreamChunk;
use deriva_storage::blob_store::BlobStore;
use crate::metrics;

/// Tees pipeline output to both a consumer channel and the chunk cache.
///
/// Each Data chunk is written to BlobStore immediately. On End, the
/// manifest is committed. On Error, partial chunks are cleaned up.
pub struct ChunkCacheWriter {
    caddr: CAddr,
    chunk_size: u32,
    blob_store: BlobStore,
}

impl ChunkCacheWriter {
    pub fn new(caddr: CAddr, chunk_size: u32, blob_store: BlobStore) -> Self {
        Self { caddr, chunk_size, blob_store }
    }

    /// Wrap a pipeline output stream, teeing chunks to blob store.
    /// Returns a new Receiver that the consumer reads from.
    pub fn wrap(
        self,
        mut input: mpsc::Receiver<StreamChunk>,
        capacity: usize,
    ) -> mpsc::Receiver<StreamChunk> {
        let (tx, rx) = mpsc::channel(capacity);

        tokio::spawn(async move {
            let mut builder = ChunkManifest::builder(self.caddr, self.chunk_size);

            loop {
                match input.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        let chunk_addr = ChunkAddr::new(self.caddr, builder.len() as u64
                            * self.chunk_size as u64);
                        // Correct: use actual running offset
                        let blob_key = ChunkAddr::new(
                            self.caddr,
                            builder.chunks.iter()
                                .map(|c| c.length as u64)
                                .sum::<u64>(),
                        ).blob_key();

                        // Write chunk to blob store
                        match self.blob_store.put_by_key(&blob_key, &chunk) {
                            Ok(_) => {
                                builder.add_chunk(chunk.len() as u32, blob_key);
                                metrics::CHUNK_CACHE_BYTES_WRITTEN
                                    .inc_by(chunk.len() as u64);
                                if tx.send(StreamChunk::Data(chunk)).await.is_err() {
                                    // Consumer dropped — clean up partial chunks
                                    self.cleanup_partial(&builder);
                                    return;
                                }
                            }
                            Err(e) => {
                                // Blob write failed — forward error, clean up
                                tracing::warn!(
                                    caddr = %self.caddr,
                                    error = %e,
                                    "chunk cache write failed; skipping cache"
                                );
                                self.cleanup_partial(&builder);
                                let _ = tx.send(StreamChunk::Error(e.to_string())).await;
                                return;
                            }
                        }
                    }

                    Some(StreamChunk::End) => {
                        // Commit manifest
                        let manifest = builder.build();
                        match manifest.to_bytes() {
                            Ok(manifest_bytes) => {
                                let mkey = manifest_key(&self.caddr);
                                if let Err(e) = self.blob_store
                                    .put_by_key(&mkey, &manifest_bytes)
                                {
                                    tracing::warn!(
                                        caddr = %self.caddr,
                                        error = %e,
                                        "manifest write failed"
                                    );
                                    // Chunks are written but manifest missing —
                                    // they become orphans for GC. Not fatal.
                                }
                                metrics::CHUNK_CACHE_WRITES_TOTAL.inc();
                                metrics::CHUNK_CACHE_MANIFEST_SIZE
                                    .observe(manifest_bytes.len() as f64);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    caddr = %self.caddr,
                                    error = %e,
                                    "manifest serialization failed"
                                );
                            }
                        }
                        let _ = tx.send(StreamChunk::End).await;
                        return;
                    }

                    Some(StreamChunk::Error(e)) => {
                        // Clean up partial chunks, forward error
                        self.cleanup_partial_from_manifest(&builder);
                        let _ = tx.send(StreamChunk::Error(e)).await;
                        return;
                    }

                    None => {
                        // Input channel closed unexpectedly
                        self.cleanup_partial_from_manifest(&builder);
                        return;
                    }
                }
            }
        });

        rx
    }

    fn cleanup_partial(&self, builder: &ChunkManifestBuilder) {
        self.cleanup_partial_from_manifest(builder);
    }

    fn cleanup_partial_from_manifest(&self, builder: &ChunkManifestBuilder) {
        for key in builder.blob_keys() {
            if let Err(e) = self.blob_store.remove_by_key(&key) {
                tracing::debug!(key = %key, error = %e, "failed to clean up partial chunk");
            }
        }
    }
}
```

### 3.4 ChunkCacheReader

Also in `crates/deriva-compute/src/chunk_cache.rs`:

```rust
/// Reads cached values from chunk-level storage.
pub struct ChunkCacheReader {
    blob_store: BlobStore,
}

impl ChunkCacheReader {
    pub fn new(blob_store: BlobStore) -> Self {
        Self { blob_store }
    }

    /// Read manifest for a CAddr. Returns None if not chunk-cached.
    pub fn read_manifest(&self, caddr: &CAddr) -> Option<ChunkManifest> {
        let mkey = manifest_key(caddr);
        let data = self.blob_store.get_by_key(&mkey).ok()??;
        ChunkManifest::from_bytes(&data).ok()
    }

    /// Full read: concatenate all chunks into a single Bytes.
    pub fn read_full(&self, caddr: &CAddr) -> Option<Bytes> {
        let manifest = self.read_manifest(caddr)?;
        let mut buf = Vec::with_capacity(manifest.total_size as usize);

        for entry in &manifest.chunks {
            let chunk = self.blob_store.get_by_key(&entry.blob_key).ok()??;
            if chunk.len() != entry.length as usize {
                tracing::warn!(
                    caddr = %caddr,
                    expected = entry.length,
                    actual = chunk.len(),
                    "chunk size mismatch"
                );
                return None; // Treat as cache miss
            }
            buf.extend_from_slice(&chunk);
        }

        metrics::CHUNK_CACHE_READS_TOTAL.inc();
        Some(Bytes::from(buf))
    }

    /// Stream read: return chunks as a streaming channel.
    pub fn read_stream(
        &self,
        caddr: &CAddr,
        capacity: usize,
    ) -> Option<mpsc::Receiver<StreamChunk>> {
        let manifest = self.read_manifest(caddr)?;
        let blob_store = self.blob_store.clone();
        let caddr = *caddr;

        let (tx, rx) = mpsc::channel(capacity);

        tokio::spawn(async move {
            for entry in &manifest.chunks {
                match blob_store.get_by_key(&entry.blob_key) {
                    Ok(Some(chunk)) => {
                        if tx.send(StreamChunk::Data(chunk)).await.is_err() {
                            return; // Consumer dropped
                        }
                    }
                    Ok(None) => {
                        tracing::warn!(
                            caddr = %caddr,
                            blob_key = %entry.blob_key,
                            "chunk missing from blob store"
                        );
                        let _ = tx.send(StreamChunk::Error(
                            "chunk cache corruption: missing chunk".into()
                        )).await;
                        return;
                    }
                    Err(e) => {
                        let _ = tx.send(StreamChunk::Error(e.to_string())).await;
                        return;
                    }
                }
            }
            metrics::CHUNK_CACHE_READS_TOTAL.inc();
            let _ = tx.send(StreamChunk::End).await;
        });

        Some(rx)
    }

    /// Range read: read only chunks overlapping [offset, offset + length).
    pub fn read_range(
        &self,
        caddr: &CAddr,
        offset: u64,
        length: u64,
    ) -> Option<Bytes> {
        let manifest = self.read_manifest(caddr)?;
        let entries = manifest.chunks_for_range(offset, length);

        if entries.is_empty() {
            return Some(Bytes::new());
        }

        let end = offset + length;
        let mut buf = Vec::with_capacity(length as usize);

        for entry in entries {
            let chunk = self.blob_store.get_by_key(&entry.blob_key).ok()??;
            let chunk_start = entry.offset;
            let chunk_end = entry.offset + entry.length as u64;

            // Compute slice within this chunk
            let slice_start = if offset > chunk_start {
                (offset - chunk_start) as usize
            } else {
                0
            };
            let slice_end = if end < chunk_end {
                (end - chunk_start) as usize
            } else {
                entry.length as usize
            };

            buf.extend_from_slice(&chunk[slice_start..slice_end]);
        }

        metrics::CHUNK_CACHE_RANGE_READS_TOTAL.inc();
        Some(Bytes::from(buf))
    }

    /// Check if a CAddr has a chunk-level cache entry.
    pub fn has_manifest(&self, caddr: &CAddr) -> bool {
        let mkey = manifest_key(caddr);
        self.blob_store.contains_key(&mkey)
    }

    /// Remove all chunks + manifest for a CAddr.
    /// Returns (chunks_removed, bytes_removed).
    pub fn remove(&self, caddr: &CAddr) -> (u64, u64) {
        let manifest = match self.read_manifest(caddr) {
            Some(m) => m,
            None => return (0, 0),
        };

        let mut count = 0u64;
        let mut bytes = 0u64;

        for entry in &manifest.chunks {
            match self.blob_store.remove_by_key(&entry.blob_key) {
                Ok(size) => {
                    count += 1;
                    bytes += size;
                }
                Err(e) => {
                    tracing::debug!(
                        blob_key = %entry.blob_key,
                        error = %e,
                        "failed to remove chunk"
                    );
                }
            }
        }

        // Remove manifest itself
        let mkey = manifest_key(caddr);
        let _ = self.blob_store.remove_by_key(&mkey);

        (count, bytes)
    }
}
```

### 3.5 BlobStore Extension — Key-Based Operations

The existing `BlobStore` is keyed by `CAddr`. Chunk-level caching
needs key-based operations (arbitrary string keys). Add to
`crates/deriva-storage/src/blob_store.rs`:

```rust
impl BlobStore {
    // ... existing CAddr-based methods ...

    /// Store a blob by arbitrary string key.
    pub fn put_by_key(&self, key: &str, data: &[u8]) -> Result<u64> {
        let path = self.key_path(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| DerivaError::Storage(e.to_string()))?;
        }
        fs::write(&path, data)
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(data.len() as u64)
    }

    /// Read a blob by arbitrary string key.
    pub fn get_by_key(&self, key: &str) -> Result<Option<Bytes>> {
        let path = self.key_path(key);
        match fs::read(&path) {
            Ok(data) => Ok(Some(Bytes::from(data))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(DerivaError::Storage(e.to_string())),
        }
    }

    /// Check if a key exists.
    pub fn contains_key(&self, key: &str) -> bool {
        self.key_path(key).exists()
    }

    /// Remove a blob by key. Returns bytes removed (0 if not found).
    pub fn remove_by_key(&self, key: &str) -> Result<u64> {
        let path = self.key_path(key);
        match fs::metadata(&path) {
            Ok(meta) => {
                let size = meta.len();
                fs::remove_file(&path)
                    .map_err(|e| DerivaError::Storage(e.to_string()))?;
                Ok(size)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(DerivaError::Storage(e.to_string())),
        }
    }

    /// Path for a string key. Uses first 4 chars as directory sharding.
    fn key_path(&self, key: &str) -> PathBuf {
        let prefix = if key.len() >= 4 { &key[..4] } else { key };
        self.root.join("chunks").join(prefix).join(key)
    }
}
```

Chunk blobs are stored under `root/chunks/` to separate them from
CAddr-keyed blobs under `root/{hex[0:2]}/`.

### 3.6 StreamingExecutor Integration

Modify `StreamingExecutor::materialize_streaming()` to use chunk cache:

```rust
impl StreamingExecutor {
    pub async fn materialize_streaming(
        &self,
        addr: &CAddr,
        dag: &dyn DagReader,
        cache: &dyn AsyncMaterializationCache,
        leaf_store: &dyn AsyncLeafStore,
        registry: &FunctionRegistry,
        blob_store: Option<&BlobStore>,  // NEW parameter
    ) -> Result<mpsc::Receiver<StreamChunk>, DerivaError> {
        // NEW: check chunk cache first (streaming read)
        if let Some(bs) = blob_store {
            let reader = ChunkCacheReader::new(bs.clone());
            if let Some(rx) = reader.read_stream(
                addr,
                self.config.channel_capacity,
            ) {
                tracing::debug!(caddr = %addr, "chunk cache hit (streaming)");
                return Ok(rx);
            }
        }

        // Existing: check monolithic cache
        if let Some(cached_data) = cache.get(addr).await {
            // ... existing path ...
        }

        // ... build and execute pipeline as before ...

        let rx = pipeline.execute().await?;

        // NEW: wrap output with ChunkCacheWriter if blob_store available
        let rx = if let Some(bs) = blob_store {
            if self.config.cache_intermediates {
                let writer = ChunkCacheWriter::new(
                    *addr,
                    self.config.chunk_size as u32,
                    bs.clone(),
                );
                writer.wrap(rx, self.config.channel_capacity)
            } else {
                rx
            }
        } else {
            rx
        };

        Ok(rx)
    }
}
```

The key change: pipeline output is wrapped with `ChunkCacheWriter`
which tees chunks to blob store transparently. The consumer sees the
same stream. No cache-after-collect buffering needed.

### 3.7 GC Integration

Modify `run_gc()` in `deriva-server/src/gc.rs` to handle chunk-level
entries:

```rust
pub async fn run_gc(
    dag: &PersistentDag,
    blobs: &BlobStore,
    cache: &SharedCache,
    pins: &PinSet,
    config: &GcConfig,
) -> Result<GcResult, DerivaError> {
    // ... existing steps 1–4 (compute live set, enumerate, identify orphans) ...

    // 5. Sweep blobs — handle both legacy and chunk-level entries
    let chunk_reader = ChunkCacheReader::new(blobs.clone());

    let (blobs_removed, bytes_reclaimed_blobs) = if config.dry_run {
        // ... existing dry_run logic ...
    } else {
        let mut total_count = 0u64;
        let mut total_bytes = 0u64;

        for addr in &orphaned_blobs {
            // Try chunk-level removal first
            let (chunk_count, chunk_bytes) = chunk_reader.remove(addr);
            if chunk_count > 0 {
                total_count += chunk_count;
                total_bytes += chunk_bytes;
            }
            // Also remove legacy single-blob entry
            let (legacy_count, legacy_bytes) = blobs.remove_batch_blobs(&[*addr])?;
            total_count += legacy_count;
            total_bytes += legacy_bytes;
        }

        (total_count, total_bytes)
    };

    // ... rest unchanged ...
}
```

### 3.8 AsyncMaterializationCache Trait Extension

Add streaming methods to the trait:

```rust
#[async_trait]
pub trait AsyncMaterializationCache: Send + Sync {
    // Existing
    async fn get(&self, addr: &CAddr) -> Option<Bytes>;
    async fn put(&self, addr: CAddr, data: Bytes) -> u64;
    async fn contains(&self, addr: &CAddr) -> bool;

    // NEW: streaming cache operations
    /// Get a cached value as a stream of chunks.
    /// Returns None if not cached. Default: falls back to get() + value_to_stream().
    async fn get_stream(
        &self,
        addr: &CAddr,
        chunk_size: usize,
        capacity: usize,
    ) -> Option<mpsc::Receiver<StreamChunk>> {
        let data = self.get(addr).await?;
        Some(value_to_stream(data, chunk_size, capacity))
    }

    /// Check if a value has a chunk-level cache entry.
    /// Default: false (no chunk cache support).
    async fn has_chunk_cache(&self, addr: &CAddr) -> bool {
        false
    }
}
```

The default implementations ensure backward compatibility — existing
`SharedCache` implementations work without changes. The chunk-level
cache is layered on top via `ChunkCacheReader` in the executor.

---

## 4. Data Flow Diagrams

### 4.1 Chunk Cache Write Path

3-stage pipeline producing 256 KB output (4 × 64 KB chunks).
`ChunkCacheWriter` tees each chunk to blob store and consumer.

```
  Pipeline                  ChunkCacheWriter                BlobStore         Consumer
  ────────                  ────────────────                ─────────         ────────

  t₁: Data(chunk₁, 64KB) ──► compute blob_key₁ ──────────► put(key₁, 64KB)
                              builder.add_chunk(64KB, key₁)
                              forward ─────────────────────────────────────► recv(chunk₁)

  t₂: Data(chunk₂, 64KB) ──► compute blob_key₂ ──────────► put(key₂, 64KB)
                              builder.add_chunk(64KB, key₂)
                              forward ─────────────────────────────────────► recv(chunk₂)

  t₃: Data(chunk₃, 64KB) ──► compute blob_key₃ ──────────► put(key₃, 64KB)
                              builder.add_chunk(64KB, key₃)
                              forward ─────────────────────────────────────► recv(chunk₃)

  t₄: Data(chunk₄, 64KB) ──► compute blob_key₄ ──────────► put(key₄, 64KB)
                              builder.add_chunk(64KB, key₄)
                              forward ─────────────────────────────────────► recv(chunk₄)

  t₅: End ──────────────────► builder.build() → manifest
                              serialize manifest ─────────► put(manifest_key, 80B)
                              forward End ─────────────────────────────────► recv(End)

  BlobStore contents after t₅:
    chunks/abc1/abc123..._chunk_0        (64 KB)
    chunks/abc1/abc123..._chunk_65536    (64 KB)
    chunks/abc1/abc123..._chunk_131072   (64 KB)
    chunks/abc1/abc123..._chunk_196608   (64 KB)
    chunks/abc1/abc123..._manifest       (80 B)

  Peak memory: 1 × 64 KB (one chunk in flight) + manifest builder (~320 B)
  Consumer sees identical stream as without caching.
```

### 4.2 Chunk Cache Full Read

Subsequent request for the same CAddr. `ChunkCacheReader::read_full()`
concatenates all chunks.

```
  ChunkCacheReader                          BlobStore
  ────────────────                          ─────────

  1. get_by_key("abc123..._manifest")  ──► read manifest (80 B)
     deserialize → ChunkManifest {
       total_size: 262144,
       chunks: [
         { offset: 0,      length: 65536, blob_key: "..._chunk_0" },
         { offset: 65536,  length: 65536, blob_key: "..._chunk_65536" },
         { offset: 131072, length: 65536, blob_key: "..._chunk_131072" },
         { offset: 196608, length: 65536, blob_key: "..._chunk_196608" },
       ]
     }

  2. Allocate BytesMut(capacity = 262144)

  3. get_by_key("..._chunk_0")         ──► read chunk₁ (64 KB)
     append to buf

  4. get_by_key("..._chunk_65536")     ──► read chunk₂ (64 KB)
     append to buf

  5. get_by_key("..._chunk_131072")    ──► read chunk₃ (64 KB)
     append to buf

  6. get_by_key("..._chunk_196608")    ──► read chunk₄ (64 KB)
     append to buf

  7. Return Bytes (262144 bytes)

  Memory: O(total_size) — same as current monolithic read.
  Use case: backward compatibility with existing get() API.
```

### 4.3 Chunk Cache Stream Read

Same CAddr, but using `read_stream()` for bounded-memory access.

```
  ChunkCacheReader                    BlobStore              Consumer
  ────────────────                    ─────────              ────────

  1. Read manifest (80 B)

  2. Spawn streaming task:

  t₁: get_by_key("..._chunk_0")  ──► read 64 KB
      tx.send(Data(chunk₁))  ──────────────────────────────► recv(chunk₁)
                                                              process chunk₁
                                                              drop chunk₁

  t₂: get_by_key("..._chunk_65536") ► read 64 KB
      tx.send(Data(chunk₂))  ──────────────────────────────► recv(chunk₂)
                                                              process chunk₂
                                                              drop chunk₂

  t₃: get_by_key("..._chunk_131072") ► read 64 KB
      tx.send(Data(chunk₃))  ──────────────────────────────► recv(chunk₃)

  t₄: get_by_key("..._chunk_196608") ► read 64 KB
      tx.send(Data(chunk₄))  ──────────────────────────────► recv(chunk₄)

  t₅: tx.send(End)  ──────────────────────────────────────► recv(End)

  Peak memory: 1 × 64 KB (channel capacity 1) to capacity × 64 KB
  Use case: StreamingExecutor cache hit — bounded memory, no full load.
```

### 4.4 Chunk Cache Range Read

Request for bytes [100_000, 200_000) from a 4 MB cached value
(64 × 64 KB chunks).

```
  ChunkCacheReader                              BlobStore
  ────────────────                              ─────────

  1. Read manifest (64 entries)

  2. chunks_for_range(100_000, 100_000):
     Binary search: first chunk ending after 100_000
       chunk₁: [0, 65536)       → ends at 65536 ≤ 100_000 → skip
       chunk₂: [65536, 131072)  → ends at 131072 > 100_000 → START here (idx=1)
     Scan forward until chunk.offset ≥ 200_000:
       chunk₂: [65536, 131072)  → offset 65536 < 200_000 → include
       chunk₃: [131072, 196608) → offset 131072 < 200_000 → include
       chunk₄: [196608, 262144) → offset 196608 < 200_000 → include
       chunk₅: [262144, 327680) → offset 262144 ≥ 200_000 → STOP
     Result: chunks [2, 3, 4] (indices 1, 2, 3)

  3. Read 3 chunks:
     get_by_key("..._chunk_65536")   ──► read chunk₂ (64 KB)
       slice: [100000-65536 .. 65536] = [34464 .. 65536] → 31072 bytes

     get_by_key("..._chunk_131072")  ──► read chunk₃ (64 KB)
       slice: [0 .. 65536] → 65536 bytes (full chunk)

     get_by_key("..._chunk_196608")  ──► read chunk₄ (64 KB)
       slice: [0 .. 200000-196608] = [0 .. 3392] → 3392 bytes

  4. Concatenate: 31072 + 65536 + 3392 = 100_000 bytes ✓

  Data read from disk: 3 × 64 KB = 192 KB
  vs full read: 4 MB
  Savings: 98.5%
```

### 4.5 Error During Cache Write — Cleanup

Pipeline errors after writing 2 of 4 chunks. `ChunkCacheWriter`
cleans up partial chunks.

```
  Pipeline                  ChunkCacheWriter                BlobStore
  ────────                  ────────────────                ─────────

  t₁: Data(chunk₁) ────────► put(key₁, 64KB) ────────────► ✓ stored
                              builder.add_chunk(key₁)
                              forward to consumer

  t₂: Data(chunk₂) ────────► put(key₂, 64KB) ────────────► ✓ stored
                              builder.add_chunk(key₂)
                              forward to consumer

  t₃: Error("disk full") ──► cleanup_partial():
                                remove_by_key(key₁) ──────► ✓ removed
                                remove_by_key(key₂) ──────► ✓ removed
                              forward Error to consumer

  BlobStore after cleanup: no chunks, no manifest.
  No orphan data left behind.
  Consumer receives: chunk₁, chunk₂, Error("disk full")
```

### 4.6 GC Eviction of Chunk-Cached Value

GC identifies CAddr as orphaned and removes all chunks + manifest.

```
  GC                        ChunkCacheReader              BlobStore
  ──                        ────────────────              ─────────

  1. Identify orphan: CAddr("abc123...")

  2. cache.remove(addr)     → remove from EvictableCache (if present)

  3. chunk_reader.remove(addr):
     a. read_manifest(addr) → get_by_key("..._manifest")
        → manifest with 4 entries

     b. For each entry:
        remove_by_key("..._chunk_0")       ──► ✓ removed (64 KB)
        remove_by_key("..._chunk_65536")   ──► ✓ removed (64 KB)
        remove_by_key("..._chunk_131072")  ──► ✓ removed (64 KB)
        remove_by_key("..._chunk_196608")  ──► ✓ removed (64 KB)

     c. remove_by_key("..._manifest")      ──► ✓ removed (80 B)

     Return: (4 chunks removed, 262224 bytes reclaimed)

  4. blobs.remove(addr)     → legacy single-blob removal (not found = no-op)

  Total reclaimed: 256 KB data + 80 B manifest
```

### 4.7 Migration — Mixed Cache Lookup

Request for a CAddr that may be cached in either old or new format.

```
  StreamingExecutor                                         Result
  ─────────────────                                         ──────

  1. Check chunk cache (new path):
     ChunkCacheReader.read_stream(addr)
       → read_manifest(addr)
       → manifest_key not found                             → None

  2. Check monolithic cache (existing path):
     cache.get(addr).await
       → EvictableCache lookup
       → Found: Bytes (256 KB)                              → Hit!
     pipeline.add_cached(addr, cached_data)
     → value_to_stream() → Receiver<StreamChunk>

  ─── OR if neither cache has it: ───

  1. Chunk cache: None
  2. Monolithic cache: None
  3. Build and execute pipeline
  4. Wrap output with ChunkCacheWriter                      → Cached as chunks
     (next request will hit chunk cache at step 1)

  Over time: old monolithic entries evicted by LRU,
  new entries written as chunks. Natural migration.
```

---

## 5. Test Specification

### 5.1 ChunkAddr & ChunkManifest Core (6 tests)

**T1 — ChunkAddr blob_key is deterministic and unique per offset**
Create `ChunkAddr::new(caddr, 0)` and `ChunkAddr::new(caddr, 65536)`.
Assert `blob_key()` values differ. Create same `ChunkAddr::new(caddr, 0)`
again. Assert `blob_key()` matches the first. Create with different
`CAddr` but same offset. Assert `blob_key()` differs. Verifies
deterministic, collision-free key derivation.

**T2 — ChunkManifest builder produces valid manifest**
Build manifest for 1 MB value at 64 KB chunks (16 entries). Call
`builder.add_chunk(65536, key)` 16 times. Call `build()`. Assert
`total_size == 1_048_576`. Assert `chunks.len() == 16`. Assert
`chunks[0].offset == 0`, `chunks[15].offset == 983040`. Call
`validate()` — assert `Ok(())`. Verifies incremental building
produces correct contiguous layout.

**T3 — ChunkManifest serialization roundtrip preserves all fields**
Build manifest with 4 entries of varying lengths (65536, 65536, 65536,
32768 — last chunk smaller). Serialize via `to_bytes()`. Deserialize
via `from_bytes()`. Assert deserialized manifest equals original
(caddr, total_size, chunk_size, all chunk entries). Verifies bincode
encoding handles variable-length last chunk.

**T4 — chunks_for_range returns correct subset for various ranges**
Build manifest for 512 KB value (8 × 64 KB chunks). Test 6 ranges:
- `(0, 65536)` → 1 chunk (first only)
- `(0, 512 * 1024)` → 8 chunks (entire value)
- `(100_000, 1)` → 1 chunk (chunk₂, containing byte 100_000)
- `(65535, 2)` → 2 chunks (spans chunk₁/chunk₂ boundary)
- `(500_000, 12_288)` → 1 chunk (last chunk, partial)
- `(0, 0)` → 0 chunks (zero-length range)
Assert each returns the expected slice of entries. Verifies binary
search correctness at boundaries.

**T5 — ChunkManifest validate catches invariant violations**
Create manifests with deliberate errors:
- Non-zero first offset → assert `validate()` returns Err
- Gap between chunks (offset 0, length 100, next offset 200) → Err
- Overlap (offset 0, length 100, next offset 50) → Err
- Sum mismatch (total_size != sum of lengths) → Err
- Empty chunks with non-zero total_size → Err
- Valid empty manifest (0 chunks, 0 total_size) → Ok
Verifies all 5 invariant checks catch corruption.

**T6 — manifest_key is distinct from any chunk blob_key**
For a given CAddr, compute `manifest_key(caddr)` and
`ChunkAddr::new(caddr, offset).blob_key()` for offsets 0, 65536,
131072. Assert manifest key does not equal any chunk key. Assert
manifest key contains "manifest" substring. Verifies no key collision
between manifest and chunk blobs.

### 5.2 ChunkCacheWriter (7 tests)

**T7 — Writer stores all chunks and manifest for complete stream**
Create `ChunkCacheWriter` with a temp `BlobStore`. Feed 256 KB as
4 × 64 KB chunks + End through `wrap()`. Collect consumer output.
Assert consumer received 4 Data chunks + End (identical to input).
Assert blob store contains 4 chunk blobs + 1 manifest blob. Read
manifest, assert 4 entries with correct offsets and lengths. Read
each chunk blob, assert content matches original chunks.

**T8 — Writer produces byte-identical consumer output as raw stream**
Run same 3-stage pipeline (Upper → Lower → Xor) twice on 1 MB input:
once with `ChunkCacheWriter` wrapping output, once without. Collect
both outputs. Assert byte-for-byte equality. Verifies writer is a
transparent pass-through that doesn't alter the stream.

**T9 — Writer cleans up partial chunks on mid-stream error**
Feed 3 Data chunks, then `StreamChunk::Error("test error")`. Assert
consumer receives 3 Data + Error. Assert blob store contains NO chunk
blobs and NO manifest (all cleaned up). Verify by listing all keys
under the chunks directory — assert empty.

**T10 — Writer cleans up when consumer drops receiver mid-stream**
Feed 10 chunks through writer. Consumer reads 3 chunks then drops
the receiver. Wait for writer task to complete (100 ms timeout).
Assert blob store contains NO manifest. Assert partial chunk blobs
are cleaned up (writer detects send failure and calls cleanup).

**T11 — Writer handles empty stream (0 chunks + End)**
Feed only `StreamChunk::End`. Assert consumer receives End. Assert
manifest is written with `total_size == 0` and `chunks == []`. Assert
no chunk blobs in blob store. Verifies empty value is cached correctly
as an empty manifest.

**T12 — Writer handles single-byte chunk**
Feed `StreamChunk::Data(Bytes::from(vec![0x42]))` + End. Assert
manifest has 1 entry with `length == 1`. Assert chunk blob contains
exactly `[0x42]`. Verifies minimum chunk size works.

**T13 — Writer handles adaptive chunk sizes (varying lengths)**
Feed 5 chunks of sizes: 64 KB, 128 KB, 32 KB, 256 KB, 16 KB (total
496 KB). Assert manifest has 5 entries with correct offsets:
0, 65536, 196608, 229376, 485376. Assert each chunk blob has correct
size. Assert `manifest.total_size == 507904`. Verifies non-uniform
chunk sizes from §2.10 adaptive chunking are handled correctly.

### 5.3 ChunkCacheReader — Full & Stream Read (6 tests)

**T14 — Full read concatenates chunks into original value**
Write 1 MB value as 16 × 64 KB chunks via `ChunkCacheWriter`. Then
`ChunkCacheReader::read_full()`. Assert returned `Bytes` equals the
original 1 MB input byte-for-byte. Verifies write→full-read roundtrip.

**T15 — Stream read delivers chunks matching original stream**
Write 512 KB via writer. Then `read_stream()` with capacity 4. Collect
all chunks from returned receiver. Concatenate. Assert equals original
512 KB. Assert received exactly 8 Data chunks + End. Verifies
write→stream-read roundtrip with bounded memory.

**T16 — Stream read with capacity 1 delivers all chunks sequentially**
Write 256 KB via writer. `read_stream()` with capacity 1. Consume
chunks one at a time with 10 ms delay between each. Assert all 4
chunks received correctly. Assert no data loss under slow consumption.
Verifies backpressure works on cache stream reads.

**T17 — Read returns None for non-existent CAddr**
Call `read_full()` and `read_stream()` on a CAddr that was never
cached. Assert both return `None`. Verifies clean cache miss behavior.

**T18 — Read returns None when manifest exists but a chunk is missing**
Write 4 chunks + manifest. Manually delete chunk₂'s blob from blob
store. Call `read_full()`. Assert returns `None` (treated as cache
miss, not partial data). Call `read_stream()`. Assert returns a
receiver that emits chunk₁ then `StreamChunk::Error`. Verifies
corruption detection.

**T19 — Read handles manifest with chunk size mismatch**
Write 4 chunks normally. Manually overwrite chunk₃'s blob with 100
bytes (instead of 65536). Call `read_full()`. Assert returns `None`
(size mismatch detected). Verifies integrity checking per chunk.

### 5.4 ChunkCacheReader — Range Read (5 tests)

**T20 — Range read within a single chunk**
Cache 1 MB value. `read_range(caddr, 10_000, 100)`. Assert returned
Bytes has length 100. Assert content matches bytes 10_000–10_099 of
original value. Only 1 chunk read from disk.

**T21 — Range read spanning chunk boundary**
Cache 256 KB value (4 × 64 KB). `read_range(caddr, 65530, 20)`.
Range spans bytes 65530–65549, crossing chunk₁/chunk₂ boundary.
Assert returned Bytes has length 20. Assert content matches original
bytes 65530–65549. Exactly 2 chunks read.

**T22 — Range read of entire value equals full read**
Cache 512 KB value. `read_range(caddr, 0, 524288)`. Assert returned
Bytes equals `read_full()` result byte-for-byte. Verifies range read
degrades gracefully to full read.

**T23 — Range read at end of value (last partial chunk)**
Cache 500 KB value (7 × 64 KB + 1 × 52 KB). `read_range(caddr,
490_000, 10_000)`. Range extends to byte 500_000 (end of value).
Assert returned Bytes has length 10_000. Assert content matches
original. Verifies last-chunk slicing with non-uniform chunk size.

**T24 — Range read returns empty Bytes for zero-length range**
Cache 256 KB value. `read_range(caddr, 100_000, 0)`. Assert returned
Bytes is empty. No chunks read from disk.

### 5.5 Integration — Pipeline + Cache Roundtrip (5 tests)

**T25 — Pipeline output cached as chunks, next request streams from cache**
Run `StreamingExecutor::materialize_streaming()` for a 3-stage pipeline
(Upper → Xor → Lower) on 2 MB input with blob_store provided. Collect
output (first execution — cache miss, writes chunks). Run same
`materialize_streaming()` again for same CAddr. Assert second execution
returns stream from chunk cache (no pipeline built). Collect second
output. Assert byte-identical to first. Verifies end-to-end cache
write + streaming cache hit.

**T26 — Chunk cache coexists with monolithic EvictableCache**
Put a 64 KB value into `SharedCache` (monolithic) for CAddr_A. Write
a 1 MB value as chunks for CAddr_B. Call `materialize_streaming()` for
CAddr_A — assert served from monolithic cache. Call for CAddr_B —
assert served from chunk cache. Assert both outputs correct. Verifies
migration coexistence.

**T27 — Large pipeline output (16 MB) cached without OOM**
Run 2-stage pipeline (Compress → Decompress) on 16 MB input with
chunk caching enabled. Monitor peak memory (RSS before and after).
Assert peak memory increase < 2 MB (bounded by chunk_size × capacity,
not output size). Assert chunk cache contains manifest with ~256
entries. Assert `read_stream()` returns correct output.

**T28 — Pipeline with cache_intermediates=false skips chunk caching**
Run pipeline with `cache_intermediates: false` and blob_store provided.
Assert no manifest written to blob store. Assert pipeline output is
correct. Verifies config flag is respected.

**T29 — Concurrent pipelines writing to chunk cache for different CAddrs**
Spawn 10 concurrent `materialize_streaming()` calls, each for a
different CAddr, each producing 256 KB output. Wait for all to
complete. Assert all 10 manifests exist in blob store. Assert all 10
outputs are correct. Assert no cross-contamination (each manifest
references only its own chunks). Verifies concurrent cache writes
don't interfere.

### 5.6 GC & Eviction (5 tests)

**T30 — GC removes all chunks + manifest for orphaned CAddr**
Cache a 512 KB value as chunks (8 chunks + manifest). Run GC with
the CAddr marked as orphaned. Assert manifest is gone. Assert all 8
chunk blobs are gone. Assert `read_full()` returns None. Assert
`read_stream()` returns None. Verifies atomic chunk-level eviction.

**T31 — GC handles mixed legacy and chunk-cached entries**
Store CAddr_A as legacy single blob. Store CAddr_B as chunk cache
(4 chunks + manifest). Mark both as orphaned. Run GC. Assert CAddr_A's
single blob removed. Assert CAddr_B's 4 chunks + manifest removed.
Assert blob store is empty. Verifies GC handles both formats.

**T32 — GC skips pinned CAddr even if chunk-cached**
Cache CAddr as chunks. Pin it in PinSet. Run GC. Assert manifest and
all chunks still present. Unpin. Run GC again. Assert all removed.
Verifies pin protection extends to chunk-level entries.

**T33 — Orphan chunk cleanup (chunks without manifest)**
Manually write 3 chunk blobs for a CAddr without writing a manifest
(simulating interrupted write). Run orphan cleanup sweep. Assert the
3 orphan chunk blobs are removed. Verifies cleanup of incomplete
cache writes.

**T34 — GC under concurrent cache write**
Start a slow `ChunkCacheWriter` (10 ms delay per chunk, 8 chunks).
After 4 chunks are written, trigger GC for the same CAddr. Assert
one of: (a) GC finds no manifest → no-op, writer completes and
manifest is written; or (b) GC removes partial chunks, writer detects
blob store errors and cleans up. Either way, no corruption: the CAddr
is either fully cached or not cached at all.

### 5.7 Edge Cases & Error Paths (4 tests)

**T35 — Blob store write failure mid-stream (disk full simulation)**
Create `BlobStore` on a tmpfs with 128 KB limit. Feed 4 × 64 KB
chunks through `ChunkCacheWriter`. First 2 chunks succeed, 3rd fails
(disk full). Assert consumer receives 2 Data chunks + Error. Assert
partial chunks cleaned up. Assert no manifest. Verifies graceful
degradation when storage is exhausted.

**T36 — Manifest deserialization failure (corrupted manifest blob)**
Write valid chunks + manifest. Overwrite manifest blob with random
bytes. Call `read_manifest()`. Assert returns None (deserialization
fails gracefully). Call `read_full()`. Assert returns None. Verifies
corrupted manifest is treated as cache miss, not panic.

**T37 — ChunkCacheWriter with §2.12 memory budget**
Run pipeline with `memory_budget = 256 KB` and chunk caching enabled.
Process 2 MB input. Assert pipeline completes (no deadlock — writer
tee doesn't add extra buffering beyond the budget). Assert chunks
cached correctly. Assert output matches expected. Verifies budget
enforcement and chunk caching coexist.

**T38 — Cache write for value with exactly 1 chunk (chunk_size boundary)**
Input = exactly 64 KB (1 chunk). Pipeline: Identity. Assert manifest
has exactly 1 entry. Assert `read_full()` returns original 64 KB.
Assert `read_range(0, 64 KB)` returns original. Assert
`read_range(0, 32 KB)` returns first half. Verifies single-chunk
edge case for all read paths.

### 5.8 Benchmarks & Performance (2 tests)

**T39 — Chunk cache write overhead vs skip-cache baseline**
Benchmark 3-stage Identity pipeline on 8 MB:
(a) No caching (raw pipeline output)
(b) With `ChunkCacheWriter` teeing to blob store
Measure throughput. Assert (b) is within 80% of (a). The overhead
is blob store I/O per chunk (~100 μs for 64 KB write to SSD). For
compute-heavy pipelines the overhead is negligible; for Identity
pipelines it's the dominant cost. Verifies caching doesn't
catastrophically slow down the pipeline.

**T40 — Range read latency vs full read for large cached value**
Cache 64 MB value as chunks (1024 × 64 KB). Benchmark:
(a) `read_full()` — reads all 1024 chunks, concatenates
(b) `read_range(offset=32MB, length=64KB)` — reads 1 chunk
Assert (b) is at least 100× faster than (a). Verifies range read
provides the expected I/O savings for large values.

### Test Summary

| # | Group | Tests | Focus |
|---|-------|-------|-------|
| 5.1 | ChunkAddr & ChunkManifest | T1–T6 | Key derivation, builder, serialization, range lookup, validation |
| 5.2 | ChunkCacheWriter | T7–T13 | Write path, transparency, error cleanup, edge sizes, adaptive chunks |
| 5.3 | ChunkCacheReader full & stream | T14–T19 | Read roundtrip, backpressure, cache miss, corruption detection |
| 5.4 | ChunkCacheReader range | T20–T24 | Single-chunk, boundary, full-range, last-chunk, zero-length |
| 5.5 | Pipeline + cache integration | T25–T29 | End-to-end roundtrip, coexistence, large output, config, concurrency |
| 5.6 | GC & eviction | T30–T34 | Atomic removal, mixed formats, pins, orphans, concurrent GC+write |
| 5.7 | Edge cases & errors | T35–T38 | Disk full, corrupt manifest, budget interaction, single-chunk |
| 5.8 | Benchmarks | T39–T40 | Write overhead, range read speedup |
| | **Total** | **40** | |

---

## 6. Edge Cases & Error Handling

| # | Scenario | Expected Behavior | Test |
|---|----------|-------------------|------|
| 1 | Empty stream (0 chunks + End) | Manifest written with `total_size=0`, empty `chunks` vec; `read_full()` returns empty `Bytes`; `read_stream()` emits End immediately | T11 |
| 2 | Single-byte chunk | Manifest has 1 entry with `length=1`; all read paths return 1 byte correctly | T12 |
| 3 | Single chunk exactly at `chunk_size` | Manifest has 1 entry; range read of partial range slices correctly | T38 |
| 4 | Last chunk smaller than `chunk_size` | Manifest records actual `length` (not `chunk_size`); `total_size` correct; `validate()` passes | T3 |
| 5 | Adaptive chunking — varying chunk sizes | Manifest entries have different `length` values; offsets remain contiguous; `chunks_for_range()` still correct | T13 |
| 6 | Blob store write failure on first chunk | No chunks stored; no manifest; Error forwarded to consumer; no orphans | T35 |
| 7 | Blob store write failure mid-stream | Partial chunks cleaned up via `cleanup_partial()`; no manifest written; Error forwarded | T9, T35 |
| 8 | Manifest serialization failure | Chunks written but no manifest; chunks become orphans for GC sweep; warning logged; End still forwarded to consumer | — |
| 9 | Manifest write failure (disk full after chunks) | Chunks exist without manifest; treated as cache miss on read; orphan cleanup removes them | T33 |
| 10 | Consumer drops receiver mid-stream | Writer detects `send().is_err()`; cleans up partial chunks; writer task exits | T10 |
| 11 | Pipeline error after N chunks cached | N chunk blobs cleaned up; no manifest; Error forwarded to consumer | T9 |
| 12 | Manifest exists but one chunk blob missing | `read_full()` returns None (cache miss); `read_stream()` emits Data for present chunks then Error | T18 |
| 13 | Chunk blob has wrong size vs manifest entry | `read_full()` returns None (size mismatch); treated as corruption | T19 |
| 14 | Manifest blob corrupted (invalid bincode) | `from_bytes()` returns Err; `read_manifest()` returns None; treated as cache miss | T36 |
| 15 | Manifest with `total_size` mismatch | `validate()` returns Err; reader can optionally validate before serving; treated as corruption | T5 |
| 16 | Range read with offset beyond value end | `chunks_for_range()` returns empty slice; `read_range()` returns empty `Bytes` | T24 |
| 17 | Range read spanning entire value | Returns all chunks concatenated; equivalent to `read_full()` | T22 |
| 18 | Range read at exact chunk boundary | No off-by-one: range `[65536, 65537)` reads only chunk₂, not chunk₁ | T21 |
| 19 | Concurrent cache writes for same CAddr | Last writer's manifest wins; chunks from earlier writes become orphans; no corruption (blob store puts are idempotent for same key) | — |
| 20 | Concurrent cache writes for different CAddrs | Independent manifests and chunk keys; no interference | T29 |
| 21 | GC during active cache write | GC finds no manifest (not yet committed) → no-op; writer completes normally; OR writer's blob writes fail if GC removes chunks mid-write → writer cleans up | T34 |
| 22 | GC removes chunk-cached entry atomically | All chunk blobs + manifest removed; subsequent reads return None | T30 |
| 23 | GC with mixed legacy + chunk entries | Legacy entries removed by `blobs.remove(addr)`; chunk entries removed via manifest traversal | T31 |
| 24 | Pinned CAddr not evicted by GC | Pin check happens before chunk removal; pinned entries preserved | T32 |
| 25 | Orphan chunks without manifest (interrupted write) | Periodic orphan sweep identifies chunk blobs with no corresponding manifest; removes them | T33 |
| 26 | Very large value (4 GB, 65536 chunks) | Manifest ~1.3 MB; writer handles 65536 iterations; `read_stream()` delivers all chunks with bounded memory | T27 (scaled) |
| 27 | `cache_intermediates = false` | `ChunkCacheWriter` not attached; no manifest or chunks written; pipeline output unchanged | T28 |
| 28 | `memory_budget > 0` with chunk caching | Writer tee adds no extra buffering; budget semaphore still controls in-flight chunks; no deadlock | T37 |
| 29 | Blob store on read-only filesystem | `put_by_key()` returns Err; writer logs warning, cleans up, forwards Error; pipeline still delivers output to consumer (caching skipped) | — |
| 30 | `chunk_size = 1` (degenerate) | 1 manifest entry per byte; manifest huge but correct; impractical but no panic | — |

---

## 7. Performance Analysis

### 7.1 Write Path Overhead

Each chunk incurs one blob store write. On SSD-backed storage:

| Operation | Latency | Notes |
|-----------|---------|-------|
| `BlobStore::put_by_key()` (64 KB) | ~100 μs | fsync-less write; OS page cache absorbs |
| `BlobStore::put_by_key()` (64 KB, fsync) | ~500 μs | With durability guarantee |
| Manifest serialization (bincode) | ~1 μs | For 100-entry manifest (~2 KB) |
| Manifest write | ~50 μs | Small blob, single write |
| `ChunkCacheWriter` overhead per chunk | ~5 μs | Key computation, builder bookkeeping, channel forward |

Compared to pipeline per-chunk costs:

| Pipeline Stage | Compute/chunk | Cache Write Overhead | Overhead % |
|---------------|--------------|---------------------|-----------|
| Identity (no-op) | ~0 ns | ~105 μs | ∞ (dominates) |
| Uppercase (64 KB) | ~2 μs | ~105 μs | 5,250% |
| Compress (64 KB) | ~200 μs | ~105 μs | 52% |
| SHA-256 (64 KB) | ~15 μs | ~105 μs | 700% |
| Compress + Encrypt (64 KB) | ~250 μs | ~105 μs | 42% |

The blob store write is the dominant cost. For compute-heavy pipelines
(compress, encrypt), the overhead is 40–50% — significant but
acceptable since caching eliminates re-execution on subsequent
requests. For cheap pipelines (uppercase, identity), the overhead
exceeds compute cost — but these pipelines are fast enough that the
absolute overhead (~100 μs/chunk) is still small.

**Mitigation:** Async blob writes. The writer could buffer chunk data
and write to blob store in a background task, decoupling pipeline
throughput from I/O latency. Deferred to implementation time.

### 7.2 Read Path Overhead

| Read Mode | Latency | Memory | Notes |
|-----------|---------|--------|-------|
| Full read (monolithic, current) | 1 × read(N bytes) | O(N) | Single blob read |
| Full read (chunk cache) | 1 manifest read + K chunk reads | O(N) | K = N / chunk_size |
| Stream read (chunk cache) | 1 manifest read + K sequential reads | O(chunk_size) | Bounded memory |
| Range read (chunk cache) | 1 manifest read + M chunk reads | O(range + chunk_size) | M = ceil(range / chunk_size) + 1 |

Full read comparison for 100 MB value:

| Method | I/O Operations | Latency (SSD) | Latency (HDD) |
|--------|---------------|---------------|---------------|
| Monolithic blob | 1 | ~10 ms | ~200 ms |
| Chunk cache (1,600 chunks) | 1,601 | ~160 ms | ~3,200 ms |
| Chunk cache (parallel, 8 threads) | ~200 batches | ~25 ms | ~500 ms |

Chunk cache full reads are ~16× slower than monolithic due to per-chunk
I/O overhead. This is acceptable because:
1. Stream reads avoid loading the full value (bounded memory).
2. Range reads are dramatically faster for partial access.
3. Full reads are the backward-compatibility path, not the primary use.

### 7.3 Range Read Savings

| Value Size | Range Size | Chunks Read | Data from Disk | vs Full Read |
|-----------|-----------|-------------|---------------|-------------|
| 100 MB | 64 KB | 1–2 | 64–128 KB | 0.06–0.13% |
| 100 MB | 1 MB | 16–17 | 1–1.1 MB | 1.0–1.1% |
| 1 GB | 64 KB | 1–2 | 64–128 KB | 0.006–0.012% |
| 1 GB | 10 MB | 157–158 | 10–10.1 MB | 0.98–0.99% |
| 4 GB | 64 KB | 1–2 | 64–128 KB | 0.002–0.003% |
| 4 GB | 100 MB | 1,563–1,564 | 100–100.1 MB | 2.4% |

Range reads provide 2–5 orders of magnitude I/O reduction for small
ranges on large values. This enables efficient HTTP byte-range serving
and partial value inspection without full materialization.

### 7.4 Storage Overhead

Chunk-level caching adds manifest storage and filesystem metadata:

| Value Size | Chunks | Manifest Size | Filesystem Overhead | Total Overhead |
|-----------|--------|--------------|--------------------|--------------:|
| 64 KB | 1 | ~40 B | ~4 KB (1 inode) | ~6% |
| 1 MB | 16 | ~340 B | ~64 KB (16 inodes) | ~6% |
| 100 MB | 1,600 | ~32 KB | ~6.4 MB (1,600 inodes) | ~6.4% |
| 1 GB | 16,384 | ~328 KB | ~64 MB (16K inodes) | ~6.3% |
| 4 GB | 65,536 | ~1.3 MB | ~256 MB (64K inodes) | ~6.3% |

The ~6% overhead is dominated by filesystem metadata (one inode per
chunk file, typically 4 KB each on ext4). The manifest itself is
negligible (~20 bytes per entry). For production deployments, a
dedicated chunk store with packed storage (multiple chunks per file)
would reduce this to < 0.1%.

### 7.5 Benchmark Study Plan

Location: `deriva-compute/benches/chunk_cache.rs`

Five benchmark groups measuring write overhead, read modes, range
efficiency, scaling, and concurrent access.

#### Benchmark 1: Cache Write Overhead — Cached vs Uncached Pipeline

Measures the throughput impact of `ChunkCacheWriter` teeing chunks
to blob store during pipeline execution.

```rust
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use bytes::Bytes;
use tempfile::TempDir;

fn bench_write_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunk_cache_write_overhead");
    let sizes = [
        256 * 1024,           // 256 KB
        1024 * 1024,          // 1 MB
        8 * 1024 * 1024,      // 8 MB
        32 * 1024 * 1024,     // 32 MB
    ];

    for &size in &sizes {
        let data = Bytes::from(vec![b'x'; size]);
        group.throughput(Throughput::Bytes(size as u64));

        // (a) Pipeline without caching
        group.bench_with_input(
            BenchmarkId::new("no_cache", size / 1024),
            &data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    run_3_stage_identity(data.clone(), PipelineConfig {
                        cache_intermediates: false,
                        ..Default::default()
                    }).await
                });
            },
        );

        // (b) Pipeline with ChunkCacheWriter
        group.bench_with_input(
            BenchmarkId::new("chunk_cached", size / 1024),
            &data,
            |b, data| {
                let tmp = TempDir::new().unwrap();
                let blob_store = BlobStore::open(tmp.path()).unwrap();
                b.to_async(&rt).iter(|| async {
                    let rx = run_3_stage_identity(data.clone(), PipelineConfig {
                        cache_intermediates: true,
                        ..Default::default()
                    }).await;
                    let writer = ChunkCacheWriter::new(
                        test_caddr(),
                        65536,
                        blob_store.clone(),
                    );
                    let rx = writer.wrap(rx, 8);
                    collect_stream(rx).await.unwrap()
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Chunk-cached is 30–60% slower for Identity pipelines
(I/O dominates). For compute-heavy pipelines (compress), overhead
drops to 20–40%. Absolute throughput still > 50 MB/s on SSD.

#### Benchmark 2: Read Mode Comparison — Full vs Stream vs Range

Compares the three read paths on a pre-cached value.

```rust
fn bench_read_modes(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunk_cache_read_modes");
    let sizes = [
        1024 * 1024,          // 1 MB
        16 * 1024 * 1024,     // 16 MB
        64 * 1024 * 1024,     // 64 MB
    ];

    for &size in &sizes {
        let tmp = TempDir::new().unwrap();
        let blob_store = BlobStore::open(tmp.path()).unwrap();
        let reader = ChunkCacheReader::new(blob_store.clone());
        let caddr = pre_cache_value(&blob_store, size); // helper: writes chunks + manifest

        group.throughput(Throughput::Bytes(size as u64));

        // (a) Full read — concatenate all chunks
        group.bench_with_input(
            BenchmarkId::new("full_read", size / (1024 * 1024)),
            &caddr,
            |b, caddr| {
                b.iter(|| {
                    reader.read_full(caddr).unwrap()
                });
            },
        );

        // (b) Stream read — bounded memory
        group.bench_with_input(
            BenchmarkId::new("stream_read", size / (1024 * 1024)),
            &caddr,
            |b, caddr| {
                b.to_async(&rt).iter(|| async {
                    let rx = reader.read_stream(caddr, 8).unwrap();
                    collect_stream(rx).await.unwrap()
                });
            },
        );

        // (c) Range read — 64 KB from middle
        group.bench_with_input(
            BenchmarkId::new("range_read_64kb", size / (1024 * 1024)),
            &caddr,
            |b, caddr| {
                let offset = (size / 2) as u64;
                b.iter(|| {
                    reader.read_range(caddr, offset, 65536).unwrap()
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Range read is 100–1000× faster than full read for large
values. Stream read throughput similar to full read but with bounded
memory. Full read latency scales linearly with value size.

#### Benchmark 3: Range Read Scaling — Fixed Range, Growing Value

Measures range read latency as the cached value grows, with a fixed
64 KB range. Demonstrates O(1) range read independent of value size.

```rust
fn bench_range_read_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("range_read_scaling");
    let value_sizes = [
        1024 * 1024,          // 1 MB (16 chunks)
        16 * 1024 * 1024,     // 16 MB (256 chunks)
        64 * 1024 * 1024,     // 64 MB (1024 chunks)
        256 * 1024 * 1024,    // 256 MB (4096 chunks)
    ];

    // Fixed range: 64 KB from the middle
    group.throughput(Throughput::Bytes(65536));

    for &size in &value_sizes {
        let tmp = TempDir::new().unwrap();
        let blob_store = BlobStore::open(tmp.path()).unwrap();
        let reader = ChunkCacheReader::new(blob_store.clone());
        let caddr = pre_cache_value(&blob_store, size);
        let offset = (size / 2) as u64;

        group.bench_with_input(
            BenchmarkId::new("range_64kb", size / (1024 * 1024)),
            &(),
            |b, _| {
                b.iter(|| {
                    reader.read_range(&caddr, offset, 65536).unwrap()
                });
            },
        );
    }
    group.finish();
}
```

**Expected:** Range read latency is constant (~200 μs) regardless of
value size. Binary search over manifest is O(log K) where K = chunks,
but K ≤ 4096 so log₂(4096) = 12 comparisons — negligible. I/O is
1–2 chunk reads regardless of total value size.

#### Benchmark 4: Manifest Operations — Serialization & Lookup

Measures manifest overhead at varying chunk counts.

```rust
fn bench_manifest_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("manifest_operations");
    let chunk_counts = [16, 256, 1024, 4096, 65536];

    for &count in &chunk_counts {
        let manifest = build_test_manifest(count); // helper: K entries

        // (a) Serialization
        group.bench_with_input(
            BenchmarkId::new("serialize", count),
            &manifest,
            |b, manifest| {
                b.iter(|| manifest.to_bytes().unwrap());
            },
        );

        // (b) Deserialization
        let bytes = manifest.to_bytes().unwrap();
        group.bench_with_input(
            BenchmarkId::new("deserialize", count),
            &bytes,
            |b, bytes| {
                b.iter(|| ChunkManifest::from_bytes(bytes).unwrap());
            },
        );

        // (c) chunks_for_range (binary search)
        group.bench_with_input(
            BenchmarkId::new("chunks_for_range", count),
            &manifest,
            |b, manifest| {
                let mid = manifest.total_size / 2;
                b.iter(|| manifest.chunks_for_range(mid, 65536));
            },
        );

        // (d) validate
        group.bench_with_input(
            BenchmarkId::new("validate", count),
            &manifest,
            |b, manifest| {
                b.iter(|| manifest.validate().unwrap());
            },
        );
    }
    group.finish();
}
```

**Expected:**
- Serialize/deserialize: ~1 μs for 16 entries, ~100 μs for 65536
- `chunks_for_range`: < 1 μs (binary search, O(log K))
- `validate`: O(K) — ~1 μs for 16, ~50 μs for 65536

#### Benchmark 5: Concurrent Cache Access — Reads & Writes

Measures throughput under concurrent read/write load.

```rust
fn bench_concurrent_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_cache_access");
    let concurrencies = [1, 4, 8, 16];

    for &n in &concurrencies {
        let tmp = TempDir::new().unwrap();
        let blob_store = BlobStore::open(tmp.path()).unwrap();

        // Pre-cache 10 values of 1 MB each
        let caddrs: Vec<_> = (0..10)
            .map(|i| pre_cache_value(&blob_store, 1024 * 1024))
            .collect();

        let reader = ChunkCacheReader::new(blob_store.clone());

        // (a) Concurrent stream reads
        group.bench_with_input(
            BenchmarkId::new("concurrent_stream_reads", n),
            &n,
            |b, &n| {
                b.to_async(&rt).iter(|| async {
                    let futs: Vec<_> = (0..n).map(|i| {
                        let caddr = caddrs[i % 10];
                        let reader = &reader;
                        async move {
                            let rx = reader.read_stream(&caddr, 4).unwrap();
                            collect_stream(rx).await.unwrap()
                        }
                    }).collect();
                    futures::future::join_all(futs).await
                });
            },
        );

        // (b) Concurrent writes (different CAddrs)
        group.bench_with_input(
            BenchmarkId::new("concurrent_writes", n),
            &n,
            |b, &n| {
                let data = Bytes::from(vec![b'a'; 256 * 1024]); // 256 KB each
                b.to_async(&rt).iter(|| async {
                    let futs: Vec<_> = (0..n).map(|i| {
                        let data = data.clone();
                        let bs = blob_store.clone();
                        async move {
                            let rx = value_to_stream(data, 65536, 8);
                            let writer = ChunkCacheWriter::new(
                                unique_caddr(i),
                                65536,
                                bs,
                            );
                            let rx = writer.wrap(rx, 8);
                            collect_stream(rx).await.unwrap()
                        }
                    }).collect();
                    futures::future::join_all(futs).await
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_write_overhead,
    bench_read_modes,
    bench_range_read_scaling,
    bench_manifest_operations,
    bench_concurrent_access,
);
criterion_main!(benches);
```

**Expected:** Stream reads scale linearly with concurrency (I/O bound).
Concurrent writes scale well up to I/O saturation (~8–16 concurrent
on SSD). No lock contention — each CAddr uses independent blob keys.

#### Benchmark Summary

| # | Group | Parameters | Measures | Expected Result |
|---|-------|-----------|----------|-----------------|
| 1 | Write overhead | 4 sizes × 2 modes | Pipeline throughput with/without cache tee | Cached 30–60% slower (I/O bound); > 50 MB/s on SSD |
| 2 | Read modes | 3 sizes × 3 modes | Full vs stream vs range read latency | Range 100–1000× faster than full for large values |
| 3 | Range scaling | 4 value sizes, fixed 64 KB range | Range read latency vs value size | Constant ~200 μs regardless of value size |
| 4 | Manifest ops | 5 chunk counts × 4 ops | Serialize, deserialize, range lookup, validate | Serialize < 100 μs; range lookup < 1 μs; all scale linearly |
| 5 | Concurrent access | 4 concurrencies × 2 modes | Read/write throughput under load | Linear scaling to I/O saturation; no lock contention |

---

## 8. Files Changed

| File | Change | Lines |
|------|--------|-------|
| `deriva-core/src/chunk_addr.rs` | **NEW** — `ChunkAddr` struct, `blob_key()`, `manifest_key()` helper | ~30 |
| `deriva-core/src/chunk_manifest.rs` | **NEW** — `ChunkEntry`, `ChunkManifest`, `ChunkManifestBuilder`, serialization, `chunks_for_range()`, `validate()` | ~130 |
| `deriva-core/src/lib.rs` | Add `pub mod chunk_addr; pub mod chunk_manifest;` | ~2 |
| `deriva-storage/src/blob_store.rs` | Add `put_by_key()`, `get_by_key()`, `contains_key()`, `remove_by_key()`, `key_path()` | ~45 |
| `deriva-compute/src/chunk_cache.rs` | **NEW** — `ChunkCacheWriter` (wrap, cleanup), `ChunkCacheReader` (read_manifest, read_full, read_stream, read_range, has_manifest, remove) | ~250 |
| `deriva-compute/src/streaming_executor.rs` | Check chunk cache before monolithic cache; wrap output with `ChunkCacheWriter` when `cache_intermediates` enabled | ~25 |
| `deriva-compute/src/cache.rs` | Add `get_stream()` and `has_chunk_cache()` default methods to `AsyncMaterializationCache` trait | ~15 |
| `deriva-compute/src/lib.rs` | Add `pub mod chunk_cache;` | ~1 |
| `deriva-compute/src/metrics.rs` | Add `CHUNK_CACHE_WRITES_TOTAL`, `CHUNK_CACHE_READS_TOTAL`, `CHUNK_CACHE_RANGE_READS_TOTAL`, `CHUNK_CACHE_BYTES_WRITTEN`, `CHUNK_CACHE_MANIFEST_SIZE` | ~30 |
| `deriva-server/src/gc.rs` | Use `ChunkCacheReader::remove()` for chunk-level eviction alongside legacy blob removal | ~15 |
| `deriva-compute/tests/chunk_cache.rs` | **NEW** — 40 tests: types, writer, reader, range, integration, GC, edge cases, benchmarks | ~1,000 |
| `deriva-compute/benches/chunk_cache.rs` | **NEW** — 5 benchmark groups: write overhead, read modes, range scaling, manifest ops, concurrent access | ~250 |

**Totals:** 12 files, ~1,793 lines (6 new files, 6 modified)

---

## 9. Dependency Changes

| Dependency | Crate | Status | Used for |
|-----------|-------|--------|----------|
| `bincode` | `deriva-core` | **NEW** | `ChunkManifest` serialization/deserialization |
| `serde` + `serde_derive` | `deriva-core` | Existing | `#[derive(Serialize, Deserialize)]` on `ChunkAddr`, `ChunkManifest`, `ChunkEntry` |
| `tokio::sync::mpsc` | `deriva-compute` | Existing | Streaming channels in `ChunkCacheReader::read_stream()` |
| `bytes::Bytes` | `deriva-core` | Existing | Chunk data type |
| `tracing` | `deriva-compute` | Existing | Warning/debug logs for cache write failures |
| `prometheus` | `deriva-compute` | Existing | Metrics counters and histograms |
| `tempfile` | `deriva-compute` (dev) | Existing | Temp directories for blob store in tests/benchmarks |
| `criterion` | `deriva-compute` (dev) | Existing | Benchmark framework |

Only one new dependency: `bincode` for compact binary manifest serialization.
Alternative: `serde_json` (already available) — larger manifests but no new dep.
Decision deferred to implementation time.

---

## 10. Design Rationale

### 10.1 Why Deferred to Phase 3?

Cache-after-collect works for the majority of workloads:

| Value Size | % of Workload (estimated) | Cache-After-Collect OK? |
|-----------|--------------------------|------------------------|
| < 1 MB | ~70% | ✅ Trivial memory cost |
| 1–100 MB | ~25% | ⚠️ Noticeable but manageable |
| 100 MB–1 GB | ~4% | ❌ Significant memory pressure |
| > 1 GB | ~1% | ❌ OOM risk on constrained containers |

Chunk-level caching adds substantial complexity:
- New types (`ChunkAddr`, `ChunkManifest`, `ChunkManifestBuilder`)
- New storage operations (`put_by_key`, `get_by_key` on `BlobStore`)
- New cache writer/reader with error handling and cleanup
- GC changes for manifest-aware eviction
- Migration path for coexistence with existing cache

This complexity is justified only for the ~5% of workloads with
outputs > 100 MB. Phase 2 delivers streaming (§2.7), budget
enforcement (§2.12), and fusion (§2.11) which benefit all workloads.
Chunk caching is deferred to Phase 3 when the foundation is stable.

### 10.2 Why Not Compress Chunks Before Caching?

| Approach | Pros | Cons |
|----------|------|------|
| Store raw chunks | Simple; no CPU overhead on write/read; range reads return exact bytes | More disk usage |
| Compress each chunk | 2–10× disk savings for compressible data | CPU overhead on every write and read; range reads require decompress; double-compression if pipeline already compresses |

Raw storage is chosen because:
1. Compression is a pipeline stage — users who want compressed cache
   can add `Compress` to their pipeline. The cache stores whatever
   the pipeline outputs.
2. Range reads must return exact bytes at exact offsets. Compressed
   chunks require full decompression before slicing, negating the
   range read benefit.
3. The cache write path must be fast (it's in the hot path). Adding
   compression adds ~200 μs/chunk latency.

### 10.3 Why Atomic Manifest + Chunks Instead of Append-Only Log?

| Approach | Pros | Cons |
|----------|------|------|
| Manifest + individual chunk blobs | Random access via manifest; range reads efficient; GC can enumerate chunks | More I/O ops; manifest is single point of failure |
| Append-only log (all chunks in one file) | Single file per CAddr; sequential write; simple | No random access without index; range reads require seeking; partial corruption loses everything |
| Packed chunks with inline index | Single file; random access via embedded index | Complex format; partial writes corrupt the file; harder to GC individual chunks |

Manifest + individual blobs is chosen because:
1. Range reads require random access to specific chunks — a manifest
   provides O(log K) lookup.
2. Individual blobs leverage the existing `BlobStore` infrastructure
   with no new file format.
3. GC can enumerate and remove chunks independently.
4. Manifest failure (corruption, missing) is a clean cache miss —
   chunks become orphans for cleanup, not corrupted data.

### 10.4 Why Threshold-Based Activation Instead of Always Chunk Cache?

Small values (< 1 MB) are more efficiently cached as monolithic blobs:

| Value Size | Monolithic Write | Chunk Write (16 chunks) | Overhead |
|-----------|-----------------|------------------------|---------|
| 64 KB | 1 I/O op | 2 I/O ops (1 chunk + manifest) | 2× |
| 256 KB | 1 I/O op | 5 I/O ops (4 chunks + manifest) | 5× |
| 1 MB | 1 I/O op | 17 I/O ops (16 chunks + manifest) | 17× |

For small values, the per-chunk I/O overhead exceeds the memory
savings (which are negligible — 1 MB buffer is fine). The threshold
(default 1 MB) ensures chunk caching activates only when the memory
benefit justifies the I/O cost.

---

## 11. Observability Integration

Five new metrics:

```rust
lazy_static! {
    /// Total chunk cache write operations (one per CAddr successfully cached).
    static ref CHUNK_CACHE_WRITES_TOTAL: IntCounter = register_int_counter!(
        "deriva_chunk_cache_writes_total",
        "Number of values cached via chunk-level cache"
    ).unwrap();

    /// Total chunk cache read operations (full or stream reads).
    static ref CHUNK_CACHE_READS_TOTAL: IntCounter = register_int_counter!(
        "deriva_chunk_cache_reads_total",
        "Number of chunk cache read hits"
    ).unwrap();

    /// Total range read operations.
    static ref CHUNK_CACHE_RANGE_READS_TOTAL: IntCounter = register_int_counter!(
        "deriva_chunk_cache_range_reads_total",
        "Number of chunk cache range reads"
    ).unwrap();

    /// Total bytes written to chunk cache.
    static ref CHUNK_CACHE_BYTES_WRITTEN: IntCounter = register_int_counter!(
        "deriva_chunk_cache_bytes_written_total",
        "Total bytes written to chunk cache"
    ).unwrap();

    /// Manifest size distribution.
    static ref CHUNK_CACHE_MANIFEST_SIZE: Histogram = register_histogram!(
        "deriva_chunk_cache_manifest_size_bytes",
        "Size of chunk cache manifests in bytes",
        vec![100.0, 500.0, 1000.0, 5000.0, 10000.0, 50000.0, 100000.0, 1000000.0]
    ).unwrap();
}
```

Dashboard queries:

```promql
# Chunk cache hit rate (should increase over time as values are cached)
rate(deriva_chunk_cache_reads_total[5m])

# Cache write throughput (bytes/sec being cached)
rate(deriva_chunk_cache_bytes_written_total[5m])

# Range read ratio (higher = more efficient partial access)
rate(deriva_chunk_cache_range_reads_total[5m])
  / rate(deriva_chunk_cache_reads_total[5m])

# Average manifest size (indicates typical value size being cached)
histogram_quantile(0.5, rate(deriva_chunk_cache_manifest_size_bytes_bucket[5m]))

# Chunk cache activity (any writes happening?)
deriva_chunk_cache_writes_total > 0
```

---

## 12. Checklist

- [ ] Create `deriva-core/src/chunk_addr.rs` with `ChunkAddr` and `manifest_key()`
- [ ] Create `deriva-core/src/chunk_manifest.rs` with `ChunkEntry`, `ChunkManifest`, `ChunkManifestBuilder`
- [ ] Implement `ChunkManifest::to_bytes()` / `from_bytes()` via bincode
- [ ] Implement `ChunkManifest::chunks_for_range()` with binary search
- [ ] Implement `ChunkManifest::validate()` for invariant checking
- [ ] Add `pub mod chunk_addr; pub mod chunk_manifest;` to `deriva-core/src/lib.rs`
- [ ] Add `bincode` dependency to `deriva-core/Cargo.toml`
- [ ] Add `put_by_key()`, `get_by_key()`, `contains_key()`, `remove_by_key()` to `BlobStore`
- [ ] Add `key_path()` helper with `chunks/` directory sharding
- [ ] Create `deriva-compute/src/chunk_cache.rs`
- [ ] Implement `ChunkCacheWriter::new()` and `wrap()`
- [ ] Implement `ChunkCacheWriter` cleanup on error/drop
- [ ] Implement `ChunkCacheReader::read_manifest()`
- [ ] Implement `ChunkCacheReader::read_full()`
- [ ] Implement `ChunkCacheReader::read_stream()`
- [ ] Implement `ChunkCacheReader::read_range()`
- [ ] Implement `ChunkCacheReader::has_manifest()` and `remove()`
- [ ] Add `pub mod chunk_cache;` to `deriva-compute/src/lib.rs`
- [ ] Add `get_stream()` and `has_chunk_cache()` defaults to `AsyncMaterializationCache`
- [ ] Integrate chunk cache check into `StreamingExecutor::materialize_streaming()`
- [ ] Wrap pipeline output with `ChunkCacheWriter` when `cache_intermediates` enabled
- [ ] Update `run_gc()` to use `ChunkCacheReader::remove()` for chunk-level eviction
- [ ] Add `CHUNK_CACHE_WRITES_TOTAL` counter to `metrics.rs`
- [ ] Add `CHUNK_CACHE_READS_TOTAL` counter to `metrics.rs`
- [ ] Add `CHUNK_CACHE_RANGE_READS_TOTAL` counter to `metrics.rs`
- [ ] Add `CHUNK_CACHE_BYTES_WRITTEN` counter to `metrics.rs`
- [ ] Add `CHUNK_CACHE_MANIFEST_SIZE` histogram to `metrics.rs`
- [ ] Unit tests: T1–T6 (ChunkAddr & ChunkManifest core)
- [ ] Unit tests: T7–T13 (ChunkCacheWriter)
- [ ] Unit tests: T14–T19 (ChunkCacheReader full & stream)
- [ ] Unit tests: T20–T24 (ChunkCacheReader range read)
- [ ] Integration tests: T25–T29 (pipeline + cache roundtrip)
- [ ] GC tests: T30–T34 (eviction, mixed formats, pins, orphans, concurrent)
- [ ] Edge case tests: T35–T38 (disk full, corrupt manifest, budget, single-chunk)
- [ ] Benchmark tests: T39–T40 (write overhead, range read speedup)
- [ ] Benchmark: write overhead (4 sizes × 2 modes)
- [ ] Benchmark: read modes (3 sizes × 3 modes)
- [ ] Benchmark: range read scaling (4 value sizes)
- [ ] Benchmark: manifest operations (5 chunk counts × 4 ops)
- [ ] Benchmark: concurrent access (4 concurrencies × 2 modes)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing §2.7–§2.12 tests still pass
- [ ] Commit and push
