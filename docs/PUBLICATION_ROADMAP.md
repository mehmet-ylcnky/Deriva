# Deriva — Publication Roadmap

> This document outlines the planned white papers for the Deriva project.
> Each paper corresponds to a major development phase and has a distinct thesis,
> audience, and contribution. Use this as a living reference when writing future papers.

---

## Paper 1 (PUBLISHED)

**Title:** Deriva: A Computation-Addressed Distributed File System — Design & Single-Node Implementation

**Status:** ✅ Published on GitHub Pages

**Phase:** Phase 1 (Single-Node Core)

**Thesis:** Storage systems can natively understand computation, enabling provenance, recomputability, and cost-aware eviction as structural properties rather than external metadata.

**Audience:** Systems researchers, data engineers, infrastructure architects

**Sections:**
1. Introduction
2. Motivation: The Derived Data Problem
3. Prior Art & Related Work (deep Nix/Bazel/DVC comparisons)
4. Core Concepts (CAddr, Recipes, Materialization, DAG, Eviction)
5. System Architecture (layered design, crate structure, gRPC API)
6. Implementation (tech stack, addressing, compute engine, storage, cache, testing)
7. Evaluation (Phase 1 metrics, comparison matrix)
8. Tradeoffs & Limitations
9. Discussion: A Data Engineering Perspective
10. Future Work
11. Conclusion
12. References

**Key Results:** 244 tests, 6 crates, 0 clippy warnings, 23 integration tests, full gRPC API (6 RPCs), CLI client.

**URL:** https://mehmet-ylcnky.github.io/Deriva/whitepaper/

---

## Paper 2

**Title:** Robust Materialization in a Computation-Addressed Store: Async Execution, Cascade Invalidation & Garbage Collection

**Status:** 📋 Planned (write after Phase 2)

**Phase:** Phase 2 (Robustness) — §2.1–§2.6, §2.8

**Thesis:** A computation-addressed store requires more than correct materialization — it needs persistent dependency tracking, deadlock-free parallel execution, transitive invalidation, and safe storage reclamation. We present a cohesive robustness layer built on a persistent DAG that enables concurrent materialization with broadcast-channel deduplication, policy-driven cascade invalidation, and mark-and-sweep garbage collection with pin-based protection — all without external orchestration.

**Audience:** Systems/concurrency researchers, async Rust community, storage systems community

**Why This Paper Matters:**
- Build systems (Bazel, Ninja) parallelize but don't own the storage — they can't GC or invalidate cached results structurally
- Dataflow engines (Spark, Dask) parallelize but treat storage as external — invalidation requires manual pipeline reruns
- No existing system combines DAG-aware parallel execution + cascade invalidation + GC in a single storage layer
- The persistent DAG is the key enabler: it makes all three features possible from a single data structure

**Planned Sections:**

1. Introduction
   - Recap of Deriva's core model (reference Paper 1)
   - The sequential materialization bottleneck in Phase 1
   - Why DAG-aware parallelism is different from generic task parallelism
   - The robustness gap: Phase 1 had no invalidation, no GC, no persistence across restarts
   - Contribution: persistent DAG as the foundation for three interlocking robustness features

2. Background
   - Critical path analysis in dependency graphs
   - Comparison with build system parallelism (Bazel, Ninja) — parallelize but don't own storage
   - Comparison with dataflow engines (Spark, Dask, Ray) — parallelize but treat storage as external
   - Comparison with cache invalidation strategies (Redis, Memcached) — no dependency awareness
   - Comparison with storage GC (Git gc, HDFS block scanner) — no computation awareness

3. Persistent DAG Store (§2.1)
   - sled-backed forward/reverse adjacency lists
   - Transactional inserts: recipe + edges atomically
   - Survives restarts — no DAG reconstruction needed
   - `all_addrs()` and `live_addr_set()` for GC live set computation
   - Design decision: sled over RocksDB (embedded, pure Rust, transactional)

4. Async Materialization (§2.2–§2.3)
   - AsyncExecutor: parallel materialization via Tokio tasks
   - Broadcast-channel deduplication: when multiple requests need the same CAddr simultaneously, only one computes — others subscribe to the broadcast channel and receive the result
   - Semaphore-bounded concurrency: acquired only at the compute step (not during input resolution) to prevent deadlock in diamond DAGs
   - Input assembly: positional slot collection ensures deterministic input ordering regardless of parallel completion order
   - Comparison: Bazel (action graph parallelism, external cache), Ninja (implicit dep parallelism), Dask (task graph with distributed scheduler)

5. Verification Mode (§2.4)
   - Dual-compute for high-assurance workloads: execute function twice, compare output hashes
   - `DeterminismViolation` error type for runtime detection of non-deterministic functions
   - Three modes: Off (production), DualCompute (full verification), Sampled (probabilistic — verify N% of materializations)
   - Performance cost: 2x compute, measured via benchmarks
   - Why this matters for CAS: if `f(inputs)` is non-deterministic, the entire addressing model breaks

6. Cascade Invalidation (§2.6)
   - Problem: when a leaf changes, all transitive dependents have stale cached materializations
   - `CascadeInvalidator`: walks reverse edges in the persistent DAG to find all transitive dependents
   - Policy-driven: `CascadePolicy` controls depth limits, whether to invalidate or just report
   - Sync and async variants for different execution contexts
   - Dry-run mode: show what would be invalidated without actually evicting
   - Comparison: database materialized view refresh (full recompute), dbt incremental models (manual dependency declaration), Airflow (DAG-level retrigger, no cache awareness)

7. Garbage Collection (§2.8)
   - Problem: orphaned blobs, orphaned recipes, stale cache entries accumulate over time
   - Mark-and-sweep: live set = `dag.live_addr_set()` ∪ pinned addrs
   - Sweep phases: orphaned blobs → orphaned recipes → stale cache entries → dangling DAG edges
   - `PinSet` (Arc<RwLock>): protect important addrs from GC via gRPC Pin/Unpin RPCs
   - `GcConfig`: grace period, max removals, dry-run mode
   - `GcResult`: blobs removed, bytes reclaimed, recipes removed, cache entries cleared
   - Comparison: Git gc (pack objects, prune unreachable), HDFS block scanner (replica-aware), S3 lifecycle rules (time-based, no dependency awareness)

8. Observability (§2.5)
   - 40+ Prometheus metrics across all subsystems: RPC latency, cache hit/miss, DAG operations, GC runs, cascade invalidations, materialization duration by function
   - Metrics exposed via axum HTTP server on separate port (/metrics + /health)
   - Key metrics: `MATERIALIZATION_DURATION`, `CACHE_HIT_TOTAL`, `CACHE_MISS_TOTAL`, `GC_BLOBS_REMOVED`, `GC_BYTES_RECLAIMED`, `CASCADE_EVICTIONS_TOTAL`
   - Design: lazy_static Prometheus counters/histograms, zero-cost when not scraped

9. Evaluation
   - Benchmark: sequential (Phase 1 Executor) vs parallel (Phase 2 AsyncExecutor) on diamond/wide/deep DAGs
   - Metric: wall-clock time, speedup factor, CPU utilization
   - Benchmark: verification mode overhead (DualCompute vs Off)
   - Benchmark: cascade invalidation time vs number of transitive dependents (N = 1, 10, 100, 1000)
   - Benchmark: GC sweep time vs number of blobs (100, 1000, 5000)
   - Benchmark: GC live set computation time vs DAG size
   - Benchmark: persistent DAG recovery time after restart
   - Benchmark: broadcast-channel dedup — N concurrent requests for same CAddr, measure total compute invocations (should be 1)

10. Tradeoffs
    - Async complexity vs performance gain (broadcast channels add code complexity)
    - Semaphore at compute-only vs full-path (deadlock prevention vs potential over-parallelism)
    - Verification mode cost vs safety guarantee (2x compute is expensive)
    - GC grace period vs storage reclamation speed
    - Pin set as manual protection vs automatic reference counting
    - Observability overhead (metric recording on every operation)

11. Conclusion
    - The persistent DAG is the single enabler for all three robustness features
    - Parallel execution, cascade invalidation, and GC are not independent features — they share the DAG and interact (GC respects cascade state, cascade uses DAG reverse edges, parallel execution populates the DAG)
    - First system to combine DAG-aware parallel materialization + structural cascade invalidation + computation-aware GC in a unified storage layer

**Methodology:**
- Controlled benchmarks on fixed hardware (document CPU, RAM, SSD specs)
- DAG shapes: diamond (depth 3, width 2), wide fan-out (1→100), deep chain (depth 20), realistic ML pipeline (mixed)
- Each benchmark: 100 runs, report p50/p95/p99
- Compare: Phase 1 sequential executor vs Phase 2 parallel executor on identical workloads
- GC benchmarks: vary blob count (100, 1000, 5000), measure sweep time and live set computation
- Cascade benchmarks: vary fan-out width (1→N), measure invalidation time

**Key Metrics to Collect:**
| Metric | Unit | Collection Method |
|--------|------|-------------------|
| Materialization wall time | ms | Instrumented executor, per-request |
| Parallelism factor | concurrent tasks / total tasks | Tokio task count during materialization |
| Dedup effectiveness | compute invocations / unique CAddrs requested | Counter in AsyncExecutor |
| DAG recovery time | ms | Time from process start to DAG fully loaded from sled |
| Verification overhead | % | Dual-compute time / single-compute time |
| Cache hit rate | % | Counter in cache layer |
| Materialization latency by function | ms | Per-function histogram via tracing |
| Cascade invalidation time | ms per N dependents | Timed cascade with varying fan-out |
| Cascade depth reached | levels | Instrumented invalidator |
| GC sweep time | ms | Timed run_gc with varying blob counts |
| GC live set computation time | μs | Timed live_addr_set() |
| GC blobs removed | count | GcResult fields |
| GC bytes reclaimed | bytes | GcResult fields |
| GC list_addrs time | ms | Timed BlobStore::list_addrs() (I/O-bound, dominates GC) |

---

## Paper 2b

**Title:** Streaming Materialization in a Computation-Addressed Store: Chunk-Level Dataflow over Dependency Graphs

**Status:** 📋 Planned (write after Phase 2)

**Phase:** Phase 2 (Robustness) — §2.7

**Thesis:** Computation-addressed storage can natively support streaming dataflow without external orchestration — by combining the dependency DAG with chunk-level processing, the system can stream derived data through multi-stage pipelines while preserving content-addressing, caching, and determinism guarantees that batch-only systems cannot offer for streaming workloads.

**Audience:** Dataflow/streaming systems researchers, data engineering community, Rust async ecosystem

**Why This Paper Matters:**
- Existing streaming systems (Flink, Kafka Streams, Spark Structured Streaming) are external orchestrators — they don't own the storage and can't leverage content-addressing for deduplication or caching
- Existing CAS systems (Nix, Bazel, IPFS) are batch-only — they materialize entire outputs before serving
- Deriva bridges the gap: streaming execution within a content-addressed store, where intermediate chunks are addressable and cacheable
- The auto-wrapping mechanism (batch functions transparently participate in streaming pipelines) is a novel contribution — no existing system does this

**Planned Sections:**

1. Introduction
   - The batch materialization limitation: client must wait for full output before receiving any bytes
   - Why streaming matters for computation-addressed storage: large derived datasets, interactive queries, pipeline composition
   - Reference Paper 1 (core model) and Paper 2 (async execution, persistent DAG)
   - Contribution: streaming dataflow that preserves CAS invariants (addressability, caching, determinism)

2. Background: Streaming Dataflow
   - Dataflow models: push vs pull, backpressure, chunk semantics
   - Comparison with Spark Structured Streaming (micro-batch, external storage)
   - Comparison with Apache Flink (true streaming, external state backends)
   - Comparison with Kafka Streams (log-based, no computation-addressing)
   - Comparison with Unix pipes (streaming but no caching, no DAG awareness, no content-addressing)
   - Gap: no existing system combines streaming execution with content-addressed storage

3. StreamChunk Protocol
   - `StreamChunk` enum: `Data(Bytes)`, `End`, `Error(String)`
   - Chunk-level processing: functions receive and emit individual chunks, not full blobs
   - Backpressure via bounded async channels (tokio::mpsc)
   - End-of-stream semantics: `End` chunk signals completion, enables downstream finalization
   - Error propagation: `Error` chunk terminates pipeline with diagnostic information

4. Streaming Compute Functions (§2.7)
   - `StreamingComputeFunction` trait: `async fn process_chunk(&self, chunk: StreamChunk, state: &mut State) -> Vec<StreamChunk>`
   - 20 built-in streaming functions across 4 categories:
     - **Chunk-by-chunk transforms** (9): identity, uppercase, lowercase, reverse, base64_encode, base64_decode, xor, compress, decompress
     - **Accumulators** (3): sha256, byte_count, checksum — accumulate state across chunks, emit result on `End`
     - **Multi-input combiners** (3): concat, interleave, zip_concat — merge multiple input streams
     - **Pipeline utilities** (5): chunk_resizer, take, skip, repeat, tee_count
   - Design: stateless transforms vs stateful accumulators — different processing models unified under one trait

5. Auto-Wrapping: Batch Functions in Streaming Pipelines
   - Problem: existing batch `ComputeFunction` implementations should work in streaming pipelines without rewriting
   - Solution: `BatchToStreamAdapter` — collects all chunks into a buffer, calls batch function on `End`, emits result as chunks
   - Transparent to pipeline construction: `FunctionRegistry` resolves streaming-first, falls back to auto-wrapped batch
   - Tradeoff: auto-wrapped functions lose streaming benefits (must buffer full input) but gain pipeline composability
   - This is novel: no existing system auto-wraps batch functions into streaming pipelines

6. StreamPipeline & DAG-Aware Pipeline Construction
   - `StreamPipeline`: ordered sequence of streaming stages, built from DAG traversal
   - `StreamingExecutor`: walks the persistent DAG from target CAddr, builds pipeline by resolving each recipe to its streaming function
   - Tee semantics: pipeline output is tee'd to both the client stream (gRPC `Get` response) and the cache (for future hits)
   - Multi-input handling: combiner functions receive multiple input streams, synchronized by the pipeline
   - Pipeline composition: pipelines can be nested (output of one pipeline feeds input of another)

7. Integration with CAS Invariants
   - Content-addressing: final output of a streaming pipeline produces the same CAddr as batch materialization (determinism preserved)
   - Caching: tee'd output is stored in `SharedCache` — subsequent requests for the same CAddr hit cache, skip pipeline
   - Invalidation: cascade invalidation (Paper 2) applies to streaming results — invalidating an input invalidates all downstream cached stream outputs
   - Verification: streaming results can be verified by re-executing the pipeline and comparing output CAddr (verification mode from Paper 2)

8. Evaluation
   - Benchmark: streaming vs batch materialization — time-to-first-byte for varying output sizes (1MB, 100MB, 1GB)
   - Benchmark: pipeline throughput — chunks/sec for transform chains of depth 1, 5, 10, 20
   - Benchmark: auto-wrap overhead — batch function via auto-wrap vs native streaming function
   - Benchmark: tee overhead — streaming with cache tee vs without
   - Benchmark: multi-input combiner throughput — concat/interleave/zip_concat with 2, 5, 10 input streams
   - Benchmark: accumulator memory — peak memory for sha256/byte_count/checksum on 1GB input (should be O(state), not O(input))
   - Comparison: Deriva streaming vs Unix pipes vs Spark Structured Streaming on equivalent workloads

9. Tradeoffs
   - Streaming complexity vs time-to-first-byte improvement
   - Auto-wrap buffering vs native streaming (convenience vs performance)
   - Tee overhead vs cache hit benefit on subsequent requests
   - Chunk size selection: small chunks = low latency, large chunks = high throughput
   - Stateful accumulators: must buffer state across chunks (memory pressure for large inputs)

10. Conclusion
    - First system to combine streaming dataflow with content-addressed storage
    - Auto-wrapping enables gradual migration: start with batch functions, add streaming implementations for hot paths
    - The persistent DAG enables pipeline construction without external orchestration — the system knows the computation graph and can build the pipeline automatically
    - Streaming preserves all CAS invariants: same CAddr, same cache behavior, same invalidation semantics

**Methodology:**
- Controlled benchmarks on fixed hardware (document CPU, RAM, SSD specs)
- Chunk sizes: 4KB, 64KB, 1MB (measure throughput and latency for each)
- Pipeline depths: 1, 5, 10, 20 stages
- Input sizes: 1MB (small), 100MB (medium), 1GB (large)
- Each benchmark: 100 runs, report p50/p95/p99
- Compare: Deriva streaming vs batch materialization (same functions, same inputs)
- Compare: Deriva streaming vs Unix pipe chain (equivalent transforms)
- Memory profiling: peak RSS during streaming vs batch for large inputs

**Key Metrics to Collect:**
| Metric | Unit | Collection Method |
|--------|------|-------------------|
| Time-to-first-byte | ms | Client-side measurement from request to first chunk |
| Time-to-completion | ms | Client-side measurement from request to last chunk |
| Pipeline throughput | MB/s | Total bytes / time-to-completion |
| Chunks per second | chunks/s | Counter in StreamingExecutor |
| Auto-wrap overhead | % | Auto-wrapped time / native streaming time |
| Tee overhead | % | Streaming with tee / streaming without tee |
| Peak memory (streaming) | MB | RSS measurement during pipeline execution |
| Peak memory (batch) | MB | RSS measurement during batch materialization |
| Memory ratio | streaming_peak / batch_peak | Derived from above (should be < 1 for large inputs) |
| Accumulator state size | bytes | Instrumented accumulator state |
| Cache hit after tee | % | Subsequent requests that hit cache |
| CAddr consistency | % identical | Streaming output CAddr vs batch output CAddr (should be 100%) |

---

## Paper 3

**Title:** Distributed Computation-Addressed Storage: Gossip, Replication & Locality-Aware Compute Routing

**Status:** 📋 Planned (write after Phase 3)

**Phase:** Phase 3 (Distribution)

**Thesis:** When the storage system has visibility into both the dependency DAG and the distributed cache state, it can route computation to data rather than moving data to computation — achieving better locality than any external orchestrator.

**Audience:** Distributed systems community, data platform architects

**Planned Sections:**

1. Introduction
   - The distribution challenge for computation-addressed storage
   - Why naive sharding breaks the model (recipes reference CAddrs that may live on other nodes)
   - Reference Papers 1, 2 & 2b for single-node foundation

2. Node Discovery: SWIM Gossip Protocol (§3.1)
   - SWIM protocol adaptation for Deriva
   - Metadata disseminated: cache contents, storage capacity, compute availability
   - Failure detection bounds and convergence properties
   - Comparison with: ZooKeeper (centralized), etcd (consensus), Consul (gossip + consensus)

3. Tiered Replication Strategy (§3.2–§3.3)
   - Recipe replication (§3.2): all nodes (tiny, critical — losing a recipe = losing recomputability)
   - Leaf data replication (§3.3): configurable factor (default 3), consistent hashing for placement
   - Cached materializations: NOT replicated (recomputable from recipe + inputs)
   - Analysis: why this tiering is optimal for computation-addressed storage
   - Comparison with: HDFS (uniform 3x replication), Cassandra (tunable consistency), S3 (11 nines)

4. Cache Placement & Eviction (§3.4)
   - Cost-aware eviction: evict cached materializations with lowest recomputation cost first
   - Placement policy: prefer nodes with available capacity and fast storage
   - Integration with gossip: cache state dissemination for routing decisions

5. Locality-Aware Compute Routing (§3.5)
   - The routing decision: when `get()` hits a cache miss, where should computation happen?
   - Algorithm: check gossip metadata → find nodes with inputs cached → route to node with most input bytes
   - Data transfer minimization: compute moves to data, not data to compute
   - Split input handling: when inputs are on different nodes, route to the node with the largest input
   - Comparison with: Spark data locality (rack-aware), Presto (connector pushdown), Dask distributed scheduler

6. Distributed Get & Materialization (§3.6)
   - Cross-node materialization: recursive get() calls across cluster
   - Caching strategy: where to cache intermediate results
   - Failure handling: retry on different node, recompute on failure

7. Consistency Model (§3.7)
   - Recipe consistency: strong (replicated to all nodes before acknowledgment)
   - Cache consistency: eventual (materialization may exist on some nodes but not others)
   - Leaf data consistency: tunable (read-after-write for replication factor, eventual for cross-node)
   - CAP analysis: Deriva chooses AP for cache, CP for recipes

8. Operational Infrastructure (§3.8–§3.17)
   - **Cluster Bootstrap & Discovery (§3.8):** seed nodes, DNS-SRV, bootstrap state machine, join protocol
   - **Cluster-Wide Garbage Collection (§3.9):** distributed mark-and-sweep, leader election, quorum agreement, tombstone propagation
   - **Data Rebalancing (§3.10):** push-based migration on ring changes, streaming transfer, rate limiting, checkpoint/resume, ownership handoff
   - **Backpressure & Admission Control (§3.11):** 3-stage admission (memory → fairness → adaptive concurrency), Vegas-style limiter, priority tiers, circuit breakers
   - **Mutual TLS (§3.12):** rustls, hot-reload via file watcher, HMAC-SHA256 for UDP gossip, certificate rotation
   - **Connection Pooling (§3.13):** lazy + warm-up, health checking, exponential backoff, idle reaper, per-node pools
   - **Batch Operations (§3.14):** scatter-gather BatchGet/BatchPutLeaf, semaphore-bounded concurrency, streaming responses, partial failure handling
   - **Admin API (§3.15):** separate port, cluster introspection, runtime config, drain/rebalance triggers, health checks, metrics export
   - **Request Hedging (§3.16):** speculative execution to second replica, adaptive delay (EWMA), budget-capped (10%), tail latency reduction
   - **Rolling Upgrades (§3.17):** protocol/feature version negotiation, SWIM version tags, feature gates, zero-downtime upgrades, backward compatibility

9. Evaluation
   - Cluster setup: 3, 5, 10 nodes (document hardware specs)
   - Benchmark: compute routing vs random placement vs round-robin
   - Metric: data transferred per materialization, wall-clock time, network utilization
   - Benchmark: node failure recovery (time to detect, time to recompute lost cached data)
   - Benchmark: recipe replication convergence time
   - Benchmark: scaling — throughput vs cluster size
   - Benchmark: operational features (GC pause time, rebalance throughput, hedge win rate, rolling upgrade duration)

10. Tradeoffs
    - Gossip protocol overhead vs discovery speed
    - Recipe replication cost (all-node) vs safety
    - Routing decision latency vs placement quality
    - Operational complexity vs production readiness

11. Conclusion

**Note on Scope:** Paper 3 covers 17 sections of detailed blueprints (24,825 lines). The paper should focus on the novel contributions (tiered replication, locality-aware routing, computation-aware consistency model) while summarizing operational infrastructure (§3.8–§3.17) as "production-readiness" contributions. The operational sections are individually well-understood techniques, but their integration into a computation-addressed system is novel.

**Methodology:**
- Multi-node test cluster (containerized, reproducible setup via Docker Compose or Kubernetes)
- Network simulation: introduce latency/bandwidth constraints between nodes
- Workloads: same DAG shapes as Paper 2, but with data distributed across nodes
- Failure injection: kill nodes during materialization, measure recovery
- Comparison targets: S3 + Airflow (industry standard), Spark (data locality), HDFS (replication)
- Each benchmark: 50 runs per configuration, report p50/p95/p99 + standard deviation

**Key Metrics to Collect:**
| Metric | Unit | Collection Method |
|--------|------|-------------------|
| Data transferred per materialization | MB | Network counters per request |
| Compute routing decision time | μs | Instrumented router |
| Gossip convergence time | ms | Time from node join to full metadata propagation |
| Node failure detection time | ms | Time from kill signal to gossip protocol detection |
| Recipe replication lag | ms | Time from recipe write to all-node availability |
| Throughput vs cluster size | ops/s | Sustained load test at each cluster size |
| Storage efficiency | Deriva total bytes / naive 3x replication bytes | `du` across all nodes |
| Rebalance throughput | MB/s | Streaming transfer measurement |
| Rebalance completion time | s per GB migrated | End-to-end migration timing |
| Backpressure rejection rate | % | Rejected / total requests under load |
| Hedge win rate | % | Hedge responses that arrived first |
| Hedge p99 improvement | ms | p99 with hedging vs without |
| Batch throughput | ops/s | BatchGet(100) vs 100 sequential gets |
| Rolling upgrade duration | min per node | Drain + swap + rejoin timing |
| mTLS handshake overhead | μs | TLS vs plaintext connection setup |

---

## Paper 4

**Title:** Structural Determinism in Distributed Storage: Canonical Serialization, Reproducibility Proofs & Content Integrity in Deriva

**Status:** 📋 Planned (write after Phase 4 Track A: §4.1–§4.5)

**Phase:** Phase 4 Track A (Determinism Backbone)

**Thesis:** Computation-addressed storage systems require determinism not as a best-effort convention but as a structurally enforced, cryptographically verifiable invariant. We present five interlocking mechanisms — canonical serialization, deterministic scheduling, reproducibility proofs, portable floating-point, and content integrity verification — that make non-determinism detectable, preventable, and provable across heterogeneous distributed nodes.

**Audience:** Systems researchers, formal verification community, scientific computing, regulated industries (finance, healthcare), reproducible research advocates

**Why This Paper Matters:**
- No existing system (Nix, Bazel, DVC, IPFS) provides end-to-end determinism guarantees with cryptographic proofs
- Nix achieves reproducibility through sandboxed builds but doesn't verify outputs cryptographically
- IPFS provides content-addressing but has no computation model to verify
- Bazel has remote caching but trusts the cache — no proof that cached results are correct
- This paper fills the gap: determinism as a first-class, verifiable system property

**Planned Sections:**

1. Introduction
   - The determinism assumption: every CAS system assumes `hash(f(inputs)) = addr` is stable, but none enforce it end-to-end
   - Failure modes: serialization drift, scheduling non-determinism, floating-point divergence, storage corruption, malicious nodes
   - Contribution: five mechanisms that close every gap in the determinism chain
   - Reference Papers 1–3 for system foundation

2. Threat Model: How Determinism Breaks
   - **Serialization drift:** `bincode` format changes across compiler/library versions → same Recipe → different CAddr
   - **Scheduling non-determinism:** parallel input resolution delivers inputs in different order → different output
   - **Floating-point divergence:** x86 FMA vs ARM non-FMA → different bytes for same computation
   - **Storage corruption:** bit-flip in blob → wrong data served under "correct" address
   - **Byzantine nodes:** malicious node returns fabricated data for any CAddr
   - Taxonomy: which threats are accidental vs adversarial, which are detectable vs preventable
   - Comparison: how Nix, Bazel, IPFS, DVC handle (or don't handle) each threat

3. Canonical Serialization & Stable Hashing (§4.1)
   - The problem: `bincode::serialize` is not version-stable
   - Deriva Canonical Format (DCF): hand-written, versioned, frozen byte-level encoding
   - Design: magic header, big-endian integers, sorted map keys, length-prefixed strings
   - Migration strategy: detect old format on read, re-encode, dual-hash verification
   - Golden file tests: known Recipe → known CAddr, checked into CI
   - Property: `∀ r₁ r₂: semantically_equal(r₁, r₂) ⟹ dcf_encode(r₁) = dcf_encode(r₂)`
   - Property: `∀ v₁ v₂ (DCF versions): dcf_v1_encode(r) = dcf_v2_encode(r)` (format frozen)
   - Comparison: Protocol Buffers deterministic serialization (not guaranteed), Cap'n Proto (zero-copy but platform-dependent padding), CBOR canonical form (RFC 7049 §3.9)

4. Deterministic Compute Scheduling (§4.2)
   - The problem: parallel materialization + non-deterministic task completion order
   - Input assembly barrier: positional slot collection, recipe-order delivery
   - Proof: regardless of parallelism level (1 core vs 64 cores), `execute()` receives identical `Vec<Bytes>`
   - Execution modes: Parallel (with barrier), Sequential (debugging), Deterministic-Replay (audit)
   - Scheduler determinism test harness: 1000 runs with random delays → identical CAddr
   - Formal property: `∀ schedules s₁ s₂: execute(recipe, resolve(inputs, s₁)) = execute(recipe, resolve(inputs, s₂))`
   - Comparison: Spark deterministic scheduling (not guaranteed for UDFs), Dask (task ordering not guaranteed), Make (sequential by default)

5. Reproducibility Proofs & Audit Trail (§4.3)
   - The problem: client must trust server that cached value is correctly derived
   - DerivationProof: Merkle chain from output through recipe graph to leaf inputs
   - Proof structure: recursive InputProof (Leaf | Derived), Ed25519 node signature
   - Verification: walk proof tree, check hash links, verify signatures — no re-execution needed
   - Proof compaction: anchored proofs for deep DAGs (reference trusted intermediate)
   - Audit log: append-only materialization history with proof hashes
   - Formal property: `verify(proof, trusted_leaves) = true ⟹ output was correctly derived from inputs`
   - Comparison: Sigstore/in-toto (software supply chain provenance), Trillian (verifiable logs), blockchain (consensus-based trust)
   - Use case: regulatory compliance (finance: prove a risk calculation was derived from specific market data)

6. Deterministic Floating-Point (§4.4)
   - The problem: IEEE 754 non-determinism across architectures (FMA, x87, transcendentals)
   - Strategy: strict IEEE mode for basic ops, softfloat for transcendentals, WASM canonicalization
   - FloatPolicy: Strict (portable), Hardware (fast), Disabled (reject floats)
   - Cross-platform golden tests: bit-exact results on x86_64 and aarch64
   - Analysis: which operations are safe (add, mul, sqrt) vs unsafe (sin, exp, pow)
   - Comparison: Java `strictfp` (deprecated, incomplete), WASM spec (NaN canonicalization only), Herbie (accuracy, not determinism)

7. Content Integrity & Merkle DAG Verification (§4.5)
   - The problem: storage corruption and malicious nodes
   - Read-path verification: `blake3(data) == claimed_addr` on every fetch
   - Background scrubber: periodic full-store scan, quarantine + re-fetch corrupted objects
   - Cross-node verification: `VerifyReplica` RPC detects silent replica divergence
   - Corruption response: quarantine → re-fetch → alert → forensics
   - Comparison: ZFS checksums (block-level, no content-addressing), HDFS block scanner, S3 (MD5 on upload only), IPFS (verify on fetch)

8. Evaluation
   - **Canonical serialization overhead:** DCF encode/decode vs bincode (expected: <2x slower, acceptable for correctness)
   - **Scheduling barrier overhead:** parallel with barrier vs without (expected: <5% overhead from slot collection)
   - **Proof generation overhead:** materialization with proof vs without (expected: <10% — proof is metadata, not re-computation)
   - **Proof verification time:** verify proof for DAG depth 1, 5, 10, 20 (expected: <1ms per level)
   - **Proof size:** bytes per proof vs DAG depth (expected: ~200 bytes per level)
   - **Softfloat overhead:** software transcendentals vs hardware (expected: 10-100x slower — but deterministic)
   - **Scrubber throughput:** MB/s verified, impact on foreground latency (expected: <5% latency impact at 50MB/s scrub rate)
   - **Cross-platform determinism:** identical CAddrs for same workload on x86_64 vs aarch64 (expected: 100% match with strict mode)
   - **End-to-end:** same DAG materialized on 3 different nodes → identical output CAddr (expected: 100%)

9. Security Analysis
   - What the determinism backbone protects against: accidental corruption, serialization bugs, scheduling races, platform divergence
   - What it does NOT protect against: compromised node signing key, side-channel attacks on computation
   - Trust model: proofs are as trustworthy as the signing node's key
   - Defense in depth: verification mode (§2.4) + proofs (§4.3) + integrity checks (§4.5) = three independent detection layers

10. Tradeoffs
    - Canonical serialization: correctness vs performance (DCF slower than bincode)
    - Softfloat: portability vs speed (10-100x for transcendentals)
    - Proof storage: auditability vs space (proofs add ~1KB per materialization)
    - Scrubber: integrity vs I/O budget
    - Overall: Deriva chooses correctness over performance at every decision point

11. Related Work
    - Content-addressed storage: IPFS, Git, Venti, Perkeep
    - Reproducible builds: Nix, Guix, Reproducible Builds project
    - Build systems: Bazel, Buck2, Pants (remote caching trust model)
    - Verifiable computation: SNARKs, STARKs (too expensive for storage), TLS certificate transparency
    - Data provenance: W3C PROV, in-toto, SLSA framework

12. Conclusion
    - Determinism is not a feature — it is the foundation
    - Five mechanisms, each addressing a different failure mode, together provide end-to-end guarantees
    - First system to combine content-addressing + computation + cryptographic reproducibility proofs

**Methodology:**
- Controlled benchmarks on fixed hardware (x86_64 primary, aarch64 cross-validation)
- Each benchmark: 100 runs, report p50/p95/p99
- Cross-platform tests: CI on both x86_64 and aarch64 (GitHub Actions + ARM runner)
- Proof verification: synthetic DAGs of varying depth (1–100 levels)
- Corruption injection: bit-flip at random positions in stored blobs
- Serialization stability: encode with Deriva v1, decode with Deriva v2 (simulated version bump)

**Key Metrics to Collect:**
| Metric | Unit | Collection Method |
|--------|------|-------------------|
| DCF encode throughput | MB/s | Benchmark on Recipe corpus |
| DCF decode throughput | MB/s | Benchmark on Recipe corpus |
| Scheduling barrier overhead | % | Parallel with/without barrier |
| Proof generation time | μs per DAG level | Instrumented executor |
| Proof verification time | μs per DAG level | Standalone verifier |
| Proof size | bytes per DAG level | Serialized proof measurement |
| Softfloat overhead ratio | hardware_time / softfloat_time | Paired benchmarks |
| Scrubber throughput | MB/s | Background scrub measurement |
| Scrubber foreground impact | % latency increase | A/B test with/without scrubber |
| Cross-platform match rate | % identical CAddrs | Same workload on x86_64 vs aarch64 |
| Corruption detection rate | % detected / % injected | Fault injection test |
| End-to-end determinism | % identical across N nodes | Multi-node same-workload test |

---

## Paper 5

**Title:** The Programmable Filesystem: WASM Functions, FUSE Mount & Sandboxed Computation in Storage

**Status:** 📋 Planned (write after Phase 4 Track B: §4.6–§4.10)

**Phase:** Phase 4 Track B (Advanced Features)

**Thesis:** User-defined deterministic functions, executed in a WASM sandbox within the storage layer, transform a file system from a passive data container into a programmable computation substrate — while maintaining the security and determinism guarantees that computation-addressed storage requires.

**Audience:** PL/systems intersection, WASM community, filesystem researchers

**Planned Sections:**

1. Introduction
   - From built-in functions to user-defined functions
   - The trust problem: how do you run arbitrary user code in a storage system?
   - WASM as the answer: deterministic, sandboxed, resource-limited
   - Integration with determinism backbone (Paper 4): WASM inherits all guarantees

2. WASM Function Plugin System (§4.6)
   - Registration: users compile functions to WASM, register with Deriva
   - Execution: Wasmtime runtime with determinism guarantees
   - Sandbox: no network, no filesystem, no clock access — non-determinism structurally impossible
   - Resource limits: memory caps, instruction count limits, timeout enforcement
   - Integration with deterministic floating-point (§4.4): WASM NaN canonicalization
   - Integration with reproducibility proofs (§4.3): WASM execution generates same proof chain
   - Comparison with: Cloudflare Workers (V8 isolates), Fastly Compute (WASM), AWS Lambda (container)

3. FUSE Filesystem Mount (§4.7)
   - CAddr → path mapping: `/deriva/ab/cd/abcdef01.../`
   - POSIX operations: `open` → `get()`, `read` → stream, `stat` → metadata lookup
   - Lazy materialization through filesystem interface: `cat /deriva/<addr>` triggers computation
   - Integration with content integrity (§4.5): FUSE reads use VerifiedGet by default
   - Use case: existing tools (editors, viewers, scripts) work with Deriva data unmodified
   - Comparison with: Plan 9 / 9P (computation-aware FS), FUSE-based CAS (git-annex), S3FS

4. Chunk-Level Partial Reads (§4.8)
   - Problem: large derived results where client needs only a subset
   - Solution: split outputs into fixed-size chunks, each with own CAddr
   - Range reads resolve to specific chunks — only those chunks are materialized
   - Integration with canonical serialization (§4.1): chunk metadata uses DCF
   - Integration with content integrity (§4.5): each chunk independently verifiable
   - Use case: large datasets, byte-range access, columnar partial reads

5. Mutable References with Cascade Invalidation (§4.9)
   - Named pointers that can be rebound to new leaf CAddrs
   - Rebinding triggers DAG dependents query → invalidate all downstream cached materializations
   - Integration with reproducibility proofs (§4.3): proof includes ref resolution snapshot
   - Model: "the input data changed, update everything downstream" without manual tracking
   - Comparison with: database materialized views, dbt incremental models

6. REST API (§4.10)
   - HTTP/JSON alternative to gRPC for broader client compatibility
   - Proof and integrity endpoints: `GET /proof/{addr}`, `GET /verify/{addr}`
   - Content negotiation: JSON, MessagePack

7. Evaluation
   - WASM overhead: native function vs WASM function execution time
   - FUSE throughput: Deriva-over-FUSE vs local FS vs S3FS vs NFS
   - Chunk read efficiency: full materialization vs partial read (measure bytes computed vs bytes returned)
   - Cascade invalidation: time to invalidate N dependents after mutable reference rebind
   - REST vs gRPC: latency comparison for same operations

8. Security Analysis
   - WASM sandbox escape surface
   - Resource exhaustion attacks (memory bombs, infinite loops)
   - Trusted native vs sandboxed WASM function tiers
   - How determinism backbone (Paper 4) protects against WASM non-determinism

9. Tradeoffs
   - WASM overhead vs security guarantee
   - FUSE overhead vs compatibility
   - Chunk granularity vs metadata overhead

10. Conclusion

**Methodology:**
- WASM benchmarks: identical functions compiled to native Rust and WASM, measure overhead ratio
- FUSE benchmarks: standard filesystem benchmarks (fio, bonnie++) on Deriva-FUSE vs local ext4 vs S3FS
- Chunk benchmarks: vary chunk size (4KB, 64KB, 1MB, 16MB), measure overhead vs read amplification
- Security: fuzzing WASM inputs, resource limit enforcement testing

**Key Metrics to Collect:**
| Metric | Unit | Collection Method |
|--------|------|-------------------|
| WASM overhead ratio | native_time / wasm_time | Paired execution of identical functions |
| FUSE throughput | MB/s | fio sequential read/write |
| FUSE latency | μs | fio random read, p50/p95/p99 |
| Chunk read amplification | bytes_computed / bytes_returned | Instrumented chunk resolver |
| Cascade invalidation time | ms / N dependents | Timed rebind + invalidation |
| WASM sandbox startup time | μs | Wasmtime instance creation |
| WASM memory limit enforcement | pass/fail | Fuzzing with memory-bomb modules |
| REST vs gRPC latency | ms | Paired request benchmarks |

---

## Paper 6

**Title:** Computation-Addressed Storage in Practice: An Empirical Evaluation of Deriva

**Status:** 📋 Planned (write after Phase 4, when all features are available)

**Phase:** Post Phase 4 (Complete System)

**Thesis:** Computation-addressed storage provides measurable advantages in storage efficiency, cascade invalidation, and reproducibility for DAG-structured data workloads — at the cost of higher cold-read latency and system complexity. This paper provides the first comprehensive empirical evaluation with real numbers.

**Audience:** Practitioners, data platform teams, engineering leadership evaluating adoption

**Planned Sections:**

1. Introduction
   - Motivation: claims from Paper 1 need empirical validation
   - Goal: honest, reproducible benchmarks showing where Deriva wins AND loses
   - All benchmark code and data published as open source

2. Experimental Setup
   - Hardware specification (CPU, RAM, SSD, network — exact models)
   - Software versions (Rust, Tokio, sled, tonic, OS kernel)
   - Comparison targets and their versions:
     - Local filesystem + shell scripts (baseline "no system")
     - S3 + Apache Airflow (industry standard data pipeline)
     - DVC (closest philosophical competitor)
     - Bazel remote cache (build-system CAS)
     - Redis (pure cache baseline for retrieval speed)
   - Reproducibility: all benchmarks runnable from a single `cargo bench` or script

3. Workloads

   3.1 Diamond DAG (Deriva's sweet spot)
   ```
   A (raw CSV, 100MB)
   ├── B = clean(A)        # 2s compute
   ├── C = normalize(A)    # 3s compute
   └── D = join(B, C)      # 1s compute
   ```
   - Measure: storage with/without D cached, retrieval of D cold vs warm
   - Measure: cascade invalidation when A changes (Deriva vs manual in each system)

   3.2 Deep Linear Chain (stress test)
   ```
   A → B → C → D → E → F → G → H  (8 levels deep)
   ```
   - Measure: cold resolution time (full chain recomputation)
   - Exposes: recomputation latency penalty on deep chains

   3.3 Wide Fan-Out (many dependents)
   ```
   A → [B₁, B₂, ..., B₁₀₀₀]  (1000 derived outputs)
   ```
   - Measure: invalidation time when A changes
   - Measure: storage savings from selective eviction

   3.4 Repeated Computation (memoization)
   ```
   Same recipe submitted 1000 times by different clients
   ```
   - Measure: how many times computation actually executes
   - Compare: Deriva (guaranteed 1) vs DVC vs S3+Airflow

   3.5 Large Blob Streaming (Deriva's weakness)
   ```
   Single 10GB file, sequential read
   ```
   - Measure: raw throughput vs local FS, S3, HDFS
   - Honest result: Deriva adds overhead (gRPC, hashing)

   3.6 Eviction Strategy Comparison
   ```
   Cache full, continuous new data arriving
   ```
   - Compare: Deriva cost-aware vs LRU vs LFU vs random
   - Measure: total recomputation time incurred over workload trace

   3.7 Real-World ML Pipeline
   ```
   Raw data → feature engineering → train/test split → model training → evaluation
   ```
   - End-to-end comparison with DVC and S3+Airflow
   - Measure: storage, latency, reproducibility, developer experience

4. Results

   4.1 Storage Efficiency
   - Deriva bytes on disk vs baseline for each workload
   - Breakdown: recipe overhead, blob storage, cache size

   4.2 Retrieval Latency
   - Cache hit: p50/p95/p99 across all workloads
   - Cache miss (recomputation): p50/p95/p99 by DAG depth
   - Comparison with each target system

   4.3 Write Throughput
   - Leaf ingestion rate (MB/s, ops/s)
   - Recipe registration rate

   4.4 Cascade Invalidation
   - Time to invalidate N dependents (N = 1, 10, 100, 1000)
   - Comparison: Deriva (one API call) vs manual (N deletes + orchestrator retrigger)

   4.5 Eviction Efficiency
   - Storage reclaimed vs recomputation cost incurred
   - Cost-aware vs LRU vs LFU: total recomputation time over 1-hour workload

   4.6 Reproducibility & Determinism
   - Bit-identical results after eviction + recomputation (SHA256 verification)
   - Cross-node determinism: same recipe materialized on 3 different nodes → identical CAddr
   - Cross-platform determinism: same workload on x86_64 vs aarch64 → identical CAddr (with strict float mode)
   - Proof verification: time to verify derivation proof for DAGs of depth 1–20
   - Corruption detection: inject bit-flips, measure detection rate and recovery time
   - Comparison: Deriva (guaranteed + provable) vs DVC (best-effort) vs S3+Airflow (not guaranteed)

   4.7 Distributed Performance (if Phase 3 complete)
   - Compute routing benefit: data transferred per materialization
   - Scaling: throughput vs cluster size (3, 5, 10 nodes)
   - Request hedging: p99 improvement with hedging enabled vs disabled
   - Batch operations: BatchGet(100) throughput vs 100 sequential gets

5. Analysis: When to Use Deriva

   Based on empirical results, provide concrete guidance:
   - ✅ Use Deriva when: DAG-structured workloads, reproducibility required, storage cost matters, repeated computations
   - ❌ Don't use Deriva when: simple blob storage, latency-critical single reads, non-deterministic pipelines
   - ⚖️ Consider Deriva when: ML pipelines, data mesh, scientific computing

6. Threats to Validity
   - Single-machine benchmarks may not reflect production behavior
   - Synthetic workloads vs real-world complexity
   - Comparison fairness (each system optimized differently)
   - Hardware-specific results

7. Conclusion

**Methodology:**
- All benchmarks automated and reproducible (published as `deriva-bench` crate or script suite)
- Each benchmark: minimum 100 runs, report mean, median, p50/p95/p99, standard deviation
- Statistical significance: paired t-test or Wilcoxon signed-rank for comparisons
- Hardware: document exact specs, run on dedicated machine (no background load)
- Comparison targets: use default/recommended configurations (document all settings)
- Data sizes: small (1MB), medium (100MB), large (10GB) variants for each workload

**Master Metrics Table:**
| Metric | Unit | Workloads | Collection |
|--------|------|-----------|------------|
| Storage ratio | Deriva bytes / baseline bytes | All | `du -sh` on data dirs |
| Retrieval latency (hit) | ms, p50/p95/p99 | All | Instrumented client |
| Retrieval latency (miss) | ms, p50/p95/p99 | 3.1, 3.2, 3.7 | Instrumented client |
| Write throughput | MB/s, ops/s | 3.5 | Sustained load test |
| Invalidation time | ms per N dependents | 3.1, 3.3 | Timed cascade |
| Eviction savings | % storage reclaimed | 3.6 | Before/after eviction |
| Recomputation overhead | % slower than cached | 3.2, 3.7 | Cold vs warm ratio |
| Memoization hit rate | % | 3.4 | Counter in cache layer |
| Correctness | bit-identical (yes/no) | All | SHA256 comparison |
| Cross-node determinism | % identical CAddrs | All (distributed) | Same recipe on N nodes |
| Cross-platform determinism | % identical CAddrs | Float workloads | x86_64 vs aarch64 |
| Proof verification time | μs per DAG level | 3.1, 3.2, 3.7 | Standalone verifier |
| Corruption detection rate | % detected | All | Fault injection |
| Scrubber recovery time | s per corrupted object | All | Quarantine + re-fetch timing |
| Data transferred | MB per materialization | 3.1, 3.2 (distributed) | Network counters |

**Predicted Results (to validate or refute):**
| Dimension | Prediction | Confidence |
|-----------|-----------|------------|
| Storage footprint | 40-60% reduction on DAG workloads | High |
| Cache hit latency | Within 2x of Redis, within 5x of local FS | Medium |
| Cache miss latency | 10-100x slower than cache hit (DAG depth dependent) | High |
| Cascade invalidation | 100-1000x faster than manual | High |
| Large blob throughput | 20-40% slower than raw S3/local FS | Medium |
| Eviction efficiency | Cost-aware saves 30-50% recomputation vs LRU | Medium |
| Reproducibility | 100% bit-identical (by design) | High |
| Cross-node determinism | 100% identical CAddrs (with determinism backbone) | High |
| Cross-platform determinism | 100% with strict float mode, <100% with hardware mode | High |
| Proof verification overhead | <1ms per DAG level | Medium |
| Corruption detection | 100% detection rate for single bit-flips | High |

---

## Timeline

| Paper | Depends On | Estimated Write Time | Target |
|-------|-----------|---------------------|--------|
| Paper 1 | Phase 1 | — | ✅ Published |
| Paper 2 | Phase 2 §2.1–§2.6, §2.8 | 2-3 weeks | After Phase 2 |
| Paper 2b | Phase 2 §2.7 | 2-3 weeks | After Phase 2 (parallel with Paper 2) |
| Paper 3 | Phase 3 complete | 3-4 weeks | After Phase 3 |
| Paper 4 | Phase 4 Track A (§4.1–§4.5) | 4-5 weeks | After determinism backbone |
| Paper 5 | Phase 4 Track B (§4.6–§4.10) | 3-4 weeks | After advanced features |
| Paper 6 | All phases + benchmarks | 4-6 weeks | Final paper |

**Total:** 7 papers covering all phases of Deriva development

## Recommended Submission Targets

| Paper | Venue | Rationale |
|-------|-------|-----------|
| Paper 2 | EuroSys / USENIX ATC | Systems/concurrency with novel dedup + GC integration |
| Paper 2b | VLDB / SIGMOD (demo) / EuroSys | Streaming dataflow in CAS — novel intersection |
| Paper 3 | USENIX ATC / EuroSys | Distributed systems with novel routing |
| Paper 4 | OSDI / SOSP | Novel determinism guarantees with proofs — strongest contribution |
| Paper 5 | USENIX ATC / FAST | Systems + filesystem intersection |
| Paper 6 | VLDB / SoCC | Empirical evaluation, practitioner audience |

## Notes

- Papers 2–6 can reference Paper 1 for core concepts — no need to re-explain CAddr, recipes, DAG
- Paper 2b references Paper 2 for persistent DAG and async execution — streaming builds on top of both
- Papers 2 and 2b can be written in parallel since they cover independent Phase 2 subsections
- Paper 4 (determinism) is the strongest academic contribution — consider submitting to a top venue
- Paper 6 should be written last because it needs the complete system for fair benchmarks
- Start collecting metrics from Phase 2 onward (instrument the code as you build)
- All papers published on GitHub Pages under the same Deriva repository
- Paper 4 could also target security venues (USENIX Security, CCS) given the integrity/proof angle
- Paper 2b could also target the Rust community (RustConf, EuroRust) given the async streaming implementation
