//! Property-based tests for format detection accuracy (Property 9).
//!
//! **Validates: Requirements 4.7**
//!
//! For any file with a standard magic byte signature (PNG, JPEG, PDF, ZIP, tar, gzip, etc.),
//! the format detection function SHALL correctly identify the MIME type.

#![cfg(feature = "format-detect")]

use bytes::Bytes;
use proptest::prelude::*;
use std::collections::BTreeMap;

use deriva_compute::builtins_format_detect::{
    EntropyScoreFn, FormatDetectFn, IsBinaryFn, IsCompressedFn, IsTextFn,
};
use deriva_compute::function::ComputeFunction;

// ---------------------------------------------------------------------------
// Magic byte signatures for known formats
// ---------------------------------------------------------------------------

/// (magic_bytes, expected_format, expected_mime)
const KNOWN_SIGNATURES: &[(&[u8], &str, &str)] = &[
    (b"\x89PNG", "png", "image/png"),
    (b"\xff\xd8\xff", "jpeg", "image/jpeg"),
    (b"GIF8", "gif", "image/gif"),
    (b"%PDF", "pdf", "application/pdf"),
    (b"PK\x03\x04", "zip", "application/zip"),
    (b"\x1f\x8b", "gzip", "application/gzip"),
    (b"\xfd7zXZ\x00", "xz", "application/x-xz"),
    (b"\x28\xb5\x2f\xfd", "zstd", "application/zstd"),
    (b"BZh", "bzip2", "application/x-bzip2"),
    (b"PAR1", "parquet", "application/vnd.apache.parquet"),
    (b"ORC", "orc", "application/x-orc"),
    (b"ARROW1", "arrow_ipc", "application/vnd.apache.arrow.file"),
    (b"Obj\x01", "avro", "application/avro"),
    (b"RIFF", "wav", "audio/wav"),
    (b"fLaC", "flac", "audio/flac"),
    (b"ID3", "mp3", "audio/mpeg"),
    (b"SQLite format 3\0", "sqlite", "application/x-sqlite3"),
    (b"\x00asm", "wasm", "application/wasm"),
    (b"\x7fELF", "elf", "application/x-elf"),
    (b"\x93NUMPY", "numpy", "application/x-numpy"),
    (b"GGUF", "gguf", "application/x-gguf"),
];

/// Compression magic bytes for IsCompressedFn tests
const COMPRESSION_SIGNATURES: &[&[u8]] = &[
    b"\x1f\x8b",           // gzip
    b"\x28\xb5\x2f\xfd",   // zstd
    b"BZh",                 // bzip2
    b"\xfd7zXZ\x00",       // xz
    b"PK\x03\x04",         // zip
];

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Strategy to pick a random known format signature index
fn signature_index_strategy() -> impl Strategy<Value = usize> {
    0..KNOWN_SIGNATURES.len()
}

/// Strategy to pick a random compression signature index
fn compression_index_strategy() -> impl Strategy<Value = usize> {
    0..COMPRESSION_SIGNATURES.len()
}

// ---------------------------------------------------------------------------
// Property 9: Detection accuracy — magic byte formats
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// **Validates: Requirements 4.7**
    ///
    /// For any file with a standard magic byte signature prepended to random
    /// payload data, FormatDetectFn SHALL correctly identify the format and MIME type.
    #[test]
    fn detection_identifies_magic_byte_formats(
        sig_idx in signature_index_strategy(),
        random_payload in prop::collection::vec(any::<u8>(), 0..512)
    ) {
        let (magic, expected_format, expected_mime) = KNOWN_SIGNATURES[sig_idx];

        // Construct input: magic bytes + random payload
        let mut data = magic.to_vec();
        data.extend_from_slice(&random_payload);

        let result = FormatDetectFn
            .execute(vec![Bytes::from(data)], &BTreeMap::new())
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&result).unwrap();

        prop_assert_eq!(
            json["format"].as_str().unwrap(),
            expected_format,
            "Expected format '{}' but got '{}' for magic {:?}",
            expected_format,
            json["format"],
            magic
        );
        prop_assert_eq!(
            json["mime"].as_str().unwrap(),
            expected_mime,
            "Expected MIME '{}' but got '{}' for format '{}'",
            expected_mime,
            json["mime"],
            expected_format
        );
        // Confidence should be high for magic-byte detections
        let confidence = json["confidence"].as_f64().unwrap();
        prop_assert!(
            confidence >= 0.9,
            "Confidence {} too low for magic-byte format '{}'",
            confidence,
            expected_format
        );
    }

    /// **Validates: Requirements 4.7**
    ///
    /// IsCompressedFn SHALL return "true" for data prepended with known
    /// compression format magic bytes.
    #[test]
    fn is_compressed_detects_compression_magic(
        sig_idx in compression_index_strategy(),
        random_payload in prop::collection::vec(any::<u8>(), 1..256)
    ) {
        let magic = COMPRESSION_SIGNATURES[sig_idx];

        let mut data = magic.to_vec();
        data.extend_from_slice(&random_payload);

        let result = IsCompressedFn
            .execute(vec![Bytes::from(data)], &BTreeMap::new())
            .unwrap();

        prop_assert_eq!(
            result.as_ref(),
            b"true",
            "IsCompressedFn should return 'true' for compression magic {:?}",
            magic
        );
    }

    /// **Validates: Requirements 4.7**
    ///
    /// EntropyScoreFn SHALL always return a value in [0.0, 8.0] for any
    /// non-empty input (Shannon entropy is bounded by log2(256) = 8).
    #[test]
    fn entropy_score_bounded(
        data in prop::collection::vec(any::<u8>(), 1..2048)
    ) {
        let result = EntropyScoreFn
            .execute(vec![Bytes::from(data)], &BTreeMap::new())
            .unwrap();

        let score_str = std::str::from_utf8(&result).unwrap();
        let score: f64 = score_str.parse().unwrap();

        prop_assert!(
            score >= 0.0 && score <= 8.0,
            "Entropy score {} out of valid range [0.0, 8.0]",
            score
        );
    }

    /// **Validates: Requirements 4.7**
    ///
    /// For any non-empty input, IsTextFn and IsBinaryFn SHALL always return
    /// opposite boolean results (they are logical complements).
    #[test]
    fn is_text_and_is_binary_are_complements(
        data in prop::collection::vec(any::<u8>(), 1..1024)
    ) {
        let input = vec![Bytes::from(data)];
        let params = BTreeMap::new();

        let text_result = IsTextFn.execute(input.clone(), &params).unwrap();
        let binary_result = IsBinaryFn.execute(input, &params).unwrap();

        let is_text = text_result.as_ref() == b"true";
        let is_binary = binary_result.as_ref() == b"true";

        prop_assert_ne!(
            is_text, is_binary,
            "IsTextFn={} and IsBinaryFn={} must be logical complements",
            std::str::from_utf8(&text_result).unwrap(),
            std::str::from_utf8(&binary_result).unwrap()
        );
    }
}
