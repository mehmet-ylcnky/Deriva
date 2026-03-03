use std::collections::HashMap;
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::StreamingComputeFunction;
use super::core::{take_one, spawn_buffered};

fn error_stream(msg: String) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(async move {
        let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(msg))).await;
    });
    rx
}

fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 { return Err("odd-length hex".into()); }
    (0..hex.len()).step_by(2).map(|i| {
        u8::from_str_radix(&hex[i..i+2], 16).map_err(|_| "invalid hex".to_string())
    }).collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── #71 StreamingJsonValidate ────────────────────────────────────────

pub struct StreamingJsonValidate;

#[async_trait]
impl StreamingComputeFunction for StreamingJsonValidate {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingJsonValidate");
        spawn_buffered(rx, 2, 64 * 1024, |buf| {
            serde_json::from_slice::<serde_json::Value>(&buf)
                .map_err(|e| format!("json: {e}"))?;
            Ok(buf) // emit original bytes unchanged
        })
    }
}

// ── #72 StreamingSchemaValidate ──────────────────────────────────────

pub struct StreamingSchemaValidate;

fn validate_schema(val: &serde_json::Value, schema: &serde_json::Value) -> Result<(), String> {
    if let Some(ty) = schema.get("type").and_then(|t| t.as_str()) {
        let ok = match ty {
            "object" => val.is_object(),
            "array" => val.is_array(),
            "string" => val.is_string(),
            "number" | "integer" => val.is_number(),
            "boolean" => val.is_boolean(),
            "null" => val.is_null(),
            _ => true,
        };
        if !ok { return Err(format!("expected type {ty}")); }
    }
    if let Some(req) = schema.get("required").and_then(|r| r.as_array()) {
        if let Some(obj) = val.as_object() {
            for field in req {
                if let Some(name) = field.as_str() {
                    if !obj.contains_key(name) { return Err(format!("missing required field: {name}")); }
                }
            }
        }
    }
    if let Some(enm) = schema.get("enum").and_then(|e| e.as_array()) {
        if !enm.contains(val) { return Err(format!("value not in enum")); }
    }
    Ok(())
}

#[async_trait]
impl StreamingComputeFunction for StreamingSchemaValidate {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingSchemaValidate");
        let schema_str = match params.get("schema") {
            Some(s) => s.clone(),
            None => return error_stream("missing param: schema".into()),
        };
        spawn_buffered(rx, 2, 64 * 1024, move |buf| {
            let val: serde_json::Value = serde_json::from_slice(&buf)
                .map_err(|e| format!("json: {e}"))?;
            let schema: serde_json::Value = serde_json::from_str(&schema_str)
                .map_err(|e| format!("schema: {e}"))?;
            validate_schema(&val, &schema)?;
            Ok(buf)
        })
    }
}

// ── #73 StreamingMagicBytes ──────────────────────────────────────────

pub struct StreamingMagicBytes;

#[async_trait]
impl StreamingComputeFunction for StreamingMagicBytes {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingMagicBytes");
        let expected_hex = match params.get("expected") {
            Some(s) => s.clone(),
            None => return error_stream("missing param: expected".into()),
        };
        let expected = match hex_to_bytes(&expected_hex) {
            Ok(b) => b,
            Err(e) => return error_stream(e),
        };
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut checked = false;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        if !checked {
                            checked = true;
                            if !chunk.starts_with(&expected) {
                                let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(
                                    format!("magic bytes mismatch: expected {expected_hex}")
                                ))).await;
                                return;
                            }
                        }
                        if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
                    }
                    Some(StreamChunk::End) | None => { let _ = tx.send(StreamChunk::End).await; return; }
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                }
            }
        });
        out
    }
}

// ── #74 StreamingSizeLimit ───────────────────────────────────────────

pub struct StreamingSizeLimit;

#[async_trait]
impl StreamingComputeFunction for StreamingSizeLimit {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingSizeLimit");
        let max: u64 = match params.get("max_bytes").and_then(|v| v.parse().ok()) {
            Some(n) => n,
            None => return error_stream("missing param: max_bytes".into()),
        };
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut total = 0u64;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        total += chunk.len() as u64;
                        if total > max {
                            let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(
                                format!("size limit exceeded: {total} > {max}")
                            ))).await;
                            return;
                        }
                        if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
                    }
                    Some(StreamChunk::End) | None => { let _ = tx.send(StreamChunk::End).await; return; }
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                }
            }
        });
        out
    }
}

// ── #75 StreamingChecksumVerify ──────────────────────────────────────

pub struct StreamingChecksumVerify;

#[async_trait]
impl StreamingComputeFunction for StreamingChecksumVerify {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingChecksumVerify");
        let expected = match params.get("expected_crc32") {
            Some(s) => s.clone(),
            None => return error_stream("missing param: expected_crc32".into()),
        };
        if expected.len() != 8 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            return error_stream(format!("checksum: invalid hex in expected_crc32: '{}'", expected));
        }
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut hasher = crc32fast::Hasher::new();
            let mut forwarded = Vec::new();
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        hasher.update(&chunk);
                        forwarded.push(chunk);
                    }
                    Some(StreamChunk::End) | None => break,
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                }
            }
            let actual = format!("{:08x}", hasher.finalize());
            if actual != expected {
                let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(
                    format!("crc32 mismatch: expected {expected}, got {actual}")
                ))).await;
                return;
            }
            for chunk in forwarded {
                if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}

// ── #76 StreamingSha256Verify ────────────────────────────────────────

pub struct StreamingSha256Verify;

#[async_trait]
impl StreamingComputeFunction for StreamingSha256Verify {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingSha256Verify");
        let expected = match params.get("expected_hash") {
            Some(s) => s.clone(),
            None => return error_stream("missing param: expected_hash".into()),
        };
        if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            return error_stream(format!("sha256_verify: expected 64 hex chars, got {}", expected.len()));
        }
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            let mut forwarded = Vec::new();
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        hasher.update(&chunk);
                        forwarded.push(chunk);
                    }
                    Some(StreamChunk::End) | None => break,
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                }
            }
            let actual = to_hex(&hasher.finalize());
            if actual != expected {
                let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(
                    format!("sha256 mismatch: expected {expected}, got {actual}")
                ))).await;
                return;
            }
            for chunk in forwarded {
                if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}

// ── #77 StreamingNonEmpty ────────────────────────────────────────────

pub struct StreamingNonEmpty;

#[async_trait]
impl StreamingComputeFunction for StreamingNonEmpty {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingNonEmpty");
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut saw_data = false;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        if !chunk.is_empty() { saw_data = true; }
                        if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
                    }
                    Some(StreamChunk::End) | None => {
                        if !saw_data {
                            let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed("stream is empty".into()))).await;
                        } else {
                            let _ = tx.send(StreamChunk::End).await;
                        }
                        return;
                    }
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                }
            }
        });
        out
    }
}
