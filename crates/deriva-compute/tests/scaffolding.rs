use deriva_core::error::DerivaError;

#[test]
fn compute_crate_can_use_core_error() {
    let err = DerivaError::ComputeFailed("test".into());
    assert!(err.to_string().contains("compute failed"));
}
