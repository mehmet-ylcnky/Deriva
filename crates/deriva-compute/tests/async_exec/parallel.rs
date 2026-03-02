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
// PARALLEL EXECUTION TESTS
// ============================================================================

#[tokio::test]
async fn test_30_parallel_fan_in_faster_than_sequential() {
    let (dag, _base_registry, cache, leaves) = setup();
    
    // Create local mutable registry with slow function
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    registry.register(Arc::new(SlowFunction::new(100)));
    let registry = Arc::new(registry);
    
    // Create 4 leaves
    let mut leaf_addrs = vec![];
    for i in 0..4 {
        let addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(addr, Bytes::from(vec![i]));
        leaf_addrs.push(addr);
    }
    
    // Create recipe with 4 slow inputs
    let mut slow_addrs = vec![];
    for addr in &leaf_addrs {
        let recipe = Recipe {
            inputs: vec![*addr],
            function_id: FunctionId { name: "slow".to_string(), version: "1".to_string() },
            params: BTreeMap::new(),
        };
        let recipe_addr = recipe.addr();
        dag.recipes.lock().unwrap().insert(recipe_addr, recipe);
        slow_addrs.push(recipe_addr);
    }
    
    // Final merge
    let final_recipe = recipe(slow_addrs, "concat");
    let final_addr = final_recipe.addr();
    dag.recipes.lock().unwrap().insert(final_addr, final_recipe);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    
    let start = Instant::now();
    executor.materialize(final_addr).await.unwrap();
    let elapsed = start.elapsed();
    
    // Parallel: ~100ms, Sequential would be ~400ms
    assert!(elapsed.as_millis() < 300, "Expected <300ms, got {}ms", elapsed.as_millis());
}

#[tokio::test]
async fn test_31_parallel_linear_chain_no_speedup() {
    let (dag, _base_registry, cache, leaves) = setup();
    
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    registry.register(Arc::new(SlowFunction::new(100)));
    let registry = Arc::new(registry);
    
    let leaf_addr = leaf(b"start");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("start"));
    
    let mut prev = leaf_addr;
    for _ in 0..3 {
        let recipe = Recipe {
            inputs: vec![prev],
            function_id: FunctionId { name: "slow".to_string(), version: "1".to_string() },
            params: BTreeMap::new(),
        };
        let addr = recipe.addr();
        dag.recipes.lock().unwrap().insert(addr, recipe);
        prev = addr;
    }
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    
    let start = Instant::now();
    executor.materialize(prev).await.unwrap();
    let elapsed = start.elapsed();
    
    // Linear chain: no parallelism, ~300ms
    assert!(elapsed.as_millis() >= 250, "Expected >=250ms, got {}ms", elapsed.as_millis());
}

#[tokio::test]
async fn test_32_parallel_diamond_dag() {
    let (dag, _base_registry, cache, leaves) = setup();
    
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    registry.register(Arc::new(SlowFunction::new(100)));
    let registry = Arc::new(registry);
    
    let leaf_addr = leaf(b"root");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("root"));
    
    let mut branch_addrs = vec![];
    for _ in 0..2 {
        let recipe = Recipe {
            inputs: vec![leaf_addr],
            function_id: FunctionId { name: "slow".to_string(), version: "1".to_string() },
            params: BTreeMap::new(),
        };
        let addr = recipe.addr();
        dag.recipes.lock().unwrap().insert(addr, recipe);
        branch_addrs.push(addr);
    }
    
    let merge = recipe(branch_addrs, "concat");
    let merge_addr = merge.addr();
    dag.recipes.lock().unwrap().insert(merge_addr, merge);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    
    let start = Instant::now();
    executor.materialize(merge_addr).await.unwrap();
    let elapsed = start.elapsed();
    
    // Parallel: ~100ms (both branches run concurrently)
    assert!(elapsed.as_millis() < 200, "Expected <200ms, got {}ms", elapsed.as_millis());
}

#[tokio::test]
async fn test_33_parallel_wide_fan_in_16_inputs() {
    let (dag, _base_registry, cache, leaves) = setup();
    
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    registry.register(Arc::new(SlowFunction::new(50)));
    let registry = Arc::new(registry);
    
    let mut slow_addrs = vec![];
    for i in 0..16 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from(vec![i]));
        
        let recipe = Recipe {
            inputs: vec![leaf_addr],
            function_id: FunctionId { name: "slow".to_string(), version: "1".to_string() },
            params: BTreeMap::new(),
        };
        let addr = recipe.addr();
        dag.recipes.lock().unwrap().insert(addr, recipe);
        slow_addrs.push(addr);
    }
    
    let final_recipe = recipe(slow_addrs, "concat");
    let final_addr = final_recipe.addr();
    dag.recipes.lock().unwrap().insert(final_addr, final_recipe);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    
    let start = Instant::now();
    executor.materialize(final_addr).await.unwrap();
    let elapsed = start.elapsed();
    
    // Parallel: ~50ms, Sequential would be ~800ms
    assert!(elapsed.as_millis() < 400, "Expected <400ms, got {}ms", elapsed.as_millis());
}

#[tokio::test]
async fn test_34_parallel_preserves_input_order() {
    let (dag, registry, cache, leaves) = setup();
    
    let mut leaf_addrs = vec![];
    for i in 0..4u8 {
        let addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(addr, Bytes::from(vec![i]));
        leaf_addrs.push(addr);
    }
    
    let concat_recipe = recipe(leaf_addrs, "concat");
    let concat_addr = concat_recipe.addr();
    dag.recipes.lock().unwrap().insert(concat_addr, concat_recipe);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(concat_addr).await.unwrap();
    
    // Verify order: [0, 1, 2, 3]
    assert_eq!(result, Bytes::from(vec![0, 1, 2, 3]));
}

#[tokio::test]
async fn test_35_parallel_single_input_no_overhead() {
    let (dag, registry, cache, leaves) = setup();
    
    let leaf_addr = leaf(b"single");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("single"));
    
    let recipe = recipe(vec![leaf_addr], "identity");
    let recipe_addr = recipe.addr();
    dag.recipes.lock().unwrap().insert(recipe_addr, recipe);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(recipe_addr).await.unwrap();
    
    assert_eq!(result, Bytes::from("single"));
}

#[tokio::test]
async fn test_36_parallel_zero_inputs_recipe() {
    let (dag, registry, cache, leaves) = setup();
    
    let recipe = recipe(vec![], "concat"); // concat works with 0 inputs
    let recipe_addr = recipe.addr();
    dag.recipes.lock().unwrap().insert(recipe_addr, recipe);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(recipe_addr).await.unwrap();
    
    // Concat with no inputs returns empty
    assert_eq!(result, Bytes::new());
}

