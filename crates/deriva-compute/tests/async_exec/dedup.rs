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
// DEDUPLICATION TESTS
// ============================================================================

#[tokio::test]
async fn test_39_dedup_shared_input() {
    let (dag, _base_registry, cache, leaves) = setup();
    
    let count = Arc::new(AtomicUsize::new(0));
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    registry.register(Arc::new(CountingFunction::new(Arc::clone(&count))));
    let registry = Arc::new(registry);
    
    let leaf_addr = leaf(b"shared");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("shared"));
    
    let counting_recipe = Recipe {
        inputs: vec![leaf_addr],
        function_id: FunctionId { name: "counting".to_string(), version: "1".to_string() },
        params: BTreeMap::new(),
    };
    let counting_addr = counting_recipe.addr();
    dag.recipes.lock().unwrap().insert(counting_addr, counting_recipe);
    
    let mut consumer_addrs = vec![];
    for _ in 0..3 {
        let recipe = recipe(vec![counting_addr], "identity");
        let addr = recipe.addr();
        dag.recipes.lock().unwrap().insert(addr, recipe);
        consumer_addrs.push(addr);
    }
    
    let executor = Arc::new(AsyncExecutor::new(dag, registry, cache, leaves));
    
    for addr in consumer_addrs {
        executor.materialize(addr).await.unwrap();
    }
    
    // Counting function should execute only once (shared input)
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_40_dedup_concurrent_same_addr() {
    let (dag, _base_registry, cache, leaves) = setup();
    
    let count = Arc::new(AtomicUsize::new(0));
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    registry.register(Arc::new(CountingFunction::new(Arc::clone(&count))));
    let registry = Arc::new(registry);
    
    let leaf_addr = leaf(b"data");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));
    
    let recipe = Recipe {
        inputs: vec![leaf_addr],
        function_id: FunctionId { name: "counting".to_string(), version: "1".to_string() },
        params: BTreeMap::new(),
    };
    let recipe_addr = recipe.addr();
    dag.recipes.lock().unwrap().insert(recipe_addr, recipe);
    
    let executor = Arc::new(AsyncExecutor::new(dag, registry, cache, leaves));
    
    // 10 concurrent materializations of same address
    let mut handles = vec![];
    for _ in 0..10 {
        let exec = Arc::clone(&executor);
        handles.push(tokio::spawn(async move {
            exec.materialize(recipe_addr).await.unwrap()
        }));
    }
    
    for h in handles {
        h.await.unwrap();
    }
    
    // Should execute only once due to deduplication
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

