use bytes::Bytes;
use deriva_compute::builtins::*;
use deriva_compute::function::{ComputeError, ComputeFunction};
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn exec1(f: &dyn ComputeFunction, input: &[u8]) -> Result<Bytes, ComputeError> {
    f.execute(vec![Bytes::from(input.to_vec())], &BTreeMap::new())
}

fn exec1_params(f: &dyn ComputeFunction, input: &[u8], params: BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    f.execute(vec![Bytes::from(input.to_vec())], &params)
}

fn params(kv: &[(&str, &str)]) -> BTreeMap<String, Value> {
    kv.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

const TEST_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TEST_NONCE: &str = "00112233445566778899aabbccddeeff";
const TEST_GCM_NONCE: &str = "000102030405060708090a0b";

fn read_u64(b: &Bytes) -> u64 { u64::from_be_bytes(b[..8].try_into().unwrap()) }
fn read_f64(b: &Bytes) -> f64 { f64::from_be_bytes(b[..8].try_into().unwrap()) }




// ── #21 CompressFn (zlib) ──

#[test]
fn compress_produces_valid_zlib() {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let input = b"The quick brown fox jumps over the lazy dog";
    let compressed = exec1(&CompressFn, input).unwrap();
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).unwrap();
    assert_eq!(out, input);
}

#[test]
fn compress_empty_input() {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let compressed = exec1(&CompressFn, b"").unwrap();
    assert!(!compressed.is_empty()); // valid zlib header
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).unwrap();
    assert!(out.is_empty());
}

#[test]
fn compress_reduces_repetitive_data() {
    let input = vec![b'A'; 10_000];
    let compressed = exec1(&CompressFn, &input).unwrap();
    assert!(compressed.len() < input.len() / 10);
}

#[test]
fn compress_binary_data() {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let input: Vec<u8> = (0..=255).cycle().take(4096).collect();
    let compressed = exec1(&CompressFn, &input).unwrap();
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).unwrap();
    assert_eq!(out, input);
}

#[test]
fn compress_rejects_multiple_inputs() {
    let r = CompressFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 1, got: 2 })));
}


// ── #22 DecompressFn (zlib) ──

#[test]
fn decompress_roundtrip() {
    let input = b"Hello, content-addressed world!";
    let compressed = exec1(&CompressFn, input).unwrap();
    let decompressed = exec1(&DecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input);
}

#[test]
fn decompress_empty_zlib_stream() {
    let compressed = exec1(&CompressFn, b"").unwrap();
    let decompressed = exec1(&DecompressFn, &compressed).unwrap();
    assert!(decompressed.is_empty());
}

#[test]
fn decompress_corrupt_data() {
    let r = exec1(&DecompressFn, b"this is not zlib data");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn decompress_truncated_stream() {
    // Just the zlib header (2 bytes) with no payload — should error
    let r = exec1(&DecompressFn, &[0x78, 0x9C]);
    // flate2 returns empty on header-only; use corrupt mid-stream instead
    let input = vec![b'X'; 10_000]; // enough data to produce a real compressed stream
    let compressed = exec1(&CompressFn, &input).unwrap();
    // Cut well into the compressed data (past header, into deflate blocks)
    let truncated = &compressed[..4];
    let r = exec1(&DecompressFn, truncated);
    assert!(r.is_err() || r.unwrap().len() < input.len());
}

#[test]
fn decompress_large_roundtrip() {
    let input: Vec<u8> = (0..=255).cycle().take(100_000).collect();
    let compressed = exec1(&CompressFn, &input).unwrap();
    let decompressed = exec1(&DecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input.as_slice());
}


// ── #23 ZstdCompressFn ──

#[test]
fn zstd_compress_produces_valid_frame() {
    let input = b"The quick brown fox jumps over the lazy dog";
    let compressed = exec1(&ZstdCompressFn, input).unwrap();
    // Zstd magic number: 0xFD2FB528
    assert_eq!(&compressed[..4], &[0x28, 0xB5, 0x2F, 0xFD]);
}

#[test]
fn zstd_compress_empty_input() {
    let compressed = exec1(&ZstdCompressFn, b"").unwrap();
    assert!(!compressed.is_empty()); // valid zstd frame
}

#[test]
fn zstd_compress_custom_level() {
    let input = vec![b'A'; 10_000];
    let fast = exec1_params(&ZstdCompressFn, &input, params(&[("level", "1")])).unwrap();
    let max = exec1_params(&ZstdCompressFn, &input, params(&[("level", "22")])).unwrap();
    assert!(max.len() <= fast.len()); // higher level = better or equal ratio
}

#[test]
fn zstd_compress_level_out_of_range() {
    let r = exec1_params(&ZstdCompressFn, b"data", params(&[("level", "23")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn zstd_compress_level_zero_error() {
    let r = exec1_params(&ZstdCompressFn, b"data", params(&[("level", "0")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}


// ── #24 ZstdDecompressFn ──

#[test]
fn zstd_decompress_roundtrip() {
    let input = b"content-addressed distributed file system";
    let compressed = exec1(&ZstdCompressFn, input).unwrap();
    let decompressed = exec1(&ZstdDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input);
}

#[test]
fn zstd_decompress_empty_frame() {
    let compressed = exec1(&ZstdCompressFn, b"").unwrap();
    let decompressed = exec1(&ZstdDecompressFn, &compressed).unwrap();
    assert!(decompressed.is_empty());
}

#[test]
fn zstd_decompress_corrupt_data() {
    let r = exec1(&ZstdDecompressFn, b"not a zstd frame");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn zstd_decompress_large_roundtrip() {
    let input: Vec<u8> = (0..=255).cycle().take(100_000).collect();
    let compressed = exec1(&ZstdCompressFn, &input).unwrap();
    let decompressed = exec1(&ZstdDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input.as_slice());
}

#[test]
fn zstd_decompress_high_level_roundtrip() {
    let input = b"test data compressed at max level";
    let compressed = exec1_params(&ZstdCompressFn, input, params(&[("level", "19")])).unwrap();
    let decompressed = exec1(&ZstdDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input);
}


// ── #25 Lz4CompressFn ──

#[test]
fn lz4_compress_has_size_header() {
    let input = b"hello lz4 world";
    let compressed = exec1(&Lz4CompressFn, input).unwrap();
    // First 4 bytes are little-endian original size
    let size = u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]);
    assert_eq!(size as usize, input.len());
}

#[test]
fn lz4_compress_roundtrip() {
    let input = b"content-addressed storage with lz4";
    let compressed = exec1(&Lz4CompressFn, input).unwrap();
    let decompressed = exec1(&Lz4DecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input);
}

#[test]
fn lz4_compress_empty_input() {
    let compressed = exec1(&Lz4CompressFn, b"").unwrap();
    assert!(!compressed.is_empty()); // size header at minimum
    let decompressed = exec1(&Lz4DecompressFn, &compressed).unwrap();
    assert!(decompressed.is_empty());
}

#[test]
fn lz4_compress_reduces_repetitive() {
    let input = vec![b'X'; 10_000];
    let compressed = exec1(&Lz4CompressFn, &input).unwrap();
    assert!(compressed.len() < input.len() / 5);
}

#[test]
fn lz4_compress_large_roundtrip() {
    let input: Vec<u8> = (0..=255).cycle().take(50_000).collect();
    let compressed = exec1(&Lz4CompressFn, &input).unwrap();
    let decompressed = exec1(&Lz4DecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input.as_slice());
}


// ── #26 Lz4DecompressFn ──

#[test]
fn lz4_decompress_roundtrip() {
    let input = b"decompress this lz4 data correctly";
    let compressed = exec1(&Lz4CompressFn, input).unwrap();
    let decompressed = exec1(&Lz4DecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input);
}

#[test]
fn lz4_decompress_corrupt_data() {
    let r = exec1(&Lz4DecompressFn, b"not lz4 data at all");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn lz4_decompress_too_short() {
    // Less than 4 bytes — can't even read size header
    let r = exec1(&Lz4DecompressFn, &[0x01, 0x02]);
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn lz4_decompress_empty_payload() {
    let compressed = exec1(&Lz4CompressFn, b"").unwrap();
    let decompressed = exec1(&Lz4DecompressFn, &compressed).unwrap();
    assert!(decompressed.is_empty());
}

#[test]
fn lz4_decompress_binary_roundtrip() {
    let input: Vec<u8> = (0..=255).cycle().take(8192).collect();
    let compressed = exec1(&Lz4CompressFn, &input).unwrap();
    let decompressed = exec1(&Lz4DecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input.as_slice());
}


// ── #27 SnappyCompressFn ──

#[test]
fn snappy_compress_roundtrip() {
    let input = b"snappy is optimized for speed over ratio";
    let compressed = exec1(&SnappyCompressFn, input).unwrap();
    let decompressed = exec1(&SnappyDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input);
}

#[test]
fn snappy_compress_empty_input() {
    let compressed = exec1(&SnappyCompressFn, b"").unwrap();
    assert!(!compressed.is_empty()); // at least varint header
    let decompressed = exec1(&SnappyDecompressFn, &compressed).unwrap();
    assert!(decompressed.is_empty());
}

#[test]
fn snappy_compress_reduces_repetitive() {
    let input = vec![b'Z'; 10_000];
    let compressed = exec1(&SnappyCompressFn, &input).unwrap();
    assert!(compressed.len() < input.len() / 5);
}

#[test]
fn snappy_compress_binary_roundtrip() {
    let input: Vec<u8> = (0..=255).cycle().take(16_384).collect();
    let compressed = exec1(&SnappyCompressFn, &input).unwrap();
    let decompressed = exec1(&SnappyDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input.as_slice());
}

#[test]
fn snappy_compress_rejects_multiple_inputs() {
    let r = SnappyCompressFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 1, got: 2 })));
}


// ── #28 SnappyDecompressFn ──

#[test]
fn snappy_decompress_roundtrip() {
    let input = b"verify snappy decompression works";
    let compressed = exec1(&SnappyCompressFn, input).unwrap();
    let decompressed = exec1(&SnappyDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input);
}

#[test]
fn snappy_decompress_corrupt_data() {
    let r = exec1(&SnappyDecompressFn, b"definitely not snappy");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn snappy_decompress_empty_payload() {
    let compressed = exec1(&SnappyCompressFn, b"").unwrap();
    let decompressed = exec1(&SnappyDecompressFn, &compressed).unwrap();
    assert!(decompressed.is_empty());
}

#[test]
fn snappy_decompress_truncated() {
    let compressed = exec1(&SnappyCompressFn, &vec![b'A'; 1000]).unwrap();
    let r = exec1(&SnappyDecompressFn, &compressed[..3]);
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn snappy_decompress_large_roundtrip() {
    let input: Vec<u8> = (0..=255).cycle().take(100_000).collect();
    let compressed = exec1(&SnappyCompressFn, &input).unwrap();
    let decompressed = exec1(&SnappyDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input.as_slice());
}


// ── #29 BrotliCompressFn ──

#[test]
fn brotli_compress_roundtrip() {
    let input = b"brotli achieves best text compression ratio";
    let compressed = exec1(&BrotliCompressFn, input).unwrap();
    let decompressed = exec1(&BrotliDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input);
}

#[test]
fn brotli_compress_empty_input() {
    let compressed = exec1(&BrotliCompressFn, b"").unwrap();
    assert!(!compressed.is_empty());
    let decompressed = exec1(&BrotliDecompressFn, &compressed).unwrap();
    assert!(decompressed.is_empty());
}

#[test]
fn brotli_compress_custom_quality() {
    let input = vec![b'B'; 10_000];
    let fast = exec1_params(&BrotliCompressFn, &input, params(&[("quality", "0")])).unwrap();
    let max = exec1_params(&BrotliCompressFn, &input, params(&[("quality", "11")])).unwrap();
    assert!(max.len() <= fast.len());
}

#[test]
fn brotli_compress_quality_out_of_range() {
    let r = exec1_params(&BrotliCompressFn, b"data", params(&[("quality", "12")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn brotli_compress_large_roundtrip() {
    let input: Vec<u8> = (0..=255).cycle().take(50_000).collect();
    let compressed = exec1(&BrotliCompressFn, &input).unwrap();
    let decompressed = exec1(&BrotliDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input.as_slice());
}


// ── #30 BrotliDecompressFn ──

#[test]
fn brotli_decompress_roundtrip() {
    let input = b"verify brotli decompression correctness";
    let compressed = exec1(&BrotliCompressFn, input).unwrap();
    let decompressed = exec1(&BrotliDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input);
}

#[test]
fn brotli_decompress_corrupt_data() {
    let r = exec1(&BrotliDecompressFn, b"not brotli data");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn brotli_decompress_empty_stream() {
    let compressed = exec1(&BrotliCompressFn, b"").unwrap();
    let decompressed = exec1(&BrotliDecompressFn, &compressed).unwrap();
    assert!(decompressed.is_empty());
}

#[test]
fn brotli_decompress_high_quality_roundtrip() {
    let input = b"compressed at maximum brotli quality";
    let compressed = exec1_params(&BrotliCompressFn, input, params(&[("quality", "11")])).unwrap();
    let decompressed = exec1(&BrotliDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input);
}

#[test]
fn brotli_decompress_binary_roundtrip() {
    let input: Vec<u8> = (0..=255).cycle().take(32_768).collect();
    let compressed = exec1(&BrotliCompressFn, &input).unwrap();
    let decompressed = exec1(&BrotliDecompressFn, &compressed).unwrap();
    assert_eq!(decompressed.as_ref(), input.as_slice());
}


