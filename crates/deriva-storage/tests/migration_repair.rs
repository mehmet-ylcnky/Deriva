//! Integration tests for migration and consistency repair in StorageBackend.

use bytes::Bytes;
use deriva_core::address::*;
use deriva_core::PersistentDag;
use deriva_storage::{StorageBackend, SledRecipeStore};
use std::collections::BTreeMap;
use tempfile::tempdir;

fn make_recipe(name: &str, inputs: Vec<CAddr>) -> Recipe {
    Recipe::new(FunctionId::new(name, "1.0.0"), inputs, BTreeMap::new())
}

/// Migration triggers when DAG is empty but recipe store is populated.
#[test]
fn migration_triggers_when_dag_empty_recipes_populated() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let db_path = root.join("storage.sled");

    let leaf = CAddr::from_bytes(b"leaf1");
    let recipe = make_recipe("compress", vec![leaf]);
    let recipe_addr = recipe.addr();

    // Manually populate recipe store without DAG
    {
        let db = sled::open(&db_path).unwrap();
        let store = SledRecipeStore::open_with_db(&db).unwrap();
        store.put(&recipe_addr, &recipe).unwrap();
        store.flush().unwrap();
        // DAG trees not touched — they remain empty
        drop(db);
    }

    // Open triggers migration
    let backend = StorageBackend::open(root).unwrap();
    assert!(backend.dag.contains(&recipe_addr));
    assert_eq!(backend.dag.inputs(&recipe_addr).unwrap(), Some(vec![leaf]));
}

/// Migration is skipped when DAG is already populated.
#[test]
fn migration_skipped_when_dag_populated() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Normal open: put recipe (populates both store and DAG)
    {
        let backend = StorageBackend::open(root).unwrap();
        let leaf = backend.put_leaf(b"data").unwrap();
        let recipe = make_recipe("identity", vec![leaf]);
        backend.put_recipe(&recipe).unwrap();
    }

    // Reopen — DAG already has entries, migration should not run
    // (verified by the fact it opens successfully in O(1)-ish time)
    let backend = StorageBackend::open(root).unwrap();
    assert!(!backend.dag.is_empty());
}

/// Repair detects and fixes recipes missing from DAG.
#[test]
fn repair_detects_and_fixes_missing_dag_entries() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let leaf = CAddr::from_bytes(b"leaf2");
    let recipe1 = make_recipe("fn_a", vec![leaf]);
    let recipe2 = make_recipe("fn_b", vec![leaf]);
    let addr1 = recipe1.addr();
    let addr2 = recipe2.addr();

    // Create backend with both recipes in store + DAG
    {
        let backend = StorageBackend::open(root).unwrap();
        backend.put_leaf(b"leaf2").unwrap();
        backend.put_recipe(&recipe1).unwrap();
        backend.put_recipe(&recipe2).unwrap();
    }

    // Simulate inconsistency: remove recipe2 from DAG only
    {
        let db = sled::open(root.join("storage.sled")).unwrap();
        let dag = PersistentDag::open(&db).unwrap();
        dag.remove(&addr2).unwrap();
        assert!(!dag.contains(&addr2));
        assert!(dag.contains(&addr1));
        drop(db);
    }

    // Repair should fix it
    let backend = StorageBackend::open(root).unwrap();
    assert!(backend.dag.contains(&addr1));
    // addr2 was removed from DAG but still in recipe store — repair fixes it
    let repaired = backend.repair_consistency().unwrap();
    assert_eq!(repaired, 1);
    assert!(backend.dag.contains(&addr2));
}

/// Repair is a no-op when fully consistent.
#[test]
fn repair_noop_when_consistent() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let backend = StorageBackend::open(root).unwrap();
    let leaf = backend.put_leaf(b"consistent").unwrap();
    let recipe = make_recipe("hash", vec![leaf]);
    backend.put_recipe(&recipe).unwrap();

    let repaired = backend.repair_consistency().unwrap();
    assert_eq!(repaired, 0);
}

/// Multiple recipes migrated correctly preserves all DAG edges.
#[test]
fn migration_multiple_recipes_preserves_edges() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let db_path = root.join("storage.sled");

    let leaf_a = CAddr::from_bytes(b"a");
    let leaf_b = CAddr::from_bytes(b"b");
    let r1 = make_recipe("step1", vec![leaf_a]);
    let r2 = make_recipe("step2", vec![r1.addr(), leaf_b]);
    let addr1 = r1.addr();
    let addr2 = r2.addr();

    // Populate recipe store only
    {
        let db = sled::open(&db_path).unwrap();
        let store = SledRecipeStore::open_with_db(&db).unwrap();
        store.put(&addr1, &r1).unwrap();
        store.put(&addr2, &r2).unwrap();
        store.flush().unwrap();
        drop(db);
    }

    // Migration should reconstruct full DAG
    let backend = StorageBackend::open(root).unwrap();
    assert!(backend.dag.contains(&addr1));
    assert!(backend.dag.contains(&addr2));
    assert_eq!(backend.dag.inputs(&addr2).unwrap(), Some(vec![addr1, leaf_b]));
    let deps = backend.dag.direct_dependents(&addr1);
    assert!(deps.contains(&addr2));
}
