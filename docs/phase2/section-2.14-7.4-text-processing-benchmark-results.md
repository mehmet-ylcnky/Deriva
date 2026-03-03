# §7.4 Text Processing Overhead — Benchmark Results

**Date:** 2026-03-04
**Platform:** Linux x86_64, single-threaded tokio runtime
**Benchmark framework:** Criterion 0.5
**Benchmark file:** `crates/deriva-benchmarks/benches/text_processing_overhead.rs`

## All 8 Text Functions Benchmarked

| # | Function | Description | Module |
|---|----------|-------------|--------|
| #78 | StreamingReplace | Find/replace (literal or regex) | text.rs |
| #79 | StreamingPrefix | Prepend header to stream | text.rs |
| #80 | StreamingSuffix | Append footer to stream | text.rs |
| #81 | StreamingLinePrefix | Prefix every line | text.rs |
| #82 | StreamingGrep | Line-level regex filter | text.rs |
| #83 | StreamingSed | Regex search-and-replace | text.rs |
| #84 | StreamingTruncateLines | Truncate lines to max bytes | text.rs |
| #85 | StreamingCharsetConvert | Charset transcoding | text.rs |

## Benchmark Scenarios

| ID | Scenario | Data Size | Functions Tested |
|----|----------|-----------|------------------|
| B1 | Replace — literal vs regex | 1 MB | Replace |
| B2 | Grep — simple literal vs complex regex | 1 MB | Grep |
| B3 | Sed — simple replace vs capture groups | 1 MB | Sed |
| B4 | LinePrefix throughput | 1 MB | LinePrefix |
| B5 | Prefix + Suffix throughput | 1 MB | Prefix, Suffix |
| B6 | TruncateLines — max 1024 vs max 40 | 1 MB | TruncateLines |
| B7 | CharsetConvert — utf8→utf8 vs latin1→utf8 | 1 MB | CharsetConvert |
| B8 | Replace scaling (64 KB–4 MB) | 64 KB–4 MB | Replace |
| B9 | Grep vs Sed vs Replace head-to-head | 1 MB | Grep, Sed, Replace |
| B10 | Full text suite — all 8 functions | 1 MB | all 8 |

## Raw Criterion Results (median times)

```
B1_replace/literal                     2.509 ms    (1 MB)
B1_replace/regex                       2.495 ms

B2_grep/simple                         1.412 ms    (1 MB)
B2_grep/complex_regex                  4.497 ms

B3_sed/simple                          1.542 ms    (1 MB)
B3_sed/capture_groups                  4.333 ms

B4_line_prefix_1mb                     1.148 ms    (1 MB)

B5_prefix_suffix/prefix                0.086 ms    (1 MB)
B5_prefix_suffix/suffix                0.169 ms

B6_truncate_lines/max_1024             0.666 ms    (1 MB)
B6_truncate_lines/max_40               0.646 ms

B7_charset_convert/utf8_to_utf8        0.681 ms    (1 MB)
B7_charset_convert/latin1_to_utf8      1.365 ms

B8_replace_scaling/64KB                0.314 ms
B8_replace_scaling/256KB               0.761 ms
B8_replace_scaling/1000KB              2.550 ms
B8_replace_scaling/4000KB             12.347 ms

B9_head_to_head/grep                   0.932 ms    (1 MB)
B9_head_to_head/sed                    0.972 ms
B9_head_to_head/replace                2.788 ms

B10_full_text_suite_1mb/prefix          0.090 ms
B10_full_text_suite_1mb/suffix          0.164 ms
B10_full_text_suite_1mb/truncate_lines  0.694 ms
B10_full_text_suite_1mb/grep_simple     0.927 ms
B10_full_text_suite_1mb/sed_simple      0.992 ms
B10_full_text_suite_1mb/line_prefix     1.330 ms
B10_full_text_suite_1mb/charset_latin1  1.603 ms
B10_full_text_suite_1mb/replace_literal 2.242 ms
```

## Derived Throughput Table (1 MB, all 8 functions)

| Function | Throughput (MB/s) | Time (ms) | Category |
|----------|------------------:|----------:|----------|
| Prefix | 11,164 | 0.090 | Stream header (single prepend) |
| Suffix | 6,111 | 0.164 | Stream footer (single append) |
| TruncateLines | 1,440 | 0.694 | Byte-level line scan |
| Grep (simple) | 1,078 | 0.927 | Line-level regex filter |
| Sed (simple) | 1,008 | 0.992 | Regex replace |
| LinePrefix | 752 | 1.330 | Per-byte scan + insert |
| CharsetConvert (latin1→utf8) | 624 | 1.603 | Per-byte char mapping |
| Replace (literal) | 446 | 2.242 | Full-text find/replace |

## Detailed Analysis

### B1: Replace — Literal vs Regex

- **Literal:** 2.509 ms (399 MB/s)
- **Regex:** 2.495 ms (401 MB/s)

Surprisingly, regex Replace is not slower than literal Replace. This is because the regex crate's `replace_all` with a simple pattern (`\d+`) uses DFA-based matching which is competitive with literal search. The Replace function's bottleneck is the `String::from_utf8_lossy` conversion and output allocation, not the search itself.

### B2: Grep — Simple vs Complex Regex

- **Simple literal:** 1.412 ms (708 MB/s)
- **Complex regex:** 4.497 ms (222 MB/s)

Complex regex (`\b\w+@\w+\.\w+\b`) is 3.2× slower than simple literal matching. The spec predicted 2,000 MB/s for simple and 200 MB/s for complex — our measured 708 MB/s and 222 MB/s are lower for simple (streaming overhead) but match well for complex regex.

### B3: Sed — Simple vs Capture Groups

- **Simple:** 1.542 ms (648 MB/s)
- **Capture groups:** 4.333 ms (231 MB/s)

Capture groups (`(\w+)@(\w+)\.(\w+)` → `$1_at_$2`) are 2.8× slower than simple replacement. The overhead comes from capture extraction and backreference substitution. The spec predicted 1,500 MB/s simple and 300 MB/s with captures — our measured values are lower for simple (overhead-bound) but close for captures.

### B4: LinePrefix

- **1 MB:** 1.148 ms (871 MB/s)

LinePrefix scans every byte looking for `\n` and inserts a prefix string. At 871 MB/s it's moderately fast — the per-byte scan with AtomicBool synchronization adds overhead compared to bulk operations.

### B5: Prefix + Suffix

- **Prefix:** 0.086 ms (11,164 MB/s)
- **Suffix:** 0.169 ms (6,111 MB/s)

These are the fastest text functions because they only touch the first/last chunk. Prefix prepends to the first data chunk; Suffix appends after the End signal. Both are essentially zero-cost passthrough with a single buffer operation.

### B6: TruncateLines

- **max_line_bytes=1024:** 0.666 ms (1,502 MB/s)
- **max_line_bytes=40:** 0.646 ms (1,548 MB/s)

Truncation limit has negligible impact on performance. The function scans for `\n` delimiters and copies bytes — the truncation itself is just a `min()` on the slice length. Both variants are fast because the operation is byte-level with no regex or string conversion.

### B7: CharsetConvert

- **utf8→utf8:** 0.681 ms (1,468 MB/s)
- **latin1→utf8:** 1.365 ms (733 MB/s)

UTF-8 passthrough is 2× faster than latin1 conversion. UTF-8→UTF-8 only validates and copies; latin1→UTF-8 must map each byte through `as char` and collect into a String, which involves per-byte allocation. The spec predicted 500 MB/s — our 733 MB/s for latin1 exceeds that estimate.

### B8: Replace Scaling

| Data Size | Time (ms) | Throughput (MB/s) |
|----------:|----------:|------------------:|
| 64 KB | 0.314 | 199 |
| 256 KB | 0.761 | 329 |
| 1,000 KB | 2.550 | 383 |
| 4,000 KB | 12.347 | 316 |

Replace throughput peaks at ~1 MB (383 MB/s) then drops at 4 MB (316 MB/s). The drop at 4 MB is likely due to L3 cache pressure — the full-text `replace_all` must hold the entire input and output in memory simultaneously, and at 4 MB the working set exceeds cache capacity.

### B9: Grep vs Sed vs Replace Head-to-Head

All three using the same simple pattern "fox" on 1 MB:

| Function | Time (ms) | Throughput (MB/s) |
|----------|----------:|------------------:|
| Grep | 0.932 | 1,073 |
| Sed | 0.972 | 1,029 |
| Replace | 2.788 | 359 |

Grep is fastest because it only filters lines (no output rewriting). Sed is close because simple replacement is cheap. Replace is 3× slower because it uses `String::from_utf8_lossy` + `replace_all` on the full chunk rather than line-by-line processing.

### B10: Full Text Suite Ranking

```
Prefix           11,164 MB/s  ████████████████████████████████████████
Suffix            6,111 MB/s  ██████████████████████
TruncateLines     1,440 MB/s  █████
Grep (simple)     1,078 MB/s  ████
Sed (simple)      1,008 MB/s  ████
LinePrefix          752 MB/s  ███
CharsetConvert      624 MB/s  ██
Replace (literal)   446 MB/s  ██
```

## Comparison with §7.4 Spec Expectations

| Function | Spec (MB/s) | Measured (MB/s) | Notes |
|----------|------------:|----------------:|-------|
| Grep (simple) | 2,000 | 1,078 | Streaming overhead limits throughput |
| Grep (complex regex) | 200 | 222 | Matches spec well |
| Sed (simple) | 1,500 | 1,008 | Overhead-bound |
| Sed (capture groups) | 300 | 231 | Close to spec |
| Replace (byte pattern) | 3,000 | 446 | Full-text approach slower than memmem |
| CharsetConvert | 500 | 624 | Exceeds spec (latin1→utf8) |

## Key Takeaways

1. **Prefix/Suffix are near-zero-cost** at 11,164/6,111 MB/s — they only touch the first/last chunk and are effectively passthrough operations.

2. **TruncateLines is the fastest per-chunk text transform** at 1,440 MB/s — byte-level scanning without regex or string conversion.

3. **Grep and Sed are competitive** at ~1,000 MB/s for simple patterns — both use line-by-line processing which is efficient for streaming.

4. **Complex regex costs 3× more** than simple patterns for both Grep (222 MB/s) and Sed (231 MB/s) — backtracking and capture extraction dominate.

5. **Replace is the slowest per-chunk function** at 446 MB/s because it converts the entire chunk to a String and runs `replace_all`, rather than processing line-by-line.

6. **CharsetConvert exceeds spec** at 624 MB/s for latin1→utf8 (spec predicted 500 MB/s). UTF-8 passthrough is 2× faster at 1,468 MB/s.

7. **Replace scaling peaks at ~1 MB** then drops at 4 MB due to cache pressure — the full-text approach doesn't scale as well as line-by-line functions.

8. **Streaming overhead is the dominant factor** for fast operations — Prefix at 0.086 ms shows the framework floor is well below 0.1 ms for trivial operations.
