# Requirements Document

## Introduction

Cascade Invalidation (Phase 2.6) adds transitive cache eviction to the Deriva system. When a leaf or recipe is invalidated, all downstream (transitive) dependents are automatically evicted from the cache. This eliminates the "silent stale data" problem where only the root is removed but its dependents remain cached with stale data. The feature supports configurable policies (immediate eviction, dry-run inspection, or no cascade), exposes results via gRPC and CLI, and maintains deadlock-free lock ordering with the existing get() path.

## Glossary

- **CAddr**: A content address (32-byte hash) that uniquely identifies a leaf value or derived recipe in the system
- **CascadeInvalidator**: The stateless engine that connects DAG reverse-edge traversal to batch cache eviction, executing cascade invalidation operations
- **CascadePolicy**: An enum controlling invalidation behavior with three variants: None, Immediate, and DryRun
- **InvalidationResult**: A struct containing the outcome of a cascade invalidation operation including evicted count, traversed count, max depth, bytes reclaimed, evicted addresses, and duration
- **EvictableCache**: The in-memory cache holding materialized values keyed by CAddr, supporting single and batch removal
- **SharedCache**: The async-safe cache wrapper using tokio RwLock, providing async batch removal and containment checks
- **DagStore**: The dependency graph store providing forward (inputs) and reverse (dependents) edge queries
- **BFS_Traversal**: Breadth-first search over reverse edges to discover all transitive dependents of a given address
- **Batch_Remove**: A cache operation that removes multiple entries in a single pass, returning aggregate statistics
- **Reverse_Edge**: A DAG edge from an input address to a recipe that depends on it, enabling dependent discovery
- **Transitive_Dependent**: Any address reachable from a root via one or more reverse edges in the DAG
- **Depth**: The BFS level distance from the root address to a transitive dependent, where direct dependents have depth 1
- **DryRun**: A cascade policy that traverses dependents and reports what would be evicted without performing actual eviction
- **CASCADE_DEPTH**: A histogram metric tracking the maximum depth reached during cascade invalidation operations

## Requirements

### Requirement 1: Cascade Policy Selection

**User Story:** As a developer, I want to choose between different invalidation behaviors, so that I can control whether eviction happens immediately, is previewed first, or is skipped entirely.

#### Acceptance Criteria

1. THE CascadePolicy enum SHALL define exactly three variants: None, Immediate, and DryRun
2. WHEN CascadePolicy is None, THE CascadeInvalidator SHALL evict only the root address from the cache without traversing reverse edges (Phase 1 backward-compatible behavior)
3. WHEN CascadePolicy is Immediate, THE CascadeInvalidator SHALL traverse all transitive dependents via reverse edges and evict each from the cache
4. WHEN CascadePolicy is DryRun, THE CascadeInvalidator SHALL traverse all transitive dependents via reverse edges and report which cached entries would be evicted without performing actual eviction
5. THE CascadePolicy enum SHALL implement Default with Immediate as the default variant

### Requirement 2: Depth-Tracking BFS Traversal

**User Story:** As a developer, I want the DAG traversal to track BFS depth for each dependent, so that I can observe invalidation blast radius and reason about cascade complexity.

#### Acceptance Criteria

1. WHEN transitive_dependents_with_depth is called for a root address, THE DagStore SHALL return a Vec of (CAddr, depth) pairs where depth represents the BFS level distance from the root starting at 1 for direct dependents
2. WHEN transitive_dependents_with_depth is called, THE DagStore SHALL return the maximum depth reached across all discovered dependents as a separate value
3. THE DagStore SHALL visit each dependent address at most once during the BFS traversal, even when a dependent is reachable via multiple paths in diamond-shaped graphs
4. WHEN transitive_dependents_with_depth is called for an address with no dependents, THE DagStore SHALL return an empty Vec and a max depth of 0
5. THE DagStore SHALL NOT include the queried root address itself in the transitive dependents result
6. THE BFS_Traversal SHALL have O(V+E) time complexity where V is the number of transitive dependents and E is the number of reverse edges traversed

### Requirement 3: Batch Cache Eviction

**User Story:** As a developer, I want to remove multiple cache entries in a single operation, so that cascade eviction is efficient and returns aggregate statistics.

#### Acceptance Criteria

1. WHEN remove_batch is called with a list of CAddr values, THE EvictableCache SHALL remove each address present in the cache and skip addresses not present, without returning an error for missing entries
2. WHEN remove_batch completes, THE EvictableCache SHALL return a tuple of (count_removed, bytes_reclaimed, removed_addrs) reflecting the entries that were actually present and removed
3. WHEN remove_batch removes entries, THE EvictableCache SHALL decrement its current_size by the total bytes of all removed entries
4. WHEN remove_batch is called with an empty list, THE EvictableCache SHALL return (0, 0, empty Vec) without modifying cache state

### Requirement 4: Synchronous Cascade Invalidation

**User Story:** As a developer, I want a synchronous cascade invalidation function that combines DAG traversal with batch eviction, so that the server can atomically invalidate a root and all its dependents.

#### Acceptance Criteria

1. WHEN CascadeInvalidator::invalidate is called with a root, dag, cache, policy, include_root, and detail_addrs parameters, THE CascadeInvalidator SHALL return an InvalidationResult containing root, evicted_count, traversed_count, max_depth, bytes_reclaimed, evicted_addrs, and duration
2. WHEN include_root is true, THE CascadeInvalidator SHALL include the root address in the eviction list alongside all transitive dependents
3. WHEN include_root is false, THE CascadeInvalidator SHALL evict only the transitive dependents without evicting the root address
4. WHEN detail_addrs is true, THE InvalidationResult SHALL contain the list of all evicted CAddr values in evicted_addrs
5. WHEN detail_addrs is false, THE InvalidationResult SHALL contain an empty evicted_addrs Vec to avoid unnecessary memory allocation
6. THE CascadeInvalidator SHALL measure elapsed wall-clock time from operation start to completion and store it in the InvalidationResult duration field
7. WHEN CascadePolicy is None and include_root is false, THE CascadeInvalidator SHALL return an InvalidationResult with all counts set to zero

### Requirement 5: Async Cascade Invalidation

**User Story:** As a developer, I want an async variant of cascade invalidation for the SharedCache path, so that the operation integrates with tokio-based async server code without blocking.

#### Acceptance Criteria

1. WHEN invalidate_cascade_async is called, THE CascadeInvalidator SHALL acquire the DAG read lock, collect all transitive dependents, and release the DAG lock before acquiring the cache write lock
2. WHEN invalidate_cascade_async completes, THE CascadeInvalidator SHALL return an InvalidationResult with the same semantic fields as the synchronous variant
3. THE async variant SHALL use the same lock ordering as the get() path (DAG read before cache write) to prevent deadlock
4. WHILE the DAG lock is released and the cache lock is not yet acquired, THE CascadeInvalidator SHALL treat the collected dependent list as a valid snapshot, accepting that DAG mutations between traversal and eviction may cause harmless over-eviction or acceptable under-eviction
5. WHEN the SharedCache remove_batch is called with a list of addresses, THE SharedCache SHALL acquire the write lock once and remove all specified entries in a single critical section, returning the count of entries actually removed

### Requirement 6: Lock Ordering and Deadlock Prevention

**User Story:** As a developer, I want cascade invalidation to follow a consistent lock ordering, so that concurrent get() and invalidate operations cannot deadlock.

#### Acceptance Criteria

1. THE synchronous CascadeInvalidator SHALL acquire the DAG read lock before acquiring the cache write lock, matching the lock order used by the get() path
2. THE async CascadeInvalidator SHALL release the DAG read lock before acquiring the cache write lock to prevent lock-ordering issues in the async context
3. WHILE both get() and cascade_invalidate() execute concurrently, THE system SHALL NOT enter a deadlock state regardless of thread scheduling
4. WHEN the DAG changes between the traversal phase and the eviction phase in the async path, THE CascadeInvalidator SHALL proceed with the stale snapshot without retrying or re-acquiring the DAG lock

### Requirement 7: gRPC CascadeInvalidate RPC

**User Story:** As a client, I want a dedicated CascadeInvalidate RPC with full policy control and detailed results, so that I can perform cascade invalidation remotely with fine-grained options.

#### Acceptance Criteria

1. WHEN a CascadeInvalidateRequest is received with addr, policy, include_root, and detail_addrs fields, THE DerivaService SHALL execute a cascade invalidation with the specified parameters and return a CascadeInvalidateResponse
2. THE CascadeInvalidateResponse SHALL contain evicted_count, traversed_count, max_depth, bytes_reclaimed, evicted_addrs (repeated bytes), and duration_micros fields
3. WHEN the policy field is "immediate", THE DerivaService SHALL use CascadePolicy::Immediate
4. WHEN the policy field is "dry_run" or "dryrun", THE DerivaService SHALL use CascadePolicy::DryRun
5. WHEN the policy field is "none", THE DerivaService SHALL use CascadePolicy::None
6. WHEN the policy field is empty or unrecognized, THE DerivaService SHALL default to CascadePolicy::Immediate
7. IF the addr field cannot be parsed as a valid CAddr, THEN THE DerivaService SHALL return an InvalidArgument gRPC status error

### Requirement 8: Enhanced Invalidate RPC (Backward Compatible)

**User Story:** As a client, I want the existing Invalidate RPC to optionally cascade, so that I can enable cascade behavior without switching to a new RPC endpoint.

#### Acceptance Criteria

1. WHEN the InvalidateRequest includes a cascade field set to true, THE DerivaService SHALL execute cascade invalidation with CascadePolicy::Immediate, include_root=true, and detail_addrs=false
2. WHEN the InvalidateRequest does not include the cascade field or sets it to false, THE DerivaService SHALL execute single-entry removal only (Phase 1 behavior)
3. THE InvalidateResponse SHALL include an evicted_count field reporting the total number of entries evicted (1 for single removal, N for cascade)
4. WHEN the cascade field is absent from the request (older clients), THE DerivaService SHALL treat it as false to maintain backward compatibility with existing clients

### Requirement 9: CLI Integration

**User Story:** As an operator, I want CLI flags for cascade invalidation, so that I can trigger and inspect cascade behavior from the command line.

#### Acceptance Criteria

1. WHEN the --cascade flag is provided on the invalidate command, THE CLI SHALL send a CascadeInvalidateRequest with CascadePolicy::Immediate
2. WHEN the --dry-run flag is provided on the invalidate command, THE CLI SHALL send a CascadeInvalidateRequest with CascadePolicy::DryRun
3. WHEN the --detail flag is provided on the invalidate command, THE CLI SHALL set detail_addrs=true and print each evicted address in the response
4. WHEN a cascade or dry-run invalidation completes, THE CLI SHALL print evicted count, traversed count, max depth, bytes reclaimed, and duration in microseconds
5. WHEN neither --cascade nor --dry-run is provided, THE CLI SHALL send a standard InvalidateRequest without cascade (backward-compatible single invalidation)

### Requirement 10: InvalidationResult Reporting

**User Story:** As a developer, I want detailed invalidation results, so that I can understand the scope and cost of each cascade operation.

#### Acceptance Criteria

1. THE InvalidationResult SHALL contain the root CAddr that triggered the invalidation
2. THE InvalidationResult SHALL contain evicted_count representing the number of cache entries actually removed (entries that were present in the cache)
3. THE InvalidationResult SHALL contain traversed_count representing the total number of transitive dependents found in the DAG (regardless of whether they were cached)
4. THE InvalidationResult SHALL contain max_depth representing the maximum BFS depth reached during traversal
5. THE InvalidationResult SHALL contain bytes_reclaimed representing the total bytes freed from the cache
6. THE InvalidationResult SHALL contain duration representing the wall-clock time of the full cascade operation
7. THE InvalidationResult SHALL provide an empty() constructor that returns a result with all counts set to zero and duration set to Duration::ZERO

### Requirement 11: Metrics and Observability

**User Story:** As an operator, I want cascade invalidation depth tracked as a metric, so that I can monitor invalidation blast radius over time.

#### Acceptance Criteria

1. WHEN a cascade invalidation completes with CascadePolicy::Immediate, THE system SHALL record the max_depth value in the CASCADE_DEPTH histogram metric
2. WHEN a cascade invalidation completes with CascadePolicy::DryRun, THE system SHALL record the max_depth value in the CASCADE_DEPTH histogram metric
3. WHEN CascadePolicy is None, THE system SHALL NOT record a CASCADE_DEPTH metric observation

### Requirement 12: SharedCache Containment Check

**User Story:** As a developer, I want to check whether an address is present in the SharedCache without modifying it, so that the DryRun policy can report cached entries without evicting them.

#### Acceptance Criteria

1. WHEN contains is called on the SharedCache with an address, THE SharedCache SHALL acquire a read lock and return true if the address is present in the cache, false otherwise
2. THE contains operation SHALL NOT update access timestamps or LRU ordering for the queried entry
3. THE contains operation SHALL NOT require a write lock, allowing concurrent containment checks with other read operations
