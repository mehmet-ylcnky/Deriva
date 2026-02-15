# §4.2 Deterministic Compute Scheduling

> **Status**: Blueprint  
> **Depends on**: §2.2 (Async Compute), §2.3 (Parallel Materialization), §4.1 (Canonical Serialization)  
> **Crate(s)**: `deriva-compute`, `deriva-core`  
> **Estimated effort**: 5–7 days  

---

## 1. Problem Statement

### 1.1 The Parallel Execution Problem

Phase 2 introduced parallel materialization for performance:

```rust
// crates/deriva-compute/src/executor.rs (current)
pub async fn materialize(&mut self, addr: CAddr) -> Result<Bytes, ExecutionError> {
    let recipe = self.dag.get_recipe(&addr)?;
    
    // Resolve inputs in parallel
    let input_futures: Vec<_> = recipe.inputs
        .iter()
        .map(|input_ref| self.resolve_input(input_ref))
        .collect();
    
    let inputs = futures::future::join_all(input_futures).await;
    
    // Execute function
    let function = self.registry.get(&recipe.function_id)?;
    function.execute(inputs, &recipe.params)
}
```

**The problem:** `join_all` completes futures in **arbitrary order**. If inputs are `[A, B, C]` and B finishes first, the executor might deliver them as `[B, A, C]` to the function.

### 1.2 Why This Breaks Determinism

**Scenario 1: Order-sensitive function**
```rust
// Function: concat(inputs)
// Recipe: concat([A, B, C])

Run 1: A finishes first → inputs = [A, B, C] → output = "ABC"
Run 2: C finishes first → inputs = [C, A, B] → output = "CAB"

Different outputs → different CAddrs → determinism broken
```

**Scenario 2: Cross-node divergence**
```
Node 1 (fast disk): A resolves in 1ms, B in 10ms, C in 5ms
                    → order: [A, C, B] → output = "ACB"

Node 2 (slow disk): A resolves in 50ms, B in 10ms, C in 20ms
                    → order: [B, C, A] → output = "BCA"

Same recipe, different nodes → different outputs → broken invariant
```

### 1.3 Current Mitigation (Insufficient)

The current code relies on **convention**:
```rust
// Function implementations are expected to not depend on input order
// But nothing enforces this!
```

This is fragile:
- New functions might accidentally depend on order
- Existing functions might have subtle order dependencies
- No way to detect violations

### 1.4 What We Need

**Guarantee:** The same recipe with the same inputs MUST produce the same output, regardless of:
- Parallel execution order
- Node hardware (fast vs slow disks)
- Network latency (distributed inputs)
- Number of CPU cores

This section makes input ordering **structurally enforced** at the scheduler level.

---

## 2. Design

### 2.1 Core Principle

**Input Assembly Barrier:** Collect all inputs in a position-indexed buffer, then deliver to the function in recipe-defined order, regardless of completion order.

```
Recipe: { inputs: [A, B, C] }

Parallel resolution:
  ┌─────┐  ┌─────┐  ┌─────┐
  │  A  │  │  B  │  │  C  │
  └──┬──┘  └──┬──┘  └──┬──┘
     │        │        │
     │ (10ms) │ (2ms)  │ (5ms)
     │        │        │
     ▼        ▼        ▼
  ┌─────────────────────────┐
  │  Input Assembly Buffer  │
  │  [0: _, 1: _, 2: _]     │
  └─────────────────────────┘
     │
     │ B completes first → buffer[1] = B
     │ C completes next  → buffer[2] = C
     │ A completes last  → buffer[0] = A
     │
     │ All slots filled → assemble in order
     ▼
  [A, B, C]  ← Always this order
     │
     ▼
  function.execute([A, B, C], params)
```

### 2.2 Architecture

```
┌──────────────────────────────────────────────────────────┐
│                    Executor                              │
│                                                          │
│  materialize(addr)                                       │
│    │                                                     │
│    ├─▶ get_recipe(addr) → Recipe                        │
│    │                                                     │
│    ├─▶ InputAssembler::new(recipe.inputs.len())         │
│    │                                                     │
│    ├─▶ spawn parallel tasks:                            │
│    │     for (idx, input_ref) in inputs.enumerate() {   │
│    │       tokio::spawn(async move {                    │
│    │         let data = resolve(input_ref).await;       │
│    │         assembler.set(idx, data);                  │
│    │       });                                           │
│    │     }                                               │
│    │                                                     │
│    ├─▶ assembler.wait_all().await → Vec<Bytes>          │
│    │     (blocks until all slots filled)                │
│    │                                                     │
│    └─▶ function.execute(ordered_inputs, params)         │
└──────────────────────────────────────────────────────────┘
```

### 2.3 Core Types

```rust
// crates/deriva-compute/src/assembler.rs (new file)

use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use bytes::Bytes;

/// Input assembly buffer with position-indexed slots
pub struct InputAssembler {
    slots: Arc<Mutex<Vec<Option<Bytes>>>>,
    notify: Arc<Notify>,
    count: usize,
}

impl InputAssembler {
    pub fn new(count: usize) -> Self {
        Self {
            slots: Arc::new(Mutex::new(vec![None; count])),
            notify: Arc::new(Notify::new()),
            count,
        }
    }

    /// Set input at specific index (called by parallel tasks)
    pub async fn set(&self, idx: usize, data: Bytes) {
        let mut slots = self.slots.lock().await;
        slots[idx] = Some(data);
        
        // Check if all slots filled
        if slots.iter().all(|s| s.is_some()) {
            self.notify.notify_one();
        }
    }

    /// Wait for all inputs to be resolved, return in order
    pub async fn wait_all(self) -> Vec<Bytes> {
        loop {
            {
                let slots = self.slots.lock().await;
                if slots.iter().all(|s| s.is_some()) {
                    // All filled — extract in order
                    return slots.iter().map(|s| s.clone().unwrap()).collect();
                }
            }
            
            // Wait for notification
            self.notify.notified().await;
        }
    }

    /// Clone handle for passing to parallel tasks
    pub fn handle(&self) -> InputAssemblerHandle {
        InputAssemblerHandle {
            slots: Arc::clone(&self.slots),
            notify: Arc::clone(&self.notify),
        }
    }
}

/// Handle for parallel tasks to set inputs
#[derive(Clone)]
pub struct InputAssemblerHandle {
    slots: Arc<Mutex<Vec<Option<Bytes>>>>,
    notify: Arc<Notify>,
}

impl InputAssemblerHandle {
    pub async fn set(&self, idx: usize, data: Bytes) {
        let mut slots = self.slots.lock().await;
        slots[idx] = Some(data);
        
        if slots.iter().all(|s| s.is_some()) {
            self.notify.notify_one();
        }
    }
}
```

### 2.4 Execution Modes

```rust
// crates/deriva-compute/src/executor.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Parallel resolution with ordering barrier (default)
    Parallel,
    
    /// Sequential resolution (inputs resolved one at a time, in order)
    Sequential,
    
    /// Replay recorded execution trace (for debugging)
    Replay,
}

pub struct ExecutorConfig {
    pub mode: ExecutionMode,
    pub max_parallelism: usize,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            mode: ExecutionMode::Parallel,
            max_parallelism: 64,
        }
    }
}
```

---

## 3. Implementation

### 3.1 Updated Executor

```rust
// crates/deriva-compute/src/executor.rs

pub struct Executor<'a, C: MaterializationCache, L: LeafStore> {
    dag: &'a DagStore,
    registry: &'a FunctionRegistry,
    cache: &'a mut C,
    leaf_store: &'a L,
    config: ExecutorConfig,
}

impl<'a, C: MaterializationCache, L: LeafStore> Executor<'a, C, L> {
    pub async fn materialize(&mut self, addr: CAddr) -> Result<Bytes, ExecutionError> {
        // Check cache first
        if let Some(cached) = self.cache.get(&addr) {
            return Ok(cached);
        }

        let recipe = self.dag.get_recipe(&addr)
            .ok_or(ExecutionError::RecipeNotFound(addr))?;

        // Resolve inputs based on execution mode
        let inputs = match self.config.mode {
            ExecutionMode::Parallel => self.resolve_parallel(&recipe.inputs).await?,
            ExecutionMode::Sequential => self.resolve_sequential(&recipe.inputs).await?,
            ExecutionMode::Replay => unimplemented!("replay mode"),
        };

        // Execute function
        let function = self.registry.get(&recipe.function_id)
            .ok_or(ExecutionError::FunctionNotFound(recipe.function_id.clone()))?;
        
        let output = function.execute(inputs, &recipe.params)?;

        // Cache result
        self.cache.put(addr, output.clone());

        Ok(output)
    }

    async fn resolve_parallel(&mut self, inputs: &[DataRef]) -> Result<Vec<Bytes>, ExecutionError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let assembler = InputAssembler::new(inputs.len());
        let handle = assembler.handle();

        // Spawn parallel resolution tasks
        let mut tasks = Vec::new();
        for (idx, input_ref) in inputs.iter().enumerate() {
            let input_ref = input_ref.clone();
            let handle = handle.clone();
            let executor_ptr = self as *mut Self;

            let task = tokio::spawn(async move {
                // SAFETY: We await all tasks before returning, so executor lives long enough
                let executor = unsafe { &mut *executor_ptr };
                let data = executor.resolve_input(&input_ref).await?;
                handle.set(idx, data).await;
                Ok::<_, ExecutionError>(())
            });

            tasks.push(task);
        }

        // Wait for all tasks
        for task in tasks {
            task.await.map_err(|e| ExecutionError::JoinError(e.to_string()))??;
        }

        // Assemble in order
        Ok(assembler.wait_all().await)
    }

    async fn resolve_sequential(&mut self, inputs: &[DataRef]) -> Result<Vec<Bytes>, ExecutionError> {
        let mut resolved = Vec::with_capacity(inputs.len());
        for input_ref in inputs {
            let data = self.resolve_input(input_ref).await?;
            resolved.push(data);
        }
        Ok(resolved)
    }

    async fn resolve_input(&mut self, input_ref: &DataRef) -> Result<Bytes, ExecutionError> {
        match input_ref {
            DataRef::Leaf(addr) => {
                self.leaf_store.get(addr)
                    .ok_or(ExecutionError::LeafNotFound(*addr))
            }
            DataRef::Derived(addr) => {
                self.materialize(*addr).await
            }
        }
    }
}
```


### 3.2 Execution Proof Recording

```rust
// crates/deriva-compute/src/proof.rs (new file)

use serde::{Serialize, Deserialize};

/// Proof that a materialization used specific inputs in specific order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionProof {
    pub recipe_addr: CAddr,
    pub input_addrs: Vec<CAddr>,
    pub output_addr: CAddr,
    pub timestamp: u64,
    pub node_id: String,
}

impl ExecutionProof {
    pub fn new(
        recipe_addr: CAddr,
        input_addrs: Vec<CAddr>,
        output_addr: CAddr,
        node_id: String,
    ) -> Self {
        Self {
            recipe_addr,
            input_addrs,
            output_addr,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            node_id,
        }
    }

    /// Verify proof: check that input order matches recipe
    pub fn verify(&self, recipe: &Recipe) -> bool {
        if self.input_addrs.len() != recipe.inputs.len() {
            return false;
        }

        for (proof_addr, recipe_ref) in self.input_addrs.iter().zip(&recipe.inputs) {
            let expected_addr = match recipe_ref {
                DataRef::Leaf(addr) => addr,
                DataRef::Derived(addr) => addr,
            };
            if proof_addr != expected_addr {
                return false;
            }
        }

        true
    }
}
```

### 3.3 Proof Storage

```rust
// crates/deriva-compute/src/executor.rs

impl<'a, C: MaterializationCache, L: LeafStore> Executor<'a, C, L> {
    pub async fn materialize_with_proof(
        &mut self,
        addr: CAddr,
    ) -> Result<(Bytes, ExecutionProof), ExecutionError> {
        let recipe = self.dag.get_recipe(&addr)
            .ok_or(ExecutionError::RecipeNotFound(addr))?;

        let inputs = self.resolve_parallel(&recipe.inputs).await?;

        // Record input CAddrs in order
        let input_addrs: Vec<CAddr> = recipe.inputs.iter().map(|r| match r {
            DataRef::Leaf(addr) => *addr,
            DataRef::Derived(addr) => *addr,
        }).collect();

        let function = self.registry.get(&recipe.function_id)
            .ok_or(ExecutionError::FunctionNotFound(recipe.function_id.clone()))?;
        
        let output = function.execute(inputs, &recipe.params)?;
        let output_addr = CAddr(blake3::hash(&output).into());

        // Generate proof
        let proof = ExecutionProof::new(
            addr,
            input_addrs,
            output_addr,
            self.node_id.clone(),
        );

        self.cache.put(addr, output.clone());

        Ok((output, proof))
    }
}
```

---

## 4. Data Flow Diagrams

### 4.1 Parallel Execution with Ordering Barrier

```
Recipe: concat([A, B, C])

Time →
  0ms    10ms   20ms   30ms
   │      │      │      │
   │      │      │      │
A  ├──────┼──────┼──────┼──────▶ resolves at 30ms
   │      │      │      │
B  ├──────▶ resolves at 10ms
   │      │      │      │
C  ├──────┼──────▶ resolves at 20ms
   │      │      │      │
   │      │      │      │
   ▼      ▼      ▼      ▼
┌─────────────────────────────┐
│   Input Assembly Buffer     │
│   [0: None, 1: None, 2: None]│
│                             │
│   t=10ms: [0: None, 1: B, 2: None]
│   t=20ms: [0: None, 1: B, 2: C]
│   t=30ms: [0: A, 1: B, 2: C]  ← All filled
│                             │
└─────────────────────────────┘
              │
              │ Assemble in order
              ▼
         [A, B, C]
              │
              ▼
    concat.execute([A, B, C])
              │
              ▼
          "ABC"  ← Always deterministic
```

### 4.2 Sequential Mode (Debugging)

```
Recipe: concat([A, B, C])

Time →
  0ms    10ms   20ms   30ms   40ms   50ms
   │      │      │      │      │      │
A  ├──────┼──────┼──────▶ resolves at 30ms
   │      │      │      │
   │      │      │      └─────▶ B starts
   │      │      │             │
B  │      │      │             ├──────▶ resolves at 40ms
   │      │      │             │
   │      │      │             └─────▶ C starts
   │      │      │                    │
C  │      │      │                    ├──────▶ resolves at 50ms
   │      │      │                    │
   │      │      │                    ▼
   │      │      │              [A, B, C]
   │      │      │                    │
   │      │      │                    ▼
   │      │      │          concat.execute([A, B, C])
   │      │      │                    │
   │      │      │                    ▼
   │      │      │                  "ABC"

Total time: 50ms (vs 30ms parallel)
But: guaranteed order, easier to debug
```

### 4.3 Cross-Node Consistency

```
Node 1 (fast disk)              Node 2 (slow disk)
      │                               │
Recipe: concat([A, B, C])       Recipe: concat([A, B, C])
      │                               │
      │ Parallel resolution           │ Parallel resolution
      │                               │
A: 1ms, B: 10ms, C: 5ms         A: 50ms, B: 10ms, C: 20ms
      │                               │
Completion order: A, C, B       Completion order: B, C, A
      │                               │
      │ Input Assembly Buffer         │ Input Assembly Buffer
      │ [0: A, 1: B, 2: C]            │ [0: A, 1: B, 2: C]
      │                               │
      │ Assemble in recipe order      │ Assemble in recipe order
      ▼                               ▼
   [A, B, C]                       [A, B, C]
      │                               │
      ▼                               ▼
    "ABC"                           "ABC"
      │                               │
      ▼                               ▼
CAddr(0xabc...)                 CAddr(0xabc...)

✅ Same CAddr despite different hardware
```

---

## 5. Test Specification

### 5.1 Unit Tests

```rust
// crates/deriva-compute/tests/deterministic_scheduling.rs

#[tokio::test]
async fn test_input_assembler_ordering() {
    let assembler = InputAssembler::new(3);
    let handle = assembler.handle();

    // Spawn tasks that complete in reverse order
    let h1 = handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        h1.set(0, Bytes::from("A")).await;
    });

    let h2 = handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        h2.set(1, Bytes::from("B")).await;
    });

    let h3 = handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        h3.set(2, Bytes::from("C")).await;
    });

    let inputs = assembler.wait_all().await;
    
    // Despite reverse completion order, inputs are in recipe order
    assert_eq!(inputs[0], Bytes::from("A"));
    assert_eq!(inputs[1], Bytes::from("B"));
    assert_eq!(inputs[2], Bytes::from("C"));
}

#[tokio::test]
async fn test_parallel_vs_sequential_same_output() {
    let dag = DagStore::new();
    let registry = FunctionRegistry::new();
    let mut cache = EvictableCache::new(1024 * 1024);
    let leaf_store = InMemoryLeafStore::new();

    // Register concat function
    registry.register(Box::new(ConcatFunction));

    // Create recipe
    let recipe = Recipe {
        function_id: FunctionId::from("concat"),
        inputs: vec![
            DataRef::Leaf(CAddr::from_bytes(b"hello")),
            DataRef::Leaf(CAddr::from_bytes(b"world")),
        ],
        params: BTreeMap::new(),
    };
    let addr = recipe.addr();
    dag.put_recipe(&recipe);

    // Store leaves
    leaf_store.put(CAddr::from_bytes(b"hello"), Bytes::from("hello"));
    leaf_store.put(CAddr::from_bytes(b"world"), Bytes::from("world"));

    // Execute in parallel mode
    let mut executor_parallel = Executor::new(
        &dag,
        &registry,
        &mut cache,
        &leaf_store,
        ExecutorConfig { mode: ExecutionMode::Parallel, ..Default::default() },
    );
    let output_parallel = executor_parallel.materialize(addr).await.unwrap();

    // Execute in sequential mode
    cache.clear();
    let mut executor_sequential = Executor::new(
        &dag,
        &registry,
        &mut cache,
        &leaf_store,
        ExecutorConfig { mode: ExecutionMode::Sequential, ..Default::default() },
    );
    let output_sequential = executor_sequential.materialize(addr).await.unwrap();

    // Outputs must be identical
    assert_eq!(output_parallel, output_sequential);
}

#[tokio::test]
async fn test_diamond_dag_determinism() {
    // Diamond DAG:
    //       A
    //      / \
    //     B   C
    //      \ /
    //       D (concat B and C)
    
    let dag = DagStore::new();
    let registry = FunctionRegistry::new();
    let mut cache = EvictableCache::new(1024 * 1024);
    let leaf_store = InMemoryLeafStore::new();

    registry.register(Box::new(ConcatFunction));

    // Leaf A
    let a_addr = CAddr::from_bytes(b"A");
    leaf_store.put(a_addr, Bytes::from("A"));

    // Recipe B: concat(A, A)
    let recipe_b = Recipe {
        function_id: FunctionId::from("concat"),
        inputs: vec![DataRef::Leaf(a_addr), DataRef::Leaf(a_addr)],
        params: BTreeMap::new(),
    };
    let b_addr = recipe_b.addr();
    dag.put_recipe(&recipe_b);

    // Recipe C: concat(A, A, A)
    let recipe_c = Recipe {
        function_id: FunctionId::from("concat"),
        inputs: vec![
            DataRef::Leaf(a_addr),
            DataRef::Leaf(a_addr),
            DataRef::Leaf(a_addr),
        ],
        params: BTreeMap::new(),
    };
    let c_addr = recipe_c.addr();
    dag.put_recipe(&recipe_c);

    // Recipe D: concat(B, C)
    let recipe_d = Recipe {
        function_id: FunctionId::from("concat"),
        inputs: vec![DataRef::Derived(b_addr), DataRef::Derived(c_addr)],
        params: BTreeMap::new(),
    };
    let d_addr = recipe_d.addr();
    dag.put_recipe(&recipe_d);

    // Execute D multiple times with random delays
    let mut executor = Executor::new(
        &dag,
        &registry,
        &mut cache,
        &leaf_store,
        ExecutorConfig::default(),
    );

    let mut outputs = Vec::new();
    for _ in 0..10 {
        cache.clear();
        let output = executor.materialize(d_addr).await.unwrap();
        outputs.push(output);
    }

    // All outputs must be identical
    for output in &outputs[1..] {
        assert_eq!(output, &outputs[0]);
    }
}
```

### 5.2 Stress Tests

```rust
#[tokio::test]
async fn test_high_fanout_determinism() {
    // Recipe with 100 inputs, all resolved in parallel
    let dag = DagStore::new();
    let registry = FunctionRegistry::new();
    let mut cache = EvictableCache::new(1024 * 1024);
    let leaf_store = InMemoryLeafStore::new();

    registry.register(Box::new(ConcatFunction));

    // Create 100 leaf inputs
    let mut inputs = Vec::new();
    for i in 0..100 {
        let addr = CAddr::from_bytes(&format!("input{}", i).as_bytes());
        leaf_store.put(addr, Bytes::from(format!("{}", i)));
        inputs.push(DataRef::Leaf(addr));
    }

    let recipe = Recipe {
        function_id: FunctionId::from("concat"),
        inputs,
        params: BTreeMap::new(),
    };
    let addr = recipe.addr();
    dag.put_recipe(&recipe);

    // Execute 100 times
    let mut executor = Executor::new(
        &dag,
        &registry,
        &mut cache,
        &leaf_store,
        ExecutorConfig::default(),
    );

    let mut outputs = Vec::new();
    for _ in 0..100 {
        cache.clear();
        let output = executor.materialize(addr).await.unwrap();
        outputs.push(output);
    }

    // All outputs must be identical
    for output in &outputs[1..] {
        assert_eq!(output, &outputs[0]);
    }
}

#[tokio::test]
async fn test_random_delays_determinism() {
    use rand::Rng;

    let dag = DagStore::new();
    let registry = FunctionRegistry::new();
    let mut cache = EvictableCache::new(1024 * 1024);
    let leaf_store = InMemoryLeafStore::new();

    // Register function that adds random delays
    struct DelayedConcatFunction;
    impl ComputeFunction for DelayedConcatFunction {
        fn id(&self) -> FunctionId {
            FunctionId::from("delayed_concat")
        }

        fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
            // Add random delay to simulate variable resolution times
            let delay_ms = rand::thread_rng().gen_range(1..50);
            std::thread::sleep(Duration::from_millis(delay_ms));

            let mut result = Vec::new();
            for input in inputs {
                result.extend_from_slice(&input);
            }
            Ok(Bytes::from(result))
        }

        fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
            ComputeCost {
                cpu_ms: input_sizes.iter().sum::<u64>() / 1000,
                memory_bytes: input_sizes.iter().sum(),
            }
        }
    }

    registry.register(Box::new(DelayedConcatFunction));

    // Create recipe with 10 inputs
    let mut inputs = Vec::new();
    for i in 0..10 {
        let addr = CAddr::from_bytes(&format!("input{}", i).as_bytes());
        leaf_store.put(addr, Bytes::from(format!("{}", i)));
        inputs.push(DataRef::Leaf(addr));
    }

    let recipe = Recipe {
        function_id: FunctionId::from("delayed_concat"),
        inputs,
        params: BTreeMap::new(),
    };
    let addr = recipe.addr();
    dag.put_recipe(&recipe);

    // Execute 1000 times with random delays
    let mut executor = Executor::new(
        &dag,
        &registry,
        &mut cache,
        &leaf_store,
        ExecutorConfig::default(),
    );

    let mut outputs = Vec::new();
    for _ in 0..1000 {
        cache.clear();
        let output = executor.materialize(addr).await.unwrap();
        outputs.push(output);
    }

    // All outputs must be identical despite random delays
    for output in &outputs[1..] {
        assert_eq!(output, &outputs[0]);
    }
}
```

### 5.3 Cross-Node Tests

```rust
#[tokio::test]
async fn test_cross_node_determinism() {
    // Simulate two nodes with different performance characteristics
    
    // Node 1: fast disk (1ms latency)
    let leaf_store_1 = DelayedLeafStore::new(Duration::from_millis(1));
    
    // Node 2: slow disk (50ms latency)
    let leaf_store_2 = DelayedLeafStore::new(Duration::from_millis(50));

    let dag = DagStore::new();
    let registry = FunctionRegistry::new();
    registry.register(Box::new(ConcatFunction));

    let recipe = Recipe {
        function_id: FunctionId::from("concat"),
        inputs: vec![
            DataRef::Leaf(CAddr::from_bytes(b"A")),
            DataRef::Leaf(CAddr::from_bytes(b"B")),
            DataRef::Leaf(CAddr::from_bytes(b"C")),
        ],
        params: BTreeMap::new(),
    };
    let addr = recipe.addr();
    dag.put_recipe(&recipe);

    // Store leaves on both nodes
    leaf_store_1.put(CAddr::from_bytes(b"A"), Bytes::from("A"));
    leaf_store_1.put(CAddr::from_bytes(b"B"), Bytes::from("B"));
    leaf_store_1.put(CAddr::from_bytes(b"C"), Bytes::from("C"));

    leaf_store_2.put(CAddr::from_bytes(b"A"), Bytes::from("A"));
    leaf_store_2.put(CAddr::from_bytes(b"B"), Bytes::from("B"));
    leaf_store_2.put(CAddr::from_bytes(b"C"), Bytes::from("C"));

    // Execute on both nodes
    let mut cache_1 = EvictableCache::new(1024 * 1024);
    let mut executor_1 = Executor::new(
        &dag,
        &registry,
        &mut cache_1,
        &leaf_store_1,
        ExecutorConfig::default(),
    );
    let output_1 = executor_1.materialize(addr).await.unwrap();

    let mut cache_2 = EvictableCache::new(1024 * 1024);
    let mut executor_2 = Executor::new(
        &dag,
        &registry,
        &mut cache_2,
        &leaf_store_2,
        ExecutorConfig::default(),
    );
    let output_2 = executor_2.materialize(addr).await.unwrap();

    // Outputs must be identical despite different latencies
    assert_eq!(output_1, output_2);
    
    // CAddrs must be identical
    let addr_1 = CAddr(blake3::hash(&output_1).into());
    let addr_2 = CAddr(blake3::hash(&output_2).into());
    assert_eq!(addr_1, addr_2);
}
```


---

## 6. Edge Cases & Error Handling

### 6.1 Edge Cases

| Case | Behavior |
|------|----------|
| Zero inputs | `InputAssembler::new(0)` returns immediately with empty vec |
| Single input | No parallelism, behaves like sequential mode |
| Task panic | Panic propagates to `join`, executor returns error |
| Timeout | Not implemented (future: add timeout to `wait_all`) |
| Duplicate set | Second `set(idx, data)` overwrites first (shouldn't happen) |

### 6.2 Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("recipe not found: {0}")]
    RecipeNotFound(CAddr),
    
    #[error("function not found: {0}")]
    FunctionNotFound(FunctionId),
    
    #[error("leaf not found: {0}")]
    LeafNotFound(CAddr),
    
    #[error("compute error: {0}")]
    ComputeError(#[from] ComputeError),
    
    #[error("task join error: {0}")]
    JoinError(String),
    
    #[error("input resolution failed at index {index}: {source}")]
    InputResolutionFailed {
        index: usize,
        source: Box<ExecutionError>,
    },
}
```

### 6.3 Failure Scenarios

**Scenario 1: One input fails to resolve**
```rust
// Input A resolves successfully
// Input B fails (leaf not found)
// Input C resolves successfully

Result: InputAssembler never completes (B slot remains None)
        → wait_all() blocks forever
        → Need timeout mechanism (future work)
```

**Mitigation:** Add timeout to `wait_all`:
```rust
pub async fn wait_all_timeout(self, timeout: Duration) -> Result<Vec<Bytes>, TimeoutError> {
    tokio::time::timeout(timeout, self.wait_all()).await
        .map_err(|_| TimeoutError)
}
```

---

## 7. Performance Analysis

### 7.1 Overhead Analysis

**Parallel mode overhead:**
- `InputAssembler` allocation: ~100 bytes
- Mutex lock per `set()`: ~50 ns
- Notify overhead: ~100 ns
- Total overhead: <1 μs per materialization

**Sequential mode overhead:**
- Zero (no assembler, no locks)

**Comparison:**
```
Parallel mode:
  - Overhead: ~1 μs
  - Benefit: inputs resolved concurrently
  - Best for: high-latency inputs (network, disk)

Sequential mode:
  - Overhead: 0 μs
  - Penalty: inputs resolved serially
  - Best for: debugging, low-latency inputs (memory cache)
```

### 7.2 Benchmark

```rust
// benches/deterministic_scheduling.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_parallel_vs_sequential(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("parallel_10_inputs", |b| {
        b.to_async(&rt).iter(|| async {
            let assembler = InputAssembler::new(10);
            let handle = assembler.handle();

            for i in 0..10 {
                let h = handle.clone();
                tokio::spawn(async move {
                    h.set(i, Bytes::from(vec![i as u8])).await;
                });
            }

            black_box(assembler.wait_all().await);
        });
    });

    c.bench_function("sequential_10_inputs", |b| {
        b.to_async(&rt).iter(|| async {
            let mut inputs = Vec::new();
            for i in 0..10 {
                inputs.push(Bytes::from(vec![i as u8]));
            }
            black_box(inputs);
        });
    });
}

criterion_group!(benches, bench_parallel_vs_sequential);
criterion_main!(benches);
```

**Expected results:**
- Parallel (10 inputs): ~5 μs
- Sequential (10 inputs): ~1 μs
- Overhead: ~4 μs (acceptable for determinism guarantee)

### 7.3 Scalability

**Input count vs overhead:**
```
10 inputs:   ~5 μs
100 inputs:  ~50 μs
1000 inputs: ~500 μs
```

Linear scaling — overhead is O(N) where N = input count.

---

## 8. Files Changed

### New Files
- `crates/deriva-compute/src/assembler.rs` — `InputAssembler` implementation
- `crates/deriva-compute/src/proof.rs` — `ExecutionProof` type
- `crates/deriva-compute/tests/deterministic_scheduling.rs` — unit tests
- `crates/deriva-compute/tests/stress_tests.rs` — stress tests
- `benches/deterministic_scheduling.rs` — performance benchmarks

### Modified Files
- `crates/deriva-compute/src/executor.rs` — parallel/sequential resolution modes
- `crates/deriva-compute/src/lib.rs` — export new types

---

## 9. Dependency Changes

```toml
# crates/deriva-compute/Cargo.toml
[dependencies]
tokio = { version = "1.35", features = ["sync", "time"] }
bytes = "1.5"
blake3 = "1.5"
serde = { version = "1.0", features = ["derive"] }

[dev-dependencies]
rand = "0.8"
```

No new dependencies — uses existing tokio primitives.

---

## 10. Design Rationale

### 10.1 Why Not Trust Function Implementations?

**Option 1: Convention-based (current)**
- Functions are expected to not depend on input order
- No enforcement
- Fragile — easy to violate accidentally

**Option 2: Structural enforcement (this section)**
- Scheduler guarantees input order
- Functions can't violate even if they try
- Robust — determinism is structurally impossible to break

**Decision:** Structural enforcement. Determinism is too important to rely on convention.

### 10.2 Why Position-Indexed Buffer?

**Alternative 1: Ordered channel**
```rust
// Inputs sent to channel in completion order
// Consumer sorts by index
```
Problem: Consumer must buffer all inputs before sorting → same complexity as our solution.

**Alternative 2: Barrier + sort**
```rust
// Collect all inputs in any order
// Sort by index before passing to function
```
Problem: Requires tagging inputs with index → more complex.

**Our solution:** Position-indexed buffer is simplest and most efficient.

### 10.3 Why Support Sequential Mode?

**Use cases:**
1. **Debugging** — easier to trace execution when inputs resolve one at a time
2. **Low-latency inputs** — if all inputs are in memory cache, parallelism adds overhead
3. **Testing** — verify parallel and sequential produce same output

Sequential mode is not the default, but it's useful for specific scenarios.

---

## 11. Observability Integration

### 11.1 Metrics

```rust
lazy_static! {
    static ref INPUT_RESOLUTION_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_input_resolution_duration_seconds",
        "Time to resolve a single input",
        &["mode"]  // "parallel" or "sequential"
    ).unwrap();

    static ref ASSEMBLY_WAIT_DURATION: Histogram = register_histogram!(
        "deriva_assembly_wait_duration_seconds",
        "Time spent waiting for all inputs to be assembled"
    ).unwrap();

    static ref PARALLEL_SPEEDUP: Histogram = register_histogram!(
        "deriva_parallel_speedup_ratio",
        "Speedup ratio: sequential time / parallel time"
    ).unwrap();
}
```

### 11.2 Logs

```rust
impl InputAssembler {
    pub async fn wait_all(self) -> Vec<Bytes> {
        let start = Instant::now();
        
        loop {
            {
                let slots = self.slots.lock().await;
                if slots.iter().all(|s| s.is_some()) {
                    let duration = start.elapsed();
                    debug!(
                        "Input assembly complete: {} inputs in {:?}",
                        self.count,
                        duration
                    );
                    ASSEMBLY_WAIT_DURATION.observe(duration.as_secs_f64());
                    
                    return slots.iter().map(|s| s.clone().unwrap()).collect();
                }
            }
            
            self.notify.notified().await;
        }
    }
}
```

### 11.3 Tracing

```rust
use tracing::{info_span, instrument};

#[instrument(skip(self), fields(recipe_addr = %addr))]
pub async fn materialize(&mut self, addr: CAddr) -> Result<Bytes, ExecutionError> {
    let span = info_span!("materialize", mode = ?self.config.mode);
    let _enter = span.enter();
    
    // ... existing code ...
}
```

---

## 12. Checklist

### Implementation
- [ ] Create `deriva-compute/src/assembler.rs` with `InputAssembler`
- [ ] Add `InputAssemblerHandle` for parallel tasks
- [ ] Create `deriva-compute/src/proof.rs` with `ExecutionProof`
- [ ] Update `Executor::materialize()` to use `resolve_parallel()`
- [ ] Add `resolve_sequential()` method
- [ ] Add `ExecutionMode` enum and `ExecutorConfig`
- [ ] Add `materialize_with_proof()` method

### Testing
- [ ] Unit test: `InputAssembler` ordering with reverse completion
- [ ] Unit test: parallel vs sequential same output
- [ ] Unit test: diamond DAG determinism
- [ ] Stress test: high fanout (100 inputs)
- [ ] Stress test: random delays (1000 iterations)
- [ ] Cross-node test: fast vs slow disk
- [ ] Property test: any input order → same output

### Documentation
- [ ] Document `ExecutionMode` in API docs
- [ ] Add examples of parallel vs sequential execution
- [ ] Document `ExecutionProof` format
- [ ] Add troubleshooting guide for non-deterministic functions

### Observability
- [ ] Add `deriva_input_resolution_duration_seconds` metric
- [ ] Add `deriva_assembly_wait_duration_seconds` metric
- [ ] Add `deriva_parallel_speedup_ratio` metric
- [ ] Add debug logs for assembly completion
- [ ] Add tracing spans for materialization

### Validation
- [ ] Run full test suite (unit + integration + stress)
- [ ] Benchmark parallel vs sequential overhead
- [ ] Test on multi-core machines (2, 4, 8, 16 cores)
- [ ] Test with high-latency inputs (network, slow disk)
- [ ] Verify cross-node determinism (x86_64 + aarch64)

### Deployment
- [ ] Deploy with `ExecutionMode::Parallel` as default
- [ ] Monitor `assembly_wait_duration` metric
- [ ] Monitor `parallel_speedup_ratio` metric
- [ ] Add admin API to switch execution mode at runtime
- [ ] Document performance characteristics

---

**Estimated effort:** 5–7 days
- Days 1-2: `InputAssembler` implementation + unit tests
- Days 3-4: Executor integration + parallel/sequential modes
- Days 5: Stress tests + cross-node tests
- Days 6-7: Benchmarks + observability + documentation

**Success criteria:**
1. All tests pass (unit + stress + cross-node)
2. Parallel overhead <5 μs per materialization
3. 1000 iterations with random delays → identical output
4. Cross-node test (fast vs slow disk) → identical CAddr
5. Diamond DAG test → stable output across 100 runs
