use bytes::Bytes;
use deriva_core::address::CAddr;
use deriva_core::cache::EvictableCache;
use async_trait::async_trait;
use tokio::sync::RwLock;

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

/// Async cache trait for the async executor
#[async_trait]
pub trait AsyncMaterializationCache: Send + Sync {
    async fn get(&self, addr: &CAddr) -> Option<Bytes>;
    async fn put(&self, addr: CAddr, data: Bytes) -> u64;
    async fn contains(&self, addr: &CAddr) -> bool;
}

/// Wraps EvictableCache with tokio RwLock for async access
pub struct SharedCache {
    inner: RwLock<EvictableCache>,
}

impl SharedCache {
    pub fn new(cache: EvictableCache) -> Self {
        Self { inner: RwLock::new(cache) }
    }

    pub async fn entry_count(&self) -> usize {
        self.inner.read().await.entry_count()
    }

    pub async fn current_size(&self) -> u64 {
        self.inner.read().await.current_size()
    }

    pub async fn hit_rate(&self) -> f64 {
        self.inner.read().await.hit_rate()
    }

    pub async fn remove(&self, addr: &CAddr) -> Option<Bytes> {
        self.inner.write().await.remove(addr)
    }
}

#[async_trait]
impl AsyncMaterializationCache for SharedCache {
    async fn get(&self, addr: &CAddr) -> Option<Bytes> {
        self.inner.write().await.get(addr)
    }

    async fn put(&self, addr: CAddr, data: Bytes) -> u64 {
        self.inner.write().await.put_simple(addr, data)
    }

    async fn contains(&self, addr: &CAddr) -> bool {
        self.inner.read().await.contains(addr)
    }
}
