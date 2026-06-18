//! Property-based tests for format round-trip correctness.
//!
//! **Validates: Requirements 4.1, 5.1, 5.2**
//!
//! Property 3: Format round-trip — write then parse produces semantically equivalent data.

use bytes::Bytes;
use deriva_compute::builtins_format_csv::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use proptest::prelude::*;
use std::collections::BTreeMap;

fn empty_params() -> BTreeMap<String, Value> {
    BTreeMap::new()
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/// Generate an alphanumeric string (no commas, quotes, or newlines) for safe CSV values.
fn safe_alphanumeric(min_len: usize, max_len: usize) -> impl Strategy<Value = String> {
    proptest::string::string_regex(&format!("[a-zA-Z0-9]{{{},{}}}", min_len, max_len))
        .unwrap()
}

/// Generate a header name: starts with a letter, followed by alphanumeric chars.
fn header_name() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-zA-Z][a-zA-Z0-9]{0,9}")
        .unwrap()
}

/// Generate unique headers (1-5 columns).
fn unique_headers(num_cols: usize) -> impl Strategy<Value = Vec<String>> {
    proptest::collection::hash_set(header_name(), num_cols)
        .prop_map(|s| s.into_iter().collect::<Vec<_>>())
}

/// Generate a single row of alphanumeric values matching the column count.
fn row_values(num_cols: usize) -> impl Strategy<Value = Vec<String>> {
    proptest::collection::vec(safe_alphanumeric(1, 10), num_cols)
}

/// Generate tabular data as a JSON array of objects (the format CsvWriteFn expects).
fn tabular_json_data() -> impl Strategy<Value = (Vec<String>, Vec<Vec<String>>)> {
    (1usize..=5).prop_flat_map(|num_cols| {
        (
            unique_headers(num_cols),
            proptest::collection::vec(row_values(num_cols), 1..=20),
        )
    })
}

/// Generate simple key-value JSON objects for NDJSON testing.
fn simple_json_object() -> impl Strategy<Value = serde_json::Value> {
    proptest::collection::btree_map(
        header_name(),
        safe_alphanumeric(1, 15).prop_map(|s| serde_json::Value::String(s)),
        1..=5,
    )
    .prop_map(|map| {
        serde_json::Value::Object(map.into_iter().collect())
    })
}

/// Generate nested JSON up to 3 levels for flatten/unflatten testing.
/// Only uses object nesting (no arrays) since unflatten only reconstructs objects.
fn nested_json(depth: usize) -> BoxedStrategy<serde_json::Value> {
    if depth == 0 {
        // Leaf: string or number
        prop_oneof![
            safe_alphanumeric(1, 8).prop_map(|s| serde_json::Value::String(s)),
            (1i64..=1000).prop_map(|n| serde_json::json!(n)),
        ]
        .boxed()
    } else {
        prop_oneof![
            // Leaf values
            safe_alphanumeric(1, 8).prop_map(|s| serde_json::Value::String(s)),
            (1i64..=1000).prop_map(|n| serde_json::json!(n)),
            // Nested object
            proptest::collection::btree_map(
                header_name(),
                nested_json(depth - 1),
                1..=3,
            )
            .prop_map(|map| serde_json::Value::Object(map.into_iter().collect())),
        ]
        .boxed()
    }
}

/// Generate a nested JSON object (top-level must be an object for flatten/unflatten).
fn nested_json_object() -> impl Strategy<Value = serde_json::Value> {
    proptest::collection::btree_map(
        header_name(),
        nested_json(2), // up to 3 levels total (root + 2 nested)
        1..=4,
    )
    .prop_map(|map| serde_json::Value::Object(map.into_iter().collect()))
}

// ---------------------------------------------------------------------------
// Property Tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 4.1**
    ///
    /// CSV write → CSV parse produces semantically equivalent data.
    /// We generate random tabular data, serialize to CSV via CsvWriteFn,
    /// parse back via CsvParseFn, and verify row count and cell values match.
    #[test]
    fn roundtrip_csv_write_then_parse(
        (headers, rows) in tabular_json_data()
    ) {
        // Build JSON array input for CsvWriteFn
        let json_rows: Vec<serde_json::Value> = rows.iter().map(|row| {
            let mut obj = serde_json::Map::new();
            for (i, h) in headers.iter().enumerate() {
                obj.insert(h.clone(), serde_json::Value::String(row[i].clone()));
            }
            serde_json::Value::Object(obj)
        }).collect();
        let input_json = serde_json::to_string(&json_rows).unwrap();

        // Write CSV
        let csv_bytes = CsvWriteFn
            .execute(vec![Bytes::from(input_json)], &empty_params())
            .unwrap();

        // Parse CSV back
        let parsed_bytes = CsvParseFn
            .execute(vec![csv_bytes], &empty_params())
            .unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_slice(&parsed_bytes).unwrap();

        // Verify row count
        prop_assert_eq!(parsed.len(), rows.len());

        // Verify each cell value matches
        for (row_idx, row) in rows.iter().enumerate() {
            let parsed_obj = parsed[row_idx].as_object().unwrap();
            for (col_idx, h) in headers.iter().enumerate() {
                let expected = &row[col_idx];
                let actual = parsed_obj.get(h).and_then(|v| v.as_str()).unwrap_or("");
                prop_assert_eq!(actual, expected.as_str(),
                    "Mismatch at row {}, col '{}': expected '{}', got '{}'",
                    row_idx, h, expected, actual);
            }
        }
    }

    /// **Validates: Requirements 5.1, 5.2**
    ///
    /// NDJSON write → NDJSON parse produces identical structure.
    /// We generate random JSON objects, serialize to NDJSON via NdjsonWriteFn,
    /// parse back via NdjsonParseFn, and verify the array matches.
    #[test]
    fn roundtrip_ndjson_write_then_parse(
        objects in proptest::collection::vec(simple_json_object(), 1..=10)
    ) {
        let input_json = serde_json::to_string(&objects).unwrap();

        // Write NDJSON
        let ndjson_bytes = NdjsonWriteFn
            .execute(vec![Bytes::from(input_json)], &empty_params())
            .unwrap();

        // Parse NDJSON back
        let parsed_bytes = NdjsonParseFn
            .execute(vec![ndjson_bytes], &empty_params())
            .unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_slice(&parsed_bytes).unwrap();

        // Verify count
        prop_assert_eq!(parsed.len(), objects.len());

        // Verify each object matches
        for (i, (original, restored)) in objects.iter().zip(parsed.iter()).enumerate() {
            prop_assert_eq!(original, restored,
                "NDJSON round-trip mismatch at index {}", i);
        }
    }

    /// **Validates: Requirements 5.1, 5.2**
    ///
    /// JSON flatten → unflatten produces identical structure.
    /// We generate random nested JSON objects, flatten with JsonFlattenFn,
    /// unflatten with JsonUnflattenFn, and verify the result matches the original.
    #[test]
    fn roundtrip_json_flatten_unflatten(
        obj in nested_json_object()
    ) {
        let input_json = serde_json::to_string(&obj).unwrap();

        // Flatten
        let flat_bytes = JsonFlattenFn
            .execute(vec![Bytes::from(input_json)], &empty_params())
            .unwrap();

        // Unflatten
        let restored_bytes = JsonUnflattenFn
            .execute(vec![flat_bytes], &empty_params())
            .unwrap();
        let restored: serde_json::Value = serde_json::from_slice(&restored_bytes).unwrap();

        prop_assert_eq!(&obj, &restored,
            "JSON flatten/unflatten round-trip mismatch.\nOriginal: {}\nRestored: {}",
            serde_json::to_string_pretty(&obj).unwrap(),
            serde_json::to_string_pretty(&restored).unwrap());
    }
}
