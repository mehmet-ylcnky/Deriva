//! Category D: Archive & Container Formats (20 functions, #263–#282)

use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write, Cursor};

fn fid(name: &str) -> FunctionId {
    FunctionId { name: name.to_string(), version: "1.0.0".to_string() }
}
fn param_str(p: &BTreeMap<String, Value>, k: &str) -> Option<String> {
    match p.get(k) { Some(Value::String(s)) => Some(s.clone()), _ => None }
}
fn one(inputs: &[Bytes]) -> Result<&Bytes, ComputeError> {
    if inputs.is_empty() { return Err(ComputeError::InputCount { expected: 1, got: 0 }); }
    Ok(&inputs[0])
}
fn cost(sizes: &[u64]) -> ComputeCost {
    ComputeCost { cpu_ms: sizes.iter().sum::<u64>() / 50 + 100, memory_bytes: sizes.iter().sum::<u64>() * 3 + 4096 }
}
fn parse_level(p: &BTreeMap<String, Value>, min: u32, max: u32, def: u32) -> Result<u32, ComputeError> {
    match param_str(p, "level") {
        Some(s) => {
            let l: u32 = s.parse().map_err(|_| ComputeError::InvalidParam("level must be integer".into()))?;
            if l < min || l > max { return Err(ComputeError::InvalidParam(format!("level must be {min}..{max}"))); }
            Ok(l)
        }
        None => Ok(def),
    }
}

// ---- #263 TarCreateFn ----
pub struct TarCreateFn;
impl ComputeFunction for TarCreateFn {
    fn id(&self) -> FunctionId { fid("tar_create") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let files: Vec<String> = match p.get("files") {
            Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_else(|_| vec![s.clone()]),
            Some(Value::List(l)) => l.iter().filter_map(|v| if let Value::String(s) = v { Some(s.clone()) } else { None }).collect(),
            _ => (0..inputs.len()).map(|i| format!("file_{i}")).collect(),
        };
        let buf = Vec::new();
        let mut ar = tar::Builder::new(buf);
        for (i, input) in inputs.iter().enumerate() {
            let name = files.get(i).map(|s| s.as_str()).unwrap_or("file");
            let mut header = tar::Header::new_gnu();
            header.set_size(input.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            ar.append_data(&mut header, name, &input[..])
                .map_err(|e| ComputeError::ExecutionFailed(format!("tar: {e}")))?;
        }
        let out = ar.into_inner().map_err(|e| ComputeError::ExecutionFailed(format!("tar finish: {e}")))?;
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #264 TarExtractFn ----
pub struct TarExtractFn;
impl ComputeFunction for TarExtractFn {
    fn id(&self) -> FunctionId { fid("tar_extract") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let filter_path = param_str(p, "path");
        let mut ar = tar::Archive::new(Cursor::new(b.as_ref()));
        let mut manifest = serde_json::Map::new();
        for entry in ar.entries().map_err(|e| ComputeError::ExecutionFailed(format!("tar: {e}")))? {
            let mut entry = entry.map_err(|e| ComputeError::ExecutionFailed(format!("tar entry: {e}")))?;
            let path = entry.path().map_err(|e| ComputeError::ExecutionFailed(format!("path: {e}")))?.to_string_lossy().to_string();
            if let Some(ref fp) = filter_path {
                if &path != fp { continue; }
            }
            let size = entry.size();
            let mut data = Vec::with_capacity(size as usize);
            entry.read_to_end(&mut data).map_err(|e| ComputeError::ExecutionFailed(format!("read: {e}")))?;
            if filter_path.is_some() { return Ok(Bytes::from(data)); }
            manifest.insert(path, serde_json::json!({"size": size, "content_b64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data)}));
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&manifest).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #265 TarListFn ----
pub struct TarListFn;
impl ComputeFunction for TarListFn {
    fn id(&self) -> FunctionId { fid("tar_list") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut ar = tar::Archive::new(Cursor::new(b.as_ref()));
        let mut entries = Vec::new();
        for entry in ar.entries().map_err(|e| ComputeError::ExecutionFailed(format!("tar: {e}")))? {
            let entry = entry.map_err(|e| ComputeError::ExecutionFailed(format!("tar entry: {e}")))?;
            let path = entry.path().map_err(|e| ComputeError::ExecutionFailed(format!("path: {e}")))?.to_string_lossy().to_string();
            entries.push(serde_json::json!({"path": path, "size": entry.size(), "mode": entry.header().mode().unwrap_or(0)}));
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&entries).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #266 TarAppendFn ----
pub struct TarAppendFn;
impl ComputeFunction for TarAppendFn {
    fn id(&self) -> FunctionId { fid("tar_append") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let filename = param_str(p, "filename").unwrap_or_else(|| "appended".to_string());
        // Strip EOF blocks (two 512-byte zero blocks at end)
        let mut base = inputs[0].to_vec();
        while base.len() >= 512 && base[base.len()-512..].iter().all(|&b| b == 0) {
            base.truncate(base.len() - 512);
        }
        let mut ar = tar::Builder::new(base);
        let data = &inputs[1];
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        ar.append_data(&mut header, &filename, &data[..])
            .map_err(|e| ComputeError::ExecutionFailed(format!("tar append: {e}")))?;
        let out = ar.into_inner().map_err(|e| ComputeError::ExecutionFailed(format!("tar finish: {e}")))?;
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #267 GzipCompressFn ----
pub struct GzipCompressFn;
impl ComputeFunction for GzipCompressFn {
    fn id(&self) -> FunctionId { fid("gzip_compress") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let level = parse_level(p, 1, 9, 6)?;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(level));
        enc.write_all(b).map_err(|e| ComputeError::ExecutionFailed(format!("gzip: {e}")))?;
        Ok(Bytes::from(enc.finish().map_err(|e| ComputeError::ExecutionFailed(format!("gzip finish: {e}")))?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #268 GzipDecompressFn ----
pub struct GzipDecompressFn;
impl ComputeFunction for GzipDecompressFn {
    fn id(&self) -> FunctionId { fid("gzip_decompress") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut dec = flate2::read::GzDecoder::new(&b[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).map_err(|e| ComputeError::ExecutionFailed(format!("gzip decompress: {e}")))?;
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #269 Bzip2CompressFn ----
pub struct Bzip2CompressFn;
impl ComputeFunction for Bzip2CompressFn {
    fn id(&self) -> FunctionId { fid("bzip2_compress") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let level = parse_level(p, 1, 9, 6)?;
        let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::new(level));
        enc.write_all(b).map_err(|e| ComputeError::ExecutionFailed(format!("bzip2: {e}")))?;
        Ok(Bytes::from(enc.finish().map_err(|e| ComputeError::ExecutionFailed(format!("bzip2 finish: {e}")))?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #270 Bzip2DecompressFn ----
pub struct Bzip2DecompressFn;
impl ComputeFunction for Bzip2DecompressFn {
    fn id(&self) -> FunctionId { fid("bzip2_decompress") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut dec = bzip2::read::BzDecoder::new(&b[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).map_err(|e| ComputeError::ExecutionFailed(format!("bzip2 decompress: {e}")))?;
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #271 XzCompressFn ----
pub struct XzCompressFn;
impl ComputeFunction for XzCompressFn {
    fn id(&self) -> FunctionId { fid("xz_compress") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let level = parse_level(p, 0, 9, 6)?;
        let mut enc = xz2::write::XzEncoder::new(Vec::new(), level);
        enc.write_all(b).map_err(|e| ComputeError::ExecutionFailed(format!("xz: {e}")))?;
        Ok(Bytes::from(enc.finish().map_err(|e| ComputeError::ExecutionFailed(format!("xz finish: {e}")))?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #272 XzDecompressFn ----
pub struct XzDecompressFn;
impl ComputeFunction for XzDecompressFn {
    fn id(&self) -> FunctionId { fid("xz_decompress") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut dec = xz2::read::XzDecoder::new(&b[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).map_err(|e| ComputeError::ExecutionFailed(format!("xz decompress: {e}")))?;
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #273 ZipCreateFn ----
pub struct ZipCreateFn;
impl ComputeFunction for ZipCreateFn {
    fn id(&self) -> FunctionId { fid("zip_create") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let method = match param_str(p, "compression").as_deref() {
            Some("stored") => zip::CompressionMethod::Stored,
            Some("zstd") => zip::CompressionMethod::Stored, // zstd requires feature; fallback to stored
            _ => zip::CompressionMethod::Deflated,
        };
        let files: Vec<String> = match p.get("files") {
            Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_else(|_| vec![s.clone()]),
            Some(Value::List(l)) => l.iter().filter_map(|v| if let Value::String(s) = v { Some(s.clone()) } else { None }).collect(),
            _ => (0..inputs.len()).map(|i| format!("file_{i}")).collect(),
        };
        let buf = Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(method);
        for (i, input) in inputs.iter().enumerate() {
            let name = files.get(i).map(|s| s.as_str()).unwrap_or("file");
            zw.start_file(name, opts).map_err(|e| ComputeError::ExecutionFailed(format!("zip: {e}")))?;
            zw.write_all(input).map_err(|e| ComputeError::ExecutionFailed(format!("zip write: {e}")))?;
        }
        let cur = zw.finish().map_err(|e| ComputeError::ExecutionFailed(format!("zip finish: {e}")))?;
        Ok(Bytes::from(cur.into_inner()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #274 ZipExtractFn ----
pub struct ZipExtractFn;
impl ComputeFunction for ZipExtractFn {
    fn id(&self) -> FunctionId { fid("zip_extract") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let filter_path = param_str(p, "path");
        let mut archive = zip::ZipArchive::new(Cursor::new(b.as_ref()))
            .map_err(|e| ComputeError::ExecutionFailed(format!("zip: {e}")))?;
        let mut manifest = serde_json::Map::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| ComputeError::ExecutionFailed(format!("zip entry: {e}")))?;
            let name = file.name().to_string();
            if let Some(ref fp) = filter_path {
                if &name != fp { continue; }
            }
            let mut data = Vec::new();
            file.read_to_end(&mut data).map_err(|e| ComputeError::ExecutionFailed(format!("read: {e}")))?;
            if filter_path.is_some() { return Ok(Bytes::from(data)); }
            manifest.insert(name, serde_json::json!({"size": data.len(), "content_b64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data)}));
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&manifest).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #275 ZipListFn ----
pub struct ZipListFn;
impl ComputeFunction for ZipListFn {
    fn id(&self) -> FunctionId { fid("zip_list") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(b.as_ref()))
            .map_err(|e| ComputeError::ExecutionFailed(format!("zip: {e}")))?;
        let mut entries = Vec::new();
        for i in 0..archive.len() {
            let file = archive.by_index_raw(i).map_err(|e| ComputeError::ExecutionFailed(format!("zip entry: {e}")))?;
            entries.push(serde_json::json!({"path": file.name().to_string(), "compressed_size": file.compressed_size(), "size": file.size()}));
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&entries).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #276 ZipExtractSingleFn ----
pub struct ZipExtractSingleFn;
impl ComputeFunction for ZipExtractSingleFn {
    fn id(&self) -> FunctionId { fid("zip_extract_single") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let path = param_str(p, "path").ok_or_else(|| ComputeError::InvalidParam("missing 'path'".into()))?;
        let mut archive = zip::ZipArchive::new(Cursor::new(b.as_ref()))
            .map_err(|e| ComputeError::ExecutionFailed(format!("zip: {e}")))?;
        let mut file = archive.by_name(&path)
            .map_err(|e| ComputeError::ExecutionFailed(format!("entry '{path}': {e}")))?;
        let mut buf = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut buf).map_err(|e| ComputeError::ExecutionFailed(format!("read: {e}")))?;
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #277 TarGzCreateFn ----
pub struct TarGzCreateFn;
impl ComputeFunction for TarGzCreateFn {
    fn id(&self) -> FunctionId { fid("tar_gz_create") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let level = parse_level(p, 1, 9, 6)?;
        // First create tar
        let tar_bytes = TarCreateFn.execute(inputs, p)?;
        // Then gzip
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(level));
        enc.write_all(&tar_bytes).map_err(|e| ComputeError::ExecutionFailed(format!("gzip: {e}")))?;
        Ok(Bytes::from(enc.finish().map_err(|e| ComputeError::ExecutionFailed(format!("gzip finish: {e}")))?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #278 TarGzExtractFn ----
pub struct TarGzExtractFn;
impl ComputeFunction for TarGzExtractFn {
    fn id(&self) -> FunctionId { fid("tar_gz_extract") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        // Decompress gzip first
        let mut dec = flate2::read::GzDecoder::new(&b[..]);
        let mut tar_data = Vec::new();
        dec.read_to_end(&mut tar_data).map_err(|e| ComputeError::ExecutionFailed(format!("gzip decompress: {e}")))?;
        // Then extract tar
        TarExtractFn.execute(vec![Bytes::from(tar_data)], p)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #279 ZstdFrameCompressFn ----
pub struct ZstdFrameCompressFn;
impl ComputeFunction for ZstdFrameCompressFn {
    fn id(&self) -> FunctionId { fid("zstd_frame_compress") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let level = parse_level(p, 1, 22, 3)? as i32;
        let out = zstd::encode_all(&b[..], level)
            .map_err(|e| ComputeError::ExecutionFailed(format!("zstd: {e}")))?;
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #280 ZstdFrameDecompressFn ----
pub struct ZstdFrameDecompressFn;
impl ComputeFunction for ZstdFrameDecompressFn {
    fn id(&self) -> FunctionId { fid("zstd_frame_decompress") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let out = zstd::decode_all(&b[..])
            .map_err(|e| ComputeError::ExecutionFailed(format!("zstd decompress: {e}")))?;
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #281 SevenZExtractFn ----
pub struct SevenZExtractFn;
impl ComputeFunction for SevenZExtractFn {
    fn id(&self) -> FunctionId { fid("sevenz_extract") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut manifest = serde_json::Map::new();
        let tmp = std::env::temp_dir().join(format!("sevenz_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).map_err(|e| ComputeError::ExecutionFailed(format!("tmpdir: {e}")))?;
        let cursor = Cursor::new(b.as_ref());
        sevenz_rust::decompress(cursor, &tmp)
            .map_err(|e| { let _ = std::fs::remove_dir_all(&tmp); ComputeError::ExecutionFailed(format!("7z: {e}")) })?;
        fn walk(dir: &std::path::Path, base: &std::path::Path, manifest: &mut serde_json::Map<String, serde_json::Value>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() { walk(&path, base, manifest); }
                    else if let Ok(data) = std::fs::read(&path) {
                        let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().to_string();
                        manifest.insert(rel, serde_json::json!({
                            "size": data.len(),
                            "content_b64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data)
                        }));
                    }
                }
            }
        }
        walk(&tmp, &tmp, &mut manifest);
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(Bytes::from(serde_json::to_string_pretty(&manifest).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #282 RarExtractFn (stub — requires native unrar lib) ----
pub struct RarExtractFn;
impl ComputeFunction for RarExtractFn {
    fn id(&self) -> FunctionId { fid("rar_extract") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        // Check RAR magic bytes
        if b.len() < 7 || &b[..7] != b"Rar!\x1a\x07\x00" && &b[..7] != b"Rar!\x1a\x07\x01" {
            return Err(ComputeError::ExecutionFailed("not a valid RAR archive".into()));
        }
        Err(ComputeError::ExecutionFailed("RAR extraction requires native unrar library (not available)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
