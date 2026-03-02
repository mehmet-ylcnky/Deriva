use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;

fn parse_byte_param(params: &BTreeMap<String, Value>, name: &str) -> Result<u8, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam(format!("{} must be 0-255", name))),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

fn parse_usize_param(params: &BTreeMap<String, Value>, name: &str) -> Result<usize, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam(format!("{} must be a positive integer", name))),
        Some(Value::Int(n)) if *n > 0 => Ok(*n as usize),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

fn parse_u64_param(params: &BTreeMap<String, Value>, name: &str) -> Result<u64, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => s.parse::<u64>().map_err(|_| ComputeError::InvalidParam(format!("{} must be a non-negative integer", name))),
        Some(Value::Int(n)) => u64::try_from(*n).map_err(|_| ComputeError::InvalidParam(format!("{} must be non-negative", name))),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

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
        let input = &inputs[0];
        // Validate: Base32 length must be multiple of 8 and must contain proper padding
        if !input.is_empty() {
            if input.len() % 8 != 0 {
                return Err(ComputeError::ExecutionFailed("invalid base32: input length must be a multiple of 8".into()));
            }
            // Validate padding: non-padding chars after padding are invalid,
            // and data bytes in padding positions must be '='
            let data_len = input.iter().position(|&b| b == b'=').unwrap_or(input.len());
            let pad_len = input.len() - data_len;
            // Valid padding lengths for Base32: 0, 1, 3, 4, 6
            if !matches!(pad_len, 0 | 1 | 3 | 4 | 6) {
                return Err(ComputeError::ExecutionFailed("invalid base32: wrong padding length".into()));
            }
            // If data_len is not at a valid boundary, padding is required
            let expected_pad = match data_len % 8 {
                0 => 0,
                2 => 6,
                4 => 4,
                5 => 3,
                7 => 1,
                _ => return Err(ComputeError::ExecutionFailed("invalid base32: invalid data length".into())),
            };
            if pad_len != expected_pad {
                return Err(ComputeError::ExecutionFailed("invalid base32: incorrect padding".into()));
            }
        }
        data_encoding::BASE32.decode(input)
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("invalid base32: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size * 5 / 8 + 5 }
    }
}

pub struct XorFn;

impl ComputeFunction for XorFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("xor", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let key: u8 = match params.get("key") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("key must be 0-255".into()))?,
            _ => return Err(ComputeError::InvalidParam("missing param: key".into())),
        };
        Ok(Bytes::from(inputs[0].iter().map(|b| b ^ key).collect::<Vec<_>>()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size }
    }
}

pub struct BitwiseAndFn;

impl ComputeFunction for BitwiseAndFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("bitwise_and", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mask = parse_byte_param(params, "mask")?;
        Ok(Bytes::from(inputs[0].iter().map(|b| b & mask).collect::<Vec<_>>()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size }
    }
}

pub struct BitwiseOrFn;

impl ComputeFunction for BitwiseOrFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("bitwise_or", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mask = parse_byte_param(params, "mask")?;
        Ok(Bytes::from(inputs[0].iter().map(|b| b | mask).collect::<Vec<_>>()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size }
    }
}

pub struct BitwiseNotFn;

impl ComputeFunction for BitwiseNotFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("bitwise_not", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        Ok(Bytes::from(inputs[0].iter().map(|b| !b).collect::<Vec<_>>()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size }
    }
}

pub struct ByteSwapFn;

impl ComputeFunction for ByteSwapFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("byte_swap", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let ws: usize = match params.get("word_size") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("word_size must be 2, 4, or 8".into()))?,
            _ => return Err(ComputeError::InvalidParam("missing param: word_size".into())),
        };
        if !matches!(ws, 2 | 4 | 8) {
            return Err(ComputeError::InvalidParam("word_size must be 2, 4, or 8".into()));
        }
        let input = &inputs[0];
        if input.len() % ws != 0 {
            return Err(ComputeError::ExecutionFailed(
                format!("input length {} not a multiple of word_size {}", input.len(), ws),
            ));
        }
        let mut out = input.to_vec();
        for chunk in out.chunks_mut(ws) {
            chunk.reverse();
        }
        Ok(Bytes::from(out))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size }
    }
}

pub struct TrimFn;

impl ComputeFunction for TrimFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("trim", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let input = &inputs[0];
        let start = input.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(input.len());
        let end = input.iter().rposition(|b| !b.is_ascii_whitespace()).map(|p| p + 1).unwrap_or(start);
        Ok(inputs[0].slice(start..end))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size }
    }
}

pub struct PadFn;

impl ComputeFunction for PadFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("pad", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let block_size = parse_usize_param(params, "block_size")?;
        if block_size == 0 || block_size > 256 {
            return Err(ComputeError::InvalidParam("block_size must be 1-256".into()));
        }
        let input = &inputs[0];
        let remainder = input.len() % block_size;
        let pad_len = if remainder == 0 { block_size } else { block_size - remainder };
        let mut out = input.to_vec();
        out.extend(std::iter::repeat(pad_len as u8).take(pad_len));
        Ok(Bytes::from(out))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size + 256 }
    }
}

pub struct LineEndingFn;

impl ComputeFunction for LineEndingFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("line_ending", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let target = match params.get("target") {
            Some(Value::String(s)) => s.as_str(),
            _ => return Err(ComputeError::InvalidParam("missing param: target".into())),
        };
        let input = &inputs[0];
        match target {
            "lf" => {
                let mut out = Vec::with_capacity(input.len());
                let mut i = 0;
                while i < input.len() {
                    if i + 1 < input.len() && input[i] == b'\r' && input[i + 1] == b'\n' {
                        out.push(b'\n');
                        i += 2;
                    } else {
                        out.push(input[i]);
                        i += 1;
                    }
                }
                Ok(Bytes::from(out))
            }
            "crlf" => {
                let mut out = Vec::with_capacity(input.len());
                for (i, &b) in input.iter().enumerate() {
                    if b == b'\n' && (i == 0 || input[i - 1] != b'\r') {
                        out.push(b'\r');
                    }
                    out.push(b);
                }
                Ok(Bytes::from(out))
            }
            _ => Err(ComputeError::InvalidParam("target must be 'lf' or 'crlf'".into())),
        }
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size * 2 }
    }
}

// ── #21 CompressFn (zlib) ──

pub struct CompressFn;

impl ComputeFunction for CompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("compress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&inputs[0])
            .map_err(|e| ComputeError::ExecutionFailed(format!("compress: {}", e)))?;
        let compressed = encoder.finish()
            .map_err(|e| ComputeError::ExecutionFailed(format!("compress finish: {}", e)))?;
        Ok(Bytes::from(compressed))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 50_000 + 1, memory_bytes: size * 2 }
    }
}

// ── #22 DecompressFn (zlib) ──

pub struct DecompressFn;

impl ComputeFunction for DecompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("decompress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use flate2::read::ZlibDecoder;
        use std::io::Read;
        let mut decoder = ZlibDecoder::new(&inputs[0][..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)
            .map_err(|e| ComputeError::ExecutionFailed(format!("decompress: {}", e)))?;
        Ok(Bytes::from(decompressed))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 50_000 + 1, memory_bytes: size * 4 }
    }
}

// ── #23 ZstdCompressFn ──

pub struct ZstdCompressFn;

impl ComputeFunction for ZstdCompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("zstd_compress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let level: i32 = match params.get("level") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("level must be 1-22".into()))?,
            None => 3,
            _ => return Err(ComputeError::InvalidParam("level must be a string".into())),
        };
        if !(1..=22).contains(&level) {
            return Err(ComputeError::InvalidParam("level must be 1-22".into()));
        }
        zstd::encode_all(&inputs[0][..], level)
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("zstd compress: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 40_000 + 1, memory_bytes: size * 2 }
    }
}

// ── #24 ZstdDecompressFn ──

pub struct ZstdDecompressFn;

impl ComputeFunction for ZstdDecompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("zstd_decompress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        zstd::decode_all(&inputs[0][..])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("zstd decompress: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 40_000 + 1, memory_bytes: size * 4 }
    }
}

// ── #25 Lz4CompressFn ──

pub struct Lz4CompressFn;

impl ComputeFunction for Lz4CompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("lz4_compress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let compressed = lz4_flex::compress_prepend_size(&inputs[0]);
        Ok(Bytes::from(compressed))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: size * 2 }
    }
}

// ── #26 Lz4DecompressFn ──

pub struct Lz4DecompressFn;

impl ComputeFunction for Lz4DecompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("lz4_decompress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        lz4_flex::decompress_size_prepended(&inputs[0])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("lz4 decompress: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: size * 4 }
    }
}

// ── #27 SnappyCompressFn ──

pub struct SnappyCompressFn;

impl ComputeFunction for SnappyCompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("snappy_compress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mut encoder = snap::raw::Encoder::new();
        encoder.compress_vec(&inputs[0])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("snappy compress: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: size * 2 }
    }
}

// ── #28 SnappyDecompressFn ──

pub struct SnappyDecompressFn;

impl ComputeFunction for SnappyDecompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("snappy_decompress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mut decoder = snap::raw::Decoder::new();
        decoder.decompress_vec(&inputs[0])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("snappy decompress: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: size * 4 }
    }
}

// ── #29 BrotliCompressFn ──

pub struct BrotliCompressFn;

impl ComputeFunction for BrotliCompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("brotli_compress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let quality: u32 = match params.get("quality") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("quality must be 0-11".into()))?,
            None => 6,
            _ => return Err(ComputeError::InvalidParam("quality must be a string".into())),
        };
        if quality > 11 {
            return Err(ComputeError::InvalidParam("quality must be 0-11".into()));
        }
        let mut output = Vec::new();
        let bp = brotli::enc::BrotliEncoderParams {
            quality: quality as i32,
            ..Default::default()
        };
        brotli::BrotliCompress(&mut &inputs[0][..], &mut output, &bp)
            .map_err(|e| ComputeError::ExecutionFailed(format!("brotli compress: {}", e)))?;
        Ok(Bytes::from(output))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 20_000 + 1, memory_bytes: size * 3 }
    }
}

// ── #30 BrotliDecompressFn ──

pub struct BrotliDecompressFn;

impl ComputeFunction for BrotliDecompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("brotli_decompress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mut output = Vec::new();
        brotli::BrotliDecompress(&mut &inputs[0][..], &mut output)
            .map_err(|e| ComputeError::ExecutionFailed(format!("brotli decompress: {}", e)))?;
        Ok(Bytes::from(output))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 50_000 + 1, memory_bytes: size * 4 }
    }
}

// ── #31 Sha256Fn ──

pub struct Sha256Fn;

impl ComputeFunction for Sha256Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("sha256", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use sha2::{Sha256, Digest};
        let hash = Sha256::digest(&inputs[0]);
        Ok(Bytes::copy_from_slice(&hash))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: 256 }
    }
}

// ── #32 Sha512Fn ──

pub struct Sha512Fn;

impl ComputeFunction for Sha512Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("sha512", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use sha2::{Sha512, Digest};
        let hash = Sha512::digest(&inputs[0]);
        Ok(Bytes::copy_from_slice(&hash))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: 256 }
    }
}

// ── Shared helpers for crypto functions ──

fn get_string_param<'a>(params: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a str, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => Ok(s.as_str()),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

fn hex_decode_param(hex: &str, name: &str) -> Result<Vec<u8>, ComputeError> {
    if hex.len() % 2 != 0 {
        return Err(ComputeError::InvalidParam(format!("odd-length hex in {}", name)));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| ComputeError::InvalidParam(format!("invalid hex in {}", name)))
        })
        .collect()
}

// ── #33 Md5Fn ──

pub struct Md5Fn;

impl ComputeFunction for Md5Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("md5", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use md5::{Md5, Digest};
        let hash = Md5::digest(&inputs[0]);
        Ok(Bytes::copy_from_slice(&hash))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: 256 }
    }
}

// ── #34 Blake3Fn ──

pub struct Blake3Fn;

impl ComputeFunction for Blake3Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("blake3", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let hash = blake3::hash(&inputs[0]);
        Ok(Bytes::copy_from_slice(hash.as_bytes()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 200_000 + 1, memory_bytes: 256 }
    }
}

// ── #35 HmacSha256Fn ──

pub struct HmacSha256Fn;

impl ComputeFunction for HmacSha256Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("hmac_sha256", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let key_hex = get_string_param(params, "key")?;
        let key = hex_decode_param(key_hex, "key")?;
        let mut mac = HmacSha256::new_from_slice(&key)
            .map_err(|e| ComputeError::ExecutionFailed(format!("hmac init: {}", e)))?;
        mac.update(&inputs[0]);
        let result = mac.finalize();
        Ok(Bytes::copy_from_slice(&result.into_bytes()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: 256 }
    }
}

// ── #36 Crc32Fn ──

pub struct Crc32Fn;

impl ComputeFunction for Crc32Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("crc32", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let crc = crc32fast::hash(&inputs[0]);
        Ok(Bytes::copy_from_slice(&crc.to_be_bytes()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 500_000 + 1, memory_bytes: 64 }
    }
}

// ── #37 EncryptFn (AES-256-CTR) ──

pub struct EncryptFn;

impl ComputeFunction for EncryptFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("encrypt", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use aes::Aes256;
        use ctr::cipher::{KeyIvInit, StreamCipher};
        type Aes256Ctr = ctr::Ctr64BE<Aes256>;

        let key = hex_decode_param(get_string_param(params, "key")?, "key")?;
        let nonce = hex_decode_param(get_string_param(params, "nonce")?, "nonce")?;
        if key.len() != 32 {
            return Err(ComputeError::InvalidParam("key must be 32 bytes (64 hex chars)".into()));
        }
        if nonce.len() != 16 {
            return Err(ComputeError::InvalidParam("nonce must be 16 bytes (32 hex chars)".into()));
        }
        let mut cipher = Aes256Ctr::new(key[..].into(), nonce[..].into());
        let mut buf = inputs[0].to_vec();
        cipher.apply_keystream(&mut buf);
        Ok(Bytes::from(buf))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 50_000 + 1, memory_bytes: size + 256 }
    }
}

// ── #38 DecryptFn (AES-256-CTR) ──

pub struct DecryptFn;

impl ComputeFunction for DecryptFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("decrypt", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        EncryptFn.execute(inputs, params)
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        EncryptFn.estimated_cost(input_sizes)
    }
}

// ── #39 AeadEncryptFn (AES-256-GCM) ──

pub struct AeadEncryptFn;

impl ComputeFunction for AeadEncryptFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("aead_encrypt", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead, Nonce};

        let key = hex_decode_param(get_string_param(params, "key")?, "key")?;
        let nonce_bytes = hex_decode_param(get_string_param(params, "nonce")?, "nonce")?;
        if key.len() != 32 {
            return Err(ComputeError::InvalidParam("key must be 32 bytes (64 hex chars)".into()));
        }
        if nonce_bytes.len() != 12 {
            return Err(ComputeError::InvalidParam("nonce must be 12 bytes (24 hex chars)".into()));
        }
        let cipher = Aes256Gcm::new(key[..].into());
        let nonce = Nonce::from_slice(&nonce_bytes);
        cipher.encrypt(nonce, inputs[0].as_ref())
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("aead encrypt: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 50_000 + 1, memory_bytes: size + 256 }
    }
}

// ── #40 AeadDecryptFn (AES-256-GCM) ──

pub struct AeadDecryptFn;

impl ComputeFunction for AeadDecryptFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("aead_decrypt", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead, Nonce};

        let key = hex_decode_param(get_string_param(params, "key")?, "key")?;
        let nonce_bytes = hex_decode_param(get_string_param(params, "nonce")?, "nonce")?;
        if key.len() != 32 {
            return Err(ComputeError::InvalidParam("key must be 32 bytes (64 hex chars)".into()));
        }
        if nonce_bytes.len() != 12 {
            return Err(ComputeError::InvalidParam("nonce must be 12 bytes (24 hex chars)".into()));
        }
        if inputs[0].len() < 16 {
            return Err(ComputeError::ExecutionFailed("ciphertext too short (missing tag)".into()));
        }
        let cipher = Aes256Gcm::new(key[..].into());
        let nonce = Nonce::from_slice(&nonce_bytes);
        cipher.decrypt(nonce, inputs[0].as_ref())
            .map(Bytes::from)
            .map_err(|_| ComputeError::ExecutionFailed("aead decrypt: authentication failed".into()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 50_000 + 1, memory_bytes: size + 256 }
    }
}

// ── #41 RedactFn ──

pub struct RedactFn;

impl ComputeFunction for RedactFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("redact", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let patterns_str = get_string_param(params, "patterns")?;
        let input_str = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("redact requires UTF-8 input".into()))?;
        let mut result = input_str.to_string();
        for pattern in patterns_str.split(',') {
            let re = regex::Regex::new(pattern.trim())
                .map_err(|e| ComputeError::InvalidParam(format!("invalid regex '{}': {}", pattern, e)))?;
            result = re.replace_all(&result, "[REDACTED]").into_owned();
        }
        Ok(Bytes::from(result))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 10_000 + 1, memory_bytes: size * 2 }
    }
}

// ── #42 ByteCountFn ──

pub struct ByteCountFn;

impl ComputeFunction for ByteCountFn {
    fn id(&self) -> FunctionId { FunctionId::new("byte_count", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        Ok(Bytes::copy_from_slice(&(inputs[0].len() as u64).to_be_bytes()))
    }
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost { ComputeCost { cpu_ms: 1, memory_bytes: 8 } }
}

// ── #43 LineCountFn ──

pub struct LineCountFn;

impl ComputeFunction for LineCountFn {
    fn id(&self) -> FunctionId { FunctionId::new("line_count", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let input = &inputs[0];
        if input.is_empty() {
            return Ok(Bytes::copy_from_slice(&0u64.to_be_bytes()));
        }
        let newlines = input.iter().filter(|&&b| b == b'\n').count() as u64;
        let count = if input.last() == Some(&b'\n') { newlines } else { newlines + 1 };
        Ok(Bytes::copy_from_slice(&count.to_be_bytes()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 500_000 + 1, memory_bytes: 8 }
    }
}

// ── #44 WordCountFn ──

pub struct WordCountFn;

impl ComputeFunction for WordCountFn {
    fn id(&self) -> FunctionId { FunctionId::new("word_count", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let mut count = 0u64;
        let mut in_word = false;
        for &b in inputs[0].iter() {
            if b.is_ascii_whitespace() {
                in_word = false;
            } else if !in_word {
                in_word = true;
                count += 1;
            }
        }
        Ok(Bytes::copy_from_slice(&count.to_be_bytes()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 500_000 + 1, memory_bytes: 8 }
    }
}

// ── #45 HistogramFn ──

pub struct HistogramFn;

impl ComputeFunction for HistogramFn {
    fn id(&self) -> FunctionId { FunctionId::new("histogram", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let mut counts = [0u64; 256];
        for &b in inputs[0].iter() {
            counts[b as usize] += 1;
        }
        let mut out = Vec::with_capacity(2048);
        for c in &counts {
            out.extend_from_slice(&c.to_be_bytes());
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 500_000 + 1, memory_bytes: 2048 }
    }
}

// ── #46 EntropyFn ──

pub struct EntropyFn;

impl ComputeFunction for EntropyFn {
    fn id(&self) -> FunctionId { FunctionId::new("entropy", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let input = &inputs[0];
        if input.is_empty() {
            return Ok(Bytes::copy_from_slice(&0.0f64.to_be_bytes()));
        }
        let mut counts = [0u64; 256];
        for &b in input.iter() {
            counts[b as usize] += 1;
        }
        let len = input.len() as f64;
        let entropy: f64 = counts.iter()
            .filter(|&&c| c > 0)
            .map(|&c| { let p = c as f64 / len; -p * p.log2() })
            .sum();
        Ok(Bytes::copy_from_slice(&entropy.to_be_bytes()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 200_000 + 1, memory_bytes: 2048 }
    }
}

// ── #47 MinMaxFn ──

pub struct MinMaxFn;

impl ComputeFunction for MinMaxFn {
    fn id(&self) -> FunctionId { FunctionId::new("min_max", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let input = &inputs[0];
        if input.is_empty() {
            return Err(ComputeError::ExecutionFailed("min_max requires non-empty input".into()));
        }
        let min = *input.iter().min().unwrap();
        let max = *input.iter().max().unwrap();
        Ok(Bytes::from(vec![min, max]))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 500_000 + 1, memory_bytes: 2 }
    }
}

// ── #48 SumFn ──

pub struct SumFn;

impl ComputeFunction for SumFn {
    fn id(&self) -> FunctionId { FunctionId::new("sum", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("sum requires UTF-8 input".into()))?;
        let mut total: f64 = 0.0;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            let n: f64 = trimmed.parse()
                .map_err(|_| ComputeError::ExecutionFailed(format!("not a number: '{}'", trimmed)))?;
            total += n;
        }
        Ok(Bytes::from(total.to_string()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: size + 64 }
    }
}

// ── #49 AverageFn ──

pub struct AverageFn;

impl ComputeFunction for AverageFn {
    fn id(&self) -> FunctionId { FunctionId::new("average", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("average requires UTF-8 input".into()))?;
        let mut total: f64 = 0.0;
        let mut count: u64 = 0;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            let n: f64 = trimmed.parse()
                .map_err(|_| ComputeError::ExecutionFailed(format!("not a number: '{}'", trimmed)))?;
            total += n;
            count += 1;
        }
        if count == 0 {
            return Err(ComputeError::ExecutionFailed("average requires at least one number".into()));
        }
        Ok(Bytes::from((total / count as f64).to_string()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: size + 64 }
    }
}

// ── #50 InterleaveFn ──

pub struct InterleaveFn;

impl ComputeFunction for InterleaveFn {
    fn id(&self) -> FunctionId { FunctionId::new("interleave", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let bs: usize = match params.get("block_size") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("block_size must be positive integer".into()))?,
            None => 1,
            _ => return Err(ComputeError::InvalidParam("block_size must be a string".into())),
        };
        if bs == 0 { return Err(ComputeError::InvalidParam("block_size must be > 0".into())); }
        let mut offsets = vec![0usize; inputs.len()];
        let total: usize = inputs.iter().map(|i| i.len()).sum();
        let mut out = Vec::with_capacity(total);
        loop {
            let mut progress = false;
            for (i, input) in inputs.iter().enumerate() {
                let start = offsets[i];
                if start < input.len() {
                    let end = (start + bs).min(input.len());
                    out.extend_from_slice(&input[start..end]);
                    offsets[i] = end;
                    progress = true;
                }
            }
            if !progress { break; }
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size: u64 = input_sizes.iter().sum();
        ComputeCost { cpu_ms: size / 200_000 + 1, memory_bytes: size }
    }
}

// ── #51 ZipConcatFn ──

pub struct ZipConcatFn;

impl ComputeFunction for ZipConcatFn {
    fn id(&self) -> FunctionId { FunctionId::new("zip_concat", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let a_str = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("zip_concat requires UTF-8 input".into()))?;
        let b_str = std::str::from_utf8(&inputs[1])
            .map_err(|_| ComputeError::ExecutionFailed("zip_concat requires UTF-8 input".into()))?;
        let a_lines: Vec<&str> = a_str.lines().collect();
        let b_lines: Vec<&str> = b_str.lines().collect();
        let max_len = a_lines.len().max(b_lines.len());
        let mut out = String::new();
        for i in 0..max_len {
            if i > 0 { out.push('\n'); }
            if let Some(a) = a_lines.get(i) { out.push_str(a); }
            if let Some(b) = b_lines.get(i) { out.push_str(b); }
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size: u64 = input_sizes.iter().sum();
        ComputeCost { cpu_ms: size / 200_000 + 1, memory_bytes: size }
    }
}

// ── #52 DiffFn ──

pub struct DiffFn;

impl ComputeFunction for DiffFn {
    fn id(&self) -> FunctionId { FunctionId::new("diff", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let old = &inputs[0];
        let new = &inputs[1];
        let mut out = Vec::new();
        let mut oi = 0;
        let mut ni = 0;
        while oi < old.len() && ni < new.len() {
            if old[oi] == new[ni] {
                let start = oi;
                while oi < old.len() && ni < new.len() && old[oi] == new[ni] { oi += 1; ni += 1; }
                out.push(0x00);
                out.extend_from_slice(&((oi - start) as u32).to_be_bytes());
            } else {
                let ostart = oi;
                let nstart = ni;
                while oi < old.len() && ni < new.len() && old[oi] != new[ni] { oi += 1; ni += 1; }
                out.push(0x02);
                out.extend_from_slice(&((oi - ostart) as u32).to_be_bytes());
                out.push(0x01);
                out.extend_from_slice(&((ni - nstart) as u32).to_be_bytes());
                out.extend_from_slice(&new[nstart..ni]);
            }
        }
        if oi < old.len() {
            out.push(0x02);
            out.extend_from_slice(&((old.len() - oi) as u32).to_be_bytes());
        }
        if ni < new.len() {
            out.push(0x01);
            out.extend_from_slice(&((new.len() - ni) as u32).to_be_bytes());
            out.extend_from_slice(&new[ni..]);
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size: u64 = input_sizes.iter().sum();
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: size * 2 }
    }
}

// ── #53 PatchFn ──

pub struct PatchFn;

impl ComputeFunction for PatchFn {
    fn id(&self) -> FunctionId { FunctionId::new("patch", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let base = &inputs[0];
        let patch = &inputs[1];
        let mut out = Vec::new();
        let mut bi = 0;
        let mut pi = 0;
        while pi < patch.len() {
            let op = patch[pi]; pi += 1;
            if pi + 4 > patch.len() { return Err(ComputeError::ExecutionFailed("truncated patch".into())); }
            let len = u32::from_be_bytes([patch[pi], patch[pi+1], patch[pi+2], patch[pi+3]]) as usize;
            pi += 4;
            match op {
                0x00 => {
                    if bi + len > base.len() { return Err(ComputeError::ExecutionFailed("patch COPY exceeds base".into())); }
                    out.extend_from_slice(&base[bi..bi+len]);
                    bi += len;
                }
                0x01 => {
                    if pi + len > patch.len() { return Err(ComputeError::ExecutionFailed("patch INSERT exceeds data".into())); }
                    out.extend_from_slice(&patch[pi..pi+len]);
                    pi += len;
                }
                0x02 => { bi += len; }
                _ => return Err(ComputeError::ExecutionFailed(format!("unknown patch op: 0x{:02x}", op))),
            }
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size: u64 = input_sizes.iter().sum();
        ComputeCost { cpu_ms: size / 100_000 + 1, memory_bytes: size * 2 }
    }
}

// ── #54 MergeSortedFn ──

pub struct MergeSortedFn;

impl ComputeFunction for MergeSortedFn {
    fn id(&self) -> FunctionId { FunctionId::new("merge_sorted", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() { return Err(ComputeError::InputCount { expected: 2, got: 0 }); }
        let strings: Vec<&str> = inputs.iter()
            .map(|i| std::str::from_utf8(i).map_err(|_| ComputeError::ExecutionFailed("merge_sorted requires UTF-8".into())))
            .collect::<Result<_, _>>()?;
        let mut iters: Vec<std::iter::Peekable<std::str::Lines<'_>>> =
            strings.iter().map(|s| s.lines().peekable()).collect();
        let mut lines = Vec::new();
        loop {
            let mut best: Option<(usize, &str)> = None;
            for (i, iter) in iters.iter_mut().enumerate() {
                if let Some(&line) = iter.peek() {
                    if best.is_none() || line < best.unwrap().1 { best = Some((i, line)); }
                }
            }
            match best {
                Some((i, _)) => { lines.push(iters[i].next().unwrap()); }
                None => break,
            }
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size: u64 = input_sizes.iter().sum();
        ComputeCost { cpu_ms: size / 50_000 + 1, memory_bytes: size * 2 }
    }
}

// ── #55 SelectFn ──

pub struct SelectFn;

impl ComputeFunction for SelectFn {
    fn id(&self) -> FunctionId { FunctionId::new("select", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let idx: usize = parse_usize_param(params, "index")?;
        if idx >= inputs.len() {
            return Err(ComputeError::ExecutionFailed(format!("index {} out of range (have {} inputs)", idx, inputs.len())));
        }
        Ok(inputs[idx].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let size = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: size }
    }
}

// ── #56 TakeFn ──

pub struct TakeFn;

impl ComputeFunction for TakeFn {
    fn id(&self) -> FunctionId { FunctionId::new("take", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let n: usize = parse_usize_param(params, "bytes")?;
        let end = n.min(inputs[0].len());
        Ok(inputs[0].slice(..end))
    }
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost { ComputeCost { cpu_ms: 1, memory_bytes: 0 } }
}

// ── #57 SkipFn ──

pub struct SkipFn;

impl ComputeFunction for SkipFn {
    fn id(&self) -> FunctionId { FunctionId::new("skip", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let n: usize = parse_usize_param(params, "bytes")?;
        let start = n.min(inputs[0].len());
        Ok(inputs[0].slice(start..))
    }
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost { ComputeCost { cpu_ms: 1, memory_bytes: 0 } }
}

// ── #58 SliceFn ──

pub struct SliceFn;

impl ComputeFunction for SliceFn {
    fn id(&self) -> FunctionId { FunctionId::new("slice", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let offset: usize = parse_usize_param(params, "offset")?;
        let length: usize = parse_usize_param(params, "length")?;
        let input = &inputs[0];
        let start = offset.min(input.len());
        let end = start.saturating_add(length).min(input.len());
        Ok(inputs[0].slice(start..end))
    }
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost { ComputeCost { cpu_ms: 1, memory_bytes: 0 } }
}

// ── #59 SortFn ──

pub struct SortFn;

impl ComputeFunction for SortFn {
    fn id(&self) -> FunctionId { FunctionId::new("sort", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("sort requires UTF-8 input".into()))?;
        let mut lines: Vec<&str> = text.lines().collect();
        lines.sort();
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #60 UniqueFn ──

pub struct UniqueFn;

impl ComputeFunction for UniqueFn {
    fn id(&self) -> FunctionId { FunctionId::new("unique", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("unique requires UTF-8 input".into()))?;
        let mut result = Vec::new();
        let mut prev: Option<&str> = None;
        for line in text.lines() {
            if prev != Some(line) { result.push(line); prev = Some(line); }
        }
        Ok(Bytes::from(result.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 100_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #61 SortUniqueFn ──

pub struct SortUniqueFn;

impl ComputeFunction for SortUniqueFn {
    fn id(&self) -> FunctionId { FunctionId::new("sort_unique", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("sort_unique requires UTF-8 input".into()))?;
        let mut lines: Vec<&str> = text.lines().collect();
        lines.sort();
        lines.dedup();
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #62 ShuffleFn ──

pub struct ShuffleFn;

impl ComputeFunction for ShuffleFn {
    fn id(&self) -> FunctionId { FunctionId::new("shuffle", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        use rand::seq::SliceRandom;
        use rand::SeedableRng;
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let seed: u64 = parse_u64_param(params, "seed")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("shuffle requires UTF-8 input".into()))?;
        let mut lines: Vec<&str> = text.lines().collect();
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        lines.shuffle(&mut rng);
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 100_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #63 HeadFn ──

pub struct HeadFn;

impl ComputeFunction for HeadFn {
    fn id(&self) -> FunctionId { FunctionId::new("head", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let n: usize = parse_usize_param(params, "lines")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("head requires UTF-8 input".into()))?;
        let result: Vec<&str> = text.lines().take(n).collect();
        Ok(Bytes::from(result.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: s }
    }
}

// ── #64 TailFn ──

pub struct TailFn;

impl ComputeFunction for TailFn {
    fn id(&self) -> FunctionId { FunctionId::new("tail", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let n: usize = parse_usize_param(params, "lines")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("tail requires UTF-8 input".into()))?;
        let all_lines: Vec<&str> = text.lines().collect();
        let start = all_lines.len().saturating_sub(n);
        Ok(Bytes::from(all_lines[start..].join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: s }
    }
}

// ── #65 SampleFn ──

pub struct SampleFn;

impl ComputeFunction for SampleFn {
    fn id(&self) -> FunctionId { FunctionId::new("sample", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        use rand::Rng;
        use rand::SeedableRng;
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let n: usize = parse_usize_param(params, "lines")?;
        let seed: u64 = parse_u64_param(params, "seed")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("sample requires UTF-8 input".into()))?;
        let all_lines: Vec<&str> = text.lines().collect();
        if n >= all_lines.len() { return Ok(Bytes::from(all_lines.join("\n"))); }
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut reservoir: Vec<usize> = (0..n).collect();
        for i in n..all_lines.len() {
            let j = rng.gen_range(0..=i);
            if j < n { reservoir[j] = i; }
        }
        reservoir.sort();
        let sampled: Vec<&str> = reservoir.iter().map(|&i| all_lines[i]).collect();
        Ok(Bytes::from(sampled.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #66 ReplaceFn ──

pub struct ReplaceFn;

impl ComputeFunction for ReplaceFn {
    fn id(&self) -> FunctionId { FunctionId::new("replace", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let find = get_string_param(params, "find")?;
        let replace = get_string_param(params, "replace")?;
        if find.is_empty() { return Ok(inputs[0].clone()); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("replace requires UTF-8 input".into()))?;
        Ok(Bytes::from(text.replace(find, replace)))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 100_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #67 RegexReplaceFn ──

pub struct RegexReplaceFn;

impl ComputeFunction for RegexReplaceFn {
    fn id(&self) -> FunctionId { FunctionId::new("regex_replace", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let pattern = get_string_param(params, "pattern")?;
        let replacement = get_string_param(params, "replacement")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("regex_replace requires UTF-8 input".into()))?;
        let re = regex::Regex::new(pattern).map_err(|e| ComputeError::InvalidParam(format!("invalid regex: {}", e)))?;
        Ok(Bytes::from(re.replace_all(text, replacement).into_owned()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #68 GrepFn ──

pub struct GrepFn;

impl ComputeFunction for GrepFn {
    fn id(&self) -> FunctionId { FunctionId::new("grep", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let pattern = get_string_param(params, "pattern")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("grep requires UTF-8 input".into()))?;
        let re = regex::Regex::new(pattern).map_err(|e| ComputeError::InvalidParam(format!("invalid regex: {}", e)))?;
        let matched: Vec<&str> = text.lines().filter(|l| re.is_match(l)).collect();
        Ok(Bytes::from(matched.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s }
    }
}

// ── #69 GrepInvertFn ──

pub struct GrepInvertFn;

impl ComputeFunction for GrepInvertFn {
    fn id(&self) -> FunctionId { FunctionId::new("grep_invert", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let pattern = get_string_param(params, "pattern")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("grep_invert requires UTF-8 input".into()))?;
        let re = regex::Regex::new(pattern).map_err(|e| ComputeError::InvalidParam(format!("invalid regex: {}", e)))?;
        let filtered: Vec<&str> = text.lines().filter(|l| !re.is_match(l)).collect();
        Ok(Bytes::from(filtered.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s }
    }
}

// ── #70 PrefixFn ──

pub struct PrefixFn;

impl ComputeFunction for PrefixFn {
    fn id(&self) -> FunctionId { FunctionId::new("prefix", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let prefix = get_string_param(params, "prefix")?;
        let mut out = Vec::with_capacity(prefix.len() + inputs[0].len());
        out.extend_from_slice(prefix.as_bytes());
        out.extend_from_slice(&inputs[0]);
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: s + 256 }
    }
}

// ── #71 SuffixFn ──

pub struct SuffixFn;

impl ComputeFunction for SuffixFn {
    fn id(&self) -> FunctionId { FunctionId::new("suffix", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let suffix = get_string_param(params, "suffix")?;
        let mut out = Vec::with_capacity(inputs[0].len() + suffix.len());
        out.extend_from_slice(&inputs[0]);
        out.extend_from_slice(suffix.as_bytes());
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: 1, memory_bytes: s + 256 }
    }
}

// ── #72 LinePrefixFn ──

pub struct LinePrefixFn;

impl ComputeFunction for LinePrefixFn {
    fn id(&self) -> FunctionId { FunctionId::new("line_prefix", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let prefix = get_string_param(params, "prefix")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("line_prefix requires UTF-8 input".into()))?;
        let result: Vec<String> = text.lines().map(|l| format!("{}{}", prefix, l)).collect();
        Ok(Bytes::from(result.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 100_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #73 LineNumberFn ──

pub struct LineNumberFn;

impl ComputeFunction for LineNumberFn {
    fn id(&self) -> FunctionId { FunctionId::new("line_number", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("line_number requires UTF-8 input".into()))?;
        let result: Vec<String> = text.lines().enumerate().map(|(i, l)| format!("{:>6}\t{}", i + 1, l)).collect();
        Ok(Bytes::from(result.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 100_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #74 TruncateLinesFn ──

pub struct TruncateLinesFn;

impl ComputeFunction for TruncateLinesFn {
    fn id(&self) -> FunctionId { FunctionId::new("truncate_lines", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let max: usize = parse_usize_param(params, "max_line_bytes")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("truncate_lines requires UTF-8 input".into()))?;
        let result: Vec<&str> = text.lines().map(|l| if l.len() > max { &l[..max] } else { l }).collect();
        Ok(Bytes::from(result.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 100_000 + 1, memory_bytes: s }
    }
}

// ── #75 CharsetConvertFn ──

pub struct CharsetConvertFn;

impl ComputeFunction for CharsetConvertFn {
    fn id(&self) -> FunctionId { FunctionId::new("charset_convert", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let from_name = get_string_param(params, "from")?;
        let to_name = get_string_param(params, "to")?;
        let from_enc = encoding_rs::Encoding::for_label(from_name.as_bytes())
            .ok_or_else(|| ComputeError::InvalidParam(format!("unknown encoding: {}", from_name)))?;
        let to_enc = encoding_rs::Encoding::for_label(to_name.as_bytes())
            .ok_or_else(|| ComputeError::InvalidParam(format!("unknown encoding: {}", to_name)))?;
        let (decoded, _, had_errors) = from_enc.decode(&inputs[0]);
        if had_errors { return Err(ComputeError::ExecutionFailed(format!("invalid {} input", from_name))); }
        let (encoded, _, _) = to_enc.encode(&decoded);
        Ok(Bytes::from(encoded.into_owned()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s * 3 }
    }
}

// ── #76 Utf8ValidateFn ──

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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 200_000 + 1, memory_bytes: 0 }
    }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s * 3 }
    }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 10_000 + 1, memory_bytes: s * 5 }
    }
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
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost { ComputeCost { cpu_ms: 1, memory_bytes: 0 } }
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
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost { ComputeCost { cpu_ms: 1, memory_bytes: 0 } }
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
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost { ComputeCost { cpu_ms: 1, memory_bytes: 0 } }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 100_000 + 1, memory_bytes: 0 }
    }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 200_000 + 1, memory_bytes: 0 }
    }
}

// ── #84 JsonPrettyPrintFn ──

pub struct JsonPrettyPrintFn;

impl ComputeFunction for JsonPrettyPrintFn {
    fn id(&self) -> FunctionId { FunctionId::new("json_pretty", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("json_pretty requires UTF-8".into()))?;
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
        let pretty = serde_json::to_string_pretty(&value).map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
        Ok(Bytes::from(pretty))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s * 3 }
    }
}

// ── #85 JsonMinifyFn ──

pub struct JsonMinifyFn;

impl ComputeFunction for JsonMinifyFn {
    fn id(&self) -> FunctionId { FunctionId::new("json_minify", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("json_minify requires UTF-8".into()))?;
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
        let compact = serde_json::to_string(&value).map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
        Ok(Bytes::from(compact))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #86 CsvToJsonFn ──

pub struct CsvToJsonFn;

impl ComputeFunction for CsvToJsonFn {
    fn id(&self) -> FunctionId { FunctionId::new("csv_to_json", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let mut reader = csv::Reader::from_reader(&inputs[0][..]);
        let headers: Vec<String> = reader.headers().map_err(|e| ComputeError::ExecutionFailed(format!("csv headers: {}", e)))?.iter().map(|h| h.to_string()).collect();
        let mut records = Vec::new();
        for result in reader.records() {
            let record = result.map_err(|e| ComputeError::ExecutionFailed(format!("csv record: {}", e)))?;
            let mut obj = serde_json::Map::new();
            for (i, field) in record.iter().enumerate() {
                let key = headers.get(i).cloned().unwrap_or_else(|| format!("col_{}", i));
                obj.insert(key, serde_json::Value::String(field.to_string()));
            }
            records.push(serde_json::Value::Object(obj));
        }
        let json = serde_json::to_string_pretty(&records).map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
        Ok(Bytes::from(json))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 20_000 + 1, memory_bytes: s * 5 }
    }
}

// ── #87 JsonToCsvFn ──

pub struct JsonToCsvFn;

impl ComputeFunction for JsonToCsvFn {
    fn id(&self) -> FunctionId { FunctionId::new("json_to_csv", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("json_to_csv requires UTF-8".into()))?;
        let array: Vec<serde_json::Map<String, serde_json::Value>> = serde_json::from_str(text).map_err(|e| ComputeError::ExecutionFailed(format!("expected JSON array of objects: {}", e)))?;
        if array.is_empty() { return Ok(Bytes::new()); }
        let mut headers: Vec<String> = array[0].keys().cloned().collect();
        headers.sort();
        let mut wtr = csv::Writer::from_writer(Vec::new());
        wtr.write_record(&headers).map_err(|e| ComputeError::ExecutionFailed(format!("csv write: {}", e)))?;
        for obj in &array {
            let row: Vec<String> = headers.iter().map(|h| match obj.get(h) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            }).collect();
            wtr.write_record(&row).map_err(|e| ComputeError::ExecutionFailed(format!("csv write: {}", e)))?;
        }
        let csv_bytes = wtr.into_inner().map_err(|e| ComputeError::ExecutionFailed(format!("csv flush: {}", e)))?;
        Ok(Bytes::from(csv_bytes))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 20_000 + 1, memory_bytes: s * 3 }
    }
}

// ── #88 JsonLinesFn ──

pub struct JsonLinesFn;

impl ComputeFunction for JsonLinesFn {
    fn id(&self) -> FunctionId { FunctionId::new("json_lines", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("json_lines requires UTF-8".into()))?;
        let array: Vec<serde_json::Value> = serde_json::from_str(text).map_err(|e| ComputeError::ExecutionFailed(format!("expected JSON array: {}", e)))?;
        let lines: Vec<String> = array.iter().map(|v| serde_json::to_string(v)).collect::<Result<_, _>>().map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 50_000 + 1, memory_bytes: s * 2 }
    }
}

// ── #89 YamlToJsonFn ──

pub struct YamlToJsonFn;

impl ComputeFunction for YamlToJsonFn {
    fn id(&self) -> FunctionId { FunctionId::new("yaml_to_json", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("yaml_to_json requires UTF-8".into()))?;
        let value: serde_yaml::Value = serde_yaml::from_str(text).map_err(|e| ComputeError::ExecutionFailed(format!("invalid YAML: {}", e)))?;
        let json = serde_json::to_string_pretty(&value).map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
        Ok(Bytes::from(json))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 30_000 + 1, memory_bytes: s * 3 }
    }
}

// ── #90 JsonToYamlFn ──

pub struct JsonToYamlFn;

impl ComputeFunction for JsonToYamlFn {
    fn id(&self) -> FunctionId { FunctionId::new("json_to_yaml", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("json_to_yaml requires UTF-8".into()))?;
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
        let yaml = serde_yaml::to_string(&value).map_err(|e| ComputeError::ExecutionFailed(format!("yaml serialize: {}", e)))?;
        Ok(Bytes::from(yaml))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 30_000 + 1, memory_bytes: s * 3 }
    }
}

// ── #91 TomlToJsonFn ──

pub struct TomlToJsonFn;

impl ComputeFunction for TomlToJsonFn {
    fn id(&self) -> FunctionId { FunctionId::new("toml_to_json", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("toml_to_json requires UTF-8".into()))?;
        let value: toml::Value = toml::from_str(text).map_err(|e| ComputeError::ExecutionFailed(format!("invalid TOML: {}", e)))?;
        let json = serde_json::to_string_pretty(&value).map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
        Ok(Bytes::from(json))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        let s = input_sizes.first().copied().unwrap_or(0);
        ComputeCost { cpu_ms: s / 30_000 + 1, memory_bytes: s * 3 }
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
    registry.register(Arc::new(XorFn));
    registry.register(Arc::new(BitwiseAndFn));
    registry.register(Arc::new(BitwiseOrFn));
    registry.register(Arc::new(BitwiseNotFn));
    registry.register(Arc::new(ByteSwapFn));
    registry.register(Arc::new(TrimFn));
    registry.register(Arc::new(PadFn));
    registry.register(Arc::new(LineEndingFn));
    registry.register(Arc::new(CompressFn));
    registry.register(Arc::new(DecompressFn));
    registry.register(Arc::new(ZstdCompressFn));
    registry.register(Arc::new(ZstdDecompressFn));
    registry.register(Arc::new(Lz4CompressFn));
    registry.register(Arc::new(Lz4DecompressFn));
    registry.register(Arc::new(SnappyCompressFn));
    registry.register(Arc::new(SnappyDecompressFn));
    registry.register(Arc::new(BrotliCompressFn));
    registry.register(Arc::new(BrotliDecompressFn));
    registry.register(Arc::new(Sha256Fn));
    registry.register(Arc::new(Sha512Fn));
    registry.register(Arc::new(Md5Fn));
    registry.register(Arc::new(Blake3Fn));
    registry.register(Arc::new(HmacSha256Fn));
    registry.register(Arc::new(Crc32Fn));
    registry.register(Arc::new(EncryptFn));
    registry.register(Arc::new(DecryptFn));
    registry.register(Arc::new(AeadEncryptFn));
    registry.register(Arc::new(AeadDecryptFn));
    registry.register(Arc::new(RedactFn));
    registry.register(Arc::new(ByteCountFn));
    registry.register(Arc::new(LineCountFn));
    registry.register(Arc::new(WordCountFn));
    registry.register(Arc::new(HistogramFn));
    registry.register(Arc::new(EntropyFn));
    registry.register(Arc::new(MinMaxFn));
    registry.register(Arc::new(SumFn));
    registry.register(Arc::new(AverageFn));
    registry.register(Arc::new(InterleaveFn));
    registry.register(Arc::new(ZipConcatFn));
    registry.register(Arc::new(DiffFn));
    registry.register(Arc::new(PatchFn));
    registry.register(Arc::new(MergeSortedFn));
    registry.register(Arc::new(SelectFn));
    registry.register(Arc::new(TakeFn));
    registry.register(Arc::new(SkipFn));
    registry.register(Arc::new(SliceFn));
    registry.register(Arc::new(SortFn));
    registry.register(Arc::new(UniqueFn));
    registry.register(Arc::new(SortUniqueFn));
    registry.register(Arc::new(ShuffleFn));
    registry.register(Arc::new(HeadFn));
    registry.register(Arc::new(TailFn));
    registry.register(Arc::new(SampleFn));
    registry.register(Arc::new(ReplaceFn));
    registry.register(Arc::new(RegexReplaceFn));
    registry.register(Arc::new(GrepFn));
    registry.register(Arc::new(GrepInvertFn));
    registry.register(Arc::new(PrefixFn));
    registry.register(Arc::new(SuffixFn));
    registry.register(Arc::new(LinePrefixFn));
    registry.register(Arc::new(LineNumberFn));
    registry.register(Arc::new(TruncateLinesFn));
    registry.register(Arc::new(CharsetConvertFn));
    registry.register(Arc::new(Utf8ValidateFn));
    registry.register(Arc::new(JsonValidateFn));
    registry.register(Arc::new(SchemaValidateFn));
    registry.register(Arc::new(MagicBytesFn));
    registry.register(Arc::new(SizeLimitFn));
    registry.register(Arc::new(NonEmptyFn));
    registry.register(Arc::new(Sha256VerifyFn));
    registry.register(Arc::new(Crc32VerifyFn));
    registry.register(Arc::new(JsonPrettyPrintFn));
    registry.register(Arc::new(JsonMinifyFn));
    registry.register(Arc::new(CsvToJsonFn));
    registry.register(Arc::new(JsonToCsvFn));
    registry.register(Arc::new(JsonLinesFn));
    registry.register(Arc::new(YamlToJsonFn));
    registry.register(Arc::new(JsonToYamlFn));
    registry.register(Arc::new(TomlToJsonFn));
}
