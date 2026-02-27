# §2.12 Memory Budget Enforcement

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 2 days

---

## 1. Problem Statement

### 1.1 Current Limitation

TODO — `PipelineConfig::memory_budget` field exists but is not enforced; a pipeline with many stages can exceed system memory even with bounded channels

### 1.2 Worst-Case Memory Analysis

TODO — quantify: N stages × capacity × chunk_size; e.g., 50 stages × 8 × 64 KB = 25 MB per pipeline; multiple concurrent pipelines compound the problem

### 1.3 Comparison

TODO — table: current (per-channel bounds only) vs global budget enforcement

---

## 2. Design

### 2.1 Architecture Overview

TODO — global `MemoryController` backed by `tokio::sync::Semaphore`; permits = `memory_budget / chunk_size`; each send acquires a permit, each recv releases one

### 2.2 Permit Lifecycle

TODO — producer acquires permit before `channel.send()`; consumer releases permit after `channel.recv()` and processing; total in-flight data bounded across entire pipeline

### 2.3 Interaction with Per-Channel Backpressure

TODO — two layers: per-channel capacity (local backpressure) + global semaphore (system-wide backpressure); global limit is the tighter constraint

### 2.4 Budget Calculation

TODO — `effective_permits = memory_budget / chunk_size`; if budget is 0 (unlimited), skip semaphore entirely

### 2.5 Concurrent Pipeline Isolation

TODO — options: shared global semaphore (all pipelines compete) vs per-pipeline semaphore (isolated budgets); recommend per-pipeline for predictability

---

## 3. Implementation

### 3.1 MemoryController Struct

TODO — wraps `Arc<Semaphore>`; `acquire()` → `OwnedSemaphorePermit`; `new(budget: usize, chunk_size: usize) -> Self`

### 3.2 Permit-Aware Channel Wrapper

TODO — `BudgetedSender<T>` / `BudgetedReceiver<T>` wrapping `mpsc::Sender/Receiver` + `Arc<MemoryController>`; send acquires permit, recv releases

### 3.3 Pipeline Integration

TODO — `StreamPipeline::execute()` creates `MemoryController` from config; passes to all stage spawns; stages use `BudgetedSender/Receiver` instead of raw channels

### 3.4 Graceful Degradation

TODO — when budget is too small for even 1 permit per stage, clamp to minimum 1 permit per channel; log warning

### 3.5 PipelineConfig Validation

TODO — validate `memory_budget` at pipeline construction; warn if budget < stages × chunk_size

---

## 4. Data Flow Diagrams

### 4.1 Normal Operation (Within Budget)

TODO — permits flowing: acquire on send, release on recv; semaphore never exhausted

### 4.2 Budget Exhaustion

TODO — all permits held; producer blocks on `semaphore.acquire()`; consumer processes chunk → releases permit → producer unblocks

### 4.3 Multiple Pipelines with Shared Budget

TODO — two pipelines competing for global permits

---

## 5. Test Specification

### 5.1 Unit Tests — MemoryController

TODO — create with budget, acquire/release permits, verify count

### 5.2 Unit Tests — BudgetedSender/Receiver

TODO — send acquires permit, recv releases; verify permit count after operations

### 5.3 Unit Tests — Budget Exhaustion Blocking

TODO — exhaust all permits, verify next send blocks until recv releases

### 5.4 Integration Tests — Pipeline Under Budget

TODO — pipeline with tight budget completes correctly; verify peak memory stays within budget

### 5.5 Integration Tests — Budget = 0 (Unlimited)

TODO — no semaphore overhead; behaves identically to current implementation

### 5.6 Benchmark — Overhead of Permit Acquisition

TODO — measure per-chunk overhead of semaphore acquire/release vs raw channel

---

## 6. Edge Cases & Error Handling

TODO — table: budget=0 (unlimited), budget < chunk_size, budget < stages × chunk_size, pipeline cancellation with held permits, permit leak on panic, concurrent pipelines exhausting shared budget

---

## 7. Performance Analysis

### 7.1 Overhead of Semaphore

TODO — `Semaphore::acquire` is ~20 ns uncontended; adds ~40 ns per chunk (acquire + release); negligible vs channel overhead (~100 ns)

### 7.2 Contention Under Pressure

TODO — when budget is tight, semaphore becomes the bottleneck; pipeline throughput degrades gracefully (slower, not failing)

### 7.3 Memory Savings

TODO — quantify: without enforcement, 10 concurrent 50-stage pipelines = 250 MB; with 10 MB budget per pipeline = 100 MB total

---

## 8. Files Changed

TODO — table of affected files

---

## 9. Dependency Changes

TODO — none (`tokio::sync::Semaphore` already available)

---

## 10. Design Rationale

### 10.1 Why Semaphore Instead of Custom Allocator?

TODO — semaphore is simple, well-tested, async-native; custom allocator is complex and error-prone

### 10.2 Why Per-Pipeline Instead of Global?

TODO — per-pipeline gives predictable behavior; global creates unpredictable interference between unrelated pipelines

### 10.3 Why Advisory Instead of Hard Limit?

TODO — in-flight chunks being processed by stage tasks are not tracked by semaphore; actual memory may slightly exceed budget; hard limits would require tracking every allocation

---

## 11. Observability Integration

TODO — `deriva_memory_budget_bytes` gauge, `deriva_memory_permits_available` gauge, `deriva_memory_budget_waits_total` counter, `deriva_memory_budget_exhaustion_seconds` histogram

---

## 12. Checklist

- [ ] Implement `MemoryController` with semaphore
- [ ] Implement `BudgetedSender` / `BudgetedReceiver` wrappers
- [ ] Integrate into `StreamPipeline::execute()`
- [ ] Add budget validation at pipeline construction
- [ ] Handle budget=0 (unlimited) bypass
- [ ] Handle budget too small (clamp + warn)
- [ ] Unit tests for MemoryController, BudgetedSender/Receiver
- [ ] Unit test for budget exhaustion blocking
- [ ] Integration test: pipeline under tight budget
- [ ] Integration test: budget=0 unchanged behavior
- [ ] Benchmark: permit overhead per chunk
- [ ] Add observability metrics
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing tests still pass
- [ ] Commit and push
