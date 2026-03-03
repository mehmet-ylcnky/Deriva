/// §7.1 Compression Codec Comparison — 10 performance benchmark scenarios
///
/// B1: Zstd compress throughput at levels 1, 3, 19 (1 MB text)
/// B2: Zstd decompress throughput at levels 1, 3, 19
/// B3: LZ4 vs Snappy compress throughput (1 MB text)
/// B4: LZ4 vs Snappy decompress throughput
/// B5: Brotli compress throughput at quality 4 vs 11 (256 KB text)
/// B6: Brotli decompress throughput at quality 4 vs 11
/// B7: All codecs compression ratio on text data (64 KB)
/// B8: All codecs compression ratio on binary data (64 KB)
/// B9: Per-chunk latency at 64 KB (all codecs, compress)
/// B10: Asymmetry factor — compress vs decompress speed ratio per codec

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

fn make_text(size: usize) -> Bytes {
    let line = "The quick brown fox jumps over the lazy dog. Lorem ipsum dolor sit amet.\n";
    Bytes::from(line.repeat(size / line.len() + 1)[..size].to_owned())
}

fn make_binary(size: usize) -> Bytes {
    let mut v = vec![0u8; size];
    for (i, b) in v.iter_mut().enumerate() { *b = (i.wrapping_mul(7) ^ (i >> 3)) as u8; }
    Bytes::from(v)
}

// ── B1: Zstd compress throughput at levels 1, 3, 19 ─────────────────
fn b1_zstd_compress_levels(c: &mut Criterion) {
    let data = make_text(1_000_000);
    let mut g = c.benchmark_group("B1_zstd_compress");
    g.sample_size(20);
    for level in ["1", "3", "19"] {
        let p = hp(&[("level", level)]);
        g.bench_with_input(BenchmarkId::from_parameter(format!("lvl{level}")), &data, |b, d| {
            b.iter(|| rt().block_on(run(&StreamingZstdCompress, d, &p)));
        });
    }
    g.finish();
}

// ── B2: Zstd decompress throughput at levels 1, 3, 19 ───────────────
fn b2_zstd_decompress_levels(c: &mut Criterion) {
    let data = make_text(1_000_000);
    let mut g = c.benchmark_group("B2_zstd_decompress");
    g.sample_size(20);
    for level in ["1", "3", "19"] {
        let compressed = rt().block_on(run(&StreamingZstdCompress, &data, &hp(&[("level", level)])));
        g.bench_with_input(BenchmarkId::from_parameter(format!("lvl{level}")), &compressed, |b, d| {
            b.iter(|| rt().block_on(run(&StreamingZstdDecompress, d, &hp(&[]))));
        });
    }
    g.finish();
}

// ── B3: LZ4 vs Snappy compress throughput ────────────────────────────
fn b3_lz4_vs_snappy_compress(c: &mut Criterion) {
    let data = make_text(1_000_000);
    let mut g = c.benchmark_group("B3_lz4_snappy_compress");
    g.sample_size(20);
    let p = hp(&[]);
    g.bench_function("lz4", |b| b.iter(|| rt().block_on(run(&StreamingLz4Compress, &data, &p))));
    g.bench_function("snappy", |b| b.iter(|| rt().block_on(run(&StreamingSnappyCompress, &data, &p))));
    g.finish();
}

// ── B4: LZ4 vs Snappy decompress throughput ──────────────────────────
fn b4_lz4_vs_snappy_decompress(c: &mut Criterion) {
    let data = make_text(1_000_000);
    let p = hp(&[]);
    let lz4c = rt().block_on(run(&StreamingLz4Compress, &data, &p));
    let snapc = rt().block_on(run(&StreamingSnappyCompress, &data, &p));
    let mut g = c.benchmark_group("B4_lz4_snappy_decompress");
    g.sample_size(20);
    g.bench_function("lz4", |b| b.iter(|| rt().block_on(run(&StreamingLz4Decompress, &lz4c, &p))));
    g.bench_function("snappy", |b| b.iter(|| rt().block_on(run(&StreamingSnappyDecompress, &snapc, &p))));
    g.finish();
}

// ── B5: Brotli compress throughput quality 4 vs 11 ───────────────────
fn b5_brotli_compress_quality(c: &mut Criterion) {
    let data = make_text(256_000);
    let mut g = c.benchmark_group("B5_brotli_compress");
    g.sample_size(10);
    for q in ["4", "11"] {
        let p = hp(&[("quality", q)]);
        g.bench_with_input(BenchmarkId::from_parameter(format!("q{q}")), &data, |b, d| {
            b.iter(|| rt().block_on(run(&StreamingBrotliCompress, d, &p)));
        });
    }
    g.finish();
}

// ── B6: Brotli decompress throughput quality 4 vs 11 ─────────────────
fn b6_brotli_decompress_quality(c: &mut Criterion) {
    let data = make_text(256_000);
    let mut g = c.benchmark_group("B6_brotli_decompress");
    g.sample_size(10);
    for q in ["4", "11"] {
        let compressed = rt().block_on(run(&StreamingBrotliCompress, &data, &hp(&[("quality", q)])));
        g.bench_with_input(BenchmarkId::from_parameter(format!("q{q}")), &compressed, |b, d| {
            b.iter(|| rt().block_on(run(&StreamingBrotliDecompress, d, &hp(&[]))));
        });
    }
    g.finish();
}

// ── B7: All codecs compression ratio on text (64 KB) ─────────────────
fn b7_ratio_text(c: &mut Criterion) {
    let data = make_text(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B7_ratio_text");
    g.sample_size(10);

    let codecs: Vec<(&str, &dyn StreamingComputeFunction, HashMap<String, String>)> = vec![
        ("zstd_1", &StreamingZstdCompress, hp(&[("level", "1")])),
        ("zstd_3", &StreamingZstdCompress, hp(&[("level", "3")])),
        ("zstd_19", &StreamingZstdCompress, hp(&[("level", "19")])),
        ("lz4", &StreamingLz4Compress, p.clone()),
        ("snappy", &StreamingSnappyCompress, p.clone()),
        ("brotli_4", &StreamingBrotliCompress, hp(&[("quality", "4")])),
        ("brotli_11", &StreamingBrotliCompress, hp(&[("quality", "11")])),
    ];

    for (name, func, params) in &codecs {
        g.bench_function(*name, |b| {
            b.iter(|| {
                let out = rt().block_on(run(*func, &data, params));
                // Return ratio for criterion to track
                data.len() as f64 / out.len() as f64
            });
        });
    }
    g.finish();
}

// ── B8: All codecs compression ratio on binary (64 KB) ───────────────
fn b8_ratio_binary(c: &mut Criterion) {
    let data = make_binary(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B8_ratio_binary");
    g.sample_size(10);

    let codecs: Vec<(&str, &dyn StreamingComputeFunction, HashMap<String, String>)> = vec![
        ("zstd_1", &StreamingZstdCompress, hp(&[("level", "1")])),
        ("zstd_3", &StreamingZstdCompress, hp(&[("level", "3")])),
        ("zstd_19", &StreamingZstdCompress, hp(&[("level", "19")])),
        ("lz4", &StreamingLz4Compress, p.clone()),
        ("snappy", &StreamingSnappyCompress, p.clone()),
        ("brotli_4", &StreamingBrotliCompress, hp(&[("quality", "4")])),
        ("brotli_11", &StreamingBrotliCompress, hp(&[("quality", "11")])),
    ];

    for (name, func, params) in &codecs {
        g.bench_function(*name, |b| {
            b.iter(|| {
                let out = rt().block_on(run(*func, &data, params));
                data.len() as f64 / out.len() as f64
            });
        });
    }
    g.finish();
}

// ── B9: Per-chunk latency at 64 KB (all codecs, compress) ────────────
fn b9_per_chunk_latency(c: &mut Criterion) {
    let data = make_text(65536); // single 64 KB chunk
    let p = hp(&[]);
    let mut g = c.benchmark_group("B9_chunk_latency_64k");
    g.sample_size(50);

    let codecs: Vec<(&str, &dyn StreamingComputeFunction, HashMap<String, String>)> = vec![
        ("zstd_1", &StreamingZstdCompress, hp(&[("level", "1")])),
        ("zstd_3", &StreamingZstdCompress, hp(&[("level", "3")])),
        ("zstd_19", &StreamingZstdCompress, hp(&[("level", "19")])),
        ("lz4", &StreamingLz4Compress, p.clone()),
        ("snappy", &StreamingSnappyCompress, p.clone()),
        ("brotli_4", &StreamingBrotliCompress, hp(&[("quality", "4")])),
        ("brotli_11", &StreamingBrotliCompress, hp(&[("quality", "11")])),
    ];

    for (name, func, params) in &codecs {
        g.bench_function(*name, |b| {
            b.iter(|| rt().block_on(run(*func, &data, params)));
        });
    }
    g.finish();
}

// ── B10: Asymmetry factor — compress/decompress speed ratio ──────────
fn b10_asymmetry(c: &mut Criterion) {
    let data = make_text(1_000_000);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B10_asymmetry");
    g.sample_size(10);

    let codecs: Vec<(&str, &dyn StreamingComputeFunction, &dyn StreamingComputeFunction, HashMap<String, String>)> = vec![
        ("zstd_3", &StreamingZstdCompress, &StreamingZstdDecompress, hp(&[("level", "3")])),
        ("lz4", &StreamingLz4Compress, &StreamingLz4Decompress, p.clone()),
        ("snappy", &StreamingSnappyCompress, &StreamingSnappyDecompress, p.clone()),
        ("brotli_4", &StreamingBrotliCompress, &StreamingBrotliDecompress, hp(&[("quality", "4")])),
    ];

    for (name, comp, decomp, params) in &codecs {
        let compressed = rt().block_on(run(*comp, &data, params));
        g.bench_function(format!("{name}_compress"), |b| {
            b.iter(|| rt().block_on(run(*comp, &data, params)));
        });
        g.bench_function(format!("{name}_decompress"), |b| {
            b.iter(|| rt().block_on(run(*decomp, &compressed, &hp(&[]))));
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    b1_zstd_compress_levels,
    b2_zstd_decompress_levels,
    b3_lz4_vs_snappy_compress,
    b4_lz4_vs_snappy_decompress,
    b5_brotli_compress_quality,
    b6_brotli_decompress_quality,
    b7_ratio_text,
    b8_ratio_binary,
    b9_per_chunk_latency,
    b10_asymmetry,
);
criterion_main!(benches);
