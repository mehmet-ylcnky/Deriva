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
            Some(Value::String(s)) => s.parse::<usize>()
                .map_err(|_| ComputeError::InvalidParam("count must be a positive integer".into()))
                .and_then(|n| if n > 0 { Ok(n) } else {
                    Err(ComputeError::InvalidParam("count must be a positive integer".into()))
                })?,
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

pub struct LowercaseFn;

impl ComputeFunction for LowercaseFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("lowercase", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        Ok(Bytes::from(inputs[0].iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<_>>()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size }
    }
}

pub struct ReverseFn;

impl ComputeFunction for ReverseFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("reverse", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mut v = inputs[0].to_vec();
        v.reverse();
        Ok(Bytes::from(v))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size }
    }
}

pub struct Base64EncodeFn;

impl ComputeFunction for Base64EncodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("base64_encode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&inputs[0]);
        Ok(Bytes::from(encoded))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size * 4 / 3 + 4 }
    }
}

pub struct Base64DecodeFn;

impl ComputeFunction for Base64DecodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("base64_decode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(&inputs[0])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("invalid base64: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size * 3 / 4 + 4 }
    }
}

pub struct HexEncodeFn;

impl ComputeFunction for HexEncodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("hex_encode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let hex: String = inputs[0].iter().map(|b| format!("{:02x}", b)).collect();
        Ok(Bytes::from(hex))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size * 2 }
    }
}

pub struct HexDecodeFn;

impl ComputeFunction for HexDecodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("hex_decode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let input = &inputs[0];
        if input.len() % 2 != 0 {
            return Err(ComputeError::ExecutionFailed("hex input must have even length".into()));
        }
        let bytes: Result<Vec<u8>, _> = (0..input.len())
            .step_by(2)
            .map(|i| {
                let s = std::str::from_utf8(&input[i..i + 2])
                    .map_err(|_| ComputeError::ExecutionFailed("invalid hex".into()))?;
                u8::from_str_radix(s, 16)
                    .map_err(|_| ComputeError::ExecutionFailed(format!("invalid hex byte: {}", s)))
            })
            .collect();
        Ok(Bytes::from(bytes?))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size / 2 + 1 }
    }
}

pub struct Base32EncodeFn;

impl ComputeFunction for Base32EncodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("base32_encode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let encoded = data_encoding::BASE32.encode(&inputs[0]);
        Ok(Bytes::from(encoded))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size * 8 / 5 + 8 }
    }
}

pub struct Base32DecodeFn;

impl ComputeFunction for Base32DecodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("base32_decode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        data_encoding::BASE32.decode(&inputs[0])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("invalid base32: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size * 5 / 8 + 5 }
    }
}

pub fn register_all(registry: &mut crate::registry::FunctionRegistry) {
    use std::sync::Arc;
    registry.register(Arc::new(IdentityFn));
    registry.register(Arc::new(ConcatFn));
    registry.register(Arc::new(UppercaseFn));
    registry.register(Arc::new(RepeatFn));
    registry.register(Arc::new(LowercaseFn));
    registry.register(Arc::new(ReverseFn));
    registry.register(Arc::new(Base64EncodeFn));
    registry.register(Arc::new(Base64DecodeFn));
    registry.register(Arc::new(HexEncodeFn));
    registry.register(Arc::new(HexDecodeFn));
    registry.register(Arc::new(Base32EncodeFn));
    registry.register(Arc::new(Base32DecodeFn));
}
