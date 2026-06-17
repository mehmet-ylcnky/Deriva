use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;
use super::{get_string_param, parse_usize_param};

pub struct ReplaceFn;

impl ComputeFunction for ReplaceFn {
    fn id(&self) -> FunctionId { FunctionId::new("replace", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let pattern = get_string_param(params, "pattern")?;
        let replacement = get_string_param(params, "replacement")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("replace requires UTF-8 input".into()))?;
        let re = regex::Regex::new(pattern).map_err(|e| ComputeError::InvalidParam(format!("invalid regex: {}", e)))?;
        Ok(Bytes::from(re.replace_all(text, replacement).into_owned()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #76 Utf8ValidateFn (in validation.rs) ──

// ── SplitFn ──

pub struct SplitFn;

impl ComputeFunction for SplitFn {
    fn id(&self) -> FunctionId { FunctionId::new("split", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let delimiter = get_string_param(params, "delimiter")?;
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("split requires UTF-8 input".into()))?;
        let parts: Vec<&str> = text.split(delimiter).collect();
        Ok(Bytes::from(parts.join("\x00")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── JoinFn ──

pub struct JoinFn;

impl ComputeFunction for JoinFn {
    fn id(&self) -> FunctionId { FunctionId::new("join", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() { return Err(ComputeError::InputCount { expected: 1, got: 0 }); }
        let separator = get_string_param(params, "separator")?;
        let parts: Result<Vec<&str>, _> = inputs.iter()
            .map(|b| std::str::from_utf8(b.as_ref()).map_err(|_| ComputeError::ExecutionFailed("join requires UTF-8 inputs".into())))
            .collect();
        let parts = parts?;
        Ok(Bytes::from(parts.join(separator)))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── SortLinesFn ──

pub struct SortLinesFn;

impl ComputeFunction for SortLinesFn {
    fn id(&self) -> FunctionId { FunctionId::new("sort_lines", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("sort_lines requires UTF-8 input".into()))?;
        let has_trailing_newline = text.ends_with('\n');
        let mut lines: Vec<&str> = text.lines().collect();
        lines.sort();
        let mut result = lines.join("\n");
        if has_trailing_newline {
            result.push('\n');
        }
        Ok(Bytes::from(result))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
}

// ── UniqueLinesFn ──

pub struct UniqueLinesFn;

impl ComputeFunction for UniqueLinesFn {
    fn id(&self) -> FunctionId { FunctionId::new("unique_lines", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("unique_lines requires UTF-8 input".into()))?;
        let mut result = Vec::new();
        let mut prev: Option<&str> = None;
        for line in text.lines() {
            if prev != Some(line) {
                result.push(line);
                prev = Some(line);
            }
        }
        Ok(Bytes::from(result.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}
