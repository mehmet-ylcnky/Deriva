use deriva_network::bloom::BloomFilter;

#[test]
fn insert_then_contains_returns_true() {
    let mut bf = BloomFilter::new(1000, 7);
    bf.insert(b"hello");
    bf.insert(b"world");
    assert!(bf.contains(b"hello"));
    assert!(bf.contains(b"world"));
}

#[test]
fn contains_non_inserted_returns_false_small_filter() {
    let mut bf = BloomFilter::new(10_000, 7);
    bf.insert(b"alpha");
    bf.insert(b"beta");
    // With a reasonably sized filter, random keys should not match
    assert!(!bf.contains(b"gamma"));
    assert!(!bf.contains(b"delta"));
    assert!(!bf.contains(b"epsilon"));
}

#[test]
fn clear_resets_all_bits() {
    let mut bf = BloomFilter::new(1000, 7);
    bf.insert(b"test");
    assert!(bf.contains(b"test"));
    bf.clear();
    assert!(!bf.contains(b"test"));
}

#[test]
fn as_bytes_from_bytes_roundtrip() {
    let mut bf = BloomFilter::new(8000, 5);
    bf.insert(b"key1");
    bf.insert(b"key2");
    bf.insert(b"key3");

    let bytes = bf.as_bytes().to_vec();
    let bf2 = BloomFilter::from_bytes(&bytes, 5);

    assert!(bf2.contains(b"key1"));
    assert!(bf2.contains(b"key2"));
    assert!(bf2.contains(b"key3"));
    assert!(!bf2.contains(b"key4"));
}

#[test]
fn false_positive_rate_under_5_percent_with_1000_items() {
    // 96,000 bits, 7 hashes, 1000 items → theoretical FPR ~1%
    let mut bf = BloomFilter::new(96_000, 7);
    for i in 0u32..1000 {
        bf.insert(&i.to_le_bytes());
    }

    // Test 10,000 items NOT inserted
    let mut false_positives = 0u32;
    for i in 1000u32..11_000 {
        if bf.contains(&i.to_le_bytes()) {
            false_positives += 1;
        }
    }

    let fpr = false_positives as f64 / 10_000.0;
    assert!(
        fpr < 0.05,
        "False positive rate {:.2}% exceeds 5% threshold",
        fpr * 100.0
    );
}

#[test]
fn empty_filter_contains_nothing() {
    let bf = BloomFilter::new(1000, 7);
    assert!(!bf.contains(b"anything"));
    assert!(!bf.contains(b""));
    assert!(!bf.contains(b"\x00\x00\x00"));
}

#[test]
fn large_data_works() {
    let mut bf = BloomFilter::new(96_000, 7);
    let large_key = vec![0xABu8; 1024];
    bf.insert(&large_key);
    assert!(bf.contains(&large_key));
}
