use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use async_trait::async_trait;
use bytes::Bytes;
use deriva_compute::async_executor::{AsyncExecutor, ExecutorConfig, DagReader};
use deriva_compute::cache::SharedCache;
use deriva_compute::leaf_store::AsyncLeafStore;
use deriva_compute::builtins;
use deriva_compute::function::{ComputeFunction, ComputeCost, ComputeError};
use deriva_compute::registry::FunctionRegistry;
use deriva_core::address::{CAddr, Recipe, FunctionId, Value};
use deriva_core::cache::EvictableCache;
use deriva_core::error::{DerivaError, Result};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Test infrastructure
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

// Slow function for timing tests
struct SlowFunction {
    duration_ms: u64,
}

impl ComputeFunction for SlowFunction {
    fn id(&self) -> FunctionId {
        FunctionId { name: "slow".to_string(), version: "1".to_string() }
    }
    
    fn execute(&self, inputs: Vec<Bytes>, _: &BTreeMap<String, Value>) -> std::result::Result<Bytes, ComputeError> {
        std::thread::sleep(Duration::from_millis(self.duration_ms));
        Ok(inputs.into_iter().next().unwrap_or_else(|| Bytes::from("slow")))
    }
    
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: self.duration_ms, memory_bytes: 1024 }
    }
}

fn setup() -> (Arc<TestDagReader>, Arc<FunctionRegistry>, Arc<SharedCache>, Arc<TestLeafStore>) {
    let dag = Arc::new(TestDagReader { recipes: Mutex::new(HashMap::new()) });
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    registry.register(Arc::new(SlowFunction { duration_ms: 10 }));
    let registry = Arc::new(registry);
    let cache = Arc::new(SharedCache::new(EvictableCache::with_max_size(1024 * 1024 * 100)));
    let leaves = Arc::new(TestLeafStore { leaves: Mutex::new(HashMap::new()) });
    (dag, registry, cache, leaves)
}

fn leaf(data: &[u8]) -> CAddr {
    CAddr::from_bytes(&Bytes::from(data.to_vec()))
}

// ============================================================================
// PARALLEL VS SEQUENTIAL BENCHMARKS
// ============================================================================

fn bench_fan_in_parallel_vs_sequential(c: &mut Criterion) {
    let mut group = c.benchmark_group("fan_in_parallel_vs_sequential");
    
    for num_inputs in [2, 4, 8, 16].iter() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (dag, registry, cache, leaves) = setup();
        
        // Create leaves
        let mut leaf_addrs = vec![];
        for i in 0..*num_inputs {
            let addr = leaf(&[i as u8]);
            leaves.leaves.lock().unwrap().insert(addr, Bytes::from(vec![i as u8]));
            leaf_addrs.push(addr);
        }
        
        // Create slow recipes
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
        let final_recipe = Recipe {
            inputs: slow_addrs,
            function_id: FunctionId { name: "concat".to_string(), version: "1.0.0".to_string() },
            params: BTreeMap::new(),
        };
        let final_addr = final_recipe.addr();
        dag.recipes.lock().unwrap().insert(final_addr, final_recipe);
        
        let executor = Arc::new(AsyncExecutor::new(
            Arc::clone(&dag),
            Arc::clone(&registry),
            Arc::clone(&cache),
            Arc::clone(&leaves),
        ));
        
        group.bench_with_input(
            BenchmarkId::new("parallel", num_inputs),
            num_inputs,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        black_box(executor.materialize(final_addr).await.unwrap())
                    })
                });
            },
        );
    }
    
    group.finish();
}

fn bench_diamond_dag(c: &mut Criterion) {
    let mut group = c.benchmark_group("diamond_dag");
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let (dag, registry, cache, leaves) = setup();
    
    let root = leaf(b"root");
    leaves.leaves.lock().unwrap().insert(root, Bytes::from("root"));
    
    // Two branches
    let branch1 = Recipe {
        inputs: vec![root],
        function_id: FunctionId { name: "slow".to_string(), version: "1".to_string() },
        params: BTreeMap::new(),
    };
    let branch1_addr = branch1.addr();
    dag.recipes.lock().unwrap().insert(branch1_addr, branch1);
    
    let branch2 = Recipe {
        inputs: vec![root],
        function_id: FunctionId { name: "slow".to_string(), version: "1".to_string() },
        params: BTreeMap::new(),
    };
    let branch2_addr = branch2.addr();
    dag.recipes.lock().unwrap().insert(branch2_addr, branch2);
    
    // Merge
    let merge = Recipe {
        inputs: vec![branch1_addr, branch2_addr],
        function_id: FunctionId { name: "concat".to_string(), version: "1.0.0".to_string() },
        params: BTreeMap::new(),
    };
    let merge_addr = merge.addr();
    dag.recipes.lock().unwrap().insert(merge_addr, merge);
    
    let executor = Arc::new(AsyncExecutor::new(dag, registry, Arc::clone(&cache), leaves));
    
    group.bench_function("parallel", |b| {
        b.iter(|| {
            rt.block_on(async {
                black_box(executor.materialize(merge_addr).await.unwrap())
            })
        });
    });
    
    group.finish();
}

// ============================================================================
// CONCURRENCY LIMIT BENCHMARKS
// ============================================================================

fn bench_concurrency_limits(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrency_limits");
    
    for limit in [1, 2, 4, 8, 16].iter() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (dag, registry, cache, leaves) = setup();
        
        // Create 16 leaves
        let mut slow_addrs = vec![];
        for i in 0..16 {
            let addr = leaf(&[i]);
            leaves.leaves.lock().unwrap().insert(addr, Bytes::from(vec![i]));
            
            let recipe = Recipe {
                inputs: vec![addr],
                function_id: FunctionId { name: "slow".to_string(), version: "1".to_string() },
                params: BTreeMap::new(),
            };
            let recipe_addr = recipe.addr();
            dag.recipes.lock().unwrap().insert(recipe_addr, recipe);
            slow_addrs.push(recipe_addr);
        }
        
        let final_recipe = Recipe {
            inputs: slow_addrs,
            function_id: FunctionId { name: "concat".to_string(), version: "1.0.0".to_string() },
            params: BTreeMap::new(),
        };
        let final_addr = final_recipe.addr();
        dag.recipes.lock().unwrap().insert(final_addr, final_recipe);
        
        let config = ExecutorConfig {
            max_concurrency: *limit,
            dedup_channel_capacity: 16,
            verification: Default::default(),
        };
        let executor = Arc::new(AsyncExecutor::with_config(
            dag,
            registry,
            Arc::clone(&cache),
            leaves,
            config,
        ));
        
        group.bench_with_input(
            BenchmarkId::from_parameter(limit),
            limit,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        black_box(executor.materialize(final_addr).await.unwrap())
                    })
                });
            },
        );
    }
    
    group.finish();
}

// ============================================================================
// DEDUPLICATION EFFECTIVENESS BENCHMARKS
// ============================================================================

fn bench_deduplication_effectiveness(c: &mut Criterion) {
    let mut group = c.benchmark_group("deduplication");
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let (dag, registry, cache, leaves) = setup();
    
    let leaf_addr = leaf(b"shared");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("shared"));
    
    let slow_recipe = Recipe {
        inputs: vec![leaf_addr],
        function_id: FunctionId { name: "slow".to_string(), version: "1".to_string() },
        params: BTreeMap::new(),
    };
    let slow_addr = slow_recipe.addr();
    dag.recipes.lock().unwrap().insert(slow_addr, slow_recipe);
    
    // Create 10 consumers
    let mut consumer_addrs = vec![];
    for _ in 0..10 {
        let recipe = Recipe {
            inputs: vec![slow_addr],
            function_id: FunctionId { name: "identity".to_string(), version: "1.0.0".to_string() },
            params: BTreeMap::new(),
        };
        let addr = recipe.addr();
        dag.recipes.lock().unwrap().insert(addr, recipe);
        consumer_addrs.push(addr);
    }
    
    let executor = Arc::new(AsyncExecutor::new(dag, registry, Arc::clone(&cache), leaves));
    
    group.bench_function("shared_input", |b| {
        b.iter(|| {
            rt.block_on(async {
                for addr in &consumer_addrs {
                    black_box(executor.materialize(*addr).await.unwrap());
                }
            })
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_fan_in_parallel_vs_sequential,
    bench_diamond_dag,
    bench_concurrency_limits,
    bench_deduplication_effectiveness
);
criterion_main!(benches);
