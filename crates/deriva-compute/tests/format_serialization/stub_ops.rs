use bytes::Bytes;
use deriva_compute::builtins_format_serialization::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- thrift_decode stub (5 tests) ----
#[test]
fn thrift_decode_returns_error() {
    let res = ThriftDecodeFn.execute(vec![Bytes::from_static(b"data")], &p(&[]));
    assert!(res.is_err());
}

#[test]
fn thrift_decode_error_mentions_not_implemented() {
    let err = ThriftDecodeFn.execute(vec![Bytes::from_static(b"data")], &p(&[])).unwrap_err();
    assert!(err.to_string().contains("not yet implemented"));
}

#[test]
fn thrift_decode_binary_protocol() {
    assert!(ThriftDecodeFn.execute(vec![Bytes::from_static(b"x")], &p(&[("protocol", "binary")])).is_err());
}

#[test]
fn thrift_decode_compact_protocol() {
    assert!(ThriftDecodeFn.execute(vec![Bytes::from_static(b"x")], &p(&[("protocol", "compact")])).is_err());
}

#[test]
fn thrift_decode_id() {
    assert_eq!(ThriftDecodeFn.id().name, "thrift_decode");
}

// ---- thrift_encode stub (5 tests) ----
#[test]
fn thrift_encode_returns_error() {
    let res = ThriftEncodeFn.execute(vec![Bytes::from_static(b"{}")], &p(&[]));
    assert!(res.is_err());
}

#[test]
fn thrift_encode_error_mentions_not_implemented() {
    let err = ThriftEncodeFn.execute(vec![Bytes::from_static(b"{}")], &p(&[])).unwrap_err();
    assert!(err.to_string().contains("not yet implemented"));
}

#[test]
fn thrift_encode_binary_protocol() {
    assert!(ThriftEncodeFn.execute(vec![Bytes::from_static(b"{}")], &p(&[("protocol", "binary")])).is_err());
}

#[test]
fn thrift_encode_compact_protocol() {
    assert!(ThriftEncodeFn.execute(vec![Bytes::from_static(b"{}")], &p(&[("protocol", "compact")])).is_err());
}

#[test]
fn thrift_encode_id() {
    assert_eq!(ThriftEncodeFn.id().name, "thrift_encode");
}

// ---- avro_to_parquet stub (5 tests) ----
#[test]
fn avro_to_parquet_returns_error() {
    assert!(AvroToParquetFn.execute(vec![Bytes::from_static(b"data")], &p(&[])).is_err());
}

#[test]
fn avro_to_parquet_mentions_phase3() {
    let err = AvroToParquetFn.execute(vec![Bytes::from_static(b"data")], &p(&[])).unwrap_err();
    assert!(err.to_string().contains("Phase 3"));
}

#[test]
fn avro_to_parquet_empty_input() {
    assert!(AvroToParquetFn.execute(vec![Bytes::new()], &p(&[])).is_err());
}

#[test]
fn avro_to_parquet_no_input() {
    assert!(AvroToParquetFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn avro_to_parquet_id() {
    assert_eq!(AvroToParquetFn.id().name, "avro_to_parquet");
}

// ---- parquet_to_avro stub (5 tests) ----
#[test]
fn parquet_to_avro_returns_error() {
    assert!(ParquetToAvroFn.execute(vec![Bytes::from_static(b"data")], &p(&[])).is_err());
}

#[test]
fn parquet_to_avro_mentions_phase3() {
    let err = ParquetToAvroFn.execute(vec![Bytes::from_static(b"data")], &p(&[])).unwrap_err();
    assert!(err.to_string().contains("Phase 3"));
}

#[test]
fn parquet_to_avro_empty_input() {
    assert!(ParquetToAvroFn.execute(vec![Bytes::new()], &p(&[])).is_err());
}

#[test]
fn parquet_to_avro_no_input() {
    assert!(ParquetToAvroFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn parquet_to_avro_id() {
    assert_eq!(ParquetToAvroFn.id().name, "parquet_to_avro");
}
