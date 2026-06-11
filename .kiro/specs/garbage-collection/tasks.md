# Implementation Plan: Garbage Collection

## Overview

This plan implements mark-and-sweep garbage collection for the Deriva computation-addressed file system. The implementation proceeds bottom-up: core data structures (GcConfig, GcResult, PinSet), storage layer extensions (BlobStore removal/enumeration), the GC engine itself (run_gc), then integration layers (gRPC service, CLI commands, Prometheus metrics). Property-based tests validate correctness invariants at each layer.

## Tasks

- [ ] 1. Define core GC data structures
  - [ ] 1.1 Create GcConfig and GcResult structs in deriva-core
    - Define `GcConfig` struct with `grace_period: Duration`, `dry_run: bool`, `detail_addrs: bool`, `max_removals: usize`
    - Implement `Default` trait with specified defaults (300s, false, false, 0)
    - Define `GcResult` struct with all fields: `blobs_removed`, `recipes_removed`, `cache_entries_removed`, `bytes_reclaimed_blobs`, `bytes_reclaimed_cache`, `total_bytes_reclaimed`, `live_blobs`, `live_recipes`, `pinned_count`, `duration`, `removed_addrs`, `dry_run`
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.8, 7.9, 7.10_

  - [ ] 1.2 Implement PinSet in deriva-core
    - Create `PinSet` struct wrapping `HashSet<CAddr>`
    - Implement `pin`, `unpin`, `is_pinned`, `count`, `list`, `as_set` methods
    - `pin` returns true if newly added, false if already present
    - `unpin` returns true if was pinned, false otherwise
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8_

  - [ ]* 1.3 Write property test for PinSet round-trip consistency
    - **Property 1: PinSet round-trip consistency**
    - Generate random sequences of pin/unpin operations on arbitrary CAddr values
    - Verify: `is_pinned(addr)` ↔ `addr ∈ list()`, `count() == list().len()`, `as_set()` matches `list()`
    - **Validates: Requirements 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8**

- [ ] 2. Implement BlobStore extensions
  - [ ] 2.1 Implement remove_with_size and remove_batch_blobs in deriva-storage
    - Add `remove_with_size(&self, addr: &CAddr) -> Result<u64>` that deletes blob file and returns its size (0 if not found)
    - Add `remove_batch_blobs(&self, addrs: &[CAddr]) -> Result<(u64, u64)>` that removes each blob, returns (count, bytes)
    - Propagate I/O errors to caller (fail-fast on batch)
    - _Requirements: 3.1, 3.2, 3.3, 3.4_

  - [ ] 2.2 Implement list_addrs and stats in deriva-storage
    - Add `list_addrs(&self) -> Result<Vec<CAddr>>` walking 2-level shard directory structure
    - Skip filenames that cannot be decoded as valid CAddr
    - Add `stats(&self) -> Result<(u64, u64)>` returning (blob_count, total_bytes)
    - Return empty Vec / (0,0) if base directory does not exist or is empty
    - _Requirements: 4.1, 4.2, 4.3, 4.4_

  - [ ]* 2.3 Write property test for BlobStore remove correctness
    - **Property 2: BlobStore remove returns correct byte counts**
    - Generate random blob sets, store them, call `remove_with_size` and `remove_batch_blobs`
    - Verify: returned byte counts equal original data lengths, count equals number of existing blobs removed
    - **Validates: Requirements 3.1, 3.3**

  - [ ]* 2.4 Write property test for BlobStore enumeration completeness
    - **Property 3: BlobStore enumeration completeness**
    - Generate random blobs, store them, call `list_addrs()` and `stats()`
    - Verify: list_addrs contains every stored CAddr, stats count and bytes match
    - **Validates: Requirements 4.1, 4.3**

- [ ] 3. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 4. Implement PersistentDag extensions and live set computation
  - [ ] 4.1 Add live_addr_set, all_addrs, inputs, and remove to PersistentDag
    - Implement `live_addr_set(&self) -> HashSet<CAddr>` returning union of all recipe outputs and inputs
    - Implement `all_addrs(&self) -> Vec<CAddr>` returning all recipe output addresses
    - Implement `inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>>` returning inputs for a recipe
    - Implement `remove(&self, addr: &CAddr) -> Result<bool>` removing a recipe from the DAG
    - _Requirements: 5.1, 6.6, 6.8, 13.4_

  - [ ]* 4.2 Write property test for live set correctness
    - **Property 4: Live set is the union of DAG references and pins**
    - Generate random DAG states and PinSets
    - Verify: computed live set = recipe outputs ∪ recipe inputs ∪ pinned addresses
    - **Validates: Requirements 5.1, 5.2**

- [ ] 5. Implement the GC engine (run_gc)
  - [ ] 5.1 Implement run_gc core logic in deriva-server
    - Create `run_gc` async function with signature matching design
    - Step 1: Compute live set from DAG + PinSet
    - Step 2: Enumerate blobs via `list_addrs`
    - Step 3: Identify orphans (blobs not in live set)
    - Step 4: Apply `max_removals` truncation if configured
    - Step 5: If not dry_run, remove orphaned blobs via `remove_batch_blobs`
    - Step 6: Identify orphaned recipes (input in removed set, output not pinned)
    - Step 7: If not dry_run, remove stale cache entries via `SharedCache.remove_batch`
    - Step 8: If not dry_run, remove orphaned recipes from DAG
    - Step 9: Build and return GcResult with all fields populated
    - Handle dry_run mode: compute sizes without performing deletions
    - Handle detail_addrs: populate removed_addrs when true, empty Vec when false
    - _Requirements: 5.2, 5.3, 5.4, 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7, 6.8, 6.9, 6.10, 6.11, 13.1, 13.2, 13.3, 13.4_

  - [ ]* 5.2 Write property test for GC safety
    - **Property 5: GC safety — live blobs are never removed**
    - Generate random DAG + blobs + pins, run GC, verify all live blobs remain in BlobStore
    - **Validates: Requirements 5.3, 13.2**

  - [ ]* 5.3 Write property test for GC liveness
    - **Property 6: GC liveness — orphaned blobs are removed**
    - Generate random orphaned blobs (not in live set), run GC with dry_run=false and max_removals=0
    - Verify all orphaned blobs are removed
    - **Validates: Requirements 5.4, 6.4**

  - [ ]* 5.4 Write property test for dry-run non-destructive
    - **Property 7: Dry-run is non-destructive**
    - Generate state, run GC with dry_run=true
    - Verify BlobStore, DAG, and Cache are unchanged; GcResult.blobs_removed matches orphan count
    - **Validates: Requirements 6.5**

  - [ ]* 5.5 Write property test for max-removals bounds
    - **Property 8: Max-removals bounds collection**
    - Generate N orphaned blobs with max_removals=M where M < N
    - Verify GcResult.blobs_removed == M
    - **Validates: Requirements 6.3**

  - [ ]* 5.6 Write property test for orphaned recipe pin protection
    - **Property 9: Orphaned recipe removal respects pins**
    - Generate recipes with orphaned inputs, pin some outputs
    - Verify pinned recipe outputs remain in DAG, unpinned are removed
    - **Validates: Requirements 6.6, 13.1, 13.2, 13.3**

  - [ ]* 5.7 Write property test for total_bytes_reclaimed invariant
    - **Property 10: GcResult total_bytes_reclaimed invariant**
    - Run GC on any state, verify total_bytes_reclaimed == bytes_reclaimed_blobs + bytes_reclaimed_cache
    - **Validates: Requirements 6.9, 7.6**

  - [ ]* 5.8 Write property test for detail addrs completeness
    - **Property 11: Detail addrs completeness**
    - Run GC with detail_addrs=true
    - Verify removed_addrs contains exactly union of removed blob addrs and removed recipe output addrs
    - Verify removed_addrs.len() == blobs_removed + recipes_removed
    - **Validates: Requirements 6.10, 6.11**

- [ ] 6. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 7. Implement gRPC service integration
  - [ ] 7.1 Add GC protobuf messages and service RPC definitions
    - Define `GcRequest` message with `grace_period_secs`, `dry_run`, `detail_addrs`, `max_removals`
    - Define `GcResponse` message with all GcResult fields mapped to proto types
    - Define `PinRequest`, `PinResponse`, `UnpinRequest`, `UnpinResponse`, `ListPinsRequest`, `ListPinsResponse`
    - Add `GarbageCollect`, `Pin`, `Unpin`, `ListPins` RPCs to the service definition
    - _Requirements: 8.1, 8.2, 9.1, 9.2, 9.3_

  - [ ] 7.2 Implement gRPC handlers in DerivaService
    - Implement `GarbageCollect` handler: parse GcRequest into GcConfig, acquire PinSet read lock, call run_gc, map GcResult to GcResponse
    - Implement `Pin` handler: parse addr bytes as CAddr, pin in ServerState, return was_new
    - Implement `Unpin` handler: parse addr bytes as CAddr, unpin in ServerState, return was_pinned
    - Implement `ListPins` handler: list all pinned addrs, return as repeated bytes + count
    - Map internal errors: DerivaError::Storage → INTERNAL, invalid CAddr → INVALID_ARGUMENT
    - _Requirements: 8.1, 8.2, 8.3, 9.1, 9.2, 9.3, 9.4_

  - [ ]* 7.3 Write integration tests for gRPC GC and pin RPCs
    - Test GarbageCollect RPC end-to-end with various config combinations
    - Test Pin/Unpin/ListPins RPCs with valid and invalid addresses
    - Test error mapping for invalid CAddr and storage errors
    - _Requirements: 8.1, 8.2, 8.3, 9.1, 9.2, 9.3, 9.4_

- [ ] 8. Implement CLI commands
  - [ ] 8.1 Add gc CLI command with flags
    - Add `gc` subcommand with `--dry-run`, `--grace-period <secs>`, `--detail`, `--max-removals <n>` flags
    - Map flags to GcRequest fields, send to server
    - Print summary: blobs_removed, recipes_removed, cache_entries_removed, total_bytes_reclaimed, live_blobs, live_recipes, pinned_count, duration
    - When --detail is set, print each removed address
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5, 10.6_

  - [ ] 8.2 Add pin, unpin, and list-pins CLI commands
    - Add `pin <addr>` subcommand: send PinRequest, print whether newly pinned
    - Add `unpin <addr>` subcommand: send UnpinRequest, print whether was previously pinned
    - Add `list-pins` subcommand: send ListPinsRequest, print all addrs and count
    - _Requirements: 11.1, 11.2, 11.3_

  - [ ]* 8.3 Write integration tests for CLI commands
    - Test gc command output formatting with dry-run and detail flags
    - Test pin/unpin/list-pins commands with valid addresses
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5, 10.6, 11.1, 11.2, 11.3_

- [ ] 9. Implement Prometheus observability metrics
  - [ ] 9.1 Register GC metrics and instrument run_gc
    - Register `deriva_gc_runs_total` counter with `mode` label
    - Register `deriva_gc_blobs_removed` histogram
    - Register `deriva_gc_bytes_reclaimed` histogram
    - Register `deriva_gc_duration_seconds` histogram
    - Register `deriva_gc_live_blobs` gauge
    - Register `deriva_gc_pinned_count` gauge
    - After each GC cycle in the service handler, record all metrics from GcResult
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5, 12.6_

  - [ ]* 9.2 Write unit tests for metrics recording
    - Verify counter increments after GC cycle
    - Verify gauge values match GcResult fields
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5, 12.6_

- [ ] 10. Wire ServerState and integration
  - [ ] 10.1 Add PinSet to ServerState and wire all components
    - Add `pin_set: Arc<RwLock<PinSet>>` to ServerState
    - Initialize PinSet in server startup
    - Ensure GarbageCollect handler acquires PinSet read lock before calling run_gc
    - Ensure Pin/Unpin handlers acquire PinSet write lock
    - Verify lock ordering compatibility with existing get() path
    - _Requirements: 2.8, 8.1, 9.1, 9.2_

- [ ] 11. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties using the `proptest` crate
- Unit tests validate specific examples and edge cases
- The implementation language is Rust, matching the existing codebase
- Lock ordering (PinSet → DAG → BlobStore → Cache) must be preserved to prevent deadlocks

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2"] },
    { "id": 1, "tasks": ["1.3", "2.1", "2.2"] },
    { "id": 2, "tasks": ["2.3", "2.4", "4.1"] },
    { "id": 3, "tasks": ["4.2", "5.1"] },
    { "id": 4, "tasks": ["5.2", "5.3", "5.4", "5.5", "5.6", "5.7", "5.8"] },
    { "id": 5, "tasks": ["7.1", "8.1", "8.2", "9.1", "10.1"] },
    { "id": 6, "tasks": ["7.2"] },
    { "id": 7, "tasks": ["7.3", "8.3", "9.2"] }
  ]
}
```
