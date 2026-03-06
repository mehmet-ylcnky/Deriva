//! Category R: Erasure Coding & Redundancy (9 functions, #466–#474)

use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;

fn fid(name: &str) -> FunctionId {
    FunctionId { name: name.to_string(), version: "1.0.0".to_string() }
}
fn param_usize(p: &BTreeMap<String, Value>, k: &str, def: usize) -> Result<usize, ComputeError> {
    match p.get(k) {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam(format!("{k}: must be integer"))),
        None => Ok(def),
        _ => Err(ComputeError::InvalidParam(format!("{k}: must be string"))),
    }
}
fn param_str(p: &BTreeMap<String, Value>, k: &str) -> Option<String> {
    match p.get(k) { Some(Value::String(s)) => Some(s.clone()), _ => None }
}
fn one(inputs: &[Bytes]) -> Result<&Bytes, ComputeError> {
    if inputs.is_empty() { return Err(ComputeError::InputCount { expected: 1, got: 0 }); }
    Ok(&inputs[0])
}
fn cost(sizes: &[u64]) -> ComputeCost {
    ComputeCost { cpu_ms: sizes.iter().sum::<u64>() / 100 + 50, memory_bytes: sizes.iter().sum::<u64>() * 3 + 1024 }
}

type RsField = reed_solomon_erasure::galois_8::Field;

// ---- #466 ReedSolomonEncodeFn ----
pub struct ReedSolomonEncodeFn;
impl ComputeFunction for ReedSolomonEncodeFn {
    fn id(&self) -> FunctionId { fid("rs_encode") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.is_empty() { return Err(ComputeError::ExecutionFailed("empty input".into())); }
        let d = param_usize(p, "data_shards", 4)?;
        let par = param_usize(p, "parity_shards", 2)?;
        if d == 0 || par == 0 { return Err(ComputeError::InvalidParam("shards must be ≥ 1".into())); }
        let r = reed_solomon_erasure::ReedSolomon::<RsField>::new(d, par)
            .map_err(|e| ComputeError::ExecutionFailed(format!("rs init: {e}")))?;
        let total_data = 4 + b.len();
        let ss = (total_data + d - 1) / d;
        let mut padded = Vec::with_capacity(ss * d);
        padded.extend_from_slice(&(b.len() as u32).to_le_bytes());
        padded.extend_from_slice(b);
        padded.resize(ss * d, 0u8);
        let mut shards: Vec<Vec<u8>> = padded.chunks(ss).map(|c| c.to_vec()).collect();
        for _ in 0..par { shards.push(vec![0u8; ss]); }
        r.encode(&mut shards).map_err(|e| ComputeError::ExecutionFailed(format!("rs encode: {e}")))?;
        Ok(Bytes::from(shards.concat()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #467 ReedSolomonDecodeFn ----
pub struct ReedSolomonDecodeFn;
impl ComputeFunction for ReedSolomonDecodeFn {
    fn id(&self) -> FunctionId { fid("rs_decode") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let d = param_usize(p, "data_shards", 4)?;
        let par = param_usize(p, "parity_shards", 2)?;
        let missing_str = param_str(p, "missing").unwrap_or_default();
        let missing: Vec<usize> = if missing_str.is_empty() { vec![] } else {
            missing_str.split(',').map(|s| s.trim().parse::<usize>()
                .map_err(|_| ComputeError::InvalidParam("missing: comma-separated indices".into())))
                .collect::<Result<_, _>>()?
        };
        let total = d + par;
        if missing.len() > par {
            return Err(ComputeError::ExecutionFailed(format!("too many missing shards: {} > {par}", missing.len())));
        }
        for &idx in &missing {
            if idx >= total { return Err(ComputeError::InvalidParam(format!("shard index {idx} out of range"))); }
        }
        let ss = b.len() / total;
        if ss == 0 || b.len() % total != 0 {
            return Err(ComputeError::ExecutionFailed("input not aligned to shard count".into()));
        }
        let r = reed_solomon_erasure::ReedSolomon::<RsField>::new(d, par)
            .map_err(|e| ComputeError::ExecutionFailed(format!("rs init: {e}")))?;
        let mut shards: Vec<Option<Vec<u8>>> = b.chunks(ss).enumerate()
            .map(|(i, c)| if missing.contains(&i) { None } else { Some(c.to_vec()) }).collect();
        r.reconstruct(&mut shards).map_err(|e| ComputeError::ExecutionFailed(format!("rs decode: {e}")))?;
        let data: Vec<u8> = shards[..d].iter().flat_map(|s| s.as_ref().unwrap().iter().copied()).collect();
        let original_len = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
        if 4 + original_len > data.len() {
            return Err(ComputeError::ExecutionFailed("corrupted length prefix".into()));
        }
        Ok(Bytes::from(data[4..4 + original_len].to_vec()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #468 ReedSolomonVerifyFn ----
pub struct ReedSolomonVerifyFn;
impl ComputeFunction for ReedSolomonVerifyFn {
    fn id(&self) -> FunctionId { fid("rs_verify") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let d = param_usize(p, "data_shards", 4)?;
        let par = param_usize(p, "parity_shards", 2)?;
        let total = d + par;
        let ss = b.len() / total;
        let r = reed_solomon_erasure::ReedSolomon::<RsField>::new(d, par)
            .map_err(|e| ComputeError::ExecutionFailed(format!("rs init: {e}")))?;
        let shards: Vec<Vec<u8>> = b.chunks(ss).map(|c| c.to_vec()).collect();
        let ok = r.verify(&shards).map_err(|e| ComputeError::ExecutionFailed(format!("rs verify: {e}")))?;
        if !ok { return Err(ComputeError::ExecutionFailed("parity check failed".into())); }
        Ok(b.clone())
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #469 XorParityFn ----
pub struct XorParityFn;
impl ComputeFunction for XorParityFn {
    fn id(&self) -> FunctionId { fid("xor_parity") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.is_empty() { return Err(ComputeError::ExecutionFailed("empty input".into())); }
        let sc = param_usize(p, "shard_count", 3)?;
        if sc == 0 { return Err(ComputeError::InvalidParam("shard_count must be ≥ 1".into())); }
        let total_data = 4 + b.len();
        let ss = (total_data + sc - 1) / sc;
        let mut padded = Vec::with_capacity(ss * sc);
        padded.extend_from_slice(&(b.len() as u32).to_le_bytes());
        padded.extend_from_slice(b);
        padded.resize(ss * sc, 0u8);
        let mut parity = vec![0u8; ss];
        for chunk in padded.chunks(ss) {
            for (i, &byte) in chunk.iter().enumerate() { parity[i] ^= byte; }
        }
        let mut result = padded;
        result.extend_from_slice(&parity);
        Ok(Bytes::from(result))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #470 XorReconstructFn ----
pub struct XorReconstructFn;
impl ComputeFunction for XorReconstructFn {
    fn id(&self) -> FunctionId { fid("xor_reconstruct") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let sc = param_usize(p, "shard_count", 3)?;
        let missing: usize = param_str(p, "missing")
            .ok_or_else(|| ComputeError::InvalidParam("missing: single shard index".into()))?
            .trim().parse().map_err(|_| ComputeError::InvalidParam("missing: must be integer".into()))?;
        let total = sc + 1;
        if missing >= total { return Err(ComputeError::InvalidParam(format!("shard index {missing} out of range"))); }
        let ss = b.len() / total;
        if ss == 0 || b.len() % total != 0 {
            return Err(ComputeError::ExecutionFailed("input not aligned".into()));
        }
        let shards: Vec<&[u8]> = b.chunks(ss).collect();
        let mut reconstructed = vec![0u8; ss];
        for (i, shard) in shards.iter().enumerate() {
            if i == missing { continue; }
            for (j, &byte) in shard.iter().enumerate() { reconstructed[j] ^= byte; }
        }
        let mut data = Vec::with_capacity(ss * sc);
        for i in 0..sc {
            if i == missing { data.extend_from_slice(&reconstructed); }
            else { data.extend_from_slice(shards[i]); }
        }
        let original_len = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
        if 4 + original_len > data.len() {
            return Err(ComputeError::ExecutionFailed("corrupted length prefix".into()));
        }
        Ok(Bytes::from(data[4..4 + original_len].to_vec()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #471 ReplicationSplitFn ----
pub struct ReplicationSplitFn;
impl ComputeFunction for ReplicationSplitFn {
    fn id(&self) -> FunctionId { fid("replication_split") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let replicas = param_usize(p, "replicas", 3)?;
        if replicas == 0 { return Err(ComputeError::InvalidParam("replicas must be ≥ 1".into())); }
        Ok(Bytes::from(b.repeat(replicas)))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #472 ReplicationVerifyFn ----
pub struct ReplicationVerifyFn;
impl ComputeFunction for ReplicationVerifyFn {
    fn id(&self) -> FunctionId { fid("replication_verify") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let replicas = param_usize(p, "replicas", 3)?;
        if replicas == 0 { return Err(ComputeError::InvalidParam("replicas must be ≥ 1".into())); }
        if b.len() % replicas != 0 { return Err(ComputeError::ExecutionFailed("input not divisible by replicas".into())); }
        let rs = b.len() / replicas;
        let first = &b[..rs];
        for i in 1..replicas {
            let replica = &b[i * rs..(i + 1) * rs];
            if let Some(pos) = first.iter().zip(replica.iter()).position(|(a, b)| a != b) {
                return Err(ComputeError::ExecutionFailed(format!("replica {i} differs at byte offset {pos}")));
            }
        }
        Ok(Bytes::from(first.to_vec()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #473 StripeSplitFn ----
pub struct StripeSplitFn;
impl ComputeFunction for StripeSplitFn {
    fn id(&self) -> FunctionId { fid("stripe_split") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let sc = param_usize(p, "stripe_count", 4)?;
        let ss = param_usize(p, "stripe_size", 65536)?;
        if sc == 0 { return Err(ComputeError::InvalidParam("stripe_count must be ≥ 1".into())); }
        let padded_len = ((b.len() + ss - 1) / ss) * ss;
        let mut padded = b.to_vec();
        padded.resize(padded_len, 0u8);
        let mut shards: Vec<Vec<u8>> = (0..sc).map(|_| Vec::new()).collect();
        for (i, chunk) in padded.chunks(ss).enumerate() {
            shards[i % sc].extend_from_slice(chunk);
        }
        let max_len = shards.iter().map(|s| s.len()).max().unwrap_or(0);
        for shard in &mut shards { shard.resize(max_len, 0u8); }
        let mut result = Vec::with_capacity(4 + max_len * sc);
        result.extend_from_slice(&(b.len() as u32).to_le_bytes());
        for shard in &shards { result.extend_from_slice(shard); }
        Ok(Bytes::from(result))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #474 StripeAssembleFn ----
pub struct StripeAssembleFn;
impl ComputeFunction for StripeAssembleFn {
    fn id(&self) -> FunctionId { fid("stripe_assemble") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let sc = param_usize(p, "stripe_count", 4)?;
        let ss = param_usize(p, "stripe_size", 65536)?;
        if b.len() < 4 { return Err(ComputeError::ExecutionFailed("input too short".into())); }
        let original_len = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
        let payload = &b[4..];
        if payload.len() % sc != 0 {
            return Err(ComputeError::ExecutionFailed("input size not aligned to stripe_count".into()));
        }
        let shard_size = payload.len() / sc;
        let shards: Vec<&[u8]> = payload.chunks(shard_size).collect();
        let stripes_per_shard = (shard_size + ss - 1) / ss;
        let mut result = Vec::with_capacity(original_len);
        for si in 0..stripes_per_shard {
            for shard in &shards {
                let start = si * ss;
                let end = (start + ss).min(shard.len());
                if start < shard.len() { result.extend_from_slice(&shard[start..end]); }
            }
        }
        result.truncate(original_len);
        Ok(Bytes::from(result))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
