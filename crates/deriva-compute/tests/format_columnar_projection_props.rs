//! Property-based tests for Parquet column projection subset (Property 5).
//!
//! **Validates: Requirements 5.1**
//!
//! For any random subset of columns selected from a known Parquet schema,
//! ParquetProjectionFn SHALL produce output containing exactly the requested
//! columns (no extra, no missing) with row counts preserved.

#![cfg(feature = "format-columnar")]

use bytes::Bytes;
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

use deriva_compute::builtins_format_columnar::{
    ParquetProjectionFn, ParquetReadFn, ParquetWriteFn,
};
use deriva_compute::function::ComputeFunction;

use arrow::array::{
    BooleanArray, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::ipc::writer::StreamWriter;

// ---------------------------------------------------------------------------
// Test schema: 6 columns with different types
// ---------------------------------------------------------------------------

const COLUMN_NAMES: &[&str] = &["id", "name", "age", "score", "active", "timestamp"];

fn test_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("age", DataType::Int32, true),
        Field::new("score", DataType::Float64, true),
        Field::new("active", DataType::Boolean, false),
        Field::new("timestamp", DataType::Int64, false),
    ]))
}

/// Create a sample RecordBatch with the test schema and given number of rows.
fn make_record_batch(num_rows: usize) -> RecordBatch {
    let ids: Vec<i32> = (0..num_rows as i32).collect();
    let names: Vec<String> = (0..num_rows).map(|i| format!("row_{i}")).collect();
    let ages: Vec<Option<i32>> = (0..num_rows).map(|i| if i % 3 == 0 { None } else { Some(20 + (i as i32) % 50) }).collect();
    let scores: Vec<Option<f64>> = (0..num_rows).map(|i| if i % 5 == 0 { None } else { Some((i as f64) * 1.5) }).collect();
    let actives: Vec<bool> = (0..num_rows).map(|i| i % 2 == 0).collect();
    let timestamps: Vec<i64> = (0..num_rows).map(|i| 1_700_000_000 + i as i64).collect();

    RecordBatch::try_new(
        test_schema(),
        vec![
            Arc::new(Int32Array::from(ids)),
            Arc::new(StringArray::from(names.iter().map(|s| s.as_str()).collect::<Vec<_>>())),
            Arc::new(Int32Array::from(ages)),
            Arc::new(Float64Array::from(scores)),
            Arc::new(BooleanArray::from(actives)),
            Arc::new(Int64Array::from(timestamps)),
        ],
    )
    .unwrap()
}

/// Serialize a RecordBatch to Arrow IPC bytes.
fn to_ipc_bytes(batch: &RecordBatch) -> Bytes {
    let schema = batch.schema();
    let mut buf = Vec::new();
    let mut writer = StreamWriter::try_new(&mut buf, &schema).unwrap();
    writer.write(batch).unwrap();
    writer.finish().unwrap();
    Bytes::from(buf)
}

/// Create a Parquet file (as Bytes) from a RecordBatch via ParquetWriteFn.
fn make_parquet(batch: &RecordBatch) -> Bytes {
    let ipc = to_ipc_bytes(batch);
    ParquetWriteFn
        .execute(vec![ipc], &BTreeMap::new())
        .expect("ParquetWriteFn should succeed")
}

/// Read a Parquet file back and return column names and row count.
fn read_parquet_columns_and_rows(parquet_data: &Bytes) -> (Vec<String>, usize) {
    let ipc = ParquetReadFn
        .execute(vec![parquet_data.clone()], &BTreeMap::new())
        .expect("ParquetReadFn should succeed");

    let cursor = std::io::Cursor::new(ipc.as_ref());
    let reader = arrow::ipc::reader::StreamReader::try_new(cursor, None).unwrap();
    let schema = reader.schema();
    let col_names: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();

    let batches: Vec<RecordBatch> = reader.into_iter().map(|r| r.unwrap()).collect();
    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();

    (col_names, total_rows)
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Strategy to generate a non-empty subset of column indices from COLUMN_NAMES.
fn column_subset_strategy() -> impl Strategy<Value = Vec<usize>> {
    // Generate a bitmask of length 6, ensuring at least one bit is set
    prop::collection::vec(any::<bool>(), 6..=6).prop_filter_map(
        "at least one column must be selected",
        |bitmask| {
            let selected: Vec<usize> = bitmask
                .iter()
                .enumerate()
                .filter_map(|(i, &b)| if b { Some(i) } else { None })
                .collect();
            if selected.is_empty() {
                None
            } else {
                Some(selected)
            }
        },
    )
}

/// Strategy to generate a row count between 1 and 50.
fn row_count_strategy() -> impl Strategy<Value = usize> {
    1_usize..=50
}

// ---------------------------------------------------------------------------
// Property 5: Column projection subset
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 5.1**
    ///
    /// For any random subset of columns selected from a Parquet file with 6
    /// typed columns, ParquetProjectionFn SHALL produce output containing
    /// exactly the requested columns (no extra, no missing) and preserve
    /// the original row count.
    #[test]
    fn projection_contains_exactly_requested_columns(
        col_indices in column_subset_strategy(),
        num_rows in row_count_strategy(),
    ) {
        // Build the expected column names from indices
        let expected_columns: Vec<&str> = col_indices
            .iter()
            .map(|&i| COLUMN_NAMES[i])
            .collect();

        // Create test data
        let batch = make_record_batch(num_rows);
        let parquet_data = make_parquet(&batch);

        // Build the columns parameter (comma-separated)
        let columns_param = expected_columns.join(",");
        let mut params = BTreeMap::new();
        params.insert(
            "columns".to_string(),
            deriva_core::address::Value::String(columns_param),
        );

        // Execute projection
        let projected = ParquetProjectionFn
            .execute(vec![parquet_data], &params)
            .expect("ParquetProjectionFn should succeed");

        // Read back the projected result
        let (actual_columns, actual_rows) = read_parquet_columns_and_rows(&Bytes::from(projected));

        // Verify: output contains exactly the requested columns
        let expected_set: std::collections::HashSet<&str> =
            expected_columns.iter().copied().collect();
        let actual_set: std::collections::HashSet<String> =
            actual_columns.iter().cloned().collect();

        // No missing columns
        for col in &expected_columns {
            prop_assert!(
                actual_set.contains(*col),
                "Missing column '{}' in projection output. Expected: {:?}, Got: {:?}",
                col,
                expected_columns,
                actual_columns
            );
        }

        // No extra columns
        for col in &actual_columns {
            prop_assert!(
                expected_set.contains(col.as_str()),
                "Extra column '{}' in projection output. Expected: {:?}, Got: {:?}",
                col,
                expected_columns,
                actual_columns
            );
        }

        // Verify column count matches exactly
        prop_assert_eq!(
            actual_columns.len(),
            expected_columns.len(),
            "Column count mismatch. Expected {} columns {:?}, got {} columns {:?}",
            expected_columns.len(),
            expected_columns,
            actual_columns.len(),
            actual_columns
        );

        // Verify row count is preserved
        prop_assert_eq!(
            actual_rows,
            num_rows,
            "Row count not preserved. Expected {} rows, got {}",
            num_rows,
            actual_rows
        );
    }
}
