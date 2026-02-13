use thiserror::Error;

#[derive(Debug, Error)]
pub enum DerivaError {
    #[error("cycle detected in DAG: {0}")]
    CycleDetected(String),

    #[error("address not found: {0}")]
    NotFound(String),

    #[error("function not registered: {0}")]
    FunctionNotFound(String),

    #[error("compute failed: {0}")]
    ComputeFailed(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("serialization error: {0}")]
    Serialization(String),
}

pub type Result<T> = std::result::Result<T, DerivaError>;
