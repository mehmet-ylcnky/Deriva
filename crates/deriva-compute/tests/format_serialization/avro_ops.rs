use bytes::Bytes;
use deriva_compute::builtins_format_serialization::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

fn make_avro_ocf(schema_json: &str, records: &[serde_json::Value]) -> Vec<u8> {
    let schema = apache_avro::Schema::parse_str(schema_json).unwrap();
    let mut writer = apache_avro::Writer::new(&schema, Vec::new());
    for rec in records {
        let avro_val = apache_avro::types::Value::from(rec.clone()).resolve(&schema).unwrap();
        writer.append(avro_val).unwrap();
    }
    writer.into_inner().unwrap()
}

const SIMPLE_SCHEMA: &str = r#"{"type":"record","name":"User","fields":[{"name":"name","type":"string"},{"name":"age","type":"int"}]}"#;

fn user_records() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"name": "Alice", "age": 30}),
        serde_json::json!({"name": "Bob", "age": 25}),
    ]
}

// ---- avro_read (5 tests) ----
#[test]
fn avro_read_two_records() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &user_records());
    let out = AvroReadFn.execute(vec![Bytes::from(ocf)], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["name"], "Alice");
}

#[test]
fn avro_read_empty_file() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &[]);
    let out = AvroReadFn.execute(vec![Bytes::from(ocf)], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
    assert!(arr.is_empty());
}

#[test]
fn avro_read_invalid_bytes() {
    let res = AvroReadFn.execute(vec![Bytes::from_static(b"not avro")], &p(&[]));
    assert!(res.is_err());
}

#[test]
fn avro_read_single_record() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &[serde_json::json!({"name":"Zoe","age":1})]);
    let out = AvroReadFn.execute(vec![Bytes::from(ocf)], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["age"], 1);
}

#[test]
fn avro_read_no_input() {
    assert!(AvroReadFn.execute(vec![], &p(&[])).is_err());
}

// ---- avro_write (5 tests) ----
#[test]
fn avro_write_roundtrip() {
    let json = serde_json::to_vec(&user_records()).unwrap();
    let ocf = AvroWriteFn.execute(
        vec![Bytes::from(json), Bytes::from(SIMPLE_SCHEMA)],
        &p(&[]),
    ).unwrap();
    let back = AvroReadFn.execute(vec![ocf], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&back).unwrap();
    assert_eq!(arr.len(), 2);
}

#[test]
fn avro_write_deflate_codec() {
    let json = serde_json::to_vec(&user_records()).unwrap();
    let ocf = AvroWriteFn.execute(
        vec![Bytes::from(json), Bytes::from(SIMPLE_SCHEMA)],
        &p(&[("codec", "deflate")]),
    ).unwrap();
    let back = AvroReadFn.execute(vec![ocf], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&back).unwrap();
    assert_eq!(arr.len(), 2);
}

#[test]
fn avro_write_empty_array() {
    let json = serde_json::to_vec(&Vec::<serde_json::Value>::new()).unwrap();
    let ocf = AvroWriteFn.execute(
        vec![Bytes::from(json), Bytes::from(SIMPLE_SCHEMA)],
        &p(&[]),
    ).unwrap();
    let back = AvroReadFn.execute(vec![ocf], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&back).unwrap();
    assert!(arr.is_empty());
}

#[test]
fn avro_write_bad_schema() {
    let json = serde_json::to_vec(&user_records()).unwrap();
    let res = AvroWriteFn.execute(
        vec![Bytes::from(json), Bytes::from_static(b"not json")],
        &p(&[]),
    );
    assert!(res.is_err());
}

#[test]
fn avro_write_needs_two_inputs() {
    let json = serde_json::to_vec(&user_records()).unwrap();
    assert!(AvroWriteFn.execute(vec![Bytes::from(json)], &p(&[])).is_err());
}

// ---- avro_schema_extract (5 tests) ----
#[test]
fn avro_schema_extract_basic() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &user_records());
    let out = AvroSchemaExtractFn.execute(vec![Bytes::from(ocf)], &p(&[])).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(schema["name"], "User");
}

#[test]
fn avro_schema_extract_has_fields() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &user_records());
    let out = AvroSchemaExtractFn.execute(vec![Bytes::from(ocf)], &p(&[])).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(schema["fields"].is_array());
}

#[test]
fn avro_schema_extract_empty_data() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &[]);
    let out = AvroSchemaExtractFn.execute(vec![Bytes::from(ocf)], &p(&[])).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(schema["type"], "record");
}

#[test]
fn avro_schema_extract_invalid() {
    assert!(AvroSchemaExtractFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn avro_schema_extract_no_input() {
    assert!(AvroSchemaExtractFn.execute(vec![], &p(&[])).is_err());
}

// ---- avro_schema_evolve (5 tests) ----
const EVOLVED_SCHEMA: &str = r#"{"type":"record","name":"User","fields":[{"name":"name","type":"string"},{"name":"age","type":"int"},{"name":"email","type":["null","string"],"default":null}]}"#;

#[test]
fn avro_schema_evolve_adds_nullable_field() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &user_records());
    let out = AvroSchemaEvolveFn.execute(
        vec![Bytes::from(ocf), Bytes::from(EVOLVED_SCHEMA)],
        &p(&[]),
    ).unwrap();
    let back = AvroReadFn.execute(vec![out], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&back).unwrap();
    assert_eq!(arr.len(), 2);
    assert!(arr[0].get("email").is_some());
}

#[test]
fn avro_schema_evolve_preserves_data() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &user_records());
    let out = AvroSchemaEvolveFn.execute(
        vec![Bytes::from(ocf), Bytes::from(EVOLVED_SCHEMA)],
        &p(&[]),
    ).unwrap();
    let back = AvroReadFn.execute(vec![out], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&back).unwrap();
    assert_eq!(arr[0]["name"], "Alice");
    assert_eq!(arr[1]["age"], 25);
}

#[test]
fn avro_schema_evolve_empty_data() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &[]);
    let out = AvroSchemaEvolveFn.execute(
        vec![Bytes::from(ocf), Bytes::from(EVOLVED_SCHEMA)],
        &p(&[]),
    ).unwrap();
    let back = AvroReadFn.execute(vec![out], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&back).unwrap();
    assert!(arr.is_empty());
}

#[test]
fn avro_schema_evolve_bad_schema() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &user_records());
    assert!(AvroSchemaEvolveFn.execute(
        vec![Bytes::from(ocf), Bytes::from_static(b"bad")],
        &p(&[]),
    ).is_err());
}

#[test]
fn avro_schema_evolve_needs_two_inputs() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &user_records());
    assert!(AvroSchemaEvolveFn.execute(vec![Bytes::from(ocf)], &p(&[])).is_err());
}

// ---- avro_to_json (5 tests) ----
#[test]
fn avro_to_json_ndjson_output() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &user_records());
    let out = AvroToJsonFn.execute(vec![Bytes::from(ocf)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["name"], "Alice");
}

#[test]
fn avro_to_json_empty() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &[]);
    let out = AvroToJsonFn.execute(vec![Bytes::from(ocf)], &p(&[])).unwrap();
    assert!(out.is_empty());
}

#[test]
fn avro_to_json_single_record() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &[serde_json::json!({"name":"X","age":99})]);
    let out = AvroToJsonFn.execute(vec![Bytes::from(ocf)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert_eq!(text.lines().count(), 1);
}

#[test]
fn avro_to_json_invalid() {
    assert!(AvroToJsonFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn avro_to_json_each_line_valid_json() {
    let ocf = make_avro_ocf(SIMPLE_SCHEMA, &user_records());
    let out = AvroToJsonFn.execute(vec![Bytes::from(ocf)], &p(&[])).unwrap();
    for line in std::str::from_utf8(&out).unwrap().lines() {
        assert!(serde_json::from_str::<serde_json::Value>(line).is_ok());
    }
}

// ---- json_to_avro (5 tests) ----
#[test]
fn json_to_avro_roundtrip() {
    let ndjson = "{\"name\":\"Alice\",\"age\":30}\n{\"name\":\"Bob\",\"age\":25}";
    let ocf = JsonToAvroFn.execute(
        vec![Bytes::from(ndjson), Bytes::from(SIMPLE_SCHEMA)],
        &p(&[]),
    ).unwrap();
    let back = AvroReadFn.execute(vec![ocf], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&back).unwrap();
    assert_eq!(arr.len(), 2);
}

#[test]
fn json_to_avro_single_line() {
    let ndjson = "{\"name\":\"Zoe\",\"age\":1}";
    let ocf = JsonToAvroFn.execute(
        vec![Bytes::from(ndjson), Bytes::from(SIMPLE_SCHEMA)],
        &p(&[]),
    ).unwrap();
    let back = AvroReadFn.execute(vec![ocf], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&back).unwrap();
    assert_eq!(arr.len(), 1);
}

#[test]
fn json_to_avro_empty_lines_skipped() {
    let ndjson = "{\"name\":\"A\",\"age\":1}\n\n{\"name\":\"B\",\"age\":2}\n";
    let ocf = JsonToAvroFn.execute(
        vec![Bytes::from(ndjson), Bytes::from(SIMPLE_SCHEMA)],
        &p(&[]),
    ).unwrap();
    let back = AvroReadFn.execute(vec![ocf], &p(&[])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&back).unwrap();
    assert_eq!(arr.len(), 2);
}

#[test]
fn json_to_avro_bad_json() {
    assert!(JsonToAvroFn.execute(
        vec![Bytes::from_static(b"not json"), Bytes::from(SIMPLE_SCHEMA)],
        &p(&[]),
    ).is_err());
}

#[test]
fn json_to_avro_needs_two_inputs() {
    assert!(JsonToAvroFn.execute(vec![Bytes::from_static(b"{}")], &p(&[])).is_err());
}
