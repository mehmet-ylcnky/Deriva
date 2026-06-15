use std::collections::{HashMap, HashSet, VecDeque};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use crate::streaming::StreamingComputeFunction;
use super::core::{take_one, spawn_accumulate};

// ── helpers ──────────────────────────────────────────────────────────

fn error_stream(msg: String) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(2);
    let _ = tx.try_send(StreamChunk::Error(deriva_core::DerivaError::ComputeFailed(msg)));
    rx
}

/// Stateful per-chunk filter: f returns Some(bytes) to forward, None to drop.
fn spawn_filter_map(
    mut rx: mpsc::Receiver<StreamChunk>,
    mut f: impl FnMut(&[u8]) -> Option<Bytes> + Send + 'static,
) -> mpsc::Receiver<StreamChunk> {
    let (tx, out) = mpsc::channel(2);
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Some(StreamChunk::Data(chunk)) => {
                    if let Some(b) = f(&chunk) {
                        if tx.send(StreamChunk::Data(b)).await.is_err() { return; }
                    }
                }
                Some(StreamChunk::End) => { let _ = tx.send(StreamChunk::End).await; return; }
                Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                None => { let _ = tx.send(StreamChunk::End).await; return; }
            }
        }
    });
    out
}

// ── #40 StreamingFilter ──────────────────────────────────────────────

pub struct StreamingFilter;

#[async_trait]
impl StreamingComputeFunction for StreamingFilter {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingFilter");
        let pred = params.get("predicate").cloned().unwrap_or_else(|| "non_empty".into());
        spawn_filter_map(rx, move |chunk| {
            let keep = if pred == "non_empty" {
                !chunk.is_empty()
            } else if let Some(pat) = pred.strip_prefix("contains:") {
                chunk.windows(pat.len()).any(|w| w == pat.as_bytes())
            } else if let Some(n) = pred.strip_prefix("min_size:") {
                n.parse::<usize>().is_ok_and(|n| chunk.len() >= n)
            } else {
                true
            };
            if keep { Some(Bytes::copy_from_slice(chunk)) } else { None }
        })
    }
}

// ── #41 StreamingLineCount ───────────────────────────────────────────

pub struct StreamingLineCount;

#[async_trait]
impl StreamingComputeFunction for StreamingLineCount {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingLineCount");
        spawn_accumulate(rx, 0u64,
            |count, b| *count += b.iter().filter(|&&c| c == b'\n').count() as u64,
            |count| Bytes::copy_from_slice(&count.to_be_bytes()))
    }
}

// ── #42 StreamingWordCount ───────────────────────────────────────────

pub struct StreamingWordCount;

#[async_trait]
impl StreamingComputeFunction for StreamingWordCount {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingWordCount");
        spawn_accumulate(rx, (0u64, false),
            |(count, in_word), b| {
                for &byte in b {
                    let is_ws = byte == b' ' || byte == b'\n' || byte == b'\r' || byte == b'\t';
                    if is_ws {
                        *in_word = false;
                    } else if !*in_word {
                        *count += 1;
                        *in_word = true;
                    }
                }
            },
            |(count, _)| Bytes::copy_from_slice(&count.to_be_bytes()))
    }
}

// ── #43 StreamingMinMax ──────────────────────────────────────────────

pub struct StreamingMinMax;

#[async_trait]
impl StreamingComputeFunction for StreamingMinMax {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingMinMax");
        spawn_accumulate(rx, (255u8, 0u8),
            |(min, max), b| {
                for &byte in b {
                    if byte < *min { *min = byte; }
                    if byte > *max { *max = byte; }
                }
            },
            |(min, max)| Bytes::from(vec![min, max]))
    }
}

// ── #44 StreamingHistogram ───────────────────────────────────────────

pub struct StreamingHistogram;

#[async_trait]
impl StreamingComputeFunction for StreamingHistogram {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingHistogram");
        spawn_accumulate(rx, [0u64; 256],
            |hist, b| { for &byte in b { hist[byte as usize] += 1; } },
            |hist| {
                let mut out = Vec::with_capacity(2048);
                for &c in &hist { out.extend_from_slice(&c.to_be_bytes()); }
                Bytes::from(out)
            })
    }
}

// ── #45 StreamingSample ──────────────────────────────────────────────

pub struct StreamingSample;

#[async_trait]
impl StreamingComputeFunction for StreamingSample {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingSample");
        let rate: usize = params.get("rate").and_then(|v| v.parse().ok()).unwrap_or(10);
        if rate == 0 { return error_stream("StreamingSample: rate must be > 0".into()); }
        let mut counter = 0usize;
        spawn_filter_map(rx, move |chunk| {
            let emit = counter.is_multiple_of(rate);
            counter += 1;
            if emit { Some(Bytes::copy_from_slice(chunk)) } else { None }
        })
    }
}

// ── #46 StreamingHead ────────────────────────────────────────────────

pub struct StreamingHead;

#[async_trait]
impl StreamingComputeFunction for StreamingHead {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingHead");
        let n: usize = params.get("chunks").and_then(|v| v.parse().ok()).unwrap_or(1);
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut emitted = 0usize;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        if emitted < n {
                            emitted += 1;
                            if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
                        }
                        if emitted >= n {
                            let _ = tx.send(StreamChunk::End).await;
                            return;
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

// ── #47 StreamingTail ────────────────────────────────────────────────

pub struct StreamingTail;

#[async_trait]
impl StreamingComputeFunction for StreamingTail {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "StreamingTail");
        let n: usize = params.get("chunks").and_then(|v| v.parse().ok()).unwrap_or(1);
        let (tx, out) = mpsc::channel(2);
        tokio::spawn(async move {
            let mut ring: VecDeque<Bytes> = VecDeque::with_capacity(n);
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        if ring.len() == n { ring.pop_front(); }
                        ring.push_back(chunk);
                    }
                    Some(StreamChunk::End) | None => break,
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                }
            }
            for chunk in ring {
                if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}

// ── #48 StreamingDeduplicate ─────────────────────────────────────────

pub struct StreamingDeduplicate;

fn hash_chunk(data: &[u8]) -> u64 {
    xxhash_rust::xxh64::xxh64(data, 0)
}

#[async_trait]
impl StreamingComputeFunction for StreamingDeduplicate {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingDeduplicate");
        let mut seen = HashSet::new();
        spawn_filter_map(rx, move |chunk| {
            if seen.insert(hash_chunk(chunk)) {
                Some(Bytes::copy_from_slice(chunk))
            } else {
                None
            }
        })
    }
}

// ── #49 StreamingSort ────────────────────────────────────────────────

pub struct StreamingSort;

#[async_trait]
impl StreamingComputeFunction for StreamingSort {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingSort");
        // Collect all chunks, sort, re-emit
        let (tx, out) = mpsc::channel(2);
        let mut rx = rx;
        tokio::spawn(async move {
            let mut chunks: Vec<Bytes> = Vec::new();
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => chunks.push(chunk),
                    Some(StreamChunk::End) | None => break,
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                }
            }
            chunks.sort_unstable_by(|a, b| a.as_ref().cmp(b.as_ref()));
            for chunk in chunks {
                if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}

// ── #50 StreamingUnique ──────────────────────────────────────────────

pub struct StreamingUnique;

#[async_trait]
impl StreamingComputeFunction for StreamingUnique {
    async fn stream_execute(&self, mut inputs: Vec<mpsc::Receiver<StreamChunk>>, _params: &HashMap<String, String>) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "StreamingUnique");
        let (tx, out) = mpsc::channel(2);
        let mut rx = rx;
        tokio::spawn(async move {
            let mut chunks: Vec<Bytes> = Vec::new();
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => chunks.push(chunk),
                    Some(StreamChunk::End) | None => break,
                    Some(StreamChunk::Error(e)) => { let _ = tx.send(StreamChunk::Error(e)).await; return; }
                }
            }
            chunks.sort_unstable_by(|a, b| a.as_ref().cmp(b.as_ref()));
            chunks.dedup_by(|a, b| a.as_ref() == b.as_ref());
            for chunk in chunks {
                if tx.send(StreamChunk::Data(chunk)).await.is_err() { return; }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}
