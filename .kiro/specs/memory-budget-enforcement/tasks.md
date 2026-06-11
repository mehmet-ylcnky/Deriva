# Implementation Plan: Memory Budget Enforcement

## Overview

Implement semaphore-based memory budget enforcement for streaming pipelines in the `deriva-compute` crate. This adds a `MemoryController` (per-pipeline) and `GlobalMemoryController` (system-wide) that limit in-flight chunks using `tokio::sync::Semaphore` permits. Channel wrappers (`BudgetedSender`/`BudgetedReceiver`) enforce acquire-before-send semantics, and `ChunkGuard` releases permits on drop. The feature is zero-overhead when disabled (budget=0) and integrates with adaptive chunk sizing to suppress growth under memory pressure.

## Tasks

- [ ] 1. Create MemoryController and core budget types
  - [ ] 1.1 Create `memory_budget.rs` module in `crates/deriva-compute/src/` with `MemoryController` struct
    - Implement `MemoryController::new(budget: usize, chunk_size: usize)` computing permits as `budget / chunk_size` (minimum 1)
    - Implement `acquire()` returning `OwnedSemaphorePermit` (async, blocks when exhausted)
    - Implement `try_acquire()` returning `Option<OwnedSemaphorePermit>`
    - Implement `available()` and `total_permits()` accessors
    - Clamp permits to 1 when budget < chunk_size, log a warning via `tracing::warn!`
    - _Requirements: 1.1, 1.3, 1.4, 1.5_

  - [ ] 1.2 Implement `ChunkGuard` struct in `memory_budget.rs`
    - Define `ChunkGuard { pub chunk: StreamChunk, _permit: Option<OwnedSemaphorePermit> }`
    - Permit released automatically via `Drop` on `OwnedSemaphorePermit` inside the guard
    - _Requirements: 2.4, 2.5_

  - [ ] 1.3 Implement `BudgetedSender` and `BudgetedReceiver` in `memory_budget.rs`
    - `BudgetedSender` wraps `mpsc::Sender<(StreamChunk, Option<OwnedSemaphorePermit>)>` and holds a `MemoryController`
    - `send()` acquires a permit for `Data` chunks, sends `End`/`Error` without a permit (sentinel bypass)
    - `BudgetedReceiver` wraps `mpsc::Receiver` and returns `ChunkGuard` from `recv()`
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 3.1, 3.2, 3.3, 3.4, 3.5_

  - [ ]* 1.4 Write property test for MemoryController in-flight invariant
    - **Property 1: In-flight invariant**
    - Generate random `budget` (1..10MB) and `chunk_size` (1KB..256KB), spawn N producers, verify peak in-flight never exceeds permits
    - **Validates: Requirements 1.1, 1.3, 2.6**

  - [ ]* 1.5 Write property test for sentinel bypass
    - **Property 3: Sentinel bypass**
    - Generate random End/Error chunks, send via BudgetedSender when semaphore has 0 permits, verify all succeed
    - **Validates: Requirements 2.3**

  - [ ]* 1.6 Write property test for ChunkGuard release on drop
    - **Property 4: ChunkGuard release on drop**
    - Acquire N permits, create ChunkGuards, drop them in random order, verify available() increases correctly
    - **Validates: Requirements 2.5, 9.1, 9.2, 9.3**

- [ ] 2. Implement GlobalMemoryController and PipelineConfig extension
  - [ ] 2.1 Implement `GlobalMemoryController` in `memory_budget.rs`
    - Wraps `Arc<Semaphore>` with `global_budget / chunk_size` permits
    - Implement `acquire()`, `available()`, `total_permits()`
    - When `global_memory_budget = 0`, no controller is created (None)
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [ ] 2.2 Extend `PipelineConfig` in `crates/deriva-compute/src/pipeline.rs`
    - Rename/alias `memory_budget` field to `per_pipeline_max` (keep backward compat with default 0)
    - Ensure `Default` impl sets `per_pipeline_max: 0`
    - _Requirements: 5.1, 5.2, 5.4, 10.2_

  - [ ] 2.3 Add `GlobalMemoryController` to `ServerState` in `crates/deriva-server/src/state.rs`
    - Store as `Arc<Option<GlobalMemoryController>>` in `ServerState`
    - Initialize from server configuration (default 256MB), `None` if set to 0
    - _Requirements: 4.1, 4.2_

  - [ ]* 2.4 Write property test for global budget bounds
    - **Property 6: Global budget bounds total system memory**
    - Spawn N concurrent pipelines sharing a GlobalMemoryController with G permits, verify total in-flight across all ≤ G
    - **Validates: Requirements 4.1, 4.3, 4.4**

- [ ] 3. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 4. Integrate budget enforcement into StreamPipeline execution
  - [ ] 4.1 Modify `StreamPipeline::execute()` in `crates/deriva-compute/src/pipeline.rs`
    - Accept optional `&GlobalMemoryController` parameter
    - When `per_pipeline_max > 0`: create per-pipeline `MemoryController`, use `BudgetedSender`/`BudgetedReceiver` for Source/Cached/StreamingStage nodes
    - When `per_pipeline_max = 0` and no global controller: use raw `mpsc` channels (zero overhead, identical to current behavior)
    - BatchStage nodes remain exempt from budget enforcement
    - _Requirements: 1.1, 1.2, 5.2, 5.3, 10.1, 10.4, 11.1, 11.2, 11.3_

  - [ ] 4.2 Update `StreamingExecutor::materialize_streaming()` in `crates/deriva-compute/src/streaming_executor.rs`
    - Thread global controller reference through to `StreamPipeline::execute()`
    - _Requirements: 4.3, 5.3_

  - [ ] 4.3 Update `DerivaService` in `crates/deriva-server/src/service.rs` to pass `GlobalMemoryController` to executor
    - Pass the global controller from `ServerState` to materialization calls
    - _Requirements: 4.1, 4.3_

  - [ ]* 4.4 Write property test for zero-overhead when disabled
    - **Property 5: Zero-overhead when disabled**
    - Execute pipeline with per_pipeline_max=0 and global_budget=0, verify no MemoryController/Semaphore/BudgetedSender created
    - **Validates: Requirements 1.2, 10.1, 10.4**

  - [ ]* 4.5 Write property test for batch mode exemption
    - **Property 9: Batch mode exemption**
    - Construct pipeline with only BatchStage nodes, verify no MemoryController created and no permits acquired
    - **Validates: Requirements 11.1, 11.3**

  - [ ]* 4.6 Write property test for backpressure without error
    - **Property 2: Backpressure without error**
    - Saturate semaphore, attempt send from BudgetedSender, verify it blocks (does not error/panic), then resumes after permit release
    - **Validates: Requirements 3.1, 3.3, 3.4, 6.2**

- [ ] 5. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 6. Implement budget release on pipeline completion and progress guarantee
  - [ ] 6.1 Ensure permit release on pipeline completion, error, and cancellation in `StreamPipeline::execute()`
    - All spawned tasks hold ChunkGuards; when tasks complete/abort, guards are dropped, permits released
    - Verify that Tokio task cancellation propagates through to ChunkGuard drop
    - _Requirements: 9.1, 9.2, 9.3, 9.4_

  - [ ] 6.2 Ensure progress guarantee with minimum 1 permit
    - Validate that `MemoryController::new` always produces at least 1 permit (already clamped in 1.1)
    - Verify pipeline can make forward progress with exactly 1 permit (consumer-driven throughput)
    - _Requirements: 1.4, 6.1, 6.3_

  - [ ]* 6.3 Write property test for pipeline completion releases all permits
    - **Property 7: Pipeline completion releases all permits**
    - Run pipelines to success/error/cancellation, verify all permits freed (available == total)
    - **Validates: Requirements 9.1, 9.2, 9.3, 9.4**

  - [ ]* 6.4 Write property test for progress guarantee
    - **Property 10: Progress guarantee**
    - Run pipeline with 1 permit, verify it completes within a timeout (no deadlock)
    - **Validates: Requirements 1.4, 6.1, 6.3**

- [ ] 7. Integrate adaptive chunk sizing with memory budget
  - [ ] 7.1 Modify `StreamingChunkResizer` in `crates/deriva-compute/src/builtins_streaming/core.rs`
    - Accept optional `MemoryController` reference via params or context
    - When controller present and available permits < 25% of total: suppress chunk growth
    - When controller present and available permits = 0: shrink chunk size
    - When no controller present: operate as standalone §2.10 (no behavioral change)
    - _Requirements: 7.1, 7.2, 7.3, 7.4_

  - [ ]* 7.2 Write property test for adaptive sizing respects budget
    - **Property 8: Adaptive sizing respects budget**
    - Set MemoryController with low available permits (<25%), request growth, verify growth suppressed
    - **Validates: Requirements 7.1, 7.2**

- [ ] 8. Add observability metrics for memory budget
  - [ ] 8.1 Add memory budget metrics to `crates/deriva-compute/src/metrics.rs`
    - Register `deriva_memory_budget_bytes` gauge (configured global budget limit)
    - Register `deriva_memory_budget_utilization` gauge (held permits / total permits, 0.0–1.0)
    - Register `deriva_memory_budget_wait_total` counter (backpressure events)
    - _Requirements: 8.1, 8.2, 8.3, 8.5_

  - [ ] 8.2 Instrument `BudgetedSender` and `GlobalMemoryController` with metric updates
    - Increment `wait_total` counter when `acquire()` would block (use `try_acquire` check before async acquire)
    - Update `utilization` gauge on acquire and release
    - Set `budget_bytes` gauge on GlobalMemoryController creation
    - Ensure metric updates use atomic operations only (no additional locks)
    - _Requirements: 8.3, 8.4, 8.5_

  - [ ]* 8.3 Write unit tests for metrics
    - Verify gauge/counter values update correctly during send/receive cycles
    - Verify backpressure counter increments when budget is exhausted
    - _Requirements: 8.1, 8.2, 8.3, 8.4_

- [ ] 9. Wire module into crate and update exports
  - [ ] 9.1 Register `memory_budget` module in `crates/deriva-compute/src/lib.rs`
    - Add `pub mod memory_budget;`
    - Re-export `MemoryController`, `GlobalMemoryController`, `BudgetedSender`, `BudgetedReceiver`, `ChunkGuard`
    - _Requirements: 10.3_

  - [ ] 9.2 Ensure `StreamingComputeFunction` trait requires no modifications
    - Verify existing streaming function implementations compile without changes
    - _Requirements: 10.3_

- [ ] 10. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The implementation language is Rust, matching the existing codebase and design document
- The `memory_budget` field already exists in `PipelineConfig` with default 0; this feature activates it

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "2.2"] },
    { "id": 1, "tasks": ["1.2", "2.1", "2.3"] },
    { "id": 2, "tasks": ["1.3", "8.1"] },
    { "id": 3, "tasks": ["1.4", "1.5", "1.6", "2.4"] },
    { "id": 4, "tasks": ["4.1"] },
    { "id": 5, "tasks": ["4.2", "4.3", "4.4", "4.5", "4.6"] },
    { "id": 6, "tasks": ["6.1", "6.2"] },
    { "id": 7, "tasks": ["6.3", "6.4", "7.1"] },
    { "id": 8, "tasks": ["7.2", "8.2"] },
    { "id": 9, "tasks": ["8.3", "9.1", "9.2"] }
  ]
}
```
