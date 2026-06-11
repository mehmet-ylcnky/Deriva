# Implementation Plan: Cascade Invalidation

## Overview

Cascade Invalidation adds transitive cache eviction to the Deriva system. The core implementation already exists across `deriva-core` (types, DAG, cache), `deriva-compute` (CascadeInvalidator engine, SharedCache), `deriva-server` (gRPC RPCs, metrics), and `deriva-cli` (flags). This task plan focuses on fixing a metrics recording bug, adding property-based tests for all 10 correctness properties, and verifying end-to-end integration.

## Tasks

- [ ] 1. Fix CASCADE_DEPTH metric recording for Policy::None
  - [ ] 1.1 Update cascade_invalidate RPC to skip metrics for Policy::None
    - In `crates/deriva-server/src/service.rs`, the `cascade_invalidate` handler currently records `CASCADE_DEPTH.observe(...)` unconditionally for all policies
    - Wrap the `CASCADE_DEPTH.observe(...)` and `CASCADE_EVICTED.observe(...)` calls in a conditional that only records when policy is Immediate or DryRun
    - Ensure Policy::None does NOT record CASCADE_DEPTH per Requirement 11.3
    - _Requirements: 11.1, 11.2, 11.3_

  - [ ] 1.2 Update invalidate RPC to record CASCADE_DEPTH only when cascade=true
    - In the `invalidate` handler, the cascade=true branch records metrics; verify it correctly records for Immediate only
    - _Requirements: 11.1, 11.3_

- [ ] 2. Property-based tests for core DAG and cache operations
  - [ ] 2.1 Create proptest infrastructure and helpers module
    - Create `crates/deriva-core/tests/cascade_proptest.rs`
    - Implement `Arbitrary`-style generators for random DAGs (1–50 nodes, 0–100 acyclic edges) and random cache states
    - Implement helper to build a random EvictableCache with random entries
    - _Requirements: 2.1, 2.3, 3.1_

  - [ ]* 2.2 Write property test: BFS traversal correctness (Property 3)
    - **Property 3: BFS traversal correctness**
    - For any random DAG and root, verify: (a) all returned addresses are reachable from root via reverse edges, (b) no duplicates, (c) root not in result, (d) max_depth equals max of individual depths or 0 if empty
    - **Validates: Requirements 2.1, 2.2, 2.3, 2.4, 2.5**

  - [ ]* 2.3 Write property test: Batch remove accuracy (Property 4)
    - **Property 4: Batch remove accuracy**
    - For any cache state and address list, verify remove_batch removes exactly the intersection of the list and cache keyset, returning correct count and addresses
    - **Validates: Requirements 3.1, 3.2, 3.4**

  - [ ]* 2.4 Write property test: Cache size invariant after batch remove (Property 5)
    - **Property 5: Cache size invariant after batch remove**
    - For any cache state, verify new current_size == old current_size - bytes_reclaimed after remove_batch
    - **Validates: Requirements 3.3**

- [ ] 3. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 4. Property-based tests for CascadeInvalidator policies
  - [ ] 4.1 Create proptest module for invalidator properties
    - Create `crates/deriva-compute/tests/cascade_proptest.rs`
    - Import DAG/cache generators from test helpers or re-implement locally
    - Set up proptest config with minimum 100 iterations
    - _Requirements: 1.1, 1.2, 1.3, 1.4_

  - [ ]* 4.2 Write property test: Policy::None performs no traversal (Property 1)
    - **Property 1: Policy::None performs no traversal**
    - For any DAG and cache, invoke with CascadePolicy::None and include_root=true, verify traversed_count==0, max_depth==0, and only root (if cached) is evicted
    - **Validates: Requirements 1.2, 4.7**

  - [ ]* 4.3 Write property test: DryRun is non-mutating preview of Immediate (Property 2)
    - **Property 2: DryRun is a non-mutating preview of Immediate**
    - For any DAG and cache, run DryRun and Immediate on same state, verify DryRun reports same evicted_count and traversed_count as Immediate, and cache is unchanged after DryRun
    - **Validates: Requirements 1.3, 1.4**

  - [ ]* 4.4 Write property test: include_root flag controls root eviction (Property 6)
    - **Property 6: include_root flag controls root eviction**
    - For any DAG/cache where root is cached, verify root absent after Immediate with include_root=true, and root remains after include_root=false
    - **Validates: Requirements 4.2, 4.3**

  - [ ]* 4.5 Write property test: detail_addrs flag controls evicted_addrs population (Property 7)
    - **Property 7: detail_addrs flag controls evicted_addrs population**
    - For any invalidation, verify evicted_addrs matches removed set when detail_addrs=true, and is empty when false
    - **Validates: Requirements 4.4, 4.5**

- [ ] 5. Property-based tests for async path and protocol parsing
  - [ ]* 5.1 Write property test: Async/sync equivalence (Property 8)
    - **Property 8: Async/sync equivalence**
    - For any DAG and cache (no concurrent mutations), verify async variant produces same evicted_count, traversed_count, and max_depth as sync variant
    - Create `crates/deriva-compute/tests/cascade_async_proptest.rs`
    - Use `#[tokio::test]` with proptest via `proptest!` macro inside async block
    - **Validates: Requirements 5.2**

  - [ ]* 5.2 Write property test: Policy string parsing round-trip (Property 9)
    - **Property 9: Policy string parsing round-trip**
    - For known strings {"none", "immediate", "dry_run", "dryrun"} verify correct variant; for arbitrary other strings verify Immediate default
    - Add test in `crates/deriva-server/tests/grpc_service.rs` or a new proptest file
    - **Validates: Requirements 7.3, 7.4, 7.5, 7.6**

  - [ ]* 5.3 Write property test: Contains accuracy without LRU side effects (Property 10)
    - **Property 10: Contains accuracy without LRU side effects**
    - For any SharedCache state and address, verify contains returns true iff address is present, and does not change eviction order
    - **Validates: Requirements 12.1, 12.2**

- [ ] 6. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 7. Integration tests for gRPC and CLI
  - [ ] 7.1 Write gRPC integration test for CascadeInvalidate RPC
    - Add tests in `crates/deriva-server/tests/grpc_service.rs`
    - Test CascadeInvalidateRequest with policy="immediate", include_root=true, detail_addrs=true on a multi-node DAG
    - Verify response fields: evicted_count, traversed_count, max_depth, bytes_reclaimed, evicted_addrs, duration_micros
    - Test with policy="dry_run" and verify cache unchanged
    - Test with policy="none" and verify only root evicted
    - Test with invalid addr and verify InvalidArgument status
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7_

  - [ ] 7.2 Write gRPC integration test for enhanced Invalidate RPC
    - Test InvalidateRequest with cascade=true and verify cascade behavior
    - Test InvalidateRequest with cascade=false and verify single-entry removal
    - Test with cascade field absent (default protobuf behavior) and verify backward compatibility
    - _Requirements: 8.1, 8.2, 8.3, 8.4_

  - [ ] 7.3 Write CLI integration test for cascade invalidation flags
    - Add tests in `crates/deriva-cli/tests/cli_integration.rs`
    - Test `invalidate <addr> --cascade` invokes CascadeInvalidateRequest with Immediate policy
    - Test `invalidate <addr> --dry-run` invokes CascadeInvalidateRequest with DryRun policy
    - Test `invalidate <addr> --cascade --detail` sets detail_addrs=true and prints evicted addresses
    - Test plain `invalidate <addr>` sends standard InvalidateRequest without cascade
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5_

- [ ] 8. Metrics and observability verification
  - [ ] 8.1 Write integration test for CASCADE_DEPTH metric recording
    - Add test in `crates/deriva-server/tests/observability.rs`
    - Perform cascade invalidation with Immediate policy and verify CASCADE_DEPTH histogram is observed
    - Perform cascade invalidation with DryRun policy and verify CASCADE_DEPTH histogram is observed
    - Perform invalidation with None policy and verify CASCADE_DEPTH histogram is NOT observed
    - _Requirements: 11.1, 11.2, 11.3_

- [ ] 9. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document using the `proptest` crate
- Unit tests validate specific examples and edge cases
- The implementation language is Rust, matching the design document
- Core implementation already exists — this plan focuses on correctness verification, bug fixes, and integration testing

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2", "2.1"] },
    { "id": 1, "tasks": ["2.2", "2.3", "2.4", "4.1"] },
    { "id": 2, "tasks": ["4.2", "4.3", "4.4", "4.5"] },
    { "id": 3, "tasks": ["5.1", "5.2", "5.3"] },
    { "id": 4, "tasks": ["7.1", "7.2", "7.3"] },
    { "id": 5, "tasks": ["8.1"] }
  ]
}
```
