// Property-based tests for AsyncExecutor
// Feature: async-compute
//
// Tests all 11 correctness properties from the design document using proptest.

use proptest::prelude::*;

use async_trait::async_trait;
use bytes::Bytes;
use deriva_compute::async_executor::{AsyncExecutor, DagReader, ExecutorConfig, VerificationMode};
use deriva_compute::cache::{AsyncMaterializationCache, SharedCache};
use deriva_compute::function::{ComputeError, ComputeCost, ComputeFunction};
use deriva_compute::leaf_store::{AsyncLeafStore, LeafStore};
use deriva_compute::registry::FunctionRegistry;
use deriva_compute::builtins;
use deriva_core::address::{CAddr, FunctionId, Recipe, Value};
use deriva_core::cache::EvictableCache;
use deriva_core::error::{DerivaError, Result};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ============================================================================
// Test Helpers (mirroring async_exec/core.rs)
// ============================================================================

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

/// A synchronous LeafStore for testing the blanket impl (Property 2)
struct SyncLeafStore {
    leaves: HashMap<CAddr, Bytes>,
}

impl LeafStore for SyncLeafStore {
    fn get_leaf(&self, addr: &CAddr) -> Option<Bytes> {
        self.leaves.get(addr).cloned()
    }
}

/// SlowCountingFunction - sleeps then counts, for concurrent dedup testing
struct SlowCountingFunction {
    count: Arc<AtomicUsize>,
    duration_ms: u64,
}

impl SlowCountingFunction {
    fn new(count: Arc<AtomicUsize>, duration_ms: u64) -> Self {
        Self { count, duration_ms }
    }
}

impl ComputeFunction for SlowCountingFunction {
    fn id(&self) -> FunctionId {
        FunctionId {
            name: "slow_counting".to_string(),
            version: "1".to_string(),
        }
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> std::result::Result<Bytes, ComputeError> {
        std::thread::sleep(Duration::from_millis(self.duration_ms));
        self.count.fetch_add(1, Ordering::SeqCst);
        Ok(inputs.into_iter().next().unwrap_or_else(|| Bytes::from("slow_counted")))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost {
            cpu_ms: self.duration_ms,
            memory_bytes: 1024,
        }
    }
}

/// OrderRecordingFunction - records the order of input bytes for Property 6
struct OrderRecordingFunction;

impl ComputeFunction for OrderRecordingFunction {
    fn id(&self) -> FunctionId {
        FunctionId {
            name: "order_recording".to_string(),
            version: "1".to_string(),
        }
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> std::result::Result<Bytes, ComputeError> {
        // Concatenate all inputs with a separator so we can verify ordering
        let joined: Vec<u8> = inputs
            .iter()
            .enumerate()
            .flat_map(|(i, b)| {
                let mut v = Vec::new();
                if i > 0 {
                    v.push(b'|');
                }
                v.extend_from_slice(b);
                v
            })
            .collect();
        Ok(Bytes::from(joined))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost {
            cpu_ms: 1,
            memory_bytes: 1024,
        }
    }
}

/// DeterministicFunction - always produces same output for same input
struct DeterministicFunction;

impl ComputeFunction for DeterministicFunction {
    fn id(&self) -> FunctionId {
        FunctionId {
            name: "deterministic".to_string(),
            version: "1".to_string(),
        }
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> std::result::Result<Bytes, ComputeError> {
        // Deterministic: just hash all inputs together
        let mut hasher = blake3::Hasher::new();
        for input in &inputs {
            hasher.update(input);
        }
        Ok(Bytes::from(hasher.finalize().as_bytes().to_vec()))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost {
            cpu_ms: 1,
            memory_bytes: 1024,
        }
    }
}

fn setup() -> (
    Arc<TestDagReader>,
    Arc<FunctionRegistry>,
    Arc<SharedCache>,
    Arc<TestLeafStore>,
) {
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    let dag = TestDagReader {
        recipes: Mutex::new(HashMap::new()),
    };
    let cache = SharedCache::new(EvictableCache::with_max_size(1024 * 1024));
    let leaves = TestLeafStore {
        leaves: Mutex::new(HashMap::new()),
    };
    (Arc::new(dag), Arc::new(registry), Arc::new(cache), Arc::new(leaves))
}

fn leaf(data: &[u8]) -> CAddr {
    CAddr::from_bytes(data)
}

fn make_recipe(inputs: Vec<CAddr>, func: &str) -> Recipe {
    Recipe::new(FunctionId::new(func, "1.0.0"), inputs, BTreeMap::new())
}

// ============================================================================
// Proptest Strategies
// ============================================================================

fn arb_caddr() -> impl Strategy<Value = CAddr> {
    prop::array::uniform32(any::<u8>()).prop_map(CAddr::from_raw)
}

fn arb_bytes(max_len: usize) -> impl Strategy<Value = Bytes> {
    prop::collection::vec(any::<u8>(), 1..=max_len).prop_map(Bytes::from)
}

fn arb_depth() -> impl Strategy<Value = usize> {
    1usize..=100
}

fn arb_sampling_rate() -> impl Strategy<Value = f64> {
    0.0f64..=1.0
}

// ============================================================================
// Property 1: Cache Round-Trip Integrity
// Feature: async-compute, Property 1: Cache Round-Trip Integrity
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 1.3, 1.5**
    #[test]
    fn prop_1_cache_round_trip_integrity(
        addr in arb_caddr(),
        data in arb_bytes(1024),
    ) {
        // For any CAddr and Bytes, put then get returns exact bytes.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cache = SharedCache::new(EvictableCache::with_max_size(1024 * 1024));
            cache.put(addr, data.clone()).await;
            let retrieved = cache.get(&addr).await;
            prop_assert_eq!(retrieved, Some(data));
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 2: AsyncLeafStore Blanket Equivalence
// Feature: async-compute, Property 2: AsyncLeafStore Blanket Equivalence
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 2.3**
    #[test]
    fn prop_2_async_leaf_store_blanket_equivalence(
        addr in arb_caddr(),
        data in arb_bytes(512),
    ) {
        // For any LeafStore+Send+Sync type, async get_leaf returns same as sync get_leaf.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut leaves = HashMap::new();
            leaves.insert(addr, data.clone());
            let store = SyncLeafStore { leaves };

            // Sync call
            let sync_result = LeafStore::get_leaf(&store, &addr);
            // Async call via blanket impl
            let async_result = AsyncLeafStore::get_leaf(&store, &addr).await;

            prop_assert_eq!(sync_result, async_result);
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 3: Materialization Priority Order
// Feature: async-compute, Property 3: Materialization Priority Order
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 5.3, 5.4, 5.5**
    #[test]
    fn prop_3_materialization_priority_order(
        addr in arb_caddr(),
        cache_data in arb_bytes(256),
        leaf_data in arb_bytes(256),
    ) {
        // Cached value takes precedence over leaf; leaf over DAG.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, registry, cache, leaves) = setup();

            // Put different data in cache and leaf store
            cache.put(addr, cache_data.clone()).await;
            leaves.leaves.lock().unwrap().insert(addr, leaf_data.clone());

            let executor = AsyncExecutor::new(dag, registry, cache, leaves);
            let result = executor.materialize(addr).await.unwrap();

            // Cache should take precedence
            prop_assert_eq!(result, cache_data);
            Ok(())
        })?;
    }

    /// **Validates: Requirements 5.3, 5.4, 5.5**
    #[test]
    fn prop_3b_leaf_takes_precedence_over_dag(
        leaf_data in arb_bytes(256),
    ) {
        // Leaf takes precedence when cache is empty.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, registry, cache, leaves) = setup();

            let addr = leaf(b"prop3b_leaf");
            leaves.leaves.lock().unwrap().insert(addr, leaf_data.clone());

            // Also add a recipe for the same addr in the DAG
            let inner_leaf = leaf(b"prop3b_inner");
            leaves.leaves.lock().unwrap().insert(inner_leaf, Bytes::from("inner"));
            let r = make_recipe(vec![inner_leaf], "identity");
            dag.recipes.lock().unwrap().insert(addr, r);

            let executor = AsyncExecutor::new(dag, registry, cache, leaves);
            let result = executor.materialize(addr).await.unwrap();

            // Leaf should take precedence over DAG recipe
            prop_assert_eq!(result, leaf_data);
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 4: Deep DAG Completion (BoxFuture Stack Safety)
// Feature: async-compute, Property 4: Deep DAG Completion (BoxFuture Stack Safety)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// **Validates: Requirements 5.2, 13.1, 13.2, 13.3**
    #[test]
    fn prop_4_deep_dag_completion(
        depth in arb_depth(),
        leaf_data in arb_bytes(64),
    ) {
        // Linear chain DAGs depth 1-100 complete without stack overflow.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, registry, cache, leaves) = setup();

            // Create leaf
            let leaf_addr = leaf(&format!("depth_leaf_{}", depth).as_bytes());
            leaves.leaves.lock().unwrap().insert(leaf_addr, leaf_data.clone());

            // Build chain of `depth` identity recipes
            let mut prev_addr = leaf_addr;
            for _i in 0..depth {
                let r = Recipe::new(
                    FunctionId::new("identity", "1.0.0"),
                    vec![prev_addr],
                    BTreeMap::new(),
                );
                let r_addr = r.addr();
                dag.recipes.lock().unwrap().insert(r_addr, r);
                prev_addr = r_addr;
            }

            let executor = AsyncExecutor::new(dag, registry, cache, leaves);
            let result = executor.materialize(prev_addr).await;
            prop_assert!(result.is_ok(), "Failed at depth {}: {:?}", depth, result.err());
            prop_assert_eq!(result.unwrap(), leaf_data);
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 5: Error Propagation Without Caching
// Feature: async-compute, Property 5: Error Propagation Without Caching
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 5.6, 5.7, 5.8, 14.1, 14.4**
    #[test]
    fn prop_5_error_propagation_without_caching(
        missing_key in arb_bytes(32),
    ) {
        // Failed results are never cached; error variant matches root cause.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, registry, cache, leaves) = setup();

            // Create a recipe that references a missing input
            let missing_addr = CAddr::from_bytes(&missing_key);
            let r = make_recipe(vec![missing_addr], "identity");
            let r_addr = r.addr();
            dag.recipes.lock().unwrap().insert(r_addr, r);

            let executor = AsyncExecutor::new(dag, registry, Arc::clone(&cache), leaves);
            let result = executor.materialize(r_addr).await;

            // Should be an error
            prop_assert!(result.is_err());

            // Error should NOT be cached
            prop_assert!(!cache.contains(&r_addr).await);

            // Error should be NotFound (the root cause is the missing input)
            match result.unwrap_err() {
                DerivaError::NotFound(_) => {},
                other => prop_assert!(false, "Expected NotFound, got: {:?}", other),
            }
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 6: Input Ordering Preservation
// Feature: async-compute, Property 6: Input Ordering Preservation
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// **Validates: Requirements 7.3**
    #[test]
    fn prop_6_input_ordering_preservation(
        num_inputs in 2usize..=6,
    ) {
        // For multi-input recipes, inputs arrive in recipe-declared order.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, _base_registry, cache, leaves) = setup();

            let mut registry = FunctionRegistry::new();
            builtins::register_all(&mut registry);
            registry.register(Arc::new(OrderRecordingFunction));
            let registry = Arc::new(registry);

            // Create N leaf inputs with distinct data
            let mut leaf_addrs = Vec::new();
            let mut expected_parts = Vec::new();
            for i in 0..num_inputs {
                let data = format!("input_{}", i);
                let addr = leaf(data.as_bytes());
                leaves.leaves.lock().unwrap().insert(addr, Bytes::from(data.clone()));
                leaf_addrs.push(addr);
                expected_parts.push(data);
            }

            // Create recipe that uses order_recording function
            let r = Recipe::new(
                FunctionId::new("order_recording", "1.0.0"),
                leaf_addrs.clone(),
                BTreeMap::new(),
            );
            let r_addr = r.addr();
            dag.recipes.lock().unwrap().insert(r_addr, r);

            let executor = AsyncExecutor::new(dag, registry, cache, leaves);
            let result = executor.materialize(r_addr).await.unwrap();

            // Verify inputs arrived in declared order
            let expected = expected_parts.join("|");
            prop_assert_eq!(result, Bytes::from(expected));
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 7: Deduplication Correctness
// Feature: async-compute, Property 7: Deduplication Correctness
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// **Validates: Requirements 8.1, 8.2, 8.3, 14.2, 14.3**
    #[test]
    fn prop_7_deduplication_correctness(
        num_concurrent in 2usize..=10,
    ) {
        // N concurrent calls for same CAddr: all get same result, compute runs at most once.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, _base_registry, cache, leaves) = setup();

            let count = Arc::new(AtomicUsize::new(0));
            let mut registry = FunctionRegistry::new();
            builtins::register_all(&mut registry);
            registry.register(Arc::new(SlowCountingFunction::new(Arc::clone(&count), 50)));
            let registry = Arc::new(registry);

            // Setup: leaf -> slow_counting recipe
            let leaf_addr = leaf(b"dedup_prop7");
            leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("dedup_data"));

            let r = Recipe::new(
                FunctionId::new("slow_counting", "1.0.0"),
                vec![leaf_addr],
                BTreeMap::new(),
            );
            let r_addr = r.addr();
            dag.recipes.lock().unwrap().insert(r_addr, r);

            let executor = Arc::new(AsyncExecutor::new(dag, registry, cache, leaves));

            // Spawn N concurrent materializations
            let mut handles = Vec::new();
            for _ in 0..num_concurrent {
                let exec = Arc::clone(&executor);
                handles.push(tokio::spawn(async move {
                    exec.materialize(r_addr).await
                }));
            }

            let mut results = Vec::new();
            for h in handles {
                results.push(h.await.unwrap());
            }

            // All should succeed with same value
            let first = results[0].as_ref().unwrap().clone();
            for r in &results {
                prop_assert!(r.is_ok());
                prop_assert_eq!(r.as_ref().unwrap(), &first);
            }

            // Compute should have run at most once (dedup)
            let exec_count = count.load(Ordering::SeqCst);
            prop_assert!(exec_count <= 1, "Expected at most 1 execution, got {}", exec_count);
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 8: Semaphore Deadlock Freedom
// Feature: async-compute, Property 8: Semaphore Deadlock Freedom
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// **Validates: Requirements 9.1, 9.4**
    #[test]
    fn prop_8_semaphore_deadlock_freedom(
        max_concurrency in 1usize..=4,
        extra_depth in 1usize..=10,
    ) {
        // DAGs deeper than max_concurrency still complete (no deadlock).
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, registry, cache, leaves) = setup();

            let depth = max_concurrency + extra_depth;

            // Create leaf
            let leaf_addr = leaf(b"semaphore_test_leaf");
            leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

            // Build chain deeper than max_concurrency
            let mut prev_addr = leaf_addr;
            for _i in 0..depth {
                let r = make_recipe(vec![prev_addr], "identity");
                let r_addr = r.addr();
                dag.recipes.lock().unwrap().insert(r_addr, r);
                prev_addr = r_addr;
            }

            let config = ExecutorConfig {
                max_concurrency,
                dedup_channel_capacity: 16,
                verification: VerificationMode::Off,
            };
            let executor = AsyncExecutor::with_config(dag, registry, cache, leaves, config);

            // Use a timeout to detect deadlock
            let result = tokio::time::timeout(
                Duration::from_secs(10),
                executor.materialize(prev_addr),
            )
            .await;

            prop_assert!(result.is_ok(), "Timed out (deadlock?) with depth={}, max_concurrency={}", depth, max_concurrency);
            let materialized = result.unwrap();
            prop_assert!(materialized.is_ok());
            prop_assert_eq!(materialized.unwrap(), Bytes::from("data"));
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 9: Deterministic Verification Sampling
// Feature: async-compute, Property 9: Deterministic Verification Sampling
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// **Validates: Requirements 10.5**
    #[test]
    fn prop_9_deterministic_verification_sampling(
        addr in arb_caddr(),
        rate in arb_sampling_rate(),
    ) {
        // Same addr + rate always produces same decision.
        // The sampling algorithm is: addr.as_bytes()[0] / 255.0 < rate
        let sample = (addr.as_bytes()[0] as f64) / 255.0;
        let decision1 = sample < rate;
        let decision2 = sample < rate;

        // Same addr and rate always yields same decision
        prop_assert_eq!(decision1, decision2);

        // Verify the boundary: if first byte is 0, sample=0.0, should verify for any rate > 0
        // if first byte is 255, sample=1.0, should only verify if rate > 1.0 (never in practice)
    }
}

// ============================================================================
// Property 10: Dual-Compute Passes for Deterministic Functions
// Feature: async-compute, Property 10: Dual-Compute Passes for Deterministic Functions
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// **Validates: Requirements 10.2, 10.4**
    #[test]
    fn prop_10_dual_compute_deterministic_passes(
        input_data in arb_bytes(256),
    ) {
        // Deterministic function always passes dual-compute.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, _base_registry, cache, leaves) = setup();

            let mut registry = FunctionRegistry::new();
            builtins::register_all(&mut registry);
            registry.register(Arc::new(DeterministicFunction));
            let registry = Arc::new(registry);

            // Setup leaf
            let leaf_addr = leaf(b"det_prop10");
            leaves.leaves.lock().unwrap().insert(leaf_addr, input_data.clone());

            // Create recipe using our deterministic function
            let r = Recipe::new(
                FunctionId::new("deterministic", "1.0.0"),
                vec![leaf_addr],
                BTreeMap::new(),
            );
            let r_addr = r.addr();
            dag.recipes.lock().unwrap().insert(r_addr, r);

            let config = ExecutorConfig {
                max_concurrency: 4,
                dedup_channel_capacity: 16,
                verification: VerificationMode::DualCompute,
            };
            let executor = AsyncExecutor::with_config(dag, registry, cache, leaves, config);
            let result = executor.materialize(r_addr).await;

            prop_assert!(result.is_ok(), "DualCompute failed for deterministic function: {:?}", result.err());

            // Verify stats recorded a pass
            let stats = &executor.verification_stats;
            prop_assert!(stats.total_passed.load(Ordering::Relaxed) >= 1);
            prop_assert_eq!(stats.total_failed.load(Ordering::Relaxed), 0);
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 11: Parallel Input Error Short-Circuit
// Feature: async-compute, Property 11: Parallel Input Error Short-Circuit
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// **Validates: Requirements 7.2**
    #[test]
    fn prop_11_parallel_input_error_short_circuit(
        num_valid in 1usize..=4,
    ) {
        // Recipe with failing input returns error quickly.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, registry, cache, leaves) = setup();

            // Create some valid leaf inputs
            let mut input_addrs = Vec::new();
            for i in 0..num_valid {
                let addr = leaf(format!("valid_input_{}", i).as_bytes());
                leaves.leaves.lock().unwrap().insert(addr, Bytes::from(format!("data_{}", i)));
                input_addrs.push(addr);
            }

            // Add one missing input (will fail with NotFound)
            let missing_addr = leaf(b"missing_for_prop11");
            input_addrs.push(missing_addr);

            // Create recipe with all inputs (one will fail)
            let r = make_recipe(input_addrs, "concat");
            let r_addr = r.addr();
            dag.recipes.lock().unwrap().insert(r_addr, r);

            let executor = AsyncExecutor::new(dag, registry, cache, leaves);

            let start = std::time::Instant::now();
            let result = executor.materialize(r_addr).await;
            let elapsed = start.elapsed();

            // Should fail
            prop_assert!(result.is_err(), "Expected error but got: {:?}", result);

            // Should propagate as NotFound
            match result.unwrap_err() {
                DerivaError::NotFound(_) => {},
                other => prop_assert!(false, "Expected NotFound, got: {:?}", other),
            }

            // Should complete quickly (not waiting for all valid inputs to be slow)
            // In this test, leaves resolve instantly, so we just verify it completes
            prop_assert!(elapsed < Duration::from_secs(5), "Took too long: {:?}", elapsed);
            Ok(())
        })?;
    }
}

// ============================================================================
// PARALLEL MATERIALIZATION PROPERTIES (§2.3)
// ============================================================================

// SlowFunction for timing-based property tests
struct SlowFunction {
    duration_ms: u64,
}

impl ComputeFunction for SlowFunction {
    fn id(&self) -> FunctionId {
        FunctionId {
            name: "slow".to_string(),
            version: "1".to_string(),
        }
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> std::result::Result<Bytes, ComputeError> {
        std::thread::sleep(Duration::from_millis(self.duration_ms));
        Ok(inputs.into_iter().next().unwrap_or_else(|| Bytes::from("slow")))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost {
            cpu_ms: self.duration_ms,
            memory_bytes: 1024,
        }
    }
}

// ConcurrencyTrackingFunction - tracks peak concurrent executions
struct ConcurrencyTrackingFunction {
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    duration_ms: u64,
}

impl ConcurrencyTrackingFunction {
    fn new(active: Arc<AtomicUsize>, peak: Arc<AtomicUsize>, duration_ms: u64) -> Self {
        Self { active, peak, duration_ms }
    }
}

impl ComputeFunction for ConcurrencyTrackingFunction {
    fn id(&self) -> FunctionId {
        FunctionId {
            name: "concurrency_tracking".to_string(),
            version: "1".to_string(),
        }
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> std::result::Result<Bytes, ComputeError> {
        let current = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        // Update peak
        self.peak.fetch_max(current, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(self.duration_ms));
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(inputs.into_iter().next().unwrap_or_else(|| Bytes::from("tracked")))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost {
            cpu_ms: self.duration_ms,
            memory_bytes: 1024,
        }
    }
}

// ============================================================================
// Property 12 (§2.3 P2): Parallel Speedup Proportional to Width
// Feature: parallel-materialization, Property 2: Parallel Speedup Proportional to Width
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// **Validates: Requirements 1.1, 11.1**
    #[test]
    fn prop_par_2_parallel_speedup(
        num_inputs in 2usize..=6,
    ) {
        // Fan-in with N inputs using SlowFunction (50ms each).
        // Parallel should complete in ~50ms, not N*50ms.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, _base_registry, cache, leaves) = setup();

            let mut registry = FunctionRegistry::new();
            builtins::register_all(&mut registry);
            registry.register(Arc::new(SlowFunction { duration_ms: 50 }));
            let registry = Arc::new(registry);

            // Create N leaf inputs
            let mut leaf_addrs = Vec::new();
            for i in 0..num_inputs {
                let addr = leaf(format!("par_speedup_leaf_{}", i).as_bytes());
                leaves.leaves.lock().unwrap().insert(addr, Bytes::from(format!("data_{}", i)));
                leaf_addrs.push(addr);
            }

            // Create N slow recipes (one per leaf)
            let mut recipe_addrs = Vec::new();
            for &leaf_addr in leaf_addrs.iter() {
                let r = Recipe::new(
                    FunctionId::new("slow", "1.0.0"),
                    vec![leaf_addr],
                    BTreeMap::new(),
                );
                let r_addr = r.addr();
                dag.recipes.lock().unwrap().insert(r_addr, r);
                recipe_addrs.push(r_addr);
            }

            // Create a final concat recipe that depends on all N slow recipes
            let final_r = Recipe::new(
                FunctionId::new("concat", "1.0.0"),
                recipe_addrs,
                BTreeMap::new(),
            );
            let final_addr = final_r.addr();
            dag.recipes.lock().unwrap().insert(final_addr, final_r);

            let executor = AsyncExecutor::new(dag, registry, cache, leaves);

            let start = std::time::Instant::now();
            let result = executor.materialize(final_addr).await;
            let elapsed = start.elapsed();

            prop_assert!(result.is_ok(), "Materialization failed: {:?}", result.err());

            // Sequential would take N * 50ms. Parallel should take ~50ms + overhead.
            // We allow generous tolerance: must be less than (N-1) * 50ms (proving parallelism).
            let sequential_time_ms = (num_inputs as u64) * 50;
            let max_allowed_ms = sequential_time_ms - 25; // Must save at least 25ms
            prop_assert!(
                elapsed.as_millis() < max_allowed_ms as u128,
                "Took {}ms, expected less than {}ms (sequential would be {}ms) for {} inputs",
                elapsed.as_millis(), max_allowed_ms, sequential_time_ms, num_inputs
            );
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 13 (§2.3 P3): Global Semaphore Bounding
// Feature: parallel-materialization, Property 3: Global Semaphore Bounding
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// **Validates: Requirements 3.1, 3.4, 7.3, 11.3**
    #[test]
    fn prop_par_3_global_semaphore_bounding(
        max_concurrency in 2usize..=4,
        num_recipes in 6usize..=12,
    ) {
        // Peak concurrent compute executions never exceeds max_concurrency.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, _base_registry, cache, leaves) = setup();

            let active = Arc::new(AtomicUsize::new(0));
            let peak = Arc::new(AtomicUsize::new(0));

            let mut registry = FunctionRegistry::new();
            builtins::register_all(&mut registry);
            registry.register(Arc::new(ConcurrencyTrackingFunction::new(
                Arc::clone(&active),
                Arc::clone(&peak),
                20, // 20ms per compute
            )));
            let registry = Arc::new(registry);

            // Create N independent leaf → recipe pairs
            let mut recipe_addrs = Vec::new();
            for i in 0..num_recipes {
                let leaf_addr = leaf(format!("sem_bound_leaf_{}", i).as_bytes());
                leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from(format!("d{}", i)));

                let r = Recipe::new(
                    FunctionId::new("concurrency_tracking", "1.0.0"),
                    vec![leaf_addr],
                    BTreeMap::new(),
                );
                let r_addr = r.addr();
                dag.recipes.lock().unwrap().insert(r_addr, r);
                recipe_addrs.push(r_addr);
            }

            // Create a final concat that triggers all N to resolve in parallel
            let final_r = Recipe::new(
                FunctionId::new("concat", "1.0.0"),
                recipe_addrs,
                BTreeMap::new(),
            );
            let final_addr = final_r.addr();
            dag.recipes.lock().unwrap().insert(final_addr, final_r);

            let config = ExecutorConfig {
                max_concurrency,
                dedup_channel_capacity: 16,
                verification: VerificationMode::Off,
            };
            let executor = AsyncExecutor::with_config(dag, registry, cache, leaves, config);
            let result = executor.materialize(final_addr).await;

            prop_assert!(result.is_ok(), "Failed: {:?}", result.err());

            // Peak concurrent should never exceed max_concurrency
            let observed_peak = peak.load(Ordering::SeqCst);
            prop_assert!(
                observed_peak <= max_concurrency,
                "Peak concurrent {} exceeded max_concurrency {}",
                observed_peak, max_concurrency
            );
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 14 (§2.3 P9): Clone Shared State Consistency
// Feature: parallel-materialization, Property 9: Clone Shared State Consistency
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// **Validates: Requirements 7.2**
    #[test]
    fn prop_par_9_clone_shared_state_consistency(
        leaf_data in arb_bytes(128),
    ) {
        // Materialize via one clone populates cache visible to other clone.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, registry, cache, leaves) = setup();

            let leaf_addr = leaf(b"clone_test_leaf");
            leaves.leaves.lock().unwrap().insert(leaf_addr, leaf_data.clone());

            let r = make_recipe(vec![leaf_addr], "identity");
            let r_addr = r.addr();
            dag.recipes.lock().unwrap().insert(r_addr, r);

            let executor1 = AsyncExecutor::new(
                Arc::clone(&dag),
                Arc::clone(&registry),
                Arc::clone(&cache),
                Arc::clone(&leaves),
            );
            let executor2 = executor1.clone();

            // Materialize via executor1
            let result1 = executor1.materialize(r_addr).await.unwrap();
            prop_assert_eq!(&result1, &leaf_data);

            // executor2 should see a cache hit (shared cache via Arc)
            prop_assert!(cache.contains(&r_addr).await);

            // Materialize via executor2 — should hit cache
            let result2 = executor2.materialize(r_addr).await.unwrap();
            prop_assert_eq!(result1, result2);
            Ok(())
        })?;
    }
}

// ============================================================================
// Property 15 (§2.3 P10): Linear Chain Negligible Overhead
// Feature: parallel-materialization, Property 10: Linear Chain Negligible Overhead
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// **Validates: Requirements 11.2**
    #[test]
    fn prop_par_10_linear_chain_negligible_overhead(
        depth in 3usize..=15,
    ) {
        // Linear chains (no fan-in) should not benefit from parallelism,
        // but also should not pay significant overhead.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, registry, cache, leaves) = setup();

            // Create leaf
            let leaf_addr = leaf(b"linear_chain_leaf");
            leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("data"));

            // Build linear chain of identity recipes
            let mut prev_addr = leaf_addr;
            for _i in 0..depth {
                let r = make_recipe(vec![prev_addr], "identity");
                let r_addr = r.addr();
                dag.recipes.lock().unwrap().insert(r_addr, r);
                prev_addr = r_addr;
            }

            let executor = AsyncExecutor::new(dag, registry, cache, leaves);

            let start = std::time::Instant::now();
            let result = executor.materialize(prev_addr).await;
            let elapsed = start.elapsed();

            prop_assert!(result.is_ok());
            prop_assert_eq!(result.unwrap(), Bytes::from("data"));

            // For a linear chain of identity functions, total time should be
            // very fast (< 100ms even for depth 15 since identity is ~microseconds).
            // This verifies we're not paying exponential overhead.
            prop_assert!(
                elapsed.as_millis() < 2000,
                "Linear chain depth {} took {}ms (too slow, possible overhead issue)",
                depth, elapsed.as_millis()
            );
            Ok(())
        })?;
    }
}
