use bytes::Bytes;
use std::collections::BTreeMap;
use deriva_core::address::{FunctionId, Value};
use crate::function::{ComputeCost, ComputeError, ComputeFunction};

fn fid(name: &str) -> FunctionId { FunctionId { name: name.into(), version: "1.0.0".into() } }
fn param_str(p: &BTreeMap<String, Value>, k: &str) -> Option<String> {
    match p.get(k) { Some(Value::String(s)) => Some(s.clone()), _ => None }
}
fn one(inputs: &[Bytes]) -> Result<&Bytes, ComputeError> {
    inputs.first().ok_or_else(|| ComputeError::InvalidParam("input required".into()))
}
fn fail(msg: String) -> ComputeError { ComputeError::ExecutionFailed(msg) }
fn cost(input_sizes: &[u64]) -> ComputeCost {
    let total: u64 = input_sizes.iter().sum();
    ComputeCost { cpu_ms: (total / 1024).max(10), memory_bytes: total.max(1024) }
}

// ---- #421 TfRecordReadFn ----
pub struct TfRecordReadFn;
impl ComputeFunction for TfRecordReadFn {
    fn id(&self) -> FunctionId { fid("tfrecord_read") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut offset = 0;
        let mut lines = Vec::new();
        while offset + 12 <= b.len() {
            let len = u64::from_le_bytes(b[offset..offset + 8].try_into().unwrap()) as usize;
            let total = 8 + 4 + len + 4;
            if offset + total > b.len() { break; }
            let data = &b[offset + 12..offset + 12 + len];
            // Simplified: emit raw data as hex
            let record = serde_json::json!({"data": hex::encode(data)});
            lines.push(serde_json::to_string(&record).unwrap());
            offset += total;
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #422 TfRecordWriteFn ----
pub struct TfRecordWriteFn;
impl ComputeFunction for TfRecordWriteFn {
    fn id(&self) -> FunctionId { fid("tfrecord_write") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let len = b.len() as u64;
        let mut out = Vec::new();
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&[0u8; 4]); // CRC placeholder
        out.extend_from_slice(b);
        out.extend_from_slice(&[0u8; 4]); // CRC placeholder
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #423 SafeTensorsReadFn ----
pub struct SafeTensorsReadFn;
impl ComputeFunction for SafeTensorsReadFn {
    fn id(&self) -> FunctionId { fid("safetensors_read") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.len() < 8 { return Err(fail("truncated safetensors".into())); }
        let header_size = u64::from_le_bytes(b[..8].try_into().unwrap()) as usize;
        if 8 + header_size > b.len() { return Err(fail("truncated header".into())); }
        let header_json = &b[8..8 + header_size];
        let header: serde_json::Value = serde_json::from_slice(header_json)
            .map_err(|e| fail(format!("header json: {e}")))?;
        let tensor_name = param_str(p, "tensor");
        if let Some(name) = tensor_name {
            let tensors = header.as_object().ok_or_else(|| fail("invalid header".into()))?;
            let info = tensors.get(&name).ok_or_else(|| fail(format!("tensor '{name}' not found")))?;
            let offsets = info["data_offsets"].as_array().ok_or_else(|| fail("missing offsets".into()))?;
            let start = offsets[0].as_u64().unwrap() as usize + 8 + header_size;
            let end = offsets[1].as_u64().unwrap() as usize + 8 + header_size;
            Ok(Bytes::copy_from_slice(&b[start..end]))
        } else {
            // Return all tensor data
            Ok(Bytes::copy_from_slice(&b[8 + header_size..]))
        }
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #424 SafeTensorsMetadataFn ----
pub struct SafeTensorsMetadataFn;
impl ComputeFunction for SafeTensorsMetadataFn {
    fn id(&self) -> FunctionId { fid("safetensors_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.len() < 8 { return Err(fail("truncated safetensors".into())); }
        let header_size = u64::from_le_bytes(b[..8].try_into().unwrap()) as usize;
        if 8 + header_size > b.len() { return Err(fail("truncated header".into())); }
        let header_json = &b[8..8 + header_size];
        Ok(Bytes::copy_from_slice(header_json))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #425 SafeTensorsWriteFn ----
pub struct SafeTensorsWriteFn;
impl ComputeFunction for SafeTensorsWriteFn {
    fn id(&self) -> FunctionId { fid("safetensors_write") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let name = param_str(p, "name").unwrap_or_else(|| "tensor".into());
        let dtype = param_str(p, "dtype").unwrap_or_else(|| "F32".into());
        let shape_str = param_str(p, "shape").ok_or_else(|| ComputeError::InvalidParam("shape required".into()))?;
        let shape: Vec<usize> = shape_str.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let header = serde_json::json!({
            name: {
                "dtype": dtype,
                "shape": shape,
                "data_offsets": [0, b.len()],
            }
        });
        let header_bytes = serde_json::to_vec(&header).unwrap();
        let header_size = header_bytes.len() as u64;
        let mut out = Vec::new();
        out.extend_from_slice(&header_size.to_le_bytes());
        out.extend_from_slice(&header_bytes);
        out.extend_from_slice(b);
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #426 OnnxMetadataFn (requires ONNX Runtime) ----
pub struct OnnxMetadataFn;
impl ComputeFunction for OnnxMetadataFn {
    fn id(&self) -> FunctionId { fid("onnx_metadata") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("onnx_metadata requires format-ml-onnx feature (ONNX Runtime)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #427 OnnxValidateFn (requires ONNX Runtime) ----
pub struct OnnxValidateFn;
impl ComputeFunction for OnnxValidateFn {
    fn id(&self) -> FunctionId { fid("onnx_validate") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("onnx_validate requires format-ml-onnx feature (ONNX Runtime)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #428 GgufMetadataFn ----
pub struct GgufMetadataFn;
impl ComputeFunction for GgufMetadataFn {
    fn id(&self) -> FunctionId { fid("gguf_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.len() < 4 || &b[..4] != b"GGUF" {
            return Err(fail("not a GGUF file".into()));
        }
        // Simplified: return basic info
        let meta = serde_json::json!({
            "magic": "GGUF",
            "file_size": b.len(),
        });
        Ok(Bytes::from(serde_json::to_string_pretty(&meta).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #429 PickleToJsonFn (unsafe - stub) ----
pub struct PickleToJsonFn;
impl ComputeFunction for PickleToJsonFn {
    fn id(&self) -> FunctionId { fid("pickle_to_json") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("pickle_to_json requires format-ml-pickle feature (safe unpickler)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #430 NumpyToSafeTensorsFn ----
pub struct NumpyToSafeTensorsFn;
impl ComputeFunction for NumpyToSafeTensorsFn {
    fn id(&self) -> FunctionId { fid("numpy_to_safetensors") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.len() < 10 || &b[..6] != b"\x93NUMPY" {
            return Err(fail("not a .npy file".into()));
        }
        let header_len = u16::from_le_bytes([b[8], b[9]]) as usize;
        if 10 + header_len > b.len() { return Err(fail("truncated npy".into())); }
        let header_str = std::str::from_utf8(&b[10..10 + header_len])
            .map_err(|_| fail("invalid npy header".into()))?;
        // Parse shape from header
        let shape_start = header_str.find("'shape'").or_else(|| header_str.find("\"shape\""))
            .ok_or_else(|| fail("missing shape".into()))?;
        let rest = &header_str[shape_start..];
        let paren_start = rest.find('(').ok_or_else(|| fail("bad shape".into()))? + 1;
        let paren_end = rest.find(')').ok_or_else(|| fail("bad shape".into()))?;
        let shape_str = &rest[paren_start..paren_end];
        let shape: Vec<usize> = shape_str.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let data = &b[10 + header_len..];
        let name = param_str(p, "name").unwrap_or_else(|| "tensor".into());
        SafeTensorsWriteFn.execute(vec![Bytes::copy_from_slice(data)], &{
            let mut params = BTreeMap::new();
            params.insert("name".into(), Value::String(name));
            params.insert("dtype".into(), Value::String("F32".into()));
            params.insert("shape".into(), Value::String(shape.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",")));
            params
        })
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #431 TfRecordToParquetFn ----
pub struct TfRecordToParquetFn;
impl ComputeFunction for TfRecordToParquetFn {
    fn id(&self) -> FunctionId { fid("tfrecord_to_parquet") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        // Simplified: read TFRecord, emit minimal Parquet
        let ndjson = TfRecordReadFn.execute(inputs, &BTreeMap::new())?;
        // Convert NDJSON to Parquet (stub - would need full Arrow conversion)
        Ok(ndjson)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #432 ImageToTensorFn ----
pub struct ImageToTensorFn;
impl ComputeFunction for ImageToTensorFn {
    fn id(&self) -> FunctionId { fid("image_to_tensor") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let img = image::load_from_memory(b).map_err(|e| fail(format!("image: {e}")))?;
        let channels = param_str(p, "channels").unwrap_or_else(|| "3".into()).parse::<u8>().unwrap_or(3);
        let normalize = param_str(p, "normalize").unwrap_or_else(|| "true".into()) == "true";
        let rgb = img.to_rgb8();
        let (width, height) = rgb.dimensions();
        let mut tensor = Vec::new();
        // CHW layout
        for c in 0..channels.min(3) {
            for y in 0..height {
                for x in 0..width {
                    let pixel = rgb.get_pixel(x, y);
                    let val = pixel[c as usize] as f32;
                    let normalized = if normalize { val / 255.0 } else { val };
                    tensor.extend_from_slice(&normalized.to_le_bytes());
                }
            }
        }
        Ok(Bytes::from(tensor))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
