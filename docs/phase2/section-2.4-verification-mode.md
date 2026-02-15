# Section 2.4: Verification Mode

> **Goal:** Add an optional dual-compute mode that executes each recipe twice and
> compares results to detect non-deterministic compute functions.
>
> **Crate:** `deriva-compute` (modify `async_executor.rs`), `deriva-server` (config flag)
>
> **Estimated tests:** ~14
>
> **Commit message pattern:** `2.4: Verification Mode — dual-compute determinism checking`

---

## 1. Problem Statement

Deriva's core invariant is **computation addressing**: the same recipe (function + inputs + params)
MUST always produce the same output. The content address IS the identity of the data.

If a `ComputeFunction` is non-deterministic (uses random numbers, timestamps, unordered
iteration, floating-point non-associativity), the system silently produces wrong results:

```
Recipe R = hash_fn(input_a, input_b)

First compute:  R → 0xabc123...  (cached)
Second compute: R → 0xdef456...  (different! but cache returns 0xabc123)

Client gets 0xabc123 — but if cache is evicted and R is recomputed,
client gets 0xdef456. The "same" address now returns different data.
```

This violates the fundamental guarantee. We need a way to detect this.

### Why Not Just Require Determinism?

We DO require it — `ComputeFunction` implementations MUST be deterministic. But:
- Developers make mistakes (e.g., iterating a `HashMap` which has random order)
- Third-party functions may have hidden non-determinism
- Floating-point operations can vary across CPU architectures
- We want to VERIFY the invariant, not just trust it

### When to Use Verification Mode

- **Development/testing:** Always on — catch bugs early
- **CI/CD:** Always on — gate deployments on determinism
- **Production:** Off by default (2x compute cost), enable for auditing
- **After function updates:** Enable temporarily to verify new function versions

### Common Sources of Non-Determinism

Understanding what causes non-determinism helps developers write correct functions
and helps us design better error messages.

| Source | Example | Detection |
|--------|---------|-----------|
| Random number generators | `rand::thread_rng()` | Always caught by DualCompute |
| Timestamps | `SystemTime::now()` | Always caught (different μs) |
| HashMap iteration order | `map.iter()` in output | Caught if order affects output bytes |
| Floating-point non-associativity | `(a+b)+c ≠ a+(b+c)` | Caught if parallel reduction used |
| Thread scheduling | Race conditions in compute | Intermittently caught |
| Uninitialized memory | Unsafe code reading padding | Intermittently caught |
| External I/O | Network calls, file reads | Always caught if external state changes |
| Process ID / thread ID | Using `std::process::id()` | Always caught |
| Memory addresses | Pointer-to-int in output | Always caught |
| Counter/sequence generators | Global `AtomicU64` counter | Always caught |

**Safe patterns** (always deterministic):
- Pure functions on input bytes
- Deterministic hashing (blake3, SHA-256)
- Sorting before output
- `BTreeMap` instead of `HashMap` for ordered iteration
- Fixed-seed RNG (if seed is derived from inputs)

### Non-Determinism Debugging Guide

When a `DeterminismViolation` is detected, the error includes:

```
DeterminismViolation for caddr(0xabc123...):
  function: transform/v1
  output_1: 1024 bytes, hash=0xdef456...
  output_2: 1024 bytes, hash=0x789abc...
```

Debugging steps:
1. **Same length, different hash:** Content differs — likely HashMap ordering or floating-point
2. **Different length:** Likely a buffer/allocation issue or conditional logic with side effects
3. **Reproducible:** Run with `DualCompute` again — if it fails consistently, the function
   is fundamentally non-deterministic
4. **Intermittent:** Race condition or timing-dependent — harder to debug, use `Sampled(1.0)`
   with many iterations to increase detection probability

---

## 2. Design

### 2.2.1 Architecture

```
Normal Mode:                        Verification Mode:

materialize(addr)                   materialize(addr)
    │                                   │
    ├── resolve inputs                  ├── resolve inputs
    │                                   │
    ├── func.execute(inputs)            ├── func.execute(inputs) → result_1
    │         │                         ├── func.execute(inputs) → result_2
    │         ▼                         │         │
    │     result                        │    result_1 == result_2 ?
    │         │                         │    ├── Yes → return result_1
    │         ▼                         │    └── No  → DeterminismViolation!
    └── cache.put(result)               │
                                        └── cache.put(result_1)
```

### 2.2.2 Verification Levels

Three levels of verification, configurable per-executor:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerificationMode {
    /// No verification — single compute (default, production)
    Off,
    /// Dual compute — execute twice, compare byte-for-byte
    DualCompute,
    /// Statistical — verify N% of materializations (sampling)
    Sampled { rate: f64 },  // 0.0 to 1.0
}
```

- **Off:** Zero overhead, single execution
- **DualCompute:** 2x compute cost, catches all non-determinism
- **Sampled { rate: 0.1 }:** 10% of materializations are dual-computed, 1.1x average cost

### 2.2.3 Verification Scope

Verification applies ONLY to the `func.execute()` step, not to:
- Cache lookups (deterministic by definition)
- Leaf reads (raw data, no computation)
- DAG traversal (deterministic graph operations)

```
materialize(addr)
    │
    ├── cache.get()        ← NOT verified (no computation)
    ├── leaf_store.get()   ← NOT verified (raw data)
    ├── dag.get_recipe()   ← NOT verified (graph lookup)
    ├── resolve inputs     ← NOT verified (recursive, each node verified individually)
    │
    ├── func.execute()     ← VERIFIED (this is where non-determinism can occur)
    │
    └── cache.put()        ← NOT verified (stores verified result)
```

### 2.2.4 Parallel Dual Compute

With §2.3's parallel infrastructure, the two executions run concurrently:

```rust
let (result_1, result_2) = tokio::join!(
    spawn_blocking(|| func.execute(inputs.clone(), &params)),
    spawn_blocking(|| func.execute(inputs, &params)),
);
```

This means verification mode costs ~1x wall-clock time (not 2x) on multi-core systems,
though it uses 2x CPU.

```
Normal:       [====== compute ======]                    Time: T
                                                         CPU:  T

Verification: [====== compute_1 ======]                  Time: T (parallel)
              [====== compute_2 ======]                  CPU:  2T
```

---

## 3. Implementation

### 3.1 Config Changes

```rust
// Add to ExecutorConfig in async_executor.rs

pub struct ExecutorConfig {
    pub max_concurrency: usize,
    pub dedup_channel_capacity: usize,
    pub verification: VerificationMode,  // NEW
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_concurrency: num_cpus::get() * 2,
            dedup_channel_capacity: 16,
            verification: VerificationMode::Off,
        }
    }
}
```

### 3.2 Verification in AsyncExecutor

Add a helper method that wraps the compute step:

```rust
impl<C, L, D> AsyncExecutor<C, L, D>
where
    C: AsyncMaterializationCache + 'static,
    L: AsyncLeafStore + 'static,
    D: DagReader + 'static,
{
    /// Execute a compute function with optional verification.
    async fn execute_verified(
        &self,
        addr: &CAddr,
        func: Arc<dyn ComputeFunction>,
        input_bytes: Vec<Bytes>,
        params: &BTreeMap<String, Value>,
    ) -> Result<Bytes> {
        let should_verify = match self.config.verification {
            VerificationMode::Off => false,
            VerificationMode::DualCompute => true,
            VerificationMode::Sampled { rate } => {
                // Use addr bytes as deterministic seed for sampling
                // This ensures the same addr is always/never verified (reproducible)
                let sample = (addr.as_bytes()[0] as f64) / 255.0;
                sample < rate
            }
        };

        if !should_verify {
            // Normal path — single execution
            let params = params.clone();
            return tokio::task::spawn_blocking(move || {
                func.execute(input_bytes, &params)
            })
            .await
            .map_err(|e| DerivaError::ComputeFailed(format!("join: {}", e)))?
            .map_err(|e| DerivaError::ComputeFailed(e.to_string()));
        }

        // Verification path — dual execution
        let func2 = Arc::clone(&func);
        let inputs2 = input_bytes.clone();
        let params1 = params.clone();
        let params2 = params.clone();

        let (result_1, result_2) = tokio::join!(
            tokio::task::spawn_blocking(move || func.execute(input_bytes, &params1)),
            tokio::task::spawn_blocking(move || func2.execute(inputs2, &params2)),
        );

        let output_1 = result_1
            .map_err(|e| DerivaError::ComputeFailed(format!("join: {}", e)))?
            .map_err(|e| DerivaError::ComputeFailed(e.to_string()))?;
        let output_2 = result_2
            .map_err(|e| DerivaError::ComputeFailed(format!("join: {}", e)))?
            .map_err(|e| DerivaError::ComputeFailed(e.to_string()))?;

        if output_1 != output_2 {
            return Err(DerivaError::DeterminismViolation {
                addr: *addr,
                function_id: func2.id(), // need to capture before move
                output_1_hash: blake3::hash(&output_1).to_hex().to_string(),
                output_2_hash: blake3::hash(&output_2).to_hex().to_string(),
                output_1_len: output_1.len(),
                output_2_len: output_2.len(),
            });
        }

        Ok(output_1)
    }
}
```

### 3.3 Updated materialize()

Replace the compute step in `materialize()`:

```rust
// In AsyncExecutor::materialize, replace step 8:

// Before (§2.3):
let func = self.registry.get(&recipe.function_id)
    .ok_or_else(|| DerivaError::FunctionNotFound(...))?;
let params = recipe.params.clone();
let output = tokio::task::spawn_blocking(move || {
    func.execute(input_bytes, &params)
}).await??;

// After (§2.4):
let func = self.registry.get(&recipe.function_id)
    .ok_or_else(|| DerivaError::FunctionNotFound(...))?;
let output = self.execute_verified(
    &addr, func, input_bytes, &recipe.params
).await?;
```

### 3.4 New Error Variant

```rust
// deriva-core/src/error.rs — add variant

#[derive(Debug, Clone, thiserror::Error)]
pub enum DerivaError {
    // ... existing variants ...

    #[error("determinism violation for {addr}: function {function_id} produced \
             different outputs ({output_1_len} bytes hash={output_1_hash} vs \
             {output_2_len} bytes hash={output_2_hash})")]
    DeterminismViolation {
        addr: CAddr,
        function_id: FunctionId,
        output_1_hash: String,
        output_2_hash: String,
        output_1_len: usize,
        output_2_len: usize,
    },
}
```

### 3.5 Server Configuration

```rust
// deriva-server/src/main.rs or config — add CLI flag

#[derive(clap::Parser)]
struct ServerArgs {
    // ... existing args ...

    /// Enable verification mode: off, dual, sampled:RATE
    #[arg(long, default_value = "off")]
    verification: String,
}

fn parse_verification(s: &str) -> VerificationMode {
    match s {
        "off" => VerificationMode::Off,
        "dual" => VerificationMode::DualCompute,
        s if s.starts_with("sampled:") => {
            let rate: f64 = s[8..].parse().expect("invalid sample rate");
            VerificationMode::Sampled { rate }
        }
        _ => panic!("invalid verification mode: {}", s),
    }
}
```

Usage:
```bash
# Development — always verify
deriva-server --verification dual

# Production — sample 5%
deriva-server --verification sampled:0.05

# Production — no verification (default)
deriva-server
```

### 3.6 Verification Metrics

Track verification results for observability (ties into §2.5):

```rust
pub struct VerificationStats {
    pub total_verified: AtomicU64,
    pub total_passed: AtomicU64,
    pub total_failed: AtomicU64,
    pub last_failure: Mutex<Option<DeterminismViolation>>,
}

impl VerificationStats {
    pub fn record_pass(&self) {
        self.total_verified.fetch_add(1, Ordering::Relaxed);
        self.total_passed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_fail(&self, violation: DeterminismViolation) {
        self.total_verified.fetch_add(1, Ordering::Relaxed);
        self.total_failed.fetch_add(1, Ordering::Relaxed);
        *self.last_failure.lock().unwrap() = Some(violation);
    }

    pub fn failure_rate(&self) -> f64 {
        let total = self.total_verified.load(Ordering::Relaxed);
        if total == 0 { return 0.0; }
        self.total_failed.load(Ordering::Relaxed) as f64 / total as f64
    }
}
```

---

## 4. Data Flow Diagrams

### 4.1 Verification Decision Flow

```
execute_verified(addr, func, inputs, params)
    │
    ├── match verification_mode:
    │     │
    │     ├── Off ──────────────────────────────────────┐
    │     │     spawn_blocking(func.execute(inputs))    │
    │     │     return result                           │
    │     │                                             │
    │     ├── DualCompute ──────────────────────────┐   │
    │     │     tokio::join!(                       │   │
    │     │       spawn_blocking(func.execute(in1)),│   │
    │     │       spawn_blocking(func.execute(in2)) │   │
    │     │     )                                   │   │
    │     │     compare result_1 == result_2        │   │
    │     │     ├── equal → return result_1         │   │
    │     │     └── differ → DeterminismViolation!  │   │
    │     │                                         │   │
    │     └── Sampled { rate } ─────────────────┐   │   │
    │           addr_byte / 255 < rate ?         │   │   │
    │           ├── Yes → (same as DualCompute)  │   │   │
    │           └── No  → (same as Off)          │   │   │
    │                                            │   │   │
    └────────────────────────────────────────────┘───┘───┘
```

### 4.2 Sampled Verification Distribution

```
CAddr bytes[0] distribution (uniform 0-255):

rate = 0.10 (10%):
  Verified:   bytes[0] < 25   (addrs 0x00.. to 0x19..)
  Unverified: bytes[0] >= 25  (addrs 0x1a.. to 0xff..)

rate = 0.50 (50%):
  Verified:   bytes[0] < 128  (addrs 0x00.. to 0x7f..)
  Unverified: bytes[0] >= 128 (addrs 0x80.. to 0xff..)

Using addr bytes as seed ensures:
  - Same addr is always verified or always not (reproducible)
  - Distribution is uniform (blake3 output is uniform)
  - No external RNG needed (deterministic sampling!)
```

### 4.3 Parallel Dual Compute Timeline

```
Normal mode:
Time ──────────────────────────────────────▶
Thread 1: [====== func.execute(inputs) ======]
                                              Total: T

Verification mode (parallel):
Time ──────────────────────────────────────▶
Thread 1: [====== func.execute(inputs) ======]
Thread 2: [====== func.execute(inputs) ======]
                                              Total: T (wall), 2T (CPU)

Verification mode (if single-threaded):
Time ──────────────────────────────────────▶
Thread 1: [====== exec 1 ======][====== exec 2 ======]
                                              Total: 2T (wall), 2T (CPU)
```

---

## 5. Test Specification

### 5.1 Unit Tests

Test helpers:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Deterministic function that counts invocations.
struct CountingIdentity {
    count: Arc<AtomicU64>,
}

impl ComputeFunction for CountingIdentity {
    fn id(&self) -> FunctionId { FunctionId::new("counting_identity", "v1") }
    fn execute(&self, inputs: Vec<Bytes>, _: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        self.count.fetch_add(1, Ordering::SeqCst);
        Ok(inputs.into_iter().next().unwrap_or_default())
    }
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 1, memory_bytes: 0 }
    }
}

/// Non-deterministic function — returns random bytes.
struct NonDeterministicFn;

impl ComputeFunction for NonDeterministicFn {
    fn id(&self) -> FunctionId { FunctionId::new("nondeterministic", "v1") }
    fn execute(&self, _: Vec<Bytes>, _: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        use std::time::SystemTime;
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH).unwrap()
            .subsec_nanos();
        Ok(Bytes::from(nanos.to_le_bytes().to_vec()))
    }
    fn estimated_cost(&self, _: &[u64]) -> ComputeCost {
        ComputeCost { cpu_ms: 1, memory_bytes: 0 }
    }
}
```

Test cases:

```
#[tokio::test]
test_verification_off_single_execution
    let count = Arc::new(AtomicU64::new(0));
    // Register CountingIdentity, create executor with Off
    executor.materialize(recipe_addr).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);

#[tokio::test]
test_verification_dual_executes_twice
    let count = Arc::new(AtomicU64::new(0));
    // Register CountingIdentity, create executor with DualCompute
    executor.materialize(recipe_addr).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);

#[tokio::test]
test_verification_dual_deterministic_passes
    // identity function — always deterministic
    let result = executor.materialize(recipe_addr).await;
    assert!(result.is_ok());

#[tokio::test]
test_verification_dual_nondeterministic_fails
    // Register NonDeterministicFn
    // Add small sleep in execute to ensure different timestamps
    let result = executor.materialize(recipe_addr).await;
    assert!(matches!(result, Err(DerivaError::DeterminismViolation { .. })));

#[tokio::test]
test_verification_dual_error_includes_details
    let result = executor.materialize(recipe_addr).await;
    if let Err(DerivaError::DeterminismViolation {
        addr, function_id, output_1_hash, output_2_hash, ..
    }) = result {
        assert_eq!(addr, recipe_addr);
        assert_eq!(function_id, FunctionId::new("nondeterministic", "v1"));
        assert_ne!(output_1_hash, output_2_hash);
    } else {
        panic!("expected DeterminismViolation");
    }

#[tokio::test]
test_verification_sampled_respects_rate
    let count = Arc::new(AtomicU64::new(0));
    // Sampled { rate: 0.5 }, materialize 256 different recipes
    // (one for each possible bytes[0] value)
    let verified = count.load(Ordering::SeqCst);
    // ~128 should be verified (executed twice), ~128 not
    // Total executions: ~128*2 + 128*1 = ~384
    assert!(verified > 300 && verified < 450);

#[tokio::test]
test_verification_sampled_deterministic_per_addr
    // Sampled { rate: 0.5 }
    // Materialize addr_X, note if verified (count == 2)
    // Evict cache, materialize addr_X again
    // Should be verified again (or not) — same decision both times

#[tokio::test]
test_verification_sampled_zero_rate
    // Sampled { rate: 0.0 }
    let count = Arc::new(AtomicU64::new(0));
    executor.materialize(recipe_addr).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1); // never verified

#[tokio::test]
test_verification_sampled_one_rate
    // Sampled { rate: 1.0 }
    let count = Arc::new(AtomicU64::new(0));
    executor.materialize(recipe_addr).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2); // always verified

#[tokio::test]
test_verification_stats_tracking
    // DualCompute, materialize 5 deterministic recipes
    assert_eq!(stats.total_verified.load(Ordering::SeqCst), 5);
    assert_eq!(stats.total_passed.load(Ordering::SeqCst), 5);
    assert_eq!(stats.total_failed.load(Ordering::SeqCst), 0);
    assert_eq!(stats.failure_rate(), 0.0);

#[tokio::test]
test_verification_stats_failure_recorded
    // DualCompute, materialize non-deterministic recipe
    assert_eq!(stats.total_failed.load(Ordering::SeqCst), 1);
    assert!(stats.last_failure.lock().unwrap().is_some());

#[tokio::test]
test_verification_cached_result_not_reverified
    let count = Arc::new(AtomicU64::new(0));
    // DualCompute
    executor.materialize(recipe_addr).await.unwrap(); // 2 executions
    executor.materialize(recipe_addr).await.unwrap(); // cache hit, 0 executions
    assert_eq!(count.load(Ordering::SeqCst), 2); // not 4

#[tokio::test]
test_verification_parallel_inputs_each_verified
    let count = Arc::new(AtomicU64::new(0));
    // DualCompute, recipe with 3 inputs (each a recipe)
    // 3 input recipes × 2 + 1 final recipe × 2 = 8 executions
    executor.materialize(final_addr).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 8);
```

### 5.2 Integration Tests

```
#[tokio::test]
test_verification_mode_server_flag
    Start server with --verification dual
    Put leaf + recipe with deterministic function
    Get should succeed

test_verification_mode_rejects_nondeterministic
    Start server with --verification dual
    Register non-deterministic function
    Put leaf + recipe
    Get should return error with DeterminismViolation details
```

---

## 6. Edge Cases & Error Handling

| Case | Expected Behavior |
|------|-------------------|
| Function returns same bytes but different Bytes allocation | Passes — comparison is by value, not pointer |
| Function returns empty bytes both times | Passes — empty == empty |
| Function panics on second execution | `spawn_blocking` catches panic → `ComputeFailed` |
| First execution succeeds, second fails with error | Return the error (not DeterminismViolation) |
| Both executions fail with different errors | Return first error |
| Verification of cached result | Skip — cache hits bypass verification |
| Sampled rate > 1.0 | Clamp to 1.0 (always verify) |
| Sampled rate < 0.0 | Clamp to 0.0 (never verify) |
| Very large output (1GB) — comparing twice | Memory cost: 2x output size during comparison |

---

## 7. Performance Expectations

| Mode | CPU Cost | Wall-Clock Cost | Memory Cost |
|------|----------|-----------------|-------------|
| Off | 1x | 1x | 1x |
| DualCompute | 2x | ~1x (parallel) | ~2x during compute |
| Sampled(0.1) | 1.1x | ~1x | ~1x average |
| Sampled(0.5) | 1.5x | ~1x | ~1.5x average |

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-compute/src/async_executor.rs` | Add `VerificationMode`, `execute_verified()`, `VerificationStats` |
| `deriva-core/src/error.rs` | Add `DeterminismViolation` variant |
| `deriva-server/src/main.rs` | Add `--verification` CLI flag |
| `deriva-compute/Cargo.toml` | Add `blake3` dependency (for error hashes) |
| `deriva-compute/tests/verification.rs` | **NEW** — ~14 unit tests |
| `deriva-server/tests/integration.rs` | Add ~2 verification integration tests |

---

## 9. gRPC Protocol Extension

### 9.1 Status RPC Enhancement

Add verification stats to the `StatusResponse` proto message:

```protobuf
// proto/deriva.proto — add fields to StatusResponse

message StatusResponse {
    uint64 recipe_count = 1;
    uint64 blob_count = 2;
    uint64 cache_entries = 3;
    uint64 cache_size_bytes = 4;
    double cache_hit_rate = 5;
    // NEW — verification stats
    string verification_mode = 6;       // "off", "dual", "sampled:0.10"
    uint64 verification_total = 7;
    uint64 verification_passed = 8;
    uint64 verification_failed = 9;
    double verification_failure_rate = 10;
}
```

### 9.2 Verify RPC (Optional)

Add a dedicated RPC to verify a specific addr on demand:

```protobuf
message VerifyRequest {
    bytes addr = 1;
}

message VerifyResponse {
    bool deterministic = 1;
    string output_hash = 2;       // hash of the verified output
    uint64 output_size = 3;
    uint64 compute_time_us = 4;   // microseconds for dual compute
    string error = 5;             // non-empty if verification failed
}

service Deriva {
    // ... existing RPCs ...
    rpc Verify(VerifyRequest) returns (VerifyResponse);
}
```

This allows clients to explicitly verify a specific recipe without enabling
global verification mode. Useful for:
- Spot-checking after function updates
- CI/CD verification of specific critical recipes
- Debugging reported non-determinism

### 9.3 CLI Extension

```bash
# Verify a specific addr
deriva verify <addr>

# Output:
# ✓ caddr(0xabc123...) is deterministic
#   function: transform/v1
#   output: 1024 bytes (hash: 0xdef456...)
#   compute time: 45ms (dual)

# Or on failure:
# ✗ caddr(0xabc123...) is NON-DETERMINISTIC!
#   function: transform/v1
#   output_1: 1024 bytes (hash: 0xdef456...)
#   output_2: 1024 bytes (hash: 0x789abc...)
#   compute time: 48ms (dual)
```

---

## 10. Benchmarking Plan

### 10.1 Verification Overhead Benchmark

```rust
fn bench_verification_overhead(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("verification_overhead");

    for mode in ["off", "dual", "sampled_10", "sampled_50"] {
        let config = match mode {
            "off" => ExecutorConfig { verification: VerificationMode::Off, ..Default::default() },
            "dual" => ExecutorConfig { verification: VerificationMode::DualCompute, ..Default::default() },
            "sampled_10" => ExecutorConfig { verification: VerificationMode::Sampled { rate: 0.1 }, ..Default::default() },
            "sampled_50" => ExecutorConfig { verification: VerificationMode::Sampled { rate: 0.5 }, ..Default::default() },
            _ => unreachable!(),
        };
        let (executor, addr) = setup_benchmark_dag(config);

        group.bench_function(mode, |b| {
            b.iter(|| {
                rt.block_on(async { executor.materialize(addr).await.unwrap() })
            });
        });
    }
    group.finish();
}
```

### 10.2 Expected Results

```
verification_overhead/off          time: [100 ms 105 ms 110 ms]  ← baseline
verification_overhead/dual         time: [105 ms 110 ms 115 ms]  ← ~5% wall-clock overhead
verification_overhead/sampled_10   time: [101 ms 106 ms 111 ms]  ← ~1% overhead
verification_overhead/sampled_50   time: [103 ms 108 ms 113 ms]  ← ~3% overhead
```

Wall-clock overhead is minimal because dual compute runs in parallel on separate
blocking threads. The overhead is just the comparison + `tokio::join!` coordination.

CPU overhead (not measured by wall-clock):
- Off: 1x
- Dual: 2x
- Sampled(0.1): 1.1x
- Sampled(0.5): 1.5x

---

## 11. Design Rationale

### 11.1 Why Byte-for-Byte Comparison Instead of Hash Comparison?

Comparing `output_1 == output_2` (byte-for-byte) is simpler and catches ALL differences.
Hash comparison (`blake3(o1) == blake3(o2)`) would miss hash collisions (astronomically
unlikely with blake3, but why add the complexity?).

For the error message, we DO hash the outputs to provide a compact representation.

### 9.2 Why Deterministic Sampling Instead of Random?

Using `addr.as_bytes()[0]` as the sampling seed means:
- The same addr is always verified or never verified (reproducible debugging)
- No external RNG dependency
- No Mutex for RNG state
- Uniform distribution (blake3 output bytes are uniform)

Random sampling would mean a flaky function might pass sometimes and fail sometimes
for the same addr, making debugging harder.

### 9.3 Why Not Verify at Insert Time?

We could verify when a recipe is first inserted (compute it twice immediately). But:
- Recipes may reference inputs that don't exist yet
- Insert should be fast (just store metadata)
- Verification is meaningful only during materialization (when inputs are available)

### 9.4 Relationship to §2.5 (Observability)

Verification stats feed into the Prometheus metrics from §2.5:

```
deriva_verification_total{result="pass"} 1234
deriva_verification_total{result="fail"} 2
deriva_verification_failure_rate 0.0016
```

---

## 10. Checklist

- [ ] Add `VerificationMode` enum (Off, DualCompute, Sampled)
- [ ] Add `verification` field to `ExecutorConfig`
- [ ] Implement `execute_verified()` method
- [ ] Add `DeterminismViolation` error variant with details
- [ ] Implement parallel dual compute with `tokio::join!`
- [ ] Implement deterministic sampling using addr bytes
- [ ] Add `VerificationStats` struct
- [ ] Add `--verification` CLI flag to server
- [ ] Add verification fields to `StatusResponse` proto
- [ ] Add `Verify` RPC to proto (optional)
- [ ] Add `verify` CLI command (optional)
- [ ] Write `CountingIdentity` and `NonDeterministicFn` test helpers
- [ ] Write unit tests (~14)
- [ ] Write integration tests (~2)
- [ ] Run full test suite
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push

---

## 11. Rollout Guide

### 11.1 Enabling Verification in Existing Deployments

```bash
# Step 1: Enable sampled verification at low rate
deriva-server --verification sampled:0.01  # 1% of materializations

# Step 2: Monitor for failures via Status RPC
deriva status
# verification_mode: sampled:0.01
# verification_total: 10000
# verification_passed: 10000
# verification_failed: 0

# Step 3: If clean, increase rate
deriva-server --verification sampled:0.10  # 10%

# Step 4: For full confidence, run dual for a period
deriva-server --verification dual

# Step 5: Once confident, disable for production performance
deriva-server  # defaults to --verification off
```

### 11.2 CI/CD Integration

```yaml
# .github/workflows/verify.yml
- name: Start Deriva with verification
  run: |
    deriva-server --verification dual &
    sleep 2

- name: Run test workload
  run: |
    deriva put test_data.bin
    deriva recipe --function transform/v1 --inputs $LEAF_ADDR
    deriva get $RECIPE_ADDR > /dev/null

- name: Check verification stats
  run: |
    STATUS=$(deriva status --json)
    FAILURES=$(echo $STATUS | jq '.verification_failed')
    if [ "$FAILURES" -gt "0" ]; then
      echo "DETERMINISM VIOLATION DETECTED"
      exit 1
    fi
```

### 11.3 Function Development Workflow

When developing a new `ComputeFunction`:

1. Write the function implementation
2. Register it in the function registry
3. Start server with `--verification dual`
4. Run representative workloads
5. Check `deriva status` for verification failures
6. If failures: debug using the error details (hashes, sizes)
7. Fix the non-determinism source (see §1 taxonomy table)
8. Repeat until clean
9. Submit for review with verification results as evidence

### 11.4 Performance Impact Assessment

Before enabling verification in production, measure the impact:

```bash
# Baseline (no verification)
wrk -t4 -c16 -d30s http://localhost:50051/get/test_addr
# Requests/sec: 1000

# With dual verification
# Requests/sec: ~950 (5% wall-clock overhead, but 2x CPU)

# With sampled 10%
# Requests/sec: ~990 (1% overhead)
```

The wall-clock impact is minimal because dual compute runs on separate threads.
The main concern is CPU utilization — monitor CPU usage when enabling verification
to ensure the host has sufficient headroom.

---

## 12. Future Extensions

### 12.1 Triple Compute (N-way Verification)

For extremely high-assurance environments, extend to N-way verification:

```rust
pub enum VerificationMode {
    Off,
    DualCompute,
    NCompute { n: usize },  // execute N times, majority vote
    Sampled { rate: f64 },
}
```

With `NCompute { n: 3 }`, the function executes 3 times. If 2/3 agree, the majority
result is used. If all 3 differ, `DeterminismViolation` is raised. This catches
intermittent non-determinism that dual compute might miss on a single run.

Cost: Nx compute, but still ~1x wall-clock with sufficient CPU cores.

### 12.2 Verification Audit Log

Persist verification results to a separate sled tree for post-hoc auditing:

```rust
// Tree: "verification_log"
// Key: timestamp_ns (u64 big-endian) ++ addr (32 bytes)
// Value: bincode(VerificationRecord)

struct VerificationRecord {
    addr: CAddr,
    function_id: FunctionId,
    passed: bool,
    output_hash: [u8; 32],
    compute_time_us: u64,
    timestamp: u64,
}
```

This enables:
- Historical analysis of verification results
- Identifying functions with intermittent non-determinism
- Compliance reporting ("all materializations in the last 30 days were verified")

### 12.3 Per-Function Verification Override

Allow verification mode to be set per-function, not just globally:

```rust
pub trait ComputeFunction: Send + Sync {
    fn id(&self) -> FunctionId;
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError>;
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost;

    /// Override global verification mode for this function.
    /// Returns None to use the global setting.
    fn verification_override(&self) -> Option<VerificationMode> {
        None  // default: use global setting
    }
}
```

Use cases:
- Known-deterministic functions (pure math): `Some(VerificationMode::Off)` — skip verification
- Known-risky functions (FFI, floating-point): `Some(VerificationMode::DualCompute)` — always verify
- New/untested functions: `Some(VerificationMode::DualCompute)` during rollout

### 12.4 Cross-Node Verification (Phase 3)

In the distributed setting (Phase 3), verification can be extended to cross-node:
compute the recipe on two different nodes and compare results. This catches:
- Architecture-dependent floating-point differences (x86 vs ARM)
- OS-dependent behavior
- Library version differences across nodes

This is significantly more expensive (network round-trip + remote compute) but provides
the strongest determinism guarantee.
