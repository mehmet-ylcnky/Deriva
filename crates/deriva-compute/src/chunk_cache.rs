use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use deriva_core::address::CAddr;
use deriva_core::streaming::StreamChunk;
use deriva_core::DerivaError;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;

// --- Error types ---

#[derive(Debug, Error)]
pub enum ChunkCacheError {
    #[error("serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("validation error: {0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, ChunkCacheError>;

// --- ChunkAddr ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkAddr {
    pub caddr: CAddr,
    pub offset: u64,
}

impl ChunkAddr {
    pub fn blob_key(&self) -> String {
        format!("{}_chunk_{}", self.caddr.to_hex(), self.offset)
    }
}

// --- manifest_key ---

pub fn manifest_key(caddr: &CAddr) -> String {
    format!("{}_manifest", caddr.to_hex())
}

// --- ChunkEntry ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkEntry {
    pub offset: u64,
    pub length: u32,
    pub blob_key: String,
}

// --- ChunkManifest ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkManifest {
    pub caddr: CAddr,
    pub total_size: u64,
    pub chunk_size: u32,
    pub chunks: Vec<ChunkEntry>,
}

impl ChunkManifest {
    /// Serialize the manifest to bytes via bincode.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(ChunkCacheError::from)
    }

    /// Deserialize a manifest from bytes via bincode.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(ChunkCacheError::from)
    }

    /// Validate manifest invariants:
    /// - chunks[0].offset == 0
    /// - ascending contiguous offsets (chunks[i+1].offset == chunks[i].offset + chunks[i].length)
    /// - sum of lengths == total_size
    pub fn validate(&self) -> Result<()> {
        if self.chunks.is_empty() {
            if self.total_size == 0 {
                return Ok(());
            }
            return Err(ChunkCacheError::Validation(
                "manifest has no chunks but total_size > 0".to_string(),
            ));
        }

        if self.chunks[0].offset != 0 {
            return Err(ChunkCacheError::Validation(format!(
                "first chunk offset is {} but expected 0",
                self.chunks[0].offset
            )));
        }

        let mut expected_offset: u64 = 0;
        for (i, chunk) in self.chunks.iter().enumerate() {
            if chunk.offset != expected_offset {
                return Err(ChunkCacheError::Validation(format!(
                    "chunk {} has offset {} but expected {}",
                    i, chunk.offset, expected_offset
                )));
            }
            expected_offset += chunk.length as u64;
        }

        if expected_offset != self.total_size {
            return Err(ChunkCacheError::Validation(format!(
                "sum of chunk lengths ({}) does not equal total_size ({})",
                expected_offset, self.total_size
            )));
        }

        Ok(())
    }

    /// Return the slice of chunks that overlap with the byte range [offset, offset+length).
    pub fn chunks_for_range(&self, offset: u64, length: u64) -> &[ChunkEntry] {
        if self.chunks.is_empty() || length == 0 {
            return &[];
        }

        let range_end = offset + length;

        // Find first chunk that overlaps: chunk.offset + chunk.length > offset
        let start_idx = self
            .chunks
            .partition_point(|c| (c.offset + c.length as u64) <= offset);

        // Find first chunk that doesn't overlap: chunk.offset >= range_end
        let end_idx = self.chunks.partition_point(|c| c.offset < range_end);

        &self.chunks[start_idx..end_idx]
    }
}

// --- ChunkManifestBuilder ---

pub struct ChunkManifestBuilder {
    caddr: CAddr,
    chunk_size: u32,
    /// The current byte offset — also used by ChunkCacheWriter to determine
    /// the offset for the next chunk.
    pub(crate) current_offset: u64,
    chunks: Vec<ChunkEntry>,
}

impl ChunkManifestBuilder {
    pub fn new(caddr: CAddr, chunk_size: u32) -> Self {
        Self {
            caddr,
            chunk_size,
            current_offset: 0,
            chunks: Vec::new(),
        }
    }

    /// Append a chunk entry at the current offset and advance the offset.
    pub fn add_chunk(&mut self, length: u32, blob_key: String) {
        self.chunks.push(ChunkEntry {
            offset: self.current_offset,
            length,
            blob_key,
        });
        self.current_offset += length as u64;
    }

    /// Build the final ChunkManifest with total_size = current_offset.
    pub fn build(self) -> ChunkManifest {
        ChunkManifest {
            caddr: self.caddr,
            total_size: self.current_offset,
            chunk_size: self.chunk_size,
            chunks: self.chunks,
        }
    }

    /// Return all blob_keys added so far (for cleanup on error).
    pub fn blob_keys(&self) -> Vec<String> {
        self.chunks.iter().map(|c| c.blob_key.clone()).collect()
    }
}

// --- ChunkBlobStore trait ---

/// Trait abstracting string-keyed blob storage operations needed by chunk caching.
/// This allows `ChunkCacheWriter` (in `deriva-compute`) to operate against any
/// backend that implements this trait, avoiding a direct dependency on `BlobStore`
/// (which lives in `deriva-storage`).
#[async_trait]
pub trait ChunkBlobStore: Send + Sync {
    /// Store a blob at the given string key.
    async fn put_chunk(&self, key: &str, data: &[u8]) -> std::result::Result<(), DerivaError>;

    /// Read a blob by its string key. Returns None if not found.
    async fn get_chunk(&self, key: &str) -> std::result::Result<Option<Bytes>, DerivaError>;

    /// Remove a blob by key. Returns true if removed, false if not found.
    async fn remove_chunk(&self, key: &str) -> std::result::Result<bool, DerivaError>;
}

// --- ChunkCacheWriter ---

/// A streaming pass-through that writes chunks to blob storage as they flow
/// through the pipeline, then commits a manifest at stream end.
///
/// Peak memory usage: one chunk + manifest builder overhead (~20 bytes per entry).
/// Implements the "manifest-last" commit strategy: chunks written first, manifest
/// written last. If the stream is interrupted, no manifest is written and all
/// previously written chunks are cleaned up.
pub struct ChunkCacheWriter {
    caddr: CAddr,
    chunk_size: u32,
    blob_store: Arc<dyn ChunkBlobStore>,
}

impl ChunkCacheWriter {
    /// Create a new ChunkCacheWriter.
    ///
    /// # Arguments
    /// - `caddr`: The computation address this value is being cached for
    /// - `chunk_size`: The nominal chunk size (used in manifest metadata)
    /// - `blob_store`: A reference to the blob storage backend
    pub fn new(caddr: CAddr, chunk_size: u32, blob_store: Arc<dyn ChunkBlobStore>) -> Self {
        Self {
            caddr,
            chunk_size,
            blob_store,
        }
    }

    /// Wrap an input stream receiver, producing a new output receiver.
    ///
    /// Each `StreamChunk::Data` is:
    /// 1. Written to BlobStore under a deterministic chunk key
    /// 2. Recorded in the manifest builder
    /// 3. Forwarded to the output receiver
    ///
    /// On `StreamChunk::End`:
    /// 1. The manifest is serialized and stored at `manifest_key(caddr)`
    /// 2. `End` is forwarded to the output
    ///
    /// On error or consumer drop, all written chunks are cleaned up and no
    /// manifest is stored.
    pub fn wrap(
        self,
        mut input: mpsc::Receiver<StreamChunk>,
        capacity: usize,
    ) -> mpsc::Receiver<StreamChunk> {
        let (tx, rx) = mpsc::channel(capacity);

        tokio::spawn(async move {
            let mut builder = ChunkManifestBuilder::new(self.caddr, self.chunk_size);

            loop {
                match input.recv().await {
                    Some(StreamChunk::Data(data)) => {
                        let chunk_addr = ChunkAddr {
                            caddr: self.caddr,
                            offset: builder.current_offset,
                        };
                        let key = chunk_addr.blob_key();
                        let len = data.len() as u32;

                        // Write chunk to blob store
                        if let Err(e) = self.blob_store.put_chunk(&key, &data).await {
                            tracing::warn!(
                                caddr = %self.caddr,
                                chunk_key = %key,
                                error = %e,
                                "chunk cache write failed, cleaning up"
                            );
                            Self::cleanup(&self.blob_store, &builder).await;
                            let _ = tx.send(StreamChunk::Error(e)).await;
                            return;
                        }

                        builder.add_chunk(len, key);

                        // Forward to consumer
                        if tx.send(StreamChunk::Data(data)).await.is_err() {
                            // Consumer dropped — cleanup
                            tracing::warn!(
                                caddr = %self.caddr,
                                "consumer dropped during chunk cache write, cleaning up"
                            );
                            Self::cleanup(&self.blob_store, &builder).await;
                            return;
                        }
                    }
                    Some(StreamChunk::End) => {
                        // Build manifest and store it
                        let manifest = builder.build();
                        match manifest.to_bytes() {
                            Ok(manifest_bytes) => {
                                let mkey = manifest_key(&self.caddr);
                                if let Err(e) =
                                    self.blob_store.put_chunk(&mkey, &manifest_bytes).await
                                {
                                    tracing::warn!(
                                        caddr = %self.caddr,
                                        error = %e,
                                        "failed to write chunk manifest"
                                    );
                                    // Chunks are written but manifest failed.
                                    // They become orphans for GC to clean.
                                    let _ = tx.send(StreamChunk::Error(e)).await;
                                    return;
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    caddr = %self.caddr,
                                    error = %e,
                                    "failed to serialize chunk manifest"
                                );
                                let _ = tx
                                    .send(StreamChunk::Error(DerivaError::Serialization(
                                        e.to_string(),
                                    )))
                                    .await;
                                return;
                            }
                        }
                        let _ = tx.send(StreamChunk::End).await;
                        return;
                    }
                    Some(StreamChunk::Error(e)) => {
                        // Cleanup and forward error
                        tracing::warn!(
                            caddr = %self.caddr,
                            error = %e,
                            "upstream error during chunk cache write, cleaning up"
                        );
                        Self::cleanup(&self.blob_store, &builder).await;
                        let _ = tx.send(StreamChunk::Error(e)).await;
                        return;
                    }
                    None => {
                        // Input channel closed without End — cleanup
                        tracing::warn!(
                            caddr = %self.caddr,
                            "input channel closed without End during chunk cache write, cleaning up"
                        );
                        Self::cleanup(&self.blob_store, &builder).await;
                        return;
                    }
                }
            }
        });

        rx
    }

    /// Remove all previously written chunk blobs. Best-effort: individual
    /// removal failures are logged but do not propagate.
    async fn cleanup(blob_store: &Arc<dyn ChunkBlobStore>, builder: &ChunkManifestBuilder) {
        for key in builder.blob_keys() {
            if let Err(e) = blob_store.remove_chunk(&key).await {
                tracing::warn!(
                    chunk_key = %key,
                    error = %e,
                    "failed to cleanup chunk blob during error recovery"
                );
            }
        }
    }
}

// --- Partial cache miss cleanup ---

/// Remove all chunk blobs and the manifest blob for a partially-corrupted cache entry.
///
/// This is used when a streaming read detects a missing chunk, indicating that
/// the cached value is incomplete. The function removes all chunk blobs referenced
/// by the manifest and the manifest blob itself to allow clean re-caching.
///
/// Cleanup is best-effort: individual removal failures are logged at warn level
/// but do not propagate or halt cleanup of remaining blobs.
pub async fn cleanup_partial_cache_miss(
    blob_store: &Arc<dyn ChunkBlobStore>,
    manifest: &ChunkManifest,
) {
    // Remove all chunk blobs referenced by the manifest
    for entry in &manifest.chunks {
        if let Err(e) = blob_store.remove_chunk(&entry.blob_key).await {
            tracing::warn!(
                caddr = %manifest.caddr,
                blob_key = %entry.blob_key,
                error = %e,
                "failed to remove chunk blob during partial cache miss cleanup"
            );
        }
    }

    // Remove the manifest blob itself
    let mkey = manifest_key(&manifest.caddr);
    if let Err(e) = blob_store.remove_chunk(&mkey).await {
        tracing::warn!(
            caddr = %manifest.caddr,
            manifest_key = %mkey,
            error = %e,
            "failed to remove manifest blob during partial cache miss cleanup"
        );
    }
}

// --- ChunkCacheReader (streaming read from cache) ---

/// Stream cached chunks from BlobStore based on a ChunkManifest.
///
/// Spawns a background task that reads chunks sequentially from the blob store
/// and sends them through a bounded channel. At most `capacity` chunks will be
/// in flight at once, bounding memory to `capacity × chunk_size`.
///
/// Returns a `Receiver<StreamChunk>` that the consumer reads from.
///
/// If any chunk blob referenced by the manifest is missing, the function treats
/// the value as a full cache miss: it cleans up the manifest and all remaining
/// chunk blobs, then sends a `StreamChunk::Error` before terminating.
pub fn stream_from_cache(
    manifest: &ChunkManifest,
    blob_store: Arc<dyn ChunkBlobStore>,
    capacity: usize,
) -> mpsc::Receiver<StreamChunk> {
    let manifest_clone = manifest.clone();
    let caddr = manifest.caddr;
    let (tx, rx) = mpsc::channel(capacity);

    tokio::spawn(async move {
        for entry in &manifest_clone.chunks {
            match blob_store.get_chunk(&entry.blob_key).await {
                Ok(Some(data)) => {
                    if tx.send(StreamChunk::Data(data)).await.is_err() {
                        // Consumer dropped the receiver — stop sending.
                        return;
                    }
                }
                Ok(None) => {
                    // Chunk blob missing — partial cache miss detected.
                    tracing::warn!(
                        caddr = %caddr,
                        blob_key = %entry.blob_key,
                        total_chunks = manifest_clone.chunks.len(),
                        "partial cache miss detected: chunk blob missing, cleaning up all cached data for CAddr"
                    );

                    // Clean up the manifest and all chunk blobs (best-effort)
                    cleanup_partial_cache_miss(&blob_store, &manifest_clone).await;

                    let _ = tx
                        .send(StreamChunk::Error(DerivaError::NotFound(format!(
                            "cache miss: chunk blob '{}' missing for CAddr {}",
                            entry.blob_key, caddr
                        ))))
                        .await;
                    return;
                }
                Err(e) => {
                    // Storage error reading chunk.
                    tracing::warn!(
                        caddr = %caddr,
                        blob_key = %entry.blob_key,
                        error = %e,
                        "storage error during streaming cache read"
                    );
                    let _ = tx.send(StreamChunk::Error(e)).await;
                    return;
                }
            }
        }

        // All chunks sent successfully — signal end of stream.
        let _ = tx.send(StreamChunk::End).await;
    });

    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_caddr() -> CAddr {
        CAddr::from_bytes(b"test data for caddr")
    }

    #[test]
    fn test_chunk_addr_blob_key() {
        let caddr = test_caddr();
        let addr = ChunkAddr { caddr, offset: 65536 };
        let key = addr.blob_key();
        assert!(key.starts_with(&caddr.to_hex()));
        assert!(key.ends_with("_chunk_65536"));
        assert_eq!(key, format!("{}_chunk_65536", caddr.to_hex()));
    }

    #[test]
    fn test_chunk_addr_deterministic() {
        let caddr = test_caddr();
        let a1 = ChunkAddr { caddr, offset: 0 };
        let a2 = ChunkAddr { caddr, offset: 0 };
        assert_eq!(a1.blob_key(), a2.blob_key());
    }

    #[test]
    fn test_chunk_addr_unique_for_different_offsets() {
        let caddr = test_caddr();
        let a1 = ChunkAddr { caddr, offset: 0 };
        let a2 = ChunkAddr { caddr, offset: 1 };
        assert_ne!(a1.blob_key(), a2.blob_key());
    }

    #[test]
    fn test_manifest_key() {
        let caddr = test_caddr();
        let key = manifest_key(&caddr);
        assert_eq!(key, format!("{}_manifest", caddr.to_hex()));
    }

    #[test]
    fn test_manifest_builder_and_validate() {
        let caddr = test_caddr();
        let mut builder = ChunkManifestBuilder::new(caddr, 64 * 1024);
        builder.add_chunk(65536, "key_0".to_string());
        builder.add_chunk(65536, "key_1".to_string());
        builder.add_chunk(1000, "key_2".to_string());

        let manifest = builder.build();
        assert_eq!(manifest.total_size, 65536 + 65536 + 1000);
        assert_eq!(manifest.chunks.len(), 3);
        assert_eq!(manifest.chunks[0].offset, 0);
        assert_eq!(manifest.chunks[1].offset, 65536);
        assert_eq!(manifest.chunks[2].offset, 131072);
        manifest.validate().unwrap();
    }

    #[test]
    fn test_manifest_validate_empty_zero_size() {
        let caddr = test_caddr();
        let manifest = ChunkManifest {
            caddr,
            total_size: 0,
            chunk_size: 64 * 1024,
            chunks: vec![],
        };
        manifest.validate().unwrap();
    }

    #[test]
    fn test_manifest_validate_empty_nonzero_fails() {
        let caddr = test_caddr();
        let manifest = ChunkManifest {
            caddr,
            total_size: 100,
            chunk_size: 64 * 1024,
            chunks: vec![],
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_manifest_validate_bad_first_offset() {
        let caddr = test_caddr();
        let manifest = ChunkManifest {
            caddr,
            total_size: 100,
            chunk_size: 64 * 1024,
            chunks: vec![ChunkEntry {
                offset: 10,
                length: 100,
                blob_key: "k".to_string(),
            }],
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_manifest_validate_non_contiguous() {
        let caddr = test_caddr();
        let manifest = ChunkManifest {
            caddr,
            total_size: 200,
            chunk_size: 64 * 1024,
            chunks: vec![
                ChunkEntry { offset: 0, length: 100, blob_key: "k0".to_string() },
                ChunkEntry { offset: 150, length: 100, blob_key: "k1".to_string() },
            ],
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_manifest_validate_size_mismatch() {
        let caddr = test_caddr();
        let manifest = ChunkManifest {
            caddr,
            total_size: 999,
            chunk_size: 64 * 1024,
            chunks: vec![ChunkEntry {
                offset: 0,
                length: 100,
                blob_key: "k".to_string(),
            }],
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_manifest_serialization_roundtrip() {
        let caddr = test_caddr();
        let mut builder = ChunkManifestBuilder::new(caddr, 1024);
        builder.add_chunk(1024, "blob_0".to_string());
        builder.add_chunk(512, "blob_1".to_string());
        let manifest = builder.build();

        let bytes = manifest.to_bytes().unwrap();
        let restored = ChunkManifest::from_bytes(&bytes).unwrap();

        assert_eq!(restored.caddr, manifest.caddr);
        assert_eq!(restored.total_size, manifest.total_size);
        assert_eq!(restored.chunk_size, manifest.chunk_size);
        assert_eq!(restored.chunks.len(), manifest.chunks.len());
        for (a, b) in restored.chunks.iter().zip(manifest.chunks.iter()) {
            assert_eq!(a.offset, b.offset);
            assert_eq!(a.length, b.length);
            assert_eq!(a.blob_key, b.blob_key);
        }
    }

    #[test]
    fn test_chunks_for_range_full_overlap() {
        let caddr = test_caddr();
        let mut builder = ChunkManifestBuilder::new(caddr, 100);
        builder.add_chunk(100, "c0".to_string());
        builder.add_chunk(100, "c1".to_string());
        builder.add_chunk(100, "c2".to_string());
        let manifest = builder.build();

        // Range covering all chunks
        let result = manifest.chunks_for_range(0, 300);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_chunks_for_range_partial() {
        let caddr = test_caddr();
        let mut builder = ChunkManifestBuilder::new(caddr, 100);
        builder.add_chunk(100, "c0".to_string());
        builder.add_chunk(100, "c1".to_string());
        builder.add_chunk(100, "c2".to_string());
        let manifest = builder.build();

        // Range that overlaps only chunk 1 and 2
        let result = manifest.chunks_for_range(150, 100);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].offset, 100);
        assert_eq!(result[1].offset, 200);
    }

    #[test]
    fn test_chunks_for_range_single_chunk() {
        let caddr = test_caddr();
        let mut builder = ChunkManifestBuilder::new(caddr, 100);
        builder.add_chunk(100, "c0".to_string());
        builder.add_chunk(100, "c1".to_string());
        builder.add_chunk(100, "c2".to_string());
        let manifest = builder.build();

        // Range within a single chunk
        let result = manifest.chunks_for_range(50, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].offset, 0);
    }

    #[test]
    fn test_chunks_for_range_empty() {
        let caddr = test_caddr();
        let mut builder = ChunkManifestBuilder::new(caddr, 100);
        builder.add_chunk(100, "c0".to_string());
        let manifest = builder.build();

        // Zero-length range
        let result = manifest.chunks_for_range(0, 0);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_chunks_for_range_beyond_end() {
        let caddr = test_caddr();
        let mut builder = ChunkManifestBuilder::new(caddr, 100);
        builder.add_chunk(100, "c0".to_string());
        let manifest = builder.build();

        // Range starting past all chunks
        let result = manifest.chunks_for_range(200, 50);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_builder_blob_keys() {
        let caddr = test_caddr();
        let mut builder = ChunkManifestBuilder::new(caddr, 1024);
        builder.add_chunk(1024, "key_a".to_string());
        builder.add_chunk(512, "key_b".to_string());

        let keys = builder.blob_keys();
        assert_eq!(keys, vec!["key_a".to_string(), "key_b".to_string()]);
    }

    // --- ChunkCacheWriter tests ---

    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory mock blob store for testing ChunkCacheWriter.
    struct MockBlobStore {
        blobs: Mutex<HashMap<String, Vec<u8>>>,
        /// If set, put_chunk will fail on the Nth call (0-indexed).
        fail_on_put: Option<usize>,
        put_count: Mutex<usize>,
    }

    impl MockBlobStore {
        fn new() -> Self {
            Self {
                blobs: Mutex::new(HashMap::new()),
                fail_on_put: None,
                put_count: Mutex::new(0),
            }
        }

        fn with_failure_on_put(n: usize) -> Self {
            Self {
                blobs: Mutex::new(HashMap::new()),
                fail_on_put: Some(n),
                put_count: Mutex::new(0),
            }
        }

        fn stored_keys(&self) -> Vec<String> {
            self.blobs.lock().unwrap().keys().cloned().collect()
        }

        fn get_blob(&self, key: &str) -> Option<Vec<u8>> {
            self.blobs.lock().unwrap().get(key).cloned()
        }
    }

    #[async_trait]
    impl ChunkBlobStore for MockBlobStore {
        async fn put_chunk(&self, key: &str, data: &[u8]) -> std::result::Result<(), DerivaError> {
            let mut count = self.put_count.lock().unwrap();
            if let Some(fail_at) = self.fail_on_put {
                if *count == fail_at {
                    *count += 1;
                    return Err(DerivaError::Storage("simulated write failure".into()));
                }
            }
            *count += 1;
            drop(count);
            self.blobs
                .lock()
                .unwrap()
                .insert(key.to_string(), data.to_vec());
            Ok(())
        }

        async fn get_chunk(
            &self,
            key: &str,
        ) -> std::result::Result<Option<Bytes>, DerivaError> {
            Ok(self
                .blobs
                .lock()
                .unwrap()
                .get(key)
                .map(|v| Bytes::from(v.clone())))
        }

        async fn remove_chunk(&self, key: &str) -> std::result::Result<bool, DerivaError> {
            Ok(self.blobs.lock().unwrap().remove(key).is_some())
        }
    }

    #[tokio::test]
    async fn test_chunk_cache_writer_single_chunk() {
        let caddr = test_caddr();
        let store = Arc::new(MockBlobStore::new());
        let writer = ChunkCacheWriter::new(caddr, 1024, store.clone());

        let (tx, rx_input) = mpsc::channel(4);
        let mut rx_output = writer.wrap(rx_input, 4);

        // Send one data chunk + End
        let data = Bytes::from(vec![42u8; 100]);
        tx.send(StreamChunk::Data(data.clone())).await.unwrap();
        tx.send(StreamChunk::End).await.unwrap();

        // Consume output
        let mut received = Vec::new();
        while let Some(chunk) = rx_output.recv().await {
            match chunk {
                StreamChunk::Data(b) => received.push(b),
                StreamChunk::End => break,
                StreamChunk::Error(e) => panic!("unexpected error: {}", e),
            }
        }

        // Verify forwarded data matches
        assert_eq!(received.len(), 1);
        assert_eq!(received[0], data);

        // Verify manifest was stored
        let mkey = manifest_key(&caddr);
        let manifest_data = store.get_blob(&mkey).expect("manifest should be stored");
        let manifest = ChunkManifest::from_bytes(&manifest_data).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.total_size, 100);
        assert_eq!(manifest.chunks.len(), 1);
    }

    #[tokio::test]
    async fn test_chunk_cache_writer_multiple_chunks() {
        let caddr = test_caddr();
        let store = Arc::new(MockBlobStore::new());
        let writer = ChunkCacheWriter::new(caddr, 64, store.clone());

        let (tx, rx_input) = mpsc::channel(8);
        let mut rx_output = writer.wrap(rx_input, 8);

        // Send three data chunks + End
        let chunks: Vec<Bytes> = vec![
            Bytes::from(vec![1u8; 64]),
            Bytes::from(vec![2u8; 64]),
            Bytes::from(vec![3u8; 32]),
        ];
        for c in &chunks {
            tx.send(StreamChunk::Data(c.clone())).await.unwrap();
        }
        tx.send(StreamChunk::End).await.unwrap();

        // Consume output
        let mut received = Vec::new();
        while let Some(chunk) = rx_output.recv().await {
            match chunk {
                StreamChunk::Data(b) => received.push(b),
                StreamChunk::End => break,
                StreamChunk::Error(e) => panic!("unexpected error: {}", e),
            }
        }

        assert_eq!(received.len(), 3);
        for (r, c) in received.iter().zip(chunks.iter()) {
            assert_eq!(r, c);
        }

        // Verify manifest
        let mkey = manifest_key(&caddr);
        let manifest_data = store.get_blob(&mkey).expect("manifest should be stored");
        let manifest = ChunkManifest::from_bytes(&manifest_data).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.total_size, 64 + 64 + 32);
        assert_eq!(manifest.chunks.len(), 3);
        assert_eq!(manifest.chunks[0].offset, 0);
        assert_eq!(manifest.chunks[1].offset, 64);
        assert_eq!(manifest.chunks[2].offset, 128);
    }

    #[tokio::test]
    async fn test_chunk_cache_writer_empty_stream() {
        let caddr = test_caddr();
        let store = Arc::new(MockBlobStore::new());
        let writer = ChunkCacheWriter::new(caddr, 1024, store.clone());

        let (tx, rx_input) = mpsc::channel(4);
        let mut rx_output = writer.wrap(rx_input, 4);

        // Send End immediately (empty stream)
        tx.send(StreamChunk::End).await.unwrap();

        // Consume output
        match rx_output.recv().await {
            Some(StreamChunk::End) => {}
            other => panic!("expected End, got {:?}", other.map(|c| c.is_end())),
        }

        // Verify manifest for empty value
        let mkey = manifest_key(&caddr);
        let manifest_data = store.get_blob(&mkey).expect("manifest should be stored");
        let manifest = ChunkManifest::from_bytes(&manifest_data).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.total_size, 0);
        assert_eq!(manifest.chunks.len(), 0);
    }

    #[tokio::test]
    async fn test_chunk_cache_writer_error_cleanup() {
        let caddr = test_caddr();
        // Fail on the 3rd put (0-indexed: chunks 0,1 succeed, chunk 2 fails)
        let store = Arc::new(MockBlobStore::with_failure_on_put(2));
        let writer = ChunkCacheWriter::new(caddr, 64, store.clone());

        let (tx, rx_input) = mpsc::channel(8);
        let mut rx_output = writer.wrap(rx_input, 8);

        // Send chunks - 3rd one will fail to store
        for i in 0..3 {
            tx.send(StreamChunk::Data(Bytes::from(vec![i; 64])))
                .await
                .unwrap();
        }

        // Consume output - should get 2 Data and then an Error
        let mut data_count = 0;
        let mut got_error = false;
        while let Some(chunk) = rx_output.recv().await {
            match chunk {
                StreamChunk::Data(_) => data_count += 1,
                StreamChunk::Error(_) => {
                    got_error = true;
                    break;
                }
                StreamChunk::End => panic!("should not get End"),
            }
        }

        assert_eq!(data_count, 2);
        assert!(got_error);

        // Verify cleanup: no chunks and no manifest should remain
        let keys = store.stored_keys();
        assert!(keys.is_empty(), "all chunks should be cleaned up, but found: {:?}", keys);
    }

    #[tokio::test]
    async fn test_chunk_cache_writer_upstream_error() {
        let caddr = test_caddr();
        let store = Arc::new(MockBlobStore::new());
        let writer = ChunkCacheWriter::new(caddr, 64, store.clone());

        let (tx, rx_input) = mpsc::channel(8);
        let mut rx_output = writer.wrap(rx_input, 8);

        // Send one chunk then an error
        tx.send(StreamChunk::Data(Bytes::from(vec![1u8; 64])))
            .await
            .unwrap();
        tx.send(StreamChunk::Error(DerivaError::ComputeFailed(
            "test error".into(),
        )))
        .await
        .unwrap();

        // Consume output
        let mut data_count = 0;
        let mut got_error = false;
        while let Some(chunk) = rx_output.recv().await {
            match chunk {
                StreamChunk::Data(_) => data_count += 1,
                StreamChunk::Error(_) => {
                    got_error = true;
                    break;
                }
                StreamChunk::End => panic!("should not get End"),
            }
        }

        assert_eq!(data_count, 1);
        assert!(got_error);

        // Verify cleanup: chunks removed, no manifest
        let keys = store.stored_keys();
        assert!(keys.is_empty(), "all chunks should be cleaned up, but found: {:?}", keys);
    }

    #[tokio::test]
    async fn test_chunk_cache_writer_consumer_drop() {
        let caddr = test_caddr();
        let store = Arc::new(MockBlobStore::new());
        let writer = ChunkCacheWriter::new(caddr, 64, store.clone());

        let (tx, rx_input) = mpsc::channel(8);
        let rx_output = writer.wrap(rx_input, 1); // capacity 1 to make it block

        // Drop the output receiver immediately
        drop(rx_output);

        // Send chunks — the writer should detect the consumer drop
        tx.send(StreamChunk::Data(Bytes::from(vec![1u8; 64])))
            .await
            .unwrap();

        // Give the background task time to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Verify cleanup: chunks removed, no manifest
        let keys = store.stored_keys();
        assert!(keys.is_empty(), "all chunks should be cleaned up, but found: {:?}", keys);
    }

    // --- stream_from_cache tests ---

    /// Helper: populate a MockBlobStore with chunk data and return the manifest.
    fn setup_cached_chunks(
        store: &MockBlobStore,
        caddr: CAddr,
        chunk_data: &[&[u8]],
    ) -> ChunkManifest {
        let mut builder = ChunkManifestBuilder::new(caddr, 1024);
        for (i, data) in chunk_data.iter().enumerate() {
            let addr = ChunkAddr {
                caddr,
                offset: builder.current_offset,
            };
            let key = addr.blob_key();
            store
                .blobs
                .lock()
                .unwrap()
                .insert(key.clone(), data.to_vec());
            builder.add_chunk(data.len() as u32, key);
            let _ = i;
        }
        builder.build()
    }

    #[tokio::test]
    async fn test_stream_from_cache_success() {
        let caddr = test_caddr();
        let store = Arc::new(MockBlobStore::new());

        let chunk_data: Vec<Vec<u8>> = vec![
            vec![1u8; 64],
            vec![2u8; 64],
            vec![3u8; 32],
        ];
        let manifest = setup_cached_chunks(
            &store,
            caddr,
            &chunk_data.iter().map(|v| v.as_slice()).collect::<Vec<_>>(),
        );

        let mut rx = stream_from_cache(&manifest, store.clone(), 4);

        // Collect all data chunks
        let mut received = Vec::new();
        let mut got_end = false;
        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Data(b) => received.push(b),
                StreamChunk::End => {
                    got_end = true;
                    break;
                }
                StreamChunk::Error(e) => panic!("unexpected error: {}", e),
            }
        }

        assert!(got_end, "should receive StreamChunk::End");
        assert_eq!(received.len(), 3);
        assert_eq!(received[0].as_ref(), &[1u8; 64]);
        assert_eq!(received[1].as_ref(), &[2u8; 64]);
        assert_eq!(received[2].as_ref(), &[3u8; 32]);
    }

    #[tokio::test]
    async fn test_stream_from_cache_missing_chunk() {
        let caddr = test_caddr();
        let store = Arc::new(MockBlobStore::new());

        // Build a manifest with 3 chunks but only store 2 of them
        let chunk_data: Vec<Vec<u8>> = vec![
            vec![1u8; 64],
            vec![2u8; 64],
            vec![3u8; 32],
        ];
        let manifest = setup_cached_chunks(
            &store,
            caddr,
            &chunk_data.iter().map(|v| v.as_slice()).collect::<Vec<_>>(),
        );

        // Remove the second chunk to simulate a partial miss
        let missing_key = &manifest.chunks[1].blob_key;
        store.blobs.lock().unwrap().remove(missing_key);

        let mut rx = stream_from_cache(&manifest, store.clone(), 4);

        // Should get first chunk as Data, then Error for the missing one
        let mut data_count = 0;
        let mut got_error = false;
        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Data(_) => data_count += 1,
                StreamChunk::Error(_) => {
                    got_error = true;
                    break;
                }
                StreamChunk::End => panic!("should not get End when a chunk is missing"),
            }
        }

        assert_eq!(data_count, 1, "should receive first chunk before error");
        assert!(got_error, "should receive error for missing chunk");
    }

    #[tokio::test]
    async fn test_stream_from_cache_empty_manifest() {
        let caddr = test_caddr();
        let store = Arc::new(MockBlobStore::new());

        // Empty manifest — zero chunks, zero total_size
        let manifest = ChunkManifest {
            caddr,
            total_size: 0,
            chunk_size: 1024,
            chunks: vec![],
        };

        let mut rx = stream_from_cache(&manifest, store.clone(), 4);

        // Should immediately get End
        match rx.recv().await {
            Some(StreamChunk::End) => {}
            Some(StreamChunk::Data(_)) => panic!("should not get Data for empty manifest"),
            Some(StreamChunk::Error(e)) => panic!("unexpected error: {}", e),
            None => panic!("channel closed without End"),
        }

        // Channel should be done after End
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn test_stream_from_cache_partial_miss_cleanup() {
        let caddr = test_caddr();
        let store = Arc::new(MockBlobStore::new());

        // Store 3 chunks and a manifest, then remove the 2nd chunk to simulate partial miss
        let chunk_data: Vec<Vec<u8>> = vec![
            vec![1u8; 64],
            vec![2u8; 64],
            vec![3u8; 32],
        ];
        let manifest = setup_cached_chunks(
            &store,
            caddr,
            &chunk_data.iter().map(|v| v.as_slice()).collect::<Vec<_>>(),
        );

        // Store the manifest blob too (so cleanup can remove it)
        let manifest_bytes = manifest.to_bytes().unwrap();
        let mkey = manifest_key(&caddr);
        store
            .blobs
            .lock()
            .unwrap()
            .insert(mkey.clone(), manifest_bytes);

        // Remove the second chunk to create a partial miss
        let missing_key = manifest.chunks[1].blob_key.clone();
        store.blobs.lock().unwrap().remove(&missing_key);

        // Verify that we have blobs stored before the read (manifest + 2 remaining chunks)
        assert_eq!(store.stored_keys().len(), 3); // chunk_0, chunk_2, manifest

        let mut rx = stream_from_cache(&manifest, store.clone(), 4);

        // Consume the stream — should get first chunk as Data, then Error
        let mut data_count = 0;
        let mut got_error = false;
        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Data(_) => data_count += 1,
                StreamChunk::Error(_) => {
                    got_error = true;
                    break;
                }
                StreamChunk::End => panic!("should not get End when a chunk is missing"),
            }
        }

        assert_eq!(data_count, 1, "should receive first chunk before error");
        assert!(got_error, "should receive error for missing chunk");

        // Give the background task a moment to complete cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Verify cleanup: ALL blobs should be removed (manifest + remaining chunks)
        let remaining_keys = store.stored_keys();
        assert!(
            remaining_keys.is_empty(),
            "all blobs should be cleaned up after partial miss, but found: {:?}",
            remaining_keys
        );
    }

    #[tokio::test]
    async fn test_cleanup_partial_cache_miss() {
        let caddr = test_caddr();
        let store = Arc::new(MockBlobStore::new());

        // Set up chunks and manifest in the store
        let chunk_data: Vec<Vec<u8>> = vec![
            vec![10u8; 128],
            vec![20u8; 128],
            vec![30u8; 64],
        ];
        let manifest = setup_cached_chunks(
            &store,
            caddr,
            &chunk_data.iter().map(|v| v.as_slice()).collect::<Vec<_>>(),
        );

        // Store the manifest blob
        let manifest_bytes = manifest.to_bytes().unwrap();
        let mkey = manifest_key(&caddr);
        store
            .blobs
            .lock()
            .unwrap()
            .insert(mkey.clone(), manifest_bytes);

        // Verify blobs are present (3 chunks + 1 manifest = 4)
        assert_eq!(store.stored_keys().len(), 4);

        // Call cleanup directly
        cleanup_partial_cache_miss(&(store.clone() as Arc<dyn ChunkBlobStore>), &manifest).await;

        // Verify all blobs are removed
        let remaining_keys = store.stored_keys();
        assert!(
            remaining_keys.is_empty(),
            "all blobs should be removed after cleanup, but found: {:?}",
            remaining_keys
        );
    }
}
