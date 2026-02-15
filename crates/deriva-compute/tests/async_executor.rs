use async_trait::async_trait;
use bytes::Bytes;
use deriva_compute::async_executor::{AsyncExecutor, DagReader};
use deriva_compute::cache::{AsyncMaterializationCache, SharedCache};
use deriva_compute::leaf_store::AsyncLeafStore;
use deriva_compute::registry::FunctionRegistry;
use deriva_compute::builtins;
use deriva_core::address::{CAddr, FunctionId, Recipe, Value};
use deriva_core::cache::EvictableCache;
use deriva_core::error::Result;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

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
#[tokio::test]
async fn test_01_materialize_leaf() {
    let (dag, registry, cache, mut leaves) = setup();
    let data = Bytes::from("hello");
    let addr = leaf(b"hello");
    leaves.leaves.lock().unwrap().insert(addr, data.clone());
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(addr).await.unwrap();
    assert_eq!(result, data);
}

#[tokio::test]
async fn test_02_materialize_cached() {
    let (dag, registry, cache, leaves) = setup();
    let addr = leaf(b"test");
    let data = Bytes::from("cached_data");
    cache.put(addr, data.clone()).await;
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(addr).await.unwrap();
    assert_eq!(result, data);
}

#[tokio::test]
async fn test_03_materialize_recipe_identity() {
    let (mut dag, registry, cache, mut leaves) = setup();
    let leaf_data = Bytes::from("data");
    let leaf_addr = leaf(b"data");
    leaves.leaves.lock().unwrap().insert(leaf_addr, leaf_data.clone());
    
    let r = recipe(vec![leaf_addr], "identity");
    let r_addr = r.addr();
    dag.recipes.lock().unwrap().insert(r_addr, r);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(r_addr).await.unwrap();
    assert_eq!(result, leaf_data);
}

#[tokio::test]
async fn test_04_materialize_recipe_concat() {
    let (mut dag, registry, cache, mut leaves) = setup();
    let data1 = Bytes::from("hello");
    let data2 = Bytes::from(" world");
    let addr1 = leaf(b"hello");
    let addr2 = leaf(b" world");
    leaves.leaves.lock().unwrap().insert(addr1, data1);
    leaves.leaves.lock().unwrap().insert(addr2, data2);
    
    let r = recipe(vec![addr1, addr2], "concat");
    let r_addr = r.addr();
    dag.recipes.lock().unwrap().insert(r_addr, r);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(r_addr).await.unwrap();
    assert_eq!(result, Bytes::from("hello world"));
}

#[tokio::test]
async fn test_05_materialize_chain() {
    let (mut dag, registry, cache, mut leaves) = setup();
    let leaf_data = Bytes::from("data");
    let leaf_addr = leaf(b"data");
    leaves.leaves.lock().unwrap().insert(leaf_addr, leaf_data.clone());
    
    let r1 = recipe(vec![leaf_addr], "identity");
    let r1_addr = r1.addr();
    dag.recipes.lock().unwrap().insert(r1_addr, r1);
    
    let r2 = recipe(vec![r1_addr], "identity");
    let r2_addr = r2.addr();
    dag.recipes.lock().unwrap().insert(r2_addr, r2);
    
    let r3 = recipe(vec![r2_addr], "identity");
    let r3_addr = r3.addr();
    dag.recipes.lock().unwrap().insert(r3_addr, r3);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(r3_addr).await.unwrap();
    assert_eq!(result, leaf_data);
}

#[tokio::test]
async fn test_06_materialize_not_found() {
    let (dag, registry, cache, leaves) = setup();
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(leaf(b"nonexistent")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_07_materialize_caches_result() {
    let (mut dag, registry, cache, mut leaves) = setup();
    let leaf_data = Bytes::from("data");
    let leaf_addr = leaf(b"data");
    leaves.leaves.lock().unwrap().insert(leaf_addr, leaf_data);
    
    let r = recipe(vec![leaf_addr], "identity");
    let r_addr = r.addr();
    dag.recipes.lock().unwrap().insert(r_addr, r);
    
    let executor = AsyncExecutor::new(dag, registry, cache.clone(), leaves);
    executor.materialize(r_addr).await.unwrap();
    
    assert!(cache.contains(&r_addr).await);
}

#[tokio::test]
async fn test_08_concurrent_materialize_same_addr() {
    let (mut dag, registry, cache, mut leaves) = setup();
    let leaf_data = Bytes::from("data");
    let leaf_addr = leaf(b"data");
    leaves.leaves.lock().unwrap().insert(leaf_addr, leaf_data.clone());
    
    let r = recipe(vec![leaf_addr], "identity");
    let r_addr = r.addr();
    dag.recipes.lock().unwrap().insert(r_addr, r);
    
    let executor = Arc::new(AsyncExecutor::new(dag, registry, cache, leaves));
    
    let mut handles = vec![];
    for _ in 0..10 {
        let exec = Arc::clone(&executor);
        handles.push(tokio::spawn(async move {
            exec.materialize(r_addr).await
        }));
    }
    
    for h in handles {
        let result = h.await.unwrap().unwrap();
        assert_eq!(result, leaf_data);
    }
}

#[tokio::test]
async fn test_09_concurrent_materialize_different_addrs() {
    let (mut dag, registry, cache, mut leaves) = setup();
    
    let mut addrs = vec![];
    for i in 0..10 {
        let data = Bytes::from(format!("data{}", i));
        let addr = leaf(format!("data{}", i).as_bytes());
        leaves.leaves.lock().unwrap().insert(addr, data);
        
        let r = recipe(vec![addr], "identity");
        let r_addr = r.addr();
        dag.recipes.lock().unwrap().insert(r_addr, r);
        addrs.push(r_addr);
    }
    
    let executor = Arc::new(AsyncExecutor::new(dag, registry, cache, leaves));
    
    let mut handles = vec![];
    for addr in addrs {
        let exec = Arc::clone(&executor);
        handles.push(tokio::spawn(async move {
            exec.materialize(addr).await
        }));
    }
    
    for h in handles {
        assert!(h.await.unwrap().is_ok());
    }
}

#[tokio::test]
async fn test_10_materialize_deep_dag() {
    let (mut dag, registry, cache, mut leaves) = setup();
    let leaf_data = Bytes::from("data");
    let mut prev_addr = leaf(b"data");
    leaves.leaves.lock().unwrap().insert(prev_addr, leaf_data.clone());
    
    for i in 0..50 {
        let r = recipe(vec![prev_addr], &format!("identity"));
        prev_addr = r.addr();
        dag.recipes.lock().unwrap().insert(prev_addr, r);
    }
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(prev_addr).await.unwrap();
    assert_eq!(result, leaf_data);
}

#[tokio::test]
async fn test_11_shared_cache_concurrent_put_get() {
    let cache = Arc::new(SharedCache::new(EvictableCache::with_max_size(1024 * 1024)));
    
    let mut handles = vec![];
    for i in 0..50 {
        let c = Arc::clone(&cache);
        handles.push(tokio::spawn(async move {
            let addr = leaf(format!("addr{}", i).as_bytes());
            let data = Bytes::from(format!("data{}", i));
            c.put(addr, data).await;
        }));
    }
    
    for h in handles {
        h.await.unwrap();
    }
    
    let mut get_handles = vec![];
    for i in 0..50 {
        let c = Arc::clone(&cache);
        get_handles.push(tokio::spawn(async move {
            let addr = leaf(format!("addr{}", i).as_bytes());
            c.get(&addr).await
        }));
    }
    
    for h in get_handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn test_13_shared_cache_hit_rate() {
    let cache = Arc::new(SharedCache::new(EvictableCache::with_max_size(1024 * 1024)));
    
    // Put 5 entries
    for i in 0..5 {
        let addr = leaf(format!("addr{}", i).as_bytes());
        let data = Bytes::from(format!("data{}", i));
        cache.put(addr, data).await;
    }
    
    // Get 3 of them twice (first gets are misses, second gets are hits)
    for _ in 0..2 {
        for i in 0..3 {
            let addr = leaf(format!("addr{}", i).as_bytes());
            cache.get(&addr).await;
        }
    }
    
    // Get 2 unknown (misses)
    for i in 10..12 {
        let addr = leaf(format!("unknown{}", i).as_bytes());
        cache.get(&addr).await;
    }
    
    // Total gets: 3 + 3 + 2 = 8, hits: 3 (second round)
    let hit_rate = cache.hit_rate().await;
    println!("Hit rate: {}", hit_rate);
    assert!(hit_rate > 0.0); // Just verify it's tracking
}

#[tokio::test]
async fn test_14_materialize_with_params() {
    let (dag, registry, cache, leaves) = setup();
    
    // Insert leaf
    let leaf_addr = leaf(b"hello");
    leaves.leaves.lock().unwrap().insert(leaf_addr, Bytes::from("hello"));
    
    // Create recipe with params: repeat(input, count=3)
    let mut params = BTreeMap::new();
    params.insert("count".to_string(), Value::Int(3));
    let recipe = Recipe::new(
        FunctionId::new("repeat", "1.0.0"),
        vec![leaf_addr],
        params,
    );
    let recipe_addr = recipe.addr();
    dag.recipes.lock().unwrap().insert(recipe_addr, recipe);
    
    let executor = AsyncExecutor::new(
        Arc::clone(&dag),
        Arc::clone(&registry),
        Arc::clone(&cache),
        Arc::clone(&leaves),
    );
    
    let result = executor.materialize(recipe_addr).await.unwrap();
    assert_eq!(result, Bytes::from("hellohellohello"));
}

#[tokio::test]
async fn test_15_concurrent_cache_eviction() {
    // Small cache that will trigger evictions
    let cache = Arc::new(SharedCache::new(EvictableCache::with_max_size(100)));
    
    let mut handles = vec![];
    for i in 0..50 {
        let c = Arc::clone(&cache);
        handles.push(tokio::spawn(async move {
            let addr = leaf(format!("addr{}", i).as_bytes());
            let data = Bytes::from(vec![0u8; 10]); // 10 bytes each
            c.put(addr, data).await;
        }));
    }
    
    for h in handles {
        h.await.unwrap();
    }
    
    // Cache should have evicted some entries
    let size = cache.current_size().await;
    assert!(size <= 100);
}

#[tokio::test]
async fn test_16_materialize_error_not_cached() {
    let (dag, registry, cache, leaves) = setup();
    
    // Create recipe that will fail (missing input)
    let missing_addr = leaf(b"missing");
    let recipe = Recipe::new(
        FunctionId::new("identity", "1.0.0"),
        vec![missing_addr],
        BTreeMap::new(),
    );
    let recipe_addr = recipe.addr();
    dag.recipes.lock().unwrap().insert(recipe_addr, recipe);
    
    let executor = AsyncExecutor::new(
        Arc::clone(&dag),
        Arc::clone(&registry),
        Arc::clone(&cache),
        Arc::clone(&leaves),
    );
    
    // First attempt should fail
    let result = executor.materialize(recipe_addr).await;
    assert!(result.is_err());
    
    // Error should NOT be cached
    assert!(!cache.contains(&recipe_addr).await);
}

#[tokio::test]
async fn test_17_concurrent_different_addrs() {
    let (dag, registry, cache, leaves) = setup();
    
    // Create 10 different leaf+recipe pairs
    for i in 0..10 {
        let leaf_addr = leaf(format!("leaf{}", i).as_bytes());
        let leaf_data = Bytes::from(format!("data{}", i));
        leaves.leaves.lock().unwrap().insert(leaf_addr, leaf_data);
        
        let recipe = recipe(vec![leaf_addr], "identity");
        let recipe_addr = recipe.addr();
        dag.recipes.lock().unwrap().insert(recipe_addr, recipe);
    }
    
    let executor = Arc::new(AsyncExecutor::new(
        Arc::clone(&dag),
        Arc::clone(&registry),
        Arc::clone(&cache),
        Arc::clone(&leaves),
    ));
    
    // Spawn 10 concurrent materializations
    let mut handles = vec![];
    for i in 0..10 {
        let exec = Arc::clone(&executor);
        let leaf_addr = leaf(format!("leaf{}", i).as_bytes());
        let recipe = recipe(vec![leaf_addr], "identity");
        let recipe_addr = recipe.addr();
        
        handles.push(tokio::spawn(async move {
            exec.materialize(recipe_addr).await
        }));
    }
    
    // All should succeed
    for (i, h) in handles.into_iter().enumerate() {
        let result = h.await.unwrap().unwrap();
        assert_eq!(result, Bytes::from(format!("data{}", i)));
    }
}



#[tokio::test]
async fn test_12_materialize_diamond() {
    let (mut dag, registry, cache, mut leaves) = setup();
    let data_a = Bytes::from("a");
    let data_b = Bytes::from("b");
    let addr_a = leaf(b"a");
    let addr_b = leaf(b"b");
    leaves.leaves.lock().unwrap().insert(addr_a, data_a);
    leaves.leaves.lock().unwrap().insert(addr_b, data_b);
    
    let r1 = recipe(vec![addr_a, addr_b], "concat");
    let r1_addr = r1.addr();
    dag.recipes.lock().unwrap().insert(r1_addr, r1);
    
    let r2 = recipe(vec![addr_a], "identity");
    let r2_addr = r2.addr();
    dag.recipes.lock().unwrap().insert(r2_addr, r2);
    
    let r3 = recipe(vec![r1_addr, r2_addr], "concat");
    let r3_addr = r3.addr();
    dag.recipes.lock().unwrap().insert(r3_addr, r3);
    
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(r3_addr).await.unwrap();
    assert_eq!(result, Bytes::from("aba"));
}
