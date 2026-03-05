use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_archive::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- GzipCompressFn (#267) / GzipDecompressFn (#268) ----
#[test]
fn gzip_roundtrip() {
    let data = b"hello world gzip test";
    let compressed = GzipCompressFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let decompressed = GzipDecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert_eq!(decompressed.as_ref(), data);
}
#[test]
fn gzip_magic_bytes() {
    let compressed = GzipCompressFn.execute(vec![Bytes::from("test")], &BTreeMap::new()).unwrap();
    assert_eq!(&compressed[..2], &[0x1f, 0x8b]);
}
#[test]
fn gzip_custom_level() {
    let data = Bytes::from(vec![b'A'; 1000]);
    let fast = GzipCompressFn.execute(vec![data.clone()], &p(&[("level", "1")])).unwrap();
    let best = GzipCompressFn.execute(vec![data], &p(&[("level", "9")])).unwrap();
    assert!(best.len() <= fast.len());
}
#[test]
fn gzip_invalid_level_error() {
    let r = GzipCompressFn.execute(vec![Bytes::from("x")], &p(&[("level", "10")]));
    assert!(r.is_err());
}
#[test]
fn gzip_decompress_invalid_error() {
    let r = GzipDecompressFn.execute(vec![Bytes::from("not gzip")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- Bzip2CompressFn (#269) / Bzip2DecompressFn (#270) ----
#[test]
fn bzip2_roundtrip() {
    let data = b"bzip2 test data here";
    let compressed = Bzip2CompressFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let decompressed = Bzip2DecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert_eq!(decompressed.as_ref(), data);
}
#[test]
fn bzip2_magic_bytes() {
    let compressed = Bzip2CompressFn.execute(vec![Bytes::from("test")], &BTreeMap::new()).unwrap();
    assert_eq!(&compressed[..2], b"BZ");
}
#[test]
fn bzip2_custom_level() {
    let data = Bytes::from(vec![b'B'; 1000]);
    let _ = Bzip2CompressFn.execute(vec![data], &p(&[("level", "1")])).unwrap();
}
#[test]
fn bzip2_invalid_level_error() {
    let r = Bzip2CompressFn.execute(vec![Bytes::from("x")], &p(&[("level", "0")]));
    assert!(r.is_err());
}
#[test]
fn bzip2_decompress_invalid_error() {
    let r = Bzip2DecompressFn.execute(vec![Bytes::from("not bzip2")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- XzCompressFn (#271) / XzDecompressFn (#272) ----
#[test]
fn xz_roundtrip() {
    let data = b"xz compression test";
    let compressed = XzCompressFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let decompressed = XzDecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert_eq!(decompressed.as_ref(), data);
}
#[test]
fn xz_magic_bytes() {
    let compressed = XzCompressFn.execute(vec![Bytes::from("test")], &BTreeMap::new()).unwrap();
    assert_eq!(&compressed[..6], &[0xFD, b'7', b'z', b'X', b'Z', 0x00]);
}
#[test]
fn xz_level_zero_allowed() {
    let _ = XzCompressFn.execute(vec![Bytes::from("data")], &p(&[("level", "0")])).unwrap();
}
#[test]
fn xz_invalid_level_error() {
    let r = XzCompressFn.execute(vec![Bytes::from("x")], &p(&[("level", "10")]));
    assert!(r.is_err());
}
#[test]
fn xz_decompress_invalid_error() {
    let r = XzDecompressFn.execute(vec![Bytes::from("not xz")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- ZstdFrameCompressFn (#279) / ZstdFrameDecompressFn (#280) ----
#[test]
fn zstd_roundtrip() {
    let data = b"zstd frame compression test";
    let compressed = ZstdFrameCompressFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let decompressed = ZstdFrameDecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert_eq!(decompressed.as_ref(), data);
}
#[test]
fn zstd_magic_bytes() {
    let compressed = ZstdFrameCompressFn.execute(vec![Bytes::from("test")], &BTreeMap::new()).unwrap();
    assert_eq!(&compressed[..4], &[0x28, 0xB5, 0x2F, 0xFD]); // zstd magic
}
#[test]
fn zstd_custom_level() {
    let _ = ZstdFrameCompressFn.execute(vec![Bytes::from("data")], &p(&[("level", "1")])).unwrap();
    let _ = ZstdFrameCompressFn.execute(vec![Bytes::from("data")], &p(&[("level", "22")])).unwrap();
}
#[test]
fn zstd_invalid_level_error() {
    let r = ZstdFrameCompressFn.execute(vec![Bytes::from("x")], &p(&[("level", "23")]));
    assert!(r.is_err());
}
#[test]
fn zstd_decompress_invalid_error() {
    let r = ZstdFrameDecompressFn.execute(vec![Bytes::from("not zstd")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- Additional dedicated tests for individual compress/decompress functions ----

// GzipDecompressFn dedicated
#[test]
fn gzip_decompress_empty_content() {
    let compressed = GzipCompressFn.execute(vec![Bytes::from("")], &BTreeMap::new()).unwrap();
    let r = GzipDecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}

// Bzip2DecompressFn dedicated
#[test]
fn bzip2_decompress_empty_content() {
    let compressed = Bzip2CompressFn.execute(vec![Bytes::from("")], &BTreeMap::new()).unwrap();
    let r = Bzip2DecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}

// XzDecompressFn dedicated
#[test]
fn xz_decompress_empty_content() {
    let compressed = XzCompressFn.execute(vec![Bytes::from("")], &BTreeMap::new()).unwrap();
    let r = XzDecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}

// ZstdFrameDecompressFn dedicated
#[test]
fn zstd_decompress_empty_content() {
    let compressed = ZstdFrameCompressFn.execute(vec![Bytes::from("")], &BTreeMap::new()).unwrap();
    let r = ZstdFrameDecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}

// Gzip large data
#[test]
fn gzip_large_data_roundtrip() {
    let data = Bytes::from(vec![b'X'; 100_000]);
    let compressed = GzipCompressFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    assert!(compressed.len() < data.len());
    let decompressed = GzipDecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert_eq!(decompressed, data);
}

// Bzip2 large data
#[test]
fn bzip2_large_data_roundtrip() {
    let data = Bytes::from(vec![b'Y'; 100_000]);
    let compressed = Bzip2CompressFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    assert!(compressed.len() < data.len());
    let decompressed = Bzip2DecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert_eq!(decompressed, data);
}

// Xz large data
#[test]
fn xz_large_data_roundtrip() {
    let data = Bytes::from(vec![b'Z'; 100_000]);
    let compressed = XzCompressFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    assert!(compressed.len() < data.len());
    let decompressed = XzDecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert_eq!(decompressed, data);
}

// Zstd large data
#[test]
fn zstd_large_data_roundtrip() {
    let data = Bytes::from(vec![b'W'; 100_000]);
    let compressed = ZstdFrameCompressFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    assert!(compressed.len() < data.len());
    let decompressed = ZstdFrameDecompressFn.execute(vec![compressed], &BTreeMap::new()).unwrap();
    assert_eq!(decompressed, data);
}

// Gzip level 9 vs 1 on repetitive data
#[test]
fn bzip2_custom_level_9() {
    let data = Bytes::from(vec![b'A'; 5000]);
    let r = Bzip2CompressFn.execute(vec![data], &p(&[("level", "9")])).unwrap();
    assert!(!r.is_empty());
}

// Xz custom level
#[test]
fn xz_custom_level_3() {
    let data = Bytes::from(vec![b'A'; 5000]);
    let r = XzCompressFn.execute(vec![data], &p(&[("level", "3")])).unwrap();
    assert!(!r.is_empty());
}

// Zstd level 1 vs default
#[test]
fn zstd_level_1_faster() {
    let data = Bytes::from(vec![b'A'; 5000]);
    let _ = ZstdFrameCompressFn.execute(vec![data], &p(&[("level", "1")])).unwrap();
}

// Gzip no input error
#[test]
fn gzip_no_input_error() {
    let r = GzipCompressFn.execute(vec![], &BTreeMap::new());
    assert!(r.is_err());
}

// Bzip2 no input error
#[test]
fn bzip2_no_input_error() {
    let r = Bzip2CompressFn.execute(vec![], &BTreeMap::new());
    assert!(r.is_err());
}

// Xz no input error
#[test]
fn xz_no_input_error() {
    let r = XzCompressFn.execute(vec![], &BTreeMap::new());
    assert!(r.is_err());
}

// Zstd no input error
#[test]
fn zstd_no_input_error() {
    let r = ZstdFrameCompressFn.execute(vec![], &BTreeMap::new());
    assert!(r.is_err());
}
