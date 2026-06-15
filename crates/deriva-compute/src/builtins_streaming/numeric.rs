use std::collections::{HashMap, VecDeque};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::StreamingComputeFunction;
use super::core::*;

const DEFAULT_CAP: usize = 8;

fn error_stream(msg: String) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(2);
    let _ = tx.try_send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(msg)));
    rx
}

// #93 StreamingSum — newline-delimited decimal sum
pub struct StreamingSum;
#[async_trait]
impl StreamingComputeFunction for StreamingSum {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "Sum");
        spawn_accumulate(rx, Vec::<u8>::new(), |state, chunk| {
            state.extend_from_slice(chunk);
        }, |state| {
            let text = String::from_utf8_lossy(&state);
            let sum: f64 = text.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| l.trim().parse::<f64>().ok()).sum();
            // Normalize -0.0 to 0.0 for clean output
            let sum = sum + 0.0;
            Bytes::from(sum.to_string())
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
                let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed("StreamingAverage: no valid numbers found".into()))).await;
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
    fn is_fusible(&self) -> bool { true }
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
    fn is_fusible(&self) -> bool { true }
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
    fn is_fusible(&self) -> bool { true }
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
    fn is_fusible(&self) -> bool { true }
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "ByteSwap");
        let ws: usize = params.get("word_size").and_then(|v| v.parse().ok()).unwrap_or(2);
        spawn_map(rx, DEFAULT_CAP, move |b| {
            if b.len() % ws != 0 {
                return Err(format!("StreamingByteSwap: chunk length {} not divisible by word_size {}", b.len(), ws));
            }
            let mut out = b.to_vec();
            for word in out.chunks_mut(ws) {
                word.reverse();
            }
            Ok(Bytes::from(out))
        })
    }
}

// #99 StreamingEntropy — Shannon entropy in bits/byte (ASCII decimal string output)
pub struct StreamingEntropy;
#[async_trait]
impl StreamingComputeFunction for StreamingEntropy {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "Entropy");
        spawn_accumulate(rx, ([0u64; 256], 0u64), |state, chunk| {
            for &b in chunk { state.0[b as usize] += 1; state.1 += 1; }
        }, |state| {
            let (freq, total) = state;
            if total == 0 { return Bytes::from("0.0"); }
            let t = total as f64;
            let h: f64 = freq.iter().filter(|&&c| c > 0).map(|&c| {
                let p = c as f64 / t;
                -p * p.log2()
            }).sum();
            // Clamp to [0.0, 8.0] range (Shannon entropy max for bytes)
            let h = h.clamp(0.0, 8.0);
            Bytes::from(h.to_string())
        })
    }
}

// #100 StreamingRollingHash — Rabin fingerprint over sliding window, append 8-byte hash per chunk
pub struct StreamingRollingHash;
#[async_trait]
impl StreamingComputeFunction for StreamingRollingHash {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "RollingHash");
        let window_size: usize = params.get("window_size").and_then(|v| v.parse().ok()).unwrap_or(48);
        if window_size == 0 {
            return error_stream("StreamingRollingHash: window_size must be > 0".into());
        }
        let base: u64 = 257;
        let modulus: u64 = (1 << 61) - 1; // Mersenne prime
        // Precompute base^window_size mod modulus
        let mut base_pow = 1u64;
        for _ in 0..window_size { base_pow = (base_pow as u128 * base as u128 % modulus as u128) as u64; }

        // Maintain sliding window state across chunks
        let mut window: VecDeque<u8> = VecDeque::with_capacity(window_size);
        let mut hash: u64 = 0;

        spawn_map_stateful(rx, DEFAULT_CAP, move |chunk| {
            // Process each byte through the rolling hash, maintaining window state
            for &b in chunk {
                if window.len() >= window_size {
                    // Remove oldest byte from hash
                    let old = window.pop_front().unwrap();
                    hash = ((hash as u128 + modulus as u128 - (old as u128 * base_pow as u128 % modulus as u128)) % modulus as u128) as u64;
                }
                // Add new byte to hash
                hash = ((hash as u128 * base as u128 + b as u128) % modulus as u128) as u64;
                window.push_back(b);
            }
            let mut out = chunk.to_vec();
            out.extend_from_slice(&hash.to_be_bytes());
            Ok(Bytes::from(out))
        })
    }
}
