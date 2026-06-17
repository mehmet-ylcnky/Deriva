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




// ── #66 ReplaceFn ──

#[test]
fn replace_basic() {
    let r = exec1_params(&ReplaceFn, b"hello world", params(&[("pattern", "world"), ("replacement", "rust")])).unwrap();
    assert_eq!(r.as_ref(), b"hello rust");
}

#[test]
fn replace_multiple_occurrences() {
    let r = exec1_params(&ReplaceFn, b"aabaa", params(&[("pattern", "a"), ("replacement", "x")])).unwrap();
    assert_eq!(r.as_ref(), b"xxbxx");
}

#[test]
fn replace_no_match() {
    let r = exec1_params(&ReplaceFn, b"hello", params(&[("pattern", "xyz"), ("replacement", "!")])).unwrap();
    assert_eq!(r.as_ref(), b"hello");
}

#[test]
fn replace_invalid_regex() {
    assert!(matches!(
        exec1_params(&ReplaceFn, b"hello", params(&[("pattern", "[bad"), ("replacement", "x")])),
        Err(ComputeError::InvalidParam(_))
    ));
}

#[test]
fn replace_empty_input() {
    let r = exec1_params(&ReplaceFn, b"", params(&[("pattern", "a"), ("replacement", "b")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn replace_capture_groups() {
    let r = exec1_params(&ReplaceFn, b"2024-01-15", params(&[("pattern", r"(\d{4})-(\d{2})-(\d{2})"), ("replacement", "$2/$3/$1")])).unwrap();
    assert_eq!(r.as_ref(), b"01/15/2024");
}

#[test]
fn replace_regex_special_chars() {
    let r = exec1_params(&ReplaceFn, b"foo123bar456", params(&[("pattern", r"\d+"), ("replacement", "NUM")])).unwrap();
    assert_eq!(r.as_ref(), b"fooNUMbarNUM");
}


// ── #67 RegexReplaceFn ──

#[test]
fn regex_replace_basic() {
    let r = exec1_params(&RegexReplaceFn, b"foo123bar", params(&[("pattern", r"\d+"), ("replacement", "NUM")])).unwrap();
    assert_eq!(r.as_ref(), b"fooNUMbar");
}

#[test]
fn regex_replace_capture_group() {
    let r = exec1_params(&RegexReplaceFn, b"2024-01-15", params(&[("pattern", r"(\d{4})-(\d{2})-(\d{2})"), ("replacement", "$2/$3/$1")])).unwrap();
    assert_eq!(r.as_ref(), b"01/15/2024");
}

#[test]
fn regex_replace_no_match() {
    let r = exec1_params(&RegexReplaceFn, b"hello", params(&[("pattern", r"\d+"), ("replacement", "X")])).unwrap();
    assert_eq!(r.as_ref(), b"hello");
}

#[test]
fn regex_replace_invalid_regex() {
    let r = exec1_params(&RegexReplaceFn, b"test", params(&[("pattern", "[invalid"), ("replacement", "x")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn regex_replace_multiple() {
    let r = exec1_params(&RegexReplaceFn, b"a1b2c3", params(&[("pattern", r"[0-9]"), ("replacement", "")])).unwrap();
    assert_eq!(r.as_ref(), b"abc");
}


// ── #68 GrepFn ──

#[test]
fn grep_matching_lines() {
    let r = exec1_params(&GrepFn, b"error: bad\ninfo: ok\nerror: fail", params(&[("pattern", "^error")])).unwrap();
    assert_eq!(r.as_ref(), b"error: bad\nerror: fail");
}

#[test]
fn grep_no_matches() {
    let r = exec1_params(&GrepFn, b"hello\nworld", params(&[("pattern", "xyz")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn grep_all_match() {
    let r = exec1_params(&GrepFn, b"aa\nab\nac", params(&[("pattern", "^a")])).unwrap();
    assert_eq!(r.as_ref(), b"aa\nab\nac");
}

#[test]
fn grep_invalid_regex() {
    let r = exec1_params(&GrepFn, b"test", params(&[("pattern", "[bad")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn grep_empty_input() {
    let r = exec1_params(&GrepFn, b"", params(&[("pattern", ".*")])).unwrap();
    assert_eq!(r.as_ref(), b"");
}


// ── #69 GrepInvertFn ──

#[test]
fn grep_invert_basic() {
    let r = exec1_params(&GrepInvertFn, b"error: bad\ninfo: ok\nerror: fail", params(&[("pattern", "^error")])).unwrap();
    assert_eq!(r.as_ref(), b"info: ok");
}

#[test]
fn grep_invert_no_matches_keeps_all() {
    let r = exec1_params(&GrepInvertFn, b"a\nb\nc", params(&[("pattern", "xyz")])).unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn grep_invert_all_match_empty() {
    let r = exec1_params(&GrepInvertFn, b"aa\nab\nac", params(&[("pattern", "^a")])).unwrap();
    assert!(r.is_empty());
}

#[test]
fn grep_invert_invalid_regex() {
    let r = exec1_params(&GrepInvertFn, b"test", params(&[("pattern", "[bad")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn grep_invert_empty_input() {
    let r = exec1_params(&GrepInvertFn, b"", params(&[("pattern", ".*")])).unwrap();
    assert_eq!(r.as_ref(), b"");
}


// ── #70 PrefixFn ──

#[test]
fn prefix_basic() {
    let r = exec1_params(&PrefixFn, b"world", params(&[("prefix", "hello ")])).unwrap();
    assert_eq!(r.as_ref(), b"hello world");
}

#[test]
fn prefix_empty_prefix() {
    let r = exec1_params(&PrefixFn, b"data", params(&[("prefix", "")])).unwrap();
    assert_eq!(r.as_ref(), b"data");
}

#[test]
fn prefix_empty_input() {
    let r = exec1_params(&PrefixFn, b"", params(&[("prefix", ">>")])).unwrap();
    assert_eq!(r.as_ref(), b">>");
}

#[test]
fn prefix_binary_safe() {
    let r = exec1_params(&PrefixFn, &[0xFF, 0xFE], params(&[("prefix", "BOM:")])).unwrap();
    assert_eq!(&r[..4], b"BOM:");
    assert_eq!(&r[4..], &[0xFF, 0xFE]);
}

#[test]
fn prefix_missing_param() {
    let r = exec1(&PrefixFn, b"data");
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}


// ── #71 SuffixFn ──

#[test]
fn suffix_basic() {
    let r = exec1_params(&SuffixFn, b"hello", params(&[("suffix", " world")])).unwrap();
    assert_eq!(r.as_ref(), b"hello world");
}

#[test]
fn suffix_empty_suffix() {
    let r = exec1_params(&SuffixFn, b"data", params(&[("suffix", "")])).unwrap();
    assert_eq!(r.as_ref(), b"data");
}

#[test]
fn suffix_empty_input() {
    let r = exec1_params(&SuffixFn, b"", params(&[("suffix", "END")])).unwrap();
    assert_eq!(r.as_ref(), b"END");
}

#[test]
fn suffix_newline() {
    let r = exec1_params(&SuffixFn, b"line", params(&[("suffix", "\n")])).unwrap();
    assert_eq!(r.as_ref(), b"line\n");
}

#[test]
fn suffix_missing_param() {
    let r = exec1(&SuffixFn, b"data");
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}


// ── #72 LinePrefixFn ──

#[test]
fn line_prefix_basic() {
    let r = exec1_params(&LinePrefixFn, b"a\nb\nc", params(&[("prefix", "> ")])).unwrap();
    assert_eq!(r.as_ref(), b"> a\n> b\n> c");
}

#[test]
fn line_prefix_empty_prefix() {
    let r = exec1_params(&LinePrefixFn, b"a\nb", params(&[("prefix", "")])).unwrap();
    assert_eq!(r.as_ref(), b"a\nb");
}

#[test]
fn line_prefix_single_line() {
    let r = exec1_params(&LinePrefixFn, b"hello", params(&[("prefix", "# ")])).unwrap();
    assert_eq!(r.as_ref(), b"# hello");
}

#[test]
fn line_prefix_empty_input() {
    let r = exec1_params(&LinePrefixFn, b"", params(&[("prefix", "> ")])).unwrap();
    assert_eq!(r.as_ref(), b"");
}

#[test]
fn line_prefix_tab_indent() {
    let r = exec1_params(&LinePrefixFn, b"x\ny", params(&[("prefix", "\t")])).unwrap();
    assert_eq!(r.as_ref(), b"\tx\n\ty");
}


// ── #73 LineNumberFn ──

#[test]
fn line_number_basic() {
    let r = exec1(&LineNumberFn, b"alpha\nbeta\ngamma").unwrap();
    assert_eq!(r.as_ref(), b"     1\talpha\n     2\tbeta\n     3\tgamma");
}

#[test]
fn line_number_single() {
    let r = exec1(&LineNumberFn, b"only").unwrap();
    assert_eq!(r.as_ref(), b"     1\tonly");
}

#[test]
fn line_number_empty() {
    let r = exec1(&LineNumberFn, b"").unwrap();
    assert_eq!(r.as_ref(), b"");
}

#[test]
fn line_number_starts_at_one() {
    let r = exec1(&LineNumberFn, b"a\nb").unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.starts_with("     1\t"));
}

#[test]
fn line_number_tab_separator() {
    let r = exec1(&LineNumberFn, b"test").unwrap();
    assert!(r.as_ref().contains(&b'\t'));
}


// ── #74 TruncateLinesFn ──

#[test]
fn truncate_lines_basic() {
    let r = exec1_params(&TruncateLinesFn, b"hello world\nhi", params(&[("max_line_bytes", "5")])).unwrap();
    assert_eq!(r.as_ref(), b"hello\nhi");
}

#[test]
fn truncate_lines_no_truncation() {
    let r = exec1_params(&TruncateLinesFn, b"ab\ncd", params(&[("max_line_bytes", "100")])).unwrap();
    assert_eq!(r.as_ref(), b"ab\ncd");
}

#[test]
fn truncate_lines_exact_length() {
    let r = exec1_params(&TruncateLinesFn, b"abcde", params(&[("max_line_bytes", "5")])).unwrap();
    assert_eq!(r.as_ref(), b"abcde");
}

#[test]
fn truncate_lines_empty() {
    let r = exec1_params(&TruncateLinesFn, b"", params(&[("max_line_bytes", "5")])).unwrap();
    assert_eq!(r.as_ref(), b"");
}

#[test]
fn truncate_lines_multiple() {
    let r = exec1_params(&TruncateLinesFn, b"abcdef\nghijkl\nmn", params(&[("max_line_bytes", "3")])).unwrap();
    assert_eq!(r.as_ref(), b"abc\nghi\nmn");
}


// ── #75 CharsetConvertFn ──

#[test]
fn charset_convert_latin1_to_utf8() {
    // ISO-8859-1: 0xE9 = é
    let r = exec1_params(&CharsetConvertFn, &[0xE9], params(&[("from", "iso-8859-1"), ("to", "utf-8")])).unwrap();
    assert_eq!(r.as_ref(), "é".as_bytes());
}

#[test]
fn charset_convert_utf8_to_latin1() {
    let r = exec1_params(&CharsetConvertFn, "é".as_bytes(), params(&[("from", "utf-8"), ("to", "iso-8859-1")])).unwrap();
    assert_eq!(r.as_ref(), &[0xE9]);
}

#[test]
fn charset_convert_utf8_roundtrip() {
    let input = "hello world".as_bytes();
    let r = exec1_params(&CharsetConvertFn, input, params(&[("from", "utf-8"), ("to", "utf-8")])).unwrap();
    assert_eq!(r.as_ref(), input);
}

#[test]
fn charset_convert_unknown_encoding() {
    let r = exec1_params(&CharsetConvertFn, b"test", params(&[("from", "bogus-999"), ("to", "utf-8")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn charset_convert_empty_input() {
    let r = exec1_params(&CharsetConvertFn, b"", params(&[("from", "utf-8"), ("to", "iso-8859-1")])).unwrap();
    assert!(r.is_empty());
}


// ── SplitFn ──

#[test]
fn split_basic() {
    let r = exec1_params(&SplitFn, b"a,b,c", params(&[("delimiter", ",")])).unwrap();
    assert_eq!(r.as_ref(), b"a\x00b\x00c");
}

#[test]
fn split_no_delimiter_found() {
    let r = exec1_params(&SplitFn, b"hello", params(&[("delimiter", ",")])).unwrap();
    assert_eq!(r.as_ref(), b"hello");
}

#[test]
fn split_empty_input() {
    let r = exec1_params(&SplitFn, b"", params(&[("delimiter", ",")])).unwrap();
    assert_eq!(r.as_ref(), b"");
}

#[test]
fn split_multi_char_delimiter() {
    let r = exec1_params(&SplitFn, b"a::b::c", params(&[("delimiter", "::")])).unwrap();
    assert_eq!(r.as_ref(), b"a\x00b\x00c");
}

#[test]
fn split_missing_param() {
    let r = exec1(&SplitFn, b"test");
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

#[test]
fn split_input_count_error() {
    let f = SplitFn;
    let r = f.execute(vec![], &params(&[("delimiter", ",")]));
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 1, got: 0 })));
}


// ── JoinFn ──

fn exec_multi_params(f: &dyn ComputeFunction, inputs: Vec<&[u8]>, p: BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    f.execute(inputs.iter().map(|b| Bytes::from(b.to_vec())).collect(), &p)
}

#[test]
fn join_basic() {
    let r = exec_multi_params(&JoinFn, vec![b"a", b"b", b"c"], params(&[("separator", ",")])).unwrap();
    assert_eq!(r.as_ref(), b"a,b,c");
}

#[test]
fn join_single_input() {
    let r = exec_multi_params(&JoinFn, vec![b"hello"], params(&[("separator", ",")])).unwrap();
    assert_eq!(r.as_ref(), b"hello");
}

#[test]
fn join_empty_separator() {
    let r = exec_multi_params(&JoinFn, vec![b"a", b"b", b"c"], params(&[("separator", "")])).unwrap();
    assert_eq!(r.as_ref(), b"abc");
}

#[test]
fn join_newline_separator() {
    let r = exec_multi_params(&JoinFn, vec![b"line1", b"line2"], params(&[("separator", "\n")])).unwrap();
    assert_eq!(r.as_ref(), b"line1\nline2");
}

#[test]
fn join_no_inputs_error() {
    let f = JoinFn;
    let r = f.execute(vec![], &params(&[("separator", ",")]));
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 1, got: 0 })));
}

#[test]
fn join_missing_param() {
    let r = exec_multi_params(&JoinFn, vec![b"a", b"b"], BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}


// ── SortLinesFn ──

#[test]
fn sort_lines_basic() {
    let r = exec1(&SortLinesFn, b"banana\napple\ncherry").unwrap();
    assert_eq!(r.as_ref(), b"apple\nbanana\ncherry");
}

#[test]
fn sort_lines_preserves_trailing_newline() {
    let r = exec1(&SortLinesFn, b"banana\napple\ncherry\n").unwrap();
    assert_eq!(r.as_ref(), b"apple\nbanana\ncherry\n");
}

#[test]
fn sort_lines_no_trailing_newline() {
    let r = exec1(&SortLinesFn, b"c\nb\na").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn sort_lines_already_sorted() {
    let r = exec1(&SortLinesFn, b"a\nb\nc").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn sort_lines_single_line() {
    let r = exec1(&SortLinesFn, b"hello").unwrap();
    assert_eq!(r.as_ref(), b"hello");
}

#[test]
fn sort_lines_empty() {
    let r = exec1(&SortLinesFn, b"").unwrap();
    assert_eq!(r.as_ref(), b"");
}

#[test]
fn sort_lines_input_count_error() {
    let f = SortLinesFn;
    let r = f.execute(vec![], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 1, got: 0 })));
}


// ── UniqueLinesFn ──

#[test]
fn unique_lines_basic() {
    let r = exec1(&UniqueLinesFn, b"a\na\nb\nb\nc").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn unique_lines_no_duplicates() {
    let r = exec1(&UniqueLinesFn, b"a\nb\nc").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\nc");
}

#[test]
fn unique_lines_non_consecutive_duplicates_kept() {
    let r = exec1(&UniqueLinesFn, b"a\nb\na").unwrap();
    assert_eq!(r.as_ref(), b"a\nb\na");
}

#[test]
fn unique_lines_all_same() {
    let r = exec1(&UniqueLinesFn, b"x\nx\nx\nx").unwrap();
    assert_eq!(r.as_ref(), b"x");
}

#[test]
fn unique_lines_empty() {
    let r = exec1(&UniqueLinesFn, b"").unwrap();
    assert_eq!(r.as_ref(), b"");
}

#[test]
fn unique_lines_input_count_error() {
    let f = UniqueLinesFn;
    let r = f.execute(vec![], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { expected: 1, got: 0 })));
}
