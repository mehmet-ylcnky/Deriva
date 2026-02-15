# Phase 4, Section 4.6: WASM Function Plugins

**Status:** Blueprint  
**Depends on:** §4.1 (Canonical Serialization), §4.2 (Deterministic Scheduling), §4.3 (Reproducibility Proofs), §4.4 (Deterministic Floats), §4.5 (Content Integrity)  
**Blocks:** §4.7 (FUSE), §4.10 (REST)

---

## 1. Problem Statement

### 1.1 Current State

Deriva's compute functions are **native Rust code** compiled into the binary:

```rust
// Current approach: native Rust functions
struct MyFunction;

impl ComputeFunction for MyFunction {
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        // Native Rust code
    }
}

// Must recompile and redeploy to add new functions
```

**Problems:**
1. **No dynamic loading**: Must recompile binary to add functions
2. **No sandboxing**: Functions have full system access
3. **No resource limits**: Functions can consume unlimited CPU/memory
4. **No portability**: Functions tied to specific Rust version/platform
5. **No user-defined functions**: Users can't deploy custom logic

### 1.2 The Problem

**Scenario 1: User Wants Custom Function**
```rust
// User wants to deploy image processing function
// ❌ Must fork Deriva, add function, recompile, redeploy
// ❌ No way to deploy without system access
```

**Scenario 2: Untrusted Code**
```rust
// Malicious function tries to access filesystem
fn execute(&self, inputs: Vec<Bytes>, _: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    std::fs::read("/etc/passwd")?;  // ❌ No sandboxing
    Ok(Bytes::new())
}
```

**Scenario 3: Resource Exhaustion**
```rust
// Function consumes all memory
fn execute(&self, inputs: Vec<Bytes>, _: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mut v = Vec::new();
    loop {
        v.push(vec![0u8; 1024 * 1024]);  // ❌ No memory limit
    }
}
```

### 1.3 Requirements

1. **Dynamic loading**: Deploy functions without recompiling Deriva
2. **Sandboxing**: Isolate functions from system resources
3. **Resource limits**: Enforce CPU/memory/time limits
4. **Determinism**: WASM execution must be deterministic (§4.4 floats)
5. **Portability**: Functions run on any platform
6. **Performance**: <10% overhead vs native Rust
7. **Integration**: WASM functions use same `ComputeFunction` trait

---

## 2. Design

### 2.1 Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     WASM Function Layer                      │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ WasmFunction │  │ WasmRuntime  │  │ WasmLoader   │      │
│  │              │  │              │  │              │      │
│  │ • Wrapper    │  │ • Wasmtime   │  │ • Validate   │      │
│  │ • Serialize  │  │ • Sandbox    │  │ • Compile    │      │
│  │ • Limits     │  │ • Limits     │  │ • Cache      │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                               │
├─────────────────────────────────────────────────────────────┤
│                  ComputeFunction Trait (§2.1)                │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ Native Rust  │  │ WASM Plugin  │  │   Registry   │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Core Types

```rust
// crates/deriva-compute/src/wasm_function.rs

use wasmtime::*;

/// WASM function wrapper
pub struct WasmFunction {
    /// Function ID
    id: FunctionId,
    /// Compiled WASM module
    module: Module,
    /// Runtime configuration
    config: WasmConfig,
}

/// WASM runtime configuration
#[derive(Debug, Clone)]
pub struct WasmConfig {
    /// Maximum memory (bytes)
    pub max_memory: u64,
    /// Maximum execution time (milliseconds)
    pub max_time_ms: u64,
    /// Enable deterministic floats (§4.4)
    pub deterministic_floats: bool,
    /// Enable fuel metering (CPU limit)
    pub enable_fuel: bool,
    /// Fuel limit (instructions)
    pub fuel_limit: u64,
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            max_memory: 64 * 1024 * 1024,  // 64 MB
            max_time_ms: 5000,              // 5 seconds
            deterministic_floats: true,
            enable_fuel: true,
            fuel_limit: 1_000_000_000,      // 1B instructions
        }
    }
}

impl WasmFunction {
    /// Load WASM module from bytes
    pub fn from_bytes(
        id: FunctionId,
        wasm_bytes: &[u8],
        config: WasmConfig,
    ) -> Result<Self, WasmError> {
        // Create engine with deterministic config
        let mut engine_config = Config::new();
        engine_config.consume_fuel(config.enable_fuel);
        engine_config.cranelift_nan_canonicalization(config.deterministic_floats);
        
        let engine = Engine::new(&engine_config)?;
        
        // Compile module
        let module = Module::from_binary(&engine, wasm_bytes)?;
        
        Ok(Self { id, module, config })
    }
}

impl ComputeFunction for WasmFunction {
    fn id(&self) -> FunctionId {
        self.id.clone()
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        // Create store with resource limits
        let mut store = Store::new(
            self.module.engine(),
            WasmState {
                memory_limit: self.config.max_memory,
            },
        );
        
        // Set fuel limit
        if self.config.enable_fuel {
            store.add_fuel(self.config.fuel_limit)?;
        }
        
        // Instantiate module
        let instance = Instance::new(&mut store, &self.module, &[])?;
        
        // Get execute function
        let execute_fn = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "execute")?;
        
        // Serialize inputs
        let input_bytes = bincode::serialize(&(inputs, params))?;
        
        // Allocate memory in WASM
        let alloc_fn = instance
            .get_typed_func::<i32, i32>(&mut store, "alloc")?;
        let input_ptr = alloc_fn.call(&mut store, input_bytes.len() as i32)?;
        
        // Write inputs to WASM memory
        let memory = instance.get_memory(&mut store, "memory")
            .ok_or(WasmError::NoMemory)?;
        memory.write(&mut store, input_ptr as usize, &input_bytes)?;
        
        // Execute with timeout
        let result = tokio::time::timeout(
            Duration::from_millis(self.config.max_time_ms),
            async {
                execute_fn.call(&mut store, (input_ptr, input_bytes.len() as i32))
            },
        ).await??;
        
        // Read result from WASM memory
        let result_len = result as usize;
        let mut result_bytes = vec![0u8; result_len];
        memory.read(&store, input_ptr as usize, &mut result_bytes)?;
        
        Ok(Bytes::from(result_bytes))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        // WASM functions have higher overhead
        ComputeCost {
            cpu_ms: 10,  // Baseline overhead
            memory_bytes: self.config.max_memory,
        }
    }

    fn uses_floats(&self) -> bool {
        // Assume WASM functions may use floats
        true
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    #[error("Wasmtime error: {0}")]
    Wasmtime(#[from] wasmtime::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    
    #[error("Timeout after {0}ms")]
    Timeout(u64),
    
    #[error("Out of fuel (CPU limit exceeded)")]
    OutOfFuel,
    
    #[error("Memory limit exceeded")]
    MemoryLimit,
    
    #[error("No memory export")]
    NoMemory,
}

/// WASM instance state
struct WasmState {
    memory_limit: u64,
}
```

### 2.3 WASM Module Interface

```rust
// WASM modules must export these functions:

// Allocate memory
#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    // Allocate and return pointer
}

// Execute function
#[no_mangle]
pub extern "C" fn execute(input_ptr: i32, input_len: i32) -> i32 {
    // Deserialize inputs
    // Run computation
    // Serialize result
    // Return result length
}
```

### 2.4 WASM Loader

```rust
// crates/deriva-compute/src/wasm_loader.rs

/// WASM module loader with validation and caching
pub struct WasmLoader {
    cache: Arc<Mutex<HashMap<CAddr, Arc<WasmFunction>>>>,
    config: WasmConfig,
}

impl WasmLoader {
    pub fn new(config: WasmConfig) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }
    
    /// Load WASM module from DagStore
    pub async fn load(
        &self,
        dag: &DagStore,
        addr: CAddr,
    ) -> Result<Arc<WasmFunction>, WasmError> {
        // Check cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some(func) = cache.get(&addr) {
                return Ok(func.clone());
            }
        }
        
        // Load from storage
        let wasm_bytes = dag.get(addr).await
            .ok_or(WasmError::NotFound(addr))?;
        
        // Validate WASM
        self.validate(&wasm_bytes)?;
        
        // Create function
        let id = FunctionId::from(format!("wasm:{}", addr.to_hex()));
        let func = Arc::new(WasmFunction::from_bytes(id, &wasm_bytes, self.config.clone())?);
        
        // Cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(addr, func.clone());
        }
        
        Ok(func)
    }
    
    fn validate(&self, wasm_bytes: &[u8]) -> Result<(), WasmError> {
        // Basic validation
        if wasm_bytes.len() > 10 * 1024 * 1024 {
            return Err(WasmError::TooLarge);
        }
        
        // Parse WASM
        let engine = Engine::default();
        let module = Module::from_binary(&engine, wasm_bytes)?;
        
        // Check exports
        let exports: Vec<_> = module.exports().map(|e| e.name()).collect();
        if !exports.contains(&"execute") {
            return Err(WasmError::MissingExport("execute"));
        }
        if !exports.contains(&"alloc") {
            return Err(WasmError::MissingExport("alloc"));
        }
        if !exports.contains(&"memory") {
            return Err(WasmError::MissingExport("memory"));
        }
        
        Ok(())
    }
}
```

---

## 3. Implementation

### 3.1 Example WASM Function (Rust)

```rust
// examples/wasm_functions/hello.rs

use std::alloc::{alloc, Layout};
use std::slice;

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    unsafe {
        let layout = Layout::from_size_align_unchecked(size as usize, 1);
        alloc(layout) as i32
    }
}

#[no_mangle]
pub extern "C" fn execute(input_ptr: i32, input_len: i32) -> i32 {
    unsafe {
        // Read input
        let input_slice = slice::from_raw_parts(input_ptr as *const u8, input_len as usize);
        let (inputs, params): (Vec<Vec<u8>>, std::collections::BTreeMap<String, String>) =
            bincode::deserialize(input_slice).unwrap();
        
        // Compute
        let result = format!("Hello, {}!", String::from_utf8_lossy(&inputs[0]));
        
        // Serialize result
        let result_bytes = result.as_bytes();
        let result_ptr = alloc(result_bytes.len() as i32);
        let result_slice = slice::from_raw_parts_mut(result_ptr as *mut u8, result_bytes.len());
        result_slice.copy_from_slice(result_bytes);
        
        result_bytes.len() as i32
    }
}
```

**Compile:**
```bash
rustc --target wasm32-unknown-unknown -O hello.rs -o hello.wasm
```

### 3.2 Registry Integration

```rust
// crates/deriva-compute/src/registry.rs

impl FunctionRegistry {
    /// Register WASM function from DagStore
    pub async fn register_wasm(
        &mut self,
        dag: &DagStore,
        addr: CAddr,
        loader: &WasmLoader,
    ) -> Result<(), RegistryError> {
        let func = loader.load(dag, addr).await?;
        self.register(func);
        Ok(())
    }
}
```

### 3.3 Executor Integration

```rust
// crates/deriva-compute/src/executor.rs

impl Executor {
    pub async fn materialize(&mut self, addr: CAddr) -> Result<Bytes, ExecutionError> {
        // ... existing logic ...
        
        // Check if function is WASM
        if recipe.function_id.as_str().starts_with("wasm:") {
            // Extract WASM module CAddr
            let wasm_addr = CAddr::from_hex(
                recipe.function_id.as_str().strip_prefix("wasm:").unwrap()
            )?;
            
            // Load WASM function
            let wasm_func = self.wasm_loader.load(&self.dag, wasm_addr).await?;
            
            // Execute
            let result = wasm_func.execute(input_bytes, &recipe.params)?;
            
            return Ok(result);
        }
        
        // ... native function execution ...
    }
}
```

### 3.4 Deterministic Float Integration

```rust
// crates/deriva-compute/src/wasm_function.rs

impl WasmFunction {
    pub fn from_bytes(
        id: FunctionId,
        wasm_bytes: &[u8],
        config: WasmConfig,
    ) -> Result<Self, WasmError> {
        let mut engine_config = Config::new();
        
        // Enable fuel metering
        engine_config.consume_fuel(config.enable_fuel);
        
        // CRITICAL: Enable NaN canonicalization (§4.4)
        if config.deterministic_floats {
            engine_config.cranelift_nan_canonicalization(true);
        }
        
        // Disable non-deterministic features
        engine_config.cranelift_opt_level(OptLevel::Speed);
        engine_config.parallel_compilation(false);  // Deterministic compilation
        
        let engine = Engine::new(&engine_config)?;
        let module = Module::from_binary(&engine, wasm_bytes)?;
        
        Ok(Self { id, module, config })
    }
}
```

### 3.5 Resource Limits

```rust
// crates/deriva-compute/src/wasm_function.rs

impl ComputeFunction for WasmFunction {
    fn execute(
        &self,
        inputs: Vec<Bytes>,
        params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        let mut store = Store::new(
            self.module.engine(),
            WasmState {
                memory_limit: self.config.max_memory,
            },
        );
        
        // Set memory limit
        store.limiter(|state| &mut state.memory_limit);
        
        // Set fuel limit (CPU)
        if self.config.enable_fuel {
            store.add_fuel(self.config.fuel_limit)
                .map_err(|_| ComputeError::OutOfFuel)?;
        }
        
        // Instantiate with timeout
        let instance = tokio::time::timeout(
            Duration::from_millis(self.config.max_time_ms),
            async { Instance::new(&mut store, &self.module, &[]) },
        )
        .await
        .map_err(|_| ComputeError::Timeout(self.config.max_time_ms))??;
        
        // ... rest of execution ...
        
        // Check remaining fuel
        let remaining_fuel = store.fuel_consumed()
            .map_err(|_| ComputeError::FuelError)?;
        
        if remaining_fuel == 0 {
            return Err(ComputeError::OutOfFuel);
        }
        
        Ok(result)
    }
}

impl wasmtime::ResourceLimiter for WasmState {
    fn memory_growing(&mut self, current: usize, desired: usize, _maximum: Option<usize>) -> bool {
        desired <= self.memory_limit as usize
    }

    fn table_growing(&mut self, _current: u32, _desired: u32, _maximum: Option<u32>) -> bool {
        true
    }
}
```

---

## 4. Data Flow Diagrams

### 4.1 WASM Function Execution

```
┌──────┐                                                      ┌──────────┐
│Client│                                                      │ Executor │
└──┬───┘                                                      └────┬─────┘
   │                                                                │
   │ materialize(addr)                                             │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                          Load recipe           │
   │                                          ─────────────         │
   │                                                                │
   │                                          function_id = "wasm:abc"│
   │                                          ─────────────         │
   │                                                                │
   │                                          Load WASM module      │
   │                                          ─────────────         │
   │                                                                │
   │                                          Create store + limits │
   │                                          ─────────────         │
   │                                                                │
   │                                          Instantiate module    │
   │                                          ─────────────         │
   │                                                                │
   │                                          Serialize inputs      │
   │                                          ─────────────         │
   │                                                                │
   │                                          Call execute()        │
   │                                          ─────────────         │
   │                                                                │
   │                                          Check fuel/memory     │
   │                                          ─────────────         │
   │                                                                │
   │                                          Deserialize result    │
   │                                          ─────────────         │
   │                                                                │
   │ Bytes                                                         │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

### 4.2 WASM Module Loading

```
┌──────┐                                                      ┌────────────┐
│Client│                                                      │ WasmLoader │
└──┬───┘                                                      └─────┬──────┘
   │                                                                │
   │ load(dag, addr)                                               │
   │───────────────────────────────────────────────────────────────>
   │                                                                │
   │                                          Check cache           │
   │                                          ─────────────         │
   │                                                                │
   │                                          ✗ Not cached          │
   │                                                                │
   │                                          dag.get(addr)         │
   │                                          ─────────────         │
   │                                                                │
   │                                          Validate WASM         │
   │                                          ─────────────         │
   │                                                                │
   │                                          ✓ Valid exports       │
   │                                                                │
   │                                          Compile module        │
   │                                          ─────────────         │
   │                                                                │
   │                                          Cache module          │
   │                                          ─────────────         │
   │                                                                │
   │ Arc<WasmFunction>                                             │
   │<───────────────────────────────────────────────────────────────
   │                                                                │
```

### 4.3 Resource Limit Enforcement

```
┌──────────┐                                                  ┌───────┐
│WasmFunction│                                                │ Store │
└─────┬────┘                                                  └───┬───┘
      │                                                            │
      │ execute(inputs, params)                                   │
      │────────────────────────────────────────────────────────────>
      │                                                            │
      │                                          add_fuel(1B)      │
      │                                          ─────────────     │
      │                                                            │
      │                                          set_memory_limit  │
      │                                          ─────────────     │
      │                                                            │
      │                                          instantiate()     │
      │                                          ─────────────     │
      │                                                            │
      │                                          call execute()    │
      │                                          ─────────────     │
      │                                                            │
      │                                          [Fuel consumed]   │
      │                                          ─────────────     │
      │                                                            │
      │                                          fuel_consumed() > limit?│
      │                                          ─────────────     │
      │                                                            │
      │                                          ✗ Within limit    │
      │                                                            │
      │ Result                                                    │
      │<────────────────────────────────────────────────────────────
      │                                                            │
```

---

## 5. Test Specification

### 5.1 Unit Tests

```rust
// crates/deriva-compute/tests/wasm_function.rs

#[tokio::test]
async fn test_wasm_hello_world() {
    let wasm_bytes = include_bytes!("../examples/wasm_functions/hello.wasm");
    let config = WasmConfig::default();
    
    let func = WasmFunction::from_bytes(
        FunctionId::from("hello"),
        wasm_bytes,
        config,
    ).unwrap();
    
    let inputs = vec![Bytes::from("World")];
    let params = BTreeMap::new();
    
    let result = func.execute(inputs, &params).unwrap();
    assert_eq!(result, Bytes::from("Hello, World!"));
}

#[tokio::test]
async fn test_wasm_memory_limit() {
    let wasm_bytes = include_bytes!("../examples/wasm_functions/memory_hog.wasm");
    let config = WasmConfig {
        max_memory: 1024 * 1024,  // 1 MB
        ..Default::default()
    };
    
    let func = WasmFunction::from_bytes(
        FunctionId::from("memory_hog"),
        wasm_bytes,
        config,
    ).unwrap();
    
    let result = func.execute(vec![], &BTreeMap::new());
    assert!(matches!(result, Err(ComputeError::MemoryLimit)));
}

#[tokio::test]
async fn test_wasm_fuel_limit() {
    let wasm_bytes = include_bytes!("../examples/wasm_functions/infinite_loop.wasm");
    let config = WasmConfig {
        fuel_limit: 1_000_000,  // 1M instructions
        ..Default::default()
    };
    
    let func = WasmFunction::from_bytes(
        FunctionId::from("infinite_loop"),
        wasm_bytes,
        config,
    ).unwrap();
    
    let result = func.execute(vec![], &BTreeMap::new());
    assert!(matches!(result, Err(ComputeError::OutOfFuel)));
}

#[tokio::test]
async fn test_wasm_timeout() {
    let wasm_bytes = include_bytes!("../examples/wasm_functions/slow.wasm");
    let config = WasmConfig {
        max_time_ms: 100,  // 100ms
        ..Default::default()
    };
    
    let func = WasmFunction::from_bytes(
        FunctionId::from("slow"),
        wasm_bytes,
        config,
    ).unwrap();
    
    let result = func.execute(vec![], &BTreeMap::new());
    assert!(matches!(result, Err(ComputeError::Timeout(_))));
}
```

### 5.2 Determinism Tests

```rust
// crates/deriva-compute/tests/wasm_determinism.rs

#[tokio::test]
async fn test_wasm_deterministic_floats() {
    let wasm_bytes = include_bytes!("../examples/wasm_functions/float_math.wasm");
    let config = WasmConfig {
        deterministic_floats: true,
        ..Default::default()
    };
    
    let func = WasmFunction::from_bytes(
        FunctionId::from("float_math"),
        wasm_bytes,
        config,
    ).unwrap();
    
    let inputs = vec![Bytes::from(1.5f64.to_le_bytes().to_vec())];
    
    // Run 10 times
    let mut results = Vec::new();
    for _ in 0..10 {
        let result = func.execute(inputs.clone(), &BTreeMap::new()).unwrap();
        results.push(result);
    }
    
    // All results should be identical
    for result in &results[1..] {
        assert_eq!(result, &results[0]);
    }
}

#[tokio::test]
async fn test_wasm_cross_platform_determinism() {
    let wasm_bytes = include_bytes!("../examples/wasm_functions/compute.wasm");
    let config = WasmConfig::default();
    
    let func = WasmFunction::from_bytes(
        FunctionId::from("compute"),
        wasm_bytes,
        config,
    ).unwrap();
    
    let inputs = vec![Bytes::from("test input")];
    let result = func.execute(inputs, &BTreeMap::new()).unwrap();
    
    // Golden hash (same on x86_64 and aarch64)
    let expected_hash = CAddr::from_hex("abc123...").unwrap();
    let actual_hash = CAddr::from_bytes(&result);
    
    assert_eq!(actual_hash, expected_hash);
}
```

### 5.3 Integration Tests

```rust
// crates/deriva-compute/tests/wasm_integration.rs

#[tokio::test]
async fn test_wasm_in_executor() {
    let dag = DagStore::new_memory();
    let mut registry = FunctionRegistry::new();
    let mut cache = ExecutionCache::new(1000);
    let leaf_store = LeafStore::new_memory();
    
    // Load WASM function
    let wasm_bytes = include_bytes!("../examples/wasm_functions/hello.wasm");
    let wasm_addr = CAddr::from_bytes(wasm_bytes);
    dag.put(wasm_addr, Bytes::from(wasm_bytes.to_vec())).await.unwrap();
    
    let loader = WasmLoader::new(WasmConfig::default());
    registry.register_wasm(&dag, wasm_addr, &loader).await.unwrap();
    
    // Create recipe
    let input = Bytes::from("World");
    let input_addr = CAddr::from_bytes(&input);
    dag.put(input_addr, input.clone()).await.unwrap();
    
    let recipe = Recipe {
        function_id: FunctionId::from(format!("wasm:{}", wasm_addr.to_hex())),
        inputs: vec![input_addr],
        params: BTreeMap::new(),
    };
    let recipe_addr = recipe.addr();
    
    // Execute
    let mut executor = Executor::new(&dag, &registry, &mut cache, &leaf_store, ExecutorConfig::default());
    let result = executor.materialize(recipe_addr).await.unwrap();
    
    assert_eq!(result, Bytes::from("Hello, World!"));
}
```

---

## 6. Edge Cases & Error Handling

### 6.1 Invalid WASM Module

```rust
#[tokio::test]
async fn test_invalid_wasm() {
    let invalid_bytes = b"not a wasm module";
    let config = WasmConfig::default();
    
    let result = WasmFunction::from_bytes(
        FunctionId::from("invalid"),
        invalid_bytes,
        config,
    );
    
    assert!(matches!(result, Err(WasmError::Wasmtime(_))));
}
```

### 6.2 Missing Exports

```rust
#[tokio::test]
async fn test_missing_exports() {
    let wasm_bytes = include_bytes!("../examples/wasm_functions/no_execute.wasm");
    let loader = WasmLoader::new(WasmConfig::default());
    
    let result = loader.validate(wasm_bytes);
    assert!(matches!(result, Err(WasmError::MissingExport("execute"))));
}
```

### 6.3 Trap During Execution

```rust
#[tokio::test]
async fn test_wasm_trap() {
    let wasm_bytes = include_bytes!("../examples/wasm_functions/panic.wasm");
    let config = WasmConfig::default();
    
    let func = WasmFunction::from_bytes(
        FunctionId::from("panic"),
        wasm_bytes,
        config,
    ).unwrap();
    
    let result = func.execute(vec![], &BTreeMap::new());
    assert!(matches!(result, Err(ComputeError::Trap(_))));
}
```

### 6.4 Large WASM Module

```rust
#[tokio::test]
async fn test_large_wasm_module() {
    let large_bytes = vec![0u8; 20 * 1024 * 1024];  // 20 MB
    let loader = WasmLoader::new(WasmConfig::default());
    
    let result = loader.validate(&large_bytes);
    assert!(matches!(result, Err(WasmError::TooLarge)));
}
```

---

## 7. Performance Analysis

### 7.1 Execution Overhead

**Native Rust function:**
```rust
fn execute(&self, inputs: Vec<Bytes>, _: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    Ok(inputs[0].clone())
}
// ~1 µs
```

**WASM function (same logic):**
```rust
// Instantiation: ~100 µs
// Serialization: ~10 µs
// Execution: ~1 µs
// Deserialization: ~10 µs
// Total: ~121 µs (121x overhead)
```

**Mitigation:** Cache compiled modules (reduces to ~21 µs, 21x overhead).

### 7.2 Memory Overhead

**Native function:**
- Stack: ~1 KB
- Heap: variable

**WASM function:**
- Linear memory: 64 MB (default)
- Stack: ~1 KB
- Module cache: ~1 MB per module

**Total:** ~65 MB per WASM function (vs ~1 KB for native).

### 7.3 Compilation Time

**WASM module compilation:**
- Small module (10 KB): ~10 ms
- Medium module (100 KB): ~50 ms
- Large module (1 MB): ~200 ms

**Mitigation:** Pre-compile and cache modules.

---

## 8. Files Changed

### New Files
- `crates/deriva-compute/src/wasm_function.rs` — `WasmFunction` wrapper
- `crates/deriva-compute/src/wasm_loader.rs` — Module loader with validation
- `crates/deriva-compute/src/wasm_config.rs` — Configuration types
- `crates/deriva-compute/tests/wasm_function.rs` — Unit tests
- `crates/deriva-compute/tests/wasm_determinism.rs` — Determinism tests
- `crates/deriva-compute/tests/wasm_integration.rs` — Integration tests
- `examples/wasm_functions/hello.rs` — Example WASM function
- `examples/wasm_functions/float_math.rs` — Float determinism example
- `examples/wasm_functions/Makefile` — Build script for WASM modules

### Modified Files
- `crates/deriva-compute/src/executor.rs` — Add WASM function support
- `crates/deriva-compute/src/registry.rs` — Add `register_wasm()`
- `crates/deriva-compute/src/lib.rs` — Export WASM types
- `crates/deriva-compute/Cargo.toml` — Add wasmtime dependency

---

## 9. Dependency Changes

```toml
# crates/deriva-compute/Cargo.toml
[dependencies]
wasmtime = "18.0"
tokio = { version = "1.35", features = ["time"] }
bincode = "1.3"
bytes = "1.5"
thiserror = "1.0"

[dev-dependencies]
tokio = { version = "1.35", features = ["full"] }
```

**New dependency:** `wasmtime` (WASM runtime)
- Size: ~10 MB
- Compile time: +30 seconds
- Runtime overhead: ~10% for WASM functions

---

## 10. Design Rationale

### 10.1 Why Wasmtime?

**Alternatives:**
- **wasmer**: Similar features, but less mature
- **wasm3**: Interpreter (slow), but smaller binary
- **lucet**: AOT compiler (fast), but less flexible

**Decision:** Wasmtime is the most mature, well-tested, and actively maintained WASM runtime.

### 10.2 Why Fuel Metering?

**Alternative:** Use OS-level CPU limits (cgroups).

**Problem:** OS limits are coarse-grained (per-process, not per-function).

**Decision:** Fuel metering provides fine-grained CPU limits per WASM function.

### 10.3 Why NaN Canonicalization?

**Problem:** WASM floats are non-deterministic by default (§4.4).

**Solution:** Wasmtime's `cranelift_nan_canonicalization` ensures all NaNs are canonical.

**Trade-off:** ~5% performance overhead for float-heavy workloads.

### 10.4 Why Module Caching?

**Problem:** Compiling WASM modules is slow (~50 ms for 100 KB module).

**Solution:** Cache compiled modules in memory.

**Trade-off:** ~1 MB memory per cached module.

---

## 11. Observability Integration

### 11.1 Metrics

```rust
lazy_static! {
    static ref WASM_EXECUTIONS: IntCounterVec = register_int_counter_vec!(
        "deriva_wasm_executions_total",
        "WASM function executions by result",
        &["result"]
    ).unwrap();

    static ref WASM_EXECUTION_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_wasm_execution_duration_seconds",
        "WASM execution duration by function",
        &["function_id"]
    ).unwrap();
    
    static ref WASM_FUEL_CONSUMED: Histogram = register_histogram!(
        "deriva_wasm_fuel_consumed",
        "Fuel consumed by WASM functions"
    ).unwrap();
    
    static ref WASM_MEMORY_USED: Histogram = register_histogram!(
        "deriva_wasm_memory_used_bytes",
        "Memory used by WASM functions"
    ).unwrap();
    
    static ref WASM_MODULE_CACHE_SIZE: IntGauge = register_int_gauge!(
        "deriva_wasm_module_cache_size",
        "Number of cached WASM modules"
    ).unwrap();
}
```

### 11.2 Logs

```rust
impl ComputeFunction for WasmFunction {
    fn execute(
        &self,
        inputs: Vec<Bytes>,
        params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        let start = Instant::now();
        
        debug!("Executing WASM function {}", self.id);
        
        // ... execution logic ...
        
        let duration = start.elapsed();
        let fuel_consumed = store.fuel_consumed().unwrap_or(0);
        
        WASM_EXECUTION_DURATION
            .with_label_values(&[self.id.as_str()])
            .observe(duration.as_secs_f64());
        WASM_FUEL_CONSUMED.observe(fuel_consumed as f64);
        WASM_EXECUTIONS.with_label_values(&["success"]).inc();
        
        info!(
            "WASM function {} completed in {:?}, fuel: {}",
            self.id, duration, fuel_consumed
        );
        
        Ok(result)
    }
}
```

### 11.3 Tracing

```rust
use tracing::{info_span, instrument};

#[instrument(skip(self, inputs, params))]
fn execute(
    &self,
    inputs: Vec<Bytes>,
    params: &BTreeMap<String, Value>,
) -> Result<Bytes, ComputeError> {
    let span = info_span!(
        "wasm_execute",
        function_id = %self.id,
        input_count = inputs.len()
    );
    let _enter = span.enter();
    
    // ... execution logic ...
}
```

---

## 12. Checklist

### Implementation
- [ ] Create `deriva-compute/src/wasm_function.rs` with `WasmFunction` type
- [ ] Implement `ComputeFunction` trait for `WasmFunction`
- [ ] Add resource limits (memory, fuel, timeout)
- [ ] Enable NaN canonicalization for deterministic floats
- [ ] Create `deriva-compute/src/wasm_loader.rs` with validation
- [ ] Implement module caching
- [ ] Add `register_wasm()` to `FunctionRegistry`
- [ ] Integrate WASM functions into `Executor`
- [ ] Create example WASM functions (hello, float_math, etc.)
- [ ] Add build script for WASM examples

### Testing
- [ ] Unit test: WASM hello world
- [ ] Unit test: memory limit enforcement
- [ ] Unit test: fuel limit enforcement
- [ ] Unit test: timeout enforcement
- [ ] Unit test: deterministic float execution
- [ ] Unit test: cross-platform determinism (golden hashes)
- [ ] Unit test: invalid WASM module
- [ ] Unit test: missing exports
- [ ] Unit test: trap during execution
- [ ] Integration test: WASM in executor
- [ ] Integration test: WASM with §4.3 proofs
- [ ] Benchmark: execution overhead (target: <10x native)
- [ ] Benchmark: compilation time (target: <100ms for 100KB)

### Documentation
- [ ] Document WASM function interface (alloc, execute, memory)
- [ ] Add examples of writing WASM functions in Rust
- [ ] Document resource limits and configuration
- [ ] Add troubleshooting guide for WASM errors
- [ ] Document deterministic float integration
- [ ] Add security considerations (sandboxing, limits)

### Observability
- [ ] Add `deriva_wasm_executions_total` counter
- [ ] Add `deriva_wasm_execution_duration_seconds` histogram
- [ ] Add `deriva_wasm_fuel_consumed` histogram
- [ ] Add `deriva_wasm_memory_used_bytes` histogram
- [ ] Add `deriva_wasm_module_cache_size` gauge
- [ ] Add debug logs for WASM execution
- [ ] Add tracing spans for WASM functions

### Validation
- [ ] Run tests on x86_64 and aarch64
- [ ] Verify deterministic execution (same CAddr across platforms)
- [ ] Benchmark execution overhead (<10x native)
- [ ] Test resource limits (memory, fuel, timeout)
- [ ] Verify NaN canonicalization works
- [ ] Test with real WASM modules (image processing, ML inference)
- [ ] Verify module caching reduces overhead

### Deployment
- [ ] Deploy with WASM support enabled
- [ ] Monitor WASM execution metrics
- [ ] Set default resource limits (64 MB memory, 1B fuel, 5s timeout)
- [ ] Document WASM deployment workflow
- [ ] Add admin API to list/reload WASM modules
- [ ] Set up CI to build example WASM modules

---

**Estimated effort:** 7–9 days
- Days 1-2: Core `WasmFunction` wrapper + Wasmtime integration
- Days 3-4: Resource limits (memory, fuel, timeout) + deterministic floats
- Day 5: Module loader + validation + caching
- Day 6: Executor integration + registry
- Days 7-8: Tests + benchmarks + example WASM functions
- Day 9: Documentation + observability

**Success criteria:**
1. All tests pass (execution, limits, determinism)
2. WASM functions are deterministic (same CAddr across platforms)
3. Execution overhead <10x native Rust
4. Resource limits work (memory, fuel, timeout)
5. NaN canonicalization ensures float determinism
6. Module caching reduces overhead to <2x native
7. Example WASM functions compile and run correctly
