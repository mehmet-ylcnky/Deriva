use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_config::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- YamlParseFn (#340) ----
#[test]
fn yaml_parse_simple() {
    let yaml = b"name: Alice\nage: 30\n";
    let r = YamlParseFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["name"], "Alice");
    assert_eq!(v["age"], 30);
}
#[test]
fn yaml_parse_nested() {
    let yaml = b"db:\n  host: localhost\n  port: 5432\n";
    let r = YamlParseFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["db"]["host"], "localhost");
}
#[test]
fn yaml_parse_list() {
    let yaml = b"items:\n  - a\n  - b\n  - c\n";
    let r = YamlParseFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 3);
}
#[test]
fn yaml_parse_boolean_types() {
    let yaml = b"enabled: true\nverbose: false\n";
    let r = YamlParseFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["enabled"], true);
    assert_eq!(v["verbose"], false);
}
#[test]
fn yaml_parse_invalid_error() {
    let yaml = b":\n  - :\n  [invalid";
    let r = YamlParseFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- YamlWriteFn (#341) ----
#[test]
fn yaml_write_simple() {
    let json = br#"{"name":"Alice","age":30}"#;
    let r = YamlWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("name:"));
    assert!(text.contains("Alice"));
}
#[test]
fn yaml_write_nested() {
    let json = br#"{"db":{"host":"localhost"}}"#;
    let r = YamlWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("host:"));
}
#[test]
fn yaml_write_array() {
    let json = br#"{"items":["a","b"]}"#;
    let r = YamlWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("- a"));
}
#[test]
fn yaml_write_roundtrip() {
    let json = br#"{"x":1,"y":"hello"}"#;
    let yaml = YamlWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let back = YamlParseFn.execute(vec![yaml], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v["x"], 1);
    assert_eq!(v["y"], "hello");
}
#[test]
fn yaml_write_invalid_json_error() {
    let r = YamlWriteFn.execute(vec![Bytes::from("not json")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- YamlValidateFn (#342) ----
#[test]
fn yaml_validate_valid() {
    let yaml = b"key: value\n";
    let r = YamlValidateFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from(&yaml[..]));
}
#[test]
fn yaml_validate_complex() {
    let yaml = b"a:\n  b: [1, 2, 3]\n  c: true\n";
    let r = YamlValidateFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from(&yaml[..]));
}
#[test]
fn yaml_validate_invalid_error() {
    let yaml = b":\n  [:\n  bad";
    let r = YamlValidateFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn yaml_validate_empty_doc() {
    let yaml = b"---\n";
    let r = YamlValidateFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from(&yaml[..]));
}
#[test]
fn yaml_validate_passthrough() {
    let yaml = b"items:\n  - one\n  - two\n";
    let r = YamlValidateFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r.as_ref(), yaml);
}

// ---- YamlMergeFn (#343) ----
#[test]
fn yaml_merge_shallow_override() {
    let base = b"name: Alice\nage: 30\n";
    let overlay = b"age: 31\ncity: NYC\n";
    let r = YamlMergeFn.execute(vec![Bytes::from(&base[..]), Bytes::from(&overlay[..])], &p(&[("strategy","shallow")])).unwrap();
    let parsed = YamlParseFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&parsed).unwrap();
    assert_eq!(v["age"], 31);
    assert_eq!(v["city"], "NYC");
    assert_eq!(v["name"], "Alice");
}
#[test]
fn yaml_merge_deep_nested() {
    let base = b"db:\n  host: localhost\n  port: 5432\n";
    let overlay = b"db:\n  port: 3306\n  name: mydb\n";
    let r = YamlMergeFn.execute(vec![Bytes::from(&base[..]), Bytes::from(&overlay[..])], &p(&[("strategy","deep")])).unwrap();
    let parsed = YamlParseFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&parsed).unwrap();
    assert_eq!(v["db"]["host"], "localhost");
    assert_eq!(v["db"]["port"], 3306);
    assert_eq!(v["db"]["name"], "mydb");
}
#[test]
fn yaml_merge_deep_array_concat() {
    let base = b"tags:\n  - a\n  - b\n";
    let overlay = b"tags:\n  - c\n";
    let r = YamlMergeFn.execute(vec![Bytes::from(&base[..]), Bytes::from(&overlay[..])], &p(&[("strategy","deep")])).unwrap();
    let parsed = YamlParseFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&parsed).unwrap();
    assert_eq!(v["tags"].as_array().unwrap().len(), 3);
}
#[test]
fn yaml_merge_three_files() {
    let a = b"x: 1\n";
    let b_data = b"y: 2\n";
    let c = b"z: 3\n";
    let r = YamlMergeFn.execute(vec![Bytes::from(&a[..]), Bytes::from(&b_data[..]), Bytes::from(&c[..])], &BTreeMap::new()).unwrap();
    let parsed = YamlParseFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&parsed).unwrap();
    assert_eq!(v["x"], 1);
    assert_eq!(v["y"], 2);
    assert_eq!(v["z"], 3);
}
#[test]
fn yaml_merge_default_is_deep() {
    let base = b"a:\n  b: 1\n";
    let overlay = b"a:\n  c: 2\n";
    let r = YamlMergeFn.execute(vec![Bytes::from(&base[..]), Bytes::from(&overlay[..])], &BTreeMap::new()).unwrap();
    let parsed = YamlParseFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&parsed).unwrap();
    assert_eq!(v["a"]["b"], 1);
    assert_eq!(v["a"]["c"], 2);
}
