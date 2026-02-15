use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, BatchSize};
use deriva_core::{CAddr, Recipe, FunctionId, Value};
use deriva_storage::{SledRecipeStore, BlobStore};
use bytes::Bytes;
use std::collections::BTreeMap;
use tempfile::TempDir;

// ============================================================================
// CAddr Computation Benchmarks
// ============================================================================

fn bench_caddr_computation(c: &mut Criterion) {
    let mut group = c.benchmark_group("caddr_computation");
    
    // Vary data sizes
    for size in [64, 256, 1024, 4096, 16384, 65536, 262144, 1048576].iter() {
        let data = Bytes::from(vec![0u8; *size]);
        
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                black_box(CAddr::from_bytes(&data))
            })
        });
    }
    
    group.finish();
}

// ============================================================================
// Recipe Store Benchmarks
// ============================================================================

fn bench_recipe_store_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("recipe_store_insert");
    
    for count in [10, 50, 100, 500, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            b.iter_batched(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let store = SledRecipeStore::open(temp_dir.path()).unwrap();
                    (store, temp_dir)
                },
                |(store, _temp_dir)| {
                    for i in 0..count {
                        let recipe = Recipe::new(
                            FunctionId::new("test", "v1"),
                            vec![],
                            BTreeMap::new(),
                        );
                        let addr = CAddr::from_bytes(&Bytes::from(format!("recipe_{}", i)));
                        store.put(&addr, &recipe).unwrap();
                    }
                },
                BatchSize::SmallInput,
            )
        });
    }
    
    group.finish();
}

fn bench_recipe_store_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("recipe_store_get");
    
    for count in [10, 50, 100, 500, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            b.iter_batched(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let store = SledRecipeStore::open(temp_dir.path()).unwrap();
                    let mut addrs = vec![];
                    
                    for i in 0..count {
                        let recipe = Recipe::new(
                            FunctionId::new("test", "v1"),
                            vec![],
                            BTreeMap::new(),
                        );
                        let addr = CAddr::from_bytes(&Bytes::from(format!("recipe_{}", i)));
                        store.put(&addr, &recipe).unwrap();
                        addrs.push(addr);
                    }
                    
                    (store, temp_dir, addrs)
                },
                |(store, _temp_dir, addrs)| {
                    for addr in addrs {
                        black_box(store.get(&addr).unwrap());
                    }
                },
                BatchSize::SmallInput,
            )
        });
    }
    
    group.finish();
}

fn bench_recipe_store_contains(c: &mut Criterion) {
    let mut group = c.benchmark_group("recipe_store_contains");
    
    for count in [100, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            b.iter_batched(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let store = SledRecipeStore::open(temp_dir.path()).unwrap();
                    let mut addrs = vec![];
                    
                    for i in 0..count {
                        let recipe = Recipe::new(
                            FunctionId::new("test", "v1"),
                            vec![],
                            BTreeMap::new(),
                        );
                        let addr = CAddr::from_bytes(&Bytes::from(format!("recipe_{}", i)));
                        store.put(&addr, &recipe).unwrap();
                        addrs.push(addr);
                    }
                    
                    (store, temp_dir, addrs)
                },
                |(store, _temp_dir, addrs)| {
                    for addr in addrs {
                        black_box(store.contains(&addr).unwrap());
                    }
                },
                BatchSize::SmallInput,
            )
        });
    }
    
    group.finish();
}

// ============================================================================
// Blob Store Benchmarks
// ============================================================================

fn bench_blob_store_put(c: &mut Criterion) {
    let mut group = c.benchmark_group("blob_store_put");
    
    for size in [64, 1024, 16384, 262144, 1048576, 10485760].iter() {
        let data = Bytes::from(vec![0u8; *size]);
        
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter_batched(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let store = BlobStore::open(temp_dir.path()).unwrap();
                    (store, temp_dir)
                },
                |(store, _temp_dir)| {
                    let addr = CAddr::from_bytes(&data);
                    store.put(&addr, &data).unwrap();
                },
                BatchSize::SmallInput,
            )
        });
    }
    
    group.finish();
}

fn bench_blob_store_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("blob_store_get");
    
    for size in [64, 1024, 16384, 262144, 1048576, 10485760].iter() {
        let data = Bytes::from(vec![0u8; *size]);
        
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter_batched(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let store = BlobStore::open(temp_dir.path()).unwrap();
                    let addr = CAddr::from_bytes(&data);
                    store.put(&addr, &data).unwrap();
                    (store, temp_dir, addr)
                },
                |(store, _temp_dir, addr)| {
                    black_box(store.get(&addr).unwrap());
                },
                BatchSize::SmallInput,
            )
        });
    }
    
    group.finish();
}

fn bench_blob_store_contains(c: &mut Criterion) {
    let mut group = c.benchmark_group("blob_store_contains");
    
    for count in [100, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            b.iter_batched(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let store = BlobStore::open(temp_dir.path()).unwrap();
                    let mut addrs = vec![];
                    
                    for i in 0..count {
                        let data = Bytes::from(format!("blob_{}", i));
                        let addr = CAddr::from_bytes(&data);
                        store.put(&addr, &data).unwrap();
                        addrs.push(addr);
                    }
                    
                    (store, temp_dir, addrs)
                },
                |(store, _temp_dir, addrs)| {
                    for addr in addrs {
                        black_box(store.contains(&addr));
                    }
                },
                BatchSize::SmallInput,
            )
        });
    }
    
    group.finish();
}

// ============================================================================
// DAG Rebuild Benchmarks (Phase 1 Baseline)
// ============================================================================

fn bench_dag_rebuild(c: &mut Criterion) {
    let mut group = c.benchmark_group("dag_rebuild");
    group.sample_size(20); // Fewer samples for expensive operations
    
    for count in [10, 50, 100, 500, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            b.iter_batched(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let store = SledRecipeStore::open(temp_dir.path()).unwrap();
                    
                    // Populate with recipes
                    for i in 0..count {
                        let recipe = Recipe::new(
                            FunctionId::new("test", "v1"),
                            vec![],
                            BTreeMap::new(),
                        );
                        let addr = CAddr::from_bytes(&Bytes::from(format!("recipe_{}", i)));
                        store.put(&addr, &recipe).unwrap();
                    }
                    
                    (store, temp_dir)
                },
                |(store, _temp_dir)| {
                    // This simulates Phase 1 startup: rebuild DAG from all recipes
                    black_box(store.rebuild_dag().unwrap());
                },
                BatchSize::SmallInput,
            )
        });
    }
    
    group.finish();
}

// ============================================================================
// Recipe Serialization Benchmarks
// ============================================================================

fn bench_recipe_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("recipe_serialization");
    
    // Simple recipe (no inputs, no params)
    let simple_recipe = Recipe::new(
        FunctionId::new("test", "v1"),
        vec![],
        BTreeMap::new(),
    );
    
    group.bench_function("simple_serialize", |b| {
        b.iter(|| {
            black_box(simple_recipe.canonical_bytes())
        })
    });
    
    // Complex recipe (10 inputs, 10 params)
    let mut params = BTreeMap::new();
    for i in 0..10 {
        params.insert(format!("param_{}", i), Value::Int(i as i64));
    }
    let complex_recipe = Recipe::new(
        FunctionId::new("complex_function", "v2"),
        vec![CAddr::from_bytes(&Bytes::from("input")); 10],
        params,
    );
    
    group.bench_function("complex_serialize", |b| {
        b.iter(|| {
            black_box(complex_recipe.canonical_bytes())
        })
    });
    
    // Recipe with large params
    let mut large_params = BTreeMap::new();
    large_params.insert("data".to_string(), Value::Bytes(vec![0u8; 10240]));
    let large_recipe = Recipe::new(
        FunctionId::new("large_function", "v1"),
        vec![],
        large_params,
    );
    
    group.bench_function("large_params_serialize", |b| {
        b.iter(|| {
            black_box(large_recipe.canonical_bytes())
        })
    });
    
    group.finish();
}

// ============================================================================
// Recipe Address Computation Benchmarks
// ============================================================================

fn bench_recipe_addr_computation(c: &mut Criterion) {
    let mut group = c.benchmark_group("recipe_addr");
    
    // Vary recipe complexity
    for input_count in [0, 1, 5, 10, 50, 100].iter() {
        let recipe = Recipe::new(
            FunctionId::new("test", "v1"),
            vec![CAddr::from_bytes(&Bytes::from("input")); *input_count],
            BTreeMap::new(),
        );
        
        group.bench_with_input(
            BenchmarkId::new("inputs", input_count),
            input_count,
            |b, _| {
                b.iter(|| {
                    black_box(recipe.addr())
                })
            },
        );
    }
    
    group.finish();
}

// ============================================================================
// Memory Usage Benchmarks
// ============================================================================

fn bench_memory_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_overhead");
    
    // Measure memory overhead of storing recipes in DAG vs just in sled
    for count in [100, 1000, 5000].iter() {
        group.bench_with_input(
            BenchmarkId::new("dag_memory", count),
            count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let temp_dir = TempDir::new().unwrap();
                        let store = SledRecipeStore::open(temp_dir.path()).unwrap();
                        
                        for i in 0..count {
                            let recipe = Recipe::new(
                                FunctionId::new("test", "v1"),
                                vec![],
                                BTreeMap::new(),
                            );
                            let addr = CAddr::from_bytes(&Bytes::from(format!("recipe_{}", i)));
                            store.put(&addr, &recipe).unwrap();
                        }
                        
                        (store, temp_dir)
                    },
                    |(store, _temp_dir)| {
                        let dag = store.rebuild_dag().unwrap();
                        black_box(dag);
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    
    group.finish();
}

// ============================================================================
// Concurrent Access Benchmarks
// ============================================================================

fn bench_concurrent_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_reads");
    
    for count in [100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let temp_dir = TempDir::new().unwrap();
                        let store = SledRecipeStore::open(temp_dir.path()).unwrap();
                        let mut addrs = vec![];
                        
                        for i in 0..count {
                            let recipe = Recipe::new(
                                FunctionId::new("test", "v1"),
                                vec![],
                                BTreeMap::new(),
                            );
                            let addr = CAddr::from_bytes(&Bytes::from(format!("recipe_{}", i)));
                            store.put(&addr, &recipe).unwrap();
                            addrs.push(addr);
                        }
                        
                        (store, temp_dir, addrs)
                    },
                    |(store, _temp_dir, addrs)| {
                        // Simulate concurrent reads
                        for addr in addrs {
                            black_box(store.get(&addr).unwrap());
                        }
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_caddr_computation,
    bench_recipe_store_insert,
    bench_recipe_store_get,
    bench_recipe_store_contains,
    bench_blob_store_put,
    bench_blob_store_get,
    bench_blob_store_contains,
    bench_dag_rebuild,
    bench_recipe_serialization,
    bench_recipe_addr_computation,
    bench_memory_overhead,
    bench_concurrent_reads,
);

criterion_main!(benches);
