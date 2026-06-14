use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;
use deriva_core::{CAddr, DerivaError};
use deriva_core::streaming::StreamChunk;
use crate::async_executor::DagReader;
use crate::cache::AsyncMaterializationCache;
use crate::chunk_cache::{ChunkBlobStore, ChunkCacheWriter};
use crate::leaf_store::AsyncLeafStore;
use crate::memory_budget::GlobalMemoryController;
use crate::pipeline::{StreamPipeline, PipelineConfig};
use crate::registry::FunctionRegistry;

/// Extended executor with streaming support.
///
/// Batch functions are automatically wrapped as single-chunk streams.
pub struct StreamingExecutor {
    pub config: PipelineConfig,
}

/// Determines which caching strategy to use for a pipeline output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePathSelection {
    /// Use monolithic cache-after-collect (small known values).
    Monolithic,
    /// Use ChunkCacheWriter for chunk-level caching (large or unknown size).
    ChunkLevel,
}

impl StreamingExecutor {
    pub fn new(config: PipelineConfig) -> Self {
        crate::metrics::STREAMING_THRESHOLD_GAUGE.set(config.streaming_threshold as i64);
        Self { config }
    }

    /// Select the caching path based on output size and the configured threshold.
    ///
    /// - Unknown size → ChunkLevel (safe default for streaming)
    /// - Known size < chunk_cache_threshold → Monolithic
    /// - Known size >= chunk_cache_threshold → ChunkLevel
    pub fn select_cache_path(&self, output_size: Option<usize>) -> CachePathSelection {
        match output_size {
            None => CachePathSelection::ChunkLevel,
            Some(s) if s < self.config.chunk_cache_threshold => CachePathSelection::Monolithic,
            Some(_) => CachePathSelection::ChunkLevel,
        }
    }

    /// Topological sort using DagReader (works with any backend).
    fn resolve_order(dag: &dyn DagReader, addr: &CAddr) -> Result<Vec<CAddr>, DerivaError> {
        let mut order = Vec::new();
        let mut visited = HashSet::new();
        Self::topo_visit(dag, addr, &mut visited, &mut order)?;
        Ok(order)
    }

    fn topo_visit(
        dag: &dyn DagReader,
        addr: &CAddr,
        visited: &mut HashSet<CAddr>,
        order: &mut Vec<CAddr>,
    ) -> Result<(), DerivaError> {
        if !visited.insert(*addr) {
            return Ok(());
        }
        if let Some(recipe) = dag.get_recipe(addr)
            .map_err(|e| DerivaError::Storage(e.to_string()))?
        {
            for input in &recipe.inputs {
                Self::topo_visit(dag, input, visited, order)?;
            }
            order.push(*addr);
        }
        Ok(())
    }

    /// Build and execute a streaming pipeline for the given recipe DAG.
    ///
    /// When a `blob_store` is provided and the output's cache path is `ChunkLevel`,
    /// the returned receiver is wrapped with `ChunkCacheWriter` so chunks are
    /// incrementally persisted as they flow through. The returned `CachePathSelection`
    /// tells the caller whether monolithic caching is still needed.
    pub async fn materialize_streaming(
        &self,
        addr: &CAddr,
        dag: &dyn DagReader,
        cache: &dyn AsyncMaterializationCache,
        leaf_store: &dyn AsyncLeafStore,
        registry: &FunctionRegistry,
        global_controller: Option<&GlobalMemoryController>,
        blob_store: Option<&Arc<dyn ChunkBlobStore>>,
    ) -> Result<(mpsc::Receiver<StreamChunk>, CachePathSelection), DerivaError> {
        // Fast path: if the target address has a chunk manifest, stream from cache directly
        if let Some(stream_rx) = cache.get_stream(addr, self.config.channel_capacity).await {
            return Ok((stream_rx, CachePathSelection::Monolithic));
        }

        let topo_order = Self::resolve_order(dag, addr)?;

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
                .map_err(|e| DerivaError::Storage(e.to_string()))?
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

            // §2.9 Size-aware mode selection
            let input_size = pipeline.total_input_size(&input_indices);
            let prefer_streaming = match input_size {
                None => true,        // unknown → streaming (safe default)
                Some(0) => true,     // empty → streaming
                Some(s) => s >= self.config.streaming_threshold,
            };

            let has_streaming = registry.has_streaming(func_id);
            let has_batch = registry.get(func_id).is_some();

            let idx = if prefer_streaming {
                if has_streaming {
                    crate::metrics::MODE_SELECTION.with_label_values(&["streaming", "above_threshold"]).inc();
                    let params = recipe.params.iter()
                        .map(|(k, v)| (k.clone(), format!("{}", v)))
                        .collect();
                    pipeline.add_streaming_stage(
                        *topo_addr, registry.get_streaming(func_id).unwrap(), params, input_indices,
                    )
                } else if has_batch {
                    crate::metrics::MODE_SELECTION.with_label_values(&["batch", "no_streaming_impl"]).inc();
                    pipeline.add_batch_stage(
                        *topo_addr, registry.get(func_id).unwrap(), recipe.params.clone(), input_indices,
                    )
                } else {
                    return Err(DerivaError::FunctionNotFound(format!("{}", func_id)));
                }
            } else {
                // prefer batch for small inputs
                if has_batch {
                    crate::metrics::MODE_SELECTION.with_label_values(&["batch", "below_threshold"]).inc();
                    pipeline.add_batch_stage(
                        *topo_addr, registry.get(func_id).unwrap(), recipe.params.clone(), input_indices,
                    )
                } else if has_streaming {
                    crate::metrics::MODE_SELECTION.with_label_values(&["streaming", "no_batch_impl"]).inc();
                    let params = recipe.params.iter()
                        .map(|(k, v)| (k.clone(), format!("{}", v)))
                        .collect();
                    pipeline.add_streaming_stage(
                        *topo_addr, registry.get_streaming(func_id).unwrap(), params, input_indices,
                    )
                } else {
                    return Err(DerivaError::FunctionNotFound(format!("{}", func_id)));
                }
            };
            addr_to_idx.insert(*topo_addr, idx);
        }

        // Determine cache path for the final output before consuming the pipeline.
        // node_data_size returns Some only for Source/Cached nodes; for computed
        // (function stage) nodes it returns None → defaults to ChunkLevel.
        let final_node_size = addr_to_idx.get(addr)
            .and_then(|&idx| pipeline.node_data_size(idx));
        let cache_path = self.select_cache_path(final_node_size);

        let rx = pipeline.execute(global_controller).await?;

        // If chunk-level caching is selected and a blob store is available,
        // wrap the output receiver with ChunkCacheWriter for incremental caching.
        let rx = if cache_path == CachePathSelection::ChunkLevel {
            if let Some(bs) = blob_store {
                let writer = ChunkCacheWriter::new(
                    *addr,
                    self.config.chunk_size as u32,
                    Arc::clone(bs),
                );
                writer.wrap(rx, self.config.channel_capacity)
            } else {
                rx
            }
        } else {
            rx
        };

        Ok((rx, cache_path))
    }
}
