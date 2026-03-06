use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_erasure::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- ReedSolomonEncodeFn (#466) ----
#[test]
fn rs_encode_default_4_2() {
    let data = Bytes::from("hello world erasure coding test!");
    let encoded = ReedSolomonEncodeFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    // 4+2=6 shards, each shard_size = ceil((4+31)/4) = 9, total = 54
    assert!(encoded.len() > data.len());
}
#[test]
fn rs_encode_decode_roundtrip() {
    let data = Bytes::from("roundtrip test data for RS coding");
    let encoded = ReedSolomonEncodeFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let decoded = ReedSolomonDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    assert_eq!(decoded, data);
}
#[test]
fn rs_encode_6_3() {
    let data = Bytes::from("six plus three config");
    let encoded = ReedSolomonEncodeFn.execute(vec![data.clone()], &p(&[("data_shards","6"),("parity_shards","3")])).unwrap();
    let decoded = ReedSolomonDecodeFn.execute(vec![encoded], &p(&[("data_shards","6"),("parity_shards","3")])).unwrap();
    assert_eq!(decoded, data);
}
#[test]
fn rs_encode_empty_error() {
    let r = ReedSolomonEncodeFn.execute(vec![Bytes::from("")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn rs_encode_zero_shards_error() {
    let r = ReedSolomonEncodeFn.execute(vec![Bytes::from("x")], &p(&[("data_shards","0")]));
    assert!(r.is_err());
}

// ---- ReedSolomonDecodeFn (#467) ----
#[test]
fn rs_decode_recover_1_missing() {
    let data = Bytes::from("recover one missing shard");
    let encoded = ReedSolomonEncodeFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let decoded = ReedSolomonDecodeFn.execute(vec![encoded], &p(&[("missing","0")])).unwrap();
    assert_eq!(decoded, data);
}
#[test]
fn rs_decode_recover_2_missing() {
    let data = Bytes::from("recover two missing shards");
    let encoded = ReedSolomonEncodeFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let decoded = ReedSolomonDecodeFn.execute(vec![encoded], &p(&[("missing","1,4")])).unwrap();
    assert_eq!(decoded, data);
}
#[test]
fn rs_decode_too_many_missing() {
    let data = Bytes::from("too many missing");
    let encoded = ReedSolomonEncodeFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let r = ReedSolomonDecodeFn.execute(vec![encoded], &p(&[("missing","0,1,2")]));
    assert!(r.is_err());
}
#[test]
fn rs_decode_out_of_range_index() {
    let data = Bytes::from("out of range");
    let encoded = ReedSolomonEncodeFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let r = ReedSolomonDecodeFn.execute(vec![encoded], &p(&[("missing","99")]));
    assert!(r.is_err());
}
#[test]
fn rs_decode_no_missing() {
    let data = Bytes::from("no missing shards");
    let encoded = ReedSolomonEncodeFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let decoded = ReedSolomonDecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    assert_eq!(decoded, data);
}

// ---- ReedSolomonVerifyFn (#468) ----
#[test]
fn rs_verify_valid() {
    let data = Bytes::from("verify this encoding");
    let encoded = ReedSolomonEncodeFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let r = ReedSolomonVerifyFn.execute(vec![encoded.clone()], &BTreeMap::new()).unwrap();
    assert_eq!(r, encoded);
}
#[test]
fn rs_verify_corrupted() {
    let data = Bytes::from("corrupt me");
    let mut encoded = ReedSolomonEncodeFn.execute(vec![data], &BTreeMap::new()).unwrap().to_vec();
    encoded[5] ^= 0xFF;
    let r = ReedSolomonVerifyFn.execute(vec![Bytes::from(encoded)], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn rs_verify_6_3() {
    let data = Bytes::from("six three verify");
    let encoded = ReedSolomonEncodeFn.execute(vec![data], &p(&[("data_shards","6"),("parity_shards","3")])).unwrap();
    let r = ReedSolomonVerifyFn.execute(vec![encoded], &p(&[("data_shards","6"),("parity_shards","3")]));
    assert!(r.is_ok());
}
#[test]
fn rs_verify_passthrough() {
    let data = Bytes::from("passthrough");
    let encoded = ReedSolomonEncodeFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let verified = ReedSolomonVerifyFn.execute(vec![encoded.clone()], &BTreeMap::new()).unwrap();
    assert_eq!(encoded, verified);
}
#[test]
fn rs_verify_10_4() {
    let data = Bytes::from("ten data four parity shards test");
    let encoded = ReedSolomonEncodeFn.execute(vec![data], &p(&[("data_shards","10"),("parity_shards","4")])).unwrap();
    let r = ReedSolomonVerifyFn.execute(vec![encoded], &p(&[("data_shards","10"),("parity_shards","4")]));
    assert!(r.is_ok());
}
