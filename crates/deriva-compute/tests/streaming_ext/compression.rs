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

// ═══════════════════════════════════════════════════════════════════════
// #51 StreamingZstdCompress
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn zstd_compress_produces_output() {
    let out = run_one(&StreamingZstdCompress, vec![b"hello world"], &hp(&[])).await;
    assert!(!out.is_empty());
    assert_ne!(out.as_ref(), b"hello world");
}

#[tokio::test]
async fn zstd_roundtrip() {
    let compressed = run_one(&StreamingZstdCompress, vec![b"hello world"], &hp(&[])).await;
    let decompressed = run_one(&StreamingZstdDecompress, vec![compressed.as_ref()], &hp(&[])).await;
    assert_eq!(decompressed.as_ref(), b"hello world");
}

#[tokio::test]
async fn zstd_compress_custom_level() {
    let out = run_one(&StreamingZstdCompress, vec![b"test data"], &hp(&[("level", "1")])).await;
    assert!(!out.is_empty());
}

#[tokio::test]
async fn zstd_compress_per_chunk() {
    let chunks = collect_chunks(&StreamingZstdCompress, vec![b"aaa", b"bbb"], &hp(&[])).await;
    assert_eq!(chunks.len(), 2);
}

#[tokio::test]
async fn zstd_compress_large() {
    let data = vec![0x42u8; 10000];
    let out = run_one(&StreamingZstdCompress, vec![&data], &hp(&[])).await;
    assert!(out.len() < data.len()); // compressible
}

// ═══════════════════════════════════════════════════════════════════════
// #52 StreamingZstdDecompress
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn zstd_decompress_basic() {
    let compressed = run_one(&StreamingZstdCompress, vec![b"test"], &hp(&[])).await;
    let out = run_one(&StreamingZstdDecompress, vec![compressed.as_ref()], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"test");
}

#[tokio::test]
async fn zstd_decompress_corrupt() {
    let err = run_one_err(&StreamingZstdDecompress, vec![b"not zstd"], &hp(&[])).await;
    assert!(!err.is_empty());
}

#[tokio::test]
async fn zstd_decompress_per_chunk() {
    let c1 = run_one(&StreamingZstdCompress, vec![b"aaa"], &hp(&[])).await;
    let c2 = run_one(&StreamingZstdCompress, vec![b"bbb"], &hp(&[])).await;
    let chunks = collect_chunks(&StreamingZstdDecompress, vec![c1.as_ref(), c2.as_ref()], &hp(&[])).await;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].as_ref(), b"aaa");
    assert_eq!(chunks[1].as_ref(), b"bbb");
}

#[tokio::test]
async fn zstd_roundtrip_binary() {
    let data: Vec<u8> = (0..=255).collect();
    let c = run_one(&StreamingZstdCompress, vec![&data], &hp(&[])).await;
    let d = run_one(&StreamingZstdDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), data.as_slice());
}

#[tokio::test]
async fn zstd_decompress_empty_frame() {
    let c = run_one(&StreamingZstdCompress, vec![b""], &hp(&[])).await;
    let d = run_one(&StreamingZstdDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"");
}

// ═══════════════════════════════════════════════════════════════════════
// #53 StreamingLz4Compress
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn lz4_compress_produces_output() {
    let out = run_one(&StreamingLz4Compress, vec![b"hello"], &hp(&[])).await;
    assert!(!out.is_empty());
}

#[tokio::test]
async fn lz4_roundtrip() {
    let c = run_one(&StreamingLz4Compress, vec![b"hello world"], &hp(&[])).await;
    let d = run_one(&StreamingLz4Decompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"hello world");
}

#[tokio::test]
async fn lz4_per_chunk() {
    let chunks = collect_chunks(&StreamingLz4Compress, vec![b"aa", b"bb"], &hp(&[])).await;
    assert_eq!(chunks.len(), 2);
}

#[tokio::test]
async fn lz4_compressible() {
    let data = vec![0x00u8; 10000];
    let out = run_one(&StreamingLz4Compress, vec![&data], &hp(&[])).await;
    assert!(out.len() < data.len());
}

#[tokio::test]
async fn lz4_roundtrip_binary() {
    let data: Vec<u8> = (0..=255).collect();
    let c = run_one(&StreamingLz4Compress, vec![&data], &hp(&[])).await;
    let d = run_one(&StreamingLz4Decompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), data.as_slice());
}

// ═══════════════════════════════════════════════════════════════════════
// #54 StreamingLz4Decompress
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn lz4_decompress_basic() {
    let c = run_one(&StreamingLz4Compress, vec![b"test"], &hp(&[])).await;
    let d = run_one(&StreamingLz4Decompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"test");
}

#[tokio::test]
async fn lz4_decompress_corrupt() {
    let err = run_one_err(&StreamingLz4Decompress, vec![b"bad"], &hp(&[])).await;
    assert!(!err.is_empty());
}

#[tokio::test]
async fn lz4_decompress_per_chunk() {
    let c1 = run_one(&StreamingLz4Compress, vec![b"xx"], &hp(&[])).await;
    let c2 = run_one(&StreamingLz4Compress, vec![b"yy"], &hp(&[])).await;
    let chunks = collect_chunks(&StreamingLz4Decompress, vec![c1.as_ref(), c2.as_ref()], &hp(&[])).await;
    assert_eq!(chunks[0].as_ref(), b"xx");
    assert_eq!(chunks[1].as_ref(), b"yy");
}

#[tokio::test]
async fn lz4_decompress_empty() {
    let c = run_one(&StreamingLz4Compress, vec![b""], &hp(&[])).await;
    let d = run_one(&StreamingLz4Decompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"");
}

#[tokio::test]
async fn lz4_decompress_large() {
    let data = vec![0xABu8; 50000];
    let c = run_one(&StreamingLz4Compress, vec![&data], &hp(&[])).await;
    let d = run_one(&StreamingLz4Decompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), data.as_slice());
}

// ═══════════════════════════════════════════════════════════════════════
// #55 StreamingSnappyCompress
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn snappy_compress_produces_output() {
    let out = run_one(&StreamingSnappyCompress, vec![b"hello"], &hp(&[])).await;
    assert!(!out.is_empty());
}

#[tokio::test]
async fn snappy_roundtrip() {
    let c = run_one(&StreamingSnappyCompress, vec![b"hello world"], &hp(&[])).await;
    let d = run_one(&StreamingSnappyDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"hello world");
}

#[tokio::test]
async fn snappy_per_chunk() {
    let chunks = collect_chunks(&StreamingSnappyCompress, vec![b"aa", b"bb"], &hp(&[])).await;
    assert_eq!(chunks.len(), 2);
}

#[tokio::test]
async fn snappy_compressible() {
    let data = vec![0x00u8; 10000];
    let out = run_one(&StreamingSnappyCompress, vec![&data], &hp(&[])).await;
    assert!(out.len() < data.len());
}

#[tokio::test]
async fn snappy_roundtrip_binary() {
    let data: Vec<u8> = (0..=255).collect();
    let c = run_one(&StreamingSnappyCompress, vec![&data], &hp(&[])).await;
    let d = run_one(&StreamingSnappyDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), data.as_slice());
}

// ═══════════════════════════════════════════════════════════════════════
// #56 StreamingSnappyDecompress
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn snappy_decompress_basic() {
    let c = run_one(&StreamingSnappyCompress, vec![b"test"], &hp(&[])).await;
    let d = run_one(&StreamingSnappyDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"test");
}

#[tokio::test]
async fn snappy_decompress_corrupt() {
    let err = run_one_err(&StreamingSnappyDecompress, vec![b"\xff\xff\xff"], &hp(&[])).await;
    assert!(!err.is_empty());
}

#[tokio::test]
async fn snappy_decompress_per_chunk() {
    let c1 = run_one(&StreamingSnappyCompress, vec![b"aa"], &hp(&[])).await;
    let c2 = run_one(&StreamingSnappyCompress, vec![b"bb"], &hp(&[])).await;
    let chunks = collect_chunks(&StreamingSnappyDecompress, vec![c1.as_ref(), c2.as_ref()], &hp(&[])).await;
    assert_eq!(chunks[0].as_ref(), b"aa");
    assert_eq!(chunks[1].as_ref(), b"bb");
}

#[tokio::test]
async fn snappy_decompress_empty() {
    let c = run_one(&StreamingSnappyCompress, vec![b""], &hp(&[])).await;
    let d = run_one(&StreamingSnappyDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"");
}

#[tokio::test]
async fn snappy_decompress_large() {
    let data = vec![0x42u8; 50000];
    let c = run_one(&StreamingSnappyCompress, vec![&data], &hp(&[])).await;
    let d = run_one(&StreamingSnappyDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), data.as_slice());
}

// ═══════════════════════════════════════════════════════════════════════
// #57 StreamingBrotliCompress
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn brotli_compress_produces_output() {
    let out = run_one(&StreamingBrotliCompress, vec![b"hello"], &hp(&[])).await;
    assert!(!out.is_empty());
}

#[tokio::test]
async fn brotli_roundtrip() {
    let c = run_one(&StreamingBrotliCompress, vec![b"hello world"], &hp(&[])).await;
    let d = run_one(&StreamingBrotliDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"hello world");
}

#[tokio::test]
async fn brotli_custom_quality() {
    let c = run_one(&StreamingBrotliCompress, vec![b"test"], &hp(&[("quality", "1")])).await;
    let d = run_one(&StreamingBrotliDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"test");
}

#[tokio::test]
async fn brotli_per_chunk() {
    let chunks = collect_chunks(&StreamingBrotliCompress, vec![b"aa", b"bb"], &hp(&[])).await;
    assert_eq!(chunks.len(), 2);
}

#[tokio::test]
async fn brotli_compressible() {
    let data = vec![0x00u8; 10000];
    let out = run_one(&StreamingBrotliCompress, vec![&data], &hp(&[])).await;
    assert!(out.len() < data.len());
}

// ═══════════════════════════════════════════════════════════════════════
// #58 StreamingBrotliDecompress
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn brotli_decompress_basic() {
    let c = run_one(&StreamingBrotliCompress, vec![b"test data"], &hp(&[])).await;
    let d = run_one(&StreamingBrotliDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"test data");
}

#[tokio::test]
async fn brotli_decompress_corrupt() {
    let err = run_one_err(&StreamingBrotliDecompress, vec![b"not brotli"], &hp(&[])).await;
    assert!(!err.is_empty());
}

#[tokio::test]
async fn brotli_decompress_per_chunk() {
    let c1 = run_one(&StreamingBrotliCompress, vec![b"xx"], &hp(&[])).await;
    let c2 = run_one(&StreamingBrotliCompress, vec![b"yy"], &hp(&[])).await;
    let chunks = collect_chunks(&StreamingBrotliDecompress, vec![c1.as_ref(), c2.as_ref()], &hp(&[])).await;
    assert_eq!(chunks[0].as_ref(), b"xx");
    assert_eq!(chunks[1].as_ref(), b"yy");
}

#[tokio::test]
async fn brotli_roundtrip_binary() {
    let data: Vec<u8> = (0..=255).collect();
    let c = run_one(&StreamingBrotliCompress, vec![&data], &hp(&[])).await;
    let d = run_one(&StreamingBrotliDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), data.as_slice());
}

#[tokio::test]
async fn brotli_decompress_empty() {
    let c = run_one(&StreamingBrotliCompress, vec![b""], &hp(&[])).await;
    let d = run_one(&StreamingBrotliDecompress, vec![c.as_ref()], &hp(&[])).await;
    assert_eq!(d.as_ref(), b"");
}

// ═══════════════════════════════════════════════════════════════════════
// #59 StreamingPad
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn pad_default_16() {
    let out = run_one(&StreamingPad, vec![b"hello"], &hp(&[])).await;
    assert_eq!(out.len(), 16);
    assert_eq!(&out[..5], b"hello");
    assert!(out[5..].iter().all(|&b| b == 0));
}

#[tokio::test]
async fn pad_already_aligned() {
    let data = [0x41u8; 16];
    let out = run_one(&StreamingPad, vec![&data], &hp(&[])).await;
    assert_eq!(out.len(), 16);
}

#[tokio::test]
async fn pad_custom_block_and_byte() {
    let out = run_one(&StreamingPad, vec![b"ab"], &hp(&[("block_size", "4"), ("padding_byte", "255")])).await;
    assert_eq!(out.len(), 4);
    assert_eq!(out.as_ref(), &[b'a', b'b', 0xff, 0xff]);
}

#[tokio::test]
async fn pad_per_chunk() {
    let chunks = collect_chunks(&StreamingPad, vec![b"a", b"bb"], &hp(&[("block_size", "4")])).await;
    assert_eq!(chunks[0].len(), 4);
    assert_eq!(chunks[1].len(), 4);
}

#[tokio::test]
async fn pad_empty_chunk() {
    let out = run_one(&StreamingPad, vec![b""], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// #60 StreamingTrim
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn trim_basic() {
    let out = run_one(&StreamingTrim, vec![b"  hello  "], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"hello");
}

#[tokio::test]
async fn trim_tabs_newlines() {
    let out = run_one(&StreamingTrim, vec![b"\t\nhello\r\n"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"hello");
}

#[tokio::test]
async fn trim_no_whitespace() {
    let out = run_one(&StreamingTrim, vec![b"hello"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"hello");
}

#[tokio::test]
async fn trim_all_whitespace() {
    let out = run_one(&StreamingTrim, vec![b"   "], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn trim_per_chunk() {
    let chunks = collect_chunks(&StreamingTrim, vec![b" a ", b" b "], &hp(&[])).await;
    assert_eq!(chunks[0].as_ref(), b"a");
    assert_eq!(chunks[1].as_ref(), b"b");
}
