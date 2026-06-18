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




// ── #92 CAddrComputeFn ──

#[test]
fn caddr_compute_deterministic() {
    let r1 = exec1(&CAddrComputeFn, b"hello").unwrap();
    let r2 = exec1(&CAddrComputeFn, b"hello").unwrap();
    assert_eq!(r1, r2);
    assert_eq!(r1.len(), 32);
}

#[test]
fn caddr_compute_different_inputs_differ() {
    let r1 = exec1(&CAddrComputeFn, b"hello").unwrap();
    let r2 = exec1(&CAddrComputeFn, b"world").unwrap();
    assert_ne!(r1, r2);
}

#[test]
fn caddr_compute_empty() {
    let r = exec1(&CAddrComputeFn, b"").unwrap();
    assert_eq!(r.len(), 32);
}

#[test]
fn caddr_compute_matches_core() {
    use deriva_core::address::CAddr;
    let data = b"test data";
    let expected = CAddr::from_bytes(data);
    let r = exec1(&CAddrComputeFn, data).unwrap();
    assert_eq!(r.as_ref(), expected.as_bytes());
}

#[test]
fn caddr_compute_binary() {
    let data: Vec<u8> = (0..=255).collect();
    let r = exec1(&CAddrComputeFn, &data).unwrap();
    assert_eq!(r.len(), 32);
}


// ── #93 CAddrVerifyFn ──

#[test]
fn caddr_verify_correct() {
    use deriva_core::address::CAddr;
    let data = b"verify me";
    let addr = CAddr::from_bytes(data);
    let hex: String = addr.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
    let r = exec1_params(&CAddrVerifyFn, data, params(&[("expected_caddr", &hex)])).unwrap();
    assert_eq!(r.as_ref(), data);
}

#[test]
fn caddr_verify_mismatch() {
    let r = exec1_params(&CAddrVerifyFn, b"data", params(&[("expected_caddr", "0000000000000000000000000000000000000000000000000000000000000000")]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn caddr_verify_wrong_length() {
    let r = exec1_params(&CAddrVerifyFn, b"data", params(&[("expected_caddr", "abcd")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn caddr_verify_passthrough() {
    use deriva_core::address::CAddr;
    let data = vec![0xAB; 1000];
    let addr = CAddr::from_bytes(&data);
    let hex: String = addr.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
    let r = exec1_params(&CAddrVerifyFn, &data, params(&[("expected_caddr", &hex)])).unwrap();
    assert_eq!(r.as_ref(), data.as_slice());
}

#[test]
fn caddr_verify_empty() {
    use deriva_core::address::CAddr;
    let addr = CAddr::from_bytes(b"");
    let hex: String = addr.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
    let r = exec1_params(&CAddrVerifyFn, b"", params(&[("expected_caddr", &hex)])).unwrap();
    assert!(r.is_empty());
}


// ── #94 CAddrEmbedFn ──

#[test]
fn caddr_embed_appends_32_bytes() {
    let r = exec1(&CAddrEmbedFn, b"hello").unwrap();
    assert_eq!(r.len(), 5 + 32);
    assert_eq!(&r[..5], b"hello");
}

#[test]
fn caddr_embed_trailing_matches_compute() {
    let data = b"test data";
    let embedded = exec1(&CAddrEmbedFn, data).unwrap();
    let computed = exec1(&CAddrComputeFn, data).unwrap();
    assert_eq!(&embedded[data.len()..], computed.as_ref());
}

#[test]
fn caddr_embed_empty() {
    let r = exec1(&CAddrEmbedFn, b"").unwrap();
    assert_eq!(r.len(), 32);
}

#[test]
fn caddr_embed_verifiable() {
    use deriva_core::address::CAddr;
    let data = b"verify this";
    let embedded = exec1(&CAddrEmbedFn, data).unwrap();
    let original = &embedded[..data.len()];
    let trailing = &embedded[data.len()..];
    let recomputed = CAddr::from_bytes(original);
    assert_eq!(recomputed.as_bytes(), trailing);
}

#[test]
fn caddr_embed_not_idempotent() {
    let r1 = exec1(&CAddrEmbedFn, b"data").unwrap();
    let r2 = exec1(&CAddrEmbedFn, &r1).unwrap();
    assert_ne!(r1.len(), r2.len());
    assert_eq!(r2.len(), r1.len() + 32);
}


// ── #95 MerkleRootFn (Spec Req 12.3: 2+ inputs, BLAKE3) ──

#[test]
fn merkle_root_deterministic() {
    let a = Bytes::from(vec![0u8; 100_000]);
    let b = Bytes::from(vec![1u8; 100_000]);
    let r1 = MerkleRootFn.execute(vec![a.clone(), b.clone()], &BTreeMap::new()).unwrap();
    let r2 = MerkleRootFn.execute(vec![a, b], &BTreeMap::new()).unwrap();
    assert_eq!(r1, r2);
    assert_eq!(r1.len(), 32);
}

#[test]
fn merkle_root_two_inputs() {
    let a = Bytes::from("input_a");
    let b = Bytes::from("input_b");
    let r = MerkleRootFn.execute(vec![a, b], &BTreeMap::new()).unwrap();
    assert_eq!(r.len(), 32);
}

#[test]
fn merkle_root_matches_manual_blake3() {
    let a = Bytes::from("left");
    let b = Bytes::from("right");
    let r = MerkleRootFn.execute(vec![a.clone(), b.clone()], &BTreeMap::new()).unwrap();
    // Manual computation: hash leaves, then hash pair
    let h_a = blake3::hash(b"left");
    let h_b = blake3::hash(b"right");
    let mut root_hasher = blake3::Hasher::new();
    root_hasher.update(h_a.as_bytes());
    root_hasher.update(h_b.as_bytes());
    let expected = root_hasher.finalize();
    assert_eq!(r.as_ref(), expected.as_bytes());
}

#[test]
fn merkle_root_three_inputs() {
    let a = Bytes::from("a");
    let b = Bytes::from("b");
    let c = Bytes::from("c");
    let r = MerkleRootFn.execute(vec![a, b, c], &BTreeMap::new()).unwrap();
    assert_eq!(r.len(), 32);
}

#[test]
fn merkle_root_single_input_error() {
    let r = exec1(&MerkleRootFn, b"data");
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, .. })));
}

#[test]
fn merkle_root_empty_inputs_error() {
    let r = MerkleRootFn.execute(vec![], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, .. })));
}


// ── #96 ContentTypeFn ──

#[test]
fn content_type_png() {
    let r = exec1(&ContentTypeFn, &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();
    assert_eq!(r.as_ref(), b"image/png");
}

#[test]
fn content_type_json() {
    let r = exec1(&ContentTypeFn, b"{\"key\":\"value\"}").unwrap();
    assert_eq!(r.as_ref(), b"application/json");
}

#[test]
fn content_type_text() {
    let r = exec1(&ContentTypeFn, b"hello world").unwrap();
    assert_eq!(r.as_ref(), b"text/plain");
}

#[test]
fn content_type_binary() {
    let r = exec1(&ContentTypeFn, &[0xFF, 0xFE, 0x00, 0x01, 0x80]).unwrap();
    assert_eq!(r.as_ref(), b"application/octet-stream");
}

#[test]
fn content_type_gzip() {
    let r = exec1(&ContentTypeFn, &[0x1F, 0x8B, 0x08, 0x00]).unwrap();
    assert_eq!(r.as_ref(), b"application/gzip");
}


// ── #97 ChunkHashFn ──

#[test]
fn chunk_hash_single_block() {
    use sha2::{Sha256, Digest};
    let data = b"small";
    let r = exec1(&ChunkHashFn, data).unwrap();
    assert_eq!(r.len(), 32);
    assert_eq!(r.as_ref(), Sha256::digest(data).as_slice());
}

#[test]
fn chunk_hash_multiple_blocks() {
    let data = vec![0u8; 100];
    let r = exec1_params(&ChunkHashFn, &data, params(&[("block_size", "30")])).unwrap();
    // 100 / 30 = 4 blocks (30+30+30+10)
    assert_eq!(r.len(), 4 * 32);
}

#[test]
fn chunk_hash_empty() {
    let r = exec1(&ChunkHashFn, b"").unwrap();
    assert!(r.is_empty());
}

#[test]
fn chunk_hash_deterministic() {
    let data = vec![0xAB; 500];
    let r1 = exec1_params(&ChunkHashFn, &data, params(&[("block_size", "100")])).unwrap();
    let r2 = exec1_params(&ChunkHashFn, &data, params(&[("block_size", "100")])).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn chunk_hash_block_size_zero_error() {
    let r = exec1_params(&ChunkHashFn, b"data", params(&[("block_size", "0")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}


// ── #98 DedupAnalyzeFn ──

#[test]
fn dedup_analyze_deterministic() {
    let data = vec![0xAB; 50000];
    let r1 = exec1(&DedupAnalyzeFn, &data).unwrap();
    let r2 = exec1(&DedupAnalyzeFn, &data).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn dedup_analyze_starts_at_zero() {
    let data = vec![0u8; 10000];
    let r = exec1(&DedupAnalyzeFn, &data).unwrap();
    let boundaries: Vec<u64> = serde_json::from_slice(&r).unwrap();
    assert_eq!(boundaries[0], 0);
}

#[test]
fn dedup_analyze_ends_at_length() {
    let data = vec![0u8; 10000];
    let r = exec1(&DedupAnalyzeFn, &data).unwrap();
    let boundaries: Vec<u64> = serde_json::from_slice(&r).unwrap();
    assert_eq!(*boundaries.last().unwrap(), 10000);
}

#[test]
fn dedup_analyze_empty() {
    let r = exec1(&DedupAnalyzeFn, b"").unwrap();
    let boundaries: Vec<u64> = serde_json::from_slice(&r).unwrap();
    assert_eq!(boundaries, vec![0]);
}

#[test]
fn dedup_analyze_small_input() {
    let data = vec![1u8; 100];
    let r = exec1(&DedupAnalyzeFn, &data).unwrap();
    let boundaries: Vec<u64> = serde_json::from_slice(&r).unwrap();
    assert_eq!(boundaries, vec![0, 100]);
}




// ── CAddrOfLeafFn (Spec Req 12.1) ──

#[test]
fn caddr_of_leaf_matches_blake3() {
    let data = b"test content";
    let r = exec1(&CAddrOfLeafFn, data).unwrap();
    assert_eq!(r.len(), 32);
    let expected = blake3::hash(data);
    assert_eq!(r.as_ref(), expected.as_bytes());
}

#[test]
fn caddr_of_leaf_matches_caddr_compute() {
    let data = b"identical behavior";
    let leaf = exec1(&CAddrOfLeafFn, data).unwrap();
    let compute = exec1(&CAddrComputeFn, data).unwrap();
    assert_eq!(leaf, compute);
}

#[test]
fn caddr_of_leaf_empty() {
    let r = exec1(&CAddrOfLeafFn, b"").unwrap();
    assert_eq!(r.len(), 32);
}

#[test]
fn caddr_of_leaf_wrong_input_count() {
    let r = CAddrOfLeafFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 1, .. })));
}

// ── EmbedMetadataFn (Spec Req 12.5) ──

#[test]
fn embed_metadata_basic() {
    let data = b"hello world";
    let r = exec1_params(&EmbedMetadataFn, data, params(&[("metadata", "{\"key\":\"value\"}")])).unwrap();
    // Header: 4 bytes length + metadata bytes + original
    let meta = "{\"key\":\"value\"}";
    assert_eq!(r.len(), 4 + meta.len() + data.len());
    // Check length prefix
    let len = u32::from_be_bytes([r[0], r[1], r[2], r[3]]) as usize;
    assert_eq!(len, meta.len());
    // Check metadata
    assert_eq!(&r[4..4+len], meta.as_bytes());
    // Check original data
    assert_eq!(&r[4+len..], data);
}

#[test]
fn embed_metadata_invalid_json() {
    let r = exec1_params(&EmbedMetadataFn, b"data", params(&[("metadata", "not json")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn embed_metadata_missing_param() {
    let r = exec1(&EmbedMetadataFn, b"data");
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn embed_metadata_wrong_input_count() {
    let r = EmbedMetadataFn.execute(vec![Bytes::from("a"), Bytes::from("b")],
        &params(&[("metadata", "{\"k\":1}")]));
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 1, .. })));
}

// ── StripMetadataFn (Spec Req 12.6) ──

#[test]
fn strip_metadata_basic() {
    // Manually create input with header
    let meta = b"{\"key\":\"value\"}";
    let data = b"original data";
    let len = meta.len() as u32;
    let mut input = Vec::new();
    input.extend_from_slice(&len.to_be_bytes());
    input.extend_from_slice(meta);
    input.extend_from_slice(data);
    let r = exec1(&StripMetadataFn, &input).unwrap();
    assert_eq!(r.as_ref(), data);
}

#[test]
fn strip_metadata_too_short() {
    let r = exec1(&StripMetadataFn, b"ab");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn strip_metadata_declared_length_exceeds_input() {
    let mut input = Vec::new();
    input.extend_from_slice(&100u32.to_be_bytes()); // claims 100 bytes of metadata
    input.extend_from_slice(b"short");
    let r = exec1(&StripMetadataFn, &input);
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

// ── Round-trip property: strip(embed(x, m)) == x (Spec Req 12.9) ──

#[test]
fn embed_strip_roundtrip() {
    let data = b"any binary content \x00\x01\x02";
    let metadata = "{\"author\":\"test\",\"version\":3}";
    let embedded = exec1_params(&EmbedMetadataFn, data, params(&[("metadata", metadata)])).unwrap();
    let stripped = exec1(&StripMetadataFn, &embedded).unwrap();
    assert_eq!(stripped.as_ref(), data);
}

#[test]
fn embed_strip_roundtrip_empty_data() {
    let embedded = exec1_params(&EmbedMetadataFn, b"", params(&[("metadata", "{\"a\":1}")])).unwrap();
    let stripped = exec1(&StripMetadataFn, &embedded).unwrap();
    assert!(stripped.is_empty());
}

// ── ContentHashFn (Spec Req 12.7) ──

#[test]
fn content_hash_sha256() {
    use sha2::{Sha256, Digest};
    let data = b"hash me";
    let r = exec1_params(&ContentHashFn, data, params(&[("algorithm", "sha256")])).unwrap();
    assert_eq!(r.as_ref(), Sha256::digest(data).as_slice());
}

#[test]
fn content_hash_sha512() {
    use sha2::{Sha512, Digest};
    let data = b"hash me";
    let r = exec1_params(&ContentHashFn, data, params(&[("algorithm", "sha512")])).unwrap();
    assert_eq!(r.as_ref(), Sha512::digest(data).as_slice());
}

#[test]
fn content_hash_blake3() {
    let data = b"hash me";
    let r = exec1_params(&ContentHashFn, data, params(&[("algorithm", "blake3")])).unwrap();
    assert_eq!(r.as_ref(), blake3::hash(data).as_bytes());
}

#[test]
fn content_hash_md5() {
    let data = b"hash me";
    let r = exec1_params(&ContentHashFn, data, params(&[("algorithm", "md5")])).unwrap();
    assert_eq!(r.len(), 16);
}

#[test]
fn content_hash_invalid_algorithm() {
    let r = exec1_params(&ContentHashFn, b"data", params(&[("algorithm", "crc32")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn content_hash_missing_param() {
    let r = exec1(&ContentHashFn, b"data");
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ── SplitBySizeFn (Spec Req 12.8) ──

#[test]
fn split_by_size_basic() {
    let data = b"abcdefghij"; // 10 bytes
    let r = exec1_params(&SplitBySizeFn, data, params(&[("chunk_size", "4")])).unwrap();
    // Expected: "abcd\0efgh\0ij"
    assert_eq!(r.as_ref(), b"abcd\0efgh\0ij");
}

#[test]
fn split_by_size_exact_multiple() {
    let data = b"aabbcc"; // 6 bytes
    let r = exec1_params(&SplitBySizeFn, data, params(&[("chunk_size", "2")])).unwrap();
    // Expected: "aa\0bb\0cc"
    assert_eq!(r.as_ref(), b"aa\0bb\0cc");
}

#[test]
fn split_by_size_larger_than_input() {
    let data = b"small";
    let r = exec1_params(&SplitBySizeFn, data, params(&[("chunk_size", "100")])).unwrap();
    // No splitting needed, just the data itself
    assert_eq!(r.as_ref(), b"small");
}

#[test]
fn split_by_size_empty_input() {
    let r = exec1_params(&SplitBySizeFn, b"", params(&[("chunk_size", "4")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn split_by_size_zero_chunk() {
    let r = exec1_params(&SplitBySizeFn, b"data", params(&[("chunk_size", "0")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn split_by_size_missing_param() {
    let r = exec1(&SplitBySizeFn, b"data");
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn split_by_size_wrong_input_count() {
    let r = SplitBySizeFn.execute(vec![Bytes::from("a"), Bytes::from("b")],
        &params(&[("chunk_size", "4")]));
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 1, .. })));
}
