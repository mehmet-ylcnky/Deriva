use deriva_core::address::CAddr;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    current_offset: u64,
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
}
