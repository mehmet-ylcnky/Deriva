use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::BTreeMap;
use tempfile::tempdir;
use deriva_core::address::{CAddr, FunctionId, Recipe};
use deriva_core::cache::{CacheConfig, EvictableCache};
use deriva_core::gc::{GcConfig, PinSet};
use deriva_core::persistent_dag::PersistentDag;
use deriva_compute::cache::SharedCache;
use deriva_storage::blob_store::BlobStore;
use deriva_server::gc::run_gc;

fn leaf(i: usize) -> CAddr {
    CAddr::from_bytes(format!("leaf_{i}").as_bytes())
}

struct GcSetup {
    dag: PersistentDag,
    blobs: BlobStore,
    cache: SharedCache,
    _dir: tempfile::TempDir,
    _db: sled::Db,
}

fn build(total_blobs: usize, recipe_count: usize, orphan_count: usize) -> GcSetup {
    let dir = tempdir().unwrap();
    let db = sled::Config::new().temporary(true).open().unwrap();
    let dag = PersistentDag::open(&db).unwrap();
    let blobs = BlobStore::open(dir.path().join("blobs")).unwrap();
    let cache = SharedCache::new(EvictableCache::new(CacheConfig::default()));

    // Create referenced blobs + recipes
    let referenced = total_blobs - orphan_count;
    for i in 0..referenced {
        let a = leaf(i);
        blobs.put(&a, &[0u8; 64]).unwrap();
        if i < recipe_count {
            let r = Recipe::new(FunctionId::new(format!("f{i}"), "1"), vec![a], BTreeMap::new());
            dag.insert(&r).unwrap();
        }
    }

    // Create orphan blobs
    for i in referenced..total_blobs {
        let a = leaf(i);
        blobs.put(&a, &[0u8; 64]).unwrap();
    }

    GcSetup { dag, blobs, cache, _dir: dir, _db: db }
}

fn bench_gc_sweep(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("gc_sweep");

    for &(blobs, recipes, orphans) in &[
        (100, 50, 50),
        (1000, 500, 500),
        (5000, 2500, 2500),
    ] {
        group.bench_with_input(
            BenchmarkId::new("blobs", blobs),
            &(blobs, recipes, orphans),
            |b, &(bl, rc, or)| {
                b.iter_with_setup(
                    || build(bl, rc, or),
                    |s| {
                        let cfg = GcConfig { grace_period: std::time::Duration::ZERO, ..Default::default() };
                        rt.block_on(run_gc(&s.dag, &s.blobs, &s.cache, &PinSet::new(), &cfg)).unwrap()
                    },
                );
            },
        );
    }
    group.finish();
}

fn bench_live_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("gc_live_set");

    for &recipe_count in &[100, 1000, 5000] {
        let db = sled::Config::new().temporary(true).open().unwrap();
        let dag = PersistentDag::open(&db).unwrap();
        for i in 0..recipe_count {
            let a = leaf(i);
            let r = Recipe::new(FunctionId::new(format!("f{i}"), "1"), vec![a], BTreeMap::new());
            dag.insert(&r).unwrap();
        }
        group.bench_with_input(
            BenchmarkId::new("recipes", recipe_count),
            &recipe_count,
            |b, _| {
                b.iter(|| dag.live_addr_set());
            },
        );
    }
    group.finish();
}

fn bench_list_addrs(c: &mut Criterion) {
    let mut group = c.benchmark_group("gc_list_addrs");

    for &count in &[100, 1000, 5000] {
        let dir = tempdir().unwrap();
        let blobs = BlobStore::open(dir.path().join("blobs")).unwrap();
        for i in 0..count {
            blobs.put(&leaf(i), &[0u8; 32]).unwrap();
        }

        group.bench_with_input(
            BenchmarkId::new("blobs", count),
            &count,
            |b, _| {
                b.iter(|| blobs.list_addrs().unwrap());
            },
        );
    }
    group.finish();
}

fn bench_dry_run_vs_actual(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("gc_dry_vs_actual");

    let size = 1000;
    group.bench_function("dry_run", |b| {
        b.iter_with_setup(
            || build(size, size / 2, size / 2),
            |s| {
                let cfg = GcConfig { grace_period: std::time::Duration::ZERO, dry_run: true, ..Default::default() };
                rt.block_on(run_gc(&s.dag, &s.blobs, &s.cache, &PinSet::new(), &cfg)).unwrap()
            },
        );
    });

    group.bench_function("actual", |b| {
        b.iter_with_setup(
            || build(size, size / 2, size / 2),
            |s| {
                let cfg = GcConfig { grace_period: std::time::Duration::ZERO, ..Default::default() };
                rt.block_on(run_gc(&s.dag, &s.blobs, &s.cache, &PinSet::new(), &cfg)).unwrap()
            },
        );
    });

    group.finish();
}

criterion_group!(benches, bench_gc_sweep, bench_live_set, bench_list_addrs, bench_dry_run_vs_actual);
criterion_main!(benches);
