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

TODO — table of 18 functions: ParquetRead, ParquetWrite, ParquetMetadata, ParquetProjection, ParquetFilter, ParquetMerge, ParquetToArrow, ArrowToParquet, OrcRead, OrcWrite, OrcMetadata, OrcFilter, ArrowIpcRead (Both), ArrowIpcWrite (Both), ArrowSchemaExtract, ParquetPartitionWrite, ParquetStatistics, ColumnarToRow

#### 3.1.2 Design Notes

TODO — Parquet/ORC require footer parsing (random access); Arrow IPC is streaming-capable (record batch per chunk); predicate pushdown uses row-group/stripe statistics to skip data

#### 3.1.3 Test Strategy

TODO — roundtrip tests (write → read), projection correctness, predicate pushdown skipping, schema evolution in merge, Arrow IPC streaming equivalence

### 3.2 Category B: Row-Oriented & Delimited Formats — CSV, TSV, JSON, NDJSON, XML (25 functions, #219–#243)

> **Phase**: 1 — High-value, low-complexity
> **Key dependencies**: `csv`, `serde_json`, `quick-xml`, `jsonpath-rust`
> **Mode**: Many streaming-capable (line-oriented)

#### 3.2.1 Function List

TODO — table of 25 functions: CsvParse (Both), CsvWrite (Both), CsvSchemaInfer, CsvColumnSelect (Both), CsvColumnRename (Both), CsvFilter (Both), CsvSort, CsvAggregate, CsvJoin, CsvDeduplicate, TsvParse (Both), NdjsonParse (Both), NdjsonWrite (Both), NdjsonFilter (Both), NdjsonProject (Both), JsonPathExtract, JsonMerge, JsonFlatten, JsonUnflatten, XmlParse, XmlWrite, XmlXPathExtract, XmlXsltTransform, XmlValidateDtd, XmlValidateXsd

#### 3.2.2 Design Notes

TODO — CSV/TSV/NDJSON are line-oriented → natural streaming; JSON/XML require full parse for structural ops; CSV streaming functions maintain header state across chunks

#### 3.2.3 Test Strategy

TODO — CSV roundtrip, schema inference accuracy, filter correctness, join types (inner/left/right/full), NDJSON streaming line-by-line, XML validation against DTD/XSD

### 3.3 Category C: Serialization Formats — Avro, Protobuf, Thrift, MessagePack, CBOR, BSON (19 functions, #244–#262)

> **Phase**: 2 — Analytics backbone
> **Key dependencies**: `apache-avro`, `prost`, `rmp-serde`, `ciborium`, `bson`
> **Mode**: Mixed (Avro streaming-capable via blocks; Protobuf via length-delimited messages)

#### 3.3.1 Function List

TODO — table of 19 functions: AvroRead (Both), AvroWrite (Both), AvroSchemaExtract, AvroSchemaEvolve, AvroToJson (Both), JsonToAvro (Both), ProtobufDecode (Both), ProtobufEncode (Both), ProtobufSchemaExtract, ThriftDecode, ThriftEncode, MsgpackDecode (Both), MsgpackEncode (Both), CborDecode (Both), CborEncode (Both), BsonDecode (Both), BsonEncode (Both), AvroToParquet, ParquetToAvro

#### 3.3.2 Design Notes

TODO — Avro OCF has block structure → streaming one block per chunk; Protobuf length-delimited → streaming one message per chunk; CBOR critical for IPFS/IPLD interop (DAG-CBOR)

#### 3.3.3 Test Strategy

TODO — schema evolution (added/removed fields), Avro codec roundtrip (null/deflate/snappy), Protobuf descriptor validation, cross-format conversion (Avro↔Parquet)

### 3.4 Category D: Archive & Container Formats — tar, zip, gzip, bzip2, xz, zstd-frame, 7z, rar (20 functions, #263–#282)

> **Phase**: 1 — High-value, low-complexity
> **Key dependencies**: `tar`, `zip`, `flate2`, `bzip2`, `xz2`, `zstd`, `sevenz-rust`
> **Mode**: Sequential archives streaming-capable; random-access archives batch-only

#### 3.4.1 Function List

TODO — table of 20 functions: TarCreate (Both), TarExtract (Both), TarList (Both), TarAppend, GzipCompress (Streaming), GzipDecompress (Streaming), Bzip2Compress (Streaming), Bzip2Decompress (Streaming), XzCompress (Streaming), XzDecompress (Streaming), ZipCreate, ZipExtract, ZipList, ZipExtractSingle, TarGzCreate (Both), TarGzExtract (Both), ZstdFrameCompress (Streaming), ZstdFrameDecompress (Streaming), SevenZExtract, RarExtract

#### 3.4.2 Design Notes

TODO — tar is sequential → streaming natural; zip requires central directory → batch; whole-stream compression (gzip/bzip2/xz/zstd-frame) maintains codec state across chunks unlike per-chunk compression in §2.14

#### 3.4.3 Test Strategy

TODO — tar create/extract roundtrip, tar.gz pipeline composition, zip random access, whole-stream vs per-chunk compression ratio comparison

### 3.5 Category E: Image Formats — PNG, JPEG, WebP, TIFF, SVG, GIF (18 functions, #283–#300)

> **Phase**: 3 — Rich media
> **Key dependencies**: `image`, `imageproc`, `resvg`, `img-hash`
> **Mode**: Predominantly batch (pixel-level random access); metadata/detection streaming-capable

#### 3.5.1 Function List

TODO — table of 18 functions: ImageMetadata (Both), ImageResize, ImageCrop, ImageRotate, ImageConvert, ImageThumbnail, ImageStripMetadata (Both), ImageToGrayscale, ImageWatermark, ImageHash, PngOptimize, JpegOptimize, SvgMinify, SvgToPng, TiffSplit, TiffMerge, GifExtractFrames, ImageDetectFormat (Both)

#### 3.5.2 Design Notes

TODO — most ops require full decode to pixel buffer; metadata extraction reads only header → streaming; perceptual hash (pHash/dHash) for near-duplicate detection

#### 3.5.3 Test Strategy

TODO — resize correctness (dimensions), format conversion roundtrip, metadata strip verification, thumbnail quality, SVG rasterization

### 3.6 Category F: Audio & Video Formats — MP3, WAV, FLAC, MP4, MKV (16 functions, #301–#316)

> **Phase**: 3 — Rich media
> **Key dependencies**: `symphonia`, `ffmpeg-sys-next` (optional)
> **Mode**: Predominantly batch; raw PCM streaming-capable

#### 3.6.1 Function List

TODO — table of 16 functions: AudioMetadata (Both), AudioConvert, AudioTrim, AudioNormalize, AudioWaveform, AudioSilenceDetect, AudioStripMetadata, VideoMetadata (Both), VideoThumbnail, VideoExtractAudio, VideoStripAudio, VideoResolution, SubtitleExtract, WavToRawPcm (Both), RawPcmToWav (Both), MediaDetectFormat (Both)

#### 3.6.2 Design Notes

TODO — media files are largest objects in storage; metadata extraction is header-only; transcoding requires full decode; WAV↔PCM is streaming (byte stream)

#### 3.6.3 Test Strategy

TODO — metadata extraction accuracy, WAV/PCM roundtrip, audio trim boundaries, silence detection thresholds

### 3.7 Category G: Document Formats — PDF, DOCX, XLSX, HTML, Markdown (23 functions, #317–#339)

> **Phase**: 3 — Rich media
> **Key dependencies**: `lopdf`, `calamine`, `scraper`, `pulldown-cmark`, `comrak`
> **Mode**: Predominantly batch (complex internal structure); HTML streaming-capable

#### 3.7.1 Function List

TODO — table of 23 functions: PdfMetadata, PdfExtractText, PdfExtractImages, PdfMerge, PdfSplit, PdfPageCount, PdfToText, DocxExtractText, DocxMetadata, XlsxRead, XlsxSheetList, XlsxToCsv, PptxExtractText, PptxSlideCount, HtmlToText (Both), HtmlExtractLinks, HtmlExtractImages, HtmlMinify (Both), MarkdownToHtml, HtmlToMarkdown, LatexToText, EpubExtractText, EpubMetadata

#### 3.7.2 Design Notes

TODO — PDF has page tree (random access); DOCX/XLSX/PPTX are ZIP-based (batch); HTML tag-aware chunking enables streaming

#### 3.7.3 Test Strategy

TODO — PDF text extraction accuracy, XLSX cell value roundtrip, HTML→text vs Markdown→HTML, multi-page TIFF/PDF split/merge

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
