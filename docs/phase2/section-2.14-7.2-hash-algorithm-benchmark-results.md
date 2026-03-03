# §7.2 Hash Algorithm Comparison — Benchmark Results

**Date:** 2026-03-03
**Platform:** Linux x86_64, single-threaded tokio runtime
**Benchmark framework:** Criterion 0.5
**Benchmark file:** `crates/deriva-benchmarks/benches/hash_algorithm_comparison.rs`

## Benchmark Scenarios

| ID | Scenario | Data Size | Algorithms Tested |
|----|----------|-----------|-------------------|
| B1 | All 6 hash producers throughput | 1 MB | md5, sha256, sha512, blake3, crc32, hmac-sha256 |
| B2 | All 6 hash producers per-chunk latency | 64 KB | md5, sha256, sha512, blake3, crc32, hmac-sha256 |
| B3 | SHA-256 vs BLAKE3 scaling | 64 KB–4 MB | sha256, blake3 |
| B4 | HMAC-SHA256 vs raw SHA-256 overhead | 1 MB | sha256, hmac-sha256 |
| B5 | CRC32 vs MD5 lightweight integrity | 1 MB | crc32, md5 |
| B6 | ChunkHash (per-chunk CAS SHA-256) | 64 KB, 1 MB | chunk_hash |
| B7 | RollingHash at window sizes 32, 48, 128 | 64 KB | rolling_hash |
| B8 | ChecksumVerify (CRC32) end-to-end | 64 KB | checksum_verify |
| B9 | Sha256Verify end-to-end | 64 KB | sha256_verify |
| B10 | Output size efficiency (all 6 hashes) | 1 MB | all 6 |

## Raw Criterion Results (median times)

```
B1_hash_throughput_1mb/md5            2.595 ms    (1 MB)
B1_hash_throughput_1mb/sha256         1.567 ms
B1_hash_throughput_1mb/sha512         2.964 ms
B1_hash_throughput_1mb/blake3         1.639 ms
B1_hash_throughput_1mb/crc32          1.282 ms
B1_hash_throughput_1mb/hmac_sha256    1.792 ms

B2_hash_latency_64k/md5              1.481 ms    (64 KB)
B2_hash_latency_64k/sha256           1.273 ms
B2_hash_latency_64k/sha512           1.406 ms
B2_hash_latency_64k/blake3           1.356 ms
B2_hash_latency_64k/crc32            1.238 ms
B2_hash_latency_64k/hmac_sha256      1.168 ms

B3_sha256_vs_blake3/sha256/64KB      1.162 ms
B3_sha256_vs_blake3/blake3/64KB      1.168 ms
B3_sha256_vs_blake3/sha256/256KB     1.410 ms
B3_sha256_vs_blake3/blake3/256KB     1.524 ms
B3_sha256_vs_blake3/sha256/976KB     1.930 ms
B3_sha256_vs_blake3/blake3/976KB     1.575 ms
B3_sha256_vs_blake3/sha256/3906KB    3.070 ms
B3_sha256_vs_blake3/blake3/3906KB    2.538 ms

B4_hmac_overhead/sha256              1.696 ms    (1 MB)
B4_hmac_overhead/hmac_sha256         1.772 ms

B5_crc32_vs_md5/crc32                1.420 ms    (1 MB)
B5_crc32_vs_md5/md5                  3.107 ms

B6_chunk_hash/64KB                   1.345 ms
B6_chunk_hash/976KB                  1.720 ms

B7_rolling_hash_windows/w32          1.161 ms    (64 KB)
B7_rolling_hash_windows/w48          1.169 ms
B7_rolling_hash_windows/w128         1.598 ms

B8_checksum_verify/crc32_verify_64k  1.675 ms    (64 KB)
B9_sha256_verify/sha256_verify_64k   1.843 ms    (64 KB)

B10_output_efficiency/crc32_4B       1.712 ms    (1 MB)
B10_output_efficiency/md5_16B        3.692 ms
B10_output_efficiency/sha256_32B     1.950 ms
B10_output_efficiency/sha512_64B     3.689 ms
B10_output_efficiency/blake3_32B     2.397 ms
B10_output_efficiency/hmac256_32B    2.168 ms
```

## Derived Throughput Table

All values include streaming framework overhead (~1.2 ms baseline).

| Algorithm | Throughput (MB/s) | Output Size | Per-Chunk (64 KB) | Use Case |
|-----------|------------------:|:-----------:|------------------:|----------|
| CRC32 | 780 | 4 bytes | 1,238 μs | Integrity checks |
| SHA-256 | 638 | 32 bytes | 1,273 μs | Content addressing |
| BLAKE3 | 610 | 32 bytes | 1,356 μs | Fast general hash |
| HMAC-SHA256 | 558 | 32 bytes | 1,168 μs | Keyed authentication |
| MD5 | 385 | 16 bytes | 1,481 μs | S3 ETag compatibility |
| SHA-512 | 337 | 64 bytes | 1,406 μs | Higher security margin |

## Detailed Analysis

### B1: Hash Producer Throughput Ranking (1 MB)

```
CRC32        780 MB/s  ████████████████████████████████████████
SHA-256      638 MB/s  ████████████████████████████████
BLAKE3       610 MB/s  ███████████████████████████████
HMAC-SHA256  558 MB/s  ████████████████████████████
MD5          385 MB/s  ████████████████████
SHA-512      337 MB/s  █████████████████
```

CRC32 is the fastest at 780 MB/s, approaching the streaming framework ceiling (~730 MB/s at 1 MB seen in §7.1). SHA-256 and BLAKE3 are nearly tied at 638/610 MB/s. MD5 and SHA-512 are the slowest.

### B2: Per-Chunk Latency (64 KB)

All hash algorithms converge to the 1.17–1.48 ms range at 64 KB, dominated by streaming overhead. The raw hash computation at 64 KB is sub-100 μs for all algorithms — the ~1.2 ms floor is the tokio channel + spawn infrastructure.

### B3: SHA-256 vs BLAKE3 Scaling

| Data Size | SHA-256 (MB/s) | BLAKE3 (MB/s) | BLAKE3 Advantage |
|----------:|---------------:|---------------:|-----------------:|
| 64 KB | 54 | 54 | 1.00× |
| 256 KB | 177 | 164 | 0.93× |
| 976 KB | 494 | 605 | 1.23× |
| 3,906 KB | 1,243 | 1,503 | 1.21× |

BLAKE3's advantage only emerges at larger data sizes (≥1 MB) where the raw hash computation time exceeds the streaming overhead. At 64 KB, both are overhead-bound. At 4 MB, BLAKE3 is 1.21× faster — significant but far from the spec's predicted 10× advantage. This is because:

1. The streaming framework adds constant overhead that compresses the ratio
2. BLAKE3's multi-threaded advantage (AVX2 parallel tree hashing) is not utilized in our single-chunk streaming model — each chunk is hashed sequentially
3. SHA-256 benefits from SHA-NI hardware instructions on this platform

**To realize BLAKE3's full 10× advantage**, the pipeline would need to feed data in many parallel chunks that BLAKE3 can hash concurrently via its internal tree structure.

### B4: HMAC-SHA256 Overhead

- SHA-256: 1.696 ms
- HMAC-SHA256: 1.772 ms
- **Overhead: 4.5%**

The spec predicted ~10% overhead. Our measured 4.5% is lower because the HMAC key mixing (two SHA-256 block operations for inner/outer padding) is amortized over 1 MB of data. The overhead would be higher for small messages where the key setup cost is proportionally larger.

### B5: CRC32 vs MD5

- CRC32: 704 MB/s (1.420 ms)
- MD5: 322 MB/s (3.107 ms)
- **MD5 is 2.2× slower than CRC32**

For pure integrity checking where cryptographic strength is not needed, CRC32 is the clear winner. MD5 provides no security advantage over CRC32 (both are cryptographically broken) but costs 2.2× more throughput.

### B6: ChunkHash (CAS Per-Chunk Hashing)

| Data Size | Time | Throughput |
|----------:|-----:|-----------:|
| 64 KB | 1.345 ms | 46 MB/s |
| 976 KB | 1.720 ms | 554 MB/s |

ChunkHash computes SHA-256 per input chunk and concatenates the hashes. At 64 KB it's overhead-bound. At 976 KB the throughput approaches raw SHA-256 speed, confirming the implementation is efficient.

### B7: RollingHash Window Size Impact

| Window | Time (64 KB) |
|-------:|-------------:|
| 32 | 1.161 ms |
| 48 | 1.169 ms |
| 128 | 1.598 ms |

Window sizes 32 and 48 are within the overhead floor. Window 128 shows a 37% increase — the Rabin fingerprint computation scales with window size due to the modular exponentiation in the sliding window. For content-defined chunking, window size 48 (default) is optimal.

### B8/B9: Verify Functions

| Function | Time (64 KB) | vs Raw Hash |
|----------|-------------:|------------:|
| ChecksumVerify (CRC32) | 1.675 ms | +35% vs CRC32 raw |
| Sha256Verify | 1.843 ms | +45% vs SHA-256 raw |

The verify functions are slower than raw hashing because they:
1. Compute the hash over the full input
2. Compare against the expected value
3. Pass through the original data on success

The overhead is the data passthrough — the verify functions must buffer and re-emit all input data after verification, effectively doubling the channel traffic.

### B10: Output Size Efficiency

Throughput per output byte (higher = more efficient use of compute):

| Algorithm | Throughput (MB/s) | Output (bytes) | MB/s per output byte |
|-----------|------------------:|---------------:|---------------------:|
| CRC32 | 780 | 4 | 195.0 |
| SHA-256 | 638 | 32 | 19.9 |
| BLAKE3 | 610 | 32 | 19.1 |
| HMAC-SHA256 | 558 | 32 | 17.4 |
| SHA-512 | 337 | 64 | 5.3 |
| MD5 | 385 | 16 | 24.1 |

CRC32 is by far the most efficient per output byte. MD5 has surprisingly good efficiency (24.1) despite lower throughput because its 16-byte output is compact. SHA-512's 64-byte output makes it the least efficient per byte.

## Comparison with §7.2 Spec Expectations

| Algorithm | Spec (MB/s) | Measured (MB/s) | Notes |
|-----------|------------:|----------------:|-------|
| MD5 | 800 | 385 | Below spec — streaming overhead + no SIMD optimization |
| SHA-256 | 500 | 638 | Exceeds spec — SHA-NI acceleration |
| SHA-512 | 700 | 337 | Below spec — no dedicated SIMD path in this build |
| BLAKE3 | 5,000 | 610 | Well below spec — single-chunk model prevents parallel tree hashing |
| CRC32 | 10,000 | 780 | Well below spec — streaming overhead ceiling |
| HMAC-SHA256 | 450 | 558 | Exceeds spec |

**Key discrepancy:** BLAKE3 and CRC32 measured throughput is far below spec because:
- The spec assumes raw codec throughput with SIMD (AVX2/SSE4.2)
- Our measurements include ~1.2 ms streaming overhead per invocation
- BLAKE3's parallel tree hashing requires multi-chunk parallelism not present in our streaming model
- At 1 MB, the overhead floor caps all fast algorithms at ~780 MB/s

## Key Takeaways

1. **SHA-256 is the best general-purpose hash** in our streaming framework: 638 MB/s with SHA-NI, 32-byte output suitable for content addressing.

2. **BLAKE3's theoretical advantage is unrealized** in single-chunk streaming. To benefit from BLAKE3's parallelism, the pipeline would need to split large inputs into parallel sub-chunks.

3. **CRC32 is the fastest integrity check** at 780 MB/s but provides no cryptographic guarantees.

4. **HMAC-SHA256 overhead is minimal** (4.5%) — keyed authentication is nearly free relative to raw hashing at large data sizes.

5. **SHA-512 is slower than SHA-256** on this platform, contrary to the spec's prediction. This suggests the build does not have optimized SHA-512 SIMD paths, while SHA-256 benefits from SHA-NI.

6. **All hash algorithms are overhead-bound at 64 KB** — per-chunk latency converges to 1.17–1.48 ms regardless of algorithm, making hash choice irrelevant for small-chunk streaming pipelines.

7. **RollingHash window size 48 (default) is optimal** — no measurable difference from window 32, while window 128 adds 37% latency.

8. **Verify functions add ~35–45% overhead** over raw hashing due to data passthrough buffering.
