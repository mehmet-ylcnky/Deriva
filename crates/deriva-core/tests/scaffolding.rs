use deriva_core::error::{DerivaError, Result};

#[test]
fn error_display_cycle() {
    let err = DerivaError::CycleDetected("A -> B -> A".into());
    assert!(err.to_string().contains("cycle detected"));
    assert!(err.to_string().contains("A -> B -> A"));
}

#[test]
fn error_display_not_found() {
    let err = DerivaError::NotFound("abc123".into());
    assert!(err.to_string().contains("not found"));
}

#[test]
fn error_display_function_not_found() {
    let err = DerivaError::FunctionNotFound("compress/1.0.0".into());
    assert!(err.to_string().contains("not registered"));
}

#[test]
fn error_display_compute_failed() {
    let err = DerivaError::ComputeFailed("out of memory".into());
    assert!(err.to_string().contains("compute failed"));
}

#[test]
fn error_display_storage() {
    let err = DerivaError::Storage("disk full".into());
    assert!(err.to_string().contains("storage error"));
}

#[test]
fn error_display_serialization() {
    let err = DerivaError::Serialization("invalid bytes".into());
    assert!(err.to_string().contains("serialization error"));
}

#[test]
fn result_type_alias_works() {
    fn ok_fn() -> Result<u32> { Ok(42) }
    fn err_fn() -> Result<u32> { Err(DerivaError::NotFound("x".into())) }
    assert_eq!(ok_fn().unwrap(), 42);
    assert!(err_fn().is_err());
}
