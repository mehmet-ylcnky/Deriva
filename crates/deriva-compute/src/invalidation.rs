use deriva_core::address::CAddr;
use deriva_core::cache::EvictableCache;
use deriva_core::dag::DagStore;
use deriva_core::invalidation::{CascadePolicy, InvalidationResult};
use deriva_core::PersistentDag;
use crate::cache::{SharedCache, AsyncMaterializationCache};
use std::sync::Arc;
use std::time::Instant;

pub struct CascadeInvalidator;

impl CascadeInvalidator {
    /// Sync invalidation using DagStore + EvictableCache directly.
    pub fn invalidate_sync(
        dag: &DagStore,
        cache: &mut EvictableCache,
        root: &CAddr,
        policy: CascadePolicy,
        include_root: bool,
        detail_addrs: bool,
    ) -> InvalidationResult {
        let start = Instant::now();

        match policy {
            CascadePolicy::None => {
                let (count, bytes, addrs) = if include_root {
                    cache.remove_batch(&[*root])
                } else {
                    (0, 0, Vec::new())
                };
                InvalidationResult {
                    root: *root,
                    evicted_count: count,
                    traversed_count: 0,
                    max_depth: 0,
                    bytes_reclaimed: bytes,
                    evicted_addrs: if detail_addrs { addrs } else { Vec::new() },
                    duration: start.elapsed(),
                }
            }

            CascadePolicy::Immediate => {
                let (dependents, max_depth) = dag.transitive_dependents_with_depth(root);
                let traversed_count = dependents.len() as u64;

                let mut to_evict: Vec<CAddr> =
                    dependents.into_iter().map(|(addr, _)| addr).collect();
                if include_root {
                    to_evict.insert(0, *root);
                }

                let (evicted_count, bytes_reclaimed, evicted_addrs) =
                    cache.remove_batch(&to_evict);

                InvalidationResult {
                    root: *root,
                    evicted_count,
                    traversed_count,
                    max_depth,
                    bytes_reclaimed,
                    evicted_addrs: if detail_addrs { evicted_addrs } else { Vec::new() },
                    duration: start.elapsed(),
                }
            }

            CascadePolicy::DryRun => {
                let (dependents, max_depth) = dag.transitive_dependents_with_depth(root);
                let traversed_count = dependents.len() as u64;

                let mut would_evict = Vec::new();
                if include_root && cache.contains(root) {
                    would_evict.push(*root);
                }
                for (addr, _) in &dependents {
                    if cache.contains(addr) {
                        would_evict.push(*addr);
                    }
                }

                InvalidationResult {
                    root: *root,
                    evicted_count: would_evict.len() as u64,
                    traversed_count,
                    max_depth,
                    bytes_reclaimed: 0,
                    evicted_addrs: if detail_addrs { would_evict } else { Vec::new() },
                    duration: start.elapsed(),
                }
            }
        }
    }

    /// Async invalidation using PersistentDag + SharedCache.
    pub async fn invalidate(
        dag: &Arc<PersistentDag>,
        cache: &Arc<SharedCache>,
        root: &CAddr,
        policy: CascadePolicy,
        include_root: bool,
        detail_addrs: bool,
    ) -> InvalidationResult {
        let start = Instant::now();

        match policy {
            CascadePolicy::None => {
                let was_present = cache.remove(root).await.is_some();
                InvalidationResult {
                    root: *root,
                    evicted_count: if include_root && was_present { 1 } else { 0 },
                    traversed_count: 0,
                    max_depth: 0,
                    bytes_reclaimed: 0,
                    evicted_addrs: Vec::new(),
                    duration: start.elapsed(),
                }
            }

            CascadePolicy::Immediate => {
                let (dependents, max_depth) = dag.transitive_dependents_with_depth(root);
                let traversed_count = dependents.len() as u64;

                let mut to_evict: Vec<CAddr> =
                    dependents.into_iter().map(|(addr, _)| addr).collect();
                if include_root {
                    to_evict.insert(0, *root);
                }

                let (evicted_count, bytes_reclaimed, evicted_addrs) =
                    cache.remove_batch(&to_evict).await;

                InvalidationResult {
                    root: *root,
                    evicted_count,
                    traversed_count,
                    max_depth,
                    bytes_reclaimed,
                    evicted_addrs: if detail_addrs { evicted_addrs } else { Vec::new() },
                    duration: start.elapsed(),
                }
            }

            CascadePolicy::DryRun => {
                let (dependents, max_depth) = dag.transitive_dependents_with_depth(root);
                let traversed_count = dependents.len() as u64;

                let mut would_evict = Vec::new();
                if include_root && cache.contains(root).await {
                    would_evict.push(*root);
                }
                for (addr, _) in &dependents {
                    if cache.contains(addr).await {
                        would_evict.push(*addr);
                    }
                }

                InvalidationResult {
                    root: *root,
                    evicted_count: would_evict.len() as u64,
                    traversed_count,
                    max_depth,
                    bytes_reclaimed: 0,
                    evicted_addrs: if detail_addrs { would_evict } else { Vec::new() },
                    duration: start.elapsed(),
                }
            }
        }
    }
}
