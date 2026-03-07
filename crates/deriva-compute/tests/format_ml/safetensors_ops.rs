use bytes::Bytes;
use deriva_compute::builtins_format_ml::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- safetensors_read (5 tests) ----
#[test]
fn safetensors_read_all() {
    let data = vec![1.0f32, 2.0, 3.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let st = make_safetensors("test", &data);
    let out = SafeTensorsReadFn.execute(vec![st], &p(&[])).unwrap();
    assert_eq!(out.len(), 12);
}

#[test]
fn safetensors_read_by_name() {
    let data = vec![1.0f32, 2.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let st = make_safetensors("test", &data);
    let out = SafeTensorsReadFn.execute(vec![st], &p(&[("tensor", "test")])).unwrap();
    assert_eq!(out.len(), 8);
}

#[test]
fn safetensors_read_invalid() {
    assert!(SafeTensorsReadFn.execute(vec![Bytes::from("bad")], &p(&[])).is_err());
}

#[test]
fn safetensors_read_no_input() {
    assert!(SafeTensorsReadFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn safetensors_read_id() {
    assert_eq!(SafeTensorsReadFn.id().name, "safetensors_read");
}

// ---- safetensors_metadata (5 tests) ----
#[test]
fn safetensors_metadata_basic() {
    let data = vec![1.0f32].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let st = make_safetensors("test", &data);
    let out = SafeTensorsMetadataFn.execute(vec![st], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(json["test"].is_object());
}

#[test]
fn safetensors_metadata_has_dtype() {
    let data = vec![1.0f32].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let st = make_safetensors("test", &data);
    let out = SafeTensorsMetadataFn.execute(vec![st], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["test"]["dtype"], "F32");
}

#[test]
fn safetensors_metadata_invalid() {
    assert!(SafeTensorsMetadataFn.execute(vec![Bytes::from("bad")], &p(&[])).is_err());
}

#[test]
fn safetensors_metadata_no_input() {
    assert!(SafeTensorsMetadataFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn safetensors_metadata_id() {
    assert_eq!(SafeTensorsMetadataFn.id().name, "safetensors_metadata");
}

// ---- safetensors_write (5 tests) ----
#[test]
fn safetensors_write_basic() {
    let data = vec![1.0f32, 2.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let out = SafeTensorsWriteFn.execute(
        vec![Bytes::from(data)],
        &p(&[("name", "test"), ("dtype", "F32"), ("shape", "2")]),
    ).unwrap();
    assert!(out.len() > 8);
}

#[test]
fn safetensors_write_roundtrip() {
    let data = vec![1.0f32, 2.0, 3.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let st = SafeTensorsWriteFn.execute(
        vec![Bytes::from(data.clone())],
        &p(&[("name", "test"), ("dtype", "F32"), ("shape", "3")]),
    ).unwrap();
    let out = SafeTensorsReadFn.execute(vec![st], &p(&[("tensor", "test")])).unwrap();
    assert_eq!(out.as_ref(), &data[..]);
}

#[test]
fn safetensors_write_missing_shape() {
    assert!(SafeTensorsWriteFn.execute(vec![Bytes::from(vec![1, 2])], &p(&[("name", "t")])).is_err());
}

#[test]
fn safetensors_write_no_input() {
    assert!(SafeTensorsWriteFn.execute(vec![], &p(&[("shape", "1")])).is_err());
}

#[test]
fn safetensors_write_id() {
    assert_eq!(SafeTensorsWriteFn.id().name, "safetensors_write");
}
