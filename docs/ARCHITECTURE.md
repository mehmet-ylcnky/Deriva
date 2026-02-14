# Deriva — Architecture

> Computation-Addressed Distributed File System

## Overview

Deriva is a distributed file system where data is addressed by the computation that produces it.
Instead of `path → bytes` (traditional FS) or `hash(bytes) → bytes` (CAS), Deriva uses
`hash(recipe) → bytes` where a recipe is `(function, inputs, params)`.

This means the system knows *how* every piece of derived data was produced, enabling:
- **Recomputation on demand** — evicted data can be transparently recomputed
- **Structural provenance** — lineage is the address, not external metadata
- **Cost-aware eviction** — evict cheap-to-recompute data first
- **Cascade invalidation** — when an input changes, all dependents are known via the DAG

## Crate Structure

```
deriva/
├── Cargo.toml                  # Workspace root
├── proto/deriva.proto          # gRPC service definition
├── crates/
│   ├── deriva-core/            # CAddr, Value, Recipe, DataRef, DerivaError
│   ├── deriva-compute/         # ComputeFunction trait, registry, builtins, executor, cache
│   ├── deriva-storage/         # SledRecipeStore, BlobStore with sharding
│   ├── deriva-network/         # (stub — Phase 3 gossip/replication)
│   ├── deriva-server/          # gRPC service (tonic), DerivaService, integration tests
│   └── deriva-cli/             # CLI client (clap) — put, recipe, get, resolve, invalidate, status
```

### Dependency Graph

```
deriva-core          (no internal deps)
    ↑
    ├── deriva-compute   (core)
    ├── deriva-storage   (core)
    ├── deriva-network   (core) [stub]
    │
    └── deriva-server    (core, compute, storage)
            ↑
            └── deriva-cli   (core, server [dev])
```

## Core Types (deriva-core)

### CAddr — Computation Address
- 32-byte BLAKE3 hash
- For leaf data: `CAddr = blake3(bytes)`
- For derived data: `CAddr = blake3(function_id || sorted_input_addrs || sorted_params)`
- Implements: `Display` (hex), `FromStr`, `Ord`, `Hash`, `Serialize/Deserialize`

### Recipe
```rust
pub struct Recipe {
    pub function: FunctionId,    // (name, version)
    pub inputs: Vec<CAddr>,      // ordered input addresses
    pub params: BTreeMap<String, String>,  // deterministic ordering
}
```

### DataRef
```rust
pub enum DataRef {
    Leaf(CAddr),                 // raw data, content-addressed
    Derived(CAddr, Recipe),      // recipe-addressed, may or may not be materialized
}
```

### Value
```rust
pub enum Value {
    Bytes(Vec<u8>),
    String(String),
    Int(i64),
    Float(f64),
    List(Vec<Value>),
}
```

## Compute Engine (deriva-compute)

### ComputeFunction Trait
```rust
pub trait ComputeFunction: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn execute(&self, inputs: Vec<Value>, params: &BTreeMap<String, String>) -> Result<Value>;
}
```

### Built-in Functions
| Function | Description |
|----------|-------------|
| `concat` | Concatenate input byte sequences |
| `sha256` | SHA-256 hash of input |
| `uppercase` | Convert string to uppercase |
| `repeat` | Repeat string N times (param: `count`) |
| `slice` | Byte slice (params: `start`, `end`) |
| `length` | Return byte length as string |

### Executor
- Resolves a CAddr by walking the DAG recursively
- Cache check → recipe lookup → resolve inputs → execute function → cache result → return
- Handles diamond dependencies (same input resolved once)

### EvictableCache
- In-memory LRU with cost-aware eviction
- Cost score: `size_bytes * (1.0 / (1.0 + recomputation_cost))`
- Evicts lowest-cost entries first (cheap to recompute, large size)
- Tracks: hit count, miss count, total evictions

## Storage Backend (deriva-storage)

### SledRecipeStore
- Persists recipes in sled embedded database
- Key: CAddr bytes, Value: bincode-serialized Recipe
- Survives process restarts

### BlobStore
- File-based blob storage with 256-way sharding
- Path: `{root}/{first_2_hex_chars}/{full_hex_addr}`
- Streaming read/write for large blobs

## DAG Store (deriva-compute)

- In-memory directed acyclic graph
- Tracks: `CAddr → Vec<CAddr>` (inputs) and `CAddr → Vec<CAddr>` (dependents)
- Cycle detection on insert
- Topological sort for execution ordering
- Dependents query for cascade invalidation

## gRPC API (deriva-server)

6 RPCs defined in `proto/deriva.proto`:

| RPC | Request | Response | Description |
|-----|---------|----------|-------------|
| `PutLeaf` | `bytes data` | `bytes addr` | Store raw data, return CAddr |
| `PutRecipe` | function + inputs + params | `bytes addr` | Register recipe, return derived CAddr |
| `Get` | `bytes addr` | `stream bytes chunk` | Retrieve (materializing if needed), server-streaming |
| `Resolve` | `bytes addr` | recipe fields | Look up recipe for a CAddr |
| `Invalidate` | `bytes addr` | `bool was_cached` | Evict cached materialization |
| `Status` | empty | counters | Recipe count, blob count, cache stats |

## Testing

- **244 total tests** across 6 crates
- **23 integration tests** in `deriva-server/tests/integration.rs`
- In-process gRPC testing (bind to `127.0.0.1:0`, no external server needed)
- Key scenarios: leaf roundtrip, recipe registration, DAG resolution, diamond deps, cache hit/miss, persistence across restart, large blob streaming, error handling, concurrent operations

## Phase 1 Commits

| Section | Commit | Tests | Description |
|---------|--------|-------|-------------|
| 1.1 | `08f0bd8` | 8 | Scaffolding — workspace, 6 crates, DerivaError, proto stub |
| 1.2 | `29b463b` | 43 | Core Types — CAddr, FunctionId, Value, Recipe, DataRef |
| 1.3 | `a6c257b` | 31 | DAG Store — cycle detection, topo sort, dependents |
| 1.4 | `35bb099` | 44 | Compute Engine — ComputeFunction trait, registry, builtins, Executor |
| 1.5 | `d552b42` | 27 | EvictableCache with cost-aware eviction |
| 1.6 | `48652c5` | 28 | SledRecipeStore, BlobStore with sharding, StorageBackend |
| 1.7 | `da2447a` | 24 | Full tonic gRPC service (6 RPCs) |
| 1.8 | `0b65a05` | 14 | CLI client — put, recipe, get, resolve, invalidate, status |
| 1.9 | `1d97098` | 25 | 23 integration tests, fixed RepeatFn string params, fixed sled lock restart |
