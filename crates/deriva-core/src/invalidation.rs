use crate::address::CAddr;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CascadePolicy {
    None,
    #[default]
    Immediate,
    DryRun,
}

#[derive(Debug, Clone)]
pub struct InvalidationResult {
    pub root: CAddr,
    pub evicted_count: u64,
    pub traversed_count: u64,
    pub max_depth: u32,
    pub bytes_reclaimed: u64,
    pub evicted_addrs: Vec<CAddr>,
    pub duration: Duration,
}

impl InvalidationResult {
    pub fn empty(root: CAddr) -> Self {
        Self {
            root,
            evicted_count: 0,
            traversed_count: 0,
            max_depth: 0,
            bytes_reclaimed: 0,
            evicted_addrs: Vec::new(),
            duration: Duration::ZERO,
        }
    }
}
