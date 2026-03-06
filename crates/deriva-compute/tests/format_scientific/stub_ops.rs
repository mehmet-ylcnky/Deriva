use bytes::Bytes;
use deriva_compute::builtins_format_scientific::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- hdf5_metadata (5 tests) ----
#[test]
fn hdf5_metadata_requires_lib() {
    let err = Hdf5MetadataFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("hdf5"));
}
#[test]
fn hdf5_metadata_id() { assert_eq!(Hdf5MetadataFn.id().name, "hdf5_metadata"); }
#[test]
fn hdf5_metadata_no_input() { assert!(Hdf5MetadataFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn hdf5_metadata_with_data() { assert!(Hdf5MetadataFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn hdf5_metadata_error_msg() {
    let err = Hdf5MetadataFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("libhdf5"));
}

// ---- hdf5_read (5 tests) ----
#[test]
fn hdf5_read_requires_lib() { assert!(Hdf5ReadFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn hdf5_read_id() { assert_eq!(Hdf5ReadFn.id().name, "hdf5_read"); }
#[test]
fn hdf5_read_no_input() { assert!(Hdf5ReadFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn hdf5_read_with_dataset() { assert!(Hdf5ReadFn.execute(vec![Bytes::new()], &p(&[("dataset", "/data")])).is_err()); }
#[test]
fn hdf5_read_error_msg() {
    let err = Hdf5ReadFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-scientific-hdf5"));
}

// ---- hdf5_write (5 tests) ----
#[test]
fn hdf5_write_requires_lib() { assert!(Hdf5WriteFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn hdf5_write_id() { assert_eq!(Hdf5WriteFn.id().name, "hdf5_write"); }
#[test]
fn hdf5_write_no_input() { assert!(Hdf5WriteFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn hdf5_write_with_params() { assert!(Hdf5WriteFn.execute(vec![Bytes::new()], &p(&[("dataset", "/d"), ("shape", "3"), ("dtype", "f64")])).is_err()); }
#[test]
fn hdf5_write_error_msg() {
    let err = Hdf5WriteFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("libhdf5"));
}

// ---- netcdf_metadata (5 tests) ----
#[test]
fn netcdf_metadata_requires_lib() { assert!(NetcdfMetadataFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn netcdf_metadata_id() { assert_eq!(NetcdfMetadataFn.id().name, "netcdf_metadata"); }
#[test]
fn netcdf_metadata_no_input() { assert!(NetcdfMetadataFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn netcdf_metadata_with_data() { assert!(NetcdfMetadataFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn netcdf_metadata_error_msg() {
    let err = NetcdfMetadataFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("libnetcdf"));
}

// ---- netcdf_read (5 tests) ----
#[test]
fn netcdf_read_requires_lib() { assert!(NetcdfReadFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn netcdf_read_id() { assert_eq!(NetcdfReadFn.id().name, "netcdf_read"); }
#[test]
fn netcdf_read_no_input() { assert!(NetcdfReadFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn netcdf_read_with_variable() { assert!(NetcdfReadFn.execute(vec![Bytes::new()], &p(&[("variable", "temp")])).is_err()); }
#[test]
fn netcdf_read_error_msg() {
    let err = NetcdfReadFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-scientific-netcdf"));
}

// ---- fits_metadata (5 tests) ----
#[test]
fn fits_metadata_requires_lib() { assert!(FitsMetadataFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn fits_metadata_id() { assert_eq!(FitsMetadataFn.id().name, "fits_metadata"); }
#[test]
fn fits_metadata_no_input() { assert!(FitsMetadataFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn fits_metadata_with_data() { assert!(FitsMetadataFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn fits_metadata_error_msg() {
    let err = FitsMetadataFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("cfitsio"));
}

// ---- fits_read (5 tests) ----
#[test]
fn fits_read_requires_lib() { assert!(FitsReadFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn fits_read_id() { assert_eq!(FitsReadFn.id().name, "fits_read"); }
#[test]
fn fits_read_no_input() { assert!(FitsReadFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn fits_read_with_hdu() { assert!(FitsReadFn.execute(vec![Bytes::new()], &p(&[("hdu", "0")])).is_err()); }
#[test]
fn fits_read_error_msg() {
    let err = FitsReadFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-scientific-fits"));
}
