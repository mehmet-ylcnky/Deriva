//! §7.3 Computational Complexity Benchmarks
//!
//! Measures scaling behavior across input sizes for each complexity class.
//! Each scenario runs at 4KB, 64KB, 256KB, 1MB to reveal O(1)/O(n)/O(n log n)/O(n×m) curves.

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use bytes::Bytes;
use deriva_compute::builtins::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(kv: &[(&str, &str)]) -> BTreeMap<String, Value> {
    kv.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

const KEY_HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const NONCE_HEX: &str = "00112233445566778899aabbccddeeff";
const GCM_NONCE: &str = "000102030405060708090a0b";

const SIZES: &[(usize, &str)] = &[
    (4 * 1024, "4KB"),
    (64 * 1024, "64KB"),
    (256 * 1024, "256KB"),
    (1024 * 1024, "1MB"),
];

fn make_text(n: usize) -> Bytes {
    let line = "the quick brown fox jumps over the lazy dog\n";
    Bytes::from(line.repeat(n / line.len() + 1)[..n].to_string())
}

fn make_sorted_text(n: usize) -> Bytes {
    let mut lines: Vec<String> = (0..n / 20).map(|i| format!("line-{:010}\n", i)).collect();
    lines.sort();
    let s = lines.join("");
    Bytes::from(s[..n.min(s.len())].to_string())
}

fn make_json(n: usize) -> Bytes {
    // Repeat small JSON objects to reach target size
    let obj = r#"{"id":1,"name":"test","value":42},"#;
    let count = n / obj.len() + 1;
    let inner: String = std::iter::repeat(obj).take(count).collect();
    let s = format!("[{}]", &inner[..inner.len() - 1]); // remove trailing comma
    Bytes::from(s[..n.min(s.len())].to_string())
}

// §7.3 Scenario 1: O(1) — Identity (zero-copy, constant time)
fn s01_o1_identity(c: &mut Criterion) {
    let f = IdentityFn;
    let mut g = c.benchmark_group("7.3_01_O1_identity");
    for &(sz, label) in SIZES {
        let input = Bytes::from(vec![0x41u8; sz]);
        g.throughput(Throughput::Bytes(sz as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &sz, |b, _| {
            b.iter(|| f.execute(vec![input.clone()], &p(&[])).unwrap());
        });
    }
    g.finish();
}

// §7.3 Scenario 2: O(n) — Uppercase (single-pass byte transform)
fn s02_on_uppercase(c: &mut Criterion) {
    let f = UppercaseFn;
    let mut g = c.benchmark_group("7.3_02_On_uppercase");
    for &(sz, label) in SIZES {
        let input = make_text(sz);
        g.throughput(Throughput::Bytes(sz as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &sz, |b, _| {
            b.iter(|| f.execute(vec![input.clone()], &p(&[])).unwrap());
        });
    }
    g.finish();
}

// §7.3 Scenario 3: O(n) — SHA-256 (single-pass accumulator)
fn s03_on_sha256(c: &mut Criterion) {
    let f = Sha256Fn;
    let mut g = c.benchmark_group("7.3_03_On_sha256");
    for &(sz, label) in SIZES {
        let input = Bytes::from(vec![0x42u8; sz]);
        g.throughput(Throughput::Bytes(sz as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &sz, |b, _| {
            b.iter(|| f.execute(vec![input.clone()], &p(&[])).unwrap());
        });
    }
    g.finish();
}

// §7.3 Scenario 4: O(n) — Zlib compress (O(n) with high constant)
fn s04_on_zlib(c: &mut Criterion) {
    let f = CompressFn;
    let mut g = c.benchmark_group("7.3_04_On_zlib_compress");
    for &(sz, label) in SIZES {
        let input = make_text(sz);
        g.throughput(Throughput::Bytes(sz as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &sz, |b, _| {
            b.iter(|| f.execute(vec![input.clone()], &p(&[])).unwrap());
        });
    }
    g.finish();
}

// §7.3 Scenario 5: O(n) high constant — AES-CTR encrypt
fn s05_on_high_encrypt(c: &mut Criterion) {
    let f = EncryptFn;
    let params = p(&[("key", KEY_HEX), ("nonce", NONCE_HEX)]);
    let mut g = c.benchmark_group("7.3_05_On_high_encrypt");
    for &(sz, label) in SIZES {
        let input = Bytes::from(vec![0x43u8; sz]);
        g.throughput(Throughput::Bytes(sz as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &sz, |b, _| {
            b.iter(|| f.execute(vec![input.clone()], &params).unwrap());
        });
    }
    g.finish();
}

// §7.3 Scenario 6: O(n) high constant — Brotli compress
fn s06_on_high_brotli(c: &mut Criterion) {
    let f = BrotliCompressFn;
    let mut g = c.benchmark_group("7.3_06_On_high_brotli");
    for &(sz, label) in SIZES {
        let input = make_text(sz);
        g.throughput(Throughput::Bytes(sz as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &sz, |b, _| {
            b.iter(|| f.execute(vec![input.clone()], &p(&[])).unwrap());
        });
    }
    g.finish();
}

// §7.3 Scenario 7: O(n log n) — Sort lines
fn s07_onlogn_sort(c: &mut Criterion) {
    let f = SortFn;
    let mut g = c.benchmark_group("7.3_07_Onlogn_sort");
    for &(sz, label) in SIZES {
        let input = make_text(sz); // unsorted text
        g.throughput(Throughput::Bytes(sz as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &sz, |b, _| {
            b.iter(|| f.execute(vec![input.clone()], &p(&[])).unwrap());
        });
    }
    g.finish();
}

// §7.3 Scenario 8: O(n log n) — Sort unique
fn s08_onlogn_sort_unique(c: &mut Criterion) {
    let f = SortUniqueFn;
    let mut g = c.benchmark_group("7.3_08_Onlogn_sort_unique");
    for &(sz, label) in SIZES {
        // Generate text with ~50% duplicate lines
        let base = "line-alpha\nline-beta\nline-gamma\nline-delta\n";
        let input = Bytes::from(base.repeat(sz / base.len() + 1)[..sz].to_string());
        g.throughput(Throughput::Bytes(sz as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &sz, |b, _| {
            b.iter(|| f.execute(vec![input.clone()], &p(&[])).unwrap());
        });
    }
    g.finish();
}

// §7.3 Scenario 9: O(n×m) — Replace (many matches)
fn s09_onm_replace(c: &mut Criterion) {
    let f = ReplaceFn;
    let params = p(&[("pattern", "the"), ("replacement", "THE")]);
    let mut g = c.benchmark_group("7.3_09_Onm_replace");
    for &(sz, label) in SIZES {
        // "the" appears ~2× per line in our text
        let input = make_text(sz);
        g.throughput(Throughput::Bytes(sz as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &sz, |b, _| {
            b.iter(|| f.execute(vec![input.clone()], &params).unwrap());
        });
    }
    g.finish();
}

// §7.3 Scenario 10: O(n×d) — Diff (similar inputs, small edit distance)
fn s10_ond_diff(c: &mut Criterion) {
    let f = DiffFn;
    let mut g = c.benchmark_group("7.3_10_Ond_diff");
    for &(sz, label) in SIZES {
        let old = make_text(sz);
        // Create "new" with ~5% edits (change every 20th byte)
        let mut new_bytes = old.to_vec();
        for i in (0..new_bytes.len()).step_by(20) {
            new_bytes[i] = b'Z';
        }
        let new_input = Bytes::from(new_bytes);
        g.throughput(Throughput::Bytes(sz as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &sz, |b, _| {
            b.iter(|| f.execute(vec![old.clone(), new_input.clone()], &p(&[])).unwrap());
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    s01_o1_identity,
    s02_on_uppercase,
    s03_on_sha256,
    s04_on_zlib,
    s05_on_high_encrypt,
    s06_on_high_brotli,
    s07_onlogn_sort,
    s08_onlogn_sort_unique,
    s09_onm_replace,
    s10_ond_diff,
);
criterion_main!(benches);
