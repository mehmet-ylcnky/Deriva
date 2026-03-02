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




#[test]
fn roundtrip_base64() {
    let input = b"hello world \xF0\x9F\x8C\x8D";
    let enc = exec1(&Base64EncodeFn, input).unwrap();
    let dec = exec1(&Base64DecodeFn, &enc).unwrap();
    assert_eq!(dec.as_ref(), input);
}

#[test]
fn roundtrip_hex() {
    let input = b"\x00\xff\x80binary";
    let enc = exec1(&HexEncodeFn, input).unwrap();
    let dec = exec1(&HexDecodeFn, &enc).unwrap();
    assert_eq!(dec.as_ref(), input);
}

#[test]
fn roundtrip_base32() {
    let input = b"roundtrip test data";
    let enc = exec1(&Base32EncodeFn, input).unwrap();
    let dec = exec1(&Base32DecodeFn, &enc).unwrap();
    assert_eq!(dec.as_ref(), input);
}

#[test]
fn roundtrip_aes_ctr() {
    let input = b"secret data for CTR mode";
    let p = params(&[("key", TEST_KEY), ("nonce", TEST_NONCE)]);
    let enc = exec1_params(&EncryptFn, input, p.clone()).unwrap();
    let dec = exec1_params(&DecryptFn, &enc, p).unwrap();
    assert_eq!(dec.as_ref(), input);
}

#[test]
fn roundtrip_aes_gcm() {
    let input = b"secret data for GCM mode";
    let p = params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)]);
    let enc = exec1_params(&AeadEncryptFn, input, p.clone()).unwrap();
    let dec = exec1_params(&AeadDecryptFn, &enc, p).unwrap();
    assert_eq!(dec.as_ref(), input);
}

#[test]
fn roundtrip_zlib() {
    let input = b"compress me with zlib please";
    let c = exec1(&CompressFn, input).unwrap();
    let d = exec1(&DecompressFn, &c).unwrap();
    assert_eq!(d.as_ref(), input);
}

#[test]
fn roundtrip_zstd() {
    let input = b"compress me with zstd please";
    let c = exec1(&ZstdCompressFn, input).unwrap();
    let d = exec1(&ZstdDecompressFn, &c).unwrap();
    assert_eq!(d.as_ref(), input);
}

#[test]
fn roundtrip_lz4() {
    let input = b"compress me with lz4 please";
    let c = exec1(&Lz4CompressFn, input).unwrap();
    let d = exec1(&Lz4DecompressFn, &c).unwrap();
    assert_eq!(d.as_ref(), input);
}

#[test]
fn roundtrip_snappy() {
    let input = b"compress me with snappy please";
    let c = exec1(&SnappyCompressFn, input).unwrap();
    let d = exec1(&SnappyDecompressFn, &c).unwrap();
    assert_eq!(d.as_ref(), input);
}

#[test]
fn roundtrip_brotli() {
    let input = b"compress me with brotli please";
    let c = exec1(&BrotliCompressFn, input).unwrap();
    let d = exec1(&BrotliDecompressFn, &c).unwrap();
    assert_eq!(d.as_ref(), input);
}

#[test]
fn roundtrip_reverse_bytes() {
    let input = b"\x00\x01\x02\xff";
    let r1 = exec1(&ReverseByteFn, input).unwrap();
    let r2 = exec1(&ReverseByteFn, &r1).unwrap();
    assert_eq!(r2.as_ref(), input);
}

#[test]
fn roundtrip_diff_patch() {
    let a = Bytes::from("line one\nline two\nline three\n");
    let b = Bytes::from("line one\nmodified\nline three\nnew line\n");
    let diff = DiffFn.execute(vec![a.clone(), b.clone()], &BTreeMap::new()).unwrap();
    let patched = PatchFn.execute(vec![a, diff], &BTreeMap::new()).unwrap();
    assert_eq!(patched, b);
}

#[test]
fn roundtrip_json_format() {
    let input = b"{\"key\":\"value\",\"num\":42}";
    let pretty = exec1(&JsonPrettyPrintFn, input).unwrap();
    let mini = exec1(&JsonMinifyFn, &pretty).unwrap();
    // Semantic roundtrip: parse both and compare
    let orig: serde_json::Value = serde_json::from_slice(input).unwrap();
    let result: serde_json::Value = serde_json::from_slice(&mini).unwrap();
    assert_eq!(orig, result);
}
