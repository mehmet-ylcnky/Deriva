//! Property-based tests for config format round-trip correctness.
//!
//! **Validates: Requirements 4.4**
//!
//! Property 3: Format round-trip (config subset) — write then parse produces
//! semantically equivalent data for YAML and TOML formats.

use bytes::Bytes;
use deriva_compute::builtins_format_config::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use proptest::prelude::*;
use std::collections::BTreeMap;

fn empty_params() -> BTreeMap<String, Value> {
    BTreeMap::new()
}

fn convert_params(from: &str, to: &str) -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    m.insert("from".to_string(), Value::String(from.to_string()));
    m.insert("to".to_string(), Value::String(to.to_string()));
    m
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/// Alphanumeric key name suitable for all config formats.
/// Starts with a letter, followed by lowercase alphanumeric chars.
fn config_key() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z][a-z0-9]{0,7}")
        .unwrap()
}

/// Simple string value: alphanumeric only (avoids YAML/TOML special chars).
fn simple_string_value() -> impl Strategy<Value = serde_json::Value> {
    proptest::string::string_regex("[a-zA-Z0-9]{1,12}")
        .unwrap()
        .prop_map(|s| serde_json::Value::String(s))
}

/// Simple integer value (positive, small range to avoid TOML issues).
fn simple_int_value() -> impl Strategy<Value = serde_json::Value> {
    (1i64..=10000).prop_map(|n| serde_json::json!(n))
}

/// Simple bool value.
fn simple_bool_value() -> impl Strategy<Value = serde_json::Value> {
    proptest::bool::ANY.prop_map(|b| serde_json::Value::Bool(b))
}

/// A leaf value: string, integer, or boolean.
fn leaf_value() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        simple_string_value(),
        simple_int_value(),
        simple_bool_value(),
    ]
}

/// Generate a flat JSON object with simple key-value pairs.
/// Suitable for both YAML and TOML top-level structures.
fn flat_config_object() -> impl Strategy<Value = serde_json::Value> {
    proptest::collection::btree_map(config_key(), leaf_value(), 1..=6)
        .prop_map(|map| serde_json::Value::Object(map.into_iter().collect()))
}

/// Generate a nested config object (one level of nesting).
/// TOML requires values in nested tables to also be tables or simple values.
fn nested_config_object() -> impl Strategy<Value = serde_json::Value> {
    let inner_table = proptest::collection::btree_map(config_key(), leaf_value(), 1..=4)
        .prop_map(|map| serde_json::Value::Object(map.into_iter().collect()));

    proptest::collection::btree_map(
        config_key(),
        prop_oneof![
            leaf_value(),
            inner_table,
        ],
        1..=5,
    )
    .prop_map(|map| serde_json::Value::Object(map.into_iter().collect()))
}

// ---------------------------------------------------------------------------
// Helper: normalize JSON for comparison
// ---------------------------------------------------------------------------

/// Normalize a JSON value for semantic comparison.
/// serde_yaml may parse integers as i64 but represent them differently on output.
/// This normalizes all numbers to f64 for comparison, and sorts object keys.
fn normalize_json(v: &serde_json::Value) -> serde_json::Value {
    match v {
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
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------
// Property Tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 4.4**
    ///
    /// YAML write → YAML parse produces semantically equivalent data.
    /// We generate random JSON objects, write to YAML via YamlWriteFn,
    /// parse back via YamlParseFn, and verify the data matches.
    #[test]
    fn roundtrip_yaml_write_then_parse(obj in flat_config_object()) {
        let input_json = serde_json::to_string(&obj).unwrap();

        // Write to YAML
        let yaml_bytes = YamlWriteFn
            .execute(vec![Bytes::from(input_json)], &empty_params())
            .unwrap();

        // Parse back from YAML
        let parsed_bytes = YamlParseFn
            .execute(vec![yaml_bytes], &empty_params())
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&parsed_bytes).unwrap();

        let original_norm = normalize_json(&obj);
        let parsed_norm = normalize_json(&parsed);

        prop_assert_eq!(&original_norm, &parsed_norm,
            "YAML round-trip mismatch.\nOriginal: {}\nParsed: {}",
            serde_json::to_string_pretty(&obj).unwrap(),
            serde_json::to_string_pretty(&parsed).unwrap());
    }

    /// **Validates: Requirements 4.4**
    ///
    /// TOML write → TOML parse produces semantically equivalent data.
    /// TOML requires top-level tables (objects), so we generate objects with
    /// simple values and nested tables.
    #[test]
    fn roundtrip_toml_write_then_parse(obj in nested_config_object()) {
        let input_json = serde_json::to_string(&obj).unwrap();

        // Write to TOML
        let toml_bytes = TomlWriteFn
            .execute(vec![Bytes::from(input_json)], &empty_params())
            .unwrap();

        // Parse back from TOML
        let parsed_bytes = TomlParseFn
            .execute(vec![toml_bytes], &empty_params())
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&parsed_bytes).unwrap();

        let original_norm = normalize_json(&obj);
        let parsed_norm = normalize_json(&parsed);

        prop_assert_eq!(&original_norm, &parsed_norm,
            "TOML round-trip mismatch.\nOriginal: {}\nParsed: {}",
            serde_json::to_string_pretty(&obj).unwrap(),
            serde_json::to_string_pretty(&parsed).unwrap());
    }

    /// **Validates: Requirements 4.4**
    ///
    /// ConfigFormatConvertFn round-trip: yaml → json → yaml produces equivalent data.
    /// Convert from YAML to JSON, then JSON back to YAML, and verify the structure matches.
    #[test]
    fn roundtrip_convert_yaml_json_yaml(obj in flat_config_object()) {
        // First write original as YAML
        let input_json = serde_json::to_string(&obj).unwrap();
        let yaml_bytes = YamlWriteFn
            .execute(vec![Bytes::from(input_json)], &empty_params())
            .unwrap();

        // Convert YAML → JSON
        let json_bytes = ConfigFormatConvertFn
            .execute(vec![yaml_bytes.clone()], &convert_params("yaml", "json"))
            .unwrap();

        // Convert JSON → YAML
        let yaml_back = ConfigFormatConvertFn
            .execute(vec![json_bytes], &convert_params("json", "yaml"))
            .unwrap();

        // Parse both YAMLs to compare semantically
        let parsed_original = YamlParseFn
            .execute(vec![yaml_bytes], &empty_params())
            .unwrap();
        let parsed_roundtrip = YamlParseFn
            .execute(vec![yaml_back], &empty_params())
            .unwrap();

        let orig: serde_json::Value = serde_json::from_slice(&parsed_original).unwrap();
        let back: serde_json::Value = serde_json::from_slice(&parsed_roundtrip).unwrap();

        prop_assert_eq!(&normalize_json(&orig), &normalize_json(&back),
            "YAML→JSON→YAML round-trip mismatch.");
    }

    /// **Validates: Requirements 4.4**
    ///
    /// ConfigFormatConvertFn round-trip: toml → json → toml produces equivalent data.
    /// Convert from TOML to JSON, then JSON back to TOML, and verify the structure matches.
    #[test]
    fn roundtrip_convert_toml_json_toml(obj in nested_config_object()) {
        // First write original as TOML
        let input_json = serde_json::to_string(&obj).unwrap();
        let toml_bytes = TomlWriteFn
            .execute(vec![Bytes::from(input_json)], &empty_params())
            .unwrap();

        // Convert TOML → JSON
        let json_bytes = ConfigFormatConvertFn
            .execute(vec![toml_bytes.clone()], &convert_params("toml", "json"))
            .unwrap();

        // Convert JSON → TOML
        let toml_back = ConfigFormatConvertFn
            .execute(vec![json_bytes], &convert_params("json", "toml"))
            .unwrap();

        // Parse both TOMLs to compare semantically
        let parsed_original = TomlParseFn
            .execute(vec![toml_bytes], &empty_params())
            .unwrap();
        let parsed_roundtrip = TomlParseFn
            .execute(vec![toml_back], &empty_params())
            .unwrap();

        let orig: serde_json::Value = serde_json::from_slice(&parsed_original).unwrap();
        let back: serde_json::Value = serde_json::from_slice(&parsed_roundtrip).unwrap();

        prop_assert_eq!(&normalize_json(&orig), &normalize_json(&back),
            "TOML→JSON→TOML round-trip mismatch.");
    }
}
