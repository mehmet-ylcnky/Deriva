use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use bytes::Bytes;
use deriva_core::{CAddr, CacheConfig, EvictableCache, DagStore, FunctionId, Recipe};
use deriva_core::invalidation::CascadePolicy;
use deriva_compute::invalidation::CascadeInvalidator;
use std::collections::BTreeMap;

fn make_leaf(i: usize) -> CAddr {
    CAddr::from_bytes(&Bytes::from(format!("leaf_{i}")))
}

fn make_recipe(func: &str, inputs: Vec<CAddr>) -> Recipe {
    Recipe::new(FunctionId::new(func, "1.0.0"), inputs, BTreeMap::new())
}

/// A → R1 → R2 → ... → R{N}
fn build_linear_chain(n: usize) -> (DagStore, EvictableCache, CAddr) {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::new(CacheConfig::default());
    let root = make_leaf(0);
    cache.put_simple(root, Bytes::from("data"));

    let mut prev = root;
    for i in 1..=n {
        let r = make_recipe(&format!("f{i}"), vec![prev]);
        let addr = dag.insert(r).unwrap();
        cache.put_simple(addr, Bytes::from("data"));
        prev = addr;
    }
    (dag, cache, root)
}

/// A → [R1, R2, ..., R{N}]
fn build_wide_fanout(n: usize) -> (DagStore, EvictableCache, CAddr) {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::new(CacheConfig::default());
    let root = make_leaf(0);
    cache.put_simple(root, Bytes::from("data"));

    for i in 1..=n {
        let r = make_recipe(&format!("f{i}"), vec![root]);
        let addr = dag.insert(r).unwrap();
        cache.put_simple(addr, Bytes::from("data"));
    }
    (dag, cache, root)
}

/// Diamond pattern: depth D, width W per level
fn build_diamond(depth: usize, width: usize) -> (DagStore, EvictableCache, CAddr) {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::new(CacheConfig::default());
    let root = make_leaf(0);
    cache.put_simple(root, Bytes::from("data"));

    let mut prev_level = vec![root];
    for d in 1..=depth {
        let mut this_level = Vec::new();
        for w in 0..width {
            let r = make_recipe(&format!("d{d}_w{w}"), prev_level.clone());
            let addr = dag.insert(r).unwrap();
            cache.put_simple(addr, Bytes::from("data"));
            this_level.push(addr);
        }
        prev_level = this_level;
    }
    (dag, cache, root)
}

fn bench_linear_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("cascade_linear_chain");
    for n in [10, 100, 1000, 10000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || build_linear_chain(n),
                |(dag, mut cache, root)| {
                    black_box(CascadeInvalidator::invalidate_sync(
                        &dag, &mut cache, &root,
                        CascadePolicy::Immediate, true, false,
                    ))
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_wide_fanout(c: &mut Criterion) {
    let mut group = c.benchmark_group("cascade_wide_fanout");
    for n in [10, 100, 1000, 10000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || build_wide_fanout(n),
                |(dag, mut cache, root)| {
                    black_box(CascadeInvalidator::invalidate_sync(
                        &dag, &mut cache, &root,
                        CascadePolicy::Immediate, true, false,
                    ))
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_diamond(c: &mut Criterion) {
    let mut group = c.benchmark_group("cascade_diamond");
    for (d, w) in [(10, 2), (10, 10), (100, 2)] {
        let label = format!("d{d}_w{w}");
        group.bench_with_input(BenchmarkId::new("depth_width", &label), &(d, w), |b, &(d, w)| {
            b.iter_batched(
                || build_diamond(d, w),
                |(dag, mut cache, root)| {
                    black_box(CascadeInvalidator::invalidate_sync(
                        &dag, &mut cache, &root,
                        CascadePolicy::Immediate, true, false,
                    ))
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

criterion_group!(benches, bench_linear_chain, bench_wide_fanout, bench_diamond);
criterion_main!(benches);
