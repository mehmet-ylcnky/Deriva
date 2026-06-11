use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::{CAddr, DerivaError, Value};
use deriva_core::streaming::StreamChunk;
use crate::streaming::{
    StreamingComputeFunction, batch_to_stream, value_to_stream,
    collect_stream, DEFAULT_CHUNK_SIZE, DEFAULT_CHANNEL_CAPACITY,
};
use crate::ComputeFunction;
use crate::metrics;

/// Default streaming threshold: 3 MB.
/// Below this, batch execution is preferred for functions that support both modes.
pub const DEFAULT_STREAMING_THRESHOLD: usize = 3 * 1024 * 1024;

/// Configuration for a streaming pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub chunk_size: usize,
    pub channel_capacity: usize,
    pub cache_intermediates: bool,
    pub memory_budget: usize,
    /// Input size threshold in bytes. Nodes whose total input size
    /// is below this value prefer batch execution when available.
    /// Set to 0 to always prefer streaming (§2.7 behaviour).
    /// Set to usize::MAX to always prefer batch.
    pub streaming_threshold: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            cache_intermediates: true,
            memory_budget: 0,
            streaming_threshold: DEFAULT_STREAMING_THRESHOLD,
        }
    }
}

enum PipelineNode {
    Source { _addr: CAddr, data: Bytes },
    Cached { _addr: CAddr, data: Bytes },
    StreamingStage {
        _addr: CAddr,
        function: Arc<dyn StreamingComputeFunction>,
        params: HashMap<String, String>,
        input_indices: Vec<usize>,
    },
    BatchStage {
        _addr: CAddr,
        function: Arc<dyn ComputeFunction>,
        params: BTreeMap<String, Value>,
        input_indices: Vec<usize>,
    },
}

/// Executes a streaming pipeline for a recipe DAG.
pub struct StreamPipeline {
    nodes: Vec<PipelineNode>,
    config: PipelineConfig,
}

impl StreamPipeline {
    pub fn new(config: PipelineConfig) -> Self {
        Self { nodes: Vec::new(), config }
    }

    pub fn add_source(&mut self, addr: CAddr, data: Bytes) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(PipelineNode::Source { _addr: addr, data });
        idx
    }

    pub fn add_cached(&mut self, addr: CAddr, data: Bytes) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(PipelineNode::Cached { _addr: addr, data });
        idx
    }

    pub fn add_streaming_stage(
        &mut self,
        addr: CAddr,
        function: Arc<dyn StreamingComputeFunction>,
        params: HashMap<String, String>,
        input_indices: Vec<usize>,
    ) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(PipelineNode::StreamingStage {
            _addr: addr, function, params, input_indices,
        });
        idx
    }

    pub fn add_batch_stage(
        &mut self,
        addr: CAddr,
        function: Arc<dyn ComputeFunction>,
        params: BTreeMap<String, Value>,
        input_indices: Vec<usize>,
    ) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(PipelineNode::BatchStage {
            _addr: addr, function, params, input_indices,
        });
        idx
    }

    /// Returns the known data size for a pipeline node, or `None` for computed nodes.
    pub fn node_data_size(&self, idx: usize) -> Option<usize> {
        match &self.nodes[idx] {
            PipelineNode::Source { data, .. } => Some(data.len()),
            PipelineNode::Cached { data, .. } => Some(data.len()),
            PipelineNode::StreamingStage { .. } => None,
            PipelineNode::BatchStage { .. } => None,
        }
    }

    /// Returns the total known input size for a set of input node indices.
    /// Returns `None` if any input has unknown size.
    pub fn total_input_size(&self, input_indices: &[usize]) -> Option<usize> {
        let mut total: usize = 0;
        for &idx in input_indices {
            let size = self.node_data_size(idx)?;
            total = total.checked_add(size)?;
        }
        Some(total)
    }

    /// Execute the pipeline, returning the output stream of the last node.
    pub async fn execute(self) -> Result<mpsc::Receiver<StreamChunk>, DerivaError> {
        let start = std::time::Instant::now();
        metrics::STREAM_PIPELINES_TOTAL.inc();

        let mut outputs: Vec<Option<mpsc::Receiver<StreamChunk>>> =
            Vec::with_capacity(self.nodes.len());

        for node in self.nodes {
            match node {
                PipelineNode::Source { data, .. }
                | PipelineNode::Cached { data, .. } => {
                    let rx = value_to_stream(
                        data,
                        self.config.chunk_size,
                        self.config.channel_capacity,
                    );
                    outputs.push(Some(rx));
                }

                PipelineNode::StreamingStage {
                    function, params, input_indices, ..
                } => {
                    let inputs: Vec<mpsc::Receiver<StreamChunk>> =
                        input_indices.iter().map(|&i| {
                            outputs[i].take().expect(
                                "input already consumed — DAG has fan-out on streaming node"
                            )
                        }).collect();
                    let fn_name = function.metric_name();
                    crate::metrics::STREAMING_FN_CALLS.with_label_values(&[fn_name]).inc();
                    let timer = crate::metrics::STREAMING_FN_DURATION.with_label_values(&[fn_name]).start_timer();
                    let mut rx = function.stream_execute(inputs, &params).await;
                    // Wrap output to observe duration on End and count errors
                    let (tx, out) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
                    let err_name = fn_name.to_string();
                    tokio::spawn(async move {
                        while let Some(chunk) = rx.recv().await {
                            match &chunk {
                                StreamChunk::End => {
                                    timer.observe_duration();
                                    let _ = tx.send(chunk).await;
                                    return;
                                }
                                StreamChunk::Error(_) => {
                                    crate::metrics::STREAMING_FN_ERRORS.with_label_values(&[&err_name]).inc();
                                    timer.observe_duration();
                                    let _ = tx.send(chunk).await;
                                    return;
                                }
                                _ => {
                                    if tx.send(chunk).await.is_err() {
                                        timer.observe_duration();
                                        return;
                                    }
                                }
                            }
                        }
                        timer.observe_duration();
                    });
                    outputs.push(Some(out));
                }

                PipelineNode::BatchStage {
                    function, params, input_indices, ..
                } => {
                    let mut input_bytes = Vec::with_capacity(input_indices.len());
                    for &i in &input_indices {
                        let rx = outputs[i].take().expect("input already consumed");
                        let bytes = collect_stream(rx).await?;
                        input_bytes.push(bytes);
                    }
                    let result = function.execute(input_bytes, &params)
                        .map_err(|e| DerivaError::ComputeFailed(e.to_string()))?;
                    let rx = batch_to_stream(result, self.config.chunk_size);
                    outputs.push(Some(rx));
                }
            }
        }

        let raw = outputs.last_mut()
            .and_then(|o| o.take())
            .ok_or_else(|| DerivaError::ComputeFailed("empty pipeline".into()))?;

        // Wrap output stream to track chunks, bytes, and pipeline duration.
        let (tx, rx) = mpsc::channel(self.config.channel_capacity);
        tokio::spawn(async move {
            let mut raw = raw;
            while let Some(chunk) = raw.recv().await {
                match &chunk {
                    StreamChunk::Data(d) => {
                        metrics::STREAM_CHUNKS_TOTAL.inc();
                        metrics::STREAM_BYTES_TOTAL.inc_by(d.len() as u64);
                    }
                    StreamChunk::End => {
                        metrics::STREAM_PIPELINE_DURATION.observe(start.elapsed().as_secs_f64());
                    }
                    StreamChunk::Error(_) => {
                        metrics::STREAM_PIPELINE_DURATION.observe(start.elapsed().as_secs_f64());
                    }
                }
                if tx.send(chunk).await.is_err() {
                    break;
                }
            }
        });
        Ok(rx)
    }
}
