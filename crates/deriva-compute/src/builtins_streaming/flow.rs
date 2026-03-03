use std::collections::HashMap;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use deriva_core::streaming::StreamChunk;
use crate::streaming::StreamingComputeFunction;
use super::core::take_one;

// ── #61 StreamingRateLimit ───────────────────────────────────────────

pub struct StreamingRateLimit;

#[async_trait]
impl StreamingComputeFunction for StreamingRateLimit {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingRateLimit");
        let bps: f64 = params.get("bytes_per_sec").and_then(|v| v.parse().ok()).unwrap_or(1_048_576.0);
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        let delay_secs = chunk.len() as f64 / bps;
                        if delay_secs > 0.0001 {
                            sleep(Duration::from_secs_f64(delay_secs)).await;
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

// ── #62 StreamingDelay ───────────────────────────────────────────────

pub struct StreamingDelay;

#[async_trait]
impl StreamingComputeFunction for StreamingDelay {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingDelay");
        let ms: u64 = params.get("delay_ms").and_then(|v| v.parse().ok()).unwrap_or(100);
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        sleep(Duration::from_millis(ms)).await;
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

// ── #63 StreamingTimeout ─────────────────────────────────────────────

pub struct StreamingTimeout;

#[async_trait]
impl StreamingComputeFunction for StreamingTimeout {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingTimeout");
        let ms: u64 = params.get("timeout_ms").and_then(|v| v.parse().ok()).unwrap_or(5000);
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            loop {
                match tokio::time::timeout(Duration::from_millis(ms), rx.recv()).await {
                    Ok(Some(StreamChunk::Data(chunk))) => {
                        if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
                    }
                    Ok(Some(StreamChunk::End)) | Ok(None) => { let _ = tx.send(StreamChunk::End).await; return; }
                    Ok(Some(StreamChunk::Error(e))) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                    Err(_) => {
                        let _ = tx.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed("timeout".into()))).await;
                        return;
                    }
                }
            }
        });
        out
    }
}

// ── #64 StreamingRetry ───────────────────────────────────────────────

pub struct StreamingRetry;

#[async_trait]
impl StreamingComputeFunction for StreamingRetry {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingRetry");
        let max: usize = params.get("max_retries").and_then(|v| v.parse().ok()).unwrap_or(3);
        // Pipeline-level: forward data, count errors, re-emit error only after max retries exhausted
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut errors = 0usize;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
                    }
                    Some(StreamChunk::End) | None => { let _ = tx.send(StreamChunk::End).await; return; }
                    Some(StreamChunk::Error(e)) => {
                        errors += 1;
                        if errors > max {
                            let _ = tx.send(StreamChunk::Error(e)).await;
                            return;
                        }
                        // Swallow error, continue reading (retry semantics at pipeline level)
                    }
                }
            }
        });
        out
    }
}

// ── #65 StreamingTee ─────────────────────────────────────────────────

pub struct StreamingTee;

#[async_trait]
impl StreamingComputeFunction for StreamingTee {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingTee");
        let n: usize = params.get("outputs").and_then(|v| v.parse().ok()).unwrap_or(2).max(1);
        let mut senders = Vec::with_capacity(n);
        let mut receivers = Vec::with_capacity(n);
        for _ in 0..n {
            let (tx, rx) = mpsc::channel(2);
            senders.push(tx);
            receivers.push(rx);
        }
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        for s in &senders {
                            if s.send(StreamChunk::Data(chunk.clone())).await.is_err() { return; }
                        }
                    }
                    Some(StreamChunk::End) | None => {
                        for s in &senders { let _ = s.send(StreamChunk::End).await; }
                        return;
                    }
                    Some(StreamChunk::Error(e)) => {
                        for s in &senders { let _ = s.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(e.to_string()))).await; }
                        return;
                    }
                }
            }
        });
        let primary = receivers.remove(0);
        // Drain extra receivers so producer doesn't block
        for mut r in receivers {
            tokio::spawn(async move { while r.recv().await.is_some() {} });
        }
        primary
    }
}

// ── #66 StreamingMerge ───────────────────────────────────────────────

pub struct StreamingMerge;

#[async_trait]
impl StreamingComputeFunction for StreamingMerge {
    async fn stream_execute(&self, inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let (tx, out) = mpsc::channel(2);
        // Spawn one task per input, all send to same output
        let tx2 = tx.clone();
        let n = inputs.len();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for mut rx in inputs {
            let tx = tx.clone();
            let done = done.clone();
            let total = n;
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Some(StreamChunk::Data(chunk)) => {
                            if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
                        }
                        Some(StreamChunk::End) | None => {
                            let prev = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            if prev + 1 == total {
                                let _ = tx.send(StreamChunk::End).await;
                            }
                            return;
                        }
                        Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                    }
                }
            });
        }
        drop(tx2);
        out
    }
}

// ── #67 StreamingBroadcast ───────────────────────────────────────────

pub struct StreamingBroadcast;

#[async_trait]
impl StreamingComputeFunction for StreamingBroadcast {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        // Like Tee but uses bounded channels — slowest consumer gates producer
        // Within single-output trait, behaves identically to Tee
        let mut rx = take_one(&mut inputs, "StreamingBroadcast");
        let n: usize = params.get("outputs").and_then(|v| v.parse().ok()).unwrap_or(2).max(1);
        let mut senders = Vec::with_capacity(n);
        let mut receivers = Vec::with_capacity(n);
        for _ in 0..n {
            let (tx, rx) = mpsc::channel(1); // capacity 1 = backpressure
            senders.push(tx);
            receivers.push(rx);
        }
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        for s in &senders {
                            if s.send(StreamChunk::Data(chunk.clone())).await.is_err() { return; }
                        }
                    }
                    Some(StreamChunk::End) | None => {
                        for s in &senders { let _ = s.send(StreamChunk::End).await; }
                        return;
                    }
                    Some(StreamChunk::Error(e)) => {
                        for s in &senders { let _ = s.send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(e.to_string()))).await; }
                        return;
                    }
                }
            }
        });
        let primary = receivers.remove(0);
        for mut r in receivers {
            tokio::spawn(async move { while r.recv().await.is_some() {} });
        }
        primary
    }
}

// ── #68 StreamingPartition ───────────────────────────────────────────

pub struct StreamingPartition;

#[async_trait]
impl StreamingComputeFunction for StreamingPartition {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingPartition");
        let pred = params.get("predicate").cloned().unwrap_or_else(|| "non_empty".into());
        // Two outputs: matching (returned) and non-matching (dropped within trait constraint)
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        let matches = if pred == "non_empty" {
                            !chunk.is_empty()
                        } else if let Some(pat) = pred.strip_prefix("contains:") {
                            chunk.as_ref().windows(pat.len()).any(|w| w == pat.as_bytes())
                        } else if let Some(n) = pred.strip_prefix("min_size:") {
                            n.parse::<usize>().map_or(false, |n| chunk.len() >= n)
                        } else {
                            true
                        };
                        if matches {
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

// ── #69 StreamingBatch ───────────────────────────────────────────────

pub struct StreamingBatch;

#[async_trait]
impl StreamingComputeFunction for StreamingBatch {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingBatch");
        let batch_size: usize = params.get("batch_size").and_then(|v| v.parse().ok()).unwrap_or(4).max(1);
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut buf = BytesMut::new();
            let mut count = 0usize;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        buf.extend_from_slice(&chunk);
                        count += 1;
                        if count >= batch_size {
                            if tx.send(StreamChunk::Data(buf.freeze())).await.is_err() { return; }
                            buf = BytesMut::new();
                            count = 0;
                        }
                    }
                    Some(StreamChunk::End) | None => {
                        if !buf.is_empty() {
                            let _ = tx.send(StreamChunk::Data(buf.freeze())).await;
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

// ── #70 StreamingDebounce ────────────────────────────────────────────

pub struct StreamingDebounce;

#[async_trait]
impl StreamingComputeFunction for StreamingDebounce {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingDebounce");
        let window_ms: u64 = params.get("window_ms").and_then(|v| v.parse().ok()).unwrap_or(100);
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut latest: Option<Bytes> = None;
            loop {
                match tokio::time::timeout(Duration::from_millis(window_ms), rx.recv()).await {
                    Ok(Some(StreamChunk::Data(chunk))) => {
                        latest = Some(chunk);
                    }
                    Ok(Some(StreamChunk::End)) | Ok(None) => {
                        if let Some(b) = latest.take() {
                            let _ = tx.send(StreamChunk::Data(b)).await;
                        }
                        let _ = tx.send(StreamChunk::End).await;
                        return;
                    }
                    Ok(Some(StreamChunk::Error(e))) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                    Err(_) => {
                        // Window expired — emit latest and wait for more
                        if let Some(b) = latest.take() {
                            if tx.send(StreamChunk::Data(b)).await.is_err() { return; }
                        }
                    }
                }
            }
        });
        out
    }
}
