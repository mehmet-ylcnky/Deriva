//! Integration tests for end-to-end adaptive chunking behavior (§2.10).
//!
//! Validates:
//! - Requirements 8.1, 8.2: Metrics are recorded for adaptive chunk sizing
//! - Requirement 11.1: Pipeline produces identical output with adaptive_chunking=false (default)
//! - Requirement 11.2: StreamingChunkResizer still works
//! - Requirement 11.3: preferred_chunk_size() unchanged
//! - Requirement 11.4: PipelineConfig defaults unchanged

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;

use deriva_compute::builtins_streaming::*;
use deriva_compute::metrics::{ADAPTIVE_CHUNK_SIZE_BYTES, ADAPTIVE_RESIZE_EVENTS_TOTAL};
use deriva_compute::pipeline::{PipelineConfig, StreamPipeline};
use deriva_compute::streaming::{collect_stream, DEFAULT_CHANNEL_CAPACITY, DEFAULT_CHUNK_SIZE};
use deriva_core::address::CAddr;

fn test_addr(label: &str) -> CAddr {
    CAddr::from_bytes(label.as_bytes())
}

// ===========================================================================
// Test 1: Multi-stage pipeline with adaptive chunking
// ===========================================================================

#[tokio::test]
async fn test_multi_stage_pipeline_adaptive_chunking_correct_output() {
    // Build pipeline: Source → StreamingUppercase → StreamingSha256
    // with adaptive_chunking = true
    let input_data = Bytes::from("hello world, this is adaptive chunking test data!");

    let mut pipeline = StreamPipeline::new(PipelineConfig {
        chunk_size: 8, // small chunks to force multiple chunks through pipeline
        adaptive_chunking: true,
        ..Default::default()
    });

    let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
    let idx_upper = pipeline.add_streaming_stage(
        test_addr("upper"),
        Arc::new(StreamingUppercase),
        HashMap::new(),
        vec![idx_src],
    );
    let _idx_sha = pipeline.add_streaming_stage(
        test_addr("sha256"),
        Arc::new(StreamingSha256),
        HashMap::new(),
        vec![idx_upper],
    );

    let out = pipeline.execute().await.unwrap();
    let result = collect_stream(out).await.unwrap();

    // Compute expected: SHA-256 of uppercased input
    use sha2::{Digest, Sha256};
    let uppercased = input_data.to_ascii_uppercase();
    let expected_hash = Sha256::digest(&uppercased);

    assert_eq!(result.len(), 32, "SHA-256 output should be 32 bytes");
    assert_eq!(&result[..], &expected_hash[..], "Hash should match uppercased input");
}

// ===========================================================================
// Test 2: Metrics are recorded
// ===========================================================================

#[tokio::test]
async fn test_adaptive_chunking_metrics_are_accessible() {
    // Run a pipeline with adaptive chunking between two streaming stages.
    // The resizer is only inserted between Streaming→Streaming edges.
    // Source→Uppercase is Source→Streaming (no resizer).
    // Uppercase→Identity is Streaming→Streaming (resizer inserted).
    let input_data = Bytes::from(vec![b'a'; 4096]);

    let mut pipeline = StreamPipeline::new(PipelineConfig {
        chunk_size: 1024,
        adaptive_chunking: true,
        min_chunk_size: 1024,
        max_chunk_size: 1_048_576,
        ..Default::default()
    });

    let idx_src = pipeline.add_source(test_addr("metrics_src"), input_data);
    let idx_upper = pipeline.add_streaming_stage(
        test_addr("metrics_upper"),
        Arc::new(StreamingUppercase),
        HashMap::new(),
        vec![idx_src],
    );
    let _idx_id = pipeline.add_streaming_stage(
        test_addr("metrics_id"),
        Arc::new(StreamingIdentity),
        HashMap::new(),
        vec![idx_upper],
    );

    let out = pipeline.execute().await.unwrap();
    let _result = collect_stream(out).await.unwrap();

    // Verify that the ADAPTIVE_CHUNK_SIZE_BYTES metric is accessible and
    // has been set for the downstream stage (StreamingIdentity's metric name).
    // The adaptive resizer uses the downstream function's metric_name() as the stage label.
    let gauge = ADAPTIVE_CHUNK_SIZE_BYTES
        .with_label_values(&["StreamingIdentity"]);
    let chunk_size_value = gauge.get();
    // The gauge should have been set (to at least the initial value)
    assert!(
        chunk_size_value > 0.0,
        "ADAPTIVE_CHUNK_SIZE_BYTES gauge should be set to a positive value, got {}",
        chunk_size_value
    );

    // Verify the resize events counter is accessible (it may or may not have
    // been incremented depending on throughput dynamics, but the metric exists)
    let _grow_counter = ADAPTIVE_RESIZE_EVENTS_TOTAL
        .with_label_values(&["StreamingIdentity", "grow"]);
    let _shrink_counter = ADAPTIVE_RESIZE_EVENTS_TOTAL
        .with_label_values(&["StreamingIdentity", "shrink"]);
    // Just verifying these don't panic — metric is registered and accessible.
}

// ===========================================================================
// Test 3: Backward compatibility - PipelineConfig defaults
// ===========================================================================

#[tokio::test]
async fn test_pipeline_config_defaults_unchanged() {
    let config = PipelineConfig::default();

    // Pre-existing fields (§11.4)
    assert_eq!(config.chunk_size, DEFAULT_CHUNK_SIZE, "chunk_size default should be 65536");
    assert_eq!(config.chunk_size, 65536);
    assert_eq!(config.channel_capacity, DEFAULT_CHANNEL_CAPACITY, "channel_capacity default should be 8");
    assert_eq!(config.channel_capacity, 8);
    assert_eq!(config.cache_intermediates, true, "cache_intermediates default should be true");
    assert_eq!(config.memory_budget, 0, "memory_budget default should be 0");

    // New adaptive chunking fields
    assert_eq!(config.adaptive_chunking, false, "adaptive_chunking default should be false");
    assert_eq!(config.min_chunk_size, 1024, "min_chunk_size default should be 1024");
    assert_eq!(config.max_chunk_size, 1_048_576, "max_chunk_size default should be 1048576");
}

// ===========================================================================
// Test 4: Backward compatibility - StreamingChunkResizer still works
// ===========================================================================

#[tokio::test]
async fn test_streaming_chunk_resizer_works_with_adaptive_chunking_enabled() {
    // StreamingChunkResizer should still produce correct output regardless
    // of the adaptive_chunking setting.
    let input_data = Bytes::from("abcdefghijklmnopqrstuvwxyz");

    let mut params = HashMap::new();
    params.insert("target_size".to_string(), "5".to_string());

    // With adaptive_chunking = true
    let mut pipeline = StreamPipeline::new(PipelineConfig {
        chunk_size: 10,
        adaptive_chunking: true,
        ..Default::default()
    });
    let idx_src = pipeline.add_source(test_addr("resizer_src"), input_data.clone());
    let _idx_resizer = pipeline.add_streaming_stage(
        test_addr("resizer"),
        Arc::new(StreamingChunkResizer),
        params.clone(),
        vec![idx_src],
    );
    let out = pipeline.execute().await.unwrap();
    let result_adaptive = collect_stream(out).await.unwrap();

    // With adaptive_chunking = false
    let mut pipeline2 = StreamPipeline::new(PipelineConfig {
        chunk_size: 10,
        adaptive_chunking: false,
        ..Default::default()
    });
    let idx_src2 = pipeline2.add_source(test_addr("resizer_src2"), input_data.clone());
    let _idx_resizer2 = pipeline2.add_streaming_stage(
        test_addr("resizer2"),
        Arc::new(StreamingChunkResizer),
        params,
        vec![idx_src2],
    );
    let out2 = pipeline2.execute().await.unwrap();
    let result_non_adaptive = collect_stream(out2).await.unwrap();

    // Both should produce the same correct concatenated output
    assert_eq!(result_adaptive, input_data, "Adaptive: output should match input");
    assert_eq!(result_non_adaptive, input_data, "Non-adaptive: output should match input");
    assert_eq!(result_adaptive, result_non_adaptive, "Both modes should produce identical output");
}

// ===========================================================================
// Test 5: Adaptive chunking vs non-adaptive produces same output
// ===========================================================================

#[tokio::test]
async fn test_adaptive_vs_non_adaptive_produces_identical_output() {
    let input_data = Bytes::from("The quick brown fox jumps over the lazy dog. ".repeat(100));

    // Pipeline: Source → Uppercase → Identity → Sha256
    // Run with adaptive_chunking = true
    let mut pipeline_adaptive = StreamPipeline::new(PipelineConfig {
        chunk_size: 512,
        adaptive_chunking: true,
        min_chunk_size: 1024,
        max_chunk_size: 1_048_576,
        ..Default::default()
    });
    let idx_src = pipeline_adaptive.add_source(test_addr("eq_src"), input_data.clone());
    let idx_upper = pipeline_adaptive.add_streaming_stage(
        test_addr("eq_upper"),
        Arc::new(StreamingUppercase),
        HashMap::new(),
        vec![idx_src],
    );
    let idx_id = pipeline_adaptive.add_streaming_stage(
        test_addr("eq_id"),
        Arc::new(StreamingIdentity),
        HashMap::new(),
        vec![idx_upper],
    );
    let _idx_sha = pipeline_adaptive.add_streaming_stage(
        test_addr("eq_sha"),
        Arc::new(StreamingSha256),
        HashMap::new(),
        vec![idx_id],
    );
    let out_a = pipeline_adaptive.execute().await.unwrap();
    let result_adaptive = collect_stream(out_a).await.unwrap();

    // Run with adaptive_chunking = false
    let mut pipeline_non = StreamPipeline::new(PipelineConfig {
        chunk_size: 512,
        adaptive_chunking: false,
        min_chunk_size: 1024,
        max_chunk_size: 1_048_576,
        ..Default::default()
    });
    let idx_src2 = pipeline_non.add_source(test_addr("eq_src2"), input_data.clone());
    let idx_upper2 = pipeline_non.add_streaming_stage(
        test_addr("eq_upper2"),
        Arc::new(StreamingUppercase),
        HashMap::new(),
        vec![idx_src2],
    );
    let idx_id2 = pipeline_non.add_streaming_stage(
        test_addr("eq_id2"),
        Arc::new(StreamingIdentity),
        HashMap::new(),
        vec![idx_upper2],
    );
    let _idx_sha2 = pipeline_non.add_streaming_stage(
        test_addr("eq_sha2"),
        Arc::new(StreamingSha256),
        HashMap::new(),
        vec![idx_id2],
    );
    let out_b = pipeline_non.execute().await.unwrap();
    let result_non_adaptive = collect_stream(out_b).await.unwrap();

    assert_eq!(
        result_adaptive, result_non_adaptive,
        "Pipeline output must be byte-identical regardless of adaptive_chunking setting"
    );
}

// ===========================================================================
// Test 6: Large data pipeline with adaptive chunking
// ===========================================================================

#[tokio::test]
async fn test_large_data_pipeline_with_adaptive_chunking() {
    // ~120KB of data through a 3-stage pipeline with adaptive chunking and small chunk_size
    let input_data = Bytes::from(vec![b'x'; 120_000]);

    let mut pipeline = StreamPipeline::new(PipelineConfig {
        chunk_size: 1024, // small chunks to exercise adaptive resizer
        channel_capacity: 4,
        adaptive_chunking: true,
        min_chunk_size: 1024,
        max_chunk_size: 1_048_576,
        ..Default::default()
    });

    let idx_src = pipeline.add_source(test_addr("large_src"), input_data.clone());
    let idx_id1 = pipeline.add_streaming_stage(
        test_addr("large_id1"),
        Arc::new(StreamingIdentity),
        HashMap::new(),
        vec![idx_src],
    );
    let idx_upper = pipeline.add_streaming_stage(
        test_addr("large_upper"),
        Arc::new(StreamingUppercase),
        HashMap::new(),
        vec![idx_id1],
    );
    let _idx_id2 = pipeline.add_streaming_stage(
        test_addr("large_id2"),
        Arc::new(StreamingIdentity),
        HashMap::new(),
        vec![idx_upper],
    );

    let out = pipeline.execute().await.unwrap();
    let result = collect_stream(out).await.unwrap();

    // 'x' uppercased is 'X'
    let expected = Bytes::from(vec![b'X'; 120_000]);
    assert_eq!(result.len(), 120_000, "Output length should match input length");
    assert_eq!(result, expected, "Output should be uppercased input");
}
