use bytes::Bytes;
use deriva_compute::builtins_format_serialization::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

fn make_bson(doc: &bson::Document) -> Vec<u8> {
    let mut buf = Vec::new();
    doc.to_writer(&mut buf).unwrap();
    buf
}

// ---- bson_decode (5 tests) ----
#[test]
fn bson_decode_simple() {
    let mut doc = bson::Document::new();
    doc.insert("name", "Alice");
    doc.insert("age", 30_i32);
    let out = BsonDecodeFn.execute(vec![Bytes::from(make_bson(&doc))], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["name"], "Alice");
    assert_eq!(v["age"], 30);
}

#[test]
fn bson_decode_nested() {
    let mut inner = bson::Document::new();
    inner.insert("x", 1_i32);
    let mut doc = bson::Document::new();
    doc.insert("nested", inner);
    let out = BsonDecodeFn.execute(vec![Bytes::from(make_bson(&doc))], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["nested"]["x"], 1);
}

#[test]
fn bson_decode_objectid() {
    let mut doc = bson::Document::new();
    doc.insert("_id", bson::oid::ObjectId::new());
    let out = BsonDecodeFn.execute(vec![Bytes::from(make_bson(&doc))], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    // Relaxed extjson: ObjectId becomes {"$oid": "..."}
    assert!(v["_id"]["$oid"].is_string());
}

#[test]
fn bson_decode_invalid() {
    assert!(BsonDecodeFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn bson_decode_no_input() {
    assert!(BsonDecodeFn.execute(vec![], &p(&[])).is_err());
}

// ---- bson_encode (5 tests) ----
#[test]
fn bson_encode_roundtrip() {
    let json = r#"{"name":"Bob","age":25}"#;
    let bson_bytes = BsonEncodeFn.execute(vec![Bytes::from(json)], &p(&[])).unwrap();
    let back = BsonDecodeFn.execute(vec![bson_bytes], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v["name"], "Bob");
    assert_eq!(v["age"], 25);
}

#[test]
fn bson_encode_nested_object() {
    let json = r#"{"a":{"b":1}}"#;
    let bson_bytes = BsonEncodeFn.execute(vec![Bytes::from(json)], &p(&[])).unwrap();
    let back = BsonDecodeFn.execute(vec![bson_bytes], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v["a"]["b"], 1);
}

#[test]
fn bson_encode_array_field() {
    let json = r#"{"tags":["a","b"]}"#;
    let bson_bytes = BsonEncodeFn.execute(vec![Bytes::from(json)], &p(&[])).unwrap();
    let back = BsonDecodeFn.execute(vec![bson_bytes], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v["tags"][0], "a");
}

#[test]
fn bson_encode_non_object_fails() {
    assert!(BsonEncodeFn.execute(vec![Bytes::from_static(b"[1,2]")], &p(&[])).is_err());
}

#[test]
fn bson_encode_bad_json() {
    assert!(BsonEncodeFn.execute(vec![Bytes::from_static(b"not json")], &p(&[])).is_err());
}
