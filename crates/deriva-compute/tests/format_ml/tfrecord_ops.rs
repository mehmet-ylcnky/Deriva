use bytes::Bytes;
use deriva_compute::builtins_format_ml::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- tfrecord_read (5 tests) ----
#[test]
fn tfrecord_read_basic() {
    let data = b"test";
    let len = data.len() as u64;
    let mut tfr = Vec::new();
    tfr.extend_from_slice(&len.to_le_bytes());
    tfr.extend_from_slice(&[0u8; 4]);
    tfr.extend_from_slice(data);
    tfr.extend_from_slice(&[0u8; 4]);
    let out = TfRecordReadFn.execute(vec![Bytes::from(tfr)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"data\""));
}

#[test]
fn tfrecord_read_empty() {
    let out = TfRecordReadFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn tfrecord_read_truncated() {
    let out = TfRecordReadFn.execute(vec![Bytes::from(vec![1, 2, 3])], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn tfrecord_read_no_input() {
    assert!(TfRecordReadFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn tfrecord_read_id() {
    assert_eq!(TfRecordReadFn.id().name, "tfrecord_read");
}

// ---- tfrecord_write (5 tests) ----
#[test]
fn tfrecord_write_basic() {
    let data = b"test";
    let out = TfRecordWriteFn.execute(vec![Bytes::from(&data[..])], &p(&[])).unwrap();
    assert_eq!(out.len(), 8 + 4 + 4 + 4);
}

#[test]
fn tfrecord_write_roundtrip() {
    let data = b"hello";
    let tfr = TfRecordWriteFn.execute(vec![Bytes::from(&data[..])], &p(&[])).unwrap();
    let out = TfRecordReadFn.execute(vec![tfr], &p(&[])).unwrap();
    assert!(out.len() > 0);
}

#[test]
fn tfrecord_write_empty() {
    let out = TfRecordWriteFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 16);
}

#[test]
fn tfrecord_write_no_input() {
    assert!(TfRecordWriteFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn tfrecord_write_id() {
    assert_eq!(TfRecordWriteFn.id().name, "tfrecord_write");
}
