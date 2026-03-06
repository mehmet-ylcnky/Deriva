use bytes::Bytes;
use deriva_compute::builtins_format_serialization::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- msgpack_decode (5 tests) ----
#[test]
fn msgpack_decode_object() {
    let packed = rmp_serde::to_vec(&serde_json::json!({"key": "value", "n": 42})).unwrap();
    let out = MsgpackDecodeFn.execute(vec![Bytes::from(packed)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["key"], "value");
    assert_eq!(v["n"], 42);
}

#[test]
fn msgpack_decode_array() {
    let packed = rmp_serde::to_vec(&serde_json::json!([1, 2, 3])).unwrap();
    let out = MsgpackDecodeFn.execute(vec![Bytes::from(packed)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v, serde_json::json!([1, 2, 3]));
}

#[test]
fn msgpack_decode_nested() {
    let packed = rmp_serde::to_vec(&serde_json::json!({"a": {"b": [1]}})).unwrap();
    let out = MsgpackDecodeFn.execute(vec![Bytes::from(packed)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["a"]["b"][0], 1);
}

#[test]
fn msgpack_decode_invalid() {
    // Truncated msgpack: starts a fixmap but has no data
    assert!(MsgpackDecodeFn.execute(vec![Bytes::from_static(b"\xc1")], &p(&[])).is_err());
}

#[test]
fn msgpack_decode_no_input() {
    assert!(MsgpackDecodeFn.execute(vec![], &p(&[])).is_err());
}

// ---- msgpack_encode (5 tests) ----
#[test]
fn msgpack_encode_roundtrip() {
    let json = r#"{"x":1,"y":"hello"}"#;
    let packed = MsgpackEncodeFn.execute(vec![Bytes::from(json)], &p(&[])).unwrap();
    let back = MsgpackDecodeFn.execute(vec![packed], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v["x"], 1);
    assert_eq!(v["y"], "hello");
}

#[test]
fn msgpack_encode_array() {
    let json = r#"[1,2,3]"#;
    let packed = MsgpackEncodeFn.execute(vec![Bytes::from(json)], &p(&[])).unwrap();
    let back = MsgpackDecodeFn.execute(vec![packed], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v, serde_json::json!([1, 2, 3]));
}

#[test]
fn msgpack_encode_null() {
    let packed = MsgpackEncodeFn.execute(vec![Bytes::from_static(b"null")], &p(&[])).unwrap();
    let back = MsgpackDecodeFn.execute(vec![packed], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert!(v.is_null());
}

#[test]
fn msgpack_encode_bad_json() {
    assert!(MsgpackEncodeFn.execute(vec![Bytes::from_static(b"not json")], &p(&[])).is_err());
}

#[test]
fn msgpack_encode_no_input() {
    assert!(MsgpackEncodeFn.execute(vec![], &p(&[])).is_err());
}
