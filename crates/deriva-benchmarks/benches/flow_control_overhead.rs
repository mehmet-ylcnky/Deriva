/// §7.5 Flow Control Overhead — 10 performance benchmark scenarios
///
/// All 10 flow functions: RateLimit, Delay, Timeout, Retry, Tee,
/// Merge, Broadcast, Partition, Batch, Debounce.
///
/// B1:  Passthrough baseline — Timeout with generous limit (1 MB)
/// B2:  RateLimit overhead — high rate (effectively passthrough, 1 MB)
/// B3:  Delay overhead — 0 ms delay (1 MB)
/// B4:  Retry passthrough — no errors (1 MB)
/// B5:  Tee scaling — N=2, N=5, N=10 (1 MB)
/// B6:  Broadcast scaling — N=2, N=5, N=10 (1 MB)
/// B7:  Merge — 1, 3, 5 input streams (1 MB total)
/// B8:  Partition — non_empty vs contains predicate (1 MB)
/// B9:  Batch — batch_size 1, 4, 16 (64 KB × 16 chunks)
/// B10: Full flow suite — all 10 functions (1 MB)

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use bytes::Bytes;
use tokio::sync::mpsc;
use std::collections::HashMap;

use deriva_compute::builtins_streaming::*;
use deriva_compute::streaming::{StreamingComputeFunction, collect_stream};
use deriva_core::streaming::StreamChunk;

fn rt() -> tokio::runtime::Runtime { tokio::runtime::Runtime::new().unwrap() }

fn hp(kv: &[(&str, &str)]) -> HashMap<String, String> {
    kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

async fn feed(data: &Bytes) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(2);
    let d = data.clone();
    tokio::spawn(async move {
        let _ = tx.send(StreamChunk::Data(d)).await;
        let _ = tx.send(StreamChunk::End).await;
    });
    rx
}

/// Feed data as N equal chunks
async fn feed_chunks(data: &Bytes, n: usize) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(n + 1);
    let d = data.clone();
    tokio::spawn(async move {
        let chunk_size = d.len() / n.max(1);
        for i in 0..n {
            let start = i * chunk_size;
            let end = if i == n - 1 { d.len() } else { start + chunk_size };
            let _ = tx.send(StreamChunk::Data(d.slice(start..end))).await;
        }
        let _ = tx.send(StreamChunk::End).await;
    });
    rx
}

/// Feed N separate input receivers (for Merge)
async fn feed_n_inputs(data: &Bytes, n: usize) -> Vec<mpsc::Receiver<StreamChunk>> {
    let chunk_size = data.len() / n.max(1);
    let mut rxs = Vec::with_capacity(n);
    for i in 0..n {
        let (tx, rx) = mpsc::channel(2);
        let start = i * chunk_size;
        let end = if i == n - 1 { data.len() } else { start + chunk_size };
        let slice = data.slice(start..end);
        tokio::spawn(async move {
            let _ = tx.send(StreamChunk::Data(slice)).await;
            let _ = tx.send(StreamChunk::End).await;
        });
        rxs.push(rx);
    }
    rxs
}

async fn run(f: &dyn StreamingComputeFunction, data: &Bytes, params: &HashMap<String, String>) -> Bytes {
    let rx = feed(data).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

async fn run_chunks(f: &dyn StreamingComputeFunction, data: &Bytes, n: usize, params: &HashMap<String, String>) -> Bytes {
    let rx = feed_chunks(data, n).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

async fn run_merge(data: &Bytes, n: usize) -> Bytes {
    let rxs = feed_n_inputs(data, n).await;
    let p = hp(&[]);
    collect_stream(StreamingMerge.stream_execute(rxs, &p).await).await.unwrap()
}

fn make_data(size: usize) -> Bytes {
    Bytes::from(vec![0xABu8; size])
}

// B1: Timeout passthrough
fn b1_timeout(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_000_000);
    c.bench_function("B1_timeout_passthrough_1mb", |b| {
        b.iter(|| r.block_on(run(&StreamingTimeout, &data, &hp(&[("timeout_ms", "60000")]))))
    });
}

// B2: RateLimit at very high rate (effectively passthrough)
fn b2_rate_limit(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_000_000);
    c.bench_function("B2_rate_limit_high_1mb", |b| {
        b.iter(|| r.block_on(run(&StreamingRateLimit, &data, &hp(&[("bytes_per_sec", "10000000000")]))))
    });
}

// B3: Delay with 0ms
fn b3_delay(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_000_000);
    c.bench_function("B3_delay_0ms_1mb", |b| {
        b.iter(|| r.block_on(run(&StreamingDelay, &data, &hp(&[("delay_ms", "0")]))))
    });
}

// B4: Retry passthrough (no errors)
fn b4_retry(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_000_000);
    c.bench_function("B4_retry_passthrough_1mb", |b| {
        b.iter(|| r.block_on(run(&StreamingRetry, &data, &hp(&[("max_retries", "3")]))))
    });
}

// B5: Tee scaling
fn b5_tee(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_000_000);
    let mut g = c.benchmark_group("B5_tee");
    for n in [2, 5, 10] {
        let ns = n.to_string();
        g.bench_with_input(BenchmarkId::new("outputs", n), &n, |b, _| {
            b.iter(|| r.block_on(run(&StreamingTee, &data, &hp(&[("outputs", &ns)]))))
        });
    }
    g.finish();
}

// B6: Broadcast scaling
fn b6_broadcast(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_000_000);
    let mut g = c.benchmark_group("B6_broadcast");
    for n in [2, 5, 10] {
        let ns = n.to_string();
        g.bench_with_input(BenchmarkId::new("outputs", n), &n, |b, _| {
            b.iter(|| r.block_on(run(&StreamingBroadcast, &data, &hp(&[("outputs", &ns)]))))
        });
    }
    g.finish();
}

// B7: Merge N inputs
fn b7_merge(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_000_000);
    let mut g = c.benchmark_group("B7_merge");
    for n in [1, 3, 5] {
        g.bench_with_input(BenchmarkId::new("inputs", n), &n, |b, &n| {
            b.iter(|| r.block_on(run_merge(&data, n)))
        });
    }
    g.finish();
}

// B8: Partition predicates
fn b8_partition(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_000_000);
    let mut g = c.benchmark_group("B8_partition");
    g.bench_function("non_empty", |b| b.iter(|| r.block_on(run(&StreamingPartition, &data, &hp(&[("predicate", "non_empty")])))));
    g.bench_function("contains", |b| b.iter(|| r.block_on(run(&StreamingPartition, &data, &hp(&[("predicate", "contains:AB")])))));
    g.finish();
}

// B9: Batch sizes (16 chunks of 64 KB = 1 MB)
fn b9_batch(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_048_576); // exactly 1 MB
    let mut g = c.benchmark_group("B9_batch");
    for bs in [1, 4, 16] {
        let bss = bs.to_string();
        g.bench_with_input(BenchmarkId::new("batch_size", bs), &bs, |b, _| {
            b.iter(|| r.block_on(run_chunks(&StreamingBatch, &data, 16, &hp(&[("batch_size", &bss)]))))
        });
    }
    g.finish();
}

// B10: Full flow suite
fn b10_full_suite(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_000_000);
    let mut g = c.benchmark_group("B10_full_flow_suite_1mb");
    g.bench_function("timeout", |b| b.iter(|| r.block_on(run(&StreamingTimeout, &data, &hp(&[("timeout_ms", "60000")])))));
    g.bench_function("rate_limit", |b| b.iter(|| r.block_on(run(&StreamingRateLimit, &data, &hp(&[("bytes_per_sec", "10000000000")])))));
    g.bench_function("delay_0ms", |b| b.iter(|| r.block_on(run(&StreamingDelay, &data, &hp(&[("delay_ms", "0")])))));
    g.bench_function("retry", |b| b.iter(|| r.block_on(run(&StreamingRetry, &data, &hp(&[("max_retries", "3")])))));
    g.bench_function("tee_2", |b| b.iter(|| r.block_on(run(&StreamingTee, &data, &hp(&[("outputs", "2")])))));
    g.bench_function("broadcast_2", |b| b.iter(|| r.block_on(run(&StreamingBroadcast, &data, &hp(&[("outputs", "2")])))));
    g.bench_function("merge_3", |b| b.iter(|| r.block_on(run_merge(&data, 3))));
    g.bench_function("partition", |b| b.iter(|| r.block_on(run(&StreamingPartition, &data, &hp(&[("predicate", "non_empty")])))));
    g.bench_function("batch_4", |b| b.iter(|| r.block_on(run_chunks(&StreamingBatch, &data, 16, &hp(&[("batch_size", "4")])))));
    g.bench_function("debounce_0ms", |b| b.iter(|| r.block_on(run(&StreamingDebounce, &data, &hp(&[("window_ms", "0")])))));
    g.finish();
}

criterion_group!(benches, b1_timeout, b2_rate_limit, b3_delay, b4_retry, b5_tee, b6_broadcast, b7_merge, b8_partition, b9_batch, b10_full_suite);
criterion_main!(benches);
