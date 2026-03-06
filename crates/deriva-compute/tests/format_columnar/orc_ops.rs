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
        Field::new("city", DataType::Utf8, false),
        Field::new("pop", DataType::Int32, false),
    ]));
    let batch = RecordBatch::try_new(schema.clone(), vec![
        Arc::new(StringArray::from(vec!["NYC", "LA", "CHI"])),
        Arc::new(Int32Array::from(vec![8000000, 4000000, 2700000])),
    ]).unwrap();
    let mut buf = Vec::new();
    let mut w = arrow::ipc::writer::StreamWriter::try_new(&mut buf, &schema).unwrap();
    w.write(&batch).unwrap();
    w.finish().unwrap();
    Bytes::from(buf)
}

fn sample_orc() -> Bytes {
    let ipc = sample_ipc();
    OrcWriteFn.execute(vec![ipc], &p(&[])).unwrap()
}

// ---- orc_read (5 tests) ----
#[test]
fn orc_read_basic() {
    let orc = sample_orc();
    let ipc = OrcReadFn.execute(vec![orc], &p(&[])).unwrap();
    assert!(!ipc.is_empty());
}

#[test]
fn orc_read_roundtrip() {
    let orc = sample_orc();
    let ipc = OrcReadFn.execute(vec![orc], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    assert!(text.contains("NYC"));
    assert!(text.contains("8000000"));
}

#[test]
fn orc_read_preserves_rows() {
    let orc = sample_orc();
    let ipc = OrcReadFn.execute(vec![orc], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    assert_eq!(std::str::from_utf8(&ndjson).unwrap().lines().count(), 3);
}

#[test]
fn orc_read_invalid() {
    assert!(OrcReadFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn orc_read_no_input() {
    assert!(OrcReadFn.execute(vec![], &p(&[])).is_err());
}

// ---- orc_write (5 tests) ----
#[test]
fn orc_write_produces_valid_orc() {
    let ipc = sample_ipc();
    let orc = OrcWriteFn.execute(vec![ipc], &p(&[])).unwrap();
    assert!(OrcReadFn.execute(vec![orc], &p(&[])).is_ok());
}

#[test]
fn orc_write_roundtrip_data() {
    let ipc = sample_ipc();
    let orc = OrcWriteFn.execute(vec![ipc], &p(&[])).unwrap();
    let ipc2 = OrcReadFn.execute(vec![orc], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc2], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&ndjson).unwrap().contains("LA"));
}

#[test]
fn orc_write_invalid_ipc() {
    assert!(OrcWriteFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn orc_write_no_input() {
    assert!(OrcWriteFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn orc_write_id() {
    assert_eq!(OrcWriteFn.id().name, "orc_write");
}

// ---- orc_metadata (5 tests) ----
#[test]
fn orc_metadata_has_num_rows() {
    let orc = sample_orc();
    let out = OrcMetadataFn.execute(vec![orc], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["num_rows"], 3);
}

#[test]
fn orc_metadata_has_stripes() {
    let orc = sample_orc();
    let out = OrcMetadataFn.execute(vec![orc], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["num_stripes"].as_i64().unwrap() >= 1);
}

#[test]
fn orc_metadata_has_schema() {
    let orc = sample_orc();
    let out = OrcMetadataFn.execute(vec![orc], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["schema"].is_string());
}

#[test]
fn orc_metadata_invalid() {
    assert!(OrcMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn orc_metadata_no_input() {
    assert!(OrcMetadataFn.execute(vec![], &p(&[])).is_err());
}

// ---- orc_filter (5 tests) ----
#[test]
fn orc_filter_eq() {
    let orc = sample_orc();
    let filtered = OrcFilterFn.execute(vec![orc], &p(&[("column", "city"), ("op", "eq"), ("value", "NYC")])).unwrap();
    let ipc = OrcReadFn.execute(vec![filtered], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("NYC"));
}

#[test]
fn orc_filter_gt() {
    let orc = sample_orc();
    let filtered = OrcFilterFn.execute(vec![orc], &p(&[("column", "pop"), ("op", "gt"), ("value", "3000000")])).unwrap();
    let ipc = OrcReadFn.execute(vec![filtered], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    assert_eq!(std::str::from_utf8(&ndjson).unwrap().lines().count(), 2);
}

#[test]
fn orc_filter_nonexistent_column() {
    let orc = sample_orc();
    assert!(OrcFilterFn.execute(vec![orc], &p(&[("column", "nope"), ("op", "eq"), ("value", "x")])).is_err());
}

#[test]
fn orc_filter_missing_params() {
    let orc = sample_orc();
    assert!(OrcFilterFn.execute(vec![orc], &p(&[])).is_err());
}

#[test]
fn orc_filter_no_input() {
    assert!(OrcFilterFn.execute(vec![], &p(&[("column", "city"), ("op", "eq"), ("value", "x")])).is_err());
}
