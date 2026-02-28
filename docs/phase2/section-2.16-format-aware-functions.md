# §2.16 Format-Aware Function Library

> **Status**: Blueprint (4-phase rollout)
> **Depends on**: §2.7 Streaming Materialization, §2.14 Streaming Function Library, §2.15 Batch Function Library
> **Crate(s)**: `deriva-compute`, `deriva-core`
> **Estimated effort**: 20–30 days across 4 phases
> **Functions**: 283 (IDs 201–483) across 19 categories

---

## 1. Problem Statement

### 1.1 Current State

The core function libraries (§2.14 streaming, §2.15 batch) provide
100 batch and 100 streaming functions operating on raw bytes. These
cover generic transforms (encoding, compression, crypto), analytics
(counts, histograms), combiners, slicing, text processing, and
validation. However, they have no awareness of file formats — a
Parquet file is just bytes, a CSV is just bytes, a PNG is just bytes.

This means pipelines cannot:
- Project specific columns from a Parquet file
- Filter CSV rows by a predicate
- Extract text from a PDF
- Validate an Avro schema
- Detect the format of an unknown blob
- Apply erasure coding for fault tolerance

### 1.2 Real-World Need

Distributed file systems store structured data in domain-specific
formats. Real-world CAS workloads require format-native operations:

| Domain | Formats | Operations |
|--------|---------|------------|
| Data engineering | Parquet, ORC, Avro, CSV, NDJSON | Column projection, predicate pushdown, schema evolution |
| DevOps | YAML, TOML, INI, HCL, logs | Config merging, log parsing, timestamp normalization |
| Web/API | JSON, XML, HTML, Protobuf | Path extraction, XSLT transform, schema validation |
| Media | PNG, JPEG, PDF, MP3, MP4 | Resize, thumbnail, text extraction, metadata strip |
| Science | HDF5, NetCDF, FITS, NumPy | Dataset slicing, array conversion, metadata extraction |
| Bioinformatics | FASTA, FASTQ, BAM, VCF | Quality trimming, variant filtering, format conversion |
| Blockchain/CAS | CID, DAG-CBOR, CAR, UnixFS | Content addressing, Merkle proofs, archive creation |
| Storage infra | Erasure coding, replication | Reed-Solomon encode/decode, parity, striping |

Without format-aware functions, users must extract data outside the
CAS, transform it, and re-ingest — losing provenance, deduplication,
and computation-addressing benefits.

### 1.3 Inspiration from Existing DFS Implementations

| System | Format Support | What We Learn |
|--------|---------------|---------------|
| HDFS | SequenceFile, Avro, Parquet, ORC, codec registry | Format-specific InputFormat/OutputFormat abstraction; codec pluggability |
| S3 | Multipart, SSE-C, content-type routing, S3 Select | Server-side filtering (predicate pushdown); content-type metadata |
| Ceph | Erasure coding, object classes, RADOS plugins | Erasure coding as first-class storage primitive; compute-at-storage |
| IPFS | UnixFS, DAG-PB, DAG-CBOR, CID, CAR archives | Content-addressing native to format; Merkle DAG as universal structure |
| Spark/Flink | Format readers, schema evolution, predicate pushdown | Columnar projection eliminates I/O; schema evolution for backward compat |

Key insight: every mature DFS eventually builds format-aware
operations. We build them as composable `ComputeFunction` /
`StreamingComputeFunction` implementations from the start.

### 1.4 Design Principles

1. **Same trait, same registry** — every format-aware function
   implements `ComputeFunction` and/or `StreamingComputeFunction`
   from §2.14/§2.15. No new traits or abstractions.
2. **Batch for random-access, streaming for sequential** — Parquet
   (footer-based) is batch; NDJSON (line-oriented) is streaming;
   many formats support both modes.
3. **All params via `BTreeMap<String, Value>`** — column names,
   filter expressions, quality levels, schema paths — all passed as
   string params, consistent with core library conventions.
4. **Feature flags for optional categories** — heavy dependencies
   (Parquet, image processing, WASM) are behind Cargo feature flags.
   Default build includes only Phase 1 (lightweight formats).
5. **IDs 201–483** — non-overlapping with core library (1–100),
   leaving 101–200 reserved for future core extensions.

---

## 2. Design

### 2.1 Mode Distribution

| Mode | Count | % | Rationale |
|------|-------|---|-----------|
| Batch only | 148 | 52% | Random-access formats (Parquet footer, ZIP central directory, PDF page tree, SQLite B-tree) |
| Both (batch + streaming) | 107 | 38% | Sequential formats with optional random access (CSV, NDJSON, Avro blocks, tar, PCAP, FASTA) |
| Streaming only | 28 | 10% | Pure stream codecs (gzip, bzip2, xz, zstd-frame) and line-oriented filters |

Batch dominance reflects the reality that most structured formats
require random access for correct parsing. Streaming-capable functions
use the accumulator or per-record pattern from §2.14.

### 2.2 Implementation Roadmap

| Phase | Categories | Functions | Effort | Dependencies |
|-------|-----------|-----------|--------|-------------|
| 1 | B (CSV/JSON), D (archive), H (config), K (logs), L (CAS/IPFS), S (detection) | 98 | 5–7 days | Lightweight: `csv`, `quick-xml`, `tar`, `zip`, `flate2`, `serde_yaml`, `cid` |
| 2 | A (columnar), C (serialization), R (erasure coding) | 46 | 7–10 days | Heavy: `parquet`, `arrow`, `apache-avro`, `prost`, `reed-solomon-erasure` |
| 3 | E (image), F (audio/video), G (documents) | 57 | 5–8 days | Media: `image`, `lopdf`, `calamine`, `symphonia` |
| 4 | I (geo), J (scientific), M (database), N (ML), O (network), P (bio), Q (binary) | 82 | 5–8 days | Domain: `geojson`, `hdf5`, `rusqlite`, `safetensors`, `noodles`, `wasmparser` |
| | **Total** | **283** | **22–33 days** | |

Phase 1 delivers immediate value with minimal dependency weight.
Each subsequent phase adds heavier dependencies behind feature flags.

### 2.3 Dependency Strategy

Unlike §2.14/§2.15 where all dependencies are unconditional, §2.16
uses Cargo feature flags to keep the default build lean:

```toml
[features]
default = ["format-phase1"]
format-phase1 = ["format-csv", "format-archive", "format-config", "format-log", "format-cas", "format-detect"]
format-phase2 = ["format-columnar", "format-serialization", "format-erasure"]
format-phase3 = ["format-image", "format-media", "format-document"]
format-phase4 = ["format-geo", "format-scientific", "format-database", "format-ml", "format-network", "format-bio", "format-binary"]
format-all = ["format-phase1", "format-phase2", "format-phase3", "format-phase4"]

# Per-category flags
format-csv = ["dep:csv", "dep:quick-xml", "dep:jsonpath-rust"]
format-archive = ["dep:tar", "dep:zip", "dep:bzip2", "dep:xz2"]
format-columnar = ["dep:parquet", "dep:arrow", "dep:orc-rust"]
format-serialization = ["dep:apache-avro", "dep:prost", "dep:rmp-serde", "dep:ciborium", "dep:bson"]
format-erasure = ["dep:reed-solomon-erasure"]
format-image = ["dep:image", "dep:resvg", "dep:img-hash"]
format-media = ["dep:symphonia"]
format-document = ["dep:lopdf", "dep:calamine", "dep:scraper", "dep:pulldown-cmark"]
format-config = ["dep:configparser", "dep:hcl-rs"]
format-log = []  # uses regex + serde_json already in workspace
format-cas = ["dep:cid", "dep:multihash", "dep:iroh-car"]
format-detect = ["dep:infer", "dep:mime_guess"]
format-geo = ["dep:geojson", "dep:flatgeobuf"]
format-scientific = ["dep:hdf5"]
format-database = ["dep:rusqlite"]
format-ml = ["dep:safetensors", "dep:wasmparser"]
format-network = ["dep:pcap-parser", "dep:mailparse"]
format-bio = ["dep:noodles"]
format-binary = ["dep:gltf", "dep:dicom-object"]
```

### 2.4 Function Numbering

| Range | Assignment |
|-------|-----------|
| 1–100 | Core batch library (§2.15) |
| 101–200 | Reserved for future core extensions |
| 201–218 | Category A: Columnar (Parquet, ORC, Arrow) |
| 219–243 | Category B: Row-oriented (CSV, JSON, XML) |
| 244–262 | Category C: Serialization (Avro, Protobuf, CBOR) |
| 263–282 | Category D: Archive (tar, zip, gzip) |
| 283–300 | Category E: Image (PNG, JPEG, SVG) |
| 301–316 | Category F: Audio/Video (MP3, WAV, MP4) |
| 317–339 | Category G: Document (PDF, DOCX, HTML) |
| 340–355 | Category H: Config (YAML, TOML, INI, HCL) |
| 356–369 | Category I: Geospatial (GeoJSON, Shapefile) |
| 370–382 | Category J: Scientific (HDF5, NetCDF, NumPy) |
| 383–395 | Category K: Log/Observability (syslog, CEF) |
| 396–410 | Category L: Blockchain/CAS (CID, CAR, UnixFS) |
| 411–420 | Category M: Database (SQLite, SST, WAL) |
| 421–432 | Category N: ML/Tensor (TFRecord, SafeTensors) |
| 433–440 | Category O: Network (PCAP, DNS, email) |
| 441–452 | Category P: Bioinformatics (FASTA, BAM, VCF) |
| 453–465 | Category Q: Specialized binary (WOFF, glTF, WASM) |
| 466–474 | Category R: Erasure coding |
| 475–483 | Category S: Universal detection/conversion |

FunctionId strings follow the pattern `category_operation@1.0.0`,
e.g., `parquet_read@1.0.0`, `csv_filter@1.0.0`, `cid_compute@1.0.0`.

### 2.5 File Organization

Each category gets its own source file to keep compilation units
manageable:

```
crates/deriva-compute/src/
├── builtins.rs                    # Core batch (#1–#100)
├── builtins_streaming.rs          # Core streaming (#1–#100)
├── builtins_format_columnar.rs    # Category A
├── builtins_format_csv.rs         # Category B
├── builtins_format_serial.rs      # Category C
├── builtins_format_archive.rs     # Category D
├── builtins_format_image.rs       # Category E
├── builtins_format_media.rs       # Category F
├── builtins_format_document.rs    # Category G
├── builtins_format_config.rs      # Category H
├── builtins_format_geo.rs         # Category I
├── builtins_format_scientific.rs  # Category J
├── builtins_format_log.rs         # Category K
├── builtins_format_cas.rs         # Category L
├── builtins_format_database.rs    # Category M
├── builtins_format_ml.rs          # Category N
├── builtins_format_network.rs     # Category O
├── builtins_format_bio.rs         # Category P
├── builtins_format_binary.rs      # Category Q
├── builtins_format_erasure.rs     # Category R
└── builtins_format_detect.rs      # Category S
```

Each file exports a `register_category_X(registry: &mut FunctionRegistry)`
function, conditionally compiled behind its feature flag:

```rust
#[cfg(feature = "format-columnar")]
pub fn register_columnar_functions(registry: &mut FunctionRegistry) {
    registry.register(Arc::new(ParquetReadFn));
    registry.register(Arc::new(ParquetWriteFn));
    // ...
}
```

---

## 3. Category Specifications

### 3.1 Category A: Columnar Data Formats — Parquet, ORC, Arrow (18 functions, #201–#218)

> **Phase**: 2 — Analytics backbone
> **Key dependencies**: `parquet`, `arrow`, `orc-rust`
> **Mode**: Predominantly batch (random-access footer metadata)

#### 3.1.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 201 | ParquetReadFn | `parquet_read@1.0.0` | Batch | — | Arrow IPC bytes |
| 202 | ParquetWriteFn | `parquet_write@1.0.0` | Batch | `compression` | Parquet bytes |
| 203 | ParquetMetadataFn | `parquet_metadata@1.0.0` | Batch | — | JSON metadata |
| 204 | ParquetProjectionFn | `parquet_projection@1.0.0` | Batch | `columns` (comma-separated) | Parquet bytes (subset) |
| 205 | ParquetFilterFn | `parquet_filter@1.0.0` | Batch | `column`, `op`, `value` | Parquet bytes (filtered) |
| 206 | ParquetMergeFn | `parquet_merge@1.0.0` | Batch | — | Parquet bytes (merged) |
| 207 | ParquetToArrowFn | `parquet_to_arrow@1.0.0` | Batch | — | Arrow IPC bytes |
| 208 | ArrowToParquetFn | `arrow_to_parquet@1.0.0` | Batch | `compression` | Parquet bytes |
| 209 | OrcReadFn | `orc_read@1.0.0` | Batch | — | Arrow IPC bytes |
| 210 | OrcWriteFn | `orc_write@1.0.0` | Batch | `compression` | ORC bytes |
| 211 | OrcMetadataFn | `orc_metadata@1.0.0` | Batch | — | JSON metadata |
| 212 | OrcFilterFn | `orc_filter@1.0.0` | Batch | `column`, `op`, `value` | ORC bytes (filtered) |
| 213 | ArrowIpcReadFn | `arrow_ipc_read@1.0.0` | Both | — | Arrow IPC bytes (normalized) |
| 214 | ArrowIpcWriteFn | `arrow_ipc_write@1.0.0` | Both | — | Arrow IPC bytes |
| 215 | ArrowSchemaExtractFn | `arrow_schema_extract@1.0.0` | Batch | — | JSON schema |
| 216 | ParquetPartitionWriteFn | `parquet_partition_write@1.0.0` | Batch | `partition_column` | JSON manifest + Parquet shards |
| 217 | ParquetStatisticsFn | `parquet_statistics@1.0.0` | Batch | — | JSON column statistics |
| 218 | ColumnarToRowFn | `columnar_to_row@1.0.0` | Batch | — | NDJSON (one JSON object per row) |

#### 3.1.2 Design Notes

**Parquet** files have a footer containing schema, row group offsets,
and column chunk metadata. All operations require reading the footer
first (random access) → batch-only. The `parquet` crate provides
`SerializedFileReader` for reading and `ArrowWriter` for writing.

**Predicate pushdown** (`ParquetFilterFn`): reads row group statistics
from the footer. If a row group's min/max for the filter column
doesn't overlap the predicate, the entire row group is skipped. This
can eliminate 90%+ of I/O for selective queries.

**Projection** (`ParquetProjectionFn`): reads only the requested
column chunks, skipping all others. For wide tables (100+ columns),
this reduces I/O proportionally.

**Arrow IPC** is the in-memory interchange format. `ArrowIpcReadFn`
and `ArrowIpcWriteFn` support streaming mode because Arrow IPC uses
a record-batch-per-message framing — each chunk is one record batch.

**ORC** is similar to Parquet (footer + stripes) but uses a different
encoding. We normalize to Arrow IPC as the internal representation,
enabling cross-format operations.

Filter operators (`op` param): `eq`, `ne`, `lt`, `le`, `gt`, `ge`,
`in`, `not_in`, `is_null`, `is_not_null`.

Compression param values: `none`, `snappy`, `gzip`, `lz4`, `zstd`.

**ParquetPartitionWriteFn** (#216) splits input into multiple Parquet
files by partition column value (Hive-style partitioning). Output is
a JSON manifest mapping partition values to CAddrs of the individual
Parquet shards. This is a multi-output function — the manifest
references CAS-stored shards.

```rust
// ParquetReadFn — reads Parquet, emits Arrow IPC
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let reader = SerializedFileReader::new(SliceableCursor::new(inputs[0].clone()))
        .map_err(|e| ComputeError::ExecutionFailed(format!("parquet read: {}", e)))?;
    let arrow_reader = ParquetFileArrowReader::new(Arc::new(reader));
    let batch_reader = arrow_reader.get_record_reader(8192)
        .map_err(|e| ComputeError::ExecutionFailed(format!("arrow reader: {}", e)))?;
    let mut buf = Vec::new();
    let mut writer = StreamWriter::try_new(&mut buf, &batch_reader.schema())
        .map_err(|e| ComputeError::ExecutionFailed(format!("ipc writer: {}", e)))?;
    for batch in batch_reader {
        let batch = batch.map_err(|e| ComputeError::ExecutionFailed(format!("read batch: {}", e)))?;
        writer.write(&batch).map_err(|e| ComputeError::ExecutionFailed(format!("write ipc: {}", e)))?;
    }
    writer.finish().map_err(|e| ComputeError::ExecutionFailed(format!("finish ipc: {}", e)))?;
    Ok(Bytes::from(buf))
}
```

```rust
// ParquetProjectionFn — read only selected columns
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let columns_str = get_string_param(params, "columns")?;
    let columns: Vec<&str> = columns_str.split(',').map(|s| s.trim()).collect();
    let reader = SerializedFileReader::new(SliceableCursor::new(inputs[0].clone()))
        .map_err(|e| ComputeError::ExecutionFailed(format!("parquet: {}", e)))?;
    let metadata = reader.metadata();
    let schema = metadata.file_metadata().schema();
    // Build projection mask from column names
    let indices: Vec<usize> = columns.iter()
        .filter_map(|name| schema.get_fields().iter().position(|f| f.name() == *name))
        .collect();
    if indices.is_empty() {
        return Err(ComputeError::InvalidParam("no matching columns found".into()));
    }
    // Read only projected columns, write new Parquet
    // ... (uses ProjectionMask and ArrowWriter)
    Ok(Bytes::from(output_buf))
}
```

#### 3.1.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| Empty Parquet file (0 rows) | Valid output with schema, 0 rows |
| Projection with non-existent column | `InvalidParam` error |
| Filter on non-existent column | `InvalidParam` error |
| Merge files with different schemas | `ExecutionFailed` — schemas must match |
| Nested/repeated columns | Supported via Arrow nested types |
| Parquet v1 vs v2 pages | Both supported by `parquet` crate |
| ORC with no stripes | Valid output with schema, 0 rows |
| Corrupt footer | `ExecutionFailed("parquet read: ...")` |

#### 3.1.4 Test Strategy

- Roundtrip: `ParquetWrite → ParquetRead` preserves all data types
- Projection: verify only requested columns present in output
- Filter: verify row count reduction and correctness
- Merge: two files with same schema → combined row count
- Statistics: verify min/max/null_count match actual data
- Arrow IPC streaming: batch and streaming mode produce identical output
- Cross-format: `ParquetToArrow → ArrowToParquet` roundtrip
- ORC: `OrcWrite → OrcRead` roundtrip

### 3.2 Category B: Row-Oriented & Delimited Formats — CSV, TSV, JSON, NDJSON, XML (25 functions, #219–#243)

> **Phase**: 1 — High-value, low-complexity
> **Key dependencies**: `csv`, `serde_json`, `quick-xml`, `jsonpath-rust`
> **Mode**: Many streaming-capable (line-oriented)

#### 3.2.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 219 | CsvParseFn | `csv_parse@1.0.0` | Both | `delimiter`, `has_header` | JSON array of objects |
| 220 | CsvWriteFn | `csv_write@1.0.0` | Both | `delimiter` | CSV bytes |
| 221 | CsvSchemaInferFn | `csv_schema_infer@1.0.0` | Batch | `sample_rows` | JSON schema |
| 222 | CsvColumnSelectFn | `csv_column_select@1.0.0` | Both | `columns` (comma-separated) | CSV (subset columns) |
| 223 | CsvColumnRenameFn | `csv_column_rename@1.0.0` | Both | `mapping` (JSON: `{"old":"new"}`) | CSV (renamed headers) |
| 224 | CsvFilterFn | `csv_filter@1.0.0` | Both | `column`, `op`, `value` | CSV (filtered rows) |
| 225 | CsvSortFn | `csv_sort@1.0.0` | Batch | `column`, `order` (`asc`/`desc`) | CSV (sorted) |
| 226 | CsvAggregateFn | `csv_aggregate@1.0.0` | Batch | `column`, `function` (`sum`/`avg`/`count`/`min`/`max`) | JSON result |
| 227 | CsvJoinFn | `csv_join@1.0.0` | Batch | `join_column`, `join_type` (`inner`/`left`/`right`/`full`) | CSV (joined) |
| 228 | CsvDeduplicateFn | `csv_deduplicate@1.0.0` | Batch | `columns` (key columns, optional) | CSV (unique rows) |
| 229 | TsvParseFn | `tsv_parse@1.0.0` | Both | `has_header` | JSON array of objects |
| 230 | NdjsonParseFn | `ndjson_parse@1.0.0` | Both | — | JSON array |
| 231 | NdjsonWriteFn | `ndjson_write@1.0.0` | Both | — | NDJSON bytes |
| 232 | NdjsonFilterFn | `ndjson_filter@1.0.0` | Both | `path`, `op`, `value` | NDJSON (filtered lines) |
| 233 | NdjsonProjectFn | `ndjson_project@1.0.0` | Both | `paths` (comma-separated JSONPaths) | NDJSON (projected fields) |
| 234 | JsonPathExtractFn | `jsonpath_extract@1.0.0` | Batch | `path` (JSONPath expression) | JSON (extracted values) |
| 235 | JsonMergeFn | `json_merge@1.0.0` | Batch | `strategy` (`shallow`/`deep`) | JSON (merged) |
| 236 | JsonFlattenFn | `json_flatten@1.0.0` | Batch | `separator` (default `"."`) | JSON (flat key-value) |
| 237 | JsonUnflattenFn | `json_unflatten@1.0.0` | Batch | `separator` (default `"."`) | JSON (nested) |
| 238 | XmlParseFn | `xml_parse@1.0.0` | Batch | — | JSON representation |
| 239 | XmlWriteFn | `xml_write@1.0.0` | Batch | `root_element` | XML bytes |
| 240 | XmlXPathExtractFn | `xml_xpath_extract@1.0.0` | Batch | `xpath` | XML fragment |
| 241 | XmlXsltTransformFn | `xml_xslt_transform@1.0.0` | Batch | — | Transformed XML/HTML |
| 242 | XmlValidateDtdFn | `xml_validate_dtd@1.0.0` | Batch | — | Pass-through or error |
| 243 | XmlValidateXsdFn | `xml_validate_xsd@1.0.0` | Batch | — | Pass-through or error |

#### 3.2.2 Design Notes

**CSV/TSV/NDJSON streaming**: these formats are line-oriented, making
them natural streaming candidates. The streaming variants maintain
header state across chunks:

- `CsvParseFn` (streaming): first chunk extracts headers, subsequent
  chunks parse rows using cached headers
- `CsvColumnSelectFn` (streaming): first chunk determines column
  indices, subsequent chunks select by index
- `CsvFilterFn` (streaming): evaluates predicate per row, emits
  matching rows immediately
- `NdjsonFilterFn` (streaming): parses each line independently,
  emits matching lines

**JSON structural operations** (`JsonMerge`, `JsonFlatten`,
`JsonUnflatten`, `JsonPathExtract`) require full parse → batch-only.

**XML** requires full DOM parse for XPath/XSLT → batch-only. DTD and
XSD validation need the complete document.

**CsvJoinFn** (#227) takes 2 inputs — left and right CSV. Join types:
- `inner`: rows matching on `join_column` in both inputs
- `left`: all left rows, matching right rows (nulls for non-matches)
- `right`: all right rows, matching left rows
- `full`: all rows from both sides

**CsvSchemaInferFn** (#221) samples up to `sample_rows` rows (default
100) and infers column types: `integer`, `float`, `boolean`, `string`,
`date`, `datetime`. Output is a JSON schema object.

Filter operators (`op` param): `eq`, `ne`, `lt`, `le`, `gt`, `ge`,
`contains`, `starts_with`, `ends_with`, `regex`.

```rust
// CsvFilterFn — filter rows by predicate
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let column = get_string_param(params, "column")?;
    let op = get_string_param(params, "op")?;
    let value = get_string_param(params, "value")?;
    let mut reader = csv::Reader::from_reader(&inputs[0][..]);
    let headers = reader.headers()
        .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?
        .clone();
    let col_idx = headers.iter().position(|h| h == column)
        .ok_or_else(|| ComputeError::InvalidParam(format!("column '{}' not found", column)))?;
    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record(&headers)
        .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
    for record in reader.records() {
        let record = record.map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
        let field = record.get(col_idx).unwrap_or("");
        if matches_predicate(field, op, value)? {
            wtr.write_record(&record)
                .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
        }
    }
    Ok(Bytes::from(wtr.into_inner().map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?))
}
```

```rust
// JsonFlattenFn — {"a":{"b":1}} → {"a.b":1}
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let sep = match params.get("separator") {
        Some(Value::String(s)) => s.as_str(),
        None => ".",
        _ => return Err(ComputeError::InvalidParam("separator must be string".into())),
    };
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("requires UTF-8".into()))?;
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
    let mut flat = serde_json::Map::new();
    flatten_recursive(&value, "", sep, &mut flat);
    Ok(Bytes::from(serde_json::to_string_pretty(&flat)
        .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?))
}
```

#### 3.2.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| CSV with no rows (header only) | Filter/select → header only. Aggregate → zero/null. |
| CSV with inconsistent column counts | Extra fields ignored, missing fields empty |
| Empty NDJSON lines | Skipped silently |
| JSONPath matching nothing | Empty array `"[]"` |
| JSON merge with conflicting keys | `shallow`: last wins. `deep`: recurse into objects, last wins for scalars |
| XML with namespaces | Preserved in parse, accessible via prefixed XPath |
| XSD validation input (2 inputs) | Input 0 = XML document, Input 1 = XSD schema |
| XSLT transform (2 inputs) | Input 0 = XML document, Input 1 = XSLT stylesheet |
| TSV with embedded tabs in quoted fields | Handled by `csv` crate with `delimiter(b'\t')` |
| Flatten already-flat JSON | Unchanged (idempotent) |
| Unflatten with ambiguous paths | `"a.b"` key and `"a"` key → `ExecutionFailed` |

#### 3.2.4 Test Strategy

- CSV roundtrip: `CsvWrite → CsvParse` preserves data
- CSV filter: verify row count and predicate correctness for each operator
- CSV join: test all 4 join types with overlapping and non-overlapping keys
- CSV schema inference: verify type detection for int, float, bool, date, string
- NDJSON streaming: batch and streaming produce identical output
- JSONPath: test nested extraction, array indexing, wildcard
- JSON flatten/unflatten roundtrip: `unflatten(flatten(x)) == x` for non-ambiguous inputs
- XML parse/write roundtrip: preserves elements, attributes, text content
- XML XPath: test element selection, attribute selection, predicate filters

### 3.3 Category C: Serialization Formats — Avro, Protobuf, Thrift, MessagePack, CBOR, BSON (19 functions, #244–#262)

> **Phase**: 2 — Analytics backbone
> **Key dependencies**: `apache-avro`, `prost`, `rmp-serde`, `ciborium`, `bson`
> **Mode**: Mixed (Avro streaming-capable via blocks; Protobuf via length-delimited messages)

#### 3.3.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 244 | AvroReadFn | `avro_read@1.0.0` | Both | — | JSON array of records |
| 245 | AvroWriteFn | `avro_write@1.0.0` | Both | `codec` (`null`/`deflate`/`snappy`) | Avro OCF bytes |
| 246 | AvroSchemaExtractFn | `avro_schema_extract@1.0.0` | Batch | — | JSON (Avro schema) |
| 247 | AvroSchemaEvolveFn | `avro_schema_evolve@1.0.0` | Batch | — | Avro OCF (re-encoded with new schema) |
| 248 | AvroToJsonFn | `avro_to_json@1.0.0` | Both | — | NDJSON (one record per line) |
| 249 | JsonToAvroFn | `json_to_avro@1.0.0` | Both | — | Avro OCF bytes |
| 250 | ProtobufDecodeFn | `protobuf_decode@1.0.0` | Both | `descriptor` (base64-encoded FileDescriptorSet) | JSON |
| 251 | ProtobufEncodeFn | `protobuf_encode@1.0.0` | Both | `descriptor`, `message_type` | Protobuf bytes |
| 252 | ProtobufSchemaExtractFn | `protobuf_schema_extract@1.0.0` | Batch | `descriptor` | JSON (message definitions) |
| 253 | ThriftDecodeFn | `thrift_decode@1.0.0` | Batch | `protocol` (`binary`/`compact`) | JSON |
| 254 | ThriftEncodeFn | `thrift_encode@1.0.0` | Batch | `protocol` | Thrift bytes |
| 255 | MsgpackDecodeFn | `msgpack_decode@1.0.0` | Both | — | JSON |
| 256 | MsgpackEncodeFn | `msgpack_encode@1.0.0` | Both | — | MessagePack bytes |
| 257 | CborDecodeFn | `cbor_decode@1.0.0` | Both | — | JSON |
| 258 | CborEncodeFn | `cbor_encode@1.0.0` | Both | — | CBOR bytes |
| 259 | BsonDecodeFn | `bson_decode@1.0.0` | Both | — | JSON |
| 260 | BsonEncodeFn | `bson_encode@1.0.0` | Both | — | BSON bytes |
| 261 | AvroToParquetFn | `avro_to_parquet@1.0.0` | Batch | `compression` | Parquet bytes |
| 262 | ParquetToAvroFn | `parquet_to_avro@1.0.0` | Batch | `codec` | Avro OCF bytes |

#### 3.3.2 Design Notes

**Avro OCF** (Object Container File) has a block structure: header
(magic + schema + sync marker), then data blocks each prefixed with
count + size. This enables streaming — each chunk processes one block.
The streaming variant caches the schema from the header chunk.

**Protobuf** uses length-delimited framing for streaming: each message
is prefixed with a varint length. The streaming variant reads one
message per chunk. The `descriptor` param provides the
`FileDescriptorSet` (compiled `.proto` schema) needed for decoding
without generated code.

**CBOR** is critical for IPFS/IPLD interop — DAG-CBOR is the
canonical encoding for IPLD data structures. Streaming is natural
because CBOR items are self-delimiting.

**MessagePack** and **BSON** are self-delimiting binary formats →
streaming reads one value/document per chunk.

**Thrift** is batch-only because the binary/compact protocols require
knowing the full message structure upfront (no self-delimiting framing
without IDL).

**Schema evolution** (`AvroSchemaEvolveFn`, #247) takes 2 inputs:
input 0 = Avro OCF with old schema, input 1 = new schema JSON. It
re-encodes records using Avro's schema resolution rules (field
defaults for added fields, field dropping for removed fields).

**Cross-format** (`AvroToParquetFn`, `ParquetToAvroFn`) converts
between the two dominant analytics formats. Both are batch-only
because Parquet requires random access.

```rust
// CborDecodeFn — CBOR → JSON
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let value: ciborium::Value = ciborium::from_reader(&inputs[0][..])
        .map_err(|e| ComputeError::ExecutionFailed(format!("cbor decode: {}", e)))?;
    let json = serde_json::to_string_pretty(&cbor_to_json(&value))
        .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?;
    Ok(Bytes::from(json))
}
```

```rust
// MsgpackDecodeFn — MessagePack → JSON
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let value: serde_json::Value = rmp_serde::from_slice(&inputs[0])
        .map_err(|e| ComputeError::ExecutionFailed(format!("msgpack decode: {}", e)))?;
    let json = serde_json::to_string_pretty(&value)
        .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?;
    Ok(Bytes::from(json))
}
```

#### 3.3.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| Avro with no records | Valid OCF with schema, empty data |
| Avro schema evolution: incompatible schemas | `ExecutionFailed` (no default for required field) |
| Protobuf without descriptor | `InvalidParam("missing param: descriptor")` |
| Protobuf unknown fields | Preserved as raw bytes in decode output |
| CBOR with tags (e.g., datetime tag 0) | Decoded to JSON string with tag annotation |
| CBOR byte strings | Base64-encoded in JSON output |
| BSON ObjectId | Serialized as `{"$oid": "..."}` extended JSON |
| BSON datetime | Serialized as `{"$date": "..."}` extended JSON |
| MessagePack binary type | Base64-encoded in JSON output |
| Thrift with unknown protocol | `InvalidParam("protocol must be binary or compact")` |
| Cross-format type mismatch (Avro logical types → Parquet) | Best-effort mapping; unsupported types → bytes |

#### 3.3.4 Test Strategy

- Avro roundtrip: `AvroWrite → AvroRead` with all codecs (null, deflate, snappy)
- Avro schema evolution: add field with default, remove optional field, type promotion (int→long)
- Avro↔JSON roundtrip: `AvroToJson → JsonToAvro` preserves records
- Avro↔Parquet: `AvroToParquet → ParquetToAvro` preserves data
- Protobuf encode/decode roundtrip with known descriptor
- CBOR encode/decode roundtrip; verify DAG-CBOR canonical ordering
- MessagePack encode/decode roundtrip for all JSON types
- BSON encode/decode roundtrip; verify extended JSON for ObjectId/datetime
- Thrift binary vs compact protocol roundtrip
- Streaming parity for all Both-mode functions (Avro, Protobuf, Msgpack, CBOR, BSON)

### 3.4 Category D: Archive & Container Formats — tar, zip, gzip, bzip2, xz, zstd-frame, 7z, rar (20 functions, #263–#282)

> **Phase**: 1 — High-value, low-complexity
> **Key dependencies**: `tar`, `zip`, `flate2`, `bzip2`, `xz2`, `zstd`, `sevenz-rust`
> **Mode**: Sequential archives streaming-capable; random-access archives batch-only

#### 3.4.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 263 | TarCreateFn | `tar_create@1.0.0` | Both | — | tar archive bytes |
| 264 | TarExtractFn | `tar_extract@1.0.0` | Both | `path` (optional — single entry) | JSON manifest + entries |
| 265 | TarListFn | `tar_list@1.0.0` | Both | — | JSON array of entry metadata |
| 266 | TarAppendFn | `tar_append@1.0.0` | Batch | `filename` | tar archive (appended) |
| 267 | GzipCompressFn | `gzip_compress@1.0.0` | Streaming | `level` (1–9, default `"6"`) | gzip bytes |
| 268 | GzipDecompressFn | `gzip_decompress@1.0.0` | Streaming | — | raw bytes |
| 269 | Bzip2CompressFn | `bzip2_compress@1.0.0` | Streaming | `level` (1–9, default `"6"`) | bzip2 bytes |
| 270 | Bzip2DecompressFn | `bzip2_decompress@1.0.0` | Streaming | — | raw bytes |
| 271 | XzCompressFn | `xz_compress@1.0.0` | Streaming | `level` (0–9, default `"6"`) | xz bytes |
| 272 | XzDecompressFn | `xz_decompress@1.0.0` | Streaming | — | raw bytes |
| 273 | ZipCreateFn | `zip_create@1.0.0` | Batch | `compression` (`stored`/`deflate`/`zstd`) | zip archive bytes |
| 274 | ZipExtractFn | `zip_extract@1.0.0` | Batch | `path` (optional — single entry) | JSON manifest + entries |
| 275 | ZipListFn | `zip_list@1.0.0` | Batch | — | JSON array of entry metadata |
| 276 | ZipExtractSingleFn | `zip_extract_single@1.0.0` | Batch | `path` (required) | raw entry bytes |
| 277 | TarGzCreateFn | `tar_gz_create@1.0.0` | Both | `level` (1–9, default `"6"`) | tar.gz bytes |
| 278 | TarGzExtractFn | `tar_gz_extract@1.0.0` | Both | — | JSON manifest + entries |
| 279 | ZstdFrameCompressFn | `zstd_frame_compress@1.0.0` | Streaming | `level` (1–22, default `"3"`) | zstd frame bytes |
| 280 | ZstdFrameDecompressFn | `zstd_frame_decompress@1.0.0` | Streaming | — | raw bytes |
| 281 | SevenZExtractFn | `sevenz_extract@1.0.0` | Batch | — | JSON manifest + entries |
| 282 | RarExtractFn | `rar_extract@1.0.0` | Batch | — | JSON manifest + entries |

#### 3.4.2 Design Notes

**Tar** is a sequential format — entries are concatenated with
512-byte headers. This makes it naturally streaming: each chunk
processes one or more entries. `TarCreateFn` takes multiple inputs
(one per file) with filenames encoded in params as JSON:
`{"files": ["a.txt", "b.txt"]}`.

**Zip** requires a central directory at the end of the file for
random access to individual entries → batch-only. `ZipExtractSingleFn`
reads only the requested entry using the central directory offset.

**Whole-stream compression** (gzip, bzip2, xz, zstd-frame) differs
from the per-chunk compression in §2.14/§2.15. These maintain codec
state across the entire stream, producing a single valid compressed
file. The streaming variants use `flate2::write::GzEncoder` (etc.)
with `write()` per chunk and `finish()` on flush.

Key difference from §2.15 `CompressFn`/`DecompressFn`:
- §2.15 `compress@1.0.0`: zlib per-chunk (each chunk independently compressed)
- §2.16 `gzip_compress@1.0.0`: gzip whole-stream (single compressed output)

**tar.gz** is a composition: `TarGzCreateFn` = tar + gzip in a single
function for convenience. Streaming: tar entries are written into a
gzip encoder.

**Extract output format**: archive extraction functions return a JSON
manifest as the primary output. The manifest maps entry paths to
metadata (size, mtime, permissions). For single-entry extraction,
raw bytes are returned directly.

```rust
// GzipCompressFn (streaming variant sketch)
fn process_chunk(&mut self, chunk: Bytes) -> Result<Option<Bytes>, ComputeError> {
    self.encoder.write_all(&chunk)
        .map_err(|e| ComputeError::ExecutionFailed(format!("gzip: {}", e)))?;
    let output = self.encoder.get_mut().drain(..).collect::<Vec<_>>();
    if output.is_empty() { Ok(None) } else { Ok(Some(Bytes::from(output))) }
}

fn finish(&mut self) -> Result<Option<Bytes>, ComputeError> {
    let inner = self.encoder.finish()
        .map_err(|e| ComputeError::ExecutionFailed(format!("gzip finish: {}", e)))?;
    Ok(Some(Bytes::from(inner)))
}
```

```rust
// ZipExtractSingleFn — extract one file by path
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let path = get_string_param(params, "path")?;
    let reader = std::io::Cursor::new(&inputs[0]);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| ComputeError::ExecutionFailed(format!("zip: {}", e)))?;
    let mut file = archive.by_name(path)
        .map_err(|e| ComputeError::ExecutionFailed(format!("entry '{}': {}", path, e)))?;
    let mut buf = Vec::with_capacity(file.size() as usize);
    std::io::Read::read_to_end(&mut file, &mut buf)
        .map_err(|e| ComputeError::ExecutionFailed(format!("read: {}", e)))?;
    Ok(Bytes::from(buf))
}
```

#### 3.4.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| Empty tar (no entries) | Valid tar with only EOF blocks |
| Tar with long filenames (>100 chars) | GNU/PAX extended headers |
| Tar with symlinks | Metadata preserved in manifest; symlink target in JSON |
| Zip with password-protected entries | `ExecutionFailed("encrypted entries not supported")` |
| Zip64 (>4 GB entries) | Supported by `zip` crate |
| Gzip with multiple members | All members decompressed sequentially |
| Corrupt archive header | `ExecutionFailed` with format-specific error |
| 7z with solid compression | Supported by `sevenz-rust` |
| Rar v5 vs v4 | Only v5 supported; v4 → `ExecutionFailed` |
| Tar append to non-tar input | `ExecutionFailed("not a valid tar archive")` |
| Compression level out of range | `InvalidParam("level must be 1..9")` |

#### 3.4.4 Test Strategy

- Tar create/extract roundtrip: verify file contents and metadata
- Tar streaming: batch and streaming produce identical archives
- Tar.gz pipeline: `TarGzCreate → TarGzExtract` roundtrip
- Zip create/extract roundtrip with all compression methods
- Zip single-entry extraction: verify correct file, others untouched
- Gzip/bzip2/xz/zstd-frame: compress → decompress roundtrip
- Whole-stream vs per-chunk compression ratio comparison
- 7z and rar extraction of known test archives
- Tar append: verify original entries preserved + new entry added

### 3.5 Category E: Image Formats — PNG, JPEG, WebP, TIFF, SVG, GIF (18 functions, #283–#300)

> **Phase**: 3 — Rich media
> **Key dependencies**: `image`, `imageproc`, `resvg`, `img-hash`
> **Mode**: Predominantly batch (pixel-level random access); metadata/detection streaming-capable

#### 3.5.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 283 | ImageMetadataFn | `image_metadata@1.0.0` | Both | — | JSON (dimensions, format, color type, EXIF) |
| 284 | ImageResizeFn | `image_resize@1.0.0` | Batch | `width`, `height`, `filter` (`nearest`/`bilinear`/`lanczos3`) | Image bytes (same format) |
| 285 | ImageCropFn | `image_crop@1.0.0` | Batch | `x`, `y`, `width`, `height` | Image bytes (same format) |
| 286 | ImageRotateFn | `image_rotate@1.0.0` | Batch | `degrees` (`90`/`180`/`270`) | Image bytes (same format) |
| 287 | ImageConvertFn | `image_convert@1.0.0` | Batch | `format` (`png`/`jpeg`/`webp`/`tiff`/`gif`/`bmp`) | Image bytes (target format) |
| 288 | ImageThumbnailFn | `image_thumbnail@1.0.0` | Batch | `max_dimension` (default `"128"`) | PNG bytes |
| 289 | ImageStripMetadataFn | `image_strip_metadata@1.0.0` | Both | — | Image bytes (EXIF/XMP removed) |
| 290 | ImageToGrayscaleFn | `image_to_grayscale@1.0.0` | Batch | — | Image bytes (grayscale, same format) |
| 291 | ImageWatermarkFn | `image_watermark@1.0.0` | Batch | `position` (`center`/`bottom-right`/`tile`) | Image bytes (watermarked) |
| 292 | ImageHashFn | `image_hash@1.0.0` | Batch | `algorithm` (`phash`/`dhash`/`ahash`) | Hex string (perceptual hash) |
| 293 | PngOptimizeFn | `png_optimize@1.0.0` | Batch | — | PNG bytes (re-encoded, smaller) |
| 294 | JpegOptimizeFn | `jpeg_optimize@1.0.0` | Batch | `quality` (1–100, default `"85"`) | JPEG bytes (re-encoded) |
| 295 | SvgMinifyFn | `svg_minify@1.0.0` | Batch | — | SVG bytes (whitespace/comments removed) |
| 296 | SvgToPngFn | `svg_to_png@1.0.0` | Batch | `width`, `height` | PNG bytes (rasterized) |
| 297 | TiffSplitFn | `tiff_split@1.0.0` | Batch | — | JSON manifest (one CAddr per page) |
| 298 | TiffMergeFn | `tiff_merge@1.0.0` | Batch | — | Multi-page TIFF bytes |
| 299 | GifExtractFramesFn | `gif_extract_frames@1.0.0` | Batch | — | JSON manifest (one CAddr per frame) |
| 300 | ImageDetectFormatFn | `image_detect_format@1.0.0` | Both | — | JSON (`{"format":"png","mime":"image/png"}`) |

#### 3.5.2 Design Notes

Most image operations require decoding to a full pixel buffer →
batch-only. The `image` crate provides `DynamicImage` as the
universal in-memory representation.

**Streaming-capable functions** read only headers:
- `ImageMetadataFn`: reads dimensions and color type from format
  header without decoding pixels. EXIF from JPEG APP1 marker.
- `ImageStripMetadataFn`: for JPEG, strips APP1/APP13 markers
  without re-encoding pixels. For PNG, removes tEXt/iTXt chunks.
- `ImageDetectFormatFn`: magic byte detection (first 8 bytes).

**ImageWatermarkFn** (#291) takes 2 inputs: input 0 = base image,
input 1 = watermark image (typically PNG with transparency).

**Perceptual hashing** (`ImageHashFn`, #292) produces a compact
fingerprint for near-duplicate detection:
- `phash`: DCT-based, robust to resize/compression
- `dhash`: gradient-based, fast
- `ahash`: average-based, simplest

Output is a hex string. Hamming distance between two hashes indicates
visual similarity (0 = identical, <10 = near-duplicate).

**SVG rasterization** (`SvgToPngFn`) uses `resvg` for high-quality
rendering with CSS/font support. Width/height params control output
resolution.

**Multi-output functions** (`TiffSplitFn`, `GifExtractFramesFn`)
produce a JSON manifest referencing individual CAS-stored frames/pages.
Each frame is stored as a separate CAS value; the manifest maps
indices to CAddrs.

```rust
// ImageResizeFn
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let width: u32 = get_string_param(params, "width")?.parse()
        .map_err(|_| ComputeError::InvalidParam("width must be positive integer".into()))?;
    let height: u32 = get_string_param(params, "height")?.parse()
        .map_err(|_| ComputeError::InvalidParam("height must be positive integer".into()))?;
    let filter_name = match params.get("filter") {
        Some(Value::String(s)) => s.as_str(),
        None => "lanczos3",
        _ => return Err(ComputeError::InvalidParam("filter must be string".into())),
    };
    let filter = match filter_name {
        "nearest" => image::imageops::FilterType::Nearest,
        "bilinear" => image::imageops::FilterType::Triangle,
        "lanczos3" => image::imageops::FilterType::Lanczos3,
        _ => return Err(ComputeError::InvalidParam("filter: nearest|bilinear|lanczos3".into())),
    };
    let img = image::load_from_memory(&inputs[0])
        .map_err(|e| ComputeError::ExecutionFailed(format!("image decode: {}", e)))?;
    let resized = img.resize_exact(width, height, filter);
    let fmt = image::guess_format(&inputs[0])
        .map_err(|e| ComputeError::ExecutionFailed(format!("format detect: {}", e)))?;
    let mut buf = Vec::new();
    resized.write_to(&mut std::io::Cursor::new(&mut buf), fmt)
        .map_err(|e| ComputeError::ExecutionFailed(format!("image encode: {}", e)))?;
    Ok(Bytes::from(buf))
}
```

#### 3.5.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| Resize to 0×0 | `InvalidParam("dimensions must be > 0")` |
| Crop region outside image bounds | `ExecutionFailed("crop region out of bounds")` |
| Rotate by non-90° increment | `InvalidParam("degrees must be 90, 180, or 270")` |
| Convert animated GIF to PNG | First frame only |
| JPEG quality 0 or >100 | `InvalidParam("quality must be 1..100")` |
| SVG with external references | Ignored (no network access) |
| Corrupt image header | `ExecutionFailed("image decode: ...")` |
| TIFF with single page → split | Manifest with one entry |
| Watermark larger than base image | Scaled down to fit |
| 16-bit TIFF | Supported; converted to 8-bit for formats that require it |

#### 3.5.4 Test Strategy

- Resize: verify output dimensions match requested width×height
- Crop: verify output dimensions and pixel content at boundaries
- Rotate 90°→180°→270°→360° = identity
- Format conversion roundtrip: PNG→JPEG→PNG (lossy, verify dimensions)
- Thumbnail: verify max dimension constraint
- Strip metadata: verify EXIF absent in output, pixels unchanged
- Grayscale: verify single-channel output
- Perceptual hash: identical images → same hash; resized → similar hash
- PNG/JPEG optimize: output ≤ input size, valid image
- SVG→PNG: verify rasterized dimensions
- TIFF split/merge roundtrip: page count preserved
- GIF frame extraction: frame count matches

### 3.6 Category F: Audio & Video Formats — MP3, WAV, FLAC, MP4, MKV (16 functions, #301–#316)

> **Phase**: 3 — Rich media
> **Key dependencies**: `symphonia`, `ffmpeg-sys-next` (optional)
> **Mode**: Predominantly batch; raw PCM streaming-capable

#### 3.6.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 301 | AudioMetadataFn | `audio_metadata@1.0.0` | Both | — | JSON (duration, sample rate, channels, codec, tags) |
| 302 | AudioConvertFn | `audio_convert@1.0.0` | Batch | `format` (`wav`/`flac`/`mp3`/`ogg`) | Audio bytes (target format) |
| 303 | AudioTrimFn | `audio_trim@1.0.0` | Batch | `start_ms`, `end_ms` | Audio bytes (trimmed) |
| 304 | AudioNormalizeFn | `audio_normalize@1.0.0` | Batch | `target_db` (default `"-3"`) | Audio bytes (peak-normalized) |
| 305 | AudioWaveformFn | `audio_waveform@1.0.0` | Batch | `width`, `height` | PNG bytes (waveform visualization) |
| 306 | AudioSilenceDetectFn | `audio_silence_detect@1.0.0` | Batch | `threshold_db` (default `"-40"`), `min_duration_ms` (default `"500"`) | JSON array of `{start_ms, end_ms}` |
| 307 | AudioStripMetadataFn | `audio_strip_metadata@1.0.0` | Batch | — | Audio bytes (ID3/Vorbis tags removed) |
| 308 | VideoMetadataFn | `video_metadata@1.0.0` | Both | — | JSON (duration, resolution, fps, codecs, streams) |
| 309 | VideoThumbnailFn | `video_thumbnail@1.0.0` | Batch | `time_ms` (default `"0"`) | PNG bytes (single frame) |
| 310 | VideoExtractAudioFn | `video_extract_audio@1.0.0` | Batch | `format` (`wav`/`mp3`/`flac`) | Audio bytes |
| 311 | VideoStripAudioFn | `video_strip_audio@1.0.0` | Batch | — | Video bytes (audio track removed) |
| 312 | VideoResolutionFn | `video_resolution@1.0.0` | Batch | `width`, `height` | Video bytes (rescaled) |
| 313 | SubtitleExtractFn | `subtitle_extract@1.0.0` | Batch | `track` (default `"0"`) | SRT/VTT text |
| 314 | WavToRawPcmFn | `wav_to_raw_pcm@1.0.0` | Both | — | Raw PCM bytes + JSON header |
| 315 | RawPcmToWavFn | `raw_pcm_to_wav@1.0.0` | Both | `sample_rate`, `channels`, `bits_per_sample` | WAV bytes |
| 316 | MediaDetectFormatFn | `media_detect_format@1.0.0` | Both | — | JSON (`{"format":"mp4","mime":"video/mp4","streams":[...]}`) |

#### 3.6.2 Design Notes

Media files are among the largest objects in storage. Most operations
require full decode → batch-only. The `symphonia` crate provides
pure-Rust demuxing and decoding for MP3, FLAC, WAV, OGG, AAC, and
MP4/MKV container parsing.

**Streaming-capable functions** read only container headers:
- `AudioMetadataFn`: reads codec info, duration, tags from header
- `VideoMetadataFn`: reads moov atom (MP4) or segment info (MKV)
- `MediaDetectFormatFn`: magic byte detection + container probe
- `WavToRawPcmFn`: strips 44-byte WAV header, streams PCM data
- `RawPcmToWavFn`: prepends WAV header, streams PCM data

**WAV↔PCM** is the only true streaming audio operation. WAV has a
fixed 44-byte header (for standard PCM), so stripping/prepending it
is trivial. `WavToRawPcmFn` output is 2 parts: first chunk = JSON
header (`{"sample_rate":44100,"channels":2,"bits_per_sample":16}`),
remaining chunks = raw PCM bytes.

**Video operations** (#309–#313) require `ffmpeg-sys-next` behind an
optional feature flag `format-media-ffmpeg`. Without it, only
container-level metadata extraction is available via `symphonia`.

**AudioWaveformFn** (#305) decodes audio to PCM, downsamples to
`width` samples, computes min/max per sample, and renders a PNG
waveform using the `image` crate (cross-dependency with Category E).

**AudioNormalizeFn** (#304) performs peak normalization: finds the
maximum absolute sample value, then scales all samples so the peak
reaches `target_db` relative to 0 dBFS.

```rust
// AudioMetadataFn (streaming — header only)
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let cursor = std::io::Cursor::new(&inputs[0]);
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
    let probed = symphonia::default::get_probe()
        .format(&Default::default(), mss, &Default::default(), &Default::default())
        .map_err(|e| ComputeError::ExecutionFailed(format!("probe: {}", e)))?;
    let format = probed.format;
    let track = format.default_track()
        .ok_or_else(|| ComputeError::ExecutionFailed("no audio track".into()))?;
    let codec = track.codec_params.codec.to_string();
    let sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(0);
    let duration_ms = track.codec_params.n_frames
        .map(|n| n * 1000 / sample_rate.max(1) as u64);
    let meta = serde_json::json!({
        "codec": codec, "sample_rate": sample_rate,
        "channels": channels, "duration_ms": duration_ms,
    });
    Ok(Bytes::from(serde_json::to_string_pretty(&meta).unwrap()))
}
```

#### 3.6.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| Trim start > end | `InvalidParam("start_ms must be < end_ms")` |
| Trim beyond duration | Clamp to actual duration |
| Normalize silent audio (all zeros) | Return unchanged (avoid division by zero) |
| Silence detect on empty audio | Empty JSON array `"[]"` |
| Video without audio track → extract audio | `ExecutionFailed("no audio track")` |
| Video without subtitle track | `ExecutionFailed("no subtitle track at index N")` |
| Corrupt container header | `ExecutionFailed("probe: ...")` |
| WAV with non-PCM encoding (e.g., ADPCM) | `ExecutionFailed("only PCM WAV supported")` |
| Raw PCM with wrong params | Produces valid but incorrect WAV (garbage audio) |
| MP4 with fragmented moov (fMP4) | Supported by symphonia |

#### 3.6.4 Test Strategy

- Audio metadata: verify duration, sample rate, channels for known files
- WAV↔PCM roundtrip: `WavToRawPcm → RawPcmToWav` preserves audio data
- Audio trim: verify output duration matches `end_ms - start_ms`
- Silence detection: known file with silence gaps → correct intervals
- Normalize: verify peak sample reaches target dB
- Video metadata: verify resolution, fps, codec for known MP4
- Video thumbnail: verify output is valid PNG with correct dimensions
- Media format detection: MP3, WAV, FLAC, MP4, MKV all correctly identified
- Streaming parity for metadata and WAV/PCM functions

### 3.7 Category G: Document Formats — PDF, DOCX, XLSX, HTML, Markdown (23 functions, #317–#339)

> **Phase**: 3 — Rich media
> **Key dependencies**: `lopdf`, `calamine`, `scraper`, `pulldown-cmark`, `comrak`
> **Mode**: Predominantly batch (complex internal structure); HTML streaming-capable

#### 3.7.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 317 | PdfMetadataFn | `pdf_metadata@1.0.0` | Batch | — | JSON (title, author, pages, producer, creation date) |
| 318 | PdfExtractTextFn | `pdf_extract_text@1.0.0` | Batch | `pages` (optional, e.g. `"1-5,8"`) | UTF-8 text |
| 319 | PdfExtractImagesFn | `pdf_extract_images@1.0.0` | Batch | — | JSON manifest (CAddr per image) |
| 320 | PdfMergeFn | `pdf_merge@1.0.0` | Batch | — | PDF bytes (merged) |
| 321 | PdfSplitFn | `pdf_split@1.0.0` | Batch | `pages` (e.g. `"1-3"`, `"5"`) | PDF bytes (subset pages) |
| 322 | PdfPageCountFn | `pdf_page_count@1.0.0` | Batch | — | JSON (`{"pages": N}`) |
| 323 | PdfToTextFn | `pdf_to_text@1.0.0` | Batch | — | UTF-8 text (all pages) |
| 324 | DocxExtractTextFn | `docx_extract_text@1.0.0` | Batch | — | UTF-8 text |
| 325 | DocxMetadataFn | `docx_metadata@1.0.0` | Batch | — | JSON (title, author, word count, revision) |
| 326 | XlsxReadFn | `xlsx_read@1.0.0` | Batch | `sheet` (name or index, default `"0"`) | CSV bytes |
| 327 | XlsxSheetListFn | `xlsx_sheet_list@1.0.0` | Batch | — | JSON array of sheet names |
| 328 | XlsxToCsvFn | `xlsx_to_csv@1.0.0` | Batch | `sheet` | CSV bytes |
| 329 | PptxExtractTextFn | `pptx_extract_text@1.0.0` | Batch | — | UTF-8 text |
| 330 | PptxSlideCountFn | `pptx_slide_count@1.0.0` | Batch | — | JSON (`{"slides": N}`) |
| 331 | HtmlToTextFn | `html_to_text@1.0.0` | Both | — | UTF-8 text (tags stripped) |
| 332 | HtmlExtractLinksFn | `html_extract_links@1.0.0` | Batch | — | JSON array of URLs |
| 333 | HtmlExtractImagesFn | `html_extract_images@1.0.0` | Batch | — | JSON array of image URLs |
| 334 | HtmlMinifyFn | `html_minify@1.0.0` | Both | — | HTML bytes (whitespace collapsed) |
| 335 | MarkdownToHtmlFn | `markdown_to_html@1.0.0` | Batch | `extensions` (`tables`/`strikethrough`/`all`, default `"all"`) | HTML bytes |
| 336 | HtmlToMarkdownFn | `html_to_markdown@1.0.0` | Batch | — | Markdown bytes |
| 337 | LatexToTextFn | `latex_to_text@1.0.0` | Batch | — | UTF-8 text (commands stripped) |
| 338 | EpubExtractTextFn | `epub_extract_text@1.0.0` | Batch | — | UTF-8 text |
| 339 | EpubMetadataFn | `epub_metadata@1.0.0` | Batch | — | JSON (title, author, language, chapters) |

#### 3.7.2 Design Notes

**PDF** has a page tree with cross-reference table → random access
required. `lopdf` provides low-level PDF object manipulation.
Text extraction iterates content streams per page, decoding font
encodings. Image extraction identifies XObject streams of type Image.

**DOCX/PPTX/XLSX** are ZIP archives containing XML files. Processing
requires: unzip → parse XML → extract content. All batch-only because
ZIP requires central directory.

- DOCX: `word/document.xml` contains paragraphs and runs
- PPTX: `ppt/slides/slideN.xml` contains text frames
- XLSX: `xl/worksheets/sheetN.xml` contains cell data; `xl/sharedStrings.xml` for string table

`calamine` handles XLSX/XLS/ODS reading with a unified API.

**HTML** operations split into streaming and batch:
- `HtmlToTextFn` (streaming): tag-aware stripping can process
  chunks incrementally, maintaining a simple state machine for
  open/close tags
- `HtmlMinifyFn` (streaming): whitespace collapsing is local
- `HtmlExtractLinksFn` (batch): needs full DOM for `<a href>` collection
- `HtmlExtractImagesFn` (batch): needs full DOM for `<img src>`

**Markdown** uses `comrak` (GFM-compatible) for Markdown→HTML with
extensions (tables, strikethrough, autolinks, task lists).
`pulldown-cmark` is an alternative for simpler cases.

**PdfMergeFn** (#320) takes N inputs (one PDF per input) and produces
a single merged PDF. Page trees are concatenated.

**PdfSplitFn** (#321) extracts a page range from a single PDF. The
`pages` param supports ranges (`"1-5"`), individual pages (`"3"`),
and comma-separated combinations (`"1-3,7,10-12"`).

```rust
// HtmlToTextFn — strip tags, extract text
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let html = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("requires UTF-8".into()))?;
    let document = scraper::Html::parse_document(html);
    let text: String = document.root_element()
        .text()
        .collect::<Vec<_>>()
        .join(" ");
    Ok(Bytes::from(text.trim().to_string()))
}
```

```rust
// XlsxToCsvFn — read sheet, emit CSV
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let sheet_param = match params.get("sheet") {
        Some(Value::String(s)) => s.as_str(),
        None => "0",
        _ => return Err(ComputeError::InvalidParam("sheet must be string".into())),
    };
    let cursor = std::io::Cursor::new(&inputs[0]);
    let mut workbook = calamine::open_workbook_auto_from_rs(cursor)
        .map_err(|e| ComputeError::ExecutionFailed(format!("xlsx: {}", e)))?;
    let sheet_name = if let Ok(idx) = sheet_param.parse::<usize>() {
        workbook.sheet_names().get(idx)
            .ok_or_else(|| ComputeError::InvalidParam(format!("sheet index {} out of range", idx)))?
            .clone()
    } else {
        sheet_param.to_string()
    };
    let range = workbook.worksheet_range(&sheet_name)
        .map_err(|e| ComputeError::ExecutionFailed(format!("sheet '{}': {}", sheet_name, e)))?;
    let mut wtr = csv::Writer::from_writer(Vec::new());
    for row in range.rows() {
        let record: Vec<String> = row.iter().map(|c| c.to_string()).collect();
        wtr.write_record(&record)
            .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
    }
    Ok(Bytes::from(wtr.into_inner().map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?))
}
```

#### 3.7.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| PDF with no text (scanned image) | Empty text output (no OCR) |
| PDF with encrypted content | `ExecutionFailed("encrypted PDF not supported")` |
| PDF page range out of bounds | Clamp to actual page count |
| PDF merge with different page sizes | Preserved (no normalization) |
| DOCX with tracked changes | Extracts accepted text only |
| XLSX with formulas | Returns computed values (not formulas) |
| XLSX with merged cells | Value in top-left cell, others empty |
| XLSX sheet not found | `InvalidParam("sheet 'X' not found")` |
| HTML with `<script>`/`<style>` | Content excluded from text extraction |
| HTML with entities (`&amp;`) | Decoded to characters |
| Markdown with raw HTML blocks | Passed through to HTML output |
| EPUB with DRM | `ExecutionFailed("DRM-protected EPUB not supported")` |
| LaTeX with custom macros | Unknown macros stripped, arguments preserved |

#### 3.7.4 Test Strategy

- PDF text extraction: known PDF → expected text content
- PDF merge: 2 PDFs → combined page count
- PDF split: extract pages 2–3 from 5-page PDF → 2 pages
- DOCX text extraction: known DOCX → expected paragraphs
- XLSX→CSV: verify cell values, sheet selection by name and index
- XLSX sheet list: verify all sheet names returned
- HTML→text: verify tags stripped, entities decoded, script/style excluded
- HTML minify: verify whitespace reduced, structure preserved
- Markdown→HTML→Markdown: approximate roundtrip (formatting may differ)
- HTML link/image extraction: verify all URLs collected
- Streaming parity for HtmlToText and HtmlMinify

### 3.8 Category H: Configuration & Schema Formats — YAML, TOML, INI, HCL (16 functions, #340–#355)

> **Phase**: 1 — High-value, low-complexity
> **Key dependencies**: `serde_yaml`, `toml`, `configparser`, `hcl-rs`
> **Mode**: All batch (small files, full parse required)

#### 3.8.1 Function List

TODO — table of 16 functions: YamlParse, YamlWrite, YamlValidate, YamlMerge, TomlParse, TomlWrite, IniParse, IniWrite, EnvParse, EnvWrite, HclParse, PropertiesParse, PropertiesWrite, PlistParse, PlistWrite, ConfigFormatConvert

#### 3.8.2 Design Notes

TODO — all batch because config files are small; `ConfigFormatConvert` is universal converter routing through JSON as intermediate

#### 3.8.3 Test Strategy

TODO — roundtrip for each format, multi-document YAML, deep merge correctness, HCL Terraform config parsing

### 3.9 Category I: Geospatial Formats — GeoJSON, Shapefile, KML, GeoTIFF (14 functions, #356–#369)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `geojson`, `geo`, `gdal` (optional), `flatgeobuf`
> **Mode**: Mixed; FlatGeobuf streaming-capable

#### 3.9.1 Function List

TODO — table of 14 functions: GeoJsonValidate, GeoJsonFilter, GeoJsonBbox, GeoJsonSimplify, GeoJsonToWkt, WktToGeoJson, ShapefileToGeoJson, KmlToGeoJson, GpxToGeoJson, GeoTiffMetadata, GeoTiffCrop, GeoTiffResample, FlatGeobufRead (Both), FlatGeobufWrite (Both)

#### 3.9.2 Design Notes

TODO — GeoJSON is JSON → full parse; FlatGeobuf designed for streaming (one feature per chunk); GeoTIFF requires random access for spatial crop

#### 3.9.3 Test Strategy

TODO — bbox computation, simplification tolerance, format conversion roundtrip, FlatGeobuf streaming feature count

### 3.10 Category J: Scientific & Numerical Data — HDF5, NetCDF, FITS, NumPy, Zarr (13 functions, #370–#382)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `hdf5`, `netcdf`, `fitsio`, `ndarray`
> **Mode**: All batch (random-access array formats)

#### 3.10.1 Function List

TODO — table of 13 functions: Hdf5Metadata, Hdf5Read, Hdf5Write, NetcdfMetadata, NetcdfRead, FitsMetadata, FitsRead, NumpyRead, NumpyWrite, ZarrRead, ZarrMetadata, NumpyToArrow, ArrowToNumpy

#### 3.10.2 Design Notes

TODO — Zarr stores arrays as individually addressable chunks → natural CAS fit; HDF5/NetCDF require random access for dataset slicing

#### 3.10.3 Test Strategy

TODO — dataset read/write roundtrip, slice correctness, metadata extraction, Zarr chunk addressing, NumPy↔Arrow conversion

### 3.11 Category K: Log & Observability Formats — Syslog, CEF, Apache/Nginx, CloudTrail, OTLP (13 functions, #383–#395)

> **Phase**: 1 — High-value, low-complexity
> **Key dependencies**: `regex`, `serde_json`
> **Mode**: Predominantly streaming (line-oriented)

#### 3.11.1 Function List

TODO — table of 13 functions: SyslogParse (Both), SyslogWrite (Both), CefParse (Both), ElfParse (Both), ApacheLogParse (Both), NginxLogParse (Both), CloudTrailParse, VpcFlowLogParse (Both), PrometheusExpositionParse (Both), OtlpDecode, LogTimestampNormalize (Both), LogLevelFilter (Both), LogAnonymize (Both)

#### 3.11.2 Design Notes

TODO — log formats are line-oriented → natural streaming; CloudTrail is JSON array → batch; timestamp normalization auto-detects input format

#### 3.11.3 Test Strategy

TODO — parse accuracy for each format, timestamp normalization across formats, level filtering, PII anonymization verification

### 3.12 Category L: Blockchain & Content-Addressing — CID, DAG-PB, DAG-CBOR, CAR, UnixFS (15 functions, #396–#410)

> **Phase**: 1 — High-value (directly relevant to CAS)
> **Key dependencies**: `cid`, `multihash`, `libipld`, `iroh-car`
> **Mode**: Mixed; CAR and CID streaming-capable

#### 3.12.1 Function List

TODO — table of 15 functions: CidCompute (Both), CidVerify (Both), CidParse, DagPbEncode, DagPbDecode, DagCborEncode, DagCborDecode, CarCreate (Both), CarExtract (Both), CarList (Both), CarVerify (Both), UnixFsChunk (Both), UnixFsAssemble, MerkleProofGenerate, MerkleProofVerify

#### 3.12.2 Design Notes

TODO — directly relevant to Computation-Addressed Store; CID computation is accumulator pattern; CAR is sequential archive → streaming; UnixFS chunking is content-defined → streaming

#### 3.12.3 Test Strategy

TODO — CID computation against known vectors, CAR create/extract roundtrip, UnixFS chunking determinism, Merkle proof generation and verification

### 3.13 Category M: Database & Storage Formats — SQLite, RocksDB SST, WAL, Postgres COPY (10 functions, #411–#420)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `rusqlite`, `rocksdb`
> **Mode**: Mixed; Postgres COPY and WAL streaming-capable

#### 3.13.1 Function List

TODO — table of 10 functions: SqliteQuery, SqliteTableList, SqliteToCsv, CsvToSqlite, PostgresCopyParse (Both), PostgresCopyWrite (Both), SstMetadata, SstScan, WalParse (Both), SqlDumpParse (Both)

#### 3.13.2 Design Notes

TODO — SQLite requires random access (B-tree); SST files have sorted structure with index blocks; WAL and SQL dump are sequential → streaming

#### 3.13.3 Test Strategy

TODO — SQLite query correctness, CSV↔SQLite roundtrip, SST key range scan, WAL record parsing

### 3.14 Category N: Machine Learning & Tensor Formats — TFRecord, SafeTensors, ONNX, GGUF (12 functions, #421–#432)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `safetensors`, `ort` (ONNX Runtime), `protobuf`
> **Mode**: Mixed; TFRecord streaming-capable

#### 3.14.1 Function List

TODO — table of 12 functions: TfRecordRead (Both), TfRecordWrite (Both), SafeTensorsRead, SafeTensorsMetadata, SafeTensorsWrite, OnnxMetadata, OnnxValidate, GgufMetadata, PickleToJson, NumpyToSafeTensors, TfRecordToParquet, ImageToTensor

#### 3.14.2 Design Notes

TODO — TFRecord has length-delimited records with CRC → streaming one Example per chunk; SafeTensors has header-based offset lookup → random access; GGUF metadata extraction for LLM model inspection

#### 3.14.3 Test Strategy

TODO — TFRecord CRC verification, SafeTensors tensor read by name, ONNX shape inference validation, GGUF architecture detection

### 3.15 Category O: Network & Protocol Formats — PCAP, DNS, HAR, Email (8 functions, #433–#440)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `pcap-parser`, `mailparse`
> **Mode**: Mixed; PCAP and mbox streaming-capable

#### 3.15.1 Function List

TODO — table of 8 functions: PcapRead (Both), PcapFilter (Both), PcapStatistics, DnsRecordParse (Both), HarParse, EmailParse, EmailExtractAttachments, MboxSplit (Both)

#### 3.15.2 Design Notes

TODO — PCAP is sequential packet stream → streaming; BPF filter expression compiled once, applied per packet; email MIME parsing requires full message

#### 3.15.3 Test Strategy

TODO — PCAP packet count, BPF filter correctness, DNS zone file parsing, email attachment extraction, mbox message splitting

### 3.16 Category P: Bioinformatics Formats — FASTA, FASTQ, SAM/BAM, VCF, BED (12 functions, #441–#452)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `noodles`, `bio`
> **Mode**: Predominantly streaming (line-oriented); BAM batch (binary indexed)

#### 3.16.1 Function List

TODO — table of 12 functions: FastaParse (Both), FastqParse (Both), FastqTrimQuality (Both), FastqFilter (Both), SamParse (Both), BamRead, BamToSam (Both), VcfParse (Both), VcfFilter (Both), BedParse (Both), GffParse (Both), FastaToFastq

#### 3.16.2 Design Notes

TODO — FASTA/FASTQ/SAM/VCF/BED are line-oriented → streaming; BAM is binary with index → batch random access; quality trimming uses sliding window

#### 3.16.3 Test Strategy

TODO — sequence parsing accuracy, quality trim correctness, VCF variant filtering, BAM region query, format conversion

### 3.17 Category Q: Font, 3D & Specialized Binary — WOFF, glTF, STL, DICOM, WASM, ELF (13 functions, #453–#465)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `woff2`, `gltf`, `dicom-object`, `wasmparser`
> **Mode**: All batch (complex binary structures)

#### 3.17.1 Function List

TODO — table of 13 functions: FontMetadata, WoffToOtf, OtfToWoff2, GltfMetadata, GltfValidate, StlRead, StlConvert, DicomMetadata, DicomToImage, DicomAnonymize, WasmValidate, WasmMetadata, ElfMetadata

#### 3.17.2 Design Notes

TODO — DICOM anonymization is HIPAA-critical (remove patient-identifying tags); WASM validation checks module structure; glTF validation against spec

#### 3.17.3 Test Strategy

TODO — font metadata extraction, WOFF↔OTF roundtrip, DICOM anonymization completeness, WASM module validation, STL ASCII↔binary conversion

### 3.18 Category R: Erasure Coding & Redundancy (9 functions, #466–#474)

> **Phase**: 2 — Analytics backbone (DFS-critical)
> **Key dependencies**: `reed-solomon-erasure`
> **Mode**: All streaming-capable (chunk-aligned block operations)

#### 3.18.1 Function List

TODO — table of 9 functions: ReedSolomonEncode (Both), ReedSolomonDecode (Both), ReedSolomonVerify (Both), XorParity (Both), XorReconstruct (Both), ReplicationSplit (Both), ReplicationVerify, StripeSplit (Both), StripeAssemble (Both)

#### 3.18.2 Design Notes

TODO — erasure coding is fundamental to DFS (HDFS-3 EC, Ceph, MinIO); Reed-Solomon operates on chunk-aligned blocks → streaming natural; XOR parity is simplest (RAID-5 style); replication is trivial fan-out

#### 3.18.3 Test Strategy

TODO — RS encode/decode roundtrip with simulated shard loss, XOR parity reconstruction, replication identity verification, stripe split/assemble roundtrip

### 3.19 Category S: Universal Format Detection & Conversion (9 functions, #475–#483)

> **Phase**: 1 — High-value (meta-functions)
> **Key dependencies**: `infer`, `mime_guess`, `jsonschema`
> **Mode**: Mixed; FormatDetect streaming-capable (header only)

#### 3.19.1 Function List

TODO — table of 9 functions: FormatDetect (Both), FormatValidate, UniversalMetadata, UniversalToJson, UniversalToText, FormatConvert, SchemaInfer, SchemaCompare, MimeTypeMap

#### 3.19.2 Design Notes

TODO — meta-functions that dispatch to category-specific implementations; FormatDetect reads magic bytes from first chunk → streaming; UniversalToText is the full-text search indexing entry point; SchemaInfer works across CSV/Parquet/Avro/JSON

#### 3.19.3 Test Strategy

TODO — format detection accuracy across all supported formats, universal metadata extraction, schema inference for tabular formats, schema compatibility comparison

---

## 4. Implementation Strategy

### 4.1 Phase 1 — High-Value, Low-Complexity (50 functions)

TODO — Categories B (CSV/JSON), D (tar/gzip), H (YAML/TOML/INI), K (log parsing), L (CID/CAR), S (format detection); estimated 5–7 days; minimal external dependencies

### 4.2 Phase 2 — Analytics Backbone (60 functions)

TODO — Categories A (Parquet/ORC/Arrow), C (Avro/Protobuf), R (erasure coding); estimated 7–10 days; heavy dependencies (parquet, arrow, apache-avro)

### 4.3 Phase 3 — Rich Media & Documents (80 functions)

TODO — Categories E (images), F (audio/video), G (documents); estimated 5–8 days; image/media processing dependencies

### 4.4 Phase 4 — Domain-Specific (93 functions)

TODO — Categories I (geospatial), J (scientific), M (database), N (ML), O (network), P (bioinformatics), Q (specialized binary); estimated 5–8 days; many optional dependencies behind feature flags

### 4.5 Feature Flag Strategy

TODO — each category behind a Cargo feature flag: `format-columnar`, `format-csv`, `format-archive`, `format-image`, etc.; `format-all` enables everything; default features include Phase 1 only

### 4.6 Registration Pattern

TODO — each category provides `register_category_X(registry: &mut FunctionRegistry)` function; called conditionally based on feature flags

---

## 5. Test Specification

### 5.1 Per-Category Test Suites

TODO — each category gets its own test file: `tests/format_columnar.rs`, `tests/format_csv.rs`, etc.; ~3 tests per function = ~849 tests total

### 5.2 Cross-Category Conversion Tests

TODO — Parquet↔Avro, CSV↔JSON, YAML↔JSON↔TOML, Arrow↔NumPy; ~15 cross-format tests

### 5.3 Format Detection Integration Tests

TODO — FormatDetect correctly identifies all supported formats; ~20 detection tests with sample files

### 5.4 Streaming Equivalence Tests

TODO — for all 107 Both-mode functions, verify batch and streaming produce identical output; ~107 parity tests

### 5.5 Benchmark Suite

TODO — per-category throughput benchmarks; Parquet read/write at various sizes; compression codec comparison for archives; image resize at various resolutions

---

## 6. Edge Cases & Error Handling

TODO — table: corrupt file headers, truncated files, unsupported format versions, encoding mismatches, empty files, files exceeding memory for batch-only functions, concurrent access to SQLite, DICOM with missing required tags

---

## 7. Performance Analysis

### 7.1 Columnar Format Performance

TODO — Parquet read with projection: skip unneeded columns → 10–100× speedup; predicate pushdown: skip row groups → 2–50× speedup depending on selectivity

### 7.2 Archive Performance

TODO — tar streaming vs zip random access; tar.gz pipeline composition overhead

### 7.3 Image Processing Performance

TODO — resize throughput vs resolution; format conversion overhead

### 7.4 Erasure Coding Performance

TODO — Reed-Solomon encode/decode throughput; XOR parity throughput; scaling with shard count

---

## 8. Files Changed

TODO — table: new files per category (`builtins_format_columnar.rs`, `builtins_format_csv.rs`, etc.), `Cargo.toml` feature flags, registry integration, test files

---

## 9. Dependency Changes

TODO — comprehensive table by phase:
- Phase 1: `csv` 1, `quick-xml` 0.36, `tar` 0.4, `zip` 2, `flate2` 1, `bzip2` 0.4, `xz2` 0.1, `serde_yaml` 0.9, `toml` 0.8, `configparser` 3, `hcl-rs` 0.18, `cid` 0.11, `multihash` 0.19, `iroh-car` 0.6, `infer` 0.16, `mime_guess` 2
- Phase 2: `parquet` 53, `arrow` 53, `orc-rust` 0.4, `apache-avro` 0.17, `prost` 0.13, `rmp-serde` 1, `ciborium` 0.2, `bson` 2, `reed-solomon-erasure` 6
- Phase 3: `image` 0.25, `resvg` 0.43, `img-hash` 3, `lopdf` 0.33, `calamine` 0.26, `scraper` 0.20, `pulldown-cmark` 0.12, `symphonia` 0.5
- Phase 4: `geojson` 0.24, `flatgeobuf` 4, `hdf5` 0.8, `rusqlite` 0.32, `safetensors` 0.4, `wasmparser` 0.218, `noodles` 0.82, `pcap-parser` 0.16, `mailparse` 0.15, `dicom-object` 0.7, `gltf` 1

---

## 10. Design Rationale

### 10.1 Why Feature Flags Instead of Separate Crates?

TODO — single crate with feature flags keeps the registry unified; separate crates would require dynamic loading or compile-time selection; feature flags are idiomatic Rust

### 10.2 Why Phase 1 Prioritizes CSV/Log/Config Over Parquet/Arrow?

TODO — CSV/log/config are ubiquitous, low-dependency, and immediately useful; Parquet/Arrow bring heavy dependencies and are needed primarily for analytics workloads

### 10.3 Why Include Niche Formats (DICOM, FASTA, PCAP)?

TODO — data lakes in healthcare, genomics, and security store these formats; a universal CAS must handle them; feature flags make them optional

### 10.4 Why 283 Functions Instead of Fewer Generic Ones?

TODO — specific functions enable precise CAddr recipes (e.g., `ParquetProjection(columns=["age","name"])` vs generic `read(format="parquet", options=...)`); specific functions have better type safety, error messages, and optimization opportunities

---

## 11. Observability Integration

TODO — per-function metrics (same as §2.14/§2.15), per-category aggregate counters, format detection accuracy tracking, per-format throughput histograms

---

## 12. Checklist

- [ ] Phase 1: Implement Category B — CSV/JSON/XML (25 functions)
- [ ] Phase 1: Implement Category D — Archive formats (20 functions)
- [ ] Phase 1: Implement Category H — Config formats (16 functions)
- [ ] Phase 1: Implement Category K — Log formats (13 functions)
- [ ] Phase 1: Implement Category L — Content-addressing (15 functions)
- [ ] Phase 1: Implement Category S — Universal detection (9 functions)
- [ ] Phase 2: Implement Category A — Columnar formats (18 functions)
- [ ] Phase 2: Implement Category C — Serialization formats (19 functions)
- [ ] Phase 2: Implement Category R — Erasure coding (9 functions)
- [ ] Phase 3: Implement Category E — Image formats (18 functions)
- [ ] Phase 3: Implement Category F — Audio/Video formats (16 functions)
- [ ] Phase 3: Implement Category G — Document formats (23 functions)
- [ ] Phase 4: Implement Categories I, J, M, N, O, P, Q (93 functions)
- [ ] Set up Cargo feature flags per category
- [ ] Registration functions per category
- [ ] Per-category test suites (~849 tests)
- [ ] Cross-category conversion tests (~15 tests)
- [ ] Format detection integration tests (~20 tests)
- [ ] Streaming equivalence tests (~107 tests)
- [ ] Per-category benchmarks
- [ ] Add observability metrics
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing tests still pass
- [ ] Commit and push
