use bytes::Bytes;
use deriva_compute::builtins::*;
use deriva_compute::function::{ComputeError, ComputeFunction};
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn exec1(f: &dyn ComputeFunction, input: &[u8]) -> Result<Bytes, ComputeError> {
    f.execute(vec![Bytes::from(input.to_vec())], &BTreeMap::new())
}

fn exec1_params(f: &dyn ComputeFunction, input: &[u8], params: BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    f.execute(vec![Bytes::from(input.to_vec())], &params)
}

fn params(kv: &[(&str, &str)]) -> BTreeMap<String, Value> {
    kv.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

const TEST_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TEST_NONCE: &str = "00112233445566778899aabbccddeeff";
const TEST_GCM_NONCE: &str = "000102030405060708090a0b";

fn read_u64(b: &Bytes) -> u64 { u64::from_be_bytes(b[..8].try_into().unwrap()) }
fn read_f64(b: &Bytes) -> f64 { f64::from_be_bytes(b[..8].try_into().unwrap()) }




// ── #84 JsonPrettyPrintFn ──

#[test]
fn json_pretty_basic() {
    let r = exec1(&JsonPrettyPrintFn, b"{\"a\":1,\"b\":2}").unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains('\n'));
    assert!(text.contains("  "));
}

#[test]
fn json_pretty_null() {
    let r = exec1(&JsonPrettyPrintFn, b"null").unwrap();
    assert_eq!(r.as_ref(), b"null");
}

#[test]
fn json_pretty_idempotent() {
    let r1 = exec1(&JsonPrettyPrintFn, b"{\"x\":1}").unwrap();
    let r2 = exec1(&JsonPrettyPrintFn, &r1).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn json_pretty_invalid() {
    let r = exec1(&JsonPrettyPrintFn, b"{bad}");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn json_pretty_array() {
    let r = exec1(&JsonPrettyPrintFn, b"[1,2,3]").unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains('\n'));
}


// ── #85 JsonMinifyFn ──

#[test]
fn json_minify_basic() {
    let r = exec1(&JsonMinifyFn, b"{\n  \"a\": 1,\n  \"b\": 2\n}").unwrap();
    assert_eq!(r.as_ref(), b"{\"a\":1,\"b\":2}");
}

#[test]
fn json_minify_already_compact() {
    let r = exec1(&JsonMinifyFn, b"{\"x\":1}").unwrap();
    assert_eq!(r.as_ref(), b"{\"x\":1}");
}

#[test]
fn json_minify_roundtrip_with_pretty() {
    let pretty = exec1(&JsonPrettyPrintFn, b"{\"a\":1}").unwrap();
    let mini = exec1(&JsonMinifyFn, &pretty).unwrap();
    assert_eq!(mini.as_ref(), b"{\"a\":1}");
}

#[test]
fn json_minify_invalid() {
    let r = exec1(&JsonMinifyFn, b"not json");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn json_minify_empty() {
    let r = exec1(&JsonMinifyFn, b"");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}


// ── #86 CsvToJsonFn ──

#[test]
fn csv_to_json_basic() {
    let r = exec1(&CsvToJsonFn, b"name,age\nAlice,30\nBob,25").unwrap();
    let v: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(v.len(), 2);
    assert_eq!(v[0]["name"], "Alice");
    assert_eq!(v[0]["age"], "30");
}

#[test]
fn csv_to_json_header_only() {
    let r = exec1(&CsvToJsonFn, b"name,age\n").unwrap();
    let v: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert!(v.is_empty());
}

#[test]
fn csv_to_json_quoted_fields() {
    let r = exec1(&CsvToJsonFn, b"msg\n\"hello, world\"").unwrap();
    let v: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(v[0]["msg"], "hello, world");
}

#[test]
fn csv_to_json_single_column() {
    let r = exec1(&CsvToJsonFn, b"val\n1\n2\n3").unwrap();
    let v: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(v.len(), 3);
}

#[test]
fn csv_to_json_empty() {
    let r = exec1(&CsvToJsonFn, b"").unwrap();
    let v: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert!(v.is_empty());
}


// ── #87 JsonToCsvFn ──

#[test]
fn json_to_csv_basic() {
    let input = r#"[{"age":"30","name":"Alice"},{"age":"25","name":"Bob"}]"#;
    let r = exec1(&JsonToCsvFn, input.as_bytes()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.starts_with("age,name\n"));
    assert!(text.contains("30,Alice"));
}

#[test]
fn json_to_csv_empty_array() {
    let r = exec1(&JsonToCsvFn, b"[]").unwrap();
    assert!(r.is_empty());
}

#[test]
fn json_to_csv_numeric_values() {
    let input = r#"[{"count":42,"label":"test"}]"#;
    let r = exec1(&JsonToCsvFn, input.as_bytes()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("42"));
}

#[test]
fn json_to_csv_missing_field() {
    let input = r#"[{"a":"1","b":"2"},{"a":"3"}]"#;
    let r = exec1(&JsonToCsvFn, input.as_bytes()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3); // header + 2 rows
}

#[test]
fn json_to_csv_invalid() {
    let r = exec1(&JsonToCsvFn, b"not json");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}


// ── #88 JsonLinesFn ──

#[test]
fn json_lines_basic() {
    let r = exec1(&JsonLinesFn, b"[1,2,3]").unwrap();
    assert_eq!(r.as_ref(), b"1\n2\n3");
}

#[test]
fn json_lines_objects() {
    let r = exec1(&JsonLinesFn, b"[{\"a\":1},{\"b\":2}]").unwrap();
    let lines: Vec<&str> = std::str::from_utf8(&r).unwrap().lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"a\":1") || lines[0].contains("\"a\": 1"));
}

#[test]
fn json_lines_empty_array() {
    let r = exec1(&JsonLinesFn, b"[]").unwrap();
    assert!(r.is_empty());
}

#[test]
fn json_lines_not_array() {
    let r = exec1(&JsonLinesFn, b"{\"a\":1}");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn json_lines_strings() {
    let r = exec1(&JsonLinesFn, b"[\"hello\",\"world\"]").unwrap();
    assert_eq!(r.as_ref(), b"\"hello\"\n\"world\"");
}


// ── #89 YamlToJsonFn ──

#[test]
fn yaml_to_json_basic() {
    let r = exec1(&YamlToJsonFn, b"name: Alice\nage: 30").unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["name"], "Alice");
    assert_eq!(v["age"], 30);
}

#[test]
fn yaml_to_json_list() {
    let r = exec1(&YamlToJsonFn, b"- a\n- b\n- c").unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v.is_array());
    assert_eq!(v.as_array().unwrap().len(), 3);
}

#[test]
fn yaml_to_json_nested() {
    let r = exec1(&YamlToJsonFn, b"outer:\n  inner: value").unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["outer"]["inner"], "value");
}

#[test]
fn yaml_to_json_invalid() {
    let r = exec1(&YamlToJsonFn, b":\n  bad:\n    - ][");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn yaml_to_json_scalar() {
    let r = exec1(&YamlToJsonFn, b"42").unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v, 42);
}


// ── #90 JsonToYamlFn ──

#[test]
fn json_to_yaml_basic() {
    let r = exec1(&JsonToYamlFn, b"{\"name\":\"Alice\",\"age\":30}").unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("name:"));
    assert!(text.contains("age:"));
}

#[test]
fn json_to_yaml_roundtrip() {
    let json_in = b"{\"x\":1,\"y\":\"hello\"}";
    let yaml = exec1(&JsonToYamlFn, json_in).unwrap();
    let json_out = exec1(&YamlToJsonFn, &yaml).unwrap();
    let v1: serde_json::Value = serde_json::from_slice(json_in).unwrap();
    let v2: serde_json::Value = serde_json::from_slice(&json_out).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn json_to_yaml_array() {
    let r = exec1(&JsonToYamlFn, b"[1,2,3]").unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("- 1"));
}

#[test]
fn json_to_yaml_invalid() {
    let r = exec1(&JsonToYamlFn, b"not json");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn json_to_yaml_null() {
    let r = exec1(&JsonToYamlFn, b"null").unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.trim() == "null" || text.trim() == "~");
}


// ── #91 TomlToJsonFn ──

#[test]
fn toml_to_json_basic() {
    let r = exec1(&TomlToJsonFn, b"[package]\nname = \"test\"\nversion = \"1.0\"").unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["package"]["name"], "test");
    assert_eq!(v["package"]["version"], "1.0");
}

#[test]
fn toml_to_json_flat() {
    let r = exec1(&TomlToJsonFn, b"key = \"value\"\nnum = 42").unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["key"], "value");
    assert_eq!(v["num"], 42);
}

#[test]
fn toml_to_json_array() {
    let r = exec1(&TomlToJsonFn, b"items = [1, 2, 3]").unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 3);
}

#[test]
fn toml_to_json_invalid() {
    let r = exec1(&TomlToJsonFn, b"[[[bad toml");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn toml_to_json_nested_tables() {
    let r = exec1(&TomlToJsonFn, b"[a]\nx = 1\n[a.b]\ny = 2").unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["a"]["x"], 1);
    assert_eq!(v["a"]["b"]["y"], 2);
}


