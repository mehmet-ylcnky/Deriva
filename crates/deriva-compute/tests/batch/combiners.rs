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
fn zip_concat_length_prefix_format() {
    // Spec 9.3: 4 bytes BE for each input length, then data in order
    let a = Bytes::from("hello\nworld");
    let b = Bytes::from(" foo\n bar");
    let r = ZipConcatFn.execute(vec![a.clone(), b.clone()], &BTreeMap::new()).unwrap();
    // Header: len_a=11 (0x0000000B), len_b=9 (0x00000009), then data
    let expected_len = 4 + 4 + 11 + 9; // 28 bytes total
    assert_eq!(r.len(), expected_len);
    // Verify length prefix
    let len_a = u32::from_be_bytes([r[0], r[1], r[2], r[3]]);
    let len_b = u32::from_be_bytes([r[4], r[5], r[6], r[7]]);
    assert_eq!(len_a, 11);
    assert_eq!(len_b, 9);
    // Verify data
    assert_eq!(&r[8..8+11], b"hello\nworld");
    assert_eq!(&r[8+11..], b" foo\n bar");
}

#[test]
fn zip_concat_both_empty() {
    let r = ZipConcatFn.execute(vec![Bytes::new(), Bytes::new()], &BTreeMap::new()).unwrap();
    // 8 bytes of zeros (two 4-byte BE lengths of 0)
    assert_eq!(r.len(), 8);
    assert_eq!(&r[..], &[0, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn zip_concat_one_empty() {
    let r = ZipConcatFn.execute(vec![Bytes::from("line1\nline2"), Bytes::new()], &BTreeMap::new()).unwrap();
    let len_a = u32::from_be_bytes([r[0], r[1], r[2], r[3]]);
    let len_b = u32::from_be_bytes([r[4], r[5], r[6], r[7]]);
    assert_eq!(len_a, 11);
    assert_eq!(len_b, 0);
    assert_eq!(&r[8..], b"line1\nline2");
}

#[test]
fn zip_concat_wrong_input_count() {
    let r = ZipConcatFn.execute(vec![Bytes::from("a")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 1 })));
}


// ── #52 DiffFn + #53 PatchFn ──

#[test]
fn diff_patch_roundtrip() {
    let old = Bytes::from("hello world\nfoo bar\nbaz");
    let new_text = Bytes::from("hello world\nfoo baz\nbaz\nqux");
    let d = DiffFn.execute(vec![old.clone(), new_text.clone()], &BTreeMap::new()).unwrap();
    // The diff should be a unified diff (text format)
    let diff_str = std::str::from_utf8(&d).unwrap();
    assert!(diff_str.contains("---"));
    assert!(diff_str.contains("+++"));
    assert!(diff_str.contains("@@"));
    // Apply the patch
    let patched = PatchFn.execute(vec![old, d], &BTreeMap::new()).unwrap();
    assert_eq!(patched, new_text);
}

#[test]
fn diff_identical_inputs() {
    let data = Bytes::from("same data\nline two");
    let d = DiffFn.execute(vec![data.clone(), data.clone()], &BTreeMap::new()).unwrap();
    let diff_str = std::str::from_utf8(&d).unwrap();
    // Identical inputs should produce only headers, no hunks
    assert!(diff_str.contains("---"));
    assert!(!diff_str.contains("@@") || diff_str.is_empty());
    // Applying an empty-hunks diff should return original
    let patched = PatchFn.execute(vec![data.clone(), d], &BTreeMap::new()).unwrap();
    assert_eq!(patched, data);
}

#[test]
fn diff_empty_old() {
    let old = Bytes::new();
    let new_text = Bytes::from("new content");
    let d = DiffFn.execute(vec![old.clone(), new_text.clone()], &BTreeMap::new()).unwrap();
    let patched = PatchFn.execute(vec![old, d], &BTreeMap::new()).unwrap();
    assert_eq!(patched, new_text);
}

#[test]
fn diff_empty_new() {
    let old = Bytes::from("old content");
    let new_text = Bytes::new();
    let d = DiffFn.execute(vec![old.clone(), new_text.clone()], &BTreeMap::new()).unwrap();
    let patched = PatchFn.execute(vec![old, d], &BTreeMap::new()).unwrap();
    assert_eq!(patched, new_text);
}

#[test]
fn diff_both_empty() {
    let d = DiffFn.execute(vec![Bytes::new(), Bytes::new()], &BTreeMap::new()).unwrap();
    let diff_str = std::str::from_utf8(&d).unwrap();
    // Should produce headers but no hunks (or minimal output)
    assert!(!diff_str.contains("@@"));
}


// ── #53 PatchFn (additional) ──

#[test]
fn patch_invalid_diff_format() {
    let r = PatchFn.execute(vec![Bytes::from("base text"), Bytes::from("not a valid diff\nrandom content")], &BTreeMap::new());
    // If there are no @@ hunks, patch should return original unchanged
    let result = r.unwrap();
    assert_eq!(result, Bytes::from("base text"));
}

#[test]
fn patch_empty_patch() {
    let r = PatchFn.execute(vec![Bytes::from("base text"), Bytes::new()], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from("base text"));
}

#[test]
fn patch_multiline_roundtrip() {
    let old_text = "line1\nline2\nline3\nline4\nline5";
    let new_text = "line1\nmodified\nline3\nline4\nline5\nline6";
    let old = Bytes::from(old_text);
    let new_b = Bytes::from(new_text);
    let d = DiffFn.execute(vec![old.clone(), new_b.clone()], &BTreeMap::new()).unwrap();
    let patched = PatchFn.execute(vec![old, d], &BTreeMap::new()).unwrap();
    assert_eq!(patched, new_b);
}

#[test]
fn patch_wrong_input_count() {
    let r = PatchFn.execute(vec![Bytes::from("a")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 1 })));
}

#[test]
fn patch_context_mismatch() {
    // Create a diff for one file and try to apply to a different file
    let old = Bytes::from("alpha\nbeta\ngamma");
    let new_text = Bytes::from("alpha\ndelta\ngamma");
    let d = DiffFn.execute(vec![old.clone(), new_text.clone()], &BTreeMap::new()).unwrap();
    // Try applying to a completely different file
    let different = Bytes::from("x\ny\nz");
    let r = PatchFn.execute(vec![different, d], &BTreeMap::new());
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



// ── Spec 9.2: InterleaveBytesFn ──

#[test]
fn interleave_bytes_basic() {
    let r = InterleaveBytesFn.execute(
        vec![Bytes::from("abc"), Bytes::from("123")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"a1b2c3");
}

#[test]
fn interleave_bytes_unequal_lengths() {
    let r = InterleaveBytesFn.execute(
        vec![Bytes::from("abcde"), Bytes::from("12")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"a1b2cde");
}

#[test]
fn interleave_bytes_three_inputs() {
    let r = InterleaveBytesFn.execute(
        vec![Bytes::from("ab"), Bytes::from("12"), Bytes::from("XY")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"a1Xb2Y");
}

#[test]
fn interleave_bytes_single_input_error() {
    let r = InterleaveBytesFn.execute(vec![Bytes::from("abc")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 1 })));
}


// ── Spec 9.6: MergeFn ──

#[test]
fn merge_two_sorted() {
    let r = MergeFn.execute(
        vec![Bytes::from("apple\ncherry"), Bytes::from("banana\ndate")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"apple\nbanana\ncherry\ndate");
}

#[test]
fn merge_three_sorted() {
    let r = MergeFn.execute(
        vec![Bytes::from("b\ne"), Bytes::from("a\nd"), Bytes::from("c\nf")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc\nd\ne\nf");
}

#[test]
fn merge_single_input_error() {
    let r = MergeFn.execute(vec![Bytes::from("a\nb")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 1 })));
}


// ── Spec 9.7: SelectInputFn ──

#[test]
fn select_input_first() {
    let r = SelectInputFn.execute(
        vec![Bytes::from("first"), Bytes::from("second")],
        &params(&[("index", "0")]),
    ).unwrap();
    assert_eq!(r, Bytes::from("first"));
}

#[test]
fn select_input_second() {
    let r = SelectInputFn.execute(
        vec![Bytes::from("first"), Bytes::from("second")],
        &params(&[("index", "1")]),
    ).unwrap();
    assert_eq!(r, Bytes::from("second"));
}

#[test]
fn select_input_out_of_range() {
    let r = SelectInputFn.execute(
        vec![Bytes::from("a"), Bytes::from("b")],
        &params(&[("index", "5")]),
    );
    assert!(matches!(r, Err(ComputeError::ExecutionFailed(_))));
}

#[test]
fn select_input_missing_param() {
    let r = SelectInputFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn select_input_single_input_error() {
    let r = SelectInputFn.execute(vec![Bytes::from("only")], &params(&[("index", "0")]));
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 1 })));
}


// ── Spec 9.8: AlternateFn ──

#[test]
fn alternate_equal_lines() {
    let r = AlternateFn.execute(
        vec![Bytes::from("a\nb\nc"), Bytes::from("1\n2\n3")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"a\n1\nb\n2\nc\n3");
}

#[test]
fn alternate_unequal_lines() {
    let r = AlternateFn.execute(
        vec![Bytes::from("a\nb\nc"), Bytes::from("1")],
        &BTreeMap::new(),
    ).unwrap();
    assert_eq!(r.as_ref(), b"a\n1\nb\nc");
}

#[test]
fn alternate_one_empty() {
    let r = AlternateFn.execute(
        vec![Bytes::from("a\nb"), Bytes::new()],
        &BTreeMap::new(),
    ).unwrap();
    // Empty string .lines() produces no lines, so only lines from input 1
    assert_eq!(r.as_ref(), b"a\nb");
}

#[test]
fn alternate_wrong_input_count() {
    let r = AlternateFn.execute(vec![Bytes::from("a")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 1 })));
}

#[test]
fn alternate_three_inputs_error() {
    let r = AlternateFn.execute(
        vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")],
        &BTreeMap::new(),
    );
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 2, got: 3 })));
}
