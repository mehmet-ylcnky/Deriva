use ::deriva_compute::builtins;
use ::deriva_compute::registry::FunctionRegistry;
use ::deriva_core::address::CAddr;
use ::deriva_server::service::proto::deriva_server::Deriva;
use ::deriva_server::service::proto::*;
use ::deriva_server::service::DerivaService;
use ::deriva_server::state::ServerState;
use ::deriva_storage::StorageBackend;
use std::sync::Arc;
use tempfile::tempdir;
use tokio_stream::StreamExt;
use tonic::Request;

fn setup() -> DerivaService {
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();
    std::mem::forget(dir); // keep tempdir alive
    let storage = StorageBackend::open(&path).unwrap();
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    let state = Arc::new(ServerState::new(storage, registry).unwrap());
    DerivaService::new(state)
}

// --- PutLeaf ---

#[tokio::test]
async fn put_leaf_returns_addr() {
    let svc = setup();
    let resp = Deriva::put_leaf(&svc, Request::new(PutLeafRequest {
        data: b"hello world".to_vec(),
    })).await.unwrap().into_inner();
    assert_eq!(resp.addr.len(), 32);
    assert_eq!(resp.addr, CAddr::from_bytes(b"hello world").as_bytes().to_vec());
}

#[tokio::test]
async fn put_leaf_deterministic() {
    let svc = setup();
    let r1 = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"same".to_vec() })).await.unwrap().into_inner();
    let r2 = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"same".to_vec() })).await.unwrap().into_inner();
    assert_eq!(r1.addr, r2.addr);
}

#[tokio::test]
async fn put_leaf_different_data_different_addr() {
    let svc = setup();
    let r1 = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"aaa".to_vec() })).await.unwrap().into_inner();
    let r2 = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"bbb".to_vec() })).await.unwrap().into_inner();
    assert_ne!(r1.addr, r2.addr);
}

#[tokio::test]
async fn put_leaf_empty() {
    let svc = setup();
    let resp = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: vec![] })).await.unwrap().into_inner();
    assert_eq!(resp.addr.len(), 32);
}

// --- PutRecipe ---

#[tokio::test]
async fn put_recipe_returns_addr() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"input".to_vec() })).await.unwrap().into_inner();
    let resp = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "identity".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr],
        params: Default::default(),
    })).await.unwrap().into_inner();
    assert_eq!(resp.addr.len(), 32);
}

#[tokio::test]
async fn put_recipe_deterministic() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"x".to_vec() })).await.unwrap().into_inner();
    let mk = || PutRecipeRequest {
        function_name: "identity".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr.clone()],
        params: Default::default(),
    };
    let r1 = Deriva::put_recipe(&svc, Request::new(mk())).await.unwrap().into_inner();
    let r2 = Deriva::put_recipe(&svc, Request::new(mk())).await.unwrap().into_inner();
    assert_eq!(r1.addr, r2.addr);
}

#[tokio::test]
async fn put_recipe_with_params() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"data".to_vec() })).await.unwrap().into_inner();
    let mut params = std::collections::HashMap::new();
    params.insert("count".into(), "3".into());
    let resp = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "repeat".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr],
        params,
    })).await.unwrap().into_inner();
    assert_eq!(resp.addr.len(), 32);
}

// --- Get (streaming) ---

async fn collect_stream(svc: &DerivaService, addr: Vec<u8>) -> Vec<u8> {
    let mut stream = Deriva::get(svc, Request::new(GetRequest { addr })).await.unwrap().into_inner();
    let mut result = Vec::new();
    while let Some(chunk) = stream.next().await {
        result.extend_from_slice(&chunk.unwrap().chunk);
    }
    result
}

#[tokio::test]
async fn get_leaf_data() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"raw bytes".to_vec() })).await.unwrap().into_inner();
    assert_eq!(collect_stream(&svc, leaf.addr).await, b"raw bytes");
}

#[tokio::test]
async fn get_derived_identity() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"hello".to_vec() })).await.unwrap().into_inner();
    let recipe = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "identity".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr],
        params: Default::default(),
    })).await.unwrap().into_inner();
    assert_eq!(collect_stream(&svc, recipe.addr).await, b"hello");
}

#[tokio::test]
async fn get_derived_uppercase() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"hello".to_vec() })).await.unwrap().into_inner();
    let recipe = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "uppercase".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr],
        params: Default::default(),
    })).await.unwrap().into_inner();
    assert_eq!(collect_stream(&svc, recipe.addr).await, b"HELLO");
}

#[tokio::test]
async fn get_chained_computation() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"chain".to_vec() })).await.unwrap().into_inner();
    let upper = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "uppercase".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr],
        params: Default::default(),
    })).await.unwrap().into_inner();
    let final_r = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "identity".into(),
        function_version: "1.0.0".into(),
        inputs: vec![upper.addr],
        params: Default::default(),
    })).await.unwrap().into_inner();
    assert_eq!(collect_stream(&svc, final_r.addr).await, b"CHAIN");
}

#[tokio::test]
async fn get_missing_addr_returns_error() {
    let svc = setup();
    let mut stream = Deriva::get(&svc, Request::new(GetRequest { addr: vec![0u8; 32] })).await.unwrap().into_inner();
    let msg = stream.next().await.unwrap();
    assert!(msg.is_err());
}

#[tokio::test]
async fn get_large_blob_streams_chunks() {
    let svc = setup();
    let data = vec![0xAB; 200_000];
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: data.clone() })).await.unwrap().into_inner();
    let mut stream = Deriva::get(&svc, Request::new(GetRequest { addr: leaf.addr })).await.unwrap().into_inner();
    let mut result = Vec::new();
    let mut chunk_count = 0;
    while let Some(chunk) = stream.next().await {
        result.extend_from_slice(&chunk.unwrap().chunk);
        chunk_count += 1;
    }
    assert_eq!(result, data);
    assert!(chunk_count >= 3, "should stream in multiple chunks");
}

// --- Resolve ---

#[tokio::test]
async fn resolve_existing_recipe() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"x".to_vec() })).await.unwrap().into_inner();
    let recipe = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "uppercase".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr.clone()],
        params: Default::default(),
    })).await.unwrap().into_inner();
    let resp = Deriva::resolve(&svc, Request::new(ResolveRequest { addr: recipe.addr })).await.unwrap().into_inner();
    assert!(resp.found);
    assert_eq!(resp.function_name, "uppercase");
    assert_eq!(resp.function_version, "1.0.0");
    assert_eq!(resp.inputs.len(), 1);
    assert_eq!(resp.inputs[0], leaf.addr);
}

#[tokio::test]
async fn resolve_missing() {
    let svc = setup();
    let resp = Deriva::resolve(&svc, Request::new(ResolveRequest { addr: vec![0u8; 32] })).await.unwrap().into_inner();
    assert!(!resp.found);
}

#[tokio::test]
async fn resolve_leaf_not_found() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"leaf".to_vec() })).await.unwrap().into_inner();
    let resp = Deriva::resolve(&svc, Request::new(ResolveRequest { addr: leaf.addr })).await.unwrap().into_inner();
    assert!(!resp.found);
}

// --- Invalidate ---

#[tokio::test]
async fn invalidate_uncached_returns_false() {
    let svc = setup();
    let resp = Deriva::invalidate(&svc, Request::new(InvalidateRequest { addr: vec![0u8; 32], cascade: false })).await.unwrap().into_inner();
    assert!(!resp.was_cached);
}

#[tokio::test]
async fn invalidate_after_get_returns_true() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"data".to_vec() })).await.unwrap().into_inner();
    let recipe = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "uppercase".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr],
        params: Default::default(),
    })).await.unwrap().into_inner();
    collect_stream(&svc, recipe.addr.clone()).await;
    let resp = Deriva::invalidate(&svc, Request::new(InvalidateRequest { addr: recipe.addr, cascade: false })).await.unwrap().into_inner();
    assert!(resp.was_cached);
}

// --- Status ---

#[tokio::test]
async fn status_empty_server() {
    let svc = setup();
    let resp = Deriva::status(&svc, Request::new(StatusRequest {})).await.unwrap().into_inner();
    assert_eq!(resp.recipe_count, 0);
    assert_eq!(resp.cache_entries, 0);
    assert_eq!(resp.cache_size_bytes, 0);
}

#[tokio::test]
async fn status_after_inserts() {
    let svc = setup();
    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"x".to_vec() })).await.unwrap().into_inner();
    Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "identity".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr],
        params: Default::default(),
    })).await.unwrap();
    let resp = Deriva::status(&svc, Request::new(StatusRequest {})).await.unwrap().into_inner();
    assert_eq!(resp.recipe_count, 1);
}

// --- Input validation ---

#[tokio::test]
async fn put_recipe_bad_addr_length() {
    let svc = setup();
    let result = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "identity".into(),
        function_version: "1.0.0".into(),
        inputs: vec![vec![0u8; 16]],
        params: Default::default(),
    })).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn get_bad_addr_length() {
    let svc = setup();
    let result = Deriva::get(&svc, Request::new(GetRequest { addr: vec![0u8; 5] })).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn resolve_bad_addr_length() {
    let svc = setup();
    let result = Deriva::resolve(&svc, Request::new(ResolveRequest { addr: vec![0u8; 10] })).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn invalidate_bad_addr_length() {
    let svc = setup();
    let result = Deriva::invalidate(&svc, Request::new(InvalidateRequest { addr: vec![0u8; 1], cascade: false })).await;
    assert!(result.is_err());
}

// --- Cascade Invalidation ---

#[tokio::test]
async fn cascade_invalidate_end_to_end() {
    let svc = setup();

    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"hello".to_vec() })).await.unwrap().into_inner();
    let recipe = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "uppercase".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr.clone()],
        params: Default::default(),
    })).await.unwrap().into_inner();

    // Materialize to populate cache
    collect_stream(&svc, recipe.addr.clone()).await;

    let resp = Deriva::cascade_invalidate(&svc, Request::new(CascadeInvalidateRequest {
        addr: leaf.addr,
        policy: "immediate".into(),
        include_root: true,
        detail_addrs: true,
    })).await.unwrap().into_inner();

    assert!(resp.evicted_count >= 1);
    assert!(resp.traversed_count >= 1);

    let status = Deriva::status(&svc, Request::new(StatusRequest {})).await.unwrap().into_inner();
    assert_eq!(status.cache_entries, 0);
}

#[tokio::test]
async fn cascade_dry_run_preserves_cache() {
    let svc = setup();

    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"data".to_vec() })).await.unwrap().into_inner();
    let recipe = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "uppercase".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr.clone()],
        params: Default::default(),
    })).await.unwrap().into_inner();

    collect_stream(&svc, recipe.addr.clone()).await;

    let resp = Deriva::cascade_invalidate(&svc, Request::new(CascadeInvalidateRequest {
        addr: leaf.addr,
        policy: "dry_run".into(),
        include_root: true,
        detail_addrs: true,
    })).await.unwrap().into_inner();

    assert!(resp.evicted_count >= 1);

    // Cache unchanged
    let status = Deriva::status(&svc, Request::new(StatusRequest {})).await.unwrap().into_inner();
    assert!(status.cache_entries >= 1);
}

#[tokio::test]
async fn backward_compat_invalidate_no_cascade() {
    let svc = setup();

    let leaf = Deriva::put_leaf(&svc, Request::new(PutLeafRequest { data: b"data".to_vec() })).await.unwrap().into_inner();
    let recipe = Deriva::put_recipe(&svc, Request::new(PutRecipeRequest {
        function_name: "uppercase".into(),
        function_version: "1.0.0".into(),
        inputs: vec![leaf.addr.clone()],
        params: Default::default(),
    })).await.unwrap().into_inner();

    collect_stream(&svc, recipe.addr.clone()).await;

    let resp = Deriva::invalidate(&svc, Request::new(InvalidateRequest {
        addr: recipe.addr,
        cascade: false,
    })).await.unwrap().into_inner();

    assert!(resp.was_cached);
    assert_eq!(resp.evicted_count, 1);
}
