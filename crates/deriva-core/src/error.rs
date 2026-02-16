use thiserror::Error;

#[derive(Debug, Error, Clone)]
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

    #[error("determinism violation for {addr}: function {function_id} produced different outputs ({output_1_len} bytes hash={output_1_hash} vs {output_2_len} bytes hash={output_2_hash})")]
    DeterminismViolation {
        addr: String,
        function_id: String,
        output_1_hash: String,
        output_2_hash: String,
        output_1_len: usize,
        output_2_len: usize,
    },
}

pub type Result<T> = std::result::Result<T, DerivaError>;
