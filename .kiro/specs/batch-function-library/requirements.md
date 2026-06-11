# Requirements Document

## Introduction

Phase 2.15 of Deriva/CAS-DFS expands the batch function library from 4 functions to 100 functions across 11 categories. Batch functions implement the synchronous `ComputeFunction` trait, receiving all inputs as `Vec<Bytes>` and returning a single `Bytes` result. They are preferred for small inputs (below the 3MB streaming threshold) via §2.9 size-aware mode selection, avoiding the overhead of task spawning, channel allocation, and chunk framing. Of the 100 total functions, approximately 70 have streaming counterparts (registered in both batch and streaming registries for automatic mode selection), approximately 20 are batch-only (format conversion, global sort, schema validation — requiring full input), and the 4 existing functions (Identity, Concat, Uppercase, Repeat) remain unchanged.

## Glossary

- **ComputeFunction**: The synchronous trait for batch compute functions. Provides `execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError>`, `id(&self) -> FunctionId`, and `estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost`.
- **FunctionRegistry**: The registry that stores both batch and streaming function implementations indexed by function name and version. Used by §2.9 mode selection to look up available implementations.
- **FunctionId**: A struct containing a function name (String) and semantic version (String), e.g. `"compress@1.0.0"`.
- **ComputeError**: The error type returned by batch functions on failure. Variants include `InputCount`, `InvalidParam`, `ExecutionFailed`, and `FunctionNotFound`.
- **ComputeCost**: A struct containing `cpu_ms` and `memory_bytes` estimates used by the scheduler for resource planning.
- **Streaming_Threshold**: The per-pipeline input size boundary (default 3MB) below which §2.9 mode selection prefers batch execution.
- **Batch_Only_Function**: A function that has no streaming counterpart because it requires full input access (e.g., global sort, JSON schema validation, format conversion).
- **Dual_Registered_Function**: A function registered in both the batch and streaming registries, enabling §2.9 to select the optimal execution mode based on input size.
- **Round_Trip_Property**: The correctness invariant that encoding followed by decoding (or vice versa) produces the original input, i.e., `decode(encode(x)) == x`.
- **Deterministic_Function**: A function that always produces identical output bytes given identical input bytes and parameters, regardless of execution environment or timing.
- **PKCS7_Padding**: A padding scheme where each padding byte contains the number of padding bytes added, used by PadFn for block alignment.

## Requirements

### Requirement 1: ComputeFunction Trait Compliance

**User Story:** As a system developer, I want all batch functions to implement the ComputeFunction trait uniformly, so that they integrate with the existing executor, registry, and §2.9 mode selection infrastructure without special-casing.

#### Acceptance Criteria

1. THE ComputeFunction implementation for each batch function SHALL provide a unique FunctionId via the `id()` method containing a name string and a version string in `"name@version"` format.
2. THE ComputeFunction implementation for each batch function SHALL validate the number of inputs received against the expected count and return `ComputeError::InputCount` when the count does not match.
3. THE ComputeFunction implementation for each batch function SHALL validate all required parameters and return `ComputeError::InvalidParam` with a descriptive message when a parameter is missing or has an invalid value.
4. THE ComputeFunction implementation for each batch function SHALL return `ComputeError::ExecutionFailed` with a descriptive message when the function logic encounters a processing error on valid inputs.
5. THE `estimated_cost` method for each batch function SHALL return a ComputeCost struct with `cpu_ms` and `memory_bytes` estimates proportional to the sum of input sizes.
6. THE ComputeFunction implementation for each batch function SHALL be both `Send` and `Sync`.

### Requirement 2: Function Registry Integration

**User Story:** As a system developer, I want all 100 batch functions registered in the FunctionRegistry, so that the executor and §2.9 mode selection can discover and invoke them by name and version.

#### Acceptance Criteria

1. WHEN the batch function library is initialized, THE FunctionRegistry SHALL contain exactly 100 batch function registrations.
2. THE FunctionRegistry SHALL index each batch function by its FunctionId (name and version pair).
3. WHEN a batch function has a streaming counterpart, THE FunctionRegistry SHALL contain both a batch registration and a streaming registration under the same function name, enabling §2.9 mode selection.
4. WHEN a batch function is batch-only (no streaming counterpart), THE FunctionRegistry SHALL contain only the batch registration for that function name.
5. IF a function name and version pair already exists in the FunctionRegistry during registration, THEN THE FunctionRegistry SHALL return an error indicating a duplicate registration.

### Requirement 3: Compression Functions

**User Story:** As a pipeline author, I want batch compression and decompression functions for zlib, zstd, lz4, snappy, brotli, and gzip codecs, so that I can compress and decompress stored values with full-input context for optimal compression ratio.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL provide `compress@1.0.0` and `decompress@1.0.0` functions using zlib encoding that accept 1 input and return compressed or decompressed bytes respectively.
2. THE Batch_Function_Library SHALL provide `zstd_compress@1.0.0` and `zstd_decompress@1.0.0` functions that accept 1 input and an optional `level` parameter (string, range "1" to "22", default "3").
3. THE Batch_Function_Library SHALL provide `lz4_compress@1.0.0` and `lz4_decompress@1.0.0` functions that accept 1 input and return lz4-framed compressed or decompressed bytes.
4. THE Batch_Function_Library SHALL provide `snappy_compress@1.0.0` and `snappy_decompress@1.0.0` functions that accept 1 input and return snappy-compressed or decompressed bytes.
5. THE Batch_Function_Library SHALL provide `brotli_compress@1.0.0` and `brotli_decompress@1.0.0` functions that accept 1 input and an optional `quality` parameter (string, range "0" to "11", default "6").
6. THE Batch_Function_Library SHALL provide `gzip_compress@1.0.0` and `gzip_decompress@1.0.0` functions that accept 1 input and return gzip-compressed or decompressed bytes.
7. FOR ALL valid byte sequences, compressing then decompressing with any codec SHALL produce output identical to the original input (Round_Trip_Property).
8. IF a decompression function receives invalid or corrupt compressed data, THEN THE function SHALL return `ComputeError::ExecutionFailed` with a message identifying the codec and the nature of the corruption.

### Requirement 4: Cryptography and Hashing Functions

**User Story:** As a pipeline author, I want batch cryptographic functions for hashing, HMAC, encryption, and digital signatures, so that I can perform integrity verification, data-at-rest encryption, and authentication within batch pipelines.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL provide `sha256@1.0.0`, `sha384@1.0.0`, and `sha512@1.0.0` hash functions that accept 1 input and return the fixed-size hash digest as raw bytes.
2. THE Batch_Function_Library SHALL provide `md5@1.0.0` that accepts 1 input and returns the 16-byte MD5 digest.
3. THE Batch_Function_Library SHALL provide `blake3@1.0.0` that accepts 1 input and returns the 32-byte BLAKE3 digest.
4. THE Batch_Function_Library SHALL provide `hmac@1.0.0` that accepts 1 input and a `key` parameter (hex-encoded string) and returns the HMAC-SHA256 authentication tag as raw bytes.
5. THE Batch_Function_Library SHALL provide `aes_ctr_encrypt@1.0.0` and `aes_ctr_decrypt@1.0.0` that accept 1 input, a `key` parameter (hex-encoded, 16 or 32 bytes), and a `nonce` parameter (hex-encoded, 16 bytes), returning the encrypted or decrypted bytes.
6. THE Batch_Function_Library SHALL provide `aes_gcm_encrypt@1.0.0` that accepts 1 input, a `key` parameter (hex-encoded, 16 or 32 bytes), and a `nonce` parameter (hex-encoded, 12 bytes), returning ciphertext concatenated with the 16-byte authentication tag.
7. THE Batch_Function_Library SHALL provide `aes_gcm_decrypt@1.0.0` that accepts 1 input (ciphertext with appended 16-byte tag), a `key` parameter, and a `nonce` parameter, returning the decrypted plaintext or an authentication error.
8. THE Batch_Function_Library SHALL provide `chacha20_encrypt@1.0.0` and `chacha20_decrypt@1.0.0` that accept 1 input, a `key` parameter (hex-encoded, 32 bytes), and a `nonce` parameter (hex-encoded, 12 bytes).
9. THE Batch_Function_Library SHALL provide `ed25519_sign@1.0.0` that accepts 1 input and a `private_key` parameter (hex-encoded, 32 bytes), returning the 64-byte Ed25519 signature.
10. THE Batch_Function_Library SHALL provide `ed25519_verify@1.0.0` that accepts 1 input, a `public_key` parameter (hex-encoded, 32 bytes), and a `signature` parameter (hex-encoded, 64 bytes), returning a single byte `0x01` for valid or `ComputeError::ExecutionFailed` for invalid.
11. THE Batch_Function_Library SHALL provide `argon2_hash@1.0.0` that accepts 1 input (password bytes), a `salt` parameter (hex-encoded, minimum 16 bytes), and optional `iterations` (string, default "3") and `memory_kb` (string, default "65536") parameters, returning the 32-byte Argon2id hash.
12. FOR ALL valid inputs and matching key/nonce pairs, encrypting then decrypting with AES-CTR, AES-GCM, or ChaCha20 SHALL produce output identical to the original input (Round_Trip_Property).
13. THE hash functions (sha256, sha384, sha512, md5, blake3) SHALL be Deterministic_Functions producing identical output for identical input regardless of execution context.

### Requirement 5: Text Processing Functions

**User Story:** As a pipeline author, I want batch text processing functions for searching, replacing, splitting, joining, and line-oriented operations, so that I can process logs, configurations, and text data within batch pipelines.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL provide `grep@1.0.0` that accepts 1 input and a `pattern` parameter (regex string), returning only the lines matching the pattern joined by newline.
2. THE Batch_Function_Library SHALL provide `replace@1.0.0` that accepts 1 input, a `pattern` parameter (regex string), and a `replacement` parameter (string with optional capture group references), returning the input with all matches replaced.
3. THE Batch_Function_Library SHALL provide `split@1.0.0` that accepts 1 input and a `delimiter` parameter (string), returning parts separated by a null byte (`0x00`).
4. THE Batch_Function_Library SHALL provide `join@1.0.0` that accepts multiple inputs and a `separator` parameter (string), returning the inputs concatenated with the separator between each pair.
5. THE Batch_Function_Library SHALL provide `trim@1.0.0` that accepts 1 input and returns the input with leading and trailing ASCII whitespace removed.
6. THE Batch_Function_Library SHALL provide `pad@1.0.0` that accepts 1 input and a `block_size` parameter (string, integer 1-256), returning the input padded to the next multiple of block_size using PKCS7_Padding.
7. THE Batch_Function_Library SHALL provide `sort_lines@1.0.0` that accepts 1 input and returns the input with lines sorted lexicographically, preserving the trailing newline if present.
8. THE Batch_Function_Library SHALL provide `unique_lines@1.0.0` that accepts 1 input and returns the input with consecutive duplicate lines removed.
9. THE Batch_Function_Library SHALL provide `line_count@1.0.0` that accepts 1 input and returns the number of newline characters as a decimal ASCII string.
10. THE Batch_Function_Library SHALL provide `word_count@1.0.0` that accepts 1 input and returns the number of whitespace-separated tokens as a decimal ASCII string.
11. THE Batch_Function_Library SHALL provide `head@1.0.0` that accepts 1 input and an `n` parameter (string, positive integer), returning the first n lines.
12. THE Batch_Function_Library SHALL provide `tail@1.0.0` that accepts 1 input and an `n` parameter (string, positive integer), returning the last n lines.
13. IF an invalid regex pattern is provided to grep or replace, THEN THE function SHALL return `ComputeError::InvalidParam` with the regex parse error message.
14. THE Batch_Function_Library SHALL provide `charset_convert@1.0.0` that accepts 1 input, a `from` parameter (source encoding name, e.g. "iso-8859-1", "utf-16le", "shift_jis"), and a `to` parameter (target encoding name, default "utf-8"), returning the transcoded bytes or `ComputeError::ExecutionFailed` if the input contains bytes invalid in the source encoding.

### Requirement 6: Validation Functions

**User Story:** As a pipeline author, I want batch validation functions that verify input structure and content, so that I can sanitize and gate data before downstream processing.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL provide `json_validate@1.0.0` that accepts 1 input and returns the input unchanged if it is valid JSON, or `ComputeError::ExecutionFailed` with the parse error location.
2. THE Batch_Function_Library SHALL provide `utf8_validate@1.0.0` that accepts 1 input and returns the input unchanged if it is valid UTF-8, or `ComputeError::ExecutionFailed` with the byte offset of the first invalid sequence.
3. THE Batch_Function_Library SHALL provide `schema_validate@1.0.0` that accepts 1 input (JSON data) and a `schema` parameter (JSON Schema string), returning the input unchanged if it conforms to the schema, or `ComputeError::ExecutionFailed` listing validation errors.
4. THE Batch_Function_Library SHALL provide `size_check@1.0.0` that accepts 1 input, a `min` parameter (string, optional, default "0"), and a `max` parameter (string, optional, default "unlimited"), returning the input unchanged if its byte length is within [min, max], or `ComputeError::ExecutionFailed` stating the actual size and allowed range.
5. THE Batch_Function_Library SHALL provide `magic_bytes@1.0.0` that accepts 1 input and a `expected` parameter (hex-encoded prefix), returning the input unchanged if the input starts with the expected byte prefix, or `ComputeError::ExecutionFailed` if the prefix does not match.
6. THE Batch_Function_Library SHALL provide `regex_match@1.0.0` that accepts 1 input and a `pattern` parameter (regex string), returning the input unchanged if the entire input matches the pattern, or `ComputeError::ExecutionFailed` indicating no match.
7. THE Batch_Function_Library SHALL provide `not_empty@1.0.0` that accepts 1 input and returns the input unchanged if its byte length is greater than zero, or `ComputeError::ExecutionFailed` indicating empty input.
8. THE Batch_Function_Library SHALL provide `content_type_check@1.0.0` that accepts 1 input and an `expected` parameter (MIME type string), returning the input unchanged if magic byte detection identifies the expected content type, or `ComputeError::ExecutionFailed` if the detected type does not match.

### Requirement 7: Analytics Functions

**User Story:** As a pipeline author, I want batch analytics functions for statistical computation, frequency analysis, and data profiling, so that I can build analysis pipelines over stored values.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL provide `histogram@1.0.0` that accepts 1 input (newline-separated numeric values) and a `buckets` parameter (comma-separated boundary values), returning a JSON object mapping bucket ranges to counts.
2. THE Batch_Function_Library SHALL provide `reservoir_sample@1.0.0` that accepts 1 input (newline-separated values), a `k` parameter (string, sample size), and an optional `seed` parameter (string, u64), returning k uniformly sampled lines. WHEN a seed is provided, the output SHALL be deterministic.
3. THE Batch_Function_Library SHALL provide `percentile@1.0.0` that accepts 1 input (newline-separated numeric values) and a `p` parameter (string, value between "0" and "100"), returning the computed percentile as a decimal ASCII string.
4. THE Batch_Function_Library SHALL provide `mean@1.0.0` and `median@1.0.0` that each accept 1 input (newline-separated numeric values) and return the computed statistic as a decimal ASCII string.
5. THE Batch_Function_Library SHALL provide `min@1.0.0` and `max@1.0.0` that each accept 1 input (newline-separated numeric values) and return the minimum or maximum value as a decimal ASCII string.
6. THE Batch_Function_Library SHALL provide `entropy@1.0.0` that accepts 1 input (raw bytes) and returns the Shannon entropy in bits as a decimal ASCII string with 6 decimal places.
7. THE Batch_Function_Library SHALL provide `frequency@1.0.0` that accepts 1 input (newline-separated values) and returns a JSON object mapping each unique value to its occurrence count, sorted by count descending.
8. THE Batch_Function_Library SHALL provide `dedup_count@1.0.0` that accepts 1 input (newline-separated values) and returns the number of unique values as a decimal ASCII string.
9. THE Batch_Function_Library SHALL provide `cardinality@1.0.0` that accepts 1 input (raw bytes) and returns the number of distinct byte values (0-255) present as a decimal ASCII string.
10. IF an analytics function receives input that cannot be parsed as the expected numeric format, THEN THE function SHALL return `ComputeError::ExecutionFailed` identifying the first unparseable value.

### Requirement 8: Slicing and Restructuring Functions

**User Story:** As a pipeline author, I want batch slicing functions for extracting, reordering, and restructuring byte sequences, so that I can select subsets and transform data ordering within batch pipelines.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL provide `take@1.0.0` that accepts 1 input and an `n` parameter (string, byte count), returning the first n bytes of the input (or the entire input if shorter than n).
2. THE Batch_Function_Library SHALL provide `skip@1.0.0` that accepts 1 input and an `n` parameter (string, byte count), returning the input with the first n bytes removed (or empty if input is shorter than n).
3. THE Batch_Function_Library SHALL provide `slice@1.0.0` that accepts 1 input, a `start` parameter (string, byte offset), and a `length` parameter (string, byte count), returning the specified byte range (clamped to input bounds).
4. THE Batch_Function_Library SHALL provide `sort@1.0.0` that accepts 1 input (newline-separated lines) and returns the lines sorted lexicographically by byte value, joined by newline.
5. THE Batch_Function_Library SHALL provide `reverse@1.0.0` that accepts 1 input and returns the entire byte sequence reversed.
6. THE Batch_Function_Library SHALL provide `shuffle@1.0.0` that accepts 1 input (newline-separated lines) and a `seed` parameter (string, u64), returning the lines in a deterministic pseudo-random order based on the seed.
7. THE Batch_Function_Library SHALL provide `nth@1.0.0` that accepts 1 input (newline-separated lines) and an `index` parameter (string, zero-based), returning the specified line or `ComputeError::ExecutionFailed` if the index is out of bounds.
8. THE Batch_Function_Library SHALL provide `chunk_split@1.0.0` that accepts 1 input and a `size` parameter (string, byte count), returning the input divided into fixed-size chunks separated by null bytes.
9. THE Batch_Function_Library SHALL provide `interleave@1.0.0` that accepts 2 or more inputs and returns the inputs interleaved line-by-line, cycling through inputs in order.
10. THE Batch_Function_Library SHALL provide `zip@1.0.0` that accepts 2 inputs (each newline-separated lines) and a `separator` parameter (string), returning corresponding lines from each input joined by the separator.
11. FOR ALL inputs and a given seed value, the `shuffle` function SHALL produce identical output across repeated invocations (Deterministic_Function given fixed seed).
12. FOR ALL inputs, `reverse(reverse(x))` SHALL produce output identical to the original input (Round_Trip_Property).

### Requirement 9: Combiner Functions

**User Story:** As a pipeline author, I want batch combiner functions that merge, diff, and select from multiple inputs, so that I can build multi-input transformation pipelines.

#### Acceptance Criteria

1. THE existing `concat@1.0.0` function (ConcatFn) SHALL remain as the primary concatenation combiner accepting 2 or more inputs.
2. THE Batch_Function_Library SHALL provide `interleave_bytes@1.0.0` that accepts 2 or more inputs and returns the inputs interleaved byte-by-byte in round-robin order.
3. THE Batch_Function_Library SHALL provide `zip_concat@1.0.0` that accepts 2 inputs and returns them concatenated with a length-prefix header (4 bytes big-endian for each input length, followed by input data in order).
4. THE Batch_Function_Library SHALL provide `diff@1.0.0` that accepts 2 inputs (original and modified, both line-based text) and returns a unified diff output.
5. THE Batch_Function_Library SHALL provide `patch@1.0.0` that accepts 2 inputs (original text and unified diff) and returns the patched text, or `ComputeError::ExecutionFailed` if the diff does not apply cleanly.
6. THE Batch_Function_Library SHALL provide `merge@1.0.0` that accepts 2 or more inputs (each newline-separated sorted lines) and returns all lines merged into a single sorted sequence.
7. THE Batch_Function_Library SHALL provide `select_input@1.0.0` that accepts 2 or more inputs and an `index` parameter (string, zero-based), returning the input at the specified index unchanged.
8. THE Batch_Function_Library SHALL provide `alternate@1.0.0` that accepts 2 inputs (each newline-separated) and returns alternating lines from each input (line 1 from input 1, line 1 from input 2, line 2 from input 1, etc.).

### Requirement 10: Format Conversion Functions

**User Story:** As a pipeline author, I want batch format conversion functions between JSON, YAML, CSV, and TOML, so that I can transform data between common serialization formats within batch pipelines.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL provide `json_to_yaml@1.0.0` that accepts 1 input (valid JSON) and returns the equivalent YAML representation.
2. THE Batch_Function_Library SHALL provide `yaml_to_json@1.0.0` that accepts 1 input (valid YAML) and returns the equivalent compact JSON representation.
3. THE Batch_Function_Library SHALL provide `csv_to_json@1.0.0` that accepts 1 input (valid CSV with header row) and returns a JSON array of objects using headers as keys.
4. THE Batch_Function_Library SHALL provide `json_to_csv@1.0.0` that accepts 1 input (JSON array of flat objects) and returns CSV with a header row derived from the union of all object keys.
5. THE Batch_Function_Library SHALL provide `toml_to_json@1.0.0` that accepts 1 input (valid TOML) and returns the equivalent compact JSON representation.
6. THE Batch_Function_Library SHALL provide `json_to_toml@1.0.0` that accepts 1 input (valid JSON object) and returns the equivalent TOML representation.
7. THE Batch_Function_Library SHALL provide `pretty_json@1.0.0` that accepts 1 input (valid JSON) and returns the JSON formatted with 2-space indentation.
8. THE Batch_Function_Library SHALL provide `minify_json@1.0.0` that accepts 1 input (valid JSON) and returns the JSON with all insignificant whitespace removed.
9. FOR ALL valid JSON inputs, `minify_json(pretty_json(x))` SHALL produce output equivalent to `minify_json(x)` (idempotent normalization).
10. IF a format conversion function receives input that cannot be parsed as the expected source format, THEN THE function SHALL return `ComputeError::ExecutionFailed` with the parse error details.
11. ALL format conversion functions SHALL be Batch_Only_Functions (no streaming counterpart) because they require the full input to parse the source format.

### Requirement 11: Encoding Functions

**User Story:** As a pipeline author, I want batch encoding and decoding functions for base64, base64url, hex, base32, base58, URL encoding, and HTML encoding, so that I can transform data representation within batch pipelines with correct padding and full-input handling.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL provide `base64_encode@1.0.0` and `base64_decode@1.0.0` implementing RFC 4648 standard Base64 encoding and decoding.
2. THE Batch_Function_Library SHALL provide `hex_encode@1.0.0` and `hex_decode@1.0.0` encoding bytes as lowercase hexadecimal characters and decoding hex strings to bytes.
3. THE Batch_Function_Library SHALL provide `base32_encode@1.0.0` and `base32_decode@1.0.0` implementing RFC 4648 Base32 encoding and decoding.
4. THE Batch_Function_Library SHALL provide `base64url_encode@1.0.0` and `base64url_decode@1.0.0` implementing RFC 4648 URL-safe Base64 encoding (using `-` and `_` instead of `+` and `/`, no padding).
5. THE Batch_Function_Library SHALL provide `base58_encode@1.0.0` and `base58_decode@1.0.0` implementing Bitcoin-style Base58 encoding and decoding (alphabet: `123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz`).
6. THE Batch_Function_Library SHALL provide `url_encode@1.0.0` and `url_decode@1.0.0` implementing RFC 3986 percent-encoding for unreserved characters and decoding percent-encoded strings.
7. THE Batch_Function_Library SHALL provide `html_encode@1.0.0` and `html_decode@1.0.0` encoding the five XML special characters (`<`, `>`, `&`, `"`, `'`) as named HTML entities and decoding named and numeric HTML entities back to characters.
8. FOR ALL valid byte sequences, `base64_decode(base64_encode(x))` SHALL produce output identical to x (Round_Trip_Property).
9. FOR ALL valid byte sequences, `hex_decode(hex_encode(x))` SHALL produce output identical to x (Round_Trip_Property).
10. FOR ALL valid byte sequences, `base32_decode(base32_encode(x))` SHALL produce output identical to x (Round_Trip_Property).
11. FOR ALL valid byte sequences, `base64url_decode(base64url_encode(x))` SHALL produce output identical to x (Round_Trip_Property).
12. FOR ALL valid byte sequences, `base58_decode(base58_encode(x))` SHALL produce output identical to x (Round_Trip_Property).
13. FOR ALL valid byte sequences, `url_decode(url_encode(x))` SHALL produce output identical to x (Round_Trip_Property).
14. FOR ALL valid UTF-8 strings, `html_decode(html_encode(x))` SHALL produce output identical to x (Round_Trip_Property).
15. IF a decode function receives input that is not valid for its encoding format, THEN THE function SHALL return `ComputeError::ExecutionFailed` identifying the nature of the encoding error.

### Requirement 12: CAS-Specific Functions

**User Story:** As a pipeline author, I want batch functions for content-addressing operations like CAddr computation, Merkle root calculation, and metadata embedding, so that I can leverage the CAS-DFS content-addressing model within batch pipelines.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL provide `caddr_of_leaf@1.0.0` that accepts 1 input and returns the 32-byte BLAKE3 content address of the input.
2. THE Batch_Function_Library SHALL provide `caddr_verify@1.0.0` that accepts 1 input and an `expected` parameter (hex-encoded 32-byte CAddr), returning the input unchanged if the computed CAddr matches expected, or `ComputeError::ExecutionFailed` if it does not match.
3. THE Batch_Function_Library SHALL provide `merkle_root@1.0.0` that accepts 2 or more inputs and returns the 32-byte BLAKE3 Merkle root computed by recursively hashing pairs of inputs.
4. THE Batch_Function_Library SHALL provide `content_type@1.0.0` that accepts 1 input and returns a MIME type string detected from magic bytes (e.g., "application/json", "image/png", "application/octet-stream" as default).
5. THE Batch_Function_Library SHALL provide `embed_metadata@1.0.0` that accepts 1 input and a `metadata` parameter (JSON object string), returning the input prepended with a length-prefixed metadata header.
6. THE Batch_Function_Library SHALL provide `strip_metadata@1.0.0` that accepts 1 input with an embedded metadata header and returns the input with the metadata header removed.
7. THE Batch_Function_Library SHALL provide `content_hash@1.0.0` that accepts 1 input and a `algorithm` parameter (string: "sha256", "sha512", "blake3", or "md5"), returning the hash digest using the specified algorithm.
8. THE Batch_Function_Library SHALL provide `split_by_size@1.0.0` that accepts 1 input and a `chunk_size` parameter (string, byte count), returning the input divided into chunks of the specified size separated by null bytes.
9. FOR ALL inputs, `strip_metadata(embed_metadata(x, metadata))` SHALL produce output identical to x (Round_Trip_Property).
10. FOR ALL inputs, `caddr_of_leaf(x)` SHALL produce the same output as `blake3(x)` (consistency with CAS-DFS addressing scheme).

### Requirement 13: Accumulator Functions

**User Story:** As a pipeline author, I want batch accumulator functions that compute running digests and counts over entire inputs, so that I can produce checksums, byte counts, and line counts with minimal overhead for small inputs.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL provide accumulator `sha256_acc@1.0.0` that accepts 1 input and returns the 32-byte SHA-256 digest (functionally identical to `sha256@1.0.0` but registered as an accumulator category function).
2. THE Batch_Function_Library SHALL provide accumulator `sha512_acc@1.0.0` that accepts 1 input and returns the 64-byte SHA-512 digest.
3. THE Batch_Function_Library SHALL provide accumulator `blake3_acc@1.0.0` that accepts 1 input and returns the 32-byte BLAKE3 digest.
4. THE Batch_Function_Library SHALL provide accumulator `crc32_acc@1.0.0` that accepts 1 input and returns the CRC-32 checksum as 4 bytes in big-endian order.
5. THE Batch_Function_Library SHALL provide accumulator `byte_count@1.0.0` that accepts 1 input and returns the byte length as a decimal ASCII string.
6. THE Batch_Function_Library SHALL provide accumulator `line_count_acc@1.0.0` that accepts 1 input and returns the count of newline characters as a decimal ASCII string.
7. THE Batch_Function_Library SHALL provide accumulator `word_count_acc@1.0.0` that accepts 1 input and returns the count of whitespace-separated tokens as a decimal ASCII string.
8. THE Batch_Function_Library SHALL provide accumulator `checksum_adler32@1.0.0` that accepts 1 input and returns the Adler-32 checksum as 4 bytes in big-endian order.
9. ALL accumulator functions SHALL be Deterministic_Functions producing identical output for identical input.
10. ALL accumulator functions SHALL have streaming counterparts registered as Dual_Registered_Functions for §2.9 mode selection.

### Requirement 14: Determinism Guarantee

**User Story:** As a system developer, I want all batch functions to be deterministic (same input always produces same output), so that content-addressing remains valid and cached results are reusable.

#### Acceptance Criteria

1. THE Batch_Function_Library SHALL ensure that every function without an explicit randomness seed parameter is a Deterministic_Function.
2. WHEN a function accepts a `seed` parameter (shuffle, reservoir_sample), THE function SHALL produce deterministic output for a given seed value across all invocations regardless of execution timing or platform.
3. THE Batch_Function_Library SHALL ensure that no function uses system time, thread-local state, or process-specific information in computing its output.
4. FOR ALL Deterministic_Functions f and all valid inputs x, `f(x) == f(x)` SHALL hold across separate invocations, process restarts, and different machines running the same version.

### Requirement 15: Error Handling Consistency

**User Story:** As a pipeline author, I want consistent error reporting across all batch functions, so that error handling logic can be written generically without function-specific error parsing.

#### Acceptance Criteria

1. WHEN a batch function receives fewer inputs than required, THE function SHALL return `ComputeError::InputCount { expected: N, got: M }` where N is the required count and M is the received count.
2. WHEN a batch function receives more inputs than the maximum accepted, THE function SHALL return `ComputeError::InputCount { expected: N, got: M }`.
3. WHEN a required parameter is missing, THE function SHALL return `ComputeError::InvalidParam` with a message containing the parameter name.
4. WHEN a parameter value cannot be parsed to the expected type, THE function SHALL return `ComputeError::InvalidParam` with a message containing the parameter name and the expected format.
5. WHEN function execution fails due to input data (corrupt compressed data, invalid JSON, out-of-range values), THE function SHALL return `ComputeError::ExecutionFailed` with a message describing the failure cause.
6. THE error messages across all batch functions SHALL use consistent formatting: lowercase descriptive text without trailing punctuation.

### Requirement 16: Empty Input Handling

**User Story:** As a pipeline author, I want predictable behavior when batch functions receive empty inputs, so that pipelines with optional or absent data produce consistent results.

#### Acceptance Criteria

1. WHEN a transform function (encoding, bitwise, text) receives empty input (zero bytes), THE function SHALL return empty output (zero bytes) without error.
2. WHEN a compression function receives empty input, THE function SHALL return a valid compressed representation of the empty sequence (codec-specific minimal output).
3. WHEN a hash or accumulator function receives empty input, THE function SHALL return the hash of the empty byte sequence (e.g., SHA-256 of empty is `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`).
4. WHEN an analytics function (mean, median, percentile) receives empty input (no values), THE function SHALL return `ComputeError::ExecutionFailed` indicating insufficient data.
5. WHEN a validation function receives empty input, THE `not_empty` function SHALL return an error, and other validation functions SHALL evaluate the empty input against their specific rules (empty string is valid UTF-8, empty string is not valid JSON).

### Requirement 17: Feature Flag Gating

**User Story:** As a library consumer, I want the extended batch function library gated behind a feature flag, so that downstream crates can choose minimal builds without pulling in compression, crypto, and format conversion dependencies.

#### Acceptance Criteria

1. THE `extended-batch` feature flag in the `deriva-compute` crate SHALL control availability of all 96 new batch functions.
2. WHEN the `extended-batch` feature is disabled, THE FunctionRegistry SHALL contain only the 4 existing batch functions (Identity, Concat, Uppercase, Repeat).
3. WHEN the `extended-batch` feature is enabled, THE FunctionRegistry SHALL contain all 100 batch functions.
4. THE `extended-batch` feature flag SHALL depend on the `extended-streaming` feature flag (pulling in shared crypto and compression dependencies).
5. THE `extended-batch` feature SHALL additionally depend on `serde_json`, `serde_yaml`, `toml`, `csv`, `encoding_rs`, `jsonschema`, and `rand` crates for format conversion and batch-only operations.

### Requirement 18: Cost Estimation Accuracy

**User Story:** As the scheduler, I want each batch function to provide CPU and memory cost estimates proportional to input size, so that resource planning and scheduling can account for function complexity.

#### Acceptance Criteria

1. THE `estimated_cost` method for transform functions (encoding, bitwise) SHALL report `cpu_ms` as approximately `total_input_bytes / (1024 * 1024)` (1ms per MB) and `memory_bytes` as `total_input_bytes`.
2. THE `estimated_cost` method for compression functions SHALL report `cpu_ms` as approximately `total_input_bytes / (256 * 1024)` (4ms per MB) and `memory_bytes` as `total_input_bytes * 2`.
3. THE `estimated_cost` method for cryptographic functions SHALL report `cpu_ms` as approximately `total_input_bytes / (512 * 1024)` (2ms per MB) and `memory_bytes` as `total_input_bytes`.
4. THE `estimated_cost` method for format conversion functions SHALL report `cpu_ms` as approximately `total_input_bytes / (128 * 1024)` (8ms per MB) and `memory_bytes` as `total_input_bytes * 3`.
5. THE `estimated_cost` method SHALL return estimates in O(1) time without reading or processing the actual input data.
