use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;

pub struct CompressFn;

impl ComputeFunction for CompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("compress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&inputs[0])
            .map_err(|e| ComputeError::ExecutionFailed(format!("compress: {}", e)))?;
        let compressed = encoder.finish()
            .map_err(|e| ComputeError::ExecutionFailed(format!("compress finish: {}", e)))?;
        Ok(Bytes::from(compressed))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #22 DecompressFn (zlib) ──

pub struct DecompressFn;

impl ComputeFunction for DecompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("decompress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use flate2::read::ZlibDecoder;
        use std::io::Read;
        let mut decoder = ZlibDecoder::new(&inputs[0][..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)
            .map_err(|e| ComputeError::ExecutionFailed(format!("decompress: {}", e)))?;
        Ok(Bytes::from(decompressed))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #23 ZstdCompressFn ──

pub struct ZstdCompressFn;

impl ComputeFunction for ZstdCompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("zstd_compress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let level: i32 = match params.get("level") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("level must be 1-22".into()))?,
            None => 3,
            _ => return Err(ComputeError::InvalidParam("level must be a string".into())),
        };
        if !(1..=22).contains(&level) {
            return Err(ComputeError::InvalidParam("level must be 1-22".into()));
        }
        zstd::encode_all(&inputs[0][..], level)
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("zstd compress: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #24 ZstdDecompressFn ──

pub struct ZstdDecompressFn;

impl ComputeFunction for ZstdDecompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("zstd_decompress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        zstd::decode_all(&inputs[0][..])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("zstd decompress: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #25 Lz4CompressFn ──

pub struct Lz4CompressFn;

impl ComputeFunction for Lz4CompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("lz4_compress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let compressed = lz4_flex::compress_prepend_size(&inputs[0]);
        Ok(Bytes::from(compressed))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #26 Lz4DecompressFn ──

pub struct Lz4DecompressFn;

impl ComputeFunction for Lz4DecompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("lz4_decompress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        lz4_flex::decompress_size_prepended(&inputs[0])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("lz4 decompress: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #27 SnappyCompressFn ──

pub struct SnappyCompressFn;

impl ComputeFunction for SnappyCompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("snappy_compress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mut encoder = snap::raw::Encoder::new();
        encoder.compress_vec(&inputs[0])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("snappy compress: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #28 SnappyDecompressFn ──

pub struct SnappyDecompressFn;

impl ComputeFunction for SnappyDecompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("snappy_decompress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mut decoder = snap::raw::Decoder::new();
        decoder.decompress_vec(&inputs[0])
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("snappy decompress: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #29 BrotliCompressFn ──

pub struct BrotliCompressFn;

impl ComputeFunction for BrotliCompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("brotli_compress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let quality: u32 = match params.get("quality") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("quality must be 0-11".into()))?,
            None => 6,
            _ => return Err(ComputeError::InvalidParam("quality must be a string".into())),
        };
        if quality > 11 {
            return Err(ComputeError::InvalidParam("quality must be 0-11".into()));
        }
        let mut output = Vec::new();
        let bp = brotli::enc::BrotliEncoderParams {
            quality: quality as i32,
            ..Default::default()
        };
        brotli::BrotliCompress(&mut &inputs[0][..], &mut output, &bp)
            .map_err(|e| ComputeError::ExecutionFailed(format!("brotli compress: {}", e)))?;
        Ok(Bytes::from(output))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #30 BrotliDecompressFn ──

pub struct BrotliDecompressFn;

impl ComputeFunction for BrotliDecompressFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("brotli_decompress", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mut output = Vec::new();
        brotli::BrotliDecompress(&mut &inputs[0][..], &mut output)
            .map_err(|e| ComputeError::ExecutionFailed(format!("brotli decompress: {}", e)))?;
        Ok(Bytes::from(output))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #31 Sha256Fn ──

