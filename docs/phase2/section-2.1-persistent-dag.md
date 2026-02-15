# Section 2.1: Persistent DAG Store

> **Goal:** Back the in-memory `DagStore` with sled so the dependency graph survives process restarts
> without requiring a full rebuild from the recipe store on every startup.
>
> **Crate:** `deriva-core` (new: `persistent_dag.rs`), `deriva-storage` (integration)
>
> **Estimated tests:** ~20
>
> **Commit message pattern:** `2.1: Persistent DAG — sled-backed dependency graph with incremental updates`

---

## 1. Problem Statement

The current `DagStore` is purely in-memory:

```rust
// Current: deriva-core/src/dag.rs
#[derive(Debug, Default)]
pub struct DagStore {
    recipes: HashMap<CAddr, Recipe>,
    forward: HashMap<CAddr, Vec<CAddr>>,    // addr → inputs
    reverse: HashMap<CAddr, HashSet<CAddr>>, // addr → dependents
}
```

On startup, `ServerState::new()` calls `storage.rebuild_dag()` which iterates ALL recipes in sled
and re-inserts them into a fresh `DagStore`:

```rust
// Current: deriva-storage/src/recipe_store.rs
pub fn rebuild_dag(&self) -> Result<DagStore> {
    let mut dag = DagStore::new();
    for result in self.iter_all() {
        let (_addr, recipe) = result?;
        dag.insert(recipe)?;
    }
    Ok(dag)
}
```

**Problems:**
- **O(N) startup time** — must deserialize and re-insert every recipe
- **Redundant data** — recipes are stored in `SledRecipeStore` AND in `DagStore.recipes`
- **Memory pressure** — the full recipe map is duplicated in memory
- **No incremental persistence** — if the process crashes mid-operation, the in-memory DAG is lost

With 1000 recipes this is fine. With 1M recipes, startup takes seconds to minutes.

### Quantified Impact

| Recipe Count | Phase 1 Startup (est.) | Phase 2.1 Startup | Memory Saved |
|-------------|------------------------|-------------------|--------------|
| 1,000 | ~50ms | ~1ms | ~200 KB |
| 10,000 | ~500ms | ~1ms | ~2 MB |
| 100,000 | ~5s | ~1ms | ~20 MB |
| 1,000,000 | ~50s | ~1ms | ~200 MB |

The memory savings come from eliminating the `DagStore.recipes` HashMap — recipes are
already in `SledRecipeStore`. The forward/reverse adjacency lists are much smaller than
full `Recipe` objects (32 bytes per CAddr vs ~200+ bytes per serialized Recipe).

---

## 2. Design

### 2.1.1 Architecture

Replace the in-memory `DagStore` with a `PersistentDag` that stores the forward and reverse
adjacency lists in sled, while keeping a hot in-memory cache for fast lookups.

```
┌─────────────────────────────────────────────┐
│              PersistentDag                   │
├─────────────────────────────────────────────┤
│  In-Memory (hot cache)                      │
│  ┌─────────────────────────────────────┐    │
│  │ forward_cache: LruCache<CAddr, Vec> │    │
│  │ reverse_cache: LruCache<CAddr, Set> │    │
│  │ recipe_count: AtomicU64             │    │
│  └─────────────────────────────────────┘    │
├─────────────────────────────────────────────┤
│  Sled Trees (persistent)                    │
│  ┌──────────────┐  ┌───────────────┐        │
│  │ tree:forward  │  │ tree:reverse  │        │
│  │ addr → [deps] │  │ addr → {deps} │        │
│  └──────────────┘  └───────────────┘        │
└─────────────────────────────────────────────┘
```

**Key insight:** We do NOT store recipes in the DAG anymore. The `SledRecipeStore` already has them.
The DAG only stores adjacency lists (forward edges and reverse edges). This eliminates the
duplication.

### 2.1.2 Sled Tree Layout

Use two separate sled trees (not the default tree) within the same sled database:

```
Tree: "dag_forward"
  Key:   CAddr (32 bytes)
  Value: bincode(Vec<CAddr>)  — the input addresses

Tree: "dag_reverse"
  Key:   CAddr (32 bytes)
  Value: bincode(Vec<CAddr>)  — the dependent addresses (stored as Vec, deduplicated on read)
```

Why separate trees instead of prefixed keys in the default tree?
- Sled trees are independent B-trees — no prefix scanning overhead
- Clean separation of concerns
- Can be iterated independently

### 2.1.3 Write Path

When a recipe is inserted:

```
insert(recipe) {
    1. Compute addr = recipe.addr()
    2. Check forward tree: if addr exists → return Ok(addr) (idempotent)
    3. Check for self-cycle: if addr in recipe.inputs → error
    4. Write to forward tree: addr → recipe.inputs
    5. For each input in recipe.inputs:
       a. Read reverse[input] (or empty vec)
       b. Append addr if not present
       c. Write reverse[input] back
    6. Flush sled (ensure durability)
    7. Update in-memory caches
    8. Return Ok(addr)
}
```

**Atomicity:** Steps 4-5 should be in a sled transaction (`db.transaction()`) to ensure
the forward and reverse edges are consistent even if the process crashes mid-write.

```
┌──────────────────────────────────────────────────┐
│                  insert(recipe)                   │
│                                                   │
│  ┌─────────────┐    ┌──────────────────────────┐ │
│  │ Check exists │───▶│ sled::transaction {      │ │
│  │ (fast path)  │    │   forward.insert(addr)   │ │
│  └─────────────┘    │   for input in inputs:    │ │
│                      │     reverse.merge(input)  │ │
│                      │ }                         │ │
│                      └──────────┬───────────────┘ │
│                                 │                  │
│                      ┌──────────▼───────────────┐ │
│                      │ Update memory caches      │ │
│                      └──────────────────────────┘ │
└──────────────────────────────────────────────────┘
```

### 2.1.4 Read Path

```
get_recipe(addr) → delegate to SledRecipeStore.get(addr)
direct_dependents(addr) → read reverse tree (cache first)
resolve_order(addr) → topological sort using forward tree reads
depth(addr) → recursive with memoization using forward tree
```

The in-memory LRU caches avoid hitting sled on every read. Cache size is configurable
(default: 10,000 entries for forward, 10,000 for reverse).

### 2.1.5 Startup Path

```
PersistentDag::open(sled_db) {
    1. Open "dag_forward" tree
    2. Open "dag_reverse" tree
    3. Read recipe_count from forward tree length
    4. Do NOT load all entries into memory
    5. Return immediately — O(1) startup
}
```

This is the key improvement: **O(1) startup** instead of O(N).

### 2.1.6 LRU Cache Layer

The raw sled reads are fast (~1-5μs for point lookups) but still orders of magnitude slower
than HashMap lookups (~50ns). For hot-path operations like `resolve_order` which may read
dozens of forward entries, an LRU cache is essential.

```rust
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Mutex;

pub struct PersistentDagConfig {
    /// Max entries in forward adjacency cache (default: 10,000)
    pub forward_cache_size: usize,
    /// Max entries in reverse adjacency cache (default: 10,000)
    pub reverse_cache_size: usize,
}

impl Default for PersistentDagConfig {
    fn default() -> Self {
        Self {
            forward_cache_size: 10_000,
            reverse_cache_size: 10_000,
        }
    }
}
```

The caches are wrapped in `Mutex` (not `RwLock`) because LRU access mutates internal order:

```rust
pub struct PersistentDag {
    forward: Tree,
    reverse: Tree,
    db: Db,
    forward_cache: Mutex<LruCache<CAddr, Vec<CAddr>>>,
    reverse_cache: Mutex<LruCache<CAddr, Vec<CAddr>>>,
}
```

Cache invalidation strategy:
- **insert**: populate forward_cache[addr], update reverse_cache[each input]
- **remove**: evict forward_cache[addr], update reverse_cache[each input]
- **No TTL needed** — data only changes through our own insert/remove methods

```
Read path with cache:

inputs(addr)
    │
    ├──▶ forward_cache.lock().get(addr)
    │       │
    │       ├── Some(inputs) → return immediately (~50ns)
    │       │
    │       └── None → sled.get(addr) (~2μs)
    │                    │
    │                    └── populate cache, return
    │
    ▼
```

### 2.1.7 Sled Transaction Semantics

Sled supports ACID transactions across multiple trees. The insert operation MUST be
transactional to prevent partial writes:

```rust
use sled::transaction::{TransactionResult, ConflictableTransactionResult};

pub fn insert_atomic(&self, recipe: &Recipe) -> Result<CAddr> {
    let addr = recipe.addr();
    let addr_bytes = addr.as_bytes().to_vec();
    let inputs = recipe.inputs.clone();
    let inputs_bytes = bincode::serialize(&inputs)
        .map_err(|e| DerivaError::Serialization(e.to_string()))?;

    // Pre-serialize all reverse updates
    let mut reverse_updates: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for input in &inputs {
        let key = input.as_bytes().to_vec();
        let mut deps: Vec<CAddr> = match self.reverse.get(&key)
            .map_err(|e| DerivaError::Storage(e.to_string()))? {
            Some(bytes) => bincode::deserialize(&bytes)
                .map_err(|e| DerivaError::Serialization(e.to_string()))?,
            None => Vec::new(),
        };
        if !deps.contains(&addr) {
            deps.push(addr);
        }
        let val = bincode::serialize(&deps)
            .map_err(|e| DerivaError::Serialization(e.to_string()))?;
        reverse_updates.push((key, val));
    }

    // Atomic transaction across both trees
    (&self.forward, &self.reverse).transaction(|(fwd, rev)| {
        fwd.insert(addr_bytes.as_slice(), inputs_bytes.as_slice())?;
        for (key, val) in &reverse_updates {
            rev.insert(key.as_slice(), val.as_slice())?;
        }
        Ok(())
    }).map_err(|e| DerivaError::Storage(format!("transaction failed: {}", e)))?;

    // Update caches after successful transaction
    if let Ok(mut cache) = self.forward_cache.lock() {
        cache.put(addr, inputs.clone());
    }
    for input in &inputs {
        // Invalidate reverse cache for inputs (will be re-read on next access)
        if let Ok(mut cache) = self.reverse_cache.lock() {
            cache.pop(input);
        }
    }

    Ok(addr)
}
```

**Why pre-compute reverse updates outside the transaction?**
Sled transactions retry on conflict. If we read inside the transaction, we'd re-read on
every retry. Pre-computing is safe because we hold no locks between read and transaction.

### 2.1.8 Consistency Repair

If the process crashes between a `SledRecipeStore.put()` and `PersistentDag.insert()`,
the recipe exists in storage but has no DAG edges. The repair logic runs on startup:

```rust
impl StorageBackend {
    /// Check for recipes that exist in the recipe store but not in the DAG.
    /// This handles crash recovery where recipe was written but DAG update failed.
    pub fn repair_consistency(&self) -> Result<u64> {
        let mut repaired = 0u64;
        for result in self.recipes.iter_all() {
            let (addr, recipe) = result?;
            if !self.dag.contains(&addr) {
                self.dag.insert(&recipe)?;
                repaired += 1;
            }
        }
        if repaired > 0 {
            self.dag.flush()?;
        }
        Ok(repaired)
    }
}
```

This is O(N) but only runs when inconsistency is detected. A faster approach for
production: keep a "last_consistent_addr" marker in sled and only scan recipes
inserted after that marker.

---

## 3. Implementation

### 3.1 New File: `deriva-core/src/persistent_dag.rs`

```rust
use crate::address::{CAddr, Recipe};
use crate::error::{DerivaError, Result};
use sled::{Db, Tree};
use std::collections::HashSet;

/// Sled-backed DAG with in-memory LRU caches for hot paths.
pub struct PersistentDag {
    forward: Tree,   // addr → Vec<CAddr> (inputs)
    reverse: Tree,   // addr → Vec<CAddr> (dependents)
    db: Db,
}

impl PersistentDag {
    pub fn open(db: &Db) -> Result<Self> {
        let forward = db.open_tree("dag_forward")
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        let reverse = db.open_tree("dag_reverse")
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(Self {
            forward,
            reverse,
            db: db.clone(),
        })
    }

    pub fn insert(&self, recipe: &Recipe) -> Result<CAddr> {
        let addr = recipe.addr();
        let addr_bytes = addr.as_bytes();

        // Idempotent: already exists
        if self.forward.contains_key(addr_bytes)
            .map_err(|e| DerivaError::Storage(e.to_string()))? {
            return Ok(addr);
        }

        // Self-cycle check
        if recipe.inputs.contains(&addr) {
            return Err(DerivaError::CycleDetected(
                format!("recipe {} lists itself as input", addr)
            ));
        }

        // Serialize inputs
        let inputs_bytes = bincode::serialize(&recipe.inputs)
            .map_err(|e| DerivaError::Serialization(e.to_string()))?;

        // Transaction: write forward + update all reverse entries atomically
        let inputs_for_tx = recipe.inputs.clone();
        let addr_for_tx = addr;

        // Write forward edge
        self.forward.insert(addr_bytes, inputs_bytes)
            .map_err(|e| DerivaError::Storage(e.to_string()))?;

        // Update reverse edges
        for input in &inputs_for_tx {
            self.add_reverse_edge(input, &addr_for_tx)?;
        }

        Ok(addr)
    }

    fn add_reverse_edge(&self, from: &CAddr, dependent: &CAddr) -> Result<()> {
        let key = from.as_bytes();
        let mut deps: Vec<CAddr> = match self.reverse.get(key)
            .map_err(|e| DerivaError::Storage(e.to_string()))? {
            Some(bytes) => bincode::deserialize(&bytes)
                .map_err(|e| DerivaError::Serialization(e.to_string()))?,
            None => Vec::new(),
        };
        if !deps.contains(dependent) {
            deps.push(*dependent);
            let bytes = bincode::serialize(&deps)
                .map_err(|e| DerivaError::Serialization(e.to_string()))?;
            self.reverse.insert(key, bytes)
                .map_err(|e| DerivaError::Storage(e.to_string()))?;
        }
        Ok(())
    }

    pub fn contains(&self, addr: &CAddr) -> bool {
        self.forward.contains_key(addr.as_bytes()).unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }

    /// Get the input addresses for a derived CAddr.
    pub fn inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        match self.forward.get(addr.as_bytes())
            .map_err(|e| DerivaError::Storage(e.to_string()))? {
            Some(bytes) => {
                let inputs: Vec<CAddr> = bincode::deserialize(&bytes)
                    .map_err(|e| DerivaError::Serialization(e.to_string()))?;
                Ok(Some(inputs))
            }
            None => Ok(None),
        }
    }

    /// Get direct dependents of a CAddr.
    pub fn direct_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        match self.reverse.get(addr.as_bytes()) {
            Ok(Some(bytes)) => {
                bincode::deserialize(&bytes).unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// Get all transitive dependents (BFS).
    pub fn transitive_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = std::collections::VecDeque::new();

        for dep in self.direct_dependents(addr) {
            if visited.insert(dep) {
                queue.push_back(dep);
            }
        }
        while let Some(current) = queue.pop_front() {
            result.push(current);
            for dep in self.direct_dependents(&current) {
                if visited.insert(dep) {
                    queue.push_back(dep);
                }
            }
        }
        result
    }

    /// Topological resolution order for materializing addr.
    pub fn resolve_order(&self, addr: &CAddr) -> Vec<CAddr> {
        let mut order = Vec::new();
        let mut visited = HashSet::new();
        self.topo_visit(addr, &mut visited, &mut order);
        order
    }

    fn topo_visit(
        &self,
        addr: &CAddr,
        visited: &mut HashSet<CAddr>,
        order: &mut Vec<CAddr>,
    ) {
        if !visited.insert(*addr) {
            return;
        }
        if let Ok(Some(inputs)) = self.inputs(addr) {
            for input in &inputs {
                self.topo_visit(input, visited, order);
            }
            order.push(*addr); // only push if it has inputs (i.e., it's a recipe)
        }
    }

    /// DAG depth of a node.
    pub fn depth(&self, addr: &CAddr) -> u32 {
        self.depth_inner(addr, &mut std::collections::HashMap::new())
    }

    fn depth_inner(
        &self,
        addr: &CAddr,
        cache: &mut std::collections::HashMap<CAddr, u32>,
    ) -> u32 {
        if let Some(&d) = cache.get(addr) {
            return d;
        }
        let d = match self.inputs(addr) {
            Ok(Some(inputs)) if !inputs.is_empty() => {
                let max = inputs.iter()
                    .map(|i| self.depth_inner(i, cache))
                    .max()
                    .unwrap_or(0);
                max + 1
            }
            _ => 0,
        };
        cache.insert(*addr, d);
        d
    }

    /// Remove a recipe from the DAG (forward + reverse edges).
    pub fn remove(&self, addr: &CAddr) -> Result<bool> {
        let inputs = match self.inputs(addr)? {
            Some(inputs) => inputs,
            None => return Ok(false),
        };

        // Remove forward edge
        self.forward.remove(addr.as_bytes())
            .map_err(|e| DerivaError::Storage(e.to_string()))?;

        // Remove from reverse edges of each input
        for input in &inputs {
            self.remove_reverse_edge(input, addr)?;
        }

        Ok(true)
    }

    fn remove_reverse_edge(&self, from: &CAddr, dependent: &CAddr) -> Result<()> {
        let key = from.as_bytes();
        if let Some(bytes) = self.reverse.get(key)
            .map_err(|e| DerivaError::Storage(e.to_string()))? {
            let mut deps: Vec<CAddr> = bincode::deserialize(&bytes)
                .map_err(|e| DerivaError::Serialization(e.to_string()))?;
            deps.retain(|d| d != dependent);
            if deps.is_empty() {
                self.reverse.remove(key)
                    .map_err(|e| DerivaError::Storage(e.to_string()))?;
            } else {
                let bytes = bincode::serialize(&deps)
                    .map_err(|e| DerivaError::Serialization(e.to_string()))?;
                self.reverse.insert(key, bytes)
                    .map_err(|e| DerivaError::Storage(e.to_string()))?;
            }
        }
        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        self.db.flush().map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(())
    }
}
```

### 3.2 Trait Extraction: `DagAccess`

To allow both the old in-memory `DagStore` (for unit tests) and the new `PersistentDag`
(for production), extract a trait:

```rust
// deriva-core/src/dag.rs — add trait

pub trait DagAccess {
    fn insert_recipe(&self, recipe: &Recipe) -> Result<CAddr>;
    fn contains(&self, addr: &CAddr) -> bool;
    fn inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>>;
    fn direct_dependents(&self, addr: &CAddr) -> Vec<CAddr>;
    fn transitive_dependents(&self, addr: &CAddr) -> Vec<CAddr>;
    fn resolve_order(&self, addr: &CAddr) -> Vec<CAddr>;
    fn depth(&self, addr: &CAddr) -> u32;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn remove(&self, addr: &CAddr) -> Result<bool>;
}
```

Implement `DagAccess` for both `DagStore` (wrapping existing methods with `&self` via interior
mutability or `&mut self` behind `RwLock`) and `PersistentDag`.

### 3.3 ServerState Changes

```rust
// Before (Phase 1):
pub struct ServerState {
    pub dag: RwLock<DagStore>,        // in-memory, rebuilt on startup
    pub cache: RwLock<EvictableCache>,
    pub registry: FunctionRegistry,
    pub storage: StorageBackend,
}

// After (Phase 2.1):
pub struct ServerState {
    pub dag: PersistentDag,            // sled-backed, O(1) startup
    pub cache: RwLock<EvictableCache>,
    pub registry: FunctionRegistry,
    pub storage: StorageBackend,
}
```

**Note:** `PersistentDag` uses sled's internal concurrency (sled is thread-safe), so we
no longer need `RwLock<DagStore>`. The `RwLock` is removed from the DAG field.

### 3.4 StorageBackend Changes

The `PersistentDag` needs access to the same sled `Db` instance as the `SledRecipeStore`.
Refactor `StorageBackend` to share the sled database:

```rust
// Before:
pub struct StorageBackend {
    pub recipes: SledRecipeStore,
    pub blobs: BlobStore,
}

// After:
pub struct StorageBackend {
    pub recipes: SledRecipeStore,
    pub blobs: BlobStore,
    pub dag: PersistentDag,
    db: sled::Db,  // shared sled instance
}

impl StorageBackend {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let db = sled::open(root.join("deriva.sled"))
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        let recipes = SledRecipeStore::from_db(&db)?;  // uses default tree
        let dag = PersistentDag::open(&db)?;            // uses dag_forward, dag_reverse trees
        let blobs = BlobStore::open(root.join("blobs"))?;
        Ok(Self { recipes, blobs, dag, db })
    }
}
```

### 3.5 Migration Path

For existing Phase 1 data (recipes in sled but no DAG trees):

```rust
impl StorageBackend {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        // ... open db, recipes, dag, blobs ...

        // Migration: if DAG trees are empty but recipes exist, populate DAG
        if dag.is_empty() && !recipes.is_empty() {
            for result in recipes.iter_all() {
                let (_addr, recipe) = result?;
                dag.insert(&recipe)?;
            }
            dag.flush()?;
        }

        Ok(Self { recipes, blobs, dag, db })
    }
}
```

This is a one-time migration. After the first run, the DAG trees are populated and
subsequent startups are O(1).

---

## 4. Data Flow Diagrams

### 4.1 Startup: Phase 1 vs Phase 2.1

```
Phase 1 Startup:
┌──────────┐    iter_all()     ┌──────────┐    insert() x N    ┌──────────┐
│   sled   │──────────────────▶│  Recipe   │──────────────────▶│ DagStore │
│ (recipes)│   O(N) deser      │  stream   │   O(N) inserts    │ (memory) │
└──────────┘                   └──────────┘                    └──────────┘
                                                               Time: O(N)

Phase 2.1 Startup:
┌──────────┐    open_tree()    ┌───────────────┐
│   sled   │──────────────────▶│ PersistentDag │   Ready immediately
│ (3 trees)│   O(1)            │  (sled trees) │
└──────────┘                   └───────────────┘
                                Time: O(1)
```

### 4.2 Insert Flow

```
put_recipe(recipe)
    │
    ▼
┌──────────────────┐
│ SledRecipeStore   │  store recipe bytes
│   .put(addr, r)   │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ PersistentDag    │  store adjacency
│   .insert(r)      │
│                    │
│  forward[addr]     │──▶ Vec<input_addrs>
│  reverse[input₁]  │──▶ append(addr)
│  reverse[input₂]  │──▶ append(addr)
└──────────────────┘
```

### 4.3 Resolve Flow

```
resolve_order(addr)
    │
    ▼
┌──────────────────┐
│ PersistentDag    │
│  .inputs(addr)    │──▶ sled read (or cache hit)
│                    │
│  for each input:   │
│    .inputs(input)  │──▶ recursive sled reads
│                    │
│  return topo order │
└──────────────────┘
```

---

## 5. Test Specification

### 5.1 Unit Tests: `deriva-core/tests/persistent_dag.rs`

All tests use a helper to create a temp sled db:

```rust
use tempfile::TempDir;
use deriva_core::persistent_dag::PersistentDag;
use deriva_core::address::{CAddr, Recipe, FunctionId, Value};
use std::collections::BTreeMap;

fn temp_dag() -> (TempDir, PersistentDag) {
    let dir = TempDir::new().unwrap();
    let db = sled::open(dir.path().join("test.sled")).unwrap();
    let dag = PersistentDag::open(&db).unwrap();
    (dir, dag) // dir must live as long as dag
}

fn leaf(data: &[u8]) -> CAddr {
    CAddr::of_leaf(data)
}

fn recipe(inputs: Vec<CAddr>, func: &str) -> Recipe {
    Recipe {
        inputs,
        function: FunctionId(func.to_string()),
        params: BTreeMap::new(),
    }
}
```

```
test_persistent_dag_insert_and_contains
    let (_dir, dag) = temp_dag();
    let a = leaf(b"hello");
    let r = recipe(vec![a], "identity");
    let addr = dag.insert(&r).unwrap();
    assert!(dag.contains(&addr));
    assert_eq!(dag.len(), 1);

test_persistent_dag_idempotent_insert
    Insert same recipe twice
    Assert len() == 1 (no duplicate)
    Assert second insert returns same CAddr

test_persistent_dag_self_cycle_rejected
    let a = leaf(b"data");
    let r = recipe(vec![a], "identity");
    let addr = r.addr();
    // Manually construct recipe with self-reference
    let bad = Recipe { inputs: vec![addr], ..r };
    assert!(dag.insert(&bad).is_err());

test_persistent_dag_inputs_query
    let a = leaf(b"a"); let b = leaf(b"b"); let c = leaf(b"c");
    let r = recipe(vec![a, b, c], "concat");
    let addr = dag.insert(&r).unwrap();
    let inputs = dag.inputs(&addr).unwrap().unwrap();
    assert_eq!(inputs.len(), 3);
    assert!(inputs.contains(&a));

test_persistent_dag_direct_dependents
    let a = leaf(b"a");
    let r = recipe(vec![a], "identity");
    let r_addr = dag.insert(&r).unwrap();
    let deps = dag.direct_dependents(&a);
    assert_eq!(deps, vec![r_addr]);

test_persistent_dag_transitive_dependents
    // Chain: a → r1 → r2 → r3
    let a = leaf(b"a");
    let r1 = recipe(vec![a], "f1");
    let r1_addr = dag.insert(&r1).unwrap();
    let r2 = recipe(vec![r1_addr], "f2");
    let r2_addr = dag.insert(&r2).unwrap();
    let r3 = recipe(vec![r2_addr], "f3");
    let r3_addr = dag.insert(&r3).unwrap();
    let deps = dag.transitive_dependents(&a);
    assert_eq!(deps.len(), 3);
    // r1 must appear before r2, r2 before r3 in BFS order
    let pos_r1 = deps.iter().position(|x| x == &r1_addr).unwrap();
    let pos_r3 = deps.iter().position(|x| x == &r3_addr).unwrap();
    assert!(pos_r1 < pos_r3);

test_persistent_dag_diamond_dependents
    // a → r1, a → r2, r1 → r3, r2 → r3
    let a = leaf(b"a");
    let r1 = recipe(vec![a], "f1");
    let r1a = dag.insert(&r1).unwrap();
    let r2 = recipe(vec![a], "f2");
    let r2a = dag.insert(&r2).unwrap();
    let r3 = recipe(vec![r1a, r2a], "f3");
    let r3a = dag.insert(&r3).unwrap();
    let deps = dag.transitive_dependents(&a);
    assert!(deps.contains(&r1a));
    assert!(deps.contains(&r2a));
    assert!(deps.contains(&r3a));

test_persistent_dag_resolve_order
    // Diamond: a,b → r1(a,b), a → r2(a), r1,r2 → r3
    let a = leaf(b"a"); let b = leaf(b"b");
    let r1 = recipe(vec![a, b], "f1");
    let r1a = dag.insert(&r1).unwrap();
    let r2 = recipe(vec![a], "f2");
    let r2a = dag.insert(&r2).unwrap();
    let r3 = recipe(vec![r1a, r2a], "f3");
    let r3a = dag.insert(&r3).unwrap();
    let order = dag.resolve_order(&r3a);
    // r1 and r2 must appear before r3
    let pos_r3 = order.iter().position(|x| x == &r3a).unwrap();
    assert!(order.iter().position(|x| x == &r1a).unwrap() < pos_r3);
    assert!(order.iter().position(|x| x == &r2a).unwrap() < pos_r3);

test_persistent_dag_depth
    // leaf(0) → r1(1) → r2(2) → r3(3)
    let a = leaf(b"a");
    let r1 = recipe(vec![a], "f1");
    let r1a = dag.insert(&r1).unwrap();
    let r2 = recipe(vec![r1a], "f2");
    let r2a = dag.insert(&r2).unwrap();
    let r3 = recipe(vec![r2a], "f3");
    let r3a = dag.insert(&r3).unwrap();
    assert_eq!(dag.depth(&a), 0);
    assert_eq!(dag.depth(&r1a), 1);
    assert_eq!(dag.depth(&r3a), 3);

test_persistent_dag_remove
    let a = leaf(b"a");
    let r = recipe(vec![a], "f");
    let addr = dag.insert(&r).unwrap();
    assert!(dag.contains(&addr));
    assert!(dag.remove(&addr).unwrap());
    assert!(!dag.contains(&addr));
    assert!(dag.direct_dependents(&a).is_empty());

test_persistent_dag_remove_nonexistent
    let fake = CAddr::of_leaf(b"nonexistent");
    assert_eq!(dag.remove(&fake).unwrap(), false);

test_persistent_dag_survives_restart
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.sled");
    {
        let db = sled::open(&db_path).unwrap();
        let dag = PersistentDag::open(&db).unwrap();
        let a = leaf(b"a");
        for i in 0..5 {
            let r = recipe(vec![a], &format!("f{}", i));
            dag.insert(&r).unwrap();
        }
        dag.flush().unwrap();
        // dag and db dropped here
    }
    {
        let db = sled::open(&db_path).unwrap();
        let dag = PersistentDag::open(&db).unwrap();
        assert_eq!(dag.len(), 5);
        let deps = dag.direct_dependents(&leaf(b"a"));
        assert_eq!(deps.len(), 5);
    }

test_persistent_dag_survives_restart_complex
    Build diamond DAG, close, reopen
    Assert resolve_order still correct
    Assert transitive_dependents still correct

test_persistent_dag_empty_on_fresh_db
    let (_dir, dag) = temp_dag();
    assert_eq!(dag.len(), 0);
    assert!(dag.is_empty());

test_persistent_dag_concurrent_inserts
    use std::thread;
    let dir = TempDir::new().unwrap();
    let db = sled::open(dir.path().join("test.sled")).unwrap();
    let dag = Arc::new(PersistentDag::open(&db).unwrap());
    let a = leaf(b"shared_input");
    let handles: Vec<_> = (0..10).map(|i| {
        let dag = Arc::clone(&dag);
        thread::spawn(move || {
            let r = recipe(vec![a], &format!("thread_{}", i));
            dag.insert(&r).unwrap();
        })
    }).collect();
    for h in handles { h.join().unwrap(); }
    assert_eq!(dag.len(), 10);
    assert_eq!(dag.direct_dependents(&a).len(), 10);
```

### 5.2 Integration Tests: `deriva-server/tests/integration.rs`

```
test_dag_persists_across_server_restart
    Start server, put leaf + recipe, get (materializes)
    Stop server (drop DerivaService)
    Restart server at same data dir
    Assert get() still works (recipe found in DAG)
    Assert resolve() returns correct recipe

test_dag_migration_from_phase1_data
    Create Phase 1 style storage (recipes in sled, no DAG trees)
    Start server — should auto-migrate
    Assert all recipes queryable via resolve()
    Assert get() works for derived addrs
```

---

## 6. Edge Cases & Error Handling

| Case | Expected Behavior |
|------|-------------------|
| sled write fails mid-transaction | Forward/reverse stay consistent (transaction rollback) |
| Process crash after forward write but before reverse | Repair on next startup via `repair_consistency()` |
| Concurrent insert of same recipe | Idempotent — second insert is a no-op |
| Remove recipe that doesn't exist | Returns `Ok(false)` |
| Query dependents of leaf (no reverse entry) | Returns empty vec |
| Very deep DAG (1000 levels) | Stack overflow risk in recursive topo_visit — see §6.1 |
| sled disk full | Returns `DerivaError::Storage` — caller should surface to user |
| Corrupted sled tree (bit rot) | sled has built-in CRC32 checksums — returns IO error |
| Concurrent insert + remove of same addr | Sled's internal locking ensures serialization |
| Empty recipe (no inputs) | Valid — represents a leaf-like recipe, depth = 0 |

### 6.1 Stack Overflow Mitigation for Deep DAGs

The recursive `topo_visit` and `depth_inner` methods risk stack overflow on very deep DAGs
(>1000 levels). The iterative alternative for `resolve_order`:

```rust
/// Iterative topological sort using Kahn's algorithm.
/// Avoids stack overflow for deep DAGs.
pub fn resolve_order_iterative(&self, addr: &CAddr) -> Vec<CAddr> {
    // Step 1: Collect all reachable nodes and their in-degrees
    let mut in_degree: HashMap<CAddr, usize> = HashMap::new();
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();

    // BFS to discover all nodes in the subgraph rooted at addr
    queue.push_back(*addr);
    visited.insert(*addr);
    while let Some(current) = queue.pop_front() {
        if let Ok(Some(inputs)) = self.inputs(&current) {
            in_degree.entry(current).or_insert(0);
            for input in &inputs {
                *in_degree.entry(*input).or_insert(0) += 0; // ensure entry exists
                if visited.insert(*input) {
                    queue.push_back(*input);
                }
            }
            // Count edges: current depends on each input
            *in_degree.entry(current).or_insert(0) = inputs.len();
        } else {
            in_degree.entry(current).or_insert(0);
        }
    }

    // Step 2: Kahn's algorithm
    let mut result = Vec::new();
    let mut ready: VecDeque<CAddr> = in_degree.iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&addr, _)| addr)
        .collect();

    while let Some(node) = ready.pop_front() {
        result.push(node);
        for dep in self.direct_dependents(&node) {
            if let Some(deg) = in_degree.get_mut(&dep) {
                *deg = deg.saturating_sub(1);
                if *deg == 0 {
                    ready.push_back(dep);
                }
            }
        }
    }

    result
}
```

This uses O(V) heap memory instead of O(V) stack frames — safe for arbitrarily deep DAGs.

---

## 7. Performance Expectations

| Operation | Phase 1 (in-memory) | Phase 2.1 (sled) | Notes |
|-----------|--------------------|--------------------|-------|
| Startup | O(N) — rebuild all | O(1) — open trees | Major improvement |
| insert | O(1) amortized | O(k) where k = input count | sled writes per input |
| contains | O(1) HashMap | O(1) sled point lookup | Comparable |
| direct_dependents | O(1) HashMap | O(1) sled point lookup | Comparable |
| resolve_order | O(V+E) in-memory | O(V+E) with sled reads | Slower per-read, but cached |
| Memory usage | O(N) all recipes | O(cache_size) | Major improvement for large N |

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-core/src/persistent_dag.rs` | **NEW** — PersistentDag implementation |
| `deriva-core/src/dag.rs` | Add `DagAccess` trait, keep existing `DagStore` for tests |
| `deriva-core/src/lib.rs` | Export `persistent_dag` module |
| `deriva-core/Cargo.toml` | Add `sled` and `bincode` dependencies |
| `deriva-storage/src/lib.rs` | Refactor `StorageBackend` to share sled Db, add migration |
| `deriva-storage/src/recipe_store.rs` | Add `from_db(&Db)` constructor |
| `deriva-server/src/state.rs` | Replace `RwLock<DagStore>` with `PersistentDag` |
| `deriva-server/src/service.rs` | Update DAG access (remove RwLock reads) |
| `deriva-core/tests/persistent_dag.rs` | **NEW** — ~14 unit tests |
| `deriva-server/tests/integration.rs` | Add ~2 restart/migration tests |

---

## 9. Checklist

- [ ] Create `PersistentDag` struct with sled trees
- [ ] Implement insert with forward + reverse edge writes
- [ ] Implement contains, inputs, direct_dependents, transitive_dependents
- [ ] Implement resolve_order (topological sort over sled reads)
- [ ] Implement depth with memoization
- [ ] Implement remove with reverse edge cleanup
- [ ] Extract `DagAccess` trait
- [ ] Refactor `StorageBackend` to share sled `Db`
- [ ] Add `from_db` constructor to `SledRecipeStore`
- [ ] Update `ServerState` to use `PersistentDag`
- [ ] Update `DerivaService` to remove `RwLock` on DAG
- [ ] Add migration path for Phase 1 data
- [ ] Write unit tests (~14)
- [ ] Write integration tests (~2)
- [ ] Run full test suite — all 244 + new tests pass
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push

---

## 10. Benchmarking Plan

Before and after measurements to validate the design:

### 10.1 Startup Benchmark

```rust
// benches/dag_startup.rs (using criterion)
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_startup(c: &mut Criterion) {
    let mut group = c.benchmark_group("dag_startup");

    for recipe_count in [100, 1_000, 10_000, 100_000] {
        // Setup: create sled db with N recipes
        let dir = tempfile::TempDir::new().unwrap();
        let db = sled::open(dir.path().join("bench.sled")).unwrap();
        let dag = PersistentDag::open(&db).unwrap();
        let leaf = CAddr::of_leaf(b"bench_leaf");
        for i in 0..recipe_count {
            let r = Recipe { /* ... */ };
            dag.insert(&r).unwrap();
        }
        dag.flush().unwrap();
        drop(dag);

        // Benchmark: open PersistentDag (should be O(1))
        group.bench_with_input(
            BenchmarkId::new("persistent_dag_open", recipe_count),
            &recipe_count,
            |b, _| {
                b.iter(|| {
                    let dag = PersistentDag::open(&db).unwrap();
                    std::hint::black_box(dag.len());
                });
            },
        );

        // Comparison: rebuild from scratch (Phase 1 approach)
        group.bench_with_input(
            BenchmarkId::new("rebuild_from_recipes", recipe_count),
            &recipe_count,
            |b, _| {
                b.iter(|| {
                    let mut dag = DagStore::new();
                    // simulate rebuild
                    for result in recipe_store.iter_all() {
                        let (_, recipe) = result.unwrap();
                        dag.insert(recipe).unwrap();
                    }
                    std::hint::black_box(dag);
                });
            },
        );
    }
    group.finish();
}
```

### 10.2 Expected Results

```
dag_startup/persistent_dag_open/100       time: [800 ns 850 ns 900 ns]
dag_startup/persistent_dag_open/1000      time: [800 ns 860 ns 920 ns]
dag_startup/persistent_dag_open/10000     time: [810 ns 870 ns 930 ns]
dag_startup/persistent_dag_open/100000    time: [820 ns 880 ns 940 ns]
                                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                                          Constant — O(1) regardless of N

dag_startup/rebuild_from_recipes/100      time: [45 μs  50 μs  55 μs]
dag_startup/rebuild_from_recipes/1000     time: [450 μs 500 μs 550 μs]
dag_startup/rebuild_from_recipes/10000    time: [4.5 ms 5.0 ms 5.5 ms]
dag_startup/rebuild_from_recipes/100000   time: [45 ms  50 ms  55 ms]
                                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                                          Linear — O(N)
```

### 10.3 Point Lookup Benchmark

```rust
fn bench_lookups(c: &mut Criterion) {
    // Setup: 10,000 recipes in DAG
    let mut group = c.benchmark_group("dag_lookups");

    group.bench_function("contains_persistent", |b| {
        b.iter(|| dag.contains(&random_addr));
    });

    group.bench_function("contains_inmemory", |b| {
        b.iter(|| memory_dag.contains(&random_addr));
    });

    group.bench_function("direct_dependents_persistent", |b| {
        b.iter(|| dag.direct_dependents(&random_addr));
    });

    group.bench_function("resolve_order_depth5", |b| {
        b.iter(|| dag.resolve_order(&depth5_addr));
    });

    group.finish();
}
```

---

## 11. Design Rationale & Alternatives Considered

### 11.1 Why sled Trees Instead of RocksDB?

| Factor | sled | RocksDB |
|--------|------|---------|
| Rust-native | ✅ Pure Rust | ❌ C++ with Rust bindings |
| Already a dependency | ✅ Used by SledRecipeStore | ❌ Would add new dep |
| Transaction support | ✅ Multi-tree transactions | ✅ WriteBatch |
| Memory overhead | ~50MB baseline | ~100MB baseline |
| Compaction | Automatic | Manual tuning needed |
| Thread safety | Built-in | Built-in |

sled wins because it's already in the dependency tree and provides sufficient performance
for our access patterns (point lookups, small range scans).

### 11.2 Why Not Store Adjacency in the Recipe Itself?

The `Recipe` struct already contains `inputs: Vec<CAddr>`. Why not just read recipes
to get forward edges?

**Problem:** Reverse edges (dependents) are not in the recipe. To find "who depends on X",
we'd need to scan ALL recipes — O(N) per query. The reverse tree gives us O(1) lookups.

### 11.3 Why LRU Instead of Full In-Memory Mirror?

A full mirror (loading all edges into memory on startup) would give O(1) reads but:
- Defeats the purpose of persistent storage (still O(N) startup)
- Uses O(N) memory for edges
- Requires synchronization between memory and disk

LRU gives us:
- O(1) startup (empty cache)
- O(cache_size) memory (bounded)
- Automatic hot-set identification
- No sync issues (cache is populated lazily from sled)

### 11.4 Why Not Use sled's `merge_operator` for Reverse Edges?

Sled supports custom merge operators that can atomically append to values. This would
simplify `add_reverse_edge`:

```rust
reverse.merge(input_key, addr_bytes); // atomically appends
```

**Problem:** The merge operator must be registered at DB open time and applies globally
to the tree. Our serialization format (bincode `Vec<CAddr>`) doesn't support simple
byte concatenation — we need to deserialize, check for duplicates, re-serialize.
A merge operator would need to do this in a callback, which adds complexity without
meaningful performance gain for our write volumes.

### 11.5 Relationship to Phase 2.2 (Async Compute)

The `PersistentDag` is designed to be `Send + Sync` (sled trees are `Send + Sync`).
This is critical for Phase 2.2 where the async executor will access the DAG from
multiple Tokio tasks. The `Mutex<LruCache>` caches are also `Send + Sync`.

The current Phase 1 `DagStore` requires `&mut self` for insert (via `RwLock`), which
creates contention. `PersistentDag` uses sled's internal locking, allowing concurrent
reads without external synchronization.

```
Phase 1 (contention):
  Task A: dag.write().insert(r1)  ──▶ blocks Task B
  Task B: dag.read().resolve(x)   ──▶ waits for A

Phase 2.1 (no contention on reads):
  Task A: dag.insert(r1)          ──▶ sled internal lock (fast)
  Task B: dag.resolve_order(x)    ──▶ sled reads (concurrent OK)
```

---

## 12. Dependency Changes

### 12.1 `deriva-core/Cargo.toml`

```toml
[dependencies]
# Existing
bytes = "1"
blake3 = "1"
serde = { version = "1", features = ["derive"] }
thiserror = "1"

# New for Phase 2.1
sled = "0.34"
bincode = "1"
lru = "0.12"
```

### 12.2 `deriva-storage/Cargo.toml`

No new dependencies — sled and bincode already present. But `StorageBackend` now
depends on `PersistentDag` from `deriva-core`:

```toml
[dependencies]
deriva-core = { path = "../deriva-core" }
# ... existing deps unchanged
```

---

## 13. Sled Tree Size Estimates

For capacity planning, estimate the sled storage overhead:

| Component | Per-Entry Size | 100K Recipes | 1M Recipes |
|-----------|---------------|-------------|------------|
| Forward key | 32 bytes (CAddr) | 3.2 MB | 32 MB |
| Forward value | ~32 × avg_inputs bytes | ~9.6 MB (avg 3 inputs) | ~96 MB |
| Reverse key | 32 bytes (CAddr) | 3.2 MB | 32 MB |
| Reverse value | ~32 × avg_dependents bytes | ~6.4 MB (avg 2 deps) | ~64 MB |
| sled overhead | ~2x raw data | ~44 MB | ~448 MB |
| **Total** | | **~66 MB** | **~672 MB** |

Compare to Phase 1 in-memory: 100K recipes × ~200 bytes/recipe = ~20 MB for recipes alone,
plus ~15 MB for adjacency = ~35 MB. The sled version uses more disk but less RAM
(only LRU cache in memory: 20K entries × ~128 bytes = ~2.5 MB).
