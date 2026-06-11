# Requirements Document

## Introduction

The Persistent DAG feature replaces the in-memory `DagStore` with a sled-backed `PersistentDag` so the dependency graph survives process restarts without requiring a full rebuild from the recipe store. This eliminates O(N) startup time, reduces memory usage by ~75%, and ensures durability of graph state across crashes.

## Glossary

- **PersistentDag**: The sled-backed struct that stores forward and reverse adjacency lists persistently, replacing the in-memory DagStore
- **CAddr**: A content address (32-byte hash) that uniquely identifies a leaf value or derived recipe in the system
- **Recipe**: A computation descriptor containing a function identifier, input addresses, and parameters; its address is the hash of its contents
- **Forward_Tree**: The sled tree (`dag_forward`) mapping each recipe address to its ordered list of input addresses
- **Reverse_Tree**: The sled tree (`dag_reverse`) mapping each address to the list of recipes that depend on it
- **DagAccess**: The trait abstraction over DAG operations, implemented by both in-memory DagStore and PersistentDag
- **Sled_Transaction**: An atomic multi-tree write operation in sled that ensures all-or-nothing semantics across forward and reverse trees
- **LRU_Cache**: Least-recently-used in-memory cache layered over sled reads for hot-path acceleration
- **StorageBackend**: The unified storage layer that owns the recipe store, blob store, and PersistentDag sharing a single sled database instance
- **ServerState**: The top-level application state holding the PersistentDag, cache, function registry, and storage backend
- **Topological_Sort**: An ordering of DAG nodes where each node appears after all its inputs, used for materialization scheduling
- **BFS_Traversal**: Breadth-first search used to discover transitive dependents of a node

## Requirements

### Requirement 1: Persistent Storage of DAG Adjacency Lists

**User Story:** As a system operator, I want the dependency graph stored persistently in sled, so that the graph survives process restarts without requiring reconstruction.

#### Acceptance Criteria

1. WHEN the PersistentDag opens a sled database, THE PersistentDag SHALL create or open two named sled trees: `dag_forward` and `dag_reverse`
2. WHEN a Recipe is inserted and no entry for the Recipe address exists in the Forward_Tree, THE PersistentDag SHALL store the mapping from the Recipe address to its input addresses in the Forward_Tree; IF an entry for the Recipe address already exists in the Forward_Tree, THEN THE PersistentDag SHALL return the existing address without modifying either tree
3. WHEN a Recipe is inserted, THE PersistentDag SHALL store the Recipe address as a dependent in the Reverse_Tree entry for each of its input addresses, without creating duplicate entries for the same dependent address
4. WHEN the process is restarted after a successful insert, THE PersistentDag SHALL retain all previously inserted adjacency data in both Forward_Tree and Reverse_Tree such that queries return identical results to those before the restart
5. THE PersistentDag SHALL serialize adjacency lists using bincode encoding for both Forward_Tree values and Reverse_Tree values
6. WHEN a Recipe is inserted, THE PersistentDag SHALL write the Forward_Tree entry and all corresponding Reverse_Tree entries within a single sled transaction so that either all edges are persisted or none are
7. IF a Recipe's input list contains the Recipe's own address, THEN THE PersistentDag SHALL reject the insertion with an error indicating a self-cycle was detected

### Requirement 2: O(1) Startup Time

**User Story:** As a system operator, I want the DAG to be ready immediately on startup, so that service availability is not blocked by graph reconstruction.

#### Acceptance Criteria

1. WHEN the PersistentDag is opened on an existing database with populated DAG trees, THE PersistentDag SHALL complete initialization by opening sled trees without iterating or deserializing stored entries
2. THE PersistentDag SHALL complete the open operation within 10 milliseconds regardless of the number of stored recipes, from 0 to 1,000,000
3. WHILE the DAG trees are already populated from a prior run, THE PersistentDag SHALL NOT call any rebuild, migration, or full-iteration operation during startup
4. IF the sled database cannot be opened or a required tree cannot be created, THEN THE PersistentDag SHALL return an error indicating the storage failure without blocking indefinitely

### Requirement 3: Atomic Write Operations

**User Story:** As a developer, I want insert operations to be atomic across both sled trees, so that forward and reverse edges remain consistent even after a crash.

#### Acceptance Criteria

1. WHEN a Recipe is inserted, THE PersistentDag SHALL write the forward edge and all corresponding reverse edge updates within a single sled multi-tree transaction that spans both the dag_forward and dag_reverse trees
2. IF the sled transaction fails, THEN THE PersistentDag SHALL leave both trees unchanged, leave any in-memory caches unmodified, and return an error to the caller
3. WHEN the sled database is reopened after a process crash that occurred during an insert, THE PersistentDag SHALL reflect either the complete transaction (forward edge and all reverse edges written) or none of the writes, with no partial state observable via queries
4. WHEN a Recipe is inserted with a content address that already exists in the dag_forward tree, THE PersistentDag SHALL return the existing address without performing any writes (idempotent insert)

### Requirement 4: Idempotent Insert

**User Story:** As a developer, I want inserting the same recipe twice to be a no-op, so that duplicate submissions do not corrupt the graph or cause errors.

#### Acceptance Criteria

1. WHEN a Recipe is inserted whose address already exists in the Forward_Tree, THE PersistentDag SHALL return the existing address without modifying the Forward_Tree or the Reverse_Tree
2. WHEN a Recipe is inserted whose address already exists in the Forward_Tree, THE PersistentDag SHALL NOT return an error
3. WHEN a Recipe is inserted multiple times, THE PersistentDag SHALL maintain a Forward_Tree entry count equal to the number of distinct recipe addresses inserted
4. WHEN two threads concurrently insert the same Recipe for the first time, THE PersistentDag SHALL store exactly one Forward_Tree entry and one set of Reverse_Tree entries for that address

### Requirement 5: Self-Cycle Detection

**User Story:** As a developer, I want the DAG to reject recipes that reference themselves as inputs, so that the graph remains acyclic.

#### Acceptance Criteria

1. WHEN a Recipe lists its own computed address among its inputs, THE PersistentDag SHALL reject the insert and return a CycleDetected error
2. WHEN a self-referencing Recipe is rejected, THE PersistentDag SHALL leave both trees unmodified
3. THE PersistentDag SHALL perform the self-cycle check before initiating the sled transaction, avoiding unnecessary I/O for invalid recipes

### Requirement 6: Forward Query (Inputs)

**User Story:** As a developer, I want to query the input addresses of a recipe, so that I can determine what data a computation depends on.

#### Acceptance Criteria

1. WHEN inputs are queried for an address present in the Forward_Tree, THE PersistentDag SHALL return Some containing the list of input CAddr values stored for that address
2. WHEN inputs are queried for an address not present in the Forward_Tree (including leaf addresses that were never inserted as recipes), THE PersistentDag SHALL return None
3. THE PersistentDag SHALL return inputs in the same positional order as the Vec<CAddr> originally provided during insert
4. IF a storage I/O error or deserialization error occurs during an inputs query, THEN THE PersistentDag SHALL return an Err result indicating the failure reason
5. WHEN inputs are queried after the PersistentDag has been closed and reopened against the same sled database, THE PersistentDag SHALL return the same ordered input list as was returned prior to close

### Requirement 7: Reverse Query (Direct Dependents)

**User Story:** As a developer, I want to query which recipes directly depend on a given address, so that I can identify what computations are affected by a change.

#### Acceptance Criteria

1. WHEN direct_dependents is queried for an address, THE PersistentDag SHALL return all recipe addresses that list the queried address as an input, with no guaranteed ordering of the returned entries
2. WHEN direct_dependents is queried for an address that has no dependents or does not exist in the DAG, THE PersistentDag SHALL return an empty list
3. THE PersistentDag SHALL NOT include duplicate entries in the direct dependents list, enforced at insert time by checking existing reverse-index entries before appending
4. IF a storage or deserialization error occurs during a direct_dependents lookup, THEN THE PersistentDag SHALL return an empty list without propagating the error
5. WHEN a recipe is removed from the DAG, THE PersistentDag SHALL remove that recipe's address from the direct dependents list of each of its inputs

### Requirement 8: Transitive Dependents (BFS)

**User Story:** As a developer, I want to discover all transitive dependents of an address, so that I can identify the full set of computations affected by an invalidation.

#### Acceptance Criteria

1. WHEN transitive_dependents is queried for an address, THE PersistentDag SHALL return all addresses reachable via BFS_Traversal through the Reverse_Tree, in BFS level-order (direct dependents before their dependents)
2. THE PersistentDag SHALL NOT include the queried address itself in the transitive dependents result
3. THE PersistentDag SHALL visit each dependent address at most once during the traversal, even when reachable via multiple paths (diamond graphs)
4. WHEN transitive_dependents is queried for an address with no dependents or an address not present in the DAG, THE PersistentDag SHALL return an empty list

### Requirement 9: Topological Resolution Order

**User Story:** As a developer, I want to get a materialization order for a recipe and its transitive inputs, so that I can schedule computations in dependency-respecting order.

#### Acceptance Criteria

1. WHEN resolve_order is queried for a recipe address, THE PersistentDag SHALL return a list of recipe addresses in Topological_Sort order, with the queried address itself as the final element of the list
2. THE PersistentDag SHALL place every recipe before any recipe that depends on it in the resolution order
3. THE PersistentDag SHALL include only addresses that have entries in the Forward_Tree (recipe nodes, not leaf nodes), and SHALL include each address at most once in the result
4. WHEN resolve_order is queried for an address that has no entry in the Forward_Tree, THE PersistentDag SHALL return an empty list

### Requirement 10: Depth Calculation

**User Story:** As a developer, I want to compute the depth of a node in the DAG, so that I can reason about materialization complexity and parallelism opportunities.

#### Acceptance Criteria

1. WHEN depth is queried for a leaf address (not present in Forward_Tree), THE PersistentDag SHALL return 0
2. WHEN depth is queried for a recipe address with one or more inputs, THE PersistentDag SHALL return 1 plus the maximum depth among its inputs
3. WHEN depth is queried for a recipe address with an empty input list, THE PersistentDag SHALL return 1
4. THE PersistentDag SHALL use memoization via a per-call cache to avoid redundant recomputation of depth values within a single depth query invocation

### Requirement 11: Recipe Removal

**User Story:** As a developer, I want to remove a recipe from the DAG, so that deleted or invalidated computations are cleaned up.

#### Acceptance Criteria

1. WHEN remove is called with an address that exists in the Forward_Tree, THE PersistentDag SHALL delete the forward edge entry for that address from the Forward_Tree
2. WHEN remove is called with an address that exists in the Forward_Tree, THE PersistentDag SHALL read the inputs from the forward edge before deletion and remove the address from the Reverse_Tree entry of each of those inputs
3. WHEN remove is called with an address that exists in the Forward_Tree, THE PersistentDag SHALL return true
4. WHEN remove is called with an address that does not exist in the Forward_Tree, THE PersistentDag SHALL return false without modifying any tree
5. WHEN a Reverse_Tree entry becomes an empty list after removing the last dependent address, THE PersistentDag SHALL delete that Reverse_Tree key entirely rather than storing an empty list
6. WHEN remove is called for an address that other recipes reference as an input, THE PersistentDag SHALL NOT modify the Forward_Tree entries of those dependent recipes

### Requirement 12: Thread-Safe Concurrent Access

**User Story:** As a developer, I want multiple threads to read and write the DAG concurrently, so that the server can handle parallel requests without external locking.

#### Acceptance Criteria

1. WHEN at least 8 threads concurrently insert distinct recipes sharing a common input, THE PersistentDag SHALL complete all inserts such that every inserted recipe is retrievable via contains and the total len equals the number of distinct recipes inserted
2. WHILE multiple threads perform concurrent read operations (inputs, direct_dependents, resolve_order), THE PersistentDag SHALL return results consistent with a valid serialization of prior inserts without requiring the caller to hold an external lock
3. THE PersistentDag SHALL rely on sled's internal locking and NOT require an external RwLock wrapper for concurrent access
4. WHEN at least 8 threads concurrently insert recipes that share the same input address, THE PersistentDag SHALL record all inserted recipe addresses in the shared input's direct_dependents result with zero lost entries
5. THE PersistentDag SHALL implement Clone, Send, and Sync so that instances can be shared across threads via Arc or direct Clone without additional synchronization wrappers

### Requirement 13: LRU Cache for Read Acceleration

**User Story:** As a developer, I want hot adjacency lists cached in memory, so that repeated reads avoid sled disk I/O.

#### Acceptance Criteria

1. THE PersistentDag SHALL maintain an LRU_Cache for forward adjacency lookups with a configurable maximum size defaulting to 10,000 entries
2. THE PersistentDag SHALL maintain an LRU_Cache for reverse adjacency lookups with a configurable maximum size defaulting to 10,000 entries
3. WHEN a cached entry is available for an inputs or direct_dependents query, THE PersistentDag SHALL return it without reading from sled
4. WHEN an insert modifies adjacency data, THE PersistentDag SHALL populate the forward cache for the new entry and invalidate reverse cache entries for each affected input
5. WHEN a remove modifies adjacency data, THE PersistentDag SHALL evict the forward cache entry for the removed address and invalidate reverse cache entries for each affected input

### Requirement 14: Migration from In-Memory DagStore

**User Story:** As a system operator, I want existing recipe data to be automatically migrated into the persistent DAG on first run, so that upgrading does not require manual intervention.

#### Acceptance Criteria

1. WHEN the PersistentDag Forward_Tree is empty and the SledRecipeStore contains one or more recipes, THE StorageBackend SHALL iterate all recipes and insert each into the PersistentDag
2. WHEN migration completes successfully, THE StorageBackend SHALL flush the PersistentDag exactly once to ensure all migrated edges are durable
3. WHEN the PersistentDag Forward_Tree already contains one or more entries, THE StorageBackend SHALL skip migration entirely without iterating the recipe store
4. IF a recipe fails to insert during migration (e.g., deserialization error), THEN THE StorageBackend SHALL skip that recipe, continue migrating remaining recipes, and log the failure

### Requirement 15: Consistency Repair on Startup

**User Story:** As a system operator, I want crash-induced inconsistencies between the recipe store and the DAG to be detected and repaired on startup, so that the system self-heals.

#### Acceptance Criteria

1. WHEN the system starts, THE StorageBackend SHALL iterate all entries in the SledRecipeStore and identify each recipe whose CAddr is not present in the PersistentDag forward index
2. WHEN inconsistent recipes are detected, THE StorageBackend SHALL insert each missing recipe into the PersistentDag using the same insert logic as normal operation (creating forward and reverse edges)
3. WHEN all repairs complete successfully, THE StorageBackend SHALL flush the PersistentDag exactly once after the final insert
4. WHEN no inconsistencies exist, THE StorageBackend SHALL complete the consistency check without modifying the PersistentDag or invoking flush
5. IF a recipe cannot be deserialized or its DAG insert fails during repair, THEN THE StorageBackend SHALL skip that recipe, continue repairing remaining entries, and return an error indicating the number of recipes that failed repair
6. WHEN at least one recipe is repaired successfully, THE StorageBackend SHALL emit a log entry indicating the count of repaired recipes

### Requirement 16: DagAccess Trait Abstraction

**User Story:** As a developer, I want a common trait for DAG operations, so that production code uses PersistentDag while tests can use the in-memory DagStore.

#### Acceptance Criteria

1. THE DagAccess trait SHALL define methods: insert(&self, recipe: &Recipe) -> Result<CAddr>, contains(&self, addr: &CAddr) -> bool, inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>>, direct_dependents(&self, addr: &CAddr) -> Vec<CAddr>, transitive_dependents(&self, addr: &CAddr) -> Vec<CAddr>, resolve_order(&self, addr: &CAddr) -> Vec<CAddr>, depth(&self, addr: &CAddr) -> u32, len(&self) -> usize, is_empty(&self) -> bool, and remove(&self, addr: &CAddr) -> Result<bool>
2. THE PersistentDag SHALL implement the DagAccess trait with all methods behaving identically to its inherent implementations
3. THE in-memory DagStore SHALL implement the DagAccess trait using interior mutability (e.g., RwLock or Mutex) so all methods take &self
4. WHEN both implementations are given the same sequence of insert and remove operations, THEN both SHALL produce equivalent query results for inputs, direct_dependents, transitive_dependents, resolve_order, and depth

### Requirement 17: Integration with StorageBackend

**User Story:** As a developer, I want the PersistentDag to share the same sled database instance as the recipe store, so that the system uses a single storage engine.

#### Acceptance Criteria

1. WHEN StorageBackend is opened with a root path, THE StorageBackend SHALL open exactly one sled::Db instance and pass that same instance to both SledRecipeStore and PersistentDag during construction
2. THE PersistentDag SHALL store its adjacency data in two named trees called "dag_forward" and "dag_reverse", which are distinct from the default tree used by SledRecipeStore for recipe bytes
3. THE ServerState SHALL hold the PersistentDag directly without an RwLock wrapper, relying on sled's inherent thread-safety for concurrent access
4. IF sled::Db fails to open or a named tree fails to open, THEN THE StorageBackend SHALL return an error and not construct any partial state
5. WHEN StorageBackend is closed and reopened at the same root path, THE PersistentDag SHALL recover all previously inserted DAG edges from the persisted "dag_forward" and "dag_reverse" trees

### Requirement 18: Memory Efficiency

**User Story:** As a system operator, I want the persistent DAG to use significantly less memory than the in-memory approach, so that the system can scale to larger datasets.

#### Acceptance Criteria

1. THE PersistentDag SHALL store only adjacency lists (forward and reverse edges) in its sled trees, with each edge entry consisting of CAddr references (32 bytes each), not full Recipe objects
2. WHEN storing 10,000 or more recipes, THE PersistentDag SHALL achieve at least 70% reduction in process heap memory attributed to DAG data compared to the in-memory DagStore storing the same recipes
3. THE PersistentDag SHALL limit its in-memory heap usage to the configured LRU_Cache entry counts (default 10,000 forward + 10,000 reverse) multiplied by entry size, plus at most 1 MB of fixed overhead for sled page buffers, tree handles, and synchronization structures
4. IF the LRU_Cache is not yet implemented, THEN THE PersistentDag SHALL consume no more than 1 MB of heap memory for DAG data beyond sled's internal page cache, regardless of the number of stored recipes
