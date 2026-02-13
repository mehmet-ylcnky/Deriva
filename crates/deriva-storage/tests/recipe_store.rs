use deriva_core::address::*;
use deriva_storage::SledRecipeStore;
use std::collections::BTreeMap;
use tempfile::tempdir;

fn recipe(name: &str, inputs: Vec<CAddr>) -> Recipe {
    Recipe::new(FunctionId::new(name, "1.0.0"), inputs, BTreeMap::new())
}

fn leaf(name: &str) -> CAddr {
    CAddr::from_bytes(name.as_bytes())
}

// --- Group 1: Basics ---

#[test]
fn put_and_get() {
    let dir = tempdir().unwrap();
    let store = SledRecipeStore::open(dir.path().join("test.sled")).unwrap();
    let r = recipe("compress", vec![leaf("data")]);
    let addr = r.addr();
    store.put(&addr, &r).unwrap();
    assert_eq!(store.get(&addr).unwrap(), Some(r));
}

#[test]
fn get_missing() {
    let dir = tempdir().unwrap();
    let store = SledRecipeStore::open(dir.path().join("test.sled")).unwrap();
    assert_eq!(store.get(&leaf("nope")).unwrap(), None);
}

#[test]
fn contains() {
    let dir = tempdir().unwrap();
    let store = SledRecipeStore::open(dir.path().join("test.sled")).unwrap();
    let r = recipe("fn", vec![leaf("x")]);
    let addr = r.addr();
    assert!(!store.contains(&addr).unwrap());
    store.put(&addr, &r).unwrap();
    assert!(store.contains(&addr).unwrap());
}

#[test]
fn put_idempotent() {
    let dir = tempdir().unwrap();
    let store = SledRecipeStore::open(dir.path().join("test.sled")).unwrap();
    let r = recipe("fn", vec![leaf("x")]);
    let addr = r.addr();
    store.put(&addr, &r).unwrap();
    store.put(&addr, &r).unwrap();
    assert_eq!(store.len(), 1);
}

#[test]
fn remove() {
    let dir = tempdir().unwrap();
    let store = SledRecipeStore::open(dir.path().join("test.sled")).unwrap();
    let r = recipe("fn", vec![leaf("x")]);
    let addr = r.addr();
    store.put(&addr, &r).unwrap();
    assert_eq!(store.remove(&addr).unwrap(), Some(r));
    assert!(!store.contains(&addr).unwrap());
}

#[test]
fn remove_missing() {
    let dir = tempdir().unwrap();
    let store = SledRecipeStore::open(dir.path().join("test.sled")).unwrap();
    assert_eq!(store.remove(&leaf("nope")).unwrap(), None);
}

#[test]
fn len_and_empty() {
    let dir = tempdir().unwrap();
    let store = SledRecipeStore::open(dir.path().join("test.sled")).unwrap();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
    let r = recipe("fn", vec![leaf("x")]);
    store.put(&r.addr(), &r).unwrap();
    assert_eq!(store.len(), 1);
    assert!(!store.is_empty());
}

// --- Group 2: Persistence ---

#[test]
fn survives_reopen() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("persist.sled");
    let r = recipe("compress", vec![leaf("data")]);
    let addr = r.addr();

    {
        let store = SledRecipeStore::open(&path).unwrap();
        store.put(&addr, &r).unwrap();
        store.flush().unwrap();
    }

    {
        let store = SledRecipeStore::open(&path).unwrap();
        assert_eq!(store.get(&addr).unwrap(), Some(r));
    }
}

#[test]
fn iter_all_returns_everything() {
    let dir = tempdir().unwrap();
    let store = SledRecipeStore::open(dir.path().join("test.sled")).unwrap();
    let r1 = recipe("a", vec![leaf("x")]);
    let r2 = recipe("b", vec![leaf("y")]);
    store.put(&r1.addr(), &r1).unwrap();
    store.put(&r2.addr(), &r2).unwrap();

    let all: Vec<_> = store.iter_all().collect::<std::result::Result<Vec<_>, _>>().unwrap();
    assert_eq!(all.len(), 2);
    let addrs: Vec<CAddr> = all.iter().map(|(a, _)| *a).collect();
    assert!(addrs.contains(&r1.addr()));
    assert!(addrs.contains(&r2.addr()));
}

#[test]
fn rebuild_dag_from_sled() {
    let dir = tempdir().unwrap();
    let store = SledRecipeStore::open(dir.path().join("test.sled")).unwrap();
    let a = leaf("A");
    let r1 = recipe("step1", vec![a]);
    let r2 = recipe("step2", vec![r1.addr()]);
    store.put(&r1.addr(), &r1).unwrap();
    store.put(&r2.addr(), &r2).unwrap();

    let dag = store.rebuild_dag().unwrap();
    assert_eq!(dag.len(), 2);
    assert!(dag.contains(&r1.addr()));
    assert!(dag.contains(&r2.addr()));
    assert_eq!(dag.resolve_order(&r2.addr()).len(), 2);
}
