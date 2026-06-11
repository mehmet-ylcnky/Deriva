//! Property-based tests for garbage collection (§2.8).
//!
//! Covers all 11 correctness properties from the GC design specification.

use std::collections::{BTreeMap, HashSet};

use bytes::Bytes;
use proptest::prelude::*;
use tempfile::tempdir;

use deriva_core::address::{CAddr, FunctionId, Recipe};
use deriva_core::cache::{CacheConfig, EvictableCache};
use deriva_core::gc::{GcConfig, PinSet};
use deriva_core::persistent_dag::PersistentDag;
use deriva_compute::cache::{AsyncMaterializationCache, SharedCache};
use deriva_server::gc::run_gc;
use deriva_storage::blob_store::BlobStore;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn setup() -> (PersistentDag, BlobStore, SharedCache, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db = sled::Config::new().temporary(true).open().unwrap();
    let dag = PersistentDag::open(&db).unwrap();
    let blobs = BlobStore::open(dir.path().join("blobs")).unwrap();
    let cache = SharedCache::new(EvictableCache::new(CacheConfig::default()));
    (dag, blobs, cache, dir)
}

fn make_addr(seed: u8) -> CAddr {
    CAddr::from_raw([seed; 32])
}

fn insert_recipe(dag: &PersistentDag, name: &str, inputs: Vec<CAddr>) -> CAddr {
    let r = Recipe::new(FunctionId::new(name, "1"), inputs, BTreeMap::new());
    dag.insert(&r).unwrap()
}

fn gc_cfg() -> GcConfig {
    GcConfig {
        grace_period: std::time::Duration::ZERO,
        dry_run: false,
        detail_addrs: false,
        max_removals: 0,
    }
}

// ─── Strategies ─────────────────────────────────────────────────────────────

/// Strategy for a unique set of seed bytes (used to create distinct CAddrs).
fn addr_seeds(max_count: usize) -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::hash_set(1u8..=254, 1..=max_count)
        .prop_map(|s| s.into_iter().collect())
}

/// Strategy for pin/unpin operations on a given set of addrs.
/// Each entry is (index_into_addrs, true=pin | false=unpin).
fn pin_ops(max_addrs: usize) -> impl Strategy<Value = Vec<(usize, bool)>> {
    proptest::collection::vec((0..max_addrs, any::<bool>()), 0..20)
}

// ─── Property 1: PinSet round-trip consistency ──────────────────────────────
// **Validates: Requirements 2.8**
// pin/unpin sequences: is_pinned ↔ addr ∈ list(), count() == list().len()

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_pinset_roundtrip(
        seeds in addr_seeds(10),
        ops in pin_ops(10),
    ) {
        let addrs: Vec<CAddr> = seeds.iter().map(|s| make_addr(*s)).collect();
        let mut pins = PinSet::new();

        for (idx, do_pin) in &ops {
            let idx = *idx % addrs.len();
            let addr = addrs[idx];
            if *do_pin {
                pins.pin(addr);
            } else {
                pins.unpin(&addr);
            }
        }

        // Invariant: is_pinned ↔ addr ∈ list()
        let listed: HashSet<CAddr> = pins.list().into_iter().collect();
        for addr in &addrs {
            prop_assert_eq!(pins.is_pinned(addr), listed.contains(addr));
        }

        // Invariant: count() == list().len()
        prop_assert_eq!(pins.count(), listed.len());
    }
}

// ─── Property 2: BlobStore remove returns correct byte counts ───────────────
// **Validates: Requirements 2.8**
// store blobs, remove, verify sizes match

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_blobstore_remove_byte_counts(
        blob_data in proptest::collection::vec(
            proptest::collection::vec(1u8..=255, 1..100),
            1..8
        ),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_dag, blobs, _cache, _dir) = setup();

            let mut expected_sizes: Vec<(CAddr, u64)> = Vec::new();
            for (i, data) in blob_data.iter().enumerate() {
                let addr = make_addr((i + 1) as u8);
                blobs.put(&addr, data).unwrap();
                expected_sizes.push((addr, data.len() as u64));
            }

            let addrs: Vec<CAddr> = expected_sizes.iter().map(|(a, _)| *a).collect();
            let (count, bytes) = blobs.remove_batch_blobs(&addrs).unwrap();

            let total_expected: u64 = expected_sizes.iter().map(|(_, s)| *s).sum();
            prop_assert_eq!(count, addrs.len() as u64);
            prop_assert_eq!(bytes, total_expected);
            Ok(())
        })?;
    }
}

// ─── Property 3: BlobStore enumeration completeness ─────────────────────────
// **Validates: Requirements 2.8**
// store blobs, list_addrs contains all stored CAddrs

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_blobstore_enumeration_completeness(
        seeds in addr_seeds(8),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_dag, blobs, _cache, _dir) = setup();

            let mut stored_addrs = HashSet::new();
            for seed in &seeds {
                let addr = make_addr(*seed);
                blobs.put(&addr, &[*seed; 10]).unwrap();
                stored_addrs.insert(addr);
            }

            let listed: HashSet<CAddr> = blobs.list_addrs().unwrap().into_iter().collect();
            for addr in &stored_addrs {
                prop_assert!(listed.contains(addr), "listed missing stored addr {:?}", addr);
            }
            prop_assert_eq!(listed.len(), stored_addrs.len());
            Ok(())
        })?;
    }
}

// ─── Property 4: Live set is union of DAG refs and pins ─────────────────────
// **Validates: Requirements 2.8**
// computed live set = recipe outputs ∪ recipe inputs ∪ pinned

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_live_set_union(
        input_seeds in addr_seeds(5),
        pin_seeds in addr_seeds(3),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, blobs, _cache, _dir) = setup();

            // Create input blobs and recipe depending on them
            let inputs: Vec<CAddr> = input_seeds.iter().map(|s| make_addr(*s)).collect();
            for addr in &inputs {
                blobs.put(addr, b"input-data").unwrap();
            }
            let recipe_output = insert_recipe(&dag, "compute", inputs.clone());

            // Pin some addrs
            let mut pins = PinSet::new();
            let pinned_addrs: Vec<CAddr> = pin_seeds.iter().map(|s| make_addr(*s)).collect();
            for addr in &pinned_addrs {
                pins.pin(*addr);
            }

            // Compute expected live set
            let mut expected_live: HashSet<CAddr> = HashSet::new();
            // Recipe outputs
            expected_live.insert(recipe_output);
            // Recipe inputs
            for addr in &inputs {
                expected_live.insert(*addr);
            }
            // Pins
            for addr in &pinned_addrs {
                expected_live.insert(*addr);
            }

            // Compute actual live set (same logic as run_gc)
            let mut actual_live = dag.live_addr_set();
            for addr in pins.as_set() {
                actual_live.insert(*addr);
            }

            // Expected is subset of actual
            for addr in &expected_live {
                prop_assert!(actual_live.contains(addr), "live set missing expected addr {:?}", addr);
            }
            Ok(())
        })?;
    }
}

// ─── Property 5: GC safety — live blobs never removed ──────────────────────
// **Validates: Requirements 2.8**
// after GC, all live blobs remain in BlobStore

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_gc_safety_live_blobs_never_removed(
        live_seeds in addr_seeds(5),
        orphan_seeds in proptest::collection::hash_set(200u8..=254, 0..5)
            .prop_map(|s| s.into_iter().collect::<Vec<_>>()),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, blobs, cache, _dir) = setup();

            // Store live blobs and register them in a recipe
            let live_addrs: Vec<CAddr> = live_seeds.iter().map(|s| make_addr(*s)).collect();
            for addr in &live_addrs {
                blobs.put(addr, b"live-data").unwrap();
            }
            let _recipe_out = insert_recipe(&dag, "f", live_addrs.clone());

            // Store orphan blobs (not referenced by any recipe)
            for seed in &orphan_seeds {
                let addr = make_addr(*seed);
                blobs.put(&addr, b"orphan-data").unwrap();
            }

            // Run GC
            let pins = PinSet::new();
            let _result = run_gc(&dag, &blobs, &cache, &pins, &gc_cfg()).await.unwrap();

            // All live blobs must still exist
            for addr in &live_addrs {
                prop_assert!(
                    blobs.get(addr).unwrap().is_some(),
                    "live blob {:?} was incorrectly removed by GC",
                    addr
                );
            }
            Ok(())
        })?;
    }
}

// ─── Property 6: GC liveness — orphaned blobs removed ──────────────────────
// **Validates: Requirements 2.8**
// after GC with max_removals=0, orphans gone

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_gc_liveness_orphans_removed(
        orphan_seeds in proptest::collection::hash_set(1u8..=254, 1..8)
            .prop_map(|s| s.into_iter().collect::<Vec<_>>()),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, blobs, cache, _dir) = setup();

            // Store only orphan blobs (no recipes reference them)
            let orphan_addrs: Vec<CAddr> = orphan_seeds.iter().map(|s| make_addr(*s)).collect();
            for addr in &orphan_addrs {
                blobs.put(addr, b"orphan").unwrap();
            }

            // GC with max_removals=0 means unlimited
            let cfg = gc_cfg(); // max_removals=0 by default
            let _result = run_gc(&dag, &blobs, &cache, &PinSet::new(), &cfg).await.unwrap();

            // All orphans should be gone
            for addr in &orphan_addrs {
                prop_assert!(
                    blobs.get(addr).unwrap().is_none(),
                    "orphan blob {:?} was not removed by GC",
                    addr
                );
            }
            Ok(())
        })?;
    }
}

// ─── Property 7: Dry-run non-destructive ────────────────────────────────────
// **Validates: Requirements 2.8**
// dry_run=true doesn't delete anything from BlobStore

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_gc_dry_run_non_destructive(
        orphan_seeds in proptest::collection::hash_set(1u8..=254, 1..8)
            .prop_map(|s| s.into_iter().collect::<Vec<_>>()),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, blobs, cache, _dir) = setup();

            let orphan_addrs: Vec<CAddr> = orphan_seeds.iter().map(|s| make_addr(*s)).collect();
            for addr in &orphan_addrs {
                blobs.put(addr, b"data").unwrap();
            }

            let mut cfg = gc_cfg();
            cfg.dry_run = true;

            let result = run_gc(&dag, &blobs, &cache, &PinSet::new(), &cfg).await.unwrap();

            // Dry run should report removals but not actually delete
            prop_assert!(result.dry_run);
            for addr in &orphan_addrs {
                prop_assert!(
                    blobs.get(addr).unwrap().is_some(),
                    "dry_run deleted blob {:?}",
                    addr
                );
            }
            Ok(())
        })?;
    }
}

// ─── Property 8: Max-removals bounds ────────────────────────────────────────
// **Validates: Requirements 2.8**
// blobs_removed <= max_removals

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_gc_max_removals_bounds(
        orphan_count in 3usize..12,
        max_removals in 1usize..6,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, blobs, cache, _dir) = setup();

            for i in 0..orphan_count {
                let addr = make_addr((i + 1) as u8);
                blobs.put(&addr, &[i as u8; 20]).unwrap();
            }

            let mut cfg = gc_cfg();
            cfg.max_removals = max_removals;

            let result = run_gc(&dag, &blobs, &cache, &PinSet::new(), &cfg).await.unwrap();

            prop_assert!(
                result.blobs_removed <= max_removals as u64,
                "blobs_removed {} exceeded max_removals {}",
                result.blobs_removed,
                max_removals
            );
            Ok(())
        })?;
    }
}

// ─── Property 9: Orphaned recipe pin protection ─────────────────────────────
// **Validates: Requirements 2.8**
// pinned recipe outputs survive even if input is orphaned

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_pinned_recipe_output_survives(
        input_seeds in addr_seeds(3),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, blobs, cache, _dir) = setup();

            // Create input blobs referenced by a recipe
            let inputs: Vec<CAddr> = input_seeds.iter().map(|s| make_addr(*s)).collect();
            for addr in &inputs {
                blobs.put(addr, b"input").unwrap();
            }
            let recipe_output = insert_recipe(&dag, "pinned-fn", inputs.clone());

            // Pin the recipe output
            let mut pins = PinSet::new();
            pins.pin(recipe_output);

            // Also store the recipe output as a blob to verify it stays
            blobs.put(&recipe_output, b"output-data").unwrap();

            let result = run_gc(&dag, &blobs, &cache, &pins, &gc_cfg()).await.unwrap();

            // Pinned recipe output blob should survive
            prop_assert!(
                blobs.get(&recipe_output).unwrap().is_some(),
                "pinned recipe output was removed"
            );

            // The recipe should not be in the orphaned recipes list
            // (because it's pinned, the code skips it in orphan detection)
            prop_assert_eq!(result.recipes_removed, 0);
            Ok(())
        })?;
    }
}

// ─── Property 10: total_bytes_reclaimed invariant ───────────────────────────
// **Validates: Requirements 2.8**
// total == bytes_reclaimed_blobs + bytes_reclaimed_cache

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_total_bytes_reclaimed_invariant(
        orphan_seeds in proptest::collection::hash_set(1u8..=200, 1..8)
            .prop_map(|s| s.into_iter().collect::<Vec<_>>()),
        cache_seeds in proptest::collection::hash_set(201u8..=254, 0..4)
            .prop_map(|s| s.into_iter().collect::<Vec<_>>()),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, blobs, cache, _dir) = setup();

            // Store orphan blobs
            for seed in &orphan_seeds {
                let addr = make_addr(*seed);
                blobs.put(&addr, &[*seed; 15]).unwrap();
            }

            // Pre-populate cache with some of the orphan addrs
            for seed in &cache_seeds {
                let addr = make_addr(*seed);
                blobs.put(&addr, &[*seed; 10]).unwrap();
                cache.put(addr, Bytes::from(vec![*seed; 10])).await;
            }

            let result = run_gc(&dag, &blobs, &cache, &PinSet::new(), &gc_cfg()).await.unwrap();

            prop_assert_eq!(
                result.total_bytes_reclaimed,
                result.bytes_reclaimed_blobs + result.bytes_reclaimed_cache,
                "total_bytes_reclaimed ({}) != blobs ({}) + cache ({})",
                result.total_bytes_reclaimed,
                result.bytes_reclaimed_blobs,
                result.bytes_reclaimed_cache
            );
            Ok(())
        })?;
    }
}

// ─── Property 11: Detail addrs completeness ─────────────────────────────────
// **Validates: Requirements 2.8**
// removed_addrs.len() == blobs_removed + recipes_removed when detail_addrs=true

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_detail_addrs_completeness(
        orphan_seeds in proptest::collection::hash_set(1u8..=254, 1..8)
            .prop_map(|s| s.into_iter().collect::<Vec<_>>()),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (dag, blobs, cache, _dir) = setup();

            for seed in &orphan_seeds {
                let addr = make_addr(*seed);
                blobs.put(&addr, &[*seed; 5]).unwrap();
            }

            let mut cfg = gc_cfg();
            cfg.detail_addrs = true;

            let result = run_gc(&dag, &blobs, &cache, &PinSet::new(), &cfg).await.unwrap();

            let expected_len = (result.blobs_removed + result.recipes_removed) as usize;
            prop_assert_eq!(
                result.removed_addrs.len(),
                expected_len,
                "removed_addrs.len() ({}) != blobs_removed ({}) + recipes_removed ({})",
                result.removed_addrs.len(),
                result.blobs_removed,
                result.recipes_removed
            );
            Ok(())
        })?;
    }
}
