use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::{CAddr, DerivaError, Value};
use deriva_core::streaming::StreamChunk;
use crate::adaptive::{AdaptiveChunkConfig, spawn_adaptive_resizer};
use crate::fusion::fuse_pipeline;
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
    /// Enable adaptive chunk sizing for the pipeline (§2.10).
    /// When true, AdaptiveResizer nodes are inserted between adjacent streaming stages.
    pub adaptive_chunking: bool,
    /// Minimum chunk size the AdaptiveResizer can produce (absolute floor: 1024 bytes).
    pub min_chunk_size: usize,
    /// Maximum chunk size the AdaptiveResizer can produce (default: 1 MB).
    pub max_chunk_size: usize,
    /// Enable pipeline fusion (§2.11).
    /// When true, adjacent fusible streaming stages are merged into a single
    /// FusedMapStage to eliminate inter-stage channel hops.
    pub enable_fusion: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            cache_intermediates: true,
            memory_budget: 0,
            streaming_threshold: DEFAULT_STREAMING_THRESHOLD,
            adaptive_chunking: false,
            min_chunk_size: 1024,
            max_chunk_size: 1_048_576,
            enable_fusion: true,
        }
    }
}

impl PipelineConfig {
    /// Validate that adaptive chunking configuration fields are consistent.
    ///
    /// Checks:
    /// - `min_chunk_size >= 1024` (absolute floor)
    /// - `min_chunk_size <= chunk_size`
    /// - `chunk_size <= max_chunk_size`
    pub fn validate(&self) -> Result<(), String> {
        if self.min_chunk_size < 1024 {
            return Err(format!(
                "min_chunk_size must be >= 1024, got {}",
                self.min_chunk_size
            ));
        }
        if self.min_chunk_size > self.chunk_size {
            return Err(format!(
                "min_chunk_size ({}) must be <= chunk_size ({})",
                self.min_chunk_size, self.chunk_size
            ));
        }
        if self.chunk_size > self.max_chunk_size {
            return Err(format!(
                "chunk_size ({}) must be <= max_chunk_size ({})",
                self.chunk_size, self.max_chunk_size
            ));
        }
        Ok(())
    }
}

/// Lightweight classification of pipeline nodes for adaptive chunking decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeType {
    Source,
    Cached,
    Streaming,
    Batch,
}

pub(crate) enum PipelineNode {
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

        // §2.11 Pipeline fusion: conditionally fuse adjacent fusible stages.
        // Fusion is skipped when adaptive_chunking is enabled because fusion
        // eliminates inter-stage channels where adaptive resizers are inserted.
        let nodes = if self.config.enable_fusion && !self.config.adaptive_chunking {
            let (fused_nodes, stats) = fuse_pipeline(self.nodes);
            if stats.stages_eliminated > 0 {
                metrics::record_fusion(stats.stages_eliminated);
                tracing::debug!(
                    original_count = stats.original_count,
                    fused_count = stats.fused_count,
                    stages_eliminated = stats.stages_eliminated,
                    fused_groups = stats.fused_groups,
                    "pipeline fusion applied"
                );
            }
            fused_nodes
        } else {
            self.nodes
        };

        // Pre-compute node types for adaptive chunking decisions.
        let node_types: Vec<NodeType> = nodes.iter().map(|node| match node {
            PipelineNode::Source { .. } => NodeType::Source,
            PipelineNode::Cached { .. } => NodeType::Cached,
            PipelineNode::StreamingStage { .. } => NodeType::Streaming,
            PipelineNode::BatchStage { .. } => NodeType::Batch,
        }).collect();

        let mut outputs: Vec<Option<mpsc::Receiver<StreamChunk>>> =
            Vec::with_capacity(nodes.len());

        for node in nodes {
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
                            let mut rx = outputs[i].take().expect(
                                "input already consumed — DAG has fan-out on streaming node"
                            );
                            // Adaptive chunking: insert resizer between adjacent streaming stages
                            if self.config.adaptive_chunking && node_types[i] == NodeType::Streaming {
                                let initial_target = function.preferred_chunk_size()
                                    .clamp(self.config.min_chunk_size, self.config.max_chunk_size);
                                rx = spawn_adaptive_resizer(
                                    rx,
                                    initial_target,
                                    self.config.min_chunk_size,
                                    self.config.max_chunk_size,
                                    self.config.channel_capacity,
                                    AdaptiveChunkConfig::default(),
                                    function.metric_name(),
                                );
                            }
                            rx
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
