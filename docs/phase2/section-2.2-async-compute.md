# Section 2.2: Async Compute Engine

> **Goal:** Replace the synchronous `Executor` with an async Tokio-based engine that
> can yield during I/O (cache/storage reads) and prepare for parallel materialization in §2.3.
>
> **Crate:** `deriva-compute` (rewrite `executor.rs`, update `cache.rs`)
>
> **Estimated tests:** ~18
>
> **Commit message pattern:** `2.2: Async Compute — tokio-based executor with async cache/leaf traits`

---

## 1. Problem Statement

The current `Executor` is fully synchronous and takes `&mut self`:

```rust
// Current: deriva-compute/src/executor.rs (48 lines)
pub struct Executor<'a, C: MaterializationCache, L: LeafStore> {
    dag: &'a DagStore,
    registry: &'a FunctionRegistry,
    cache: &'a mut C,       // ← &mut prevents sharing across tasks
    leaf_store: &'a L,
}

impl<'a, C: MaterializationCache, L: LeafStore> Executor<'a, C, L> {
    pub fn materialize(&mut self, addr: &CAddr) -> Result<Bytes> {
        // 1. cache check (needs &mut self because cache.get updates access_count)
        // 2. leaf check
        // 3. recipe lookup from dag
        // 4. recursive materialize for each input (sequential!)
        // 5. function execute
        // 6. cache put
    }
}
```

**Problems:**

1. **`&mut C` prevents concurrency** — the cache trait requires `&mut self` for `get()` because
   `EvictableCache::get` updates `access_count` and `last_accessed`. This means only one task
   can hold the executor at a time.

2. **Sequential input resolution** — inputs are materialized in a `for` loop, one at a time.
   A recipe with 10 independent inputs resolves them serially.

3. **Blocking in async context** — the `get()` RPC in `service.rs` spawns a tokio task but
   immediately acquires `dag.read()` + `cache.write()` (std `RwLock`), blocking the tokio
   runtime thread:

```rust
// Current: service.rs get() RPC
tokio::spawn(async move {
    let result = {
        let dag = state.dag.read().unwrap();          // std::sync blocking
        let mut cache = state.cache.write().unwrap();  // std::sync blocking
        let mut executor = Executor::new(&dag, &state.registry, &mut *cache, &state.storage.blobs);
        executor.materialize(&addr)                    // sync recursive call
    };
    // ... stream chunks
});
```

4. **Lock scope too wide** — both `dag` read lock and `cache` write lock are held for the
   entire materialization, including compute time. Other RPCs are blocked.

### Quantified Impact

| Scenario | Phase 1 (sync) | Phase 2.2 (async) |
|----------|----------------|-------------------|
| 10 concurrent Get RPCs | Serialized by cache write lock | Concurrent, lock-per-operation |
| Recipe with 5 inputs, each 100ms compute | 500ms total | 500ms (still sequential — §2.3 adds parallelism) |
| Get RPC during long compute | Blocked waiting for cache lock | Can proceed, fine-grained locking |
| Tokio thread utilization | 1 thread blocked per Get | Yields during I/O, all threads available |

Note: §2.2 makes the engine async but keeps input resolution sequential. §2.3 adds
parallel fan-out for independent inputs.

---

## 2. Design

### 2.2.1 Architecture Overview

```
Phase 1 (current):
┌──────────────┐     &mut cache      ┌──────────────┐
│  Executor    │────────────────────▶│EvictableCache│
│  (sync,      │     &dag            │  (behind     │
│   recursive) │────────────────────▶│   RwLock)    │
└──────────────┘                     └──────────────┘
       │
       │  Sequential: input₁ → input₂ → input₃
       ▼

Phase 2.2 (target):
┌──────────────┐     Arc<AsyncCache>  ┌──────────────┐
│ AsyncExecutor│────────────────────▶│EvictableCache│
│  (async,     │     Arc<PersistentDag>│  (internal   │
│   recursive) │────────────────────▶│   RwLock)    │
└──────────────┘                     └──────────────┘
       │
       │  Sequential: input₁.await → input₂.await → input₃.await
       │  (§2.3 will make this parallel)
       ▼
```

### 2.2.2 Key Design Decisions

**Decision 1: Interior mutability for cache — `RwLock` inside `EvictableCache`**

Instead of requiring `&mut self` for `get()`, move the mutable state behind a `tokio::sync::RwLock`
inside the cache. The `MaterializationCache` trait methods take `&self`.

```
Before: cache.get(&mut self, addr) → needs exclusive access
After:  cache.get(&self, addr)     → internal RwLock, concurrent reads OK
```

**Decision 2: Remove lifetime parameters from Executor**

The current `Executor<'a, C, L>` borrows everything. The async version uses `Arc` for shared
ownership, eliminating lifetime parameters entirely:

```
Before: Executor<'a, C: MaterializationCache, L: LeafStore>
After:  AsyncExecutor (owns Arc references, no lifetimes)
```

**Decision 3: `async fn materialize` with `BoxFuture` for recursion**

Rust doesn't support `async fn` recursion directly. Use `BoxFuture`:

```rust
fn materialize(&self, addr: CAddr) -> BoxFuture<'_, Result<Bytes>> {
    Box::pin(async move {
        // ... recursive calls via self.materialize(input).await
    })
}
```

**Decision 4: Keep `ComputeFunction::execute` synchronous**

Compute functions are CPU-bound (hash, concat, transform). Making them async adds overhead
with no benefit. Wrap in `tokio::task::spawn_blocking` for expensive computations.

### 2.2.3 Trait Changes

```rust
// NEW: deriva-compute/src/cache.rs
#[async_trait]
pub trait AsyncMaterializationCache: Send + Sync {
    async fn get(&self, addr: &CAddr) -> Option<Bytes>;
    async fn put(&self, addr: CAddr, data: Bytes) -> u64;
    async fn contains(&self, addr: &CAddr) -> bool;
}
```

```rust
// NEW: deriva-compute/src/leaf_store.rs
#[async_trait]
pub trait AsyncLeafStore: Send + Sync {
    async fn get_leaf(&self, addr: &CAddr) -> Option<Bytes>;
}
```

### 2.2.4 DAG Access

After §2.1, the DAG is a `PersistentDag` (sled-backed, thread-safe). No async needed —
sled reads are fast enough to call synchronously. But we wrap in `spawn_blocking` if
resolve_order touches many nodes:

```
Simple lookup (contains, inputs): call directly — <5μs
Complex traversal (resolve_order): spawn_blocking if depth > 10
```

---

## 3. Implementation

### 3.1 Updated `MaterializationCache` Trait

```rust
// deriva-compute/src/cache.rs — REPLACE entire file

use async_trait::async_trait;
use bytes::Bytes;
use deriva_core::address::CAddr;
use deriva_core::cache::EvictableCache;
use tokio::sync::RwLock;
use std::sync::Arc;

/// Synchronous cache trait — kept for unit tests and backward compat.
pub trait MaterializationCache {
    fn get(&mut self, addr: &CAddr) -> Option<Bytes>;
    fn put(&mut self, addr: CAddr, data: Bytes) -> u64;
    fn contains(&self, addr: &CAddr) -> bool;
}

impl MaterializationCache for EvictableCache {
    fn get(&mut self, addr: &CAddr) -> Option<Bytes> {
        self.get(addr)
    }
    fn put(&mut self, addr: CAddr, data: Bytes) -> u64 {
        self.put_simple(addr, data)
    }
    fn contains(&self, addr: &CAddr) -> bool {
        self.contains(addr)
    }
}

/// Async cache trait for the async executor.
#[async_trait]
pub trait AsyncMaterializationCache: Send + Sync {
    async fn get(&self, addr: &CAddr) -> Option<Bytes>;
    async fn put(&self, addr: CAddr, data: Bytes) -> u64;
    async fn contains(&self, addr: &CAddr) -> bool;
}

/// Wraps EvictableCache with a tokio RwLock for async access.
pub struct SharedCache {
    inner: RwLock<EvictableCache>,
}

impl SharedCache {
    pub fn new(cache: EvictableCache) -> Self {
        Self { inner: RwLock::new(cache) }
    }

    pub async fn entry_count(&self) -> usize {
        self.inner.read().await.entry_count()
    }

    pub async fn current_size(&self) -> u64 {
        self.inner.read().await.current_size()
    }

    pub async fn hit_rate(&self) -> f64 {
        self.inner.read().await.hit_rate()
    }

    pub async fn remove(&self, addr: &CAddr) -> Option<Bytes> {
        self.inner.write().await.remove(addr)
    }
}

#[async_trait]
impl AsyncMaterializationCache for SharedCache {
    async fn get(&self, addr: &CAddr) -> Option<Bytes> {
        // get() updates access_count → needs write lock
        self.inner.write().await.get(addr)
    }

    async fn put(&self, addr: CAddr, data: Bytes) -> u64 {
        self.inner.write().await.put_simple(addr, data)
    }

    async fn contains(&self, addr: &CAddr) -> bool {
        self.inner.read().await.contains(addr)
    }
}
```

### 3.2 Updated `LeafStore` Trait

```rust
// deriva-compute/src/leaf_store.rs — ADD async trait

use async_trait::async_trait;
use bytes::Bytes;
use deriva_core::address::CAddr;

/// Synchronous leaf store — kept for backward compat.
pub trait LeafStore {
    fn get_leaf(&self, addr: &CAddr) -> Option<Bytes>;
}

/// Async leaf store for the async executor.
#[async_trait]
pub trait AsyncLeafStore: Send + Sync {
    async fn get_leaf(&self, addr: &CAddr) -> Option<Bytes>;
}

/// Blanket impl: any sync LeafStore that is Send+Sync can be used as AsyncLeafStore.
#[async_trait]
impl<T: LeafStore + Send + Sync> AsyncLeafStore for T {
    async fn get_leaf(&self, addr: &CAddr) -> Option<Bytes> {
        LeafStore::get_leaf(self, addr)
    }
}
```

### 3.3 New `AsyncExecutor`

```rust
// deriva-compute/src/async_executor.rs — NEW FILE

use crate::cache::AsyncMaterializationCache;
use crate::leaf_store::AsyncLeafStore;
use crate::registry::FunctionRegistry;
use bytes::Bytes;
use deriva_core::address::CAddr;
use deriva_core::error::{DerivaError, Result};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Trait for DAG access — abstracts over DagStore and PersistentDag.
/// Sync because sled reads are fast (<5μs).
pub trait DagReader: Send + Sync {
    fn get_inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>>;
    fn get_recipe(&self, addr: &CAddr) -> Result<Option<deriva_core::address::Recipe>>;
}

pub struct AsyncExecutor<C, L, D> {
    cache: Arc<C>,
    leaf_store: Arc<L>,
    dag: Arc<D>,
    registry: Arc<FunctionRegistry>,
}

impl<C, L, D> AsyncExecutor<C, L, D>
where
    C: AsyncMaterializationCache + 'static,
    L: AsyncLeafStore + 'static,
    D: DagReader + 'static,
{
    pub fn new(
        dag: Arc<D>,
        registry: Arc<FunctionRegistry>,
        cache: Arc<C>,
        leaf_store: Arc<L>,
    ) -> Self {
        Self { cache, leaf_store, dag, registry }
    }

    /// Materialize a CAddr — resolves recursively through the DAG.
    /// Returns the computed bytes.
    pub fn materialize(&self, addr: CAddr) -> BoxFuture<'_, Result<Bytes>> {
        Box::pin(async move {
            // 1. Cache check
            if let Some(bytes) = self.cache.get(&addr).await {
                return Ok(bytes);
            }

            // 2. Leaf check
            if let Some(bytes) = self.leaf_store.get_leaf(&addr).await {
                return Ok(bytes);
            }

            // 3. Recipe lookup (sync — sled is fast)
            let recipe = self.dag.get_recipe(&addr)?
                .ok_or_else(|| DerivaError::NotFound(addr.to_string()))?;

            // 4. Resolve inputs sequentially (§2.3 makes this parallel)
            let mut input_bytes = Vec::with_capacity(recipe.inputs.len());
            for input_addr in &recipe.inputs {
                let bytes = self.materialize(*input_addr).await?;
                input_bytes.push(bytes);
            }

            // 5. Execute compute function
            let func = self.registry.get(&recipe.function_id)
                .ok_or_else(|| DerivaError::FunctionNotFound(
                    recipe.function_id.to_string()
                ))?;

            let output = {
                let params = recipe.params.clone();
                // CPU-bound work — run on blocking thread pool
                tokio::task::spawn_blocking(move || {
                    func.execute(input_bytes, &params)
                })
                .await
                .map_err(|e| DerivaError::ComputeFailed(
                    format!("task join error: {}", e)
                ))?
                .map_err(|e| DerivaError::ComputeFailed(e.to_string()))?
            };

            // 6. Cache the result
            self.cache.put(addr, output.clone()).await;

            Ok(output)
        })
    }
}
```

### 3.4 `DagReader` Implementations

```rust
// In deriva-core/src/dag.rs — add impl
use crate::address::{CAddr, Recipe};
use crate::error::Result;

impl DagReader for DagStore {
    fn get_inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        Ok(self.get_recipe(addr).map(|r| r.inputs.clone()))
    }
    fn get_recipe(&self, addr: &CAddr) -> Result<Option<Recipe>> {
        Ok(DagStore::get_recipe(self, addr).cloned())
    }
}

// In deriva-core/src/persistent_dag.rs — add impl
impl DagReader for PersistentDag {
    fn get_inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        self.inputs(addr)
    }
    fn get_recipe(&self, addr: &CAddr) -> Result<Option<Recipe>> {
        // PersistentDag doesn't store recipes — delegate to recipe store
        // This requires the recipe store to be accessible. Two options:
        // (a) Store Arc<SledRecipeStore> in PersistentDag
        // (b) Create a composite DagReader that combines PersistentDag + RecipeStore
        // We choose (b) — see CombinedDagReader below
        unimplemented!("use CombinedDagReader instead")
    }
}

/// Combines PersistentDag (for inputs/edges) with SledRecipeStore (for full recipes).
pub struct CombinedDagReader {
    pub dag: Arc<PersistentDag>,
    pub recipes: Arc<SledRecipeStore>,
}

impl DagReader for CombinedDagReader {
    fn get_inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        self.dag.inputs(addr)
    }
    fn get_recipe(&self, addr: &CAddr) -> Result<Option<Recipe>> {
        self.recipes.get(addr)
    }
}
```

### 3.5 Updated `ServerState`

```rust
// deriva-server/src/state.rs — REPLACE

use deriva_compute::async_executor::{AsyncExecutor, CombinedDagReader};
use deriva_compute::cache::SharedCache;
use deriva_compute::registry::FunctionRegistry;
use deriva_core::persistent_dag::PersistentDag;
use deriva_storage::{StorageBackend, SledRecipeStore, BlobStore};
use std::sync::Arc;

pub struct ServerState {
    pub executor: AsyncExecutor<SharedCache, BlobStore, CombinedDagReader>,
    pub cache: Arc<SharedCache>,
    pub dag: Arc<PersistentDag>,
    pub recipes: Arc<SledRecipeStore>,
    pub registry: Arc<FunctionRegistry>,
    pub storage: StorageBackend,
}

impl ServerState {
    pub fn new(storage: StorageBackend, registry: FunctionRegistry) -> Self {
        let cache = Arc::new(SharedCache::new(
            deriva_core::cache::EvictableCache::default()
        ));
        let dag = Arc::new(storage.dag.clone()); // PersistentDag is Clone (sled trees are Arc internally)
        let recipes = Arc::new(storage.recipes.clone());
        let blobs = Arc::new(storage.blobs.clone());
        let registry = Arc::new(registry);

        let dag_reader = Arc::new(CombinedDagReader {
            dag: Arc::clone(&dag),
            recipes: Arc::clone(&recipes),
        });

        let executor = AsyncExecutor::new(
            dag_reader,
            Arc::clone(&registry),
            Arc::clone(&cache),
            blobs,
        );

        Self { executor, cache, dag, recipes, registry, storage }
    }
}
```

### 3.6 Updated `DerivaService`

The key change: `get()` RPC no longer holds locks for the entire materialization.

```rust
// deriva-server/src/service.rs — key changes only

#[tonic::async_trait]
impl Deriva for DerivaService {
    // put_leaf — unchanged (sync storage write is fine)

    async fn put_recipe(
        &self,
        request: Request<PutRecipeRequest>,
    ) -> Result<Response<PutRecipeResponse>, Status> {
        let req = request.get_ref();
        let inputs: Vec<CAddr> = req.inputs.iter()
            .map(|b| parse_addr(b))
            .collect::<Result<_, _>>()?;
        let recipe = Recipe::new(
            FunctionId::new(&req.function_name, &req.function_version),
            inputs,
            parse_params(&req.params),
        );
        let addr = self.state.storage.put_recipe(&recipe)
            .map_err(|e| Status::internal(e.to_string()))?;

        // PersistentDag.insert is thread-safe — no RwLock needed
        self.state.dag.insert(&recipe)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(PutRecipeResponse {
            addr: addr.as_bytes().to_vec(),
        }))
    }

    type GetStream = ReceiverStream<Result<GetResponse, Status>>;

    async fn get(
        &self,
        request: Request<GetRequest>,
    ) -> Result<Response<Self::GetStream>, Status> {
        let addr = parse_addr(&request.get_ref().addr)?;
        let state = Arc::clone(&self.state);
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            // No locks held! AsyncExecutor uses fine-grained internal locking.
            let result = state.executor.materialize(addr).await;

            match result {
                Ok(data) => {
                    for chunk in data.chunks(CHUNK_SIZE) {
                        if tx.send(Ok(GetResponse {
                            chunk: chunk.to_vec(),
                        })).await.is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(Status::internal(e.to_string()))).await;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn resolve(
        &self,
        request: Request<ResolveRequest>,
    ) -> Result<Response<ResolveResponse>, Status> {
        let addr = parse_addr(&request.get_ref().addr)?;
        // Direct sled read — no lock needed
        match self.state.recipes.get(&addr)
            .map_err(|e| Status::internal(e.to_string()))? {
            Some(recipe) => Ok(Response::new(ResolveResponse {
                found: true,
                function_name: recipe.function_id.name.clone(),
                function_version: recipe.function_id.version.clone(),
                inputs: recipe.inputs.iter().map(|a| a.as_bytes().to_vec()).collect(),
                params: recipe.params.iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
            })),
            None => Ok(Response::new(ResolveResponse::default())),
        }
    }

    async fn invalidate(
        &self,
        request: Request<InvalidateRequest>,
    ) -> Result<Response<InvalidateResponse>, Status> {
        let addr = parse_addr(&request.get_ref().addr)?;
        let was_cached = self.state.cache.remove(&addr).await.is_some();
        Ok(Response::new(InvalidateResponse { was_cached }))
    }

    async fn status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        Ok(Response::new(StatusResponse {
            recipe_count: self.state.dag.len() as u64,
            blob_count: 0,
            cache_entries: self.state.cache.entry_count().await as u64,
            cache_size_bytes: self.state.cache.current_size().await,
            cache_hit_rate: self.state.cache.hit_rate().await,
        }))
    }
}
```

---

## 4. Data Flow Diagrams

### 4.1 Phase 1 vs Phase 2.2 Get RPC

```
Phase 1 — Get RPC (blocking):
┌─────────┐  spawn   ┌──────────────────────────────────────────┐
│ tonic    │────────▶│ tokio task                                │
│ handler  │         │                                           │
└─────────┘         │  dag.read().unwrap()     ← BLOCKS thread  │
                     │  cache.write().unwrap()  ← BLOCKS thread  │
                     │  executor.materialize()  ← sync recursive │
                     │  // dag + cache locks held entire time     │
                     │  stream chunks                             │
                     └──────────────────────────────────────────┘

Phase 2.2 — Get RPC (async):
┌─────────┐  spawn   ┌──────────────────────────────────────────┐
│ tonic    │────────▶│ tokio task                                │
│ handler  │         │                                           │
└─────────┘         │  executor.materialize(addr).await          │
                     │    ├─ cache.get().await    ← lock: ~1μs   │
                     │    ├─ leaf_store.get()     ← no lock      │
                     │    ├─ dag.get_recipe()     ← sled read    │
                     │    ├─ materialize(i1).await ← recursive   │
                     │    ├─ materialize(i2).await ← recursive   │
                     │    ├─ spawn_blocking(func)  ← CPU work    │
                     │    └─ cache.put().await     ← lock: ~1μs  │
                     │  stream chunks                             │
                     └──────────────────────────────────────────┘
```

### 4.2 Lock Contention Comparison

```
Phase 1 — Two concurrent Get RPCs:

Time ──────────────────────────────────────────────▶

Task A: [====== cache.write() held for 500ms ======]
Task B:                                              [====== 500ms ======]
         ▲ blocked waiting for cache lock             ▲ finally runs
Total: 1000ms

Phase 2.2 — Two concurrent Get RPCs:

Time ──────────────────────────────────────────────▶

Task A: [c][leaf][dag][mat1][mat2][compute][c]  ← cache locks are ~1μs each
Task B:  [c][leaf][dag][mat1][mat2][compute][c] ← runs concurrently!
          ▲ tiny lock windows, no blocking
Total: ~500ms (overlapped)
```

### 4.3 spawn_blocking for CPU-Bound Compute

```
materialize(addr)
    │
    ├── cache.get().await          ← tokio task (async)
    ├── leaf_store.get().await     ← tokio task (async)
    ├── dag.get_recipe()           ← sync (fast sled read)
    ├── materialize(input).await   ← tokio task (recursive)
    │
    ├── spawn_blocking(|| {        ← moves to blocking thread pool
    │       func.execute(inputs)   ← CPU-bound work
    │   }).await                   ← tokio task resumes when done
    │
    └── cache.put().await          ← tokio task (async)
```

**Why `spawn_blocking`?** Compute functions (hash, concat, transform) are CPU-bound.
Running them on the tokio runtime thread pool would starve other async tasks. The
blocking thread pool is separate and sized for CPU work (default: 512 threads).

For cheap functions (identity, small concat), the `spawn_blocking` overhead (~5μs) is
negligible. For expensive functions (large transforms), it prevents runtime starvation.

**Optimization for later:** Add `ComputeFunction::is_cheap() -> bool` hint. If true,
run inline without `spawn_blocking`.

---

## 5. Async Recursion Deep Dive

### 5.1 Why BoxFuture?

Rust async functions are compiled to state machines. Recursive async functions create
infinitely-sized state machines — the compiler rejects them:

```rust
// This does NOT compile:
async fn materialize(&self, addr: CAddr) -> Result<Bytes> {
    for input in inputs {
        self.materialize(input).await; // recursive — infinite type!
    }
}
```

`BoxFuture` heap-allocates the future, breaking the infinite type:

```rust
fn materialize(&self, addr: CAddr) -> BoxFuture<'_, Result<Bytes>> {
    Box::pin(async move {
        for input in inputs {
            self.materialize(input).await; // OK — BoxFuture has known size
        }
    })
}
```

### 5.2 Cost of BoxFuture

Each recursive call allocates one `BoxFuture` on the heap:
- Allocation: ~50-100 bytes per future + allocator overhead
- For a DAG of depth 10: 10 allocations ≈ ~1KB total
- For a DAG of depth 100: 100 allocations ≈ ~10KB total

This is negligible compared to the data being materialized (typically KB-MB per node).

### 5.3 Stack Depth

Unlike sync recursion, async recursion with `BoxFuture` does NOT grow the call stack.
Each `.await` suspends the current future and the executor polls the inner future.
The stack depth stays constant regardless of DAG depth.

```
Sync recursion (Phase 1):     Async recursion (Phase 2.2):
Stack frame: materialize(D)   Heap: BoxFuture(D)
  Stack frame: materialize(C)   Heap: BoxFuture(C)
    Stack frame: materialize(B)   Heap: BoxFuture(B)
      Stack frame: materialize(A)   Heap: BoxFuture(A)
Stack depth: O(N)              Stack depth: O(1), Heap: O(N)
```

This eliminates the stack overflow risk identified in §2.1 for deep DAGs.

---

## 6. Cache Lock Optimization: Read vs Write

The `SharedCache` uses a write lock for `get()` because `EvictableCache::get` updates
`access_count` and `last_accessed`. This means concurrent `get()` calls are serialized.

### 6.1 Optimization: Split Hot/Cold State

```rust
pub struct OptimizedSharedCache {
    /// Data storage — read lock for lookups
    data: RwLock<HashMap<CAddr, Bytes>>,
    /// Metadata — write lock only for stats updates
    meta: RwLock<HashMap<CAddr, CacheMetadata>>,
    /// Eviction state
    eviction: Mutex<EvictionState>,
}

struct CacheMetadata {
    size: u64,
    access_count: AtomicU64,  // atomic — no lock needed!
    last_accessed: AtomicU64, // atomic timestamp
    recompute_cost: f64,
    pinned: bool,
}
```

With atomic counters, `get()` only needs a read lock on `data`:

```rust
async fn get(&self, addr: &CAddr) -> Option<Bytes> {
    let data = self.data.read().await;
    if let Some(bytes) = data.get(addr) {
        // Update stats atomically — no write lock needed
        if let Some(meta) = self.meta.read().await.get(addr) {
            meta.access_count.fetch_add(1, Ordering::Relaxed);
            meta.last_accessed.store(now_millis(), Ordering::Relaxed);
        }
        Some(bytes.clone())
    } else {
        None
    }
}
```

**Trade-off:** More complex implementation, but allows truly concurrent cache reads.
Implement this optimization if benchmarks show cache lock contention is a bottleneck.
For Phase 2.2, the simple `RwLock<EvictableCache>` approach is sufficient.

---

## 7. Keeping the Sync Executor

The old sync `Executor` is NOT deleted. It remains for:
- Unit tests that don't need tokio runtime
- Benchmarking sync vs async overhead
- Potential embedded/no-std use cases

```rust
// deriva-compute/src/lib.rs — updated exports
pub mod builtins;
pub mod cache;
pub mod executor;          // sync — kept
pub mod async_executor;    // NEW — async
pub mod function;
pub mod leaf_store;
pub mod registry;

pub use executor::Executor;
pub use async_executor::AsyncExecutor;
```

---

## 8. Test Specification

### 8.1 Unit Tests: `deriva-compute/tests/async_executor.rs`

Helper setup:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use bytes::Bytes;
use deriva_core::address::*;
use deriva_core::dag::DagStore;
use deriva_compute::async_executor::{AsyncExecutor, DagReader};
use deriva_compute::cache::{AsyncMaterializationCache, SharedCache};
use deriva_compute::leaf_store::AsyncLeafStore;
use deriva_compute::registry::FunctionRegistry;
use deriva_compute::builtins;
use std::collections::BTreeMap;

/// In-memory DagReader for tests (wraps DagStore).
struct TestDagReader {
    dag: DagStore,
    recipes: std::collections::HashMap<CAddr, Recipe>,
}

impl DagReader for TestDagReader {
    fn get_inputs(&self, addr: &CAddr) -> deriva_core::error::Result<Option<Vec<CAddr>>> {
        Ok(self.dag.get_recipe(addr).map(|r| r.inputs.clone()))
    }
    fn get_recipe(&self, addr: &CAddr) -> deriva_core::error::Result<Option<Recipe>> {
        Ok(self.recipes.get(addr).cloned())
    }
}

/// In-memory leaf store for tests.
struct TestLeafStore {
    leaves: std::collections::HashMap<CAddr, Bytes>,
}

#[async_trait::async_trait]
impl AsyncLeafStore for TestLeafStore {
    async fn get_leaf(&self, addr: &CAddr) -> Option<Bytes> {
        self.leaves.get(addr).cloned()
    }
}

fn setup() -> (
    Arc<TestDagReader>,
    Arc<FunctionRegistry>,
    Arc<SharedCache>,
    Arc<TestLeafStore>,
) {
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    let dag = TestDagReader {
        dag: DagStore::new(),
        recipes: std::collections::HashMap::new(),
    };
    let cache = SharedCache::new(deriva_core::cache::EvictableCache::default());
    let leaves = TestLeafStore { leaves: std::collections::HashMap::new() };
    (Arc::new(dag), Arc::new(registry), Arc::new(cache), Arc::new(leaves))
}
```

Test cases:

```
#[tokio::test]
test_async_materialize_leaf
    Insert leaf "hello" into TestLeafStore
    let executor = AsyncExecutor::new(dag, registry, cache, leaves);
    let result = executor.materialize(leaf_addr).await.unwrap();
    assert_eq!(result, Bytes::from("hello"));

#[tokio::test]
test_async_materialize_cached
    Pre-populate cache with addr → "cached_data"
    let result = executor.materialize(addr).await.unwrap();
    assert_eq!(result, Bytes::from("cached_data"));
    // Verify it came from cache (no leaf/dag access needed)

#[tokio::test]
test_async_materialize_recipe_identity
    Insert leaf "data" into leaves
    Insert recipe: identity(leaf_addr) into dag
    let result = executor.materialize(recipe_addr).await.unwrap();
    assert_eq!(result, Bytes::from("data"));

#[tokio::test]
test_async_materialize_recipe_concat
    Insert leaf "hello" and leaf " world"
    Insert recipe: concat(leaf1, leaf2)
    let result = executor.materialize(recipe_addr).await.unwrap();
    assert_eq!(result, Bytes::from("hello world"));

#[tokio::test]
test_async_materialize_chain
    leaf → identity → identity → identity (depth 3)
    let result = executor.materialize(final_addr).await.unwrap();
    assert_eq!(result, leaf_data);

#[tokio::test]
test_async_materialize_diamond
    leaf_a, leaf_b → concat(a,b) = r1
    leaf_a → identity(a) = r2
    r1, r2 → concat(r1, r2) = r3
    let result = executor.materialize(r3_addr).await.unwrap();
    // r3 = concat(concat(a,b), identity(a)) = concat("ab", "a") = "aba"

#[tokio::test]
test_async_materialize_not_found
    let result = executor.materialize(unknown_addr).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), DerivaError::NotFound(_)));

#[tokio::test]
test_async_materialize_function_not_found
    Insert recipe with unknown function "nonexistent_fn"
    let result = executor.materialize(recipe_addr).await;
    assert!(matches!(result.unwrap_err(), DerivaError::FunctionNotFound(_)));

#[tokio::test]
test_async_materialize_caches_result
    Insert leaf + recipe
    executor.materialize(recipe_addr).await.unwrap();
    // Verify result is now in cache
    assert!(cache.contains(&recipe_addr).await);

#[tokio::test]
test_async_concurrent_materialize_same_addr
    Insert leaf + recipe
    // Spawn 10 concurrent materializations of the same addr
    let handles: Vec<_> = (0..10).map(|_| {
        let exec = executor.clone(); // AsyncExecutor must be Clone
        tokio::spawn(async move { exec.materialize(addr).await })
    }).collect();
    for h in handles {
        let result = h.await.unwrap().unwrap();
        assert_eq!(result, expected);
    }

#[tokio::test]
test_async_concurrent_materialize_different_addrs
    Insert 10 different leaf+recipe pairs
    Spawn 10 concurrent materializations
    All should succeed without deadlock

#[tokio::test]
test_async_materialize_deep_dag
    Build chain of depth 50: leaf → r1 → r2 → ... → r50
    let result = executor.materialize(r50_addr).await.unwrap();
    assert_eq!(result, leaf_data);
    // Verifies no stack overflow with BoxFuture recursion

#[tokio::test]
test_shared_cache_concurrent_put_get
    Spawn 100 tasks: 50 putting, 50 getting
    No deadlocks, no panics
    All puts succeed, gets return correct data or None

#[tokio::test]
test_shared_cache_hit_rate
    Put 5 entries, get 3 of them twice, get 2 unknown
    hit_rate should be 3/5 = 0.6 (first gets are misses, second gets are hits)
```

### 8.2 Integration Tests

```
#[tokio::test]
test_async_get_rpc_concurrent
    Start server
    Put 5 different leaves + recipes
    Spawn 5 concurrent gRPC Get requests
    All should return correct data
    Total time should be < 5x single request time

#[tokio::test]
test_async_get_rpc_during_put
    Start server
    Spawn long-running Get (recipe with expensive compute)
    Simultaneously do PutLeaf + PutRecipe
    Both should succeed without blocking

#[tokio::test]
test_async_invalidate_during_get
    Start server, put leaf + recipe, materialize once (cached)
    Spawn Get request (should hit cache)
    Simultaneously invalidate the addr
    Get should return data (either cached or re-materialized)
    No deadlock or panic

#[tokio::test]
test_async_status_during_heavy_load
    Start server, put 100 recipes
    Spawn 50 concurrent Get requests
    Simultaneously call Status RPC
    Status should return without blocking
```

---

## 9. Edge Cases & Error Handling

| Case | Expected Behavior |
|------|-------------------|
| Recursive materialization hits missing recipe | `DerivaError::NotFound` propagated up |
| `spawn_blocking` panics inside compute function | `JoinError` caught, converted to `ComputeFailed` |
| tokio runtime shutdown during materialization | Futures cancelled, no resource leak (Drop cleans up) |
| Cache write lock held during eviction | Other `get()` calls wait briefly (~μs for eviction) |
| Concurrent put + get of same CAddr | Serialized by cache write lock — consistent |
| Very large materialization result (>1GB) | Cache `put` triggers eviction, may evict itself if over limit |
| DAG cycle (should be prevented by insert) | `resolve_order` would loop — but cycles are rejected at insert time |
| `ComputeFunction::execute` returns error | Error propagated, partial results NOT cached |

---

## 10. Performance Expectations

| Metric | Phase 1 (sync) | Phase 2.2 (async) | Notes |
|--------|----------------|-------------------|-------|
| Single Get latency | ~same | ~same + 5μs overhead | BoxFuture alloc + spawn_blocking |
| 10 concurrent Gets | 10x serial | ~1x (overlapped) | Major improvement |
| Cache lock hold time | Entire materialization | ~1μs per get/put | Major improvement |
| Tokio thread utilization | 1 blocked per Get | All threads available | Better under load |
| Memory per Get | Stack frames | Heap futures (~1KB) | Slightly more heap |

---

## 11. Files Changed

| File | Change |
|------|--------|
| `deriva-compute/src/async_executor.rs` | **NEW** — AsyncExecutor with BoxFuture recursion |
| `deriva-compute/src/cache.rs` | Add `AsyncMaterializationCache` trait, `SharedCache` wrapper |
| `deriva-compute/src/leaf_store.rs` | Add `AsyncLeafStore` trait with blanket impl |
| `deriva-compute/src/lib.rs` | Export `async_executor` module |
| `deriva-compute/Cargo.toml` | Add `async-trait`, `futures`, `tokio` dependencies |
| `deriva-server/src/state.rs` | Replace `RwLock<DagStore>` + `RwLock<EvictableCache>` with Arc-based |
| `deriva-server/src/service.rs` | Rewrite all RPCs to use async executor, remove std locks |
| `deriva-compute/tests/async_executor.rs` | **NEW** — ~14 unit tests |
| `deriva-server/tests/integration.rs` | Add ~4 concurrency integration tests |

---

## 12. Dependency Changes

### 12.1 `deriva-compute/Cargo.toml`

```toml
[dependencies]
# Existing
bytes = "1"
deriva-core = { path = "../deriva-core" }

# New for Phase 2.2
async-trait = "0.1"
futures = "0.3"
tokio = { version = "1", features = ["rt", "sync", "macros"] }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "test-util"] }
```

### 12.2 `deriva-server/Cargo.toml`

No new dependencies — already has `tokio` and `tonic`.

---

## 13. Design Rationale & Alternatives

### 13.1 Why Not `async-recursion` Crate?

The `async-recursion` crate provides a proc macro that automatically boxes recursive
async functions:

```rust
#[async_recursion]
async fn materialize(&self, addr: CAddr) -> Result<Bytes> { ... }
```

We use manual `BoxFuture` instead because:
- One less dependency
- Explicit about the heap allocation
- Easier to reason about lifetime bounds
- `async-recursion` is a thin wrapper around `BoxFuture` anyway

### 13.2 Why `tokio::sync::RwLock` Instead of `std::sync::RwLock`?

`std::sync::RwLock` blocks the OS thread. In an async context, this blocks the tokio
worker thread, reducing parallelism. `tokio::sync::RwLock` yields the task instead:

```
std::sync::RwLock:
  Task A holds lock → Task B calls lock() → OS thread BLOCKED
  Other tasks on same thread: STARVED

tokio::sync::RwLock:
  Task A holds lock → Task B calls lock().await → Task B YIELDS
  Other tasks on same thread: can run while B waits
```

### 13.3 Why Not Make ComputeFunction Async?

Making `ComputeFunction::execute` async would require:
- All function implementations to be async
- `async_trait` on the trait (heap allocation per call)
- No benefit — compute functions are CPU-bound, not I/O-bound

`spawn_blocking` is the correct pattern for CPU-bound work in async contexts.

### 13.4 Relationship to §2.3 (Parallel Materialization)

§2.2 makes the engine async but keeps input resolution sequential:

```rust
// §2.2: sequential
for input_addr in &recipe.inputs {
    let bytes = self.materialize(*input_addr).await;
    input_bytes.push(bytes);
}
```

§2.3 will change this to parallel:

```rust
// §2.3: parallel (preview)
let futures: Vec<_> = recipe.inputs.iter()
    .map(|addr| self.materialize(*addr))
    .collect();
let input_bytes = futures::future::try_join_all(futures).await?;
```

The async foundation from §2.2 makes this a one-line change. Without async, parallel
materialization would require thread pools, channels, and complex synchronization.

---

## 14. Checklist

- [ ] Create `AsyncMaterializationCache` trait
- [ ] Implement `SharedCache` wrapper with `tokio::sync::RwLock`
- [ ] Create `AsyncLeafStore` trait with blanket impl
- [ ] Create `DagReader` trait
- [ ] Implement `CombinedDagReader` (PersistentDag + SledRecipeStore)
- [ ] Create `AsyncExecutor` with `BoxFuture` recursion
- [ ] Add `spawn_blocking` for compute function execution
- [ ] Update `ServerState` to use `Arc`-based shared ownership
- [ ] Rewrite `DerivaService` RPCs to use async executor
- [ ] Remove `std::sync::RwLock` from DAG and cache access in service
- [ ] Keep sync `Executor` for backward compat
- [ ] Update `lib.rs` exports
- [ ] Add `async-trait`, `futures`, `tokio` to `deriva-compute/Cargo.toml`
- [ ] Write unit tests (~14)
- [ ] Write integration tests (~4)
- [ ] Run full test suite — all previous + new tests pass
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
