use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use bytes::Bytes;
use deriva_compute::builtins::*;
use deriva_compute::builtins_streaming::*;
use deriva_compute::function::ComputeFunction;
use deriva_compute::pipeline::{PipelineConfig, StreamPipeline};
use deriva_compute::streaming::collect_stream;
use deriva_compute::streaming::StreamingComputeFunction;
use deriva_core::address::{CAddr, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

fn addr(s: &str) -> CAddr { CAddr::from_bytes(s.as_bytes()) }
fn rt() -> tokio::runtime::Runtime { tokio::runtime::Runtime::new().unwrap() }
fn bp(kv: &[(&str, &str)]) -> BTreeMap<String, Value> {
    kv.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}
fn sp(kv: &[(&str, &str)]) -> HashMap<String, String> {
    kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn run_batch(f: &dyn ComputeFunction, input: Bytes, params: &BTreeMap<String, Value>) -> Bytes {
    f.execute(vec![input], params).unwrap()
}

fn run_streaming(
    f: Arc<dyn StreamingComputeFunction>,
    input: Bytes,
    params: HashMap<String, String>,
) -> Bytes {
    rt().block_on(async {
        let mut p = StreamPipeline::new(PipelineConfig::default());
        let s = p.add_source(addr("src"), input);
        p.add_streaming_stage(addr("op"), f, params, vec![s]);
        collect_stream(p.execute().await.unwrap()).await.unwrap()
    })
}

// §7.1 Scenario 1: Identity pass-through at multiple sizes
fn s01_identity(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_01_identity");
    for &size in &[64 * 1024, 1024 * 1024, 10 * 1024 * 1024] {
        let input = Bytes::from(vec![0u8; size]);
        let label = format!("{}KB", size / 1024);
        g.throughput(Throughput::Bytes(size as u64));
        g.bench_with_input(BenchmarkId::new("batch", &label), &input, |b, inp| {
            b.iter(|| run_batch(&IdentityFn, inp.clone(), &BTreeMap::new()))
        });
        g.bench_with_input(BenchmarkId::new("stream", &label), &input, |b, inp| {
            b.iter(|| run_streaming(Arc::new(StreamingIdentity), inp.clone(), HashMap::new()))
        });
    }
    g.finish();
}

// §7.1 Scenario 2: Uppercase transform
fn s02_uppercase(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_02_uppercase");
    let input = Bytes::from(vec![b'a'; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| run_batch(&UppercaseFn, input.clone(), &BTreeMap::new()))
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| run_streaming(Arc::new(StreamingUppercase), input.clone(), HashMap::new()))
    });
    g.finish();
}

// §7.1 Scenario 3: Lowercase transform
fn s03_lowercase(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_03_lowercase");
    let input = Bytes::from(vec![b'A'; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| run_batch(&LowercaseFn, input.clone(), &BTreeMap::new()))
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| run_streaming(Arc::new(StreamingLowercase), input.clone(), HashMap::new()))
    });
    g.finish();
}

// §7.1 Scenario 4: Base64 encode
fn s04_base64_encode(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_04_base64_encode");
    let input = Bytes::from(vec![0xABu8; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| run_batch(&Base64EncodeFn, input.clone(), &BTreeMap::new()))
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| run_streaming(Arc::new(StreamingBase64Encode), input.clone(), HashMap::new()))
    });
    g.finish();
}

// §7.1 Scenario 5: Base64 decode
fn s05_base64_decode(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_05_base64_decode");
    let raw = Bytes::from(vec![0xABu8; 1_048_576]);
    let encoded = Base64EncodeFn.execute(vec![raw], &BTreeMap::new()).unwrap();
    g.throughput(Throughput::Bytes(encoded.len() as u64));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| run_batch(&Base64DecodeFn, encoded.clone(), &BTreeMap::new()))
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| run_streaming(Arc::new(StreamingBase64Decode), encoded.clone(), HashMap::new()))
    });
    g.finish();
}

// §7.1 Scenario 6: Zlib compression
fn s06_compress(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_06_compress");
    let input = Bytes::from(vec![b'x'; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| run_batch(&CompressFn, input.clone(), &BTreeMap::new()))
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| run_streaming(Arc::new(StreamingCompress), input.clone(), HashMap::new()))
    });
    g.finish();
}

// §7.1 Scenario 7: Zlib decompression
fn s07_decompress(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_07_decompress");
    let raw = Bytes::from(vec![b'x'; 1_048_576]);
    let compressed = CompressFn.execute(vec![raw], &BTreeMap::new()).unwrap();
    g.throughput(Throughput::Bytes(compressed.len() as u64));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| run_batch(&DecompressFn, compressed.clone(), &BTreeMap::new()))
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| run_streaming(Arc::new(StreamingDecompress), compressed.clone(), HashMap::new()))
    });
    g.finish();
}

// §7.1 Scenario 8: SHA-256 hashing
fn s08_sha256(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_08_sha256");
    let input = Bytes::from(vec![0u8; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| run_batch(&Sha256Fn, input.clone(), &BTreeMap::new()))
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| run_streaming(Arc::new(StreamingSha256), input.clone(), HashMap::new()))
    });
    g.finish();
}

// §7.1 Scenario 9: Byte count accumulator
fn s09_byte_count(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_09_byte_count");
    let input = Bytes::from(vec![0u8; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| run_batch(&ByteCountFn, input.clone(), &BTreeMap::new()))
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| run_streaming(Arc::new(StreamingByteCount), input.clone(), HashMap::new()))
    });
    g.finish();
}

// §7.1 Scenario 10: XOR transform
fn s10_xor(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_10_xor");
    let input = Bytes::from(vec![0xAA; 1_048_576]);
    let bp_xor = bp(&[("key", "ff")]);
    let sp_xor = sp(&[("key", "ff")]);
    g.throughput(Throughput::Bytes(1_048_576));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| run_batch(&XorFn, input.clone(), &bp_xor))
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| run_streaming(Arc::new(StreamingXor), input.clone(), sp_xor.clone()))
    });
    g.finish();
}

// §7.1 Scenario 11: 3-stage pipeline (upper → lower → sha256)
fn s11_pipeline_3stage(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_11_pipeline_3stage");
    let input = Bytes::from(vec![b'a'; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| {
            let r1 = UppercaseFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
            let r2 = LowercaseFn.execute(vec![r1], &BTreeMap::new()).unwrap();
            Sha256Fn.execute(vec![r2], &BTreeMap::new()).unwrap()
        })
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| {
            rt().block_on(async {
                let mut p = StreamPipeline::new(PipelineConfig::default());
                let s = p.add_source(addr("src"), input.clone());
                let s1 = p.add_streaming_stage(addr("up"), Arc::new(StreamingUppercase), HashMap::new(), vec![s]);
                let s2 = p.add_streaming_stage(addr("lo"), Arc::new(StreamingLowercase), HashMap::new(), vec![s1]);
                p.add_streaming_stage(addr("sha"), Arc::new(StreamingSha256), HashMap::new(), vec![s2]);
                collect_stream(p.execute().await.unwrap()).await.unwrap()
            })
        })
    });
    g.finish();
}

// §7.1 Scenario 12: Compress → encrypt pipeline
fn s12_compress_encrypt(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_12_compress_xor");
    let input = Bytes::from(vec![b'z'; 1_048_576]);
    let bp_xor = bp(&[("key", "ab")]);
    let sp_xor = sp(&[("key", "ab")]);
    g.throughput(Throughput::Bytes(1_048_576));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| {
            let c = CompressFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
            XorFn.execute(vec![c], &bp_xor).unwrap()
        })
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| {
            rt().block_on(async {
                let mut p = StreamPipeline::new(PipelineConfig::default());
                let s = p.add_source(addr("src"), input.clone());
                let s1 = p.add_streaming_stage(addr("cmp"), Arc::new(StreamingCompress), HashMap::new(), vec![s]);
                p.add_streaming_stage(addr("xor"), Arc::new(StreamingXor), sp_xor.clone(), vec![s1]);
                collect_stream(p.execute().await.unwrap()).await.unwrap()
            })
        })
    });
    g.finish();
}

// §7.1 Scenario 13: Scaling — identity at 10MB
fn s13_large_identity(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_13_identity_10mb");
    g.sample_size(10);
    let input = Bytes::from(vec![0u8; 10 * 1024 * 1024]);
    g.throughput(Throughput::Bytes(input.len() as u64));
    g.bench_function("batch", |b| {
        b.iter(|| run_batch(&IdentityFn, input.clone(), &BTreeMap::new()))
    });
    g.bench_function("stream", |b| {
        b.iter(|| run_streaming(Arc::new(StreamingIdentity), input.clone(), HashMap::new()))
    });
    g.finish();
}

// §7.1 Scenario 14: Chunk size impact on streaming throughput
fn s14_chunk_size_impact(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_14_chunk_size");
    let input = Bytes::from(vec![b'a'; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));
    for &chunk in &[4096, 65536, 262144] {
        g.bench_with_input(BenchmarkId::new("stream", chunk), &chunk, |b, &cs| {
            b.iter(|| {
                rt().block_on(async {
                    let cfg = PipelineConfig { chunk_size: cs, ..Default::default() };
                    let mut p = StreamPipeline::new(cfg);
                    let s = p.add_source(addr("src"), input.clone());
                    p.add_streaming_stage(addr("up"), Arc::new(StreamingUppercase), HashMap::new(), vec![s]);
                    collect_stream(p.execute().await.unwrap()).await.unwrap()
                })
            })
        });
    }
    // Batch baseline (no chunking)
    g.bench_function("batch_baseline", |b| {
        b.iter(|| run_batch(&UppercaseFn, input.clone(), &BTreeMap::new()))
    });
    g.finish();
}

// §7.1 Scenario 15: 5-stage deep pipeline
fn s15_pipeline_5stage(c: &mut Criterion) {
    let mut g = c.benchmark_group("7.1_15_pipeline_5stage");
    let input = Bytes::from(vec![b'a'; 1_048_576]);
    g.throughput(Throughput::Bytes(1_048_576));
    g.bench_function("batch_1mb", |b| {
        b.iter(|| {
            let r1 = UppercaseFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
            let r2 = LowercaseFn.execute(vec![r1], &BTreeMap::new()).unwrap();
            let r3 = Base64EncodeFn.execute(vec![r2], &BTreeMap::new()).unwrap();
            let r4 = Base64DecodeFn.execute(vec![r3], &BTreeMap::new()).unwrap();
            Sha256Fn.execute(vec![r4], &BTreeMap::new()).unwrap()
        })
    });
    g.bench_function("stream_1mb", |b| {
        b.iter(|| {
            rt().block_on(async {
                let mut p = StreamPipeline::new(PipelineConfig::default());
                let s = p.add_source(addr("src"), input.clone());
                let s1 = p.add_streaming_stage(addr("up"), Arc::new(StreamingUppercase), HashMap::new(), vec![s]);
                let s2 = p.add_streaming_stage(addr("lo"), Arc::new(StreamingLowercase), HashMap::new(), vec![s1]);
                let s3 = p.add_streaming_stage(addr("b64e"), Arc::new(StreamingBase64Encode), HashMap::new(), vec![s2]);
                let s4 = p.add_streaming_stage(addr("b64d"), Arc::new(StreamingBase64Decode), HashMap::new(), vec![s3]);
                p.add_streaming_stage(addr("sha"), Arc::new(StreamingSha256), HashMap::new(), vec![s4]);
                collect_stream(p.execute().await.unwrap()).await.unwrap()
            })
        })
    });
    g.finish();
}

criterion_group!(
    benches,
    s01_identity,
    s02_uppercase,
    s03_lowercase,
    s04_base64_encode,
    s05_base64_decode,
    s06_compress,
    s07_decompress,
    s08_sha256,
    s09_byte_count,
    s10_xor,
    s11_pipeline_3stage,
    s12_compress_encrypt,
    s13_large_identity,
    s14_chunk_size_impact,
    s15_pipeline_5stage,
);
criterion_main!(benches);
