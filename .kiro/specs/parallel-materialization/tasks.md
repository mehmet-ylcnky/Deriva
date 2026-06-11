# Implementation Plan: Parallel Materialization

## Overview

Phase 2.3 replaces sequential input resolution with parallel fan-out using `futures::future::try_join_all`. The core parallel implementation already exists in `crates/deriva-compute/src/async_executor.rs` with semaphore-bounded concurrency, in-flight deduplication, and scoped permit acquisition. This plan covers verifying the implementation, adding proptest as a dependency, writing property-based tests for all 11 correctness properties, and writing unit/integration tests for edge cases and performance validation.

## Tasks

- [ ] 1. Add proptest dependency and set up test infrastructure
  - [ ] 1.1 Add proptest as a dev-dependency to deriva-compute
    - Add `proptest = { workspace = true }` to `[dev-dependencies]` in `crates/deriva-compute/Cargo.toml`
    - Add `tokio-test = "0.4"` if not already present for async test utilities
    - Verify the workspace Cargo.toml already has `proptest = "1"` in workspace dependencies
    - _Requirements: N/A (infrastructure)_

  - [ ] 1.2 Create property test module structure
    - Create `crates/deriva-compute/tests/async_exec/properties.rs` module file
    - Add `mod properties;` to `crates/deriva-compute/tests/async_executor.rs`
    - Set up shared test helpers: `MockDag`, `MockCache`, `MockLeafStore`, `SlowFunction`, `FailingFunction`
    - Define proptest generators for random DAG shapes (linear chains, fan-in, diamond, binary tree)
    - _Requirements: N/A (infrastructure)_

- [ ] 2. Verify and complete core parallel implementation
  - [ ] 2.1 Verify try_join_all parallel input resolution
    - Confirm `materialize()` in `crates/deriva-compute/src/async_executor.rs` uses `try_join_all` for parallel input resolution (step 6)
    - Confirm the output order matches input order (positional preservation)
    - Confirm zero-input recipes pass an empty vector to compute function
    - Confirm single-input recipes use the same try_join_all path
    - _Requirements: 1.1, 1.2, 1.3, 1.4_

  - [ ] 2.2 Verify error short-circuiting and sibling cancellation
    - Confirm `try_join_all` error handling broadcasts the error and removes from InFlightMap
    - Confirm semaphore permits are released on cancellation via Drop
    - Confirm errors propagate without additional wrapping layers through nested parallel resolutions
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

  - [ ] 2.3 Verify semaphore-based concurrency control
    - Confirm semaphore is created with `config.max_concurrency` permits
    - Confirm semaphore is stored in `Arc` and shared across clones
    - Confirm `ComputeFailed("semaphore error: ...")` is returned when semaphore is closed
    - _Requirements: 3.1, 3.2, 3.3, 3.4_

  - [ ] 2.4 Verify deadlock prevention via scoped permit acquisition
    - Confirm permit is acquired only after try_join_all completes (step 7, after step 6)
    - Confirm no permit is held during recursive input resolution
    - Confirm permit is released after compute completes (via scope drop)
    - _Requirements: 4.1, 4.2, 4.3, 4.4_

  - [ ] 2.5 Verify in-flight request deduplication
    - Confirm InFlightMap check occurs after cache/leaf and subscribes to existing broadcast channel
    - Confirm new producer registers broadcast::Sender before computing
    - Confirm successful result is broadcast and entry removed
    - Confirm error result is broadcast and entry removed
    - Confirm RecvError maps to `ComputeFailed("producer task failed or was cancelled")`
    - Confirm InFlightMap lock is released before awaiting broadcast channel
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6_

  - [ ] 2.6 Verify ExecutorConfig and constructor API
    - Confirm `ExecutorConfig` has `max_concurrency` (default: `num_cpus::get() * 2`) and `dedup_channel_capacity` (default: 16)
    - Confirm `with_config` constructor initializes semaphore from config
    - Confirm `new` constructor uses `ExecutorConfig::default()`
    - _Requirements: 6.1, 6.2, 6.3, 6.4_

  - [ ] 2.7 Verify Clone implementation and shared state
    - Confirm all internal fields are `Arc`-wrapped
    - Confirm `Clone` implementation shares cache, semaphore, and InFlightMap
    - _Requirements: 7.1, 7.2, 7.3, 7.4_

  - [ ] 2.8 Verify cache and leaf bypass without permit
    - Confirm cache hit returns immediately without acquiring semaphore or checking InFlightMap
    - Confirm leaf hit returns immediately without acquiring semaphore or checking InFlightMap
    - Confirm resolution order is: cache → leaf → dedup → compute
    - _Requirements: 9.1, 9.2, 9.3_

- [ ] 3. Checkpoint - Verify implementation is correct
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 4. Write property-based tests for parallel correctness
  - [ ] 4.1 Write property test for positional order preservation
    - **Property 1: Positional Order Preservation**
    - Generate random recipes with N distinct leaf inputs (N in 1..20)
    - Verify `output[i] == materialize(recipe.inputs[i])` for all i
    - Use unique leaf values to distinguish positions
    - **Validates: Requirements 1.2, 10.1, 10.2, 10.3**

  - [ ] 4.2 Write property test for parallel speedup
    - **Property 2: Parallel Speedup Proportional to Width**
    - Generate random fan-in DAGs with N inputs (N in 2..8) each using `SlowFunction` with configurable sleep
    - Verify wall-clock time ≈ max(Tᵢ) rather than sum(Tᵢ), within tolerance
    - Account for semaphore batching when N > max_concurrency
    - **Validates: Requirements 1.1, 11.1**

  - [ ] 4.3 Write property test for global semaphore bounding
    - **Property 3: Global Semaphore Bounding**
    - Generate N concurrent materialization requests (N > max_concurrency)
    - Use `AtomicUsize` counter to track peak concurrent compute executions
    - Verify peak never exceeds `max_concurrency`
    - **Validates: Requirements 3.1, 3.4, 7.3, 11.3**

  - [ ] 4.4 Write property test for deadlock freedom
    - **Property 4: Deadlock Freedom for All DAG Shapes**
    - Generate random DAGs with depth D and fan-out F where D × F > max_concurrency
    - Run materialization with a timeout (e.g., 5 seconds)
    - Verify materialization completes without hanging
    - **Validates: Requirements 4.1, 4.2, 4.4**

  - [ ] 4.5 Write property test for computation deduplication
    - **Property 5: Computation Deduplication**
    - Generate a shared CAddr requested concurrently by N tasks (N in 2..10)
    - Use `AtomicUsize` counter to track how many times compute function executes
    - Verify compute executes exactly once and all requesters get the same result
    - **Validates: Requirements 5.1, 7.4**

  - [ ] 4.6 Write property test for error broadcast to all subscribers
    - **Property 6: Error Broadcast to All Subscribers**
    - Generate a failing CAddr with N concurrent subscribers (N in 2..10)
    - Verify all N subscribers receive the same error variant
    - Verify no subscriber hangs indefinitely (timeout-based)
    - **Validates: Requirements 2.1, 5.4, 8.2**

  - [ ] 4.7 Write property test for error short-circuit cancellation
    - **Property 7: Error Short-Circuit Cancellation**
    - Generate recipes with one failing input among N inputs (N in 2..8)
    - Use `SlowFunction` for non-failing inputs with long sleep
    - Verify wall-clock time ≈ time-to-first-failure rather than sum of all inputs
    - Verify no compute function is invoked for the recipe
    - **Validates: Requirements 2.1, 2.4**

  - [ ] 4.8 Write property test for cache and leaf bypass without permit
    - **Property 8: Cache and Leaf Bypass Without Permit**
    - Saturate semaphore (acquire all permits externally)
    - Materialize CAddrs present in cache or leaf store
    - Verify materialization succeeds despite zero available permits
    - **Validates: Requirements 9.1, 9.2**

  - [ ] 4.9 Write property test for clone shared state consistency
    - **Property 9: Clone Shared State Consistency**
    - Clone executor, materialize via one clone
    - Verify the other clone observes a cache hit for same CAddr
    - **Validates: Requirements 7.2**

  - [ ] 4.10 Write property test for linear chain negligible overhead
    - **Property 10: Linear Chain Negligible Overhead**
    - Generate linear chain DAGs of varying depth D (D in 2..20)
    - Verify total wall-clock time is within D × T + D × 5μs of expected sequential time
    - **Validates: Requirements 11.2**

  - [ ] 4.11 Write property test for error propagation without wrapping
    - **Property 11: Error Propagation Without Wrapping**
    - Generate errors at various DAG depths (depth in 1..10)
    - Verify the error received at top level is the original `DerivaError` variant
    - Verify no `ComputeFailed(ComputeFailed(...))` nesting occurs
    - **Validates: Requirements 2.4**

- [ ] 5. Checkpoint - Property tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 6. Write unit tests for edge cases and configuration
  - [ ] 6.1 Write unit tests for ExecutorConfig defaults and custom values
    - Test `ExecutorConfig::default()` produces `max_concurrency = num_cpus::get() * 2` and `dedup_channel_capacity = 16`
    - Test `with_config` correctly initializes semaphore with custom `max_concurrency`
    - _Requirements: 6.1, 6.2, 6.3, 6.4_

  - [ ] 6.2 Write unit tests for error scenarios
    - Test semaphore closed returns `ComputeFailed("semaphore error: ...")`
    - Test broadcast dropped returns `ComputeFailed("producer task failed or was cancelled")`
    - Test recipe not found returns `NotFound(addr)`
    - Test function not found returns `FunctionNotFound(id)`
    - _Requirements: 3.3, 5.5_

  - [ ] 6.3 Write unit tests for resolution order and special cases
    - Test resolution order: cache checked before leaf, both before InFlightMap
    - Test zero-inputs recipe calls compute with empty vec
    - Test single-input recipe works correctly
    - Test same CAddr appearing twice in recipe.inputs is deduplicated
    - _Requirements: 9.3, 1.3, 1.4_

  - [ ]* 6.4 Write unit test for DerivaError Clone
    - Verify `DerivaError` implements `Clone` without additional heap allocations beyond String fields
    - Clone various error variants and verify equality
    - _Requirements: 8.1, 8.3_

- [ ] 7. Write integration tests for end-to-end parallel resolution
  - [ ]* 7.1 Write integration test for parallel fan-in via gRPC
    - Test a wide fan-in DAG (N leaves → merge) resolves correctly through the full compute pipeline
    - Verify results are correct and timing shows parallelism
    - _Requirements: 1.1, 1.2, 11.1_

  - [ ]* 7.2 Write integration test for multiple concurrent clients
    - Spawn multiple cloned executor instances materializing concurrently
    - Verify no deadlock occurs and semaphore is shared correctly
    - _Requirements: 7.3, 7.4, 4.4_

  - [ ]* 7.3 Write stress test for 100 concurrent materializations
    - Launch 100 concurrent materialization requests on a complex DAG
    - Verify resource stability (no OOM, no deadlock, all complete)
    - _Requirements: 3.1, 11.3_

- [ ] 8. Final checkpoint - All tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- The core parallel implementation already exists in `crates/deriva-compute/src/async_executor.rs` — task group 2 verifies correctness rather than writing from scratch
- Property tests use `proptest` crate with minimum 100 iterations per property
- Property test tag format: `Feature: parallel-materialization, Property {N}: {property_text}`
- The `SlowFunction` test helper uses `std::thread::sleep` inside `spawn_blocking` to simulate compute time
- All property tests use in-memory mock stores (no external services)
- `DerivaError` already derives `Clone` in `crates/deriva-core/src/error.rs`
- `proptest` needs to be added to `crates/deriva-compute/Cargo.toml` dev-dependencies

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["1.2", "2.1", "2.2", "2.3", "2.4", "2.5", "2.6", "2.7", "2.8"] },
    { "id": 2, "tasks": ["4.1", "4.2", "4.3", "4.4", "4.5", "4.6", "4.7", "4.8", "4.9", "4.10", "4.11"] },
    { "id": 3, "tasks": ["6.1", "6.2", "6.3", "6.4"] },
    { "id": 4, "tasks": ["7.1", "7.2", "7.3"] }
  ]
}
```
