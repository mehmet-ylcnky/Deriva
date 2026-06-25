//! LRU cache behavior tests for PersistentDag.

use deriva_core::persistent_dag::{PersistentDag, PersistentDagConfig};
use deriva_core::{CAddr, FunctionId, Recipe};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use tempfile::TempDir;

fn temp_dag() -> (TempDir, PersistentDag) {
    let dir = TempDir::new().unwrap();
    let db = sled::open(dir.path().join("test.sled")).unwrap();
    let dag = PersistentDag::open(&db).unwrap();
    (dir, dag)
}

fn temp_dag_small_cache() -> (TempDir, PersistentDag) {
    let dir = TempDir::new().unwrap();
    let db = sled::open(dir.path().join("test.sled")).unwrap();
    let config = PersistentDagConfig {
        forward_cache_size: NonZeroUsize::new(2).unwrap(),
        reverse_cache_size: NonZeroUsize::new(2).unwrap(),
    };
    let dag = PersistentDag::open_with_config(&db, config).unwrap();
    (dir, dag)
}

fn leaf(data: &[u8]) -> CAddr {
    CAddr::from_bytes(data)
}

fn recipe(inputs: Vec<CAddr>, func: &str) -> Recipe {
    Recipe::new(FunctionId::new(func, "1.0"), inputs, BTreeMap::new())
}

#[test]
fn cache_hit_returns_correct_data() {
    let (_dir, dag) = temp_dag();
    let a = leaf(b"a");
    let b = leaf(b"b");
    let r = recipe(vec![a, b], "f");
    let addr = dag.insert(&r).unwrap();

    // First call populates cache
    let inputs1 = dag.inputs(&addr).unwrap().unwrap();
    // Second call should hit cache — same result
    let inputs2 = dag.inputs(&addr).unwrap().unwrap();
    assert_eq!(inputs1, inputs2);
    assert_eq!(inputs1, vec![a, b]);
}

#[test]
fn cache_populated_on_insert() {
    let (_dir, dag) = temp_dag();
    let a = leaf(b"a");
    let r = recipe(vec![a], "f");
    let addr = dag.insert(&r).unwrap();

    // insert() populates forward cache, so inputs() should hit cache immediately
    let inputs = dag.inputs(&addr).unwrap().unwrap();
    assert_eq!(inputs, vec![a]);
}

#[test]
fn cache_invalidation_on_insert() {
    let (_dir, dag) = temp_dag();
    let a = leaf(b"a");

    // Insert r1 depending on a
    let r1 = recipe(vec![a], "f1");
    let addr1 = dag.insert(&r1).unwrap();

    // Query dependents of a — populates reverse cache
    let deps1 = dag.direct_dependents(&a);
    assert_eq!(deps1, vec![addr1]);

    // Insert r2 also depending on a — should invalidate reverse cache for a
    let r2 = recipe(vec![a], "f2");
    let addr2 = dag.insert(&r2).unwrap();

    // Query again — should reflect both dependents
    let deps2 = dag.direct_dependents(&a);
    assert!(deps2.contains(&addr1));
    assert!(deps2.contains(&addr2));
    assert_eq!(deps2.len(), 2);
}

#[test]
fn cache_eviction_on_remove() {
    let (_dir, dag) = temp_dag();
    let a = leaf(b"a");
    let r = recipe(vec![a], "f");
    let addr = dag.insert(&r).unwrap();

    // Ensure cached
    assert!(dag.inputs(&addr).unwrap().is_some());

    // Remove
    assert!(dag.remove(&addr).unwrap());

    // After remove, inputs should return None
    assert!(dag.inputs(&addr).unwrap().is_none());
}

#[test]
fn cache_population_on_miss() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.sled");

    // Insert with one DAG instance
    let a = leaf(b"a");
    let addr;
    {
        let db = sled::open(&db_path).unwrap();
        let dag = PersistentDag::open(&db).unwrap();
        let r = recipe(vec![a], "f");
        addr = dag.insert(&r).unwrap();
    }

    // Reopen — fresh cache, data only in sled
    {
        let db = sled::open(&db_path).unwrap();
        let dag = PersistentDag::open(&db).unwrap();

        // First call = cache miss, reads from sled
        let inputs = dag.inputs(&addr).unwrap().unwrap();
        assert_eq!(inputs, vec![a]);

        // Second call = cache hit
        let inputs2 = dag.inputs(&addr).unwrap().unwrap();
        assert_eq!(inputs2, vec![a]);
    }
}

#[test]
fn small_cache_evicts_lru_entries() {
    let (_dir, dag) = temp_dag_small_cache();
    let a = leaf(b"a");
    let b = leaf(b"b");
    let c = leaf(b"c");

    let r1 = recipe(vec![a], "f1");
    let r2 = recipe(vec![b], "f2");
    let r3 = recipe(vec![c], "f3");

    let addr1 = dag.insert(&r1).unwrap();
    let addr2 = dag.insert(&r2).unwrap();
    let addr3 = dag.insert(&r3).unwrap();

    // Cache size = 2, so addr1 should be evicted after inserting addr3
    // But all should still be queryable (just slower on cache miss)
    assert_eq!(dag.inputs(&addr1).unwrap().unwrap(), vec![a]);
    assert_eq!(dag.inputs(&addr2).unwrap().unwrap(), vec![b]);
    assert_eq!(dag.inputs(&addr3).unwrap().unwrap(), vec![c]);
}

#[test]
fn reverse_cache_populated_on_dependents_query() {
    let (_dir, dag) = temp_dag();
    let a = leaf(b"a");
    let r = recipe(vec![a], "f");
    let addr = dag.insert(&r).unwrap();

    // First call populates reverse cache
    let deps1 = dag.direct_dependents(&a);
    assert_eq!(deps1, vec![addr]);

    // Second call hits cache
    let deps2 = dag.direct_dependents(&a);
    assert_eq!(deps2, vec![addr]);
}
