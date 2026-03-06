use bytes::Bytes;
use deriva_compute::builtins_format_image::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

fn png_10x10() -> Bytes {
    let img = image::DynamicImage::new_rgb8(10, 10);
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();
    Bytes::from(buf)
}

fn jpeg_20x15() -> Bytes {
    let img = image::DynamicImage::new_rgb8(20, 15);
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg).unwrap();
    Bytes::from(buf)
}

// ---- image_metadata (5 tests) ----
#[test]
fn image_metadata_png() {
    let out = ImageMetadataFn.execute(vec![png_10x10()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["width"], 10);
    assert_eq!(v["height"], 10);
    assert_eq!(v["format"], "png");
}

#[test]
fn image_metadata_jpeg() {
    let out = ImageMetadataFn.execute(vec![jpeg_20x15()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["width"], 20);
    assert_eq!(v["height"], 15);
    assert_eq!(v["format"], "jpeg");
}

#[test]
fn image_metadata_has_color_type() {
    let out = ImageMetadataFn.execute(vec![png_10x10()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["color_type"].is_string());
}

#[test]
fn image_metadata_invalid() {
    assert!(ImageMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn image_metadata_no_input() {
    assert!(ImageMetadataFn.execute(vec![], &p(&[])).is_err());
}

// ---- image_detect_format (5 tests) ----
#[test]
fn detect_format_png() {
    let out = ImageDetectFormatFn.execute(vec![png_10x10()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["format"], "png");
    assert_eq!(v["mime"], "image/png");
}

#[test]
fn detect_format_jpeg() {
    let out = ImageDetectFormatFn.execute(vec![jpeg_20x15()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["format"], "jpeg");
    assert_eq!(v["mime"], "image/jpeg");
}

#[test]
fn detect_format_invalid() {
    assert!(ImageDetectFormatFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn detect_format_no_input() {
    assert!(ImageDetectFormatFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn detect_format_id() {
    assert_eq!(ImageDetectFormatFn.id().name, "image_detect_format");
}

// ---- image_strip_metadata (5 tests) ----
#[test]
fn strip_metadata_png_roundtrip() {
    let png = png_10x10();
    let stripped = ImageStripMetadataFn.execute(vec![png], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![stripped], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 10);
}

#[test]
fn strip_metadata_jpeg() {
    let jpg = jpeg_20x15();
    let stripped = ImageStripMetadataFn.execute(vec![jpg], &p(&[])).unwrap();
    assert!(!stripped.is_empty());
}

#[test]
fn strip_metadata_preserves_format() {
    let jpg = jpeg_20x15();
    let stripped = ImageStripMetadataFn.execute(vec![jpg], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageDetectFormatFn.execute(vec![stripped], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["format"], "jpeg");
}

#[test]
fn strip_metadata_invalid() {
    assert!(ImageStripMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn strip_metadata_no_input() {
    assert!(ImageStripMetadataFn.execute(vec![], &p(&[])).is_err());
}

// ---- image_hash (5 tests) ----
#[test]
fn image_hash_ahash() {
    let out = ImageHashFn.execute(vec![png_10x10()], &p(&[("algorithm", "ahash")])).unwrap();
    let hex = std::str::from_utf8(&out).unwrap();
    assert_eq!(hex.len(), 16); // 64 bits = 16 hex chars
}

#[test]
fn image_hash_dhash() {
    let out = ImageHashFn.execute(vec![png_10x10()], &p(&[("algorithm", "dhash")])).unwrap();
    assert_eq!(out.len(), 16);
}

#[test]
fn image_hash_phash_default() {
    let out = ImageHashFn.execute(vec![png_10x10()], &p(&[])).unwrap();
    assert_eq!(out.len(), 16); // default is phash
}

#[test]
fn image_hash_same_image_same_hash() {
    let png = png_10x10();
    let h1 = ImageHashFn.execute(vec![png.clone()], &p(&[("algorithm", "ahash")])).unwrap();
    let h2 = ImageHashFn.execute(vec![png], &p(&[("algorithm", "ahash")])).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn image_hash_invalid_algorithm() {
    assert!(ImageHashFn.execute(vec![png_10x10()], &p(&[("algorithm", "bogus")])).is_err());
}
