//! Property-based tests for §2.7 Streaming Materialization
//!
//! Covers 12 correctness properties from the design document using proptest.
//! Uses ProptestConfig::with_cases(50) since streaming tests involve tokio tasks.

use std::collections::HashMap;

use bytes::Bytes;
use proptest::prelude::*;
use tokio::sync::mpsc;

use deriva_compute::builtins_streaming::*;
use deriva_compute::pipeline::{PipelineConfig, StreamPipeline};
use deriva_compute::streaming::{
    collect_stream, tee_stream, value_to_stream, StreamingComputeFunction,
    DEFAULT_CHANNEL_CAPACITY,
};
use deriva_core::streaming::StreamChunk;
use deriva_core::DerivaError;

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

/// Strategy for chunk sizes (1..=512).
fn arb_chunk_size() -> impl Strategy<Value = usize> {
    1usize..=512
}

/// Drain a receiver into a Vec<StreamChunk> for protocol inspection.
async fn drain_stream(mut rx: mpsc::Receiver<StreamChunk>) -> Vec<StreamChunk> {
    let mut chunks = Vec::new();
    while let Some(c) = rx.recv().await {
        chunks.push(c);
    }
    chunks
}

// ============================================================================
// Property 1: Stream Protocol Ordering
// Any stream from value_to_stream follows Data* then End pattern
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_stream_protocol_ordering(
        data in arb_data(),
        chunk_size in arb_chunk_size(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data);
            let rx = value_to_stream(bytes, chunk_size, DEFAULT_CHANNEL_CAPACITY);
            let chunks = drain_stream(rx).await;

            // Must have at least End
            assert!(!chunks.is_empty(), "stream must produce at least End");

            // Last chunk must be End
            assert!(chunks.last().unwrap().is_end(), "last chunk must be End");

            // All chunks before last must be Data
            for chunk in &chunks[..chunks.len() - 1] {
                assert!(chunk.is_data(), "non-terminal chunk must be Data");
            }
        });
    }
}

// ============================================================================
// Property 2: Non-Empty Data Chunks
// No Data(empty) chunks from value_to_stream
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_non_empty_data_chunks(
        data in arb_data(),
        chunk_size in arb_chunk_size(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data);
            let rx = value_to_stream(bytes, chunk_size, DEFAULT_CHANNEL_CAPACITY);
            let chunks = drain_stream(rx).await;

            for chunk in &chunks {
                if let StreamChunk::Data(d) = chunk {
                    assert!(!d.is_empty(), "Data chunk must not be empty");
                }
            }
        });
    }
}

// ============================================================================
// Property 3: Adapter Round-Trip
// collect_stream(value_to_stream(data, chunk_size)) == data for any data and chunk_size > 0
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_adapter_round_trip(
        data in arb_data(),
        chunk_size in arb_chunk_size(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data.clone());
            let rx = value_to_stream(bytes, chunk_size, DEFAULT_CHANNEL_CAPACITY);
            let result = collect_stream(rx).await.unwrap();
            assert_eq!(result.as_ref(), data.as_slice());
        });
    }
}

// ============================================================================
// Property 4: Tee Stream Correctness
// Both outputs of tee_stream collect to same bytes as original
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_tee_stream_correctness(
        data in arb_data(),
        chunk_size in arb_chunk_size(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data.clone());
            let rx = value_to_stream(bytes, chunk_size, DEFAULT_CHANNEL_CAPACITY);
            let (rx_a, rx_b) = tee_stream(rx, DEFAULT_CHANNEL_CAPACITY);

            // Collect both concurrently to avoid deadlock from bounded channels
            let (result_a, result_b) = tokio::join!(
                collect_stream(rx_a),
                collect_stream(rx_b),
            );

            assert_eq!(result_a.unwrap().as_ref(), data.as_slice());
            assert_eq!(result_b.unwrap().as_ref(), data.as_slice());
        });
    }
}

// ============================================================================
// Property 5: Error Propagation
// If input stream has Error, collect_stream returns that error
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_error_propagation(
        prefix_data in arb_data(),
        error_msg in "[a-zA-Z0-9 ]{1,50}",
    ) {
        let rt = rt();
        rt.block_on(async {
            let (tx, rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

            // Send some data, then an error
            tokio::spawn(async move {
                if !prefix_data.is_empty() {
                    let _ = tx.send(StreamChunk::Data(Bytes::from(prefix_data))).await;
                }
                let _ = tx.send(StreamChunk::Error(
                    DerivaError::ComputeFailed(error_msg.clone())
                )).await;
            });

            let result = collect_stream(rx).await;
            assert!(result.is_err(), "collect_stream must return error");
        });
    }
}

// ============================================================================
// Property 6: Pipeline Equivalence
// StreamPipeline with identity function produces same output as input
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_pipeline_identity_equivalence(
        data in arb_nonempty_data(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data.clone());
            let addr = deriva_core::CAddr::from_bytes(b"test-source");
            let stage_addr = deriva_core::CAddr::from_bytes(b"test-identity");

            let config = PipelineConfig {
                chunk_size: 64,
                channel_capacity: DEFAULT_CHANNEL_CAPACITY,
                cache_intermediates: false,
                memory_budget: 0,
                ..Default::default()
            };

            let mut pipeline = StreamPipeline::new(config);
            let src_idx = pipeline.add_source(addr, bytes);
            let identity: std::sync::Arc<dyn StreamingComputeFunction> =
                std::sync::Arc::new(StreamingIdentity);
            pipeline.add_streaming_stage(
                stage_addr,
                identity,
                HashMap::new(),
                vec![src_idx],
            );

            let out_rx = pipeline.execute(None).await.unwrap();
            let result = collect_stream(out_rx).await.unwrap();
            assert_eq!(result.as_ref(), data.as_slice());
        });
    }
}

// ============================================================================
// Property 7: Concat Sequential Correctness
// StreamingConcat with N inputs produces concatenation in order
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_concat_sequential_correctness(
        inputs in prop::collection::vec(arb_data(), 1..5),
        chunk_size in arb_chunk_size(),
    ) {
        let rt = rt();
        rt.block_on(async {
            // Build expected concatenation
            let expected: Vec<u8> = inputs.iter().flatten().copied().collect();

            // Create input streams
            let receivers: Vec<mpsc::Receiver<StreamChunk>> = inputs
                .iter()
                .map(|data| {
                    let bytes = Bytes::from(data.clone());
                    value_to_stream(bytes, chunk_size, DEFAULT_CHANNEL_CAPACITY)
                })
                .collect();

            let concat = StreamingConcat;
            let out = concat.stream_execute(receivers, &HashMap::new()).await;
            let result = collect_stream(out).await.unwrap();
            assert_eq!(result.as_ref(), expected.as_slice());
        });
    }
}

// ============================================================================
// Property 8: ChunkResizer Correctness
// All output chunks (except last) have exactly target_size bytes,
// and collected output == input
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_chunk_resizer_correctness(
        data in arb_nonempty_data(),
        chunk_size in arb_chunk_size(),
        target_size in 1usize..=256,
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data.clone());
            let rx = value_to_stream(bytes, chunk_size, DEFAULT_CHANNEL_CAPACITY);

            let mut params = HashMap::new();
            params.insert("target_size".to_string(), target_size.to_string());

            let resizer = StreamingChunkResizer;
            let out = resizer.stream_execute(vec![rx], &params).await;

            let chunks = drain_stream(out).await;

            // Collect data chunks
            let data_chunks: Vec<&Bytes> = chunks.iter().filter_map(|c| {
                if let StreamChunk::Data(d) = c { Some(d) } else { None }
            }).collect();

            // All except possibly the last data chunk should be exactly target_size
            if data_chunks.len() > 1 {
                for chunk in &data_chunks[..data_chunks.len() - 1] {
                    assert_eq!(
                        chunk.len(), target_size,
                        "non-final chunk must be exactly target_size"
                    );
                }
            }

            // Last data chunk must be <= target_size
            if let Some(last) = data_chunks.last() {
                assert!(last.len() <= target_size, "last chunk must be <= target_size");
            }

            // Total collected must equal input
            let total: Vec<u8> = data_chunks.iter().flat_map(|c| c.iter().copied()).collect();
            assert_eq!(total, data);

            // Last element in stream must be End
            assert!(chunks.last().unwrap().is_end());
        });
    }
}

// ============================================================================
// Property 9: Take/Skip Complementarity
// Take(N) ++ Skip(N) == original input
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_take_skip_complementarity(
        data in arb_nonempty_data(),
        chunk_size in arb_chunk_size(),
        n in 0usize..4096,
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data.clone());
            let n_clamped = n.min(data.len());

            // Take(n)
            let rx_take = value_to_stream(bytes.clone(), chunk_size, DEFAULT_CHANNEL_CAPACITY);
            let mut params_take = HashMap::new();
            params_take.insert("bytes".to_string(), n_clamped.to_string());
            let take_fn = StreamingTake;
            let take_out = take_fn.stream_execute(vec![rx_take], &params_take).await;
            let take_result = collect_stream(take_out).await.unwrap();

            // Skip(n)
            let rx_skip = value_to_stream(bytes.clone(), chunk_size, DEFAULT_CHANNEL_CAPACITY);
            let mut params_skip = HashMap::new();
            params_skip.insert("bytes".to_string(), n_clamped.to_string());
            let skip_fn = StreamingSkip;
            let skip_out = skip_fn.stream_execute(vec![rx_skip], &params_skip).await;
            let skip_result = collect_stream(skip_out).await.unwrap();

            // Concatenation should equal original
            let mut combined = Vec::with_capacity(take_result.len() + skip_result.len());
            combined.extend_from_slice(&take_result);
            combined.extend_from_slice(&skip_result);
            assert_eq!(combined, data);
        });
    }
}

// ============================================================================
// Property 10: XOR Involution
// XOR twice with same key == original
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_xor_involution(
        data in arb_nonempty_data(),
        chunk_size in arb_chunk_size(),
        key in 1u8..=255,
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data.clone());
            let mut params = HashMap::new();
            params.insert("key".to_string(), key.to_string());

            // First XOR pass
            let rx1 = value_to_stream(bytes, chunk_size, DEFAULT_CHANNEL_CAPACITY);
            let xor_fn = StreamingXor;
            let out1 = xor_fn.stream_execute(vec![rx1], &params).await;
            let intermediate = collect_stream(out1).await.unwrap();

            // Second XOR pass
            let rx2 = value_to_stream(intermediate, chunk_size, DEFAULT_CHANNEL_CAPACITY);
            let out2 = xor_fn.stream_execute(vec![rx2], &params).await;
            let result = collect_stream(out2).await.unwrap();

            assert_eq!(result.as_ref(), data.as_slice());
        });
    }
}

// ============================================================================
// Property 11: Compress/Decompress Round-Trip
// compress then decompress == original (per-chunk zlib)
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_compress_decompress_round_trip(
        data in arb_nonempty_data(),
        chunk_size in arb_chunk_size(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data.clone());
            let params = HashMap::new();

            // Compress: each chunk is independently compressed
            let rx = value_to_stream(bytes, chunk_size, DEFAULT_CHANNEL_CAPACITY);
            let compress_fn = StreamingCompress;
            let compressed_rx = compress_fn.stream_execute(vec![rx], &params).await;

            // We need to decompress each chunk independently.
            // The streaming compress/decompress work chunk-by-chunk,
            // so we pass the compressed stream through decompress.
            let decompress_fn = StreamingDecompress;
            let decompressed_rx = decompress_fn.stream_execute(vec![compressed_rx], &params).await;
            let result = collect_stream(decompressed_rx).await.unwrap();

            assert_eq!(result.as_ref(), data.as_slice());
        });
    }
}

// ============================================================================
// Property 12: Accumulator Chunk-Split Invariance
// ByteCount/Sha256 produce same result regardless of chunk boundaries
// **Validates: Requirements 2.7**
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_accumulator_chunk_split_invariance(
        data in arb_nonempty_data(),
        chunk_size_a in arb_chunk_size(),
        chunk_size_b in arb_chunk_size(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let bytes = Bytes::from(data.clone());
            let params = HashMap::new();

            // ByteCount with chunk_size_a
            let rx_a = value_to_stream(bytes.clone(), chunk_size_a, DEFAULT_CHANNEL_CAPACITY);
            let bc = StreamingByteCount;
            let out_a = bc.stream_execute(vec![rx_a], &params).await;
            let count_a = collect_stream(out_a).await.unwrap();

            // ByteCount with chunk_size_b
            let rx_b = value_to_stream(bytes.clone(), chunk_size_b, DEFAULT_CHANNEL_CAPACITY);
            let out_b = bc.stream_execute(vec![rx_b], &params).await;
            let count_b = collect_stream(out_b).await.unwrap();

            assert_eq!(count_a, count_b, "ByteCount must be chunk-split invariant");

            // Verify the count is correct
            let expected_count = (data.len() as u64).to_be_bytes();
            assert_eq!(count_a.as_ref(), &expected_count);

            // SHA-256 with chunk_size_a
            let rx_a = value_to_stream(bytes.clone(), chunk_size_a, DEFAULT_CHANNEL_CAPACITY);
            let sha = StreamingSha256;
            let out_a = sha.stream_execute(vec![rx_a], &params).await;
            let hash_a = collect_stream(out_a).await.unwrap();

            // SHA-256 with chunk_size_b
            let rx_b = value_to_stream(bytes.clone(), chunk_size_b, DEFAULT_CHANNEL_CAPACITY);
            let out_b = sha.stream_execute(vec![rx_b], &params).await;
            let hash_b = collect_stream(out_b).await.unwrap();

            assert_eq!(hash_a, hash_b, "SHA-256 must be chunk-split invariant");
        });
    }
}
