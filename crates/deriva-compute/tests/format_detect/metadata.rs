use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_detect::UniversalMetadataFn;
use std::collections::BTreeMap;

fn metadata(input: &[u8]) -> serde_json::Value {
    let result = UniversalMetadataFn.execute(vec![Bytes::from(input.to_vec())], &BTreeMap::new()).unwrap();
    serde_json::from_slice(&result).unwrap()
}

#[test]
fn metadata_csv_reports_columns_and_row_count() {
    let csv = b"name,age,city\nAlice,30,NYC\nBob,25,SF\nCharlie,35,LA\n";
    let m = metadata(csv);
    assert_eq!(m["format"], "csv");
    assert_eq!(m["metadata"]["columns"], serde_json::json!(["name", "age", "city"]));
    assert_eq!(m["metadata"]["row_count"], 3);
}

#[test]
fn metadata_json_array_reports_type() {
    let json = b"[1, 2, 3]";
    let m = metadata(json);
    assert_eq!(m["format"], "json");
    assert_eq!(m["metadata"]["type"], "array");
}

#[test]
fn metadata_json_object_reports_type() {
    let json = br#"{"key": "value", "nested": {"a": 1}}"#;
    let m = metadata(json);
    assert_eq!(m["format"], "json");
    assert_eq!(m["metadata"]["type"], "object");
}

#[test]
fn metadata_toml_reports_top_level_keys() {
    let toml = b"[database]\nhost = \"localhost\"\nport = 5432\n\n[server]\nworkers = 4\n";
    let m = metadata(toml);
    assert_eq!(m["format"], "toml");
    let keys = m["metadata"]["top_level_keys"].as_array().unwrap();
    assert!(keys.contains(&serde_json::json!("database")));
    assert!(keys.contains(&serde_json::json!("server")));
}

#[test]
fn metadata_binary_reports_size_only() {
    let png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00];
    let m = metadata(&png);
    assert_eq!(m["format"], "png");
    assert_eq!(m["metadata"]["size_bytes"], 10);
}
