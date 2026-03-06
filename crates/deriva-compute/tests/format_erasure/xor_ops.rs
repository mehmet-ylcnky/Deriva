use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_erasure::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- XorParityFn (#469) ----
#[test]
fn xor_parity_roundtrip_shard0() {
    let data = Bytes::from("xor parity test data");
    let encoded = XorParityFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let decoded = XorReconstructFn.execute(vec![encoded], &p(&[("missing","0")])).unwrap();
    assert_eq!(decoded, data);
}
#[test]
fn xor_parity_roundtrip_shard1() {
    let data = Bytes::from("xor parity test data");
    let encoded = XorParityFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let decoded = XorReconstructFn.execute(vec![encoded], &p(&[("missing","1")])).unwrap();
    assert_eq!(decoded, data);
}
#[test]
fn xor_parity_roundtrip_shard2() {
    let data = Bytes::from("xor parity test data");
    let encoded = XorParityFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let decoded = XorReconstructFn.execute(vec![encoded], &p(&[("missing","2")])).unwrap();
    assert_eq!(decoded, data);
}
#[test]
fn xor_parity_roundtrip_parity_shard() {
    let data = Bytes::from("xor parity test data");
    let encoded = XorParityFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    // Missing the parity shard (index 3 for shard_count=3)
    let decoded = XorReconstructFn.execute(vec![encoded], &p(&[("missing","3")])).unwrap();
    assert_eq!(decoded, data);
}
#[test]
fn xor_parity_empty_error() {
    let r = XorParityFn.execute(vec![Bytes::from("")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- XorReconstructFn (#470) ----
#[test]
fn xor_reconstruct_shard_count_2() {
    let data = Bytes::from("two shard xor");
    let encoded = XorParityFn.execute(vec![data.clone()], &p(&[("shard_count","2")])).unwrap();
    let decoded = XorReconstructFn.execute(vec![encoded], &p(&[("shard_count","2"),("missing","0")])).unwrap();
    assert_eq!(decoded, data);
}
#[test]
fn xor_reconstruct_shard_count_5() {
    let data = Bytes::from("five shard xor test data here!!");
    let encoded = XorParityFn.execute(vec![data.clone()], &p(&[("shard_count","5")])).unwrap();
    let decoded = XorReconstructFn.execute(vec![encoded], &p(&[("shard_count","5"),("missing","3")])).unwrap();
    assert_eq!(decoded, data);
}
#[test]
fn xor_reconstruct_out_of_range() {
    let data = Bytes::from("test");
    let encoded = XorParityFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let r = XorReconstructFn.execute(vec![encoded], &p(&[("missing","99")]));
    assert!(r.is_err());
}
#[test]
fn xor_reconstruct_missing_param() {
    let data = Bytes::from("test");
    let encoded = XorParityFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let r = XorReconstructFn.execute(vec![encoded], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn xor_reconstruct_binary_data() {
    let data = Bytes::from(vec![0u8, 1, 2, 255, 254, 253, 128, 64, 32]);
    let encoded = XorParityFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let decoded = XorReconstructFn.execute(vec![encoded], &p(&[("missing","1")])).unwrap();
    assert_eq!(decoded, data);
}
