use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_config::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- ConfigFormatConvertFn (#355) ----
#[test]
fn convert_yaml_to_json() {
    let yaml = b"name: Alice\nage: 30\n";
    let r = ConfigFormatConvertFn.execute(vec![Bytes::from(&yaml[..])], &p(&[("from","yaml"),("to","json")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["name"], "Alice");
}
#[test]
fn convert_json_to_toml() {
    let json = br#"{"name":"Alice","age":30}"#;
    let r = ConfigFormatConvertFn.execute(vec![Bytes::from(&json[..])], &p(&[("from","json"),("to","toml")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("name = \"Alice\""));
}
#[test]
fn convert_toml_to_yaml() {
    let toml_data = b"name = \"Alice\"\n";
    let r = ConfigFormatConvertFn.execute(vec![Bytes::from(&toml_data[..])], &p(&[("from","toml"),("to","yaml")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("Alice"));
}
#[test]
fn convert_env_to_json() {
    let env = b"KEY=value\n";
    let r = ConfigFormatConvertFn.execute(vec![Bytes::from(&env[..])], &p(&[("from","env"),("to","json")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["KEY"], "value");
}
#[test]
fn convert_unsupported_format_error() {
    let r = ConfigFormatConvertFn.execute(vec![Bytes::from("data")], &p(&[("from","xml"),("to","json")]));
    assert!(r.is_err());
}
