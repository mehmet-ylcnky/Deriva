# Deriva — Implementation Plan

> Development roadmap from Phase 1 (complete) through Phase 4.

---

## Phase 1: Single-Node Core ✅ COMPLETE

**Goal:** Prove that computation-addressed storage works as a single-node system.

**Result:** 244 tests, 6 crates, 0 clippy warnings, full gRPC API, CLI client.

| Section | What | Tests |
|---------|------|-------|
| 1.1 | Scaffolding — workspace, 6 crates, DerivaError, proto stub | 8 |
| 1.2 | Core Types — CAddr, FunctionId, Value, Recipe, DataRef | 43 |
| 1.3 | DAG Store — cycle detection, topo sort, dependents | 31 |
| 1.4 | Compute Engine — ComputeFunction trait, registry, builtins, Executor | 44 |
| 1.5 | EvictableCache with cost-aware eviction | 27 |
| 1.6 | SledRecipeStore, BlobStore with sharding, StorageBackend | 28 |
| 1.7 | Full tonic gRPC service (6 RPCs) | 24 |
| 1.8 | CLI client — put, recipe, get, resolve, invalidate, status | 14 |
| 1.9 | Integration tests, bug fixes (RepeatFn string params, sled lock restart) | 25 |

---

## Phase 2: Robustness

**Goal:** Make the single-node system production-grade.

### 2.1 Persistent DAG Store
- Back the in-memory DAG with sled so dependency graph survives restarts
- On startup: load DAG from sled, rebuild in-memory adjacency lists
- Tests: restart with complex DAG, verify dependents/inputs queries still work

### 2.2 Async Compute Engine
- Replace synchronous executor with async Tokio tasks
- Parallel materialization of independent DAG branches
- When a recipe has two unmaterialized inputs, compute both concurrently
- Tests: diamond DAG parallel resolution, verify wall-clock improvement

### 2.3 Parallel Materialization
- Critical path scheduling: prioritize the longest chain
- Concurrency limit to prevent resource exhaustion
- Tests: wide fan-out (1→100), deep chain, mixed DAG shapes

### 2.4 Verification Mode
- Dual-compute: execute each function twice, compare hashes
- If hashes differ → flag function as non-deterministic, reject result
- Configurable: per-request or global toggle
- Tests: deterministic function passes, inject non-deterministic function fails

### 2.5 Observability
- Structured logging via `tracing` crate
- Prometheus-compatible metrics endpoint (new gRPC RPC or HTTP sidecar)
- Key metrics: cache hit rate, materialization latency by function, DAG depth, eviction rate
- Tests: verify metrics counters increment correctly

---

## Phase 3: Distribution

**Goal:** Scale to multiple nodes with smart compute routing.

### 3.1 Node Discovery — SWIM Gossip
- Implement SWIM protocol for membership and failure detection
- Gossip metadata: cache contents, storage capacity, compute load
- Configurable probe interval and suspicion timeout
- Tests: node join/leave detection, metadata convergence

### 3.2 Recipe Replication
- Replicate recipes to ALL nodes (tiny, critical data)
- Synchronous replication: PutRecipe returns only after all nodes acknowledge
- Tests: recipe available on all nodes after put, survives single node failure

### 3.3 Leaf Data Replication
- Configurable replication factor (default 3)
- Consistent hashing for placement decisions
- Tests: leaf available on N nodes, survives (N-1) node failures

### 3.4 Cache Placement Policy
- Cached materializations are NOT replicated (recomputable)
- Cache placement informed by access patterns and compute routing
- Tests: eviction on one node, transparent recomputation on access

### 3.5 Locality-Aware Compute Routing
- On cache miss: check gossip metadata for nodes with inputs cached
- Route computation to node with most input bytes already local
- Minimize data transfer across network
- Tests: verify computation routed to data-local node, measure bytes transferred

### 3.6 Distributed Get Protocol
- Client sends `Get(addr)` to any node
- Node checks local cache → checks gossip for remote cache → routes computation
- Streaming response back to client regardless of which node computed
- Tests: end-to-end distributed resolution, multi-hop scenarios

### 3.7 Consistency Model
- Recipes: strong consistency (all-node replication)
- Leaf data: tunable consistency (replication factor)
- Cache: eventual consistency (best-effort, recomputable)
- Tests: read-after-write for recipes and leaves, eventual consistency for cache

---

## Phase 4: Advanced Features

**Goal:** Expand to new use cases with WASM, FUSE, and partial reads.

### 4.1 WASM Function Plugins
- Users compile custom functions to WASM, register with Deriva
- Wasmtime runtime with determinism guarantees
- Sandbox: no network, no filesystem, no clock — non-determinism structurally impossible
- Resource limits: memory cap, instruction count, timeout
- Tests: WASM function execution, sandbox escape attempts blocked, resource limits enforced

### 4.2 FUSE Filesystem Mount
- Mount Deriva as local filesystem via FUSE
- CAddr → path mapping: `/deriva/{first_2_hex}/{full_hex}/`
- `open` → `get()`, `read` → stream, `stat` → metadata
- Lazy materialization through standard file operations
- Tests: `cat`, `ls`, `stat` on mounted Deriva, large file streaming

### 4.3 Chunk-Level Partial Reads
- Large outputs split into fixed-size chunks, each with own CAddr
- Range read resolves to specific chunks, only those are materialized
- Configurable chunk size (default 1MB)
- Tests: partial read of large derived result, verify only needed chunks computed

### 4.4 Mutable References
- Named pointers that can be rebound to new leaf CAddrs
- Rebinding triggers cascade invalidation via DAG dependents query
- All downstream cached materializations evicted
- Tests: rebind reference, verify all dependents invalidated, recomputation on next access

### 4.5 REST API
- HTTP/JSON alternative to gRPC for broader client compatibility
- Same operations: put, recipe, get, resolve, invalidate, status
- Tests: mirror all gRPC integration tests over REST

---

## Development Principles

- **Minimal code** — write only what's needed, no speculative abstractions
- **Test-driven** — every section adds tests, cumulative count grows monotonically
- **Clippy clean** — zero warnings at all times
- **Incremental commits** — one commit per section, each leaves the system in a working state
- **Backward compatible** — new phases don't break existing APIs or stored data
