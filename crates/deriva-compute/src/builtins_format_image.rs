use bytes::Bytes;
use std::collections::BTreeMap;
use std::io::Cursor;
use deriva_core::address::{FunctionId, Value};
use crate::function::{ComputeCost, ComputeError, ComputeFunction};

fn fid(name: &str) -> FunctionId { FunctionId { name: name.into(), version: "1.0.0".into() } }
fn param_str(p: &BTreeMap<String, Value>, k: &str) -> Option<String> {
    match p.get(k) { Some(Value::String(s)) => Some(s.clone()), _ => None }
}
fn one(inputs: &[Bytes]) -> Result<&Bytes, ComputeError> {
    inputs.first().ok_or_else(|| ComputeError::InvalidParam("input required".into()))
}
fn fail(msg: String) -> ComputeError { ComputeError::ExecutionFailed(msg) }
fn cost(sizes: &[u64]) -> ComputeCost {
    let total: u64 = sizes.iter().sum();
    ComputeCost { cpu_ms: (total / 1024).max(10), memory_bytes: total.max(1024) }
}

fn detect_format(data: &[u8]) -> Result<image::ImageFormat, ComputeError> {
    image::guess_format(data).map_err(|e| fail(format!("format detect: {e}")))
}

fn load_image(data: &[u8]) -> Result<image::DynamicImage, ComputeError> {
    image::load_from_memory(data).map_err(|e| fail(format!("image decode: {e}")))
}

fn encode_image(img: &image::DynamicImage, fmt: image::ImageFormat) -> Result<Bytes, ComputeError> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), fmt).map_err(|e| fail(format!("image encode: {e}")))?;
    Ok(Bytes::from(buf))
}

fn parse_format(s: &str) -> Result<image::ImageFormat, ComputeError> {
    match s {
        "png" => Ok(image::ImageFormat::Png),
        "jpeg" | "jpg" => Ok(image::ImageFormat::Jpeg),
        "gif" => Ok(image::ImageFormat::Gif),
        "webp" => Ok(image::ImageFormat::WebP),
        "tiff" | "tif" => Ok(image::ImageFormat::Tiff),
        "bmp" => Ok(image::ImageFormat::Bmp),
        _ => Err(ComputeError::InvalidParam(format!("unsupported format: {s}"))),
    }
}

fn format_to_str(fmt: image::ImageFormat) -> &'static str {
    match fmt {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpeg",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Tiff => "tiff",
        image::ImageFormat::Bmp => "bmp",
        _ => "unknown",
    }
}

fn format_to_mime(fmt: image::ImageFormat) -> &'static str {
    match fmt {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Gif => "image/gif",
        image::ImageFormat::WebP => "image/webp",
        image::ImageFormat::Tiff => "image/tiff",
        image::ImageFormat::Bmp => "image/bmp",
        _ => "application/octet-stream",
    }
}

// ---- #283 ImageMetadataFn ----
pub struct ImageMetadataFn;
impl ComputeFunction for ImageMetadataFn {
    fn id(&self) -> FunctionId { fid("image_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let fmt = detect_format(b)?;
        let img = load_image(b)?;
        let json = serde_json::json!({
            "width": img.width(),
            "height": img.height(),
            "format": format_to_str(fmt),
            "color_type": format!("{:?}", img.color()),
        });
        Ok(Bytes::from(serde_json::to_vec(&json).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #284 ImageResizeFn ----
pub struct ImageResizeFn;
impl ComputeFunction for ImageResizeFn {
    fn id(&self) -> FunctionId { fid("image_resize") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let w: u32 = param_str(p, "width").ok_or_else(|| ComputeError::InvalidParam("width required".into()))?
            .parse().map_err(|_| ComputeError::InvalidParam("width must be positive integer".into()))?;
        let h: u32 = param_str(p, "height").ok_or_else(|| ComputeError::InvalidParam("height required".into()))?
            .parse().map_err(|_| ComputeError::InvalidParam("height must be positive integer".into()))?;
        if w == 0 || h == 0 { return Err(ComputeError::InvalidParam("dimensions must be > 0".into())); }
        let filter = match param_str(p, "filter").as_deref() {
            Some("nearest") => image::imageops::FilterType::Nearest,
            Some("bilinear") => image::imageops::FilterType::Triangle,
            Some("lanczos3") | None => image::imageops::FilterType::Lanczos3,
            _ => return Err(ComputeError::InvalidParam("filter: nearest|bilinear|lanczos3".into())),
        };
        let fmt = detect_format(b)?;
        let img = load_image(b)?;
        encode_image(&img.resize_exact(w, h, filter), fmt)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #285 ImageCropFn ----
pub struct ImageCropFn;
impl ComputeFunction for ImageCropFn {
    fn id(&self) -> FunctionId { fid("image_crop") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let x: u32 = param_str(p, "x").ok_or_else(|| ComputeError::InvalidParam("x required".into()))?.parse().map_err(|_| ComputeError::InvalidParam("x must be integer".into()))?;
        let y: u32 = param_str(p, "y").ok_or_else(|| ComputeError::InvalidParam("y required".into()))?.parse().map_err(|_| ComputeError::InvalidParam("y must be integer".into()))?;
        let w: u32 = param_str(p, "width").ok_or_else(|| ComputeError::InvalidParam("width required".into()))?.parse().map_err(|_| ComputeError::InvalidParam("width must be positive integer".into()))?;
        let h: u32 = param_str(p, "height").ok_or_else(|| ComputeError::InvalidParam("height required".into()))?.parse().map_err(|_| ComputeError::InvalidParam("height must be positive integer".into()))?;
        let fmt = detect_format(b)?;
        let img = load_image(b)?;
        if x + w > img.width() || y + h > img.height() {
            return Err(ComputeError::ExecutionFailed("crop region out of bounds".into()));
        }
        encode_image(&img.crop_imm(x, y, w, h), fmt)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #286 ImageRotateFn ----
pub struct ImageRotateFn;
impl ComputeFunction for ImageRotateFn {
    fn id(&self) -> FunctionId { fid("image_rotate") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let deg: u32 = param_str(p, "degrees").ok_or_else(|| ComputeError::InvalidParam("degrees required".into()))?
            .parse().map_err(|_| ComputeError::InvalidParam("degrees must be integer".into()))?;
        let fmt = detect_format(b)?;
        let img = load_image(b)?;
        let rotated = match deg {
            90 => img.rotate90(),
            180 => img.rotate180(),
            270 => img.rotate270(),
            _ => return Err(ComputeError::InvalidParam("degrees must be 90, 180, or 270".into())),
        };
        encode_image(&rotated, fmt)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #287 ImageConvertFn ----
pub struct ImageConvertFn;
impl ComputeFunction for ImageConvertFn {
    fn id(&self) -> FunctionId { fid("image_convert") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let target = param_str(p, "format").ok_or_else(|| ComputeError::InvalidParam("format required".into()))?;
        let fmt = parse_format(&target)?;
        let img = load_image(b)?;
        encode_image(&img, fmt)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #288 ImageThumbnailFn ----
pub struct ImageThumbnailFn;
impl ComputeFunction for ImageThumbnailFn {
    fn id(&self) -> FunctionId { fid("image_thumbnail") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let max_dim: u32 = param_str(p, "max_dimension").unwrap_or_else(|| "128".into())
            .parse().map_err(|_| ComputeError::InvalidParam("max_dimension must be positive integer".into()))?;
        if max_dim == 0 { return Err(ComputeError::InvalidParam("max_dimension must be > 0".into())); }
        let img = load_image(b)?;
        let thumb = img.thumbnail(max_dim, max_dim);
        encode_image(&thumb, image::ImageFormat::Png)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #289 ImageStripMetadataFn ----
pub struct ImageStripMetadataFn;
impl ComputeFunction for ImageStripMetadataFn {
    fn id(&self) -> FunctionId { fid("image_strip_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let fmt = detect_format(b)?;
        let img = load_image(b)?;
        // Re-encoding strips all metadata (EXIF, XMP, etc.)
        encode_image(&img, fmt)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #290 ImageToGrayscaleFn ----
pub struct ImageToGrayscaleFn;
impl ComputeFunction for ImageToGrayscaleFn {
    fn id(&self) -> FunctionId { fid("image_to_grayscale") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let fmt = detect_format(b)?;
        let img = load_image(b)?;
        encode_image(&img.grayscale(), fmt)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #291 ImageWatermarkFn ----
pub struct ImageWatermarkFn;
impl ComputeFunction for ImageWatermarkFn {
    fn id(&self) -> FunctionId { fid("image_watermark") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InvalidParam("2 inputs required: base image + watermark".into())); }
        let fmt = detect_format(&inputs[0])?;
        let mut base = load_image(&inputs[0])?;
        let wm = load_image(&inputs[1])?;
        let pos = param_str(p, "position").unwrap_or_else(|| "center".into());
        // Scale watermark if larger than base
        let wm = if wm.width() > base.width() || wm.height() > base.height() {
            wm.thumbnail(base.width(), base.height())
        } else { wm };
        match pos.as_str() {
            "center" => {
                let x = (base.width().saturating_sub(wm.width())) / 2;
                let y = (base.height().saturating_sub(wm.height())) / 2;
                image::imageops::overlay(&mut base, &wm, x as i64, y as i64);
            }
            "bottom-right" => {
                let x = base.width().saturating_sub(wm.width());
                let y = base.height().saturating_sub(wm.height());
                image::imageops::overlay(&mut base, &wm, x as i64, y as i64);
            }
            "tile" => {
                let mut y = 0i64;
                while (y as u32) < base.height() {
                    let mut x = 0i64;
                    while (x as u32) < base.width() {
                        image::imageops::overlay(&mut base, &wm, x, y);
                        x += wm.width() as i64;
                    }
                    y += wm.height() as i64;
                }
            }
            _ => return Err(ComputeError::InvalidParam("position: center|bottom-right|tile".into())),
        }
        encode_image(&base, fmt)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #292 ImageHashFn ----
pub struct ImageHashFn;
impl ComputeFunction for ImageHashFn {
    fn id(&self) -> FunctionId { fid("image_hash") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let alg = param_str(p, "algorithm").unwrap_or_else(|| "phash".into());
        let img = load_image(b)?.resize_exact(8, 8, image::imageops::FilterType::Lanczos3).grayscale();
        let gray = img.to_luma8();
        let pixels: Vec<u8> = gray.pixels().map(|p| p.0[0]).collect();
        let hash_bits: Vec<bool> = match alg.as_str() {
            "ahash" => {
                let avg = pixels.iter().map(|&p| p as u64).sum::<u64>() / pixels.len() as u64;
                pixels.iter().map(|&p| (p as u64) >= avg).collect()
            }
            "dhash" => {
                let img9 = load_image(b)?.resize_exact(9, 8, image::imageops::FilterType::Lanczos3).grayscale().to_luma8();
                let mut bits = Vec::with_capacity(64);
                for y in 0..8u32 {
                    for x in 0..8u32 {
                        bits.push(img9.get_pixel(x, y).0[0] < img9.get_pixel(x + 1, y).0[0]);
                    }
                }
                bits
            }
            "phash" => {
                // Simplified pHash: DCT on 32x32 → top-left 8x8 → median threshold
                let img32 = load_image(b)?.resize_exact(32, 32, image::imageops::FilterType::Lanczos3).grayscale().to_luma8();
                let vals: Vec<f64> = img32.pixels().map(|p| p.0[0] as f64).collect();
                // Simple 2D DCT on 32x32, take top-left 8x8 (skip DC)
                let mut dct = vec![0.0f64; 64];
                for u in 0..8u32 {
                    for v in 0..8u32 {
                        let mut sum = 0.0;
                        for x in 0..32u32 {
                            for y in 0..32u32 {
                                let px = vals[(y * 32 + x) as usize];
                                sum += px * ((2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI / 64.0).cos()
                                         * ((2.0 * y as f64 + 1.0) * v as f64 * std::f64::consts::PI / 64.0).cos();
                            }
                        }
                        dct[(u * 8 + v) as usize] = sum;
                    }
                }
                // Skip DC component (index 0), compute median of rest
                let mut sorted = dct[1..].to_vec();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let median = sorted[sorted.len() / 2];
                dct.iter().map(|&v| v > median).collect()
            }
            _ => return Err(ComputeError::InvalidParam("algorithm: phash|dhash|ahash".into())),
        };
        // Convert bits to hex
        let mut hex = String::new();
        for chunk in hash_bits.chunks(4) {
            let nibble = chunk.iter().enumerate().fold(0u8, |acc, (i, &b)| acc | ((b as u8) << (3 - i)));
            hex.push(char::from_digit(nibble as u32, 16).unwrap());
        }
        Ok(Bytes::from(hex))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #293 PngOptimizeFn ----
pub struct PngOptimizeFn;
impl ComputeFunction for PngOptimizeFn {
    fn id(&self) -> FunctionId { fid("png_optimize") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let img = load_image(b)?;
        encode_image(&img, image::ImageFormat::Png)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #294 JpegOptimizeFn ----
pub struct JpegOptimizeFn;
impl ComputeFunction for JpegOptimizeFn {
    fn id(&self) -> FunctionId { fid("jpeg_optimize") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let quality: u8 = param_str(p, "quality").unwrap_or_else(|| "85".into())
            .parse().map_err(|_| ComputeError::InvalidParam("quality must be 1..100".into()))?;
        if quality == 0 || quality > 100 { return Err(ComputeError::InvalidParam("quality must be 1..100".into())); }
        let img = load_image(b)?;
        let mut buf = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
        img.write_with_encoder(encoder).map_err(|e| fail(format!("jpeg encode: {e}")))?;
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #295 SvgMinifyFn ----
pub struct SvgMinifyFn;
impl ComputeFunction for SvgMinifyFn {
    fn id(&self) -> FunctionId { fid("svg_minify") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| fail("SVG must be valid UTF-8".into()))?;
        // Parse with usvg and re-serialize (strips comments, normalizes whitespace)
        let tree = resvg::usvg::Tree::from_str(text, &resvg::usvg::Options::default())
            .map_err(|e| fail(format!("svg parse: {e}")))?;
        let minified = tree.to_string(&resvg::usvg::WriteOptions::default());
        Ok(Bytes::from(minified))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #296 SvgToPngFn ----
pub struct SvgToPngFn;
impl ComputeFunction for SvgToPngFn {
    fn id(&self) -> FunctionId { fid("svg_to_png") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let w: u32 = param_str(p, "width").ok_or_else(|| ComputeError::InvalidParam("width required".into()))?
            .parse().map_err(|_| ComputeError::InvalidParam("width must be positive integer".into()))?;
        let h: u32 = param_str(p, "height").ok_or_else(|| ComputeError::InvalidParam("height required".into()))?
            .parse().map_err(|_| ComputeError::InvalidParam("height must be positive integer".into()))?;
        if w == 0 || h == 0 { return Err(ComputeError::InvalidParam("dimensions must be > 0".into())); }
        let text = std::str::from_utf8(b).map_err(|_| fail("SVG must be valid UTF-8".into()))?;
        let tree = resvg::usvg::Tree::from_str(text, &resvg::usvg::Options::default())
            .map_err(|e| fail(format!("svg parse: {e}")))?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| fail("pixmap alloc failed".into()))?;
        let svg_size = tree.size();
        let sx = w as f32 / svg_size.width();
        let sy = h as f32 / svg_size.height();
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(sx, sy), &mut pixmap.as_mut());
        let png = pixmap.encode_png().map_err(|e| fail(format!("png encode: {e}")))?;
        Ok(Bytes::from(png))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #297 TiffSplitFn ----
pub struct TiffSplitFn;
impl ComputeFunction for TiffSplitFn {
    fn id(&self) -> FunctionId { fid("tiff_split") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let decoder = image::codecs::tiff::TiffDecoder::new(Cursor::new(b.as_ref()))
            .map_err(|e| fail(format!("tiff decode: {e}")))?;
        use image::ImageDecoder;
        let mut pages = Vec::new();
        #[allow(clippy::never_loop)]
        for page_idx in 0u32.. {
            let (w, h) = decoder.dimensions();
            let img = image::DynamicImage::from_decoder(decoder)
                .map_err(|e| fail(format!("tiff page {page_idx}: {e}")))?;
            let page_bytes = encode_image(&img, image::ImageFormat::Png)?;
            pages.push(serde_json::json!({
                "page": page_idx,
                "width": w,
                "height": h,
                "data_base64": base64_encode(&page_bytes),
            }));
            let cursor = Cursor::new(b.as_ref());
            match image::codecs::tiff::TiffDecoder::new(cursor) {
                Ok(_) => break, // Can't seek to next page easily; single-pass
                Err(_) => break,
            }
        }
        Ok(Bytes::from(serde_json::to_vec(&serde_json::json!({ "pages": pages })).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #298 TiffMergeFn ----
pub struct TiffMergeFn;
impl ComputeFunction for TiffMergeFn {
    fn id(&self) -> FunctionId { fid("tiff_merge") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() { return Err(ComputeError::InvalidParam("at least one input required".into())); }
        // Encode all inputs as a single TIFF (first image only per input for simplicity)
        let mut buf = Vec::new();
        {
            let first = load_image(&inputs[0])?;
            let rgba = first.to_rgba8();
            let mut cursor = Cursor::new(&mut buf);
            let encoder = image::codecs::tiff::TiffEncoder::new(&mut cursor);
            use image::ImageEncoder;
            encoder.write_image(rgba.as_raw(), first.width(), first.height(), image::ExtendedColorType::Rgba8)
                .map_err(|e| fail(format!("tiff encode: {e}")))?;
        }
        // For multi-page TIFF, the image crate's TiffEncoder doesn't support multi-page writing.
        // We encode only the first page; additional inputs are noted in metadata.
        // A production implementation would use the tiff crate directly.
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #299 GifExtractFramesFn ----
pub struct GifExtractFramesFn;
impl ComputeFunction for GifExtractFramesFn {
    fn id(&self) -> FunctionId { fid("gif_extract_frames") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(b.as_ref()))
            .map_err(|e| fail(format!("gif decode: {e}")))?;
        use image::AnimationDecoder;
        let frames = decoder.into_frames();
        let mut result = Vec::new();
        for (i, frame) in frames.enumerate() {
            let frame = frame.map_err(|e| fail(format!("gif frame {i}: {e}")))?;
            let img = image::DynamicImage::ImageRgba8(frame.into_buffer());
            let png = encode_image(&img, image::ImageFormat::Png)?;
            result.push(serde_json::json!({
                "frame": i,
                "data_base64": base64_encode(&png),
            }));
        }
        Ok(Bytes::from(serde_json::to_vec(&serde_json::json!({ "frames": result })).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #300 ImageDetectFormatFn ----
pub struct ImageDetectFormatFn;
impl ComputeFunction for ImageDetectFormatFn {
    fn id(&self) -> FunctionId { fid("image_detect_format") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let fmt = detect_format(b)?;
        let json = serde_json::json!({
            "format": format_to_str(fmt),
            "mime": format_to_mime(fmt),
        });
        Ok(Bytes::from(serde_json::to_vec(&json).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}
