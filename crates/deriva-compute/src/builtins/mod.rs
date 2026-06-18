mod transforms;
mod compression;
mod crypto;
mod accumulators;
mod combiners;
mod slicing;
mod text;
mod validation;
mod format_conversion;
mod cas;
mod batch_only;
#[cfg(feature = "extended-batch")]
mod analytics;

pub use transforms::*;
pub use compression::*;
pub use crypto::*;
pub use accumulators::*;
pub use combiners::*;
pub use slicing::*;
pub use text::*;
pub use validation::*;
pub use format_conversion::*;
pub use cas::*;
pub use batch_only::*;
#[cfg(feature = "extended-batch")]
pub use analytics::*;

use crate::function::{ComputeCost, ComputeError};
use deriva_core::address::Value;
use std::collections::BTreeMap;

pub(crate) fn parse_byte_param(params: &BTreeMap<String, Value>, name: &str) -> Result<u8, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam(format!("{} must be 0-255", name))),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

pub(crate) fn parse_usize_param(params: &BTreeMap<String, Value>, name: &str) -> Result<usize, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam(format!("{} must be a positive integer", name))),
        Some(Value::Int(n)) if *n > 0 => Ok(*n as usize),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

pub(crate) fn parse_u64_param(params: &BTreeMap<String, Value>, name: &str) -> Result<u64, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => s.parse::<u64>().map_err(|_| ComputeError::InvalidParam(format!("{} must be a non-negative integer", name))),
        Some(Value::Int(n)) => u64::try_from(*n).map_err(|_| ComputeError::InvalidParam(format!("{} must be non-negative", name))),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

/// Split on \n, preserving empty trailing segment.
#[allow(dead_code)]
pub(crate) fn split_lines(input: &[u8]) -> Vec<&[u8]> {
    if input.is_empty() { return vec![b""]; }
    input.split(|&b| b == b'\n').collect()
}

pub(crate) fn get_string_param<'a>(params: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a str, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => Ok(s.as_str()),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

pub(crate) fn hex_decode_param(hex: &str, name: &str) -> Result<Vec<u8>, ComputeError> {
    if !hex.len().is_multiple_of(2) {
        return Err(ComputeError::InvalidParam(format!("odd-length hex in {}", name)));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| ComputeError::InvalidParam(format!("invalid hex in {}", name)))
        })
        .collect()
}

/// Spec §4.2 cost formula: base_cost * (total_input_bytes / 1024).max(1)
fn spec_cost(base: u64, input_sizes: &[u64]) -> ComputeCost {
    let total: u64 = input_sizes.iter().sum();
    let scale = (total / 1024).max(1);
    ComputeCost { cpu_ms: base * scale, memory_bytes: total }
}

pub fn register_all(registry: &mut crate::registry::FunctionRegistry) {
    use std::sync::Arc;
    registry.register(Arc::new(IdentityFn));
    registry.register(Arc::new(ConcatFn));
    registry.register(Arc::new(UppercaseFn));
    registry.register(Arc::new(RepeatFn));
    registry.register(Arc::new(LowercaseFn));
    registry.register(Arc::new(ReverseFn));
    registry.register(Arc::new(Base64EncodeFn));
    registry.register(Arc::new(Base64DecodeFn));
    registry.register(Arc::new(HexEncodeFn));
    registry.register(Arc::new(HexDecodeFn));
    registry.register(Arc::new(Base32EncodeFn));
    registry.register(Arc::new(Base32DecodeFn));
    registry.register(Arc::new(XorFn));
    registry.register(Arc::new(BitwiseAndFn));
    registry.register(Arc::new(BitwiseOrFn));
    registry.register(Arc::new(BitwiseNotFn));
    registry.register(Arc::new(ByteSwapFn));
    registry.register(Arc::new(TrimFn));
    registry.register(Arc::new(PadFn));
    registry.register(Arc::new(LineEndingFn));
    registry.register(Arc::new(CompressFn));
    registry.register(Arc::new(DecompressFn));
    registry.register(Arc::new(ZstdCompressFn));
    registry.register(Arc::new(ZstdDecompressFn));
    registry.register(Arc::new(Lz4CompressFn));
    registry.register(Arc::new(Lz4DecompressFn));
    registry.register(Arc::new(SnappyCompressFn));
    registry.register(Arc::new(SnappyDecompressFn));
    registry.register(Arc::new(BrotliCompressFn));
    registry.register(Arc::new(BrotliDecompressFn));
    registry.register(Arc::new(GzipCompressFn));
    registry.register(Arc::new(GzipDecompressFn));
    registry.register(Arc::new(Sha256Fn));
    registry.register(Arc::new(Sha384Fn));
    registry.register(Arc::new(Sha512Fn));
    registry.register(Arc::new(Md5Fn));
    registry.register(Arc::new(Blake3Fn));
    registry.register(Arc::new(HmacSha256Fn));
    registry.register(Arc::new(Crc32Fn));
    registry.register(Arc::new(EncryptFn));
    registry.register(Arc::new(DecryptFn));
    registry.register(Arc::new(AeadEncryptFn));
    registry.register(Arc::new(AeadDecryptFn));
    registry.register(Arc::new(RedactFn));
    registry.register(Arc::new(ByteCountFn));
    registry.register(Arc::new(LineCountFn));
    registry.register(Arc::new(WordCountFn));
    registry.register(Arc::new(HistogramFn));
    registry.register(Arc::new(EntropyFn));
    registry.register(Arc::new(MinMaxFn));
    registry.register(Arc::new(SumFn));
    registry.register(Arc::new(AverageFn));
    registry.register(Arc::new(InterleaveFn));
    registry.register(Arc::new(ZipConcatFn));
    registry.register(Arc::new(DiffFn));
    registry.register(Arc::new(PatchFn));
    registry.register(Arc::new(MergeSortedFn));
    registry.register(Arc::new(SelectFn));
    registry.register(Arc::new(TakeFn));
    registry.register(Arc::new(SkipFn));
    registry.register(Arc::new(SliceFn));
    registry.register(Arc::new(SortFn));
    registry.register(Arc::new(UniqueFn));
    registry.register(Arc::new(SortUniqueFn));
    registry.register(Arc::new(ShuffleFn));
    registry.register(Arc::new(HeadFn));
    registry.register(Arc::new(TailFn));
    registry.register(Arc::new(SampleFn));
    registry.register(Arc::new(NthFn));
    registry.register(Arc::new(ChunkSplitFn));
    registry.register(Arc::new(ZipFn));
    registry.register(Arc::new(ReplaceFn));
    registry.register(Arc::new(RegexReplaceFn));
    registry.register(Arc::new(GrepFn));
    registry.register(Arc::new(GrepInvertFn));
    registry.register(Arc::new(PrefixFn));
    registry.register(Arc::new(SuffixFn));
    registry.register(Arc::new(LinePrefixFn));
    registry.register(Arc::new(LineNumberFn));
    registry.register(Arc::new(TruncateLinesFn));
    registry.register(Arc::new(CharsetConvertFn));
    registry.register(Arc::new(Utf8ValidateFn));
    registry.register(Arc::new(JsonValidateFn));
    registry.register(Arc::new(SchemaValidateFn));
    registry.register(Arc::new(MagicBytesFn));
    registry.register(Arc::new(SizeLimitFn));
    registry.register(Arc::new(NonEmptyFn));
    registry.register(Arc::new(Sha256VerifyFn));
    registry.register(Arc::new(Crc32VerifyFn));
    registry.register(Arc::new(JsonPrettyPrintFn));
    registry.register(Arc::new(JsonMinifyFn));
    registry.register(Arc::new(CsvToJsonFn));
    registry.register(Arc::new(JsonToCsvFn));
    registry.register(Arc::new(JsonLinesFn));
    registry.register(Arc::new(YamlToJsonFn));
    registry.register(Arc::new(JsonToYamlFn));
    registry.register(Arc::new(TomlToJsonFn));
    registry.register(Arc::new(CAddrComputeFn));
    registry.register(Arc::new(CAddrVerifyFn));
    registry.register(Arc::new(CAddrEmbedFn));
    registry.register(Arc::new(MerkleRootFn));
    registry.register(Arc::new(ContentTypeFn));
    registry.register(Arc::new(ChunkHashFn));
    registry.register(Arc::new(DedupAnalyzeFn));
    registry.register(Arc::new(ReverseByteFn));
    registry.register(Arc::new(SortBytesFn));

    // Streaming functions (#1–#100)
    crate::builtins_streaming::register_streaming_builtins(registry);

    // Extended batch functions (gated behind extended-batch feature)
    #[cfg(feature = "extended-batch")]
    register_extended_batch(registry);
}

/// Register extended batch functions (96 new functions).
/// Gated behind the `extended-batch` feature flag.
#[cfg(feature = "extended-batch")]
pub fn register_extended_batch(registry: &mut crate::registry::FunctionRegistry) {
    use std::sync::Arc;

    // Encoding functions (base64url, base58, url, html)
    registry.register(Arc::new(Base64UrlEncodeFn));
    registry.register(Arc::new(Base64UrlDecodeFn));
    registry.register(Arc::new(Base58EncodeFn));
    registry.register(Arc::new(Base58DecodeFn));
    registry.register(Arc::new(UrlEncodeFn));
    registry.register(Arc::new(UrlDecodeFn));
    registry.register(Arc::new(HtmlEncodeFn));
    registry.register(Arc::new(HtmlDecodeFn));

    // Crypto functions (HMAC, AES-CTR, AES-GCM, ChaCha20)
    registry.register(Arc::new(HmacFn));
    registry.register(Arc::new(AesCtrEncryptFn));
    registry.register(Arc::new(AesCtrDecryptFn));
    registry.register(Arc::new(AesGcmEncryptFn));
    registry.register(Arc::new(AesGcmDecryptFn));
    registry.register(Arc::new(ChaCha20EncryptFn));
    registry.register(Arc::new(ChaCha20DecryptFn));

    // Crypto: Ed25519 and Argon2
    registry.register(Arc::new(Ed25519SignFn));
    registry.register(Arc::new(Ed25519VerifyFn));
    registry.register(Arc::new(Argon2HashFn));

    // Text: split, join, sort_lines, unique_lines
    registry.register(Arc::new(SplitFn));
    registry.register(Arc::new(JoinFn));
    registry.register(Arc::new(SortLinesFn));
    registry.register(Arc::new(UniqueLinesFn));

    // Validation: size_check, not_empty, regex_match, content_type_check
    registry.register(Arc::new(SizeCheckFn));
    registry.register(Arc::new(NotEmptyFn));
    registry.register(Arc::new(RegexMatchFn));
    registry.register(Arc::new(ContentTypeCheckFn));

    // Analytics functions
    registry.register(Arc::new(ReservoirSampleFn));
    registry.register(Arc::new(PercentileFn));
    registry.register(Arc::new(MeanFn));
    registry.register(Arc::new(MedianFn));
    registry.register(Arc::new(NumericMinFn));
    registry.register(Arc::new(NumericMaxFn));
    registry.register(Arc::new(FrequencyFn));
    registry.register(Arc::new(DedupCountFn));
    registry.register(Arc::new(CardinalityFn));

    // Combiner functions (Spec Requirement 9)
    registry.register(Arc::new(InterleaveBytesFn));
    registry.register(Arc::new(MergeFn));
    registry.register(Arc::new(SelectInputFn));
    registry.register(Arc::new(AlternateFn));
}
