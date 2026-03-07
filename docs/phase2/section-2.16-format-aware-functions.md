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

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 340 | YamlParseFn | `yaml_parse@1.0.0` | Batch | — | JSON |
| 341 | YamlWriteFn | `yaml_write@1.0.0` | Batch | — | YAML bytes |
| 342 | YamlValidateFn | `yaml_validate@1.0.0` | Batch | — | Pass-through or error |
| 343 | YamlMergeFn | `yaml_merge@1.0.0` | Batch | `strategy` (`shallow`/`deep`) | YAML bytes (merged) |
| 344 | TomlParseFn | `toml_parse@1.0.0` | Batch | — | JSON |
| 345 | TomlWriteFn | `toml_write@1.0.0` | Batch | — | TOML bytes |
| 346 | IniParseFn | `ini_parse@1.0.0` | Batch | — | JSON (sections as nested objects) |
| 347 | IniWriteFn | `ini_write@1.0.0` | Batch | — | INI bytes |
| 348 | EnvParseFn | `env_parse@1.0.0` | Batch | — | JSON (key-value pairs) |
| 349 | EnvWriteFn | `env_write@1.0.0` | Batch | — | `.env` bytes |
| 350 | HclParseFn | `hcl_parse@1.0.0` | Batch | — | JSON |
| 351 | PropertiesParseFn | `properties_parse@1.0.0` | Batch | — | JSON (key-value pairs) |
| 352 | PropertiesWriteFn | `properties_write@1.0.0` | Batch | — | Java `.properties` bytes |
| 353 | PlistParseFn | `plist_parse@1.0.0` | Batch | — | JSON |
| 354 | PlistWriteFn | `plist_write@1.0.0` | Batch | — | XML plist bytes |
| 355 | ConfigFormatConvertFn | `config_format_convert@1.0.0` | Batch | `from`, `to` (format names) | Target format bytes |

#### 3.8.2 Design Notes

All config formats are batch-only because:
1. Files are small (typically <1 MB)
2. Full parse is required for correct interpretation
3. No benefit from streaming at these sizes

**JSON as hub**: all parse functions emit JSON, all write functions
consume JSON. This enables the universal converter
`ConfigFormatConvertFn` which routes through JSON:
`source → JSON → target`.

Supported format names for `ConfigFormatConvertFn`: `yaml`, `toml`,
`ini`, `env`, `hcl`, `properties`, `plist`, `json`.

**YamlMergeFn** (#343) takes N inputs and merges them. Strategy:
- `shallow`: top-level keys from later inputs override earlier
- `deep`: recursively merge nested objects; arrays concatenated;
  scalars from later inputs override

This is the standard config overlay pattern (base.yaml + env.yaml +
local.yaml).

**Multi-document YAML**: `YamlParseFn` parses only the first document.
Multi-document YAML (`---` separated) is uncommon in config files.
If needed, a future `YamlParseAllFn` can handle it.

**INI sections** map to nested JSON objects:
```ini
[database]
host = localhost
port = 5432
```
→
```json
{"database": {"host": "localhost", "port": "5432"}}
```

**HCL** (HashiCorp Configuration Language) is used by Terraform,
Vault, Consul. `hcl-rs` parses HCL2 syntax into a JSON-compatible
structure. HCL blocks become nested objects.

**.env files** follow the `KEY=VALUE` format with optional quoting
and `#` comments. `EnvParseFn` handles quoted values, multiline
values, and variable expansion (`${VAR}`).

**Java .properties** uses `key=value` or `key:value` with `\` line
continuation. `configparser` handles the parsing.

```rust
// IniParseFn
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("requires UTF-8".into()))?;
    let mut config = configparser::ini::Ini::new();
    config.read(text.to_string())
        .map_err(|e| ComputeError::ExecutionFailed(format!("ini parse: {}", e)))?;
    let mut root = serde_json::Map::new();
    for section in config.sections() {
        let mut obj = serde_json::Map::new();
        if let Some(map) = config.get_map_ref().get(&section) {
            for (key, val) in map {
                obj.insert(key.clone(), match val {
                    Some(v) => serde_json::Value::String(v.clone()),
                    None => serde_json::Value::Null,
                });
            }
        }
        root.insert(section, serde_json::Value::Object(obj));
    }
    Ok(Bytes::from(serde_json::to_string_pretty(&root).unwrap()))
}
```

```rust
// ConfigFormatConvertFn — universal converter via JSON hub
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let from = get_string_param(params, "from")?;
    let to = get_string_param(params, "to")?;
    // Step 1: parse source → serde_json::Value
    let value = match from {
        "yaml" => serde_yaml::from_slice(&inputs[0]).map_err(|e| ComputeError::ExecutionFailed(format!("yaml: {}", e)))?,
        "toml" => toml::from_str(std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("UTF-8".into()))?)
            .map_err(|e| ComputeError::ExecutionFailed(format!("toml: {}", e)))?,
        "json" => serde_json::from_slice(&inputs[0]).map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?,
        _ => return Err(ComputeError::InvalidParam(format!("unsupported source format: {}", from))),
    };
    // Step 2: serialize → target format
    let output = match to {
        "yaml" => serde_yaml::to_string(&value).map_err(|e| ComputeError::ExecutionFailed(format!("yaml: {}", e)))?,
        "toml" => toml::to_string_pretty(&value).map_err(|e| ComputeError::ExecutionFailed(format!("toml: {}", e)))?,
        "json" => serde_json::to_string_pretty(&value).map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?,
        _ => return Err(ComputeError::InvalidParam(format!("unsupported target format: {}", to))),
    };
    Ok(Bytes::from(output))
}
```

#### 3.8.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| YAML with anchors/aliases | Resolved during parse |
| YAML with tags (`!!int`) | Interpreted by serde_yaml |
| TOML datetime types | Serialized as strings in JSON |
| TOML inline tables vs standard tables | Both parsed identically |
| INI with duplicate keys | Last value wins |
| INI with no section (global keys) | Placed under `"default"` section |
| .env with `export` prefix | `export KEY=VALUE` → strips `export` |
| .env with empty value | `KEY=` → `{"KEY": ""}` |
| HCL with Terraform-specific syntax | Parsed as generic HCL blocks |
| Properties with Unicode escapes (`\uXXXX`) | Decoded to UTF-8 |
| Plist binary format | `ExecutionFailed("only XML plist supported")` |
| Convert between incompatible formats (e.g., INI → TOML with nested) | Best-effort; flat structure preserved |

#### 3.8.4 Test Strategy

- Parse/write roundtrip for each format: YAML, TOML, INI, .env, properties, plist
- YAML merge: shallow override, deep recursive merge, array concatenation
- YAML validate: valid YAML → pass-through, invalid → error
- INI section parsing: verify nested JSON structure
- HCL parse: Terraform-style config with blocks, attributes, expressions
- ConfigFormatConvert: YAML→TOML→JSON→YAML roundtrip (semantic preservation)
- .env with quoting: single quotes, double quotes, unquoted
- Properties with line continuation and Unicode escapes

### 3.9 Category I: Geospatial Formats — GeoJSON, Shapefile, KML, GeoTIFF (14 functions, #356–#369)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `geojson`, `geo`, `gdal` (optional), `flatgeobuf`
> **Mode**: Mixed; FlatGeobuf streaming-capable

#### 3.9.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 356 | GeoJsonValidateFn | `geojson_validate@1.0.0` | Batch | — | Pass-through or error |
| 357 | GeoJsonFilterFn | `geojson_filter@1.0.0` | Batch | `property`, `op`, `value` | GeoJSON (filtered features) |
| 358 | GeoJsonBboxFn | `geojson_bbox@1.0.0` | Batch | — | JSON (`[min_lon, min_lat, max_lon, max_lat]`) |
| 359 | GeoJsonSimplifyFn | `geojson_simplify@1.0.0` | Batch | `tolerance` (default `"0.001"`) | GeoJSON (simplified geometries) |
| 360 | GeoJsonToWktFn | `geojson_to_wkt@1.0.0` | Batch | — | WKT text (one geometry per line) |
| 361 | WktToGeoJsonFn | `wkt_to_geojson@1.0.0` | Batch | — | GeoJSON |
| 362 | ShapefileToGeoJsonFn | `shapefile_to_geojson@1.0.0` | Batch | — | GeoJSON |
| 363 | KmlToGeoJsonFn | `kml_to_geojson@1.0.0` | Batch | — | GeoJSON |
| 364 | GpxToGeoJsonFn | `gpx_to_geojson@1.0.0` | Batch | — | GeoJSON |
| 365 | GeoTiffMetadataFn | `geotiff_metadata@1.0.0` | Batch | — | JSON (CRS, bounds, resolution, bands) |
| 366 | GeoTiffCropFn | `geotiff_crop@1.0.0` | Batch | `bbox` (`"min_lon,min_lat,max_lon,max_lat"`) | GeoTIFF bytes (cropped) |
| 367 | GeoTiffResampleFn | `geotiff_resample@1.0.0` | Batch | `width`, `height`, `method` (`nearest`/`bilinear`) | GeoTIFF bytes (resampled) |
| 368 | FlatGeobufReadFn | `flatgeobuf_read@1.0.0` | Both | `bbox` (optional spatial filter) | GeoJSON |
| 369 | FlatGeobufWriteFn | `flatgeobuf_write@1.0.0` | Both | — | FlatGeobuf bytes |

#### 3.9.2 Design Notes

**GeoJSON** is JSON → full parse required for structural operations.
The `geojson` crate provides typed parsing. The `geo` crate provides
computational geometry algorithms (simplification, bounding box,
area).

**Simplification** (`GeoJsonSimplifyFn`) uses the Ramer-Douglas-Peucker
algorithm via `geo::algorithm::simplify`. The `tolerance` param
controls the maximum distance a simplified point can deviate from the
original geometry (in coordinate units, typically degrees).

**FlatGeobuf** is designed for streaming — features are stored
sequentially with an optional spatial index. The streaming variant
reads one feature per chunk. With `bbox` param, the spatial index
enables efficient spatial filtering without reading all features.

**GeoTIFF** operations require `gdal` (optional, behind
`format-geo-gdal` feature flag). Without GDAL, only metadata
extraction is available via TIFF tag parsing. Crop and resample
require raster data access.

**Shapefile** is a multi-file format (.shp, .shx, .dbf). Input is
expected as a ZIP archive containing all component files. The function
unzips, parses, and converts to GeoJSON.

**KML/GPX** are XML-based formats. Parsing uses `quick-xml` (already
in workspace from Category B) with format-specific element mapping.

All conversion functions normalize to GeoJSON as the hub format,
consistent with the JSON-hub pattern used throughout §2.16.

```rust
// GeoJsonBboxFn
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("requires UTF-8".into()))?;
    let gj: geojson::GeoJson = text.parse()
        .map_err(|e| ComputeError::ExecutionFailed(format!("geojson: {}", e)))?;
    let collection = geojson::FeatureCollection::try_from(gj)
        .map_err(|e| ComputeError::ExecutionFailed(format!("expected FeatureCollection: {}", e)))?;
    let mut bbox = [f64::MAX, f64::MAX, f64::MIN, f64::MIN]; // [min_lon, min_lat, max_lon, max_lat]
    for feature in &collection.features {
        if let Some(ref geom) = feature.geometry {
            update_bbox_from_geometry(geom, &mut bbox);
        }
    }
    Ok(Bytes::from(serde_json::to_string(&bbox).unwrap()))
}
```

#### 3.9.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| GeoJSON with no features | Validate → pass-through; Bbox → `ExecutionFailed("no features")` |
| GeoJSON with null geometry | Feature preserved, skipped in bbox/simplify |
| Simplify with tolerance 0 | No simplification (pass-through) |
| Simplify collapses polygon to line | Polygon removed from output |
| WKT with unsupported geometry type | `ExecutionFailed("unsupported: ...")` |
| Shapefile ZIP missing .dbf | `ExecutionFailed("missing .dbf component")` |
| GeoTIFF without CRS | Metadata reports `"crs": null` |
| GeoTIFF crop bbox outside raster extent | `ExecutionFailed("bbox outside raster bounds")` |
| FlatGeobuf with spatial index + bbox filter | Uses index for O(log n) lookup |
| FlatGeobuf without spatial index + bbox filter | Falls back to sequential scan |
| KML with nested folders | Flattened to single FeatureCollection |

#### 3.9.4 Test Strategy

- GeoJSON validate: valid → pass-through, invalid → error
- GeoJSON filter: property-based feature selection
- GeoJSON bbox: verify against manually computed bounds
- GeoJSON simplify: output vertex count < input; topology preserved
- GeoJSON↔WKT roundtrip: geometry types preserved
- Shapefile→GeoJSON: verify feature count and property names
- KML/GPX→GeoJSON: verify coordinate extraction
- FlatGeobuf read/write roundtrip: feature count and properties preserved
- FlatGeobuf streaming: batch and streaming produce identical GeoJSON
- GeoTIFF metadata: verify CRS, bounds, band count for known file

### 3.10 Category J: Scientific & Numerical Data — HDF5, NetCDF, FITS, NumPy, Zarr (13 functions, #370–#382)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `hdf5`, `netcdf`, `fitsio`, `ndarray`
> **Mode**: All batch (random-access array formats)

#### 3.10.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 370 | Hdf5MetadataFn | `hdf5_metadata@1.0.0` | Batch | — | JSON (groups, datasets, attributes, shapes, dtypes) |
| 371 | Hdf5ReadFn | `hdf5_read@1.0.0` | Batch | `dataset` (path, e.g. `"/group/data"`) | Arrow IPC bytes |
| 372 | Hdf5WriteFn | `hdf5_write@1.0.0` | Batch | `dataset`, `shape`, `dtype` | HDF5 bytes |
| 373 | NetcdfMetadataFn | `netcdf_metadata@1.0.0` | Batch | — | JSON (dimensions, variables, global attributes) |
| 374 | NetcdfReadFn | `netcdf_read@1.0.0` | Batch | `variable` | Arrow IPC bytes |
| 375 | FitsMetadataFn | `fits_metadata@1.0.0` | Batch | — | JSON (HDU list, headers, dimensions) |
| 376 | FitsReadFn | `fits_read@1.0.0` | Batch | `hdu` (index, default `"0"`) | Arrow IPC bytes |
| 377 | NumpyReadFn | `numpy_read@1.0.0` | Batch | — | Arrow IPC bytes |
| 378 | NumpyWriteFn | `numpy_write@1.0.0` | Batch | `dtype` (`f32`/`f64`/`i32`/`i64`), `shape` | `.npy` bytes |
| 379 | ZarrReadFn | `zarr_read@1.0.0` | Batch | `array` (path within store) | Arrow IPC bytes |
| 380 | ZarrMetadataFn | `zarr_metadata@1.0.0` | Batch | — | JSON (arrays, shapes, chunks, dtypes, compressor) |
| 381 | NumpyToArrowFn | `numpy_to_arrow@1.0.0` | Batch | — | Arrow IPC bytes |
| 382 | ArrowToNumpyFn | `arrow_to_numpy@1.0.0` | Batch | `dtype` | `.npy` bytes |

#### 3.10.2 Design Notes

All scientific formats store multi-dimensional arrays with metadata.
They require random access for dataset/variable slicing → all batch.

**Arrow IPC as interchange**: all read functions emit Arrow IPC,
enabling downstream processing with Category A columnar functions.
Multi-dimensional arrays are flattened to Arrow record batches
(one column per array dimension or variable).

**HDF5** is a hierarchical container with groups (directories) and
datasets (arrays). The `hdf5` crate wraps the C library. Datasets
can be sliced by hyperslab (start, stride, count) — a future
`Hdf5SliceFn` could expose this.

**NetCDF** is built on HDF5 (NetCDF-4) or has its own format
(NetCDF-3/classic). Variables are named arrays with dimensions and
attributes. Climate/weather data is the primary use case.

**FITS** (Flexible Image Transport System) is the standard in
astronomy. Files contain Header Data Units (HDUs), each with a
header (key-value pairs) and data (image or table). `fitsio` wraps
the CFITSIO C library.

**NumPy `.npy`** is a simple format: magic bytes + header (shape,
dtype, order) + raw array data. Parsing is straightforward without
external C dependencies.

**Zarr** stores arrays as individually addressable chunks in a
directory-like structure. Each chunk is a separate blob — a natural
fit for CAS (each chunk gets its own CAddr). `ZarrReadFn` takes a
ZIP or directory-in-tar representation of the Zarr store.
`ZarrMetadataFn` reads `.zarray` and `.zattrs` JSON files.

```rust
// NumpyReadFn — parse .npy, emit Arrow IPC
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let input = &inputs[0];
    // Verify magic: \x93NUMPY
    if input.len() < 10 || &input[..6] != b"\x93NUMPY" {
        return Err(ComputeError::ExecutionFailed("not a valid .npy file".into()));
    }
    let header_len = u16::from_le_bytes([input[8], input[9]]) as usize;
    let header_str = std::str::from_utf8(&input[10..10 + header_len])
        .map_err(|_| ComputeError::ExecutionFailed("invalid npy header".into()))?;
    // Parse dtype, shape, fortran_order from header dict
    let (dtype, shape) = parse_npy_header(header_str)?;
    let data = &input[10 + header_len..];
    // Convert raw bytes to Arrow array based on dtype and shape
    let arrow_batch = raw_to_arrow(data, &dtype, &shape)?;
    serialize_arrow_ipc(&arrow_batch)
}
```

#### 3.10.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| HDF5 dataset path not found | `InvalidParam("dataset '/x' not found")` |
| HDF5 with external links | `ExecutionFailed("external links not supported")` |
| NetCDF unlimited dimension | Read all available records |
| NetCDF variable not found | `InvalidParam("variable 'x' not found")` |
| FITS with multiple HDUs | `hdu` param selects which; default 0 (primary) |
| FITS image HDU vs table HDU | Both converted to Arrow; image flattened to 1D |
| NumPy Fortran order | Transposed to C order during read |
| NumPy structured dtype | Each field becomes an Arrow column |
| NumPy object dtype | `ExecutionFailed("object dtype not supported")` |
| Zarr with missing chunks | Filled with fill_value from `.zarray` metadata |
| Zarr with unsupported compressor | `ExecutionFailed("unsupported compressor: ...")` |
| Arrow→NumPy with mixed column types | `InvalidParam("all columns must be same dtype")` |

#### 3.10.4 Test Strategy

- HDF5 metadata: verify group/dataset hierarchy for known file
- HDF5 read/write roundtrip: dataset values preserved
- NetCDF metadata: verify dimensions, variables, attributes
- NetCDF read: verify variable values against known data
- FITS metadata: verify HDU list and header keywords
- NumPy read/write roundtrip: array values and shape preserved
- NumPy↔Arrow roundtrip: dtype mapping correctness (f32, f64, i32, i64)
- Zarr metadata: verify chunk shape, dtype, compressor
- Zarr read: verify array values with known chunked data

### 3.11 Category K: Log & Observability Formats — Syslog, CEF, Apache/Nginx, CloudTrail, OTLP (13 functions, #383–#395)

> **Phase**: 1 — High-value, low-complexity
> **Key dependencies**: `regex`, `serde_json`
> **Mode**: Predominantly streaming (line-oriented)

#### 3.11.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 383 | SyslogParseFn | `syslog_parse@1.0.0` | Both | `format` (`rfc3164`/`rfc5424`, default `"rfc5424"`) | NDJSON |
| 384 | SyslogWriteFn | `syslog_write@1.0.0` | Both | `format` | Syslog text |
| 385 | CefParseFn | `cef_parse@1.0.0` | Both | — | NDJSON |
| 386 | ElfParseFn | `elf_parse@1.0.0` | Both | — | NDJSON (Extended Log Format) |
| 387 | ApacheLogParseFn | `apache_log_parse@1.0.0` | Both | `format` (`combined`/`common`, default `"combined"`) | NDJSON |
| 388 | NginxLogParseFn | `nginx_log_parse@1.0.0` | Both | `format` (`combined`/`json`, default `"combined"`) | NDJSON |
| 389 | CloudTrailParseFn | `cloudtrail_parse@1.0.0` | Batch | — | NDJSON (one event per line) |
| 390 | VpcFlowLogParseFn | `vpc_flow_log_parse@1.0.0` | Both | — | NDJSON |
| 391 | PrometheusExpositionParseFn | `prometheus_exposition_parse@1.0.0` | Both | — | JSON (metrics with labels) |
| 392 | OtlpDecodeFn | `otlp_decode@1.0.0` | Batch | `signal` (`traces`/`metrics`/`logs`) | JSON |
| 393 | LogTimestampNormalizeFn | `log_timestamp_normalize@1.0.0` | Both | `output_format` (default `"iso8601"`) | NDJSON (timestamps normalized) |
| 394 | LogLevelFilterFn | `log_level_filter@1.0.0` | Both | `min_level` (`debug`/`info`/`warn`/`error`/`fatal`) | NDJSON (filtered) |
| 395 | LogAnonymizeFn | `log_anonymize@1.0.0` | Both | `fields` (comma-separated: `ip`, `email`, `user`) | NDJSON (PII redacted) |

#### 3.11.2 Design Notes

Log formats are line-oriented → natural streaming. Each line is
parsed independently, emitted as one NDJSON record. The streaming
variants process one line per chunk boundary (splitting on `\n`).

**Syslog RFC 5424** format:
`<priority>version timestamp hostname app-name procid msgid structured-data msg`
Parsed into JSON fields: `priority`, `facility`, `severity`,
`timestamp`, `hostname`, `app_name`, `procid`, `msgid`, `message`.

**CEF** (Common Event Format) is used by SIEM systems:
`CEF:version|vendor|product|version|id|name|severity|extensions`
Extensions are key-value pairs.

**Apache/Nginx combined** log format:
`ip - user [timestamp] "method path protocol" status size "referer" "user-agent"`
Parsed via regex into structured fields.

**CloudTrail** is a JSON array of events → batch-only (full parse
needed to extract the `Records` array). Each event becomes one
NDJSON line.

**OTLP** (OpenTelemetry Protocol) uses Protobuf encoding. The
`signal` param selects traces, metrics, or logs. Decoded to JSON
using embedded proto descriptors.

**LogTimestampNormalizeFn** (#393) auto-detects common timestamp
formats and normalizes to a consistent output:
- ISO 8601: `2024-01-15T10:30:00Z`
- Syslog: `Jan 15 10:30:00`
- Apache: `[15/Jan/2024:10:30:00 +0000]`
- Unix epoch: `1705312200`
- Custom: `2024/01/15 10:30:00`

**LogAnonymizeFn** (#395) replaces PII with deterministic hashes:
- `ip`: IPv4/IPv6 addresses → `REDACTED_IP_<hash8>`
- `email`: email addresses → `REDACTED_EMAIL_<hash8>`
- `user`: username fields → `REDACTED_USER_<hash8>`

Hashes are deterministic (same input → same redacted value) to
preserve correlation analysis while removing PII.

```rust
// ApacheLogParseFn — parse combined log format
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("requires UTF-8".into()))?;
    let re = regex::Regex::new(
        r#"^(\S+) \S+ (\S+) \[([^\]]+)\] "(\S+) (\S+) (\S+)" (\d+) (\d+|-) "([^"]*)" "([^"]*)""#
    ).unwrap();
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.is_empty() { continue; }
        if let Some(caps) = re.captures(line) {
            let record = serde_json::json!({
                "remote_addr": caps.get(1).map(|m| m.as_str()),
                "remote_user": caps.get(2).map(|m| m.as_str()),
                "timestamp": caps.get(3).map(|m| m.as_str()),
                "method": caps.get(4).map(|m| m.as_str()),
                "path": caps.get(5).map(|m| m.as_str()),
                "protocol": caps.get(6).map(|m| m.as_str()),
                "status": caps.get(7).map(|m| m.as_str()),
                "body_bytes": caps.get(8).map(|m| m.as_str()),
                "referer": caps.get(9).map(|m| m.as_str()),
                "user_agent": caps.get(10).map(|m| m.as_str()),
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }
    }
    Ok(Bytes::from(lines.join("\n")))
}
```

#### 3.11.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| Malformed syslog line | Skipped; emitted as `{"raw": "...","parse_error": true}` |
| Syslog without structured data | `structured_data` field is `null` |
| Apache log with `-` for missing fields | Mapped to `null` in JSON |
| CloudTrail with empty Records array | Empty NDJSON output |
| OTLP with unknown signal type | `InvalidParam("signal must be traces, metrics, or logs")` |
| Timestamp in unknown format | Preserved as-is with `"normalized": false` flag |
| Log level not detected | Line passes through LogLevelFilter unchanged |
| Anonymize with no matching PII | Line unchanged |
| VPC flow log v2 vs v5 fields | Auto-detected from header line |
| Prometheus exposition with histograms | Bucket/sum/count expanded to separate metrics |

#### 3.11.4 Test Strategy

- Syslog parse: RFC 3164 and RFC 5424 format correctness
- Syslog write/parse roundtrip: semantic preservation
- CEF parse: verify vendor, product, severity, extension key-values
- Apache/Nginx parse: verify all fields extracted from combined format
- CloudTrail parse: verify event count matches Records array length
- VPC flow log: verify field extraction for v2 and v5 formats
- Timestamp normalize: verify all 5 input formats → ISO 8601
- Level filter: verify `min_level=warn` drops debug/info lines
- Anonymize: verify PII replaced, deterministic hashes, non-PII unchanged
- Streaming parity for all Both-mode functions

### 3.12 Category L: Blockchain & Content-Addressing — CID, DAG-PB, DAG-CBOR, CAR, UnixFS (15 functions, #396–#410)

> **Phase**: 1 — High-value (directly relevant to CAS)
> **Key dependencies**: `cid`, `multihash`, `libipld`, `iroh-car`
> **Mode**: Mixed; CAR and CID streaming-capable

#### 3.12.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 396 | CidComputeFn | `cid_compute@1.0.0` | Both | `version` (`0`/`1`, default `"1"`), `codec` (`dag-pb`/`dag-cbor`/`raw`, default `"raw"`), `hash` (`sha2-256`/`blake3`, default `"sha2-256"`) | CID string (multibase-encoded) |
| 397 | CidVerifyFn | `cid_verify@1.0.0` | Both | `expected_cid` | Pass-through or error |
| 398 | CidParseFn | `cid_parse@1.0.0` | Batch | — | JSON (`{"version":1,"codec":"raw","hash":"sha2-256","digest":"..."}`) |
| 399 | DagPbEncodeFn | `dag_pb_encode@1.0.0` | Batch | — | DAG-PB bytes |
| 400 | DagPbDecodeFn | `dag_pb_decode@1.0.0` | Batch | — | JSON (links + data) |
| 401 | DagCborEncodeFn | `dag_cbor_encode@1.0.0` | Batch | — | DAG-CBOR bytes (canonical) |
| 402 | DagCborDecodeFn | `dag_cbor_decode@1.0.0` | Batch | — | JSON |
| 403 | CarCreateFn | `car_create@1.0.0` | Both | `roots` (comma-separated CIDs) | CARv1 bytes |
| 404 | CarExtractFn | `car_extract@1.0.0` | Both | `cid` (optional — single block) | JSON manifest or raw block |
| 405 | CarListFn | `car_list@1.0.0` | Both | — | JSON array of `{"cid":"...","size":N}` |
| 406 | CarVerifyFn | `car_verify@1.0.0` | Both | — | Pass-through or error (verifies all block CIDs) |
| 407 | UnixFsChunkFn | `unixfs_chunk@1.0.0` | Both | `chunk_size` (default `"262144"`), `strategy` (`fixed`/`rabin`) | DAG-PB root block + child blocks as CAR |
| 408 | UnixFsAssembleFn | `unixfs_assemble@1.0.0` | Batch | — | Raw file bytes (reassembled from UnixFS DAG) |
| 409 | MerkleProofGenerateFn | `merkle_proof_generate@1.0.0` | Batch | `target_cid` | JSON proof (path from root to target) |
| 410 | MerkleProofVerifyFn | `merkle_proof_verify@1.0.0` | Batch | `root_cid` | JSON (`{"valid":true}`) or error |

#### 3.12.2 Design Notes

This category is directly relevant to the Computation-Addressed Store.
IPFS/IPLD primitives provide interoperability with the broader
content-addressing ecosystem.

**CID** (Content Identifier) is a self-describing content address:
`<multibase><version><multicodec><multihash>`. `CidComputeFn` hashes
the input and wraps it in a CID. The streaming variant accumulates
the hash across chunks and emits the CID on flush.

**DAG-PB** (Protobuf-based DAG) is the original IPFS block format.
A node has `Data` (bytes) and `Links` (array of `{Name, Hash, Tsize}`).
Used by UnixFS for file chunking.

**DAG-CBOR** is the canonical IPLD encoding — deterministic CBOR with
CID links. Critical for structured data in content-addressed systems.
Encoding must follow canonical rules: sorted map keys, no duplicate
keys, minimal integer encoding.

**CAR** (Content Addressable aRchive) is a sequential archive of
IPLD blocks. Header contains root CIDs, followed by
`<varint-length><cid><block-data>` entries. Sequential format →
streaming natural. The streaming variant reads/writes one block per
chunk.

**UnixFS** is IPFS's file representation. Large files are split into
chunks, each chunk becomes a DAG-PB leaf, and a balanced tree of
intermediate nodes links them. `UnixFsChunkFn` implements this:
- `fixed`: fixed-size chunks (default 256 KB)
- `rabin`: content-defined chunking via Rabin fingerprinting

Output is a CAR archive containing all blocks (root + intermediates +
leaves). The root CID is the file's content address.

**Merkle proofs** enable verifying that a specific block belongs to a
DAG without having the entire DAG. `MerkleProofGenerateFn` takes a
CAR archive and a target CID, producing the path from root to target.
`MerkleProofVerifyFn` takes a proof and verifies the hash chain.

```rust
// CidComputeFn
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use cid::Cid;
    use multihash::{Code, MultihashDigest};
    let version = match params.get("version") {
        Some(Value::String(s)) => s.parse::<u64>().map_err(|_| ComputeError::InvalidParam("version: 0 or 1".into()))?,
        None => 1,
        _ => return Err(ComputeError::InvalidParam("version must be string".into())),
    };
    let codec = match params.get("codec") {
        Some(Value::String(s)) => match s.as_str() {
            "raw" => 0x55,
            "dag-pb" => 0x70,
            "dag-cbor" => 0x71,
            _ => return Err(ComputeError::InvalidParam("codec: raw|dag-pb|dag-cbor".into())),
        },
        None => 0x55,
        _ => return Err(ComputeError::InvalidParam("codec must be string".into())),
    };
    let hash = Code::Sha2_256.digest(&inputs[0]);
    let cid = if version == 0 {
        Cid::new_v0(hash).map_err(|e| ComputeError::ExecutionFailed(format!("cidv0: {}", e)))?
    } else {
        Cid::new_v1(codec, hash)
    };
    Ok(Bytes::from(cid.to_string()))
}
```

```rust
// CarListFn (streaming — reads blocks sequentially)
fn process_chunk(&mut self, chunk: Bytes) -> Result<Option<Bytes>, ComputeError> {
    self.buffer.extend_from_slice(&chunk);
    let mut entries = Vec::new();
    while let Some((cid, block, rest)) = try_read_car_block(&self.buffer)? {
        entries.push(serde_json::json!({"cid": cid.to_string(), "size": block.len()}));
        self.buffer = rest;
    }
    if entries.is_empty() { return Ok(None); }
    let lines: Vec<String> = entries.iter().map(|e| serde_json::to_string(e).unwrap()).collect();
    Ok(Some(Bytes::from(lines.join("\n"))))
}
```

#### 3.12.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| CIDv0 with non-dag-pb codec | `ExecutionFailed("CIDv0 requires dag-pb codec")` |
| CIDv0 with non-sha2-256 hash | `ExecutionFailed("CIDv0 requires sha2-256")` |
| CID verify mismatch | `ExecutionFailed("CID mismatch: expected ..., got ...")` |
| CID parse of invalid multibase | `ExecutionFailed("invalid CID: ...")` |
| DAG-CBOR with non-canonical encoding | `ExecutionFailed("non-canonical CBOR")` |
| DAG-CBOR with CID links | Links decoded as `{"/": "bafy..."}` JSON |
| CAR with no blocks | Valid CAR with header only |
| CAR block CID mismatch | `CarVerifyFn` reports which block failed |
| UnixFS chunk with rabin on small input | Single chunk (no splitting) |
| UnixFS assemble with missing blocks | `ExecutionFailed("missing block: bafy...")` |
| Merkle proof for non-existent CID | `ExecutionFailed("target CID not in DAG")` |
| Empty input → CID | Valid CID of empty bytes |

#### 3.12.4 Test Strategy

- CID compute: verify against known IPFS CID test vectors
- CID compute/verify roundtrip: compute then verify succeeds
- CID parse: verify version, codec, hash algorithm extraction
- DAG-PB encode/decode roundtrip: links and data preserved
- DAG-CBOR encode/decode roundtrip: canonical ordering verified
- DAG-CBOR with CID links: links survive encode/decode
- CAR create/extract roundtrip: all blocks recoverable
- CAR list: verify block count and CIDs
- CAR verify: valid archive passes; tampered block fails
- UnixFS chunk/assemble roundtrip: original file recovered
- UnixFS fixed vs rabin: both produce valid DAGs
- Merkle proof generate/verify roundtrip: proof validates
- Streaming parity for CID, CAR, and UnixFS functions

### 3.13 Category M: Database & Storage Formats — SQLite, RocksDB SST, WAL, Postgres COPY (10 functions, #411–#420)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `rusqlite`, `rocksdb`
> **Mode**: Mixed; Postgres COPY and WAL streaming-capable

#### 3.13.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 411 | SqliteQueryFn | `sqlite_query@1.0.0` | Batch | `sql` (SELECT statement) | CSV bytes |
| 412 | SqliteTableListFn | `sqlite_table_list@1.0.0` | Batch | — | JSON array of table names |
| 413 | SqliteToCsvFn | `sqlite_to_csv@1.0.0` | Batch | `table` | CSV bytes |
| 414 | CsvToSqliteFn | `csv_to_sqlite@1.0.0` | Batch | `table` | SQLite database bytes |
| 415 | PostgresCopyParseFn | `postgres_copy_parse@1.0.0` | Both | `format` (`text`/`binary`, default `"text"`) | CSV bytes |
| 416 | PostgresCopyWriteFn | `postgres_copy_write@1.0.0` | Both | `format` | Postgres COPY bytes |
| 417 | SstMetadataFn | `sst_metadata@1.0.0` | Batch | — | JSON (key range, entry count, compression, level) |
| 418 | SstScanFn | `sst_scan@1.0.0` | Batch | `start_key` (optional), `end_key` (optional) | NDJSON (`{"key":"...","value":"..."}` per entry) |
| 419 | WalParseFn | `wal_parse@1.0.0` | Both | `format` (`postgres`/`mysql`, default `"postgres"`) | NDJSON (one operation per line) |
| 420 | SqlDumpParseFn | `sql_dump_parse@1.0.0` | Both | — | NDJSON (one statement per line) |

#### 3.13.2 Design Notes

**SQLite** is a single-file embedded database with B-tree pages →
random access required → batch-only. `rusqlite` wraps the C library.
The input is a complete SQLite database file; output is query results
as CSV or table metadata as JSON.

`SqliteQueryFn` executes read-only SELECT statements only. The `sql`
param is validated to reject writes (INSERT/UPDATE/DELETE/DROP/ALTER).
This is enforced by opening the database in read-only mode.

`CsvToSqliteFn` creates a new SQLite database with a single table,
inferring column types from CSV data (integer, real, text).

**Postgres COPY** is the bulk data format used by `COPY TO/FROM`.
Text format is tab-delimited with `\N` for NULL. Binary format has
a fixed header, then tuples with field count + length-prefixed values.
Text format is line-oriented → streaming natural. Binary format
has self-delimiting tuples → also streaming.

**RocksDB SST** (Sorted String Table) files are the on-disk format
for LSM-tree storage engines (RocksDB, LevelDB, CockroachDB, TiKV).
They have a sorted key-value structure with index blocks and metadata
blocks at the end → batch for metadata, sequential scan for data.

**WAL** (Write-Ahead Log) is sequential by nature → streaming.
Postgres WAL contains logical replication messages; MySQL binlog
contains row events. Each record becomes one NDJSON line with
operation type (`insert`/`update`/`delete`), table, and row data.

**SQL dump** files (from `pg_dump`, `mysqldump`) are sequential SQL
statements → streaming. Each statement becomes one NDJSON record
with `type` (`create_table`/`insert`/`copy`), `table`, and `data`.

```rust
// SqliteQueryFn
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let sql = get_string_param(params, "sql")?;
    // Reject non-SELECT statements
    let normalized = sql.trim().to_uppercase();
    if !normalized.starts_with("SELECT") {
        return Err(ComputeError::InvalidParam("only SELECT statements allowed".into()));
    }
    let tmp = tempfile::NamedTempFile::new()
        .map_err(|e| ComputeError::ExecutionFailed(format!("tmpfile: {}", e)))?;
    std::fs::write(tmp.path(), &inputs[0])
        .map_err(|e| ComputeError::ExecutionFailed(format!("write db: {}", e)))?;
    let conn = rusqlite::Connection::open_with_flags(
        tmp.path(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ).map_err(|e| ComputeError::ExecutionFailed(format!("sqlite: {}", e)))?;
    let mut stmt = conn.prepare(sql)
        .map_err(|e| ComputeError::ExecutionFailed(format!("sql: {}", e)))?;
    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count).map(|i| stmt.column_name(i).unwrap().to_string()).collect();
    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record(&col_names).map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
    let mut rows = stmt.query([])
        .map_err(|e| ComputeError::ExecutionFailed(format!("query: {}", e)))?;
    while let Some(row) = rows.next().map_err(|e| ComputeError::ExecutionFailed(format!("row: {}", e)))? {
        let record: Vec<String> = (0..col_count)
            .map(|i| row.get::<_, String>(i).unwrap_or_default())
            .collect();
        wtr.write_record(&record).map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
    }
    Ok(Bytes::from(wtr.into_inner().map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?))
}
```

#### 3.13.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| SQLite query with write statement | `InvalidParam("only SELECT statements allowed")` |
| SQLite corrupt database | `ExecutionFailed("sqlite: database disk image is malformed")` |
| SQLite empty table | CSV with header only |
| CSV→SQLite with mixed types in column | Inferred as TEXT |
| Postgres COPY binary with NULL fields | Represented as empty in CSV |
| SST with compression (snappy/zstd/lz4) | Decompressed transparently |
| SST key range scan with no matches | Empty NDJSON output |
| WAL with unsupported format | `InvalidParam("format: postgres or mysql")` |
| WAL truncated mid-record | Last incomplete record skipped with warning |
| SQL dump with multi-line INSERT | Concatenated into single statement record |
| SQLite with attached databases | Not supported; single database only |

#### 3.13.4 Test Strategy

- SQLite query: verify SELECT results match expected CSV
- SQLite table list: verify all tables returned
- SQLite↔CSV roundtrip: `CsvToSqlite → SqliteToCsv` preserves data
- Postgres COPY text parse/write roundtrip
- Postgres COPY binary parse: verify field extraction
- SST metadata: verify key range, entry count for known SST file
- SST scan: verify key-value pairs, range scan correctness
- WAL parse: verify operation types and row data
- SQL dump parse: verify statement extraction and classification
- Streaming parity for Postgres COPY, WAL, and SQL dump functions

### 3.14 Category N: Machine Learning & Tensor Formats — TFRecord, SafeTensors, ONNX, GGUF (12 functions, #421–#432)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `safetensors`, `ort` (ONNX Runtime), `protobuf`
> **Mode**: Mixed; TFRecord streaming-capable

#### 3.14.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 421 | TfRecordReadFn | `tfrecord_read@1.0.0` | Both | `max_records` (optional) | NDJSON (one `tf.train.Example` per line) |
| 422 | TfRecordWriteFn | `tfrecord_write@1.0.0` | Both | — | TFRecord bytes |
| 423 | SafeTensorsReadFn | `safetensors_read@1.0.0` | Batch | `tensor` (name, optional — all if omitted) | Raw tensor bytes (flat, little-endian) |
| 424 | SafeTensorsMetadataFn | `safetensors_metadata@1.0.0` | Batch | — | JSON (`{"tensors":{"name":{"dtype":"F32","shape":[3,4],"offset":[0,48]},...},"metadata":{...}}`) |
| 425 | SafeTensorsWriteFn | `safetensors_write@1.0.0` | Batch | `name`, `dtype` (`F16`/`BF16`/`F32`/`F64`/`I8`/`I32`/`I64`/`U8`), `shape` (comma-separated dims) | SafeTensors bytes |
| 426 | OnnxMetadataFn | `onnx_metadata@1.0.0` | Batch | — | JSON (opset, inputs, outputs, nodes count, producer) |
| 427 | OnnxValidateFn | `onnx_validate@1.0.0` | Batch | — | Pass-through or error (shape/type consistency) |
| 428 | GgufMetadataFn | `gguf_metadata@1.0.0` | Batch | — | JSON (architecture, quant type, context length, tensor count, file size) |
| 429 | PickleToJsonFn | `pickle_to_json@1.0.0` | Batch | `max_depth` (default `"10"`) | JSON |
| 430 | NumpyToSafeTensorsFn | `numpy_to_safetensors@1.0.0` | Batch | `name` (tensor name, default `"tensor"`) | SafeTensors bytes |
| 431 | TfRecordToParquetFn | `tfrecord_to_parquet@1.0.0` | Batch | — | Parquet bytes (features as columns) |
| 432 | ImageToTensorFn | `image_to_tensor@1.0.0` | Batch | `width`, `height`, `channels` (`"1"`/`"3"`/`"4"`, default `"3"`), `dtype` (default `"F32"`), `normalize` (`"true"`/`"false"`, default `"true"`) | Raw tensor bytes (CHW layout, little-endian) |

#### 3.14.2 Design Notes

**TFRecord** is TensorFlow's standard data format. Each record is:
`<uint64 length><uint32 masked_crc_of_length><byte[length] data><uint32 masked_crc_of_data>`.
Length-delimited with CRC → streaming natural (one `Example` per
chunk). The `data` is a serialized `tf.train.Example` protobuf
containing a `Features` map. `TfRecordReadFn` decodes each Example
to JSON with feature types (`bytes_list`, `float_list`, `int64_list`).

```rust
// TfRecordReadFn — streaming: emit one NDJSON line per record
fn process_chunk(&mut self, chunk: Bytes) -> Result<Option<Bytes>, ComputeError> {
    self.buffer.extend_from_slice(&chunk);
    let mut lines = Vec::new();
    while self.buffer.len() >= 12 {
        let len = u64::from_le_bytes(self.buffer[0..8].try_into().unwrap()) as usize;
        let total = 8 + 4 + len + 4; // length + crc + data + crc
        if self.buffer.len() < total { break; }
        let data = &self.buffer[12..12 + len];
        verify_masked_crc(&self.buffer[0..8], &self.buffer[8..12])?;
        verify_masked_crc(data, &self.buffer[12 + len..total])?;
        let example = decode_tf_example(data)?;
        lines.push(serde_json::to_string(&example).unwrap());
        self.buffer.advance(total);
    }
    if lines.is_empty() { return Ok(None); }
    Ok(Some(Bytes::from(lines.join("\n"))))
}
```

**SafeTensors** is Hugging Face's safe tensor serialization format.
File layout: `<u64 header_size><JSON header><tensor data>`. The JSON
header maps tensor names to `{dtype, shape, data_offsets: [start, end]}`.
Header-based offset lookup → random access → batch-only.
No arbitrary code execution (unlike Pickle), making it safe to load
untrusted models.

`SafeTensorsWriteFn` takes raw tensor bytes as input and wraps them
in the SafeTensors format. Multiple tensors require multiple pipeline
stages (one per tensor) concatenated.

**ONNX** (Open Neural Network Exchange) uses protobuf encoding.
`OnnxMetadataFn` extracts the `ModelProto` header: opset version,
input/output tensor shapes and types, node count, and producer info.
`OnnxValidateFn` performs shape inference — verifying that all
intermediate tensor shapes are consistent through the graph.

**GGUF** (GPT-Generated Unified Format) is the standard format for
quantized LLM models (llama.cpp). File starts with magic `GGUF`,
followed by metadata key-value pairs and tensor info. Metadata
includes architecture (`llama`/`mistral`/`phi`), quantization type
(`Q4_0`/`Q5_K_M`/`Q8_0`), context length, and tensor count.
`GgufMetadataFn` reads only the header — no need to load tensor data.

**PickleToJsonFn** safely deserializes Python pickle format to JSON.
Uses a restricted unpickler that rejects `__reduce__`, `__setstate__`,
and other code-execution opcodes. Only data types are allowed:
dict, list, tuple, str, int, float, bool, None. `max_depth` prevents
stack overflow on deeply nested structures.

**NumpyToSafeTensorsFn** converts `.npy` files (NumPy's native format)
to SafeTensors. The `.npy` header contains dtype and shape; data
follows in row-major order. Supported dtypes: float16/32/64,
int8/32/64, uint8.

**ImageToTensorFn** converts image bytes (PNG/JPEG/WebP) to a raw
float tensor in CHW (Channel × Height × Width) layout — the standard
input format for vision models. When `normalize` is true, pixel values
are scaled from `[0, 255]` to `[0.0, 1.0]`.

#### 3.14.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| TFRecord CRC mismatch | `ExecutionFailed("CRC mismatch at record N")` |
| TFRecord truncated record | Streaming: buffer until complete; batch: error |
| SafeTensors header > 100 MB | `ExecutionFailed("header too large")` |
| SafeTensors tensor name not found | `ExecutionFailed("tensor 'X' not found")` |
| SafeTensors overlapping offsets | `ExecutionFailed("invalid tensor offsets")` |
| ONNX invalid protobuf | `ExecutionFailed("invalid ONNX model")` |
| ONNX shape inference failure | `OnnxValidateFn` returns error with node name |
| GGUF unsupported version | `ExecutionFailed("unsupported GGUF version N")` |
| Pickle with code execution opcodes | `ExecutionFailed("unsafe pickle opcode: REDUCE")` |
| NumPy fortran-order array | Transposed to C-order before conversion |
| NumPy unsupported dtype (complex, object) | `ExecutionFailed("unsupported dtype: complex128")` |
| ImageToTensor with CMYK image | Converted to RGB first |
| ImageToTensor with alpha channel, channels=3 | Alpha dropped |
| Empty TFRecord file | Empty output |

#### 3.14.4 Test Strategy

- TFRecord read/write roundtrip: features preserved through encode/decode
- TFRecord CRC: valid records pass; corrupted CRC detected
- TFRecord streaming parity: same output as batch
- SafeTensors read/write roundtrip: tensor data and metadata preserved
- SafeTensors metadata: verify dtype, shape, offsets for known file
- SafeTensors single tensor extraction by name
- ONNX metadata: verify opset, inputs, outputs for known model
- ONNX validate: valid model passes; broken shape fails
- GGUF metadata: verify architecture and quant type for known model
- Pickle safe deserialization: data-only pickles succeed; code-execution pickles rejected
- NumPy→SafeTensors: dtype and shape preserved
- TFRecord→Parquet: features become columns with correct types
- ImageToTensor: verify CHW layout, normalization, channel count

### 3.15 Category O: Network & Protocol Formats — PCAP, DNS, HAR, Email (8 functions, #433–#440)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `pcap-parser`, `mailparse`
> **Mode**: Mixed; PCAP and mbox streaming-capable

#### 3.15.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 433 | PcapReadFn | `pcap_read@1.0.0` | Both | `max_packets` (optional) | NDJSON (one packet per line: timestamp, src/dst IP, protocol, length, payload hex) |
| 434 | PcapFilterFn | `pcap_filter@1.0.0` | Both | `filter` (BPF expression, e.g. `"tcp port 80"`) | PCAP bytes (filtered subset) |
| 435 | PcapStatisticsFn | `pcap_statistics@1.0.0` | Batch | — | JSON (packet count, duration, bytes, protocol breakdown, top talkers) |
| 436 | DnsRecordParseFn | `dns_record_parse@1.0.0` | Both | — | NDJSON (one record per line: name, type, class, TTL, rdata) |
| 437 | HarParseFn | `har_parse@1.0.0` | Batch | `summary` (`"true"`/`"false"`, default `"false"`) | JSON (entries with timing, status, URL) or summary stats |
| 438 | EmailParseFn | `email_parse@1.0.0` | Batch | — | JSON (headers, body text, body html, attachment list) |
| 439 | EmailExtractAttachmentsFn | `email_extract_attachments@1.0.0` | Batch | `index` (0-based, optional — all if omitted) | Raw attachment bytes (single) or tar archive (multiple) |
| 440 | MboxSplitFn | `mbox_split@1.0.0` | Both | `max_messages` (optional) | NDJSON (one message per line: from, to, subject, date, size) |

#### 3.15.2 Design Notes

**PCAP** (Packet Capture) is the standard format from `tcpdump`/
Wireshark. File starts with a global header (magic, version, snaplen,
link type), followed by sequential packet records each with a
per-packet header (timestamp, captured length, original length) and
packet data. Sequential layout → streaming natural.

`PcapReadFn` decodes each packet's Ethernet/IP/TCP/UDP headers and
emits one NDJSON line per packet. The streaming variant processes
packets as they arrive.

`PcapFilterFn` compiles a BPF (Berkeley Packet Filter) expression
once, then applies it to each packet. Matching packets are written
to a new PCAP file preserving the global header. BPF compilation
uses `pcap-parser`'s filter support.

```rust
// PcapFilterFn — streaming
fn init(&mut self, params: &BTreeMap<String, Value>) -> Result<(), ComputeError> {
    let expr = get_string_param(params, "filter")?;
    self.bpf = compile_bpf(&expr)
        .map_err(|e| ComputeError::InvalidParam(format!("BPF: {}", e)))?;
    self.header_emitted = false;
    Ok(())
}
fn process_chunk(&mut self, chunk: Bytes) -> Result<Option<Bytes>, ComputeError> {
    self.buffer.extend_from_slice(&chunk);
    let mut out = Vec::new();
    if !self.header_emitted {
        if self.buffer.len() < 24 { return Ok(None); }
        let global_header = self.buffer[..24].to_vec();
        self.link_type = parse_link_type(&global_header)?;
        out.extend_from_slice(&global_header);
        self.buffer.advance(24);
        self.header_emitted = true;
    }
    while let Some((pkt_header, pkt_data, rest)) = try_read_pcap_packet(&self.buffer)? {
        if self.bpf.matches(&pkt_data, self.link_type) {
            out.extend_from_slice(&pkt_header);
            out.extend_from_slice(&pkt_data);
        }
        self.buffer = rest;
    }
    if out.is_empty() { return Ok(None); }
    Ok(Some(Bytes::from(out)))
}
```

**DNS zone files** are line-oriented text: each line is a resource
record (`name TTL class type rdata`). Comments start with `;`.
`$ORIGIN` and `$TTL` directives set defaults. Line-oriented →
streaming natural. `DnsRecordParseFn` handles multi-line records
(parenthesized continuations) and relative names.

**HAR** (HTTP Archive) is a JSON format recording browser network
activity. Contains `log.entries[]` with request/response details and
timing. Since it's a single JSON object, it requires full parsing →
batch. `summary` mode emits aggregate stats (total requests, total
bytes, average response time, status code distribution).

**Email** uses MIME (RFC 2045–2049) with nested multipart boundaries.
Full message required for boundary resolution → batch.
`EmailParseFn` extracts headers, text/html body parts, and lists
attachments with filename, content-type, and size.
`EmailExtractAttachmentsFn` returns raw bytes — single attachment
by index, or all attachments packed as a tar archive.

**Mbox** is a concatenation of email messages separated by `From `
lines (with space after "From"). Line-oriented separator → streaming.
`MboxSplitFn` emits metadata per message without parsing full MIME.

#### 3.15.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| PCAP with pcapng format | `ExecutionFailed("pcapng not supported; use pcap format")` |
| Invalid BPF expression | `InvalidParam("BPF: ...")` at init |
| PCAP truncated packet (caplen < original) | Decoded with available bytes; payload truncated |
| PCAP empty file (header only) | Empty NDJSON / empty PCAP output |
| DNS zone with $INCLUDE | `ExecutionFailed("$INCLUDE not supported")` |
| DNS multi-line record (parentheses) | Concatenated before parsing |
| HAR with missing timing fields | Timing fields set to `-1` in output |
| Email with nested multipart | Recursively parsed; all parts extracted |
| Email with base64 attachment | Decoded transparently |
| Email with charset other than UTF-8 | Converted to UTF-8 |
| Mbox with "From " in message body | Escaped `>From ` handled correctly |
| Mbox empty file | Empty output |

#### 3.15.4 Test Strategy

- PCAP read: verify packet count, IP addresses, protocol for known capture
- PCAP filter: BPF `"tcp port 80"` filters correctly; non-matching packets excluded
- PCAP statistics: verify protocol breakdown and byte counts
- PCAP streaming parity: same packet output as batch
- DNS parse: verify A, AAAA, MX, CNAME, TXT records from zone file
- DNS multi-line records: parenthesized SOA parsed correctly
- HAR parse: verify entry count, URLs, status codes
- HAR summary: verify aggregate statistics
- Email parse: verify headers, body, attachment list
- Email extract: single attachment by index; all as tar
- Mbox split: verify message count and metadata extraction
- Mbox streaming parity: same message boundaries as batch

### 3.16 Category P: Bioinformatics Formats — FASTA, FASTQ, SAM/BAM, VCF, BED (12 functions, #441–#452)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `noodles`, `bio`
> **Mode**: Predominantly streaming (line-oriented); BAM batch (binary indexed)

#### 3.16.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 441 | FastaParseFn | `fasta_parse@1.0.0` | Both | — | NDJSON (`{"id":"...","description":"...","sequence":"...","length":N}`) |
| 442 | FastqParseFn | `fastq_parse@1.0.0` | Both | — | NDJSON (`{"id":"...","sequence":"...","quality":"...","length":N}`) |
| 443 | FastqTrimQualityFn | `fastq_trim_quality@1.0.0` | Both | `min_quality` (Phred score, default `"20"`), `window` (default `"5"`) | FASTQ bytes (trimmed) |
| 444 | FastqFilterFn | `fastq_filter@1.0.0` | Both | `min_length` (optional), `max_n_ratio` (optional, default `"0.1"`), `min_avg_quality` (optional) | FASTQ bytes (filtered subset) |
| 445 | SamParseFn | `sam_parse@1.0.0` | Both | `header` (`"true"`/`"false"`, default `"false"`) | NDJSON (one alignment per line: qname, flag, rname, pos, mapq, cigar, seq, qual) |
| 446 | BamReadFn | `bam_read@1.0.0` | Batch | `region` (optional, e.g. `"chr1:1000-2000"`) | NDJSON (same schema as SamParse) |
| 447 | BamToSamFn | `bam_to_sam@1.0.0` | Both | — | SAM text bytes |
| 448 | VcfParseFn | `vcf_parse@1.0.0` | Both | `header` (`"true"`/`"false"`, default `"false"`) | NDJSON (`{"chrom":"...","pos":N,"id":"...","ref":"...","alt":["..."],"qual":N,"filter":"...","info":{...}}`) |
| 449 | VcfFilterFn | `vcf_filter@1.0.0` | Both | `chrom` (optional), `min_qual` (optional), `filter_pass` (`"true"`/`"false"`, default `"false"`) | VCF text bytes (filtered subset, header preserved) |
| 450 | BedParseFn | `bed_parse@1.0.0` | Both | — | NDJSON (`{"chrom":"...","start":N,"end":N,"name":"...","score":N,"strand":"..."}`) |
| 451 | GffParseFn | `gff_parse@1.0.0` | Both | `version` (`"2"`/`"3"`, default `"3"`) | NDJSON (`{"seqid":"...","source":"...","type":"...","start":N,"end":N,"score":N,"strand":"...","phase":"...","attributes":{...}}`) |
| 452 | FastaToFastqFn | `fasta_to_fastq@1.0.0` | Both | `default_quality` (Phred char, default `"I"` = Q40) | FASTQ bytes (synthetic quality scores) |

#### 3.16.2 Design Notes

All bioinformatics text formats are line-oriented or record-oriented
with simple delimiters → streaming natural.

**FASTA** uses `>` as record separator. Each record has a header line
(`>id description`) followed by one or more sequence lines. The
streaming variant buffers until the next `>` to emit a complete record.

**FASTQ** uses a 4-line record: `@id`, sequence, `+`, quality.
Quality scores are Phred+33 encoded ASCII characters. Streaming
processes 4 lines at a time.

`FastqTrimQualityFn` implements sliding-window quality trimming
(Trimmomatic SLIDINGWINDOW algorithm): scan from 3' end, compute
average quality in a window of size `window`, trim when average
drops below `min_quality`. This is the standard preprocessing step
for NGS data.

`FastqFilterFn` applies multiple criteria per read:
- `min_length`: discard reads shorter than threshold
- `max_n_ratio`: discard reads with too many ambiguous bases (N)
- `min_avg_quality`: discard reads with low average Phred score

```rust
// FastqTrimQualityFn — streaming: process 4 lines at a time
fn process_chunk(&mut self, chunk: Bytes) -> Result<Option<Bytes>, ComputeError> {
    self.buffer.extend_from_slice(&chunk);
    let mut out = Vec::new();
    while let Some((record, rest)) = try_read_fastq_record(&self.buffer)? {
        let trimmed_len = sliding_window_trim(
            &record.quality, self.min_quality, self.window,
        );
        if trimmed_len > 0 {
            writeln!(out, "@{}", record.id).unwrap();
            out.extend_from_slice(&record.sequence[..trimmed_len]);
            out.push(b'\n');
            out.extend_from_slice(b"+\n");
            out.extend_from_slice(&record.quality[..trimmed_len]);
            out.push(b'\n');
        }
        self.buffer = rest;
    }
    if out.is_empty() { return Ok(None); }
    Ok(Some(Bytes::from(out)))
}
```

**SAM** (Sequence Alignment/Map) is tab-delimited text with an
optional header (`@HD`, `@SQ`, `@RG` lines). Each alignment line has
11 mandatory fields. Line-oriented → streaming.

**BAM** is the binary, BGZF-compressed version of SAM. BGZF blocks
are independent gzip blocks (max 64 KB uncompressed) enabling random
access via `.bai` index. Region queries require the index → batch.
`BamToSamFn` decompresses BGZF blocks sequentially → streaming
possible (without index).

**VCF** (Variant Call Format) is tab-delimited with a header section
(`##` meta-info, `#CHROM` column header). Each data line is one
variant. `VcfFilterFn` preserves the header and filters data lines
by chromosome, quality threshold, or PASS filter status.

**BED** (Browser Extensible Data) is tab-delimited with 3–12 columns.
No header. Simplest bioinformatics format → trivial streaming.

**GFF** (General Feature Format) version 2 (GTF) and version 3 (GFF3)
are tab-delimited with 9 columns. Attributes column differs between
versions: GFF2 uses `key "value";` pairs, GFF3 uses `key=value;`.

`FastaToFastqFn` converts FASTA to FASTQ by assigning a uniform
quality score to every base. This is useful when downstream tools
require FASTQ input but quality data is unavailable.

#### 3.16.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| FASTA multi-line sequence | Lines concatenated into single sequence |
| FASTA empty sequence (header only) | Emitted with `"length":0` |
| FASTQ quality length ≠ sequence length | `ExecutionFailed("quality/sequence length mismatch at record N")` |
| FASTQ trim removes entire read | Read omitted from output |
| SAM with unmapped read (flag 0x4) | Parsed normally; `rname` = `"*"` |
| BAM without .bai index + region query | `ExecutionFailed("region query requires BAM index")` |
| BAM truncated BGZF block | `ExecutionFailed("truncated BGZF block")` |
| VCF with multiple ALT alleles | `alt` field is JSON array |
| VCF missing QUAL (`.`) | `qual` = `null` in JSON |
| VCF filter with no matches | Header only in output |
| BED with fewer than 6 columns | Optional fields set to defaults (name=`.`, score=0, strand=`.`) |
| GFF comment lines (`#`) | Skipped |
| GFF3 URL-encoded attributes | Decoded (`%20` → space) |
| Empty input for any function | Empty output |

#### 3.16.4 Test Strategy

- FASTA parse: single-line and multi-line sequences; verify id, description, length
- FASTQ parse: verify all 4 fields; quality string preserved
- FASTQ trim: known quality profile → verify trim position matches expected
- FASTQ filter: verify min_length, max_n_ratio, min_avg_quality each independently
- FASTQ trim + filter pipeline: combined preprocessing
- SAM parse: verify mandatory fields from known alignment
- SAM header inclusion/exclusion toggle
- BAM read: verify same alignments as equivalent SAM
- BAM region query: verify only overlapping alignments returned
- BAM→SAM: output matches original SAM (excluding sort order)
- VCF parse: verify chrom, pos, ref, alt, qual, info fields
- VCF filter by chromosome, quality, PASS status
- BED parse: 3-column, 6-column, 12-column BED files
- GFF2 vs GFF3 attribute parsing differences
- FASTA→FASTQ: verify quality string length matches sequence
- Streaming parity for all Both-mode functions

### 3.17 Category Q: Font, 3D & Specialized Binary — WOFF, glTF, STL, DICOM, WASM, ELF (13 functions, #453–#465)

> **Phase**: 4 — Domain-specific
> **Key dependencies**: `woff2`, `gltf`, `dicom-object`, `wasmparser`
> **Mode**: All batch (complex binary structures)

#### 3.17.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 453 | FontMetadataFn | `font_metadata@1.0.0` | Batch | — | JSON (family, style, weight, version, glyph count, tables, supported scripts) |
| 454 | WoffToOtfFn | `woff_to_otf@1.0.0` | Batch | — | OTF/TTF bytes (decompressed) |
| 455 | OtfToWoff2Fn | `otf_to_woff2@1.0.0` | Batch | — | WOFF2 bytes (Brotli-compressed) |
| 456 | GltfMetadataFn | `gltf_metadata@1.0.0` | Batch | — | JSON (scene count, node count, mesh count, material count, texture count, animation count, total vertices) |
| 457 | GltfValidateFn | `gltf_validate@1.0.0` | Batch | — | Pass-through or error (spec conformance) |
| 458 | StlReadFn | `stl_read@1.0.0` | Batch | — | JSON (`{"format":"ascii"|"binary","triangle_count":N,"bounds":{"min":[x,y,z],"max":[x,y,z]}}`) |
| 459 | StlConvertFn | `stl_convert@1.0.0` | Batch | `target` (`"ascii"`/`"binary"`) | STL bytes in target format |
| 460 | DicomMetadataFn | `dicom_metadata@1.0.0` | Batch | `tags` (optional, comma-separated tag names to extract) | JSON (patient name, study date, modality, dimensions, pixel spacing, transfer syntax) |
| 461 | DicomToImageFn | `dicom_to_image@1.0.0` | Batch | `format` (`"png"`/`"jpeg"`, default `"png"`), `window_center` (optional), `window_width` (optional) | Image bytes |
| 462 | DicomAnonymizeFn | `dicom_anonymize@1.0.0` | Batch | `keep_tags` (optional, comma-separated tags to preserve) | DICOM bytes (PHI removed) |
| 463 | WasmValidateFn | `wasm_validate@1.0.0` | Batch | — | Pass-through or error (module structure, type consistency) |
| 464 | WasmMetadataFn | `wasm_metadata@1.0.0` | Batch | — | JSON (version, imports, exports, memory limits, table count, function count, custom sections) |
| 465 | ElfMetadataFn | `elf_metadata@1.0.0` | Batch | — | JSON (class, endianness, OS/ABI, type, machine, entry point, sections, symbols count) |

#### 3.17.2 Design Notes

All formats in this category have complex binary structures requiring
random access or full-file parsing → batch-only.

**Font formats**: WOFF (Web Open Font Format) wraps OTF/TTF tables
with per-table compression (zlib for WOFF1, Brotli for WOFF2).
`FontMetadataFn` reads the `name` table for family/style, `head`
for version, `maxp` for glyph count, and `OS/2` for weight class.
`WoffToOtfFn` decompresses each table and reconstructs the OTF
table directory. `OtfToWoff2Fn` applies Brotli compression with
WOFF2-specific font preprocessing transforms.

**glTF** (GL Transmission Format) is the "JPEG of 3D". Two variants:
`.gltf` (JSON + external `.bin`) and `.glb` (binary container).
`GltfMetadataFn` parses the JSON descriptor to extract scene graph
statistics. `GltfValidateFn` checks spec conformance: accessor
bounds, buffer view alignment, material PBR constraints, animation
sampler consistency.

**STL** (Stereolithography) has two variants: ASCII (`solid`/
`endsolid` delimiters with `facet normal`/`vertex` text) and binary
(80-byte header + triangle count + 50 bytes per triangle).
`StlReadFn` auto-detects format and computes bounding box.
`StlConvertFn` converts between ASCII and binary representations.

**DICOM** (Digital Imaging and Communications in Medicine) is the
standard for medical imaging. Files contain a preamble, `DICM` magic,
and a sequence of tag-length-value elements. Tags are identified by
(group, element) pairs.

`DicomAnonymizeFn` is HIPAA-critical. It removes all 18 HIPAA
identifiers from the DICOM dataset:

```rust
// HIPAA Safe Harbor: 18 identifier categories mapped to DICOM tags
const PHI_TAGS: &[(u16, u16)] = &[
    (0x0010, 0x0010), // PatientName
    (0x0010, 0x0020), // PatientID
    (0x0010, 0x0030), // PatientBirthDate
    (0x0010, 0x0040), // PatientSex
    (0x0010, 0x1000), // OtherPatientIDs
    (0x0010, 0x1001), // OtherPatientNames
    (0x0010, 0x1010), // PatientAge
    (0x0010, 0x1020), // PatientSize
    (0x0010, 0x1030), // PatientWeight
    (0x0010, 0x1040), // PatientAddress
    (0x0010, 0x2154), // PatientTelephoneNumbers
    (0x0008, 0x0080), // InstitutionName
    (0x0008, 0x0081), // InstitutionAddress
    (0x0008, 0x0090), // ReferringPhysicianName
    (0x0008, 0x1048), // PhysiciansOfRecord
    (0x0008, 0x1050), // PerformingPhysicianName
    (0x0008, 0x1070), // OperatorsName
    (0x0020, 0x4000), // ImageComments (may contain PHI)
];
```

Tags in `keep_tags` are preserved even if they appear in the PHI list
(explicit opt-in). All other PHI tags are replaced with empty values
or removed. UIDs are re-mapped to maintain referential integrity
without leaking the original identifiers.

`DicomToImageFn` extracts pixel data and applies windowing (contrast
adjustment). If `window_center`/`window_width` are not provided, the
values from the DICOM header are used. Supports 8-bit, 12-bit, and
16-bit pixel data with various photometric interpretations
(MONOCHROME1, MONOCHROME2, RGB).

**WASM** (WebAssembly) modules have a well-defined binary format:
magic (`\0asm`), version, then typed sections (type, import, function,
table, memory, global, export, start, element, code, data, custom).
`wasmparser` provides streaming validation but we use batch mode for
simplicity since modules are typically small. `WasmValidateFn` checks
structural validity and type consistency. `WasmMetadataFn` extracts
the module interface without executing code.

**ELF** (Executable and Linkable Format) is the standard binary
format on Linux/Unix. `ElfMetadataFn` reads the ELF header (class
32/64, endianness, OS/ABI, object type, machine architecture),
section headers (count, names), and symbol table summary. No code
execution — metadata extraction only.

#### 3.17.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| WOFF1 vs WOFF2 auto-detection | Magic bytes: `wOFF` (WOFF1) vs `wOF2` (WOFF2) |
| WOFF with unknown table tags | Preserved in OTF output |
| OTF with CFF outlines (not TrueType) | Supported; CFF table preserved |
| glTF with external buffers (`.bin`) | `ExecutionFailed("external buffers not supported; use .glb")` |
| glTF with Draco-compressed meshes | `ExecutionFailed("Draco compression not supported")` |
| STL ASCII with degenerate triangles | Parsed; zero-area triangles included in count |
| STL binary with incorrect triangle count | `ExecutionFailed("triangle count mismatch")` |
| DICOM without pixel data | `DicomMetadataFn` succeeds; `DicomToImageFn` errors |
| DICOM with compressed pixel data (JPEG2000) | Decompressed transparently |
| DICOM anonymize with `keep_tags` including PHI | Kept (explicit opt-in) |
| WASM with unknown custom sections | Listed in metadata; validation passes |
| WASM with bulk-memory proposal | Validated if `wasmparser` supports it |
| ELF stripped binary (no symbols) | `symbols_count: 0` in metadata |
| ELF with multiple symbol tables | All tables counted |
| Empty input for any function | `ExecutionFailed("empty input")` |

#### 3.17.4 Test Strategy

- Font metadata: verify family, weight, glyph count for known font
- WOFF→OTF→WOFF2 roundtrip: font renders identically (metadata preserved)
- WOFF1 and WOFF2 decompression: table checksums match original OTF
- glTF metadata: verify counts for known model
- glTF validate: valid model passes; broken accessor fails
- STL read: verify triangle count and bounding box for known model
- STL ASCII↔binary roundtrip: triangle data preserved
- DICOM metadata: verify patient-safe fields for known file
- DICOM→image: verify dimensions and pixel value range
- DICOM anonymize: verify all 18 PHI tag categories removed
- DICOM anonymize with keep_tags: specified tags preserved
- WASM validate: valid module passes; type mismatch fails
- WASM metadata: verify imports, exports, memory for known module
- ELF metadata: verify class, machine, section count for known binary

### 3.18 Category R: Erasure Coding & Redundancy (9 functions, #466–#474)

> **Phase**: 2 — Analytics backbone (DFS-critical)
> **Key dependencies**: `reed-solomon-erasure`
> **Mode**: All streaming-capable (chunk-aligned block operations)

#### 3.18.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 466 | ReedSolomonEncodeFn | `rs_encode@1.0.0` | Both | `data_shards` (default `"4"`), `parity_shards` (default `"2"`) | Concatenated shard bytes (`data_shards + parity_shards` equal-length blocks) |
| 467 | ReedSolomonDecodeFn | `rs_decode@1.0.0` | Both | `data_shards`, `parity_shards`, `missing` (comma-separated 0-based shard indices) | Original data bytes (reconstructed) |
| 468 | ReedSolomonVerifyFn | `rs_verify@1.0.0` | Both | `data_shards`, `parity_shards` | Pass-through or error (parity consistency check) |
| 469 | XorParityFn | `xor_parity@1.0.0` | Both | `shard_count` (default `"3"`) | Concatenated bytes: original shards + 1 XOR parity shard |
| 470 | XorReconstructFn | `xor_reconstruct@1.0.0` | Both | `shard_count`, `missing` (single shard index) | Original data bytes (reconstructed) |
| 471 | ReplicationSplitFn | `replication_split@1.0.0` | Both | `replicas` (default `"3"`) | Concatenated identical copies |
| 472 | ReplicationVerifyFn | `replication_verify@1.0.0` | Batch | `replicas` | Pass-through or error (all replicas identical) |
| 473 | StripeSplitFn | `stripe_split@1.0.0` | Both | `stripe_count` (default `"4"`), `stripe_size` (bytes, default `"65536"`) | Concatenated stripe shards (round-robin distribution) |
| 474 | StripeAssembleFn | `stripe_assemble@1.0.0` | Both | `stripe_count`, `stripe_size` | Original data bytes (interleaved reassembly) |

#### 3.18.2 Design Notes

Erasure coding is fundamental to distributed file systems. HDFS-3
uses Reed-Solomon (6+3 default), Ceph uses Reed-Solomon or LRC,
MinIO uses Reed-Solomon. This category provides the building blocks
for the DFS storage layer.

**Data layout convention**: All multi-shard functions use a simple
concatenation layout. Input/output is `shard_count` equal-length
blocks concatenated. The shard size is `ceil(input_len / data_shards)`
for RS, `ceil(input_len / shard_count)` for XOR/stripe. The last
data shard is zero-padded to equal length. A 4-byte little-endian
original length prefix is prepended before encoding so decode can
strip padding.

**Reed-Solomon** uses Galois Field GF(2^8) arithmetic. The
`reed-solomon-erasure` crate provides the core encode/decode.
Streaming operates on chunk-aligned blocks — each chunk must be a
complete set of shards. The streaming variant processes one
"stripe" (data_shards + parity_shards blocks) per chunk.

```rust
// ReedSolomonEncodeFn
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let data_shards = get_usize_param(params, "data_shards", 4)?;
    let parity_shards = get_usize_param(params, "parity_shards", 2)?;
    let r = ReedSolomon::new(data_shards, parity_shards)
        .map_err(|e| ComputeError::ExecutionFailed(format!("rs init: {}", e)))?;
    let input = &inputs[0];
    let original_len = input.len();
    let shard_size = (original_len + data_shards - 1) / data_shards;
    // Prepend original length, then pad
    let mut padded = Vec::with_capacity(4 + shard_size * data_shards);
    padded.extend_from_slice(&(original_len as u32).to_le_bytes());
    padded.extend_from_slice(input);
    padded.resize(4 + shard_size * data_shards, 0u8);
    // Split into shards
    let mut shards: Vec<Vec<u8>> = padded.chunks(shard_size)
        .map(|c| c.to_vec())
        .collect();
    // Add empty parity shards
    for _ in 0..parity_shards {
        shards.push(vec![0u8; shard_size]);
    }
    r.encode(&mut shards)
        .map_err(|e| ComputeError::ExecutionFailed(format!("rs encode: {}", e)))?;
    Ok(Bytes::from(shards.concat()))
}
```

```rust
// ReedSolomonDecodeFn
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let data_shards = get_usize_param(params, "data_shards", 4)?;
    let parity_shards = get_usize_param(params, "parity_shards", 2)?;
    let missing: Vec<usize> = get_string_param(params, "missing")?
        .split(',').map(|s| s.trim().parse::<usize>()
            .map_err(|_| ComputeError::InvalidParam("missing: comma-separated indices".into())))
        .collect::<Result<_, _>>()?;
    let total = data_shards + parity_shards;
    let shard_size = inputs[0].len() / total;
    let r = ReedSolomon::new(data_shards, parity_shards)
        .map_err(|e| ComputeError::ExecutionFailed(format!("rs init: {}", e)))?;
    let mut shards: Vec<Option<Vec<u8>>> = inputs[0]
        .chunks(shard_size)
        .enumerate()
        .map(|(i, c)| if missing.contains(&i) { None } else { Some(c.to_vec()) })
        .collect();
    r.reconstruct(&mut shards)
        .map_err(|e| ComputeError::ExecutionFailed(format!("rs decode: {}", e)))?;
    let data: Vec<u8> = shards[..data_shards].iter()
        .flat_map(|s| s.as_ref().unwrap().iter().copied())
        .collect();
    let original_len = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
    Ok(Bytes::from(data[4..4 + original_len].to_vec()))
}
```

**XOR parity** is the simplest erasure code (RAID-5 style). Given
`shard_count` data shards, the parity shard is the XOR of all data
shards. Can reconstruct any single missing shard. Streaming XORs
each chunk as it arrives, emitting the parity on flush.

**Replication** is trivial fan-out: input is duplicated `replicas`
times. `ReplicationVerifyFn` checks all replicas are byte-identical.
Batch-only for verify (needs all replicas simultaneously).

**Striping** distributes data across shards in round-robin fashion
at `stripe_size` granularity (default 64 KB). This is the RAID-0
pattern — no redundancy, but enables parallel I/O. Combined with
RS or XOR parity in a pipeline for RAID-5/6 equivalents.

Streaming for all encode/split functions: process one stripe-width
of data per chunk. Streaming for decode/assemble: process one
stripe-width of shards per chunk.

#### 3.18.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| RS data_shards=0 or parity_shards=0 | `InvalidParam("shards must be ≥ 1")` |
| RS missing more shards than parity_shards | `ExecutionFailed("too many missing shards: N > parity_shards")` |
| RS missing index out of range | `InvalidParam("shard index N out of range")` |
| RS input not divisible by data_shards | Zero-padded; original length stored in prefix |
| RS empty input | `ExecutionFailed("empty input")` |
| XOR missing > 1 shard | `ExecutionFailed("XOR can reconstruct at most 1 shard")` |
| XOR shard_count=1 | Parity is copy of data (degenerate case) |
| Replication replicas=0 | `InvalidParam("replicas must be ≥ 1")` |
| Replication verify with mismatched replica | `ExecutionFailed("replica N differs at byte offset M")` |
| Stripe input smaller than stripe_size | Single stripe, last shard short |
| Stripe assemble with wrong stripe_count | `ExecutionFailed("input size not aligned to stripe_count × stripe_size")` |
| Streaming chunk not aligned to shard boundary | Buffered until complete stripe available |

#### 3.18.4 Test Strategy

- RS encode/decode roundtrip: original data recovered (4+2, 6+3, 10+4 configs)
- RS decode with 1, 2, max missing shards: all recover correctly
- RS decode with too many missing: error
- RS verify: valid encoding passes; corrupted shard fails
- RS streaming parity: same output as batch
- XOR parity/reconstruct roundtrip: each shard recoverable individually
- XOR with 2 missing: error
- Replication split: all replicas byte-identical
- Replication verify: identical passes; tampered fails
- Stripe split/assemble roundtrip: original data recovered
- Stripe with non-aligned input: padding handled correctly
- Pipeline test: stripe_split → rs_encode → (simulate loss) → rs_decode → stripe_assemble
- Pipeline test: xor_parity → (simulate loss) → xor_reconstruct

### 3.19 Category S: Universal Format Detection & Conversion (9 functions, #475–#483)

> **Phase**: 1 — High-value (meta-functions)
> **Key dependencies**: `infer`, `mime_guess`, `jsonschema`
> **Mode**: Mixed; FormatDetect streaming-capable (header only)

#### 3.19.1 Function List

| # | Function | FunctionId | Mode | Params | Output |
|---|----------|-----------|------|--------|--------|
| 475 | FormatDetectFn | `format_detect@1.0.0` | Both | — | JSON (`{"format":"parquet","mime":"application/vnd.apache.parquet","category":"columnar","confidence":0.99}`) |
| 476 | FormatValidateFn | `format_validate@1.0.0` | Batch | `format` (expected format name) | Pass-through or error (structural validation) |
| 477 | UniversalMetadataFn | `universal_metadata@1.0.0` | Batch | — | JSON (auto-detected format + format-specific metadata) |
| 478 | UniversalToJsonFn | `universal_to_json@1.0.0` | Batch | `max_records` (optional, default `"1000"`) | JSON (format-agnostic structured representation) |
| 479 | UniversalToTextFn | `universal_to_text@1.0.0` | Batch | — | UTF-8 text (plain-text extraction for indexing) |
| 480 | FormatConvertFn | `format_convert@1.0.0` | Batch | `from` (optional, auto-detected), `to` (target format name) | Converted bytes |
| 481 | SchemaInferFn | `schema_infer@1.0.0` | Batch | `sample_rows` (default `"1000"`) | JSON Schema (inferred from tabular data) |
| 482 | SchemaCompareFn | `schema_compare@1.0.0` | Batch | — | JSON (`{"compatible":bool,"changes":[{"field":"...","type":"added|removed|changed","detail":"..."}]}`) |
| 483 | MimeTypeMapFn | `mime_type_map@1.0.0` | Batch | `direction` (`"ext_to_mime"`/`"mime_to_ext"`, default `"ext_to_mime"`) | JSON mapping |

#### 3.19.2 Design Notes

Category S provides meta-functions that dispatch to category-specific
implementations. These are the primary entry points for users who
don't know (or don't want to specify) the input format.

**FormatDetectFn** is the cornerstone. It reads magic bytes and
structural hints to identify the format. Detection strategy:

1. **Magic bytes** (first 4–16 bytes): covers ~80% of binary formats
2. **Structural probe**: for text formats without magic bytes (CSV,
   JSON, XML, YAML), attempt parsing the first few KB
3. **Extension hint**: if input metadata includes a filename, use
   extension as tiebreaker

Streaming variant: only needs the first chunk (typically 64 KB) to
detect. Emits the detection result and passes all data through.

```rust
// FormatDetectFn — detection dispatch table (abbreviated)
const MAGIC_TABLE: &[(&[u8], &str, &str, &str)] = &[
    (b"PAR1",                "parquet",    "application/vnd.apache.parquet", "columnar"),
    (b"ORC",                 "orc",        "application/x-orc",             "columnar"),
    (b"ARROW1",              "arrow_ipc",  "application/vnd.apache.arrow.file", "columnar"),
    (b"Obj\x01",             "avro",       "application/avro",              "serialization"),
    (b"\x89PNG",             "png",        "image/png",                     "image"),
    (b"\xff\xd8\xff",        "jpeg",       "image/jpeg",                    "image"),
    (b"RIFF",                "wav",        "audio/wav",                     "audio"),  // + WAVE check
    (b"GIF8",                "gif",        "image/gif",                     "image"),
    (b"PK\x03\x04",         "zip",        "application/zip",               "archive"),
    (b"\x1f\x8b",           "gzip",       "application/gzip",              "archive"),
    (b"\xfd7zXZ\x00",       "xz",         "application/x-xz",             "archive"),
    (b"\x28\xb5\x2f\xfd",   "zstd",       "application/zstd",              "archive"),
    (b"BZh",                 "bzip2",      "application/x-bzip2",           "archive"),
    (b"%PDF",                "pdf",        "application/pdf",               "document"),
    (b"SQLite format 3\x00","sqlite",     "application/x-sqlite3",         "database"),
    (b"\x00asm",             "wasm",       "application/wasm",              "binary"),
    (b"\x7fELF",             "elf",        "application/x-elf",             "binary"),
    (b"DICM",                "dicom",      "application/dicom",             "binary"),  // at offset 128
    (b"\x93NUMPY",           "numpy",      "application/x-numpy",           "scientific"),
    (b"GGUF",                "gguf",       "application/x-gguf",            "ml"),
    (b"fLaC",                "flac",       "audio/flac",                    "audio"),
    (b"ID3",                 "mp3",        "audio/mpeg",                    "audio"),
];

fn detect(input: &[u8]) -> FormatResult {
    // 1. Magic bytes
    for (magic, format, mime, category) in MAGIC_TABLE {
        if input.starts_with(magic) {
            return FormatResult { format, mime, category, confidence: 0.99 };
        }
    }
    // 2. Structural probe for text formats
    let text = std::str::from_utf8(&input[..input.len().min(4096)]).ok();
    if let Some(t) = text {
        let trimmed = t.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return FormatResult { format: "json", mime: "application/json", category: "row", confidence: 0.90 };
        }
        if trimmed.starts_with("<?xml") || trimmed.starts_with("<") {
            return FormatResult { format: "xml", mime: "application/xml", category: "row", confidence: 0.85 };
        }
        if trimmed.starts_with("---") || trimmed.contains(":\n") {
            return FormatResult { format: "yaml", mime: "application/x-yaml", category: "config", confidence: 0.70 };
        }
        if trimmed.contains(',') && trimmed.contains('\n') {
            return FormatResult { format: "csv", mime: "text/csv", category: "row", confidence: 0.60 };
        }
    }
    FormatResult { format: "unknown", mime: "application/octet-stream", category: "unknown", confidence: 0.0 }
}
```

**FormatValidateFn** takes an expected format name and dispatches to
the corresponding category's validation logic. For example,
`format="parquet"` delegates to Parquet footer parsing,
`format="json"` delegates to JSON syntax validation. This is a
convenience wrapper that avoids requiring users to know which
category-specific function to call.

**UniversalMetadataFn** chains `FormatDetect` → category-specific
metadata function. Auto-detects format, then dispatches:
- Parquet → `parquet_metadata`
- PNG → `png_metadata`
- SQLite → `sqlite_table_list`
- etc.

Returns a unified JSON envelope:
`{"format":"parquet","metadata":{...format-specific...}}`.

**UniversalToJsonFn** converts any supported format to a JSON
representation. This is the "hub format" for cross-format conversion.
Tabular formats become `[{row}, ...]`. Documents become
`{"text":"...","metadata":{...}}`. Binary formats become
`{"format":"...","summary":{...}}`. `max_records` limits output
for large datasets.

**UniversalToTextFn** extracts plain text from any format — the
entry point for full-text search indexing in the DFS. Dispatches to:
- CSV/JSON/XML → field values concatenated
- PDF/DOCX/HTML → text extraction
- Images → empty (or OCR placeholder)
- Binary → hex dump summary

**FormatConvertFn** converts between formats using JSON as the
intermediate hub. Conversion path: `source → JSON → target`.
Supported conversions are limited to formats within the same
"convertibility class":
- Tabular: CSV ↔ JSON ↔ Parquet ↔ Avro ↔ ORC
- Config: YAML ↔ TOML ↔ JSON
- Image: PNG ↔ JPEG ↔ WebP
- Document: HTML ↔ Markdown ↔ text

Unsupported cross-class conversions (e.g., PNG → Parquet) return
`InvalidParam("no conversion path from png to parquet")`.

**SchemaInferFn** analyzes tabular data (CSV, JSON, Parquet, Avro)
and produces a JSON Schema describing the structure. For CSV, it
samples `sample_rows` rows to infer column types (integer, number,
string, boolean, null). For Parquet/Avro, the schema is extracted
directly from metadata.

**SchemaCompareFn** takes two JSON Schemas (concatenated, newline-
separated) and produces a compatibility report: added fields, removed
fields, type changes. This supports schema evolution workflows —
checking whether a new data version is backward-compatible.

**MimeTypeMapFn** is a pure lookup function. Given a file extension,
returns the MIME type (or vice versa). Uses the `mime_guess` crate's
database. Input is the extension or MIME string as UTF-8 text.

#### 3.19.3 Edge Cases

| Edge Case | Behavior |
|-----------|----------|
| FormatDetect on empty input | `{"format":"unknown","confidence":0.0}` |
| FormatDetect ambiguous (e.g., XML that looks like HTML) | Higher-confidence match wins; HTML checked before generic XML |
| FormatDetect DICOM (magic at offset 128) | Special-cased: checks bytes 128–132 for `DICM` |
| FormatValidate with unknown format name | `InvalidParam("unknown format: ...")` |
| UniversalMetadata on unknown format | `ExecutionFailed("cannot extract metadata: unknown format")` |
| UniversalToJson on binary format (ELF, WASM) | Returns summary JSON, not full content |
| UniversalToText on image/audio | Returns empty string (no text content) |
| FormatConvert unsupported path | `InvalidParam("no conversion path from X to Y")` |
| FormatConvert with data loss (e.g., Parquet types → CSV) | Proceeds with best-effort; types flattened to strings |
| SchemaInfer on non-tabular format | `InvalidParam("schema inference requires tabular format")` |
| SchemaInfer with all-null column | Inferred as `"type":"string"` (most permissive) |
| SchemaCompare with identical schemas | `{"compatible":true,"changes":[]}` |
| MimeTypeMap unknown extension | `{"mime":"application/octet-stream"}` |
| MimeTypeMap unknown MIME type | `{"extensions":[]}` |

#### 3.19.4 Test Strategy

- FormatDetect: test against every supported binary format (magic bytes)
- FormatDetect: test text format detection (CSV, JSON, XML, YAML)
- FormatDetect: ambiguous inputs (JSON that starts with `[{`, XML vs HTML)
- FormatDetect streaming: first chunk sufficient for detection
- FormatValidate: valid file passes; corrupted file fails (per format)
- UniversalMetadata: verify dispatch to correct category function
- UniversalToJson: CSV, Parquet, JSON, PDF all produce valid JSON
- UniversalToText: PDF, DOCX, HTML produce text; binary returns empty
- FormatConvert: CSV→Parquet→CSV roundtrip preserves data
- FormatConvert: YAML→TOML→JSON roundtrip preserves structure
- FormatConvert: unsupported path returns error
- SchemaInfer: CSV with mixed types infers correct JSON Schema
- SchemaInfer: Parquet schema extracted directly (no sampling)
- SchemaCompare: added/removed/changed fields detected
- SchemaCompare: identical schemas report compatible
- MimeTypeMap: known extensions and MIME types resolve correctly

---

## 4. Implementation Strategy

### 4.1 Phase 1 — High-Value, Low-Complexity (98 functions)

**Categories**: B (25), D (20), H (16), K (13), L (15), S (9)
**Estimated effort**: 5–7 days
**Dependencies**: lightweight (`csv`, `quick-xml`, `tar`, `flate2`, `serde_yaml`, `toml`, `cid`, `infer`)

Implementation order within Phase 1:
1. **S (Universal detection)** — needed by all other categories for dispatch
2. **B (CSV/JSON/XML)** — most common formats, foundation for hub conversions
3. **H (Config — YAML/TOML/INI)** — small, self-contained, high daily use
4. **D (Archive — tar/gzip/zip)** — streaming pipeline composition
5. **K (Log/Observability)** — line-oriented parsers, reuse CSV infrastructure
6. **L (CID/CAR/UnixFS)** — CAS-native, directly used by storage layer

Each category is implemented as a single source file, registered behind
its feature flag, and tested before moving to the next.

### 4.2 Phase 2 — Analytics Backbone (46 functions)

**Categories**: A (18), C (19), R (9)
**Estimated effort**: 7–10 days
**Dependencies**: heavy (`parquet`, `arrow`, `orc-rust`, `apache-avro`, `prost`, `reed-solomon-erasure`)

Implementation order:
1. **R (Erasure coding)** — smallest, DFS-critical, no format parsing
2. **C (Serialization — Avro/Protobuf)** — needed for Parquet metadata
3. **A (Columnar — Parquet/ORC/Arrow)** — largest, most complex; Arrow IPC as interchange

### 4.3 Phase 3 — Rich Media & Documents (57 functions)

**Categories**: E (18), F (16), G (23)
**Estimated effort**: 5–8 days
**Dependencies**: media processing (`image`, `symphonia`, `lopdf`, `calamine`)

Implementation order:
1. **E (Image)** — `image` crate covers PNG/JPEG/WebP/GIF/TIFF
2. **G (Document)** — PDF/XLSX/HTML text extraction for search indexing
3. **F (Audio/Video)** — metadata extraction only (no transcoding)

### 4.4 Phase 4 — Domain-Specific (82 functions)

**Categories**: I (14), J (13), M (10), N (12), O (8), P (12), Q (13)
**Estimated effort**: 5–8 days
**Dependencies**: many optional (`noodles`, `hdf5`, `rusqlite`, `safetensors`, `wasmparser`, `pcap-parser`, `dicom-object`)

All Phase 4 categories are independent — can be implemented in any
order or in parallel. Each is behind its own feature flag and has
zero coupling to other Phase 4 categories.

### 4.5 Feature Flag Strategy

```toml
# Cargo.toml — feature flags
[features]
default = ["format-phase1"]

# Phase bundles
format-phase1 = ["format-csv", "format-archive", "format-config", "format-log", "format-cas", "format-detect"]
format-phase2 = ["format-columnar", "format-serialization", "format-erasure"]
format-phase3 = ["format-image", "format-audio", "format-document"]
format-phase4 = ["format-geo", "format-scientific", "format-database", "format-ml", "format-network", "format-bio", "format-binary"]
format-all = ["format-phase1", "format-phase2", "format-phase3", "format-phase4"]

# Per-category flags
format-csv = ["dep:csv", "dep:quick-xml", "dep:serde_json"]
format-archive = ["dep:tar", "dep:zip", "dep:flate2", "dep:bzip2", "dep:xz2", "dep:zstd"]
format-config = ["dep:serde_yaml", "dep:toml", "dep:configparser", "dep:hcl-rs"]
format-log = []  # no extra deps — regex + serde_json
format-cas = ["dep:cid", "dep:multihash", "dep:libipld", "dep:iroh-car"]
format-detect = ["dep:infer", "dep:mime_guess"]
format-columnar = ["dep:parquet", "dep:arrow", "dep:orc-rust"]
format-serialization = ["dep:apache-avro", "dep:prost", "dep:rmp-serde", "dep:ciborium", "dep:bson"]
format-erasure = ["dep:reed-solomon-erasure"]
format-image = ["dep:image", "dep:resvg", "dep:img-hash"]
format-audio = ["dep:symphonia"]
format-document = ["dep:lopdf", "dep:calamine", "dep:scraper", "dep:pulldown-cmark"]
format-geo = ["dep:geojson", "dep:flatgeobuf"]
format-scientific = ["dep:hdf5"]
format-database = ["dep:rusqlite"]
format-ml = ["dep:safetensors"]
format-network = ["dep:pcap-parser", "dep:mailparse"]
format-bio = ["dep:noodles"]
format-binary = ["dep:wasmparser", "dep:dicom-object", "dep:gltf"]
```

### 4.6 Registration Pattern

```rust
// crates/deriva-compute/src/builtins_format.rs
pub fn register_format_functions(registry: &mut FunctionRegistry) {
    #[cfg(feature = "format-csv")]
    builtins_format_csv::register_category_b(registry);
    #[cfg(feature = "format-archive")]
    builtins_format_archive::register_category_d(registry);
    #[cfg(feature = "format-config")]
    builtins_format_config::register_category_h(registry);
    #[cfg(feature = "format-log")]
    builtins_format_log::register_category_k(registry);
    #[cfg(feature = "format-cas")]
    builtins_format_cas::register_category_l(registry);
    #[cfg(feature = "format-detect")]
    builtins_format_detect::register_category_s(registry);
    #[cfg(feature = "format-columnar")]
    builtins_format_columnar::register_category_a(registry);
    #[cfg(feature = "format-serialization")]
    builtins_format_serialization::register_category_c(registry);
    #[cfg(feature = "format-erasure")]
    builtins_format_erasure::register_category_r(registry);
    #[cfg(feature = "format-image")]
    builtins_format_image::register_category_e(registry);
    #[cfg(feature = "format-audio")]
    builtins_format_audio::register_category_f(registry);
    #[cfg(feature = "format-document")]
    builtins_format_document::register_category_g(registry);
    #[cfg(feature = "format-geo")]
    builtins_format_geo::register_category_i(registry);
    #[cfg(feature = "format-scientific")]
    builtins_format_scientific::register_category_j(registry);
    #[cfg(feature = "format-database")]
    builtins_format_database::register_category_m(registry);
    #[cfg(feature = "format-ml")]
    builtins_format_ml::register_category_n(registry);
    #[cfg(feature = "format-network")]
    builtins_format_network::register_category_o(registry);
    #[cfg(feature = "format-bio")]
    builtins_format_bio::register_category_p(registry);
    #[cfg(feature = "format-binary")]
    builtins_format_binary::register_category_q(registry);
}
```

Each `register_category_X` function follows the same pattern:

```rust
// builtins_format_csv.rs (example)
pub fn register_category_b(registry: &mut FunctionRegistry) {
    registry.register(Arc::new(CsvParseFn));       // #219
    registry.register(Arc::new(CsvWriteFn));        // #220
    // ... remaining 23 functions
}
```

---

## 5. Test Specification

### 5.1 Per-Category Test Suites

Each category gets its own integration test file under
`crates/deriva-compute/tests/`, gated behind the corresponding
feature flag (`#[cfg(feature = "format-csv")]`).

| Category | Test File | Functions | Tests (~3/fn) |
|----------|-----------|-----------|---------------|
| A Columnar | `format_columnar.rs` | 18 | 54 |
| B Row-oriented | `format_csv.rs` | 25 | 75 |
| C Serialization | `format_serialization.rs` | 19 | 57 |
| D Archive | `format_archive.rs` | 20 | 60 |
| E Image | `format_image.rs` | 18 | 54 |
| F Audio/Video | `format_audio.rs` | 16 | 48 |
| G Document | `format_document.rs` | 23 | 69 |
| H Config | `format_config.rs` | 16 | 48 |
| I Geospatial | `format_geo.rs` | 14 | 42 |
| J Scientific | `format_scientific.rs` | 13 | 39 |
| K Log | `format_log.rs` | 13 | 39 |
| L CAS | `format_cas.rs` | 15 | 45 |
| M Database | `format_database.rs` | 10 | 30 |
| N ML/Tensor | `format_ml.rs` | 12 | 36 |
| O Network | `format_network.rs` | 8 | 24 |
| P Bioinformatics | `format_bio.rs` | 12 | 36 |
| Q Specialized | `format_binary.rs` | 13 | 39 |
| R Erasure | `format_erasure.rs` | 9 | 27 |
| S Universal | `format_detect.rs` | 9 | 27 |
| **Total** | **19 files** | **283** | **~849** |

### 5.2 Cross-Category Conversion Tests

File: `tests/format_cross_conversion.rs`

| Test | Path | Assertion |
|------|------|-----------|
| CSV → Parquet → CSV | tabular roundtrip | Data preserved (types may widen) |
| CSV → JSON → CSV | text roundtrip | Field values identical |
| Parquet → Avro → Parquet | columnar↔serialization | Schema and data preserved |
| YAML → JSON → TOML → YAML | config roundtrip | Structure preserved |
| Arrow IPC → NumPy → SafeTensors | tensor interchange | Dtype, shape, values preserved |
| HTML → Markdown → text | document chain | Text content preserved |
| PNG → WebP → PNG | lossless image | Pixel data identical |
| FASTA → FASTQ → FASTA | bio conversion | Sequence preserved |
| Syslog → JSON → NDJSON | log normalization | Records preserved |
| GeoJSON → KML → GeoJSON | geo roundtrip | Coordinates preserved |
| **Total** | | **~15 tests** |

### 5.3 Format Detection Integration Tests

File: `tests/format_detection.rs`

Test `FormatDetectFn` against minimal valid sample bytes for every
supported format. Each test verifies format name, MIME type, category,
and confidence ≥ threshold.

- Binary formats with magic bytes: Parquet, ORC, Arrow, Avro, PNG,
  JPEG, GIF, WebP, TIFF, WAV, FLAC, MP3, PDF, ZIP, gzip, bzip2, xz,
  zstd, SQLite, WASM, ELF, DICOM, NumPy, GGUF (~22 tests)
- Text format structural detection: CSV, JSON, NDJSON, XML, YAML,
  TOML, INI, HTML, Markdown (~8 tests)
- Ambiguous inputs: JSON array vs CSV, XML vs HTML, YAML vs INI (~3 tests)
- **Total: ~33 tests**

### 5.4 Streaming Equivalence Tests

File: `tests/format_streaming_parity.rs`

For all 107 Both-mode functions, verify batch and streaming produce
identical output. Each function tested at 3 chunk sizes (1 byte,
64 bytes, 65536 bytes) via macro:

```rust
macro_rules! streaming_parity_test {
    ($name:ident, $fn_type:ty, $input:expr, $params:expr) => {
        #[test]
        fn $name() {
            let func = <$fn_type>::default();
            let batch_out = func.execute(vec![$input.clone()], &$params).unwrap();
            for chunk_size in [1, 64, 65536] {
                let stream_out = run_streaming(&func, &$input, chunk_size, &$params).unwrap();
                assert_eq!(batch_out, stream_out,
                    "parity failed at chunk_size={}", chunk_size);
            }
        }
    };
}
```

**Total: ~107 tests** (one per Both-mode function).

### 5.5 Benchmark Suite

File: `benches/format_benchmarks.rs` (Criterion)

| Benchmark | Variants |
|-----------|----------|
| Parquet read | 1 MB, 100 MB, 1 GB; with/without projection |
| CSV parse | 1 MB, 100 MB; with/without schema |
| gzip compress/decompress | 1 MB, 100 MB; levels 1, 6, 9 |
| RS encode/decode | 4+2, 6+3, 10+4; 1 MB, 64 MB |
| Image resize | 1024², 4096²; PNG, JPEG |
| CID compute | 1 KB, 1 MB, 1 GB; SHA-256, BLAKE3 |
| FormatDetect | 100 files, mixed formats |

**Grand total: ~849 unit + ~15 cross + ~33 detection + ~107 parity = ~1004 tests**

---

## 6. Edge Cases & Error Handling

All format functions map errors to `ComputeError` variants following
the same conventions as §2.14/§2.15:

| Error Class | ComputeError Variant | Examples |
|-------------|---------------------|----------|
| Invalid/missing parameter | `InvalidParam(String)` | Unknown codec name, negative shard count, unsupported format in `FormatConvert` |
| Corrupt or invalid input | `ExecutionFailed(String)` | Bad magic bytes, CRC mismatch, truncated header, malformed protobuf |
| Unsupported variant | `ExecutionFailed(String)` | pcapng instead of pcap, WOFF3, GGUF version 99 |
| Resource limit exceeded | `ExecutionFailed(String)` | SafeTensors header > 100 MB, Pickle depth > max_depth |
| Missing dependency data | `ExecutionFailed(String)` | BAM region query without index, UnixFS assemble with missing block |
| Empty input | Context-dependent | CID → valid CID of empty bytes; most others → empty output or error |

**Cross-cutting edge cases handled uniformly:**

| Edge Case | Behavior |
|-----------|----------|
| Empty input (0 bytes) | Functions that produce metadata/CID: valid output. Parse/read functions: empty output. Validate functions: error. |
| Non-UTF-8 text input | Text parsers (CSV, JSON, XML, YAML, config, log, bio) attempt UTF-8; fall back to Latin-1 for CSV; error for strict formats (JSON, YAML) |
| Truncated file (valid header, incomplete body) | Batch: error with byte offset. Streaming: emit complete records, buffer remainder, error on flush if incomplete. |
| Corrupt magic bytes | `FormatDetectFn` returns `"unknown"`. Category-specific functions return `ExecutionFailed("invalid X header")`. |
| File exceeds available memory (batch-only) | No built-in limit — caller is responsible via §2.9 size-aware mode selection routing large files to streaming variants. |
| Concurrent access | Not applicable — functions are stateless; each invocation gets its own input copy. SQLite opened read-only on a temp file copy. |
| Integer overflow in headers | Validated before allocation: reject declared sizes > input length or > 4 GB for 32-bit fields. |
| Zip bomb / decompression bomb | Archive functions enforce `max_size` param (default 1 GB uncompressed). Image functions enforce max dimensions (default 16384×16384). |
| DICOM PHI in unexpected tags | `DicomAnonymizeFn` removes the 18 HIPAA categories; private tags (odd group numbers) removed entirely unless in `keep_tags`. |
| Pickle code execution | `PickleToJsonFn` rejects REDUCE, BUILD, INST, OBJ, NEWOBJ opcodes — only data-construction opcodes allowed. |

---

## 7. Performance Analysis

### 7.1 Columnar Format Performance

Columnar formats offer the largest performance gains through
selective reading:

| Optimization | Mechanism | Expected Speedup |
|-------------|-----------|-----------------|
| Column projection | Skip unneeded column chunks entirely | 10–100× (proportional to columns skipped) |
| Predicate pushdown | Skip row groups via min/max statistics | 2–50× (depends on selectivity and data distribution) |
| Dictionary encoding | Decompress only dictionary + indices | 2–5× for low-cardinality string columns |
| Page-level filtering | Skip pages within a column chunk | 1.5–3× additional on top of row group skip |

**Memory profile**: Parquet read with projection loads only selected
columns into memory. A 10 GB Parquet file with 100 columns, reading
2 columns, requires ~200 MB memory (2% of file). Without projection,
the full row group (~128 MB default) must be buffered.

**ORC vs Parquet**: ORC has built-in bloom filters and lightweight
indexes per stripe. For point lookups, ORC can be 2–3× faster than
Parquet without external indexes. For full scans, performance is
comparable.

**Arrow IPC**: Zero-copy reads when memory-mapped. Read throughput
limited by memory bandwidth (~10 GB/s on modern hardware). No
deserialization overhead.

### 7.2 Archive Performance

| Operation | Throughput (single core) | Bottleneck |
|-----------|------------------------|------------|
| tar list/extract (uncompressed) | ~2 GB/s | I/O |
| tar.gz decompress (level 6) | ~400 MB/s | CPU (zlib) |
| tar.zst decompress | ~1.5 GB/s | CPU (zstd) |
| zip extract (deflate) | ~400 MB/s | CPU (zlib) |
| gzip compress (level 1) | ~300 MB/s | CPU |
| gzip compress (level 9) | ~30 MB/s | CPU |
| zstd compress (level 3) | ~500 MB/s | CPU |
| zstd compress (level 19) | ~10 MB/s | CPU |

**Streaming vs batch**: tar is sequential → streaming adds zero
overhead. zip requires central directory at EOF → batch for listing,
but individual file extraction can stream if offset is known.

**Pipeline composition**: `tar_extract | gzip_decompress` adds one
buffer copy per stage. Overhead is ~5% compared to a fused
tar.gz reader, acceptable for the composability benefit.

### 7.3 Image Processing Performance

| Operation | 1024×1024 | 4096×4096 | Scaling |
|-----------|-----------|-----------|---------|
| PNG decode | ~15 ms | ~200 ms | O(pixels) |
| JPEG decode | ~5 ms | ~60 ms | O(pixels) |
| Resize (Lanczos3) | ~20 ms | ~300 ms | O(src × dst pixels) |
| PNG encode | ~30 ms | ~500 ms | O(pixels) |
| WebP encode (lossless) | ~50 ms | ~800 ms | O(pixels) |
| Perceptual hash | ~2 ms | ~5 ms | O(pixels) after 8×8 resize |

**Memory**: Image processing requires full pixel buffer in memory.
A 4096×4096 RGBA image = 64 MB. The `max_dimension` guard (default
16384) caps memory at ~1 GB per image.

### 7.4 Erasure Coding Performance

| Configuration | Encode (MB/s) | Decode (MB/s) | Verify (MB/s) |
|--------------|---------------|---------------|----------------|
| RS 4+2 | ~800 | ~600 | ~1200 |
| RS 6+3 | ~600 | ~450 | ~900 |
| RS 10+4 | ~400 | ~300 | ~700 |
| XOR (any count) | ~3000 | ~3000 | ~3000 |
| Replication | ~memcpy | ~memcpy | ~memcpy |

Reed-Solomon throughput scales inversely with total shard count due
to GF(2^8) matrix multiplication. XOR parity is SIMD-accelerated
and approaches memory bandwidth. Replication is a memcpy.

**Streaming overhead**: RS operates on aligned blocks. If the
streaming chunk size aligns with shard size (the common case in DFS),
overhead is zero. Misaligned chunks require buffering one stripe
width (~256 KB for 4+2 with 64 KB shards).

### 7.5 Benchmark Specifications

All benchmarks use Criterion with the following methodology:

**Environment requirements**:
- Warm-up: 3 seconds per benchmark
- Measurement: 5 seconds, minimum 100 iterations
- Statistical: report mean, median, std dev, throughput (MB/s)
- Baseline: saved for regression detection across commits

**Benchmark groups** (file: `benches/format_benchmarks.rs`):

```rust
// Group 1: Columnar read throughput
fn bench_parquet_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("parquet_read");
    group.throughput(Throughput::Bytes(data.len() as u64));
    for cols in [1, 5, "all"] {
        group.bench_with_input(
            BenchmarkId::new("projection", cols), &data,
            |b, data| b.iter(|| parquet_read(data, cols)),
        );
    }
    group.finish();
}
```

| Group | Benchmark ID | Input Sizes | Variants |
|-------|-------------|-------------|----------|
| `parquet_read` | `projection/{1,5,all}` | 1 MB, 100 MB | With/without predicate |
| `parquet_write` | `codec/{none,snappy,zstd}` | 1 MB, 100 MB | 3 codecs |
| `csv_parse` | `rows/{1K,100K,1M}` | 100 KB, 10 MB, 100 MB | With/without schema |
| `archive_compress` | `algo/{gzip,zstd,bzip2,xz}` | 1 MB, 100 MB | Default level |
| `archive_decompress` | `algo/{gzip,zstd,bzip2,xz}` | 1 MB, 100 MB | — |
| `rs_encode` | `config/{4+2,6+3,10+4}` | 1 MB, 64 MB | — |
| `rs_decode` | `config/{4+2,6+3,10+4}` | 1 MB, 64 MB | 1 missing shard |
| `image_resize` | `res/{1024,4096}` | PNG, JPEG | Lanczos3 |
| `cid_compute` | `hash/{sha256,blake3}` | 1 KB, 1 MB, 1 GB | — |
| `format_detect` | `batch/100` | 100 mixed files | — |
| `json_parse` | `size/{1K,100K,1M}` | 1 KB, 100 KB, 1 MB | — |
| `avro_roundtrip` | `records/{1K,100K}` | 100 KB, 10 MB | — |

**Regression thresholds**: CI fails if any benchmark regresses > 10%
from the saved baseline. Baselines are updated on merge to `main`.

**Throughput targets** (minimum acceptable on a single core):

| Category | Operation | Target |
|----------|-----------|--------|
| Columnar | Parquet read (full scan) | ≥ 500 MB/s |
| Columnar | Parquet read (2/100 cols) | ≥ 5 GB/s effective |
| Row | CSV parse | ≥ 200 MB/s |
| Row | JSON parse | ≥ 150 MB/s |
| Archive | zstd decompress | ≥ 1 GB/s |
| Archive | gzip decompress | ≥ 300 MB/s |
| Erasure | RS 4+2 encode | ≥ 500 MB/s |
| Erasure | XOR parity | ≥ 2 GB/s |
| CAS | CID compute (SHA-256) | ≥ 500 MB/s |
| CAS | CID compute (BLAKE3) | ≥ 2 GB/s |
| Image | PNG decode 1024² | ≤ 20 ms |
| Detection | FormatDetect | ≤ 10 µs per file |

---

## 8. Files Changed

| File | Action | Description |
|------|--------|-------------|
| `crates/deriva-compute/src/builtins_format.rs` | New | Top-level registration dispatcher (`register_format_functions`) |
| `crates/deriva-compute/src/builtins_format_columnar.rs` | New | Category A: Parquet, ORC, Arrow IPC (18 functions) |
| `crates/deriva-compute/src/builtins_format_csv.rs` | New | Category B: CSV, TSV, JSON, NDJSON, XML (25 functions) |
| `crates/deriva-compute/src/builtins_format_serialization.rs` | New | Category C: Avro, Protobuf, Thrift, MsgPack, CBOR, BSON (19 functions) |
| `crates/deriva-compute/src/builtins_format_archive.rs` | New | Category D: tar, zip, gzip, bzip2, xz, zstd, 7z, rar (20 functions) |
| `crates/deriva-compute/src/builtins_format_image.rs` | New | Category E: PNG, JPEG, WebP, TIFF, SVG, GIF (18 functions) |
| `crates/deriva-compute/src/builtins_format_audio.rs` | New | Category F: MP3, WAV, FLAC, MP4, MKV (16 functions) |
| `crates/deriva-compute/src/builtins_format_document.rs` | New | Category G: PDF, DOCX, XLSX, HTML, Markdown (23 functions) |
| `crates/deriva-compute/src/builtins_format_config.rs` | New | Category H: YAML, TOML, INI, HCL (16 functions) |
| `crates/deriva-compute/src/builtins_format_geo.rs` | New | Category I: GeoJSON, Shapefile, KML, GeoTIFF (14 functions) |
| `crates/deriva-compute/src/builtins_format_scientific.rs` | New | Category J: HDF5, NetCDF, FITS, NumPy, Zarr (13 functions) |
| `crates/deriva-compute/src/builtins_format_log.rs` | New | Category K: Syslog, CEF, Apache/Nginx, CloudTrail, OTLP (13 functions) |
| `crates/deriva-compute/src/builtins_format_cas.rs` | New | Category L: CID, DAG-PB, DAG-CBOR, CAR, UnixFS (15 functions) |
| `crates/deriva-compute/src/builtins_format_database.rs` | New | Category M: SQLite, RocksDB SST, WAL, Postgres COPY (10 functions) |
| `crates/deriva-compute/src/builtins_format_ml.rs` | New | Category N: TFRecord, SafeTensors, ONNX, GGUF (12 functions) |
| `crates/deriva-compute/src/builtins_format_network.rs` | New | Category O: PCAP, DNS, HAR, Email (8 functions) |
| `crates/deriva-compute/src/builtins_format_bio.rs` | New | Category P: FASTA, FASTQ, SAM/BAM, VCF, BED (12 functions) |
| `crates/deriva-compute/src/builtins_format_binary.rs` | New | Category Q: WOFF, glTF, STL, DICOM, WASM, ELF (13 functions) |
| `crates/deriva-compute/src/builtins_format_erasure.rs` | New | Category R: Reed-Solomon, XOR, replication, striping (9 functions) |
| `crates/deriva-compute/src/builtins_format_detect.rs` | New | Category S: FormatDetect, universal conversion, schema (9 functions) |
| `crates/deriva-compute/src/lib.rs` | Modify | Add `mod builtins_format*`; call `register_format_functions` |
| `crates/deriva-compute/Cargo.toml` | Modify | Add 19 feature flags + ~45 optional dependencies |
| `crates/deriva-compute/tests/format_columnar.rs` | New | Category A tests (~54) |
| `crates/deriva-compute/tests/format_csv.rs` | New | Category B tests (~75) |
| `crates/deriva-compute/tests/format_serialization.rs` | New | Category C tests (~57) |
| `crates/deriva-compute/tests/format_archive.rs` | New | Category D tests (~60) |
| `crates/deriva-compute/tests/format_image.rs` | New | Category E tests (~54) |
| `crates/deriva-compute/tests/format_audio.rs` | New | Category F tests (~48) |
| `crates/deriva-compute/tests/format_document.rs` | New | Category G tests (~69) |
| `crates/deriva-compute/tests/format_config.rs` | New | Category H tests (~48) |
| `crates/deriva-compute/tests/format_geo.rs` | New | Category I tests (~42) |
| `crates/deriva-compute/tests/format_scientific.rs` | New | Category J tests (~39) |
| `crates/deriva-compute/tests/format_log.rs` | New | Category K tests (~39) |
| `crates/deriva-compute/tests/format_cas.rs` | New | Category L tests (~45) |
| `crates/deriva-compute/tests/format_database.rs` | New | Category M tests (~30) |
| `crates/deriva-compute/tests/format_ml.rs` | New | Category N tests (~36) |
| `crates/deriva-compute/tests/format_network.rs` | New | Category O tests (~24) |
| `crates/deriva-compute/tests/format_bio.rs` | New | Category P tests (~36) |
| `crates/deriva-compute/tests/format_binary.rs` | New | Category Q tests (~39) |
| `crates/deriva-compute/tests/format_erasure.rs` | New | Category R tests (~27) |
| `crates/deriva-compute/tests/format_detect.rs` | New | Category S tests (~27) |
| `crates/deriva-compute/tests/format_cross_conversion.rs` | New | Cross-category roundtrip tests (~15) |
| `crates/deriva-compute/tests/format_detection.rs` | New | Format detection integration tests (~33) |
| `crates/deriva-compute/tests/format_streaming_parity.rs` | New | Streaming equivalence tests (~107) |
| `crates/deriva-compute/benches/format_benchmarks.rs` | New | Criterion benchmark suite (12 groups) |

**Summary**: 20 new source files, 2 modified files, 23 new test files, 1 benchmark file = **46 files total**.

---

## 9. Dependency Changes

All dependencies are optional, gated behind feature flags. No new
dependencies are added to the default build unless `format-phase1`
is enabled (which it is by default).

### Phase 1 (16 new crates)

| Crate | Version | Feature Flag | Purpose |
|-------|---------|-------------|---------|
| `csv` | 1 | `format-csv` | CSV read/write |
| `quick-xml` | 0.36 | `format-csv` | XML read/write |
| `tar` | 0.4 | `format-archive` | tar archive read/write |
| `zip` | 2 | `format-archive` | zip archive read/write |
| `flate2` | 1 | `format-archive` | gzip compress/decompress |
| `bzip2` | 0.4 | `format-archive` | bzip2 compress/decompress |
| `xz2` | 0.1 | `format-archive` | xz compress/decompress |
| `zstd` | 0.13 | `format-archive` | zstd compress/decompress |
| `serde_yaml` | 0.9 | `format-config` | YAML parse/write |
| `toml` | 0.8 | `format-config` | TOML parse/write |
| `configparser` | 3 | `format-config` | INI parse/write |
| `hcl-rs` | 0.18 | `format-config` | HCL parse |
| `cid` | 0.11 | `format-cas` | CID computation/parsing |
| `multihash` | 0.19 | `format-cas` | Multihash digests |
| `libipld` | 0.16 | `format-cas` | DAG-PB, DAG-CBOR codec |
| `iroh-car` | 0.6 | `format-cas` | CAR archive read/write |
| `infer` | 0.16 | `format-detect` | Magic byte detection |
| `mime_guess` | 2 | `format-detect` | MIME↔extension mapping |

### Phase 2 (9 new crates)

| Crate | Version | Feature Flag | Purpose |
|-------|---------|-------------|---------|
| `parquet` | 53 | `format-columnar` | Parquet read/write |
| `arrow` | 53 | `format-columnar` | Arrow arrays, IPC |
| `orc-rust` | 0.4 | `format-columnar` | ORC read |
| `apache-avro` | 0.17 | `format-serialization` | Avro read/write |
| `prost` | 0.13 | `format-serialization` | Protobuf decode/encode |
| `rmp-serde` | 1 | `format-serialization` | MessagePack serde |
| `ciborium` | 0.2 | `format-serialization` | CBOR read/write |
| `bson` | 2 | `format-serialization` | BSON read/write |
| `reed-solomon-erasure` | 6 | `format-erasure` | Reed-Solomon GF(2^8) |

### Phase 3 (8 new crates)

| Crate | Version | Feature Flag | Purpose |
|-------|---------|-------------|---------|
| `image` | 0.25 | `format-image` | PNG/JPEG/WebP/GIF/TIFF decode/encode |
| `resvg` | 0.43 | `format-image` | SVG rasterization |
| `img-hash` | 3 | `format-image` | Perceptual hashing |
| `symphonia` | 0.5 | `format-audio` | MP3/WAV/FLAC/MP4 metadata |
| `lopdf` | 0.33 | `format-document` | PDF text extraction |
| `calamine` | 0.26 | `format-document` | XLSX/XLS/ODS read |
| `scraper` | 0.20 | `format-document` | HTML parsing |
| `pulldown-cmark` | 0.12 | `format-document` | Markdown parsing |

### Phase 4 (11 new crates)

| Crate | Version | Feature Flag | Purpose |
|-------|---------|-------------|---------|
| `geojson` | 0.24 | `format-geo` | GeoJSON read/write |
| `flatgeobuf` | 4 | `format-geo` | FlatGeobuf, Shapefile |
| `hdf5` | 0.8 | `format-scientific` | HDF5 read (requires libhdf5 system dep) |
| `rusqlite` | 0.32 | `format-database` | SQLite read/write (bundled) |
| `safetensors` | 0.4 | `format-ml` | SafeTensors read/write |
| `wasmparser` | 0.218 | `format-binary` | WASM validation/metadata |
| `dicom-object` | 0.7 | `format-binary` | DICOM read/write/anonymize |
| `gltf` | 1 | `format-binary` | glTF parse/validate |
| `pcap-parser` | 0.16 | `format-network` | PCAP packet parsing |
| `mailparse` | 0.15 | `format-network` | Email MIME parsing |
| `noodles` | 0.82 | `format-bio` | SAM/BAM/VCF/FASTA/FASTQ |

### Summary

| Phase | New Crates | System Dependencies |
|-------|-----------|-------------------|
| 1 | 18 | None |
| 2 | 9 | None |
| 3 | 8 | None |
| 4 | 11 | `libhdf5` (for `hdf5` crate, Category J only) |
| **Total** | **46** | **1 optional** |

**Build impact**: With default features (`format-phase1` only), 18
new crates are compiled. Full `format-all` adds 46 crates. Each
phase is additive — enabling Phase 2 does not pull Phase 3/4 deps.

---

## 10. Design Rationale

### 10.1 Why Feature Flags Instead of Separate Crates?

A single `deriva-compute` crate with feature flags keeps the
`FunctionRegistry` unified — all 283 functions register through the
same `ComputeFunction` trait and share the same `Bytes` I/O contract.
Separate crates would require either:
- Dynamic loading (`dlopen`) — adds runtime complexity, ABI stability
  concerns, and platform-specific behavior
- A workspace of crates with a top-level aggregator — adds 19 crates
  to the workspace, each with its own `Cargo.toml`, increasing
  maintenance burden without meaningful isolation benefit

Feature flags are idiomatic Rust for optional functionality. They
provide compile-time selection with zero runtime cost, and `cargo`
handles dependency resolution automatically. The tradeoff is longer
`Cargo.toml` feature sections, which is acceptable for 19 flags.

### 10.2 Why Phase 1 Prioritizes CSV/Log/Config Over Parquet/Arrow?

Phase 1 targets formats that are:
1. **Ubiquitous** — CSV, JSON, YAML, gzip are used in every domain
2. **Low-dependency** — no heavy C/C++ bindings (unlike `parquet`
   which pulls `arrow` → `arrow-buffer` → `arrow-schema` → ...)
3. **Immediately useful** — config files, log parsing, and archive
   extraction are needed for bootstrapping the DFS itself
4. **CAS-native** — CID/CAR/UnixFS (Category L) directly serve the
   Computation-Addressed Store, the project's core abstraction

Parquet/Arrow (Phase 2) bring ~2 MB of compiled code and complex
build requirements. Deferring them lets Phase 1 ship fast with a
small binary.

### 10.3 Why Include Niche Formats (DICOM, FASTA, PCAP)?

A universal content-addressed store must handle data as-is. Real
data lakes contain:
- **Healthcare**: DICOM images (PACS systems store petabytes)
- **Genomics**: FASTA/FASTQ/BAM (a single genome run = 100+ GB)
- **Security**: PCAP captures (network forensics, compliance)

Without format-aware functions, these files are opaque blobs —
addressable but not queryable. With them, a CAddr recipe can express
`bam_read(region="chr1:1000-2000")` and the DFS can skip irrelevant
blocks.

Feature flags make these zero-cost for users who don't need them.
The `format-phase4` bundle is not in `default` features.

### 10.4 Why 283 Functions Instead of Fewer Generic Ones?

**Specific functions enable precise computation addresses.** A CAddr
recipe `parquet_projection(columns=["age","name"])` is:
- **Self-describing**: the function name documents intent
- **Type-safe**: params are validated at registration time
- **Optimizable**: the executor knows to skip unneeded Parquet columns
- **Cacheable**: identical CAddr → identical result (deterministic)

A generic `read(format="parquet", options={"columns":["age","name"]})`
loses all of these properties — the executor cannot optimize without
parsing the opaque `options` map, error messages are generic, and
the function's behavior depends on a runtime string.

The 283-function approach follows the Unix philosophy: each function
does one thing well. Composition happens at the pipeline level, not
inside a monolithic dispatcher.

---

## 11. Observability Integration

Same metric framework as §2.14/§2.15 — all format functions emit
metrics through the existing `MetricsCollector` trait.

### Per-Function Metrics

Every format function emits on each invocation:

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `compute_function_duration_seconds` | Histogram | `function_id`, `mode` | Wall-clock execution time |
| `compute_function_input_bytes` | Counter | `function_id` | Total input bytes processed |
| `compute_function_output_bytes` | Counter | `function_id` | Total output bytes produced |
| `compute_function_errors_total` | Counter | `function_id`, `error_type` | Error count by variant (`InvalidParam`, `ExecutionFailed`) |

### Per-Category Aggregate Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `format_category_invocations_total` | Counter | `category` (A–S) | Total invocations per category |
| `format_category_bytes_processed` | Counter | `category` | Total bytes processed per category |

### Format-Specific Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `format_detect_confidence` | Histogram | `detected_format` | Detection confidence distribution |
| `format_detect_unknown_total` | Counter | — | Files that could not be identified |
| `format_parquet_row_groups_skipped` | Counter | `function_id` | Row groups skipped via predicate pushdown |
| `format_parquet_columns_projected` | Histogram | `function_id` | Number of columns read vs total |
| `format_rs_shards_reconstructed` | Counter | `config` | Shards reconstructed in RS decode |
| `format_archive_compression_ratio` | Histogram | `algorithm` | Compression ratio (output/input) |

### Structured Logging

All format functions log at `DEBUG` level:
```
DEBUG function_id="csv_parse@1.0.0" input_bytes=1048576 output_rows=25000 duration_ms=12
DEBUG function_id="parquet_projection@1.0.0" columns_requested=2 columns_total=50 row_groups_read=3 row_groups_skipped=7
DEBUG function_id="format_detect@1.0.0" detected="parquet" confidence=0.99 magic="PAR1"
DEBUG function_id="rs_decode@1.0.0" config="4+2" missing=[1,3] reconstructed=2
```

`WARN` level for recoverable issues:
```
WARN function_id="wal_parse@1.0.0" msg="truncated record at offset 4096, skipping"
WARN function_id="csv_parse@1.0.0" msg="non-UTF-8 input, falling back to Latin-1"
```

---

## 12. Checklist

### 12.1 Phase 1 Implementation
- [x] Category S: Universal detection — `builtins_format_detect.rs` (9 functions)
- [x] Category B: CSV/JSON/XML — `builtins_format_csv.rs` (25 functions)
- [x] Category H: Config — `builtins_format_config.rs` (16 functions)
- [x] Category D: Archive — `builtins_format_archive.rs` (20 functions)
- [x] Category K: Log — `builtins_format_log.rs` (13 functions)
- [x] Category L: CAS — `builtins_format_cas.rs` (15 functions)

### 12.2 Phase 2 Implementation
- [x] Category R: Erasure coding — `builtins_format_erasure.rs` (9 functions)
- [x] Category C: Serialization — `builtins_format_serialization.rs` (19 functions)
- [x] Category A: Columnar — `builtins_format_columnar.rs` (18 functions)

### 12.3 Phase 3 Implementation
- [x] Category E: Image — `builtins_format_image.rs` (18 functions)
- [x] Category G: Document — `builtins_format_document.rs` (23 functions)
- [x] Category F: Audio/Video — `builtins_format_audio.rs` (16 functions)

### 12.4 Phase 4 Implementation
- [x] Category I: Geospatial — `builtins_format_geo.rs` (14 functions)
- [x] Category J: Scientific — `builtins_format_scientific.rs` (13 functions)
- [x] Category M: Database — `builtins_format_database.rs` (10 functions)
- [ ] Category N: ML/Tensor — `builtins_format_ml.rs` (12 functions)
- [ ] Category O: Network — `builtins_format_network.rs` (8 functions)
- [ ] Category P: Bioinformatics — `builtins_format_bio.rs` (12 functions)
- [ ] Category Q: Specialized binary — `builtins_format_binary.rs` (13 functions)

### 12.5 Infrastructure
- [ ] `builtins_format.rs` registration dispatcher
- [ ] `Cargo.toml` feature flags (19 category flags + 4 phase bundles + `format-all`)
- [ ] `lib.rs` module declarations and conditional registration
- [ ] Feature flag CI matrix (test each phase independently)

### 12.6 Testing
- [ ] 19 per-category test files (~849 tests)
- [ ] Cross-category conversion tests (~15 tests)
- [ ] Format detection integration tests (~33 tests)
- [ ] Streaming equivalence tests (~107 tests)
- [ ] Criterion benchmark suite (12 groups)

### 12.7 Quality Gates
- [ ] `cargo clippy --workspace --all-features -- -D warnings` clean
- [ ] `cargo test --workspace --all-features` passes
- [ ] `cargo test --workspace` passes (default features only)
- [ ] All existing §2.14/§2.15 tests still pass
- [ ] No throughput regression > 10% on benchmarks
- [ ] Observability metrics emitted for all functions
- [ ] Commit and push
