use std::collections::HashMap;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;

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
