use bytes::Bytes;
use deriva_compute::builtins_format_serialization::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

fn cbor_encode(val: &ciborium::Value) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::into_writer(val, &mut buf).unwrap();
    buf
}

// ---- cbor_decode (5 tests) ----
#[test]
fn cbor_decode_object() {
    let cbor = cbor_encode(&ciborium::Value::Map(vec![
        (ciborium::Value::Text("name".into()), ciborium::Value::Text("Alice".into())),
        (ciborium::Value::Text("age".into()), ciborium::Value::Integer(30.into())),
    ]));
    let out = CborDecodeFn.execute(vec![Bytes::from(cbor)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["name"], "Alice");
    assert_eq!(v["age"], 30);
}

#[test]
fn cbor_decode_bytes_to_base64() {
    let cbor = cbor_encode(&ciborium::Value::Bytes(vec![0xDE, 0xAD]));
    let out = CborDecodeFn.execute(vec![Bytes::from(cbor)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["$bytes"].is_string());
}

#[test]
fn cbor_decode_tag_annotated() {
    let cbor = cbor_encode(&ciborium::Value::Tag(1, Box::new(ciborium::Value::Integer(1234567890.into()))));
    let out = CborDecodeFn.execute(vec![Bytes::from(cbor)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["$tag"], 1);
    assert_eq!(v["value"], 1234567890);
}

#[test]
fn cbor_decode_array() {
    let cbor = cbor_encode(&ciborium::Value::Array(vec![
        ciborium::Value::Integer(1.into()),
        ciborium::Value::Integer(2.into()),
    ]));
    let out = CborDecodeFn.execute(vec![Bytes::from(cbor)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v, serde_json::json!([1, 2]));
}

#[test]
fn cbor_decode_invalid() {
    assert!(CborDecodeFn.execute(vec![Bytes::from_static(b"\xff\xff\xff")], &p(&[])).is_err());
}

// ---- cbor_encode (5 tests) ----
#[test]
fn cbor_encode_roundtrip() {
    let json = r#"{"key":"val","n":42}"#;
    let cbor = CborEncodeFn.execute(vec![Bytes::from(json)], &p(&[])).unwrap();
    let back = CborDecodeFn.execute(vec![cbor], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v["key"], "val");
    assert_eq!(v["n"], 42);
}

#[test]
fn cbor_encode_array() {
    let json = r#"[1,true,null,"hi"]"#;
    let cbor = CborEncodeFn.execute(vec![Bytes::from(json)], &p(&[])).unwrap();
    let back = CborDecodeFn.execute(vec![cbor], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v[0], 1);
    assert_eq!(v[1], true);
    assert!(v[2].is_null());
    assert_eq!(v[3], "hi");
}

#[test]
fn cbor_encode_nested() {
    let json = r#"{"a":{"b":[1,2]}}"#;
    let cbor = CborEncodeFn.execute(vec![Bytes::from(json)], &p(&[])).unwrap();
    let back = CborDecodeFn.execute(vec![cbor], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v["a"]["b"][1], 2);
}

#[test]
fn cbor_encode_bad_json() {
    assert!(CborEncodeFn.execute(vec![Bytes::from_static(b"not json")], &p(&[])).is_err());
}

#[test]
fn cbor_encode_no_input() {
    assert!(CborEncodeFn.execute(vec![], &p(&[])).is_err());
}
