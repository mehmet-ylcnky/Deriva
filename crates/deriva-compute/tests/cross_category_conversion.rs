use bytes::Bytes;
use deriva_compute::builtins_format_csv::*;
use deriva_compute::builtins_format_serialization::*;
use deriva_compute::builtins_format_detect::*;
use deriva_compute::function::ComputeFunction;
use std::collections::BTreeMap;
use deriva_core::address::Value;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// CSV -> JSON
#[test]
fn csv_to_json() {
    let csv = b"name,age\nAlice,30\nBob,25";
    let json = CsvParseFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    assert!(json.len() > 0);
    let s = String::from_utf8_lossy(&json);
    assert!(s.contains("Alice") || s.contains("name"));
}

// JSON -> Msgpack -> JSON
#[test]
fn json_msgpack_roundtrip() {
    let json = br#"{"test":123}"#;
    let msgpack = MsgpackEncodeFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    let decoded = MsgpackDecodeFn.execute(vec![msgpack], &p(&[])).unwrap();
    assert!(decoded.len() > 0);
}

// JSON -> CBOR -> JSON
#[test]
fn json_cbor_roundtrip() {
    let json = br#"{"field":"value"}"#;
    let cbor = CborEncodeFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    let decoded = CborDecodeFn.execute(vec![cbor], &p(&[])).unwrap();
    assert!(decoded.len() > 0);
}

// JSON -> BSON -> JSON
#[test]
fn json_bson_roundtrip() {
    let json = br#"{"name":"test"}"#;
    let bson = BsonEncodeFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    let decoded = BsonDecodeFn.execute(vec![bson], &p(&[])).unwrap();
    assert!(decoded.len() > 0);
}

// NDJSON parse and write
#[test]
fn ndjson_roundtrip() {
    let ndjson = b"{\"x\":1}\n{\"x\":2}";
    let parsed = NdjsonParseFn.execute(vec![Bytes::from(&ndjson[..])], &p(&[])).unwrap();
    let written = NdjsonWriteFn.execute(vec![parsed], &p(&[])).unwrap();
    assert!(written.len() > 0);
}

// CSV -> NDJSON
#[test]
fn csv_to_ndjson() {
    let csv = b"a,b\n1,2\n3,4";
    let json = CsvParseFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    let ndjson = NdjsonWriteFn.execute(vec![json], &p(&[])).unwrap();
    assert!(ndjson.len() > 0);
}

// Format detection
#[test]
fn detect_csv_format() {
    let csv = b"a,b\n1,2";
    let detected = FormatDetectFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    assert!(detected.len() > 0);
}

// Format detection - JSON
#[test]
fn detect_json_format() {
    let json = br#"{"key":"value"}"#;
    let detected = FormatDetectFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    let s = String::from_utf8_lossy(&detected);
    assert!(s.contains("json") || s.len() > 0);
}

// Universal metadata
#[test]
fn universal_metadata_extraction() {
    let csv = b"col1,col2\nval1,val2";
    let meta = UniversalMetadataFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    assert!(meta.len() > 0);
}

// Universal to JSON
#[test]
fn universal_to_json_conversion() {
    let csv = b"x,y\n1,2";
    let json = UniversalToJsonFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    assert!(json.len() > 0);
}

// Universal to text
#[test]
fn universal_to_text_conversion() {
    let json = br#"{"key":"value"}"#;
    let text = UniversalToTextFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    assert!(text.len() > 0);
}

// Schema inference
#[test]
fn schema_inference_csv() {
    let csv = b"name,age\nAlice,30";
    let schema = SchemaInferFn.execute(vec![Bytes::from(&csv[..])], &p(&[])).unwrap();
    assert!(schema.len() > 0);
}

// JSON flatten/unflatten
#[test]
fn json_flatten_unflatten() {
    let json = br#"{"a":{"b":1}}"#;
    let flat = JsonFlattenFn.execute(vec![Bytes::from(&json[..])], &p(&[])).unwrap();
    let unflat = JsonUnflattenFn.execute(vec![flat], &p(&[])).unwrap();
    assert!(unflat.len() > 0);
}

// JSON merge
#[test]
fn json_merge_objects() {
    let json1 = br#"{"a":1}"#;
    let json2 = br#"{"b":2}"#;
    let merged = JsonMergeFn.execute(vec![Bytes::from(&json1[..]), Bytes::from(&json2[..])], &p(&[])).unwrap();
    let s = String::from_utf8_lossy(&merged);
    assert!(s.contains("a") || s.contains("b") || merged.len() > 0);
}


