use bytes::Bytes;
use deriva_compute::builtins_format_scientific::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- zarr_read (5 tests) ----
#[test]
fn zarr_read_basic() {
    let zarr = make_zarr_zip(&[1.0, 2.0, 3.0, 4.0]);
    let out = ZarrReadFn.execute(vec![zarr], &p(&[])).unwrap();
    assert_eq!(&out[..6], b"ARROW1");
}

#[test]
fn zarr_read_invalid_zip() {
    assert!(ZarrReadFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn zarr_read_no_input() {
    assert!(ZarrReadFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn zarr_read_produces_arrow() {
    let zarr = make_zarr_zip(&[10.0, 20.0]);
    let out = ZarrReadFn.execute(vec![zarr], &p(&[])).unwrap();
    // Should be valid Arrow IPC
    assert!(out.len() > 10);
}

#[test]
fn zarr_read_id() {
    assert_eq!(ZarrReadFn.id().name, "zarr_read");
}

// ---- zarr_metadata (5 tests) ----
#[test]
fn zarr_metadata_basic() {
    let zarr = make_zarr_zip(&[1.0, 2.0, 3.0]);
    let out = ZarrMetadataFn.execute(vec![zarr], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let arrays = v["arrays"].as_array().unwrap();
    assert_eq!(arrays.len(), 1);
}

#[test]
fn zarr_metadata_has_shape() {
    let zarr = make_zarr_zip(&[1.0, 2.0, 3.0]);
    let out = ZarrMetadataFn.execute(vec![zarr], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["arrays"][0]["shape"][0], 3);
}

#[test]
fn zarr_metadata_has_dtype() {
    let zarr = make_zarr_zip(&[1.0]);
    let out = ZarrMetadataFn.execute(vec![zarr], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["arrays"][0]["dtype"], "<f8");
}

#[test]
fn zarr_metadata_invalid() {
    assert!(ZarrMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn zarr_metadata_id() {
    assert_eq!(ZarrMetadataFn.id().name, "zarr_metadata");
}
