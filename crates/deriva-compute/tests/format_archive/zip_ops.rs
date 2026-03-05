use bytes::Bytes;
use deriva_compute::function::{ComputeFunction, ComputeError};
use deriva_compute::builtins_format_archive::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- ZipCreateFn (#273) ----
#[test]
fn zip_create_single() {
    let r = ZipCreateFn.execute(vec![Bytes::from("hello")], &p(&[("files", "[\"test.txt\"]")])).unwrap();
    assert_eq!(&r[..2], b"PK"); // zip magic
}
#[test]
fn zip_create_multiple() {
    let r = ZipCreateFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &p(&[("files", "[\"a.txt\",\"b.txt\"]")])).unwrap();
    let list = ZipListFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&list).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}
#[test]
fn zip_create_stored() {
    let r = ZipCreateFn.execute(vec![Bytes::from("data")], &p(&[("files", "[\"f\"]"), ("compression", "stored")])).unwrap();
    assert!(!r.is_empty());
}
#[test]
fn zip_create_extract_roundtrip() {
    let data = b"zip roundtrip test";
    let zip = ZipCreateFn.execute(vec![Bytes::from(&data[..])], &p(&[("files", "[\"test.txt\"]")])).unwrap();
    let extracted = ZipExtractFn.execute(vec![zip], &p(&[("path", "test.txt")])).unwrap();
    assert_eq!(extracted.as_ref(), data);
}
#[test]
fn zip_create_default_names() {
    let r = ZipCreateFn.execute(vec![Bytes::from("x")], &BTreeMap::new()).unwrap();
    let list = ZipListFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&list).unwrap();
    assert!(v[0]["path"].as_str().unwrap().contains("file_0"));
}

// ---- ZipExtractFn (#274) ----
#[test]
fn zip_extract_all_manifest() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &p(&[("files", "[\"x\",\"y\"]")])).unwrap();
    let r = ZipExtractFn.execute(vec![zip], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["x"].is_object());
    assert!(v["y"].is_object());
}
#[test]
fn zip_extract_single_by_path() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("aaa"), Bytes::from("bbb")], &p(&[("files", "[\"a\",\"b\"]")])).unwrap();
    let r = ZipExtractFn.execute(vec![zip], &p(&[("path", "b")])).unwrap();
    assert_eq!(r.as_ref(), b"bbb");
}
#[test]
fn zip_extract_has_size() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("12345")], &p(&[("files", "[\"f\"]")])).unwrap();
    let r = ZipExtractFn.execute(vec![zip], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["f"]["size"], 5);
}
#[test]
fn zip_extract_invalid_zip_error() {
    let r = ZipExtractFn.execute(vec![Bytes::from("not a zip")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn zip_extract_binary_content() {
    let data = vec![0u8, 1, 2, 255, 254];
    let zip = ZipCreateFn.execute(vec![Bytes::from(data.clone())], &p(&[("files", "[\"bin\"]")])).unwrap();
    let r = ZipExtractFn.execute(vec![zip], &p(&[("path", "bin")])).unwrap();
    assert_eq!(r.as_ref(), &data[..]);
}

// ---- ZipListFn (#275) ----
#[test]
fn zip_list_entries() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &p(&[("files", "[\"x\",\"y\"]")])).unwrap();
    let r = ZipListFn.execute(vec![zip], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}
#[test]
fn zip_list_has_path_and_size() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("hello")], &p(&[("files", "[\"test.txt\"]")])).unwrap();
    let r = ZipListFn.execute(vec![zip], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v[0]["path"], "test.txt");
    assert_eq!(v[0]["size"], 5);
}
#[test]
fn zip_list_has_compressed_size() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("data")], &p(&[("files", "[\"f\"]")])).unwrap();
    let r = ZipListFn.execute(vec![zip], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v[0]["compressed_size"].as_u64().is_some());
}
#[test]
fn zip_list_invalid_zip_error() {
    let r = ZipListFn.execute(vec![Bytes::from("bad")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn zip_list_multiple_sizes() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("ab"), Bytes::from("cdef")], &p(&[("files", "[\"s\",\"l\"]")])).unwrap();
    let r = ZipListFn.execute(vec![zip], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    let sizes: Vec<u64> = v.as_array().unwrap().iter().map(|e| e["size"].as_u64().unwrap()).collect();
    assert!(sizes.contains(&2) && sizes.contains(&4));
}

// ---- ZipExtractSingleFn (#276) ----
#[test]
fn zip_extract_single_correct_content() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("aaa"), Bytes::from("bbb")], &p(&[("files", "[\"a\",\"b\"]")])).unwrap();
    let r = ZipExtractSingleFn.execute(vec![zip], &p(&[("path", "a")])).unwrap();
    assert_eq!(r.as_ref(), b"aaa");
}
#[test]
fn zip_extract_single_missing_path_error() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("x")], &p(&[("files", "[\"f\"]")])).unwrap();
    let r = ZipExtractSingleFn.execute(vec![zip], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn zip_extract_single_nonexistent_entry_error() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("x")], &p(&[("files", "[\"f\"]")])).unwrap();
    let r = ZipExtractSingleFn.execute(vec![zip], &p(&[("path", "nope")]));
    assert!(r.is_err());
}
#[test]
fn zip_extract_single_binary() {
    let data = vec![0u8, 128, 255];
    let zip = ZipCreateFn.execute(vec![Bytes::from(data.clone())], &p(&[("files", "[\"bin\"]")])).unwrap();
    let r = ZipExtractSingleFn.execute(vec![zip], &p(&[("path", "bin")])).unwrap();
    assert_eq!(r.as_ref(), &data[..]);
}
#[test]
fn zip_extract_single_empty_file() {
    let zip = ZipCreateFn.execute(vec![Bytes::from("")], &p(&[("files", "[\"empty\"]")])).unwrap();
    let r = ZipExtractSingleFn.execute(vec![zip], &p(&[("path", "empty")])).unwrap();
    assert!(r.is_empty());
}

// ---- ZipCreateFn stored compression roundtrip ----
#[test]
fn zip_stored_roundtrip() {
    let data = b"stored data";
    let zip = ZipCreateFn.execute(vec![Bytes::from(&data[..])], &p(&[("files", "[\"f\"]"), ("compression", "stored")])).unwrap();
    let r = ZipExtractSingleFn.execute(vec![zip], &p(&[("path", "f")])).unwrap();
    assert_eq!(r.as_ref(), data);
}
#[test]
fn zip_create_large_file() {
    let data = Bytes::from(vec![b'Z'; 50_000]);
    let zip = ZipCreateFn.execute(vec![data.clone()], &p(&[("files", "[\"big\"]")])).unwrap();
    let r = ZipExtractSingleFn.execute(vec![zip], &p(&[("path", "big")])).unwrap();
    assert_eq!(r, data);
}
#[test]
fn zip_extract_no_input_error() {
    let r = ZipExtractFn.execute(vec![], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn zip_list_no_input_error() {
    let r = ZipListFn.execute(vec![], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn zip_extract_single_no_input_error() {
    let r = ZipExtractSingleFn.execute(vec![], &p(&[("path", "f")]));
    assert!(r.is_err());
}
