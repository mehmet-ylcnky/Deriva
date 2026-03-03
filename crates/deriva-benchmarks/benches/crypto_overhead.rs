/// §7.3 Crypto Overhead — 10 performance benchmark scenarios
///
/// All 9 crypto functions: Encrypt, Decrypt, AeadEncrypt, AeadDecrypt,
/// HmacSha256, Md5, Sha512, Blake3, Redact, plus XOR cipher from core.
///
/// B1: AES-256-CTR encrypt vs decrypt throughput (1 MB)
/// B2: AES-256-GCM encrypt vs decrypt throughput (1 MB)
/// B3: AES-CTR vs AES-GCM overhead comparison
/// B4: XOR cipher throughput (baseline, 1 MB)
/// B5: All 5 cipher functions per-chunk latency (64 KB)
/// B6: Encrypt scaling (64 KB, 256 KB, 1 MB, 4 MB)
/// B7: Redact throughput — default patterns vs custom regex
/// B8: HMAC-SHA256 + MD5 + SHA-512 + BLAKE3 throughput (1 MB, crypto module)
/// B9: Compress→Encrypt pipeline cost (zstd-3 then AES-CTR)
/// B10: Full crypto suite comparison at 1 MB (all 9 functions)

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use bytes::Bytes;
use tokio::sync::mpsc;
use std::collections::HashMap;

use deriva_compute::builtins_streaming::*;
use deriva_compute::streaming::{StreamingComputeFunction, collect_stream};
use deriva_core::streaming::StreamChunk;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn hp(kv: &[(&str, &str)]) -> HashMap<String, String> {
    kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

async fn feed(data: &Bytes) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(2);
    let d = data.clone();
    tokio::spawn(async move {
        let _ = tx.send(StreamChunk::Data(d)).await;
        let _ = tx.send(StreamChunk::End).await;
    });
    rx
}

async fn run(f: &dyn StreamingComputeFunction, data: &Bytes, params: &HashMap<String, String>) -> Bytes {
    let rx = feed(data).await;
    collect_stream(f.stream_execute(vec![rx], params).await).await.unwrap()
}

fn make_text(size: usize) -> Bytes {
    let lines = [
        "The quick brown fox jumps over the lazy dog near the riverbank.\n",
        "Email: user@example.com, SSN: 123-45-6789, phone: 555-0100.\n",
        "Alice was beginning to get very tired of sitting by her sister.\n",
        "Credit card: 4111-1111-1111-1111, IP: 192.168.1.100 detected.\n",
    ];
    let mut buf = Vec::with_capacity(size);
    let mut i = 0;
    while buf.len() < size {
        buf.extend_from_slice(lines[i % lines.len()].as_bytes());
        i += 1;
    }
    buf.truncate(size);
    Bytes::from(buf)
}

fn make_data(size: usize) -> Bytes {
    Bytes::from(vec![0x41u8; size])
}

const KEY_HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const NONCE_CTR: &str = "00000000000000000000000000000001";
const NONCE_PREFIX: &str = "0000000000000001";
const HMAC_KEY: &str = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b";

fn ctr_params() -> HashMap<String, String> { hp(&[("key", KEY_HEX), ("nonce", NONCE_CTR)]) }
fn gcm_params() -> HashMap<String, String> { hp(&[("key", KEY_HEX), ("nonce_prefix", NONCE_PREFIX)]) }

// ── B1: AES-256-CTR encrypt vs decrypt (1 MB) ──────────────────────
fn b1_aes_ctr(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let p = ctr_params();
    let encrypted = rt().block_on(run(&StreamingEncrypt, &data, &p));
    let mut g = c.benchmark_group("B1_aes_ctr");
    g.sample_size(20);
    g.bench_function("encrypt", |b| b.iter(|| rt().block_on(run(&StreamingEncrypt, &data, &p))));
    g.bench_function("decrypt", |b| b.iter(|| rt().block_on(run(&StreamingDecrypt, &encrypted, &p))));
    g.finish();
}

// ── B2: AES-256-GCM encrypt vs decrypt (1 MB) ──────────────────────
fn b2_aes_gcm(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let p = gcm_params();
    let encrypted = rt().block_on(run(&StreamingAeadEncrypt, &data, &p));
    let mut g = c.benchmark_group("B2_aes_gcm");
    g.sample_size(20);
    g.bench_function("encrypt", |b| b.iter(|| rt().block_on(run(&StreamingAeadEncrypt, &data, &p))));
    g.bench_function("decrypt", |b| b.iter(|| rt().block_on(run(&StreamingAeadDecrypt, &encrypted, &p))));
    g.finish();
}

// ── B3: AES-CTR vs AES-GCM overhead ────────────────────────────────
fn b3_ctr_vs_gcm(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let ctr = ctr_params();
    let gcm = gcm_params();
    let mut g = c.benchmark_group("B3_ctr_vs_gcm");
    g.sample_size(20);
    g.bench_function("aes_ctr", |b| b.iter(|| rt().block_on(run(&StreamingEncrypt, &data, &ctr))));
    g.bench_function("aes_gcm", |b| b.iter(|| rt().block_on(run(&StreamingAeadEncrypt, &data, &gcm))));
    g.finish();
}

// ── B4: XOR cipher throughput (1 MB) ────────────────────────────────
fn b4_xor_cipher(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let p = hp(&[("key", "42")]);
    let mut g = c.benchmark_group("B4_xor_cipher");
    g.sample_size(20);
    g.bench_function("xor_1mb", |b| b.iter(|| rt().block_on(run(&StreamingXor, &data, &p))));
    g.finish();
}

// ── B5: All 5 ciphers per-chunk latency (64 KB) ────────────────────
fn b5_cipher_latency_64k(c: &mut Criterion) {
    let data = make_data(65536);
    let ctr = ctr_params();
    let gcm = gcm_params();
    let xor_p = hp(&[("key", "42")]);
    let ctr_enc = rt().block_on(run(&StreamingEncrypt, &data, &ctr));
    let gcm_enc = rt().block_on(run(&StreamingAeadEncrypt, &data, &gcm));
    let mut g = c.benchmark_group("B5_cipher_latency_64k");
    g.sample_size(50);
    g.bench_function("aes_ctr_enc", |b| b.iter(|| rt().block_on(run(&StreamingEncrypt, &data, &ctr))));
    g.bench_function("aes_ctr_dec", |b| b.iter(|| rt().block_on(run(&StreamingDecrypt, &ctr_enc, &ctr))));
    g.bench_function("aes_gcm_enc", |b| b.iter(|| rt().block_on(run(&StreamingAeadEncrypt, &data, &gcm))));
    g.bench_function("aes_gcm_dec", |b| b.iter(|| rt().block_on(run(&StreamingAeadDecrypt, &gcm_enc, &gcm))));
    g.bench_function("xor", |b| b.iter(|| rt().block_on(run(&StreamingXor, &data, &xor_p))));
    g.finish();
}

// ── B6: Encrypt scaling (64 KB–4 MB) ───────────────────────────────
fn b6_encrypt_scaling(c: &mut Criterion) {
    let ctr = ctr_params();
    let mut g = c.benchmark_group("B6_encrypt_scaling");
    g.sample_size(10);
    for size in [65536, 262144, 1_000_000, 4_000_000] {
        let data = make_data(size);
        g.bench_with_input(BenchmarkId::from_parameter(format!("{}KB", size / 1024)), &data, |b, d| {
            b.iter(|| rt().block_on(run(&StreamingEncrypt, d, &ctr)));
        });
    }
    g.finish();
}

// ── B7: Redact throughput — default vs custom regex ─────────────────
fn b7_redact(c: &mut Criterion) {
    let data = make_text(1_000_000);
    let mut g = c.benchmark_group("B7_redact");
    g.sample_size(20);
    g.bench_function("default_patterns", |b| {
        b.iter(|| rt().block_on(run(&StreamingRedact, &data, &hp(&[]))));
    });
    g.bench_function("custom_regex", |b| {
        b.iter(|| rt().block_on(run(&StreamingRedact, &data, &hp(&[("patterns", r"\b\d{3}-\d{2}-\d{4}\b,\b4\d{3}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}\b")]))));
    });
    g.finish();
}

// ── B8: Hash functions from crypto module (1 MB) ────────────────────
fn b8_crypto_hashes(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let empty = hp(&[]);
    let hmac_p = hp(&[("key", HMAC_KEY)]);
    let mut g = c.benchmark_group("B8_crypto_hashes");
    g.sample_size(20);
    g.bench_function("hmac_sha256", |b| b.iter(|| rt().block_on(run(&StreamingHmacSha256, &data, &hmac_p))));
    g.bench_function("md5", |b| b.iter(|| rt().block_on(run(&StreamingMd5, &data, &empty))));
    g.bench_function("sha512", |b| b.iter(|| rt().block_on(run(&StreamingSha512, &data, &empty))));
    g.bench_function("blake3", |b| b.iter(|| rt().block_on(run(&StreamingBlake3, &data, &empty))));
    g.finish();
}

// ── B9: Compress→Encrypt pipeline cost ──────────────────────────────
fn b9_compress_then_encrypt(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let zstd_p = hp(&[("level", "3")]);
    let ctr = ctr_params();
    let mut g = c.benchmark_group("B9_compress_encrypt_pipeline");
    g.sample_size(10);
    g.bench_function("zstd3_only", |b| {
        b.iter(|| rt().block_on(run(&StreamingZstdCompress, &data, &zstd_p)));
    });
    g.bench_function("aes_ctr_only", |b| {
        b.iter(|| rt().block_on(run(&StreamingEncrypt, &data, &ctr)));
    });
    g.bench_function("zstd3_then_aes_ctr", |b| {
        b.iter(|| rt().block_on(async {
            let compressed = run(&StreamingZstdCompress, &data, &zstd_p).await;
            run(&StreamingEncrypt, &compressed, &ctr).await
        }));
    });
    g.finish();
}

// ── B10: Full crypto suite at 1 MB (all 9 functions) ────────────────
fn b10_full_suite(c: &mut Criterion) {
    let data = make_data(1_000_000);
    let text = make_text(1_000_000);
    let ctr = ctr_params();
    let gcm = gcm_params();
    let xor_p = hp(&[("key", "42")]);
    let hmac_p = hp(&[("key", HMAC_KEY)]);
    let empty = hp(&[]);
    let ctr_enc = rt().block_on(run(&StreamingEncrypt, &data, &ctr));
    let gcm_enc = rt().block_on(run(&StreamingAeadEncrypt, &data, &gcm));

    let mut g = c.benchmark_group("B10_full_crypto_suite_1mb");
    g.sample_size(10);
    g.bench_function("encrypt_ctr", |b| b.iter(|| rt().block_on(run(&StreamingEncrypt, &data, &ctr))));
    g.bench_function("decrypt_ctr", |b| b.iter(|| rt().block_on(run(&StreamingDecrypt, &ctr_enc, &ctr))));
    g.bench_function("encrypt_gcm", |b| b.iter(|| rt().block_on(run(&StreamingAeadEncrypt, &data, &gcm))));
    g.bench_function("decrypt_gcm", |b| b.iter(|| rt().block_on(run(&StreamingAeadDecrypt, &gcm_enc, &gcm))));
    g.bench_function("xor", |b| b.iter(|| rt().block_on(run(&StreamingXor, &data, &xor_p))));
    g.bench_function("hmac_sha256", |b| b.iter(|| rt().block_on(run(&StreamingHmacSha256, &data, &hmac_p))));
    g.bench_function("md5", |b| b.iter(|| rt().block_on(run(&StreamingMd5, &data, &empty))));
    g.bench_function("sha512", |b| b.iter(|| rt().block_on(run(&StreamingSha512, &data, &empty))));
    g.bench_function("blake3", |b| b.iter(|| rt().block_on(run(&StreamingBlake3, &data, &empty))));
    g.bench_function("redact", |b| b.iter(|| rt().block_on(run(&StreamingRedact, &text, &empty))));
    g.finish();
}

criterion_group!(
    benches,
    b1_aes_ctr,
    b2_aes_gcm,
    b3_ctr_vs_gcm,
    b4_xor_cipher,
    b5_cipher_latency_64k,
    b6_encrypt_scaling,
    b7_redact,
    b8_crypto_hashes,
    b9_compress_then_encrypt,
    b10_full_suite,
);
criterion_main!(benches);
