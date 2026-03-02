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




// ── #50 InterleaveFn ──

#[test]
fn interleave_byte_level() {
    let r = InterleaveFn.execute(vec![Bytes::from("abc"), Bytes::from("123")], &BTreeMap::new()).unwrap();
    assert_eq!(r.as_ref(), b"a1b2c3");
}

#[test]
fn interleave_block_size_2() {
    let r = InterleaveFn.execute(
        vec![Bytes::from("aabb"), Bytes::from("1122")],
        &params(&[("block_size", "2")]),
    ).unwrap();
    assert_eq!(r.as_ref(), b"aa11bb22");
}

#[test]
fn interleave_unequal_lengths() {
    let r = InterleaveFn.execute(vec![Bytes::from("abcde"), Bytes::from("12")], &BTreeMap::new()).unwrap();
    assert_eq!(r.as_ref(), b"a1b2cde");
}

#[test]
fn interleave_three_inputs() {
    let r = InterleaveFn.execute(
        vec![Bytes::from("ab"), Bytes::from("12"), Bytes::from("XY")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"a1Xb2Y");
}

#[test]
fn interleave_block_size_zero_error() {
    let r = InterleaveFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &params(&[("block_size", "0")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}


// ── #51 ZipConcatFn ──

#[test]
fn zip_concat_equal_lines() {
    let r = ZipConcatFn.execute(vec![Bytes::from("hello\nworld"), Bytes::from(" foo\n bar")], &BTreeMap::new()).unwrap();
    assert_eq!(r.as_ref(), b"hello foo\nworld bar");
}

#[test]
fn zip_concat_unequal_lines() {
    let r = ZipConcatFn.execute(vec![Bytes::from("a\nb\nc"), Bytes::from("1")], &BTreeMap::new()).unwrap();
    assert_eq!(r.as_ref(), b"a1\nb\nc");
}

#[test]
fn zip_concat_both_empty() {
    let r = ZipConcatFn.execute(vec![Bytes::new(), Bytes::new()], &BTreeMap::new()).unwrap();
    assert_eq!(r.as_ref(), b"");
}

#[test]
fn zip_concat_one_empty() {
    let r = ZipConcatFn.execute(vec![Bytes::from("line1\nline2"), Bytes::new()], &BTreeMap::new()).unwrap();
    assert_eq!(r.as_ref(), b"line1\nline2");
}

#[test]
fn zip_concat_wrong_input_count() {
    let r = ZipConcatFn.execute(vec![Bytes::from("a")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 1 })));
}


// ── #52 DiffFn + #53 PatchFn ──

#[test]
fn diff_patch_roundtrip() {
    let old = Bytes::from("hello world");
    let new = Bytes::from("hello rust!");
    let d = DiffFn.execute(vec![old.clone(), new.clone()], &BTreeMap::new()).unwrap();
    let patched = PatchFn.execute(vec![old, d], &BTreeMap::new()).unwrap();
    assert_eq!(patched, new);
}

#[test]
fn diff_identical_inputs() {
    let data = Bytes::from("same data");
    let d = DiffFn.execute(vec![data.clone(), data.clone()], &BTreeMap::new()).unwrap();
    let patched = PatchFn.execute(vec![data.clone(), d], &BTreeMap::new()).unwrap();
    assert_eq!(patched, data);
}

#[test]
fn diff_empty_old() {
    let old = Bytes::new();
    let new = Bytes::from("new content");
    let d = DiffFn.execute(vec![old.clone(), new.clone()], &BTreeMap::new()).unwrap();
    let patched = PatchFn.execute(vec![old, d], &BTreeMap::new()).unwrap();
    assert_eq!(patched, new);
}

#[test]
fn diff_empty_new() {
    let old = Bytes::from("old content");
    let new = Bytes::new();
    let d = DiffFn.execute(vec![old.clone(), new.clone()], &BTreeMap::new()).unwrap();
    let patched = PatchFn.execute(vec![old, d], &BTreeMap::new()).unwrap();
    assert_eq!(patched, new);
}

#[test]
fn diff_both_empty() {
    let d = DiffFn.execute(vec![Bytes::new(), Bytes::new()], &BTreeMap::new()).unwrap();
    assert!(d.is_empty());
}


// ── #53 PatchFn (additional) ──

#[test]
fn patch_corrupt_data() {
    let r = PatchFn.execute(vec![Bytes::from("base"), Bytes::from(vec![0xFF])], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn patch_empty_patch() {
    let r = PatchFn.execute(vec![Bytes::from("base"), Bytes::new()], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}

#[test]
fn patch_binary_roundtrip() {
    let old: Vec<u8> = (0..100).collect();
    let new: Vec<u8> = (50..150).collect();
    let d = DiffFn.execute(vec![Bytes::from(old.clone()), Bytes::from(new.clone())], &BTreeMap::new()).unwrap();
    let patched = PatchFn.execute(vec![Bytes::from(old), d], &BTreeMap::new()).unwrap();
    assert_eq!(patched.as_ref(), new.as_slice());
}

#[test]
fn patch_wrong_input_count() {
    let r = PatchFn.execute(vec![Bytes::from("a")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 1 })));
}

#[test]
fn patch_truncated() {
    // op byte but no length
    let r = PatchFn.execute(vec![Bytes::from("base"), Bytes::from(vec![0x00, 0x00])], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}


// ── #54 MergeSortedFn ──

#[test]
fn merge_sorted_two_inputs() {
    let r = MergeSortedFn.execute(
        vec![Bytes::from("apple\ncherry"), Bytes::from("banana\ndate")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"apple\nbanana\ncherry\ndate");
}

#[test]
fn merge_sorted_one_empty() {
    let r = MergeSortedFn.execute(
        vec![Bytes::from("a\nb\nc"), Bytes::new()],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn merge_sorted_three_inputs() {
    let r = MergeSortedFn.execute(
        vec![Bytes::from("b\ne"), Bytes::from("a\nd"), Bytes::from("c\nf")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc\nd\ne\nf");
}

#[test]
fn merge_sorted_duplicates() {
    let r = MergeSortedFn.execute(
        vec![Bytes::from("a\na"), Bytes::from("a\nb")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"a\na\na\nb");
}

#[test]
fn merge_sorted_empty_inputs() {
    let r = MergeSortedFn.execute(vec![Bytes::new(), Bytes::new()], &BTreeMap::new()).unwrap();
    assert_eq!(r.as_ref(), b"");
}


// ── #55 SelectFn ──

#[test]
fn select_first() {
    let r = SelectFn.execute(
        vec![Bytes::from("first"), Bytes::from("second")],
        &params(&[("index", "0")]),
    ).unwrap();
    assert_eq!(r, Bytes::from("first"));
}

#[test]
fn select_second() {
    let r = SelectFn.execute(
        vec![Bytes::from("first"), Bytes::from("second")],
        &params(&[("index", "1")]),
    ).unwrap();
    assert_eq!(r, Bytes::from("second"));
}

#[test]
fn select_out_of_range() {
    let r = SelectFn.execute(vec![Bytes::from("only")], &params(&[("index", "5")]));
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn select_missing_param() {
    let r = SelectFn.execute(vec![Bytes::from("data")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn select_single_input() {
    let r = SelectFn.execute(vec![Bytes::from("only")], &params(&[("index", "0")])).unwrap();
    assert_eq!(r, Bytes::from("only"));
}


