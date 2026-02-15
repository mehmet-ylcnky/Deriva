# Phase 4, Section 4.9: Mutable References

**Status:** Blueprint  
**Depends on:** §4.1 (Canonical Serialization), §4.3 (Reproducibility Proofs), §4.5 (Content Integrity)  
**Blocks:** §4.10 (REST)

---

## 1. Problem Statement

### 1.1 Current State

Deriva is **purely immutable** — all content is content-addressed:

```rust
// Current approach: immutable only
let addr = CAddr::from_bytes(&bytes);
dag.put(addr, bytes).await?;

// CAddr never changes — content is immutable
```

**Problems:**
1. **No mutable state**: Can't represent evolving data (databases, logs, configs)
2. **No named references**: Must track CAddrs manually
3. **No versioning**: Can't track history of changes
4. **No atomic updates**: Can't update multiple refs atomically
5. **No garbage collection**: Can't identify unreachable content

### 1.2 The Problem

**Scenario 1: Configuration File**
```rust
// User wants mutable config that evolves over time
let config_v1 = CAddr::from_hex("abc123...");
// ❌ No way to update "config" to point to new version
// ❌ Must manually track latest CAddr
```

**Scenario 2: Database Snapshot**
```rust
// Database takes periodic snapshots
let snapshot_t1 = CAddr::from_hex("def456...");
let snapshot_t2 = CAddr::from_hex("789abc...");
// ❌ No way to name "latest_snapshot"
// ❌ No history of previous snapshots
```

**Scenario 3: Build Artifacts**
```rust
// CI/CD wants to publish "latest" build
let build_123 = CAddr::from_hex("aaa111...");
let build_124 = CAddr::from_hex("bbb222...");
// ❌ No atomic update of "latest" pointer
// ❌ Race conditions if multiple builds finish simultaneously
```

### 1.3 Requirements

1. **Named references**: Human-readable names for CAddrs
2. **Atomic updates**: Compare-and-swap for concurrent updates
3. **Version history**: Track all previous values
4. **Immutable snapshots**: Each version is immutable (§4.3 proofs)
5. **Garbage collection**: Identify unreachable content
6. **Namespace isolation**: Per-user or per-project refs
7. **Performance**: <1ms for ref reads, <10ms for updates

---

## 2. Design

### 2.1 Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                   Mutable Reference Layer                    │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   RefStore   │  │  RefHistory  │  │   RefLock    │      │
│  │              │  │              │  │              │      │
│  │ • get        │  │ • Versions   │  │ • CAS        │      │
│  │ • set        │  │ • Timestamps │  │ • Optimistic │      │
│  │ • cas        │  │ • Audit log  │  │ • Retry      │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                               │
├─────────────────────────────────────────────────────────────┤
│                      DagStore (§2.3)                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ VerifiedGet  │  │  BlobStore   │  │  RecipeStore │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Core Types

```rust
// crates/deriva-storage/src/ref_store.rs

/// Mutable reference name
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RefName {
    /// Namespace (e.g., "user:alice", "project:myapp")
    pub namespace: String,
    /// Reference name (e.g., "config", "latest", "main")
    pub name: String,
}

impl RefName {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }
    
    pub fn to_string(&self) -> String {
        format!("{}:{}", self.namespace, self.name)
    }
}

/// Reference value with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefValue {
    /// Current CAddr
    pub addr: CAddr,
    /// Version number (monotonically increasing)
    pub version: u64,
    /// Timestamp of last update
    pub updated_at: SystemTime,
    /// Optional metadata
    pub metadata: BTreeMap<String, String>,
}

/// Reference history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefHistoryEntry {
    pub addr: CAddr,
    pub version: u64,
    pub updated_at: SystemTime,
    pub updated_by: Option<String>,
}

/// Mutable reference store
pub struct RefStore {
    refs: Arc<Mutex<HashMap<RefName, RefValue>>>,
    history: Arc<Mutex<HashMap<RefName, Vec<RefHistoryEntry>>>>,
}

impl RefStore {
    pub fn new() -> Self {
        Self {
            refs: Arc::new(Mutex::new(HashMap::new())),
            history: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Get current value of reference
    pub fn get(&self, name: &RefName) -> Option<RefValue> {
        let refs = self.refs.lock().unwrap();
        refs.get(name).cloned()
    }
    
    /// Set reference (unconditional)
    pub fn set(&self, name: RefName, addr: CAddr) -> RefValue {
        let mut refs = self.refs.lock().unwrap();
        let mut history = self.history.lock().unwrap();
        
        let version = refs.get(&name)
            .map(|v| v.version + 1)
            .unwrap_or(1);
        
        let value = RefValue {
            addr,
            version,
            updated_at: SystemTime::now(),
            metadata: BTreeMap::new(),
        };
        
        // Add to history
        history.entry(name.clone())
            .or_insert_with(Vec::new)
            .push(RefHistoryEntry {
                addr,
                version,
                updated_at: value.updated_at,
                updated_by: None,
            });
        
        refs.insert(name, value.clone());
        value
    }
    
    /// Compare-and-swap (atomic update)
    pub fn cas(
        &self,
        name: RefName,
        expected_version: u64,
        new_addr: CAddr,
    ) -> Result<RefValue, CasError> {
        let mut refs = self.refs.lock().unwrap();
        let mut history = self.history.lock().unwrap();
        
        // Check current version
        let current = refs.get(&name);
        
        match current {
            Some(current) if current.version != expected_version => {
                return Err(CasError::VersionMismatch {
                    expected: expected_version,
                    actual: current.version,
                });
            }
            None if expected_version != 0 => {
                return Err(CasError::NotFound);
            }
            _ => {}
        }
        
        let version = expected_version + 1;
        
        let value = RefValue {
            addr: new_addr,
            version,
            updated_at: SystemTime::now(),
            metadata: BTreeMap::new(),
        };
        
        // Add to history
        history.entry(name.clone())
            .or_insert_with(Vec::new)
            .push(RefHistoryEntry {
                addr: new_addr,
                version,
                updated_at: value.updated_at,
                updated_by: None,
            });
        
        refs.insert(name, value.clone());
        Ok(value)
    }
    
    /// Get history of reference
    pub fn history(&self, name: &RefName) -> Vec<RefHistoryEntry> {
        let history = self.history.lock().unwrap();
        history.get(name).cloned().unwrap_or_default()
    }
    
    /// List all references in namespace
    pub fn list(&self, namespace: &str) -> Vec<(RefName, RefValue)> {
        let refs = self.refs.lock().unwrap();
        refs.iter()
            .filter(|(name, _)| name.namespace == namespace)
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    }
    
    /// Delete reference
    pub fn delete(&self, name: &RefName) -> Option<RefValue> {
        let mut refs = self.refs.lock().unwrap();
        refs.remove(name)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CasError {
    #[error("Version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u64, actual: u64 },
    
    #[error("Reference not found")]
    NotFound,
}
```

### 2.3 Persistent Storage

```rust
// crates/deriva-storage/src/ref_store.rs

impl RefStore {
    /// Save to persistent storage
    pub async fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let refs = self.refs.lock().unwrap().clone();
        let history = self.history.lock().unwrap().clone();
        
        let data = RefStoreData { refs, history };
        let bytes = bincode::serialize(&data)?;
        
        tokio::fs::write(path, bytes).await?;
        Ok(())
    }
    
    /// Load from persistent storage
    pub async fn load(path: &Path) -> Result<Self, std::io::Error> {
        let bytes = tokio::fs::read(path).await?;
        let data: RefStoreData = bincode::deserialize(&bytes)?;
        
        Ok(Self {
            refs: Arc::new(Mutex::new(data.refs)),
            history: Arc::new(Mutex::new(data.history)),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct RefStoreData {
    refs: HashMap<RefName, RefValue>,
    history: HashMap<RefName, Vec<RefHistoryEntry>>,
}
```

### 2.4 Retry Logic

```rust
// crates/deriva-storage/src/ref_store.rs

impl RefStore {
    /// Update with retry on conflict
    pub async fn update_with_retry<F>(
        &self,
        name: RefName,
        max_retries: usize,
        update_fn: F,
    ) -> Result<RefValue, CasError>
    where
        F: Fn(Option<CAddr>) -> CAddr,
    {
        for attempt in 0..max_retries {
            let current = self.get(&name);
            let expected_version = current.as_ref().map(|v| v.version).unwrap_or(0);
            let current_addr = current.map(|v| v.addr);
            
            let new_addr = update_fn(current_addr);
            
            match self.cas(name.clone(), expected_version, new_addr) {
                Ok(value) => return Ok(value),
                Err(CasError::VersionMismatch { .. }) if attempt < max_retries - 1 => {
                    // Retry
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        
        Err(CasError::VersionMismatch {
            expected: 0,
            actual: 0,
        })
    }
}
```

---

## 3. Implementation

### 3.1 DagStore Integration

```rust
// crates/deriva-storage/src/dag_store.rs

impl DagStore {
    /// Get by reference name
    pub async fn get_ref(&self, name: &RefName) -> Option<Bytes> {
        let ref_value = self.ref_store.get(name)?;
        self.get(ref_value.addr).await
    }
    
    /// Set reference
    pub fn set_ref(&self, name: RefName, addr: CAddr) -> RefValue {
        self.ref_store.set(name, addr)
    }
    
    /// Update reference with CAS
    pub fn cas_ref(
        &self,
        name: RefName,
        expected_version: u64,
        new_addr: CAddr,
    ) -> Result<RefValue, CasError> {
        self.ref_store.cas(name, expected_version, new_addr)
    }
}
```

### 3.2 REST API

```rust
// crates/deriva-server/src/rest.rs

#[derive(Deserialize)]
struct GetRefRequest {
    namespace: String,
    name: String,
}

#[derive(Serialize)]
struct GetRefResponse {
    addr: String,
    version: u64,
    updated_at: String,
}

async fn get_ref(
    State(state): State<AppState>,
    Json(req): Json<GetRefRequest>,
) -> Result<Json<GetRefResponse>, StatusCode> {
    let ref_name = RefName::new(req.namespace, req.name);
    
    let value = state.ref_store.get(&ref_name)
        .ok_or(StatusCode::NOT_FOUND)?;
    
    Ok(Json(GetRefResponse {
        addr: value.addr.to_hex(),
        version: value.version,
        updated_at: format!("{:?}", value.updated_at),
    }))
}

#[derive(Deserialize)]
struct SetRefRequest {
    namespace: String,
    name: String,
    addr: String,
}

async fn set_ref(
    State(state): State<AppState>,
    Json(req): Json<SetRefRequest>,
) -> Result<Json<GetRefResponse>, StatusCode> {
    let ref_name = RefName::new(req.namespace, req.name);
    let addr = CAddr::from_hex(&req.addr)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    let value = state.ref_store.set(ref_name, addr);
    
    Ok(Json(GetRefResponse {
        addr: value.addr.to_hex(),
        version: value.version,
        updated_at: format!("{:?}", value.updated_at),
    }))
}

#[derive(Deserialize)]
struct CasRefRequest {
    namespace: String,
    name: String,
    expected_version: u64,
    new_addr: String,
}

async fn cas_ref(
    State(state): State<AppState>,
    Json(req): Json<CasRefRequest>,
) -> Result<Json<GetRefResponse>, StatusCode> {
    let ref_name = RefName::new(req.namespace, req.name);
    let new_addr = CAddr::from_hex(&req.new_addr)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    let value = state.ref_store.cas(ref_name, req.expected_version, new_addr)
        .map_err(|_| StatusCode::CONFLICT)?;
    
    Ok(Json(GetRefResponse {
        addr: value.addr.to_hex(),
        version: value.version,
        updated_at: format!("{:?}", value.updated_at),
    }))
}

#[derive(Deserialize)]
struct ListRefsRequest {
    namespace: String,
}

#[derive(Serialize)]
struct ListRefsResponse {
    refs: Vec<RefEntry>,
}

#[derive(Serialize)]
struct RefEntry {
    name: String,
    addr: String,
    version: u64,
}

async fn list_refs(
    State(state): State<AppState>,
    Json(req): Json<ListRefsRequest>,
) -> Json<ListRefsResponse> {
    let refs = state.ref_store.list(&req.namespace);
    
    let entries = refs.into_iter()
        .map(|(name, value)| RefEntry {
            name: name.name,
            addr: value.addr.to_hex(),
            version: value.version,
        })
        .collect();
    
    Json(ListRefsResponse { refs: entries })
}
```

### 3.3 CLI Commands

```rust
// crates/deriva-cli/src/main.rs

#[derive(Subcommand)]
enum RefCommand {
    /// Get reference value
    Get {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        name: String,
    },
    /// Set reference value
    Set {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        addr: String,
    },
    /// Compare-and-swap reference
    Cas {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        expected_version: u64,
        #[arg(short, long)]
        new_addr: String,
    },
    /// List references in namespace
    List {
        #[arg(short, long)]
        namespace: String,
    },
    /// Show reference history
    History {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        name: String,
    },
}

async fn handle_ref_command(cmd: RefCommand, client: &DerivaClient) -> Result<()> {
    match cmd {
        RefCommand::Get { namespace, name } => {
            let ref_name = RefName::new(namespace, name);
            let value = client.get_ref(&ref_name).await?;
            println!("addr: {}", value.addr.to_hex());
            println!("version: {}", value.version);
            println!("updated_at: {:?}", value.updated_at);
        }
        RefCommand::Set { namespace, name, addr } => {
            let ref_name = RefName::new(namespace, name);
            let addr = CAddr::from_hex(&addr)?;
            let value = client.set_ref(ref_name, addr).await?;
            println!("Set to version {}", value.version);
        }
        RefCommand::Cas { namespace, name, expected_version, new_addr } => {
            let ref_name = RefName::new(namespace, name);
            let new_addr = CAddr::from_hex(&new_addr)?;
            match client.cas_ref(ref_name, expected_version, new_addr).await {
                Ok(value) => println!("Updated to version {}", value.version),
                Err(e) => println!("CAS failed: {}", e),
            }
        }
        RefCommand::List { namespace } => {
            let refs = client.list_refs(&namespace).await?;
            for (name, value) in refs {
                println!("{}: {} (v{})", name.name, value.addr.to_hex(), value.version);
            }
        }
        RefCommand::History { namespace, name } => {
            let ref_name = RefName::new(namespace, name);
            let history = client.ref_history(&ref_name).await?;
            for entry in history {
                println!("v{}: {} at {:?}", entry.version, entry.addr.to_hex(), entry.updated_at);
            }
        }
    }
    Ok(())
}
```

---

## 4. Data Flow Diagrams

### 4.1 Set Reference Flow

```
┌──────┐                                                      ┌──────────┐
│Client│                                                      │ RefStore │
└──┬───┘                                                      └────┬─────┘
   │                                                                │
   │ set_ref("config", addr_v2)                                    │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                          Lock refs             │
   │                                          ─────────────         │
   │                                                                │
   │                                          Get current version   │
   │                                          ─────────────         │
   │                                                                │
   │                                          version = 1 + 1 = 2   │
   │                                          ─────────────         │
   │                                                                │
   │                                          Add to history        │
   │                                          ─────────────         │
   │                                                                │
   │                                          Update ref            │
   │                                          ─────────────         │
   │                                                                │
   │                                          Unlock refs           │
   │                                          ─────────────         │
   │                                                                │
   │ RefValue { addr: addr_v2, version: 2 }                        │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

### 4.2 CAS Flow (Success)

```
┌──────┐                                                      ┌──────────┐
│Client│                                                      │ RefStore │
└──┬───┘                                                      └────┬─────┘
   │                                                                │
   │ cas_ref("config", expected_v=2, addr_v3)                      │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                          Lock refs             │
   │                                          ─────────────         │
   │                                                                │
   │                                          Get current version   │
   │                                          ─────────────         │
   │                                                                │
   │                                          current.version = 2   │
   │                                          ─────────────         │
   │                                                                │
   │                                          expected = 2?         │
   │                                          ─────────────         │
   │                                                                │
   │                                          ✓ Match               │
   │                                          ─────────────         │
   │                                                                │
   │                                          Update to version 3   │
   │                                          ─────────────         │
   │                                                                │
   │ RefValue { addr: addr_v3, version: 3 }                        │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

### 4.3 CAS Flow (Conflict)

```
┌──────┐                                                      ┌──────────┐
│Client│                                                      │ RefStore │
└──┬───┘                                                      └────┬─────┘
   │                                                                │
   │ cas_ref("config", expected_v=2, addr_v3)                      │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                          Lock refs             │
   │                                          ─────────────         │
   │                                                                │
   │                                          Get current version   │
   │                                          ─────────────         │
   │                                                                │
   │                                          current.version = 3   │
   │                                          ─────────────         │
   │                                                                │
   │                                          expected = 2?         │
   │                                          ─────────────         │
   │                                                                │
   │                                          ✗ Mismatch            │
   │                                          ─────────────         │
   │                                                                │
   │ CasError::VersionMismatch { expected: 2, actual: 3 }          │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

---

## 5. Test Specification

### 5.1 Unit Tests

```rust
// crates/deriva-storage/tests/ref_store.rs

#[test]
fn test_set_ref() {
    let store = RefStore::new();
    let name = RefName::new("test", "config");
    let addr = CAddr::from_hex("abc123...").unwrap();
    
    let value = store.set(name.clone(), addr);
    
    assert_eq!(value.addr, addr);
    assert_eq!(value.version, 1);
    
    // Get should return same value
    let retrieved = store.get(&name).unwrap();
    assert_eq!(retrieved.addr, addr);
    assert_eq!(retrieved.version, 1);
}

#[test]
fn test_set_ref_increments_version() {
    let store = RefStore::new();
    let name = RefName::new("test", "config");
    
    let addr1 = CAddr::from_hex("aaa111...").unwrap();
    let value1 = store.set(name.clone(), addr1);
    assert_eq!(value1.version, 1);
    
    let addr2 = CAddr::from_hex("bbb222...").unwrap();
    let value2 = store.set(name.clone(), addr2);
    assert_eq!(value2.version, 2);
}

#[test]
fn test_cas_success() {
    let store = RefStore::new();
    let name = RefName::new("test", "config");
    
    let addr1 = CAddr::from_hex("aaa111...").unwrap();
    store.set(name.clone(), addr1);
    
    let addr2 = CAddr::from_hex("bbb222...").unwrap();
    let result = store.cas(name.clone(), 1, addr2);
    
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value.addr, addr2);
    assert_eq!(value.version, 2);
}

#[test]
fn test_cas_version_mismatch() {
    let store = RefStore::new();
    let name = RefName::new("test", "config");
    
    let addr1 = CAddr::from_hex("aaa111...").unwrap();
    store.set(name.clone(), addr1);
    
    let addr2 = CAddr::from_hex("bbb222...").unwrap();
    let result = store.cas(name.clone(), 999, addr2);
    
    assert!(matches!(result, Err(CasError::VersionMismatch { expected: 999, actual: 1 })));
}

#[test]
fn test_cas_not_found() {
    let store = RefStore::new();
    let name = RefName::new("test", "nonexistent");
    
    let addr = CAddr::from_hex("aaa111...").unwrap();
    let result = store.cas(name, 1, addr);
    
    assert!(matches!(result, Err(CasError::NotFound)));
}

#[test]
fn test_cas_create_new() {
    let store = RefStore::new();
    let name = RefName::new("test", "new");
    
    let addr = CAddr::from_hex("aaa111...").unwrap();
    let result = store.cas(name.clone(), 0, addr);
    
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value.version, 1);
}

#[test]
fn test_history() {
    let store = RefStore::new();
    let name = RefName::new("test", "config");
    
    let addr1 = CAddr::from_hex("aaa111...").unwrap();
    store.set(name.clone(), addr1);
    
    let addr2 = CAddr::from_hex("bbb222...").unwrap();
    store.set(name.clone(), addr2);
    
    let addr3 = CAddr::from_hex("ccc333...").unwrap();
    store.set(name.clone(), addr3);
    
    let history = store.history(&name);
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].version, 1);
    assert_eq!(history[1].version, 2);
    assert_eq!(history[2].version, 3);
}

#[test]
fn test_list_refs() {
    let store = RefStore::new();
    
    store.set(RefName::new("ns1", "ref1"), CAddr::from_hex("aaa111...").unwrap());
    store.set(RefName::new("ns1", "ref2"), CAddr::from_hex("bbb222...").unwrap());
    store.set(RefName::new("ns2", "ref3"), CAddr::from_hex("ccc333...").unwrap());
    
    let ns1_refs = store.list("ns1");
    assert_eq!(ns1_refs.len(), 2);
    
    let ns2_refs = store.list("ns2");
    assert_eq!(ns2_refs.len(), 1);
}

#[test]
fn test_delete_ref() {
    let store = RefStore::new();
    let name = RefName::new("test", "config");
    let addr = CAddr::from_hex("abc123...").unwrap();
    
    store.set(name.clone(), addr);
    
    let deleted = store.delete(&name);
    assert!(deleted.is_some());
    
    let retrieved = store.get(&name);
    assert!(retrieved.is_none());
}
```

### 5.2 Concurrency Tests

```rust
// crates/deriva-storage/tests/ref_concurrency.rs

#[tokio::test]
async fn test_concurrent_cas() {
    let store = Arc::new(RefStore::new());
    let name = RefName::new("test", "counter");
    
    // Initialize
    store.set(name.clone(), CAddr::from_hex("000000...").unwrap());
    
    // Spawn 10 concurrent CAS operations
    let mut handles = vec![];
    for i in 0..10 {
        let store = store.clone();
        let name = name.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..10 {
                loop {
                    let current = store.get(&name).unwrap();
                    let new_addr = CAddr::from_hex(&format!("{:06x}...", i)).unwrap();
                    
                    match store.cas(name.clone(), current.version, new_addr) {
                        Ok(_) => break,
                        Err(CasError::VersionMismatch { .. }) => {
                            // Retry
                            tokio::time::sleep(Duration::from_millis(1)).await;
                            continue;
                        }
                        Err(e) => panic!("Unexpected error: {}", e),
                    }
                }
            }
        });
        handles.push(handle);
    }
    
    // Wait for all
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Final version should be 101 (1 initial + 100 updates)
    let final_value = store.get(&name).unwrap();
    assert_eq!(final_value.version, 101);
}

#[tokio::test]
async fn test_update_with_retry() {
    let store = Arc::new(RefStore::new());
    let name = RefName::new("test", "config");
    
    // Initialize
    let addr1 = CAddr::from_hex("aaa111...").unwrap();
    store.set(name.clone(), addr1);
    
    // Update with retry
    let addr2 = CAddr::from_hex("bbb222...").unwrap();
    let result = store.update_with_retry(name.clone(), 10, |_| addr2).await;
    
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value.addr, addr2);
}
```

### 5.3 Persistence Tests

```rust
// crates/deriva-storage/tests/ref_persistence.rs

#[tokio::test]
async fn test_save_and_load() {
    let store = RefStore::new();
    let name = RefName::new("test", "config");
    let addr = CAddr::from_hex("abc123...").unwrap();
    
    store.set(name.clone(), addr);
    
    // Save
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    store.save(temp_file.path()).await.unwrap();
    
    // Load
    let loaded = RefStore::load(temp_file.path()).await.unwrap();
    
    let value = loaded.get(&name).unwrap();
    assert_eq!(value.addr, addr);
    assert_eq!(value.version, 1);
}

#[tokio::test]
async fn test_save_preserves_history() {
    let store = RefStore::new();
    let name = RefName::new("test", "config");
    
    store.set(name.clone(), CAddr::from_hex("aaa111...").unwrap());
    store.set(name.clone(), CAddr::from_hex("bbb222...").unwrap());
    store.set(name.clone(), CAddr::from_hex("ccc333...").unwrap());
    
    // Save
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    store.save(temp_file.path()).await.unwrap();
    
    // Load
    let loaded = RefStore::load(temp_file.path()).await.unwrap();
    
    let history = loaded.history(&name);
    assert_eq!(history.len(), 3);
}
```

---

## 6. Edge Cases & Error Handling

### 6.1 Concurrent Updates

**Handled by CAS:** Version mismatch returns error, client retries.

### 6.2 Reference Not Found

```rust
#[test]
fn test_get_nonexistent() {
    let store = RefStore::new();
    let name = RefName::new("test", "nonexistent");
    
    let result = store.get(&name);
    assert!(result.is_none());
}
```

### 6.3 Empty Namespace

```rust
#[test]
fn test_list_empty_namespace() {
    let store = RefStore::new();
    let refs = store.list("empty");
    assert_eq!(refs.len(), 0);
}
```

---

## 7. Performance Analysis

### 7.1 Operation Latency

**get():** ~100 ns (in-memory hash lookup)
**set():** ~1 µs (lock + insert + history)
**cas():** ~1 µs (lock + check + insert)
**list():** ~10 µs per 1000 refs (filter + clone)

### 7.2 Memory Usage

**Per reference:** ~200 bytes (RefValue + history entry)
**1000 refs:** ~200 KB
**1M refs:** ~200 MB

### 7.3 Concurrency

**CAS throughput:** ~1M ops/sec (single ref)
**Parallel CAS:** Scales linearly with number of refs

---

## 8. Files Changed

### New Files
- `crates/deriva-storage/src/ref_store.rs` — Reference store implementation
- `crates/deriva-storage/tests/ref_store.rs` — Unit tests
- `crates/deriva-storage/tests/ref_concurrency.rs` — Concurrency tests
- `crates/deriva-storage/tests/ref_persistence.rs` — Persistence tests
- `crates/deriva-cli/src/ref_commands.rs` — CLI commands

### Modified Files
- `crates/deriva-storage/src/dag_store.rs` — Add ref methods
- `crates/deriva-storage/src/lib.rs` — Export ref types
- `crates/deriva-server/src/rest.rs` — Add ref endpoints
- `crates/deriva-cli/src/main.rs` — Add ref subcommands

---

## 9. Dependency Changes

```toml
# crates/deriva-storage/Cargo.toml
[dependencies]
tokio = { version = "1.35", features = ["fs"] }
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"
```

No new dependencies.

---

## 10. Design Rationale

### 10.1 Why CAS Instead of Locks?

**Alternative:** Distributed locks.

**Problem:** Locks don't compose, deadlock risk.

**Decision:** CAS is lock-free and composable.

### 10.2 Why Version Numbers?

**Alternative:** Timestamps.

**Problem:** Clock skew, non-monotonic.

**Decision:** Version numbers are monotonic and deterministic.

### 10.3 Why Namespaces?

**Alternative:** Global flat namespace.

**Problem:** Name collisions, no isolation.

**Decision:** Namespaces provide isolation and organization.

### 10.4 Why Keep History?

**Alternative:** Only store current value.

**Problem:** Can't audit changes, no rollback.

**Decision:** History enables auditing and rollback.

---

## 11. Observability Integration

### 11.1 Metrics

```rust
lazy_static! {
    static ref REF_OPERATIONS: IntCounterVec = register_int_counter_vec!(
        "deriva_ref_operations_total",
        "Reference operations by type and result",
        &["operation", "result"]
    ).unwrap();

    static ref REF_CAS_CONFLICTS: IntCounter = register_int_counter!(
        "deriva_ref_cas_conflicts_total",
        "CAS conflicts (version mismatches)"
    ).unwrap();
    
    static ref REF_COUNT: IntGaugeVec = register_int_gauge_vec!(
        "deriva_ref_count",
        "Number of references by namespace",
        &["namespace"]
    ).unwrap();
}
```

### 11.2 Logs

```rust
impl RefStore {
    pub fn cas(
        &self,
        name: RefName,
        expected_version: u64,
        new_addr: CAddr,
    ) -> Result<RefValue, CasError> {
        debug!("CAS: {} v{} -> {}", name.to_string(), expected_version, new_addr.to_hex());
        
        // ... CAS logic ...
        
        match result {
            Ok(value) => {
                info!("CAS success: {} v{}", name.to_string(), value.version);
                REF_OPERATIONS.with_label_values(&["cas", "success"]).inc();
            }
            Err(CasError::VersionMismatch { expected, actual }) => {
                warn!("CAS conflict: {} expected v{}, got v{}", name.to_string(), expected, actual);
                REF_CAS_CONFLICTS.inc();
                REF_OPERATIONS.with_label_values(&["cas", "conflict"]).inc();
            }
            Err(_) => {
                REF_OPERATIONS.with_label_values(&["cas", "error"]).inc();
            }
        }
        
        result
    }
}
```

---

## 12. Checklist

### Implementation
- [ ] Create `deriva-storage/src/ref_store.rs` with RefStore type
- [ ] Implement get(), set(), cas() operations
- [ ] Implement history tracking
- [ ] Implement list() and delete()
- [ ] Add update_with_retry() helper
- [ ] Implement save() and load() for persistence
- [ ] Add ref methods to DagStore
- [ ] Add REST API endpoints (get, set, cas, list, history)
- [ ] Add CLI commands (get, set, cas, list, history)

### Testing
- [ ] Unit test: set reference
- [ ] Unit test: set increments version
- [ ] Unit test: CAS success
- [ ] Unit test: CAS version mismatch
- [ ] Unit test: CAS not found
- [ ] Unit test: CAS create new
- [ ] Unit test: history tracking
- [ ] Unit test: list references
- [ ] Unit test: delete reference
- [ ] Concurrency test: concurrent CAS
- [ ] Concurrency test: update with retry
- [ ] Persistence test: save and load
- [ ] Persistence test: history preserved
- [ ] Benchmark: get latency (<1 µs)
- [ ] Benchmark: CAS throughput (>1M ops/sec)

### Documentation
- [ ] Document reference naming conventions
- [ ] Add examples of CAS usage
- [ ] Document retry strategies
- [ ] Add troubleshooting guide for conflicts
- [ ] Document namespace isolation
- [ ] Add garbage collection guidance

### Observability
- [ ] Add `deriva_ref_operations_total` counter
- [ ] Add `deriva_ref_cas_conflicts_total` counter
- [ ] Add `deriva_ref_count` gauge
- [ ] Add debug logs for CAS operations
- [ ] Add warning logs for conflicts

### Validation
- [ ] Test concurrent CAS (10 threads, 100 ops each)
- [ ] Verify version monotonicity
- [ ] Test persistence (save/load)
- [ ] Verify history tracking
- [ ] Test namespace isolation
- [ ] Benchmark CAS throughput (target: >1M ops/sec)

### Deployment
- [ ] Deploy with ref store enabled
- [ ] Monitor CAS conflict rate
- [ ] Set up periodic persistence (every 5 minutes)
- [ ] Document ref naming conventions
- [ ] Add admin API to query ref statistics

---

**Estimated effort:** 4–6 days
- Days 1-2: Core RefStore (get, set, cas, history)
- Day 3: Persistence + retry logic
- Day 4: REST API + CLI
- Days 5-6: Tests + benchmarks + documentation

**Success criteria:**
1. All tests pass (set, cas, history, concurrency)
2. CAS latency <1 µs
3. CAS throughput >1M ops/sec (single ref)
4. Concurrent CAS works correctly (no lost updates)
5. Persistence preserves refs and history
6. REST API and CLI work correctly
7. Namespace isolation works
