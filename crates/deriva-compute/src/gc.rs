use std::collections::HashSet;
use std::sync::Arc;
use deriva_core::CAddr;
use deriva_core::gc::PinSet;
use deriva_core::persistent_dag::PersistentDag;

use crate::chunk_cache::{manifest_key, ChunkBlobStore, ChunkManifest};

/// Compute the set of live CAddrs that must not be garbage collected.
pub fn compute_live_set(dag: &PersistentDag, pins: &PinSet) -> HashSet<CAddr> {
    let mut live = dag.live_addr_set();
    for addr in pins.as_set() {
        live.insert(*addr);
    }
    live
}

/// Result of chunk-level garbage collection for a set of unreachable CAddrs.
#[derive(Debug, Clone, Default)]
pub struct ChunkGcResult {
    /// Number of individual chunk blobs successfully removed.
    pub chunk_blobs_removed: u64,
    /// Number of manifest blobs successfully removed.
    pub manifests_removed: u64,
    /// Number of individual chunk/manifest removal operations that failed.
    pub removal_errors: u64,
    /// Estimated bytes reclaimed from chunk blob removal (sum of entry.length for removed chunks).
    pub chunk_bytes_reclaimed: u64,
}

/// Remove chunk-cached entries for unreachable CAddrs.
///
/// For each CAddr:
/// 1. Try to read its ChunkManifest from the blob store
/// 2. If manifest exists: remove all referenced chunk blobs + the manifest blob
/// 3. If no manifest exists: this is a legacy entry (handled elsewhere)
/// 4. If a chunk removal fails: log warning, continue with remaining chunks
///
/// Returns (chunk_blobs_removed, chunk_bytes_reclaimed) for reporting in GcResult.
pub async fn remove_chunk_cached_entries(
    addrs: &[CAddr],
    blob_store: &Arc<dyn ChunkBlobStore>,
) -> (u64, u64) {
    let result = gc_chunk_entries(blob_store, addrs).await;
    (result.chunk_blobs_removed, result.chunk_bytes_reclaimed)
}

/// Remove all chunk blobs and manifests for unreachable CAddrs.
///
/// For each CAddr in `unreachable_addrs`:
/// - If a ChunkManifest exists: parse it, remove all referenced chunk blobs,
///   then remove the manifest blob itself.
/// - If no manifest exists: this is a legacy entry handled by the caller's
///   existing blob removal path (skipped here).
/// - If any individual chunk removal fails: log at warn level, continue with
///   remaining chunks.
pub async fn gc_chunk_entries(
    blob_store: &Arc<dyn ChunkBlobStore>,
    unreachable_addrs: &[CAddr],
) -> ChunkGcResult {
    let mut result = ChunkGcResult::default();

    for addr in unreachable_addrs {
        let mkey = manifest_key(addr);

        // Try to load the manifest for this CAddr
        let manifest_data = match blob_store.get_chunk(&mkey).await {
            Ok(Some(data)) => data,
            Ok(None) => {
                // No manifest — legacy single-blob entry, skip (handled elsewhere)
                continue;
            }
            Err(e) => {
                tracing::warn!(
                    caddr = %addr,
                    manifest_key = %mkey,
                    error = %e,
                    "failed to read manifest during GC, skipping CAddr"
                );
                result.removal_errors += 1;
                continue;
            }
        };

        // Parse the manifest
        let manifest = match ChunkManifest::from_bytes(&manifest_data) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    caddr = %addr,
                    error = %e,
                    "failed to parse ChunkManifest during GC, skipping CAddr"
                );
                result.removal_errors += 1;
                continue;
            }
        };

        // Remove all chunk blobs referenced by the manifest
        for entry in &manifest.chunks {
            match blob_store.remove_chunk(&entry.blob_key).await {
                Ok(_) => {
                    result.chunk_blobs_removed += 1;
                    result.chunk_bytes_reclaimed += entry.length as u64;
                }
                Err(e) => {
                    tracing::warn!(
                        caddr = %addr,
                        blob_key = %entry.blob_key,
                        error = %e,
                        "failed to remove chunk blob during GC"
                    );
                    result.removal_errors += 1;
                }
            }
        }

        // Remove the manifest blob itself
        match blob_store.remove_chunk(&mkey).await {
            Ok(_) => {
                result.manifests_removed += 1;
            }
            Err(e) => {
                tracing::warn!(
                    caddr = %addr,
                    manifest_key = %mkey,
                    error = %e,
                    "failed to remove manifest blob during GC"
                );
                result.removal_errors += 1;
            }
        }
    }

    result
}

/// Detect and remove orphan chunk blobs that have no corresponding manifest.
///
/// Orphan chunks are remnants of interrupted ChunkCacheWriter writes.
/// A chunk blob `{caddr_hex}_chunk_{offset}` is an orphan if no
/// `{caddr_hex}_manifest` blob exists in the store.
///
/// Returns the number of orphan chunks removed.
pub async fn remove_orphan_chunks(
    blob_store: &Arc<dyn ChunkBlobStore>,
) -> u64 {
    // 1. List all chunk keys from the store
    let chunk_keys = match blob_store.list_chunk_keys().await {
        Ok(keys) => keys,
        Err(e) => {
            tracing::warn!(error = %e, "failed to list chunk keys for orphan detection");
            return 0;
        }
    };

    if chunk_keys.is_empty() {
        return 0;
    }

    // 2. Collect unique CAddr hex prefixes from chunk keys
    let mut prefix_to_chunks: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for key in &chunk_keys {
        if let Some(prefix) = key.split("_chunk_").next() {
            prefix_to_chunks
                .entry(prefix.to_string())
                .or_default()
                .push(key.clone());
        }
    }

    // 3. Check manifest existence for each unique prefix
    let mut orphan_count: u64 = 0;
    for (prefix, chunks) in &prefix_to_chunks {
        let mkey = format!("{}_manifest", prefix);
        let has_manifest = match blob_store.get_chunk(&mkey).await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => {
                tracing::warn!(
                    prefix = %prefix,
                    error = %e,
                    "failed to check manifest existence during orphan detection, skipping prefix"
                );
                continue;
            }
        };

        // 4. If no manifest, remove all chunks for this prefix
        if !has_manifest {
            tracing::warn!(
                prefix = %prefix,
                orphan_chunks = chunks.len(),
                "detected orphan chunks with no manifest, removing"
            );
            for chunk_key in chunks {
                match blob_store.remove_chunk(chunk_key).await {
                    Ok(_) => {
                        orphan_count += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            chunk_key = %chunk_key,
                            error = %e,
                            "failed to remove orphan chunk"
                        );
                    }
                }
            }
        }
    }

    orphan_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use bytes::Bytes;
    use deriva_core::DerivaError;
    use std::collections::HashMap;
    use std::sync::Mutex;

    use crate::chunk_cache::{ChunkEntry, ChunkManifest};

    /// A mock ChunkBlobStore for testing GC behavior.
    struct MockBlobStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
        /// Keys that should fail on remove.
        fail_on_remove: Mutex<HashSet<String>>,
    }

    impl MockBlobStore {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
                fail_on_remove: Mutex::new(HashSet::new()),
            }
        }

        fn insert(&self, key: &str, value: Vec<u8>) {
            self.data.lock().unwrap().insert(key.to_string(), value);
        }

        fn set_fail_on_remove(&self, key: &str) {
            self.fail_on_remove
                .lock()
                .unwrap()
                .insert(key.to_string());
        }

        fn contains(&self, key: &str) -> bool {
            self.data.lock().unwrap().contains_key(key)
        }
    }

    #[async_trait]
    impl ChunkBlobStore for MockBlobStore {
        async fn put_chunk(&self, key: &str, data: &[u8]) -> Result<(), DerivaError> {
            self.data
                .lock()
                .unwrap()
                .insert(key.to_string(), data.to_vec());
            Ok(())
        }

        async fn get_chunk(&self, key: &str) -> Result<Option<Bytes>, DerivaError> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .get(key)
                .map(|v| Bytes::from(v.clone())))
        }

        async fn remove_chunk(&self, key: &str) -> Result<bool, DerivaError> {
            if self.fail_on_remove.lock().unwrap().contains(key) {
                return Err(DerivaError::Storage(format!(
                    "simulated removal failure for {}",
                    key
                )));
            }
            Ok(self.data.lock().unwrap().remove(key).is_some())
        }

        async fn list_chunk_keys(&self) -> Result<Vec<String>, DerivaError> {
            let data = self.data.lock().unwrap();
            Ok(data
                .keys()
                .filter(|k| k.contains("_chunk_"))
                .cloned()
                .collect())
        }
    }

    fn test_caddr(seed: u8) -> CAddr {
        CAddr::from_raw([seed; 32])
    }

    fn store_manifest(store: &MockBlobStore, caddr: &CAddr, manifest: &ChunkManifest) {
        let mkey = manifest_key(caddr);
        let bytes = manifest.to_bytes().unwrap();
        store.insert(&mkey, bytes);
    }

    #[tokio::test]
    async fn gc_removes_all_chunks_and_manifest() {
        let store = Arc::new(MockBlobStore::new());
        let caddr = test_caddr(1);

        let chunk_keys: Vec<String> = (0..3)
            .map(|i| format!("{}_chunk_{}", caddr.to_hex(), i * 1024))
            .collect();

        // Store chunk blobs
        for key in &chunk_keys {
            store.insert(key, vec![0u8; 1024]);
        }

        // Build and store manifest
        let manifest = ChunkManifest {
            caddr,
            total_size: 3072,
            chunk_size: 1024,
            chunks: chunk_keys
                .iter()
                .enumerate()
                .map(|(i, key)| ChunkEntry {
                    offset: (i as u64) * 1024,
                    length: 1024,
                    blob_key: key.clone(),
                })
                .collect(),
        };
        store_manifest(&store, &caddr, &manifest);

        let result =
            gc_chunk_entries(&(store.clone() as Arc<dyn ChunkBlobStore>), &[caddr]).await;

        assert_eq!(result.chunk_blobs_removed, 3);
        assert_eq!(result.manifests_removed, 1);
        assert_eq!(result.removal_errors, 0);

        // All blobs should be gone
        for key in &chunk_keys {
            assert!(!store.contains(key));
        }
        assert!(!store.contains(&manifest_key(&caddr)));
    }

    #[tokio::test]
    async fn gc_skips_legacy_entries_without_manifest() {
        let store = Arc::new(MockBlobStore::new());
        let caddr = test_caddr(2);

        // No manifest stored — this is a legacy entry
        let result =
            gc_chunk_entries(&(store.clone() as Arc<dyn ChunkBlobStore>), &[caddr]).await;

        assert_eq!(result.chunk_blobs_removed, 0);
        assert_eq!(result.manifests_removed, 0);
        assert_eq!(result.removal_errors, 0);
    }

    #[tokio::test]
    async fn gc_continues_on_chunk_removal_failure() {
        let store = Arc::new(MockBlobStore::new());
        let caddr = test_caddr(3);

        let chunk_keys: Vec<String> = (0..3)
            .map(|i| format!("{}_chunk_{}", caddr.to_hex(), i * 1024))
            .collect();

        for key in &chunk_keys {
            store.insert(key, vec![0u8; 1024]);
        }

        let manifest = ChunkManifest {
            caddr,
            total_size: 3072,
            chunk_size: 1024,
            chunks: chunk_keys
                .iter()
                .enumerate()
                .map(|(i, key)| ChunkEntry {
                    offset: (i as u64) * 1024,
                    length: 1024,
                    blob_key: key.clone(),
                })
                .collect(),
        };
        store_manifest(&store, &caddr, &manifest);

        // Make the second chunk fail on remove
        store.set_fail_on_remove(&chunk_keys[1]);

        let result =
            gc_chunk_entries(&(store.clone() as Arc<dyn ChunkBlobStore>), &[caddr]).await;

        // First and third chunk removed successfully, second failed
        assert_eq!(result.chunk_blobs_removed, 2);
        assert_eq!(result.manifests_removed, 1);
        assert_eq!(result.removal_errors, 1);

        // First and third are gone, second still present
        assert!(!store.contains(&chunk_keys[0]));
        assert!(store.contains(&chunk_keys[1]));
        assert!(!store.contains(&chunk_keys[2]));
        assert!(!store.contains(&manifest_key(&caddr)));
    }

    #[tokio::test]
    async fn gc_handles_multiple_addrs() {
        let store = Arc::new(MockBlobStore::new());

        let caddr1 = test_caddr(10);
        let caddr2 = test_caddr(20);

        // First CAddr: has manifest with 2 chunks
        let keys1: Vec<String> = (0..2)
            .map(|i| format!("{}_chunk_{}", caddr1.to_hex(), i * 512))
            .collect();
        for key in &keys1 {
            store.insert(key, vec![0u8; 512]);
        }
        let manifest1 = ChunkManifest {
            caddr: caddr1,
            total_size: 1024,
            chunk_size: 512,
            chunks: keys1
                .iter()
                .enumerate()
                .map(|(i, key)| ChunkEntry {
                    offset: (i as u64) * 512,
                    length: 512,
                    blob_key: key.clone(),
                })
                .collect(),
        };
        store_manifest(&store, &caddr1, &manifest1);

        // Second CAddr: no manifest (legacy)
        // Nothing stored for caddr2

        let result = gc_chunk_entries(
            &(store.clone() as Arc<dyn ChunkBlobStore>),
            &[caddr1, caddr2],
        )
        .await;

        assert_eq!(result.chunk_blobs_removed, 2);
        assert_eq!(result.manifests_removed, 1);
        assert_eq!(result.removal_errors, 0);
    }

    // --- Orphan chunk detection tests ---

    #[tokio::test]
    async fn orphan_chunks_with_no_manifest_are_removed() {
        let store = Arc::new(MockBlobStore::new());
        let caddr = test_caddr(50);

        // Store chunk blobs without a manifest (simulating interrupted write)
        let chunk_keys: Vec<String> = (0..3)
            .map(|i| format!("{}_chunk_{}", caddr.to_hex(), i * 1024))
            .collect();
        for key in &chunk_keys {
            store.insert(key, vec![0u8; 1024]);
        }

        let removed = remove_orphan_chunks(&(store.clone() as Arc<dyn ChunkBlobStore>)).await;

        assert_eq!(removed, 3);
        for key in &chunk_keys {
            assert!(!store.contains(key));
        }
    }

    #[tokio::test]
    async fn chunks_with_manifest_are_not_removed_as_orphans() {
        let store = Arc::new(MockBlobStore::new());
        let caddr = test_caddr(60);

        // Store chunk blobs AND a manifest
        let chunk_keys: Vec<String> = (0..2)
            .map(|i| format!("{}_chunk_{}", caddr.to_hex(), i * 1024))
            .collect();
        for key in &chunk_keys {
            store.insert(key, vec![0u8; 1024]);
        }
        let manifest = ChunkManifest {
            caddr,
            total_size: 2048,
            chunk_size: 1024,
            chunks: chunk_keys
                .iter()
                .enumerate()
                .map(|(i, key)| ChunkEntry {
                    offset: (i as u64) * 1024,
                    length: 1024,
                    blob_key: key.clone(),
                })
                .collect(),
        };
        store_manifest(&store, &caddr, &manifest);

        let removed = remove_orphan_chunks(&(store.clone() as Arc<dyn ChunkBlobStore>)).await;

        assert_eq!(removed, 0);
        for key in &chunk_keys {
            assert!(store.contains(key));
        }
    }

    #[tokio::test]
    async fn mixed_orphan_and_valid_chunks() {
        let store = Arc::new(MockBlobStore::new());

        // Valid: caddr1 has chunks + manifest
        let caddr1 = test_caddr(70);
        let valid_keys: Vec<String> = (0..2)
            .map(|i| format!("{}_chunk_{}", caddr1.to_hex(), i * 512))
            .collect();
        for key in &valid_keys {
            store.insert(key, vec![0u8; 512]);
        }
        let manifest = ChunkManifest {
            caddr: caddr1,
            total_size: 1024,
            chunk_size: 512,
            chunks: valid_keys
                .iter()
                .enumerate()
                .map(|(i, key)| ChunkEntry {
                    offset: (i as u64) * 512,
                    length: 512,
                    blob_key: key.clone(),
                })
                .collect(),
        };
        store_manifest(&store, &caddr1, &manifest);

        // Orphan: caddr2 has chunks but NO manifest
        let caddr2 = test_caddr(80);
        let orphan_keys: Vec<String> = (0..3)
            .map(|i| format!("{}_chunk_{}", caddr2.to_hex(), i * 256))
            .collect();
        for key in &orphan_keys {
            store.insert(key, vec![0u8; 256]);
        }

        let removed = remove_orphan_chunks(&(store.clone() as Arc<dyn ChunkBlobStore>)).await;

        // Only orphan chunks removed
        assert_eq!(removed, 3);

        // Valid chunks still present
        for key in &valid_keys {
            assert!(store.contains(key));
        }
        // Orphan chunks gone
        for key in &orphan_keys {
            assert!(!store.contains(key));
        }
    }

    #[tokio::test]
    async fn orphan_detection_with_no_chunks() {
        let store = Arc::new(MockBlobStore::new());

        // Empty store — no chunks at all
        let removed = remove_orphan_chunks(&(store.clone() as Arc<dyn ChunkBlobStore>)).await;
        assert_eq!(removed, 0);
    }
}
