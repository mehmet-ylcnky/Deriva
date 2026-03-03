use std::collections::HashMap;
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::StreamingComputeFunction;
use super::core::*;

const DEFAULT_CAP: usize = 8;

// #93 StreamingSum — newline-delimited decimal sum
pub struct StreamingSum;
#[async_trait]
impl StreamingComputeFunction for StreamingSum {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "Sum");
        spawn_accumulate(rx, (0.0f64, Vec::<u8>::new()), |state, chunk| {
            state.1.extend_from_slice(chunk);
        }, |state| {
            let text = String::from_utf8_lossy(&state.1);
            let sum: f64 = text.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| l.trim().parse::<f64>().ok()).sum();
            let s = if sum == 0.0 { "0".to_string() } else { sum.to_string() };
            Bytes::from(s)
        })
    }
}

// #94 StreamingAverage — newline-delimited decimal average
pub struct StreamingAverage;
#[async_trait]
impl StreamingComputeFunction for StreamingAverage {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "Average");
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let mut rx = rx;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(c)) => buf.extend_from_slice(&c),
                    Some(StreamChunk::End) => break,
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                    None => break,
                }
            }
            let text = String::from_utf8_lossy(&buf);
            let vals: Vec<f64> = text.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| l.trim().parse().ok()).collect();
            if vals.is_empty() {
                let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed("no valid numbers".into()))).await;
                return;
            }
            let avg = vals.iter().sum::<f64>() / vals.len() as f64;
            let _ = tx.send(StreamChunk::Data(Bytes::from(avg.to_string()))).await;
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}

// #95 StreamingBitwiseAnd — AND each byte with mask
pub struct StreamingBitwiseAnd;
#[async_trait]
impl StreamingComputeFunction for StreamingBitwiseAnd {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "BitwiseAnd");
        let mask = params.get("mask").and_then(|v| v.parse::<u8>().ok()).unwrap_or(0xFF);
        spawn_map(rx, DEFAULT_CAP, move |b| {
            Ok(Bytes::from(b.iter().map(|byte| byte & mask).collect::<Vec<_>>()))
        })
    }
}

// #96 StreamingBitwiseOr — OR each byte with mask
pub struct StreamingBitwiseOr;
#[async_trait]
impl StreamingComputeFunction for StreamingBitwiseOr {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "BitwiseOr");
        let mask = params.get("mask").and_then(|v| v.parse::<u8>().ok()).unwrap_or(0x00);
        spawn_map(rx, DEFAULT_CAP, move |b| {
            Ok(Bytes::from(b.iter().map(|byte| byte | mask).collect::<Vec<_>>()))
        })
    }
}

// #97 StreamingBitwiseNot — complement each byte
pub struct StreamingBitwiseNot;
#[async_trait]
impl StreamingComputeFunction for StreamingBitwiseNot {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "BitwiseNot");
        spawn_map(rx, DEFAULT_CAP, |b| {
            Ok(Bytes::from(b.iter().map(|byte| !byte).collect::<Vec<_>>()))
        })
    }
}

// #98 StreamingByteSwap — swap byte order within words
pub struct StreamingByteSwap;
#[async_trait]
impl StreamingComputeFunction for StreamingByteSwap {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "ByteSwap");
        let ws: usize = params.get("word_size").and_then(|v| v.parse().ok()).unwrap_or(2);
        spawn_map(rx, DEFAULT_CAP, move |b| {
            if b.len() % ws != 0 {
                return Err(format!("chunk length {} not divisible by word_size {}", b.len(), ws));
            }
            let mut out = b.to_vec();
            for word in out.chunks_mut(ws) {
                word.reverse();
            }
            Ok(Bytes::from(out))
        })
    }
}

// #99 StreamingEntropy — Shannon entropy in bits
pub struct StreamingEntropy;
#[async_trait]
impl StreamingComputeFunction for StreamingEntropy {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "Entropy");
        spawn_accumulate(rx, ([0u64; 256], 0u64), |state, chunk| {
            for &b in chunk { state.0[b as usize] += 1; state.1 += 1; }
        }, |state| {
            let (freq, total) = state;
            if total == 0 { return Bytes::from(0.0f64.to_be_bytes().to_vec()); }
            let t = total as f64;
            let h: f64 = freq.iter().filter(|&&c| c > 0).map(|&c| {
                let p = c as f64 / t;
                -p * p.log2()
            }).sum();
            Bytes::from(h.to_be_bytes().to_vec())
        })
    }
}

// #100 StreamingRollingHash — Rabin fingerprint, append 8-byte hash per chunk
pub struct StreamingRollingHash;
#[async_trait]
impl StreamingComputeFunction for StreamingRollingHash {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "RollingHash");
        let window_size: usize = params.get("window_size").and_then(|v| v.parse().ok()).unwrap_or(48);
        let base: u64 = 257;
        let modulus: u64 = (1 << 61) - 1; // Mersenne prime
        // Precompute base^window_size mod modulus
        let mut base_pow = 1u64;
        for _ in 0..window_size { base_pow = (base_pow as u128 * base as u128 % modulus as u128) as u64; }

        spawn_map_stateful(rx, DEFAULT_CAP, move |chunk| {
            // We keep a simple rolling hash state in captured vars via interior pattern
            // For simplicity: compute Rabin hash over the last `window_size` bytes of the chunk
            let mut hash: u64 = 0;
            let start = if chunk.len() > window_size { chunk.len() - window_size } else { 0 };
            for &b in &chunk[start..] {
                hash = ((hash as u128 * base as u128 + b as u128) % modulus as u128) as u64;
            }
            let mut out = chunk.to_vec();
            out.extend_from_slice(&hash.to_be_bytes());
            Ok(Bytes::from(out))
        })
    }
}
