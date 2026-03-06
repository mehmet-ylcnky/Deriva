use bytes::Bytes;
use deriva_compute::builtins_format_geo::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- geojson_to_wkt (5 tests) ----
#[test]
fn geojson_to_wkt_points() {
    let out = GeoJsonToWktFn.execute(vec![Bytes::from(FC_TWO_POINTS)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("POINT"));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn geojson_to_wkt_polygon() {
    let out = GeoJsonToWktFn.execute(vec![Bytes::from(FC_POLYGON)], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("POLYGON"));
}

#[test]
fn geojson_to_wkt_no_input() {
    assert!(GeoJsonToWktFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn geojson_to_wkt_empty_fc() {
    let fc = r#"{"type":"FeatureCollection","features":[]}"#;
    let out = GeoJsonToWktFn.execute(vec![Bytes::from(fc)], &p(&[])).unwrap();
    assert!(out.is_empty());
}

#[test]
fn geojson_to_wkt_id() {
    assert_eq!(GeoJsonToWktFn.id().name, "geojson_to_wkt");
}

// ---- wkt_to_geojson (5 tests) ----
#[test]
fn wkt_to_geojson_point() {
    let wkt = "POINT(1.0 2.0)";
    let out = WktToGeoJsonFn.execute(vec![Bytes::from(wkt)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["features"].as_array().unwrap().len(), 1);
    assert_eq!(v["features"][0]["geometry"]["type"], "Point");
}

#[test]
fn wkt_to_geojson_multi_line() {
    let wkt = "POINT(1 2)\nPOINT(3 4)";
    let out = WktToGeoJsonFn.execute(vec![Bytes::from(wkt)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["features"].as_array().unwrap().len(), 2);
}

#[test]
fn wkt_to_geojson_roundtrip() {
    let out1 = GeoJsonToWktFn.execute(vec![Bytes::from(FC_TWO_POINTS)], &p(&[])).unwrap();
    let out2 = WktToGeoJsonFn.execute(vec![out1], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out2).unwrap();
    assert_eq!(v["features"].as_array().unwrap().len(), 2);
}

#[test]
fn wkt_to_geojson_invalid() {
    assert!(WktToGeoJsonFn.execute(vec![Bytes::from("NOT_WKT")], &p(&[])).is_err());
}

#[test]
fn wkt_to_geojson_id() {
    assert_eq!(WktToGeoJsonFn.id().name, "wkt_to_geojson");
}

// ---- shapefile_to_geojson (5 tests) ----
#[test]
fn shapefile_to_geojson_basic() {
    let shp = make_shapefile_zip();
    let out = ShapefileToGeoJsonFn.execute(vec![shp], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["type"], "FeatureCollection");
    assert!(!v["features"].as_array().unwrap().is_empty());
}

#[test]
fn shapefile_to_geojson_has_properties() {
    let shp = make_shapefile_zip();
    let out = ShapefileToGeoJsonFn.execute(vec![shp], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["features"][0]["properties"].is_object());
}

#[test]
fn shapefile_to_geojson_invalid_zip() {
    assert!(ShapefileToGeoJsonFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn shapefile_to_geojson_no_input() {
    assert!(ShapefileToGeoJsonFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn shapefile_to_geojson_id() {
    assert_eq!(ShapefileToGeoJsonFn.id().name, "shapefile_to_geojson");
}

// ---- kml_to_geojson (5 tests) ----
#[test]
fn kml_to_geojson_basic() {
    let out = KmlToGeoJsonFn.execute(vec![make_kml()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["features"].as_array().unwrap().len(), 2);
}

#[test]
fn kml_to_geojson_has_name() {
    let out = KmlToGeoJsonFn.execute(vec![make_kml()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["features"][0]["properties"]["name"], "Test Point");
}

#[test]
fn kml_to_geojson_point_coords() {
    let out = KmlToGeoJsonFn.execute(vec![make_kml()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["features"][0]["geometry"]["type"], "Point");
}

#[test]
fn kml_to_geojson_no_input() {
    assert!(KmlToGeoJsonFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn kml_to_geojson_id() {
    assert_eq!(KmlToGeoJsonFn.id().name, "kml_to_geojson");
}

// ---- gpx_to_geojson (5 tests) ----
#[test]
fn gpx_to_geojson_basic() {
    let out = GpxToGeoJsonFn.execute(vec![make_gpx()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let features = v["features"].as_array().unwrap();
    assert_eq!(features.len(), 2); // 1 waypoint + 1 track
}

#[test]
fn gpx_to_geojson_waypoint() {
    let out = GpxToGeoJsonFn.execute(vec![make_gpx()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["features"][0]["geometry"]["type"], "Point");
    assert_eq!(v["features"][0]["properties"]["name"], "Seattle");
}

#[test]
fn gpx_to_geojson_track() {
    let out = GpxToGeoJsonFn.execute(vec![make_gpx()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["features"][1]["geometry"]["type"], "LineString");
}

#[test]
fn gpx_to_geojson_no_input() {
    assert!(GpxToGeoJsonFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn gpx_to_geojson_id() {
    assert_eq!(GpxToGeoJsonFn.id().name, "gpx_to_geojson");
}
