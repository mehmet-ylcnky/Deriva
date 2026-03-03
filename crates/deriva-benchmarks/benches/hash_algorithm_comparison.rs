/// §7.2 Hash Algorithm Comparison — 10 performance benchmark scenarios
///
/// B1: All 6 hash producers throughput at 1 MB
/// B2: All 6 hash producers per-chunk latency at 64 KB
/// B3: SHA-256 vs BLAKE3 scaling (64 KB, 256 KB, 1 MB, 4 MB)
/// B4: HMAC-SHA256 vs raw SHA-256 overhead
/// B5: CRC32 vs MD5 — lightweight integrity comparison
/// B6: ChunkHash throughput (per-chunk SHA-256 in CAS)
/// B7: RollingHash throughput at window sizes 32, 48, 128
/// B8: ChecksumVerify (CRC32) end-to-end throughput
/// B9: Sha256Verify end-to-end throughput
/// B10: Output size efficiency — bytes hashed per output byte

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use bytes::Bytes;
use tokio::sync::mpsc;
use std::collections::HashMap;

use deriva_compute::builtins_streaming::*;
use deriva_compute::streaming::{StreamingComputeFunction, collect_stream};
use deriva_core::streaming::StreamChunk;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

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

async fn run(f: &dyn StreamingComputeFunction, data: &Bytes, params: &HashMap<String, String>) -> Bytes {
    let rx = feed(data).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

fn make_data(size: usize) -> Bytes {
    Bytes::from(vec![0x41u8; size])
}

const HMAC_KEY: &str = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b";

// ── B1: All 6 hash producers throughput at 1 MB ─────────────────────
fn b1_hash_throughput_1mb(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let empty = hp(&[]);
    let hmac_p = hp(&[("key", HMAC_KEY)]);
    let mut g = c.benchmark_group("B1_hash_throughput_1mb");
    g.sample_size(20);

    let hashes: Vec<(&str, &dyn StreamingComputeFunction, &HashMap<String, String>)> = vec![
        ("md5", &StreamingMd5, &empty),
        ("sha256", &StreamingSha256, &empty),
        ("sha512", &StreamingSha512, &empty),
        ("blake3", &StreamingBlake3, &empty),
        ("crc32", &StreamingChecksum, &empty),
        ("hmac_sha256", &StreamingHmacSha256, &hmac_p),
    ];

    for (name, func, params) in &hashes {
        g.bench_function(*name, |b| b.iter(|| rt().block_on(run(*func, &data, params))));
    }
    g.finish();
}

// ── B2: Per-chunk latency at 64 KB ──────────────────────────────────
fn b2_hash_latency_64k(c: &mut Criterion) {
    let data = make_data(65536);
    let empty = hp(&[]);
    let hmac_p = hp(&[("key", HMAC_KEY)]);
    let mut g = c.benchmark_group("B2_hash_latency_64k");
    g.sample_size(50);

    let hashes: Vec<(&str, &dyn StreamingComputeFunction, &HashMap<String, String>)> = vec![
        ("md5", &StreamingMd5, &empty),
        ("sha256", &StreamingSha256, &empty),
        ("sha512", &StreamingSha512, &empty),
        ("blake3", &StreamingBlake3, &empty),
        ("crc32", &StreamingChecksum, &empty),
        ("hmac_sha256", &StreamingHmacSha256, &hmac_p),
    ];

    for (name, func, params) in &hashes {
        g.bench_function(*name, |b| b.iter(|| rt().block_on(run(*func, &data, params))));
    }
    g.finish();
}

// ── B3: SHA-256 vs BLAKE3 scaling ───────────────────────────────────
fn b3_sha256_vs_blake3_scaling(c: &mut Criterion) {
    let empty = hp(&[]);
    let mut g = c.benchmark_group("B3_sha256_vs_blake3");
    g.sample_size(10);

    for size in [65536, 262144, 1_000_000, 4_000_000] {
        let data = make_data(size);
        let label = format!("{}KB", size / 1024);
        g.bench_with_input(BenchmarkId::new("sha256", &label), &data, |b, d| {
            b.iter(|| rt().block_on(run(&StreamingSha256, d, &empty)));
        });
        g.bench_with_input(BenchmarkId::new("blake3", &label), &data, |b, d| {
            b.iter(|| rt().block_on(run(&StreamingBlake3, d, &empty)));
        });
    }
    g.finish();
}

// ── B4: HMAC-SHA256 vs raw SHA-256 overhead ─────────────────────────
fn b4_hmac_overhead(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let empty = hp(&[]);
    let hmac_p = hp(&[("key", HMAC_KEY)]);
    let mut g = c.benchmark_group("B4_hmac_overhead");
    g.sample_size(20);
    g.bench_function("sha256", |b| b.iter(|| rt().block_on(run(&StreamingSha256, &data, &empty))));
    g.bench_function("hmac_sha256", |b| b.iter(|| rt().block_on(run(&StreamingHmacSha256, &data, &hmac_p))));
    g.finish();
}

// ── B5: CRC32 vs MD5 — lightweight integrity ────────────────────────
fn b5_crc32_vs_md5(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let empty = hp(&[]);
    let mut g = c.benchmark_group("B5_crc32_vs_md5");
    g.sample_size(20);
    g.bench_function("crc32", |b| b.iter(|| rt().block_on(run(&StreamingChecksum, &data, &empty))));
    g.bench_function("md5", |b| b.iter(|| rt().block_on(run(&StreamingMd5, &data, &empty))));
    g.finish();
}

// ── B6: ChunkHash throughput ────────────────────────────────────────
fn b6_chunk_hash(c: &mut Criterion) {
    let empty = hp(&[]);
    let mut g = c.benchmark_group("B6_chunk_hash");
    g.sample_size(20);
    for size in [65536, 1_000_000] {
        let data = make_data(size);
        g.bench_with_input(BenchmarkId::from_parameter(format!("{}KB", size / 1024)), &data, |b, d| {
            b.iter(|| rt().block_on(run(&StreamingChunkHash, d, &empty)));
        });
    }
    g.finish();
}

// ── B7: RollingHash window sizes ────────────────────────────────────
fn b7_rolling_hash_windows(c: &mut Criterion) {
    let data = make_data(65536);
    let mut g = c.benchmark_group("B7_rolling_hash_windows");
    g.sample_size(20);
    for ws in ["32", "48", "128"] {
        let p = hp(&[("window_size", ws)]);
        g.bench_with_input(BenchmarkId::from_parameter(format!("w{ws}")), &data, |b, d| {
            b.iter(|| rt().block_on(run(&StreamingRollingHash, d, &p)));
        });
    }
    g.finish();
}

// ── B8: ChecksumVerify end-to-end ───────────────────────────────────
fn b8_checksum_verify(c: &mut Criterion) {
    let data = make_data(65536);
    let p = hp(&[("expected_crc32", "a09b0680")]);
    let mut g = c.benchmark_group("B8_checksum_verify");
    g.sample_size(50);
    g.bench_function("crc32_verify_64k", |b| {
        b.iter(|| rt().block_on(run(&StreamingChecksumVerify, &data, &p)));
    });
    g.finish();
}

// ── B9: Sha256Verify end-to-end ─────────────────────────────────────
fn b9_sha256_verify(c: &mut Criterion) {
    let data = make_data(65536);
    let p = hp(&[("expected_hash", "156c38442089c1323d3e3ba549a6ac24341c47e8b6367bec4740c9b8c865826e")]);
    let mut g = c.benchmark_group("B9_sha256_verify");
    g.sample_size(50);
    g.bench_function("sha256_verify_64k", |b| {
        b.iter(|| rt().block_on(run(&StreamingSha256Verify, &data, &p)));
    });
    g.finish();
}

// ── B10: Output size efficiency ─────────────────────────────────────
fn b10_output_efficiency(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let empty = hp(&[]);
    let hmac_p = hp(&[("key", HMAC_KEY)]);
    let mut g = c.benchmark_group("B10_output_efficiency");
    g.sample_size(10);

    let hashes: Vec<(&str, &dyn StreamingComputeFunction, &HashMap<String, String>)> = vec![
        ("crc32_4B", &StreamingChecksum, &empty),
        ("md5_16B", &StreamingMd5, &empty),
        ("sha256_32B", &StreamingSha256, &empty),
        ("sha512_64B", &StreamingSha512, &empty),
        ("blake3_32B", &StreamingBlake3, &empty),
        ("hmac256_32B", &StreamingHmacSha256, &hmac_p),
    ];

    for (name, func, params) in &hashes {
        g.bench_function(*name, |b| b.iter(|| rt().block_on(run(*func, &data, params))));
    }
    g.finish();
}

criterion_group!(
    benches,
    b1_hash_throughput_1mb,
    b2_hash_latency_64k,
    b3_sha256_vs_blake3_scaling,
    b4_hmac_overhead,
    b5_crc32_vs_md5,
    b6_chunk_hash,
    b7_rolling_hash_windows,
    b8_checksum_verify,
    b9_sha256_verify,
    b10_output_efficiency,
);
criterion_main!(benches);
