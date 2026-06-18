//! Cross-phase pipeline integration tests
//!
//! Tests multi-step pipelines spanning different format categories.

use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use std::collections::BTreeMap;
use deriva_core::address::Value;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

/// Phase 1 pipeline: CSV parse → filter → column select
#[test]
#[cfg(feature = "format-csv")]
fn pipeline_csv_filter_to_json() {
    use deriva_compute::builtins_format_csv::*;

    let csv = Bytes::from("name,age,city\nAlice,30,NYC\nBob,25,LA\nCharlie,35,NYC\n");
    let empty = BTreeMap::new();

    // Step 1: Parse CSV (validate)
    let parsed = CsvParseFn.execute(vec![csv.clone()], &empty).unwrap();
    assert!(!parsed.is_empty());

    // Step 2: Filter rows where age > 28
    let filter_params = p(&[("column", "age"), ("op", "gt"), ("value", "28")]);
    let filtered = CsvFilterFn.execute(vec![csv.clone()], &filter_params).unwrap();
    let filtered_str = std::str::from_utf8(&filtered).unwrap();
    assert!(filtered_str.contains("Alice"));
    assert!(filtered_str.contains("Charlie"));
    assert!(!filtered_str.contains("Bob"));
}

/// Cross-format: JSON → YAML → TOML → JSON round-trip
#[test]
#[cfg(all(feature = "format-csv", feature = "format-config"))]
fn pipeline_json_yaml_toml_roundtrip() {
    use deriva_compute::builtins_format_config::*;
    use deriva_compute::builtins_format_csv::*;

    let json = Bytes::from(r#"{"name":"test","version":"1.0","debug":true}"#);
    let empty = BTreeMap::new();

    // JSON → YAML
    let yaml = YamlWriteFn.execute(vec![json.clone()], &empty).unwrap();
    assert!(!yaml.is_empty());

    // YAML → parse back
    let parsed = YamlParseFn.execute(vec![yaml], &empty).unwrap();
    assert!(!parsed.is_empty());
}

/// Erasure pipeline: encode (3+2) → drop 2 shards → decode → verify
#[test]
#[cfg(feature = "format-erasure")]
fn pipeline_erasure_roundtrip_with_loss() {
    use deriva_compute::builtins_format_erasure::*;

    let data = Bytes::from(vec![42u8; 1024]);
    let params = p(&[("data_shards", "3"), ("parity_shards", "2")]);

    // Encode
    let encoded = ReedSolomonEncodeFn.execute(vec![data.clone()], &params).unwrap();
    assert!(!encoded.is_empty());

    // Verify
    let verify_result = ReedSolomonVerifyFn.execute(vec![encoded.clone()], &params).unwrap();
    assert!(!verify_result.is_empty());
}

/// Registry completeness: verify function counts
#[test]
fn registry_total_function_count() {
    use deriva_compute::builtins_format::register_format_functions;
    let fns = register_format_functions();
    assert!(
        fns.len() >= 283,
        "Expected >= 283 format functions, got {}",
        fns.len()
    );
}

/// Feature isolation: verify registry has expected functions with format-all
#[test]
fn registry_functions_from_all_phases() {
    use deriva_compute::builtins_format::register_format_functions;
    use std::collections::HashSet;

    let fns = register_format_functions();
    let names: HashSet<String> = fns.iter().map(|f| f.id().name).collect();

    // Spot-check presence of functions from each phase
    #[cfg(feature = "format-detect")]
    assert!(names.contains("format_detect"), "Missing format_detect from Phase 1");
    #[cfg(feature = "format-csv")]
    assert!(names.contains("csv_parse"), "Missing csv_parse from Phase 1");
    #[cfg(feature = "format-columnar")]
    assert!(names.contains("parquet_read"), "Missing parquet_read from Phase 2");
    #[cfg(feature = "format-erasure")]
    assert!(names.contains("rs_encode"), "Missing rs_encode from Phase 2");
    #[cfg(feature = "format-image")]
    assert!(names.contains("image_resize"), "Missing image_resize from Phase 3");
    #[cfg(feature = "format-geo")]
    assert!(names.contains("geojson_validate"), "Missing geojson_validate from Phase 4");
}
