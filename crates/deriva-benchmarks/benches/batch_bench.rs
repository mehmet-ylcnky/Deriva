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

fn make_input(size: usize) -> Bytes {
    Bytes::from(vec![b'A'; size])
}

fn make_text_input(lines: usize) -> Bytes {
    let line = "the quick brown fox jumps over the lazy dog\n";
    Bytes::from(line.repeat(lines))
}

fn make_csv_input(rows: usize) -> Bytes {
    let mut s = String::from("name,age,city\n");
    for i in 0..rows {
        s.push_str(&format!("user{},{},city{}\n", i, 20 + (i % 60), i % 100));
    }
    Bytes::from(s)
}

// ── Spec §5.5 benchmarks ──

// 1. Compression throughput at multiple sizes
fn bench_compression(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression");
    for size in [1_024, 1_048_576] {
        let input = make_input(size);
        let label = if size < 1_048_576 { "1KB" } else { "1MB" };
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("zlib", label), &input, |b, inp| {
            b.iter(|| CompressFn.execute(vec![inp.clone()], &BTreeMap::new()).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("zstd", label), &input, |b, inp| {
            b.iter(|| ZstdCompressFn.execute(vec![inp.clone()], &BTreeMap::new()).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("lz4", label), &input, |b, inp| {
            b.iter(|| Lz4CompressFn.execute(vec![inp.clone()], &BTreeMap::new()).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("snappy", label), &input, |b, inp| {
            b.iter(|| SnappyCompressFn.execute(vec![inp.clone()], &BTreeMap::new()).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("brotli", label), &input, |b, inp| {
            b.iter(|| BrotliCompressFn.execute(vec![inp.clone()], &BTreeMap::new()).unwrap())
        });
    }
    group.finish();
}

// 2. Crypto: AES-CTR vs AES-GCM encrypt+decrypt roundtrip
fn bench_crypto_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_roundtrip");
    for size in [1_024, 1_048_576] {
        let input = make_input(size);
        let label = if size < 1_048_576 { "1KB" } else { "1MB" };
        let ctr_p = p(&[("key", KEY_HEX), ("nonce", NONCE_HEX)]);
        let gcm_p = p(&[("key", KEY_HEX), ("nonce", GCM_NONCE)]);
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("aes_ctr", label), &input, |b, inp| {
            b.iter(|| {
                let enc = EncryptFn.execute(vec![inp.clone()], &ctr_p).unwrap();
                DecryptFn.execute(vec![enc], &ctr_p).unwrap()
            })
        });
        group.bench_with_input(BenchmarkId::new("aes_gcm", label), &input, |b, inp| {
            b.iter(|| {
                let enc = AeadEncryptFn.execute(vec![inp.clone()], &gcm_p).unwrap();
                AeadDecryptFn.execute(vec![enc], &gcm_p).unwrap()
            })
        });
    }
    group.finish();
}

// 3. Sort: batch sort at 1MB
fn bench_sort(c: &mut Criterion) {
    let mut group = c.benchmark_group("sort");
    let data: Vec<u8> = (0..1_000_000).map(|i| (i * 7 + 13) as u8).collect();
    let input = Bytes::from(data);
    group.throughput(Throughput::Bytes(1_000_000));

    group.bench_function("sort_lines_1mb", |b| {
        let text = make_text_input(20_000);
        b.iter(|| SortFn.execute(vec![text.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("sort_bytes_1mb", |b| {
        b.iter(|| SortBytesFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.finish();
}

// 4. Format conversion: CSV↔JSON
fn bench_format_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("format_conversion");
    let csv_10k = make_csv_input(10_000);
    group.throughput(Throughput::Bytes(csv_10k.len() as u64));

    group.bench_function("csv_to_json_10k", |b| {
        b.iter(|| CsvToJsonFn.execute(vec![csv_10k.clone()], &BTreeMap::new()).unwrap())
    });

    let json_data = CsvToJsonFn.execute(vec![csv_10k.clone()], &BTreeMap::new()).unwrap();
    group.bench_function("json_to_csv_10k", |b| {
        b.iter(|| JsonToCsvFn.execute(vec![json_data.clone()], &BTreeMap::new()).unwrap())
    });

    group.bench_function("json_pretty_10k", |b| {
        b.iter(|| JsonPrettyPrintFn.execute(vec![json_data.clone()], &BTreeMap::new()).unwrap())
    });

    group.bench_function("json_minify_10k", |b| {
        let pretty = JsonPrettyPrintFn.execute(vec![json_data.clone()], &BTreeMap::new()).unwrap();
        b.iter(|| JsonMinifyFn.execute(vec![pretty.clone()], &BTreeMap::new()).unwrap())
    });
    group.finish();
}

// ── Additional scenarios ──

// 5. Hashing throughput comparison
fn bench_hashing(c: &mut Criterion) {
    let mut group = c.benchmark_group("hashing");
    let input = make_input(1_048_576);
    group.throughput(Throughput::Bytes(1_048_576));

    group.bench_function("sha256_1mb", |b| {
        b.iter(|| Sha256Fn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("sha512_1mb", |b| {
        b.iter(|| Sha512Fn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("blake3_1mb", |b| {
        b.iter(|| Blake3Fn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("md5_1mb", |b| {
        b.iter(|| Md5Fn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("crc32_1mb", |b| {
        b.iter(|| Crc32Fn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.finish();
}

// 6. Encoding throughput
fn bench_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("encoding");
    let input = make_input(1_048_576);
    group.throughput(Throughput::Bytes(1_048_576));

    group.bench_function("base64_encode_1mb", |b| {
        b.iter(|| Base64EncodeFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("base64_decode_1mb", |b| {
        let enc = Base64EncodeFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
        b.iter(|| Base64DecodeFn.execute(vec![enc.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("hex_encode_1mb", |b| {
        b.iter(|| HexEncodeFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("base32_encode_1mb", |b| {
        b.iter(|| Base32EncodeFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.finish();
}

// 7. Text processing throughput
fn bench_text_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("text_processing");
    let text = make_text_input(20_000);
    group.throughput(Throughput::Bytes(text.len() as u64));

    group.bench_function("grep_20k_lines", |b| {
        let gp = p(&[("pattern", "fox")]);
        b.iter(|| GrepFn.execute(vec![text.clone()], &gp).unwrap())
    });
    group.bench_function("regex_replace_20k_lines", |b| {
        let rp = p(&[("pattern", "\\bfox\\b"), ("replacement", "cat")]);
        b.iter(|| RegexReplaceFn.execute(vec![text.clone()], &rp).unwrap())
    });
    group.bench_function("line_count_20k", |b| {
        b.iter(|| LineCountFn.execute(vec![text.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("unique_20k_lines", |b| {
        b.iter(|| UniqueFn.execute(vec![text.clone()], &BTreeMap::new()).unwrap())
    });
    group.finish();
}

// 8. CAS operations
fn bench_cas(c: &mut Criterion) {
    let mut group = c.benchmark_group("cas");
    let input = make_input(1_048_576);
    group.throughput(Throughput::Bytes(1_048_576));

    group.bench_function("caddr_compute_1mb", |b| {
        b.iter(|| CAddrComputeFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("merkle_root_1mb_64k_blocks", |b| {
        let mp = p(&[("block_size", "65536")]);
        b.iter(|| MerkleRootFn.execute(vec![input.clone()], &mp).unwrap())
    });
    group.bench_function("chunk_hash_1mb_64k_blocks", |b| {
        let cp = p(&[("block_size", "65536")]);
        b.iter(|| ChunkHashFn.execute(vec![input.clone()], &cp).unwrap())
    });
    group.bench_function("dedup_analyze_1mb", |b| {
        b.iter(|| DedupAnalyzeFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("caddr_embed_1mb", |b| {
        b.iter(|| CAddrEmbedFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
    group.finish();
}

// 9. Compression roundtrip (compress + decompress)
fn bench_compression_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_roundtrip");
    let input = make_input(1_048_576);
    group.throughput(Throughput::Bytes(1_048_576));

    group.bench_function("zlib_roundtrip_1mb", |b| {
        b.iter(|| {
            let c = CompressFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
            DecompressFn.execute(vec![c], &BTreeMap::new()).unwrap()
        })
    });
    group.bench_function("zstd_roundtrip_1mb", |b| {
        b.iter(|| {
            let c = ZstdCompressFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
            ZstdDecompressFn.execute(vec![c], &BTreeMap::new()).unwrap()
        })
    });
    group.bench_function("lz4_roundtrip_1mb", |b| {
        b.iter(|| {
            let c = Lz4CompressFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
            Lz4DecompressFn.execute(vec![c], &BTreeMap::new()).unwrap()
        })
    });
    group.finish();
}

// 10. Pipeline composition overhead
fn bench_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline");
    let input = make_input(65_536);
    let kp = p(&[("key", KEY_HEX), ("nonce", NONCE_HEX)]);

    group.bench_function("compress_encrypt_b64_64kb", |b| {
        b.iter(|| {
            let c = CompressFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
            let e = EncryptFn.execute(vec![c], &kp).unwrap();
            Base64EncodeFn.execute(vec![e], &BTreeMap::new()).unwrap()
        })
    });
    group.bench_function("b64_decrypt_decompress_64kb", |b| {
        let c = CompressFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
        let e = EncryptFn.execute(vec![c], &kp).unwrap();
        let encoded = Base64EncodeFn.execute(vec![e], &BTreeMap::new()).unwrap();
        b.iter(|| {
            let d = Base64DecodeFn.execute(vec![encoded.clone()], &BTreeMap::new()).unwrap();
            let dec = DecryptFn.execute(vec![d], &kp).unwrap();
            DecompressFn.execute(vec![dec], &BTreeMap::new()).unwrap()
        })
    });
    group.finish();
}

// 11. Validation throughput
fn bench_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation");
    let json = Bytes::from(serde_json::to_vec(&serde_json::json!({
        "items": (0..1000).map(|i| serde_json::json!({"id": i, "name": format!("item{}", i)})).collect::<Vec<_>>()
    })).unwrap());
    group.throughput(Throughput::Bytes(json.len() as u64));

    group.bench_function("json_validate_1k_items", |b| {
        b.iter(|| JsonValidateFn.execute(vec![json.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("utf8_validate_1k_items", |b| {
        b.iter(|| Utf8ValidateFn.execute(vec![json.clone()], &BTreeMap::new()).unwrap())
    });
    group.bench_function("content_type_1k_items", |b| {
        b.iter(|| ContentTypeFn.execute(vec![json.clone()], &BTreeMap::new()).unwrap())
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_compression,
    bench_crypto_roundtrip,
    bench_sort,
    bench_format_conversion,
    bench_hashing,
    bench_encoding,
    bench_text_processing,
    bench_cas,
    bench_compression_roundtrip,
    bench_pipeline,
    bench_validation,
);
criterion_main!(benches);
