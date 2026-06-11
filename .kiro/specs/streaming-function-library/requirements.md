# Requirements Document

## Introduction

Phase 2.14 expands the streaming function library from 20 to 100 functions across 13 categories (4 existing + 9 new). The existing 20 functions (§2.7) cover basic transforms, accumulators, combiners, and utilities. This expansion adds cryptography, compression codecs, text processing, validation, analytics, flow control, CAS-specific operations, numeric operations, and encoding functions. All functions implement the `StreamingComputeFunction` trait, process data chunk-by-chunk through bounded channels, and integrate with the existing pipeline infrastructure.

## Glossary

- **Pipeline**: A directed acyclic graph of streaming compute stages connected by bounded mpsc channels
- **StreamingComputeFunction**: The trait all streaming functions implement, providing `stream_execute()`, `supports_streaming()`, `preferred_chunk_size()`, and `channel_capacity()`
- **StreamChunk**: An enum (`Data(Bytes)`, `End`, `Error(DerivaError)`) representing a unit of data flowing through the pipeline
- **Function_Registry**: The central registry where all streaming functions are registered by snake_case name
- **spawn_map**: An existing helper that spawns a stateless per-chunk transform task
- **spawn_accumulate**: An existing helper that folds all chunks into state and emits a single result on End
- **spawn_boundary_map**: A new helper that buffers trailing partial lines for text functions operating on line boundaries
- **spawn_buffered**: A new helper that collects entire input into memory, applies a transform, and re-emits as chunked output
- **spawn_passthrough**: A new helper that forwards chunks unchanged while maintaining side-state
- **Fusible_Function**: A pure stateless map function eligible for fusion into a single pipeline task (per §2.11)
- **CAddr**: A content address (SHA-256 hash) uniquely identifying a value in the CAS-DFS system
- **Backpressure**: Flow control mechanism where bounded channel capacity causes upstream producers to block when downstream is slow
- **Chunk_Boundary**: The point where a chunk ends, which may split multi-byte characters, lines, or patterns

## Requirements

### Requirement 1: Compression Codec Functions

**User Story:** As a pipeline author, I want multiple compression codec options (zstd, lz4, snappy, brotli), so that I can choose the optimal speed/ratio tradeoff for each workload.

#### Acceptance Criteria

1. WHEN a Data chunk is received, THE StreamingZstdCompress function SHALL compress the chunk using Zstandard at the level specified by the `level` parameter (1–22, default 3) and emit the compressed chunk
2. WHEN a Data chunk is received, THE StreamingZstdDecompress function SHALL decompress the chunk as a standalone zstd frame and emit the decompressed chunk
3. WHEN a Data chunk is received, THE StreamingLz4Compress function SHALL compress the chunk using LZ4 block compression with prepended size and emit the compressed chunk
4. WHEN a Data chunk is received, THE StreamingLz4Decompress function SHALL decompress the chunk using LZ4 size-prepended format and emit the decompressed chunk
5. WHEN a Data chunk is received, THE StreamingSnappyCompress function SHALL compress the chunk using Snappy and emit the compressed chunk
6. WHEN a Data chunk is received, THE StreamingSnappyDecompress function SHALL decompress the chunk using Snappy and emit the decompressed chunk
7. WHEN a Data chunk is received, THE StreamingBrotliCompress function SHALL compress the chunk using Brotli at the quality specified by the `quality` parameter (0–11, default 4) and emit the compressed chunk
8. WHEN a Data chunk is received, THE StreamingBrotliDecompress function SHALL decompress the chunk as a standalone Brotli stream and emit the decompressed chunk
9. IF a compressed chunk contains corrupt data, THEN THE decompression function SHALL emit an Error chunk with a descriptive message including the codec name and error detail
10. FOR ALL compression codec pairs, compressing then decompressing a chunk SHALL produce bytes identical to the original chunk (round-trip property)

### Requirement 2: Cryptography Functions

**User Story:** As a pipeline author, I want streaming encryption, decryption, and keyed hashing, so that I can build secure data pipelines with authenticated encryption and integrity verification.

#### Acceptance Criteria

1. WHEN a Data chunk is received, THE StreamingEncrypt function SHALL encrypt the chunk using AES-256-CTR with the key and nonce provided as hex-encoded parameters, maintaining counter state across chunks
2. WHEN a Data chunk is received, THE StreamingDecrypt function SHALL decrypt the chunk using AES-256-CTR with the same key and nonce parameters, maintaining counter state across chunks
3. FOR ALL byte sequences, encrypting then decrypting with the same key and nonce SHALL produce bytes identical to the original (round-trip property)
4. WHEN a Data chunk is received, THE StreamingAeadEncrypt function SHALL encrypt the chunk using AES-256-GCM with a per-chunk nonce derived from `nonce_prefix || chunk_index_u32` and append the 16-byte authentication tag
5. WHEN a Data chunk is received, THE StreamingAeadDecrypt function SHALL strip the trailing 16-byte tag, decrypt using AES-256-GCM, and verify the authentication tag
6. IF the GCM authentication tag verification fails, THEN THE StreamingAeadDecrypt function SHALL emit an Error chunk with message indicating the failing chunk index
7. WHEN all Data chunks have been received, THE StreamingHmacSha256 function SHALL emit a 32-byte HMAC-SHA256 digest computed using the hex-encoded key parameter
8. WHEN all Data chunks have been received, THE StreamingMd5 function SHALL emit a 16-byte MD5 digest
9. WHEN all Data chunks have been received, THE StreamingSha512 function SHALL emit a 64-byte SHA-512 digest
10. WHEN all Data chunks have been received, THE StreamingBlake3 function SHALL emit a 32-byte BLAKE3 digest
11. IF a key parameter has invalid hex encoding or incorrect length (not 32 bytes for AES-256), THEN THE cryptography function SHALL emit an Error chunk with a message specifying the expected key length
12. IF a nonce parameter has invalid hex encoding or incorrect length, THEN THE cryptography function SHALL emit an Error chunk with a message specifying the expected nonce length

### Requirement 3: Text Processing Functions

**User Story:** As a pipeline author, I want line-oriented text processing functions (grep, sed, replace, line prefix, charset convert), so that I can build log processing and text transformation pipelines.

#### Acceptance Criteria

1. WHEN a Data chunk is received, THE StreamingGrep function SHALL emit only lines matching the regex specified in the `pattern` parameter, preserving line terminators
2. WHEN the `invert` parameter is set to `"true"`, THE StreamingGrep function SHALL emit only lines that do not match the pattern
3. WHEN a Data chunk is received, THE StreamingSed function SHALL apply regex find-and-replace on each line using the `pattern` and `replacement` parameters, supporting capture group references
4. WHEN a Data chunk is received, THE StreamingReplace function SHALL replace all occurrences of the `find` parameter with the `replace` parameter within each line
5. WHEN a Data chunk is received, THE StreamingLinePrefix function SHALL prepend the `prefix` parameter string after every newline character and at the start of the stream
6. WHEN a Data chunk is received, THE StreamingTruncateLines function SHALL truncate each line to the byte length specified by `max_line_bytes` (default 1024), preserving the newline terminator
7. WHEN a Data chunk is received, THE StreamingCharsetConvert function SHALL convert bytes from the encoding specified in `from` parameter to the encoding specified in `to` parameter (default "utf8")
8. WHEN a line is split across a chunk boundary, THE text processing function SHALL buffer the trailing partial line and prepend it to the next chunk before processing
9. IF the `pattern` parameter contains an invalid regex, THEN THE text function SHALL emit an Error chunk with a message describing the regex compilation failure
10. WHEN a Data chunk is received, THE StreamingRedact function SHALL replace all matches of the comma-separated regex patterns with `[REDACTED]`

### Requirement 4: Validation Functions

**User Story:** As a pipeline author, I want streaming validation functions, so that I can verify data format, integrity, and size constraints before downstream processing.

#### Acceptance Criteria

1. WHEN all Data chunks have been buffered, THE StreamingJsonValidate function SHALL parse the input as JSON and emit the original bytes unchanged if valid
2. IF the buffered input is not valid JSON, THEN THE StreamingJsonValidate function SHALL emit an Error chunk with a message including the parse error location (line and column)
3. WHEN all Data chunks have been buffered, THE StreamingSchemaValidate function SHALL validate the parsed JSON against the JSON Schema provided in the `schema` parameter and emit original bytes unchanged if valid
4. WHEN the first Data chunk is received, THE StreamingMagicBytes function SHALL verify that the chunk begins with the hex-encoded byte sequence specified in the `expected` parameter
5. IF the first chunk does not start with the expected magic bytes, THEN THE StreamingMagicBytes function SHALL emit an Error chunk with a message including expected and actual bytes
6. WHILE chunks are being received, THE StreamingSizeLimit function SHALL track cumulative byte count and emit an Error chunk immediately when the `max_bytes` threshold is exceeded
7. WHEN all Data chunks have been received, THE StreamingChecksumVerify function SHALL compare the computed CRC32 checksum against the `expected_crc32` hex parameter and emit an Error if they differ
8. WHEN all Data chunks have been received, THE StreamingSha256Verify function SHALL compare the computed SHA-256 hash against the `expected_hash` hex parameter and emit an Error if they differ
9. WHILE the stream is being validated by StreamingChecksumVerify or StreamingSha256Verify, THE function SHALL forward all Data chunks to downstream unchanged during accumulation
10. WHEN at least one Data chunk is expected, THE StreamingNonEmpty function SHALL emit an Error chunk on End if zero Data chunks were received

### Requirement 5: Analytics Functions

**User Story:** As a pipeline author, I want streaming analytics functions (line count, word count, histogram, sampling, dedup, sort), so that I can build data analysis pipelines over large streams.

#### Acceptance Criteria

1. WHEN all Data chunks have been received, THE StreamingLineCount function SHALL emit the total count of newline characters as a u64 in 8-byte big-endian format
2. WHEN all Data chunks have been received, THE StreamingWordCount function SHALL emit the total count of whitespace-delimited words as a u64 in 8-byte big-endian format, tracking word boundaries across chunks
3. WHEN all Data chunks have been received, THE StreamingMinMax function SHALL emit 2 bytes representing the minimum and maximum byte values observed
4. WHEN all Data chunks have been received, THE StreamingHistogram function SHALL emit 2048 bytes representing 256 u64 big-endian byte-frequency counts
5. WHEN a Data chunk is received, THE StreamingSample function SHALL emit only every Nth chunk as specified by the `rate` parameter (default 10)
6. WHEN a Data chunk is received, THE StreamingHead function SHALL forward the first N chunks as specified by the `chunks` parameter (default 1) and emit End after the Nth chunk
7. WHEN all Data chunks have been received, THE StreamingTail function SHALL emit only the last N chunks as specified by the `chunks` parameter (default 1), buffered in a ring buffer
8. WHEN a Data chunk is received, THE StreamingDeduplicate function SHALL drop the chunk if its xxhash64 digest matches any previously seen chunk
9. WHEN all Data chunks have been buffered, THE StreamingSort function SHALL sort chunks by lexicographic byte order and re-emit them in sorted order
10. WHEN all Data chunks have been buffered, THE StreamingUnique function SHALL deduplicate chunks by content, sort them, and re-emit unique chunks in sorted order
11. WHEN a Data chunk is received, THE StreamingFilter function SHALL forward the chunk only if it satisfies the predicate specified in the `predicate` parameter ("non_empty", "contains:PATTERN", or "min_size:N")

### Requirement 6: Flow Control Functions

**User Story:** As a pipeline author, I want flow control functions (rate limit, timeout, retry, tee, merge, partition, broadcast), so that I can build resilient fan-out/fan-in pipelines with throttling and error recovery.

#### Acceptance Criteria

1. WHILE chunks are being forwarded, THE StreamingRateLimit function SHALL throttle throughput to the bytes-per-second limit specified by the `bytes_per_sec` parameter (default 1048576)
2. WHEN a Data chunk is received, THE StreamingDelay function SHALL wait the number of milliseconds specified by `delay_ms` (default 100) before forwarding the chunk
3. IF no Data chunk is received within the milliseconds specified by `timeout_ms` (default 5000), THEN THE StreamingTimeout function SHALL emit an Error chunk indicating the timeout duration
4. IF the upstream emits an Error, THEN THE StreamingRetry function SHALL restart the upstream function up to `max_retries` times (default 3) before propagating the Error
5. WHEN a Data chunk is received, THE StreamingTee function SHALL clone the chunk to N output receivers as specified by the `outputs` parameter (default 2)
6. WHEN multiple input streams provide Data chunks, THE StreamingMerge function SHALL emit chunks in arrival order from whichever input is ready first
7. WHEN a Data chunk is received, THE StreamingBroadcast function SHALL send the chunk to all output receivers, blocking until the slowest consumer accepts
8. WHEN a Data chunk is received, THE StreamingPartition function SHALL route the chunk to output A if the `predicate` is satisfied, or output B otherwise
9. WHILE chunks are accumulating, THE StreamingBatch function SHALL collect N chunks as specified by `batch_size` (default 4) and emit them concatenated as a single chunk
10. WHEN a Data chunk is received within a debounce window, THE StreamingDebounce function SHALL emit only the last chunk received within the `window_ms` period (default 100)

### Requirement 7: CAS-Specific Operations

**User Story:** As a pipeline author, I want CAS-specific streaming functions (CAddr embed/verify, Merkle tree, content type detection, diff/patch), so that I can leverage content addressing within pipelines.

#### Acceptance Criteria

1. WHILE Data chunks are being received, THE StreamingCAddrEmbed function SHALL forward all chunks unchanged while computing a SHA-256 hash, and emit the 32-byte CAddr as a final Data chunk before End
2. WHILE Data chunks are being received, THE StreamingCAddrVerify function SHALL forward all chunks unchanged while computing a SHA-256 hash, and emit an Error on End if the hash does not match the `expected_caddr` hex parameter
3. WHEN all Data chunks have been received, THE StreamingMerkleTree function SHALL compute a SHA-256 hash per chunk as leaves, build a binary Merkle tree bottom-up, and emit the 32-byte root hash on End
4. WHEN the first Data chunk is received, THE StreamingContentType function SHALL detect the MIME type from magic bytes and prepend a metadata chunk containing `Content-Type: <mime>\n` before forwarding
5. WHEN Data chunks are received from two input streams, THE StreamingDiff function SHALL XOR corresponding bytes from each stream pairwise and emit the resulting diff chunk
6. WHEN Data chunks are received from two input streams (base and diff), THE StreamingPatch function SHALL XOR the diff onto the base and emit the patched chunk
7. WHEN a Data chunk is received, THE StreamingChunkHash function SHALL append a 32-byte SHA-256 hash of the chunk's content to the chunk itself

### Requirement 8: Numeric Operations

**User Story:** As a pipeline author, I want numeric streaming functions (sum, average, bitwise ops, rolling hash, entropy), so that I can build numeric processing and statistical analysis pipelines.

#### Acceptance Criteria

1. WHEN all Data chunks have been received, THE StreamingSum function SHALL parse newline-delimited decimal numbers, compute their sum, and emit the result as an ASCII decimal string
2. WHEN all Data chunks have been received, THE StreamingAverage function SHALL parse newline-delimited decimal numbers, compute sum divided by count, and emit the result as an ASCII decimal string
3. IF zero valid numbers are found, THEN THE StreamingAverage function SHALL emit an Error chunk indicating no valid numbers were present
4. WHEN a Data chunk is received, THE StreamingBitwiseAnd function SHALL AND each byte with the `mask` parameter (u8, default 0xFF) and emit the result
5. WHEN a Data chunk is received, THE StreamingBitwiseOr function SHALL OR each byte with the `mask` parameter (u8, default 0x00) and emit the result
6. WHEN a Data chunk is received, THE StreamingBitwiseNot function SHALL complement each byte and emit the result
7. WHEN a Data chunk is received, THE StreamingByteSwap function SHALL swap adjacent byte pairs (big-endian to little-endian u16 words) and emit the result
8. WHEN all Data chunks have been received, THE StreamingEntropy function SHALL compute Shannon entropy over the byte frequency distribution and emit the result as an ASCII decimal string (bits per byte, 0.0–8.0)
9. WHEN a Data chunk is received, THE StreamingRollingHash function SHALL compute a rolling hash over a sliding window and emit the hash values appended to each chunk

### Requirement 9: Encoding and Format Conversion Functions

**User Story:** As a pipeline author, I want encoding conversion functions (hex, base32, UTF-8 validation, line endings, JSON formatting), so that I can transform data between formats within pipelines.

#### Acceptance Criteria

1. WHEN a Data chunk is received, THE StreamingHexEncode function SHALL encode each byte as 2 lowercase hex ASCII characters and emit the result (output is 2× input size)
2. WHEN a Data chunk is received, THE StreamingHexDecode function SHALL decode pairs of hex ASCII characters into bytes and emit the result
3. IF a chunk contains an odd number of hex characters or an invalid hex character, THEN THE StreamingHexDecode function SHALL emit an Error chunk with a message including the invalid character and its position
4. WHEN a Data chunk is received, THE StreamingBase32Encode function SHALL encode the chunk as Base32 ASCII and emit the result
5. WHEN a Data chunk is received, THE StreamingBase32Decode function SHALL decode Base32 ASCII input into bytes and emit the result
6. IF a chunk contains invalid Base32 characters, THEN THE StreamingBase32Decode function SHALL emit an Error chunk with a descriptive message
7. WHEN a Data chunk is received, THE StreamingUtf8Validate function SHALL validate the chunk as UTF-8, buffering up to 3 trailing bytes for split multi-byte sequences, and emit validated bytes
8. IF the chunk contains an invalid UTF-8 byte sequence, THEN THE StreamingUtf8Validate function SHALL emit an Error chunk with a message including the invalid byte value and its position
9. WHEN a Data chunk is received, THE StreamingLineEnding function SHALL convert line endings to the format specified by the `target` parameter ("lf" for `\r\n` → `\n`, "crlf" for `\n` → `\r\n`)
10. WHEN all Data chunks have been buffered, THE StreamingJsonPrettyPrint function SHALL parse the input as JSON and re-emit it with indented formatting
11. WHEN all Data chunks have been buffered, THE StreamingJsonMinify function SHALL parse the input as JSON and re-emit it with all unnecessary whitespace removed
12. WHEN all Data chunks have been buffered, THE StreamingJsonLines function SHALL parse the input as a JSON array and re-emit each element as a single line terminated by newline (NDJSON format)
13. FOR ALL valid JSON inputs, pretty-printing then minifying SHALL produce valid JSON equivalent to the original (round-trip property)

### Requirement 10: Helper Pattern Infrastructure

**User Story:** As a function implementer, I want reusable helper patterns (boundary_map, buffered, passthrough), so that new streaming functions can be implemented consistently with minimal boilerplate.

#### Acceptance Criteria

1. THE spawn_boundary_map helper SHALL buffer trailing bytes after the last newline in each chunk and prepend them to the next chunk before applying the transform function
2. WHEN an End chunk is received, THE spawn_boundary_map helper SHALL flush any remaining buffered bytes through the transform function before emitting End
3. THE spawn_buffered helper SHALL collect all Data chunks into a single contiguous buffer, apply the transform function once, and re-emit the result as chunks of `chunk_size` bytes
4. IF the transform function within spawn_buffered returns an error, THEN THE helper SHALL emit an Error chunk with the transform's error message
5. THE spawn_passthrough helper SHALL forward each Data chunk according to the PassAction returned by the `on_chunk` callback (Forward, Replace, Drop, or Error)
6. WHEN an End chunk is received, THE spawn_passthrough helper SHALL invoke the `on_end` callback and emit any returned bytes as a final Data chunk before End
7. WHEN an Error chunk is received from upstream, THE helper function SHALL propagate the Error chunk downstream and terminate the task
8. WHEN the downstream receiver is dropped, THE helper function SHALL terminate silently without logging an error

### Requirement 11: Function Registration and Feature Gating

**User Story:** As a system integrator, I want all new streaming functions registered in the Function_Registry behind a feature flag, so that the base binary remains lean and functions are discoverable by name.

#### Acceptance Criteria

1. THE Function_Registry SHALL contain entries for all 80 new streaming functions using snake_case names matching the struct name pattern (e.g., `streaming_encrypt` for StreamingEncrypt)
2. WHERE the `extended-streaming` Cargo feature is enabled, THE Function_Registry SHALL include all 80 new streaming functions
3. WHERE the `extended-streaming` Cargo feature is not enabled, THE Function_Registry SHALL exclude all 80 new streaming functions and include only the original 20
4. THE Function_Registry SHALL resolve a streaming function by name in O(1) time using a HashMap lookup
5. WHEN a registered streaming function is looked up by name, THE Function_Registry SHALL return a reference that implements StreamingComputeFunction

### Requirement 12: Fusibility Declarations

**User Story:** As a pipeline optimizer, I want fusible functions to declare their fusibility via `is_fusible()`, so that the pipeline fusion engine (§2.11) can merge adjacent fusible stages into a single task.

#### Acceptance Criteria

1. THE following 12 new functions SHALL return `true` from `is_fusible()`: StreamingHexEncode, StreamingHexDecode, StreamingBase32Encode, StreamingBase32Decode, StreamingBitwiseAnd, StreamingBitwiseOr, StreamingBitwiseNot, StreamingPad, StreamingTrim, StreamingLineEnding, StreamingByteSwap, StreamingChunkHash
2. THE remaining 68 new functions SHALL return `false` from `is_fusible()`
3. WHEN two adjacent pipeline stages are both fusible, THE Pipeline_Fusion_Engine SHALL be able to merge them into a single task without changing observable output

### Requirement 13: Error Handling Contract

**User Story:** As a pipeline author, I want consistent error propagation across all streaming functions, so that failures are reported predictably and pipelines terminate cleanly.

#### Acceptance Criteria

1. WHEN an upstream Error chunk is received, THE streaming function SHALL propagate the Error chunk downstream and terminate its task
2. WHEN an internal computation error occurs, THE streaming function SHALL emit an Error chunk with a `ComputeFailed` message that includes the function name and a descriptive error detail
3. WHEN the downstream receiver channel is closed, THE streaming function SHALL terminate its task silently
4. THE Error chunk message SHALL include sufficient context for diagnosis: function name, error type, and relevant position or parameter information
5. IF a required parameter is missing from the params HashMap, THEN THE streaming function SHALL emit an Error chunk specifying the missing parameter name

### Requirement 14: Configuration via Parameters

**User Story:** As a pipeline author, I want all function-specific configuration passed through the params HashMap with documented keys and defaults, so that functions are configurable without code changes.

#### Acceptance Criteria

1. THE streaming function SHALL read configuration from the `params: &HashMap<String, String>` argument passed to `stream_execute()`
2. WHEN a parameter has a documented default value, THE streaming function SHALL use that default if the parameter key is absent from the HashMap
3. WHEN a parameter value cannot be parsed (e.g., non-numeric string for a numeric parameter), THE streaming function SHALL emit an Error chunk specifying the parameter name and the parse failure
4. THE cryptography functions SHALL accept `key` as a hex-encoded string and validate its length before processing any chunks
5. THE compression functions SHALL accept `level` or `quality` as an integer string within the documented range

### Requirement 15: Test Coverage

**User Story:** As a developer, I want comprehensive tests for each streaming function covering correctness, empty input, error propagation, and streaming-batch equivalence, so that all functions are verified to work correctly.

#### Acceptance Criteria

1. THE test suite SHALL include at least one correctness test per function verifying that known input produces expected output
2. THE test suite SHALL include an empty-input test per function verifying that an immediate End chunk is handled without panicking
3. THE test suite SHALL include an error-propagation test per function verifying that an upstream Error chunk is forwarded downstream
4. FOR ALL compression codec pairs and encryption pairs, THE test suite SHALL include a round-trip test verifying that encode then decode produces the original input for arbitrary byte sequences
5. FOR ALL accumulator functions, THE test suite SHALL verify that processing input as a single chunk produces the same result as processing it split across multiple chunks (streaming-batch equivalence)
6. THE test suite SHALL verify that fusible functions produce identical output whether executed standalone or fused with an adjacent fusible function
