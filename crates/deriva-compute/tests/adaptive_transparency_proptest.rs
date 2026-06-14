//! Property-based test for Pipeline Semantic Transparency (Property 6).
//!
//! Verifies that for any valid pipeline DAG with streaming stages and any input data,
//! the final collected output with `adaptive_chunking = true` is byte-identical to the
//! output with `adaptive_chunking = false`. Adaptive chunking affects only internal chunk
//! boundaries, never the semantic content.
//!
//! **Validates: Requirements 4.4, 5.7, 9.2, 11.1**

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use proptest::prelude::*;

use deriva_compute::builtins_streaming::{StreamingIdentity, StreamingLowercase, StreamingUppercase};
use deriva_compute::pipeline::{PipelineConfig, StreamPipeline};
use deriva_compute::streaming::{collect_stream, StreamingComputeFunction};
use deriva_core::CAddr;

// ============================================================================
// Helpers
// ============================================================================

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn test_addr(label: &str) -> CAddr {
    CAddr::from_bytes(label.as_bytes())
}

/// Strategy for arbitrary byte data (0..4KB to keep tests fast).
fn arb_data() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..4096)
}

/// Strategy for chunk sizes valid for adaptive chunking (must satisfy min <= chunk <= max).
/// We pick chunk_size in [1024, 8192] to ensure it is always >= min_chunk_size (1024).
fn arb_chunk_size() -> impl Strategy<Value = usize> {
    1024usize..=8192
}

/// Strategy for a streaming function index (0 = Identity, 1 = Uppercase, 2 = Lowercase).
fn arb_stage_fn() -> impl Strategy<Value = usize> {
    0usize..3
}

/// Strategy for number of pipeline stages (1 to 3).
fn arb_num_stages() -> impl Strategy<Value = usize> {
    1usize..=3
}

/// Build a streaming function from an index.
fn make_streaming_fn(idx: usize) -> Arc<dyn StreamingComputeFunction> {
    match idx % 3 {
        0 => Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
        1 => Arc::new(StreamingUppercase) as Arc<dyn StreamingComputeFunction>,
        2 => Arc::new(StreamingLowercase) as Arc<dyn StreamingComputeFunction>,
        _ => unreachable!(),
    }
}

/// Build and execute a pipeline with given stages and config, returning collected output bytes.
async fn execute_pipeline(
    input_data: &[u8],
    stage_fns: &[usize],
    config: PipelineConfig,
) -> Bytes {
    let mut pipeline = StreamPipeline::new(config);
    let src_idx = pipeline.add_source(test_addr("source"), Bytes::from(input_data.to_vec()));

    let mut prev_idx = src_idx;
    for (i, &fn_idx) in stage_fns.iter().enumerate() {
        let func = make_streaming_fn(fn_idx);
        prev_idx = pipeline.add_streaming_stage(
            test_addr(&format!("stage-{}", i)),
            func,
            HashMap::new(),
            vec![prev_idx],
        );
    }

    let rx = pipeline.execute(None).await.unwrap();
    collect_stream(rx).await.unwrap()
}

// ============================================================================
// Property 6: Pipeline Semantic Transparency
//
// For any valid pipeline DAG with streaming stages and any input data,
// the final collected output with adaptive_chunking = true SHALL be
// byte-identical to the output with adaptive_chunking = false.
//
// **Validates: Requirements 4.4, 5.7, 9.2, 11.1**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_pipeline_semantic_transparency(
        input_data in arb_data(),
        num_stages in arb_num_stages(),
        stage_indices in prop::collection::vec(arb_stage_fn(), 1..=3),
        chunk_size in arb_chunk_size(),
        min_chunk_size in Just(1024usize),
        max_chunk_size in prop_oneof![
            Just(8192usize),
            Just(65536usize),
            Just(1_048_576usize),
        ],
    ) {
        // Ensure we use exactly num_stages stages
        let stages: Vec<usize> = stage_indices.into_iter().take(num_stages).collect();
        // Skip if we don't have enough stages generated
        prop_assume!(!stages.is_empty());

        // Ensure chunk_size <= max_chunk_size for valid config
        let chunk_size = chunk_size.min(max_chunk_size);

        let rt = rt();
        rt.block_on(async {
            // Execute WITHOUT adaptive chunking
            let config_off = PipelineConfig {
                chunk_size,
                channel_capacity: 8,
                cache_intermediates: false,
                memory_budget: 0,
                streaming_threshold: 0,
                adaptive_chunking: false,
                min_chunk_size,
                max_chunk_size,
                enable_fusion: true,
                per_pipeline_max: 0,
            };
            let output_off = execute_pipeline(&input_data, &stages, config_off).await;

            // Execute WITH adaptive chunking
            let config_on = PipelineConfig {
                chunk_size,
                channel_capacity: 8,
                cache_intermediates: false,
                memory_budget: 0,
                streaming_threshold: 0,
                adaptive_chunking: true,
                min_chunk_size,
                max_chunk_size,
                enable_fusion: true,
                per_pipeline_max: 0,
            };
            let output_on = execute_pipeline(&input_data, &stages, config_on).await;

            // Outputs must be byte-identical
            assert_eq!(
                output_off, output_on,
                "Pipeline output must be identical with adaptive_chunking=true vs false.\n\
                 Input len: {}, stages: {:?}, chunk_size: {}, min: {}, max: {}",
                input_data.len(), stages, chunk_size, min_chunk_size, max_chunk_size
            );
        });
    }
}
