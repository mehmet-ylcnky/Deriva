use bytes::Bytes;
use deriva_core::address::CAddr;

pub trait LeafStore {
    fn get_leaf(&self, addr: &CAddr) -> Option<Bytes>;
}
