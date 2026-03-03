use std::collections::HashMap;
use std::io::{Read, Write};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::StreamingComputeFunction;
use super::core::*;

const CAP: usize = 8;

// #51 StreamingZstdCompress
pub struct StreamingZstdCompress;
#[async_trait]
impl StreamingComputeFunction for StreamingZstdCompress {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "ZstdCompress");
        let level = params.get("level").and_then(|v| v.parse::<i32>().ok()).unwrap_or(3);
        spawn_map(rx, CAP, move |chunk| {
            zstd::encode_all(chunk, level).map(Bytes::from).map_err(|e| e.to_string())
        })
    }
}

// #52 StreamingZstdDecompress
pub struct StreamingZstdDecompress;
#[async_trait]
impl StreamingComputeFunction for StreamingZstdDecompress {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "ZstdDecompress");
        spawn_map(rx, CAP, |chunk| {
            zstd::decode_all(chunk).map(Bytes::from).map_err(|e| e.to_string())
        })
    }
}

// #53 StreamingLz4Compress
pub struct StreamingLz4Compress;
#[async_trait]
impl StreamingComputeFunction for StreamingLz4Compress {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "Lz4Compress");
        spawn_map(rx, CAP, |chunk| {
            Ok(Bytes::from(lz4_flex::compress_prepend_size(chunk)))
        })
    }
}

// #54 StreamingLz4Decompress
pub struct StreamingLz4Decompress;
#[async_trait]
impl StreamingComputeFunction for StreamingLz4Decompress {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "Lz4Decompress");
        spawn_map(rx, CAP, |chunk| {
            lz4_flex::decompress_size_prepended(chunk).map(Bytes::from).map_err(|e| e.to_string())
        })
    }
}

// #55 StreamingSnappyCompress
pub struct StreamingSnappyCompress;
#[async_trait]
impl StreamingComputeFunction for StreamingSnappyCompress {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "SnappyCompress");
        spawn_map(rx, CAP, |chunk| {
            let mut enc = snap::raw::Encoder::new();
            enc.compress_vec(chunk).map(Bytes::from).map_err(|e| e.to_string())
        })
    }
}

// #56 StreamingSnappyDecompress
pub struct StreamingSnappyDecompress;
#[async_trait]
impl StreamingComputeFunction for StreamingSnappyDecompress {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "SnappyDecompress");
        spawn_map(rx, CAP, |chunk| {
            let mut dec = snap::raw::Decoder::new();
            dec.decompress_vec(chunk).map(Bytes::from).map_err(|e| e.to_string())
        })
    }
}

// #57 StreamingBrotliCompress
pub struct StreamingBrotliCompress;
#[async_trait]
impl StreamingComputeFunction for StreamingBrotliCompress {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "BrotliCompress");
        let quality = params.get("quality").and_then(|v| v.parse::<u32>().ok()).unwrap_or(4);
        spawn_map(rx, CAP, move |chunk| {
            let mut out = Vec::new();
            {
                let mut w = brotli::CompressorWriter::new(&mut out, 4096, quality, 22);
                w.write_all(chunk).map_err(|e| e.to_string())?;
            }
            Ok(Bytes::from(out))
        })
    }
}

// #58 StreamingBrotliDecompress
pub struct StreamingBrotliDecompress;
#[async_trait]
impl StreamingComputeFunction for StreamingBrotliDecompress {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "BrotliDecompress");
        spawn_map(rx, CAP, |chunk| {
            let mut out = Vec::new();
            brotli::Decompressor::new(chunk, 4096).read_to_end(&mut out).map_err(|e| e.to_string())?;
            Ok(Bytes::from(out))
        })
    }
}

// #59 StreamingPad — pad to block_size multiple
pub struct StreamingPad;
#[async_trait]
impl StreamingComputeFunction for StreamingPad {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "Pad");
        let block_size = params.get("block_size").and_then(|v| v.parse::<usize>().ok()).unwrap_or(16);
        let pad_byte = params.get("padding_byte").and_then(|v| v.parse::<u8>().ok()).unwrap_or(0x00);
        spawn_map(rx, CAP, move |chunk| {
            if chunk.is_empty() || chunk.len() % block_size == 0 {
                return Ok(Bytes::copy_from_slice(chunk));
            }
            let pad_len = block_size - (chunk.len() % block_size);
            let mut out = chunk.to_vec();
            out.extend(std::iter::repeat(pad_byte).take(pad_len));
            Ok(Bytes::from(out))
        })
    }
}

// #60 StreamingTrim — strip leading/trailing ASCII whitespace per chunk
pub struct StreamingTrim;
#[async_trait]
impl StreamingComputeFunction for StreamingTrim {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "Trim");
        spawn_map(rx, CAP, |chunk| {
            let s = chunk.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(chunk.len());
            let e = chunk.iter().rposition(|b| !b.is_ascii_whitespace()).map(|p| p + 1).unwrap_or(s);
            Ok(Bytes::copy_from_slice(&chunk[s..e]))
        })
    }
}
