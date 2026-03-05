use bytes::Bytes;
use deriva_compute::function::{ComputeFunction, ComputeError};
use deriva_compute::builtins_format_archive::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- TarCreateFn (#263) ----
#[test]
fn tar_create_single_file() {
    let r = TarCreateFn.execute(vec![Bytes::from("hello")], &p(&[("files", "[\"test.txt\"]")])).unwrap();
    assert!(r.len() > 512); // tar header is 512 bytes
}
#[test]
fn tar_create_multiple_files() {
    let r = TarCreateFn.execute(
        vec![Bytes::from("aaa"), Bytes::from("bbb")],
        &p(&[("files", "[\"a.txt\",\"b.txt\"]")])
    ).unwrap();
    // List to verify both entries
    let list = TarListFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&list).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}
#[test]
fn tar_create_default_names() {
    let r = TarCreateFn.execute(vec![Bytes::from("data")], &BTreeMap::new()).unwrap();
    let list = TarListFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&list).unwrap();
    assert!(v[0]["path"].as_str().unwrap().contains("file_0"));
}
#[test]
fn tar_create_empty_content() {
    let r = TarCreateFn.execute(vec![Bytes::from("")], &p(&[("files", "[\"empty.txt\"]")])).unwrap();
    assert!(!r.is_empty());
}
#[test]
fn tar_create_extract_roundtrip() {
    let data = b"hello world";
    let tar = TarCreateFn.execute(vec![Bytes::from(&data[..])], &p(&[("files", "[\"test.txt\"]")])).unwrap();
    let extracted = TarExtractFn.execute(vec![tar], &p(&[("path", "test.txt")])).unwrap();
    assert_eq!(extracted.as_ref(), data);
}

// ---- TarExtractFn (#264) ----
#[test]
fn tar_extract_all_manifest() {
    let tar = TarCreateFn.execute(vec![Bytes::from("aaa"), Bytes::from("bbb")], &p(&[("files", "[\"a.txt\",\"b.txt\"]")])).unwrap();
    let r = TarExtractFn.execute(vec![tar], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["a.txt"].is_object());
    assert!(v["b.txt"].is_object());
}
#[test]
fn tar_extract_single_by_path() {
    let tar = TarCreateFn.execute(vec![Bytes::from("content_a"), Bytes::from("content_b")], &p(&[("files", "[\"a.txt\",\"b.txt\"]")])).unwrap();
    let r = TarExtractFn.execute(vec![tar], &p(&[("path", "b.txt")])).unwrap();
    assert_eq!(r.as_ref(), b"content_b");
}
#[test]
fn tar_extract_preserves_size() {
    let tar = TarCreateFn.execute(vec![Bytes::from("12345")], &p(&[("files", "[\"f.txt\"]")])).unwrap();
    let r = TarExtractFn.execute(vec![tar], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["f.txt"]["size"], 5);
}
#[test]
fn tar_extract_nonexistent_path() {
    let tar = TarCreateFn.execute(vec![Bytes::from("data")], &p(&[("files", "[\"a.txt\"]")])).unwrap();
    let r = TarExtractFn.execute(vec![tar], &p(&[("path", "nope.txt")])).unwrap();
    // Returns empty manifest (no match)
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v.as_object().unwrap().is_empty());
}
#[test]
fn tar_extract_binary_content() {
    let data = vec![0u8, 1, 2, 255, 254, 253];
    let tar = TarCreateFn.execute(vec![Bytes::from(data.clone())], &p(&[("files", "[\"bin\"]")])).unwrap();
    let r = TarExtractFn.execute(vec![tar], &p(&[("path", "bin")])).unwrap();
    assert_eq!(r.as_ref(), &data[..]);
}

// ---- TarListFn (#265) ----
#[test]
fn tar_list_entries() {
    let tar = TarCreateFn.execute(vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")], &p(&[("files", "[\"x\",\"y\",\"z\"]")])).unwrap();
    let r = TarListFn.execute(vec![tar], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 3);
}
#[test]
fn tar_list_has_path_and_size() {
    let tar = TarCreateFn.execute(vec![Bytes::from("hello")], &p(&[("files", "[\"test.txt\"]")])).unwrap();
    let r = TarListFn.execute(vec![tar], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v[0]["path"], "test.txt");
    assert_eq!(v[0]["size"], 5);
}
#[test]
fn tar_list_empty_archive() {
    let tar = TarCreateFn.execute(vec![], &BTreeMap::new()).unwrap();
    let r = TarListFn.execute(vec![tar], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v.as_array().unwrap().is_empty());
}
#[test]
fn tar_list_has_mode() {
    let tar = TarCreateFn.execute(vec![Bytes::from("x")], &p(&[("files", "[\"f\"]")])).unwrap();
    let r = TarListFn.execute(vec![tar], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v[0]["mode"].as_u64().is_some());
}
#[test]
fn tar_list_multiple_sizes() {
    let tar = TarCreateFn.execute(vec![Bytes::from("ab"), Bytes::from("cdef")], &p(&[("files", "[\"s\",\"l\"]")])).unwrap();
    let r = TarListFn.execute(vec![tar], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v[0]["size"], 2);
    assert_eq!(v[1]["size"], 4);
}

// ---- TarAppendFn (#266) ----
#[test]
fn tar_append_adds_entry() {
    let tar = TarCreateFn.execute(vec![Bytes::from("orig")], &p(&[("files", "[\"a.txt\"]")])).unwrap();
    let appended = TarAppendFn.execute(vec![tar, Bytes::from("new")], &p(&[("filename", "b.txt")])).unwrap();
    let list = TarListFn.execute(vec![appended], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&list).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}
#[test]
fn tar_append_preserves_original() {
    let tar = TarCreateFn.execute(vec![Bytes::from("orig_data")], &p(&[("files", "[\"a.txt\"]")])).unwrap();
    let appended = TarAppendFn.execute(vec![tar, Bytes::from("new_data")], &p(&[("filename", "b.txt")])).unwrap();
    let r = TarExtractFn.execute(vec![appended], &p(&[("path", "a.txt")])).unwrap();
    assert_eq!(r.as_ref(), b"orig_data");
}
#[test]
fn tar_append_new_content_correct() {
    let tar = TarCreateFn.execute(vec![Bytes::from("x")], &p(&[("files", "[\"a\"]")])).unwrap();
    let appended = TarAppendFn.execute(vec![tar, Bytes::from("new_content")], &p(&[("filename", "b")])).unwrap();
    let r = TarExtractFn.execute(vec![appended], &p(&[("path", "b")])).unwrap();
    assert_eq!(r.as_ref(), b"new_content");
}
#[test]
fn tar_append_requires_two_inputs() {
    let r = TarAppendFn.execute(vec![Bytes::from("x")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { .. })));
}
#[test]
fn tar_append_default_filename() {
    let tar = TarCreateFn.execute(vec![Bytes::from("x")], &p(&[("files", "[\"a\"]")])).unwrap();
    let appended = TarAppendFn.execute(vec![tar, Bytes::from("y")], &BTreeMap::new()).unwrap();
    let list = TarListFn.execute(vec![appended], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&list).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}

// ---- TarGzCreateFn (#277) ----
#[test]
fn tar_gz_create_produces_gzip() {
    let r = TarGzCreateFn.execute(vec![Bytes::from("hello")], &p(&[("files", "[\"f.txt\"]")])).unwrap();
    assert_eq!(&r[..2], &[0x1f, 0x8b]); // gzip magic
}
#[test]
fn tar_gz_create_extract_roundtrip() {
    let data = b"test content";
    let tgz = TarGzCreateFn.execute(vec![Bytes::from(&data[..])], &p(&[("files", "[\"test.txt\"]")])).unwrap();
    let extracted = TarGzExtractFn.execute(vec![tgz], &p(&[("path", "test.txt")])).unwrap();
    assert_eq!(extracted.as_ref(), data);
}
#[test]
fn tar_gz_extract_manifest() {
    let tgz = TarGzCreateFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &p(&[("files", "[\"x\",\"y\"]")])).unwrap();
    let r = TarGzExtractFn.execute(vec![tgz], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["x"].is_object());
    assert!(v["y"].is_object());
}
#[test]
fn tar_gz_smaller_than_tar() {
    let big = Bytes::from(vec![b'A'; 10000]);
    let tar = TarCreateFn.execute(vec![big.clone()], &p(&[("files", "[\"f\"]")])).unwrap();
    let tgz = TarGzCreateFn.execute(vec![big], &p(&[("files", "[\"f\"]")])).unwrap();
    assert!(tgz.len() < tar.len());
}
#[test]
fn tar_gz_invalid_decompress_error() {
    let r = TarGzExtractFn.execute(vec![Bytes::from("not gzip")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- TarGzExtractFn (#278) — additional dedicated tests ----
#[test]
fn tar_gz_extract_all_entries() {
    let tgz = TarGzCreateFn.execute(vec![Bytes::from("aa"), Bytes::from("bb")], &p(&[("files", "[\"a\",\"b\"]")])).unwrap();
    let r = TarGzExtractFn.execute(vec![tgz], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v.as_object().unwrap().len(), 2);
}
#[test]
fn tar_gz_extract_single_entry() {
    let tgz = TarGzCreateFn.execute(vec![Bytes::from("content")], &p(&[("files", "[\"f.txt\"]")])).unwrap();
    let r = TarGzExtractFn.execute(vec![tgz], &p(&[("path", "f.txt")])).unwrap();
    assert_eq!(r.as_ref(), b"content");
}
#[test]
fn tar_gz_extract_preserves_size() {
    let tgz = TarGzCreateFn.execute(vec![Bytes::from("12345")], &p(&[("files", "[\"f\"]")])).unwrap();
    let r = TarGzExtractFn.execute(vec![tgz], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["f"]["size"], 5);
}
#[test]
fn tar_gz_create_custom_level() {
    let r = TarGzCreateFn.execute(vec![Bytes::from("data")], &p(&[("files", "[\"f\"]"), ("level", "1")])).unwrap();
    assert_eq!(&r[..2], &[0x1f, 0x8b]);
}
#[test]
fn tar_gz_create_empty() {
    let r = TarGzCreateFn.execute(vec![], &BTreeMap::new()).unwrap();
    assert_eq!(&r[..2], &[0x1f, 0x8b]);
}
