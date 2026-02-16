use async_trait::async_trait;
use bytes::Bytes;
use deriva_compute::async_executor::{AsyncExecutor, DagReader, ExecutorConfig, VerificationMode};
use deriva_compute::cache::{AsyncMaterializationCache, SharedCache};
use deriva_compute::function::{ComputeFunction, ComputeError, ComputeCost};
use deriva_compute::leaf_store::AsyncLeafStore;
use deriva_compute::registry::FunctionRegistry;
use deriva_core::address::{CAddr, FunctionId, Recipe, Value};
use deriva_core::cache::EvictableCache;
use deriva_core::error::DerivaError;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

// Test helper: Deterministic identity function that counts invocations
#[derive(Clone)]
struct CountingIdentity {
    count: Arc<AtomicU64>,
}

impl CountingIdentity {
    fn new() -> Self {
        Self {
            count: Arc::new(AtomicU64::new(0)),
        }
    }

    fn invocation_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }
}

impl ComputeFunction for CountingIdentity {
    fn id(&self) -> FunctionId {
        FunctionId::new("counting_identity", "v1")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> std::result::Result<Bytes, ComputeError> {
        self.count.fetch_add(1, Ordering::Relaxed);
        Ok(inputs.first().cloned().unwrap_or_else(|| Bytes::from("empty")))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 1, memory_bytes: 1024 }
    }
}

// Test helper: Non-deterministic function that returns current timestamp
#[derive(Clone)]
struct NonDeterministicFn;

impl ComputeFunction for NonDeterministicFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("nondeterministic", "v1")
    }

    fn execute(&self, _inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> std::result::Result<Bytes, ComputeError> {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Ok(Bytes::from(nanos.to_string()))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 1, memory_bytes: 1024 }
    }
}

// Mock DAG for testing
struct MockDag {
    recipes: Arc<Mutex<HashMap<CAddr, Recipe>>>,
}

impl MockDag {
    fn new() -> Self {
        Self {
            recipes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl DagReader for MockDag {
    fn get_inputs(&self, addr: &CAddr) -> deriva_core::error::Result<Option<Vec<CAddr>>> {
        Ok(self.recipes.lock().unwrap().get(addr).map(|r| r.inputs.clone()))
    }

    fn get_recipe(&self, addr: &CAddr) -> deriva_core::error::Result<Option<Recipe>> {
        Ok(self.recipes.lock().unwrap().get(addr).cloned())
    }
}

// Mock leaf store
struct MockLeafStore {
    pub leaves: Arc<Mutex<HashMap<CAddr, Bytes>>>,
}

impl MockLeafStore {
    fn new() -> Self {
        Self {
            leaves: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl AsyncLeafStore for MockLeafStore {
    async fn get_leaf(&self, addr: &CAddr) -> Option<Bytes> {
        self.leaves.lock().unwrap().get(addr).cloned()
    }
}

// Test setup helper
fn setup() -> (
    Arc<MockDag>,
    FunctionRegistry,
    Arc<SharedCache>,
    Arc<MockLeafStore>,
) {
    let dag = Arc::new(MockDag::new());
    let registry = FunctionRegistry::new();
    let cache = Arc::new(SharedCache::new(EvictableCache::new(Default::default())));
    let leaves = Arc::new(MockLeafStore::new());
    (dag, registry, cache, leaves)
}

// Helper to create a leaf CAddr
fn leaf(data: &[u8]) -> CAddr {
    CAddr::from_bytes(data)
}

// Helper to create a recipe
fn recipe(inputs: Vec<CAddr>, function_id: &str) -> Recipe {
    Recipe {
        function_id: FunctionId::new(function_id, "v1"),
        inputs,
        params: BTreeMap::new(),
    }
}

// Test 1: Off mode executes once
#[tokio::test]
async fn test_verification_off_single_execution() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::Off,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let leaf_addr = leaf(b"test");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

    let r = recipe(vec![leaf_addr], "counting_identity");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    executor.materialize(addr).await.unwrap();
    assert_eq!(func_clone.invocation_count(), 1);
}

// Test 2: DualCompute mode executes twice
#[tokio::test]
async fn test_verification_dual_executes_twice() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let leaf_addr = leaf(b"test");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

    let r = recipe(vec![leaf_addr], "counting_identity");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    executor.materialize(addr).await.unwrap();
    assert_eq!(func_clone.invocation_count(), 2);
}

// Test 3: DualCompute with deterministic function passes
#[tokio::test]
async fn test_verification_dual_deterministic_passes() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(CountingIdentity::new()));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let leaf_addr = leaf(b"test");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

    let r = recipe(vec![leaf_addr], "counting_identity");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    let result = executor.materialize(addr).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Bytes::from("data"));
}

// Test 4: DualCompute with non-deterministic function fails
#[tokio::test]
async fn test_verification_dual_nondeterministic_fails() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(NonDeterministicFn));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let r = recipe(vec![], "nondeterministic");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    let result = executor.materialize(addr).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        DerivaError::DeterminismViolation { .. } => {}
        e => panic!("Expected DeterminismViolation, got {:?}", e),
    }
}

// Test 5: Error includes detailed fields
#[tokio::test]
async fn test_verification_dual_error_includes_details() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(NonDeterministicFn));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let r = recipe(vec![], "nondeterministic");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    let result = executor.materialize(addr).await;
    match result.unwrap_err() {
        DerivaError::DeterminismViolation {
            addr: err_addr,
            function_id,
            output_1_hash,
            output_2_hash,
            output_1_len,
            output_2_len,
        } => {
            assert!(!err_addr.is_empty());
            assert_eq!(function_id, "nondeterministic/v1");
            assert!(!output_1_hash.is_empty());
            assert!(!output_2_hash.is_empty());
            assert_ne!(output_1_hash, output_2_hash);
            assert!(output_1_len > 0);
            assert!(output_2_len > 0);
        }
        e => panic!("Expected DeterminismViolation, got {:?}", e),
    }
}

// Test 6: Sampled mode respects rate
#[tokio::test]
async fn test_verification_sampled_respects_rate() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::Sampled { rate: 0.5 },
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let mut verified_count = 0;
    let mut not_verified_count = 0;

    for i in 0..100 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

        let r = recipe(vec![leaf_addr], "counting_identity");
        let addr = r.addr();
        dag.recipes.lock().unwrap().insert(addr, r);

        let before = func_clone.invocation_count();
        executor.materialize(addr).await.unwrap();
        let after = func_clone.invocation_count();

        if after - before == 2 {
            verified_count += 1;
        } else {
            not_verified_count += 1;
        }
    }

    // Should be roughly 50/50 (allow 30-70% range)
    assert!(verified_count >= 30 && verified_count <= 70);
    assert!(not_verified_count >= 30 && not_verified_count <= 70);
}

// Test 7: Sampled mode is deterministic per address
#[tokio::test]
async fn test_verification_sampled_deterministic_per_addr() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    let func_arc: Arc<dyn ComputeFunction> = Arc::new(func);
    registry.register(Arc::clone(&func_arc));

    let config = ExecutorConfig {
        verification: VerificationMode::Sampled { rate: 0.5 },
        ..Default::default()
    };

    let leaf_addr = leaf(b"test");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

    let r = recipe(vec![leaf_addr], "counting_identity");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    // First execution
    let mut registry1 = FunctionRegistry::new();
    registry1.register(Arc::clone(&func_arc));
    let config1 = ExecutorConfig {
        verification: VerificationMode::Sampled { rate: 0.5 },
        ..Default::default()
    };
    let executor1 = AsyncExecutor::with_config(dag.clone(), Arc::new(registry1), cache.clone(), leaves.clone(), config1);
    let before1 = func_clone.invocation_count();
    executor1.materialize(addr).await.unwrap();
    let after1 = func_clone.invocation_count();
    let verified1 = (after1 - before1) == 2;

    // Second execution with fresh executor and separate cache
    let cache2 = Arc::new(SharedCache::new(EvictableCache::new(Default::default())));
    let mut registry2 = FunctionRegistry::new();
    registry2.register(Arc::clone(&func_arc));
    let config2 = ExecutorConfig {
        verification: VerificationMode::Sampled { rate: 0.5 },
        ..Default::default()
    };
    let executor2 = AsyncExecutor::with_config(dag.clone(), Arc::new(registry2), cache2, leaves.clone(), config2);
    let before2 = func_clone.invocation_count();
    executor2.materialize(addr).await.unwrap();
    let after2 = func_clone.invocation_count();
    let verified2 = (after2 - before2) == 2;

    // Same address should have same verification decision
    assert_eq!(verified1, verified2);
}

// Test 8: Sampled rate=0.0 never verifies
#[tokio::test]
async fn test_verification_sampled_zero_rate() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::Sampled { rate: 0.0 },
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    for i in 0..20 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

        let r = recipe(vec![leaf_addr], "counting_identity");
        let addr = r.addr();
        dag.recipes.lock().unwrap().insert(addr, r);

        let before = func_clone.invocation_count();
        executor.materialize(addr).await.unwrap();
        let after = func_clone.invocation_count();

        assert_eq!(after - before, 1, "Should execute once (not verified)");
    }
}

// Test 9: Sampled rate=1.0 always verifies
#[tokio::test]
async fn test_verification_sampled_one_rate() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::Sampled { rate: 1.0 },
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    for i in 0..20 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

        let r = recipe(vec![leaf_addr], "counting_identity");
        let addr = r.addr();
        dag.recipes.lock().unwrap().insert(addr, r);

        let before = func_clone.invocation_count();
        executor.materialize(addr).await.unwrap();
        let after = func_clone.invocation_count();

        assert_eq!(after - before, 2, "Should execute twice (verified)");
    }
}

// Test 10: Stats tracking works
#[tokio::test]
async fn test_verification_stats_tracking() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(CountingIdentity::new()));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let leaf_addr = leaf(b"test");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

    let r = recipe(vec![leaf_addr], "counting_identity");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    executor.materialize(addr).await.unwrap();

    let stats = &executor.verification_stats;
    assert_eq!(stats.total_verified.load(Ordering::Relaxed), 1);
    assert_eq!(stats.total_passed.load(Ordering::Relaxed), 1);
    assert_eq!(stats.total_failed.load(Ordering::Relaxed), 0);
    assert_eq!(stats.failure_rate(), 0.0);
}

// Test 11: Stats failure recorded
#[tokio::test]
async fn test_verification_stats_failure_recorded() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(NonDeterministicFn));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let r = recipe(vec![], "nondeterministic");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    let _ = executor.materialize(addr).await;

    let stats = &executor.verification_stats;
    assert_eq!(stats.total_verified.load(Ordering::Relaxed), 1);
    assert_eq!(stats.total_passed.load(Ordering::Relaxed), 0);
    assert_eq!(stats.total_failed.load(Ordering::Relaxed), 1);
    assert_eq!(stats.failure_rate(), 1.0);

    let last_failure = stats.last_failure.lock().await;
    assert!(last_failure.is_some());
}

// Test 12: Cached result not reverified
#[tokio::test]
async fn test_verification_cached_result_not_reverified() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let leaf_addr = leaf(b"test");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

    let r = recipe(vec![leaf_addr], "counting_identity");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    // First call: verified (2 executions)
    executor.materialize(addr).await.unwrap();
    assert_eq!(func_clone.invocation_count(), 2);

    // Second call: cache hit (0 additional executions)
    executor.materialize(addr).await.unwrap();
    assert_eq!(func_clone.invocation_count(), 2);
}

// Test 13: Parallel inputs each verified
#[tokio::test]
async fn test_verification_parallel_inputs_each_verified() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    // Create two leaf inputs
    let leaf1 = leaf(b"input1");
    let leaf2 = leaf(b"input2");
    leaves.leaves.lock().unwrap().insert(leaf1, Bytes::from("data1"));
    leaves.leaves.lock().unwrap().insert(leaf2, Bytes::from("data2"));

    // Create recipes for each input
    let r1 = recipe(vec![leaf1], "counting_identity");
    let addr1 = r1.addr();
    dag.recipes.lock().unwrap().insert(addr1, r1);

    let r2 = recipe(vec![leaf2], "counting_identity");
    let addr2 = r2.addr();
    dag.recipes.lock().unwrap().insert(addr2, r2);

    // Create final recipe combining both
    let r_final = recipe(vec![addr1, addr2], "counting_identity");
    let addr_final = r_final.addr();
    dag.recipes.lock().unwrap().insert(addr_final, r_final);

    executor.materialize(addr_final).await.unwrap();

    // Each of 3 recipes verified: 3 * 2 = 6 executions
    assert_eq!(func_clone.invocation_count(), 6);
}

// Test 14: Off mode with multiple calls
#[tokio::test]
async fn test_verification_off_multiple_calls() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::Off,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    for i in 0..5 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

        let r = recipe(vec![leaf_addr], "counting_identity");
        let addr = r.addr();
        dag.recipes.lock().unwrap().insert(addr, r);

        executor.materialize(addr).await.unwrap();
    }

    // 5 calls, each executed once
    assert_eq!(func_clone.invocation_count(), 5);
}

// Test 15: DualCompute stats accumulate
#[tokio::test]
async fn test_verification_dual_stats_accumulate() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(CountingIdentity::new()));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    for i in 0..10 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

        let r = recipe(vec![leaf_addr], "counting_identity");
        let addr = r.addr();
        dag.recipes.lock().unwrap().insert(addr, r);

        executor.materialize(addr).await.unwrap();
    }

    let stats = &executor.verification_stats;
    assert_eq!(stats.total_verified.load(Ordering::Relaxed), 10);
    assert_eq!(stats.total_passed.load(Ordering::Relaxed), 10);
}

// Test 16: Mixed deterministic and non-deterministic
#[tokio::test]
async fn test_verification_mixed_functions() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(CountingIdentity::new()));
    registry.register(Arc::new(NonDeterministicFn));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    // Deterministic
    let leaf1 = leaf(b"det");
    leaves.leaves.lock().unwrap().insert(leaf1, Bytes::from("data"));
    let r1 = recipe(vec![leaf1], "counting_identity");
    let addr1 = r1.addr();
    dag.recipes.lock().unwrap().insert(addr1, r1);

    // Non-deterministic
    let r2 = recipe(vec![], "nondeterministic");
    let addr2 = r2.addr();
    dag.recipes.lock().unwrap().insert(addr2, r2);

    assert!(executor.materialize(addr1).await.is_ok());
    assert!(executor.materialize(addr2).await.is_err());

    let stats = &executor.verification_stats;
    assert_eq!(stats.total_verified.load(Ordering::Relaxed), 2);
    assert_eq!(stats.total_passed.load(Ordering::Relaxed), 1);
    assert_eq!(stats.total_failed.load(Ordering::Relaxed), 1);
    assert_eq!(stats.failure_rate(), 0.5);
}

// Test 17: Sampled mode stats
#[tokio::test]
async fn test_verification_sampled_stats() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(CountingIdentity::new()));

    let config = ExecutorConfig {
        verification: VerificationMode::Sampled { rate: 0.5 },
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    for i in 0..100 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

        let r = recipe(vec![leaf_addr], "counting_identity");
        let addr = r.addr();
        dag.recipes.lock().unwrap().insert(addr, r);

        executor.materialize(addr).await.unwrap();
    }

    let stats = &executor.verification_stats;
    let verified = stats.total_verified.load(Ordering::Relaxed);
    // Should verify roughly 50 (allow 30-70)
    assert!(verified >= 30 && verified <= 70);
    assert_eq!(stats.total_passed.load(Ordering::Relaxed), verified);
}

// Test 18: Empty inputs deterministic
#[tokio::test]
async fn test_verification_empty_inputs() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(CountingIdentity::new()));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let r = recipe(vec![], "counting_identity");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    let result = executor.materialize(addr).await.unwrap();
    assert_eq!(result, Bytes::from("empty"));
}

// Test 19: Verification mode can be changed per executor
#[tokio::test]
async fn test_verification_mode_per_executor() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    let func_arc: Arc<dyn ComputeFunction> = Arc::new(func);
    registry.register(Arc::clone(&func_arc));

    let leaf_addr = leaf(b"test");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

    let r = recipe(vec![leaf_addr], "counting_identity");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    // Executor 1: Off
    let config1 = ExecutorConfig {
        verification: VerificationMode::Off,
        ..Default::default()
    };
    let executor1 = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache.clone(), leaves.clone(), config1);
    let before1 = func_clone.invocation_count();
    executor1.materialize(addr).await.unwrap();
    let after1 = func_clone.invocation_count();
    assert_eq!(after1 - before1, 1);

    // Executor 2: DualCompute with separate cache
    let cache2 = Arc::new(SharedCache::new(EvictableCache::new(Default::default())));
    let config2 = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let mut registry2 = FunctionRegistry::new();
    registry2.register(Arc::clone(&func_arc));
    let executor2 = AsyncExecutor::with_config(dag.clone(), Arc::new(registry2), cache2, leaves.clone(), config2);
    let before2 = func_clone.invocation_count();
    executor2.materialize(addr).await.unwrap();
    let after2 = func_clone.invocation_count();
    assert_eq!(after2 - before2, 2);
}

// Test 20: Failure rate calculation
#[tokio::test]
async fn test_verification_failure_rate_calculation() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(CountingIdentity::new()));
    registry.register(Arc::new(NonDeterministicFn));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    // 3 deterministic
    for i in 0..3 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));
        let r = recipe(vec![leaf_addr], "counting_identity");
        let addr = r.addr();
        dag.recipes.lock().unwrap().insert(addr, r);
        executor.materialize(addr).await.unwrap();
    }

    // 1 non-deterministic
    let r = recipe(vec![], "nondeterministic");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);
    let _ = executor.materialize(addr).await;

    let stats = &executor.verification_stats;
    assert_eq!(stats.failure_rate(), 0.25); // 1 failure out of 4
}

// Test 21: Sampled with different rates
#[tokio::test]
async fn test_verification_sampled_different_rates() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    // Test rate 0.1 (10%)
    let config = ExecutorConfig {
        verification: VerificationMode::Sampled { rate: 0.1 },
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache.clone(), leaves.clone(), config);

    let mut verified = 0;
    for i in 0..100 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));
        let r = recipe(vec![leaf_addr], "counting_identity");
        let addr = r.addr();
        dag.recipes.lock().unwrap().insert(addr, r);

        let before = func_clone.invocation_count();
        executor.materialize(addr).await.unwrap();
        let after = func_clone.invocation_count();
        if after - before == 2 {
            verified += 1;
        }
    }

    // Should verify roughly 10 (allow 3-20)
    assert!(verified >= 3 && verified <= 20);
}

// Test 22: Off mode never updates stats
#[tokio::test]
async fn test_verification_off_no_stats() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(CountingIdentity::new()));

    let config = ExecutorConfig {
        verification: VerificationMode::Off,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    for i in 0..10 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));
        let r = recipe(vec![leaf_addr], "counting_identity");
        let addr = r.addr();
        dag.recipes.lock().unwrap().insert(addr, r);
        executor.materialize(addr).await.unwrap();
    }

    let stats = &executor.verification_stats;
    assert_eq!(stats.total_verified.load(Ordering::Relaxed), 0);
}

// Test 23: Deep DAG verification
#[tokio::test]
async fn test_verification_deep_dag() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    // Create chain: leaf -> r1 -> r2 -> r3
    let leaf = leaf(b"base");
    leaves.leaves.lock().unwrap().insert(leaf, Bytes::from("data"));

    let r1 = recipe(vec![leaf], "counting_identity");
    let addr1 = r1.addr();
    dag.recipes.lock().unwrap().insert(addr1, r1);

    let r2 = recipe(vec![addr1], "counting_identity");
    let addr2 = r2.addr();
    dag.recipes.lock().unwrap().insert(addr2, r2);

    let r3 = recipe(vec![addr2], "counting_identity");
    let addr3 = r3.addr();
    dag.recipes.lock().unwrap().insert(addr3, r3);

    executor.materialize(addr3).await.unwrap();

    // 3 recipes, each verified: 3 * 2 = 6
    assert_eq!(func_clone.invocation_count(), 6);
}

// Test 24: Concurrent verification
#[tokio::test]
async fn test_verification_concurrent() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(CountingIdentity::new()));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        max_concurrency: 4,
        ..Default::default()
    };
    let executor = Arc::new(AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config));

    let mut handles = vec![];
    for i in 0..10 {
        let exec = Arc::clone(&executor);
        let dag_clone = Arc::clone(&dag);
        let leaves_clone = Arc::clone(&leaves);

        let handle = tokio::spawn(async move {
            let leaf_addr = leaf(&[i]);
            leaves_clone.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

            let r = recipe(vec![leaf_addr], "counting_identity");
            let addr = r.addr();
            dag_clone.recipes.lock().unwrap().insert(addr, r);

            exec.materialize(addr).await.unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let stats = &executor.verification_stats;
    assert_eq!(stats.total_verified.load(Ordering::Relaxed), 10);
}

// Test 25: Sampled boundary values
#[tokio::test]
async fn test_verification_sampled_boundaries() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    // Test rate 0.01 (1%)
    let config = ExecutorConfig {
        verification: VerificationMode::Sampled { rate: 0.01 },
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache.clone(), leaves.clone(), config);

    let mut verified = 0;
    for i in 0..100 {
        let leaf_addr = leaf(&[i]);
        leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));
        let r = recipe(vec![leaf_addr], "counting_identity");
        let addr = r.addr();
        dag.recipes.lock().unwrap().insert(addr, r);

        let before = func_clone.invocation_count();
        executor.materialize(addr).await.unwrap();
        let after = func_clone.invocation_count();
        if after - before == 2 {
            verified += 1;
        }
    }

    // Should verify roughly 1 (allow 0-5)
    assert!(verified <= 5);
}

// Test 26: Non-deterministic error message format
#[tokio::test]
async fn test_verification_error_message_format() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(NonDeterministicFn));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let r = recipe(vec![], "nondeterministic");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    let result = executor.materialize(addr).await;
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("determinism violation"));
    assert!(err_msg.contains("nondeterministic"));
    assert!(err_msg.contains("hash="));
}

// Test 27: Stats with zero verifications
#[tokio::test]
async fn test_verification_stats_zero_verifications() {
    let (dag, registry, cache, leaves) = setup();

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag, Arc::new(registry), cache, leaves.clone(), config);

    let stats = &executor.verification_stats;
    assert_eq!(stats.failure_rate(), 0.0);
}

// Test 28: Multiple failures accumulate
#[tokio::test]
async fn test_verification_multiple_failures() {
    let (dag, mut registry, cache, leaves) = setup();
    registry.register(Arc::new(NonDeterministicFn));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    for i in 0..5 {
        let r = recipe(vec![], "nondeterministic");
        let addr = CAddr::from_raw([i; 32]); // Different addresses
        dag.recipes.lock().unwrap().insert(addr, r);
        let _ = executor.materialize(addr).await;
    }

    let stats = &executor.verification_stats;
    assert_eq!(stats.total_failed.load(Ordering::Relaxed), 5);
    assert_eq!(stats.failure_rate(), 1.0);
}

// Test 29: Sampled mode with single address
#[tokio::test]
async fn test_verification_sampled_single_address() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::Sampled { rate: 0.5 },
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let leaf_addr = leaf(b"single");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

    let r = recipe(vec![leaf_addr], "counting_identity");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    // Multiple calls to same address
    for _ in 0..5 {
        executor.materialize(addr).await.unwrap();
    }

    // First call verified or not, rest are cache hits
    let count = func_clone.invocation_count();
    assert!(count == 1 || count == 2);
}

// Test 30: Verification with leaf-only recipe
#[tokio::test]
async fn test_verification_leaf_only() {
    let (dag, mut registry, cache, leaves) = setup();
    let func = CountingIdentity::new();
    let func_clone = func.clone();
    registry.register(Arc::new(func));

    let config = ExecutorConfig {
        verification: VerificationMode::DualCompute,
        ..Default::default()
    };
    let executor = AsyncExecutor::with_config(dag.clone(), Arc::new(registry), cache, leaves.clone(), config);

    let leaf_addr = leaf(b"leaf");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("leaf_data"));

    let r = recipe(vec![leaf_addr], "counting_identity");
    let addr = r.addr();
    dag.recipes.lock().unwrap().insert(addr, r);

    let result = executor.materialize(addr).await.unwrap();
    assert_eq!(result, Bytes::from("leaf_data"));
    assert_eq!(func_clone.invocation_count(), 2); // Verified
}



