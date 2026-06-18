//! Shared utilities for format-aware functions.
//!
//! Provides consistent parameter parsing, error formatting, and cost estimation
//! used across all format-aware function categories (§2.16).

use crate::function::{ComputeCost, ComputeError};
use deriva_core::address::Value;
use std::collections::BTreeMap;

// === Parameter parsing helpers ===

/// Parse the `columns` parameter (comma-separated column names).
/// Returns the list of column names, trimmed.
///
/// Convention (Requirement 8.1): all column/field selection uses key "columns"
/// with comma-separated values.
/// Example: "name,age,city" → vec!["name", "age", "city"]
pub fn parse_columns_param(params: &BTreeMap<String, Value>) -> Result<Vec<String>, ComputeError> {
    match params.get("columns") {
        Some(Value::String(s)) if !s.is_empty() => {
            Ok(s.split(',').map(|c| c.trim().to_string()).collect())
        }
        Some(Value::String(_)) => Err(ComputeError::InvalidParam(
            "columns: must be a non-empty comma-separated list".to_string(),
        )),
        None => Err(ComputeError::InvalidParam(
            "missing param 'columns': comma-separated column names expected".to_string(),
        )),
        _ => Err(ComputeError::InvalidParam(
            "columns: must be a string value".to_string(),
        )),
    }
}

/// Parse the optional `columns` parameter. Returns None if not present.
pub fn parse_columns_param_optional(
    params: &BTreeMap<String, Value>,
) -> Result<Option<Vec<String>>, ComputeError> {
    match params.get("columns") {
        Some(Value::String(s)) if !s.is_empty() => {
            Ok(Some(s.split(',').map(|c| c.trim().to_string()).collect()))
        }
        Some(Value::String(_)) => Err(ComputeError::InvalidParam(
            "columns: must be a non-empty comma-separated list if provided".to_string(),
        )),
        None => Ok(None),
        _ => Err(ComputeError::InvalidParam(
            "columns: must be a string value".to_string(),
        )),
    }
}

/// Parse the `filter` parameter (filter expression string).
/// Convention (Requirement 8.2): all filter/predicate parameters use key "filter".
pub fn parse_filter_param(params: &BTreeMap<String, Value>) -> Result<String, ComputeError> {
    match params.get("filter") {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(Value::String(_)) => Err(ComputeError::InvalidParam(
            "filter: must be a non-empty expression".to_string(),
        )),
        None => Err(ComputeError::InvalidParam(
            "missing param 'filter': filter expression expected".to_string(),
        )),
        _ => Err(ComputeError::InvalidParam(
            "filter: must be a string value".to_string(),
        )),
    }
}

/// Parse the optional `filter` parameter. Returns None if not present.
pub fn parse_filter_param_optional(
    params: &BTreeMap<String, Value>,
) -> Result<Option<String>, ComputeError> {
    match params.get("filter") {
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
        Some(Value::String(_)) => Err(ComputeError::InvalidParam(
            "filter: must be a non-empty expression if provided".to_string(),
        )),
        None => Ok(None),
        _ => Err(ComputeError::InvalidParam(
            "filter: must be a string value".to_string(),
        )),
    }
}

/// Parse the `output_format` parameter (target format identifier).
/// Convention (Requirement 8.3): all output format params use key "output_format".
pub fn parse_output_format_param(
    params: &BTreeMap<String, Value>,
) -> Result<String, ComputeError> {
    match params.get("output_format") {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(Value::String(_)) => Err(ComputeError::InvalidParam(
            "output_format: must be a non-empty format identifier".to_string(),
        )),
        None => Err(ComputeError::InvalidParam(
            "missing param 'output_format': format identifier expected (e.g., \"json\", \"csv\", \"parquet\")".to_string(),
        )),
        _ => Err(ComputeError::InvalidParam(
            "output_format: must be a string value".to_string(),
        )),
    }
}

/// Parse the optional `output_format` parameter. Returns None if not present.
pub fn parse_output_format_param_optional(
    params: &BTreeMap<String, Value>,
) -> Result<Option<String>, ComputeError> {
    match params.get("output_format") {
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
        None => Ok(None),
        _ => Err(ComputeError::InvalidParam(
            "output_format: must be a non-empty string if provided".to_string(),
        )),
    }
}

/// Parse the `quality` parameter (numeric quality/compression level, 0-100).
/// Convention (Requirement 8.4): all quality/compression parameters use key "quality".
pub fn parse_quality_param(params: &BTreeMap<String, Value>) -> Result<u8, ComputeError> {
    match params.get("quality") {
        Some(Value::String(s)) => {
            let q: i64 = s.parse().map_err(|_| {
                ComputeError::InvalidParam("quality: must be an integer 0-100".to_string())
            })?;
            if q < 0 || q > 100 {
                return Err(ComputeError::InvalidParam(
                    "quality: must be in range 0-100".to_string(),
                ));
            }
            Ok(q as u8)
        }
        Some(Value::Int(n)) => {
            if *n < 0 || *n > 100 {
                return Err(ComputeError::InvalidParam(
                    "quality: must be in range 0-100".to_string(),
                ));
            }
            Ok(*n as u8)
        }
        None => Err(ComputeError::InvalidParam(
            "missing param 'quality': integer 0-100 expected".to_string(),
        )),
        _ => Err(ComputeError::InvalidParam(
            "quality: must be a numeric value 0-100".to_string(),
        )),
    }
}

/// Parse the optional `quality` parameter with a default value.
pub fn parse_quality_param_or_default(
    params: &BTreeMap<String, Value>,
    default: u8,
) -> Result<u8, ComputeError> {
    match params.get("quality") {
        None => Ok(default),
        _ => parse_quality_param(params),
    }
}

/// Parse the `path` parameter (dot-notation or JSONPath for nested access).
/// Convention (Requirement 8.5): all schema path parameters use key "path".
pub fn parse_path_param(params: &BTreeMap<String, Value>) -> Result<String, ComputeError> {
    match params.get("path") {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(Value::String(_)) => Err(ComputeError::InvalidParam(
            "path: must be a non-empty path expression".to_string(),
        )),
        None => Err(ComputeError::InvalidParam(
            "missing param 'path': dot-notation or JSONPath expected (e.g., \"$.data.items\")".to_string(),
        )),
        _ => Err(ComputeError::InvalidParam(
            "path: must be a string value".to_string(),
        )),
    }
}

/// Parse the optional `path` parameter. Returns None if not present.
pub fn parse_path_param_optional(
    params: &BTreeMap<String, Value>,
) -> Result<Option<String>, ComputeError> {
    match params.get("path") {
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
        None => Ok(None),
        _ => Err(ComputeError::InvalidParam(
            "path: must be a non-empty string if provided".to_string(),
        )),
    }
}

// === Error formatting helpers ===

/// Format a function error following the convention: "{function}: {error_detail}"
/// (Requirement 9.1)
pub fn format_error(function: &str, detail: &str) -> ComputeError {
    ComputeError::ExecutionFailed(format!("{}: {}", function, detail))
}

/// Format a parse error with location information.
/// Example: "csv_parse: unclosed quote at row 5"
pub fn format_parse_error(
    function: &str,
    detail: &str,
    location: Option<&str>,
) -> ComputeError {
    match location {
        Some(loc) => {
            ComputeError::ExecutionFailed(format!("{}: {} at {}", function, detail, loc))
        }
        None => ComputeError::ExecutionFailed(format!("{}: {}", function, detail)),
    }
}

/// Format a "not found" error listing available items.
/// Example: "parquet_project: column 'foo' not found, available: [bar, baz, qux]"
pub fn format_not_found_error(
    function: &str,
    kind: &str,
    requested: &str,
    available: &[&str],
) -> ComputeError {
    let avail_str = available.join(", ");
    ComputeError::ExecutionFailed(format!(
        "{}: {} '{}' not found, available: [{}]",
        function, kind, requested, avail_str
    ))
}

/// Format a missing parameter error.
/// Example: "csv_filter: missing param 'filter'"
pub fn format_missing_param(function: &str, param: &str) -> ComputeError {
    ComputeError::InvalidParam(format!("{}: missing param '{}'", function, param))
}

// === Cost estimation helpers ===

/// Standard cost for format-aware functions.
/// Uses §4.2 formula: base_cost * (total_input_bytes / 1024).max(1)
pub fn format_cost(base_cpu_ms: u64, input_sizes: &[u64]) -> ComputeCost {
    let total: u64 = input_sizes.iter().sum();
    let scale = (total / 1024).max(1);
    ComputeCost {
        cpu_ms: base_cpu_ms * scale,
        memory_bytes: total,
    }
}

/// Cost for format-aware functions with memory amplification factor.
/// Some operations (decompression, XML parsing) use more memory than input size.
pub fn format_cost_amplified(
    base_cpu_ms: u64,
    input_sizes: &[u64],
    memory_factor: u64,
) -> ComputeCost {
    let total: u64 = input_sizes.iter().sum();
    let scale = (total / 1024).max(1);
    ComputeCost {
        cpu_ms: base_cpu_ms * scale,
        memory_bytes: total * memory_factor,
    }
}

/// Light cost for simple parsing/detection operations (low CPU per byte).
pub fn format_cost_light(input_sizes: &[u64]) -> ComputeCost {
    format_cost(10, input_sizes)
}

/// Medium cost for standard format operations (parse, filter, project).
pub fn format_cost_medium(input_sizes: &[u64]) -> ComputeCost {
    format_cost(50, input_sizes)
}

/// Heavy cost for computationally intensive operations (compression, image resize, erasure coding).
pub fn format_cost_heavy(input_sizes: &[u64]) -> ComputeCost {
    format_cost(100, input_sizes)
}
