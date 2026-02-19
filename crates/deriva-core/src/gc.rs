use std::time::Duration;
use crate::CAddr;

/// Result of a garbage collection cycle.
#[derive(Debug, Clone)]
pub struct GcResult {
    pub blobs_removed: u64,
    pub recipes_removed: u64,
    pub cache_entries_removed: u64,
    pub bytes_reclaimed_blobs: u64,
    pub bytes_reclaimed_cache: u64,
    pub total_bytes_reclaimed: u64,
    pub live_blobs: u64,
    pub live_recipes: u64,
    pub pinned_count: u64,
    pub duration: Duration,
    pub removed_addrs: Vec<CAddr>,
    pub dry_run: bool,
}

/// Configuration for a GC cycle.
#[derive(Debug, Clone)]
pub struct GcConfig {
    /// Don't collect addrs put within this duration.
    pub grace_period: Duration,
    /// Report only, don't actually delete.
    pub dry_run: bool,
    /// Include removed addrs in result.
    pub detail_addrs: bool,
    /// Max blobs to remove (0 = unlimited).
    pub max_removals: usize,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            grace_period: Duration::from_secs(300),
            dry_run: false,
            detail_addrs: false,
            max_removals: 0,
        }
    }
}

use std::collections::HashSet;

/// A set of pinned CAddrs protected from garbage collection.
#[derive(Debug, Clone, Default)]
pub struct PinSet {
    pinned: HashSet<CAddr>,
}

impl PinSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pin(&mut self, addr: CAddr) -> bool {
        self.pinned.insert(addr)
    }

    pub fn unpin(&mut self, addr: &CAddr) -> bool {
        self.pinned.remove(addr)
    }

    pub fn is_pinned(&self, addr: &CAddr) -> bool {
        self.pinned.contains(addr)
    }

    pub fn count(&self) -> usize {
        self.pinned.len()
    }

    pub fn list(&self) -> Vec<CAddr> {
        self.pinned.iter().copied().collect()
    }

    pub fn as_set(&self) -> &HashSet<CAddr> {
        &self.pinned
    }
}
