/// §7.4 Text Processing Overhead — 10 performance benchmark scenarios
///
/// All 8 text functions: Replace, Prefix, Suffix, LinePrefix, Grep, Sed,
/// TruncateLines, CharsetConvert.
///
/// B1:  Replace — literal vs regex (1 MB)
/// B2:  Grep — simple literal vs complex regex (1 MB)
/// B3:  Sed — simple replace vs capture groups (1 MB)
/// B4:  LinePrefix throughput (1 MB)
/// B5:  Prefix + Suffix throughput (1 MB)
/// B6:  TruncateLines throughput (1 MB)
/// B7:  CharsetConvert — utf8→utf8 vs latin1→utf8 (1 MB)
/// B8:  Text scaling — Replace at 64 KB, 256 KB, 1 MB, 4 MB
/// B9:  Grep vs Sed vs Replace head-to-head (1 MB, same simple pattern)
/// B10: Full text suite — all 8 functions (1 MB)

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

async fn run(f: &dyn StreamingComputeFunction, data: &Bytes, params: &HashMap<String, String>) -> Bytes {
    let rx = feed(data).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

fn make_text(size: usize) -> Bytes {
    let line = "The quick brown fox jumps over the lazy dog 12345 hello@example.com\n";
    let mut buf = Vec::with_capacity(size);
    while buf.len() < size { buf.extend_from_slice(line.as_bytes()); }
    buf.truncate(size);
    Bytes::from(buf)
}

fn make_latin1(size: usize) -> Bytes {
    let mut buf = vec![0u8; size];
    for (i, b) in buf.iter_mut().enumerate() { *b = (i % 128) as u8; }
    Bytes::from(buf)
}

// B1: Replace — literal vs regex
fn b1_replace(c: &mut Criterion) {
    let r = rt();
    let data = make_text(1_000_000);
    let f = StreamingReplace;
    let mut g = c.benchmark_group("B1_replace");
    g.bench_function("literal", |b| b.iter(|| r.block_on(run(&f, &data, &hp(&[("find", "fox"), ("replace", "cat")])))));
    g.bench_function("regex", |b| b.iter(|| r.block_on(run(&f, &data, &hp(&[("find", r"\d+"), ("replace", "NUM")])))));
    g.finish();
}

// B2: Grep — simple vs complex regex
fn b2_grep(c: &mut Criterion) {
    let r = rt();
    let data = make_text(1_000_000);
    let f = StreamingGrep;
    let mut g = c.benchmark_group("B2_grep");
    g.bench_function("simple", |b| b.iter(|| r.block_on(run(&f, &data, &hp(&[("pattern", "fox")])))));
    g.bench_function("complex_regex", |b| b.iter(|| r.block_on(run(&f, &data, &hp(&[("pattern", r"\b\w+@\w+\.\w+\b")])))));
    g.finish();
}

// B3: Sed — simple vs capture groups
fn b3_sed(c: &mut Criterion) {
    let r = rt();
    let data = make_text(1_000_000);
    let f = StreamingSed;
    let mut g = c.benchmark_group("B3_sed");
    g.bench_function("simple", |b| b.iter(|| r.block_on(run(&f, &data, &hp(&[("pattern", "fox"), ("replacement", "cat")])))));
    g.bench_function("capture_groups", |b| b.iter(|| r.block_on(run(&f, &data, &hp(&[("pattern", r"(\w+)@(\w+)\.(\w+)"), ("replacement", "$1_at_$2")])))));
    g.finish();
}

// B4: LinePrefix throughput
fn b4_line_prefix(c: &mut Criterion) {
    let r = rt();
    let data = make_text(1_000_000);
    let f = StreamingLinePrefix;
    c.bench_function("B4_line_prefix_1mb", |b| b.iter(|| r.block_on(run(&f, &data, &hp(&[("prefix", ">>> ")])))));
}

// B5: Prefix + Suffix
fn b5_prefix_suffix(c: &mut Criterion) {
    let r = rt();
    let data = make_text(1_000_000);
    let mut g = c.benchmark_group("B5_prefix_suffix");
    g.bench_function("prefix", |b| b.iter(|| r.block_on(run(&StreamingPrefix, &data, &hp(&[("prefix", "HEADER\n")])))));
    g.bench_function("suffix", |b| b.iter(|| r.block_on(run(&StreamingSuffix, &data, &hp(&[("suffix", "\nFOOTER")])))));
    g.finish();
}

// B6: TruncateLines
fn b6_truncate_lines(c: &mut Criterion) {
    let r = rt();
    let data = make_text(1_000_000);
    let f = StreamingTruncateLines;
    let mut g = c.benchmark_group("B6_truncate_lines");
    g.bench_function("max_1024", |b| b.iter(|| r.block_on(run(&f, &data, &hp(&[("max_line_bytes", "1024")])))));
    g.bench_function("max_40", |b| b.iter(|| r.block_on(run(&f, &data, &hp(&[("max_line_bytes", "40")])))));
    g.finish();
}

// B7: CharsetConvert
fn b7_charset_convert(c: &mut Criterion) {
    let r = rt();
    let utf8 = make_text(1_000_000);
    let latin1 = make_latin1(1_000_000);
    let f = StreamingCharsetConvert;
    let mut g = c.benchmark_group("B7_charset_convert");
    g.bench_function("utf8_to_utf8", |b| b.iter(|| r.block_on(run(&f, &utf8, &hp(&[("from", "utf8"), ("to", "utf8")])))));
    g.bench_function("latin1_to_utf8", |b| b.iter(|| r.block_on(run(&f, &latin1, &hp(&[("from", "latin1"), ("to", "utf8")])))));
    g.finish();
}

// B8: Replace scaling
fn b8_scaling(c: &mut Criterion) {
    let r = rt();
    let f = StreamingReplace;
    let p = hp(&[("find", "fox"), ("replace", "cat")]);
    let mut g = c.benchmark_group("B8_replace_scaling");
    for &kb in &[64, 256, 1000, 4000] {
        let data = make_text(kb * 1024);
        g.bench_with_input(BenchmarkId::new("size", format!("{}KB", kb)), &data, |b, d| {
            b.iter(|| r.block_on(run(&f, d, &p)))
        });
    }
    g.finish();
}

// B9: Grep vs Sed vs Replace head-to-head
fn b9_head_to_head(c: &mut Criterion) {
    let r = rt();
    let data = make_text(1_000_000);
    let mut g = c.benchmark_group("B9_head_to_head");
    g.bench_function("replace", |b| b.iter(|| r.block_on(run(&StreamingReplace, &data, &hp(&[("find", "fox"), ("replace", "cat")])))));
    g.bench_function("grep", |b| b.iter(|| r.block_on(run(&StreamingGrep, &data, &hp(&[("pattern", "fox")])))));
    g.bench_function("sed", |b| b.iter(|| r.block_on(run(&StreamingSed, &data, &hp(&[("pattern", "fox"), ("replacement", "cat")])))));
    g.finish();
}

// B10: Full text suite
fn b10_full_suite(c: &mut Criterion) {
    let r = rt();
    let data = make_text(1_000_000);
    let latin1 = make_latin1(1_000_000);
    let mut g = c.benchmark_group("B10_full_text_suite_1mb");
    g.bench_function("replace_literal", |b| b.iter(|| r.block_on(run(&StreamingReplace, &data, &hp(&[("find", "fox"), ("replace", "cat")])))));
    g.bench_function("grep_simple", |b| b.iter(|| r.block_on(run(&StreamingGrep, &data, &hp(&[("pattern", "fox")])))));
    g.bench_function("sed_simple", |b| b.iter(|| r.block_on(run(&StreamingSed, &data, &hp(&[("pattern", "fox"), ("replacement", "cat")])))));
    g.bench_function("line_prefix", |b| b.iter(|| r.block_on(run(&StreamingLinePrefix, &data, &hp(&[("prefix", ">>> ")])))));
    g.bench_function("prefix", |b| b.iter(|| r.block_on(run(&StreamingPrefix, &data, &hp(&[("prefix", "HEADER\n")])))));
    g.bench_function("suffix", |b| b.iter(|| r.block_on(run(&StreamingSuffix, &data, &hp(&[("suffix", "\nFOOTER")])))));
    g.bench_function("truncate_lines", |b| b.iter(|| r.block_on(run(&StreamingTruncateLines, &data, &hp(&[("max_line_bytes", "40")])))));
    g.bench_function("charset_latin1", |b| b.iter(|| r.block_on(run(&StreamingCharsetConvert, &latin1, &hp(&[("from", "latin1"), ("to", "utf8")])))));
    g.finish();
}

criterion_group!(benches, b1_replace, b2_grep, b3_sed, b4_line_prefix, b5_prefix_suffix, b6_truncate_lines, b7_charset_convert, b8_scaling, b9_head_to_head, b10_full_suite);
criterion_main!(benches);
