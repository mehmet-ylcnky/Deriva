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
        Field::new("region", DataType::Utf8, false),
        Field::new("sales", DataType::Int32, false),
    ]));
    let batch = RecordBatch::try_new(schema.clone(), vec![
        Arc::new(StringArray::from(vec!["US", "EU", "US", "EU"])),
        Arc::new(Int32Array::from(vec![100, 200, 150, 250])),
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

// ---- parquet_partition_write (5 tests) ----
#[test]
fn parquet_partition_write_basic() {
    let pq = sample_parquet();
    let out = ParquetPartitionWriteFn.execute(vec![pq], &p(&[("partition_column", "region")])).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(manifest.is_object());
    assert!(manifest.as_object().unwrap().len() > 0);
}

#[test]
fn parquet_partition_write_correct_partition_count() {
    let pq = sample_parquet();
    let out = ParquetPartitionWriteFn.execute(vec![pq], &p(&[("partition_column", "region")])).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(manifest.as_object().unwrap().len(), 2); // US and EU
}

#[test]
fn parquet_partition_write_has_row_counts() {
    let pq = sample_parquet();
    let out = ParquetPartitionWriteFn.execute(vec![pq], &p(&[("partition_column", "region")])).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&out).unwrap();
    for (_key, val) in manifest.as_object().unwrap() {
        assert!(val["num_rows"].is_number());
        assert!(val["size_bytes"].is_number());
    }
}

#[test]
fn parquet_partition_write_missing_param() {
    let pq = sample_parquet();
    assert!(ParquetPartitionWriteFn.execute(vec![pq], &p(&[])).is_err());
}

#[test]
fn parquet_partition_write_no_input() {
    assert!(ParquetPartitionWriteFn.execute(vec![], &p(&[("partition_column", "region")])).is_err());
}

// ---- parquet_statistics (5 tests) ----
#[test]
fn parquet_statistics_is_object() {
    let pq = sample_parquet();
    let out = ParquetStatisticsFn.execute(vec![pq], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v.is_object());
}

#[test]
fn parquet_statistics_column_names() {
    let pq = sample_parquet();
    let out = ParquetStatisticsFn.execute(vec![pq], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("region"));
    assert!(obj.contains_key("sales"));
}

#[test]
fn parquet_statistics_has_null_count() {
    let pq = sample_parquet();
    let out = ParquetStatisticsFn.execute(vec![pq], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    for (_name, stats) in v.as_object().unwrap() {
        assert!(stats.get("null_count").is_some());
    }
}

#[test]
fn parquet_statistics_invalid() {
    assert!(ParquetStatisticsFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn parquet_statistics_no_input() {
    assert!(ParquetStatisticsFn.execute(vec![], &p(&[])).is_err());
}

// ---- columnar_to_row (5 tests) ----
#[test]
fn columnar_to_row_from_ipc() {
    let ipc = sample_ipc();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    assert_eq!(text.lines().count(), 4);
}

#[test]
fn columnar_to_row_from_parquet() {
    let pq = sample_parquet();
    let ndjson = ColumnarToRowFn.execute(vec![pq], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    assert!(text.contains("US"));
    assert!(text.contains("100"));
}

#[test]
fn columnar_to_row_valid_json_lines() {
    let ipc = sample_ipc();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    for line in std::str::from_utf8(&ndjson).unwrap().lines() {
        assert!(serde_json::from_str::<serde_json::Value>(line).is_ok());
    }
}

#[test]
fn columnar_to_row_invalid() {
    assert!(ColumnarToRowFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn columnar_to_row_no_input() {
    assert!(ColumnarToRowFn.execute(vec![], &p(&[])).is_err());
}
