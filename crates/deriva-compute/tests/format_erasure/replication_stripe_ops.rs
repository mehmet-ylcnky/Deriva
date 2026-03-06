use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_erasure::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- ReplicationSplitFn (#471) ----
#[test]
fn replication_split_3() {
    let data = Bytes::from("replicate me");
    let r = ReplicationSplitFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    assert_eq!(r.len(), data.len() * 3);
}
#[test]
fn replication_split_1() {
    let data = Bytes::from("single");
    let r = ReplicationSplitFn.execute(vec![data.clone()], &p(&[("replicas","1")])).unwrap();
    assert_eq!(r, data);
}
#[test]
fn replication_split_verify_roundtrip() {
    let data = Bytes::from("roundtrip replication");
    let split = ReplicationSplitFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let verified = ReplicationVerifyFn.execute(vec![split], &BTreeMap::new()).unwrap();
    assert_eq!(verified, data);
}
#[test]
fn replication_split_zero_error() {
    let r = ReplicationSplitFn.execute(vec![Bytes::from("x")], &p(&[("replicas","0")]));
    assert!(r.is_err());
}
#[test]
fn replication_split_5() {
    let data = Bytes::from("five copies");
    let r = ReplicationSplitFn.execute(vec![data.clone()], &p(&[("replicas","5")])).unwrap();
    assert_eq!(r.len(), data.len() * 5);
}

// ---- ReplicationVerifyFn (#472) ----
#[test]
fn replication_verify_valid() {
    let data = Bytes::from("verify me");
    let split = ReplicationSplitFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let r = ReplicationVerifyFn.execute(vec![split], &BTreeMap::new()).unwrap();
    assert_eq!(r, data);
}
#[test]
fn replication_verify_tampered() {
    let data = Bytes::from("tamper test");
    let mut split = ReplicationSplitFn.execute(vec![data], &BTreeMap::new()).unwrap().to_vec();
    // Tamper with second replica
    let rs = split.len() / 3;
    split[rs] ^= 0xFF;
    let r = ReplicationVerifyFn.execute(vec![Bytes::from(split)], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn replication_verify_returns_single_copy() {
    let data = Bytes::from("single copy output");
    let split = ReplicationSplitFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let verified = ReplicationVerifyFn.execute(vec![split], &BTreeMap::new()).unwrap();
    assert_eq!(verified.len(), data.len());
}
#[test]
fn replication_verify_not_divisible() {
    let r = ReplicationVerifyFn.execute(vec![Bytes::from("abc")], &p(&[("replicas","2")]));
    assert!(r.is_err());
}
#[test]
fn replication_verify_zero_error() {
    let r = ReplicationVerifyFn.execute(vec![Bytes::from("x")], &p(&[("replicas","0")]));
    assert!(r.is_err());
}

// ---- StripeSplitFn (#473) ----
#[test]
fn stripe_split_assemble_roundtrip() {
    let data = Bytes::from(vec![0x42u8; 256]);
    let split = StripeSplitFn.execute(vec![data.clone()], &p(&[("stripe_count","4"),("stripe_size","64")])).unwrap();
    let assembled = StripeAssembleFn.execute(vec![split], &p(&[("stripe_count","4"),("stripe_size","64")])).unwrap();
    assert_eq!(assembled, data);
}
#[test]
fn stripe_split_small_input() {
    let data = Bytes::from("small");
    let split = StripeSplitFn.execute(vec![data.clone()], &p(&[("stripe_count","2"),("stripe_size","16")])).unwrap();
    let assembled = StripeAssembleFn.execute(vec![split], &p(&[("stripe_count","2"),("stripe_size","16")])).unwrap();
    assert_eq!(assembled, data);
}
#[test]
fn stripe_split_output_has_length_prefix() {
    let data = Bytes::from("length prefix");
    let split = StripeSplitFn.execute(vec![data.clone()], &p(&[("stripe_count","2"),("stripe_size","16")])).unwrap();
    let stored_len = u32::from_le_bytes(split[..4].try_into().unwrap()) as usize;
    assert_eq!(stored_len, data.len());
}
#[test]
fn stripe_split_zero_count_error() {
    let r = StripeSplitFn.execute(vec![Bytes::from("x")], &p(&[("stripe_count","0")]));
    assert!(r.is_err());
}
#[test]
fn stripe_split_3_stripes() {
    let data = Bytes::from(vec![0xABu8; 192]);
    let split = StripeSplitFn.execute(vec![data.clone()], &p(&[("stripe_count","3"),("stripe_size","64")])).unwrap();
    let assembled = StripeAssembleFn.execute(vec![split], &p(&[("stripe_count","3"),("stripe_size","64")])).unwrap();
    assert_eq!(assembled, data);
}

// ---- StripeAssembleFn (#474) ----
#[test]
fn stripe_assemble_too_short() {
    let r = StripeAssembleFn.execute(vec![Bytes::from("ab")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn stripe_assemble_preserves_length() {
    let data = Bytes::from(vec![0x42u8; 100]);
    let split = StripeSplitFn.execute(vec![data.clone()], &p(&[("stripe_count","2"),("stripe_size","32")])).unwrap();
    let assembled = StripeAssembleFn.execute(vec![split], &p(&[("stripe_count","2"),("stripe_size","32")])).unwrap();
    assert_eq!(assembled.len(), 100);
}
#[test]
fn stripe_assemble_binary() {
    let data = Bytes::from(vec![0u8, 1, 2, 255, 254, 253, 128, 64]);
    let split = StripeSplitFn.execute(vec![data.clone()], &p(&[("stripe_count","2"),("stripe_size","4")])).unwrap();
    let assembled = StripeAssembleFn.execute(vec![split], &p(&[("stripe_count","2"),("stripe_size","4")])).unwrap();
    assert_eq!(assembled, data);
}
#[test]
fn stripe_assemble_large() {
    let data = Bytes::from(vec![0x42u8; 1024]);
    let split = StripeSplitFn.execute(vec![data.clone()], &p(&[("stripe_count","4"),("stripe_size","128")])).unwrap();
    let assembled = StripeAssembleFn.execute(vec![split], &p(&[("stripe_count","4"),("stripe_size","128")])).unwrap();
    assert_eq!(assembled, data);
}
#[test]
fn stripe_assemble_non_aligned() {
    let data = Bytes::from(vec![0x42u8; 100]); // not aligned to stripe_size=64
    let split = StripeSplitFn.execute(vec![data.clone()], &p(&[("stripe_count","2"),("stripe_size","64")])).unwrap();
    let assembled = StripeAssembleFn.execute(vec![split], &p(&[("stripe_count","2"),("stripe_size","64")])).unwrap();
    assert_eq!(assembled, data);
}
