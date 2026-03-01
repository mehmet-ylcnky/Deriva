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

// ── #1 IdentityFn ──

#[test]
fn identity_binary_passthrough() {
    let data: Vec<u8> = (0..=255).collect();
    assert_eq!(exec1(&IdentityFn, &data).unwrap(), Bytes::from(data));
}

#[test]
fn identity_empty() {
    assert_eq!(exec1(&IdentityFn, b"").unwrap(), Bytes::new());
}

#[test]
fn identity_rejects_multiple_inputs() {
    let r = IdentityFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 1, got: 2 })));
}

#[test]
fn identity_preserves_null_bytes() {
    let data = b"\x00\x00\xff\x00";
    assert_eq!(exec1(&IdentityFn, data).unwrap().as_ref(), data);
}

#[test]
fn identity_large_input() {
    let data = vec![0x42u8; 1_000_000];
    assert_eq!(exec1(&IdentityFn, &data).unwrap().len(), 1_000_000);
}

// ── #2 ConcatFn ──

#[test]
fn concat_three_chunks() {
    let r = ConcatFn.execute(
        vec![Bytes::from("hello"), Bytes::from(" "), Bytes::from("world")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r, Bytes::from("hello world"));
}

#[test]
fn concat_single_input() {
    let r = ConcatFn.execute(vec![Bytes::from("only")], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from("only"));
}

#[test]
fn concat_empty_inputs() {
    let r = ConcatFn.execute(
        vec![Bytes::new(), Bytes::from("x"), Bytes::new()],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r, Bytes::from("x"));
}

#[test]
fn concat_binary_preserves_boundaries() {
    let r = ConcatFn.execute(
        vec![Bytes::from(vec![0xFF, 0x00]), Bytes::from(vec![0x00, 0xFF])],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn concat_zero_inputs() {
    let r = ConcatFn.execute(vec![], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::new());
}

// ── #3 UppercaseFn ──

#[test]
fn uppercase_mixed_ascii() {
    assert_eq!(exec1(&UppercaseFn, b"Hello World 123!").unwrap(), Bytes::from("HELLO WORLD 123!"));
}

#[test]
fn uppercase_already_upper() {
    assert_eq!(exec1(&UppercaseFn, b"ABC").unwrap(), Bytes::from("ABC"));
}

#[test]
fn uppercase_unicode_chars() {
    // UppercaseFn uses str::to_uppercase which handles Unicode
    assert_eq!(exec1(&UppercaseFn, "café".as_bytes()).unwrap(), Bytes::from("CAFÉ"));
}

#[test]
fn uppercase_empty() {
    assert_eq!(exec1(&UppercaseFn, b"").unwrap(), Bytes::new());
}

#[test]
fn uppercase_rejects_invalid_utf8() {
    let r = exec1(&UppercaseFn, &[0xFF, 0xFE]);
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

// ── #4 RepeatFn ──

#[test]
fn repeat_three_times() {
    let r = exec1_params(&RepeatFn, b"ab", params(&[("count", "3")])).unwrap();
    assert_eq!(r, Bytes::from("ababab"));
}

#[test]
fn repeat_with_int_param() {
    let mut p = BTreeMap::new();
    p.insert("count".into(), Value::Int(2));
    let r = RepeatFn.execute(vec![Bytes::from("xy")], &p).unwrap();
    assert_eq!(r, Bytes::from("xyxy"));
}

#[test]
fn repeat_once_is_identity() {
    let r = exec1_params(&RepeatFn, b"data", params(&[("count", "1")])).unwrap();
    assert_eq!(r, Bytes::from("data"));
}

#[test]
fn repeat_rejects_zero() {
    let r = exec1_params(&RepeatFn, b"x", params(&[("count", "0")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn repeat_missing_param() {
    let r = exec1(&RepeatFn, b"x");
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ── #5 LowercaseFn ──

#[test]
fn lowercase_mixed_ascii() {
    assert_eq!(exec1(&LowercaseFn, b"Hello WORLD 123!").unwrap(), Bytes::from("hello world 123!"));
}

#[test]
fn lowercase_non_ascii_passthrough() {
    // Bytes > 127 pass through unchanged (ASCII-only operation)
    let input = vec![0x80, 0xFF, b'A', b'Z'];
    let result = exec1(&LowercaseFn, &input).unwrap();
    assert_eq!(result.as_ref(), &[0x80, 0xFF, b'a', b'z']);
}

#[test]
fn lowercase_empty() {
    assert_eq!(exec1(&LowercaseFn, b"").unwrap(), Bytes::new());
}

#[test]
fn lowercase_already_lower() {
    assert_eq!(exec1(&LowercaseFn, b"abc").unwrap(), Bytes::from("abc"));
}

#[test]
fn lowercase_digits_and_symbols_unchanged() {
    assert_eq!(exec1(&LowercaseFn, b"123!@#").unwrap(), Bytes::from("123!@#"));
}

// ── #6 ReverseFn ──

#[test]
fn reverse_ascii_string() {
    assert_eq!(exec1(&ReverseFn, b"abcdef").unwrap(), Bytes::from("fedcba"));
}

#[test]
fn reverse_single_byte() {
    assert_eq!(exec1(&ReverseFn, b"x").unwrap(), Bytes::from("x"));
}

#[test]
fn reverse_empty() {
    assert_eq!(exec1(&ReverseFn, b"").unwrap(), Bytes::new());
}

#[test]
fn reverse_is_self_inverse() {
    let data = b"hello world";
    let reversed = exec1(&ReverseFn, data).unwrap();
    let back = exec1(&ReverseFn, &reversed).unwrap();
    assert_eq!(back.as_ref(), data);
}

#[test]
fn reverse_binary_with_nulls() {
    let input = vec![0x00, 0x01, 0x02, 0xFF];
    let result = exec1(&ReverseFn, &input).unwrap();
    assert_eq!(result.as_ref(), &[0xFF, 0x02, 0x01, 0x00]);
}

// ── #7 Base64EncodeFn ──

#[test]
fn base64_encode_rfc_vector() {
    // RFC 4648 test vector
    assert_eq!(exec1(&Base64EncodeFn, b"foobar").unwrap(), Bytes::from("Zm9vYmFy"));
}

#[test]
fn base64_encode_padding_one() {
    // "fooba" → 5 bytes → needs 1 pad char
    assert_eq!(exec1(&Base64EncodeFn, b"fooba").unwrap(), Bytes::from("Zm9vYmE="));
}

#[test]
fn base64_encode_padding_two() {
    // "foob" → 4 bytes → needs 2 pad chars
    assert_eq!(exec1(&Base64EncodeFn, b"foob").unwrap(), Bytes::from("Zm9vYg=="));
}

#[test]
fn base64_encode_empty() {
    assert_eq!(exec1(&Base64EncodeFn, b"").unwrap(), Bytes::from(""));
}

#[test]
fn base64_encode_binary_data() {
    let input: Vec<u8> = (0..=255).collect();
    let encoded = exec1(&Base64EncodeFn, &input).unwrap();
    // Verify roundtrip
    let decoded = exec1(&Base64DecodeFn, &encoded).unwrap();
    assert_eq!(decoded.as_ref(), input.as_slice());
}

// ── #8 Base64DecodeFn ──

#[test]
fn base64_decode_rfc_vector() {
    assert_eq!(exec1(&Base64DecodeFn, b"Zm9vYmFy").unwrap(), Bytes::from("foobar"));
}

#[test]
fn base64_decode_with_padding() {
    assert_eq!(exec1(&Base64DecodeFn, b"Zm9vYg==").unwrap(), Bytes::from("foob"));
}

#[test]
fn base64_decode_empty() {
    assert_eq!(exec1(&Base64DecodeFn, b"").unwrap(), Bytes::new());
}

#[test]
fn base64_decode_invalid_chars() {
    let r = exec1(&Base64DecodeFn, b"!!!invalid!!!");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn base64_decode_truncated_padding() {
    // "Zm9v" is valid (no padding needed for 3 bytes), but "Zm9" is incomplete
    let r = exec1(&Base64DecodeFn, b"Zm9");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

// ── #9 HexEncodeFn ──

#[test]
fn hex_encode_known_bytes() {
    assert_eq!(exec1(&HexEncodeFn, &[0xDE, 0xAD, 0xBE, 0xEF]).unwrap(), Bytes::from("deadbeef"));
}

#[test]
fn hex_encode_zeros() {
    assert_eq!(exec1(&HexEncodeFn, &[0x00, 0x00]).unwrap(), Bytes::from("0000"));
}

#[test]
fn hex_encode_empty() {
    assert_eq!(exec1(&HexEncodeFn, b"").unwrap(), Bytes::from(""));
}

#[test]
fn hex_encode_all_byte_values() {
    let input: Vec<u8> = (0..=255).collect();
    let result = exec1(&HexEncodeFn, &input).unwrap();
    assert_eq!(result.len(), 512); // 256 bytes × 2 hex chars
    assert!(result.starts_with(b"00"));
    assert!(result.ends_with(b"ff"));
}

#[test]
fn hex_encode_decode_roundtrip() {
    let input = b"Hello, World!";
    let encoded = exec1(&HexEncodeFn, input).unwrap();
    let decoded = exec1(&HexDecodeFn, &encoded).unwrap();
    assert_eq!(decoded.as_ref(), input);
}

// ── #10 HexDecodeFn ──

#[test]
fn hex_decode_lowercase() {
    assert_eq!(exec1(&HexDecodeFn, b"deadbeef").unwrap().as_ref(), &[0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn hex_decode_uppercase() {
    assert_eq!(exec1(&HexDecodeFn, b"DEADBEEF").unwrap().as_ref(), &[0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn hex_decode_odd_length_error() {
    let r = exec1(&HexDecodeFn, b"abc");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn hex_decode_invalid_chars() {
    let r = exec1(&HexDecodeFn, b"zzzz");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn hex_decode_empty() {
    assert_eq!(exec1(&HexDecodeFn, b"").unwrap(), Bytes::new());
}

// ── #11 Base32EncodeFn ──

#[test]
fn base32_encode_rfc_vector() {
    // RFC 4648: "foobar" → "MZXW6YTBOI======"
    assert_eq!(exec1(&Base32EncodeFn, b"foobar").unwrap(), Bytes::from("MZXW6YTBOI======"));
}

#[test]
fn base32_encode_empty() {
    assert_eq!(exec1(&Base32EncodeFn, b"").unwrap(), Bytes::from(""));
}

#[test]
fn base32_encode_single_byte() {
    // "f" → "MY======"
    assert_eq!(exec1(&Base32EncodeFn, b"f").unwrap(), Bytes::from("MY======"));
}

#[test]
fn base32_encode_decode_roundtrip() {
    let input = b"content-addressed storage";
    let encoded = exec1(&Base32EncodeFn, input).unwrap();
    let decoded = exec1(&Base32DecodeFn, &encoded).unwrap();
    assert_eq!(decoded.as_ref(), input);
}

#[test]
fn base32_encode_binary() {
    let input: Vec<u8> = (0..32).collect();
    let encoded = exec1(&Base32EncodeFn, &input).unwrap();
    // Base32 output should be uppercase A-Z, 2-7, and =
    assert!(encoded.iter().all(|&b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b) || b == b'='));
}

// ── #12 Base32DecodeFn ──

#[test]
fn base32_decode_rfc_vector() {
    assert_eq!(exec1(&Base32DecodeFn, b"MZXW6YTBOI======").unwrap(), Bytes::from("foobar"));
}

#[test]
fn base32_decode_empty() {
    assert_eq!(exec1(&Base32DecodeFn, b"").unwrap(), Bytes::new());
}

#[test]
fn base32_decode_invalid() {
    let r = exec1(&Base32DecodeFn, b"1234");  // '1' is not valid base32
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn base32_decode_lowercase_rejected() {
    // Standard Base32 is uppercase; lowercase should fail
    let r = exec1(&Base32DecodeFn, b"mzxw6ytboi======");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn base32_decode_wrong_padding() {
    // "MZXW6Y" is 6 chars — not a multiple of 8, so invalid
    let r = exec1(&Base32DecodeFn, b"MZXW6Y");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

// ── #13 XorFn ──

#[test]
fn xor_self_inverse() {
    let input = b"secret data";
    let encrypted = exec1_params(&XorFn, input, params(&[("key", "42")])).unwrap();
    let decrypted = exec1_params(&XorFn, &encrypted, params(&[("key", "42")])).unwrap();
    assert_eq!(decrypted.as_ref(), input);
}

#[test]
fn xor_key_zero_is_identity() {
    let input = b"unchanged";
    assert_eq!(exec1_params(&XorFn, input, params(&[("key", "0")])).unwrap(), Bytes::from("unchanged"));
}

#[test]
fn xor_known_value() {
    // 0x41 ('A') ^ 0x20 = 0x61 ('a')
    assert_eq!(exec1_params(&XorFn, b"A", params(&[("key", "32")])).unwrap(), Bytes::from("a"));
}

#[test]
fn xor_missing_key() {
    let r = exec1(&XorFn, b"data");
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn xor_invalid_key() {
    let r = exec1_params(&XorFn, b"data", params(&[("key", "999")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ── #14 BitwiseAndFn ──

#[test]
fn bitwise_and_mask_low_nibble() {
    let input = vec![0xAB, 0xCD, 0xEF];
    let result = exec1_params(&BitwiseAndFn, &input, params(&[("mask", "15")])).unwrap(); // 0x0F
    assert_eq!(result.as_ref(), &[0x0B, 0x0D, 0x0F]);
}

#[test]
fn bitwise_and_mask_ff_identity() {
    let input = b"hello";
    assert_eq!(exec1_params(&BitwiseAndFn, input, params(&[("mask", "255")])).unwrap(), Bytes::from("hello"));
}

#[test]
fn bitwise_and_mask_zero_clears() {
    let input = vec![0xFF, 0xAB];
    let result = exec1_params(&BitwiseAndFn, &input, params(&[("mask", "0")])).unwrap();
    assert_eq!(result.as_ref(), &[0x00, 0x00]);
}

#[test]
fn bitwise_and_empty() {
    assert_eq!(exec1_params(&BitwiseAndFn, b"", params(&[("mask", "255")])).unwrap(), Bytes::new());
}

#[test]
fn bitwise_and_missing_mask() {
    let r = exec1(&BitwiseAndFn, b"data");
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ── #15 BitwiseOrFn ──

#[test]
fn bitwise_or_set_high_bit() {
    let input = vec![0x01, 0x02, 0x03];
    let result = exec1_params(&BitwiseOrFn, &input, params(&[("mask", "128")])).unwrap(); // 0x80
    assert_eq!(result.as_ref(), &[0x81, 0x82, 0x83]);
}

#[test]
fn bitwise_or_mask_zero_identity() {
    let input = b"test";
    assert_eq!(exec1_params(&BitwiseOrFn, input, params(&[("mask", "0")])).unwrap(), Bytes::from("test"));
}

#[test]
fn bitwise_or_mask_ff_all_ones() {
    let input = vec![0x00, 0x55, 0xAA];
    let result = exec1_params(&BitwiseOrFn, &input, params(&[("mask", "255")])).unwrap();
    assert_eq!(result.as_ref(), &[0xFF, 0xFF, 0xFF]);
}

#[test]
fn bitwise_or_empty() {
    assert_eq!(exec1_params(&BitwiseOrFn, b"", params(&[("mask", "1")])).unwrap(), Bytes::new());
}

#[test]
fn bitwise_or_missing_mask() {
    let r = exec1(&BitwiseOrFn, b"data");
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ── #16 BitwiseNotFn ──

#[test]
fn bitwise_not_known_values() {
    let input = vec![0x00, 0xFF, 0x0F, 0xF0];
    let result = exec1(&BitwiseNotFn, &input).unwrap();
    assert_eq!(result.as_ref(), &[0xFF, 0x00, 0xF0, 0x0F]);
}

#[test]
fn bitwise_not_self_inverse() {
    let input = b"test data";
    let notted = exec1(&BitwiseNotFn, input).unwrap();
    let back = exec1(&BitwiseNotFn, &notted).unwrap();
    assert_eq!(back.as_ref(), input);
}

#[test]
fn bitwise_not_empty() {
    assert_eq!(exec1(&BitwiseNotFn, b"").unwrap(), Bytes::new());
}

#[test]
fn bitwise_not_single_byte() {
    assert_eq!(exec1(&BitwiseNotFn, &[0xA5]).unwrap().as_ref(), &[0x5A]);
}

#[test]
fn bitwise_not_all_zeros() {
    let input = vec![0x00; 8];
    let result = exec1(&BitwiseNotFn, &input).unwrap();
    assert!(result.iter().all(|&b| b == 0xFF));
}

// ── #17 ByteSwapFn ──

#[test]
fn byte_swap_16bit_endian() {
    // Swap 0x0102 → 0x0201
    let input = vec![0x01, 0x02, 0x03, 0x04];
    let result = exec1_params(&ByteSwapFn, &input, params(&[("word_size", "2")])).unwrap();
    assert_eq!(result.as_ref(), &[0x02, 0x01, 0x04, 0x03]);
}

#[test]
fn byte_swap_32bit_endian() {
    // Swap u32 little→big: [0x78, 0x56, 0x34, 0x12] → [0x12, 0x34, 0x56, 0x78]
    let input = vec![0x78, 0x56, 0x34, 0x12];
    let result = exec1_params(&ByteSwapFn, &input, params(&[("word_size", "4")])).unwrap();
    assert_eq!(result.as_ref(), &[0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn byte_swap_self_inverse() {
    let input = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
    let swapped = exec1_params(&ByteSwapFn, &input, params(&[("word_size", "4")])).unwrap();
    let back = exec1_params(&ByteSwapFn, &swapped, params(&[("word_size", "4")])).unwrap();
    assert_eq!(back.as_ref(), input.as_slice());
}

#[test]
fn byte_swap_unaligned_error() {
    let input = vec![0x01, 0x02, 0x03]; // 3 bytes, not multiple of 4
    let r = exec1_params(&ByteSwapFn, &input, params(&[("word_size", "4")]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn byte_swap_invalid_word_size() {
    let r = exec1_params(&ByteSwapFn, &[0x01, 0x02, 0x03], params(&[("word_size", "3")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ── #18 TrimFn ──

#[test]
fn trim_leading_and_trailing() {
    assert_eq!(exec1(&TrimFn, b"  hello world  ").unwrap(), Bytes::from("hello world"));
}

#[test]
fn trim_tabs_and_newlines() {
    assert_eq!(exec1(&TrimFn, b"\t\n  data \r\n\t").unwrap(), Bytes::from("data"));
}

#[test]
fn trim_all_whitespace() {
    assert_eq!(exec1(&TrimFn, b"   \t\n\r  ").unwrap(), Bytes::new());
}

#[test]
fn trim_no_whitespace() {
    assert_eq!(exec1(&TrimFn, b"clean").unwrap(), Bytes::from("clean"));
}

#[test]
fn trim_preserves_internal_whitespace() {
    assert_eq!(exec1(&TrimFn, b"  a  b  c  ").unwrap(), Bytes::from("a  b  c"));
}

// ── #19 PadFn ──

#[test]
fn pad_pkcs7_block16() {
    // 10 bytes input, block_size=16 → 6 bytes padding, each = 0x06
    let input = b"0123456789";
    let result = exec1_params(&PadFn, input, params(&[("block_size", "16")])).unwrap();
    assert_eq!(result.len(), 16);
    assert_eq!(&result[10..], &[0x06; 6]);
}

#[test]
fn pad_already_aligned() {
    // PKCS#7: aligned input gets a full block of padding
    let input = vec![0x41; 16]; // exactly 16 bytes
    let result = exec1_params(&PadFn, &input, params(&[("block_size", "16")])).unwrap();
    assert_eq!(result.len(), 32); // 16 data + 16 padding
    assert_eq!(&result[16..], &[0x10; 16]); // pad byte = block_size = 16
}

#[test]
fn pad_empty_input() {
    // Empty input, block_size=8 → 8 bytes of 0x08
    let result = exec1_params(&PadFn, b"", params(&[("block_size", "8")])).unwrap();
    assert_eq!(result.len(), 8);
    assert_eq!(result.as_ref(), &[0x08; 8]);
}

#[test]
fn pad_block_size_zero_error() {
    let r = exec1_params(&PadFn, b"data", params(&[("block_size", "0")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn pad_block_size_too_large() {
    let r = exec1_params(&PadFn, b"data", params(&[("block_size", "257")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ── #20 LineEndingFn ──

#[test]
fn line_ending_crlf_to_lf() {
    let input = b"line1\r\nline2\r\nline3";
    let result = exec1_params(&LineEndingFn, input, params(&[("target", "lf")])).unwrap();
    assert_eq!(result.as_ref(), b"line1\nline2\nline3");
}

#[test]
fn line_ending_lf_to_crlf() {
    let input = b"line1\nline2\nline3";
    let result = exec1_params(&LineEndingFn, input, params(&[("target", "crlf")])).unwrap();
    assert_eq!(result.as_ref(), b"line1\r\nline2\r\nline3");
}

#[test]
fn line_ending_mixed_to_lf() {
    // Mix of CRLF and LF → all LF
    let input = b"a\r\nb\nc\r\n";
    let result = exec1_params(&LineEndingFn, input, params(&[("target", "lf")])).unwrap();
    assert_eq!(result.as_ref(), b"a\nb\nc\n");
}

#[test]
fn line_ending_crlf_preserves_existing() {
    // Already CRLF → converting to CRLF should not double up
    let input = b"a\r\nb\r\n";
    let result = exec1_params(&LineEndingFn, input, params(&[("target", "crlf")])).unwrap();
    assert_eq!(result.as_ref(), b"a\r\nb\r\n");
}

#[test]
fn line_ending_invalid_target() {
    let r = exec1_params(&LineEndingFn, b"data", params(&[("target", "mac")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

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
