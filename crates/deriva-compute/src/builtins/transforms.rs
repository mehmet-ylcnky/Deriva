use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;
use super::{parse_byte_param, parse_usize_param};

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

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost { ComputeCost { cpu_ms: 0, memory_bytes: 0 } }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
        if !input.len().is_multiple_of(2) {
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
            if !input.len().is_multiple_of(8) {
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
        if !input.len().is_multiple_of(ws) {
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
        out.extend(std::iter::repeat_n(pad_len as u8, pad_len));
        Ok(Bytes::from(out))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

pub struct Base64UrlEncodeFn;

impl ComputeFunction for Base64UrlEncodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("base64url_encode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use base64::Engine;
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&inputs[0]);
        Ok(Bytes::from(encoded))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

pub struct Base64UrlDecodeFn;

impl ComputeFunction for Base64UrlDecodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("base64url_decode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&inputs[0])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("invalid base64url: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

pub struct Base58EncodeFn;

impl ComputeFunction for Base58EncodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("base58_encode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let encoded = bs58::encode(&inputs[0]).with_alphabet(bs58::Alphabet::BITCOIN).into_string();
        Ok(Bytes::from(encoded))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

pub struct Base58DecodeFn;

impl ComputeFunction for Base58DecodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("base58_decode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        bs58::decode(&inputs[0])
            .with_alphabet(bs58::Alphabet::BITCOIN)
            .into_vec()
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("invalid base58: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

pub struct UrlEncodeFn;

impl ComputeFunction for UrlEncodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("url_encode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use percent_encoding::{percent_encode, NON_ALPHANUMERIC, AsciiSet};
        // RFC 3986 unreserved characters: ALPHA / DIGIT / "-" / "." / "_" / "~"
        // We encode everything except those.
        const RFC3986_UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'.')
            .remove(b'_')
            .remove(b'~');
        let encoded = percent_encode(&inputs[0], RFC3986_UNRESERVED).to_string();
        Ok(Bytes::from(encoded))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

pub struct UrlDecodeFn;

impl ComputeFunction for UrlDecodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("url_decode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use percent_encoding::percent_decode;
        let decoded = percent_decode(&inputs[0]).collect::<Vec<u8>>();
        Ok(Bytes::from(decoded))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

pub struct HtmlEncodeFn;

impl ComputeFunction for HtmlEncodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("html_encode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let input = &inputs[0];
        let mut out = Vec::with_capacity(input.len());
        for &b in input.iter() {
            match b {
                b'&' => out.extend_from_slice(b"&amp;"),
                b'<' => out.extend_from_slice(b"&lt;"),
                b'>' => out.extend_from_slice(b"&gt;"),
                b'"' => out.extend_from_slice(b"&quot;"),
                b'\'' => out.extend_from_slice(b"&#39;"),
                _ => out.push(b),
            }
        }
        Ok(Bytes::from(out))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

pub struct HtmlDecodeFn;

impl ComputeFunction for HtmlDecodeFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("html_decode", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let input = &inputs[0];
        let input_str = std::str::from_utf8(input)
            .map_err(|e| ComputeError::ExecutionFailed(format!("invalid utf-8 for html_decode: {}", e)))?;
        let mut out = Vec::with_capacity(input.len());
        let mut chars = input_str.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '&' {
                // Collect entity until ';' or end
                let mut entity = String::new();
                let mut found_semicolon = false;
                for next_ch in chars.by_ref() {
                    if next_ch == ';' {
                        found_semicolon = true;
                        break;
                    }
                    entity.push(next_ch);
                    // Limit entity length to prevent runaway
                    if entity.len() > 10 {
                        break;
                    }
                }
                if found_semicolon {
                    match entity.as_str() {
                        "amp" => out.extend_from_slice(b"&"),
                        "lt" => out.extend_from_slice(b"<"),
                        "gt" => out.extend_from_slice(b">"),
                        "quot" => out.extend_from_slice(b"\""),
                        "apos" => out.extend_from_slice(b"'"),
                        _ if entity.starts_with('#') => {
                            // Numeric entity
                            let num_str = &entity[1..];
                            let code_point = if let Some(hex_str) = num_str.strip_prefix('x') {
                                u32::from_str_radix(hex_str, 16).ok()
                            } else {
                                num_str.parse::<u32>().ok()
                            };
                            match code_point.and_then(char::from_u32) {
                                Some(decoded_char) => {
                                    let mut buf = [0u8; 4];
                                    out.extend_from_slice(decoded_char.encode_utf8(&mut buf).as_bytes());
                                }
                                None => {
                                    return Err(ComputeError::ExecutionFailed(
                                        format!("invalid html entity: &{};", entity)
                                    ));
                                }
                            }
                        }
                        _ => {
                            return Err(ComputeError::ExecutionFailed(
                                format!("invalid html entity: &{};", entity)
                            ));
                        }
                    }
                } else {
                    // No semicolon found — invalid entity
                    return Err(ComputeError::ExecutionFailed(
                        "invalid html: unterminated entity reference".into()
                    ));
                }
            } else {
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
        }
        Ok(Bytes::from(out))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #21 CompressFn (zlib) ──

