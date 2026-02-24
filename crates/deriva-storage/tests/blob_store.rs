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

// --- Group 5: GC Extensions ---

#[test]
fn remove_with_size_returns_bytes() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("a"), &[0; 42]).unwrap();
    let freed = store.remove_with_size(&addr("a")).unwrap();
    assert_eq!(freed, 42);
    assert!(!store.contains(&addr("a")));
}

#[test]
fn remove_with_size_missing_returns_zero() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    assert_eq!(store.remove_with_size(&addr("nope")).unwrap(), 0);
}

#[test]
fn remove_batch_blobs_removes_all() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("a"), &[0; 10]).unwrap();
    store.put(&addr("b"), &[0; 20]).unwrap();
    store.put(&addr("c"), &[0; 30]).unwrap();
    let (count, bytes) = store.remove_batch_blobs(&[addr("a"), addr("b")]).unwrap();
    assert_eq!(count, 2);
    assert_eq!(bytes, 30);
    assert!(store.contains(&addr("c")));
}

#[test]
fn remove_batch_blobs_skips_missing() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("a"), &[0; 10]).unwrap();
    let (count, bytes) = store.remove_batch_blobs(&[addr("a"), addr("nope")]).unwrap();
    assert_eq!(count, 1);
    assert_eq!(bytes, 10);
}

#[test]
fn remove_batch_blobs_empty_slice() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    let (count, bytes) = store.remove_batch_blobs(&[]).unwrap();
    assert_eq!(count, 0);
    assert_eq!(bytes, 0);
}

#[test]
fn list_addrs_returns_all() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("x"), b"xx").unwrap();
    store.put(&addr("y"), b"yy").unwrap();
    let addrs = store.list_addrs().unwrap();
    assert_eq!(addrs.len(), 2);
    assert!(addrs.contains(&addr("x")));
    assert!(addrs.contains(&addr("y")));
}

#[test]
fn list_addrs_empty_store() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    assert!(store.list_addrs().unwrap().is_empty());
}

#[test]
fn stats_counts_and_bytes() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open(dir.path().join("blobs")).unwrap();
    store.put(&addr("a"), &[0; 100]).unwrap();
    store.put(&addr("b"), &[0; 200]).unwrap();
    let (count, bytes) = store.stats().unwrap();
    assert_eq!(count, 2);
    assert_eq!(bytes, 300);
}
