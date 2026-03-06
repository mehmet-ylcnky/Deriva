use bytes::Bytes;
use deriva_compute::builtins_format_geo::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- geotiff_metadata (5 tests) ----
#[test]
fn geotiff_metadata_basic() {
    let tiff = make_tiff();
    let out = GeoTiffMetadataFn.execute(vec![tiff], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["width"], 64);
    assert_eq!(v["height"], 32);
    assert_eq!(v["bands"], 3);
}

#[test]
fn geotiff_metadata_format() {
    let tiff = make_tiff();
    let out = GeoTiffMetadataFn.execute(vec![tiff], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["format"], "GeoTIFF");
}

#[test]
fn geotiff_metadata_not_tiff() {
    assert!(GeoTiffMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn geotiff_metadata_no_input() {
    assert!(GeoTiffMetadataFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn geotiff_metadata_id() {
    assert_eq!(GeoTiffMetadataFn.id().name, "geotiff_metadata");
}

// ---- geotiff_crop (5 tests — stubbed, requires GDAL) ----
#[test]
fn geotiff_crop_requires_gdal() {
    let err = GeoTiffCropFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("gdal"));
}

#[test]
fn geotiff_crop_id() {
    assert_eq!(GeoTiffCropFn.id().name, "geotiff_crop");
}

#[test]
fn geotiff_crop_no_input() {
    assert!(GeoTiffCropFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn geotiff_crop_with_bbox() {
    assert!(GeoTiffCropFn.execute(vec![Bytes::new()], &p(&[("bbox", "0,0,1,1")])).is_err());
}

#[test]
fn geotiff_crop_error_type() {
    assert!(GeoTiffCropFn.execute(vec![Bytes::new()], &p(&[])).is_err());
}

// ---- geotiff_resample (5 tests — stubbed, requires GDAL) ----
#[test]
fn geotiff_resample_requires_gdal() {
    let err = GeoTiffResampleFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("gdal"));
}

#[test]
fn geotiff_resample_id() {
    assert_eq!(GeoTiffResampleFn.id().name, "geotiff_resample");
}

#[test]
fn geotiff_resample_no_input() {
    assert!(GeoTiffResampleFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn geotiff_resample_with_params() {
    assert!(GeoTiffResampleFn.execute(vec![Bytes::new()], &p(&[("width", "100"), ("height", "100")])).is_err());
}

#[test]
fn geotiff_resample_error_msg() {
    let err = GeoTiffResampleFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-geo-gdal"));
}

// ---- flatgeobuf_read + flatgeobuf_write (5 tests each) ----
#[test]
fn flatgeobuf_write_basic() {
    let out = FlatGeobufWriteFn.execute(vec![Bytes::from(FC_TWO_POINTS)], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn flatgeobuf_roundtrip() {
    let fgb = FlatGeobufWriteFn.execute(vec![Bytes::from(FC_TWO_POINTS)], &p(&[])).unwrap();
    let out = FlatGeobufReadFn.execute(vec![fgb], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["type"], "FeatureCollection");
}

#[test]
fn flatgeobuf_write_invalid() {
    assert!(FlatGeobufWriteFn.execute(vec![Bytes::from_static(b"not json")], &p(&[])).is_err());
}

#[test]
fn flatgeobuf_write_no_input() {
    assert!(FlatGeobufWriteFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn flatgeobuf_write_id() {
    assert_eq!(FlatGeobufWriteFn.id().name, "flatgeobuf_write");
}

#[test]
fn flatgeobuf_read_basic() {
    let fgb = FlatGeobufWriteFn.execute(vec![Bytes::from(FC_TWO_POINTS)], &p(&[])).unwrap();
    let out = FlatGeobufReadFn.execute(vec![fgb], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn flatgeobuf_read_invalid() {
    assert!(FlatGeobufReadFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn flatgeobuf_read_no_input() {
    assert!(FlatGeobufReadFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn flatgeobuf_read_with_bbox() {
    let fgb = FlatGeobufWriteFn.execute(vec![Bytes::from(FC_TWO_POINTS)], &p(&[])).unwrap();
    // bbox that covers first point only
    let out = FlatGeobufReadFn.execute(vec![fgb], &p(&[("bbox", "0,1,2,3")])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn flatgeobuf_read_id() {
    assert_eq!(FlatGeobufReadFn.id().name, "flatgeobuf_read");
}
