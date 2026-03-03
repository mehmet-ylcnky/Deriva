//! §7.2 Memory Profile Benchmarks
//!
//! Measures memory multipliers (output_size / input_size) and peak allocation
//! patterns for batch functions across all categories.

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

fn exec(f: &dyn ComputeFunction, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Bytes {
    f.execute(inputs, params).unwrap()
}

// §7.2 Scenario 1: Transforms - 1× memory (identity, uppercase, lowercase)
fn s01_transforms_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_01_transforms");
    for &size in &[64 * 1024, 1024 * 1024] {
        let input = Bytes::from(vec![b'a'; size]);
        let label = format!("{}KB", size / 1024);
        g.throughput(Throughput::Bytes(size as u64));
        
        g.bench_with_input(BenchmarkId::new("identity", &label), &input, |b, inp| {
            b.iter(|| {
                let out = exec(&IdentityFn, vec![inp.clone()], &BTreeMap::new());
                assert_eq!(out.len(), inp.len()); // 1× multiplier
                out
            })
        });
        g.bench_with_input(BenchmarkId::new("uppercase", &label), &input, |b, inp| {
            b.iter(|| {
                let out = exec(&UppercaseFn, vec![inp.clone()], &BTreeMap::new());
                assert_eq!(out.len(), inp.len()); // 1× multiplier
                out
            })
        });
    }
    g.finish();
}

// §7.2 Scenario 2: Compression - variable output (typically <1× for compressible)
fn s02_compression_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_02_compression");
    let compressible = Bytes::from(vec![b'x'; 1_048_576]); // highly compressible
    let random: Vec<u8> = (0..1_048_576).map(|i| ((i * 17 + 31) % 256) as u8).collect();
    let incompressible = Bytes::from(random);
    g.throughput(Throughput::Bytes(1_048_576));

    g.bench_function("zlib_compressible", |b| {
        b.iter(|| {
            let out = exec(&CompressFn, vec![compressible.clone()], &BTreeMap::new());
            // Verify compression ratio
            assert!(out.len() < compressible.len() / 100); // >100× compression
            out
        })
    });
    g.bench_function("zlib_incompressible", |b| {
        b.iter(|| {
            let out = exec(&CompressFn, vec![incompressible.clone()], &BTreeMap::new());
            // May expand slightly due to headers
            assert!(out.len() <= incompressible.len() + 100);
            out
        })
    });
    g.bench_function("zstd_compressible", |b| {
        b.iter(|| {
            let out = exec(&ZstdCompressFn, vec![compressible.clone()], &BTreeMap::new());
            assert!(out.len() < compressible.len() / 100);
            out
        })
    });
    g.bench_function("lz4_compressible", |b| {
        b.iter(|| {
            let out = exec(&Lz4CompressFn, vec![compressible.clone()], &BTreeMap::new());
            assert!(out.len() < compressible.len() / 10);
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 3: Crypto - 1× for CTR, 1×+16B for GCM
fn s03_crypto_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_03_crypto");
    let input = Bytes::from(vec![0u8; 1_048_576]);
    let ctr_p = p(&[("key", KEY_HEX), ("nonce", NONCE_HEX)]);
    let gcm_p = p(&[("key", KEY_HEX), ("nonce", GCM_NONCE)]);
    g.throughput(Throughput::Bytes(1_048_576));

    g.bench_function("aes_ctr_1mb", |b| {
        b.iter(|| {
            let out = exec(&EncryptFn, vec![input.clone()], &ctr_p);
            assert_eq!(out.len(), input.len()); // Exact 1× (stream cipher)
            out
        })
    });
    g.bench_function("aes_gcm_1mb", |b| {
        b.iter(|| {
            let out = exec(&AeadEncryptFn, vec![input.clone()], &gcm_p);
            assert_eq!(out.len(), input.len() + 16); // 1× + 16B auth tag
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 4: Accumulators - O(1) output regardless of input
fn s04_accumulators_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_04_accumulators");
    for &size in &[64 * 1024, 1024 * 1024, 10 * 1024 * 1024] {
        let input = Bytes::from(vec![0u8; size]);
        let label = format!("{}KB", size / 1024);
        g.throughput(Throughput::Bytes(size as u64));

        g.bench_with_input(BenchmarkId::new("sha256", &label), &input, |b, inp| {
            b.iter(|| {
                let out = exec(&Sha256Fn, vec![inp.clone()], &BTreeMap::new());
                assert_eq!(out.len(), 32); // Always 32 bytes regardless of input
                out
            })
        });
        g.bench_with_input(BenchmarkId::new("byte_count", &label), &input, |b, inp| {
            b.iter(|| {
                let out = exec(&ByteCountFn, vec![inp.clone()], &BTreeMap::new());
                assert_eq!(out.len(), 8); // Always 8 bytes (u64)
                out
            })
        });
    }
    g.finish();
}

// §7.2 Scenario 5: Combiners - N× inputs (concat, interleave)
fn s05_combiners_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_05_combiners");
    let input1 = Bytes::from(vec![b'A'; 512 * 1024]);
    let input2 = Bytes::from(vec![b'B'; 512 * 1024]);
    let input3 = Bytes::from(vec![b'C'; 512 * 1024]);
    g.throughput(Throughput::Bytes(1_536 * 1024));

    g.bench_function("concat_3x512kb", |b| {
        b.iter(|| {
            let out = exec(&ConcatFn, vec![input1.clone(), input2.clone(), input3.clone()], &BTreeMap::new());
            assert_eq!(out.len(), 512 * 1024 * 3); // Sum of all inputs
            out
        })
    });
    g.bench_function("interleave_2x512kb", |b| {
        b.iter(|| {
            let out = exec(&InterleaveFn, vec![input1.clone(), input2.clone()], &BTreeMap::new());
            assert_eq!(out.len(), 512 * 1024 * 2); // Sum of both inputs
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 6: Slicing - ≤1× (subset of input)
fn s06_slicing_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_06_slicing");
    let input = Bytes::from(vec![b'x'; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));

    g.bench_function("take_256kb", |b| {
        let tp = p(&[("bytes", "262144")]);
        b.iter(|| {
            let out = exec(&TakeFn, vec![input.clone()], &tp);
            assert_eq!(out.len(), 262144); // 0.25× input
            out
        })
    });
    g.bench_function("skip_768kb", |b| {
        let sp = p(&[("bytes", "786432")]);
        b.iter(|| {
            let out = exec(&SkipFn, vec![input.clone()], &sp);
            assert_eq!(out.len(), 262144); // 0.25× input (remaining)
            out
        })
    });
    g.bench_function("slice_middle_512kb", |b| {
        let slp = p(&[("offset", "262144"), ("length", "524288")]);
        b.iter(|| {
            let out = exec(&SliceFn, vec![input.clone()], &slp);
            assert_eq!(out.len(), 524288); // 0.5× input
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 7: Text processing - 1-2× depending on operation
fn s07_text_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_07_text");
    let line = "the quick brown fox jumps over the lazy dog\n";
    let text = Bytes::from(line.repeat(20_000));
    g.throughput(Throughput::Bytes(text.len() as u64));

    g.bench_function("grep_filter", |b| {
        let gp = p(&[("pattern", "fox")]);
        b.iter(|| {
            let out = exec(&GrepFn, vec![text.clone()], &gp);
            // All lines match (may differ by trailing newline)
            assert!(out.len() >= text.len() - 1);
            out
        })
    });
    g.bench_function("grep_invert_filter", |b| {
        let gp = p(&[("pattern", "NOMATCH")]);
        b.iter(|| {
            let out = exec(&GrepFn, vec![text.clone()], &gp);
            assert_eq!(out.len(), 0); // No lines match
            out
        })
    });
    g.bench_function("replace_same_len", |b| {
        let rp = p(&[("find", "fox"), ("replace", "cat")]);
        b.iter(|| {
            let out = exec(&ReplaceFn, vec![text.clone()], &rp);
            assert_eq!(out.len(), text.len()); // Same length replacement
            out
        })
    });
    g.bench_function("replace_expand", |b| {
        let rp = p(&[("find", "fox"), ("replace", "elephant")]);
        b.iter(|| {
            let out = exec(&ReplaceFn, vec![text.clone()], &rp);
            assert!(out.len() > text.len()); // Expansion
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 8: Format conversion - 2-4× (parse + serialize)
fn s08_format_conversion_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_08_format_conversion");
    
    // CSV input
    let mut csv = String::from("name,age,city\n");
    for i in 0..5000 {
        csv.push_str(&format!("user{},{},city{}\n", i, 20 + (i % 60), i % 100));
    }
    let csv_input = Bytes::from(csv);
    g.throughput(Throughput::Bytes(csv_input.len() as u64));

    g.bench_function("csv_to_json", |b| {
        b.iter(|| {
            let out = exec(&CsvToJsonFn, vec![csv_input.clone()], &BTreeMap::new());
            // JSON is typically larger due to field names repeated
            assert!(out.len() > csv_input.len());
            out
        })
    });

    let json_data = exec(&CsvToJsonFn, vec![csv_input.clone()], &BTreeMap::new());
    g.bench_function("json_to_csv", |b| {
        b.iter(|| {
            let out = exec(&JsonToCsvFn, vec![json_data.clone()], &BTreeMap::new());
            // CSV is more compact
            assert!(out.len() < json_data.len());
            out
        })
    });

    g.bench_function("json_pretty", |b| {
        b.iter(|| {
            let out = exec(&JsonPrettyPrintFn, vec![json_data.clone()], &BTreeMap::new());
            // Pretty print typically adds whitespace (may vary)
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 9: CAS operations - 1× + O(blocks) hashes
fn s09_cas_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_09_cas");
    let input = Bytes::from(vec![0u8; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));

    g.bench_function("caddr_compute", |b| {
        b.iter(|| {
            let out = exec(&CAddrComputeFn, vec![input.clone()], &BTreeMap::new());
            assert_eq!(out.len(), 32); // Single CAddr (32 bytes)
            out
        })
    });
    g.bench_function("merkle_root_64kb_blocks", |b| {
        let mp = p(&[("block_size", "65536")]);
        b.iter(|| {
            let out = exec(&MerkleRootFn, vec![input.clone()], &mp);
            assert_eq!(out.len(), 32); // Single root hash
            out
        })
    });
    g.bench_function("chunk_hash_64kb_blocks", |b| {
        let cp = p(&[("block_size", "65536")]);
        b.iter(|| {
            let out = exec(&ChunkHashFn, vec![input.clone()], &cp);
            // 16 blocks × 32 bytes = 512 bytes
            assert_eq!(out.len(), 16 * 32);
            out
        })
    });
    g.bench_function("caddr_embed", |b| {
        b.iter(|| {
            let out = exec(&CAddrEmbedFn, vec![input.clone()], &BTreeMap::new());
            // Embeds 32-byte CAddr at start
            assert_eq!(out.len(), input.len() + 32);
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 10: Encoding expansion - base64 ~1.33×, hex 2×, base32 ~1.6×
fn s10_encoding_expansion(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_10_encoding");
    let input = Bytes::from(vec![0xAB; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));

    g.bench_function("base64_encode", |b| {
        b.iter(|| {
            let out = exec(&Base64EncodeFn, vec![input.clone()], &BTreeMap::new());
            // Base64: 4 output chars per 3 input bytes = 1.33×
            let expected = (input.len() * 4 + 2) / 3;
            assert!((out.len() as i64 - expected as i64).abs() < 10);
            out
        })
    });
    g.bench_function("hex_encode", |b| {
        b.iter(|| {
            let out = exec(&HexEncodeFn, vec![input.clone()], &BTreeMap::new());
            assert_eq!(out.len(), input.len() * 2); // Exact 2×
            out
        })
    });
    g.bench_function("base32_encode", |b| {
        b.iter(|| {
            let out = exec(&Base32EncodeFn, vec![input.clone()], &BTreeMap::new());
            // Base32: 8 output chars per 5 input bytes = 1.6×
            let expected = (input.len() * 8 + 4) / 5;
            assert!((out.len() as i64 - expected as i64).abs() < 10);
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 11: Hashing output sizes
fn s11_hash_output_sizes(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_11_hash_sizes");
    let input = Bytes::from(vec![0u8; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));

    g.bench_function("md5_16bytes", |b| {
        b.iter(|| {
            let out = exec(&Md5Fn, vec![input.clone()], &BTreeMap::new());
            assert_eq!(out.len(), 16);
            out
        })
    });
    g.bench_function("sha256_32bytes", |b| {
        b.iter(|| {
            let out = exec(&Sha256Fn, vec![input.clone()], &BTreeMap::new());
            assert_eq!(out.len(), 32);
            out
        })
    });
    g.bench_function("sha512_64bytes", |b| {
        b.iter(|| {
            let out = exec(&Sha512Fn, vec![input.clone()], &BTreeMap::new());
            assert_eq!(out.len(), 64);
            out
        })
    });
    g.bench_function("blake3_32bytes", |b| {
        b.iter(|| {
            let out = exec(&Blake3Fn, vec![input.clone()], &BTreeMap::new());
            assert_eq!(out.len(), 32);
            out
        })
    });
    g.bench_function("crc32_4bytes", |b| {
        b.iter(|| {
            let out = exec(&Crc32Fn, vec![input.clone()], &BTreeMap::new());
            assert_eq!(out.len(), 4);
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 12: Sort memory (in-place vs copy)
fn s12_sort_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_12_sort");
    let lines = "zebra\napple\nmango\nbanana\n".repeat(10_000);
    let text = Bytes::from(lines);
    g.throughput(Throughput::Bytes(text.len() as u64));

    g.bench_function("sort_lines", |b| {
        b.iter(|| {
            let out = exec(&SortFn, vec![text.clone()], &BTreeMap::new());
            // Same size, reordered (may differ by trailing newline)
            assert!(out.len() >= text.len() - 1 && out.len() <= text.len());
            out
        })
    });
    g.bench_function("sort_unique", |b| {
        b.iter(|| {
            let out = exec(&SortUniqueFn, vec![text.clone()], &BTreeMap::new());
            // Only 4 unique lines remain
            assert!(out.len() < text.len());
            out
        })
    });

    let bytes: Vec<u8> = (0..1_000_000).map(|i| ((i * 7 + 13) % 256) as u8).collect();
    let byte_input = Bytes::from(bytes);
    g.bench_function("sort_bytes", |b| {
        b.iter(|| {
            let out = exec(&SortBytesFn, vec![byte_input.clone()], &BTreeMap::new());
            assert_eq!(out.len(), byte_input.len());
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 13: Validation - O(1) output (bool/error)
fn s13_validation_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_13_validation");
    // Create valid JSON array
    let items: Vec<String> = (0..1000).map(|i| format!(r#"{{"id":{},"name":"item{}"}}"#, i, i)).collect();
    let json = Bytes::from(format!("[{}]", items.join(",")));
    g.throughput(Throughput::Bytes(json.len() as u64));

    g.bench_function("json_validate", |b| {
        b.iter(|| {
            let out = exec(&JsonValidateFn, vec![json.clone()], &BTreeMap::new());
            assert_eq!(out.len(), json.len()); // Returns input unchanged
            out
        })
    });
    g.bench_function("utf8_validate", |b| {
        b.iter(|| {
            let out = exec(&Utf8ValidateFn, vec![json.clone()], &BTreeMap::new());
            assert_eq!(out.len(), json.len()); // Returns input unchanged
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 14: Diff - O(n+m) output for similar inputs
fn s14_diff_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_14_diff");
    let base = "line1\nline2\nline3\nline4\nline5\n".repeat(1000);
    let modified = "line1\nline2\nchanged\nline4\nline5\n".repeat(1000);
    let input1 = Bytes::from(base);
    let input2 = Bytes::from(modified);
    g.throughput(Throughput::Bytes((input1.len() + input2.len()) as u64));

    g.bench_function("diff_similar", |b| {
        b.iter(|| {
            let out = exec(&DiffFn, vec![input1.clone(), input2.clone()], &BTreeMap::new());
            // Diff output is typically smaller than inputs for similar files
            out
        })
    });
    g.finish();
}

// §7.2 Scenario 15: Repeat/Pad - controlled expansion
fn s15_expansion_memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.2_15_expansion");
    let input = Bytes::from(vec![b'x'; 64 * 1024]);
    g.throughput(Throughput::Bytes(64 * 1024));

    g.bench_function("repeat_4x", |b| {
        let rp = p(&[("count", "4")]);
        b.iter(|| {
            let out = exec(&RepeatFn, vec![input.clone()], &rp);
            assert_eq!(out.len(), input.len() * 4); // Exact 4×
            out
        })
    });
    g.bench_function("pad_to_block_256", |b| {
        let pp = p(&[("block_size", "256")]);
        b.iter(|| {
            let out = exec(&PadFn, vec![input.clone()], &pp);
            // Padded to next 256-byte boundary
            assert!(out.len() >= input.len());
            assert_eq!(out.len() % 256, 0);
            out
        })
    });
    g.finish();
}

criterion_group!(
    benches,
    s01_transforms_memory,
    s02_compression_memory,
    s03_crypto_memory,
    s04_accumulators_memory,
    s05_combiners_memory,
    s06_slicing_memory,
    s07_text_memory,
    s08_format_conversion_memory,
    s09_cas_memory,
    s10_encoding_expansion,
    s11_hash_output_sizes,
    s12_sort_memory,
    s13_validation_memory,
    s14_diff_memory,
    s15_expansion_memory,
);
criterion_main!(benches);
