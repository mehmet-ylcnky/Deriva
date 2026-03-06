use bytes::Bytes;
use deriva_compute::builtins_format_geo::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- geojson_validate (5 tests) ----
#[test]
fn geojson_validate_valid() {
    let out = GeoJsonValidateFn.execute(vec![Bytes::from(FC_TWO_POINTS)], &p(&[])).unwrap();
    assert_eq!(out.as_ref(), FC_TWO_POINTS.as_bytes());
}

#[test]
fn geojson_validate_invalid() {
    assert!(GeoJsonValidateFn.execute(vec![Bytes::from_static(b"not json")], &p(&[])).is_err());
}

#[test]
fn geojson_validate_no_input() {
    assert!(GeoJsonValidateFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn geojson_validate_empty_fc() {
    let fc = r#"{"type":"FeatureCollection","features":[]}"#;
    let out = GeoJsonValidateFn.execute(vec![Bytes::from(fc)], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn geojson_validate_id() {
    assert_eq!(GeoJsonValidateFn.id().name, "geojson_validate");
}

// ---- geojson_filter (5 tests) ----
#[test]
fn geojson_filter_eq() {
    let out = GeoJsonFilterFn.execute(
        vec![Bytes::from(FC_TWO_POINTS)],
        &p(&[("property", "name"), ("op", "eq"), ("value", "A")]),
    ).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["features"].as_array().unwrap().len(), 1);
}

#[test]
fn geojson_filter_ne() {
    let out = GeoJsonFilterFn.execute(
        vec![Bytes::from(FC_TWO_POINTS)],
        &p(&[("property", "name"), ("op", "ne"), ("value", "A")]),
    ).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["features"].as_array().unwrap().len(), 1);
    assert_eq!(v["features"][0]["properties"]["name"], "B");
}

#[test]
fn geojson_filter_no_match() {
    let out = GeoJsonFilterFn.execute(
        vec![Bytes::from(FC_TWO_POINTS)],
        &p(&[("property", "name"), ("op", "eq"), ("value", "Z")]),
    ).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["features"].as_array().unwrap().len(), 0);
}

#[test]
fn geojson_filter_missing_property() {
    assert!(GeoJsonFilterFn.execute(vec![Bytes::from(FC_TWO_POINTS)], &p(&[("op", "eq"), ("value", "A")])).is_err());
}

#[test]
fn geojson_filter_id() {
    assert_eq!(GeoJsonFilterFn.id().name, "geojson_filter");
}

// ---- geojson_bbox (5 tests) ----
#[test]
fn geojson_bbox_two_points() {
    let out = GeoJsonBboxFn.execute(vec![Bytes::from(FC_TWO_POINTS)], &p(&[])).unwrap();
    let v: Vec<f64> = serde_json::from_slice(&out).unwrap();
    assert_eq!(v, vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn geojson_bbox_polygon() {
    let out = GeoJsonBboxFn.execute(vec![Bytes::from(FC_POLYGON)], &p(&[])).unwrap();
    let v: Vec<f64> = serde_json::from_slice(&out).unwrap();
    assert_eq!(v, vec![0.0, 0.0, 10.0, 10.0]);
}

#[test]
fn geojson_bbox_empty_fc() {
    let fc = r#"{"type":"FeatureCollection","features":[]}"#;
    assert!(GeoJsonBboxFn.execute(vec![Bytes::from(fc)], &p(&[])).is_err());
}

#[test]
fn geojson_bbox_no_input() {
    assert!(GeoJsonBboxFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn geojson_bbox_id() {
    assert_eq!(GeoJsonBboxFn.id().name, "geojson_bbox");
}

// ---- geojson_simplify (5 tests) ----
#[test]
fn geojson_simplify_reduces_vertices() {
    let out = GeoJsonSimplifyFn.execute(
        vec![Bytes::from(FC_LINE)], &p(&[("tolerance", "2.0")]),
    ).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let coords = v["features"][0]["geometry"]["coordinates"].as_array().unwrap();
    // Original has 11 points, simplified should have fewer
    assert!(coords.len() < 11, "got {} coords", coords.len());
}

#[test]
fn geojson_simplify_zero_tolerance() {
    let out = GeoJsonSimplifyFn.execute(
        vec![Bytes::from(FC_LINE)], &p(&[("tolerance", "0.0")]),
    ).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let coords = v["features"][0]["geometry"]["coordinates"].as_array().unwrap();
    assert_eq!(coords.len(), 11); // No simplification
}

#[test]
fn geojson_simplify_default_tolerance() {
    let out = GeoJsonSimplifyFn.execute(vec![Bytes::from(FC_LINE)], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn geojson_simplify_no_input() {
    assert!(GeoJsonSimplifyFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn geojson_simplify_id() {
    assert_eq!(GeoJsonSimplifyFn.id().name, "geojson_simplify");
}
