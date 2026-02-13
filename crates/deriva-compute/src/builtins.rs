use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;

pub struct IdentityFn;

impl ComputeFunction for IdentityFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("identity", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        Ok(inputs.into_iter().next().unwrap())
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 0, memory_bytes: 0 }
    }
}

pub struct ConcatFn;

impl ComputeFunction for ConcatFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("concat", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let total: usize = inputs.iter().map(|b| b.len()).sum();
        let mut out = Vec::with_capacity(total);
        for b in inputs {
            out.extend_from_slice(&b);
        }
        Ok(Bytes::from(out))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let total: u64 = input_sizes.iter().sum();
        ComputeCost { cpu_ms: 1, memory_bytes: total }
    }
}

pub struct UppercaseFn;

impl ComputeFunction for UppercaseFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("uppercase", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let s = std::str::from_utf8(&inputs[0])
            .map_err(|e| ComputeError::ExecutionFailed(e.to_string()))?;
        Ok(Bytes::from(s.to_uppercase()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size }
    }
}

pub struct RepeatFn;

impl ComputeFunction for RepeatFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("repeat", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let count = match params.get("count") {
            Some(Value::Int(n)) if *n > 0 => *n as usize,
            _ => return Err(ComputeError::InvalidParam("count must be a positive integer".into())),
        };
        let input = &inputs[0];
        let mut out = Vec::with_capacity(input.len() * count);
        for _ in 0..count {
            out.extend_from_slice(input);
        }
        Ok(Bytes::from(out))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size * 10 }
    }
}
