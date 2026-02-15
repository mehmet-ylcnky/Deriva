# Phase 2.1 Implementation Readiness

**Date:** 2026-02-15  
**Section:** Persistent DAG Store  
**Blueprint:** `docs/phase2/section-2.1-persistent-dag.md` (1,512 lines)

---

## ✅ Pre-Implementation Checklist

- [x] **Baseline metrics established** — See `PHASE1_BASELINE.md`
- [x] **Blueprint reviewed** — 1,512 lines, comprehensive design
- [x] **Docker environment ready** — 3-node cluster configured
- [x] **Tests passing** — 104 tests, 0 clippy warnings
- [x] **Dependencies up to date** — Rust 1.93.0, all crates current

---

## Blueprint Summary

### Problem
- Current `DagStore` is in-memory only
- Startup requires O(N) rebuild from all recipes in sled
- 1M recipes = ~100 second startup time
- Recipes duplicated in memory (sled + DAG)

### Solution
- `PersistentDag` with sled-backed adjacency lists
- Store only forward/reverse edges, not full recipes
- LRU cache for hot adjacency lists
- O(1) startup time regardless of recipe count

### Key Design Points

1. **Two sled trees:**
   - `tree:forward` — addr → [input_addrs]
   - `tree:reverse` — addr → {dependent_addrs}

2. **In-memory LRU cache:**
   - Cache size: 1000 entries (configurable)
   - Eviction: LRU policy
   - Write-through to sled

3. **API compatibility:**
   - Same interface as current `DagStore`
   - Drop-in replacement
   - No changes to calling code

4. **Memory savings:**
   - Phase 1: ~200 bytes per recipe (full Recipe + edges)
   - Phase 2.1: ~50 bytes per recipe (edges only)
   - 1M recipes: 200 MB → 50 MB (75% reduction)

5. **Startup improvement:**
   - Phase 1: O(N) — iterate all recipes, rebuild DAG
   - Phase 2.1: O(1) — adjacency lists already in sled
   - 1M recipes: 100s → 1ms (100,000x improvement)

### Implementation Plan

**Files to create:**
- `crates/deriva-core/src/persistent_dag.rs` — New `PersistentDag` struct

**Files to modify:**
- `crates/deriva-core/src/lib.rs` — Export `PersistentDag`
- `crates/deriva-storage/src/recipe_store.rs` — Remove `rebuild_dag()`
- `crates/deriva-server/src/main.rs` — Use `PersistentDag` instead of `DagStore`

**Tests to add (~20):**
- Persistent DAG insert/query
- Restart persistence
- LRU cache behavior
- Cycle detection
- Topological sort
- Dependents query
- Forward/reverse edge consistency

### Expected Effort
- **Implementation:** 1-2 days
- **Testing:** 0.5-1 day
- **Total:** 1.5-3 days

---

## Unclear Requirements (None Identified)

The blueprint is comprehensive and clear. All design decisions are well-documented with:
- Rationale for each choice
- Data structures specified
- API contracts defined
- Test cases outlined
- Performance targets quantified

**No blockers identified.**

---

## Phase 2.1 Success Criteria

| Metric | Current (Phase 1) | Target (Phase 2.1) | How to Measure |
|--------|-------------------|--------------------| ---------------|
| Startup time (1K recipes) | ~100 ms | <1 ms | Benchmark |
| Startup time (100K recipes) | ~10 s | <1 ms | Benchmark |
| Memory (1M recipes) | ~200 MB | ~50 MB | Process RSS |
| Tests passing | 104 | ~124 | `cargo test` |
| Clippy warnings | 0 | 0 | `cargo clippy` |

---

## Ready to Implement

**Status:** ✅ **READY**

All prerequisites complete:
1. ✅ Baseline metrics documented
2. ✅ Blueprint thoroughly reviewed
3. ✅ No unclear requirements
4. ✅ Success criteria defined
5. ✅ Development environment configured

**Next command:**
```bash
# Create Phase 2 branch (when ready)
git checkout -b phase-2

# Start implementation
# Create crates/deriva-core/src/persistent_dag.rs
```

**Estimated completion:** 1.5-3 days from start
