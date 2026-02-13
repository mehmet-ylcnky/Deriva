use deriva_core::address::*;
use deriva_storage::StorageBackend;
use std::collections::BTreeMap;
use tempfile::tempdir;

#[test]
fn put_leaf_and_retrieve() {
    let dir = tempdir().unwrap();
    let backend = StorageBackend::open(dir.path()).unwrap();
    let addr = backend.put_leaf(b"raw sensor data").unwrap();
    let data = backend.blobs.get(&addr).unwrap();
    assert_eq!(data, Some(bytes::Bytes::from("raw sensor data")));
}

#[test]
fn put_recipe_and_rebuild_dag() {
    let dir = tempdir().unwrap();
    let backend = StorageBackend::open(dir.path()).unwrap();
    let leaf_addr = backend.put_leaf(b"input").unwrap();
    let recipe = Recipe::new(FunctionId::new("compress", "1.0.0"), vec![leaf_addr], BTreeMap::new());
    let recipe_addr = backend.put_recipe(&recipe).unwrap();

    let dag = backend.rebuild_dag().unwrap();
    assert!(dag.contains(&recipe_addr));
    assert_eq!(dag.get_recipe(&recipe_addr), Some(&recipe));
}

#[test]
fn full_roundtrip_survives_reopen() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let recipe = Recipe::new(
        FunctionId::new("transform", "1.0.0"),
        vec![CAddr::from_bytes(b"persistent data")],
        BTreeMap::from([("mode".into(), Value::String("fast".into()))]),
    );

    let leaf_addr;
    let recipe_addr;

    {
        let backend = StorageBackend::open(&root).unwrap();
        leaf_addr = backend.put_leaf(b"persistent data").unwrap();
        recipe_addr = backend.put_recipe(&recipe).unwrap();
        backend.recipes.flush().unwrap();
    }

    {
        let backend = StorageBackend::open(&root).unwrap();
        assert_eq!(backend.blobs.get(&leaf_addr).unwrap(), Some(bytes::Bytes::from("persistent data")));
        assert_eq!(backend.recipes.get(&recipe_addr).unwrap(), Some(recipe));
        let dag = backend.rebuild_dag().unwrap();
        assert!(dag.contains(&recipe_addr));
    }
}
