# §7.7 Benchmark Specifications — Fusible Chain Scaling Results

**Date:** 2026-03-04
**Platform:** Linux x86_64, single-threaded tokio runtime
**Benchmark framework:** Criterion 0.5
**Benchmark file:** `crates/deriva-benchmarks/benches/fusible_chain_scaling.rs`

## Fusible Functions Tested

All functions marked `is_fusible() -> true` using `spawn_map` (per-chunk, stateless transforms):

| Function | Transform | Data Expansion |
|----------|-----------|----------------|
| Identity | passthrough | 1× |
| BitwiseNot | byte-level NOT | 1× |
| Uppercase | ASCII case | 1× |
| Lowercase | ASCII case | 1× |
| HexEncode | bytes → hex string | 2× |
| HexDecode | hex string → bytes | 0.5× |
| Base64Encode | bytes → base64 | 1.33× |
| Base64Decode | base64 → bytes | 0.75× |

## Benchmark Scenarios

| ID | Scenario | Data Size | Functions Tested |
|----|----------|-----------|------------------|
| B1 | HexEncode→HexDecode roundtrip depth 2–16 | 64 KB | HexEncode, HexDecode |
| B2 | BitwiseNot chain depth 2–16 | 64 KB | BitwiseNot |
| B3 | Base64 roundtrip depth 2–8 | 64 KB | Base64Encode, Base64Decode |
| B4 | Uppercase→Lowercase roundtrip depth 2–8 | 64 KB | Uppercase, Lowercase |
| B5 | Identity chain depth 1–16 (pure overhead) | 64 KB | Identity |
| B6 | Mixed: HexEncode→Uppercase→HexDecode depth 3–9 | 64 KB | HexEncode, Uppercase, HexDecode |
| B7 | Single fusible function throughput | 1 MB | Identity, BitwiseNot, Uppercase, HexEncode, Base64Encode |
| B8 | BitwiseNot at 64 KB vs 1 MB × depth 4, 8 | 64 KB / 1 MB | BitwiseNot |
| B9 | Base64Encode→Encrypt→Decrypt→Base64Decode | 64 KB | Base64Encode/Decode, Encrypt, Decrypt |
| B10 | Full fusible suite at depth 4 | 64 KB | all chain types |

## Raw Criterion Results (median times)

```
B1_hex_roundtrip/depth/2                2.703 ms    (64 KB)
B1_hex_roundtrip/depth/4                4.394 ms
B1_hex_roundtrip/depth/8                9.514 ms
B1_hex_roundtrip/depth/16              19.212 ms

B2_bitwise_not_chain/depth/2            77.3 µs     (64 KB)
B2_bitwise_not_chain/depth/4           123.1 µs
B2_bitwise_not_chain/depth/8           123.8 µs
B2_bitwise_not_chain/depth/16          185.4 µs

B3_base64_roundtrip/depth/2            178.8 µs     (64 KB)
B3_base64_roundtrip/depth/4            205.6 µs
B3_base64_roundtrip/depth/8            357.5 µs

B4_case_roundtrip/depth/2               75.2 µs     (64 KB)
B4_case_roundtrip/depth/4               97.5 µs
B4_case_roundtrip/depth/8              139.5 µs

B5_identity_chain/depth/1               69.2 µs     (64 KB)
B5_identity_chain/depth/4               84.2 µs
B5_identity_chain/depth/8               88.0 µs
B5_identity_chain/depth/16              89.4 µs

B6_mixed_fusible/depth/3              3297.0 µs     (64 KB)
B6_mixed_fusible/depth/6              5389.0 µs
B6_mixed_fusible/depth/9              7922.0 µs

B7_single_fusible_1mb/identity          78.4 µs     (1 MB)
B7_single_fusible_1mb/bitwise_not      124.2 µs
B7_single_fusible_1mb/uppercase        135.8 µs
B7_single_fusible_1mb/base64_encode    569.6 µs
B7_single_fusible_1mb/hex_encode     38676.0 µs

B8_not_size_depth/64KB/4               103.3 µs
B8_not_size_depth/64KB/8               114.4 µs
B8_not_size_depth/1MB/4                237.1 µs
B8_not_size_depth/1MB/8                419.6 µs

B9_b64_encrypt_decrypt_b64_64k        178.7 µs     (64 KB)

B10_full_fusible_depth4/identity        83.9 µs     (64 KB)
B10_full_fusible_depth4/bitwise_not     92.5 µs
B10_full_fusible_depth4/case_roundtrip  98.6 µs
B10_full_fusible_depth4/base64_roundtrip 204.3 µs
B10_full_fusible_depth4/hex_roundtrip  5354.2 µs
```

## Detailed Analysis

### B5: Identity Chain — Pure Pipeline Overhead

| Depth | Time (µs) | Per-Stage (µs) |
|------:|----------:|--------------:|
| 1 | 69.2 | 69.2 |
| 4 | 84.2 | 21.0 |
| 8 | 88.0 | 11.0 |
| 16 | 89.4 | 5.6 |

Identity chains reveal the pure channel overhead. Remarkably, depth 4→16 adds only ~5 µs total. This is because tokio's single-threaded runtime processes the entire chain synchronously within one poll cycle — the data flows through all stages without yielding. The per-stage overhead converges to ~1–2 µs at high depth, confirming the spec's prediction.

### B2: BitwiseNot Chain — Cheap Compute + Overhead

| Depth | Time (µs) | Per-Stage (µs) |
|------:|----------:|--------------:|
| 2 | 77.3 | 38.7 |
| 4 | 123.1 | 30.8 |
| 8 | 123.8 | 15.5 |
| 16 | 185.4 | 11.6 |

BitwiseNot allocates a new `Vec<u8>` per chunk per stage (byte-level NOT). The per-stage cost decreases with depth because the runtime amortizes task scheduling. At depth 8 and 16, the chain is nearly as fast as depth 4 — the allocation overhead is small for 64 KB chunks.

### B1: Hex Roundtrip — Expensive Transform Dominates

| Depth | Time (ms) | Per-Stage (µs) |
|------:|----------:|--------------:|
| 2 | 2.703 | 1,352 |
| 4 | 4.394 | 1,098 |
| 8 | 9.514 | 1,189 |
| 16 | 19.212 | 1,201 |

Hex roundtrip scales linearly with depth at ~1.2 ms per stage. HexEncode is extremely expensive (format!("{:02x}") per byte = 64K format calls) and dominates over pipeline overhead. This is the worst-case fusible function — at 1 MB it takes 38.7 ms (26 MB/s).

### B3: Base64 Roundtrip

| Depth | Time (µs) | Per-Stage (µs) |
|------:|----------:|--------------:|
| 2 | 178.8 | 89.4 |
| 4 | 205.6 | 51.4 |
| 8 | 357.5 | 44.7 |

Base64 is much faster than Hex because the base64 crate uses SIMD-optimized encoding. Per-stage cost is ~45–90 µs, compared to ~1,200 µs for Hex.

### B4: Case Roundtrip

| Depth | Time (µs) | Per-Stage (µs) |
|------:|----------:|--------------:|
| 2 | 75.2 | 37.6 |
| 4 | 97.5 | 24.4 |
| 8 | 139.5 | 17.4 |

Uppercase/Lowercase are byte-level transforms (ASCII range check + flip bit 5). Very cheap — approaching Identity overhead at high depth.

### B7: Single Fusible Function Throughput (1 MB)

| Function | Time | Throughput (MB/s) |
|----------|-----:|------------------:|
| Identity | 78.4 µs | 12,754 |
| BitwiseNot | 124.2 µs | 8,048 |
| Uppercase | 135.8 µs | 7,364 |
| Base64Encode | 569.6 µs | 1,756 |
| HexEncode | 38,676 µs | 26 |

```
Identity      12,754 MB/s  ████████████████████████████████████████
BitwiseNot     8,048 MB/s  █████████████████████████
Uppercase      7,364 MB/s  ███████████████████████
Base64Encode   1,756 MB/s  █████
HexEncode         26 MB/s  ▏
```

HexEncode is 490× slower than Identity due to per-byte `format!` string allocation. This is a prime candidate for optimization (lookup table or SIMD hex encoding).

### B8: Size vs Depth (BitwiseNot)

| Size | Depth 4 | Depth 8 | Depth 8/4 Ratio |
|------|--------:|--------:|----------------:|
| 64 KB | 103.3 µs | 114.4 µs | 1.11× |
| 1 MB | 237.1 µs | 419.6 µs | 1.77× |

At 64 KB, doubling depth adds only 11%. At 1 MB, doubling depth adds 77%. This shows that at larger data sizes, the per-stage compute cost (byte-level NOT on 1 MB) becomes significant relative to the pipeline overhead, so depth scaling is more linear.

### B9: Base64→Encrypt→Decrypt→Base64 Pipeline

- **4-stage pipeline:** 178.7 µs (64 KB)

This mixed pipeline (2 fusible + 2 crypto stages) completes in under 200 µs for 64 KB. The crypto stages (AES-CTR encrypt/decrypt) add minimal overhead because the base64-encoded data is only ~87 KB (1.33× expansion), and AES-NI processes this in microseconds.

### B10: Full Fusible Suite at Depth 4

| Chain Type | Time (64 KB) |
|------------|-------------:|
| Identity × 4 | 83.9 µs |
| BitwiseNot × 4 | 92.5 µs |
| Case roundtrip × 2 | 98.6 µs |
| Base64 roundtrip × 2 | 204.3 µs |
| Hex roundtrip × 2 | 5,354.2 µs |

## Pipeline Overhead Model

From the Identity chain (B5), the pipeline overhead model is:

```
Total time ≈ base_overhead + (depth × per_stage_overhead) + (depth × transform_cost)
```

Where:
- `base_overhead` ≈ 65 µs (task spawn + first channel send/recv)
- `per_stage_overhead` ≈ 1–2 µs (channel hop, amortized by tokio)
- `transform_cost` varies by function (0 for Identity, ~1.2 ms for HexEncode)

For cheap transforms (Identity, BitwiseNot, Case), pipeline overhead dominates and depth scaling is sub-linear. For expensive transforms (HexEncode), the transform cost dominates and depth scaling is linear.

## Key Takeaways

1. **Pipeline depth overhead is sub-linear for cheap transforms** — Identity at depth 16 (89 µs) is only 29% slower than depth 1 (69 µs). Tokio processes the chain in a single poll cycle.

2. **Per-stage overhead is ~1–2 µs** for fusible functions — negligible compared to any real transform cost.

3. **HexEncode is the bottleneck** at 26 MB/s (1 MB) — per-byte `format!` is 490× slower than Identity. A lookup-table implementation would bring this to ~1,000 MB/s.

4. **Base64 is 67× faster than Hex** thanks to SIMD-optimized encoding in the base64 crate.

5. **Mixed pipelines (B9) are efficient** — a 4-stage Base64→Encrypt→Decrypt→Base64 pipeline completes in 179 µs for 64 KB.

6. **Depth scaling depends on data size** — at 64 KB, doubling depth adds 11%; at 1 MB, it adds 77% (BitwiseNot). Larger data makes per-stage compute cost more visible.

7. **Fusion opportunity confirmed** — Identity chains at depth 16 take only 89 µs vs 69 µs at depth 1. With §2.11 fusion, a depth-16 chain could be collapsed to a single stage, saving ~20 µs (~23%).
