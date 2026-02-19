use std::collections::HashSet;
use std::time::Instant;
use deriva_core::CAddr;
use deriva_core::error::DerivaError;
use deriva_core::gc::{GcConfig, GcResult, PinSet};
use deriva_core::persistent_dag::PersistentDag;
use deriva_compute::cache::SharedCache;
use deriva_storage::blob_store::BlobStore;

/// Run a garbage collection cycle.
pub async fn run_gc(
    dag: &PersistentDag,
    blobs: &BlobStore,
    cache: &SharedCache,
    pins: &PinSet,
    config: &GcConfig,
) -> Result<GcResult, DerivaError> {
    let start = Instant::now();

    // 1. Compute live set (recipe outputs + inputs + pins)
    let mut live_set = dag.live_addr_set();
    for addr in pins.as_set() {
        live_set.insert(*addr);
    }

    // 2. Enumerate all blobs
    let all_blobs = blobs.list_addrs()?;

    // 3. Identify orphaned blobs
    let mut orphaned_blobs: Vec<CAddr> = all_blobs.iter()
        .filter(|a| !live_set.contains(a))
        .copied()
        .collect();

    // 4. Apply max_removals limit
    if config.max_removals > 0 && orphaned_blobs.len() > config.max_removals {
        orphaned_blobs.truncate(config.max_removals);
    }

    // 5. Sweep blobs
    let (blobs_removed, bytes_reclaimed_blobs) = if config.dry_run {
        let mut bytes = 0u64;
        for addr in &orphaned_blobs {
            if let Ok(Some(data)) = blobs.get(addr) {
                bytes += data.len() as u64;
            }
        }
        (orphaned_blobs.len() as u64, bytes)
    } else {
        blobs.remove_batch_blobs(&orphaned_blobs)?
    };

    // 6. Find orphaned recipes (any input was just removed)
    let removed_set: HashSet<CAddr> = orphaned_blobs.iter().copied().collect();
    let mut orphaned_recipes = Vec::new();
    for addr in dag.all_addrs() {
        if pins.is_pinned(&addr) { continue; }
        if let Ok(Some(inputs)) = dag.inputs(&addr) {
            if inputs.iter().any(|i| removed_set.contains(i)) {
                orphaned_recipes.push(addr);
            }
        }
    }
    let recipes_removed = orphaned_recipes.len() as u64;

    // 7. Remove stale cache entries
    let mut all_removed: Vec<CAddr> = orphaned_blobs.clone();
    all_removed.extend_from_slice(&orphaned_recipes);

    let (cache_entries_removed, bytes_reclaimed_cache, _) = if config.dry_run {
        (0u64, 0u64, vec![])
    } else {
        cache.remove_batch(&all_removed).await
    };

    // 8. Remove orphaned recipes from DAG
    if !config.dry_run {
        for addr in &orphaned_recipes {
            let _ = dag.remove(addr);
        }
    }

    Ok(GcResult {
        blobs_removed,
        recipes_removed,
        cache_entries_removed,
        bytes_reclaimed_blobs,
        bytes_reclaimed_cache,
        total_bytes_reclaimed: bytes_reclaimed_blobs + bytes_reclaimed_cache,
        live_blobs: (all_blobs.len() as u64).saturating_sub(blobs_removed),
        live_recipes: dag.len() as u64,
        pinned_count: pins.count() as u64,
        duration: start.elapsed(),
        removed_addrs: if config.detail_addrs { all_removed } else { vec![] },
        dry_run: config.dry_run,
    })
}
