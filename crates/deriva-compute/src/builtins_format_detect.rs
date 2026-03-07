//! Category S: Universal Format Detection & Conversion (9 functions, #475–#483)
//! Phase 1 — meta-functions that dispatch to category-specific implementations.

use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Detection helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct FormatResult {
    pub format: String,
    pub mime: String,
    pub category: String,
    pub confidence: f64,
}

/// Magic-byte table: (prefix, format, mime, category)
const MAGIC_TABLE: &[(&[u8], &str, &str, &str)] = &[
    (b"PAR1",              "parquet",   "application/vnd.apache.parquet",       "columnar"),
    (b"ORC",               "orc",       "application/x-orc",                   "columnar"),
    (b"ARROW1",            "arrow_ipc", "application/vnd.apache.arrow.file",   "columnar"),
    (b"Obj\x01",           "avro",      "application/avro",                    "serialization"),
    (b"\x89PNG",           "png",       "image/png",                           "image"),
    (b"\xff\xd8\xff",      "jpeg",      "image/jpeg",                          "image"),
    (b"GIF8",              "gif",       "image/gif",                           "image"),
    (b"RIFF",              "wav",       "audio/wav",                           "audio"),
    (b"fLaC",              "flac",      "audio/flac",                          "audio"),
    (b"ID3",               "mp3",       "audio/mpeg",                          "audio"),
    (b"PK\x03\x04",       "zip",       "application/zip",                     "archive"),
    (b"\x1f\x8b",         "gzip",      "application/gzip",                    "archive"),
    (b"\xfd7zXZ\x00",     "xz",        "application/x-xz",                   "archive"),
    (b"\x28\xb5\x2f\xfd", "zstd",      "application/zstd",                   "archive"),
    (b"BZh",               "bzip2",     "application/x-bzip2",                "archive"),
    (b"%PDF",              "pdf",       "application/pdf",                     "document"),
    (b"SQLite format 3\0","sqlite",    "application/x-sqlite3",              "database"),
    (b"\x00asm",           "wasm",      "application/wasm",                   "binary"),
    (b"\x7fELF",           "elf",       "application/x-elf",                  "binary"),
    (b"\x93NUMPY",         "numpy",     "application/x-numpy",                "scientific"),
    (b"GGUF",              "gguf",      "application/x-gguf",                 "ml"),
];

pub fn detect_format(input: &[u8]) -> FormatResult {
    if input.is_empty() {
        return FormatResult {
            format: "unknown".into(), mime: "application/octet-stream".into(),
            category: "unknown".into(), confidence: 0.0,
        };
    }
    // 1. Magic bytes
    for &(magic, fmt, mime, cat) in MAGIC_TABLE {
        if input.starts_with(magic) {
            return FormatResult {
                format: fmt.into(), mime: mime.into(),
                category: cat.into(), confidence: 0.99,
            };
        }
    }
    // DICOM: magic at offset 128
    if input.len() >= 132 && &input[128..132] == b"DICM" {
        return FormatResult {
            format: "dicom".into(), mime: "application/dicom".into(),
            category: "binary".into(), confidence: 0.99,
        };
    }
    // 2. Structural probe for text formats
    let probe_len = input.len().min(4096);
    if let Ok(text) = std::str::from_utf8(&input[..probe_len]) {
        let t = text.trim_start();
        // HTML before generic XML
        if t.starts_with("<!DOCTYPE html") || t.starts_with("<!doctype html")
            || t.starts_with("<html") || t.starts_with("<HTML")
        {
            return FormatResult {
                format: "html".into(), mime: "text/html".into(),
                category: "document".into(), confidence: 0.90,
            };
        }
        if t.starts_with("<?xml") || t.starts_with('<') && t.contains("</") {
            return FormatResult {
                format: "xml".into(), mime: "application/xml".into(),
                category: "row".into(), confidence: 0.85,
            };
        }
        // TOML: [section] header followed by key = value
        if t.starts_with('[') && !t.starts_with("[[") && t.contains(" = ") {
            if let Some(bracket_end) = t.find(']') {
                let section = &t[1..bracket_end];
                if !section.contains('{') && !section.contains('"') {
                    return FormatResult {
                        format: "toml".into(), mime: "application/toml".into(),
                        category: "config".into(), confidence: 0.80,
                    };
                }
            }
        }
        if t.starts_with('{') || t.starts_with('[') {
            // Distinguish NDJSON from JSON
            if t.starts_with('{') && t.contains('\n') {
                return FormatResult {
                    format: "ndjson".into(), mime: "application/x-ndjson".into(),
                    category: "row".into(), confidence: 0.85,
                };
            }
            return FormatResult {
                format: "json".into(), mime: "application/json".into(),
                category: "row".into(), confidence: 0.90,
            };
        }
        if t.starts_with("---") || (t.contains(":\n") && !t.contains(',')) {
            return FormatResult {
                format: "yaml".into(), mime: "application/x-yaml".into(),
                category: "config".into(), confidence: 0.70,
            };
        }
        // TOML: key = value without section headers
        if t.contains(" = ") && t.lines().next().is_some_and(|l| l.contains('=')) {
            return FormatResult {
                format: "toml".into(), mime: "application/toml".into(),
                category: "config".into(), confidence: 0.65,
            };
        }
        // CSV heuristic: commas + newlines
        if t.contains(',') && t.contains('\n') {
            return FormatResult {
                format: "csv".into(), mime: "text/csv".into(),
                category: "row".into(), confidence: 0.60,
            };
        }
        // TSV heuristic
        if t.contains('\t') && t.contains('\n') {
            return FormatResult {
                format: "tsv".into(), mime: "text/tab-separated-values".into(),
                category: "row".into(), confidence: 0.55,
            };
        }
    }
    FormatResult {
        format: "unknown".into(), mime: "application/octet-stream".into(),
        category: "unknown".into(), confidence: 0.0,
    }
}

fn get_string_param<'a>(params: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a str, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => Ok(s.as_str()),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

fn get_optional_string_param<'a>(params: &'a BTreeMap<String, Value>, name: &str, default: &'a str) -> &'a str {
    match params.get(name) {
        Some(Value::String(s)) => s.as_str(),
        _ => default,
    }
}

// ---------------------------------------------------------------------------
// MIME lookup tables
// ---------------------------------------------------------------------------

const EXT_MIME_TABLE: &[(&str, &str)] = &[
    ("csv",  "text/csv"),
    ("tsv",  "text/tab-separated-values"),
    ("json", "application/json"),
    ("xml",  "application/xml"),
    ("html", "text/html"),
    ("yaml", "application/x-yaml"),
    ("yml",  "application/x-yaml"),
    ("toml", "application/toml"),
    ("ini",  "text/plain"),
    ("pdf",  "application/pdf"),
    ("png",  "image/png"),
    ("jpg",  "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif",  "image/gif"),
    ("svg",  "image/svg+xml"),
    ("mp3",  "audio/mpeg"),
    ("wav",  "audio/wav"),
    ("mp4",  "video/mp4"),
    ("zip",  "application/zip"),
    ("gz",   "application/gzip"),
    ("tar",  "application/x-tar"),
    ("parquet", "application/vnd.apache.parquet"),
    ("avro", "application/avro"),
    ("orc",  "application/x-orc"),
    ("wasm", "application/wasm"),
    ("txt",  "text/plain"),
    ("md",   "text/markdown"),
    ("js",   "application/javascript"),
    ("css",  "text/css"),
    ("py",   "text/x-python"),
    ("rs",   "text/x-rust"),
];

// ---------------------------------------------------------------------------
// #475 FormatDetectFn (Both)
// ---------------------------------------------------------------------------

pub struct FormatDetectFn;

impl ComputeFunction for FormatDetectFn {
    fn id(&self) -> FunctionId { FunctionId::new("format_detect", "1.0.0") }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() {
            return Err(ComputeError::InputCount { expected: 1, got: 0 });
        }
        let result = detect_format(&inputs[0]);
        let json = serde_json::to_string(&result)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?;
        Ok(Bytes::from(json))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 1, memory_bytes: 4096 }
    }
}

// ---------------------------------------------------------------------------
// #476 FormatValidateFn (Batch)
// ---------------------------------------------------------------------------

pub struct FormatValidateFn;

impl ComputeFunction for FormatValidateFn {
    fn id(&self) -> FunctionId { FunctionId::new("format_validate", "1.0.0") }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() {
            return Err(ComputeError::InputCount { expected: 1, got: 0 });
        }
        let expected = get_string_param(params, "format")?;
        let data = &inputs[0];
        let valid = match expected {
            "json" => serde_json::from_slice::<serde_json::Value>(data).is_ok(),
            "csv" => csv::Reader::from_reader(&data[..]).records().next().is_none_or(|r| r.is_ok()),
            "yaml" => serde_yaml::from_slice::<serde_yaml::Value>(data).is_ok(),
            "toml" => std::str::from_utf8(data).ok().and_then(|s| s.parse::<toml::Value>().ok()).is_some(),
            "xml" => std::str::from_utf8(data).ok().is_some_and(|s| {
                quick_xml::Reader::from_str(s).read_event().is_ok()
            }),
            "parquet" => data.len() >= 4 && &data[..4] == b"PAR1",
            "gzip" => data.len() >= 2 && &data[..2] == b"\x1f\x8b",
            "zip" => data.len() >= 4 && &data[..4] == b"PK\x03\x04",
            "png" => data.len() >= 4 && &data[..4] == b"\x89PNG",
            "jpeg" => data.len() >= 3 && &data[..3] == b"\xff\xd8\xff",
            "pdf" => data.len() >= 4 && &data[..4] == b"%PDF",
            other => return Err(ComputeError::InvalidParam(format!("unknown format: {}", other))),
        };
        if valid {
            Ok(inputs.into_iter().next().unwrap())
        } else {
            Err(ComputeError::ExecutionFailed(format!("input is not valid {}", expected)))
        }
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 5, memory_bytes: input_sizes.first().copied().unwrap_or(0) }
    }
}

// ---------------------------------------------------------------------------
// #477 UniversalMetadataFn (Batch)
// ---------------------------------------------------------------------------

pub struct UniversalMetadataFn;

impl ComputeFunction for UniversalMetadataFn {
    fn id(&self) -> FunctionId { FunctionId::new("universal_metadata", "1.0.0") }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() {
            return Err(ComputeError::InputCount { expected: 1, got: 0 });
        }
        let data = &inputs[0];
        let detected = detect_format(data);
        let metadata: serde_json::Value = match detected.format.as_str() {
            "json" => {
                let v: serde_json::Value = serde_json::from_slice(data)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?;
                let kind = if v.is_array() { "array" } else { "object" };
                serde_json::json!({"type": kind, "size_bytes": data.len()})
            }
            "csv" => {
                let mut rdr = csv::Reader::from_reader(&data[..]);
                let headers: Vec<String> = rdr.headers()
                    .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?
                    .iter().map(|h| h.to_string()).collect();
                let row_count = rdr.records().count();
                serde_json::json!({"columns": headers, "row_count": row_count, "size_bytes": data.len()})
            }
            "yaml" => {
                serde_json::json!({"size_bytes": data.len()})
            }
            "toml" => {
                let text = std::str::from_utf8(data)
                    .map_err(|_| ComputeError::ExecutionFailed("toml requires UTF-8".into()))?;
                let val: toml::Value = text.parse()
                    .map_err(|e| ComputeError::ExecutionFailed(format!("toml: {}", e)))?;
                let keys: Vec<String> = match &val {
                    toml::Value::Table(t) => t.keys().cloned().collect(),
                    _ => vec![],
                };
                serde_json::json!({"top_level_keys": keys, "size_bytes": data.len()})
            }
            _ => {
                serde_json::json!({"size_bytes": data.len()})
            }
        };
        let envelope = serde_json::json!({
            "format": detected.format,
            "mime": detected.mime,
            "category": detected.category,
            "confidence": detected.confidence,
            "metadata": metadata,
        });
        let json = serde_json::to_string(&envelope)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?;
        Ok(Bytes::from(json))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 10, memory_bytes: input_sizes.first().copied().unwrap_or(0) * 2 }
    }
}

// ---------------------------------------------------------------------------
// #478 UniversalToJsonFn (Batch)
// ---------------------------------------------------------------------------

pub struct UniversalToJsonFn;

impl ComputeFunction for UniversalToJsonFn {
    fn id(&self) -> FunctionId { FunctionId::new("universal_to_json", "1.0.0") }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() {
            return Err(ComputeError::InputCount { expected: 1, got: 0 });
        }
        let max_records: usize = get_optional_string_param(params, "max_records", "1000")
            .parse().map_err(|_| ComputeError::InvalidParam("max_records must be integer".into()))?;
        let data = &inputs[0];
        let detected = detect_format(data);
        let json_str = match detected.format.as_str() {
            "json" => {
                // Already JSON — pass through (but validate)
                let _: serde_json::Value = serde_json::from_slice(data)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?;
                String::from_utf8_lossy(data).into_owned()
            }
            "ndjson" => {
                let text = std::str::from_utf8(data)
                    .map_err(|_| ComputeError::ExecutionFailed("ndjson requires UTF-8".into()))?;
                let records: Vec<serde_json::Value> = text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .take(max_records)
                    .map(serde_json::from_str)
                    .collect::<Result<_, _>>()
                    .map_err(|e| ComputeError::ExecutionFailed(format!("ndjson: {}", e)))?;
                serde_json::to_string(&records)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?
            }
            "csv" | "tsv" => {
                let mut rdr = csv::ReaderBuilder::new()
                    .delimiter(if detected.format == "tsv" { b'\t' } else { b',' })
                    .from_reader(&data[..]);
                let headers: Vec<String> = rdr.headers()
                    .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?
                    .iter().map(|h| h.to_string()).collect();
                let mut records = Vec::new();
                for result in rdr.records().take(max_records) {
                    let record = result.map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
                    let mut obj = serde_json::Map::new();
                    for (i, field) in record.iter().enumerate() {
                        let key = headers.get(i).map(|s| s.as_str()).unwrap_or("_unknown");
                        obj.insert(key.to_string(), serde_json::Value::String(field.to_string()));
                    }
                    records.push(serde_json::Value::Object(obj));
                }
                serde_json::to_string(&records)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?
            }
            "yaml" => {
                let val: serde_json::Value = serde_yaml::from_slice(data)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("yaml: {}", e)))?;
                serde_json::to_string(&val)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?
            }
            "toml" => {
                let text = std::str::from_utf8(data)
                    .map_err(|_| ComputeError::ExecutionFailed("toml requires UTF-8".into()))?;
                let val: toml::Value = text.parse()
                    .map_err(|e| ComputeError::ExecutionFailed(format!("toml: {}", e)))?;
                serde_json::to_string(&val)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?
            }
            other => {
                // Binary/unknown: summary
                serde_json::to_string(&serde_json::json!({
                    "format": other,
                    "summary": format!("{} bytes of {} data", data.len(), other),
                })).map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?
            }
        };
        Ok(Bytes::from(json_str))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 20, memory_bytes: input_sizes.first().copied().unwrap_or(0) * 3 }
    }
}

// ---------------------------------------------------------------------------
// #479 UniversalToTextFn (Batch)
// ---------------------------------------------------------------------------

pub struct UniversalToTextFn;

impl ComputeFunction for UniversalToTextFn {
    fn id(&self) -> FunctionId { FunctionId::new("universal_to_text", "1.0.0") }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() {
            return Err(ComputeError::InputCount { expected: 1, got: 0 });
        }
        let data = &inputs[0];
        let detected = detect_format(data);
        let text = match detected.format.as_str() {
            "json" | "ndjson" | "yaml" | "toml" | "xml" | "html" | "csv" | "tsv" => {
                // Text-based formats: return as-is (already UTF-8)
                String::from_utf8_lossy(data).into_owned()
            }
            _ => {
                // Binary formats: no text content
                String::new()
            }
        };
        Ok(Bytes::from(text))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 5, memory_bytes: input_sizes.first().copied().unwrap_or(0) }
    }
}

// ---------------------------------------------------------------------------
// #480 FormatConvertFn (Batch)
// ---------------------------------------------------------------------------

pub struct FormatConvertFn;

impl ComputeFunction for FormatConvertFn {
    fn id(&self) -> FunctionId { FunctionId::new("format_convert", "1.0.0") }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() {
            return Err(ComputeError::InputCount { expected: 1, got: 0 });
        }
        let data = &inputs[0];
        let from = match params.get("from") {
            Some(Value::String(s)) => s.clone(),
            _ => detect_format(data).format,
        };
        let to = get_string_param(params, "to")?;

        // Convertibility classes
        let tabular = &["csv", "tsv", "json", "ndjson", "yaml", "toml"];
        let config = &["yaml", "toml", "json"];

        let from_s = from.as_str();
        if !tabular.contains(&from_s) && !config.contains(&from_s) {
            return Err(ComputeError::InvalidParam(format!("no conversion path from {} to {}", from, to)));
        }
        if !tabular.contains(&to) && !config.contains(&to) {
            return Err(ComputeError::InvalidParam(format!("no conversion path from {} to {}", from, to)));
        }

        // Step 1: parse source to intermediate JSON value
        let intermediate: serde_json::Value = match from_s {
            "json" => serde_json::from_slice(data)
                .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?,
            "ndjson" => {
                let text = std::str::from_utf8(data)
                    .map_err(|_| ComputeError::ExecutionFailed("ndjson requires UTF-8".into()))?;
                let records: Vec<serde_json::Value> = text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(serde_json::from_str)
                    .collect::<Result<_, _>>()
                    .map_err(|e| ComputeError::ExecutionFailed(format!("ndjson: {}", e)))?;
                serde_json::Value::Array(records)
            }
            "yaml" => serde_yaml::from_slice(data)
                .map_err(|e| ComputeError::ExecutionFailed(format!("yaml: {}", e)))?,
            "toml" => {
                let text = std::str::from_utf8(data)
                    .map_err(|_| ComputeError::ExecutionFailed("toml requires UTF-8".into()))?;
                let val: toml::Value = text.parse()
                    .map_err(|e| ComputeError::ExecutionFailed(format!("toml: {}", e)))?;
                serde_json::to_value(val)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("toml->json: {}", e)))?
            }
            "csv" | "tsv" => {
                let delim = if from_s == "tsv" { b'\t' } else { b',' };
                let mut rdr = csv::ReaderBuilder::new().delimiter(delim).from_reader(&data[..]);
                let headers: Vec<String> = rdr.headers()
                    .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?
                    .iter().map(|h| h.to_string()).collect();
                let mut records = Vec::new();
                for result in rdr.records() {
                    let record = result.map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
                    let mut obj = serde_json::Map::new();
                    for (i, field) in record.iter().enumerate() {
                        let key = headers.get(i).map(|s| s.as_str()).unwrap_or("_unknown");
                        obj.insert(key.to_string(), serde_json::Value::String(field.to_string()));
                    }
                    records.push(serde_json::Value::Object(obj));
                }
                serde_json::Value::Array(records)
            }
            _ => return Err(ComputeError::InvalidParam(format!("no conversion path from {} to {}", from, to))),
        };

        // Step 2: serialize to target format
        let output = match to {
            "json" => serde_json::to_string_pretty(&intermediate)
                .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?,
            "ndjson" => {
                match &intermediate {
                    serde_json::Value::Array(arr) => arr.iter()
                        .map(serde_json::to_string)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| ComputeError::ExecutionFailed(format!("ndjson: {}", e)))?
                        .join("\n"),
                    _ => serde_json::to_string(&intermediate)
                        .map_err(|e| ComputeError::ExecutionFailed(format!("ndjson: {}", e)))?,
                }
            }
            "yaml" => serde_yaml::to_string(&intermediate)
                .map_err(|e| ComputeError::ExecutionFailed(format!("yaml: {}", e)))?,
            "toml" => {
                // toml requires a table at top level
                let toml_val: toml::Value = serde_json::from_value(intermediate.clone())
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json->toml: {}", e)))?;
                toml::to_string_pretty(&toml_val)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("toml: {}", e)))?
            }
            "csv" | "tsv" => {
                let delim = if to == "tsv" { b'\t' } else { b',' };
                let arr = intermediate.as_array()
                    .ok_or_else(|| ComputeError::ExecutionFailed("csv output requires array of objects".into()))?;
                if arr.is_empty() {
                    return Ok(Bytes::new());
                }
                // Collect headers from first object
                let headers: Vec<String> = arr[0].as_object()
                    .ok_or_else(|| ComputeError::ExecutionFailed("csv rows must be objects".into()))?
                    .keys().cloned().collect();
                let mut wtr = csv::WriterBuilder::new().delimiter(delim).from_writer(Vec::new());
                wtr.write_record(&headers)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
                for row in arr {
                    let obj = row.as_object()
                        .ok_or_else(|| ComputeError::ExecutionFailed("csv rows must be objects".into()))?;
                    let fields: Vec<String> = headers.iter()
                        .map(|h| match obj.get(h) {
                            Some(serde_json::Value::String(s)) => s.clone(),
                            Some(v) => v.to_string(),
                            None => String::new(),
                        }).collect();
                    wtr.write_record(&fields)
                        .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
                }
                String::from_utf8(wtr.into_inner()
                    .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("utf8: {}", e)))?
            }
            _ => return Err(ComputeError::InvalidParam(format!("no conversion path from {} to {}", from, to))),
        };
        Ok(Bytes::from(output))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 30, memory_bytes: input_sizes.first().copied().unwrap_or(0) * 4 }
    }
}

// ---------------------------------------------------------------------------
// #481 SchemaInferFn (Batch)
// ---------------------------------------------------------------------------

pub struct SchemaInferFn;

impl ComputeFunction for SchemaInferFn {
    fn id(&self) -> FunctionId { FunctionId::new("schema_infer", "1.0.0") }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() {
            return Err(ComputeError::InputCount { expected: 1, got: 0 });
        }
        let sample_rows: usize = get_optional_string_param(params, "sample_rows", "1000")
            .parse().map_err(|_| ComputeError::InvalidParam("sample_rows must be integer".into()))?;
        let data = &inputs[0];
        let detected = detect_format(data);

        match detected.format.as_str() {
            "csv" | "tsv" => {
                let delim = if detected.format == "tsv" { b'\t' } else { b',' };
                let mut rdr = csv::ReaderBuilder::new().delimiter(delim).from_reader(&data[..]);
                let headers: Vec<String> = rdr.headers()
                    .map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?
                    .iter().map(|h| h.to_string()).collect();
                // Collect sample values per column
                let ncols = headers.len();
                let mut col_values: Vec<Vec<String>> = vec![Vec::new(); ncols];
                for result in rdr.records().take(sample_rows) {
                    let record = result.map_err(|e| ComputeError::ExecutionFailed(format!("csv: {}", e)))?;
                    for (i, field) in record.iter().enumerate() {
                        if i < ncols {
                            col_values[i].push(field.to_string());
                        }
                    }
                }
                let mut properties = serde_json::Map::new();
                for (i, header) in headers.iter().enumerate() {
                    let inferred = infer_column_type(&col_values[i]);
                    properties.insert(header.clone(), serde_json::json!({"type": inferred}));
                }
                let schema = serde_json::json!({
                    "type": "object",
                    "properties": properties,
                });
                Ok(Bytes::from(serde_json::to_string_pretty(&schema)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?))
            }
            "json" | "ndjson" => {
                // Parse and infer from first object
                let val: serde_json::Value = serde_json::from_slice(data)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?;
                let sample = match &val {
                    serde_json::Value::Array(arr) => arr.first(),
                    obj @ serde_json::Value::Object(_) => Some(obj),
                    _ => None,
                };
                let schema = match sample {
                    Some(serde_json::Value::Object(obj)) => {
                        let mut properties = serde_json::Map::new();
                        for (key, value) in obj {
                            let t = match value {
                                serde_json::Value::Bool(_) => "boolean",
                                serde_json::Value::Number(n) if n.is_f64() => "number",
                                serde_json::Value::Number(_) => "integer",
                                serde_json::Value::String(_) => "string",
                                serde_json::Value::Array(_) => "array",
                                serde_json::Value::Object(_) => "object",
                                serde_json::Value::Null => "string",
                            };
                            properties.insert(key.clone(), serde_json::json!({"type": t}));
                        }
                        serde_json::json!({"type": "object", "properties": properties})
                    }
                    _ => serde_json::json!({"type": "object", "properties": {}}),
                };
                Ok(Bytes::from(serde_json::to_string_pretty(&schema)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?))
            }
            other => Err(ComputeError::InvalidParam(format!(
                "schema inference requires tabular format, got {}", other
            ))),
        }
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 15, memory_bytes: input_sizes.first().copied().unwrap_or(0) * 2 }
    }
}

fn infer_column_type(values: &[String]) -> &'static str {
    if values.is_empty() { return "string"; }
    let non_empty: Vec<&str> = values.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect();
    if non_empty.is_empty() { return "string"; } // all-null
    if non_empty.iter().all(|v| v.parse::<i64>().is_ok()) { return "integer"; }
    if non_empty.iter().all(|v| v.parse::<f64>().is_ok()) { return "number"; }
    if non_empty.iter().all(|v| *v == "true" || *v == "false") { return "boolean"; }
    "string"
}

// ---------------------------------------------------------------------------
// #482 SchemaCompareFn (Batch)
// ---------------------------------------------------------------------------

pub struct SchemaCompareFn;

impl ComputeFunction for SchemaCompareFn {
    fn id(&self) -> FunctionId { FunctionId::new("schema_compare", "1.0.0") }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 {
            return Err(ComputeError::InputCount { expected: 2, got: inputs.len() });
        }
        let schema1: serde_json::Value = serde_json::from_slice(&inputs[0])
            .map_err(|e| ComputeError::ExecutionFailed(format!("schema1: {}", e)))?;
        let schema2: serde_json::Value = serde_json::from_slice(&inputs[1])
            .map_err(|e| ComputeError::ExecutionFailed(format!("schema2: {}", e)))?;

        let props1 = schema1.get("properties").and_then(|p| p.as_object());
        let props2 = schema2.get("properties").and_then(|p| p.as_object());

        let (p1, p2) = match (props1, props2) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                let compatible = schema1 == schema2;
                let result = serde_json::json!({"compatible": compatible, "changes": []});
                return Ok(Bytes::from(serde_json::to_string(&result).unwrap()));
            }
        };

        let mut changes = Vec::new();
        // Fields in schema2 not in schema1 → added
        for key in p2.keys() {
            if !p1.contains_key(key) {
                changes.push(serde_json::json!({"field": key, "type": "added", "detail": format!("new field: {}", key)}));
            }
        }
        // Fields in schema1 not in schema2 → removed
        for key in p1.keys() {
            if !p2.contains_key(key) {
                changes.push(serde_json::json!({"field": key, "type": "removed", "detail": format!("removed field: {}", key)}));
            }
        }
        // Fields in both but different type → changed
        for (key, v1) in p1 {
            if let Some(v2) = p2.get(key) {
                if v1 != v2 {
                    changes.push(serde_json::json!({
                        "field": key, "type": "changed",
                        "detail": format!("{} -> {}", v1, v2),
                    }));
                }
            }
        }
        let compatible = changes.is_empty();
        let result = serde_json::json!({"compatible": compatible, "changes": changes});
        Ok(Bytes::from(serde_json::to_string(&result)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 5, memory_bytes: 8192 }
    }
}

// ---------------------------------------------------------------------------
// #483 MimeTypeMapFn (Batch)
// ---------------------------------------------------------------------------

pub struct MimeTypeMapFn;

impl ComputeFunction for MimeTypeMapFn {
    fn id(&self) -> FunctionId { FunctionId::new("mime_type_map", "1.0.0") }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() {
            return Err(ComputeError::InputCount { expected: 1, got: 0 });
        }
        let direction = get_optional_string_param(params, "direction", "ext_to_mime");
        let input_str = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("input must be UTF-8".into()))?
            .trim();

        let result = match direction {
            "ext_to_mime" => {
                let ext = input_str.trim_start_matches('.');
                let mime = EXT_MIME_TABLE.iter()
                    .find(|(e, _)| *e == ext)
                    .map(|(_, m)| *m)
                    .unwrap_or("application/octet-stream");
                serde_json::json!({"mime": mime})
            }
            "mime_to_ext" => {
                let exts: Vec<&str> = EXT_MIME_TABLE.iter()
                    .filter(|(_, m)| *m == input_str)
                    .map(|(e, _)| *e)
                    .collect();
                serde_json::json!({"extensions": exts})
            }
            _ => return Err(ComputeError::InvalidParam(
                "direction must be 'ext_to_mime' or 'mime_to_ext'".into()
            )),
        };
        Ok(Bytes::from(serde_json::to_string(&result)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 1, memory_bytes: 1024 }
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

use crate::registry::FunctionRegistry;
use std::sync::Arc;

pub fn register_detect_functions(registry: &mut FunctionRegistry) {
    registry.register(Arc::new(FormatDetectFn));
    registry.register(Arc::new(FormatValidateFn));
    registry.register(Arc::new(UniversalMetadataFn));
    registry.register(Arc::new(UniversalToJsonFn));
    registry.register(Arc::new(UniversalToTextFn));
    registry.register(Arc::new(FormatConvertFn));
    registry.register(Arc::new(SchemaInferFn));
    registry.register(Arc::new(SchemaCompareFn));
    registry.register(Arc::new(MimeTypeMapFn));
}
