# §2.14 Streaming Function Library Expansion

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 5–7 days (80 functions across 9 categories)

---

## 1. Problem Statement

### 1.1 Current State

TODO — 20 built-in streaming functions (9 transforms, 3 accumulators, 3 combiners, 5 utilities); covers basic pipeline testing but insufficient for real workloads

### 1.2 Coverage Gaps

TODO — no cryptography, no additional compression codecs (zstd/lz4/snappy/brotli), no text processing (grep/sed/replace), no validation, no CAS-specific operations, limited analytics

### 1.3 Goal

TODO — grow to 100 streaming functions covering crypto, encoding, analytics, compression, flow control, validation, text processing, CAS-specific, and numeric operations

---

## 2. Design

### 2.1 Implementation Patterns

TODO — reusable helpers: `spawn_map()` for stateless per-chunk transforms, `spawn_accumulate()` for fold-style accumulators; new functions follow established patterns

### 2.2 Category Overview

TODO — table of 9 categories with function counts:
- Cryptography & Security (9)
- Encoding & Format Conversion (10)
- Data Processing & Analytics (11)
- Compression & Transformation (10)
- Flow Control & Pipeline Utilities (10)
- Validation & Integrity (7)
- Text Processing (8)
- CAS-Specific Operations (7)
- Numeric & Scientific (8)

### 2.3 Dependency Strategy

TODO — new crate dependencies: `aes`, `ctr`, `aes-gcm`, `hmac`, `sha2` (already present), `md-5`, `blake3`, `zstd`, `lz4_flex`, `snap`, `brotli`, `regex`

### 2.4 Implementation Priority

TODO — 3 tiers: high (most useful, low complexity), medium (useful, moderate complexity), low (niche or high complexity)

---

## 3. Function Specifications

### 3.1 Cryptography & Security (9 functions)

#### 3.1.1 StreamingEncrypt (#21)
TODO — AES-256-CTR per-chunk encryption; counter state across chunks; params: `key`, `nonce`; pattern: map; memory: O(chunk); complexity: Medium

#### 3.1.2 StreamingDecrypt (#22)
TODO — AES-256-CTR per-chunk decryption; inverse of Encrypt

#### 3.1.3 StreamingAeadEncrypt (#23)
TODO — AES-256-GCM authenticated encryption; appends 16-byte tag per chunk; pattern: buffered

#### 3.1.4 StreamingAeadDecrypt (#24)
TODO — AES-256-GCM authenticated decryption; verifies tag per chunk; emits Error on tamper

#### 3.1.5 StreamingHmacSha256 (#25)
TODO — HMAC-SHA256 keyed hash; accumulator; emits 32-byte MAC on End

#### 3.1.6 StreamingMd5 (#26)
TODO — MD5 digest for legacy/S3 ETag compatibility; accumulator; emits 16 bytes

#### 3.1.7 StreamingSha512 (#27)
TODO — SHA-512 digest; accumulator; emits 64 bytes

#### 3.1.8 StreamingBlake3 (#28)
TODO — BLAKE3 hash; faster than SHA-256; accumulator; emits 32 bytes

#### 3.1.9 StreamingRedact (#29)
TODO — regex-based PII redaction; replaces matches with `[REDACTED]`; params: `patterns[]`; map

### 3.2 Encoding & Format Conversion (10 functions)

#### 3.2.1 StreamingHexEncode (#30)
TODO — encode each byte as 2 hex chars; output 2× input; map

#### 3.2.2 StreamingHexDecode (#31)
TODO — decode hex to bytes; Error on invalid hex; map

#### 3.2.3 StreamingUtf8Validate (#32)
TODO — pass-through validating UTF-8; boundary-aware (buffers up to 3 trailing bytes for split multi-byte chars); map

#### 3.2.4 StreamingLineEnding (#33)
TODO — convert `\r\n` ↔ `\n`; params: `target` (lf/crlf); map

#### 3.2.5 StreamingJsonPrettyPrint (#34)
TODO — buffer full JSON, re-emit pretty-printed; buffered; O(input)

#### 3.2.6 StreamingJsonMinify (#35)
TODO — buffer full JSON, strip whitespace; buffered; O(input)

#### 3.2.7 StreamingJsonLines (#36)
TODO — ensure each chunk ends with `\n` for NDJSON; map

#### 3.2.8 StreamingCsvToJson (#37)
TODO — parse CSV header, convert rows to JSON objects; stateful; O(header+chunk); High complexity

#### 3.2.9 StreamingBase32Encode (#38)
TODO — Base32 encoding per chunk; map

#### 3.2.10 StreamingBase32Decode (#39)
TODO — Base32 decoding per chunk; map

### 3.3 Data Processing & Analytics (11 functions)

#### 3.3.1 StreamingFilter (#40)
TODO — drop chunks where predicate returns false; params: `predicate` (non_empty, contains:pattern); map

#### 3.3.2 StreamingLineCount (#41)
TODO — count `\n` bytes; accumulator; emit u64 on End

#### 3.3.3 StreamingWordCount (#42)
TODO — count whitespace-delimited words; tracks in-word state; accumulator

#### 3.3.4 StreamingMinMax (#43)
TODO — track min/max byte values; accumulator; emit [min, max] on End

#### 3.3.5 StreamingHistogram (#44)
TODO — 256-bucket byte frequency histogram; accumulator; emit 2 KB on End

#### 3.3.6 StreamingSample (#45)
TODO — reservoir sampling: emit every Nth chunk; params: `rate`; offset-tracking

#### 3.3.7 StreamingHead (#46)
TODO — emit first N chunks (not bytes); params: `chunks`; offset-tracking

#### 3.3.8 StreamingTail (#47)
TODO — emit last N chunks; ring buffer of size N; params: `chunks`; O(N×chunk)

#### 3.3.9 StreamingDeduplicate (#48)
TODO — drop duplicate chunks by content hash; O(seen_hashes); set-based

#### 3.3.10 StreamingSort (#49)
TODO — buffer all chunks, sort by byte order, re-emit; O(input); buffer-all

#### 3.3.11 StreamingUnique (#50)
TODO — buffer all, deduplicate, re-emit sorted unique; O(input); buffer-all

### 3.4 Compression & Transformation (10 functions)

#### 3.4.1 StreamingZstdCompress (#51)
TODO — Zstandard per-chunk compression; map; Medium

#### 3.4.2 StreamingZstdDecompress (#52)
TODO — Zstandard per-chunk decompression; map

#### 3.4.3 StreamingLz4Compress (#53)
TODO — LZ4 per-chunk; fastest compression; map; Low complexity

#### 3.4.4 StreamingLz4Decompress (#54)
TODO — LZ4 per-chunk decompression; map

#### 3.4.5 StreamingSnappyCompress (#55)
TODO — Snappy per-chunk; map; Low complexity

#### 3.4.6 StreamingSnappyDecompress (#56)
TODO — Snappy per-chunk decompression; map

#### 3.4.7 StreamingBrotliCompress (#57)
TODO — Brotli per-chunk; best ratio for text; map; Medium

#### 3.4.8 StreamingBrotliDecompress (#58)
TODO — Brotli per-chunk decompression; map

#### 3.4.9 StreamingPad (#59)
TODO — pad each chunk to fixed size; params: `block_size`, `padding_byte`; map

#### 3.4.10 StreamingTrim (#60)
TODO — strip leading/trailing whitespace per chunk; map

### 3.5 Flow Control & Pipeline Utilities (10 functions)

#### 3.5.1 StreamingRateLimit (#61)
TODO — throttle to target bytes/sec; `tokio::time::sleep` between sends; params: `bytes_per_sec`; pass-through

#### 3.5.2 StreamingDelay (#62)
TODO — fixed delay before each chunk; params: `delay_ms`; for testing backpressure

#### 3.5.3 StreamingTimeout (#63)
TODO — emit Error if no chunk within deadline; params: `timeout_ms`; pass-through

#### 3.5.4 StreamingRetry (#64)
TODO — on upstream Error, restart upstream up to N times; params: `max_retries`; wrapper; High complexity

#### 3.5.5 StreamingTee (#65)
TODO — duplicate input to N output receivers; fan-out; O(chunk×N)

#### 3.5.6 StreamingMerge (#66)
TODO — select-based merge: emit whichever input ready first; non-deterministic; multi-recv

#### 3.5.7 StreamingBroadcast (#67)
TODO — like Tee but slowest consumer gates producer; fan-out with backpressure

#### 3.5.8 StreamingPartition (#68)
TODO — route chunks to 1 of 2 outputs based on predicate; params: `predicate`; fan-out

#### 3.5.9 StreamingBatch (#69)
TODO — collect N chunks into one large chunk; inverse of ChunkResizer; params: `batch_size`

#### 3.5.10 StreamingDebounce (#70)
TODO — suppress rapid consecutive chunks; emit after quiet period; params: `window_ms`

### 3.6 Validation & Integrity (7 functions)

#### 3.6.1 StreamingJsonValidate (#71)
TODO — buffer full input, parse as JSON, emit unchanged or Error; buffered

#### 3.6.2 StreamingSchemaValidate (#72)
TODO — validate JSON against JSON Schema; params: `schema`; buffered; High complexity

#### 3.6.3 StreamingMagicBytes (#73)
TODO — check first chunk starts with expected bytes; params: `expected`; Error on mismatch; first-chunk only

#### 3.6.4 StreamingSizeLimit (#74)
TODO — track total bytes; Error if limit exceeded; params: `max_bytes`; offset-tracking

#### 3.6.5 StreamingChecksumVerify (#75)
TODO — compute CRC32, compare against expected; params: `expected_crc32`; accumulator

#### 3.6.6 StreamingSha256Verify (#76)
TODO — compute SHA-256, compare against expected; params: `expected_hash`; accumulator

#### 3.6.7 StreamingNonEmpty (#77)
TODO — Error if stream ends with zero data chunks; pass-through

### 3.7 Text Processing (8 functions)

#### 3.7.1 StreamingReplace (#78)
TODO — byte pattern replacement per chunk; boundary-aware for patterns split across chunks; params: `find`, `replace`; map

#### 3.7.2 StreamingPrefix (#79)
TODO — prepend fixed bytes before first data chunk; params: `prefix`; first-chunk map

#### 3.7.3 StreamingSuffix (#80)
TODO — append fixed bytes after last data chunk; params: `suffix`; pass-through

#### 3.7.4 StreamingLinePrefix (#81)
TODO — prepend string to each line; params: `prefix_fn`; map

#### 3.7.5 StreamingGrep (#82)
TODO — keep only lines matching regex; boundary-aware; params: `pattern`; map

#### 3.7.6 StreamingSed (#83)
TODO — regex find-and-replace per line; params: `pattern`, `replacement`; map

#### 3.7.7 StreamingTruncateLines (#84)
TODO — truncate each line to max N bytes; params: `max_line_bytes`; map

#### 3.7.8 StreamingCharsetConvert (#85)
TODO — convert between encodings (e.g., Latin-1 → UTF-8); params: `from`, `to`; map

### 3.8 CAS-Specific Operations (7 functions)

#### 3.8.1 StreamingCAddrEmbed (#86)
TODO — compute CAddr of stream, embed as trailing metadata chunk; pass-through + SHA-256 accumulation

#### 3.8.2 StreamingCAddrVerify (#87)
TODO — compute CAddr, verify against expected; params: `expected_caddr`; accumulator; Error on mismatch

#### 3.8.3 StreamingDiff (#88)
TODO — two inputs chunk-by-chunk, emit binary diff; multi-recv pairwise; High complexity

#### 3.8.4 StreamingPatch (#89)
TODO — apply binary patch stream to base stream; inverse of Diff; multi-recv; High complexity

#### 3.8.5 StreamingMerkleTree (#90)
TODO — build Merkle tree over chunks; emit root hash on End; accumulator; O(depth×32B)

#### 3.8.6 StreamingContentType (#91)
TODO — detect MIME type from first chunk magic bytes; prepend metadata chunk; first-chunk map

#### 3.8.7 StreamingChunkHash (#92)
TODO — append 32-byte SHA-256 hash to each chunk; per-chunk integrity; map

### 3.9 Numeric & Scientific (8 functions)

#### 3.9.1 StreamingSum (#93)
TODO — parse newline-delimited decimals, sum; accumulator; emit total on End

#### 3.9.2 StreamingAverage (#94)
TODO — running sum + count; accumulator; emit average on End

#### 3.9.3 StreamingBitwiseAnd (#95)
TODO — AND each byte with mask; params: `mask`; map

#### 3.9.4 StreamingBitwiseOr (#96)
TODO — OR each byte with mask; params: `mask`; map

#### 3.9.5 StreamingBitwiseNot (#97)
TODO — complement each byte; map

#### 3.9.6 StreamingByteSwap (#98)
TODO — swap byte order within 2/4/8-byte words; params: `word_size`; map

#### 3.9.7 StreamingEntropy (#99)
TODO — Shannon entropy via byte histogram; accumulator; emit f64 on End

#### 3.9.8 StreamingRollingHash (#100)
TODO — Rabin fingerprint over sliding window; params: `window_size`; map; useful for content-defined chunking

---

## 4. Implementation Strategy

### 4.1 Helper Reuse

TODO — most new functions use existing `spawn_map()` or `spawn_accumulate()` helpers; only flow control and CAS-specific need custom task spawning

### 4.2 Boundary-Aware Processing

TODO — text functions (grep, replace, sed) must handle patterns split across chunk boundaries; strategy: buffer trailing partial line, prepend to next chunk

### 4.3 Fusibility Annotations

TODO — which new functions are fusible (pure map, stateless): HexEncode/Decode, BitwiseAnd/Or/Not, Pad, Trim, LineEnding, Base32Encode/Decode (~12 of 80)

---

## 5. Test Specification

### 5.1 Per-Function Unit Tests

TODO — each function gets: basic correctness, multi-chunk input, empty input, error propagation; ~3 tests per function = ~240 tests

### 5.2 Roundtrip Tests

TODO — encode/decode pairs: Hex, Base32, Encrypt/Decrypt, AEAD, Compress/Decompress (zstd, lz4, snappy, brotli); ~8 roundtrip tests

### 5.3 Boundary Tests

TODO — text functions with patterns split across chunks: grep, replace, sed, UTF-8 validate; ~6 tests

### 5.4 Pipeline Composition Tests

TODO — multi-stage pipelines combining new functions; ~5 integration tests

### 5.5 Benchmark Suite

TODO — throughput comparison across compression codecs (zlib vs zstd vs lz4 vs snappy vs brotli); hash throughput (SHA-256 vs SHA-512 vs BLAKE3 vs MD5)

---

## 6. Edge Cases & Error Handling

TODO — table: invalid key/nonce for crypto, invalid hex/base32 input, regex compilation failure, zero-length chunks through analytics, timeout with no data, retry with persistent failure, partition with all-true/all-false predicate

---

## 7. Performance Analysis

### 7.1 Compression Codec Comparison

TODO — expected throughput table: zlib (~200 MB/s), zstd (~500 MB/s), lz4 (~2 GB/s), snappy (~1.5 GB/s), brotli (~50 MB/s)

### 7.2 Hash Algorithm Comparison

TODO — expected throughput: MD5 (~800 MB/s), SHA-256 (~500 MB/s), SHA-512 (~700 MB/s), BLAKE3 (~5 GB/s)

### 7.3 Crypto Overhead

TODO — AES-256-CTR ~3 GB/s on modern CPUs; negligible per-chunk overhead

---

## 8. Files Changed

TODO — table: `builtins_streaming.rs` (extend), new test files, `Cargo.toml` dependencies

---

## 9. Dependency Changes

TODO — table: `aes` 0.8, `ctr` 0.9, `aes-gcm` 0.10, `hmac` 0.12, `md-5` 0.10, `blake3` 1, `zstd` 0.13, `lz4_flex` 0.11, `snap` 1, `brotli` 6, `regex` 1, `data-encoding` 2 (for base32)

---

## 10. Design Rationale

### 10.1 Why Per-Chunk Compression Instead of Whole-Stream?

TODO — per-chunk is simpler and composable; whole-stream compression (§2.16 Category D) is a separate format-aware function with better ratios but requires state across chunks

### 10.2 Why Per-Chunk Crypto Instead of Whole-Stream?

TODO — CTR mode is naturally per-chunk (counter increments); GCM per-chunk gives per-chunk authentication; whole-stream GCM would require buffering

### 10.3 Why Include Legacy Algorithms (MD5)?

TODO — S3 ETags use MD5; compatibility with existing data lake infrastructure requires it

---

## 11. Observability Integration

TODO — per-function execution counter `deriva_streaming_fn_calls_total{function="..."}`, per-function error counter, per-function duration histogram

---

## 12. Checklist

- [ ] Implement crypto functions (#21–#29) — 9 functions
- [ ] Implement encoding functions (#30–#39) — 10 functions
- [ ] Implement analytics functions (#40–#50) — 11 functions
- [ ] Implement compression functions (#51–#60) — 10 functions
- [ ] Implement flow control functions (#61–#70) — 10 functions
- [ ] Implement validation functions (#71–#77) — 7 functions
- [ ] Implement text processing functions (#78–#85) — 8 functions
- [ ] Implement CAS-specific functions (#86–#92) — 7 functions
- [ ] Implement numeric functions (#93–#100) — 8 functions
- [ ] Add fusibility annotations to pure map functions
- [ ] Register all 80 new functions in `FunctionRegistry`
- [ ] Unit tests (~240 tests, ~3 per function)
- [ ] Roundtrip tests for encode/decode pairs (~8 tests)
- [ ] Boundary tests for text functions (~6 tests)
- [ ] Pipeline composition tests (~5 tests)
- [ ] Benchmark: compression codec comparison
- [ ] Benchmark: hash algorithm comparison
- [ ] Add per-function observability metrics
- [ ] Update `Cargo.toml` with new dependencies
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing tests still pass
- [ ] Commit and push
