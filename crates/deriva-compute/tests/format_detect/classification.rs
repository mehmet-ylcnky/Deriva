use bytes::Bytes;
use deriva_compute::builtins_format_detect::{IsBinaryFn, IsCompressedFn, IsEncryptedFn, IsTextFn};
use deriva_compute::function::ComputeFunction;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// IsTextFn tests
// ---------------------------------------------------------------------------

#[test]
fn is_text_plain_ascii() {
    let data = b"Hello, world! This is plain text.\nWith multiple lines.\n";
    let result = IsTextFn
        .execute(vec![Bytes::from(data.to_vec())], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"true");
}

#[test]
fn is_text_utf8_multibyte() {
    let data = "héllo wörld — Unicode text with em-dash".as_bytes();
    let result = IsTextFn
        .execute(vec![Bytes::from(data.to_vec())], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"true");
}

#[test]
fn is_text_binary_with_nulls() {
    // Binary data with null bytes interspersed
    let data: Vec<u8> = vec![0x00, 0x01, 0x02, 0xFF, 0x00, 0x89, 0x50, 0x4E, 0x47];
    let result = IsTextFn
        .execute(vec![Bytes::from(data)], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"false");
}

#[test]
fn is_text_empty_input_errors() {
    let result = IsTextFn.execute(vec![Bytes::new()], &BTreeMap::new());
    assert!(result.is_err());
}

#[test]
fn is_text_no_input_errors() {
    let result = IsTextFn.execute(vec![], &BTreeMap::new());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// IsBinaryFn tests
// ---------------------------------------------------------------------------

#[test]
fn is_binary_plain_text_returns_false() {
    let data = b"This is plain text content for testing.\n";
    let result = IsBinaryFn
        .execute(vec![Bytes::from(data.to_vec())], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"false");
}

#[test]
fn is_binary_binary_data_returns_true() {
    // PNG-like binary data with nulls
    let data: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
        0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    ];
    let result = IsBinaryFn
        .execute(vec![Bytes::from(data)], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"true");
}

#[test]
fn is_binary_empty_input_errors() {
    let result = IsBinaryFn.execute(vec![Bytes::new()], &BTreeMap::new());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// IsCompressedFn tests
// ---------------------------------------------------------------------------

#[test]
fn is_compressed_gzip_magic() {
    // gzip magic bytes: 0x1f 0x8b
    let data = vec![0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff];
    let result = IsCompressedFn
        .execute(vec![Bytes::from(data)], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"true");
}

#[test]
fn is_compressed_zstd_magic() {
    // zstd magic: 0x28 0xb5 0x2f 0xfd
    let data = vec![0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x00, 0x00, 0x00];
    let result = IsCompressedFn
        .execute(vec![Bytes::from(data)], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"true");
}

#[test]
fn is_compressed_bzip2_magic() {
    // bzip2 magic: "BZh"
    let mut data = b"BZh9".to_vec();
    data.extend_from_slice(&[0x31, 0x41, 0x59, 0x26, 0x53, 0x59]);
    let result = IsCompressedFn
        .execute(vec![Bytes::from(data)], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"true");
}

#[test]
fn is_compressed_plain_text_returns_false() {
    let data = b"This is plain text, definitely not compressed data at all.";
    let result = IsCompressedFn
        .execute(vec![Bytes::from(data.to_vec())], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"false");
}

#[test]
fn is_compressed_empty_input_errors() {
    let result = IsCompressedFn.execute(vec![Bytes::new()], &BTreeMap::new());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// IsEncryptedFn tests
// ---------------------------------------------------------------------------

#[test]
fn is_encrypted_high_entropy_no_magic() {
    // Generate high-entropy binary data that won't be classified as text.
    // Use all 256 byte values (including null and control chars) to ensure
    // both high entropy and binary classification.
    let mut data: Vec<u8> = (0..=255u8).collect();
    // Repeat to get 512 bytes (still max entropy, definitely not text due to nulls & control chars)
    data.extend(data.clone());
    let result = IsEncryptedFn
        .execute(vec![Bytes::from(data)], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"true");
}

#[test]
fn is_encrypted_compressed_data_returns_false() {
    // gzip data → compressed, not encrypted
    let data = vec![0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff];
    let result = IsEncryptedFn
        .execute(vec![Bytes::from(data)], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"false");
}

#[test]
fn is_encrypted_plain_text_returns_false() {
    let data = b"This is just plain text, not encrypted at all.";
    let result = IsEncryptedFn
        .execute(vec![Bytes::from(data.to_vec())], &BTreeMap::new())
        .unwrap();
    assert_eq!(&result[..], b"false");
}

#[test]
fn is_encrypted_empty_input_errors() {
    let result = IsEncryptedFn.execute(vec![Bytes::new()], &BTreeMap::new());
    assert!(result.is_err());
}
