use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;

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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #88 JsonLinesFn ──

pub struct JsonLinesFn;

impl ComputeFunction for JsonLinesFn {
    fn id(&self) -> FunctionId { FunctionId::new("json_lines", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
        let text = std::str::from_utf8(&inputs[0]).map_err(|_| ComputeError::ExecutionFailed("json_lines requires UTF-8".into()))?;
        let array: Vec<serde_json::Value> = serde_json::from_str(text).map_err(|e| ComputeError::ExecutionFailed(format!("expected JSON array: {}", e)))?;
        let lines: Vec<String> = array.iter().map(serde_json::to_string).collect::<Result<_, _>>().map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
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
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #92 CAddrComputeFn ──

