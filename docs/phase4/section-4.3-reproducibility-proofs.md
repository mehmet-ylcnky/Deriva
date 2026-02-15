# §4.3 Reproducibility Proofs & Audit Trail

> **Status**: Blueprint  
> **Depends on**: §4.1 (Canonical Serialization), §4.2 (Deterministic Scheduling)  
> **Crate(s)**: `deriva-core`, `deriva-compute`, `deriva-storage`  
> **Estimated effort**: 8–10 days  

---

## 1. Problem Statement

### 1.1 The Trust Problem

Current state: A client requests CAddr `0xabc...` and receives data. How do they know it's correct?

```
Client                    Server
  │                         │
  │ Get(0xabc...)           │
  ├────────────────────────>│
  │                         │
  │                         │ [returns data]
  │      data               │
  │<────────────────────────┤
  │                         │
  │ ??? Is this correct ??? │
```

**The client must trust:**
1. The server computed the recipe correctly
2. The cached result wasn't corrupted
3. The recipe graph is what the client expects
4. No malicious tampering occurred

**No way to verify without re-executing everything.**

### 1.2 Why This Matters

**Regulated industries:**
- **Finance:** Audit trail for risk calculations, compliance reports
- **Healthcare:** Provenance of medical imaging pipelines, drug discovery results
- **Scientific computing:** Reproducibility of research results, peer review

**Security:**
- **Supply chain:** Verify build artifacts came from specific source code
- **ML pipelines:** Prove model was trained on specific dataset with specific hyperparameters
- **Data lineage:** Track data transformations for GDPR compliance

**Current limitations:**
- No cryptographic proof of correctness
- No audit trail of who computed what, when
- No way to detect tampering
- No way to verify without full re-execution

### 1.3 What We Need

**Merkle proof chains** — every derived value carries a cryptographic proof that:
1. Links output to inputs through the recipe graph
2. Can be verified by third parties without re-execution
3. Includes signatures from computing nodes
4. Forms an audit trail for compliance

**Key properties:**
- **Verifiable:** Anyone with the proof can verify correctness
- **Compact:** Proof size is O(log N) for DAG depth N (with anchoring)
- **Tamper-evident:** Any modification invalidates the proof
- **Non-repudiable:** Node signatures prevent denial

---

## 2. Design

### 2.1 Proof Structure

```rust
// crates/deriva-core/src/proof.rs (new file)

use serde::{Serialize, Deserialize};
use ed25519_dalek::{Signature, PublicKey};

/// Cryptographic proof that a value was correctly derived
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivationProof {
    /// Recipe that produced this value
    pub recipe_addr: CAddr,
    
    /// Proofs for each input (recursive)
    pub input_proofs: Vec<InputProof>,
    
    /// Output address (hash of output data)
    pub output_addr: CAddr,
    
    /// Hash of output data (for verification)
    pub output_hash: [u8; 32],
    
    /// Node that performed the computation
    pub compute_node_id: NodeId,
    
    /// Timestamp (Unix epoch seconds)
    pub timestamp: u64,
    
    /// Ed25519 signature over (recipe_addr || input_addrs || output_hash || timestamp)
    pub signature: Signature,
}

/// Proof for a single input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputProof {
    /// Leaf data (trusted, no further proof)
    Leaf {
        addr: CAddr,
        hash: [u8; 32],
    },
    
    /// Derived data (recursive proof)
    Derived {
        addr: CAddr,
        proof: Box<DerivationProof>,
    },
    
    /// Anchored proof (reference to trusted intermediate)
    Anchored {
        addr: CAddr,
        anchor_proof_id: ProofId,
    },
}

/// Unique identifier for a proof (hash of proof structure)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProofId([u8; 32]);

/// Node identifier (public key)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeId(pub PublicKey);
```

### 2.2 Proof Generation

```
Materialization:
  Recipe R with inputs [A, B, C]
  
  1. Resolve inputs (with proofs):
     A → (data_a, proof_a)
     B → (data_b, proof_b)
     C → (data_c, proof_c)
  
  2. Execute function:
     output = function.execute([data_a, data_b, data_c], params)
  
  3. Generate proof:
     proof = DerivationProof {
       recipe_addr: R.addr(),
       input_proofs: [proof_a, proof_b, proof_c],
       output_addr: CAddr(blake3(output)),
       output_hash: blake3(output),
       compute_node_id: self.node_id,
       timestamp: now(),
       signature: sign(recipe_addr || input_addrs || output_hash || timestamp),
     }
  
  4. Store proof alongside cached value:
     cache.put(output_addr, (output, proof))
```

### 2.3 Proof Verification

```rust
impl DerivationProof {
    /// Verify proof without re-executing computation
    pub fn verify(&self, trusted_leaves: &HashSet<CAddr>) -> Result<(), ProofError> {
        // 1. Verify signature
        let message = self.signing_message();
        self.compute_node_id.0.verify(&message, &self.signature)
            .map_err(|_| ProofError::InvalidSignature)?;
        
        // 2. Verify input proofs recursively
        for input_proof in &self.input_proofs {
            match input_proof {
                InputProof::Leaf { addr, hash } => {
                    if !trusted_leaves.contains(addr) {
                        return Err(ProofError::UntrustedLeaf(*addr));
                    }
                }
                InputProof::Derived { addr, proof } => {
                    proof.verify(trusted_leaves)?;
                    if proof.output_addr != *addr {
                        return Err(ProofError::AddressMismatch {
                            expected: *addr,
                            actual: proof.output_addr,
                        });
                    }
                }
                InputProof::Anchored { addr, anchor_proof_id } => {
                    // Anchored proofs require external verification
                    // (caller must have verified the anchor)
                    if !trusted_leaves.contains(addr) {
                        return Err(ProofError::UntrustedAnchor(*addr));
                    }
                }
            }
        }
        
        // 3. Verify recipe exists and matches
        // (caller must provide recipe store)
        
        Ok(())
    }
    
    fn signing_message(&self) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(self.recipe_addr.as_bytes());
        for input_proof in &self.input_proofs {
            msg.extend_from_slice(input_proof.addr().as_bytes());
        }
        msg.extend_from_slice(&self.output_hash);
        msg.extend_from_slice(&self.timestamp.to_be_bytes());
        msg
    }
}
```

### 2.4 Proof Compaction (Anchoring)

For deep DAGs, full recursive proofs can be large. **Anchoring** allows referencing a trusted intermediate proof:

```
Full proof (depth 10):
  D10 → D9 → D8 → D7 → D6 → D5 → D4 → D3 → D2 → D1 → Leaf
  
  Proof size: O(depth) = 10 proof objects

Anchored proof (anchor at D5):
  D10 → D9 → D8 → D7 → D6 → [Anchor: D5]
  
  Proof size: O(depth - anchor_depth) = 5 proof objects
  
  Verification:
    1. Verify D10 → D6 (5 steps)
    2. Check D5 is in trusted anchor set
    3. Done (no need to verify D5 → Leaf)
```

**Anchor management:**
```rust
pub struct AnchorStore {
    /// Trusted proof IDs (verified and stored)
    anchors: HashMap<CAddr, ProofId>,
}

impl AnchorStore {
    pub fn add_anchor(&mut self, addr: CAddr, proof: &DerivationProof) -> ProofId {
        let proof_id = ProofId(blake3::hash(&bincode::serialize(proof).unwrap()).into());
        self.anchors.insert(addr, proof_id);
        proof_id
    }
    
    pub fn is_anchored(&self, addr: &CAddr) -> bool {
        self.anchors.contains_key(addr)
    }
}
```

---

## 3. Implementation

### 3.1 Node Signing Keys

```rust
// crates/deriva-server/src/node_identity.rs (new file)

use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signature, Signer};
use std::path::Path;

pub struct NodeIdentity {
    keypair: Keypair,
    node_id: NodeId,
}

impl NodeIdentity {
    /// Generate new identity
    pub fn generate() -> Self {
        let mut csprng = rand::rngs::OsRng;
        let keypair = Keypair::generate(&mut csprng);
        let node_id = NodeId(keypair.public);
        Self { keypair, node_id }
    }
    
    /// Load from file
    pub fn load(path: &Path) -> Result<Self, std::io::Error> {
        let bytes = std::fs::read(path)?;
        let secret = SecretKey::from_bytes(&bytes[..32])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let public = PublicKey::from(&secret);
        let keypair = Keypair { secret, public };
        let node_id = NodeId(public);
        Ok(Self { keypair, node_id })
    }
    
    /// Save to file
    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        std::fs::write(path, self.keypair.secret.as_bytes())
    }
    
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }
    
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.keypair.sign(message)
    }
}
```

### 3.2 Proof-Generating Executor

```rust
// crates/deriva-compute/src/executor.rs

impl<'a, C: MaterializationCache, L: LeafStore> Executor<'a, C, L> {
    pub async fn materialize_with_proof(
        &mut self,
        addr: CAddr,
    ) -> Result<(Bytes, DerivationProof), ExecutionError> {
        // Check cache (with proof)
        if let Some((data, proof)) = self.cache.get_with_proof(&addr) {
            return Ok((data, proof));
        }

        let recipe = self.dag.get_recipe(&addr)
            .ok_or(ExecutionError::RecipeNotFound(addr))?;

        // Resolve inputs with proofs
        let mut input_data = Vec::new();
        let mut input_proofs = Vec::new();

        for input_ref in &recipe.inputs {
            let (data, proof) = self.resolve_input_with_proof(input_ref).await?;
            input_data.push(data);
            input_proofs.push(proof);
        }

        // Execute function
        let function = self.registry.get(&recipe.function_id)
            .ok_or(ExecutionError::FunctionNotFound(recipe.function_id.clone()))?;
        
        let output = function.execute(input_data, &recipe.params)?;
        let output_hash = blake3::hash(&output);
        let output_addr = CAddr(output_hash.into());

        // Generate proof
        let proof = DerivationProof {
            recipe_addr: addr,
            input_proofs,
            output_addr,
            output_hash: output_hash.into(),
            compute_node_id: self.node_identity.node_id().clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: self.sign_proof(&addr, &input_proofs, &output_hash),
        };

        // Cache with proof
        self.cache.put_with_proof(output_addr, output.clone(), proof.clone());

        Ok((output, proof))
    }

    async fn resolve_input_with_proof(
        &mut self,
        input_ref: &DataRef,
    ) -> Result<(Bytes, InputProof), ExecutionError> {
        match input_ref {
            DataRef::Leaf(addr) => {
                let data = self.leaf_store.get(addr)
                    .ok_or(ExecutionError::LeafNotFound(*addr))?;
                let hash = blake3::hash(&data);
                Ok((data, InputProof::Leaf {
                    addr: *addr,
                    hash: hash.into(),
                }))
            }
            DataRef::Derived(addr) => {
                let (data, proof) = self.materialize_with_proof(*addr).await?;
                Ok((data, InputProof::Derived {
                    addr: *addr,
                    proof: Box::new(proof),
                }))
            }
        }
    }

    fn sign_proof(
        &self,
        recipe_addr: &CAddr,
        input_proofs: &[InputProof],
        output_hash: &blake3::Hash,
    ) -> Signature {
        let mut msg = Vec::new();
        msg.extend_from_slice(recipe_addr.as_bytes());
        for input_proof in input_proofs {
            msg.extend_from_slice(input_proof.addr().as_bytes());
        }
        msg.extend_from_slice(output_hash.as_bytes());
        msg.extend_from_slice(&std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_be_bytes());
        
        self.node_identity.sign(&msg)
    }
}
```

### 3.3 Proof Storage

```rust
// crates/deriva-storage/src/proof_store.rs (new file)

use sled::Db;

pub struct ProofStore {
    db: Db,
}

impl ProofStore {
    pub fn open(path: &Path) -> Result<Self, sled::Error> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    pub fn put(&self, addr: &CAddr, proof: &DerivationProof) -> Result<(), StorageError> {
        let bytes = bincode::serialize(proof)
            .map_err(|e| StorageError::SerializationFailed(e.to_string()))?;
        self.db.insert(addr.as_bytes(), bytes)?;
        Ok(())
    }

    pub fn get(&self, addr: &CAddr) -> Result<Option<DerivationProof>, StorageError> {
        match self.db.get(addr.as_bytes())? {
            Some(bytes) => {
                let proof = bincode::deserialize(&bytes)
                    .map_err(|e| StorageError::DeserializationFailed(e.to_string()))?;
                Ok(Some(proof))
            }
            None => Ok(None),
        }
    }

    pub fn delete(&self, addr: &CAddr) -> Result<(), StorageError> {
        self.db.remove(addr.as_bytes())?;
        Ok(())
    }
}
```

### 3.4 Audit Log

```rust
// crates/deriva-storage/src/audit_log.rs (new file)

use std::fs::OpenOptions;
use std::io::Write;

/// Append-only audit log of all materializations
pub struct AuditLog {
    file: std::fs::File,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub recipe_addr: CAddr,
    pub output_addr: CAddr,
    pub compute_node_id: NodeId,
    pub proof_id: ProofId,
}

impl AuditLog {
    pub fn open(path: &Path) -> Result<Self, std::io::Error> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self { file })
    }

    pub fn append(&mut self, entry: &AuditEntry) -> Result<(), std::io::Error> {
        let json = serde_json::to_string(entry)?;
        writeln!(self.file, "{}", json)?;
        self.file.flush()?;
        Ok(())
    }

    pub fn query(
        &self,
        start_time: u64,
        end_time: u64,
    ) -> Result<Vec<AuditEntry>, std::io::Error> {
        let contents = std::fs::read_to_string(self.file)?;
        let entries: Vec<AuditEntry> = contents
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .filter(|e: &AuditEntry| e.timestamp >= start_time && e.timestamp <= end_time)
            .collect();
        Ok(entries)
    }
}
```


### 3.5 RPC Integration

```rust
// crates/deriva-server/src/service.proto

message GetProofRequest {
  bytes addr = 1;
}

message GetProofResponse {
  DerivationProof proof = 1;
}

message VerifyProofRequest {
  DerivationProof proof = 1;
  repeated bytes trusted_leaves = 2;
}

message VerifyProofResponse {
  bool valid = 1;
  string error = 2;
}

service DerivaService {
  // ... existing RPCs ...
  
  rpc GetProof(GetProofRequest) returns (GetProofResponse);
  rpc VerifyProof(VerifyProofRequest) returns (VerifyProofResponse);
}
```

```rust
// crates/deriva-server/src/service.rs

impl DerivaService {
    pub async fn get_proof(
        &self,
        request: Request<GetProofRequest>,
    ) -> Result<Response<GetProofResponse>, Status> {
        let addr = CAddr::from_bytes(&request.get_ref().addr);
        
        let proof = self.proof_store.get(&addr)
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("proof not found"))?;
        
        Ok(Response::new(GetProofResponse { proof: Some(proof) }))
    }

    pub async fn verify_proof(
        &self,
        request: Request<VerifyProofRequest>,
    ) -> Result<Response<VerifyProofResponse>, Status> {
        let req = request.get_ref();
        let proof = req.proof.as_ref()
            .ok_or_else(|| Status::invalid_argument("proof required"))?;
        
        let trusted_leaves: HashSet<CAddr> = req.trusted_leaves
            .iter()
            .map(|bytes| CAddr::from_bytes(bytes))
            .collect();
        
        match proof.verify(&trusted_leaves) {
            Ok(()) => Ok(Response::new(VerifyProofResponse {
                valid: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(VerifyProofResponse {
                valid: false,
                error: e.to_string(),
            })),
        }
    }
}
```

---

## 4. Data Flow Diagrams

### 4.1 Proof Generation

```
Client                Executor              ProofStore         AuditLog
  │                      │                      │                 │
  │ Materialize(R)       │                      │                 │
  ├─────────────────────>│                      │                 │
  │                      │                      │                 │
  │                      │ Resolve inputs       │                 │
  │                      │ with proofs          │                 │
  │                      │                      │                 │
  │                      │ Execute function     │                 │
  │                      │ output = f(inputs)   │                 │
  │                      │                      │                 │
  │                      │ Generate proof:      │                 │
  │                      │ - recipe_addr        │                 │
  │                      │ - input_proofs       │                 │
  │                      │ - output_hash        │                 │
  │                      │ - sign()             │                 │
  │                      │                      │                 │
  │                      │ store(addr, proof)   │                 │
  │                      ├─────────────────────>│                 │
  │                      │                      │                 │
  │                      │ append(audit_entry)  │                 │
  │                      ├─────────────────────────────────────>│
  │                      │                      │                 │
  │   (output, proof)    │                      │                 │
  │<─────────────────────┤                      │                 │
```

### 4.2 Proof Verification

```
Verifier            DerivaService         ProofStore         RecipeStore
  │                      │                     │                  │
  │ GetProof(addr)       │                     │                  │
  ├─────────────────────>│                     │                  │
  │                      │                     │                  │
  │                      │ get(addr)           │                  │
  │                      ├────────────────────>│                  │
  │                      │                     │                  │
  │                      │      proof          │                  │
  │                      │<────────────────────┤                  │
  │                      │                     │                  │
  │        proof         │                     │                  │
  │<─────────────────────┤                     │                  │
  │                      │                     │                  │
  │ Verify locally:      │                     │                  │
  │ 1. Check signature   │                     │                  │
  │ 2. Verify input      │                     │                  │
  │    proofs (recursive)│                     │                  │
  │ 3. Check recipe      │                     │                  │
  │    matches           │                     │                  │
  │                      │                     │                  │
  │ ✅ Valid             │                     │                  │
```

### 4.3 Anchored Proof (Compaction)

```
Full proof (depth 5):

  D5 ──▶ D4 ──▶ D3 ──▶ D2 ──▶ D1 ──▶ Leaf
  │       │       │       │       │
  proof5  proof4  proof3  proof2  proof1

  Total size: 5 proof objects


Anchored proof (anchor at D3):

  D5 ──▶ D4 ──▶ [Anchor: D3]
  │       │       │
  proof5  proof4  anchor_ref

  Total size: 2 proof objects + 1 anchor reference

  Verification:
    1. Verify D5 → D4 (check signature, input proofs)
    2. Verify D4 → D3 (check signature, input proofs)
    3. Check D3 is in trusted anchor set ✅
    4. Done (no need to verify D3 → Leaf)
```

---

## 5. Test Specification

### 5.1 Unit Tests

```rust
// crates/deriva-core/tests/proof_verification.rs

#[test]
fn test_proof_generation_leaf() {
    let node_identity = NodeIdentity::generate();
    let leaf_addr = CAddr::from_bytes(b"leaf_data");
    let leaf_hash = blake3::hash(b"leaf_data");

    let proof = InputProof::Leaf {
        addr: leaf_addr,
        hash: leaf_hash.into(),
    };

    // Leaf proofs don't need verification (trusted)
    assert_eq!(proof.addr(), leaf_addr);
}

#[test]
fn test_proof_generation_derived() {
    let node_identity = NodeIdentity::generate();
    
    // Create a simple recipe: concat(leaf_a, leaf_b)
    let leaf_a = CAddr::from_bytes(b"A");
    let leaf_b = CAddr::from_bytes(b"B");
    
    let recipe = Recipe {
        function_id: FunctionId::from("concat"),
        inputs: vec![DataRef::Leaf(leaf_a), DataRef::Leaf(leaf_b)],
        params: BTreeMap::new(),
    };
    let recipe_addr = recipe.addr();

    let output = b"AB";
    let output_hash = blake3::hash(output);
    let output_addr = CAddr(output_hash.into());

    let proof = DerivationProof {
        recipe_addr,
        input_proofs: vec![
            InputProof::Leaf { addr: leaf_a, hash: blake3::hash(b"A").into() },
            InputProof::Leaf { addr: leaf_b, hash: blake3::hash(b"B").into() },
        ],
        output_addr,
        output_hash: output_hash.into(),
        compute_node_id: node_identity.node_id().clone(),
        timestamp: 1234567890,
        signature: node_identity.sign(&proof.signing_message()),
    };

    // Verify proof
    let mut trusted_leaves = HashSet::new();
    trusted_leaves.insert(leaf_a);
    trusted_leaves.insert(leaf_b);

    assert!(proof.verify(&trusted_leaves).is_ok());
}

#[test]
fn test_proof_verification_invalid_signature() {
    let node_identity_1 = NodeIdentity::generate();
    let node_identity_2 = NodeIdentity::generate();
    
    let leaf_addr = CAddr::from_bytes(b"leaf");
    let recipe = Recipe {
        function_id: FunctionId::from("identity"),
        inputs: vec![DataRef::Leaf(leaf_addr)],
        params: BTreeMap::new(),
    };
    let recipe_addr = recipe.addr();

    let output_hash = blake3::hash(b"output");
    let output_addr = CAddr(output_hash.into());

    // Sign with node_identity_1
    let proof = DerivationProof {
        recipe_addr,
        input_proofs: vec![InputProof::Leaf {
            addr: leaf_addr,
            hash: blake3::hash(b"leaf").into(),
        }],
        output_addr,
        output_hash: output_hash.into(),
        compute_node_id: node_identity_1.node_id().clone(),
        timestamp: 1234567890,
        signature: node_identity_1.sign(&proof.signing_message()),
    };

    // Tamper with proof (change output_addr)
    let mut tampered_proof = proof.clone();
    tampered_proof.output_addr = CAddr::from_bytes(b"tampered");

    // Verification should fail (signature doesn't match)
    let mut trusted_leaves = HashSet::new();
    trusted_leaves.insert(leaf_addr);

    assert!(tampered_proof.verify(&trusted_leaves).is_err());
}

#[test]
fn test_proof_verification_untrusted_leaf() {
    let node_identity = NodeIdentity::generate();
    
    let leaf_addr = CAddr::from_bytes(b"leaf");
    let recipe = Recipe {
        function_id: FunctionId::from("identity"),
        inputs: vec![DataRef::Leaf(leaf_addr)],
        params: BTreeMap::new(),
    };
    let recipe_addr = recipe.addr();

    let output_hash = blake3::hash(b"output");
    let output_addr = CAddr(output_hash.into());

    let proof = DerivationProof {
        recipe_addr,
        input_proofs: vec![InputProof::Leaf {
            addr: leaf_addr,
            hash: blake3::hash(b"leaf").into(),
        }],
        output_addr,
        output_hash: output_hash.into(),
        compute_node_id: node_identity.node_id().clone(),
        timestamp: 1234567890,
        signature: node_identity.sign(&proof.signing_message()),
    };

    // Empty trusted set → verification fails
    let trusted_leaves = HashSet::new();
    assert!(matches!(
        proof.verify(&trusted_leaves),
        Err(ProofError::UntrustedLeaf(_))
    ));
}
```

### 5.2 Integration Tests

```rust
// crates/deriva-compute/tests/proof_integration.rs

#[tokio::test]
async fn test_end_to_end_proof_generation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let dag = DagStore::new();
    let registry = FunctionRegistry::new();
    let mut cache = EvictableCache::new(1024 * 1024);
    let leaf_store = InMemoryLeafStore::new();
    let node_identity = NodeIdentity::generate();

    registry.register(Box::new(ConcatFunction));

    // Store leaves
    let leaf_a = CAddr::from_bytes(b"A");
    let leaf_b = CAddr::from_bytes(b"B");
    leaf_store.put(leaf_a, Bytes::from("A"));
    leaf_store.put(leaf_b, Bytes::from("B"));

    // Create recipe
    let recipe = Recipe {
        function_id: FunctionId::from("concat"),
        inputs: vec![DataRef::Leaf(leaf_a), DataRef::Leaf(leaf_b)],
        params: BTreeMap::new(),
    };
    let recipe_addr = recipe.addr();
    dag.put_recipe(&recipe);

    // Materialize with proof
    let mut executor = Executor::new(
        &dag,
        &registry,
        &mut cache,
        &leaf_store,
        node_identity,
    );
    let (output, proof) = executor.materialize_with_proof(recipe_addr).await.unwrap();

    // Verify output
    assert_eq!(output, Bytes::from("AB"));

    // Verify proof
    let mut trusted_leaves = HashSet::new();
    trusted_leaves.insert(leaf_a);
    trusted_leaves.insert(leaf_b);

    assert!(proof.verify(&trusted_leaves).is_ok());
    assert_eq!(proof.recipe_addr, recipe_addr);
    assert_eq!(proof.output_addr, CAddr(blake3::hash(&output).into()));
}

#[tokio::test]
async fn test_multi_level_proof() {
    // DAG:
    //   L1, L2 → R1 → D1
    //   L3, L4 → R2 → D2
    //   D1, D2 → R3 → D3

    let dag = DagStore::new();
    let registry = FunctionRegistry::new();
    let mut cache = EvictableCache::new(1024 * 1024);
    let leaf_store = InMemoryLeafStore::new();
    let node_identity = NodeIdentity::generate();

    registry.register(Box::new(ConcatFunction));

    // Leaves
    let l1 = CAddr::from_bytes(b"L1");
    let l2 = CAddr::from_bytes(b"L2");
    let l3 = CAddr::from_bytes(b"L3");
    let l4 = CAddr::from_bytes(b"L4");
    leaf_store.put(l1, Bytes::from("L1"));
    leaf_store.put(l2, Bytes::from("L2"));
    leaf_store.put(l3, Bytes::from("L3"));
    leaf_store.put(l4, Bytes::from("L4"));

    // R1: concat(L1, L2)
    let r1 = Recipe {
        function_id: FunctionId::from("concat"),
        inputs: vec![DataRef::Leaf(l1), DataRef::Leaf(l2)],
        params: BTreeMap::new(),
    };
    let d1 = r1.addr();
    dag.put_recipe(&r1);

    // R2: concat(L3, L4)
    let r2 = Recipe {
        function_id: FunctionId::from("concat"),
        inputs: vec![DataRef::Leaf(l3), DataRef::Leaf(l4)],
        params: BTreeMap::new(),
    };
    let d2 = r2.addr();
    dag.put_recipe(&r2);

    // R3: concat(D1, D2)
    let r3 = Recipe {
        function_id: FunctionId::from("concat"),
        inputs: vec![DataRef::Derived(d1), DataRef::Derived(d2)],
        params: BTreeMap::new(),
    };
    let d3 = r3.addr();
    dag.put_recipe(&r3);

    // Materialize D3 with proof
    let mut executor = Executor::new(
        &dag,
        &registry,
        &mut cache,
        &leaf_store,
        node_identity,
    );
    let (output, proof) = executor.materialize_with_proof(d3).await.unwrap();

    // Verify output
    assert_eq!(output, Bytes::from("L1L2L3L4"));

    // Verify proof (recursive)
    let mut trusted_leaves = HashSet::new();
    trusted_leaves.insert(l1);
    trusted_leaves.insert(l2);
    trusted_leaves.insert(l3);
    trusted_leaves.insert(l4);

    assert!(proof.verify(&trusted_leaves).is_ok());

    // Check proof structure
    assert_eq!(proof.input_proofs.len(), 2);
    match &proof.input_proofs[0] {
        InputProof::Derived { proof: d1_proof, .. } => {
            assert_eq!(d1_proof.input_proofs.len(), 2);
        }
        _ => panic!("expected derived proof"),
    }
}
```

### 5.3 Audit Log Tests

```rust
#[tokio::test]
async fn test_audit_log_append() {
    let temp_dir = tempfile::tempdir().unwrap();
    let log_path = temp_dir.path().join("audit.log");
    let mut audit_log = AuditLog::open(&log_path).unwrap();

    let entry = AuditEntry {
        timestamp: 1234567890,
        recipe_addr: CAddr::from_bytes(b"recipe"),
        output_addr: CAddr::from_bytes(b"output"),
        compute_node_id: NodeId(PublicKey::from_bytes(&[0u8; 32]).unwrap()),
        proof_id: ProofId([0u8; 32]),
    };

    audit_log.append(&entry).unwrap();

    // Read back
    let entries = audit_log.query(0, u64::MAX).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].timestamp, entry.timestamp);
}

#[tokio::test]
async fn test_audit_log_query_time_range() {
    let temp_dir = tempfile::tempdir().unwrap();
    let log_path = temp_dir.path().join("audit.log");
    let mut audit_log = AuditLog::open(&log_path).unwrap();

    // Append entries with different timestamps
    for i in 0..10 {
        let entry = AuditEntry {
            timestamp: 1000 + i * 100,
            recipe_addr: CAddr::from_bytes(&format!("recipe{}", i).as_bytes()),
            output_addr: CAddr::from_bytes(&format!("output{}", i).as_bytes()),
            compute_node_id: NodeId(PublicKey::from_bytes(&[0u8; 32]).unwrap()),
            proof_id: ProofId([0u8; 32]),
        };
        audit_log.append(&entry).unwrap();
    }

    // Query range [1200, 1500]
    let entries = audit_log.query(1200, 1500).unwrap();
    assert_eq!(entries.len(), 4); // timestamps 1200, 1300, 1400, 1500
}
```

---

## 6. Edge Cases & Error Handling

### 6.1 Edge Cases

| Case | Behavior |
|------|----------|
| Proof for leaf | Not generated (leaves are trusted) |
| Proof for cached value | Retrieved from ProofStore, not regenerated |
| Proof eviction | Evicted with cached value, regenerated on re-materialization |
| Anchored proof with missing anchor | Verification fails with `UntrustedAnchor` |
| Signature verification failure | Verification fails with `InvalidSignature` |
| Tampered proof | Signature mismatch → verification fails |
| Clock skew | Timestamp is informational only, not verified |

### 6.2 Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProofError {
    #[error("invalid signature")]
    InvalidSignature,
    
    #[error("untrusted leaf: {0}")]
    UntrustedLeaf(CAddr),
    
    #[error("untrusted anchor: {0}")]
    UntrustedAnchor(CAddr),
    
    #[error("address mismatch: expected {expected}, got {actual}")]
    AddressMismatch {
        expected: CAddr,
        actual: CAddr,
    },
    
    #[error("proof not found: {0}")]
    ProofNotFound(CAddr),
    
    #[error("serialization failed: {0}")]
    SerializationFailed(String),
}
```

---

## 7. Performance Analysis

### 7.1 Proof Size

**Single-level proof:**
```
DerivationProof {
  recipe_addr: 32 bytes
  input_proofs: N * (32 + 32) bytes  (addr + hash per leaf)
  output_addr: 32 bytes
  output_hash: 32 bytes
  compute_node_id: 32 bytes
  timestamp: 8 bytes
  signature: 64 bytes
}

Total: ~200 + 64N bytes
```

**Multi-level proof (depth D, fanout F):**
```
Full recursive proof: O(F^D) proof objects
Anchored proof (anchor at depth A): O(F^(D-A)) proof objects
```

**Example:**
- Depth 5, fanout 2: 31 proof objects (~6 KB)
- Depth 5, fanout 2, anchor at depth 3: 7 proof objects (~1.4 KB)

### 7.2 Verification Performance

**Benchmark:**
```rust
// benches/proof_verification.rs

fn bench_proof_verification(c: &mut Criterion) {
    let node_identity = NodeIdentity::generate();
    
    // Generate proof for depth-5 DAG
    let proof = generate_test_proof(5, 2);  // depth 5, fanout 2
    
    let mut trusted_leaves = HashSet::new();
    for leaf in collect_leaves(&proof) {
        trusted_leaves.insert(leaf);
    }

    c.bench_function("verify_proof_depth5", |b| {
        b.iter(|| {
            black_box(proof.verify(&trusted_leaves).unwrap());
        });
    });
}
```

**Expected results:**
- Depth 1: ~50 μs (1 signature verification)
- Depth 5: ~250 μs (5 signature verifications)
- Depth 10: ~500 μs (10 signature verifications)

**Signature verification is the bottleneck** (~50 μs per signature with Ed25519).

### 7.3 Storage Overhead

**Proof storage:**
- 1 proof per cached value
- Average proof size: ~1 KB (depth 3, fanout 2)
- 1M cached values: ~1 GB proof storage

**Mitigation:**
- Anchor frequently-used intermediate results
- Evict proofs with cached values
- Compress proofs (gzip: ~50% reduction)

---

## 8. Files Changed

### New Files
- `crates/deriva-core/src/proof.rs` — `DerivationProof`, `InputProof`, `ProofId`
- `crates/deriva-server/src/node_identity.rs` — `NodeIdentity`, Ed25519 signing
- `crates/deriva-storage/src/proof_store.rs` — `ProofStore` (sled-based)
- `crates/deriva-storage/src/audit_log.rs` — `AuditLog` (append-only)
- `crates/deriva-core/tests/proof_verification.rs` — unit tests
- `crates/deriva-compute/tests/proof_integration.rs` — integration tests
- `benches/proof_verification.rs` — performance benchmarks

### Modified Files
- `crates/deriva-compute/src/executor.rs` — `materialize_with_proof()`
- `crates/deriva-server/src/service.proto` — `GetProof`, `VerifyProof` RPCs
- `crates/deriva-server/src/service.rs` — RPC handlers
- `crates/deriva-core/src/cache.rs` — `put_with_proof()`, `get_with_proof()`

---

## 9. Dependency Changes

```toml
# crates/deriva-core/Cargo.toml
[dependencies]
ed25519-dalek = "2.1"
rand = "0.8"
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"

# crates/deriva-storage/Cargo.toml
[dependencies]
sled = "0.34"
serde_json = "1.0"
```

---

## 10. Design Rationale

### 10.1 Why Ed25519?

| Algorithm | Signature Size | Verification Speed | Security |
|-----------|----------------|-------------------|----------|
| RSA-2048 | 256 bytes | Slow (~1 ms) | 112-bit |
| ECDSA P-256 | 64 bytes | Medium (~200 μs) | 128-bit |
| **Ed25519** | **64 bytes** | **Fast (~50 μs)** | **128-bit** |

Ed25519 is the best choice for proof signatures: small, fast, secure.

### 10.2 Why Recursive Proofs?

**Alternative: Flat proof (all leaves listed)**
```
Proof {
  recipe_addr,
  all_leaf_addrs: Vec<CAddr>,
  output_hash,
  signature,
}
```

**Problem:** Doesn't prove intermediate computations were correct. A malicious node could:
1. Compute intermediate results incorrectly
2. Use correct leaves + correct final recipe
3. Produce a valid flat proof for an invalid result

**Recursive proofs** prove every step of the computation, not just the final result.

### 10.3 Why Anchoring?

Deep DAGs produce large proofs. Anchoring allows:
1. **Proof compaction** — reference trusted intermediates instead of full recursion
2. **Incremental verification** — verify new computations without re-verifying old ones
3. **Proof caching** — store anchors, reuse across multiple proofs

---

## 11. Observability Integration

### 11.1 Metrics

```rust
lazy_static! {
    static ref PROOF_GENERATION_DURATION: Histogram = register_histogram!(
        "deriva_proof_generation_duration_seconds",
        "Time to generate a derivation proof"
    ).unwrap();

    static ref PROOF_VERIFICATION_DURATION: Histogram = register_histogram!(
        "deriva_proof_verification_duration_seconds",
        "Time to verify a derivation proof"
    ).unwrap();

    static ref PROOF_SIZE_BYTES: Histogram = register_histogram!(
        "deriva_proof_size_bytes",
        "Size of generated proofs in bytes"
    ).unwrap();

    static ref AUDIT_LOG_ENTRIES: IntCounter = register_int_counter!(
        "deriva_audit_log_entries_total",
        "Total number of audit log entries"
    ).unwrap();
}
```

### 11.2 Logs

```rust
impl Executor {
    pub async fn materialize_with_proof(&mut self, addr: CAddr) -> Result<(Bytes, DerivationProof), ExecutionError> {
        info!("Generating proof for {}", addr);
        let start = Instant::now();
        
        // ... generate proof ...
        
        let duration = start.elapsed();
        let proof_size = bincode::serialize(&proof).unwrap().len();
        
        info!(
            "Proof generated: addr={}, size={} bytes, duration={:?}",
            addr, proof_size, duration
        );
        
        PROOF_GENERATION_DURATION.observe(duration.as_secs_f64());
        PROOF_SIZE_BYTES.observe(proof_size as f64);
        
        Ok((output, proof))
    }
}
```

---

## 12. Checklist

### Implementation
- [ ] Create `deriva-core/src/proof.rs` with `DerivationProof`, `InputProof`
- [ ] Create `deriva-server/src/node_identity.rs` with Ed25519 signing
- [ ] Create `deriva-storage/src/proof_store.rs` with sled backend
- [ ] Create `deriva-storage/src/audit_log.rs` with append-only log
- [ ] Update `Executor::materialize_with_proof()` to generate proofs
- [ ] Add `GetProof` and `VerifyProof` RPCs
- [ ] Add proof anchoring support

### Testing
- [ ] Unit test: proof generation for leaf, single-level, multi-level
- [ ] Unit test: proof verification (valid, invalid signature, untrusted leaf)
- [ ] Integration test: end-to-end proof generation + verification
- [ ] Integration test: multi-level DAG proof
- [ ] Audit log test: append, query by time range
- [ ] Benchmark: proof verification performance by depth
- [ ] Benchmark: proof size by depth and fanout

### Documentation
- [ ] Document proof format in `docs/proof-format.md`
- [ ] Add examples of proof generation and verification
- [ ] Document anchoring strategy
- [ ] Add audit log query examples

### Observability
- [ ] Add `deriva_proof_generation_duration_seconds` metric
- [ ] Add `deriva_proof_verification_duration_seconds` metric
- [ ] Add `deriva_proof_size_bytes` histogram
- [ ] Add `deriva_audit_log_entries_total` counter
- [ ] Add info logs for proof generation

### Validation
- [ ] Run full test suite
- [ ] Benchmark proof verification (target: <500 μs for depth 10)
- [ ] Test proof storage (1M proofs, measure disk usage)
- [ ] Test audit log (1M entries, measure query performance)
- [ ] Verify signature verification with different key pairs

### Deployment
- [ ] Generate node identity keys for all nodes
- [ ] Deploy with proof generation enabled
- [ ] Monitor proof size and verification time
- [ ] Set up audit log rotation (daily)
- [ ] Document proof verification for clients

---

**Estimated effort:** 8–10 days
- Days 1-2: Core proof types + Ed25519 signing
- Days 3-4: Proof generation in executor
- Days 5-6: Proof storage + audit log
- Days 7-8: RPC integration + tests
- Days 9-10: Benchmarks + documentation

**Success criteria:**
1. All tests pass (unit + integration)
2. Proof verification <500 μs for depth 10
3. Proof size <10 KB for depth 10, fanout 2
4. Audit log supports 1M entries with <1s query time
5. Signature verification works with multiple node identities
