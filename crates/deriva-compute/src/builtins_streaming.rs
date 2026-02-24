use std::collections::HashMap;
use std::io::Write;

use async_trait::async_trait;
use base64::Engine;
use bytes::{Bytes, BytesMut};
use tokio::sync::mpsc;

use deriva_core::streaming::StreamChunk;

use crate::streaming::{StreamingComputeFunction, DEFAULT_CHANNEL_CAPACITY};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Spawn a task that reads one input, transforms each Data chunk, and forwards.
fn spawn_map(
    mut rx: mpsc::Receiver<StreamChunk>,
    cap: usize,
    f: impl Fn(&[u8]) -> Result<Bytes, String> + Send + 'static,
) -> mpsc::Receiver<StreamChunk> {
    let (tx, out) = mpsc::channel(cap);
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Some(StreamChunk::Data(chunk)) => {
                    let mapped = match f(&chunk) {
                        Ok(b) => StreamChunk::Data(b),
                        Err(e) => {
                            let _ = tx
                                .send(StreamChunk::Error(
                                    deriva_core::DerivaError::ComputeFailed(e),
                                ))
                                .await;
                            return;
                        }
                    };
                    if tx.send(mapped).await.is_err() {
                        return;
                    }
                }
                Some(StreamChunk::End) => {
                    let _ = tx.send(StreamChunk::End).await;
                    return;
                }
                Some(StreamChunk::Error(e)) => {
                    let _ = tx.send(StreamChunk::Error(e)).await;
                    return;
                }
                None => return,
            }
        }
    });
    out
}

/// Spawn a task that accumulates all input, then emits a single result.
fn spawn_accumulate<S: Send + 'static>(
    mut rx: mpsc::Receiver<StreamChunk>,
    init: S,
    fold: impl Fn(&mut S, &[u8]) + Send + 'static,
    finalize: impl FnOnce(S) -> Bytes + Send + 'static,
) -> mpsc::Receiver<StreamChunk> {
    let (tx, out) = mpsc::channel(2);
    tokio::spawn(async move {
        let mut state = init;
        loop {
            match rx.recv().await {
                Some(StreamChunk::Data(chunk)) => fold(&mut state, &chunk),
                Some(StreamChunk::End) => break,
                Some(StreamChunk::Error(e)) => {
                    let _ = tx.send(StreamChunk::Error(e)).await;
                    return;
                }
                None => break,
            }
        }
        let _ = tx.send(StreamChunk::Data(finalize(state))).await;
        let _ = tx.send(StreamChunk::End).await;
    });
    out
}

fn take_one(inputs: &mut Vec<mpsc::Receiver<StreamChunk>>, name: &str) -> mpsc::Receiver<StreamChunk> {
    assert_eq!(inputs.len(), 1, "{name} takes exactly 1 input");
    inputs.remove(0)
}

// ===========================================================================
// Category 1: Single-Input Chunk-by-Chunk Transforms
// ===========================================================================

/// 1. Passes input chunks through unchanged.
pub struct StreamingIdentity;

#[async_trait]
impl StreamingComputeFunction for StreamingIdentity {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        take_one(&mut inputs, "identity")
    }
}

/// 2. Converts each chunk to uppercase ASCII.
pub struct StreamingUppercase;

#[async_trait]
impl StreamingComputeFunction for StreamingUppercase {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "uppercase");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            Ok(Bytes::from(b.to_ascii_uppercase()))
        })
    }
}

/// 3. Converts each chunk to lowercase ASCII.
pub struct StreamingLowercase;

#[async_trait]
impl StreamingComputeFunction for StreamingLowercase {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "lowercase");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            Ok(Bytes::from(b.to_ascii_lowercase()))
        })
    }
}

/// 4. Reverses bytes within each chunk.
pub struct StreamingReverse;

#[async_trait]
impl StreamingComputeFunction for StreamingReverse {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "reverse");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            let mut v = b.to_vec();
            v.reverse();
            Ok(Bytes::from(v))
        })
    }
}

/// 5. Base64-encodes each chunk.
pub struct StreamingBase64Encode;

#[async_trait]
impl StreamingComputeFunction for StreamingBase64Encode {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "base64_encode");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            Ok(Bytes::from(
                base64::engine::general_purpose::STANDARD.encode(b),
            ))
        })
    }
}

/// 6. Base64-decodes each chunk.
pub struct StreamingBase64Decode;

#[async_trait]
impl StreamingComputeFunction for StreamingBase64Decode {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "base64_decode");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            base64::engine::general_purpose::STANDARD
                .decode(b)
                .map(Bytes::from)
                .map_err(|e| format!("base64 decode: {e}"))
        })
    }
}

/// 7. XORs each byte with `params["key"]` (single byte).
pub struct StreamingXor;

#[async_trait]
impl StreamingComputeFunction for StreamingXor {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "xor");
        let key: u8 = params
            .get("key")
            .and_then(|k| k.parse().ok())
            .unwrap_or(0xFF);
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, move |b| {
            Ok(Bytes::from(b.iter().map(|byte| byte ^ key).collect::<Vec<_>>()))
        })
    }
}

/// 8. Zlib-compresses each chunk independently.
pub struct StreamingCompress;

#[async_trait]
impl StreamingComputeFunction for StreamingCompress {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "compress");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            enc.write_all(b).map_err(|e| format!("compress: {e}"))?;
            enc.finish()
                .map(Bytes::from)
                .map_err(|e| format!("compress finish: {e}"))
        })
    }
}

/// 9. Zlib-decompresses each chunk independently.
pub struct StreamingDecompress;

#[async_trait]
impl StreamingComputeFunction for StreamingDecompress {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "decompress");
        spawn_map(rx, DEFAULT_CHANNEL_CAPACITY, |b| {
            let mut dec = flate2::write::ZlibDecoder::new(Vec::new());
            dec.write_all(b).map_err(|e| format!("decompress: {e}"))?;
            dec.finish()
                .map(Bytes::from)
                .map_err(|e| format!("decompress finish: {e}"))
        })
    }
}

// ===========================================================================
// Category 2: Single-Input Accumulators
// ===========================================================================

/// 10. Rolling SHA-256, emits 32-byte digest.
pub struct StreamingSha256;

#[async_trait]
impl StreamingComputeFunction for StreamingSha256 {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        use sha2::{Digest, Sha256};
        let rx = take_one(&mut inputs, "sha256");
        spawn_accumulate(
            rx,
            Sha256::new(),
            |h, b| {
                h.update(b);
            },
            |h| Bytes::copy_from_slice(&h.finalize()),
        )
    }
}

/// 11. Counts total bytes, emits u64 as 8-byte big-endian.
pub struct StreamingByteCount;

#[async_trait]
impl StreamingComputeFunction for StreamingByteCount {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "byte_count");
        spawn_accumulate(
            rx,
            0u64,
            |count, b| *count += b.len() as u64,
            |count| Bytes::copy_from_slice(&count.to_be_bytes()),
        )
    }
}

/// 12. Rolling CRC32, emits 4-byte big-endian checksum.
pub struct StreamingChecksum;

#[async_trait]
impl StreamingComputeFunction for StreamingChecksum {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let rx = take_one(&mut inputs, "checksum");
        spawn_accumulate(
            rx,
            crc32fast::Hasher::new(),
            |h, b| h.update(b),
            |h| Bytes::copy_from_slice(&h.finalize().to_be_bytes()),
        )
    }
}

// ===========================================================================
// Category 3: Multi-Input Combiners
// ===========================================================================

/// 13. Reads inputs sequentially, emitting chunks as they arrive.
pub struct StreamingConcat;

#[async_trait]
impl StreamingComputeFunction for StreamingConcat {
    async fn stream_execute(
        &self,
        inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let (tx, rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
        tokio::spawn(async move {
            for mut input_rx in inputs {
                loop {
                    match input_rx.recv().await {
                        Some(StreamChunk::Data(c)) => {
                            if tx.send(StreamChunk::Data(c)).await.is_err() {
                                return;
                            }
                        }
                        Some(StreamChunk::End) | None => break,
                        Some(StreamChunk::Error(e)) => {
                            let _ = tx.send(StreamChunk::Error(e)).await;
                            return;
                        }
                    }
                }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        rx
    }
}

/// 14. Round-robin chunks from N inputs.
pub struct StreamingInterleave;

#[async_trait]
impl StreamingComputeFunction for StreamingInterleave {
    async fn stream_execute(
        &self,
        inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let (tx, rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
        tokio::spawn(async move {
            let mut streams: Vec<Option<mpsc::Receiver<StreamChunk>>> =
                inputs.into_iter().map(Some).collect();
            let mut alive = streams.len();
            while alive > 0 {
                for slot in streams.iter_mut() {
                    let Some(ref mut s) = slot else { continue };
                    match s.recv().await {
                        Some(StreamChunk::Data(c)) => {
                            if tx.send(StreamChunk::Data(c)).await.is_err() {
                                return;
                            }
                        }
                        Some(StreamChunk::End) | None => {
                            *slot = None;
                            alive -= 1;
                        }
                        Some(StreamChunk::Error(e)) => {
                            let _ = tx.send(StreamChunk::Error(e)).await;
                            return;
                        }
                    }
                }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        rx
    }
}

/// 15. Pair-wise concatenate chunks from exactly 2 inputs.
pub struct StreamingZipConcat;

#[async_trait]
impl StreamingComputeFunction for StreamingZipConcat {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        assert_eq!(inputs.len(), 2, "zip_concat takes exactly 2 inputs");
        let mut rx_b = inputs.remove(1);
        let mut rx_a = inputs.remove(0);
        let (tx, rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

        tokio::spawn(async move {
            loop {
                let a = rx_a.recv().await;
                let b = rx_b.recv().await;
                match (a, b) {
                    (Some(StreamChunk::Data(da)), Some(StreamChunk::Data(db))) => {
                        let mut buf = BytesMut::with_capacity(da.len() + db.len());
                        buf.extend_from_slice(&da);
                        buf.extend_from_slice(&db);
                        if tx.send(StreamChunk::Data(buf.freeze())).await.is_err() {
                            return;
                        }
                    }
                    (Some(StreamChunk::Error(e)), _) | (_, Some(StreamChunk::Error(e))) => {
                        let _ = tx.send(StreamChunk::Error(e)).await;
                        return;
                    }
                    // Either or both ended — drain the other and finish
                    (Some(StreamChunk::Data(d)), _) | (_, Some(StreamChunk::Data(d))) => {
                        let _ = tx.send(StreamChunk::Data(d)).await;
                        break;
                    }
                    _ => break,
                }
            }
            // Drain remaining from both
            for rx_rem in [&mut rx_a, &mut rx_b] {
                loop {
                    match rx_rem.recv().await {
                        Some(StreamChunk::Data(c)) => {
                            if tx.send(StreamChunk::Data(c)).await.is_err() {
                                return;
                            }
                        }
                        Some(StreamChunk::End) | None => break,
                        Some(StreamChunk::Error(e)) => {
                            let _ = tx.send(StreamChunk::Error(e)).await;
                            return;
                        }
                    }
                }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        rx
    }
}

// ===========================================================================
// Category 4: Pipeline Utilities
// ===========================================================================

/// 16. Re-chunks to `params["target_size"]` (default 64KB).
pub struct StreamingChunkResizer;

#[async_trait]
impl StreamingComputeFunction for StreamingChunkResizer {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "chunk_resizer");
        let target: usize = params
            .get("target_size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(crate::streaming::DEFAULT_CHUNK_SIZE);
        let (tx, out) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

        tokio::spawn(async move {
            let mut buf = BytesMut::new();
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        buf.extend_from_slice(&chunk);
                        while buf.len() >= target {
                            let piece = buf.split_to(target).freeze();
                            if tx.send(StreamChunk::Data(piece)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Some(StreamChunk::End) => break,
                    Some(StreamChunk::Error(e)) => {
                        let _ = tx.send(StreamChunk::Error(e)).await;
                        return;
                    }
                    None => break,
                }
            }
            if !buf.is_empty() {
                let _ = tx.send(StreamChunk::Data(buf.freeze())).await;
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}

/// 17. Emits only the first `params["bytes"]` bytes, then End.
pub struct StreamingTake;

#[async_trait]
impl StreamingComputeFunction for StreamingTake {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "take");
        let limit: usize = params
            .get("bytes")
            .and_then(|s| s.parse().ok())
            .unwrap_or(usize::MAX);
        let (tx, out) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

        tokio::spawn(async move {
            let mut remaining = limit;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        if remaining == 0 {
                            break;
                        }
                        if chunk.len() <= remaining {
                            remaining -= chunk.len();
                            if tx.send(StreamChunk::Data(chunk)).await.is_err() {
                                return;
                            }
                        } else {
                            let _ = tx
                                .send(StreamChunk::Data(chunk.slice(..remaining)))
                                .await;
                            break;
                        }
                    }
                    Some(StreamChunk::End) | None => break,
                    Some(StreamChunk::Error(e)) => {
                        let _ = tx.send(StreamChunk::Error(e)).await;
                        return;
                    }
                }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}

/// 18. Skips the first `params["bytes"]` bytes, then passes through.
pub struct StreamingSkip;

#[async_trait]
impl StreamingComputeFunction for StreamingSkip {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "skip");
        let skip: usize = params
            .get("bytes")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let (tx, out) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

        tokio::spawn(async move {
            let mut to_skip = skip;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        if to_skip == 0 {
                            if tx.send(StreamChunk::Data(chunk)).await.is_err() {
                                return;
                            }
                        } else if chunk.len() <= to_skip {
                            to_skip -= chunk.len();
                        } else {
                            let remainder = chunk.slice(to_skip..);
                            to_skip = 0;
                            if tx.send(StreamChunk::Data(remainder)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Some(StreamChunk::End) | None => break,
                    Some(StreamChunk::Error(e)) => {
                        let _ = tx.send(StreamChunk::Error(e)).await;
                        return;
                    }
                }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}

/// 19. Repeats the full input `params["count"]` times.
pub struct StreamingRepeat;

#[async_trait]
impl StreamingComputeFunction for StreamingRepeat {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "repeat");
        let count: usize = params
            .get("count")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let (tx, out) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

        tokio::spawn(async move {
            // Collect full input
            let mut buf = BytesMut::new();
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(c)) => buf.extend_from_slice(&c),
                    Some(StreamChunk::End) | None => break,
                    Some(StreamChunk::Error(e)) => {
                        let _ = tx.send(StreamChunk::Error(e)).await;
                        return;
                    }
                }
            }
            let data = buf.freeze();
            for _ in 0..count {
                if tx.send(StreamChunk::Data(data.clone())).await.is_err() {
                    return;
                }
            }
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}

/// 20. Passes data through, appends byte count as decimal ASCII trailer.
pub struct StreamingTeeCount;

#[async_trait]
impl StreamingComputeFunction for StreamingTeeCount {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        let mut rx = take_one(&mut inputs, "tee_count");
        let (tx, out) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

        tokio::spawn(async move {
            let mut total: u64 = 0;
            loop {
                match rx.recv().await {
                    Some(StreamChunk::Data(c)) => {
                        total += c.len() as u64;
                        if tx.send(StreamChunk::Data(c)).await.is_err() {
                            return;
                        }
                    }
                    Some(StreamChunk::End) | None => break,
                    Some(StreamChunk::Error(e)) => {
                        let _ = tx.send(StreamChunk::Error(e)).await;
                        return;
                    }
                }
            }
            let _ = tx
                .send(StreamChunk::Data(Bytes::from(total.to_string())))
                .await;
            let _ = tx.send(StreamChunk::End).await;
        });
        out
    }
}
