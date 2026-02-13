use ::deriva_compute::builtins;
use ::deriva_compute::registry::FunctionRegistry;
use ::deriva_server::service::proto::deriva_server::Deriva;
use ::deriva_server::service::proto::*;
use ::deriva_server::service::DerivaService;
use ::deriva_server::state::ServerState;
use ::deriva_storage::StorageBackend;
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
