/// §7.6 Buffered Function Memory Impact — 10 performance benchmark scenarios
///
/// All 9 buffered functions: JsonPrettyPrint, JsonMinify, JsonLines,
/// CsvToJson, JsonValidate, SchemaValidate, Sort, Unique, CharsetConvert.
///
/// B1:  JsonPrettyPrint vs JsonMinify (64 KB JSON)
/// B2:  JsonLines throughput (64 KB JSON array)
/// B3:  CsvToJson throughput (64 KB CSV)
/// B4:  JsonValidate vs SchemaValidate (64 KB JSON)
/// B5:  Sort vs Unique (1000 chunks × 64 bytes)
/// B6:  CharsetConvert buffered (64 KB latin1→utf8)
/// B7:  JSON scaling — JsonMinify at 4 KB, 16 KB, 64 KB, 256 KB
/// B8:  Sort scaling — 100, 500, 2000 chunks
/// B9:  CsvToJson vs JsonLines head-to-head (64 KB)
/// B10: Full buffered suite — all 9 functions

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

/// Feed data as N equal chunks (for Sort/Unique)
async fn feed_chunks(data: &[Bytes]) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(data.len() + 1);
    let chunks: Vec<Bytes> = data.to_vec();
    tokio::spawn(async move {
        for c in chunks { let _ = tx.send(StreamChunk::Data(c)).await; }
        let _ = tx.send(StreamChunk::End).await;
    });
    rx
}

async fn run(f: &dyn StreamingComputeFunction, data: &Bytes, params: &HashMap<String, String>) -> Bytes {
    let rx = feed(data).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

async fn run_multi(f: &dyn StreamingComputeFunction, chunks: &[Bytes], params: &HashMap<String, String>) -> Bytes {
    let rx = feed_chunks(chunks).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

fn make_json(size: usize) -> Bytes {
    // Build a JSON object with enough keys to reach target size
    let mut s = String::from("{");
    let mut i = 0;
    while s.len() < size - 10 {
        if i > 0 { s.push(','); }
        s.push_str(&format!("\"key{}\":\"value{}\"", i, i));
        i += 1;
    }
    s.push('}');
    Bytes::from(s)
}

fn make_json_array(size: usize) -> Bytes {
    let mut s = String::from("[");
    let mut i = 0;
    while s.len() < size - 10 {
        if i > 0 { s.push(','); }
        s.push_str(&format!("{{\"id\":{},\"name\":\"item{}\"}}", i, i));
        i += 1;
    }
    s.push(']');
    Bytes::from(s)
}

fn make_csv(size: usize) -> Bytes {
    let mut s = String::from("id,name,value\n");
    let mut i = 0;
    while s.len() < size {
        s.push_str(&format!("{},item{},{}\n", i, i, i * 10));
        i += 1;
    }
    Bytes::from(s)
}

fn make_sort_chunks(n: usize) -> Vec<Bytes> {
    (0..n).rev().map(|i| Bytes::from(format!("line-{:06}", i))).collect()
}

fn make_latin1(size: usize) -> Bytes {
    Bytes::from((0..size).map(|i| (i % 128) as u8).collect::<Vec<_>>())
}

// B1: JsonPrettyPrint vs JsonMinify
fn b1_pretty_vs_minify(c: &mut Criterion) {
    let r = rt();
    let data = make_json(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B1_json_format");
    g.bench_function("pretty_print", |b| b.iter(|| r.block_on(run(&StreamingJsonPrettyPrint, &data, &p))));
    g.bench_function("minify", |b| b.iter(|| r.block_on(run(&StreamingJsonMinify, &data, &p))));
    g.finish();
}

// B2: JsonLines
fn b2_json_lines(c: &mut Criterion) {
    let r = rt();
    let data = make_json_array(65536);
    c.bench_function("B2_json_lines_64k", |b| b.iter(|| r.block_on(run(&StreamingJsonLines, &data, &hp(&[])))));
}

// B3: CsvToJson
fn b3_csv_to_json(c: &mut Criterion) {
    let r = rt();
    let data = make_csv(65536);
    c.bench_function("B3_csv_to_json_64k", |b| b.iter(|| r.block_on(run(&StreamingCsvToJson, &data, &hp(&[])))));
}

// B4: JsonValidate vs SchemaValidate
fn b4_validate(c: &mut Criterion) {
    let r = rt();
    let data = make_json(65536);
    let schema = r#"{"type":"object"}"#;
    let mut g = c.benchmark_group("B4_validate");
    g.bench_function("json_validate", |b| b.iter(|| r.block_on(run(&StreamingJsonValidate, &data, &hp(&[])))));
    g.bench_function("schema_validate", |b| b.iter(|| r.block_on(run(&StreamingSchemaValidate, &data, &hp(&[("schema", schema)])))));
    g.finish();
}

// B5: Sort vs Unique
fn b5_sort_unique(c: &mut Criterion) {
    let r = rt();
    let chunks = make_sort_chunks(1000);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B5_sort_unique");
    g.bench_function("sort_1000", |b| b.iter(|| r.block_on(run_multi(&StreamingSort, &chunks, &p))));
    g.bench_function("unique_1000", |b| b.iter(|| r.block_on(run_multi(&StreamingUnique, &chunks, &p))));
    g.finish();
}

// B6: CharsetConvert buffered
fn b6_charset(c: &mut Criterion) {
    let r = rt();
    let data = make_latin1(65536);
    c.bench_function("B6_charset_latin1_64k", |b| b.iter(|| r.block_on(run(&StreamingCharsetConvert, &data, &hp(&[("from", "latin1"), ("to", "utf8")])))));
}

// B7: JSON scaling
fn b7_json_scaling(c: &mut Criterion) {
    let r = rt();
    let p = hp(&[]);
    let mut g = c.benchmark_group("B7_json_minify_scaling");
    for &kb in &[4, 16, 64, 256] {
        let data = make_json(kb * 1024);
        g.bench_with_input(BenchmarkId::new("size", format!("{}KB", kb)), &data, |b, d| {
            b.iter(|| r.block_on(run(&StreamingJsonMinify, d, &p)))
        });
    }
    g.finish();
}

// B8: Sort scaling
fn b8_sort_scaling(c: &mut Criterion) {
    let r = rt();
    let p = hp(&[]);
    let mut g = c.benchmark_group("B8_sort_scaling");
    for &n in &[100, 500, 2000] {
        let chunks = make_sort_chunks(n);
        g.bench_with_input(BenchmarkId::new("chunks", n), &chunks, |b, ch| {
            b.iter(|| r.block_on(run_multi(&StreamingSort, ch, &p)))
        });
    }
    g.finish();
}

// B9: CsvToJson vs JsonLines head-to-head
fn b9_head_to_head(c: &mut Criterion) {
    let r = rt();
    let csv = make_csv(65536);
    let json_arr = make_json_array(65536);
    let p = hp(&[]);
    let mut g = c.benchmark_group("B9_head_to_head");
    g.bench_function("csv_to_json", |b| b.iter(|| r.block_on(run(&StreamingCsvToJson, &csv, &p))));
    g.bench_function("json_lines", |b| b.iter(|| r.block_on(run(&StreamingJsonLines, &json_arr, &p))));
    g.finish();
}

// B10: Full buffered suite
fn b10_full_suite(c: &mut Criterion) {
    let r = rt();
    let json = make_json(65536);
    let json_arr = make_json_array(65536);
    let csv = make_csv(65536);
    let latin1 = make_latin1(65536);
    let sort_chunks = make_sort_chunks(500);
    let p = hp(&[]);
    let schema_p = hp(&[("schema", r#"{"type":"object"}"#)]);
    let charset_p = hp(&[("from", "latin1"), ("to", "utf8")]);
    let mut g = c.benchmark_group("B10_full_buffered_suite_64k");
    g.bench_function("json_pretty", |b| b.iter(|| r.block_on(run(&StreamingJsonPrettyPrint, &json, &p))));
    g.bench_function("json_minify", |b| b.iter(|| r.block_on(run(&StreamingJsonMinify, &json, &p))));
    g.bench_function("json_lines", |b| b.iter(|| r.block_on(run(&StreamingJsonLines, &json_arr, &p))));
    g.bench_function("csv_to_json", |b| b.iter(|| r.block_on(run(&StreamingCsvToJson, &csv, &p))));
    g.bench_function("json_validate", |b| b.iter(|| r.block_on(run(&StreamingJsonValidate, &json, &p))));
    g.bench_function("schema_validate", |b| b.iter(|| r.block_on(run(&StreamingSchemaValidate, &json, &schema_p))));
    g.bench_function("sort_500", |b| b.iter(|| r.block_on(run_multi(&StreamingSort, &sort_chunks, &p))));
    g.bench_function("unique_500", |b| b.iter(|| r.block_on(run_multi(&StreamingUnique, &sort_chunks, &p))));
    g.bench_function("charset_latin1", |b| b.iter(|| r.block_on(run(&StreamingCharsetConvert, &latin1, &charset_p))));
    g.finish();
}

criterion_group!(benches, b1_pretty_vs_minify, b2_json_lines, b3_csv_to_json, b4_validate, b5_sort_unique, b6_charset, b7_json_scaling, b8_sort_scaling, b9_head_to_head, b10_full_suite);
criterion_main!(benches);
