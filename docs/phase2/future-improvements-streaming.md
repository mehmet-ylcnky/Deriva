# Future Improvements: Streaming Materialization

Identified during the Paper 2b evaluation (benchmarks on `streaming_pipeline.rs` and `batch_vs_streaming` sweep). These are concrete, implementation-ready improvements informed by empirical data.

---

## 1. Size-Aware Execution Mode Selection

**Problem:** The `StreamingExecutor` currently selects streaming over batch whenever a `StreamingComputeFunction` is registered — regardless of input size. Benchmarks show batch is up to 9.3× faster below ~3 MB due to streaming's fixed overhead (channel creation, Tokio task spawning, chunk framing).

**Crossover data (5-stage pipeline, Criterion.rs median):**

| Input Size | Batch | Streaming | Winner |
|------------|-------|-----------|--------|
| 1 MB | 0.17 ms | 1.59 ms | Batch 9.3× |
| 2 MB | 0.41 ms | 2.36 ms | Batch 5.7× |
| 4 MB | 4.18 ms | 3.03 ms | Stream 1.4× |
| 8 MB | 9.84 ms | 4.72 ms | Stream 2.1× |

**Implementation:**

```rust
// In PipelineConfig:
pub streaming_threshold: usize, // default: 3 * 1024 * 1024 (3 MB)

// In StreamingExecutor::materialize_streaming(), at the selection point:
let input_size: usize = input_indices.iter()
    .filter_map(|&i| pipeline.node_data_size(i))
    .sum();

let use_streaming = input_size > self.config.streaming_threshold
    || input_size == 0; // unknown size → default to streaming

if use_streaming {
    if let Some(streaming_fn) = registry.get_streaming(func_id) {
        // streaming path
    }
} else if let Some(batch_fn) = registry.get(func_id) {
    // batch path — faster for small inputs
} else if let Some(streaming_fn) = registry.get_streaming(func_id) {
    // no batch impl, use streaming anyway
}
```

**Complexity:** ~20 lines. Requires adding `node_data_size(idx) -> Option<usize>` to `StreamPipeline` — trivial for `Source`/`Cached` nodes (return `bytes.len()`), returns `None` for computed nodes.

**Limitation:** Input size is only known for leaf/cached nodes. For intermediate computed nodes, the size is unknown until execution. Could be extended with size hints from function metadata.

---

## 2. Adaptive Chunk Sizing

**Problem:** The default 64 KB chunk size is near-optimal for the general case, but benchmarks show significant sensitivity:

| Chunk Size | Latency (1 MB) | Channel Ops |
|------------|----------------|-------------|
| 1 KB | 8.56 ms | 1,000 |
| 16 KB | 1.76 ms | 63 |
| 64 KB | 1.50 ms | 16 |
| 256 KB | 1.62 ms | 4 |
| 1 MB | 1.33 ms | 1 |

**Improvement:** Dynamically adjust chunk size based on observed throughput. If a stage processes chunks faster than its downstream can consume (backpressure detected), increase chunk size to reduce channel overhead. If a consumer is fast, decrease chunk size for better TTFB.

**Implementation:** Add a `ChunkResizer` stage that monitors send/recv timing and adjusts the `Bytes::slice()` boundaries. Could be inserted automatically by the pipeline builder between stages with mismatched throughput characteristics.

---

## 3. Pipeline Fusion

**Problem:** Adjacent map stages (e.g., `StreamingUppercase` → `StreamingLowercase`) each require a channel hop (~100 ns per chunk). For simple byte transforms, the channel overhead can exceed the compute cost.

**Improvement:** Detect fusible adjacent stages during pipeline construction and merge them into a single stage that applies both transforms per chunk. The optimizer must prove that fusion preserves semantics (both stages are pure map functions with no state).

**Implementation:** Add a `FusedMapStage` that holds a `Vec<Arc<dyn StreamingComputeFunction>>` and applies them sequentially per chunk within a single Tokio task. The pipeline builder identifies fusible chains during topological construction.

---

## 4. Memory Budget Enforcement

**Problem:** `PipelineConfig::memory_budget` exists but is not enforced. A pipeline with many stages could exceed system memory even with bounded channels.

**Improvement:** A global memory controller that tracks total buffer allocation across all pipeline channels. When total memory approaches the budget, the controller pauses producers by holding channel permits — providing system-wide backpressure beyond per-channel bounds.

**Implementation:** Use a `tokio::sync::Semaphore` with permits equal to `memory_budget / chunk_size`. Each `send()` acquires a permit, each `recv()` releases one. This bounds total in-flight data across the entire pipeline.

---

## 5. Streaming-Aware Caching

**Problem:** Cache-after-collect requires buffering the full result before caching. For very large outputs (>1 GB), this defeats the bounded-memory benefit of streaming.

**Improvement:** A chunk-level cache that stores individual chunks keyed by `(CAddr, offset)`. Enables partial cache hits (serve bytes 0–64KB from cache while computing the rest) and range reads without full materialization.

**Complexity:** High. Requires changes to the cache interface, CAddr model (sub-addressing), and GC (chunk-level reference counting). Deferred to Phase 3.

---

## 6. Expanded Streaming Function Library

**Current state:** 20 built-in streaming functions across 4 categories (9 transforms, 3 accumulators, 3 combiners, 5 utilities). The `StreamingComputeFunction` trait and `spawn_map`/`spawn_accumulate` helpers make adding new functions straightforward.

**Goal:** Grow the library to cover common data processing, cryptographic, encoding, validation, and analytics patterns. Each function below includes its category, execution pattern, memory profile, and implementation complexity estimate.

### Existing 20 Functions (for reference)

| # | Function | Category | Pattern |
|---|----------|----------|---------|
| 1 | `StreamingIdentity` | Transform | map |
| 2 | `StreamingUppercase` | Transform | map |
| 3 | `StreamingLowercase` | Transform | map |
| 4 | `StreamingReverse` | Transform | map |
| 5 | `StreamingBase64Encode` | Transform | map |
| 6 | `StreamingBase64Decode` | Transform | map |
| 7 | `StreamingXor` | Transform | map |
| 8 | `StreamingCompress` | Transform | map |
| 9 | `StreamingDecompress` | Transform | map |
| 10 | `StreamingSha256` | Accumulator | fold |
| 11 | `StreamingByteCount` | Accumulator | fold |
| 12 | `StreamingChecksum` | Accumulator | fold |
| 13 | `StreamingConcat` | Combiner | multi-recv sequential |
| 14 | `StreamingInterleave` | Combiner | multi-recv round-robin |
| 15 | `StreamingZipConcat` | Combiner | multi-recv pairwise |
| 16 | `StreamingChunkResizer` | Utility | buffered |
| 17 | `StreamingTake` | Utility | offset-tracking |
| 18 | `StreamingSkip` | Utility | offset-tracking |
| 19 | `StreamingRepeat` | Utility | buffer-all |
| 20 | `StreamingTeeCount` | Utility | pass-through + state |

### New Functions — Cryptography & Security

| # | Function | Category | Pattern | Memory | Complexity | Description |
|---|----------|----------|---------|--------|------------|-------------|
| 21 | `StreamingEncrypt` | Transform | map | O(chunk) | Medium | AES-256-CTR per-chunk encryption. Counter state carried across chunks. Params: `key`, `nonce`. |
| 22 | `StreamingDecrypt` | Transform | map | O(chunk) | Medium | AES-256-CTR per-chunk decryption. Inverse of `StreamingEncrypt`. |
| 23 | `StreamingAeadEncrypt` | Transform | buffered | O(chunk+16) | Medium | AES-256-GCM authenticated encryption. Appends 16-byte auth tag per chunk. |
| 24 | `StreamingAeadDecrypt` | Transform | buffered | O(chunk+16) | Medium | AES-256-GCM authenticated decryption. Verifies tag per chunk, emits `Error` on tamper. |
| 25 | `StreamingHmacSha256` | Accumulator | fold | O(64B) | Low | HMAC-SHA256 keyed hash. Params: `key`. Emits 32-byte MAC on `End`. |
| 26 | `StreamingMd5` | Accumulator | fold | O(16B) | Low | MD5 digest (for legacy compatibility / S3 ETag matching, not security). |
| 27 | `StreamingSha512` | Accumulator | fold | O(64B) | Low | SHA-512 digest. Emits 64-byte hash on `End`. |
| 28 | `StreamingBlake3` | Accumulator | fold | O(1.8KB) | Low | BLAKE3 hash — faster than SHA-256 on modern CPUs. Emits 32-byte hash. |
| 29 | `StreamingRedact` | Transform | map | O(chunk) | Medium | Regex-based PII redaction. Replaces matches with `[REDACTED]`. Params: `patterns[]`. |

### New Functions — Encoding & Format Conversion

| # | Function | Category | Pattern | Memory | Complexity | Description |
|---|----------|----------|---------|--------|------------|-------------|
| 30 | `StreamingHexEncode` | Transform | map | O(2×chunk) | Low | Encode each byte as 2 hex chars. Output is 2× input size. |
| 31 | `StreamingHexDecode` | Transform | map | O(chunk/2) | Low | Decode hex string to bytes. Emits `Error` on invalid hex. |
| 32 | `StreamingUtf8Validate` | Transform | map | O(chunk) | Low | Pass-through that validates UTF-8. Emits `Error` on invalid sequences. Boundary-aware: buffers up to 3 trailing bytes for multi-byte chars split across chunks. |
| 33 | `StreamingLineEnding` | Transform | map | O(chunk) | Low | Convert line endings: `\r\n` → `\n` or vice versa. Params: `target` (`lf` or `crlf`). |
| 34 | `StreamingJsonPrettyPrint` | Utility | buffered | O(input) | Medium | Buffer full JSON, parse, re-emit pretty-printed. Non-streaming output. |
| 35 | `StreamingJsonMinify` | Utility | buffered | O(input) | Medium | Buffer full JSON, strip whitespace, re-emit compact. |
| 36 | `StreamingJsonLines` | Transform | map | O(chunk) | Low | Ensure each chunk ends with `\n`. For NDJSON/JSONL formatting. |
| 37 | `StreamingCsvToJson` | Utility | buffered | O(header+chunk) | High | Parse CSV header, then convert each row to JSON object. Stateful: remembers header across chunks. |
| 38 | `StreamingBase32Encode` | Transform | map | O(chunk×8/5) | Low | Base32 encoding per chunk. |
| 39 | `StreamingBase32Decode` | Transform | map | O(chunk×5/8) | Low | Base32 decoding per chunk. |

### New Functions — Data Processing & Analytics

| # | Function | Category | Pattern | Memory | Complexity | Description |
|---|----------|----------|---------|--------|------------|-------------|
| 40 | `StreamingFilter` | Transform | map | O(chunk) | Low | Drop chunks where a predicate returns false. Params: `predicate` (e.g., `non_empty`, `contains:pattern`). |
| 41 | `StreamingLineCount` | Accumulator | fold | O(8B) | Low | Count `\n` bytes across all chunks. Emit count as u64 on `End`. |
| 42 | `StreamingWordCount` | Accumulator | fold | O(8B+1B) | Low | Count whitespace-delimited words. Tracks in-word state (1 byte) + counter (8 bytes). |
| 43 | `StreamingMinMax` | Accumulator | fold | O(2B) | Low | Track min and max byte values across the stream. Emit `[min, max]` on `End`. |
| 44 | `StreamingHistogram` | Accumulator | fold | O(256×8B) | Low | 256-bucket byte frequency histogram. Emit 2KB histogram on `End`. |
| 45 | `StreamingSample` | Utility | offset-tracking | O(chunk) | Medium | Reservoir sampling: emit every Nth chunk. Params: `rate` (e.g., 10 = keep 1 in 10). |
| 46 | `StreamingHead` | Utility | offset-tracking | O(chunk) | Low | Emit first N chunks (not bytes — unlike `StreamingTake`). Params: `chunks`. |
| 47 | `StreamingTail` | Utility | ring-buffer | O(N×chunk) | Medium | Emit last N chunks. Requires ring buffer of size N. Params: `chunks`. |
| 48 | `StreamingDeduplicate` | Utility | set | O(seen_hashes) | Medium | Drop duplicate chunks (by content hash). Memory grows with unique chunk count. |
| 49 | `StreamingSort` | Utility | buffer-all | O(input) | Medium | Buffer all chunks, sort by byte order, re-emit. Non-streaming output. |
| 50 | `StreamingUnique` | Utility | buffer-all | O(input) | Medium | Buffer all, deduplicate, re-emit sorted unique chunks. |

### New Functions — Compression & Transformation

| # | Function | Category | Pattern | Memory | Complexity | Description |
|---|----------|----------|---------|--------|------------|-------------|
| 51 | `StreamingZstdCompress` | Transform | map | O(chunk) | Medium | Zstandard compression per chunk. Better ratios than zlib at similar speed. |
| 52 | `StreamingZstdDecompress` | Transform | map | O(chunk) | Medium | Zstandard decompression per chunk. |
| 53 | `StreamingLz4Compress` | Transform | map | O(chunk) | Low | LZ4 compression — fastest compression, lower ratio. |
| 54 | `StreamingLz4Decompress` | Transform | map | O(chunk) | Low | LZ4 decompression. |
| 55 | `StreamingSnappyCompress` | Transform | map | O(chunk) | Low | Snappy compression — Google's fast compressor. |
| 56 | `StreamingSnappyDecompress` | Transform | map | O(chunk) | Low | Snappy decompression. |
| 57 | `StreamingBrotliCompress` | Transform | map | O(chunk) | Medium | Brotli compression — best ratio for text/web content. |
| 58 | `StreamingBrotliDecompress` | Transform | map | O(chunk) | Medium | Brotli decompression. |
| 59 | `StreamingPad` | Transform | map | O(chunk+pad) | Low | Pad each chunk to a fixed size. Params: `block_size`, `padding_byte`. |
| 60 | `StreamingTrim` | Transform | map | O(chunk) | Low | Strip leading/trailing whitespace bytes from each chunk. |

### New Functions — Flow Control & Pipeline Utilities

| # | Function | Category | Pattern | Memory | Complexity | Description |
|---|----------|----------|---------|--------|------------|-------------|
| 61 | `StreamingRateLimit` | Utility | pass-through | O(chunk) | Medium | Throttle output to target bytes/sec. Uses `tokio::time::sleep` between sends. Params: `bytes_per_sec`. |
| 62 | `StreamingDelay` | Utility | pass-through | O(chunk) | Low | Add fixed delay before each chunk. Params: `delay_ms`. For testing backpressure. |
| 63 | `StreamingTimeout` | Utility | pass-through | O(chunk) | Medium | Emit `Error` if no chunk arrives within deadline. Params: `timeout_ms`. |
| 64 | `StreamingRetry` | Utility | wrapper | O(chunk) | High | On upstream `Error`, restart the upstream function up to N times. Params: `max_retries`. |
| 65 | `StreamingTee` | Utility | fan-out | O(chunk×N) | Medium | Duplicate input to N output receivers. Each output gets a clone of every chunk. |
| 66 | `StreamingMerge` | Combiner | multi-recv | O(N×chunk) | Medium | Select-based merge: emit whichever input has a chunk ready first (non-deterministic order). |
| 67 | `StreamingBroadcast` | Combiner | fan-out | O(chunk×N) | Medium | Like `Tee` but with backpressure: slowest consumer gates the producer. |
| 68 | `StreamingPartition` | Utility | fan-out | O(chunk×2) | Medium | Route chunks to one of 2 outputs based on a predicate. Params: `predicate`. |
| 69 | `StreamingBatch` | Utility | buffered | O(N×chunk) | Low | Collect N chunks into one large chunk. Inverse of `ChunkResizer`. Params: `batch_size`. |
| 70 | `StreamingDebounce` | Utility | pass-through | O(chunk) | Medium | Suppress rapid consecutive chunks; emit only after a quiet period. Params: `window_ms`. |

### New Functions — Validation & Integrity

| # | Function | Category | Pattern | Memory | Complexity | Description |
|---|----------|----------|---------|--------|------------|-------------|
| 71 | `StreamingJsonValidate` | Utility | buffered | O(input) | Medium | Buffer full input, parse as JSON, emit unchanged if valid, `Error` if not. |
| 72 | `StreamingSchemaValidate` | Utility | buffered | O(input) | High | Validate JSON against a JSON Schema. Params: `schema`. |
| 73 | `StreamingMagicBytes` | Transform | map (first chunk) | O(chunk) | Low | Check first chunk starts with expected magic bytes. Params: `expected`. Emit `Error` on mismatch. |
| 74 | `StreamingSizeLimit` | Utility | offset-tracking | O(8B) | Low | Track total bytes; emit `Error` if limit exceeded. Params: `max_bytes`. |
| 75 | `StreamingChecksumVerify` | Accumulator | fold | O(4B) | Low | Compute CRC32 and compare against expected value. Params: `expected_crc32`. Emit `Error` on mismatch. |
| 76 | `StreamingSha256Verify` | Accumulator | fold | O(32B) | Low | Compute SHA-256 and compare against expected hash. Params: `expected_hash`. |
| 77 | `StreamingNonEmpty` | Transform | map | O(1B) | Low | Emit `Error` if stream ends with zero data chunks. Pass-through otherwise. |

### New Functions — Text Processing

| # | Function | Category | Pattern | Memory | Complexity | Description |
|---|----------|----------|---------|--------|------------|-------------|
| 78 | `StreamingReplace` | Transform | map | O(chunk) | Medium | Byte pattern replacement within each chunk. Params: `find`, `replace`. Boundary-aware for patterns split across chunks. |
| 79 | `StreamingPrefix` | Transform | map (first) | O(prefix) | Low | Prepend a fixed byte sequence before the first data chunk. Params: `prefix`. |
| 80 | `StreamingSuffix` | Utility | pass-through | O(suffix) | Low | Append a fixed byte sequence after the last data chunk (before `End`). Params: `suffix`. |
| 81 | `StreamingLinePrefix` | Transform | map | O(chunk) | Medium | Prepend a string to each line (e.g., line numbers, timestamps). Params: `prefix_fn`. |
| 82 | `StreamingGrep` | Transform | map | O(chunk) | Medium | Keep only lines matching a regex. Params: `pattern`. Boundary-aware for lines split across chunks. |
| 83 | `StreamingSed` | Transform | map | O(chunk) | Medium | Regex find-and-replace per line. Params: `pattern`, `replacement`. |
| 84 | `StreamingTruncateLines` | Transform | map | O(chunk) | Low | Truncate each line to max N bytes. Params: `max_line_bytes`. |
| 85 | `StreamingCharsetConvert` | Transform | map | O(chunk) | Medium | Convert between character encodings (e.g., Latin-1 → UTF-8). Params: `from`, `to`. |

### New Functions — CAS-Specific Operations

| # | Function | Category | Pattern | Memory | Complexity | Description |
|---|----------|----------|---------|--------|------------|-------------|
| 86 | `StreamingCAddrEmbed` | Utility | pass-through + state | O(chunk) | Medium | Compute CAddr (content hash) of the stream and embed it as a trailing metadata chunk. Combines pass-through with SHA-256 accumulation. |
| 87 | `StreamingCAddrVerify` | Accumulator | fold | O(32B) | Medium | Compute CAddr and verify against an expected address. Params: `expected_caddr`. Emit `Error` on mismatch. Ensures content integrity in CAS pipelines. |
| 88 | `StreamingDiff` | Combiner | multi-recv pairwise | O(chunk×2) | High | Read two inputs chunk-by-chunk, emit a binary diff (byte-level XOR or edit script). For CAS deduplication analysis. |
| 89 | `StreamingPatch` | Combiner | multi-recv | O(chunk×2) | High | Apply a binary patch stream to a base stream. Inverse of `StreamingDiff`. |
| 90 | `StreamingMerkleTree` | Accumulator | fold | O(depth×32B) | High | Build a Merkle tree over chunks. Emit root hash on `End`. Enables chunk-level integrity verification for CAS. |
| 91 | `StreamingContentType` | Transform | map (first chunk) | O(chunk) | Medium | Detect content type from magic bytes (first chunk) and prepend a metadata chunk with MIME type string. |
| 92 | `StreamingChunkHash` | Transform | map | O(chunk+32B) | Low | Append a 32-byte SHA-256 hash to each chunk. Enables per-chunk integrity verification without full-stream accumulation. |

### New Functions — Numeric & Scientific

| # | Function | Category | Pattern | Memory | Complexity | Description |
|---|----------|----------|---------|--------|------------|-------------|
| 93 | `StreamingSum` | Accumulator | fold | O(8B) | Low | Interpret chunks as newline-delimited decimal numbers, sum them. Emit total on `End`. |
| 94 | `StreamingAverage` | Accumulator | fold | O(16B) | Low | Running sum + count, emit average on `End`. |
| 95 | `StreamingBitwiseAnd` | Transform | map | O(chunk) | Low | Bitwise AND each byte with a mask. Params: `mask`. |
| 96 | `StreamingBitwiseOr` | Transform | map | O(chunk) | Low | Bitwise OR each byte with a mask. Params: `mask`. |
| 97 | `StreamingBitwiseNot` | Transform | map | O(chunk) | Low | Bitwise NOT (complement) of each byte. |
| 98 | `StreamingByteSwap` | Transform | map | O(chunk) | Low | Swap byte order within 2/4/8-byte words. Params: `word_size`. For endianness conversion. |
| 99 | `StreamingEntropy` | Accumulator | fold | O(256×8B) | Low | Compute Shannon entropy of the byte stream. Uses byte histogram (same as `StreamingHistogram`), computes entropy on `End`. |
| 100 | `StreamingRollingHash` | Transform | map | O(chunk+window) | Medium | Compute rolling hash (Rabin fingerprint) over a sliding window. Params: `window_size`. Useful for content-defined chunking in CAS. |

### Summary

| Status | Count | Categories |
|--------|-------|------------|
| **Existing (implemented)** | 20 | 9 transforms, 3 accumulators, 3 combiners, 5 utilities |
| **New proposed** | 80 | See above: crypto (9), encoding (10), analytics (11), compression (10), flow control (10), validation (7), text (8), CAS-specific (7), numeric (8) |
| **Total library target** | **100** | Full coverage of common streaming data operations |

### Implementation Priority

1. **High priority** (most useful, low complexity): `StreamingBlake3`, `StreamingFilter`, `StreamingHexEncode/Decode`, `StreamingSizeLimit`, `StreamingTimeout`, `StreamingTee`, `StreamingMerge`, `StreamingPrefix/Suffix`, `StreamingCAddrVerify`
2. **Medium priority** (useful, moderate complexity): `StreamingEncrypt/Decrypt`, `StreamingZstdCompress/Decompress`, `StreamingGrep`, `StreamingReplace`, `StreamingMerkleTree`, `StreamingRollingHash`, `StreamingPartition`
3. **Low priority** (niche or high complexity): `StreamingJsonPrettyPrint`, `StreamingCsvToJson`, `StreamingSchemaValidate`, `StreamingDiff/Patch`, `StreamingSort`

---

## 7. Expanded Batch Function Library

**Current state:** 4 batch functions — `IdentityFn`, `ConcatFn`, `UppercaseFn`, `RepeatFn`. These cover only the minimum needed for initial CAS pipeline testing. The batch path (`ComputeFunction` trait) takes `Vec<Bytes>` inputs and returns a single `Bytes`, making it simpler than the streaming trait but limited to in-memory operation.

**Goal:** Achieve parity with the streaming library where applicable, and add batch-only functions that benefit from having the full input available at once (e.g., sorting, global search-replace, whole-file compression). Each function implements `ComputeFunction` with `id()`, `execute()`, and `estimated_cost()`.

**Design note:** Not every streaming function needs a batch counterpart. Accumulators (SHA-256, byte count, checksum) are trivially batch — they just operate on the full `Bytes` at once. Combiners map directly to multi-input `execute()`. Flow control functions (rate limit, timeout, debounce) are streaming-only concepts with no batch equivalent.

### Existing 4 Functions (for reference)

| # | Function | FunctionId | Inputs | Description |
|---|----------|-----------|--------|-------------|
| 1 | `IdentityFn` | `identity@1.0.0` | 1 | Pass-through, returns input unchanged |
| 2 | `ConcatFn` | `concat@1.0.0` | N | Concatenate all inputs sequentially |
| 3 | `UppercaseFn` | `uppercase@1.0.0` | 1 | UTF-8 uppercase conversion |
| 4 | `RepeatFn` | `repeat@1.0.0` | 1 | Repeat input N times. Params: `count` |

### Category 1: Transforms (single input → single output)

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 5 | `LowercaseFn` | `lowercase@1.0.0` | Low | UTF-8 lowercase conversion |
| 6 | `ReverseFn` | `reverse@1.0.0` | Low | Reverse all bytes |
| 7 | `Base64EncodeFn` | `base64_encode@1.0.0` | Low | Base64-encode entire input |
| 8 | `Base64DecodeFn` | `base64_decode@1.0.0` | Low | Base64-decode entire input |
| 9 | `HexEncodeFn` | `hex_encode@1.0.0` | Low | Hex-encode entire input |
| 10 | `HexDecodeFn` | `hex_decode@1.0.0` | Low | Hex-decode entire input |
| 11 | `Base32EncodeFn` | `base32_encode@1.0.0` | Low | Base32-encode entire input |
| 12 | `Base32DecodeFn` | `base32_decode@1.0.0` | Low | Base32-decode entire input |
| 13 | `XorFn` | `xor@1.0.0` | Low | XOR each byte with key. Params: `key` |
| 14 | `BitwiseAndFn` | `bitwise_and@1.0.0` | Low | AND each byte with mask. Params: `mask` |
| 15 | `BitwiseOrFn` | `bitwise_or@1.0.0` | Low | OR each byte with mask. Params: `mask` |
| 16 | `BitwiseNotFn` | `bitwise_not@1.0.0` | Low | Complement each byte |
| 17 | `ByteSwapFn` | `byte_swap@1.0.0` | Low | Swap byte order within words. Params: `word_size` (2/4/8) |
| 18 | `TrimFn` | `trim@1.0.0` | Low | Strip leading/trailing whitespace |
| 19 | `PadFn` | `pad@1.0.0` | Low | Pad to fixed size. Params: `block_size`, `padding_byte` |
| 20 | `LineEndingFn` | `line_ending@1.0.0` | Low | Convert `\r\n` ↔ `\n`. Params: `target` (`lf`/`crlf`) |

### Category 2: Compression (single input → single output)

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 21 | `CompressFn` | `compress@1.0.0` | Low | Zlib compress entire input (whole-stream, better ratio than per-chunk streaming) |
| 22 | `DecompressFn` | `decompress@1.0.0` | Low | Zlib decompress entire input |
| 23 | `ZstdCompressFn` | `zstd_compress@1.0.0` | Medium | Zstandard compress. Params: `level` (1–22) |
| 24 | `ZstdDecompressFn` | `zstd_decompress@1.0.0` | Medium | Zstandard decompress |
| 25 | `Lz4CompressFn` | `lz4_compress@1.0.0` | Low | LZ4 compress — fastest |
| 26 | `Lz4DecompressFn` | `lz4_decompress@1.0.0` | Low | LZ4 decompress |
| 27 | `SnappyCompressFn` | `snappy_compress@1.0.0` | Low | Snappy compress |
| 28 | `SnappyDecompressFn` | `snappy_decompress@1.0.0` | Low | Snappy decompress |
| 29 | `BrotliCompressFn` | `brotli_compress@1.0.0` | Medium | Brotli compress. Params: `quality` (0–11) |
| 30 | `BrotliDecompressFn` | `brotli_decompress@1.0.0` | Medium | Brotli decompress |

### Category 3: Cryptography & Hashing (single input → single output)

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 31 | `Sha256Fn` | `sha256@1.0.0` | Low | SHA-256 hash. Returns 32-byte digest |
| 32 | `Sha512Fn` | `sha512@1.0.0` | Low | SHA-512 hash. Returns 64-byte digest |
| 33 | `Md5Fn` | `md5@1.0.0` | Low | MD5 hash (legacy/S3 ETag compat). Returns 16-byte digest |
| 34 | `Blake3Fn` | `blake3@1.0.0` | Low | BLAKE3 hash — fastest. Returns 32-byte digest |
| 35 | `HmacSha256Fn` | `hmac_sha256@1.0.0` | Low | HMAC-SHA256 keyed hash. Params: `key`. Returns 32-byte MAC |
| 36 | `Crc32Fn` | `crc32@1.0.0` | Low | CRC32 checksum. Returns 4 bytes |
| 37 | `EncryptFn` | `encrypt@1.0.0` | Medium | AES-256-CTR encrypt. Params: `key`, `nonce` |
| 38 | `DecryptFn` | `decrypt@1.0.0` | Medium | AES-256-CTR decrypt. Params: `key`, `nonce` |
| 39 | `AeadEncryptFn` | `aead_encrypt@1.0.0` | Medium | AES-256-GCM authenticated encrypt. Appends 16-byte tag |
| 40 | `AeadDecryptFn` | `aead_decrypt@1.0.0` | Medium | AES-256-GCM authenticated decrypt. Verifies tag |
| 41 | `RedactFn` | `redact@1.0.0` | Medium | Regex-based PII redaction. Params: `patterns[]` |

### Category 4: Accumulators / Analytics (single input → small output)

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 42 | `ByteCountFn` | `byte_count@1.0.0` | Low | Return input length as 8-byte big-endian u64 |
| 43 | `LineCountFn` | `line_count@1.0.0` | Low | Count `\n` bytes, return as u64 |
| 44 | `WordCountFn` | `word_count@1.0.0` | Low | Count whitespace-delimited words, return as u64 |
| 45 | `HistogramFn` | `histogram@1.0.0` | Low | 256-bucket byte frequency histogram. Returns 2KB |
| 46 | `EntropyFn` | `entropy@1.0.0` | Low | Shannon entropy of byte distribution. Returns f64 (8 bytes) |
| 47 | `MinMaxFn` | `min_max@1.0.0` | Low | Min and max byte values. Returns `[min, max]` (2 bytes) |
| 48 | `SumFn` | `sum@1.0.0` | Low | Parse newline-delimited decimals, return sum as string |
| 49 | `AverageFn` | `average@1.0.0` | Low | Parse newline-delimited decimals, return average as string |

### Category 5: Combiners (multi-input → single output)

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 50 | `InterleaveFn` | `interleave@1.0.0` | Medium | Round-robin bytes from each input in chunk-sized blocks. Params: `block_size` |
| 51 | `ZipConcatFn` | `zip_concat@1.0.0` | Low | Pairwise concatenation of 2 inputs in blocks |
| 52 | `DiffFn` | `diff@1.0.0` | High | Binary diff between 2 inputs. Returns edit script |
| 53 | `PatchFn` | `patch@1.0.0` | High | Apply binary patch (input 0 = base, input 1 = patch) |
| 54 | `MergeSortedFn` | `merge_sorted@1.0.0` | Medium | Merge N sorted line-delimited inputs into one sorted output |
| 55 | `SelectFn` | `select@1.0.0` | Low | Return input at index N. Params: `index`. For conditional pipelines |

### Category 6: Slicing & Restructuring (single input → single output)

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 56 | `TakeFn` | `take@1.0.0` | Low | Return first N bytes. Params: `bytes` |
| 57 | `SkipFn` | `skip@1.0.0` | Low | Return all bytes after offset N. Params: `bytes` |
| 58 | `SliceFn` | `slice@1.0.0` | Low | Return bytes [offset..offset+length]. Params: `offset`, `length` |
| 59 | `SortFn` | `sort@1.0.0` | Medium | Sort lines lexicographically |
| 60 | `UniqueFn` | `unique@1.0.0` | Medium | Deduplicate sorted lines (like `uniq`) |
| 61 | `SortUniqueFn` | `sort_unique@1.0.0` | Medium | Sort + deduplicate in one pass |
| 62 | `ShuffleFn` | `shuffle@1.0.0` | Medium | Randomly reorder lines. Params: `seed` (deterministic) |
| 63 | `HeadFn` | `head@1.0.0` | Low | Return first N lines. Params: `lines` |
| 64 | `TailFn` | `tail@1.0.0` | Low | Return last N lines. Params: `lines` |
| 65 | `SampleFn` | `sample@1.0.0` | Medium | Reservoir sample N lines. Params: `lines`, `seed` |

### Category 7: Text Processing (single input → single output)

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 66 | `ReplaceFn` | `replace@1.0.0` | Medium | Byte pattern find-and-replace. Params: `find`, `replace` |
| 67 | `RegexReplaceFn` | `regex_replace@1.0.0` | Medium | Regex find-and-replace. Params: `pattern`, `replacement` |
| 68 | `GrepFn` | `grep@1.0.0` | Medium | Keep lines matching regex. Params: `pattern` |
| 69 | `GrepInvertFn` | `grep_invert@1.0.0` | Medium | Drop lines matching regex. Params: `pattern` |
| 70 | `PrefixFn` | `prefix@1.0.0` | Low | Prepend bytes. Params: `prefix` |
| 71 | `SuffixFn` | `suffix@1.0.0` | Low | Append bytes. Params: `suffix` |
| 72 | `LinePrefixFn` | `line_prefix@1.0.0` | Medium | Prepend string to each line. Params: `prefix` |
| 73 | `LineNumberFn` | `line_number@1.0.0` | Medium | Prepend line numbers (`1: `, `2: `, ...) |
| 74 | `TruncateLinesFn` | `truncate_lines@1.0.0` | Low | Truncate each line to max N bytes. Params: `max_line_bytes` |
| 75 | `CharsetConvertFn` | `charset_convert@1.0.0` | Medium | Convert encoding (e.g., Latin-1 → UTF-8). Params: `from`, `to` |

### Category 8: Validation & Integrity (single input → input or error)

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 76 | `Utf8ValidateFn` | `utf8_validate@1.0.0` | Low | Return input unchanged if valid UTF-8, error otherwise |
| 77 | `JsonValidateFn` | `json_validate@1.0.0` | Medium | Return input unchanged if valid JSON, error otherwise |
| 78 | `SchemaValidateFn` | `schema_validate@1.0.0` | High | Validate JSON against JSON Schema. Params: `schema` |
| 79 | `MagicBytesFn` | `magic_bytes@1.0.0` | Low | Verify input starts with expected bytes. Params: `expected` |
| 80 | `SizeLimitFn` | `size_limit@1.0.0` | Low | Error if input exceeds limit. Params: `max_bytes` |
| 81 | `NonEmptyFn` | `non_empty@1.0.0` | Low | Error if input is 0 bytes |
| 82 | `Sha256VerifyFn` | `sha256_verify@1.0.0` | Low | Compute SHA-256, error if mismatch. Params: `expected_hash` |
| 83 | `Crc32VerifyFn` | `crc32_verify@1.0.0` | Low | Compute CRC32, error if mismatch. Params: `expected_crc32` |

### Category 9: Format Conversion (single input → single output)

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 84 | `JsonPrettyPrintFn` | `json_pretty@1.0.0` | Medium | Parse JSON, re-emit pretty-printed |
| 85 | `JsonMinifyFn` | `json_minify@1.0.0` | Medium | Parse JSON, re-emit compact |
| 86 | `CsvToJsonFn` | `csv_to_json@1.0.0` | High | Parse CSV, emit JSON array of objects |
| 87 | `JsonToCsvFn` | `json_to_csv@1.0.0` | High | Parse JSON array of objects, emit CSV |
| 88 | `JsonLinesFn` | `json_lines@1.0.0` | Low | Ensure each line is newline-terminated (NDJSON formatting) |
| 89 | `YamlToJsonFn` | `yaml_to_json@1.0.0` | Medium | Parse YAML, emit JSON |
| 90 | `JsonToYamlFn` | `json_to_yaml@1.0.0` | Medium | Parse JSON, emit YAML |
| 91 | `TomlToJsonFn` | `toml_to_json@1.0.0` | Medium | Parse TOML, emit JSON |

### Category 10: CAS-Specific Operations

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 92 | `CAddrComputeFn` | `caddr_compute@1.0.0` | Low | Compute CAddr (content-address) of input. Returns the address bytes |
| 93 | `CAddrVerifyFn` | `caddr_verify@1.0.0` | Medium | Compute CAddr, error if mismatch. Params: `expected_caddr` |
| 94 | `CAddrEmbedFn` | `caddr_embed@1.0.0` | Medium | Append CAddr as trailing metadata to the output |
| 95 | `MerkleRootFn` | `merkle_root@1.0.0` | High | Build Merkle tree over fixed-size blocks, return 32-byte root hash. Params: `block_size` |
| 96 | `ContentTypeFn` | `content_type@1.0.0` | Medium | Detect MIME type from magic bytes, prepend as metadata header |
| 97 | `ChunkHashFn` | `chunk_hash@1.0.0` | Medium | Split input into fixed blocks, append SHA-256 hash per block. Params: `block_size` |
| 98 | `DedupAnalyzeFn` | `dedup_analyze@1.0.0` | High | Compute rolling hash fingerprints for content-defined chunking analysis. Params: `window_size` |

### Batch-Only Functions (no streaming equivalent)

These functions benefit from having the full input in memory — they cannot be meaningfully streamed.

| # | Function | FunctionId | Complexity | Description |
|---|----------|-----------|------------|-------------|
| 99 | `GlobalSortFn` | `global_sort@1.0.0` | Medium | Sort all bytes (not lines). Useful for binary deduplication analysis |
| 100 | `GlobalReverseFn` | `global_reverse@1.0.0` | Low | Reverse entire byte sequence (streaming `Reverse` only reverses per-chunk) |

### Summary

| Status | Count | Categories |
|--------|-------|------------|
| **Existing (implemented)** | 4 | Identity, Concat, Uppercase, Repeat |
| **New proposed** | 96 | See above: transforms (16), compression (10), crypto (11), accumulators (8), combiners (6), slicing (10), text (10), validation (8), format conversion (8), CAS-specific (7), batch-only (2) |
| **Total library target** | **100** | Full coverage of common batch data operations |

### Implementation Priority

1. **High priority — streaming parity** (low complexity, direct counterparts to existing streaming functions): `LowercaseFn`, `ReverseFn`, `Base64EncodeFn/DecodeFn`, `HexEncodeFn/DecodeFn`, `XorFn`, `CompressFn/DecompressFn`, `Sha256Fn`, `ByteCountFn`, `Crc32Fn`, `TakeFn`, `SkipFn`
2. **Medium priority — common operations**: `ZstdCompressFn/DecompressFn`, `Blake3Fn`, `GrepFn`, `ReplaceFn`, `SortFn`, `UniqueFn`, `JsonValidateFn`, `JsonPrettyPrintFn/MinifyFn`, `CAddrComputeFn/VerifyFn`
3. **Low priority — niche or high complexity**: `DiffFn/PatchFn`, `SchemaValidateFn`, `CsvToJsonFn/JsonToCsvFn`, `MerkleRootFn`, `DedupAnalyzeFn`, `ShuffleFn`

### Batch vs Streaming Parity Matrix

Not all functions need both implementations. This matrix clarifies which functions are batch-only, streaming-only, or both:

| Pattern | Batch | Streaming | Rationale |
|---------|-------|-----------|-----------|
| Transforms (map) | ✅ | ✅ | Both: batch is simpler, streaming handles large inputs |
| Compression | ✅ | ✅ | Both: batch gets better ratios (whole-input dictionary), streaming bounds memory |
| Hashing/Accumulators | ✅ | ✅ | Both: batch is trivial, streaming avoids buffering |
| Combiners | ✅ | ✅ | Both: batch uses `Vec<Bytes>` inputs, streaming uses multi-receiver |
| Flow control (rate limit, timeout) | ❌ | ✅ | Streaming-only: no concept of "rate" in batch |
| Global sort/reverse | ✅ | ❌ | Batch-only: requires full input to produce correct output |
| Format conversion (JSON↔CSV) | ✅ | ⚠️ | Batch preferred: parsing requires full structure; streaming version buffers anyway |
| Validation | ✅ | ⚠️ | Batch preferred: most validators need full input; streaming can do prefix checks |

---

## 8. Format-Aware Function Library

**See:** [`format-aware-functions.md`](format-aware-functions.md) — 283 functions across 19 categories covering all major file formats encountered in distributed file systems (columnar, row-oriented, serialization, archive, image, audio/video, document, config, geospatial, scientific, log, content-addressing, database, ML, network, bioinformatics, 3D/binary, erasure coding, universal detection). Combined with §6 and §7, the total library target is **483 functions**.

---

## 9. Channel Capacity Sweep Study — ✅ COMPLETED

> **Status:** Implemented and benchmarked. Results published in Paper 2b §8.7. The §3.4 backpressure section now references empirical data instead of informal claims.

**Summary of results** (7 capacities × 6 input sizes, 5-stage identity pipeline, Criterion.rs median):

| Capacity | 1 MB | 4 MB | 16 MB | 64 MB | 256 MB | 512 MB |
|----------|------|------|-------|-------|--------|--------|
| 1 | 2.72 ms | 7.01 ms | 22.7 ms | 130.3 ms | 501.1 ms | 1,128 ms |
| **8 (default)** | **1.75 ms** | **2.72 ms** | **7.75 ms** | **50.8 ms** | **185.2 ms** | **368.7 ms** |
| 64 | 1.66 ms | 2.82 ms | 4.63 ms | 43.3 ms | 132.9 ms | 245.3 ms |

**Verdict:** Capacity 8 captures 80–90% of available throughput gain while using only 2.5 MB per 5-stage pipeline. Capacity 16 is a reasonable alternative for ≥256 MB workloads (14–21% faster, 2× memory). Capacity 32+ is not justified by the data.
