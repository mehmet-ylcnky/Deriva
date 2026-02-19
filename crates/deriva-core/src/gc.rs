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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_addr(seed: u8) -> CAddr {
        CAddr::from_raw([seed; 32])
    }

    #[test]
    fn pin_new_returns_true() {
        let mut pins = PinSet::new();
        assert!(pins.pin(test_addr(1)));
        assert_eq!(pins.count(), 1);
    }

    #[test]
    fn pin_duplicate_returns_false() {
        let mut pins = PinSet::new();
        let a = test_addr(1);
        pins.pin(a);
        assert!(!pins.pin(a));
        assert_eq!(pins.count(), 1);
    }

    #[test]
    fn unpin_existing_returns_true() {
        let mut pins = PinSet::new();
        let a = test_addr(1);
        pins.pin(a);
        assert!(pins.unpin(&a));
        assert_eq!(pins.count(), 0);
    }

    #[test]
    fn unpin_missing_returns_false() {
        let mut pins = PinSet::new();
        assert!(!pins.unpin(&test_addr(1)));
    }

    #[test]
    fn is_pinned_reflects_state() {
        let mut pins = PinSet::new();
        let a = test_addr(1);
        assert!(!pins.is_pinned(&a));
        pins.pin(a);
        assert!(pins.is_pinned(&a));
        pins.unpin(&a);
        assert!(!pins.is_pinned(&a));
    }

    #[test]
    fn list_returns_all_pinned() {
        let mut pins = PinSet::new();
        let a = test_addr(1);
        let b = test_addr(2);
        pins.pin(a);
        pins.pin(b);
        let list = pins.list();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&a));
        assert!(list.contains(&b));
    }

    #[test]
    fn as_set_matches_pins() {
        let mut pins = PinSet::new();
        let a = test_addr(1);
        pins.pin(a);
        assert!(pins.as_set().contains(&a));
        assert_eq!(pins.as_set().len(), 1);
    }

    #[test]
    fn empty_pinset_defaults() {
        let pins = PinSet::new();
        assert_eq!(pins.count(), 0);
        assert!(pins.list().is_empty());
        assert!(pins.as_set().is_empty());
    }
}
