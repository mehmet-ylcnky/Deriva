use crate::cache::AsyncMaterializationCache;
use crate::leaf_store::AsyncLeafStore;
use crate::registry::FunctionRegistry;
use bytes::Bytes;
use deriva_core::address::{CAddr, Recipe};
use deriva_core::dag::DagStore;
use deriva_core::error::{DerivaError, Result};
use deriva_core::recipe_store::RecipeStore;
use deriva_core::PersistentDag;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, Semaphore};

/// Trait for DAG access - abstracts over DagStore and PersistentDag
pub trait DagReader: Send + Sync {
    fn get_inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>>;
    fn get_recipe(&self, addr: &CAddr) -> Result<Option<Recipe>>;
}

/// DagReader implementation for DagStore
impl DagReader for DagStore {
    fn get_inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        Ok(self.get_recipe(addr).map(|r| r.inputs.clone()))
    }

    fn get_recipe(&self, addr: &CAddr) -> Result<Option<Recipe>> {
        Ok(self.get_recipe(addr).cloned())
    }
}

/// Combines PersistentDag (for inputs/edges) with recipe store (for full recipes)
pub struct CombinedDagReader<R: RecipeStore> {
    pub dag: Arc<PersistentDag>,
    pub recipes: Arc<R>,
}

impl<R: RecipeStore> DagReader for CombinedDagReader<R> {
    fn get_inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        self.dag.inputs(addr)
    }

    fn get_recipe(&self, addr: &CAddr) -> Result<Option<Recipe>> {
        self.recipes.get(addr)
    }
}

/// Configuration for AsyncExecutor
pub struct ExecutorConfig {
    /// Max concurrent materialization tasks (default: num_cpus * 2)
    pub max_concurrency: usize,
    /// Broadcast channel capacity for dedup (default: 16)
    pub dedup_channel_capacity: usize,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_concurrency: num_cpus::get() * 2,
            dedup_channel_capacity: 16,
        }
    }
}

pub struct AsyncExecutor<C, L, D> {
    cache: Arc<C>,
    leaf_store: Arc<L>,
    dag: Arc<D>,
    registry: Arc<FunctionRegistry>,
    semaphore: Arc<Semaphore>,
    in_flight: Arc<Mutex<HashMap<CAddr, broadcast::Sender<Result<Bytes>>>>>,
    pub config: ExecutorConfig,
}

impl<C, L, D> Clone for AsyncExecutor<C, L, D> {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            leaf_store: Arc::clone(&self.leaf_store),
            dag: Arc::clone(&self.dag),
            registry: Arc::clone(&self.registry),
            semaphore: Arc::clone(&self.semaphore),
            in_flight: Arc::clone(&self.in_flight),
            config: ExecutorConfig {
                max_concurrency: self.config.max_concurrency,
                dedup_channel_capacity: self.config.dedup_channel_capacity,
            },
        }
    }
}

impl<C, L, D> AsyncExecutor<C, L, D>
where
    C: AsyncMaterializationCache + 'static,
    L: AsyncLeafStore + 'static,
    D: DagReader + 'static,
{
    pub fn new(
        dag: Arc<D>,
        registry: Arc<FunctionRegistry>,
        cache: Arc<C>,
        leaf_store: Arc<L>,
    ) -> Self {
        Self::with_config(dag, registry, cache, leaf_store, ExecutorConfig::default())
    }

    pub fn with_config(
        dag: Arc<D>,
        registry: Arc<FunctionRegistry>,
        cache: Arc<C>,
        leaf_store: Arc<L>,
        config: ExecutorConfig,
    ) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrency));
        Self {
            cache,
            leaf_store,
            dag,
            registry,
            semaphore,
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Materialize a CAddr - resolves recursively through the DAG
    pub fn materialize(&self, addr: CAddr) -> BoxFuture<'_, Result<Bytes>> {
        Box::pin(async move {
            // 1. Cache check
            if let Some(bytes) = self.cache.get(&addr).await {
                return Ok(bytes);
            }

            // 2. Leaf check
            if let Some(bytes) = self.leaf_store.get_leaf(&addr).await {
                return Ok(bytes);
            }

            // 3. Deduplication check - if already computing, subscribe to result
            {
                let in_flight = self.in_flight.lock().await;
                if let Some(tx) = in_flight.get(&addr) {
                    let mut rx = tx.subscribe();
                    drop(in_flight);
                    // Handle broadcast errors: RecvError means sender dropped (computation failed/cancelled)
                    return match rx.recv().await {
                        Ok(result) => result,
                        Err(_) => Err(DerivaError::ComputeFailed(
                            "producer task failed or was cancelled".into()
                        )),
                    };
                }
            }

            // 4. Register as producer
            let (tx, _rx) = broadcast::channel(self.config.dedup_channel_capacity);
            {
                let mut in_flight = self.in_flight.lock().await;
                in_flight.insert(addr, tx.clone());
            }

            // 5. Recipe lookup
            let recipe = match self.dag.get_recipe(&addr) {
                Ok(Some(r)) => r,
                Ok(None) => {
                    let err = Err(DerivaError::NotFound(addr.to_string()));
                    let _ = tx.send(err.clone());
                    let mut in_flight = self.in_flight.lock().await;
                    in_flight.remove(&addr);
                    return err;
                }
                Err(e) => {
                    let err = Err(e);
                    let _ = tx.send(err.clone());
                    let mut in_flight = self.in_flight.lock().await;
                    in_flight.remove(&addr);
                    return err;
                }
            };

            // 6. Parallel input resolution - try_join_all cancels all on first error
            let futures: Vec<_> = recipe.inputs.iter()
                .map(|input_addr| self.materialize(*input_addr))
                .collect();
            let input_bytes = match futures::future::try_join_all(futures).await {
                Ok(bytes) => bytes,
                Err(e) => {
                    // Broadcast error to waiting subscribers
                    let err = Err(e);
                    let _ = tx.send(err.clone());
                    let mut in_flight = self.in_flight.lock().await;
                    in_flight.remove(&addr);
                    return err;
                }
            };

            // 7. Acquire semaphore ONLY for compute step (prevent deadlock)
            let _permit = match self.semaphore.acquire().await {
                Ok(p) => p,
                Err(e) => {
                    let err = Err(DerivaError::ComputeFailed(format!("semaphore error: {}", e)));
                    let _ = tx.send(err.clone());
                    let mut in_flight = self.in_flight.lock().await;
                    in_flight.remove(&addr);
                    return err;
                }
            };

            // 8. Execute compute function
            let func = match self.registry.get(&recipe.function_id) {
                Some(f) => f,
                None => {
                    let err = Err(DerivaError::FunctionNotFound(recipe.function_id.to_string()));
                    let _ = tx.send(err.clone());
                    let mut in_flight = self.in_flight.lock().await;
                    in_flight.remove(&addr);
                    return err;
                }
            };

            let result = {
                let params = recipe.params.clone();
                tokio::task::spawn_blocking(move || {
                    func.execute(input_bytes, &params)
                })
                .await
                .map_err(|e| DerivaError::ComputeFailed(
                    format!("task join error: {}", e)
                ))
                .and_then(|r| r.map_err(|e| DerivaError::ComputeFailed(e.to_string())))
            };

            // 9. Broadcast result to waiting tasks (ignore send errors - no subscribers is ok)
            let _ = tx.send(result.clone());

            // 10. Clean up in_flight entry
            {
                let mut in_flight = self.in_flight.lock().await;
                in_flight.remove(&addr);
            }

            // 11. Cache on success
            if let Ok(ref output) = result {
                self.cache.put(addr, output.clone()).await;
            }

            result
        })
    }
}
