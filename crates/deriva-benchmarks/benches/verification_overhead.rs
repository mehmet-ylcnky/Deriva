use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use async_trait::async_trait;
use bytes::Bytes;
use deriva_compute::async_executor::{AsyncExecutor, ExecutorConfig, DagReader, VerificationMode};
use deriva_compute::cache::SharedCache;
use deriva_compute::leaf_store::AsyncLeafStore;
use deriva_compute::function::{ComputeFunction, ComputeCost, ComputeError};
use deriva_compute::registry::FunctionRegistry;
use deriva_core::address::{CAddr, Recipe, FunctionId, Value};
use deriva_core::cache::EvictableCache;
use deriva_core::error::{DerivaError, Result};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

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

struct Identity;
impl ComputeFunction for Identity {
    fn id(&self) -> FunctionId {
        FunctionId { name: "identity".into(), version: "1".into() }
    }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> std::result::Result<Bytes, ComputeError> {
        Ok(inputs.first().cloned().unwrap_or_default())
    }
    fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 1, memory_bytes: 1024 }
    }
}

fn bench_verification_modes(c: &mut Criterion) {
    let mut group = c.benchmark_group("verification_overhead");
    
    let modes = vec![
        ("off", VerificationMode::Off),
        ("dual", VerificationMode::DualCompute),
        ("sampled_0.1", VerificationMode::Sampled { rate: 0.1 }),
        ("sampled_0.5", VerificationMode::Sampled { rate: 0.5 }),
    ];
    
    for (name, mode) in modes {
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            b.iter(|| {
                rt.block_on(async {
                    let dag = Arc::new(TestDagReader { recipes: Mutex::new(HashMap::new()) });
                    let mut registry = FunctionRegistry::new();
                    registry.register(Arc::new(Identity));
                    let cache = Arc::new(SharedCache::new(EvictableCache::new(Default::default())));
                    let leaves = Arc::new(TestLeafStore { leaves: Mutex::new(HashMap::new()) });
                    
                    let config = ExecutorConfig {
                        verification: mode.clone(),
                        ..Default::default()
                    };
                    let executor = AsyncExecutor::with_config(
                        dag.clone(),
                        Arc::new(registry),
                        cache,
                        leaves.clone(),
                        config,
                    );
                    
                    let leaf_data = Bytes::from("test");
                    let leaf_addr = CAddr::from_bytes(&leaf_data);
                    leaves.leaves.lock().unwrap().insert(leaf_addr, leaf_data);
                    
                    let recipe = Recipe {
                        inputs: vec![leaf_addr],
                        function_id: FunctionId::new("identity", "1"),
                        params: BTreeMap::new(),
                    };
                    let recipe_addr = recipe.addr();
                    dag.recipes.lock().unwrap().insert(recipe_addr, recipe);
                    
                    black_box(executor.materialize(recipe_addr).await.unwrap());
                })
            });
        });
    }
    
    group.finish();
}

criterion_group!(benches, bench_verification_modes);
criterion_main!(benches);
