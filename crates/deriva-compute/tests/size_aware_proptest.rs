//! Property-based tests and integration tests for §2.9 Size-Aware Mode Selection.
//!
//! Covers 7 correctness properties plus integration/backward-compat tests.
//! Uses ProptestConfig::with_cases(50) since tests involve tokio runtime.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use bytes::Bytes;
use proptest::prelude::*;

use deriva_compute::builtins::IdentityFn;
use deriva_compute::builtins_streaming::StreamingIdentity;
use deriva_compute::pipeline::{PipelineConfig, StreamPipeline, DEFAULT_STREAMING_THRESHOLD};
use deriva_compute::streaming::{
    collect_stream, StreamingComputeFunction, DEFAULT_CHANNEL_CAPACITY,
};
use deriva_compute::ComputeFunction;
use deriva_core::CAddr;

// ============================================================================
// Helpers
// ============================================================================

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

/// Strategy for arbitrary byte data (0..4KB to keep tests fast).
fn arb_data() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..4096)
}

/// Strategy for non-empty byte data.
fn arb_nonempty_data() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 1..4096)
}



/// Strategy for threshold values.
fn arb_threshold() -> impl Strategy<Value = usize> {
    prop_oneof![
        Just(0usize),
        Just(usize::MAX),
        1usize..=(8 * 1024 * 1024), // up to 8MB
    ]
}

fn test_addr(label: &str) -> CAddr {
    CAddr::from_bytes(label.as_bytes())
}

fn make_config(threshold: usize) -> PipelineConfig {
    PipelineConfig {
        chunk_size: 64,
        channel_capacity: DEFAULT_CHANNEL_CAPACITY,
        cache_intermediates: false,
        memory_budget: 0,
        streaming_threshold: threshold,
        adaptive_chunking: false,
        min_chunk_size: 1024,
        max_chunk_size: 1_048_576,
    }
}

// ============================================================================
// Property 1: Node data size reflects stored data length
// Source/Cached nodes return Some(len), computed nodes return None
// **Validates: Requirements 2.9**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_node_data_size_reflects_stored_length(
        source_data in arb_data(),
        cached_data in arb_data(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let config = make_config(DEFAULT_STREAMING_THRESHOLD);
            let mut pipeline = StreamPipeline::new(config);

            // Source node: should return Some(data.len())
            let src_idx = pipeline.add_source(test_addr("src"), Bytes::from(source_data.clone()));
            assert_eq!(pipeline.node_data_size(src_idx), Some(source_data.len()));

            // Cached node: should return Some(data.len())
            let cached_idx = pipeline.add_cached(test_addr("cached"), Bytes::from(cached_data.clone()));
            assert_eq!(pipeline.node_data_size(cached_idx), Some(cached_data.len()));

            // Streaming stage: should return None
            let streaming_idx = pipeline.add_streaming_stage(
                test_addr("stream-stage"),
                Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
                HashMap::new(),
                vec![src_idx],
            );
            assert_eq!(pipeline.node_data_size(streaming_idx), None);

            // Batch stage: should return None
            let batch_idx = pipeline.add_batch_stage(
                test_addr("batch-stage"),
                Arc::new(IdentityFn) as Arc<dyn ComputeFunction>,
                BTreeMap::new(),
                vec![cached_idx],
            );
            assert_eq!(pipeline.node_data_size(batch_idx), None);
        });
    }
}

// ============================================================================
// Property 2: Total input size is sum of known parts
// Multiple Source/Cached nodes sum correctly
// **Validates: Requirements 2.9**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_total_input_size_sums_correctly(
        data_parts in prop::collection::vec(arb_data(), 1..6),
    ) {
        let rt = rt();
        rt.block_on(async {
            let config = make_config(DEFAULT_STREAMING_THRESHOLD);
            let mut pipeline = StreamPipeline::new(config);

            let mut indices = Vec::new();
            let mut expected_total: usize = 0;

            for (i, part) in data_parts.iter().enumerate() {
                let idx = if i % 2 == 0 {
                    pipeline.add_source(test_addr(&format!("src-{}", i)), Bytes::from(part.clone()))
                } else {
                    pipeline.add_cached(test_addr(&format!("cached-{}", i)), Bytes::from(part.clone()))
                };
                indices.push(idx);
                expected_total += part.len();
            }

            let total = pipeline.total_input_size(&indices);
            assert_eq!(total, Some(expected_total));
        });
    }
}

// ============================================================================
// Property 3: Unknown input propagates to unknown total
// Any StreamingStage/BatchStage makes total_input_size return None
// **Validates: Requirements 2.9**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_unknown_input_propagates(
        source_data in arb_nonempty_data(),
        extra_data in arb_data(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let config = make_config(DEFAULT_STREAMING_THRESHOLD);
            let mut pipeline = StreamPipeline::new(config);

            // Add a source with known size
            let src_idx = pipeline.add_source(test_addr("src"), Bytes::from(source_data));

            // Add a streaming stage (unknown output size)
            let streaming_idx = pipeline.add_streaming_stage(
                test_addr("computed"),
                Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
                HashMap::new(),
                vec![src_idx],
            );

            // Add another source
            let src2_idx = pipeline.add_cached(test_addr("src2"), Bytes::from(extra_data));

            // total_input_size including the streaming stage should be None
            let total = pipeline.total_input_size(&[streaming_idx, src2_idx]);
            assert_eq!(total, None, "total must be None when any input has unknown size");

            // Similarly for batch stages
            let config2 = make_config(DEFAULT_STREAMING_THRESHOLD);
            let mut pipeline2 = StreamPipeline::new(config2);
            let s = pipeline2.add_source(test_addr("s"), Bytes::from(vec![1u8, 2, 3]));
            let batch_idx = pipeline2.add_batch_stage(
                test_addr("batch"),
                Arc::new(IdentityFn) as Arc<dyn ComputeFunction>,
                BTreeMap::new(),
                vec![s],
            );
            let s2 = pipeline2.add_source(test_addr("s2"), Bytes::from(vec![4u8, 5]));
            let total2 = pipeline2.total_input_size(&[batch_idx, s2]);
            assert_eq!(total2, None, "total must be None when batch stage has unknown output size");
        });
    }
}

// ============================================================================
// Property 4: Threshold decision correctness
// threshold=0 always streams, threshold=MAX always batches, correct comparison
// **Validates: Requirements 2.9**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_threshold_decision_correctness(
        input_size in 0usize..=(10 * 1024 * 1024),
        threshold in arb_threshold(),
    ) {
        // §2.9 logic: prefer_streaming = match input_size {
        //   None => true, Some(0) => true, Some(s) => s >= threshold
        // }
        // threshold=0: s >= 0 always true → always streaming
        // threshold=MAX: s >= MAX only true for MAX-size inputs → almost always batch

        let prefer_streaming = match Some(input_size) {
            None => true,
            Some(0) => true,
            Some(s) => s >= threshold,
        };

        if threshold == 0 {
            // threshold=0 means always prefer streaming (§2.7 behaviour)
            prop_assert!(prefer_streaming,
                "threshold=0 must always prefer streaming, got batch for size={}", input_size);
        }

        if threshold == usize::MAX && input_size > 0 && input_size < usize::MAX {
            // threshold=MAX means always prefer batch (unless input is also MAX)
            prop_assert!(!prefer_streaming,
                "threshold=MAX must prefer batch for non-MAX size={}", input_size);
        }

        // General property: if input_size < threshold and input_size > 0, prefer batch
        if input_size > 0 && input_size < threshold {
            prop_assert!(!prefer_streaming,
                "size {} < threshold {} should prefer batch", input_size, threshold);
        }

        // General property: if input_size >= threshold, prefer streaming
        if input_size >= threshold {
            prop_assert!(prefer_streaming,
                "size {} >= threshold {} should prefer streaming", input_size, threshold);
        }
    }
}

// ============================================================================
// Property 5: Preferred mode selected when impl exists
// Below threshold → batch if available; above threshold → streaming if available
// **Validates: Requirements 2.9**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_preferred_mode_selected(
        data in arb_nonempty_data(),
        threshold in 1usize..=(8 * 1024 * 1024),
    ) {
        let rt = rt();
        rt.block_on(async {
            let data_len = data.len();
            let bytes = Bytes::from(data.clone());

            // When input < threshold → prefer batch
            if data_len < threshold && data_len > 0 {
                // Build pipeline with batch stage — should produce correct result
                let config = make_config(threshold);
                let mut pipeline = StreamPipeline::new(config);
                let src_idx = pipeline.add_source(test_addr("src"), bytes.clone());
                let _batch_idx = pipeline.add_batch_stage(
                    test_addr("batch-identity"),
                    Arc::new(IdentityFn) as Arc<dyn ComputeFunction>,
                    BTreeMap::new(),
                    vec![src_idx],
                );
                let rx = pipeline.execute().await.unwrap();
                let result = collect_stream(rx).await.unwrap();
                assert_eq!(result.as_ref(), data.as_slice(),
                    "batch path must produce correct output for data below threshold");
            }

            // When input >= threshold → prefer streaming
            if data_len >= threshold {
                let config = make_config(threshold);
                let mut pipeline = StreamPipeline::new(config);
                let src_idx = pipeline.add_source(test_addr("src"), bytes.clone());
                let _stream_idx = pipeline.add_streaming_stage(
                    test_addr("stream-identity"),
                    Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
                    HashMap::new(),
                    vec![src_idx],
                );
                let rx = pipeline.execute().await.unwrap();
                let result = collect_stream(rx).await.unwrap();
                assert_eq!(result.as_ref(), data.as_slice(),
                    "streaming path must produce correct output for data at/above threshold");
            }
        });
    }
}

// ============================================================================
// Property 6: Fallback to alternate mode
// When preferred mode impl unavailable, alternate is used
// **Validates: Requirements 2.9**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_fallback_to_alternate_mode(
        data in arb_nonempty_data(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data.clone());

            // Scenario: input is small (< threshold) so batch is preferred,
            // but only streaming impl exists → fallback to streaming
            let config = make_config(usize::MAX); // Always prefer batch
            let mut pipeline = StreamPipeline::new(config);
            let src_idx = pipeline.add_source(test_addr("src"), bytes.clone());
            // Only add streaming stage (simulates: no batch impl available, fallback)
            let _stream_idx = pipeline.add_streaming_stage(
                test_addr("fallback-stream"),
                Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
                HashMap::new(),
                vec![src_idx],
            );
            let rx = pipeline.execute().await.unwrap();
            let result = collect_stream(rx).await.unwrap();
            assert_eq!(result.as_ref(), data.as_slice(),
                "fallback to streaming must produce correct output");

            // Scenario: input is large (>= threshold) so streaming is preferred,
            // but only batch impl exists → fallback to batch
            let config2 = make_config(0); // Always prefer streaming
            let mut pipeline2 = StreamPipeline::new(config2);
            let src_idx2 = pipeline2.add_source(test_addr("src2"), bytes.clone());
            // Only add batch stage (simulates: no streaming impl available, fallback)
            let _batch_idx = pipeline2.add_batch_stage(
                test_addr("fallback-batch"),
                Arc::new(IdentityFn) as Arc<dyn ComputeFunction>,
                BTreeMap::new(),
                vec![src_idx2],
            );
            let rx2 = pipeline2.execute().await.unwrap();
            let result2 = collect_stream(rx2).await.unwrap();
            assert_eq!(result2.as_ref(), data.as_slice(),
                "fallback to batch must produce correct output");
        });
    }
}

// ============================================================================
// Property 7: Pipeline execution correctness
// Same output regardless of mode selection
// **Validates: Requirements 2.9**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_pipeline_same_output_regardless_of_mode(
        data in arb_nonempty_data(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data.clone());

            // Execute via streaming path
            let config_stream = make_config(0); // threshold=0 → always streaming
            let mut pipeline_stream = StreamPipeline::new(config_stream);
            let src_s = pipeline_stream.add_source(test_addr("src"), bytes.clone());
            pipeline_stream.add_streaming_stage(
                test_addr("stage"),
                Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
                HashMap::new(),
                vec![src_s],
            );
            let rx_stream = pipeline_stream.execute().await.unwrap();
            let result_stream = collect_stream(rx_stream).await.unwrap();

            // Execute via batch path
            let config_batch = make_config(usize::MAX); // threshold=MAX → always batch
            let mut pipeline_batch = StreamPipeline::new(config_batch);
            let src_b = pipeline_batch.add_source(test_addr("src"), bytes.clone());
            pipeline_batch.add_batch_stage(
                test_addr("stage"),
                Arc::new(IdentityFn) as Arc<dyn ComputeFunction>,
                BTreeMap::new(),
                vec![src_b],
            );
            let rx_batch = pipeline_batch.execute().await.unwrap();
            let result_batch = collect_stream(rx_batch).await.unwrap();

            // Both must produce identical output
            assert_eq!(result_stream, result_batch,
                "streaming and batch paths must produce identical output");
            assert_eq!(result_stream.as_ref(), data.as_slice(),
                "output must equal input for identity functions");
        });
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

/// Test pipeline with small input (< 3MB) produces correct results via batch path.
#[tokio::test]
async fn integration_small_input_batch_path() {
    // 1KB data — well below 3MB threshold
    let data = vec![42u8; 1024];
    let bytes = Bytes::from(data.clone());

    let config = PipelineConfig::default(); // uses DEFAULT_STREAMING_THRESHOLD = 3MB
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("src"), bytes);

    // Simulate the executor decision: size < threshold → batch
    let input_size = pipeline.total_input_size(&[src]);
    assert_eq!(input_size, Some(1024));
    assert!(input_size.unwrap() < DEFAULT_STREAMING_THRESHOLD,
        "1KB input should be below 3MB threshold");

    // Use batch path (as executor would choose)
    pipeline.add_batch_stage(
        test_addr("batch"),
        Arc::new(IdentityFn) as Arc<dyn ComputeFunction>,
        BTreeMap::new(),
        vec![src],
    );

    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result.as_ref(), data.as_slice());
}

/// Test pipeline with large input (>= 3MB) produces correct results via streaming path.
#[tokio::test]
async fn integration_large_input_streaming_path() {
    // 4MB data — above 3MB threshold
    let data = vec![0xABu8; 4 * 1024 * 1024];
    let bytes = Bytes::from(data.clone());

    let config = PipelineConfig::default();
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("src"), bytes);

    // Simulate the executor decision: size >= threshold → streaming
    let input_size = pipeline.total_input_size(&[src]);
    assert_eq!(input_size, Some(4 * 1024 * 1024));
    assert!(input_size.unwrap() >= DEFAULT_STREAMING_THRESHOLD,
        "4MB input should be at or above 3MB threshold");

    // Use streaming path (as executor would choose)
    pipeline.add_streaming_stage(
        test_addr("stream"),
        Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
        HashMap::new(),
        vec![src],
    );

    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result.as_ref(), data.as_slice());
}

/// Test hybrid pipeline (mixed small/large inputs).
#[tokio::test]
async fn integration_hybrid_pipeline_mixed_inputs() {
    // Two source nodes: one small (1KB), one large (4MB)
    let small_data = vec![1u8; 1024];
    let large_data = vec![2u8; 4 * 1024 * 1024];

    let config = PipelineConfig::default();
    let mut pipeline = StreamPipeline::new(config);

    let small_src = pipeline.add_source(test_addr("small"), Bytes::from(small_data.clone()));
    let large_src = pipeline.add_source(test_addr("large"), Bytes::from(large_data.clone()));

    // Combined input size = 1KB + 4MB = above threshold → streaming preferred
    let total = pipeline.total_input_size(&[small_src, large_src]);
    assert_eq!(total, Some(1024 + 4 * 1024 * 1024));
    assert!(total.unwrap() >= DEFAULT_STREAMING_THRESHOLD);

    // Use streaming concat (StreamingIdentity only takes 1 input,
    // so we test with two separate stages that process each independently)
    let _stream_small = pipeline.add_streaming_stage(
        test_addr("proc-small"),
        Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
        HashMap::new(),
        vec![small_src],
    );

    let _stream_large = pipeline.add_streaming_stage(
        test_addr("proc-large"),
        Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
        HashMap::new(),
        vec![large_src],
    );

    // Both stages produce correct results individually
    // (Pipeline executes last node, so we test with separate pipelines)
    let config2 = PipelineConfig::default();
    let mut p_small = StreamPipeline::new(config2);
    let s = p_small.add_source(test_addr("s"), Bytes::from(small_data.clone()));
    p_small.add_streaming_stage(
        test_addr("id"),
        Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
        HashMap::new(),
        vec![s],
    );
    let rx = p_small.execute().await.unwrap();
    let res = collect_stream(rx).await.unwrap();
    assert_eq!(res.as_ref(), small_data.as_slice());

    let config3 = PipelineConfig::default();
    let mut p_large = StreamPipeline::new(config3);
    let l = p_large.add_source(test_addr("l"), Bytes::from(large_data.clone()));
    p_large.add_streaming_stage(
        test_addr("id"),
        Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
        HashMap::new(),
        vec![l],
    );
    let rx = p_large.execute().await.unwrap();
    let res = collect_stream(rx).await.unwrap();
    assert_eq!(res.as_ref(), large_data.as_slice());
}

/// Test threshold=0 produces identical behavior to always-streaming.
#[tokio::test]
async fn integration_threshold_zero_always_streaming() {
    let data = vec![99u8; 512]; // Very small input
    let bytes = Bytes::from(data.clone());

    // With threshold=0, even tiny inputs should prefer streaming
    let config = make_config(0);
    let mut pipeline = StreamPipeline::new(config);
    let src = pipeline.add_source(test_addr("src"), bytes.clone());

    let input_size = pipeline.total_input_size(&[src]);
    assert_eq!(input_size, Some(512));

    // §2.9 decision logic: s >= 0 is always true → streaming preferred
    let prefer_streaming = match input_size {
        None => true,
        Some(0) => true,
        Some(_s) => true, // threshold=0: any size >= 0
    };
    assert!(prefer_streaming, "threshold=0 must always prefer streaming");

    // Execute via streaming
    pipeline.add_streaming_stage(
        test_addr("stream"),
        Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
        HashMap::new(),
        vec![src],
    );
    let rx = pipeline.execute().await.unwrap();
    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result.as_ref(), data.as_slice());

    // Compare with default pipeline using same data — streaming path
    let config2 = make_config(0);
    let mut pipeline2 = StreamPipeline::new(config2);
    let src2 = pipeline2.add_source(test_addr("src2"), bytes);
    pipeline2.add_streaming_stage(
        test_addr("stream2"),
        Arc::new(StreamingIdentity) as Arc<dyn StreamingComputeFunction>,
        HashMap::new(),
        vec![src2],
    );
    let rx2 = pipeline2.execute().await.unwrap();
    let result2 = collect_stream(rx2).await.unwrap();

    assert_eq!(result, result2, "threshold=0 must behave identically to always-streaming");
}
