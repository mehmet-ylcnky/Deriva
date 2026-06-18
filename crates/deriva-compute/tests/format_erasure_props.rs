//! Property-based tests for erasure coding round-trip (Property 6).
//!
//! **Validates: Requirements 5.3**
//!
//! For any random byte sequence and valid (data_shards, parity_shards) config,
//! encoding then dropping up to parity_shards shards then decoding SHALL
//! produce data exactly equal to the original input.

#![cfg(feature = "format-erasure")]

use bytes::Bytes;
use proptest::prelude::*;
use std::collections::BTreeMap;

use deriva_compute::builtins_format_erasure::{ReedSolomonDecodeFn, ReedSolomonEncodeFn};
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
        .collect()
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Generate random byte vectors between 1 and 1024 bytes.
fn data_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 1..=1024)
}

/// Generate valid (data_shards, parity_shards) combinations.
/// data_shards: 2..=10, parity_shards: 1..=5
fn shard_config_strategy() -> impl Strategy<Value = (usize, usize)> {
    (2usize..=10, 1usize..=5)
}

// ---------------------------------------------------------------------------
// Property 6: Erasure coding round-trip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 5.3**
    ///
    /// For any random byte sequence and valid (data_shards, parity_shards) config,
    /// encode → drop up to parity_shards shards → decode SHALL exactly reconstruct
    /// the original data.
    #[test]
    fn erasure_coding_roundtrip(
        data in data_strategy(),
        (data_shards, parity_shards) in shard_config_strategy(),
    ) {
        let total_shards = data_shards + parity_shards;

        // Encode
        let encode_params = params(&[
            ("data_shards", &data_shards.to_string()),
            ("parity_shards", &parity_shards.to_string()),
        ]);
        let input = Bytes::from(data.clone());
        let encoded = ReedSolomonEncodeFn
            .execute(vec![input], &encode_params)
            .expect("ReedSolomonEncodeFn should succeed");

        // Generate missing indices (0 to parity_shards unique indices)
        // We do this deterministically from the test inputs to keep proptest happy
        let runner = proptest::test_runner::TestRunner::deterministic();
        let _ = runner;

        // For the property test, we test with a random subset using a nested strategy.
        // Instead, we'll generate the missing set inline from the data bytes as a seed.
        let num_missing = if data.is_empty() {
            0
        } else {
            (data[0] as usize) % (parity_shards + 1)
        };

        let mut missing_indices: Vec<usize> = Vec::new();
        for i in 0..num_missing {
            let idx = if data.len() > i + 1 {
                (data[i + 1] as usize) % total_shards
            } else {
                i % total_shards
            };
            if !missing_indices.contains(&idx) {
                missing_indices.push(idx);
            }
        }
        // Ensure we don't exceed parity_shards
        missing_indices.truncate(parity_shards);

        // Build missing parameter string
        let missing_str = missing_indices
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let decode_params = params(&[
            ("data_shards", &data_shards.to_string()),
            ("parity_shards", &parity_shards.to_string()),
            ("missing", &missing_str),
        ]);

        // Decode
        let decoded = ReedSolomonDecodeFn
            .execute(vec![encoded], &decode_params)
            .expect(&format!(
                "ReedSolomonDecodeFn should succeed with missing=[{}], data_shards={}, parity_shards={}",
                missing_str, data_shards, parity_shards
            ));

        // Verify exact reconstruction
        prop_assert_eq!(
            decoded.as_ref(),
            data.as_slice(),
            "Erasure coding round-trip failed.\n\
             data_shards={}, parity_shards={}, missing=[{}], data_len={}",
            data_shards,
            parity_shards,
            missing_str,
            data.len()
        );
    }
}
