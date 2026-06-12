use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;

use deriva_core::streaming::StreamChunk;

use crate::streaming::{StreamingComputeFunction, DEFAULT_CHANNEL_CAPACITY};

/// A composite streaming stage that applies a chain of fusible transforms
/// sequentially per chunk within a single Tokio task.
///
/// Eliminates inter-stage channel hops by applying all transforms in sequence
/// for each input chunk, forwarding the final result to the output channel.
pub struct FusedMapStage {
    /// Ordered vector of (function, params) pairs. Applied in sequence per chunk.
    stages: Vec<(Arc<dyn StreamingComputeFunction>, HashMap<String, String>)>,
}

impl FusedMapStage {
    /// Create a new fused stage from a vector of (function, params) pairs.
    ///
    /// # Panics (debug builds)
    /// Panics if `stages.len() < 2` — fusion requires at least two stages.
    pub fn new(
        stages: Vec<(Arc<dyn StreamingComputeFunction>, HashMap<String, String>)>,
    ) -> Self {
        debug_assert!(
            stages.len() >= 2,
            "FusedMapStage requires at least 2 stages, got {}",
            stages.len()
        );
        Self { stages }
    }

    /// Number of contained transforms.
    pub fn len(&self) -> usize {
        self.stages.len()
    }

    /// Returns true if there are no contained transforms.
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// Names of contained transforms (for observability).
    pub fn stage_names(&self) -> Vec<&'static str> {
        self.stages.iter().map(|(f, _)| f.metric_name()).collect()
    }
}

/// Apply the full chain of transforms to a single chunk of bytes.
/// Returns the final transformed bytes, or sends an error to `tx` and returns `None`.
async fn apply_chain(
    stages: &[(Arc<dyn StreamingComputeFunction>, HashMap<String, String>)],
    initial: Bytes,
    tx: &mpsc::Sender<StreamChunk>,
) -> Option<Bytes> {
    let mut current_bytes = initial;

    for (stage_fn, stage_params) in stages {
        // Create a synthetic single-chunk input: Data + End
        let (syn_tx, syn_rx) = mpsc::channel(2);
        let _ = syn_tx.send(StreamChunk::Data(current_bytes)).await;
        let _ = syn_tx.send(StreamChunk::End).await;
        drop(syn_tx);

        // Execute the transform
        let mut result_rx = stage_fn.stream_execute(vec![syn_rx], stage_params).await;

        // Collect the output — expect one or more Data chunks then End
        let mut collected: Vec<Bytes> = Vec::new();
        loop {
            match result_rx.recv().await {
                Some(StreamChunk::Data(chunk)) => {
                    collected.push(chunk);
                }
                Some(StreamChunk::End) => break,
                Some(StreamChunk::Error(e)) => {
                    // Mid-chain error: propagate to output
                    let _ = tx.send(StreamChunk::Error(e)).await;
                    return None;
                }
                None => {
                    // Channel closed without End — treat as error
                    let _ = tx
                        .send(StreamChunk::Error(
                            deriva_core::DerivaError::ComputeFailed(
                                "fused stage inner transform closed without End".into(),
                            ),
                        ))
                        .await;
                    return None;
                }
            }
        }

        // Combine collected chunks into single Bytes for next stage
        current_bytes = if collected.len() == 1 {
            collected.into_iter().next().unwrap()
        } else {
            let total_len: usize = collected.iter().map(|c| c.len()).sum();
            let mut buf = Vec::with_capacity(total_len);
            for chunk in collected {
                buf.extend_from_slice(&chunk);
            }
            Bytes::from(buf)
        };
    }

    Some(current_bytes)
}

#[async_trait]
impl StreamingComputeFunction for FusedMapStage {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        assert!(inputs.len() == 1, "FusedMapStage expects exactly 1 input");
        let mut input_rx = inputs.remove(0);

        let (tx, rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

        // Clone Arc references and params for the spawned task.
        let stages: Vec<(Arc<dyn StreamingComputeFunction>, HashMap<String, String>)> =
            self.stages.iter().map(|(f, p)| (Arc::clone(f), p.clone())).collect();

        tokio::spawn(async move {
            loop {
                match input_rx.recv().await {
                    Some(StreamChunk::Data(bytes)) => {
                        // Apply each transform sequentially
                        match apply_chain(&stages, bytes, &tx).await {
                            Some(result) => {
                                if tx.send(StreamChunk::Data(result)).await.is_err() {
                                    // Downstream dropped — stop processing
                                    return;
                                }
                            }
                            None => {
                                // Error already sent by apply_chain
                                return;
                            }
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
                    None => {
                        // Input channel closed without End/Error — terminate
                        return;
                    }
                }
            }
        });

        rx
    }

    fn is_fusible(&self) -> bool {
        false
    }

    fn metric_name(&self) -> &'static str {
        "FusedMapStage"
    }
}
