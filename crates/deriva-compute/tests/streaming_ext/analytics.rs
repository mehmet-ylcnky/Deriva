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

/// Collect output as separate chunks (not concatenated).
async fn collect_chunks(f: &dyn StreamingComputeFunction, chunks: Vec<&[u8]>, params: &HashMap<String, String>) -> Vec<Bytes> {
    let rx = make_stream(chunks).await;
    let mut out_rx = f.stream_execute(vec![rx], params).await;
    let mut result = Vec::new();
    loop {
        match out_rx.recv().await {
            Some(StreamChunk::Data(b)) => result.push(b),
            Some(StreamChunk::End) | None => break,
            Some(StreamChunk::Error(e)) => panic!("unexpected error: {e}"),
        }
    }
    result
}

fn read_u64(b: &[u8]) -> u64 {
    u64::from_be_bytes(b.try_into().unwrap())
}

// ═══════════════════════════════════════════════════════════════════════
// #40 StreamingFilter
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn filter_non_empty() {
    let chunks = collect_chunks(&StreamingFilter, vec![b"a", b"", b"b"], &hp(&[("predicate", "non_empty")])).await;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].as_ref(), b"a");
    assert_eq!(chunks[1].as_ref(), b"b");
}

#[tokio::test]
async fn filter_contains() {
    let chunks = collect_chunks(&StreamingFilter, vec![b"hello world", b"foo", b"world bar"], &hp(&[("predicate", "contains:world")])).await;
    assert_eq!(chunks.len(), 2);
}

#[tokio::test]
async fn filter_min_size() {
    let chunks = collect_chunks(&StreamingFilter, vec![b"ab", b"abcde", b"a"], &hp(&[("predicate", "min_size:3")])).await;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].as_ref(), b"abcde");
}

#[tokio::test]
async fn filter_default_non_empty() {
    let chunks = collect_chunks(&StreamingFilter, vec![b"", b"x"], &hp(&[])).await;
    assert_eq!(chunks.len(), 1);
}

#[tokio::test]
async fn filter_all_dropped() {
    let out = run_one(&StreamingFilter, vec![b"", b""], &hp(&[("predicate", "non_empty")])).await;
    assert_eq!(out.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// #41 StreamingLineCount
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn line_count_basic() {
    let out = run_one(&StreamingLineCount, vec![b"a\nb\nc\n"], &hp(&[])).await;
    assert_eq!(read_u64(&out), 3);
}

#[tokio::test]
async fn line_count_no_newlines() {
    let out = run_one(&StreamingLineCount, vec![b"hello"], &hp(&[])).await;
    assert_eq!(read_u64(&out), 0);
}

#[tokio::test]
async fn line_count_multi_chunk() {
    let out = run_one(&StreamingLineCount, vec![b"a\n", b"b\nc\n"], &hp(&[])).await;
    assert_eq!(read_u64(&out), 3);
}

#[tokio::test]
async fn line_count_empty() {
    let out = run_one(&StreamingLineCount, vec![b""], &hp(&[])).await;
    assert_eq!(read_u64(&out), 0);
}

#[tokio::test]
async fn line_count_output_is_8_bytes() {
    let out = run_one(&StreamingLineCount, vec![b"\n"], &hp(&[])).await;
    assert_eq!(out.len(), 8);
}

// ═══════════════════════════════════════════════════════════════════════
// #42 StreamingWordCount
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn word_count_basic() {
    let out = run_one(&StreamingWordCount, vec![b"hello world foo"], &hp(&[])).await;
    assert_eq!(read_u64(&out), 3);
}

#[tokio::test]
async fn word_count_multi_space() {
    let out = run_one(&StreamingWordCount, vec![b"  a   b  "], &hp(&[])).await;
    assert_eq!(read_u64(&out), 2);
}

#[tokio::test]
async fn word_count_split_word() {
    // "hel" + "lo world" — word spans chunks
    let out = run_one(&StreamingWordCount, vec![b"hel", b"lo world"], &hp(&[])).await;
    assert_eq!(read_u64(&out), 2);
}

#[tokio::test]
async fn word_count_empty() {
    let out = run_one(&StreamingWordCount, vec![b""], &hp(&[])).await;
    assert_eq!(read_u64(&out), 0);
}

#[tokio::test]
async fn word_count_newlines_tabs() {
    let out = run_one(&StreamingWordCount, vec![b"a\nb\tc\rd"], &hp(&[])).await;
    assert_eq!(read_u64(&out), 4);
}

// ═══════════════════════════════════════════════════════════════════════
// #43 StreamingMinMax
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn minmax_basic() {
    let out = run_one(&StreamingMinMax, vec![b"\x05\x01\xff\x80"], &hp(&[])).await;
    assert_eq!(out.as_ref(), &[0x01, 0xff]);
}

#[tokio::test]
async fn minmax_single_byte() {
    let out = run_one(&StreamingMinMax, vec![b"\x42"], &hp(&[])).await;
    assert_eq!(out.as_ref(), &[0x42, 0x42]);
}

#[tokio::test]
async fn minmax_multi_chunk() {
    let out = run_one(&StreamingMinMax, vec![b"\x10\x20", b"\x05\x30"], &hp(&[])).await;
    assert_eq!(out.as_ref(), &[0x05, 0x30]);
}

#[tokio::test]
async fn minmax_all_same() {
    let out = run_one(&StreamingMinMax, vec![b"\xaa\xaa\xaa"], &hp(&[])).await;
    assert_eq!(out.as_ref(), &[0xaa, 0xaa]);
}

#[tokio::test]
async fn minmax_output_is_2_bytes() {
    let out = run_one(&StreamingMinMax, vec![b"abc"], &hp(&[])).await;
    assert_eq!(out.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// #44 StreamingHistogram
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn histogram_output_size() {
    let out = run_one(&StreamingHistogram, vec![b"a"], &hp(&[])).await;
    assert_eq!(out.len(), 2048); // 256 × 8
}

#[tokio::test]
async fn histogram_single_byte() {
    let out = run_one(&StreamingHistogram, vec![b"aaa"], &hp(&[])).await;
    let offset = b'a' as usize * 8;
    let count = u64::from_be_bytes(out[offset..offset + 8].try_into().unwrap());
    assert_eq!(count, 3);
}

#[tokio::test]
async fn histogram_zero_for_absent() {
    let out = run_one(&StreamingHistogram, vec![b"a"], &hp(&[])).await;
    let offset = b'z' as usize * 8;
    let count = u64::from_be_bytes(out[offset..offset + 8].try_into().unwrap());
    assert_eq!(count, 0);
}

#[tokio::test]
async fn histogram_multi_chunk() {
    let out = run_one(&StreamingHistogram, vec![b"ab", b"ab"], &hp(&[])).await;
    let off_a = b'a' as usize * 8;
    let off_b = b'b' as usize * 8;
    assert_eq!(u64::from_be_bytes(out[off_a..off_a + 8].try_into().unwrap()), 2);
    assert_eq!(u64::from_be_bytes(out[off_b..off_b + 8].try_into().unwrap()), 2);
}

#[tokio::test]
async fn histogram_binary() {
    let out = run_one(&StreamingHistogram, vec![&[0u8, 0, 255]], &hp(&[])).await;
    let off_0 = 0;
    let off_255 = 255 * 8;
    assert_eq!(u64::from_be_bytes(out[off_0..off_0 + 8].try_into().unwrap()), 2);
    assert_eq!(u64::from_be_bytes(out[off_255..off_255 + 8].try_into().unwrap()), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// #45 StreamingSample
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sample_every_2nd() {
    let chunks = collect_chunks(&StreamingSample, vec![b"a", b"b", b"c", b"d"], &hp(&[("rate", "2")])).await;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].as_ref(), b"a"); // index 0
    assert_eq!(chunks[1].as_ref(), b"c"); // index 2
}

#[tokio::test]
async fn sample_every_1st() {
    let chunks = collect_chunks(&StreamingSample, vec![b"a", b"b", b"c"], &hp(&[("rate", "1")])).await;
    assert_eq!(chunks.len(), 3);
}

#[tokio::test]
async fn sample_default_10() {
    // 15 chunks, rate=10 → emit index 0 and 10
    let input: Vec<&[u8]> = (0..15).map(|_| &b"x"[..]).collect();
    let chunks = collect_chunks(&StreamingSample, input, &hp(&[])).await;
    assert_eq!(chunks.len(), 2);
}

#[tokio::test]
async fn sample_rate_zero_error() {
    let err = run_one_err(&StreamingSample, vec![b"x"], &hp(&[("rate", "0")])).await;
    assert!(err.contains("rate"));
}

#[tokio::test]
async fn sample_single_chunk() {
    let chunks = collect_chunks(&StreamingSample, vec![b"only"], &hp(&[("rate", "5")])).await;
    assert_eq!(chunks.len(), 1); // index 0 always emitted
}

// ═══════════════════════════════════════════════════════════════════════
// #46 StreamingHead
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn head_default_1() {
    let chunks = collect_chunks(&StreamingHead, vec![b"a", b"b", b"c"], &hp(&[])).await;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].as_ref(), b"a");
}

#[tokio::test]
async fn head_2_chunks() {
    let chunks = collect_chunks(&StreamingHead, vec![b"a", b"b", b"c"], &hp(&[("chunks", "2")])).await;
    assert_eq!(chunks.len(), 2);
}

#[tokio::test]
async fn head_more_than_available() {
    let chunks = collect_chunks(&StreamingHead, vec![b"a"], &hp(&[("chunks", "5")])).await;
    assert_eq!(chunks.len(), 1);
}

#[tokio::test]
async fn head_zero() {
    let chunks = collect_chunks(&StreamingHead, vec![b"a", b"b"], &hp(&[("chunks", "0")])).await;
    assert_eq!(chunks.len(), 0);
}

#[tokio::test]
async fn head_preserves_content() {
    let chunks = collect_chunks(&StreamingHead, vec![b"hello", b"world"], &hp(&[("chunks", "1")])).await;
    assert_eq!(chunks[0].as_ref(), b"hello");
}

// ═══════════════════════════════════════════════════════════════════════
// #47 StreamingTail
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn tail_default_1() {
    let chunks = collect_chunks(&StreamingTail, vec![b"a", b"b", b"c"], &hp(&[])).await;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].as_ref(), b"c");
}

#[tokio::test]
async fn tail_2_chunks() {
    let chunks = collect_chunks(&StreamingTail, vec![b"a", b"b", b"c"], &hp(&[("chunks", "2")])).await;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].as_ref(), b"b");
    assert_eq!(chunks[1].as_ref(), b"c");
}

#[tokio::test]
async fn tail_more_than_available() {
    let chunks = collect_chunks(&StreamingTail, vec![b"a", b"b"], &hp(&[("chunks", "5")])).await;
    assert_eq!(chunks.len(), 2);
}

#[tokio::test]
async fn tail_single_chunk() {
    let chunks = collect_chunks(&StreamingTail, vec![b"only"], &hp(&[("chunks", "1")])).await;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].as_ref(), b"only");
}

#[tokio::test]
async fn tail_preserves_order() {
    let chunks = collect_chunks(&StreamingTail, vec![b"1", b"2", b"3", b"4", b"5"], &hp(&[("chunks", "3")])).await;
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].as_ref(), b"3");
    assert_eq!(chunks[1].as_ref(), b"4");
    assert_eq!(chunks[2].as_ref(), b"5");
}

// ═══════════════════════════════════════════════════════════════════════
// #48 StreamingDeduplicate
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn dedup_removes_duplicates() {
    let chunks = collect_chunks(&StreamingDeduplicate, vec![b"a", b"b", b"a", b"c", b"b"], &hp(&[])).await;
    assert_eq!(chunks.len(), 3);
}

#[tokio::test]
async fn dedup_preserves_order() {
    let chunks = collect_chunks(&StreamingDeduplicate, vec![b"x", b"y", b"x"], &hp(&[])).await;
    assert_eq!(chunks[0].as_ref(), b"x");
    assert_eq!(chunks[1].as_ref(), b"y");
}

#[tokio::test]
async fn dedup_all_same() {
    let chunks = collect_chunks(&StreamingDeduplicate, vec![b"dup", b"dup", b"dup"], &hp(&[])).await;
    assert_eq!(chunks.len(), 1);
}

#[tokio::test]
async fn dedup_all_unique() {
    let chunks = collect_chunks(&StreamingDeduplicate, vec![b"a", b"b", b"c"], &hp(&[])).await;
    assert_eq!(chunks.len(), 3);
}

#[tokio::test]
async fn dedup_empty_chunks() {
    let chunks = collect_chunks(&StreamingDeduplicate, vec![b"", b"", b"a"], &hp(&[])).await;
    assert_eq!(chunks.len(), 2); // one empty + "a"
}

// ═══════════════════════════════════════════════════════════════════════
// #49 StreamingSort
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sort_basic() {
    let chunks = collect_chunks(&StreamingSort, vec![b"c", b"a", b"b"], &hp(&[])).await;
    assert_eq!(chunks[0].as_ref(), b"a");
    assert_eq!(chunks[1].as_ref(), b"b");
    assert_eq!(chunks[2].as_ref(), b"c");
}

#[tokio::test]
async fn sort_already_sorted() {
    let chunks = collect_chunks(&StreamingSort, vec![b"a", b"b", b"c"], &hp(&[])).await;
    assert_eq!(chunks[0].as_ref(), b"a");
}

#[tokio::test]
async fn sort_single() {
    let chunks = collect_chunks(&StreamingSort, vec![b"only"], &hp(&[])).await;
    assert_eq!(chunks.len(), 1);
}

#[tokio::test]
async fn sort_binary_order() {
    let chunks = collect_chunks(&StreamingSort, vec![&[0xffu8], &[0x00], &[0x80]], &hp(&[])).await;
    assert_eq!(chunks[0].as_ref(), &[0x00]);
    assert_eq!(chunks[1].as_ref(), &[0x80]);
    assert_eq!(chunks[2].as_ref(), &[0xff]);
}

#[tokio::test]
async fn sort_preserves_count() {
    let chunks = collect_chunks(&StreamingSort, vec![b"z", b"a", b"m", b"b", b"y"], &hp(&[])).await;
    assert_eq!(chunks.len(), 5);
}

// ═══════════════════════════════════════════════════════════════════════
// #50 StreamingUnique
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn unique_dedup_and_sort() {
    let chunks = collect_chunks(&StreamingUnique, vec![b"c", b"a", b"b", b"a", b"c"], &hp(&[])).await;
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].as_ref(), b"a");
    assert_eq!(chunks[1].as_ref(), b"b");
    assert_eq!(chunks[2].as_ref(), b"c");
}

#[tokio::test]
async fn unique_all_same() {
    let chunks = collect_chunks(&StreamingUnique, vec![b"x", b"x", b"x"], &hp(&[])).await;
    assert_eq!(chunks.len(), 1);
}

#[tokio::test]
async fn unique_already_unique_sorted() {
    let chunks = collect_chunks(&StreamingUnique, vec![b"a", b"b", b"c"], &hp(&[])).await;
    assert_eq!(chunks.len(), 3);
}

#[tokio::test]
async fn unique_single() {
    let chunks = collect_chunks(&StreamingUnique, vec![b"only"], &hp(&[])).await;
    assert_eq!(chunks.len(), 1);
}

#[tokio::test]
async fn unique_binary() {
    let chunks = collect_chunks(&StreamingUnique, vec![&[0xff], &[0x00], &[0xff], &[0x00]], &hp(&[])).await;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].as_ref(), &[0x00]);
    assert_eq!(chunks[1].as_ref(), &[0xff]);
}
