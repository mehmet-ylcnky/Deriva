use std::sync::Arc;
use deriva_core::address::FunctionId;
use crate::registry::FunctionRegistry;
use crate::builtins_streaming::*;

fn sid(name: &str) -> FunctionId {
    FunctionId::new(name, "1.0")
}

/// Register all 100 streaming compute functions.
pub fn register_streaming_builtins(registry: &mut FunctionRegistry) {
    // §1 Original 20 (#1–#20)
    registry.register_streaming(Arc::new(StreamingIdentity), sid("streaming_identity"));
    registry.register_streaming(Arc::new(StreamingUppercase), sid("streaming_uppercase"));
    registry.register_streaming(Arc::new(StreamingLowercase), sid("streaming_lowercase"));
    registry.register_streaming(Arc::new(StreamingReverse), sid("streaming_reverse"));
    registry.register_streaming(Arc::new(StreamingBase64Encode), sid("streaming_base64_encode"));
    registry.register_streaming(Arc::new(StreamingBase64Decode), sid("streaming_base64_decode"));
    registry.register_streaming(Arc::new(StreamingXor), sid("streaming_xor"));
    registry.register_streaming(Arc::new(StreamingCompress), sid("streaming_compress"));
    registry.register_streaming(Arc::new(StreamingDecompress), sid("streaming_decompress"));
    registry.register_streaming(Arc::new(StreamingSha256), sid("streaming_sha256"));
    registry.register_streaming(Arc::new(StreamingByteCount), sid("streaming_byte_count"));
    registry.register_streaming(Arc::new(StreamingChecksum), sid("streaming_checksum"));
    registry.register_streaming(Arc::new(StreamingConcat), sid("streaming_concat"));
    registry.register_streaming(Arc::new(StreamingInterleave), sid("streaming_interleave"));
    registry.register_streaming(Arc::new(StreamingZipConcat), sid("streaming_zip_concat"));
    registry.register_streaming(Arc::new(StreamingChunkResizer), sid("streaming_chunk_resizer"));
    registry.register_streaming(Arc::new(StreamingTake), sid("streaming_take"));
    registry.register_streaming(Arc::new(StreamingSkip), sid("streaming_skip"));
    registry.register_streaming(Arc::new(StreamingRepeat), sid("streaming_repeat"));
    registry.register_streaming(Arc::new(StreamingTeeCount), sid("streaming_tee_count"));

    // §3.1 Crypto & Security (#21–#29)
    registry.register_streaming(Arc::new(StreamingEncrypt), sid("streaming_encrypt"));
    registry.register_streaming(Arc::new(StreamingDecrypt), sid("streaming_decrypt"));
    registry.register_streaming(Arc::new(StreamingAeadEncrypt), sid("streaming_aead_encrypt"));
    registry.register_streaming(Arc::new(StreamingAeadDecrypt), sid("streaming_aead_decrypt"));
    registry.register_streaming(Arc::new(StreamingHmacSha256), sid("streaming_hmac"));
    registry.register_streaming(Arc::new(StreamingMd5), sid("streaming_md5"));
    registry.register_streaming(Arc::new(StreamingSha512), sid("streaming_sha512"));
    registry.register_streaming(Arc::new(StreamingBlake3), sid("streaming_blake3"));
    registry.register_streaming(Arc::new(StreamingRedact), sid("streaming_redact"));

    // §3.2 Encoding & Format (#30–#39)
    registry.register_streaming(Arc::new(StreamingHexEncode), sid("streaming_hex_encode"));
    registry.register_streaming(Arc::new(StreamingHexDecode), sid("streaming_hex_decode"));
    registry.register_streaming(Arc::new(StreamingUtf8Validate), sid("streaming_utf8_validate"));
    registry.register_streaming(Arc::new(StreamingLineEnding), sid("streaming_line_ending"));
    registry.register_streaming(Arc::new(StreamingJsonPrettyPrint), sid("streaming_json_pretty_print"));
    registry.register_streaming(Arc::new(StreamingJsonMinify), sid("streaming_json_minify"));
    registry.register_streaming(Arc::new(StreamingJsonLines), sid("streaming_json_lines"));
    registry.register_streaming(Arc::new(StreamingCsvToJson), sid("streaming_csv_to_json"));
    registry.register_streaming(Arc::new(StreamingBase32Encode), sid("streaming_base32_encode"));
    registry.register_streaming(Arc::new(StreamingBase32Decode), sid("streaming_base32_decode"));

    // §3.3 Data Processing & Analytics (#40–#50)
    registry.register_streaming(Arc::new(StreamingFilter), sid("streaming_filter"));
    registry.register_streaming(Arc::new(StreamingLineCount), sid("streaming_line_count"));
    registry.register_streaming(Arc::new(StreamingWordCount), sid("streaming_word_count"));
    registry.register_streaming(Arc::new(StreamingMinMax), sid("streaming_min_max"));
    registry.register_streaming(Arc::new(StreamingHistogram), sid("streaming_histogram"));
    registry.register_streaming(Arc::new(StreamingSample), sid("streaming_sample"));
    registry.register_streaming(Arc::new(StreamingHead), sid("streaming_head"));
    registry.register_streaming(Arc::new(StreamingTail), sid("streaming_tail"));
    registry.register_streaming(Arc::new(StreamingDeduplicate), sid("streaming_deduplicate"));
    registry.register_streaming(Arc::new(StreamingSort), sid("streaming_sort"));
    registry.register_streaming(Arc::new(StreamingUnique), sid("streaming_unique"));

    // §3.4 Compression & Transformation (#51–#60)
    registry.register_streaming(Arc::new(StreamingZstdCompress), sid("streaming_zstd_compress"));
    registry.register_streaming(Arc::new(StreamingZstdDecompress), sid("streaming_zstd_decompress"));
    registry.register_streaming(Arc::new(StreamingLz4Compress), sid("streaming_lz4_compress"));
    registry.register_streaming(Arc::new(StreamingLz4Decompress), sid("streaming_lz4_decompress"));
    registry.register_streaming(Arc::new(StreamingSnappyCompress), sid("streaming_snappy_compress"));
    registry.register_streaming(Arc::new(StreamingSnappyDecompress), sid("streaming_snappy_decompress"));
    registry.register_streaming(Arc::new(StreamingBrotliCompress), sid("streaming_brotli_compress"));
    registry.register_streaming(Arc::new(StreamingBrotliDecompress), sid("streaming_brotli_decompress"));
    registry.register_streaming(Arc::new(StreamingPad), sid("streaming_pad"));
    registry.register_streaming(Arc::new(StreamingTrim), sid("streaming_trim"));

    // §3.5 Flow Control & Pipeline (#61–#70)
    registry.register_streaming(Arc::new(StreamingRateLimit), sid("streaming_rate_limit"));
    registry.register_streaming(Arc::new(StreamingDelay), sid("streaming_delay"));
    registry.register_streaming(Arc::new(StreamingTimeout), sid("streaming_timeout"));
    registry.register_streaming(Arc::new(StreamingRetry), sid("streaming_retry"));
    registry.register_streaming(Arc::new(StreamingTee), sid("streaming_tee"));
    registry.register_streaming(Arc::new(StreamingMerge), sid("streaming_merge"));
    registry.register_streaming(Arc::new(StreamingBroadcast), sid("streaming_broadcast"));
    registry.register_streaming(Arc::new(StreamingPartition), sid("streaming_partition"));
    registry.register_streaming(Arc::new(StreamingBatch), sid("streaming_batch"));
    registry.register_streaming(Arc::new(StreamingDebounce), sid("streaming_debounce"));

    // §3.6 Validation & Integrity (#71–#77)
    registry.register_streaming(Arc::new(StreamingJsonValidate), sid("streaming_json_validate"));
    registry.register_streaming(Arc::new(StreamingSchemaValidate), sid("streaming_schema_validate"));
    registry.register_streaming(Arc::new(StreamingMagicBytes), sid("streaming_magic_bytes"));
    registry.register_streaming(Arc::new(StreamingSizeLimit), sid("streaming_size_limit"));
    registry.register_streaming(Arc::new(StreamingChecksumVerify), sid("streaming_checksum_verify"));
    registry.register_streaming(Arc::new(StreamingSha256Verify), sid("streaming_sha256_verify"));
    registry.register_streaming(Arc::new(StreamingNonEmpty), sid("streaming_non_empty"));

    // §3.7 Text Processing (#78–#85)
    registry.register_streaming(Arc::new(StreamingReplace), sid("streaming_replace"));
    registry.register_streaming(Arc::new(StreamingPrefix), sid("streaming_prefix"));
    registry.register_streaming(Arc::new(StreamingSuffix), sid("streaming_suffix"));
    registry.register_streaming(Arc::new(StreamingLinePrefix), sid("streaming_line_prefix"));
    registry.register_streaming(Arc::new(StreamingGrep), sid("streaming_grep"));
    registry.register_streaming(Arc::new(StreamingSed), sid("streaming_sed"));
    registry.register_streaming(Arc::new(StreamingTruncateLines), sid("streaming_truncate_lines"));
    registry.register_streaming(Arc::new(StreamingCharsetConvert), sid("streaming_charset_convert"));

    // §3.8 CAS-Specific Operations (#86–#92)
    registry.register_streaming(Arc::new(StreamingCAddrEmbed), sid("streaming_caddr_embed"));
    registry.register_streaming(Arc::new(StreamingCAddrVerify), sid("streaming_caddr_verify"));
    registry.register_streaming(Arc::new(StreamingDiff), sid("streaming_diff"));
    registry.register_streaming(Arc::new(StreamingPatch), sid("streaming_patch"));
    registry.register_streaming(Arc::new(StreamingMerkleTree), sid("streaming_merkle_tree"));
    registry.register_streaming(Arc::new(StreamingContentType), sid("streaming_content_type"));
    registry.register_streaming(Arc::new(StreamingChunkHash), sid("streaming_chunk_hash"));

    // §3.9 Numeric & Scientific (#93–#100)
    registry.register_streaming(Arc::new(StreamingSum), sid("streaming_sum"));
    registry.register_streaming(Arc::new(StreamingAverage), sid("streaming_average"));
    registry.register_streaming(Arc::new(StreamingBitwiseAnd), sid("streaming_bitwise_and"));
    registry.register_streaming(Arc::new(StreamingBitwiseOr), sid("streaming_bitwise_or"));
    registry.register_streaming(Arc::new(StreamingBitwiseNot), sid("streaming_bitwise_not"));
    registry.register_streaming(Arc::new(StreamingByteSwap), sid("streaming_byte_swap"));
    registry.register_streaming(Arc::new(StreamingEntropy), sid("streaming_entropy"));
    registry.register_streaming(Arc::new(StreamingRollingHash), sid("streaming_rolling_hash"));
}
