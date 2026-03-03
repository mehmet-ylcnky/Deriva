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
    let (tx, rx) = mpsc::channel(8);
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
// #30 StreamingHexEncode
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn hex_encode_basic() {
    let out = run_one(&StreamingHexEncode, vec![b"\x00\xff\xab"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"00ffab");
}

#[tokio::test]
async fn hex_encode_ascii() {
    let out = run_one(&StreamingHexEncode, vec![b"ABC"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"414243");
}

#[tokio::test]
async fn hex_encode_empty() {
    let out = run_one(&StreamingHexEncode, vec![b""], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn hex_encode_doubles_size() {
    let out = run_one(&StreamingHexEncode, vec![b"hello"], &hp(&[])).await;
    assert_eq!(out.len(), 10);
}

#[tokio::test]
async fn hex_encode_multi_chunk() {
    let out = run_one(&StreamingHexEncode, vec![b"\xde", b"\xad"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"dead");
}

// ═══════════════════════════════════════════════════════════════════════
// #31 StreamingHexDecode
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn hex_decode_basic() {
    let out = run_one(&StreamingHexDecode, vec![b"00ffab"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"\x00\xff\xab");
}

#[tokio::test]
async fn hex_decode_roundtrip() {
    let enc = run_one(&StreamingHexEncode, vec![b"test data"], &hp(&[])).await;
    let dec = run_one(&StreamingHexDecode, vec![enc.as_ref()], &hp(&[])).await;
    assert_eq!(dec.as_ref(), b"test data");
}

#[tokio::test]
async fn hex_decode_empty() {
    let out = run_one(&StreamingHexDecode, vec![b""], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn hex_decode_odd_length_error() {
    let err = run_one_err(&StreamingHexDecode, vec![b"abc"], &hp(&[])).await;
    assert!(err.contains("odd-length"));
}

#[tokio::test]
async fn hex_decode_invalid_char_error() {
    let err = run_one_err(&StreamingHexDecode, vec![b"zz"], &hp(&[])).await;
    assert!(err.contains("invalid"));
}

// ═══════════════════════════════════════════════════════════════════════
// #32 StreamingUtf8Validate
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn utf8_valid_ascii() {
    let out = run_one(&StreamingUtf8Validate, vec![b"hello world"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"hello world");
}

#[tokio::test]
async fn utf8_valid_multibyte() {
    let out = run_one(&StreamingUtf8Validate, vec!["héllo".as_bytes()], &hp(&[])).await;
    assert_eq!(std::str::from_utf8(&out).unwrap(), "héllo");
}

#[tokio::test]
async fn utf8_split_multibyte() {
    // é is 0xC3 0xA9 — split across chunks
    let out = run_one(&StreamingUtf8Validate, vec![b"h\xc3", b"\xa9llo"], &hp(&[])).await;
    assert_eq!(std::str::from_utf8(&out).unwrap(), "héllo");
}

#[tokio::test]
async fn utf8_invalid_byte() {
    let err = run_one_err(&StreamingUtf8Validate, vec![b"\xff\xfe"], &hp(&[])).await;
    assert!(err.contains("utf8"));
}

#[tokio::test]
async fn utf8_empty() {
    let out = run_one(&StreamingUtf8Validate, vec![b""], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// #33 StreamingLineEnding
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn line_ending_crlf_to_lf() {
    let out = run_one(&StreamingLineEnding, vec![b"a\r\nb\r\n"], &hp(&[("target", "lf")])).await;
    assert_eq!(out.as_ref(), b"a\nb\n");
}

#[tokio::test]
async fn line_ending_lf_to_crlf() {
    let out = run_one(&StreamingLineEnding, vec![b"a\nb\n"], &hp(&[("target", "crlf")])).await;
    assert_eq!(out.as_ref(), b"a\r\nb\r\n");
}

#[tokio::test]
async fn line_ending_default_is_lf() {
    let out = run_one(&StreamingLineEnding, vec![b"a\r\nb"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"a\nb");
}

#[tokio::test]
async fn line_ending_no_double_crlf() {
    // Already \r\n should not become \r\r\n
    let out = run_one(&StreamingLineEnding, vec![b"a\r\nb"], &hp(&[("target", "crlf")])).await;
    assert_eq!(out.as_ref(), b"a\r\nb");
}

#[tokio::test]
async fn line_ending_empty() {
    let out = run_one(&StreamingLineEnding, vec![b""], &hp(&[("target", "lf")])).await;
    assert_eq!(out.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// #34 StreamingJsonPrettyPrint
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn json_pretty_basic() {
    let out = run_one(&StreamingJsonPrettyPrint, vec![br#"{"a":1}"#], &hp(&[])).await;
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("  \"a\": 1"));
}

#[tokio::test]
async fn json_pretty_array() {
    let out = run_one(&StreamingJsonPrettyPrint, vec![b"[1,2,3]"], &hp(&[])).await;
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains('\n'));
}

#[tokio::test]
async fn json_pretty_multi_chunk() {
    let out = run_one(&StreamingJsonPrettyPrint, vec![br#"{"a""#, br#":1}"#], &hp(&[])).await;
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("\"a\""));
}

#[tokio::test]
async fn json_pretty_invalid() {
    let err = run_one_err(&StreamingJsonPrettyPrint, vec![b"not json"], &hp(&[])).await;
    assert!(err.contains("json"));
}

#[tokio::test]
async fn json_pretty_roundtrip() {
    let pretty = run_one(&StreamingJsonPrettyPrint, vec![br#"{"x":42}"#], &hp(&[])).await;
    let mini = run_one(&StreamingJsonMinify, vec![pretty.as_ref()], &hp(&[])).await;
    assert_eq!(mini.as_ref(), br#"{"x":42}"#);
}

// ═══════════════════════════════════════════════════════════════════════
// #35 StreamingJsonMinify
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn json_minify_strips_whitespace() {
    let out = run_one(&StreamingJsonMinify, vec![b"{ \"a\" : 1 }"], &hp(&[])).await;
    assert_eq!(out.as_ref(), br#"{"a":1}"#);
}

#[tokio::test]
async fn json_minify_already_compact() {
    let out = run_one(&StreamingJsonMinify, vec![br#"{"a":1}"#], &hp(&[])).await;
    assert_eq!(out.as_ref(), br#"{"a":1}"#);
}

#[tokio::test]
async fn json_minify_array() {
    let out = run_one(&StreamingJsonMinify, vec![b"[ 1 , 2 , 3 ]"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"[1,2,3]");
}

#[tokio::test]
async fn json_minify_invalid() {
    let err = run_one_err(&StreamingJsonMinify, vec![b"{broken"], &hp(&[])).await;
    assert!(err.contains("json"));
}

#[tokio::test]
async fn json_minify_empty_object() {
    let out = run_one(&StreamingJsonMinify, vec![b"  {  }  "], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"{}");
}

// ═══════════════════════════════════════════════════════════════════════
// #36 StreamingJsonLines
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn json_lines_basic() {
    let out = run_one(&StreamingJsonLines, vec![br#"[{"a":1},{"b":2}]"#], &hp(&[])).await;
    let s = String::from_utf8_lossy(&out);
    let lines: Vec<&str> = s.trim().split('\n').collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], r#"{"a":1}"#);
    assert_eq!(lines[1], r#"{"b":2}"#);
}

#[tokio::test]
async fn json_lines_single_element() {
    let out = run_one(&StreamingJsonLines, vec![b"[42]"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"42\n");
}

#[tokio::test]
async fn json_lines_empty_array() {
    let out = run_one(&StreamingJsonLines, vec![b"[]"], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn json_lines_not_array() {
    let err = run_one_err(&StreamingJsonLines, vec![br#"{"a":1}"#], &hp(&[])).await;
    assert!(err.contains("array"));
}

#[tokio::test]
async fn json_lines_multi_chunk() {
    let out = run_one(&StreamingJsonLines, vec![b"[1,", b"2,3]"], &hp(&[])).await;
    let s = String::from_utf8_lossy(&out);
    assert_eq!(s.trim().split('\n').count(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// #37 StreamingCsvToJson
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn csv_to_json_basic() {
    let csv = b"name,age\nAlice,30\nBob,25\n";
    let out = run_one(&StreamingCsvToJson, vec![csv], &hp(&[])).await;
    let lines: Vec<serde_json::Value> = String::from_utf8_lossy(&out).lines()
        .filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["name"], "Alice");
    assert_eq!(lines[0]["age"], "30");
}

#[tokio::test]
async fn csv_to_json_single_row() {
    let csv = b"x\n42\n";
    let out = run_one(&StreamingCsvToJson, vec![csv], &hp(&[])).await;
    let lines: Vec<serde_json::Value> = String::from_utf8_lossy(&out).lines()
        .filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["x"], "42");
}

#[tokio::test]
async fn csv_to_json_multi_chunk() {
    let out = run_one(&StreamingCsvToJson, vec![b"a,b\n1,", b"2\n"], &hp(&[])).await;
    let lines: Vec<serde_json::Value> = String::from_utf8_lossy(&out).lines()
        .filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["a"], "1");
    assert_eq!(lines[0]["b"], "2");
}

#[tokio::test]
async fn csv_to_json_empty_body() {
    let csv = b"col1,col2\n";
    let out = run_one(&StreamingCsvToJson, vec![csv], &hp(&[])).await;
    let text = String::from_utf8_lossy(&out);
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 0);
}

#[tokio::test]
async fn csv_to_json_quoted_fields() {
    let csv = b"name,desc\n\"A,B\",\"hello\"\n";
    let out = run_one(&StreamingCsvToJson, vec![csv], &hp(&[])).await;
    let lines: Vec<serde_json::Value> = String::from_utf8_lossy(&out).lines()
        .filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines[0]["name"], "A,B");
}


// ═══════════════════════════════════════════════════════════════════════
// #38 StreamingBase32Encode
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn base32_encode_basic() {
    let out = run_one(&StreamingBase32Encode, vec![b"hello"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"NBSWY3DP");
}

#[tokio::test]
async fn base32_encode_empty() {
    let out = run_one(&StreamingBase32Encode, vec![b""], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn base32_encode_expands() {
    let out = run_one(&StreamingBase32Encode, vec![b"test"], &hp(&[])).await;
    assert!(out.len() > 4); // base32 expands ~1.6×
}

#[tokio::test]
async fn base32_encode_multi_chunk() {
    // Each chunk encoded independently
    let out = run_one(&StreamingBase32Encode, vec![b"he", b"llo"], &hp(&[])).await;
    assert!(out.len() > 0);
}

#[tokio::test]
async fn base32_encode_binary() {
    let out = run_one(&StreamingBase32Encode, vec![&[0u8, 255, 128]], &hp(&[])).await;
    assert!(out.len() > 0);
    // Should be valid ASCII
    assert!(out.iter().all(|&b| b.is_ascii()));
}

// ═══════════════════════════════════════════════════════════════════════
// #39 StreamingBase32Decode
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn base32_decode_basic() {
    let out = run_one(&StreamingBase32Decode, vec![b"NBSWY3DP"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"hello");
}

#[tokio::test]
async fn base32_decode_roundtrip() {
    let enc = run_one(&StreamingBase32Encode, vec![b"roundtrip"], &hp(&[])).await;
    let dec = run_one(&StreamingBase32Decode, vec![enc.as_ref()], &hp(&[])).await;
    assert_eq!(dec.as_ref(), b"roundtrip");
}

#[tokio::test]
async fn base32_decode_empty() {
    let out = run_one(&StreamingBase32Decode, vec![b""], &hp(&[])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn base32_decode_invalid() {
    let err = run_one_err(&StreamingBase32Decode, vec![b"!!!"], &hp(&[])).await;
    assert!(err.contains("base32"));
}

#[tokio::test]
async fn base32_decode_padded() {
    // "f" encodes to "MY======" in base32
    let out = run_one(&StreamingBase32Decode, vec![b"MY======"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"f");
}
