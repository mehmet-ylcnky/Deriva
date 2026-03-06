use bytes::Bytes;
use deriva_compute::builtins_format_image::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

fn png_20x10() -> Bytes {
    let img = image::DynamicImage::new_rgb8(20, 10);
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();
    Bytes::from(buf)
}

fn png_100x80() -> Bytes {
    let img = image::DynamicImage::new_rgb8(100, 80);
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();
    Bytes::from(buf)
}

// ---- image_resize (5 tests) ----
#[test]
fn resize_basic() {
    let resized = ImageResizeFn.execute(vec![png_20x10()], &p(&[("width", "40"), ("height", "20")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![resized], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 40);
    assert_eq!(v["height"], 20);
}

#[test]
fn resize_nearest_filter() {
    let resized = ImageResizeFn.execute(vec![png_20x10()], &p(&[("width", "5"), ("height", "5"), ("filter", "nearest")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![resized], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 5);
}

#[test]
fn resize_zero_dimension() {
    assert!(ImageResizeFn.execute(vec![png_20x10()], &p(&[("width", "0"), ("height", "10")])).is_err());
}

#[test]
fn resize_missing_params() {
    assert!(ImageResizeFn.execute(vec![png_20x10()], &p(&[])).is_err());
}

#[test]
fn resize_no_input() {
    assert!(ImageResizeFn.execute(vec![], &p(&[("width", "10"), ("height", "10")])).is_err());
}

// ---- image_crop (5 tests) ----
#[test]
fn crop_basic() {
    let cropped = ImageCropFn.execute(vec![png_100x80()], &p(&[("x", "10"), ("y", "10"), ("width", "50"), ("height", "40")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![cropped], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 50);
    assert_eq!(v["height"], 40);
}

#[test]
fn crop_full_image() {
    let cropped = ImageCropFn.execute(vec![png_100x80()], &p(&[("x", "0"), ("y", "0"), ("width", "100"), ("height", "80")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![cropped], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 100);
}

#[test]
fn crop_out_of_bounds() {
    assert!(ImageCropFn.execute(vec![png_100x80()], &p(&[("x", "90"), ("y", "0"), ("width", "20"), ("height", "10")])).is_err());
}

#[test]
fn crop_missing_params() {
    assert!(ImageCropFn.execute(vec![png_100x80()], &p(&[])).is_err());
}

#[test]
fn crop_no_input() {
    assert!(ImageCropFn.execute(vec![], &p(&[("x", "0"), ("y", "0"), ("width", "10"), ("height", "10")])).is_err());
}

// ---- image_rotate (5 tests) ----
#[test]
fn rotate_90() {
    let rotated = ImageRotateFn.execute(vec![png_20x10()], &p(&[("degrees", "90")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![rotated], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 10); // swapped
    assert_eq!(v["height"], 20);
}

#[test]
fn rotate_180() {
    let rotated = ImageRotateFn.execute(vec![png_20x10()], &p(&[("degrees", "180")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![rotated], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 20); // same
    assert_eq!(v["height"], 10);
}

#[test]
fn rotate_270() {
    let rotated = ImageRotateFn.execute(vec![png_20x10()], &p(&[("degrees", "270")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![rotated], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 10);
}

#[test]
fn rotate_invalid_degrees() {
    assert!(ImageRotateFn.execute(vec![png_20x10()], &p(&[("degrees", "45")])).is_err());
}

#[test]
fn rotate_no_input() {
    assert!(ImageRotateFn.execute(vec![], &p(&[("degrees", "90")])).is_err());
}

// ---- image_convert (5 tests) ----
#[test]
fn convert_png_to_jpeg() {
    let jpg = ImageConvertFn.execute(vec![png_20x10()], &p(&[("format", "jpeg")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageDetectFormatFn.execute(vec![jpg], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["format"], "jpeg");
}

#[test]
fn convert_png_to_bmp() {
    let bmp = ImageConvertFn.execute(vec![png_20x10()], &p(&[("format", "bmp")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageDetectFormatFn.execute(vec![bmp], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["format"], "bmp");
}

#[test]
fn convert_preserves_dimensions() {
    let bmp = ImageConvertFn.execute(vec![png_20x10()], &p(&[("format", "bmp")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![bmp], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 20);
    assert_eq!(v["height"], 10);
}

#[test]
fn convert_invalid_format() {
    assert!(ImageConvertFn.execute(vec![png_20x10()], &p(&[("format", "xyz")])).is_err());
}

#[test]
fn convert_missing_format() {
    assert!(ImageConvertFn.execute(vec![png_20x10()], &p(&[])).is_err());
}

// ---- image_thumbnail (5 tests) ----
#[test]
fn thumbnail_default_128() {
    let thumb = ImageThumbnailFn.execute(vec![png_100x80()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![thumb], &p(&[])).unwrap()
    ).unwrap();
    // 100x80 with max_dim 128 → stays 100x80 (already fits)
    assert!(v["width"].as_u64().unwrap() <= 128);
    assert!(v["height"].as_u64().unwrap() <= 128);
}

#[test]
fn thumbnail_custom_size() {
    let thumb = ImageThumbnailFn.execute(vec![png_100x80()], &p(&[("max_dimension", "50")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![thumb], &p(&[])).unwrap()
    ).unwrap();
    assert!(v["width"].as_u64().unwrap() <= 50);
    assert!(v["height"].as_u64().unwrap() <= 50);
}

#[test]
fn thumbnail_output_is_png() {
    let thumb = ImageThumbnailFn.execute(vec![png_100x80()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageDetectFormatFn.execute(vec![thumb], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["format"], "png");
}

#[test]
fn thumbnail_zero_dimension() {
    assert!(ImageThumbnailFn.execute(vec![png_100x80()], &p(&[("max_dimension", "0")])).is_err());
}

#[test]
fn thumbnail_no_input() {
    assert!(ImageThumbnailFn.execute(vec![], &p(&[])).is_err());
}

// ---- image_to_grayscale (5 tests) ----
#[test]
fn grayscale_basic() {
    let gray = ImageToGrayscaleFn.execute(vec![png_20x10()], &p(&[])).unwrap();
    assert!(!gray.is_empty());
}

#[test]
fn grayscale_preserves_dimensions() {
    let gray = ImageToGrayscaleFn.execute(vec![png_100x80()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![gray], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 100);
    assert_eq!(v["height"], 80);
}

#[test]
fn grayscale_preserves_format() {
    let gray = ImageToGrayscaleFn.execute(vec![png_20x10()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageDetectFormatFn.execute(vec![gray], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["format"], "png");
}

#[test]
fn grayscale_invalid() {
    assert!(ImageToGrayscaleFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn grayscale_no_input() {
    assert!(ImageToGrayscaleFn.execute(vec![], &p(&[])).is_err());
}

// ---- image_watermark (5 tests) ----
#[test]
fn watermark_center() {
    let base = png_100x80();
    let wm = png_20x10();
    let out = ImageWatermarkFn.execute(vec![base, wm], &p(&[("position", "center")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![out], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 100);
}

#[test]
fn watermark_bottom_right() {
    let base = png_100x80();
    let wm = png_20x10();
    let out = ImageWatermarkFn.execute(vec![base, wm], &p(&[("position", "bottom-right")])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn watermark_tile() {
    let base = png_100x80();
    let wm = png_20x10();
    let out = ImageWatermarkFn.execute(vec![base, wm], &p(&[("position", "tile")])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn watermark_missing_second_input() {
    assert!(ImageWatermarkFn.execute(vec![png_100x80()], &p(&[("position", "center")])).is_err());
}

#[test]
fn watermark_invalid_position() {
    assert!(ImageWatermarkFn.execute(vec![png_100x80(), png_20x10()], &p(&[("position", "top-left")])).is_err());
}
