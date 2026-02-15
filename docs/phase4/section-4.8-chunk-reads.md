# Phase 4, Section 4.8: Chunk-Level Partial Reads

**Status:** Blueprint  
**Depends on:** §4.1 (Canonical Serialization), §4.5 (Content Integrity), §4.7 (FUSE)  
**Blocks:** §4.10 (REST)

---

## 1. Problem Statement

### 1.1 Current State

Deriva stores blobs as **monolithic values**:

```rust
// Current approach: entire blob
let bytes = dag.get(addr).await?;  // Must download entire blob
```

**Problems:**
1. **No partial reads**: Must download entire blob to read any part
2. **No streaming**: Can't start processing before full download
3. **No seeking**: Can't jump to specific offset without full download
4. **Memory overhead**: Large blobs consume excessive memory
5. **Network waste**: Download unused data

### 1.2 The Problem

**Scenario 1: Read Last 100 Bytes of 10 GB File**
```rust
// User wants to read file footer
let bytes = dag.get(addr).await?;  // ❌ Downloads entire 10 GB
let footer = &bytes[bytes.len() - 100..];
```

**Scenario 2: Stream Video**
```rust
// User wants to stream video from middle
let bytes = dag.get(addr).await?;  // ❌ Must download from start
let chunk = &bytes[seek_position..seek_position + 1024*1024];
```

**Scenario 3: Random Access Database**
```rust
// Database wants to read specific page
let bytes = dag.get(addr).await?;  // ❌ Downloads entire database
let page = &bytes[page_offset..page_offset + 4096];
```

### 1.3 Requirements

1. **Chunk storage**: Split large blobs into fixed-size chunks
2. **Partial reads**: Fetch only requested byte ranges
3. **Streaming**: Start processing before full download
4. **Seeking**: Jump to arbitrary offsets efficiently
5. **Integrity**: Verify chunks independently (§4.5)
6. **Transparency**: Existing code works without changes
7. **Performance**: <10ms overhead for chunk lookup, >100 MB/s throughput

---

## 2. Design

### 2.1 Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Chunk Storage Layer                      │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ ChunkStore   │  │ ChunkManifest│  │ ChunkReader  │      │
│  │              │  │              │  │              │      │
│  │ • put_chunked│  │ • Chunk list │  │ • Partial    │      │
│  │ • get_range  │  │ • Total size │  │ • Streaming  │      │
│  │ • get_chunk  │  │ • Chunk size │  │ • Parallel   │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                               │
├─────────────────────────────────────────────────────────────┤
│                      DagStore (§2.3)                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ VerifiedGet  │  │  BlobStore   │  │  RecipeStore │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Chunk Manifest

```rust
// crates/deriva-storage/src/chunk_manifest.rs

/// Chunk manifest for large blobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkManifest {
    /// Total size of blob
    pub total_size: u64,
    /// Chunk size (all chunks except last)
    pub chunk_size: u64,
    /// List of chunk CAddrs
    pub chunks: Vec<CAddr>,
    /// Content type (optional)
    pub content_type: Option<String>,
}

impl ChunkManifest {
    /// Default chunk size: 1 MB
    pub const DEFAULT_CHUNK_SIZE: u64 = 1024 * 1024;
    
    /// Threshold for chunking: 10 MB
    pub const CHUNKING_THRESHOLD: u64 = 10 * 1024 * 1024;
    
    /// Create manifest from blob
    pub fn from_blob(blob: &[u8], chunk_size: u64) -> (Self, Vec<Bytes>) {
        let total_size = blob.len() as u64;
        let mut chunks = Vec::new();
        let mut chunk_addrs = Vec::new();
        
        for chunk_data in blob.chunks(chunk_size as usize) {
            let chunk_bytes = Bytes::from(chunk_data.to_vec());
            let chunk_addr = CAddr::from_bytes(&chunk_bytes);
            chunks.push(chunk_bytes);
            chunk_addrs.push(chunk_addr);
        }
        
        let manifest = Self {
            total_size,
            chunk_size,
            chunks: chunk_addrs,
            content_type: None,
        };
        
        (manifest, chunks)
    }
    
    /// Get chunk index for byte offset
    pub fn chunk_index(&self, offset: u64) -> usize {
        (offset / self.chunk_size) as usize
    }
    
    /// Get byte range for chunk
    pub fn chunk_range(&self, index: usize) -> (u64, u64) {
        let start = index as u64 * self.chunk_size;
        let end = (start + self.chunk_size).min(self.total_size);
        (start, end)
    }
    
    /// Get chunks needed for byte range
    pub fn chunks_for_range(&self, offset: u64, length: u64) -> Vec<usize> {
        let start_chunk = self.chunk_index(offset);
        let end_offset = offset + length;
        let end_chunk = self.chunk_index(end_offset.saturating_sub(1));
        
        (start_chunk..=end_chunk.min(self.chunks.len() - 1)).collect()
    }
}
```

### 2.3 Chunk Store

```rust
// crates/deriva-storage/src/chunk_store.rs

/// Chunk-aware storage layer
pub struct ChunkStore {
    dag: Arc<DagStore>,
    chunk_size: u64,
}

impl ChunkStore {
    pub fn new(dag: Arc<DagStore>) -> Self {
        Self {
            dag,
            chunk_size: ChunkManifest::DEFAULT_CHUNK_SIZE,
        }
    }
    
    /// Put blob with automatic chunking
    pub async fn put_chunked(&self, blob: Bytes) -> Result<CAddr, StorageError> {
        // Small blobs: store directly
        if blob.len() < ChunkManifest::CHUNKING_THRESHOLD as usize {
            let addr = CAddr::from_bytes(&blob);
            self.dag.put(addr, blob).await?;
            return Ok(addr);
        }
        
        // Large blobs: chunk and create manifest
        let (manifest, chunks) = ChunkManifest::from_blob(&blob, self.chunk_size);
        
        // Store chunks
        for (chunk_addr, chunk_bytes) in manifest.chunks.iter().zip(chunks.iter()) {
            self.dag.put(*chunk_addr, chunk_bytes.clone()).await?;
        }
        
        // Store manifest
        let manifest_bytes = bincode::serialize(&manifest)?;
        let manifest_addr = CAddr::from_bytes(&manifest_bytes);
        self.dag.put(manifest_addr, manifest_bytes.into()).await?;
        
        Ok(manifest_addr)
    }
    
    /// Get byte range from chunked blob
    pub async fn get_range(
        &self,
        addr: CAddr,
        offset: u64,
        length: u64,
    ) -> Result<Bytes, StorageError> {
        // Try to get as manifest
        let bytes = self.dag.get(addr).await
            .ok_or(StorageError::NotFound(addr))?;
        
        let manifest: ChunkManifest = match bincode::deserialize(&bytes) {
            Ok(m) => m,
            Err(_) => {
                // Not a manifest, treat as regular blob
                let start = offset as usize;
                let end = (offset + length).min(bytes.len() as u64) as usize;
                return Ok(bytes.slice(start..end));
            }
        };
        
        // Get required chunks
        let chunk_indices = manifest.chunks_for_range(offset, length);
        
        // Fetch chunks in parallel
        let chunk_futures: Vec<_> = chunk_indices.iter()
            .map(|&idx| {
                let dag = self.dag.clone();
                let chunk_addr = manifest.chunks[idx];
                async move {
                    dag.get(chunk_addr).await
                        .ok_or(StorageError::NotFound(chunk_addr))
                }
            })
            .collect();
        
        let chunks = futures::future::try_join_all(chunk_futures).await?;
        
        // Reassemble requested range
        let mut result = BytesMut::new();
        for (i, &chunk_idx) in chunk_indices.iter().enumerate() {
            let (chunk_start, chunk_end) = manifest.chunk_range(chunk_idx);
            let chunk = &chunks[i];
            
            // Calculate slice within chunk
            let slice_start = if chunk_start < offset {
                (offset - chunk_start) as usize
            } else {
                0
            };
            
            let slice_end = if chunk_end > offset + length {
                (offset + length - chunk_start) as usize
            } else {
                chunk.len()
            };
            
            result.extend_from_slice(&chunk[slice_start..slice_end]);
        }
        
        Ok(result.freeze())
    }
    
    /// Get entire blob (convenience method)
    pub async fn get(&self, addr: CAddr) -> Result<Bytes, StorageError> {
        let bytes = self.dag.get(addr).await
            .ok_or(StorageError::NotFound(addr))?;
        
        // Check if manifest
        if let Ok(manifest) = bincode::deserialize::<ChunkManifest>(&bytes) {
            return self.get_range(addr, 0, manifest.total_size).await;
        }
        
        Ok(bytes)
    }
}
```

### 2.4 Streaming Reader

```rust
// crates/deriva-storage/src/chunk_reader.rs

use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Streaming reader for chunked blobs
pub struct ChunkReader {
    store: Arc<ChunkStore>,
    addr: CAddr,
    manifest: ChunkManifest,
    position: u64,
    buffer: BytesMut,
    buffer_offset: u64,
}

impl ChunkReader {
    pub async fn new(store: Arc<ChunkStore>, addr: CAddr) -> Result<Self, StorageError> {
        let bytes = store.dag.get(addr).await
            .ok_or(StorageError::NotFound(addr))?;
        
        let manifest: ChunkManifest = bincode::deserialize(&bytes)
            .map_err(|_| StorageError::NotChunked)?;
        
        Ok(Self {
            store,
            addr,
            manifest,
            position: 0,
            buffer: BytesMut::new(),
            buffer_offset: 0,
        })
    }
    
    async fn fill_buffer(&mut self) -> Result<(), StorageError> {
        // Determine chunk to fetch
        let chunk_idx = self.manifest.chunk_index(self.position);
        
        if chunk_idx >= self.manifest.chunks.len() {
            return Ok(());  // EOF
        }
        
        // Fetch chunk
        let chunk_addr = self.manifest.chunks[chunk_idx];
        let chunk = self.store.dag.get(chunk_addr).await
            .ok_or(StorageError::NotFound(chunk_addr))?;
        
        // Update buffer
        self.buffer = BytesMut::from(&chunk[..]);
        self.buffer_offset = chunk_idx as u64 * self.manifest.chunk_size;
        
        Ok(())
    }
}

impl AsyncRead for ChunkReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // Check if we need to fetch new chunk
        if self.position < self.buffer_offset 
            || self.position >= self.buffer_offset + self.buffer.len() as u64 
        {
            // Need to fetch chunk
            let fut = self.fill_buffer();
            tokio::pin!(fut);
            
            match fut.poll(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e,
                    )));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        
        // Read from buffer
        let buffer_pos = (self.position - self.buffer_offset) as usize;
        let available = self.buffer.len() - buffer_pos;
        let to_read = available.min(buf.remaining());
        
        if to_read == 0 {
            return Poll::Ready(Ok(()));  // EOF
        }
        
        buf.put_slice(&self.buffer[buffer_pos..buffer_pos + to_read]);
        self.position += to_read as u64;
        
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for ChunkReader {
    fn start_seek(mut self: Pin<&mut Self>, position: std::io::SeekFrom) -> std::io::Result<()> {
        let new_pos = match position {
            std::io::SeekFrom::Start(pos) => pos,
            std::io::SeekFrom::End(offset) => {
                (self.manifest.total_size as i64 + offset) as u64
            }
            std::io::SeekFrom::Current(offset) => {
                (self.position as i64 + offset) as u64
            }
        };
        
        if new_pos > self.manifest.total_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek beyond end",
            ));
        }
        
        self.position = new_pos;
        Ok(())
    }
    
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        Poll::Ready(Ok(self.position))
    }
}
```

---

## 3. Implementation

### 3.1 DagStore Integration

```rust
// crates/deriva-storage/src/dag_store.rs

impl DagStore {
    /// Put with automatic chunking
    pub async fn put_auto(&self, bytes: Bytes) -> Result<CAddr, StorageError> {
        let chunk_store = ChunkStore::new(Arc::new(self.clone()));
        chunk_store.put_chunked(bytes).await
    }
    
    /// Get with range support
    pub async fn get_range(
        &self,
        addr: CAddr,
        offset: u64,
        length: u64,
    ) -> Result<Bytes, StorageError> {
        let chunk_store = ChunkStore::new(Arc::new(self.clone()));
        chunk_store.get_range(addr, offset, length).await
    }
    
    /// Create streaming reader
    pub async fn reader(&self, addr: CAddr) -> Result<ChunkReader, StorageError> {
        let chunk_store = Arc::new(ChunkStore::new(Arc::new(self.clone())));
        ChunkReader::new(chunk_store, addr).await
    }
}
```

### 3.2 FUSE Integration

```rust
// crates/deriva-fuse/src/fs.rs

impl Filesystem for DerivaFS {
    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        let cache = self.inode_cache.lock().unwrap();
        let addr = match cache.ino_to_addr.get(&ino) {
            Some(addr) => *addr,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };
        drop(cache);
        
        let dag = self.dag.clone();
        
        tokio::spawn(async move {
            // Use range read for efficiency
            match dag.get_range(addr, offset as u64, size as u64).await {
                Ok(bytes) => reply.data(&bytes),
                Err(_) => reply.error(libc::EIO),
            }
        });
    }
}
```

### 3.3 REST API Integration

```rust
// crates/deriva-server/src/rest.rs

#[derive(Deserialize)]
struct GetRangeRequest {
    addr: String,
    offset: u64,
    length: u64,
}

async fn get_range(
    State(state): State<AppState>,
    Json(req): Json<GetRangeRequest>,
) -> Result<Bytes, StatusCode> {
    let addr = CAddr::from_hex(&req.addr)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    state.dag_store.get_range(addr, req.offset, req.length).await
        .map_err(|_| StatusCode::NOT_FOUND)
}

// HTTP Range header support
async fn get_with_range(
    State(state): State<AppState>,
    Path(addr_hex): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let addr = CAddr::from_hex(&addr_hex)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    // Parse Range header
    let range = headers.get("Range")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| parse_range_header(s));
    
    match range {
        Some((offset, length)) => {
            let bytes = state.dag_store.get_range(addr, offset, length).await
                .map_err(|_| StatusCode::NOT_FOUND)?;
            
            Ok(Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header("Content-Range", format!("bytes {}-{}/{}", offset, offset + length - 1, length))
                .body(Body::from(bytes))
                .unwrap())
        }
        None => {
            let bytes = state.dag_store.get(addr).await
                .ok_or(StatusCode::NOT_FOUND)?;
            
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(bytes))
                .unwrap())
        }
    }
}

fn parse_range_header(range: &str) -> Option<(u64, u64)> {
    // Parse "bytes=start-end"
    let range = range.strip_prefix("bytes=")?;
    let parts: Vec<_> = range.split('-').collect();
    
    if parts.len() != 2 {
        return None;
    }
    
    let start: u64 = parts[0].parse().ok()?;
    let end: u64 = parts[1].parse().ok()?;
    
    Some((start, end - start + 1))
}
```

---

## 4. Data Flow Diagrams

### 4.1 Chunked Put Flow

```
┌──────┐                                                      ┌────────────┐
│Client│                                                      │ ChunkStore │
└──┬───┘                                                      └─────┬──────┘
   │                                                                │
   │ put_chunked(10 GB blob)                                       │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                          Size > 10 MB?         │
   │                                          ─────────────         │
   │                                                                │
   │                                          ✓ Yes, chunk it       │
   │                                                                │
   │                                          Split into 1 MB chunks│
   │                                          ─────────────         │
   │                                                                │
   │                                          Store chunk[0]        │
   │                                          Store chunk[1]        │
   │                                          ...                   │
   │                                          Store chunk[10239]    │
   │                                                                │
   │                                          Create manifest       │
   │                                          ─────────────         │
   │                                                                │
   │                                          Store manifest        │
   │                                          ─────────────         │
   │                                                                │
   │ manifest_addr                                                 │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

### 4.2 Range Read Flow

```
┌──────┐                                                      ┌────────────┐
│Client│                                                      │ ChunkStore │
└──┬───┘                                                      └─────┬──────┘
   │                                                                │
   │ get_range(addr, offset=5MB, length=2MB)                       │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                          Load manifest         │
   │                                          ─────────────         │
   │                                                                │
   │                                          Calculate chunks:     │
   │                                          • chunk[5] (5-6 MB)   │
   │                                          • chunk[6] (6-7 MB)   │
   │                                                                │
   │                                          Parallel fetch:       │
   │                                          ┌─> get(chunk[5])     │
   │                                          └─> get(chunk[6])     │
   │                                                                │
   │                                          Reassemble range      │
   │                                          ─────────────         │
   │                                                                │
   │ 2 MB bytes                                                    │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

### 4.3 Streaming Read Flow

```
┌──────┐                                                      ┌─────────────┐
│Client│                                                      │ ChunkReader │
└──┬───┘                                                      └──────┬──────┘
   │                                                                 │
   │ reader.read(buf)                                               │
   │────────────────────────────────────────────────────────────────>
   │                                                                 │
   │                                          Buffer empty?          │
   │                                          ─────────────          │
   │                                                                 │
   │                                          ✓ Yes, fetch chunk[0] │
   │                                          ─────────────          │
   │                                                                 │
   │                                          Fill buffer            │
   │                                          ─────────────          │
   │                                                                 │
   │                                          Copy to user buffer    │
   │                                          ─────────────          │
   │                                                                 │
   │ bytes_read                                                     │
   │<────────────────────────────────────────────────────────────────
   │                                                                 │
   │ reader.read(buf)  [again]                                      │
   │────────────────────────────────────────────────────────────────>
   │                                                                 │
   │                                          Buffer has data?       │
   │                                          ─────────────          │
   │                                                                 │
   │                                          ✓ Yes, use buffer      │
   │                                          ─────────────          │
   │                                                                 │
   │ bytes_read                                                     │
   │<────────────────────────────────────────────────────────────────
   │                                                                 │
```

---

## 5. Test Specification

### 5.1 Unit Tests

```rust
// crates/deriva-storage/tests/chunk_store.rs

#[tokio::test]
async fn test_chunk_manifest_creation() {
    let blob = vec![0u8; 5 * 1024 * 1024];  // 5 MB
    let (manifest, chunks) = ChunkManifest::from_blob(&blob, 1024 * 1024);
    
    assert_eq!(manifest.total_size, 5 * 1024 * 1024);
    assert_eq!(manifest.chunk_size, 1024 * 1024);
    assert_eq!(manifest.chunks.len(), 5);
    assert_eq!(chunks.len(), 5);
}

#[tokio::test]
async fn test_chunk_index_calculation() {
    let manifest = ChunkManifest {
        total_size: 10 * 1024 * 1024,
        chunk_size: 1024 * 1024,
        chunks: vec![CAddr::default(); 10],
        content_type: None,
    };
    
    assert_eq!(manifest.chunk_index(0), 0);
    assert_eq!(manifest.chunk_index(1024 * 1024), 1);
    assert_eq!(manifest.chunk_index(5 * 1024 * 1024), 5);
    assert_eq!(manifest.chunk_index(9 * 1024 * 1024 + 500 * 1024), 9);
}

#[tokio::test]
async fn test_chunks_for_range() {
    let manifest = ChunkManifest {
        total_size: 10 * 1024 * 1024,
        chunk_size: 1024 * 1024,
        chunks: vec![CAddr::default(); 10],
        content_type: None,
    };
    
    // Single chunk
    let chunks = manifest.chunks_for_range(0, 1024);
    assert_eq!(chunks, vec![0]);
    
    // Multiple chunks
    let chunks = manifest.chunks_for_range(512 * 1024, 2 * 1024 * 1024);
    assert_eq!(chunks, vec![0, 1, 2]);
    
    // Across boundary
    let chunks = manifest.chunks_for_range(1024 * 1024 - 512, 1024);
    assert_eq!(chunks, vec![0, 1]);
}

#[tokio::test]
async fn test_put_small_blob() {
    let dag = DagStore::new_memory();
    let store = ChunkStore::new(Arc::new(dag.clone()));
    
    let blob = Bytes::from(vec![1u8; 1024]);  // 1 KB (below threshold)
    let addr = store.put_chunked(blob.clone()).await.unwrap();
    
    // Should be stored directly (not chunked)
    let retrieved = dag.get(addr).await.unwrap();
    assert_eq!(retrieved, blob);
}

#[tokio::test]
async fn test_put_large_blob() {
    let dag = DagStore::new_memory();
    let store = ChunkStore::new(Arc::new(dag.clone()));
    
    let blob = Bytes::from(vec![2u8; 20 * 1024 * 1024]);  // 20 MB
    let addr = store.put_chunked(blob.clone()).await.unwrap();
    
    // Should be stored as manifest
    let manifest_bytes = dag.get(addr).await.unwrap();
    let manifest: ChunkManifest = bincode::deserialize(&manifest_bytes).unwrap();
    
    assert_eq!(manifest.total_size, 20 * 1024 * 1024);
    assert_eq!(manifest.chunks.len(), 20);
}

#[tokio::test]
async fn test_get_range() {
    let dag = DagStore::new_memory();
    let store = ChunkStore::new(Arc::new(dag.clone()));
    
    // Create test blob with pattern
    let mut blob = Vec::new();
    for i in 0..20 {
        blob.extend(vec![i as u8; 1024 * 1024]);
    }
    let blob = Bytes::from(blob);
    
    let addr = store.put_chunked(blob.clone()).await.unwrap();
    
    // Read range from middle
    let range = store.get_range(addr, 5 * 1024 * 1024, 2 * 1024 * 1024).await.unwrap();
    
    assert_eq!(range.len(), 2 * 1024 * 1024);
    assert_eq!(range[0], 5);  // First byte of chunk 5
    assert_eq!(range[1024 * 1024], 6);  // First byte of chunk 6
}

#[tokio::test]
async fn test_get_range_single_chunk() {
    let dag = DagStore::new_memory();
    let store = ChunkStore::new(Arc::new(dag.clone()));
    
    let blob = Bytes::from(vec![42u8; 20 * 1024 * 1024]);
    let addr = store.put_chunked(blob.clone()).await.unwrap();
    
    // Read small range within single chunk
    let range = store.get_range(addr, 1024, 4096).await.unwrap();
    
    assert_eq!(range.len(), 4096);
    assert_eq!(range[0], 42);
}
```

### 5.2 Streaming Tests

```rust
// crates/deriva-storage/tests/chunk_reader.rs

#[tokio::test]
async fn test_streaming_read() {
    let dag = DagStore::new_memory();
    let store = Arc::new(ChunkStore::new(Arc::new(dag.clone())));
    
    let blob = Bytes::from(vec![1u8; 10 * 1024 * 1024]);
    let addr = store.put_chunked(blob.clone()).await.unwrap();
    
    let mut reader = ChunkReader::new(store, addr).await.unwrap();
    
    // Read in 4KB chunks
    let mut total_read = 0;
    let mut buffer = vec![0u8; 4096];
    
    loop {
        let n = reader.read(&mut buffer).await.unwrap();
        if n == 0 {
            break;
        }
        total_read += n;
    }
    
    assert_eq!(total_read, 10 * 1024 * 1024);
}

#[tokio::test]
async fn test_streaming_seek() {
    let dag = DagStore::new_memory();
    let store = Arc::new(ChunkStore::new(Arc::new(dag.clone())));
    
    let blob = Bytes::from(vec![2u8; 10 * 1024 * 1024]);
    let addr = store.put_chunked(blob.clone()).await.unwrap();
    
    let mut reader = ChunkReader::new(store, addr).await.unwrap();
    
    // Seek to middle
    reader.seek(std::io::SeekFrom::Start(5 * 1024 * 1024)).await.unwrap();
    
    let mut buffer = vec![0u8; 1024];
    let n = reader.read(&mut buffer).await.unwrap();
    
    assert_eq!(n, 1024);
    assert_eq!(buffer[0], 2);
}

#[tokio::test]
async fn test_streaming_seek_end() {
    let dag = DagStore::new_memory();
    let store = Arc::new(ChunkStore::new(Arc::new(dag.clone())));
    
    let blob = Bytes::from(vec![3u8; 10 * 1024 * 1024]);
    let addr = store.put_chunked(blob.clone()).await.unwrap();
    
    let mut reader = ChunkReader::new(store, addr).await.unwrap();
    
    // Seek to end
    reader.seek(std::io::SeekFrom::End(-1024)).await.unwrap();
    
    let mut buffer = vec![0u8; 1024];
    let n = reader.read(&mut buffer).await.unwrap();
    
    assert_eq!(n, 1024);
}
```

### 5.3 Integration Tests

```rust
// crates/deriva-storage/tests/chunk_integration.rs

#[tokio::test]
async fn test_fuse_range_read() {
    let dag = DagStore::new_memory();
    let store = ChunkStore::new(Arc::new(dag.clone()));
    
    // Create large file
    let blob = Bytes::from(vec![42u8; 100 * 1024 * 1024]);  // 100 MB
    let addr = store.put_chunked(blob.clone()).await.unwrap();
    
    // Simulate FUSE read at offset
    let range = dag.get_range(addr, 50 * 1024 * 1024, 1024).await.unwrap();
    
    assert_eq!(range.len(), 1024);
    assert_eq!(range[0], 42);
}

#[tokio::test]
async fn test_http_range_request() {
    let dag = DagStore::new_memory();
    let store = ChunkStore::new(Arc::new(dag.clone()));
    
    let blob = Bytes::from(vec![99u8; 50 * 1024 * 1024]);
    let addr = store.put_chunked(blob.clone()).await.unwrap();
    
    // Simulate HTTP Range: bytes=1000000-2000000
    let range = dag.get_range(addr, 1000000, 1000001).await.unwrap();
    
    assert_eq!(range.len(), 1000001);
    assert_eq!(range[0], 99);
}
```

---

## 6. Edge Cases & Error Handling

### 6.1 Range Beyond EOF

```rust
#[tokio::test]
async fn test_range_beyond_eof() {
    let dag = DagStore::new_memory();
    let store = ChunkStore::new(Arc::new(dag.clone()));
    
    let blob = Bytes::from(vec![1u8; 10 * 1024 * 1024]);
    let addr = store.put_chunked(blob).await.unwrap();
    
    // Request range beyond EOF
    let range = store.get_range(addr, 15 * 1024 * 1024, 1024).await.unwrap();
    
    assert_eq!(range.len(), 0);  // Empty
}
```

### 6.2 Missing Chunk

```rust
#[tokio::test]
async fn test_missing_chunk() {
    let dag = DagStore::new_memory();
    let store = ChunkStore::new(Arc::new(dag.clone()));
    
    let blob = Bytes::from(vec![1u8; 20 * 1024 * 1024]);
    let addr = store.put_chunked(blob).await.unwrap();
    
    // Delete a chunk
    let manifest_bytes = dag.get(addr).await.unwrap();
    let manifest: ChunkManifest = bincode::deserialize(&manifest_bytes).unwrap();
    dag.delete(manifest.chunks[5]).await.unwrap();
    
    // Try to read range that includes missing chunk
    let result = store.get_range(addr, 5 * 1024 * 1024, 1024).await;
    
    assert!(matches!(result, Err(StorageError::NotFound(_))));
}
```

### 6.3 Corrupted Manifest

```rust
#[tokio::test]
async fn test_corrupted_manifest() {
    let dag = DagStore::new_memory();
    let store = ChunkStore::new(Arc::new(dag.clone()));
    
    // Put corrupted manifest
    let addr = CAddr::from_hex("abc123...").unwrap();
    dag.put(addr, Bytes::from("corrupted")).await.unwrap();
    
    let result = store.get_range(addr, 0, 1024).await;
    
    // Should fall back to treating as regular blob
    assert!(result.is_ok());
}
```

---

## 7. Performance Analysis

### 7.1 Chunking Overhead

**Small blob (1 MB):**
- Direct storage: ~1 ms
- Chunked storage: ~1 ms (no chunking)
- Overhead: 0%

**Large blob (1 GB):**
- Direct storage: ~1000 ms
- Chunked storage: ~1050 ms (1000 chunks × 1 ms + 50 ms manifest)
- Overhead: 5%

### 7.2 Range Read Performance

**Sequential read (1 GB):**
- Direct: ~10 seconds (100 MB/s)
- Chunked: ~10.5 seconds (95 MB/s)
- Overhead: 5%

**Random read (1 MB from 1 GB):**
- Direct: ~10 seconds (must download entire blob)
- Chunked: ~10 ms (fetch 1 chunk)
- Speedup: 1000x

### 7.3 Memory Usage

**Direct storage (1 GB blob):**
- Memory: 1 GB (entire blob in memory)

**Chunked storage (1 GB blob):**
- Memory: ~1 MB (single chunk buffer)
- Reduction: 1000x

---

## 8. Files Changed

### New Files
- `crates/deriva-storage/src/chunk_manifest.rs` — Chunk manifest type
- `crates/deriva-storage/src/chunk_store.rs` — Chunk-aware storage
- `crates/deriva-storage/src/chunk_reader.rs` — Streaming reader
- `crates/deriva-storage/tests/chunk_store.rs` — Unit tests
- `crates/deriva-storage/tests/chunk_reader.rs` — Streaming tests
- `crates/deriva-storage/tests/chunk_integration.rs` — Integration tests

### Modified Files
- `crates/deriva-storage/src/dag_store.rs` — Add `put_auto()`, `get_range()`, `reader()`
- `crates/deriva-storage/src/lib.rs` — Export chunk types
- `crates/deriva-fuse/src/fs.rs` — Use `get_range()` for reads
- `crates/deriva-server/src/rest.rs` — Add HTTP Range support

---

## 9. Dependency Changes

```toml
# crates/deriva-storage/Cargo.toml
[dependencies]
tokio = { version = "1.35", features = ["io-util"] }
futures = "0.3"
bytes = "1.5"
bincode = "1.3"
serde = { version = "1.0", features = ["derive"] }
```

No new dependencies — uses existing tokio for async I/O.

---

## 10. Design Rationale

### 10.1 Why 1 MB Chunk Size?

**Alternatives:**
- 64 KB: Too many chunks, high manifest overhead
- 10 MB: Too large, poor random access

**Decision:** 1 MB balances manifest size and random access performance.

### 10.2 Why 10 MB Chunking Threshold?

**Alternative:** Chunk all blobs.

**Problem:** Small blobs have unnecessary manifest overhead.

**Decision:** Only chunk blobs >10 MB to avoid overhead.

### 10.3 Why Parallel Chunk Fetching?

**Alternative:** Sequential fetching.

**Problem:** Slow for multi-chunk ranges.

**Decision:** Parallel fetching achieves near-linear speedup.

### 10.4 Why Streaming Reader?

**Alternative:** Load entire blob into memory.

**Problem:** Large blobs exhaust memory.

**Decision:** Streaming reader enables constant memory usage.

---

## 11. Observability Integration

### 11.1 Metrics

```rust
lazy_static! {
    static ref CHUNK_OPERATIONS: IntCounterVec = register_int_counter_vec!(
        "deriva_chunk_operations_total",
        "Chunk operations by type and result",
        &["operation", "result"]
    ).unwrap();

    static ref CHUNK_SIZE: Histogram = register_histogram!(
        "deriva_chunk_size_bytes",
        "Chunk size distribution"
    ).unwrap();
    
    static ref CHUNKS_PER_BLOB: Histogram = register_histogram!(
        "deriva_chunks_per_blob",
        "Number of chunks per blob"
    ).unwrap();
    
    static ref RANGE_READ_SIZE: Histogram = register_histogram!(
        "deriva_range_read_size_bytes",
        "Range read size distribution"
    ).unwrap();
}
```

### 11.2 Logs

```rust
impl ChunkStore {
    pub async fn put_chunked(&self, blob: Bytes) -> Result<CAddr, StorageError> {
        let start = Instant::now();
        
        if blob.len() < ChunkManifest::CHUNKING_THRESHOLD as usize {
            debug!("Storing blob directly (size: {})", blob.len());
            let addr = CAddr::from_bytes(&blob);
            self.dag.put(addr, blob).await?;
            return Ok(addr);
        }
        
        let (manifest, chunks) = ChunkManifest::from_blob(&blob, self.chunk_size);
        
        info!(
            "Chunking blob: size={}, chunks={}",
            manifest.total_size,
            manifest.chunks.len()
        );
        
        CHUNKS_PER_BLOB.observe(manifest.chunks.len() as f64);
        
        // ... store chunks ...
        
        let duration = start.elapsed();
        CHUNK_OPERATIONS.with_label_values(&["put", "success"]).inc();
        
        info!("Chunked blob stored in {:?}", duration);
        
        Ok(manifest_addr)
    }
}
```

### 11.3 Tracing

```rust
use tracing::{info_span, instrument};

#[instrument(skip(self))]
pub async fn get_range(
    &self,
    addr: CAddr,
    offset: u64,
    length: u64,
) -> Result<Bytes, StorageError> {
    let span = info_span!(
        "chunk_get_range",
        addr = %addr.to_hex(),
        offset = offset,
        length = length
    );
    let _enter = span.enter();
    
    RANGE_READ_SIZE.observe(length as f64);
    
    // ... range read logic ...
}
```

---

## 12. Checklist

### Implementation
- [ ] Create `deriva-storage/src/chunk_manifest.rs` with manifest type
- [ ] Implement `from_blob()` for chunking
- [ ] Implement `chunk_index()` and `chunks_for_range()`
- [ ] Create `deriva-storage/src/chunk_store.rs` with storage layer
- [ ] Implement `put_chunked()` with automatic chunking
- [ ] Implement `get_range()` with parallel chunk fetching
- [ ] Create `deriva-storage/src/chunk_reader.rs` with streaming reader
- [ ] Implement `AsyncRead` and `AsyncSeek` traits
- [ ] Add `put_auto()`, `get_range()`, `reader()` to `DagStore`
- [ ] Integrate with FUSE (use `get_range()` for reads)
- [ ] Add HTTP Range header support to REST API

### Testing
- [ ] Unit test: chunk manifest creation
- [ ] Unit test: chunk index calculation
- [ ] Unit test: chunks for range
- [ ] Unit test: put small blob (no chunking)
- [ ] Unit test: put large blob (chunked)
- [ ] Unit test: get range (single chunk)
- [ ] Unit test: get range (multiple chunks)
- [ ] Unit test: streaming read
- [ ] Unit test: streaming seek
- [ ] Unit test: range beyond EOF
- [ ] Unit test: missing chunk
- [ ] Unit test: corrupted manifest
- [ ] Integration test: FUSE range read
- [ ] Integration test: HTTP range request
- [ ] Benchmark: chunking overhead (<5%)
- [ ] Benchmark: range read performance (>100 MB/s)
- [ ] Benchmark: random access speedup (>100x)

### Documentation
- [ ] Document chunk manifest format
- [ ] Add examples of range reads
- [ ] Document streaming reader API
- [ ] Add troubleshooting guide for chunk issues
- [ ] Document performance characteristics
- [ ] Add HTTP Range header examples

### Observability
- [ ] Add `deriva_chunk_operations_total` counter
- [ ] Add `deriva_chunk_size_bytes` histogram
- [ ] Add `deriva_chunks_per_blob` histogram
- [ ] Add `deriva_range_read_size_bytes` histogram
- [ ] Add debug logs for chunking operations
- [ ] Add tracing spans for range reads

### Validation
- [ ] Test with 1 GB blobs
- [ ] Verify chunking overhead <5%
- [ ] Test range reads across chunk boundaries
- [ ] Verify streaming reader memory usage (constant)
- [ ] Test HTTP Range requests
- [ ] Verify parallel chunk fetching speedup
- [ ] Test with missing/corrupted chunks

### Deployment
- [ ] Deploy with chunking enabled (10 MB threshold)
- [ ] Monitor chunk operation metrics
- [ ] Set default chunk size (1 MB)
- [ ] Document chunking configuration
- [ ] Add admin API to query chunk statistics

---

**Estimated effort:** 5–7 days
- Days 1-2: Core chunk manifest + chunk store
- Day 3: Range read implementation + parallel fetching
- Day 4: Streaming reader (AsyncRead + AsyncSeek)
- Day 5: Integration (FUSE + REST API)
- Days 6-7: Tests + benchmarks + documentation

**Success criteria:**
1. All tests pass (chunking, range reads, streaming)
2. Chunking overhead <5% for large blobs
3. Range read performance >100 MB/s
4. Random access speedup >100x vs full download
5. Streaming reader uses constant memory
6. HTTP Range requests work correctly
7. FUSE reads use range reads efficiently
