use async_trait::async_trait;
use bytes::Bytes;
use deriva_core::address::CAddr;

pub trait LeafStore {
    fn get_leaf(&self, addr: &CAddr) -> Option<Bytes>;
}

/// Async leaf store for the async executor
#[async_trait]
pub trait AsyncLeafStore: Send + Sync {
    async fn get_leaf(&self, addr: &CAddr) -> Option<Bytes>;
}

/// Blanket impl: any sync LeafStore that is Send+Sync can be used as AsyncLeafStore
#[async_trait]
impl<T: LeafStore + Send + Sync> AsyncLeafStore for T {
    async fn get_leaf(&self, addr: &CAddr) -> Option<Bytes> {
        LeafStore::get_leaf(self, addr)
    }
}
