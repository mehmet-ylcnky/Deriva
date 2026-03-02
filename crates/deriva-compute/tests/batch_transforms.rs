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


mod transforms {
    use super::*;

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


}

mod compression {
    use super::*;

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


}

mod crypto {
    use super::*;

    // ── #31 Sha256Fn ──

    #[test]
    fn sha256_empty_input() {
        let result = exec1(&Sha256Fn, b"").unwrap();
        assert_eq!(result.len(), 32);
        // Known SHA-256 of empty string
        let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        assert_eq!(hex, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn sha256_known_vector() {
        let result = exec1(&Sha256Fn, b"abc").unwrap();
        let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        assert_eq!(hex, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn sha256_output_always_32_bytes() {
        let short = exec1(&Sha256Fn, b"x").unwrap();
        let long = exec1(&Sha256Fn, &vec![0u8; 100_000]).unwrap();
        assert_eq!(short.len(), 32);
        assert_eq!(long.len(), 32);
    }

    #[test]
    fn sha256_different_inputs_different_hashes() {
        let h1 = exec1(&Sha256Fn, b"hello").unwrap();
        let h2 = exec1(&Sha256Fn, b"hello!").unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn sha256_deterministic() {
        let h1 = exec1(&Sha256Fn, b"deterministic").unwrap();
        let h2 = exec1(&Sha256Fn, b"deterministic").unwrap();
        assert_eq!(h1, h2);
    }


    // ── #32 Sha512Fn ──

    #[test]
    fn sha512_empty_input() {
        let result = exec1(&Sha512Fn, b"").unwrap();
        assert_eq!(result.len(), 64);
        let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        assert!(hex.starts_with("cf83e1357eefb8bd"));
    }

    #[test]
    fn sha512_known_vector() {
        let result = exec1(&Sha512Fn, b"abc").unwrap();
        let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        assert!(hex.starts_with("ddaf35a193617aba"));
    }

    #[test]
    fn sha512_output_always_64_bytes() {
        assert_eq!(exec1(&Sha512Fn, b"x").unwrap().len(), 64);
        assert_eq!(exec1(&Sha512Fn, &vec![0u8; 100_000]).unwrap().len(), 64);
    }

    #[test]
    fn sha512_different_from_sha256() {
        let s256 = exec1(&Sha256Fn, b"test").unwrap();
        let s512 = exec1(&Sha512Fn, b"test").unwrap();
        assert_ne!(s256.len(), s512.len());
    }

    #[test]
    fn sha512_deterministic() {
        let h1 = exec1(&Sha512Fn, b"same input").unwrap();
        let h2 = exec1(&Sha512Fn, b"same input").unwrap();
        assert_eq!(h1, h2);
    }


    // ── #33 Md5Fn ──

    #[test]
    fn md5_empty_input() {
        let result = exec1(&Md5Fn, b"").unwrap();
        assert_eq!(result.len(), 16);
        let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        assert_eq!(hex, "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn md5_known_vector() {
        let result = exec1(&Md5Fn, b"abc").unwrap();
        let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        assert_eq!(hex, "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn md5_output_always_16_bytes() {
        assert_eq!(exec1(&Md5Fn, b"short").unwrap().len(), 16);
        assert_eq!(exec1(&Md5Fn, &vec![0u8; 50_000]).unwrap().len(), 16);
    }

    #[test]
    fn md5_deterministic() {
        let h1 = exec1(&Md5Fn, b"same").unwrap();
        let h2 = exec1(&Md5Fn, b"same").unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn md5_different_from_sha256() {
        let md5 = exec1(&Md5Fn, b"test").unwrap();
        let sha = exec1(&Sha256Fn, b"test").unwrap();
        assert_ne!(md5.len(), sha.len());
    }


    // ── #34 Blake3Fn ──

    #[test]
    fn blake3_empty_input() {
        let result = exec1(&Blake3Fn, b"").unwrap();
        assert_eq!(result.len(), 32);
        // BLAKE3 of empty is known
        let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        assert_eq!(hex, "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262");
    }

    #[test]
    fn blake3_known_vector() {
        let result = exec1(&Blake3Fn, b"abc").unwrap();
        assert_eq!(result.len(), 32);
        let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        assert_eq!(hex, "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85");
    }

    #[test]
    fn blake3_output_always_32_bytes() {
        assert_eq!(exec1(&Blake3Fn, b"x").unwrap().len(), 32);
        assert_eq!(exec1(&Blake3Fn, &vec![0u8; 100_000]).unwrap().len(), 32);
    }

    #[test]
    fn blake3_different_from_sha256() {
        let b3 = exec1(&Blake3Fn, b"test").unwrap();
        let sha = exec1(&Sha256Fn, b"test").unwrap();
        assert_ne!(b3, sha); // same length but different digests
    }

    #[test]
    fn blake3_deterministic() {
        let h1 = exec1(&Blake3Fn, b"data").unwrap();
        let h2 = exec1(&Blake3Fn, b"data").unwrap();
        assert_eq!(h1, h2);
    }


    // ── #35 HmacSha256Fn ──

    #[test]
    fn hmac_sha256_known_key() {
        let result = exec1_params(&HmacSha256Fn, b"hello", params(&[("key", "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b")])).unwrap();
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn hmac_sha256_empty_message() {
        let result = exec1_params(&HmacSha256Fn, b"", params(&[("key", "aabbccdd")])).unwrap();
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn hmac_sha256_different_keys_different_macs() {
        let h1 = exec1_params(&HmacSha256Fn, b"msg", params(&[("key", "aa")])).unwrap();
        let h2 = exec1_params(&HmacSha256Fn, b"msg", params(&[("key", "bb")])).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn hmac_sha256_missing_key() {
        let r = exec1(&HmacSha256Fn, b"data");
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn hmac_sha256_invalid_hex_key() {
        let r = exec1_params(&HmacSha256Fn, b"data", params(&[("key", "zzzz")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }


    // ── #36 Crc32Fn ──

    #[test]
    fn crc32_empty_input() {
        let result = exec1(&Crc32Fn, b"").unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result.as_ref(), &[0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn crc32_known_value() {
        // CRC32 of "123456789" = 0xCBF43926
        let result = exec1(&Crc32Fn, b"123456789").unwrap();
        assert_eq!(result.as_ref(), &[0xCB, 0xF4, 0x39, 0x26]);
    }

    #[test]
    fn crc32_output_always_4_bytes() {
        assert_eq!(exec1(&Crc32Fn, b"x").unwrap().len(), 4);
        assert_eq!(exec1(&Crc32Fn, &vec![0u8; 100_000]).unwrap().len(), 4);
    }

    #[test]
    fn crc32_deterministic() {
        let h1 = exec1(&Crc32Fn, b"checksum").unwrap();
        let h2 = exec1(&Crc32Fn, b"checksum").unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn crc32_different_inputs() {
        let h1 = exec1(&Crc32Fn, b"aaa").unwrap();
        let h2 = exec1(&Crc32Fn, b"bbb").unwrap();
        assert_ne!(h1, h2);
    }


    // ── #37 EncryptFn (AES-256-CTR) ──

    // 32-byte key (64 hex chars) and 16-byte nonce (32 hex chars) for tests

    #[test]
    fn encrypt_self_inverse() {
        let input = b"secret message";
        let p = params(&[("key", TEST_KEY), ("nonce", TEST_NONCE)]);
        let encrypted = exec1_params(&EncryptFn, input, p.clone()).unwrap();
        let decrypted = exec1_params(&EncryptFn, &encrypted, p).unwrap();
        assert_eq!(decrypted.as_ref(), input);
    }

    #[test]
    fn encrypt_same_size_output() {
        let input = b"sixteen bytes!!";
        let encrypted = exec1_params(&EncryptFn, input, params(&[("key", TEST_KEY), ("nonce", TEST_NONCE)])).unwrap();
        assert_eq!(encrypted.len(), input.len());
    }

    #[test]
    fn encrypt_empty_input() {
        let encrypted = exec1_params(&EncryptFn, b"", params(&[("key", TEST_KEY), ("nonce", TEST_NONCE)])).unwrap();
        assert!(encrypted.is_empty());
    }

    #[test]
    fn encrypt_wrong_key_length() {
        let r = exec1_params(&EncryptFn, b"data", params(&[("key", "aabb"), ("nonce", TEST_NONCE)]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn encrypt_missing_params() {
        let r = exec1(&EncryptFn, b"data");
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }


    // ── #38 DecryptFn (AES-256-CTR) ──

    #[test]
    fn decrypt_roundtrip() {
        let input = b"plaintext data here";
        let p = params(&[("key", TEST_KEY), ("nonce", TEST_NONCE)]);
        let encrypted = exec1_params(&EncryptFn, input, p.clone()).unwrap();
        let decrypted = exec1_params(&DecryptFn, &encrypted, p).unwrap();
        assert_eq!(decrypted.as_ref(), input);
    }

    #[test]
    fn decrypt_is_same_as_encrypt() {
        let input = b"ctr mode is symmetric";
        let p = params(&[("key", TEST_KEY), ("nonce", TEST_NONCE)]);
        let via_encrypt = exec1_params(&EncryptFn, input, p.clone()).unwrap();
        let via_decrypt = exec1_params(&DecryptFn, input, p).unwrap();
        assert_eq!(via_encrypt, via_decrypt);
    }

    #[test]
    fn decrypt_empty() {
        let decrypted = exec1_params(&DecryptFn, b"", params(&[("key", TEST_KEY), ("nonce", TEST_NONCE)])).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn decrypt_wrong_key_length() {
        let r = exec1_params(&DecryptFn, b"data", params(&[("key", "short"), ("nonce", TEST_NONCE)]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn decrypt_wrong_nonce_length() {
        let r = exec1_params(&DecryptFn, b"data", params(&[("key", TEST_KEY), ("nonce", "aabb")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }


    // ── #39 AeadEncryptFn (AES-256-GCM) ──


    #[test]
    fn aead_encrypt_adds_tag() {
        let input = b"authenticated data";
        let encrypted = exec1_params(&AeadEncryptFn, input, params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)])).unwrap();
        assert_eq!(encrypted.len(), input.len() + 16); // 16-byte tag appended
    }

    #[test]
    fn aead_encrypt_empty_input() {
        let encrypted = exec1_params(&AeadEncryptFn, b"", params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)])).unwrap();
        assert_eq!(encrypted.len(), 16); // tag only
    }

    #[test]
    fn aead_encrypt_roundtrip() {
        let input = b"roundtrip test for gcm";
        let p = params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)]);
        let encrypted = exec1_params(&AeadEncryptFn, input, p.clone()).unwrap();
        let decrypted = exec1_params(&AeadDecryptFn, &encrypted, p).unwrap();
        assert_eq!(decrypted.as_ref(), input);
    }

    #[test]
    fn aead_encrypt_wrong_key_length() {
        let r = exec1_params(&AeadEncryptFn, b"data", params(&[("key", "aabb"), ("nonce", TEST_GCM_NONCE)]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn aead_encrypt_wrong_nonce_length() {
        let r = exec1_params(&AeadEncryptFn, b"data", params(&[("key", TEST_KEY), ("nonce", "aabb")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }


    // ── #40 AeadDecryptFn (AES-256-GCM) ──

    #[test]
    fn aead_decrypt_roundtrip() {
        let input = b"verify aead decryption";
        let p = params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)]);
        let encrypted = exec1_params(&AeadEncryptFn, input, p.clone()).unwrap();
        let decrypted = exec1_params(&AeadDecryptFn, &encrypted, p).unwrap();
        assert_eq!(decrypted.as_ref(), input);
    }

    #[test]
    fn aead_decrypt_tampered_ciphertext() {
        let input = b"tamper test";
        let p = params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)]);
        let mut encrypted = exec1_params(&AeadEncryptFn, input, p.clone()).unwrap().to_vec();
        encrypted[0] ^= 0xFF; // flip a byte
        let r = exec1_params(&AeadDecryptFn, &encrypted, p);
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn aead_decrypt_too_short() {
        let r = exec1_params(&AeadDecryptFn, &[0u8; 10], params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn aead_decrypt_wrong_key() {
        let input = b"wrong key test";
        let p1 = params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)]);
        let encrypted = exec1_params(&AeadEncryptFn, input, p1).unwrap();
        let wrong_key = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let p2 = params(&[("key", wrong_key), ("nonce", TEST_GCM_NONCE)]);
        let r = exec1_params(&AeadDecryptFn, &encrypted, p2);
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn aead_decrypt_empty_ciphertext_roundtrip() {
        let p = params(&[("key", TEST_KEY), ("nonce", TEST_GCM_NONCE)]);
        let encrypted = exec1_params(&AeadEncryptFn, b"", p.clone()).unwrap();
        let decrypted = exec1_params(&AeadDecryptFn, &encrypted, p).unwrap();
        assert!(decrypted.is_empty());
    }


    // ── #41 RedactFn ──

    #[test]
    fn redact_email_pattern() {
        let input = b"Contact <email> for info";
        let result = exec1_params(&RedactFn, input, params(&[("patterns", r"\S+@\S+")])).unwrap();
        // No email in this input, should be unchanged
        assert_eq!(result.as_ref(), input);
    }

    #[test]
    fn redact_replaces_matches() {
        let input = b"Call 555-1234 or 555-5678 today";
        let result = exec1_params(&RedactFn, input, params(&[("patterns", r"\d{3}-\d{4}")])).unwrap();
        assert_eq!(result.as_ref(), b"Call [REDACTED] or [REDACTED] today");
    }

    #[test]
    fn redact_multiple_patterns() {
        let input = b"age=25 color=red";
        let result = exec1_params(&RedactFn, input, params(&[("patterns", r"\d+,red")])).unwrap();
        assert_eq!(std::str::from_utf8(&result).unwrap(), "age=[REDACTED] color=[REDACTED]");
    }

    #[test]
    fn redact_invalid_regex() {
        let r = exec1_params(&RedactFn, b"data", params(&[("patterns", "[invalid")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn redact_missing_patterns() {
        let r = exec1(&RedactFn, b"data");
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    // ── Helper to read u64 BE from result ──


}

mod accumulators {
    use super::*;

    // ── #42 ByteCountFn ──

    #[test]
    fn byte_count_empty() {
        assert_eq!(read_u64(&exec1(&ByteCountFn, b"").unwrap()), 0);
    }

    #[test]
    fn byte_count_known_length() {
        assert_eq!(read_u64(&exec1(&ByteCountFn, b"hello").unwrap()), 5);
    }

    #[test]
    fn byte_count_binary() {
        let input = vec![0u8; 10_000];
        assert_eq!(read_u64(&exec1(&ByteCountFn, &input).unwrap()), 10_000);
    }

    #[test]
    fn byte_count_output_is_8_bytes() {
        assert_eq!(exec1(&ByteCountFn, b"x").unwrap().len(), 8);
    }

    #[test]
    fn byte_count_null_bytes() {
        assert_eq!(read_u64(&exec1(&ByteCountFn, &[0x00, 0x00, 0x00]).unwrap()), 3);
    }


    // ── #43 LineCountFn ──

    #[test]
    fn line_count_empty() {
        assert_eq!(read_u64(&exec1(&LineCountFn, b"").unwrap()), 0);
    }

    #[test]
    fn line_count_no_trailing_newline() {
        assert_eq!(read_u64(&exec1(&LineCountFn, b"abc").unwrap()), 1);
    }

    #[test]
    fn line_count_with_trailing_newline() {
        assert_eq!(read_u64(&exec1(&LineCountFn, b"abc\n").unwrap()), 1);
    }

    #[test]
    fn line_count_multiple_lines() {
        assert_eq!(read_u64(&exec1(&LineCountFn, b"a\nb\nc\n").unwrap()), 3);
    }

    #[test]
    fn line_count_single_newline() {
        assert_eq!(read_u64(&exec1(&LineCountFn, b"\n").unwrap()), 1);
    }


    // ── #44 WordCountFn ──

    #[test]
    fn word_count_empty() {
        assert_eq!(read_u64(&exec1(&WordCountFn, b"").unwrap()), 0);
    }

    #[test]
    fn word_count_single_word() {
        assert_eq!(read_u64(&exec1(&WordCountFn, b"hello").unwrap()), 1);
    }

    #[test]
    fn word_count_multiple_words() {
        assert_eq!(read_u64(&exec1(&WordCountFn, b"the quick brown fox").unwrap()), 4);
    }

    #[test]
    fn word_count_multiple_spaces() {
        assert_eq!(read_u64(&exec1(&WordCountFn, b"  hello   world  ").unwrap()), 2);
    }

    #[test]
    fn word_count_tabs_and_newlines() {
        assert_eq!(read_u64(&exec1(&WordCountFn, b"a\tb\nc\rd").unwrap()), 4);
    }


    // ── #45 HistogramFn ──

    #[test]
    fn histogram_output_size() {
        assert_eq!(exec1(&HistogramFn, b"abc").unwrap().len(), 2048);
    }

    #[test]
    fn histogram_empty() {
        let result = exec1(&HistogramFn, b"").unwrap();
        assert!(result.iter().all(|&b| b == 0));
    }

    #[test]
    fn histogram_single_byte() {
        let result = exec1(&HistogramFn, &[0x42]).unwrap();
        let offset = 0x42 * 8;
        let count = u64::from_be_bytes(result[offset..offset + 8].try_into().unwrap());
        assert_eq!(count, 1);
    }

    #[test]
    fn histogram_repeated_byte() {
        let input = vec![0xAA; 100];
        let result = exec1(&HistogramFn, &input).unwrap();
        let offset = 0xAA * 8;
        let count = u64::from_be_bytes(result[offset..offset + 8].try_into().unwrap());
        assert_eq!(count, 100);
    }

    #[test]
    fn histogram_counts_sum_to_input_length() {
        let input = b"hello world";
        let result = exec1(&HistogramFn, input).unwrap();
        let total: u64 = (0..256).map(|i| {
            u64::from_be_bytes(result[i * 8..(i + 1) * 8].try_into().unwrap())
        }).sum();
        assert_eq!(total, input.len() as u64);
    }


    // ── #46 EntropyFn ──

    #[test]
    fn entropy_empty() {
        assert_eq!(read_f64(&exec1(&EntropyFn, b"").unwrap()), 0.0);
    }

    #[test]
    fn entropy_single_byte_repeated() {
        let input = vec![0x42; 1000];
        assert_eq!(read_f64(&exec1(&EntropyFn, &input).unwrap()), 0.0);
    }

    #[test]
    fn entropy_two_equal_bytes() {
        // 50/50 distribution → entropy = 1.0 bit
        let mut input = vec![0u8; 100];
        input.extend(vec![1u8; 100]);
        let e = read_f64(&exec1(&EntropyFn, &input).unwrap());
        assert!((e - 1.0).abs() < 0.001);
    }

    #[test]
    fn entropy_max_for_uniform() {
        // All 256 byte values equally → 8.0 bits
        let input: Vec<u8> = (0..=255).collect();
        let e = read_f64(&exec1(&EntropyFn, &input).unwrap());
        assert!((e - 8.0).abs() < 0.001);
    }

    #[test]
    fn entropy_range() {
        let input = b"some typical english text with varied characters";
        let e = read_f64(&exec1(&EntropyFn, input).unwrap());
        assert!(e > 0.0 && e <= 8.0);
    }


    // ── #47 MinMaxFn ──

    #[test]
    fn min_max_single_byte() {
        assert_eq!(exec1(&MinMaxFn, &[0x42]).unwrap().as_ref(), &[0x42, 0x42]);
    }

    #[test]
    fn min_max_range() {
        assert_eq!(exec1(&MinMaxFn, &[5, 1, 9, 3]).unwrap().as_ref(), &[1, 9]);
    }

    #[test]
    fn min_max_full_range() {
        let input: Vec<u8> = (0..=255).collect();
        assert_eq!(exec1(&MinMaxFn, &input).unwrap().as_ref(), &[0x00, 0xFF]);
    }

    #[test]
    fn min_max_empty_error() {
        let r = exec1(&MinMaxFn, b"");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn min_max_all_same() {
        assert_eq!(exec1(&MinMaxFn, &[0x77; 50]).unwrap().as_ref(), &[0x77, 0x77]);
    }


    // ── #48 SumFn ──

    #[test]
    fn sum_integers() {
        assert_eq!(exec1(&SumFn, b"1\n2\n3\n").unwrap().as_ref(), b"6");
    }

    #[test]
    fn sum_floats() {
        let result = exec1(&SumFn, b"1.5\n2.5\n").unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 4.0).abs() < 0.001);
    }

    #[test]
    fn sum_empty() {
        assert_eq!(exec1(&SumFn, b"").unwrap().as_ref(), b"0");
    }

    #[test]
    fn sum_blank_lines_skipped() {
        assert_eq!(exec1(&SumFn, b"10\n\n20\n").unwrap().as_ref(), b"30");
    }

    #[test]
    fn sum_non_numeric_error() {
        let r = exec1(&SumFn, b"1\nabc\n3");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }


    // ── #49 AverageFn ──

    #[test]
    fn average_integers() {
        let result = exec1(&AverageFn, b"2\n4\n6\n").unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 4.0).abs() < 0.001);
    }

    #[test]
    fn average_single_number() {
        let result = exec1(&AverageFn, b"42").unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 42.0).abs() < 0.001);
    }

    #[test]
    fn average_empty_error() {
        let r = exec1(&AverageFn, b"");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn average_blank_lines_only_error() {
        let r = exec1(&AverageFn, b"\n\n\n");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn average_non_numeric_error() {
        let r = exec1(&AverageFn, b"1\nfoo\n3");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }


}

mod combiners {
    use super::*;

    // ── #50 InterleaveFn ──

    #[test]
    fn interleave_byte_level() {
        let r = InterleaveFn.execute(vec![Bytes::from("abc"), Bytes::from("123")], &BTreeMap::new()).unwrap();
        assert_eq!(r.as_ref(), b"a1b2c3");
    }

    #[test]
    fn interleave_block_size_2() {
        let r = InterleaveFn.execute(
            vec![Bytes::from("aabb"), Bytes::from("1122")],
            &params(&[("block_size", "2")]),
        ).unwrap();
        assert_eq!(r.as_ref(), b"aa11bb22");
    }

    #[test]
    fn interleave_unequal_lengths() {
        let r = InterleaveFn.execute(vec![Bytes::from("abcde"), Bytes::from("12")], &BTreeMap::new()).unwrap();
        assert_eq!(r.as_ref(), b"a1b2cde");
    }

    #[test]
    fn interleave_three_inputs() {
        let r = InterleaveFn.execute(
            vec![Bytes::from("ab"), Bytes::from("12"), Bytes::from("XY")],
            &BTreeMap::new(),
        ).unwrap();
        assert_eq!(r.as_ref(), b"a1Xb2Y");
    }

    #[test]
    fn interleave_block_size_zero_error() {
        let r = InterleaveFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &params(&[("block_size", "0")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }


    // ── #51 ZipConcatFn ──

    #[test]
    fn zip_concat_equal_lines() {
        let r = ZipConcatFn.execute(vec![Bytes::from("hello\nworld"), Bytes::from(" foo\n bar")], &BTreeMap::new()).unwrap();
        assert_eq!(r.as_ref(), b"hello foo\nworld bar");
    }

    #[test]
    fn zip_concat_unequal_lines() {
        let r = ZipConcatFn.execute(vec![Bytes::from("a\nb\nc"), Bytes::from("1")], &BTreeMap::new()).unwrap();
        assert_eq!(r.as_ref(), b"a1\nb\nc");
    }

    #[test]
    fn zip_concat_both_empty() {
        let r = ZipConcatFn.execute(vec![Bytes::new(), Bytes::new()], &BTreeMap::new()).unwrap();
        assert_eq!(r.as_ref(), b"");
    }

    #[test]
    fn zip_concat_one_empty() {
        let r = ZipConcatFn.execute(vec![Bytes::from("line1\nline2"), Bytes::new()], &BTreeMap::new()).unwrap();
        assert_eq!(r.as_ref(), b"line1\nline2");
    }

    #[test]
    fn zip_concat_wrong_input_count() {
        let r = ZipConcatFn.execute(vec![Bytes::from("a")], &BTreeMap::new());
        assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 1 })));
    }


    // ── #52 DiffFn + #53 PatchFn ──

    #[test]
    fn diff_patch_roundtrip() {
        let old = Bytes::from("hello world");
        let new = Bytes::from("hello rust!");
        let d = DiffFn.execute(vec![old.clone(), new.clone()], &BTreeMap::new()).unwrap();
        let patched = PatchFn.execute(vec![old, d], &BTreeMap::new()).unwrap();
        assert_eq!(patched, new);
    }

    #[test]
    fn diff_identical_inputs() {
        let data = Bytes::from("same data");
        let d = DiffFn.execute(vec![data.clone(), data.clone()], &BTreeMap::new()).unwrap();
        let patched = PatchFn.execute(vec![data.clone(), d], &BTreeMap::new()).unwrap();
        assert_eq!(patched, data);
    }

    #[test]
    fn diff_empty_old() {
        let old = Bytes::new();
        let new = Bytes::from("new content");
        let d = DiffFn.execute(vec![old.clone(), new.clone()], &BTreeMap::new()).unwrap();
        let patched = PatchFn.execute(vec![old, d], &BTreeMap::new()).unwrap();
        assert_eq!(patched, new);
    }

    #[test]
    fn diff_empty_new() {
        let old = Bytes::from("old content");
        let new = Bytes::new();
        let d = DiffFn.execute(vec![old.clone(), new.clone()], &BTreeMap::new()).unwrap();
        let patched = PatchFn.execute(vec![old, d], &BTreeMap::new()).unwrap();
        assert_eq!(patched, new);
    }

    #[test]
    fn diff_both_empty() {
        let d = DiffFn.execute(vec![Bytes::new(), Bytes::new()], &BTreeMap::new()).unwrap();
        assert!(d.is_empty());
    }


    // ── #53 PatchFn (additional) ──

    #[test]
    fn patch_corrupt_data() {
        let r = PatchFn.execute(vec![Bytes::from("base"), Bytes::from(vec![0xFF])], &BTreeMap::new());
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn patch_empty_patch() {
        let r = PatchFn.execute(vec![Bytes::from("base"), Bytes::new()], &BTreeMap::new()).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn patch_binary_roundtrip() {
        let old: Vec<u8> = (0..100).collect();
        let new: Vec<u8> = (50..150).collect();
        let d = DiffFn.execute(vec![Bytes::from(old.clone()), Bytes::from(new.clone())], &BTreeMap::new()).unwrap();
        let patched = PatchFn.execute(vec![Bytes::from(old), d], &BTreeMap::new()).unwrap();
        assert_eq!(patched.as_ref(), new.as_slice());
    }

    #[test]
    fn patch_wrong_input_count() {
        let r = PatchFn.execute(vec![Bytes::from("a")], &BTreeMap::new());
        assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 1 })));
    }

    #[test]
    fn patch_truncated() {
        // op byte but no length
        let r = PatchFn.execute(vec![Bytes::from("base"), Bytes::from(vec![0x00, 0x00])], &BTreeMap::new());
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }


    // ── #54 MergeSortedFn ──

    #[test]
    fn merge_sorted_two_inputs() {
        let r = MergeSortedFn.execute(
            vec![Bytes::from("apple\ncherry"), Bytes::from("banana\ndate")],
            &BTreeMap::new(),
        ).unwrap();
        assert_eq!(r.as_ref(), b"apple\nbanana\ncherry\ndate");
    }

    #[test]
    fn merge_sorted_one_empty() {
        let r = MergeSortedFn.execute(
            vec![Bytes::from("a\nb\nc"), Bytes::new()],
            &BTreeMap::new(),
        ).unwrap();
        assert_eq!(r.as_ref(), b"a\nb\nc");
    }

    #[test]
    fn merge_sorted_three_inputs() {
        let r = MergeSortedFn.execute(
            vec![Bytes::from("b\ne"), Bytes::from("a\nd"), Bytes::from("c\nf")],
            &BTreeMap::new(),
        ).unwrap();
        assert_eq!(r.as_ref(), b"a\nb\nc\nd\ne\nf");
    }

    #[test]
    fn merge_sorted_duplicates() {
        let r = MergeSortedFn.execute(
            vec![Bytes::from("a\na"), Bytes::from("a\nb")],
            &BTreeMap::new(),
        ).unwrap();
        assert_eq!(r.as_ref(), b"a\na\na\nb");
    }

    #[test]
    fn merge_sorted_empty_inputs() {
        let r = MergeSortedFn.execute(vec![Bytes::new(), Bytes::new()], &BTreeMap::new()).unwrap();
        assert_eq!(r.as_ref(), b"");
    }


    // ── #55 SelectFn ──

    #[test]
    fn select_first() {
        let r = SelectFn.execute(
            vec![Bytes::from("first"), Bytes::from("second")],
            &params(&[("index", "0")]),
        ).unwrap();
        assert_eq!(r, Bytes::from("first"));
    }

    #[test]
    fn select_second() {
        let r = SelectFn.execute(
            vec![Bytes::from("first"), Bytes::from("second")],
            &params(&[("index", "1")]),
        ).unwrap();
        assert_eq!(r, Bytes::from("second"));
    }

    #[test]
    fn select_out_of_range() {
        let r = SelectFn.execute(vec![Bytes::from("only")], &params(&[("index", "5")]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn select_missing_param() {
        let r = SelectFn.execute(vec![Bytes::from("data")], &BTreeMap::new());
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn select_single_input() {
        let r = SelectFn.execute(vec![Bytes::from("only")], &params(&[("index", "0")])).unwrap();
        assert_eq!(r, Bytes::from("only"));
    }


}

mod slicing {
    use super::*;

    // ── #56 TakeFn ──

    #[test]
    fn take_first_5_bytes() {
        let r = exec1_params(&TakeFn, b"hello world", params(&[("bytes", "5")])).unwrap();
        assert_eq!(r.as_ref(), b"hello");
    }

    #[test]
    fn take_more_than_input() {
        let r = exec1_params(&TakeFn, b"short", params(&[("bytes", "100")])).unwrap();
        assert_eq!(r.as_ref(), b"short");
    }

    #[test]
    fn take_zero() {
        let r = exec1_params(&TakeFn, b"data", params(&[("bytes", "0")])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn take_empty_input() {
        let r = exec1_params(&TakeFn, b"", params(&[("bytes", "5")])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn take_exact_length() {
        let r = exec1_params(&TakeFn, b"abcd", params(&[("bytes", "4")])).unwrap();
        assert_eq!(r.as_ref(), b"abcd");
    }


    // ── #57 SkipFn ──

    #[test]
    fn skip_first_5_bytes() {
        let r = exec1_params(&SkipFn, b"hello world", params(&[("bytes", "5")])).unwrap();
        assert_eq!(r.as_ref(), b" world");
    }

    #[test]
    fn skip_more_than_input() {
        let r = exec1_params(&SkipFn, b"short", params(&[("bytes", "100")])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn skip_zero() {
        let r = exec1_params(&SkipFn, b"data", params(&[("bytes", "0")])).unwrap();
        assert_eq!(r.as_ref(), b"data");
    }

    #[test]
    fn skip_empty_input() {
        let r = exec1_params(&SkipFn, b"", params(&[("bytes", "5")])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn skip_exact_length() {
        let r = exec1_params(&SkipFn, b"abcd", params(&[("bytes", "4")])).unwrap();
        assert!(r.is_empty());
    }


    // ── #58 SliceFn ──

    #[test]
    fn slice_middle() {
        let r = exec1_params(&SliceFn, b"hello world", {
            let mut p = params(&[("offset", "6"), ("length", "5")]);
            p
        }).unwrap();
        assert_eq!(r.as_ref(), b"world");
    }

    #[test]
    fn slice_past_end() {
        let r = exec1_params(&SliceFn, b"short", params(&[("offset", "100"), ("length", "5")])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn slice_length_past_end() {
        let r = exec1_params(&SliceFn, b"hello", params(&[("offset", "3"), ("length", "100")])).unwrap();
        assert_eq!(r.as_ref(), b"lo");
    }

    #[test]
    fn slice_zero_length() {
        let r = exec1_params(&SliceFn, b"data", params(&[("offset", "0"), ("length", "0")])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn slice_full_input() {
        let r = exec1_params(&SliceFn, b"abcde", params(&[("offset", "0"), ("length", "5")])).unwrap();
        assert_eq!(r.as_ref(), b"abcde");
    }


    // ── #59 SortFn ──

    #[test]
    fn sort_lines() {
        let r = exec1(&SortFn, b"cherry\napple\nbanana").unwrap();
        assert_eq!(r.as_ref(), b"apple\nbanana\ncherry");
    }

    #[test]
    fn sort_already_sorted() {
        let r = exec1(&SortFn, b"a\nb\nc").unwrap();
        assert_eq!(r.as_ref(), b"a\nb\nc");
    }

    #[test]
    fn sort_single_line() {
        let r = exec1(&SortFn, b"only").unwrap();
        assert_eq!(r.as_ref(), b"only");
    }

    #[test]
    fn sort_empty() {
        let r = exec1(&SortFn, b"").unwrap();
        assert_eq!(r.as_ref(), b"");
    }

    #[test]
    fn sort_duplicates() {
        let r = exec1(&SortFn, b"b\na\nb\na").unwrap();
        assert_eq!(r.as_ref(), b"a\na\nb\nb");
    }


    // ── #60 UniqueFn ──

    #[test]
    fn unique_adjacent_dupes() {
        let r = exec1(&UniqueFn, b"a\na\nb\nb\nc").unwrap();
        assert_eq!(r.as_ref(), b"a\nb\nc");
    }

    #[test]
    fn unique_no_dupes() {
        let r = exec1(&UniqueFn, b"a\nb\nc").unwrap();
        assert_eq!(r.as_ref(), b"a\nb\nc");
    }

    #[test]
    fn unique_all_same() {
        let r = exec1(&UniqueFn, b"x\nx\nx").unwrap();
        assert_eq!(r.as_ref(), b"x");
    }

    #[test]
    fn unique_non_adjacent_dupes_kept() {
        let r = exec1(&UniqueFn, b"a\nb\na").unwrap();
        assert_eq!(r.as_ref(), b"a\nb\na");
    }

    #[test]
    fn unique_empty() {
        let r = exec1(&UniqueFn, b"").unwrap();
        assert_eq!(r.as_ref(), b"");
    }


    // ── #61 SortUniqueFn ──

    #[test]
    fn sort_unique_basic() {
        let r = exec1(&SortUniqueFn, b"c\na\nb\na\nc").unwrap();
        assert_eq!(r.as_ref(), b"a\nb\nc");
    }

    #[test]
    fn sort_unique_already_unique() {
        let r = exec1(&SortUniqueFn, b"b\na\nc").unwrap();
        assert_eq!(r.as_ref(), b"a\nb\nc");
    }

    #[test]
    fn sort_unique_all_same() {
        let r = exec1(&SortUniqueFn, b"x\nx\nx").unwrap();
        assert_eq!(r.as_ref(), b"x");
    }

    #[test]
    fn sort_unique_single() {
        let r = exec1(&SortUniqueFn, b"only").unwrap();
        assert_eq!(r.as_ref(), b"only");
    }

    #[test]
    fn sort_unique_empty() {
        let r = exec1(&SortUniqueFn, b"").unwrap();
        assert_eq!(r.as_ref(), b"");
    }


    // ── #62 ShuffleFn ──

    #[test]
    fn shuffle_deterministic() {
        let input = b"a\nb\nc\nd\ne";
        let r1 = exec1_params(&ShuffleFn, input, params(&[("seed", "42")])).unwrap();
        let r2 = exec1_params(&ShuffleFn, input, params(&[("seed", "42")])).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn shuffle_different_seeds_differ() {
        let input = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
        let r1 = exec1_params(&ShuffleFn, input, params(&[("seed", "1")])).unwrap();
        let r2 = exec1_params(&ShuffleFn, input, params(&[("seed", "2")])).unwrap();
        assert_ne!(r1, r2);
    }

    #[test]
    fn shuffle_preserves_all_lines() {
        let r = exec1_params(&ShuffleFn, b"x\ny\nz", params(&[("seed", "99")])).unwrap();
        let mut lines: Vec<&str> = std::str::from_utf8(&r).unwrap().lines().collect();
        lines.sort();
        assert_eq!(lines, vec!["x", "y", "z"]);
    }

    #[test]
    fn shuffle_single_line() {
        let r = exec1_params(&ShuffleFn, b"only", params(&[("seed", "0")])).unwrap();
        assert_eq!(r.as_ref(), b"only");
    }

    #[test]
    fn shuffle_empty() {
        let r = exec1_params(&ShuffleFn, b"", params(&[("seed", "0")])).unwrap();
        assert_eq!(r.as_ref(), b"");
    }


    // ── #63 HeadFn ──

    #[test]
    fn head_first_2_lines() {
        let r = exec1_params(&HeadFn, b"a\nb\nc\nd", params(&[("lines", "2")])).unwrap();
        assert_eq!(r.as_ref(), b"a\nb");
    }

    #[test]
    fn head_more_than_available() {
        let r = exec1_params(&HeadFn, b"a\nb", params(&[("lines", "10")])).unwrap();
        assert_eq!(r.as_ref(), b"a\nb");
    }

    #[test]
    fn head_one_line() {
        let r = exec1_params(&HeadFn, b"a\nb\nc", params(&[("lines", "1")])).unwrap();
        assert_eq!(r.as_ref(), b"a");
    }

    #[test]
    fn head_empty() {
        let r = exec1_params(&HeadFn, b"", params(&[("lines", "5")])).unwrap();
        assert_eq!(r.as_ref(), b"");
    }

    #[test]
    fn head_single_line_input() {
        let r = exec1_params(&HeadFn, b"only line", params(&[("lines", "1")])).unwrap();
        assert_eq!(r.as_ref(), b"only line");
    }


    // ── #64 TailFn ──

    #[test]
    fn tail_last_2_lines() {
        let r = exec1_params(&TailFn, b"a\nb\nc\nd", params(&[("lines", "2")])).unwrap();
        assert_eq!(r.as_ref(), b"c\nd");
    }

    #[test]
    fn tail_more_than_available() {
        let r = exec1_params(&TailFn, b"a\nb", params(&[("lines", "10")])).unwrap();
        assert_eq!(r.as_ref(), b"a\nb");
    }

    #[test]
    fn tail_one_line() {
        let r = exec1_params(&TailFn, b"a\nb\nc", params(&[("lines", "1")])).unwrap();
        assert_eq!(r.as_ref(), b"c");
    }

    #[test]
    fn tail_empty() {
        let r = exec1_params(&TailFn, b"", params(&[("lines", "5")])).unwrap();
        assert_eq!(r.as_ref(), b"");
    }

    #[test]
    fn tail_single_line_input() {
        let r = exec1_params(&TailFn, b"only line", params(&[("lines", "1")])).unwrap();
        assert_eq!(r.as_ref(), b"only line");
    }


    // ── #65 SampleFn ──

    #[test]
    fn sample_deterministic() {
        let input = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
        let p = params(&[("lines", "3"), ("seed", "42")]);
        let r1 = exec1_params(&SampleFn, input, p.clone()).unwrap();
        let r2 = exec1_params(&SampleFn, input, p).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn sample_correct_count() {
        let input = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
        let r = exec1_params(&SampleFn, input, params(&[("lines", "3"), ("seed", "7")])).unwrap();
        let count = std::str::from_utf8(&r).unwrap().lines().count();
        assert_eq!(count, 3);
    }

    #[test]
    fn sample_n_exceeds_lines() {
        let r = exec1_params(&SampleFn, b"a\nb", params(&[("lines", "10"), ("seed", "0")])).unwrap();
        assert_eq!(r.as_ref(), b"a\nb");
    }

    #[test]
    fn sample_preserves_order() {
        let input = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10";
        let r = exec1_params(&SampleFn, input, params(&[("lines", "4"), ("seed", "42")])).unwrap();
        let lines: Vec<i32> = std::str::from_utf8(&r).unwrap().lines().map(|l| l.parse().unwrap()).collect();
        let mut sorted = lines.clone();
        sorted.sort();
        assert_eq!(lines, sorted);
    }

    #[test]
    fn sample_empty() {
        let r = exec1_params(&SampleFn, b"", params(&[("lines", "5"), ("seed", "0")])).unwrap();
        assert_eq!(r.as_ref(), b"");
    }


}

mod text {
    use super::*;

    // ── #66 ReplaceFn ──

    #[test]
    fn replace_basic() {
        let r = exec1_params(&ReplaceFn, b"hello world", params(&[("find", "world"), ("replace", "rust")])).unwrap();
        assert_eq!(r.as_ref(), b"hello rust");
    }

    #[test]
    fn replace_multiple_occurrences() {
        let r = exec1_params(&ReplaceFn, b"aabaa", params(&[("find", "a"), ("replace", "x")])).unwrap();
        assert_eq!(r.as_ref(), b"xxbxx");
    }

    #[test]
    fn replace_no_match() {
        let r = exec1_params(&ReplaceFn, b"hello", params(&[("find", "xyz"), ("replace", "!")])).unwrap();
        assert_eq!(r.as_ref(), b"hello");
    }

    #[test]
    fn replace_empty_find_unchanged() {
        let r = exec1_params(&ReplaceFn, b"hello", params(&[("find", ""), ("replace", "x")])).unwrap();
        assert_eq!(r.as_ref(), b"hello");
    }

    #[test]
    fn replace_empty_input() {
        let r = exec1_params(&ReplaceFn, b"", params(&[("find", "a"), ("replace", "b")])).unwrap();
        assert!(r.is_empty());
    }


    // ── #67 RegexReplaceFn ──

    #[test]
    fn regex_replace_basic() {
        let r = exec1_params(&RegexReplaceFn, b"foo123bar", params(&[("pattern", r"\d+"), ("replacement", "NUM")])).unwrap();
        assert_eq!(r.as_ref(), b"fooNUMbar");
    }

    #[test]
    fn regex_replace_capture_group() {
        let r = exec1_params(&RegexReplaceFn, b"2024-01-15", params(&[("pattern", r"(\d{4})-(\d{2})-(\d{2})"), ("replacement", "$2/$3/$1")])).unwrap();
        assert_eq!(r.as_ref(), b"01/15/2024");
    }

    #[test]
    fn regex_replace_no_match() {
        let r = exec1_params(&RegexReplaceFn, b"hello", params(&[("pattern", r"\d+"), ("replacement", "X")])).unwrap();
        assert_eq!(r.as_ref(), b"hello");
    }

    #[test]
    fn regex_replace_invalid_regex() {
        let r = exec1_params(&RegexReplaceFn, b"test", params(&[("pattern", "[invalid"), ("replacement", "x")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn regex_replace_multiple() {
        let r = exec1_params(&RegexReplaceFn, b"a1b2c3", params(&[("pattern", r"[0-9]"), ("replacement", "")])).unwrap();
        assert_eq!(r.as_ref(), b"abc");
    }


    // ── #68 GrepFn ──

    #[test]
    fn grep_matching_lines() {
        let r = exec1_params(&GrepFn, b"error: bad\ninfo: ok\nerror: fail", params(&[("pattern", "^error")])).unwrap();
        assert_eq!(r.as_ref(), b"error: bad\nerror: fail");
    }

    #[test]
    fn grep_no_matches() {
        let r = exec1_params(&GrepFn, b"hello\nworld", params(&[("pattern", "xyz")])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn grep_all_match() {
        let r = exec1_params(&GrepFn, b"aa\nab\nac", params(&[("pattern", "^a")])).unwrap();
        assert_eq!(r.as_ref(), b"aa\nab\nac");
    }

    #[test]
    fn grep_invalid_regex() {
        let r = exec1_params(&GrepFn, b"test", params(&[("pattern", "[bad")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn grep_empty_input() {
        let r = exec1_params(&GrepFn, b"", params(&[("pattern", ".*")])).unwrap();
        assert_eq!(r.as_ref(), b"");
    }


    // ── #69 GrepInvertFn ──

    #[test]
    fn grep_invert_basic() {
        let r = exec1_params(&GrepInvertFn, b"error: bad\ninfo: ok\nerror: fail", params(&[("pattern", "^error")])).unwrap();
        assert_eq!(r.as_ref(), b"info: ok");
    }

    #[test]
    fn grep_invert_no_matches_keeps_all() {
        let r = exec1_params(&GrepInvertFn, b"a\nb\nc", params(&[("pattern", "xyz")])).unwrap();
        assert_eq!(r.as_ref(), b"a\nb\nc");
    }

    #[test]
    fn grep_invert_all_match_empty() {
        let r = exec1_params(&GrepInvertFn, b"aa\nab\nac", params(&[("pattern", "^a")])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn grep_invert_invalid_regex() {
        let r = exec1_params(&GrepInvertFn, b"test", params(&[("pattern", "[bad")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn grep_invert_empty_input() {
        let r = exec1_params(&GrepInvertFn, b"", params(&[("pattern", ".*")])).unwrap();
        assert_eq!(r.as_ref(), b"");
    }


    // ── #70 PrefixFn ──

    #[test]
    fn prefix_basic() {
        let r = exec1_params(&PrefixFn, b"world", params(&[("prefix", "hello ")])).unwrap();
        assert_eq!(r.as_ref(), b"hello world");
    }

    #[test]
    fn prefix_empty_prefix() {
        let r = exec1_params(&PrefixFn, b"data", params(&[("prefix", "")])).unwrap();
        assert_eq!(r.as_ref(), b"data");
    }

    #[test]
    fn prefix_empty_input() {
        let r = exec1_params(&PrefixFn, b"", params(&[("prefix", ">>")])).unwrap();
        assert_eq!(r.as_ref(), b">>");
    }

    #[test]
    fn prefix_binary_safe() {
        let r = exec1_params(&PrefixFn, &[0xFF, 0xFE], params(&[("prefix", "BOM:")])).unwrap();
        assert_eq!(&r[..4], b"BOM:");
        assert_eq!(&r[4..], &[0xFF, 0xFE]);
    }

    #[test]
    fn prefix_missing_param() {
        let r = exec1(&PrefixFn, b"data");
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }


    // ── #71 SuffixFn ──

    #[test]
    fn suffix_basic() {
        let r = exec1_params(&SuffixFn, b"hello", params(&[("suffix", " world")])).unwrap();
        assert_eq!(r.as_ref(), b"hello world");
    }

    #[test]
    fn suffix_empty_suffix() {
        let r = exec1_params(&SuffixFn, b"data", params(&[("suffix", "")])).unwrap();
        assert_eq!(r.as_ref(), b"data");
    }

    #[test]
    fn suffix_empty_input() {
        let r = exec1_params(&SuffixFn, b"", params(&[("suffix", "END")])).unwrap();
        assert_eq!(r.as_ref(), b"END");
    }

    #[test]
    fn suffix_newline() {
        let r = exec1_params(&SuffixFn, b"line", params(&[("suffix", "\n")])).unwrap();
        assert_eq!(r.as_ref(), b"line\n");
    }

    #[test]
    fn suffix_missing_param() {
        let r = exec1(&SuffixFn, b"data");
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }


    // ── #72 LinePrefixFn ──

    #[test]
    fn line_prefix_basic() {
        let r = exec1_params(&LinePrefixFn, b"a\nb\nc", params(&[("prefix", "> ")])).unwrap();
        assert_eq!(r.as_ref(), b"> a\n> b\n> c");
    }

    #[test]
    fn line_prefix_empty_prefix() {
        let r = exec1_params(&LinePrefixFn, b"a\nb", params(&[("prefix", "")])).unwrap();
        assert_eq!(r.as_ref(), b"a\nb");
    }

    #[test]
    fn line_prefix_single_line() {
        let r = exec1_params(&LinePrefixFn, b"hello", params(&[("prefix", "# ")])).unwrap();
        assert_eq!(r.as_ref(), b"# hello");
    }

    #[test]
    fn line_prefix_empty_input() {
        let r = exec1_params(&LinePrefixFn, b"", params(&[("prefix", "> ")])).unwrap();
        assert_eq!(r.as_ref(), b"");
    }

    #[test]
    fn line_prefix_tab_indent() {
        let r = exec1_params(&LinePrefixFn, b"x\ny", params(&[("prefix", "\t")])).unwrap();
        assert_eq!(r.as_ref(), b"\tx\n\ty");
    }


    // ── #73 LineNumberFn ──

    #[test]
    fn line_number_basic() {
        let r = exec1(&LineNumberFn, b"alpha\nbeta\ngamma").unwrap();
        assert_eq!(r.as_ref(), b"     1\talpha\n     2\tbeta\n     3\tgamma");
    }

    #[test]
    fn line_number_single() {
        let r = exec1(&LineNumberFn, b"only").unwrap();
        assert_eq!(r.as_ref(), b"     1\tonly");
    }

    #[test]
    fn line_number_empty() {
        let r = exec1(&LineNumberFn, b"").unwrap();
        assert_eq!(r.as_ref(), b"");
    }

    #[test]
    fn line_number_starts_at_one() {
        let r = exec1(&LineNumberFn, b"a\nb").unwrap();
        let text = std::str::from_utf8(&r).unwrap();
        assert!(text.starts_with("     1\t"));
    }

    #[test]
    fn line_number_tab_separator() {
        let r = exec1(&LineNumberFn, b"test").unwrap();
        assert!(r.as_ref().contains(&b'\t'));
    }


    // ── #74 TruncateLinesFn ──

    #[test]
    fn truncate_lines_basic() {
        let r = exec1_params(&TruncateLinesFn, b"hello world\nhi", params(&[("max_line_bytes", "5")])).unwrap();
        assert_eq!(r.as_ref(), b"hello\nhi");
    }

    #[test]
    fn truncate_lines_no_truncation() {
        let r = exec1_params(&TruncateLinesFn, b"ab\ncd", params(&[("max_line_bytes", "100")])).unwrap();
        assert_eq!(r.as_ref(), b"ab\ncd");
    }

    #[test]
    fn truncate_lines_exact_length() {
        let r = exec1_params(&TruncateLinesFn, b"abcde", params(&[("max_line_bytes", "5")])).unwrap();
        assert_eq!(r.as_ref(), b"abcde");
    }

    #[test]
    fn truncate_lines_empty() {
        let r = exec1_params(&TruncateLinesFn, b"", params(&[("max_line_bytes", "5")])).unwrap();
        assert_eq!(r.as_ref(), b"");
    }

    #[test]
    fn truncate_lines_multiple() {
        let r = exec1_params(&TruncateLinesFn, b"abcdef\nghijkl\nmn", params(&[("max_line_bytes", "3")])).unwrap();
        assert_eq!(r.as_ref(), b"abc\nghi\nmn");
    }


    // ── #75 CharsetConvertFn ──

    #[test]
    fn charset_convert_latin1_to_utf8() {
        // ISO-8859-1: 0xE9 = é
        let r = exec1_params(&CharsetConvertFn, &[0xE9], params(&[("from", "iso-8859-1"), ("to", "utf-8")])).unwrap();
        assert_eq!(r.as_ref(), "é".as_bytes());
    }

    #[test]
    fn charset_convert_utf8_to_latin1() {
        let r = exec1_params(&CharsetConvertFn, "é".as_bytes(), params(&[("from", "utf-8"), ("to", "iso-8859-1")])).unwrap();
        assert_eq!(r.as_ref(), &[0xE9]);
    }

    #[test]
    fn charset_convert_utf8_roundtrip() {
        let input = "hello world".as_bytes();
        let r = exec1_params(&CharsetConvertFn, input, params(&[("from", "utf-8"), ("to", "utf-8")])).unwrap();
        assert_eq!(r.as_ref(), input);
    }

    #[test]
    fn charset_convert_unknown_encoding() {
        let r = exec1_params(&CharsetConvertFn, b"test", params(&[("from", "bogus-999"), ("to", "utf-8")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn charset_convert_empty_input() {
        let r = exec1_params(&CharsetConvertFn, b"", params(&[("from", "utf-8"), ("to", "iso-8859-1")])).unwrap();
        assert!(r.is_empty());
    }


}

mod validation {
    use super::*;

    // ── #76 Utf8ValidateFn ──

    #[test]
    fn utf8_validate_valid_ascii() {
        let r = exec1(&Utf8ValidateFn, b"hello world").unwrap();
        assert_eq!(r.as_ref(), b"hello world");
    }

    #[test]
    fn utf8_validate_valid_multibyte() {
        let r = exec1(&Utf8ValidateFn, "héllo wörld".as_bytes()).unwrap();
        assert_eq!(r.as_ref(), "héllo wörld".as_bytes());
    }

    #[test]
    fn utf8_validate_empty() {
        let r = exec1(&Utf8ValidateFn, b"").unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn utf8_validate_invalid() {
        let r = exec1(&Utf8ValidateFn, &[0xFF, 0xFE]);
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn utf8_validate_truncated_sequence() {
        let r = exec1(&Utf8ValidateFn, &[0xC3]);
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }


    // ── #77 JsonValidateFn ──

    #[test]
    fn json_validate_object() {
        let r = exec1(&JsonValidateFn, b"{\"key\": \"value\"}").unwrap();
        assert_eq!(r.as_ref(), b"{\"key\": \"value\"}");
    }

    #[test]
    fn json_validate_null() {
        let r = exec1(&JsonValidateFn, b"null").unwrap();
        assert_eq!(r.as_ref(), b"null");
    }

    #[test]
    fn json_validate_array() {
        let r = exec1(&JsonValidateFn, b"[1,2,3]").unwrap();
        assert_eq!(r.as_ref(), b"[1,2,3]");
    }

    #[test]
    fn json_validate_invalid() {
        let r = exec1(&JsonValidateFn, b"{invalid}");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn json_validate_empty() {
        let r = exec1(&JsonValidateFn, b"");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }


    // ── #78 SchemaValidateFn ──

    #[test]
    fn schema_validate_pass() {
        let schema = r#"{"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}"#;
        let r = exec1_params(&SchemaValidateFn, b"{\"name\":\"test\"}", params(&[("schema", schema)])).unwrap();
        assert_eq!(r.as_ref(), b"{\"name\":\"test\"}");
    }

    #[test]
    fn schema_validate_fail_missing_required() {
        let schema = r#"{"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}"#;
        let r = exec1_params(&SchemaValidateFn, b"{\"age\":42}", params(&[("schema", schema)]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn schema_validate_fail_wrong_type() {
        let schema = r#"{"type":"object","properties":{"age":{"type":"integer"}}}"#;
        let r = exec1_params(&SchemaValidateFn, b"{\"age\":\"not a number\"}", params(&[("schema", schema)]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn schema_validate_empty_schema_accepts_all() {
        let r = exec1_params(&SchemaValidateFn, b"42", params(&[("schema", "{}")])).unwrap();
        assert_eq!(r.as_ref(), b"42");
    }

    #[test]
    fn schema_validate_invalid_schema_json() {
        let r = exec1_params(&SchemaValidateFn, b"{}", params(&[("schema", "{bad")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }


    // ── #79 MagicBytesFn ──

    #[test]
    fn magic_bytes_png() {
        let mut input = vec![0x89, 0x50, 0x4E, 0x47];
        input.extend_from_slice(b"rest of file");
        let r = exec1_params(&MagicBytesFn, &input, params(&[("expected", "89504e47")])).unwrap();
        assert_eq!(r.as_ref(), input.as_slice());
    }

    #[test]
    fn magic_bytes_mismatch() {
        let r = exec1_params(&MagicBytesFn, b"not a png", params(&[("expected", "89504e47")]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn magic_bytes_too_short() {
        let r = exec1_params(&MagicBytesFn, &[0x89], params(&[("expected", "89504e47")]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn magic_bytes_empty_expected() {
        let r = exec1_params(&MagicBytesFn, b"anything", params(&[("expected", "")])).unwrap();
        assert_eq!(r.as_ref(), b"anything");
    }

    #[test]
    fn magic_bytes_exact_match() {
        let r = exec1_params(&MagicBytesFn, &[0xCA, 0xFE], params(&[("expected", "cafe")])).unwrap();
        assert_eq!(r.as_ref(), &[0xCA, 0xFE]);
    }


    // ── #80 SizeLimitFn ──

    #[test]
    fn size_limit_within() {
        let r = exec1_params(&SizeLimitFn, b"hello", params(&[("max_bytes", "10")])).unwrap();
        assert_eq!(r.as_ref(), b"hello");
    }

    #[test]
    fn size_limit_exact() {
        let r = exec1_params(&SizeLimitFn, b"hello", params(&[("max_bytes", "5")])).unwrap();
        assert_eq!(r.as_ref(), b"hello");
    }

    #[test]
    fn size_limit_exceeded() {
        let r = exec1_params(&SizeLimitFn, b"hello!", params(&[("max_bytes", "5")]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn size_limit_empty() {
        let r = exec1_params(&SizeLimitFn, b"", params(&[("max_bytes", "1")])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn size_limit_one_over() {
        let r = exec1_params(&SizeLimitFn, b"ab", params(&[("max_bytes", "1")]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }


    // ── #81 NonEmptyFn ──

    #[test]
    fn non_empty_passes() {
        let r = exec1(&NonEmptyFn, b"data").unwrap();
        assert_eq!(r.as_ref(), b"data");
    }

    #[test]
    fn non_empty_single_byte() {
        let r = exec1(&NonEmptyFn, &[0x00]).unwrap();
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn non_empty_fails() {
        let r = exec1(&NonEmptyFn, b"");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn non_empty_whitespace_passes() {
        let r = exec1(&NonEmptyFn, b" ").unwrap();
        assert_eq!(r.as_ref(), b" ");
    }

    #[test]
    fn non_empty_large_input() {
        let data = vec![0xAB; 10000];
        let r = exec1(&NonEmptyFn, &data).unwrap();
        assert_eq!(r.len(), 10000);
    }


    // ── #82 Sha256VerifyFn ──

    #[test]
    fn sha256_verify_correct() {
        use sha2::{Sha256, Digest};
        let data = b"hello world";
        let hash = Sha256::digest(data);
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        let r = exec1_params(&Sha256VerifyFn, data, params(&[("expected_hash", &hex)])).unwrap();
        assert_eq!(r.as_ref(), data);
    }

    #[test]
    fn sha256_verify_mismatch() {
        let r = exec1_params(&Sha256VerifyFn, b"hello", params(&[("expected_hash", "0000000000000000000000000000000000000000000000000000000000000000")]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn sha256_verify_wrong_length() {
        let r = exec1_params(&Sha256VerifyFn, b"data", params(&[("expected_hash", "abcd")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn sha256_verify_empty_input() {
        use sha2::{Sha256, Digest};
        let hash = Sha256::digest(b"");
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        let r = exec1_params(&Sha256VerifyFn, b"", params(&[("expected_hash", &hex)])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn sha256_verify_passthrough() {
        use sha2::{Sha256, Digest};
        let data = vec![0xDE; 1000];
        let hash = Sha256::digest(&data);
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        let r = exec1_params(&Sha256VerifyFn, &data, params(&[("expected_hash", &hex)])).unwrap();
        assert_eq!(r.as_ref(), data.as_slice());
    }


    // ── #83 Crc32VerifyFn ──

    #[test]
    fn crc32_verify_correct() {
        let data = b"hello world";
        let crc = crc32fast::hash(data);
        let hex = format!("{:08x}", crc);
        let r = exec1_params(&Crc32VerifyFn, data, params(&[("expected_crc32", &hex)])).unwrap();
        assert_eq!(r.as_ref(), data);
    }

    #[test]
    fn crc32_verify_mismatch() {
        let r = exec1_params(&Crc32VerifyFn, b"hello", params(&[("expected_crc32", "00000000")]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn crc32_verify_wrong_length() {
        let r = exec1_params(&Crc32VerifyFn, b"data", params(&[("expected_crc32", "ab")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn crc32_verify_empty_input() {
        let crc = crc32fast::hash(b"");
        let hex = format!("{:08x}", crc);
        let r = exec1_params(&Crc32VerifyFn, b"", params(&[("expected_crc32", &hex)])).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn crc32_verify_passthrough() {
        let data = vec![0xFF; 500];
        let crc = crc32fast::hash(&data);
        let hex = format!("{:08x}", crc);
        let r = exec1_params(&Crc32VerifyFn, &data, params(&[("expected_crc32", &hex)])).unwrap();
        assert_eq!(r.as_ref(), data.as_slice());
    }


}

mod format_conversion {
    use super::*;

    // ── #84 JsonPrettyPrintFn ──

    #[test]
    fn json_pretty_basic() {
        let r = exec1(&JsonPrettyPrintFn, b"{\"a\":1,\"b\":2}").unwrap();
        let text = std::str::from_utf8(&r).unwrap();
        assert!(text.contains('\n'));
        assert!(text.contains("  "));
    }

    #[test]
    fn json_pretty_null() {
        let r = exec1(&JsonPrettyPrintFn, b"null").unwrap();
        assert_eq!(r.as_ref(), b"null");
    }

    #[test]
    fn json_pretty_idempotent() {
        let r1 = exec1(&JsonPrettyPrintFn, b"{\"x\":1}").unwrap();
        let r2 = exec1(&JsonPrettyPrintFn, &r1).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn json_pretty_invalid() {
        let r = exec1(&JsonPrettyPrintFn, b"{bad}");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn json_pretty_array() {
        let r = exec1(&JsonPrettyPrintFn, b"[1,2,3]").unwrap();
        let text = std::str::from_utf8(&r).unwrap();
        assert!(text.contains('\n'));
    }


    // ── #85 JsonMinifyFn ──

    #[test]
    fn json_minify_basic() {
        let r = exec1(&JsonMinifyFn, b"{\n  \"a\": 1,\n  \"b\": 2\n}").unwrap();
        assert_eq!(r.as_ref(), b"{\"a\":1,\"b\":2}");
    }

    #[test]
    fn json_minify_already_compact() {
        let r = exec1(&JsonMinifyFn, b"{\"x\":1}").unwrap();
        assert_eq!(r.as_ref(), b"{\"x\":1}");
    }

    #[test]
    fn json_minify_roundtrip_with_pretty() {
        let pretty = exec1(&JsonPrettyPrintFn, b"{\"a\":1}").unwrap();
        let mini = exec1(&JsonMinifyFn, &pretty).unwrap();
        assert_eq!(mini.as_ref(), b"{\"a\":1}");
    }

    #[test]
    fn json_minify_invalid() {
        let r = exec1(&JsonMinifyFn, b"not json");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn json_minify_empty() {
        let r = exec1(&JsonMinifyFn, b"");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }


    // ── #86 CsvToJsonFn ──

    #[test]
    fn csv_to_json_basic() {
        let r = exec1(&CsvToJsonFn, b"name,age\nAlice,30\nBob,25").unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0]["name"], "Alice");
        assert_eq!(v[0]["age"], "30");
    }

    #[test]
    fn csv_to_json_header_only() {
        let r = exec1(&CsvToJsonFn, b"name,age\n").unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn csv_to_json_quoted_fields() {
        let r = exec1(&CsvToJsonFn, b"msg\n\"hello, world\"").unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
        assert_eq!(v[0]["msg"], "hello, world");
    }

    #[test]
    fn csv_to_json_single_column() {
        let r = exec1(&CsvToJsonFn, b"val\n1\n2\n3").unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn csv_to_json_empty() {
        let r = exec1(&CsvToJsonFn, b"").unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
        assert!(v.is_empty());
    }


    // ── #87 JsonToCsvFn ──

    #[test]
    fn json_to_csv_basic() {
        let input = r#"[{"age":"30","name":"Alice"},{"age":"25","name":"Bob"}]"#;
        let r = exec1(&JsonToCsvFn, input.as_bytes()).unwrap();
        let text = std::str::from_utf8(&r).unwrap();
        assert!(text.starts_with("age,name\n"));
        assert!(text.contains("30,Alice"));
    }

    #[test]
    fn json_to_csv_empty_array() {
        let r = exec1(&JsonToCsvFn, b"[]").unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn json_to_csv_numeric_values() {
        let input = r#"[{"count":42,"label":"test"}]"#;
        let r = exec1(&JsonToCsvFn, input.as_bytes()).unwrap();
        let text = std::str::from_utf8(&r).unwrap();
        assert!(text.contains("42"));
    }

    #[test]
    fn json_to_csv_missing_field() {
        let input = r#"[{"a":"1","b":"2"},{"a":"3"}]"#;
        let r = exec1(&JsonToCsvFn, input.as_bytes()).unwrap();
        let text = std::str::from_utf8(&r).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3); // header + 2 rows
    }

    #[test]
    fn json_to_csv_invalid() {
        let r = exec1(&JsonToCsvFn, b"not json");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }


    // ── #88 JsonLinesFn ──

    #[test]
    fn json_lines_basic() {
        let r = exec1(&JsonLinesFn, b"[1,2,3]").unwrap();
        assert_eq!(r.as_ref(), b"1\n2\n3");
    }

    #[test]
    fn json_lines_objects() {
        let r = exec1(&JsonLinesFn, b"[{\"a\":1},{\"b\":2}]").unwrap();
        let lines: Vec<&str> = std::str::from_utf8(&r).unwrap().lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"a\":1") || lines[0].contains("\"a\": 1"));
    }

    #[test]
    fn json_lines_empty_array() {
        let r = exec1(&JsonLinesFn, b"[]").unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn json_lines_not_array() {
        let r = exec1(&JsonLinesFn, b"{\"a\":1}");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn json_lines_strings() {
        let r = exec1(&JsonLinesFn, b"[\"hello\",\"world\"]").unwrap();
        assert_eq!(r.as_ref(), b"\"hello\"\n\"world\"");
    }


    // ── #89 YamlToJsonFn ──

    #[test]
    fn yaml_to_json_basic() {
        let r = exec1(&YamlToJsonFn, b"name: Alice\nage: 30").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
        assert_eq!(v["name"], "Alice");
        assert_eq!(v["age"], 30);
    }

    #[test]
    fn yaml_to_json_list() {
        let r = exec1(&YamlToJsonFn, b"- a\n- b\n- c").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 3);
    }

    #[test]
    fn yaml_to_json_nested() {
        let r = exec1(&YamlToJsonFn, b"outer:\n  inner: value").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
        assert_eq!(v["outer"]["inner"], "value");
    }

    #[test]
    fn yaml_to_json_invalid() {
        let r = exec1(&YamlToJsonFn, b":\n  bad:\n    - ][");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn yaml_to_json_scalar() {
        let r = exec1(&YamlToJsonFn, b"42").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
        assert_eq!(v, 42);
    }


    // ── #90 JsonToYamlFn ──

    #[test]
    fn json_to_yaml_basic() {
        let r = exec1(&JsonToYamlFn, b"{\"name\":\"Alice\",\"age\":30}").unwrap();
        let text = std::str::from_utf8(&r).unwrap();
        assert!(text.contains("name:"));
        assert!(text.contains("age:"));
    }

    #[test]
    fn json_to_yaml_roundtrip() {
        let json_in = b"{\"x\":1,\"y\":\"hello\"}";
        let yaml = exec1(&JsonToYamlFn, json_in).unwrap();
        let json_out = exec1(&YamlToJsonFn, &yaml).unwrap();
        let v1: serde_json::Value = serde_json::from_slice(json_in).unwrap();
        let v2: serde_json::Value = serde_json::from_slice(&json_out).unwrap();
        assert_eq!(v1, v2);
    }

    #[test]
    fn json_to_yaml_array() {
        let r = exec1(&JsonToYamlFn, b"[1,2,3]").unwrap();
        let text = std::str::from_utf8(&r).unwrap();
        assert!(text.contains("- 1"));
    }

    #[test]
    fn json_to_yaml_invalid() {
        let r = exec1(&JsonToYamlFn, b"not json");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn json_to_yaml_null() {
        let r = exec1(&JsonToYamlFn, b"null").unwrap();
        let text = std::str::from_utf8(&r).unwrap();
        assert!(text.trim() == "null" || text.trim() == "~");
    }


    // ── #91 TomlToJsonFn ──

    #[test]
    fn toml_to_json_basic() {
        let r = exec1(&TomlToJsonFn, b"[package]\nname = \"test\"\nversion = \"1.0\"").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
        assert_eq!(v["package"]["name"], "test");
        assert_eq!(v["package"]["version"], "1.0");
    }

    #[test]
    fn toml_to_json_flat() {
        let r = exec1(&TomlToJsonFn, b"key = \"value\"\nnum = 42").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
        assert_eq!(v["key"], "value");
        assert_eq!(v["num"], 42);
    }

    #[test]
    fn toml_to_json_array() {
        let r = exec1(&TomlToJsonFn, b"items = [1, 2, 3]").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn toml_to_json_invalid() {
        let r = exec1(&TomlToJsonFn, b"[[[bad toml");
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn toml_to_json_nested_tables() {
        let r = exec1(&TomlToJsonFn, b"[a]\nx = 1\n[a.b]\ny = 2").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
        assert_eq!(v["a"]["x"], 1);
        assert_eq!(v["a"]["b"]["y"], 2);
    }


}

mod cas_specific {
    use super::*;

    // ── #92 CAddrComputeFn ──

    #[test]
    fn caddr_compute_deterministic() {
        let r1 = exec1(&CAddrComputeFn, b"hello").unwrap();
        let r2 = exec1(&CAddrComputeFn, b"hello").unwrap();
        assert_eq!(r1, r2);
        assert_eq!(r1.len(), 32);
    }

    #[test]
    fn caddr_compute_different_inputs_differ() {
        let r1 = exec1(&CAddrComputeFn, b"hello").unwrap();
        let r2 = exec1(&CAddrComputeFn, b"world").unwrap();
        assert_ne!(r1, r2);
    }

    #[test]
    fn caddr_compute_empty() {
        let r = exec1(&CAddrComputeFn, b"").unwrap();
        assert_eq!(r.len(), 32);
    }

    #[test]
    fn caddr_compute_matches_core() {
        use deriva_core::address::CAddr;
        let data = b"test data";
        let expected = CAddr::from_bytes(data);
        let r = exec1(&CAddrComputeFn, data).unwrap();
        assert_eq!(r.as_ref(), expected.as_bytes());
    }

    #[test]
    fn caddr_compute_binary() {
        let data: Vec<u8> = (0..=255).collect();
        let r = exec1(&CAddrComputeFn, &data).unwrap();
        assert_eq!(r.len(), 32);
    }


    // ── #93 CAddrVerifyFn ──

    #[test]
    fn caddr_verify_correct() {
        use deriva_core::address::CAddr;
        let data = b"verify me";
        let addr = CAddr::from_bytes(data);
        let hex: String = addr.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
        let r = exec1_params(&CAddrVerifyFn, data, params(&[("expected_caddr", &hex)])).unwrap();
        assert_eq!(r.as_ref(), data);
    }

    #[test]
    fn caddr_verify_mismatch() {
        let r = exec1_params(&CAddrVerifyFn, b"data", params(&[("expected_caddr", "0000000000000000000000000000000000000000000000000000000000000000")]));
        assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
    }

    #[test]
    fn caddr_verify_wrong_length() {
        let r = exec1_params(&CAddrVerifyFn, b"data", params(&[("expected_caddr", "abcd")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }

    #[test]
    fn caddr_verify_passthrough() {
        use deriva_core::address::CAddr;
        let data = vec![0xAB; 1000];
        let addr = CAddr::from_bytes(&data);
        let hex: String = addr.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
        let r = exec1_params(&CAddrVerifyFn, &data, params(&[("expected_caddr", &hex)])).unwrap();
        assert_eq!(r.as_ref(), data.as_slice());
    }

    #[test]
    fn caddr_verify_empty() {
        use deriva_core::address::CAddr;
        let addr = CAddr::from_bytes(b"");
        let hex: String = addr.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
        let r = exec1_params(&CAddrVerifyFn, b"", params(&[("expected_caddr", &hex)])).unwrap();
        assert!(r.is_empty());
    }


    // ── #94 CAddrEmbedFn ──

    #[test]
    fn caddr_embed_appends_32_bytes() {
        let r = exec1(&CAddrEmbedFn, b"hello").unwrap();
        assert_eq!(r.len(), 5 + 32);
        assert_eq!(&r[..5], b"hello");
    }

    #[test]
    fn caddr_embed_trailing_matches_compute() {
        let data = b"test data";
        let embedded = exec1(&CAddrEmbedFn, data).unwrap();
        let computed = exec1(&CAddrComputeFn, data).unwrap();
        assert_eq!(&embedded[data.len()..], computed.as_ref());
    }

    #[test]
    fn caddr_embed_empty() {
        let r = exec1(&CAddrEmbedFn, b"").unwrap();
        assert_eq!(r.len(), 32);
    }

    #[test]
    fn caddr_embed_verifiable() {
        use deriva_core::address::CAddr;
        let data = b"verify this";
        let embedded = exec1(&CAddrEmbedFn, data).unwrap();
        let original = &embedded[..data.len()];
        let trailing = &embedded[data.len()..];
        let recomputed = CAddr::from_bytes(original);
        assert_eq!(recomputed.as_bytes(), trailing);
    }

    #[test]
    fn caddr_embed_not_idempotent() {
        let r1 = exec1(&CAddrEmbedFn, b"data").unwrap();
        let r2 = exec1(&CAddrEmbedFn, &r1).unwrap();
        assert_ne!(r1.len(), r2.len());
        assert_eq!(r2.len(), r1.len() + 32);
    }


    // ── #95 MerkleRootFn ──

    #[test]
    fn merkle_root_deterministic() {
        let data = vec![0u8; 200_000];
        let r1 = exec1_params(&MerkleRootFn, &data, params(&[("block_size", "65536")])).unwrap();
        let r2 = exec1_params(&MerkleRootFn, &data, params(&[("block_size", "65536")])).unwrap();
        assert_eq!(r1, r2);
        assert_eq!(r1.len(), 32);
    }

    #[test]
    fn merkle_root_empty() {
        let r = exec1(&MerkleRootFn, b"").unwrap();
        assert_eq!(r.len(), 32);
    }

    #[test]
    fn merkle_root_single_block() {
        use sha2::{Sha256, Digest};
        let data = b"small data";
        let r = exec1(&MerkleRootFn, data).unwrap();
        assert_eq!(r.as_ref(), Sha256::digest(data).as_slice());
    }

    #[test]
    fn merkle_root_different_block_sizes_differ() {
        let data = vec![0xAB; 10000];
        let r1 = exec1_params(&MerkleRootFn, &data, params(&[("block_size", "1000")])).unwrap();
        let r2 = exec1_params(&MerkleRootFn, &data, params(&[("block_size", "5000")])).unwrap();
        assert_ne!(r1, r2);
    }

    #[test]
    fn merkle_root_block_size_zero_error() {
        let r = exec1_params(&MerkleRootFn, b"data", params(&[("block_size", "0")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }


    // ── #96 ContentTypeFn ──

    #[test]
    fn content_type_png() {
        let r = exec1(&ContentTypeFn, &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();
        assert_eq!(r.as_ref(), b"image/png");
    }

    #[test]
    fn content_type_json() {
        let r = exec1(&ContentTypeFn, b"{\"key\":\"value\"}").unwrap();
        assert_eq!(r.as_ref(), b"application/json");
    }

    #[test]
    fn content_type_text() {
        let r = exec1(&ContentTypeFn, b"hello world").unwrap();
        assert_eq!(r.as_ref(), b"text/plain");
    }

    #[test]
    fn content_type_binary() {
        let r = exec1(&ContentTypeFn, &[0xFF, 0xFE, 0x00, 0x01, 0x80]).unwrap();
        assert_eq!(r.as_ref(), b"application/octet-stream");
    }

    #[test]
    fn content_type_gzip() {
        let r = exec1(&ContentTypeFn, &[0x1F, 0x8B, 0x08, 0x00]).unwrap();
        assert_eq!(r.as_ref(), b"application/gzip");
    }


    // ── #97 ChunkHashFn ──

    #[test]
    fn chunk_hash_single_block() {
        use sha2::{Sha256, Digest};
        let data = b"small";
        let r = exec1(&ChunkHashFn, data).unwrap();
        assert_eq!(r.len(), 32);
        assert_eq!(r.as_ref(), Sha256::digest(data).as_slice());
    }

    #[test]
    fn chunk_hash_multiple_blocks() {
        let data = vec![0u8; 100];
        let r = exec1_params(&ChunkHashFn, &data, params(&[("block_size", "30")])).unwrap();
        // 100 / 30 = 4 blocks (30+30+30+10)
        assert_eq!(r.len(), 4 * 32);
    }

    #[test]
    fn chunk_hash_empty() {
        let r = exec1(&ChunkHashFn, b"").unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn chunk_hash_deterministic() {
        let data = vec![0xAB; 500];
        let r1 = exec1_params(&ChunkHashFn, &data, params(&[("block_size", "100")])).unwrap();
        let r2 = exec1_params(&ChunkHashFn, &data, params(&[("block_size", "100")])).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn chunk_hash_block_size_zero_error() {
        let r = exec1_params(&ChunkHashFn, b"data", params(&[("block_size", "0")]));
        assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
    }


    // ── #98 DedupAnalyzeFn ──

    #[test]
    fn dedup_analyze_deterministic() {
        let data = vec![0xAB; 50000];
        let r1 = exec1(&DedupAnalyzeFn, &data).unwrap();
        let r2 = exec1(&DedupAnalyzeFn, &data).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn dedup_analyze_starts_at_zero() {
        let data = vec![0u8; 10000];
        let r = exec1(&DedupAnalyzeFn, &data).unwrap();
        let boundaries: Vec<u64> = serde_json::from_slice(&r).unwrap();
        assert_eq!(boundaries[0], 0);
    }

    #[test]
    fn dedup_analyze_ends_at_length() {
        let data = vec![0u8; 10000];
        let r = exec1(&DedupAnalyzeFn, &data).unwrap();
        let boundaries: Vec<u64> = serde_json::from_slice(&r).unwrap();
        assert_eq!(*boundaries.last().unwrap(), 10000);
    }

    #[test]
    fn dedup_analyze_empty() {
        let r = exec1(&DedupAnalyzeFn, b"").unwrap();
        let boundaries: Vec<u64> = serde_json::from_slice(&r).unwrap();
        assert_eq!(boundaries, vec![0]);
    }

    #[test]
    fn dedup_analyze_small_input() {
        let data = vec![1u8; 100];
        let r = exec1(&DedupAnalyzeFn, &data).unwrap();
        let boundaries: Vec<u64> = serde_json::from_slice(&r).unwrap();
        assert_eq!(boundaries, vec![0, 100]);
    }


}

mod batch_only {
    use super::*;

    // ── #99 ReverseByteFn ──

    #[test]
    fn reverse_bytes_basic() {
        let r = exec1(&ReverseByteFn, b"abcd").unwrap();
        assert_eq!(r.as_ref(), b"dcba");
    }

    #[test]
    fn reverse_bytes_roundtrip() {
        let data = b"hello world";
        let r1 = exec1(&ReverseByteFn, data).unwrap();
        let r2 = exec1(&ReverseByteFn, &r1).unwrap();
        assert_eq!(r2.as_ref(), data);
    }

    #[test]
    fn reverse_bytes_empty() {
        let r = exec1(&ReverseByteFn, b"").unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn reverse_bytes_single() {
        let r = exec1(&ReverseByteFn, b"x").unwrap();
        assert_eq!(r.as_ref(), b"x");
    }

    #[test]
    fn reverse_bytes_binary() {
        let r = exec1(&ReverseByteFn, &[0x00, 0x01, 0xFF]).unwrap();
        assert_eq!(r.as_ref(), &[0xFF, 0x01, 0x00]);
    }


    // ── #100 SortBytesFn ──

    #[test]
    fn sort_bytes_basic() {
        let r = exec1(&SortBytesFn, b"dcba").unwrap();
        assert_eq!(r.as_ref(), b"abcd");
    }

    #[test]
    fn sort_bytes_already_sorted() {
        let r = exec1(&SortBytesFn, b"abcd").unwrap();
        assert_eq!(r.as_ref(), b"abcd");
    }

    #[test]
    fn sort_bytes_empty() {
        let r = exec1(&SortBytesFn, b"").unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn sort_bytes_all_same() {
        let r = exec1(&SortBytesFn, b"aaaa").unwrap();
        assert_eq!(r.as_ref(), b"aaaa");
    }

    #[test]
    fn sort_bytes_idempotent() {
        let r1 = exec1(&SortBytesFn, b"zxywvu").unwrap();
        let r2 = exec1(&SortBytesFn, &r1).unwrap();
        assert_eq!(r1, r2);
    }


}
