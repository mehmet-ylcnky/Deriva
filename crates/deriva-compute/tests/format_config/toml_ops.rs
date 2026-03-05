use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_config::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

// ---- TomlParseFn (#344) ----
#[test]
fn toml_parse_simple() {
    let toml_data = b"name = \"Alice\"\nage = 30\n";
    let r = TomlParseFn.execute(vec![Bytes::from(&toml_data[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["name"], "Alice");
    assert_eq!(v["age"], 30);
}
#[test]
fn toml_parse_table() {
    let toml_data = b"[database]\nhost = \"localhost\"\nport = 5432\n";
    let r = TomlParseFn.execute(vec![Bytes::from(&toml_data[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["database"]["host"], "localhost");
    assert_eq!(v["database"]["port"], 5432);
}
#[test]
fn toml_parse_array_of_tables() {
    let toml_data = b"[[servers]]\nname = \"alpha\"\n[[servers]]\nname = \"beta\"\n";
    let r = TomlParseFn.execute(vec![Bytes::from(&toml_data[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["servers"].as_array().unwrap().len(), 2);
}
#[test]
fn toml_parse_inline_table() {
    let toml_data = b"point = { x = 1, y = 2 }\n";
    let r = TomlParseFn.execute(vec![Bytes::from(&toml_data[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["point"]["x"], 1);
}
#[test]
fn toml_parse_invalid_error() {
    let r = TomlParseFn.execute(vec![Bytes::from("[bad\nno closing")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- TomlWriteFn (#345) ----
#[test]
fn toml_write_simple() {
    let json = br#"{"name":"Alice","age":30}"#;
    let r = TomlWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("name = \"Alice\""));
    assert!(text.contains("age = 30"));
}
#[test]
fn toml_write_nested() {
    let json = br#"{"database":{"host":"localhost","port":5432}}"#;
    let r = TomlWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("[database]"));
    assert!(text.contains("host = \"localhost\""));
}
#[test]
fn toml_write_roundtrip() {
    let toml_data = b"[server]\nhost = \"localhost\"\nport = 8080\n";
    let json = TomlParseFn.execute(vec![Bytes::from(&toml_data[..])], &BTreeMap::new()).unwrap();
    let back = TomlWriteFn.execute(vec![json], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&back).unwrap();
    assert!(text.contains("host = \"localhost\""));
    assert!(text.contains("port = 8080"));
}
#[test]
fn toml_write_boolean() {
    let json = br#"{"debug":true,"verbose":false}"#;
    let r = TomlWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("debug = true"));
}
#[test]
fn toml_write_invalid_json_error() {
    let r = TomlWriteFn.execute(vec![Bytes::from("not json")], &BTreeMap::new());
    assert!(r.is_err());
}
