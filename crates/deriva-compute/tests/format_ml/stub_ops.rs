use bytes::Bytes;
use deriva_compute::builtins_format_ml::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- onnx_metadata (5 tests) ----
#[test]
fn onnx_metadata_requires_lib() { assert!(OnnxMetadataFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn onnx_metadata_id() { assert_eq!(OnnxMetadataFn.id().name, "onnx_metadata"); }
#[test]
fn onnx_metadata_no_input() { assert!(OnnxMetadataFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn onnx_metadata_with_data() { assert!(OnnxMetadataFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn onnx_metadata_error_msg() {
    let err = OnnxMetadataFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("ONNX Runtime"));
}

// ---- onnx_validate (5 tests) ----
#[test]
fn onnx_validate_requires_lib() { assert!(OnnxValidateFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn onnx_validate_id() { assert_eq!(OnnxValidateFn.id().name, "onnx_validate"); }
#[test]
fn onnx_validate_no_input() { assert!(OnnxValidateFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn onnx_validate_with_data() { assert!(OnnxValidateFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn onnx_validate_error_msg() {
    let err = OnnxValidateFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-ml-onnx"));
}

// ---- pickle_to_json (5 tests) ----
#[test]
fn pickle_to_json_requires_lib() { assert!(PickleToJsonFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn pickle_to_json_id() { assert_eq!(PickleToJsonFn.id().name, "pickle_to_json"); }
#[test]
fn pickle_to_json_no_input() { assert!(PickleToJsonFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn pickle_to_json_with_data() { assert!(PickleToJsonFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn pickle_to_json_error_msg() {
    let err = PickleToJsonFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("pickle"));
}
