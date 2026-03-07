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

// ---- #453 FontMetadataFn (requires font parsing) ----
pub struct FontMetadataFn;
impl ComputeFunction for FontMetadataFn {
    fn id(&self) -> FunctionId { fid("font_metadata") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("font_metadata requires format-binary-font feature".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #454 WoffToOtfFn (requires woff2) ----
pub struct WoffToOtfFn;
impl ComputeFunction for WoffToOtfFn {
    fn id(&self) -> FunctionId { fid("woff_to_otf") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("woff_to_otf requires format-binary-font feature (woff2)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #455 OtfToWoff2Fn (requires woff2) ----
pub struct OtfToWoff2Fn;
impl ComputeFunction for OtfToWoff2Fn {
    fn id(&self) -> FunctionId { fid("otf_to_woff2") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("otf_to_woff2 requires format-binary-font feature (woff2)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #456 GltfMetadataFn (requires gltf) ----
pub struct GltfMetadataFn;
impl ComputeFunction for GltfMetadataFn {
    fn id(&self) -> FunctionId { fid("gltf_metadata") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("gltf_metadata requires format-binary-gltf feature (gltf)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #457 GltfValidateFn (requires gltf) ----
pub struct GltfValidateFn;
impl ComputeFunction for GltfValidateFn {
    fn id(&self) -> FunctionId { fid("gltf_validate") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("gltf_validate requires format-binary-gltf feature (gltf)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #458 StlReadFn ----
pub struct StlReadFn;
impl ComputeFunction for StlReadFn {
    fn id(&self) -> FunctionId { fid("stl_read") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.len() < 5 { return Err(fail("truncated STL".into())); }
        let format = if &b[..5] == b"solid" { "ascii" } else { "binary" };
        let triangle_count = if format == "binary" {
            if b.len() < 84 { return Err(fail("truncated binary STL".into())); }
            u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize
        } else {
            b.iter().filter(|&&c| c == b'f').count() / 5 // rough estimate
        };
        let result = serde_json::json!({
            "format": format,
            "triangle_count": triangle_count,
        });
        Ok(Bytes::from(serde_json::to_string_pretty(&result).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #459 StlConvertFn ----
pub struct StlConvertFn;
impl ComputeFunction for StlConvertFn {
    fn id(&self) -> FunctionId { fid("stl_convert") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        // Simplified: just pass through
        Ok(b.clone())
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #460 DicomMetadataFn (requires dicom-object) ----
pub struct DicomMetadataFn;
impl ComputeFunction for DicomMetadataFn {
    fn id(&self) -> FunctionId { fid("dicom_metadata") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("dicom_metadata requires format-binary-dicom feature (dicom-object)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #461 DicomToImageFn (requires dicom-object) ----
pub struct DicomToImageFn;
impl ComputeFunction for DicomToImageFn {
    fn id(&self) -> FunctionId { fid("dicom_to_image") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("dicom_to_image requires format-binary-dicom feature (dicom-object)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #462 DicomAnonymizeFn (requires dicom-object) ----
pub struct DicomAnonymizeFn;
impl ComputeFunction for DicomAnonymizeFn {
    fn id(&self) -> FunctionId { fid("dicom_anonymize") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("dicom_anonymize requires format-binary-dicom feature (dicom-object)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #463 WasmValidateFn ----
pub struct WasmValidateFn;
impl ComputeFunction for WasmValidateFn {
    fn id(&self) -> FunctionId { fid("wasm_validate") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.len() < 8 || &b[..4] != b"\0asm" {
            return Err(fail("not a WASM module".into()));
        }
        Ok(b.clone())
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #464 WasmMetadataFn ----
pub struct WasmMetadataFn;
impl ComputeFunction for WasmMetadataFn {
    fn id(&self) -> FunctionId { fid("wasm_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.len() < 8 || &b[..4] != b"\0asm" {
            return Err(fail("not a WASM module".into()));
        }
        let version = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        let result = serde_json::json!({
            "version": version,
            "size": b.len(),
        });
        Ok(Bytes::from(serde_json::to_string_pretty(&result).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #465 ElfMetadataFn ----
pub struct ElfMetadataFn;
impl ComputeFunction for ElfMetadataFn {
    fn id(&self) -> FunctionId { fid("elf_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.len() < 16 || &b[..4] != b"\x7fELF" {
            return Err(fail("not an ELF file".into()));
        }
        let class = match b[4] { 1 => "32-bit", 2 => "64-bit", _ => "unknown" };
        let endian = match b[5] { 1 => "little", 2 => "big", _ => "unknown" };
        let result = serde_json::json!({
            "class": class,
            "endianness": endian,
            "size": b.len(),
        });
        Ok(Bytes::from(serde_json::to_string_pretty(&result).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
