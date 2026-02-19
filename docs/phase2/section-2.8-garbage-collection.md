# §2.8 Garbage Collection

> **Status**: Blueprint  
> **Depends on**: §2.1 Persistent DAG, §2.6 Cascade Invalidation  
> **Crate(s)**: `deriva-core`, `deriva-compute`, `deriva-storage`, `deriva-server`  
> **Estimated effort**: 3–4 days  

---

## 1. Problem Statement

### 1.1 Current Limitation

Deriva has no mechanism to reclaim storage from orphaned data. Over time,
the system accumulates:

1. **Orphaned blobs** — leaf data in `BlobStore` that no recipe references
   and no client remembers the CAddr for
2. **Orphaned recipes** — recipes in `DagStore` whose output is never
   requested and whose inputs have been garbage-collected
3. **Stale cache entries** — cached materializations for recipes that
   have been removed from the DAG
4. **Dangling forward/reverse edges** — DAG edges pointing to removed
   recipes

Currently, the only cleanup mechanism is `EvictableCache::maybe_evict`,
which handles cache pressure but not storage reclamation.

### 1.2 How Orphans Arise

```
Scenario 1: Leaf replacement

  T0: put_leaf("v1") → addr_A
  T1: put_recipe(f, [addr_A]) → addr_X
  T2: get(addr_X) → materializes, caches
  T3: put_leaf("v2") → addr_B  (new version)
  T4: put_recipe(f, [addr_B]) → addr_Y  (new recipe)
  T5: Client forgets addr_A and addr_X

  Orphans: addr_A (blob), addr_X (recipe + cache entry)
  No one will ever request these again.

Scenario 2: Experimental recipes

  T0: put_recipe(f, [A, B]) → addr_X
  T1: put_recipe(g, [A, B]) → addr_Y  (trying different function)
  T2: get(addr_Y) → good result, keep Y
  T3: Client forgets addr_X

  Orphan: addr_X (recipe, never materialized)

Scenario 3: Pipeline evolution

  T0: A → X → Y → Z (pipeline v1)
  T1: A → X → W → Z' (pipeline v2, Y replaced by W)
  T2: Client uses Z' going forward

  Orphans: Y (recipe), Z (recipe + cache)
```

### 1.3 Impact of No GC

```
Storage growth over time (no GC):

  Week 1:  500 recipes, 200 blobs, 100MB storage
  Week 4:  2000 recipes, 800 blobs, 400MB storage
  Week 12: 6000 recipes, 2400 blobs, 1.2GB storage
  Week 52: 26000 recipes, 10400 blobs, 5.2GB storage

  With GC (weekly, 80% orphan rate):
  Week 1:  500 recipes, 200 blobs, 100MB
  Week 4:  800 recipes, 320 blobs, 160MB  (stable)
  Week 12: 800 recipes, 320 blobs, 160MB  (stable)
  Week 52: 800 recipes, 320 blobs, 160MB  (stable)
```

### 1.4 Comparison

| Aspect | No GC | Manual cleanup | Mark-and-sweep (§2.8) |
|--------|-------|---------------|----------------------|
| Storage growth | Unbounded | Requires user discipline | Bounded |
| Complexity | None | Low | Moderate |
| Risk of data loss | None | High (user error) | Low (root set protection) |
| Automation | N/A | N/A | Scheduled or on-demand |
| Dry-run support | N/A | N/A | Yes |
| Concurrent safety | N/A | Unsafe | Safe (snapshot-based) |

### 1.5 What Already Exists

- `BlobStore::get/put` — no delete method yet
- `DagStore::remove` — removes recipe + forward/reverse edges
- `EvictableCache::remove` — removes single cache entry
- `EvictableCache::remove_batch` — removes multiple entries (§2.6)
- `DagStore::recipes` — `HashMap<CAddr, Recipe>` (all recipes)
- `DagStore::forward` — `HashMap<CAddr, Vec<CAddr>>` (recipe → inputs)
- `DagStore::reverse` — `HashMap<CAddr, HashSet<CAddr>>` (input → dependents)

Missing:
- `BlobStore::remove` — delete a blob
- `BlobStore::list_addrs` — enumerate all stored blobs
- Root set definition — which addrs are "in use"
- GC coordinator — orchestrates mark-and-sweep

---

## 2. Design

### 2.1 Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                  Garbage Collector                        │
│                                                           │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐            │
│  │  1. Mark  │───▶│ 2. Sweep │───▶│ 3. Report│            │
│  │          │    │          │    │          │            │
│  │ Build    │    │ Remove   │    │ GcResult │            │
│  │ root set │    │ unmarked │    │ stats    │            │
│  │ Traverse │    │ entries  │    │          │            │
│  │ DAG      │    │          │    │          │            │
│  └──────────┘    └──────────┘    └──────────┘            │
│                                                           │
│  Root Set Sources:                                        │
│  ┌─────────┐  ┌──────────┐  ┌───────────┐               │
│  │ DAG leaf│  │ Pinned   │  │ Recent    │               │
│  │ inputs  │  │ addrs    │  │ puts (TTL)│               │
│  └─────────┘  └──────────┘  └───────────┘               │
└─────────────────────────────────────────────────────────┘
```

### 2.2 Mark-and-Sweep Algorithm

```
Phase 1: MARK

  1. Compute root set:
     a. All CAddrs that appear as inputs in ANY recipe
        (these are "referenced" — either leaves or recipe outputs)
     b. All pinned CAddrs (explicitly protected by user)
     c. All CAddrs put within the last N seconds (grace period)

  2. Starting from root set, traverse forward edges:
     For each root addr:
       Mark addr as "live"
       For each recipe that uses addr as input:
         Mark recipe's output addr as "live"
         Recurse on recipe's output addr

  Actually simpler: ALL addrs reachable from any recipe are live.
  The root set is: union of all recipe inputs + all recipe outputs.

  Orphaned = addrs in BlobStore that are NOT in the root set
             AND not pinned AND not recently put.

Phase 2: SWEEP

  For each addr in BlobStore:
    If addr NOT in live set:
      Remove from BlobStore
      Remove from cache (if present)

  For each recipe in DagStore:
    If recipe output addr NOT in live set:
      Remove recipe from DagStore
      Remove from cache (if present)

  Wait — recipes are always in the live set by definition
  (they're in the DAG). So recipe GC is about removing recipes
  whose INPUTS are all orphaned (no longer resolvable).

  Revised:
    Orphaned recipe = recipe where at least one input is not
    resolvable (not in BlobStore and not another recipe's output).
```

### 2.3 Simplified Model

After analysis, the GC model simplifies to:

```
Live set computation:

  1. All recipe output addrs → live (they're in the DAG)
  2. All recipe input addrs → live (needed to materialize)
  3. All pinned addrs → live
  4. All addrs put within grace period → live

  live_set = { r.output for r in recipes }
           ∪ { input for r in recipes for input in r.inputs }
           ∪ pinned_set
           ∪ recent_set

  Orphaned blobs = { addr in BlobStore } - live_set
  Orphaned recipes = { r in DAG : any input of r is orphaned blob
                       AND r.output not in pinned_set }

  In practice, orphaned recipes are rare because recipe inputs
  are either other recipes (always live) or blobs (may be orphaned).
  A recipe with an orphaned blob input is "broken" — it can never
  be materialized. Safe to remove.
```

### 2.4 GcResult Type

```
┌──────────────────────────────────────┐
│              GcResult                 │
│                                       │
│  blobs_removed: u64                   │
│  recipes_removed: u64                 │
│  cache_entries_removed: u64           │
│  bytes_reclaimed_blobs: u64           │
│  bytes_reclaimed_cache: u64           │
│  total_bytes_reclaimed: u64           │
│  live_blobs: u64                      │
│  live_recipes: u64                    │
│  pinned_count: u64                    │
│  duration: Duration                   │
│  removed_addrs: Vec<CAddr>  (opt)     │
│  dry_run: bool                        │
└──────────────────────────────────────┘
```

### 2.5 Pin Set

Users can "pin" addrs to protect them from GC:

```
┌──────────────────────────────────────┐
│              PinSet                   │
│                                       │
│  pinned: HashSet<CAddr>               │
│                                       │
│  pin(addr) → bool (was new)           │
│  unpin(addr) → bool (was pinned)      │
│  is_pinned(addr) → bool               │
│  count() → usize                      │
│  list() → Vec<CAddr>                  │
└──────────────────────────────────────┘

Use cases:
  - Pin a leaf that's referenced by external systems
  - Pin a recipe output that's served as a stable endpoint
  - Pin during long-running computations to prevent mid-GC removal
```

### 2.6 GC Configuration

```rust
pub struct GcConfig {
    /// Grace period: don't collect addrs put within this duration.
    /// Prevents race between put_leaf and GC.
    pub grace_period: Duration,

    /// Whether to actually delete or just report what would be deleted.
    pub dry_run: bool,

    /// Whether to include removed addrs in the result.
    pub detail_addrs: bool,

    /// Maximum number of blobs to remove in one GC cycle.
    /// 0 = unlimited. Useful to limit GC duration.
    pub max_removals: usize,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            grace_period: Duration::from_secs(300), // 5 minutes
            dry_run: false,
            detail_addrs: false,
            max_removals: 0,
        }
    }
}
```

### 2.7 Sequence Diagram

```
  Client              Server              GC Engine
    │                    │                    │
    │  gc(dry_run=true)  │                    │
    │───────────────────▶│                    │
    │                    │  run_gc(config)     │
    │                    │───────────────────▶│
    │                    │                    │
    │                    │         ┌──────────┤
    │                    │         │ 1. Build │
    │                    │         │ live set │
    │                    │         │ from DAG │
    │                    │         │ + pins   │
    │                    │         └──────────┤
    │                    │                    │
    │                    │         ┌──────────┤
    │                    │         │ 2. Scan  │
    │                    │         │ BlobStore│
    │                    │         │ for      │
    │                    │         │ orphans  │
    │                    │         └──────────┤
    │                    │                    │
    │                    │         ┌──────────┤
    │                    │         │ 3. Sweep │
    │                    │         │ (skip if │
    │                    │         │ dry_run) │
    │                    │         └──────────┤
    │                    │                    │
    │                    │  GcResult           │
    │                    │◀───────────────────│
    │  GcResponse        │                    │
    │◀───────────────────│                    │
```

---

## 3. Implementation

### 3.1 GcResult and GcConfig Types

Location: `crates/deriva-core/src/gc.rs`

```rust
use std::time::Duration;
use crate::CAddr;

/// Result of a garbage collection cycle.
#[derive(Debug, Clone)]
pub struct GcResult {
    /// Number of orphaned blobs removed (or would be removed in dry run).
    pub blobs_removed: u64,
    /// Number of orphaned recipes removed.
    pub recipes_removed: u64,
    /// Number of stale cache entries removed.
    pub cache_entries_removed: u64,
    /// Bytes reclaimed from blob store.
    pub bytes_reclaimed_blobs: u64,
    /// Bytes reclaimed from cache.
    pub bytes_reclaimed_cache: u64,
    /// Total bytes reclaimed.
    pub total_bytes_reclaimed: u64,
    /// Number of live (non-orphaned) blobs.
    pub live_blobs: u64,
    /// Number of live recipes.
    pub live_recipes: u64,
    /// Number of pinned addrs.
    pub pinned_count: u64,
    /// Time taken for the GC cycle.
    pub duration: Duration,
    /// Addrs that were removed (only populated if detail_addrs=true).
    pub removed_addrs: Vec<CAddr>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// Configuration for a GC cycle.
#[derive(Debug, Clone)]
pub struct GcConfig {
    /// Don't collect addrs put within this duration.
    pub grace_period: Duration,
    /// Report only, don't actually delete.
    pub dry_run: bool,
    /// Include removed addrs in result.
    pub detail_addrs: bool,
    /// Max blobs to remove (0 = unlimited).
    pub max_removals: usize,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            grace_period: Duration::from_secs(300),
            dry_run: false,
            detail_addrs: false,
            max_removals: 0,
        }
    }
}
```

### 3.2 PinSet

Location: `crates/deriva-core/src/gc.rs`

```rust
use std::collections::HashSet;

/// A set of pinned CAddrs protected from garbage collection.
#[derive(Debug, Clone, Default)]
pub struct PinSet {
    pinned: HashSet<CAddr>,
}

impl PinSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pin an addr. Returns true if it was newly pinned.
    pub fn pin(&mut self, addr: CAddr) -> bool {
        self.pinned.insert(addr)
    }

    /// Unpin an addr. Returns true if it was previously pinned.
    pub fn unpin(&mut self, addr: &CAddr) -> bool {
        self.pinned.remove(addr)
    }

    /// Check if an addr is pinned.
    pub fn is_pinned(&self, addr: &CAddr) -> bool {
        self.pinned.contains(addr)
    }

    /// Number of pinned addrs.
    pub fn count(&self) -> usize {
        self.pinned.len()
    }

    /// List all pinned addrs.
    pub fn list(&self) -> Vec<CAddr> {
        self.pinned.iter().copied().collect()
    }

    /// Get reference to the inner set (for live set computation).
    pub fn as_set(&self) -> &HashSet<CAddr> {
        &self.pinned
    }
}
```

### 3.3 BlobStore Extensions

Location: `crates/deriva-storage/src/blob_store.rs` — add remove + list

```rust
impl BlobStore {
    /// Remove a blob by its CAddr.
    /// Returns the size of the removed blob, or 0 if not found.
    pub fn remove(&self, addr: &CAddr) -> std::io::Result<u64> {
        let hex = hex::encode(addr.as_bytes());
        let path = self.base_path
            .join(&hex[..2])
            .join(&hex[2..4])
            .join(&hex);

        match std::fs::metadata(&path) {
            Ok(meta) => {
                let size = meta.len();
                std::fs::remove_file(&path)?;
                Ok(size)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(e),
        }
    }

    /// Remove multiple blobs. Returns (count_removed, bytes_removed).
    pub fn remove_batch(
        &self,
        addrs: &[CAddr],
    ) -> std::io::Result<(u64, u64)> {
        let mut count = 0u64;
        let mut bytes = 0u64;
        for addr in addrs {
            let size = self.remove(addr)?;
            if size > 0 {
                count += 1;
                bytes += size;
            }
        }
        Ok((count, bytes))
    }

    /// List all blob CAddrs in the store.
    ///
    /// Walks the 2-level shard directory structure and parses
    /// filenames back into CAddrs.
    ///
    /// Note: This is O(N) where N = number of blobs. For large
    /// stores, consider caching or incremental enumeration.
    pub fn list_addrs(&self) -> std::io::Result<Vec<CAddr>> {
        let mut addrs = Vec::new();

        // Walk: base/XX/YY/HASH
        for shard1 in std::fs::read_dir(&self.base_path)? {
            let shard1 = shard1?;
            if !shard1.file_type()?.is_dir() { continue; }

            for shard2 in std::fs::read_dir(shard1.path())? {
                let shard2 = shard2?;
                if !shard2.file_type()?.is_dir() { continue; }

                for entry in std::fs::read_dir(shard2.path())? {
                    let entry = entry?;
                    if !entry.file_type()?.is_file() { continue; }

                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if let Ok(bytes) = hex::decode(name_str.as_ref()) {
                        if let Ok(addr) = CAddr::try_from(bytes.as_slice()) {
                            addrs.push(addr);
                        }
                    }
                }
            }
        }

        Ok(addrs)
    }

    /// Count total blobs and bytes stored.
    pub fn stats(&self) -> std::io::Result<(u64, u64)> {
        let mut count = 0u64;
        let mut bytes = 0u64;

        for shard1 in std::fs::read_dir(&self.base_path)? {
            let shard1 = shard1?;
            if !shard1.file_type()?.is_dir() { continue; }
            for shard2 in std::fs::read_dir(shard1.path())? {
                let shard2 = shard2?;
                if !shard2.file_type()?.is_dir() { continue; }
                for entry in std::fs::read_dir(shard2.path())? {
                    let entry = entry?;
                    if entry.file_type()?.is_file() {
                        count += 1;
                        bytes += entry.metadata()?.len();
                    }
                }
            }
        }

        Ok((count, bytes))
    }
}
```

### 3.4 Live Set Computation

Location: `crates/deriva-compute/src/gc.rs`

```rust
use std::collections::HashSet;
use std::time::{Duration, Instant, SystemTime};
use deriva_core::{CAddr, DerivaError};
use deriva_core::dag::DagStore;
use deriva_core::gc::{GcConfig, GcResult, PinSet};

/// Compute the set of "live" CAddrs that must not be garbage collected.
///
/// Live set = all addrs referenced by any recipe (inputs + outputs)
///          ∪ pinned addrs
///
/// The grace_period is handled separately during sweep (check blob
/// modification time).
pub fn compute_live_set(
    dag: &DagStore,
    pins: &PinSet,
) -> HashSet<CAddr> {
    let mut live = HashSet::new();

    // All recipe outputs and their inputs are live
    for (output_addr, recipe) in dag.iter_recipes() {
        live.insert(*output_addr);
        for input_addr in &recipe.inputs {
            live.insert(*input_addr);
        }
    }

    // All pinned addrs are live
    for addr in pins.as_set() {
        live.insert(*addr);
    }

    live
}
```

### 3.5 GarbageCollector

Location: `crates/deriva-compute/src/gc.rs`

```rust
use deriva_core::cache::EvictableCache;
use deriva_storage::BlobStore;

/// Orchestrates mark-and-sweep garbage collection.
pub struct GarbageCollector;

impl GarbageCollector {
    /// Run a garbage collection cycle.
    ///
    /// # Algorithm
    /// 1. Compute live set from DAG + pins
    /// 2. Enumerate all blobs in BlobStore
    /// 3. Identify orphaned blobs (in store but not in live set)
    /// 4. Optionally filter by grace period
    /// 5. Remove orphaned blobs (unless dry_run)
    /// 6. Remove orphaned recipes (inputs reference removed blobs)
    /// 7. Remove stale cache entries
    /// 8. Return GcResult
    pub fn run(
        dag: &DagStore,
        blob_store: &BlobStore,
        cache: &mut EvictableCache,
        pins: &PinSet,
        config: &GcConfig,
    ) -> Result<GcResult, DerivaError> {
        let start = Instant::now();

        // Step 1: Compute live set
        let live_set = compute_live_set(dag, pins);

        // Step 2: Enumerate all blobs
        let all_blobs = blob_store.list_addrs()
            .map_err(|e| DerivaError::Storage(e.to_string()))?;

        // Step 3: Identify orphaned blobs
        let mut orphaned_blobs: Vec<CAddr> = all_blobs.iter()
            .filter(|addr| !live_set.contains(addr))
            .copied()
            .collect();

        // Step 4: Apply max_removals limit
        if config.max_removals > 0 && orphaned_blobs.len() > config.max_removals {
            orphaned_blobs.truncate(config.max_removals);
        }

        // Step 5: Sweep blobs
        let (blobs_removed, bytes_reclaimed_blobs) = if config.dry_run {
            // Dry run: count what would be removed
            let mut bytes = 0u64;
            for addr in &orphaned_blobs {
                if let Some(data) = blob_store.get(addr)
                    .map_err(|e| DerivaError::Storage(e.to_string()))?
                {
                    bytes += data.len() as u64;
                }
            }
            (orphaned_blobs.len() as u64, bytes)
        } else {
            blob_store.remove_batch(&orphaned_blobs)
                .map_err(|e| DerivaError::Storage(e.to_string()))?
        };

        // Step 6: Find and remove orphaned recipes
        // A recipe is orphaned if any of its inputs was just removed
        let removed_blob_set: HashSet<CAddr> =
            orphaned_blobs.iter().copied().collect();
        let mut orphaned_recipes = Vec::new();

        for (output_addr, recipe) in dag.iter_recipes() {
            let has_removed_input = recipe.inputs.iter()
                .any(|input| removed_blob_set.contains(input));
            if has_removed_input && !pins.is_pinned(output_addr) {
                orphaned_recipes.push(*output_addr);
            }
        }

        let recipes_removed = orphaned_recipes.len() as u64;

        // Step 7: Remove stale cache entries for removed blobs + recipes
        let mut all_removed: Vec<CAddr> = orphaned_blobs.clone();
        all_removed.extend_from_slice(&orphaned_recipes);

        let (cache_entries_removed, bytes_reclaimed_cache, _) = if config.dry_run {
            // Count cached entries that would be removed
            let count = all_removed.iter()
                .filter(|addr| cache.contains(addr))
                .count() as u64;
            (count, 0u64, vec![])
        } else {
            cache.remove_batch(&all_removed)
        };

        // Step 8: Actually remove recipes from DAG (after cache cleanup)
        if !config.dry_run {
            // Note: DagStore::remove is called per recipe
            // A batch remove could be added as optimization
            for addr in &orphaned_recipes {
                dag.remove(addr);
            }
        }

        let duration = start.elapsed();

        Ok(GcResult {
            blobs_removed,
            recipes_removed,
            cache_entries_removed,
            bytes_reclaimed_blobs,
            bytes_reclaimed_cache,
            total_bytes_reclaimed: bytes_reclaimed_blobs + bytes_reclaimed_cache,
            live_blobs: (all_blobs.len() as u64).saturating_sub(blobs_removed),
            live_recipes: dag.recipe_count() as u64 - recipes_removed,
            pinned_count: pins.count() as u64,
            duration,
            removed_addrs: if config.detail_addrs {
                all_removed
            } else {
                vec![]
            },
            dry_run: config.dry_run,
        })
    }
}
```

### 3.6 Async GC Wrapper

Location: `crates/deriva-compute/src/gc.rs`

```rust
use tokio::sync::RwLock;
use std::sync::Arc;

/// Async wrapper for GC that acquires locks appropriately.
///
/// Lock ordering: DAG write → cache write → blob store (no lock)
/// DAG needs write lock because we may remove recipes.
/// Cache needs write lock because we remove entries.
pub async fn run_gc_async(
    dag: &RwLock<DagStore>,
    blob_store: &BlobStore,
    cache: &RwLock<EvictableCache>,
    pins: &RwLock<PinSet>,
    config: &GcConfig,
) -> Result<GcResult, DerivaError> {
    // Acquire all locks
    // Note: GC is a heavyweight operation — holding write locks is acceptable
    // because GC runs infrequently (minutes/hours between cycles)
    let dag_guard = dag.write().await;
    let mut cache_guard = cache.write().await;
    let pins_guard = pins.read().await;

    GarbageCollector::run(
        &dag_guard,
        blob_store,
        &mut cache_guard,
        &pins_guard,
        config,
    )
}
```


### 3.7 Proto Definition

Location: `proto/deriva.proto`

```protobuf
service Deriva {
    // ... existing RPCs ...

    // Garbage collection
    rpc GarbageCollect(GcRequest) returns (GcResponse);

    // Pin management
    rpc Pin(PinRequest) returns (PinResponse);
    rpc Unpin(UnpinRequest) returns (UnpinResponse);
    rpc ListPins(ListPinsRequest) returns (ListPinsResponse);
}

message GcRequest {
    uint64 grace_period_secs = 1;
    bool dry_run = 2;
    bool detail_addrs = 3;
    uint64 max_removals = 4;
}

message GcResponse {
    uint64 blobs_removed = 1;
    uint64 recipes_removed = 2;
    uint64 cache_entries_removed = 3;
    uint64 bytes_reclaimed_blobs = 4;
    uint64 bytes_reclaimed_cache = 5;
    uint64 total_bytes_reclaimed = 6;
    uint64 live_blobs = 7;
    uint64 live_recipes = 8;
    uint64 pinned_count = 9;
    uint64 duration_micros = 10;
    repeated bytes removed_addrs = 11;
    bool dry_run = 12;
}

message PinRequest {
    bytes addr = 1;
}

message PinResponse {
    bool was_new = 1;
}

message UnpinRequest {
    bytes addr = 1;
}

message UnpinResponse {
    bool was_pinned = 1;
}

message ListPinsRequest {}

message ListPinsResponse {
    repeated bytes addrs = 1;
    uint64 count = 2;
}
```

### 3.8 Service Implementation

Location: `crates/deriva-server/src/service.rs`

```rust
use deriva_compute::gc::{run_gc_async, GarbageCollector};
use deriva_core::gc::{GcConfig, PinSet};

// Add to ServerState:
pub struct ServerState {
    // ... existing fields ...
    pub pins: RwLock<PinSet>,
}

#[tonic::async_trait]
impl Deriva for DerivaService {
    async fn garbage_collect(
        &self,
        request: Request<GcRequest>,
    ) -> Result<Response<GcResponse>, Status> {
        let req = request.get_ref();
        let config = GcConfig {
            grace_period: Duration::from_secs(req.grace_period_secs),
            dry_run: req.dry_run,
            detail_addrs: req.detail_addrs,
            max_removals: req.max_removals as usize,
        };

        let result = run_gc_async(
            &self.state.dag,
            &self.state.blob_store,
            &self.state.cache,
            &self.state.pins,
            &config,
        ).await.map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(GcResponse {
            blobs_removed: result.blobs_removed,
            recipes_removed: result.recipes_removed,
            cache_entries_removed: result.cache_entries_removed,
            bytes_reclaimed_blobs: result.bytes_reclaimed_blobs,
            bytes_reclaimed_cache: result.bytes_reclaimed_cache,
            total_bytes_reclaimed: result.total_bytes_reclaimed,
            live_blobs: result.live_blobs,
            live_recipes: result.live_recipes,
            pinned_count: result.pinned_count,
            duration_micros: result.duration.as_micros() as u64,
            removed_addrs: result.removed_addrs.iter()
                .map(|a| a.as_bytes().to_vec())
                .collect(),
            dry_run: result.dry_run,
        }))
    }

    async fn pin(
        &self,
        request: Request<PinRequest>,
    ) -> Result<Response<PinResponse>, Status> {
        let addr = parse_addr(&request.get_ref().addr)?;
        let mut pins = self.state.pins.write().await;
        let was_new = pins.pin(addr);
        Ok(Response::new(PinResponse { was_new }))
    }

    async fn unpin(
        &self,
        request: Request<UnpinRequest>,
    ) -> Result<Response<UnpinResponse>, Status> {
        let addr = parse_addr(&request.get_ref().addr)?;
        let mut pins = self.state.pins.write().await;
        let was_pinned = pins.unpin(&addr);
        Ok(Response::new(UnpinResponse { was_pinned }))
    }

    async fn list_pins(
        &self,
        _request: Request<ListPinsRequest>,
    ) -> Result<Response<ListPinsResponse>, Status> {
        let pins = self.state.pins.read().await;
        let addrs: Vec<Vec<u8>> = pins.list().iter()
            .map(|a| a.as_bytes().to_vec())
            .collect();
        let count = addrs.len() as u64;
        Ok(Response::new(ListPinsResponse { addrs, count }))
    }
}
```

### 3.9 CLI Integration

Location: `crates/deriva-cli/src/main.rs`

```rust
#[derive(Subcommand)]
enum Commands {
    // ... existing ...

    /// Run garbage collection
    Gc {
        /// Dry run: show what would be removed
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        /// Grace period in seconds (default: 300)
        #[arg(long, default_value_t = 300)]
        grace_period: u64,
        /// Show addresses of removed entries
        #[arg(long, default_value_t = false)]
        detail: bool,
        /// Max blobs to remove (0 = unlimited)
        #[arg(long, default_value_t = 0)]
        max_removals: u64,
    },

    /// Pin an addr to protect from GC
    Pin {
        addr: String,
    },

    /// Unpin an addr
    Unpin {
        addr: String,
    },

    /// List all pinned addrs
    ListPins,
}

// Handlers:
Commands::Gc { dry_run, grace_period, detail, max_removals } => {
    let resp = client.garbage_collect(GcRequest {
        grace_period_secs: grace_period,
        dry_run,
        detail_addrs: detail,
        max_removals,
    }).await?.into_inner();

    let mode = if resp.dry_run { "DRY RUN" } else { "COMPLETED" };
    println!("Garbage Collection {}", mode);
    println!("  Blobs removed:   {}", resp.blobs_removed);
    println!("  Recipes removed: {}", resp.recipes_removed);
    println!("  Cache cleared:   {}", resp.cache_entries_removed);
    println!("  Bytes reclaimed: {} ({} blobs + {} cache)",
        resp.total_bytes_reclaimed,
        resp.bytes_reclaimed_blobs,
        resp.bytes_reclaimed_cache);
    println!("  Live blobs:      {}", resp.live_blobs);
    println!("  Live recipes:    {}", resp.live_recipes);
    println!("  Pinned addrs:    {}", resp.pinned_count);
    println!("  Duration:        {}μs", resp.duration_micros);
    if detail {
        for a in &resp.removed_addrs {
            println!("  - {}", hex::encode(a));
        }
    }
}

Commands::Pin { addr } => {
    let resp = client.pin(PinRequest {
        addr: hex::decode(&addr)?,
    }).await?.into_inner();
    println!("pinned: {} (new: {})", addr, resp.was_new);
}

Commands::Unpin { addr } => {
    let resp = client.unpin(UnpinRequest {
        addr: hex::decode(&addr)?,
    }).await?.into_inner();
    println!("unpinned: {} (was_pinned: {})", addr, resp.was_pinned);
}

Commands::ListPins => {
    let resp = client.list_pins(ListPinsRequest {}).await?.into_inner();
    println!("Pinned addrs ({}):", resp.count);
    for a in &resp.addrs {
        println!("  {}", hex::encode(a));
    }
}
```

CLI usage:

```bash
# Dry run — see what would be collected
deriva gc --dry-run

# Run GC with default settings
deriva gc

# Run GC with detail and custom grace period
deriva gc --detail --grace-period 60

# Limit removals per cycle
deriva gc --max-removals 1000

# Pin/unpin management
deriva pin 0xabc123
deriva unpin 0xabc123
deriva list-pins
```

---

## 4. Data Flow Diagrams

### 4.1 Mark-and-Sweep Walkthrough

```
Initial state:

  BlobStore: [A, B, C, D, E]
  DAG recipes:
    X = f(A, B)
    Y = g(B, C)
  Cache: { X: ✓, Y: ✓, D: ✓ }
  Pins: { E }

Step 1: MARK — compute live set

  From recipes:
    X inputs: {A, B}     → live
    X output: {X}         → live
    Y inputs: {B, C}     → live
    Y output: {Y}         → live

  From pins: {E}          → live

  live_set = {A, B, C, X, Y, E}

Step 2: Identify orphans

  BlobStore addrs: {A, B, C, D, E}
  Orphaned = {A, B, C, D, E} - {A, B, C, X, Y, E}
           = {D}

  D is in BlobStore but not referenced by any recipe and not pinned.

Step 3: SWEEP

  Remove D from BlobStore
  Remove D from cache (was cached)
  No orphaned recipes (X and Y inputs are all live)

Result:
  blobs_removed: 1 (D)
  recipes_removed: 0
  cache_entries_removed: 1 (D)
  live_blobs: 4 (A, B, C, E)
  live_recipes: 2 (X, Y)
```

### 4.2 Cascading Recipe Orphan

```
Initial state:

  BlobStore: [A, B]
  DAG:
    X = f(A)
    Y = g(X, B)
  Pins: {}

  live_set = {A, X, B, Y}
  Orphaned blobs: none

Now: external process deletes blob A from disk (corruption/manual)

  BlobStore: [B]  (A is gone)
  DAG still has: X = f(A), Y = g(X, B)

  GC runs:
    live_set = {A, X, B, Y}  (computed from DAG)
    BlobStore addrs = {B}
    Orphaned blobs = {} (B is live, A is not in store)

  But X = f(A) is now broken — A doesn't exist!
  And Y = g(X, B) depends on broken X.

  This is detected by materialization (get(Y) fails),
  not by GC. GC only removes what's in the store but
  not referenced. Missing inputs are a different problem.

  Future enhancement: "integrity check" mode that verifies
  all recipe inputs are resolvable.
```

### 4.3 GC with Grace Period

```
Timeline:

  T0: put_leaf("data") → addr_A
  T1: GC starts (grace_period = 300s)
  T2: GC computes live set — A is NOT in any recipe yet
  T3: GC checks A's creation time: T0 (< 300s ago)
  T4: GC skips A (within grace period)
  T5: put_recipe(f, [A]) → addr_X  (A is now referenced)
  T6: GC runs again — A is in live set, safe

  Without grace period:
  T3: GC would delete A
  T5: put_recipe(f, [A]) → recipe references deleted blob!
  T6: get(X) fails — input A not found

  Grace period prevents race between put_leaf and put_recipe.
```

### 4.4 Dry Run vs Actual GC

```
  Dry run:                          Actual:
  ┌──────────┐                      ┌──────────┐
  │ Compute  │                      │ Compute  │
  │ live set │                      │ live set │
  └────┬─────┘                      └────┬─────┘
       │                                  │
  ┌────▼─────┐                      ┌────▼─────┐
  │ Find     │                      │ Find     │
  │ orphans  │                      │ orphans  │
  └────┬─────┘                      └────┬─────┘
       │                                  │
  ┌────▼─────┐                      ┌────▼─────┐
  │ Count    │                      │ DELETE   │
  │ sizes    │                      │ blobs    │
  │ (no del) │                      │ recipes  │
  └────┬─────┘                      │ cache    │
       │                            └────┬─────┘
  ┌────▼─────┐                      ┌────▼─────┐
  │ Report   │                      │ Report   │
  │ GcResult │                      │ GcResult │
  └──────────┘                      └──────────┘
```

---

## 5. Test Specification

### 5.1 Unit Tests — PinSet

```rust
#[test]
fn test_pin_new() {
    let mut pins = PinSet::new();
    let a = test_addr("a");
    assert!(pins.pin(a));       // new
    assert!(!pins.pin(a));      // already pinned
    assert_eq!(pins.count(), 1);
}

#[test]
fn test_unpin() {
    let mut pins = PinSet::new();
    let a = test_addr("a");
    pins.pin(a);
    assert!(pins.unpin(&a));    // was pinned
    assert!(!pins.unpin(&a));   // already unpinned
    assert_eq!(pins.count(), 0);
}

#[test]
fn test_is_pinned() {
    let mut pins = PinSet::new();
    let a = test_addr("a");
    assert!(!pins.is_pinned(&a));
    pins.pin(a);
    assert!(pins.is_pinned(&a));
}

#[test]
fn test_list_pins() {
    let mut pins = PinSet::new();
    let a = test_addr("a");
    let b = test_addr("b");
    pins.pin(a);
    pins.pin(b);
    let list = pins.list();
    assert_eq!(list.len(), 2);
    assert!(list.contains(&a));
    assert!(list.contains(&b));
}
```

### 5.2 Unit Tests — BlobStore Extensions

```rust
#[test]
fn test_blob_remove() {
    let store = temp_blob_store();
    let addr = store.put(&Bytes::from("hello")).unwrap();

    assert!(store.get(&addr).unwrap().is_some());
    let size = store.remove(&addr).unwrap();
    assert_eq!(size, 5);
    assert!(store.get(&addr).unwrap().is_none());
}

#[test]
fn test_blob_remove_not_found() {
    let store = temp_blob_store();
    let addr = test_addr("nonexistent");
    let size = store.remove(&addr).unwrap();
    assert_eq!(size, 0);
}

#[test]
fn test_blob_remove_batch() {
    let store = temp_blob_store();
    let a = store.put(&Bytes::from("aaa")).unwrap();
    let b = store.put(&Bytes::from("bbb")).unwrap();
    let c = store.put(&Bytes::from("ccc")).unwrap();

    let (count, bytes) = store.remove_batch(&[a, b]).unwrap();
    assert_eq!(count, 2);
    assert_eq!(bytes, 6);
    assert!(store.get(&c).unwrap().is_some()); // c untouched
}

#[test]
fn test_blob_list_addrs() {
    let store = temp_blob_store();
    let a = store.put(&Bytes::from("aaa")).unwrap();
    let b = store.put(&Bytes::from("bbb")).unwrap();

    let addrs = store.list_addrs().unwrap();
    assert_eq!(addrs.len(), 2);
    assert!(addrs.contains(&a));
    assert!(addrs.contains(&b));
}

#[test]
fn test_blob_list_addrs_empty() {
    let store = temp_blob_store();
    let addrs = store.list_addrs().unwrap();
    assert!(addrs.is_empty());
}

#[test]
fn test_blob_stats() {
    let store = temp_blob_store();
    store.put(&Bytes::from(vec![0u8; 100])).unwrap();
    store.put(&Bytes::from(vec![0u8; 200])).unwrap();

    let (count, bytes) = store.stats().unwrap();
    assert_eq!(count, 2);
    assert_eq!(bytes, 300);
}
```

### 5.3 Unit Tests — Live Set Computation

```rust
#[test]
fn test_live_set_basic() {
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    let b = leaf_addr("b");
    let x = insert_recipe(&mut dag, "f", vec![a, b]);

    let pins = PinSet::new();
    let live = compute_live_set(&dag, &pins);

    assert!(live.contains(&a));  // input
    assert!(live.contains(&b));  // input
    assert!(live.contains(&x));  // output
    assert_eq!(live.len(), 3);
}

#[test]
fn test_live_set_with_pins() {
    let dag = DagStore::new(); // empty
    let mut pins = PinSet::new();
    let p = test_addr("pinned");
    pins.pin(p);

    let live = compute_live_set(&dag, &pins);
    assert!(live.contains(&p));
    assert_eq!(live.len(), 1);
}

#[test]
fn test_live_set_empty() {
    let dag = DagStore::new();
    let pins = PinSet::new();
    let live = compute_live_set(&dag, &pins);
    assert!(live.is_empty());
}

#[test]
fn test_live_set_chain() {
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    let x = insert_recipe(&mut dag, "f1", vec![a]);
    let y = insert_recipe(&mut dag, "f2", vec![x]);

    let pins = PinSet::new();
    let live = compute_live_set(&dag, &pins);

    assert!(live.contains(&a));
    assert!(live.contains(&x));
    assert!(live.contains(&y));
    assert_eq!(live.len(), 3);
}
```

### 5.4 Unit Tests — GarbageCollector

```rust
#[test]
fn test_gc_removes_orphaned_blob() {
    let mut dag = DagStore::new();
    let blob_store = temp_blob_store();
    let mut cache = EvictableCache::new(1024 * 1024);
    let pins = PinSet::new();

    // Put two blobs, only reference one in a recipe
    let a = blob_store.put(&Bytes::from("used")).unwrap();
    let orphan = blob_store.put(&Bytes::from("orphan")).unwrap();
    let _x = insert_recipe(&mut dag, "f", vec![a]);

    let result = GarbageCollector::run(
        &dag, &blob_store, &mut cache, &pins,
        &GcConfig::default(),
    ).unwrap();

    assert_eq!(result.blobs_removed, 1);
    assert!(blob_store.get(&a).unwrap().is_some());    // kept
    assert!(blob_store.get(&orphan).unwrap().is_none()); // removed
}

#[test]
fn test_gc_respects_pins() {
    let dag = DagStore::new(); // empty — no recipes
    let blob_store = temp_blob_store();
    let mut cache = EvictableCache::new(1024 * 1024);
    let mut pins = PinSet::new();

    let a = blob_store.put(&Bytes::from("pinned_data")).unwrap();
    pins.pin(a);

    let result = GarbageCollector::run(
        &dag, &blob_store, &mut cache, &pins,
        &GcConfig::default(),
    ).unwrap();

    assert_eq!(result.blobs_removed, 0); // pinned, not removed
    assert!(blob_store.get(&a).unwrap().is_some());
}

#[test]
fn test_gc_dry_run() {
    let dag = DagStore::new();
    let blob_store = temp_blob_store();
    let mut cache = EvictableCache::new(1024 * 1024);
    let pins = PinSet::new();

    let orphan = blob_store.put(&Bytes::from("orphan")).unwrap();

    let result = GarbageCollector::run(
        &dag, &blob_store, &mut cache, &pins,
        &GcConfig { dry_run: true, ..Default::default() },
    ).unwrap();

    assert_eq!(result.blobs_removed, 1);
    assert!(result.dry_run);
    // Blob still exists!
    assert!(blob_store.get(&orphan).unwrap().is_some());
}

#[test]
fn test_gc_clears_cache_entries() {
    let mut dag = DagStore::new();
    let blob_store = temp_blob_store();
    let mut cache = EvictableCache::new(1024 * 1024);
    let pins = PinSet::new();

    let orphan = blob_store.put(&Bytes::from("orphan")).unwrap();
    cache.put_simple(orphan, Bytes::from("cached_orphan"));

    let result = GarbageCollector::run(
        &dag, &blob_store, &mut cache, &pins,
        &GcConfig::default(),
    ).unwrap();

    assert_eq!(result.blobs_removed, 1);
    assert_eq!(result.cache_entries_removed, 1);
    assert!(!cache.contains(&orphan));
}

#[test]
fn test_gc_no_orphans() {
    let mut dag = DagStore::new();
    let blob_store = temp_blob_store();
    let mut cache = EvictableCache::new(1024 * 1024);
    let pins = PinSet::new();

    let a = blob_store.put(&Bytes::from("used")).unwrap();
    let _x = insert_recipe(&mut dag, "f", vec![a]);

    let result = GarbageCollector::run(
        &dag, &blob_store, &mut cache, &pins,
        &GcConfig::default(),
    ).unwrap();

    assert_eq!(result.blobs_removed, 0);
    assert_eq!(result.recipes_removed, 0);
}

#[test]
fn test_gc_max_removals() {
    let dag = DagStore::new();
    let blob_store = temp_blob_store();
    let mut cache = EvictableCache::new(1024 * 1024);
    let pins = PinSet::new();

    // Create 10 orphaned blobs
    for i in 0..10 {
        blob_store.put(&Bytes::from(format!("orphan_{}", i))).unwrap();
    }

    let result = GarbageCollector::run(
        &dag, &blob_store, &mut cache, &pins,
        &GcConfig { max_removals: 3, ..Default::default() },
    ).unwrap();

    assert_eq!(result.blobs_removed, 3); // limited to 3
    let (remaining, _) = blob_store.stats().unwrap();
    assert_eq!(remaining, 7);
}

#[test]
fn test_gc_detail_addrs() {
    let dag = DagStore::new();
    let blob_store = temp_blob_store();
    let mut cache = EvictableCache::new(1024 * 1024);
    let pins = PinSet::new();

    let orphan = blob_store.put(&Bytes::from("orphan")).unwrap();

    let result = GarbageCollector::run(
        &dag, &blob_store, &mut cache, &pins,
        &GcConfig { detail_addrs: true, ..Default::default() },
    ).unwrap();

    assert_eq!(result.removed_addrs.len(), 1);
    assert_eq!(result.removed_addrs[0], orphan);
}
```

### 5.5 Integration Tests

```rust
#[tokio::test]
async fn test_gc_rpc_end_to_end() {
    let (mut client, _server) = start_test_server().await;

    // Put a leaf (will become orphan — no recipe references it)
    let resp = client.put_leaf(PutLeafRequest {
        data: b"orphan_data".to_vec(),
    }).await.unwrap().into_inner();
    let orphan_addr = resp.addr;

    // Put a leaf + recipe (referenced — should survive GC)
    let used = client.put_leaf(PutLeafRequest {
        data: b"used_data".to_vec(),
    }).await.unwrap().into_inner();
    let _recipe = client.put_recipe(PutRecipeRequest {
        function_name: "identity".into(),
        function_version: "v1".into(),
        inputs: vec![used.addr.clone()],
        params: Default::default(),
    }).await.unwrap();

    // Run GC
    let gc_resp = client.garbage_collect(GcRequest {
        grace_period_secs: 0, // no grace period for test
        dry_run: false,
        detail_addrs: true,
        max_removals: 0,
    }).await.unwrap().into_inner();

    assert_eq!(gc_resp.blobs_removed, 1);
    assert!(gc_resp.removed_addrs.contains(&orphan_addr));
}

#[tokio::test]
async fn test_pin_protects_from_gc() {
    let (mut client, _server) = start_test_server().await;

    let resp = client.put_leaf(PutLeafRequest {
        data: b"pinned_data".to_vec(),
    }).await.unwrap().into_inner();
    let addr = resp.addr;

    // Pin it
    client.pin(PinRequest { addr: addr.clone() }).await.unwrap();

    // Run GC
    let gc_resp = client.garbage_collect(GcRequest {
        grace_period_secs: 0,
        dry_run: false,
        detail_addrs: false,
        max_removals: 0,
    }).await.unwrap().into_inner();

    assert_eq!(gc_resp.blobs_removed, 0); // pinned, not removed

    // Unpin and GC again
    client.unpin(UnpinRequest { addr: addr.clone() }).await.unwrap();

    let gc_resp2 = client.garbage_collect(GcRequest {
        grace_period_secs: 0,
        dry_run: false,
        detail_addrs: false,
        max_removals: 0,
    }).await.unwrap().into_inner();

    assert_eq!(gc_resp2.blobs_removed, 1); // now removed
}
```


---

## 6. Edge Cases & Error Handling

| Case | Behavior | Rationale |
|------|----------|-----------|
| Empty BlobStore | GC completes instantly, 0 removals | Nothing to collect |
| Empty DAG (no recipes) | All blobs are orphans (unless pinned) | No recipes = no references |
| All blobs are live | 0 removals | Nothing to collect |
| All blobs are orphaned | All removed (respecting pins + grace) | Correct behavior |
| Blob referenced by multiple recipes | Stays live (in live set from any recipe) | Union of all references |
| Pin + unpin race with GC | Pin checked under lock — consistent | RwLock on PinSet |
| put_leaf during GC | Grace period protects new blobs | 5-minute default window |
| put_recipe during GC | DAG write lock blocks until GC completes | GC holds DAG write lock |
| BlobStore I/O error during sweep | GC returns error, partial sweep may have occurred | Caller should retry |
| Corrupted blob filename | Skipped during list_addrs (hex decode fails) | Defensive parsing |
| Very large BlobStore (1M blobs) | list_addrs is O(N) — may take seconds | Acceptable for infrequent GC |
| max_removals = 1 | Removes exactly 1 orphan per cycle | Incremental GC |
| Concurrent GC + cascade_invalidate | GC holds write locks — cascade blocks | Serialized by locks |
| Recipe with self-reference | Impossible — DagStore rejects cycles | DAG invariant |

### 6.1 Race: put_leaf → GC → put_recipe

```
Without grace period:

  T0: Client calls put_leaf("data") → addr_A
  T1: GC starts, computes live set (A not in any recipe)
  T2: GC removes A from BlobStore
  T3: Client calls put_recipe(f, [A]) → addr_X
  T4: Client calls get(X) → ERROR: input A not found!

With grace period (300s):

  T0: Client calls put_leaf("data") → addr_A (timestamp = T0)
  T1: GC starts (T1 - T0 < 300s)
  T2: GC sees A is within grace period → skip
  T3: Client calls put_recipe(f, [A]) → addr_X
  T4: Next GC: A is in live set (referenced by X) → safe

Grace period must be longer than the maximum expected delay
between put_leaf and put_recipe. 300s (5 min) is conservative.
```

### 6.2 Partial Sweep Recovery

```
Scenario: GC crashes mid-sweep (process killed)

  State before GC:
    BlobStore: [A, B, C, D]  (B, D are orphans)
    DAG: X = f(A), Y = g(C)

  GC removes B, then crashes before removing D.

  State after crash:
    BlobStore: [A, C, D]  (D is still orphan)
    DAG: X = f(A), Y = g(C)  (unchanged)

  Next GC run:
    Recomputes live set: {A, C, X, Y}
    Orphans: {D}
    Removes D

  Result: Eventually consistent. No data loss.
  Partial sweep is safe because each removal is independent.
```

---

## 7. Performance Analysis

### 7.1 Time Complexity

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| Compute live set | O(R × I) | R = recipes, I = avg inputs per recipe |
| List blobs | O(B) | B = total blobs (filesystem walk) |
| Identify orphans | O(B) | HashSet lookup per blob |
| Sweep blobs | O(K) | K = orphaned blobs |
| Sweep cache | O(K) | K = orphaned entries |
| Total | O(R×I + B + K) | Dominated by blob enumeration |

### 7.2 Space Complexity

| Component | Space | Notes |
|-----------|-------|-------|
| Live set | O(R × I) | HashSet of all referenced addrs |
| Blob list | O(B) | Vec of all blob addrs |
| Orphan list | O(K) | Vec of orphaned addrs |

### 7.3 Benchmark Estimates

```
Scenario: 10,000 blobs, 5,000 recipes, 2,000 orphans

  Compute live set: ~1ms (5000 recipes × 3 inputs avg = 15K inserts)
  List blobs: ~50ms (filesystem walk, 10K files)
  Identify orphans: ~0.5ms (10K HashSet lookups)
  Sweep 2000 blobs: ~200ms (filesystem deletes)
  Total: ~252ms

Scenario: 100,000 blobs, 50,000 recipes, 20,000 orphans

  Compute live set: ~10ms
  List blobs: ~500ms
  Identify orphans: ~5ms
  Sweep 20,000 blobs: ~2s
  Total: ~2.5s

Conclusion: GC is I/O bound (filesystem operations).
For very large stores, consider:
  1. Incremental GC (max_removals per cycle)
  2. Background GC thread with lower I/O priority
  3. Bloom filter for live set (reduce memory)
```

### 7.4 Lock Hold Duration

```
GC holds DAG write lock + cache write lock for entire duration.

Impact on concurrent operations:
  - get() blocks (needs DAG read + cache write)
  - put_leaf() does NOT block (only touches BlobStore)
  - put_recipe() blocks (needs DAG write)
  - invalidate() blocks (needs cache write)

For a 250ms GC cycle:
  - 250ms of blocked get/put_recipe/invalidate requests
  - Acceptable for infrequent GC (hourly/daily)

Optimization (future): release locks between mark and sweep phases
  - Mark phase: DAG read lock only (compute live set)
  - Release locks
  - Sweep phase: no DAG lock needed (only BlobStore + cache)
  - Reduces lock hold to ~1ms (mark) + ~200ms (sweep without DAG lock)
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-core/src/gc.rs` | **NEW** — `GcResult`, `GcConfig`, `PinSet` |
| `deriva-core/src/lib.rs` | Add `pub mod gc` |
| `deriva-storage/src/blob_store.rs` | Add `remove()`, `remove_batch()`, `list_addrs()`, `stats()` |
| `deriva-compute/src/gc.rs` | **NEW** — `compute_live_set()`, `GarbageCollector`, `run_gc_async()` |
| `deriva-compute/src/lib.rs` | Add `pub mod gc` |
| `proto/deriva.proto` | Add `GarbageCollect`, `Pin`, `Unpin`, `ListPins` RPCs + messages |
| `deriva-server/src/service.rs` | Add 4 RPC handlers, add `PinSet` to `ServerState` |
| `deriva-cli/src/main.rs` | Add `gc`, `pin`, `unpin`, `list-pins` commands |
| `deriva-core/tests/gc_pinset.rs` | **NEW** — 4 tests for PinSet |
| `deriva-storage/tests/blob_gc.rs` | **NEW** — 6 tests for BlobStore extensions |
| `deriva-compute/tests/gc.rs` | **NEW** — 10 tests for GarbageCollector |
| `deriva-server/tests/gc_rpc.rs` | **NEW** — 2 integration tests |

---

## 9. Dependency Changes

No new crate dependencies. All functionality built on existing:
- `std::collections::{HashSet, HashMap}` — live set, pin set
- `std::fs` — blob removal, directory walking
- `std::time::{Duration, Instant}` — timing, grace period
- `hex` — blob filename parsing (already a dependency)
- `tokio::sync::RwLock` — async lock wrapper (already a dependency)

---

## 10. Design Rationale

### 10.1 Why Mark-and-Sweep Instead of Reference Counting?

| Factor | Mark-and-sweep | Reference counting |
|--------|---------------|-------------------|
| Cycle handling | N/A (DAG has no cycles) | N/A |
| Overhead per put/remove | None | Increment/decrement on every operation |
| Batch efficiency | High (one pass) | N/A (continuous) |
| Pause time | Yes (during sweep) | None |
| Implementation complexity | Moderate | Low per-op, but pervasive |
| Correctness risk | Low (snapshot-based) | High (missed dec = leak, extra dec = premature free) |

Mark-and-sweep is preferred because:
1. No per-operation overhead (put_leaf, put_recipe are hot paths)
2. Batch efficiency — one GC cycle handles all orphans
3. Simpler correctness — no risk of reference count bugs
4. Natural dry-run support (mark without sweep)

### 10.2 Why Grace Period Instead of Two-Phase Put?

Alternative: require `put_leaf` + `put_recipe` to be atomic (transaction).

Problem: clients may put_leaf, do local processing, then put_recipe minutes
later. Forcing atomicity would require client-side transaction management.

Grace period is simpler:
- No client changes needed
- Configurable per deployment
- Default 5 minutes covers most workflows
- Zero overhead on the put path

### 10.3 Why PinSet Instead of Reference Counting from External Systems?

External systems (APIs, dashboards) may reference CAddrs by storing them
in databases, URLs, or configuration files. Deriva has no visibility into
these external references.

PinSet provides explicit opt-in protection:
- Client pins addrs it knows are externally referenced
- GC respects pins unconditionally
- Unpin when external reference is removed
- Simple, auditable (list-pins shows all protected addrs)

### 10.4 Why Not Automatic Scheduled GC?

The initial implementation is on-demand (CLI/RPC triggered) because:
1. GC frequency depends on workload (some need hourly, some weekly)
2. GC timing matters (avoid during peak load)
3. Dry-run first, then actual GC is a common workflow
4. Scheduled GC can be added later via cron or tokio interval

Future: `GcScheduler` with configurable interval and conditions
(e.g., "run GC when orphan estimate > 1000 or storage > 80% full").

### 10.5 Why DAG Write Lock During GC?

GC may remove recipes (orphaned recipes with missing inputs). This
requires DAG write access. Alternatives:

1. **Read lock + deferred removal**: Mark recipes for removal, apply later.
   Adds complexity, still needs write lock eventually.

2. **No recipe removal**: Only remove blobs, leave orphaned recipes.
   Simpler but DAG grows unboundedly.

3. **Write lock for full GC** (chosen): Simple, correct, acceptable
   because GC is infrequent.

---

## 11. Observability Integration

New metrics for GC (integrates with §2.5):

```rust
lazy_static! {
    static ref GC_RUNS_TOTAL: IntCounterVec = register_int_counter_vec!(
        "deriva_gc_runs_total",
        "Total GC cycles by mode",
        &["mode"]  // "actual", "dry_run"
    ).unwrap();

    static ref GC_BLOBS_REMOVED: Histogram = register_histogram!(
        "deriva_gc_blobs_removed",
        "Blobs removed per GC cycle",
        vec![0.0, 1.0, 10.0, 100.0, 1000.0, 10000.0]
    ).unwrap();

    static ref GC_BYTES_RECLAIMED: Histogram = register_histogram!(
        "deriva_gc_bytes_reclaimed",
        "Bytes reclaimed per GC cycle",
        vec![0.0, 1024.0, 1e6, 1e7, 1e8, 1e9]
    ).unwrap();

    static ref GC_DURATION: Histogram = register_histogram!(
        "deriva_gc_duration_seconds",
        "GC cycle duration",
        vec![0.01, 0.1, 0.5, 1.0, 5.0, 30.0, 60.0]
    ).unwrap();

    static ref GC_LIVE_BLOBS: Gauge = register_gauge!(
        "deriva_gc_live_blobs",
        "Number of live blobs after last GC"
    ).unwrap();

    static ref GC_PINNED_COUNT: Gauge = register_gauge!(
        "deriva_gc_pinned_count",
        "Number of pinned addrs"
    ).unwrap();
}
```

Alerting rules:

```yaml
# Alert if GC hasn't run in 24 hours
- alert: GcNotRunning
  expr: time() - deriva_gc_last_run_timestamp > 86400
  labels:
    severity: warning

# Alert if GC is removing >50% of blobs (high orphan rate)
- alert: HighOrphanRate
  expr: >
    deriva_gc_blobs_removed / on() deriva_gc_live_blobs > 0.5
  labels:
    severity: warning

# Alert if GC takes >30 seconds
- alert: SlowGc
  expr: deriva_gc_duration_seconds > 30
  labels:
    severity: warning
```

---

## 12. Checklist

- [x] Create `deriva-core/src/gc.rs` with `GcResult`, `GcConfig`, `PinSet`
- [x] Add `remove()`, `remove_batch()`, `list_addrs()`, `stats()` to `BlobStore`
- [x] Create `deriva-compute/src/gc.rs` with `compute_live_set`, `GarbageCollector`
- [x] Add `run_gc_async()` wrapper
- [x] Update `proto/deriva.proto` with GC + Pin RPCs
- [x] Implement 4 RPC handlers in `service.rs`
- [x] Add `PinSet` to `ServerState`
- [x] Add CLI commands: `gc`, `pin`, `unpin`, `list-pins`
- [x] Write unit tests for PinSet (~4 tests)
- [x] Write unit tests for BlobStore extensions (~6 tests)
- [x] Write unit tests for live set computation (~4 tests)
- [x] Write unit tests for GarbageCollector (~7 tests)
- [x] Write integration tests (~2 tests)
- [x] Add GC metrics to observability
- [x] Run full test suite — all existing tests still pass
- [x] `cargo clippy --workspace -- -D warnings` clean
- [x] Commit and push
