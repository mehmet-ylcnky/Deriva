use ::deriva_compute::builtins;
use ::deriva_compute::registry::FunctionRegistry;
use ::deriva_server::service::proto::deriva_server::Deriva;
use ::deriva_server::service::proto::*;
use ::deriva_server::service::DerivaService;
use ::deriva_server::state::ServerState;
use ::deriva_storage::StorageBackend;
use std::sync::Arc;
use tempfile::tempdir;
use tonic::Request;

fn setup() -> DerivaService {
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();
    std::mem::forget(dir);
    let storage = StorageBackend::open(&path).unwrap();
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    let state = Arc::new(ServerState::new(storage, registry).unwrap());
    DerivaService::new(state)
}

fn gc_req() -> GcRequest {
    GcRequest { grace_period_secs: 0, dry_run: false, detail_addrs: true, max_removals: 0 }
}

async fn put_leaf(svc: &DerivaService, data: &[u8]) -> Vec<u8> {
    Deriva::put_leaf(svc, Request::new(PutLeafRequest { data: data.to_vec() }))
        .await.unwrap().into_inner().addr
}

async fn put_recipe(svc: &DerivaService, inputs: Vec<Vec<u8>>) {
    Deriva::put_recipe(svc, Request::new(PutRecipeRequest {
        function_name: "identity".into(),
        function_version: "1.0.0".into(),
        inputs,
        params: Default::default(),
    })).await.unwrap();
}

#[tokio::test]
async fn gc_rpc_removes_orphan() {
    let svc = setup();
    let orphan = put_leaf(&svc, b"orphan").await;
    let used = put_leaf(&svc, b"used").await;
    put_recipe(&svc, vec![used]).await;

    let r = Deriva::garbage_collect(&svc, Request::new(gc_req())).await.unwrap().into_inner();
    assert_eq!(r.blobs_removed, 1);
    assert!(r.removed_addrs.contains(&orphan));
}

#[tokio::test]
async fn gc_rpc_keeps_referenced() {
    let svc = setup();
    let a = put_leaf(&svc, b"data").await;
    put_recipe(&svc, vec![a.clone()]).await;

    let r = Deriva::garbage_collect(&svc, Request::new(gc_req())).await.unwrap().into_inner();
    assert_eq!(r.blobs_removed, 0);
}

#[tokio::test]
async fn pin_protects_from_gc() {
    let svc = setup();
    let a = put_leaf(&svc, b"pinme").await;
    Deriva::pin(&svc, Request::new(PinRequest { addr: a.clone() })).await.unwrap();

    let r = Deriva::garbage_collect(&svc, Request::new(gc_req())).await.unwrap().into_inner();
    assert_eq!(r.blobs_removed, 0);
    assert_eq!(r.pinned_count, 1);
}

#[tokio::test]
async fn unpin_then_gc_removes() {
    let svc = setup();
    let a = put_leaf(&svc, b"temp").await;
    Deriva::pin(&svc, Request::new(PinRequest { addr: a.clone() })).await.unwrap();
    Deriva::unpin(&svc, Request::new(UnpinRequest { addr: a.clone() })).await.unwrap();

    let r = Deriva::garbage_collect(&svc, Request::new(gc_req())).await.unwrap().into_inner();
    assert_eq!(r.blobs_removed, 1);
}

#[tokio::test]
async fn list_pins_rpc() {
    let svc = setup();
    let a = put_leaf(&svc, b"a").await;
    let b = put_leaf(&svc, b"b").await;
    Deriva::pin(&svc, Request::new(PinRequest { addr: a.clone() })).await.unwrap();
    Deriva::pin(&svc, Request::new(PinRequest { addr: b.clone() })).await.unwrap();

    let r = Deriva::list_pins(&svc, Request::new(ListPinsRequest {})).await.unwrap().into_inner();
    assert_eq!(r.count, 2);
    assert!(r.addrs.contains(&a));
    assert!(r.addrs.contains(&b));
}

#[tokio::test]
async fn pin_idempotent() {
    let svc = setup();
    let a = put_leaf(&svc, b"x").await;
    let r1 = Deriva::pin(&svc, Request::new(PinRequest { addr: a.clone() })).await.unwrap().into_inner();
    let r2 = Deriva::pin(&svc, Request::new(PinRequest { addr: a.clone() })).await.unwrap().into_inner();
    assert!(r1.was_new);
    assert!(!r2.was_new);
    let list = Deriva::list_pins(&svc, Request::new(ListPinsRequest {})).await.unwrap().into_inner();
    assert_eq!(list.count, 1);
}

#[tokio::test]
async fn gc_dry_run_rpc() {
    let svc = setup();
    let _orphan = put_leaf(&svc, b"orphan").await;

    let mut req = gc_req();
    req.dry_run = true;
    let r = Deriva::garbage_collect(&svc, Request::new(req)).await.unwrap().into_inner();
    assert_eq!(r.blobs_removed, 1);
    assert!(r.dry_run);

    // Still there after dry run
    let r2 = Deriva::garbage_collect(&svc, Request::new(gc_req())).await.unwrap().into_inner();
    assert_eq!(r2.blobs_removed, 1); // still removable
}

#[tokio::test]
async fn gc_max_removals_rpc() {
    let svc = setup();
    for i in 0u8..5 {
        put_leaf(&svc, &[i; 10]).await;
    }

    let mut req = gc_req();
    req.max_removals = 2;
    let r = Deriva::garbage_collect(&svc, Request::new(req)).await.unwrap().into_inner();
    assert_eq!(r.blobs_removed, 2);

    // Remaining 3
    let r2 = Deriva::garbage_collect(&svc, Request::new(gc_req())).await.unwrap().into_inner();
    assert_eq!(r2.blobs_removed, 3);
}
