# Implementation Plan: Streaming Function Library Expansion

## Overview

Phase 2.14 expands the streaming function library from 20 to 100 functions across 13 categories. The implementation is structured around: (1) adding the `spawn_passthrough` helper pattern and `PassAction` enum to complete the helper infrastructure, (2) verifying/completing function implementations across all 9 new categories, (3) ensuring feature gating and fusibility declarations are correct, (4) writing comprehensive tests including property-based tests for round-trip properties and chunk-split invariance.

## Tasks

- [ ] 1. Implement spawn_passthrough helper and PassAction enum
  - [ ] 1.1 Add PassAction enum and spawn_passthrough helper to `crates/deriva-compute/src/builtins_streaming/core.rs`
    - Define `PassAction` enum with variants: `Forward`, `Replace(Bytes)`, `Drop`, `Error(String)`
    - Implement `spawn_passthrough<S: Send + 'static>` that accepts `rx`, `cap`, `init`, `on_chunk`, and `on_end` callbacks
    - Handle upstream Error propagation and downstream channel close per design contracts
    - On `End`: invoke `on_end`, emit returned bytes as final Data chunk, then emit End
    - _Requirements: 10.5, 10.6, 10.7, 10.8_

  - [ ]* 1.2 Write unit tests for spawn_passthrough helper
    - Test Forward action passes chunks unchanged
    - Test Replace action substitutes chunk content
    - Test Drop action suppresses chunks
    - Test Error action emits error and terminates
    - Test on_end callback emits final data chunk
    - Test upstream Error propagation
    - Test downstream close terminates silently
    - _Requirements: 10.5, 10.6, 10.7, 10.8_

- [ ] 2. Implement and verify compression codec functions
  - [ ] 2.1 Verify compression functions in `crates/deriva-compute/src/builtins_streaming/compression.rs`
    - Verify StreamingZstdCompress/Decompress, StreamingLz4Compress/Decompress, StreamingSnappyCompress/Decompress, StreamingBrotliCompress/Decompress are complete
    - Verify `level` parameter validation for zstd (1–22), `quality` for brotli (0–11)
    - Verify error messages include codec name and error detail on corrupt data
    - Verify StreamingPad and StreamingTrim with is_fusible() = true
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9, 12.1_

  - [ ]* 2.2 Write property test for compression round-trip
    - **Property 1: Compression round-trip**
    - For arbitrary byte sequences and all 4 codec pairs (zstd, lz4, snappy, brotli), compress then decompress == original
    - **Validates: Requirements 1.10**

  - [ ]* 2.3 Write unit tests for compression error handling
    - Test corrupt data produces Error chunk with codec name
    - Test out-of-range level/quality parameters
    - Test empty input handling for each codec
    - _Requirements: 1.9, 13.2, 14.5_

- [ ] 3. Implement and verify cryptography functions
  - [ ] 3.1 Verify crypto functions in `crates/deriva-compute/src/builtins_streaming/crypto.rs`
    - Verify StreamingEncrypt/Decrypt maintain AES-256-CTR counter state across chunks
    - Verify StreamingAeadEncrypt appends 16-byte tag and StreamingAeadDecrypt strips/verifies it
    - Verify per-chunk nonce derivation: `nonce_prefix || chunk_index_u32`
    - Verify StreamingHmacSha256, StreamingMd5, StreamingSha512, StreamingBlake3 emit correct-size digests
    - Verify StreamingRedact replaces comma-separated regex patterns
    - Verify key/nonce hex validation with proper error messages
    - _Requirements: 2.1, 2.2, 2.4, 2.5, 2.6, 2.7, 2.8, 2.9, 2.10, 2.11, 2.12_

  - [ ]* 3.2 Write property test for AES-CTR encryption round-trip
    - **Property 2: Encryption round-trip (AES-CTR)**
    - For arbitrary bytes, random 32-byte key and 16-byte nonce, encrypt then decrypt == original
    - **Validates: Requirements 2.3**

  - [ ]* 3.3 Write property test for AEAD encryption round-trip
    - **Property 3: AEAD round-trip (AES-GCM)**
    - For arbitrary bytes, random key and nonce prefix, aead_encrypt then aead_decrypt == original
    - **Validates: Requirements 2.4, 2.5**

  - [ ]* 3.4 Write unit tests for cryptography error conditions
    - Test invalid hex key produces error with expected length info
    - Test invalid nonce produces error with expected nonce length
    - Test AEAD tag verification failure emits Error with chunk index
    - Test empty input handling for all hash/digest functions
    - _Requirements: 2.6, 2.11, 2.12, 13.2_

- [ ] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 5. Implement and verify text processing functions
  - [ ] 5.1 Verify text processing functions in `crates/deriva-compute/src/builtins_streaming/text.rs`
    - Verify StreamingGrep filters lines by regex with `invert` option
    - Verify StreamingSed applies regex find-and-replace with capture group references
    - Verify StreamingReplace does literal string replacement per line
    - Verify StreamingLinePrefix prepends prefix after newlines and at start
    - Verify StreamingTruncateLines truncates to `max_line_bytes` preserving newline
    - Verify StreamingCharsetConvert uses encoding_rs for charset conversion
    - Verify StreamingRedact replaces patterns with `[REDACTED]`
    - Verify all line-oriented functions use spawn_boundary_map for partial line buffering
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7, 3.8, 3.9, 3.10_

  - [ ]* 5.2 Write property test for boundary-map line preservation
    - **Property 6: Boundary-map line preservation**
    - For arbitrary text with newlines split at random chunk boundaries, multi-chunk result == single-chunk result
    - **Validates: Requirements 3.8, 10.1, 10.2**

  - [ ]* 5.3 Write unit tests for text processing
    - Test grep with matching/non-matching lines and invert mode
    - Test sed with capture group replacement
    - Test invalid regex pattern produces Error chunk
    - Test chunk boundary handling for split lines
    - _Requirements: 3.1, 3.2, 3.3, 3.9_

- [ ] 6. Implement and verify validation functions
  - [ ] 6.1 Verify validation functions in `crates/deriva-compute/src/builtins_streaming/validation.rs`
    - Verify StreamingJsonValidate buffers then validates JSON, emits original on success
    - Verify StreamingSchemaValidate validates against `schema` parameter
    - Verify StreamingMagicBytes checks first chunk prefix against `expected` hex
    - Verify StreamingSizeLimit tracks cumulative bytes and errors on `max_bytes` exceeded
    - Verify StreamingChecksumVerify computes CRC32 and compares to `expected_crc32`
    - Verify StreamingSha256Verify computes SHA-256 and compares to `expected_hash`
    - Verify StreamingNonEmpty errors on End with zero Data chunks
    - Verify accumulator verifiers forward Data chunks during accumulation
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 4.7, 4.8, 4.9, 4.10_

  - [ ]* 6.2 Write unit tests for validation functions
    - Test valid JSON passes through, invalid JSON emits error with line/col
    - Test magic bytes match/mismatch
    - Test size limit exceeded mid-stream
    - Test checksum match/mismatch
    - Test non-empty passes with data, errors on immediate End
    - _Requirements: 4.1, 4.2, 4.4, 4.5, 4.6, 4.7, 4.10_

- [ ] 7. Implement and verify analytics functions
  - [ ] 7.1 Verify analytics functions in `crates/deriva-compute/src/builtins_streaming/analytics.rs`
    - Verify StreamingLineCount emits u64 big-endian count of newlines
    - Verify StreamingWordCount tracks word boundaries across chunks
    - Verify StreamingMinMax emits 2-byte min/max
    - Verify StreamingHistogram emits 2048-byte frequency table
    - Verify StreamingSample emits every Nth chunk
    - Verify StreamingHead forwards first N chunks then emits End
    - Verify StreamingTail uses ring buffer for last N chunks
    - Verify StreamingDeduplicate drops chunks with duplicate xxhash64
    - Verify StreamingSort and StreamingUnique buffer and re-emit in sorted order
    - Verify StreamingFilter supports "non_empty", "contains:PATTERN", "min_size:N"
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 5.7, 5.8, 5.9, 5.10, 5.11_

  - [ ]* 7.2 Write property test for accumulator chunk-split invariance
    - **Property 5: Accumulator chunk-split invariance**
    - For LineCount, WordCount, MinMax, Histogram, Entropy, Sum, Average — single chunk result == multi-chunk result for same data
    - **Validates: Requirements 15.5**

  - [ ]* 7.3 Write unit tests for analytics functions
    - Test line count, word count with known inputs
    - Test head/tail with various N values
    - Test dedup drops duplicate chunks
    - Test sample rate selection
    - Test filter predicates
    - _Requirements: 5.1, 5.2, 5.5, 5.6, 5.7, 5.8, 5.11_

- [ ] 8. Implement and verify flow control functions
  - [ ] 8.1 Verify flow control functions in `crates/deriva-compute/src/builtins_streaming/flow.rs`
    - Verify StreamingRateLimit throttles to `bytes_per_sec`
    - Verify StreamingDelay waits `delay_ms` before forwarding
    - Verify StreamingTimeout emits Error if no chunk within `timeout_ms`
    - Verify StreamingRetry restarts upstream up to `max_retries` times
    - Verify StreamingTee clones to N output receivers
    - Verify StreamingMerge emits chunks in arrival order from multiple inputs
    - Verify StreamingBroadcast blocks until slowest consumer accepts
    - Verify StreamingPartition routes by predicate to output A/B
    - Verify StreamingBatch collects N chunks and emits concatenated
    - Verify StreamingDebounce emits last chunk within `window_ms`
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7, 6.8, 6.9, 6.10_

  - [ ]* 8.2 Write unit tests for flow control functions
    - Test rate limit throttles below threshold
    - Test timeout error after deadline
    - Test tee produces N identical outputs
    - Test batch collects correct number of chunks
    - Test partition routes correctly by predicate
    - _Requirements: 6.1, 6.3, 6.5, 6.9, 6.8_

- [ ] 9. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 10. Implement and verify CAS-specific operations
  - [ ] 10.1 Verify CAS functions in `crates/deriva-compute/src/builtins_streaming/cas.rs`
    - Verify StreamingCAddrEmbed forwards chunks while computing SHA-256, emits 32-byte CAddr before End
    - Verify StreamingCAddrVerify forwards chunks while computing SHA-256, errors if mismatch
    - Verify StreamingMerkleTree computes per-chunk SHA-256 leaves and binary tree root
    - Verify StreamingContentType detects MIME from magic bytes and prepends metadata chunk
    - Verify StreamingDiff XORs bytes pairwise from two input streams
    - Verify StreamingPatch XORs diff onto base
    - Verify StreamingChunkHash appends 32-byte SHA-256 per chunk (is_fusible() = true)
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 12.1_

  - [ ]* 10.2 Write unit tests for CAS operations
    - Test CAddr embed produces correct SHA-256 digest
    - Test CAddr verify passes on match, errors on mismatch
    - Test Merkle tree root for known leaf hashes
    - Test diff/patch round-trip: XOR(base, XOR(base, modified)) == modified
    - Test ChunkHash appends correct hash
    - _Requirements: 7.1, 7.2, 7.3, 7.5, 7.6, 7.7_

- [ ] 11. Implement and verify numeric operations
  - [ ] 11.1 Verify numeric functions in `crates/deriva-compute/src/builtins_streaming/numeric.rs`
    - Verify StreamingSum/Average parse newline-delimited decimals correctly
    - Verify StreamingAverage emits Error when zero valid numbers found
    - Verify StreamingBitwiseAnd/Or/Not apply byte-level ops with mask parameter
    - Verify StreamingByteSwap swaps adjacent byte pairs (is_fusible() = true)
    - Verify StreamingEntropy computes Shannon entropy in bits/byte range 0.0–8.0
    - Verify StreamingRollingHash computes rolling hash over sliding window
    - Verify BitwiseAnd, BitwiseOr, BitwiseNot, ByteSwap declare is_fusible() = true
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8, 8.9, 12.1_

  - [ ]* 11.2 Write unit tests for numeric functions
    - Test sum/average with known decimal inputs
    - Test average error on empty input
    - Test bitwise operations with known mask values
    - Test byte swap with known input
    - Test entropy for uniform distribution (should be ~8.0)
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8_

- [ ] 12. Implement and verify encoding and format conversion functions
  - [ ] 12.1 Verify encoding functions in `crates/deriva-compute/src/builtins_streaming/encoding.rs`
    - Verify StreamingHexEncode/Decode produce correct 2× / 0.5× output
    - Verify StreamingBase32Encode/Decode with data-encoding crate
    - Verify StreamingUtf8Validate buffers trailing bytes for split multi-byte sequences
    - Verify StreamingLineEnding converts between \n and \r\n
    - Verify StreamingJsonPrettyPrint/Minify/JsonLines use buffered pattern
    - Verify HexEncode, HexDecode, Base32Encode, Base32Decode, LineEnding declare is_fusible() = true
    - Verify error messages include invalid char/position for hex/base32/utf8 failures
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7, 9.8, 9.9, 9.10, 9.11, 9.12, 12.1_

  - [ ]* 12.2 Write property test for encoding round-trip
    - **Property 4: Encoding round-trip (hex, base32)**
    - For arbitrary byte sequences, encode then decode with hex/base32 == original
    - **Validates: Requirements 9.1, 9.2, 9.3, 9.4, 9.5, 9.6**

  - [ ]* 12.3 Write property test for JSON pretty-print/minify round-trip
    - **Property 4 (JSON subset): JSON format round-trip**
    - For valid JSON, pretty-print then minify produces valid equivalent JSON
    - **Validates: Requirements 9.13**

  - [ ]* 12.4 Write unit tests for encoding error conditions
    - Test odd-length hex input emits error with position
    - Test invalid hex character emits error with character
    - Test invalid Base32 characters produce error
    - Test invalid UTF-8 sequences produce error with byte value and position
    - _Requirements: 9.3, 9.6, 9.8_

- [ ] 13. Verify function registration and feature gating
  - [ ] 13.1 Verify registration and feature gating in `crates/deriva-compute/src/builtins_streaming/registration.rs` and `mod.rs`
    - Verify all 80 new functions are registered with snake_case names matching struct names
    - Verify `#[cfg(feature = "extended-streaming")]` gate wraps all 80 new registrations
    - Verify without `extended-streaming` feature, only original 20 are registered
    - Verify HashMap-based O(1) lookup in registry
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5_

  - [ ]* 13.2 Write unit tests for registration and feature gating
    - Test all 80 names resolve to valid StreamingComputeFunction references
    - Test lookup returns None for unknown function names
    - _Requirements: 11.1, 11.4, 11.5_

- [ ] 14. Verify fusibility declarations and error handling contract
  - [ ] 14.1 Verify fusibility declarations across all modules
    - Verify exactly 12 new functions return `is_fusible() = true`: HexEncode, HexDecode, Base32Encode, Base32Decode, BitwiseAnd, BitwiseOr, BitwiseNot, Pad, Trim, LineEnding, ByteSwap, ChunkHash
    - Verify remaining 68 new functions return `is_fusible() = false`
    - _Requirements: 12.1, 12.2_

  - [ ]* 14.2 Write property test for fusible output equivalence
    - **Property 7: Fusible output equivalence**
    - For pairs of adjacent fusible functions and arbitrary input, separate execution == fused execution
    - **Validates: Requirements 12.3**

  - [ ] 14.3 Verify error handling contract across all functions
    - Verify upstream Error propagation (forward and terminate)
    - Verify internal errors emit Error chunk with function name and detail
    - Verify downstream channel close causes silent termination
    - Verify missing required parameter produces Error with parameter name
    - _Requirements: 13.1, 13.2, 13.3, 13.4, 13.5, 14.1, 14.2, 14.3, 14.4, 14.5_

  - [ ]* 14.4 Write property test for error propagation consistency
    - **Property 8: Error propagation consistency**
    - For any streaming function, upstream Error chunk is forwarded downstream with no additional Data
    - **Validates: Requirements 13.1**

  - [ ]* 14.5 Write property test for empty input handling
    - **Property 9: Empty input handling**
    - For any streaming function, immediate End does not panic and produces valid output/termination
    - **Validates: Requirements 15.2**

- [ ] 15. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 16. Integration tests and benchmarks
  - [ ] 16.1 Write integration tests for multi-stage pipelines in `crates/deriva-compute/tests/streaming_ext/integration.rs`
    - Test secure pipeline: ZstdCompress → AesEncrypt → HmacSha256
    - Test log pipeline: Grep("ERROR") → LineCount
    - Test validation pipeline: JsonValidate → SizeLimit → Sha256Verify
    - Test fan-out: Tee(3) → [Sha256, ByteCount, Histogram]
    - Test fusion chain: HexEncode → BitwiseNot → Base32Encode (fused)
    - _Requirements: 12.3, 15.6_

  - [ ]* 16.2 Write benchmark suite in `crates/deriva-benchmarks/benches/`
    - Benchmark compression codec throughput (zstd vs lz4 vs snappy vs brotli)
    - Benchmark crypto throughput (AES-CTR, AES-GCM, HMAC, BLAKE3, SHA-512)
    - Benchmark text processing throughput (grep, sed on 100MB data)
    - Benchmark fusible chain throughput (fused vs unfused 5-stage)
    - _Requirements: 15.6_

- [ ] 17. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The project uses Rust with the `proptest` crate for property-based testing
- All streaming function implementations live in `crates/deriva-compute/src/builtins_streaming/`
- Tests go in `crates/deriva-compute/tests/streaming_ext/`
- Run tests with: `cargo test --features extended-streaming -p deriva-compute`

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["1.2", "2.1", "3.1", "13.1"] },
    { "id": 2, "tasks": ["2.2", "2.3", "3.2", "3.3", "3.4", "5.1", "6.1", "7.1", "8.1", "10.1", "11.1", "12.1", "13.2"] },
    { "id": 3, "tasks": ["5.2", "5.3", "6.2", "7.2", "7.3", "8.2", "10.2", "11.2", "12.2", "12.3", "12.4", "14.1"] },
    { "id": 4, "tasks": ["14.2", "14.3", "14.4", "14.5"] },
    { "id": 5, "tasks": ["16.1", "16.2"] }
  ]
}
```
