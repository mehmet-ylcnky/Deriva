use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_config::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- IniParseFn (#346) ----
#[test]
fn ini_parse_sections() {
    let ini = b"[database]\nhost = localhost\nport = 5432\n";
    let r = IniParseFn.execute(vec![Bytes::from(&ini[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["database"]["host"], "localhost");
    assert_eq!(v["database"]["port"], "5432");
}
#[test]
fn ini_parse_multiple_sections() {
    let ini = b"[db]\nhost = localhost\n[cache]\nttl = 300\n";
    let r = IniParseFn.execute(vec![Bytes::from(&ini[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["db"].is_object());
    assert!(v["cache"].is_object());
}
#[test]
fn ini_parse_global_keys_default_section() {
    let ini = b"global_key = value\n[section]\nk = v\n";
    let r = IniParseFn.execute(vec![Bytes::from(&ini[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["default"]["global_key"], "value");
}
#[test]
fn ini_parse_duplicate_keys_last_wins() {
    let ini = b"[s]\nk = first\nk = second\n";
    let r = IniParseFn.execute(vec![Bytes::from(&ini[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["s"]["k"], "second");
}
#[test]
fn ini_parse_empty_value() {
    let ini = b"[s]\nkey =\n";
    let r = IniParseFn.execute(vec![Bytes::from(&ini[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["s"]["key"].is_string() || v["s"]["key"].is_null());
}

// ---- IniWriteFn (#347) ----
#[test]
fn ini_write_sections() {
    let json = br#"{"database":{"host":"localhost","port":"5432"}}"#;
    let r = IniWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("[database]"));
    assert!(text.contains("host = localhost"));
}
#[test]
fn ini_write_multiple_sections() {
    let json = br#"{"db":{"h":"l"},"cache":{"ttl":"300"}}"#;
    let r = IniWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("[db]"));
    assert!(text.contains("[cache]"));
}
#[test]
fn ini_write_roundtrip() {
    let ini = b"[server]\nhost = localhost\nport = 8080\n";
    let json = IniParseFn.execute(vec![Bytes::from(&ini[..])], &BTreeMap::new()).unwrap();
    let back = IniWriteFn.execute(vec![json], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&back).unwrap();
    assert!(text.contains("[server]"));
    assert!(text.contains("host = localhost"));
}
#[test]
fn ini_write_invalid_json_error() {
    let r = IniWriteFn.execute(vec![Bytes::from("not json")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn ini_write_non_object_error() {
    let r = IniWriteFn.execute(vec![Bytes::from("[1,2,3]")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- EnvParseFn (#348) ----
#[test]
fn env_parse_simple() {
    let env = b"DB_HOST=localhost\nDB_PORT=5432\n";
    let r = EnvParseFn.execute(vec![Bytes::from(&env[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["DB_HOST"], "localhost");
    assert_eq!(v["DB_PORT"], "5432");
}
#[test]
fn env_parse_quoted_values() {
    let env = b"NAME=\"Alice Bob\"\nPATH='some/path'\n";
    let r = EnvParseFn.execute(vec![Bytes::from(&env[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["NAME"], "Alice Bob");
    assert_eq!(v["PATH"], "some/path");
}
#[test]
fn env_parse_comments_skipped() {
    let env = b"# comment\nKEY=value\n";
    let r = EnvParseFn.execute(vec![Bytes::from(&env[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["KEY"], "value");
    assert!(v.as_object().unwrap().len() == 1);
}
#[test]
fn env_parse_export_prefix() {
    let env = b"export API_KEY=secret123\n";
    let r = EnvParseFn.execute(vec![Bytes::from(&env[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["API_KEY"], "secret123");
}
#[test]
fn env_parse_empty_value() {
    let env = b"EMPTY=\n";
    let r = EnvParseFn.execute(vec![Bytes::from(&env[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["EMPTY"], "");
}

// ---- EnvWriteFn (#349) ----
#[test]
fn env_write_simple() {
    let json = br#"{"DB_HOST":"localhost","DB_PORT":"5432"}"#;
    let r = EnvWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("DB_HOST=localhost"));
    assert!(text.contains("DB_PORT=5432"));
}
#[test]
fn env_write_quotes_spaces() {
    let json = br#"{"NAME":"Alice Bob"}"#;
    let r = EnvWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("NAME=\"Alice Bob\""));
}
#[test]
fn env_write_roundtrip() {
    let env = b"KEY=value\nNAME=test\n";
    let json = EnvParseFn.execute(vec![Bytes::from(&env[..])], &BTreeMap::new()).unwrap();
    let back = EnvWriteFn.execute(vec![json], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&back).unwrap();
    assert!(text.contains("KEY=value"));
    assert!(text.contains("NAME=test"));
}
#[test]
fn env_write_invalid_json_error() {
    let r = EnvWriteFn.execute(vec![Bytes::from("bad")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn env_write_non_object_error() {
    let r = EnvWriteFn.execute(vec![Bytes::from("42")], &BTreeMap::new());
    assert!(r.is_err());
}
