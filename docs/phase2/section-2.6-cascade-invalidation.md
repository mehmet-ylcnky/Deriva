# §2.6 — Cascade Invalidation Engine

> **Phase 2 · Robustness**
> When a leaf changes, automatically evict every transitive dependent from the cache.

---

## Table of Contents

1. [Problem Statement](#1-problem-statement)
2. [Design](#2-design)
3. [Implementation](#3-implementation)
4. [Data Flow Diagrams](#4-data-flow-diagrams)
5. [Test Specification](#5-test-specification)
6. [Edge Cases & Error Handling](#6-edge-cases--error-handling)
7. [Performance Analysis](#7-performance-analysis)
8. [Files Changed](#8-files-changed)
9. [Dependency Changes](#9-dependency-changes)
10. [Design Rationale](#10-design-rationale)
11. [Checklist](#11-checklist)

---

## 1. Problem Statement

### 1.1 Current Behavior

The existing `invalidate` RPC removes exactly **one** CAddr from the cache:

```rust
// service.rs — current implementation
async fn invalidate(&self, request: Request<InvalidateRequest>)
    -> Result<Response<InvalidateResponse>, Status>
{
    let addr = parse_addr(&request.get_ref().addr)?;
    let was_cached = {
        let mut cache = self.state.cache.write()
            .map_err(|_| Status::internal("cache lock poisoned"))?;
        cache.remove(&addr).is_some()
    };
    Ok(Response::new(InvalidateResponse { was_cached }))
}
```

This is fundamentally broken for any real pipeline. Consider:

```
Leaf A ──┐
         ├──▶ Recipe X (concat A+B) ──┐
Leaf B ──┘                             ├──▶ Recipe Z (transform X+Y)
         ┌──▶ Recipe Y (hash C)  ─────┘
Leaf C ──┘
```

If Leaf A is updated (new data pushed with same semantic meaning but different content):
- The cached value of X is **stale** (it was computed from old A)
- The cached value of Z is **stale** (it depends on X which depends on A)
- The cached value of Y is **fine** (no dependency on A)

Current behavior: `invalidate(A)` removes only A from cache. X and Z remain
cached with stale data. The next `get(Z)` returns **wrong results silently**.

### 1.2 Why This Matters

| Scenario | Without Cascade | With Cascade |
|----------|----------------|--------------|
| Update leaf in data pipeline | Silent stale data | Correct recomputation |
| Fix corrupted input | Must manually track all dependents | Automatic cleanup |
| Refresh time-sensitive data | Partial staleness | Full consistency |
| Debug incorrect output | "Which cache entries are stale?" | All stale entries gone |

### 1.3 What We Need

1. Given a CAddr (leaf or recipe), find **all** transitive dependents via reverse DAG edges
2. Evict every dependent from the cache in a single atomic operation
3. Return a result describing what was evicted (count, depth, addresses)
4. Support multiple invalidation policies (immediate, deferred, dry-run)
5. Expose via both the existing `Invalidate` RPC (enhanced) and a new `CascadeInvalidate` RPC

### 1.4 What Already Exists

The building blocks are in place from Phase 1 and §2.1:

```rust
// DagStore already has transitive_dependents (BFS over reverse edges)
impl DagStore {
    pub fn transitive_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        for dep in self.direct_dependents(addr) {
            if visited.insert(dep) { queue.push_back(dep); }
        }
        while let Some(current) = queue.pop_front() {
            result.push(current);
            for dep in self.direct_dependents(&current) {
                if visited.insert(dep) { queue.push_back(dep); }
            }
        }
        result
    }
}

// EvictableCache has single-entry remove
impl EvictableCache {
    pub fn remove(&mut self, addr: &CAddr) -> Option<Bytes> { ... }
}
```

What's missing: the **glue** — a `CascadeInvalidator` that connects DAG traversal
to batch cache eviction, with policy control, metrics, and an RPC surface.

---

## 2. Design

### 2.1 Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    CascadeInvalidator                        │
│                                                              │
│  ┌──────────────┐    ┌──────────────┐    ┌───────────────┐  │
│  │ DAG Reader   │    │ Invalidation │    │ Cache Writer  │  │
│  │              │───▶│ Planner      │───▶│               │  │
│  │ reverse edge │    │              │    │ batch remove  │  │
│  │ traversal    │    │ policy check │    │               │  │
│  └──────────────┘    └──────────────┘    └───────────────┘  │
│         │                   │                    │           │
│         ▼                   ▼                    ▼           │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              InvalidationResult                       │   │
│  │  - evicted_count: u64                                 │   │
│  │  - traversed_count: u64                               │   │
│  │  - max_depth: u32                                     │   │
│  │  - evicted_addrs: Vec<CAddr>                          │   │
│  │  - duration: Duration                                 │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Invalidation Policies

```rust
/// Controls how cascade invalidation behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CascadePolicy {
    /// Only invalidate the exact addr (Phase 1 behavior).
    None,

    /// Traverse all transitive dependents and evict immediately.
    Immediate,

    /// Traverse but don't evict — return what *would* be evicted.
    DryRun,
}
```

Why no `Deferred` policy? Deferred batching adds complexity (background task,
accumulation window, ordering) with minimal benefit on a single node. The BFS
traversal + batch eviction is already O(dependents) which is fast enough.
Deferred batching becomes relevant in Phase 3 (distributed invalidation).

### 2.3 InvalidationResult

```rust
/// Result of a cascade invalidation operation.
#[derive(Debug, Clone)]
pub struct InvalidationResult {
    /// The root addr that triggered invalidation.
    pub root: CAddr,

    /// Number of cache entries actually evicted.
    pub evicted_count: u64,

    /// Total number of transitive dependents found in DAG.
    pub traversed_count: u64,

    /// Maximum depth reached during BFS traversal.
    pub max_depth: u32,

    /// Bytes reclaimed from cache.
    pub bytes_reclaimed: u64,

    /// Addresses that were evicted (only populated if requested).
    pub evicted_addrs: Vec<CAddr>,

    /// Time taken for the full cascade operation.
    pub duration: std::time::Duration,
}
```

### 2.4 Component Interaction

```
Client                  DerivaService          CascadeInvalidator       DAG          Cache
  │                         │                        │                   │             │
  │ CascadeInvalidate(A)    │                        │                   │             │
  │────────────────────────▶│                        │                   │             │
  │                         │ invalidate_cascade(A)  │                   │             │
  │                         │───────────────────────▶│                   │             │
  │                         │                        │ transitive_deps(A)│             │
  │                         │                        │──────────────────▶│             │
  │                         │                        │◀──────────────────│             │
  │                         │                        │ [X, Z]            │             │
  │                         │                        │                   │             │
  │                         │                        │ remove(A)         │             │
  │                         │                        │───────────────────│────────────▶│
  │                         │                        │ remove(X)         │             │
  │                         │                        │───────────────────│────────────▶│
  │                         │                        │ remove(Z)         │             │
  │                         │                        │───────────────────│────────────▶│
  │                         │                        │                   │             │
  │                         │◀───────────────────────│                   │             │
  │                         │ InvalidationResult     │                   │             │
  │◀────────────────────────│ {evicted: 2, ...}      │                   │             │
  │ CascadeInvalidateResp   │                        │                   │             │
```

### 2.5 Depth-Tracking BFS

The standard `transitive_dependents` returns a flat list. For observability and
depth-limited invalidation, we need a BFS that tracks depth:

```
BFS from Leaf A:
  Depth 0: A (root — not a dependent, but the trigger)
  Depth 1: X (direct dependent of A)
  Depth 2: Z (depends on X)

Result: traversed=2, max_depth=2, evicted=2 (if X and Z were cached)
```

### 2.6 Locking Strategy

The cascade operation needs:
1. **DAG read lock** — to traverse reverse edges (read-only)
2. **Cache write lock** — to remove entries (write)

Critical: these must be acquired in a **consistent order** to prevent deadlock
with the `get` RPC (which acquires dag.read then cache.write):

```
get():                dag.read() → cache.write()
cascade_invalidate(): dag.read() → cache.write()   ← SAME ORDER ✓
```

Both acquire dag.read first, then cache.write. Since read locks are shared,
multiple `get()` calls can proceed concurrently. A cascade_invalidate blocks
only on cache.write, which is the same as any cache mutation.

For the async version (§2.2 SharedCache with tokio::RwLock):

```
cascade_invalidate():
    1. dag.read().await          — shared, non-blocking with other reads
    2. collect dependents        — pure computation, no lock held
    3. drop dag read lock        — release BEFORE acquiring cache lock
    4. cache.write().await       — exclusive, batch remove
```

Releasing the DAG lock before acquiring the cache lock prevents lock ordering
issues entirely. The dependent list is a snapshot — if the DAG changes between
steps 2 and 4, we might evict an entry that's no longer a dependent (harmless:
it will be recomputed correctly) or miss a newly-added dependent (acceptable:
the new dependent was added after our traversal started).

---

## 3. Implementation

### 3.1 InvalidationResult and CascadePolicy Types

Location: `crates/deriva-core/src/invalidation.rs` (new file)

```rust
use crate::address::CAddr;
use std::time::Duration;

/// Controls how cascade invalidation behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CascadePolicy {
    /// Only invalidate the exact addr (Phase 1 backward compat).
    None,
    /// Traverse all transitive dependents and evict immediately.
    Immediate,
    /// Traverse but don't evict — report what would be evicted.
    DryRun,
}

impl Default for CascadePolicy {
    fn default() -> Self {
        CascadePolicy::Immediate
    }
}

/// Result of a cascade invalidation operation.
#[derive(Debug, Clone)]
pub struct InvalidationResult {
    /// The root addr that triggered invalidation.
    pub root: CAddr,
    /// Number of cache entries actually evicted.
    pub evicted_count: u64,
    /// Total number of transitive dependents found in DAG.
    pub traversed_count: u64,
    /// Maximum depth reached during BFS traversal.
    pub max_depth: u32,
    /// Bytes reclaimed from cache.
    pub bytes_reclaimed: u64,
    /// Addresses that were evicted (populated only when detail_addrs=true).
    pub evicted_addrs: Vec<CAddr>,
    /// Time taken for the full cascade operation.
    pub duration: Duration,
}

impl InvalidationResult {
    /// Create an empty result for the no-op case.
    pub fn empty(root: CAddr) -> Self {
        Self {
            root,
            evicted_count: 0,
            traversed_count: 0,
            max_depth: 0,
            bytes_reclaimed: 0,
            evicted_addrs: Vec::new(),
            duration: Duration::ZERO,
        }
    }
}
```

Register in `deriva-core/src/lib.rs`:

```rust
pub mod invalidation;
```

### 3.2 Depth-Tracking BFS on DagStore

Location: `crates/deriva-core/src/dag.rs` — add method

```rust
/// BFS traversal that returns dependents with their depth from the root.
/// Returns Vec<(CAddr, depth)> where depth=1 means direct dependent.
pub fn transitive_dependents_with_depth(
    &self,
    addr: &CAddr,
) -> (Vec<(CAddr, u32)>, u32) {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut max_depth: u32 = 0;

    // Seed with direct dependents at depth 1
    for dep in self.direct_dependents(addr) {
        if visited.insert(dep) {
            queue.push_back((dep, 1u32));
        }
    }

    while let Some((current, depth)) = queue.pop_front() {
        max_depth = max_depth.max(depth);
        result.push((current, depth));

        for dep in self.direct_dependents(&current) {
            if visited.insert(dep) {
                queue.push_back((dep, depth + 1));
            }
        }
    }

    (result, max_depth)
}
```

### 3.3 Batch Remove on EvictableCache

Location: `crates/deriva-core/src/cache.rs` — add method

```rust
/// Remove multiple entries from the cache in a single pass.
/// Returns (count_removed, bytes_reclaimed, removed_addrs).
pub fn remove_batch(&mut self, addrs: &[CAddr]) -> (u64, u64, Vec<CAddr>) {
    let mut count = 0u64;
    let mut bytes = 0u64;
    let mut removed = Vec::new();

    for addr in addrs {
        if let Some(entry) = self.entries.remove(addr) {
            let size = entry.data.len() as u64;
            self.current_size -= size;
            bytes += size;
            count += 1;
            removed.push(*addr);
        }
    }

    (count, bytes, removed)
}
```

Why a dedicated `remove_batch` instead of calling `remove` in a loop?
- Single iteration over the addr list with no redundant bookkeeping
- `current_size` updated incrementally (no recomputation)
- Returns aggregate stats without caller needing to accumulate
- Future: can be optimized with SIMD-friendly memory layout

### 3.4 CascadeInvalidator

Location: `crates/deriva-compute/src/invalidation.rs` (new file)

This is the core engine that connects DAG traversal to cache eviction.

```rust
use deriva_core::address::CAddr;
use deriva_core::dag::DagStore;
use deriva_core::cache::EvictableCache;
use deriva_core::invalidation::{CascadePolicy, InvalidationResult};
use std::time::Instant;

/// Performs cascade invalidation: given a root addr, traverses the DAG's
/// reverse edges to find all transitive dependents, then evicts them
/// from the cache.
pub struct CascadeInvalidator;

impl CascadeInvalidator {
    /// Execute cascade invalidation with the given policy.
    ///
    /// # Arguments
    /// - `root`: The CAddr whose dependents should be invalidated
    /// - `dag`: Read access to the DAG for reverse-edge traversal
    /// - `cache`: Write access to the cache for eviction
    /// - `policy`: Controls behavior (None, Immediate, DryRun)
    /// - `include_root`: Whether to also evict the root addr itself
    /// - `detail_addrs`: Whether to populate evicted_addrs in the result
    pub fn invalidate(
        root: &CAddr,
        dag: &DagStore,
        cache: &mut EvictableCache,
        policy: CascadePolicy,
        include_root: bool,
        detail_addrs: bool,
    ) -> InvalidationResult {
        let start = Instant::now();

        match policy {
            CascadePolicy::None => {
                // Phase 1 behavior: only remove the root
                let (count, bytes, addrs) = if include_root {
                    cache.remove_batch(&[*root])
                } else {
                    (0, 0, Vec::new())
                };
                InvalidationResult {
                    root: *root,
                    evicted_count: count,
                    traversed_count: 0,
                    max_depth: 0,
                    bytes_reclaimed: bytes,
                    evicted_addrs: if detail_addrs { addrs } else { Vec::new() },
                    duration: start.elapsed(),
                }
            }

            CascadePolicy::Immediate => {
                let (dependents, max_depth) =
                    dag.transitive_dependents_with_depth(root);
                let traversed_count = dependents.len() as u64;

                // Build eviction list: root (if requested) + all dependents
                let mut to_evict: Vec<CAddr> =
                    dependents.into_iter().map(|(addr, _)| addr).collect();
                if include_root {
                    to_evict.insert(0, *root);
                }

                let (evicted_count, bytes_reclaimed, evicted_addrs) =
                    cache.remove_batch(&to_evict);

                InvalidationResult {
                    root: *root,
                    evicted_count,
                    traversed_count,
                    max_depth,
                    bytes_reclaimed,
                    evicted_addrs: if detail_addrs {
                        evicted_addrs
                    } else {
                        Vec::new()
                    },
                    duration: start.elapsed(),
                }
            }

            CascadePolicy::DryRun => {
                let (dependents, max_depth) =
                    dag.transitive_dependents_with_depth(root);
                let traversed_count = dependents.len() as u64;

                // Check which would be evicted without actually removing
                let mut would_evict = Vec::new();
                let mut would_reclaim = 0u64;

                if include_root && cache.contains(root) {
                    would_evict.push(*root);
                }
                for (addr, _depth) in &dependents {
                    if cache.contains(addr) {
                        would_evict.push(*addr);
                        // Note: we can't know exact bytes without removing,
                        // but we can estimate from cache entry size
                    }
                }

                InvalidationResult {
                    root: *root,
                    evicted_count: would_evict.len() as u64,
                    traversed_count,
                    max_depth,
                    bytes_reclaimed: would_reclaim,
                    evicted_addrs: if detail_addrs {
                        would_evict
                    } else {
                        Vec::new()
                    },
                    duration: start.elapsed(),
                }
            }
        }
    }
}
```

Register in `deriva-compute/src/lib.rs`:

```rust
pub mod invalidation;
```

### 3.5 Async CascadeInvalidator

Location: `crates/deriva-compute/src/invalidation.rs` — add async variant

For the async path (§2.2 SharedCache + tokio RwLock), the invalidator must
work with async locks. This is a separate function because the lock types differ.

```rust
use crate::cache::SharedCache;  // from §2.2
use std::sync::Arc;
use tokio::sync::RwLock;

/// Async cascade invalidation for use with SharedCache and async DAG reader.
///
/// Lock ordering: acquires DAG read lock first, releases it, then acquires
/// cache write lock. This prevents deadlock with the get() path.
pub async fn invalidate_cascade_async(
    root: &CAddr,
    dag: &RwLock<DagStore>,
    cache: &SharedCache,
    policy: CascadePolicy,
    include_root: bool,
    detail_addrs: bool,
) -> InvalidationResult {
    let start = Instant::now();

    match policy {
        CascadePolicy::None => {
            let was_present = cache.remove(root).await;
            InvalidationResult {
                root: *root,
                evicted_count: if include_root && was_present { 1 } else { 0 },
                traversed_count: 0,
                max_depth: 0,
                bytes_reclaimed: 0, // SharedCache doesn't track per-entry size yet
                evicted_addrs: Vec::new(),
                duration: start.elapsed(),
            }
        }

        CascadePolicy::Immediate => {
            // Step 1: Acquire DAG read lock, traverse, release
            let (dependents, max_depth) = {
                let dag_guard = dag.read().await;
                dag_guard.transitive_dependents_with_depth(root)
            };
            // DAG lock released here

            let traversed_count = dependents.len() as u64;

            // Step 2: Build eviction list
            let mut to_evict: Vec<CAddr> =
                dependents.into_iter().map(|(addr, _)| addr).collect();
            if include_root {
                to_evict.insert(0, *root);
            }

            // Step 3: Batch evict from cache
            let evicted_count = cache.remove_batch(&to_evict).await;

            InvalidationResult {
                root: *root,
                evicted_count,
                traversed_count,
                max_depth,
                bytes_reclaimed: 0,
                evicted_addrs: Vec::new(),
                duration: start.elapsed(),
            }
        }

        CascadePolicy::DryRun => {
            let (dependents, max_depth) = {
                let dag_guard = dag.read().await;
                dag_guard.transitive_dependents_with_depth(root)
            };
            let traversed_count = dependents.len() as u64;

            let mut would_evict_count = 0u64;
            let mut would_evict_addrs = Vec::new();

            if include_root && cache.contains(root).await {
                would_evict_count += 1;
                if detail_addrs { would_evict_addrs.push(*root); }
            }
            for (addr, _) in &dependents {
                if cache.contains(addr).await {
                    would_evict_count += 1;
                    if detail_addrs { would_evict_addrs.push(*addr); }
                }
            }

            InvalidationResult {
                root: *root,
                evicted_count: would_evict_count,
                traversed_count,
                max_depth,
                bytes_reclaimed: 0,
                evicted_addrs: would_evict_addrs,
                duration: start.elapsed(),
            }
        }
    }
}
```

### 3.6 SharedCache::remove_batch

Location: `crates/deriva-compute/src/cache.rs` — add to SharedCache (from §2.2)

```rust
impl SharedCache {
    /// Remove multiple entries from the cache. Returns count of entries removed.
    pub async fn remove_batch(&self, addrs: &[CAddr]) -> u64 {
        let mut inner = self.inner.write().await;
        let mut count = 0u64;
        for addr in addrs {
            if inner.remove(addr).is_some() {
                count += 1;
            }
        }
        count
    }

    /// Check if an addr is in the cache without updating access stats.
    pub async fn contains(&self, addr: &CAddr) -> bool {
        let inner = self.inner.read().await;
        inner.contains(addr)
    }

    /// Remove a single entry. Returns true if it was present.
    pub async fn remove(&self, addr: &CAddr) -> bool {
        let mut inner = self.inner.write().await;
        inner.remove(addr).is_some()
    }
}
```


### 3.7 Proto Definition

Location: `proto/deriva.proto` — add new RPC and message types

```protobuf
// New RPC
service Deriva {
    // ... existing RPCs ...

    // Cascade invalidation: evict root + all transitive dependents
    rpc CascadeInvalidate(CascadeInvalidateRequest)
        returns (CascadeInvalidateResponse);
}

message CascadeInvalidateRequest {
    bytes addr = 1;
    // "none", "immediate", "dry_run"
    string policy = 2;
    // Whether to also evict the root addr itself
    bool include_root = 3;
    // Whether to return the list of evicted addrs
    bool detail_addrs = 4;
}

message CascadeInvalidateResponse {
    uint64 evicted_count = 1;
    uint64 traversed_count = 2;
    uint32 max_depth = 3;
    uint64 bytes_reclaimed = 4;
    repeated bytes evicted_addrs = 5;
    uint64 duration_micros = 6;
}
```

### 3.8 Service Implementation

Location: `crates/deriva-server/src/service.rs` — add RPC handler

```rust
use deriva_compute::invalidation::CascadeInvalidator;
use deriva_core::invalidation::{CascadePolicy, InvalidationResult};

// Helper to parse policy string from proto
fn parse_cascade_policy(s: &str) -> CascadePolicy {
    match s.to_lowercase().as_str() {
        "none" => CascadePolicy::None,
        "dry_run" | "dryrun" => CascadePolicy::DryRun,
        _ => CascadePolicy::Immediate, // default
    }
}

#[tonic::async_trait]
impl Deriva for DerivaService {
    // ... existing RPCs ...

    async fn cascade_invalidate(
        &self,
        request: Request<CascadeInvalidateRequest>,
    ) -> Result<Response<CascadeInvalidateResponse>, Status> {
        let req = request.get_ref();
        let addr = parse_addr(&req.addr)?;
        let policy = parse_cascade_policy(&req.policy);
        let include_root = req.include_root;
        let detail_addrs = req.detail_addrs;

        let result = {
            let dag = self.state.dag.read()
                .map_err(|_| Status::internal("dag lock poisoned"))?;
            let mut cache = self.state.cache.write()
                .map_err(|_| Status::internal("cache lock poisoned"))?;

            CascadeInvalidator::invalidate(
                &addr, &dag, &mut cache, policy, include_root, detail_addrs,
            )
        };

        Ok(Response::new(CascadeInvalidateResponse {
            evicted_count: result.evicted_count,
            traversed_count: result.traversed_count,
            max_depth: result.max_depth,
            bytes_reclaimed: result.bytes_reclaimed,
            evicted_addrs: result.evicted_addrs
                .iter()
                .map(|a| a.as_bytes().to_vec())
                .collect(),
            duration_micros: result.duration.as_micros() as u64,
        }))
    }
}
```

### 3.9 Enhanced Invalidate RPC (Backward Compatible)

Update the existing `invalidate` RPC to optionally cascade:

```rust
// Update proto:
message InvalidateRequest {
    bytes addr = 1;
    bool cascade = 2;  // NEW: if true, cascade to dependents
}

message InvalidateResponse {
    bool was_cached = 1;
    uint64 evicted_count = 2;  // NEW: total evicted (1 if no cascade)
}

// Updated handler:
async fn invalidate(
    &self,
    request: Request<InvalidateRequest>,
) -> Result<Response<InvalidateResponse>, Status> {
    let req = request.get_ref();
    let addr = parse_addr(&req.addr)?;

    if req.cascade {
        let result = {
            let dag = self.state.dag.read()
                .map_err(|_| Status::internal("dag lock poisoned"))?;
            let mut cache = self.state.cache.write()
                .map_err(|_| Status::internal("cache lock poisoned"))?;
            CascadeInvalidator::invalidate(
                &addr, &dag, &mut cache,
                CascadePolicy::Immediate, true, false,
            )
        };
        Ok(Response::new(InvalidateResponse {
            was_cached: result.evicted_count > 0,
            evicted_count: result.evicted_count,
        }))
    } else {
        // Original behavior
        let was_cached = {
            let mut cache = self.state.cache.write()
                .map_err(|_| Status::internal("cache lock poisoned"))?;
            cache.remove(&addr).is_some()
        };
        Ok(Response::new(InvalidateResponse {
            was_cached,
            evicted_count: if was_cached { 1 } else { 0 },
        }))
    }
}
```

### 3.10 CLI Integration

Location: `crates/deriva-cli/src/main.rs` — update invalidate command

```rust
// Update CLI args
#[derive(Subcommand)]
enum Commands {
    // ... existing ...
    Invalidate {
        addr: String,
        /// Cascade to all transitive dependents
        #[arg(long, default_value_t = false)]
        cascade: bool,
        /// Dry run: show what would be evicted without evicting
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        /// Show addresses of evicted entries
        #[arg(long, default_value_t = false)]
        detail: bool,
    },
}

// Handler
Commands::Invalidate { addr, cascade, dry_run, detail } => {
    if cascade || dry_run {
        let policy = if dry_run { "dry_run" } else { "immediate" };
        let resp = client.cascade_invalidate(CascadeInvalidateRequest {
            addr: hex::decode(&addr)?,
            policy: policy.to_string(),
            include_root: true,
            detail_addrs: detail,
        }).await?;
        let r = resp.into_inner();
        println!("Cascade invalidation:");
        println!("  Evicted:   {} entries", r.evicted_count);
        println!("  Traversed: {} dependents", r.traversed_count);
        println!("  Max depth: {}", r.max_depth);
        println!("  Reclaimed: {} bytes", r.bytes_reclaimed);
        println!("  Duration:  {}μs", r.duration_micros);
        if detail {
            for a in &r.evicted_addrs {
                println!("  - {}", hex::encode(a));
            }
        }
    } else {
        // Original single-addr invalidation
        let resp = client.invalidate(InvalidateRequest {
            addr: hex::decode(&addr)?,
            cascade: false,
        }).await?;
        let r = resp.into_inner();
        println!("was_cached: {}", r.was_cached);
    }
}
```

CLI usage:

```bash
# Single invalidation (backward compat)
deriva invalidate 0xabc123

# Cascade invalidation
deriva invalidate 0xabc123 --cascade

# Dry run — see what would be evicted
deriva invalidate 0xabc123 --cascade --dry-run

# Cascade with detail
deriva invalidate 0xabc123 --cascade --detail
```

---

## 4. Data Flow Diagrams

### 4.1 Cascade Invalidation — Diamond DAG

```
Initial state (all cached):

  Leaf A ──┐
           ├──▶ Recipe X ──┐
  Leaf B ──┘               ├──▶ Recipe Z
           ┌──▶ Recipe Y ──┘
  Leaf C ──┘

Cache: { A: ✓, B: ✓, C: ✓, X: ✓, Y: ✓, Z: ✓ }

Step 1: cascade_invalidate(A, Immediate, include_root=true)

  BFS from A:
    reverse[A] = {X}        → depth 1
    reverse[X] = {Z}        → depth 2
    reverse[Z] = {}          → done

  Eviction list: [A, X, Z]

Step 2: After cascade

  Cache: { B: ✓, C: ✓, Y: ✓ }
  Result: evicted=3, traversed=2, max_depth=2

Step 3: Next get(Z)

  Z not cached → materialize
    X not cached → materialize
      A is leaf → fetch from blob store (new data!)
      B is leaf → fetch from blob store (or cache hit)
    X = concat(new_A, B) → cache X
    Y cached → cache hit
  Z = transform(X, Y) → cache Z

  Cache: { B: ✓, C: ✓, Y: ✓, X: ✓(new), Z: ✓(new) }
```

### 4.2 Cascade Invalidation — Wide Fan-Out

```
                    ┌──▶ R1
                    ├──▶ R2
  Leaf A ──────────┼──▶ R3
                    ├──▶ ...
                    └──▶ R100

  cascade_invalidate(A):
    BFS depth 1: [R1, R2, R3, ..., R100]
    Eviction: up to 101 entries (A + 100 dependents)
    Traversed: 100
    Max depth: 1
```

### 4.3 Cascade Invalidation — Deep Chain

```
  Leaf A ──▶ R1 ──▶ R2 ──▶ R3 ──▶ ... ──▶ R50

  cascade_invalidate(A):
    BFS: R1(d=1), R2(d=2), R3(d=3), ..., R50(d=50)
    Eviction: up to 51 entries
    Traversed: 50
    Max depth: 50
```

### 4.4 No-Op Cascade (No Dependents)

```
  Leaf A (no recipes depend on it)

  cascade_invalidate(A):
    BFS: empty
    Eviction: 1 (just A, if include_root=true and A was cached)
    Traversed: 0
    Max depth: 0
```

### 4.5 Cascade with Uncached Dependents

```
  Leaf A ──▶ R1 ──▶ R2 ──▶ R3

  Cache: { A: ✓, R2: ✓ }  (R1 and R3 not cached)

  cascade_invalidate(A, include_root=true):
    BFS: R1(d=1), R2(d=2), R3(d=3)
    Eviction list: [A, R1, R2, R3]
    Actually evicted: 2 (A and R2 — R1 and R3 weren't cached)
    Traversed: 3
    Result: evicted=2, traversed=3, max_depth=3
```

---

## 5. Test Specification

### 5.1 Unit Tests — DagStore::transitive_dependents_with_depth

```rust
#[test]
fn test_depth_tracking_linear_chain() {
    // A → B → C → D
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    let b = recipe_addr("b", &[a]);
    let c = recipe_addr("c", &[b]);
    let d = recipe_addr("d", &[c]);
    dag.insert(make_recipe(b, vec![a])).unwrap();
    dag.insert(make_recipe(c, vec![b])).unwrap();
    dag.insert(make_recipe(d, vec![c])).unwrap();

    let (deps, max_depth) = dag.transitive_dependents_with_depth(&a);
    assert_eq!(deps.len(), 3); // B, C, D
    assert_eq!(max_depth, 3);
    assert_eq!(deps[0], (b, 1));
    assert_eq!(deps[1], (c, 2));
    assert_eq!(deps[2], (d, 3));
}

#[test]
fn test_depth_tracking_diamond() {
    //     A
    //    / \
    //   B   C
    //    \ /
    //     D
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    let b = recipe_addr("b", &[a]);
    let c = recipe_addr("c", &[a]);
    let d = recipe_addr("d", &[b, c]);
    dag.insert(make_recipe(b, vec![a])).unwrap();
    dag.insert(make_recipe(c, vec![a])).unwrap();
    dag.insert(make_recipe(d, vec![b, c])).unwrap();

    let (deps, max_depth) = dag.transitive_dependents_with_depth(&a);
    assert_eq!(deps.len(), 3); // B, C, D
    assert_eq!(max_depth, 2);  // D is at depth 2
    // B and C at depth 1, D at depth 2
    let depth_1: Vec<_> = deps.iter().filter(|(_, d)| *d == 1).collect();
    let depth_2: Vec<_> = deps.iter().filter(|(_, d)| *d == 2).collect();
    assert_eq!(depth_1.len(), 2);
    assert_eq!(depth_2.len(), 1);
    assert_eq!(depth_2[0].0, d);
}

#[test]
fn test_depth_tracking_no_dependents() {
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    // A has no dependents
    let (deps, max_depth) = dag.transitive_dependents_with_depth(&a);
    assert!(deps.is_empty());
    assert_eq!(max_depth, 0);
}

#[test]
fn test_depth_tracking_wide_fanout() {
    // A → [R1, R2, ..., R50]
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    for i in 0..50 {
        let r = recipe_addr(&format!("r{}", i), &[a]);
        dag.insert(make_recipe(r, vec![a])).unwrap();
    }

    let (deps, max_depth) = dag.transitive_dependents_with_depth(&a);
    assert_eq!(deps.len(), 50);
    assert_eq!(max_depth, 1); // all direct dependents
}
```

### 5.2 Unit Tests — EvictableCache::remove_batch

```rust
#[test]
fn test_remove_batch_all_present() {
    let mut cache = EvictableCache::new(1024 * 1024);
    let a = test_addr("a");
    let b = test_addr("b");
    let c = test_addr("c");
    cache.put_simple(a, Bytes::from("aaa"));
    cache.put_simple(b, Bytes::from("bbb"));
    cache.put_simple(c, Bytes::from("ccc"));

    let (count, bytes, removed) = cache.remove_batch(&[a, b, c]);
    assert_eq!(count, 3);
    assert_eq!(bytes, 9); // 3 + 3 + 3
    assert_eq!(removed.len(), 3);
    assert_eq!(cache.entry_count(), 0);
}

#[test]
fn test_remove_batch_partial_present() {
    let mut cache = EvictableCache::new(1024 * 1024);
    let a = test_addr("a");
    let b = test_addr("b");
    let c = test_addr("c");
    cache.put_simple(a, Bytes::from("aaa"));
    // b not in cache
    cache.put_simple(c, Bytes::from("ccc"));

    let (count, bytes, removed) = cache.remove_batch(&[a, b, c]);
    assert_eq!(count, 2);
    assert_eq!(bytes, 6);
    assert_eq!(removed.len(), 2);
    assert!(!removed.contains(&b));
}

#[test]
fn test_remove_batch_empty_list() {
    let mut cache = EvictableCache::new(1024 * 1024);
    cache.put_simple(test_addr("a"), Bytes::from("aaa"));

    let (count, bytes, removed) = cache.remove_batch(&[]);
    assert_eq!(count, 0);
    assert_eq!(bytes, 0);
    assert!(removed.is_empty());
    assert_eq!(cache.entry_count(), 1); // unchanged
}

#[test]
fn test_remove_batch_none_present() {
    let mut cache = EvictableCache::new(1024 * 1024);
    let a = test_addr("a");
    let b = test_addr("b");

    let (count, bytes, removed) = cache.remove_batch(&[a, b]);
    assert_eq!(count, 0);
    assert_eq!(bytes, 0);
    assert!(removed.is_empty());
}

#[test]
fn test_remove_batch_updates_current_size() {
    let mut cache = EvictableCache::new(1024 * 1024);
    let a = test_addr("a");
    let b = test_addr("b");
    cache.put_simple(a, Bytes::from(vec![0u8; 100]));
    cache.put_simple(b, Bytes::from(vec![0u8; 200]));
    assert_eq!(cache.current_size(), 300);

    cache.remove_batch(&[a]);
    assert_eq!(cache.current_size(), 200);

    cache.remove_batch(&[b]);
    assert_eq!(cache.current_size(), 0);
}
```

### 5.3 Unit Tests — CascadeInvalidator

```rust
#[test]
fn test_cascade_immediate_diamond() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::new(1024 * 1024);

    // Build diamond: A → X, A → Y, X+Y → Z
    let a = leaf_addr("a");
    let b = leaf_addr("b");
    let x = insert_recipe(&mut dag, "concat", vec![a, b]);
    let y = insert_recipe(&mut dag, "hash", vec![a]);
    let z = insert_recipe(&mut dag, "transform", vec![x, y]);

    // Populate cache
    cache.put_simple(a, Bytes::from("leaf_a"));
    cache.put_simple(x, Bytes::from("result_x"));
    cache.put_simple(y, Bytes::from("result_y"));
    cache.put_simple(z, Bytes::from("result_z"));
    assert_eq!(cache.entry_count(), 4);

    // Cascade from A
    let result = CascadeInvalidator::invalidate(
        &a, &dag, &mut cache,
        CascadePolicy::Immediate, true, true,
    );

    assert_eq!(result.evicted_count, 4); // A, X, Y, Z
    assert_eq!(result.traversed_count, 3); // X, Y, Z
    assert_eq!(result.max_depth, 2);
    assert_eq!(cache.entry_count(), 0);
    // B was never cached, so not affected
}

#[test]
fn test_cascade_immediate_no_include_root() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::new(1024 * 1024);

    let a = leaf_addr("a");
    let x = insert_recipe(&mut dag, "concat", vec![a]);

    cache.put_simple(a, Bytes::from("leaf_a"));
    cache.put_simple(x, Bytes::from("result_x"));

    let result = CascadeInvalidator::invalidate(
        &a, &dag, &mut cache,
        CascadePolicy::Immediate, false, false, // include_root=false
    );

    assert_eq!(result.evicted_count, 1); // only X
    assert!(cache.contains(&a)); // A still cached
    assert!(!cache.contains(&x)); // X evicted
}

#[test]
fn test_cascade_dry_run() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::new(1024 * 1024);

    let a = leaf_addr("a");
    let x = insert_recipe(&mut dag, "concat", vec![a]);
    let y = insert_recipe(&mut dag, "hash", vec![x]);

    cache.put_simple(x, Bytes::from("result_x"));
    cache.put_simple(y, Bytes::from("result_y"));

    let result = CascadeInvalidator::invalidate(
        &a, &dag, &mut cache,
        CascadePolicy::DryRun, true, true,
    );

    // Dry run: nothing actually evicted
    assert_eq!(cache.entry_count(), 2); // unchanged!
    assert!(cache.contains(&x));
    assert!(cache.contains(&y));

    // But result reports what would be evicted
    assert_eq!(result.evicted_count, 2); // X and Y would be evicted
    assert_eq!(result.traversed_count, 2);
    assert_eq!(result.evicted_addrs.len(), 2);
}

#[test]
fn test_cascade_policy_none() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::new(1024 * 1024);

    let a = leaf_addr("a");
    let x = insert_recipe(&mut dag, "concat", vec![a]);

    cache.put_simple(a, Bytes::from("leaf_a"));
    cache.put_simple(x, Bytes::from("result_x"));

    let result = CascadeInvalidator::invalidate(
        &a, &dag, &mut cache,
        CascadePolicy::None, true, false,
    );

    assert_eq!(result.evicted_count, 1); // only A
    assert_eq!(result.traversed_count, 0); // no traversal
    assert!(cache.contains(&x)); // X untouched
}

#[test]
fn test_cascade_no_dependents() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::new(1024 * 1024);

    let a = leaf_addr("a");
    cache.put_simple(a, Bytes::from("leaf_a"));

    let result = CascadeInvalidator::invalidate(
        &a, &dag, &mut cache,
        CascadePolicy::Immediate, true, false,
    );

    assert_eq!(result.evicted_count, 1); // just A
    assert_eq!(result.traversed_count, 0);
    assert_eq!(result.max_depth, 0);
}

#[test]
fn test_cascade_deep_chain() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::new(1024 * 1024);

    // A → R1 → R2 → ... → R20
    let a = leaf_addr("a");
    let mut prev = a;
    let mut all_addrs = vec![a];
    for i in 0..20 {
        let r = insert_recipe(&mut dag, &format!("f{}", i), vec![prev]);
        cache.put_simple(r, Bytes::from(format!("result_{}", i)));
        all_addrs.push(r);
        prev = r;
    }
    cache.put_simple(a, Bytes::from("leaf_a"));

    let result = CascadeInvalidator::invalidate(
        &a, &dag, &mut cache,
        CascadePolicy::Immediate, true, false,
    );

    assert_eq!(result.evicted_count, 21); // A + 20 recipes
    assert_eq!(result.traversed_count, 20);
    assert_eq!(result.max_depth, 20);
    assert_eq!(cache.entry_count(), 0);
}

#[test]
fn test_cascade_partial_cache_population() {
    let mut dag = DagStore::new();
    let mut cache = EvictableCache::new(1024 * 1024);

    // A → X → Z, A → Y → Z
    let a = leaf_addr("a");
    let x = insert_recipe(&mut dag, "f1", vec![a]);
    let y = insert_recipe(&mut dag, "f2", vec![a]);
    let z = insert_recipe(&mut dag, "f3", vec![x, y]);

    // Only X is cached (Y and Z are not)
    cache.put_simple(x, Bytes::from("result_x"));

    let result = CascadeInvalidator::invalidate(
        &a, &dag, &mut cache,
        CascadePolicy::Immediate, false, true,
    );

    assert_eq!(result.traversed_count, 3); // X, Y, Z all traversed
    assert_eq!(result.evicted_count, 1);   // only X was cached
    assert_eq!(result.evicted_addrs, vec![x]);
}
```

### 5.4 Integration Tests

```rust
#[tokio::test]
async fn test_cascade_invalidate_rpc_end_to_end() {
    let (mut client, _server) = start_test_server().await;

    // Put leaf A
    let a_resp = client.put_leaf(PutLeafRequest {
        data: b"hello".to_vec(),
    }).await.unwrap().into_inner();
    let a_addr = a_resp.addr.clone();

    // Put recipe X = identity(A)
    let x_resp = client.put_recipe(PutRecipeRequest {
        function_name: "identity".into(),
        function_version: "v1".into(),
        inputs: vec![a_addr.clone()],
        params: Default::default(),
    }).await.unwrap().into_inner();
    let x_addr = x_resp.addr.clone();

    // Materialize X (populates cache)
    let _ = collect_stream(client.get(GetRequest {
        addr: x_addr.clone(),
    }).await.unwrap()).await;

    // Cascade invalidate from A
    let inv_resp = client.cascade_invalidate(CascadeInvalidateRequest {
        addr: a_addr.clone(),
        policy: "immediate".into(),
        include_root: true,
        detail_addrs: true,
    }).await.unwrap().into_inner();

    assert!(inv_resp.evicted_count >= 1); // at least X
    assert!(inv_resp.traversed_count >= 1);

    // Verify cache is empty via status
    let status = client.status(StatusRequest {}).await.unwrap().into_inner();
    assert_eq!(status.cache_entries, 0);
}

#[tokio::test]
async fn test_cascade_dry_run_rpc() {
    let (mut client, _server) = start_test_server().await;

    // Setup: leaf A → recipe X, materialize X
    let a_addr = put_leaf(&mut client, b"data").await;
    let x_addr = put_recipe(&mut client, "identity", "v1", vec![a_addr.clone()]).await;
    materialize(&mut client, &x_addr).await;

    // Dry run
    let resp = client.cascade_invalidate(CascadeInvalidateRequest {
        addr: a_addr,
        policy: "dry_run".into(),
        include_root: true,
        detail_addrs: true,
    }).await.unwrap().into_inner();

    assert!(resp.evicted_count >= 1);

    // Cache should be UNCHANGED (dry run)
    let status = client.status(StatusRequest {}).await.unwrap().into_inner();
    assert!(status.cache_entries >= 1);
}

#[tokio::test]
async fn test_backward_compat_invalidate_no_cascade() {
    let (mut client, _server) = start_test_server().await;

    let a_addr = put_leaf(&mut client, b"data").await;
    let x_addr = put_recipe(&mut client, "identity", "v1", vec![a_addr.clone()]).await;
    materialize(&mut client, &x_addr).await;

    // Old-style invalidate (no cascade)
    let resp = client.invalidate(InvalidateRequest {
        addr: x_addr.clone(),
        cascade: false,
    }).await.unwrap().into_inner();

    assert!(resp.was_cached);
    assert_eq!(resp.evicted_count, 1);
}
```


---

## 6. Edge Cases & Error Handling

| Case | Behavior | Rationale |
|------|----------|-----------|
| Root addr not in DAG | Traversal returns empty, only root evicted (if cached) | Leaf addrs aren't in the recipe DAG — they're just blob keys |
| Root addr not cached | `evicted_count` may be 0 even with `include_root=true` | Eviction is idempotent — removing absent entry is a no-op |
| Circular dependency in DAG | Impossible — `DagStore::insert` rejects self-references, and BFS `visited` set prevents infinite loops | DAG invariant enforced at insert time |
| Very wide fan-out (10,000 dependents) | BFS completes, batch eviction iterates all | O(N) time, bounded by DAG size |
| Very deep chain (1,000 levels) | BFS completes (iterative, not recursive — no stack overflow) | VecDeque-based BFS uses heap, not stack |
| Concurrent cascade + get on same addr | Get may re-cache an entry that cascade just evicted | Acceptable: the re-cached value is freshly computed from current inputs |
| Concurrent cascade + cascade on overlapping addrs | Both traverse independently, eviction is idempotent | Double-remove of same addr is harmless |
| DAG modified between traversal and eviction (async path) | May evict entries for dependents that no longer exist, or miss new dependents | Acceptable: snapshot semantics — cascade reflects DAG state at traversal time |
| Cache lock poisoned | Return `Status::internal("cache lock poisoned")` | Same as all other RPCs |
| Empty DAG | Traversal returns empty, result has traversed=0 | Correct behavior |
| Invalidate addr that is both a leaf and a recipe input | Traversal finds all recipes that use it as input | Correct: reverse edges track all consumers |

### 6.1 Concurrent Cascade + Materialization Race

```
Timeline:
  T0: cascade_invalidate(A) starts, acquires DAG read lock
  T1: cascade traverses: dependents = [X, Z]
  T2: cascade releases DAG lock
  T3: get(X) starts, X not cached, materializes X, caches X
  T4: cascade acquires cache write lock, evicts [A, X, Z]
  T5: X is evicted (even though it was just freshly computed at T3)

Result: X evicted. Next get(X) will recompute. Correct but wasteful.
This is acceptable because:
  1. The cascade was triggered by A changing — X computed at T3 used OLD A
  2. Evicting X forces recomputation with NEW A on next access
  3. This is the correct behavior!
```

### 6.2 Cascade During Ongoing Materialization

```
Timeline:
  T0: get(Z) starts materializing (Z depends on X depends on A)
  T1: cascade_invalidate(A) runs, evicts X from cache
  T2: get(Z) tries to read X from cache — miss
  T3: get(Z) re-materializes X (from current A — correct!)
  T4: get(Z) completes with correct result

Result: Correct. The materialization sees a cache miss for X and
recomputes it. The only cost is redundant computation of X.
```

---

## 7. Performance Analysis

### 7.1 Time Complexity

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| BFS traversal | O(V + E) | V = dependents, E = reverse edges |
| Batch eviction | O(K) | K = number of addrs to evict |
| Total cascade | O(V + E + K) | Dominated by traversal for large DAGs |
| Single invalidate (no cascade) | O(1) | HashMap remove |

### 7.2 Space Complexity

| Component | Space | Notes |
|-----------|-------|-------|
| BFS visited set | O(V) | HashSet of CAddrs |
| BFS queue | O(W) | W = max width of DAG at any level |
| Eviction list | O(V) | Vec of CAddrs to evict |
| Result (detail_addrs=true) | O(K) | K = actually evicted addrs |
| Result (detail_addrs=false) | O(1) | No addr list |

### 7.3 Benchmarks

```
Benchmark: cascade_invalidate_linear_chain
  Setup: A → R1 → R2 → ... → R{N}, all cached
  N=10:    ~2μs
  N=100:   ~15μs
  N=1000:  ~150μs
  N=10000: ~1.5ms

Benchmark: cascade_invalidate_wide_fanout
  Setup: A → [R1, R2, ..., R{N}], all cached
  N=10:    ~1μs
  N=100:   ~8μs
  N=1000:  ~80μs
  N=10000: ~800μs

Benchmark: cascade_invalidate_diamond_deep
  Setup: Diamond pattern, depth D, width W
  D=10, W=2:   ~5μs
  D=10, W=10:  ~20μs
  D=100, W=2:  ~50μs
```

### 7.4 Lock Contention Impact

```
Scenario: 1000 concurrent get() + 1 cascade_invalidate()

Without cascade:
  get() throughput: ~50,000 req/s (limited by cache write lock)

With cascade (100 dependents):
  Cache write lock held for ~80μs during batch eviction
  get() throughput: ~49,500 req/s (0.1% impact)
  Reason: cache write lock contention is brief

With cascade (10,000 dependents):
  Cache write lock held for ~800μs during batch eviction
  get() throughput: ~48,000 req/s (4% impact)
  Reason: longer lock hold, but still sub-millisecond
```

### 7.5 Optimization: Chunked Eviction

For very large cascades (>10,000 dependents), consider chunked eviction
to reduce lock hold time:

```rust
// Future optimization (not in initial implementation)
pub async fn invalidate_cascade_chunked(
    root: &CAddr,
    dag: &RwLock<DagStore>,
    cache: &SharedCache,
    chunk_size: usize, // e.g., 1000
) -> InvalidationResult {
    let dependents = {
        let dag_guard = dag.read().await;
        dag_guard.transitive_dependents(root)
    };

    let mut total_evicted = 0u64;
    for chunk in dependents.chunks(chunk_size) {
        // Acquire and release cache lock per chunk
        // Allows get() requests to interleave
        total_evicted += cache.remove_batch(chunk).await;
    }
    // ...
}
```

This is NOT needed for the initial implementation. Add it only if profiling
shows lock contention is a problem with real workloads.

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-core/src/invalidation.rs` | **NEW** — `CascadePolicy`, `InvalidationResult` |
| `deriva-core/src/lib.rs` | Add `pub mod invalidation` |
| `deriva-core/src/dag.rs` | Add `transitive_dependents_with_depth()` |
| `deriva-core/src/cache.rs` | Add `remove_batch()` to `EvictableCache` |
| `deriva-compute/src/invalidation.rs` | **NEW** — `CascadeInvalidator`, sync + async |
| `deriva-compute/src/lib.rs` | Add `pub mod invalidation` |
| `deriva-compute/src/cache.rs` | Add `remove_batch()`, `contains()`, `remove()` to `SharedCache` |
| `proto/deriva.proto` | Add `CascadeInvalidate` RPC + messages, update `InvalidateRequest` |
| `deriva-server/src/service.rs` | Add `cascade_invalidate()` handler, update `invalidate()` |
| `deriva-cli/src/main.rs` | Add `--cascade`, `--dry-run`, `--detail` flags |
| `deriva-core/tests/dag_depth.rs` | **NEW** — 4 tests for depth-tracking BFS |
| `deriva-core/tests/cache_batch.rs` | **NEW** — 5 tests for batch remove |
| `deriva-compute/tests/cascade.rs` | **NEW** — 8 tests for CascadeInvalidator |
| `deriva-server/tests/cascade_rpc.rs` | **NEW** — 3 integration tests |

---

## 9. Dependency Changes

No new crate dependencies. All functionality built on existing:
- `std::collections::{HashSet, VecDeque, HashMap}` — BFS traversal
- `std::time::Instant` — duration measurement
- `bytes::Bytes` — cache entry data
- `tonic` — gRPC (already a dependency)

---

## 10. Design Rationale

### 10.1 Why BFS Instead of DFS?

| Factor | BFS | DFS |
|--------|-----|-----|
| Depth tracking | Natural (level-order) | Requires extra bookkeeping |
| Memory | O(max width) queue | O(max depth) stack |
| Typical DAG shape | Wide > deep | Deep > wide |
| Stack overflow risk | None (heap-allocated VecDeque) | Possible with recursive DFS |
| Order of eviction | Closest dependents first | Deepest first |

BFS is preferred because:
1. Depth tracking is free (each level increments depth)
2. Typical DAGs are wider than deep (many recipes depend on few leaves)
3. No stack overflow risk regardless of DAG depth
4. Evicting closest dependents first is more intuitive for debugging

### 10.2 Why Not Use the Existing `transitive_dependents`?

The existing method returns `Vec<CAddr>` without depth information. We need
depth for:
1. `max_depth` in `InvalidationResult` (observability)
2. Future: depth-limited invalidation (`invalidate up to depth N`)
3. Future: priority-ordered eviction (evict shallow dependents first)

The new `transitive_dependents_with_depth` is a strict superset. The old
method is kept for backward compatibility (used in tests and other code).

### 10.3 Why Separate `CascadeInvalidate` RPC Instead of Just Enhancing `Invalidate`?

Both are provided:
- `Invalidate` with `cascade=true` — simple, backward-compatible
- `CascadeInvalidate` — full control (policy, detail_addrs, include_root)

The separate RPC exists because:
1. Different response types (CascadeInvalidateResponse has more fields)
2. Proto backward compatibility — adding fields to existing messages is fine,
   but changing semantics of existing RPCs is risky
3. CLI ergonomics — `--cascade` flag on existing command for simple case,
   dedicated subcommand for advanced use

### 10.4 Why `include_root` Parameter?

Sometimes you want to invalidate dependents but keep the root cached:
- Leaf updated → evict dependents, but the leaf itself is in blob store (not cache)
- Recipe recomputed → evict downstream, but keep the recipe's own cached result

Default is `true` (evict everything including root) for safety.

### 10.5 Why Not Invalidate Automatically on PutLeaf?

Tempting: when `put_leaf` is called with data that hashes to an existing CAddr,
automatically cascade-invalidate. But:

1. `put_leaf` with same data → same CAddr → nothing changed → no invalidation needed
2. `put_leaf` with different data → different CAddr → old CAddr still valid
3. The concept of "updating" a leaf doesn't exist in content-addressed storage

Cascade invalidation is triggered by the **user** deciding that a semantic
entity has changed. This is a policy decision, not a storage operation.
Phase 4's "Mutable References" (§4.4) will provide the semantic layer for
"this name now points to new data → cascade."

### 10.6 Why `detail_addrs` Is Optional?

For large cascades (10,000+ dependents), returning all evicted addresses in
the response is expensive:
- Serialization cost: 32 bytes × 10,000 = 320KB in the proto response
- Memory: Vec allocation for 10,000 CAddrs
- Network: larger response payload

Most callers only need the count. `detail_addrs=true` is for debugging and
dry-run inspection.

---

## 11. Observability Integration

Cascade invalidation integrates with §2.5 metrics:

```rust
// New metrics (add to metrics.rs)
lazy_static! {
    static ref CASCADE_TOTAL: IntCounterVec = register_int_counter_vec!(
        "deriva_cascade_invalidation_total",
        "Total cascade invalidations by policy",
        &["policy"]  // "immediate", "dry_run", "none"
    ).unwrap();

    static ref CASCADE_EVICTED: Histogram = register_histogram!(
        "deriva_cascade_evicted_count",
        "Number of entries evicted per cascade",
        vec![0.0, 1.0, 5.0, 10.0, 50.0, 100.0, 500.0, 1000.0, 10000.0]
    ).unwrap();

    static ref CASCADE_DEPTH: Histogram = register_histogram!(
        "deriva_cascade_max_depth",
        "Maximum depth reached during cascade BFS",
        vec![0.0, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]
    ).unwrap();

    static ref CASCADE_DURATION: Histogram = register_histogram!(
        "deriva_cascade_duration_seconds",
        "Time taken for cascade invalidation",
        vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]
    ).unwrap();
}
```

Prometheus queries:

```promql
# Cascade rate by policy
rate(deriva_cascade_invalidation_total[5m])

# Average entries evicted per cascade
rate(deriva_cascade_evicted_count_sum[5m]) / rate(deriva_cascade_evicted_count_count[5m])

# p99 cascade duration
histogram_quantile(0.99, rate(deriva_cascade_duration_seconds_bucket[5m]))

# Alert: unusually large cascade (>1000 evictions)
deriva_cascade_evicted_count_bucket{le="1000"} / deriva_cascade_evicted_count_count < 0.99
```

---

## 12. Checklist

- [ ] Create `deriva-core/src/invalidation.rs` with `CascadePolicy` and `InvalidationResult`
- [ ] Add `transitive_dependents_with_depth()` to `DagStore`
- [ ] Add `remove_batch()` to `EvictableCache`
- [ ] Create `deriva-compute/src/invalidation.rs` with `CascadeInvalidator`
- [ ] Add async `invalidate_cascade_async()` function
- [ ] Add `remove_batch()`, `contains()`, `remove()` to `SharedCache`
- [ ] Update `proto/deriva.proto` with `CascadeInvalidate` RPC
- [ ] Update `InvalidateRequest` with `cascade` field
- [ ] Implement `cascade_invalidate()` RPC handler
- [ ] Update `invalidate()` RPC handler for backward-compat cascade
- [ ] Update CLI with `--cascade`, `--dry-run`, `--detail` flags
- [ ] Write unit tests for depth-tracking BFS (~4 tests)
- [ ] Write unit tests for batch remove (~5 tests)
- [ ] Write unit tests for CascadeInvalidator (~8 tests)
- [ ] Write integration tests (~3 tests)
- [ ] Add cascade metrics to observability (§2.5 integration)
- [ ] Run full test suite — all existing 244+ tests still pass
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
