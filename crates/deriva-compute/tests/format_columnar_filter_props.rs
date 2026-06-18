//! Property-based tests for predicate pushdown correctness (Property 4).
//!
//! **Validates: Requirements 5.4**
//!
//! For any Parquet file and filter predicate, the result of `parquet_filter`
//! SHALL contain exactly the rows that satisfy the predicate — no false positives
//! (incorrect rows included) and no false negatives (correct rows excluded).

#![cfg(feature = "format-columnar")]

use bytes::Bytes;
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

use arrow::array::{Int32Array, RecordBatch};
use arrow::datatypes::{DataType, Field, Schema};

use deriva_compute::builtins_format_columnar::{
    ColumnarToRowFn, ParquetFilterFn, ParquetWriteFn,
};
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
        .collect()
}

/// Create IPC bytes from a single Int32 column named "val".
fn make_ipc(values: &[i32]) -> Bytes {
    let schema = Arc::new(Schema::new(vec![Field::new("val", DataType::Int32, false)]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![Arc::new(Int32Array::from(values.to_vec()))],
    )
    .unwrap();
    let mut buf = Vec::new();
    let mut w = arrow::ipc::writer::StreamWriter::try_new(&mut buf, &schema).unwrap();
    w.write(&batch).unwrap();
    w.finish().unwrap();
    Bytes::from(buf)
}

/// Apply the predicate locally to compute expected results.
fn expected_filter(values: &[i32], op: &str, threshold: i32) -> Vec<i32> {
    values
        .iter()
        .copied()
        .filter(|&v| match op {
            "eq" => v == threshold,
            "ne" => v != threshold,
            "lt" => v < threshold,
            "le" => v <= threshold,
            "gt" => v > threshold,
            "ge" => v >= threshold,
            _ => unreachable!(),
        })
        .collect()
}

/// Parse NDJSON output to extract the "val" column values.
fn parse_ndjson_vals(ndjson: &[u8]) -> Vec<i32> {
    let text = std::str::from_utf8(ndjson).unwrap();
    if text.is_empty() {
        return Vec::new();
    }
    text.lines()
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            v["val"].as_i64().unwrap() as i32
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Strategy for filter operator names.
fn op_strategy() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("eq"),
        Just("ne"),
        Just("lt"),
        Just("le"),
        Just("gt"),
        Just("ge"),
    ]
}

// ---------------------------------------------------------------------------
// Property 4: Predicate pushdown correctness
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 5.4**
    ///
    /// For any random Int32 data and random filter predicate (op, threshold),
    /// ParquetFilterFn SHALL produce exactly the rows satisfying the predicate:
    /// - No false positives (every output row satisfies the predicate)
    /// - No false negatives (output count matches manual filtering of the original)
    #[test]
    fn filter_predicate_pushdown_correctness(
        values in prop::collection::vec(-1000i32..1000i32, 1..100),
        op in op_strategy(),
        threshold in -1000i32..1000i32,
    ) {
        // 1. Create Parquet from random data
        let ipc = make_ipc(&values);
        let parquet = ParquetWriteFn.execute(vec![ipc], &params(&[])).unwrap();

        // 2. Apply filter via ParquetFilterFn
        let threshold_str = threshold.to_string();
        let filtered_parquet = ParquetFilterFn.execute(
            vec![parquet],
            &params(&[("column", "val"), ("op", op), ("value", &threshold_str)]),
        ).unwrap();

        // 3. Convert filtered result to NDJSON for inspection
        let ndjson = ColumnarToRowFn.execute(vec![filtered_parquet], &params(&[])).unwrap();
        let actual_vals = parse_ndjson_vals(&ndjson);

        // 4. Compute expected values by manual filtering
        let expected_vals = expected_filter(&values, op, threshold);

        // 5. Verify no false negatives: counts must match
        prop_assert_eq!(
            actual_vals.len(),
            expected_vals.len(),
            "Row count mismatch for op='{}' threshold={}: expected {} rows, got {}",
            op, threshold, expected_vals.len(), actual_vals.len()
        );

        // 6. Verify no false positives: every output row satisfies the predicate
        for (i, &val) in actual_vals.iter().enumerate() {
            let satisfies = match op {
                "eq" => val == threshold,
                "ne" => val != threshold,
                "lt" => val < threshold,
                "le" => val <= threshold,
                "gt" => val > threshold,
                "ge" => val >= threshold,
                _ => unreachable!(),
            };
            prop_assert!(
                satisfies,
                "False positive at row {}: val={} does not satisfy op='{}' threshold={}",
                i, val, op, threshold
            );
        }

        // 7. Verify exact match of values (preserving order)
        prop_assert_eq!(
            actual_vals,
            expected_vals,
            "Filtered values differ from expected for op='{}' threshold={}",
            op, threshold
        );
    }
}
