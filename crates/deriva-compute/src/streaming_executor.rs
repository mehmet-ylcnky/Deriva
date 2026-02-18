use std::collections::HashMap;
use tokio::sync::mpsc;
use deriva_core::{CAddr, DerivaError, DagStore};
use deriva_core::streaming::StreamChunk;
use crate::cache::AsyncMaterializationCache;
use crate::leaf_store::AsyncLeafStore;
use crate::pipeline::{StreamPipeline, PipelineConfig};
use crate::registry::FunctionRegistry;

/// Extended executor with streaming support.
///
/// Batch functions are automatically wrapped as single-chunk streams.
pub struct StreamingExecutor {
    pub config: PipelineConfig,
}

impl StreamingExecutor {
    pub fn new(config: PipelineConfig) -> Self {
        Self { config }
    }

    /// Build and execute a streaming pipeline for the given recipe DAG.
    ///
    /// Returns a receiver that yields output chunks. The caller is
    /// responsible for collecting and caching the result.
    pub async fn materialize_streaming(
        &self,
        addr: &CAddr,
        dag: &DagStore,
        cache: &dyn AsyncMaterializationCache,
        leaf_store: &dyn AsyncLeafStore,
        registry: &FunctionRegistry,
    ) -> Result<mpsc::Receiver<StreamChunk>, DerivaError> {
        let topo_order = dag.resolve_order(addr);

        let mut pipeline = StreamPipeline::new(self.config.clone());
        let mut addr_to_idx: HashMap<CAddr, usize> = HashMap::new();

        for topo_addr in &topo_order {
            // Check cache first
            if let Some(cached_data) = cache.get(topo_addr).await {
                let idx = pipeline.add_cached(*topo_addr, cached_data);
                addr_to_idx.insert(*topo_addr, idx);
                continue;
            }

            let recipe = dag.get_recipe(topo_addr)
                .ok_or_else(|| DerivaError::NotFound(format!("{}", topo_addr)))?;

            // Ensure all inputs are in the pipeline (leaves aren't in topo_order)
            for input_addr in &recipe.inputs {
                if addr_to_idx.contains_key(input_addr) {
                    continue;
                }
                if let Some(cached) = cache.get(input_addr).await {
                    let idx = pipeline.add_cached(*input_addr, cached);
                    addr_to_idx.insert(*input_addr, idx);
                } else if let Some(leaf) = leaf_store.get_leaf(input_addr).await {
                    let idx = pipeline.add_source(*input_addr, leaf);
                    addr_to_idx.insert(*input_addr, idx);
                } else {
                    return Err(DerivaError::NotFound(format!("{}", input_addr)));
                }
            }

            let input_indices: Vec<usize> = recipe.inputs.iter()
                .map(|a| *addr_to_idx.get(a).expect("input resolved above"))
                .collect();

            let func_id = &recipe.function_id;

            if let Some(streaming_fn) = registry.get_streaming(func_id) {
                let params = recipe.params.iter()
                    .map(|(k, v)| (k.clone(), format!("{}", v)))
                    .collect();
                let idx = pipeline.add_streaming_stage(
                    *topo_addr, streaming_fn, params, input_indices,
                );
                addr_to_idx.insert(*topo_addr, idx);
            } else if let Some(batch_fn) = registry.get(func_id) {
                let idx = pipeline.add_batch_stage(
                    *topo_addr, batch_fn, recipe.params.clone(), input_indices,
                );
                addr_to_idx.insert(*topo_addr, idx);
            } else {
                return Err(DerivaError::FunctionNotFound(format!("{}", func_id)));
            }
        }

        pipeline.execute().await
    }
}
