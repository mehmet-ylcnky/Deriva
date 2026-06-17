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

// ── SizeCheckFn (size_check@1.0.0) ──

pub struct SizeCheckFn;

impl ComputeFunction for SizeCheckFn {
    fn id(&self) -> FunctionId { FunctionId::new("size_check", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let min: usize = match params.get("min") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("min must be a non-negative integer".into()))?,
            None => 0,
            _ => return Err(ComputeError::InvalidParam("min must be a string".into())),
        };
        let max: Option<usize> = match params.get("max") {
            Some(Value::String(s)) if s == "unlimited" => None,
            Some(Value::String(s)) => Some(s.parse().map_err(|_| ComputeError::InvalidParam("max must be a non-negative integer or \"unlimited\"".into()))?),
            None => None,
            _ => return Err(ComputeError::InvalidParam("max must be a string".into())),
        };
        let size = inputs[0].len();
        if size < min {
            return Err(ComputeError::ExecutionFailed(format!(
                "input size {} is below minimum {}", size, min
            )));
        }
        if let Some(max_val) = max {
            if size > max_val {
                return Err(ComputeError::ExecutionFailed(format!(
                    "input size {} exceeds maximum {}", size, max_val
                )));
            }
        }
        Ok(inputs[0].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── NotEmptyFn (not_empty@1.0.0) ──

pub struct NotEmptyFn;

impl ComputeFunction for NotEmptyFn {
    fn id(&self) -> FunctionId { FunctionId::new("not_empty", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        if inputs[0].is_empty() { return Err(ComputeError::ExecutionFailed("input is empty".into())); }
        Ok(inputs[0].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── RegexMatchFn (regex_match@1.0.0) ──

pub struct RegexMatchFn;

impl ComputeFunction for RegexMatchFn {
    fn id(&self) -> FunctionId { FunctionId::new("regex_match", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let pattern = get_string_param(params, "pattern")?;
        let re = regex::Regex::new(pattern)
            .map_err(|e| ComputeError::InvalidParam(format!("invalid regex pattern: {}", e)))?;
        let text = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("input is not valid UTF-8".into()))?;
        // The entire input must match the pattern
        if let Some(m) = re.find(text) {
            if m.start() == 0 && m.end() == text.len() {
                return Ok(inputs[0].clone());
            }
        }
        Err(ComputeError::ExecutionFailed(format!("input does not match pattern: {}", pattern)))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
}

// ── ContentTypeCheckFn (content_type_check@1.0.0) ──

pub struct ContentTypeCheckFn;

impl ContentTypeCheckFn {
    fn detect_content_type(data: &[u8]) -> &'static str {
        if data.len() >= 4 && data[0..4] == [0x25, 0x50, 0x44, 0x46] {
            return "application/pdf";
        }
        if data.len() >= 4 && data[0..4] == [0x89, 0x50, 0x4E, 0x47] {
            return "image/png";
        }
        if data.len() >= 3 && data[0..3] == [0xFF, 0xD8, 0xFF] {
            return "image/jpeg";
        }
        if data.len() >= 4 && data[0..4] == [0x47, 0x49, 0x46, 0x38] {
            return "image/gif";
        }
        if data.len() >= 4 && data[0..4] == [0x50, 0x4B, 0x03, 0x04] {
            return "application/zip";
        }
        if data.len() >= 2 && data[0..2] == [0x1F, 0x8B] {
            return "application/gzip";
        }
        if data.len() >= 5 && data[0..5] == [0x3C, 0x3F, 0x78, 0x6D, 0x6C] {
            return "application/xml";
        }
        // JSON detection: starts with { or [ (after optional whitespace)
        if let Ok(text) = std::str::from_utf8(data) {
            let trimmed = text.trim_start();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                return "application/json";
            }
        }
        "application/octet-stream"
    }
}

impl ComputeFunction for ContentTypeCheckFn {
    fn id(&self) -> FunctionId { FunctionId::new("content_type_check", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let expected = get_string_param(params, "expected")?;
        let detected = Self::detect_content_type(&inputs[0]);
        if detected == expected {
            Ok(inputs[0].clone())
        } else {
            Err(ComputeError::ExecutionFailed(format!(
                "content type mismatch: expected {}, detected {}", expected, detected
            )))
        }
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #84 JsonPrettyPrintFn ──

