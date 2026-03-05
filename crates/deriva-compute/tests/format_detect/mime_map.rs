use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_detect::MimeTypeMapFn;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn ext_to_mime(ext: &str) -> serde_json::Value {
    let result = MimeTypeMapFn.execute(vec![Bytes::from(ext.to_string())], &BTreeMap::new()).unwrap();
    serde_json::from_slice(&result).unwrap()
}

fn mime_to_ext(mime: &str) -> serde_json::Value {
    let mut params = BTreeMap::new();
    params.insert("direction".into(), Value::String("mime_to_ext".into()));
    let result = MimeTypeMapFn.execute(vec![Bytes::from(mime.to_string())], &params).unwrap();
    serde_json::from_slice(&result).unwrap()
}

#[test]
fn known_extension_resolves_to_mime() {
    let r = ext_to_mime("json");
    assert_eq!(r["mime"], "application/json");
}

#[test]
fn dotted_extension_handled() {
    let r = ext_to_mime(".png");
    assert_eq!(r["mime"], "image/png");
}

#[test]
fn unknown_extension_returns_octet_stream() {
    let r = ext_to_mime("xyz123");
    assert_eq!(r["mime"], "application/octet-stream");
}

#[test]
fn mime_to_extension_lookup() {
    let r = mime_to_ext("text/csv");
    let exts = r["extensions"].as_array().unwrap();
    assert!(exts.contains(&serde_json::json!("csv")));
}

#[test]
fn unknown_mime_returns_empty_extensions() {
    let r = mime_to_ext("application/x-totally-unknown");
    let exts = r["extensions"].as_array().unwrap();
    assert!(exts.is_empty());
}
