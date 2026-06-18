//! Property-based tests for serialization format round-trip (Property 3, serialization subset).
//!
//! **Validates: Requirements 5.2**
//!
//! For any random JSON-serializable data, encoding then decoding through
//! Avro, CBOR, and MessagePack SHALL produce data semantically equivalent
//! to the original input.

#![cfg(feature = "format-serialization")]

use bytes::Bytes;
use proptest::prelude::*;
use std::collections::BTreeMap;

use deriva_compute::builtins_format_serialization::{
    AvroReadFn, AvroWriteFn, CborDecodeFn, CborEncodeFn, MsgpackDecodeFn, MsgpackEncodeFn,
};
use deriva_compute::function::ComputeFunction;

// ---------------------------------------------------------------------------
// Avro schema for property tests: a simple record with string + int fields
// ---------------------------------------------------------------------------

const AVRO_SCHEMA: &str = r#"{"type":"record","name":"Test","fields":[{"name":"name","type":"string"},{"name":"value","type":"int"}]}"#;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Strategy to generate a random string suitable for Avro "string" type.
/// Avoids empty strings to keep generated data interesting.
fn avro_string_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_ ]{1,50}"
}

/// Strategy to generate a random i32 value for the Avro "int" field.
fn avro_int_strategy() -> impl Strategy<Value = i32> {
    any::<i32>()
}

/// Strategy to generate a vector of Avro-compatible records (as JSON objects).
/// Each record has {"name": <string>, "value": <int>}.
fn avro_records_strategy() -> impl Strategy<Value = Vec<serde_json::Value>> {
    prop::collection::vec(
        (avro_string_strategy(), avro_int_strategy()).prop_map(|(name, value)| {
            serde_json::json!({"name": name, "value": value})
        }),
        1..=20,
    )
}

/// Strategy to generate a JSON value suitable for CBOR/MsgPack round-trip.
/// Generates objects with string keys and values that are strings, integers, or booleans.
fn json_value_strategy() -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(|b| serde_json::Value::Bool(b)),
        (-1_000_000i64..1_000_000i64).prop_map(|n| serde_json::json!(n)),
        "[a-zA-Z0-9_ ]{0,30}".prop_map(|s| serde_json::Value::String(s)),
        Just(serde_json::Value::Null),
    ];

    // Generate a JSON object with 1-10 key-value pairs
    prop::collection::btree_map("[a-z]{1,8}", leaf, 1..=10)
        .prop_map(|map| {
            let obj: serde_json::Map<String, serde_json::Value> = map.into_iter().collect();
            serde_json::Value::Object(obj)
        })
}

// ---------------------------------------------------------------------------
// Property 3: Avro write → Avro read round-trip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 5.2**
    ///
    /// For any random list of records conforming to the Avro schema,
    /// AvroWriteFn → AvroReadFn SHALL produce the same records.
    #[test]
    fn avro_write_read_roundtrip(records in avro_records_strategy()) {
        let params = BTreeMap::new();

        // Serialize records to JSON bytes
        let json_bytes = Bytes::from(serde_json::to_string(&records).unwrap());
        let schema_bytes = Bytes::from(AVRO_SCHEMA.to_string());

        // Avro write: JSON array + schema → Avro OCF bytes
        let avro_bytes = AvroWriteFn
            .execute(vec![json_bytes, schema_bytes], &params)
            .expect("AvroWriteFn should succeed");

        // Avro read: Avro OCF bytes → JSON array bytes
        let result_bytes = AvroReadFn
            .execute(vec![avro_bytes], &params)
            .expect("AvroReadFn should succeed");

        // Parse the result back to JSON
        let result_str = std::str::from_utf8(&result_bytes).expect("result should be UTF-8");
        let result: Vec<serde_json::Value> = serde_json::from_str(result_str)
            .expect("result should be valid JSON array");

        // Verify same number of records
        prop_assert_eq!(
            result.len(),
            records.len(),
            "Record count mismatch. Expected {}, got {}",
            records.len(),
            result.len()
        );

        // Verify each record matches
        for (i, (expected, actual)) in records.iter().zip(result.iter()).enumerate() {
            let expected_name = expected.get("name").and_then(|v| v.as_str());
            let actual_name = actual.get("name").and_then(|v| v.as_str());
            prop_assert_eq!(
                actual_name, expected_name,
                "Record {} name mismatch", i
            );

            let expected_value = expected.get("value").and_then(|v| v.as_i64());
            let actual_value = actual.get("value").and_then(|v| v.as_i64());
            prop_assert_eq!(
                actual_value, expected_value,
                "Record {} value mismatch", i
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Property 3: CBOR encode → CBOR decode round-trip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 5.2**
    ///
    /// For any random JSON object, CborEncodeFn → CborDecodeFn SHALL produce
    /// a JSON value semantically equivalent to the original.
    #[test]
    fn cbor_encode_decode_roundtrip(json_val in json_value_strategy()) {
        let params = BTreeMap::new();

        // Encode JSON → CBOR
        let json_bytes = Bytes::from(serde_json::to_string(&json_val).unwrap());
        let cbor_bytes = CborEncodeFn
            .execute(vec![json_bytes], &params)
            .expect("CborEncodeFn should succeed");

        // Decode CBOR → JSON
        let result_bytes = CborDecodeFn
            .execute(vec![cbor_bytes], &params)
            .expect("CborDecodeFn should succeed");

        let result_str = std::str::from_utf8(&result_bytes).expect("result should be UTF-8");
        let result: serde_json::Value = serde_json::from_str(result_str)
            .expect("result should be valid JSON");

        // Compare: the CBOR round-trip should preserve structure and values.
        // Note: CBOR integers come back via i128 but JSON only has Number.
        // We compare by normalizing both sides to canonical JSON strings.
        let expected_normalized = normalize_json(&json_val);
        let actual_normalized = normalize_json(&result);

        prop_assert_eq!(
            actual_normalized,
            expected_normalized,
            "CBOR round-trip mismatch.\nOriginal: {}\nResult: {}",
            serde_json::to_string_pretty(&json_val).unwrap(),
            serde_json::to_string_pretty(&result).unwrap()
        );
    }
}

// ---------------------------------------------------------------------------
// Property 3: MsgPack encode → MsgPack decode round-trip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 5.2**
    ///
    /// For any random JSON value, MsgpackEncodeFn → MsgpackDecodeFn SHALL
    /// produce a JSON value semantically equivalent to the original.
    #[test]
    fn msgpack_encode_decode_roundtrip(json_val in json_value_strategy()) {
        let params = BTreeMap::new();

        // Encode JSON → MessagePack
        let json_bytes = Bytes::from(serde_json::to_string(&json_val).unwrap());
        let msgpack_bytes = MsgpackEncodeFn
            .execute(vec![json_bytes], &params)
            .expect("MsgpackEncodeFn should succeed");

        // Decode MessagePack → JSON
        let result_bytes = MsgpackDecodeFn
            .execute(vec![msgpack_bytes], &params)
            .expect("MsgpackDecodeFn should succeed");

        let result_str = std::str::from_utf8(&result_bytes).expect("result should be UTF-8");
        let result: serde_json::Value = serde_json::from_str(result_str)
            .expect("result should be valid JSON");

        // Compare normalized JSON values
        let expected_normalized = normalize_json(&json_val);
        let actual_normalized = normalize_json(&result);

        prop_assert_eq!(
            actual_normalized,
            expected_normalized,
            "MsgPack round-trip mismatch.\nOriginal: {}\nResult: {}",
            serde_json::to_string_pretty(&json_val).unwrap(),
            serde_json::to_string_pretty(&result).unwrap()
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Normalize a JSON value for comparison purposes.
/// - Sorts object keys (already sorted by serde_json::Map)
/// - Converts integer representations to a canonical form
fn normalize_json(val: &serde_json::Value) -> serde_json::Value {
    match val {
        serde_json::Value::Object(map) => {
            let normalized: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), normalize_json(v)))
                .collect();
            serde_json::Value::Object(normalized)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(normalize_json).collect())
        }
        serde_json::Value::Number(n) => {
            // Normalize all integers to i64 representation
            if let Some(i) = n.as_i64() {
                serde_json::json!(i)
            } else if let Some(u) = n.as_u64() {
                serde_json::json!(u)
            } else {
                val.clone()
            }
        }
        _ => val.clone(),
    }
}
