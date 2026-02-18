use bytes::Bytes;
use crate::address::CAddr;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComputeCost {
    pub cpu_ms: u64,
    pub memory_bytes: u64,
}

impl Default for ComputeCost {
    fn default() -> Self {
        Self { cpu_ms: 1, memory_bytes: 0 }
    }
}

#[derive(Debug)]
struct CacheEntry {
    data: Bytes,
    size: u64,
    access_count: u64,
    last_accessed: Instant,
    recompute_cost: ComputeCost,
    pinned: bool,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub max_size: u64,
    pub pressure_threshold: f64,
    pub target_ratio: f64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size: 1_073_741_824,
            pressure_threshold: 0.8,
            target_ratio: 0.6,
        }
    }
}

#[derive(Debug)]
pub struct EvictableCache {
    entries: HashMap<CAddr, CacheEntry>,
    current_size: u64,
    config: CacheConfig,
    hits: u64,
    misses: u64,
    eviction_count: u64,
}

impl EvictableCache {
    pub fn new(config: CacheConfig) -> Self {
        Self {
            entries: HashMap::new(),
            current_size: 0,
            config,
            hits: 0,
            misses: 0,
            eviction_count: 0,
        }
    }

    pub fn with_max_size(max_size: u64) -> Self {
        Self::new(CacheConfig { max_size, ..Default::default() })
    }

    pub fn get(&mut self, addr: &CAddr) -> Option<Bytes> {
        if let Some(entry) = self.entries.get_mut(addr) {
            entry.access_count += 1;
            entry.last_accessed = Instant::now();
            self.hits += 1;
            Some(entry.data.clone())
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn put(&mut self, addr: CAddr, data: Bytes, recompute_cost: ComputeCost) -> u64 {
        let size = data.len() as u64;

        if let Some(entry) = self.entries.get_mut(&addr) {
            self.current_size -= entry.size;
            entry.data = data;
            entry.size = size;
            entry.recompute_cost = recompute_cost;
            self.current_size += size;
            return size;
        }

        self.entries.insert(addr, CacheEntry {
            data,
            size,
            access_count: 0,
            last_accessed: Instant::now(),
            recompute_cost,
            pinned: false,
        });
        self.current_size += size;

        self.maybe_evict();
        size
    }

    pub fn put_simple(&mut self, addr: CAddr, data: Bytes) -> u64 {
        self.put(addr, data, ComputeCost::default())
    }

    pub fn contains(&self, addr: &CAddr) -> bool {
        self.entries.contains_key(addr)
    }

    pub fn remove(&mut self, addr: &CAddr) -> Option<Bytes> {
        let entry = self.entries.remove(addr)?;
        self.current_size -= entry.size;
        Some(entry.data)
    }

    pub fn remove_batch(&mut self, addrs: &[CAddr]) -> (u64, u64, Vec<CAddr>) {
        let mut count = 0u64;
        let mut bytes = 0u64;
        let mut removed = Vec::new();
        for addr in addrs {
            if let Some(entry) = self.entries.remove(addr) {
                self.current_size -= entry.size;
                bytes += entry.size;
                count += 1;
                removed.push(*addr);
            }
        }
        (count, bytes, removed)
    }

    pub fn pin(&mut self, addr: &CAddr) -> bool {
        if let Some(entry) = self.entries.get_mut(addr) {
            entry.pinned = true;
            true
        } else {
            false
        }
    }

    pub fn unpin(&mut self, addr: &CAddr) -> bool {
        if let Some(entry) = self.entries.get_mut(addr) {
            entry.pinned = false;
            true
        } else {
            false
        }
    }

    pub fn current_size(&self) -> u64 {
        self.current_size
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

    pub fn eviction_count(&self) -> u64 {
        self.eviction_count
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 { 0.0 } else { self.hits as f64 / total as f64 }
    }

    pub fn score(&self, addr: &CAddr) -> Option<f64> {
        self.entries.get(addr).map(Self::eviction_score)
    }

    fn eviction_score(entry: &CacheEntry) -> f64 {
        let cost = entry.recompute_cost.cpu_ms.max(1) as f64;
        let frequency = (entry.access_count + 1) as f64;
        let size = entry.size.max(1) as f64;
        (cost * frequency) / size
    }

    fn maybe_evict(&mut self) {
        let threshold = (self.config.max_size as f64 * self.config.pressure_threshold) as u64;
        if self.current_size <= threshold {
            return;
        }

        let target = (self.config.max_size as f64 * self.config.target_ratio) as u64;

        let mut scored: Vec<(CAddr, f64)> = self.entries.iter()
            .filter(|(_, e)| !e.pinned)
            .map(|(addr, entry)| (*addr, Self::eviction_score(entry)))
            .collect();

        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        for (addr, _) in scored {
            if self.current_size <= target {
                break;
            }
            if let Some(entry) = self.entries.remove(&addr) {
                self.current_size -= entry.size;
                self.eviction_count += 1;
            }
        }
    }
}
