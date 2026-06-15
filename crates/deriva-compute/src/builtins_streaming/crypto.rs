//! §3.1 Streaming Cryptography & Security (#21–#29)

use std::collections::HashMap;
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::{StreamingComputeFunction, DEFAULT_CHANNEL_CAPACITY};
use super::core::{spawn_map_stateful, spawn_accumulate, take_one};

fn hex_decode(s: &str, name: &str) -> Result<Vec<u8>, String> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| format!("{}: invalid hex", name)))
        .collect()
}

fn get_param<'a>(params: &'a HashMap<String, String>, key: &str) -> Result<&'a str, String> {
    params.get(key).map(|s| s.as_str()).ok_or_else(|| format!("missing param: {}", key))
}

// ── #21 StreamingEncrypt (AES-256-CTR) ──

pub struct StreamingEncrypt;

#[async_trait]
impl StreamingComputeFunction for StreamingEncrypt {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "encrypt");
        let (key, nonce) = match parse_aes_ctr_params(params) {
            Ok(v) => v,
            Err(e) => return error_stream(e).await,
        };
        use aes::Aes256;
        use ctr::cipher::{KeyIvInit, StreamCipher};
        type Aes256Ctr = ctr::Ctr64BE<Aes256>;
        let mut cipher = Aes256Ctr::new(key[..].into(), nonce[..].into());
        spawn_map_stateful(rx, DEFAULT_CHANNEL_CAPACITY, move |b| {
            let mut buf = b.to_vec();
            cipher.apply_keystream(&mut buf);
            Ok(Bytes::from(buf))
        })
    }
}

// ── #22 StreamingDecrypt (AES-256-CTR, symmetric) ──

pub struct StreamingDecrypt;

#[async_trait]
impl StreamingComputeFunction for StreamingDecrypt {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "decrypt");
        let (key, nonce) = match parse_aes_ctr_params(params) {
            Ok(v) => v,
            Err(e) => return error_stream(e).await,
        };
        use aes::Aes256;
        use ctr::cipher::{KeyIvInit, StreamCipher};
        type Aes256Ctr = ctr::Ctr64BE<Aes256>;
        let mut cipher = Aes256Ctr::new(key[..].into(), nonce[..].into());
        spawn_map_stateful(rx, DEFAULT_CHANNEL_CAPACITY, move |b| {
            let mut buf = b.to_vec();
            cipher.apply_keystream(&mut buf);
            Ok(Bytes::from(buf))
        })
    }
}

fn parse_aes_ctr_params(params: &HashMap<String, String>) -> Result<(Vec<u8>, Vec<u8>), String> {
    let key = hex_decode(get_param(params, "key")?, "key")?;
    let nonce = hex_decode(get_param(params, "nonce")?, "nonce")?;
    if key.len() != 32 { return Err("key must be 32 bytes (64 hex chars)".into()); }
    if nonce.len() != 16 { return Err("nonce must be 16 bytes (32 hex chars)".into()); }
    Ok((key, nonce))
}

// ── #23 StreamingAeadEncrypt (AES-256-GCM, per-chunk) ──

pub struct StreamingAeadEncrypt;

#[async_trait]
impl StreamingComputeFunction for StreamingAeadEncrypt {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "aead_encrypt");
        let (key, nonce_prefix) = match parse_aead_params(params) {
            Ok(v) => v,
            Err(e) => return error_stream(e).await,
        };
        use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead, Nonce};
        let cipher = Aes256Gcm::new(key[..].into());
        let mut chunk_idx: u32 = 0;
        spawn_map_stateful(rx, DEFAULT_CHANNEL_CAPACITY, move |b| {
            let mut nonce_bytes = [0u8; 12];
            nonce_bytes[..8].copy_from_slice(&nonce_prefix);
            nonce_bytes[8..12].copy_from_slice(&chunk_idx.to_be_bytes());
            let nonce = Nonce::from(nonce_bytes);
            chunk_idx += 1;
            cipher.encrypt(&nonce, b)
                .map(Bytes::from)
                .map_err(|e| format!("aead encrypt: {}", e))
        })
    }
}

// ── #24 StreamingAeadDecrypt (AES-256-GCM, per-chunk) ──

pub struct StreamingAeadDecrypt;

#[async_trait]
impl StreamingComputeFunction for StreamingAeadDecrypt {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "aead_decrypt");
        let (key, nonce_prefix) = match parse_aead_params(params) {
            Ok(v) => v,
            Err(e) => return error_stream(e).await,
        };
        use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead, Nonce};
        let cipher = Aes256Gcm::new(key[..].into());
        let mut chunk_idx: u32 = 0;
        spawn_map_stateful(rx, DEFAULT_CHANNEL_CAPACITY, move |b| {
            let mut nonce_bytes = [0u8; 12];
            nonce_bytes[..8].copy_from_slice(&nonce_prefix);
            nonce_bytes[8..12].copy_from_slice(&chunk_idx.to_be_bytes());
            let nonce = Nonce::from(nonce_bytes);
            chunk_idx += 1;
            cipher.decrypt(&nonce, b)
                .map(Bytes::from)
                .map_err(|_e| format!("aead: authentication failed at chunk {}", chunk_idx - 1))
        })
    }
}

fn parse_aead_params(params: &HashMap<String, String>) -> Result<(Vec<u8>, [u8; 8]), String> {
    let key = hex_decode(get_param(params, "key")?, "key")?;
    let nonce_prefix = hex_decode(get_param(params, "nonce_prefix")?, "nonce_prefix")?;
    if key.len() != 32 { return Err("key must be 32 bytes (64 hex chars)".into()); }
    if nonce_prefix.len() != 8 { return Err("nonce_prefix must be 8 bytes (16 hex chars)".into()); }
    let mut np = [0u8; 8];
    np.copy_from_slice(&nonce_prefix);
    Ok((key, np))
}

// ── #25 StreamingHmacSha256 ──

pub struct StreamingHmacSha256;

#[async_trait]
impl StreamingComputeFunction for StreamingHmacSha256 {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "hmac_sha256");
        let key = match get_param(params, "key").and_then(|k| hex_decode(k, "key")) {
            Ok(v) => v,
            Err(e) => return error_stream(e).await,
        };
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mac = match HmacSha256::new_from_slice(&key) {
            Ok(m) => m,
            Err(e) => return error_stream(format!("hmac init: {}", e)).await,
        };
        spawn_accumulate(rx, mac, |m, b| m.update(b), |m| {
            Bytes::copy_from_slice(&m.finalize().into_bytes())
        })
    }
}

// ── #26 StreamingMd5 ──

pub struct StreamingMd5;

#[async_trait]
impl StreamingComputeFunction for StreamingMd5 {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        use md5::Digest;
        let rx = take_one(&mut inputs, "md5");
        spawn_accumulate(rx, md5::Md5::new(), |h, b| h.update(b), |h| {
            Bytes::copy_from_slice(&h.finalize())
        })
    }
}

// ── #27 StreamingSha512 ──

pub struct StreamingSha512;

#[async_trait]
impl StreamingComputeFunction for StreamingSha512 {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        use sha2::{Sha512, Digest};
        let rx = take_one(&mut inputs, "sha512");
        spawn_accumulate(rx, Sha512::new(), |h, b| h.update(b), |h| {
            Bytes::copy_from_slice(&h.finalize())
        })
    }
}

// ── #28 StreamingBlake3 ──

pub struct StreamingBlake3;

#[async_trait]
impl StreamingComputeFunction for StreamingBlake3 {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "blake3");
        spawn_accumulate(rx, blake3::Hasher::new(), |h, b| { h.update(b); }, |h| {
            Bytes::copy_from_slice(h.finalize().as_bytes())
        })
    }
}

// ── #29 StreamingRedact ──

pub struct StreamingRedact;

#[async_trait]
impl StreamingComputeFunction for StreamingRedact {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "redact");
        let pattern_str = params.get("patterns").map(|s| s.as_str()).unwrap_or("default");
        let patterns: Vec<String> = if pattern_str == "default" {
            vec![
                r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b".into(), // email
                r"\b\d{3}[-.]?\d{3}[-.]?\d{4}\b".into(),                         // phone
                r"\b\d{3}-\d{2}-\d{4}\b".into(),                                 // SSN
            ]
        } else {
            pattern_str.split(',').map(|s| s.trim().to_string()).collect()
        };
        let regexes: Vec<regex::Regex> = match patterns.iter()
            .map(|p| regex::Regex::new(p).map_err(|e| format!("redact: invalid regex '{}': {}", p, e)))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(v) => v,
            Err(e) => return error_stream(e).await,
        };
        super::core::spawn_boundary_map(rx, DEFAULT_CHANNEL_CAPACITY, move |chunk| {
            let mut text = String::from_utf8_lossy(chunk).into_owned();
            for re in &regexes {
                text = re.replace_all(&text, "[REDACTED]").into_owned();
            }
            Ok(Bytes::from(text))
        })
    }
}

#[allow(dead_code)]
fn spawn_map(
    rx: mpsc::Receiver<StreamChunk>,
    cap: usize,
    f: impl Fn(&[u8]) -> Result<Bytes, String> + Send + 'static,
) -> mpsc::Receiver<StreamChunk> {
    super::core::spawn_map(rx, cap, f)
}

async fn error_stream(msg: String) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(1);
    let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(msg))).await;
    rx
}
