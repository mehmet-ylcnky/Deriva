# Requirements Document

## Introduction

Phase 2.12 of the Computation-Addressed Distributed File System (Deriva/CAS-DFS) enforces memory budget limits across concurrent streaming pipelines to prevent out-of-memory (OOM) conditions under load. The `memory_budget` field in `PipelineConfig` (currently declared but unused, defaulting to 0=unlimited) is activated with per-pipeline enforcement logic using `tokio::sync::Semaphore`-based permit accounting. A `MemoryController` wraps the semaphore and translates byte budgets into permit counts (one permit = one chunk). Before sending each data chunk, a producer acquires a permit; when the consumer finishes processing, the permit is released (via ChunkGuard drop). When the budget is exhausted, producers await permit release — applying backpressure that slows pipelines rather than failing them. A configurable global memory budget (default: 256MB) can optionally limit total in-flight bytes across all pipelines. Adaptive chunk sizing (§2.10) integration ensures the AdaptiveResizer consults available budget before growing chunks. Three Prometheus metrics provide observability into budget utilization, backpressure frequency, and configured limits.

## Glossary

- **MemoryController**: A per-pipeline struct wrapping a `tokio::sync::Semaphore` that limits total in-flight data (chunks in channels) for a single pipeline execution. Permits represent chunk-sized units of memory.
- **Global_Memory_Budget**: A process-wide configurable limit (default: 256MB) on total in-flight chunk bytes across all concurrent streaming pipelines. Enforced via a shared semaphore or token allocator.
- **Per_Pipeline_Max**: An optional per-pipeline memory budget (field `memory_budget` in PipelineConfig). When set to 0, no per-pipeline limit is enforced.
- **BudgetedSender**: A wrapper around `mpsc::Sender` that acquires a semaphore permit from the pipeline's MemoryController before sending each data chunk.
- **BudgetedReceiver**: A wrapper around `mpsc::Receiver` that returns a ChunkGuard, releasing the semaphore permit after the consumer finishes processing a chunk (on drop).
- **ChunkGuard**: A struct holding a `StreamChunk` and its associated `OwnedSemaphorePermit`. The permit is released when the guard is dropped after processing.
- **Permit**: A unit of memory accounting in the MemoryController, representing one chunk's worth of in-flight data (`chunk_size` bytes).
- **Backpressure**: The condition where producers await permit availability because the memory budget is exhausted. Pipelines slow down rather than fail.
- **StreamPipeline**: The pipeline builder and executor that constructs and runs a DAG of streaming stages (defined in §2.7).
- **PipelineConfig**: Configuration struct for pipeline execution, containing `chunk_size`, `channel_capacity`, `cache_intermediates`, and `memory_budget` fields.
- **StreamChunk**: The enum representing a unit of data in the streaming pipeline: `Data(Bytes)`, `End`, or `Error(DerivaError)`.
- **AdaptiveResizer**: A pipeline node (from §2.10) that re-chunks data to a dynamically-adjusted target size based on throughput feedback.
- **Batch_Mode**: Pipeline execution mode where functions receive complete input data and produce complete output data without streaming channels, exempt from memory budget enforcement.

## Requirements

### Requirement 1: Per-Pipeline MemoryController

**User Story:** As a system developer, I want each pipeline with a non-zero memory budget to enforce in-flight data limits using a semaphore, so that total in-flight data across all channels in a pipeline is bounded.

#### Acceptance Criteria

1. WHEN `memory_budget` is greater than 0 in PipelineConfig, THE StreamPipeline SHALL create a MemoryController with `memory_budget / chunk_size` permits (minimum 1 permit).
2. WHEN `memory_budget` is 0 in PipelineConfig, THE StreamPipeline SHALL execute without creating a MemoryController, using raw `mpsc` channels with zero overhead (identical to current behavior).
3. THE MemoryController SHALL use a `tokio::sync::Semaphore` to limit total in-flight chunks across all channels in the pipeline.
4. IF `memory_budget` is less than `chunk_size`, THEN THE MemoryController SHALL clamp permits to 1 and log a warning, ensuring the pipeline can always make progress.
5. THE MemoryController SHALL expose `available()` returning the number of currently available permits and `total_permits()` returning the configured total.

### Requirement 2: Per-Chunk Budget Accounting

**User Story:** As a system developer, I want channel wrappers that acquire budget before sending a chunk and release budget when the chunk is consumed, so that in-flight memory is tracked accurately.

#### Acceptance Criteria

1. WHEN a BudgetedSender sends a `Data` chunk, THE BudgetedSender SHALL acquire one permit from the MemoryController before placing the chunk in the channel.
2. WHEN the MemoryController has no available permits, THE BudgetedSender send operation SHALL await until a permit becomes available (backpressure).
3. WHEN a BudgetedSender sends an `End` or `Error` chunk, THE BudgetedSender SHALL send without acquiring a permit (sentinel chunks carry no data payload).
4. WHEN a BudgetedReceiver receives a chunk, THE BudgetedReceiver SHALL return a ChunkGuard that holds the associated permit until the guard is dropped.
5. WHEN a ChunkGuard is dropped after the consumer finishes processing, THE associated permit SHALL be released back to the MemoryController semaphore.
6. FOR ALL in-flight data chunks across all channels in a pipeline, THE invariant SHALL hold: each data chunk holds exactly one permit, and total held permits does not exceed `memory_budget / chunk_size`.

### Requirement 3: Backpressure Behavior

**User Story:** As a system operator, I want memory budget exhaustion to cause producers to wait (backpressure) rather than reject or error, so that pipelines slow down gracefully under memory pressure instead of failing.

#### Acceptance Criteria

1. WHEN the MemoryController semaphore has zero available permits, THE BudgetedSender SHALL block (async await) until a consumer releases a permit by dropping a ChunkGuard.
2. THE backpressure wait SHALL be non-blocking to the Tokio runtime (async-native semaphore acquire, not a mutex or spinlock).
3. WHEN a permit becomes available, THE blocked BudgetedSender SHALL resume sending without error.
4. THE system SHALL NOT return errors or reject chunks due to memory budget exhaustion; budget exhaustion causes throughput reduction only.
5. WHEN backpressure occurs at a BudgetedSender, THE pipeline SHALL continue processing all already-in-flight chunks downstream without interruption.

### Requirement 4: Global Memory Budget

**User Story:** As a system operator, I want a process-wide memory budget that limits total in-flight bytes across all concurrent streaming pipelines, so that the system does not exhaust available memory under high concurrency.

#### Acceptance Criteria

1. THE system SHALL support a configurable `global_memory_budget` limit, defaulting to 268435456 bytes (256MB).
2. WHEN the `global_memory_budget` is set to 0, THE system SHALL impose no global limit (unlimited mode, backward compatible with current behavior).
3. THE Global_Memory_Budget SHALL track total in-flight bytes across all active pipelines using a shared semaphore or token-based allocator wrapped in an Arc.
4. WHEN the global budget is exhausted, THE system SHALL apply backpressure to producers across all pipelines (producers await until consumers free memory).
5. WHEN a pipeline completes or is cancelled, THE in-flight permits held by that pipeline SHALL be released back to the global budget.

### Requirement 5: Per-Pipeline Max Configuration

**User Story:** As a system developer, I want an optional per-pipeline maximum budget, so that individual pipelines can be constrained independently of the global budget.

#### Acceptance Criteria

1. THE PipelineConfig SHALL support a `per_pipeline_max` field specifying the maximum in-flight bytes for a single pipeline (0 = unlimited, no per-pipeline MemoryController created).
2. WHEN `per_pipeline_max` is greater than 0, THE StreamPipeline SHALL create a MemoryController with `per_pipeline_max / chunk_size` permits.
3. WHEN both `global_memory_budget` and `per_pipeline_max` are configured, THE system SHALL enforce both independently: the global budget limits total in-flight bytes system-wide, and per-pipeline budget limits in-flight bytes within a single pipeline.
4. WHEN `per_pipeline_max` is not specified, THE system SHALL default to 0 (unlimited per-pipeline, relying on global budget only).

### Requirement 6: Graceful Degradation

**User Story:** As a system operator, I want pipelines to slow down rather than fail when memory pressure is high, so that the system remains available under load with reduced throughput.

#### Acceptance Criteria

1. WHEN all permits in a MemoryController are held by in-flight chunks, THE pipeline SHALL make progress at the rate of its slowest consumer (consumer-driven throughput).
2. THE pipeline SHALL NOT produce errors, panics, or timeouts due to memory budget exhaustion alone.
3. WHEN a permit is released by a consumer, THE blocked producer SHALL resume within one Tokio scheduler tick.
4. WHEN concurrent pipelines share a global budget and the budget is exhausted, THE pipelines SHALL each slow to the rate their consumers free memory, without starvation of any single pipeline.

### Requirement 7: Adaptive Chunk Sizing Integration

**User Story:** As a system developer, I want the AdaptiveResizer to respect memory budget constraints, so that chunk size growth does not cause a pipeline to exceed its memory allocation.

#### Acceptance Criteria

1. WHEN adaptive chunking is enabled and a MemoryController is present, THE AdaptiveResizer SHALL consult the MemoryController's available permits before applying a chunk size increase.
2. IF the MemoryController has fewer available permits than 25% of total permits (low-watermark), THEN THE AdaptiveResizer SHALL suppress chunk growth and retain the current chunk size.
3. IF the MemoryController has zero available permits, THEN THE AdaptiveResizer SHALL attempt to shrink the chunk size to reduce memory pressure.
4. WHEN adaptive chunking is disabled or no MemoryController is present (budget=0), THE AdaptiveResizer SHALL operate without budget-awareness (identical to standalone §2.10 behavior).

### Requirement 8: Observability Metrics

**User Story:** As a system operator, I want Prometheus metrics for memory budget utilization and backpressure frequency, so that I can monitor system health and tune budget configuration in production.

#### Acceptance Criteria

1. THE system SHALL expose a `deriva_memory_budget_bytes` gauge metric reporting the configured global memory budget limit in bytes.
2. THE system SHALL expose a `deriva_memory_budget_utilization` gauge metric reporting the ratio of currently held permits to total permits (value between 0.0 and 1.0).
3. THE system SHALL expose a `deriva_memory_budget_wait_total` counter metric that increments each time a BudgetedSender blocks waiting for a permit (each backpressure event).
4. WHEN permits are acquired or released, THE `deriva_memory_budget_utilization` metric SHALL be updated to reflect the current utilization ratio.
5. THE metrics SHALL impose negligible overhead (atomic counter increments only) and SHALL NOT require additional locks or allocations on the send/receive hot path.

### Requirement 9: Budget Release on Pipeline Completion

**User Story:** As a system developer, I want all permits to be released deterministically when a pipeline finishes, so that freed capacity becomes available to other pipelines without leaks.

#### Acceptance Criteria

1. WHEN a pipeline completes successfully, all ChunkGuards SHALL be dropped and all associated permits SHALL be released back to the MemoryController and global budget.
2. WHEN a pipeline fails with an error, all in-flight ChunkGuards SHALL be dropped (via task cancellation) and all permits SHALL be released.
3. WHEN a pipeline is cancelled (Tokio task dropped or abort signal), THE Drop implementation on channel wrappers and ChunkGuards SHALL release all held permits.
4. THE permit release SHALL occur within the same Tokio runtime tick as the task drop, preventing budget leaks under cancellation.

### Requirement 10: Backward Compatibility

**User Story:** As a system developer, I want the memory budget enforcement feature to be fully backward-compatible, so that existing pipelines with default configuration experience no behavioral or performance changes.

#### Acceptance Criteria

1. WHEN `memory_budget` is 0 in PipelineConfig (the default) and `global_memory_budget` is 0, THE StreamPipeline SHALL execute with identical behavior, performance, and code paths as pre-§2.12 implementation.
2. THE PipelineConfig default values for all pre-existing fields (`chunk_size`, `channel_capacity`, `cache_intermediates`, `memory_budget`) SHALL remain unchanged.
3. THE existing `StreamingComputeFunction` trait SHALL NOT require modifications to existing implementations for budget enforcement to function.
4. WHEN memory budget enforcement is disabled (all budgets = 0), THE system SHALL NOT create MemoryController instances, semaphores, or budget-tracking structures.

### Requirement 11: Batch Mode Exemption

**User Story:** As a system developer, I want batch execution mode to be exempt from memory budget enforcement, so that batch functions continue to allocate temporary buffers freely without permit overhead.

#### Acceptance Criteria

1. WHEN a pipeline contains only BatchStage nodes, THE StreamPipeline SHALL NOT create a MemoryController or acquire permits from the global budget.
2. WHEN a pipeline contains mixed BatchStage and StreamingStage nodes, THE StreamPipeline SHALL calculate permit requirements based only on the StreamingStage count (excluding BatchStage nodes).
3. THE BatchStage execution path SHALL NOT use BudgetedSender or BudgetedReceiver wrappers.
