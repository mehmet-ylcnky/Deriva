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

**Title:** Async DAG Execution: Concurrent Materialization in a Computation-Addressed Store

**Status:** 📋 Planned (write after Phase 2)

**Phase:** Phase 2 (Robustness)

**Thesis:** Dependency-aware parallelism enables optimal materialization scheduling — independent DAG branches can be computed concurrently, and the persistent DAG allows the system to make scheduling decisions that no external orchestrator can.

**Audience:** Systems/concurrency researchers, async Rust community

**Planned Sections:**

1. Introduction
   - Recap of Deriva's core model (reference Paper 1)
   - The sequential materialization bottleneck in Phase 1
   - Why DAG-aware parallelism is different from generic task parallelism

2. Background: DAG Scheduling
   - Critical path analysis in dependency graphs
   - Comparison with build system parallelism (Bazel, Ninja)
   - Comparison with dataflow engines (Spark, Dask, Ray)

3. Design
   - Persistent DAG store (sled-backed, survives restarts)
   - Async materialization via Tokio tasks
   - Parallel branch detection: when two inputs of a recipe are both unmaterialized, compute concurrently
   - Scheduling heuristics: critical path first vs breadth-first vs cost-weighted

4. Verification Mode
   - Dual-compute for high-assurance workloads
   - Hash comparison to detect non-deterministic functions at runtime
   - Performance cost of verification (2x compute, measured)

5. Observability
   - Structured logging via `tracing` crate
   - Prometheus-compatible metrics export
   - Key metrics: cache hit rate, materialization latency by function, DAG depth distribution, eviction rate
   - Dashboard design for operators

6. Evaluation
   - Benchmark: sequential vs parallel materialization on diamond/wide/deep DAGs
   - Metric: wall-clock time, CPU utilization, memory pressure
   - Benchmark: verification mode overhead (2x compute cost, measured)
   - Benchmark: persistent DAG recovery time after restart

7. Tradeoffs
   - Async complexity vs performance gain
   - Verification mode cost vs safety guarantee
   - Observability overhead

8. Conclusion

**Methodology:**
- Controlled benchmarks on fixed hardware (document CPU, RAM, SSD specs)
- DAG shapes: diamond (depth 3, width 2), wide fan-out (1→100), deep chain (depth 20), realistic ML pipeline (mixed)
- Each benchmark: 100 runs, report p50/p95/p99
- Compare: Phase 1 sequential executor vs Phase 2 parallel executor on identical workloads

**Key Metrics to Collect:**
| Metric | Unit | Collection Method |
|--------|------|-------------------|
| Materialization wall time | ms | Instrumented executor, per-request |
| Parallelism factor | concurrent tasks / total tasks | Tokio task count during materialization |
| DAG recovery time | ms | Time from process start to DAG fully loaded from sled |
| Verification overhead | % | Dual-compute time / single-compute time |
| Cache hit rate | % | Counter in cache layer |
| Materialization latency by function | ms | Per-function histogram via tracing |

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
   - Reference Papers 1 & 2 for single-node foundation

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
| Paper 2 | Phase 2 complete | 2-3 weeks | After Phase 2 |
| Paper 3 | Phase 3 complete | 3-4 weeks | After Phase 3 |
| Paper 4 | Phase 4 Track A (§4.1–§4.5) | 4-5 weeks | After determinism backbone |
| Paper 5 | Phase 4 Track B (§4.6–§4.10) | 3-4 weeks | After advanced features |
| Paper 6 | All phases + benchmarks | 4-6 weeks | Final paper |

**Total:** 6 papers covering all phases of Deriva development

## Recommended Submission Targets

| Paper | Venue | Rationale |
|-------|-------|-----------|
| Paper 3 | USENIX ATC / EuroSys | Distributed systems with novel routing |
| Paper 4 | OSDI / SOSP | Novel determinism guarantees with proofs — strongest contribution |
| Paper 5 | USENIX ATC / FAST | Systems + filesystem intersection |
| Paper 6 | VLDB / SoCC | Empirical evaluation, practitioner audience |

## Notes

- Papers 2-5 can reference Paper 1 for core concepts — no need to re-explain CAddr, recipes, DAG
- Paper 4 (determinism) is the strongest academic contribution — consider submitting to a top venue
- Paper 6 should be written last because it needs the complete system for fair benchmarks
- Start collecting metrics from Phase 2 onward (instrument the code as you build)
- All papers published on GitHub Pages under the same Deriva repository
- Paper 4 could also target security venues (USENIX Security, CCS) given the integrity/proof angle
