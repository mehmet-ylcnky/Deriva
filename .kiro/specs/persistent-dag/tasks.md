# Implementation Plan: Persistent DAG

## Overview

Complete the PersistentDag implementation by adding LRU cache acceleration, defining the DagAccess trait abstraction, adapting the in-memory DagStore for trait compatibility (interior mutability), implementing migration and consistency repair in StorageBackend, updating ServerState integration, and adding property-based tests to validate correctness properties.

## Tasks

- [x] 1. Define DagAccess trait and adapt DagStore
  - [x] 1.1 Define the DagAccess trait in `crates/deriva-core/src/dag.rs`
    - Add a `DagAccess` trait with methods: `insert(&self, recipe: &Recipe) -> Result<CAddr>`, `contains(&self, addr: &CAddr) -> bool`, `inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>>`, `direct_dependents(&self, addr: &CAddr) -> Vec<CAddr>`, `transitive_dependents(&self, addr: &CAddr) -> Vec<CAddr>`, `resolve_order(&self, addr: &CAddr) -> Vec<CAddr>`, `depth(&self, addr: &CAddr) -> u32`, `len(&self) -> usize`, `is_empty(&self) -> bool`, `remove(&self, addr: &CAddr) -> Result<bool>`
    - Trait must require `Send + Sync`
    - Export the trait from `crates/deriva-core/src/lib.rs`
    - _Requirements: 16.1_

  - [x] 1.2 Refactor DagStore to use interior mutability and implement DagAccess
    - Wrap internal state in `RwLock` or `Mutex` so all methods take `&self`
    - Change `remove` to return `Result<bool>` instead of `Option<Recipe>`
    - Implement the `DagAccess` trait for `DagStore`
    - Ensure existing tests still pass with the refactored interface
    - _Requirements: 16.3, 16.4_

  - [x] 1.3 Implement DagAccess for PersistentDag
    - Add `impl DagAccess for PersistentDag` in `crates/deriva-core/src/persistent_dag.rs`
    - All trait methods delegate to existing inherent implementations
    - Ensure PersistentDag derives/implements Clone + Send + Sync
    - _Requirements: 12.5, 16.2_

- [x] 2. Add LRU cache layer to PersistentDag
  - [x] 2.1 Add LRU cache fields and configuration to PersistentDag
    - Add `lru` crate dependency to `crates/deriva-core/Cargo.toml`
    - Add `PersistentDagConfig` struct with configurable forward and reverse cache sizes (default 10,000 each)
    - Add `forward_cache: Arc<Mutex<LruCache<CAddr, Vec<CAddr>>>>` and `reverse_cache: Arc<Mutex<LruCache<CAddr, Vec<CAddr>>>>` fields
    - Add `open_with_config(db: &Db, config: PersistentDagConfig) -> Result<Self>` constructor
    - Update existing `open` to use default config
    - _Requirements: 13.1, 13.2_

  - [x] 2.2 Integrate LRU cache into read paths (inputs, direct_dependents)
    - In `inputs()`: check `forward_cache` first; on miss, read from sled and populate cache
    - In `direct_dependents()`: check `reverse_cache` first; on miss, read from sled and populate cache
    - Cache operations must not propagate lock poisoning to callers (use unwrap — acceptable per design)
    - _Requirements: 13.3_

  - [x] 2.3 Integrate LRU cache into write paths (insert, remove)
    - In `insert()`: after successful transaction, populate `forward_cache` for new entry and invalidate `reverse_cache` entries for each input
    - In `remove()`: after successful removal, evict `forward_cache` for removed address and invalidate `reverse_cache` entries for each former input
    - _Requirements: 13.4, 13.5_

  - [ ]* 2.4 Write unit tests for LRU cache behavior
    - Test cache hit returns correct data without sled read
    - Test cache invalidation on insert
    - Test cache eviction on remove
    - Test cache population on miss
    - _Requirements: 13.3, 13.4, 13.5_

- [x] 3. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 4. Implement migration and consistency repair in StorageBackend
  - [x] 4.1 Implement one-time migration logic in StorageBackend
    - When `PersistentDag` forward tree is empty and `SledRecipeStore` has recipes, iterate all recipes and insert each into the DAG
    - After migration completes, call `dag.flush()` once
    - If DAG already has entries, skip migration entirely
    - If individual recipe fails during migration (deserialization error), skip it, log the failure, and continue
    - _Requirements: 14.1, 14.2, 14.3, 14.4_

  - [x] 4.2 Implement consistency repair in StorageBackend
    - Add `repair_consistency(&self) -> Result<u64>` method
    - Iterate all entries in `SledRecipeStore`, identify recipes whose CAddr is not present in the DAG forward index
    - Insert each missing recipe into the DAG using standard insert logic
    - If all repairs succeed, flush once; if no inconsistencies, do not modify DAG or invoke flush
    - On individual failures, skip and continue; return error with count of failed repairs
    - Log the count of repaired recipes when at least one is repaired
    - _Requirements: 15.1, 15.2, 15.3, 15.4, 15.5, 15.6_

  - [x] 4.3 Wire migration and repair into StorageBackend::open
    - Call migration logic during `open()` (before repair)
    - Call `repair_consistency()` after migration (or if migration was skipped)
    - Ensure startup remains O(1) when DAG is already populated and consistent (no full iteration)
    - _Requirements: 2.1, 2.2, 2.3, 14.3, 15.4_

  - [ ]* 4.4 Write integration tests for migration and repair
    - Test migration triggers when DAG empty but recipe store populated
    - Test migration skipped when DAG already populated
    - Test repair detects and fixes missing DAG entries
    - Test repair no-op when fully consistent
    - Test partial failures during migration/repair are skipped gracefully
    - _Requirements: 14.1, 14.2, 14.3, 14.4, 15.1, 15.2, 15.3, 15.4, 15.5, 15.6_

- [x] 5. Update ServerState and integration points
  - [x] 5.1 Update ServerState to hold PersistentDag directly (no RwLock)
    - Modify `ServerState` struct in `crates/deriva-server` to hold `PersistentDag` directly instead of any `RwLock<DagStore>`
    - Update all call sites that accessed the DAG through a lock to use direct method calls
    - Ensure concurrent access works via sled's internal thread-safety
    - _Requirements: 12.3, 17.3_

  - [x] 5.2 Ensure StorageBackend shares single sled::Db instance
    - Verify `StorageBackend::open` opens exactly one `sled::Db` and passes it to both `SledRecipeStore` and `PersistentDag`
    - PersistentDag uses named trees `dag_forward` and `dag_reverse`, distinct from the default tree
    - If sled::Db or named tree fails to open, return error without constructing partial state
    - _Requirements: 17.1, 17.2, 17.4, 17.5_

- [ ] 6. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 7. Property-based tests for correctness properties
  - [ ]* 7.1 Write property test: Insert-Query Round Trip
    - **Property 1: Insert-Query Round Trip**
    - For any valid Recipe with non-self-referencing inputs, inserting and querying `inputs(addr)` returns `Some(inputs)` positionally identical to original
    - Use `proptest` to generate random CAddr values and Recipe instances
    - **Validates: Requirements 1.2, 6.1, 6.2, 6.3**

  - [ ]* 7.2 Write property test: Idempotent Insert
    - **Property 2: Idempotent Insert**
    - Inserting same recipe N times produces same `len()` as inserting once, same `inputs()` result, no error
    - **Validates: Requirements 1.2, 3.4, 4.1, 4.2, 4.3**

  - [ ]* 7.3 Write property test: Self-Cycle Rejection Preserves State
    - **Property 3: Self-Cycle Rejection Preserves State**
    - Recipe whose address appears in its own inputs returns CycleDetected error; DAG state unchanged
    - **Validates: Requirements 1.7, 5.1, 5.2**

  - [ ]* 7.4 Write property test: Reverse Edge Correctness
    - **Property 4: Reverse Edge Correctness**
    - For any set of inserted recipes, `direct_dependents(addr)` returns exactly the set of recipe addresses whose inputs contain `addr`
    - **Validates: Requirements 1.3, 7.1, 7.2, 7.3**

  - [ ]* 7.5 Write property test: Remove Correctness
    - **Property 5: Remove Correctness**
    - Removing present address returns true, causes contains to return false, removes from dependents; removing absent returns false without modification
    - **Validates: Requirements 7.5, 11.1, 11.2, 11.3, 11.4, 11.5, 11.6**

  - [ ]* 7.6 Write property test: Persistence Round Trip
    - **Property 6: Persistence Round Trip**
    - After insert/remove sequence, close and reopen DAG; all queries return identical results
    - **Validates: Requirements 1.4, 6.5, 17.5**

  - [ ]* 7.7 Write property test: Transitive Dependents Completeness
    - **Property 7: Transitive Dependents Completeness**
    - `transitive_dependents(addr)` returns exactly BFS-reachable set, each address once, queried address excluded, level-order maintained
    - **Validates: Requirements 8.1, 8.2, 8.3, 8.4**

  - [ ]* 7.8 Write property test: Topological Order Validity
    - **Property 8: Topological Order Validity**
    - `resolve_order(addr)` returns sequence where inputs appear before dependents, queried address last, only forward-tree entries included
    - **Validates: Requirements 9.1, 9.2, 9.3, 9.4**

  - [ ]* 7.9 Write property test: Depth Computation Correctness
    - **Property 9: Depth Computation Correctness**
    - `depth(addr)` returns 0 for leaves, `1 + max(depth(inputs))` for recipes with inputs
    - **Validates: Requirements 10.1, 10.2**

  - [ ]* 7.10 Write property test: Bincode Serialization Round Trip
    - **Property 10: Bincode Serialization Round Trip**
    - For any `Vec<CAddr>` of length 0..1000, serialize then deserialize produces equal value
    - **Validates: Requirements 1.5**

  - [ ]* 7.11 Write property test: Model Equivalence (DagStore vs PersistentDag)
    - **Property 11: Model Equivalence**
    - For any sequence of insert/remove operations applied to both DagStore and PersistentDag, all query methods return equivalent results
    - Place in `crates/deriva-core/tests/dag_access_model.rs`
    - **Validates: Requirements 16.4**

- [ ] 8. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The existing PersistentDag implementation already covers basic insert/remove/query functionality; this plan focuses on the remaining gaps (LRU cache, DagAccess trait, migration, repair, property tests)
- All code uses Rust with existing project dependencies (sled, bincode, proptest)

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["1.2", "1.3"] },
    { "id": 2, "tasks": ["2.1"] },
    { "id": 3, "tasks": ["2.2", "2.3"] },
    { "id": 4, "tasks": ["2.4", "4.1"] },
    { "id": 5, "tasks": ["4.2", "4.3"] },
    { "id": 6, "tasks": ["4.4", "5.1", "5.2"] },
    { "id": 7, "tasks": ["7.1", "7.2", "7.3", "7.4", "7.10"] },
    { "id": 8, "tasks": ["7.5", "7.6", "7.7", "7.8", "7.9", "7.11"] }
  ]
}
```
