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
    leaves: Arc<Mutex<HashMap<CAddr, Bytes>>>,
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
    Arc<FunctionRegistry>,
    Arc<SharedCache>,
    Arc<MockLeafStore>,
) {
    let dag = Arc::new(MockDag::new());
    let registry = Arc::new(FunctionRegistry::new());
    let cache = Arc::new(SharedCache::new(EvictableCache::new(Default::default())));
    let leaves = Arc::new(MockLeafStore::new());
    (dag, registry, cache, leaves)
}

// Helper to create a recipe
fn recipe(inputs: Vec<CAddr>, function_id: &str) -> Recipe {
    Recipe {
        function_id: FunctionId::new(function_id, "v1"),
        inputs,
        params: BTreeMap::new(),
    }
}
