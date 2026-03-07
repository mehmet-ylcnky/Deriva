use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;

pub struct ByteCountFn;

impl ComputeFunction for ByteCountFn {
    fn id(&self) -> FunctionId { FunctionId::new("byte_count", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        Ok(Bytes::copy_from_slice(&(inputs[0].len() as u64).to_be_bytes()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #50 InterleaveFn ──

