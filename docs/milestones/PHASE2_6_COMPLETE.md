# Phase 2.6 Complete: Cascade Invalidation

**Date:** 2026-02-18  
**Commit:** 6be7f44  
**Status:** ✅ Complete

---

## Overview

Implemented cascade invalidation — when a leaf or recipe is invalidated, all downstream dependents are automatically evicted from cache. Supports configurable policies and depth-limited traversal.

## Implementation

### Core Components

**CascadePolicy enum:**
- `Immediate` — evict all downstream dependents immediately
- `DryRun` — report what would be evicted without acting
- `None` — no cascade, invalidate only the target

**CascadeInvalidator:**
- BFS traversal of the reverse dependency graph
- Depth tracking to limit blast radius
- Batch eviction for efficiency
- Both sync and async variants

### gRPC Integration

- `cascade_invalidate` RPC — new endpoint with policy selection
- Backward-compatible `invalidate` RPC unchanged

### Observability

- `CASCADE_DEPTH` metric tracks invalidation depth distribution

## Testing

Criterion benchmarks matching spec §7.3 scenarios for cascade performance across varying graph depths and fan-out patterns.

## Next Steps

Phase 2.7: Streaming Materialization
