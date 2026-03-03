use std::collections::HashMap;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::StreamingComputeFunction;
use super::core::{take_one, spawn_boundary_map, spawn_buffered};

fn error_stream(msg: String) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(async move {
        let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(msg))).await;
    });
    rx
}

// ── #78 StreamingReplace ─────────────────────────────────────────────

pub struct StreamingReplace;

#[async_trait]
impl StreamingComputeFunction for StreamingReplace {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingReplace");
        let find = match params.get("find") {
            Some(s) if !s.is_empty() => s.clone(),
            Some(_) => return error_stream("replace: find pattern must not be empty".into()),
            None => return error_stream("missing param: find".into()),
        };
        let replace = params.get("replace").cloned().unwrap_or_default();
        // Check if find contains regex metacharacters
        let is_regex = find.contains(|c: char| ".*+?^${}()|[]\\".contains(c));
        if is_regex {
            let re = match regex::Regex::new(&find) {
                Ok(r) => r,
                Err(e) => return error_stream(format!("invalid regex: {e}")),
            };
            spawn_boundary_map(rx, 2, move |chunk| {
                let s = String::from_utf8_lossy(chunk);
                let result = re.replace_all(&s, replace.as_str());
                Ok(Bytes::from(result.into_owned()))
            })
        } else {
            spawn_boundary_map(rx, 2, move |chunk| {
                let find_bytes = find.as_bytes();
                let replace_bytes = replace.as_bytes();
                if find_bytes.is_empty() { return Ok(Bytes::copy_from_slice(chunk)); }
                let mut out = Vec::new();
                let mut i = 0;
                while i < chunk.len() {
                    if i + find_bytes.len() <= chunk.len() && &chunk[i..i + find_bytes.len()] == find_bytes {
                        out.extend_from_slice(replace_bytes);
                        i += find_bytes.len();
                    } else {
                        out.push(chunk[i]);
                        i += 1;
                    }
                }
                Ok(Bytes::from(out))
            })
        }
    }
}

// ── #79 StreamingPrefix ──────────────────────────────────────────────

pub struct StreamingPrefix;

#[async_trait]
impl StreamingComputeFunction for StreamingPrefix {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingPrefix");
        let prefix = Bytes::from(params.get("prefix").cloned().unwrap_or_default());
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut emitted = false;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        if !emitted {
                            emitted = true;
                            let mut buf = BytesMut::with_capacity(prefix.len() + chunk.len());
                            buf.extend_from_slice(&prefix);
                            buf.extend_from_slice(&chunk);
                            if tx.send(StreamChunk::Data(buf.freeze())).await.is_err() { return; }
                        } else {
                            if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
                        }
                    }
                    Some(StreamChunk::End) | None => { let _ = tx.send(StreamChunk::End).await; return; }
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                }
            }
        });
        out
    }
}

// ── #80 StreamingSuffix ──────────────────────────────────────────────

pub struct StreamingSuffix;

#[async_trait]
impl StreamingComputeFunction for StreamingSuffix {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingSuffix");
        let suffix = Bytes::from(params.get("suffix").cloned().unwrap_or_default());
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
                    }
                    Some(StreamChunk::End) | None => {
                        if !suffix.is_empty() {
                            let _ = tx.send(StreamChunk::Data(suffix)).await;
                        }
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

// ── #81 StreamingLinePrefix ──────────────────────────────────────────

pub struct StreamingLinePrefix;

#[async_trait]
impl StreamingComputeFunction for StreamingLinePrefix {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingLinePrefix");
        let prefix = params.get("prefix").cloned().unwrap_or_default();
        let at_line_start = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        spawn_boundary_map(rx, 2, move |chunk| {
            let pfx = prefix.as_bytes();
            let mut out = Vec::new();
            for &b in chunk {
                if at_line_start.load(std::sync::atomic::Ordering::SeqCst) {
                    out.extend_from_slice(pfx);
                    at_line_start.store(false, std::sync::atomic::Ordering::SeqCst);
                }
                out.push(b);
                if b == b'\n' {
                    at_line_start.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
            Ok(Bytes::from(out))
        })
    }
}

// ── #82 StreamingGrep ────────────────────────────────────────────────

pub struct StreamingGrep;

#[async_trait]
impl StreamingComputeFunction for StreamingGrep {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingGrep");
        let pattern = match params.get("pattern") {
            Some(s) => s.clone(),
            None => return error_stream("missing param: pattern".into()),
        };
        let invert = params.get("invert").map_or(false, |v| v == "true");
        let re = match regex::Regex::new(&pattern) {
            Ok(r) => r,
            Err(e) => return error_stream(format!("invalid regex: {e}")),
        };
        spawn_boundary_map(rx, 2, move |chunk| {
            let text = String::from_utf8_lossy(chunk);
            let mut out = String::new();
            for line in text.split('\n') {
                let matches = re.is_match(line);
                if matches != invert {
                    if !out.is_empty() { out.push('\n'); }
                    out.push_str(line);
                }
            }
            if !out.is_empty() && text.ends_with('\n') {
                out.push('\n');
            }
            Ok(Bytes::from(out))
        })
    }
}

// ── #83 StreamingSed ─────────────────────────────────────────────────

pub struct StreamingSed;

#[async_trait]
impl StreamingComputeFunction for StreamingSed {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingSed");
        let pattern = match params.get("pattern") {
            Some(s) => s.clone(),
            None => return error_stream("missing param: pattern".into()),
        };
        let replacement = params.get("replacement").cloned().unwrap_or_default();
        let re = match regex::Regex::new(&pattern) {
            Ok(r) => r,
            Err(e) => return error_stream(format!("invalid regex: {e}")),
        };
        spawn_boundary_map(rx, 2, move |chunk| {
            let text = String::from_utf8_lossy(chunk);
            let result = re.replace_all(&text, replacement.as_str());
            Ok(Bytes::from(result.into_owned()))
        })
    }
}

// ── #84 StreamingTruncateLines ───────────────────────────────────────

pub struct StreamingTruncateLines;

#[async_trait]
impl StreamingComputeFunction for StreamingTruncateLines {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingTruncateLines");
        let max: usize = params.get("max_line_bytes").and_then(|v| v.parse().ok()).unwrap_or(1024);
        spawn_boundary_map(rx, 2, move |chunk| {
            let mut out = Vec::new();
            for (i, line) in chunk.split(|&b| b == b'\n').enumerate() {
                if i > 0 { out.push(b'\n'); }
                let end = line.len().min(max);
                out.extend_from_slice(&line[..end]);
            }
            Ok(Bytes::from(out))
        })
    }
}

// ── #85 StreamingCharsetConvert ──────────────────────────────────────

pub struct StreamingCharsetConvert;

#[async_trait]
impl StreamingComputeFunction for StreamingCharsetConvert {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingCharsetConvert");
        let from = params.get("from").cloned().unwrap_or_else(|| "utf8".into());
        let to = params.get("to").cloned().unwrap_or_else(|| "utf8".into());
        spawn_buffered(rx, 2, 64 * 1024, move |buf| {
            let text = match from.as_str() {
                "latin1" | "iso-8859-1" => buf.iter().map(|&b| b as char).collect::<String>(),
                "utf8" | "utf-8" => std::str::from_utf8(&buf).map_err(|e| format!("utf8 decode: {e}"))?.to_string(),
                "ascii" => {
                    if buf.iter().any(|&b| b > 127) { return Err("non-ascii byte in input".into()); }
                    std::str::from_utf8(&buf).map_err(|e| format!("ascii: {e}"))?.to_string()
                }
                "utf16le" => {
                    if buf.len() % 2 != 0 { return Err("utf16le: odd byte count".into()); }
                    let u16s: Vec<u16> = buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                    String::from_utf16(&u16s).map_err(|e| format!("utf16le: {e}"))?
                }
                "utf16be" => {
                    if buf.len() % 2 != 0 { return Err("utf16be: odd byte count".into()); }
                    let u16s: Vec<u16> = buf.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                    String::from_utf16(&u16s).map_err(|e| format!("utf16be: {e}"))?
                }
                _ => return Err(format!("unsupported source encoding: {from}")),
            };
            match to.as_str() {
                "utf8" | "utf-8" => Ok(Bytes::from(text)),
                "latin1" | "iso-8859-1" => {
                    let mut out = Vec::with_capacity(text.len());
                    for c in text.chars() {
                        if c as u32 > 255 { return Err(format!("unmappable char for latin1: {c}")); }
                        out.push(c as u8);
                    }
                    Ok(Bytes::from(out))
                }
                "ascii" => {
                    let mut out = Vec::with_capacity(text.len());
                    for c in text.chars() {
                        if !c.is_ascii() { return Err(format!("unmappable char for ascii: {c}")); }
                        out.push(c as u8);
                    }
                    Ok(Bytes::from(out))
                }
                "utf16le" => {
                    let mut out = Vec::new();
                    for u in text.encode_utf16() { out.extend_from_slice(&u.to_le_bytes()); }
                    Ok(Bytes::from(out))
                }
                "utf16be" => {
                    let mut out = Vec::new();
                    for u in text.encode_utf16() { out.extend_from_slice(&u.to_be_bytes()); }
                    Ok(Bytes::from(out))
                }
                _ => Err(format!("unsupported target encoding: {to}")),
            }
        })
    }
}
