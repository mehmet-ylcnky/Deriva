use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_detect::SchemaCompareFn;
use std::collections::BTreeMap;

fn compare(s1: &str, s2: &str) -> serde_json::Value {
    let result = SchemaCompareFn.execute(
        vec![Bytes::from(s1.to_string()), Bytes::from(s2.to_string())],
        &BTreeMap::new(),
    ).unwrap();
    serde_json::from_slice(&result).unwrap()
}

#[test]
fn identical_schemas_are_compatible() {
    let schema = r#"{"type":"object","properties":{"name":{"type":"string"},"age":{"type":"integer"}}}"#;
    let r = compare(schema, schema);
    assert_eq!(r["compatible"], true);
    assert!(r["changes"].as_array().unwrap().is_empty());
}

#[test]
fn added_field_detected() {
    let s1 = r#"{"type":"object","properties":{"name":{"type":"string"}}}"#;
    let s2 = r#"{"type":"object","properties":{"name":{"type":"string"},"email":{"type":"string"}}}"#;
    let r = compare(s1, s2);
    assert_eq!(r["compatible"], false);
    let changes = r["changes"].as_array().unwrap();
    assert!(changes.iter().any(|c| c["field"] == "email" && c["type"] == "added"));
}

#[test]
fn removed_field_detected() {
    let s1 = r#"{"type":"object","properties":{"name":{"type":"string"},"age":{"type":"integer"}}}"#;
    let s2 = r#"{"type":"object","properties":{"name":{"type":"string"}}}"#;
    let r = compare(s1, s2);
    assert_eq!(r["compatible"], false);
    let changes = r["changes"].as_array().unwrap();
    assert!(changes.iter().any(|c| c["field"] == "age" && c["type"] == "removed"));
}

#[test]
fn type_change_detected() {
    let s1 = r#"{"type":"object","properties":{"score":{"type":"integer"}}}"#;
    let s2 = r#"{"type":"object","properties":{"score":{"type":"number"}}}"#;
    let r = compare(s1, s2);
    assert_eq!(r["compatible"], false);
    let changes = r["changes"].as_array().unwrap();
    assert!(changes.iter().any(|c| c["field"] == "score" && c["type"] == "changed"));
}

#[test]
fn multiple_changes_all_reported() {
    let s1 = r#"{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"integer"},"c":{"type":"boolean"}}}"#;
    let s2 = r#"{"type":"object","properties":{"a":{"type":"number"},"d":{"type":"string"}}}"#;
    let r = compare(s1, s2);
    assert_eq!(r["compatible"], false);
    let changes = r["changes"].as_array().unwrap();
    // a: changed, b: removed, c: removed, d: added
    assert!(changes.len() >= 4);
    assert!(changes.iter().any(|c| c["field"] == "a" && c["type"] == "changed"));
    assert!(changes.iter().any(|c| c["field"] == "b" && c["type"] == "removed"));
    assert!(changes.iter().any(|c| c["field"] == "c" && c["type"] == "removed"));
    assert!(changes.iter().any(|c| c["field"] == "d" && c["type"] == "added"));
}
