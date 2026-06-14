use std::sync::Arc;

use bytes::Bytes;
use deriva_core::address::CAddr;
use deriva_core::cache::EvictableCache;
use deriva_core::streaming::StreamChunk;
use async_trait::async_trait;
use tokio::sync::mpsc::Receiver;
use tokio::sync::RwLock;

use crate::chunk_cache::{manifest_key, stream_from_cache, range_read, ChunkBlobStore, ChunkManifest, ChunkAddr, ChunkManifestBuilder, cleanup_partial_cache_miss};
use crate::metrics::{CACHE_EVICTION_TOTAL, CACHE_SIZE, CACHE_ENTRIES, CACHE_HIT_RATE};

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

    /// Stream cached chunks for the given address via a bounded channel.
    /// Returns `None` if no manifest exists for this address.
    async fn get_stream(&self, _addr: &CAddr, _capacity: usize) -> Option<Receiver<StreamChunk>> {
        None
    }

    /// Check whether a chunk manifest exists for the given address.
    async fn has_manifest(&self, _addr: &CAddr) -> bool {
        false
    }

    /// Read a byte range from cached chunks without loading the entire value.
    /// Returns `None` if no manifest exists for this address.
    async fn range_read(&self, _addr: &CAddr, _offset: u64, _length: u64) -> Option<Bytes> {
        None
    }
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

    pub async fn remove_batch(&self, addrs: &[CAddr]) -> (u64, u64, Vec<CAddr>) {
        self.inner.write().await.remove_batch(addrs)
    }
}

#[async_trait]
impl AsyncMaterializationCache for SharedCache {
    async fn get(&self, addr: &CAddr) -> Option<Bytes> {
        let mut inner = self.inner.write().await;
        let result = inner.get(addr);
        let hit_rate = inner.hit_rate();
        CACHE_HIT_RATE.set(hit_rate);
        result
    }

    async fn put(&self, addr: CAddr, data: Bytes) -> u64 {
        let mut inner = self.inner.write().await;
        let before_count = inner.entry_count();
        let size = inner.put_simple(addr, data);
        let after_count = inner.entry_count();

        // Detect evictions: we added 1 entry, so if count didn't increase
        // accordingly, entries were evicted.
        if before_count >= after_count {
            let evicted = (before_count + 1).saturating_sub(after_count);
            CACHE_EVICTION_TOTAL.inc_by(evicted as u64);
        }

        // Update gauges to reflect current cache state
        CACHE_SIZE.set(inner.current_size() as f64);
        CACHE_ENTRIES.set(inner.entry_count() as f64);

        size
    }

    async fn contains(&self, addr: &CAddr) -> bool {
        self.inner.read().await.contains(addr)
    }
}

/// A chunk-aware cache that composes an inner `AsyncMaterializationCache` with
/// a `ChunkBlobStore`, enabling streaming reads, manifest checks, and range reads
/// from chunked cached data.
pub struct ChunkAwareCache {
    inner: Arc<dyn AsyncMaterializationCache>,
    blob_store: Arc<dyn ChunkBlobStore>,
}

impl ChunkAwareCache {
    pub fn new(
        inner: Arc<dyn AsyncMaterializationCache>,
        blob_store: Arc<dyn ChunkBlobStore>,
    ) -> Self {
        Self { inner, blob_store }
    }

    /// Load and deserialize the chunk manifest for the given address, if it exists.
    async fn load_manifest(&self, addr: &CAddr) -> Option<ChunkManifest> {
        let key = manifest_key(addr);
        match self.blob_store.get_chunk(&key).await {
            Ok(Some(data)) => match ChunkManifest::from_bytes(&data) {
                Ok(manifest) => Some(manifest),
                Err(e) => {
                    tracing::warn!(
                        caddr = %addr,
                        error = %e,
                        "failed to deserialize chunk manifest"
                    );
                    None
                }
            },
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(
                    caddr = %addr,
                    error = %e,
                    "error reading chunk manifest from blob store"
                );
                None
            }
        }
    }
}

#[async_trait]
impl AsyncMaterializationCache for ChunkAwareCache {
    async fn get(&self, addr: &CAddr) -> Option<Bytes> {
        // Fast path: check in-memory cache first
        if let Some(data) = self.inner.get(addr).await {
            return Some(data);
        }

        // Slow path: try to assemble from chunk blob store via manifest
        let manifest = self.load_manifest(addr).await?;

        // Read all chunks in offset order and concatenate
        let mut assembled = Vec::with_capacity(manifest.total_size as usize);
        for entry in &manifest.chunks {
            match self.blob_store.get_chunk(&entry.blob_key).await {
                Ok(Some(chunk_data)) => {
                    assembled.extend_from_slice(&chunk_data);
                }
                Ok(None) => {
                    // Partial cache miss: a chunk is missing
                    tracing::warn!(
                        caddr = %addr,
                        blob_key = %entry.blob_key,
                        "partial cache miss during full-value get, cleaning up"
                    );
                    cleanup_partial_cache_miss(&self.blob_store, &manifest).await;
                    return None;
                }
                Err(e) => {
                    tracing::warn!(
                        caddr = %addr,
                        blob_key = %entry.blob_key,
                        error = %e,
                        "blob store error during full-value get"
                    );
                    return None;
                }
            }
        }

        Some(Bytes::from(assembled))
    }

    async fn put(&self, addr: CAddr, data: Bytes) -> u64 {
        let data_len = data.len();

        // Store in inner (in-memory) cache
        let size = self.inner.put(addr, data.clone()).await;

        // Also write to blob store as a single chunk with a single-entry manifest
        let chunk_addr = ChunkAddr { caddr: addr, offset: 0 };
        let blob_key = chunk_addr.blob_key();

        if let Err(e) = self.blob_store.put_chunk(&blob_key, &data).await {
            tracing::warn!(
                caddr = %addr,
                error = %e,
                "failed to write chunk blob during monolithic put"
            );
            return size;
        }

        // Build a single-entry manifest
        let mut builder = ChunkManifestBuilder::new(addr, data_len as u32);
        builder.add_chunk(data_len as u32, blob_key);
        let manifest = builder.build();

        match manifest.to_bytes() {
            Ok(manifest_bytes) => {
                let mkey = manifest_key(&addr);
                if let Err(e) = self.blob_store.put_chunk(&mkey, &manifest_bytes).await {
                    tracing::warn!(
                        caddr = %addr,
                        error = %e,
                        "failed to write manifest during monolithic put"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    caddr = %addr,
                    error = %e,
                    "failed to serialize manifest during monolithic put"
                );
            }
        }

        size
    }

    async fn contains(&self, addr: &CAddr) -> bool {
        // Check in-memory cache first
        if self.inner.contains(addr).await {
            return true;
        }
        // Fall back to checking if manifest exists in blob store
        self.has_manifest(addr).await
    }

    async fn get_stream(&self, addr: &CAddr, capacity: usize) -> Option<Receiver<StreamChunk>> {
        let manifest = self.load_manifest(addr).await?;
        Some(stream_from_cache(&manifest, Arc::clone(&self.blob_store), capacity))
    }

    async fn has_manifest(&self, addr: &CAddr) -> bool {
        let key = manifest_key(addr);
        match self.blob_store.get_chunk(&key).await {
            Ok(Some(_)) => true,
            _ => false,
        }
    }

    async fn range_read(&self, addr: &CAddr, offset: u64, length: u64) -> Option<Bytes> {
        let manifest = self.load_manifest(addr).await?;
        match range_read(&manifest, &self.blob_store, offset, length).await {
            Ok(data) => Some(data),
            Err(e) => {
                tracing::warn!(
                    caddr = %addr,
                    offset = offset,
                    length = length,
                    error = %e,
                    "range_read failed for cached chunk data"
                );
                None
            }
        }
    }
}
