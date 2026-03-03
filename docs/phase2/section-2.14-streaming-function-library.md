# §2.14 Streaming Function Library Expansion

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 5–7 days (80 functions across 9 categories)

---

## 1. Problem Statement

### 1.1 Current State

The streaming function library in `builtins_streaming.rs` provides 20
functions across 4 categories:

```
Category 1 — Single-Input Transforms (9 functions, #1–#9):
  spawn_map() pattern — stateless per-chunk byte transforms
  ┌──────────────────────────────────────────────────────────────┐
  │ #1  StreamingIdentity      pass-through (no-op)              │
  │ #2  StreamingUppercase     b.to_ascii_uppercase()            │
  │ #3  StreamingLowercase     b.to_ascii_lowercase()            │
  │ #4  StreamingReverse       v.reverse()                       │
  │ #5  StreamingBase64Encode  STANDARD.encode(b)                │
  │ #6  StreamingBase64Decode  STANDARD.decode(b)                │
  │ #7  StreamingXor           byte ^ key per byte               │
  │ #8  StreamingCompress      flate2 ZlibEncoder per chunk      │
  │ #9  StreamingDecompress    flate2 ZlibDecoder per chunk      │
  └──────────────────────────────────────────────────────────────┘

Category 2 — Single-Input Accumulators (3 functions, #10–#12):
  spawn_accumulate() pattern — fold all chunks, emit single result on End
  ┌──────────────────────────────────────────────────────────────┐
  │ #10 StreamingSha256        SHA-256 digest → 32 bytes         │
  │ #11 StreamingByteCount     total bytes → u64 big-endian      │
  │ #12 StreamingChecksum      CRC32 → 4 bytes big-endian        │
  └──────────────────────────────────────────────────────────────┘

Category 3 — Multi-Input Combiners (3 functions, #13–#15):
  Custom task spawning — fan-in from N inputs
  ┌──────────────────────────────────────────────────────────────┐
  │ #13 StreamingConcat        sequential read of N inputs       │
  │ #14 StreamingInterleave    round-robin from N inputs         │
  │ #15 StreamingZipConcat     pair-wise concat from 2 inputs    │
  └──────────────────────────────────────────────────────────────┘

Category 4 — Pipeline Utilities (5 functions, #16–#20):
  Custom task spawning — stateful per-chunk processing
  ┌──────────────────────────────────────────────────────────────┐
  │ #16 StreamingChunkResizer  re-chunk to target_size           │
  │ #17 StreamingTake          first N bytes                     │
  │ #18 StreamingSkip          skip first N bytes                │
  │ #19 StreamingRepeat        buffer + repeat N times           │
  │ #20 StreamingTeeCount      pass-through + append byte count  │
  └──────────────────────────────────────────────────────────────┘
```

These 20 functions are sufficient for testing the streaming pipeline
infrastructure (§2.7) but inadequate for production workloads.

### 1.2 Coverage Gaps

| Domain | What's Missing | Impact |
|--------|---------------|--------|
| Cryptography | No encryption (AES-CTR/GCM), no HMAC, no BLAKE3, no SHA-512, no MD5 | Cannot build secure pipelines; no S3 ETag compatibility |
| Compression | Only zlib; no zstd, lz4, snappy, brotli | zlib is 4–10× slower than lz4/zstd; no codec choice |
| Text processing | No grep, sed, replace, line prefix, charset conversion | Cannot process log files, CSV, or text streams |
| Validation | No JSON validation, schema validation, magic bytes, size limits | No input sanitization or format verification |
| Analytics | No line/word count, histogram, sampling, dedup, sort | Cannot build data analysis pipelines |
| Flow control | No rate limiting, timeout, retry, tee, merge, partition | Cannot build resilient or fan-out pipelines |
| CAS-specific | No CAddr embed/verify, Merkle tree, content type detection | Cannot leverage content-addressing in pipelines |
| Numeric | No sum, average, bitwise ops, entropy, rolling hash | Cannot build numeric processing pipelines |
| Encoding | No hex, base32, UTF-8 validation, line ending conversion, JSON formatting | Limited format conversion options |

The 20 existing functions cover only 2 of 11 domains (basic transforms
and basic accumulators). Real-world pipelines require at minimum:
encryption for data-at-rest, multiple compression codecs for
performance/ratio tradeoffs, text processing for log pipelines, and
validation for input sanitization.

### 1.3 Comparison: Current vs Target

| Dimension | Current (20 functions) | Target (100 functions) |
|-----------|----------------------|----------------------|
| Categories | 4 | 13 (4 existing + 9 new) |
| Transform patterns | `spawn_map` only | `spawn_map`, boundary-aware map, buffered, multi-recv |
| Accumulator patterns | `spawn_accumulate` only | `spawn_accumulate`, offset-tracking, set-based, ring-buffer |
| Compression codecs | zlib (1) | zlib, zstd, lz4, snappy, brotli (5) |
| Hash algorithms | SHA-256 (1) | SHA-256, SHA-512, MD5, BLAKE3, CRC32, HMAC-SHA256 (6) |
| Crypto | XOR only (toy) | AES-256-CTR, AES-256-GCM, HMAC, PII redaction (4) |
| Text processing | None | grep, sed, replace, line prefix, charset convert, truncate (8) |
| Validation | None | JSON, JSON Schema, magic bytes, size limit, checksum verify (7) |
| Flow control | None | rate limit, timeout, retry, tee, merge, partition, batch (10) |
| Fusible functions | 9 (#1–#9) | ~21 (9 existing + ~12 new pure maps) |
| External dependencies | `base64`, `flate2`, `sha2`, `crc32fast` | +`aes`, `ctr`, `aes-gcm`, `hmac`, `md-5`, `blake3`, `zstd`, `lz4_flex`, `snap`, `brotli`, `regex`, `data-encoding` |

### 1.4 Real-World Pipeline Examples That Cannot Be Built Today

```
1. Secure data pipeline (requires #21, #25, #51):
   Source → ZstdCompress → AesEncrypt → HmacSha256
   ✗ No encryption, no HMAC, no zstd

2. Log processing pipeline (requires #82, #41, #44):
   Source → Grep("ERROR") → LineCount → Histogram
   ✗ No grep, no line count, no histogram

3. Data validation pipeline (requires #71, #74, #76):
   Source → JsonValidate → SizeLimit(10MB) → Sha256Verify(expected)
   ✗ No JSON validation, no size limit, no hash verify

4. S3-compatible ingest (requires #26, #51, #86):
   Source → ZstdCompress → Md5 → CAddrEmbed
   ✗ No MD5, no zstd, no CAddr embed

5. Fan-out analytics (requires #65, #10, #11, #44):
   Source → Tee(3) → [Sha256, ByteCount, Histogram]
   ✗ No tee (fan-out)
```

### 1.5 What Exists vs What's Missing

**Exists today:**

1. `spawn_map()` helper — spawns a task per stage, stateless per-chunk
   transform. Used by all 9 Category 1 functions.
2. `spawn_accumulate()` helper — fold all chunks into state, emit
   single result on End. Used by all 3 Category 2 functions.
3. `take_one()` helper — assert single input, extract receiver.
4. `StreamingComputeFunction` trait — `stream_execute()`,
   `supports_streaming()`, `preferred_chunk_size()`,
   `channel_capacity()`.
5. `StreamChunk` enum — `Data(Bytes)`, `End`, `Error(DerivaError)`.
6. `DEFAULT_CHUNK_SIZE` (64 KB) and `DEFAULT_CHANNEL_CAPACITY` (8).
7. Pipeline builder (`StreamPipeline::execute()`) that wires stages.

**Missing:**

1. 80 new streaming functions (#21–#100) across 9 categories
2. Boundary-aware map helper — buffers trailing partial line for text
   functions that operate on line boundaries
3. Buffered helper — collects full input, transforms, re-emits (for
   JSON pretty-print, sort, etc.)
4. Multi-receiver helper — fan-out/fan-in patterns (tee, merge,
   partition)
5. Offset-tracking helper — tracks cumulative byte position across
   chunks (for Take, Skip, SizeLimit, Head, Tail)
6. Fusibility annotations — `is_fusible()` trait method (from §2.11)
   for the ~12 new pure-map functions
7. Per-function observability — execution counters, error counters,
   duration histograms
8. 12 new crate dependencies for crypto, compression, and text
   processing
9. ~240 unit tests + 8 roundtrip tests + 6 boundary tests + 5
   integration tests + benchmarks
10. Function registry entries for all 80 new functions

---

## 2. Design

### 2.1 Implementation Patterns

Every new function reuses one of five helper patterns. The first two
exist today; the remaining three are new:

```
Pattern 1: spawn_map() [EXISTING]
  Stateless per-chunk transform. Input chunk → f(chunk) → output chunk.
  Used by: #1–#9 (existing), #21–#22, #30–#31, #38–#39, #51–#60,
           #79, #81, #84, #93–#97, #98, #100 (~35 functions)

Pattern 2: spawn_accumulate() [EXISTING]
  Fold all chunks into state S, emit finalize(S) on End.
  Used by: #10–#12 (existing), #25–#28, #41–#44, #75–#76, #87,
           #90, #93–#94, #99 (~15 functions)

Pattern 3: spawn_boundary_map() [NEW]
  Like spawn_map but buffers trailing partial line (up to last \n).
  Prepends leftover to next chunk. Flushes remainder on End.
  Used by: #32–#33, #78, #82–#83, #85 (~6 functions)

Pattern 4: spawn_buffered() [NEW]
  Collects entire input into Vec<u8>, applies transform, re-emits
  as chunked output. Memory = O(input). For transforms that need
  the full value (JSON parse, sort, dedup).
  Used by: #34–#36, #49–#50, #71–#72 (~6 functions)

Pattern 5: spawn_passthrough() [NEW]
  Forwards chunks unchanged while maintaining side-state (byte offset,
  timer, counter). Emits metadata or Error based on state.
  Used by: #46–#48, #61–#63, #65–#70, #73–#74, #77, #80, #86,
           #91 (~18 functions)
```

Helper signatures:

```rust
// Pattern 3: boundary-aware map
fn spawn_boundary_map(
    rx: mpsc::Receiver<StreamChunk>,
    cap: usize,
    f: impl Fn(&[u8]) -> Result<Bytes, String> + Send + 'static,
) -> mpsc::Receiver<StreamChunk>;

// Pattern 4: buffered transform
fn spawn_buffered(
    rx: mpsc::Receiver<StreamChunk>,
    cap: usize,
    chunk_size: usize,
    f: impl FnOnce(Bytes) -> Result<Bytes, String> + Send + 'static,
) -> mpsc::Receiver<StreamChunk>;

// Pattern 5: passthrough with side-state
fn spawn_passthrough<S: Send + 'static>(
    rx: mpsc::Receiver<StreamChunk>,
    cap: usize,
    init: S,
    on_chunk: impl Fn(&mut S, &Bytes) -> PassAction + Send + 'static,
    on_end: impl FnOnce(S) -> Option<Bytes> + Send + 'static,
) -> mpsc::Receiver<StreamChunk>;

enum PassAction {
    Forward,          // pass chunk unchanged
    Replace(Bytes),   // replace chunk
    Drop,             // suppress chunk
    Error(String),    // emit error, terminate
}
```

### 2.2 Category Overview

| # | Category | Count | Functions | Primary Pattern | New Deps |
|---|----------|-------|-----------|----------------|----------|
| 1 | Cryptography & Security | 9 | #21–#29 | map, accumulate | `aes`, `ctr`, `aes-gcm`, `hmac`, `md-5`, `blake3`, `regex` |
| 2 | Encoding & Format Conversion | 10 | #30–#39 | map, boundary_map, buffered | `data-encoding` |
| 3 | Data Processing & Analytics | 11 | #40–#50 | accumulate, passthrough, buffered | — |
| 4 | Compression & Transformation | 10 | #51–#60 | map | `zstd`, `lz4_flex`, `snap`, `brotli` |
| 5 | Flow Control & Pipeline Utilities | 10 | #61–#70 | passthrough, custom | — |
| 6 | Validation & Integrity | 7 | #71–#77 | buffered, passthrough, accumulate | — |
| 7 | Text Processing | 8 | #78–#85 | boundary_map, passthrough, map | — |
| 8 | CAS-Specific Operations | 7 | #86–#92 | passthrough, accumulate, custom | — |
| 9 | Numeric & Scientific | 8 | #93–#100 | map, accumulate | — |
| | **Total** | **80** | **#21–#100** | | **12 new crates** |

### 2.3 Dependency Strategy

All new dependencies are well-established, audited Rust crates:

| Crate | Version | Category | Size Impact | Why This Crate |
|-------|---------|----------|-------------|---------------|
| `aes` | 0.8 | Crypto | ~50 KB | RustCrypto; hardware AES-NI on x86_64 |
| `ctr` | 0.9 | Crypto | ~15 KB | CTR mode for `aes`; streaming-friendly |
| `aes-gcm` | 0.10 | Crypto | ~30 KB | AEAD authenticated encryption |
| `hmac` | 0.12 | Crypto | ~10 KB | Keyed MAC; pairs with `sha2` (existing) |
| `md-5` | 0.10 | Crypto | ~20 KB | S3 ETag compatibility; RustCrypto family |
| `blake3` | 1 | Crypto | ~100 KB | Fastest hash; SIMD-optimized; single crate |
| `zstd` | 0.13 | Compression | ~500 KB | C binding via `zstd-safe`; best ratio/speed |
| `lz4_flex` | 0.11 | Compression | ~60 KB | Pure Rust LZ4; fastest compression |
| `snap` | 1 | Compression | ~40 KB | Snappy; Google's fast compressor |
| `brotli` | 6 | Compression | ~200 KB | Best text compression ratio |
| `regex` | 1 | Text | ~300 KB | Grep, sed, redact; well-optimized |
| `data-encoding` | 2 | Encoding | ~30 KB | Base32; no-std compatible |

Total added: ~1,355 KB of compiled dependencies. All are optional
behind `feature = "extended-streaming"` to keep the base binary lean.

Feature flag in `Cargo.toml`:

```toml
[features]
default = []
extended-streaming = [
    "aes", "ctr", "aes-gcm", "hmac", "md-5", "blake3",
    "zstd", "lz4_flex", "snap", "brotli", "regex", "data-encoding",
]
```

### 2.4 Implementation Priority

| Tier | Functions | Rationale |
|------|-----------|-----------|
| **High** (implement first) | #21–#22 (AES-CTR), #25 (HMAC), #28 (BLAKE3), #30–#31 (Hex), #41 (LineCount), #51–#54 (Zstd+LZ4), #61 (RateLimit), #65 (Tee), #71 (JsonValidate), #74 (SizeLimit), #82 (Grep), #86 (CAddrEmbed) | Most requested; enables secure, validated, multi-codec pipelines |
| **Medium** (implement second) | #23–#24 (AEAD), #26–#27 (MD5, SHA-512), #29 (Redact), #32–#33 (UTF-8, LineEnding), #34–#36 (JSON format), #42–#44 (WordCount, MinMax, Histogram), #55–#58 (Snappy, Brotli), #62–#63 (Delay, Timeout), #66 (Merge), #73 (MagicBytes), #75–#76 (Checksum/SHA verify), #78 (Replace), #83 (Sed), #87 (CAddrVerify), #90 (MerkleTree), #93–#94 (Sum, Average), #99 (Entropy) | Useful; moderate complexity |
| **Low** (implement last) | #37 (CsvToJson), #38–#39 (Base32), #45–#50 (Sample through Unique), #59–#60 (Pad, Trim), #64 (Retry), #67–#70 (Broadcast through Debounce), #72 (SchemaValidate), #77 (NonEmpty), #79–#81 (Prefix/Suffix/LinePrefix), #84–#85 (TruncateLines, CharsetConvert), #88–#89 (Diff, Patch), #91–#92 (ContentType, ChunkHash), #95–#98 (Bitwise, ByteSwap), #100 (RollingHash) | Niche or high complexity |

### 2.5 Function Numbering and Registration

All 80 new functions are numbered #21–#100 continuing from the existing
#1–#20. Each function is registered in the `FunctionRegistry` with a
snake_case name matching the struct name:

```rust
// Registration pattern (in function_registry.rs or equivalent):
registry.register_streaming("streaming_encrypt",      Box::new(StreamingEncrypt));
registry.register_streaming("streaming_decrypt",      Box::new(StreamingDecrypt));
registry.register_streaming("streaming_aead_encrypt", Box::new(StreamingAeadEncrypt));
// ... 77 more
```

Functions gated behind `extended-streaming` feature use conditional
compilation:

```rust
#[cfg(feature = "extended-streaming")]
registry.register_streaming("streaming_encrypt", Box::new(StreamingEncrypt));
```

### 2.6 Fusibility Annotations

From §2.11 Pipeline Fusion, functions that are pure stateless maps
can be fused into a single task. The following new functions qualify:

| Function | # | Why Fusible |
|----------|---|-------------|
| StreamingHexEncode | #30 | Pure byte → 2-hex-char map |
| StreamingHexDecode | #31 | Pure hex → byte map |
| StreamingBase32Encode | #38 | Pure byte → base32 map |
| StreamingBase32Decode | #39 | Pure base32 → byte map |
| StreamingBitwiseAnd | #95 | Pure byte & mask map |
| StreamingBitwiseOr | #96 | Pure byte | mask map |
| StreamingBitwiseNot | #97 | Pure byte complement map |
| StreamingPad | #59 | Pure chunk → padded chunk map |
| StreamingTrim | #60 | Pure chunk → trimmed chunk map |
| StreamingLineEnding | #33 | Pure \r\n ↔ \n map |
| StreamingByteSwap | #98 | Pure word-swap map |
| StreamingChunkHash | #92 | Pure chunk → chunk+hash map |

12 of 80 new functions are fusible, bringing the total to 21 of 100
(9 existing + 12 new).

### 2.7 Error Handling Strategy

All functions follow the same error contract as existing functions:

```
On upstream Error → propagate Error downstream, terminate task
On internal error → emit Error(DerivaError::ComputeFailed(msg)), terminate
On downstream closed → terminate silently (consumer dropped)
```

Category-specific error behaviors:

| Category | Error Condition | Behavior |
|----------|----------------|----------|
| Crypto (#21–#24) | Invalid key length, invalid nonce | `ComputeFailed("aes: invalid key length 15, expected 32")` |
| Crypto (#25–#28) | — | Accumulators cannot fail mid-stream |
| Encoding (#30–#31) | Invalid hex character | `ComputeFailed("hex decode: invalid char 'G' at position 5")` |
| Encoding (#32) | Invalid UTF-8 | `ComputeFailed("utf8: invalid byte 0xfe at position 42")` |
| Compression (#51–#58) | Corrupt compressed data | `ComputeFailed("zstd decompress: frame header error")` |
| Validation (#71–#72) | Invalid JSON/schema | `ComputeFailed("json: expected ',' at line 3 col 5")` |
| Validation (#74) | Size exceeded | `ComputeFailed("size limit: 10485760 bytes exceeded")` |
| Flow (#63) | Timeout | `ComputeFailed("timeout: no chunk within 5000ms")` |
| Flow (#64) | Retries exhausted | `ComputeFailed("retry: 3 attempts failed")` |
| Text (#85) | Unknown charset | `ComputeFailed("charset: unknown encoding 'cp437'")` |

### 2.8 Interaction with Other Phase 2 Features

| Feature | Interaction |
|---------|------------|
| §2.7 Streaming Materialization | All 80 functions implement `StreamingComputeFunction` and plug into `StreamPipeline` |
| §2.9 Size-Aware Mode Selection | Large-value mode uses streaming functions; batch mode uses batch equivalents from §2.15 |
| §2.10 Adaptive Chunk Sizing | Chunk size affects per-chunk crypto overhead (smaller chunks = more AES blocks, more GCM tags) |
| §2.11 Pipeline Fusion | 12 new fusible functions; fusion optimizer recognizes `is_fusible()` on new functions |
| §2.12 Memory Budget Enforcement | Buffered functions (#34–#36, #49–#50, #71–#72) must acquire budget permits proportional to input size |
| §2.13 Streaming-Aware Caching | Cached values can be read as streams; new functions consume cached streams transparently |

---

## 3. Function Specifications

### 3.1 Cryptography & Security (9 functions)

#### 3.1.1 StreamingEncrypt (#21)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` with captured cipher state |
| Algorithm | AES-256-CTR |
| Params | `key` (64 hex chars = 32 bytes), `nonce` (32 hex chars = 16 bytes) |
| Memory | O(chunk_size) — encrypts in-place |
| Complexity | Medium |
| Fusible | No (stateful — CTR counter increments across chunks) |

CTR mode is naturally streaming: the counter increments per 16-byte
block. State (current counter position) is captured in a closure and
mutated across chunks. Each chunk produces an equal-length ciphertext.

```rust
// Core logic per chunk:
let mut buf = chunk.to_vec();
cipher.apply_keystream(&mut buf);  // aes::Aes256Ctr
Bytes::from(buf)
```

Error: `ComputeFailed` if key/nonce hex is invalid or wrong length.

#### 3.1.2 StreamingDecrypt (#22)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` with captured cipher state |
| Algorithm | AES-256-CTR (symmetric — decrypt = encrypt) |
| Params | `key` (64 hex chars), `nonce` (32 hex chars) |
| Memory | O(chunk_size) |
| Complexity | Medium |
| Fusible | No (stateful) |

Identical implementation to #21 — CTR mode is symmetric. Separate
struct for semantic clarity in pipeline definitions.

#### 3.1.3 StreamingAeadEncrypt (#23)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` (per-chunk encrypt + tag) |
| Algorithm | AES-256-GCM |
| Params | `key` (64 hex chars), `nonce_prefix` (24 hex chars = 12 bytes base) |
| Output | Each chunk → ciphertext + 16-byte GCM tag appended |
| Memory | O(chunk_size + 16) |
| Complexity | Medium |
| Fusible | No (stateful — nonce counter) |

Per-chunk nonce = `nonce_prefix || chunk_index_u32`. Each output chunk
is 16 bytes larger than input (GCM tag appended). Chunk index tracked
as state.

#### 3.1.4 StreamingAeadDecrypt (#24)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` (per-chunk verify + decrypt) |
| Algorithm | AES-256-GCM |
| Params | `key` (64 hex chars), `nonce_prefix` (24 hex chars) |
| Input | Each chunk = ciphertext + 16-byte GCM tag |
| Memory | O(chunk_size) |
| Complexity | Medium |
| Fusible | No (stateful) |

Strips trailing 16 bytes as tag, decrypts remainder, verifies tag.
On tag mismatch: `ComputeFailed("aead: authentication failed at chunk N")`.

#### 3.1.5 StreamingHmacSha256 (#25)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| Algorithm | HMAC-SHA256 |
| Params | `key` (hex-encoded) |
| Output | 32 bytes on End |
| Memory | O(1) — incremental MAC state |
| Complexity | Low |
| Fusible | No (accumulator) |

```rust
spawn_accumulate(rx, HmacSha256::new_from_slice(&key)?,
    |mac, b| mac.update(b),
    |mac| Bytes::copy_from_slice(&mac.finalize().into_bytes()))
```

#### 3.1.6 StreamingMd5 (#26)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| Algorithm | MD5 |
| Output | 16 bytes on End |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (accumulator) |

Required for S3 ETag compatibility. Uses `md-5` crate (RustCrypto).

#### 3.1.7 StreamingSha512 (#27)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| Algorithm | SHA-512 |
| Output | 64 bytes on End |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (accumulator) |

Uses existing `sha2` crate (`Sha512` instead of `Sha256`).

#### 3.1.8 StreamingBlake3 (#28)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| Algorithm | BLAKE3 |
| Output | 32 bytes on End |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (accumulator) |

~10× faster than SHA-256 due to SIMD and tree hashing. Uses `blake3`
crate with default features (SIMD auto-detected).

#### 3.1.9 StreamingRedact (#29)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_boundary_map` |
| Algorithm | Regex replacement |
| Params | `patterns` (comma-separated regex list) |
| Output | Chunk with matches replaced by `[REDACTED]` |
| Memory | O(chunk_size + compiled regex) |
| Complexity | Medium |
| Fusible | No (boundary-aware stateful) |

Compiles regexes once at construction. Uses boundary-aware map to
handle patterns split across chunk boundaries. Default patterns
include email, phone, SSN if `patterns` param is `"default"`.

### 3.2 Encoding & Format Conversion (10 functions)

#### 3.2.1 StreamingHexEncode (#30)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Output | Each byte → 2 lowercase hex ASCII chars; output 2× input size |
| Memory | O(chunk_size × 2) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

```rust
spawn_map(rx, cap, |b| {
    Ok(Bytes::from(hex::encode(b)))
})
```

#### 3.2.2 StreamingHexDecode (#31)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Input | Hex ASCII chars (2 per output byte) |
| Memory | O(chunk_size / 2) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

Error on odd-length chunk or invalid hex char. Chunks must contain
even number of hex characters (enforced by upstream ChunkResizer or
caller).

#### 3.2.3 StreamingUtf8Validate (#32)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_boundary_map` |
| Behavior | Pass-through; validates UTF-8; buffers up to 3 trailing bytes for split multi-byte chars |
| Memory | O(chunk_size + 3) |
| Complexity | Medium |
| Fusible | No (boundary state) |

Multi-byte UTF-8 sequences (2–4 bytes) may be split across chunk
boundaries. The function buffers trailing incomplete sequences and
prepends them to the next chunk. Error on invalid byte sequences.

#### 3.2.4 StreamingLineEnding (#33)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Params | `target`: `"lf"` (default) or `"crlf"` |
| Behavior | `\r\n` → `\n` (lf mode) or `\n` → `\r\n` (crlf mode) |
| Memory | O(chunk_size × 2) worst case (all `\n` → `\r\n`) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

#### 3.2.5 StreamingJsonPrettyPrint (#34)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_buffered` |
| Behavior | Buffer full JSON input, parse, re-emit pretty-printed |
| Memory | O(input_size) |
| Complexity | Medium |
| Fusible | No (buffered) |

Uses `serde_json::from_slice` → `serde_json::to_vec_pretty`. Error
on invalid JSON.

#### 3.2.6 StreamingJsonMinify (#35)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_buffered` |
| Behavior | Buffer full JSON, strip whitespace |
| Memory | O(input_size) |
| Complexity | Medium |
| Fusible | No (buffered) |

Uses `serde_json::from_slice` → `serde_json::to_vec` (compact).

#### 3.2.7 StreamingJsonLines (#36)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_buffered` |
| Behavior | Buffer full JSON array, emit one JSON object per line (NDJSON) |
| Memory | O(input_size) |
| Complexity | Medium |
| Fusible | No (buffered) |

Input must be a JSON array. Each element is serialized as a single
line terminated by `\n`.

#### 3.2.8 StreamingCsvToJson (#37)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_boundary_map` (line-oriented) |
| Behavior | Parse CSV header from first line; convert each subsequent line to JSON object |
| State | Header row stored after first chunk |
| Memory | O(chunk_size + header) |
| Complexity | High |
| Fusible | No (stateful) |

First chunk (or first line of first chunk) is parsed as CSV header.
Subsequent lines become `{"col1":"val1","col2":"val2",...}\n`.

#### 3.2.9 StreamingBase32Encode (#38)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Output | Base32-encoded ASCII; output ~1.6× input |
| Memory | O(chunk_size × 1.6) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

Uses `data-encoding::BASE32` encoder.

#### 3.2.10 StreamingBase32Decode (#39)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Memory | O(chunk_size) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

Error on invalid Base32 input.

### 3.3 Data Processing & Analytics (11 functions)

#### 3.3.1 StreamingFilter (#40)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` with `PassAction::Forward` / `PassAction::Drop` |
| Params | `predicate`: `"non_empty"` (drop zero-length), `"contains:PATTERN"`, `"min_size:N"` |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (conditional drop) |

#### 3.3.2 StreamingLineCount (#41)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| Behavior | Count `\n` bytes across all chunks |
| Output | u64 as 8-byte big-endian on End |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (accumulator) |

```rust
spawn_accumulate(rx, 0u64,
    |count, b| *count += bytecount::count(b, b'\n') as u64,
    |count| Bytes::copy_from_slice(&count.to_be_bytes()))
```

#### 3.3.3 StreamingWordCount (#42)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| State | `(u64, bool)` — (word_count, in_word) |
| Behavior | Count whitespace-delimited words; tracks whether previous chunk ended mid-word |
| Output | u64 as 8-byte big-endian on End |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (accumulator) |

#### 3.3.4 StreamingMinMax (#43)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| State | `(u8, u8)` — (min, max) initialized to (255, 0) |
| Output | 2 bytes `[min, max]` on End |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (accumulator) |

#### 3.3.5 StreamingHistogram (#44)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| State | `[u64; 256]` — byte frequency counts |
| Output | 2,048 bytes (256 × u64 big-endian) on End |
| Memory | O(1) — 2 KB fixed state |
| Complexity | Low |
| Fusible | No (accumulator) |

#### 3.3.6 StreamingSample (#45)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` |
| Params | `rate`: emit every Nth chunk (default 10) |
| State | chunk counter |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (stateful) |

Emits chunk when `counter % rate == 0`. Useful for preview/sampling
large streams.

#### 3.3.7 StreamingHead (#46)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` |
| Params | `chunks`: number of chunks to emit (default 1) |
| Behavior | Forward first N chunks, then send End, drop remaining |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (stateful) |

Unlike `StreamingTake` (#17) which counts bytes, this counts chunks.

#### 3.3.8 StreamingTail (#47)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom task — ring buffer |
| Params | `chunks`: number of trailing chunks to keep (default 1) |
| State | `VecDeque<Bytes>` of capacity N |
| Memory | O(N × chunk_size) |
| Complexity | Medium |
| Fusible | No (buffered) |

Buffers last N chunks in a ring buffer. On End, emits all buffered
chunks in order.

#### 3.3.9 StreamingDeduplicate (#48)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` with hash set |
| State | `HashSet<u64>` — xxhash of each seen chunk |
| Behavior | Drop chunks whose hash is already in the set |
| Memory | O(unique_chunks × 8 bytes) |
| Complexity | Medium |
| Fusible | No (stateful) |

Uses xxhash64 for fast hashing. Collision probability negligible for
typical stream sizes.

#### 3.3.10 StreamingSort (#49)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_buffered` |
| Behavior | Collect all chunks, sort by byte order, re-emit |
| Memory | O(input_size) |
| Complexity | Medium |
| Fusible | No (buffered) |

Sorts chunks as `&[u8]` using `sort_unstable()`. Useful for
deterministic output ordering.

#### 3.3.11 StreamingUnique (#50)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_buffered` |
| Behavior | Collect all, deduplicate by content, sort, re-emit |
| Memory | O(input_size) |
| Complexity | Medium |
| Fusible | No (buffered) |

Combines sort + dedup. Emits unique chunks in sorted order.

### 3.4 Compression & Transformation (10 functions)

#### 3.4.1 StreamingZstdCompress (#51)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Algorithm | Zstandard (level 3 default) |
| Params | `level`: 1–22 (default 3) |
| Output | Each chunk independently compressed |
| Memory | O(chunk_size + zstd context ~128 KB) |
| Complexity | Medium |
| Fusible | No (compression context state) |

Per-chunk compression — each chunk is a standalone zstd frame.
Decompressor does not need cross-chunk state. Level 3 gives ~3×
compression at ~500 MB/s.

#### 3.4.2 StreamingZstdDecompress (#52)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Memory | O(chunk_size × decompression_ratio) |
| Complexity | Medium |
| Fusible | No |

Each input chunk is a standalone zstd frame. Error on corrupt frame.

#### 3.4.3 StreamingLz4Compress (#53)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Algorithm | LZ4 block compression |
| Output | Per-chunk compressed blocks |
| Memory | O(chunk_size) |
| Complexity | Low |
| Fusible | No |

Fastest compression (~2 GB/s). Uses `lz4_flex::compress_prepend_size`.

#### 3.4.4 StreamingLz4Decompress (#54)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Memory | O(chunk_size × ratio) |
| Complexity | Low |
| Fusible | No |

Uses `lz4_flex::decompress_size_prepended`.

#### 3.4.5 StreamingSnappyCompress (#55)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Algorithm | Snappy |
| Memory | O(chunk_size) |
| Complexity | Low |
| Fusible | No |

~1.5 GB/s compression. Uses `snap::raw::Encoder`.

#### 3.4.6 StreamingSnappyDecompress (#56)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Memory | O(chunk_size × ratio) |
| Complexity | Low |
| Fusible | No |

Uses `snap::raw::Decoder`.

#### 3.4.7 StreamingBrotliCompress (#57)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Algorithm | Brotli (quality 4 default) |
| Params | `quality`: 0–11 (default 4) |
| Memory | O(chunk_size + brotli context ~1 MB) |
| Complexity | Medium |
| Fusible | No |

Best compression ratio for text (~50 MB/s at quality 4). Higher
quality = better ratio but slower.

#### 3.4.8 StreamingBrotliDecompress (#58)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Memory | O(chunk_size × ratio) |
| Complexity | Medium |
| Fusible | No |

#### 3.4.9 StreamingPad (#59)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Params | `block_size` (default 16), `padding_byte` (default `0x00`) |
| Behavior | Pad each chunk to next multiple of block_size |
| Memory | O(chunk_size + block_size) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

Useful before block ciphers that require aligned input.

#### 3.4.10 StreamingTrim (#60)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Behavior | Strip leading and trailing ASCII whitespace from each chunk |
| Memory | O(chunk_size) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

Note: trims per-chunk, not per-stream. For stream-level trim, use
Skip + Take.

### 3.5 Flow Control & Pipeline Utilities (10 functions)

#### 3.5.1 StreamingRateLimit (#61)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` with timer |
| Params | `bytes_per_sec` (default 1048576 = 1 MB/s) |
| Behavior | Forward chunks with `tokio::time::sleep` between sends to throttle throughput |
| State | Cumulative bytes sent, elapsed time |
| Memory | O(1) |
| Complexity | Medium |
| Fusible | No (timer state) |

Sleep duration = `chunk.len() / bytes_per_sec` seconds. Useful for
testing backpressure and simulating slow consumers.

#### 3.5.2 StreamingDelay (#62)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` with fixed sleep |
| Params | `delay_ms` (default 100) |
| Behavior | Sleep `delay_ms` before forwarding each chunk |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (timer) |

For testing pipeline behavior under latency.

#### 3.5.3 StreamingTimeout (#63)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom task with `tokio::time::timeout` |
| Params | `timeout_ms` (default 5000) |
| Behavior | If no chunk received within deadline, emit Error |
| Memory | O(1) |
| Complexity | Medium |
| Fusible | No (timer) |

```rust
match tokio::time::timeout(Duration::from_millis(ms), rx.recv()).await {
    Ok(Some(chunk)) => /* forward */,
    Err(_) => /* emit ComputeFailed("timeout") */,
}
```

#### 3.5.4 StreamingRetry (#64)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom wrapper task |
| Params | `max_retries` (default 3) |
| Behavior | On upstream Error, restart upstream function up to N times |
| Memory | O(1) |
| Complexity | High |
| Fusible | No |

Requires access to the upstream function factory to re-create the
input stream. This is a pipeline-level concern — the retry wrapper
re-invokes the upstream `stream_execute()`.

#### 3.5.5 StreamingTee (#65)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom fan-out task |
| Params | `outputs` (default 2) |
| Behavior | Duplicate each input chunk to N output receivers |
| Output | Returns first receiver; additional receivers stored in shared state |
| Memory | O(chunk_size × N) per chunk in flight |
| Complexity | High |
| Fusible | No (fan-out) |

Each output receiver gets a `clone()` of the `Bytes` chunk (cheap —
reference-counted). Backpressure: producer blocks if any output
channel is full.

#### 3.5.6 StreamingMerge (#66)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom multi-recv with `tokio::select!` |
| Behavior | Emit whichever input has a chunk ready first; non-deterministic ordering |
| Memory | O(1) |
| Complexity | Medium |
| Fusible | No (multi-input) |

Ends when all inputs have sent End.

#### 3.5.7 StreamingBroadcast (#67)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom fan-out with backpressure |
| Behavior | Like Tee but slowest consumer gates the producer |
| Memory | O(chunk_size × N) |
| Complexity | High |
| Fusible | No (fan-out) |

Uses `tokio::sync::broadcast` channel. Slow consumers cause the
producer to block, preventing unbounded buffering.

#### 3.5.8 StreamingPartition (#68)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom fan-out (2 outputs) |
| Params | `predicate`: `"non_empty"`, `"contains:PATTERN"`, `"min_size:N"` |
| Behavior | Route each chunk to output A (predicate true) or output B (predicate false) |
| Memory | O(1) |
| Complexity | Medium |
| Fusible | No (fan-out) |

#### 3.5.9 StreamingBatch (#69)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom task with accumulation buffer |
| Params | `batch_size` (default 4 = collect 4 chunks into 1) |
| Behavior | Collect N chunks, concatenate into one large chunk, emit |
| Memory | O(N × chunk_size) |
| Complexity | Low |
| Fusible | No (stateful) |

Inverse of `StreamingChunkResizer` (#16). Useful for reducing channel
overhead when downstream prefers larger chunks.

#### 3.5.10 StreamingDebounce (#70)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom task with timer |
| Params | `window_ms` (default 100) |
| Behavior | After receiving a chunk, wait `window_ms` for more; emit only the last chunk in the window |
| Memory | O(chunk_size) — holds latest chunk |
| Complexity | Medium |
| Fusible | No (timer) |

Useful for suppressing rapid bursts of small chunks.

### 3.6 Validation & Integrity (7 functions)

#### 3.6.1 StreamingJsonValidate (#71)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_buffered` |
| Behavior | Buffer full input, parse as JSON, emit unchanged if valid |
| Memory | O(input_size) |
| Complexity | Medium |
| Fusible | No (buffered) |

Error: `ComputeFailed("json: expected ',' at line 3 col 5")`.
On success, emits the original bytes unchanged (not re-serialized).

#### 3.6.2 StreamingSchemaValidate (#72)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_buffered` |
| Params | `schema` (JSON Schema as JSON string) |
| Behavior | Buffer full input, parse JSON, validate against schema, emit unchanged |
| Memory | O(input_size + schema_size) |
| Complexity | High |
| Fusible | No (buffered) |

Uses `serde_json` for parsing + manual schema validation (subset:
type checks, required fields, enum values). Full JSON Schema draft-07
support deferred.

#### 3.6.3 StreamingMagicBytes (#73)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` (first-chunk only check) |
| Params | `expected` (hex-encoded magic bytes, e.g., `"89504e47"` for PNG) |
| Behavior | Check first chunk starts with expected bytes; Error on mismatch; pass all chunks through |
| State | `bool` — checked flag |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (stateful) |

#### 3.6.4 StreamingSizeLimit (#74)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` with byte counter |
| Params | `max_bytes` (required) |
| Behavior | Track cumulative bytes; Error if limit exceeded |
| State | `u64` running total |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (stateful) |

Error emitted immediately when the chunk that crosses the limit is
received — does not wait for End.

#### 3.6.5 StreamingChecksumVerify (#75)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| Params | `expected_crc32` (hex string, 8 chars) |
| Behavior | Compute CRC32 over all chunks; on End, compare with expected |
| Output | Original stream passed through; Error on mismatch emitted after End |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (accumulator) |

Note: this is a verify-only function. The original data is forwarded
as-is during accumulation (uses a tee pattern internally).

#### 3.6.6 StreamingSha256Verify (#76)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| Params | `expected_hash` (hex string, 64 chars) |
| Behavior | Compute SHA-256; on End, compare with expected |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (accumulator) |

Same tee pattern as #75 — data forwarded during accumulation.

#### 3.6.7 StreamingNonEmpty (#77)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` |
| State | `bool` — saw_data flag |
| Behavior | Pass all chunks through; on End, Error if no Data chunks were seen |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (stateful) |

### 3.7 Text Processing (8 functions)

#### 3.7.1 StreamingReplace (#78)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_boundary_map` |
| Params | `find` (byte pattern or regex), `replace` (replacement bytes) |
| Behavior | Replace all occurrences of `find` with `replace` per line |
| Memory | O(chunk_size + longest_line) |
| Complexity | Medium |
| Fusible | No (boundary-aware) |

Boundary-aware: buffers trailing partial line to handle patterns
split across chunk boundaries. Uses `regex` crate if `find` contains
regex metacharacters, otherwise byte-level `memmem` search.

#### 3.7.2 StreamingPrefix (#79)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` (first-chunk map) |
| Params | `prefix` (bytes to prepend, as UTF-8 string) |
| Behavior | Prepend `prefix` bytes before the first Data chunk only |
| State | `bool` — emitted flag |
| Memory | O(prefix.len()) |
| Complexity | Low |
| Fusible | No (stateful) |

#### 3.7.3 StreamingSuffix (#80)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` |
| Params | `suffix` (bytes to append, as UTF-8 string) |
| Behavior | On End, emit `suffix` as final Data chunk before End |
| Memory | O(suffix.len()) |
| Complexity | Low |
| Fusible | No (stateful) |

#### 3.7.4 StreamingLinePrefix (#81)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_boundary_map` |
| Params | `prefix` (string to prepend to each line) |
| Behavior | Prepend `prefix` after every `\n` and at start of stream |
| Memory | O(chunk_size + prefix.len() × lines_per_chunk) |
| Complexity | Medium |
| Fusible | No (boundary-aware) |

Useful for adding line numbers, timestamps, or indentation.

#### 3.7.5 StreamingGrep (#82)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_boundary_map` |
| Params | `pattern` (regex), `invert` (optional, `"true"` for grep -v) |
| Behavior | Keep only lines matching regex; drop non-matching lines |
| Memory | O(chunk_size + compiled regex) |
| Complexity | Medium |
| Fusible | No (boundary-aware) |

Line-oriented: splits on `\n`, tests each line, emits matching lines
joined with `\n`. Boundary-aware for lines split across chunks.

#### 3.7.6 StreamingSed (#83)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_boundary_map` |
| Params | `pattern` (regex), `replacement` (replacement string with `$1` capture groups) |
| Behavior | Regex find-and-replace per line |
| Memory | O(chunk_size + compiled regex) |
| Complexity | Medium |
| Fusible | No (boundary-aware) |

Uses `regex::Regex::replace_all` per line. Supports capture group
references in replacement string.

#### 3.7.7 StreamingTruncateLines (#84)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_boundary_map` |
| Params | `max_line_bytes` (default 1024) |
| Behavior | Truncate each line to max N bytes; preserve `\n` |
| Memory | O(chunk_size) |
| Complexity | Low |
| Fusible | No (boundary-aware) |

#### 3.7.8 StreamingCharsetConvert (#85)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_boundary_map` |
| Params | `from` (source encoding, e.g., `"latin1"`), `to` (target, default `"utf8"`) |
| Behavior | Convert between character encodings |
| Memory | O(chunk_size × 4) worst case (single-byte → 4-byte UTF-8) |
| Complexity | High |
| Fusible | No (boundary-aware) |

Supports: `latin1`/`iso-8859-1`, `utf8`, `utf16le`, `utf16be`,
`ascii`. Boundary-aware for multi-byte sequences split across chunks.
Error on unmappable characters.

### 3.8 CAS-Specific Operations (7 functions)

#### 3.8.1 StreamingCAddrEmbed (#86)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` + accumulate hybrid |
| Behavior | Forward all chunks while computing SHA-256; on End, emit 32-byte CAddr as final Data chunk before End |
| State | `Sha256` hasher |
| Memory | O(1) — streaming hash |
| Complexity | Medium |
| Fusible | No (stateful) |

The embedded CAddr allows downstream consumers to verify the content
address of the stream without re-reading it.

#### 3.8.2 StreamingCAddrVerify (#87)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` + accumulate hybrid |
| Params | `expected_caddr` (hex string, 64 chars) |
| Behavior | Forward all chunks while computing SHA-256; on End, compare with expected; Error on mismatch |
| Memory | O(1) |
| Complexity | Medium |
| Fusible | No (stateful) |

Same tee pattern as #75/#76 — data forwarded during accumulation.

#### 3.8.3 StreamingDiff (#88)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom multi-recv (2 inputs) |
| Behavior | Read chunks pairwise from 2 inputs; emit binary diff (XOR of corresponding bytes) |
| Memory | O(chunk_size) |
| Complexity | High |
| Fusible | No (multi-input) |

Requires both inputs to have same chunk boundaries (use ChunkResizer
upstream). Unequal-length inputs: shorter stream padded with zeros.

#### 3.8.4 StreamingPatch (#89)

| Attribute | Value |
|-----------|-------|
| Pattern | Custom multi-recv (2 inputs: base + diff) |
| Behavior | Apply XOR diff to base stream; inverse of #88 |
| Memory | O(chunk_size) |
| Complexity | High |
| Fusible | No (multi-input) |

`base_chunk XOR diff_chunk = patched_chunk`. Requires aligned chunks.

#### 3.8.5 StreamingMerkleTree (#90)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| Behavior | Build Merkle tree over chunk hashes; emit 32-byte root hash on End |
| State | `Vec<[u8; 32]>` — leaf hashes, then internal nodes |
| Memory | O(num_chunks × 32) for leaves; O(depth × 32) during finalization |
| Complexity | Medium |
| Fusible | No (accumulator) |

Each chunk is SHA-256 hashed to produce a leaf. Tree is built
bottom-up on End. Useful for content verification and incremental
updates.

#### 3.8.6 StreamingContentType (#91)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_passthrough` (first-chunk inspection) |
| Behavior | Detect MIME type from first chunk's magic bytes; prepend metadata chunk `Content-Type: <mime>\n` |
| State | `bool` — detected flag |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (stateful) |

Detection table (first bytes → MIME):
- `89 50 4E 47` → `image/png`
- `FF D8 FF` → `image/jpeg`
- `47 49 46 38` → `image/gif`
- `25 50 44 46` → `application/pdf`
- `50 4B 03 04` → `application/zip`
- `1F 8B` → `application/gzip`
- `7B` → `application/json` (heuristic)
- Default → `application/octet-stream`

#### 3.8.7 StreamingChunkHash (#92)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Behavior | Append 32-byte SHA-256 hash to each chunk |
| Output | Each chunk grows by 32 bytes |
| Memory | O(chunk_size + 32) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

Per-chunk integrity: each output chunk = `original_bytes || sha256(original_bytes)`.
Verifier can split last 32 bytes and check.

### 3.9 Numeric & Scientific (8 functions)

#### 3.9.1 StreamingSum (#93)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| Behavior | Parse newline-delimited decimal numbers, sum them |
| State | `f64` running total |
| Output | ASCII decimal string on End |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (accumulator) |

Skips blank lines and lines that fail to parse (with warning log).

#### 3.9.2 StreamingAverage (#94)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| State | `(f64, u64)` — (sum, count) |
| Output | ASCII decimal string (`sum / count`) on End |
| Memory | O(1) |
| Complexity | Low |
| Fusible | No (accumulator) |

Error if count is zero (no valid numbers found).

#### 3.9.3 StreamingBitwiseAnd (#95)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Params | `mask` (u8, default 0xFF) |
| Behavior | AND each byte with mask |
| Memory | O(chunk_size) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

```rust
spawn_map(rx, cap, move |b| {
    Ok(Bytes::from(b.iter().map(|byte| byte & mask).collect::<Vec<_>>()))
})
```

#### 3.9.4 StreamingBitwiseOr (#96)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Params | `mask` (u8, default 0x00) |
| Behavior | OR each byte with mask |
| Memory | O(chunk_size) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

#### 3.9.5 StreamingBitwiseNot (#97)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Behavior | Complement each byte (`!byte`) |
| Memory | O(chunk_size) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

#### 3.9.6 StreamingByteSwap (#98)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` |
| Params | `word_size`: 2, 4, or 8 (default 2) |
| Behavior | Swap byte order within each word |
| Memory | O(chunk_size) |
| Complexity | Low |
| Fusible | **Yes** — pure stateless map |

Chunk length must be divisible by `word_size`; Error otherwise.
Useful for endianness conversion of binary data.

#### 3.9.7 StreamingEntropy (#99)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_accumulate` |
| State | `[u64; 256]` byte frequency + `u64` total bytes |
| Output | 8-byte f64 (big-endian) Shannon entropy in bits on End |
| Memory | O(1) — 2 KB fixed state |
| Complexity | Low |
| Fusible | No (accumulator) |

Shannon entropy: `H = -Σ p(x) × log₂(p(x))` where `p(x) = count[x] / total`.
Range: 0.0 (all same byte) to 8.0 (uniform distribution).
Useful for detecting compressed/encrypted data (entropy > 7.5).

#### 3.9.8 StreamingRollingHash (#100)

| Attribute | Value |
|-----------|-------|
| Pattern | `spawn_map` with captured state |
| Params | `window_size` (default 48) |
| Behavior | Compute Rabin fingerprint over sliding window; append 8-byte hash to each chunk |
| State | Rolling hash state (window buffer) |
| Memory | O(window_size + chunk_size) |
| Complexity | Medium |
| Fusible | No (stateful — window carries across chunks) |

Rabin fingerprint enables content-defined chunking: when the hash
modulo a threshold equals zero, a chunk boundary is detected. This
function computes and appends the hash; boundary detection is a
downstream concern.

---

## 4. Implementation Strategy

### 4.1 Helper Reuse

Function-to-helper mapping across all 100 functions (20 existing + 80 new):

```
spawn_map() — 44 functions (9 existing + 35 new)
  Existing: #1–#9 (Identity, Uppercase, Lowercase, Reverse, Base64Enc/Dec, Xor, Compress, Decompress)
  New:      #21–#22 (AES-CTR), #30–#31 (Hex), #33 (LineEnding), #38–#39 (Base32),
            #51–#60 (Zstd/LZ4/Snappy/Brotli + Pad/Trim), #59–#60,
            #92 (ChunkHash), #95–#98 (Bitwise + ByteSwap)

spawn_accumulate() — 18 functions (3 existing + 15 new)
  Existing: #10–#12 (Sha256, ByteCount, Checksum)
  New:      #25–#28 (HMAC, MD5, SHA-512, BLAKE3), #41–#44 (LineCount, WordCount, MinMax, Histogram),
            #75–#76 (ChecksumVerify, Sha256Verify), #87 (CAddrVerify),
            #90 (MerkleTree), #93–#94 (Sum, Average), #99 (Entropy)

spawn_boundary_map() [NEW] — 9 functions
  #29 (Redact), #32 (Utf8Validate), #37 (CsvToJson),
  #78 (Replace), #81 (LinePrefix), #82 (Grep), #83 (Sed),
  #84 (TruncateLines), #85 (CharsetConvert)

spawn_buffered() [NEW] — 6 functions
  #34–#36 (JsonPrettyPrint, JsonMinify, JsonLines),
  #49–#50 (Sort, Unique), #71–#72 (JsonValidate, SchemaValidate)

spawn_passthrough() [NEW] — 14 functions
  #40 (Filter), #45–#46 (Sample, Head), #61–#62 (RateLimit, Delay),
  #73–#74 (MagicBytes, SizeLimit), #77 (NonEmpty),
  #79–#80 (Prefix, Suffix), #86 (CAddrEmbed), #91 (ContentType)

Custom task spawning — 9 functions
  Existing: #13–#15 (Concat, Interleave, ZipConcat), #16–#20 (ChunkResizer, Take, Skip, Repeat, TeeCount)
  New:      #23–#24 (AEAD), #47 (Tail), #48 (Deduplicate), #63–#70 (Timeout, Retry, Tee, Merge,
            Broadcast, Partition, Batch, Debounce), #88–#89 (Diff, Patch), #100 (RollingHash)
```

Helper coverage: 91 of 100 functions use one of the 5 standard helpers.
Only 9 require custom task spawning (mostly flow control fan-out and
multi-input CAS operations).

### 4.2 Boundary-Aware Processing

The `spawn_boundary_map` helper handles text functions that operate on
line boundaries where patterns may be split across chunks:

```
Chunk N:          ...some text ERROR: disk\n
Chunk N+1:        full at /dev/sda\nmore text...

Problem: grep("ERROR: disk full") must match across the boundary.
```

Strategy:

```
┌─────────────────────────────────────────────────────────────┐
│ spawn_boundary_map(rx, cap, line_fn)                        │
│                                                             │
│   leftover: BytesMut = empty                                │
│                                                             │
│   on Data(chunk):                                           │
│     buf = leftover + chunk                                  │
│     find last \n position in buf                            │
│       if found at pos:                                      │
│         complete_lines = buf[..pos+1]                        │
│         leftover = buf[pos+1..]                              │
│         emit line_fn(complete_lines)                         │
│       else:                                                 │
│         leftover = buf  (no complete line yet)               │
│                                                             │
│   on End:                                                   │
│     if leftover not empty:                                  │
│       emit line_fn(leftover)  (final partial line)           │
│     emit End                                                │
└─────────────────────────────────────────────────────────────┘
```

Memory: O(longest_line) for the leftover buffer. For typical log files
(lines < 1 KB), this is negligible. For pathological inputs (no `\n`
in entire stream), the leftover grows to full input size — same as
`spawn_buffered`.

Functions using this helper: #29, #32, #37, #78, #81–#85 (9 total).

### 4.3 Fusibility Annotations

From §2.11, fusible functions must implement `is_fusible() -> bool`
returning `true`. The fusion optimizer groups consecutive fusible
stages into a single `FusedMapStage`.

Complete fusibility table (21 of 100):

| # | Function | Fusible | Reason |
|---|----------|---------|--------|
| 1 | Identity | ✅ | No-op |
| 2 | Uppercase | ✅ | Pure byte map |
| 3 | Lowercase | ✅ | Pure byte map |
| 4 | Reverse | ✅ | Pure byte map |
| 5 | Base64Encode | ✅ | Pure byte map |
| 6 | Base64Decode | ✅ | Pure byte map |
| 7 | Xor | ✅ | Pure byte map (key captured) |
| 8 | Compress | ✅ | Stateless per-chunk |
| 9 | Decompress | ✅ | Stateless per-chunk |
| 30 | HexEncode | ✅ | Pure byte map |
| 31 | HexDecode | ✅ | Pure byte map |
| 33 | LineEnding | ✅ | Pure byte map |
| 38 | Base32Encode | ✅ | Pure byte map |
| 39 | Base32Decode | ✅ | Pure byte map |
| 59 | Pad | ✅ | Pure byte map |
| 60 | Trim | ✅ | Pure byte map |
| 92 | ChunkHash | ✅ | Pure byte map |
| 95 | BitwiseAnd | ✅ | Pure byte map |
| 96 | BitwiseOr | ✅ | Pure byte map |
| 97 | BitwiseNot | ✅ | Pure byte map |
| 98 | ByteSwap | ✅ | Pure byte map |

Non-fusible reasons:
- Accumulators (#10–#12, #25–#28, #41–#44, etc.) — consume all before emitting
- Boundary-aware (#29, #32, #37, #78, #81–#85) — carry leftover state
- Stateful (#21–#24, #45–#48, #61–#70, etc.) — counter/timer/buffer state
- Multi-input (#13–#15, #66, #88–#89) — fan-in semantics
- Buffered (#34–#36, #49–#50, #71–#72) — collect-all pattern

### 4.4 File Organization

The 80 new functions are split across category-specific modules to
keep file sizes manageable:

```
crates/deriva-compute/src/
├── builtins_streaming.rs              # Existing 20 functions (unchanged)
├── builtins_streaming_crypto.rs       # #21–#29  (9 functions, ~350 lines)
├── builtins_streaming_encoding.rs     # #30–#39  (10 functions, ~300 lines)
├── builtins_streaming_analytics.rs    # #40–#50  (11 functions, ~400 lines)
├── builtins_streaming_compression.rs  # #51–#60  (10 functions, ~300 lines)
├── builtins_streaming_flow.rs         # #61–#70  (10 functions, ~500 lines)
├── builtins_streaming_validation.rs   # #71–#77  (7 functions, ~250 lines)
├── builtins_streaming_text.rs         # #78–#85  (8 functions, ~350 lines)
├── builtins_streaming_cas.rs          # #86–#92  (7 functions, ~300 lines)
├── builtins_streaming_numeric.rs      # #93–#100 (8 functions, ~250 lines)
└── streaming_helpers.rs               # spawn_boundary_map, spawn_buffered,
                                       # spawn_passthrough, PassAction (~150 lines)
```

Total: 11 new files, ~3,150 lines. Each category file imports helpers
from `streaming_helpers.rs` and the existing `builtins_streaming.rs`.

### 4.5 Registration Strategy

All functions are registered in a single `register_streaming_builtins()`
function, gated by feature flag:

```rust
// In function_registry.rs or equivalent:

pub fn register_streaming_builtins(registry: &mut FunctionRegistry) {
    // Existing 20 (always available)
    registry.register_streaming("streaming_identity", Box::new(StreamingIdentity));
    // ... #1–#20

    // Extended library (feature-gated)
    #[cfg(feature = "extended-streaming")]
    {
        // Crypto (#21–#29)
        registry.register_streaming("streaming_encrypt", Box::new(StreamingEncrypt));
        // ... all 80 new functions
    }
}
```

### 4.6 Implementation Order

Following the priority tiers from §2.4, implementation proceeds in
3 phases within the sprint:

```
Days 1–2: High priority (20 functions)
  Helpers: spawn_boundary_map, spawn_buffered, spawn_passthrough
  Crypto:  #21–#22 (AES-CTR), #25 (HMAC), #28 (BLAKE3)
  Encoding: #30–#31 (Hex)
  Analytics: #41 (LineCount)
  Compression: #51–#54 (Zstd, LZ4)
  Flow: #61 (RateLimit), #65 (Tee)
  Validation: #71 (JsonValidate), #74 (SizeLimit)
  Text: #82 (Grep)
  CAS: #86 (CAddrEmbed)
  Tests: ~60 unit tests for high-priority functions

Days 3–4: Medium priority (35 functions)
  Crypto: #23–#24 (AEAD), #26–#27 (MD5, SHA-512), #29 (Redact)
  Encoding: #32–#33, #34–#36
  Analytics: #42–#44
  Compression: #55–#58 (Snappy, Brotli)
  Flow: #62–#63, #66
  Validation: #73, #75–#76
  Text: #78, #83
  CAS: #87, #90
  Numeric: #93–#94, #99
  Tests: ~105 unit tests + 8 roundtrip tests

Days 5–7: Low priority (25 functions) + integration + benchmarks
  Remaining: #37–#39, #45–#50, #59–#60, #64, #67–#70, #72, #77,
             #79–#81, #84–#85, #88–#89, #91–#92, #95–#98, #100
  Tests: ~75 unit tests + 6 boundary tests + 5 integration tests
  Benchmarks: compression codec + hash algorithm comparison
```

---

## 5. Test Specification

Total: 200 tests (T1–T200) covering all 100 streaming functions (#1–#100).
Every function has at least 1 dedicated test; critical functions have 2–4.
Tests are non-trivial and reflect real-world use-case scenarios including
multi-chunk inputs, boundary conditions, error paths, roundtrip pairs,
cross-function pipelines, and concurrency.

### 5.0 Existing Functions #1–#20 (T1–T20)

**T1 — Identity preserves 100 MB stream byte-for-byte (#1)**
Feed 1,536 × 64 KB chunks. Assert every output chunk equals its input.
Assert chunk count and total bytes preserved. Validates zero-copy path.

**T2 — Uppercase/Lowercase roundtrip on mixed ASCII+binary (#2, #3)**
Feed chunks containing ASCII letters + non-ASCII bytes (0x80–0xFF).
`Uppercase → Lowercase`. Assert ASCII letters are lowercased. Assert
non-ASCII bytes unchanged (ASCII-only transform).

**T3 — Reverse applied twice is identity (#4)**
Feed 10 chunks. `Reverse → Reverse`. Assert output equals input.

**T4 — Base64Encode/Decode roundtrip with binary data (#5, #6)**
Feed 256 KB random binary across 4 chunks. `Encode → Decode`. Assert
byte-identical. Assert encoded output is valid Base64 (only `[A-Za-z0-9+/=]`).

**T5 — Base64Decode rejects invalid input (#6)**
Feed `"!!!not-base64!!!"`. Assert Error containing "base64 decode".

**T6 — XorCipher applied twice with same key is identity (#7)**
Feed 5 chunks with `key=0xAB`. `Xor(0xAB) → Xor(0xAB)`. Assert
output equals input (XOR is self-inverse).

**T7 — Compress/Decompress roundtrip on 1 MB text (#8, #9)**
Feed 1 MB English text across 16 chunks. `Compress → Decompress`.
Assert byte-identical. Assert compressed total < input total.

**T8 — Decompress rejects non-zlib data (#9)**
Feed random bytes. Assert Error containing "decompress".

**T9 — SHA-256 matches known NIST test vector (#10)**
Feed NIST FIPS 180-4 test message `"abc"` as single chunk. Assert
output = `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

**T10 — SHA-256 on multi-chunk input matches single-pass (#10)**
Feed 1 MB as 16 chunks and as 1 chunk. Assert both produce identical
32-byte digest. Validates incremental hashing correctness.

**T11 — ByteCount on 10 MB stream (#11)**
Feed 160 × 64 KB chunks (= 10,485,760 bytes). Assert output = u64
big-endian of 10485760.

**T12 — Checksum CRC32 matches crc32fast reference (#12)**
Feed known input across 5 chunks. Compute CRC32 independently with
`crc32fast`. Assert outputs match.

**T13 — Concat reads 3 inputs sequentially (#13)**
3 inputs: A sends 3 chunks, B sends 2, C sends 4. Assert output has
9 chunks in order: A's 3, then B's 2, then C's 4.

**T14 — Interleave round-robins from 3 inputs (#14)**
3 inputs each send 4 chunks labeled A1–A4, B1–B4, C1–C4. Assert
output order: A1, B1, C1, A2, B2, C2, A3, B3, C3, A4, B4, C4.

**T15 — ZipConcat pairs chunks from 2 inputs (#15)**
Input A: chunks ["aa", "bb"]. Input B: chunks ["11", "22"]. Assert
output: ["aa11", "bb22"].

**T16 — ChunkResizer re-chunks to target size (#16)**
Feed 3 chunks of sizes [100, 50, 150] with `target_size=80`. Assert
output chunks are [80, 70, 80, 70] (re-chunked). Assert total bytes
= 300.

**T17 — Take(100) on 1 MB stream emits exactly 100 bytes (#17)**
Feed 16 × 64 KB chunks with `bytes=100`. Assert output total = 100
bytes. Assert End received after truncation.

**T18 — Skip(1000) on 2 KB stream skips first 1000 bytes (#18)**
Feed 2048 bytes across 4 chunks with `bytes=1000`. Assert output
total = 1048 bytes. Assert first output byte matches input byte 1000.

**T19 — Repeat(3) on 64 KB input produces 192 KB (#19)**
Feed 1 chunk of 64 KB with `count=3`. Assert output has 3 chunks,
each identical to input. Assert total = 192 KB.

**T20 — TeeCount appends correct byte count as ASCII (#20)**
Feed 3 chunks totaling 12,345 bytes. Assert output has 4 chunks:
original 3 + `"12345"` as ASCII. Assert End follows.

### 5.1 Cryptography & Security (T21–T40)

**T21 — AES-CTR encrypt/decrypt roundtrip across 5 chunks (#21, #22)**
Feed 5 × 64 KB chunks through `Encrypt(key, nonce) → Decrypt(key, nonce)`.
Assert output equals input byte-for-byte. Validates CTR counter state
maintained correctly across chunk boundaries.

**T22 — AES-CTR ciphertext differs from plaintext (#21)**
Encrypt 3 chunks of known content. Assert every output chunk differs
from its input. Assert output chunks differ from each other (CTR
produces unique keystream per block position).

**T23 — AES-CTR with wrong key produces wrong plaintext (#21, #22)**
Encrypt with key A, decrypt with key B. Assert decrypted ≠ original.
No Error — CTR decryption always "succeeds" but produces garbage.

**T24 — Encrypt with invalid key length emits Error (#21)**
Pass 15-byte hex key (not 32). Assert
`Error("aes: invalid key length 15, expected 32")`.

**T25 — Encrypt with invalid nonce emits Error (#21)**
Pass 10-byte nonce (not 16). Assert Error about nonce length.

**T26 — AEAD encrypt/decrypt roundtrip (#23, #24)**
Feed 4 chunks through `AeadEncrypt → AeadDecrypt`. Assert byte-identical.
Assert each encrypted chunk is exactly 16 bytes larger (GCM tag).

**T27 — AEAD tamper detection flips single bit (#23, #24)**
Encrypt 3 chunks. Flip one bit in chunk 2's ciphertext. Decrypt: chunk 1
succeeds, chunk 2 emits `Error("aead: authentication failed at chunk 1")`.

**T28 — AEAD with invalid nonce_prefix length (#23)**
Pass 10-byte nonce_prefix (not 12). Assert Error on construction.

**T29 — AEAD encrypt produces unique ciphertext per chunk (#23)**
Encrypt 3 identical plaintext chunks. Assert all 3 ciphertext chunks
differ (per-chunk nonce counter ensures uniqueness).

**T30 — HMAC-SHA256 matches RFC 4231 test vector (#25)**
RFC 4231 Test Case 2: key=`"Jefe"`, data=`"what do ya want for nothing?"`
split across 3 chunks. Assert output =
`5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843`.

**T31 — HMAC-SHA256 with empty key emits Error (#25)**
Pass empty key. Assert Error about key length.

**T32 — MD5 matches known digest for S3 ETag (#26)**
Feed `"Hello, World!"` as 2 chunks. Assert output = 16 bytes matching
`65a8e27d8879283831b664bd8b7f0ad4`.

**T33 — MD5 on empty input (#26)**
Feed 0 Data + End. Assert output = MD5 of empty string
(`d41d8cd98f00b204e9800998ecf8427e`).

**T34 — SHA-512 produces 64-byte output matching reference (#27)**
Feed 1 MB (16 chunks). Assert output is exactly 64 bytes. Compare
against independently computed `sha2::Sha512`.

**T35 — BLAKE3 matches reference implementation (#28)**
Feed `"BLAKE3 test vector"` as single chunk. Assert output matches
`blake3::hash(b"BLAKE3 test vector")`.

**T36 — BLAKE3 on multi-chunk equals single-chunk (#28)**
Feed 1 MB as 16 chunks and as 1 chunk. Assert identical 32-byte digest.

**T37 — All 4 hash functions produce different digests (#26, #27, #28, #10)**
Feed identical 1 MB through MD5, SHA-256, SHA-512, BLAKE3. Assert
output lengths 16, 32, 64, 32. Assert all digests differ.

**T38 — Redact replaces email, phone, SSN (#29)**
Input: `"Contact user@example.com or call 555-123-4567, SSN: 123-45-6789, name: John"`.
Assert PII replaced with `[REDACTED]`. Assert `"name: John"` unchanged.

**T39 — Redact with pattern split across chunk boundary (#29)**
Email `"user@exam"` in chunk 1, `"ple.com rest"` in chunk 2. Assert
full email redacted despite spanning chunks.

**T40 — Redact with custom patterns param (#29)**
Set `patterns="\\d{4}-\\d{4}"`. Input: `"card 1234-5678 ok"`. Assert
output: `"card [REDACTED] ok"`. Assert default PII patterns NOT applied.

### 5.2 Encoding & Format Conversion (T41–T62)

**T41 — HexEncode output is exactly 2× input and lowercase (#30)**
Feed 128 KB random binary. Assert output length = 256 KB. Assert all
bytes in `[0-9a-f]`.

**T42 — HexEncode/Decode roundtrip with all 256 byte values (#30, #31)**
Create 256-byte chunk containing 0x00–0xFF. `HexEncode → HexDecode`.
Assert byte-identical.

**T43 — HexDecode rejects invalid hex character (#31)**
Feed `"48656c6c6fGG"`. Assert Error with position info.

**T44 — HexDecode rejects odd-length input (#31)**
Feed 11-char hex string. Assert Error about odd length.

**T45 — UTF-8 validate passes 4-byte char split across chunks (#32)**
🦀 (`F0 9F A6 80`) split: [F0, 9F] in chunk 1, [A6, 80] in chunk 2.
Assert pass-through succeeds. Collect and assert valid UTF-8.

**T46 — UTF-8 validate buffers 3-byte sequence at chunk end (#32)**
3-byte UTF-8 char (€ = `E2 82 AC`) with first byte at end of chunk 1,
remaining 2 bytes at start of chunk 2. Assert correct reassembly.

**T47 — UTF-8 validate rejects invalid byte 0xFE (#32)**
Feed `[0x48, 0x65, 0xFE, 0x6C]`. Assert Error with byte and position.

**T48 — LineEnding CRLF→LF on Windows-style text (#33)**
Feed `"line1\r\nline2\r\nline3\r\n"` across 2 chunks. `target=lf`.
Assert output = `"line1\nline2\nline3\n"`. Assert size decreased.

**T49 — LineEnding LF→CRLF (#33)**
Feed `"line1\nline2\n"`. `target=crlf`. Assert `"line1\r\nline2\r\n"`.

**T50 — LineEnding on mixed endings normalizes all (#33)**
Feed `"a\r\nb\nc\r\nd\n"`. `target=lf`. Assert `"a\nb\nc\nd\n"`.

**T51 — JsonPrettyPrint produces indented output (#34)**
Feed compact `{"a":1,"b":[2,3]}`. Assert output contains `\n` and
spaces. Parse as JSON and assert semantically equal.

**T52 — JsonPrettyPrint rejects invalid JSON (#34)**
Feed `{invalid}`. Assert Error with parse position.

**T53 — JsonMinify strips all whitespace (#35)**
Feed pretty JSON with newlines/tabs across 3 chunks. Assert output
has no formatting whitespace. Assert valid JSON.

**T54 — JsonPrettyPrint → JsonMinify roundtrip (#34, #35)**
Feed compact JSON. `PrettyPrint → Minify`. Assert byte-identical to
original.

**T55 — JsonLines converts array to NDJSON (#36)**
Feed `[{"a":1},{"b":2},{"c":3}]` across 2 chunks. Assert 3 output
lines, each valid JSON object terminated by `\n`.

**T56 — JsonLines rejects non-array JSON (#36)**
Feed `{"not":"array"}`. Assert Error.

**T57 — CsvToJson 1000-row CSV across 8 chunks (#37)**
5 columns, 1000 rows. Assert 1000 NDJSON lines. Parse line 500 and
verify all column names present.

**T58 — CsvToJson with header split across boundary (#37)**
Header `"name,email,pho"` in chunk 1, `"ne,city,zip\n..."` in chunk 2.
Assert 5 columns parsed correctly.

**T59 — CsvToJson with quoted fields containing commas (#37)**
Row: `"Smith","New York, NY","12345"`. Assert JSON field contains
comma: `{"col2":"New York, NY"}`.

**T60 — Base32 encode/decode roundtrip (#38, #39)**
Feed 128 KB binary. `Encode → Decode`. Assert byte-identical. Assert
encoded contains only `[A-Z2-7=]`.

**T61 — Base32Decode rejects invalid characters (#39)**
Feed `"JBSWY3DP!!!"`. Assert Error.

**T62 — Base32Encode on empty input (#38)**
Feed 0 Data + End. Assert output is End only (empty encodes to empty).

### 5.3 Data Processing & Analytics (T63–T86)

**T63 — Filter non_empty drops zero-length chunks (#40)**
Feed 10 chunks: 7 non-empty, 3 zero-length. Assert output has 7 chunks.

**T64 — Filter contains:ERROR keeps matching chunks (#40)**
Feed 5 chunks, 2 containing `"ERROR"`. `predicate=contains:ERROR`.
Assert output has 2 chunks.

**T65 — Filter min_size:100 drops small chunks (#40)**
Feed chunks of sizes [50, 200, 30, 150, 80]. `predicate=min_size:100`.
Assert output has 2 chunks (200, 150).

**T66 — LineCount on 10,000-line log across 12 uneven chunks (#41)**
Exactly 10,000 `\n` chars split at arbitrary positions. Assert output
= 10000 as u64 big-endian.

**T67 — LineCount on input with no newlines (#41)**
Feed 3 chunks of binary data with no `\n`. Assert output = 0.

**T68 — LineCount on empty stream (#41)**
Feed 0 Data + End. Assert output = 0.

**T69 — WordCount tracks in-word state across boundary (#42)**
`"hello wor"` in chunk 1, `"ld goodbye"` in chunk 2. Assert count = 3
(not 4 — "world" is one word split across chunks).

**T70 — WordCount on multiple whitespace types (#42)**
Input: `"word1\tword2\n  word3"`. Assert count = 3 (tab, newline,
multiple spaces all delimit).

**T71 — MinMax on binary data (#43)**
Chunks containing 0x10–0xEF. Assert `[0x10, 0xEF]`. Single chunk
`[0x00, 0xFF]`. Assert `[0x00, 0xFF]`.

**T72 — Histogram uniform distribution (#44)**
256 bytes, one of each 0x00–0xFF. Assert all 256 buckets = 1.

**T73 — Histogram skewed distribution (#44)**
1024 bytes all `0x41`. Assert bucket 0x41 = 1024, all others = 0.

**T74 — Sample(rate=10) on 100 chunks (#45)**
Assert output has 10 chunks at indices 0, 10, 20, ..., 90. Assert
content matches corresponding input chunks.

**T75 — Sample(rate=1) passes all chunks (#45)**
Feed 20 chunks. Assert all 20 pass through.

**T76 — Head(3) on 100-chunk stream (#46)**
Assert exactly 3 Data + End. Verify chunks match input 0, 1, 2.

**T77 — Head(0) emits only End (#46)**
Feed 5 chunks. Assert End only, no Data.

**T78 — Head(100) on 5-chunk stream returns all 5 (#46)**
N > stream length. Assert all 5 returned + End.

**T79 — Tail(2) on 50-chunk stream (#47)**
Sequential chunks "chunk_00"–"chunk_49". Assert output = chunks 48, 49.

**T80 — Tail(100) on 5-chunk stream returns all 5 (#47)**
N > stream length. Assert all 5 in order.

**T81 — Tail(1) returns only last chunk (#47)**
Feed 10 chunks. Assert output = chunk 9 only.

**T82 — Deduplicate drops repeated chunks (#48)**
10 chunks: [0,3,6] identical A, [1,4,7] identical B, [2,5,8,9] unique.
Assert 6 output chunks (4 dropped).

**T83 — Deduplicate on all-unique stream passes all (#48)**
Feed 10 unique chunks. Assert all 10 pass through.

**T84 — Sort byte-orders 5 chunks (#49)**
`"cherry"`, `"apple"`, `"banana"`, `"date"`, `"elderberry"`. Assert
sorted order.

**T85 — Sort preserves duplicates (#49)**
3 identical chunks. Assert output has 3 (sort ≠ dedup).

**T86 — Unique deduplicates and sorts (#50)**
`"banana"`, `"apple"`, `"banana"`, `"cherry"`, `"apple"`. Assert
output: `"apple"`, `"banana"`, `"cherry"`.

### 5.4 Compression & Transformation (T87–T106)

**T87 — Zstd roundtrip level 1 (#51, #52)**
1 MB text, 16 chunks. Assert byte-identical. Assert compressed < input.

**T88 — Zstd roundtrip level 19 (#51, #52)**
Same input. Assert byte-identical. Assert level 19 < level 1 size.

**T89 — Zstd decompress rejects corrupt frame (#52)**
Feed random bytes. Assert Error.

**T90 — Zstd on empty stream (#51, #52)**
Feed 0 Data + End. Assert End only (no crash).

**T91 — LZ4 roundtrip with random binary (#53, #54)**
512 KB random. Assert byte-identical. No error even if ratio > 1.

**T92 — LZ4 roundtrip with compressible text (#53, #54)**
1 MB repeated `"AAAA..."`. Assert byte-identical. Assert < 10% size.

**T93 — LZ4 decompress rejects truncated block (#54)**
Feed truncated LZ4 data. Assert Error.

**T94 — Snappy roundtrip on repeated text (#55, #56)**
1 MB repeated text. Assert byte-identical. Assert < 50% size.

**T95 — Snappy decompress rejects non-Snappy data (#56)**
Feed zstd data to Snappy. Assert Error.

**T96 — Brotli roundtrip on HTML (#57, #58)**
256 KB HTML. Assert byte-identical. Assert Brotli ratio < Zstd-3 on
same text input.

**T97 — Brotli quality affects ratio (#57)**
Quality 1 vs 11 on same input. Assert q11 smaller. Both decompress OK.

**T98 — Brotli decompress rejects corrupt data (#58)**
Feed random bytes. Assert Error.

**T99 — Cross-codec rejection: Zstd → LZ4 decompressor (#52, #54)**
Assert Error. Codecs not interchangeable.

**T100 — Cross-codec rejection: Snappy → Brotli decompressor (#56, #58)**
Assert Error.

**T101 — All 5 codecs roundtrip on same 1 MB input (#8/#9, #51–#58)**
zlib, zstd, lz4, snappy, brotli. Assert all 5 produce byte-identical
output. Compare compressed sizes.

**T102 — Pad to 16-byte blocks (#59)**
Chunks [10, 16, 20, 1] → [16, 16, 32, 16]. Assert 0x00 padding.
Assert original content at start.

**T103 — Pad with custom padding_byte (#59)**
`padding_byte=0xFF`. Assert padding bytes are 0xFF not 0x00.

**T104 — Pad on already-aligned chunk (#59)**
64-byte chunk with `block_size=16`. Assert output = input (no padding).

**T105 — Trim strips whitespace per chunk (#60)**
`"  hello  "`, `"\t\nworld\n\t"`, `"no_space"` → `"hello"`, `"world"`,
`"no_space"`.

**T106 — Trim on all-whitespace chunk (#60)**
Feed `"   \t\n  "`. Assert output is empty chunk (0 bytes).

### 5.5 Flow Control & Pipeline Utilities (T107–T126)

**T107 — RateLimit throttles to target throughput (#61)**
`bytes_per_sec=65536`. 4 × 64 KB chunks. Assert elapsed ≥ 3s. 20% tolerance.

**T108 — RateLimit with high limit is near-instant (#61)**
`bytes_per_sec=1073741824`. 1 MB. Assert elapsed < 100 ms.

**T109 — Delay adds fixed latency per chunk (#62)**
`delay_ms=50`. 10 chunks. Assert elapsed ≥ 500 ms. Content preserved.

**T110 — Delay(0) is effectively pass-through (#62)**
`delay_ms=0`. 10 chunks. Assert elapsed < 50 ms. All chunks correct.

**T111 — Timeout emits Error after deadline (#63)**
`timeout_ms=100`. Send 1 chunk, delay 200 ms. Assert Error.

**T112 — Timeout passes through fast stream (#63)**
`timeout_ms=1000`. 5 chunks at 10 ms gaps. Assert all pass + End.

**T113 — Retry succeeds on second attempt (#64)**
Mock: fail first, succeed second. `max_retries=3`. Assert success data.

**T114 — Retry exhausts retries (#64)**
Mock: always fail. `max_retries=2`. Assert Error("retry: 2 attempts failed").

**T115 — Retry with max_retries=0 fails immediately (#64)**
Mock: fail. `max_retries=0`. Assert Error on first failure.

**T116 — Tee duplicates to 3 outputs (#65)**
10 chunks. Assert all 3 receivers get identical 10 Data + End.

**T117 — Tee with slow consumer preserves data (#65)**
20 chunks. Consumer B has 50 ms delay. Assert both get all 20.

**T118 — Tee(1) is effectively pass-through (#65)**
`outputs=1`. Assert single output matches input exactly.

**T119 — Merge from 3 inputs (#66)**
3 inputs × 5 chunks. Assert output has 15 Data + End. All present.

**T120 — Merge with one empty input (#66)**
Input A: 5 chunks. Input B: 0 Data + End. Assert output has 5 chunks.

**T121 — Broadcast gates on slowest consumer (#67)**
10 chunks, 2 consumers. B blocks 200 ms at chunk 3. Assert producer
waits. Both get all 10.

**T122 — Partition routes by min_size (#68)**
Sizes [50, 200, 30, 150, 80]. `min_size:100`. A=[200,150], B=[50,30,80].

**T123 — Partition all-true (#68)**
All match. A=all, B=empty+End.

**T124 — Batch(4) collects correctly (#69)**
12 × 16 KB → 3 × 64 KB. Total = 192 KB.

**T125 — Batch with remainder (#69)**
7 chunks, `batch_size=3`. Output: 2 full + 1 partial = 3 chunks.

**T126 — Debounce suppresses rapid bursts (#70)**
`window_ms=100`. 5 chunks at 10 ms, wait 200 ms, 1 chunk. Assert 2
output chunks.

### 5.6 Validation & Integrity (T127–T142)

**T127 — JsonValidate accepts valid JSON across chunks (#71)**
`{"key":"value","arr":[1,2,3]}` split across 2 chunks. Assert pass-through.

**T128 — JsonValidate rejects trailing comma (#71)**
`{"key":"value",}`. Assert Error with position.

**T129 — JsonValidate rejects truncated JSON (#71)**
`{"key":"val`. Assert Error.

**T130 — SchemaValidate accepts conforming JSON (#72)**
Schema requires `name` (string) + `age` (integer). Input: `{"name":"Alice","age":30}`. Pass.

**T131 — SchemaValidate rejects missing required field (#72)**
`{"name":"Alice"}` (no `age`). Assert Error about missing field.

**T132 — SchemaValidate rejects wrong type (#72)**
`{"name":"Alice","age":"thirty"}`. Assert Error about type mismatch.

**T133 — MagicBytes accepts PNG header (#73)**
First chunk: `89 50 4E 47...`. `expected="89504e47"`. Assert pass-through.

**T134 — MagicBytes rejects JPEG when expecting PNG (#73)**
First chunk: `FF D8 FF E0`. Assert Error on first chunk.

**T135 — MagicBytes on empty first chunk (#73)**
First chunk is 0 bytes. Assert Error (no magic bytes to check).

**T136 — SizeLimit allows under-limit stream (#74)**
`max_bytes=1048576`. Feed 999 KB. Assert pass-through + End.

**T137 — SizeLimit rejects over-limit at crossing chunk (#74)**
Feed 1.1 MB in 18 chunks. Assert Error at the chunk crossing 1 MB.

**T138 — ChecksumVerify correct CRC32 (#75)**
Known input. Correct CRC32. Assert pass-through + End.

**T139 — ChecksumVerify wrong CRC32 (#75)**
`expected_crc32=00000000`. Assert data passes, Error after End.

**T140 — Sha256Verify correct hash (#76)**
Assert pass-through + End.

**T141 — Sha256Verify wrong hash (#76)**
Assert data passes, Error after End with expected vs actual.

**T142 — NonEmpty passes non-empty, rejects empty (#77)**
Non-empty: 3 chunks → pass + End. Empty: 0 Data + End → Error.

### 5.7 Text Processing (T143–T162)

**T143 — Replace within single chunk (#78)**
`"hello world hello"`. Find `"hello"`, replace `"hi"`. Assert `"hi world hi"`.

**T144 — Replace across chunk boundary (#78)**
`"hello wo"` + `"rld goodbye"`. Find `"world"`, replace `"earth"`.
Assert `"hello earth goodbye"`.

**T145 — Replace with regex pattern (#78)**
Find `"\\d+"`, replace `"N"`. Input `"item 42 costs 100"`. Assert
`"item N costs N"`.

**T146 — Prefix prepends to first chunk only (#79)**
`prefix="HEADER: "`. 3 chunks. Assert first starts with `"HEADER: "`.
Chunks 2–3 unchanged.

**T147 — Prefix on empty stream (#79)**
0 Data + End. Assert output = 1 Data chunk (`"HEADER: "`) + End.

**T148 — Suffix appends after last data (#80)**
`suffix="\n--END--"`. 3 chunks. Assert 4 Data chunks: original 3 +
suffix. End follows.

**T149 — LinePrefix on every line (#81)**
`prefix="> "`. `"line1\nline2\nline3\n"` across 2 chunks. Assert
`"> line1\n> line2\n> line3\n"`.

**T150 — LinePrefix with line split across chunks (#81)**
`"line1\nli"` + `"ne2\nline3\n"`. Assert all 3 lines get prefix.

**T151 — Grep keeps matching lines (#82)**
100-line log, 15 contain `"ERROR"`. Assert 15 output lines.

**T152 — Grep invert mode (#82)**
Same input. `invert=true`. Assert 85 lines.

**T153 — Grep with line split across boundary (#82)**
`"ERROR: dis"` + `"k full\n"`. Pattern `"ERROR"`. Assert line included.

**T154 — Grep with no matches produces empty output (#82)**
Pattern `"ZZZZZ"` on 100 lines. Assert 0 lines + End.

**T155 — Sed with capture groups (#83)**
`"2024-01-15 ERROR"`. Pattern `"(\d{4})-(\d{2})-(\d{2})"`, replacement
`"$3/$2/$1"`. Assert `"15/01/2024 ERROR"`.

**T156 — Sed no matches passes unchanged (#83)**
`"no dates here"`. Same pattern. Assert unchanged.

**T157 — Sed invalid regex emits Error (#83)**
Pattern `"[invalid"`. Assert Error about regex compilation.

**T158 — TruncateLines truncates long lines (#84)**
`max_line_bytes=10`. `"short\nvery long line here\nok\n"`. Assert
second line truncated to 10 bytes.

**T159 — TruncateLines on short lines passes unchanged (#84)**
All lines < max. Assert byte-identical output.

**T160 — CharsetConvert Latin-1 → UTF-8 (#85)**
`[0xC9, 0x6C, 0xE8, 0x76, 0x65]` → valid UTF-8 "Élève".

**T161 — CharsetConvert UTF-16LE → UTF-8 (#85)**
UTF-16LE encoded "Hello" → UTF-8 "Hello".

**T162 — CharsetConvert unknown encoding emits Error (#85)**
`from=cp437`. Assert Error.

### 5.8 CAS-Specific Operations (T163–T176)

**T163 — CAddrEmbed appends correct SHA-256 (#86)**
4 chunks. Last Data before End = 32 bytes. Independently compute
SHA-256 of preceding chunks. Assert match.

**T164 — CAddrEmbed on empty stream (#86)**
0 Data + End. Assert 1 Data (SHA-256 of empty) + End.

**T165 — CAddrEmbed on single-byte stream (#86)**
1 chunk = `[0x42]`. Assert embedded hash = SHA-256 of `[0x42]`.

**T166 — CAddrVerify accepts correct hash (#87)**
Known 4-chunk input. Correct hash. Assert pass-through + End.

**T167 — CAddrVerify rejects wrong hash (#87)**
`expected_caddr=0000...0000`. Assert data passes, Error after End.

**T168 — Diff XOR of two aligned streams (#88)**
A: `[0xFF, 0x00, 0xAA]` × 3 chunks. B: `[0x00, 0xFF, 0x55]` × 3.
Assert diff = `[0xFF, 0xFF, 0xFF]` × 3.

**T169 — Diff with unequal-length streams (#88)**
A: 3 chunks. B: 5 chunks. Assert output has 5 chunks (A padded with
zeros for chunks 4–5).

**T170 — Patch reverses Diff (#88, #89)**
`Patch(base, Diff(base, target)) == target`. 4-chunk streams.

**T171 — MerkleTree deterministic root (#90)**
Same 8 chunks twice. Assert identical 32-byte root.

**T172 — MerkleTree root changes on 1-byte change (#90)**
Change 1 byte in chunk 4. Assert root differs.

**T173 — MerkleTree on single chunk (#90)**
1 chunk. Assert root = SHA-256 of that chunk (tree is just the leaf).

**T174 — ContentType detects PNG, JPEG, PDF, gzip (#91)**
4 separate runs with magic bytes. Assert correct `Content-Type:` for
each: `image/png`, `image/jpeg`, `application/pdf`, `application/gzip`.

**T175 — ContentType unknown format defaults to octet-stream (#91)**
Random bytes with no known magic. Assert `application/octet-stream`.

**T176 — ChunkHash appends 32-byte SHA-256 per chunk (#92)**
Chunks [100, 200, 50] → [132, 232, 82]. Split last 32 bytes, verify
SHA-256 of prefix.

### 5.9 Numeric & Scientific (T177–T192)

**T177 — Sum on decimal stream (#93)**
`"10.5\n20.0\n30.5\n"` across 2 chunks. Assert output ≈ `"61"`.

**T178 — Sum skips blank and non-numeric lines (#93)**
`"10\n\nhello\n20\n"`. Assert `"30"`.

**T179 — Sum on empty stream (#93)**
0 Data + End. Assert `"0"`.

**T180 — Average on 3 numbers (#94)**
`"10\n20\n30\n"`. Assert output ≈ `"20"`.

**T181 — Average on empty stream emits Error (#94)**
0 Data + End. Assert Error("average: no valid numbers").

**T182 — Average on single number (#94)**
`"42.5\n"`. Assert output = `"42.5"`.

**T183 — BitwiseAnd correctness (#95)**
`[0xFF, 0x0F, 0xF0, 0x00]` with `mask=0x0F`. Assert `[0x0F, 0x0F, 0x00, 0x00]`.

**T184 — BitwiseOr correctness (#96)**
Same input, `mask=0xF0`. Assert `[0xFF, 0xFF, 0xF0, 0xF0]`.

**T185 — BitwiseNot correctness (#97)**
Same input. Assert `[0x00, 0xF0, 0x0F, 0xFF]`.

**T186 — BitwiseNot applied twice is identity (#97)**
`Not → Not`. Assert output = input.

**T187 — ByteSwap 2-byte words (#98)**
`[0x01, 0x02, 0x03, 0x04]`, `word_size=2`. Assert `[0x02, 0x01, 0x04, 0x03]`.

**T188 — ByteSwap 4-byte words (#98)**
`[0x01, 0x02, 0x03, 0x04]`, `word_size=4`. Assert `[0x04, 0x03, 0x02, 0x01]`.

**T189 — ByteSwap rejects non-aligned chunk (#98)**
5-byte chunk, `word_size=4`. Assert Error.

**T190 — ByteSwap applied twice is identity (#98)**
`Swap(2) → Swap(2)`. Assert output = input.

**T191 — Entropy constant vs random (#99)**
64 KB `0x00` → entropy < 0.01. 64 KB random → entropy > 7.9.

**T192 — RollingHash deterministic output (#100)**
Same 4 chunks twice. Assert identical output. Assert hash bytes differ
between chunks.

### 5.10 Cross-Function Pipeline Integration (T193–T200)

**T193 — Secure data pipeline: ZstdCompress → AesEncrypt → HmacSha256**
Feed 1 MB text. Assert HMAC output = 32 bytes. Decrypt + decompress
separately and assert byte-identical to original.

**T194 — Log processing: Grep("ERROR") → LineCount**
100-line log, 15 errors. Assert line count = 15.

**T195 — Validation pipeline: JsonValidate → SizeLimit(10MB) → Sha256Verify**
Valid 50 KB JSON with correct hash. Assert pass-through. Then wrong
hash — assert Error.

**T196 — S3-compatible ingest: ZstdCompress → Md5 → CAddrEmbed**
1 MB input. Assert MD5 = 16 bytes. Assert CAddr = 32 bytes. Assert
compressed data recoverable.

**T197 — Fan-out analytics: Tee(3) → [Sha256, ByteCount, Histogram]**
1 MB input. Assert SHA-256 = 32 bytes, ByteCount = 1048576, Histogram
= 2048 bytes. All three computed from same input.

**T198 — Normalize + encrypt: Uppercase → LineEnding(lf) → AesEncrypt → AesDecrypt → Lowercase**
Feed mixed-case text with CRLF. Assert final output is lowercase with
LF endings.

**T199 — 10-stage fusible chain: Identity × 5 → HexEncode → HexDecode → BitwiseNot → BitwiseNot → Trim**
Feed 64 KB. Assert output = trimmed input. Validates fusion optimizer
handles long chains.

**T200 — Concurrent: 50 independent pipelines each running Compress → Decompress**
Spawn 50 pipelines simultaneously. Assert all 50 produce byte-identical
output. Assert no deadlocks or data corruption under contention.

---

## 6. Edge Cases & Error Handling

| # | Category | Edge Case | Expected Behavior | Functions Affected |
|---|----------|-----------|-------------------|-------------------|
| 1 | Crypto | Key is valid hex but wrong length (e.g., 30 bytes) | `ComputeFailed("aes: invalid key length 30, expected 32")` | #21–#24 |
| 2 | Crypto | Nonce is empty string | `ComputeFailed("aes: nonce must be 16 bytes")` | #21, #22 |
| 3 | Crypto | Key contains non-hex characters | `ComputeFailed("aes: invalid hex in key")` | #21–#25 |
| 4 | Crypto | AEAD chunk index overflows u32 (>4 billion chunks) | `ComputeFailed("aead: chunk index overflow")` — practically unreachable at 64 KB chunks (256 TB) | #23, #24 |
| 5 | Crypto | HMAC with zero-length key | `ComputeFailed("hmac: key must not be empty")` | #25 |
| 6 | Crypto | Redact with invalid regex pattern | `ComputeFailed("redact: invalid regex: ...")` at construction, before any chunks | #29 |
| 7 | Crypto | Redact with empty patterns param | No-op pass-through (nothing to redact) | #29 |
| 8 | Encoding | Hex input has odd number of characters | `ComputeFailed("hex decode: odd length 11")` | #31 |
| 9 | Encoding | Hex input contains uppercase (A–F) | Decode succeeds (case-insensitive) | #31 |
| 10 | Encoding | UTF-8 stream ends mid-sequence (e.g., [0xC3] then End) | `ComputeFailed("utf8: incomplete sequence at end of stream")` | #32 |
| 11 | Encoding | JSON input is valid but >100 MB | Buffered functions allocate O(input) — may OOM; §2.12 budget enforcement should reject | #34–#36, #71, #72 |
| 12 | Encoding | CSV with no header row (empty first line) | `ComputeFailed("csv: empty header row")` | #37 |
| 13 | Encoding | CSV with mismatched column count in data row | `ComputeFailed("csv: row 42 has 3 columns, header has 5")` | #37 |
| 14 | Encoding | Base32 input with padding in wrong position | `ComputeFailed("base32: invalid padding")` | #39 |
| 15 | Compression | Decompress input that is valid but decompresses to >10× chunk size | Output chunk may be very large; bounded by available memory | #52, #54, #56, #58 |
| 16 | Compression | Compress empty chunk (0 bytes) | Emit codec-specific empty frame (valid compressed representation of nothing) | #51, #53, #55, #57 |
| 17 | Compression | Zstd level out of range (e.g., level=99) | `ComputeFailed("zstd: level 99 out of range 1..=22")` | #51 |
| 18 | Compression | Brotli quality out of range | `ComputeFailed("brotli: quality 15 out of range 0..=11")` | #57 |
| 19 | Compression | Pad with block_size=0 | `ComputeFailed("pad: block_size must be > 0")` | #59 |
| 20 | Compression | Trim on chunk that is entirely whitespace | Emit zero-length chunk (not dropped — Filter handles dropping) | #60 |
| 21 | Flow | RateLimit with bytes_per_sec=0 | `ComputeFailed("rate_limit: bytes_per_sec must be > 0")` | #61 |
| 22 | Flow | Timeout with timeout_ms=0 | Immediate timeout on first recv — effectively always errors | #63 |
| 23 | Flow | Retry with max_retries=0 | No retries; first error propagates immediately | #64 |
| 24 | Flow | Tee with outputs=0 | `ComputeFailed("tee: outputs must be >= 1")` | #65 |
| 25 | Flow | Merge with 0 inputs | Emit End immediately (nothing to merge) | #66 |
| 26 | Flow | Partition with all chunks matching predicate | Output A gets all chunks; output B gets 0 Data + End | #68 |
| 27 | Flow | Partition with no chunks matching | Output A gets 0 Data + End; output B gets all chunks | #68 |
| 28 | Flow | Batch with batch_size=1 | Effectively pass-through (each chunk emitted individually) | #69 |
| 29 | Flow | Debounce with window_ms=0 | Effectively pass-through (no suppression) | #70 |
| 30 | Flow | Broadcast consumer drops receiver mid-stream | Producer detects closed channel, terminates gracefully | #67 |
| 31 | Validation | JSON Schema param is itself invalid JSON | `ComputeFailed("schema: invalid schema JSON: ...")` | #72 |
| 32 | Validation | MagicBytes expected longer than first chunk | `ComputeFailed("magic: first chunk (10 bytes) shorter than expected (16 bytes)")` | #73 |
| 33 | Validation | SizeLimit with max_bytes=0 | First Data chunk triggers Error immediately | #74 |
| 34 | Validation | ChecksumVerify expected_crc32 is not valid hex | `ComputeFailed("checksum: invalid hex in expected_crc32")` | #75 |
| 35 | Validation | Sha256Verify expected_hash wrong length | `ComputeFailed("sha256_verify: expected 64 hex chars, got 32")` | #76 |
| 36 | Text | Grep with invalid regex | `ComputeFailed("grep: invalid regex: ...")` at construction | #82 |
| 37 | Text | Sed replacement references non-existent capture group | `ComputeFailed("sed: capture group $3 not found")` | #83 |
| 38 | Text | Replace find pattern is empty string | `ComputeFailed("replace: find pattern must not be empty")` | #78 |
| 39 | Text | TruncateLines with max_line_bytes=0 | Every line truncated to empty; only `\n` chars remain | #84 |
| 40 | Text | CharsetConvert from=utf8 to=utf8 | No-op pass-through (same encoding) | #85 |
| 41 | Text | LinePrefix with empty prefix | No-op pass-through | #81 |
| 42 | CAS | Diff with inputs of different chunk sizes | XOR operates on min(len_a, len_b) per pair; shorter chunk zero-padded | #88 |
| 43 | CAS | Patch with diff longer than base | Extra diff chunks applied to zero-padded base (extends output) | #89 |
| 44 | CAS | MerkleTree on single chunk | Root = SHA-256 of that chunk (degenerate tree) | #90 |
| 45 | CAS | ContentType on chunk shorter than 4 bytes | Best-effort detection with available bytes; may default to `application/octet-stream` | #91 |
| 46 | CAS | ChunkHash on empty chunk (0 bytes) | Output = 32 bytes (SHA-256 of empty = `e3b0c44298fc...`) | #92 |
| 47 | Numeric | Sum with NaN/Infinity in input | Skip line with warning (treat as non-numeric) | #93 |
| 48 | Numeric | Sum overflow (f64 precision loss on very large sums) | No error; f64 precision limits apply (~15 significant digits) | #93 |
| 49 | Numeric | ByteSwap word_size not in {2, 4, 8} | `ComputeFailed("byte_swap: word_size must be 2, 4, or 8")` | #98 |
| 50 | Numeric | Entropy on single-byte stream | Entropy = 0.0 (only one symbol observed) | #99 |
| 51 | Numeric | RollingHash window_size=0 | `ComputeFailed("rolling_hash: window_size must be > 0")` | #100 |
| 52 | Numeric | RollingHash window_size > chunk_size | Window wraps across chunks; state carried correctly | #100 |
| 53 | General | Any function receives Error as first chunk | Propagate Error immediately, terminate task | All |
| 54 | General | Downstream consumer drops receiver mid-stream | `tx.send().await.is_err()` → task terminates silently | All |
| 55 | General | Empty stream (0 Data + End) through map function | Emit End only (no Data) | All map functions |
| 56 | General | Empty stream through accumulator | Emit finalize(init_state) + End | All accumulators |
| 57 | General | Single zero-byte chunk through any function | Function-specific: maps emit 0-byte output; accumulators fold empty | All |
| 58 | General | Very large chunk (100 MB single chunk) | Functions process normally; memory = O(chunk_size) for maps | All map functions |
| 59 | General | Channel capacity=1 (extreme backpressure) | All functions work correctly; throughput reduced | All |
| 60 | General | Concurrent access: 100 pipelines using same function type | No shared mutable state between instances; all succeed | All |

---

## 7. Performance Analysis

### 7.1 Compression Codec Comparison

Expected throughput on modern x86_64 (single core, 64 KB chunks):

| Codec | Compress (MB/s) | Decompress (MB/s) | Ratio (text) | Ratio (binary) | Per-Chunk Latency |
|-------|----------------|-------------------|-------------|---------------|-------------------|
| zlib (existing, level=fast) | ~200 | ~400 | 3.0× | 1.5× | ~320 μs |
| zstd level 1 | ~500 | ~1,200 | 2.8× | 1.4× | ~128 μs |
| zstd level 3 (default) | ~350 | ~1,200 | 3.2× | 1.6× | ~183 μs |
| zstd level 19 | ~10 | ~1,200 | 3.8× | 1.8× | ~6,400 μs |
| lz4 | ~2,000 | ~4,000 | 2.1× | 1.2× | ~32 μs |
| snappy | ~1,500 | ~3,000 | 2.3× | 1.3× | ~43 μs |
| brotli quality 4 | ~50 | ~400 | 4.0× | 1.7× | ~1,280 μs |
| brotli quality 11 | ~5 | ~400 | 4.5× | 2.0× | ~12,800 μs |

Key takeaways:
- LZ4 is 10× faster than zlib for compression with ~70% of the ratio
- Zstd level 3 matches zlib ratio at 1.75× the speed
- Brotli quality 11 has the best ratio but is 40× slower than zstd-3
- All codecs decompress faster than they compress (asymmetric)
- Per-chunk latency at 64 KB: LZ4 (32 μs) to brotli-11 (12.8 ms)

#### 7.1.1 Benchmark Results (Measured)

> Full analysis: [`docs/phase2/section-2.14-7.1-compression-codec-benchmark-results.md`](section-2.14-7.1-compression-codec-benchmark-results.md)

10 benchmark scenarios executed via Criterion (`benches/compression_codec_comparison.rs`):

| Codec | Measured Compress (MB/s) | Measured Decompress (MB/s) | Per-Chunk Latency (64 KB) |
|-------|-------------------------:|---------------------------:|--------------------------:|
| zstd level 1 | 717 | 657 | 1,421 μs |
| zstd level 3 | 695 | 703 | 1,479 μs |
| zstd level 19 | 30 | 670 | 31,305 μs |
| lz4 | 728 | 720 | 1,331 μs |
| snappy | 733 | 709 | 1,325 μs |
| brotli quality 4 | 96 | 164 | 2,347 μs |
| brotli quality 11 | 16 | 159 | 4,607 μs |

**Key findings:**
- Streaming framework overhead (~1.2 ms) creates a ~730 MB/s throughput ceiling for fast codecs
- Zstd level 3 is the optimal default: 695 MB/s compress, near-identical to level 1
- LZ4 ≈ Snappy in streaming context (raw speed differences masked by overhead)
- Brotli quality 4 is the practical maximum for real-time streaming (96 MB/s)
- Decompression throughput is overhead-bound for all fast codecs (~670–720 MB/s)

### 7.2 Hash Algorithm Comparison

| Algorithm | Throughput (MB/s) | Output Size | Per-Chunk (64 KB) | SIMD | Use Case |
|-----------|------------------|-------------|-------------------|------|----------|
| MD5 (#26) | ~800 | 16 bytes | ~80 μs | No | S3 ETag compatibility |
| SHA-256 (#10, existing) | ~500 | 32 bytes | ~128 μs | SHA-NI | Content addressing |
| SHA-512 (#27) | ~700 | 64 bytes | ~91 μs | No | Higher security margin |
| BLAKE3 (#28) | ~5,000 | 32 bytes | ~13 μs | AVX2/NEON | Fastest general hash |
| CRC32 (#12, existing) | ~10,000 | 4 bytes | ~6 μs | SSE4.2 | Integrity checks |
| HMAC-SHA256 (#25) | ~450 | 32 bytes | ~142 μs | SHA-NI | Keyed authentication |

Key takeaways:
- BLAKE3 is 10× faster than SHA-256 and 6× faster than SHA-512
- SHA-512 is faster than SHA-256 on 64-bit CPUs (wider internal state)
- CRC32 with SSE4.2 is the fastest integrity check
- HMAC-SHA256 is ~10% slower than raw SHA-256 (key mixing overhead)

#### 7.2.1 Benchmark Results (Measured)

> Full analysis: [`docs/phase2/section-2.14-7.2-hash-algorithm-benchmark-results.md`](section-2.14-7.2-hash-algorithm-benchmark-results.md)

10 benchmark scenarios executed via Criterion (`benches/hash_algorithm_comparison.rs`), covering all 6 hash producers + ChunkHash, RollingHash, ChecksumVerify, Sha256Verify:

| Algorithm | Measured Throughput (MB/s) | Output Size | Per-Chunk (64 KB) |
|-----------|---------------------------:|:-----------:|-----------:|
| CRC32 | 780 | 4 bytes | 1,238 μs |
| SHA-256 | 638 | 32 bytes | 1,273 μs |
| BLAKE3 | 610 | 32 bytes | 1,356 μs |
| HMAC-SHA256 | 558 | 32 bytes | 1,168 μs |
| MD5 | 385 | 16 bytes | 1,481 μs |
| SHA-512 | 337 | 64 bytes | 1,406 μs |

**Key findings:**
- SHA-256 is the best general-purpose hash (638 MB/s with SHA-NI)
- BLAKE3 parallelism unrealized in single-chunk streaming (610 MB/s vs spec 5,000 MB/s)
- HMAC-SHA256 overhead is only 4.5% over raw SHA-256
- All algorithms overhead-bound at 64 KB (~1.2 ms floor)
- Verify functions add 35–45% overhead due to data passthrough

### 7.3 Crypto Overhead

| Operation | Throughput (MB/s) | Per-Chunk (64 KB) | Notes |
|-----------|------------------|-------------------|-------|
| AES-256-CTR encrypt (#21) | ~3,000 | ~21 μs | AES-NI hardware acceleration |
| AES-256-CTR decrypt (#22) | ~3,000 | ~21 μs | Symmetric (same as encrypt) |
| AES-256-GCM encrypt (#23) | ~2,500 | ~26 μs | +16 byte tag per chunk |
| AES-256-GCM decrypt (#24) | ~2,500 | ~26 μs | +tag verification |
| XOR cipher (#7, existing) | ~10,000 | ~6 μs | Trivial (toy cipher) |

AES-NI makes encryption negligible relative to compression or I/O.
A `Compress → Encrypt` pipeline is dominated by compression:
- zstd-3 compress: 183 μs/chunk
- AES-CTR encrypt: 21 μs/chunk
- Total: 204 μs/chunk (encryption adds 10%)

### 7.4 Text Processing Overhead

| Function | Throughput (MB/s) | Per-Chunk (64 KB) | Notes |
|----------|------------------|-------------------|-------|
| Grep (simple pattern) (#82) | ~2,000 | ~32 μs | regex with literal optimization |
| Grep (complex regex) (#82) | ~200 | ~320 μs | backtracking regex |
| Sed (simple replace) (#83) | ~1,500 | ~43 μs | literal find + replace |
| Sed (capture groups) (#83) | ~300 | ~213 μs | regex with captures |
| Replace (byte pattern) (#78) | ~3,000 | ~21 μs | memmem search |
| Redact (3 PII patterns) (#29) | ~150 | ~427 μs | 3 compiled regexes per line |
| LineCount (#41) | ~8,000 | ~8 μs | bytecount::count SIMD |
| CharsetConvert (#85) | ~500 | ~128 μs | per-byte lookup table |

Boundary-aware functions add ~2 μs/chunk overhead for leftover buffer
management — negligible relative to the transform itself.

### 7.5 Flow Control Overhead

| Function | Overhead per Chunk | Notes |
|----------|-------------------|-------|
| RateLimit (#61) | ~1 μs + sleep | Sleep dominates; overhead is timer syscall |
| Delay (#62) | ~1 μs + sleep | Fixed sleep per chunk |
| Timeout (#63) | ~2 μs | tokio::time::timeout wrapper |
| Tee (N=2) (#65) | ~50 ns × N | Bytes::clone is ref-count bump |
| Tee (N=10) (#65) | ~500 ns | 10 clone + 10 channel sends |
| Merge (N=3) (#66) | ~100 ns | tokio::select! overhead |
| Batch (N=4) (#69) | ~200 ns | BytesMut extend + freeze |
| Partition (#68) | ~50 ns | Predicate eval + 1 channel send |

Flow control functions add minimal overhead. Tee scales linearly with
output count due to `Bytes::clone` (cheap — reference counted, no copy).

### 7.6 Buffered Function Memory Impact

Functions using `spawn_buffered` collect the entire input before
transforming. Memory impact relative to §2.12 budget enforcement:

| Function | Memory = O(?) | 1 MB Input | 100 MB Input | 1 GB Input |
|----------|--------------|-----------|-------------|-----------|
| JsonPrettyPrint (#34) | O(input × 2) | 2 MB | 200 MB | 2 GB ⚠️ |
| JsonMinify (#35) | O(input) | 1 MB | 100 MB | 1 GB ⚠️ |
| JsonLines (#36) | O(input) | 1 MB | 100 MB | 1 GB ⚠️ |
| JsonValidate (#71) | O(input) | 1 MB | 100 MB | 1 GB ⚠️ |
| SchemaValidate (#72) | O(input + schema) | 1 MB | 100 MB | 1 GB ⚠️ |
| Sort (#49) | O(input) | 1 MB | 100 MB | 1 GB ⚠️ |
| Unique (#50) | O(input) | 1 MB | 100 MB | 1 GB ⚠️ |

These functions must acquire §2.12 memory budget permits proportional
to input size. For inputs > memory_budget, the pipeline should use
§2.9 size-aware mode selection to route to batch execution instead.

### 7.7 Benchmark Specifications

Seven Criterion.rs benchmark groups with full code:

#### Benchmark 1: Compression Codec Throughput

```rust
fn bench_compression_codecs(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let sizes = [64 * 1024, 256 * 1024, 1024 * 1024]; // 64 KB, 256 KB, 1 MB
    let text_data = generate_english_text(1024 * 1024);

    let mut group = c.benchmark_group("compression_codecs");
    group.throughput(criterion::Throughput::Bytes(1024 * 1024));

    for &size in &sizes {
        let input = text_data[..size].to_vec();
        let chunks = chunk_bytes(&input, 64 * 1024);

        for codec in ["zlib", "zstd_1", "zstd_3", "zstd_19", "lz4", "snappy", "brotli_4", "brotli_11"] {
            group.bench_function(
                &format!("{codec}/{size}"),
                |b| b.iter(|| {
                    rt.block_on(async {
                        let rx = stream_from_chunks(chunks.clone());
                        let out = create_compressor(codec).stream_execute(vec![rx], &params(codec)).await;
                        drain_stream(out).await
                    })
                }),
            );
        }
    }
    group.finish();
}
```

Measures: compress throughput (MB/s) for 8 codec configurations × 3 sizes.

#### Benchmark 2: Hash Algorithm Throughput

```rust
fn bench_hash_algorithms(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data = vec![0x42u8; 10 * 1024 * 1024]; // 10 MB
    let chunks = chunk_bytes(&data, 64 * 1024); // 160 chunks

    let mut group = c.benchmark_group("hash_algorithms");
    group.throughput(criterion::Throughput::Bytes(10 * 1024 * 1024));

    for (name, func) in [
        ("md5", Box::new(StreamingMd5) as Box<dyn StreamingComputeFunction>),
        ("sha256", Box::new(StreamingSha256)),
        ("sha512", Box::new(StreamingSha512)),
        ("blake3", Box::new(StreamingBlake3)),
        ("crc32", Box::new(StreamingChecksum)),
        ("hmac_sha256", Box::new(StreamingHmacSha256)),
    ] {
        group.bench_function(name, |b| b.iter(|| {
            rt.block_on(async {
                let rx = stream_from_chunks(chunks.clone());
                let params = if name == "hmac_sha256" {
                    hmac_params("0102030405060708091011121314151617181920212223242526272829303132")
                } else {
                    HashMap::new()
                };
                let out = func.stream_execute(vec![rx], &params).await;
                drain_stream(out).await
            })
        }));
    }
    group.finish();
}
```

Measures: hash throughput (MB/s) for 6 algorithms on 10 MB input.

#### Benchmark 3: Crypto Throughput

```rust
fn bench_crypto(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data = vec![0x42u8; 10 * 1024 * 1024];
    let chunks = chunk_bytes(&data, 64 * 1024);
    let key = "0".repeat(64); // 32-byte key as hex
    let nonce = "0".repeat(32); // 16-byte nonce as hex

    let mut group = c.benchmark_group("crypto");
    group.throughput(criterion::Throughput::Bytes(10 * 1024 * 1024));

    for (name, func) in [
        ("aes_ctr_encrypt", Box::new(StreamingEncrypt) as Box<dyn StreamingComputeFunction>),
        ("aes_ctr_decrypt", Box::new(StreamingDecrypt)),
        ("aes_gcm_encrypt", Box::new(StreamingAeadEncrypt)),
        ("aes_gcm_decrypt", Box::new(StreamingAeadDecrypt)),
        ("xor", Box::new(StreamingXor)),
    ] {
        group.bench_function(name, |b| b.iter(|| {
            rt.block_on(async {
                let rx = stream_from_chunks(chunks.clone());
                let params = crypto_params(&key, &nonce);
                let out = func.stream_execute(vec![rx], &params).await;
                drain_stream(out).await
            })
        }));
    }
    group.finish();
}
```

Measures: encrypt/decrypt throughput for AES-CTR, AES-GCM, XOR on 10 MB.

#### Benchmark 4: Text Processing Throughput

```rust
fn bench_text_processing(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // 1 MB log file: 10,000 lines, ~100 bytes each, 15% contain "ERROR"
    let log_data = generate_log_data(10_000, 0.15);
    let chunks = chunk_bytes(&log_data, 64 * 1024);

    let mut group = c.benchmark_group("text_processing");
    group.throughput(criterion::Throughput::Bytes(log_data.len() as u64));

    let cases: Vec<(&str, Box<dyn StreamingComputeFunction>, HashMap<String, String>)> = vec![
        ("grep_simple", Box::new(StreamingGrep), params_from([("pattern", "ERROR")])),
        ("grep_regex", Box::new(StreamingGrep), params_from([("pattern", "ERROR.*timeout|WARN.*disk")])),
        ("sed_replace", Box::new(StreamingSed), params_from([("pattern", "ERROR"), ("replacement", "WARN")])),
        ("line_count", Box::new(StreamingLineCount), HashMap::new()),
        ("word_count", Box::new(StreamingWordCount), HashMap::new()),
        ("redact_3_patterns", Box::new(StreamingRedact), params_from([("patterns", "default")])),
    ];

    for (name, func, params) in &cases {
        group.bench_function(*name, |b| b.iter(|| {
            rt.block_on(async {
                let rx = stream_from_chunks(chunks.clone());
                let out = func.stream_execute(vec![rx], params).await;
                drain_stream(out).await
            })
        }));
    }
    group.finish();
}
```

Measures: text function throughput on realistic log data.

#### Benchmark 5: Roundtrip Pipeline Throughput

```rust
fn bench_roundtrip_pipelines(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data = generate_english_text(10 * 1024 * 1024); // 10 MB
    let chunks = chunk_bytes(&data, 64 * 1024);

    let mut group = c.benchmark_group("roundtrip_pipelines");
    group.throughput(criterion::Throughput::Bytes(10 * 1024 * 1024));

    // Pipeline: Compress → Encrypt → Decrypt → Decompress
    for codec in ["zlib", "zstd_3", "lz4", "snappy"] {
        group.bench_function(&format!("{codec}_aes_roundtrip"), |b| b.iter(|| {
            rt.block_on(async {
                let rx = stream_from_chunks(chunks.clone());
                let compressed = create_compressor(codec).stream_execute(vec![rx], &params(codec)).await;
                let encrypted = StreamingEncrypt.stream_execute(vec![compressed], &crypto_params(&key, &nonce)).await;
                let decrypted = StreamingDecrypt.stream_execute(vec![encrypted], &crypto_params(&key, &nonce)).await;
                let decompressed = create_decompressor(codec).stream_execute(vec![decrypted], &HashMap::new()).await;
                drain_stream(decompressed).await
            })
        }));
    }
    group.finish();
}
```

Measures: end-to-end pipeline throughput for compress+encrypt roundtrips.

#### Benchmark 6: Flow Control Overhead

```rust
fn bench_flow_control(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data = vec![0x42u8; 1024 * 1024]; // 1 MB
    let chunks = chunk_bytes(&data, 64 * 1024); // 16 chunks

    let mut group = c.benchmark_group("flow_control");
    group.throughput(criterion::Throughput::Bytes(1024 * 1024));

    for n_outputs in [1, 2, 4, 8] {
        group.bench_function(&format!("tee_{n_outputs}"), |b| b.iter(|| {
            rt.block_on(async {
                let rx = stream_from_chunks(chunks.clone());
                let params = params_from([("outputs", &n_outputs.to_string())]);
                let out = StreamingTee.stream_execute(vec![rx], &params).await;
                drain_stream(out).await
                // Note: only drains primary output; measures producer overhead
            })
        }));
    }

    for batch_size in [1, 2, 4, 8, 16] {
        group.bench_function(&format!("batch_{batch_size}"), |b| b.iter(|| {
            rt.block_on(async {
                let rx = stream_from_chunks(chunks.clone());
                let params = params_from([("batch_size", &batch_size.to_string())]);
                let out = StreamingBatch.stream_execute(vec![rx], &params).await;
                drain_stream(out).await
            })
        }));
    }
    group.finish();
}
```

Measures: Tee fan-out scaling (1–8 outputs), Batch collection overhead.

#### Benchmark 7: Fusible Chain Scaling

```rust
fn bench_fusible_chain(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data = vec![0x42u8; 1024 * 1024];
    let chunks = chunk_bytes(&data, 64 * 1024);

    let mut group = c.benchmark_group("fusible_chain_scaling");
    group.throughput(criterion::Throughput::Bytes(1024 * 1024));

    // Chain of N fusible functions: HexEncode → HexDecode repeated
    for depth in [2, 4, 8, 16] {
        group.bench_function(&format!("hex_roundtrip_depth_{depth}"), |b| b.iter(|| {
            rt.block_on(async {
                let mut rx = stream_from_chunks(chunks.clone());
                for _ in 0..depth / 2 {
                    rx = StreamingHexEncode.stream_execute(vec![rx], &HashMap::new()).await;
                    rx = StreamingHexDecode.stream_execute(vec![rx], &HashMap::new()).await;
                }
                drain_stream(rx).await
            })
        }));
    }

    // Same chains but with §2.11 fusion enabled (when implemented)
    // Demonstrates fusion benefit: N tasks → 1 task
    for depth in [2, 4, 8, 16] {
        group.bench_function(&format!("bitwise_not_chain_{depth}"), |b| b.iter(|| {
            rt.block_on(async {
                let mut rx = stream_from_chunks(chunks.clone());
                for _ in 0..depth {
                    rx = StreamingBitwiseNot.stream_execute(vec![rx], &HashMap::new()).await;
                }
                drain_stream(rx).await
            })
        }));
    }
    group.finish();
}
```

Measures: channel overhead scaling with pipeline depth for fusible
functions. Even-depth BitwiseNot chains should produce identity output
(self-inverse), isolating pure overhead.

---

## 8. Files Changed

| File | Change | Lines |
|------|--------|-------|
| `deriva-compute/src/streaming_helpers.rs` | **NEW** — `spawn_boundary_map()`, `spawn_buffered()`, `spawn_passthrough()`, `PassAction` enum | ~150 |
| `deriva-compute/src/builtins_streaming_crypto.rs` | **NEW** — #21–#29: Encrypt, Decrypt, AeadEncrypt, AeadDecrypt, HmacSha256, Md5, Sha512, Blake3, Redact | ~350 |
| `deriva-compute/src/builtins_streaming_encoding.rs` | **NEW** — #30–#39: HexEncode/Decode, Utf8Validate, LineEnding, JsonPrettyPrint/Minify/Lines, CsvToJson, Base32Encode/Decode | ~300 |
| `deriva-compute/src/builtins_streaming_analytics.rs` | **NEW** — #40–#50: Filter, LineCount, WordCount, MinMax, Histogram, Sample, Head, Tail, Deduplicate, Sort, Unique | ~400 |
| `deriva-compute/src/builtins_streaming_compression.rs` | **NEW** — #51–#60: Zstd/LZ4/Snappy/Brotli compress+decompress, Pad, Trim | ~300 |
| `deriva-compute/src/builtins_streaming_flow.rs` | **NEW** — #61–#70: RateLimit, Delay, Timeout, Retry, Tee, Merge, Broadcast, Partition, Batch, Debounce | ~500 |
| `deriva-compute/src/builtins_streaming_validation.rs` | **NEW** — #71–#77: JsonValidate, SchemaValidate, MagicBytes, SizeLimit, ChecksumVerify, Sha256Verify, NonEmpty | ~250 |
| `deriva-compute/src/builtins_streaming_text.rs` | **NEW** — #78–#85: Replace, Prefix, Suffix, LinePrefix, Grep, Sed, TruncateLines, CharsetConvert | ~350 |
| `deriva-compute/src/builtins_streaming_cas.rs` | **NEW** — #86–#92: CAddrEmbed, CAddrVerify, Diff, Patch, MerkleTree, ContentType, ChunkHash | ~300 |
| `deriva-compute/src/builtins_streaming_numeric.rs` | **NEW** — #93–#100: Sum, Average, BitwiseAnd/Or/Not, ByteSwap, Entropy, RollingHash | ~250 |
| `deriva-compute/src/builtins_streaming.rs` | Add `pub mod` declarations for all 10 new category modules + `streaming_helpers` | ~15 |
| `deriva-compute/src/lib.rs` | Re-export new modules | ~5 |
| `deriva-compute/src/function_registry.rs` | Register 80 new functions with `#[cfg(feature)]` gating | ~100 |
| `deriva-compute/Cargo.toml` | Add `extended-streaming` feature flag + 12 new optional dependencies | ~25 |
| `deriva-compute/tests/streaming_crypto.rs` | **NEW** — T21–T40 (20 tests) | ~400 |
| `deriva-compute/tests/streaming_encoding.rs` | **NEW** — T41–T62 (22 tests) | ~450 |
| `deriva-compute/tests/streaming_analytics.rs` | **NEW** — T63–T86 (24 tests) | ~500 |
| `deriva-compute/tests/streaming_compression.rs` | **NEW** — T87–T106 (20 tests) | ~400 |
| `deriva-compute/tests/streaming_flow.rs` | **NEW** — T107–T126 (20 tests) | ~450 |
| `deriva-compute/tests/streaming_validation.rs` | **NEW** — T127–T142 (16 tests) | ~350 |
| `deriva-compute/tests/streaming_text.rs` | **NEW** — T143–T162 (20 tests) | ~400 |
| `deriva-compute/tests/streaming_cas.rs` | **NEW** — T163–T176 (14 tests) | ~300 |
| `deriva-compute/tests/streaming_numeric.rs` | **NEW** — T177–T192 (16 tests) | ~350 |
| `deriva-compute/tests/streaming_existing.rs` | **NEW** — T1–T20 (20 tests for #1–#20) | ~400 |
| `deriva-compute/tests/streaming_integration.rs` | **NEW** — T193–T200 (8 cross-function pipeline tests) | ~250 |
| `deriva-compute/benches/streaming_functions.rs` | **NEW** — 7 benchmark groups with full Criterion.rs code | ~400 |

**Totals:** 26 files, ~7,170 lines (22 new files, 4 modified)

---

## 9. Dependency Changes

| Crate | Version | Feature | Required By | Status |
|-------|---------|---------|-------------|--------|
| `aes` | 0.8 | `extended-streaming` | #21, #22 (AES-CTR) | **NEW** |
| `ctr` | 0.9 | `extended-streaming` | #21, #22 (CTR mode) | **NEW** |
| `aes-gcm` | 0.10 | `extended-streaming` | #23, #24 (AEAD) | **NEW** |
| `hmac` | 0.12 | `extended-streaming` | #25 (HMAC-SHA256) | **NEW** |
| `md-5` | 0.10 | `extended-streaming` | #26 (MD5) | **NEW** |
| `blake3` | 1 | `extended-streaming` | #28 (BLAKE3) | **NEW** |
| `zstd` | 0.13 | `extended-streaming` | #51, #52 (Zstandard) | **NEW** |
| `lz4_flex` | 0.11 | `extended-streaming` | #53, #54 (LZ4) | **NEW** |
| `snap` | 1 | `extended-streaming` | #55, #56 (Snappy) | **NEW** |
| `brotli` | 6 | `extended-streaming` | #57, #58 (Brotli) | **NEW** |
| `regex` | 1 | `extended-streaming` | #29, #78, #82, #83 (text) | **NEW** |
| `data-encoding` | 2 | `extended-streaming` | #38, #39 (Base32) | **NEW** |
| `sha2` | 0.10 | default | #10, #27 (SHA-256/512) | Existing |
| `crc32fast` | 1 | default | #12 (CRC32) | Existing |
| `base64` | 0.22 | default | #5, #6 (Base64) | Existing |
| `flate2` | 1 | default | #8, #9 (zlib) | Existing |
| `serde_json` | 1 | default | #34–#36, #71 (JSON) | Existing |
| `tokio` | 1 | default | All (async runtime) | Existing |
| `bytes` | 1 | default | All (chunk data) | Existing |
| `criterion` | 0.5 | dev | Benchmarks | Existing |

12 new dependencies, all gated behind `feature = "extended-streaming"`.
Base binary size unchanged when feature is disabled.

---

## 10. Design Rationale

### 10.1 Why Per-Chunk Compression Instead of Whole-Stream?

| Approach | Pros | Cons |
|----------|------|------|
| Per-chunk (§2.14) | Simple; composable; each chunk independent; no cross-chunk state | Lower ratio (no cross-chunk dictionary); overhead per frame header |
| Whole-stream (§2.16) | Better ratio (dictionary spans chunks); single frame | Requires state across chunks; cannot random-access; decompressor must see all prior chunks |

Per-chunk is chosen for the streaming function library because:
1. Each chunk is a standalone compressed frame — decompressor needs no
   prior context. This enables range reads (§2.13) on compressed data.
2. Pipeline fusion (§2.11) can fuse per-chunk compressors with other
   maps. Whole-stream compressors cannot be fused.
3. Error isolation: a corrupt chunk affects only that chunk, not the
   entire stream.
4. Whole-stream compression is deferred to §2.16 (Format-Aware
   Functions) where it belongs — as a format-level concern, not a
   per-chunk transform.

### 10.2 Why Per-Chunk Crypto Instead of Whole-Stream?

CTR mode is naturally per-chunk: the counter increments per 16-byte
AES block, and the block position within the stream is deterministic
from the chunk index and chunk sizes. No cross-chunk buffering needed.

GCM per-chunk gives per-chunk authentication — each chunk's integrity
is independently verifiable. Whole-stream GCM would require buffering
the entire stream to verify the single tag at the end, defeating
streaming.

Trade-off: per-chunk GCM adds 16 bytes overhead per chunk (tag). At
64 KB chunks, this is 0.024% overhead — negligible.

### 10.3 Why Include Legacy Algorithms (MD5)?

MD5 is cryptographically broken for collision resistance but is still
required for:
1. S3 ETag computation — AWS S3 uses MD5 for single-part upload ETags.
   Any system interoperating with S3 needs MD5.
2. Legacy data lake compatibility — existing data catalogs may store
   MD5 checksums.
3. Non-security checksumming — MD5 is faster than SHA-256 and
   sufficient for accidental corruption detection.

The function is clearly documented as "legacy/compatibility only" and
not recommended for security purposes.

### 10.4 Why Feature-Gate Extended Functions?

The 12 new dependencies add ~1.4 MB to the compiled binary. For
deployments that only need the core 20 functions (e.g., embedded or
minimal containers), the `extended-streaming` feature flag keeps the
base binary lean.

Feature gating also:
- Reduces compile time for development builds (~30s saved)
- Avoids pulling C dependencies (`zstd` wraps `libzstd`) in pure-Rust
  environments
- Allows selective enablement (future: per-category features)

### 10.5 Why Boundary-Aware Map Instead of Always Buffering?

Text functions (#29, #32, #37, #78, #81–#85) need to handle patterns
split across chunk boundaries. Two approaches:

| Approach | Memory | Latency | Complexity |
|----------|--------|---------|-----------|
| `spawn_boundary_map` (leftover buffer) | O(longest_line) | Streaming — emits per chunk | Medium |
| `spawn_buffered` (collect all) | O(input_size) | Batch — emits after End | Low |

Boundary-aware map is chosen because:
1. Memory is bounded by longest line (~1 KB for logs), not input size
2. Output starts immediately — consumer sees results while input is
   still arriving
3. Compatible with §2.12 memory budget enforcement (small permits)

---

## 11. Observability Integration

Three per-function metrics, registered dynamically:

```rust
lazy_static! {
    /// Per-function invocation counter.
    static ref STREAMING_FN_CALLS: IntCounterVec = register_int_counter_vec!(
        "deriva_streaming_fn_calls_total",
        "Number of streaming function invocations",
        &["function"]
    ).unwrap();

    /// Per-function error counter.
    static ref STREAMING_FN_ERRORS: IntCounterVec = register_int_counter_vec!(
        "deriva_streaming_fn_errors_total",
        "Number of streaming function errors",
        &["function"]
    ).unwrap();

    /// Per-function execution duration (wall-clock from first chunk to End).
    static ref STREAMING_FN_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_streaming_fn_duration_seconds",
        "Streaming function execution duration",
        &["function"],
        vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 30.0, 60.0]
    ).unwrap();
}
```

Instrumentation is added in each helper (`spawn_map`, `spawn_accumulate`,
`spawn_boundary_map`, `spawn_buffered`, `spawn_passthrough`) so all
functions get metrics automatically:

```rust
fn spawn_map_instrumented(
    name: &'static str,
    rx: mpsc::Receiver<StreamChunk>,
    cap: usize,
    f: impl Fn(&[u8]) -> Result<Bytes, String> + Send + 'static,
) -> mpsc::Receiver<StreamChunk> {
    STREAMING_FN_CALLS.with_label_values(&[name]).inc();
    let timer = STREAMING_FN_DURATION.with_label_values(&[name]).start_timer();
    let error_counter = STREAMING_FN_ERRORS.with_label_values(&[name]);
    // ... spawn task, on Error: error_counter.inc(), on End: timer.observe_duration()
}
```

Dashboard queries:

```promql
# Top 10 most-called streaming functions
topk(10, rate(deriva_streaming_fn_calls_total[5m]))

# Error rate per function (should be < 1%)
rate(deriva_streaming_fn_errors_total[5m])
  / rate(deriva_streaming_fn_calls_total[5m])

# p99 execution duration per function
histogram_quantile(0.99,
  rate(deriva_streaming_fn_duration_seconds_bucket[5m]))

# Slowest functions (p99 > 1s)
histogram_quantile(0.99,
  rate(deriva_streaming_fn_duration_seconds_bucket[5m])) > 1

# Function usage distribution (which functions are actually used?)
sort_desc(sum by (function) (deriva_streaming_fn_calls_total))
```

---

## 12. Checklist

**Helpers & Infrastructure:**
- [ ] Create `streaming_helpers.rs` with `spawn_boundary_map()`
- [ ] Create `streaming_helpers.rs` with `spawn_buffered()`
- [ ] Create `streaming_helpers.rs` with `spawn_passthrough()` + `PassAction`
- [ ] Add `extended-streaming` feature flag to `Cargo.toml`
- [ ] Add 12 optional dependencies gated behind feature
- [ ] Add instrumented helper wrappers for observability

**Crypto (#21–#29) — 9 functions:**
- [ ] Implement `StreamingEncrypt` (AES-256-CTR)
- [ ] Implement `StreamingDecrypt` (AES-256-CTR)
- [ ] Implement `StreamingAeadEncrypt` (AES-256-GCM)
- [ ] Implement `StreamingAeadDecrypt` (AES-256-GCM)
- [ ] Implement `StreamingHmacSha256`
- [ ] Implement `StreamingMd5`
- [ ] Implement `StreamingSha512`
- [ ] Implement `StreamingBlake3`
- [ ] Implement `StreamingRedact`

**Encoding (#30–#39) — 10 functions:**
- [ ] Implement `StreamingHexEncode`
- [ ] Implement `StreamingHexDecode`
- [ ] Implement `StreamingUtf8Validate`
- [ ] Implement `StreamingLineEnding`
- [ ] Implement `StreamingJsonPrettyPrint`
- [ ] Implement `StreamingJsonMinify`
- [ ] Implement `StreamingJsonLines`
- [ ] Implement `StreamingCsvToJson`
- [ ] Implement `StreamingBase32Encode`
- [ ] Implement `StreamingBase32Decode`

**Analytics (#40–#50) — 11 functions:**
- [ ] Implement `StreamingFilter`
- [ ] Implement `StreamingLineCount`
- [ ] Implement `StreamingWordCount`
- [ ] Implement `StreamingMinMax`
- [ ] Implement `StreamingHistogram`
- [ ] Implement `StreamingSample`
- [ ] Implement `StreamingHead`
- [ ] Implement `StreamingTail`
- [ ] Implement `StreamingDeduplicate`
- [ ] Implement `StreamingSort`
- [ ] Implement `StreamingUnique`

**Compression (#51–#60) — 10 functions:**
- [ ] Implement `StreamingZstdCompress`
- [ ] Implement `StreamingZstdDecompress`
- [ ] Implement `StreamingLz4Compress`
- [ ] Implement `StreamingLz4Decompress`
- [ ] Implement `StreamingSnappyCompress`
- [ ] Implement `StreamingSnappyDecompress`
- [ ] Implement `StreamingBrotliCompress`
- [ ] Implement `StreamingBrotliDecompress`
- [ ] Implement `StreamingPad`
- [ ] Implement `StreamingTrim`

**Flow Control (#61–#70) — 10 functions:**
- [ ] Implement `StreamingRateLimit`
- [ ] Implement `StreamingDelay`
- [ ] Implement `StreamingTimeout`
- [ ] Implement `StreamingRetry`
- [ ] Implement `StreamingTee`
- [ ] Implement `StreamingMerge`
- [ ] Implement `StreamingBroadcast`
- [ ] Implement `StreamingPartition`
- [ ] Implement `StreamingBatch`
- [ ] Implement `StreamingDebounce`

**Validation (#71–#77) — 7 functions:**
- [ ] Implement `StreamingJsonValidate`
- [ ] Implement `StreamingSchemaValidate`
- [ ] Implement `StreamingMagicBytes`
- [ ] Implement `StreamingSizeLimit`
- [ ] Implement `StreamingChecksumVerify`
- [ ] Implement `StreamingSha256Verify`
- [ ] Implement `StreamingNonEmpty`

**Text (#78–#85) — 8 functions:**
- [ ] Implement `StreamingReplace`
- [ ] Implement `StreamingPrefix`
- [ ] Implement `StreamingSuffix`
- [ ] Implement `StreamingLinePrefix`
- [ ] Implement `StreamingGrep`
- [ ] Implement `StreamingSed`
- [ ] Implement `StreamingTruncateLines`
- [ ] Implement `StreamingCharsetConvert`

**CAS (#86–#92) — 7 functions:**
- [ ] Implement `StreamingCAddrEmbed`
- [ ] Implement `StreamingCAddrVerify`
- [ ] Implement `StreamingDiff`
- [ ] Implement `StreamingPatch`
- [ ] Implement `StreamingMerkleTree`
- [ ] Implement `StreamingContentType`
- [ ] Implement `StreamingChunkHash`

**Numeric (#93–#100) — 8 functions:**
- [ ] Implement `StreamingSum`
- [ ] Implement `StreamingAverage`
- [ ] Implement `StreamingBitwiseAnd`
- [ ] Implement `StreamingBitwiseOr`
- [ ] Implement `StreamingBitwiseNot`
- [ ] Implement `StreamingByteSwap`
- [ ] Implement `StreamingEntropy`
- [ ] Implement `StreamingRollingHash`

**Registration & Fusibility:**
- [ ] Register all 80 functions in `FunctionRegistry` with `#[cfg(feature)]`
- [ ] Add `is_fusible() -> true` for 12 fusible functions (#30, #31, #33, #38, #39, #59, #60, #92, #95–#98)

**Tests (200 total):**
- [ ] T1–T20: existing functions #1–#20
- [ ] T21–T40: crypto #21–#29
- [ ] T41–T62: encoding #30–#39
- [ ] T63–T86: analytics #40–#50
- [ ] T87–T106: compression #51–#60
- [ ] T107–T126: flow control #61–#70
- [ ] T127–T142: validation #71–#77
- [ ] T143–T162: text #78–#85
- [ ] T163–T176: CAS #86–#92
- [ ] T177–T192: numeric #93–#100
- [ ] T193–T200: cross-function integration pipelines

**Benchmarks (7 groups):**
- [ ] Benchmark 1: compression codec throughput (8 codecs × 3 sizes)
- [ ] Benchmark 2: hash algorithm throughput (6 algorithms × 10 MB)
- [ ] Benchmark 3: crypto throughput (5 operations × 10 MB)
- [ ] Benchmark 4: text processing throughput (6 functions × log data)
- [ ] Benchmark 5: roundtrip pipeline throughput (4 codec+AES roundtrips)
- [ ] Benchmark 6: flow control overhead (Tee 1–8, Batch 1–16)
- [ ] Benchmark 7: fusible chain scaling (depths 2–16)

**Observability:**
- [ ] Add `deriva_streaming_fn_calls_total` counter vec
- [ ] Add `deriva_streaming_fn_errors_total` counter vec
- [ ] Add `deriva_streaming_fn_duration_seconds` histogram vec
- [ ] Instrument all 5 helpers with automatic metrics

**Validation:**
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] `cargo clippy --workspace --features extended-streaming -- -D warnings` clean
- [ ] All existing §2.7–§2.13 tests still pass
- [ ] All 200 new tests pass
- [ ] All 7 benchmark groups run without error
- [ ] Commit and push
