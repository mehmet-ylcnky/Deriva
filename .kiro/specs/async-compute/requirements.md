# Requirements Document

## Introduction

Phase 2.2 of the Computation-Addressed Distributed File System (Deriva/CAS-DFS) replaces the synchronous Executor with an async Tokio-based engine. The Async Compute Engine yields during I/O operations (cache and storage reads), enables fine-grained locking for concurrent request handling, eliminates stack overflow risk for deep DAGs via heap-allocated futures, and prepares the foundation for parallel materialization in Phase 2.3.

## Glossary

- **AsyncExecutor**: The async Tokio-based computation engine that resolves CAddr values by recursively traversing the DAG, checking cache and leaf stores, and executing compute functions.
- **AsyncMaterializationCache**: An async trait providing `get`, `put`, and `contains` methods for caching materialized computation results.
- **SharedCache**: A concrete implementation of AsyncMaterializationCache that wraps an EvictableCache with a `tokio::sync::RwLock` for interior mutability.
- **AsyncLeafStore**: An async trait providing `get_leaf` for retrieving raw leaf data by address.
- **DagReader**: A synchronous trait that abstracts DAG access, providing `get_inputs` and `get_recipe` methods over PersistentDag and RecipeStore.
- **CombinedDagReader**: A struct that combines PersistentDag (for graph edges/inputs) with a RecipeStore (for full recipe data) to implement DagReader.
- **RecipeStore**: A shared trait in `deriva-core` for recipe storage access, returning `Option<Recipe>` by CAddr.
- **CAddr**: A content address (hash) that uniquely identifies a computation result or leaf value.
- **Recipe**: A computation definition consisting of a function identifier, input addresses, and parameters.
- **BoxFuture**: A heap-allocated future (`Pin<Box<dyn Future>>`) enabling async recursion without growing the call stack.
- **spawn_blocking**: A Tokio function that moves CPU-bound work to a dedicated blocking thread pool, preventing starvation of the async runtime.
- **Executor**: The original synchronous executor from Phase 1, retained for unit tests and benchmarking.
- **Semaphore**: A Tokio concurrency primitive used to limit concurrent compute operations and prevent resource exhaustion.
- **InFlightMap**: A mutex-protected map of CAddr to broadcast channels, used for deduplicating concurrent requests for the same address.

## Requirements

### Requirement 1: Async Cache Trait

**User Story:** As a system developer, I want an async cache trait with interior mutability, so that multiple concurrent tasks can access the materialization cache without holding exclusive locks for the entire materialization duration.

#### Acceptance Criteria

1. THE AsyncMaterializationCache trait SHALL provide async `get`, `put`, and `contains` methods that accept `&self` (shared reference).
2. THE SharedCache SHALL wrap an EvictableCache with a `tokio::sync::RwLock` for interior mutability.
3. WHEN `get` is called on SharedCache, THE SharedCache SHALL acquire a write lock (because get updates access_count and last_accessed metadata) and release it within the duration of the single cache lookup operation.
4. WHEN `contains` is called on SharedCache, THE SharedCache SHALL acquire only a read lock.
5. WHEN `put` is called on SharedCache, THE SharedCache SHALL acquire a write lock and release it within the duration of the single cache insertion operation.
6. THE AsyncMaterializationCache trait SHALL require `Send + Sync` bounds so that implementations can be shared across tokio tasks.

### Requirement 2: Async Leaf Store Trait

**User Story:** As a system developer, I want an async leaf store trait with a blanket implementation for sync stores, so that existing synchronous LeafStore implementations can be used without modification in the async engine.

#### Acceptance Criteria

1. THE AsyncLeafStore trait SHALL provide an async `get_leaf` method that returns `Option<Bytes>` for a given CAddr.
2. THE AsyncLeafStore trait SHALL require `Send + Sync` bounds.
3. WHEN a type implements the synchronous `LeafStore` trait and is `Send + Sync`, THE system SHALL provide a blanket implementation of AsyncLeafStore that delegates to the synchronous `get_leaf` method.

### Requirement 3: DagReader Abstraction

**User Story:** As a system developer, I want a DagReader trait that abstracts over different DAG storage backends, so that the AsyncExecutor can work with both in-memory DagStore (for tests) and PersistentDag + RecipeStore (for production).

#### Acceptance Criteria

1. THE DagReader trait SHALL provide synchronous `get_inputs` and `get_recipe` methods (sled reads complete in less than 5 microseconds, making async unnecessary).
2. THE DagReader trait SHALL require `Send + Sync` bounds.
3. THE DagStore SHALL implement DagReader directly for unit test usage.
4. THE CombinedDagReader SHALL combine a PersistentDag (for input/edge queries) with a RecipeStore (for full recipe retrieval) into a single DagReader implementation.
5. THE CombinedDagReader SHALL be generic over any type implementing RecipeStore.

### Requirement 4: RecipeStore Trait

**User Story:** As a system developer, I want a shared RecipeStore trait in `deriva-core`, so that recipe storage access can be abstracted independently from the DAG structure and reused across different modules.

#### Acceptance Criteria

1. THE RecipeStore trait SHALL provide a `get` method that returns `Result<Option<Recipe>>` for a given CAddr.
2. THE RecipeStore trait SHALL require `Send + Sync` bounds.
3. THE RecipeStore trait SHALL reside in `deriva-core` so that both `deriva-compute` and `deriva-storage` can depend on it without circular dependencies.

### Requirement 5: AsyncExecutor Recursive Materialization

**User Story:** As a system developer, I want an async executor that resolves CAddr values by recursively traversing the DAG, so that the system can yield during I/O and support concurrent Get requests without blocking the tokio runtime.

#### Acceptance Criteria

1. THE AsyncExecutor SHALL use `Arc` for shared ownership of cache, leaf_store, dag, and registry (no lifetime parameters).
2. THE AsyncExecutor SHALL implement `materialize` using `BoxFuture` for async recursion, achieving O(1) stack depth regardless of DAG depth.
3. WHEN `materialize` is called, THE AsyncExecutor SHALL first check the cache, then the leaf store, then look up the recipe from the DagReader, resolve inputs, execute the compute function, and cache the result.
4. WHEN a value is found in cache, THE AsyncExecutor SHALL return it immediately without consulting the leaf store or DAG.
5. WHEN a value is found in the leaf store, THE AsyncExecutor SHALL return it immediately without consulting the DAG.
6. IF a recipe is not found in the DAG for a given CAddr, THEN THE AsyncExecutor SHALL return a `NotFound` error.
7. IF a function referenced by a recipe is not found in the registry, THEN THE AsyncExecutor SHALL return a `FunctionNotFound` error.
8. WHEN a compute function fails, THE AsyncExecutor SHALL propagate the error and not cache the failed result.

### Requirement 6: CPU-Bound Compute Isolation

**User Story:** As a system developer, I want CPU-bound compute functions to execute on a blocking thread pool, so that they do not starve the tokio async runtime of worker threads.

#### Acceptance Criteria

1. WHEN executing a compute function, THE AsyncExecutor SHALL use `tokio::task::spawn_blocking` to move the work to the blocking thread pool.
2. IF `spawn_blocking` returns a join error (task panic or cancellation), THEN THE AsyncExecutor SHALL return a `ComputeFailed` error with a descriptive message.
3. THE AsyncExecutor SHALL await the blocking task result and resume on the async runtime after completion.

### Requirement 7: Parallel Input Resolution

**User Story:** As a system developer, I want the executor to resolve independent recipe inputs in parallel, so that fan-in patterns complete in wall-clock time proportional to the longest branch rather than the sum of all branches.

#### Acceptance Criteria

1. WHEN a recipe has multiple inputs, THE AsyncExecutor SHALL resolve all inputs concurrently using `futures::future::try_join_all`.
2. IF any input resolution fails, THEN THE AsyncExecutor SHALL short-circuit and propagate the first error encountered.
3. THE AsyncExecutor SHALL collect all resolved input bytes in the same order as the recipe's input list before executing the compute function.

### Requirement 8: Concurrent Request Deduplication

**User Story:** As a system developer, I want concurrent requests for the same CAddr to be deduplicated, so that only one computation executes while subsequent requesters wait for the result via a broadcast channel.

#### Acceptance Criteria

1. WHEN a materialization for a CAddr is already in-flight, THE AsyncExecutor SHALL subscribe the new requester to the existing broadcast channel instead of starting a duplicate computation.
2. WHEN a producer completes materialization (success or error), THE AsyncExecutor SHALL broadcast the result to all subscribed receivers.
3. WHEN the producer finishes broadcasting, THE AsyncExecutor SHALL remove the CAddr from the in-flight map.
4. IF a broadcast channel receive fails (producer dropped), THEN THE AsyncExecutor SHALL return a `ComputeFailed` error indicating the producer task was cancelled.

### Requirement 9: Semaphore-Based Concurrency Control

**User Story:** As a system developer, I want a semaphore to limit concurrent compute operations, so that resource exhaustion is prevented and deep DAGs do not deadlock.

#### Acceptance Criteria

1. THE AsyncExecutor SHALL acquire a semaphore permit only at the compute step (after input resolution completes), not during input resolution.
2. THE ExecutorConfig SHALL provide a configurable `max_concurrency` setting (default: `num_cpus * 2`).
3. WHEN the semaphore cannot be acquired (closed), THE AsyncExecutor SHALL return a `ComputeFailed` error.
4. THE AsyncExecutor SHALL release the semaphore permit after the compute function completes (before caching the result).

### Requirement 10: Verification Mode for Determinism Checking

**User Story:** As a system developer, I want optional dual-compute verification, so that non-deterministic compute functions can be detected by executing them twice and comparing outputs byte-for-byte.

#### Acceptance Criteria

1. THE VerificationMode enum SHALL support three modes: `Off` (single execution, production default), `DualCompute` (parallel dual execution for all recipes), and `Sampled { rate }` (deterministic sampling of a fraction of recipes).
2. WHEN VerificationMode is `DualCompute` or a sampled recipe is selected, THE AsyncExecutor SHALL execute the compute function twice in parallel using `tokio::join!` and compare outputs byte-for-byte.
3. IF dual-compute outputs differ, THEN THE AsyncExecutor SHALL return a `DeterminismViolation` error containing the address, function ID, output hashes, and output lengths.
4. THE VerificationStats SHALL track total verified, passed, and failed counts using atomic counters.
5. WHEN VerificationMode is `Sampled { rate }`, THE AsyncExecutor SHALL use deterministic sampling based on the first byte of the address (`addr.as_bytes()[0] / 255.0 < rate`), ensuring the same address always receives the same verification decision.

### Requirement 11: Observability Integration

**User Story:** As a system developer, I want the async executor to emit Prometheus metrics and tracing spans, so that materialization performance and behavior can be monitored in production.

#### Acceptance Criteria

1. THE AsyncExecutor SHALL record materialization duration via a histogram metric for each completed materialization.
2. THE AsyncExecutor SHALL track active materializations via a gauge metric that increments on entry and decrements on exit.
3. THE AsyncExecutor SHALL count materialization outcomes (cache_hit, leaf, computed) via a counter metric with outcome labels.
4. THE AsyncExecutor SHALL count cache accesses (hit, miss) via a counter metric.
5. THE AsyncExecutor SHALL record compute function duration, input bytes, and output bytes via histogram metrics labeled by function name.
6. WHEN `materialize` is called, THE AsyncExecutor SHALL create a tracing span containing the target address for structured logging.

### Requirement 12: Backward Compatibility with Synchronous Executor

**User Story:** As a system developer, I want the original synchronous Executor to remain available, so that unit tests without a tokio runtime and sync-vs-async benchmarks continue to work.

#### Acceptance Criteria

1. THE synchronous `MaterializationCache` trait (with `&mut self` methods) SHALL remain unchanged and available.
2. THE synchronous `LeafStore` trait SHALL remain unchanged and available.
3. THE synchronous `Executor` module SHALL remain exported from `deriva-compute`.
4. THE `deriva-compute` crate SHALL export both `Executor` (sync) and `AsyncExecutor` (async) from its public API.

### Requirement 13: Heap-Allocated Futures for Deep DAGs

**User Story:** As a system developer, I want async recursion via BoxFuture to eliminate stack overflow risk, so that arbitrarily deep DAGs can be materialized safely.

#### Acceptance Criteria

1. THE AsyncExecutor SHALL use `BoxFuture` (heap-allocated futures) for recursive materialization calls, resulting in O(1) stack depth.
2. WHEN materializing a DAG of depth N, THE AsyncExecutor SHALL allocate approximately N heap futures (approximately 50-100 bytes each) rather than N stack frames.
3. WHEN materializing a DAG of depth 50 or more, THE AsyncExecutor SHALL complete without stack overflow.

### Requirement 14: Error Propagation and Non-Caching of Failures

**User Story:** As a system developer, I want errors to propagate correctly and not be cached, so that transient failures can be retried and permanent failures are reported accurately.

#### Acceptance Criteria

1. IF materialization of a CAddr fails (NotFound, FunctionNotFound, ComputeFailed, or DeterminismViolation), THEN THE AsyncExecutor SHALL NOT cache the error result.
2. WHEN an error occurs during materialization, THE AsyncExecutor SHALL broadcast the error to all deduplicated subscribers waiting for that CAddr.
3. WHEN an error occurs, THE AsyncExecutor SHALL clean up the in-flight map entry for that CAddr before returning.
4. THE AsyncExecutor SHALL propagate the original error type without wrapping it in additional layers (except for spawn_blocking join errors which are mapped to ComputeFailed).
