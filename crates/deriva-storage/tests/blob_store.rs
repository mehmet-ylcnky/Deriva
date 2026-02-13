use bytes::Bytes;
use deriva_compute::leaf_store::LeafStore;
use deriva_core::address::CAddr;
use deriva_storage::BlobStore;
use tempfile::tempdir;

fn addr(name: &str) -> CAddr {
    CAddr::from_bytes(name.as_bytes())
}

// --- Group 1: Basics ---

#[test]
fn put_and_get() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("a"), b"hello").unwrap();
    assert_eq!(store.get(&addr("a")).unwrap(), Some(Bytes::from("hello")));
}

#[test]
fn get_missing() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    assert_eq!(store.get(&addr("nope")).unwrap(), None);
}

#[test]
fn contains() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    assert!(!store.contains(&addr("a")));
    store.put(&addr("a"), b"data").unwrap();
    assert!(store.contains(&addr("a")));
}

#[test]
fn put_overwrites() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("a"), b"v1").unwrap();
    store.put(&addr("a"), b"v2").unwrap();
    assert_eq!(store.get(&addr("a")).unwrap(), Some(Bytes::from("v2")));
}

#[test]
fn remove_blob() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("a"), b"data").unwrap();
    assert!(store.remove(&addr("a")).unwrap());
    assert!(!store.contains(&addr("a")));
}

#[test]
fn remove_missing() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    assert!(!store.remove(&addr("nope")).unwrap());
}

#[test]
fn empty_blob() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("empty"), b"").unwrap();
    assert_eq!(store.get(&addr("empty")).unwrap(), Some(Bytes::new()));
}

#[test]
fn large_blob() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    let data = vec![0xAB; 1_000_000];
    let a = CAddr::from_bytes(&data);
    store.put(&a, &data).unwrap();
    let loaded = store.get(&a).unwrap().unwrap();
    assert_eq!(loaded.len(), 1_000_000);
}

// --- Group 2: Sharding ---

#[test]
fn blob_path_uses_sharding() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    let a = CAddr::from_bytes(b"test sharding");
    store.put(&a, b"data").unwrap();
    let hex = a.to_hex();
    let expected = dir.path().join("blobs").join(&hex[..2]).join(&hex[2..4]).join(&hex);
    assert!(expected.exists());
}

#[test]
fn many_blobs_distributed() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    for i in 0..100 {
        let data = format!("blob_{i}");
        let a = CAddr::from_bytes(data.as_bytes());
        store.put(&a, data.as_bytes()).unwrap();
    }
    for i in 0..100 {
        let data = format!("blob_{i}");
        let a = CAddr::from_bytes(data.as_bytes());
        assert!(store.contains(&a));
    }
}

// --- Group 3: total_size ---

#[test]
fn total_size_empty() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    assert_eq!(store.total_size().unwrap(), 0);
}

#[test]
fn total_size_tracks_blobs() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("a"), &[0; 100]).unwrap();
    store.put(&addr("b"), &[0; 200]).unwrap();
    assert_eq!(store.total_size().unwrap(), 300);
}

#[test]
fn total_size_after_remove() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("a"), &[0; 100]).unwrap();
    store.put(&addr("b"), &[0; 200]).unwrap();
    store.remove(&addr("a")).unwrap();
    assert_eq!(store.total_size().unwrap(), 200);
}

// --- Group 4: LeafStore trait ---

#[test]
fn leaf_store_trait_get() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    let a = CAddr::from_bytes(b"leaf data");
    store.put(&a, b"leaf data").unwrap();
    assert_eq!(store.get_leaf(&a), Some(Bytes::from("leaf data")));
}

#[test]
fn leaf_store_trait_missing() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    assert_eq!(store.get_leaf(&addr("nope")), None);
}
