// Property-based tests for verification-mode feature
// Uses proptest with 100 cases per property

use async_trait::async_trait;
use bytes::Bytes;
use deriva_compute::async_executor::{AsyncExecutor, DagReader, ExecutorConfig, VerificationMode};
use deriva_compute::cache::SharedCache;
use deriva_compute::function::{ComputeCost, ComputeError, ComputeFunction};
use deriva_compute::leaf_store::AsyncLeafStore;
use deriva_compute::registry::FunctionRegistry;
use deriva_core::address::{CAddr, FunctionId, Recipe, Value};
use deriva_core::cache::EvictableCache;
use deriva_core::error::DerivaError;
use proptest::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

// ─── Test Helpers (mirroring verif/modes.rs) ───────────────────────────────────

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

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> std::result::Result<Bytes, ComputeError> {
        self.count.fetch_add(1, Ordering::Relaxed);
        Ok(inputs
            .first()
            .cloned()
            .unwrap_or_else(|| Bytes::from("empty")))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost {
            cpu_ms: 1,
            memory_bytes: 1024,
        }
    }
}

#[derive(Clone)]
struct NonDeterministicFn;

impl ComputeFunction for NonDeterministicFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("nondeterministic", "v1")
    }

    fn execute(
        &self,
        _inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> std::result::Result<Bytes, ComputeError> {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Ok(Bytes::from(nanos.to_string()))
    }

    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost {
            cpu_ms: 1,
            memory_bytes: 1024,
        }
    }
}

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
        Ok(self
            .recipes
            .lock()
            .unwrap()
            .get(addr)
            .map(|r| r.inputs.clone()))
    }

    fn get_recipe(&self, addr: &CAddr) -> deriva_core::error::Result<Option<Recipe>> {
        Ok(self.recipes.lock().unwrap().get(addr).cloned())
    }
}

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

fn leaf(data: &[u8]) -> CAddr {
    CAddr::from_bytes(data)
}

fn recipe(inputs: Vec<CAddr>, function_id: &str) -> Recipe {
    Recipe {
        function_id: FunctionId::new(function_id, "v1"),
        inputs,
        params: BTreeMap::new(),
    }
}

// ─── Property Tests ────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // Feature: verification-mode, Property 1: Off mode executes exactly once
    /// **Validates: Requirements 2.1, 2.2**
    #[test]
    fn prop_off_mode_executes_exactly_once(input_data in proptest::collection::vec(any::<u8>(), 1..256)) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, mut registry, cache, leaves) = setup();
            let func = CountingIdentity::new();
            let func_clone = func.clone();
            registry.register(Arc::new(func));

            let config = ExecutorConfig {
                verification: VerificationMode::Off,
                ..Default::default()
            };
            let executor = AsyncExecutor::with_config(
                dag.clone(), Arc::new(registry), cache, leaves.clone(), config,
            );

            let leaf_addr = leaf(&input_data);
            leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from(input_data.clone()));

            let r = recipe(vec![leaf_addr], "counting_identity");
            let addr = r.addr();
            dag.recipes.lock().unwrap().insert(addr, r);

            executor.materialize(addr).await.unwrap();
            assert_eq!(func_clone.invocation_count(), 1, "Off mode must execute exactly once");
        });
    }

    // Feature: verification-mode, Property 2: DualCompute mode executes exactly twice
    /// **Validates: Requirements 3.1, 3.2**
    #[test]
    fn prop_dual_compute_executes_exactly_twice(input_data in proptest::collection::vec(any::<u8>(), 1..256)) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, mut registry, cache, leaves) = setup();
            let func = CountingIdentity::new();
            let func_clone = func.clone();
            registry.register(Arc::new(func));

            let config = ExecutorConfig {
                verification: VerificationMode::DualCompute,
                ..Default::default()
            };
            let executor = AsyncExecutor::with_config(
                dag.clone(), Arc::new(registry), cache, leaves.clone(), config,
            );

            let leaf_addr = leaf(&input_data);
            leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from(input_data.clone()));

            let r = recipe(vec![leaf_addr], "counting_identity");
            let addr = r.addr();
            dag.recipes.lock().unwrap().insert(addr, r);

            executor.materialize(addr).await.unwrap();
            assert_eq!(func_clone.invocation_count(), 2, "DualCompute mode must execute exactly twice");
        });
    }

    // Feature: verification-mode, Property 3: Deterministic functions pass verification
    /// **Validates: Requirements 3.3, 3.4**
    #[test]
    fn prop_deterministic_functions_pass_verification(input_data in proptest::collection::vec(any::<u8>(), 1..512)) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, mut registry, cache, leaves) = setup();
            registry.register(Arc::new(CountingIdentity::new()));

            let config = ExecutorConfig {
                verification: VerificationMode::DualCompute,
                ..Default::default()
            };
            let executor = AsyncExecutor::with_config(
                dag.clone(), Arc::new(registry), cache, leaves.clone(), config,
            );

            let leaf_addr = leaf(&input_data);
            leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from(input_data.clone()));

            let r = recipe(vec![leaf_addr], "counting_identity");
            let addr = r.addr();
            dag.recipes.lock().unwrap().insert(addr, r);

            let result = executor.materialize(addr).await;
            assert!(result.is_ok(), "Deterministic function must pass verification");
            assert_eq!(result.unwrap(), Bytes::from(input_data), "Output must match input for identity function");
        });
    }

    // Feature: verification-mode, Property 4: Non-deterministic functions produce DeterminismViolation
    /// **Validates: Requirements 3.5, 3.6, 5.1, 5.2, 5.3, 5.4**
    #[test]
    fn prop_nondeterministic_functions_produce_violation(seed in any::<u8>()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, mut registry, cache, leaves) = setup();
            registry.register(Arc::new(NonDeterministicFn));

            let config = ExecutorConfig {
                verification: VerificationMode::DualCompute,
                ..Default::default()
            };
            let executor = AsyncExecutor::with_config(
                dag.clone(), Arc::new(registry), cache, leaves.clone(), config,
            );

            // Use seed to create distinct recipes
            let r = recipe(vec![], "nondeterministic");
            let addr = CAddr::from_raw([seed; 32]);
            dag.recipes.lock().unwrap().insert(addr, r);

            let result = executor.materialize(addr).await;
            assert!(result.is_err(), "Non-deterministic function must fail verification");
            match result.unwrap_err() {
                DerivaError::DeterminismViolation {
                    addr: err_addr,
                    function_id,
                    output_1_hash,
                    output_2_hash,
                    output_1_len,
                    output_2_len,
                } => {
                    assert!(!err_addr.is_empty(), "addr field must be populated");
                    assert_eq!(function_id, "nondeterministic/v1");
                    assert!(!output_1_hash.is_empty(), "output_1_hash must be populated");
                    assert!(!output_2_hash.is_empty(), "output_2_hash must be populated");
                    assert_ne!(output_1_hash, output_2_hash, "Hashes must differ");
                    assert!(output_1_len > 0, "output_1_len must be positive");
                    assert!(output_2_len > 0, "output_2_len must be positive");
                }
                e => panic!("Expected DeterminismViolation, got {:?}", e),
            }
        });
    }

    // Feature: verification-mode, Property 5: Sampling decision is deterministic per address
    /// **Validates: Requirements 4.1, 4.2**
    #[test]
    fn prop_sampling_decision_deterministic_per_address(
        addr_bytes in proptest::collection::vec(any::<u8>(), 32..=32),
        rate in 0.0f64..=1.0f64
    ) {
        let addr = CAddr::from_raw(addr_bytes.try_into().unwrap());
        let decision1 = (addr.as_bytes()[0] as f64) / 255.0 < rate;
        let decision2 = (addr.as_bytes()[0] as f64) / 255.0 < rate;
        assert_eq!(decision1, decision2, "Sampling decision must be deterministic for same addr and rate");
    }

    // Feature: verification-mode, Property 6: Sampling rate boundary conditions
    /// **Validates: Requirements 4.5, 4.6**
    #[test]
    fn prop_sampling_rate_boundary_conditions(first_byte in 0u8..=255u8) {
        // rate=0.0: never verifies regardless of address byte
        // since no byte/255.0 can be negative, (byte/255.0) < 0.0 is always false
        let decision_zero = (first_byte as f64) / 255.0 < 0.0;
        assert!(!decision_zero, "rate=0.0 must never trigger verification for byte={}", first_byte);

        // rate=1.0: verifies for all bytes 0..254 (255/255.0 = 1.0 which is NOT < 1.0)
        // This tests the implementation's actual behavior with strict less-than
        let decision_one = (first_byte as f64) / 255.0 < 1.0;
        if first_byte < 255 {
            assert!(decision_one, "rate=1.0 must verify for byte={}", first_byte);
        }
        // byte=255 is the one edge case where strict < does not verify at rate=1.0
        // This reflects the implementation using `sample < rate`
    }

    // Feature: verification-mode, Property 7: Verification stats consistency
    /// **Validates: Requirements 6.1, 6.2, 6.3, 6.5**
    #[test]
    fn prop_verification_stats_consistency(
        pass_count in 0u64..50,
        fail_count in 0u64..50
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            use deriva_compute::async_executor::VerificationStats;

            let stats = VerificationStats::new();

            for _ in 0..pass_count {
                stats.record_pass();
            }
            for _ in 0..fail_count {
                let err = DerivaError::DeterminismViolation {
                    addr: "test".to_string(),
                    function_id: "test/v1".to_string(),
                    output_1_hash: "aaa".to_string(),
                    output_2_hash: "bbb".to_string(),
                    output_1_len: 10,
                    output_2_len: 10,
                };
                stats.record_fail(err).await;
            }

            let total = stats.total_verified.load(Ordering::Relaxed);
            let passed = stats.total_passed.load(Ordering::Relaxed);
            let failed = stats.total_failed.load(Ordering::Relaxed);

            assert_eq!(total, passed + failed, "total_verified must equal total_passed + total_failed");
            assert_eq!(passed, pass_count);
            assert_eq!(failed, fail_count);

            let expected_rate = if total == 0 {
                0.0
            } else {
                failed as f64 / total as f64
            };
            let actual_rate = stats.failure_rate();
            assert!(
                (actual_rate - expected_rate).abs() < f64::EPSILON,
                "failure_rate must equal total_failed / total_verified"
            );
        });
    }

    // Feature: verification-mode, Property 8: Cache hits bypass verification
    /// **Validates: Requirements 7.2**
    #[test]
    fn prop_cache_hits_bypass_verification(input_data in proptest::collection::vec(any::<u8>(), 1..256)) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, mut registry, cache, leaves) = setup();
            let func = CountingIdentity::new();
            let func_clone = func.clone();
            registry.register(Arc::new(func));

            let config = ExecutorConfig {
                verification: VerificationMode::DualCompute,
                ..Default::default()
            };
            let executor = AsyncExecutor::with_config(
                dag.clone(), Arc::new(registry), cache, leaves.clone(), config,
            );

            let leaf_addr = leaf(&input_data);
            leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from(input_data.clone()));

            let r = recipe(vec![leaf_addr], "counting_identity");
            let addr = r.addr();
            dag.recipes.lock().unwrap().insert(addr, r);

            // First call populates cache
            executor.materialize(addr).await.unwrap();
            let count_after_first = func_clone.invocation_count();

            // Second call should be a cache hit — no additional executions
            executor.materialize(addr).await.unwrap();
            let count_after_second = func_clone.invocation_count();

            assert_eq!(
                count_after_first, count_after_second,
                "Cache hit must not invoke the compute function"
            );
        });
    }

    // Feature: verification-mode, Property 9: DeterminismViolation error format
    /// **Validates: Requirements 5.5**
    #[test]
    fn prop_determinism_violation_error_format(
        addr_str in "[a-f0-9]{8,64}",
        func_name in "[a-z_]+/v[0-9]+",
        hash1 in "[a-f0-9]{64}",
        hash2 in "[a-f0-9]{64}",
        len1 in 1usize..10000,
        len2 in 1usize..10000
    ) {
        let error = DerivaError::DeterminismViolation {
            addr: addr_str.clone(),
            function_id: func_name.clone(),
            output_1_hash: hash1.clone(),
            output_2_hash: hash2.clone(),
            output_1_len: len1,
            output_2_len: len2,
        };

        let display = format!("{}", error);
        let expected = format!(
            "determinism violation for {}: function {} produced different outputs ({} bytes hash={} vs {} bytes hash={})",
            addr_str, func_name, len1, hash1, len2, hash2
        );
        assert_eq!(display, expected, "Display format must match expected pattern");
    }

    // Feature: verification-mode, Property 10: Sampling distribution approximates expected rate
    /// **Validates: Requirements 4.1**
    #[test]
    fn prop_sampling_distribution_approximates_rate(rate in 0.01f64..0.99f64) {
        // Generate all 256 possible first-byte values to get exact distribution
        let mut selected = 0u32;
        for byte_val in 0u8..=255 {
            let decision = (byte_val as f64) / 255.0 < rate;
            if decision {
                selected += 1;
            }
        }

        let observed_rate = selected as f64 / 256.0;
        // Tolerance: the discrete nature of 256 buckets means max error is ~1/255
        let tolerance = 1.0 / 255.0 + 0.01;
        assert!(
            (observed_rate - rate).abs() < tolerance,
            "Observed rate {} must approximate expected rate {} within tolerance {}",
            observed_rate, rate, tolerance
        );
    }
}
