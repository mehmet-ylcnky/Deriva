use bytes::Bytes;
use std::collections::BTreeMap;
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
fn cost(input_sizes: &[u64]) -> ComputeCost {
    let total: u64 = input_sizes.iter().sum();
    ComputeCost { cpu_ms: (total / 1024).max(10), memory_bytes: total.max(1024) }
}
fn utf8(b: &[u8]) -> Result<&str, ComputeError> {
    std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))
}
fn parse_fc(b: &[u8]) -> Result<geojson::FeatureCollection, ComputeError> {
    let text = utf8(b)?;
    let gj: geojson::GeoJson = text.parse().map_err(|e| fail(format!("geojson: {e}")))?;
    geojson::FeatureCollection::try_from(gj).map_err(|e| fail(format!("expected FeatureCollection: {e}")))
}

fn update_bbox(geom: &geojson::Geometry, bbox: &mut [f64; 4]) {
    fn visit_coords(coords: &geojson::Value, bbox: &mut [f64; 4]) {
        match coords {
            geojson::Value::Point(c) => { update_pt(c, bbox); }
            geojson::Value::MultiPoint(pts) | geojson::Value::LineString(pts) => {
                for c in pts { update_pt(c, bbox); }
            }
            geojson::Value::MultiLineString(lines) | geojson::Value::Polygon(lines) => {
                for ring in lines { for c in ring { update_pt(c, bbox); } }
            }
            geojson::Value::MultiPolygon(polys) => {
                for poly in polys { for ring in poly { for c in ring { update_pt(c, bbox); } } }
            }
            geojson::Value::GeometryCollection(geoms) => {
                for g in geoms { visit_coords(&g.value, bbox); }
            }
        }
    }
    fn update_pt(c: &[f64], bbox: &mut [f64; 4]) {
        if c.len() >= 2 {
            if c[0] < bbox[0] { bbox[0] = c[0]; }
            if c[1] < bbox[1] { bbox[1] = c[1]; }
            if c[0] > bbox[2] { bbox[2] = c[0]; }
            if c[1] > bbox[3] { bbox[3] = c[1]; }
        }
    }
    visit_coords(&geom.value, bbox);
}

// ---- #356 GeoJsonValidateFn ----
pub struct GeoJsonValidateFn;
impl ComputeFunction for GeoJsonValidateFn {
    fn id(&self) -> FunctionId { fid("geojson_validate") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let _: geojson::GeoJson = text.parse().map_err(|e| fail(format!("invalid geojson: {e}")))?;
        Ok(b.clone())
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #357 GeoJsonFilterFn ----
pub struct GeoJsonFilterFn;
impl ComputeFunction for GeoJsonFilterFn {
    fn id(&self) -> FunctionId { fid("geojson_filter") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let prop = param_str(p, "property").ok_or_else(|| ComputeError::InvalidParam("property required".into()))?;
        let op = param_str(p, "op").unwrap_or_else(|| "eq".into());
        let val = param_str(p, "value").ok_or_else(|| ComputeError::InvalidParam("value required".into()))?;
        let mut fc = parse_fc(b)?;
        fc.features.retain(|f| {
            let props = match &f.properties { Some(p) => p, None => return false };
            let v = match props.get(&prop) { Some(v) => v, None => return false };
            let s = match v { serde_json::Value::String(s) => s.clone(), other => other.to_string() };
            match op.as_str() {
                "eq" => s == val,
                "ne" => s != val,
                "gt" => s.parse::<f64>().ok().zip(val.parse::<f64>().ok()).map(|(a,b)| a > b).unwrap_or(false),
                "lt" => s.parse::<f64>().ok().zip(val.parse::<f64>().ok()).map(|(a,b)| a < b).unwrap_or(false),
                "contains" => s.contains(&val),
                _ => false,
            }
        });
        Ok(Bytes::from(serde_json::to_string(&geojson::GeoJson::from(fc)).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #358 GeoJsonBboxFn ----
pub struct GeoJsonBboxFn;
impl ComputeFunction for GeoJsonBboxFn {
    fn id(&self) -> FunctionId { fid("geojson_bbox") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let fc = parse_fc(b)?;
        if fc.features.is_empty() { return Err(fail("no features".into())); }
        let mut bbox = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for f in &fc.features {
            if let Some(ref g) = f.geometry { update_bbox(g, &mut bbox); }
        }
        Ok(Bytes::from(serde_json::to_string(&bbox).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #359 GeoJsonSimplifyFn ----
pub struct GeoJsonSimplifyFn;
impl ComputeFunction for GeoJsonSimplifyFn {
    fn id(&self) -> FunctionId { fid("geojson_simplify") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let tolerance: f64 = param_str(p, "tolerance").unwrap_or_else(|| "0.001".into())
            .parse().map_err(|_| ComputeError::InvalidParam("invalid tolerance".into()))?;
        let mut fc = parse_fc(b)?;
        use geo::algorithm::simplify::Simplify;
        for f in &mut fc.features {
            if let Some(ref mut geom) = f.geometry {
                if let Ok(g) = geo::Geometry::<f64>::try_from(geom.value.clone()) {
                    let simplified = match g {
                        geo::Geometry::LineString(ls) => geo::Geometry::LineString(ls.simplify(&tolerance)),
                        geo::Geometry::MultiLineString(mls) => geo::Geometry::MultiLineString(mls.simplify(&tolerance)),
                        geo::Geometry::Polygon(p) => geo::Geometry::Polygon(p.simplify(&tolerance)),
                        geo::Geometry::MultiPolygon(mp) => geo::Geometry::MultiPolygon(mp.simplify(&tolerance)),
                        other => other, // Points etc. can't be simplified
                    };
                    if let Ok(gj_geom) = geojson::Geometry::try_from(&simplified) {
                        *geom = gj_geom;
                    }
                }
            }
        }
        Ok(Bytes::from(serde_json::to_string(&geojson::GeoJson::from(fc)).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #360 GeoJsonToWktFn ----
pub struct GeoJsonToWktFn;
impl ComputeFunction for GeoJsonToWktFn {
    fn id(&self) -> FunctionId { fid("geojson_to_wkt") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let fc = parse_fc(b)?;
        use wkt::ToWkt;
        let mut lines = Vec::new();
        for f in &fc.features {
            if let Some(ref geom) = f.geometry {
                let g: geo::Geometry<f64> = geom.value.clone().try_into()
                    .map_err(|e| fail(format!("geometry convert: {e}")))?;
                lines.push(g.to_wkt().to_string());
            }
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #361 WktToGeoJsonFn ----
pub struct WktToGeoJsonFn;
impl ComputeFunction for WktToGeoJsonFn {
    fn id(&self) -> FunctionId { fid("wkt_to_geojson") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let mut features = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            let wkt_obj: wkt::Wkt<f64> = line.parse()
                .map_err(|e| fail(format!("wkt parse: {e}")))?;
            let g: geo::Geometry<f64> = wkt_obj.try_into()
                .map_err(|e| fail(format!("wkt convert: {e}")))?;
            let gj_geom = geojson::Geometry::try_from(&g)
                .map_err(|e| fail(format!("to geojson: {e}")))?;
            features.push(geojson::Feature {
                geometry: Some(gj_geom), properties: Some(serde_json::Map::new()),
                bbox: None, id: None, foreign_members: None,
            });
        }
        let fc = geojson::FeatureCollection { features, bbox: None, foreign_members: None };
        Ok(Bytes::from(serde_json::to_string(&geojson::GeoJson::from(fc)).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #362 ShapefileToGeoJsonFn ----
pub struct ShapefileToGeoJsonFn;
impl ComputeFunction for ShapefileToGeoJsonFn {
    fn id(&self) -> FunctionId { fid("shapefile_to_geojson") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        // Input is a ZIP containing .shp, .shx, .dbf
        let cursor = std::io::Cursor::new(b.as_ref());
        let mut zip = zip::ZipArchive::new(cursor).map_err(|e| fail(format!("zip: {e}")))?;
        let mut shp_data = None;
        let mut dbf_data = None;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| fail(format!("zip entry: {e}")))?;
            let name = f.name().to_lowercase();
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut buf).map_err(|e| fail(format!("read: {e}")))?;
            if name.ends_with(".shp") { shp_data = Some(buf); }
            else if name.ends_with(".dbf") { dbf_data = Some(buf); }
        }
        let shp = shp_data.ok_or_else(|| fail("missing .shp component".into()))?;
        let dbf = dbf_data.ok_or_else(|| fail("missing .dbf component".into()))?;
        let shp_reader = shapefile::ShapeReader::new(std::io::Cursor::new(shp))
            .map_err(|e| fail(format!("shp: {e}")))?;
        let dbf_reader = shapefile::dbase::Reader::new(std::io::Cursor::new(dbf))
            .map_err(|e| fail(format!("dbf: {e}")))?;
        let mut reader = shapefile::Reader::new(shp_reader, dbf_reader);
        let records = reader.read().map_err(|e| fail(format!("shapefile read: {e}")))?;
        let mut features = Vec::new();
        for (shape, record) in records {
            let geom = shape_to_geojson_geom(&shape)?;
            let mut props = serde_json::Map::new();
            for (name, value) in record {
                let v = match value {
                    shapefile::dbase::FieldValue::Character(Some(s)) => serde_json::Value::String(s),
                    shapefile::dbase::FieldValue::Numeric(Some(n)) => serde_json::json!(n),
                    shapefile::dbase::FieldValue::Float(Some(n)) => serde_json::json!(n),
                    shapefile::dbase::FieldValue::Integer(n) => serde_json::json!(n),
                    shapefile::dbase::FieldValue::Logical(Some(b)) => serde_json::json!(b),
                    _ => serde_json::Value::Null,
                };
                props.insert(name, v);
            }
            features.push(geojson::Feature {
                geometry: Some(geom), properties: Some(props),
                bbox: None, id: None, foreign_members: None,
            });
        }
        let fc = geojson::FeatureCollection { features, bbox: None, foreign_members: None };
        Ok(Bytes::from(serde_json::to_string(&geojson::GeoJson::from(fc)).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn shape_to_geojson_geom(shape: &shapefile::Shape) -> Result<geojson::Geometry, ComputeError> {
    use shapefile::Shape;
    match shape {
        Shape::Point(p) => Ok(geojson::Geometry::new(geojson::Value::Point(vec![p.x, p.y]))),
        Shape::Polyline(pl) => {
            let lines: Vec<Vec<Vec<f64>>> = pl.parts().iter()
                .map(|part| part.iter().map(|p| vec![p.x, p.y]).collect()).collect();
            if lines.len() == 1 {
                Ok(geojson::Geometry::new(geojson::Value::LineString(lines.into_iter().next().unwrap())))
            } else {
                Ok(geojson::Geometry::new(geojson::Value::MultiLineString(lines)))
            }
        }
        Shape::Polygon(pg) => {
            let rings: Vec<Vec<Vec<f64>>> = pg.rings().iter().map(|ring| {
                use shapefile::record::polygon::PolygonRing;
                match ring {
                    PolygonRing::Outer(pts) | PolygonRing::Inner(pts) =>
                        pts.iter().map(|p| vec![p.x, p.y]).collect(),
                }
            }).collect();
            Ok(geojson::Geometry::new(geojson::Value::Polygon(rings)))
        }
        Shape::NullShape => Err(fail("null shape".into())),
        _ => Err(fail(format!("unsupported shape type"))),
    }
}

// ---- #363 KmlToGeoJsonFn ----
pub struct KmlToGeoJsonFn;
impl ComputeFunction for KmlToGeoJsonFn {
    fn id(&self) -> FunctionId { fid("kml_to_geojson") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        // Simple KML parser: extract <coordinates> from <Placemark> elements
        let mut features = Vec::new();
        for placemark in text.split("<Placemark").skip(1) {
            let end = placemark.find("</Placemark").unwrap_or(placemark.len());
            let pm = &placemark[..end];
            // Extract name
            let name = extract_xml_text(pm, "name");
            // Extract coordinates
            if let Some(coords_text) = extract_xml_text(pm, "coordinates") {
                let coords = parse_kml_coords(&coords_text);
                let geom = if coords.len() == 1 {
                    geojson::Geometry::new(geojson::Value::Point(coords[0].clone()))
                } else if pm.contains("<Polygon") || pm.contains("<LinearRing") {
                    geojson::Geometry::new(geojson::Value::Polygon(vec![coords]))
                } else {
                    geojson::Geometry::new(geojson::Value::LineString(coords))
                };
                let mut props = serde_json::Map::new();
                if let Some(n) = name { props.insert("name".into(), serde_json::Value::String(n)); }
                features.push(geojson::Feature {
                    geometry: Some(geom), properties: Some(props),
                    bbox: None, id: None, foreign_members: None,
                });
            }
        }
        let fc = geojson::FeatureCollection { features, bbox: None, foreign_members: None };
        Ok(Bytes::from(serde_json::to_string(&geojson::GeoJson::from(fc)).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn extract_xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)?;
    let after_open = &xml[start..];
    let content_start = after_open.find('>')? + 1;
    let content = &after_open[content_start..];
    let end = content.find(&close)?;
    Some(content[..end].trim().to_string())
}

fn parse_kml_coords(text: &str) -> Vec<Vec<f64>> {
    text.split_whitespace().filter_map(|s| {
        let parts: Vec<f64> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if parts.len() >= 2 { Some(parts) } else { None }
    }).collect()
}

// ---- #364 GpxToGeoJsonFn ----
pub struct GpxToGeoJsonFn;
impl ComputeFunction for GpxToGeoJsonFn {
    fn id(&self) -> FunctionId { fid("gpx_to_geojson") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let mut features = Vec::new();
        // Parse waypoints: <wpt lat="..." lon="...">
        for wpt in text.split("<wpt ").skip(1) {
            let lat = extract_attr(wpt, "lat");
            let lon = extract_attr(wpt, "lon");
            if let (Some(lat), Some(lon)) = (lat, lon) {
                let name = extract_xml_text(wpt, "name");
                let mut props = serde_json::Map::new();
                if let Some(n) = name { props.insert("name".into(), serde_json::Value::String(n)); }
                features.push(geojson::Feature {
                    geometry: Some(geojson::Geometry::new(geojson::Value::Point(vec![lon, lat]))),
                    properties: Some(props), bbox: None, id: None, foreign_members: None,
                });
            }
        }
        // Parse tracks: <trk> → <trkseg> → <trkpt lat="..." lon="...">
        for trk in text.split("<trk>").skip(1) {
            let mut coords = Vec::new();
            for trkpt in trk.split("<trkpt ").skip(1) {
                if let (Some(lat), Some(lon)) = (extract_attr(trkpt, "lat"), extract_attr(trkpt, "lon")) {
                    coords.push(vec![lon, lat]);
                }
            }
            if !coords.is_empty() {
                let name = extract_xml_text(trk, "name");
                let mut props = serde_json::Map::new();
                if let Some(n) = name { props.insert("name".into(), serde_json::Value::String(n)); }
                features.push(geojson::Feature {
                    geometry: Some(geojson::Geometry::new(geojson::Value::LineString(coords))),
                    properties: Some(props), bbox: None, id: None, foreign_members: None,
                });
            }
        }
        let fc = geojson::FeatureCollection { features, bbox: None, foreign_members: None };
        Ok(Bytes::from(serde_json::to_string(&geojson::GeoJson::from(fc)).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn extract_attr(s: &str, attr: &str) -> Option<f64> {
    let pat = format!("{}=\"", attr);
    let start = s.find(&pat)? + pat.len();
    let rest = &s[start..];
    let end = rest.find('"')?;
    rest[..end].parse().ok()
}

// ---- #365 GeoTiffMetadataFn ----
pub struct GeoTiffMetadataFn;
impl ComputeFunction for GeoTiffMetadataFn {
    fn id(&self) -> FunctionId { fid("geotiff_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        // Parse TIFF header for basic metadata (no GDAL needed)
        if b.len() < 8 { return Err(fail("not a TIFF file".into())); }
        let le = &b[0..2] == b"II";
        let be = &b[0..2] == b"MM";
        if !le && !be { return Err(fail("not a TIFF file".into())); }
        let r16 = |off: usize| -> u16 {
            if le { u16::from_le_bytes([b[off], b[off+1]]) }
            else { u16::from_be_bytes([b[off], b[off+1]]) }
        };
        let r32 = |off: usize| -> u32 {
            if le { u32::from_le_bytes([b[off], b[off+1], b[off+2], b[off+3]]) }
            else { u32::from_be_bytes([b[off], b[off+1], b[off+2], b[off+3]]) }
        };
        let magic = r16(2);
        if magic != 42 { return Err(fail("not a TIFF file".into())); }
        let ifd_offset = r32(4) as usize;
        if ifd_offset + 2 > b.len() { return Err(fail("truncated TIFF".into())); }
        let num_entries = r16(ifd_offset) as usize;
        let mut width = 0u32;
        let mut height = 0u32;
        let mut bands = 0u32;
        for i in 0..num_entries {
            let entry_off = ifd_offset + 2 + i * 12;
            if entry_off + 12 > b.len() { break; }
            let tag = r16(entry_off);
            let value = r32(entry_off + 8);
            match tag {
                256 => width = value,   // ImageWidth
                257 => height = value,  // ImageLength
                277 => bands = value,   // SamplesPerPixel
                _ => {}
            }
        }
        let meta = serde_json::json!({
            "width": width, "height": height, "bands": bands,
            "format": "GeoTIFF", "crs": null,
        });
        Ok(Bytes::from(serde_json::to_string_pretty(&meta).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #366 GeoTiffCropFn (requires GDAL) ----
pub struct GeoTiffCropFn;
impl ComputeFunction for GeoTiffCropFn {
    fn id(&self) -> FunctionId { fid("geotiff_crop") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("geotiff_crop requires format-geo-gdal feature".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #367 GeoTiffResampleFn (requires GDAL) ----
pub struct GeoTiffResampleFn;
impl ComputeFunction for GeoTiffResampleFn {
    fn id(&self) -> FunctionId { fid("geotiff_resample") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("geotiff_resample requires format-geo-gdal feature".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #368 FlatGeobufReadFn ----
pub struct FlatGeobufReadFn;
impl ComputeFunction for FlatGeobufReadFn {
    fn id(&self) -> FunctionId { fid("flatgeobuf_read") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut cursor = std::io::Cursor::new(b.as_ref());
        let reader = flatgeobuf::FgbReader::open(&mut cursor)
            .map_err(|e| fail(format!("fgb open: {e}")))?;
        let mut out = Vec::new();
        let mut writer = geozero::geojson::GeoJsonWriter::new(&mut out);
        if let Some(bbox_str) = param_str(p, "bbox") {
            let parts: Vec<f64> = bbox_str.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            if parts.len() == 4 {
                let mut iter = reader.select_bbox(parts[0], parts[1], parts[2], parts[3])
                    .map_err(|e| fail(format!("fgb select: {e}")))?;
                iter.process_features(&mut writer).map_err(|e| fail(format!("fgb process: {e}")))?;
            } else {
                return Err(ComputeError::InvalidParam("bbox must be min_lon,min_lat,max_lon,max_lat".into()));
            }
        } else {
            let mut iter = reader.select_all()
                .map_err(|e| fail(format!("fgb select: {e}")))?;
            iter.process_features(&mut writer).map_err(|e| fail(format!("fgb process: {e}")))?;
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #369 FlatGeobufWriteFn ----
pub struct FlatGeobufWriteFn;
impl ComputeFunction for FlatGeobufWriteFn {
    fn id(&self) -> FunctionId { fid("flatgeobuf_write") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let mut fgb = flatgeobuf::FgbWriter::create("data", flatgeobuf::GeometryType::Unknown)
            .map_err(|e| fail(format!("fgb create: {e}")))?;
        use geozero::GeozeroDatasource;
        let mut reader = geozero::geojson::GeoJsonReader(text.as_bytes());
        reader.process(&mut fgb).map_err(|e| fail(format!("fgb write: {e}")))?;
        let mut out = Vec::new();
        fgb.write(&mut out).map_err(|e| fail(format!("fgb finish: {e}")))?;
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
