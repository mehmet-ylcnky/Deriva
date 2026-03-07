//! §3.2 Streaming Encoding & Format Conversion (#30–#39)

use std::collections::HashMap;
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::{StreamingComputeFunction, DEFAULT_CHANNEL_CAPACITY, DEFAULT_CHUNK_SIZE};
use super::core::{spawn_map, spawn_buffered, take_one};

#[allow(dead_code)]
async fn error_stream(msg: String) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(1);
    let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(msg))).await;
    rx
}

// ── #30 StreamingHexEncode ──

pub struct StreamingHexEncode;

#[async_trait]
impl StreamingComputeFunction for StreamingHexEncode {
    fn is_fusible(&self) -> bool { true }
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "hex_encode");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            let hex: String = b.iter().map(|byte| format!("{:02x}", byte)).collect();
            Ok(Bytes::from(hex))
        })
    }
}

// ── #31 StreamingHexDecode ──

pub struct StreamingHexDecode;

#[async_trait]
impl StreamingComputeFunction for StreamingHexDecode {
    fn is_fusible(&self) -> bool { true }
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "hex_decode");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            let s = std::str::from_utf8(b).map_err(|e| format!("hex decode: {}", e))?;
            if s.len() % 2 != 0 { return Err("hex decode: odd-length input".into()); }
            (0..s.len()).step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i+2], 16).map_err(|_| format!("hex decode: invalid char at {}", i)))
                .collect::<Result<Vec<u8>, _>>()
                .map(Bytes::from)
        })
    }
}

// ── #32 StreamingUtf8Validate ──

pub struct StreamingUtf8Validate;

#[async_trait]
impl StreamingComputeFunction for StreamingUtf8Validate {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "utf8_validate");
        let (tx, out) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
        tokio::spawn(async move {
            let mut pending: Vec<u8> = Vec::new();
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        let mut buf = std::mem::take(&mut pending);
                        buf.extend_from_slice(&chunk);
                        let trail = incomplete_utf8_tail(&buf);
                        let valid_end = buf.len() - trail;
                        if let Err(e) = std::str::from_utf8(&buf[..valid_end]) {
                            let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(
                                format!("utf8: invalid byte at position {}", e.valid_up_to())))).await;
                            return;
                        }
                        pending = buf[valid_end..].to_vec();
                        if valid_end > 0
                            && tx.send(StreamChunk::Data(Bytes::copy_from_slice(&buf[..valid_end]))).await.is_err() { return; }
                    }
                    Some(StreamChunk::End) => {
                        if !pending.is_empty() {
                            let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(
                                format!("utf8: incomplete sequence at end of stream ({} trailing bytes)", pending.len())))).await;
                            return;
                        }
                        let _ = tx.send(StreamChunk::End).await;
                        return;
                    }
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                    None => return,
                }
            }
        });
        out
    }
}

/// Count trailing bytes that could be an incomplete UTF-8 sequence (0–3).
fn incomplete_utf8_tail(buf: &[u8]) -> usize {
    if buf.is_empty() { return 0; }
    // Check last 1–3 bytes for a leading byte without enough continuation bytes
    for i in 1..=3.min(buf.len()) {
        let idx = buf.len() - i;
        let b = buf[idx];
        let expected_len = if b & 0x80 == 0 { 1 }
            else if b & 0xE0 == 0xC0 { 2 }
            else if b & 0xF0 == 0xE0 { 3 }
            else if b & 0xF8 == 0xF0 { 4 }
            else { continue }; // continuation byte, keep looking
        if expected_len > i { return i; }
        return 0;
    }
    0
}

// ── #33 StreamingLineEnding ──

pub struct StreamingLineEnding;

#[async_trait]
impl StreamingComputeFunction for StreamingLineEnding {
    fn is_fusible(&self) -> bool { true }
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "line_ending");
        let to_crlf = params.get("target").map(|s| s.as_str()) == Some("crlf");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, move |b| {
            if to_crlf {
                // \n → \r\n (but not already \r\n)
                let mut out = Vec::with_capacity(b.len() * 2);
                for i in 0..b.len() {
                    if b[i] == b'\n' && (i == 0 || b[i - 1] != b'\r') {
                        out.push(b'\r');
                    }
                    out.push(b[i]);
                }
                Ok(Bytes::from(out))
            } else {
                // \r\n → \n
                let mut out = Vec::with_capacity(b.len());
                let mut i = 0;
                while i < b.len() {
                    if i + 1 < b.len() && b[i] == b'\r' && b[i + 1] == b'\n' {
                        out.push(b'\n');
                        i += 2;
                    } else {
                        out.push(b[i]);
                        i += 1;
                    }
                }
                Ok(Bytes::from(out))
            }
        })
    }
}

// ── #34 StreamingJsonPrettyPrint ──

pub struct StreamingJsonPrettyPrint;

#[async_trait]
impl StreamingComputeFunction for StreamingJsonPrettyPrint {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "json_pretty_print");
        spawn_buffered(rx, DEFAULT_CHANNEL_CAPACITY, DEFAULT_CHUNK_SIZE, |b| {
            let val: serde_json::Value = serde_json::from_slice(&b)
                .map_err(|e| format!("json: {}", e))?;
            serde_json::to_vec_pretty(&val)
                .map(Bytes::from)
                .map_err(|e| format!("json: {}", e))
        })
    }
}

// ── #35 StreamingJsonMinify ──

pub struct StreamingJsonMinify;

#[async_trait]
impl StreamingComputeFunction for StreamingJsonMinify {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "json_minify");
        spawn_buffered(rx, DEFAULT_CHANNEL_CAPACITY, DEFAULT_CHUNK_SIZE, |b| {
            let val: serde_json::Value = serde_json::from_slice(&b)
                .map_err(|e| format!("json: {}", e))?;
            serde_json::to_vec(&val)
                .map(Bytes::from)
                .map_err(|e| format!("json: {}", e))
        })
    }
}

// ── #36 StreamingJsonLines ──

pub struct StreamingJsonLines;

#[async_trait]
impl StreamingComputeFunction for StreamingJsonLines {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "json_lines");
        spawn_buffered(rx, DEFAULT_CHANNEL_CAPACITY, DEFAULT_CHUNK_SIZE, |b| {
            let arr: Vec<serde_json::Value> = serde_json::from_slice(&b)
                .map_err(|e| format!("json_lines: expected array: {}", e))?;
            let mut out = Vec::new();
            for item in &arr {
                serde_json::to_writer(&mut out, item)
                    .map_err(|e| format!("json_lines: {}", e))?;
                out.push(b'\n');
            }
            Ok(Bytes::from(out))
        })
    }
}

// ── #37 StreamingCsvToJson ──

pub struct StreamingCsvToJson;

#[async_trait]
impl StreamingComputeFunction for StreamingCsvToJson {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "csv_to_json");
        // Use buffered since we need the header from the first line
        spawn_buffered(rx, DEFAULT_CHANNEL_CAPACITY, DEFAULT_CHUNK_SIZE, |b| {
            let mut rdr = csv::Reader::from_reader(b.as_ref());
            let headers: Vec<String> = rdr.headers()
                .map_err(|e| format!("csv: {}", e))?
                .iter().map(|s| s.to_string()).collect();
            let mut out = Vec::new();
            for result in rdr.records() {
                let record = result.map_err(|e| format!("csv: {}", e))?;
                let obj: serde_json::Map<String, serde_json::Value> = headers.iter()
                    .zip(record.iter())
                    .map(|(h, v)| (h.clone(), serde_json::Value::String(v.to_string())))
                    .collect();
                serde_json::to_writer(&mut out, &obj)
                    .map_err(|e| format!("csv: {}", e))?;
                out.push(b'\n');
            }
            Ok(Bytes::from(out))
        })
    }
}

// ── #38 StreamingBase32Encode ──

pub struct StreamingBase32Encode;

#[async_trait]
impl StreamingComputeFunction for StreamingBase32Encode {
    fn is_fusible(&self) -> bool { true }
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "base32_encode");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            Ok(Bytes::from(data_encoding::BASE32.encode(b)))
        })
    }
}

// ── #39 StreamingBase32Decode ──

pub struct StreamingBase32Decode;

#[async_trait]
impl StreamingComputeFunction for StreamingBase32Decode {
    fn is_fusible(&self) -> bool { true }
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "base32_decode");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            let s = std::str::from_utf8(b).map_err(|e| format!("base32: {}", e))?;
            data_encoding::BASE32.decode(s.as_bytes())
                .map(Bytes::from)
                .map_err(|e| format!("base32 decode: {}", e))
        })
    }
}
