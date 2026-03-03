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

// ═══════════════════════════════════════════════════════════════════════
// #93 StreamingSum
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sum_basic() {
    let out = run_one(&StreamingSum, vec![b"1\n2\n3\n"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"6");
}

#[tokio::test]
async fn sum_floats() {
    let out = run_one(&StreamingSum, vec![b"1.5\n2.5\n"], &hp(&[])).await;
    let v: f64 = std::str::from_utf8(&out).unwrap().parse().unwrap();
    assert!((v - 4.0).abs() < 1e-9);
}

#[tokio::test]
async fn sum_multi_chunk() {
    let out = run_one(&StreamingSum, vec![b"10\n", b"20\n30\n"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"60");
}

#[tokio::test]
async fn sum_skips_blank_and_invalid() {
    let out = run_one(&StreamingSum, vec![b"5\n\nabc\n10\n"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"15");
}

#[tokio::test]
async fn sum_empty() {
    let out = run_one(&StreamingSum, vec![], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"0");
}

// ═══════════════════════════════════════════════════════════════════════
// #94 StreamingAverage
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn average_basic() {
    let out = run_one(&StreamingAverage, vec![b"2\n4\n6\n"], &hp(&[])).await;
    let v: f64 = std::str::from_utf8(&out).unwrap().parse().unwrap();
    assert!((v - 4.0).abs() < 1e-9);
}

#[tokio::test]
async fn average_single() {
    let out = run_one(&StreamingAverage, vec![b"42\n"], &hp(&[])).await;
    let v: f64 = std::str::from_utf8(&out).unwrap().parse().unwrap();
    assert!((v - 42.0).abs() < 1e-9);
}

#[tokio::test]
async fn average_multi_chunk() {
    let out = run_one(&StreamingAverage, vec![b"10\n", b"20\n"], &hp(&[])).await;
    let v: f64 = std::str::from_utf8(&out).unwrap().parse().unwrap();
    assert!((v - 15.0).abs() < 1e-9);
}

#[tokio::test]
async fn average_no_valid_numbers() {
    let err = run_one_err(&StreamingAverage, vec![b"abc\nxyz\n"], &hp(&[])).await;
    assert!(err.contains("no valid numbers"));
}

#[tokio::test]
async fn average_empty_error() {
    let err = run_one_err(&StreamingAverage, vec![], &hp(&[])).await;
    assert!(err.contains("no valid numbers"));
}

// ═══════════════════════════════════════════════════════════════════════
// #95 StreamingBitwiseAnd
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn bitwise_and_default_mask() {
    let out = run_one(&StreamingBitwiseAnd, vec![b"\xff\x0f"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"\xff\x0f"); // 0xFF mask = identity
}

#[tokio::test]
async fn bitwise_and_custom_mask() {
    let out = run_one(&StreamingBitwiseAnd, vec![b"\xff\x0f"], &hp(&[("mask", "15")])).await;
    assert_eq!(out.as_ref(), &[0x0f, 0x0f]);
}

#[tokio::test]
async fn bitwise_and_zero_mask() {
    let out = run_one(&StreamingBitwiseAnd, vec![b"\xff\xab"], &hp(&[("mask", "0")])).await;
    assert_eq!(out.as_ref(), &[0x00, 0x00]);
}

#[tokio::test]
async fn bitwise_and_multi_chunk() {
    let chunks = collect_chunks(&StreamingBitwiseAnd, vec![b"\xff", b"\x0f"], &hp(&[("mask", "15")])).await;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].as_ref(), &[0x0f]);
    assert_eq!(chunks[1].as_ref(), &[0x0f]);
}

#[tokio::test]
async fn bitwise_and_preserves_low_bits() {
    let out = run_one(&StreamingBitwiseAnd, vec![b"\xab"], &hp(&[("mask", "240")])).await;
    assert_eq!(out[0], 0xab & 0xf0);
}

// ═══════════════════════════════════════════════════════════════════════
// #96 StreamingBitwiseOr
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn bitwise_or_default_mask() {
    let out = run_one(&StreamingBitwiseOr, vec![b"\x0f"], &hp(&[])).await;
    assert_eq!(out.as_ref(), b"\x0f"); // 0x00 mask = identity
}

#[tokio::test]
async fn bitwise_or_custom_mask() {
    let out = run_one(&StreamingBitwiseOr, vec![b"\x00\xf0"], &hp(&[("mask", "15")])).await;
    assert_eq!(out.as_ref(), &[0x0f, 0xff]);
}

#[tokio::test]
async fn bitwise_or_all_ones() {
    let out = run_one(&StreamingBitwiseOr, vec![b"\x00"], &hp(&[("mask", "255")])).await;
    assert_eq!(out.as_ref(), &[0xff]);
}

#[tokio::test]
async fn bitwise_or_multi_chunk() {
    let chunks = collect_chunks(&StreamingBitwiseOr, vec![b"\x00", b"\x00"], &hp(&[("mask", "1")])).await;
    assert_eq!(chunks[0].as_ref(), &[0x01]);
    assert_eq!(chunks[1].as_ref(), &[0x01]);
}

#[tokio::test]
async fn bitwise_or_idempotent() {
    let out = run_one(&StreamingBitwiseOr, vec![b"\xff"], &hp(&[("mask", "255")])).await;
    assert_eq!(out.as_ref(), &[0xff]);
}

// ═══════════════════════════════════════════════════════════════════════
// #97 StreamingBitwiseNot
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn bitwise_not_basic() {
    let out = run_one(&StreamingBitwiseNot, vec![b"\x00\xff"], &hp(&[])).await;
    assert_eq!(out.as_ref(), &[0xff, 0x00]);
}

#[tokio::test]
async fn bitwise_not_double_inverse() {
    let data = b"\xab\xcd\xef";
    let first = run_one(&StreamingBitwiseNot, vec![data], &hp(&[])).await;
    let second = run_one(&StreamingBitwiseNot, vec![first.as_ref()], &hp(&[])).await;
    assert_eq!(second.as_ref(), data);
}

#[tokio::test]
async fn bitwise_not_multi_chunk() {
    let chunks = collect_chunks(&StreamingBitwiseNot, vec![b"\x0f", b"\xf0"], &hp(&[])).await;
    assert_eq!(chunks[0].as_ref(), &[0xf0]);
    assert_eq!(chunks[1].as_ref(), &[0x0f]);
}

#[tokio::test]
async fn bitwise_not_all_zeros() {
    let out = run_one(&StreamingBitwiseNot, vec![b"\x00\x00\x00"], &hp(&[])).await;
    assert!(out.iter().all(|&b| b == 0xff));
}

#[tokio::test]
async fn bitwise_not_single_byte() {
    let out = run_one(&StreamingBitwiseNot, vec![b"\xaa"], &hp(&[])).await;
    assert_eq!(out.as_ref(), &[0x55]);
}

// ═══════════════════════════════════════════════════════════════════════
// #98 StreamingByteSwap
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn byteswap_16bit() {
    let out = run_one(&StreamingByteSwap, vec![b"\x01\x02\x03\x04"], &hp(&[("word_size", "2")])).await;
    assert_eq!(out.as_ref(), &[0x02, 0x01, 0x04, 0x03]);
}

#[tokio::test]
async fn byteswap_32bit() {
    let out = run_one(&StreamingByteSwap, vec![b"\x01\x02\x03\x04"], &hp(&[("word_size", "4")])).await;
    assert_eq!(out.as_ref(), &[0x04, 0x03, 0x02, 0x01]);
}

#[tokio::test]
async fn byteswap_default_16() {
    let out = run_one(&StreamingByteSwap, vec![b"\xab\xcd"], &hp(&[])).await;
    assert_eq!(out.as_ref(), &[0xcd, 0xab]);
}

#[tokio::test]
async fn byteswap_not_divisible_error() {
    let err = run_one_err(&StreamingByteSwap, vec![b"\x01\x02\x03"], &hp(&[("word_size", "2")])).await;
    assert!(err.contains("not divisible"));
}

#[tokio::test]
async fn byteswap_double_inverse() {
    let data = b"\x01\x02\x03\x04";
    let first = run_one(&StreamingByteSwap, vec![data], &hp(&[("word_size", "4")])).await;
    let second = run_one(&StreamingByteSwap, vec![first.as_ref()], &hp(&[("word_size", "4")])).await;
    assert_eq!(second.as_ref(), data);
}

// ═══════════════════════════════════════════════════════════════════════
// #99 StreamingEntropy
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn entropy_uniform_byte() {
    // All same byte → entropy = 0.0
    let out = run_one(&StreamingEntropy, vec![&[0xAA; 100]], &hp(&[])).await;
    let h = f64::from_be_bytes(out[..8].try_into().unwrap());
    assert!((h - 0.0).abs() < 1e-9);
}

#[tokio::test]
async fn entropy_two_values() {
    // Equal distribution of 2 values → entropy = 1.0 bit
    let data: Vec<u8> = (0..100).map(|i| if i % 2 == 0 { 0 } else { 1 }).collect();
    let out = run_one(&StreamingEntropy, vec![&data], &hp(&[])).await;
    let h = f64::from_be_bytes(out[..8].try_into().unwrap());
    assert!((h - 1.0).abs() < 0.01);
}

#[tokio::test]
async fn entropy_output_8_bytes() {
    let out = run_one(&StreamingEntropy, vec![b"hello"], &hp(&[])).await;
    assert_eq!(out.len(), 8);
}

#[tokio::test]
async fn entropy_multi_chunk() {
    let out1 = run_one(&StreamingEntropy, vec![b"abcabc"], &hp(&[])).await;
    let out2 = run_one(&StreamingEntropy, vec![b"abc", b"abc"], &hp(&[])).await;
    assert_eq!(out1, out2);
}

#[tokio::test]
async fn entropy_high_for_random() {
    // 256 distinct bytes → entropy close to 8.0
    let data: Vec<u8> = (0..=255).collect();
    let out = run_one(&StreamingEntropy, vec![&data], &hp(&[])).await;
    let h = f64::from_be_bytes(out[..8].try_into().unwrap());
    assert!(h > 7.9);
}

// ═══════════════════════════════════════════════════════════════════════
// #100 StreamingRollingHash
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rolling_hash_appends_8_bytes() {
    let chunks = collect_chunks(&StreamingRollingHash, vec![b"hello"], &hp(&[])).await;
    assert_eq!(chunks[0].len(), 5 + 8);
}

#[tokio::test]
async fn rolling_hash_deterministic() {
    let c1 = collect_chunks(&StreamingRollingHash, vec![b"test"], &hp(&[])).await;
    let c2 = collect_chunks(&StreamingRollingHash, vec![b"test"], &hp(&[])).await;
    assert_eq!(c1[0], c2[0]);
}

#[tokio::test]
async fn rolling_hash_different_data() {
    let c1 = collect_chunks(&StreamingRollingHash, vec![b"aaa"], &hp(&[])).await;
    let c2 = collect_chunks(&StreamingRollingHash, vec![b"bbb"], &hp(&[])).await;
    assert_ne!(c1[0], c2[0]);
}

#[tokio::test]
async fn rolling_hash_per_chunk() {
    let chunks = collect_chunks(&StreamingRollingHash, vec![b"a", b"b"], &hp(&[])).await;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].len(), 1 + 8);
    assert_eq!(chunks[1].len(), 1 + 8);
}

#[tokio::test]
async fn rolling_hash_custom_window() {
    let c1 = collect_chunks(&StreamingRollingHash, vec![b"hello world"], &hp(&[("window_size", "4")])).await;
    assert_eq!(c1[0].len(), 11 + 8);
}
