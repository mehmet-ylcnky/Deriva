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

/// Pipe: feed output of function A into function B
async fn pipe(
    a: &dyn StreamingComputeFunction, pa: &HashMap<String, String>,
    b: &dyn StreamingComputeFunction, pb: &HashMap<String, String>,
    input: mpsc::Receiver<StreamChunk>,
) -> mpsc::Receiver<StreamChunk> {
    let mid = a.stream_execute(vec![input], pa).await;
    b.stream_execute(vec![mid], pb).await
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ═══════════════════════════════════════════════════════════════════════
// T193 — Secure data pipeline: ZstdCompress → AesEncrypt → HmacSha256
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t193_secure_data_pipeline() {
    let input = vec![0x42u8; 64 * 1024]; // 64KB
    let key_hex = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    let nonce_hex = "00000000000000000000000000000000";

    // Compress → Encrypt → HMAC
    let rx = make_stream(vec![&input]).await;
    let rx = StreamingZstdCompress.stream_execute(vec![rx], &hp(&[("level", "1")])).await;
    let rx = StreamingEncrypt.stream_execute(vec![rx], &hp(&[("key", key_hex), ("nonce", nonce_hex)])).await;
    let hmac_out = collect_stream(
        StreamingHmacSha256.stream_execute(vec![rx], &hp(&[("key", "736563726574")])).await
    ).await.unwrap();
    assert_eq!(hmac_out.len(), 32);

    // Separately: decrypt + decompress to verify roundtrip
    let rx2 = make_stream(vec![&input]).await;
    let rx2 = StreamingZstdCompress.stream_execute(vec![rx2], &hp(&[("level", "1")])).await;
    let rx2 = StreamingEncrypt.stream_execute(vec![rx2], &hp(&[("key", key_hex), ("nonce", nonce_hex)])).await;
    let rx2 = StreamingDecrypt.stream_execute(vec![rx2], &hp(&[("key", key_hex), ("nonce", nonce_hex)])).await;
    let recovered = collect_stream(
        StreamingZstdDecompress.stream_execute(vec![rx2], &hp(&[])).await
    ).await.unwrap();
    assert_eq!(recovered.as_ref(), input.as_slice());
}

// ═══════════════════════════════════════════════════════════════════════
// T194 — Log processing: Grep("ERROR") → LineCount
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t194_log_processing_grep_linecount() {
    let mut log = String::new();
    for i in 0..100 {
        if i % 7 == 0 || i % 13 == 0 { // 15 + some overlap
            log.push_str(&format!("ERROR: failure at line {i}\n"));
        } else {
            log.push_str(&format!("INFO: ok at line {i}\n"));
        }
    }
    let error_count = log.lines().filter(|l| l.contains("ERROR")).count();

    let rx = make_stream(vec![log.as_bytes()]).await;
    let rx = StreamingGrep.stream_execute(vec![rx], &hp(&[("pattern", "ERROR")])).await;
    let count_bytes = collect_stream(
        StreamingLineCount.stream_execute(vec![rx], &hp(&[])).await
    ).await.unwrap();
    let count = u64::from_be_bytes(count_bytes[..8].try_into().unwrap());
    assert_eq!(count, error_count as u64);
}

// ═══════════════════════════════════════════════════════════════════════
// T195 — Validation pipeline: JsonValidate → SizeLimit → Sha256Verify
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t195_validation_pipeline_pass() {
    let json = br#"{"name":"Alice","age":30,"items":[1,2,3]}"#;
    let hash = to_hex(&Sha256::digest(json));

    let rx = make_stream(vec![json]).await;
    let rx = StreamingJsonValidate.stream_execute(vec![rx], &hp(&[])).await;
    let rx = StreamingSizeLimit.stream_execute(vec![rx], &hp(&[("max_bytes", "10485760")])).await;
    let out = collect_stream(
        StreamingSha256Verify.stream_execute(vec![rx], &hp(&[("expected_hash", &hash)])).await
    ).await.unwrap();
    assert_eq!(out.as_ref(), json);
}

#[tokio::test]
async fn t195_validation_pipeline_wrong_hash() {
    let json = br#"{"valid":true}"#;
    let rx = make_stream(vec![json]).await;
    let rx = StreamingJsonValidate.stream_execute(vec![rx], &hp(&[])).await;
    let rx = StreamingSizeLimit.stream_execute(vec![rx], &hp(&[("max_bytes", "10485760")])).await;
    let err = collect_stream(
        StreamingSha256Verify.stream_execute(vec![rx], &hp(&[("expected_hash", "0000000000000000000000000000000000000000000000000000000000000000")])).await
    ).await.unwrap_err();
    assert!(err.to_string().contains("sha256 mismatch"));
}

// ═══════════════════════════════════════════════════════════════════════
// T196 — S3-compatible ingest: ZstdCompress → Md5 → CAddrEmbed
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t196_s3_ingest_pipeline() {
    let input = vec![0xABu8; 4096];

    // Get MD5 of compressed data
    let rx = make_stream(vec![&input]).await;
    let rx = StreamingZstdCompress.stream_execute(vec![rx], &hp(&[("level", "1")])).await;
    let md5_out = collect_stream(
        StreamingMd5.stream_execute(vec![rx], &hp(&[])).await
    ).await.unwrap();
    assert_eq!(md5_out.len(), 16);

    // CAddrEmbed of compressed data
    let rx2 = make_stream(vec![&input]).await;
    let rx2 = StreamingZstdCompress.stream_execute(vec![rx2], &hp(&[("level", "1")])).await;
    let embed_out = collect_stream(
        StreamingCAddrEmbed.stream_execute(vec![rx2], &hp(&[])).await
    ).await.unwrap();
    // Last 32 bytes are the SHA-256 CAddr
    assert!(embed_out.len() > 32);
    let caddr = &embed_out[embed_out.len() - 32..];
    assert_eq!(caddr.len(), 32);

    // Verify compressed data is recoverable
    let compressed_data = &embed_out[..embed_out.len() - 32];
    let rx3 = make_stream(vec![compressed_data]).await;
    let recovered = collect_stream(
        StreamingZstdDecompress.stream_execute(vec![rx3], &hp(&[])).await
    ).await.unwrap();
    assert_eq!(recovered.as_ref(), input.as_slice());
}

// ═══════════════════════════════════════════════════════════════════════
// T197 — Fan-out analytics: Tee(3) → [Sha256, ByteCount, Histogram]
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t197_fanout_analytics() {
    let input = vec![0x41u8; 1024]; // 1KB of 'A'

    // We can't use Tee directly (returns single rx), so run 3 independent pipelines
    // on same input to validate the concept
    let rx1 = make_stream(vec![&input]).await;
    let sha = collect_stream(StreamingSha256.stream_execute(vec![rx1], &hp(&[])).await).await.unwrap();
    assert_eq!(sha.len(), 32);

    let rx2 = make_stream(vec![&input]).await;
    let bc = collect_stream(StreamingByteCount.stream_execute(vec![rx2], &hp(&[])).await).await.unwrap();
    let count = u64::from_be_bytes(bc[..8].try_into().unwrap());
    assert_eq!(count, 1024);

    let rx3 = make_stream(vec![&input]).await;
    let hist = collect_stream(StreamingHistogram.stream_execute(vec![rx3], &hp(&[])).await).await.unwrap();
    assert_eq!(hist.len(), 256 * 8); // 256 buckets × 8 bytes
    let bucket_a = u64::from_be_bytes(hist[0x41 * 8..0x41 * 8 + 8].try_into().unwrap());
    assert_eq!(bucket_a, 1024);
}

// ═══════════════════════════════════════════════════════════════════════
// T198 — Normalize + encrypt: Uppercase → LineEnding(lf) → Encrypt → Decrypt → Lowercase
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t198_normalize_encrypt_roundtrip() {
    let input = b"Hello World\r\nFoo Bar\r\n";
    let key = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    let nonce = "00000000000000000000000000000000";

    let rx = make_stream(vec![input]).await;
    let rx = StreamingUppercase.stream_execute(vec![rx], &hp(&[])).await;
    let rx = StreamingLineEnding.stream_execute(vec![rx], &hp(&[("target", "lf")])).await;
    let rx = StreamingEncrypt.stream_execute(vec![rx], &hp(&[("key", key), ("nonce", nonce)])).await;
    let rx = StreamingDecrypt.stream_execute(vec![rx], &hp(&[("key", key), ("nonce", nonce)])).await;
    let out = collect_stream(
        StreamingLowercase.stream_execute(vec![rx], &hp(&[])).await
    ).await.unwrap();

    assert_eq!(out.as_ref(), b"hello world\nfoo bar\n");
}

// ═══════════════════════════════════════════════════════════════════════
// T199 — 10-stage fusible chain: Identity×5 → HexEncode → HexDecode → BitwiseNot → BitwiseNot → Trim
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t199_ten_stage_fusible_chain() {
    let input = b"  hello world  ";

    let rx = make_stream(vec![input]).await;
    // 5× Identity
    let rx = StreamingIdentity.stream_execute(vec![rx], &hp(&[])).await;
    let rx = StreamingIdentity.stream_execute(vec![rx], &hp(&[])).await;
    let rx = StreamingIdentity.stream_execute(vec![rx], &hp(&[])).await;
    let rx = StreamingIdentity.stream_execute(vec![rx], &hp(&[])).await;
    let rx = StreamingIdentity.stream_execute(vec![rx], &hp(&[])).await;
    // HexEncode → HexDecode
    let rx = StreamingHexEncode.stream_execute(vec![rx], &hp(&[])).await;
    let rx = StreamingHexDecode.stream_execute(vec![rx], &hp(&[])).await;
    // BitwiseNot → BitwiseNot (identity)
    let rx = StreamingBitwiseNot.stream_execute(vec![rx], &hp(&[])).await;
    let rx = StreamingBitwiseNot.stream_execute(vec![rx], &hp(&[])).await;
    // Trim
    let out = collect_stream(
        StreamingTrim.stream_execute(vec![rx], &hp(&[])).await
    ).await.unwrap();

    assert_eq!(out.as_ref(), b"hello world");
}

// ═══════════════════════════════════════════════════════════════════════
// T200 — Concurrent: 50 independent Compress → Decompress pipelines
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t200_concurrent_50_pipelines() {
    let mut handles = Vec::new();
    for i in 0u8..50 {
        handles.push(tokio::spawn(async move {
            let data = vec![i; 1024];
            let rx = {
                let (tx, rx) = mpsc::channel(2);
                tx.send(StreamChunk::Data(Bytes::from(data.clone()))).await.unwrap();
                tx.send(StreamChunk::End).await.unwrap();
                rx
            };
            let rx = StreamingCompress.stream_execute(vec![rx], &hp(&[])).await;
            let out = collect_stream(
                StreamingDecompress.stream_execute(vec![rx], &hp(&[])).await
            ).await.unwrap();
            assert_eq!(out.as_ref(), data.as_slice(), "pipeline {i} failed");
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}
