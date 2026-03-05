use bytes::Bytes;
use deriva_compute::function::{ComputeFunction, ComputeError};
use deriva_compute::builtins_format_detect::FormatConvertFn;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn convert_params(from: Option<&str>, to: &str) -> BTreeMap<String, Value> {
    let mut p = BTreeMap::new();
    if let Some(f) = from {
        p.insert("from".into(), Value::String(f.into()));
    }
    p.insert("to".into(), Value::String(to.into()));
    p
}

#[test]
fn csv_to_json_roundtrip() {
    let csv = b"name,age\nAlice,30\nBob,25\n";
    // CSV → JSON
    let json = FormatConvertFn.execute(
        vec![Bytes::from(&csv[..])], &convert_params(Some("csv"), "json")
    ).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&json).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["name"], "Alice");
    // JSON → CSV
    let csv_back = FormatConvertFn.execute(
        vec![json], &convert_params(Some("json"), "csv")
    ).unwrap();
    let text = std::str::from_utf8(&csv_back).unwrap();
    assert!(text.contains("Alice"));
    assert!(text.contains("Bob"));
}

#[test]
fn yaml_to_toml_preserves_structure() {
    let yaml = b"database:\n  host: localhost\n  port: 5432\n";
    let toml_out = FormatConvertFn.execute(
        vec![Bytes::from(&yaml[..])], &convert_params(Some("yaml"), "toml")
    ).unwrap();
    let text = std::str::from_utf8(&toml_out).unwrap();
    assert!(text.contains("localhost"));
    assert!(text.contains("5432"));
}

#[test]
fn json_to_yaml_conversion() {
    let json = br#"{"server":{"host":"0.0.0.0","port":8080}}"#;
    let yaml = FormatConvertFn.execute(
        vec![Bytes::from(&json[..])], &convert_params(Some("json"), "yaml")
    ).unwrap();
    let text = std::str::from_utf8(&yaml).unwrap();
    assert!(text.contains("0.0.0.0"));
    assert!(text.contains("8080"));
}

#[test]
fn auto_detect_from_format() {
    // Don't specify "from" — let it auto-detect CSV
    let csv = b"x,y\n1,2\n3,4\n";
    let result = FormatConvertFn.execute(
        vec![Bytes::from(&csv[..])], &convert_params(None, "json")
    ).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&result).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["x"], "1");
}

#[test]
fn unsupported_conversion_returns_error() {
    let png = vec![0x89, 0x50, 0x4E, 0x47];
    let result = FormatConvertFn.execute(
        vec![Bytes::from(png)], &convert_params(Some("png"), "csv")
    );
    assert!(result.is_err());
    match result.unwrap_err() {
        ComputeError::InvalidParam(msg) => assert!(msg.contains("no conversion path")),
        other => panic!("expected InvalidParam, got {:?}", other),
    }
}
