# Section 2.3: Parallel Materialization

> **Goal:** Resolve independent DAG branches concurrently using `try_join_all`,
> turning the sequential input loop from §2.2 into parallel fan-out.
>
> **Crate:** `deriva-compute` (modify `async_executor.rs`)
>
> **Estimated tests:** ~16
>
> **Commit message pattern:** `2.3: Parallel Materialization — concurrent independent branch resolution`

---

## 1. Problem Statement

After §2.2, the `AsyncExecutor` resolves inputs sequentially:

```rust
// §2.2 AsyncExecutor::materialize — sequential input resolution
let mut input_bytes = Vec::with_capacity(recipe.inputs.len());
for input_addr in &recipe.inputs {
    let bytes = self.materialize(*input_addr).await?;  // waits for each
    input_bytes.push(bytes);
}
```

For a recipe with N independent inputs, each taking T time to materialize:
- **Sequential:** N × T total time
- **Parallel:** max(T₁, T₂, ..., Tₙ) total time

### Motivating Example: Diamond DAG

```
        leaf_a    leaf_b    leaf_c    leaf_d
          │         │         │         │
          ▼         ▼         ▼         ▼
       transform  transform  transform  transform
       (100ms)    (100ms)    (100ms)    (100ms)
          │         │         │         │
          └────┬────┘         └────┬────┘
               ▼                   ▼
            merge_ab            merge_cd
            (50ms)              (50ms)
               │                   │
               └────────┬─────────┘
                        ▼
                    final_merge
                      (50ms)

Sequential: 4×100 + 2×50 + 50 = 550ms
Parallel:   100 + 50 + 50 = 200ms  (2.75x speedup)
```

### Quantified Impact by DAG Shape

| DAG Shape | Inputs | Sequential | Parallel | Speedup |
|-----------|--------|-----------|----------|---------|
| Linear chain (A→B→C→D) | 1 each | 4T | 4T | 1x (no parallelism) |
| Wide fan-in (4 leaves → merge) | 4 | 4T | T | 4x |
| Diamond (above) | mixed | 550ms | 200ms | 2.75x |
| Binary tree depth 4 | 2 each | 15T | 4T | 3.75x |
| Star (16 leaves → merge) | 16 | 16T | T | 16x |

The wider the fan-in, the greater the speedup. Linear chains see no benefit.

---

## 2. Design

### 2.3.1 Core Change: One Line

The entire parallel materialization change is conceptually one line:

```rust
// Before (§2.2 — sequential):
let mut input_bytes = Vec::with_capacity(recipe.inputs.len());
for input_addr in &recipe.inputs {
    input_bytes.push(self.materialize(*input_addr).await?);
}

// After (§2.3 — parallel):
let futures: Vec<_> = recipe.inputs.iter()
    .map(|addr| self.materialize(*addr))
    .collect();
let input_bytes = futures::future::try_join_all(futures).await?;
```

`try_join_all` runs all futures concurrently on the tokio runtime. If any fails,
all others are cancelled and the first error is returned.

### 2.3.2 Architecture

```
§2.2 Sequential:                    §2.3 Parallel:

materialize(final)                  materialize(final)
  │                                   │
  ├─ materialize(input_1).await       ├─┬─ materialize(input_1) ─┐
  │    └─ ... (100ms)                 │ ├─ materialize(input_2) ─┤
  ├─ materialize(input_2).await       │ ├─ materialize(input_3) ─┤ try_join_all
  │    └─ ... (100ms)                 │ └─ materialize(input_4) ─┘
  ├─ materialize(input_3).await       │         │ concurrent
  │    └─ ... (100ms)                 │         ▼
  └─ materialize(input_4).await       ├─ all inputs ready (max 100ms)
       └─ ... (100ms)                 │
  Total: 400ms                        └─ execute function
                                      Total: ~100ms
```

### 2.3.3 Concurrency Control

Unbounded parallelism is dangerous. A recipe with 1000 inputs would spawn 1000
concurrent materialization trees. Each tree may spawn more. This can:
- Exhaust memory (thousands of in-flight futures)
- Overwhelm sled with concurrent reads
- Cause cache thrashing

**Solution: Semaphore-bounded concurrency**

```rust
use tokio::sync::Semaphore;

pub struct AsyncExecutor<C, L, D> {
    cache: Arc<C>,
    leaf_store: Arc<L>,
    dag: Arc<D>,
    registry: Arc<FunctionRegistry>,
    concurrency: Arc<Semaphore>,  // NEW — limits parallel materializations
}
```

Default concurrency limit: `num_cpus * 2` (e.g., 16 on an 8-core machine).
This bounds the total number of concurrent materialization tasks across the
entire executor, not per-recipe.

```
With semaphore (limit=8):

Recipe with 16 inputs:
  Batch 1: inputs 1-8  (concurrent, ~100ms)
  Batch 2: inputs 9-16 (concurrent, ~100ms)
  Total: ~200ms (vs 1600ms sequential, vs ~100ms unbounded)

The semaphore doesn't batch explicitly — it just limits how many
materialize() calls can be active simultaneously. Tokio schedules
them as permits become available.
```

### 2.3.4 Deduplication: Concurrent Requests for Same CAddr

When parallel branches share inputs, the same CAddr may be materialized
concurrently by multiple tasks:

```
        leaf_a
       /      \
    r1(a)    r2(a)     ← both need to materialize leaf_a
       \      /
      merge(r1, r2)
```

Without deduplication, `leaf_a` is fetched twice. With many shared inputs,
this wastes compute and I/O.

**Solution: In-flight request map**

```rust
use tokio::sync::broadcast;
use std::collections::HashMap;
use tokio::sync::Mutex;

pub struct AsyncExecutor<C, L, D> {
    // ... existing fields ...
    in_flight: Mutex<HashMap<CAddr, broadcast::Sender<Result<Bytes>>>>,
}
```

When `materialize(addr)` is called:
1. Check cache → hit? return immediately
2. Check `in_flight` → someone already computing this addr?
   - Yes → subscribe to their broadcast channel, await result
   - No → insert sender into `in_flight`, compute, broadcast result, remove from map

```
Without dedup:                    With dedup:

Task A: materialize(leaf_a)       Task A: materialize(leaf_a)
Task B: materialize(leaf_a)         ├─ in_flight.insert(leaf_a, tx)
  │         │                        ├─ compute...
  │         │                       Task B: materialize(leaf_a)
  ▼         ▼                         ├─ in_flight.get(leaf_a) → rx
compute   compute                     └─ rx.recv().await
  │         │                       Task A:
  ▼         ▼                         ├─ tx.send(result)
result    result                      └─ in_flight.remove(leaf_a)
                                    
2x compute                          1x compute, 2x result
```

---

## 3. Implementation

### 3.1 Updated `AsyncExecutor`

```rust
// deriva-compute/src/async_executor.rs — updated

use crate::cache::AsyncMaterializationCache;
use crate::leaf_store::AsyncLeafStore;
use crate::registry::FunctionRegistry;
use bytes::Bytes;
use deriva_core::address::CAddr;
use deriva_core::error::{DerivaError, Result};
use futures::future::{BoxFuture, try_join_all};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, Semaphore};

pub trait DagReader: Send + Sync {
    fn get_inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>>;
    fn get_recipe(&self, addr: &CAddr) -> Result<Option<deriva_core::address::Recipe>>;
}

pub struct ExecutorConfig {
    /// Max concurrent materialization tasks (default: num_cpus * 2)
    pub max_concurrency: usize,
    /// Broadcast channel capacity for dedup (default: 16)
    pub dedup_channel_capacity: usize,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_concurrency: num_cpus::get() * 2,
            dedup_channel_capacity: 16,
        }
    }
}

pub struct AsyncExecutor<C, L, D> {
    cache: Arc<C>,
    leaf_store: Arc<L>,
    dag: Arc<D>,
    registry: Arc<FunctionRegistry>,
    semaphore: Arc<Semaphore>,
    in_flight: Arc<Mutex<HashMap<CAddr, broadcast::Sender<Result<Bytes>>>>>,
    config: ExecutorConfig,
}

impl<C, L, D> Clone for AsyncExecutor<C, L, D> {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            leaf_store: Arc::clone(&self.leaf_store),
            dag: Arc::clone(&self.dag),
            registry: Arc::clone(&self.registry),
            semaphore: Arc::clone(&self.semaphore),
            in_flight: Arc::clone(&self.in_flight),
            config: ExecutorConfig {
                max_concurrency: self.config.max_concurrency,
                dedup_channel_capacity: self.config.dedup_channel_capacity,
            },
        }
    }
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
        Self::with_config(dag, registry, cache, leaf_store, ExecutorConfig::default())
    }

    pub fn with_config(
        dag: Arc<D>,
        registry: Arc<FunctionRegistry>,
        cache: Arc<C>,
        leaf_store: Arc<L>,
        config: ExecutorConfig,
    ) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrency));
        Self {
            cache, leaf_store, dag, registry, semaphore,
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    pub fn materialize(&self, addr: CAddr) -> BoxFuture<'_, Result<Bytes>> {
        Box::pin(async move {
            // 1. Cache check (no semaphore needed)
            if let Some(bytes) = self.cache.get(&addr).await {
                return Ok(bytes);
            }

            // 2. Leaf check (no semaphore needed)
            if let Some(bytes) = self.leaf_store.get_leaf(&addr).await {
                return Ok(bytes);
            }

            // 3. Dedup check — is someone already computing this?
            {
                let map = self.in_flight.lock().await;
                if let Some(tx) = map.get(&addr) {
                    let mut rx = tx.subscribe();
                    drop(map); // release lock before awaiting
                    return rx.recv().await
                        .map_err(|_| DerivaError::ComputeFailed(
                            "in-flight broadcast dropped".into()
                        ))?;
                }
            }

            // 4. Register as the producer for this addr
            let (tx, _) = broadcast::channel(self.config.dedup_channel_capacity);
            {
                let mut map = self.in_flight.lock().await;
                map.insert(addr, tx.clone());
            }

            // 5. Acquire semaphore permit (bounds total concurrency)
            let _permit = self.semaphore.acquire().await
                .map_err(|_| DerivaError::ComputeFailed("semaphore closed".into()))?;

            // 6. Recipe lookup
            let recipe = self.dag.get_recipe(&addr)?
                .ok_or_else(|| DerivaError::NotFound(addr.to_string()))?;

            // 7. PARALLEL input resolution
            let futures: Vec<_> = recipe.inputs.iter()
                .map(|input| self.materialize(*input))
                .collect();
            let input_bytes = try_join_all(futures).await?;

            // 8. Execute compute function
            let func = self.registry.get(&recipe.function_id)
                .ok_or_else(|| DerivaError::FunctionNotFound(
                    recipe.function_id.to_string()
                ))?;
            let params = recipe.params.clone();
            let output = tokio::task::spawn_blocking(move || {
                func.execute(input_bytes, &params)
            })
            .await
            .map_err(|e| DerivaError::ComputeFailed(format!("join: {}", e)))?
            .map_err(|e| DerivaError::ComputeFailed(e.to_string()))?;

            // 9. Cache result
            self.cache.put(addr, output.clone()).await;

            // 10. Broadcast result to waiters and clean up
            let _ = tx.send(Ok(output.clone()));
            {
                let mut map = self.in_flight.lock().await;
                map.remove(&addr);
            }

            Ok(output)
        })
    }
}
```

### 3.2 Error Propagation in Parallel

When `try_join_all` encounters an error, it:
1. Returns the first error immediately
2. Drops all other futures (cancelling them)

This is the correct behavior — if one input fails, the recipe can't be computed.
The cancelled futures release their semaphore permits via Drop.

```
try_join_all([mat(a), mat(b), mat(c)])
                │         │         │
                OK        Err!      ─── cancelled (dropped)
                │         │
                ─── cancelled        
                          │
                    return Err
```

For the dedup broadcast, if the producer fails:

```rust
// In the error path, broadcast the error too:
let result = self.compute_inner(addr, recipe).await;
match &result {
    Ok(output) => { let _ = tx.send(Ok(output.clone())); }
    Err(e) => { let _ = tx.send(Err(e.clone())); }  // DerivaError must be Clone
}
self.in_flight.lock().await.remove(&addr);
result
```

This requires `DerivaError: Clone`. Check if it already is — if not, add `#[derive(Clone)]`
to `DerivaError` in `deriva-core/src/error.rs`.

---

## 4. Data Flow Diagrams

### 4.1 Parallel Fan-Out with Semaphore

```
materialize(final_merge)
    │
    ├── recipe.inputs = [merge_ab, merge_cd]
    │
    ├── try_join_all ──┬── materialize(merge_ab) ──────────────────┐
    │                  │     │                                      │
    │                  │     ├── try_join_all ──┬── mat(transform_a)│
    │                  │     │                  └── mat(transform_b)│
    │                  │     │                     (concurrent)     │
    │                  │     └── execute merge_ab                   │
    │                  │                                            │
    │                  └── materialize(merge_cd) ──────────────────┤
    │                        │                                      │
    │                        ├── try_join_all ──┬── mat(transform_c)│
    │                        │                  └── mat(transform_d)│
    │                        │                     (concurrent)     │
    │                        └── execute merge_cd                   │
    │                                                               │
    │                  ◄── all 4 transforms run concurrently! ─────┘
    │
    └── execute final_merge(merge_ab_result, merge_cd_result)
```

### 4.2 Semaphore Flow (limit=4)

```
Time ──────────────────────────────────────────────────▶

Permits: [4 available]

mat(transform_a): acquire ─── [compute 100ms] ─── release
mat(transform_b): acquire ─── [compute 100ms] ─── release
mat(transform_c): acquire ─── [compute 100ms] ─── release
mat(transform_d): acquire ─── [compute 100ms] ─── release
                  ▲ all 4 acquired simultaneously (limit=4)

mat(merge_ab):    ──── wait for a,b ──── acquire ─── [50ms] ─── release
mat(merge_cd):    ──── wait for c,d ──── acquire ─── [50ms] ─── release
                                         ▲ permits available after transforms done

mat(final):       ──── wait for ab,cd ──── acquire ─── [50ms] ─── release

Total: 100 + 50 + 50 = 200ms
```

### 4.3 Deduplication Flow

```
materialize(merge) needs inputs [r1(leaf_a), r2(leaf_a)]
    │
    ├── try_join_all:
    │     ├── mat(r1) → needs leaf_a → mat(leaf_a)  [Task A]
    │     └── mat(r2) → needs leaf_a → mat(leaf_a)  [Task B]
    │
    │   Task A arrives first:
    │     in_flight.insert(leaf_a, tx)
    │     compute leaf_a...
    │
    │   Task B arrives second:
    │     in_flight.get(leaf_a) → Some(tx)
    │     rx = tx.subscribe()
    │     rx.recv().await  ← waits for Task A
    │
    │   Task A completes:
    │     tx.send(Ok(leaf_a_data))
    │     in_flight.remove(leaf_a)
    │
    │   Task B receives:
    │     rx.recv() → Ok(leaf_a_data)
    │
    └── Both r1 and r2 have leaf_a data, only computed once
```

### 4.4 Error Cancellation Flow

```
try_join_all([mat(a), mat(b), mat(c)])

mat(a): ─── computing... ─── Ok(data_a)
mat(b): ─── computing... ─── Err(NotFound)  ← FAILS
mat(c): ─── computing... ─── CANCELLED (future dropped)
                                    │
                                    ▼
                              Semaphore permit released via Drop
                              In-flight entry cleaned up via Drop

try_join_all returns: Err(NotFound)
```

---

## 5. Test Specification

### 5.1 Unit Tests: `deriva-compute/tests/parallel_materialization.rs`

```rust
// Test helpers — reuse from §2.2 tests, add timing utilities
use std::time::Instant;
use tokio::time::Duration;

/// A slow compute function that sleeps for a configurable duration.
/// Used to verify parallel execution by measuring wall-clock time.
struct SlowFunction {
    id: FunctionId,
    delay_ms: u64,
}

impl ComputeFunction for SlowFunction {
    fn id(&self) -> FunctionId { self.id.clone() }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        std::thread::sleep(Duration::from_millis(self.delay_ms));
        // Concat all inputs
        let mut out = Vec::new();
        for input in inputs { out.extend_from_slice(&input); }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: self.delay_ms, memory_bytes: 0 }
    }
}
```

```
#[tokio::test]
test_parallel_fan_in_faster_than_sequential
    Register SlowFunction with 100ms delay
    Create 4 leaves, 4 recipes (each: slow_fn(leaf_i)), 1 merge recipe
    let start = Instant::now();
    executor.materialize(merge_addr).await.unwrap();
    let elapsed = start.elapsed();
    // Sequential would be 4*100 + merge = ~400ms+
    // Parallel should be ~100ms + merge
    assert!(elapsed < Duration::from_millis(300));

#[tokio::test]
test_parallel_linear_chain_no_speedup
    Create chain: leaf → slow(leaf) → slow(r1) → slow(r2)
    let start = Instant::now();
    executor.materialize(r2_addr).await.unwrap();
    let elapsed = start.elapsed();
    // Linear chain: no parallelism possible, should be ~300ms
    assert!(elapsed > Duration::from_millis(250));

#[tokio::test]
test_parallel_diamond_dag
    leaf_a → slow(a)=r1, leaf_a → slow(a)=r2, r1+r2 → merge
    Verify correct result
    Verify timing shows r1 and r2 ran concurrently

#[tokio::test]
test_parallel_wide_fan_in_16_inputs
    16 leaves → 16 slow recipes → 1 merge
    let start = Instant::now();
    executor.materialize(merge_addr).await.unwrap();
    let elapsed = start.elapsed();
    // With concurrency limit of 16+, should be ~100ms not 1600ms
    assert!(elapsed < Duration::from_millis(400));

#[tokio::test]
test_semaphore_limits_concurrency
    Create executor with max_concurrency=2
    4 leaves → 4 slow(100ms) recipes → merge
    let start = Instant::now();
    executor.materialize(merge_addr).await.unwrap();
    let elapsed = start.elapsed();
    // With limit=2: batch of 2 (100ms) + batch of 2 (100ms) = ~200ms
    assert!(elapsed > Duration::from_millis(150));
    assert!(elapsed < Duration::from_millis(350));

#[tokio::test]
test_dedup_shared_input
    leaf_a → r1(a), r2(a) → merge(r1, r2)
    Use a counting function that tracks invocation count
    executor.materialize(merge_addr).await.unwrap();
    // leaf_a should only be fetched once despite two tasks needing it
    // (dedup via in_flight map)

#[tokio::test]
test_dedup_concurrent_same_addr
    Spawn 10 concurrent materialize(same_addr) calls
    Use counting function
    All should return same result
    Function should execute only once

#[tokio::test]
test_parallel_error_cancels_siblings
    leaf_a (exists), leaf_b (MISSING)
    recipe: concat(leaf_a, leaf_b)
    let result = executor.materialize(recipe_addr).await;
    assert!(result.is_err());
    // leaf_a materialization should be cancelled when leaf_b fails

#[tokio::test]
test_parallel_error_in_deep_branch
    leaf → r1 → r2(missing_input) → merge with r3
    Verify error propagates up correctly
    Verify r3 branch is cancelled

#[tokio::test]
test_parallel_preserves_input_order
    4 leaves with data "a", "b", "c", "d"
    recipe: concat(a, b, c, d)
    result = executor.materialize(recipe_addr).await.unwrap();
    assert_eq!(result, Bytes::from("abcd"));
    // try_join_all preserves order even though execution is concurrent

#[tokio::test]
test_parallel_cache_populated_for_intermediates
    leaf → r1 → r2 → r3
    executor.materialize(r3_addr).await.unwrap();
    // All intermediates should be cached
    assert!(cache.contains(&r1_addr).await);
    assert!(cache.contains(&r2_addr).await);
    assert!(cache.contains(&r3_addr).await);

#[tokio::test]
test_parallel_cached_inputs_skip_compute
    Pre-populate cache with r1_result and r2_result
    recipe: merge(r1, r2)
    Use counting slow function
    executor.materialize(merge_addr).await.unwrap();
    // r1 and r2 should come from cache, not recomputed
    // Only merge function should execute

#[tokio::test]
test_executor_config_custom
    let config = ExecutorConfig {
        max_concurrency: 4,
        dedup_channel_capacity: 32,
    };
    let executor = AsyncExecutor::with_config(dag, reg, cache, leaves, config);
    // Verify it works with custom config

#[tokio::test]
test_parallel_single_input_no_overhead
    recipe with 1 input — should work identically to sequential
    Verify correct result

#[tokio::test]
test_parallel_zero_inputs_recipe
    recipe with empty inputs vec
    Should call function with empty input_bytes
    Verify no panic

#[tokio::test]
test_parallel_stress_100_concurrent
    Create 100 different leaf+recipe pairs
    Spawn 100 concurrent materialize calls
    All should succeed
    No deadlocks, no panics
```

### 5.2 Integration Tests

```
#[tokio::test]
test_parallel_get_rpc_fan_in
    Start server with SlowFunction registered
    Put 4 leaves + 4 slow recipes + 1 merge recipe via gRPC
    Time the Get(merge) RPC
    Verify wall-clock time shows parallel execution

#[tokio::test]
test_parallel_multiple_clients
    Start server
    5 clients each request different recipes simultaneously
    All should complete without deadlock
    Total time < 5x single request
```

---

## 6. Edge Cases & Error Handling

| Case | Expected Behavior |
|------|-------------------|
| Recipe with 0 inputs | `try_join_all([])` returns `Ok(vec![])` immediately |
| Recipe with 1 input | `try_join_all([future])` — no overhead vs sequential |
| All inputs cached | Cache hits return before semaphore acquire — no permit consumed |
| Semaphore closed (executor dropped) | `Err(ComputeFailed("semaphore closed"))` |
| Broadcast receiver dropped before send | `tx.send()` returns error — ignored (result already computed) |
| Dedup race: two tasks check in_flight simultaneously | Mutex serializes — one becomes producer, other becomes consumer |
| Producer panics after registering in in_flight | Broadcast channel dropped → receivers get `RecvError` → `ComputeFailed` |
| Deeply nested parallel (tree depth 20, fan-out 4) | Semaphore prevents explosion — at most `max_concurrency` active |
| Input order must be preserved | `try_join_all` preserves order — output[i] corresponds to input[i] |
| Same CAddr appears twice in recipe.inputs | Dedup ensures single computation, both slots get same result |

### 6.1 Deadlock Analysis

**Can the semaphore deadlock?**

Consider: `materialize(A)` acquires permit, then calls `materialize(B)` which needs a permit.
If all permits are held by tasks waiting for their children, deadlock occurs.

```
Scenario (max_concurrency=2):
  mat(A) acquires permit 1, needs mat(B) and mat(C)
  mat(B) acquires permit 2, needs mat(D)
  mat(C) needs permit — BLOCKED (all 2 permits held)
  mat(B) needs mat(D) — mat(D) needs permit — BLOCKED
  → DEADLOCK: A waits for C, C waits for permit, permits held by A and B
```

**Solution: Acquire semaphore only for the compute step, not the entire materialization.**

```rust
pub fn materialize(&self, addr: CAddr) -> BoxFuture<'_, Result<Bytes>> {
    Box::pin(async move {
        // Cache/leaf checks — no permit needed
        if let Some(bytes) = self.cache.get(&addr).await { return Ok(bytes); }
        if let Some(bytes) = self.leaf_store.get_leaf(&addr).await { return Ok(bytes); }

        // Dedup check — no permit needed
        // ... (same as before)

        // Recipe lookup — no permit needed
        let recipe = self.dag.get_recipe(&addr)?
            .ok_or_else(|| DerivaError::NotFound(addr.to_string()))?;

        // Parallel input resolution — NO PERMIT (recursive calls acquire their own)
        let futures: Vec<_> = recipe.inputs.iter()
            .map(|input| self.materialize(*input))
            .collect();
        let input_bytes = try_join_all(futures).await?;

        // ONLY acquire permit for the compute step
        let _permit = self.semaphore.acquire().await
            .map_err(|_| DerivaError::ComputeFailed("semaphore closed".into()))?;

        let func = self.registry.get(&recipe.function_id)
            .ok_or_else(|| DerivaError::FunctionNotFound(
                recipe.function_id.to_string()
            ))?;
        let params = recipe.params.clone();
        let output = tokio::task::spawn_blocking(move || {
            func.execute(input_bytes, &params)
        }).await
        .map_err(|e| DerivaError::ComputeFailed(format!("join: {}", e)))?
        .map_err(|e| DerivaError::ComputeFailed(e.to_string()))?;

        // Release permit (via _permit drop) before cache write
        drop(_permit);

        self.cache.put(addr, output.clone()).await;

        // Broadcast + cleanup
        let _ = tx.send(Ok(output.clone()));
        self.in_flight.lock().await.remove(&addr);

        Ok(output)
    })
}
```

Now the semaphore only limits concurrent `spawn_blocking` calls (CPU work), not the
entire materialization tree. Recursive calls are free to proceed, and only the actual
compute step is bounded. **No deadlock possible.**

---

## 7. Performance Expectations

| DAG Shape | Sequential (§2.2) | Parallel (§2.3) | Speedup |
|-----------|-------------------|-----------------|---------|
| Linear depth 10, 100ms each | 1000ms | 1000ms | 1x |
| Fan-in 10, 100ms each | 1000ms | ~100ms | ~10x |
| Binary tree depth 4, 50ms each | 750ms | ~200ms | ~3.75x |
| Diamond (4 transforms + 2 merges + final) | 550ms | ~200ms | ~2.75x |
| Star 100 inputs, 10ms each | 1000ms | ~50ms (semaphore batching) | ~20x |

### 7.1 Overhead

| Component | Cost | When |
|-----------|------|------|
| `try_join_all` setup | ~1μs per input | Every recipe |
| `BoxFuture` allocation | ~100 bytes per recursive call | Every non-cached node |
| Semaphore acquire | ~50ns (uncontended) | Every compute step |
| Dedup map lookup | ~200ns (Mutex + HashMap) | Every non-cached node |
| Broadcast channel | ~500 bytes per in-flight addr | Only during active dedup |

Total overhead per materialization: ~2-5μs — negligible vs compute time.

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-compute/src/async_executor.rs` | Add semaphore, in_flight dedup, `try_join_all` |
| `deriva-compute/src/async_executor.rs` | Add `ExecutorConfig`, `Clone` impl |
| `deriva-core/src/error.rs` | Add `Clone` derive to `DerivaError` (for broadcast) |
| `deriva-compute/Cargo.toml` | Add `num_cpus` dependency |
| `deriva-compute/tests/parallel_materialization.rs` | **NEW** — ~16 unit tests |
| `deriva-server/tests/integration.rs` | Add ~2 parallel integration tests |

---

## 9. Dependency Changes

```toml
# deriva-compute/Cargo.toml — add
[dependencies]
num_cpus = "1"
# futures, tokio already added in §2.2
```

---

## 10. Design Rationale & Alternatives

### 10.1 Why `try_join_all` Instead of `FuturesUnordered`?

`FuturesUnordered` polls futures as they complete, which is useful for streaming results.
But we need ALL inputs before executing the function — order matters for `func.execute(inputs)`.

`try_join_all`:
- Preserves input order (output[i] = result of future[i])
- Short-circuits on first error
- Simpler API

`FuturesUnordered` would require collecting into a Vec and re-sorting by index.

### 10.2 Why Broadcast Channel for Dedup Instead of `tokio::sync::watch`?

`watch` only keeps the latest value — if a receiver is slow, it misses the result.
`broadcast` buffers messages, ensuring all subscribers get the result even if they
subscribe slightly late.

Alternative: `shared_future` pattern using `tokio::sync::oneshot` + `Arc<OnceCell>`.
This is simpler but doesn't handle the case where the producer errors — `OnceCell`
can only be set once, so error recovery is harder.

### 10.3 Why Not Use a Task Spawning Model?

Instead of `try_join_all` (which runs futures on the current task), we could spawn
each input as a separate tokio task:

```rust
let handles: Vec<_> = recipe.inputs.iter()
    .map(|addr| tokio::spawn(self.clone().materialize_owned(*addr)))
    .collect();
```

Problems:
- Requires `AsyncExecutor: 'static` (no borrowed references)
- Each task has overhead (~200 bytes + scheduler bookkeeping)
- Harder to cancel on error (need `AbortHandle`)
- `try_join_all` is sufficient and simpler

### 10.4 Relationship to §2.4 (Verification Mode)

Verification mode (§2.4) computes each recipe twice and compares results. With parallel
materialization, the two computations can run concurrently:

```rust
// §2.4 preview: parallel verification
let (result1, result2) = tokio::join!(
    self.compute_once(addr, &recipe, &input_bytes),
    self.compute_once(addr, &recipe, &input_bytes),
);
if result1 != result2 {
    return Err(DerivaError::DeterminismViolation(addr));
}
```

The parallel infrastructure from §2.3 makes this natural.

---

## 11. Benchmarking Plan

### 11.1 Parallel Speedup Benchmark

```rust
// benches/parallel_materialization.rs
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_fan_in(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("parallel_fan_in");

    for fan_in in [1, 2, 4, 8, 16, 32] {
        // Setup: fan_in leaves → fan_in slow(50ms) recipes → 1 merge
        let (executor, merge_addr) = setup_fan_in_dag(fan_in, 50);

        group.bench_with_input(
            BenchmarkId::new("parallel", fan_in),
            &fan_in,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        executor.materialize(merge_addr).await.unwrap()
                    })
                });
            },
        );
    }
    group.finish();
}

fn bench_dedup_effectiveness(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("dedup");

    // Diamond: shared leaf accessed by N branches
    for branches in [2, 4, 8, 16] {
        let (executor, root_addr) = setup_shared_leaf_dag(branches);

        group.bench_with_input(
            BenchmarkId::new("shared_leaf", branches),
            &branches,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        executor.materialize(root_addr).await.unwrap()
                    })
                });
            },
        );
    }
    group.finish();
}
```

### 11.2 Expected Results

```
parallel_fan_in/parallel/1     time: [55 ms  58 ms  61 ms]   ← 1 input, ~50ms compute
parallel_fan_in/parallel/2     time: [55 ms  58 ms  61 ms]   ← 2 concurrent, still ~50ms
parallel_fan_in/parallel/4     time: [56 ms  59 ms  62 ms]   ← 4 concurrent, still ~50ms
parallel_fan_in/parallel/8     time: [57 ms  60 ms  63 ms]   ← 8 concurrent, ~50ms + overhead
parallel_fan_in/parallel/16    time: [58 ms  62 ms  66 ms]   ← 16 concurrent, slight overhead
parallel_fan_in/parallel/32    time: [110 ms 115 ms 120 ms]  ← semaphore batching (limit=16)

dedup/shared_leaf/2            time: [52 ms  55 ms  58 ms]   ← leaf computed once
dedup/shared_leaf/16           time: [53 ms  56 ms  59 ms]   ← still once, 15 deduped
```

### 11.3 Semaphore Tuning Benchmark

```rust
fn bench_semaphore_limits(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("semaphore_tuning");

    // Fixed workload: 32 fan-in, 50ms each
    for limit in [1, 2, 4, 8, 16, 32, 64] {
        let config = ExecutorConfig { max_concurrency: limit, ..Default::default() };
        let (executor, merge_addr) = setup_fan_in_dag_with_config(32, 50, config);

        group.bench_with_input(
            BenchmarkId::new("limit", limit),
            &limit,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        executor.materialize(merge_addr).await.unwrap()
                    })
                });
            },
        );
    }
    group.finish();
}
```

Expected: diminishing returns after `limit >= num_cpus` because `spawn_blocking`
thread pool becomes the bottleneck.

```
semaphore_tuning/limit/1     time: [1600 ms]  ← fully sequential
semaphore_tuning/limit/2     time: [800 ms]
semaphore_tuning/limit/4     time: [400 ms]
semaphore_tuning/limit/8     time: [200 ms]   ← matches CPU cores
semaphore_tuning/limit/16    time: [105 ms]   ← slight improvement
semaphore_tuning/limit/32    time: [100 ms]   ← diminishing returns
semaphore_tuning/limit/64    time: [100 ms]   ← no further improvement
```

---

## 12. Checklist

- [ ] Move semaphore acquire to compute-only scope (prevent deadlock)
- [ ] Add `try_join_all` for parallel input resolution
- [ ] Implement in-flight dedup with broadcast channels
- [ ] Add `ExecutorConfig` with `max_concurrency` and `dedup_channel_capacity`
- [ ] Add `Clone` to `AsyncExecutor`
- [ ] Add `Clone` to `DerivaError`
- [ ] Add `num_cpus` dependency
- [ ] Write `SlowFunction` test helper
- [ ] Write timing-based parallel tests (~10)
- [ ] Write dedup tests (~3)
- [ ] Write error propagation tests (~3)
- [ ] Write integration tests (~2)
- [ ] Verify no deadlock with small semaphore limits
- [ ] Run full test suite — all previous + new tests pass
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
