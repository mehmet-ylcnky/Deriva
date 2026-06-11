# Requirements Document

## Introduction

Phase 2.16 of the Computation-Addressed Distributed File System (Deriva/CAS-DFS) adds a format-aware function library providing 283 functions (IDs 201–483) across 19 format categories. Unlike the core libraries (§2.14 streaming, §2.15 batch) which operate on raw bytes, these functions understand file format structure — enabling column projection from Parquet, CSV row filtering, image resizing, erasure coding, config merging, and more. Functions implement the same `ComputeFunction` and/or `StreamingComputeFunction` traits, are registered in the same FunctionRegistry, and use `BTreeMap<String, Value>` for all parameters. Heavy dependencies are gated behind Cargo feature flags with a 4-phase rollout.

## Glossary

- **Format_Category**: A grouping of related format-aware functions (e.g., CSV/JSON, columnar, archive, image) identified by a letter prefix (A–S)
- **Feature_Flag**: A Cargo feature that conditionally compiles a format category and its dependencies, keeping the default build lean
- **Phase_Rollout**: The 4-phase delivery plan: Phase 1 (lightweight formats), Phase 2 (heavy columnar/serialization/erasure), Phase 3 (media/documents), Phase 4 (domain-specific)
- **Function_ID**: A numeric identifier in the range 201–483 assigned to each format-aware function, non-overlapping with core library IDs (1–100)
- **ComputeFunction**: The batch compute trait from §2.15 that processes full Bytes inputs
- **StreamingComputeFunction**: The streaming compute trait from §2.14 that processes chunked input/output
- **BTreeMap_Params**: The standard parameter passing mechanism using `BTreeMap<String, Value>` for all function configuration (column names, filter expressions, quality levels, etc.)
- **Predicate_Pushdown**: The technique of applying filter conditions during format parsing to avoid materializing excluded data
- **Column_Projection**: Reading only specified columns from a columnar format (Parquet, ORC, Arrow) rather than the full row
- **Schema_Evolution**: The ability to read data written with an older or newer schema by matching fields by name or position
- **Erasure_Coding**: A redundancy technique that encodes data into N data + M parity shards, tolerating up to M shard losses
- **Content_Detection**: Identifying the format of a blob by inspecting magic bytes, file headers, or content structure

## Requirements

### Requirement 1: Same Trait and Registry Integration

**User Story:** As a system developer, I want format-aware functions to implement the same traits and register in the same registry as core functions, so that they are usable in pipelines without any new abstractions or API changes.

#### Acceptance Criteria

1. EVERY format-aware function SHALL implement `ComputeFunction` and/or `StreamingComputeFunction` from the existing trait definitions in §2.14/§2.15
2. EVERY format-aware function SHALL be registered in the existing `FunctionRegistry` using `register()` and/or `register_streaming()` methods
3. EVERY format-aware function SHALL accept parameters exclusively via `BTreeMap<String, Value>` consistent with core library conventions
4. EVERY format-aware function SHALL have a unique Function_ID in the range 201–483, non-overlapping with core library IDs (1–100) and reserved range (101–200)
5. WHEN a format-aware function is registered, THE FunctionRegistry SHALL treat it identically to core functions for mode selection (§2.9), pipeline fusion (§2.11), and all other pipeline optimizations

### Requirement 2: Feature Flag Gating

**User Story:** As a system operator, I want format-aware functions behind Cargo feature flags, so that the default build remains lean and only desired format categories add their dependencies.

#### Acceptance Criteria

1. THE Cargo.toml SHALL define a `format-phase1` feature that includes lightweight format categories (CSV/JSON, archive, config, logs, CAS/IPFS, detection)
2. THE Cargo.toml SHALL define `format-phase2`, `format-phase3`, and `format-phase4` features for progressively heavier dependency sets
3. THE Cargo.toml SHALL define a `format-all` feature that enables all four phases
4. THE Cargo.toml SHALL define per-category features (e.g., `format-csv`, `format-columnar`, `format-image`) for fine-grained control
5. THE default feature set SHALL include `format-phase1` only
6. WHEN a feature flag is disabled, THE corresponding format-aware functions SHALL not be compiled, registered, or available in the FunctionRegistry
7. WHEN a feature flag is enabled, THE corresponding functions SHALL be automatically registered during FunctionRegistry initialization

### Requirement 3: Mode Distribution (Batch/Streaming/Both)

**User Story:** As a system developer, I want format-aware functions to support the appropriate execution mode based on format characteristics, so that random-access formats use batch and sequential formats use streaming.

#### Acceptance Criteria

1. FORMAT-AWARE functions operating on random-access formats (Parquet footer, ZIP central directory, PDF page tree, SQLite B-tree) SHALL be implemented as batch-only (ComputeFunction)
2. FORMAT-AWARE functions operating on sequential formats (CSV rows, NDJSON lines, tar entries, log lines) SHALL be implemented as both batch and streaming
3. FORMAT-AWARE functions operating on pure stream codecs (gzip frames, bzip2 blocks) SHALL be implemented as streaming-only (StreamingComputeFunction)
4. WHEN both batch and streaming implementations exist for a function, THE §2.9 size-aware mode selection SHALL automatically choose the optimal path based on input size

### Requirement 4: Phase 1 — Lightweight Format Functions (CSV/JSON, Archive, Config, Logs, CAS, Detection)

**User Story:** As a system developer, I want Phase 1 format functions covering everyday formats, so that CSV parsing, JSON extraction, archive operations, config merging, log processing, and content detection are available with minimal dependencies.

#### Acceptance Criteria

1. THE Phase 1 library SHALL provide CSV functions: parse, project columns, filter rows, sort, aggregate, convert to/from JSON/NDJSON
2. THE Phase 1 library SHALL provide JSON/XML functions: path extraction (JSONPath/XPath), schema validation, pretty-print, minify, merge, diff
3. THE Phase 1 library SHALL provide archive functions: tar create/extract/list, zip create/extract/list, gzip/bzip2/xz compress/decompress (format-aware, not raw codec)
4. THE Phase 1 library SHALL provide config functions: YAML/TOML/INI parse, merge, path extraction, format conversion between config formats
5. THE Phase 1 library SHALL provide log functions: line filtering (regex), timestamp extraction, structured log parsing (JSON logs), field extraction
6. THE Phase 1 library SHALL provide CAS/IPFS functions: CID compute, CID verify, DAG-CBOR encode/decode, CAR archive create/extract, UnixFS chunking
7. THE Phase 1 library SHALL provide detection functions: format detection via magic bytes, MIME type inference, encoding detection (UTF-8/Latin-1/etc.)
8. THE Phase 1 library SHALL depend only on lightweight crates: csv, quick-xml, jsonpath-rust, tar, zip, flate2, serde_yaml, toml, cid, multihash, infer

### Requirement 5: Phase 2 — Columnar, Serialization, and Erasure Coding Functions

**User Story:** As a system developer, I want Phase 2 format functions for data engineering workloads, so that Parquet column projection, Avro schema evolution, Protobuf encode/decode, and erasure coding are available.

#### Acceptance Criteria

1. THE Phase 2 library SHALL provide columnar functions: Parquet column projection, row group filtering, schema extraction, statistics reading; Arrow IPC read/write; ORC read/write
2. THE Phase 2 library SHALL provide serialization functions: Avro encode/decode with schema evolution, Protobuf encode/decode, MessagePack encode/decode, CBOR encode/decode, BSON encode/decode
3. THE Phase 2 library SHALL provide erasure coding functions: Reed-Solomon encode (data → data+parity shards), decode (reconstruct from available shards), verify integrity
4. THE Parquet functions SHALL support predicate pushdown via row group statistics when a filter expression is provided in params
5. THE erasure coding functions SHALL accept params for data_shards count and parity_shards count

### Requirement 6: Phase 3 — Media and Document Functions

**User Story:** As a system developer, I want Phase 3 format functions for media processing, so that image resize, audio metadata extraction, PDF text extraction, and spreadsheet operations are available.

#### Acceptance Criteria

1. THE Phase 3 library SHALL provide image functions: resize, thumbnail, crop, rotate, format conversion (PNG↔JPEG↔WebP), metadata extraction (EXIF), perceptual hash
2. THE Phase 3 library SHALL provide audio/video functions: metadata extraction, duration, codec identification, audio waveform summary
3. THE Phase 3 library SHALL provide document functions: PDF text extraction, PDF page count, Excel/CSV read, Markdown to HTML conversion, HTML text extraction

### Requirement 7: Phase 4 — Domain-Specific Functions

**User Story:** As a system developer, I want Phase 4 format functions for specialized domains, so that geospatial, scientific, database, ML, network, and bioinformatics workloads can use format-native operations within the CAS.

#### Acceptance Criteria

1. THE Phase 4 library SHALL provide geospatial functions: GeoJSON feature extraction, coordinate transformation, bounding box filter, FlatGeoBuf read
2. THE Phase 4 library SHALL provide scientific functions: HDF5 dataset read/slice, NumPy array load/save, FITS header extraction
3. THE Phase 4 library SHALL provide database functions: SQLite query execution, table export to CSV/JSON, schema extraction
4. THE Phase 4 library SHALL provide ML functions: SafeTensors metadata/tensor extraction, ONNX model metadata, WASM module validation
5. THE Phase 4 library SHALL provide network functions: PCAP packet parsing/filtering, email (MIME) parsing, HTTP archive (HAR) extraction
6. THE Phase 4 library SHALL provide bioinformatics functions: FASTA/FASTQ read/filter, BAM region extraction, VCF variant filtering

### Requirement 8: Parameter Convention Consistency

**User Story:** As a system developer, I want all format-aware functions to follow consistent parameter naming conventions, so that pipeline authors can predict parameter names without consulting documentation for each function.

#### Acceptance Criteria

1. ALL column/field selection parameters SHALL use the key name `columns` with a comma-separated string value (e.g., `"name,age,city"`)
2. ALL filter/predicate parameters SHALL use the key name `filter` with an expression string value (e.g., `"age > 21"`)
3. ALL output format parameters SHALL use the key name `output_format` with a format identifier string (e.g., `"json"`, `"csv"`, `"parquet"`)
4. ALL quality/compression level parameters SHALL use the key name `quality` with a numeric string value (e.g., `"85"` for JPEG quality)
5. ALL schema path parameters SHALL use the key name `path` with a dot-notation or JSONPath string (e.g., `"$.data.items"`)
6. WHEN a required parameter is missing, THE function SHALL return a descriptive error indicating which parameter is required and its expected format

### Requirement 9: Error Handling for Malformed Input

**User Story:** As a system developer, I want format-aware functions to produce clear errors when input data does not match the expected format, so that pipeline failures are diagnosable without external tools.

#### Acceptance Criteria

1. WHEN a format-aware function receives input that cannot be parsed as the expected format, THE function SHALL return a `DerivaError::Compute` error with a message indicating the expected format, the parse failure location (byte offset or line number when available), and the nature of the error
2. WHEN a JSON path extraction receives valid JSON but the specified path does not exist, THE function SHALL return an empty Bytes result rather than an error (path not found is not a parse failure)
3. WHEN a columnar projection receives a column name not present in the schema, THE function SHALL return an error listing the requested column and the available columns
4. WHEN an archive extraction receives a valid archive but the specified entry does not exist, THE function SHALL return an error listing the requested entry and available entries

### Requirement 10: Function Count and Category Coverage

**User Story:** As a system developer, I want the format-aware library to provide comprehensive coverage across 19 categories, so that common format operations are available without external tooling.

#### Acceptance Criteria

1. THE format-aware library SHALL provide a total of at least 283 functions across phases 1–4
2. THE format-aware library SHALL cover at least 19 distinct format categories
3. EACH format category SHALL provide at least 5 functions covering common operations for that format domain
4. THE function distribution SHALL be approximately: 52% batch-only, 38% both batch+streaming, 10% streaming-only, reflecting format access pattern requirements

### Requirement 11: Testing Strategy

**User Story:** As a system developer, I want each format-aware function to have unit tests with real format samples, so that correctness is verified against actual file format specimens.

#### Acceptance Criteria

1. EACH format-aware function SHALL have at least one unit test exercising the happy path with a valid format sample
2. EACH format-aware function SHALL have at least one unit test exercising the error path with invalid or malformed input
3. THE test suite SHALL include small embedded test fixtures (< 10KB each) for lightweight formats (CSV, JSON, TOML, YAML)
4. THE test suite SHALL use `include_bytes!` for binary format fixtures (Parquet, PNG, ZIP) stored in a `tests/fixtures/` directory
5. WHEN a feature flag is disabled, THE corresponding tests SHALL be conditionally compiled out via `#[cfg(feature = "...")]`
