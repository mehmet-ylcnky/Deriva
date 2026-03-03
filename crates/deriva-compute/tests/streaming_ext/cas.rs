use std::collections::HashMap;
use bytes::Bytes;
use sha2::{Sha256, Digest};
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use deriva_compute::builtins_streaming::*;
use deriva_compute::streaming::*;

fn hp(kv: &[(&str, &str)]) -> HashMap<String, String> {
    kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

async fn make_stream(chunks: Vec<&[u8]>) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(chunks.len() + 1);
    for c in chunks { tx.send(StreamChunk::Data(Bytes::copy_from_slice(c))).await.unwrap(); }
    tx.send(StreamChunk::End).await.unwrap();
    rx
}

async fn run_one(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> Bytes {
    let rx = make_stream(chunks).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

async fn run_one_err(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> String {
    let rx = make_stream(chunks).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap_err().to_string()
}

async fn collect_chunks(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> Vec<Bytes> {
    let rx = make_stream(chunks).await;
    let mut out_rx = f.stream_execute(vec![rx], params).await;
    let mut result = Vec::new();
    loop {
        match out_rx.recv().await {
            Some(StreamChunk::Data(b)) => result.push(b),
            Some(StreamChunk::End) | None => break,
            Some(StreamChunk::Error(e)) => panic!("unexpected error: {e}"),
        }
    }
    result
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ═══════════════════════════════════════════════════════════════════════
// #86 StreamingCAddrEmbed
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn caddr_embed_appends_hash() {
    let out = run_one(&StreamingCAddrEmbed, vec![b"hello"], &hp(&[])).await;
    // "hello" (5 bytes) + SHA-256 (32 bytes) = 37 bytes
    assert_eq!(out.len(), 5 + 32);
    assert_eq!(&out[..5], b"hello");
}

#[tokio::test]
async fn caddr_embed_correct_hash() {
    let out = run_one(&StreamingCAddrEmbed, vec![b"hello"], &hp(&[])).await;
    let expected = Sha256::digest(b"hello");
    assert_eq!(&out[5..], expected.as_slice());
}

#[tokio::test]
async fn caddr_embed_multi_chunk() {
    let out = run_one(&StreamingCAddrEmbed, vec![b"hel", b"lo"], &hp(&[])).await;
    let expected = Sha256::digest(b"hello");
    assert_eq!(&out[5..], expected.as_slice());
}

#[tokio::test]
async fn caddr_embed_empty() {
    let out = run_one(&StreamingCAddrEmbed, vec![], &hp(&[])).await;
    assert_eq!(out.len(), 32); // just the hash of empty
}

#[tokio::test]
async fn caddr_embed_preserves_data() {
    let chunks = collect_chunks(&StreamingCAddrEmbed, vec![b"abc", b"def"], &hp(&[])).await;
    // First chunks are data, last is hash
    assert!(chunks.len() >= 2);
    let last = &chunks[chunks.len() - 1];
    assert_eq!(last.len(), 32);
}

// ═══════════════════════════════════════════════════════════════════════
// #87 StreamingCAddrVerify
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn caddr_verify_correct() {
    let hash = to_hex(&Sha256::digest(b"hello"));
    let out = run_one(&StreamingCAddrVerify, vec![b"hello"], &hp(&[("expected_caddr", &hash)])).await;
    assert_eq!(out.as_ref(), b"hello");
}

#[tokio::test]
async fn caddr_verify_wrong() {
    let err = run_one_err(&StreamingCAddrVerify, vec![b"hello"], &hp(&[("expected_caddr", "0000000000000000000000000000000000000000000000000000000000000000")])).await;
    assert!(err.contains("caddr mismatch"));
}

#[tokio::test]
async fn caddr_verify_multi_chunk() {
    let hash = to_hex(&Sha256::digest(b"helloworld"));
    let out = run_one(&StreamingCAddrVerify, vec![b"hello", b"world"], &hp(&[("expected_caddr", &hash)])).await;
    assert_eq!(out.as_ref(), b"helloworld");
}

#[tokio::test]
async fn caddr_verify_missing_param() {
    let err = run_one_err(&StreamingCAddrVerify, vec![b"x"], &hp(&[])).await;
    assert!(err.contains("missing param"));
}

#[tokio::test]
async fn caddr_verify_preserves_data() {
    let data = b"binary\x00\xff";
    let hash = to_hex(&Sha256::digest(data));
    let out = run_one(&StreamingCAddrVerify, vec![data], &hp(&[("expected_caddr", &hash)])).await;
    assert_eq!(out.as_ref(), data);
}

// ═══════════════════════════════════════════════════════════════════════
// #88 StreamingDiff
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn diff_xor_basic() {
    let rx_a = make_stream(vec![b"\xff\x00\xaa"]).await;
    let rx_b = make_stream(vec![b"\x00\xff\x55"]).await;
    let out = collect_stream(StreamingDiff.stream_execute(vec![rx_a, rx_b], &hp(&[])).await).await.unwrap();
    assert_eq!(out.as_ref(), &[0xff, 0xff, 0xff]);
}

#[tokio::test]
async fn diff_same_input_zeros() {
    let rx_a = make_stream(vec![b"hello"]).await;
    let rx_b = make_stream(vec![b"hello"]).await;
    let out = collect_stream(StreamingDiff.stream_execute(vec![rx_a, rx_b], &hp(&[])).await).await.unwrap();
    assert!(out.iter().all(|&b| b == 0));
}

#[tokio::test]
async fn diff_unequal_length() {
    let rx_a = make_stream(vec![b"abc"]).await;
    let rx_b = make_stream(vec![b"a"]).await;
    let out = collect_stream(StreamingDiff.stream_execute(vec![rx_a, rx_b], &hp(&[])).await).await.unwrap();
    assert_eq!(out.len(), 3); // padded
    assert_eq!(out[0], 0); // a ^ a = 0
}

#[tokio::test]
async fn diff_multi_chunk() {
    let rx_a = make_stream(vec![b"\x01", b"\x02"]).await;
    let rx_b = make_stream(vec![b"\x03", b"\x04"]).await;
    let out = collect_stream(StreamingDiff.stream_execute(vec![rx_a, rx_b], &hp(&[])).await).await.unwrap();
    assert_eq!(out.as_ref(), &[0x02, 0x06]);
}

#[tokio::test]
async fn diff_requires_two_inputs() {
    let rx = make_stream(vec![b"x"]).await;
    let err = collect_stream(StreamingDiff.stream_execute(vec![rx], &hp(&[])).await).await.unwrap_err();
    assert!(err.to_string().contains("2 inputs"));
}

// ═══════════════════════════════════════════════════════════════════════
// #89 StreamingPatch
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn patch_roundtrip() {
    // diff = a XOR b, patch = a XOR diff = b
    let a = b"\x01\x02\x03";
    let b_data = b"\x04\x05\x06";
    let rx_a1 = make_stream(vec![a]).await;
    let rx_b1 = make_stream(vec![b_data]).await;
    let diff = collect_stream(StreamingDiff.stream_execute(vec![rx_a1, rx_b1], &hp(&[])).await).await.unwrap();

    let rx_a2 = make_stream(vec![a]).await;
    let rx_diff = make_stream(vec![diff.as_ref()]).await;
    let patched = collect_stream(StreamingPatch.stream_execute(vec![rx_a2, rx_diff], &hp(&[])).await).await.unwrap();
    assert_eq!(patched.as_ref(), b_data);
}

#[tokio::test]
async fn patch_identity() {
    // XOR with zeros = identity
    let rx_a = make_stream(vec![b"hello"]).await;
    let rx_z = make_stream(vec![&[0u8; 5]]).await;
    let out = collect_stream(StreamingPatch.stream_execute(vec![rx_a, rx_z], &hp(&[])).await).await.unwrap();
    assert_eq!(out.as_ref(), b"hello");
}

#[tokio::test]
async fn patch_self_xor_zeros() {
    let rx_a = make_stream(vec![b"abc"]).await;
    let rx_b = make_stream(vec![b"abc"]).await;
    let out = collect_stream(StreamingPatch.stream_execute(vec![rx_a, rx_b], &hp(&[])).await).await.unwrap();
    assert!(out.iter().all(|&b| b == 0));
}

#[tokio::test]
async fn patch_multi_chunk() {
    let rx_a = make_stream(vec![b"\x10", b"\x20"]).await;
    let rx_d = make_stream(vec![b"\x01", b"\x02"]).await;
    let out = collect_stream(StreamingPatch.stream_execute(vec![rx_a, rx_d], &hp(&[])).await).await.unwrap();
    assert_eq!(out.as_ref(), &[0x11, 0x22]);
}

#[tokio::test]
async fn patch_requires_two_inputs() {
    let rx = make_stream(vec![b"x"]).await;
    let err = collect_stream(StreamingPatch.stream_execute(vec![rx], &hp(&[])).await).await.unwrap_err();
    assert!(err.to_string().contains("2 inputs"));
}

// ═══════════════════════════════════════════════════════════════════════
// #90 StreamingMerkleTree
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn merkle_single_chunk() {
    let out = run_one(&StreamingMerkleTree, vec![b"leaf"], &hp(&[])).await;
    assert_eq!(out.len(), 32);
    // Single leaf: root = SHA-256("leaf") (no pairing needed)
    let expected = Sha256::digest(b"leaf");
    assert_eq!(out.as_ref(), expected.as_slice());
}

#[tokio::test]
async fn merkle_two_chunks() {
    let out = run_one(&StreamingMerkleTree, vec![b"a", b"b"], &hp(&[])).await;
    assert_eq!(out.len(), 32);
    let ha: [u8; 32] = Sha256::digest(b"a").into();
    let hb: [u8; 32] = Sha256::digest(b"b").into();
    let mut h = Sha256::new();
    h.update(ha);
    h.update(hb);
    assert_eq!(out.as_ref(), h.finalize().as_slice());
}

#[tokio::test]
async fn merkle_output_32_bytes() {
    let out = run_one(&StreamingMerkleTree, vec![b"a", b"b", b"c", b"d"], &hp(&[])).await;
    assert_eq!(out.len(), 32);
}

#[tokio::test]
async fn merkle_deterministic() {
    let out1 = run_one(&StreamingMerkleTree, vec![b"x", b"y"], &hp(&[])).await;
    let out2 = run_one(&StreamingMerkleTree, vec![b"x", b"y"], &hp(&[])).await;
    assert_eq!(out1, out2);
}

#[tokio::test]
async fn merkle_different_data_different_root() {
    let out1 = run_one(&StreamingMerkleTree, vec![b"a", b"b"], &hp(&[])).await;
    let out2 = run_one(&StreamingMerkleTree, vec![b"c", b"d"], &hp(&[])).await;
    assert_ne!(out1, out2);
}

// ═══════════════════════════════════════════════════════════════════════
// #91 StreamingContentType
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn content_type_png() {
    let out = run_one(&StreamingContentType, vec![b"\x89PNG\r\n\x1a\nrest"], &hp(&[])).await;
    assert!(out.starts_with(b"Content-Type: image/png\n"));
}

#[tokio::test]
async fn content_type_json() {
    let out = run_one(&StreamingContentType, vec![br#"{"key":"val"}"#], &hp(&[])).await;
    assert!(out.starts_with(b"Content-Type: application/json\n"));
}

#[tokio::test]
async fn content_type_gzip() {
    let out = run_one(&StreamingContentType, vec![b"\x1f\x8b\x08data"], &hp(&[])).await;
    assert!(out.starts_with(b"Content-Type: application/gzip\n"));
}

#[tokio::test]
async fn content_type_unknown() {
    let out = run_one(&StreamingContentType, vec![b"\x00\x01\x02"], &hp(&[])).await;
    assert!(out.starts_with(b"Content-Type: application/octet-stream\n"));
}

#[tokio::test]
async fn content_type_preserves_data() {
    let data = b"\x89PNGrest";
    let out = run_one(&StreamingContentType, vec![data], &hp(&[])).await;
    let header = b"Content-Type: image/png\n";
    assert_eq!(&out[header.len()..], data);
}

// ═══════════════════════════════════════════════════════════════════════
// #92 StreamingChunkHash
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn chunk_hash_appends_32_bytes() {
    let chunks = collect_chunks(&StreamingChunkHash, vec![b"hello"], &hp(&[])).await;
    assert_eq!(chunks[0].len(), 5 + 32);
}

#[tokio::test]
async fn chunk_hash_correct() {
    let chunks = collect_chunks(&StreamingChunkHash, vec![b"hello"], &hp(&[])).await;
    let expected = Sha256::digest(b"hello");
    assert_eq!(&chunks[0][5..], expected.as_slice());
}

#[tokio::test]
async fn chunk_hash_per_chunk() {
    let chunks = collect_chunks(&StreamingChunkHash, vec![b"a", b"b"], &hp(&[])).await;
    assert_eq!(chunks.len(), 2);
    assert_eq!(&chunks[0][1..], Sha256::digest(b"a").as_slice());
    assert_eq!(&chunks[1][1..], Sha256::digest(b"b").as_slice());
}

#[tokio::test]
async fn chunk_hash_preserves_data() {
    let chunks = collect_chunks(&StreamingChunkHash, vec![b"data"], &hp(&[])).await;
    assert_eq!(&chunks[0][..4], b"data");
}

#[tokio::test]
async fn chunk_hash_empty_chunk() {
    let chunks = collect_chunks(&StreamingChunkHash, vec![b""], &hp(&[])).await;
    assert_eq!(chunks[0].len(), 32); // 0 data + 32 hash
    assert_eq!(chunks[0].as_ref(), Sha256::digest(b"").as_slice());
}
