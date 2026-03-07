use bytes::Bytes;
use deriva_compute::builtins_format_binary::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- font_metadata (5 tests) ----
#[test]
fn font_metadata_requires_lib() { assert!(FontMetadataFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn font_metadata_id() { assert_eq!(FontMetadataFn.id().name, "font_metadata"); }
#[test]
fn font_metadata_no_input() { assert!(FontMetadataFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn font_metadata_with_data() { assert!(FontMetadataFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn font_metadata_error_msg() {
    let err = FontMetadataFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("font"));
}

// ---- woff_to_otf (5 tests) ----
#[test]
fn woff_to_otf_requires_lib() { assert!(WoffToOtfFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn woff_to_otf_id() { assert_eq!(WoffToOtfFn.id().name, "woff_to_otf"); }
#[test]
fn woff_to_otf_no_input() { assert!(WoffToOtfFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn woff_to_otf_with_data() { assert!(WoffToOtfFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn woff_to_otf_error_msg() {
    let err = WoffToOtfFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("woff2"));
}

// ---- otf_to_woff2 (5 tests) ----
#[test]
fn otf_to_woff2_requires_lib() { assert!(OtfToWoff2Fn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn otf_to_woff2_id() { assert_eq!(OtfToWoff2Fn.id().name, "otf_to_woff2"); }
#[test]
fn otf_to_woff2_no_input() { assert!(OtfToWoff2Fn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn otf_to_woff2_with_data() { assert!(OtfToWoff2Fn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn otf_to_woff2_error_msg() {
    let err = OtfToWoff2Fn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-binary-font"));
}

// ---- gltf_metadata (5 tests) ----
#[test]
fn gltf_metadata_requires_lib() { assert!(GltfMetadataFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn gltf_metadata_id() { assert_eq!(GltfMetadataFn.id().name, "gltf_metadata"); }
#[test]
fn gltf_metadata_no_input() { assert!(GltfMetadataFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn gltf_metadata_with_data() { assert!(GltfMetadataFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn gltf_metadata_error_msg() {
    let err = GltfMetadataFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("gltf"));
}

// ---- gltf_validate (5 tests) ----
#[test]
fn gltf_validate_requires_lib() { assert!(GltfValidateFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn gltf_validate_id() { assert_eq!(GltfValidateFn.id().name, "gltf_validate"); }
#[test]
fn gltf_validate_no_input() { assert!(GltfValidateFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn gltf_validate_with_data() { assert!(GltfValidateFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn gltf_validate_error_msg() {
    let err = GltfValidateFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-binary-gltf"));
}

// ---- dicom_metadata (5 tests) ----
#[test]
fn dicom_metadata_requires_lib() { assert!(DicomMetadataFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn dicom_metadata_id() { assert_eq!(DicomMetadataFn.id().name, "dicom_metadata"); }
#[test]
fn dicom_metadata_no_input() { assert!(DicomMetadataFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn dicom_metadata_with_tags() { assert!(DicomMetadataFn.execute(vec![Bytes::new()], &p(&[("tags", "PatientName")])).is_err()); }
#[test]
fn dicom_metadata_error_msg() {
    let err = DicomMetadataFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("dicom-object"));
}

// ---- dicom_to_image (5 tests) ----
#[test]
fn dicom_to_image_requires_lib() { assert!(DicomToImageFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn dicom_to_image_id() { assert_eq!(DicomToImageFn.id().name, "dicom_to_image"); }
#[test]
fn dicom_to_image_no_input() { assert!(DicomToImageFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn dicom_to_image_with_format() { assert!(DicomToImageFn.execute(vec![Bytes::new()], &p(&[("format", "png")])).is_err()); }
#[test]
fn dicom_to_image_error_msg() {
    let err = DicomToImageFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-binary-dicom"));
}

// ---- dicom_anonymize (5 tests) ----
#[test]
fn dicom_anonymize_requires_lib() { assert!(DicomAnonymizeFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn dicom_anonymize_id() { assert_eq!(DicomAnonymizeFn.id().name, "dicom_anonymize"); }
#[test]
fn dicom_anonymize_no_input() { assert!(DicomAnonymizeFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn dicom_anonymize_with_keep() { assert!(DicomAnonymizeFn.execute(vec![Bytes::new()], &p(&[("keep_tags", "StudyDate")])).is_err()); }
#[test]
fn dicom_anonymize_error_msg() {
    let err = DicomAnonymizeFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("dicom"));
}
