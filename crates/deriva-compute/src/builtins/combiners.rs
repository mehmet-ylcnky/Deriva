use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;
use super::{parse_usize_param, get_string_param};

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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── #56 TakeFn ──

