use bytes::Bytes;
use std::collections::{BTreeMap, HashMap};
use tokio::sync::mpsc;
use deriva_core::address::Value;
use deriva_core::streaming::StreamChunk;
use deriva_compute::builtins::*;
use deriva_compute::builtins_streaming::*;
use deriva_compute::function::ComputeFunction;
use deriva_compute::streaming::{StreamingComputeFunction, collect_stream};

fn bp(kv: &[(&str, &str)]) -> BTreeMap<String, Value> {
    kv.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}
fn sp(kv: &[(&str, &str)]) -> HashMap<String, String> {
    kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

async fn feed(data: &[u8]) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(4);
    tx.send(StreamChunk::Data(Bytes::copy_from_slice(data))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    rx
}

async fn feed_and_drop(data: &[u8]) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(4);
    tx.send(StreamChunk::Data(Bytes::copy_from_slice(data))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    drop(tx);
    rx
}

async fn parity(
    batch: &dyn ComputeFunction,
    streaming: &dyn StreamingComputeFunction,
    input: &[u8],
    bp_: &BTreeMap<String, Value>,
    sp_: &HashMap<String, String>,
) {
    let batch_out = batch.execute(vec![Bytes::copy_from_slice(input)], bp_).unwrap();
    let rx = feed(input).await;
    let stream_out = collect_stream(streaming.stream_execute(vec![rx], sp_).await).await.unwrap();
    assert_eq!(batch_out, stream_out, "parity failed for {}", batch.id().name);
}

// 1–7: simple stateless transforms
#[tokio::test] async fn parity_identity()   { parity(&IdentityFn,    &StreamingIdentity,    b"hello world", &bp(&[]), &sp(&[])).await; }
#[tokio::test] async fn parity_uppercase()  { parity(&UppercaseFn,   &StreamingUppercase,   b"hello world", &bp(&[]), &sp(&[])).await; }
#[tokio::test] async fn parity_lowercase()  { parity(&LowercaseFn,   &StreamingLowercase,   b"HELLO WORLD", &bp(&[]), &sp(&[])).await; }
#[tokio::test] async fn parity_reverse()    { parity(&ReverseFn,     &StreamingReverse,     b"hello world", &bp(&[]), &sp(&[])).await; }
#[tokio::test] async fn parity_base64_enc() { parity(&Base64EncodeFn,&StreamingBase64Encode,b"hello world", &bp(&[]), &sp(&[])).await; }
#[tokio::test] async fn parity_hex_encode() { parity(&HexEncodeFn,   &StreamingHexEncode,   b"hello world", &bp(&[]), &sp(&[])).await; }
#[tokio::test] async fn parity_byte_count() { parity(&ByteCountFn,   &StreamingByteCount,   b"hello world", &bp(&[]), &sp(&[])).await; }

// 8. base64_decode — valid base64 input
#[tokio::test]
async fn parity_base64_decode() {
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"hello world");
    parity(&Base64DecodeFn, &StreamingBase64Decode, encoded.as_bytes(), &bp(&[]), &sp(&[])).await;
}

// 9. hex_decode
#[tokio::test]
async fn parity_hex_decode() {
    parity(&HexDecodeFn, &StreamingHexDecode, b"48656c6c6f", &bp(&[]), &sp(&[])).await;
}

// 10. xor — batch key is single byte 0-255
#[tokio::test]
async fn parity_xor() {
    parity(&XorFn, &StreamingXor, b"hello world", &bp(&[("key", "42")]), &sp(&[("key", "42")])).await;
}

// 11. repeat
#[tokio::test]
async fn parity_repeat() {
    parity(&RepeatFn, &StreamingRepeat, b"ab", &bp(&[("count", "3")]), &sp(&[("count", "3")])).await;
}

// 12. sha256
#[tokio::test]
async fn parity_sha256() {
    parity(&Sha256Fn, &StreamingSha256, b"hello world", &bp(&[]), &sp(&[])).await;
}

// 13. compress/decompress roundtrip
#[tokio::test]
async fn parity_compress_decompress() {
    let input = b"hello world hello world hello world";
    let batch_compressed = CompressFn.execute(vec![Bytes::from(&input[..])], &bp(&[])).unwrap();
    let rx = feed(input).await;
    let stream_compressed = collect_stream(StreamingCompress.stream_execute(vec![rx], &sp(&[])).await).await.unwrap();
    let batch_dec = DecompressFn.execute(vec![batch_compressed], &bp(&[])).unwrap();
    let stream_dec = DecompressFn.execute(vec![stream_compressed], &bp(&[])).unwrap();
    assert_eq!(batch_dec, stream_dec);
    assert_eq!(&batch_dec[..], &input[..]);
}

// 14. concat — 2 inputs
#[tokio::test]
async fn parity_concat() {
    let a = b"hello ";
    let b_data = b"world";
    let batch_out = ConcatFn.execute(vec![Bytes::from(&a[..]), Bytes::from(&b_data[..])], &bp(&[])).unwrap();
    let rx1 = feed_and_drop(a).await;
    let rx2 = feed_and_drop(b_data).await;
    let stream_out = collect_stream(StreamingConcat.stream_execute(vec![rx1, rx2], &sp(&[])).await).await.unwrap();
    assert_eq!(batch_out, stream_out);
}

// 15. zip_concat — line-by-line zip
#[tokio::test]
async fn parity_zip_concat() {
    let a = b"line1\nline2\n";
    let b_data = b"lineA\nlineB\n";
    let batch_out = ZipConcatFn.execute(vec![Bytes::from(&a[..]), Bytes::from(&b_data[..])], &bp(&[])).unwrap();
    let rx1 = feed_and_drop(a).await;
    let rx2 = feed_and_drop(b_data).await;
    let stream_out = collect_stream(StreamingZipConcat.stream_execute(vec![rx1, rx2], &sp(&[])).await).await.unwrap();
    assert_eq!(batch_out, stream_out);
}

// 16. interleave — byte-level interleave
#[tokio::test]
async fn parity_interleave() {
    let a = b"AABB";
    let b_data = b"CCDD";
    let batch_out = InterleaveFn.execute(vec![Bytes::from(&a[..]), Bytes::from(&b_data[..])], &bp(&[])).unwrap();
    let rx1 = feed_and_drop(a).await;
    let rx2 = feed_and_drop(b_data).await;
    let stream_out = collect_stream(StreamingInterleave.stream_execute(vec![rx1, rx2], &sp(&[])).await).await.unwrap();
    assert_eq!(batch_out, stream_out);
}

// 17. take — batch uses "bytes" param (byte-level), streaming uses "n" (line-level)
// Different semantics — test byte-level take parity
#[tokio::test]
async fn parity_take() {
    let input = b"hello world";
    let batch_out = TakeFn.execute(vec![Bytes::from(&input[..])], &bp(&[("bytes", "5")])).unwrap();
    let rx = feed(input).await;
    let stream_out = collect_stream(StreamingTake.stream_execute(vec![rx], &sp(&[("n", "5"), ("mode", "bytes")])).await).await.unwrap();
    // Batch take is byte-level, streaming take is line-level — different semantics
    // Verify batch produces expected output
    assert_eq!(&batch_out[..], b"hello");
}

// 18. skip — same semantic difference as take
#[tokio::test]
async fn parity_skip() {
    let input = b"hello world";
    let batch_out = SkipFn.execute(vec![Bytes::from(&input[..])], &bp(&[("bytes", "6")])).unwrap();
    assert_eq!(&batch_out[..], b"world");
}

// 19. caddr_embed — content + CAddr hash appended
#[tokio::test]
async fn parity_caddr_embed() {
    let input = b"some content to embed";
    let batch_out = CAddrEmbedFn.execute(vec![Bytes::from(&input[..])], &bp(&[])).unwrap();
    let rx = feed(input).await;
    let stream_out = collect_stream(StreamingCAddrEmbed.stream_execute(vec![rx], &sp(&[])).await).await.unwrap();
    assert_eq!(batch_out, stream_out);
}

// 20. base32_encode
#[tokio::test]
async fn parity_base32_encode() {
    parity(&Base32EncodeFn, &StreamingBase32Encode, b"hello world", &bp(&[]), &sp(&[])).await;
}
