use bytes::Bytes;
use deriva_core::address::CAddr;

pub trait MaterializationCache {
    fn get(&self, addr: &CAddr) -> Option<Bytes>;
    fn put(&mut self, addr: CAddr, data: Bytes) -> u64;
    fn contains(&self, addr: &CAddr) -> bool;
}
