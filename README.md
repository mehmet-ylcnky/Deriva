<p align="center">
  <img src="logo.png" width="250" alt="Deriva logo"><br>
  A computation-addressed distributed file system built in Rust.
</p>

---

Deriva stores data by its content hash (BLAKE3) and treats computation as a first-class citizen — recipes describing how to derive data are stored alongside the data itself. If a result can be recomputed, it doesn't need to be replicated; the recipe travels instead.

## Core Concepts

- **Content-Addressing** — every piece of data is identified by its BLAKE3 hash (`CAddr`). Store once, reference everywhere.
- **Recipes** — a recipe is `function + inputs + params`. Its CAddr is deterministic: same computation always produces the same address.
- **DAG Store** — recipes form a directed acyclic graph. Deriva tracks dependencies so invalidation, re-computation, and garbage collection are graph walks.
- **Compute-over-Replicate** — instead of copying large results across nodes, Deriva can ship the recipe and re-derive locally when the inputs are available.

## Architecture

```
┌─────────────┐  gRPC   ┌─────────────────────────────────────────┐
│  deriva-cli │◄───────►│  deriva-server                          │
└─────────────┘         │  ┌──────────────┐   ┌────────────────┐  │
                        │  │deriva-compute│   │ deriva-storage │  │
                        │  └──────┬───────┘   └───────┬────────┘  │
                        │         │                   │           │
                        │     ┌───┴───────────────────┴┐          │
                        │     │      deriva-core       │          │
                        │     │  CAddr, Recipe, DAG,   │          │
                        │     │  Value, FunctionId     │          │
                        │     └────────────────────────┘          │
                        │                                         │
                        │  ┌──────────────┐                       │
                        │  │deriva-network│  gossip, routing      │
                        │  └──────────────┘                       │
                        └─────────────────────────────────────────┘
```

## Crates

| Crate | Purpose |
|-------|---------|
| `deriva-core` | Content-addressed types, DAG store, cache, value types |
| `deriva-compute` | Function registry, async executor, WASM plugins, versioning |
| `deriva-storage` | Persistent leaf/recipe storage, chunked reads |
| `deriva-network` | Gossip protocol, hash ring, replication, consistency |
| `deriva-server` | gRPC service, REST dashboard, FUSE mount |
| `deriva-cli` | Command-line client |

## Features

- BLAKE3 content-addressed storage
- DAG-based dependency tracking with automatic invalidation
- Async parallel computation with in-flight deduplication
- **Verification mode for determinism checking** (dual-compute, sampling)
- Cost-aware eviction cache
- WASM plugin system for user-defined compute functions
- Gossip-based cluster membership (SWIM protocol)
- Consistent hashing with tunable quorum (N/W/R)
- FUSE filesystem mount
- REST dashboard with observability endpoints
- Mutable references with history and cascade invalidation
- Pin API for non-evictable data
- Pre-warming scheduler based on access patterns
- Function versioning with migration support

## Quick Start

### Running the Server

```bash
# Default mode (no verification)
cargo run --bin deriva-server

# With verification enabled
cargo run --bin deriva-server -- --verification dual
cargo run --bin deriva-server -- --verification sampled:0.1  # Verify 10%
```

### Verification Mode

Deriva can detect non-deterministic compute functions by executing them twice and comparing outputs:

```bash
# Verify all recipes (development/testing)
deriva-server --verification dual

# Verify 10% of recipes (production monitoring)
deriva-server --verification sampled:0.1

# No verification (production default)
deriva-server --verification off
```

**Performance:** Parallel execution means dual-compute has minimal overhead (~32µs vs ~40µs for single execution).

### Troubleshooting Non-Determinism

If you see `determinism violation` errors:

**Common causes:**
- Random number generation (`rand::random()`)
- System time (`SystemTime::now()`)
- HashMap iteration order
- Thread IDs or race conditions

**Solutions:**
- Use `rand::SeedableRng` with deterministic seeds
- Pass timestamps as function parameters
- Use `BTreeMap` instead of `HashMap`
- Avoid global mutable state

**Example error:**
```
determinism violation for CAddr(a71479b7...): function my_func/1
produced different outputs (8 bytes hash=4d067153... vs 8 bytes hash=d63bd9a8...)
```

## Building

```bash
cargo build --workspace
```

## Testing

```bash
cargo test --workspace
```

## License

MIT
