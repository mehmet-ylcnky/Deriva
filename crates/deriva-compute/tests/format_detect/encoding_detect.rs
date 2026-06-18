use bytes::Bytes;
use deriva_compute::builtins_format_detect::EncodingDetectFn;
use deriva_compute::function::ComputeFunction;
use std::collections::BTreeMap;

fn detect_encoding(input: &[u8]) -> serde_json::Value {
    let result = EncodingDetectFn
        .execute(vec![Bytes::from(input.to_vec())], &BTreeMap::new())
        .unwrap();
    serde_json::from_slice(&result).unwrap()
}

#[test]
fn encoding_detect_ascii_text() {
    let data = b"Hello, world! This is plain ASCII text.";
    let r = detect_encoding(data);
    let enc = r["encoding"].as_str().unwrap();
    assert!(enc == "ASCII" || enc == "UTF-8", "expected ASCII or UTF-8, got {}", enc);
    assert!(r["confidence"].as_f64().unwrap() > 0.9);
}

#[test]
fn encoding_detect_utf8_with_bom() {
    let data = vec![0xEF, 0xBB, 0xBF, b'h', b'e', b'l', b'l', b'o'];
    let r = detect_encoding(&data);
    assert_eq!(r["encoding"], "UTF-8");
    assert!(r["confidence"].as_f64().unwrap() >= 0.99);
}

#[test]
fn encoding_detect_utf8_multibyte() {
    // "héllo" in UTF-8
    let data = "héllo wörld".as_bytes();
    let r = detect_encoding(data);
    assert_eq!(r["encoding"], "UTF-8");
    assert!(r["confidence"].as_f64().unwrap() > 0.9);
}

#[test]
fn encoding_detect_utf16le_with_bom() {
    // UTF-16LE BOM followed by "Hi"
    let data = vec![0xFF, 0xFE, b'H', 0x00, b'i', 0x00];
    let r = detect_encoding(&data);
    assert_eq!(r["encoding"], "UTF-16LE");
    assert!(r["confidence"].as_f64().unwrap() >= 0.99);
}

#[test]
fn encoding_detect_utf16be_with_bom() {
    // UTF-16BE BOM followed by "Hi"
    let data = vec![0xFE, 0xFF, 0x00, b'H', 0x00, b'i'];
    let r = detect_encoding(&data);
    assert_eq!(r["encoding"], "UTF-16BE");
    assert!(r["confidence"].as_f64().unwrap() >= 0.99);
}

#[test]
fn encoding_detect_latin1() {
    // Latin-1 text with bytes in 0x80-0xFF range that are NOT valid UTF-8 sequences
    // 0xE9 alone is not a valid UTF-8 start byte followed by continuation bytes
    let mut data = b"caf".to_vec();
    data.push(0xE9); // 'é' in Latin-1
    // Make it invalid UTF-8 by not following up with a continuation byte
    data.extend_from_slice(b" cr");
    data.push(0xE8); // 'è' in Latin-1
    data.extend_from_slice(b"me");
    let r = detect_encoding(&data);
    assert_eq!(r["encoding"], "Latin-1");
    assert!(r["confidence"].as_f64().unwrap() > 0.0);
}

#[test]
fn encoding_detect_empty_input_errors() {
    let result = EncodingDetectFn.execute(vec![Bytes::new()], &BTreeMap::new());
    assert!(result.is_err());
}

#[test]
fn encoding_detect_no_input_errors() {
    let result = EncodingDetectFn.execute(vec![], &BTreeMap::new());
    assert!(result.is_err());
}
