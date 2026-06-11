# Implementation Plan: Verification Mode

## Overview

This plan implements Phase 2.4 — optional dual-compute verification for the Deriva executor. The core implementation (VerificationMode enum, VerificationStats, execute_verified method, DeterminismViolation error, Status/Verify RPCs, CLI parsing) already exists in the codebase. Tasks focus on completing any remaining gaps, adding property-based tests for all 10 correctness properties, and ensuring full integration test coverage.

## Tasks

- [ ] 1. Validate and harden core verification infrastructure
  - [ ] 1.1 Validate VerificationMode enum, ExecutorConfig, and VerificationStats in `crates/deriva-compute/src/async_executor.rs`
    - Ensure `VerificationMode` derives `Debug, Clone, Copy, PartialEq, Default` with `Off` as default
    - Confirm `ExecutorConfig` includes `verification: VerificationMode` field defaulting to `Off`
    - Verify `VerificationStats` uses `AtomicU64` with `Ordering::Relaxed` for counters and `tokio::sync::Mutex` for `last_failure`
    - Validate `failure_rate()` returns 0.0 when `total_verified == 0`
    - _Requirements: 1.1, 1.2, 6.1, 6.2, 6.3, 6.5_

  - [ ] 1.2 Validate DeterminismViolation error variant in `crates/deriva-core/src/error.rs`
    - Confirm the error variant includes `addr`, `function_id`, `output_1_hash`, `output_2_hash`, `output_1_len`, `output_2_len`
    - Confirm the `Display` format matches: `determinism violation for {addr}: function {function_id} produced different outputs ({output_1_len} bytes hash={output_1_hash} vs {output_2_len} bytes hash={output_2_hash})`
    - Ensure `DerivaError` derives `Clone` for `last_failure` storage
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [ ] 1.3 Validate `execute_verified()` method in `crates/deriva-compute/src/async_executor.rs`
    - Confirm Off mode: single `spawn_blocking` call, no stats update
    - Confirm DualCompute mode: `tokio::join!` with two `spawn_blocking` calls
    - Confirm Sampled mode: `(addr.as_bytes()[0] as f64) / 255.0 < rate` decision
    - Confirm byte-for-byte comparison (`output1 == output2`) on hot path
    - Confirm blake3 hashes computed only on mismatch for error reporting
    - _Requirements: 2.1, 2.2, 2.3, 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 4.1, 4.2, 4.3, 4.4, 10.1, 10.2_

  - [ ] 1.4 Validate CLI parsing in `crates/deriva-server/src/main.rs`
    - Confirm `parse_verification()` handles "off", "dual", "sampled:RATE"
    - Confirm invalid strings produce descriptive error and server fails to start
    - Confirm rate validation rejects values outside `0.0..=1.0`
    - _Requirements: 1.3, 1.4, 1.5, 1.6_

- [ ] 2. Validate verification scope and RPC implementations
  - [ ] 2.1 Validate verification scope in `materialize()` method
    - Confirm cache hits return without calling `execute_verified()`
    - Confirm leaf nodes return without calling `execute_verified()`
    - Confirm only the `func.execute()` step within `execute_verified()` is dual-computed
    - Confirm recursive input materialization verifies each compute node independently
    - _Requirements: 7.1, 7.2, 7.3, 7.4_

  - [ ] 2.2 Validate Status RPC in `crates/deriva-server/src/service.rs`
    - Confirm `StatusResponse` includes `verification_mode`, `verification_total`, `verification_passed`, `verification_failed`, `verification_failure_rate`
    - Confirm mode string formatting: "off", "dual", "sampled:RATE"
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5_

  - [ ] 2.3 Validate Verify RPC in `crates/deriva-server/src/service.rs`
    - Confirm Verify RPC performs dual-compute regardless of configured mode
    - Confirm success response: `deterministic: true`, output_hash, output_size, compute_time_us
    - Confirm failure response: `deterministic: false`, error string
    - Confirm NOT_FOUND status for nonexistent address
    - Confirm INTERNAL error for input resolution failures
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5_

- [ ] 3. Checkpoint - Validate existing implementation
  - Ensure all existing unit tests pass (`cargo test -p deriva-compute --test verification`), ask the user if questions arise.

- [ ] 4. Property-based tests for execution mode correctness
  - [ ]* 4.1 Write property test for Off mode single execution
    - **Property 1: Off mode executes exactly once**
    - Generate random recipes with `CountingIdentity`, verify invocation_count == 1 in Off mode
    - Use `proptest` with min 100 iterations
    - File: `crates/deriva-compute/tests/verif/properties.rs`
    - **Validates: Requirements 2.1, 2.2**

  - [ ]* 4.2 Write property test for DualCompute mode double execution
    - **Property 2: DualCompute mode executes exactly twice**
    - Generate random recipes with `CountingIdentity`, verify invocation_count == 2 in DualCompute mode
    - Use `proptest` with min 100 iterations
    - File: `crates/deriva-compute/tests/verif/properties.rs`
    - **Validates: Requirements 3.1, 3.2**

  - [ ]* 4.3 Write property test for deterministic functions passing verification
    - **Property 3: Deterministic functions pass verification**
    - Generate random input bytes, verify dual-compute returns Ok with correct output
    - Use `proptest` with min 100 iterations
    - File: `crates/deriva-compute/tests/verif/properties.rs`
    - **Validates: Requirements 3.3, 3.4**

  - [ ]* 4.4 Write property test for non-deterministic functions producing DeterminismViolation
    - **Property 4: Non-deterministic functions produce DeterminismViolation**
    - Generate functions with varying non-determinism, verify DeterminismViolation error with correct fields
    - File: `crates/deriva-compute/tests/verif/properties.rs`
    - **Validates: Requirements 3.5, 3.6, 5.1, 5.2, 5.3, 5.4**

- [ ] 5. Property-based tests for sampling and statistics
  - [ ]* 5.1 Write property test for deterministic sampling decision
    - **Property 5: Sampling decision is deterministic per address**
    - Generate random CAddrs and rates, verify same decision across multiple evaluations
    - Use `proptest` with min 100 iterations
    - File: `crates/deriva-compute/tests/verif/properties.rs`
    - **Validates: Requirements 4.1, 4.2**

  - [ ]* 5.2 Write property test for sampling rate boundary conditions
    - **Property 6: Sampling rate boundary conditions**
    - Generate random CAddrs, verify rate=0.0 never verifies and rate=1.0 always verifies
    - Use `proptest` with min 100 iterations
    - File: `crates/deriva-compute/tests/verif/properties.rs`
    - **Validates: Requirements 4.5, 4.6**

  - [ ]* 5.3 Write property test for verification stats consistency
    - **Property 7: Verification stats consistency**
    - Generate random pass/fail sequences, verify total_verified == total_passed + total_failed and failure_rate correctness
    - Use `proptest` with min 100 iterations
    - File: `crates/deriva-compute/tests/verif/properties.rs`
    - **Validates: Requirements 6.1, 6.2, 6.3, 6.5**

  - [ ]* 5.4 Write property test for cache hits bypassing verification
    - **Property 8: Cache hits bypass verification**
    - Generate cached recipes with `CountingIdentity`, verify zero function invocations regardless of mode
    - Use `proptest` with min 100 iterations
    - File: `crates/deriva-compute/tests/verif/properties.rs`
    - **Validates: Requirements 7.2**

- [ ] 6. Property-based tests for error format and distribution
  - [ ]* 6.1 Write property test for DeterminismViolation error format
    - **Property 9: DeterminismViolation error format**
    - Generate random DeterminismViolation values, verify Display format matches expected pattern via regex
    - Use `proptest` with min 100 iterations
    - File: `crates/deriva-compute/tests/verif/properties.rs`
    - **Validates: Requirements 5.5**

  - [ ]* 6.2 Write property test for sampling distribution approximating expected rate
    - **Property 10: Sampling distribution approximates expected rate**
    - Generate 256+ random CAddrs with various rates, verify observed verification fraction within tolerance (±1/255 + epsilon)
    - Use `proptest` with min 100 iterations
    - File: `crates/deriva-compute/tests/verif/properties.rs`
    - **Validates: Requirements 4.1**

- [ ] 7. Checkpoint - Property tests pass
  - Ensure all tests pass (`cargo test -p deriva-compute --test verification`), ask the user if questions arise.

- [ ] 8. Integration tests for server-level verification
  - [ ]* 8.1 Write integration test for Status RPC including verification statistics
    - Create service with DualCompute mode, execute recipes, verify Status response fields
    - File: `crates/deriva-server/tests/integration.rs`
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5_

  - [ ]* 8.2 Write integration test for Verify RPC with deterministic function
    - Verify RPC returns `deterministic: true`, correct hash, size, and timing
    - File: `crates/deriva-server/tests/integration.rs`
    - _Requirements: 9.1, 9.2_

  - [ ]* 8.3 Write integration test for Verify RPC not-found error
    - Verify RPC for nonexistent address returns NOT_FOUND status
    - File: `crates/deriva-server/tests/integration.rs`
    - _Requirements: 9.4_

  - [ ]* 8.4 Write integration test for parse_verification with valid and invalid inputs
    - Test "off", "dual", "sampled:0.1" parse correctly
    - Test invalid strings produce descriptive errors
    - File: `crates/deriva-server/tests/integration.rs`
    - _Requirements: 1.3, 1.4, 1.5, 1.6_

- [ ] 9. Final checkpoint - Full test suite passes
  - Ensure all tests pass (`cargo test -p deriva-compute -p deriva-server`), ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- The core verification implementation already exists in the codebase; tasks 1-2 are validation/hardening passes
- Property tests validate universal correctness properties from the design document
- Each property test maps 1:1 to a correctness property in the design
- All property tests use the `proptest` crate (already in workspace dependencies)
- Property tests go in `crates/deriva-compute/tests/verif/properties.rs` (new file, registered in `verification.rs` module)
- Integration tests extend the existing `crates/deriva-server/tests/integration.rs` file
- The implementation language is Rust (matching the existing codebase)

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2"] },
    { "id": 1, "tasks": ["1.3", "1.4"] },
    { "id": 2, "tasks": ["2.1", "2.2", "2.3"] },
    { "id": 3, "tasks": ["4.1", "4.2", "4.3", "4.4"] },
    { "id": 4, "tasks": ["5.1", "5.2", "5.3", "5.4"] },
    { "id": 5, "tasks": ["6.1", "6.2"] },
    { "id": 6, "tasks": ["8.1", "8.2", "8.3", "8.4"] }
  ]
}
```
