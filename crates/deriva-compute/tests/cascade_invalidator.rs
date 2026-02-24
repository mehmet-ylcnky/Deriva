use bytes::Bytes;
use deriva_compute::invalidation::CascadeInvalidator;
use deriva_core::address::{CAddr, FunctionId, Recipe};
use deriva_core::cache::EvictableCache;
use deriva_core::dag::DagStore;
use deriva_core::invalidation::CascadePolicy;
use std::collections::BTreeMap;

fn leaf_addr(name: &str) -> CAddr {
    CAddr::from_bytes(name.as_bytes())
}

fn insert_recipe(dag: &mut DagStore, func: &str, inputs: Vec<CAddr>) -> CAddr {
    let recipe = Recipe::new(FunctionId::new(func, "v1"), inputs, BTreeMap::new());
    dag.insert(recipe).unwrap()
}

#[test]
fn cascade_immediate_diamond() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::with_max_size(1024 * 1024);

    let a = leaf_addr("a");
    let b = leaf_addr("b");
    let x = insert_recipe(&mut dag, "concat", vec![a, b]);
    let y = insert_recipe(&mut dag, "hash", vec![a]);
    let z = insert_recipe(&mut dag, "transform", vec![x, y]);

    cache.put_simple(a, Bytes::from("leaf_a"));
    cache.put_simple(x, Bytes::from("result_x"));
    cache.put_simple(y, Bytes::from("result_y"));
    cache.put_simple(z, Bytes::from("result_z"));
    assert_eq!(cache.entry_count(), 4);

    let result = CascadeInvalidator::invalidate_sync(
        &dag, &mut cache, &a,
        CascadePolicy::Immediate, true, true,
    );

    assert_eq!(result.evicted_count, 4);
    assert_eq!(result.traversed_count, 3);
    assert_eq!(result.max_depth, 2);
    assert_eq!(cache.entry_count(), 0);
}

#[test]
fn cascade_immediate_no_include_root() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::with_max_size(1024 * 1024);

    let a = leaf_addr("a");
    let x = insert_recipe(&mut dag, "concat", vec![a]);

    cache.put_simple(a, Bytes::from("leaf_a"));
    cache.put_simple(x, Bytes::from("result_x"));

    let result = CascadeInvalidator::invalidate_sync(
        &dag, &mut cache, &a,
        CascadePolicy::Immediate, false, false,
    );

    assert_eq!(result.evicted_count, 1);
    assert!(cache.contains(&a));
    assert!(!cache.contains(&x));
}

#[test]
fn cascade_dry_run() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::with_max_size(1024 * 1024);

    let a = leaf_addr("a");
    let x = insert_recipe(&mut dag, "concat", vec![a]);
    let y = insert_recipe(&mut dag, "hash", vec![x]);

    cache.put_simple(x, Bytes::from("result_x"));
    cache.put_simple(y, Bytes::from("result_y"));

    let result = CascadeInvalidator::invalidate_sync(
        &dag, &mut cache, &a,
        CascadePolicy::DryRun, true, true,
    );

    assert_eq!(cache.entry_count(), 2); // unchanged
    assert!(cache.contains(&x));
    assert!(cache.contains(&y));
    assert_eq!(result.evicted_count, 2);
    assert_eq!(result.traversed_count, 2);
    assert_eq!(result.evicted_addrs.len(), 2);
}

#[test]
fn cascade_policy_none() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::with_max_size(1024 * 1024);

    let a = leaf_addr("a");
    let _x = insert_recipe(&mut dag, "concat", vec![a]);

    cache.put_simple(a, Bytes::from("leaf_a"));

    let result = CascadeInvalidator::invalidate_sync(
        &dag, &mut cache, &a,
        CascadePolicy::None, true, false,
    );

    assert_eq!(result.evicted_count, 1);
    assert_eq!(result.traversed_count, 0);
}

#[test]
fn cascade_no_dependents() {
    let dag = DagStore::new();
    let mut cache = EvictableCache::with_max_size(1024 * 1024);

    let a = leaf_addr("a");
    cache.put_simple(a, Bytes::from("leaf_a"));

    let result = CascadeInvalidator::invalidate_sync(
        &dag, &mut cache, &a,
        CascadePolicy::Immediate, true, false,
    );

    assert_eq!(result.evicted_count, 1);
    assert_eq!(result.traversed_count, 0);
    assert_eq!(result.max_depth, 0);
}

#[test]
fn cascade_deep_chain() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::with_max_size(1024 * 1024);

    let a = leaf_addr("a");
    cache.put_simple(a, Bytes::from("leaf_a"));
    let mut prev = a;
    for i in 0..20 {
        let r = insert_recipe(&mut dag, &format!("f{}", i), vec![prev]);
        cache.put_simple(r, Bytes::from(format!("result_{}", i)));
        prev = r;
    }

    let result = CascadeInvalidator::invalidate_sync(
        &dag, &mut cache, &a,
        CascadePolicy::Immediate, true, false,
    );

    assert_eq!(result.evicted_count, 21);
    assert_eq!(result.traversed_count, 20);
    assert_eq!(result.max_depth, 20);
    assert_eq!(cache.entry_count(), 0);
}

#[test]
fn cascade_partial_cache_population() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::with_max_size(1024 * 1024);

    let a = leaf_addr("a");
    let x = insert_recipe(&mut dag, "f1", vec![a]);
    let y = insert_recipe(&mut dag, "f2", vec![a]);
    let _z = insert_recipe(&mut dag, "f3", vec![x, y]);

    // Only X is cached
    cache.put_simple(x, Bytes::from("result_x"));

    let result = CascadeInvalidator::invalidate_sync(
        &dag, &mut cache, &a,
        CascadePolicy::Immediate, false, true,
    );

    assert_eq!(result.traversed_count, 3);
    assert_eq!(result.evicted_count, 1);
    assert_eq!(result.evicted_addrs, vec![x]);
}

#[test]
fn cascade_immediate_preserves_unrelated() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::with_max_size(1024 * 1024);

    let a = leaf_addr("a");
    let b = leaf_addr("b");
    let x = insert_recipe(&mut dag, "f1", vec![a]);
    let y = insert_recipe(&mut dag, "f2", vec![b]);

    cache.put_simple(x, Bytes::from("result_x"));
    cache.put_simple(y, Bytes::from("result_y"));

    CascadeInvalidator::invalidate_sync(
        &dag, &mut cache, &a,
        CascadePolicy::Immediate, false, false,
    );

    assert!(!cache.contains(&x));
    assert!(cache.contains(&y)); // unrelated, untouched
}
