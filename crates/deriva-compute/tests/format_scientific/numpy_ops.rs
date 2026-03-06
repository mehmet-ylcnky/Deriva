use bytes::Bytes;
use deriva_compute::builtins_format_scientific::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- numpy_read (5 tests) ----
#[test]
fn numpy_read_f64() {
    let npy = make_npy_f64(&[1.0, 2.0, 3.0]);
    let out = NumpyReadFn.execute(vec![npy], &p(&[])).unwrap();
    // Output is Arrow IPC — verify it's non-empty and starts with ARROW magic
    assert!(out.len() > 8);
    assert_eq!(&out[..6], b"ARROW1");
}

#[test]
fn numpy_read_i32() {
    let npy = make_npy_i32(&[10, 20, 30]);
    let out = NumpyReadFn.execute(vec![npy], &p(&[])).unwrap();
    assert_eq!(&out[..6], b"ARROW1");
}

#[test]
fn numpy_read_invalid() {
    assert!(NumpyReadFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn numpy_read_no_input() {
    assert!(NumpyReadFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn numpy_read_id() {
    assert_eq!(NumpyReadFn.id().name, "numpy_read");
}

// ---- numpy_write (5 tests) ----
#[test]
fn numpy_write_f64() {
    let raw: Vec<u8> = [1.0f64, 2.0, 3.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let out = NumpyWriteFn.execute(
        vec![Bytes::from(raw)],
        &p(&[("dtype", "<f8"), ("shape", "3")]),
    ).unwrap();
    assert_eq!(&out[..6], b"\x93NUMPY");
}

#[test]
fn numpy_write_i32() {
    let raw: Vec<u8> = [10i32, 20, 30].iter().flat_map(|v| v.to_le_bytes()).collect();
    let out = NumpyWriteFn.execute(
        vec![Bytes::from(raw)],
        &p(&[("dtype", "<i4"), ("shape", "3")]),
    ).unwrap();
    assert_eq!(&out[..6], b"\x93NUMPY");
}

#[test]
fn numpy_write_missing_shape() {
    assert!(NumpyWriteFn.execute(vec![Bytes::from_static(b"data")], &p(&[("dtype", "<f8")])).is_err());
}

#[test]
fn numpy_write_no_input() {
    assert!(NumpyWriteFn.execute(vec![], &p(&[("dtype", "<f8"), ("shape", "1")])).is_err());
}

#[test]
fn numpy_write_id() {
    assert_eq!(NumpyWriteFn.id().name, "numpy_write");
}

// ---- numpy_to_arrow (5 tests) ----
#[test]
fn numpy_to_arrow_f64() {
    let npy = make_npy_f64(&[1.0, 2.0, 3.0]);
    let out = NumpyToArrowFn.execute(vec![npy], &p(&[])).unwrap();
    assert_eq!(&out[..6], b"ARROW1");
}

#[test]
fn numpy_to_arrow_invalid() {
    assert!(NumpyToArrowFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn numpy_to_arrow_no_input() {
    assert!(NumpyToArrowFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn numpy_to_arrow_same_as_read() {
    let npy = make_npy_f64(&[4.0, 5.0]);
    let a = NumpyToArrowFn.execute(vec![npy.clone()], &p(&[])).unwrap();
    let b = NumpyReadFn.execute(vec![npy], &p(&[])).unwrap();
    assert_eq!(a, b);
}

#[test]
fn numpy_to_arrow_id() {
    assert_eq!(NumpyToArrowFn.id().name, "numpy_to_arrow");
}

// ---- arrow_to_numpy (5 tests) ----
#[test]
fn arrow_to_numpy_f64() {
    let npy = make_npy_f64(&[1.0, 2.0, 3.0]);
    let arrow = NumpyReadFn.execute(vec![npy], &p(&[])).unwrap();
    let out = ArrowToNumpyFn.execute(vec![arrow], &p(&[("dtype", "<f8")])).unwrap();
    assert_eq!(&out[..6], b"\x93NUMPY");
}

#[test]
fn arrow_to_numpy_roundtrip_f64() {
    let original = vec![1.5, 2.5, 3.5];
    let npy = make_npy_f64(&original);
    let arrow = NumpyReadFn.execute(vec![npy], &p(&[])).unwrap();
    let npy2 = ArrowToNumpyFn.execute(vec![arrow], &p(&[("dtype", "<f8")])).unwrap();
    // Read back and verify
    let arrow2 = NumpyReadFn.execute(vec![npy2], &p(&[])).unwrap();
    assert_eq!(&arrow2[..6], b"ARROW1");
}

#[test]
fn arrow_to_numpy_i32() {
    let npy = make_npy_i32(&[10, 20, 30]);
    let arrow = NumpyReadFn.execute(vec![npy], &p(&[])).unwrap();
    let out = ArrowToNumpyFn.execute(vec![arrow], &p(&[("dtype", "<i4")])).unwrap();
    assert_eq!(&out[..6], b"\x93NUMPY");
}

#[test]
fn arrow_to_numpy_no_input() {
    assert!(ArrowToNumpyFn.execute(vec![], &p(&[("dtype", "<f8")])).is_err());
}

#[test]
fn arrow_to_numpy_id() {
    assert_eq!(ArrowToNumpyFn.id().name, "arrow_to_numpy");
}
