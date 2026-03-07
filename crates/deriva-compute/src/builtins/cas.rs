use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;
use super::{get_string_param, hex_decode_param};
use deriva_core::address::CAddr;

pub struct CAddrComputeFn;

impl ComputeFunction for CAddrComputeFn {
    fn id(&self) -> FunctionId { FunctionId::new("caddr_compute", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let addr = CAddr::from_bytes(&inputs[0]);
        Ok(Bytes::copy_from_slice(addr.as_bytes()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #93 CAddrVerifyFn ──

pub struct CAddrVerifyFn;

impl ComputeFunction for CAddrVerifyFn {
    fn id(&self) -> FunctionId { FunctionId::new("caddr_verify", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let expected_hex = get_string_param(params, "expected_caddr")?;
        let expected = hex_decode_param(expected_hex, "expected_caddr")?;
        if expected.len() != 32 { return Err(ComputeError::InvalidParam("expected_caddr must be 32 bytes (64 hex chars)".into())); }
        let actual = CAddr::from_bytes(&inputs[0]);
        if actual.as_bytes() != expected.as_slice() {
            return Err(ComputeError::ExecutionFailed(format!("CAddr mismatch: expected {}, got {}",
                expected_hex, actual.as_bytes().iter().map(|b| format!("{:02x}", b)).collect::<String>())));
        }
        Ok(inputs[0].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #94 CAddrEmbedFn ──

pub struct CAddrEmbedFn;

impl ComputeFunction for CAddrEmbedFn {
    fn id(&self) -> FunctionId { FunctionId::new("caddr_embed", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let addr = CAddr::from_bytes(&inputs[0]);
        let mut out = Vec::with_capacity(inputs[0].len() + 32);
        out.extend_from_slice(&inputs[0]);
        out.extend_from_slice(addr.as_bytes());
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #95 MerkleRootFn ──

pub struct MerkleRootFn;

impl ComputeFunction for MerkleRootFn {
    fn id(&self) -> FunctionId { FunctionId::new("merkle_root", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        use sha2::{Sha256, Digest};
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let bs: usize = match params.get("block_size") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("block_size must be positive".into()))?,
            None => 65536,
            _ => return Err(ComputeError::InvalidParam("block_size must be a string".into())),
        };
        if bs == 0 { return Err(ComputeError::InvalidParam("block_size must be > 0".into())); }
        let input = &inputs[0];
        if input.is_empty() { return Ok(Bytes::copy_from_slice(&Sha256::digest(b""))); }
        let mut hashes: Vec<[u8; 32]> = input.chunks(bs).map(|c| Sha256::digest(c).into()).collect();
        while hashes.len() > 1 {
            let mut next = Vec::with_capacity(hashes.len().div_ceil(2));
            for pair in hashes.chunks(2) {
                if pair.len() == 2 {
                    let mut h = Sha256::new(); h.update(pair[0]); h.update(pair[1]);
                    next.push(h.finalize().into());
                } else { next.push(pair[0]); }
            }
            hashes = next;
        }
        Ok(Bytes::copy_from_slice(&hashes[0]))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
}

// ── #96 ContentTypeFn ──

pub struct ContentTypeFn;

impl ComputeFunction for ContentTypeFn {
    fn id(&self) -> FunctionId { FunctionId::new("content_type", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let input = &inputs[0];
        let mime = if input.starts_with(b"\x89PNG\r\n\x1a\n") { "image/png" }
            else if input.starts_with(b"\xff\xd8\xff") { "image/jpeg" }
            else if input.starts_with(b"GIF87a") || input.starts_with(b"GIF89a") { "image/gif" }
            else if input.starts_with(b"PK\x03\x04") { "application/zip" }
            else if input.starts_with(b"\x1f\x8b") { "application/gzip" }
            else if input.starts_with(b"%PDF") { "application/pdf" }
            else if input.starts_with(b"\x28\xb5\x2f\xfd") { "application/zstd" }
            else if input.starts_with(b"{") || input.starts_with(b"[") { "application/json" }
            else if std::str::from_utf8(input).is_ok() { "text/plain" }
            else { "application/octet-stream" };
        Ok(Bytes::from(mime))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #97 ChunkHashFn ──

pub struct ChunkHashFn;

impl ComputeFunction for ChunkHashFn {
    fn id(&self) -> FunctionId { FunctionId::new("chunk_hash", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        use sha2::{Sha256, Digest};
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let bs: usize = match params.get("block_size") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("block_size must be positive".into()))?,
            None => 65536,
            _ => return Err(ComputeError::InvalidParam("block_size must be a string".into())),
        };
        if bs == 0 { return Err(ComputeError::InvalidParam("block_size must be > 0".into())); }
        let input = &inputs[0];
        let mut out = Vec::with_capacity((input.len() / bs + 1) * 32);
        for chunk in input.chunks(bs) { out.extend_from_slice(&Sha256::digest(chunk)); }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #98 DedupAnalyzeFn ──

pub struct DedupAnalyzeFn;

impl ComputeFunction for DedupAnalyzeFn {
    fn id(&self) -> FunctionId { FunctionId::new("dedup_analyze", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let window: usize = match params.get("window_size") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("window_size must be positive".into()))?,
            None => 48,
            _ => return Err(ComputeError::InvalidParam("window_size must be a string".into())),
        };
        let input = &inputs[0];
        let min_chunk = 2048;
        let max_chunk = 65536;
        let mask: u64 = 0x1FFF;
        let mut boundaries = vec![0u64];
        let mut hash: u64 = 0;
        let mut last_boundary = 0usize;
        for (i, &b) in input.iter().enumerate() {
            hash = hash.wrapping_mul(31).wrapping_add(b as u64);
            if i >= window {
                let old = input[i - window] as u64;
                hash = hash.wrapping_sub(old.wrapping_mul(31u64.wrapping_pow(window as u32)));
            }
            let chunk_len = i - last_boundary + 1;
            if (chunk_len >= min_chunk && (hash & mask) == 0) || chunk_len >= max_chunk {
                boundaries.push((i + 1) as u64);
                last_boundary = i + 1;
                hash = 0;
            }
        }
        if last_boundary < input.len() { boundaries.push(input.len() as u64); }
        let json = serde_json::to_string(&boundaries).map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?;
        Ok(Bytes::from(json))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
}

// ── #99 ReverseByteFn ──

