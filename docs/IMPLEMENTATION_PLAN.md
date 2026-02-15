# Deriva — Implementation Plan

> Development roadmap from Phase 1 (complete) through Phase 4.
>
> **Core invariant:** Determinism is the backbone. The same recipe with the same inputs
> MUST produce the same output, the same CAddr, on any node, at any time, on any architecture.
> Every design decision flows from this invariant.

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

## Phase 2: Robustness ✅ COMPLETE

**Goal:** Make the single-node system production-grade.

**Result:** 8 sections, 11,122 total lines of detailed blueprints.

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

### 2.6 Cascade Invalidation Engine
- When a leaf is updated, automatically invalidate all transitive dependents from cache
- BFS/DFS traversal of reverse DAG edges to find all downstream CAddrs
- Batch eviction with atomic counter tracking
- `InvalidationResult` with count of evicted entries and traversal depth
- Configurable: `CascadePolicy` (None, Immediate, Deferred with batching)
- Tests: diamond DAG cascade, deep chain cascade, no-op when no dependents cached

### 2.7 Streaming Materialization
- Chunked compute pipeline: functions yield `Stream<Item = Bytes>` instead of `Vec<u8>`
- `StreamingComputeFunction` trait alongside existing `ComputeFunction`
- gRPC response stream starts before full materialization completes
- Backpressure via bounded channels between compute and gRPC stream
- Configurable chunk size (default 64KB)
- Tests: 100MB+ output streams without OOM, backpressure pauses compute, chunk ordering preserved

### 2.8 Garbage Collection
- Mark-and-sweep GC for orphaned blobs and unreferenced recipes
- Root set: all recipes reachable from current DAG + all leaves referenced by recipes
- Sweep: remove blobs and recipes not in root set
- `GcResult` with counts of swept blobs, recipes, bytes reclaimed
- Safe mode: dry-run that reports what would be collected without deleting
- Tests: orphan detection after recipe removal, GC preserves reachable data, concurrent GC + reads safe

---

## Phase 3: Distribution (§3.1–§3.7 ✅ COMPLETE, §3.8–§3.17 planned)

**Goal:** Scale to multiple nodes with smart compute routing.

**Result (core):** 7 sections, 11,125 total lines of detailed blueprints.

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

### 3.8 Cluster Bootstrap & Discovery
- Seed node list for initial cluster formation
- DNS-based discovery as alternative (SRV records)
- Bootstrap state machine: discover → join → sync recipes → sync ring → ready
- Graceful shutdown: drain in-flight requests, transfer hinted handoffs, leave ring
- Node identity persistence across restarts (node ID stored on disk)
- Tests: cold start 3-node cluster from seed list, node restart rejoins with same identity, DNS discovery resolves peers

### 3.9 Cluster-Wide Garbage Collection
- Distributed mark-and-sweep protocol coordinated by leader election (leader = lowest NodeId)
- Phase 1 (mark): leader broadcasts GC intent, each node reports local root set (referenced CAddrs)
- Phase 2 (sweep): leader computes global root set (union), broadcasts sweep list
- Each node deletes unreferenced blobs/recipes from local storage
- Quorum agreement before sweep (majority must confirm root set)
- Safe mode: dry-run across cluster, report what would be collected
- Distributed GC lock prevents concurrent GC runs
- Tests: orphan blob collected after recipe deletion, GC safe during concurrent writes, leader failover mid-GC restarts protocol

### 3.10 Data Migration & Rebalancing
- When nodes join/leave, consistent hash ring changes ownership of key ranges
- Rebalancing transfers data from old owners to new owners
- Streaming transfer: source node streams blobs to destination in priority order (most accessed first)
- Throttling: configurable bandwidth limit to avoid saturating network during rebalance
- Progress tracking: rebalance status per key range (pending, transferring, complete)
- Handoff completion: old owner deletes transferred data after new owner confirms
- Tests: add node to 3-node cluster, verify data migrates to new owner, remove node and verify replicas rebuilt

### 3.11 Backpressure & Admission Control
- Per-node request queue with configurable depth limit (default 1024)
- Load shedding: reject new requests with `RESOURCE_EXHAUSTED` when queue full
- Priority levels: client reads > client writes > internal replication > GC
- Adaptive concurrency limit based on measured latency (TCP Vegas-style)
- Client retry budget: server returns `retry-after` header, client respects backoff
- Memory pressure detection: shed load when heap usage exceeds 80% of configured max
- Tests: queue overflow returns proper error, priority ordering under load, memory pressure triggers shedding

### 3.12 Mutual TLS & Node Authentication
- All inter-node gRPC channels use mTLS with X.509 certificates
- Certificate-based node identity: CN = node name, SAN = node address
- Shared CA for cluster: nodes reject connections from unknown CAs
- Certificate rotation without downtime (watch file for changes, hot-reload)
- Client-to-server TLS: optional, configurable (plaintext for dev, TLS for prod)
- Authorization: node certificates grant full cluster access (no per-node ACLs in Phase 3)
- Tests: unauthenticated node rejected, valid cert accepted, expired cert rejected, hot-reload works

### 3.13 Connection Pooling & Channel Management
- Persistent gRPC channel pool: one channel per peer, lazily created
- Health checking: periodic ping, mark channel unhealthy after 3 failures
- Warm-up: pre-connect to all known peers on startup
- Idle timeout: close channels unused for 5 minutes
- Reconnection with exponential backoff (100ms → 200ms → 400ms → ... → 10s cap)
- Channel reuse across all internal RPCs (FetchValue, RouteCompute, Replicate)
- Tests: channel reuse across multiple RPCs, unhealthy channel replaced, idle channel closed

### 3.14 Batch Operations
- `BatchGet(addrs[])`: scatter requests across ring, gather results, stream back
- `BatchPutLeaf(data[])`: scatter writes to replica sets, gather acks per consistency level
- Parallel fan-out with configurable max concurrency (default 32)
- Partial failure handling: return per-address success/error map
- Streaming batch response: results arrive as they complete (not all-or-nothing)
- Tests: batch get 100 addresses across 3 nodes, batch put with partial failure, streaming order

### 3.15 Admin API & Cluster Introspection
- HTTP API (separate port, default 9090) for operational visibility
- `GET /cluster/members` — current membership with state, load, uptime
- `GET /cluster/ring` — consistent hash ring visualization (key ranges per node)
- `GET /cluster/stats` — aggregate cache hit rate, replication lag, GC status
- `GET /node/config` — current node configuration (consistency levels, timeouts)
- `POST /node/drain` — initiate graceful drain for maintenance
- `GET /node/health` — liveness + readiness probes (Kubernetes-compatible)
- JSON responses, no authentication (bind to localhost or internal network only)
- Tests: membership endpoint reflects SWIM state, ring endpoint matches hash ring, drain stops accepting new requests

### 3.16 Request Hedging & Speculative Execution
- On remote fetch: send request to 2 replicas simultaneously, take first response
- Cancel the slower request when first response arrives
- Configurable hedge delay: wait N ms before sending second request (default 5ms)
- Only hedge read operations (gets, fetches) — never hedge writes
- Hedge budget: max 10% of requests are hedged (avoid doubling load)
- Integrates with circuit breaker: don't hedge to nodes with open circuits
- Tests: hedge reduces p99 latency, cancelled request doesn't consume resources, hedge budget limits extra load

### 3.17 Rolling Upgrades & Version Negotiation
- Internal RPC version field: nodes include protocol version in every request
- Version compatibility matrix: node accepts requests from version N and N-1
- Graceful drain before upgrade: node stops accepting new requests, finishes in-flight
- Recipe function versioning: FunctionId includes version, old and new coexist
- Feature flags: new features disabled until all nodes upgraded (cluster-wide min version)
- Upgrade status endpoint: `GET /admin/upgrade` shows per-node versions
- Tests: mixed-version cluster handles RPCs correctly, old node rejects too-new protocol, feature flag gates new behavior

---

## Phase 4: Determinism Backbone & Advanced Features

**Goal:** Establish determinism as the structurally enforced, cryptographically verifiable backbone of Deriva, then expand to new use cases.

Phase 4 is split into two tracks:
- **§4.1–§4.5 Determinism Backbone** — make non-determinism structurally impossible, provable, and auditable
- **§4.6–§4.10 Advanced Features** — WASM plugins, FUSE mount, partial reads, mutable refs, REST API

---

### Track A: Determinism Backbone (§4.1–§4.5)

### 4.1 Canonical Serialization & Stable Hashing
- **Problem:** `bincode` is not version-stable. A bincode format change across Deriva versions means the same `Recipe` produces different `canonical_bytes()` → different `CAddr`. This silently fractures the entire address space. The system's core invariant (`same recipe = same address`) depends on serialization stability, but nothing enforces it.
- **Solution:** Define a Deriva Canonical Format (DCF) — a hand-written, versioned, byte-level serialization that is frozen per format version and independent of any serde framework.
- DCF spec: deterministic field ordering (sorted by field tag), fixed-width integers (big-endian), length-prefixed strings (u32 LE length + UTF-8 bytes), sorted map entries (lexicographic by key bytes)
- Format version byte prefix: `DCF\x01` magic header. Old formats remain decodable forever.
- `CanonicalEncoder` / `CanonicalDecoder` traits replacing `bincode::serialize` in `Recipe::canonical_bytes()` and `CAddr::from_bytes()`
- Migration: detect old-format recipes on read (no magic header), re-encode to DCF, store updated version. Dual-hash verification during migration window.
- Property tests: encode/decode roundtrip for all core types, cross-version compatibility (serialize with v1, deserialize with v2), canonical form uniqueness (two semantically equal values produce identical bytes)
- Golden file tests: known Recipe → known CAddr, checked into repo. CI fails if canonical encoding changes.
- Tests: format stability across versions, migration from bincode to DCF, golden hash vectors, fuzz encode/decode

### 4.2 Deterministic Compute Scheduling
- **Problem:** §2.2–§2.3 introduced parallel materialization. When a recipe has inputs `[A, B, C]` and all three are computed concurrently, the `ComputeFunction::execute()` receives `inputs: Vec<Bytes>` — but if the parallel scheduler resolves them in different orders on different runs (B finishes before A), and the function is sensitive to input ordering, the output changes. The current system relies on convention (inputs ordered by recipe definition), but nothing enforces this at the scheduler level.
- **Solution:** Deterministic input assembly — the scheduler MUST deliver inputs to `execute()` in exactly the order specified by `Recipe.inputs`, regardless of which parallel task completes first.
- Input assembly barrier: all inputs collected into a `Vec<Option<Bytes>>` indexed by position, function called only when all slots filled, assembled in recipe-defined order
- Parallel execution proof: each materialization records the input CAddrs it consumed and the order, stored as metadata alongside the cached result
- Scheduler determinism test harness: inject artificial random delays into input resolution, verify output CAddr is identical across 1000 runs
- Idempotent re-execution guarantee: if the same recipe is materialized on two different nodes with different parallelism levels (node A has 4 cores, node B has 64), the result MUST be identical
- Configurable execution mode: `Parallel` (default, with ordering barrier), `Sequential` (inputs resolved one at a time in order, for debugging), `Deterministic-Replay` (replay a recorded execution trace)
- Tests: diamond DAG with random delays produces stable CAddr, sequential vs parallel mode produces identical output, cross-node materialization equivalence, input ordering invariant under fan-out

### 4.3 Reproducibility Proofs & Audit Trail
- **Problem:** A client receives a value for CAddr `0xabc`. How do they know it was correctly derived? They must trust the server. There's no way to independently verify that the cached result matches what the recipe would produce, without re-executing the entire computation. For regulated industries (finance, healthcare, scientific computing), this is insufficient.
- **Solution:** Merkle proof chains — every derived value carries a `DerivationProof` that cryptographically links the output to its inputs through the recipe graph, enabling third-party verification without re-execution.
- `DerivationProof` structure: `{ recipe_addr: CAddr, input_proofs: Vec<InputProof>, output_addr: CAddr, output_hash: blake3, compute_node_id: NodeId, timestamp: u64, signature: Ed25519Signature }`
- `InputProof`: either `Leaf { addr, hash }` or `Derived { addr, proof: Box<DerivationProof> }` — recursive Merkle chain down to leaves
- Proof generation: automatic during materialization. Executor records proof as side-effect.
- Proof verification: `verify_proof(proof, trusted_leaf_set) → bool` — walks the proof tree, checks each hash link, verifies signature. Does NOT re-execute functions (that's what the proof replaces).
- Proof storage: proofs stored alongside cached values in blob store. Evicted with the cached value. Re-generated on re-materialization.
- Proof compaction: for deep DAGs, full recursive proofs can be large. Support "anchored proofs" that reference a trusted intermediate CAddr (already verified) instead of recursing to leaves.
- Node signing key: each node has an Ed25519 keypair. Public key advertised via SWIM. Proofs signed by the computing node.
- `GetProof(addr)` RPC: returns the derivation proof for a cached value. Returns NOT_FOUND if value not cached (proof regenerated on next materialization).
- Audit log: append-only log of all materializations with proof hashes. Enables "who computed what, when, from what inputs" queries.
- Tests: proof generation for leaf, single-recipe, multi-level DAG. Proof verification succeeds for valid chain, fails for tampered output. Anchored proof verification. Proof survives cache eviction + re-materialization (new proof, same structure).

### 4.4 Deterministic Floating-Point
- **Problem:** IEEE 754 floating-point is not deterministic across architectures. `x86_64` and `aarch64` can produce different results for the same operation due to: extended precision (x87 80-bit), fused multiply-add (FMA) availability, different rounding in transcendental functions (`sin`, `exp`), SIMD vs scalar paths. If a `ComputeFunction` uses floats and runs on different hardware, the output bytes differ → different CAddr → broken invariant.
- **Solution:** A `DeterministicFloat` library that guarantees bit-identical results across all platforms, integrated into the compute sandbox.
- `DeterministicFloat` wrapper type: `f32`/`f64` operations routed through software implementations when hardware results are non-portable
- Strategy 1 (default): **Strict IEEE mode** — disable FMA (`#[cfg(target_feature = ...)]` guards), force rounding mode, avoid x87 (already default on x86_64 with SSE2). Sufficient for basic arithmetic (+, -, *, /, sqrt).
- Strategy 2 (transcendentals): **Softfloat for non-basic ops** — `sin`, `cos`, `exp`, `log`, `pow` implemented in software using MPFR-derived algorithms with proven error bounds. These functions are NOT IEEE-mandated to be deterministic.
- Strategy 3 (WASM): Wasmtime already enforces deterministic floats (NaN canonicalization, no FMA unless explicit). WASM functions get this for free.
- `FloatPolicy` enum: `Strict` (software transcendentals), `Hardware` (trust hardware, faster but non-portable), `Disabled` (reject functions that use floats)
- Compile-time detection: `#[derive(DeterministicCompute)]` proc macro that scans function body for float operations and emits warnings or errors based on policy
- Cross-platform golden tests: known float computations with known bit-exact results, tested on x86_64 and aarch64 in CI
- Tests: basic arithmetic identical across platforms, transcendental functions identical via softfloat, NaN canonicalization, float policy enforcement, WASM float determinism

### 4.5 Content Integrity & Merkle DAG Verification
- **Problem:** Nothing verifies that stored data actually matches its CAddr. A bit-flip in storage (cosmic ray, disk corruption, firmware bug) silently corrupts a blob — and the system serves wrong data under a "correct" address. Similarly, a malicious node could serve fabricated data for any CAddr. The system trusts storage and network implicitly.
- **Solution:** Full Merkle DAG integrity verification — every read optionally verifies `blake3(data) == claimed_addr`, and a background scrubber periodically verifies all stored data.
- **Read-path verification:** `VerifiedGet` mode — after fetching data (local or remote), recompute `blake3(data)` and compare to requested CAddr. If mismatch: discard, log corruption event, fetch from another replica, report to admin.
- **Background scrubber:** Periodic full-store scan that verifies every blob and recipe. Configurable: scrub interval (default 24h), scrub rate limit (default 50MB/s to avoid I/O saturation), priority (lower than client requests).
- **Scrub result:** `ScrubReport { total_objects, verified_ok, corrupted: Vec<(CAddr, CorruptionType)>, duration, bytes_scanned }`. Corrupted objects quarantined (moved to `.quarantine/` directory), re-fetched from replicas.
- **Recipe integrity:** Verify `blake3(recipe.canonical_bytes()) == stored_addr` for all recipes. Catches serialization bugs and storage corruption.
- **Cross-node verification:** `VerifyReplica(addr)` RPC — ask a peer to hash its copy of a blob and return the hash. Compare with local hash. Detects silent divergence between replicas.
- **Corruption response:** On detection: (1) quarantine corrupted copy, (2) fetch correct copy from healthy replica, (3) emit `integrity_violation` metric + alert, (4) log full details for forensics.
- **Integrity metadata:** Each blob optionally stores a small integrity record: `{ addr, blake3, size, stored_at, last_verified_at, verification_count }`. Enables "when was this last verified?" queries.
- Admin API: `GET /admin/scrub/status` — current scrub progress, last report. `POST /admin/scrub/trigger` — start immediate scrub. `GET /admin/integrity/{addr}` — verify single object on demand.
- Tests: inject bit-flip in stored blob, verify detection on read. Scrubber finds corrupted blob, quarantines, re-fetches. Cross-node verification detects divergent replica. Recipe integrity check catches serialization corruption. VerifiedGet rejects tampered remote response.

---

### Track B: Advanced Features (§4.6–§4.10)

### 4.6 WASM Function Plugins
- Users compile custom functions to WASM, register with Deriva
- Wasmtime runtime with determinism guarantees (NaN canonicalization, no non-deterministic imports)
- Sandbox: no network, no filesystem, no clock, no random — non-determinism structurally impossible
- Resource limits: memory cap (default 64MB), fuel/instruction count (default 1B instructions), wall-clock timeout (default 30s)
- WASM function interface: `fn compute(inputs: &[&[u8]], params: &[(key, value)]) -> Vec<u8>`
- Function registration: upload `.wasm` binary, Deriva validates (no disallowed imports), stores as blob, registers in FunctionRegistry
- Hot-reload: update WASM binary, new version gets new FunctionId (content-addressed), old recipes still reference old version
- Integration with §4.4 Deterministic Floating-Point: WASM floats are already deterministic via Wasmtime's canonicalization
- Integration with §4.3 Reproducibility Proofs: WASM execution generates same proof chain as native functions
- Tests: WASM function execution, sandbox escape attempts blocked (no WASI imports), resource limits enforced (OOM, fuel exhaustion, timeout), determinism across runs, cross-platform WASM determinism

### 4.7 FUSE Filesystem Mount
- Mount Deriva as local filesystem via FUSE
- CAddr → path mapping: `/deriva/{first_2_hex}/{full_hex}`
- `open` → `get()`, `read` → stream, `stat` → metadata (size from blob store, mtime=0 for immutable)
- Lazy materialization through standard file operations — `cat /deriva/ab/abcdef...` triggers full DAG resolution
- Read-only mount (content-addressed data is immutable)
- Directory listing: `/deriva/` lists recently accessed CAddrs (configurable LRU)
- Integration with §4.5 Content Integrity: FUSE reads use VerifiedGet mode by default
- Tests: `cat`, `ls`, `stat` on mounted Deriva, large file streaming, concurrent reads, unmount cleanup

### 4.8 Chunk-Level Partial Reads
- Large outputs split into fixed-size chunks, each with own CAddr
- `ChunkedValue`: metadata blob listing chunk CAddrs in order + total size
- Range read `GetRange(addr, offset, length)` resolves to specific chunks, only those are materialized
- Configurable chunk size (default 1MB)
- Chunk CAddrs are deterministic: `chunk_addr = blake3(parent_addr || chunk_index || chunk_data)`
- Integration with §4.1 Canonical Serialization: chunk metadata uses DCF format
- Integration with §4.5 Content Integrity: each chunk independently verifiable
- Tests: partial read of large derived result, verify only needed chunks computed, chunk boundary reads, full reassembly matches original

### 4.9 Mutable References
- Named pointers (`RefName → CAddr`) that can be rebound to new leaf CAddrs
- `RefStore`: persistent key-value store (sled) mapping string names to CAddrs
- Rebinding triggers cascade invalidation via DAG dependents query (§2.6)
- All downstream cached materializations evicted on rebind
- Atomic rebind: update pointer + invalidate in single transaction
- History: optional ref history log (`RefName → Vec<(CAddr, timestamp)>`) for audit
- Integration with §4.3 Reproducibility Proofs: proof chain includes ref resolution snapshot (which CAddr the ref pointed to at materialization time)
- Tests: rebind reference, verify all dependents invalidated, recomputation on next access, concurrent rebind + read safety, ref history query

### 4.10 REST API
- HTTP/JSON alternative to gRPC for broader client compatibility (axum-based)
- Same operations: `PUT /leaf`, `POST /recipe`, `GET /value/{addr}`, `GET /resolve/{addr}`, `POST /invalidate/{addr}`, `GET /status`
- Content-type negotiation: JSON (default), MessagePack (optional)
- Integration with §4.3: `GET /proof/{addr}` returns derivation proof as JSON
- Integration with §4.5: `GET /verify/{addr}` triggers on-demand integrity check
- Tests: mirror all gRPC integration tests over REST, content negotiation, error responses

---

## Phase 5: Production Hardening

**Goal:** Make Deriva production-ready with monitoring, disaster recovery, and operational tooling.

### 5.1 Monitoring & Alerting
- Grafana dashboards for cluster health, performance, and capacity
- PagerDuty/Opsgenie integration for critical alerts
- SLO/SLI definitions and tracking
- Anomaly detection for cache hit rate, latency, error rate

### 5.2 Disaster Recovery
- Backup/restore for recipes, leaf data, and cluster state
- Point-in-time recovery (PITR) for mutable references
- Cross-region backup replication
- Recovery time objective (RTO) and recovery point objective (RPO) guarantees

### 5.3 Multi-Region Deployment
- Cross-region recipe replication
- Geo-aware compute routing (prefer local region)
- WAN-optimized gossip protocol
- Regional failure isolation

### 5.4 Performance Optimization
- CPU profiling and hot path optimization
- Memory allocation profiling
- Network I/O optimization (zero-copy, batching)
- Storage I/O optimization (read-ahead, write coalescing)

### 5.5 Security Hardening
- Comprehensive audit logging (who, what, when, where)
- Role-based access control (RBAC) for API operations
- Secrets management integration (Vault, AWS Secrets Manager)
- Security scanning and vulnerability management

### 5.6 Operational Runbooks
- Incident response procedures
- Capacity planning guidelines
- Upgrade procedures and rollback plans
- Common failure scenarios and remediation

---

## Phase 6: Advanced Computation

**Goal:** Extend computation capabilities beyond CPU-bound functions.

### 6.1 GPU Compute Support
- CUDA/ROCm function execution
- GPU resource scheduling and allocation
- Multi-GPU parallelism
- GPU memory management

### 6.2 Distributed Training
- ML model training across cluster nodes
- Parameter server architecture
- Gradient aggregation and synchronization
- Checkpointing and fault tolerance

### 6.3 Streaming Computation
- Real-time data pipeline support
- Windowed aggregations
- Event-time processing
- Watermark handling

### 6.4 Query Optimization
- Push-down predicates to reduce data transfer
- Lazy evaluation and short-circuiting
- Query plan optimization
- Cost-based execution planning

### 6.5 Incremental Computation
- Reuse partial results when inputs change
- Fine-grained dependency tracking
- Differential dataflow integration
- Adaptive recomputation strategies

---

## Phase 7: Ecosystem Integration

**Goal:** Integrate Deriva with existing data infrastructure and tools.

### 7.1 S3 Compatibility Layer
- S3 API implementation (GetObject, PutObject, ListBucket)
- Drop-in replacement for S3 clients
- Multipart upload support
- Presigned URL generation

### 7.2 Kubernetes Operator
- Custom Resource Definitions (CRDs) for Deriva resources
- Automated deployment and scaling
- StatefulSet management for cluster nodes
- Helm charts for easy installation

### 7.3 Terraform Provider
- Infrastructure as code for Deriva resources
- State management and drift detection
- Import existing resources
- Plan/apply workflow integration

### 7.4 Language SDKs
- Python client library (idiomatic, type-hinted)
- JavaScript/TypeScript SDK (browser + Node.js)
- Go client library
- Java client library

### 7.5 Data Connectors
- Apache Spark integration (read/write DataFrames)
- Dask distributed integration
- Ray integration for ML workloads
- Pandas read/write support

---

## Phase 8: Research Extensions

**Goal:** Explore advanced research directions and novel capabilities.

### 8.1 Formal Verification
- TLA+ specifications for core protocols
- Proof of correctness for consistency model
- Model checking for distributed algorithms
- Verified implementation (Coq/Isabelle)

### 8.2 Byzantine Fault Tolerance
- Malicious node detection and isolation
- Cryptographic proof verification
- Quorum-based consensus for critical operations
- Reputation system for node trustworthiness

### 8.3 Differential Privacy
- Privacy-preserving computation primitives
- Noise injection for aggregate queries
- Privacy budget tracking
- Formal privacy guarantees (ε-differential privacy)

### 8.4 Homomorphic Encryption
- Compute on encrypted data
- Fully homomorphic encryption (FHE) integration
- Secure multi-party computation (MPC)
- Zero-knowledge proofs for computation correctness

### 8.5 Advanced Provenance
- Fine-grained lineage tracking
- Provenance queries (what-if analysis)
- Blame assignment for incorrect results
- Provenance-based access control

---

## Development Principles

- **Determinism is the backbone** — every feature must preserve the invariant: same recipe + same inputs = same output = same CAddr, everywhere, always
- **Minimal code** — write only what's needed, no speculative abstractions
- **Test-driven** — every section adds tests, cumulative count grows monotonically
- **Clippy clean** — zero warnings at all times
- **Incremental commits** — one commit per section, each leaves the system in a working state
- **Backward compatible** — new phases don't break existing APIs or stored data
