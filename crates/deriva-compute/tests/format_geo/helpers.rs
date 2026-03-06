use bytes::Bytes;
use deriva_core::address::Value;
use std::collections::BTreeMap;

pub fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

pub const FC_TWO_POINTS: &str = r#"{"type":"FeatureCollection","features":[
  {"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,2.0]},"properties":{"name":"A","pop":"100"}},
  {"type":"Feature","geometry":{"type":"Point","coordinates":[3.0,4.0]},"properties":{"name":"B","pop":"200"}}
]}"#;

pub const FC_POLYGON: &str = r#"{"type":"FeatureCollection","features":[
  {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[10,0],[10,10],[0,10],[0,0]]]},"properties":{"name":"square"}}
]}"#;

pub const FC_LINE: &str = r#"{"type":"FeatureCollection","features":[
  {"type":"Feature","geometry":{"type":"LineString","coordinates":[[0,0],[1,1],[2,0],[3,1],[4,0],[5,1],[6,0],[7,1],[8,0],[9,1],[10,0]]},"properties":{}}
]}"#;

pub fn make_kml() -> Bytes {
    Bytes::from(r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <Placemark>
    <name>Test Point</name>
    <Point><coordinates>-122.0822,37.4220,0</coordinates></Point>
  </Placemark>
  <Placemark>
    <name>Test Line</name>
    <LineString><coordinates>-122.084,37.422,0 -122.085,37.423,0</coordinates></LineString>
  </Placemark>
</Document>
</kml>"#)
}

pub fn make_gpx() -> Bytes {
    Bytes::from(r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1">
  <wpt lat="47.6062" lon="-122.3321"><name>Seattle</name></wpt>
  <trk><name>Trail</name><trkseg>
    <trkpt lat="47.6" lon="-122.3"/>
    <trkpt lat="47.7" lon="-122.4"/>
  </trkseg></trk>
</gpx>"#)
}

/// Create a minimal shapefile ZIP with one point
pub fn make_shapefile_zip() -> Bytes {
    use std::io::Write;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();

        // .shp file: minimal point shapefile
        zip.start_file("test.shp", opts).unwrap();
        // File header (100 bytes)
        let mut shp = Vec::new();
        shp.extend_from_slice(&9994i32.to_be_bytes()); // file code
        shp.extend_from_slice(&[0u8; 20]); // unused
        shp.extend_from_slice(&60i32.to_be_bytes()); // file length in 16-bit words (100+20=120 bytes = 60 words)
        shp.extend_from_slice(&1000i32.to_le_bytes()); // version
        shp.extend_from_slice(&1i32.to_le_bytes()); // shape type: Point
        // Bounding box (8 doubles)
        for v in &[1.0f64, 2.0, 1.0, 2.0, 0.0, 0.0, 0.0, 0.0] {
            shp.extend_from_slice(&v.to_le_bytes());
        }
        // Record: header (8 bytes) + point (20 bytes)
        shp.extend_from_slice(&1i32.to_be_bytes()); // record number
        shp.extend_from_slice(&10i32.to_be_bytes()); // content length in 16-bit words (20 bytes = 10 words)
        shp.extend_from_slice(&1i32.to_le_bytes()); // shape type: Point
        shp.extend_from_slice(&1.0f64.to_le_bytes()); // x
        shp.extend_from_slice(&2.0f64.to_le_bytes()); // y
        zip.write_all(&shp).unwrap();

        // .dbf file: minimal dbase
        zip.start_file("test.dbf", opts).unwrap();
        let mut dbf = Vec::new();
        dbf.push(3); // version
        dbf.extend_from_slice(&[24, 1, 1]); // date YY MM DD
        dbf.extend_from_slice(&1u32.to_le_bytes()); // num records
        let header_size: u16 = 32 + 32 + 1; // header + 1 field + terminator
        dbf.extend_from_slice(&header_size.to_le_bytes());
        dbf.extend_from_slice(&11u16.to_le_bytes()); // record size (1 deletion flag + 10 field)
        dbf.extend_from_slice(&[0u8; 20]); // reserved
        // Field descriptor: "NAME" Character(10)
        let mut field = [0u8; 32];
        field[..4].copy_from_slice(b"NAME");
        field[11] = b'C'; // type
        field[16] = 10; // field length
        dbf.extend_from_slice(&field);
        dbf.push(0x0D); // header terminator
        // Record
        dbf.push(b' '); // deletion flag
        let mut val = b"TestPt    ".to_vec();
        val.truncate(10);
        dbf.extend_from_slice(&val);
        zip.write_all(&dbf).unwrap();

        zip.finish().unwrap();
    }
    Bytes::from(buf.into_inner())
}

/// Create a minimal TIFF (not geo-referenced, just valid TIFF structure)
pub fn make_tiff() -> Bytes {
    let mut buf = Vec::new();
    // Little-endian TIFF header
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&8u32.to_le_bytes()); // IFD offset

    // IFD with 3 entries
    let num_entries: u16 = 3;
    buf.extend_from_slice(&num_entries.to_le_bytes());

    // Entry: ImageWidth (tag 256), SHORT, count 1, value 64
    buf.extend_from_slice(&256u16.to_le_bytes());
    buf.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&64u32.to_le_bytes());

    // Entry: ImageLength (tag 257), SHORT, count 1, value 32
    buf.extend_from_slice(&257u16.to_le_bytes());
    buf.extend_from_slice(&3u16.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&32u32.to_le_bytes());

    // Entry: SamplesPerPixel (tag 277), SHORT, count 1, value 3
    buf.extend_from_slice(&277u16.to_le_bytes());
    buf.extend_from_slice(&3u16.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&3u32.to_le_bytes());

    // Next IFD offset = 0 (no more IFDs)
    buf.extend_from_slice(&0u32.to_le_bytes());

    Bytes::from(buf)
}
