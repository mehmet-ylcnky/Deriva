use deriva_compute::builtins;
use deriva_compute::registry::FunctionRegistry;
use deriva_core::address::FunctionId;

#[test]
#[cfg(feature = "extended-batch")]
fn accumulator_functions_are_registered() {
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);

    let expected_accumulators = [
        ("sha256_acc", "1.0.0"),
        ("sha512_acc", "1.0.0"),
        ("blake3_acc", "1.0.0"),
        ("crc32_acc", "1.0.0"),
        ("line_count_acc", "1.0.0"),
        ("word_count_acc", "1.0.0"),
        ("checksum_adler32", "1.0.0"),
    ];

    for (name, version) in expected_accumulators {
        let id = FunctionId::new(name, version);
        assert!(
            registry.contains(&id),
            "Expected accumulator function '{}@{}' to be registered",
            name,
            version
        );
    }
}

#[test]
#[cfg(feature = "extended-batch")]
fn registry_total_count_is_at_least_100() {
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);

    // The registry should have a substantial number of functions.
    // Base (~96) + extended-batch (~51 with accumulators) + streaming registrations
    // We verify it's at least 100 batch functions total (the spec minimum).
    let count = registry.len();
    assert!(
        count >= 100,
        "Expected at least 100 batch functions, got {}",
        count
    );
}

#[test]
#[cfg(feature = "extended-batch")]
fn duplicate_registration_does_not_increase_count() {
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);

    let count_before = registry.len();

    // Re-register the extended batch functions (includes accumulators).
    // Since HashMap::insert overwrites, count should remain the same.
    builtins::register_extended_batch(&mut registry);

    let count_after = registry.len();

    assert_eq!(
        count_before, count_after,
        "Duplicate registration should not increase count (HashMap::insert overwrites). Before: {}, After: {}",
        count_before, count_after
    );
}

#[test]
#[cfg(feature = "extended-batch")]
fn extended_batch_adds_seven_accumulator_functions() {
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);

    // Verify all 7 accumulator functions are present
    let accumulator_ids: Vec<FunctionId> = vec![
        FunctionId::new("sha256_acc", "1.0.0"),
        FunctionId::new("sha512_acc", "1.0.0"),
        FunctionId::new("blake3_acc", "1.0.0"),
        FunctionId::new("crc32_acc", "1.0.0"),
        FunctionId::new("line_count_acc", "1.0.0"),
        FunctionId::new("word_count_acc", "1.0.0"),
        FunctionId::new("checksum_adler32", "1.0.0"),
    ];

    let acc_count = accumulator_ids
        .iter()
        .filter(|id| registry.contains(id))
        .count();

    assert_eq!(
        acc_count, 7,
        "All 7 accumulator functions should be registered, found {}",
        acc_count
    );
}
