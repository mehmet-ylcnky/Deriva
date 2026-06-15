use std::collections::HashMap;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::StreamingComputeFunction;
use super::core::{take_one, spawn_boundary_map, spawn_buffered};
use encoding_rs;

fn error_stream(msg: String) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(2);
    let _ = tx.try_send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(msg)));
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
            Some(_) => return error_stream("StreamingReplace: find pattern must not be empty".into()),
            None => return error_stream("StreamingReplace: missing required param 'find'".into()),
        };
        let replace = params.get("replace").cloned().unwrap_or_default();
        // Check if find contains regex metacharacters
        let is_regex = find.contains(|c: char| ".*+?^${}()|[]\\".contains(c));
        if is_regex {
            let re = match regex::Regex::new(&find) {
                Ok(r) => r,
                Err(e) => return error_stream(format!("StreamingReplace: invalid regex: {e}")),
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
                        } else if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
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
            None => return error_stream("StreamingGrep: missing required param 'pattern'".into()),
        };
        let invert = params.get("invert").is_some_and(|v| v == "true");
        let re = match regex::Regex::new(&pattern) {
            Ok(r) => r,
            Err(e) => return error_stream(format!("StreamingGrep: invalid regex: {e}")),
        };
        spawn_boundary_map(rx, 2, move |chunk| {
            let text = String::from_utf8_lossy(chunk);
            let mut out = String::new();
            // Process each line preserving its terminator
            let mut start = 0;
            for (i, c) in text.char_indices() {
                if c == '\n' {
                    let line = &text[start..i]; // line content without \n
                    let matches = re.is_match(line);
                    if matches != invert {
                        out.push_str(&text[start..=i]); // include the \n
                    }
                    start = i + 1;
                }
            }
            // Handle trailing content (partial line without terminating \n)
            if start < text.len() {
                let line = &text[start..];
                let matches = re.is_match(line);
                if matches != invert {
                    out.push_str(line);
                }
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
            None => return error_stream("StreamingSed: missing required param 'pattern'".into()),
        };
        let replacement = params.get("replacement").cloned().unwrap_or_default();
        let re = match regex::Regex::new(&pattern) {
            Ok(r) => r,
            Err(e) => return error_stream(format!("StreamingSed: invalid regex: {e}")),
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

/// Normalize encoding labels to WHATWG-compatible labels for encoding_rs
fn normalize_encoding_label(label: &str) -> &str {
    match label {
        "utf8" => "utf-8",
        "utf16le" => "utf-16le",
        "utf16be" => "utf-16be",
        "latin1" => "iso-8859-1",
        other => other,
    }
}

#[async_trait]
impl StreamingComputeFunction for StreamingCharsetConvert {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingCharsetConvert");
        let from = params.get("from").cloned().unwrap_or_else(|| "utf-8".into());
        let to = params.get("to").cloned().unwrap_or_else(|| "utf-8".into());

        // Special-case ASCII: encoding_rs maps it to windows-1252,
        // but we need strict ASCII validation.
        let from_is_ascii = matches!(from.as_str(), "ascii" | "us-ascii");
        let to_is_ascii = matches!(to.as_str(), "ascii" | "us-ascii");

        let from_label = normalize_encoding_label(&from);
        let to_label = normalize_encoding_label(&to);

        let from_enc = if from_is_ascii {
            encoding_rs::UTF_8 // We'll validate ASCII separately
        } else {
            match encoding_rs::Encoding::for_label(from_label.as_bytes()) {
                Some(enc) => enc,
                None => return error_stream(format!("StreamingCharsetConvert: unsupported encoding '{from}'")),
            }
        };
        let to_enc = if to_is_ascii {
            encoding_rs::UTF_8 // We'll validate ASCII separately
        } else {
            match encoding_rs::Encoding::for_label(to_label.as_bytes()) {
                Some(enc) => enc,
                None => return error_stream(format!("StreamingCharsetConvert: unsupported encoding '{to}'")),
            }
        };

        spawn_buffered(rx, 2, 64 * 1024, move |buf| {
            // Decode from source encoding to internal string
            let decoded: String = if from_is_ascii {
                // Strict ASCII validation
                if buf.iter().any(|&b| b > 127) {
                    return Err("charset: non-ascii byte in input".into());
                }
                String::from_utf8(buf.to_vec()).map_err(|e| format!("charset: {e}"))?
            } else {
                let (cow, _, had_errors) = from_enc.decode(&buf);
                if had_errors {
                    return Err(format!("charset: invalid {} input", from_enc.name()));
                }
                cow.into_owned()
            };

            // Encode from internal string to target encoding
            if to_is_ascii {
                if decoded.bytes().any(|b| b > 127) {
                    return Err("charset: non-ascii characters in output".into());
                }
                Ok(Bytes::from(decoded.into_bytes()))
            } else {
                let (encoded, _, had_errors) = to_enc.encode(&decoded);
                if had_errors {
                    return Err(format!("charset: unmappable characters for {}", to_enc.name()));
                }
                Ok(Bytes::from(encoded.into_owned()))
            }
        })
    }
}
