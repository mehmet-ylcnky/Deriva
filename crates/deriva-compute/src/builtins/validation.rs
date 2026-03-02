use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;
use super::{get_string_param, hex_decode_param, parse_usize_param};

pub struct Utf8ValidateFn;

impl ComputeFunction for Utf8ValidateFn {
    fn id(&self) -> FunctionId { FunctionId::new("utf8_validate", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        match std::str::from_utf8(&inputs[0]) {
            Ok(_) => Ok(inputs[0].clone()),
            Err(e) => Err(ComputeError::ExecutionFailed(format!("invalid UTF-8 at byte offset {}", e.valid_up_to()))),
        }
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #77 JsonValidateFn ──

pub struct JsonValidateFn;

impl ComputeFunction for JsonValidateFn {
    fn id(&self) -> FunctionId { FunctionId::new("json_validate", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("JSON must be UTF-8".into()))?;
        serde_json::from_str::<serde_json::Value>(text).map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
        Ok(inputs[0].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #78 SchemaValidateFn ──

pub struct SchemaValidateFn;

impl ComputeFunction for SchemaValidateFn {
    fn id(&self) -> FunctionId { FunctionId::new("schema_validate", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let schema_str = get_string_param(params, "schema")?;
        let schema: serde_json::Value = serde_json::from_str(schema_str).map_err(|e| ComputeError::InvalidParam(format!("invalid schema JSON: {}", e)))?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("input must be UTF-8".into()))?;
        let instance: serde_json::Value = serde_json::from_str(text).map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
        let validator = jsonschema::validator_for(&schema).map_err(|e| ComputeError::InvalidParam(format!("invalid schema: {}", e)))?;
        let errors: Vec<String> = validator.iter_errors(&instance).map(|e| format!("{}: {}", e.instance_path, e)).collect();
        if errors.is_empty() { Ok(inputs[0].clone()) }
        else { Err(ComputeError::ExecutionFailed(format!("schema validation failed:\n{}", errors.join("\n")))) }
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(200, input_sizes) }
}

// ── #79 MagicBytesFn ──

pub struct MagicBytesFn;

impl ComputeFunction for MagicBytesFn {
    fn id(&self) -> FunctionId { FunctionId::new("magic_bytes", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let hex = get_string_param(params, "expected")?;
        let expected = hex_decode_param(hex, "expected")?;
        let input = &inputs[0];
        if input.len() < expected.len() {
            return Err(ComputeError::ExecutionFailed(format!("input too short: {} bytes, expected at least {}", input.len(), expected.len())));
        }
        if &input[..expected.len()] != expected.as_slice() {
            return Err(ComputeError::ExecutionFailed(format!("magic bytes mismatch: expected {}, got {}",
                hex, input[..expected.len()].iter().map(|b| format!("{:02x}", b)).collect::<String>())));
        }
        Ok(inputs[0].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #80 SizeLimitFn ──

pub struct SizeLimitFn;

impl ComputeFunction for SizeLimitFn {
    fn id(&self) -> FunctionId { FunctionId::new("size_limit", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let max: usize = parse_usize_param(params, "max_bytes")?;
        if inputs[0].len() > max {
            return Err(ComputeError::ExecutionFailed(format!("input size {} exceeds limit {}", inputs[0].len(), max)));
        }
        Ok(inputs[0].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── #81 NonEmptyFn ──

pub struct NonEmptyFn;

impl ComputeFunction for NonEmptyFn {
    fn id(&self) -> FunctionId { FunctionId::new("non_empty", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        if inputs[0].is_empty() { return Err(ComputeError::ExecutionFailed("input is empty".into())); }
        Ok(inputs[0].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── #82 Sha256VerifyFn ──

pub struct Sha256VerifyFn;

impl ComputeFunction for Sha256VerifyFn {
    fn id(&self) -> FunctionId { FunctionId::new("sha256_verify", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        use sha2::{Sha256, Digest};
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let expected_hex = get_string_param(params, "expected_hash")?;
        let expected = hex_decode_param(expected_hex, "expected_hash")?;
        if expected.len() != 32 { return Err(ComputeError::InvalidParam("expected_hash must be 32 bytes (64 hex chars)".into())); }
        let actual = Sha256::digest(&inputs[0]);
        if actual.as_slice() != expected.as_slice() {
            return Err(ComputeError::ExecutionFailed(format!("SHA-256 mismatch: expected {}, got {}",
                expected_hex, actual.iter().map(|b| format!("{:02x}", b)).collect::<String>())));
        }
        Ok(inputs[0].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #83 Crc32VerifyFn ──

pub struct Crc32VerifyFn;

impl ComputeFunction for Crc32VerifyFn {
    fn id(&self) -> FunctionId { FunctionId::new("crc32_verify", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let expected_hex = get_string_param(params, "expected_crc32")?;
        let expected_bytes = hex_decode_param(expected_hex, "expected_crc32")?;
        if expected_bytes.len() != 4 { return Err(ComputeError::InvalidParam("expected_crc32 must be 4 bytes (8 hex chars)".into())); }
        let expected = u32::from_be_bytes([expected_bytes[0], expected_bytes[1], expected_bytes[2], expected_bytes[3]]);
        let actual = crc32fast::hash(&inputs[0]);
        if actual != expected {
            return Err(ComputeError::ExecutionFailed(format!("CRC32 mismatch: expected {:08x}, got {:08x}", expected, actual)));
        }
        Ok(inputs[0].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #84 JsonPrettyPrintFn ──

