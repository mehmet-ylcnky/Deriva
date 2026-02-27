# §2.15 Batch Function Library Expansion

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization (for registry integration)
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 4–5 days (96 functions across 10 categories)

---

## 1. Problem Statement

### 1.1 Current State

TODO — 4 batch functions: `IdentityFn`, `ConcatFn`, `UppercaseFn`, `RepeatFn`; covers only minimum for initial CAS pipeline testing

### 1.2 Coverage Gaps

TODO — no compression codecs, no cryptography, no hashing beyond CAddr, no text processing, no validation, no format conversion, no slicing/restructuring, no CAS-specific operations

### 1.3 Goal

TODO — grow to 100 batch functions; achieve parity with streaming library where applicable; add batch-only functions that benefit from full-input access (sort, global search-replace, whole-file compression)

### 1.4 Batch vs Streaming Parity Matrix

TODO — table: which patterns need both (transforms, compression, hashing), streaming-only (flow control), batch-only (global sort/reverse), batch-preferred (format conversion, validation)

---

## 2. Design

### 2.1 Implementation Pattern

TODO — all functions implement `ComputeFunction` trait: `id() -> FunctionId`, `execute(&[Bytes], params) -> Result<Bytes>`, `estimated_cost() -> u64`

### 2.2 Category Overview

TODO — table of 10 categories with function counts:
- Transforms (16)
- Compression (10)
- Cryptography & Hashing (11)
- Accumulators / Analytics (8)
- Combiners (6)
- Slicing & Restructuring (10)
- Text Processing (10)
- Validation & Integrity (8)
- Format Conversion (8)
- CAS-Specific Operations (7)
- Batch-Only (2)

### 2.3 Dependency Strategy

TODO — shares most dependencies with §2.14 streaming library; additional: `serde_json`, `serde_yaml`, `toml`, `rand` (for shuffle)

### 2.4 Implementation Priority

TODO — 3 tiers: high (streaming parity, low complexity), medium (common operations), low (niche/high complexity)

---

## 3. Function Specifications

### 3.1 Transforms (16 functions)

#### 3.1.1 LowercaseFn (#5)
TODO — UTF-8 lowercase; `lowercase@1.0.0`; Low

#### 3.1.2 ReverseFn (#6)
TODO — reverse all bytes; `reverse@1.0.0`; Low

#### 3.1.3 Base64EncodeFn (#7)
TODO — base64 encode; `base64_encode@1.0.0`; Low

#### 3.1.4 Base64DecodeFn (#8)
TODO — base64 decode; `base64_decode@1.0.0`; Low

#### 3.1.5 HexEncodeFn (#9)
TODO — hex encode; `hex_encode@1.0.0`; Low

#### 3.1.6 HexDecodeFn (#10)
TODO — hex decode; `hex_decode@1.0.0`; Low

#### 3.1.7 Base32EncodeFn (#11)
TODO — base32 encode; `base32_encode@1.0.0`; Low

#### 3.1.8 Base32DecodeFn (#12)
TODO — base32 decode; `base32_decode@1.0.0`; Low

#### 3.1.9 XorFn (#13)
TODO — XOR each byte with key; params: `key`; `xor@1.0.0`; Low

#### 3.1.10 BitwiseAndFn (#14)
TODO — AND with mask; params: `mask`; `bitwise_and@1.0.0`; Low

#### 3.1.11 BitwiseOrFn (#15)
TODO — OR with mask; params: `mask`; `bitwise_or@1.0.0`; Low

#### 3.1.12 BitwiseNotFn (#16)
TODO — complement each byte; `bitwise_not@1.0.0`; Low

#### 3.1.13 ByteSwapFn (#17)
TODO — swap byte order within words; params: `word_size` (2/4/8); `byte_swap@1.0.0`; Low

#### 3.1.14 TrimFn (#18)
TODO — strip leading/trailing whitespace; `trim@1.0.0`; Low

#### 3.1.15 PadFn (#19)
TODO — pad to fixed size; params: `block_size`, `padding_byte`; `pad@1.0.0`; Low

#### 3.1.16 LineEndingFn (#20)
TODO — convert `\r\n` ↔ `\n`; params: `target`; `line_ending@1.0.0`; Low

### 3.2 Compression (10 functions)

#### 3.2.1 CompressFn (#21)
TODO — zlib whole-input compress (better ratio than per-chunk streaming); `compress@1.0.0`; Low

#### 3.2.2 DecompressFn (#22)
TODO — zlib whole-input decompress; `decompress@1.0.0`; Low

#### 3.2.3 ZstdCompressFn (#23)
TODO — Zstandard; params: `level` (1–22); `zstd_compress@1.0.0`; Medium

#### 3.2.4 ZstdDecompressFn (#24)
TODO — Zstandard decompress; `zstd_decompress@1.0.0`; Medium

#### 3.2.5 Lz4CompressFn (#25)
TODO — LZ4 fastest; `lz4_compress@1.0.0`; Low

#### 3.2.6 Lz4DecompressFn (#26)
TODO — LZ4 decompress; `lz4_decompress@1.0.0`; Low

#### 3.2.7 SnappyCompressFn (#27)
TODO — Snappy; `snappy_compress@1.0.0`; Low

#### 3.2.8 SnappyDecompressFn (#28)
TODO — Snappy decompress; `snappy_decompress@1.0.0`; Low

#### 3.2.9 BrotliCompressFn (#29)
TODO — Brotli; params: `quality` (0–11); `brotli_compress@1.0.0`; Medium

#### 3.2.10 BrotliDecompressFn (#30)
TODO — Brotli decompress; `brotli_decompress@1.0.0`; Medium

### 3.3 Cryptography & Hashing (11 functions)

#### 3.3.1 Sha256Fn (#31)
TODO — SHA-256; returns 32 bytes; `sha256@1.0.0`; Low

#### 3.3.2 Sha512Fn (#32)
TODO — SHA-512; returns 64 bytes; `sha512@1.0.0`; Low

#### 3.3.3 Md5Fn (#33)
TODO — MD5 legacy/S3 ETag; returns 16 bytes; `md5@1.0.0`; Low

#### 3.3.4 Blake3Fn (#34)
TODO — BLAKE3 fastest; returns 32 bytes; `blake3@1.0.0`; Low

#### 3.3.5 HmacSha256Fn (#35)
TODO — HMAC-SHA256; params: `key`; returns 32 bytes; `hmac_sha256@1.0.0`; Low

#### 3.3.6 Crc32Fn (#36)
TODO — CRC32; returns 4 bytes; `crc32@1.0.0`; Low

#### 3.3.7 EncryptFn (#37)
TODO — AES-256-CTR; params: `key`, `nonce`; `encrypt@1.0.0`; Medium

#### 3.3.8 DecryptFn (#38)
TODO — AES-256-CTR decrypt; params: `key`, `nonce`; `decrypt@1.0.0`; Medium

#### 3.3.9 AeadEncryptFn (#39)
TODO — AES-256-GCM; appends 16-byte tag; `aead_encrypt@1.0.0`; Medium

#### 3.3.10 AeadDecryptFn (#40)
TODO — AES-256-GCM decrypt; verifies tag; `aead_decrypt@1.0.0`; Medium

#### 3.3.11 RedactFn (#41)
TODO — regex PII redaction; params: `patterns[]`; `redact@1.0.0`; Medium

### 3.4 Accumulators / Analytics (8 functions)

#### 3.4.1 ByteCountFn (#42)
TODO — return input length as u64 big-endian; `byte_count@1.0.0`; Low

#### 3.4.2 LineCountFn (#43)
TODO — count `\n`; return u64; `line_count@1.0.0`; Low

#### 3.4.3 WordCountFn (#44)
TODO — count whitespace-delimited words; return u64; `word_count@1.0.0`; Low

#### 3.4.4 HistogramFn (#45)
TODO — 256-bucket byte frequency; returns 2 KB; `histogram@1.0.0`; Low

#### 3.4.5 EntropyFn (#46)
TODO — Shannon entropy; returns f64 (8 bytes); `entropy@1.0.0`; Low

#### 3.4.6 MinMaxFn (#47)
TODO — min/max byte values; returns [min, max]; `min_max@1.0.0`; Low

#### 3.4.7 SumFn (#48)
TODO — parse newline-delimited decimals, sum; return string; `sum@1.0.0`; Low

#### 3.4.8 AverageFn (#49)
TODO — parse decimals, average; return string; `average@1.0.0`; Low

### 3.5 Combiners (6 functions)

#### 3.5.1 InterleaveFn (#50)
TODO — round-robin bytes from inputs in blocks; params: `block_size`; `interleave@1.0.0`; Medium

#### 3.5.2 ZipConcatFn (#51)
TODO — pairwise concatenation of 2 inputs; `zip_concat@1.0.0`; Low

#### 3.5.3 DiffFn (#52)
TODO — binary diff between 2 inputs; returns edit script; `diff@1.0.0`; High

#### 3.5.4 PatchFn (#53)
TODO — apply binary patch (input 0 = base, input 1 = patch); `patch@1.0.0`; High

#### 3.5.5 MergeSortedFn (#54)
TODO — merge N sorted line-delimited inputs; `merge_sorted@1.0.0`; Medium

#### 3.5.6 SelectFn (#55)
TODO — return input at index N; params: `index`; `select@1.0.0`; Low

### 3.6 Slicing & Restructuring (10 functions)

#### 3.6.1 TakeFn (#56)
TODO — first N bytes; params: `bytes`; `take@1.0.0`; Low

#### 3.6.2 SkipFn (#57)
TODO — all bytes after offset N; params: `bytes`; `skip@1.0.0`; Low

#### 3.6.3 SliceFn (#58)
TODO — bytes [offset..offset+length]; params: `offset`, `length`; `slice@1.0.0`; Low

#### 3.6.4 SortFn (#59)
TODO — sort lines lexicographically; `sort@1.0.0`; Medium

#### 3.6.5 UniqueFn (#60)
TODO — deduplicate sorted lines; `unique@1.0.0`; Medium

#### 3.6.6 SortUniqueFn (#61)
TODO — sort + deduplicate; `sort_unique@1.0.0`; Medium

#### 3.6.7 ShuffleFn (#62)
TODO — randomly reorder lines; params: `seed` (deterministic); `shuffle@1.0.0`; Medium

#### 3.6.8 HeadFn (#63)
TODO — first N lines; params: `lines`; `head@1.0.0`; Low

#### 3.6.9 TailFn (#64)
TODO — last N lines; params: `lines`; `tail@1.0.0`; Low

#### 3.6.10 SampleFn (#65)
TODO — reservoir sample N lines; params: `lines`, `seed`; `sample@1.0.0`; Medium

### 3.7 Text Processing (10 functions)

#### 3.7.1 ReplaceFn (#66)
TODO — byte pattern find-and-replace; params: `find`, `replace`; `replace@1.0.0`; Medium

#### 3.7.2 RegexReplaceFn (#67)
TODO — regex find-and-replace; params: `pattern`, `replacement`; `regex_replace@1.0.0`; Medium

#### 3.7.3 GrepFn (#68)
TODO — keep lines matching regex; params: `pattern`; `grep@1.0.0`; Medium

#### 3.7.4 GrepInvertFn (#69)
TODO — drop lines matching regex; params: `pattern`; `grep_invert@1.0.0`; Medium

#### 3.7.5 PrefixFn (#70)
TODO — prepend bytes; params: `prefix`; `prefix@1.0.0`; Low

#### 3.7.6 SuffixFn (#71)
TODO — append bytes; params: `suffix`; `suffix@1.0.0`; Low

#### 3.7.7 LinePrefixFn (#72)
TODO — prepend string to each line; params: `prefix`; `line_prefix@1.0.0`; Medium

#### 3.7.8 LineNumberFn (#73)
TODO — prepend line numbers; `line_number@1.0.0`; Medium

#### 3.7.9 TruncateLinesFn (#74)
TODO — truncate each line to max N bytes; params: `max_line_bytes`; `truncate_lines@1.0.0`; Low

#### 3.7.10 CharsetConvertFn (#75)
TODO — convert encoding; params: `from`, `to`; `charset_convert@1.0.0`; Medium

### 3.8 Validation & Integrity (8 functions)

#### 3.8.1 Utf8ValidateFn (#76)
TODO — return unchanged if valid UTF-8, error otherwise; `utf8_validate@1.0.0`; Low

#### 3.8.2 JsonValidateFn (#77)
TODO — return unchanged if valid JSON, error otherwise; `json_validate@1.0.0`; Medium

#### 3.8.3 SchemaValidateFn (#78)
TODO — validate JSON against JSON Schema; params: `schema`; `schema_validate@1.0.0`; High

#### 3.8.4 MagicBytesFn (#79)
TODO — verify input starts with expected bytes; params: `expected`; `magic_bytes@1.0.0`; Low

#### 3.8.5 SizeLimitFn (#80)
TODO — error if input exceeds limit; params: `max_bytes`; `size_limit@1.0.0`; Low

#### 3.8.6 NonEmptyFn (#81)
TODO — error if input is 0 bytes; `non_empty@1.0.0`; Low

#### 3.8.7 Sha256VerifyFn (#82)
TODO — compute SHA-256, error if mismatch; params: `expected_hash`; `sha256_verify@1.0.0`; Low

#### 3.8.8 Crc32VerifyFn (#83)
TODO — compute CRC32, error if mismatch; params: `expected_crc32`; `crc32_verify@1.0.0`; Low

### 3.9 Format Conversion (8 functions)

#### 3.9.1 JsonPrettyPrintFn (#84)
TODO — parse JSON, re-emit pretty-printed; `json_pretty@1.0.0`; Medium

#### 3.9.2 JsonMinifyFn (#85)
TODO — parse JSON, re-emit compact; `json_minify@1.0.0`; Medium

#### 3.9.3 CsvToJsonFn (#86)
TODO — parse CSV, emit JSON array of objects; `csv_to_json@1.0.0`; High

#### 3.9.4 JsonToCsvFn (#87)
TODO — parse JSON array, emit CSV; `json_to_csv@1.0.0`; High

#### 3.9.5 JsonLinesFn (#88)
TODO — ensure newline-terminated lines; `json_lines@1.0.0`; Low

#### 3.9.6 YamlToJsonFn (#89)
TODO — parse YAML, emit JSON; `yaml_to_json@1.0.0`; Medium

#### 3.9.7 JsonToYamlFn (#90)
TODO — parse JSON, emit YAML; `json_to_yaml@1.0.0`; Medium

#### 3.9.8 TomlToJsonFn (#91)
TODO — parse TOML, emit JSON; `toml_to_json@1.0.0`; Medium

### 3.10 CAS-Specific Operations (7 functions)

#### 3.10.1 CAddrComputeFn (#92)
TODO — compute CAddr of input; return address bytes; `caddr_compute@1.0.0`; Low

#### 3.10.2 CAddrVerifyFn (#93)
TODO — compute CAddr, error if mismatch; params: `expected_caddr`; `caddr_verify@1.0.0`; Medium

#### 3.10.3 CAddrEmbedFn (#94)
TODO — append CAddr as trailing metadata; `caddr_embed@1.0.0`; Medium

#### 3.10.4 MerkleRootFn (#95)
TODO — Merkle tree over fixed blocks, return 32-byte root; params: `block_size`; `merkle_root@1.0.0`; High

#### 3.10.5 ContentTypeFn (#96)
TODO — detect MIME from magic bytes, prepend metadata header; `content_type@1.0.0`; Medium

#### 3.10.6 ChunkHashFn (#97)
TODO — split into blocks, append SHA-256 per block; params: `block_size`; `chunk_hash@1.0.0`; Medium

#### 3.10.7 DedupAnalyzeFn (#98)
TODO — rolling hash fingerprints for content-defined chunking; params: `window_size`; `dedup_analyze@1.0.0`; High

### 3.11 Batch-Only (2 functions)

#### 3.11.1 GlobalSortFn (#99)
TODO — sort all bytes (not lines); for binary dedup analysis; `global_sort@1.0.0`; Medium

#### 3.11.2 GlobalReverseFn (#100)
TODO — reverse entire byte sequence (streaming Reverse only reverses per-chunk); `global_reverse@1.0.0`; Low

---

## 4. Implementation Strategy

### 4.1 Estimated Cost Heuristics

TODO — each function provides `estimated_cost()` for pipeline optimizer; heuristic: Low complexity = 1, Medium = 10, High = 100; compression/crypto scale with input size

### 4.2 Shared Utility Functions

TODO — common helpers: `parse_lines()`, `join_lines()`, `parse_u64_param()`, `parse_byte_param()`; reduce boilerplate across text/analytics functions

### 4.3 Error Handling Convention

TODO — all functions return `ComputeError` with descriptive message; invalid params → `ComputeError::InvalidParams`; invalid input → `ComputeError::InvalidInput`

---

## 5. Test Specification

### 5.1 Per-Function Unit Tests

TODO — each function: basic correctness, empty input, error cases; ~3 tests per function = ~288 tests

### 5.2 Roundtrip Tests

TODO — encode/decode pairs: Base64, Hex, Base32, Encrypt/Decrypt, AEAD, all 5 compression codecs; ~10 roundtrip tests

### 5.3 Streaming Parity Tests

TODO — for functions with both batch and streaming implementations, verify identical output for same input; ~40 parity tests

### 5.4 Pipeline Composition Tests

TODO — multi-function batch pipelines; ~5 integration tests

### 5.5 Benchmark Suite

TODO — batch vs streaming throughput for shared functions at various input sizes (1 KB, 1 MB, 100 MB)

---

## 6. Edge Cases & Error Handling

TODO — table: empty input for all categories, invalid params (bad regex, out-of-range level), input too large for memory, multi-input with mismatched sizes, deterministic shuffle with same seed

---

## 7. Performance Analysis

### 7.1 Batch Advantage Over Streaming

TODO — whole-input compression gets 5–15% better ratio than per-chunk; whole-input sort is correct (streaming sort is per-chunk only); batch hashing is simpler (no state management)

### 7.2 Memory Profile

TODO — all batch functions are O(input) memory; acceptable for values <1 GB; larger values should use streaming path

---

## 8. Files Changed

TODO — table: `builtins.rs` (extend), new test files, `Cargo.toml` dependencies

---

## 9. Dependency Changes

TODO — table: shares most with §2.14; additional: `serde_json` 1, `serde_yaml` 0.9, `toml` 0.8, `rand` 0.8 (for shuffle), `csv` 1 (for CSV↔JSON), `encoding_rs` 0.8 (for charset), `jsonschema` 0.18

---

## 10. Design Rationale

### 10.1 Why Not Just Use Streaming for Everything?

TODO — batch is simpler to implement, test, and reason about; many functions (sort, aggregate, format conversion) need full input anyway; batch gets better compression ratios

### 10.2 Why Separate FunctionId Versioning?

TODO — `name@version` allows breaking changes without breaking existing recipes; old recipes reference old version, new recipes use new version; both coexist in registry

### 10.3 Why Include Format Conversion Here Instead of §2.16?

TODO — simple conversions (JSON↔YAML, JSON↔CSV) are general-purpose; §2.16 covers complex format-specific operations (Parquet, Avro, HDF5)

---

## 11. Observability Integration

TODO — per-function execution counter `deriva_batch_fn_calls_total{function="..."}`, per-function error counter, per-function duration histogram, per-function input_size histogram

---

## 12. Checklist

- [ ] Implement transforms (#5–#20) — 16 functions
- [ ] Implement compression (#21–#30) — 10 functions
- [ ] Implement crypto & hashing (#31–#41) — 11 functions
- [ ] Implement accumulators (#42–#49) — 8 functions
- [ ] Implement combiners (#50–#55) — 6 functions
- [ ] Implement slicing (#56–#65) — 10 functions
- [ ] Implement text processing (#66–#75) — 10 functions
- [ ] Implement validation (#76–#83) — 8 functions
- [ ] Implement format conversion (#84–#91) — 8 functions
- [ ] Implement CAS-specific (#92–#98) — 7 functions
- [ ] Implement batch-only (#99–#100) — 2 functions
- [ ] Register all 96 new functions in `FunctionRegistry`
- [ ] Unit tests (~288 tests, ~3 per function)
- [ ] Roundtrip tests (~10 tests)
- [ ] Streaming parity tests (~40 tests)
- [ ] Pipeline composition tests (~5 tests)
- [ ] Benchmark: batch vs streaming throughput
- [ ] Add per-function observability metrics
- [ ] Update `Cargo.toml` with new dependencies
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing tests still pass
- [ ] Commit and push
