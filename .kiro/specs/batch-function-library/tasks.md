# Implementation Plan: Batch Function Library Expansion

## Overview

Expand the batch function library from 4 to 100 functions across 11 categories. Each function implements the `ComputeFunction` trait in Rust, with consistent error handling, deterministic behavior, feature-flag gating, and proper registry integration. Implementation proceeds category-by-category, with property-based tests validating correctness invariants.

## Tasks

- [ ] 1. Set up project structure and feature flag configuration
  - [ ] 1.1 Configure `extended-batch` feature flag in Cargo.toml
    - Add `extended-batch` feature flag that depends on `extended-streaming`
    - Add dependencies: `serde_json`, `serde_yaml`, `toml`, `csv`, `encoding_rs`, `jsonschema`, `rand`, `chacha20poly1305`, `ed25519-dalek`, `argon2`
    - Ensure feature flag gates all 96 new functions
    - _Requirements: 2.1, 2.2, 2.5_

  - [ ] 1.2 Create module structure for batch function categories
    - Create `src/batch/` module directory with category submodules: `transforms`, `compression`, `crypto`, `analytics`, `combiners`, `slicing`, `text`, `validation`, `format_conversion`, `cas`, `accumulators`
    - Create a `register_extended_batch(registry: &mut FunctionRegistry)` function gated behind `#[cfg(feature = "extended-batch")]`
    - Wire module into existing crate structure
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

- [ ] 2. Implement encoding/transform functions
  - [ ] 2.1 Implement base64, hex, base32, base64url, base58, url, and html encode/decode functions
    - Implement `base64_encode@1.0.0`, `base64_decode@1.0.0` (RFC 4648)
    - Implement `hex_encode@1.0.0`, `hex_decode@1.0.0` (lowercase hex)
    - Implement `base32_encode@1.0.0`, `base32_decode@1.0.0` (RFC 4648)
    - Implement `base64url_encode@1.0.0`, `base64url_decode@1.0.0` (URL-safe, no padding)
    - Implement `base58_encode@1.0.0`, `base58_decode@1.0.0` (Bitcoin alphabet)
    - Implement `url_encode@1.0.0`, `url_decode@1.0.0` (RFC 3986 percent-encoding)
    - Implement `html_encode@1.0.0`, `html_decode@1.0.0` (5 XML special chars)
    - Each validates input count == 1, returns `ComputeError::InputCount` otherwise
    - Decode functions return `ComputeError::ExecutionFailed` on invalid encoding input
    - Empty input returns empty output for all transform functions
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5, 11.6, 11.7, 11.15, 1.2, 1.6_

  - [ ]* 2.2 Write property tests for encoding round-trip (Property 1)
    - **Property 1: Round-trip for encoding pairs**
    - For each of the 7 encode/decode pairs, verify `decode(encode(x)) == x` with random byte inputs
    - **Validates: Requirements 11.8, 11.9, 11.10, 11.11, 11.12, 11.13, 11.14**

  - [ ] 2.3 Implement remaining transform functions (lowercase, reverse, xor, bitwise, pad, line-ending, byte-swap, trim)
    - Implement `lowercase@1.0.0` (ASCII lowercase)
    - Implement `reverse@1.0.0` (byte sequence reversal)
    - Implement `xor@1.0.0` (1 input + `key` param, XOR each byte)
    - Implement `bitwise_and@1.0.0`, `bitwise_or@1.0.0`, `bitwise_not@1.0.0` (mask params)
    - Implement `byte_swap@1.0.0` with `word_size` param (2, 4, or 8)
    - Implement `trim@1.0.0` (leading/trailing whitespace)
    - Implement `pad@1.0.0` with `block_size` param (PKCS7)
    - Implement `line_ending@1.0.0` with `target` param ("lf" or "crlf")
    - _Requirements: 1.2, 1.3, 1.6_

  - [ ]* 2.4 Write property tests for reverse involution and XOR involution (Properties 6, 9)
    - **Property 6: Reverse involution** — `reverse(reverse(x)) == x`
    - **Property 9: XOR involution** — `xor(xor(x, k), k) == x`
    - **Validates: Requirements 1.6**

- [ ] 3. Implement compression functions
  - [ ] 3.1 Implement compression and decompression for all 6 codecs
    - Implement `compress@1.0.0` / `decompress@1.0.0` (zlib)
    - Implement `zstd_compress@1.0.0` / `zstd_decompress@1.0.0` (level param "1"-"22", default "3")
    - Implement `lz4_compress@1.0.0` / `lz4_decompress@1.0.0` (lz4-framed)
    - Implement `snappy_compress@1.0.0` / `snappy_decompress@1.0.0`
    - Implement `brotli_compress@1.0.0` / `brotli_decompress@1.0.0` (quality param "0"-"11", default "6")
    - Implement `gzip_compress@1.0.0` / `gzip_decompress@1.0.0`
    - Validate input count == 1, validate level/quality params
    - Return `ComputeError::ExecutionFailed` on corrupt data with codec name
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.8, 1.2, 1.3, 1.4_

  - [ ]* 3.2 Write property tests for compression round-trip (Property 2)
    - **Property 2: Round-trip for compression pairs**
    - For each of the 6 pairs, verify `decompress(compress(x)) == x`
    - **Validates: Requirements 3.7**

- [ ] 4. Implement cryptography and hashing functions
  - [ ] 4.1 Implement hash functions (sha256, sha384, sha512, md5, blake3)
    - Implement `sha256@1.0.0`, `sha384@1.0.0`, `sha512@1.0.0` returning fixed-size digests
    - Implement `md5@1.0.0` returning 16-byte digest
    - Implement `blake3@1.0.0` returning 32-byte digest
    - Validate input count == 1
    - _Requirements: 4.1, 4.2, 4.3, 4.13, 1.2_

  - [ ] 4.2 Implement HMAC and encryption functions
    - Implement `hmac@1.0.0` with `key` param (hex), returns HMAC-SHA256 tag
    - Implement `aes_ctr_encrypt@1.0.0` / `aes_ctr_decrypt@1.0.0` (key 16/32 hex, nonce 16 hex)
    - Implement `aes_gcm_encrypt@1.0.0` / `aes_gcm_decrypt@1.0.0` (key 16/32 hex, nonce 12 hex, tag appended)
    - Implement `chacha20_encrypt@1.0.0` / `chacha20_decrypt@1.0.0` (key 32 hex, nonce 12 hex)
    - Validate params, return `ComputeError::InvalidParam` for bad key/nonce length
    - _Requirements: 4.4, 4.5, 4.6, 4.7, 4.8, 1.2, 1.3, 1.4_

  - [ ] 4.3 Implement Ed25519 and Argon2 functions
    - Implement `ed25519_sign@1.0.0` with `private_key` param
    - Implement `ed25519_verify@1.0.0` with `public_key` and `signature` params
    - Implement `argon2_hash@1.0.0` with `salt`, optional `iterations` and `memory_kb` params
    - _Requirements: 4.9, 4.10, 4.11, 1.2, 1.3_

  - [ ]* 4.4 Write property tests for encryption round-trip and hash determinism (Properties 3, 4)
    - **Property 3: Encryption round-trip** — `decrypt(encrypt(x, key, nonce), key, nonce) == x`
    - **Property 4: Hash determinism** — same input always produces same hash
    - **Validates: Requirements 4.12, 4.13**

- [ ] 5. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 6. Implement text processing functions
  - [ ] 6.1 Implement text search and transform functions
    - Implement `grep@1.0.0`, `replace@1.0.0`, `split@1.0.0`, `join@1.0.0`
    - Implement `trim@1.0.0`, `sort_lines@1.0.0`, `unique_lines@1.0.0`
    - Return `ComputeError::InvalidParam` for invalid regex
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.7, 5.8, 5.13, 1.2, 1.3_

  - [ ] 6.2 Implement text counting and line extraction functions
    - Implement `line_count@1.0.0`, `word_count@1.0.0`, `head@1.0.0`, `tail@1.0.0`
    - Implement `charset_convert@1.0.0` with `from`/`to` params
    - _Requirements: 5.9, 5.10, 5.11, 5.12, 5.14, 1.2, 1.3, 1.4_

  - [ ]* 6.3 Write property test for sort correctness (Property 5)
    - **Property 5: Sort stability** — output is sorted and contains all input lines
    - **Validates: Requirements 5.7**

- [ ] 7. Implement validation functions
  - [ ] 7.1 Implement all 8 validation functions
    - Implement `json_validate@1.0.0`, `utf8_validate@1.0.0`, `schema_validate@1.0.0`, `size_check@1.0.0`, `magic_bytes@1.0.0`, `regex_match@1.0.0`, `not_empty@1.0.0`, `content_type_check@1.0.0`
    - Return descriptive errors with location/size/pattern info
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7, 6.8, 1.2, 1.3, 1.4_

  - [ ]* 7.2 Write property test for input count validation (Property 10)
    - **Property 10: Input count validation** — wrong count returns `InputCount` error
    - **Validates: Requirements 1.2**

- [ ] 8. Implement analytics functions
  - [ ] 8.1 Implement all 10 analytics functions
    - Implement `histogram@1.0.0`, `reservoir_sample@1.0.0`, `percentile@1.0.0`, `mean@1.0.0`, `median@1.0.0`, `min@1.0.0`, `max@1.0.0`, `entropy@1.0.0`, `frequency@1.0.0`, `dedup_count@1.0.0`, `cardinality@1.0.0`
    - Return `ComputeError::ExecutionFailed` for unparseable numeric input
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.8, 7.9, 7.10, 1.2, 1.3, 1.4_

- [ ] 9. Implement slicing and restructuring functions
  - [ ] 9.1 Implement all 10 slicing functions
    - Implement `take@1.0.0`, `skip@1.0.0`, `slice@1.0.0`, `sort@1.0.0`, `reverse@1.0.0`, `shuffle@1.0.0`, `nth@1.0.0`, `chunk_split@1.0.0`, `interleave@1.0.0`, `zip@1.0.0`
    - Shuffle requires `seed` param for deterministic output
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8, 8.9, 8.10, 1.2, 1.3, 1.4_

  - [ ]* 9.2 Write property test for shuffle determinism (Property 7)
    - **Property 7: Shuffle determinism** — same seed produces same output
    - **Validates: Requirements 8.11**

- [ ] 10. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 11. Implement combiner functions
  - [ ] 11.1 Implement all 8 combiner functions
    - Verify existing `concat@1.0.0` remains unchanged
    - Implement `interleave_bytes@1.0.0`, `zip_concat@1.0.0`, `diff@1.0.0`, `patch@1.0.0`, `merge@1.0.0`, `select_input@1.0.0`, `alternate@1.0.0`
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7, 9.8, 1.2, 1.3, 1.4_

- [ ] 12. Implement format conversion functions
  - [ ] 12.1 Implement all 8 format conversion functions (batch-only)
    - Implement `json_to_yaml@1.0.0`, `yaml_to_json@1.0.0`, `csv_to_json@1.0.0`, `json_to_csv@1.0.0`, `toml_to_json@1.0.0`, `json_to_toml@1.0.0`, `pretty_json@1.0.0`, `minify_json@1.0.0`
    - Return `ComputeError::ExecutionFailed` for unparseable source format
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5, 10.6, 10.7, 10.8, 10.10, 10.11, 1.2, 1.4_

  - [ ]* 12.2 Write property test for format conversion round-trip (Property 8)
    - **Property 8: Format conversion round-trip** — `yaml_to_json(json_to_yaml(x))` semantically equivalent
    - **Validates: Requirements 10.1, 10.2**

- [ ] 13. Implement CAS-specific functions
  - [ ] 13.1 Implement all 9 CAS functions
    - Implement `caddr_of_leaf@1.0.0`, `caddr_verify@1.0.0`, `merkle_root@1.0.0`, `content_type@1.0.0`, `embed_metadata@1.0.0`, `strip_metadata@1.0.0`, `content_hash@1.0.0`, `split_by_size@1.0.0`
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5, 12.6, 12.7, 12.8, 1.2, 1.3, 1.4_

  - [ ]* 13.2 Write property test for CAddr consistency (Property 12)
    - **Property 12: CAddr consistency** — `caddr_of_leaf(x) == blake3(x)`
    - **Validates: Requirements 12.10**

- [ ] 14. Implement accumulator functions and finalize registry
  - [ ] 14.1 Implement all accumulator functions
    - Implement `byte_count@1.0.0`, `line_count_acc@1.0.0`, `word_count_acc@1.0.0`, `crc32_acc@1.0.0`, `checksum_adler32@1.0.0`
    - Dual-register with streaming counterparts
    - _Requirements: 13.1, 13.2, 13.3, 13.4, 13.5, 1.2, 1.6_

  - [ ] 14.2 Wire all functions into `register_extended_batch` and verify count
    - Register all 96 new functions
    - Dual-register ~70 functions in both batch and streaming registries
    - Verify total count == 100 batch functions
    - Verify duplicate registration returns error
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [ ]* 14.3 Write property test for cost estimation (Property 11)
    - **Property 11: Cost proportionality** — cost increases with input sizes
    - **Validates: Requirements 1.5**

- [ ] 15. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- All functions use Rust and implement the existing `ComputeFunction` trait
- The `extended-batch` feature flag gates all new functionality

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2"] },
    { "id": 1, "tasks": ["2.1", "3.1", "4.1"] },
    { "id": 2, "tasks": ["2.2", "2.3", "3.2", "4.2", "4.3"] },
    { "id": 3, "tasks": ["2.4", "4.4", "6.1", "7.1", "8.1"] },
    { "id": 4, "tasks": ["6.2", "6.3", "7.2", "9.1"] },
    { "id": 5, "tasks": ["9.2", "11.1", "12.1", "13.1", "14.1"] },
    { "id": 6, "tasks": ["12.2", "13.2"] },
    { "id": 7, "tasks": ["14.2", "14.3"] }
  ]
}
```
