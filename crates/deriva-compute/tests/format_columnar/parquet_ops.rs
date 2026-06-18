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
        Field::new("name", DataType::Utf8, false),
        Field::new("age", DataType::Int32, false),
    ]));
    let batch = RecordBatch::try_new(schema.clone(), vec![
        Arc::new(StringArray::from(vec!["Alice", "Bob", "Charlie"])),
        Arc::new(Int32Array::from(vec![30, 25, 35])),
    ]).unwrap();
    let mut buf = Vec::new();
    let mut w = arrow::ipc::writer::StreamWriter::try_new(&mut buf, &schema).unwrap();
    w.write(&batch).unwrap();
    w.finish().unwrap();
    Bytes::from(buf)
}

fn sample_parquet() -> Bytes {
    let ipc = sample_ipc();
    ParquetWriteFn.execute(vec![ipc], &p(&[])).unwrap()
}

// ---- parquet_read (5 tests) ----
#[test]
fn parquet_read_basic() {
    let pq = sample_parquet();
    let ipc = ParquetReadFn.execute(vec![pq], &p(&[])).unwrap();
    assert!(!ipc.is_empty());
}

#[test]
fn parquet_read_roundtrip_data() {
    let pq = sample_parquet();
    let ipc = ParquetReadFn.execute(vec![pq], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    assert!(text.contains("Alice"));
    assert!(text.contains("Bob"));
}

#[test]
fn parquet_read_invalid() {
    assert!(ParquetReadFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn parquet_read_no_input() {
    assert!(ParquetReadFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn parquet_read_preserves_row_count() {
    let pq = sample_parquet();
    let ipc = ParquetReadFn.execute(vec![pq], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    assert_eq!(std::str::from_utf8(&ndjson).unwrap().lines().count(), 3);
}

// ---- parquet_write (5 tests) ----
#[test]
fn parquet_write_default_compression() {
    let ipc = sample_ipc();
    let pq = ParquetWriteFn.execute(vec![ipc], &p(&[])).unwrap();
    assert!(!pq.is_empty());
    // Verify it's valid parquet by reading back
    assert!(ParquetReadFn.execute(vec![pq], &p(&[])).is_ok());
}

#[test]
fn parquet_write_snappy() {
    let ipc = sample_ipc();
    let pq = ParquetWriteFn.execute(vec![ipc], &p(&[("compression", "snappy")])).unwrap();
    assert!(ParquetReadFn.execute(vec![pq], &p(&[])).is_ok());
}

#[test]
fn parquet_write_zstd() {
    let ipc = sample_ipc();
    let pq = ParquetWriteFn.execute(vec![ipc], &p(&[("compression", "zstd")])).unwrap();
    assert!(ParquetReadFn.execute(vec![pq], &p(&[])).is_ok());
}

#[test]
fn parquet_write_invalid_compression() {
    let ipc = sample_ipc();
    assert!(ParquetWriteFn.execute(vec![ipc], &p(&[("compression", "bogus")])).is_err());
}

#[test]
fn parquet_write_no_input() {
    assert!(ParquetWriteFn.execute(vec![], &p(&[])).is_err());
}

// ---- parquet_metadata (5 tests) ----
#[test]
fn parquet_metadata_has_num_rows() {
    let pq = sample_parquet();
    let out = ParquetMetadataFn.execute(vec![pq], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["num_rows"], 3);
}

#[test]
fn parquet_metadata_has_row_groups() {
    let pq = sample_parquet();
    let out = ParquetMetadataFn.execute(vec![pq], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["row_groups"].is_array());
    assert!(v["num_row_groups"].as_i64().unwrap() >= 1);
}

#[test]
fn parquet_metadata_has_version() {
    let pq = sample_parquet();
    let out = ParquetMetadataFn.execute(vec![pq], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["version"].is_number());
}

#[test]
fn parquet_metadata_invalid() {
    assert!(ParquetMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn parquet_metadata_no_input() {
    assert!(ParquetMetadataFn.execute(vec![], &p(&[])).is_err());
}

// ---- parquet_projection (5 tests) ----
#[test]
fn parquet_projection_single_column() {
    let pq = sample_parquet();
    let projected = ParquetProjectionFn.execute(vec![pq], &p(&[("columns", "name")])).unwrap();
    let ipc = ParquetReadFn.execute(vec![projected], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    assert!(text.contains("Alice"));
    // age should not be present
    assert!(!text.contains("30"));
}

#[test]
fn parquet_projection_multiple_columns() {
    let pq = sample_parquet();
    let projected = ParquetProjectionFn.execute(vec![pq], &p(&[("columns", "name,age")])).unwrap();
    let ipc = ParquetReadFn.execute(vec![projected], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    assert!(text.contains("Alice"));
    assert!(text.contains("30"));
}

#[test]
fn parquet_projection_missing_columns_param() {
    let pq = sample_parquet();
    assert!(ParquetProjectionFn.execute(vec![pq], &p(&[])).is_err());
}

#[test]
fn parquet_projection_invalid_column_lists_available() {
    let pq = sample_parquet();
    let err = ParquetProjectionFn.execute(vec![pq], &p(&[("columns", "nonexistent")])).unwrap_err();
    let msg = err.to_string();
    // Error should mention the invalid column and list available ones (Requirement 9.3)
    assert!(msg.contains("nonexistent"), "error should mention the invalid column name");
    assert!(msg.contains("name"), "error should list available column 'name'");
    assert!(msg.contains("age"), "error should list available column 'age'");
}

#[test]
fn parquet_projection_preserves_row_count() {
    let pq = sample_parquet();
    let projected = ParquetProjectionFn.execute(vec![pq], &p(&[("columns", "age")])).unwrap();
    let ipc = ParquetReadFn.execute(vec![projected], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    assert_eq!(std::str::from_utf8(&ndjson).unwrap().lines().count(), 3);
}

#[test]
fn parquet_projection_no_input() {
    assert!(ParquetProjectionFn.execute(vec![], &p(&[("columns", "name")])).is_err());
}

// ---- parquet_filter (5 tests + statistics pushdown) ----

/// Test row group statistics-based pushdown: create a parquet file with multiple row groups
/// and verify that filtering across row group boundaries produces correct results.
/// This validates Requirement 5.4 (predicate pushdown via row group statistics).
#[test]
fn parquet_filter_statistics_pushdown_multi_row_group() {
    // Create a larger dataset that will produce multiple row groups when written
    // with a small row group size. We write two separate batches to force multiple row groups.
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("value", DataType::Int32, false),
    ]));
    // Batch 1: ids 1-100, values 1-100 (row group stats: min=1, max=100)
    let ids1: Vec<i32> = (1..=100).collect();
    let vals1: Vec<i32> = (1..=100).collect();
    let batch1 = RecordBatch::try_new(schema.clone(), vec![
        Arc::new(Int32Array::from(ids1)),
        Arc::new(Int32Array::from(vals1)),
    ]).unwrap();
    // Batch 2: ids 101-200, values 101-200 (row group stats: min=101, max=200)
    let ids2: Vec<i32> = (101..=200).collect();
    let vals2: Vec<i32> = (101..=200).collect();
    let batch2 = RecordBatch::try_new(schema.clone(), vec![
        Arc::new(Int32Array::from(ids2)),
        Arc::new(Int32Array::from(vals2)),
    ]).unwrap();
    // Write with small max_row_group_size to force multiple row groups
    let props = parquet::file::properties::WriterProperties::builder()
        .set_max_row_group_size(100)
        .build();
    let mut buf = Vec::new();
    let mut writer = parquet::arrow::ArrowWriter::try_new(&mut buf, schema.clone(), Some(props)).unwrap();
    writer.write(&batch1).unwrap();
    writer.write(&batch2).unwrap();
    writer.close().unwrap();
    let pq = Bytes::from(buf);

    // Verify there are indeed multiple row groups
    let meta_out = ParquetMetadataFn.execute(vec![pq.clone()], &p(&[])).unwrap();
    let meta: serde_json::Value = serde_json::from_slice(&meta_out).unwrap();
    assert!(meta["num_row_groups"].as_i64().unwrap() >= 2, "expected multiple row groups");

    // Filter: value > 150 should only return rows from the second row group
    // With statistics-based pushdown, the first row group (max=100) would be skipped entirely
    let filtered = ParquetFilterFn.execute(vec![pq.clone()], &p(&[("column", "value"), ("op", "gt"), ("value", "150")])).unwrap();
    let ipc = ParquetReadFn.execute(vec![filtered], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    // Should have exactly 50 rows (151-200)
    assert_eq!(text.lines().count(), 50);
    // Verify all results satisfy the predicate
    for line in text.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(row["value"].as_i64().unwrap() > 150);
    }
}

/// Test that parquet_statistics returns min/max info that could be used for pushdown
#[test]
fn parquet_filter_statistics_available_for_pushdown() {
    // Verify that row group statistics are actually populated for filter use
    let pq = sample_parquet();
    let stats_out = ParquetStatisticsFn.execute(vec![pq], &p(&[])).unwrap();
    let stats: serde_json::Value = serde_json::from_slice(&stats_out).unwrap();
    // The 'age' column should have min/max statistics
    assert!(stats.get("age").is_some(), "age column should have statistics");
    let age_stats = &stats["age"];
    assert!(age_stats.get("min").is_some(), "should have min statistic");
    assert!(age_stats.get("max").is_some(), "should have max statistic");
}

#[test]
fn parquet_filter_eq() {
    let pq = sample_parquet();
    let filtered = ParquetFilterFn.execute(vec![pq], &p(&[("column", "name"), ("op", "eq"), ("value", "Alice")])).unwrap();
    let ipc = ParquetReadFn.execute(vec![filtered], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("Alice"));
}

#[test]
fn parquet_filter_gt_int() {
    let pq = sample_parquet();
    let filtered = ParquetFilterFn.execute(vec![pq], &p(&[("column", "age"), ("op", "gt"), ("value", "28")])).unwrap();
    let ipc = ParquetReadFn.execute(vec![filtered], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    assert_eq!(text.lines().count(), 2); // Alice(30) and Charlie(35)
}

#[test]
fn parquet_filter_missing_column_param() {
    let pq = sample_parquet();
    assert!(ParquetFilterFn.execute(vec![pq], &p(&[("op", "eq"), ("value", "x")])).is_err());
}

#[test]
fn parquet_filter_nonexistent_column() {
    let pq = sample_parquet();
    assert!(ParquetFilterFn.execute(vec![pq], &p(&[("column", "nope"), ("op", "eq"), ("value", "x")])).is_err());
}

#[test]
fn parquet_filter_no_input() {
    assert!(ParquetFilterFn.execute(vec![], &p(&[("column", "name"), ("op", "eq"), ("value", "x")])).is_err());
}

// ---- parquet_merge (5 tests) ----
#[test]
fn parquet_merge_two_files() {
    let pq = sample_parquet();
    let merged = ParquetMergeFn.execute(vec![pq.clone(), pq], &p(&[])).unwrap();
    let ipc = ParquetReadFn.execute(vec![merged], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    assert_eq!(std::str::from_utf8(&ndjson).unwrap().lines().count(), 6);
}

#[test]
fn parquet_merge_single_file() {
    let pq = sample_parquet();
    let merged = ParquetMergeFn.execute(vec![pq], &p(&[])).unwrap();
    let ipc = ParquetReadFn.execute(vec![merged], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    assert_eq!(std::str::from_utf8(&ndjson).unwrap().lines().count(), 3);
}

#[test]
fn parquet_merge_preserves_data() {
    let pq = sample_parquet();
    let merged = ParquetMergeFn.execute(vec![pq.clone(), pq], &p(&[])).unwrap();
    let ipc = ParquetReadFn.execute(vec![merged], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    let text = std::str::from_utf8(&ndjson).unwrap();
    assert_eq!(text.matches("Alice").count(), 2);
}

#[test]
fn parquet_merge_no_input() {
    assert!(ParquetMergeFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn parquet_merge_invalid() {
    assert!(ParquetMergeFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

// ---- parquet_to_arrow (5 tests) ----
#[test]
fn parquet_to_arrow_roundtrip() {
    let pq = sample_parquet();
    let ipc = ParquetToArrowFn.execute(vec![pq], &p(&[])).unwrap();
    let pq2 = ArrowToParquetFn.execute(vec![ipc], &p(&[])).unwrap();
    let ipc2 = ParquetToArrowFn.execute(vec![pq2], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc2], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&ndjson).unwrap().contains("Alice"));
}

#[test]
fn parquet_to_arrow_produces_ipc() {
    let pq = sample_parquet();
    let ipc = ParquetToArrowFn.execute(vec![pq], &p(&[])).unwrap();
    assert!(!ipc.is_empty());
    // Should be readable as IPC
    let ndjson = ColumnarToRowFn.execute(vec![ipc], &p(&[])).unwrap();
    assert_eq!(std::str::from_utf8(&ndjson).unwrap().lines().count(), 3);
}

#[test]
fn parquet_to_arrow_invalid() {
    assert!(ParquetToArrowFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn parquet_to_arrow_no_input() {
    assert!(ParquetToArrowFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn parquet_to_arrow_id() {
    assert_eq!(ParquetToArrowFn.id().name, "parquet_to_arrow");
}

// ---- arrow_to_parquet (5 tests) ----
#[test]
fn arrow_to_parquet_with_compression() {
    let ipc = sample_ipc();
    let pq = ArrowToParquetFn.execute(vec![ipc], &p(&[("compression", "snappy")])).unwrap();
    assert!(ParquetReadFn.execute(vec![pq], &p(&[])).is_ok());
}

#[test]
fn arrow_to_parquet_roundtrip() {
    let ipc = sample_ipc();
    let pq = ArrowToParquetFn.execute(vec![ipc.clone()], &p(&[])).unwrap();
    let ipc2 = ParquetToArrowFn.execute(vec![pq], &p(&[])).unwrap();
    let ndjson = ColumnarToRowFn.execute(vec![ipc2], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&ndjson).unwrap().contains("Alice"));
}

#[test]
fn arrow_to_parquet_invalid() {
    assert!(ArrowToParquetFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn arrow_to_parquet_no_input() {
    assert!(ArrowToParquetFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn arrow_to_parquet_id() {
    assert_eq!(ArrowToParquetFn.id().name, "arrow_to_parquet");
}
