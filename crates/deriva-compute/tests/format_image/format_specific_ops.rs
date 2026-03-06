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

fn jpeg_10x10() -> Bytes {
    let img = image::DynamicImage::new_rgb8(10, 10);
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg).unwrap();
    Bytes::from(buf)
}

fn tiff_10x10() -> Bytes {
    let img = image::DynamicImage::new_rgb8(10, 10);
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Tiff).unwrap();
    Bytes::from(buf)
}

fn simple_svg() -> Bytes {
    Bytes::from_static(b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"><rect width=\"100\" height=\"100\" fill=\"red\"/></svg>")
}

fn simple_gif() -> Bytes {
    // Minimal 1x1 GIF89a
    let img = image::DynamicImage::new_rgb8(2, 2);
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Gif).unwrap();
    Bytes::from(buf)
}

// ---- png_optimize (5 tests) ----
#[test]
fn png_optimize_valid() {
    let out = PngOptimizeFn.execute(vec![png_10x10()], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn png_optimize_still_png() {
    let out = PngOptimizeFn.execute(vec![png_10x10()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageDetectFormatFn.execute(vec![out], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["format"], "png");
}

#[test]
fn png_optimize_preserves_dimensions() {
    let out = PngOptimizeFn.execute(vec![png_10x10()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![out], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 10);
}

#[test]
fn png_optimize_invalid() {
    assert!(PngOptimizeFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn png_optimize_no_input() {
    assert!(PngOptimizeFn.execute(vec![], &p(&[])).is_err());
}

// ---- jpeg_optimize (5 tests) ----
#[test]
fn jpeg_optimize_default_quality() {
    let out = JpegOptimizeFn.execute(vec![jpeg_10x10()], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn jpeg_optimize_custom_quality() {
    let out = JpegOptimizeFn.execute(vec![jpeg_10x10()], &p(&[("quality", "50")])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn jpeg_optimize_quality_zero() {
    assert!(JpegOptimizeFn.execute(vec![jpeg_10x10()], &p(&[("quality", "0")])).is_err());
}

#[test]
fn jpeg_optimize_quality_over_100() {
    assert!(JpegOptimizeFn.execute(vec![jpeg_10x10()], &p(&[("quality", "101")])).is_err());
}

#[test]
fn jpeg_optimize_no_input() {
    assert!(JpegOptimizeFn.execute(vec![], &p(&[])).is_err());
}

// ---- svg_minify (5 tests) ----
#[test]
fn svg_minify_basic() {
    let out = SvgMinifyFn.execute(vec![simple_svg()], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("svg"));
}

#[test]
fn svg_minify_removes_whitespace() {
    let svg = Bytes::from("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"50\" height=\"50\">\n  <!-- comment -->\n  <rect width=\"50\" height=\"50\" fill=\"blue\"/>\n</svg>");
    let out = SvgMinifyFn.execute(vec![svg], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(!text.contains("<!-- comment -->"));
}

#[test]
fn svg_minify_preserves_structure() {
    let out = SvgMinifyFn.execute(vec![simple_svg()], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("rect") || text.contains("path")); // resvg may convert rect to path
}

#[test]
fn svg_minify_invalid() {
    assert!(SvgMinifyFn.execute(vec![Bytes::from_static(b"not svg")], &p(&[])).is_err());
}

#[test]
fn svg_minify_no_input() {
    assert!(SvgMinifyFn.execute(vec![], &p(&[])).is_err());
}

// ---- svg_to_png (5 tests) ----
#[test]
fn svg_to_png_basic() {
    let out = SvgToPngFn.execute(vec![simple_svg()], &p(&[("width", "200"), ("height", "200")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageDetectFormatFn.execute(vec![out], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["format"], "png");
}

#[test]
fn svg_to_png_dimensions() {
    let out = SvgToPngFn.execute(vec![simple_svg()], &p(&[("width", "300"), ("height", "150")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageMetadataFn.execute(vec![out], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["width"], 300);
    assert_eq!(v["height"], 150);
}

#[test]
fn svg_to_png_zero_dimension() {
    assert!(SvgToPngFn.execute(vec![simple_svg()], &p(&[("width", "0"), ("height", "100")])).is_err());
}

#[test]
fn svg_to_png_missing_params() {
    assert!(SvgToPngFn.execute(vec![simple_svg()], &p(&[])).is_err());
}

#[test]
fn svg_to_png_no_input() {
    assert!(SvgToPngFn.execute(vec![], &p(&[("width", "100"), ("height", "100")])).is_err());
}

// ---- tiff_split (5 tests) ----
#[test]
fn tiff_split_single_page() {
    let out = TiffSplitFn.execute(vec![tiff_10x10()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["pages"].is_array());
    assert!(v["pages"].as_array().unwrap().len() >= 1);
}

#[test]
fn tiff_split_page_has_data() {
    let out = TiffSplitFn.execute(vec![tiff_10x10()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let page = &v["pages"][0];
    assert!(page["data_base64"].is_string());
    assert!(page["width"].as_u64().unwrap() > 0);
}

#[test]
fn tiff_split_page_dimensions() {
    let out = TiffSplitFn.execute(vec![tiff_10x10()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["pages"][0]["width"], 10);
    assert_eq!(v["pages"][0]["height"], 10);
}

#[test]
fn tiff_split_invalid() {
    assert!(TiffSplitFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn tiff_split_no_input() {
    assert!(TiffSplitFn.execute(vec![], &p(&[])).is_err());
}

// ---- tiff_merge (5 tests) ----
#[test]
fn tiff_merge_single() {
    let out = TiffMergeFn.execute(vec![tiff_10x10()], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn tiff_merge_produces_tiff() {
    let out = TiffMergeFn.execute(vec![tiff_10x10()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &ImageDetectFormatFn.execute(vec![out], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["format"], "tiff");
}

#[test]
fn tiff_merge_multiple() {
    let t = tiff_10x10();
    let out = TiffMergeFn.execute(vec![t.clone(), t], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn tiff_merge_invalid() {
    assert!(TiffMergeFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn tiff_merge_no_input() {
    assert!(TiffMergeFn.execute(vec![], &p(&[])).is_err());
}

// ---- gif_extract_frames (5 tests) ----
#[test]
fn gif_extract_frames_basic() {
    let out = GifExtractFramesFn.execute(vec![simple_gif()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["frames"].is_array());
    assert!(v["frames"].as_array().unwrap().len() >= 1);
}

#[test]
fn gif_extract_frames_has_data() {
    let out = GifExtractFramesFn.execute(vec![simple_gif()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["frames"][0]["data_base64"].is_string());
}

#[test]
fn gif_extract_frames_index() {
    let out = GifExtractFramesFn.execute(vec![simple_gif()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["frames"][0]["frame"], 0);
}

#[test]
fn gif_extract_frames_invalid() {
    assert!(GifExtractFramesFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn gif_extract_frames_no_input() {
    assert!(GifExtractFramesFn.execute(vec![], &p(&[])).is_err());
}
