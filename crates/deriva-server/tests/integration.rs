use ::deriva_compute::builtins;
use ::deriva_compute::registry::FunctionRegistry;
use ::deriva_server::service::proto::deriva_server::Deriva;
use ::deriva_server::service::proto::*;
use ::deriva_server::service::DerivaService;
use ::deriva_server::state::ServerState;
use ::deriva_storage::StorageBackend;
use futures::FutureExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tokio_stream::StreamExt;
use tonic::Request;

struct TestHarness {
    svc: Option<DerivaService>,
    _dir: TempDir,
    root: PathBuf,
}

impl TestHarness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let svc = Some(build_service(&root));
        Self { svc, _dir: dir, root }
    }

    fn svc(&self) -> &DerivaService {
        self.svc.as_ref().unwrap()
    }

    fn restart(&mut self) {
        // Drop old service (and its Arc<ServerState>) to release sled lock
        self.svc = None;
        self.svc = Some(build_service(&self.root));
    }
}

fn build_service(root: &Path) -> DerivaService {
    let storage = StorageBackend::open(root).unwrap();
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    let state = Arc::new(ServerState::new(storage, registry).unwrap());
    DerivaService::new(state)
}

async fn put_leaf(svc: &DerivaService, data: &[u8]) -> Vec<u8> {
    Deriva::put_leaf(svc, Request::new(PutLeafRequest { data: data.to_vec() }))
        .await
        .unwrap()
        .into_inner()
        .addr
}

async fn put_recipe(
    svc: &DerivaService,
    name: &str,
    ver: &str,
    inputs: Vec<Vec<u8>>,
    params: std::collections::HashMap<String, String>,
) -> Vec<u8> {
    Deriva::put_recipe(
        svc,
        Request::new(PutRecipeRequest {
            function_name: name.into(),
            function_version: ver.into(),
            inputs,
            params,
        }),
    )
    .await
    .unwrap()
    .into_inner()
    .addr
}

async fn get_all(svc: &DerivaService, addr: &[u8]) -> Vec<u8> {
    let mut stream = Deriva::get(svc, Request::new(GetRequest { addr: addr.to_vec() }))
        .await
        .unwrap()
        .into_inner();
    let mut result = Vec::new();
    while let Some(chunk) = stream.next().await {
        result.extend_from_slice(&chunk.unwrap().chunk);
    }
    result
}

async fn status(svc: &DerivaService) -> StatusResponse {
    Deriva::status(svc, Request::new(StatusRequest {}))
        .await
        .unwrap()
        .into_inner()
}

// === Scenario A: Single Leaf Roundtrip ===

#[tokio::test]
async fn leaf_put_and_get() {
    let h = TestHarness::new();
    let addr = put_leaf(h.svc(), b"hello world").await;
    assert_eq!(get_all(h.svc(), &addr).await, b"hello world");
}

#[tokio::test]
async fn leaf_deterministic_addr() {
    let h = TestHarness::new();
    let a1 = put_leaf(h.svc(), b"same").await;
    let a2 = put_leaf(h.svc(), b"same").await;
    assert_eq!(a1, a2);
}

#[tokio::test]
async fn leaf_different_data_different_addr() {
    let h = TestHarness::new();
    assert_ne!(
        put_leaf(h.svc(), b"aaa").await,
        put_leaf(h.svc(), b"bbb").await
    );
}

// === Scenario B: Single Derived Value ===

#[tokio::test]
async fn derive_uppercase() {
    let h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"hello").await;
    let recipe = put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf], Default::default()).await;
    assert_eq!(get_all(h.svc(), &recipe).await, b"HELLO");
}

#[tokio::test]
async fn derive_identity() {
    let h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"passthrough").await;
    let recipe = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
    assert_eq!(get_all(h.svc(), &recipe).await, b"passthrough");
}

#[tokio::test]
async fn derive_concat() {
    let h = TestHarness::new();
    let a = put_leaf(h.svc(), b"foo").await;
    let b = put_leaf(h.svc(), b"bar").await;
    let recipe = put_recipe(h.svc(), "concat", "1.0.0", vec![a, b], Default::default()).await;
    assert_eq!(get_all(h.svc(), &recipe).await, b"foobar");
}

#[tokio::test]
async fn derive_repeat() {
    let h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"ab").await;
    let mut params = std::collections::HashMap::new();
    params.insert("count".into(), "3".into());
    let recipe = put_recipe(h.svc(), "repeat", "1.0.0", vec![leaf], params).await;
    assert_eq!(get_all(h.svc(), &recipe).await, b"ababab");
}

// === Scenario C: Multi-Level DAG ===

#[tokio::test]
async fn three_level_chain() {
    let h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"hello").await;
    let s1 = put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf], Default::default()).await;
    let s2 = put_recipe(h.svc(), "identity", "1.0.0", vec![s1], Default::default()).await;
    assert_eq!(get_all(h.svc(), &s2).await, b"HELLO");
}

#[tokio::test]
async fn four_level_chain() {
    let h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"deep").await;
    let s1 = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
    let s2 = put_recipe(h.svc(), "uppercase", "1.0.0", vec![s1], Default::default()).await;
    let s3 = put_recipe(h.svc(), "identity", "1.0.0", vec![s2], Default::default()).await;
    assert_eq!(get_all(h.svc(), &s3).await, b"DEEP");
}

// === Scenario D: Diamond Dependency ===

#[tokio::test]
async fn diamond_dag() {
    let h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"x").await;
    let upper =
        put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf.clone()], Default::default()).await;
    let ident = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
    let merged = put_recipe(h.svc(), "concat", "1.0.0", vec![upper, ident], Default::default()).await;
    assert_eq!(get_all(h.svc(), &merged).await, b"Xx");
}

// === Scenario E: Cache Hit on Second Get ===

#[tokio::test]
async fn second_get_hits_cache() {
    let h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"cached").await;
    let recipe = put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf], Default::default()).await;

    let d1 = get_all(h.svc(), &recipe).await;
    let s1 = status(h.svc()).await;

    let d2 = get_all(h.svc(), &recipe).await;
    let s2 = status(h.svc()).await;

    assert_eq!(d1, b"CACHED");
    assert_eq!(d1, d2);
    assert!(s2.cache_hit_rate >= s1.cache_hit_rate);
}

// === Scenario F: Invalidate + Recompute ===

#[tokio::test]
async fn invalidate_then_recompute() {
    let h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"data").await;
    let recipe = put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf], Default::default()).await;

    assert_eq!(get_all(h.svc(), &recipe).await, b"DATA");

    let inv = Deriva::invalidate(h.svc(), Request::new(InvalidateRequest { addr: recipe.clone() }))
        .await
        .unwrap()
        .into_inner();
    assert!(inv.was_cached);

    let inv2 =
        Deriva::invalidate(h.svc(), Request::new(InvalidateRequest { addr: recipe.clone() }))
            .await
            .unwrap()
            .into_inner();
    assert!(!inv2.was_cached);

    assert_eq!(get_all(h.svc(), &recipe).await, b"DATA");
}

// === Scenario G: Persistence Across Restart ===

#[tokio::test]
async fn recipes_survive_restart() {
    let mut h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"persist me").await;
    let recipe =
        put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf.clone()], Default::default()).await;

    h.restart();

    let resp =
        Deriva::resolve(h.svc(), Request::new(ResolveRequest { addr: recipe.clone() }))
            .await
            .unwrap()
            .into_inner();
    assert!(resp.found);
    assert_eq!(resp.function_name, "uppercase");

    assert_eq!(get_all(h.svc(), &leaf).await, b"persist me");
    assert_eq!(get_all(h.svc(), &recipe).await, b"PERSIST ME");
}

#[tokio::test]
async fn dag_structure_survives_restart() {
    let mut h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"a").await;
    let s1 = put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf], Default::default()).await;
    let s2 = put_recipe(h.svc(), "identity", "1.0.0", vec![s1], Default::default()).await;

    h.restart();

    assert_eq!(get_all(h.svc(), &s2).await, b"A");
    assert_eq!(status(h.svc()).await.recipe_count, 2);
}

#[tokio::test]
async fn cache_cold_after_restart() {
    let mut h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"x").await;
    let recipe = put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf], Default::default()).await;

    get_all(h.svc(), &recipe).await;
    assert!(status(h.svc()).await.cache_entries > 0);

    h.restart();
    assert_eq!(status(h.svc()).await.cache_entries, 0);
}

// === Scenario H: Large Blob Streaming ===

#[tokio::test]
async fn large_leaf_streams_correctly() {
    let h = TestHarness::new();
    let big = vec![0x42; 500_000];
    let addr = put_leaf(h.svc(), &big).await;
    let data = get_all(h.svc(), &addr).await;
    assert_eq!(data.len(), 500_000);
    assert!(data.iter().all(|&b| b == 0x42));
}

#[tokio::test]
async fn large_derived_streams_correctly() {
    let h = TestHarness::new();
    let big = vec![b'a'; 300_000];
    let addr = put_leaf(h.svc(), &big).await;
    let recipe = put_recipe(h.svc(), "uppercase", "1.0.0", vec![addr], Default::default()).await;
    let data = get_all(h.svc(), &recipe).await;
    assert_eq!(data.len(), 300_000);
    assert!(data.iter().all(|&b| b == b'A'));
}

// === Scenario I: Error Cases ===

#[tokio::test]
async fn get_nonexistent_addr() {
    let h = TestHarness::new();
    let mut stream = Deriva::get(h.svc(), Request::new(GetRequest { addr: vec![0xFF; 32] }))
        .await
        .unwrap()
        .into_inner();
    assert!(stream.next().await.unwrap().is_err());
}

#[tokio::test]
async fn recipe_with_missing_function() {
    let h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"x").await;
    let recipe =
        put_recipe(h.svc(), "nonexistent_fn", "1.0.0", vec![leaf], Default::default()).await;
    let mut stream = Deriva::get(h.svc(), Request::new(GetRequest { addr: recipe }))
        .await
        .unwrap()
        .into_inner();
    assert!(stream.next().await.unwrap().is_err());
}

#[tokio::test]
async fn bad_addr_length_rejected() {
    let h = TestHarness::new();
    assert!(Deriva::get(h.svc(), Request::new(GetRequest { addr: vec![0; 5] }))
        .await
        .is_err());
}

#[tokio::test]
async fn resolve_nonexistent() {
    let h = TestHarness::new();
    let resp = Deriva::resolve(h.svc(), Request::new(ResolveRequest { addr: vec![0; 32] }))
        .await
        .unwrap()
        .into_inner();
    assert!(!resp.found);
}

// === Scenario J: Concurrent Operations ===

#[tokio::test]
async fn concurrent_puts() {
    let h = TestHarness::new();
    let data: Vec<Vec<u8>> = (0..20).map(|i| format!("concurrent_{}", i).into_bytes()).collect();
    let mut handles = Vec::new();
    for d in &data {
        handles.push(put_leaf(h.svc(), d));
    }
    let addrs: Vec<Vec<u8>> = futures::future::join_all(handles).await;
    let unique: std::collections::HashSet<Vec<u8>> = addrs.into_iter().collect();
    assert_eq!(unique.len(), 20);
}

#[tokio::test]
async fn concurrent_gets_same_addr() {
    let h = TestHarness::new();
    let leaf = put_leaf(h.svc(), b"shared").await;
    let recipe = put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf], Default::default()).await;

    let mut handles = Vec::new();
    for _ in 0..10 {
        handles.push(get_all(h.svc(), &recipe));
    }
    let results: Vec<Vec<u8>> = futures::future::join_all(handles).await;
    for r in &results {
        assert_eq!(r, b"SHARED");
    }
}

// --- Phase 2.2 Integration Tests ---

#[tokio::test]
async fn test_async_get_rpc_concurrent() {
    let h = TestHarness::new();
    
    // Put 5 different leaves + recipes
    let mut addrs = Vec::new();
    for i in 0..5 {
        let data = format!("data{}", i);
        let leaf = put_leaf(h.svc(), data.as_bytes()).await;
        let recipe = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
        addrs.push((recipe, data));
    }
    
    // Spawn 5 concurrent Get requests using join_all (no spawn needed)
    let start = std::time::Instant::now();
    let mut handles = Vec::new();
    for (addr, expected) in &addrs {
        handles.push(async {
            let result = get_all(h.svc(), addr).await;
            (result, expected.clone())
        });
    }
    let results = futures::future::join_all(handles).await;
    let elapsed = start.elapsed();
    
    // Verify all results
    for (result, expected) in results {
        assert_eq!(result, expected.as_bytes());
    }
    
    println!("5 concurrent Gets completed in {:?}", elapsed);
}

#[tokio::test]
async fn test_async_get_rpc_during_put() {
    let h = TestHarness::new();
    
    // Create initial recipe
    let leaf1 = put_leaf(h.svc(), b"initial").await;
    let recipe1 = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf1], Default::default()).await;
    
    // Execute Get and Put concurrently using join
    let get_fut = get_all(h.svc(), &recipe1);
    let put_fut = async {
        let leaf2 = put_leaf(h.svc(), b"concurrent").await;
        put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf2], Default::default()).await
    };
    
    let (result, _) = tokio::join!(get_fut, put_fut);
    assert_eq!(result, b"initial");
}

#[tokio::test]
async fn test_async_invalidate_during_get() {
    let h = TestHarness::new();
    
    // Put leaf + recipe, materialize once (cached)
    let leaf = put_leaf(h.svc(), b"cached").await;
    let recipe = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
    let _ = get_all(h.svc(), &recipe).await; // Prime cache
    
    // Execute Get and Invalidate concurrently
    let get_fut = get_all(h.svc(), &recipe);
    let inv_fut = async {
        Deriva::invalidate(h.svc(), Request::new(InvalidateRequest { addr: recipe.clone() }))
            .await
            .unwrap()
    };
    
    let (result, _) = tokio::join!(get_fut, inv_fut);
    assert_eq!(result, b"cached");
}

#[tokio::test]
async fn test_async_status_during_heavy_load() {
    let h = TestHarness::new();
    
    // Put 20 recipes
    let mut addrs = Vec::new();
    for i in 0..20 {
        let data = format!("load{}", i);
        let leaf = put_leaf(h.svc(), data.as_bytes()).await;
        let recipe = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
        addrs.push(recipe);
    }
    
    // Execute 10 Gets and 5 Status calls concurrently
    let get_futs: Vec<_> = addrs[..10].iter().map(|addr| get_all(h.svc(), addr)).collect();
    let status_futs: Vec<_> = (0..5).map(|_| status(h.svc())).collect();
    
    // Run all concurrently
    let (get_results, status_results) = tokio::join!(
        futures::future::join_all(get_futs),
        futures::future::join_all(status_futs)
    );
    
    // Verify Gets succeeded
    assert_eq!(get_results.len(), 10);
    
    // Verify Status calls returned valid counts
    assert_eq!(status_results.len(), 5);
    for resp in status_results {
        assert!(resp.recipe_count >= 20);
    }
}

#[tokio::test]
async fn test_async_concurrent_resolve() {
    let h = TestHarness::new();
    
    // Put 10 recipes
    let mut addrs = Vec::new();
    for i in 0..10 {
        let leaf = put_leaf(h.svc(), format!("data{}", i).as_bytes()).await;
        let recipe = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
        addrs.push(recipe);
    }
    
    // Resolve all concurrently
    let resolve_futs: Vec<_> = addrs.iter().map(|addr| async {
        Deriva::resolve(h.svc(), Request::new(ResolveRequest { addr: addr.clone() }))
            .await
            .unwrap()
            .into_inner()
    }).collect();
    
    let results = futures::future::join_all(resolve_futs).await;
    
    // All should be found
    for resp in results {
        assert!(resp.found);
        assert_eq!(resp.function_name, "identity");
    }
}

#[tokio::test]
async fn test_async_put_recipe_during_get() {
    let h = TestHarness::new();
    
    // Put initial recipe
    let leaf1 = put_leaf(h.svc(), b"initial").await;
    let recipe1 = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf1], Default::default()).await;
    
    // Concurrent Get and PutRecipe
    let get_fut = get_all(h.svc(), &recipe1);
    let put_fut = async {
        let leaf2 = put_leaf(h.svc(), b"concurrent").await;
        put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf2], Default::default()).await
    };
    
    let (result, new_recipe) = tokio::join!(get_fut, put_fut);
    
    assert_eq!(result, b"initial");
    assert_eq!(new_recipe.len(), 32);
}

#[tokio::test]
async fn test_async_multiple_invalidations() {
    let h = TestHarness::new();
    
    // Put and cache 5 recipes
    let mut addrs = Vec::new();
    for i in 0..5 {
        let leaf = put_leaf(h.svc(), format!("data{}", i).as_bytes()).await;
        let recipe = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
        let _ = get_all(h.svc(), &recipe).await; // Cache it
        addrs.push(recipe);
    }
    
    // Invalidate all concurrently
    let inv_futs: Vec<_> = addrs.iter().map(|addr| async {
        Deriva::invalidate(h.svc(), Request::new(InvalidateRequest { addr: addr.clone() }))
            .await
            .unwrap()
            .into_inner()
    }).collect();
    
    let results = futures::future::join_all(inv_futs).await;
    
    // All should have been cached
    for resp in results {
        assert!(resp.was_cached);
    }
}

#[tokio::test]
async fn test_async_get_chain_concurrent() {
    let h = TestHarness::new();
    
    // Build chain: leaf -> r1 -> r2 -> r3
    let leaf = put_leaf(h.svc(), b"chain").await;
    let r1 = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf.clone()], Default::default()).await;
    let r2 = put_recipe(h.svc(), "identity", "1.0.0", vec![r1.clone()], Default::default()).await;
    let r3 = put_recipe(h.svc(), "identity", "1.0.0", vec![r2.clone()], Default::default()).await;
    
    // Get all levels concurrently
    let futs = vec![
        get_all(h.svc(), &leaf),
        get_all(h.svc(), &r1),
        get_all(h.svc(), &r2),
        get_all(h.svc(), &r3),
    ];
    
    let results = futures::future::join_all(futs).await;
    
    // All should return same data
    for result in results {
        assert_eq!(result, b"chain");
    }
}

#[tokio::test]
async fn test_async_status_consistency() {
    let h = TestHarness::new();
    
    // Initial status
    let s1 = status(h.svc()).await;
    assert_eq!(s1.recipe_count, 0);
    
    // Add recipes
    for i in 0..10 {
        let leaf = put_leaf(h.svc(), format!("data{}", i).as_bytes()).await;
        put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
    }
    
    // Status should reflect additions
    let s2 = status(h.svc()).await;
    assert_eq!(s2.recipe_count, 10);
    
    // Cache some
    for i in 0..5 {
        let leaf = put_leaf(h.svc(), format!("data{}", i).as_bytes()).await;
        let recipe = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
        get_all(h.svc(), &recipe).await;
    }
    
    let s3 = status(h.svc()).await;
    assert!(s3.cache_entries > 0);
}

#[tokio::test]
async fn test_async_concurrent_diamond_materialization() {
    let h = TestHarness::new();
    
    // Build diamond: a,b -> r1(concat), a -> r2(identity), r1,r2 -> r3(concat)
    let a = put_leaf(h.svc(), b"a").await;
    let b = put_leaf(h.svc(), b"b").await;
    let r1 = put_recipe(h.svc(), "concat", "1.0.0", vec![a.clone(), b], Default::default()).await;
    let r2 = put_recipe(h.svc(), "identity", "1.0.0", vec![a], Default::default()).await;
    let r3 = put_recipe(h.svc(), "concat", "1.0.0", vec![r1, r2], Default::default()).await;
    
    // Materialize r3 multiple times concurrently
    let futs: Vec<_> = (0..5).map(|_| get_all(h.svc(), &r3)).collect();
    let results = futures::future::join_all(futs).await;
    
    // All should return "aba"
    for result in results {
        assert_eq!(result, b"aba");
    }
}

#[tokio::test]
async fn test_async_interleaved_operations() {
    let h = TestHarness::new();
    
    // Interleave puts, gets, resolves, and status calls
    let leaf = put_leaf(h.svc(), b"test").await;
    
    let futs = vec![
        async { put_recipe(h.svc(), "identity", "1.0.0", vec![leaf.clone()], Default::default()).await; 0 }.boxed(),
        async { status(h.svc()).await; 1 }.boxed(),
        async { put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf.clone()], Default::default()).await; 2 }.boxed(),
        async { get_all(h.svc(), &leaf).await; 3 }.boxed(),
        async { status(h.svc()).await; 4 }.boxed(),
    ];
    
    let results = futures::future::join_all(futs).await;
    assert_eq!(results.len(), 5);
}

// ============================================================================
// PARALLEL MATERIALIZATION INTEGRATION TESTS
// ============================================================================

#[tokio::test]
async fn test_parallel_get_rpc_fan_in() {
    use std::time::Instant;
    
    let h = TestHarness::new();
    
    // Create 4 leaves
    let mut leaves = vec![];
    for i in 0..4 {
        leaves.push(put_leaf(h.svc(), &[i]).await);
    }
    
    // Create fan-in recipe
    let recipe = put_recipe(h.svc(), "concat", "1.0.0", leaves, Default::default()).await;
    
    // Measure parallel execution time
    let start = Instant::now();
    let result = get_all(h.svc(), &recipe).await;
    let elapsed = start.elapsed();
    
    assert_eq!(result, vec![0, 1, 2, 3]);
    // Should be fast (parallel execution)
    assert!(elapsed.as_millis() < 500);
}

#[tokio::test]
async fn test_parallel_multiple_clients() {
    let h = TestHarness::new();
    
    let leaf = put_leaf(h.svc(), b"data").await;
    let recipe = put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf], Default::default()).await;
    
    // 5 concurrent clients requesting same recipe
    let futs: Vec<_> = (0..5).map(|_| get_all(h.svc(), &recipe)).collect();
    let results = futures::future::join_all(futs).await;
    
    // All should get same result
    for result in results {
        assert_eq!(result, b"DATA");
    }
}

#[tokio::test]
async fn test_parallel_diamond_dag_rpc() {
    let h = TestHarness::new();
    
    let root = put_leaf(h.svc(), b"root").await;
    
    // Two branches from root
    let branch1 = put_recipe(h.svc(), "uppercase", "1.0.0", vec![root.clone()], Default::default()).await;
    let branch2 = put_recipe(h.svc(), "identity", "1.0.0", vec![root.clone()], Default::default()).await;
    
    // Merge branches
    let merge = put_recipe(h.svc(), "concat", "1.0.0", vec![branch1, branch2], Default::default()).await;
    
    let result = get_all(h.svc(), &merge).await;
    assert_eq!(result, b"ROOTroot");
}

#[tokio::test]
async fn test_parallel_dedup_across_clients() {
    let h = TestHarness::new();
    
    let leaf = put_leaf(h.svc(), b"shared").await;
    let recipe = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
    
    // 10 concurrent clients requesting same address
    let futs: Vec<_> = (0..10).map(|_| get_all(h.svc(), &recipe)).collect();
    let results = futures::future::join_all(futs).await;
    
    // All should succeed with same result
    for result in results {
        assert_eq!(result, b"shared");
    }
}

#[tokio::test]
async fn test_parallel_wide_fan_in_rpc() {
    let h = TestHarness::new();
    
    // Create 16 leaves
    let mut leaves = vec![];
    for i in 0..16u8 {
        leaves.push(put_leaf(h.svc(), &[i]).await);
    }
    
    // Create wide fan-in
    let recipe = put_recipe(h.svc(), "concat", "1.0.0", leaves, Default::default()).await;
    
    let result = get_all(h.svc(), &recipe).await;
    assert_eq!(result, (0..16u8).collect::<Vec<_>>());
}

#[tokio::test]
async fn test_parallel_error_propagation_rpc() {
    let h = TestHarness::new();
    
    let valid = put_leaf(h.svc(), b"valid").await;
    let invalid = vec![0u8; 32]; // Non-existent address
    
    // Recipe with missing input
    let recipe = put_recipe(h.svc(), "concat", "1.0.0", vec![valid, invalid], Default::default()).await;
    
    // Should fail gracefully
    let mut stream = Deriva::get(h.svc(), Request::new(GetRequest { addr: recipe }))
        .await
        .unwrap()
        .into_inner();
    
    let result = stream.next().await;
    assert!(result.is_some());
    assert!(result.unwrap().is_err());
}

#[tokio::test]
async fn test_parallel_cache_effectiveness_rpc() {
    let h = TestHarness::new();
    
    let leaf = put_leaf(h.svc(), b"data").await;
    let recipe = put_recipe(h.svc(), "uppercase", "1.0.0", vec![leaf], Default::default()).await;
    
    // First call - computes
    let result1 = get_all(h.svc(), &recipe).await;
    
    // Second call - should hit cache
    let start = std::time::Instant::now();
    let result2 = get_all(h.svc(), &recipe).await;
    let elapsed = start.elapsed();
    
    assert_eq!(result1, result2);
    assert_eq!(result1, b"DATA");
    // Cache hit should be very fast
    assert!(elapsed.as_millis() < 50);
}

#[tokio::test]
async fn test_parallel_stress_concurrent_recipes() {
    let h = TestHarness::new();
    
    // Create 20 different recipes
    let mut recipe_addrs = vec![];
    for i in 0..20u8 {
        let leaf = put_leaf(h.svc(), &[i]).await;
        let recipe = put_recipe(h.svc(), "identity", "1.0.0", vec![leaf], Default::default()).await;
        recipe_addrs.push((recipe, i));
    }
    
    // Materialize all concurrently
    let svc = h.svc();
    let futs: Vec<_> = recipe_addrs.iter()
        .map(|(addr, expected)| {
            let addr = addr.clone();
            let expected = *expected;
            async move {
                let result = get_all(svc, &addr).await;
                (result, expected)
            }
        })
        .collect();
    
    let results = futures::future::join_all(futs).await;
    
    // Verify all results
    for (result, expected) in results {
        assert_eq!(result, vec![expected]);
    }
}

// Test 43: Server starts with --verification dual flag
#[tokio::test]
async fn test_verification_mode_server_flag() {
    use deriva_compute::async_executor::VerificationMode;
    
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageBackend::open(dir.path()).unwrap();
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    
    let state = Arc::new(ServerState::with_verification(
        storage,
        registry,
        VerificationMode::DualCompute,
    ).unwrap());
    
    let svc = DerivaService::new(state);
    let resp = Deriva::status(&svc, Request::new(StatusRequest {})).await.unwrap().into_inner();
    
    assert_eq!(resp.verification_mode, "dual");
}

// Test 44: Verification mode rejects non-deterministic function
#[tokio::test]
async fn test_verification_mode_rejects_nondeterministic() {
    use deriva_compute::async_executor::VerificationMode;
    use deriva_compute::{ComputeFunction, ComputeCost, ComputeError};
    use deriva_core::address::{FunctionId, Value, CAddr, Recipe};
    use bytes::Bytes;
    use std::collections::BTreeMap;
    
    struct NonDeterministic;
    impl ComputeFunction for NonDeterministic {
        fn id(&self) -> FunctionId { 
            FunctionId { name: "nondeterministic".into(), version: "1".into() }
        }
        fn execute(&self, _inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
            Ok(Bytes::from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos().to_string()))
        }
        fn estimated_cost(&self, _input_sizes: &[u64]) -> ComputeCost {
            ComputeCost { cpu_ms: 1, memory_bytes: 1024 }
        }
    }
    
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageBackend::open(dir.path()).unwrap();
    let mut registry = FunctionRegistry::new();
    registry.register(Arc::new(NonDeterministic));
    
    let state = Arc::new(ServerState::with_verification(
        storage,
        registry,
        VerificationMode::DualCompute,
    ).unwrap());
    
    let leaf_data = Bytes::from("input");
    let leaf_addr = CAddr::from_bytes(&leaf_data);
    state.storage.put_leaf(&leaf_data).unwrap();
    
    let recipe = Recipe::new(
        FunctionId::new("nondeterministic", "1"),
        vec![leaf_addr],
        BTreeMap::new(),
    );
    let recipe_addr = state.storage.put_recipe(&recipe).unwrap();
    
    let result = state.executor.materialize(recipe_addr).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("determinism violation"));
}

