use std::collections::HashMap;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use deriva_compute::builtins_streaming::*;
use deriva_compute::streaming::*;

fn hp(kv: &[(&str, &str)]) -> HashMap<String, String> {
    kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

async fn make_stream(chunks: Vec<&[u8]>) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(8);
    for c in chunks { tx.send(StreamChunk::Data(Bytes::copy_from_slice(c))).await.unwrap(); }
    tx.send(StreamChunk::End).await.unwrap();
    rx
}

async fn run_one(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> Bytes {
    let rx = make_stream(chunks).await;
    let out = f.stream_execute(vec![rx], params).await;
    collect_stream(out).await.unwrap()
}

async fn run_one_err(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> String {
    let rx = make_stream(chunks).await;
    let out = f.stream_execute(vec![rx], params).await;
    collect_stream(out).await.unwrap_err().to_string()
}

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const NONCE: &str = "00112233445566778899aabbccddeeff";
const GCM_NONCE_PREFIX: &str = "0011223344556677";

// ═══════════════════════════════════════════════════════════════════════
// #21 StreamingEncrypt
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn encrypt_single_chunk() {
    let p = hp(&[("key", KEY), ("nonce", NONCE)]);
    let out = run_one(&StreamingEncrypt, vec![b"hello world"], &p).await;
    assert_eq!(out.len(), 11);
    assert_ne!(out.as_ref(), b"hello world");
}

#[tokio::test]
async fn encrypt_multi_chunk() {
    let p = hp(&[("key", KEY), ("nonce", NONCE)]);
    let out = run_one(&StreamingEncrypt, vec![b"hello", b" world"], &p).await;
    // Same as encrypting "hello world" in one shot
    let one_shot = run_one(&StreamingEncrypt, vec![b"hello world"], &p).await;
    assert_eq!(out, one_shot);
}

#[tokio::test]
async fn encrypt_empty() {
    let p = hp(&[("key", KEY), ("nonce", NONCE)]);
    let out = run_one(&StreamingEncrypt, vec![b""], &p).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn encrypt_bad_key() {
    let p = hp(&[("key", "short"), ("nonce", NONCE)]);
    let err = run_one_err(&StreamingEncrypt, vec![b"data"], &p).await;
    assert!(err.contains("key"), "error should mention key: {}", err);
}

#[tokio::test]
async fn encrypt_missing_param() {
    let p = hp(&[("key", KEY)]);
    let err = run_one_err(&StreamingEncrypt, vec![b"data"], &p).await;
    assert!(err.contains("nonce"), "error should mention nonce: {}", err);
}

// ═══════════════════════════════════════════════════════════════════════
// #22 StreamingDecrypt
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn decrypt_roundtrip() {
    let p = hp(&[("key", KEY), ("nonce", NONCE)]);
    let ct = run_one(&StreamingEncrypt, vec![b"secret message"], &p).await;
    let pt = run_one(&StreamingDecrypt, vec![ct.as_ref()], &p).await;
    assert_eq!(pt.as_ref(), b"secret message");
}

#[tokio::test]
async fn decrypt_multi_chunk_roundtrip() {
    let p = hp(&[("key", KEY), ("nonce", NONCE)]);
    let ct = run_one(&StreamingEncrypt, vec![b"aaa", b"bbb", b"ccc"], &p).await;
    let pt = run_one(&StreamingDecrypt, vec![&ct[..3], &ct[3..6], &ct[6..]], &p).await;
    assert_eq!(pt.as_ref(), b"aaabbbccc");
}

#[tokio::test]
async fn decrypt_empty() {
    let p = hp(&[("key", KEY), ("nonce", NONCE)]);
    let out = run_one(&StreamingDecrypt, vec![b""], &p).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn decrypt_bad_key_length() {
    let p = hp(&[("key", "abcd"), ("nonce", NONCE)]);
    let err = run_one_err(&StreamingDecrypt, vec![b"data"], &p).await;
    assert!(err.contains("key"));
}

#[tokio::test]
async fn decrypt_preserves_length() {
    let p = hp(&[("key", KEY), ("nonce", NONCE)]);
    let input = vec![0u8; 1000];
    let ct = run_one(&StreamingEncrypt, vec![&input], &p).await;
    assert_eq!(ct.len(), 1000);
}

// ═══════════════════════════════════════════════════════════════════════
// #23 StreamingAeadEncrypt
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn aead_encrypt_adds_tag() {
    let p = hp(&[("key", KEY), ("nonce_prefix", GCM_NONCE_PREFIX)]);
    let out = run_one(&StreamingAeadEncrypt, vec![b"hello"], &p).await;
    // 5 bytes plaintext + 16 bytes GCM tag
    assert_eq!(out.len(), 5 + 16);
}

#[tokio::test]
async fn aead_encrypt_multi_chunk() {
    let p = hp(&[("key", KEY), ("nonce_prefix", GCM_NONCE_PREFIX)]);
    let out = run_one(&StreamingAeadEncrypt, vec![b"aaa", b"bbb"], &p).await;
    // Each chunk gets its own tag: (3+16) + (3+16) = 38
    assert_eq!(out.len(), 38);
}

#[tokio::test]
async fn aead_encrypt_empty_chunk() {
    let p = hp(&[("key", KEY), ("nonce_prefix", GCM_NONCE_PREFIX)]);
    let out = run_one(&StreamingAeadEncrypt, vec![b""], &p).await;
    // Empty plaintext + 16 byte tag
    assert_eq!(out.len(), 16);
}

#[tokio::test]
async fn aead_encrypt_bad_key() {
    let p = hp(&[("key", "short"), ("nonce_prefix", GCM_NONCE_PREFIX)]);
    let err = run_one_err(&StreamingAeadEncrypt, vec![b"data"], &p).await;
    assert!(err.contains("key"));
}

#[tokio::test]
async fn aead_encrypt_bad_nonce_prefix() {
    let p = hp(&[("key", KEY), ("nonce_prefix", "ab")]);
    let err = run_one_err(&StreamingAeadEncrypt, vec![b"data"], &p).await;
    assert!(err.contains("nonce_prefix"));
}

// ═══════════════════════════════════════════════════════════════════════
// #24 StreamingAeadDecrypt
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn aead_decrypt_roundtrip() {
    let p = hp(&[("key", KEY), ("nonce_prefix", GCM_NONCE_PREFIX)]);
    let ct = run_one(&StreamingAeadEncrypt, vec![b"secret"], &p).await;
    let pt = run_one(&StreamingAeadDecrypt, vec![ct.as_ref()], &p).await;
    assert_eq!(pt.as_ref(), b"secret");
}

#[tokio::test]
async fn aead_decrypt_multi_chunk_roundtrip() {
    let p = hp(&[("key", KEY), ("nonce_prefix", GCM_NONCE_PREFIX)]);
    // Encrypt 2 chunks separately
    let ct = run_one(&StreamingAeadEncrypt, vec![b"aaa", b"bbb"], &p).await;
    // Each chunk is 3+16=19 bytes
    let pt = run_one(&StreamingAeadDecrypt, vec![&ct[..19], &ct[19..]], &p).await;
    assert_eq!(pt.as_ref(), b"aaabbb");
}

#[tokio::test]
async fn aead_decrypt_tampered() {
    let p = hp(&[("key", KEY), ("nonce_prefix", GCM_NONCE_PREFIX)]);
    let mut ct = run_one(&StreamingAeadEncrypt, vec![b"secret"], &p).await.to_vec();
    ct[0] ^= 0xFF; // tamper
    let err = run_one_err(&StreamingAeadDecrypt, vec![&ct], &p).await;
    assert!(err.contains("authentication failed"));
}

#[tokio::test]
async fn aead_decrypt_wrong_key() {
    let p_enc = hp(&[("key", KEY), ("nonce_prefix", GCM_NONCE_PREFIX)]);
    let ct = run_one(&StreamingAeadEncrypt, vec![b"data"], &p_enc).await;
    let other_key = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let p_dec = hp(&[("key", other_key), ("nonce_prefix", GCM_NONCE_PREFIX)]);
    let err = run_one_err(&StreamingAeadDecrypt, vec![ct.as_ref()], &p_dec).await;
    assert!(err.contains("authentication failed"));
}

#[tokio::test]
async fn aead_decrypt_missing_param() {
    let p = hp(&[("key", KEY)]);
    let err = run_one_err(&StreamingAeadDecrypt, vec![b"data"], &p).await;
    assert!(err.contains("nonce_prefix"));
}

// ═══════════════════════════════════════════════════════════════════════
// #25 StreamingHmacSha256
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn hmac_single_chunk() {
    let p = hp(&[("key", "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b")]);
    let out = run_one(&StreamingHmacSha256, vec![b"Hi There"], &p).await;
    assert_eq!(out.len(), 32);
}

#[tokio::test]
async fn hmac_multi_chunk_matches_single() {
    let p = hp(&[("key", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]);
    let one = run_one(&StreamingHmacSha256, vec![b"hello world"], &p).await;
    let multi = run_one(&StreamingHmacSha256, vec![b"hello", b" world"], &p).await;
    assert_eq!(one, multi);
}

#[tokio::test]
async fn hmac_empty_input() {
    let p = hp(&[("key", "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b")]);
    let out = run_one(&StreamingHmacSha256, vec![b""], &p).await;
    assert_eq!(out.len(), 32);
}

#[tokio::test]
async fn hmac_different_keys_differ() {
    let p1 = hp(&[("key", "aa".repeat(16).as_str())]);
    let p2 = hp(&[("key", "bb".repeat(16).as_str())]);
    let h1 = run_one(&StreamingHmacSha256, vec![b"data"], &p1).await;
    let h2 = run_one(&StreamingHmacSha256, vec![b"data"], &p2).await;
    assert_ne!(h1, h2);
}

#[tokio::test]
async fn hmac_missing_key() {
    let p = hp(&[]);
    let err = run_one_err(&StreamingHmacSha256, vec![b"data"], &p).await;
    assert!(err.contains("key"));
}

// ═══════════════════════════════════════════════════════════════════════
// #26 StreamingMd5
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn md5_known_value() {
    let out = run_one(&StreamingMd5, vec![b""], &hp(&[])).await;
    assert_eq!(out.len(), 16);
    assert_eq!(to_hex(&out), "d41d8cd98f00b204e9800998ecf8427e");
}

#[tokio::test]
async fn md5_hello() {
    let out = run_one(&StreamingMd5, vec![b"hello"], &hp(&[])).await;
    assert_eq!(to_hex(&out), "5d41402abc4b2a76b9719d911017c592");
}

#[tokio::test]
async fn md5_multi_chunk() {
    let one = run_one(&StreamingMd5, vec![b"hello world"], &hp(&[])).await;
    let multi = run_one(&StreamingMd5, vec![b"hello", b" world"], &hp(&[])).await;
    assert_eq!(one, multi);
}

#[tokio::test]
async fn md5_output_16_bytes() {
    let out = run_one(&StreamingMd5, vec![b"test data"], &hp(&[])).await;
    assert_eq!(out.len(), 16);
}

#[tokio::test]
async fn md5_different_inputs_differ() {
    let a = run_one(&StreamingMd5, vec![b"aaa"], &hp(&[])).await;
    let b = run_one(&StreamingMd5, vec![b"bbb"], &hp(&[])).await;
    assert_ne!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════
// #27 StreamingSha512
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sha512_output_64_bytes() {
    let out = run_one(&StreamingSha512, vec![b"test"], &hp(&[])).await;
    assert_eq!(out.len(), 64);
}

#[tokio::test]
async fn sha512_empty() {
    let out = run_one(&StreamingSha512, vec![b""], &hp(&[])).await;
    assert_eq!(out.len(), 64);
    // Known SHA-512 of empty string
    assert_eq!(&to_hex(&out)[..16], "cf83e1357eefb8bd");
}

#[tokio::test]
async fn sha512_multi_chunk() {
    let one = run_one(&StreamingSha512, vec![b"abcdef"], &hp(&[])).await;
    let multi = run_one(&StreamingSha512, vec![b"abc", b"def"], &hp(&[])).await;
    assert_eq!(one, multi);
}

#[tokio::test]
async fn sha512_differs_from_sha256() {
    let s256 = run_one(&StreamingSha256, vec![b"test"], &hp(&[])).await;
    let s512 = run_one(&StreamingSha512, vec![b"test"], &hp(&[])).await;
    assert_ne!(s256.len(), s512.len());
}

#[tokio::test]
async fn sha512_deterministic() {
    let a = run_one(&StreamingSha512, vec![b"same"], &hp(&[])).await;
    let b = run_one(&StreamingSha512, vec![b"same"], &hp(&[])).await;
    assert_eq!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════
// #28 StreamingBlake3
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn blake3_output_32_bytes() {
    let out = run_one(&StreamingBlake3, vec![b"test"], &hp(&[])).await;
    assert_eq!(out.len(), 32);
}

#[tokio::test]
async fn blake3_empty() {
    let out = run_one(&StreamingBlake3, vec![b""], &hp(&[])).await;
    assert_eq!(out.len(), 32);
}

#[tokio::test]
async fn blake3_multi_chunk() {
    let one = run_one(&StreamingBlake3, vec![b"hello world"], &hp(&[])).await;
    let multi = run_one(&StreamingBlake3, vec![b"hello", b" world"], &hp(&[])).await;
    assert_eq!(one, multi);
}

#[tokio::test]
async fn blake3_differs_from_sha256() {
    let sha = run_one(&StreamingSha256, vec![b"test"], &hp(&[])).await;
    let b3 = run_one(&StreamingBlake3, vec![b"test"], &hp(&[])).await;
    assert_ne!(sha, b3);
}

#[tokio::test]
async fn blake3_deterministic() {
    let a = run_one(&StreamingBlake3, vec![b"data"], &hp(&[])).await;
    let b = run_one(&StreamingBlake3, vec![b"data"], &hp(&[])).await;
    assert_eq!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════
// #29 StreamingRedact
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn redact_email() {
    let p = hp(&[("patterns", "default")]);
    let out = run_one(&StreamingRedact, vec![b"contact user@example.com please"], &p).await;
    assert!(String::from_utf8_lossy(&out).contains("[REDACTED]"));
    assert!(!String::from_utf8_lossy(&out).contains("user@example.com"));
}

#[tokio::test]
async fn redact_phone() {
    let p = hp(&[("patterns", "default")]);
    let out = run_one(&StreamingRedact, vec![b"call 555-123-4567 now"], &p).await;
    assert!(String::from_utf8_lossy(&out).contains("[REDACTED]"));
}

#[tokio::test]
async fn redact_ssn() {
    let p = hp(&[("patterns", "default")]);
    let out = run_one(&StreamingRedact, vec![b"ssn: 123-45-6789"], &p).await;
    assert!(String::from_utf8_lossy(&out).contains("[REDACTED]"));
    assert!(!String::from_utf8_lossy(&out).contains("123-45-6789"));
}

#[tokio::test]
async fn redact_custom_pattern() {
    let p = hp(&[("patterns", r"\d+")]);
    let out = run_one(&StreamingRedact, vec![b"order 42 item 7"], &p).await;
    let s = String::from_utf8_lossy(&out);
    assert!(!s.contains("42"));
    assert!(!s.contains("7"));
}

#[tokio::test]
async fn redact_no_match_passthrough() {
    let p = hp(&[("patterns", "default")]);
    let out = run_one(&StreamingRedact, vec![b"nothing to redact here"], &p).await;
    assert_eq!(out.as_ref(), b"nothing to redact here");
}
