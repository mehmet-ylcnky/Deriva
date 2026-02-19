# Phase 2.8 Complete — Garbage Collection

## Summary
Mark-and-sweep garbage collector for the computation-addressed DFS. Identifies and removes orphaned blobs, broken recipes, and stale cache entries while respecting pinned addresses.

## Implemented
- **GcResult / GcConfig / PinSet** — core types in `deriva-core/src/gc.rs`
- **BlobStore extensions** — `remove_with_size`, `remove_batch_blobs`, `list_addrs`, `stats`
- **Live set computation** — `PersistentDag::live_addr_set()` + `compute_live_set(dag, pins)`
- **GC engine** — `run_gc()` async mark-and-sweep in `deriva-server/src/gc.rs`
- **Proto + gRPC** — 4 RPCs: `GarbageCollect`, `Pin`, `Unpin`, `ListPins`
- **CLI** — `gc`, `pin`, `unpin`, `list-pins` commands
- **Observability** — 6 Prometheus metrics (runs, blobs removed, bytes reclaimed, duration, live blobs, pinned count)
- **Benchmarks** — 4 groups: sweep, live set, list_addrs, dry vs actual

## Test Results
- 516 tests passing across workspace (40 new GC tests)
- `cargo clippy --workspace --lib -- -D warnings` clean

## Key Design Decisions
- GC engine in `deriva-server` (not `deriva-compute`) to avoid circular dependency
- PinSet behind `Arc<RwLock>` in ServerState for concurrent access
- Grace period config field for race protection between put_leaf and put_recipe
- max_removals for incremental GC cycles

## Checklist
- [x] GcResult, GcConfig, PinSet types
- [x] BlobStore extensions (remove_with_size, remove_batch_blobs, list_addrs, stats)
- [x] Live set computation (compute_live_set)
- [x] GC engine (run_gc)
- [x] PinSet in ServerState
- [x] Proto definitions + 4 RPC handlers
- [x] CLI commands (gc, pin, unpin, list-pins)
- [x] Unit tests: PinSet (8), BlobStore (8), live set (8), GC engine (8)
- [x] Integration tests (8)
- [x] GC metrics (6 Prometheus metrics)
- [x] Benchmarks (4 groups)
- [x] Full test suite passing (516 tests)
- [x] Clippy clean
