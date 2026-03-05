use bytes::Bytes;
use deriva_compute::function::{ComputeFunction, ComputeError};
use deriva_compute::builtins_format_csv::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- JsonPathExtractFn (#234) ----
#[test]
fn jsonpath_extract_nested() {
    let json = br#"{"store":{"book":[{"title":"A"},{"title":"B"}]}}"#;
    let r = JsonPathExtractFn.execute(vec![Bytes::from(&json[..])], &p(&[("path","$.store.book[*].title")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
}
#[test]
fn jsonpath_extract_single_value() {
    let json = br#"{"a":{"b":{"c":42}}}"#;
    let r = JsonPathExtractFn.execute(vec![Bytes::from(&json[..])], &p(&[("path","$.a.b.c")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v.to_string().contains("42"));
}
#[test]
fn jsonpath_extract_array_index() {
    let json = br#"{"items":["a","b","c"]}"#;
    let r = JsonPathExtractFn.execute(vec![Bytes::from(&json[..])], &p(&[("path","$.items[1]")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v.to_string().contains("b"));
}
#[test]
fn jsonpath_extract_no_match_returns_empty() {
    let json = br#"{"a":1}"#;
    let r = JsonPathExtractFn.execute(vec![Bytes::from(&json[..])], &p(&[("path","$.nonexistent")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    // Should be empty array or null
    assert!(v.is_array() || v.is_null());
}
#[test]
fn jsonpath_extract_wildcard() {
    let json = br#"{"users":[{"name":"alice"},{"name":"bob"}]}"#;
    let r = JsonPathExtractFn.execute(vec![Bytes::from(&json[..])], &p(&[("path","$.users[*].name")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("alice"));
    assert!(text.contains("bob"));
}

// ---- JsonMergeFn (#235) ----
#[test]
fn json_merge_shallow() {
    let a = br#"{"x":1,"y":2}"#;
    let b = br#"{"y":3,"z":4}"#;
    let r = JsonMergeFn.execute(vec![Bytes::from(&a[..]), Bytes::from(&b[..])], &p(&[("strategy","shallow")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["x"], 1);
    assert_eq!(v["y"], 3); // last wins
    assert_eq!(v["z"], 4);
}
#[test]
fn json_merge_deep_nested() {
    let a = br#"{"config":{"db":{"host":"localhost"},"cache":true}}"#;
    let b = br#"{"config":{"db":{"port":5432},"debug":false}}"#;
    let r = JsonMergeFn.execute(vec![Bytes::from(&a[..]), Bytes::from(&b[..])], &p(&[("strategy","deep")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["config"]["db"]["host"], "localhost"); // preserved
    assert_eq!(v["config"]["db"]["port"], 5432); // added
    assert_eq!(v["config"]["cache"], true); // preserved
}
#[test]
fn json_merge_deep_scalar_override() {
    let a = br#"{"a":{"b":1}}"#;
    let b = br#"{"a":{"b":2}}"#;
    let r = JsonMergeFn.execute(vec![Bytes::from(&a[..]), Bytes::from(&b[..])], &p(&[("strategy","deep")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["a"]["b"], 2);
}
#[test]
fn json_merge_default_is_shallow() {
    let a = br#"{"x":1}"#;
    let b = br#"{"y":2}"#;
    let r = JsonMergeFn.execute(vec![Bytes::from(&a[..]), Bytes::from(&b[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["x"], 1);
    assert_eq!(v["y"], 2);
}
#[test]
fn json_merge_invalid_strategy_error() {
    let a = br#"{"x":1}"#;
    let b = br#"{"y":2}"#;
    let r = JsonMergeFn.execute(vec![Bytes::from(&a[..]), Bytes::from(&b[..])], &p(&[("strategy","invalid")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ---- JsonFlattenFn (#236) ----
#[test]
fn json_flatten_nested() {
    let json = br#"{"a":{"b":{"c":1},"d":2}}"#;
    let r = JsonFlattenFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["a.b.c"], 1);
    assert_eq!(v["a.d"], 2);
}
#[test]
fn json_flatten_custom_separator() {
    let json = br#"{"a":{"b":1}}"#;
    let r = JsonFlattenFn.execute(vec![Bytes::from(&json[..])], &p(&[("separator","/")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["a/b"], 1);
}
#[test]
fn json_flatten_already_flat() {
    let json = br#"{"x":1,"y":"hello"}"#;
    let r = JsonFlattenFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["x"], 1);
    assert_eq!(v["y"], "hello");
}
#[test]
fn json_flatten_with_arrays() {
    let json = br#"{"items":[10,20,30]}"#;
    let r = JsonFlattenFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["items.0"], 10);
    assert_eq!(v["items.2"], 30);
}
#[test]
fn json_flatten_unflatten_roundtrip() {
    let json = br#"{"a":{"b":1},"c":{"d":{"e":2}}}"#;
    let flat = JsonFlattenFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let unflat = JsonUnflattenFn.execute(vec![flat], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&unflat).unwrap();
    assert_eq!(v["a"]["b"], 1);
    assert_eq!(v["c"]["d"]["e"], 2);
}

// ---- JsonUnflattenFn (#237) ----
#[test]
fn json_unflatten_basic() {
    let json = br#"{"a.b":1,"a.c":2,"d":3}"#;
    let r = JsonUnflattenFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["a"]["b"], 1);
    assert_eq!(v["a"]["c"], 2);
    assert_eq!(v["d"], 3);
}
#[test]
fn json_unflatten_deep() {
    let json = br#"{"a.b.c.d":42}"#;
    let r = JsonUnflattenFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["a"]["b"]["c"]["d"], 42);
}
#[test]
fn json_unflatten_custom_separator() {
    let json = br#"{"a/b":1}"#;
    let r = JsonUnflattenFn.execute(vec![Bytes::from(&json[..])], &p(&[("separator","/")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["a"]["b"], 1);
}
#[test]
fn json_unflatten_already_nested_keys() {
    let json = br#"{"x":1,"y":2}"#;
    let r = JsonUnflattenFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["x"], 1);
}
#[test]
fn json_unflatten_multiple_branches() {
    let json = br#"{"a.x":1,"a.y":2,"b.x":3,"b.y":4}"#;
    let r = JsonUnflattenFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["a"]["x"], 1);
    assert_eq!(v["b"]["y"], 4);
}
