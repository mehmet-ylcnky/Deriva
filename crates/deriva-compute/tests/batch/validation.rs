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




// ── #76 Utf8ValidateFn ──

#[test]
fn utf8_validate_valid_ascii() {
    let r = exec1(&Utf8ValidateFn, b"hello world").unwrap();
    assert_eq!(r.as_ref(), b"hello world");
}

#[test]
fn utf8_validate_valid_multibyte() {
    let r = exec1(&Utf8ValidateFn, "héllo wörld".as_bytes()).unwrap();
    assert_eq!(r.as_ref(), "héllo wörld".as_bytes());
}

#[test]
fn utf8_validate_empty() {
    let r = exec1(&Utf8ValidateFn, b"").unwrap();
    assert!(r.is_empty());
}

#[test]
fn utf8_validate_invalid() {
    let r = exec1(&Utf8ValidateFn, &[0xFF, 0xFE]);
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn utf8_validate_truncated_sequence() {
    let r = exec1(&Utf8ValidateFn, &[0xC3]);
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}


// ── #77 JsonValidateFn ──

#[test]
fn json_validate_object() {
    let r = exec1(&JsonValidateFn, b"{\"key\": \"value\"}").unwrap();
    assert_eq!(r.as_ref(), b"{\"key\": \"value\"}");
}

#[test]
fn json_validate_null() {
    let r = exec1(&JsonValidateFn, b"null").unwrap();
    assert_eq!(r.as_ref(), b"null");
}

#[test]
fn json_validate_array() {
    let r = exec1(&JsonValidateFn, b"[1,2,3]").unwrap();
    assert_eq!(r.as_ref(), b"[1,2,3]");
}

#[test]
fn json_validate_invalid() {
    let r = exec1(&JsonValidateFn, b"{invalid}");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn json_validate_empty() {
    let r = exec1(&JsonValidateFn, b"");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}


// ── #78 SchemaValidateFn ──

#[test]
fn schema_validate_pass() {
    let schema = r#"{"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}"#;
    let r = exec1_params(&SchemaValidateFn, b"{\"name\":\"test\"}", params(&[("schema", schema)])).unwrap();
    assert_eq!(r.as_ref(), b"{\"name\":\"test\"}");
}

#[test]
fn schema_validate_fail_missing_required() {
    let schema = r#"{"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}"#;
    let r = exec1_params(&SchemaValidateFn, b"{\"age\":42}", params(&[("schema", schema)]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn schema_validate_fail_wrong_type() {
    let schema = r#"{"type":"object","properties":{"age":{"type":"integer"}}}"#;
    let r = exec1_params(&SchemaValidateFn, b"{\"age\":\"not a number\"}", params(&[("schema", schema)]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn schema_validate_empty_schema_accepts_all() {
    let r = exec1_params(&SchemaValidateFn, b"42", params(&[("schema", "{}")])).unwrap();
    assert_eq!(r.as_ref(), b"42");
}

#[test]
fn schema_validate_invalid_schema_json() {
    let r = exec1_params(&SchemaValidateFn, b"{}", params(&[("schema", "{bad")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}


// ── #79 MagicBytesFn ──

#[test]
fn magic_bytes_png() {
    let mut input = vec![0x89, 0x50, 0x4E, 0x47];
    input.extend_from_slice(b"rest of file");
    let r = exec1_params(&MagicBytesFn, &input, params(&[("expected", "89504e47")])).unwrap();
    assert_eq!(r.as_ref(), input.as_slice());
}

#[test]
fn magic_bytes_mismatch() {
    let r = exec1_params(&MagicBytesFn, b"not a png", params(&[("expected", "89504e47")]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn magic_bytes_too_short() {
    let r = exec1_params(&MagicBytesFn, &[0x89], params(&[("expected", "89504e47")]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn magic_bytes_empty_expected() {
    let r = exec1_params(&MagicBytesFn, b"anything", params(&[("expected", "")])).unwrap();
    assert_eq!(r.as_ref(), b"anything");
}

#[test]
fn magic_bytes_exact_match() {
    let r = exec1_params(&MagicBytesFn, &[0xCA, 0xFE], params(&[("expected", "cafe")])).unwrap();
    assert_eq!(r.as_ref(), &[0xCA, 0xFE]);
}


// ── #80 SizeLimitFn ──

#[test]
fn size_limit_within() {
    let r = exec1_params(&SizeLimitFn, b"hello", params(&[("max_bytes", "10")])).unwrap();
    assert_eq!(r.as_ref(), b"hello");
}

#[test]
fn size_limit_exact() {
    let r = exec1_params(&SizeLimitFn, b"hello", params(&[("max_bytes", "5")])).unwrap();
    assert_eq!(r.as_ref(), b"hello");
}

#[test]
fn size_limit_exceeded() {
    let r = exec1_params(&SizeLimitFn, b"hello!", params(&[("max_bytes", "5")]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn size_limit_empty() {
    let r = exec1_params(&SizeLimitFn, b"", params(&[("max_bytes", "1")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn size_limit_one_over() {
    let r = exec1_params(&SizeLimitFn, b"ab", params(&[("max_bytes", "1")]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}


// ── #81 NonEmptyFn ──

#[test]
fn non_empty_passes() {
    let r = exec1(&NonEmptyFn, b"data").unwrap();
    assert_eq!(r.as_ref(), b"data");
}

#[test]
fn non_empty_single_byte() {
    let r = exec1(&NonEmptyFn, &[0x00]).unwrap();
    assert_eq!(r.len(), 1);
}

#[test]
fn non_empty_fails() {
    let r = exec1(&NonEmptyFn, b"");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn non_empty_whitespace_passes() {
    let r = exec1(&NonEmptyFn, b" ").unwrap();
    assert_eq!(r.as_ref(), b" ");
}

#[test]
fn non_empty_large_input() {
    let data = vec![0xAB; 10000];
    let r = exec1(&NonEmptyFn, &data).unwrap();
    assert_eq!(r.len(), 10000);
}


// ── #82 Sha256VerifyFn ──

#[test]
fn sha256_verify_correct() {
    use sha2::{Sha256, Digest};
    let data = b"hello world";
    let hash = Sha256::digest(data);
    let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
    let r = exec1_params(&Sha256VerifyFn, data, params(&[("expected_hash", &hex)])).unwrap();
    assert_eq!(r.as_ref(), data);
}

#[test]
fn sha256_verify_mismatch() {
    let r = exec1_params(&Sha256VerifyFn, b"hello", params(&[("expected_hash", "0000000000000000000000000000000000000000000000000000000000000000")]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn sha256_verify_wrong_length() {
    let r = exec1_params(&Sha256VerifyFn, b"data", params(&[("expected_hash", "abcd")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn sha256_verify_empty_input() {
    use sha2::{Sha256, Digest};
    let hash = Sha256::digest(b"");
    let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
    let r = exec1_params(&Sha256VerifyFn, b"", params(&[("expected_hash", &hex)])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn sha256_verify_passthrough() {
    use sha2::{Sha256, Digest};
    let data = vec![0xDE; 1000];
    let hash = Sha256::digest(&data);
    let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
    let r = exec1_params(&Sha256VerifyFn, &data, params(&[("expected_hash", &hex)])).unwrap();
    assert_eq!(r.as_ref(), data.as_slice());
}


// ── #83 Crc32VerifyFn ──

#[test]
fn crc32_verify_correct() {
    let data = b"hello world";
    let crc = crc32fast::hash(data);
    let hex = format!("{:08x}", crc);
    let r = exec1_params(&Crc32VerifyFn, data, params(&[("expected_crc32", &hex)])).unwrap();
    assert_eq!(r.as_ref(), data);
}

#[test]
fn crc32_verify_mismatch() {
    let r = exec1_params(&Crc32VerifyFn, b"hello", params(&[("expected_crc32", "00000000")]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn crc32_verify_wrong_length() {
    let r = exec1_params(&Crc32VerifyFn, b"data", params(&[("expected_crc32", "ab")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn crc32_verify_empty_input() {
    let crc = crc32fast::hash(b"");
    let hex = format!("{:08x}", crc);
    let r = exec1_params(&Crc32VerifyFn, b"", params(&[("expected_crc32", &hex)])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn crc32_verify_passthrough() {
    let data = vec![0xFF; 500];
    let crc = crc32fast::hash(&data);
    let hex = format!("{:08x}", crc);
    let r = exec1_params(&Crc32VerifyFn, &data, params(&[("expected_crc32", &hex)])).unwrap();
    assert_eq!(r.as_ref(), data.as_slice());
}


