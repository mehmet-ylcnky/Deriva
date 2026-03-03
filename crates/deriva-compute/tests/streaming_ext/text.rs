use std::collections::HashMap;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use deriva_compute::builtins_streaming::*;
use deriva_compute::streaming::*;

fn hp(kv: &[(&str, &str)]) -> HashMap<String, String> {
    kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

async fn make_stream(chunks: Vec<&[u8]>) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(chunks.len() + 1);
    for c in chunks { tx.send(StreamChunk::Data(Bytes::copy_from_slice(c))).await.unwrap(); }
    tx.send(StreamChunk::End).await.unwrap();
    rx
}

async fn run_one(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> Bytes {
    let rx = make_stream(chunks).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

async fn run_one_err(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> String {
    let rx = make_stream(chunks).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap_err().to_string()
}

// ═══════════════════════════════════════════════════════════════════════
// #78 StreamingReplace
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn replace_literal() {
    let out = run_one(&StreamingReplace, vec![b"hello world\n"], &hp(&[("find", "world"), ("replace", "rust")])).await;
    assert_eq!(out.as_ref(), b"hello rust\n");
}

#[tokio::test]
async fn replace_regex() {
    let out = run_one(&StreamingReplace, vec![b"foo123bar456\n"], &hp(&[("find", "[0-9]+"), ("replace", "N")])).await;
    assert_eq!(out.as_ref(), b"fooNbarN\n");
}

#[tokio::test]
async fn replace_no_match() {
    let out = run_one(&StreamingReplace, vec![b"hello\n"], &hp(&[("find", "xyz"), ("replace", "abc")])).await;
    assert_eq!(out.as_ref(), b"hello\n");
}

#[tokio::test]
async fn replace_multiple_occurrences() {
    let out = run_one(&StreamingReplace, vec![b"aaa\n"], &hp(&[("find", "a"), ("replace", "bb")])).await;
    assert_eq!(out.as_ref(), b"bbbbbb\n");
}

#[tokio::test]
async fn replace_missing_param() {
    let err = run_one_err(&StreamingReplace, vec![b"x"], &hp(&[])).await;
    assert!(err.contains("missing param"));
}

// ═══════════════════════════════════════════════════════════════════════
// #79 StreamingPrefix
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn prefix_basic() {
    let out = run_one(&StreamingPrefix, vec![b"world"], &hp(&[("prefix", "hello ")])).await;
    assert_eq!(out.as_ref(), b"hello world");
}

#[tokio::test]
async fn prefix_multi_chunk() {
    let out = run_one(&StreamingPrefix, vec![b"a", b"b"], &hp(&[("prefix", ">")])).await;
    assert_eq!(out.as_ref(), b">ab");
}

#[tokio::test]
async fn prefix_empty_prefix() {
    let out = run_one(&StreamingPrefix, vec![b"data"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"data");
}

#[tokio::test]
async fn prefix_only_first_chunk() {
    let out = run_one(&StreamingPrefix, vec![b"1", b"2", b"3"], &hp(&[("prefix", "X")])).await;
    assert_eq!(out.as_ref(), b"X123");
}

#[tokio::test]
async fn prefix_empty_stream() {
    let out = run_one(&StreamingPrefix, vec![], &hp(&[("prefix", "X")])).await;
    assert_eq!(out.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// #80 StreamingSuffix
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn suffix_basic() {
    let out = run_one(&StreamingSuffix, vec![b"hello"], &hp(&[("suffix", " world")])).await;
    assert_eq!(out.as_ref(), b"hello world");
}

#[tokio::test]
async fn suffix_multi_chunk() {
    let out = run_one(&StreamingSuffix, vec![b"a", b"b"], &hp(&[("suffix", "!")])).await;
    assert_eq!(out.as_ref(), b"ab!");
}

#[tokio::test]
async fn suffix_empty_suffix() {
    let out = run_one(&StreamingSuffix, vec![b"data"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"data");
}

#[tokio::test]
async fn suffix_empty_stream() {
    let out = run_one(&StreamingSuffix, vec![], &hp(&[("suffix", "X")])).await;
    assert_eq!(out.as_ref(), b"X");
}

#[tokio::test]
async fn suffix_appended_before_end() {
    let out = run_one(&StreamingSuffix, vec![b"data\n"], &hp(&[("suffix", "EOF\n")])).await;
    assert_eq!(out.as_ref(), b"data\nEOF\n");
}

// ═══════════════════════════════════════════════════════════════════════
// #81 StreamingLinePrefix
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn line_prefix_basic() {
    let out = run_one(&StreamingLinePrefix, vec![b"a\nb\nc\n"], &hp(&[("prefix", "> ")])).await;
    assert_eq!(out.as_ref(), b"> a\n> b\n> c\n");
}

#[tokio::test]
async fn line_prefix_single_line() {
    let out = run_one(&StreamingLinePrefix, vec![b"hello"], &hp(&[("prefix", "# ")])).await;
    assert_eq!(out.as_ref(), b"# hello");
}

#[tokio::test]
async fn line_prefix_empty_prefix() {
    let out = run_one(&StreamingLinePrefix, vec![b"a\nb\n"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"a\nb\n");
}

#[tokio::test]
async fn line_prefix_empty_input() {
    let out = run_one(&StreamingLinePrefix, vec![], &hp(&[("prefix", "> ")])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn line_prefix_multi_chunk() {
    let out = run_one(&StreamingLinePrefix, vec![b"a\nb\n", b"c\n"], &hp(&[("prefix", "-")])).await;
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("-a\n"));
    assert!(s.contains("-c\n"));
}

// ═══════════════════════════════════════════════════════════════════════
// #82 StreamingGrep
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn grep_match() {
    let out = run_one(&StreamingGrep, vec![b"foo\nbar\nbaz\n"], &hp(&[("pattern", "ba")])).await;
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("bar"));
    assert!(s.contains("baz"));
    assert!(!s.contains("foo"));
}

#[tokio::test]
async fn grep_invert() {
    let out = run_one(&StreamingGrep, vec![b"foo\nbar\nbaz\n"], &hp(&[("pattern", "ba"), ("invert", "true")])).await;
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("foo"));
    assert!(!s.contains("bar"));
}

#[tokio::test]
async fn grep_no_match() {
    let out = run_one(&StreamingGrep, vec![b"aaa\nbbb\n"], &hp(&[("pattern", "xyz")])).await;
    assert_eq!(out.len(), 0);
}

#[tokio::test]
async fn grep_regex_pattern() {
    let out = run_one(&StreamingGrep, vec![b"abc123\ndef456\nghi\n"], &hp(&[("pattern", "[0-9]+")])).await;
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("abc123"));
    assert!(s.contains("def456"));
    assert!(!s.contains("ghi"));
}

#[tokio::test]
async fn grep_missing_param() {
    let err = run_one_err(&StreamingGrep, vec![b"x"], &hp(&[])).await;
    assert!(err.contains("missing param"));
}

// ═══════════════════════════════════════════════════════════════════════
// #83 StreamingSed
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sed_basic() {
    let out = run_one(&StreamingSed, vec![b"hello world\n"], &hp(&[("pattern", "world"), ("replacement", "rust")])).await;
    assert_eq!(out.as_ref(), b"hello rust\n");
}

#[tokio::test]
async fn sed_capture_group() {
    let out = run_one(&StreamingSed, vec![b"foo123\n"], &hp(&[("pattern", "(foo)(\\d+)"), ("replacement", "$1-$2")])).await;
    assert_eq!(out.as_ref(), b"foo-123\n");
}

#[tokio::test]
async fn sed_no_match() {
    let out = run_one(&StreamingSed, vec![b"hello\n"], &hp(&[("pattern", "xyz"), ("replacement", "abc")])).await;
    assert_eq!(out.as_ref(), b"hello\n");
}

#[tokio::test]
async fn sed_global_replace() {
    let out = run_one(&StreamingSed, vec![b"aaa\n"], &hp(&[("pattern", "a"), ("replacement", "b")])).await;
    assert_eq!(out.as_ref(), b"bbb\n");
}

#[tokio::test]
async fn sed_missing_param() {
    let err = run_one_err(&StreamingSed, vec![b"x"], &hp(&[])).await;
    assert!(err.contains("missing param"));
}

// ═══════════════════════════════════════════════════════════════════════
// #84 StreamingTruncateLines
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn truncate_lines_basic() {
    let out = run_one(&StreamingTruncateLines, vec![b"abcdef\n"], &hp(&[("max_line_bytes", "3")])).await;
    assert_eq!(out.as_ref(), b"abc\n");
}

#[tokio::test]
async fn truncate_lines_short_line() {
    let out = run_one(&StreamingTruncateLines, vec![b"ab\n"], &hp(&[("max_line_bytes", "10")])).await;
    assert_eq!(out.as_ref(), b"ab\n");
}

#[tokio::test]
async fn truncate_lines_multi() {
    let out = run_one(&StreamingTruncateLines, vec![b"abcdef\n123456\n"], &hp(&[("max_line_bytes", "4")])).await;
    assert_eq!(out.as_ref(), b"abcd\n1234\n");
}

#[tokio::test]
async fn truncate_lines_default_1024() {
    let line = "x".repeat(2000) + "\n";
    let out = run_one(&StreamingTruncateLines, vec![line.as_bytes()], &hp(&[])).await;
    let first_line = out.split(|&b| b == b'\n').next().unwrap();
    assert_eq!(first_line.len(), 1024);
}

#[tokio::test]
async fn truncate_lines_no_newline() {
    let out = run_one(&StreamingTruncateLines, vec![b"abcdef"], &hp(&[("max_line_bytes", "3")])).await;
    assert_eq!(out.as_ref(), b"abc");
}

// ═══════════════════════════════════════════════════════════════════════
// #85 StreamingCharsetConvert
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn charset_latin1_to_utf8() {
    // Latin1 0xe9 = é
    let out = run_one(&StreamingCharsetConvert, vec![b"caf\xe9\n"], &hp(&[("from", "latin1"), ("to", "utf8")])).await;
    assert_eq!(std::str::from_utf8(&out).unwrap(), "café\n");
}

#[tokio::test]
async fn charset_utf8_to_latin1() {
    let out = run_one(&StreamingCharsetConvert, vec!["café\n".as_bytes()], &hp(&[("from", "utf8"), ("to", "latin1")])).await;
    assert_eq!(out.as_ref(), b"caf\xe9\n");
}

#[tokio::test]
async fn charset_utf16le_to_utf8() {
    // "AB" in UTF-16LE = 41 00 42 00
    let out = run_one(&StreamingCharsetConvert, vec![b"\x41\x00\x42\x00\x0a\x00"], &hp(&[("from", "utf16le"), ("to", "utf8")])).await;
    assert_eq!(out.as_ref(), b"AB\n");
}

#[tokio::test]
async fn charset_unsupported_encoding() {
    let err = run_one_err(&StreamingCharsetConvert, vec![b"x\n"], &hp(&[("from", "ebcdic")])).await;
    assert!(err.contains("unsupported"));
}

#[tokio::test]
async fn charset_ascii_non_ascii_error() {
    let err = run_one_err(&StreamingCharsetConvert, vec![b"\x80\n"], &hp(&[("from", "ascii"), ("to", "utf8")])).await;
    assert!(err.contains("non-ascii"));
}
