use bytes::Bytes;
use deriva_compute::function::{ComputeFunction, ComputeError};
use deriva_compute::builtins_format_detect::FormatValidateFn;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn params(format: &str) -> BTreeMap<String, Value> {
    let mut p = BTreeMap::new();
    p.insert("format".into(), Value::String(format.into()));
    p
}

#[test]
fn validate_valid_json_object() {
    let data = br#"{"users":[{"name":"alice","age":30},{"name":"bob","age":25}]}"#;
    let result = FormatValidateFn.execute(vec![Bytes::from(&data[..])], &params("json"));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Bytes::from(&data[..])); // pass-through
}

#[test]
fn validate_invalid_json_returns_error() {
    let data = b"{not valid json at all";
    let result = FormatValidateFn.execute(vec![Bytes::from(&data[..])], &params("json"));
    assert!(result.is_err());
    match result.unwrap_err() {
        ComputeError::ExecutionFailed(msg) => assert!(msg.contains("not valid json")),
        other => panic!("expected ExecutionFailed, got {:?}", other),
    }
}

#[test]
fn validate_csv_with_headers_and_rows() {
    let data = b"id,name,score\n1,alice,95\n2,bob,87\n";
    let result = FormatValidateFn.execute(vec![Bytes::from(&data[..])], &params("csv"));
    assert!(result.is_ok());
}

#[test]
fn validate_yaml_complex_document() {
    let data = b"services:\n  web:\n    image: nginx\n    ports:\n      - 80:80\n  db:\n    image: postgres\n";
    let result = FormatValidateFn.execute(vec![Bytes::from(&data[..])], &params("yaml"));
    assert!(result.is_ok());
}

#[test]
fn validate_unknown_format_returns_invalid_param() {
    let data = b"some data";
    let result = FormatValidateFn.execute(vec![Bytes::from(&data[..])], &params("foobar"));
    assert!(result.is_err());
    match result.unwrap_err() {
        ComputeError::InvalidParam(msg) => assert!(msg.contains("unknown format: foobar")),
        other => panic!("expected InvalidParam, got {:?}", other),
    }
}
