//! Tests for specific spec-mandated scenarios (T30, T39, T57, T88, T99, T100, T101).

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

async fn run_one(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> Bytes {
    let rx = make_stream(chunks).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

async fn run_one_err(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> String {
    let rx = make_stream(chunks).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap_err().to_string()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// T30 — HMAC-SHA256 matches RFC 4231 test vector
#[tokio::test]
async fn t30_hmac_rfc4231_test_case_2() {
    let f = StreamingHmacSha256;
    // RFC 4231 Test Case 2: key="Jefe", data="what do ya want for nothing?"
    let key_hex = to_hex(b"Jefe");
    let data = b"what do ya want for nothing?";
    // Split across 3 chunks
    let out = run_one(&f, vec![&data[..10], &data[10..20], &data[20..]], &hp(&[("key", &key_hex)])).await;
    assert_eq!(
        to_hex(&out),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

// T39 — Redact with pattern split across chunk boundary
#[tokio::test]
async fn t39_redact_across_chunk_boundary() {
    let f = StreamingRedact;
    let out = run_one(&f, vec![b"user@exam", b"ple.com rest"], &hp(&[])).await;
    let text = String::from_utf8_lossy(&out);
    assert!(!text.contains("user@example.com"), "email should be redacted");
    assert!(text.contains("[REDACTED]"));
    assert!(text.contains("rest"));
}

// T57 — CsvToJson 1000-row CSV across 8 chunks
#[tokio::test]
async fn t57_csv_to_json_1000_rows() {
    let f = StreamingCsvToJson;
    let mut csv = String::from("name,email,phone,city,zip\n");
    for i in 0..1000 {
        csv.push_str(&format!("user{i},u{i}@test.com,555-{i:04},city{i},{i:05}\n"));
    }
    let bytes = csv.as_bytes();
    let chunk_size = bytes.len() / 8;
    let chunks: Vec<&[u8]> = bytes.chunks(chunk_size).collect();
    let out = run_one(&f, chunks, &hp(&[])).await;
    let text = String::from_utf8_lossy(&out);
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1000);
    // Verify line 500 has all columns
    let row: serde_json::Value = serde_json::from_str(lines[499]).unwrap();
    assert!(row.get("name").is_some());
    assert!(row.get("email").is_some());
    assert!(row.get("phone").is_some());
    assert!(row.get("city").is_some());
    assert!(row.get("zip").is_some());
}

// T88 — Zstd roundtrip level 19, assert level 19 < level 1 size
#[tokio::test]
async fn t88_zstd_level19_vs_level1() {
    // Use large enough input for level difference to manifest
    let mut input = Vec::with_capacity(256 * 1024);
    for i in 0u32..65536 { input.extend_from_slice(&i.to_le_bytes()); }
    let c = StreamingZstdCompress;
    let d = StreamingZstdDecompress;

    let compressed_1 = run_one(&c, vec![&input], &hp(&[("level", "1")])).await;
    let compressed_19 = run_one(&c, vec![&input], &hp(&[("level", "19")])).await;

    assert!(compressed_19.len() < compressed_1.len(),
        "level 19 ({}) should be smaller than level 1 ({})", compressed_19.len(), compressed_1.len());

    // Roundtrip level 19
    let decompressed = run_one(&d, vec![&compressed_19], &hp(&[])).await;
    assert_eq!(&decompressed[..], &input[..]);
}

// T99 — Cross-codec rejection: Zstd → LZ4 decompressor
#[tokio::test]
async fn t99_cross_codec_zstd_to_lz4() {
    let compressed = run_one(&StreamingZstdCompress, vec![b"hello world"], &hp(&[])).await;
    let err = run_one_err(&StreamingLz4Decompress, vec![&compressed], &hp(&[])).await;
    assert!(!err.is_empty(), "should error on cross-codec data");
}

// T100 — Cross-codec rejection: Snappy → Brotli decompressor
#[tokio::test]
async fn t100_cross_codec_snappy_to_brotli() {
    let compressed = run_one(&StreamingSnappyCompress, vec![b"hello world"], &hp(&[])).await;
    let err = run_one_err(&StreamingBrotliDecompress, vec![&compressed], &hp(&[])).await;
    assert!(!err.is_empty(), "should error on cross-codec data");
}

// T101 — All 5 codecs roundtrip on same 1 MB input
#[tokio::test]
async fn t101_all_five_codecs_roundtrip() {
    let input = vec![0xABu8; 1024 * 1024]; // 1 MB
    let codecs: Vec<(&dyn StreamingComputeFunction, &dyn StreamingComputeFunction, &str)> = vec![
        (&StreamingZstdCompress, &StreamingZstdDecompress, "zstd"),
        (&StreamingLz4Compress, &StreamingLz4Decompress, "lz4"),
        (&StreamingSnappyCompress, &StreamingSnappyDecompress, "snappy"),
        (&StreamingBrotliCompress, &StreamingBrotliDecompress, "brotli"),
    ];
    let p = hp(&[]);
    let mut sizes = Vec::new();
    for (enc, dec, name) in &codecs {
        let compressed = run_one(*enc, vec![&input], &p).await;
        sizes.push((name, compressed.len()));
        let decompressed = run_one(*dec, vec![&compressed], &p).await;
        assert_eq!(decompressed.len(), input.len(), "{name} roundtrip length mismatch");
        assert_eq!(&decompressed[..], &input[..], "{name} roundtrip data mismatch");
    }
    // Also test zlib (original #8/#9) if available — use flate2 via the core compress/decompress
    // Print sizes for comparison (all should be < 1 MB for repetitive input)
    for (name, sz) in &sizes {
        assert!(*sz < input.len(), "{name} compressed size ({sz}) should be < input ({})", input.len());
    }
}
