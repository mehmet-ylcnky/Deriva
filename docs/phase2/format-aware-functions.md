# Format-Aware Function Library for Computation-Addressed Store

Comprehensive function catalog covering all major file formats encountered in distributed file systems (HDFS, S3, GCS, Ceph, IPFS, Alluxio). Each function specifies its mode (Batch, Streaming, or Both), execution pattern, memory profile, and implementation complexity.

This document extends the core function libraries defined in `future-improvements-streaming.md` (§6: 100 streaming functions, §7: 100 batch functions). Functions here are numbered starting at 201 to avoid ID collisions.

**Design principles:**
- Every format-aware function implements `ComputeFunction` (batch) and/or `StreamingComputeFunction` (streaming)
- Batch mode: `execute(Vec<Bytes>, params) -> Result<Bytes, ComputeError>` — full input in memory
- Streaming mode: `start(Vec<Receiver<StreamChunk>>, params) -> Receiver<StreamChunk>` — bounded memory
- Functions that require random access or full-input parsing are batch-only
- Functions that can operate on byte streams without structural knowledge are streaming-capable
- All params are passed via `BTreeMap<String, Value>`

**Inspiration from existing DFS implementations:**
- HDFS: SequenceFile, Avro, Parquet, ORC, codec registry (gzip, snappy, lz4, zstd)
- S3: multipart upload/download, SSE-C encryption, content-type detection, lifecycle transitions
- GCS: compose objects, content-encoding, customer-managed encryption
- Ceph: erasure coding, object classes, RADOS gateway transformations
- IPFS: UnixFS chunking, DAG-PB/DAG-CBOR serialization, CID computation
- Alluxio: transparent codec, tiered storage transforms, UFS adapters
- Apache Spark/Flink: format readers/writers, schema evolution, predicate pushdown

---

## Category A: Columnar Data Formats (Parquet, ORC, Arrow)

These formats are the backbone of analytics in HDFS/S3-based data lakes. They use columnar storage with row groups, column chunks, and embedded metadata/statistics. Batch mode is strongly preferred because columnar formats require random access to footer metadata.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 201 | `ParquetRead` | Batch | full-parse | O(input) | High | Parse Parquet file, return all row groups as concatenated record batches (Arrow IPC format). Params: `columns[]` (projection), `row_group_indices[]` |
| 202 | `ParquetWrite` | Batch | full-build | O(input) | High | Convert Arrow IPC / CSV / JSON input into Parquet format. Params: `compression` (snappy/zstd/gzip/none), `row_group_size`, `schema` |
| 203 | `ParquetMetadata` | Batch | metadata-only | O(footer) | Medium | Extract Parquet footer metadata: schema, row counts, column statistics, row group offsets. Returns JSON |
| 204 | `ParquetProjection` | Batch | selective-read | O(selected_cols) | High | Read only specified columns from Parquet. Params: `columns[]`. Avoids reading unneeded column chunks |
| 205 | `ParquetFilter` | Batch | predicate-pushdown | O(matching_rows) | High | Read Parquet with row-group-level predicate pushdown. Params: `predicate` (e.g., `age > 30`). Skips non-matching row groups using statistics |
| 206 | `ParquetMerge` | Batch | multi-input | O(sum_inputs) | High | Merge multiple Parquet files into one with unified schema. Handles schema evolution (added/removed columns) |
| 207 | `ParquetToArrow` | Batch | conversion | O(input) | Medium | Convert Parquet to Arrow IPC streaming format |
| 208 | `ArrowToParquet` | Batch | conversion | O(input) | Medium | Convert Arrow IPC to Parquet. Params: `compression`, `row_group_size` |
| 209 | `OrcRead` | Batch | full-parse | O(input) | High | Parse ORC file, return record batches. Params: `columns[]`, `stripe_indices[]` |
| 210 | `OrcWrite` | Batch | full-build | O(input) | High | Convert input to ORC format. Params: `compression` (zlib/snappy/lz4/zstd), `stripe_size` |
| 211 | `OrcMetadata` | Batch | metadata-only | O(footer) | Medium | Extract ORC footer: schema, stripe info, column statistics. Returns JSON |
| 212 | `OrcFilter` | Batch | predicate-pushdown | O(matching_rows) | High | ORC with predicate pushdown using stripe/row-index statistics |
| 213 | `ArrowIpcRead` | Both | streaming-capable | O(batch) | Medium | Parse Arrow IPC stream format. Streaming mode emits one record batch per chunk |
| 214 | `ArrowIpcWrite` | Both | streaming-capable | O(batch) | Medium | Write Arrow IPC stream format. Streaming mode accepts record batches as chunks |
| 215 | `ArrowSchemaExtract` | Batch | metadata-only | O(schema) | Low | Extract Arrow schema from IPC file. Returns JSON schema definition |
| 216 | `ParquetPartitionWrite` | Batch | multi-output | O(input) | High | Write Parquet partitioned by column values (Hive-style: `col=value/`). Params: `partition_columns[]` |
| 217 | `ParquetStatistics` | Batch | analytics | O(footer) | Medium | Compute column-level statistics (min, max, null count, distinct count) from Parquet metadata without reading data |
| 218 | `ColumnarToRow` | Batch | conversion | O(input) | Medium | Convert columnar format (Parquet/ORC/Arrow) to row-oriented format (CSV/JSON). Params: `input_format`, `output_format` |

---

## Category B: Row-Oriented & Delimited Formats (CSV, TSV, JSON, NDJSON, XML)

Common interchange formats. CSV/TSV and NDJSON are line-oriented and naturally streaming. JSON and XML require full parsing for structural operations but can be streamed for pass-through/validation.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 219 | `CsvParse` | Both | line-streaming | O(row) | Medium | Parse CSV with header detection. Streaming: emit one row per chunk as JSON object. Params: `delimiter`, `quote_char`, `has_header` |
| 220 | `CsvWrite` | Both | line-streaming | O(row) | Medium | Convert JSON objects/arrays to CSV. Streaming: accept JSON objects, emit CSV rows. Params: `delimiter`, `include_header` |
| 221 | `CsvSchemaInfer` | Batch | full-scan | O(input) | Medium | Scan CSV to infer column types (int, float, string, date, boolean). Returns JSON schema |
| 222 | `CsvColumnSelect` | Both | line-streaming | O(row) | Low | Select/reorder columns by name or index. Params: `columns[]` |
| 223 | `CsvColumnRename` | Both | line-streaming | O(header+row) | Low | Rename columns. Params: `mapping` (e.g., `{"old_name": "new_name"}`) |
| 224 | `CsvFilter` | Both | line-streaming | O(row) | Medium | Filter rows by column predicate. Params: `column`, `operator` (eq/ne/gt/lt/contains/regex), `value` |
| 225 | `CsvSort` | Batch | full-buffer | O(input) | Medium | Sort rows by column(s). Params: `sort_columns[]`, `ascending[]` |
| 226 | `CsvAggregate` | Batch | full-scan | O(groups) | High | Group-by aggregation (sum, count, avg, min, max). Params: `group_by[]`, `aggregations[]` |
| 227 | `CsvJoin` | Batch | multi-input | O(smaller_input) | High | Join two CSV files on key column(s). Params: `join_type` (inner/left/right/full), `left_key`, `right_key` |
| 228 | `CsvDeduplicate` | Batch | full-buffer | O(input) | Medium | Remove duplicate rows. Params: `key_columns[]` (optional, default: all columns) |
| 229 | `TsvParse` | Both | line-streaming | O(row) | Low | TSV parsing (tab-delimited CSV variant). Same interface as `CsvParse` with `delimiter=\t` |
| 230 | `NdjsonParse` | Both | line-streaming | O(line) | Medium | Parse newline-delimited JSON. Streaming: validate and pass each line. Emit `Error` on invalid JSON line |
| 231 | `NdjsonWrite` | Both | line-streaming | O(line) | Low | Ensure each JSON value is on its own line with trailing `\n` |
| 232 | `NdjsonFilter` | Both | line-streaming | O(line) | Medium | Filter NDJSON lines by JSONPath predicate. Params: `path`, `operator`, `value` |
| 233 | `NdjsonProject` | Both | line-streaming | O(line) | Medium | Select specific fields from each NDJSON line. Params: `fields[]` |
| 234 | `JsonPathExtract` | Batch | full-parse | O(input) | Medium | Extract values using JSONPath expressions. Params: `path` (e.g., `$.store.book[*].author`) |
| 235 | `JsonMerge` | Batch | multi-input | O(sum_inputs) | Medium | Deep-merge multiple JSON objects. Params: `strategy` (overwrite/append/error_on_conflict) |
| 236 | `JsonFlatten` | Batch | full-parse | O(input) | Medium | Flatten nested JSON to dot-notation keys (e.g., `{"a":{"b":1}}` → `{"a.b":1}`) |
| 237 | `JsonUnflatten` | Batch | full-parse | O(input) | Medium | Reverse of flatten: dot-notation keys → nested structure |
| 238 | `XmlParse` | Batch | full-parse | O(input) | High | Parse XML to JSON representation. Params: `preserve_attributes`, `text_key` |
| 239 | `XmlWrite` | Batch | full-build | O(input) | High | Convert JSON to XML. Params: `root_element`, `indent` |
| 240 | `XmlXPathExtract` | Batch | full-parse | O(input) | High | Extract nodes using XPath. Params: `xpath` |
| 241 | `XmlXsltTransform` | Batch | full-parse | O(input+stylesheet) | High | Apply XSLT stylesheet transformation. Params: `stylesheet` (inline or CAddr reference) |
| 242 | `XmlValidateDtd` | Batch | full-parse | O(input+dtd) | High | Validate XML against DTD. Params: `dtd` |
| 243 | `XmlValidateXsd` | Batch | full-parse | O(input+schema) | High | Validate XML against XSD schema. Params: `schema` |

---

## Category C: Serialization Formats (Avro, Protobuf, Thrift, MessagePack, CBOR, BSON)

Schema-driven binary formats used heavily in distributed systems for RPC, event streaming, and data lake storage. Avro is the HDFS standard; Protobuf dominates gRPC; CBOR is the IPFS/IPLD standard.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 244 | `AvroRead` | Both | streaming-capable | O(block) | High | Parse Avro Object Container File. Streaming: emit one data block per chunk. Embedded schema is extracted and prepended as first chunk |
| 245 | `AvroWrite` | Both | streaming-capable | O(block) | High | Write Avro OCF. Params: `schema` (JSON), `codec` (null/deflate/snappy), `block_size` |
| 246 | `AvroSchemaExtract` | Batch | metadata-only | O(header) | Low | Extract Avro schema from file header. Returns JSON |
| 247 | `AvroSchemaEvolve` | Batch | full-parse | O(input) | High | Re-encode Avro data under a new (compatible) schema. Params: `reader_schema`. Handles field additions, removals, promotions |
| 248 | `AvroToJson` | Both | streaming-capable | O(block) | Medium | Convert Avro records to JSON. Streaming: one JSON object per record |
| 249 | `JsonToAvro` | Both | streaming-capable | O(record) | Medium | Convert JSON objects to Avro. Params: `schema` |
| 250 | `ProtobufDecode` | Both | message-streaming | O(message) | High | Decode Protobuf messages to JSON. Params: `descriptor` (compiled .proto schema). Streaming: length-delimited messages |
| 251 | `ProtobufEncode` | Both | message-streaming | O(message) | High | Encode JSON to Protobuf. Params: `descriptor`, `message_type` |
| 252 | `ProtobufSchemaExtract` | Batch | metadata-only | O(descriptor) | Medium | Extract message definitions from compiled descriptor. Returns JSON |
| 253 | `ThriftDecode` | Batch | full-parse | O(input) | High | Decode Thrift binary/compact protocol to JSON. Params: `protocol` (binary/compact), `struct_definition` |
| 254 | `ThriftEncode` | Batch | full-build | O(input) | High | Encode JSON to Thrift. Params: `protocol`, `struct_definition` |
| 255 | `MsgpackDecode` | Both | streaming-capable | O(value) | Medium | Decode MessagePack to JSON |
| 256 | `MsgpackEncode` | Both | streaming-capable | O(value) | Medium | Encode JSON to MessagePack |
| 257 | `CborDecode` | Both | streaming-capable | O(value) | Medium | Decode CBOR to JSON. Critical for IPFS/IPLD interop (DAG-CBOR) |
| 258 | `CborEncode` | Both | streaming-capable | O(value) | Medium | Encode JSON to CBOR |
| 259 | `BsonDecode` | Both | streaming-capable | O(document) | Medium | Decode BSON to JSON. For MongoDB-origin data in data lakes |
| 260 | `BsonEncode` | Both | streaming-capable | O(document) | Medium | Encode JSON to BSON |
| 261 | `AvroToParquet` | Batch | conversion | O(input) | High | Direct Avro → Parquet conversion with schema mapping. Params: `compression` |
| 262 | `ParquetToAvro` | Batch | conversion | O(input) | High | Direct Parquet → Avro conversion |

---

## Category D: Archive & Container Formats (tar, zip, gzip, bzip2, xz, 7z, rar, zstd-frame)

Archive formats wrap multiple files or apply whole-stream compression. Distributed file systems routinely store/serve these. Streaming is natural for sequential archives (tar, gzip) but not for random-access archives (zip).

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 263 | `TarCreate` | Both | streaming-capable | O(entry) | Medium | Create tar archive from multiple inputs. Streaming: each input becomes one tar entry. Params: `filenames[]`, `permissions[]` |
| 264 | `TarExtract` | Both | streaming-capable | O(entry) | Medium | Extract tar archive. Streaming: emit entries sequentially (header + data). Params: `filter_names[]` (optional, extract specific files) |
| 265 | `TarList` | Both | streaming-capable | O(header) | Low | List tar entries (names, sizes, timestamps) without extracting data. Returns JSON array |
| 266 | `TarAppend` | Batch | multi-input | O(archive+new) | Medium | Append new entries to existing tar archive |
| 267 | `GzipCompress` | Streaming | whole-stream | O(window) | Medium | Whole-stream gzip compression (unlike per-chunk `StreamingCompress`). Maintains zlib dictionary across chunks for better ratio. Params: `level` (1–9) |
| 268 | `GzipDecompress` | Streaming | whole-stream | O(window) | Medium | Whole-stream gzip decompression. Maintains inflater state across chunks |
| 269 | `Bzip2Compress` | Streaming | whole-stream | O(900KB) | Medium | Bzip2 compression. Block size up to 900KB. Params: `block_size` (1–9) |
| 270 | `Bzip2Decompress` | Streaming | whole-stream | O(900KB) | Medium | Bzip2 decompression |
| 271 | `XzCompress` | Streaming | whole-stream | O(dict_size) | Medium | XZ/LZMA2 compression. Best ratio for text. Params: `preset` (0–9), `dict_size` |
| 272 | `XzDecompress` | Streaming | whole-stream | O(dict_size) | Medium | XZ/LZMA2 decompression |
| 273 | `ZipCreate` | Batch | full-build | O(input) | High | Create ZIP archive. Params: `filenames[]`, `compression` (store/deflate/zstd), `password` (optional) |
| 274 | `ZipExtract` | Batch | random-access | O(entry) | High | Extract from ZIP. Requires central directory (random access). Params: `filter_names[]` |
| 275 | `ZipList` | Batch | metadata-only | O(central_dir) | Medium | List ZIP entries from central directory. Returns JSON |
| 276 | `ZipExtractSingle` | Batch | random-access | O(entry) | Medium | Extract single file from ZIP by name. Params: `filename` |
| 277 | `TarGzCreate` | Both | streaming-capable | O(entry+window) | Medium | Create .tar.gz (tar piped through gzip). Combines `TarCreate` + `GzipCompress` |
| 278 | `TarGzExtract` | Both | streaming-capable | O(entry+window) | Medium | Extract .tar.gz. Combines `GzipDecompress` + `TarExtract` |
| 279 | `ZstdFrameCompress` | Streaming | whole-stream | O(window) | Medium | Zstandard frame compression (whole-stream with dictionary). Params: `level`, `dict` (optional trained dictionary CAddr) |
| 280 | `ZstdFrameDecompress` | Streaming | whole-stream | O(window) | Medium | Zstandard frame decompression |
| 281 | `SevenZExtract` | Batch | random-access | O(entry) | High | Extract from 7z archive. Params: `filter_names[]` |
| 282 | `RarExtract` | Batch | random-access | O(entry) | High | Extract from RAR archive (read-only, no creation due to proprietary format) |

---

## Category E: Image Formats (PNG, JPEG, WebP, TIFF, BMP, SVG, HEIF, AVIF, GIF, ICO)

Image processing is a major workload in data lakes (ML training pipelines, CDN origin transforms, medical imaging). Most operations are batch-only due to pixel-level random access, but format detection and metadata extraction can stream.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 283 | `ImageMetadata` | Both | header-parse | O(header) | Medium | Extract EXIF/XMP/IPTC metadata from any image format. Streaming: only reads first chunks. Returns JSON |
| 284 | `ImageResize` | Batch | full-decode | O(pixels) | High | Resize image. Params: `width`, `height`, `filter` (lanczos/bilinear/nearest), `maintain_aspect` |
| 285 | `ImageCrop` | Batch | full-decode | O(pixels) | Medium | Crop to rectangle. Params: `x`, `y`, `width`, `height` |
| 286 | `ImageRotate` | Batch | full-decode | O(pixels) | Medium | Rotate image. Params: `degrees` (90/180/270 or arbitrary) |
| 287 | `ImageConvert` | Batch | full-decode | O(pixels) | High | Convert between formats. Params: `output_format` (png/jpeg/webp/tiff/bmp/avif), `quality` (for lossy) |
| 288 | `ImageThumbnail` | Batch | full-decode | O(pixels) | Medium | Generate thumbnail. Params: `max_dimension`. Optimized path: reads only needed resolution levels for JPEG/TIFF |
| 289 | `ImageStripMetadata` | Both | streaming-capable | O(chunk) | Medium | Remove EXIF/GPS/XMP metadata while preserving image data. Privacy-critical for data lakes |
| 290 | `ImageToGrayscale` | Batch | full-decode | O(pixels) | Medium | Convert to grayscale |
| 291 | `ImageWatermark` | Batch | full-decode | O(pixels) | High | Apply text or image watermark. Params: `text`/`overlay_caddr`, `position`, `opacity` |
| 292 | `ImageHash` | Batch | full-decode | O(pixels) | Medium | Compute perceptual hash (pHash/dHash) for near-duplicate detection. Returns 8-byte hash |
| 293 | `PngOptimize` | Batch | full-decode | O(pixels) | High | Re-encode PNG with optimal compression (filter selection, palette reduction) |
| 294 | `JpegOptimize` | Batch | full-decode | O(pixels) | Medium | Re-encode JPEG with optimal Huffman tables. Params: `quality` |
| 295 | `SvgMinify` | Batch | full-parse | O(input) | Medium | Minify SVG: remove comments, collapse whitespace, shorten attributes |
| 296 | `SvgToPng` | Batch | full-render | O(pixels) | High | Rasterize SVG to PNG. Params: `width`, `height`, `dpi` |
| 297 | `TiffSplit` | Batch | random-access | O(page) | Medium | Split multi-page TIFF into individual pages. Returns concatenated single-page TIFFs |
| 298 | `TiffMerge` | Batch | multi-input | O(sum_pages) | Medium | Merge multiple TIFFs into multi-page TIFF |
| 299 | `GifExtractFrames` | Batch | full-parse | O(frame) | Medium | Extract individual frames from animated GIF. Returns concatenated PNGs with frame metadata |
| 300 | `ImageDetectFormat` | Both | header-parse | O(header) | Low | Detect image format from magic bytes. Returns format name + dimensions. Streaming: reads first chunk only |

---

## Category F: Audio & Video Formats (MP3, WAV, FLAC, OGG, MP4, MKV, WebM, AVI)

Media files are among the largest objects in distributed storage. Metadata extraction and transcoding are key operations. Most are batch-only due to container format complexity, but raw PCM audio and transport streams can be streamed.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 301 | `AudioMetadata` | Both | header-parse | O(header) | Medium | Extract ID3/Vorbis/FLAC metadata (title, artist, album, duration, bitrate). Returns JSON |
| 302 | `AudioConvert` | Batch | full-decode | O(samples) | High | Transcode between audio formats. Params: `output_format` (mp3/wav/flac/ogg/aac), `bitrate`, `sample_rate`, `channels` |
| 303 | `AudioTrim` | Batch | partial-decode | O(segment) | High | Extract time range. Params: `start_ms`, `end_ms` |
| 304 | `AudioNormalize` | Batch | full-decode | O(samples) | High | Normalize audio levels (peak or LUFS normalization). Params: `target_lufs` |
| 305 | `AudioWaveform` | Batch | full-decode | O(samples) | Medium | Generate waveform data (peak values per time bucket). Params: `buckets`. Returns JSON array |
| 306 | `AudioSilenceDetect` | Batch | full-decode | O(samples) | Medium | Detect silence regions. Params: `threshold_db`, `min_duration_ms`. Returns JSON array of `[start_ms, end_ms]` |
| 307 | `AudioStripMetadata` | Batch | selective-copy | O(input) | Medium | Remove all metadata tags while preserving audio data |
| 308 | `VideoMetadata` | Both | header-parse | O(header) | Medium | Extract container metadata (duration, codecs, resolution, fps, bitrate). Returns JSON |
| 309 | `VideoThumbnail` | Batch | partial-decode | O(frame) | High | Extract single frame as image. Params: `timestamp_ms`, `output_format` (png/jpeg) |
| 310 | `VideoExtractAudio` | Batch | demux | O(audio_track) | High | Demux audio track from video container. Params: `track_index`, `output_format` |
| 311 | `VideoStripAudio` | Batch | remux | O(input) | High | Remove audio tracks, keep video only |
| 312 | `VideoResolution` | Batch | full-transcode | O(frames) | Very High | Resize video. Params: `width`, `height`, `codec`, `bitrate`. Requires full transcode |
| 313 | `SubtitleExtract` | Batch | demux | O(subtitle_track) | Medium | Extract subtitle track (SRT/VTT/ASS). Params: `track_index`, `output_format` |
| 314 | `WavToRawPcm` | Both | streaming-capable | O(chunk) | Low | Strip WAV header, emit raw PCM samples. Streaming-capable because PCM is a byte stream |
| 315 | `RawPcmToWav` | Both | streaming-capable | O(chunk) | Low | Prepend WAV header to raw PCM stream. Params: `sample_rate`, `channels`, `bits_per_sample` |
| 316 | `MediaDetectFormat` | Both | header-parse | O(header) | Medium | Detect media container format and codecs from magic bytes + container probing |

---

## Category G: Document Formats (PDF, DOCX, XLSX, PPTX, HTML, Markdown, LaTeX, EPUB)

Document processing is common in enterprise data lakes and content management systems. Most require full parsing due to complex internal structure (ZIP-based Office formats, PDF page tree).

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 317 | `PdfMetadata` | Batch | header-parse | O(xref) | Medium | Extract PDF metadata (title, author, page count, creation date). Returns JSON |
| 318 | `PdfExtractText` | Batch | full-parse | O(input) | High | Extract text content from all pages. Params: `pages[]` (optional, specific pages) |
| 319 | `PdfExtractImages` | Batch | full-parse | O(input) | High | Extract embedded images. Returns concatenated images with offset metadata |
| 320 | `PdfMerge` | Batch | multi-input | O(sum_inputs) | High | Merge multiple PDFs into one. Preserves bookmarks and links |
| 321 | `PdfSplit` | Batch | random-access | O(input) | High | Split PDF by page ranges. Params: `ranges[]` (e.g., `["1-5", "10-15"]`) |
| 322 | `PdfPageCount` | Batch | metadata-only | O(xref) | Low | Return page count without parsing page content |
| 323 | `PdfToText` | Batch | full-parse | O(input) | High | Full PDF-to-plaintext conversion with layout preservation |
| 324 | `DocxExtractText` | Batch | full-parse | O(input) | High | Extract text from DOCX (Office Open XML). Handles paragraphs, tables, headers |
| 325 | `DocxMetadata` | Batch | partial-parse | O(metadata) | Medium | Extract DOCX metadata (author, title, word count, revision). Returns JSON |
| 326 | `XlsxRead` | Batch | full-parse | O(input) | High | Parse XLSX spreadsheet. Params: `sheet_name`/`sheet_index`, `range` (e.g., `A1:D100`). Returns CSV or JSON |
| 327 | `XlsxSheetList` | Batch | partial-parse | O(workbook) | Low | List sheet names and dimensions. Returns JSON |
| 328 | `XlsxToCsv` | Batch | full-parse | O(input) | High | Convert XLSX sheet to CSV. Params: `sheet`, `delimiter` |
| 329 | `PptxExtractText` | Batch | full-parse | O(input) | High | Extract text from PowerPoint slides |
| 330 | `PptxSlideCount` | Batch | partial-parse | O(metadata) | Low | Return slide count |
| 331 | `HtmlToText` | Both | streaming-capable | O(chunk) | Medium | Strip HTML tags, decode entities, extract text. Streaming: tag-aware chunking |
| 332 | `HtmlExtractLinks` | Batch | full-parse | O(input) | Medium | Extract all `<a href>` links. Returns JSON array |
| 333 | `HtmlExtractImages` | Batch | full-parse | O(input) | Medium | Extract all `<img src>` references. Returns JSON array |
| 334 | `HtmlMinify` | Both | streaming-capable | O(chunk) | Medium | Remove comments, collapse whitespace, minify inline CSS/JS |
| 335 | `MarkdownToHtml` | Batch | full-parse | O(input) | Medium | Convert Markdown to HTML. Params: `flavor` (commonmark/gfm) |
| 336 | `HtmlToMarkdown` | Batch | full-parse | O(input) | High | Convert HTML to Markdown |
| 337 | `LatexToText` | Batch | full-parse | O(input) | High | Strip LaTeX commands, extract text content |
| 338 | `EpubExtractText` | Batch | full-parse | O(input) | High | Extract text from EPUB chapters |
| 339 | `EpubMetadata` | Batch | partial-parse | O(metadata) | Medium | Extract EPUB metadata (title, author, chapters). Returns JSON |

---

## Category H: Configuration & Schema Formats (YAML, TOML, INI, .env, HCL, Properties, Plist)

Configuration files are small but ubiquitous. Validation and conversion between formats is the primary use case.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 340 | `YamlParse` | Batch | full-parse | O(input) | Medium | Parse YAML to JSON. Handles multi-document YAML (separated by `---`) |
| 341 | `YamlWrite` | Batch | full-build | O(input) | Medium | Convert JSON to YAML. Params: `indent`, `flow_style` |
| 342 | `YamlValidate` | Batch | full-parse | O(input) | Medium | Validate YAML syntax. Returns input unchanged or error |
| 343 | `YamlMerge` | Batch | multi-input | O(sum_inputs) | Medium | Deep-merge multiple YAML documents |
| 344 | `TomlParse` | Batch | full-parse | O(input) | Medium | Parse TOML to JSON |
| 345 | `TomlWrite` | Batch | full-build | O(input) | Medium | Convert JSON to TOML |
| 346 | `IniParse` | Batch | full-parse | O(input) | Low | Parse INI to JSON (sections become nested objects) |
| 347 | `IniWrite` | Batch | full-build | O(input) | Low | Convert JSON to INI format |
| 348 | `EnvParse` | Batch | full-parse | O(input) | Low | Parse `.env` file to JSON key-value pairs. Handles quoting, comments, multiline |
| 349 | `EnvWrite` | Batch | full-build | O(input) | Low | Convert JSON to `.env` format |
| 350 | `HclParse` | Batch | full-parse | O(input) | High | Parse HashiCorp HCL (Terraform configs) to JSON |
| 351 | `PropertiesParse` | Batch | full-parse | O(input) | Low | Parse Java `.properties` to JSON |
| 352 | `PropertiesWrite` | Batch | full-build | O(input) | Low | Convert JSON to `.properties` format |
| 353 | `PlistParse` | Batch | full-parse | O(input) | Medium | Parse Apple plist (XML or binary) to JSON |
| 354 | `PlistWrite` | Batch | full-build | O(input) | Medium | Convert JSON to plist. Params: `format` (xml/binary) |
| 355 | `ConfigFormatConvert` | Batch | full-parse | O(input) | Medium | Universal config converter. Params: `input_format`, `output_format`. Supports all formats in this category |

---

## Category I: Geospatial Formats (GeoJSON, Shapefile, KML, GeoTIFF, WKT, GPX, FlatGeobuf)

Geospatial data is a growing segment of data lake workloads (satellite imagery, IoT location data, mapping). GeoJSON is streaming-friendly; raster formats (GeoTIFF) require random access.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 356 | `GeoJsonValidate` | Batch | full-parse | O(input) | Medium | Validate GeoJSON structure (RFC 7946). Returns input unchanged or error |
| 357 | `GeoJsonFilter` | Batch | full-parse | O(input) | Medium | Filter features by property predicate. Params: `property`, `operator`, `value` |
| 358 | `GeoJsonBbox` | Batch | full-parse | O(input) | Medium | Compute bounding box of all features. Returns `[minLon, minLat, maxLon, maxLat]` |
| 359 | `GeoJsonSimplify` | Batch | full-parse | O(input) | High | Simplify geometries (Douglas-Peucker). Params: `tolerance`. Reduces file size for visualization |
| 360 | `GeoJsonToWkt` | Batch | full-parse | O(input) | Medium | Convert GeoJSON geometries to WKT (Well-Known Text) |
| 361 | `WktToGeoJson` | Batch | full-parse | O(input) | Medium | Convert WKT to GeoJSON |
| 362 | `ShapefileToGeoJson` | Batch | full-parse | O(input) | High | Convert Shapefile (.shp + .dbf + .shx) to GeoJSON. Multi-input: 3 files |
| 363 | `KmlToGeoJson` | Batch | full-parse | O(input) | High | Convert KML/KMZ to GeoJSON |
| 364 | `GpxToGeoJson` | Batch | full-parse | O(input) | Medium | Convert GPX tracks/waypoints to GeoJSON |
| 365 | `GeoTiffMetadata` | Batch | header-parse | O(header) | Medium | Extract GeoTIFF metadata (CRS, bounds, resolution, bands). Returns JSON |
| 366 | `GeoTiffCrop` | Batch | random-access | O(region) | High | Crop GeoTIFF to bounding box. Params: `bbox` |
| 367 | `GeoTiffResample` | Batch | full-decode | O(pixels) | High | Resample GeoTIFF to different resolution. Params: `target_resolution`, `method` (nearest/bilinear/cubic) |
| 368 | `FlatGeobufRead` | Both | streaming-capable | O(feature) | Medium | Read FlatGeobuf (streaming-optimized geospatial format). Streaming: one feature per chunk |
| 369 | `FlatGeobufWrite` | Both | streaming-capable | O(feature) | Medium | Write FlatGeobuf from GeoJSON features |

---

## Category J: Scientific & Numerical Data Formats (HDF5, NetCDF, FITS, NumPy, Zarr)

Scientific computing generates massive datasets stored in self-describing array formats. These are common in climate science, astronomy, genomics, and ML training data.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 370 | `Hdf5Metadata` | Batch | metadata-only | O(metadata) | Medium | List HDF5 groups, datasets, attributes. Returns JSON hierarchy |
| 371 | `Hdf5Read` | Batch | random-access | O(dataset) | High | Read specific dataset from HDF5. Params: `path` (e.g., `/group1/dataset`), `slice` (e.g., `[0:100, :]`) |
| 372 | `Hdf5Write` | Batch | full-build | O(input) | High | Write data as HDF5 dataset. Params: `path`, `dtype`, `shape`, `compression` |
| 373 | `NetcdfMetadata` | Batch | metadata-only | O(metadata) | Medium | List NetCDF dimensions, variables, attributes. Returns JSON |
| 374 | `NetcdfRead` | Batch | random-access | O(variable) | High | Read specific variable from NetCDF. Params: `variable`, `slice` |
| 375 | `FitsMetadata` | Batch | header-parse | O(header) | Medium | Read FITS header (astronomy). Returns JSON of header cards |
| 376 | `FitsRead` | Batch | random-access | O(hdu) | High | Read specific HDU (Header Data Unit) from FITS. Params: `hdu_index` |
| 377 | `NumpyRead` | Batch | full-parse | O(input) | Medium | Parse .npy/.npz (NumPy array format). Returns raw array bytes with shape/dtype metadata |
| 378 | `NumpyWrite` | Batch | full-build | O(input) | Medium | Write data as .npy format. Params: `dtype`, `shape` |
| 379 | `ZarrRead` | Batch | chunk-access | O(chunk) | High | Read Zarr chunk. Params: `array_path`, `chunk_coords`. Zarr stores arrays as individually addressable chunks — natural fit for CAS |
| 380 | `ZarrMetadata` | Batch | metadata-only | O(metadata) | Medium | Read Zarr `.zarray` and `.zattrs` metadata. Returns JSON |
| 381 | `NumpyToArrow` | Batch | conversion | O(input) | Medium | Convert NumPy array to Arrow tensor |
| 382 | `ArrowToNumpy` | Batch | conversion | O(input) | Medium | Convert Arrow tensor to NumPy array |

---

## Category K: Log & Observability Formats (Syslog, CEF, ELF, W3C, OTLP, Prometheus)

Log processing is one of the highest-volume streaming workloads. Most log formats are line-oriented and naturally streaming.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 383 | `SyslogParse` | Both | line-streaming | O(line) | Medium | Parse RFC 5424 syslog messages to JSON. Streaming: one message per line |
| 384 | `SyslogWrite` | Both | line-streaming | O(line) | Medium | Convert JSON to syslog format |
| 385 | `CefParse` | Both | line-streaming | O(line) | Medium | Parse ArcSight Common Event Format to JSON |
| 386 | `ElfParse` | Both | line-streaming | O(line) | Medium | Parse Extended Log Format (IIS/W3C) to JSON. Handles `#Fields` directive |
| 387 | `ApacheLogParse` | Both | line-streaming | O(line) | Medium | Parse Apache Combined/Common log format to JSON |
| 388 | `NginxLogParse` | Both | line-streaming | O(line) | Medium | Parse Nginx access log to JSON. Params: `log_format` (combined/custom pattern) |
| 389 | `CloudTrailParse` | Batch | full-parse | O(input) | Medium | Parse AWS CloudTrail JSON logs. Extract events, filter by service/action |
| 390 | `VpcFlowLogParse` | Both | line-streaming | O(line) | Medium | Parse AWS VPC Flow Log records to JSON |
| 391 | `PrometheusExpositionParse` | Both | line-streaming | O(line) | Medium | Parse Prometheus exposition format (metrics + labels) to JSON |
| 392 | `OtlpDecode` | Batch | full-parse | O(input) | High | Decode OpenTelemetry Protocol (Protobuf) traces/metrics/logs to JSON |
| 393 | `LogTimestampNormalize` | Both | line-streaming | O(line) | Medium | Normalize timestamps across log formats to ISO 8601. Params: `input_format` (auto-detect or explicit) |
| 394 | `LogLevelFilter` | Both | line-streaming | O(line) | Low | Filter log lines by severity level. Params: `min_level` (debug/info/warn/error/fatal) |
| 395 | `LogAnonymize` | Both | line-streaming | O(line) | Medium | Anonymize IPs, emails, usernames in log lines. Params: `fields[]` to anonymize |

---

## Category L: Blockchain, Content-Addressing & DAG Formats (CID, DAG-PB, DAG-CBOR, CAR, UnixFS)

Directly relevant to our Computation-Addressed Store. These formats are used by IPFS, IPLD, and content-addressed systems for data integrity, linking, and exchange.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 396 | `CidCompute` | Both | accumulator | O(32B) | Medium | Compute CIDv1 (Content Identifier) for input. Params: `codec` (raw/dag-pb/dag-cbor), `hash` (sha2-256/blake3). Returns multibase-encoded CID |
| 397 | `CidVerify` | Both | accumulator | O(32B) | Medium | Verify input matches expected CID. Params: `expected_cid`. Emit error on mismatch |
| 398 | `CidParse` | Batch | metadata-only | O(cid) | Low | Parse CID string into components (version, codec, multihash, digest). Returns JSON |
| 399 | `DagPbEncode` | Batch | full-build | O(input) | High | Encode data + links as DAG-PB (Protobuf DAG node). Params: `links[]` (name, cid, size) |
| 400 | `DagPbDecode` | Batch | full-parse | O(input) | High | Decode DAG-PB node. Returns JSON with `data` (base64) and `links[]` |
| 401 | `DagCborEncode` | Batch | full-build | O(input) | Medium | Encode JSON as DAG-CBOR (CBOR with CID link support). IPLD canonical format |
| 402 | `DagCborDecode` | Batch | full-parse | O(input) | Medium | Decode DAG-CBOR to JSON. CID links become `{"/": "bafy..."}` |
| 403 | `CarCreate` | Both | streaming-capable | O(block) | High | Create CAR (Content Addressable aRchive) file. Streaming: header + blocks sequentially. Params: `roots[]` (CIDs) |
| 404 | `CarExtract` | Both | streaming-capable | O(block) | High | Extract blocks from CAR file. Streaming: emit one block per chunk with CID prefix |
| 405 | `CarList` | Both | streaming-capable | O(header) | Medium | List block CIDs in CAR file without extracting data |
| 406 | `CarVerify` | Both | streaming-capable | O(block) | High | Verify all blocks in CAR match their CIDs. Emit error on first mismatch |
| 407 | `UnixFsChunk` | Both | streaming-capable | O(chunk) | High | Chunk input using UnixFS strategy (fixed-size or content-defined). Emit DAG-PB leaf nodes. Params: `chunk_strategy` (fixed/rabin), `chunk_size` |
| 408 | `UnixFsAssemble` | Batch | full-build | O(tree) | High | Build UnixFS DAG tree from leaf nodes. Returns root DAG-PB node |
| 409 | `MerkleProofGenerate` | Batch | tree-traversal | O(depth×32B) | High | Generate Merkle inclusion proof for a specific leaf. Params: `leaf_index`. Returns proof path |
| 410 | `MerkleProofVerify` | Batch | proof-check | O(depth×32B) | Medium | Verify Merkle inclusion proof. Params: `root_hash`, `leaf_hash`, `proof_path` |

---

## Category M: Database & Storage Formats (SQLite, LevelDB, RocksDB SST, WAL, Postgres COPY)

Database export/import formats encountered when migrating data into/out of distributed file systems.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 411 | `SqliteQuery` | Batch | random-access | O(result_set) | High | Execute SQL query against SQLite database file. Params: `query`. Returns CSV or JSON |
| 412 | `SqliteTableList` | Batch | metadata-only | O(schema) | Medium | List tables and schemas in SQLite database. Returns JSON |
| 413 | `SqliteToCsv` | Batch | full-scan | O(table) | High | Export SQLite table to CSV. Params: `table_name` |
| 414 | `CsvToSqlite` | Batch | full-build | O(input) | High | Import CSV into new SQLite database. Params: `table_name`, `schema` (optional, auto-infer) |
| 415 | `PostgresCopyParse` | Both | line-streaming | O(row) | Medium | Parse PostgreSQL COPY text format to CSV/JSON. Streaming: one row per line |
| 416 | `PostgresCopyWrite` | Both | line-streaming | O(row) | Medium | Convert CSV/JSON to PostgreSQL COPY format |
| 417 | `SstMetadata` | Batch | metadata-only | O(metadata) | High | Read RocksDB/LevelDB SST file metadata (key range, entry count, compression). Returns JSON |
| 418 | `SstScan` | Batch | full-scan | O(entry) | High | Scan SST file entries. Params: `start_key`, `end_key`, `limit`. Returns key-value pairs as JSON |
| 419 | `WalParse` | Both | streaming-capable | O(record) | High | Parse write-ahead log records (generic WAL format). Streaming: one record per chunk |
| 420 | `SqlDumpParse` | Both | line-streaming | O(statement) | Medium | Parse SQL dump (mysqldump/pg_dump) into structured records. Streaming: one INSERT per chunk |

---

## Category N: Machine Learning & Tensor Formats (TFRecord, SafeTensors, ONNX, GGUF, Pickle)

ML model and training data formats are increasingly stored in data lakes. These range from simple tensor serialization to complex model graphs.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 421 | `TfRecordRead` | Both | streaming-capable | O(record) | Medium | Parse TFRecord (TensorFlow). Streaming: one Example per chunk. Includes CRC verification |
| 422 | `TfRecordWrite` | Both | streaming-capable | O(record) | Medium | Write TFRecord format. Streaming: accept serialized Examples |
| 423 | `SafeTensorsRead` | Batch | random-access | O(tensor) | Medium | Read specific tensor from SafeTensors file. Params: `tensor_name`. Header-based offset lookup |
| 424 | `SafeTensorsMetadata` | Batch | header-parse | O(header) | Low | List tensor names, shapes, dtypes from SafeTensors header. Returns JSON |
| 425 | `SafeTensorsWrite` | Batch | full-build | O(input) | Medium | Write tensors in SafeTensors format. Params: `tensors` (name → shape + dtype) |
| 426 | `OnnxMetadata` | Batch | partial-parse | O(metadata) | Medium | Extract ONNX model metadata (inputs, outputs, ops, opset version). Returns JSON |
| 427 | `OnnxValidate` | Batch | full-parse | O(input) | High | Validate ONNX model structure (shape inference, op compatibility) |
| 428 | `GgufMetadata` | Batch | header-parse | O(header) | Medium | Extract GGUF (llama.cpp) model metadata (architecture, quantization, context length). Returns JSON |
| 429 | `PickleToJson` | Batch | full-parse | O(input) | High | Safely deserialize Python pickle to JSON (restricted unpickler — no arbitrary code execution) |
| 430 | `NumpyToSafeTensors` | Batch | conversion | O(input) | Medium | Convert NumPy .npy to SafeTensors format |
| 431 | `TfRecordToParquet` | Batch | conversion | O(input) | High | Convert TFRecord Examples to Parquet (flatten features to columns) |
| 432 | `ImageToTensor` | Batch | full-decode | O(pixels) | Medium | Decode image to raw tensor (CHW or HWC layout). Params: `layout`, `dtype` (float32/uint8), `normalize` |

---

## Category O: Network & Protocol Formats (PCAP, DNS, HTTP archive, Email)

Network capture and protocol data stored in data lakes for security analysis and compliance.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 433 | `PcapRead` | Both | streaming-capable | O(packet) | High | Parse PCAP/PCAPng packet captures. Streaming: one packet per chunk. Returns packet metadata + payload |
| 434 | `PcapFilter` | Both | streaming-capable | O(packet) | High | Filter packets by BPF expression. Params: `filter` (e.g., `tcp port 80`). Streaming: drop non-matching |
| 435 | `PcapStatistics` | Batch | full-scan | O(counters) | Medium | Compute packet statistics (protocol distribution, top talkers, bandwidth). Returns JSON |
| 436 | `DnsRecordParse` | Both | line-streaming | O(record) | Medium | Parse DNS zone file to JSON records |
| 437 | `HarParse` | Batch | full-parse | O(input) | Medium | Parse HTTP Archive (HAR) format. Extract requests, responses, timings. Returns JSON |
| 438 | `EmailParse` | Batch | full-parse | O(input) | High | Parse RFC 5322 email (MIME). Extract headers, body parts, attachments. Returns JSON + attachment bytes |
| 439 | `EmailExtractAttachments` | Batch | full-parse | O(input) | High | Extract MIME attachments from email. Returns concatenated attachments with metadata |
| 440 | `MboxSplit` | Both | streaming-capable | O(message) | Medium | Split mbox file into individual messages. Streaming: one message per chunk |

---

## Category P: Bioinformatics Formats (FASTA, FASTQ, SAM/BAM, VCF, BED, GFF)

Genomics and bioinformatics generate some of the largest datasets in science. Many formats are line-oriented and streaming-friendly.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 441 | `FastaParse` | Both | streaming-capable | O(sequence) | Medium | Parse FASTA sequences. Streaming: one sequence (header + bases) per chunk |
| 442 | `FastqParse` | Both | streaming-capable | O(record) | Medium | Parse FASTQ (sequence + quality scores). Streaming: one 4-line record per chunk |
| 443 | `FastqTrimQuality` | Both | streaming-capable | O(record) | Medium | Trim low-quality bases from FASTQ reads. Params: `min_quality`, `window_size` |
| 444 | `FastqFilter` | Both | streaming-capable | O(record) | Medium | Filter FASTQ reads by length/quality. Params: `min_length`, `min_avg_quality` |
| 445 | `SamParse` | Both | line-streaming | O(record) | High | Parse SAM (Sequence Alignment Map) to JSON. Streaming: one alignment per line |
| 446 | `BamRead` | Batch | random-access | O(record) | Very High | Parse BAM (binary SAM). Requires index for random access. Params: `region` (e.g., `chr1:1000-2000`) |
| 447 | `BamToSam` | Both | streaming-capable | O(record) | High | Convert BAM to SAM text format |
| 448 | `VcfParse` | Both | line-streaming | O(record) | High | Parse VCF (Variant Call Format) to JSON. Streaming: one variant per line |
| 449 | `VcfFilter` | Both | line-streaming | O(record) | High | Filter VCF variants by quality/region/type. Params: `min_qual`, `region`, `variant_type` |
| 450 | `BedParse` | Both | line-streaming | O(record) | Low | Parse BED (genomic intervals) to JSON |
| 451 | `GffParse` | Both | line-streaming | O(record) | Medium | Parse GFF3/GTF (gene annotations) to JSON |
| 452 | `FastaToFastq` | Batch | conversion | O(input) | Low | Convert FASTA to FASTQ with dummy quality scores. Params: `default_quality` |

---

## Category Q: Font, 3D & Specialized Binary Formats (WOFF, OTF, glTF, STL, DICOM, Shapefile)

Specialized formats encountered in specific verticals (web, gaming, medical, GIS) that data lakes must handle.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 453 | `FontMetadata` | Batch | header-parse | O(header) | Medium | Extract font metadata (family, style, weight, glyph count) from OTF/TTF/WOFF/WOFF2. Returns JSON |
| 454 | `WoffToOtf` | Batch | full-decode | O(input) | Medium | Decompress WOFF/WOFF2 to OTF/TTF |
| 455 | `OtfToWoff2` | Batch | full-encode | O(input) | Medium | Compress OTF/TTF to WOFF2 |
| 456 | `GltfMetadata` | Batch | partial-parse | O(metadata) | Medium | Extract glTF 3D model metadata (meshes, materials, animations). Returns JSON |
| 457 | `GltfValidate` | Batch | full-parse | O(input) | High | Validate glTF against specification |
| 458 | `StlRead` | Batch | full-parse | O(input) | Medium | Parse STL (3D printing). Returns vertex/face data as JSON or binary |
| 459 | `StlConvert` | Batch | full-parse | O(input) | Medium | Convert between ASCII and binary STL. Params: `output_format` (ascii/binary) |
| 460 | `DicomMetadata` | Batch | header-parse | O(header) | High | Extract DICOM medical image metadata (patient info redacted, modality, dimensions). Returns JSON |
| 461 | `DicomToImage` | Batch | full-decode | O(pixels) | High | Convert DICOM to standard image format (PNG/TIFF). Params: `output_format`, `window_center`, `window_width` |
| 462 | `DicomAnonymize` | Batch | full-parse | O(input) | High | Remove patient-identifying DICOM tags (HIPAA compliance). Preserves image data |
| 463 | `WasmValidate` | Batch | full-parse | O(input) | High | Validate WebAssembly module structure |
| 464 | `WasmMetadata` | Batch | partial-parse | O(metadata) | Medium | Extract WASM module metadata (imports, exports, memory limits). Returns JSON |
| 465 | `ElfMetadata` | Batch | header-parse | O(header) | Medium | Extract ELF binary metadata (architecture, sections, symbols). Returns JSON |

---

## Category R: Erasure Coding & Redundancy (DFS-specific)

Erasure coding is fundamental to distributed file systems (HDFS-3 EC, Ceph, MinIO). These functions implement the encoding/decoding at the data level.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 466 | `ReedSolomonEncode` | Both | streaming-capable | O(k×chunk) | Very High | Reed-Solomon erasure encode. Params: `data_shards` (k), `parity_shards` (m). Input split into k data shards, m parity shards computed. Streaming: operates on chunk-aligned blocks |
| 467 | `ReedSolomonDecode` | Both | streaming-capable | O(k×chunk) | Very High | Reed-Solomon erasure decode. Multi-input: accepts any k of (k+m) shards. Reconstructs original data |
| 468 | `ReedSolomonVerify` | Both | streaming-capable | O(k×chunk) | High | Verify parity shards are consistent with data shards without full decode |
| 469 | `XorParity` | Both | streaming-capable | O(chunk) | Low | Simple XOR parity across N inputs (RAID-5 style). Multi-input: XOR all inputs byte-by-byte |
| 470 | `XorReconstruct` | Both | streaming-capable | O(chunk) | Low | Reconstruct missing shard from XOR parity + remaining shards |
| 471 | `ReplicationSplit` | Both | streaming-capable | O(chunk) | Low | Duplicate input to N identical outputs (replication factor). Params: `replicas` |
| 472 | `ReplicationVerify` | Batch | multi-input | O(input) | Low | Verify all N replicas are identical. Error on first mismatch |
| 473 | `StripeSplit` | Both | streaming-capable | O(chunk) | Medium | Split input into N equal-sized stripes (HDFS block-level striping). Params: `stripe_count`, `stripe_size` |
| 474 | `StripeAssemble` | Both | streaming-capable | O(chunk) | Medium | Reassemble striped data from N inputs in order |

---

## Category S: Universal Format Detection & Conversion

Meta-functions that operate across all format categories. These are the "Swiss army knife" functions for a DFS that must handle arbitrary file types.

| # | Function | Mode | Pattern | Memory | Complexity | Description |
|---|----------|------|---------|--------|------------|-------------|
| 475 | `FormatDetect` | Both | header-parse | O(header) | Medium | Detect file format from magic bytes + heuristics. Returns JSON: `{format, mime_type, confidence, details}`. Covers all categories A–R |
| 476 | `FormatValidate` | Batch | full-parse | O(input) | High | Validate file against its detected format specification. Auto-detects format, then applies format-specific validator |
| 477 | `UniversalMetadata` | Batch | format-aware | O(header) | High | Extract metadata from any supported format. Auto-detects format, dispatches to format-specific metadata extractor |
| 478 | `UniversalToJson` | Batch | format-aware | O(input) | Very High | Convert any structured format to JSON. Auto-detects format (CSV, Parquet, Avro, XML, YAML, etc.) and converts |
| 479 | `UniversalToText` | Batch | format-aware | O(input) | Very High | Extract text content from any format (PDF, DOCX, HTML, email, etc.). For full-text search indexing |
| 480 | `FormatConvert` | Batch | format-aware | O(input) | Very High | Universal format converter. Params: `output_format`. Routes through appropriate category-specific converter |
| 481 | `SchemaInfer` | Batch | full-scan | O(input) | High | Infer schema from any tabular format (CSV, Parquet, Avro, JSON). Returns unified JSON Schema |
| 482 | `SchemaCompare` | Batch | multi-input | O(schemas) | Medium | Compare two schemas for compatibility (added/removed/changed fields). Returns diff JSON |
| 483 | `MimeTypeMap` | Batch | lookup | O(1) | Low | Map file extension or magic bytes to MIME type. No parsing needed — pure lookup table |

---

## Grand Summary

| Category | Range | Count | Key Formats |
|----------|-------|-------|-------------|
| A: Columnar Data | 201–218 | 18 | Parquet, ORC, Arrow IPC |
| B: Row-Oriented & Delimited | 219–243 | 25 | CSV, TSV, JSON, NDJSON, XML |
| C: Serialization | 244–262 | 19 | Avro, Protobuf, Thrift, MessagePack, CBOR, BSON |
| D: Archive & Container | 263–282 | 20 | tar, zip, gzip, bzip2, xz, zstd-frame, 7z, rar |
| E: Image | 283–300 | 18 | PNG, JPEG, WebP, TIFF, BMP, SVG, HEIF, AVIF, GIF |
| F: Audio & Video | 301–316 | 16 | MP3, WAV, FLAC, OGG, MP4, MKV, WebM |
| G: Document | 317–339 | 23 | PDF, DOCX, XLSX, PPTX, HTML, Markdown, LaTeX, EPUB |
| H: Configuration & Schema | 340–355 | 16 | YAML, TOML, INI, .env, HCL, Properties, Plist |
| I: Geospatial | 356–369 | 14 | GeoJSON, Shapefile, KML, GeoTIFF, GPX, FlatGeobuf |
| J: Scientific & Numerical | 370–382 | 13 | HDF5, NetCDF, FITS, NumPy, Zarr |
| K: Log & Observability | 383–395 | 13 | Syslog, CEF, ELF, Apache/Nginx, CloudTrail, OTLP, Prometheus |
| L: Blockchain & Content-Addressing | 396–410 | 15 | CID, DAG-PB, DAG-CBOR, CAR, UnixFS, Merkle proofs |
| M: Database & Storage | 411–420 | 10 | SQLite, LevelDB/RocksDB SST, WAL, Postgres COPY |
| N: Machine Learning & Tensor | 421–432 | 12 | TFRecord, SafeTensors, ONNX, GGUF, Pickle |
| O: Network & Protocol | 433–440 | 8 | PCAP, DNS, HAR, Email/MIME, mbox |
| P: Bioinformatics | 441–452 | 12 | FASTA, FASTQ, SAM/BAM, VCF, BED, GFF |
| Q: Font, 3D & Specialized Binary | 453–465 | 13 | WOFF, OTF, glTF, STL, DICOM, WASM, ELF |
| R: Erasure Coding & Redundancy | 466–474 | 9 | Reed-Solomon, XOR parity, replication, striping |
| S: Universal Detection & Conversion | 475–483 | 9 | Format detection, universal metadata/conversion, schema inference |
| | | **283** | |

### Complete Library Totals

| Library | Functions | Document |
|---------|-----------|----------|
| Core Streaming (§6) | 100 | `future-improvements-streaming.md` §6 |
| Core Batch (§7) | 100 | `future-improvements-streaming.md` §7 |
| Format-Aware (this document) | 283 | `format-aware-functions.md` |
| **Grand Total** | **483** | |

### Mode Distribution (Format-Aware Functions)

| Mode | Count | Percentage |
|------|-------|------------|
| Batch only | 148 | 52% |
| Both (Batch + Streaming) | 107 | 38% |
| Streaming only | 28 | 10% |

Batch-only dominance reflects the reality that most structured formats (Parquet, PDF, ZIP, HDF5, ONNX) require random access or full parsing. Streaming-capable functions are concentrated in line-oriented formats (CSV, NDJSON, logs), sequential archives (tar, CAR), and byte-level operations (erasure coding, compression).

### Implementation Roadmap

**Phase 1 — High-value, low-complexity (50 functions):**
Categories B (CSV/JSON), D (tar/gzip), H (YAML/TOML/INI), K (log parsing), L (CID/CAR), S (format detection)

**Phase 2 — Analytics backbone (60 functions):**
Categories A (Parquet/ORC/Arrow), C (Avro/Protobuf), R (erasure coding)

**Phase 3 — Rich media & documents (80 functions):**
Categories E (images), F (audio/video), G (documents)

**Phase 4 — Domain-specific (93 functions):**
Categories I (geospatial), J (scientific), M (database), N (ML), O (network), P (bioinformatics), Q (specialized binary)
