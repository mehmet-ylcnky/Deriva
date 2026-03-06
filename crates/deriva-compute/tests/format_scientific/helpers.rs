use bytes::Bytes;
use deriva_core::address::Value;
use std::collections::BTreeMap;
use std::io::Write;

pub fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

/// Create a .npy file with f64 data
pub fn make_npy_f64(values: &[f64]) -> Bytes {
    let shape = format!("{}", values.len());
    let header = format!("{{'descr': '<f8', 'fortran_order': False, 'shape': ({shape},), }}");
    let prefix_len = 10;
    let total = prefix_len + header.len() + 1;
    let padding = (64 - (total % 64)) % 64;
    let header_len = (header.len() + 1 + padding) as u16;

    let mut buf = Vec::new();
    buf.extend_from_slice(b"\x93NUMPY");
    buf.push(1); buf.push(0);
    buf.extend_from_slice(&header_len.to_le_bytes());
    buf.extend_from_slice(header.as_bytes());
    buf.extend(std::iter::repeat(b' ').take(padding));
    buf.push(b'\n');
    for &v in values { buf.extend_from_slice(&v.to_le_bytes()); }
    Bytes::from(buf)
}

/// Create a .npy file with i32 data
pub fn make_npy_i32(values: &[i32]) -> Bytes {
    let shape = format!("{}", values.len());
    let header = format!("{{'descr': '<i4', 'fortran_order': False, 'shape': ({shape},), }}");
    let prefix_len = 10;
    let total = prefix_len + header.len() + 1;
    let padding = (64 - (total % 64)) % 64;
    let header_len = (header.len() + 1 + padding) as u16;

    let mut buf = Vec::new();
    buf.extend_from_slice(b"\x93NUMPY");
    buf.push(1); buf.push(0);
    buf.extend_from_slice(&header_len.to_le_bytes());
    buf.extend_from_slice(header.as_bytes());
    buf.extend(std::iter::repeat(b' ').take(padding));
    buf.push(b'\n');
    for &v in values { buf.extend_from_slice(&v.to_le_bytes()); }
    Bytes::from(buf)
}

/// Create a minimal Zarr store as ZIP with one f64 array
pub fn make_zarr_zip(values: &[f64]) -> Bytes {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();

        // .zarray metadata
        zip.start_file(".zarray", opts).unwrap();
        let meta = serde_json::json!({
            "shape": [values.len()],
            "chunks": [values.len()],
            "dtype": "<f8",
            "compressor": null,
            "fill_value": 0.0,
            "order": "C",
            "zarr_format": 2,
        });
        zip.write_all(serde_json::to_string(&meta).unwrap().as_bytes()).unwrap();

        // Single chunk "0"
        zip.start_file("0", opts).unwrap();
        for &v in values { zip.write_all(&v.to_le_bytes()).unwrap(); }

        zip.finish().unwrap();
    }
    Bytes::from(buf.into_inner())
}
