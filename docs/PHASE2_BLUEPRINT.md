# Phase 2: Robustness — Development Blueprint

> Make the single-node system production-grade.
>
> **Prerequisite:** Phase 1 complete (commit `1d97098`, 244 tests, 6 crates)
>
> **Estimated new tests:** ~80–100
>
> **Crates modified:** `deriva-core`, `deriva-compute`, `deriva-server`

## Sections

| Section | File | Goal | Est. Tests |
|---------|------|------|------------|
| [2.1](phase2/section-2.1-persistent-dag.md) | Persistent DAG Store | DAG survives restarts via sled | ~20 |
| [2.2](phase2/section-2.2-async-compute.md) | Async Compute Engine | Replace sync executor with async Tokio | ~15 |
| [2.3](phase2/section-2.3-parallel-materialization.md) | Parallel Materialization | Concurrent independent branch resolution | ~20 |
| [2.4](phase2/section-2.4-verification-mode.md) | Verification Mode | Dual-compute determinism checking | ~15 |
| [2.5](phase2/section-2.5-observability.md) | Observability | tracing + Prometheus metrics | ~15 |

## Architecture After Phase 2

```
┌─────────────────────────────────────────────────────────┐
│                     gRPC Service                        │
│                   (tonic, async)                         │
├─────────────────────────────────────────────────────────┤
│              Async Executor (Tokio tasks)                │
│         ┌──────────┬──────────┬──────────┐              │
│         │ Branch A │ Branch B │ Branch C │  parallel     │
│         └────┬─────┴────┬─────┴────┬─────┘              │
│              └──────────┼──────────┘                     │
│                    join point                            │
├──────────┬──────────┬──────────┬────────────────────────┤
│ DAG      │ Cache    │ Registry │ Verification            │
│ (sled)   │ (memory) │          │ (optional dual-compute) │
├──────────┴──────────┴──────────┴────────────────────────┤
│              Storage Backend                             │
│         SledRecipeStore + BlobStore                      │
├─────────────────────────────────────────────────────────┤
│              Observability                               │
│     tracing spans + Prometheus counters/histograms       │
└─────────────────────────────────────────────────────────┘
```

## Key Design Decisions

1. **DAG persistence uses sled** (same as recipe store) — not a separate DB
2. **Async executor** replaces sync `Executor` — the sync version remains as a fallback for tests
3. **Parallelism is DAG-aware** — only independent branches run concurrently
4. **Verification is opt-in** — per-request flag, not global (too expensive for default)
5. **Observability is always-on** — zero-cost when no subscriber attached (tracing's design)
