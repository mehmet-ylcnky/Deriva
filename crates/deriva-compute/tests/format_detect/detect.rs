use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_detect::FormatDetectFn;
use std::collections::BTreeMap;

fn detect(input: &[u8]) -> serde_json::Value {
    let result = FormatDetectFn.execute(vec![Bytes::from(input.to_vec())], &BTreeMap::new()).unwrap();
    serde_json::from_slice(&result).unwrap()
}

#[test]
fn detect_png_from_real_header() {
    // Real PNG header: magic + IHDR chunk for a 1x1 pixel image
    let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG magic
    png.extend_from_slice(&[0x00, 0x00, 0x00, 0x0D]); // IHDR length
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&[0; 13]); // IHDR data
    let r = detect(&png);
    assert_eq!(r["format"], "png");
    assert_eq!(r["mime"], "image/png");
    assert_eq!(r["category"], "image");
    assert!(r["confidence"].as_f64().unwrap() > 0.95);
}

#[test]
fn detect_gzip_compressed_data() {
    // Real gzip header: magic + method(deflate) + flags
    let gz = vec![0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff];
    let r = detect(&gz);
    assert_eq!(r["format"], "gzip");
    assert_eq!(r["category"], "archive");
}

#[test]
fn detect_csv_with_realistic_data() {
    let csv = b"name,age,city,salary\nAlice,30,NYC,85000\nBob,25,SF,92000\nCharlie,35,LA,78000\n";
    let r = detect(csv);
    assert_eq!(r["format"], "csv");
    assert_eq!(r["mime"], "text/csv");
    assert_eq!(r["category"], "row");
}

#[test]
fn detect_ndjson_multiline() {
    let ndjson = b"{\"event\":\"login\",\"user\":\"alice\"}\n{\"event\":\"logout\",\"user\":\"bob\"}\n{\"event\":\"error\",\"code\":500}\n";
    let r = detect(ndjson);
    assert_eq!(r["format"], "ndjson");
    assert_eq!(r["category"], "row");
}

#[test]
fn detect_empty_input_returns_unknown() {
    let r = detect(b"");
    assert_eq!(r["format"], "unknown");
    assert_eq!(r["confidence"], 0.0);
    assert_eq!(r["mime"], "application/octet-stream");
}
