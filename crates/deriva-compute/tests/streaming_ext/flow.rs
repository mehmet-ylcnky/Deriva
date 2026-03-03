use std::collections::HashMap;
use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant, sleep};
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

// Helper: make a stream from a channel sender (for manual control)
fn make_manual_stream() -> (mpsc::Sender<StreamChunk>, mpsc::Receiver<StreamChunk>) {
    mpsc::channel(8)
}

// ═══════════════════════════════════════════════════════════════════════
// #61 StreamingRateLimit
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rate_limit_passthrough() {
    let out = run_one(&StreamingRateLimit, vec![b"hello"], &hp(&[("bytes_per_sec", "1000000")])).await;
    assert_eq!(out.as_ref(), b"hello");
}

#[tokio::test]
async fn rate_limit_adds_delay() {
    let start = Instant::now();
    // 1000 bytes at 10000 bytes/sec = 0.1s delay
    let data = vec![0u8; 1000];
    let out = run_one(&StreamingRateLimit, vec![&data], &hp(&[("bytes_per_sec", "10000")])).await;
    assert_eq!(out.len(), 1000);
    assert!(start.elapsed() >= Duration::from_millis(50)); // at least some delay
}

#[tokio::test]
async fn rate_limit_empty() {
    let out = run_one(&StreamingRateLimit, vec![b""], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn rate_limit_multi_chunk() {
    let out = run_one(&StreamingRateLimit, vec![b"a", b"b"], &hp(&[("bytes_per_sec", "1000000")])).await;
    assert_eq!(out.as_ref(), b"ab");
}

#[tokio::test]
async fn rate_limit_default_1mb() {
    // Default 1MB/s, tiny chunk = negligible delay
    let out = run_one(&StreamingRateLimit, vec![b"x"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"x");
}

// ═══════════════════════════════════════════════════════════════════════
// #62 StreamingDelay
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn delay_passthrough() {
    let out = run_one(&StreamingDelay, vec![b"data"], &hp(&[("delay_ms", "1")])).await;
    assert_eq!(out.as_ref(), b"data");
}

#[tokio::test]
async fn delay_adds_latency() {
    let start = Instant::now();
    let _ = run_one(&StreamingDelay, vec![b"a", b"b"], &hp(&[("delay_ms", "50")])).await;
    assert!(start.elapsed() >= Duration::from_millis(80)); // 2 chunks × 50ms
}

#[tokio::test]
async fn delay_empty() {
    let out = run_one(&StreamingDelay, vec![b""], &hp(&[("delay_ms", "1")])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn delay_default_100ms() {
    let start = Instant::now();
    let _ = run_one(&StreamingDelay, vec![b"x"], &hp(&[])).await;
    assert!(start.elapsed() >= Duration::from_millis(80));
}

#[tokio::test]
async fn delay_preserves_content() {
    let out = run_one(&StreamingDelay, vec![b"hello", b" world"], &hp(&[("delay_ms", "1")])).await;
    assert_eq!(out.as_ref(), b"hello world");
}

// ═══════════════════════════════════════════════════════════════════════
// #63 StreamingTimeout
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn timeout_normal_passthrough() {
    let out = run_one(&StreamingTimeout, vec![b"data"], &hp(&[("timeout_ms", "5000")])).await;
    assert_eq!(out.as_ref(), b"data");
}

#[tokio::test]
async fn timeout_fires_on_stall() {
    let (tx, rx) = make_manual_stream();
    tx.send(StreamChunk::Data(Bytes::from("first"))).await.unwrap();
    // Don't send End — let it timeout
    let mut out = StreamingTimeout.stream_execute(vec![rx], &hp(&[("timeout_ms", "50")])).await;
    // First chunk should arrive
    match out.recv().await {
        Some(StreamChunk::Data(b)) => assert_eq!(b.as_ref(), b"first"),
        other => panic!("expected data, got {:?}", other),
    }
    // Then timeout error
    match out.recv().await {
        Some(StreamChunk::Error(e)) => assert!(e.to_string().contains("timeout")),
        other => panic!("expected timeout error, got {:?}", other),
    }
}

#[tokio::test]
async fn timeout_empty_stream() {
    let out = run_one(&StreamingTimeout, vec![], &hp(&[("timeout_ms", "5000")])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn timeout_multi_chunk() {
    let out = run_one(&StreamingTimeout, vec![b"a", b"b"], &hp(&[("timeout_ms", "5000")])).await;
    assert_eq!(out.as_ref(), b"ab");
}

#[tokio::test]
async fn timeout_default_5000ms() {
    // Fast stream, default timeout should not fire
    let out = run_one(&StreamingTimeout, vec![b"ok"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"ok");
}

// ═══════════════════════════════════════════════════════════════════════
// #64 StreamingRetry
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn retry_passthrough_no_errors() {
    let out = run_one(&StreamingRetry, vec![b"data"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"data");
}

#[tokio::test]
async fn retry_swallows_errors_under_max() {
    let (tx, rx) = make_manual_stream();
    tx.send(StreamChunk::Data(Bytes::from("a"))).await.unwrap();
    tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed("err1".into()))).await.unwrap();
    tx.send(StreamChunk::Data(Bytes::from("b"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    let out = collect_stream(StreamingRetry.stream_execute(vec![rx], &hp(&[("max_retries", "3")])).await).await.unwrap();
    assert_eq!(out.as_ref(), b"ab");
}

#[tokio::test]
async fn retry_fails_after_max() {
    let (tx, rx) = make_manual_stream();
    for _ in 0..4 {
        tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed("fail".into()))).await.unwrap();
    }
    let err = collect_stream(StreamingRetry.stream_execute(vec![rx], &hp(&[("max_retries", "3")])).await).await.unwrap_err();
    assert!(err.to_string().contains("fail"));
}

#[tokio::test]
async fn retry_default_3() {
    let (tx, rx) = make_manual_stream();
    // 3 errors (within limit) then data
    for _ in 0..3 {
        tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed("e".into()))).await.unwrap();
    }
    tx.send(StreamChunk::Data(Bytes::from("ok"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    let out = collect_stream(StreamingRetry.stream_execute(vec![rx], &hp(&[])).await).await.unwrap();
    assert_eq!(out.as_ref(), b"ok");
}

#[tokio::test]
async fn retry_zero_retries() {
    let (tx, rx) = make_manual_stream();
    tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed("boom".into()))).await.unwrap();
    let err = collect_stream(StreamingRetry.stream_execute(vec![rx], &hp(&[("max_retries", "0")])).await).await.unwrap_err();
    assert!(err.to_string().contains("boom"));
}

// ═══════════════════════════════════════════════════════════════════════
// #65 StreamingTee
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn tee_passthrough() {
    let out = run_one(&StreamingTee, vec![b"data"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"data");
}

#[tokio::test]
async fn tee_multi_chunk() {
    let out = run_one(&StreamingTee, vec![b"a", b"b", b"c"], &hp(&[("outputs", "2")])).await;
    assert_eq!(out.as_ref(), b"abc");
}

#[tokio::test]
async fn tee_single_output() {
    let out = run_one(&StreamingTee, vec![b"x"], &hp(&[("outputs", "1")])).await;
    assert_eq!(out.as_ref(), b"x");
}

#[tokio::test]
async fn tee_empty() {
    let out = run_one(&StreamingTee, vec![], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn tee_preserves_content() {
    let out = run_one(&StreamingTee, vec![b"hello world"], &hp(&[("outputs", "3")])).await;
    assert_eq!(out.as_ref(), b"hello world");
}

// ═══════════════════════════════════════════════════════════════════════
// #66 StreamingMerge
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn merge_two_inputs() {
    let rx1 = make_stream(vec![b"a"]).await;
    let rx2 = make_stream(vec![b"b"]).await;
    let out = collect_stream(StreamingMerge.stream_execute(vec![rx1, rx2], &hp(&[])).await).await.unwrap();
    // Both bytes present (order non-deterministic)
    assert_eq!(out.len(), 2);
    assert!(out.as_ref().contains(&b'a'));
    assert!(out.as_ref().contains(&b'b'));
}

#[tokio::test]
async fn merge_single_input() {
    let rx = make_stream(vec![b"only"]).await;
    let out = collect_stream(StreamingMerge.stream_execute(vec![rx], &hp(&[])).await).await.unwrap();
    assert_eq!(out.as_ref(), b"only");
}

#[tokio::test]
async fn merge_empty_inputs() {
    let rx1 = make_stream(vec![]).await;
    let rx2 = make_stream(vec![]).await;
    let out = collect_stream(StreamingMerge.stream_execute(vec![rx1, rx2], &hp(&[])).await).await.unwrap();
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn merge_preserves_all_data() {
    let rx1 = make_stream(vec![b"aa"]).await;
    let rx2 = make_stream(vec![b"bb"]).await;
    let rx3 = make_stream(vec![b"cc"]).await;
    let out = collect_stream(StreamingMerge.stream_execute(vec![rx1, rx2, rx3], &hp(&[])).await).await.unwrap();
    assert_eq!(out.len(), 6);
}

#[tokio::test]
async fn merge_multi_chunk_inputs() {
    let rx1 = make_stream(vec![b"a", b"b"]).await;
    let rx2 = make_stream(vec![b"c"]).await;
    let out = collect_stream(StreamingMerge.stream_execute(vec![rx1, rx2], &hp(&[])).await).await.unwrap();
    assert_eq!(out.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// #67 StreamingBroadcast
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn broadcast_passthrough() {
    let out = run_one(&StreamingBroadcast, vec![b"data"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"data");
}

#[tokio::test]
async fn broadcast_multi_chunk() {
    let out = run_one(&StreamingBroadcast, vec![b"a", b"b"], &hp(&[("outputs", "2")])).await;
    assert_eq!(out.as_ref(), b"ab");
}

#[tokio::test]
async fn broadcast_single_output() {
    let out = run_one(&StreamingBroadcast, vec![b"x"], &hp(&[("outputs", "1")])).await;
    assert_eq!(out.as_ref(), b"x");
}

#[tokio::test]
async fn broadcast_empty() {
    let out = run_one(&StreamingBroadcast, vec![], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn broadcast_preserves_content() {
    let out = run_one(&StreamingBroadcast, vec![b"hello"], &hp(&[("outputs", "3")])).await;
    assert_eq!(out.as_ref(), b"hello");
}

// ═══════════════════════════════════════════════════════════════════════
// #68 StreamingPartition
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn partition_non_empty() {
    let chunks = collect_chunks(&StreamingPartition, vec![b"a", b"", b"b"], &hp(&[("predicate", "non_empty")])).await;
    assert_eq!(chunks.len(), 2);
}

#[tokio::test]
async fn partition_contains() {
    let chunks = collect_chunks(&StreamingPartition, vec![b"hello", b"world", b"help"], &hp(&[("predicate", "contains:hel")])).await;
    assert_eq!(chunks.len(), 2); // "hello" and "help"
}

#[tokio::test]
async fn partition_min_size() {
    let chunks = collect_chunks(&StreamingPartition, vec![b"ab", b"abcde"], &hp(&[("predicate", "min_size:3")])).await;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].as_ref(), b"abcde");
}

#[tokio::test]
async fn partition_default_non_empty() {
    let chunks = collect_chunks(&StreamingPartition, vec![b"", b"x"], &hp(&[])).await;
    assert_eq!(chunks.len(), 1);
}

#[tokio::test]
async fn partition_all_match() {
    let out = run_one(&StreamingPartition, vec![b"abc", b"def"], &hp(&[("predicate", "min_size:1")])).await;
    assert_eq!(out.as_ref(), b"abcdef");
}

// ═══════════════════════════════════════════════════════════════════════
// #69 StreamingBatch
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn batch_4_into_1() {
    let chunks = collect_chunks(&StreamingBatch, vec![b"a", b"b", b"c", b"d"], &hp(&[("batch_size", "4")])).await;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].as_ref(), b"abcd");
}

#[tokio::test]
async fn batch_remainder() {
    // 5 chunks, batch_size=3 → 1 full batch + 1 remainder
    let chunks = collect_chunks(&StreamingBatch, vec![b"a", b"b", b"c", b"d", b"e"], &hp(&[("batch_size", "3")])).await;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].as_ref(), b"abc");
    assert_eq!(chunks[1].as_ref(), b"de");
}

#[tokio::test]
async fn batch_size_1() {
    let chunks = collect_chunks(&StreamingBatch, vec![b"a", b"b"], &hp(&[("batch_size", "1")])).await;
    assert_eq!(chunks.len(), 2);
}

#[tokio::test]
async fn batch_default_4() {
    let chunks = collect_chunks(&StreamingBatch, vec![b"1", b"2", b"3", b"4", b"5"], &hp(&[])).await;
    assert_eq!(chunks.len(), 2); // 4+1
}

#[tokio::test]
async fn batch_preserves_all_data() {
    let out = run_one(&StreamingBatch, vec![b"he", b"ll", b"o"], &hp(&[("batch_size", "2")])).await;
    assert_eq!(out.as_ref(), b"hello");
}

// ═══════════════════════════════════════════════════════════════════════
// #70 StreamingDebounce
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn debounce_single_chunk() {
    let out = run_one(&StreamingDebounce, vec![b"data"], &hp(&[("window_ms", "10")])).await;
    assert_eq!(out.as_ref(), b"data");
}

#[tokio::test]
async fn debounce_rapid_burst_keeps_last() {
    // All chunks arrive instantly, then End — should emit last chunk
    let out = run_one(&StreamingDebounce, vec![b"a", b"b", b"c"], &hp(&[("window_ms", "200")])).await;
    assert_eq!(out.as_ref(), b"c");
}

#[tokio::test]
async fn debounce_spaced_chunks() {
    // Chunks with gaps > window → each emitted
    let (tx, rx) = make_manual_stream();
    let params = hp(&[("window_ms", "30")]);
    let mut out = StreamingDebounce.stream_execute(vec![rx], &params).await;
    tx.send(StreamChunk::Data(Bytes::from("first"))).await.unwrap();
    sleep(Duration::from_millis(60)).await;
    tx.send(StreamChunk::Data(Bytes::from("second"))).await.unwrap();
    sleep(Duration::from_millis(60)).await;
    tx.send(StreamChunk::End).await.unwrap();
    let mut results = Vec::new();
    loop {
        match out.recv().await {
            Some(StreamChunk::Data(b)) => results.push(b),
            Some(StreamChunk::End) | None => break,
            _ => panic!("unexpected"),
        }
    }
    assert!(results.len() >= 2);
}

#[tokio::test]
async fn debounce_empty() {
    let out = run_one(&StreamingDebounce, vec![], &hp(&[("window_ms", "10")])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn debounce_default_100ms() {
    let out = run_one(&StreamingDebounce, vec![b"x"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"x");
}
