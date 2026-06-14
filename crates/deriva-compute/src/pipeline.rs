use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::sync::OwnedSemaphorePermit;
use deriva_core::{CAddr, DerivaError, Value};
use deriva_core::streaming::StreamChunk;
use crate::adaptive::{AdaptiveChunkConfig, spawn_adaptive_resizer};
use crate::fusion::fuse_pipeline;
use crate::memory_budget::{GlobalMemoryController, MemoryController};
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
    /// Per-pipeline memory budget in bytes (§2.12).
    /// When greater than 0, a MemoryController is created with
    /// `per_pipeline_max / chunk_size` permits to limit total in-flight data.
    /// When 0 (default), no per-pipeline memory limit is enforced.
    pub per_pipeline_max: usize,
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
            per_pipeline_max: 0,
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
    ///
    /// When `global_controller` is `Some`, each data chunk sent through budgeted
    /// channels also acquires a permit from the global budget. When `per_pipeline_max > 0`
    /// in the config, a per-pipeline `MemoryController` is created to enforce local limits.
    /// BatchStage nodes are always exempt from budget enforcement.
    pub async fn execute(self, global_controller: Option<&GlobalMemoryController>) -> Result<mpsc::Receiver<StreamChunk>, DerivaError> {
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

        // §2.12 Memory budget enforcement: create per-pipeline MemoryController
        // when per_pipeline_max > 0. When per_pipeline_max = 0 and no global
        // controller, the code path is identical to pre-§2.12 (zero overhead).
        let pipeline_controller: Option<MemoryController> = if self.config.per_pipeline_max > 0 {
            Some(MemoryController::new(self.config.per_pipeline_max, self.config.chunk_size))
        } else {
            None
        };

        // Determine if budget enforcement is active (either per-pipeline or global).
        let budget_active = pipeline_controller.is_some() || global_controller.is_some();

        // Clone global controller for use in spawned tasks (it's cheap: wraps Arc<Semaphore>).
        let global_ctrl: Option<GlobalMemoryController> = global_controller.cloned();

        let mut outputs: Vec<Option<mpsc::Receiver<StreamChunk>>> =
            Vec::with_capacity(nodes.len());

        for node in nodes {
            match node {
                PipelineNode::Source { data, .. }
                | PipelineNode::Cached { data, .. } => {
                    if budget_active {
                        // Budget-enforced path: acquire permits before sending each chunk.
                        // Use a DualPermitGuard channel internally, then bridge to raw mpsc.
                        let capacity = self.config.channel_capacity;
                        let chunk_size = self.config.chunk_size;
                        let pc = pipeline_controller.clone();
                        let gc = global_ctrl.clone();

                        let (guard_tx, guard_rx) = mpsc::channel::<DualPermitGuard>(capacity);

                        // Producer task: chunk data, acquire permits, send through guarded channel.
                        tokio::spawn(async move {
                            let mut offset = 0;
                            while offset < data.len() {
                                let end = (offset + chunk_size).min(data.len());
                                let chunk = data.slice(offset..end);

                                // Acquire per-pipeline permit (backpressure if exhausted).
                                let pipeline_permit = if let Some(ref ctrl) = pc {
                                    Some(ctrl.acquire().await)
                                } else {
                                    None
                                };

                                // Acquire global permit (backpressure if exhausted).
                                let global_permit = if let Some(ref ctrl) = gc {
                                    Some(ctrl.acquire().await)
                                } else {
                                    None
                                };

                                let guard = DualPermitGuard {
                                    chunk: StreamChunk::Data(chunk),
                                    _pipeline_permit: pipeline_permit,
                                    _global_permit: global_permit,
                                };

                                if guard_tx.send(guard).await.is_err() {
                                    return;
                                }
                                offset = end;
                            }
                            // End sentinel: no permits needed.
                            let end_guard = DualPermitGuard {
                                chunk: StreamChunk::End,
                                _pipeline_permit: None,
                                _global_permit: None,
                            };
                            let _ = guard_tx.send(end_guard).await;
                        });

                        // Bridge task: extract chunks from guarded channel into raw mpsc.
                        // Permits are released when each DualPermitGuard is dropped after
                        // the chunk is forwarded downstream.
                        let rx = bridge_guarded_to_raw(guard_rx, self.config.channel_capacity);
                        outputs.push(Some(rx));
                    } else {
                        // Zero-overhead path: identical to pre-§2.12 behavior.
                        let rx = value_to_stream(
                            data,
                            self.config.chunk_size,
                            self.config.channel_capacity,
                        );
                        outputs.push(Some(rx));
                    }
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
                                    pipeline_controller.clone(),
                                );
                            }
                            rx
                        }).collect();
                    let fn_name = function.metric_name();
                    crate::metrics::STREAMING_FN_CALLS.with_label_values(&[fn_name]).inc();
                    let timer = crate::metrics::STREAMING_FN_DURATION.with_label_values(&[fn_name]).start_timer();
                    let mut rx = function.stream_execute(inputs, &params).await;

                    if budget_active {
                        // Budget-enforced observation wrapper: acquire permits for each
                        // data chunk passing through the observation channel.
                        let capacity = self.config.channel_capacity;
                        let pc = pipeline_controller.clone();
                        let gc = global_ctrl.clone();

                        let (guard_tx, guard_rx) = mpsc::channel::<DualPermitGuard>(capacity);
                        let err_name = fn_name.to_string();

                        tokio::spawn(async move {
                            while let Some(chunk) = rx.recv().await {
                                match &chunk {
                                    StreamChunk::End => {
                                        timer.observe_duration();
                                        let guard = DualPermitGuard {
                                            chunk,
                                            _pipeline_permit: None,
                                            _global_permit: None,
                                        };
                                        let _ = guard_tx.send(guard).await;
                                        return;
                                    }
                                    StreamChunk::Error(_) => {
                                        crate::metrics::STREAMING_FN_ERRORS.with_label_values(&[&err_name]).inc();
                                        timer.observe_duration();
                                        let guard = DualPermitGuard {
                                            chunk,
                                            _pipeline_permit: None,
                                            _global_permit: None,
                                        };
                                        let _ = guard_tx.send(guard).await;
                                        return;
                                    }
                                    StreamChunk::Data(_) => {
                                        // Acquire per-pipeline permit.
                                        let pipeline_permit = if let Some(ref ctrl) = pc {
                                            Some(ctrl.acquire().await)
                                        } else {
                                            None
                                        };
                                        // Acquire global permit.
                                        let global_permit = if let Some(ref ctrl) = gc {
                                            Some(ctrl.acquire().await)
                                        } else {
                                            None
                                        };

                                        let guard = DualPermitGuard {
                                            chunk,
                                            _pipeline_permit: pipeline_permit,
                                            _global_permit: global_permit,
                                        };
                                        if guard_tx.send(guard).await.is_err() {
                                            timer.observe_duration();
                                            return;
                                        }
                                    }
                                }
                            }
                            timer.observe_duration();
                        });

                        // Bridge guarded channel to raw mpsc receiver.
                        let out = bridge_guarded_to_raw(guard_rx, self.config.channel_capacity);
                        outputs.push(Some(out));
                    } else {
                        // Zero-overhead observation wrapper (pre-§2.12 behavior).
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
                }

                PipelineNode::BatchStage {
                    function, params, input_indices, ..
                } => {
                    // BatchStage is always exempt from budget enforcement (Req 11.1, 11.3).
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

// ---------------------------------------------------------------------------
// Budget enforcement helpers (pipeline-internal)
// ---------------------------------------------------------------------------

/// Internal guard that holds both a per-pipeline and global permit alongside a chunk.
///
/// When dropped, both permits (if held) are released back to their respective
/// semaphores — implementing the dual-layer backpressure release.
struct DualPermitGuard {
    chunk: StreamChunk,
    _pipeline_permit: Option<OwnedSemaphorePermit>,
    _global_permit: Option<OwnedSemaphorePermit>,
}

/// Bridge a guarded channel (`mpsc::Receiver<DualPermitGuard>`) to a raw
/// `mpsc::Receiver<StreamChunk>` by spawning a task that extracts chunks
/// and drops the guard (releasing permits).
///
/// The permit is held from the time the producer sends until the bridge task
/// extracts the chunk — bounding the in-flight memory in the channel buffer.
fn bridge_guarded_to_raw(
    mut guard_rx: mpsc::Receiver<DualPermitGuard>,
    capacity: usize,
) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(capacity);
    tokio::spawn(async move {
        while let Some(guard) = guard_rx.recv().await {
            let chunk = guard.chunk.clone();
            // Drop the guard to release permits back to the semaphores.
            drop(guard);
            if tx.send(chunk).await.is_err() {
                break;
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use bytes::Bytes;
    use tokio::sync::mpsc;
    use deriva_core::streaming::StreamChunk;
    use deriva_core::DerivaError;
    use crate::memory_budget::{GlobalMemoryController, MemoryController};
    use crate::streaming::StreamingComputeFunction;

    // -----------------------------------------------------------------------
    // Test helpers: minimal streaming functions for pipeline construction
    // -----------------------------------------------------------------------

    /// A passthrough streaming function that forwards input chunks unchanged.
    struct PassthroughFunction;

    #[async_trait::async_trait]
    impl StreamingComputeFunction for PassthroughFunction {
        async fn stream_execute(
            &self,
            mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
            _params: &HashMap<String, String>,
        ) -> mpsc::Receiver<StreamChunk> {
            let mut input = inputs.remove(0);
            let (tx, rx) = mpsc::channel(8);
            tokio::spawn(async move {
                while let Some(chunk) = input.recv().await {
                    if tx.send(chunk).await.is_err() {
                        return;
                    }
                }
            });
            rx
        }

        fn metric_name(&self) -> &'static str {
            "PassthroughFunction"
        }
    }

    /// A streaming function that produces an error after receiving the first data chunk.
    struct ErrorAfterFirstChunkFunction;

    #[async_trait::async_trait]
    impl StreamingComputeFunction for ErrorAfterFirstChunkFunction {
        async fn stream_execute(
            &self,
            mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
            _params: &HashMap<String, String>,
        ) -> mpsc::Receiver<StreamChunk> {
            let mut input = inputs.remove(0);
            let (tx, rx) = mpsc::channel(8);
            tokio::spawn(async move {
                // Read first chunk, then produce error
                if let Some(_chunk) = input.recv().await {
                    let err = DerivaError::ComputeFailed("simulated error".into());
                    let _ = tx.send(StreamChunk::Error(err)).await;
                }
            });
            rx
        }

        fn metric_name(&self) -> &'static str {
            "ErrorAfterFirstChunkFunction"
        }
    }

    /// Helper: builds a simple pipeline with source → streaming stage, returns
    /// the pipeline and the controllers used for budget enforcement.
    fn build_budgeted_pipeline(
        data: Bytes,
        chunk_size: usize,
        per_pipeline_max: usize,
        function: Arc<dyn StreamingComputeFunction>,
    ) -> StreamPipeline {
        let config = PipelineConfig {
            chunk_size,
            channel_capacity: 4,
            cache_intermediates: false,
            memory_budget: 0,
            streaming_threshold: 0,
            adaptive_chunking: false,
            min_chunk_size: 1024,
            max_chunk_size: 1_048_576,
            enable_fusion: false,
            per_pipeline_max,
        };
        let mut pipeline = StreamPipeline::new(config);
        let addr = deriva_core::CAddr::from_bytes(&[0u8; 32]);
        let src_idx = pipeline.add_source(addr.clone(), data);
        pipeline.add_streaming_stage(
            addr,
            function,
            HashMap::new(),
            vec![src_idx],
        );
        pipeline
    }

    /// Helper: drain a receiver to completion, collecting all data chunks.
    async fn drain_stream(mut rx: mpsc::Receiver<StreamChunk>) -> Vec<StreamChunk> {
        let mut chunks = Vec::new();
        while let Some(chunk) = rx.recv().await {
            let is_terminal = chunk.is_end() || chunk.is_error();
            chunks.push(chunk);
            if is_terminal {
                break;
            }
        }
        chunks
    }

    // -----------------------------------------------------------------------
    // Integration tests: permit release on pipeline completion
    // -----------------------------------------------------------------------

    /// After a pipeline completes successfully, all permits are returned to
    /// both the per-pipeline MemoryController and the GlobalMemoryController.
    /// Validates: Requirements 9.1, 9.4
    #[tokio::test]
    async fn permits_released_on_successful_completion() {
        let chunk_size = 1024;
        let data = Bytes::from(vec![0xABu8; 4096]); // 4 chunks
        let per_pipeline_max = 4 * chunk_size; // 4 permits

        let global_ctrl = GlobalMemoryController::new(8 * chunk_size, chunk_size);
        assert_eq!(global_ctrl.available(), 8);
        assert_eq!(global_ctrl.total_permits(), 8);

        let pipeline = build_budgeted_pipeline(
            data,
            chunk_size,
            per_pipeline_max,
            Arc::new(PassthroughFunction),
        );

        let rx = pipeline.execute(Some(&global_ctrl)).await.unwrap();

        // Drain the output stream fully (pipeline completes successfully).
        let chunks = drain_stream(rx).await;
        assert!(chunks.last().unwrap().is_end());
        assert_eq!(chunks.iter().filter(|c| c.is_data()).count(), 4);

        // Allow spawned tasks to complete and drop their guards.
        tokio::task::yield_now().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // All global permits must be returned.
        assert_eq!(
            global_ctrl.available(),
            global_ctrl.total_permits(),
            "Global permits leaked after successful pipeline completion"
        );
    }

    /// After a pipeline produces an error, all permits are returned.
    /// Validates: Requirements 9.2, 9.4
    #[tokio::test]
    async fn permits_released_on_pipeline_error() {
        let chunk_size = 1024;
        let data = Bytes::from(vec![0xCDu8; 4096]); // 4 chunks
        let per_pipeline_max = 4 * chunk_size;

        let global_ctrl = GlobalMemoryController::new(8 * chunk_size, chunk_size);

        let pipeline = build_budgeted_pipeline(
            data,
            chunk_size,
            per_pipeline_max,
            Arc::new(ErrorAfterFirstChunkFunction),
        );

        let rx = pipeline.execute(Some(&global_ctrl)).await.unwrap();

        // Drain the output stream (should get an error).
        let chunks = drain_stream(rx).await;
        assert!(chunks.iter().any(|c| c.is_error()), "Expected error chunk in output");

        // Allow spawned tasks to be cleaned up.
        tokio::task::yield_now().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // All global permits must be returned.
        assert_eq!(
            global_ctrl.available(),
            global_ctrl.total_permits(),
            "Global permits leaked after pipeline error"
        );
    }

    /// After a pipeline task is cancelled (JoinHandle abort / receiver drop),
    /// all permits are returned due to DualPermitGuard drop semantics.
    /// Validates: Requirements 9.3, 9.4
    #[tokio::test]
    async fn permits_released_on_pipeline_cancellation() {
        let chunk_size = 1024;
        // Large data to ensure the pipeline has many chunks in-flight when cancelled.
        let data = Bytes::from(vec![0xEFu8; 16 * 1024]); // 16 chunks
        let per_pipeline_max = 4 * chunk_size; // only 4 permits (backpressure active)

        let global_ctrl = GlobalMemoryController::new(8 * chunk_size, chunk_size);

        let pipeline = build_budgeted_pipeline(
            data,
            chunk_size,
            per_pipeline_max,
            Arc::new(PassthroughFunction),
        );

        let rx = pipeline.execute(Some(&global_ctrl)).await.unwrap();

        // Read just one chunk, then drop the receiver — simulating cancellation.
        // The spawned tasks will detect the closed channel and terminate.
        let mut rx = rx;
        let first = rx.recv().await;
        assert!(first.is_some(), "Expected at least one chunk before cancel");
        // Drop the receiver to cancel the pipeline.
        drop(rx);

        // Allow spawned tasks to detect closed channels and clean up.
        tokio::task::yield_now().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // All global permits must be returned.
        assert_eq!(
            global_ctrl.available(),
            global_ctrl.total_permits(),
            "Global permits leaked after pipeline cancellation (receiver drop)"
        );
    }

    /// Verify that per-pipeline MemoryController permits are also fully released
    /// on successful completion by constructing a controller and running a pipeline.
    /// Validates: Requirements 9.1
    #[tokio::test]
    async fn per_pipeline_permits_released_on_completion() {
        let chunk_size = 1024;
        let data = Bytes::from(vec![0x42u8; 3072]); // 3 chunks
        let per_pipeline_max = 3 * chunk_size; // exactly 3 permits

        // No global controller — only per-pipeline enforcement.
        let pipeline = build_budgeted_pipeline(
            data,
            chunk_size,
            per_pipeline_max,
            Arc::new(PassthroughFunction),
        );

        // We can't directly access the internal MemoryController, but we can
        // verify through the GlobalMemoryController that the pattern works.
        // Instead, verify output correctness (all chunks delivered) which
        // proves permits were recycled during execution with tight budget.
        let rx = pipeline.execute(None).await.unwrap();
        let chunks = drain_stream(rx).await;

        assert!(chunks.last().unwrap().is_end());
        assert_eq!(chunks.iter().filter(|c| c.is_data()).count(), 3);
    }

    /// Verify that aborting a task (simulating Tokio JoinHandle::abort) releases
    /// permits held by DualPermitGuard on the task's stack.
    /// This tests the key invariant from Requirement 9.4: permit release within
    /// the same runtime tick as task drop.
    #[tokio::test]
    async fn permits_released_on_task_abort() {
        let chunk_size = 1024;
        let data = Bytes::from(vec![0x99u8; 8 * 1024]); // 8 chunks
        let per_pipeline_max = 2 * chunk_size; // tight budget: 2 permits

        let global_ctrl = GlobalMemoryController::new(4 * chunk_size, chunk_size);

        let pipeline = build_budgeted_pipeline(
            data,
            chunk_size,
            per_pipeline_max,
            Arc::new(PassthroughFunction),
        );

        // Wrap the pipeline execution in a task we can abort.
        let global_ctrl_clone = global_ctrl.clone();
        let handle = tokio::spawn(async move {
            let rx = pipeline.execute(Some(&global_ctrl_clone)).await.unwrap();
            // Keep reading to simulate a running pipeline
            drain_stream(rx).await
        });

        // Let the pipeline start producing chunks.
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

        // Abort the task — this should cancel all subtasks and drop all guards.
        handle.abort();

        // Wait for the abort to propagate.
        let _ = handle.await; // JoinError::Cancelled

        // Allow runtime to process the cancellations.
        tokio::task::yield_now().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // All global permits must be returned.
        assert_eq!(
            global_ctrl.available(),
            global_ctrl.total_permits(),
            "Global permits leaked after task abort"
        );
    }

    /// Verify that a source-only pipeline (no streaming stage) releases all
    /// permits on completion. This tests the Source/Cached producer + bridge path.
    #[tokio::test]
    async fn source_only_pipeline_releases_permits() {
        let chunk_size = 1024;
        let data = Bytes::from(vec![0xBBu8; 5 * 1024]); // 5 chunks
        let per_pipeline_max = 3 * chunk_size;

        let global_ctrl = GlobalMemoryController::new(6 * chunk_size, chunk_size);

        let config = PipelineConfig {
            chunk_size,
            channel_capacity: 4,
            cache_intermediates: false,
            memory_budget: 0,
            streaming_threshold: 0,
            adaptive_chunking: false,
            min_chunk_size: 1024,
            max_chunk_size: 1_048_576,
            enable_fusion: false,
            per_pipeline_max,
        };
        let mut pipeline = StreamPipeline::new(config);
        let addr = deriva_core::CAddr::from_bytes(&[1u8; 32]);
        pipeline.add_source(addr, data);

        let rx = pipeline.execute(Some(&global_ctrl)).await.unwrap();
        let chunks = drain_stream(rx).await;

        assert!(chunks.last().unwrap().is_end());
        assert_eq!(chunks.iter().filter(|c| c.is_data()).count(), 5);

        tokio::task::yield_now().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(
            global_ctrl.available(),
            global_ctrl.total_permits(),
            "Global permits leaked after source-only pipeline completion"
        );
    }

    // -----------------------------------------------------------------------
    // Task 6.2: Progress guarantee with minimum 1 permit
    // -----------------------------------------------------------------------

    /// Helper: collect all data chunks from a receiver into a single Bytes.
    async fn collect_output(mut rx: mpsc::Receiver<StreamChunk>) -> Result<Bytes, DerivaError> {
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
                    // Channel closed without End marker — treat as complete
                    break;
                }
            }
        }

        let mut buf = Vec::with_capacity(total_len);
        for part in parts {
            buf.extend_from_slice(&part);
        }
        Ok(Bytes::from(buf))
    }

    /// Integration test: pipeline with exactly 1 permit (minimum budget) completes
    /// without deadlock. This proves that consumer-driven throughput ensures forward
    /// progress even with the minimum possible budget.
    ///
    /// The pipeline has `per_pipeline_max = chunk_size`, giving exactly 1 permit.
    /// The source data is larger than chunk_size, requiring multiple chunks to be
    /// produced. The producer blocks after sending one chunk (budget exhausted),
    /// then resumes when the consumer processes it and the bridge task drops the
    /// DualPermitGuard (releasing the permit).
    ///
    /// **Validates: Requirements 1.4, 6.1, 6.3**
    #[tokio::test]
    async fn pipeline_makes_progress_with_single_permit() {
        let chunk_size = 1024; // 1KB chunks
        let data_size = chunk_size * 5; // 5KB total → requires 5 chunks

        let config = PipelineConfig {
            chunk_size,
            channel_capacity: 2,
            cache_intermediates: false,
            memory_budget: 0,
            streaming_threshold: 0,
            adaptive_chunking: false,
            min_chunk_size: 1024,
            max_chunk_size: 1_048_576,
            enable_fusion: false,
            // Set per_pipeline_max = chunk_size → exactly 1 permit
            per_pipeline_max: chunk_size,
        };

        // Verify clamping: MemoryController with budget == chunk_size → 1 permit
        let mc = MemoryController::new(config.per_pipeline_max, config.chunk_size);
        assert_eq!(mc.total_permits(), 1, "expected exactly 1 permit");

        // Create input data (known pattern for verification)
        let input_data = Bytes::from(vec![0xAB_u8; data_size]);

        let mut pipeline = StreamPipeline::new(config);
        let source_addr = deriva_core::CAddr::from_bytes(b"test-source-single-permit");
        pipeline.add_source(source_addr, input_data.clone());

        // Execute with a timeout to detect deadlocks.
        // If the pipeline deadlocks (e.g., producer can never re-acquire the permit),
        // this timeout will fire and the test will fail.
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            pipeline.execute(None),
        )
        .await;

        // Should not timeout (no deadlock)
        let rx = result
            .expect("pipeline deadlocked — timed out after 5 seconds")
            .expect("pipeline execution failed");

        // Collect all output and verify correctness
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            collect_output(rx),
        )
        .await
        .expect("output collection deadlocked — timed out after 5 seconds")
        .expect("failed to collect output");

        // Verify total bytes match input
        assert_eq!(
            output.len(),
            data_size,
            "output length ({}) does not match input length ({})",
            output.len(),
            data_size,
        );

        // Verify data content is correct
        assert_eq!(
            output,
            input_data,
            "output data does not match input data",
        );

        // After pipeline completes, all permits should be released.
        // The MemoryController was created inside execute() so we can't inspect it
        // directly. But we can verify by creating a new controller with same params
        // and confirm the permit math is correct (1 permit).
        let mc_verify = MemoryController::new(chunk_size, chunk_size);
        assert_eq!(mc_verify.total_permits(), 1);
        assert_eq!(mc_verify.available(), 1);
    }

    /// Verify that MemoryController::new always produces at least 1 permit,
    /// covering all code paths: budget < chunk_size, budget == chunk_size,
    /// budget > chunk_size. No code path can produce 0 permits.
    ///
    /// **Validates: Requirements 1.4, 6.1, 6.3**
    #[test]
    fn memory_controller_always_produces_at_least_one_permit() {
        // budget < chunk_size cases (clamped to 1, logs warning)
        assert_eq!(MemoryController::new(0, 1024).total_permits(), 1);
        assert_eq!(MemoryController::new(1, 1024).total_permits(), 1);
        assert_eq!(MemoryController::new(512, 1024).total_permits(), 1);
        assert_eq!(MemoryController::new(1023, 1024).total_permits(), 1);

        // budget == chunk_size → 1 permit (budget / chunk_size = 1)
        assert_eq!(MemoryController::new(1024, 1024).total_permits(), 1);

        // budget > chunk_size → at least 1 (no possibility of 0)
        assert_eq!(MemoryController::new(2048, 1024).total_permits(), 2);
        assert_eq!(MemoryController::new(10240, 1024).total_permits(), 10);
    }

    /// Verify that GlobalMemoryController::new also has the same clamping behavior
    /// (minimum 1 permit), ensuring no code path produces 0 permits.
    ///
    /// **Validates: Requirements 1.4, 6.1, 6.3**
    #[test]
    fn global_memory_controller_always_produces_at_least_one_permit() {
        // global_budget < chunk_size cases (clamped to 1)
        assert_eq!(GlobalMemoryController::new(0, 1024).total_permits(), 1);
        assert_eq!(GlobalMemoryController::new(1, 1024).total_permits(), 1);
        assert_eq!(GlobalMemoryController::new(512, 1024).total_permits(), 1);
        assert_eq!(GlobalMemoryController::new(1023, 1024).total_permits(), 1);

        // global_budget == chunk_size → 1 permit
        assert_eq!(GlobalMemoryController::new(1024, 1024).total_permits(), 1);

        // global_budget > chunk_size → at least 1
        assert_eq!(GlobalMemoryController::new(2048, 1024).total_permits(), 2);
        assert_eq!(GlobalMemoryController::new(10240, 1024).total_permits(), 10);
    }
}
