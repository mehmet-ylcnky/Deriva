use bytes::Bytes;
use deriva_compute::function::{ComputeFunction, ComputeError};
use deriva_compute::builtins_format_detect::SchemaInferFn;
use std::collections::BTreeMap;

#[test]
fn infer_csv_with_mixed_types() {
    let csv = b"name,age,score,active\nAlice,30,95.5,true\nBob,25,87.0,false\nCharlie,35,92.3,true\n";
    let result = SchemaInferFn.execute(vec![Bytes::from(&csv[..])], &BTreeMap::new()).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["name"]["type"], "string");
    assert_eq!(schema["properties"]["age"]["type"], "integer");
    assert_eq!(schema["properties"]["score"]["type"], "number");
    assert_eq!(schema["properties"]["active"]["type"], "boolean");
}

#[test]
fn infer_csv_with_all_null_column() {
    let csv = b"id,value\n1,\n2,\n3,\n";
    let result = SchemaInferFn.execute(vec![Bytes::from(&csv[..])], &BTreeMap::new()).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(schema["properties"]["id"]["type"], "integer");
    assert_eq!(schema["properties"]["value"]["type"], "string"); // all-empty → string
}

#[test]
fn infer_json_object_types() {
    let json = br#"[{"name":"alice","age":30,"score":95.5,"active":true}]"#;
    let result = SchemaInferFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(schema["properties"]["name"]["type"], "string");
    assert_eq!(schema["properties"]["age"]["type"], "integer");
    assert_eq!(schema["properties"]["active"]["type"], "boolean");
}

#[test]
fn infer_json_with_nested_objects() {
    let json = br#"{"config":{"db":{"host":"localhost"}},"items":[1,2,3]}"#;
    let result = SchemaInferFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(schema["properties"]["config"]["type"], "object");
    assert_eq!(schema["properties"]["items"]["type"], "array");
}

#[test]
fn infer_non_tabular_returns_error() {
    let png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let result = SchemaInferFn.execute(vec![Bytes::from(png)], &BTreeMap::new());
    assert!(result.is_err());
    match result.unwrap_err() {
        ComputeError::InvalidParam(msg) => assert!(msg.contains("tabular format")),
        other => panic!("expected InvalidParam, got {:?}", other),
    }
}
