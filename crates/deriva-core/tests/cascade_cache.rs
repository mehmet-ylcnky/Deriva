use bytes::Bytes;
use deriva_core::address::CAddr;
use deriva_core::cache::EvictableCache;

fn addr(name: &str) -> CAddr {
    CAddr::from_bytes(name.as_bytes())
}

#[test]
fn remove_batch_all_present() {
    let mut cache = EvictableCache::with_max_size(1024 * 1024);
    let a = addr("a");
    let b = addr("b");
    let c = addr("c");
    cache.put_simple(a, Bytes::from("aaa"));
    cache.put_simple(b, Bytes::from("bbb"));
    cache.put_simple(c, Bytes::from("ccc"));

    let (count, bytes, removed) = cache.remove_batch(&[a, b, c]);
    assert_eq!(count, 3);
    assert_eq!(bytes, 9);
    assert_eq!(removed.len(), 3);
    assert_eq!(cache.entry_count(), 0);
}

#[test]
fn remove_batch_partial_present() {
    let mut cache = EvictableCache::with_max_size(1024 * 1024);
    let a = addr("a");
    let b = addr("b");
    let c = addr("c");
    cache.put_simple(a, Bytes::from("aaa"));
    cache.put_simple(c, Bytes::from("ccc"));

    let (count, bytes, removed) = cache.remove_batch(&[a, b, c]);
    assert_eq!(count, 2);
    assert_eq!(bytes, 6);
    assert_eq!(removed.len(), 2);
    assert!(!removed.contains(&b));
}

#[test]
fn remove_batch_empty_list() {
    let mut cache = EvictableCache::with_max_size(1024 * 1024);
    cache.put_simple(addr("a"), Bytes::from("aaa"));

    let (count, bytes, removed) = cache.remove_batch(&[]);
    assert_eq!(count, 0);
    assert_eq!(bytes, 0);
    assert!(removed.is_empty());
    assert_eq!(cache.entry_count(), 1);
}

#[test]
fn remove_batch_none_present() {
    let mut cache = EvictableCache::with_max_size(1024 * 1024);
    let (count, bytes, removed) = cache.remove_batch(&[addr("a"), addr("b")]);
    assert_eq!(count, 0);
    assert_eq!(bytes, 0);
    assert!(removed.is_empty());
}

#[test]
fn remove_batch_updates_current_size() {
    let mut cache = EvictableCache::with_max_size(1024 * 1024);
    let a = addr("a");
    let b = addr("b");
    cache.put_simple(a, Bytes::from(vec![0u8; 100]));
    cache.put_simple(b, Bytes::from(vec![0u8; 200]));
    assert_eq!(cache.current_size(), 300);

    cache.remove_batch(&[a]);
    assert_eq!(cache.current_size(), 200);

    cache.remove_batch(&[b]);
    assert_eq!(cache.current_size(), 0);
}
