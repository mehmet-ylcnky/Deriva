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
}
