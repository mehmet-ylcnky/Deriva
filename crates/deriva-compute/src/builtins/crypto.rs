use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;
use super::{get_string_param, hex_decode_param};

pub struct Sha256Fn;

impl ComputeFunction for Sha256Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("sha256", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use sha2::{Sha256, Digest};
        let hash = Sha256::digest(&inputs[0]);
        Ok(Bytes::copy_from_slice(&hash))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #32 Sha512Fn ──

pub struct Sha512Fn;

impl ComputeFunction for Sha512Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("sha512", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use sha2::{Sha512, Digest};
        let hash = Sha512::digest(&inputs[0]);
        Ok(Bytes::copy_from_slice(&hash))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #33 Md5Fn ──

pub struct Md5Fn;

impl ComputeFunction for Md5Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("md5", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use md5::{Md5, Digest};
        let hash = Md5::digest(&inputs[0]);
        Ok(Bytes::copy_from_slice(&hash))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #34 Blake3Fn ──

pub struct Blake3Fn;

impl ComputeFunction for Blake3Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("blake3", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let hash = blake3::hash(&inputs[0]);
        Ok(Bytes::copy_from_slice(hash.as_bytes()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #35 HmacSha256Fn ──

pub struct HmacSha256Fn;

impl ComputeFunction for HmacSha256Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("hmac_sha256", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let key_hex = get_string_param(params, "key")?;
        let key = hex_decode_param(key_hex, "key")?;
        let mut mac = HmacSha256::new_from_slice(&key)
            .map_err(|e| ComputeError::ExecutionFailed(format!("hmac init: {}", e)))?;
        mac.update(&inputs[0]);
        let result = mac.finalize();
        Ok(Bytes::copy_from_slice(&result.into_bytes()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #36 Crc32Fn ──

pub struct Crc32Fn;

impl ComputeFunction for Crc32Fn {
    fn id(&self) -> FunctionId {
        FunctionId::new("crc32", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let crc = crc32fast::hash(&inputs[0]);
        Ok(Bytes::copy_from_slice(&crc.to_be_bytes()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #37 EncryptFn (AES-256-CTR) ──

pub struct EncryptFn;

impl ComputeFunction for EncryptFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("encrypt", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use aes::Aes256;
        use ctr::cipher::{KeyIvInit, StreamCipher};
        type Aes256Ctr = ctr::Ctr64BE<Aes256>;

        let key = hex_decode_param(get_string_param(params, "key")?, "key")?;
        let nonce = hex_decode_param(get_string_param(params, "nonce")?, "nonce")?;
        if key.len() != 32 {
            return Err(ComputeError::InvalidParam("key must be 32 bytes (64 hex chars)".into()));
        }
        if nonce.len() != 16 {
            return Err(ComputeError::InvalidParam("nonce must be 16 bytes (32 hex chars)".into()));
        }
        let mut cipher = Aes256Ctr::new(key[..].into(), nonce[..].into());
        let mut buf = inputs[0].to_vec();
        cipher.apply_keystream(&mut buf);
        Ok(Bytes::from(buf))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(200, input_sizes) }
}

// ── #38 DecryptFn (AES-256-CTR) ──

pub struct DecryptFn;

impl ComputeFunction for DecryptFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("decrypt", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        EncryptFn.execute(inputs, params)
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(200, input_sizes) }
}

// ── #39 AeadEncryptFn (AES-256-GCM) ──

pub struct AeadEncryptFn;

impl ComputeFunction for AeadEncryptFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("aead_encrypt", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead, Nonce};

        let key = hex_decode_param(get_string_param(params, "key")?, "key")?;
        let nonce_bytes = hex_decode_param(get_string_param(params, "nonce")?, "nonce")?;
        if key.len() != 32 {
            return Err(ComputeError::InvalidParam("key must be 32 bytes (64 hex chars)".into()));
        }
        if nonce_bytes.len() != 12 {
            return Err(ComputeError::InvalidParam("nonce must be 12 bytes (24 hex chars)".into()));
        }
        let cipher = Aes256Gcm::new(key[..].into());
        let nonce = Nonce::from_slice(&nonce_bytes);
        cipher.encrypt(nonce, inputs[0].as_ref())
            .map(Bytes::from)
            .map_err(|e| ComputeError::ExecutionFailed(format!("aead encrypt: {}", e)))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(200, input_sizes) }
}

// ── #40 AeadDecryptFn (AES-256-GCM) ──

pub struct AeadDecryptFn;

impl ComputeFunction for AeadDecryptFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("aead_decrypt", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead, Nonce};

        let key = hex_decode_param(get_string_param(params, "key")?, "key")?;
        let nonce_bytes = hex_decode_param(get_string_param(params, "nonce")?, "nonce")?;
        if key.len() != 32 {
            return Err(ComputeError::InvalidParam("key must be 32 bytes (64 hex chars)".into()));
        }
        if nonce_bytes.len() != 12 {
            return Err(ComputeError::InvalidParam("nonce must be 12 bytes (24 hex chars)".into()));
        }
        if inputs[0].len() < 16 {
            return Err(ComputeError::ExecutionFailed("ciphertext too short (missing tag)".into()));
        }
        let cipher = Aes256Gcm::new(key[..].into());
        let nonce = Nonce::from_slice(&nonce_bytes);
        cipher.decrypt(nonce, inputs[0].as_ref())
            .map(Bytes::from)
            .map_err(|_| ComputeError::ExecutionFailed("aead decrypt: authentication failed".into()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(200, input_sizes) }
}

// ── #41 RedactFn ──

pub struct RedactFn;

impl ComputeFunction for RedactFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("redact", "1.0.0")
    }

    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let patterns_str = get_string_param(params, "patterns")?;
        let input_str = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("redact requires UTF-8 input".into()))?;
        let mut result = input_str.to_string();
        for pattern in patterns_str.split(',') {
            let re = regex::Regex::new(pattern.trim())
                .map_err(|e| ComputeError::InvalidParam(format!("invalid regex '{}': {}", pattern, e)))?;
            result = re.replace_all(&result, "[REDACTED]").into_owned();
        }
        Ok(Bytes::from(result))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(50, input_sizes) }
}

// ── #42 ByteCountFn ──

