# Implementation Plan: Format-Aware Function Library

## Overview

Implement the Phase 2.16 format-aware function library providing 283 functions (IDs 201–483) across 19 format categories. Functions implement the existing `ComputeFunction` and/or `StreamingComputeFunction` traits and register in the same `FunctionRegistry`. Implementation follows the 4-phase rollout with Cargo feature flags gating heavy dependencies. All code is Rust, located in `crates/deriva-compute/src/`.

## Tasks

- [ ] 1. Set up feature flag infrastructure and registration scaffolding
  - [ ] 1.1 Define Cargo feature flags in `crates/deriva-compute/Cargo.toml`
    - Add per-category features: `format-csv`, `format-archive`, `format-config`, `format-log`, `format-cas`, `format-detect`, `format-columnar`, `format-serialization`, `format-erasure`, `format-image`, `format-media`, `format-document`, `format-geo`, `format-scientific`, `format-database`, `format-ml`, `format-network`, `format-bio`, `format-binary`
    - Add phase aggregate features: `format-phase1`, `format-phase2`, `format-phase3`, `format-phase4`, `format-all`
    - Set default features to include `format-phase1`
    - Add conditional dependencies for each feature (csv, quick-xml, jsonpath-rust, tar, zip, flate2, serde_yaml, toml, cid, multihash, infer for Phase 1)
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [ ] 1.2 Create registration dispatcher in `crates/deriva-compute/src/builtins/mod.rs`
    - Add conditional calls to `register_*_functions()` inside `register_all()` gated by `#[cfg(feature = "format-*")]`
    - Ensure disabled features produce no compiled code or registry entries
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ] 1.3 Create shared format utility module `crates/deriva-compute/src/builtins_format_common.rs`
    - Add shared parameter parsing helpers: `parse_columns_param`, `parse_filter_param`, `parse_output_format_param`, `parse_quality_param`, `parse_path_param`
    - Add shared error formatting helpers following the convention: `"{function}: {error_detail}"`
    - Add shared cost estimation helpers
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 9.1_

  - [ ]* 1.4 Write unit tests for feature flag isolation
    - Test that disabled features do not register functions (FunctionNotFound for disabled categories)
    - Test that enabled features register the expected count of functions
    - _Requirements: 2.6, 2.7_

- [ ] 2. Implement Phase 1 — Category S: Format Detection (9 functions, IDs 475–483)
  - [ ] 2.1 Extend existing `builtins_format_detect.rs` with remaining detection functions
    - Implement: `encoding_detect`, `is_text`, `is_binary`, `is_compressed`, `is_encrypted`, `byte_histogram`, `entropy_score`, `structure_heuristic` (some already exist, extend with missing ones)
    - Assign IDs 475–483, implement `ComputeFunction` trait for each
    - Return descriptive errors for empty input
    - _Requirements: 4.7, 1.1, 1.4, 10.3_

  - [ ]* 2.2 Write property test for format detection accuracy
    - **Property 9: Detection accuracy**
    - **Validates: Requirements 4.7**

  - [ ]* 2.3 Write unit tests for detection functions
    - Test magic byte detection for PNG, JPEG, PDF, ZIP, tar, gzip
    - Test encoding detection for UTF-8, Latin-1, UTF-16
    - Test is_text/is_binary classification
    - _Requirements: 4.7, 11.1, 11.2_

- [ ] 3. Implement Phase 1 — Category B: CSV/JSON/XML (25 functions, IDs 219–243)
  - [ ] 3.1 Create `crates/deriva-compute/src/builtins_format_csv.rs` with CSV functions
    - Implement: `csv_parse`, `csv_write`, `csv_schema_infer`, `csv_column_select`, `csv_column_rename`, `csv_filter`, `csv_sort`, `csv_aggregate`, `csv_join`, `csv_dedup`
    - Implement both `ComputeFunction` and `StreamingComputeFunction` for sequential operations (filter, column_select)
    - Use `columns` param key for column selection, `filter` param key for row filtering
    - _Requirements: 4.1, 3.2, 8.1, 8.2, 1.1, 1.3_

  - [ ] 3.2 Add JSON/NDJSON/XML functions to `builtins_format_csv.rs`
    - Implement: `ndjson_parse`, `ndjson_write`, `ndjson_filter`, `ndjson_project`, `json_path_extract`, `json_merge`, `json_flatten`, `json_unflatten`, `xml_parse`, `xml_write`, `xml_xpath_extract`, `xml_xslt_transform`, `xml_validate_dtd`, `xml_validate_xsd`
    - Assign IDs 219–243 across all functions
    - NDJSON functions implement both batch and streaming modes
    - _Requirements: 4.2, 3.2, 8.5, 1.1, 1.3, 1.4_

  - [ ] 3.3 Implement `register_csv_functions()` with conditional compilation
    - Register all 25 functions gated behind `#[cfg(feature = "format-csv")]`
    - Register streaming variants for CSV/NDJSON sequential functions
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 3.4 Write property test for CSV/JSON round-trip
    - **Property 3: Format round-trip**
    - Test: CSV write → CSV parse produces semantically equivalent data
    - Test: JSON serialize → JSON parse produces identical structure
    - **Validates: Requirements 4.1, 5.1, 5.2**

  - [ ]* 3.5 Write unit tests for CSV/JSON/XML functions
    - Test CSV parse with embedded test fixture (< 10KB)
    - Test JSONPath extraction with valid JSON and missing paths (returns empty Bytes)
    - Test XML validation with valid/invalid DTD
    - Test error on malformed CSV (unclosed quote)
    - _Requirements: 9.1, 9.2, 11.1, 11.2, 11.3_

- [ ] 4. Implement Phase 1 — Category D: Archive (20 functions, IDs 263–282)
  - [ ] 4.1 Create `crates/deriva-compute/src/builtins_format_archive.rs`
    - Implement tar functions: `tar_create`, `tar_extract`, `tar_list`, `tar_extract_entry`, `tar_append`
    - Implement zip functions: `zip_create`, `zip_extract`, `zip_list`, `zip_extract_entry`, `zip_add_file`
    - Implement format-aware compress/decompress: `gzip_compress`, `gzip_decompress`, `bzip2_compress`, `bzip2_decompress`, `xz_compress`, `xz_decompress`
    - Implement 7z read-only: `sevenz_list`, `sevenz_extract`
    - Assign IDs 263–282
    - tar create/extract and gzip/bzip2 operations support streaming mode
    - _Requirements: 4.3, 3.2, 1.1, 1.4_

  - [ ] 4.2 Implement `register_archive_functions()` with conditional compilation
    - Register all 20 functions gated behind `#[cfg(feature = "format-archive")]`
    - Register streaming variants for sequential archive operations
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 4.3 Write unit tests for archive functions
    - Test tar create → extract round-trip
    - Test zip create → list → extract single entry
    - Test error on corrupt tar header (descriptive error with offset)
    - Test error on ZIP with unsupported compression method
    - Test error on non-existent entry (lists available entries)
    - Use `include_bytes!` for binary fixtures in `tests/fixtures/`
    - _Requirements: 4.3, 9.1, 9.4, 11.1, 11.2, 11.4_

- [ ] 5. Implement Phase 1 — Category H: Config (16 functions, IDs 340–355)
  - [ ] 5.1 Create `crates/deriva-compute/src/builtins_format_config.rs`
    - Implement YAML functions: `yaml_parse`, `yaml_write`, `yaml_merge`, `yaml_path_extract`, `yaml_validate`
    - Implement TOML functions: `toml_parse`, `toml_write`, `toml_merge`, `toml_path_extract`
    - Implement INI functions: `ini_parse`, `ini_write`, `ini_merge`
    - Implement HCL functions: `hcl_parse`, `hcl_validate`
    - Implement cross-format: `yaml_to_json`, `toml_to_json`, `ini_to_json`
    - Assign IDs 340–355, all batch-only (random-access formats)
    - Use `path` param key for dot-notation path extraction
    - _Requirements: 4.4, 8.5, 1.1, 1.4_

  - [ ] 5.2 Implement `register_config_functions()` with conditional compilation
    - Register all 16 functions gated behind `#[cfg(feature = "format-config")]`
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 5.3 Write property test for config round-trip
    - **Property 3: Format round-trip (config subset)**
    - Test: YAML write → YAML parse, TOML write → TOML parse produce equivalent data
    - **Validates: Requirements 4.4**

  - [ ]* 5.4 Write unit tests for config functions
    - Test YAML/TOML/INI parse with embedded fixtures (< 10KB)
    - Test cross-format conversion: YAML→JSON→TOML round-trip
    - Test path extraction with valid/invalid paths
    - Test merge of two config files
    - _Requirements: 4.4, 9.1, 11.1, 11.2, 11.3_

- [ ] 6. Implement Phase 1 — Category K: Logs (13 functions, IDs 383–395)
  - [ ] 6.1 Create `crates/deriva-compute/src/builtins_format_log.rs`
    - Implement syslog functions: `syslog_parse`, `syslog_filter_severity`, `syslog_filter_facility`
    - Implement JSON log functions: `json_log_parse`, `json_log_filter_field`, `json_log_extract_timestamps`
    - Implement CEF functions: `cef_parse`, `cef_validate`
    - Implement generic functions: `log_line_grep`, `log_timestamp_normalize`, `log_field_extract`
    - Assign IDs 383–395
    - Log line processing supports both batch and streaming modes
    - Use `filter` param key for severity/field filtering
    - _Requirements: 4.5, 3.2, 8.2, 1.1, 1.4_

  - [ ] 6.2 Implement `register_log_functions()` with conditional compilation
    - Register all 13 functions gated behind `#[cfg(feature = "format-log")]`
    - Register streaming variants for line-oriented operations
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 6.3 Write unit tests for log functions
    - Test syslog parse with standard syslog line
    - Test JSON log field extraction
    - Test timestamp normalization across formats
    - Test regex-based field extraction
    - _Requirements: 4.5, 11.1, 11.2, 11.3_

- [ ] 7. Implement Phase 1 — Category L: CAS/IPFS (15 functions, IDs 396–410)
  - [ ] 7.1 Create `crates/deriva-compute/src/builtins_format_cas.rs`
    - Implement CID functions: `cid_compute`, `cid_verify`, `cid_extract_codec`, `cid_extract_hash`
    - Implement DAG-CBOR functions: `dag_cbor_encode`, `dag_cbor_decode`, `dag_cbor_patch`
    - Implement CAR functions: `car_create`, `car_extract`, `car_list_roots`, `car_verify`
    - Implement UnixFS functions: `unixfs_chunk_fixed`, `unixfs_chunk_rabin`, `unixfs_assemble`
    - Implement Merkle proof functions: `merkle_proof_generate`, `merkle_proof_verify`
    - Assign IDs 396–410, batch-only for CID/DAG operations, streaming for chunking
    - _Requirements: 4.6, 3.2, 1.1, 1.4_

  - [ ] 7.2 Implement `register_cas_functions()` with conditional compilation
    - Register all 15 functions gated behind `#[cfg(feature = "format-cas")]`
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 7.3 Write unit tests for CAS/IPFS functions
    - Test CID compute → verify round-trip
    - Test DAG-CBOR encode → decode round-trip
    - Test CAR create → extract → verify integrity
    - Test UnixFS chunking produces correct Merkle tree
    - _Requirements: 4.6, 11.1, 11.2_

- [ ] 8. Checkpoint — Phase 1 complete
  - Ensure all tests pass, ask the user if questions arise.
  - Verify 98 functions are registered when `format-phase1` is enabled
  - Verify no functions from Phase 2/3/4 are present with default features

- [ ] 9. Implement Phase 2 — Category A: Columnar (18 functions, IDs 201–218)
  - [ ] 9.1 Add Phase 2 dependencies to `Cargo.toml`
    - Add `parquet`, `arrow`, `apache-avro`, `prost`, `rmp-serde`, `ciborium`, `bson`, `reed-solomon-erasure` gated behind respective features
    - _Requirements: 5.1, 5.2, 5.3_

  - [ ] 9.2 Create `crates/deriva-compute/src/builtins_format_columnar.rs`
    - Implement Parquet functions: `parquet_read`, `parquet_write`, `parquet_metadata`, `parquet_column_project`, `parquet_filter`, `parquet_merge`, `parquet_statistics`, `parquet_partition_write`
    - Implement Arrow IPC functions: `arrow_ipc_read`, `arrow_ipc_write`, `arrow_ipc_schema`
    - Implement ORC functions: `orc_read`, `orc_write`, `orc_metadata`, `orc_filter`
    - Implement cross-format: `parquet_to_arrow`, `columnar_to_ndjson`
    - Assign IDs 201–218, all batch-only (random-access formats)
    - Implement predicate pushdown via row group statistics in `parquet_filter`
    - Use `columns` param key for projection, `filter` param key for predicates
    - _Requirements: 5.1, 5.4, 3.1, 8.1, 8.2, 1.1, 1.4_

  - [ ] 9.3 Implement `register_columnar_functions()` with conditional compilation
    - Register all 18 functions gated behind `#[cfg(feature = "format-columnar")]`
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 9.4 Write property test for column projection subset
    - **Property 5: Column projection subset**
    - Generate random column selections from a Parquet schema, verify output contains exactly requested columns
    - **Validates: Requirements 5.1**

  - [ ]* 9.5 Write property test for predicate pushdown correctness
    - **Property 4: Predicate pushdown correctness**
    - Generate random Parquet data and filter predicates, verify filtered rows satisfy predicate exactly (no false positives/negatives)
    - **Validates: Requirements 5.4**

  - [ ]* 9.6 Write unit tests for columnar functions
    - Test Parquet read/write round-trip with `include_bytes!` fixture
    - Test column projection with valid/invalid column names (error lists available columns)
    - Test row group filter with statistics-based pushdown
    - _Requirements: 5.1, 9.3, 11.1, 11.2, 11.4_

- [ ] 10. Implement Phase 2 — Category C: Serialization (19 functions, IDs 244–262)
  - [ ] 10.1 Create `crates/deriva-compute/src/builtins_format_serial.rs`
    - Implement Avro functions: `avro_read`, `avro_write`, `avro_schema_extract`, `avro_schema_evolve`, `avro_to_json`, `avro_to_parquet`
    - Implement Protobuf functions: `protobuf_decode`, `protobuf_encode`, `protobuf_schema_extract`
    - Implement Thrift functions: `thrift_decode`, `thrift_encode`
    - Implement MessagePack functions: `msgpack_decode`, `msgpack_encode`
    - Implement CBOR functions: `cbor_decode`, `cbor_encode`
    - Implement BSON functions: `bson_decode`, `bson_encode`
    - Assign IDs 244–262
    - Avro and CBOR support both batch and streaming modes
    - _Requirements: 5.2, 3.2, 1.1, 1.4_

  - [ ] 10.2 Implement `register_serialization_functions()` with conditional compilation
    - Register all 19 functions gated behind `#[cfg(feature = "format-serialization")]`
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 10.3 Write property test for serialization round-trip
    - **Property 3: Format round-trip (serialization subset)**
    - Test: Avro write → Avro read, CBOR encode → CBOR decode, MsgPack encode → decode
    - **Validates: Requirements 5.2**

  - [ ]* 10.4 Write unit tests for serialization functions
    - Test Avro schema evolution (read with newer/older schema)
    - Test Protobuf encode/decode with known descriptor
    - Test error on Avro schema mismatch (descriptive error)
    - _Requirements: 5.2, 9.1, 11.1, 11.2_

- [ ] 11. Implement Phase 2 — Category R: Erasure Coding (9 functions, IDs 466–474)
  - [ ] 11.1 Create `crates/deriva-compute/src/builtins_format_erasure.rs`
    - Implement: `erasure_encode`, `erasure_decode`, `erasure_verify`, `erasure_optimal_shards`, `erasure_validate_params`, `erasure_shard_size`, `erasure_parity_count`, `erasure_missing_shards`, `erasure_reconstruct`
    - Assign IDs 466–474, batch-only
    - Accept `data_shards` and `parity_shards` params
    - Return descriptive error when too many shards are missing
    - _Requirements: 5.3, 5.5, 9.1, 1.1, 1.4_

  - [ ] 11.2 Implement `register_erasure_functions()` with conditional compilation
    - Register all 9 functions gated behind `#[cfg(feature = "format-erasure")]`
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 11.3 Write property test for erasure coding round-trip
    - **Property 6: Erasure coding round-trip**
    - Generate random byte sequences and valid (data_shards, parity_shards) configs
    - Encode → drop up to parity_shards shards → decode, verify exact reconstruction
    - **Validates: Requirements 5.3**

  - [ ]* 11.4 Write unit tests for erasure coding functions
    - Test encode → decode with zero shard loss
    - Test encode → decode with maximum tolerable shard loss
    - Test error when too many shards missing (error message includes counts)
    - Test parameter validation (invalid shard counts)
    - _Requirements: 5.3, 5.5, 9.1, 11.1, 11.2_

- [ ] 12. Checkpoint — Phase 2 complete
  - Ensure all tests pass, ask the user if questions arise.
  - Verify 46 additional functions registered when `format-phase2` is enabled
  - Verify Parquet predicate pushdown works with row group statistics

- [ ] 13. Implement Phase 3 — Category E: Image (18 functions, IDs 283–300)
  - [ ] 13.1 Create `crates/deriva-compute/src/builtins_format_image.rs`
    - Add `image` crate dependency gated behind `format-image` feature
    - Implement: `image_resize`, `image_thumbnail`, `image_crop`, `image_rotate`, `image_convert_format`, `image_metadata_exif`, `image_perceptual_hash`, plus additional image utility functions
    - Assign IDs 283–300, all batch-only (random-access)
    - Use `quality` param key for JPEG quality, `output_format` for target format
    - _Requirements: 6.1, 3.1, 8.3, 8.4, 1.1, 1.4_

  - [ ] 13.2 Implement `register_image_functions()` with conditional compilation
    - Register all 18 functions gated behind `#[cfg(feature = "format-image")]`
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 13.3 Write unit tests for image functions
    - Test resize produces valid output image with correct dimensions
    - Test format conversion PNG→JPEG→WebP
    - Test EXIF metadata extraction
    - Test error on unsupported/corrupt image format
    - Use `include_bytes!` for image fixtures in `tests/fixtures/`
    - _Requirements: 6.1, 9.1, 11.1, 11.2, 11.4_

- [ ] 14. Implement Phase 3 — Category F: Audio/Video (16 functions, IDs 301–316)
  - [ ] 14.1 Create `crates/deriva-compute/src/builtins_format_media.rs`
    - Add `symphonia` dependency gated behind `format-media` feature
    - Implement: `audio_metadata`, `audio_duration`, `audio_codec_id`, `audio_waveform_summary`, `video_metadata`, `video_duration`, `video_codec_id`, and additional media utility functions
    - Assign IDs 301–316, all batch-only
    - _Requirements: 6.2, 3.1, 1.1, 1.4_

  - [ ] 14.2 Implement `register_media_functions()` with conditional compilation
    - Register all 16 functions gated behind `#[cfg(feature = "format-media")]`
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 14.3 Write unit tests for audio/video functions
    - Test audio metadata extraction from WAV/MP3 fixtures
    - Test duration calculation accuracy
    - Test error on corrupt/unsupported media
    - _Requirements: 6.2, 11.1, 11.2, 11.4_

- [ ] 15. Implement Phase 3 — Category G: Document (23 functions, IDs 317–339)
  - [ ] 15.1 Create `crates/deriva-compute/src/builtins_format_document.rs`
    - Add `lopdf`, `calamine`, `scraper`, `pulldown-cmark` dependencies gated behind `format-document` feature
    - Implement PDF functions: `pdf_extract_text`, `pdf_page_count`, `pdf_metadata`
    - Implement Excel/spreadsheet functions: `excel_read`, `excel_sheet_list`, `excel_to_csv`
    - Implement Markdown/HTML functions: `markdown_to_html`, `html_extract_text`, `html_extract_links`
    - Additional document utility functions to reach 23 total
    - Assign IDs 317–339, all batch-only (random-access documents)
    - _Requirements: 6.3, 3.1, 1.1, 1.4_

  - [ ] 15.2 Implement `register_document_functions()` with conditional compilation
    - Register all 23 functions gated behind `#[cfg(feature = "format-document")]`
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 15.3 Write unit tests for document functions
    - Test PDF text extraction against reference text
    - Test Excel read with small spreadsheet fixture
    - Test Markdown→HTML conversion
    - Test error on corrupt PDF (descriptive error with location)
    - _Requirements: 6.3, 9.1, 11.1, 11.2, 11.4_

- [ ] 16. Checkpoint — Phase 3 complete
  - Ensure all tests pass, ask the user if questions arise.
  - Verify 57 additional functions registered when `format-phase3` is enabled

- [ ] 17. Implement Phase 4 — Domain-specific functions (82 functions total)
  - [ ] 17.1 Create `crates/deriva-compute/src/builtins_format_geo.rs` (14 functions, IDs 356–369)
    - Add `geojson` dependency gated behind `format-geo` feature
    - Implement: `geojson_feature_extract`, `geo_coord_transform`, `geo_bbox_filter`, `flatgeobuf_read`, and additional geospatial functions
    - Assign IDs 356–369, all batch-only
    - _Requirements: 7.1, 1.1, 1.4_

  - [ ] 17.2 Create `crates/deriva-compute/src/builtins_format_scientific.rs` (13 functions, IDs 370–382)
    - Add `hdf5` and numpy-related dependencies gated behind `format-scientific` feature
    - Implement: `hdf5_dataset_read`, `hdf5_dataset_slice`, `numpy_load`, `numpy_save`, `fits_header_extract`, and additional scientific functions
    - Assign IDs 370–382, all batch-only
    - _Requirements: 7.2, 1.1, 1.4_

  - [ ] 17.3 Create `crates/deriva-compute/src/builtins_format_database.rs` (10 functions, IDs 411–420)
    - Add `rusqlite` dependency gated behind `format-database` feature
    - Implement: `sqlite_query`, `sqlite_table_export_csv`, `sqlite_table_export_json`, `sqlite_schema_extract`, and additional database functions
    - Assign IDs 411–420, all batch-only
    - _Requirements: 7.3, 1.1, 1.4_

  - [ ] 17.4 Create `crates/deriva-compute/src/builtins_format_ml.rs` (12 functions, IDs 421–432)
    - Add `safetensors` dependency gated behind `format-ml` feature
    - Implement: `safetensors_metadata`, `safetensors_tensor_extract`, `onnx_metadata`, `wasm_module_validate`, and additional ML functions
    - Assign IDs 421–432, all batch-only
    - _Requirements: 7.4, 1.1, 1.4_

  - [ ] 17.5 Create `crates/deriva-compute/src/builtins_format_network.rs` (8 functions, IDs 433–440)
    - Add `pcap-parser` dependency gated behind `format-network` feature
    - Implement: `pcap_parse`, `pcap_filter`, `email_mime_parse`, `har_extract`, and additional network functions
    - Assign IDs 433–440, batch-only for PCAP, streaming for packet filtering
    - _Requirements: 7.5, 1.1, 1.4_

  - [ ] 17.6 Create `crates/deriva-compute/src/builtins_format_bio.rs` (12 functions, IDs 441–452)
    - Add `noodles` dependency gated behind `format-bio` feature
    - Implement: `fasta_read`, `fasta_filter`, `fastq_read`, `fastq_filter`, `bam_region_extract`, `vcf_variant_filter`, and additional bioinformatics functions
    - Assign IDs 441–452, FASTA/FASTQ support streaming mode
    - _Requirements: 7.6, 3.2, 1.1, 1.4_

  - [ ] 17.7 Create `crates/deriva-compute/src/builtins_format_binary.rs` (13 functions, IDs 453–465)
    - Add `wasmparser` dependency gated behind `format-binary` feature
    - Implement: `woff_metadata`, `gltf_validate`, `wasm_parse`, and additional specialized binary functions
    - Assign IDs 453–465, all batch-only
    - _Requirements: 7.4, 1.1, 1.4_

  - [ ] 17.8 Implement registration functions for all Phase 4 categories
    - Create `register_geo_functions()`, `register_scientific_functions()`, `register_database_functions()`, `register_ml_functions()`, `register_network_functions()`, `register_bio_functions()`, `register_binary_functions()` — each gated behind respective feature
    - Wire all into the `register_all()` dispatcher
    - _Requirements: 2.6, 2.7, 1.2_

  - [ ]* 17.9 Write unit tests for Phase 4 functions
    - Test at least one happy-path and one error-path per category
    - Test FASTA/FASTQ read/filter, SQLite query, SafeTensors metadata
    - Test error on malformed domain-specific files (no panics)
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 9.1, 11.1, 11.2_

- [ ] 18. Checkpoint — Phase 4 complete
  - Ensure all tests pass, ask the user if questions arise.
  - Verify 82 additional functions registered when `format-phase4` is enabled
  - Verify total of at least 283 functions across all phases

- [ ] 19. Cross-cutting correctness and integration
  - [ ] 19.1 Implement ID uniqueness validation in registry
    - Add a debug assertion or test that verifies no duplicate IDs exist in range 201–483
    - Verify no overlap with core library IDs (1–100)
    - _Requirements: 1.4, 10.1, 10.2_

  - [ ]* 19.2 Write property test for ID uniqueness
    - **Property 8: ID uniqueness**
    - Enumerate all registered format-aware functions, verify all IDs are distinct within 201–483 and do not overlap with 1–100
    - **Validates: Requirements 1.4**

  - [ ]* 19.3 Write property test for mode selection compatibility
    - **Property 7: Mode selection compatibility**
    - For functions registered in both batch and streaming, verify batch and streaming produce semantically equivalent output
    - **Validates: Requirements 3.4**

  - [ ]* 19.4 Write property test for error on invalid format
    - **Property 10: Error on invalid format**
    - Generate random non-conforming bytes, verify functions return `ComputeError::ExecutionFailed` with descriptive message (no panics)
    - **Validates: Requirements 9.1**

  - [ ] 19.5 Write integration tests for cross-phase pipelines
    - Test Phase 1 pipeline: CSV parse → filter → JSON convert → cache
    - Test Phase 2 pipeline: Parquet read → column project → Arrow → Avro write
    - Test erasure pipeline: data → encode (3+2) → drop 2 shards → decode → verify
    - Test cross-format: JSON → YAML → TOML → JSON round-trip
    - Test feature isolation: disabled features produce FunctionNotFound
    - Test registry completeness: count registered functions per enabled phase matches expected (98, 46, 57, 82)
    - _Requirements: 1.2, 1.5, 2.6, 10.1, 10.4_

  - [ ]* 19.6 Write property test for trait compliance
    - **Property 1: Trait compliance**
    - For a sample of format-aware functions, verify `execute()` with valid inputs produces `Ok(Bytes)` following core library contracts
    - **Validates: Requirements 1.1, 1.2, 1.3**

  - [ ]* 19.7 Write property test for feature isolation
    - **Property 2: Feature isolation**
    - Verify that only functions whose feature is enabled appear in the registry
    - **Validates: Requirements 2.6, 2.7**

- [ ] 20. Final checkpoint — All phases complete
  - Ensure all tests pass, ask the user if questions arise.
  - Verify `format-all` enables all 283+ functions
  - Verify default features enable only Phase 1 (98 functions)
  - Verify all parameter conventions are consistent across categories

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation after each phase
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The existing `builtins_format_detect.rs` already has 9 functions; task 2.1 extends/completes it
- All functions follow the existing `ComputeFunction` trait pattern seen in `builtins/mod.rs`
- Binary test fixtures should be kept small and stored in `tests/fixtures/`
- Embedded text fixtures (CSV, JSON, TOML, YAML) should be < 10KB inline

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["1.2", "1.3"] },
    { "id": 2, "tasks": ["1.4", "2.1", "3.1", "4.1", "5.1", "6.1", "7.1"] },
    { "id": 3, "tasks": ["2.2", "2.3", "3.2", "3.3", "4.2", "5.2", "6.2", "7.2"] },
    { "id": 4, "tasks": ["3.4", "3.5", "4.3", "5.3", "5.4", "6.3", "7.3"] },
    { "id": 5, "tasks": ["9.1"] },
    { "id": 6, "tasks": ["9.2", "10.1", "11.1"] },
    { "id": 7, "tasks": ["9.3", "9.4", "9.5", "9.6", "10.2", "10.3", "10.4", "11.2", "11.3", "11.4"] },
    { "id": 8, "tasks": ["13.1", "14.1", "15.1"] },
    { "id": 9, "tasks": ["13.2", "13.3", "14.2", "14.3", "15.2", "15.3"] },
    { "id": 10, "tasks": ["17.1", "17.2", "17.3", "17.4", "17.5", "17.6", "17.7"] },
    { "id": 11, "tasks": ["17.8", "17.9"] },
    { "id": 12, "tasks": ["19.1", "19.5"] },
    { "id": 13, "tasks": ["19.2", "19.3", "19.4", "19.6", "19.7"] }
  ]
}
```
