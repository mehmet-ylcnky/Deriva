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


// ── #95 MerkleRootFn ──

#[test]
fn merkle_root_deterministic() {
    let data = vec![0u8; 200_000];
    let r1 = exec1_params(&MerkleRootFn, &data, params(&[("block_size", "65536")])).unwrap();
    let r2 = exec1_params(&MerkleRootFn, &data, params(&[("block_size", "65536")])).unwrap();
    assert_eq!(r1, r2);
    assert_eq!(r1.len(), 32);
}

#[test]
fn merkle_root_empty() {
    let r = exec1(&MerkleRootFn, b"").unwrap();
    assert_eq!(r.len(), 32);
}

#[test]
fn merkle_root_single_block() {
    use sha2::{Sha256, Digest};
    let data = b"small data";
    let r = exec1(&MerkleRootFn, data).unwrap();
    assert_eq!(r.as_ref(), Sha256::digest(data).as_slice());
}

#[test]
fn merkle_root_different_block_sizes_differ() {
    let data = vec![0xAB; 10000];
    let r1 = exec1_params(&MerkleRootFn, &data, params(&[("block_size", "1000")])).unwrap();
    let r2 = exec1_params(&MerkleRootFn, &data, params(&[("block_size", "5000")])).unwrap();
    assert_ne!(r1, r2);
}

#[test]
fn merkle_root_block_size_zero_error() {
    let r = exec1_params(&MerkleRootFn, b"data", params(&[("block_size", "0")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
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


