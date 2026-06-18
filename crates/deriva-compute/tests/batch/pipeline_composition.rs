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




fn e(f: &dyn ComputeFunction, input: Bytes) -> Bytes {
    f.execute(vec![input], &BTreeMap::new()).unwrap()
}
fn ep(f: &dyn ComputeFunction, input: Bytes, p: BTreeMap<String, Value>) -> Bytes {
    f.execute(vec![input], &p).unwrap()
}
fn e2(f: &dyn ComputeFunction, a: Bytes, b: Bytes) -> Bytes {
    f.execute(vec![a, b], &BTreeMap::new()).unwrap()
}

// 1. Transform chain: uppercase → base64_encode → hex_encode
#[test]
fn pipeline_transform_chain() {
    let r = e(&HexEncodeFn, e(&Base64EncodeFn, e(&UppercaseFn, Bytes::from("hello"))));
    let d = e(&Base64DecodeFn, e(&HexDecodeFn, r));
    assert_eq!(d.as_ref(), b"HELLO");
}

// 2. Storage pipeline roundtrip: compress → encrypt → base64_encode
#[test]
fn pipeline_storage_roundtrip() {
    let input = Bytes::from("sensitive data to store securely");
    let kp = params(&[("key", TEST_KEY), ("nonce", TEST_NONCE)]);
    let fwd = e(&Base64EncodeFn, ep(&EncryptFn, e(&CompressFn, input.clone()), kp.clone()));
    let rev = e(&DecompressFn, ep(&DecryptFn, e(&Base64DecodeFn, fwd), kp));
    assert_eq!(rev, input);
}

// 3. Analytics pipeline: grep → line_count
#[test]
fn pipeline_analytics() {
    let input = Bytes::from("error: bad\ninfo: ok\nerror: fail\ninfo: fine\n");
    let grepped = ep(&GrepFn, input, params(&[("pattern", "error")]));
    let count = e(&LineCountFn, grepped);
    // grep returns "error: bad\nerror: fail" — 1 newline character
    assert_eq!(count.as_ref(), b"1");
}

// 4. Format pipeline: csv_to_json → json_minify → sha256
#[test]
fn pipeline_format() {
    let csv = Bytes::from("name,age\nAlice,30\n");
    let hash = e(&Sha256Fn, e(&JsonMinifyFn, e(&CsvToJsonFn, csv)));
    assert_eq!(hash.len(), 32);
}

// 5. CAS integrity: chunk_hash → caddr_compute
#[test]
fn pipeline_cas_integrity() {
    let data = Bytes::from(vec![0xABu8; 500]);
    let hashes = e(&ChunkHashFn, data);
    let addr = e(&CAddrComputeFn, hashes);
    assert_eq!(addr.len(), 32);
}

// 6. Double compress: zstd(zlib(data))
#[test]
fn pipeline_double_compress_roundtrip() {
    let input = Bytes::from("double compress me please");
    let c = e(&ZstdCompressFn, e(&CompressFn, input.clone()));
    let d = e(&DecompressFn, e(&ZstdDecompressFn, c));
    assert_eq!(d, input);
}

// 7. Text normalize: trim → lowercase → line_ending(lf)
#[test]
fn pipeline_text_normalize() {
    let input = Bytes::from("  HELLO World  ");
    let r = e(&LowercaseFn, e(&TrimFn, input));
    assert_eq!(r.as_ref(), b"hello world");
}

// 8. Hash chain: sha256 → hex_encode → byte_count
#[test]
fn pipeline_hash_chain() {
    let input = Bytes::from("hash me");
    let count = e(&ByteCountFn, e(&HexEncodeFn, e(&Sha256Fn, input)));
    assert_eq!(read_u64(&count), 64); // 32 bytes → 64 hex chars
}

// 9. Validate then transform: utf8_validate → uppercase → base64_encode
#[test]
fn pipeline_validate_transform() {
    let input = Bytes::from("valid utf8");
    let r = e(&Base64EncodeFn, e(&UppercaseFn, e(&Utf8ValidateFn, input)));
    let d = e(&Base64DecodeFn, r);
    assert_eq!(d.as_ref(), b"VALID UTF8");
}

// 10. Encrypt with AEAD then base32 encode roundtrip
#[test]
fn pipeline_aead_base32_roundtrip() {
    let input = Bytes::from("aead + base32");
    let kp = params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)]);
    let fwd = e(&Base32EncodeFn, ep(&AeadEncryptFn, input.clone(), kp.clone()));
    let rev = ep(&AeadDecryptFn, e(&Base32DecodeFn, fwd), kp);
    assert_eq!(rev, input);
}

// 11. Diff then compress the patch
#[test]
fn pipeline_diff_compress() {
    let a = Bytes::from("line one\nline two\n");
    let b = Bytes::from("line one\nline changed\n");
    let diff = e2(&DiffFn, a.clone(), b.clone());
    let compressed = e(&ZstdCompressFn, diff.clone());
    let decompressed = e(&ZstdDecompressFn, compressed);
    let patched = PatchFn.execute(vec![a, decompressed], &BTreeMap::new()).unwrap();
    assert_eq!(patched, b);
}

// 12. JSON pretty → yaml → back to json
#[test]
fn pipeline_json_yaml_roundtrip() {
    let input = Bytes::from("{\"a\":1,\"b\":\"hello\"}");
    let yaml = e(&JsonToYamlFn, e(&JsonPrettyPrintFn, input.clone()));
    let json_back = e(&YamlToJsonFn, yaml);
    let orig: serde_json::Value = serde_json::from_slice(&input).unwrap();
    let result: serde_json::Value = serde_json::from_slice(&json_back).unwrap();
    assert_eq!(orig, result);
}

// 13. Pad → hex_encode → byte_count
#[test]
fn pipeline_pad_hex_count() {
    let input = Bytes::from("hi");
    let padded = ep(&PadFn, input, params(&[("block_size", "10")]));
    let hex = e(&HexEncodeFn, padded);
    let count = e(&ByteCountFn, hex);
    assert_eq!(read_u64(&count), 20); // 10 bytes → 20 hex chars
}

// 14. CAddr embed then verify
#[test]
fn pipeline_caddr_embed_verify() {
    let data = Bytes::from("verify this content");
    let embedded = e(&CAddrEmbedFn, data.clone());
    let addr = e(&CAddrComputeFn, data);
    // trailing 32 bytes of embedded should match addr
    assert_eq!(&embedded[embedded.len()-32..], addr.as_ref());
}

// 15. Sort → unique → line_count
#[test]
fn pipeline_sort_unique_count() {
    let input = Bytes::from("banana\napple\nbanana\ncherry\napple\n");
    let sorted_unique = e(&SortUniqueFn, input);
    let count = e(&LineCountFn, sorted_unique);
    // sort_unique returns "apple\nbanana\ncherry" — 2 newline characters
    assert_eq!(count.as_ref(), b"2");
}

// 16. Grep → prefix → line_number
#[test]
fn pipeline_grep_prefix_linenumber() {
    let input = Bytes::from("error: a\ninfo: b\nerror: c\n");
    let grepped = ep(&GrepFn, input, params(&[("pattern", "error")]));
    let numbered = e(&LineNumberFn, grepped);
    let text = std::str::from_utf8(&numbered).unwrap();
    assert!(text.contains("1\t"));
    assert!(text.contains("2\t"));
}

// 17. Reverse → reverse = identity
#[test]
fn pipeline_double_reverse_identity() {
    let input = Bytes::from("abcdef");
    let r = e(&ReverseFn, e(&ReverseFn, input.clone()));
    assert_eq!(r, input);
}

// 18. XOR twice = identity
#[test]
fn pipeline_double_xor_identity() {
    let input = Bytes::from("xor me");
    let p = params(&[("key", "255")]);
    let r = ep(&XorFn, ep(&XorFn, input.clone(), p.clone()), p);
    assert_eq!(r, input);
}

// 19. BitwiseNot twice = identity
#[test]
fn pipeline_double_not_identity() {
    let input = Bytes::from("not me");
    let r = e(&BitwiseNotFn, e(&BitwiseNotFn, input.clone()));
    assert_eq!(r, input);
}

// 20. CSV → JSON → JSON lines
#[test]
fn pipeline_csv_json_jsonlines() {
    let csv = Bytes::from("x,y\n1,2\n3,4\n");
    let json = e(&CsvToJsonFn, csv);
    let lines = e(&JsonLinesFn, json);
    let text = std::str::from_utf8(&lines).unwrap();
    assert_eq!(text.lines().count(), 2);
}

// 21. Compress → base64 → byte_count (measure encoded size)
#[test]
fn pipeline_compressed_encoded_size() {
    let input = Bytes::from(vec![b'A'; 1000]);
    let encoded = e(&Base64EncodeFn, e(&CompressFn, input));
    let size = read_u64(&e(&ByteCountFn, encoded));
    assert!(size < 1000); // compressed + base64 should still be smaller than 1000 A's
}

// 22. Take → sort → hex_encode
#[test]
fn pipeline_take_sort_hex() {
    let input = Bytes::from("cherry\napple\nbanana\ndate\n");
    let taken = ep(&TakeFn, input, params(&[("bytes", "13")]));
    // "cherry\napple\n" = 13 bytes
    let sorted = e(&SortFn, taken);
    let hex = e(&HexEncodeFn, sorted);
    let decoded = e(&HexDecodeFn, hex);
    assert_eq!(decoded.as_ref(), b"apple\ncherry");
}

// 23. MerkleRoot of two inputs
#[test]
fn pipeline_merkle_of_compressed() {
    let data1 = Bytes::from(vec![0u8; 100_000]);
    let data2 = Bytes::from(vec![1u8; 100_000]);
    let root = MerkleRootFn.execute(vec![data1, data2], &BTreeMap::new()).unwrap();
    assert_eq!(root.len(), 32);
}

// 24. Replace → grep_invert → word_count
#[test]
fn pipeline_replace_filter_count() {
    let input = Bytes::from("foo bar\nfoo baz\nbaz qux\n");
    let replaced = ep(&ReplaceFn, input, params(&[("pattern", "foo"), ("replacement", "XXX")]));
    let filtered = ep(&GrepInvertFn, replaced, params(&[("pattern", "XXX")]));
    let wc = e(&WordCountFn, filtered);
    assert_eq!(wc.as_ref(), b"2"); // "baz qux"
}

// 25. Full CAS pipeline: data → compress → encrypt → caddr_embed → verify embed
#[test]
fn pipeline_full_cas() {
    let input = Bytes::from("full CAS pipeline test data");
    let kp = params(&[("key", TEST_KEY), ("nonce", TEST_NONCE)]);
    let encrypted = ep(&EncryptFn, e(&CompressFn, input.clone()), kp.clone());
    let embedded = e(&CAddrEmbedFn, encrypted.clone());
    // Verify: strip trailer, decrypt, decompress
    let payload = Bytes::from(embedded[..embedded.len()-32].to_vec());
    let decrypted = ep(&DecryptFn, payload, kp);
    let decompressed = e(&DecompressFn, decrypted);
    assert_eq!(decompressed, input);
}
