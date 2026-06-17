use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;
use super::{parse_usize_param, parse_u64_param};

pub struct TakeFn;

impl ComputeFunction for TakeFn {
    fn id(&self) -> FunctionId { FunctionId::new("take", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let n: usize = parse_usize_param(params, "bytes")?;
        let end = n.min(inputs[0].len());
        Ok(inputs[0].slice(..end))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #63 HeadFn ──

pub struct HeadFn;

impl ComputeFunction for HeadFn {
    fn id(&self) -> FunctionId { FunctionId::new("head", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let n: usize = parse_usize_param(params, "n")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("head requires UTF-8 input".into()))?;
        let result: Vec<&str> = text.lines().take(n).collect();
        Ok(Bytes::from(result.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── #64 TailFn ──

pub struct TailFn;

impl ComputeFunction for TailFn {
    fn id(&self) -> FunctionId { FunctionId::new("tail", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let n: usize = parse_usize_param(params, "n")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("tail requires UTF-8 input".into()))?;
        let all_lines: Vec<&str> = text.lines().collect();
        let start = all_lines.len().saturating_sub(n);
        Ok(Bytes::from(all_lines[start..].join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #66 ReplaceFn ──

