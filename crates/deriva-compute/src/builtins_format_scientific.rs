use bytes::Bytes;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;
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

fn arrow_ipc_serialize(batch: &arrow::record_batch::RecordBatch) -> Result<Bytes, ComputeError> {
    let mut buf = Vec::new();
    {
        let mut writer = arrow::ipc::writer::FileWriter::try_new(&mut buf, &batch.schema())
            .map_err(|e| fail(format!("arrow ipc: {e}")))?;
        writer.write(batch).map_err(|e| fail(format!("arrow write: {e}")))?;
        writer.finish().map_err(|e| fail(format!("arrow finish: {e}")))?;
    }
    Ok(Bytes::from(buf))
}

fn arrow_ipc_deserialize(data: &[u8]) -> Result<arrow::record_batch::RecordBatch, ComputeError> {
    let cursor = Cursor::new(data);
    let reader = arrow::ipc::reader::FileReader::try_new(cursor, None)
        .map_err(|e| fail(format!("arrow ipc read: {e}")))?;
    let batches: Vec<_> = reader.into_iter().collect::<Result<Vec<_>, _>>()
        .map_err(|e| fail(format!("arrow read: {e}")))?;
    if batches.is_empty() { return Err(fail("empty arrow file".into())); }
    Ok(batches.into_iter().next().unwrap())
}

// ---- #370 Hdf5MetadataFn (requires libhdf5) ----
pub struct Hdf5MetadataFn;
impl ComputeFunction for Hdf5MetadataFn {
    fn id(&self) -> FunctionId { fid("hdf5_metadata") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("hdf5_metadata requires format-scientific-hdf5 feature (libhdf5)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #371 Hdf5ReadFn (requires libhdf5) ----
pub struct Hdf5ReadFn;
impl ComputeFunction for Hdf5ReadFn {
    fn id(&self) -> FunctionId { fid("hdf5_read") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("hdf5_read requires format-scientific-hdf5 feature (libhdf5)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #372 Hdf5WriteFn (requires libhdf5) ----
pub struct Hdf5WriteFn;
impl ComputeFunction for Hdf5WriteFn {
    fn id(&self) -> FunctionId { fid("hdf5_write") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("hdf5_write requires format-scientific-hdf5 feature (libhdf5)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #373 NetcdfMetadataFn (requires libnetcdf) ----
pub struct NetcdfMetadataFn;
impl ComputeFunction for NetcdfMetadataFn {
    fn id(&self) -> FunctionId { fid("netcdf_metadata") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("netcdf_metadata requires format-scientific-netcdf feature (libnetcdf)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #374 NetcdfReadFn (requires libnetcdf) ----
pub struct NetcdfReadFn;
impl ComputeFunction for NetcdfReadFn {
    fn id(&self) -> FunctionId { fid("netcdf_read") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("netcdf_read requires format-scientific-netcdf feature (libnetcdf)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #375 FitsMetadataFn (requires cfitsio) ----
pub struct FitsMetadataFn;
impl ComputeFunction for FitsMetadataFn {
    fn id(&self) -> FunctionId { fid("fits_metadata") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("fits_metadata requires format-scientific-fits feature (cfitsio)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #376 FitsReadFn (requires cfitsio) ----
pub struct FitsReadFn;
impl ComputeFunction for FitsReadFn {
    fn id(&self) -> FunctionId { fid("fits_read") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("fits_read requires format-scientific-fits feature (cfitsio)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- .npy format helpers ----

fn parse_npy_header(header: &str) -> Result<(String, Vec<usize>), ComputeError> {
    // Header is a Python dict like: {'descr': '<f8', 'fortran_order': False, 'shape': (3, 4), }
    let descr_start = header.find("'descr'").or_else(|| header.find("\"descr\""))
        .ok_or_else(|| fail("npy: missing descr".into()))?;
    let rest = &header[descr_start..];
    let q = if rest.contains("'") { '\'' } else { '"' };
    // Find the dtype value between quotes after the colon
    let colon = rest.find(':').ok_or_else(|| fail("npy: bad header".into()))?;
    let after_colon = &rest[colon + 1..];
    let start = after_colon.find(q).ok_or_else(|| fail("npy: bad descr".into()))? + 1;
    let end = after_colon[start..].find(q).ok_or_else(|| fail("npy: bad descr".into()))?;
    let dtype = after_colon[start..start + end].to_string();

    // Parse shape
    let shape_start = header.find("'shape'").or_else(|| header.find("\"shape\""))
        .ok_or_else(|| fail("npy: missing shape".into()))?;
    let rest = &header[shape_start..];
    let paren_start = rest.find('(').ok_or_else(|| fail("npy: bad shape".into()))? + 1;
    let paren_end = rest.find(')').ok_or_else(|| fail("npy: bad shape".into()))?;
    let shape_str = &rest[paren_start..paren_end];
    let shape: Vec<usize> = shape_str.split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    Ok((dtype, shape))
}

fn npy_dtype_size(dtype: &str) -> Result<usize, ComputeError> {
    // Strip endian prefix
    let d = dtype.trim_start_matches(|c| c == '<' || c == '>' || c == '=' || c == '|');
    match d {
        "f4" => Ok(4), "f8" => Ok(8),
        "i4" => Ok(4), "i8" => Ok(8),
        "i2" => Ok(2), "i1" => Ok(1),
        "u4" => Ok(4), "u8" => Ok(8),
        "u2" => Ok(2), "u1" => Ok(1),
        "b1" => Ok(1),
        _ => Err(fail(format!("unsupported npy dtype: {dtype}"))),
    }
}

fn make_npy(dtype: &str, shape: &[usize], data: &[u8]) -> Vec<u8> {
    let header = format!("{{'descr': '{dtype}', 'fortran_order': False, 'shape': ({},), }}", 
        shape.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", "));
    // Pad header to multiple of 64
    let prefix_len = 10; // magic(6) + version(2) + header_len(2)
    let total = prefix_len + header.len() + 1; // +1 for newline
    let padding = (64 - (total % 64)) % 64;
    let header_len = (header.len() + 1 + padding) as u16;

    let mut buf = Vec::new();
    buf.extend_from_slice(b"\x93NUMPY");
    buf.push(1); buf.push(0); // version 1.0
    buf.extend_from_slice(&header_len.to_le_bytes());
    buf.extend_from_slice(header.as_bytes());
    buf.extend(std::iter::repeat(b' ').take(padding));
    buf.push(b'\n');
    buf.extend_from_slice(data);
    buf
}

// ---- #377 NumpyReadFn ----
pub struct NumpyReadFn;
impl ComputeFunction for NumpyReadFn {
    fn id(&self) -> FunctionId { fid("numpy_read") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.len() < 10 || &b[..6] != b"\x93NUMPY" {
            return Err(fail("not a valid .npy file".into()));
        }
        let header_len = u16::from_le_bytes([b[8], b[9]]) as usize;
        if 10 + header_len > b.len() { return Err(fail("truncated npy header".into())); }
        let header_str = std::str::from_utf8(&b[10..10 + header_len])
            .map_err(|_| fail("invalid npy header".into()))?;
        let (dtype, shape) = parse_npy_header(header_str)?;
        let data = &b[10 + header_len..];
        let batch = npy_data_to_arrow(data, &dtype, &shape)?;
        arrow_ipc_serialize(&batch)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn npy_data_to_arrow(data: &[u8], dtype: &str, shape: &[usize]) -> Result<arrow::record_batch::RecordBatch, ComputeError> {
    use arrow::array::*;
    use arrow::datatypes::*;
    let total_elements: usize = shape.iter().product();
    let d = dtype.trim_start_matches(|c| c == '<' || c == '>' || c == '=' || c == '|');
    let elem_size = npy_dtype_size(dtype)?;
    if data.len() < total_elements * elem_size {
        return Err(fail("npy data too short".into()));
    }
    let (field, array): (Field, Arc<dyn Array>) = match d {
        "f4" => {
            let vals: Vec<f32> = data.chunks_exact(4).take(total_elements)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            (Field::new("data", DataType::Float32, false), Arc::new(Float32Array::from(vals)))
        }
        "f8" => {
            let vals: Vec<f64> = data.chunks_exact(8).take(total_elements)
                .map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect();
            (Field::new("data", DataType::Float64, false), Arc::new(Float64Array::from(vals)))
        }
        "i4" => {
            let vals: Vec<i32> = data.chunks_exact(4).take(total_elements)
                .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            (Field::new("data", DataType::Int32, false), Arc::new(Int32Array::from(vals)))
        }
        "i8" => {
            let vals: Vec<i64> = data.chunks_exact(8).take(total_elements)
                .map(|c| i64::from_le_bytes(c.try_into().unwrap())).collect();
            (Field::new("data", DataType::Int64, false), Arc::new(Int64Array::from(vals)))
        }
        _ => return Err(fail(format!("unsupported dtype for arrow: {dtype}"))),
    };
    let schema = Arc::new(Schema::new(vec![field]));
    arrow::record_batch::RecordBatch::try_new(schema, vec![array])
        .map_err(|e| fail(format!("arrow batch: {e}")))
}

// ---- #378 NumpyWriteFn ----
pub struct NumpyWriteFn;
impl ComputeFunction for NumpyWriteFn {
    fn id(&self) -> FunctionId { fid("numpy_write") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let dtype = param_str(p, "dtype").unwrap_or_else(|| "<f8".into());
        let shape_str = param_str(p, "shape").ok_or_else(|| ComputeError::InvalidParam("shape required".into()))?;
        let shape: Vec<usize> = shape_str.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if shape.is_empty() { return Err(ComputeError::InvalidParam("invalid shape".into())); }
        // Input is raw bytes
        let npy = make_npy(&dtype, &shape, b);
        Ok(Bytes::from(npy))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #379 ZarrReadFn ----
pub struct ZarrReadFn;
impl ComputeFunction for ZarrReadFn {
    fn id(&self) -> FunctionId { fid("zarr_read") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let array_path = param_str(p, "array").unwrap_or_default();
        let cursor = Cursor::new(b.as_ref());
        let mut zip = zip::ZipArchive::new(cursor).map_err(|e| fail(format!("zip: {e}")))?;
        // Read .zarray metadata
        let zarray_path = if array_path.is_empty() { ".zarray".to_string() }
            else { format!("{}/.zarray", array_path.trim_matches('/')) };
        let zarray_json = read_zip_text(&mut zip, &zarray_path)?;
        let meta: serde_json::Value = serde_json::from_str(&zarray_json)
            .map_err(|e| fail(format!("zarray json: {e}")))?;
        let dtype = meta["dtype"].as_str().unwrap_or("<f8").to_string();
        let shape: Vec<usize> = meta["shape"].as_array()
            .ok_or_else(|| fail("missing shape".into()))?
            .iter().filter_map(|v| v.as_u64().map(|n| n as usize)).collect();
        let chunks: Vec<usize> = meta["chunks"].as_array()
            .ok_or_else(|| fail("missing chunks".into()))?
            .iter().filter_map(|v| v.as_u64().map(|n| n as usize)).collect();
        let total_elements: usize = shape.iter().product();
        let elem_size = npy_dtype_size(&dtype)?;
        // Read chunk files and assemble data (1D case for simplicity)
        let mut raw_data = vec![0u8; total_elements * elem_size];
        let chunk_elements: usize = chunks.iter().product();
        let num_chunks = (total_elements + chunk_elements - 1) / chunk_elements;
        let prefix = if array_path.is_empty() { String::new() }
            else { format!("{}/", array_path.trim_matches('/')) };
        for i in 0..num_chunks {
            let chunk_path = format!("{}{}", prefix, i);
            if let Ok(chunk_data) = read_zip_bytes(&mut zip, &chunk_path) {
                let offset = i * chunk_elements * elem_size;
                let copy_len = chunk_data.len().min(raw_data.len() - offset);
                raw_data[offset..offset + copy_len].copy_from_slice(&chunk_data[..copy_len]);
            }
        }
        let batch = npy_data_to_arrow(&raw_data, &dtype, &shape)?;
        arrow_ipc_serialize(&batch)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn read_zip_text(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String, ComputeError> {
    let mut f = zip.by_name(name).map_err(|e| fail(format!("zip entry '{name}': {e}")))?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut f, &mut s).map_err(|e| fail(format!("read: {e}")))?;
    Ok(s)
}

fn read_zip_bytes(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, ComputeError> {
    let mut f = zip.by_name(name).map_err(|e| fail(format!("zip entry '{name}': {e}")))?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut buf).map_err(|e| fail(format!("read: {e}")))?;
    Ok(buf)
}

// ---- #380 ZarrMetadataFn ----
pub struct ZarrMetadataFn;
impl ComputeFunction for ZarrMetadataFn {
    fn id(&self) -> FunctionId { fid("zarr_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let cursor = Cursor::new(b.as_ref());
        let mut zip = zip::ZipArchive::new(cursor).map_err(|e| fail(format!("zip: {e}")))?;
        let mut arrays = Vec::new();
        // Scan for .zarray files
        let names: Vec<String> = (0..zip.len()).filter_map(|i| {
            zip.by_index(i).ok().map(|f| f.name().to_string())
        }).collect();
        for name in &names {
            if name.ends_with(".zarray") {
                let mut f = zip.by_name(name).unwrap();
                let mut s = String::new();
                std::io::Read::read_to_string(&mut f, &mut s).ok();
                if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&s) {
                    let path = name.trim_end_matches("/.zarray").trim_end_matches(".zarray");
                    arrays.push(serde_json::json!({
                        "path": if path.is_empty() { "/" } else { path },
                        "shape": meta["shape"],
                        "chunks": meta["chunks"],
                        "dtype": meta["dtype"],
                        "compressor": meta["compressor"],
                    }));
                }
            }
        }
        let result = serde_json::json!({ "arrays": arrays });
        Ok(Bytes::from(serde_json::to_string_pretty(&result).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #381 NumpyToArrowFn ----
pub struct NumpyToArrowFn;
impl ComputeFunction for NumpyToArrowFn {
    fn id(&self) -> FunctionId { fid("numpy_to_arrow") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        // Same as NumpyReadFn — parse .npy, emit Arrow IPC
        NumpyReadFn.execute(inputs, &BTreeMap::new())
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #382 ArrowToNumpyFn ----
pub struct ArrowToNumpyFn;
impl ComputeFunction for ArrowToNumpyFn {
    fn id(&self) -> FunctionId { fid("arrow_to_numpy") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let dtype = param_str(p, "dtype").unwrap_or_else(|| "<f8".into());
        let batch = arrow_ipc_deserialize(b)?;
        if batch.num_columns() == 0 { return Err(fail("no columns".into())); }
        let col = batch.column(0);
        let num_rows = col.len();
        use arrow::array::*;
        use arrow::datatypes::DataType;
        let d = dtype.trim_start_matches(|c| c == '<' || c == '>' || c == '=' || c == '|');
        let raw: Vec<u8> = match d {
            "f4" => {
                let arr = arrow::compute::cast(col, &DataType::Float32).map_err(|e| fail(format!("cast: {e}")))?;
                let a = arr.as_any().downcast_ref::<Float32Array>().unwrap();
                a.values().iter().flat_map(|v| v.to_le_bytes()).collect()
            }
            "f8" => {
                let arr = arrow::compute::cast(col, &DataType::Float64).map_err(|e| fail(format!("cast: {e}")))?;
                let a = arr.as_any().downcast_ref::<Float64Array>().unwrap();
                a.values().iter().flat_map(|v| v.to_le_bytes()).collect()
            }
            "i4" => {
                let arr = arrow::compute::cast(col, &DataType::Int32).map_err(|e| fail(format!("cast: {e}")))?;
                let a = arr.as_any().downcast_ref::<Int32Array>().unwrap();
                a.values().iter().flat_map(|v| v.to_le_bytes()).collect()
            }
            "i8" => {
                let arr = arrow::compute::cast(col, &DataType::Int64).map_err(|e| fail(format!("cast: {e}")))?;
                let a = arr.as_any().downcast_ref::<Int64Array>().unwrap();
                a.values().iter().flat_map(|v| v.to_le_bytes()).collect()
            }
            _ => return Err(fail(format!("unsupported dtype: {dtype}"))),
        };
        let npy = make_npy(&dtype, &[num_rows], &raw);
        Ok(Bytes::from(npy))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
