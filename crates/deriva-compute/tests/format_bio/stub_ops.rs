use bytes::Bytes;
use deriva_compute::builtins_format_bio::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- bam_read (5 tests) ----
#[test]
fn bam_read_requires_lib() { assert!(BamReadFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn bam_read_id() { assert_eq!(BamReadFn.id().name, "bam_read"); }
#[test]
fn bam_read_no_input() { assert!(BamReadFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn bam_read_with_region() { assert!(BamReadFn.execute(vec![Bytes::new()], &p(&[("region", "chr1:1000-2000")])).is_err()); }
#[test]
fn bam_read_error_msg() {
    let err = BamReadFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("noodles"));
}

// ---- bam_to_sam (5 tests) ----
#[test]
fn bam_to_sam_requires_lib() { assert!(BamToSamFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn bam_to_sam_id() { assert_eq!(BamToSamFn.id().name, "bam_to_sam"); }
#[test]
fn bam_to_sam_no_input() { assert!(BamToSamFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn bam_to_sam_with_data() { assert!(BamToSamFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn bam_to_sam_error_msg() {
    let err = BamToSamFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-bio-bam"));
}
