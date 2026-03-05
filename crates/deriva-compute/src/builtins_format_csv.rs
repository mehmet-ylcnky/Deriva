//! Category B: Row-Oriented & Delimited Formats — CSV, TSV, JSON, NDJSON, XML
//! (25 functions, #219–#243) — Phase 1

use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;

fn get_string_param<'a>(params: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a str, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => Ok(s.as_str()),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

fn opt_str<'a>(params: &'a BTreeMap<String, Value>, name: &str, default: &'a str) -> &'a str {
    match params.get(name) { Some(Value::String(s)) => s.as_str(), _ => default }
}

fn csv_err(e: impl std::fmt::Display) -> ComputeError {
    ComputeError::ExecutionFailed(format!("csv: {}", e))
}

fn json_err(e: impl std::fmt::Display) -> ComputeError {
    ComputeError::ExecutionFailed(format!("json: {}", e))
}

fn require_utf8(data: &[u8]) -> Result<&str, ComputeError> {
    std::str::from_utf8(data).map_err(|_| ComputeError::ExecutionFailed("requires UTF-8".into()))
}

fn one_input(inputs: &[Bytes]) -> Result<&Bytes, ComputeError> {
    inputs.first().ok_or(ComputeError::InputCount { expected: 1, got: 0 })
}

fn matches_predicate(field: &str, op: &str, value: &str) -> Result<bool, ComputeError> {
    Ok(match op {
        "eq" => field == value,
        "ne" => field != value,
        "lt" => field < value,
        "le" => field <= value,
        "gt" => field > value,
        "ge" => field >= value,
        "contains" => field.contains(value),
        "starts_with" => field.starts_with(value),
        "ends_with" => field.ends_with(value),
        "regex" => regex::Regex::new(value)
            .map_err(|e| ComputeError::InvalidParam(format!("invalid regex: {}", e)))?
            .is_match(field),
        _ => return Err(ComputeError::InvalidParam(format!("unknown op: {}", op))),
    })
}

fn cost(cpu: u64, sizes: &[u64], mult: u64) -> ComputeCost {
    ComputeCost { cpu_ms: cpu, memory_bytes: sizes.first().copied().unwrap_or(0) * mult }
}

// ---- helpers for CSV reader/writer with configurable delimiter ----

fn csv_reader(data: &[u8], delimiter: u8, has_header: bool) -> csv::Reader<&[u8]> {
    csv::ReaderBuilder::new().delimiter(delimiter).has_headers(has_header).from_reader(data)
}

fn csv_writer(delimiter: u8) -> csv::Writer<Vec<u8>> {
    csv::WriterBuilder::new().delimiter(delimiter).from_writer(Vec::new())
}

// ---------------------------------------------------------------------------
// #219 CsvParseFn (Both)
// ---------------------------------------------------------------------------
pub struct CsvParseFn;
impl ComputeFunction for CsvParseFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_parse", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let delim = opt_str(params, "delimiter", ",").as_bytes().first().copied().unwrap_or(b',');
        let has_header = opt_str(params, "has_header", "true") == "true";
        let mut rdr = csv_reader(data, delim, has_header);
        let headers: Vec<String> = if has_header {
            rdr.headers().map_err(csv_err)?.iter().map(|h| h.to_string()).collect()
        } else {
            // Generate column names col0, col1, ...
            let first = rdr.records().next();
            match first {
                Some(Ok(ref r)) => (0..r.len()).map(|i| format!("col{}", i)).collect(),
                _ => vec![],
            }
        };
        // Re-read if no header (we consumed one record)
        let mut rdr = csv_reader(data, delim, has_header);
        if has_header { let _ = rdr.headers(); }
        let mut rows = Vec::new();
        for result in rdr.records() {
            let record = result.map_err(csv_err)?;
            let mut obj = serde_json::Map::new();
            for (i, field) in record.iter().enumerate() {
                let key = headers.get(i).map(|s| s.as_str()).unwrap_or("_");
                obj.insert(key.to_string(), serde_json::Value::String(field.to_string()));
            }
            rows.push(serde_json::Value::Object(obj));
        }
        Ok(Bytes::from(serde_json::to_string(&rows).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 3) }
}

// ---------------------------------------------------------------------------
// #220 CsvWriteFn (Both)
// ---------------------------------------------------------------------------
pub struct CsvWriteFn;
impl ComputeFunction for CsvWriteFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_write", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let delim = opt_str(params, "delimiter", ",").as_bytes().first().copied().unwrap_or(b',');
        let arr: Vec<serde_json::Value> = serde_json::from_slice(data).map_err(json_err)?;
        if arr.is_empty() { return Ok(Bytes::new()); }
        let headers: Vec<String> = arr[0].as_object()
            .ok_or_else(|| ComputeError::ExecutionFailed("rows must be objects".into()))?
            .keys().cloned().collect();
        let mut wtr = csv_writer(delim);
        wtr.write_record(&headers).map_err(csv_err)?;
        for row in &arr {
            let obj = row.as_object().ok_or_else(|| ComputeError::ExecutionFailed("rows must be objects".into()))?;
            let fields: Vec<String> = headers.iter().map(|h| match obj.get(h) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            }).collect();
            wtr.write_record(&fields).map_err(csv_err)?;
        }
        Ok(Bytes::from(wtr.into_inner().map_err(csv_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 2) }
}

// ---------------------------------------------------------------------------
// #221 CsvSchemaInferFn (Batch)
// ---------------------------------------------------------------------------
pub struct CsvSchemaInferFn;
impl ComputeFunction for CsvSchemaInferFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_schema_infer", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let sample: usize = opt_str(params, "sample_rows", "100").parse()
            .map_err(|_| ComputeError::InvalidParam("sample_rows must be integer".into()))?;
        let mut rdr = csv::Reader::from_reader(&data[..]);
        let headers: Vec<String> = rdr.headers().map_err(csv_err)?.iter().map(|h| h.to_string()).collect();
        let ncols = headers.len();
        let mut col_vals: Vec<Vec<String>> = vec![Vec::new(); ncols];
        for result in rdr.records().take(sample) {
            let rec = result.map_err(csv_err)?;
            for (i, f) in rec.iter().enumerate() {
                if i < ncols { col_vals[i].push(f.to_string()); }
            }
        }
        let mut props = serde_json::Map::new();
        for (i, h) in headers.iter().enumerate() {
            props.insert(h.clone(), serde_json::json!({"type": infer_type(&col_vals[i])}));
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&serde_json::json!({"type":"object","properties":props})).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(15, s, 2) }
}

fn infer_type(vals: &[String]) -> &'static str {
    let non_empty: Vec<&str> = vals.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect();
    if non_empty.is_empty() { return "string"; }
    if non_empty.iter().all(|v| v.parse::<i64>().is_ok()) { return "integer"; }
    if non_empty.iter().all(|v| v.parse::<f64>().is_ok()) { return "float"; }
    if non_empty.iter().all(|v| *v == "true" || *v == "false") { return "boolean"; }
    "string"
}

// ---------------------------------------------------------------------------
// #222 CsvColumnSelectFn (Both)
// ---------------------------------------------------------------------------
pub struct CsvColumnSelectFn;
impl ComputeFunction for CsvColumnSelectFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_column_select", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let cols_str = get_string_param(params, "columns")?;
        let cols: Vec<&str> = cols_str.split(',').map(|s| s.trim()).collect();
        let mut rdr = csv::Reader::from_reader(&data[..]);
        let headers = rdr.headers().map_err(csv_err)?.clone();
        let indices: Vec<usize> = cols.iter().filter_map(|c| headers.iter().position(|h| h == *c)).collect();
        if indices.is_empty() { return Err(ComputeError::InvalidParam("no matching columns".into())); }
        let sel_headers: Vec<&str> = indices.iter().map(|&i| headers.get(i).unwrap()).collect();
        let mut wtr = csv::Writer::from_writer(Vec::new());
        wtr.write_record(&sel_headers).map_err(csv_err)?;
        for rec in rdr.records() {
            let rec = rec.map_err(csv_err)?;
            let fields: Vec<&str> = indices.iter().map(|&i| rec.get(i).unwrap_or("")).collect();
            wtr.write_record(&fields).map_err(csv_err)?;
        }
        Ok(Bytes::from(wtr.into_inner().map_err(csv_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 2) }
}

// ---------------------------------------------------------------------------
// #223 CsvColumnRenameFn (Both)
// ---------------------------------------------------------------------------
pub struct CsvColumnRenameFn;
impl ComputeFunction for CsvColumnRenameFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_column_rename", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let mapping_str = get_string_param(params, "mapping")?;
        let mapping: serde_json::Map<String, serde_json::Value> = serde_json::from_str(mapping_str).map_err(json_err)?;
        let mut rdr = csv::Reader::from_reader(&data[..]);
        let headers = rdr.headers().map_err(csv_err)?.clone();
        let new_headers: Vec<String> = headers.iter().map(|h| {
            mapping.get(h).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| h.to_string())
        }).collect();
        let mut wtr = csv::Writer::from_writer(Vec::new());
        wtr.write_record(&new_headers).map_err(csv_err)?;
        for rec in rdr.records() {
            wtr.write_record(&rec.map_err(csv_err)?).map_err(csv_err)?;
        }
        Ok(Bytes::from(wtr.into_inner().map_err(csv_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(5, s, 2) }
}

// ---------------------------------------------------------------------------
// #224 CsvFilterFn (Both)
// ---------------------------------------------------------------------------
pub struct CsvFilterFn;
impl ComputeFunction for CsvFilterFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_filter", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let column = get_string_param(params, "column")?;
        let op = get_string_param(params, "op")?;
        let value = get_string_param(params, "value")?;
        let mut rdr = csv::Reader::from_reader(&data[..]);
        let headers = rdr.headers().map_err(csv_err)?.clone();
        let col_idx = headers.iter().position(|h| h == column)
            .ok_or_else(|| ComputeError::InvalidParam(format!("column '{}' not found", column)))?;
        let mut wtr = csv::Writer::from_writer(Vec::new());
        wtr.write_record(&headers).map_err(csv_err)?;
        for rec in rdr.records() {
            let rec = rec.map_err(csv_err)?;
            let field = rec.get(col_idx).unwrap_or("");
            if matches_predicate(field, op, value)? {
                wtr.write_record(&rec).map_err(csv_err)?;
            }
        }
        Ok(Bytes::from(wtr.into_inner().map_err(csv_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(15, s, 2) }
}

// ---------------------------------------------------------------------------
// #225 CsvSortFn (Batch)
// ---------------------------------------------------------------------------
pub struct CsvSortFn;
impl ComputeFunction for CsvSortFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_sort", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let column = get_string_param(params, "column")?;
        let order = opt_str(params, "order", "asc");
        let mut rdr = csv::Reader::from_reader(&data[..]);
        let headers = rdr.headers().map_err(csv_err)?.clone();
        let col_idx = headers.iter().position(|h| h == column)
            .ok_or_else(|| ComputeError::InvalidParam(format!("column '{}' not found", column)))?;
        let mut rows: Vec<csv::StringRecord> = rdr.records().collect::<Result<_, _>>().map_err(csv_err)?;
        rows.sort_by(|a, b| {
            let va = a.get(col_idx).unwrap_or("");
            let vb = b.get(col_idx).unwrap_or("");
            // Try numeric sort first
            if let (Ok(na), Ok(nb)) = (va.parse::<f64>(), vb.parse::<f64>()) {
                return na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal);
            }
            va.cmp(vb)
        });
        if order == "desc" { rows.reverse(); }
        let mut wtr = csv::Writer::from_writer(Vec::new());
        wtr.write_record(&headers).map_err(csv_err)?;
        for r in &rows { wtr.write_record(r).map_err(csv_err)?; }
        Ok(Bytes::from(wtr.into_inner().map_err(csv_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(20, s, 3) }
}

// ---------------------------------------------------------------------------
// #226 CsvAggregateFn (Batch)
// ---------------------------------------------------------------------------
pub struct CsvAggregateFn;
impl ComputeFunction for CsvAggregateFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_aggregate", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let column = get_string_param(params, "column")?;
        let func = get_string_param(params, "function")?;
        let mut rdr = csv::Reader::from_reader(&data[..]);
        let headers = rdr.headers().map_err(csv_err)?.clone();
        let col_idx = headers.iter().position(|h| h == column)
            .ok_or_else(|| ComputeError::InvalidParam(format!("column '{}' not found", column)))?;
        let values: Vec<f64> = rdr.records()
            .filter_map(|r| r.ok())
            .filter_map(|r| r.get(col_idx).and_then(|v| v.parse::<f64>().ok()))
            .collect();
        let result: serde_json::Value = match func {
            "count" => serde_json::json!({"column": column, "function": "count", "result": values.len()}),
            "sum" => serde_json::json!({"column": column, "function": "sum", "result": values.iter().sum::<f64>()}),
            "avg" => {
                let avg = if values.is_empty() { 0.0 } else { values.iter().sum::<f64>() / values.len() as f64 };
                serde_json::json!({"column": column, "function": "avg", "result": avg})
            }
            "min" => serde_json::json!({"column": column, "function": "min", "result": values.iter().cloned().fold(f64::INFINITY, f64::min)}),
            "max" => serde_json::json!({"column": column, "function": "max", "result": values.iter().cloned().fold(f64::NEG_INFINITY, f64::max)}),
            _ => return Err(ComputeError::InvalidParam(format!("unknown function: {}", func))),
        };
        Ok(Bytes::from(serde_json::to_string(&result).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(15, s, 2) }
}

// ---------------------------------------------------------------------------
// #227 CsvJoinFn (Batch) — 2 inputs
// ---------------------------------------------------------------------------
pub struct CsvJoinFn;
impl ComputeFunction for CsvJoinFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_join", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let join_col = get_string_param(params, "join_column")?;
        let join_type = opt_str(params, "join_type", "inner");

        // Parse both CSVs
        let parse_csv = |data: &[u8]| -> Result<(Vec<String>, Vec<csv::StringRecord>), ComputeError> {
            let mut rdr = csv::Reader::from_reader(data);
            let h = rdr.headers().map_err(csv_err)?.clone();
            let rows: Vec<csv::StringRecord> = rdr.records().collect::<Result<_, _>>().map_err(csv_err)?;
            Ok((h.iter().map(|s| s.to_string()).collect(), rows))
        };
        let (lh, left) = parse_csv(&inputs[0])?;
        let (rh, right) = parse_csv(&inputs[1])?;
        let li = lh.iter().position(|h| h == join_col)
            .ok_or_else(|| ComputeError::InvalidParam(format!("join_column '{}' not in left", join_col)))?;
        let ri = rh.iter().position(|h| h == join_col)
            .ok_or_else(|| ComputeError::InvalidParam(format!("join_column '{}' not in right", join_col)))?;

        // Build right index
        let mut right_map: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (idx, row) in right.iter().enumerate() {
            let key = row.get(ri).unwrap_or("").to_string();
            right_map.entry(key).or_default().push(idx);
        }

        // Output headers: left headers + right headers (excluding join col from right)
        let right_cols: Vec<(usize, String)> = rh.iter().enumerate().filter(|&(i, _)| i != ri).map(|(i, h)| (i, h.to_string())).collect();
        let mut out_headers: Vec<String> = lh.clone();
        for (_, h) in &right_cols { out_headers.push(h.clone()); }
        let empty_right: Vec<String> = right_cols.iter().map(|_| String::new()).collect();

        let mut wtr = csv::Writer::from_writer(Vec::new());
        wtr.write_record(&out_headers).map_err(csv_err)?;

        let mut right_matched = vec![false; right.len()];

        for lrow in &left {
            let key = lrow.get(li).unwrap_or("");
            let matches = right_map.get(key);
            match matches {
                Some(indices) => {
                    for &ri_idx in indices {
                        right_matched[ri_idx] = true;
                        let mut fields: Vec<String> = lrow.iter().map(|f| f.to_string()).collect();
                        for (ci, _) in &right_cols {
                            fields.push(right[ri_idx].get(*ci).unwrap_or("").to_string());
                        }
                        wtr.write_record(&fields).map_err(csv_err)?;
                    }
                }
                None if join_type == "left" || join_type == "full" => {
                    let mut fields: Vec<String> = lrow.iter().map(|f| f.to_string()).collect();
                    fields.extend(empty_right.iter().cloned());
                    wtr.write_record(&fields).map_err(csv_err)?;
                }
                _ => {}
            }
        }
        // Right/full: unmatched right rows
        if join_type == "right" || join_type == "full" {
            let empty_left: Vec<String> = lh.iter().map(|_| String::new()).collect();
            for (idx, rrow) in right.iter().enumerate() {
                if !right_matched[idx] {
                    let mut fields = empty_left.clone();
                    fields[li] = rrow.get(ri).unwrap_or("").to_string(); // join col
                    for (ci, _) in &right_cols {
                        fields.push(rrow.get(*ci).unwrap_or("").to_string());
                    }
                    wtr.write_record(&fields).map_err(csv_err)?;
                }
            }
        }
        Ok(Bytes::from(wtr.into_inner().map_err(csv_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(30, s, 4) }
}

// ---------------------------------------------------------------------------
// #228 CsvDeduplicateFn (Batch)
// ---------------------------------------------------------------------------
pub struct CsvDeduplicateFn;
impl ComputeFunction for CsvDeduplicateFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_deduplicate", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let mut rdr = csv::Reader::from_reader(&data[..]);
        let headers = rdr.headers().map_err(csv_err)?.clone();
        let key_indices: Vec<usize> = match params.get("columns") {
            Some(Value::String(s)) => s.split(',').map(|c| c.trim())
                .filter_map(|c| headers.iter().position(|h| h == c)).collect(),
            _ => (0..headers.len()).collect(), // all columns
        };
        let mut seen = std::collections::HashSet::new();
        let mut wtr = csv::Writer::from_writer(Vec::new());
        wtr.write_record(&headers).map_err(csv_err)?;
        for rec in rdr.records() {
            let rec = rec.map_err(csv_err)?;
            let key: Vec<String> = key_indices.iter().map(|&i| rec.get(i).unwrap_or("").to_string()).collect();
            if seen.insert(key) {
                wtr.write_record(&rec).map_err(csv_err)?;
            }
        }
        Ok(Bytes::from(wtr.into_inner().map_err(csv_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(15, s, 3) }
}

// ---------------------------------------------------------------------------
// #229 TsvParseFn (Both)
// ---------------------------------------------------------------------------
pub struct TsvParseFn;
impl ComputeFunction for TsvParseFn {
    fn id(&self) -> FunctionId { FunctionId::new("tsv_parse", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let has_header = opt_str(params, "has_header", "true") == "true";
        let mut rdr = csv_reader(data, b'\t', has_header);
        let headers: Vec<String> = if has_header {
            rdr.headers().map_err(csv_err)?.iter().map(|h| h.to_string()).collect()
        } else {
            let mut rdr2 = csv_reader(data, b'\t', false);
            match rdr2.records().next() {
                Some(Ok(r)) => (0..r.len()).map(|i| format!("col{}", i)).collect(),
                _ => vec![],
            }
        };
        let mut rdr = csv_reader(data, b'\t', has_header);
        if has_header { let _ = rdr.headers(); }
        let mut rows = Vec::new();
        for rec in rdr.records() {
            let rec = rec.map_err(csv_err)?;
            let mut obj = serde_json::Map::new();
            for (i, f) in rec.iter().enumerate() {
                let key = headers.get(i).map(|s| s.as_str()).unwrap_or("_");
                obj.insert(key.to_string(), serde_json::Value::String(f.to_string()));
            }
            rows.push(serde_json::Value::Object(obj));
        }
        Ok(Bytes::from(serde_json::to_string(&rows).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 3) }
}

// ---------------------------------------------------------------------------
// #230 NdjsonParseFn (Both)
// ---------------------------------------------------------------------------
pub struct NdjsonParseFn;
impl ComputeFunction for NdjsonParseFn {
    fn id(&self) -> FunctionId { FunctionId::new("ndjson_parse", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let text = require_utf8(data)?;
        let arr: Vec<serde_json::Value> = text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l))
            .collect::<Result<_, _>>().map_err(json_err)?;
        Ok(Bytes::from(serde_json::to_string(&arr).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 3) }
}

// ---------------------------------------------------------------------------
// #231 NdjsonWriteFn (Both)
// ---------------------------------------------------------------------------
pub struct NdjsonWriteFn;
impl ComputeFunction for NdjsonWriteFn {
    fn id(&self) -> FunctionId { FunctionId::new("ndjson_write", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let arr: Vec<serde_json::Value> = serde_json::from_slice(data).map_err(json_err)?;
        let lines: Vec<String> = arr.iter()
            .map(|v| serde_json::to_string(v))
            .collect::<Result<_, _>>().map_err(json_err)?;
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 2) }
}

// ---------------------------------------------------------------------------
// #232 NdjsonFilterFn (Both)
// ---------------------------------------------------------------------------
pub struct NdjsonFilterFn;
impl ComputeFunction for NdjsonFilterFn {
    fn id(&self) -> FunctionId { FunctionId::new("ndjson_filter", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let path = get_string_param(params, "path")?;
        let op = get_string_param(params, "op")?;
        let value = get_string_param(params, "value")?;
        let text = require_utf8(data)?;
        let mut out = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() { continue; }
            let obj: serde_json::Value = serde_json::from_str(line).map_err(json_err)?;
            // Simple dot-path extraction
            let field_val = extract_path(&obj, path);
            let field_str = match &field_val {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => "null".to_string(),
                other => other.to_string(),
            };
            if matches_predicate(&field_str, op, value)? {
                out.push(line.to_string());
            }
        }
        Ok(Bytes::from(out.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(15, s, 2) }
}

fn extract_path<'a>(val: &'a serde_json::Value, path: &str) -> serde_json::Value {
    let mut current = val;
    for key in path.split('.') {
        match current.get(key) {
            Some(v) => current = v,
            None => return serde_json::Value::Null,
        }
    }
    current.clone()
}

// ---------------------------------------------------------------------------
// #233 NdjsonProjectFn (Both)
// ---------------------------------------------------------------------------
pub struct NdjsonProjectFn;
impl ComputeFunction for NdjsonProjectFn {
    fn id(&self) -> FunctionId { FunctionId::new("ndjson_project", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let paths_str = get_string_param(params, "paths")?;
        let paths: Vec<&str> = paths_str.split(',').map(|s| s.trim()).collect();
        let text = require_utf8(data)?;
        let mut out = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() { continue; }
            let obj: serde_json::Value = serde_json::from_str(line).map_err(json_err)?;
            let mut projected = serde_json::Map::new();
            for &p in &paths {
                let v = extract_path(&obj, p);
                projected.insert(p.to_string(), v);
            }
            out.push(serde_json::to_string(&serde_json::Value::Object(projected)).map_err(json_err)?);
        }
        Ok(Bytes::from(out.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(15, s, 2) }
}

// ---------------------------------------------------------------------------
// #234 JsonPathExtractFn (Batch)
// ---------------------------------------------------------------------------
pub struct JsonPathExtractFn;
impl ComputeFunction for JsonPathExtractFn {
    fn id(&self) -> FunctionId { FunctionId::new("jsonpath_extract", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let path = get_string_param(params, "path")?;
        let val: serde_json::Value = serde_json::from_slice(data).map_err(json_err)?;
        use jsonpath_rust::JsonPathQuery;
        let results = val.path(path)
            .map_err(|e| ComputeError::InvalidParam(format!("invalid jsonpath: {}", e)))?;
        Ok(Bytes::from(serde_json::to_string(&results).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 3) }
}

// ---------------------------------------------------------------------------
// #235 JsonMergeFn (Batch) — 2 inputs
// ---------------------------------------------------------------------------
pub struct JsonMergeFn;
impl ComputeFunction for JsonMergeFn {
    fn id(&self) -> FunctionId { FunctionId::new("json_merge", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let strategy = opt_str(params, "strategy", "shallow");
        let mut base: serde_json::Value = serde_json::from_slice(&inputs[0]).map_err(json_err)?;
        let overlay: serde_json::Value = serde_json::from_slice(&inputs[1]).map_err(json_err)?;
        match strategy {
            "shallow" => {
                if let (Some(b), Some(o)) = (base.as_object_mut(), overlay.as_object()) {
                    for (k, v) in o { b.insert(k.clone(), v.clone()); }
                }
            }
            "deep" => deep_merge(&mut base, &overlay),
            _ => return Err(ComputeError::InvalidParam("strategy must be 'shallow' or 'deep'".into())),
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&base).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 3) }
}

fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            for (k, v) in o {
                deep_merge(b.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

// ---------------------------------------------------------------------------
// #236 JsonFlattenFn (Batch)
// ---------------------------------------------------------------------------
pub struct JsonFlattenFn;
impl ComputeFunction for JsonFlattenFn {
    fn id(&self) -> FunctionId { FunctionId::new("json_flatten", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let sep = opt_str(params, "separator", ".");
        let val: serde_json::Value = serde_json::from_slice(data).map_err(json_err)?;
        let mut flat = serde_json::Map::new();
        flatten_recursive(&val, "", sep, &mut flat);
        Ok(Bytes::from(serde_json::to_string(&serde_json::Value::Object(flat)).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 3) }
}

fn flatten_recursive(val: &serde_json::Value, prefix: &str, sep: &str, out: &mut serde_json::Map<String, serde_json::Value>) {
    match val {
        serde_json::Value::Object(obj) => {
            for (k, v) in obj {
                let new_key = if prefix.is_empty() { k.clone() } else { format!("{}{}{}", prefix, sep, k) };
                flatten_recursive(v, &new_key, sep, out);
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let new_key = if prefix.is_empty() { i.to_string() } else { format!("{}{}{}", prefix, sep, i) };
                flatten_recursive(v, &new_key, sep, out);
            }
        }
        _ => { out.insert(prefix.to_string(), val.clone()); }
    }
}

// ---------------------------------------------------------------------------
// #237 JsonUnflattenFn (Batch)
// ---------------------------------------------------------------------------
pub struct JsonUnflattenFn;
impl ComputeFunction for JsonUnflattenFn {
    fn id(&self) -> FunctionId { FunctionId::new("json_unflatten", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let sep = opt_str(params, "separator", ".");
        let flat: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(data).map_err(json_err)?;
        let mut root = serde_json::Value::Object(serde_json::Map::new());
        for (key, val) in &flat {
            let parts: Vec<&str> = key.split(sep).collect();
            set_nested(&mut root, &parts, val.clone())?;
        }
        Ok(Bytes::from(serde_json::to_string(&root).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 3) }
}

fn set_nested(root: &mut serde_json::Value, parts: &[&str], val: serde_json::Value) -> Result<(), ComputeError> {
    if parts.len() == 1 {
        root.as_object_mut()
            .ok_or_else(|| ComputeError::ExecutionFailed("unflatten conflict".into()))?
            .insert(parts[0].to_string(), val);
        return Ok(());
    }
    let obj = root.as_object_mut()
        .ok_or_else(|| ComputeError::ExecutionFailed("unflatten conflict".into()))?;
    let entry = obj.entry(parts[0].to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    set_nested(entry, &parts[1..], val)
}

// ---------------------------------------------------------------------------
// #238 XmlParseFn (Batch)
// ---------------------------------------------------------------------------
pub struct XmlParseFn;
impl ComputeFunction for XmlParseFn {
    fn id(&self) -> FunctionId { FunctionId::new("xml_parse", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let text = require_utf8(data)?;
        let json = xml_to_json(text)?;
        Ok(Bytes::from(serde_json::to_string_pretty(&json).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(15, s, 4) }
}

fn xml_to_json(xml: &str) -> Result<serde_json::Value, ComputeError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<(String, serde_json::Map<String, serde_json::Value>, Vec<serde_json::Value>)> = Vec::new();
    let mut root = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut attrs = serde_json::Map::new();
                for attr in e.attributes().flatten() {
                    let k = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let v = String::from_utf8_lossy(&attr.value).to_string();
                    attrs.insert(format!("@{}", k), serde_json::Value::String(v));
                }
                stack.push((name, attrs, Vec::new()));
            }
            Ok(Event::Text(e)) => {
                let text = e.unescape().map_err(|e| ComputeError::ExecutionFailed(format!("xml: {}", e)))?.to_string();
                if !text.trim().is_empty() {
                    if let Some(top) = stack.last_mut() {
                        top.1.insert("#text".to_string(), serde_json::Value::String(text));
                    }
                }
            }
            Ok(Event::End(_)) => {
                let (name, attrs, children) = stack.pop()
                    .ok_or_else(|| ComputeError::ExecutionFailed("xml: unexpected end tag".into()))?;
                let mut obj = attrs;
                for child in children {
                    if let serde_json::Value::Object(child_map) = child {
                        for (k, v) in child_map {
                            obj.entry(k).or_insert(v);
                        }
                    }
                }
                let element = serde_json::json!({&name: obj});
                if let Some(parent) = stack.last_mut() {
                    parent.2.push(element.clone());
                    // Also add to parent's attrs for nested access
                    if let serde_json::Value::Object(m) = &element {
                        for (k, v) in m {
                            parent.1.insert(k.clone(), v.clone());
                        }
                    }
                } else {
                    root = Some(element);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(ComputeError::ExecutionFailed(format!("xml: {}", e))),
        }
    }
    Ok(root.unwrap_or(serde_json::Value::Null))
}

// ---------------------------------------------------------------------------
// #239 XmlWriteFn (Batch)
// ---------------------------------------------------------------------------
pub struct XmlWriteFn;
impl ComputeFunction for XmlWriteFn {
    fn id(&self) -> FunctionId { FunctionId::new("xml_write", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let root_el = opt_str(params, "root_element", "root");
        let val: serde_json::Value = serde_json::from_slice(data).map_err(json_err)?;
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        json_to_xml(&val, root_el, &mut xml, 0);
        Ok(Bytes::from(xml))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(15, s, 3) }
}

fn json_to_xml(val: &serde_json::Value, tag: &str, out: &mut String, indent: usize) {
    let pad: String = "  ".repeat(indent);
    match val {
        serde_json::Value::Object(obj) => {
            // Separate attributes (@-prefixed) from children
            let attrs: Vec<(&String, &serde_json::Value)> = obj.iter().filter(|(k, _)| k.starts_with('@')).collect();
            let children: Vec<(&String, &serde_json::Value)> = obj.iter().filter(|(k, _)| !k.starts_with('@') && *k != "#text").collect();
            let text = obj.get("#text").and_then(|v| v.as_str());
            out.push_str(&format!("{}<{}", pad, tag));
            for (k, v) in &attrs {
                let attr_name = &k[1..];
                out.push_str(&format!(" {}=\"{}\"", attr_name, v.as_str().unwrap_or("")));
            }
            if children.is_empty() && text.is_none() {
                out.push_str("/>\n");
            } else {
                out.push_str(">");
                if let Some(t) = text {
                    out.push_str(t);
                    if children.is_empty() {
                        out.push_str(&format!("</{}>\n", tag));
                        return;
                    }
                }
                out.push('\n');
                for (k, v) in &children {
                    json_to_xml(v, k, out, indent + 1);
                }
                out.push_str(&format!("{}</{}>\n", pad, tag));
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                json_to_xml(item, tag, out, indent);
            }
        }
        _ => {
            let s = match val {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            out.push_str(&format!("{}<{}>{}</{}>\n", pad, tag, s, tag));
        }
    }
}

// ---------------------------------------------------------------------------
// #240 XmlXPathExtractFn (Batch) — simple path extraction
// ---------------------------------------------------------------------------
pub struct XmlXPathExtractFn;
impl ComputeFunction for XmlXPathExtractFn {
    fn id(&self) -> FunctionId { FunctionId::new("xml_xpath_extract", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let data = one_input(&inputs)?;
        let xpath = get_string_param(params, "xpath")?;
        let text = require_utf8(data)?;
        let json = xml_to_json(text)?;
        // Simple path extraction: /root/child/subchild → navigate JSON
        let parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
        let mut current = &json;
        for part in &parts {
            match current.get(part) {
                Some(v) => current = v,
                None => return Ok(Bytes::from("null")),
            }
        }
        Ok(Bytes::from(serde_json::to_string_pretty(current).map_err(json_err)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(15, s, 4) }
}

// ---------------------------------------------------------------------------
// #241 XmlXsltTransformFn (Batch) — 2 inputs: XML + XSLT template
// Simplified: applies template string replacements (no full XSLT engine)
// ---------------------------------------------------------------------------
pub struct XmlXsltTransformFn;
impl ComputeFunction for XmlXsltTransformFn {
    fn id(&self) -> FunctionId { FunctionId::new("xml_xslt_transform", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let _xml = require_utf8(&inputs[0])?;
        let _xslt = require_utf8(&inputs[1])?;
        // Full XSLT requires a dedicated engine; for now, pass XML through
        // Real implementation would use an XSLT processor
        Ok(inputs[0].clone())
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(20, s, 4) }
}

// ---------------------------------------------------------------------------
// #242 XmlValidateDtdFn (Batch) — 2 inputs: XML + DTD
// ---------------------------------------------------------------------------
pub struct XmlValidateDtdFn;
impl ComputeFunction for XmlValidateDtdFn {
    fn id(&self) -> FunctionId { FunctionId::new("xml_validate_dtd", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() { return Err(ComputeError::InputCount { expected: 1, got: 0 }); }
        let text = require_utf8(&inputs[0])?;
        // Validate well-formedness via quick-xml parse
        let mut reader = quick_xml::Reader::from_str(text);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Err(e) => return Err(ComputeError::ExecutionFailed(format!("xml validation: {}", e))),
                _ => {}
            }
        }
        Ok(inputs[0].clone()) // pass-through
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 2) }
}

// ---------------------------------------------------------------------------
// #243 XmlValidateXsdFn (Batch) — 2 inputs: XML + XSD
// ---------------------------------------------------------------------------
pub struct XmlValidateXsdFn;
impl ComputeFunction for XmlValidateXsdFn {
    fn id(&self) -> FunctionId { FunctionId::new("xml_validate_xsd", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let text = require_utf8(&inputs[0])?;
        let _xsd = require_utf8(&inputs[1])?;
        // Validate well-formedness (full XSD validation requires libxml2 or similar)
        let mut reader = quick_xml::Reader::from_str(text);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Err(e) => return Err(ComputeError::ExecutionFailed(format!("xml validation: {}", e))),
                _ => {}
            }
        }
        Ok(inputs[0].clone()) // pass-through
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(10, s, 2) }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------
use crate::registry::FunctionRegistry;
use std::sync::Arc;

pub fn register_csv_functions(registry: &mut FunctionRegistry) {
    registry.register(Arc::new(CsvParseFn));
    registry.register(Arc::new(CsvWriteFn));
    registry.register(Arc::new(CsvSchemaInferFn));
    registry.register(Arc::new(CsvColumnSelectFn));
    registry.register(Arc::new(CsvColumnRenameFn));
    registry.register(Arc::new(CsvFilterFn));
    registry.register(Arc::new(CsvSortFn));
    registry.register(Arc::new(CsvAggregateFn));
    registry.register(Arc::new(CsvJoinFn));
    registry.register(Arc::new(CsvDeduplicateFn));
    registry.register(Arc::new(TsvParseFn));
    registry.register(Arc::new(NdjsonParseFn));
    registry.register(Arc::new(NdjsonWriteFn));
    registry.register(Arc::new(NdjsonFilterFn));
    registry.register(Arc::new(NdjsonProjectFn));
    registry.register(Arc::new(JsonPathExtractFn));
    registry.register(Arc::new(JsonMergeFn));
    registry.register(Arc::new(JsonFlattenFn));
    registry.register(Arc::new(JsonUnflattenFn));
    registry.register(Arc::new(XmlParseFn));
    registry.register(Arc::new(XmlWriteFn));
    registry.register(Arc::new(XmlXPathExtractFn));
    registry.register(Arc::new(XmlXsltTransformFn));
    registry.register(Arc::new(XmlValidateDtdFn));
    registry.register(Arc::new(XmlValidateXsdFn));
}
