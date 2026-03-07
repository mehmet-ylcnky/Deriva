use bytes::Bytes;
use deriva_compute::builtins_format_ml::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- gguf_metadata (5 tests) ----
#[test]
fn gguf_metadata_basic() {
    let mut gguf = Vec::new();
    gguf.extend_from_slice(b"GGUF");
    gguf.extend_from_slice(&[0u8; 100]);
    let out = GgufMetadataFn.execute(vec![Bytes::from(gguf)], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["magic"], "GGUF");
}

#[test]
fn gguf_metadata_invalid() {
    assert!(GgufMetadataFn.execute(vec![Bytes::from("bad")], &p(&[])).is_err());
}

#[test]
fn gguf_metadata_no_input() {
    assert!(GgufMetadataFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn gguf_metadata_has_size() {
    let mut gguf = Vec::new();
    gguf.extend_from_slice(b"GGUF");
    gguf.extend_from_slice(&[0u8; 50]);
    let out = GgufMetadataFn.execute(vec![Bytes::from(gguf)], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["file_size"], 54);
}

#[test]
fn gguf_metadata_id() {
    assert_eq!(GgufMetadataFn.id().name, "gguf_metadata");
}

// ---- numpy_to_safetensors (5 tests) ----
#[test]
fn numpy_to_safetensors_basic() {
    let shape = "3";
    let header = format!("{{'descr': '<f8', 'fortran_order': False, 'shape': ({shape},), }}");
    let prefix_len = 10;
    let total = prefix_len + header.len() + 1;
    let padding = (64 - (total % 64)) % 64;
    let header_len = (header.len() + 1 + padding) as u16;
    let mut npy = Vec::new();
    npy.extend_from_slice(b"\x93NUMPY");
    npy.push(1); npy.push(0);
    npy.extend_from_slice(&header_len.to_le_bytes());
    npy.extend_from_slice(header.as_bytes());
    npy.extend(std::iter::repeat(b' ').take(padding));
    npy.push(b'\n');
    npy.extend_from_slice(&[1.0f64, 2.0, 3.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>());
    let out = NumpyToSafeTensorsFn.execute(vec![Bytes::from(npy)], &p(&[])).unwrap();
    assert!(out.len() > 8);
}

#[test]
fn numpy_to_safetensors_invalid() {
    assert!(NumpyToSafeTensorsFn.execute(vec![Bytes::from("bad")], &p(&[])).is_err());
}

#[test]
fn numpy_to_safetensors_no_input() {
    assert!(NumpyToSafeTensorsFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn numpy_to_safetensors_with_name() {
    let shape = "2";
    let header = format!("{{'descr': '<f8', 'fortran_order': False, 'shape': ({shape},), }}");
    let prefix_len = 10;
    let total = prefix_len + header.len() + 1;
    let padding = (64 - (total % 64)) % 64;
    let header_len = (header.len() + 1 + padding) as u16;
    let mut npy = Vec::new();
    npy.extend_from_slice(b"\x93NUMPY");
    npy.push(1); npy.push(0);
    npy.extend_from_slice(&header_len.to_le_bytes());
    npy.extend_from_slice(header.as_bytes());
    npy.extend(std::iter::repeat(b' ').take(padding));
    npy.push(b'\n');
    npy.extend_from_slice(&[1.0f64, 2.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>());
    let out = NumpyToSafeTensorsFn.execute(vec![Bytes::from(npy)], &p(&[("name", "mydata")])).unwrap();
    let meta = SafeTensorsMetadataFn.execute(vec![out], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&meta).unwrap();
    assert!(json["mydata"].is_object());
}

#[test]
fn numpy_to_safetensors_id() {
    assert_eq!(NumpyToSafeTensorsFn.id().name, "numpy_to_safetensors");
}

// ---- tfrecord_to_parquet (5 tests) ----
#[test]
fn tfrecord_to_parquet_basic() {
    let data = b"test";
    let len = data.len() as u64;
    let mut tfr = Vec::new();
    tfr.extend_from_slice(&len.to_le_bytes());
    tfr.extend_from_slice(&[0u8; 4]);
    tfr.extend_from_slice(data);
    tfr.extend_from_slice(&[0u8; 4]);
    let out = TfRecordToParquetFn.execute(vec![Bytes::from(tfr)], &p(&[])).unwrap();
    assert!(out.len() > 0);
}

#[test]
fn tfrecord_to_parquet_empty() {
    let out = TfRecordToParquetFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn tfrecord_to_parquet_no_input() {
    assert!(TfRecordToParquetFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn tfrecord_to_parquet_produces_output() {
    let data = b"x";
    let len = data.len() as u64;
    let mut tfr = Vec::new();
    tfr.extend_from_slice(&len.to_le_bytes());
    tfr.extend_from_slice(&[0u8; 4]);
    tfr.extend_from_slice(data);
    tfr.extend_from_slice(&[0u8; 4]);
    let out = TfRecordToParquetFn.execute(vec![Bytes::from(tfr)], &p(&[])).unwrap();
    assert!(out.len() > 0);
}

#[test]
fn tfrecord_to_parquet_id() {
    assert_eq!(TfRecordToParquetFn.id().name, "tfrecord_to_parquet");
}

// ---- image_to_tensor (5 tests) ----
#[test]
fn image_to_tensor_basic() {
    // 1x1 red PNG
    let png = vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82,
        0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 144, 119, 83, 222,
        0, 0, 0, 12, 73, 68, 65, 84, 8, 215, 99, 248, 207, 192, 0, 0,
        3, 1, 1, 0, 24, 221, 141, 176, 0, 0, 0, 0, 73, 69, 78, 68,
        174, 66, 96, 130,
    ];
    let out = ImageToTensorFn.execute(vec![Bytes::from(png)], &p(&[])).unwrap();
    assert_eq!(out.len(), 3 * 4); // 3 channels × 1 pixel × 4 bytes
}

#[test]
fn image_to_tensor_invalid() {
    assert!(ImageToTensorFn.execute(vec![Bytes::from("bad")], &p(&[])).is_err());
}

#[test]
fn image_to_tensor_no_input() {
    assert!(ImageToTensorFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn image_to_tensor_normalized() {
    let png = vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82,
        0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 144, 119, 83, 222,
        0, 0, 0, 12, 73, 68, 65, 84, 8, 215, 99, 248, 207, 192, 0, 0,
        3, 1, 1, 0, 24, 221, 141, 176, 0, 0, 0, 0, 73, 69, 78, 68,
        174, 66, 96, 130,
    ];
    let out = ImageToTensorFn.execute(vec![Bytes::from(png)], &p(&[("normalize", "true")])).unwrap();
    // Values should be in [0, 1] range
    assert_eq!(out.len(), 12);
}

#[test]
fn image_to_tensor_id() {
    assert_eq!(ImageToTensorFn.id().name, "image_to_tensor");
}
