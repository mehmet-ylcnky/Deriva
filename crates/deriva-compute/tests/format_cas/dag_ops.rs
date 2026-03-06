use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_cas::*;
use std::collections::BTreeMap;

// ---- DagPbEncodeFn (#399) ----
#[test]
fn dag_pb_encode_data_only() {
    let json = r#"{"links":[],"data":"aGVsbG8="}"#;
    let r = DagPbEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    assert!(!r.is_empty());
}
#[test]
fn dag_pb_encode_decode_roundtrip() {
    let json = r#"{"links":[],"data":"dGVzdA=="}"#;
    let encoded = DagPbEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagPbDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(v["data"], "dGVzdA==");
}
#[test]
fn dag_pb_encode_no_data() {
    let json = r#"{"links":[]}"#;
    // Empty PbNode with no links and no data encodes to 0 bytes in protobuf — that's valid
    let r = DagPbEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    // Decode should still work
    let decoded = DagPbDecodeFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert!(v["data"].is_null());
}
#[test]
fn dag_pb_encode_invalid_json() {
    let r = DagPbEncodeFn.execute(vec![Bytes::from("not json")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn dag_pb_encode_empty_links() {
    let json = r#"{"links":[],"data":"AA=="}"#;
    let encoded = DagPbEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagPbDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert!(v["links"].as_array().unwrap().is_empty());
}

// ---- DagPbDecodeFn (#400) ----
#[test]
fn dag_pb_decode_invalid() {
    let r = DagPbDecodeFn.execute(vec![Bytes::from(vec![0xFF, 0xFF])], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn dag_pb_decode_has_links_field() {
    let json = r#"{"links":[],"data":"eA=="}"#;
    let encoded = DagPbEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagPbDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert!(v["links"].is_array());
}
#[test]
fn dag_pb_decode_preserves_data() {
    let json = r#"{"links":[],"data":"AQID"}"#;
    let encoded = DagPbEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagPbDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(v["data"], "AQID");
}
#[test]
fn dag_pb_decode_empty_input() {
    // Empty protobuf is valid (all fields optional)
    let encoded = DagPbEncodeFn.execute(vec![Bytes::from(r#"{"links":[]}"#)], &BTreeMap::new()).unwrap();
    let decoded = DagPbDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert!(v["data"].is_null());
}
#[test]
fn dag_pb_decode_no_data_field() {
    let json = r#"{"links":[]}"#;
    let encoded = DagPbEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagPbDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert!(v["data"].is_null());
}

// ---- DagCborEncodeFn (#401) ----
#[test]
fn dag_cbor_encode_basic() {
    let json = r#"{"key":"value","num":42}"#;
    let r = DagCborEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    assert!(!r.is_empty());
}
#[test]
fn dag_cbor_encode_decode_roundtrip() {
    let json = r#"{"a":1,"b":"hello","c":[1,2,3]}"#;
    let encoded = DagCborEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagCborDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(v["a"], 1);
    assert_eq!(v["b"], "hello");
}
#[test]
fn dag_cbor_encode_nested() {
    let json = r#"{"outer":{"inner":"value"}}"#;
    let encoded = DagCborEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagCborDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(v["outer"]["inner"], "value");
}
#[test]
fn dag_cbor_encode_null() {
    let json = r#"{"x":null}"#;
    let encoded = DagCborEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagCborDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert!(v["x"].is_null());
}
#[test]
fn dag_cbor_encode_invalid_json() {
    let r = DagCborEncodeFn.execute(vec![Bytes::from("not json")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- DagCborDecodeFn (#402) ----
#[test]
fn dag_cbor_decode_invalid() {
    let r = DagCborDecodeFn.execute(vec![Bytes::from(vec![0xFF])], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn dag_cbor_decode_bool() {
    let json = r#"{"flag":true}"#;
    let encoded = DagCborEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagCborDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(v["flag"], true);
}
#[test]
fn dag_cbor_decode_array() {
    let json = r#"[1,2,3]"#;
    let encoded = DagCborEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagCborDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 3);
}
#[test]
fn dag_cbor_decode_string() {
    let json = r#""hello""#;
    let encoded = DagCborEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let decoded = DagCborDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(v, "hello");
}
#[test]
fn dag_cbor_decode_deterministic() {
    let json = r#"{"z":1,"a":2}"#;
    let e1 = DagCborEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let e2 = DagCborEncodeFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    assert_eq!(e1, e2);
}
