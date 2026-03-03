//! §6 Edge Case tests — EC3,6,10,17,18,19,21,34,35,36,38,51

use std::collections::HashMap;
use bytes::Bytes;
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

async fn run_one_err(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> String {
    let rx = make_stream(chunks).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap_err().to_string()
}

// EC3 — AES key contains non-hex characters
#[tokio::test]
async fn ec3_key_non_hex_chars() {
    let err = run_one_err(&StreamingEncrypt, vec![b"data"],
        &hp(&[("key", "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"), ("nonce", "00000000000000000000000000000000")])).await;
    assert!(err.contains("invalid hex"), "got: {err}");
}

// EC6 — Redact with invalid regex pattern
#[tokio::test]
async fn ec6_redact_invalid_regex() {
    let err = run_one_err(&StreamingRedact, vec![b"data"],
        &hp(&[("patterns", "[invalid")])).await;
    assert!(err.contains("invalid regex"), "got: {err}");
}

// EC10 — UTF-8 stream ends mid-sequence
#[tokio::test]
async fn ec10_utf8_ends_mid_sequence() {
    // Send 0xC3 (start of 2-byte UTF-8) then End — no continuation byte
    let err = run_one_err(&StreamingUtf8Validate, vec![&[0xC3]], &hp(&[])).await;
    assert!(err.contains("incomplete sequence"), "got: {err}");
}

// EC17 — Zstd level out of range
#[tokio::test]
async fn ec17_zstd_level_out_of_range() {
    let err = run_one_err(&StreamingZstdCompress, vec![b"data"],
        &hp(&[("level", "99")])).await;
    assert!(err.contains("out of range"), "got: {err}");
}

// EC18 — Brotli quality out of range
#[tokio::test]
async fn ec18_brotli_quality_out_of_range() {
    let err = run_one_err(&StreamingBrotliCompress, vec![b"data"],
        &hp(&[("quality", "15")])).await;
    assert!(err.contains("out of range"), "got: {err}");
}

// EC19 — Pad with block_size=0
#[tokio::test]
async fn ec19_pad_block_size_zero() {
    let err = run_one_err(&StreamingPad, vec![b"data"],
        &hp(&[("block_size", "0")])).await;
    assert!(err.contains("block_size must be > 0"), "got: {err}");
}

// EC21 — RateLimit with bytes_per_sec=0
#[tokio::test]
async fn ec21_rate_limit_zero_bps() {
    let err = run_one_err(&StreamingRateLimit, vec![b"data"],
        &hp(&[("bytes_per_sec", "0")])).await;
    assert!(err.contains("bytes_per_sec must be > 0"), "got: {err}");
}

// EC34 — ChecksumVerify with invalid hex in expected_crc32
#[tokio::test]
async fn ec34_checksum_verify_invalid_hex() {
    let err = run_one_err(&StreamingChecksumVerify, vec![b"data"],
        &hp(&[("expected_crc32", "ZZZZZZZZ")])).await;
    assert!(err.contains("invalid hex"), "got: {err}");
}

// EC35 — Sha256Verify with wrong-length expected_hash
#[tokio::test]
async fn ec35_sha256_verify_wrong_length() {
    let err = run_one_err(&StreamingSha256Verify, vec![b"data"],
        &hp(&[("expected_hash", "abcd1234")])).await;
    assert!(err.contains("expected 64 hex chars"), "got: {err}");
}

// EC36 — Grep with invalid regex
#[tokio::test]
async fn ec36_grep_invalid_regex() {
    let err = run_one_err(&StreamingGrep, vec![b"data\n"],
        &hp(&[("pattern", "[invalid")])).await;
    assert!(err.contains("invalid regex"), "got: {err}");
}

// EC38 — Replace with empty find pattern
#[tokio::test]
async fn ec38_replace_empty_find() {
    let err = run_one_err(&StreamingReplace, vec![b"data"],
        &hp(&[("find", ""), ("replace", "x")])).await;
    assert!(err.contains("find pattern must not be empty"), "got: {err}");
}

// EC51 — RollingHash with window_size=0
#[tokio::test]
async fn ec51_rolling_hash_window_zero() {
    let err = run_one_err(&StreamingRollingHash, vec![b"data"],
        &hp(&[("window_size", "0")])).await;
    assert!(err.contains("window_size must be > 0"), "got: {err}");
}
