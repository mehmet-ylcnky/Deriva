use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use bytes::Bytes;
use deriva_compute::builtins_streaming::*;
use deriva_compute::pipeline::{PipelineConfig, StreamPipeline};
use deriva_compute::streaming::collect_stream;
use deriva_core::address::CAddr;
use std::collections::HashMap;
use std::sync::Arc;

fn test_addr(label: &str) -> CAddr {
    CAddr::from_bytes(label.as_bytes())
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn bench_streaming_vs_batch_1mb(c: &mut Criterion) {
    let data = Bytes::from(vec![0u8; 1_000_000]);
    c.bench_function("streaming_identity_1mb", |b| {
        b.iter(|| {
            rt().block_on(async {
                let mut p = StreamPipeline::new(PipelineConfig::default());
                let s = p.add_source(test_addr("a"), data.clone());
                p.add_streaming_stage(
                    test_addr("id"),
                    Arc::new(StreamingIdentity),
                    HashMap::new(),
                    vec![s],
                );
                let rx = p.execute().await.unwrap();
                collect_stream(rx).await.unwrap()
            })
        });
    });
}

fn bench_streaming_vs_batch_100mb(c: &mut Criterion) {
    let data = Bytes::from(vec![0u8; 100_000_000]);
    let mut group = c.benchmark_group("streaming_identity_100mb");
    group.sample_size(10);
    group.bench_function("100mb", |b| {
        b.iter(|| {
            rt().block_on(async {
                let mut p = StreamPipeline::new(PipelineConfig::default());
                let s = p.add_source(test_addr("a"), data.clone());
                p.add_streaming_stage(
                    test_addr("id"),
                    Arc::new(StreamingIdentity),
                    HashMap::new(),
                    vec![s],
                );
                let rx = p.execute().await.unwrap();
                collect_stream(rx).await.unwrap()
            })
        });
    });
    group.finish();
}

fn bench_streaming_3_stage_pipeline(c: &mut Criterion) {
    let data = Bytes::from(vec![b'a'; 1_000_000]);
    c.bench_function("streaming_3stage_1mb", |b| {
        b.iter(|| {
            rt().block_on(async {
                let mut p = StreamPipeline::new(PipelineConfig::default());
                let s = p.add_source(test_addr("src"), data.clone());
                let s1 = p.add_streaming_stage(
                    test_addr("upper"),
                    Arc::new(StreamingUppercase),
                    HashMap::new(),
                    vec![s],
                );
                let s2 = p.add_streaming_stage(
                    test_addr("lower"),
                    Arc::new(StreamingLowercase),
                    HashMap::new(),
                    vec![s1],
                );
                p.add_streaming_stage(
                    test_addr("sha"),
                    Arc::new(StreamingSha256),
                    HashMap::new(),
                    vec![s2],
                );
                let rx = p.execute().await.unwrap();
                collect_stream(rx).await.unwrap()
            })
        });
    });
}

fn bench_backpressure_slow_consumer(c: &mut Criterion) {
    use deriva_core::streaming::StreamChunk;
    let data = Bytes::from(vec![0u8; 256_000]); // 256KB = 4 chunks at 64KB
    c.bench_function("streaming_backpressure", |b| {
        b.iter(|| {
            rt().block_on(async {
                let mut p = StreamPipeline::new(PipelineConfig::default());
                let s = p.add_source(test_addr("a"), data.clone());
                p.add_streaming_stage(
                    test_addr("id"),
                    Arc::new(StreamingIdentity),
                    HashMap::new(),
                    vec![s],
                );
                let mut rx = p.execute().await.unwrap();
                let mut total = 0usize;
                loop {
                    match rx.recv().await {
                        Some(StreamChunk::Data(d)) => {
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                            total += d.len();
                        }
                        Some(StreamChunk::End) | None => break,
                        Some(StreamChunk::Error(e)) => panic!("{e:?}"),
                    }
                }
                total
            })
        });
    });
}

fn bench_chunk_size_comparison(c: &mut Criterion) {
    let data = Bytes::from(vec![0u8; 1_000_000]);
    let mut group = c.benchmark_group("chunk_size_comparison");
    for size in [1024, 16_384, 65_536, 262_144, 1_048_576] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &sz| {
            b.iter(|| {
                rt().block_on(async {
                    let cfg = PipelineConfig { chunk_size: sz, ..Default::default() };
                    let mut p = StreamPipeline::new(cfg);
                    let s = p.add_source(test_addr("a"), data.clone());
                    p.add_streaming_stage(
                        test_addr("id"),
                        Arc::new(StreamingIdentity),
                        HashMap::new(),
                        vec![s],
                    );
                    let rx = p.execute().await.unwrap();
                    collect_stream(rx).await.unwrap()
                })
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_streaming_vs_batch_1mb,
    bench_streaming_vs_batch_100mb,
    bench_streaming_3_stage_pipeline,
    bench_backpressure_slow_consumer,
    bench_chunk_size_comparison,
);
criterion_main!(benches);
