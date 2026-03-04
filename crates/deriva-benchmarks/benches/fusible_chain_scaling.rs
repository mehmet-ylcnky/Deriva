/// §7.7 Benchmark Specifications — Fusible Chain Scaling
///
/// Tests pipeline depth overhead for fusible (spawn_map) functions.
/// Measures channel overhead scaling as pipeline depth increases.
///
/// B1:  HexEncode→HexDecode roundtrip depth 2, 4, 8, 16
/// B2:  BitwiseNot chain depth 2, 4, 8, 16 (self-inverse, identity at even depth)
/// B3:  Base64Encode→Base64Decode roundtrip depth 2, 4, 8
/// B4:  Uppercase→Lowercase roundtrip depth 2, 4, 8
/// B5:  Identity chain depth 1, 4, 8, 16 (pure overhead baseline)
/// B6:  Mixed fusible pipeline: HexEncode→Uppercase→HexDecode (depth 3 vs 6 vs 9)
/// B7:  Single fusible function throughput comparison (1 MB)
/// B8:  Chain depth scaling — BitwiseNot at 64 KB vs 1 MB
/// B9:  Roundtrip pipeline: Base64Encode→Encrypt→Decrypt→Base64Decode
/// B10: Full fusible suite — all chain types at depth 4

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

async fn drain(rx: mpsc::Receiver<StreamChunk>) -> Bytes {
    collect_stream(rx).await.unwrap()
}

fn make_data(size: usize) -> Bytes { Bytes::from(vec![0x42u8; size]) }
fn make_text(size: usize) -> Bytes {
    let line = "the quick brown fox jumps over the lazy dog\n";
    let mut buf = Vec::with_capacity(size);
    while buf.len() < size { buf.extend_from_slice(line.as_bytes()); }
    buf.truncate(size);
    Bytes::from(buf)
}

const KEY_HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const NONCE_CTR: &str = "00000000000000000000000000000001";

// B1: HexEncode→HexDecode roundtrip
fn b1_hex_roundtrip(c: &mut Criterion) {
    let r = rt();
    let data = make_data(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B1_hex_roundtrip");
    for depth in [2, 4, 8, 16] {
        g.bench_with_input(BenchmarkId::new("depth", depth), &depth, |b, &d| {
            b.iter(|| r.block_on(async {
                let mut rx = feed(&data).await;
                for _ in 0..d / 2 {
                    rx = StreamingHexEncode.stream_execute(vec![rx], &p).await;
                    rx = StreamingHexDecode.stream_execute(vec![rx], &p).await;
                }
                drain(rx).await
            }))
        });
    }
    g.finish();
}

// B2: BitwiseNot chain (self-inverse)
fn b2_bitwise_not(c: &mut Criterion) {
    let r = rt();
    let data = make_data(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B2_bitwise_not_chain");
    for depth in [2, 4, 8, 16] {
        g.bench_with_input(BenchmarkId::new("depth", depth), &depth, |b, &d| {
            b.iter(|| r.block_on(async {
                let mut rx = feed(&data).await;
                for _ in 0..d { rx = StreamingBitwiseNot.stream_execute(vec![rx], &p).await; }
                drain(rx).await
            }))
        });
    }
    g.finish();
}

// B3: Base64 roundtrip
fn b3_base64_roundtrip(c: &mut Criterion) {
    let r = rt();
    let data = make_data(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B3_base64_roundtrip");
    for depth in [2, 4, 8] {
        g.bench_with_input(BenchmarkId::new("depth", depth), &depth, |b, &d| {
            b.iter(|| r.block_on(async {
                let mut rx = feed(&data).await;
                for _ in 0..d / 2 {
                    rx = StreamingBase64Encode.stream_execute(vec![rx], &p).await;
                    rx = StreamingBase64Decode.stream_execute(vec![rx], &p).await;
                }
                drain(rx).await
            }))
        });
    }
    g.finish();
}

// B4: Uppercase→Lowercase roundtrip
fn b4_case_roundtrip(c: &mut Criterion) {
    let r = rt();
    let data = make_text(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B4_case_roundtrip");
    for depth in [2, 4, 8] {
        g.bench_with_input(BenchmarkId::new("depth", depth), &depth, |b, &d| {
            b.iter(|| r.block_on(async {
                let mut rx = feed(&data).await;
                for _ in 0..d / 2 {
                    rx = StreamingUppercase.stream_execute(vec![rx], &p).await;
                    rx = StreamingLowercase.stream_execute(vec![rx], &p).await;
                }
                drain(rx).await
            }))
        });
    }
    g.finish();
}

// B5: Identity chain (pure overhead)
fn b5_identity(c: &mut Criterion) {
    let r = rt();
    let data = make_data(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B5_identity_chain");
    for depth in [1, 4, 8, 16] {
        g.bench_with_input(BenchmarkId::new("depth", depth), &depth, |b, &d| {
            b.iter(|| r.block_on(async {
                let mut rx = feed(&data).await;
                for _ in 0..d { rx = StreamingIdentity.stream_execute(vec![rx], &p).await; }
                drain(rx).await
            }))
        });
    }
    g.finish();
}

// B6: Mixed fusible pipeline
fn b6_mixed(c: &mut Criterion) {
    let r = rt();
    let data = make_data(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B6_mixed_fusible");
    for repeats in [1, 2, 3] {
        let depth = repeats * 3;
        g.bench_with_input(BenchmarkId::new("depth", depth), &repeats, |b, &reps| {
            b.iter(|| r.block_on(async {
                let mut rx = feed(&data).await;
                for _ in 0..reps {
                    rx = StreamingHexEncode.stream_execute(vec![rx], &p).await;
                    rx = StreamingUppercase.stream_execute(vec![rx], &p).await;
                    rx = StreamingHexDecode.stream_execute(vec![rx], &p).await;
                }
                drain(rx).await
            }))
        });
    }
    g.finish();
}

// B7: Single fusible function throughput (1 MB)
fn b7_single_throughput(c: &mut Criterion) {
    let r = rt();
    let data = make_data(1_000_000);
    let text = make_text(1_000_000);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B7_single_fusible_1mb");
    g.bench_function("identity", |b| b.iter(|| r.block_on(async { drain(StreamingIdentity.stream_execute(vec![feed(&data).await], &p).await).await })));
    g.bench_function("bitwise_not", |b| b.iter(|| r.block_on(async { drain(StreamingBitwiseNot.stream_execute(vec![feed(&data).await], &p).await).await })));
    g.bench_function("uppercase", |b| b.iter(|| r.block_on(async { drain(StreamingUppercase.stream_execute(vec![feed(&text).await], &p).await).await })));
    g.bench_function("hex_encode", |b| b.iter(|| r.block_on(async { drain(StreamingHexEncode.stream_execute(vec![feed(&data).await], &p).await).await })));
    g.bench_function("base64_encode", |b| b.iter(|| r.block_on(async { drain(StreamingBase64Encode.stream_execute(vec![feed(&data).await], &p).await).await })));
    g.finish();
}

// B8: BitwiseNot chain at 64 KB vs 1 MB
fn b8_size_vs_depth(c: &mut Criterion) {
    let r = rt();
    let p = hp(&[]);
    let mut g = c.benchmark_group("B8_not_size_depth");
    for &(label, size) in &[("64KB", 65536usize), ("1MB", 1_000_000)] {
        let data = make_data(size);
        for depth in [4, 8] {
            g.bench_with_input(BenchmarkId::new(label, depth), &depth, |b, &d| {
                b.iter(|| r.block_on(async {
                    let mut rx = feed(&data).await;
                    for _ in 0..d { rx = StreamingBitwiseNot.stream_execute(vec![rx], &p).await; }
                    drain(rx).await
                }))
            });
        }
    }
    g.finish();
}

// B9: Roundtrip pipeline with crypto
fn b9_crypto_roundtrip(c: &mut Criterion) {
    let r = rt();
    let data = make_data(65536);
    let p = hp(&[]);
    let cp = hp(&[("key", KEY_HEX), ("nonce", NONCE_CTR)]);
    c.bench_function("B9_b64_encrypt_decrypt_b64_64k", |b| {
        b.iter(|| r.block_on(async {
            let rx = feed(&data).await;
            let rx = StreamingBase64Encode.stream_execute(vec![rx], &p).await;
            let rx = StreamingEncrypt.stream_execute(vec![rx], &cp).await;
            let rx = StreamingDecrypt.stream_execute(vec![rx], &cp).await;
            let rx = StreamingBase64Decode.stream_execute(vec![rx], &p).await;
            drain(rx).await
        }))
    });
}

// B10: Full fusible suite at depth 4
fn b10_full_suite(c: &mut Criterion) {
    let r = rt();
    let data = make_data(65536);
    let text = make_text(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B10_full_fusible_depth4");
    g.bench_function("hex_roundtrip", |b| b.iter(|| r.block_on(async {
        let mut rx = feed(&data).await;
        for _ in 0..2 { rx = StreamingHexEncode.stream_execute(vec![rx], &p).await; rx = StreamingHexDecode.stream_execute(vec![rx], &p).await; }
        drain(rx).await
    })));
    g.bench_function("bitwise_not", |b| b.iter(|| r.block_on(async {
        let mut rx = feed(&data).await;
        for _ in 0..4 { rx = StreamingBitwiseNot.stream_execute(vec![rx], &p).await; }
        drain(rx).await
    })));
    g.bench_function("base64_roundtrip", |b| b.iter(|| r.block_on(async {
        let mut rx = feed(&data).await;
        for _ in 0..2 { rx = StreamingBase64Encode.stream_execute(vec![rx], &p).await; rx = StreamingBase64Decode.stream_execute(vec![rx], &p).await; }
        drain(rx).await
    })));
    g.bench_function("case_roundtrip", |b| b.iter(|| r.block_on(async {
        let mut rx = feed(&text).await;
        for _ in 0..2 { rx = StreamingUppercase.stream_execute(vec![rx], &p).await; rx = StreamingLowercase.stream_execute(vec![rx], &p).await; }
        drain(rx).await
    })));
    g.bench_function("identity", |b| b.iter(|| r.block_on(async {
        let mut rx = feed(&data).await;
        for _ in 0..4 { rx = StreamingIdentity.stream_execute(vec![rx], &p).await; }
        drain(rx).await
    })));
    g.finish();
}

criterion_group!(benches, b1_hex_roundtrip, b2_bitwise_not, b3_base64_roundtrip, b4_case_roundtrip, b5_identity, b6_mixed, b7_single_throughput, b8_size_vs_depth, b9_crypto_roundtrip, b10_full_suite);
criterion_main!(benches);
