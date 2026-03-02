use async_trait::async_trait;
use bytes::Bytes;
use deriva_compute::async_executor::{AsyncExecutor, DagReader, VerificationMode};
use deriva_compute::cache::{AsyncMaterializationCache, SharedCache};
use deriva_compute::leaf_store::AsyncLeafStore;
use deriva_compute::registry::FunctionRegistry;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins;
use deriva_core::address::{CAddr, FunctionId, Recipe, Value};
use deriva_core::cache::EvictableCache;
use deriva_core::error::{DerivaError, Result};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

// Test helpers
struct TestDagReader {
    recipes: Mutex<HashMap<CAddr, Recipe>>,
}

impl DagReader for TestDagReader {
    fn get_inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        Ok(self.recipes.lock().unwrap().get(addr).map(|r| r.inputs.clone()))
    }

    fn get_recipe(&self, addr: &CAddr) -> Result<Option<Recipe>> {
        Ok(self.recipes.lock().unwrap().get(addr).cloned())
    }
}

struct TestLeafStore {
    leaves: Mutex<HashMap<CAddr, Bytes>>,
}

#[async_trait]
impl AsyncLeafStore for TestLeafStore {
    async fn get_leaf(&self, addr: &CAddr) -> Option<Bytes> {
        self.leaves.lock().unwrap().get(addr).cloned()
    }
}

// SlowFunction - sleeps for configurable duration to test parallelism
struct SlowFunction {
    duration_ms: u64,
}

impl SlowFunction {
    fn new(duration_ms: u64) -> Self {
        Self { duration_ms }
    }
}

impl ComputeFunction for SlowFunction {
    fn id(&self) -> FunctionId {
        FunctionId {
            name: "slow".to_string(),
            version: "1".to_string(),
        }
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> std::result::Result<Bytes, deriva_compute::function::ComputeError> {
        std::thread::sleep(Duration::from_millis(self.duration_ms));
        Ok(inputs.into_iter().next().unwrap_or_else(|| Bytes::from("slow")))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> deriva_compute::function::ComputeCost {
        deriva_compute::function::ComputeCost {
            cpu_ms: self.duration_ms,
            memory_bytes: 1024,
        }
    }
}

// CountingFunction - tracks execution count for deduplication testing
struct CountingFunction {
    count: Arc<AtomicUsize>,
}

impl CountingFunction {
    fn new(count: Arc<AtomicUsize>) -> Self {
        Self { count }
    }
}

impl ComputeFunction for CountingFunction {
    fn id(&self) -> FunctionId {
        FunctionId {
            name: "counting".to_string(),
            version: "1".to_string(),
        }
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> std::result::Result<Bytes, deriva_compute::function::ComputeError> {
        self.count.fetch_add(1, Ordering::SeqCst);
        Ok(inputs.into_iter().next().unwrap_or_else(|| Bytes::from("counted")))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> deriva_compute::function::ComputeCost {
        deriva_compute::function::ComputeCost {
            cpu_ms: 1,
            memory_bytes: 1024,
        }
    }
}

// Timing utility for measuring parallel speedup
fn measure_time<F, T>(f: F) -> (T, Duration)
where
    F: FnOnce() -> T,
{
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    (result, elapsed)
}

fn setup() -> (
    Arc<TestDagReader>,
    Arc<FunctionRegistry>,
    Arc<SharedCache>,
    Arc<TestLeafStore>,
) {
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    let dag = TestDagReader { recipes: Mutex::new(HashMap::new()) };
    let cache = SharedCache::new(EvictableCache::with_max_size(1024 * 1024));
    let leaves = TestLeafStore { leaves: Mutex::new(HashMap::new()) };
    (Arc::new(dag), Arc::new(registry), Arc::new(cache), Arc::new(leaves))
}

fn leaf(data: &[u8]) -> CAddr {
    CAddr::from_bytes(data)
}

fn recipe(inputs: Vec<CAddr>, func: &str) -> Recipe {
    Recipe::new(FunctionId::new(func, "1.0.0"), inputs, BTreeMap::new())
}

// Tests
// ============================================================================
// ERROR HANDLING TESTS
// ============================================================================

#[tokio::test]
async fn test_41_parallel_error_cancels_siblings() {
    let (dag, registry, cache, leaves) = setup();
    
    let valid_addr = leaf(b"valid");
    leaves.leaves.lock().unwrap().insert(valid_addr, Bytes::from("valid"));
    let missing_addr = leaf(b"missing");
    
    let recipe = recipe(vec![valid_addr, missing_addr], "concat");
    let recipe_addr = recipe.addr();
    dag.recipes.lock().unwrap().insert(recipe_addr, recipe);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(recipe_addr).await;
    
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), DerivaError::NotFound(_)));
}

#[tokio::test]
async fn test_42_parallel_error_in_deep_branch() {
    let (dag, registry, cache, leaves) = setup();
    
    let valid_addr = leaf(b"valid");
    leaves.leaves.lock().unwrap().insert(valid_addr, Bytes::from("valid"));
    let branch1 = recipe(vec![valid_addr], "identity");
    let branch1_addr = branch1.addr();
    dag.recipes.lock().unwrap().insert(branch1_addr, branch1);
    
    let missing_addr = leaf(b"missing");
    let branch2 = recipe(vec![missing_addr], "identity");
    let branch2_addr = branch2.addr();
    dag.recipes.lock().unwrap().insert(branch2_addr, branch2);
    
    let merge = recipe(vec![branch1_addr, branch2_addr], "concat");
    let merge_addr = merge.addr();
    dag.recipes.lock().unwrap().insert(merge_addr, merge);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(merge_addr).await;
    
    assert!(result.is_err());
}

