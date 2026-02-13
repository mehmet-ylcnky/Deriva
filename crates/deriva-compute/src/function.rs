use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComputeCost {
    pub cpu_ms: u64,
    pub memory_bytes: u64,
}

pub trait ComputeFunction: Send + Sync {
    fn id(&self) -> FunctionId;

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError>;

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost;
}

impl std::fmt::Debug for dyn ComputeFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ComputeFunction({})", self.id())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ComputeError {
    #[error("invalid input count: expected {expected}, got {got}")]
    InputCount { expected: usize, got: usize },

    #[error("invalid parameter: {0}")]
    InvalidParam(String),

    #[error("execution failed: {0}")]
    ExecutionFailed(String),
}
