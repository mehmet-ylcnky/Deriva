use std::collections::HashMap;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use deriva_core::DerivaError;

/// Default chunk size for streaming: 64KB
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;

/// Default channel capacity (number of chunks buffered)
pub const DEFAULT_CHANNEL_CAPACITY: usize = 8;

/// A compute function that supports streaming input and output.
///
/// # Contract
/// - Input receivers yield zero or more `Data` chunks followed by exactly one `End` or `Error`.
/// - The returned receiver must follow the same protocol.
/// - If any input yields `Error`, the function should propagate the error and terminate.
#[async_trait::async_trait]
pub trait StreamingComputeFunction: Send + Sync {
    /// Process streaming inputs and produce streaming output.
    async fn stream_execute(
        &self,
        inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk>;

    /// Whether this function supports streaming.
    fn supports_streaming(&self) -> bool {
        true
    }

    /// Preferred chunk size in bytes.
    fn preferred_chunk_size(&self) -> usize {
        DEFAULT_CHUNK_SIZE
    }

    /// Channel capacity for this function's output.
    fn channel_capacity(&self) -> usize {
        DEFAULT_CHANNEL_CAPACITY
    }
}

/// Wraps a batch result as a chunked stream.
pub fn batch_to_stream(data: Bytes, chunk_size: usize) -> mpsc::Receiver<StreamChunk> {
    value_to_stream(data, chunk_size, DEFAULT_CHANNEL_CAPACITY)
}

/// Wraps a leaf/blob value as a chunked stream.
pub fn value_to_stream(
    data: Bytes,
    chunk_size: usize,
    capacity: usize,
) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(capacity);
    tokio::spawn(async move {
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + chunk_size).min(data.len());
            let chunk = data.slice(offset..end);
            if tx.send(StreamChunk::Data(chunk)).await.is_err() {
                return;
            }
            offset = end;
        }
        let _ = tx.send(StreamChunk::End).await;
    });
    rx
}

/// Collects a stream back into a single Bytes value.
pub async fn collect_stream(mut rx: mpsc::Receiver<StreamChunk>) -> Result<Bytes, DerivaError> {
    let mut parts: Vec<Bytes> = Vec::new();
    let mut total_len = 0usize;

    loop {
        match rx.recv().await {
            Some(StreamChunk::Data(chunk)) => {
                total_len += chunk.len();
                parts.push(chunk);
            }
            Some(StreamChunk::End) => break,
            Some(StreamChunk::Error(e)) => return Err(e),
            None => {
                return Err(DerivaError::ComputeFailed(
                    "stream closed without End marker".into(),
                ));
            }
        }
    }

    if parts.len() == 1 {
        Ok(parts.into_iter().next().unwrap())
    } else {
        let mut buf = Vec::with_capacity(total_len);
        for part in parts {
            buf.extend_from_slice(&part);
        }
        Ok(Bytes::from(buf))
    }
}
