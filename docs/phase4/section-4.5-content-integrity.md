# Phase 4, Section 4.5: Content Integrity & Merkle DAG Verification

**Status:** Blueprint  
**Depends on:** §4.1 (Canonical Serialization), §4.2 (Deterministic Scheduling), §4.3 (Reproducibility Proofs), §4.4 (Deterministic Floats)  
**Blocks:** §4.6 (WASM), §4.7 (FUSE), §4.8 (Chunks), §4.9 (Mutable Refs), §4.10 (REST)

---

## 1. Problem Statement

### 1.1 Current State

Deriva's content addressing provides **implicit integrity** — if you have a CAddr, you can verify the content matches. But:

1. **No verification on read**: `DagStore::get(addr)` returns bytes without checking integrity
2. **No Merkle proofs**: Can't prove a value is part of a DAG without downloading the entire DAG
3. **No incremental verification**: Must download and hash entire blobs to verify
4. **No tamper detection**: Silent corruption if storage backend is compromised
5. **No audit trail**: Can't prove when/how content was verified

### 1.2 The Problem

**Scenario 1: Corrupted Storage**
```rust
// Attacker modifies blob in storage backend
let addr = CAddr::from_hex("abc123...");
let bytes = dag_store.get(addr).await?;  // ❌ Returns corrupted data, no verification
```

**Scenario 2: Partial Download**
```rust
// User wants to verify a 10GB blob without downloading it all
let addr = CAddr::from_hex("def456...");
let bytes = dag_store.get(addr).await?;  // ❌ Must download entire 10GB to verify
```

**Scenario 3: Merkle Proof**
```rust
// User wants to prove a value is part of a DAG
let root = CAddr::from_hex("789abc...");
let leaf = CAddr::from_hex("def012...");
// ❌ No way to generate/verify Merkle proof
```

### 1.3 Requirements

1. **Verified reads**: Every `get()` must verify content matches CAddr
2. **Merkle proofs**: Generate/verify proofs that a value is part of a DAG
3. **Incremental verification**: Verify large blobs without full download (chunk-level)
4. **Tamper detection**: Detect and report corrupted content
5. **Audit trail**: Log all verification attempts (success/failure)
6. **Performance**: Verification overhead <5% for small values, <1% for large blobs

---

## 2. Design

### 2.1 Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Content Integrity Layer                  │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ VerifiedGet  │  │ MerkleProof  │  │ ChunkVerify  │      │
│  │              │  │              │  │              │      │
│  │ • Hash check │  │ • Generate   │  │ • Streaming  │      │
│  │ • Retry      │  │ • Verify     │  │ • Parallel   │      │
│  │ • Audit log  │  │ • Serialize  │  │ • Resume     │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                               │
├─────────────────────────────────────────────────────────────┤
│                      DagStore (§2.3)                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ RecipeStore  │  │  BlobStore   │  │  ChunkStore  │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Core Types

```rust
// crates/deriva-storage/src/verified_get.rs

/// Verified read result
#[derive(Debug, Clone)]
pub struct VerifiedBytes {
    /// The actual content
    pub bytes: Bytes,
    /// The CAddr that was verified
    pub addr: CAddr,
    /// Verification timestamp
    pub verified_at: SystemTime,
}

/// Verification error
#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: CAddr, actual: CAddr },
    
    #[error("Content not found: {0}")]
    NotFound(CAddr),
    
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    
    #[error("Merkle proof verification failed")]
    InvalidProof,
}

/// Verified get with retry and audit logging
pub struct VerifiedGet {
    store: Arc<DagStore>,
    audit_log: Arc<AuditLog>,
    max_retries: usize,
}

impl VerifiedGet {
    pub async fn get(&self, addr: CAddr) -> Result<VerifiedBytes, VerificationError> {
        for attempt in 0..self.max_retries {
            match self.try_get(addr).await {
                Ok(verified) => {
                    self.audit_log.log_success(addr, verified.verified_at);
                    return Ok(verified);
                }
                Err(e) if attempt < self.max_retries - 1 => {
                    warn!("Verification failed (attempt {}): {}", attempt + 1, e);
                    continue;
                }
                Err(e) => {
                    self.audit_log.log_failure(addr, &e);
                    return Err(e);
                }
            }
        }
        unreachable!()
    }
    
    async fn try_get(&self, addr: CAddr) -> Result<VerifiedBytes, VerificationError> {
        // Get bytes from storage
        let bytes = self.store.get(addr).await
            .ok_or(VerificationError::NotFound(addr))?;
        
        // Compute hash
        let actual = CAddr::from_bytes(&bytes);
        
        // Verify
        if actual != addr {
            return Err(VerificationError::HashMismatch {
                expected: addr,
                actual,
            });
        }
        
        Ok(VerifiedBytes {
            bytes,
            addr,
            verified_at: SystemTime::now(),
        })
    }
}
```

### 2.3 Merkle Proof

```rust
// crates/deriva-storage/src/merkle_proof.rs

/// Merkle proof that a value is part of a DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// Root CAddr
    pub root: CAddr,
    /// Leaf CAddr
    pub leaf: CAddr,
    /// Path from root to leaf (list of sibling hashes)
    pub path: Vec<ProofNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofNode {
    /// Sibling hash
    pub sibling: CAddr,
    /// Direction: true = left, false = right
    pub is_left: bool,
}

impl MerkleProof {
    /// Generate proof that `leaf` is part of DAG rooted at `root`
    pub async fn generate(
        store: &DagStore,
        root: CAddr,
        leaf: CAddr,
    ) -> Result<Self, VerificationError> {
        let mut path = Vec::new();
        let mut current = root;
        
        // Traverse DAG from root to leaf
        while current != leaf {
            let bytes = store.get(current).await
                .ok_or(VerificationError::NotFound(current))?;
            
            // Deserialize as Recipe
            let recipe: Recipe = bincode::deserialize(&bytes)
                .map_err(|_| VerificationError::NotFound(current))?;
            
            // Find which input contains the leaf
            let mut found = false;
            for (i, input_addr) in recipe.inputs.iter().enumerate() {
                if Self::contains(store, *input_addr, leaf).await? {
                    // Add siblings to path
                    for (j, sibling_addr) in recipe.inputs.iter().enumerate() {
                        if i != j {
                            path.push(ProofNode {
                                sibling: *sibling_addr,
                                is_left: j < i,
                            });
                        }
                    }
                    current = *input_addr;
                    found = true;
                    break;
                }
            }
            
            if !found {
                return Err(VerificationError::InvalidProof);
            }
        }
        
        Ok(MerkleProof { root, leaf, path })
    }
    
    /// Verify proof
    pub fn verify(&self) -> bool {
        let mut current = self.leaf;
        
        for node in &self.path {
            // Combine current with sibling
            let combined = if node.is_left {
                CAddr::combine(node.sibling, current)
            } else {
                CAddr::combine(current, node.sibling)
            };
            current = combined;
        }
        
        current == self.root
    }
    
    /// Check if `needle` is contained in DAG rooted at `haystack`
    async fn contains(
        store: &DagStore,
        haystack: CAddr,
        needle: CAddr,
    ) -> Result<bool, VerificationError> {
        if haystack == needle {
            return Ok(true);
        }
        
        let bytes = store.get(haystack).await
            .ok_or(VerificationError::NotFound(haystack))?;
        
        let recipe: Recipe = match bincode::deserialize(&bytes) {
            Ok(r) => r,
            Err(_) => return Ok(false),  // Leaf value, not a recipe
        };
        
        for input_addr in recipe.inputs {
            if Self::contains(store, input_addr, needle).await? {
                return Ok(true);
            }
        }
        
        Ok(false)
    }
}

impl CAddr {
    /// Combine two CAddrs (for Merkle tree)
    pub fn combine(left: CAddr, right: CAddr) -> CAddr {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&left.0);
        hasher.update(&right.0);
        CAddr(hasher.finalize().into())
    }
}
```

### 2.4 Chunk-Level Verification

```rust
// crates/deriva-storage/src/chunk_verify.rs

/// Chunk-level verification for large blobs
pub struct ChunkVerifier {
    store: Arc<DagStore>,
    chunk_size: usize,
}

impl ChunkVerifier {
    /// Verify large blob incrementally
    pub async fn verify_chunked(
        &self,
        addr: CAddr,
    ) -> Result<VerifiedBytes, VerificationError> {
        // Get chunk manifest
        let manifest_bytes = self.store.get(addr).await
            .ok_or(VerificationError::NotFound(addr))?;
        
        let manifest: ChunkManifest = bincode::deserialize(&manifest_bytes)
            .map_err(|_| VerificationError::NotFound(addr))?;
        
        // Verify each chunk in parallel
        let chunk_futures: Vec<_> = manifest.chunks.iter()
            .map(|chunk_addr| self.verify_chunk(*chunk_addr))
            .collect();
        
        let chunks = futures::future::try_join_all(chunk_futures).await?;
        
        // Reassemble
        let mut bytes = BytesMut::with_capacity(manifest.total_size);
        for chunk in chunks {
            bytes.extend_from_slice(&chunk.bytes);
        }
        
        // Verify total hash
        let actual = CAddr::from_bytes(&bytes);
        if actual != addr {
            return Err(VerificationError::HashMismatch {
                expected: addr,
                actual,
            });
        }
        
        Ok(VerifiedBytes {
            bytes: bytes.freeze(),
            addr,
            verified_at: SystemTime::now(),
        })
    }
    
    async fn verify_chunk(&self, addr: CAddr) -> Result<VerifiedBytes, VerificationError> {
        let bytes = self.store.get(addr).await
            .ok_or(VerificationError::NotFound(addr))?;
        
        let actual = CAddr::from_bytes(&bytes);
        if actual != addr {
            return Err(VerificationError::HashMismatch {
                expected: addr,
                actual,
            });
        }
        
        Ok(VerifiedBytes {
            bytes,
            addr,
            verified_at: SystemTime::now(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkManifest {
    pub chunks: Vec<CAddr>,
    pub total_size: usize,
}
```

### 2.5 Audit Log

```rust
// crates/deriva-storage/src/audit_log.rs

/// Audit log for verification events
pub struct AuditLog {
    log: Arc<Mutex<Vec<AuditEntry>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub addr: CAddr,
    pub timestamp: SystemTime,
    pub result: AuditResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditResult {
    Success,
    HashMismatch { expected: CAddr, actual: CAddr },
    NotFound,
    StorageError(String),
}

impl AuditLog {
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    pub fn log_success(&self, addr: CAddr, timestamp: SystemTime) {
        let mut log = self.log.lock().unwrap();
        log.push(AuditEntry {
            addr,
            timestamp,
            result: AuditResult::Success,
        });
    }
    
    pub fn log_failure(&self, addr: CAddr, error: &VerificationError) {
        let mut log = self.log.lock().unwrap();
        let result = match error {
            VerificationError::HashMismatch { expected, actual } => {
                AuditResult::HashMismatch {
                    expected: *expected,
                    actual: *actual,
                }
            }
            VerificationError::NotFound(_) => AuditResult::NotFound,
            VerificationError::Storage(e) => AuditResult::StorageError(e.to_string()),
            VerificationError::InvalidProof => AuditResult::StorageError("Invalid proof".into()),
        };
        
        log.push(AuditEntry {
            addr,
            timestamp: SystemTime::now(),
            result,
        });
    }
    
    pub fn get_entries(&self) -> Vec<AuditEntry> {
        self.log.lock().unwrap().clone()
    }
}
```

---

## 3. Implementation

### 3.1 VerifiedGet Integration

```rust
// crates/deriva-storage/src/dag_store.rs

impl DagStore {
    /// Get with verification
    pub async fn get_verified(&self, addr: CAddr) -> Result<VerifiedBytes, VerificationError> {
        let verified_get = VerifiedGet {
            store: Arc::new(self.clone()),
            audit_log: Arc::new(AuditLog::new()),
            max_retries: 3,
        };
        
        verified_get.get(addr).await
    }
    
    /// Get with Merkle proof
    pub async fn get_with_proof(
        &self,
        root: CAddr,
        leaf: CAddr,
    ) -> Result<(VerifiedBytes, MerkleProof), VerificationError> {
        // Generate proof
        let proof = MerkleProof::generate(self, root, leaf).await?;
        
        // Verify proof
        if !proof.verify() {
            return Err(VerificationError::InvalidProof);
        }
        
        // Get verified bytes
        let verified = self.get_verified(leaf).await?;
        
        Ok((verified, proof))
    }
}
```

### 3.2 Executor Integration

```rust
// crates/deriva-compute/src/executor.rs

impl Executor {
    pub async fn materialize(&mut self, addr: CAddr) -> Result<Bytes, ExecutionError> {
        // Check cache
        if let Some(bytes) = self.cache.get(&addr) {
            return Ok(bytes);
        }
        
        // Try verified get
        match self.dag.get_verified(addr).await {
            Ok(verified) => {
                self.cache.insert(addr, verified.bytes.clone());
                return Ok(verified.bytes);
            }
            Err(VerificationError::NotFound(_)) => {
                // Need to compute
            }
            Err(e) => {
                return Err(ExecutionError::Verification(e));
            }
        }
        
        // ... rest of materialization logic ...
    }
}
```

### 3.3 REST API Integration

```rust
// crates/deriva-server/src/rest.rs

#[derive(Deserialize)]
struct GetWithProofRequest {
    root: String,
    leaf: String,
}

#[derive(Serialize)]
struct GetWithProofResponse {
    bytes: String,  // Base64
    proof: MerkleProof,
}

async fn get_with_proof(
    State(state): State<AppState>,
    Json(req): Json<GetWithProofRequest>,
) -> Result<Json<GetWithProofResponse>, StatusCode> {
    let root = CAddr::from_hex(&req.root)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let leaf = CAddr::from_hex(&req.leaf)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    let (verified, proof) = state.dag_store.get_with_proof(root, leaf).await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    
    Ok(Json(GetWithProofResponse {
        bytes: base64::encode(&verified.bytes),
        proof,
    }))
}

#[derive(Deserialize)]
struct VerifyProofRequest {
    proof: MerkleProof,
}

#[derive(Serialize)]
struct VerifyProofResponse {
    valid: bool,
}

async fn verify_proof(
    Json(req): Json<VerifyProofRequest>,
) -> Json<VerifyProofResponse> {
    Json(VerifyProofResponse {
        valid: req.proof.verify(),
    })
}
```

---

## 4. Data Flow Diagrams

### 4.1 Verified Get Flow

```
┌──────┐                                                      ┌──────────┐
│Client│                                                      │ DagStore │
└──┬───┘                                                      └────┬─────┘
   │                                                                │
   │ get_verified(addr)                                            │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                                   get(addr)    │
   │                                                   ────────────>│
   │                                                                │
   │                                                   bytes        │
   │                                                   <────────────│
   │                                                                │
   │                                          hash(bytes)           │
   │                                          ─────────────         │
   │                                                                │
   │                                          actual == addr?       │
   │                                          ─────────────         │
   │                                                                │
   │                                          ✓ Match              │
   │                                                                │
   │                                          log_success()         │
   │                                          ─────────────         │
   │                                                                │
   │ VerifiedBytes { bytes, addr, verified_at }                    │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

### 4.2 Merkle Proof Generation

```
┌──────┐                                                      ┌──────────┐
│Client│                                                      │ DagStore │
└──┬───┘                                                      └────┬─────┘
   │                                                                │
   │ generate_proof(root, leaf)                                    │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                                   get(root)    │
   │                                                   ────────────>│
   │                                                                │
   │                                                   Recipe       │
   │                                                   <────────────│
   │                                                                │
   │                                          contains(input, leaf)?│
   │                                          ─────────────         │
   │                                                                │
   │                                          ✓ Found in input[1]  │
   │                                                                │
   │                                          Add siblings to path  │
   │                                          ─────────────         │
   │                                                                │
   │                                          Recurse on input[1]   │
   │                                          ─────────────         │
   │                                                                │
   │ MerkleProof { root, leaf, path }                              │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

### 4.3 Chunk Verification Flow

```
┌──────┐                                                      ┌──────────┐
│Client│                                                      │ DagStore │
└──┬───┘                                                      └────┬─────┘
   │                                                                │
   │ verify_chunked(addr)                                          │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                                   get(addr)    │
   │                                                   ────────────>│
   │                                                                │
   │                                                   ChunkManifest│
   │                                                   <────────────│
   │                                                                │
   │                                          Parallel verify chunks│
   │                                          ─────────────         │
   │                                                                │
   │                                          ┌─> verify_chunk(c1)  │
   │                                          ├─> verify_chunk(c2)  │
   │                                          └─> verify_chunk(c3)  │
   │                                                                │
   │                                          Reassemble chunks     │
   │                                          ─────────────         │
   │                                                                │
   │                                          hash(reassembled)     │
   │                                          ─────────────         │
   │                                                                │
   │                                          actual == addr?       │
   │                                          ─────────────         │
   │                                                                │
   │ VerifiedBytes                                                 │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

---

## 5. Test Specification

### 5.1 Unit Tests

```rust
// crates/deriva-storage/tests/verified_get.rs

#[tokio::test]
async fn test_verified_get_success() {
    let store = DagStore::new_memory();
    let bytes = Bytes::from("hello world");
    let addr = CAddr::from_bytes(&bytes);
    
    store.put(addr, bytes.clone()).await.unwrap();
    
    let verified = store.get_verified(addr).await.unwrap();
    assert_eq!(verified.bytes, bytes);
    assert_eq!(verified.addr, addr);
}

#[tokio::test]
async fn test_verified_get_hash_mismatch() {
    let store = DagStore::new_memory();
    let bytes = Bytes::from("hello world");
    let addr = CAddr::from_bytes(&bytes);
    
    // Put different content
    let wrong_bytes = Bytes::from("wrong content");
    store.put(addr, wrong_bytes).await.unwrap();
    
    let result = store.get_verified(addr).await;
    assert!(matches!(result, Err(VerificationError::HashMismatch { .. })));
}

#[tokio::test]
async fn test_verified_get_not_found() {
    let store = DagStore::new_memory();
    let addr = CAddr::from_hex("abc123...").unwrap();
    
    let result = store.get_verified(addr).await;
    assert!(matches!(result, Err(VerificationError::NotFound(_))));
}

#[tokio::test]
async fn test_verified_get_retry() {
    let store = DagStore::new_memory();
    let bytes = Bytes::from("hello world");
    let addr = CAddr::from_bytes(&bytes);
    
    // Simulate transient failure
    let fail_count = Arc::new(AtomicUsize::new(0));
    let fail_count_clone = fail_count.clone();
    
    store.set_get_hook(move |_| {
        if fail_count_clone.fetch_add(1, Ordering::SeqCst) < 2 {
            None  // Fail first 2 attempts
        } else {
            Some(bytes.clone())
        }
    });
    
    let verified = store.get_verified(addr).await.unwrap();
    assert_eq!(verified.bytes, bytes);
    assert_eq!(fail_count.load(Ordering::SeqCst), 3);
}
```

### 5.2 Merkle Proof Tests

```rust
// crates/deriva-storage/tests/merkle_proof.rs

#[tokio::test]
async fn test_merkle_proof_simple() {
    let store = DagStore::new_memory();
    
    // Create simple DAG: root -> [leaf1, leaf2]
    let leaf1 = Bytes::from("leaf1");
    let leaf2 = Bytes::from("leaf2");
    let addr1 = CAddr::from_bytes(&leaf1);
    let addr2 = CAddr::from_bytes(&leaf2);
    
    store.put(addr1, leaf1.clone()).await.unwrap();
    store.put(addr2, leaf2.clone()).await.unwrap();
    
    let recipe = Recipe {
        function_id: FunctionId::from("identity"),
        inputs: vec![addr1, addr2],
        params: BTreeMap::new(),
    };
    let root = recipe.addr();
    store.put(root, bincode::serialize(&recipe).unwrap().into()).await.unwrap();
    
    // Generate proof
    let proof = MerkleProof::generate(&store, root, addr1).await.unwrap();
    
    assert_eq!(proof.root, root);
    assert_eq!(proof.leaf, addr1);
    assert_eq!(proof.path.len(), 1);
    assert_eq!(proof.path[0].sibling, addr2);
    
    // Verify proof
    assert!(proof.verify());
}

#[tokio::test]
async fn test_merkle_proof_deep() {
    let store = DagStore::new_memory();
    
    // Create deep DAG: root -> mid -> leaf
    let leaf = Bytes::from("leaf");
    let leaf_addr = CAddr::from_bytes(&leaf);
    store.put(leaf_addr, leaf.clone()).await.unwrap();
    
    let mid_recipe = Recipe {
        function_id: FunctionId::from("identity"),
        inputs: vec![leaf_addr],
        params: BTreeMap::new(),
    };
    let mid_addr = mid_recipe.addr();
    store.put(mid_addr, bincode::serialize(&mid_recipe).unwrap().into()).await.unwrap();
    
    let root_recipe = Recipe {
        function_id: FunctionId::from("identity"),
        inputs: vec![mid_addr],
        params: BTreeMap::new(),
    };
    let root_addr = root_recipe.addr();
    store.put(root_addr, bincode::serialize(&root_recipe).unwrap().into()).await.unwrap();
    
    // Generate proof
    let proof = MerkleProof::generate(&store, root_addr, leaf_addr).await.unwrap();
    
    assert_eq!(proof.root, root_addr);
    assert_eq!(proof.leaf, leaf_addr);
    assert!(proof.verify());
}

#[tokio::test]
async fn test_merkle_proof_invalid() {
    let store = DagStore::new_memory();
    
    let leaf1 = Bytes::from("leaf1");
    let leaf2 = Bytes::from("leaf2");
    let addr1 = CAddr::from_bytes(&leaf1);
    let addr2 = CAddr::from_bytes(&leaf2);
    
    store.put(addr1, leaf1.clone()).await.unwrap();
    store.put(addr2, leaf2.clone()).await.unwrap();
    
    // Create proof with wrong root
    let proof = MerkleProof {
        root: addr2,
        leaf: addr1,
        path: vec![],
    };
    
    assert!(!proof.verify());
}
```

### 5.3 Chunk Verification Tests

```rust
// crates/deriva-storage/tests/chunk_verify.rs

#[tokio::test]
async fn test_chunk_verify_success() {
    let store = DagStore::new_memory();
    let verifier = ChunkVerifier {
        store: Arc::new(store.clone()),
        chunk_size: 1024,
    };
    
    // Create chunks
    let chunk1 = Bytes::from(vec![1u8; 1024]);
    let chunk2 = Bytes::from(vec![2u8; 1024]);
    let addr1 = CAddr::from_bytes(&chunk1);
    let addr2 = CAddr::from_bytes(&chunk2);
    
    store.put(addr1, chunk1.clone()).await.unwrap();
    store.put(addr2, chunk2.clone()).await.unwrap();
    
    // Create manifest
    let manifest = ChunkManifest {
        chunks: vec![addr1, addr2],
        total_size: 2048,
    };
    let manifest_bytes = bincode::serialize(&manifest).unwrap();
    let manifest_addr = CAddr::from_bytes(&manifest_bytes);
    store.put(manifest_addr, manifest_bytes.into()).await.unwrap();
    
    // Verify
    let verified = verifier.verify_chunked(manifest_addr).await.unwrap();
    assert_eq!(verified.bytes.len(), 2048);
}

#[tokio::test]
async fn test_chunk_verify_corrupted_chunk() {
    let store = DagStore::new_memory();
    let verifier = ChunkVerifier {
        store: Arc::new(store.clone()),
        chunk_size: 1024,
    };
    
    let chunk1 = Bytes::from(vec![1u8; 1024]);
    let addr1 = CAddr::from_bytes(&chunk1);
    
    // Put corrupted chunk
    let corrupted = Bytes::from(vec![99u8; 1024]);
    store.put(addr1, corrupted).await.unwrap();
    
    let manifest = ChunkManifest {
        chunks: vec![addr1],
        total_size: 1024,
    };
    let manifest_bytes = bincode::serialize(&manifest).unwrap();
    let manifest_addr = CAddr::from_bytes(&manifest_bytes);
    store.put(manifest_addr, manifest_bytes.into()).await.unwrap();
    
    let result = verifier.verify_chunked(manifest_addr).await;
    assert!(matches!(result, Err(VerificationError::HashMismatch { .. })));
}
```

### 5.4 Audit Log Tests

```rust
// crates/deriva-storage/tests/audit_log.rs

#[tokio::test]
async fn test_audit_log_success() {
    let audit_log = AuditLog::new();
    let addr = CAddr::from_hex("abc123...").unwrap();
    
    audit_log.log_success(addr, SystemTime::now());
    
    let entries = audit_log.get_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].addr, addr);
    assert!(matches!(entries[0].result, AuditResult::Success));
}

#[tokio::test]
async fn test_audit_log_failure() {
    let audit_log = AuditLog::new();
    let addr = CAddr::from_hex("abc123...").unwrap();
    let expected = CAddr::from_hex("def456...").unwrap();
    
    let error = VerificationError::HashMismatch {
        expected,
        actual: addr,
    };
    
    audit_log.log_failure(addr, &error);
    
    let entries = audit_log.get_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].addr, addr);
    assert!(matches!(
        entries[0].result,
        AuditResult::HashMismatch { .. }
    ));
}
```

---

## 6. Edge Cases & Error Handling

### 6.1 Corrupted Storage

**Scenario:** Storage backend returns corrupted data.

```rust
#[tokio::test]
async fn test_corrupted_storage() {
    let store = DagStore::new_memory();
    let bytes = Bytes::from("hello world");
    let addr = CAddr::from_bytes(&bytes);
    
    // Simulate corruption
    store.put(addr, Bytes::from("corrupted")).await.unwrap();
    
    let result = store.get_verified(addr).await;
    assert!(matches!(result, Err(VerificationError::HashMismatch { .. })));
}
```

**Handling:**
- Return `VerificationError::HashMismatch`
- Log to audit log
- Retry with different replica (if available)

### 6.2 Partial DAG

**Scenario:** Merkle proof generation fails because part of DAG is missing.

```rust
#[tokio::test]
async fn test_partial_dag() {
    let store = DagStore::new_memory();
    
    let leaf = Bytes::from("leaf");
    let leaf_addr = CAddr::from_bytes(&leaf);
    // Don't put leaf in store
    
    let recipe = Recipe {
        function_id: FunctionId::from("identity"),
        inputs: vec![leaf_addr],
        params: BTreeMap::new(),
    };
    let root = recipe.addr();
    store.put(root, bincode::serialize(&recipe).unwrap().into()).await.unwrap();
    
    let result = MerkleProof::generate(&store, root, leaf_addr).await;
    assert!(matches!(result, Err(VerificationError::NotFound(_))));
}
```

**Handling:**
- Return `VerificationError::NotFound`
- Trigger replication to fetch missing parts

### 6.3 Concurrent Modifications

**Scenario:** Content is modified while verification is in progress.

```rust
#[tokio::test]
async fn test_concurrent_modification() {
    let store = DagStore::new_memory();
    let bytes = Bytes::from("hello world");
    let addr = CAddr::from_bytes(&bytes);
    
    store.put(addr, bytes.clone()).await.unwrap();
    
    // Start verification
    let store_clone = store.clone();
    let verify_task = tokio::spawn(async move {
        store_clone.get_verified(addr).await
    });
    
    // Modify content concurrently
    store.put(addr, Bytes::from("modified")).await.unwrap();
    
    let result = verify_task.await.unwrap();
    // Either succeeds (read before modification) or fails (read after)
    assert!(result.is_ok() || matches!(result, Err(VerificationError::HashMismatch { .. })));
}
```

**Handling:**
- Use snapshot isolation (if storage backend supports it)
- Retry on mismatch
- Log concurrent modification events

### 6.4 Large Proof Paths

**Scenario:** Merkle proof path is very long (deep DAG).

```rust
#[tokio::test]
async fn test_large_proof_path() {
    let store = DagStore::new_memory();
    
    // Create deep DAG (100 levels)
    let mut current = Bytes::from("leaf");
    let mut current_addr = CAddr::from_bytes(&current);
    store.put(current_addr, current.clone()).await.unwrap();
    
    for _ in 0..100 {
        let recipe = Recipe {
            function_id: FunctionId::from("identity"),
            inputs: vec![current_addr],
            params: BTreeMap::new(),
        };
        current_addr = recipe.addr();
        store.put(current_addr, bincode::serialize(&recipe).unwrap().into()).await.unwrap();
    }
    
    let leaf_addr = CAddr::from_bytes(&Bytes::from("leaf"));
    let proof = MerkleProof::generate(&store, current_addr, leaf_addr).await.unwrap();
    
    assert_eq!(proof.path.len(), 100);
    assert!(proof.verify());
}
```

**Handling:**
- Set maximum proof depth (e.g., 1000 levels)
- Return error if exceeded
- Consider proof compression for very deep DAGs

### 6.5 NaN in Proofs

**Scenario:** Proof contains NaN values (from §4.4).

```rust
#[tokio::test]
async fn test_proof_with_nan() {
    let store = DagStore::new_memory();
    
    // Create recipe with NaN parameter
    let recipe = Recipe {
        function_id: FunctionId::from("compute"),
        inputs: vec![],
        params: {
            let mut map = BTreeMap::new();
            map.insert("x".into(), Value::Float(f64::NAN));
            map
        },
    };
    
    let addr = recipe.addr();
    store.put(addr, bincode::serialize(&recipe).unwrap().into()).await.unwrap();
    
    // Proof generation should work (NaN is canonicalized)
    let proof = MerkleProof::generate(&store, addr, addr).await.unwrap();
    assert!(proof.verify());
}
```

**Handling:**
- NaN canonicalization (from §4.4) ensures deterministic hashing
- Proofs work correctly with NaN values

---

## 7. Performance Analysis

### 7.1 Verification Overhead

**Baseline (no verification):**
```rust
let bytes = store.get(addr).await.unwrap();
// ~100 µs for 1KB value
```

**With verification:**
```rust
let verified = store.get_verified(addr).await.unwrap();
// ~105 µs for 1KB value (5% overhead)
```

**Breakdown:**
- Storage read: 100 µs
- BLAKE3 hash: 5 µs (1 GB/s throughput)
- Comparison: <1 µs

**Target:** <5% overhead for small values, <1% for large blobs.

### 7.2 Merkle Proof Generation

**Simple DAG (2 levels, 10 inputs):**
- Proof generation: ~1 ms
- Proof size: ~320 bytes (10 siblings × 32 bytes)
- Verification: ~50 µs

**Deep DAG (100 levels, 1 input per level):**
- Proof generation: ~100 ms
- Proof size: ~3.2 KB (100 siblings × 32 bytes)
- Verification: ~5 ms

**Optimization:** Cache intermediate proofs for frequently accessed paths.

### 7.3 Chunk Verification

**10 GB blob, 1 MB chunks:**
- Total chunks: 10,000
- Parallel verification (100 workers): ~10 seconds
- Sequential verification: ~1000 seconds

**Speedup:** 100x with parallelization.

### 7.4 Audit Log Overhead

**Memory usage:**
- 1M entries: ~100 MB (100 bytes per entry)
- 10M entries: ~1 GB

**Mitigation:**
- Rotate logs daily
- Compress old logs
- Store in separate database

---

## 8. Files Changed

### New Files
- `crates/deriva-storage/src/verified_get.rs` — `VerifiedGet` implementation
- `crates/deriva-storage/src/merkle_proof.rs` — `MerkleProof` generation/verification
- `crates/deriva-storage/src/chunk_verify.rs` — Chunk-level verification
- `crates/deriva-storage/src/audit_log.rs` — Audit logging
- `crates/deriva-storage/tests/verified_get.rs` — Unit tests
- `crates/deriva-storage/tests/merkle_proof.rs` — Merkle proof tests
- `crates/deriva-storage/tests/chunk_verify.rs` — Chunk verification tests
- `crates/deriva-storage/tests/audit_log.rs` — Audit log tests

### Modified Files
- `crates/deriva-storage/src/dag_store.rs` — Add `get_verified()`, `get_with_proof()`
- `crates/deriva-storage/src/lib.rs` — Export new types
- `crates/deriva-core/src/address.rs` — Add `CAddr::combine()`
- `crates/deriva-compute/src/executor.rs` — Use `get_verified()` in materialization
- `crates/deriva-server/src/rest.rs` — Add `/get_with_proof`, `/verify_proof` endpoints

---

## 9. Dependency Changes

```toml
# crates/deriva-storage/Cargo.toml
[dependencies]
blake3 = "1.5"
bytes = "1.5"
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"
tokio = { version = "1.35", features = ["sync"] }
futures = "0.3"
thiserror = "1.0"

[dev-dependencies]
tokio = { version = "1.35", features = ["full"] }
```

No new dependencies — uses existing BLAKE3 for hashing.

---

## 10. Design Rationale

### 10.1 Why Verify on Every Read?

**Alternative:** Trust storage backend, only verify on write.

**Problem:** Storage can be compromised after write (bit rot, malicious modification).

**Decision:** Verify on every read to detect corruption immediately.

### 10.2 Why Merkle Proofs?

**Alternative:** Download entire DAG to verify membership.

**Problem:** Inefficient for large DAGs (10 GB+ blobs).

**Decision:** Merkle proofs allow verification without full download (logarithmic proof size).

### 10.3 Why Audit Log?

**Alternative:** No logging, just return errors.

**Problem:** Can't diagnose patterns (e.g., specific node always returns corrupted data).

**Decision:** Audit log enables forensics and anomaly detection.

### 10.4 Why Chunk-Level Verification?

**Alternative:** Verify entire blob at once.

**Problem:** Must download entire blob before verification (slow for large blobs).

**Decision:** Chunk-level verification enables streaming and early failure detection.

---

## 11. Observability Integration

### 11.1 Metrics

```rust
lazy_static! {
    static ref VERIFICATION_TOTAL: IntCounterVec = register_int_counter_vec!(
        "deriva_verification_total",
        "Total verifications by result",
        &["result"]
    ).unwrap();

    static ref VERIFICATION_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_verification_duration_seconds",
        "Verification duration by type",
        &["type"]
    ).unwrap();
    
    static ref MERKLE_PROOF_SIZE: Histogram = register_histogram!(
        "deriva_merkle_proof_size_bytes",
        "Merkle proof size in bytes"
    ).unwrap();
    
    static ref CHUNK_VERIFICATION_PARALLELISM: Histogram = register_histogram!(
        "deriva_chunk_verification_parallelism",
        "Number of chunks verified in parallel"
    ).unwrap();
}
```

### 11.2 Logs

```rust
impl VerifiedGet {
    async fn try_get(&self, addr: CAddr) -> Result<VerifiedBytes, VerificationError> {
        let start = Instant::now();
        
        let bytes = self.store.get(addr).await
            .ok_or(VerificationError::NotFound(addr))?;
        
        let actual = CAddr::from_bytes(&bytes);
        
        if actual != addr {
            error!(
                "Hash mismatch: expected {}, got {}",
                addr.to_hex(),
                actual.to_hex()
            );
            VERIFICATION_TOTAL.with_label_values(&["hash_mismatch"]).inc();
            return Err(VerificationError::HashMismatch { expected: addr, actual });
        }
        
        let duration = start.elapsed();
        VERIFICATION_DURATION
            .with_label_values(&["get"])
            .observe(duration.as_secs_f64());
        VERIFICATION_TOTAL.with_label_values(&["success"]).inc();
        
        debug!("Verified {} in {:?}", addr.to_hex(), duration);
        
        Ok(VerifiedBytes {
            bytes,
            addr,
            verified_at: SystemTime::now(),
        })
    }
}
```

### 11.3 Tracing

```rust
use tracing::{info_span, instrument};

#[instrument(skip(self))]
pub async fn get_verified(&self, addr: CAddr) -> Result<VerifiedBytes, VerificationError> {
    let span = info_span!("verified_get", addr = %addr.to_hex());
    let _enter = span.enter();
    
    // ... verification logic ...
}

#[instrument(skip(store))]
pub async fn generate(
    store: &DagStore,
    root: CAddr,
    leaf: CAddr,
) -> Result<MerkleProof, VerificationError> {
    let span = info_span!(
        "merkle_proof_generate",
        root = %root.to_hex(),
        leaf = %leaf.to_hex()
    );
    let _enter = span.enter();
    
    // ... proof generation logic ...
}
```

---

## 12. Checklist

### Implementation
- [ ] Create `deriva-storage/src/verified_get.rs` with `VerifiedGet` type
- [ ] Implement `try_get()` with hash verification
- [ ] Implement retry logic (max 3 attempts)
- [ ] Create `deriva-storage/src/merkle_proof.rs` with `MerkleProof` type
- [ ] Implement `MerkleProof::generate()` (DAG traversal)
- [ ] Implement `MerkleProof::verify()` (hash chain verification)
- [ ] Implement `MerkleProof::contains()` (membership check)
- [ ] Add `CAddr::combine()` for Merkle tree hashing
- [ ] Create `deriva-storage/src/chunk_verify.rs` with `ChunkVerifier`
- [ ] Implement parallel chunk verification
- [ ] Create `deriva-storage/src/audit_log.rs` with `AuditLog`
- [ ] Implement `log_success()` and `log_failure()`
- [ ] Add `get_verified()` to `DagStore`
- [ ] Add `get_with_proof()` to `DagStore`
- [ ] Integrate `get_verified()` into `Executor::materialize()`
- [ ] Add REST endpoints: `/get_with_proof`, `/verify_proof`

### Testing
- [ ] Unit test: verified get success
- [ ] Unit test: verified get hash mismatch
- [ ] Unit test: verified get not found
- [ ] Unit test: verified get retry on transient failure
- [ ] Unit test: Merkle proof simple DAG (2 levels)
- [ ] Unit test: Merkle proof deep DAG (100 levels)
- [ ] Unit test: Merkle proof invalid (wrong root)
- [ ] Unit test: Merkle proof partial DAG (missing nodes)
- [ ] Unit test: chunk verification success
- [ ] Unit test: chunk verification corrupted chunk
- [ ] Unit test: chunk verification parallel (10,000 chunks)
- [ ] Unit test: audit log success
- [ ] Unit test: audit log failure
- [ ] Integration test: verified get in executor
- [ ] Integration test: Merkle proof via REST API
- [ ] Benchmark: verification overhead (<5% for 1KB)
- [ ] Benchmark: Merkle proof generation (simple vs deep)
- [ ] Benchmark: chunk verification parallelism (100x speedup)

### Documentation
- [ ] Document verified get API
- [ ] Add examples of Merkle proof generation/verification
- [ ] Document chunk verification for large blobs
- [ ] Add troubleshooting guide for verification failures
- [ ] Document audit log format and rotation
- [ ] Add security considerations (tamper detection)

### Observability
- [ ] Add `deriva_verification_total` counter
- [ ] Add `deriva_verification_duration_seconds` histogram
- [ ] Add `deriva_merkle_proof_size_bytes` histogram
- [ ] Add `deriva_chunk_verification_parallelism` histogram
- [ ] Add error logs for hash mismatches
- [ ] Add debug logs for successful verifications
- [ ] Add tracing spans for verified get and proof generation

### Validation
- [ ] Run tests with corrupted storage backend
- [ ] Verify hash mismatch detection (100% accuracy)
- [ ] Benchmark verification overhead (<5% for small, <1% for large)
- [ ] Test Merkle proofs with deep DAGs (100+ levels)
- [ ] Test chunk verification with 10 GB blobs
- [ ] Verify audit log captures all events
- [ ] Test concurrent modifications (snapshot isolation)

### Deployment
- [ ] Enable verified get by default in production
- [ ] Monitor verification failure rate (target: <0.01%)
- [ ] Set up alerts for hash mismatches
- [ ] Configure audit log rotation (daily)
- [ ] Document verification policy (always verify vs trust cache)
- [ ] Add admin API to query audit log

---

**Estimated effort:** 5–7 days
- Days 1-2: Core `VerifiedGet` + hash verification + retry logic
- Day 3: Merkle proof generation/verification + `CAddr::combine()`
- Day 4: Chunk-level verification + parallelization
- Day 5: Audit log + integration with executor
- Days 6-7: Tests + benchmarks + REST API + documentation

**Success criteria:**
1. All tests pass (verified get, Merkle proofs, chunk verification)
2. Verification overhead <5% for small values, <1% for large blobs
3. Merkle proofs work for deep DAGs (100+ levels)
4. Chunk verification achieves 100x speedup with parallelization
5. Hash mismatch detection is 100% accurate
6. Audit log captures all verification events
7. REST API exposes proof generation/verification
