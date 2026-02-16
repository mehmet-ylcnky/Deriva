//! # Async Executor with Parallel Materialization
//!
//! This module implements concurrent materialization of computation-addressed values
//! with three key features:
//!
//! ## 1. Parallel Input Resolution
//! Independent recipe inputs are materialized concurrently using `try_join_all`,
//! enabling significant speedup for fan-in patterns (e.g., 4 inputs with 100ms each
//! complete in ~100ms instead of 400ms sequentially).
//!
//! ## 2. Deduplication via Broadcast Channels
//! When multiple tasks request the same address concurrently, only one computation
//! executes while others subscribe to a broadcast channel for the result. This is
//! implemented using an `in_flight` map that tracks ongoing computations.
//!
//! **Mechanism:**
//! - First task to request an address becomes the "producer" and creates a broadcast channel
//! - Subsequent tasks become "subscribers" and wait for the result
//! - Producer broadcasts result (success or error) to all subscribers
//! - Channel is removed from `in_flight` map after completion
//!
//! ## 3. Semaphore-Based Concurrency Control
//! A semaphore limits concurrent compute operations to prevent resource exhaustion.
//! **Critical for deadlock prevention:** The semaphore is acquired ONLY at the compute
//! step, NOT during input resolution. This ensures tasks waiting for inputs don't hold
//! semaphore permits, preventing deadlock in deep DAGs.
//!
//! **Deadlock Prevention Strategy:**
//! ```text
//! ❌ WRONG (deadlock risk):
//!    acquire_semaphore() → resolve_inputs() → compute()
//!    (holds permit while waiting for inputs)
//!
//! ✅ CORRECT (deadlock-free):
//!    resolve_inputs() → acquire_semaphore() → compute()
//!    (only holds permit during actual computation)
//! ```

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
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Verification mode for determinism checking
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerificationMode {
    /// No verification — single compute (default, production)
    Off,
    /// Dual compute — execute twice, compare byte-for-byte
    DualCompute,
    /// Statistical — verify N% of materializations (sampling)
    Sampled { rate: f64 },
}

impl Default for VerificationMode {
    fn default() -> Self {
        Self::Off
    }
}

/// Statistics for verification operations
pub struct VerificationStats {
    pub total_verified: AtomicU64,
    pub total_passed: AtomicU64,
    pub total_failed: AtomicU64,
    pub last_failure: tokio::sync::Mutex<Option<DerivaError>>,
}

impl VerificationStats {
    pub fn new() -> Self {
        Self {
            total_verified: AtomicU64::new(0),
            total_passed: AtomicU64::new(0),
            total_failed: AtomicU64::new(0),
            last_failure: tokio::sync::Mutex::new(None),
        }
    }

    pub fn record_pass(&self) {
        self.total_verified.fetch_add(1, Ordering::Relaxed);
        self.total_passed.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn record_fail(&self, error: DerivaError) {
        self.total_verified.fetch_add(1, Ordering::Relaxed);
        self.total_failed.fetch_add(1, Ordering::Relaxed);
        *self.last_failure.lock().await = Some(error);
    }

    pub fn failure_rate(&self) -> f64 {
        let verified = self.total_verified.load(Ordering::Relaxed);
        if verified == 0 {
            return 0.0;
        }
        let failed = self.total_failed.load(Ordering::Relaxed);
        failed as f64 / verified as f64
    }
}

impl Default for VerificationStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for AsyncExecutor
pub struct ExecutorConfig {
    /// Max concurrent materialization tasks (default: num_cpus * 2)
    pub max_concurrency: usize,
    /// Broadcast channel capacity for dedup (default: 16)
    pub dedup_channel_capacity: usize,
    /// Verification mode for determinism checking (default: Off)
    pub verification: VerificationMode,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_concurrency: num_cpus::get() * 2,
            dedup_channel_capacity: 16,
            verification: VerificationMode::Off,
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
    verification_stats: Arc<VerificationStats>,
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
                verification: self.config.verification,
            },
            verification_stats: Arc::clone(&self.verification_stats),
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
            verification_stats: Arc::new(VerificationStats::new()),
        }
    }

    /// Materialize a CAddr - resolves recursively through the DAG
    ///
    /// # Parallel Materialization Algorithm
    ///
    /// This method implements concurrent materialization with deduplication:
    ///
    /// 1. **Cache Check**: Return immediately if already computed
    /// 2. **Leaf Check**: Return immediately if it's a leaf value
    /// 3. **Deduplication Check**: If another task is computing this address,
    ///    subscribe to its broadcast channel and wait for the result
    /// 4. **Register as Producer**: Create broadcast channel and register in `in_flight` map
    /// 5. **Recipe Lookup**: Get the recipe from DAG (broadcast errors on failure)
    /// 6. **Parallel Input Resolution**: Use `try_join_all` to materialize all inputs
    ///    concurrently. This is the key parallelism point - independent branches
    ///    execute simultaneously.
    /// 7. **Acquire Semaphore**: CRITICAL - acquire permit ONLY before compute step,
    ///    not during input resolution. This prevents deadlock in deep DAGs.
    /// 8. **Execute Compute**: Run the function in blocking thread pool
    /// 9. **Broadcast Result**: Send result (success or error) to all waiting subscribers
    /// 10. **Cleanup**: Remove from `in_flight` map
    /// 11. **Cache on Success**: Store result in cache for future requests
    ///
    /// # Deadlock Prevention
    ///
    /// The semaphore is acquired at step 7 (after input resolution) rather than
    /// at the beginning. This ensures tasks waiting for inputs don't hold permits,
    /// preventing circular wait conditions in deep DAGs.
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

            // 6. Parallel input resolution - CRITICAL: try_join_all enables concurrent execution
            // All independent inputs materialize simultaneously, providing significant speedup
            // for fan-in patterns (e.g., 4 inputs complete in ~100ms instead of 400ms)
            let futures: Vec<_> = recipe.inputs.iter()
                .map(|input_addr| self.materialize(*input_addr))
                .collect();
            let input_bytes = match futures::future::try_join_all(futures).await {
                Ok(bytes) => bytes,
                Err(e) => {
                    // Broadcast error to waiting subscribers before cleanup
                    let err = Err(e);
                    let _ = tx.send(err.clone());
                    let mut in_flight = self.in_flight.lock().await;
                    in_flight.remove(&addr);
                    return err;
                }
            };

            // 7. Acquire semaphore ONLY for compute step - DEADLOCK PREVENTION
            // By acquiring the permit AFTER input resolution, we ensure tasks waiting
            // for inputs don't hold permits. This prevents circular wait in deep DAGs.
            // Example: With limit=2 and 4 inputs, first 2 compute while others wait,
            // then next 2 compute - batched execution without deadlock.
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

            // 8. Execute compute function in blocking thread pool
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

            // 9. Broadcast result to all waiting subscribers
            // Deduplication payoff: All tasks waiting for this address receive the result
            // without recomputing. Ignore send errors (no subscribers is ok).
            let _ = tx.send(result.clone());

            // 10. Clean up in_flight entry - computation complete
            {
                let mut in_flight = self.in_flight.lock().await;
                in_flight.remove(&addr);
            }

            // 11. Cache on success for future requests
            if let Ok(ref output) = result {
                self.cache.put(addr, output.clone()).await;
            }

            result
        })
    }
}
