use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use std::collections::BTreeMap;
use deriva_core::address::Value;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// Group 1: CSV Operations
fn bench_csv(c: &mut Criterion) {
    use deriva_compute::builtins_format_csv::*;
    let mut group = c.benchmark_group("csv");
    
    let csv_data = "name,age,city\nAlice,30,NYC\nBob,25,LA\nCharlie,35,SF\n".repeat(100);
    group.bench_function("parse", |b| b.iter(|| {
        CsvParseFn.execute(vec![black_box(Bytes::from(csv_data.clone()))], &p(&[])).unwrap()
    }));
    
    let json_data = r#"[{"name":"Alice","age":"30"}]"#;
    group.bench_function("write", |b| b.iter(|| {
        CsvWriteFn.execute(vec![black_box(Bytes::from(json_data))], &p(&[])).unwrap()
    }));
    
    group.finish();
}

// Group 2: JSON/NDJSON Operations
fn bench_json(c: &mut Criterion) {
    use deriva_compute::builtins_format_csv::*;
    let mut group = c.benchmark_group("json");
    
    let ndjson = "{\"a\":1}\n{\"b\":2}\n".repeat(100);
    group.bench_function("ndjson_parse", |b| b.iter(|| {
        NdjsonParseFn.execute(vec![black_box(Bytes::from(ndjson.clone()))], &p(&[])).unwrap()
    }));
    
    let json = r#"[{"a":1},{"b":2}]"#;
    group.bench_function("ndjson_write", |b| b.iter(|| {
        NdjsonWriteFn.execute(vec![black_box(Bytes::from(json))], &p(&[])).unwrap()
    }));
    
    group.finish();
}

// Group 3: Serialization (Msgpack, CBOR, BSON)
fn bench_serialization(c: &mut Criterion) {
    use deriva_compute::builtins_format_serialization::*;
    let mut group = c.benchmark_group("serialization");
    
    let json = r#"{"key":"value","num":123,"nested":{"a":1}}"#;
    group.bench_function("msgpack_encode", |b| b.iter(|| {
        MsgpackEncodeFn.execute(vec![black_box(Bytes::from(json))], &p(&[])).unwrap()
    }));
    
    let encoded = MsgpackEncodeFn.execute(vec![Bytes::from(json)], &p(&[])).unwrap();
    group.bench_function("msgpack_decode", |b| b.iter(|| {
        MsgpackDecodeFn.execute(vec![black_box(encoded.clone())], &p(&[])).unwrap()
    }));
    
    group.bench_function("cbor_encode", |b| b.iter(|| {
        CborEncodeFn.execute(vec![black_box(Bytes::from(json))], &p(&[])).unwrap()
    }));
    
    group.finish();
}

// Group 4: Compression
fn bench_compression(c: &mut Criterion) {
    use deriva_compute::builtins_format_archive::*;
    let mut group = c.benchmark_group("compression");
    
    let data = "test data ".repeat(1000);
    group.bench_function("gzip_compress", |b| b.iter(|| {
        GzipCompressFn.execute(vec![black_box(Bytes::from(data.clone()))], &p(&[])).unwrap()
    }));
    
    let compressed = GzipCompressFn.execute(vec![Bytes::from(data.clone())], &p(&[])).unwrap();
    group.bench_function("gzip_decompress", |b| b.iter(|| {
        GzipDecompressFn.execute(vec![black_box(compressed.clone())], &p(&[])).unwrap()
    }));
    
    group.bench_function("zstd_compress", |b| b.iter(|| {
        ZstdFrameCompressFn.execute(vec![black_box(Bytes::from(data.clone()))], &p(&[])).unwrap()
    }));
    
    group.finish();
}

// Group 5: Config Formats
fn bench_config(c: &mut Criterion) {
    use deriva_compute::builtins_format_config::*;
    let mut group = c.benchmark_group("config");
    
    let yaml = "key: value\nlist:\n  - item1\n  - item2\n".repeat(10);
    group.bench_function("yaml_parse", |b| b.iter(|| {
        YamlParseFn.execute(vec![black_box(Bytes::from(yaml.clone()))], &p(&[])).unwrap()
    }));
    
    let json = r#"{"key":"value","list":["item1","item2"]}"#;
    group.bench_function("yaml_write", |b| b.iter(|| {
        YamlWriteFn.execute(vec![black_box(Bytes::from(json))], &p(&[])).unwrap()
    }));
    
    let toml = "key = \"value\"\n".repeat(10);
    group.bench_function("toml_parse", |b| b.iter(|| {
        TomlParseFn.execute(vec![black_box(Bytes::from(toml.clone()))], &p(&[])).unwrap()
    }));
    
    group.finish();
}

// Group 6: Document Processing
fn bench_document(c: &mut Criterion) {
    use deriva_compute::builtins_format_document::*;
    let mut group = c.benchmark_group("document");
    
    let html = "<html><body><p>test</p></body></html>".repeat(10);
    group.bench_function("html_to_text", |b| b.iter(|| {
        HtmlToTextFn.execute(vec![black_box(Bytes::from(html.clone()))], &p(&[])).unwrap()
    }));
    
    let markdown = "# Header\n\nParagraph\n".repeat(10);
    group.bench_function("markdown_to_html", |b| b.iter(|| {
        MarkdownToHtmlFn.execute(vec![black_box(Bytes::from(markdown.clone()))], &p(&[])).unwrap()
    }));
    
    group.finish();
}

// Group 7: Geospatial
fn bench_geo(c: &mut Criterion) {
    use deriva_compute::builtins_format_geo::*;
    let mut group = c.benchmark_group("geo");
    
    let geojson = r#"{"type":"Point","coordinates":[0,0]}"#;
    group.bench_function("geojson_validate", |b| b.iter(|| {
        GeoJsonValidateFn.execute(vec![black_box(Bytes::from(geojson))], &p(&[])).unwrap()
    }));
    
    let wkt = "POINT(0 0)";
    group.bench_function("wkt_to_geojson", |b| b.iter(|| {
        WktToGeoJsonFn.execute(vec![black_box(Bytes::from(wkt))], &p(&[])).unwrap()
    }));
    
    group.finish();
}

// Group 8: Format Detection
fn bench_detection(c: &mut Criterion) {
    use deriva_compute::builtins_format_detect::*;
    let mut group = c.benchmark_group("detection");
    
    let csv = "a,b,c\n1,2,3\n".repeat(10);
    group.bench_function("detect_csv", |b| b.iter(|| {
        FormatDetectFn.execute(vec![black_box(Bytes::from(csv.clone()))], &p(&[])).unwrap()
    }));
    
    let json = r#"{"key":"value"}"#;
    group.bench_function("detect_json", |b| b.iter(|| {
        FormatDetectFn.execute(vec![black_box(Bytes::from(json))], &p(&[])).unwrap()
    }));
    
    group.bench_function("universal_to_json", |b| b.iter(|| {
        UniversalToJsonFn.execute(vec![black_box(Bytes::from(csv.clone()))], &p(&[])).unwrap()
    }));
    
    group.finish();
}

// Group 9: Erasure Coding
fn bench_erasure(c: &mut Criterion) {
    use deriva_compute::builtins_format_erasure::*;
    let mut group = c.benchmark_group("erasure");
    
    let data = "test data ".repeat(100);
    group.bench_function("xor_parity", |b| b.iter(|| {
        let _ = XorParityFn.execute(vec![black_box(Bytes::from(data.clone()))], &p(&[]));
    }));
    
    group.finish();
}

// Group 10: CAS/IPFS
fn bench_cas(c: &mut Criterion) {
    use deriva_compute::builtins_format_cas::*;
    let mut group = c.benchmark_group("cas");
    
    let data = "test data for CID computation";
    group.bench_function("cid_compute", |b| b.iter(|| {
        CidComputeFn.execute(vec![black_box(Bytes::from(data))], &p(&[])).unwrap()
    }));
    
    group.finish();
}

// Group 11: Log Parsing
fn bench_log(c: &mut Criterion) {
    use deriva_compute::builtins_format_log::*;
    let mut group = c.benchmark_group("log");
    
    let syslog = "<34>Oct 11 22:14:15 mymachine su: 'su root' failed";
    group.bench_function("syslog_parse", |b| b.iter(|| {
        let _ = SyslogParseFn.execute(vec![black_box(Bytes::from(syslog))], &p(&[]));
    }));
    
    group.finish();
}

// Group 12: XML Processing
fn bench_xml(c: &mut Criterion) {
    use deriva_compute::builtins_format_csv::*;
    let mut group = c.benchmark_group("xml");
    
    let xml = "<?xml version='1.0'?><root><item>test</item></root>".repeat(10);
    group.bench_function("xml_parse", |b| b.iter(|| {
        XmlParseFn.execute(vec![black_box(Bytes::from(xml.clone()))], &p(&[])).unwrap()
    }));
    
    let json = r#"{"root":{"item":"test"}}"#;
    group.bench_function("xml_write", |b| b.iter(|| {
        XmlWriteFn.execute(vec![black_box(Bytes::from(json))], &p(&[])).unwrap()
    }));
    
    group.finish();
}

criterion_group!(
    benches,
    bench_csv,
    bench_json,
    bench_serialization,
    bench_compression,
    bench_config,
    bench_document,
    bench_geo,
    bench_detection,
    bench_erasure,
    bench_cas,
    bench_log,
    bench_xml
);
criterion_main!(benches);
