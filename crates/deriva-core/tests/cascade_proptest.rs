//! Property-based tests for cascade invalidation correctness.
//!
//! Tests cover: CascadePolicy behavior, BFS traversal, batch removal invariants,
//! include_root/detail_addrs flags, async/sync equivalence, policy parsing, and
//! cache contains accuracy.

use bytes::Bytes;
use std::collections::{BTreeMap, HashSet};

use deriva_core::address::{CAddr, FunctionId, Recipe};
use deriva_core::cache::EvictableCache;
use deriva_core::dag::DagStore;
use deriva_core::invalidation::CascadePolicy;
use deriva_compute::invalidation::CascadeInvalidator;

use proptest::prelude::*;

// --- Helpers ---

fn leaf_addr(seed: u8) -> CAddr {
    CAddr::from_bytes(&[seed])
}

fn make_recipe(func_name: &str, inputs: Vec<CAddr>) -> Recipe {
    Recipe::new(
        FunctionId::new(func_name, "v1"),
        inputs,
        BTreeMap::new(),
    )
}

/// Strategy to generate a random DAG with 1..=max_nodes derived nodes building on a single root.
/// Returns (DagStore, root_addr, all_addrs_including_root).
fn arb_dag(max_nodes: usize) -> impl Strategy<Value = (DagStore, CAddr, Vec<CAddr>)> {
    // Generate root seed and number of derived nodes
    (any::<u8>(), 1..=max_nodes).prop_flat_map(move |(root_seed, node_count)| {
        // Generate unique function names for each derived node
        let names: Vec<String> = (0..node_count).map(|i| format!("f{}", i)).collect();
        // For each node, pick which previous nodes (by index) it depends on
        let input_strategies: Vec<_> = (0..node_count)
            .map(|i| {
                // Each node can depend on the root (index 0) or any previous derived node
                let available = i + 1; // root + i derived nodes so far
                prop::collection::vec(0..available, 1..=available.min(3))
            })
            .collect();
        (Just(root_seed), Just(names), input_strategies)
    }).prop_map(|(root_seed, names, input_indices_vec)| {
        let dag = DagStore::new();
        let root = leaf_addr(root_seed);
        let mut all_addrs = vec![root];

        for (i, input_indices) in input_indices_vec.iter().enumerate() {
            let inputs: Vec<CAddr> = input_indices.iter()
                .map(|&idx| all_addrs[idx])
                .collect::<HashSet<_>>() // deduplicate
                .into_iter()
                .collect();
            let recipe = make_recipe(&names[i], inputs);
            let addr = dag.insert(&recipe).unwrap();
            all_addrs.push(addr);
        }

        (dag, root, all_addrs)
    })
}

/// Parse cascade policy from string (mirrors server-side logic).
fn parse_cascade_policy(s: &str) -> CascadePolicy {
    match s.to_lowercase().as_str() {
        "none" => CascadePolicy::None,
        "immediate" => CascadePolicy::Immediate,
        "dry_run" | "dryrun" => CascadePolicy::DryRun,
        _ => CascadePolicy::Immediate,
    }
}

// --- Properties ---

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Property 1: Policy::None performs no traversal**
    /// CascadePolicy::None with include_root=true: traversed_count==0, max_depth==0, only root evicted.
    ///
    /// **Validates: Requirements 2.6**
    #[test]
    fn policy_none_performs_no_traversal(
        (dag, root, all_addrs) in arb_dag(10),
        cache_flags in prop::collection::vec((any::<bool>(), 1..=50u8), 1..=11),
    ) {
        // Build cache with all addrs from dag
        let mut cache = EvictableCache::with_max_size(1024 * 1024);
        for (i, addr) in all_addrs.iter().enumerate() {
            if i < cache_flags.len() && cache_flags[i].0 {
                let data = vec![0u8; cache_flags[i].1 as usize];
                cache.put_simple(*addr, Bytes::from(data));
            }
        }

        let root_was_present = cache.contains(&root);
        let result = CascadeInvalidator::invalidate_sync(
            &dag, &mut cache, &root, CascadePolicy::None, true, true,
        );

        prop_assert_eq!(result.traversed_count, 0, "None policy must not traverse");
        prop_assert_eq!(result.max_depth, 0, "None policy has zero depth");
        if root_was_present {
            prop_assert_eq!(result.evicted_count, 1, "Only root should be evicted");
            prop_assert!(!cache.contains(&root), "Root should be removed");
        } else {
            prop_assert_eq!(result.evicted_count, 0, "Nothing to evict if root absent");
        }
    }

    /// **Property 2: DryRun is non-mutating preview of Immediate**
    /// Same DAG/cache: DryRun reports same evicted_count/traversed_count as Immediate,
    /// and cache unchanged after DryRun.
    ///
    /// **Validates: Requirements 2.6**
    #[test]
    fn dryrun_is_nonmutating_preview(
        (dag, root, all_addrs) in arb_dag(8),
        cache_flags in prop::collection::vec((any::<bool>(), 1..=50u8), 1..=9),
    ) {
        // Build two identical caches
        let mut cache_dry = EvictableCache::with_max_size(1024 * 1024);
        let mut cache_imm = EvictableCache::with_max_size(1024 * 1024);
        for (i, addr) in all_addrs.iter().enumerate() {
            if i < cache_flags.len() && cache_flags[i].0 {
                let data = vec![0u8; cache_flags[i].1 as usize];
                cache_dry.put_simple(*addr, Bytes::from(data.clone()));
                cache_imm.put_simple(*addr, Bytes::from(data));
            }
        }

        let size_before = cache_dry.current_size();
        let count_before = cache_dry.entry_count();

        let dry_result = CascadeInvalidator::invalidate_sync(
            &dag, &mut cache_dry, &root, CascadePolicy::DryRun, true, true,
        );
        let imm_result = CascadeInvalidator::invalidate_sync(
            &dag, &mut cache_imm, &root, CascadePolicy::Immediate, true, true,
        );

        // DryRun should not mutate cache
        prop_assert_eq!(cache_dry.current_size(), size_before, "DryRun must not change size");
        prop_assert_eq!(cache_dry.entry_count(), count_before, "DryRun must not change entry count");

        // DryRun and Immediate should report same traversal and eviction counts
        prop_assert_eq!(dry_result.traversed_count, imm_result.traversed_count,
            "DryRun and Immediate must traverse same count");
        prop_assert_eq!(dry_result.evicted_count, imm_result.evicted_count,
            "DryRun and Immediate must report same evicted count");
    }

    /// **Property 3: BFS traversal correctness**
    /// All returned addrs reachable from root via reverse edges, no duplicates, root excluded,
    /// max_depth correct.
    ///
    /// **Validates: Requirements 2.6**
    #[test]
    fn bfs_traversal_correctness(
        (dag, root, _all_addrs) in arb_dag(12),
    ) {
        let (dependents, max_depth) = dag.transitive_dependents_with_depth(&root);

        // No duplicates
        let addrs: Vec<CAddr> = dependents.iter().map(|(a, _)| *a).collect();
        let unique: HashSet<&CAddr> = addrs.iter().collect();
        prop_assert_eq!(addrs.len(), unique.len(), "No duplicate addresses in BFS result");

        // Root excluded
        prop_assert!(!addrs.contains(&root), "Root must not appear in dependents");

        // Max depth matches reported
        let actual_max = dependents.iter().map(|(_, d)| *d).max().unwrap_or(0);
        prop_assert_eq!(max_depth, actual_max, "max_depth must match actual maximum");

        // All addrs reachable from root (verify via direct_dependents path)
        for (addr, _depth) in &dependents {
            // Verify each returned addr is in the transitive_dependents set
            let all_deps = dag.transitive_dependents(&root);
            prop_assert!(all_deps.contains(addr),
                "Each BFS result must be reachable from root via reverse edges");
        }
    }

    /// **Property 4: Batch remove accuracy**
    /// remove_batch removes exactly the intersection of list and cache keyset.
    ///
    /// **Validates: Requirements 2.6**
    #[test]
    fn batch_remove_accuracy(
        present_seeds in prop::collection::vec(0..200u8, 1..=20),
        request_seeds in prop::collection::vec(0..200u8, 1..=20),
    ) {
        let mut cache = EvictableCache::with_max_size(1024 * 1024);
        let present_addrs: Vec<CAddr> = present_seeds.iter()
            .map(|s| CAddr::from_bytes(&[*s, 0]))
            .collect();
        let request_addrs: Vec<CAddr> = request_seeds.iter()
            .map(|s| CAddr::from_bytes(&[*s, 0]))
            .collect();

        // Insert present addrs into cache
        let present_set: HashSet<CAddr> = present_addrs.iter().copied().collect();
        for addr in &present_set {
            cache.put_simple(*addr, Bytes::from(vec![1u8; 10]));
        }

        let request_set: HashSet<CAddr> = request_addrs.iter().copied().collect();
        let expected_removed: HashSet<CAddr> = present_set.intersection(&request_set).copied().collect();

        let (count, _bytes, removed) = cache.remove_batch(&request_addrs);

        let removed_set: HashSet<CAddr> = removed.into_iter().collect();
        let expected_count = expected_removed.len() as u64;
        prop_assert_eq!(removed_set, expected_removed,
            "remove_batch must remove exactly the intersection");
        prop_assert_eq!(count, expected_count);
    }

    /// **Property 5: Cache size invariant after batch remove**
    /// new current_size == old current_size - bytes_reclaimed.
    ///
    /// **Validates: Requirements 2.6**
    #[test]
    fn cache_size_invariant_after_batch_remove(
        seeds in prop::collection::vec(0..50u8, 1..=15),
        sizes in prop::collection::vec(1..=100u16, 1..=15),
        remove_indices in prop::collection::vec(0..15usize, 0..=10),
    ) {
        let mut cache = EvictableCache::with_max_size(1024 * 1024);
        let addrs: Vec<CAddr> = seeds.iter().map(|s| CAddr::from_bytes(&[*s, 1])).collect();

        for (i, addr) in addrs.iter().enumerate() {
            if i < sizes.len() {
                let data = vec![0u8; sizes[i] as usize];
                cache.put_simple(*addr, Bytes::from(data));
            }
        }

        let size_before = cache.current_size();
        let to_remove: Vec<CAddr> = remove_indices.iter()
            .filter(|&&i| i < addrs.len())
            .map(|&i| addrs[i])
            .collect();

        let (_count, bytes_reclaimed, _removed) = cache.remove_batch(&to_remove);
        let size_after = cache.current_size();

        prop_assert_eq!(size_after, size_before - bytes_reclaimed,
            "new_size must equal old_size - bytes_reclaimed");
    }

    /// **Property 6: include_root flag controls root eviction**
    /// Root absent after Immediate+include_root=true, root remains after include_root=false.
    ///
    /// **Validates: Requirements 2.6**
    #[test]
    fn include_root_flag_controls_eviction(
        (dag, root, all_addrs) in arb_dag(6),
        cache_flags in prop::collection::vec(1..=50u8, 1..=7),
    ) {
        // Build cache with root definitely present
        let mut cache_include = EvictableCache::with_max_size(1024 * 1024);
        let mut cache_exclude = EvictableCache::with_max_size(1024 * 1024);
        cache_include.put_simple(root, Bytes::from(vec![42u8; 10]));
        cache_exclude.put_simple(root, Bytes::from(vec![42u8; 10]));

        for (i, addr) in all_addrs.iter().skip(1).enumerate() {
            if i < cache_flags.len() {
                let data = vec![0u8; cache_flags[i] as usize];
                cache_include.put_simple(*addr, Bytes::from(data.clone()));
                cache_exclude.put_simple(*addr, Bytes::from(data));
            }
        }

        CascadeInvalidator::invalidate_sync(
            &dag, &mut cache_include, &root, CascadePolicy::Immediate, true, false,
        );
        CascadeInvalidator::invalidate_sync(
            &dag, &mut cache_exclude, &root, CascadePolicy::Immediate, false, false,
        );

        prop_assert!(!cache_include.contains(&root),
            "Root should be absent after include_root=true");
        prop_assert!(cache_exclude.contains(&root),
            "Root should remain after include_root=false");
    }

    /// **Property 7: detail_addrs flag controls evicted_addrs**
    /// evicted_addrs populated when true, empty when false.
    ///
    /// **Validates: Requirements 2.6**
    #[test]
    fn detail_addrs_flag_controls_output(
        (dag, root, all_addrs) in arb_dag(6),
        cache_flags in prop::collection::vec(1..=50u8, 1..=7),
    ) {
        // Build two identical caches
        let mut cache_detail = EvictableCache::with_max_size(1024 * 1024);
        let mut cache_no_detail = EvictableCache::with_max_size(1024 * 1024);
        for (i, addr) in all_addrs.iter().enumerate() {
            if i < cache_flags.len() {
                let data = vec![0u8; cache_flags[i] as usize];
                cache_detail.put_simple(*addr, Bytes::from(data.clone()));
                cache_no_detail.put_simple(*addr, Bytes::from(data));
            }
        }

        let result_with = CascadeInvalidator::invalidate_sync(
            &dag, &mut cache_detail, &root, CascadePolicy::Immediate, true, true,
        );
        let result_without = CascadeInvalidator::invalidate_sync(
            &dag, &mut cache_no_detail, &root, CascadePolicy::Immediate, true, false,
        );

        if result_with.evicted_count > 0 {
            prop_assert!(!result_with.evicted_addrs.is_empty(),
                "detail_addrs=true should populate evicted_addrs when items evicted");
        }
        prop_assert!(result_without.evicted_addrs.is_empty(),
            "detail_addrs=false should always produce empty evicted_addrs");
        // Both should evict the same count
        prop_assert_eq!(result_with.evicted_count, result_without.evicted_count);
    }

    /// **Property 8: Async/sync equivalence**
    /// Both variants produce same counts for same inputs (use tokio::runtime).
    ///
    /// **Validates: Requirements 2.6**
    #[test]
    fn async_sync_equivalence(
        (dag, root, all_addrs) in arb_dag(6),
        cache_flags in prop::collection::vec((any::<bool>(), 1..=50u8), 1..=7),
    ) {
        use std::sync::Arc;
        use deriva_compute::cache::SharedCache;

        // Build identical EvictableCache and SharedCache
        let mut sync_cache = EvictableCache::with_max_size(1024 * 1024);
        let mut async_inner = EvictableCache::with_max_size(1024 * 1024);
        for (i, addr) in all_addrs.iter().enumerate() {
            if i < cache_flags.len() && cache_flags[i].0 {
                let data = vec![0u8; cache_flags[i].1 as usize];
                sync_cache.put_simple(*addr, Bytes::from(data.clone()));
                async_inner.put_simple(*addr, Bytes::from(data));
            }
        }

        let shared_cache = Arc::new(SharedCache::new(async_inner));

        // Build a PersistentDag with same structure as DagStore
        let tmpdir = tempfile::tempdir().unwrap();
        let sled_db = sled::open(tmpdir.path()).unwrap();
        let persistent_dag = Arc::new(deriva_core::PersistentDag::open(&sled_db).unwrap());

        // Re-insert all recipes into the PersistentDag
        // We need to reconstruct the same DAG - rebuild from DagStore's recipes
        for addr in all_addrs.iter().skip(1) {
            if let Some(recipe) = dag.get_recipe(addr) {
                let _ = persistent_dag.insert(&recipe);
            }
        }

        let sync_result = CascadeInvalidator::invalidate_sync(
            &dag, &mut sync_cache, &root, CascadePolicy::Immediate, true, true,
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let async_result = rt.block_on(async {
            CascadeInvalidator::invalidate(
                &persistent_dag, &shared_cache, &root, CascadePolicy::Immediate, true, true,
            ).await
        });

        prop_assert_eq!(sync_result.traversed_count, async_result.traversed_count,
            "Sync and async must traverse the same count");
        prop_assert_eq!(sync_result.evicted_count, async_result.evicted_count,
            "Sync and async must evict the same count");
    }

    /// **Property 9: Policy string parsing**
    /// "none"→None, "immediate"→Immediate, "dry_run"→DryRun, "dryrun"→DryRun,
    /// anything else→Immediate.
    ///
    /// **Validates: Requirements 2.6**
    #[test]
    fn policy_string_parsing(input in "[a-z_]{0,20}") {
        let policy = parse_cascade_policy(&input);
        match input.to_lowercase().as_str() {
            "none" => prop_assert_eq!(policy, CascadePolicy::None),
            "immediate" => prop_assert_eq!(policy, CascadePolicy::Immediate),
            "dry_run" | "dryrun" => prop_assert_eq!(policy, CascadePolicy::DryRun),
            _ => prop_assert_eq!(policy, CascadePolicy::Immediate,
                "Unknown strings should default to Immediate"),
        }
    }

    /// **Property 10: Contains accuracy without LRU effects**
    /// cache.contains returns true iff addr present; doesn't change eviction state.
    ///
    /// **Validates: Requirements 2.6**
    #[test]
    fn contains_accuracy_no_lru_effects(
        present_seeds in prop::collection::vec(0..100u8, 1..=20),
        query_seeds in prop::collection::vec(0..150u8, 1..=20),
    ) {
        let mut cache = EvictableCache::with_max_size(1024 * 1024);
        let present_addrs: HashSet<CAddr> = present_seeds.iter()
            .map(|s| CAddr::from_bytes(&[*s, 2]))
            .collect();

        for addr in &present_addrs {
            cache.put_simple(*addr, Bytes::from(vec![1u8; 5]));
        }

        let size_before = cache.current_size();
        let count_before = cache.entry_count();

        let query_addrs: Vec<CAddr> = query_seeds.iter()
            .map(|s| CAddr::from_bytes(&[*s, 2]))
            .collect();

        for addr in &query_addrs {
            let result = cache.contains(addr);
            let expected = present_addrs.contains(addr);
            prop_assert_eq!(result, expected,
                "contains must return true iff addr was inserted");
        }

        // contains must not mutate cache state
        prop_assert_eq!(cache.current_size(), size_before,
            "contains must not change size");
        prop_assert_eq!(cache.entry_count(), count_before,
            "contains must not change entry count");
    }
}
