use std::collections::HashMap;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use deriva_compute::builtins_streaming::*;
use deriva_compute::streaming::*;

fn hp(kv: &[(&str, &str)]) -> HashMap<String, String> {
    kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

async fn make_stream(chunks: Vec<&[u8]>) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(chunks.len() + 1);
    for c in chunks { tx.send(StreamChunk::Data(Bytes::copy_from_slice(c))).await.unwrap(); }
    tx.send(StreamChunk::End).await.unwrap();
    rx
}

async fn run_one(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> Bytes {
    let rx = make_stream(chunks).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

async fn run_one_err(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> String {
    let rx = make_stream(chunks).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap_err().to_string()
}

// ═══════════════════════════════════════════════════════════════════════
// #71 StreamingJsonValidate
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn json_validate_valid_object() {
    let out = run_one(&StreamingJsonValidate, vec![br#"{"a":1}"#], &hp(&[])).await;
    assert_eq!(out.as_ref(), br#"{"a":1}"#);
}

#[tokio::test]
async fn json_validate_valid_array() {
    let out = run_one(&StreamingJsonValidate, vec![b"[1,2,3]"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"[1,2,3]");
}

#[tokio::test]
async fn json_validate_invalid() {
    let err = run_one_err(&StreamingJsonValidate, vec![b"{broken"], &hp(&[])).await;
    assert!(err.contains("json"));
}

#[tokio::test]
async fn json_validate_multi_chunk() {
    let out = run_one(&StreamingJsonValidate, vec![br#"{"a""#, br#":1}"#], &hp(&[])).await;
    assert_eq!(out.as_ref(), br#"{"a":1}"#);
}

#[tokio::test]
async fn json_validate_preserves_whitespace() {
    let input = b"{ \"a\" : 1 }";
    let out = run_one(&StreamingJsonValidate, vec![input], &hp(&[])).await;
    assert_eq!(out.as_ref(), input); // original bytes unchanged
}

// ═══════════════════════════════════════════════════════════════════════
// #72 StreamingSchemaValidate
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn schema_validate_type_ok() {
    let schema = r#"{"type":"object"}"#;
    let out = run_one(&StreamingSchemaValidate, vec![br#"{"a":1}"#], &hp(&[("schema", schema)])).await;
    assert_eq!(out.as_ref(), br#"{"a":1}"#);
}

#[tokio::test]
async fn schema_validate_type_mismatch() {
    let schema = r#"{"type":"array"}"#;
    let err = run_one_err(&StreamingSchemaValidate, vec![br#"{"a":1}"#], &hp(&[("schema", schema)])).await;
    assert!(err.contains("expected type array"));
}

#[tokio::test]
async fn schema_validate_required_ok() {
    let schema = r#"{"type":"object","required":["name"]}"#;
    let err = run_one_err(&StreamingSchemaValidate, vec![br#"{"age":30}"#], &hp(&[("schema", schema)])).await;
    assert!(err.contains("missing required field: name"));
}

#[tokio::test]
async fn schema_validate_required_present() {
    let schema = r#"{"type":"object","required":["name"]}"#;
    let out = run_one(&StreamingSchemaValidate, vec![br#"{"name":"Alice"}"#], &hp(&[("schema", schema)])).await;
    assert!(out.len() > 0);
}

#[tokio::test]
async fn schema_validate_missing_param() {
    let err = run_one_err(&StreamingSchemaValidate, vec![b"{}"], &hp(&[])).await;
    assert!(err.contains("missing param"));
}

// ═══════════════════════════════════════════════════════════════════════
// #73 StreamingMagicBytes
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn magic_bytes_match() {
    // PNG magic: 89 50 4e 47
    let data = b"\x89PNG rest of data";
    let out = run_one(&StreamingMagicBytes, vec![data], &hp(&[("expected", "89504e47")])).await;
    assert_eq!(out.as_ref(), data);
}

#[tokio::test]
async fn magic_bytes_mismatch() {
    let err = run_one_err(&StreamingMagicBytes, vec![b"not png"], &hp(&[("expected", "89504e47")])).await;
    assert!(err.contains("magic bytes mismatch"));
}

#[tokio::test]
async fn magic_bytes_multi_chunk() {
    let out = run_one(&StreamingMagicBytes, vec![b"\x89PNG", b" more"], &hp(&[("expected", "89504e47")])).await;
    assert_eq!(out.as_ref(), b"\x89PNG more");
}

#[tokio::test]
async fn magic_bytes_missing_param() {
    let err = run_one_err(&StreamingMagicBytes, vec![b"data"], &hp(&[])).await;
    assert!(err.contains("missing param"));
}

#[tokio::test]
async fn magic_bytes_short_prefix() {
    let out = run_one(&StreamingMagicBytes, vec![b"\xffdata"], &hp(&[("expected", "ff")])).await;
    assert_eq!(out.as_ref(), b"\xffdata");
}

// ═══════════════════════════════════════════════════════════════════════
// #74 StreamingSizeLimit
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn size_limit_under() {
    let out = run_one(&StreamingSizeLimit, vec![b"hello"], &hp(&[("max_bytes", "100")])).await;
    assert_eq!(out.as_ref(), b"hello");
}

#[tokio::test]
async fn size_limit_exceeded() {
    let err = run_one_err(&StreamingSizeLimit, vec![b"hello world"], &hp(&[("max_bytes", "5")])).await;
    assert!(err.contains("size limit exceeded"));
}

#[tokio::test]
async fn size_limit_exact() {
    let out = run_one(&StreamingSizeLimit, vec![b"12345"], &hp(&[("max_bytes", "5")])).await;
    assert_eq!(out.as_ref(), b"12345");
}

#[tokio::test]
async fn size_limit_multi_chunk() {
    let err = run_one_err(&StreamingSizeLimit, vec![b"aaa", b"bbb", b"ccc"], &hp(&[("max_bytes", "7")])).await;
    assert!(err.contains("size limit"));
}

#[tokio::test]
async fn size_limit_missing_param() {
    let err = run_one_err(&StreamingSizeLimit, vec![b"x"], &hp(&[])).await;
    assert!(err.contains("missing param"));
}

// ═══════════════════════════════════════════════════════════════════════
// #75 StreamingChecksumVerify
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn checksum_verify_correct() {
    // CRC32 of "hello" = 3610a686
    let out = run_one(&StreamingChecksumVerify, vec![b"hello"], &hp(&[("expected_crc32", "3610a686")])).await;
    assert_eq!(out.as_ref(), b"hello");
}

#[tokio::test]
async fn checksum_verify_wrong() {
    let err = run_one_err(&StreamingChecksumVerify, vec![b"hello"], &hp(&[("expected_crc32", "00000000")])).await;
    assert!(err.contains("crc32 mismatch"));
}

#[tokio::test]
async fn checksum_verify_multi_chunk() {
    // CRC32 of "helloworld" = f9eb20ad
    let out = run_one(&StreamingChecksumVerify, vec![b"hello", b"world"], &hp(&[("expected_crc32", "f9eb20ad")])).await;
    assert_eq!(out.as_ref(), b"helloworld");
}

#[tokio::test]
async fn checksum_verify_missing_param() {
    let err = run_one_err(&StreamingChecksumVerify, vec![b"x"], &hp(&[])).await;
    assert!(err.contains("missing param"));
}

#[tokio::test]
async fn checksum_verify_preserves_data() {
    let data = b"some binary \x00\xff data";
    let crc = format!("{:08x}", crc32fast::hash(data));
    let out = run_one(&StreamingChecksumVerify, vec![data], &hp(&[("expected_crc32", &crc)])).await;
    assert_eq!(out.as_ref(), data);
}

// ═══════════════════════════════════════════════════════════════════════
// #76 StreamingSha256Verify
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sha256_verify_correct() {
    use sha2::{Sha256, Digest};
    let hash = format!("{:x}", Sha256::digest(b"hello"));
    let out = run_one(&StreamingSha256Verify, vec![b"hello"], &hp(&[("expected_hash", &hash)])).await;
    assert_eq!(out.as_ref(), b"hello");
}

#[tokio::test]
async fn sha256_verify_wrong() {
    let err = run_one_err(&StreamingSha256Verify, vec![b"hello"], &hp(&[("expected_hash", "0000000000000000000000000000000000000000000000000000000000000000")])).await;
    assert!(err.contains("sha256 mismatch"));
}

#[tokio::test]
async fn sha256_verify_multi_chunk() {
    use sha2::{Sha256, Digest};
    let hash = format!("{:x}", Sha256::digest(b"helloworld"));
    let out = run_one(&StreamingSha256Verify, vec![b"hello", b"world"], &hp(&[("expected_hash", &hash)])).await;
    assert_eq!(out.as_ref(), b"helloworld");
}

#[tokio::test]
async fn sha256_verify_missing_param() {
    let err = run_one_err(&StreamingSha256Verify, vec![b"x"], &hp(&[])).await;
    assert!(err.contains("missing param"));
}

#[tokio::test]
async fn sha256_verify_preserves_data() {
    use sha2::{Sha256, Digest};
    let data = b"binary\x00\xff";
    let hash = format!("{:x}", Sha256::digest(data));
    let out = run_one(&StreamingSha256Verify, vec![data], &hp(&[("expected_hash", &hash)])).await;
    assert_eq!(out.as_ref(), data);
}

// ═══════════════════════════════════════════════════════════════════════
// #77 StreamingNonEmpty
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn non_empty_with_data() {
    let out = run_one(&StreamingNonEmpty, vec![b"data"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"data");
}

#[tokio::test]
async fn non_empty_error_on_empty() {
    let err = run_one_err(&StreamingNonEmpty, vec![], &hp(&[])).await;
    assert!(err.contains("empty"));
}

#[tokio::test]
async fn non_empty_zero_len_chunks() {
    let err = run_one_err(&StreamingNonEmpty, vec![b"", b""], &hp(&[])).await;
    assert!(err.contains("empty"));
}

#[tokio::test]
async fn non_empty_passthrough() {
    let out = run_one(&StreamingNonEmpty, vec![b"a", b"b"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"ab");
}

#[tokio::test]
async fn non_empty_mixed_empty_and_data() {
    let out = run_one(&StreamingNonEmpty, vec![b"", b"x", b""], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"x");
}
