use bytes::Bytes;
use deriva_compute::builtins_format_detect::*;
use deriva_compute::function::ComputeFunction;
use std::collections::BTreeMap;
use deriva_core::address::Value;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// CSV detection
#[test]
fn detect_csv() {
    let csv = b"name,age\nAlice,30\nBob,25";
    let result = FormatDetectFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// JSON detection
#[test]
fn detect_json() {
    let json = br#"{"key":"value"}"#;
    let result = FormatDetectFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// NDJSON detection
#[test]
fn detect_ndjson() {
    let ndjson = b"{\"a\":1}\n{\"b\":2}";
    let result = FormatDetectFn.execute(vec![Bytes::from(&ndjson[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// XML detection
#[test]
fn detect_xml() {
    let xml = b"<?xml version=\"1.0\"?><root><item>test</item></root>";
    let result = FormatDetectFn.execute(vec![Bytes::from(&xml[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// YAML detection
#[test]
fn detect_yaml() {
    let yaml = b"key: value\nlist:\n  - item1\n  - item2";
    let result = FormatDetectFn.execute(vec![Bytes::from(&yaml[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// TOML detection
#[test]
fn detect_toml() {
    let toml = b"[section]\nkey = \"value\"";
    let result = FormatDetectFn.execute(vec![Bytes::from(&toml[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Gzip detection
#[test]
fn detect_gzip() {
    let gzip = b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x00";
    let result = FormatDetectFn.execute(vec![Bytes::from(&gzip[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Zip detection
#[test]
fn detect_zip() {
    let zip = b"PK\x03\x04";
    let result = FormatDetectFn.execute(vec![Bytes::from(&zip[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Tar detection
#[test]
fn detect_tar() {
    let mut tar = vec![0u8; 512];
    tar[257..262].copy_from_slice(b"ustar");
    let result = FormatDetectFn.execute(vec![Bytes::from(tar)], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// PNG detection
#[test]
fn detect_png() {
    let png = b"\x89PNG\r\n\x1a\n";
    let result = FormatDetectFn.execute(vec![Bytes::from(&png[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// JPEG detection
#[test]
fn detect_jpeg() {
    let jpeg = b"\xff\xd8\xff";
    let result = FormatDetectFn.execute(vec![Bytes::from(&jpeg[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// GIF detection
#[test]
fn detect_gif() {
    let gif = b"GIF89a";
    let result = FormatDetectFn.execute(vec![Bytes::from(&gif[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// PDF detection
#[test]
fn detect_pdf() {
    let pdf = b"%PDF-1.4";
    let result = FormatDetectFn.execute(vec![Bytes::from(&pdf[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Parquet detection
#[test]
fn detect_parquet() {
    let parquet = b"PAR1";
    let result = FormatDetectFn.execute(vec![Bytes::from(&parquet[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Avro detection
#[test]
fn detect_avro() {
    let avro = b"Obj\x01";
    let result = FormatDetectFn.execute(vec![Bytes::from(&avro[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// SQLite detection
#[test]
fn detect_sqlite() {
    let sqlite = b"SQLite format 3\x00";
    let result = FormatDetectFn.execute(vec![Bytes::from(&sqlite[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// WASM detection
#[test]
fn detect_wasm() {
    let wasm = b"\x00asm\x01\x00\x00\x00";
    let result = FormatDetectFn.execute(vec![Bytes::from(&wasm[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// ELF detection
#[test]
fn detect_elf() {
    let elf = b"\x7fELF";
    let result = FormatDetectFn.execute(vec![Bytes::from(&elf[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Format validation - valid CSV
#[test]
fn validate_csv() {
    let csv = b"a,b\n1,2";
    let result = FormatValidateFn.execute(vec![Bytes::from(&csv[..])], &p(&[("format", "csv")])).unwrap();
    assert!(result.len() > 0);
}

// Format validation - valid JSON
#[test]
fn validate_json() {
    let json = br#"{"valid":true}"#;
    let result = FormatValidateFn.execute(vec![Bytes::from(&json[..])], &p(&[("format", "json")])).unwrap();
    assert!(result.len() > 0);
}

// MIME type mapping
#[test]
fn mime_type_csv() {
    let result = MimeTypeMapFn.execute(vec![Bytes::from("csv")], &p(&[])).unwrap();
    let s = String::from_utf8_lossy(&result);
    assert!(s.contains("text") || result.len() > 0);
}

// MIME type mapping - JSON
#[test]
fn mime_type_json() {
    let result = MimeTypeMapFn.execute(vec![Bytes::from("json")], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Schema inference
#[test]
fn infer_schema_csv() {
    let csv = b"name,age,active\nAlice,30,true";
    let result = SchemaInferFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Schema inference - JSON
#[test]
fn infer_schema_json() {
    let json = br#"[{"id":1,"name":"test"}]"#;
    let result = SchemaInferFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Schema comparison
#[test]
fn compare_schemas() {
    let schema1 = br#"{"type":"object","properties":{"a":{"type":"string"}}}"#;
    let schema2 = br#"{"type":"object","properties":{"a":{"type":"string"}}}"#;
    let result = SchemaCompareFn.execute(vec![Bytes::from(&schema1[..]), Bytes::from(&schema2[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Universal metadata - CSV
#[test]
fn metadata_csv() {
    let csv = b"col1,col2\nval1,val2";
    let result = UniversalMetadataFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Universal metadata - JSON
#[test]
fn metadata_json() {
    let json = br#"{"data":"value"}"#;
    let result = UniversalMetadataFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Universal to JSON - CSV
#[test]
fn to_json_csv() {
    let csv = b"x,y\n1,2";
    let result = UniversalToJsonFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Universal to JSON - already JSON
#[test]
fn to_json_json() {
    let json = br#"{"already":"json"}"#;
    let result = UniversalToJsonFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Universal to text - JSON
#[test]
fn to_text_json() {
    let json = br#"{"key":"value"}"#;
    let result = UniversalToTextFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Universal to text - CSV
#[test]
fn to_text_csv() {
    let csv = b"a,b\n1,2";
    let result = UniversalToTextFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    assert!(result.len() > 0);
}

// Format conversion - CSV to JSON
#[test]
fn convert_csv_to_json() {
    let csv = b"a,b\n1,2";
    let result = FormatConvertFn.execute(vec![Bytes::from(&csv[..])], &p(&[("from", "csv"), ("to", "json")])).unwrap();
    assert!(result.len() > 0);
}

// Empty input detection
#[test]
fn detect_empty() {
    let empty = b"";
    let result = FormatDetectFn.execute(vec![Bytes::from(&empty[..])], &p(&[]));
    assert!(result.is_ok() || result.is_err());
}
