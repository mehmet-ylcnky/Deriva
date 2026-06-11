# Implementation Plan: Async Compute Engine

## Overview

Replace the synchronous `Executor` with a Tokio-based `AsyncExecutor` that uses `Arc`-based shared ownership, `BoxFuture` for async recursion, `tokio::sync::RwLock` for interior mutability, `spawn_blocking` for CPU isolation, `try_join_all` for parallel input resolution, and broadcast-based deduplication. Implementation spans `deriva-core`, `deriva-compute`, `deriva-storage`, and `deriva-server` crates.

## Tasks

- [ ] 1. Define shared traits and types in `deriva-core`
  - [ ] 1.1 Add `RecipeStore` trait to `deriva-core`
    - Create `crates/deriva-core/src/recipe_store.rs` with the `RecipeStore` trait (`fn get(&self, addr: &CAddr) -> Result<Option<Recipe>>`) requiring `Send + Sync`
    - Export from `crates/deriva-core/src/lib.rs`
    - _Requirements: 4.1, 4.2, 4.3_

  - [ ] 1.2 Add `AsyncMaterializationCache` trait to `deriva-compute`
    - Create `crates/deriva-compute/src/async_cache.rs` with the `AsyncMaterializationCache` trait providing async `get(&self)`, `put(&self)`, and `contains(&self)` methods with `Send + Sync` bounds
    - Use `#[async_trait]` for the trait definition
    - _Requirements: 1.1, 1.6_

  - [ ] 1.3 Add `AsyncLeafStore` trait to `deriva-compute`
    - Create `crates/deriva-compute/src/async_leaf_store.rs` with the `AsyncLeafStore` trait providing async `get_leaf(&self)` with `Send + Sync` bounds
    - Implement blanket impl for any `T: LeafStore + Send + Sync`
    - _Requirements: 2.1, 2.2, 2.3_

  - [ ] 1.4 Add `DagReader` trait to `deriva-compute`
    - Create `crates/deriva-compute/src/dag_reader.rs` with synchronous `get_inputs` and `get_recipe` methods requiring `Send + Sync`
    - Implement `DagReader` for existing `DagStore` (unit test backend)
    - _Requirements: 3.1, 3.2, 3.3_

- [ ] 2. Implement SharedCache and CombinedDagReader
  - [ ] 2.1 Implement `SharedCache` struct
    - Create `crates/deriva-compute/src/shared_cache.rs` wrapping `EvictableCache` with `tokio::sync::RwLock`
    - Implement `AsyncMaterializationCache` trait: `get` acquires write lock (updates metadata), `contains` acquires read lock, `put` acquires write lock
    - Add helper methods: `entry_count`, `current_size`, `hit_rate`, `remove`
    - _Requirements: 1.2, 1.3, 1.4, 1.5_

  - [ ]* 2.2 Write property test for SharedCache round-trip integrity
    - **Property 1: Cache Round-Trip Integrity**
    - Generate random CAddr and Bytes; put then get must return exact bytes
    - **Validates: Requirements 1.3, 1.5**

  - [ ] 2.3 Implement `CombinedDagReader` struct
    - Create `crates/deriva-compute/src/combined_dag_reader.rs` combining `Arc<PersistentDag>` with `Arc<R: RecipeStore>` into a `DagReader` implementation
    - `get_inputs` delegates to PersistentDag, `get_recipe` delegates to RecipeStore
    - _Requirements: 3.4, 3.5_

  - [ ] 2.4 Implement `SledRecipeStore` in `deriva-storage`
    - Create `crates/deriva-storage/src/recipe_store.rs` implementing the `RecipeStore` trait from `deriva-core` using sled
    - _Requirements: 4.1, 4.2, 4.3_

  - [ ]* 2.5 Write property test for AsyncLeafStore blanket equivalence
    - **Property 2: AsyncLeafStore Blanket Equivalence**
    - For any type implementing `LeafStore + Send + Sync`, async `get_leaf` via blanket impl returns same result as synchronous call
    - **Validates: Requirements 2.3**

- [ ] 3. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 4. Implement AsyncExecutor core materialization
  - [ ] 4.1 Create `AsyncExecutor` struct and `ExecutorConfig`
    - Create `crates/deriva-compute/src/async_executor.rs` with `AsyncExecutor<C, L, D>` struct holding `Arc`-wrapped cache, leaf_store, dag, registry, semaphore, in_flight map, config, and verification_stats
    - Define `ExecutorConfig` with `max_concurrency`, `dedup_channel_capacity`, and `verification` fields
    - Define `VerificationMode` enum with `Off`, `DualCompute`, and `Sampled { rate }` variants
    - Define `VerificationStats` with atomic counters
    - _Requirements: 5.1, 9.2, 10.1, 10.4_

  - [ ] 4.2 Implement `materialize` method with BoxFuture recursion
    - Implement the core resolution logic: check cache → check leaf store → check in-flight map → lookup recipe → resolve inputs via `try_join_all` → acquire semaphore → `spawn_blocking` compute → cache result
    - Use `BoxFuture` for recursion to achieve O(1) stack depth
    - Ensure inputs are collected in recipe-declared order after parallel resolution
    - _Requirements: 5.2, 5.3, 5.4, 5.5, 5.6, 5.7, 5.8, 6.1, 6.2, 6.3, 7.1, 7.2, 7.3, 9.1, 9.3, 9.4, 13.1, 13.2, 13.3_

  - [ ] 4.3 Implement in-flight deduplication with broadcast channels
    - First requester inserts `(addr, broadcast::Sender)` into InFlightMap
    - Subsequent requesters subscribe to existing channel
    - Producer broadcasts result (success or error) and removes entry
    - Handle recv failures as `ComputeFailed`
    - _Requirements: 8.1, 8.2, 8.3, 8.4_

  - [ ] 4.4 Implement verification mode logic
    - When `DualCompute` or sampled-selected, execute compute function twice in parallel via `tokio::join!`
    - Compare outputs byte-for-byte; on mismatch return `DeterminismViolation` error
    - Implement deterministic sampling: `addr.as_bytes()[0] as f64 / 255.0 < rate`
    - Update `VerificationStats` atomic counters
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5_

  - [ ]* 4.5 Write property tests for materialization correctness
    - **Property 3: Materialization Priority Order** — cached value takes precedence over leaf; leaf takes precedence over DAG
    - **Property 5: Error Propagation Without Caching** — failed results are never cached; error variant matches root cause
    - **Property 6: Input Ordering Preservation** — inputs arrive in recipe-declared order regardless of resolution timing
    - **Validates: Requirements 5.3, 5.4, 5.5, 5.6, 5.7, 5.8, 7.3, 14.1, 14.4**

  - [ ]* 4.6 Write property test for deep DAG completion
    - **Property 4: Deep DAG Completion (BoxFuture Stack Safety)**
    - Generate linear chain DAGs of depth 1-200; verify materialization completes without stack overflow
    - **Validates: Requirements 5.2, 13.1, 13.2, 13.3**

  - [ ]* 4.7 Write property test for deduplication correctness
    - **Property 7: Deduplication Correctness**
    - Spawn N concurrent `materialize` calls for same CAddr; verify all get same result and compute runs at most once
    - **Validates: Requirements 8.1, 8.2, 8.3, 14.2, 14.3**

  - [ ]* 4.8 Write property test for semaphore deadlock freedom
    - **Property 8: Semaphore Deadlock Freedom**
    - Generate DAGs deeper than `max_concurrency`; verify materialization completes without deadlock
    - **Validates: Requirements 9.1, 9.4**

  - [ ]* 4.9 Write property tests for verification mode
    - **Property 9: Deterministic Verification Sampling** — same addr + rate always produces same decision
    - **Property 10: Dual-Compute Passes for Deterministic Functions** — deterministic function always passes dual-compute
    - **Validates: Requirements 10.2, 10.4, 10.5**

  - [ ]* 4.10 Write property test for parallel input error short-circuit
    - **Property 11: Parallel Input Error Short-Circuit**
    - Generate recipes where at least one input fails; verify `materialize` returns an error
    - **Validates: Requirements 7.2**

- [ ] 5. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 6. Add error types and observability
  - [ ] 6.1 Extend `DerivaError` with new variants
    - Add `DeterminismViolation { addr, function_id, output_1_hash, output_2_hash, output_1_len, output_2_len }` variant to existing error enum
    - Ensure existing `NotFound`, `FunctionNotFound`, and `ComputeFailed` variants cover all materialization failure modes
    - _Requirements: 5.6, 5.7, 5.8, 6.2, 10.3, 14.4_

  - [ ] 6.2 Add Prometheus metrics to AsyncExecutor
    - Register metrics: `deriva_materialize_total` (counter with outcome labels), `deriva_materialize_duration_seconds` (histogram), `deriva_materialize_active` (gauge), `deriva_cache_total` (counter with hit/miss), `deriva_compute_duration_seconds` (histogram by function), `deriva_compute_input_bytes` (histogram by function), `deriva_compute_output_bytes` (histogram by function)
    - Instrument `materialize` to record all metrics at appropriate points
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5_

  - [ ] 6.3 Add tracing spans to AsyncExecutor
    - Create a tracing span in `materialize` containing the target CAddr
    - Add span events for cache hit, leaf hit, recipe lookup, compute start/end
    - _Requirements: 11.6_

  - [ ]* 6.4 Write unit tests for error propagation and non-caching
    - Test `NotFound`, `FunctionNotFound`, `ComputeFailed` are returned correctly
    - Test failed results are never cached
    - Test in-flight map cleanup on error
    - Test broadcast of errors to deduplicated subscribers
    - _Requirements: 14.1, 14.2, 14.3, 14.4_

- [ ] 7. Integrate into `deriva-server`
  - [ ] 7.1 Refactor `ServerState` to use `AsyncExecutor`
    - Replace synchronous executor with `AsyncExecutor<SharedCache, BlobStore, CombinedDagReader<SledRecipeStore>>`
    - Remove outer `RwLock` wrappers from `dag` and `cache` fields — concurrency is handled internally
    - Wire `Arc<SharedCache>`, `Arc<PersistentDag>`, `Arc<SledRecipeStore>`, `Arc<FunctionRegistry>` into the executor
    - _Requirements: 5.1, 12.4_

  - [ ] 7.2 Update gRPC `Get` handler to use async materialization
    - Replace synchronous `executor.get()` call with `executor.materialize(addr).await`
    - Remove any `RwLock::write()` acquisition at the RPC handler level
    - Ensure streaming response still works with the async result
    - _Requirements: 5.3, 5.4, 5.5_

  - [ ] 7.3 Ensure backward compatibility of synchronous executor
    - Verify `Executor`, `MaterializationCache` trait, and `LeafStore` trait remain exported from `deriva-compute`
    - Add a `pub mod executor;` re-export alongside new `pub mod async_executor;`
    - _Requirements: 12.1, 12.2, 12.3, 12.4_

  - [ ]* 7.4 Write integration tests for concurrent gRPC operations
    - Test 5 concurrent gRPC Get requests return correct data
    - Test Get and Put operations don't block each other
    - Test Status RPC responds during 50 concurrent Gets
    - Test Prometheus metrics reflect actual operations
    - _Requirements: 1.1, 5.3, 8.1, 11.1_

- [ ] 8. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties defined in the design document
- Unit tests validate specific examples and edge cases
- The synchronous `Executor` is retained for backward compatibility (Requirement 12)
- All code is Rust, matching the existing workspace and design document

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2", "1.3", "1.4"] },
    { "id": 1, "tasks": ["2.1", "2.3", "2.4", "2.5"] },
    { "id": 2, "tasks": ["2.2", "4.1", "6.1"] },
    { "id": 3, "tasks": ["4.2", "4.3", "4.4"] },
    { "id": 4, "tasks": ["4.5", "4.6", "4.7", "4.8", "4.9", "4.10"] },
    { "id": 5, "tasks": ["6.2", "6.3", "6.4"] },
    { "id": 6, "tasks": ["7.1"] },
    { "id": 7, "tasks": ["7.2", "7.3"] },
    { "id": 8, "tasks": ["7.4"] }
  ]
}
```
