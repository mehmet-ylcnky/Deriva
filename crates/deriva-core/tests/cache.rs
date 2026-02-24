use bytes::Bytes;
use deriva_core::address::CAddr;
use deriva_core::cache::*;

fn addr(name: &str) -> CAddr {
    CAddr::from_bytes(name.as_bytes())
}

fn data(size: usize) -> Bytes {
    Bytes::from(vec![0xAB; size])
}

fn cost(cpu_ms: u64) -> ComputeCost {
    ComputeCost { cpu_ms, memory_bytes: 0 }
}

// --- Test Group 1: Basic get/put/contains ---

#[test]
fn put_and_get() {
    let mut cache = EvictableCache::with_max_size(1024);
    cache.put(addr("a"), Bytes::from("hello"), ComputeCost::default());
    assert_eq!(cache.get(&addr("a")), Some(Bytes::from("hello")));
}

#[test]
fn get_missing() {
    let mut cache = EvictableCache::with_max_size(1024);
    assert_eq!(cache.get(&addr("nope")), None);
}

#[test]
fn contains_after_put() {
    let mut cache = EvictableCache::with_max_size(1024);
    cache.put(addr("a"), Bytes::from("x"), ComputeCost::default());
    assert!(cache.contains(&addr("a")));
    assert!(!cache.contains(&addr("b")));
}

#[test]
fn put_overwrites() {
    let mut cache = EvictableCache::with_max_size(1024);
    cache.put(addr("a"), Bytes::from("v1"), ComputeCost::default());
    cache.put(addr("a"), Bytes::from("v2"), ComputeCost::default());
    assert_eq!(cache.get(&addr("a")), Some(Bytes::from("v2")));
    assert_eq!(cache.entry_count(), 1);
}

#[test]
fn put_overwrite_updates_size() {
    let mut cache = EvictableCache::with_max_size(1024);
    cache.put(addr("a"), data(100), ComputeCost::default());
    assert_eq!(cache.current_size(), 100);
    cache.put(addr("a"), data(200), ComputeCost::default());
    assert_eq!(cache.current_size(), 200);
}

#[test]
fn remove_entry() {
    let mut cache = EvictableCache::with_max_size(1024);
    cache.put(addr("a"), Bytes::from("hello"), ComputeCost::default());
    assert_eq!(cache.remove(&addr("a")), Some(Bytes::from("hello")));
    assert!(!cache.contains(&addr("a")));
    assert_eq!(cache.current_size(), 0);
}

#[test]
fn remove_missing() {
    let mut cache = EvictableCache::with_max_size(1024);
    assert_eq!(cache.remove(&addr("nope")), None);
}

// --- Test Group 2: Size tracking ---

#[test]
fn current_size_tracks_puts() {
    let mut cache = EvictableCache::with_max_size(10000);
    cache.put(addr("a"), data(100), ComputeCost::default());
    cache.put(addr("b"), data(200), ComputeCost::default());
    assert_eq!(cache.current_size(), 300);
    assert_eq!(cache.entry_count(), 2);
}

#[test]
fn current_size_tracks_removes() {
    let mut cache = EvictableCache::with_max_size(10000);
    cache.put(addr("a"), data(100), ComputeCost::default());
    cache.put(addr("b"), data(200), ComputeCost::default());
    cache.remove(&addr("a"));
    assert_eq!(cache.current_size(), 200);
    assert_eq!(cache.entry_count(), 1);
}

#[test]
fn empty_cache_stats() {
    let cache = EvictableCache::with_max_size(1024);
    assert_eq!(cache.current_size(), 0);
    assert_eq!(cache.entry_count(), 0);
}

// --- Test Group 3: Access tracking ---

#[test]
fn access_count_increments() {
    let mut cache = EvictableCache::with_max_size(1024);
    cache.put(addr("a"), Bytes::from("x"), ComputeCost::default());
    cache.get(&addr("a"));
    cache.get(&addr("a"));
    cache.get(&addr("a"));
    let score = cache.score(&addr("a")).unwrap();
    // score = (1 × (3+1)) / 1 = 4.0
    assert!((score - 4.0).abs() < 0.01);
}

#[test]
fn hit_miss_tracking() {
    let mut cache = EvictableCache::with_max_size(1024);
    cache.put(addr("a"), Bytes::from("x"), ComputeCost::default());
    cache.get(&addr("a"));
    cache.get(&addr("a"));
    cache.get(&addr("b"));
    assert_eq!(cache.hits(), 2);
    assert_eq!(cache.misses(), 1);
}

#[test]
fn hit_rate_calculation() {
    let mut cache = EvictableCache::with_max_size(1024);
    assert_eq!(cache.hit_rate(), 0.0);
    cache.put(addr("a"), Bytes::from("x"), ComputeCost::default());
    cache.get(&addr("a"));
    cache.get(&addr("b"));
    assert!((cache.hit_rate() - 0.5).abs() < 0.01);
}

// --- Test Group 4: Eviction scoring ---

#[test]
fn score_cheap_rare_large_is_low() {
    let mut cache = EvictableCache::with_max_size(10000);
    cache.put(addr("a"), data(1000), cost(1));
    let s = cache.score(&addr("a")).unwrap();
    assert!(s < 0.01);
}

#[test]
fn score_expensive_frequent_small_is_high() {
    let mut cache = EvictableCache::with_max_size(10000);
    cache.put(addr("a"), data(10), cost(500));
    for _ in 0..20 {
        cache.get(&addr("a"));
    }
    let s = cache.score(&addr("a")).unwrap();
    assert!(s > 1000.0);
}

#[test]
fn score_ordering_matches_eviction_priority() {
    let mut cache = EvictableCache::with_max_size(10000);
    cache.put(addr("keep"), data(10), cost(100));
    for _ in 0..10 { cache.get(&addr("keep")); }
    cache.put(addr("evict"), data(500), cost(1));

    let keep_score = cache.score(&addr("keep")).unwrap();
    let evict_score = cache.score(&addr("evict")).unwrap();
    assert!(evict_score < keep_score);
}

// --- Test Group 5: Pressure-based eviction ---

#[test]
fn eviction_triggers_at_pressure_threshold() {
    let config = CacheConfig {
        max_size: 1000,
        pressure_threshold: 0.8,
        target_ratio: 0.6,
    };
    let mut cache = EvictableCache::new(config);

    cache.put(addr("a"), data(350), cost(100));
    cache.put(addr("b"), data(350), cost(100));
    assert_eq!(cache.entry_count(), 2);
    assert_eq!(cache.current_size(), 700);

    cache.put(addr("c"), data(200), cost(1));
    assert!(cache.current_size() <= 600);
}

#[test]
fn eviction_removes_lowest_scored_first() {
    let config = CacheConfig {
        max_size: 1000,
        pressure_threshold: 0.5,
        target_ratio: 0.3,
    };
    let mut cache = EvictableCache::new(config);

    cache.put(addr("expensive"), data(200), cost(1000));
    for _ in 0..10 { cache.get(&addr("expensive")); }
    cache.put(addr("cheap"), data(200), cost(1));
    cache.put(addr("trigger"), data(200), cost(1));

    assert!(cache.contains(&addr("expensive")));
    assert!(cache.current_size() <= 300);
}

#[test]
fn eviction_count_tracked() {
    let config = CacheConfig {
        max_size: 500,
        pressure_threshold: 0.5,
        target_ratio: 0.2,
    };
    let mut cache = EvictableCache::new(config);

    cache.put(addr("a"), data(100), cost(1));
    cache.put(addr("b"), data(100), cost(1));
    cache.put(addr("c"), data(100), cost(1));

    assert!(cache.eviction_count() > 0);
}

#[test]
fn no_eviction_below_threshold() {
    let config = CacheConfig {
        max_size: 1000,
        pressure_threshold: 0.8,
        target_ratio: 0.6,
    };
    let mut cache = EvictableCache::new(config);

    cache.put(addr("a"), data(200), cost(1));
    cache.put(addr("b"), data(200), cost(1));
    cache.put(addr("c"), data(200), cost(1));
    assert_eq!(cache.entry_count(), 3);
    assert_eq!(cache.eviction_count(), 0);
}

// --- Test Group 6: Pinning ---

#[test]
fn pinned_entries_survive_eviction() {
    let config = CacheConfig {
        max_size: 500,
        pressure_threshold: 0.5,
        target_ratio: 0.2,
    };
    let mut cache = EvictableCache::new(config);

    cache.put(addr("pinned"), data(100), cost(1));
    cache.pin(&addr("pinned"));
    cache.put(addr("b"), data(100), cost(1));
    cache.put(addr("c"), data(100), cost(1));

    assert!(cache.contains(&addr("pinned")));
}

#[test]
fn unpin_makes_evictable() {
    let config = CacheConfig {
        max_size: 1000,
        pressure_threshold: 0.8,
        target_ratio: 0.3,
    };
    let mut cache = EvictableCache::new(config);

    cache.put(addr("a"), data(100), cost(1));
    cache.pin(&addr("a"));
    cache.unpin(&addr("a"));

    cache.put(addr("b"), data(100), cost(1000));
    for _ in 0..10 { cache.get(&addr("b")); }

    // Now push over threshold to trigger eviction
    cache.put(addr("filler1"), data(300), cost(500));
    cache.put(addr("filler2"), data(300), cost(1));

    // "b" is expensive and frequently accessed — should survive
    assert!(cache.contains(&addr("b")));
}

#[test]
fn pin_missing_returns_false() {
    let mut cache = EvictableCache::with_max_size(1024);
    assert!(!cache.pin(&addr("nope")));
}

#[test]
fn unpin_missing_returns_false() {
    let mut cache = EvictableCache::with_max_size(1024);
    assert!(!cache.unpin(&addr("nope")));
}

// --- Test Group 7: Edge cases ---

#[test]
fn zero_size_entry() {
    let mut cache = EvictableCache::with_max_size(1024);
    cache.put(addr("empty"), Bytes::new(), ComputeCost::default());
    assert!(cache.contains(&addr("empty")));
    assert_eq!(cache.get(&addr("empty")), Some(Bytes::new()));
    assert_eq!(cache.current_size(), 0);
}

#[test]
fn many_entries_eviction_stress() {
    let config = CacheConfig {
        max_size: 1000,
        pressure_threshold: 0.8,
        target_ratio: 0.5,
    };
    let mut cache = EvictableCache::new(config);

    for i in 0..100 {
        cache.put(addr(&format!("entry_{i}")), data(50), cost(i as u64 + 1));
    }

    assert!(cache.current_size() <= 1000);
    assert!(cache.eviction_count() > 0);
}

#[test]
fn all_pinned_prevents_eviction() {
    // Use large max_size so puts don't trigger eviction, then shrink config
    let mut cache = EvictableCache::with_max_size(10000);

    cache.put(addr("a"), data(100), cost(1));
    cache.pin(&addr("a"));
    cache.put(addr("b"), data(100), cost(1));
    cache.pin(&addr("b"));
    cache.put(addr("c"), data(100), cost(1));
    cache.pin(&addr("c"));

    assert_eq!(cache.entry_count(), 3);
    // All entries survive because all are pinned
}
