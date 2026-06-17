use bytes::Bytes;
use deriva_compute::builtins::*;
use deriva_compute::function::{ComputeError, ComputeFunction};
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn exec1(f: &dyn ComputeFunction, input: &[u8]) -> Result<Bytes, ComputeError> {
    f.execute(vec![Bytes::from(input.to_vec())], &BTreeMap::new())
}

fn exec1_params(f: &dyn ComputeFunction, input: &[u8], params: BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    f.execute(vec![Bytes::from(input.to_vec())], &params)
}

fn params(kv: &[(&str, &str)]) -> BTreeMap<String, Value> {
    kv.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

const TEST_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TEST_NONCE: &str = "00112233445566778899aabbccddeeff";
const TEST_GCM_NONCE: &str = "000102030405060708090a0b";

fn read_u64(b: &Bytes) -> u64 { u64::from_be_bytes(b[..8].try_into().unwrap()) }
fn read_f64(b: &Bytes) -> f64 { f64::from_be_bytes(b[..8].try_into().unwrap()) }




// ── #42 ByteCountFn ──

#[test]
fn byte_count_empty() {
    assert_eq!(read_u64(&exec1(&ByteCountFn, b"").unwrap()), 0);
}

#[test]
fn byte_count_known_length() {
    assert_eq!(read_u64(&exec1(&ByteCountFn, b"hello").unwrap()), 5);
}

#[test]
fn byte_count_binary() {
    let input = vec![0u8; 10_000];
    assert_eq!(read_u64(&exec1(&ByteCountFn, &input).unwrap()), 10_000);
}

#[test]
fn byte_count_output_is_8_bytes() {
    assert_eq!(exec1(&ByteCountFn, b"x").unwrap().len(), 8);
}

#[test]
fn byte_count_null_bytes() {
    assert_eq!(read_u64(&exec1(&ByteCountFn, &[0x00, 0x00, 0x00]).unwrap()), 3);
}


// ── #43 LineCountFn ──

#[test]
fn line_count_empty() {
    assert_eq!(exec1(&LineCountFn, b"").unwrap().as_ref(), b"0");
}

#[test]
fn line_count_no_trailing_newline() {
    // "abc" has 0 newline characters
    assert_eq!(exec1(&LineCountFn, b"abc").unwrap().as_ref(), b"0");
}

#[test]
fn line_count_with_trailing_newline() {
    assert_eq!(exec1(&LineCountFn, b"abc\n").unwrap().as_ref(), b"1");
}

#[test]
fn line_count_multiple_lines() {
    assert_eq!(exec1(&LineCountFn, b"a\nb\nc\n").unwrap().as_ref(), b"3");
}

#[test]
fn line_count_single_newline() {
    assert_eq!(exec1(&LineCountFn, b"\n").unwrap().as_ref(), b"1");
}


// ── #44 WordCountFn ──

#[test]
fn word_count_empty() {
    assert_eq!(exec1(&WordCountFn, b"").unwrap().as_ref(), b"0");
}

#[test]
fn word_count_single_word() {
    assert_eq!(exec1(&WordCountFn, b"hello").unwrap().as_ref(), b"1");
}

#[test]
fn word_count_multiple_words() {
    assert_eq!(exec1(&WordCountFn, b"the quick brown fox").unwrap().as_ref(), b"4");
}

#[test]
fn word_count_multiple_spaces() {
    assert_eq!(exec1(&WordCountFn, b"  hello   world  ").unwrap().as_ref(), b"2");
}

#[test]
fn word_count_tabs_and_newlines() {
    assert_eq!(exec1(&WordCountFn, b"a\tb\nc\rd").unwrap().as_ref(), b"4");
}


// ── #45 HistogramFn ──

#[test]
fn histogram_output_size() {
    assert_eq!(exec1(&HistogramFn, b"abc").unwrap().len(), 2048);
}

#[test]
fn histogram_empty() {
    let result = exec1(&HistogramFn, b"").unwrap();
    assert!(result.iter().all(|&b| b == 0));
}

#[test]
fn histogram_single_byte() {
    let result = exec1(&HistogramFn, &[0x42]).unwrap();
    let offset = 0x42 * 8;
    let count = u64::from_be_bytes(result[offset..offset + 8].try_into().unwrap());
    assert_eq!(count, 1);
}

#[test]
fn histogram_repeated_byte() {
    let input = vec![0xAA; 100];
    let result = exec1(&HistogramFn, &input).unwrap();
    let offset = 0xAA * 8;
    let count = u64::from_be_bytes(result[offset..offset + 8].try_into().unwrap());
    assert_eq!(count, 100);
}

#[test]
fn histogram_counts_sum_to_input_length() {
    let input = b"hello world";
    let result = exec1(&HistogramFn, input).unwrap();
    let total: u64 = (0..256).map(|i| {
        u64::from_be_bytes(result[i * 8..(i + 1) * 8].try_into().unwrap())
    }).sum();
    assert_eq!(total, input.len() as u64);
}


// ── #46 EntropyFn ──

#[test]
fn entropy_empty() {
    assert_eq!(read_f64(&exec1(&EntropyFn, b"").unwrap()), 0.0);
}

#[test]
fn entropy_single_byte_repeated() {
    let input = vec![0x42; 1000];
    assert_eq!(read_f64(&exec1(&EntropyFn, &input).unwrap()), 0.0);
}

#[test]
fn entropy_two_equal_bytes() {
    // 50/50 distribution → entropy = 1.0 bit
    let mut input = vec![0u8; 100];
    input.extend(vec![1u8; 100]);
    let e = read_f64(&exec1(&EntropyFn, &input).unwrap());
    assert!((e - 1.0).abs() < 0.001);
}

#[test]
fn entropy_max_for_uniform() {
    // All 256 byte values equally → 8.0 bits
    let input: Vec<u8> = (0..=255).collect();
    let e = read_f64(&exec1(&EntropyFn, &input).unwrap());
    assert!((e - 8.0).abs() < 0.001);
}

#[test]
fn entropy_range() {
    let input = b"some typical english text with varied characters";
    let e = read_f64(&exec1(&EntropyFn, input).unwrap());
    assert!(e > 0.0 && e <= 8.0);
}


// ── #47 MinMaxFn ──

#[test]
fn min_max_single_byte() {
    assert_eq!(exec1(&MinMaxFn, &[0x42]).unwrap().as_ref(), &[0x42, 0x42]);
}

#[test]
fn min_max_range() {
    assert_eq!(exec1(&MinMaxFn, &[5, 1, 9, 3]).unwrap().as_ref(), &[1, 9]);
}

#[test]
fn min_max_full_range() {
    let input: Vec<u8> = (0..=255).collect();
    assert_eq!(exec1(&MinMaxFn, &input).unwrap().as_ref(), &[0x00, 0xFF]);
}

#[test]
fn min_max_empty_error() {
    let r = exec1(&MinMaxFn, b"");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn min_max_all_same() {
    assert_eq!(exec1(&MinMaxFn, &[0x77; 50]).unwrap().as_ref(), &[0x77, 0x77]);
}


// ── #48 SumFn ──

#[test]
fn sum_integers() {
    assert_eq!(exec1(&SumFn, b"1\n2\n3\n").unwrap().as_ref(), b"6");
}

#[test]
fn sum_floats() {
    let result = exec1(&SumFn, b"1.5\n2.5\n").unwrap();
    let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
    assert!((val - 4.0).abs() < 0.001);
}

#[test]
fn sum_empty() {
    assert_eq!(exec1(&SumFn, b"").unwrap().as_ref(), b"0");
}

#[test]
fn sum_blank_lines_skipped() {
    assert_eq!(exec1(&SumFn, b"10\n\n20\n").unwrap().as_ref(), b"30");
}

#[test]
fn sum_non_numeric_error() {
    let r = exec1(&SumFn, b"1\nabc\n3");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}


// ── #49 AverageFn ──

#[test]
fn average_integers() {
    let result = exec1(&AverageFn, b"2\n4\n6\n").unwrap();
    let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
    assert!((val - 4.0).abs() < 0.001);
}

#[test]
fn average_single_number() {
    let result = exec1(&AverageFn, b"42").unwrap();
    let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
    assert!((val - 42.0).abs() < 0.001);
}

#[test]
fn average_empty_error() {
    let r = exec1(&AverageFn, b"");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn average_blank_lines_only_error() {
    let r = exec1(&AverageFn, b"\n\n\n");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn average_non_numeric_error() {
    let r = exec1(&AverageFn, b"1\nfoo\n3");
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}


