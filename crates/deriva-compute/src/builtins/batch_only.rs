use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;

pub struct ReverseByteFn;

impl ComputeFunction for ReverseByteFn {
    fn id(&self) -> FunctionId { FunctionId::new("reverse_bytes", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let mut out = inputs[0].to_vec();
        out.reverse();
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── #100 SortBytesFn ──

pub struct SortBytesFn;

impl ComputeFunction for SortBytesFn {
    fn id(&self) -> FunctionId { FunctionId::new("sort_bytes", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let mut out = inputs[0].to_vec();
        out.sort_unstable();
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
}

