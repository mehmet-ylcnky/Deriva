//! Tests for §2.7 Streaming Materialization (§5.1–5.8 + new builtins)

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::mpsc;

use deriva_core::address::CAddr;
use deriva_core::streaming::StreamChunk;
use deriva_core::DerivaError;
use deriva_compute::builtins_streaming::*;
use deriva_compute::pipeline::{PipelineConfig, StreamPipeline};
use deriva_compute::streaming::*;

fn test_addr(label: &str) -> CAddr {
    CAddr::from_bytes(label.as_bytes())
}

/// Helper: send chunks and End to a channel, return receiver.
async fn make_stream(chunks: Vec<&[u8]>) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(8);
    for c in chunks {
        tx.send(StreamChunk::Data(Bytes::copy_from_slice(c))).await.unwrap();
    }
    tx.send(StreamChunk::End).await.unwrap();
    rx
}

/// Helper: execute a single-input streaming function.
async fn run_one(
    f: &dyn StreamingComputeFunction,
    chunks: Vec<&[u8]>,
    params: &HashMap<String, String>,
) -> Result<Bytes, DerivaError> {
    let rx = make_stream(chunks).await;
    let out = f.stream_execute(vec![rx], params).await;
    collect_stream(out).await
}

// =========================================================================
// §5.1 Unit Tests — StreamChunk
// =========================================================================

#[test]
fn test_stream_chunk_data() {
    let chunk = StreamChunk::Data(Bytes::from("hello"));
    assert!(chunk.is_data());
    assert!(!chunk.is_end());
    assert!(!chunk.is_error());
    assert_eq!(chunk.data_len(), 5);
}

#[test]
fn test_stream_chunk_end() {
    let chunk = StreamChunk::End;
    assert!(!chunk.is_data());
    assert!(chunk.is_end());
    assert_eq!(chunk.data_len(), 0);
}

#[test]
fn test_stream_chunk_into_data() {
    let chunk = StreamChunk::Data(Bytes::from("hello"));
    assert_eq!(chunk.into_data(), Some(Bytes::from("hello")));
    assert_eq!(StreamChunk::End.into_data(), None);
}

// =========================================================================
// §5.2 Unit Tests — batch_to_stream
// =========================================================================

#[tokio::test]
async fn test_batch_to_stream_small() {
    let data = Bytes::from("hello world");
    let mut rx = batch_to_stream(data.clone(), 64 * 1024);
    match rx.recv().await.unwrap() {
        StreamChunk::Data(chunk) => assert_eq!(chunk, data),
        other => panic!("expected Data, got {:?}", other),
    }
    assert!(rx.recv().await.unwrap().is_end());
}

#[tokio::test]
async fn test_batch_to_stream_chunked() {
    let data = Bytes::from(vec![0u8; 200]);
    let mut rx = batch_to_stream(data, 64);
    let mut total = 0;
    let mut chunk_count = 0;
    loop {
        match rx.recv().await.unwrap() {
            StreamChunk::Data(chunk) => {
                assert!(chunk.len() <= 64);
                total += chunk.len();
                chunk_count += 1;
            }
            StreamChunk::End => break,
            StreamChunk::Error(e) => panic!("error: {:?}", e),
        }
    }
    assert_eq!(total, 200);
    assert_eq!(chunk_count, 4);
}

#[tokio::test]
async fn test_batch_to_stream_empty() {
    let mut rx = batch_to_stream(Bytes::new(), 64);
    assert!(rx.recv().await.unwrap().is_end());
}

// =========================================================================
// §5.3 Unit Tests — collect_stream
// =========================================================================

#[tokio::test]
async fn test_collect_stream_basic() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("hello "))).await.unwrap();
    tx.send(StreamChunk::Data(Bytes::from("world"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    drop(tx);
    assert_eq!(collect_stream(rx).await.unwrap(), Bytes::from("hello world"));
}

#[tokio::test]
async fn test_collect_stream_single_chunk() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("hello"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    drop(tx);
    assert_eq!(collect_stream(rx).await.unwrap(), Bytes::from("hello"));
}

#[tokio::test]
async fn test_collect_stream_error() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("partial"))).await.unwrap();
    tx.send(StreamChunk::Error(DerivaError::ComputeFailed("boom".into()))).await.unwrap();
    drop(tx);
    assert!(collect_stream(rx).await.is_err());
}

#[tokio::test]
async fn test_collect_stream_channel_closed() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("partial"))).await.unwrap();
    drop(tx);
    assert!(collect_stream(rx).await.is_err());
}

// =========================================================================
// §5.4 Unit Tests — StreamingConcat
// =========================================================================

#[tokio::test]
async fn test_streaming_concat_two_inputs() {
    let rx_a = make_stream(vec![b"hello "]).await;
    let rx_b = make_stream(vec![b"world"]).await;
    let out = StreamingConcat.stream_execute(vec![rx_a, rx_b], &HashMap::new()).await;
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("hello world"));
}

#[tokio::test]
async fn test_streaming_concat_empty_input() {
    let rx_a = make_stream(vec![]).await;
    let out = StreamingConcat.stream_execute(vec![rx_a], &HashMap::new()).await;
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::new());
}

#[tokio::test]
async fn test_streaming_concat_error_propagation() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("ok"))).await.unwrap();
    tx.send(StreamChunk::Error(DerivaError::ComputeFailed("fail".into()))).await.unwrap();
    let out = StreamingConcat.stream_execute(vec![rx], &HashMap::new()).await;
    assert!(collect_stream(out).await.is_err());
}

// =========================================================================
// §5.5 Unit Tests — StreamingSha256
// =========================================================================

#[tokio::test]
async fn test_streaming_sha256() {
    let result = run_one(&StreamingSha256, vec![b"hello"], &HashMap::new()).await.unwrap();
    assert_eq!(result.len(), 32);
    use sha2::{Sha256, Digest};
    assert_eq!(&result[..], &Sha256::digest(b"hello")[..]);
}

#[tokio::test]
async fn test_streaming_sha256_chunked_input() {
    let result = run_one(&StreamingSha256, vec![b"hel", b"lo"], &HashMap::new()).await.unwrap();
    use sha2::{Sha256, Digest};
    assert_eq!(&result[..], &Sha256::digest(b"hello")[..]);
}

// =========================================================================
// §5.6 Unit Tests — StreamingUppercase
// =========================================================================

#[tokio::test]
async fn test_streaming_uppercase() {
    let result = run_one(&StreamingUppercase, vec![b"hello", b" world"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("HELLO WORLD"));
}

// =========================================================================
// §5.7 Unit Tests — tee_stream
// =========================================================================

#[tokio::test]
async fn test_tee_stream() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("hello"))).await.unwrap();
    tx.send(StreamChunk::Data(Bytes::from(" world"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    drop(tx);

    let (rx_a, rx_b) = tee_stream(rx, 8);
    let (a, b) = tokio::join!(collect_stream(rx_a), collect_stream(rx_b));
    assert_eq!(a.unwrap(), Bytes::from("hello world"));
    assert_eq!(b.unwrap(), Bytes::from("hello world"));
}

#[tokio::test]
async fn test_tee_stream_one_receiver_dropped() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("data"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    drop(tx);

    let (rx_a, rx_b) = tee_stream(rx, 8);
    drop(rx_b);
    assert_eq!(collect_stream(rx_a).await.unwrap(), Bytes::from("data"));
}

// =========================================================================
// §5.8 Integration Tests — Pipeline
// =========================================================================

#[tokio::test]
async fn test_pipeline_streaming_concat_then_uppercase() {
    let mut pipeline = StreamPipeline::new(PipelineConfig {
        chunk_size: 5,
        ..Default::default()
    });
    let idx_a = pipeline.add_source(test_addr("a"), Bytes::from("hello "));
    let idx_b = pipeline.add_source(test_addr("b"), Bytes::from("world"));
    let idx_concat = pipeline.add_streaming_stage(
        test_addr("concat"), Arc::new(StreamingConcat), HashMap::new(), vec![idx_a, idx_b],
    );
    let _idx_upper = pipeline.add_streaming_stage(
        test_addr("upper"), Arc::new(StreamingUppercase), HashMap::new(), vec![idx_concat],
    );
    let out = pipeline.execute().await.unwrap();
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("HELLO WORLD"));
}

#[tokio::test]
async fn test_pipeline_large_data_backpressure() {
    let data = Bytes::from(vec![b'x'; 1_000_000]);
    let mut pipeline = StreamPipeline::new(PipelineConfig {
        chunk_size: 1024,
        channel_capacity: 2,
        ..Default::default()
    });
    let idx_a = pipeline.add_source(test_addr("a"), data.clone());
    let _idx_id = pipeline.add_streaming_stage(
        test_addr("id"), Arc::new(StreamingIdentity), HashMap::new(), vec![idx_a],
    );
    let out = pipeline.execute().await.unwrap();
    let result = collect_stream(out).await.unwrap();
    assert_eq!(result.len(), 1_000_000);
    assert_eq!(result, data);
}

// =========================================================================
// New builtins — Category 1: Chunk Transforms
// =========================================================================

#[tokio::test]
async fn test_streaming_identity() {
    let result = run_one(&StreamingIdentity, vec![b"abc", b"def"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("abcdef"));
}

#[tokio::test]
async fn test_streaming_lowercase() {
    let result = run_one(&StreamingLowercase, vec![b"HELLO", b" WORLD"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("hello world"));
}

#[tokio::test]
async fn test_streaming_reverse() {
    let result = run_one(&StreamingReverse, vec![b"abc"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("cba"));
}

#[tokio::test]
async fn test_streaming_reverse_multi_chunk() {
    // Each chunk reversed independently
    let rx = make_stream(vec![b"ab", b"cd"]).await;
    let out = StreamingReverse.stream_execute(vec![rx], &HashMap::new()).await;
    let result = collect_stream(out).await.unwrap();
    assert_eq!(result, Bytes::from("badc"));
}

#[tokio::test]
async fn test_streaming_base64_encode() {
    let result = run_one(&StreamingBase64Encode, vec![b"hello"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("aGVsbG8="));
}

#[tokio::test]
async fn test_streaming_base64_decode() {
    let result = run_one(&StreamingBase64Decode, vec![b"aGVsbG8="], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("hello"));
}

#[tokio::test]
async fn test_streaming_base64_roundtrip() {
    let encoded = run_one(&StreamingBase64Encode, vec![b"test data"], &HashMap::new()).await.unwrap();
    let rx = make_stream(vec![&encoded]).await;
    let out = StreamingBase64Decode.stream_execute(vec![rx], &HashMap::new()).await;
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("test data"));
}

#[tokio::test]
async fn test_streaming_base64_decode_invalid() {
    let result = run_one(&StreamingBase64Decode, vec![b"!!!invalid!!!"], &HashMap::new()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_streaming_xor() {
    let mut params = HashMap::new();
    params.insert("key".into(), "255".into()); // 0xFF
    let result = run_one(&StreamingXor, vec![b"\x00\x01\x02"], &params).await.unwrap();
    assert_eq!(&result[..], &[0xFF, 0xFE, 0xFD]);
}

#[tokio::test]
async fn test_streaming_xor_roundtrip() {
    let mut params = HashMap::new();
    params.insert("key".into(), "42".into());
    let encrypted = run_one(&StreamingXor, vec![b"hello"], &params).await.unwrap();
    let rx = make_stream(vec![&encrypted]).await;
    let out = StreamingXor.stream_execute(vec![rx], &params).await;
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("hello"));
}

#[tokio::test]
async fn test_streaming_compress_decompress() {
    let original = Bytes::from("hello world hello world hello world");
    let compressed = run_one(&StreamingCompress, vec![&original], &HashMap::new()).await.unwrap();
    assert_ne!(compressed, original); // should be different
    let rx = make_stream(vec![&compressed]).await;
    let out = StreamingDecompress.stream_execute(vec![rx], &HashMap::new()).await;
    assert_eq!(collect_stream(out).await.unwrap(), original);
}

// =========================================================================
// New builtins — Category 2: Accumulators
// =========================================================================

#[tokio::test]
async fn test_streaming_byte_count() {
    let result = run_one(&StreamingByteCount, vec![b"hello", b" world"], &HashMap::new()).await.unwrap();
    assert_eq!(result.len(), 8);
    let count = u64::from_be_bytes(result[..8].try_into().unwrap());
    assert_eq!(count, 11);
}

#[tokio::test]
async fn test_streaming_byte_count_empty() {
    let result = run_one(&StreamingByteCount, vec![], &HashMap::new()).await.unwrap();
    let count = u64::from_be_bytes(result[..8].try_into().unwrap());
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_streaming_checksum() {
    let result = run_one(&StreamingChecksum, vec![b"hello"], &HashMap::new()).await.unwrap();
    assert_eq!(result.len(), 4);
    let mut h = crc32fast::Hasher::new();
    h.update(b"hello");
    let expected = h.finalize().to_be_bytes();
    assert_eq!(&result[..], &expected);
}

#[tokio::test]
async fn test_streaming_checksum_chunked() {
    // Same result regardless of chunking
    let r1 = run_one(&StreamingChecksum, vec![b"hello world"], &HashMap::new()).await.unwrap();
    let r2 = run_one(&StreamingChecksum, vec![b"hello", b" world"], &HashMap::new()).await.unwrap();
    assert_eq!(r1, r2);
}

// =========================================================================
// New builtins — Category 3: Combiners
// =========================================================================

#[tokio::test]
async fn test_streaming_interleave() {
    let rx_a = make_stream(vec![b"a1", b"a2", b"a3"]).await;
    let rx_b = make_stream(vec![b"b1", b"b2"]).await;
    let out = StreamingInterleave.stream_execute(vec![rx_a, rx_b], &HashMap::new()).await;
    let result = collect_stream(out).await.unwrap();
    // Round-robin: a1, b1, a2, b2, a3
    assert_eq!(result, Bytes::from("a1b1a2b2a3"));
}

#[tokio::test]
async fn test_streaming_interleave_single() {
    let rx = make_stream(vec![b"only"]).await;
    let out = StreamingInterleave.stream_execute(vec![rx], &HashMap::new()).await;
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("only"));
}

#[tokio::test]
async fn test_streaming_zip_concat() {
    let rx_a = make_stream(vec![b"aa", b"bb"]).await;
    let rx_b = make_stream(vec![b"11", b"22"]).await;
    let out = StreamingZipConcat.stream_execute(vec![rx_a, rx_b], &HashMap::new()).await;
    let result = collect_stream(out).await.unwrap();
    // Pairs: aa+11, bb+22
    assert_eq!(result, Bytes::from("aa11bb22"));
}

#[tokio::test]
async fn test_streaming_zip_concat_uneven() {
    let rx_a = make_stream(vec![b"aa", b"bb", b"cc"]).await;
    let rx_b = make_stream(vec![b"11"]).await;
    let out = StreamingZipConcat.stream_execute(vec![rx_a, rx_b], &HashMap::new()).await;
    let result = collect_stream(out).await.unwrap();
    // Pair: aa+11, then drain: bb, cc
    assert_eq!(result, Bytes::from("aa11bbcc"));
}

// =========================================================================
// New builtins — Category 4: Utilities
// =========================================================================

#[tokio::test]
async fn test_streaming_chunk_resizer() {
    let mut params = HashMap::new();
    params.insert("target_size".into(), "4".into());
    // Input: 10 bytes in one chunk → should be re-chunked to 4, 4, 2
    let rx = make_stream(vec![b"0123456789"]).await;
    let mut out = StreamingChunkResizer.stream_execute(vec![rx], &params).await;
    let mut chunks = vec![];
    loop {
        match out.recv().await.unwrap() {
            StreamChunk::Data(c) => chunks.push(c),
            StreamChunk::End => break,
            StreamChunk::Error(e) => panic!("{:?}", e),
        }
    }
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0], Bytes::from("0123"));
    assert_eq!(chunks[1], Bytes::from("4567"));
    assert_eq!(chunks[2], Bytes::from("89"));
}

#[tokio::test]
async fn test_streaming_take() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "5".into());
    let result = run_one(&StreamingTake, vec![b"hello world"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("hello"));
}

#[tokio::test]
async fn test_streaming_take_multi_chunk() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "7".into());
    let result = run_one(&StreamingTake, vec![b"hello", b" world"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("hello w"));
}

#[tokio::test]
async fn test_streaming_take_more_than_available() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "100".into());
    let result = run_one(&StreamingTake, vec![b"short"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("short"));
}

#[tokio::test]
async fn test_streaming_skip() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "6".into());
    let result = run_one(&StreamingSkip, vec![b"hello world"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("world"));
}

#[tokio::test]
async fn test_streaming_skip_multi_chunk() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "3".into());
    let result = run_one(&StreamingSkip, vec![b"he", b"llo world"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("lo world"));
}

#[tokio::test]
async fn test_streaming_skip_more_than_available() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "100".into());
    let result = run_one(&StreamingSkip, vec![b"short"], &params).await.unwrap();
    assert_eq!(result, Bytes::new());
}

#[tokio::test]
async fn test_streaming_repeat() {
    let mut params = HashMap::new();
    params.insert("count".into(), "3".into());
    let result = run_one(&StreamingRepeat, vec![b"ab"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("ababab"));
}

#[tokio::test]
async fn test_streaming_repeat_zero() {
    let mut params = HashMap::new();
    params.insert("count".into(), "0".into());
    let result = run_one(&StreamingRepeat, vec![b"ab"], &params).await.unwrap();
    assert_eq!(result, Bytes::new());
}

#[tokio::test]
async fn test_streaming_tee_count() {
    let result = run_one(&StreamingTeeCount, vec![b"hello", b" world"], &HashMap::new()).await.unwrap();
    // Original data + "11" (byte count as ASCII)
    assert_eq!(result, Bytes::from("hello world11"));
}

#[tokio::test]
async fn test_streaming_tee_count_empty() {
    let result = run_one(&StreamingTeeCount, vec![], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("0"));
}

// =========================================================================
// Pipeline integration — new functions
// =========================================================================

#[tokio::test]
async fn test_pipeline_compress_then_decompress() {
    let mut pipeline = StreamPipeline::new(PipelineConfig::default());
    let idx_src = pipeline.add_source(test_addr("src"), Bytes::from("hello world"));
    let idx_comp = pipeline.add_streaming_stage(
        test_addr("comp"), Arc::new(StreamingCompress), HashMap::new(), vec![idx_src],
    );
    let _idx_dec = pipeline.add_streaming_stage(
        test_addr("dec"), Arc::new(StreamingDecompress), HashMap::new(), vec![idx_comp],
    );
    let out = pipeline.execute().await.unwrap();
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("hello world"));
}

#[tokio::test]
async fn test_pipeline_take_skip_compose() {
    // skip(3) then take(5) on "hello world" → "lo wo"
    let mut skip_params = HashMap::new();
    skip_params.insert("bytes".into(), "3".into());
    let mut take_params = HashMap::new();
    take_params.insert("bytes".into(), "5".into());

    let mut pipeline = StreamPipeline::new(PipelineConfig { chunk_size: 4, ..Default::default() });
    let idx_src = pipeline.add_source(test_addr("src"), Bytes::from("hello world"));
    let idx_skip = pipeline.add_streaming_stage(
        test_addr("skip"), Arc::new(StreamingSkip), skip_params, vec![idx_src],
    );
    let _idx_take = pipeline.add_streaming_stage(
        test_addr("take"), Arc::new(StreamingTake), take_params, vec![idx_skip],
    );
    let out = pipeline.execute().await.unwrap();
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("lo wo"));
}
