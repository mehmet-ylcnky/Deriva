use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_cas::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- CidComputeFn (#396) ----
#[test]
fn cid_compute_default_raw_v1() {
    let r = CidComputeFn.execute(vec![Bytes::from("hello world")], &BTreeMap::new()).unwrap();
    let cid_str = std::str::from_utf8(&r).unwrap();
    assert!(cid_str.starts_with("baf"), "CIDv1 should start with baf, got: {cid_str}");
}
#[test]
fn cid_compute_empty_input() {
    let r = CidComputeFn.execute(vec![Bytes::from("")], &BTreeMap::new()).unwrap();
    let cid_str = std::str::from_utf8(&r).unwrap();
    assert!(!cid_str.is_empty());
}
#[test]
fn cid_compute_dag_cbor_codec() {
    let r = CidComputeFn.execute(vec![Bytes::from("data")], &p(&[("codec","dag-cbor")])).unwrap();
    let cid_str = std::str::from_utf8(&r).unwrap();
    assert!(cid_str.starts_with("bafy"));
}
#[test]
fn cid_compute_blake3_hash() {
    let r = CidComputeFn.execute(vec![Bytes::from("data")], &p(&[("hash","blake3")])).unwrap();
    assert!(!r.is_empty());
}
#[test]
fn cid_compute_deterministic() {
    let r1 = CidComputeFn.execute(vec![Bytes::from("same")], &BTreeMap::new()).unwrap();
    let r2 = CidComputeFn.execute(vec![Bytes::from("same")], &BTreeMap::new()).unwrap();
    assert_eq!(r1, r2);
}

// ---- CidVerifyFn (#397) ----
#[test]
fn cid_verify_valid() {
    let cid = CidComputeFn.execute(vec![Bytes::from("test")], &BTreeMap::new()).unwrap();
    let cid_str = std::str::from_utf8(&cid).unwrap();
    let r = CidVerifyFn.execute(vec![Bytes::from("test")], &p(&[("expected_cid", cid_str)])).unwrap();
    assert_eq!(r, Bytes::from("test"));
}
#[test]
fn cid_verify_mismatch() {
    let cid = CidComputeFn.execute(vec![Bytes::from("test")], &BTreeMap::new()).unwrap();
    let cid_str = std::str::from_utf8(&cid).unwrap();
    let r = CidVerifyFn.execute(vec![Bytes::from("different")], &p(&[("expected_cid", cid_str)]));
    assert!(r.is_err());
}
#[test]
fn cid_verify_missing_param() {
    let r = CidVerifyFn.execute(vec![Bytes::from("test")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn cid_verify_invalid_cid() {
    let r = CidVerifyFn.execute(vec![Bytes::from("test")], &p(&[("expected_cid","not-a-cid")]));
    assert!(r.is_err());
}
#[test]
fn cid_verify_passthrough() {
    let data = b"binary\x00data";
    let cid = CidComputeFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let cid_str = std::str::from_utf8(&cid).unwrap();
    let r = CidVerifyFn.execute(vec![Bytes::from(&data[..])], &p(&[("expected_cid", cid_str)])).unwrap();
    assert_eq!(&r[..], data);
}

// ---- CidParseFn (#398) ----
#[test]
fn cid_parse_v1_raw() {
    let cid = CidComputeFn.execute(vec![Bytes::from("hello")], &BTreeMap::new()).unwrap();
    let r = CidParseFn.execute(vec![cid], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["version"], 1);
    assert_eq!(v["codec"], "raw");
    assert_eq!(v["hash"], "sha2-256");
}
#[test]
fn cid_parse_dag_cbor() {
    let cid = CidComputeFn.execute(vec![Bytes::from("x")], &p(&[("codec","dag-cbor")])).unwrap();
    let r = CidParseFn.execute(vec![cid], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["codec"], "dag-cbor");
}
#[test]
fn cid_parse_has_digest() {
    let cid = CidComputeFn.execute(vec![Bytes::from("data")], &BTreeMap::new()).unwrap();
    let r = CidParseFn.execute(vec![cid], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["digest"].as_str().unwrap().len() > 0);
}
#[test]
fn cid_parse_invalid() {
    let r = CidParseFn.execute(vec![Bytes::from("not-a-cid")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn cid_parse_blake3() {
    let cid = CidComputeFn.execute(vec![Bytes::from("x")], &p(&[("hash","blake3")])).unwrap();
    let r = CidParseFn.execute(vec![cid], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["hash"], "blake3");
}
