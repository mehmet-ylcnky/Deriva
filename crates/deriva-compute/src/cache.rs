use bytes::Bytes;
use deriva_core::address::CAddr;
use deriva_core::cache::EvictableCache;

pub trait MaterializationCache {
    fn get(&mut self, addr: &CAddr) -> Option<Bytes>;
    fn put(&mut self, addr: CAddr, data: Bytes) -> u64;
    fn contains(&self, addr: &CAddr) -> bool;
}

impl MaterializationCache for EvictableCache {
    fn get(&mut self, addr: &CAddr) -> Option<Bytes> {
        self.get(addr)
    }

    fn put(&mut self, addr: CAddr, data: Bytes) -> u64 {
        self.put_simple(addr, data)
    }

    fn contains(&self, addr: &CAddr) -> bool {
        self.contains(addr)
    }
}
