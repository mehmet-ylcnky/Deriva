# Phase 4, Section 4.7: FUSE Filesystem Mount

**Status:** Blueprint  
**Depends on:** §4.1 (Canonical Serialization), §4.5 (Content Integrity), §4.6 (WASM)  
**Blocks:** §4.10 (REST)

---

## 1. Problem Statement

### 1.1 Current State

Deriva is accessed via **programmatic API** (Rust library, gRPC, REST):

```rust
// Current approach: programmatic access
let dag = DagStore::new(...);
let bytes = dag.get(addr).await?;

// Users must write code to interact with Deriva
```

**Problems:**
1. **No filesystem interface**: Can't use standard tools (ls, cat, grep)
2. **No POSIX compatibility**: Can't mount as a directory
3. **No lazy loading**: Must download entire DAG to access
4. **No streaming**: Can't read large files incrementally
5. **No integration**: Can't use with existing tools (editors, compilers)

### 1.2 The Problem

**Scenario 1: User Wants to Browse DAG**
```bash
# User wants to explore DAG structure
# ❌ Must write custom code to traverse DAG
# ❌ Can't use standard tools (ls, tree, find)
```

**Scenario 2: User Wants to Read File**
```bash
# User wants to read a file stored in Deriva
# ❌ Must use Deriva API
# ❌ Can't use standard tools (cat, less, vim)
```

**Scenario 3: User Wants to Stream Large File**
```bash
# User wants to stream a 10 GB video
# ❌ Must download entire file before playback
# ❌ Can't seek to specific position
```

### 1.3 Requirements

1. **FUSE mount**: Mount Deriva as a POSIX filesystem
2. **Lazy loading**: Only fetch content when accessed
3. **Streaming**: Support partial reads and seeking
4. **Read-only**: No write operations (content-addressed = immutable)
5. **DAG navigation**: Browse DAG structure as directories
6. **Standard tools**: Work with ls, cat, grep, vim, etc.
7. **Performance**: <10ms latency for metadata, <100ms for small files

---

## 2. Design

### 2.1 Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     FUSE Filesystem Layer                    │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ DerivaFS     │  │ InodeCache   │  │ FileHandle   │      │
│  │              │  │              │  │              │      │
│  │ • lookup     │  │ • CAddr→ino  │  │ • Streaming  │      │
│  │ • read       │  │ • Metadata   │  │ • Seeking    │      │
│  │ • readdir    │  │ • LRU evict  │  │ • Buffering  │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                               │
├─────────────────────────────────────────────────────────────┤
│                      DagStore (§2.3)                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ VerifiedGet  │  │  BlobStore   │  │  ChunkStore  │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Filesystem Structure

```
/mnt/deriva/
├── by-addr/
│   ├── abc123.../          # Recipe (directory)
│   │   ├── inputs/
│   │   │   ├── 0 -> ../../def456.../
│   │   │   └── 1 -> ../../789abc.../
│   │   ├── function_id     # File containing function ID
│   │   ├── params.json     # File containing params
│   │   └── output          # File containing computed result
│   └── def456.../          # Leaf value (file)
└── by-name/
    └── my-computation -> ../by-addr/abc123.../
```

### 2.3 Core Types

```rust
// crates/deriva-fuse/src/fs.rs

use fuser::{FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use std::time::{Duration, SystemTime};

/// Deriva FUSE filesystem
pub struct DerivaFS {
    dag: Arc<DagStore>,
    inode_cache: Arc<Mutex<InodeCache>>,
    file_handles: Arc<Mutex<HashMap<u64, FileHandle>>>,
    next_fh: AtomicU64,
}

/// Inode cache (CAddr <-> inode mapping)
struct InodeCache {
    addr_to_ino: HashMap<CAddr, u64>,
    ino_to_addr: HashMap<u64, CAddr>,
    metadata: HashMap<u64, FileAttr>,
    next_ino: u64,
}

impl InodeCache {
    const ROOT_INO: u64 = 1;
    const BY_ADDR_INO: u64 = 2;
    const BY_NAME_INO: u64 = 3;
    
    fn new() -> Self {
        let mut cache = Self {
            addr_to_ino: HashMap::new(),
            ino_to_addr: HashMap::new(),
            metadata: HashMap::new(),
            next_ino: 4,  // Start after reserved inodes
        };
        
        // Initialize root directory
        cache.metadata.insert(Self::ROOT_INO, FileAttr {
            ino: Self::ROOT_INO,
            size: 0,
            blocks: 0,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: FileType::Directory,
            perm: 0o555,  // r-xr-xr-x
            nlink: 2,
            uid: 1000,
            gid: 1000,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        });
        
        // Initialize by-addr directory
        cache.metadata.insert(Self::BY_ADDR_INO, FileAttr {
            ino: Self::BY_ADDR_INO,
            size: 0,
            blocks: 0,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: FileType::Directory,
            perm: 0o555,
            nlink: 2,
            uid: 1000,
            gid: 1000,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        });
        
        cache
    }
    
    fn get_or_create(&mut self, addr: CAddr, kind: FileType, size: u64) -> u64 {
        if let Some(&ino) = self.addr_to_ino.get(&addr) {
            return ino;
        }
        
        let ino = self.next_ino;
        self.next_ino += 1;
        
        self.addr_to_ino.insert(addr, ino);
        self.ino_to_addr.insert(ino, addr);
        
        self.metadata.insert(ino, FileAttr {
            ino,
            size,
            blocks: (size + 4095) / 4096,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind,
            perm: if kind == FileType::Directory { 0o555 } else { 0o444 },
            nlink: if kind == FileType::Directory { 2 } else { 1 },
            uid: 1000,
            gid: 1000,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        });
        
        ino
    }
}

/// File handle for streaming reads
struct FileHandle {
    addr: CAddr,
    offset: u64,
    buffer: Vec<u8>,
}

impl DerivaFS {
    pub fn new(dag: Arc<DagStore>) -> Self {
        Self {
            dag,
            inode_cache: Arc::new(Mutex::new(InodeCache::new())),
            file_handles: Arc::new(Mutex::new(HashMap::new())),
            next_fh: AtomicU64::new(1),
        }
    }
    
    /// Mount filesystem
    pub fn mount(self, mountpoint: &str) -> Result<(), FuseError> {
        let options = vec![
            fuser::MountOption::RO,
            fuser::MountOption::FSName("deriva".to_string()),
            fuser::MountOption::AutoUnmount,
        ];
        
        fuser::mount2(self, mountpoint, &options)?;
        Ok(())
    }
}
```

### 2.4 FUSE Operations

```rust
// crates/deriva-fuse/src/fs.rs

impl Filesystem for DerivaFS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_str().unwrap();
        
        // Handle special directories
        if parent == InodeCache::ROOT_INO {
            if name_str == "by-addr" {
                let attr = self.inode_cache.lock().unwrap()
                    .metadata.get(&InodeCache::BY_ADDR_INO).unwrap().clone();
                reply.entry(&Duration::from_secs(1), &attr, 0);
                return;
            }
        }
        
        // Handle by-addr lookups
        if parent == InodeCache::BY_ADDR_INO {
            // Parse CAddr from name
            if let Ok(addr) = CAddr::from_hex(name_str) {
                // Check if exists
                let dag = self.dag.clone();
                let cache = self.inode_cache.clone();
                
                tokio::spawn(async move {
                    if let Some(bytes) = dag.get(addr).await {
                        let mut cache = cache.lock().unwrap();
                        
                        // Determine if recipe or leaf
                        let kind = if bincode::deserialize::<Recipe>(&bytes).is_ok() {
                            FileType::Directory
                        } else {
                            FileType::RegularFile
                        };
                        
                        let ino = cache.get_or_create(addr, kind, bytes.len() as u64);
                        let attr = cache.metadata.get(&ino).unwrap().clone();
                        
                        reply.entry(&Duration::from_secs(1), &attr, 0);
                    } else {
                        reply.error(libc::ENOENT);
                    }
                });
                return;
            }
        }
        
        reply.error(libc::ENOENT);
    }
    
    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        let cache = self.inode_cache.lock().unwrap();
        
        if let Some(attr) = cache.metadata.get(&ino) {
            reply.attr(&Duration::from_secs(1), attr);
        } else {
            reply.error(libc::ENOENT);
        }
    }
    
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
            match dag.get(addr).await {
                Some(bytes) => {
                    let start = offset as usize;
                    let end = (start + size as usize).min(bytes.len());
                    
                    if start >= bytes.len() {
                        reply.data(&[]);
                    } else {
                        reply.data(&bytes[start..end]);
                    }
                }
                None => reply.error(libc::ENOENT),
            }
        });
    }
    
    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        // Root directory
        if ino == InodeCache::ROOT_INO {
            if offset == 0 {
                reply.add(InodeCache::ROOT_INO, 0, FileType::Directory, ".");
                reply.add(InodeCache::ROOT_INO, 1, FileType::Directory, "..");
                reply.add(InodeCache::BY_ADDR_INO, 2, FileType::Directory, "by-addr");
            }
            reply.ok();
            return;
        }
        
        // by-addr directory (empty, entries created on-demand)
        if ino == InodeCache::BY_ADDR_INO {
            if offset == 0 {
                reply.add(InodeCache::BY_ADDR_INO, 0, FileType::Directory, ".");
                reply.add(InodeCache::ROOT_INO, 1, FileType::Directory, "..");
            }
            reply.ok();
            return;
        }
        
        // Recipe directory (show inputs, function_id, params, output)
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
            match dag.get(addr).await {
                Some(bytes) => {
                    if let Ok(recipe) = bincode::deserialize::<Recipe>(&bytes) {
                        if offset == 0 {
                            reply.add(ino, 0, FileType::Directory, ".");
                            reply.add(InodeCache::BY_ADDR_INO, 1, FileType::Directory, "..");
                            reply.add(ino + 1, 2, FileType::Directory, "inputs");
                            reply.add(ino + 2, 3, FileType::RegularFile, "function_id");
                            reply.add(ino + 3, 4, FileType::RegularFile, "params.json");
                            reply.add(ino + 4, 5, FileType::RegularFile, "output");
                        }
                        reply.ok();
                    } else {
                        reply.error(libc::ENOTDIR);
                    }
                }
                None => reply.error(libc::ENOENT),
            }
        });
    }
}
```

---

## 3. Implementation

### 3.1 Streaming Reads

```rust
// crates/deriva-fuse/src/streaming.rs

impl FileHandle {
    /// Read chunk with buffering
    pub async fn read_chunk(
        &mut self,
        dag: &DagStore,
        offset: u64,
        size: u32,
    ) -> Result<Bytes, FuseError> {
        // Check if requested range is in buffer
        if offset >= self.offset && offset + size as u64 <= self.offset + self.buffer.len() as u64 {
            let start = (offset - self.offset) as usize;
            let end = start + size as usize;
            return Ok(Bytes::from(self.buffer[start..end].to_vec()));
        }
        
        // Fetch new chunk
        let bytes = dag.get(self.addr).await
            .ok_or(FuseError::NotFound)?;
        
        // Update buffer
        self.offset = offset;
        self.buffer = bytes[offset as usize..].to_vec();
        
        // Return requested range
        let end = size.min(self.buffer.len() as u32) as usize;
        Ok(Bytes::from(self.buffer[..end].to_vec()))
    }
}
```

### 3.2 Recipe Directory Structure

```rust
// crates/deriva-fuse/src/recipe_dir.rs

/// Generate directory entries for a recipe
pub fn recipe_entries(recipe: &Recipe) -> Vec<DirEntry> {
    let mut entries = vec![
        DirEntry {
            name: "function_id".into(),
            kind: FileType::RegularFile,
            content: Some(recipe.function_id.as_str().as_bytes().to_vec()),
        },
        DirEntry {
            name: "params.json".into(),
            kind: FileType::RegularFile,
            content: Some(serde_json::to_vec_pretty(&recipe.params).unwrap()),
        },
    ];
    
    // Add inputs directory
    entries.push(DirEntry {
        name: "inputs".into(),
        kind: FileType::Directory,
        content: None,
    });
    
    // Add output file (computed result)
    entries.push(DirEntry {
        name: "output".into(),
        kind: FileType::RegularFile,
        content: None,  // Lazy-loaded
    });
    
    entries
}

pub struct DirEntry {
    pub name: String,
    pub kind: FileType,
    pub content: Option<Vec<u8>>,
}
```

### 3.3 Lazy Materialization

```rust
// crates/deriva-fuse/src/fs.rs

impl DerivaFS {
    /// Read output file (triggers materialization)
    async fn read_output(&self, recipe_addr: CAddr) -> Result<Bytes, FuseError> {
        // Check if already materialized
        if let Some(bytes) = self.dag.get(recipe_addr).await {
            return Ok(bytes);
        }
        
        // Materialize on-demand
        let mut executor = Executor::new(
            &self.dag,
            &self.registry,
            &mut self.cache,
            &self.leaf_store,
            ExecutorConfig::default(),
        );
        
        let result = executor.materialize(recipe_addr).await?;
        Ok(result)
    }
}
```

### 3.4 Mount Command

```rust
// crates/deriva-fuse/src/main.rs

use clap::Parser;

#[derive(Parser)]
struct Args {
    /// Mount point
    #[arg(short, long)]
    mountpoint: String,
    
    /// Deriva server URL
    #[arg(short, long, default_value = "http://localhost:50051")]
    server: String,
    
    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    if args.debug {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Debug)
            .init();
    }
    
    // Connect to Deriva server
    let dag = DagStore::connect(&args.server).await?;
    
    // Create filesystem
    let fs = DerivaFS::new(Arc::new(dag));
    
    // Mount
    println!("Mounting Deriva at {}", args.mountpoint);
    fs.mount(&args.mountpoint)?;
    
    Ok(())
}
```

---

## 4. Data Flow Diagrams

### 4.1 File Read Flow

```
┌──────┐                                                      ┌──────────┐
│ User │                                                      │ DerivaFS │
└──┬───┘                                                      └────┬─────┘
   │                                                                │
   │ cat /mnt/deriva/by-addr/abc123.../output                      │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                          lookup("abc123...")   │
   │                                          ─────────────         │
   │                                                                │
   │                                          dag.get(abc123...)    │
   │                                          ─────────────         │
   │                                                                │
   │                                          Recipe found          │
   │                                          ─────────────         │
   │                                                                │
   │                                          open("output")        │
   │                                          ─────────────         │
   │                                                                │
   │                                          materialize(abc123...)│
   │                                          ─────────────         │
   │                                                                │
   │                                          read(offset=0, size=4K)│
   │                                          ─────────────         │
   │                                                                │
   │ "Hello, World!"                                               │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

### 4.2 Directory Listing Flow

```
┌──────┐                                                      ┌──────────┐
│ User │                                                      │ DerivaFS │
└──┬───┘                                                      └────┬─────┘
   │                                                                │
   │ ls /mnt/deriva/by-addr/abc123.../                             │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                          lookup("abc123...")   │
   │                                          ─────────────         │
   │                                                                │
   │                                          dag.get(abc123...)    │
   │                                          ─────────────         │
   │                                                                │
   │                                          Deserialize Recipe    │
   │                                          ─────────────         │
   │                                                                │
   │                                          readdir()             │
   │                                          ─────────────         │
   │                                                                │
   │                                          Generate entries:     │
   │                                          • inputs/             │
   │                                          • function_id         │
   │                                          • params.json         │
   │                                          • output              │
   │                                                                │
   │ inputs/  function_id  params.json  output                     │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

### 4.3 Streaming Read Flow

```
┌──────┐                                                      ┌──────────┐
│ User │                                                      │FileHandle│
└──┬───┘                                                      └────┬─────┘
   │                                                                │
   │ read(offset=1GB, size=4KB)                                    │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                          Check buffer          │
   │                                          ─────────────         │
   │                                                                │
   │                                          ✗ Not in buffer       │
   │                                                                │
   │                                          Fetch chunk at 1GB    │
   │                                          ─────────────         │
   │                                                                │
   │                                          Update buffer         │
   │                                          ─────────────         │
   │                                                                │
   │                                          Return 4KB            │
   │                                          ─────────────         │
   │                                                                │
   │ 4KB data                                                      │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

---

## 5. Test Specification

### 5.1 Unit Tests

```rust
// crates/deriva-fuse/tests/fs.rs

#[tokio::test]
async fn test_mount_root() {
    let dag = DagStore::new_memory();
    let fs = DerivaFS::new(Arc::new(dag));
    
    // Test root directory
    let mut reply = MockReplyDirectory::new();
    fs.readdir(&Request::default(), InodeCache::ROOT_INO, 0, 0, &mut reply);
    
    let entries = reply.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, ".");
    assert_eq!(entries[1].name, "..");
    assert_eq!(entries[2].name, "by-addr");
}

#[tokio::test]
async fn test_lookup_leaf() {
    let dag = DagStore::new_memory();
    let bytes = Bytes::from("hello world");
    let addr = CAddr::from_bytes(&bytes);
    dag.put(addr, bytes.clone()).await.unwrap();
    
    let fs = DerivaFS::new(Arc::new(dag));
    
    // Lookup by CAddr
    let mut reply = MockReplyEntry::new();
    fs.lookup(
        &Request::default(),
        InodeCache::BY_ADDR_INO,
        OsStr::new(&addr.to_hex()),
        &mut reply,
    );
    
    let entry = reply.entry().unwrap();
    assert_eq!(entry.attr.kind, FileType::RegularFile);
    assert_eq!(entry.attr.size, 11);
}

#[tokio::test]
async fn test_read_file() {
    let dag = DagStore::new_memory();
    let bytes = Bytes::from("hello world");
    let addr = CAddr::from_bytes(&bytes);
    dag.put(addr, bytes.clone()).await.unwrap();
    
    let mut fs = DerivaFS::new(Arc::new(dag));
    
    // Get inode
    let cache = fs.inode_cache.lock().unwrap();
    let ino = cache.get_or_create(addr, FileType::RegularFile, 11);
    drop(cache);
    
    // Read file
    let mut reply = MockReplyData::new();
    fs.read(&Request::default(), ino, 0, 0, 11, 0, None, &mut reply);
    
    let data = reply.data().unwrap();
    assert_eq!(data, b"hello world");
}

#[tokio::test]
async fn test_read_partial() {
    let dag = DagStore::new_memory();
    let bytes = Bytes::from("hello world");
    let addr = CAddr::from_bytes(&bytes);
    dag.put(addr, bytes.clone()).await.unwrap();
    
    let mut fs = DerivaFS::new(Arc::new(dag));
    
    let cache = fs.inode_cache.lock().unwrap();
    let ino = cache.get_or_create(addr, FileType::RegularFile, 11);
    drop(cache);
    
    // Read partial (offset=6, size=5)
    let mut reply = MockReplyData::new();
    fs.read(&Request::default(), ino, 0, 6, 5, 0, None, &mut reply);
    
    let data = reply.data().unwrap();
    assert_eq!(data, b"world");
}
```

### 5.2 Integration Tests

```rust
// crates/deriva-fuse/tests/integration.rs

#[tokio::test]
async fn test_mount_and_read() {
    let dag = DagStore::new_memory();
    let bytes = Bytes::from("test content");
    let addr = CAddr::from_bytes(&bytes);
    dag.put(addr, bytes.clone()).await.unwrap();
    
    let fs = DerivaFS::new(Arc::new(dag));
    
    // Mount in background
    let mountpoint = "/tmp/deriva-test";
    std::fs::create_dir_all(mountpoint).unwrap();
    
    let mount_handle = tokio::spawn(async move {
        fs.mount(mountpoint).unwrap();
    });
    
    // Wait for mount
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Read file
    let path = format!("{}/by-addr/{}", mountpoint, addr.to_hex());
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content, "test content");
    
    // Unmount
    std::process::Command::new("fusermount")
        .arg("-u")
        .arg(mountpoint)
        .status()
        .unwrap();
    
    mount_handle.abort();
}

#[tokio::test]
async fn test_recipe_directory() {
    let dag = DagStore::new_memory();
    
    // Create recipe
    let input = Bytes::from("input");
    let input_addr = CAddr::from_bytes(&input);
    dag.put(input_addr, input.clone()).await.unwrap();
    
    let recipe = Recipe {
        function_id: FunctionId::from("identity"),
        inputs: vec![input_addr],
        params: BTreeMap::new(),
    };
    let recipe_addr = recipe.addr();
    dag.put(recipe_addr, bincode::serialize(&recipe).unwrap().into()).await.unwrap();
    
    let fs = DerivaFS::new(Arc::new(dag));
    
    // Mount
    let mountpoint = "/tmp/deriva-recipe-test";
    std::fs::create_dir_all(mountpoint).unwrap();
    
    let mount_handle = tokio::spawn(async move {
        fs.mount(mountpoint).unwrap();
    });
    
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // List recipe directory
    let recipe_dir = format!("{}/by-addr/{}", mountpoint, recipe_addr.to_hex());
    let entries: Vec<_> = std::fs::read_dir(&recipe_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_str().unwrap().to_string())
        .collect();
    
    assert!(entries.contains(&"inputs".to_string()));
    assert!(entries.contains(&"function_id".to_string()));
    assert!(entries.contains(&"params.json".to_string()));
    assert!(entries.contains(&"output".to_string()));
    
    // Read function_id
    let function_id_path = format!("{}/function_id", recipe_dir);
    let function_id = std::fs::read_to_string(&function_id_path).unwrap();
    assert_eq!(function_id, "identity");
    
    // Unmount
    std::process::Command::new("fusermount")
        .arg("-u")
        .arg(mountpoint)
        .status()
        .unwrap();
    
    mount_handle.abort();
}
```

---

## 6. Edge Cases & Error Handling

### 6.1 Non-Existent CAddr

```rust
#[tokio::test]
async fn test_lookup_nonexistent() {
    let dag = DagStore::new_memory();
    let fs = DerivaFS::new(Arc::new(dag));
    
    let mut reply = MockReplyEntry::new();
    fs.lookup(
        &Request::default(),
        InodeCache::BY_ADDR_INO,
        OsStr::new("nonexistent"),
        &mut reply,
    );
    
    assert_eq!(reply.error(), Some(libc::ENOENT));
}
```

### 6.2 Invalid CAddr Format

```rust
#[tokio::test]
async fn test_lookup_invalid_addr() {
    let dag = DagStore::new_memory();
    let fs = DerivaFS::new(Arc::new(dag));
    
    let mut reply = MockReplyEntry::new();
    fs.lookup(
        &Request::default(),
        InodeCache::BY_ADDR_INO,
        OsStr::new("invalid-hex"),
        &mut reply,
    );
    
    assert_eq!(reply.error(), Some(libc::ENOENT));
}
```

### 6.3 Read Beyond EOF

```rust
#[tokio::test]
async fn test_read_beyond_eof() {
    let dag = DagStore::new_memory();
    let bytes = Bytes::from("hello");
    let addr = CAddr::from_bytes(&bytes);
    dag.put(addr, bytes.clone()).await.unwrap();
    
    let mut fs = DerivaFS::new(Arc::new(dag));
    
    let cache = fs.inode_cache.lock().unwrap();
    let ino = cache.get_or_create(addr, FileType::RegularFile, 5);
    drop(cache);
    
    // Read beyond EOF
    let mut reply = MockReplyData::new();
    fs.read(&Request::default(), ino, 0, 100, 10, 0, None, &mut reply);
    
    let data = reply.data().unwrap();
    assert_eq!(data, b"");  // Empty read
}
```

### 6.4 Concurrent Reads

```rust
#[tokio::test]
async fn test_concurrent_reads() {
    let dag = DagStore::new_memory();
    let bytes = Bytes::from("test content");
    let addr = CAddr::from_bytes(&bytes);
    dag.put(addr, bytes.clone()).await.unwrap();
    
    let fs = Arc::new(Mutex::new(DerivaFS::new(Arc::new(dag))));
    
    // Spawn 10 concurrent reads
    let mut handles = vec![];
    for _ in 0..10 {
        let fs = fs.clone();
        let handle = tokio::spawn(async move {
            let mut fs = fs.lock().unwrap();
            let cache = fs.inode_cache.lock().unwrap();
            let ino = cache.addr_to_ino.get(&addr).unwrap();
            drop(cache);
            
            let mut reply = MockReplyData::new();
            fs.read(&Request::default(), *ino, 0, 0, 12, 0, None, &mut reply);
            reply.data().unwrap()
        });
        handles.push(handle);
    }
    
    // All reads should succeed
    for handle in handles {
        let data = handle.await.unwrap();
        assert_eq!(data, b"test content");
    }
}
```

---

## 7. Performance Analysis

### 7.1 Metadata Operations

**lookup():**
- Cache hit: ~1 µs
- Cache miss: ~10 ms (network fetch)

**getattr():**
- Cache hit: ~1 µs
- Cache miss: ~10 ms

**readdir():**
- Root: ~10 µs (static)
- Recipe: ~10 ms (deserialize)

### 7.2 Read Operations

**Small file (1 KB):**
- First read: ~10 ms (fetch)
- Cached read: ~100 µs

**Large file (1 GB):**
- Sequential read: ~10 seconds (100 MB/s)
- Random read: ~10 ms per 4 KB chunk

### 7.3 Memory Usage

**Inode cache:**
- 1000 inodes: ~100 KB
- 10,000 inodes: ~1 MB
- 100,000 inodes: ~10 MB

**File handle buffers:**
- 10 open files: ~40 KB (4 KB buffer each)
- 100 open files: ~400 KB

---

## 8. Files Changed

### New Files
- `crates/deriva-fuse/src/fs.rs` — FUSE filesystem implementation
- `crates/deriva-fuse/src/inode_cache.rs` — Inode cache
- `crates/deriva-fuse/src/file_handle.rs` — File handle for streaming
- `crates/deriva-fuse/src/recipe_dir.rs` — Recipe directory structure
- `crates/deriva-fuse/src/streaming.rs` — Streaming read implementation
- `crates/deriva-fuse/src/main.rs` — Mount command
- `crates/deriva-fuse/tests/fs.rs` — Unit tests
- `crates/deriva-fuse/tests/integration.rs` — Integration tests
- `crates/deriva-fuse/Cargo.toml` — Dependencies

### Modified Files
- `Cargo.toml` — Add deriva-fuse crate

---

## 9. Dependency Changes

```toml
# crates/deriva-fuse/Cargo.toml
[dependencies]
fuser = "0.14"
libc = "0.2"
tokio = { version = "1.35", features = ["full"] }
bytes = "1.5"
bincode = "1.3"
serde_json = "1.0"
log = "0.4"
env_logger = "0.11"
clap = { version = "4.4", features = ["derive"] }

[dev-dependencies]
tempfile = "3.8"
```

**New dependency:** `fuser` (FUSE library for Rust)

---

## 10. Design Rationale

### 10.1 Why Read-Only?

**Alternative:** Support writes (create, modify, delete).

**Problem:** Content-addressed storage is immutable by design.

**Decision:** Read-only filesystem matches Deriva's immutability guarantee.

### 10.2 Why Lazy Loading?

**Alternative:** Pre-fetch entire DAG on mount.

**Problem:** DAGs can be terabytes in size.

**Decision:** Lazy loading fetches content only when accessed.

### 10.3 Why Recipe Directories?

**Alternative:** Flat file structure (all CAddrs as files).

**Problem:** Can't navigate DAG structure.

**Decision:** Recipe directories expose inputs, params, and output as navigable structure.

### 10.4 Why Inode Cache?

**Alternative:** Generate inodes on-the-fly.

**Problem:** FUSE requires stable inodes (same CAddr = same inode).

**Decision:** Cache CAddr → inode mapping for stability.

---

## 11. Observability Integration

### 11.1 Metrics

```rust
lazy_static! {
    static ref FUSE_OPERATIONS: IntCounterVec = register_int_counter_vec!(
        "deriva_fuse_operations_total",
        "FUSE operations by type and result",
        &["operation", "result"]
    ).unwrap();

    static ref FUSE_OPERATION_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_fuse_operation_duration_seconds",
        "FUSE operation duration by type",
        &["operation"]
    ).unwrap();
    
    static ref FUSE_INODE_CACHE_SIZE: IntGauge = register_int_gauge!(
        "deriva_fuse_inode_cache_size",
        "Number of cached inodes"
    ).unwrap();
    
    static ref FUSE_OPEN_FILES: IntGauge = register_int_gauge!(
        "deriva_fuse_open_files",
        "Number of open file handles"
    ).unwrap();
}
```

### 11.2 Logs

```rust
impl Filesystem for DerivaFS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let start = Instant::now();
        
        debug!("lookup: parent={}, name={:?}", parent, name);
        
        // ... lookup logic ...
        
        let duration = start.elapsed();
        FUSE_OPERATION_DURATION
            .with_label_values(&["lookup"])
            .observe(duration.as_secs_f64());
        FUSE_OPERATIONS
            .with_label_values(&["lookup", "success"])
            .inc();
        
        info!("lookup completed in {:?}", duration);
    }
}
```

### 11.3 Tracing

```rust
use tracing::{info_span, instrument};

#[instrument(skip(self, reply))]
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
    let span = info_span!(
        "fuse_read",
        ino = ino,
        offset = offset,
        size = size
    );
    let _enter = span.enter();
    
    // ... read logic ...
}
```

---

## 12. Checklist

### Implementation
- [ ] Create `deriva-fuse/src/fs.rs` with `DerivaFS` type
- [ ] Implement `Filesystem` trait (lookup, getattr, read, readdir)
- [ ] Create `deriva-fuse/src/inode_cache.rs` with CAddr → inode mapping
- [ ] Implement lazy loading (fetch on access)
- [ ] Create `deriva-fuse/src/file_handle.rs` for streaming reads
- [ ] Implement recipe directory structure (inputs/, function_id, params.json, output)
- [ ] Add mount command with CLI args
- [ ] Implement buffered reads for large files
- [ ] Add inode cache eviction (LRU)

### Testing
- [ ] Unit test: mount root directory
- [ ] Unit test: lookup leaf value
- [ ] Unit test: lookup recipe
- [ ] Unit test: read file (full)
- [ ] Unit test: read file (partial)
- [ ] Unit test: readdir root
- [ ] Unit test: readdir recipe
- [ ] Unit test: non-existent CAddr
- [ ] Unit test: invalid CAddr format
- [ ] Unit test: read beyond EOF
- [ ] Unit test: concurrent reads
- [ ] Integration test: mount and read
- [ ] Integration test: recipe directory structure
- [ ] Benchmark: lookup latency (<10ms)
- [ ] Benchmark: read throughput (>100 MB/s)

### Documentation
- [ ] Document mount command usage
- [ ] Add examples of browsing DAG with standard tools
- [ ] Document filesystem structure (by-addr/, by-name/)
- [ ] Add troubleshooting guide for mount issues
- [ ] Document performance characteristics
- [ ] Add security considerations (read-only, no writes)

### Observability
- [ ] Add `deriva_fuse_operations_total` counter
- [ ] Add `deriva_fuse_operation_duration_seconds` histogram
- [ ] Add `deriva_fuse_inode_cache_size` gauge
- [ ] Add `deriva_fuse_open_files` gauge
- [ ] Add debug logs for FUSE operations
- [ ] Add tracing spans for read/lookup

### Validation
- [ ] Test mount on Linux
- [ ] Test with standard tools (ls, cat, grep, vim)
- [ ] Verify lazy loading (only fetches accessed content)
- [ ] Test streaming reads for large files
- [ ] Verify recipe directory structure
- [ ] Test concurrent access (multiple processes)
- [ ] Benchmark read performance (target: >100 MB/s)

### Deployment
- [ ] Deploy FUSE mount as systemd service
- [ ] Monitor FUSE operation metrics
- [ ] Document mount options (read-only, auto-unmount)
- [ ] Add admin API to list mounted filesystems
- [ ] Set up alerts for mount failures

---

**Estimated effort:** 6–8 days
- Days 1-2: Core FUSE implementation (lookup, getattr, read, readdir)
- Day 3: Inode cache + lazy loading
- Day 4: Recipe directory structure + streaming reads
- Day 5: Mount command + CLI
- Days 6-7: Tests + benchmarks
- Day 8: Documentation + observability

**Success criteria:**
1. All tests pass (lookup, read, readdir)
2. Mount works on Linux with standard tools
3. Lazy loading only fetches accessed content
4. Read performance >100 MB/s for large files
5. Recipe directories expose DAG structure
6. Concurrent access works correctly
7. Metadata operations <10ms latency
