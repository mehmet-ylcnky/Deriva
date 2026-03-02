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


