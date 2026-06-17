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




// ── §6.1 Universal Edge Cases ──

// Empty input (0 bytes)
#[test]
fn empty_input_byte_level_functions() {
    assert!(exec1(&UppercaseFn, b"").unwrap().is_empty());
    assert!(exec1(&LowercaseFn, b"").unwrap().is_empty());
    assert!(exec1(&ReverseFn, b"").unwrap().is_empty());
    assert!(exec1(&TrimFn, b"").unwrap().is_empty());
    assert!(exec1(&BitwiseNotFn, b"").unwrap().is_empty());
    assert!(exec1(&ReverseByteFn, b"").unwrap().is_empty());
    assert!(exec1(&SortBytesFn, b"").unwrap().is_empty());
}

#[test]
fn empty_input_encoding_functions() {
    assert!(exec1(&Base64EncodeFn, b"").unwrap().is_empty());
    assert!(exec1(&HexEncodeFn, b"").unwrap().is_empty());
    assert!(exec1(&Base32EncodeFn, b"").unwrap().is_empty());
}

#[test]
fn empty_input_hash_functions() {
    assert_eq!(exec1(&Sha256Fn, b"").unwrap().len(), 32);
    assert_eq!(exec1(&Sha512Fn, b"").unwrap().len(), 64);
    assert_eq!(exec1(&Blake3Fn, b"").unwrap().len(), 32);
    assert_eq!(exec1(&Md5Fn, b"").unwrap().len(), 16);
    assert_eq!(exec1(&Crc32Fn, b"").unwrap().len(), 4);
}

#[test]
fn empty_input_accumulators() {
    assert_eq!(read_u64(&exec1(&ByteCountFn, b"").unwrap()), 0);
    assert_eq!(exec1(&LineCountFn, b"").unwrap().as_ref(), b"0");
    assert_eq!(exec1(&WordCountFn, b"").unwrap().as_ref(), b"0");
}

#[test]
fn empty_input_compression() {
    let c = exec1(&CompressFn, b"").unwrap();
    let d = exec1(&DecompressFn, &c).unwrap();
    assert!(d.is_empty());
}

#[test]
fn empty_input_cas() {
    assert_eq!(exec1(&CAddrComputeFn, b"").unwrap().len(), 32);
    assert_eq!(exec1(&CAddrEmbedFn, b"").unwrap().len(), 32);
    assert!(exec1(&ChunkHashFn, b"").unwrap().is_empty());
    assert_eq!(exec1(&MerkleRootFn, b"").unwrap().len(), 32);
}

// Single byte input
#[test]
fn single_byte_processing() {
    assert_eq!(exec1(&UppercaseFn, b"a").unwrap().as_ref(), b"A");
    assert_eq!(exec1(&LowercaseFn, b"Z").unwrap().as_ref(), b"z");
    assert_eq!(exec1(&ReverseFn, b"x").unwrap().as_ref(), b"x");
    assert_eq!(exec1(&ReverseByteFn, b"x").unwrap().as_ref(), b"x");
    assert_eq!(exec1(&SortBytesFn, b"x").unwrap().as_ref(), b"x");
    assert_eq!(read_u64(&exec1(&ByteCountFn, b"x").unwrap()), 1);
    assert_eq!(exec1(&BitwiseNotFn, b"\x0F").unwrap().as_ref(), &[0xF0]);
}

// Wrong input count
#[test]
fn wrong_input_count_single_input_fns() {
    let two = vec![Bytes::from("a"), Bytes::from("b")];
    assert!(matches!(UppercaseFn.execute(two.clone(), &BTreeMap::new()), Err(ComputeError::InputCount { expected: 1, .. })));
    assert!(matches!(Sha256Fn.execute(two.clone(), &BTreeMap::new()), Err(ComputeError::InputCount { expected: 1, .. })));
    assert!(matches!(CompressFn.execute(two.clone(), &BTreeMap::new()), Err(ComputeError::InputCount { expected: 1, .. })));
    assert!(matches!(ReverseByteFn.execute(two.clone(), &BTreeMap::new()), Err(ComputeError::InputCount { expected: 1, .. })));
    assert!(matches!(CAddrComputeFn.execute(two.clone(), &BTreeMap::new()), Err(ComputeError::InputCount { expected: 1, .. })));
}

#[test]
fn wrong_input_count_multi_input_fns() {
    let one = vec![Bytes::from("a")];
    assert!(matches!(DiffFn.execute(one.clone(), &BTreeMap::new()), Err(ComputeError::InputCount { expected: 2, .. })));
    assert!(matches!(PatchFn.execute(one.clone(), &BTreeMap::new()), Err(ComputeError::InputCount { expected: 2, .. })));
    assert!(matches!(InterleaveFn.execute(one.clone(), &BTreeMap::new()), Err(ComputeError::InputCount { expected: 2, .. })));
}

// Non-UTF-8 input
#[test]
fn non_utf8_text_functions_fail() {
    let bad = &[0xFF, 0xFE, 0x80];
    let rp = params(&[("pattern", "x"), ("replacement", "y")]);
    assert!(matches!(exec1_params(&ReplaceFn, bad, rp), Err(ComputeError::ExecutionFailed(_))));
    assert!(matches!(exec1(&LineNumberFn, bad), Err(ComputeError::ExecutionFailed(_))));
    assert!(matches!(exec1(&Utf8ValidateFn, bad), Err(ComputeError::ExecutionFailed(_))));
    assert!(matches!(exec1(&JsonPrettyPrintFn, bad), Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn non_utf8_byte_functions_succeed() {
    let bad = &[0xFF, 0xFE, 0x80];
    assert!(exec1(&BitwiseNotFn, bad).is_ok());
    assert!(exec1(&Sha256Fn, bad).is_ok());
    assert!(exec1(&CompressFn, bad).is_ok());
    assert!(exec1(&ReverseByteFn, bad).is_ok());
    assert!(exec1(&SortBytesFn, bad).is_ok());
    assert!(exec1(&CAddrComputeFn, bad).is_ok());
}

// ── §6.2 Parameter Edge Cases ──

// Missing required param
#[test]
fn missing_required_param() {
    assert!(matches!(exec1(&XorFn, b"data"), Err(ComputeError::InvalidParam(_))));
    assert!(matches!(exec1(&EncryptFn, b"data"), Err(ComputeError::InvalidParam(_))));
    assert!(matches!(exec1(&GrepFn, b"data"), Err(ComputeError::InvalidParam(_))));
    assert!(matches!(exec1(&RegexReplaceFn, b"data"), Err(ComputeError::InvalidParam(_))));
}

// Param wrong type (Int instead of String where string expected)
#[test]
fn param_wrong_type() {
    let mut p = BTreeMap::new();
    p.insert("pattern".to_string(), Value::Int(42));
    assert!(matches!(GrepFn.execute(vec![Bytes::from("data")], &p), Err(ComputeError::InvalidParam(_))));
}

// Numeric param = 0 where invalid
#[test]
fn zero_numeric_param() {
    assert!(matches!(
        exec1_params(&MerkleRootFn, b"data", params(&[("block_size", "0")])),
        Err(ComputeError::InvalidParam(_))
    ));
    assert!(matches!(
        exec1_params(&ChunkHashFn, b"data", params(&[("block_size", "0")])),
        Err(ComputeError::InvalidParam(_))
    ));
}

// Numeric param negative via string
#[test]
fn negative_numeric_param() {
    assert!(matches!(
        exec1_params(&TakeFn, b"data", params(&[("bytes", "-1")])),
        Err(ComputeError::InvalidParam(_))
    ));
    assert!(matches!(
        exec1_params(&SkipFn, b"data", params(&[("bytes", "-5")])),
        Err(ComputeError::InvalidParam(_))
    ));
}

// Invalid hex param
#[test]
fn invalid_hex_param() {
    let bad_key = params(&[("key", "not_valid_hex"), ("nonce", TEST_NONCE)]);
    assert!(matches!(exec1_params(&EncryptFn, b"data", bad_key), Err(ComputeError::InvalidParam(_))));
}

// Hex param odd length
#[test]
fn hex_param_odd_length() {
    let odd = params(&[("key", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde"), ("nonce", TEST_NONCE)]);
    assert!(matches!(exec1_params(&EncryptFn, b"data", odd), Err(ComputeError::InvalidParam(_))));
}

// Regex param invalid syntax
#[test]
fn regex_invalid_syntax() {
    let p = params(&[("pattern", "[invalid(")]);
    assert!(matches!(exec1_params(&GrepFn, b"data\n", p.clone()), Err(ComputeError::InvalidParam(_))));
    let rp = params(&[("pattern", "[invalid("), ("replacement", "x")]);
    assert!(matches!(exec1_params(&RegexReplaceFn, b"data", rp), Err(ComputeError::InvalidParam(_) | ComputeError::ExecutionFailed(_))));
}

// Level param out of range
#[test]
fn zstd_level_out_of_range() {
    assert!(matches!(
        exec1_params(&ZstdCompressFn, b"data", params(&[("level", "0")])),
        Err(ComputeError::InvalidParam(_))
    ));
    assert!(matches!(
        exec1_params(&ZstdCompressFn, b"data", params(&[("level", "23")])),
        Err(ComputeError::InvalidParam(_))
    ));
}

#[test]
fn brotli_quality_out_of_range() {
    assert!(matches!(
        exec1_params(&BrotliCompressFn, b"data", params(&[("quality", "12")])),
        Err(ComputeError::InvalidParam(_))
    ));
}

// ── §6.3 Category-Specific Edge Cases ──

// Compression: decompress non-compressed data
#[test]
fn decompress_non_compressed_data() {
    assert!(matches!(exec1(&DecompressFn, b"not compressed"), Err(ComputeError::ExecutionFailed(_))));
    assert!(matches!(exec1(&ZstdDecompressFn, b"not compressed"), Err(ComputeError::ExecutionFailed(_))));
    assert!(matches!(exec1(&Lz4DecompressFn, b"not compressed"), Err(ComputeError::ExecutionFailed(_))));
    assert!(matches!(exec1(&SnappyDecompressFn, b"not compressed"), Err(ComputeError::ExecutionFailed(_))));
    assert!(matches!(exec1(&BrotliDecompressFn, b"not compressed"), Err(ComputeError::ExecutionFailed(_))));
}

// Compression: compress already-compressed data succeeds (may grow)
#[test]
fn compress_already_compressed() {
    let c1 = exec1(&CompressFn, b"some data to compress").unwrap();
    let c2 = exec1(&CompressFn, &c1).unwrap();
    assert!(!c2.is_empty()); // succeeds, doesn't error
    // verify we can decompress both layers
    let d1 = exec1(&DecompressFn, &c2).unwrap();
    let d2 = exec1(&DecompressFn, &d1).unwrap();
    assert_eq!(d2.as_ref(), b"some data to compress");
}

// Crypto: decrypt with wrong key (CTR: silent corruption)
#[test]
fn decrypt_wrong_key_ctr() {
    let p1 = params(&[("key", TEST_KEY), ("nonce", TEST_NONCE)]);
    let enc = exec1_params(&EncryptFn, b"secret", p1).unwrap();
    let wrong_key = params(&[("key", "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"), ("nonce", TEST_NONCE)]);
    // CTR mode: decryption "succeeds" but produces garbage (silent corruption)
    let dec = exec1_params(&DecryptFn, &enc, wrong_key).unwrap();
    assert_ne!(dec.as_ref(), b"secret");
}

// Crypto: AEAD tampered ciphertext
#[test]
fn aead_tampered_ciphertext() {
    let p = params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)]);
    let mut enc = exec1_params(&AeadEncryptFn, b"secret", p.clone()).unwrap().to_vec();
    enc[0] ^= 0xFF;
    assert!(matches!(
        AeadDecryptFn.execute(vec![Bytes::from(enc)], &p),
        Err(ComputeError::ExecutionFailed(_))
    ));
}

// Combiners: different length inputs
#[test]
fn interleave_different_lengths() {
    let a = Bytes::from("ab");
    let b = Bytes::from("1234");
    let r = InterleaveFn.execute(vec![a, b], &BTreeMap::new()).unwrap();
    // Interleave all bytes: a1b2 then remaining 34
    assert_eq!(r.as_ref(), b"a1b234");
}

#[test]
fn zip_concat_different_lengths() {
    let a = Bytes::from("line1\n");
    let b = Bytes::from("line1\nline2\n");
    let r = ZipConcatFn.execute(vec![a, b], &BTreeMap::new()).unwrap();
    assert!(!r.is_empty()); // succeeds with different lengths
}

// Slicing: take N > input length → return full input
#[test]
fn take_beyond_length() {
    let r = exec1_params(&TakeFn, b"short", params(&[("bytes", "9999")])).unwrap();
    assert_eq!(r.as_ref(), b"short");
}

// Slicing: skip N > input length → return empty
#[test]
fn skip_beyond_length() {
    let r = exec1_params(&SkipFn, b"short", params(&[("bytes", "9999")])).unwrap();
    assert!(r.is_empty());
}

// Slicing: slice with start > end
#[test]
fn slice_start_beyond_end() {
    assert!(matches!(
        exec1_params(&SliceFn, b"hello", params(&[("start", "10"), ("end", "5")])),
        Err(ComputeError::InvalidParam(_))
    ));
}

// Text: replace with invalid regex pattern
#[test]
fn replace_invalid_regex_pattern() {
    assert!(matches!(
        exec1_params(&ReplaceFn, b"hello", params(&[("pattern", "[invalid"), ("replacement", "x")])),
        Err(ComputeError::InvalidParam(_))
    ));
}

// Text: grep on binary (non-UTF-8) input — requires UTF-8, fails on binary
#[test]
fn grep_binary_input() {
    let bad = &[0xFF, 0xFE, 0x80];
    let p = params(&[("pattern", "match")]);
    assert!(matches!(exec1_params(&GrepFn, bad, p), Err(ComputeError::ExecutionFailed(_))));
}

// Validation: utf8_validate on valid UTF-8 → passthrough
#[test]
fn utf8_validate_passthrough() {
    let input = "valid UTF-8 with émojis 🎉";
    let r = exec1(&Utf8ValidateFn, input.as_bytes()).unwrap();
    assert_eq!(r.as_ref(), input.as_bytes());
}

// Validation: size_limit at exact boundary → passthrough
#[test]
fn size_limit_exactly_at_limit() {
    let r = exec1_params(&SizeLimitFn, b"12345", params(&[("max_bytes", "5")])).unwrap();
    assert_eq!(r.as_ref(), b"12345");
}

// Validation: size_limit over boundary → error
#[test]
fn size_limit_over_boundary() {
    assert!(matches!(
        exec1_params(&SizeLimitFn, b"123456", params(&[("max_bytes", "5")])),
        Err(ComputeError::ExecutionFailed(_))
    ));
}

// Format: JSON parse of truncated input
#[test]
fn json_parse_truncated() {
    assert!(matches!(exec1(&JsonPrettyPrintFn, b"{\"key\":"), Err(ComputeError::ExecutionFailed(_))));
    assert!(matches!(exec1(&JsonMinifyFn, b"[1,2,"), Err(ComputeError::ExecutionFailed(_))));
    assert!(matches!(exec1(&JsonValidateFn, b"{broken"), Err(ComputeError::ExecutionFailed(_))));
}

// Format: CSV with header only → empty array
#[test]
fn csv_header_only() {
    let r = exec1(&CsvToJsonFn, b"name,age\n").unwrap();
    assert_eq!(r.as_ref(), b"[]");
}

// CAS: CAddrVerify mismatch
#[test]
fn caddr_verify_mismatch() {
    let r = exec1_params(&CAddrVerifyFn, b"data",
        params(&[("expected_caddr", "0000000000000000000000000000000000000000000000000000000000000000")]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

// CAS: MerkleRoot block_size > input → single leaf = hash of full input
#[test]
fn merkle_root_block_larger_than_input() {
    use sha2::{Sha256, Digest};
    let data = b"small";
    let r = exec1_params(&MerkleRootFn, data, params(&[("block_size", "999999")])).unwrap();
    assert_eq!(r.as_ref(), Sha256::digest(data).as_slice());
}

// Batch-Only: sort_bytes all-zero → unchanged
#[test]
fn sort_bytes_all_zero() {
    let input = vec![0u8; 100];
    let r = exec1(&SortBytesFn, &input).unwrap();
    assert_eq!(r.as_ref(), input.as_slice());
}

// ── §6.4 Determinism Guarantees ──

// ShuffleFn deterministic with seed
#[test]
fn shuffle_deterministic_with_seed() {
    let input = b"line1\nline2\nline3\nline4\nline5\n";
    let p = params(&[("seed", "42")]);
    let r1 = exec1_params(&ShuffleFn, input, p.clone()).unwrap();
    let r2 = exec1_params(&ShuffleFn, input, p).unwrap();
    assert_eq!(r1, r2);
}

// All other functions deterministic (spot check)
#[test]
fn determinism_spot_check() {
    let data = b"determinism test data";
    assert_eq!(exec1(&Sha256Fn, data).unwrap(), exec1(&Sha256Fn, data).unwrap());
    assert_eq!(exec1(&CompressFn, data).unwrap(), exec1(&CompressFn, data).unwrap());
    assert_eq!(exec1(&CAddrComputeFn, data).unwrap(), exec1(&CAddrComputeFn, data).unwrap());
    assert_eq!(exec1(&SortBytesFn, data).unwrap(), exec1(&SortBytesFn, data).unwrap());
    assert_eq!(exec1(&DedupAnalyzeFn, data).unwrap(), exec1(&DedupAnalyzeFn, data).unwrap());
}
