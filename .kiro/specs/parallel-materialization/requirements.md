# Requirements Document

## Introduction

Phase 2.3 of the Computation-Addressed Distributed File System (Deriva/CAS-DFS) replaces the sequential input resolution loop from Phase 2.2 with parallel fan-out using `futures::future::try_join_all`. This enables concurrent resolution of independent DAG branches during materialization, achieving near-linear speedup proportional to DAG width. The phase also introduces semaphore-based concurrency control to prevent resource exhaustion, in-flight request deduplication to avoid redundant computation of shared sub-DAGs, and deadlock-free design by guarding only the CPU-bound compute step.

## Glossary

- **AsyncExecutor**: The async Tokio-based computation engine that resolves CAddr values by recursively traversing the DAG, checking cache and leaf stores, and executing compute functions.
- **CAddr**: A content address (hash) that uniquely identifies a computation result or leaf value.
- **Recipe**: A computation definition consisting of a function identifier, ordered input addresses, and parameters.
- **try_join_all**: A futures combinator that drives multiple futures concurrently and returns all results in input order, short-circuiting on the first error.
- **Semaphore**: A Tokio concurrency primitive (`tokio::sync::Semaphore`) used to limit the number of concurrent compute operations.
- **InFlightMap**: An `Arc<Mutex<HashMap<CAddr, broadcast::Sender>>>` tracking addresses currently being computed, enabling deduplication of concurrent requests.
- **BroadcastChannel**: A `tokio::sync::broadcast` channel that allows one producer to deliver a result to multiple subscribers waiting for the same CAddr.
- **ExecutorConfig**: A configuration struct controlling `max_concurrency` and `dedup_channel_capacity` for the AsyncExecutor.
- **ComputeStep**: The CPU-bound function execution phase within materialization, after all inputs have been resolved.
- **InputResolution**: The phase of materialization where a recipe's input CAddrs are recursively materialized to obtain their byte contents.
- **DerivaError**: The crate-level error type used throughout the system; requires `Clone` for broadcast propagation.
- **DAG_Width**: The maximum number of independent inputs at any single recipe node, determining potential parallelism.

## Requirements

### Requirement 1: Parallel Input Resolution via try_join_all

**User Story:** As a system developer, I want all recipe inputs resolved concurrently using `try_join_all`, so that fan-in patterns complete in wall-clock time proportional to the longest branch rather than the sum of all branches.

#### Acceptance Criteria

1. WHEN a recipe has multiple inputs, THE AsyncExecutor SHALL create a future for each input and resolve all inputs concurrently using `futures::future::try_join_all`.
2. THE AsyncExecutor SHALL collect all resolved input bytes in the same positional order as the recipe's input list (output[i] corresponds to input[i]) regardless of completion order.
3. WHEN a recipe has zero inputs, THE AsyncExecutor SHALL pass an empty vector to the compute function without error.
4. WHEN a recipe has exactly one input, THE AsyncExecutor SHALL resolve it using the same `try_join_all` path with no additional overhead compared to a direct await.

### Requirement 2: Error Short-Circuiting and Sibling Cancellation

**User Story:** As a system developer, I want parallel input resolution to short-circuit on the first error, so that resources are not wasted computing sibling inputs that can no longer contribute to a successful result.

#### Acceptance Criteria

1. IF any input resolution fails during `try_join_all`, THEN THE AsyncExecutor SHALL return the first error immediately and cancel all remaining sibling futures.
2. WHEN a cancelled future holds a semaphore permit, THE AsyncExecutor SHALL release the permit via the Drop implementation.
3. WHEN a cancelled future is registered as a producer in the InFlightMap, THE AsyncExecutor SHALL ensure the broadcast channel is dropped, causing subscribers to receive a `RecvError`.
4. THE AsyncExecutor SHALL propagate errors from nested parallel resolutions up through all parent `try_join_all` calls without wrapping them in additional error layers.

### Requirement 3: Semaphore-Based Concurrency Control

**User Story:** As a system developer, I want a semaphore to bound the number of concurrent compute operations, so that unbounded parallelism does not exhaust CPU, memory, or I/O resources.

#### Acceptance Criteria

1. THE AsyncExecutor SHALL use a `tokio::sync::Semaphore` to limit the total number of concurrent ComputeStep executions across the entire executor instance.
2. THE ExecutorConfig SHALL provide a configurable `max_concurrency` setting with a default value of `num_cpus::get() * 2`.
3. WHEN the semaphore is closed (executor dropped), THE AsyncExecutor SHALL return a `ComputeFailed` error with the message "semaphore closed".
4. THE AsyncExecutor SHALL store the Semaphore in an `Arc` so that cloned executor instances share the same concurrency limit.

### Requirement 4: Deadlock Prevention via Scoped Permit Acquisition

**User Story:** As a system developer, I want the semaphore permit acquired only at the compute step rather than for the entire materialization, so that recursive DAG traversal cannot deadlock when parent tasks hold permits while waiting for children.

#### Acceptance Criteria

1. THE AsyncExecutor SHALL acquire the semaphore permit only after all inputs have been resolved (immediately before the `spawn_blocking` compute call).
2. THE AsyncExecutor SHALL NOT hold a semaphore permit during the InputResolution phase (recursive `try_join_all` calls).
3. THE AsyncExecutor SHALL release the semaphore permit after the compute function completes and before caching the result.
4. WHEN the semaphore has `max_concurrency` permits and the DAG has depth D with fan-out F, THE AsyncExecutor SHALL complete materialization without deadlock regardless of D and F values.

### Requirement 5: In-Flight Request Deduplication

**User Story:** As a system developer, I want concurrent requests for the same CAddr to be deduplicated via a broadcast channel, so that shared sub-DAG nodes are computed only once even when multiple parallel branches request them simultaneously.

#### Acceptance Criteria

1. WHEN a materialization for a CAddr is already registered in the InFlightMap, THE AsyncExecutor SHALL subscribe the new requester to the existing broadcast channel and await the result.
2. WHEN no in-flight computation exists for a CAddr (and no cache hit), THE AsyncExecutor SHALL register a new broadcast::Sender in the InFlightMap before proceeding with computation.
3. WHEN the producer completes successfully, THE AsyncExecutor SHALL broadcast `Ok(Bytes)` to all subscribers and remove the CAddr from the InFlightMap.
4. WHEN the producer encounters an error, THE AsyncExecutor SHALL broadcast `Err(DerivaError)` to all subscribers and remove the CAddr from the InFlightMap.
5. IF a subscriber receives a `RecvError` (producer dropped without sending), THEN THE AsyncExecutor SHALL return a `ComputeFailed` error with the message "in-flight broadcast dropped".
6. THE AsyncExecutor SHALL release the InFlightMap lock before awaiting the broadcast channel to avoid holding the mutex across an await point.

### Requirement 6: ExecutorConfig for Tunable Concurrency

**User Story:** As a system developer, I want configurable concurrency and channel capacity settings, so that the executor can be tuned for different hardware profiles and workload patterns.

#### Acceptance Criteria

1. THE ExecutorConfig SHALL expose a `max_concurrency` field controlling the semaphore permit count.
2. THE ExecutorConfig SHALL expose a `dedup_channel_capacity` field controlling the broadcast channel buffer size (default: 16).
3. THE AsyncExecutor SHALL provide a `with_config` constructor that accepts an ExecutorConfig and initializes the semaphore and channel capacity accordingly.
4. THE AsyncExecutor SHALL provide a `new` constructor that uses `ExecutorConfig::default()` for backward compatibility.

### Requirement 7: Clone Implementation for Multi-Task Usage

**User Story:** As a system developer, I want AsyncExecutor to implement Clone via Arc-wrapped internals, so that the executor can be shared across multiple tokio tasks and gRPC handlers without lifetime issues.

#### Acceptance Criteria

1. THE AsyncExecutor SHALL wrap all internal fields (cache, leaf_store, dag, registry, semaphore, in_flight) in `Arc` for shared ownership.
2. THE AsyncExecutor SHALL implement `Clone` such that cloned instances share the same underlying cache, semaphore, and InFlightMap.
3. WHEN multiple cloned AsyncExecutor instances materialize concurrently, THE shared semaphore SHALL enforce a single global concurrency limit across all clones.
4. WHEN multiple cloned AsyncExecutor instances materialize the same CAddr concurrently, THE shared InFlightMap SHALL deduplicate the computation across clones.

### Requirement 8: DerivaError Clone Derivation

**User Story:** As a system developer, I want DerivaError to implement Clone, so that errors can be broadcast to multiple subscribers via the deduplication broadcast channel.

#### Acceptance Criteria

1. THE DerivaError type SHALL derive or implement the `Clone` trait.
2. WHEN an error is broadcast to N subscribers, each subscriber SHALL receive an independent clone of the error.
3. THE DerivaError Clone implementation SHALL NOT introduce additional heap allocations beyond those inherent in the error's String fields.

### Requirement 9: Cache and Leaf Bypass Without Permit Consumption

**User Story:** As a system developer, I want cache hits and leaf store hits to return immediately without acquiring a semaphore permit, so that cached results do not consume concurrency capacity and previously computed values are served with minimal latency.

#### Acceptance Criteria

1. WHEN a CAddr is found in the cache, THE AsyncExecutor SHALL return the cached value without acquiring a semaphore permit or checking the InFlightMap.
2. WHEN a CAddr is found in the leaf store, THE AsyncExecutor SHALL return the leaf value without acquiring a semaphore permit or checking the InFlightMap.
3. THE AsyncExecutor SHALL check the cache before the leaf store, and both before the InFlightMap, following the resolution order: cache → leaf → dedup → compute.

### Requirement 10: Correct Materialization Order Preservation

**User Story:** As a system developer, I want parallel resolution to preserve input ordering, so that compute functions receive their inputs in the exact positional order specified by the recipe regardless of which branch completes first.

#### Acceptance Criteria

1. FOR ALL recipes with N inputs, THE AsyncExecutor SHALL guarantee that `input_bytes[i]` contains the materialized result of `recipe.inputs[i]` for all i in 0..N.
2. WHEN inputs complete in any arbitrary order during parallel resolution, THE AsyncExecutor SHALL collect results positionally (not in completion order).
3. WHEN a compute function is called, THE AsyncExecutor SHALL pass inputs as a `Vec<Bytes>` where the vector index corresponds to the recipe input index.

### Requirement 11: Performance Characteristics for Fan-In DAGs

**User Story:** As a system developer, I want parallel materialization to achieve near-linear speedup proportional to DAG width, so that wide fan-in patterns benefit from available hardware parallelism.

#### Acceptance Criteria

1. WHEN a recipe has N independent inputs each requiring time T to compute, THE AsyncExecutor SHALL complete input resolution in approximately max(T₁, T₂, ..., Tₙ) wall-clock time rather than sum(T₁ + T₂ + ... + Tₙ).
2. WHEN a DAG is a linear chain (no fan-in), THE AsyncExecutor SHALL complete in approximately sequential time with negligible overhead (less than 5 microseconds per node from parallel infrastructure).
3. WHEN the number of concurrent compute operations reaches `max_concurrency`, THE AsyncExecutor SHALL queue additional compute operations until permits become available, resulting in batched execution.

