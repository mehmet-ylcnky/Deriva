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




// ── #56 TakeFn ──

#[test]
fn take_first_5_bytes() {
    let r = exec1_params(&TakeFn, b"hello world", params(&[("bytes", "5")])).unwrap();
    assert_eq!(r.as_ref(), b"hello");
}

#[test]
fn take_more_than_input() {
    let r = exec1_params(&TakeFn, b"short", params(&[("bytes", "100")])).unwrap();
    assert_eq!(r.as_ref(), b"short");
}

#[test]
fn take_zero() {
    let r = exec1_params(&TakeFn, b"data", params(&[("bytes", "0")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn take_empty_input() {
    let r = exec1_params(&TakeFn, b"", params(&[("bytes", "5")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn take_exact_length() {
    let r = exec1_params(&TakeFn, b"abcd", params(&[("bytes", "4")])).unwrap();
    assert_eq!(r.as_ref(), b"abcd");
}


// ── #57 SkipFn ──

#[test]
fn skip_first_5_bytes() {
    let r = exec1_params(&SkipFn, b"hello world", params(&[("bytes", "5")])).unwrap();
    assert_eq!(r.as_ref(), b" world");
}

#[test]
fn skip_more_than_input() {
    let r = exec1_params(&SkipFn, b"short", params(&[("bytes", "100")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn skip_zero() {
    let r = exec1_params(&SkipFn, b"data", params(&[("bytes", "0")])).unwrap();
    assert_eq!(r.as_ref(), b"data");
}

#[test]
fn skip_empty_input() {
    let r = exec1_params(&SkipFn, b"", params(&[("bytes", "5")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn skip_exact_length() {
    let r = exec1_params(&SkipFn, b"abcd", params(&[("bytes", "4")])).unwrap();
    assert!(r.is_empty());
}


// ── #58 SliceFn ──

#[test]
fn slice_middle() {
    let r = exec1_params(&SliceFn, b"hello world", {
        let mut p = params(&[("offset", "6"), ("length", "5")]);
        p
    }).unwrap();
    assert_eq!(r.as_ref(), b"world");
}

#[test]
fn slice_past_end() {
    let r = exec1_params(&SliceFn, b"short", params(&[("offset", "100"), ("length", "5")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn slice_length_past_end() {
    let r = exec1_params(&SliceFn, b"hello", params(&[("offset", "3"), ("length", "100")])).unwrap();
    assert_eq!(r.as_ref(), b"lo");
}

#[test]
fn slice_zero_length() {
    let r = exec1_params(&SliceFn, b"data", params(&[("offset", "0"), ("length", "0")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn slice_full_input() {
    let r = exec1_params(&SliceFn, b"abcde", params(&[("offset", "0"), ("length", "5")])).unwrap();
    assert_eq!(r.as_ref(), b"abcde");
}


// ── #59 SortFn ──

#[test]
fn sort_lines() {
    let r = exec1(&SortFn, b"cherry\napple\nbanana").unwrap();
    assert_eq!(r.as_ref(), b"apple\nbanana\ncherry");
}

#[test]
fn sort_already_sorted() {
    let r = exec1(&SortFn, b"a\nb\nc").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn sort_single_line() {
    let r = exec1(&SortFn, b"only").unwrap();
    assert_eq!(r.as_ref(), b"only");
}

#[test]
fn sort_empty() {
    let r = exec1(&SortFn, b"").unwrap();
    assert_eq!(r.as_ref(), b"");
}

#[test]
fn sort_duplicates() {
    let r = exec1(&SortFn, b"b\na\nb\na").unwrap();
    assert_eq!(r.as_ref(), b"a\na\nb\nb");
}


// ── #60 UniqueFn ──

#[test]
fn unique_adjacent_dupes() {
    let r = exec1(&UniqueFn, b"a\na\nb\nb\nc").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn unique_no_dupes() {
    let r = exec1(&UniqueFn, b"a\nb\nc").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn unique_all_same() {
    let r = exec1(&UniqueFn, b"x\nx\nx").unwrap();
    assert_eq!(r.as_ref(), b"x");
}

#[test]
fn unique_non_adjacent_dupes_kept() {
    let r = exec1(&UniqueFn, b"a\nb\na").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\na");
}

#[test]
fn unique_empty() {
    let r = exec1(&UniqueFn, b"").unwrap();
    assert_eq!(r.as_ref(), b"");
}


// ── #61 SortUniqueFn ──

#[test]
fn sort_unique_basic() {
    let r = exec1(&SortUniqueFn, b"c\na\nb\na\nc").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn sort_unique_already_unique() {
    let r = exec1(&SortUniqueFn, b"b\na\nc").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn sort_unique_all_same() {
    let r = exec1(&SortUniqueFn, b"x\nx\nx").unwrap();
    assert_eq!(r.as_ref(), b"x");
}

#[test]
fn sort_unique_single() {
    let r = exec1(&SortUniqueFn, b"only").unwrap();
    assert_eq!(r.as_ref(), b"only");
}

#[test]
fn sort_unique_empty() {
    let r = exec1(&SortUniqueFn, b"").unwrap();
    assert_eq!(r.as_ref(), b"");
}


// ── #62 ShuffleFn ──

#[test]
fn shuffle_deterministic() {
    let input = b"a\nb\nc\nd\ne";
    let r1 = exec1_params(&ShuffleFn, input, params(&[("seed", "42")])).unwrap();
    let r2 = exec1_params(&ShuffleFn, input, params(&[("seed", "42")])).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn shuffle_different_seeds_differ() {
    let input = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
    let r1 = exec1_params(&ShuffleFn, input, params(&[("seed", "1")])).unwrap();
    let r2 = exec1_params(&ShuffleFn, input, params(&[("seed", "2")])).unwrap();
    assert_ne!(r1, r2);
}

#[test]
fn shuffle_preserves_all_lines() {
    let r = exec1_params(&ShuffleFn, b"x\ny\nz", params(&[("seed", "99")])).unwrap();
    let mut lines: Vec<&str> = std::str::from_utf8(&r).unwrap().lines().collect();
    lines.sort();
    assert_eq!(lines, vec!["x", "y", "z"]);
}

#[test]
fn shuffle_single_line() {
    let r = exec1_params(&ShuffleFn, b"only", params(&[("seed", "0")])).unwrap();
    assert_eq!(r.as_ref(), b"only");
}

#[test]
fn shuffle_empty() {
    let r = exec1_params(&ShuffleFn, b"", params(&[("seed", "0")])).unwrap();
    assert_eq!(r.as_ref(), b"");
}


// ── #63 HeadFn ──

#[test]
fn head_first_2_lines() {
    let r = exec1_params(&HeadFn, b"a\nb\nc\nd", params(&[("lines", "2")])).unwrap();
    assert_eq!(r.as_ref(), b"a\nb");
}

#[test]
fn head_more_than_available() {
    let r = exec1_params(&HeadFn, b"a\nb", params(&[("lines", "10")])).unwrap();
    assert_eq!(r.as_ref(), b"a\nb");
}

#[test]
fn head_one_line() {
    let r = exec1_params(&HeadFn, b"a\nb\nc", params(&[("lines", "1")])).unwrap();
    assert_eq!(r.as_ref(), b"a");
}

#[test]
fn head_empty() {
    let r = exec1_params(&HeadFn, b"", params(&[("lines", "5")])).unwrap();
    assert_eq!(r.as_ref(), b"");
}

#[test]
fn head_single_line_input() {
    let r = exec1_params(&HeadFn, b"only line", params(&[("lines", "1")])).unwrap();
    assert_eq!(r.as_ref(), b"only line");
}


// ── #64 TailFn ──

#[test]
fn tail_last_2_lines() {
    let r = exec1_params(&TailFn, b"a\nb\nc\nd", params(&[("lines", "2")])).unwrap();
    assert_eq!(r.as_ref(), b"c\nd");
}

#[test]
fn tail_more_than_available() {
    let r = exec1_params(&TailFn, b"a\nb", params(&[("lines", "10")])).unwrap();
    assert_eq!(r.as_ref(), b"a\nb");
}

#[test]
fn tail_one_line() {
    let r = exec1_params(&TailFn, b"a\nb\nc", params(&[("lines", "1")])).unwrap();
    assert_eq!(r.as_ref(), b"c");
}

#[test]
fn tail_empty() {
    let r = exec1_params(&TailFn, b"", params(&[("lines", "5")])).unwrap();
    assert_eq!(r.as_ref(), b"");
}

#[test]
fn tail_single_line_input() {
    let r = exec1_params(&TailFn, b"only line", params(&[("lines", "1")])).unwrap();
    assert_eq!(r.as_ref(), b"only line");
}


// ── #65 SampleFn ──

#[test]
fn sample_deterministic() {
    let input = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
    let p = params(&[("lines", "3"), ("seed", "42")]);
    let r1 = exec1_params(&SampleFn, input, p.clone()).unwrap();
    let r2 = exec1_params(&SampleFn, input, p).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn sample_correct_count() {
    let input = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
    let r = exec1_params(&SampleFn, input, params(&[("lines", "3"), ("seed", "7")])).unwrap();
    let count = std::str::from_utf8(&r).unwrap().lines().count();
    assert_eq!(count, 3);
}

#[test]
fn sample_n_exceeds_lines() {
    let r = exec1_params(&SampleFn, b"a\nb", params(&[("lines", "10"), ("seed", "0")])).unwrap();
    assert_eq!(r.as_ref(), b"a\nb");
}

#[test]
fn sample_preserves_order() {
    let input = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10";
    let r = exec1_params(&SampleFn, input, params(&[("lines", "4"), ("seed", "42")])).unwrap();
    let lines: Vec<i32> = std::str::from_utf8(&r).unwrap().lines().map(|l| l.parse().unwrap()).collect();
    let mut sorted = lines.clone();
    sorted.sort();
    assert_eq!(lines, sorted);
}

#[test]
fn sample_empty() {
    let r = exec1_params(&SampleFn, b"", params(&[("lines", "5"), ("seed", "0")])).unwrap();
    assert_eq!(r.as_ref(), b"");
}


