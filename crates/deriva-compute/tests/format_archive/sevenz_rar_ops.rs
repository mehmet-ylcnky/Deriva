use bytes::Bytes;
use deriva_compute::function::{ComputeFunction, ComputeError};
use deriva_compute::builtins_format_archive::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- SevenZExtractFn (#281) ----
// We create a 7z archive in-memory using sevenz_rust::SevenZWriter, then extract
#[test]
fn sevenz_extract_roundtrip() {
    // Create a 7z archive with one file
    let tmp = std::env::temp_dir().join(format!("sevenz_test_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let archive_path = tmp.join("test.7z");
    {
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut writer = sevenz_rust::SevenZWriter::new(file).unwrap();
        let entry = { let mut e = sevenz_rust::SevenZArchiveEntry::new(); e.name = "hello.txt".to_string(); e };
        writer.push_archive_entry(entry, Some(std::io::Cursor::new(b"hello 7z"))).unwrap();
        writer.finish().unwrap();
    }
    let data = std::fs::read(&archive_path).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    let r = SevenZExtractFn.execute(vec![Bytes::from(data)], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v.as_object().unwrap().len() >= 1);
}
#[test]
fn sevenz_extract_content_correct() {
    let tmp = std::env::temp_dir().join(format!("sevenz_test2_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let archive_path = tmp.join("test.7z");
    {
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut writer = sevenz_rust::SevenZWriter::new(file).unwrap();
        let entry = { let mut e = sevenz_rust::SevenZArchiveEntry::new(); e.name = "data.txt".to_string(); e };
        writer.push_archive_entry(entry, Some(std::io::Cursor::new(b"test_content"))).unwrap();
        writer.finish().unwrap();
    }
    let data = std::fs::read(&archive_path).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    let r = SevenZExtractFn.execute(vec![Bytes::from(data)], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    // Check that content_b64 decodes to "test_content"
    let b64 = v["data.txt"]["content_b64"].as_str().unwrap();
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).unwrap();
    assert_eq!(decoded, b"test_content");
}
#[test]
fn sevenz_extract_multiple_files() {
    let tmp = std::env::temp_dir().join(format!("sevenz_test3_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let archive_path = tmp.join("test.7z");
    {
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut writer = sevenz_rust::SevenZWriter::new(file).unwrap();
        writer.push_archive_entry({ let mut e = sevenz_rust::SevenZArchiveEntry::new(); e.name = "a.txt".to_string(); e }, Some(std::io::Cursor::new(b"aaa"))).unwrap();
        writer.push_archive_entry({ let mut e = sevenz_rust::SevenZArchiveEntry::new(); e.name = "b.txt".to_string(); e }, Some(std::io::Cursor::new(b"bbb"))).unwrap();
        writer.finish().unwrap();
    }
    let data = std::fs::read(&archive_path).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    let r = SevenZExtractFn.execute(vec![Bytes::from(data)], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v.as_object().unwrap().len() >= 2);
}
#[test]
fn sevenz_extract_invalid_error() {
    let r = SevenZExtractFn.execute(vec![Bytes::from("not a 7z file")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn sevenz_extract_has_size() {
    let tmp = std::env::temp_dir().join(format!("sevenz_test4_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let archive_path = tmp.join("test.7z");
    {
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut writer = sevenz_rust::SevenZWriter::new(file).unwrap();
        writer.push_archive_entry({ let mut e = sevenz_rust::SevenZArchiveEntry::new(); e.name = "f.txt".to_string(); e }, Some(std::io::Cursor::new(b"12345"))).unwrap();
        writer.finish().unwrap();
    }
    let data = std::fs::read(&archive_path).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    let r = SevenZExtractFn.execute(vec![Bytes::from(data)], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["f.txt"]["size"], 5);
}

// ---- RarExtractFn (#282) ----
#[test]
fn rar_extract_not_rar_error() {
    let r = RarExtractFn.execute(vec![Bytes::from("not rar data")], &BTreeMap::new());
    assert!(r.is_err());
    let err = r.unwrap_err().to_string();
    assert!(err.contains("not a valid RAR"));
}
#[test]
fn rar_extract_rar_magic_returns_unsupported() {
    // RAR5 magic: Rar!\x1a\x07\x01\x00
    let mut data = vec![0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x01];
    data.extend_from_slice(&[0; 100]);
    let r = RarExtractFn.execute(vec![Bytes::from(data)], &BTreeMap::new());
    assert!(r.is_err());
    let err = r.unwrap_err().to_string();
    assert!(err.contains("native unrar"));
}
#[test]
fn rar_extract_empty_input_error() {
    let r = RarExtractFn.execute(vec![Bytes::from("")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn rar_extract_no_input_error() {
    let r = RarExtractFn.execute(vec![], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { .. })));
}
#[test]
fn rar_extract_short_data_error() {
    let r = RarExtractFn.execute(vec![Bytes::from("Rar")], &BTreeMap::new());
    assert!(r.is_err());
}
