use std::collections::HashMap;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use sha2::{Sha256, Digest};
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::StreamingComputeFunction;
use super::core::{take_one, spawn_accumulate, spawn_map};

fn error_stream(msg: String) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(async move {
        let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(msg))).await;
    });
    rx
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── #86 StreamingCAddrEmbed ──────────────────────────────────────────

pub struct StreamingCAddrEmbed;

#[async_trait]
impl StreamingComputeFunction for StreamingCAddrEmbed {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingCAddrEmbed");
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut hasher = Sha256::new();
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        hasher.update(&chunk);
                        if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
                    }
                    Some(StreamChunk::End) | None => {
                        let hash = hasher.finalize();
                        let _ = tx.send(StreamChunk::Data(Bytes::copy_from_slice(&hash))).await;
                        let _ = tx.send(StreamChunk::End).await;
                        return;
                    }
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                }
            }
        });
        out
    }
}

// ── #87 StreamingCAddrVerify ─────────────────────────────────────────

pub struct StreamingCAddrVerify;

#[async_trait]
impl StreamingComputeFunction for StreamingCAddrVerify {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingCAddrVerify");
        let expected = match params.get("expected_caddr") {
            Some(s) => s.clone(),
            None => return error_stream("missing param: expected_caddr".into()),
        };
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
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
                    format!("caddr mismatch: expected {expected}, got {actual}")
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

// ── #88 StreamingDiff ────────────────────────────────────────────────

pub struct StreamingDiff;

#[async_trait]
impl StreamingComputeFunction for StreamingDiff {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        if inputs.len() < 2 { return error_stream("StreamingDiff requires 2 inputs".into()); }
        let mut rx_b = inputs.remove(1);
        let mut rx_a = inputs.remove(0);
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            loop {
                let a = rx_a.recv().await;
                let b = rx_b.recv().await;
                match (a, b) {
                    (Some(StreamChunk::Data(ca)), Some(StreamChunk::Data(cb))) => {
                        let len = ca.len().max(cb.len());
                        let mut xor = Vec::with_capacity(len);
                        for i in 0..len {
                            let ba = if i < ca.len() { ca[i] } else { 0 };
                            let bb = if i < cb.len() { cb[i] } else { 0 };
                            xor.push(ba ^ bb);
                        }
                        if tx.send(StreamChunk::Data(Bytes::from(xor))).await.is_err() { return; }
                    }
                    (Some(StreamChunk::Data(ca)), Some(StreamChunk::End) | None) => {
                        // b ended, pad with zeros = just emit a
                        if tx.send(StreamChunk::Data(ca)).await.is_err() { return; }
                    }
                    (Some(StreamChunk::End) | None, Some(StreamChunk::Data(cb))) => {
                        if tx.send(StreamChunk::Data(cb)).await.is_err() { return; }
                    }
                    (Some(StreamChunk::Error(e)), _) | (_, Some(StreamChunk::Error(e))) => {
                        let _ = tx.send(StreamChunk::Error(e)).await; return;
                    }
                    _ => { let _ = tx.send(StreamChunk::End).await; return; }
                }
            }
        });
        out
    }
}

// ── #89 StreamingPatch ───────────────────────────────────────────────

pub struct StreamingPatch;

#[async_trait]
impl StreamingComputeFunction for StreamingPatch {
    async fn stream_execute(&self, inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        // Patch = base XOR diff, same as Diff
        StreamingDiff.stream_execute(inputs, params).await
    }
}

// ── #90 StreamingMerkleTree ──────────────────────────────────────────

pub struct StreamingMerkleTree;

#[async_trait]
impl StreamingComputeFunction for StreamingMerkleTree {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingMerkleTree");
        spawn_accumulate(rx, Vec::<[u8; 32]>::new(),
            |leaves, chunk| {
                let hash: [u8; 32] = Sha256::digest(chunk).into();
                leaves.push(hash);
            },
            |leaves| {
                if leaves.is_empty() {
                    return Bytes::from(vec![0u8; 32]);
                }
                let mut level = leaves;
                while level.len() > 1 {
                    let mut next = Vec::with_capacity((level.len() + 1) / 2);
                    for pair in level.chunks(2) {
                        let mut h = Sha256::new();
                        h.update(pair[0]);
                        if pair.len() > 1 { h.update(pair[1]); } else { h.update(pair[0]); }
                        next.push(h.finalize().into());
                    }
                    level = next;
                }
                Bytes::copy_from_slice(&level[0])
            })
    }
}

// ── #91 StreamingContentType ─────────────────────────────────────────

pub struct StreamingContentType;

fn detect_mime(data: &[u8]) -> &'static str {
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) { return "image/png"; }
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) { return "image/jpeg"; }
    if data.starts_with(&[0x47, 0x49, 0x46, 0x38]) { return "image/gif"; }
    if data.starts_with(&[0x25, 0x50, 0x44, 0x46]) { return "application/pdf"; }
    if data.starts_with(&[0x50, 0x4B, 0x03, 0x04]) { return "application/zip"; }
    if data.starts_with(&[0x1F, 0x8B]) { return "application/gzip"; }
    if data.first() == Some(&0x7B) { return "application/json"; }
    "application/octet-stream"
}

#[async_trait]
impl StreamingComputeFunction for StreamingContentType {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingContentType");
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut detected = false;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        if !detected {
                            detected = true;
                            let mime = detect_mime(&chunk);
                            let header = format!("Content-Type: {mime}\n");
                            if tx.send(StreamChunk::Data(Bytes::from(header))).await.is_err() { return; }
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

// ── #92 StreamingChunkHash ───────────────────────────────────────────

pub struct StreamingChunkHash;

#[async_trait]
impl StreamingComputeFunction for StreamingChunkHash {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingChunkHash");
        spawn_map(rx, 2, |chunk| {
            let hash = Sha256::digest(chunk);
            let mut out = BytesMut::with_capacity(chunk.len() + 32);
            out.extend_from_slice(chunk);
            out.extend_from_slice(&hash);
            Ok(out.freeze())
        })
    }
}
