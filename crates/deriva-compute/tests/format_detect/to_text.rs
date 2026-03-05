use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_detect::UniversalToTextFn;
use std::collections::BTreeMap;

#[test]
fn csv_to_text_returns_raw_content() {
    let csv = b"name,age\nAlice,30\nBob,25\n";
    let result = UniversalToTextFn.execute(vec![Bytes::from(&csv[..])], &BTreeMap::new()).unwrap();
    assert_eq!(result, Bytes::from(&csv[..]));
}

#[test]
fn json_to_text_returns_raw_content() {
    let json = br#"{"key":"value","nested":{"a":1}}"#;
    let result = UniversalToTextFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    assert_eq!(result, Bytes::from(&json[..]));
}

#[test]
fn yaml_to_text_returns_raw_content() {
    let yaml = b"server:\n  host: 0.0.0.0\n  port: 8080\n";
    let result = UniversalToTextFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new()).unwrap();
    assert_eq!(result, Bytes::from(&yaml[..]));
}

#[test]
fn binary_png_returns_empty_text() {
    let png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0xFE];
    let result = UniversalToTextFn.execute(vec![Bytes::from(png)], &BTreeMap::new()).unwrap();
    assert!(result.is_empty());
}

#[test]
fn gzip_binary_returns_empty_text() {
    let gz = vec![0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];
    let result = UniversalToTextFn.execute(vec![Bytes::from(gz)], &BTreeMap::new()).unwrap();
    assert!(result.is_empty());
}
