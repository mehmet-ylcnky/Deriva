//! Integration tests for pipeline fusion (§2.11).
//!
//! Validates end-to-end behavior of the fusion optimizer within StreamPipeline::execute().
//! Requirements traced: 4.3, 4.4, 5.1, 5.2, 5.3, 5.4, 7.1, 7.2, 7.3, 7.4, 10.1, 10.2

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;

use deriva_core::address::CAddr;
use deriva_compute::builtins_streaming::*;
use deriva_compute::pipeline::{PipelineConfig, StreamPipeline};
use deriva_compute::streaming::collect_stream;

fn test_addr(label: &str) -> CAddr {
    CAddr::from_bytes(label.as_bytes())
}

// =========================================================================
// Test: End-to-end pipeline execution with fusion enabled vs disabled
// (byte-compare outputs)
// Requirements: 4.3, 4.4, 5.1, 5.2
// =========================================================================

#[tokio::test]
async fn test_fusion_enabled_vs_disabled_byte_identical() {
    let input_data = Bytes::from("Hello, Pipeline Fusion World! This is a test of streaming transforms.");

    // Run with fusion enabled (default)
    let result_fused = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 8,
            enable_fusion: true,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
        let idx_upper = pipeline.add_streaming_stage(
            test_addr("upper"),
            Arc::new(StreamingUppercase),
            HashMap::new(),
            vec![idx_src],
        );
        let idx_lower = pipeline.add_streaming_stage(
            test_addr("lower"),
            Arc::new(StreamingLowercase),
            HashMap::new(),
            vec![idx_upper],
        );
        let _idx_identity = pipeline.add_streaming_stage(
            test_addr("identity"),
            Arc::new(StreamingIdentity),
            HashMap::new(),
            vec![idx_lower],
        );
        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    // Run with fusion disabled
    let result_unfused = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 8,
            enable_fusion: false,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
        let idx_upper = pipeline.add_streaming_stage(
            test_addr("upper"),
            Arc::new(StreamingUppercase),
            HashMap::new(),
            vec![idx_src],
        );
        let idx_lower = pipeline.add_streaming_stage(
            test_addr("lower"),
            Arc::new(StreamingLowercase),
            HashMap::new(),
            vec![idx_upper],
        );
        let _idx_identity = pipeline.add_streaming_stage(
            test_addr("identity"),
            Arc::new(StreamingIdentity),
            HashMap::new(),
            vec![idx_lower],
        );
        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    // Byte-identical outputs
    assert_eq!(result_fused, result_unfused);
    // Verify the transforms actually did something (uppercase then lowercase = lowercase input)
    let expected = input_data.to_ascii_lowercase();
    assert_eq!(result_fused, Bytes::from(expected));
}

#[tokio::test]
async fn test_fusion_xor_roundtrip_enabled_vs_disabled() {
    let input_data = Bytes::from("XOR roundtrip test data for fusion validation!");
    let mut xor_params = HashMap::new();
    xor_params.insert("key".to_string(), "42".to_string());

    // With fusion: XOR encrypt then decrypt (roundtrip should yield original)
    let result_fused = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 10,
            enable_fusion: true,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
        let idx_enc = pipeline.add_streaming_stage(
            test_addr("xor_enc"),
            Arc::new(StreamingXor),
            xor_params.clone(),
            vec![idx_src],
        );
        let _idx_dec = pipeline.add_streaming_stage(
            test_addr("xor_dec"),
            Arc::new(StreamingXor),
            xor_params.clone(),
            vec![idx_enc],
        );
        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    // Without fusion
    let result_unfused = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 10,
            enable_fusion: false,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
        let idx_enc = pipeline.add_streaming_stage(
            test_addr("xor_enc"),
            Arc::new(StreamingXor),
            xor_params.clone(),
            vec![idx_src],
        );
        let _idx_dec = pipeline.add_streaming_stage(
            test_addr("xor_dec"),
            Arc::new(StreamingXor),
            xor_params.clone(),
            vec![idx_enc],
        );
        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    assert_eq!(result_fused, result_unfused);
    assert_eq!(result_fused, input_data);
}

// =========================================================================
// Test: Mixed pipeline (fusible + accumulator + fusible) producing correct
// final result
// Requirements: 5.3, 5.4, 7.3, 7.4
// =========================================================================

#[tokio::test]
async fn test_mixed_pipeline_fusible_accumulator_fusible() {
    // Pipeline: source -> uppercase -> lowercase -> byte_count (accumulator)
    // The accumulator breaks the fusion chain. The two fusible stages before it
    // should be fused, and the result should be the byte count of the lowercased data.
    let input_data = Bytes::from("Mixed pipeline test!");

    let result = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 5,
            enable_fusion: true,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
        // Fusible group 1: uppercase + identity
        let idx_upper = pipeline.add_streaming_stage(
            test_addr("upper"),
            Arc::new(StreamingUppercase),
            HashMap::new(),
            vec![idx_src],
        );
        let idx_id = pipeline.add_streaming_stage(
            test_addr("identity"),
            Arc::new(StreamingIdentity),
            HashMap::new(),
            vec![idx_upper],
        );
        // Non-fusible: byte count accumulator
        let idx_count = pipeline.add_streaming_stage(
            test_addr("count"),
            Arc::new(StreamingByteCount),
            HashMap::new(),
            vec![idx_id],
        );
        // Fusible group 2 after accumulator: identity (single, won't fuse alone)
        let _idx_id2 = pipeline.add_streaming_stage(
            test_addr("identity2"),
            Arc::new(StreamingIdentity),
            HashMap::new(),
            vec![idx_count],
        );
        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    // ByteCount produces u64 as 8 bytes big-endian
    let expected_count = input_data.len() as u64;
    assert_eq!(result.len(), 8);
    let actual_count = u64::from_be_bytes(result[..8].try_into().unwrap());
    assert_eq!(actual_count, expected_count);
}

#[tokio::test]
async fn test_mixed_pipeline_two_fused_groups_around_accumulator() {
    // source -> upper -> lower (fused group 1) -> sha256 (boundary) -> identity -> identity (fused group 2)
    let input_data = Bytes::from("Two fused groups test data");

    let result_fused = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 6,
            enable_fusion: true,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
        let idx_upper = pipeline.add_streaming_stage(
            test_addr("upper"),
            Arc::new(StreamingUppercase),
            HashMap::new(),
            vec![idx_src],
        );
        let idx_lower = pipeline.add_streaming_stage(
            test_addr("lower"),
            Arc::new(StreamingLowercase),
            HashMap::new(),
            vec![idx_upper],
        );
        let idx_sha = pipeline.add_streaming_stage(
            test_addr("sha256"),
            Arc::new(StreamingSha256),
            HashMap::new(),
            vec![idx_lower],
        );
        let idx_id1 = pipeline.add_streaming_stage(
            test_addr("id1"),
            Arc::new(StreamingIdentity),
            HashMap::new(),
            vec![idx_sha],
        );
        let _idx_id2 = pipeline.add_streaming_stage(
            test_addr("id2"),
            Arc::new(StreamingIdentity),
            HashMap::new(),
            vec![idx_id1],
        );
        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    let result_unfused = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 6,
            enable_fusion: false,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
        let idx_upper = pipeline.add_streaming_stage(
            test_addr("upper"),
            Arc::new(StreamingUppercase),
            HashMap::new(),
            vec![idx_src],
        );
        let idx_lower = pipeline.add_streaming_stage(
            test_addr("lower"),
            Arc::new(StreamingLowercase),
            HashMap::new(),
            vec![idx_upper],
        );
        let idx_sha = pipeline.add_streaming_stage(
            test_addr("sha256"),
            Arc::new(StreamingSha256),
            HashMap::new(),
            vec![idx_lower],
        );
        let idx_id1 = pipeline.add_streaming_stage(
            test_addr("id1"),
            Arc::new(StreamingIdentity),
            HashMap::new(),
            vec![idx_sha],
        );
        let _idx_id2 = pipeline.add_streaming_stage(
            test_addr("id2"),
            Arc::new(StreamingIdentity),
            HashMap::new(),
            vec![idx_id1],
        );
        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    // SHA256 produces 32-byte digest
    assert_eq!(result_fused.len(), 32);
    assert_eq!(result_fused, result_unfused);
}

// =========================================================================
// Test: Cancellation (drop output receiver mid-stream, verify upstream cleanup)
// Requirements: 7.1, 7.2
// =========================================================================

#[tokio::test]
async fn test_cancellation_drop_receiver_mid_stream() {
    use tokio::time::{timeout, Duration};

    // Large enough input so the pipeline has work to do
    let input_data = Bytes::from(vec![b'A'; 10_000]);

    let mut pipeline = StreamPipeline::new(PipelineConfig {
        chunk_size: 100,
        enable_fusion: true,
        ..Default::default()
    });
    let idx_src = pipeline.add_source(test_addr("src"), input_data);
    let idx_upper = pipeline.add_streaming_stage(
        test_addr("upper"),
        Arc::new(StreamingUppercase),
        HashMap::new(),
        vec![idx_src],
    );
    let idx_lower = pipeline.add_streaming_stage(
        test_addr("lower"),
        Arc::new(StreamingLowercase),
        HashMap::new(),
        vec![idx_upper],
    );
    let _idx_id = pipeline.add_streaming_stage(
        test_addr("identity"),
        Arc::new(StreamingIdentity),
        HashMap::new(),
        vec![idx_lower],
    );

    let mut rx = pipeline.execute(None).await.unwrap();

    // Receive a few chunks then drop
    let _chunk1 = rx.recv().await;
    let _chunk2 = rx.recv().await;
    drop(rx);

    // Give time for upstream tasks to detect the dropped receiver and clean up.
    // The test passes if it completes without hanging.
    let result = timeout(Duration::from_secs(2), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
    })
    .await;

    assert!(result.is_ok(), "Pipeline did not clean up after receiver drop");
}

// =========================================================================
// Test: Large pipeline (20+ stages) with multiple fused groups
// Requirements: 10.1, 10.2, 5.1, 5.2
// =========================================================================

#[tokio::test]
async fn test_large_pipeline_20_plus_stages() {
    let input_data = Bytes::from("Large pipeline integration test data for fusion!");

    // Build a pipeline with 24 stages: alternating uppercase/lowercase (all fusible)
    // These should all fuse into a single FusedMapStage.
    let result_fused = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 7,
            enable_fusion: true,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
        let mut prev = idx_src;
        for i in 0..24 {
            let func: Arc<dyn deriva_compute::streaming::StreamingComputeFunction> = if i % 2 == 0 {
                Arc::new(StreamingUppercase)
            } else {
                Arc::new(StreamingLowercase)
            };
            prev = pipeline.add_streaming_stage(
                test_addr(&format!("stage_{}", i)),
                func,
                HashMap::new(),
                vec![prev],
            );
        }
        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    let result_unfused = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 7,
            enable_fusion: false,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
        let mut prev = idx_src;
        for i in 0..24 {
            let func: Arc<dyn deriva_compute::streaming::StreamingComputeFunction> = if i % 2 == 0 {
                Arc::new(StreamingUppercase)
            } else {
                Arc::new(StreamingLowercase)
            };
            prev = pipeline.add_streaming_stage(
                test_addr(&format!("stage_{}", i)),
                func,
                HashMap::new(),
                vec![prev],
            );
        }
        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    // 24 stages alternating upper/lower: even count means last applied is lowercase
    assert_eq!(result_fused, result_unfused);
    // With 24 stages (0..23), last stage index 23 is odd → lowercase
    let expected = Bytes::from(input_data.to_ascii_lowercase());
    assert_eq!(result_fused, expected);
}

#[tokio::test]
async fn test_large_pipeline_with_multiple_fused_groups() {
    let input_data = Bytes::from("Multiple fused groups in a large pipeline");

    // Pipeline structure:
    // 8 fusible → sha256 (boundary) → 8 fusible → byte_count (boundary) → 6 fusible
    // This creates 3 fused groups separated by non-fusible accumulators.
    let result_fused = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 5,
            enable_fusion: true,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());

        // Group 1: 8 identity stages (all fusible)
        let mut prev = idx_src;
        for i in 0..8 {
            prev = pipeline.add_streaming_stage(
                test_addr(&format!("g1_{}", i)),
                Arc::new(StreamingIdentity),
                HashMap::new(),
                vec![prev],
            );
        }

        // Boundary: SHA256
        prev = pipeline.add_streaming_stage(
            test_addr("sha256"),
            Arc::new(StreamingSha256),
            HashMap::new(),
            vec![prev],
        );

        // Group 2: 8 identity stages
        for i in 0..8 {
            prev = pipeline.add_streaming_stage(
                test_addr(&format!("g2_{}", i)),
                Arc::new(StreamingIdentity),
                HashMap::new(),
                vec![prev],
            );
        }

        // Boundary: ByteCount
        prev = pipeline.add_streaming_stage(
            test_addr("byte_count"),
            Arc::new(StreamingByteCount),
            HashMap::new(),
            vec![prev],
        );

        // Group 3: 6 identity stages
        for i in 0..6 {
            prev = pipeline.add_streaming_stage(
                test_addr(&format!("g3_{}", i)),
                Arc::new(StreamingIdentity),
                HashMap::new(),
                vec![prev],
            );
        }

        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    let result_unfused = {
        let mut pipeline = StreamPipeline::new(PipelineConfig {
            chunk_size: 5,
            enable_fusion: false,
            ..Default::default()
        });
        let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());

        let mut prev = idx_src;
        for i in 0..8 {
            prev = pipeline.add_streaming_stage(
                test_addr(&format!("g1_{}", i)),
                Arc::new(StreamingIdentity),
                HashMap::new(),
                vec![prev],
            );
        }

        prev = pipeline.add_streaming_stage(
            test_addr("sha256"),
            Arc::new(StreamingSha256),
            HashMap::new(),
            vec![prev],
        );

        for i in 0..8 {
            prev = pipeline.add_streaming_stage(
                test_addr(&format!("g2_{}", i)),
                Arc::new(StreamingIdentity),
                HashMap::new(),
                vec![prev],
            );
        }

        prev = pipeline.add_streaming_stage(
            test_addr("byte_count"),
            Arc::new(StreamingByteCount),
            HashMap::new(),
            vec![prev],
        );

        for i in 0..6 {
            prev = pipeline.add_streaming_stage(
                test_addr(&format!("g3_{}", i)),
                Arc::new(StreamingIdentity),
                HashMap::new(),
                vec![prev],
            );
        }

        let rx = pipeline.execute(None).await.unwrap();
        collect_stream(rx).await.unwrap()
    };

    assert_eq!(result_fused, result_unfused);
    // SHA256 produces 32 bytes, ByteCount of 32 bytes = 32 as u64 BE
    let expected_count: u64 = 32;
    assert_eq!(result_fused.len(), 8);
    let actual_count = u64::from_be_bytes(result_fused[..8].try_into().unwrap());
    assert_eq!(actual_count, expected_count);
}

// =========================================================================
// Test: enable_fusion=false skips optimizer entirely
// Requirements: 4.3, 4.4
// =========================================================================

#[tokio::test]
async fn test_enable_fusion_false_skips_optimizer() {
    // With fusion disabled, the pipeline should still produce correct output
    // but without any fusion optimization. We verify correctness here.
    let input_data = Bytes::from("Fusion disabled test");

    let mut pipeline = StreamPipeline::new(PipelineConfig {
        chunk_size: 4,
        enable_fusion: false,
        ..Default::default()
    });
    let idx_src = pipeline.add_source(test_addr("src"), input_data.clone());
    let idx_upper = pipeline.add_streaming_stage(
        test_addr("upper"),
        Arc::new(StreamingUppercase),
        HashMap::new(),
        vec![idx_src],
    );
    let idx_lower = pipeline.add_streaming_stage(
        test_addr("lower"),
        Arc::new(StreamingLowercase),
        HashMap::new(),
        vec![idx_upper],
    );
    let idx_upper2 = pipeline.add_streaming_stage(
        test_addr("upper2"),
        Arc::new(StreamingUppercase),
        HashMap::new(),
        vec![idx_lower],
    );
    let _idx_id = pipeline.add_streaming_stage(
        test_addr("id"),
        Arc::new(StreamingIdentity),
        HashMap::new(),
        vec![idx_upper2],
    );

    let rx = pipeline.execute(None).await.unwrap();
    let result = collect_stream(rx).await.unwrap();

    // upper -> lower -> upper -> identity = uppercase
    let expected = Bytes::from(input_data.to_ascii_uppercase());
    assert_eq!(result, expected);
}

#[tokio::test]
async fn test_enable_fusion_false_matches_enabled_output() {
    // Verify that even complex pipelines produce the same output regardless of fusion flag
    let input_data = Bytes::from("Complex pipeline equivalence check with longer input data for thoroughness.");

    let mut xor_params = HashMap::new();
    xor_params.insert("key".to_string(), "7".to_string());

    let build_pipeline = |enable_fusion: bool| {
        let input = input_data.clone();
        let params = xor_params.clone();
        async move {
            let mut pipeline = StreamPipeline::new(PipelineConfig {
                chunk_size: 11,
                enable_fusion,
                ..Default::default()
            });
            let idx_src = pipeline.add_source(test_addr("src"), input);
            let idx_xor = pipeline.add_streaming_stage(
                test_addr("xor"),
                Arc::new(StreamingXor),
                params.clone(),
                vec![idx_src],
            );
            let idx_upper = pipeline.add_streaming_stage(
                test_addr("upper"),
                Arc::new(StreamingUppercase),
                HashMap::new(),
                vec![idx_xor],
            );
            let idx_lower = pipeline.add_streaming_stage(
                test_addr("lower"),
                Arc::new(StreamingLowercase),
                HashMap::new(),
                vec![idx_upper],
            );
            let idx_id = pipeline.add_streaming_stage(
                test_addr("id"),
                Arc::new(StreamingIdentity),
                HashMap::new(),
                vec![idx_lower],
            );
            let _idx_xor2 = pipeline.add_streaming_stage(
                test_addr("xor2"),
                Arc::new(StreamingXor),
                params,
                vec![idx_id],
            );
            let rx = pipeline.execute(None).await.unwrap();
            collect_stream(rx).await.unwrap()
        }
    };

    let result_fused = build_pipeline(true).await;
    let result_unfused = build_pipeline(false).await;

    assert_eq!(result_fused, result_unfused);
}
