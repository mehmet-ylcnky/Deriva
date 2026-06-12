use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;

use deriva_core::CAddr;
use deriva_core::streaming::StreamChunk;

use crate::pipeline::PipelineNode;
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


/// Statistics from a fusion pass.
#[derive(Debug, Clone, Default)]
pub struct FusionStats {
    /// Original number of pipeline nodes before fusion.
    pub original_count: usize,
    /// Number of nodes after fusion.
    pub fused_count: usize,
    /// Number of stages eliminated (original_count - fused_count).
    pub stages_eliminated: usize,
    /// Number of FusedMapStage groups created.
    pub fused_groups: usize,
}

/// Fuse adjacent fusible streaming stages into FusedMapStage nodes.
///
/// Returns a new node list with fusible chains merged. Non-fusible nodes
/// pass through unchanged. Input indices are renumbered.
///
/// Time complexity: O(N) where N = number of pipeline nodes.
pub(crate) fn fuse_pipeline(nodes: Vec<PipelineNode>) -> (Vec<PipelineNode>, FusionStats) {
    let original_count = nodes.len();

    // Short-circuit: nothing to fuse with fewer than 2 nodes.
    if original_count < 2 {
        return (
            nodes,
            FusionStats {
                original_count,
                fused_count: original_count,
                stages_eliminated: 0,
                fused_groups: 0,
            },
        );
    }

    // Step 1: Build fan-out map.
    // fan_out[i] = number of nodes that reference node i in their input_indices.
    let mut fan_out = vec![0usize; original_count];
    for node in &nodes {
        match node {
            PipelineNode::StreamingStage { input_indices, .. }
            | PipelineNode::BatchStage { input_indices, .. } => {
                for &idx in input_indices {
                    fan_out[idx] += 1;
                }
            }
            _ => {}
        }
    }

    // Step 2-5: Scan nodes left-to-right, collecting fusible runs.
    let mut result: Vec<PipelineNode> = Vec::with_capacity(original_count);
    // Maps old index → new index for renumbering.
    let mut index_map: Vec<usize> = vec![0; original_count];
    let mut fused_groups: usize = 0;

    // A run entry: (original_index, function, params)
    let mut current_run: Vec<(usize, Arc<dyn StreamingComputeFunction>, HashMap<String, String>)> =
        Vec::new();

    // We iterate using indexed access since we need to borrow nodes for inspection.
    let mut i = 0;
    while i < original_count {
        let is_fusible_candidate = match &nodes[i] {
            PipelineNode::StreamingStage {
                function,
                input_indices,
                ..
            } => {
                i > 0
                    && function.is_fusible()
                    && input_indices.len() == 1
                    && input_indices[0] == i - 1
                    && fan_out[i - 1] == 1
            }
            _ => false,
        };

        if is_fusible_candidate {
            // Extract function and params from the node for the run.
            match &nodes[i] {
                PipelineNode::StreamingStage {
                    function, params, ..
                } => {
                    current_run.push((i, Arc::clone(function), params.clone()));
                }
                _ => unreachable!(),
            }
        } else {
            // Flush current run first.
            if !current_run.is_empty() {
                flush_run_inline(
                    &current_run,
                    &mut result,
                    &mut index_map,
                    &mut fused_groups,
                    &nodes,
                );
                current_run.clear();
            }

            // Push the current (non-fusible) node.
            let new_idx = result.len();
            index_map[i] = new_idx;

            match &nodes[i] {
                PipelineNode::Source { _addr, data } => {
                    result.push(PipelineNode::Source {
                        _addr: *_addr,
                        data: data.clone(),
                    });
                }
                PipelineNode::Cached { _addr, data } => {
                    result.push(PipelineNode::Cached {
                        _addr: *_addr,
                        data: data.clone(),
                    });
                }
                PipelineNode::StreamingStage {
                    _addr,
                    function,
                    params,
                    input_indices,
                } => {
                    result.push(PipelineNode::StreamingStage {
                        _addr: *_addr,
                        function: Arc::clone(function),
                        params: params.clone(),
                        input_indices: input_indices.clone(),
                    });
                }
                PipelineNode::BatchStage {
                    _addr,
                    function,
                    params,
                    input_indices,
                } => {
                    result.push(PipelineNode::BatchStage {
                        _addr: *_addr,
                        function: Arc::clone(function),
                        params: params.clone(),
                        input_indices: input_indices.clone(),
                    });
                }
            }
        }

        i += 1;
    }

    // Step 5: Flush any remaining run after the loop.
    if !current_run.is_empty() {
        flush_run_inline(
            &current_run,
            &mut result,
            &mut index_map,
            &mut fused_groups,
            &nodes,
        );
        current_run.clear();
    }

    // Step 6: Renumber all input_indices using the index_map.
    for node in &mut result {
        match node {
            PipelineNode::StreamingStage { input_indices, .. }
            | PipelineNode::BatchStage { input_indices, .. } => {
                for idx in input_indices.iter_mut() {
                    *idx = index_map[*idx];
                }
            }
            _ => {}
        }
    }

    let fused_count = result.len();
    let stages_eliminated = original_count - fused_count;

    (
        result,
        FusionStats {
            original_count,
            fused_count,
            stages_eliminated,
            fused_groups,
        },
    )
}

/// Helper to flush a fusible run into the result vector.
///
/// If the run has ≥ 2 entries, creates a FusedMapStage. If only 1 entry, pushes as-is.
fn flush_run_inline(
    current_run: &[(usize, Arc<dyn StreamingComputeFunction>, HashMap<String, String>)],
    result: &mut Vec<PipelineNode>,
    index_map: &mut [usize],
    fused_groups: &mut usize,
    nodes: &[PipelineNode],
) {
    if current_run.is_empty() {
        return;
    }

    if current_run.len() >= 2 {
        // Create FusedMapStage.
        let first_in_run = current_run[0].0;
        // The predecessor of the run is first_in_run - 1 (guaranteed by
        // adjacency condition: input_indices[0] == i - 1 for the first element).
        let run_predecessor_idx = first_in_run - 1;

        let stages: Vec<(Arc<dyn StreamingComputeFunction>, HashMap<String, String>)> =
            current_run
                .iter()
                .map(|(_, f, p)| (Arc::clone(f), p.clone()))
                .collect();

        let fused = FusedMapStage::new(stages);
        let new_idx = result.len();

        // Map all run entries to this new index.
        for (orig_idx, _, _) in current_run {
            index_map[*orig_idx] = new_idx;
        }

        result.push(PipelineNode::StreamingStage {
            _addr: CAddr::from_raw([0u8; 32]),
            function: Arc::new(fused),
            params: HashMap::new(),
            // Store the original predecessor index; renumbering happens in final pass.
            input_indices: vec![run_predecessor_idx],
        });

        *fused_groups += 1;
    } else {
        // Single entry — push as-is (no fusion for singletons).
        let (orig_idx, function, params) = &current_run[0];
        let new_idx = result.len();
        index_map[*orig_idx] = new_idx;

        let input_indices = match &nodes[*orig_idx] {
            PipelineNode::StreamingStage { input_indices, .. } => input_indices.clone(),
            _ => unreachable!(),
        };

        result.push(PipelineNode::StreamingStage {
            _addr: CAddr::from_raw([0u8; 32]),
            function: Arc::clone(function),
            params: params.clone(),
            input_indices,
        });
    }
}
