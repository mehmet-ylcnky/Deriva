use bytes::Bytes;
use deriva_compute::builtins::*;
use deriva_compute::function::{ComputeError, ComputeFunction};
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn exec1(f: &dyn ComputeFunction, input: &[u8]) -> Result<Bytes, ComputeError> {
    f.execute(vec![Bytes::from(input.to_vec())], &BTreeMap::new())
}

fn exec1_params(f: &dyn ComputeFunction, input: &[u8], params: BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    f.execute(vec![Bytes::from(input.to_vec())], &params)
}

fn params(kv: &[(&str, &str)]) -> BTreeMap<String, Value> {
    kv.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

const TEST_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TEST_NONCE: &str = "00112233445566778899aabbccddeeff";
const TEST_GCM_NONCE: &str = "000102030405060708090a0b";

fn read_u64(b: &Bytes) -> u64 { u64::from_be_bytes(b[..8].try_into().unwrap()) }
fn read_f64(b: &Bytes) -> f64 { f64::from_be_bytes(b[..8].try_into().unwrap()) }




// ── #99 ReverseByteFn ──

#[test]
fn reverse_bytes_basic() {
    let r = exec1(&ReverseByteFn, b"abcd").unwrap();
    assert_eq!(r.as_ref(), b"dcba");
}

#[test]
fn reverse_bytes_roundtrip() {
    let data = b"hello world";
    let r1 = exec1(&ReverseByteFn, data).unwrap();
    let r2 = exec1(&ReverseByteFn, &r1).unwrap();
    assert_eq!(r2.as_ref(), data);
}

#[test]
fn reverse_bytes_empty() {
    let r = exec1(&ReverseByteFn, b"").unwrap();
    assert!(r.is_empty());
}

#[test]
fn reverse_bytes_single() {
    let r = exec1(&ReverseByteFn, b"x").unwrap();
    assert_eq!(r.as_ref(), b"x");
}

#[test]
fn reverse_bytes_binary() {
    let r = exec1(&ReverseByteFn, &[0x00, 0x01, 0xFF]).unwrap();
    assert_eq!(r.as_ref(), &[0xFF, 0x01, 0x00]);
}


// ── #100 SortBytesFn ──

#[test]
fn sort_bytes_basic() {
    let r = exec1(&SortBytesFn, b"dcba").unwrap();
    assert_eq!(r.as_ref(), b"abcd");
}

#[test]
fn sort_bytes_already_sorted() {
    let r = exec1(&SortBytesFn, b"abcd").unwrap();
    assert_eq!(r.as_ref(), b"abcd");
}

#[test]
fn sort_bytes_empty() {
    let r = exec1(&SortBytesFn, b"").unwrap();
    assert!(r.is_empty());
}

#[test]
fn sort_bytes_all_same() {
    let r = exec1(&SortBytesFn, b"aaaa").unwrap();
    assert_eq!(r.as_ref(), b"aaaa");
}

#[test]
fn sort_bytes_idempotent() {
    let r1 = exec1(&SortBytesFn, b"zxywvu").unwrap();
    let r2 = exec1(&SortBytesFn, &r1).unwrap();
    assert_eq!(r1, r2);
}


