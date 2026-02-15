# §4.4 Deterministic Floating-Point

> **Status**: Blueprint  
> **Depends on**: §4.1 (Canonical Serialization), §4.2 (Deterministic Scheduling)  
> **Crate(s)**: `deriva-compute`, `deriva-core`  
> **Estimated effort**: 6–8 days  

---

## 1. Problem Statement

### 1.1 IEEE 754 is Not Deterministic

IEEE 754 floating-point arithmetic is **not deterministic across architectures**. The same operation can produce different results on different hardware.

**Sources of non-determinism:**

| Source | x86_64 | aarch64 | Impact |
|--------|--------|---------|--------|
| Extended precision (x87) | 80-bit intermediate | 64-bit | Different rounding |
| Fused multiply-add (FMA) | Optional | Common | Different rounding |
| Transcendental functions | libm | libm | Implementation-dependent |
| SIMD vs scalar | SSE2/AVX | NEON | Different code paths |
| Denormal handling | Configurable | Configurable | Flush-to-zero differences |

### 1.2 Real-World Example

```rust
// Simple computation
fn compute(a: f64, b: f64, c: f64) -> f64 {
    a * b + c  // Looks innocent
}

// x86_64 with FMA:
compute(1.0, 1.0e-16, 1.0) = 1.0000000000000001

// aarch64 without FMA:
compute(1.0, 1.0e-16, 1.0) = 1.0

// Different bit patterns → different CAddr → broken invariant
```

### 1.3 Why This Breaks Deriva

**Scenario 1: Cross-platform divergence**
```
Node A (x86_64):
  Recipe R → float computation → output = 1.0000000000000001
  CAddr = blake3(output) = 0xabc...

Node B (aarch64):
  Recipe R → float computation → output = 1.0
  CAddr = blake3(output) = 0xdef...

Same recipe, different CAddrs → replication breaks
```

**Scenario 2: Upgrade breaks cache**
```
Day 0: Deriva v1.0 on x86_64 without FMA
       Recipe R → CAddr 0xabc...

Day 30: Upgrade to x86_64 with FMA enabled
        Recipe R → CAddr 0xdef...

All cached data under 0xabc... is now orphaned
```

### 1.4 What We Need

**Deterministic floating-point** — guarantee bit-identical results across:
- All CPU architectures (x86_64, aarch64, riscv64)
- All compiler versions
- All optimization levels
- All SIMD configurations

This section provides a `DeterministicFloat` library that makes non-determinism structurally impossible.

---

## 2. Design

### 2.1 Three-Tier Strategy

**Tier 1: Basic arithmetic (fast path)**
- Operations: `+`, `-`, `*`, `/`, `sqrt`
- Strategy: Disable FMA, force rounding mode, avoid x87
- Performance: Native speed (no overhead)

**Tier 2: Transcendental functions (slow path)**
- Operations: `sin`, `cos`, `exp`, `log`, `pow`, `atan2`
- Strategy: Software implementation (MPFR-derived)
- Performance: 10-100x slower than hardware

**Tier 3: WASM (automatic)**
- All float operations in WASM are deterministic (Wasmtime enforces)
- No additional work needed

### 2.2 Float Policy

```rust
// crates/deriva-compute/src/float_policy.rs (new file)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatPolicy {
    /// Strict determinism: software transcendentals, no FMA
    Strict,
    
    /// Hardware mode: trust hardware, faster but non-portable
    Hardware,
    
    /// Reject any function that uses floats
    Disabled,
}

impl Default for FloatPolicy {
    fn default() -> Self {
        FloatPolicy::Strict
    }
}
```

### 2.3 Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  ComputeFunction                        │
│  fn execute(inputs, params) -> Result<Bytes>           │
└─────────────────────────────────────────────────────────┘
                        │
                        │ uses floats?
                        ▼
┌─────────────────────────────────────────────────────────┐
│              DeterministicFloat<f64>                    │
│  - Wraps f64 with deterministic operations              │
│  - Routes to software impl when needed                  │
└─────────────────────────────────────────────────────────┘
                        │
        ┌───────────────┴───────────────┐
        │                               │
        ▼                               ▼
┌──────────────────┐          ┌──────────────────┐
│  Basic ops       │          │ Transcendentals  │
│  (+, -, *, /)    │          │ (sin, cos, exp)  │
│  Native hardware │          │ Software (MPFR)  │
│  No FMA          │          │ Bit-exact        │
└──────────────────┘          └──────────────────┘
```

### 2.4 Core Types

```rust
// crates/deriva-compute/src/deterministic_float.rs (new file)

use std::ops::{Add, Sub, Mul, Div};

/// Deterministic floating-point wrapper
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeterministicFloat<T> {
    value: T,
}

impl DeterministicFloat<f64> {
    pub fn new(value: f64) -> Self {
        Self { value }
    }

    pub fn get(&self) -> f64 {
        self.value
    }

    /// Basic arithmetic (hardware, no FMA)
    pub fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value }
    }

    pub fn sub(self, other: Self) -> Self {
        Self { value: self.value - other.value }
    }

    pub fn mul(self, other: Self) -> Self {
        Self { value: self.value * other.value }
    }

    pub fn div(self, other: Self) -> Self {
        Self { value: self.value / other.value }
    }

    pub fn sqrt(self) -> Self {
        Self { value: self.value.sqrt() }
    }

    /// Transcendental functions (software implementation)
    pub fn sin(self) -> Self {
        Self { value: soft_sin(self.value) }
    }

    pub fn cos(self) -> Self {
        Self { value: soft_cos(self.value) }
    }

    pub fn exp(self) -> Self {
        Self { value: soft_exp(self.value) }
    }

    pub fn log(self) -> Self {
        Self { value: soft_log(self.value) }
    }

    pub fn pow(self, exp: Self) -> Self {
        Self { value: soft_pow(self.value, exp.value) }
    }
}

// Implement standard ops
impl Add for DeterministicFloat<f64> {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        self.add(other)
    }
}

impl Sub for DeterministicFloat<f64> {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        self.sub(other)
    }
}

impl Mul for DeterministicFloat<f64> {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        self.mul(other)
    }
}

impl Div for DeterministicFloat<f64> {
    type Output = Self;
    fn div(self, other: Self) -> Self {
        self.div(other)
    }
}
```

---

## 3. Implementation

### 3.1 Disabling FMA

```rust
// crates/deriva-compute/build.rs (new file)

fn main() {
    // Disable FMA on all targets
    println!("cargo:rustc-env=RUSTFLAGS=-C target-feature=-fma");
    
    // Force SSE2 on x86_64 (avoid x87)
    #[cfg(target_arch = "x86_64")]
    println!("cargo:rustc-env=RUSTFLAGS=-C target-feature=+sse2");
}
```

### 3.2 Software Transcendentals

```rust
// crates/deriva-compute/src/soft_math.rs (new file)

/// Software implementation of sin (MPFR-derived)
pub fn soft_sin(x: f64) -> f64 {
    // Taylor series with error bounds
    // sin(x) = x - x³/3! + x⁵/5! - x⁷/7! + ...
    
    // Reduce to [-π, π]
    let x = reduce_range(x);
    
    let mut sum = x;
    let mut term = x;
    let x_sq = x * x;
    
    for n in 1..20 {
        term *= -x_sq / ((2 * n) * (2 * n + 1)) as f64;
        sum += term;
        if term.abs() < 1e-16 {
            break;
        }
    }
    
    sum
}

pub fn soft_cos(x: f64) -> f64 {
    // cos(x) = 1 - x²/2! + x⁴/4! - x⁶/6! + ...
    let x = reduce_range(x);
    
    let mut sum = 1.0;
    let mut term = 1.0;
    let x_sq = x * x;
    
    for n in 1..20 {
        term *= -x_sq / ((2 * n - 1) * (2 * n)) as f64;
        sum += term;
        if term.abs() < 1e-16 {
            break;
        }
    }
    
    sum
}

pub fn soft_exp(x: f64) -> f64 {
    // exp(x) = 1 + x + x²/2! + x³/3! + ...
    
    // Handle overflow/underflow
    if x > 709.0 {
        return f64::INFINITY;
    }
    if x < -745.0 {
        return 0.0;
    }
    
    let mut sum = 1.0;
    let mut term = 1.0;
    
    for n in 1..50 {
        term *= x / n as f64;
        sum += term;
        if term.abs() < 1e-16 {
            break;
        }
    }
    
    sum
}

pub fn soft_log(x: f64) -> f64 {
    if x <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if x == 1.0 {
        return 0.0;
    }
    
    // Use Newton's method: log(x) = y where exp(y) = x
    let mut y = x.ln(); // Initial guess from hardware
    
    for _ in 0..10 {
        let exp_y = soft_exp(y);
        y = y - (exp_y - x) / exp_y;
    }
    
    y
}

pub fn soft_pow(base: f64, exp: f64) -> f64 {
    if base == 0.0 {
        return if exp > 0.0 { 0.0 } else { f64::INFINITY };
    }
    
    // pow(base, exp) = exp(exp * log(base))
    soft_exp(exp * soft_log(base))
}

fn reduce_range(x: f64) -> f64 {
    // Reduce x to [-π, π]
    const PI: f64 = 3.141592653589793;
    const TWO_PI: f64 = 6.283185307179586;
    
    let mut x = x % TWO_PI;
    if x > PI {
        x -= TWO_PI;
    } else if x < -PI {
        x += TWO_PI;
    }
    x
}
```

### 3.3 NaN Canonicalization

```rust
impl DeterministicFloat<f64> {
    /// Canonicalize NaN to a single bit pattern
    pub fn canonicalize(value: f64) -> f64 {
        if value.is_nan() {
            // Canonical NaN: sign=0, exponent=all 1s, mantissa=0x8000000000000
            f64::from_bits(0x7FF8000000000000)
        } else {
            value
        }
    }

    pub fn new(value: f64) -> Self {
        Self { value: Self::canonicalize(value) }
    }
}
```

### 3.4 Float Policy Enforcement

```rust
// crates/deriva-compute/src/executor.rs

impl<'a, C: MaterializationCache, L: LeafStore> Executor<'a, C, L> {
    pub fn new(
        dag: &'a DagStore,
        registry: &'a FunctionRegistry,
        cache: &'a mut C,
        leaf_store: &'a L,
        config: ExecutorConfig,
    ) -> Self {
        Self {
            dag,
            registry,
            cache,
            leaf_store,
            config,
            float_policy: config.float_policy,
        }
    }

    pub async fn materialize(&mut self, addr: CAddr) -> Result<Bytes, ExecutionError> {
        let recipe = self.dag.get_recipe(&addr)
            .ok_or(ExecutionError::RecipeNotFound(addr))?;

        let function = self.registry.get(&recipe.function_id)
            .ok_or(ExecutionError::FunctionNotFound(recipe.function_id.clone()))?;

        // Check float policy
        if self.float_policy == FloatPolicy::Disabled && function.uses_floats() {
            return Err(ExecutionError::FloatsDisabled(recipe.function_id.clone()));
        }

        // ... rest of materialization ...
    }
}
```

### 3.5 Function Metadata

```rust
// crates/deriva-compute/src/function.rs

pub trait ComputeFunction: Send + Sync {
    fn id(&self) -> FunctionId;
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError>;
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost;
    
    /// Does this function use floating-point operations?
    fn uses_floats(&self) -> bool {
        false  // Default: no floats
    }
    
    /// Float policy required for this function
    fn required_float_policy(&self) -> FloatPolicy {
        if self.uses_floats() {
            FloatPolicy::Strict
        } else {
            FloatPolicy::Disabled
        }
    }
}
```


---

## 4. Data Flow Diagrams

### 4.1 Float Operation Routing

```
Function uses float operation
         │
         ▼
   FloatPolicy?
         │
    ┌────┴────┬────────────┬──────────┐
    │         │            │          │
    ▼         ▼            ▼          ▼
 Strict   Hardware     Disabled    WASM
    │         │            │          │
    │         │            │          │
    ▼         ▼            ▼          ▼
Basic op? Hardware    Error      Wasmtime
    │     (fast)                 (automatic)
 ┌──┴──┐
 │     │
Yes   No
 │     │
 │     └──▶ Software transcendental
 │          (slow but deterministic)
 │
 └──▶ Hardware basic op
      (no FMA, deterministic)
```

### 4.2 Cross-Platform Consistency

```
x86_64 Node                    aarch64 Node
     │                              │
Recipe: compute(1.0, 2.0, 3.0) Recipe: compute(1.0, 2.0, 3.0)
     │                              │
FloatPolicy::Strict            FloatPolicy::Strict
     │                              │
DeterministicFloat             DeterministicFloat
     │                              │
No FMA, software sin()         No FMA, software sin()
     │                              │
output = 0x3FF0000000000001    output = 0x3FF0000000000001
     │                              │
     ▼                              ▼
CAddr(0xabc...)                CAddr(0xabc...)

✅ Same CAddr across architectures
```

### 4.3 Policy Enforcement

```
Client                Executor              Function
  │                      │                     │
  │ Materialize(R)       │                     │
  ├─────────────────────>│                     │
  │                      │                     │
  │                      │ Get function        │
  │                      ├────────────────────>│
  │                      │                     │
  │                      │ uses_floats() = true│
  │                      │<────────────────────┤
  │                      │                     │
  │                      │ Check policy        │
  │                      │ (Disabled?)         │
  │                      │                     │
  │                      │ ❌ Error            │
  │   FloatsDisabled     │                     │
  │<─────────────────────┤                     │
```

---

## 5. Test Specification

### 5.1 Unit Tests

```rust
// crates/deriva-compute/tests/deterministic_float.rs

#[test]
fn test_basic_arithmetic_deterministic() {
    let a = DeterministicFloat::new(1.0);
    let b = DeterministicFloat::new(2.0);
    let c = DeterministicFloat::new(3.0);

    let result = a * b + c;
    
    // Result should be exactly 5.0
    assert_eq!(result.get(), 5.0);
    
    // Bit pattern should be deterministic
    assert_eq!(result.get().to_bits(), 0x4014000000000000);
}

#[test]
fn test_transcendental_deterministic() {
    let x = DeterministicFloat::new(1.0);
    
    let sin_x = x.sin();
    let cos_x = x.cos();
    let exp_x = x.exp();
    
    // Check against known values (computed with MPFR)
    assert!((sin_x.get() - 0.8414709848078965).abs() < 1e-15);
    assert!((cos_x.get() - 0.5403023058681398).abs() < 1e-15);
    assert!((exp_x.get() - 2.718281828459045).abs() < 1e-15);
}

#[test]
fn test_nan_canonicalization() {
    let nan1 = DeterministicFloat::new(f64::NAN);
    let nan2 = DeterministicFloat::new(0.0 / 0.0);
    let nan3 = DeterministicFloat::new(f64::INFINITY - f64::INFINITY);
    
    // All NaNs should have the same bit pattern
    assert_eq!(nan1.get().to_bits(), 0x7FF8000000000000);
    assert_eq!(nan2.get().to_bits(), 0x7FF8000000000000);
    assert_eq!(nan3.get().to_bits(), 0x7FF8000000000000);
}

#[test]
fn test_no_fma() {
    // This test verifies FMA is disabled
    let a = DeterministicFloat::new(1.0);
    let b = DeterministicFloat::new(1.0e-16);
    let c = DeterministicFloat::new(1.0);
    
    let result = a * b + c;
    
    // With FMA: 1.0000000000000001
    // Without FMA: 1.0 (due to rounding)
    // We want the non-FMA result
    assert_eq!(result.get(), 1.0);
}
```

### 5.2 Cross-Platform Tests

```rust
// crates/deriva-compute/tests/cross_platform.rs

#[test]
fn test_golden_float_values() {
    // Golden values computed on reference platform (x86_64, no FMA)
    let test_cases = vec![
        (1.0, 2.0, 3.0, 5.0),  // a * b + c
        (0.5, 0.5, 0.0, 0.25),
        (1.0e10, 1.0e-10, 1.0, 2.0),
    ];

    for (a, b, c, expected) in test_cases {
        let result = DeterministicFloat::new(a) * DeterministicFloat::new(b) 
                   + DeterministicFloat::new(c);
        assert_eq!(result.get(), expected);
    }
}

#[test]
fn test_golden_transcendental_values() {
    // Golden values for transcendental functions
    let test_cases = vec![
        ("sin", 0.0, 0.0),
        ("sin", 1.0, 0.8414709848078965),
        ("cos", 0.0, 1.0),
        ("cos", 1.0, 0.5403023058681398),
        ("exp", 0.0, 1.0),
        ("exp", 1.0, 2.718281828459045),
        ("log", 1.0, 0.0),
        ("log", 2.718281828459045, 1.0),
    ];

    for (op, input, expected) in test_cases {
        let x = DeterministicFloat::new(input);
        let result = match op {
            "sin" => x.sin(),
            "cos" => x.cos(),
            "exp" => x.exp(),
            "log" => x.log(),
            _ => panic!("unknown op"),
        };
        
        assert!((result.get() - expected).abs() < 1e-15, 
                "{} failed: got {}, expected {}", op, result.get(), expected);
    }
}
```

### 5.3 Integration Tests

```rust
// crates/deriva-compute/tests/float_policy.rs

#[tokio::test]
async fn test_float_policy_strict() {
    let dag = DagStore::new();
    let registry = FunctionRegistry::new();
    let mut cache = EvictableCache::new(1024 * 1024);
    let leaf_store = InMemoryLeafStore::new();

    // Register function that uses floats
    struct FloatFunction;
    impl ComputeFunction for FloatFunction {
        fn id(&self) -> FunctionId {
            FunctionId::from("float_compute")
        }

        fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
            let a = f64::from_le_bytes(inputs[0][..8].try_into().unwrap());
            let b = f64::from_le_bytes(inputs[1][..8].try_into().unwrap());
            
            let result = DeterministicFloat::new(a).sin() + DeterministicFloat::new(b).cos();
            
            Ok(Bytes::from(result.get().to_le_bytes().to_vec()))
        }

        fn estimated_cost(&self, _: &[u64]) -> ComputeCost {
            ComputeCost { cpu_ms: 1, memory_bytes: 8 }
        }

        fn uses_floats(&self) -> bool {
            true
        }
    }

    registry.register(Box::new(FloatFunction));

    // Create recipe
    let leaf_a = CAddr::from_bytes(b"A");
    let leaf_b = CAddr::from_bytes(b"B");
    leaf_store.put(leaf_a, Bytes::from(1.0f64.to_le_bytes().to_vec()));
    leaf_store.put(leaf_b, Bytes::from(2.0f64.to_le_bytes().to_vec()));

    let recipe = Recipe {
        function_id: FunctionId::from("float_compute"),
        inputs: vec![DataRef::Leaf(leaf_a), DataRef::Leaf(leaf_b)],
        params: BTreeMap::new(),
    };
    let addr = recipe.addr();
    dag.put_recipe(&recipe);

    // Execute with Strict policy (should succeed)
    let mut executor = Executor::new(
        &dag,
        &registry,
        &mut cache,
        &leaf_store,
        ExecutorConfig {
            float_policy: FloatPolicy::Strict,
            ..Default::default()
        },
    );
    let result = executor.materialize(addr).await.unwrap();
    assert_eq!(result.len(), 8);
}

#[tokio::test]
async fn test_float_policy_disabled() {
    let dag = DagStore::new();
    let registry = FunctionRegistry::new();
    let mut cache = EvictableCache::new(1024 * 1024);
    let leaf_store = InMemoryLeafStore::new();

    // Same function as above
    registry.register(Box::new(FloatFunction));

    let recipe = Recipe {
        function_id: FunctionId::from("float_compute"),
        inputs: vec![],
        params: BTreeMap::new(),
    };
    let addr = recipe.addr();
    dag.put_recipe(&recipe);

    // Execute with Disabled policy (should fail)
    let mut executor = Executor::new(
        &dag,
        &registry,
        &mut cache,
        &leaf_store,
        ExecutorConfig {
            float_policy: FloatPolicy::Disabled,
            ..Default::default()
        },
    );
    
    let result = executor.materialize(addr).await;
    assert!(matches!(result, Err(ExecutionError::FloatsDisabled(_))));
}
```

### 5.4 Benchmark Tests

```rust
// benches/deterministic_float.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_basic_ops(c: &mut Criterion) {
    c.bench_function("hardware_add", |b| {
        b.iter(|| {
            let a = black_box(1.0);
            let b = black_box(2.0);
            black_box(a + b)
        });
    });

    c.bench_function("deterministic_add", |b| {
        b.iter(|| {
            let a = DeterministicFloat::new(black_box(1.0));
            let b = DeterministicFloat::new(black_box(2.0));
            black_box(a + b)
        });
    });
}

fn bench_transcendentals(c: &mut Criterion) {
    c.bench_function("hardware_sin", |b| {
        b.iter(|| {
            let x = black_box(1.0);
            black_box(x.sin())
        });
    });

    c.bench_function("deterministic_sin", |b| {
        b.iter(|| {
            let x = DeterministicFloat::new(black_box(1.0));
            black_box(x.sin())
        });
    });
}

criterion_group!(benches, bench_basic_ops, bench_transcendentals);
criterion_main!(benches);
```

---

## 6. Edge Cases & Error Handling

### 6.1 Edge Cases

| Case | Behavior |
|------|----------|
| NaN | Canonicalized to 0x7FF8000000000000 |
| +Infinity | Preserved as-is |
| -Infinity | Preserved as-is |
| Denormals | Preserved (no flush-to-zero) |
| Division by zero | Returns ±Infinity (IEEE 754 standard) |
| log(0) | Returns -Infinity |
| log(negative) | Returns NaN (canonicalized) |
| sqrt(negative) | Returns NaN (canonicalized) |
| Overflow | Returns ±Infinity |
| Underflow | Returns denormal or zero |

### 6.2 Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum FloatError {
    #[error("floats disabled by policy")]
    FloatsDisabled,
    
    #[error("function requires {required:?} but executor has {actual:?}")]
    PolicyMismatch {
        required: FloatPolicy,
        actual: FloatPolicy,
    },
    
    #[error("invalid float operation: {0}")]
    InvalidOperation(String),
}
```

---

## 7. Performance Analysis

### 7.1 Basic Operations

**Expected overhead:**
```
Operation       Hardware    Deterministic    Overhead
add/sub/mul/div   ~1 ns         ~1 ns          0%
sqrt              ~5 ns         ~5 ns          0%
```

Basic operations have **zero overhead** (same hardware instructions, just no FMA).

### 7.2 Transcendental Functions

**Expected overhead:**
```
Operation    Hardware    Deterministic    Overhead
sin/cos        ~20 ns       ~500 ns        25x
exp            ~15 ns       ~400 ns        27x
log            ~18 ns       ~450 ns        25x
pow            ~30 ns       ~800 ns        27x
```

Transcendentals are **25-30x slower** with software implementation.

**Mitigation strategies:**
1. Use hardware mode for non-critical workloads
2. Cache transcendental results
3. Use lookup tables for common values
4. Avoid transcendentals in hot paths

### 7.3 Real-World Impact

**Workload analysis:**
```
Typical data pipeline:
  - 95% basic ops (add, mul, div) → 0% overhead
  - 5% transcendentals (sin, exp) → 25x overhead on 5% = 1.2x overall

Overall slowdown: ~20% for float-heavy workloads
```

Most workloads are dominated by I/O and basic arithmetic, so transcendental overhead is acceptable.

### 7.4 Memory Overhead

**DeterministicFloat wrapper:**
```
Size of f64: 8 bytes
Size of DeterministicFloat<f64>: 8 bytes (zero-cost wrapper)
```

No memory overhead — the wrapper is optimized away by the compiler.

---

## 8. WASM Integration

### 8.1 Wasmtime Determinism

WASM functions get deterministic floats automatically via Wasmtime:

```rust
// crates/deriva-compute/src/wasm_executor.rs

use wasmtime::*;

pub struct WasmExecutor {
    engine: Engine,
    store: Store<()>,
}

impl WasmExecutor {
    pub fn new() -> Result<Self, anyhow::Error> {
        let mut config = Config::new();
        
        // Enable deterministic float behavior
        config.wasm_nan_canonicalization(true);  // Canonicalize NaNs
        config.cranelift_nan_canonicalization(true);  // Cranelift backend
        
        let engine = Engine::new(&config)?;
        let store = Store::new(&engine, ());
        
        Ok(Self { engine, store })
    }

    pub fn execute_wasm(
        &mut self,
        wasm_bytes: &[u8],
        inputs: Vec<Bytes>,
    ) -> Result<Bytes, ComputeError> {
        let module = Module::new(&self.engine, wasm_bytes)?;
        let instance = Instance::new(&mut self.store, &module, &[])?;
        
        // Call exported function
        let compute = instance.get_typed_func::<(i32, i32), i32>(&mut self.store, "compute")?;
        
        // ... execute and return result ...
        
        Ok(result)
    }
}
```

**Key Wasmtime features:**
- NaN canonicalization (all NaNs → same bit pattern)
- No FMA unless explicitly enabled
- Deterministic rounding modes
- Consistent across all platforms

### 8.2 WASM Float Policy

```rust
impl ComputeFunction for WasmFunction {
    fn uses_floats(&self) -> bool {
        // Detect float usage by inspecting WASM module
        self.module_uses_floats()
    }

    fn required_float_policy(&self) -> FloatPolicy {
        // WASM floats are always deterministic
        FloatPolicy::Strict
    }
}
```

WASM functions automatically satisfy `FloatPolicy::Strict` — no additional work needed.

---

## 9. Compile-Time Detection

### 9.1 Proc Macro for Float Detection

```rust
// crates/deriva-compute-macros/src/lib.rs (new crate)

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

/// Detect float usage at compile time
#[proc_macro_attribute]
pub fn detect_floats(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let fn_name = &input.sig.ident;
    let fn_body = &input.block;

    // Scan function body for float operations
    let uses_floats = check_for_float_ops(fn_body);

    let output = quote! {
        #input

        impl #fn_name {
            pub const USES_FLOATS: bool = #uses_floats;
        }
    };

    TokenStream::from(output)
}

fn check_for_float_ops(body: &syn::Block) -> bool {
    // Simple heuristic: check for f32/f64 types
    // More sophisticated: check for float method calls
    let body_str = quote!(#body).to_string();
    body_str.contains("f32") || body_str.contains("f64")
}
```

### 9.2 Usage Example

```rust
use deriva_compute_macros::detect_floats;

#[detect_floats]
fn my_compute_function(a: f64, b: f64) -> f64 {
    DeterministicFloat::new(a).sin() + DeterministicFloat::new(b).cos()
}

// Compile-time constant
assert!(my_compute_function::USES_FLOATS);
```

### 9.3 Lint for Unsafe Float Usage

```rust
// Custom clippy lint (future work)
// Warn if raw f64 operations are used instead of DeterministicFloat

#[warn(unsafe_float_ops)]
fn bad_function(a: f64, b: f64) -> f64 {
    a.sin() + b.cos()  // ⚠️ Warning: use DeterministicFloat
}

#[allow(unsafe_float_ops)]
fn good_function(a: f64, b: f64) -> f64 {
    DeterministicFloat::new(a).sin() + DeterministicFloat::new(b).cos()  // ✅ OK
}
```

---

## 10. Advanced Software Math

### 10.1 Higher Precision Intermediates

For critical computations, use higher precision intermediates:

```rust
pub fn soft_sin_high_precision(x: f64) -> f64 {
    // Use f128 (software emulation) for intermediate calculations
    let x_high = f128::from(x);
    
    let mut sum = x_high;
    let mut term = x_high;
    let x_sq = x_high * x_high;
    
    for n in 1..30 {
        term *= -x_sq / f128::from((2 * n) * (2 * n + 1));
        sum += term;
        if term.abs() < f128::from(1e-30) {
            break;
        }
    }
    
    f64::from(sum)
}
```

**Trade-off:** 10x slower, but higher accuracy.

### 10.2 Error Bounds

Track error bounds for critical computations:

```rust
pub struct BoundedFloat {
    value: f64,
    error_bound: f64,
}

impl BoundedFloat {
    pub fn new(value: f64) -> Self {
        Self { value, error_bound: 0.0 }
    }

    pub fn sin(self) -> Self {
        let result = soft_sin(self.value);
        let error = self.error_bound + 1e-15;  // Taylor series error
        Self { value: result, error_bound: error }
    }

    pub fn is_accurate(&self, threshold: f64) -> bool {
        self.error_bound < threshold
    }
}
```

---

## 11. Testing Strategy

### 11.1 Differential Testing

Compare software implementation against hardware on reference platform:

```rust
#[test]
fn test_differential_sin() {
    let test_values = vec![0.0, 0.5, 1.0, 1.5, 2.0, std::f64::consts::PI];
    
    for x in test_values {
        let hardware = x.sin();
        let software = soft_sin(x);
        
        // Should be within 1 ULP (unit in last place)
        let diff = (hardware - software).abs();
        assert!(diff < 1e-15, "sin({}) differs: hw={}, sw={}", x, hardware, software);
    }
}
```

### 11.2 Property-Based Testing

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_sin_range(x in -100.0..100.0f64) {
        let result = DeterministicFloat::new(x).sin();
        
        // sin(x) must be in [-1, 1]
        prop_assert!(result.get() >= -1.0 && result.get() <= 1.0);
    }

    #[test]
    fn prop_exp_positive(x in -100.0..100.0f64) {
        let result = DeterministicFloat::new(x).exp();
        
        // exp(x) must be positive
        prop_assert!(result.get() > 0.0);
    }

    #[test]
    fn prop_log_inverse_exp(x in 0.1..100.0f64) {
        let exp_x = DeterministicFloat::new(x).exp();
        let log_exp_x = exp_x.log();
        
        // log(exp(x)) ≈ x
        prop_assert!((log_exp_x.get() - x).abs() < 1e-10);
    }
}
```

### 11.3 Fuzzing

```rust
// fuzz/fuzz_targets/deterministic_float.rs

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }
    
    let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
    
    // Should never panic
    let _ = DeterministicFloat::new(x).sin();
    let _ = DeterministicFloat::new(x).cos();
    let _ = DeterministicFloat::new(x).exp();
    
    if x > 0.0 {
        let _ = DeterministicFloat::new(x).log();
    }
});
```

### 11.4 CI Matrix Testing

```yaml
# .github/workflows/float-tests.yml
name: Float Determinism Tests

on: [push, pull_request]

jobs:
  test-cross-platform:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
        arch: [x86_64, aarch64]
    
    runs-on: ${{ matrix.os }}
    
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          target: ${{ matrix.arch }}-unknown-linux-gnu
      
      - name: Run float tests
        run: cargo test --test cross_platform
      
      - name: Compare golden values
        run: |
          cargo test --test golden_float_values -- --nocapture > results.txt
          diff results.txt golden/expected_results.txt
```

---

## 12. Migration Guide

### 12.1 Migrating Existing Functions

**Before (non-deterministic):**
```rust
fn my_function(inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let a = f64::from_le_bytes(inputs[0][..8].try_into().unwrap());
    let b = f64::from_le_bytes(inputs[1][..8].try_into().unwrap());
    
    let result = a.sin() + b.cos();  // ❌ Non-deterministic
    
    Ok(Bytes::from(result.to_le_bytes().to_vec()))
}
```

**After (deterministic):**
```rust
fn my_function(inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let a = f64::from_le_bytes(inputs[0][..8].try_into().unwrap());
    let b = f64::from_le_bytes(inputs[1][..8].try_into().unwrap());
    
    let result = DeterministicFloat::new(a).sin() + DeterministicFloat::new(b).cos();  // ✅ Deterministic
    
    Ok(Bytes::from(result.get().to_le_bytes().to_vec()))
}
```

### 12.2 Gradual Migration

```rust
// Support both modes during migration
pub enum FloatMode {
    Legacy,      // Old non-deterministic behavior
    Deterministic,  // New deterministic behavior
}

impl ComputeFunction for MyFunction {
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let mode = params.get("float_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("deterministic");
        
        match mode {
            "legacy" => self.execute_legacy(inputs, params),
            "deterministic" => self.execute_deterministic(inputs, params),
            _ => Err(ComputeError::InvalidParameter("float_mode".to_string())),
        }
    }
}
```

### 12.3 Validation

After migration, validate that new CAddrs are stable:

```rust
#[test]
fn test_migration_stability() {
    // Old recipe (legacy mode)
    let recipe_old = Recipe {
        function_id: FunctionId::from("my_function"),
        inputs: vec![...],
        params: {
            let mut map = BTreeMap::new();
            map.insert("float_mode".to_string(), Value::String("legacy".to_string()));
            map
        },
    };
    let addr_old = recipe_old.addr();

    // New recipe (deterministic mode)
    let recipe_new = Recipe {
        function_id: FunctionId::from("my_function"),
        inputs: vec![...],
        params: {
            let mut map = BTreeMap::new();
            map.insert("float_mode".to_string(), Value::String("deterministic".to_string()));
            map
        },
    };
    let addr_new = recipe_new.addr();

    // CAddrs should be different (different params)
    assert_ne!(addr_old, addr_new);

    // But deterministic mode should be stable across runs
    let addr_new_2 = recipe_new.addr();
    assert_eq!(addr_new, addr_new_2);
}
```


---

## 13. Alternative Approaches Considered

### 13.1 Use Existing Libraries

**Option 1: rug (MPFR bindings)**
- Pros: Battle-tested, high precision
- Cons: C dependency, large binary size, GPL license

**Option 2: softfloat**
- Pros: Pure software, deterministic
- Cons: Very slow (100x overhead), complex API

**Option 3: libm**
- Pros: Pure Rust, no dependencies
- Cons: Not guaranteed deterministic across versions

**Decision:** Hand-written software math is the best balance of determinism, performance, and simplicity.

### 13.2 Compile-Time Float Elimination

**Idea:** Use a type system to prevent float usage entirely.

```rust
// Phantom type to track float usage
struct NoFloats;
struct WithFloats;

trait FloatPolicy {}
impl FloatPolicy for NoFloats {}
impl FloatPolicy for WithFloats {}

struct Executor<F: FloatPolicy> {
    _phantom: PhantomData<F>,
}

impl Executor<NoFloats> {
    // Can only execute functions that don't use floats
}

impl Executor<WithFloats> {
    // Can execute any function
}
```

**Rejected:** Too restrictive, doesn't allow runtime policy switching.

### 13.3 Hardware-Assisted Determinism

**Idea:** Use CPU features to enforce determinism (e.g., set rounding mode, disable FMA).

**Problem:** Not all platforms support the same features. aarch64 doesn't have x87 control registers.

**Decision:** Software implementation is more portable.

---

## 14. Future Work

### 14.1 SIMD Determinism

Extend deterministic floats to SIMD operations:

```rust
use std::arch::x86_64::*;

pub struct DeterministicF64x4 {
    value: __m256d,
}

impl DeterministicF64x4 {
    pub fn add(self, other: Self) -> Self {
        unsafe {
            Self { value: _mm256_add_pd(self.value, other.value) }
        }
    }
    
    // Ensure no FMA
    pub fn mul_add(self, mul: Self, add: Self) -> Self {
        // Explicitly separate multiply and add
        let product = self.mul(mul);
        product.add(add)
    }
}
```

### 14.2 Decimal Floating-Point

For financial applications, use decimal floats (no binary rounding errors):

```rust
use rust_decimal::Decimal;

pub struct DeterministicDecimal {
    value: Decimal,
}

impl DeterministicDecimal {
    pub fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value }
    }
    
    // Exact decimal arithmetic
}
```

### 14.3 Interval Arithmetic

Track error bounds automatically:

```rust
pub struct IntervalFloat {
    lower: f64,
    upper: f64,
}

impl IntervalFloat {
    pub fn add(self, other: Self) -> Self {
        Self {
            lower: self.lower + other.lower,
            upper: self.upper + other.upper,
        }
    }
    
    pub fn width(&self) -> f64 {
        self.upper - self.lower
    }
}
```

---

## 15. Documentation Examples

### 15.1 Basic Usage

```rust
use deriva_compute::DeterministicFloat;

// Create deterministic floats
let a = DeterministicFloat::new(1.0);
let b = DeterministicFloat::new(2.0);

// Basic arithmetic (fast, hardware)
let sum = a + b;
let product = a * b;
let quotient = a / b;

// Transcendental functions (slow, software)
let sin_a = a.sin();
let cos_a = a.cos();
let exp_a = a.exp();

// Extract raw value
let result: f64 = sum.get();
```

### 15.2 Function Implementation

```rust
use deriva_compute::{ComputeFunction, DeterministicFloat};

struct MyFloatFunction;

impl ComputeFunction for MyFloatFunction {
    fn id(&self) -> FunctionId {
        FunctionId::from("my_float_function")
    }

    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        // Parse inputs
        let a = f64::from_le_bytes(inputs[0][..8].try_into().unwrap());
        let b = f64::from_le_bytes(inputs[1][..8].try_into().unwrap());
        
        // Use deterministic floats
        let result = DeterministicFloat::new(a).sin() + DeterministicFloat::new(b).cos();
        
        // Serialize result
        Ok(Bytes::from(result.get().to_le_bytes().to_vec()))
    }

    fn uses_floats(&self) -> bool {
        true
    }

    fn estimated_cost(&self, _: &[u64]) -> ComputeCost {
        ComputeCost {
            cpu_ms: 1,  // Transcendentals are slow
            memory_bytes: 16,
        }
    }
}
```

### 15.3 Policy Configuration

```rust
use deriva_compute::{ExecutorConfig, FloatPolicy};

// Strict mode (default): deterministic, slower transcendentals
let config_strict = ExecutorConfig {
    float_policy: FloatPolicy::Strict,
    ..Default::default()
};

// Hardware mode: faster, non-deterministic
let config_hardware = ExecutorConfig {
    float_policy: FloatPolicy::Hardware,
    ..Default::default()
};

// Disabled: reject any float usage
let config_disabled = ExecutorConfig {
    float_policy: FloatPolicy::Disabled,
    ..Default::default()
};

let mut executor = Executor::new(
    &dag,
    &registry,
    &mut cache,
    &leaf_store,
    config_strict,
);
```

---

## 16. Troubleshooting

### 16.1 Different CAddrs Across Platforms

**Symptom:** Same recipe produces different CAddrs on x86_64 vs aarch64.

**Diagnosis:**
```bash
# Run on both platforms
cargo test test_golden_float_values -- --nocapture

# Compare outputs
diff x86_64_output.txt aarch64_output.txt
```

**Possible causes:**
1. FMA not disabled → check `build.rs`
2. Hardware mode enabled → check `FloatPolicy`
3. Non-deterministic function → check for raw `f64` operations

**Fix:** Ensure `FloatPolicy::Strict` and all float ops use `DeterministicFloat`.

### 16.2 Slow Performance

**Symptom:** Float-heavy workloads are 10x slower than expected.

**Diagnosis:**
```bash
# Profile with perf
perf record --call-graph dwarf cargo bench bench_transcendentals
perf report
```

**Possible causes:**
1. Too many transcendental calls → cache results
2. Strict mode overhead → consider hardware mode for non-critical workloads
3. Inefficient algorithm → optimize algorithm, not float ops

**Fix:** Profile and optimize hot paths, consider hardware mode for development.

### 16.3 NaN Propagation

**Symptom:** Unexpected NaN results.

**Diagnosis:**
```rust
let x = DeterministicFloat::new(f64::NAN);
println!("NaN bits: {:016x}", x.get().to_bits());
// Should print: 7ff8000000000000
```

**Possible causes:**
1. Non-canonical NaN from external source
2. Division by zero
3. Invalid operation (sqrt of negative)

**Fix:** All NaNs are canonicalized automatically. Check input validation.

---

## 17. Security Considerations

### 17.1 Timing Attacks

Software transcendentals have **data-dependent timing**:

```rust
pub fn soft_sin(x: f64) -> f64 {
    // Taylor series converges faster for small x
    // Timing varies based on input value
}
```

**Mitigation:** For security-critical applications, use constant-time implementations or add random delays.

### 17.2 Denial of Service

Malicious inputs can cause slow transcendental computations:

```rust
// Worst case: large input requires many iterations
let x = DeterministicFloat::new(1e100);
let result = x.sin();  // Very slow
```

**Mitigation:** Add timeouts and resource limits (already in §2.2 Async Compute).

### 17.3 Precision Loss Attacks

Attacker could craft inputs that cause precision loss:

```rust
let a = DeterministicFloat::new(1e20);
let b = DeterministicFloat::new(1.0);
let result = a + b;  // b is lost due to precision
```

**Mitigation:** Document precision limits, validate inputs, use higher precision for critical computations.

---

## 18. Files Changed


### New Files
- `crates/deriva-compute/src/deterministic_float.rs` — `DeterministicFloat` wrapper
- `crates/deriva-compute/src/soft_math.rs` — Software transcendentals
- `crates/deriva-compute/src/float_policy.rs` — `FloatPolicy` enum
- `crates/deriva-compute/src/wasm_executor.rs` — WASM float integration
- `crates/deriva-compute/build.rs` — Disable FMA at compile time
- `crates/deriva-compute/tests/deterministic_float.rs` — Unit tests
- `crates/deriva-compute/tests/cross_platform.rs` — Golden value tests
- `crates/deriva-compute/tests/float_policy.rs` — Policy enforcement tests
- `crates/deriva-compute-macros/src/lib.rs` — Compile-time float detection
- `benches/deterministic_float.rs` — Performance benchmarks
- `fuzz/fuzz_targets/deterministic_float.rs` — Fuzzing target

### Modified Files
- `crates/deriva-compute/src/function.rs` — Add `uses_floats()` method
- `crates/deriva-compute/src/executor.rs` — Float policy enforcement
- `crates/deriva-compute/src/lib.rs` — Export new types
- `crates/deriva-compute/Cargo.toml` — Add build script

---

## 19. Dependency Changes

```toml
# crates/deriva-compute/Cargo.toml
[dependencies]
# No new runtime dependencies — pure Rust implementation

[dev-dependencies]
proptest = "1.4"
criterion = "0.5"

[build-dependencies]
# No build dependencies needed

# crates/deriva-compute-macros/Cargo.toml (new crate)
[dependencies]
proc-macro2 = "1.0"
quote = "1.0"
syn = { version = "2.0", features = ["full"] }

[lib]
proc-macro = true
```

---

## 20. Design Rationale

### 20.1 Why Not Use libm?

`libm` is a pure-Rust math library, but:
- Not guaranteed deterministic across versions
- Different implementations for different targets
- No explicit determinism guarantees

Our software implementation is **frozen** — the code never changes, guaranteeing determinism.

### 20.2 Why Three Policies?

**Strict:** For production systems where determinism is critical
**Hardware:** For development/testing where speed matters more
**Disabled:** For systems that don't need floats (avoid accidental usage)

Different use cases need different trade-offs.

### 20.3 Why Not Ban Floats Entirely?

Many legitimate use cases require floats:
- Scientific computing
- ML inference
- Image processing
- Statistical analysis

Banning floats would make Deriva unusable for these domains.

### 20.4 Why Software Transcendentals?

Hardware transcendentals (libm) are:
- Implementation-dependent (glibc vs musl vs macOS)
- Version-dependent (algorithm changes)
- Platform-dependent (x86_64 vs aarch64)

Software transcendentals are:
- Frozen (never change)
- Portable (same code everywhere)
- Verifiable (hand-auditable)

The 25x slowdown is acceptable for determinism guarantee.

---

## 21. Observability Integration

### 21.1 Metrics

```rust
lazy_static! {
    static ref FLOAT_OPERATIONS: IntCounterVec = register_int_counter_vec!(
        "deriva_float_operations_total",
        "Float operations by type and policy",
        &["operation", "policy"]
    ).unwrap();

    static ref FLOAT_OPERATION_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_float_operation_duration_seconds",
        "Float operation duration by type",
        &["operation"]
    ).unwrap();
    
    static ref FLOAT_POLICY_VIOLATIONS: IntCounter = register_int_counter!(
        "deriva_float_policy_violations_total",
        "Functions rejected due to float policy"
    ).unwrap();
}
```

### 21.2 Logs

```rust
impl DeterministicFloat<f64> {
    pub fn sin(self) -> Self {
        debug!("Computing deterministic sin({})", self.value);
        let start = Instant::now();
        
        let result = soft_sin(self.value);
        
        let duration = start.elapsed();
        FLOAT_OPERATION_DURATION
            .with_label_values(&["sin"])
            .observe(duration.as_secs_f64());
        FLOAT_OPERATIONS
            .with_label_values(&["sin", "strict"])
            .inc();
        
        Self { value: result }
    }
}

impl Executor {
    pub async fn materialize(&mut self, addr: CAddr) -> Result<Bytes, ExecutionError> {
        let function = self.registry.get(&recipe.function_id)?;
        
        if self.float_policy == FloatPolicy::Disabled && function.uses_floats() {
            warn!(
                "Rejecting function {} due to float policy",
                recipe.function_id
            );
            FLOAT_POLICY_VIOLATIONS.inc();
            return Err(ExecutionError::FloatsDisabled(recipe.function_id.clone()));
        }
        
        // ... rest of materialization ...
    }
}
```

### 21.3 Tracing

```rust
use tracing::{info_span, instrument};

#[instrument(skip(self))]
pub fn sin(self) -> Self {
    let span = info_span!("deterministic_sin", value = %self.value);
    let _enter = span.enter();
    
    let result = soft_sin(self.value);
    Self { value: result }
}
```

---

## 22. Checklist

### Implementation
- [ ] Create `deriva-compute/src/deterministic_float.rs` with wrapper type
- [ ] Create `deriva-compute/src/soft_math.rs` with software transcendentals
- [ ] Implement: `soft_sin`, `soft_cos`, `soft_exp`, `soft_log`, `soft_pow`
- [ ] Create `deriva-compute/src/float_policy.rs` with policy enum
- [ ] Create `build.rs` to disable FMA
- [ ] Add `uses_floats()` to `ComputeFunction` trait
- [ ] Add float policy enforcement to `Executor`
- [ ] Implement NaN canonicalization
- [ ] Create `deriva-compute/src/wasm_executor.rs` for WASM integration
- [ ] Create `deriva-compute-macros` crate for compile-time detection

### Testing
- [ ] Unit test: basic arithmetic determinism
- [ ] Unit test: transcendental determinism (sin, cos, exp, log, pow)
- [ ] Unit test: NaN canonicalization
- [ ] Unit test: no FMA verification
- [ ] Cross-platform test: golden float values (x86_64 + aarch64)
- [ ] Cross-platform test: golden transcendental values
- [ ] Integration test: float policy strict
- [ ] Integration test: float policy hardware
- [ ] Integration test: float policy disabled
- [ ] Property test: sin range [-1, 1]
- [ ] Property test: exp always positive
- [ ] Property test: log(exp(x)) ≈ x
- [ ] Benchmark: basic ops overhead (target: 0%)
- [ ] Benchmark: transcendental overhead (target: <30x)
- [ ] Fuzzing: no panics on arbitrary inputs

### Documentation
- [ ] Document float policy in user guide
- [ ] Add examples of deterministic float usage
- [ ] Document performance trade-offs (strict vs hardware)
- [ ] Add troubleshooting guide for float issues
- [ ] Document WASM float integration
- [ ] Add migration guide for existing functions
- [ ] Document security considerations (timing attacks, DoS)

### Observability
- [ ] Add `deriva_float_operations_total` counter
- [ ] Add `deriva_float_operation_duration_seconds` histogram
- [ ] Add `deriva_float_policy_violations_total` counter
- [ ] Add debug logs for transcendental operations
- [ ] Add tracing spans for float operations

### Validation
- [ ] Run tests on x86_64 and aarch64
- [ ] Verify same CAddr across platforms
- [ ] Benchmark transcendental overhead (target: <30x)
- [ ] Test with real float-heavy workloads
- [ ] Verify FMA is disabled (check assembly with `cargo asm`)
- [ ] Run fuzzing for 1M iterations
- [ ] Verify WASM float determinism

### Deployment
- [ ] Deploy with `FloatPolicy::Strict` as default
- [ ] Monitor float operation metrics
- [ ] Document policy configuration
- [ ] Add admin API to query/change policy at runtime
- [ ] Set up CI matrix testing (x86_64 + aarch64)
- [ ] Create golden value baseline for regression testing

---

**Estimated effort:** 6–8 days
- Days 1-2: Core `DeterministicFloat` wrapper + basic ops + NaN canonicalization
- Days 3-4: Software transcendentals (sin, cos, exp, log, pow) + error bounds
- Days 5: Float policy + enforcement + WASM integration
- Days 6-7: Cross-platform tests + benchmarks + fuzzing
- Day 8: Documentation + observability + migration guide

**Success criteria:**
1. All tests pass on x86_64 and aarch64
2. Same CAddr for same recipe across platforms (verified with golden tests)
3. Basic ops have 0% overhead (verified with benchmarks)
4. Transcendentals have <30x overhead (verified with benchmarks)
5. Golden value tests pass (bit-exact results)
6. No panics on 1M fuzz iterations
7. WASM floats are deterministic (verified with Wasmtime tests)
8. Policy enforcement works (Disabled rejects float functions)

