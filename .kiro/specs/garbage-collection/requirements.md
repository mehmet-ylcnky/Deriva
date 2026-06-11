# Requirements Document

## Introduction

Garbage Collection (Phase 2.8) adds mark-and-sweep storage reclamation to the Deriva computation-addressed distributed file system. Over time, the system accumulates orphaned blobs (leaf data no recipe references), orphaned recipes (recipes with broken inputs), and stale cache entries. The GC subsystem identifies and removes these unreachable objects while respecting pinned addresses and a configurable grace period to prevent races with concurrent puts. The feature provides dry-run previews, incremental collection via max-removal limits, detailed reporting, and full gRPC and CLI integration with Prometheus observability metrics.

## Glossary

- **CAddr**: A content address (32-byte hash) that uniquely identifies a leaf value or derived recipe in the system
- **BlobStore**: The persistent storage layer for leaf data, organized in a 2-level shard directory structure (base/XX/YY/HASH)
- **PersistentDag**: The dependency graph storing recipes with their inputs and outputs, providing forward and reverse edge traversal
- **SharedCache**: The async-safe cache wrapper holding materialized values keyed by CAddr, supporting single and batch removal
- **GC_Engine**: The async mark-and-sweep garbage collector implemented as the run_gc function in deriva-server
- **GcResult**: A struct containing the outcome of a garbage collection cycle including counts of removed items, bytes reclaimed, live counts, duration, and optional removed address details
- **GcConfig**: A struct containing configuration for a GC cycle: grace_period, dry_run, detail_addrs, and max_removals
- **PinSet**: A HashSet-based container of CAddr values explicitly protected from garbage collection, behind Arc<RwLock> in ServerState
- **Live_Set**: The union of all recipe output addresses, all recipe input addresses, and all pinned addresses; any blob not in this set is considered orphaned
- **Orphaned_Blob**: A blob in BlobStore whose CAddr does not appear in the Live_Set
- **Orphaned_Recipe**: A recipe in PersistentDag where at least one input references a removed blob AND whose output is not pinned
- **Grace_Period**: A configurable Duration (default 5 minutes) protecting recently-put blobs from collection to prevent races between put_leaf and GC
- **Dry_Run**: A GC mode that computes and reports what would be removed without performing actual deletions
- **Max_Removals**: A configurable limit on the number of blobs removed in a single GC cycle (0 means unlimited)
- **ServerState**: The shared server state struct holding the PersistentDag, BlobStore, SharedCache, and PinSet

## Requirements

### Requirement 1: GC Configuration

**User Story:** As a system operator, I want to configure garbage collection parameters, so that I can control GC behavior including grace periods, dry-run mode, detail verbosity, and removal limits.

#### Acceptance Criteria

1. THE GcConfig SHALL define a grace_period field of type Duration with a default value of 300 seconds
2. THE GcConfig SHALL define a dry_run field of type bool with a default value of false
3. THE GcConfig SHALL define a detail_addrs field of type bool with a default value of false
4. THE GcConfig SHALL define a max_removals field of type usize with a default value of 0, where 0 represents unlimited removals
5. THE GcConfig SHALL implement the Default trait with the specified default values

### Requirement 2: Pin Set Management

**User Story:** As a developer, I want to pin addresses to protect them from garbage collection, so that critical data remains available regardless of whether it is referenced by any recipe.

#### Acceptance Criteria

1. WHEN pin is called with a CAddr, THE PinSet SHALL insert the address and return true if the address was newly added
2. WHEN pin is called with an already-pinned CAddr, THE PinSet SHALL return false without modifying state
3. WHEN unpin is called with a pinned CAddr, THE PinSet SHALL remove the address and return true
4. WHEN unpin is called with a CAddr that is not pinned, THE PinSet SHALL return false without modifying state
5. WHEN is_pinned is called, THE PinSet SHALL return true if the address is in the set, false otherwise
6. WHEN count is called, THE PinSet SHALL return the number of currently pinned addresses
7. WHEN list is called, THE PinSet SHALL return a Vec containing all currently pinned CAddr values
8. THE PinSet SHALL provide an as_set method returning a reference to the underlying HashSet for live set computation

### Requirement 3: BlobStore Removal Operations

**User Story:** As a developer, I want the BlobStore to support removal of individual and batches of blobs with size reporting, so that the GC engine can reclaim storage and report bytes freed.

#### Acceptance Criteria

1. WHEN remove_with_size is called with a CAddr, THE BlobStore SHALL delete the blob file and return the size in bytes of the removed blob
2. WHEN remove_with_size is called with a CAddr that does not exist, THE BlobStore SHALL return 0 without error
3. WHEN remove_batch_blobs is called with a slice of CAddr values, THE BlobStore SHALL remove each blob that exists and return a tuple of (count_removed, bytes_removed)
4. IF an I/O error occurs during blob removal, THEN THE BlobStore SHALL propagate the error to the caller

### Requirement 4: BlobStore Enumeration and Statistics

**User Story:** As a developer, I want to enumerate all blob addresses and retrieve aggregate statistics from the BlobStore, so that the GC engine can identify orphaned blobs and report live counts.

#### Acceptance Criteria

1. WHEN list_addrs is called, THE BlobStore SHALL walk the 2-level shard directory structure and return a Vec of all CAddr values present in the store
2. WHEN list_addrs encounters a filename that cannot be decoded as a valid CAddr, THE BlobStore SHALL skip that entry without error
3. WHEN stats is called, THE BlobStore SHALL return a tuple of (blob_count, total_bytes) by walking all shard directories
4. IF the base directory does not exist or is empty, THEN THE BlobStore SHALL return an empty Vec from list_addrs and (0, 0) from stats

### Requirement 5: Live Set Computation

**User Story:** As a developer, I want to compute the set of live addresses from the DAG and pin set, so that the GC engine can determine which blobs are orphaned.

#### Acceptance Criteria

1. THE PersistentDag SHALL provide a live_addr_set method that returns the union of all recipe output addresses and all recipe input addresses
2. WHEN compute_live_set is called with a PersistentDag and PinSet, THE GC_Engine SHALL return a HashSet containing all recipe outputs, all recipe inputs, and all pinned addresses
3. WHEN a CAddr is present in the Live_Set, THE GC_Engine SHALL treat it as non-orphaned regardless of whether it exists in the BlobStore
4. WHEN a blob CAddr is not in the Live_Set, THE GC_Engine SHALL identify it as an Orphaned_Blob eligible for removal

### Requirement 6: Mark-and-Sweep GC Engine

**User Story:** As a developer, I want an async mark-and-sweep GC engine that identifies and removes orphaned blobs, broken recipes, and stale cache entries, so that storage is reclaimed safely.

#### Acceptance Criteria

1. WHEN run_gc is called, THE GC_Engine SHALL compute the Live_Set from the PersistentDag and PinSet
2. WHEN run_gc is called, THE GC_Engine SHALL enumerate all blobs via BlobStore list_addrs and identify those not in the Live_Set as orphaned
3. WHEN max_removals is greater than 0 and the orphaned blob count exceeds max_removals, THE GC_Engine SHALL truncate the orphaned blob list to max_removals entries
4. WHEN dry_run is false, THE GC_Engine SHALL remove orphaned blobs from the BlobStore via remove_batch_blobs
5. WHEN dry_run is true, THE GC_Engine SHALL compute the size of orphaned blobs without performing any deletions
6. WHEN blobs have been removed, THE GC_Engine SHALL identify orphaned recipes as those with at least one input in the removed blob set AND whose output is not pinned
7. WHEN dry_run is false, THE GC_Engine SHALL remove stale cache entries for all orphaned blobs and orphaned recipes via SharedCache remove_batch
8. WHEN dry_run is false, THE GC_Engine SHALL remove orphaned recipes from the PersistentDag
9. WHEN run_gc completes, THE GC_Engine SHALL return a GcResult populated with all removal counts, bytes reclaimed, live counts, pinned count, duration, and the dry_run flag
10. WHEN detail_addrs is true, THE GcResult SHALL contain the list of all removed CAddr values in removed_addrs
11. WHEN detail_addrs is false, THE GcResult SHALL contain an empty removed_addrs Vec

### Requirement 7: GC Result Reporting

**User Story:** As a system operator, I want comprehensive GC results showing what was removed and what remains, so that I can monitor storage health and verify GC correctness.

#### Acceptance Criteria

1. THE GcResult SHALL contain blobs_removed representing the number of orphaned blobs removed or that would be removed in dry-run mode
2. THE GcResult SHALL contain recipes_removed representing the number of orphaned recipes removed
3. THE GcResult SHALL contain cache_entries_removed representing the number of stale cache entries removed
4. THE GcResult SHALL contain bytes_reclaimed_blobs representing the total bytes freed from the BlobStore
5. THE GcResult SHALL contain bytes_reclaimed_cache representing the total bytes freed from the cache
6. THE GcResult SHALL contain total_bytes_reclaimed equal to bytes_reclaimed_blobs plus bytes_reclaimed_cache
7. THE GcResult SHALL contain live_blobs representing the count of blobs remaining after GC
8. THE GcResult SHALL contain live_recipes representing the count of recipes remaining after GC
9. THE GcResult SHALL contain pinned_count representing the number of addresses currently pinned
10. THE GcResult SHALL contain duration representing the wall-clock time of the GC cycle

### Requirement 8: gRPC GarbageCollect RPC

**User Story:** As a client, I want a GarbageCollect RPC with full configuration control and detailed results, so that I can trigger and monitor garbage collection remotely.

#### Acceptance Criteria

1. WHEN a GcRequest is received with grace_period_secs, dry_run, detail_addrs, and max_removals fields, THE DerivaService SHALL execute run_gc with the corresponding GcConfig and return a GcResponse
2. THE GcResponse SHALL contain blobs_removed, recipes_removed, cache_entries_removed, bytes_reclaimed_blobs, bytes_reclaimed_cache, total_bytes_reclaimed, live_blobs, live_recipes, pinned_count, duration_micros, removed_addrs, and dry_run fields
3. IF run_gc returns an error, THEN THE DerivaService SHALL return an Internal gRPC status error with the error message

### Requirement 9: gRPC Pin Management RPCs

**User Story:** As a client, I want Pin, Unpin, and ListPins RPCs, so that I can manage the pin set remotely to protect addresses from garbage collection.

#### Acceptance Criteria

1. WHEN a PinRequest is received with an addr field, THE DerivaService SHALL pin the address in the ServerState PinSet and return a PinResponse with was_new indicating whether the address was newly pinned
2. WHEN an UnpinRequest is received with an addr field, THE DerivaService SHALL unpin the address from the ServerState PinSet and return an UnpinResponse with was_pinned indicating whether the address was previously pinned
3. WHEN a ListPinsRequest is received, THE DerivaService SHALL return a ListPinsResponse containing all pinned addresses as repeated bytes and a count field
4. IF the addr field in PinRequest or UnpinRequest cannot be parsed as a valid CAddr, THEN THE DerivaService SHALL return an InvalidArgument gRPC status error

### Requirement 10: CLI Garbage Collection Command

**User Story:** As an operator, I want a gc CLI command with flags for dry-run, grace period, detail, and max removals, so that I can trigger and inspect garbage collection from the command line.

#### Acceptance Criteria

1. WHEN the gc command is executed, THE CLI SHALL send a GcRequest to the server with the specified configuration parameters
2. WHEN the --dry-run flag is provided, THE CLI SHALL set dry_run=true in the GcRequest
3. WHEN the --grace-period flag is provided with a value in seconds, THE CLI SHALL set grace_period_secs to that value in the GcRequest
4. WHEN the --detail flag is provided, THE CLI SHALL set detail_addrs=true and print each removed address from the response
5. WHEN the --max-removals flag is provided with a value, THE CLI SHALL set max_removals to that value in the GcRequest
6. WHEN a GC operation completes, THE CLI SHALL print blobs_removed, recipes_removed, cache_entries_removed, total_bytes_reclaimed, live_blobs, live_recipes, pinned_count, and duration

### Requirement 11: CLI Pin Management Commands

**User Story:** As an operator, I want pin, unpin, and list-pins CLI commands, so that I can manage the pin set from the command line.

#### Acceptance Criteria

1. WHEN the pin command is executed with an address argument, THE CLI SHALL send a PinRequest and print whether the address was newly pinned
2. WHEN the unpin command is executed with an address argument, THE CLI SHALL send an UnpinRequest and print whether the address was previously pinned
3. WHEN the list-pins command is executed, THE CLI SHALL send a ListPinsRequest and print all pinned addresses with their count

### Requirement 12: GC Observability Metrics

**User Story:** As an operator, I want Prometheus metrics for garbage collection activity, so that I can monitor GC health and tune configuration over time.

#### Acceptance Criteria

1. WHEN a GC cycle completes, THE system SHALL increment a gc_runs_total counter metric
2. WHEN a GC cycle removes blobs, THE system SHALL increment a gc_blobs_removed_total counter by the number of blobs removed
3. WHEN a GC cycle reclaims bytes, THE system SHALL increment a gc_bytes_reclaimed_total counter by the total bytes reclaimed
4. WHEN a GC cycle completes, THE system SHALL observe the cycle duration in a gc_duration_seconds histogram metric
5. WHEN a GC cycle completes, THE system SHALL set a gc_live_blobs gauge to the number of live blobs remaining
6. WHEN a GC cycle completes, THE system SHALL set a gc_pinned_count gauge to the number of currently pinned addresses

### Requirement 13: Orphaned Recipe Safety

**User Story:** As a developer, I want GC to only remove recipes that are truly broken and unrestorable, so that pinned recipe outputs are never inadvertently removed.

#### Acceptance Criteria

1. WHEN a recipe has at least one input in the removed blob set, THE GC_Engine SHALL classify it as an Orphaned_Recipe candidate
2. WHEN an Orphaned_Recipe candidate has its output address in the PinSet, THE GC_Engine SHALL skip removal of that recipe
3. WHEN an Orphaned_Recipe candidate has its output address NOT in the PinSet, THE GC_Engine SHALL remove the recipe from the PersistentDag
4. THE GC_Engine SHALL remove orphaned recipes only AFTER sweeping orphaned blobs, ensuring the removed blob set is finalized before recipe analysis
