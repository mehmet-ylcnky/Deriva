use bytes::Bytes;
use deriva_compute::builtins_format_columnar::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use arrow::array::*;
use arrow::datatypes::*;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

fn sample_ipc() -> Bytes {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("label", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(schema.clone(), vec![
        Arc::new(Int32Array::from(vec![1, 2, 3])),
        Arc::new(StringArray::from(vec!["a", "b", "c"])),
    ]).unwrap();
    let mut buf = Vec::new();
    let mut w = arrow::ipc::writer::StreamWriter::try_new(&mut buf, &schema).unwrap();
    w.write(&batch).unwrap();
    w.finish().unwrap();
    Bytes::from(buf)
}

fn sample_parquet() -> Bytes {
    ParquetWriteFn.execute(vec![sample_ipc()], &p(&[])).unwrap()
}

// ---- arrow_ipc_read (5 tests) ----
#[test]
fn arrow_ipc_read_basic() {
    let ipc = sample_ipc();
    let out = ArrowIpcReadFn.execute(vec![ipc], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn arrow_ipc_read_roundtrip() {
    let ipc = sample_ipc();
    let out = ArrowIpcReadFn.execute(vec![ipc], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![out], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&ndjson).unwrap().contains("\"a\""));
}

#[test]
fn arrow_ipc_read_invalid() {
    assert!(ArrowIpcReadFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn arrow_ipc_read_no_input() {
    assert!(ArrowIpcReadFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn arrow_ipc_read_preserves_rows() {
    let ipc = sample_ipc();
    let out = ArrowIpcReadFn.execute(vec![ipc], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![out], &p(&[])).unwrap();
    assert_eq!(std::str::from_utf8(&ndjson).unwrap().lines().count(), 3);
}

// ---- arrow_ipc_write (5 tests) ----
#[test]
fn arrow_ipc_write_basic() {
    let ipc = sample_ipc();
    let out = ArrowIpcWriteFn.execute(vec![ipc], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn arrow_ipc_write_roundtrip() {
    let ipc = sample_ipc();
    let out = ArrowIpcWriteFn.execute(vec![ipc], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![out], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&ndjson).unwrap().contains("\"b\""));
}

#[test]
fn arrow_ipc_write_invalid() {
    assert!(ArrowIpcWriteFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn arrow_ipc_write_no_input() {
    assert!(ArrowIpcWriteFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn arrow_ipc_write_id() {
    assert_eq!(ArrowIpcWriteFn.id().name, "arrow_ipc_write");
}

// ---- arrow_schema_extract (5 tests) ----
#[test]
fn arrow_schema_extract_from_ipc() {
    let ipc = sample_ipc();
    let out = ArrowSchemaExtractFn.execute(vec![ipc], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["fields"].is_array());
    assert_eq!(v["fields"].as_array().unwrap().len(), 2);
}

#[test]
fn arrow_schema_extract_from_parquet() {
    let pq = sample_parquet();
    let out = ArrowSchemaExtractFn.execute(vec![pq], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["fields"].is_array());
}

#[test]
fn arrow_schema_extract_field_names() {
    let ipc = sample_ipc();
    let out = ArrowSchemaExtractFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("id"));
    assert!(text.contains("label"));
}

#[test]
fn arrow_schema_extract_invalid() {
    assert!(ArrowSchemaExtractFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn arrow_schema_extract_no_input() {
    assert!(ArrowSchemaExtractFn.execute(vec![], &p(&[])).is_err());
}
