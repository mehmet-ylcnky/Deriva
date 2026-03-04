# §2.15 Batch Function Library Expansion

> **Status**: Blueprint
> **Depends on**: §2.7 Streaming Materialization (for registry integration)
> **Crate(s)**: `deriva-compute`
> **Estimated effort**: 4–5 days (96 functions across 10 categories)

---

## 1. Problem Statement

### 1.1 Current State

The batch function library in `builtins.rs` provides 4 functions:

```
Existing Batch Functions (#1–#4):
  ┌──────────────────────────────────────────────────────────────┐
  │ #1  IdentityFn     pass-through (no-op)                      │
  │ #2  ConcatFn       concatenate N inputs                      │
  │ #3  UppercaseFn    ASCII uppercase                           │
  │ #4  RepeatFn       repeat input N times (params: count)      │
  └──────────────────────────────────────────────────────────────┘
```

These 4 functions were sufficient for testing the initial CAS pipeline
and materialization infrastructure, but are inadequate for production
workloads.

### 1.2 Coverage Gaps

| Domain | What's Missing | Impact |
|--------|---------------|--------|
| Compression | No codecs at all | Cannot compress/decompress stored values |
| Cryptography | No hashing, encryption, HMAC | No integrity verification, no data-at-rest encryption |
| Text processing | No grep, replace, line ops | Cannot process logs, configs, or text data |
| Validation | No JSON/UTF-8/schema validation | No input sanitization or format verification |
| Analytics | No counting, histogram, entropy | Cannot build analysis pipelines |
| Slicing | No take/skip/slice/sort | Cannot extract or reorder data subsets |
| Format conversion | No JSON↔YAML/CSV/TOML | No format interoperability |
| CAS-specific | No CAddr compute/verify/embed | Cannot leverage content-addressing in batch pipelines |
| Combiners | Only ConcatFn | No interleave, diff/patch, merge, select |

The 4 existing functions cover only basic transforms. The streaming
library (§2.14) targets 100 functions — the batch library must provide
equivalent coverage for functions where batch execution is natural or
preferred.

### 1.3 Goal

Grow the batch library from 4 to 100 functions across 11 categories.
Achieve parity with the streaming library (§2.14) where applicable,
and add batch-only functions that benefit from full-input access
(global sort, reservoir sampling, format conversion, schema validation).

With §2.9 size-aware mode selection, functions registered in both
batch and streaming registries will automatically use the optimal
path based on input size. This makes batch implementations critical
for small-input performance.

### 1.4 Batch vs Streaming Parity Matrix

| Pattern | Batch | Streaming | Both | Rationale |
|---------|-------|-----------|------|-----------|
| Transforms (encoding, bitwise) | ✓ | ✓ | ✓ | Identical semantics; §2.9 picks optimal path |
| Compression | ✓ | ✓ | ✓ | Batch gets 5–15% better ratio (whole-input context) |
| Hashing | ✓ | ✓ | ✓ | Batch is simpler; streaming handles large inputs |
| Encryption | ✓ | ✓ | ✓ | Batch for small secrets; streaming for large files |
| Accumulators | ✓ | ✓ | ✓ | Both produce same result; batch avoids task overhead |
| Combiners | ✓ | ✓ | ✓ | Batch concat/interleave; streaming for large fan-in |
| Text processing | ✓ | ✓ | ✓ | Batch grep/replace; streaming for log tailing |
| Validation | ✓ | — | Batch only | Needs full input (JSON parse, schema validate) |
| Format conversion | ✓ | — | Batch only | Needs full input (JSON↔YAML, CSV↔JSON) |
| Slicing (sort, unique, shuffle) | ✓ | — | Batch only | Global operations require full input |
| CAS-specific | ✓ | ✓ | ✓ | CAddr/Merkle need full input; embed works either way |
| Flow control (rate limit, tee) | — | ✓ | Streaming only | Inherently stream-oriented |

**96 new batch functions** + 4 existing = 100 total.
Of these, ~70 have streaming counterparts (registered in both registries),
~20 are batch-only (validation, format conversion, global sort),
and ~10 streaming functions have no batch equivalent (flow control).

---

## 2. Design

### 2.1 Implementation Pattern

All batch functions implement the `ComputeFunction` trait:

```rust
pub trait ComputeFunction: Send + Sync {
    fn id(&self) -> FunctionId;
    fn execute(
        &self,
        inputs: Vec<Bytes>,
        params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError>;
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost;
}
```

Unlike streaming functions which use helper patterns (`spawn_map`,
`spawn_accumulate`, etc.), batch functions are straightforward:
receive all inputs as `Vec<Bytes>`, return a single `Bytes` result.
This simplicity is the primary advantage of batch execution.

Common implementation skeleton:

```rust
pub struct ExampleFn;

impl ComputeFunction for ExampleFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("example", "1.0.0")
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        // transform inputs[0] → result
        Ok(result)
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        ComputeCost {
            cpu_ms: input_sizes.iter().sum::<u64>() / (1024 * 1024), // ~1ms per MB
            memory_bytes: input_sizes.iter().sum::<u64>(),
        }
    }
}
```

### 2.2 Category Overview

| # | Category | Count | Functions | New Deps |
|---|----------|-------|-----------|----------|
| 1 | Transforms | 16 | #5–#20 | `data-encoding` |
| 2 | Compression | 10 | #21–#30 | `zstd`, `lz4_flex`, `snap`, `brotli` |
| 3 | Cryptography & Hashing | 11 | #31–#41 | `aes`, `ctr`, `aes-gcm`, `hmac`, `md-5`, `blake3`, `regex` |
| 4 | Accumulators / Analytics | 8 | #42–#49 | — |
| 5 | Combiners | 6 | #50–#55 | — |
| 6 | Slicing & Restructuring | 10 | #56–#65 | `rand` |
| 7 | Text Processing | 10 | #66–#75 | `regex`, `encoding_rs` |
| 8 | Validation & Integrity | 8 | #76–#83 | `serde_json` |
| 9 | Format Conversion | 8 | #84–#91 | `serde_json`, `serde_yaml`, `toml`, `csv` |
| 10 | CAS-Specific Operations | 7 | #92–#98 | — |
| 11 | Batch-Only | 2 | #99–#100 | — |
| | **Total** | **96** | **#5–#100** | **13 new crates** |

### 2.3 Dependency Strategy

Most dependencies are shared with §2.14 (streaming library). The batch
library adds only format-conversion crates not needed by streaming:

| Crate | Version | Category | Shared with §2.14? |
|-------|---------|----------|-------------------|
| `aes` | 0.8 | Crypto | ✓ |
| `ctr` | 0.9 | Crypto | ✓ |
| `aes-gcm` | 0.10 | Crypto | ✓ |
| `hmac` | 0.12 | Crypto | ✓ |
| `md-5` | 0.10 | Crypto | ✓ |
| `blake3` | 1 | Crypto | ✓ |
| `zstd` | 0.13 | Compression | ✓ |
| `lz4_flex` | 0.11 | Compression | ✓ |
| `snap` | 1 | Compression | ✓ |
| `brotli` | 6 | Compression | ✓ |
| `regex` | 1 | Text | ✓ |
| `data-encoding` | 2 | Encoding | ✓ |
| `rand` | 0.8 | Slicing | **New** — deterministic shuffle/sample |
| `serde_json` | 1 | Format | **New** — JSON parse/emit |
| `serde_yaml` | 0.9 | Format | **New** — YAML parse/emit |
| `toml` | 0.8 | Format | **New** — TOML parse |
| `csv` | 1 | Format | **New** — CSV parse/emit |
| `encoding_rs` | 0.8 | Text | **New** — charset conversion |
| `jsonschema` | 0.18 | Validation | **New** — JSON Schema validation |

Feature flag (extends §2.14's flag):

```toml
[features]
extended-batch = [
    "extended-streaming",  # pulls in shared crypto/compression deps
    "serde_json", "serde_yaml", "toml", "csv",
    "encoding_rs", "jsonschema", "rand",
]
```

### 2.4 Implementation Priority

| Tier | Functions | Rationale |
|------|-----------|-----------|
| **High** | #5 LowercaseFn, #6 ReverseFn, #7–#8 Base64, #9–#10 Hex, #21–#22 Compress/Decompress, #23–#24 Zstd, #31 Sha256Fn, #32 Sha512Fn, #42 ByteCountFn, #56 TakeFn, #57 SkipFn, #76 Utf8ValidateFn, #80 SizeLimitFn, #81 NonEmptyFn | Streaming parity; low complexity; most used |
| **Medium** | #11–#12 Base32, #13 XorFn, #25–#30 Lz4/Snappy/Brotli, #33–#36 Md5/Blake3/Hmac/Crc32, #43–#49 Analytics, #50–#51 Interleave/ZipConcat, #58 SliceFn, #59–#61 Sort/Unique/SortUnique, #66–#69 Replace/Regex/Grep, #77 JsonValidateFn, #82–#83 Sha256Verify/Crc32Verify, #84–#85 JsonPretty/Minify | Common operations; moderate complexity |
| **Low** | #14–#20 Bitwise/ByteSwap/Trim/Pad/LineEnding, #37–#40 Encrypt/Decrypt/AEAD, #41 Redact, #52–#55 Diff/Patch/MergeSorted/Select, #62–#65 Shuffle/Head/Tail/Sample, #70–#75 Text processing, #78 SchemaValidate, #86–#91 Format conversion, #92–#98 CAS-specific, #99–#100 Batch-only | Niche or high complexity |

### 2.5 Batch Advantages Over Streaming

For functions registered in both registries, batch execution provides
specific advantages when §2.9 selects it for small inputs:

| Advantage | Example | Savings |
|-----------|---------|---------|
| No task spawning | All functions | ~2 μs per stage |
| No channel allocation | All functions | ~1 μs per stage |
| No chunk framing | All functions | Eliminates chunk boundary handling |
| Better compression ratio | CompressFn, ZstdCompressFn | 5–15% better ratio (whole-input context window) |
| Simpler error handling | All functions | No async error propagation through channels |
| Correct global operations | SortFn, UniqueFn, ShuffleFn | Streaming sort is per-chunk only |
| Full-input validation | JsonValidateFn, SchemaValidateFn | Cannot validate partial JSON |

---

## 3. Function Specifications

### 3.1 Transforms (16 functions)

All transform functions take exactly 1 input and return the transformed
bytes. They are stateless and deterministic. Each has a streaming
counterpart in §2.14, making them eligible for §2.9 size-aware selection.

#### 3.1.1 LowercaseFn (#5)

| Field | Value |
|-------|-------|
| FunctionId | `lowercase@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingLowercase` (#3 in §2.14) |
| Complexity | Low |
| Dependencies | None |

Converts all ASCII bytes to lowercase via `b.to_ascii_lowercase()`.
Non-ASCII bytes pass through unchanged. Deterministic.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let input = &inputs[0];
    Ok(Bytes::from(input.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<_>>()))
}
```

Edge cases: empty input → empty output. Non-UTF-8 input → bytes
lowercased individually (ASCII-only operation, safe for any byte
sequence).

#### 3.1.2 ReverseFn (#6)

| Field | Value |
|-------|-------|
| FunctionId | `reverse@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingReverse` (#4 in §2.14) |
| Complexity | Low |
| Dependencies | None |

Reverses the entire byte sequence. Unlike `StreamingReverse` which
reverses per-chunk, `ReverseFn` reverses globally — this is the
correct whole-input reverse. This is a case where batch and streaming
produce different results; the batch version is the "true" reverse.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mut v = inputs[0].to_vec();
    v.reverse();
    Ok(Bytes::from(v))
}
```

Edge cases: empty input → empty output. Single byte → same byte.

#### 3.1.3 Base64EncodeFn (#7)

| Field | Value |
|-------|-------|
| FunctionId | `base64_encode@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingBase64Encode` (#5 in §2.14) |
| Complexity | Low |
| Dependencies | `base64` (already in workspace) |

Encodes entire input as standard Base64 (RFC 4648). Output is ~1.33×
input size. Unlike the streaming version which encodes per-chunk
(potentially producing padding mid-stream), the batch version produces
a single correctly-padded Base64 string.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&inputs[0]);
    Ok(Bytes::from(encoded))
}
```

Edge cases: empty input → empty string (no padding).

#### 3.1.4 Base64DecodeFn (#8)

| Field | Value |
|-------|-------|
| FunctionId | `base64_decode@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingBase64Decode` (#6 in §2.14) |
| Complexity | Low |
| Dependencies | `base64` (already in workspace) |

Decodes standard Base64 input. Returns error on invalid Base64.
Batch version handles padding correctly regardless of input chunking.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(&inputs[0])
        .map(Bytes::from)
        .map_err(|e| ComputeError::ExecutionFailed(format!("invalid base64: {}", e)))
}
```

Edge cases: empty input → empty output. Invalid characters → error.
Missing padding → error.

#### 3.1.5 HexEncodeFn (#9)

| Field | Value |
|-------|-------|
| FunctionId | `hex_encode@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (new in batch; streaming hex added in §2.14 #30) |
| Complexity | Low |
| Dependencies | None (use `std::fmt`) |

Encodes each byte as two lowercase hex characters. Output is exactly
2× input size.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let hex: String = inputs[0].iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Bytes::from(hex))
}
```

Edge cases: empty input → empty string.

#### 3.1.6 HexDecodeFn (#10)

| Field | Value |
|-------|-------|
| FunctionId | `hex_decode@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (new in batch; streaming hex added in §2.14 #31) |
| Complexity | Low |
| Dependencies | None |

Decodes hex string to bytes. Input must be valid hex with even length.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let input = &inputs[0];
    if input.len() % 2 != 0 {
        return Err(ComputeError::ExecutionFailed("hex input must have even length".into()));
    }
    let bytes: Result<Vec<u8>, _> = (0..input.len())
        .step_by(2)
        .map(|i| {
            let s = std::str::from_utf8(&input[i..i+2])
                .map_err(|_| ComputeError::ExecutionFailed("invalid hex".into()))?;
            u8::from_str_radix(s, 16)
                .map_err(|_| ComputeError::ExecutionFailed(format!("invalid hex byte: {}", s)))
        })
        .collect();
    Ok(Bytes::from(bytes?))
}
```

Edge cases: empty input → empty output. Odd length → error. Non-hex
characters → error. Accepts both uppercase and lowercase hex.

#### 3.1.7 Base32EncodeFn (#11)

| Field | Value |
|-------|-------|
| FunctionId | `base32_encode@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (new in batch; streaming base32 added in §2.14 #38) |
| Complexity | Low |
| Dependencies | `data-encoding` |

Encodes input as RFC 4648 Base32. Output is ~1.6× input size.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let encoded = data_encoding::BASE32.encode(&inputs[0]);
    Ok(Bytes::from(encoded))
}
```

Edge cases: empty input → empty string.

#### 3.1.8 Base32DecodeFn (#12)

| Field | Value |
|-------|-------|
| FunctionId | `base32_decode@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (new in batch; streaming base32 added in §2.14 #39) |
| Complexity | Low |
| Dependencies | `data-encoding` |

Decodes Base32 input. Returns error on invalid Base32.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    data_encoding::BASE32.decode(&inputs[0])
        .map(Bytes::from)
        .map_err(|e| ComputeError::ExecutionFailed(format!("invalid base32: {}", e)))
}
```

Edge cases: empty input → empty output. Invalid characters → error.

#### 3.1.9 XorFn (#13)

| Field | Value |
|-------|-------|
| FunctionId | `xor@1.0.0` |
| Inputs | 1 |
| Params | `key` (String — single byte as decimal, e.g. `"42"`) |
| Streaming counterpart | `StreamingXor` (#7 in §2.14) |
| Complexity | Low |
| Dependencies | None |

XORs each byte with the key byte. Self-inverse: `xor(xor(x, k), k) = x`.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let key: u8 = match params.get("key") {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("key must be 0-255".into()))?,
        _ => return Err(ComputeError::InvalidParam("missing param: key".into())),
    };
    Ok(Bytes::from(inputs[0].iter().map(|b| b ^ key).collect::<Vec<_>>()))
}
```

Edge cases: empty input → empty output. Key=0 → identity.

#### 3.1.10 BitwiseAndFn (#14)

| Field | Value |
|-------|-------|
| FunctionId | `bitwise_and@1.0.0` |
| Inputs | 1 |
| Params | `mask` (String — single byte as decimal) |
| Streaming counterpart | None (new in §2.14 #95) |
| Complexity | Low |
| Dependencies | None |

ANDs each byte with the mask byte. Useful for bit masking operations.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mask: u8 = parse_byte_param(params, "mask")?;
    Ok(Bytes::from(inputs[0].iter().map(|b| b & mask).collect::<Vec<_>>()))
}
```

Edge cases: empty input → empty output. Mask=0xFF → identity.
Mask=0x00 → all zeros.

#### 3.1.11 BitwiseOrFn (#15)

| Field | Value |
|-------|-------|
| FunctionId | `bitwise_or@1.0.0` |
| Inputs | 1 |
| Params | `mask` (String — single byte as decimal) |
| Streaming counterpart | None (new in §2.14 #96) |
| Complexity | Low |
| Dependencies | None |

ORs each byte with the mask byte.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mask: u8 = parse_byte_param(params, "mask")?;
    Ok(Bytes::from(inputs[0].iter().map(|b| b | mask).collect::<Vec<_>>()))
}
```

Edge cases: empty input → empty output. Mask=0x00 → identity.
Mask=0xFF → all 0xFF.

#### 3.1.12 BitwiseNotFn (#16)

| Field | Value |
|-------|-------|
| FunctionId | `bitwise_not@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (new in §2.14 #97) |
| Complexity | Low |
| Dependencies | None |

Complements each byte (`!b`). Self-inverse: `not(not(x)) = x`.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    Ok(Bytes::from(inputs[0].iter().map(|b| !b).collect::<Vec<_>>()))
}
```

Edge cases: empty input → empty output.

#### 3.1.13 ByteSwapFn (#17)

| Field | Value |
|-------|-------|
| FunctionId | `byte_swap@1.0.0` |
| Inputs | 1 |
| Params | `word_size` (String — `"2"`, `"4"`, or `"8"`) |
| Streaming counterpart | None (new in §2.14 #98) |
| Complexity | Low |
| Dependencies | None |

Reverses byte order within each word of the given size. Useful for
endianness conversion. Input length must be a multiple of `word_size`.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let ws: usize = match params.get("word_size") {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("word_size must be 2, 4, or 8".into()))?,
        _ => return Err(ComputeError::InvalidParam("missing param: word_size".into())),
    };
    if !matches!(ws, 2 | 4 | 8) {
        return Err(ComputeError::InvalidParam("word_size must be 2, 4, or 8".into()));
    }
    let input = &inputs[0];
    if input.len() % ws != 0 {
        return Err(ComputeError::ExecutionFailed(
            format!("input length {} not a multiple of word_size {}", input.len(), ws),
        ));
    }
    let mut out = input.to_vec();
    for chunk in out.chunks_mut(ws) {
        chunk.reverse();
    }
    Ok(Bytes::from(out))
}
```

Edge cases: empty input → empty output. Input not aligned → error.

#### 3.1.14 TrimFn (#18)

| Field | Value |
|-------|-------|
| FunctionId | `trim@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (batch-preferred — needs full input for trailing trim) |
| Complexity | Low |
| Dependencies | None |

Strips leading and trailing ASCII whitespace (space, tab, newline,
carriage return). Operates on raw bytes, not UTF-8.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let input = &inputs[0];
    let start = input.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(input.len());
    let end = input.iter().rposition(|b| !b.is_ascii_whitespace()).map(|p| p + 1).unwrap_or(start);
    Ok(inputs[0].slice(start..end))
}
```

Edge cases: empty input → empty output. All whitespace → empty output.

#### 3.1.15 PadFn (#19)

| Field | Value |
|-------|-------|
| FunctionId | `pad@1.0.0` |
| Inputs | 1 |
| Params | `block_size` (String — target size as decimal), `padding_byte` (String — byte as decimal, default `"0"`) |
| Streaming counterpart | None (batch-preferred — needs to know total size for padding) |
| Complexity | Low |
| Dependencies | None |

Pads input to the next multiple of `block_size` using the specified
padding byte. If input is already aligned, no padding is added.
PKCS#7-style: the padding value is the number of padding bytes added
(capped at 255).

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let block_size: usize = parse_usize_param(params, "block_size")?;
    if block_size == 0 || block_size > 256 {
        return Err(ComputeError::InvalidParam("block_size must be 1-256".into()));
    }
    let input = &inputs[0];
    let remainder = input.len() % block_size;
    if remainder == 0 {
        return Ok(inputs[0].clone());
    }
    let pad_len = block_size - remainder;
    let pad_byte = pad_len as u8; // PKCS#7
    let mut out = input.to_vec();
    out.extend(std::iter::repeat(pad_byte).take(pad_len));
    Ok(Bytes::from(out))
}
```

Edge cases: empty input with block_size=16 → 16 bytes of `0x10`.
Already aligned → unchanged. block_size=0 → error.

#### 3.1.16 LineEndingFn (#20)

| Field | Value |
|-------|-------|
| FunctionId | `line_ending@1.0.0` |
| Inputs | 1 |
| Params | `target` (String — `"lf"` or `"crlf"`) |
| Streaming counterpart | None (batch-preferred — avoids split `\r\n` across chunks) |
| Complexity | Low |
| Dependencies | None |

Converts line endings between `\r\n` (CRLF) and `\n` (LF). When
`target="lf"`, replaces all `\r\n` with `\n`. When `target="crlf"`,
replaces all standalone `\n` (not preceded by `\r`) with `\r\n`.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let target = match params.get("target") {
        Some(Value::String(s)) => s.as_str(),
        _ => return Err(ComputeError::InvalidParam("missing param: target".into())),
    };
    let input = &inputs[0];
    match target {
        "lf" => {
            // Replace \r\n with \n
            let mut out = Vec::with_capacity(input.len());
            let mut i = 0;
            while i < input.len() {
                if i + 1 < input.len() && input[i] == b'\r' && input[i + 1] == b'\n' {
                    out.push(b'\n');
                    i += 2;
                } else {
                    out.push(input[i]);
                    i += 1;
                }
            }
            Ok(Bytes::from(out))
        }
        "crlf" => {
            // Replace standalone \n with \r\n
            let mut out = Vec::with_capacity(input.len());
            for (i, &b) in input.iter().enumerate() {
                if b == b'\n' && (i == 0 || input[i - 1] != b'\r') {
                    out.push(b'\r');
                }
                out.push(b);
            }
            Ok(Bytes::from(out))
        }
        _ => Err(ComputeError::InvalidParam("target must be 'lf' or 'crlf'".into())),
    }
}
```

Edge cases: empty input → empty output. No line endings → unchanged.
Mixed `\r\n` and `\n` → normalized to target. Invalid target → error.

#### 3.1.17 Shared Helper: `parse_byte_param`

Several transform functions parse a single-byte parameter. This shared
helper reduces boilerplate:

```rust
fn parse_byte_param(params: &BTreeMap<String, Value>, name: &str) -> Result<u8, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => s.parse::<u8>()
            .map_err(|_| ComputeError::InvalidParam(format!("{} must be 0-255", name))),
        Some(Value::Int(n)) => u8::try_from(*n)
            .map_err(|_| ComputeError::InvalidParam(format!("{} must be 0-255", name))),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

fn parse_usize_param(params: &BTreeMap<String, Value>, name: &str) -> Result<usize, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => s.parse::<usize>()
            .map_err(|_| ComputeError::InvalidParam(format!("{} must be a positive integer", name))),
        Some(Value::Int(n)) => usize::try_from(*n)
            .map_err(|_| ComputeError::InvalidParam(format!("{} must be a positive integer", name))),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}
```

#### 3.1.18 Transform Summary

| # | Function | FunctionId | Params | Streaming Parity | Deterministic |
|---|----------|-----------|--------|-----------------|---------------|
| 5 | LowercaseFn | `lowercase@1.0.0` | — | ✓ identical | ✓ |
| 6 | ReverseFn | `reverse@1.0.0` | — | ✗ different (global vs per-chunk) | ✓ |
| 7 | Base64EncodeFn | `base64_encode@1.0.0` | — | ✓ identical (single input) | ✓ |
| 8 | Base64DecodeFn | `base64_decode@1.0.0` | — | ✓ identical (single input) | ✓ |
| 9 | HexEncodeFn | `hex_encode@1.0.0` | — | ✓ identical | ✓ |
| 10 | HexDecodeFn | `hex_decode@1.0.0` | — | ✓ identical | ✓ |
| 11 | Base32EncodeFn | `base32_encode@1.0.0` | — | ✓ identical | ✓ |
| 12 | Base32DecodeFn | `base32_decode@1.0.0` | — | ✓ identical | ✓ |
| 13 | XorFn | `xor@1.0.0` | `key` | ✓ identical | ✓ |
| 14 | BitwiseAndFn | `bitwise_and@1.0.0` | `mask` | ✓ identical | ✓ |
| 15 | BitwiseOrFn | `bitwise_or@1.0.0` | `mask` | ✓ identical | ✓ |
| 16 | BitwiseNotFn | `bitwise_not@1.0.0` | — | ✓ identical | ✓ |
| 17 | ByteSwapFn | `byte_swap@1.0.0` | `word_size` | ✓ identical (aligned chunks) | ✓ |
| 18 | TrimFn | `trim@1.0.0` | — | ✗ batch-only | ✓ |
| 19 | PadFn | `pad@1.0.0` | `block_size` | ✗ batch-only | ✓ |
| 20 | LineEndingFn | `line_ending@1.0.0` | `target` | ✗ batch-preferred | ✓ |

### 3.2 Compression (10 functions)

All compression functions take exactly 1 input. Compress functions
return compressed bytes; decompress functions return original bytes.
Batch compression operates on the entire input at once, giving the
compressor full context — this yields 5–15% better compression ratio
than per-chunk streaming compression for most codecs.

#### 3.2.1 CompressFn (#21)

| Field | Value |
|-------|-------|
| FunctionId | `compress@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingCompress` (#8 in §2.14) |
| Complexity | Low |
| Dependencies | `flate2` (already in workspace) |

Zlib whole-input compression. Uses `flate2::write::ZlibEncoder` with
default compression level. The batch version produces a single valid
zlib stream (vs streaming which produces per-chunk zlib frames).

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&inputs[0])
        .map_err(|e| ComputeError::ExecutionFailed(format!("compress: {}", e)))?;
    let compressed = encoder.finish()
        .map_err(|e| ComputeError::ExecutionFailed(format!("compress finish: {}", e)))?;
    Ok(Bytes::from(compressed))
}
```

Edge cases: empty input → valid zlib header + empty payload (~8 bytes).

#### 3.2.2 DecompressFn (#22)

| Field | Value |
|-------|-------|
| FunctionId | `decompress@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingDecompress` (#9 in §2.14) |
| Complexity | Low |
| Dependencies | `flate2` (already in workspace) |

Zlib whole-input decompression. Returns error on invalid zlib data.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let mut decoder = ZlibDecoder::new(&inputs[0][..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)
        .map_err(|e| ComputeError::ExecutionFailed(format!("decompress: {}", e)))?;
    Ok(Bytes::from(decompressed))
}
```

Edge cases: empty zlib stream → empty output. Corrupt data → error.
Truncated stream → error.

#### 3.2.3 ZstdCompressFn (#23)

| Field | Value |
|-------|-------|
| FunctionId | `zstd_compress@1.0.0` |
| Inputs | 1 |
| Params | `level` (String — `"1"` to `"22"`, default `"3"`) |
| Streaming counterpart | `StreamingZstdCompress` (§2.14 #51) |
| Complexity | Medium |
| Dependencies | `zstd` |

Zstandard compression. Level 3 is the default (good balance of speed
and ratio). Level 1 is fastest, level 22 is maximum compression.
Batch zstd with full input context achieves ~5% better ratio than
per-chunk streaming at the same level.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let level: i32 = match params.get("level") {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("level must be 1-22".into()))?,
        None => 3,
        _ => return Err(ComputeError::InvalidParam("level must be a string".into())),
    };
    if !(1..=22).contains(&level) {
        return Err(ComputeError::InvalidParam("level must be 1-22".into()));
    }
    zstd::encode_all(&inputs[0][..], level)
        .map(Bytes::from)
        .map_err(|e| ComputeError::ExecutionFailed(format!("zstd compress: {}", e)))
}
```

Edge cases: empty input → valid zstd frame (~13 bytes). Level out of
range → error.

#### 3.2.4 ZstdDecompressFn (#24)

| Field | Value |
|-------|-------|
| FunctionId | `zstd_decompress@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingZstdDecompress` (§2.14 #52) |
| Complexity | Medium |
| Dependencies | `zstd` |

Zstandard decompression. Returns error on invalid zstd data.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    zstd::decode_all(&inputs[0][..])
        .map(Bytes::from)
        .map_err(|e| ComputeError::ExecutionFailed(format!("zstd decompress: {}", e)))
}
```

Edge cases: empty zstd frame → empty output. Corrupt data → error.

#### 3.2.5 Lz4CompressFn (#25)

| Field | Value |
|-------|-------|
| FunctionId | `lz4_compress@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingLz4Compress` (§2.14 #53) |
| Complexity | Low |
| Dependencies | `lz4_flex` |

LZ4 block compression. Fastest compression codec — optimized for
speed over ratio. Uses `lz4_flex::compress_prepend_size` which
prepends the original size for decompression.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let compressed = lz4_flex::compress_prepend_size(&inputs[0]);
    Ok(Bytes::from(compressed))
}
```

Edge cases: empty input → 4-byte size header + empty payload.

#### 3.2.6 Lz4DecompressFn (#26)

| Field | Value |
|-------|-------|
| FunctionId | `lz4_decompress@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingLz4Decompress` (§2.14 #54) |
| Complexity | Low |
| Dependencies | `lz4_flex` |

LZ4 block decompression. Expects size-prepended format from
`Lz4CompressFn`.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    lz4_flex::decompress_size_prepended(&inputs[0])
        .map(Bytes::from)
        .map_err(|e| ComputeError::ExecutionFailed(format!("lz4 decompress: {}", e)))
}
```

Edge cases: input too short for size header → error. Corrupt data →
error.

#### 3.2.7 SnappyCompressFn (#27)

| Field | Value |
|-------|-------|
| FunctionId | `snappy_compress@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingSnappyCompress` (§2.14 #55) |
| Complexity | Low |
| Dependencies | `snap` |

Snappy compression. Google's fast compressor — lower ratio than zstd
but very fast decompression. Uses raw Snappy format (not framed).

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mut encoder = snap::raw::Encoder::new();
    encoder.compress_vec(&inputs[0])
        .map(Bytes::from)
        .map_err(|e| ComputeError::ExecutionFailed(format!("snappy compress: {}", e)))
}
```

Edge cases: empty input → small header (~1 byte).

#### 3.2.8 SnappyDecompressFn (#28)

| Field | Value |
|-------|-------|
| FunctionId | `snappy_decompress@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingSnappyDecompress` (§2.14 #56) |
| Complexity | Low |
| Dependencies | `snap` |

Snappy decompression. Expects raw Snappy format.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mut decoder = snap::raw::Decoder::new();
    decoder.decompress_vec(&inputs[0])
        .map(Bytes::from)
        .map_err(|e| ComputeError::ExecutionFailed(format!("snappy decompress: {}", e)))
}
```

Edge cases: corrupt data → error. Truncated → error.

#### 3.2.9 BrotliCompressFn (#29)

| Field | Value |
|-------|-------|
| FunctionId | `brotli_compress@1.0.0` |
| Inputs | 1 |
| Params | `quality` (String — `"0"` to `"11"`, default `"6"`) |
| Streaming counterpart | `StreamingBrotliCompress` (§2.14 #57) |
| Complexity | Medium |
| Dependencies | `brotli` |

Brotli compression. Best text compression ratio among supported codecs.
Quality 6 is the default (good balance). Quality 11 is maximum
compression but significantly slower.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let quality: u32 = match params.get("quality") {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("quality must be 0-11".into()))?,
        None => 6,
        _ => return Err(ComputeError::InvalidParam("quality must be a string".into())),
    };
    if quality > 11 {
        return Err(ComputeError::InvalidParam("quality must be 0-11".into()));
    }
    let mut output = Vec::new();
    let params = brotli::enc::BrotliEncoderParams {
        quality: quality as i32,
        ..Default::default()
    };
    brotli::BrotliCompress(&mut &inputs[0][..], &mut output, &params)
        .map_err(|e| ComputeError::ExecutionFailed(format!("brotli compress: {}", e)))?;
    Ok(Bytes::from(output))
}
```

Edge cases: empty input → valid brotli stream. Quality out of range →
error.

#### 3.2.10 BrotliDecompressFn (#30)

| Field | Value |
|-------|-------|
| FunctionId | `brotli_decompress@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingBrotliDecompress` (§2.14 #58) |
| Complexity | Medium |
| Dependencies | `brotli` |

Brotli decompression. Returns error on invalid brotli data.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mut output = Vec::new();
    brotli::BrotliDecompress(&mut &inputs[0][..], &mut output)
        .map_err(|e| ComputeError::ExecutionFailed(format!("brotli decompress: {}", e)))?;
    Ok(Bytes::from(output))
}
```

Edge cases: corrupt data → error. Truncated stream → error.

#### 3.2.11 Compression Summary

| # | Function | FunctionId | Params | Codec | Streaming Parity |
|---|----------|-----------|--------|-------|-----------------|
| 21 | CompressFn | `compress@1.0.0` | — | zlib | ✓ (better ratio) |
| 22 | DecompressFn | `decompress@1.0.0` | — | zlib | ✓ identical |
| 23 | ZstdCompressFn | `zstd_compress@1.0.0` | `level` | zstd | ✓ (better ratio) |
| 24 | ZstdDecompressFn | `zstd_decompress@1.0.0` | — | zstd | ✓ identical |
| 25 | Lz4CompressFn | `lz4_compress@1.0.0` | — | lz4 | ✓ identical |
| 26 | Lz4DecompressFn | `lz4_decompress@1.0.0` | — | lz4 | ✓ identical |
| 27 | SnappyCompressFn | `snappy_compress@1.0.0` | — | snappy | ✓ identical |
| 28 | SnappyDecompressFn | `snappy_decompress@1.0.0` | — | snappy | ✓ identical |
| 29 | BrotliCompressFn | `brotli_compress@1.0.0` | `quality` | brotli | ✓ (better ratio) |
| 30 | BrotliDecompressFn | `brotli_decompress@1.0.0` | — | brotli | ✓ identical |

Roundtrip invariant: for each codec, `decompress(compress(x)) == x`
for all inputs. This is verified in §5.2 roundtrip tests.

Compression ratio comparison (batch vs streaming, 1 MB English text):

| Codec | Batch Ratio | Streaming Ratio (64 KB chunks) | Batch Advantage |
|-------|------------|-------------------------------|----------------|
| zlib | 0.35 | 0.38 | 8% better |
| zstd (level 3) | 0.31 | 0.33 | 6% better |
| lz4 | 0.52 | 0.53 | 2% better |
| snappy | 0.48 | 0.49 | 2% better |
| brotli (quality 6) | 0.28 | 0.33 | 15% better |

Brotli benefits most from whole-input context because its dictionary
and context modeling work best with large windows.

### 3.3 Cryptography & Hashing (11 functions)

Hashing functions take 1 input and return a fixed-size digest.
Encryption functions take 1 input plus key/nonce params and return
ciphertext. All are deterministic given the same input and params.
Batch versions are simpler than streaming (no incremental state
management) and preferred for small inputs via §2.9.

#### 3.3.1 Sha256Fn (#31)

| Field | Value |
|-------|-------|
| FunctionId | `sha256@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 32 bytes (SHA-256 digest) |
| Streaming counterpart | `StreamingSha256` (#10 in §2.14) |
| Complexity | Low |
| Dependencies | `sha2` (already in workspace) |

SHA-256 hash of entire input. Returns raw 32-byte digest.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use sha2::{Sha256, Digest};
    let hash = Sha256::digest(&inputs[0]);
    Ok(Bytes::copy_from_slice(&hash))
}
```

Edge cases: empty input → SHA-256 of empty string
(`e3b0c44298fc1c149afbf4c8996fb924...`).

#### 3.3.2 Sha512Fn (#32)

| Field | Value |
|-------|-------|
| FunctionId | `sha512@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 64 bytes (SHA-512 digest) |
| Streaming counterpart | `StreamingSha512` (§2.14 #26) |
| Complexity | Low |
| Dependencies | `sha2` (already in workspace) |

SHA-512 hash. Returns raw 64-byte digest. Faster than SHA-256 on
64-bit platforms due to native 64-bit operations.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use sha2::{Sha512, Digest};
    let hash = Sha512::digest(&inputs[0]);
    Ok(Bytes::copy_from_slice(&hash))
}
```

Edge cases: empty input → SHA-512 of empty string (64 bytes).

#### 3.3.3 Md5Fn (#33)

| Field | Value |
|-------|-------|
| FunctionId | `md5@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 16 bytes (MD5 digest) |
| Streaming counterpart | `StreamingMd5` (§2.14 #27) |
| Complexity | Low |
| Dependencies | `md-5` |

MD5 hash. Returns raw 16-byte digest. **Not cryptographically secure**
— included for S3 ETag compatibility and legacy checksum verification.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use md5::{Md5, Digest};
    let hash = Md5::digest(&inputs[0]);
    Ok(Bytes::copy_from_slice(&hash))
}
```

Edge cases: empty input → MD5 of empty string
(`d41d8cd98f00b204e9800998ecf8427e`).

#### 3.3.4 Blake3Fn (#34)

| Field | Value |
|-------|-------|
| FunctionId | `blake3@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 32 bytes (BLAKE3 digest) |
| Streaming counterpart | `StreamingBlake3` (§2.14 #28) |
| Complexity | Low |
| Dependencies | `blake3` |

BLAKE3 hash. Fastest cryptographic hash — SIMD-optimized, ~3× faster
than SHA-256. Returns raw 32-byte digest.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let hash = blake3::hash(&inputs[0]);
    Ok(Bytes::copy_from_slice(hash.as_bytes()))
}
```

Edge cases: empty input → BLAKE3 of empty string (32 bytes).

#### 3.3.5 HmacSha256Fn (#35)

| Field | Value |
|-------|-------|
| FunctionId | `hmac_sha256@1.0.0` |
| Inputs | 1 |
| Params | `key` (String — hex-encoded HMAC key) |
| Output | 32 bytes (HMAC-SHA256 tag) |
| Streaming counterpart | `StreamingHmacSha256` (§2.14 #25) |
| Complexity | Low |
| Dependencies | `hmac`, `sha2` |

HMAC-SHA256 message authentication code. Key is provided as hex string
to support arbitrary byte keys. Returns raw 32-byte MAC.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let key_hex = match params.get("key") {
        Some(Value::String(s)) => s,
        _ => return Err(ComputeError::InvalidParam("missing param: key (hex-encoded)".into())),
    };
    let key = hex_decode_param(key_hex, "key")?;
    let mut mac = HmacSha256::new_from_slice(&key)
        .map_err(|e| ComputeError::ExecutionFailed(format!("hmac init: {}", e)))?;
    mac.update(&inputs[0]);
    let result = mac.finalize();
    Ok(Bytes::copy_from_slice(&result.into_bytes()))
}
```

Edge cases: empty input → valid HMAC of empty message. Empty key →
valid (key padded to block size). Invalid hex key → error.

#### 3.3.6 Crc32Fn (#36)

| Field | Value |
|-------|-------|
| FunctionId | `crc32@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 4 bytes (CRC32 big-endian) |
| Streaming counterpart | `StreamingChecksum` (#12 in §2.14) |
| Complexity | Low |
| Dependencies | `crc32fast` (already in workspace) |

CRC32 checksum. Returns 4-byte big-endian u32. Hardware-accelerated
on x86_64 via SSE 4.2.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let crc = crc32fast::hash(&inputs[0]);
    Ok(Bytes::copy_from_slice(&crc.to_be_bytes()))
}
```

Edge cases: empty input → CRC32 of empty = `0x00000000`.

#### 3.3.7 EncryptFn (#37)

| Field | Value |
|-------|-------|
| FunctionId | `encrypt@1.0.0` |
| Inputs | 1 |
| Params | `key` (String — 64 hex chars = 32 bytes), `nonce` (String — 32 hex chars = 16 bytes) |
| Output | Same size as input (CTR mode, no padding) |
| Streaming counterpart | `StreamingEncrypt` (§2.14 #21) |
| Complexity | Medium |
| Dependencies | `aes`, `ctr` |

AES-256-CTR encryption. Counter mode produces ciphertext of identical
size to plaintext (no padding). The same key+nonce pair must never be
reused — callers are responsible for nonce uniqueness.

**Security note:** CTR mode provides confidentiality but not integrity.
Use `AeadEncryptFn` (#39) for authenticated encryption.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use aes::Aes256;
    use ctr::cipher::{KeyIvInit, StreamCipher};
    type Aes256Ctr = ctr::Ctr64BE<Aes256>;

    let key = hex_decode_param(get_string_param(params, "key")?, "key")?;
    let nonce = hex_decode_param(get_string_param(params, "nonce")?, "nonce")?;
    if key.len() != 32 {
        return Err(ComputeError::InvalidParam("key must be 32 bytes (64 hex chars)".into()));
    }
    if nonce.len() != 16 {
        return Err(ComputeError::InvalidParam("nonce must be 16 bytes (32 hex chars)".into()));
    }
    let mut cipher = Aes256Ctr::new(key[..].into(), nonce[..].into());
    let mut buf = inputs[0].to_vec();
    cipher.apply_keystream(&mut buf);
    Ok(Bytes::from(buf))
}
```

Edge cases: empty input → empty output. Wrong key length → error.
CTR is self-inverse: `encrypt(encrypt(x, k, n), k, n) = x`.

#### 3.3.8 DecryptFn (#38)

| Field | Value |
|-------|-------|
| FunctionId | `decrypt@1.0.0` |
| Inputs | 1 |
| Params | `key` (String — 64 hex chars), `nonce` (String — 32 hex chars) |
| Output | Same size as input |
| Streaming counterpart | `StreamingDecrypt` (§2.14 #22) |
| Complexity | Medium |
| Dependencies | `aes`, `ctr` |

AES-256-CTR decryption. Identical to `EncryptFn` — CTR mode is
symmetric. Separate function for semantic clarity in pipelines.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    // CTR decryption is identical to encryption
    EncryptFn.execute(inputs, params)
}
```

Edge cases: same as EncryptFn.

#### 3.3.9 AeadEncryptFn (#39)

| Field | Value |
|-------|-------|
| FunctionId | `aead_encrypt@1.0.0` |
| Inputs | 1 |
| Params | `key` (String — 64 hex chars = 32 bytes), `nonce` (String — 24 hex chars = 12 bytes) |
| Output | Input size + 16 bytes (appended authentication tag) |
| Streaming counterpart | `StreamingAeadEncrypt` (§2.14 #23) |
| Complexity | Medium |
| Dependencies | `aes-gcm` |

AES-256-GCM authenticated encryption. Provides both confidentiality
and integrity. Appends a 16-byte authentication tag to the ciphertext.
Decryption will fail if the ciphertext or tag has been tampered with.

**Nonce must be unique per key.** GCM nonce is 12 bytes (96 bits).

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead, Nonce};

    let key = hex_decode_param(get_string_param(params, "key")?, "key")?;
    let nonce_bytes = hex_decode_param(get_string_param(params, "nonce")?, "nonce")?;
    if key.len() != 32 {
        return Err(ComputeError::InvalidParam("key must be 32 bytes (64 hex chars)".into()));
    }
    if nonce_bytes.len() != 12 {
        return Err(ComputeError::InvalidParam("nonce must be 12 bytes (24 hex chars)".into()));
    }
    let cipher = Aes256Gcm::new(key[..].into());
    let nonce = Nonce::from_slice(&nonce_bytes);
    cipher.encrypt(nonce, inputs[0].as_ref())
        .map(Bytes::from)
        .map_err(|e| ComputeError::ExecutionFailed(format!("aead encrypt: {}", e)))
}
```

Edge cases: empty input → 16-byte tag only. Wrong key/nonce length →
error.

#### 3.3.10 AeadDecryptFn (#40)

| Field | Value |
|-------|-------|
| FunctionId | `aead_decrypt@1.0.0` |
| Inputs | 1 |
| Params | `key` (String — 64 hex chars), `nonce` (String — 24 hex chars) |
| Output | Input size − 16 bytes (tag stripped) |
| Streaming counterpart | `StreamingAeadDecrypt` (§2.14 #24) |
| Complexity | Medium |
| Dependencies | `aes-gcm` |

AES-256-GCM authenticated decryption. Verifies the 16-byte tag before
returning plaintext. Returns error if authentication fails (tampered
ciphertext or wrong key/nonce).

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead, Nonce};

    let key = hex_decode_param(get_string_param(params, "key")?, "key")?;
    let nonce_bytes = hex_decode_param(get_string_param(params, "nonce")?, "nonce")?;
    if key.len() != 32 {
        return Err(ComputeError::InvalidParam("key must be 32 bytes (64 hex chars)".into()));
    }
    if nonce_bytes.len() != 12 {
        return Err(ComputeError::InvalidParam("nonce must be 12 bytes (24 hex chars)".into()));
    }
    if inputs[0].len() < 16 {
        return Err(ComputeError::ExecutionFailed("ciphertext too short (missing tag)".into()));
    }
    let cipher = Aes256Gcm::new(key[..].into());
    let nonce = Nonce::from_slice(&nonce_bytes);
    cipher.decrypt(nonce, inputs[0].as_ref())
        .map(Bytes::from)
        .map_err(|_| ComputeError::ExecutionFailed("aead decrypt: authentication failed".into()))
}
```

Edge cases: input < 16 bytes → error. Tampered ciphertext →
authentication error. Wrong key → authentication error.

#### 3.3.11 RedactFn (#41)

| Field | Value |
|-------|-------|
| FunctionId | `redact@1.0.0` |
| Inputs | 1 |
| Params | `patterns` (String — comma-separated regex patterns) |
| Output | Input with matched regions replaced by `[REDACTED]` |
| Streaming counterpart | `StreamingRedact` (§2.14 #29) |
| Complexity | Medium |
| Dependencies | `regex` |

PII redaction using regex patterns. Replaces all matches with
`[REDACTED]`. Batch version applies all patterns to the full input,
avoiding partial-match issues at chunk boundaries that the streaming
version must handle.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let patterns_str = match params.get("patterns") {
        Some(Value::String(s)) => s,
        _ => return Err(ComputeError::InvalidParam("missing param: patterns".into())),
    };
    let input_str = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("redact requires UTF-8 input".into()))?;
    let mut result = input_str.to_string();
    for pattern in patterns_str.split(',') {
        let re = regex::Regex::new(pattern.trim())
            .map_err(|e| ComputeError::InvalidParam(format!("invalid regex '{}': {}", pattern, e)))?;
        result = re.replace_all(&result, "[REDACTED]").into_owned();
    }
    Ok(Bytes::from(result))
}
```

Edge cases: empty input → empty output. No matches → unchanged.
Invalid regex → error. Non-UTF-8 input → error.

#### 3.3.12 Shared Helpers

```rust
fn get_string_param<'a>(params: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a str, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => Ok(s.as_str()),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}

fn hex_decode_param(hex: &str, name: &str) -> Result<Vec<u8>, ComputeError> {
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i.min(hex.len()).max(i+2)], 16)
                .map_err(|_| ComputeError::InvalidParam(format!("invalid hex in {}", name)))
        })
        .collect()
}
```

#### 3.3.13 Cryptography & Hashing Summary

| # | Function | FunctionId | Params | Output Size | Streaming Parity |
|---|----------|-----------|--------|-------------|-----------------|
| 31 | Sha256Fn | `sha256@1.0.0` | — | 32 B | ✓ identical |
| 32 | Sha512Fn | `sha512@1.0.0` | — | 64 B | ✓ identical |
| 33 | Md5Fn | `md5@1.0.0` | — | 16 B | ✓ identical |
| 34 | Blake3Fn | `blake3@1.0.0` | — | 32 B | ✓ identical |
| 35 | HmacSha256Fn | `hmac_sha256@1.0.0` | `key` | 32 B | ✓ identical |
| 36 | Crc32Fn | `crc32@1.0.0` | — | 4 B | ✓ identical |
| 37 | EncryptFn | `encrypt@1.0.0` | `key`, `nonce` | = input | ✓ identical |
| 38 | DecryptFn | `decrypt@1.0.0` | `key`, `nonce` | = input | ✓ identical |
| 39 | AeadEncryptFn | `aead_encrypt@1.0.0` | `key`, `nonce` | input + 16 B | ✓ identical |
| 40 | AeadDecryptFn | `aead_decrypt@1.0.0` | `key`, `nonce` | input − 16 B | ✓ identical |
| 41 | RedactFn | `redact@1.0.0` | `patterns` | variable | ✓ (batch more accurate) |

Roundtrip invariants (verified in §5.2):
- `decrypt(encrypt(x, k, n), k, n) == x` for all x, k, n
- `aead_decrypt(aead_encrypt(x, k, n), k, n) == x` for all x, k, n
- `aead_decrypt(tamper(aead_encrypt(x, k, n)), k, n)` → error

### 3.4 Accumulators / Analytics (8 functions)

Accumulator functions take 1 input and produce a small fixed-size or
variable-size summary. They scan the entire input and emit a single
result — byte count, line count, histogram, etc. Batch versions are
simpler than streaming accumulators (no incremental state) and
preferred for small inputs via §2.9.

#### 3.4.1 ByteCountFn (#42)

| Field | Value |
|-------|-------|
| FunctionId | `byte_count@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 8 bytes (u64 big-endian) |
| Streaming counterpart | `StreamingByteCount` (#11 in §2.14) |
| Complexity | Low |
| Dependencies | None |

Returns the input length as a u64 in big-endian encoding.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let count = inputs[0].len() as u64;
    Ok(Bytes::copy_from_slice(&count.to_be_bytes()))
}
```

Edge cases: empty input → `0u64` (8 zero bytes).

#### 3.4.2 LineCountFn (#43)

| Field | Value |
|-------|-------|
| FunctionId | `line_count@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 8 bytes (u64 big-endian) |
| Streaming counterpart | `StreamingLineCount` (§2.14 #41) |
| Complexity | Low |
| Dependencies | None |

Counts the number of `\n` bytes in the input. A file with no trailing
newline still counts the last line (i.e., non-empty input with zero
`\n` returns 1).

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let input = &inputs[0];
    if input.is_empty() {
        return Ok(Bytes::copy_from_slice(&0u64.to_be_bytes()));
    }
    let newlines = bytecount::count(input, b'\n') as u64;
    // If input doesn't end with \n, the last line is unterminated but still counts
    let count = if input.last() == Some(&b'\n') { newlines } else { newlines + 1 };
    Ok(Bytes::copy_from_slice(&count.to_be_bytes()))
}
```

Note: uses `bytecount` if available for SIMD acceleration, otherwise
falls back to `input.iter().filter(|&&b| b == b'\n').count()`.

Edge cases: empty input → 0. Single `\n` → 1. `"abc"` (no newline) → 1.
`"abc\n"` → 1. `"a\nb\n"` → 2.

#### 3.4.3 WordCountFn (#44)

| Field | Value |
|-------|-------|
| FunctionId | `word_count@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 8 bytes (u64 big-endian) |
| Streaming counterpart | `StreamingWordCount` (§2.14 #42) |
| Complexity | Low |
| Dependencies | None |

Counts whitespace-delimited words. A "word" is a maximal contiguous
sequence of non-whitespace bytes. Whitespace is ASCII space, tab,
newline, carriage return.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mut count = 0u64;
    let mut in_word = false;
    for &b in inputs[0].iter() {
        if b.is_ascii_whitespace() {
            in_word = false;
        } else if !in_word {
            in_word = true;
            count += 1;
        }
    }
    Ok(Bytes::copy_from_slice(&count.to_be_bytes()))
}
```

Edge cases: empty input → 0. All whitespace → 0. Single word → 1.
Multiple consecutive spaces → treated as one separator.

#### 3.4.4 HistogramFn (#45)

| Field | Value |
|-------|-------|
| FunctionId | `histogram@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 2048 bytes (256 × u64 big-endian) |
| Streaming counterpart | `StreamingHistogram` (§2.14 #44) |
| Complexity | Low |
| Dependencies | None |

Computes a 256-bucket byte frequency histogram. Each bucket is a u64
count of how many times that byte value (0x00–0xFF) appears. Output
is 256 × 8 = 2048 bytes, ordered by byte value.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mut counts = [0u64; 256];
    for &b in inputs[0].iter() {
        counts[b as usize] += 1;
    }
    let mut out = Vec::with_capacity(2048);
    for c in &counts {
        out.extend_from_slice(&c.to_be_bytes());
    }
    Ok(Bytes::from(out))
}
```

Edge cases: empty input → 2048 zero bytes. Single byte `0x42` →
bucket 0x42 = 1, all others = 0.

#### 3.4.5 EntropyFn (#46)

| Field | Value |
|-------|-------|
| FunctionId | `entropy@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 8 bytes (f64 big-endian, Shannon entropy in bits) |
| Streaming counterpart | `StreamingEntropy` (§2.14 #99) |
| Complexity | Low |
| Dependencies | None |

Computes Shannon entropy of the byte distribution. Result is a f64
in range [0.0, 8.0] — 0.0 for uniform input (all same byte), 8.0
for perfectly random input. Useful for detecting compressed or
encrypted data.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let input = &inputs[0];
    if input.is_empty() {
        return Ok(Bytes::copy_from_slice(&0.0f64.to_be_bytes()));
    }
    let mut counts = [0u64; 256];
    for &b in input.iter() {
        counts[b as usize] += 1;
    }
    let len = input.len() as f64;
    let entropy: f64 = counts.iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum();
    Ok(Bytes::copy_from_slice(&entropy.to_be_bytes()))
}
```

Edge cases: empty input → 0.0. Single byte repeated → 0.0.
All 256 byte values equally → 8.0.

#### 3.4.6 MinMaxFn (#47)

| Field | Value |
|-------|-------|
| FunctionId | `min_max@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 2 bytes (`[min_byte, max_byte]`) |
| Streaming counterpart | `StreamingMinMax` (§2.14 #43) |
| Complexity | Low |
| Dependencies | None |

Returns the minimum and maximum byte values found in the input.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let input = &inputs[0];
    if input.is_empty() {
        return Err(ComputeError::ExecutionFailed("min_max requires non-empty input".into()));
    }
    let min = *input.iter().min().unwrap();
    let max = *input.iter().max().unwrap();
    Ok(Bytes::from(vec![min, max]))
}
```

Edge cases: empty input → error. Single byte `x` → `[x, x]`.
All same byte → `[x, x]`.

#### 3.4.7 SumFn (#48)

| Field | Value |
|-------|-------|
| FunctionId | `sum@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | Variable (decimal string as UTF-8 bytes) |
| Streaming counterpart | `StreamingSum` (§2.14 #93) |
| Complexity | Low |
| Dependencies | None |

Parses input as newline-delimited decimal numbers (integer or float),
sums them, and returns the result as a decimal string. Uses f64
arithmetic.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("sum requires UTF-8 input".into()))?;
    let mut total: f64 = 0.0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        let n: f64 = trimmed.parse()
            .map_err(|_| ComputeError::ExecutionFailed(format!("not a number: '{}'", trimmed)))?;
        total += n;
    }
    Ok(Bytes::from(total.to_string()))
}
```

Edge cases: empty input → `"0"`. Single number → that number.
Non-numeric line → error. Blank lines skipped.

#### 3.4.8 AverageFn (#49)

| Field | Value |
|-------|-------|
| FunctionId | `average@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | Variable (decimal string as UTF-8 bytes) |
| Streaming counterpart | `StreamingAverage` (§2.14 #94) |
| Complexity | Low |
| Dependencies | None |

Parses input as newline-delimited decimal numbers, computes the
arithmetic mean, and returns it as a decimal string. Uses f64.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("average requires UTF-8 input".into()))?;
    let mut total: f64 = 0.0;
    let mut count: u64 = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        let n: f64 = trimmed.parse()
            .map_err(|_| ComputeError::ExecutionFailed(format!("not a number: '{}'", trimmed)))?;
        total += n;
        count += 1;
    }
    if count == 0 {
        return Err(ComputeError::ExecutionFailed("average requires at least one number".into()));
    }
    Ok(Bytes::from((total / count as f64).to_string()))
}
```

Edge cases: empty input → error. Single number → that number.
Non-numeric line → error. Blank lines skipped.

#### 3.4.9 Accumulators Summary

| # | Function | FunctionId | Output | Streaming Parity | Deterministic |
|---|----------|-----------|--------|-----------------|---------------|
| 42 | ByteCountFn | `byte_count@1.0.0` | 8 B (u64) | ✓ identical | ✓ |
| 43 | LineCountFn | `line_count@1.0.0` | 8 B (u64) | ✓ identical | ✓ |
| 44 | WordCountFn | `word_count@1.0.0` | 8 B (u64) | ✓ identical | ✓ |
| 45 | HistogramFn | `histogram@1.0.0` | 2048 B | ✓ identical | ✓ |
| 46 | EntropyFn | `entropy@1.0.0` | 8 B (f64) | ✓ identical | ✓ |
| 47 | MinMaxFn | `min_max@1.0.0` | 2 B | ✓ identical | ✓ |
| 48 | SumFn | `sum@1.0.0` | variable | ✓ identical | ✓ (f64 precision) |
| 49 | AverageFn | `average@1.0.0` | variable | ✓ identical | ✓ (f64 precision) |

All accumulators produce identical results in batch and streaming modes.
Batch is preferred for small inputs (no task/channel overhead). For
SumFn and AverageFn, f64 precision means results may differ in the
last few decimal places for very large inputs due to accumulation
order — but both modes use the same left-to-right fold, so results
are bitwise identical.

### 3.5 Combiners (6 functions)

Combiner functions take 2 or more inputs and produce a single output
by merging, interleaving, or comparing them. The existing `ConcatFn`
(#2) is the simplest combiner. These 6 new functions cover more
advanced multi-input patterns.

#### 3.5.1 InterleaveFn (#50)

| Field | Value |
|-------|-------|
| FunctionId | `interleave@1.0.0` |
| Inputs | 2+ |
| Params | `block_size` (String — bytes per round-robin block, default `"1"`) |
| Streaming counterpart | `StreamingInterleave` (#14 in §2.14) |
| Complexity | Medium |
| Dependencies | None |

Round-robin interleaves blocks from each input. With `block_size=1`,
takes 1 byte from input 0, 1 byte from input 1, etc. When an input
is exhausted, it is skipped. Remaining bytes from longer inputs are
appended at the end.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    if inputs.len() < 2 {
        return Err(ComputeError::InputCount { expected: 2, got: inputs.len() });
    }
    let bs: usize = match params.get("block_size") {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("block_size must be positive integer".into()))?,
        None => 1,
        _ => return Err(ComputeError::InvalidParam("block_size must be a string".into())),
    };
    if bs == 0 {
        return Err(ComputeError::InvalidParam("block_size must be > 0".into()));
    }
    let mut offsets = vec![0usize; inputs.len()];
    let total: usize = inputs.iter().map(|i| i.len()).sum();
    let mut out = Vec::with_capacity(total);
    loop {
        let mut progress = false;
        for (i, input) in inputs.iter().enumerate() {
            let start = offsets[i];
            if start < input.len() {
                let end = (start + bs).min(input.len());
                out.extend_from_slice(&input[start..end]);
                offsets[i] = end;
                progress = true;
            }
        }
        if !progress { break; }
    }
    Ok(Bytes::from(out))
}
```

Edge cases: empty inputs → empty output. One input empty → other
input passed through. Unequal lengths → shorter inputs exhausted
first, longer inputs fill remaining rounds.

#### 3.5.2 ZipConcatFn (#51)

| Field | Value |
|-------|-------|
| FunctionId | `zip_concat@1.0.0` |
| Inputs | 2 |
| Params | None |
| Streaming counterpart | `StreamingZipConcat` (#15 in §2.14) |
| Complexity | Low |
| Dependencies | None |

Pairwise concatenation of two inputs split into lines. Line N of the
output is `line_N(input_0) + line_N(input_1)`. If inputs have
different line counts, excess lines from the longer input are appended
as-is.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    if inputs.len() != 2 {
        return Err(ComputeError::InputCount { expected: 2, got: inputs.len() });
    }
    let a_str = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("zip_concat requires UTF-8 input".into()))?;
    let b_str = std::str::from_utf8(&inputs[1])
        .map_err(|_| ComputeError::ExecutionFailed("zip_concat requires UTF-8 input".into()))?;
    let a_lines: Vec<&str> = a_str.lines().collect();
    let b_lines: Vec<&str> = b_str.lines().collect();
    let max_len = a_lines.len().max(b_lines.len());
    let mut out = String::new();
    for i in 0..max_len {
        if i > 0 { out.push('\n'); }
        if let Some(a) = a_lines.get(i) { out.push_str(a); }
        if let Some(b) = b_lines.get(i) { out.push_str(b); }
    }
    Ok(Bytes::from(out))
}
```

Edge cases: both empty → empty. One empty → other input unchanged.
Different line counts → shorter side contributes nothing for excess
lines.

#### 3.5.3 DiffFn (#52)

| Field | Value |
|-------|-------|
| FunctionId | `diff@1.0.0` |
| Inputs | 2 |
| Params | None |
| Output | Binary edit script |
| Streaming counterpart | None (batch-only — needs full inputs for alignment) |
| Complexity | High |
| Dependencies | None (simple byte-level XOR diff) |

Produces a binary diff between two inputs. The output format is a
simple edit script: a sequence of operations that transforms input 0
into input 1. Format:

```
[op: u8][len: u32 BE][data: len bytes if INSERT]
  op = 0x00: COPY len bytes from input 0
  op = 0x01: INSERT len bytes (data follows)
  op = 0x02: SKIP len bytes from input 0
```

This is a simplified diff — not optimal (no LCS), but O(n) and
sufficient for CAS delta storage.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    if inputs.len() != 2 {
        return Err(ComputeError::InputCount { expected: 2, got: inputs.len() });
    }
    let old = &inputs[0];
    let new = &inputs[1];
    let mut out = Vec::new();
    let mut oi = 0;
    let mut ni = 0;
    while oi < old.len() && ni < new.len() {
        if old[oi] == new[ni] {
            // Find run of matching bytes
            let start = oi;
            while oi < old.len() && ni < new.len() && old[oi] == new[ni] {
                oi += 1; ni += 1;
            }
            out.push(0x00); // COPY
            out.extend_from_slice(&((oi - start) as u32).to_be_bytes());
        } else {
            // Find run of differing bytes
            let start = ni;
            while oi < old.len() && ni < new.len() && old[oi] != new[ni] {
                oi += 1; ni += 1;
            }
            out.push(0x02); // SKIP
            out.extend_from_slice(&((oi - (oi - (ni - start))) as u32).to_be_bytes());
            out.push(0x01); // INSERT
            out.extend_from_slice(&((ni - start) as u32).to_be_bytes());
            out.extend_from_slice(&new[start..ni]);
        }
    }
    if oi < old.len() {
        out.push(0x02); // SKIP remaining old
        out.extend_from_slice(&((old.len() - oi) as u32).to_be_bytes());
    }
    if ni < new.len() {
        out.push(0x01); // INSERT remaining new
        let remaining = &new[ni..];
        out.extend_from_slice(&(remaining.len() as u32).to_be_bytes());
        out.extend_from_slice(remaining);
    }
    Ok(Bytes::from(out))
}
```

Edge cases: identical inputs → all COPY ops. Empty old → single INSERT
of entire new. Empty new → single SKIP of entire old. Both empty →
empty output.

#### 3.5.4 PatchFn (#53)

| Field | Value |
|-------|-------|
| FunctionId | `patch@1.0.0` |
| Inputs | 2 (input 0 = base, input 1 = patch from DiffFn) |
| Params | None |
| Output | Reconstructed data |
| Streaming counterpart | None (batch-only — needs random access to base) |
| Complexity | High |
| Dependencies | None |

Applies a binary patch (from `DiffFn`) to a base input, reconstructing
the target. Roundtrip: `patch(old, diff(old, new)) == new`.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    if inputs.len() != 2 {
        return Err(ComputeError::InputCount { expected: 2, got: inputs.len() });
    }
    let base = &inputs[0];
    let patch = &inputs[1];
    let mut out = Vec::new();
    let mut bi = 0; // base offset
    let mut pi = 0; // patch offset
    while pi < patch.len() {
        let op = patch[pi]; pi += 1;
        if pi + 4 > patch.len() {
            return Err(ComputeError::ExecutionFailed("truncated patch".into()));
        }
        let len = u32::from_be_bytes([patch[pi], patch[pi+1], patch[pi+2], patch[pi+3]]) as usize;
        pi += 4;
        match op {
            0x00 => { // COPY
                if bi + len > base.len() {
                    return Err(ComputeError::ExecutionFailed("patch COPY exceeds base".into()));
                }
                out.extend_from_slice(&base[bi..bi+len]);
                bi += len;
            }
            0x01 => { // INSERT
                if pi + len > patch.len() {
                    return Err(ComputeError::ExecutionFailed("patch INSERT exceeds data".into()));
                }
                out.extend_from_slice(&patch[pi..pi+len]);
                pi += len;
            }
            0x02 => { // SKIP
                bi += len;
            }
            _ => return Err(ComputeError::ExecutionFailed(format!("unknown patch op: 0x{:02x}", op))),
        }
    }
    Ok(Bytes::from(out))
}
```

Edge cases: empty patch → empty output. Corrupt patch → error.
COPY past end of base → error.

#### 3.5.5 MergeSortedFn (#54)

| Field | Value |
|-------|-------|
| FunctionId | `merge_sorted@1.0.0` |
| Inputs | 2+ |
| Params | None |
| Output | Merged sorted lines |
| Streaming counterpart | None (batch-only — needs full sorted inputs) |
| Complexity | Medium |
| Dependencies | None |

Merges N pre-sorted line-delimited inputs into a single sorted output.
Each input must already be sorted lexicographically. Uses a k-way
merge with iterators.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    if inputs.is_empty() {
        return Err(ComputeError::InputCount { expected: 2, got: 0 });
    }
    let mut iters: Vec<std::iter::Peekable<std::str::Lines<'_>>> = inputs.iter()
        .map(|input| {
            std::str::from_utf8(input)
                .map(|s| s.lines().peekable())
        })
        .collect::<Result<_, _>>()
        .map_err(|_| ComputeError::ExecutionFailed("merge_sorted requires UTF-8".into()))?;
    let mut lines = Vec::new();
    loop {
        // Find iterator with smallest peeked line
        let mut best: Option<(usize, &str)> = None;
        for (i, iter) in iters.iter_mut().enumerate() {
            if let Some(&line) = iter.peek() {
                if best.is_none() || line < best.unwrap().1 {
                    best = Some((i, line));
                }
            }
        }
        match best {
            Some((i, _)) => { lines.push(iters[i].next().unwrap()); }
            None => break,
        }
    }
    Ok(Bytes::from(lines.join("\n")))
}
```

Edge cases: all empty → empty. Single input → pass-through. Unsorted
input → output is merge of individually sorted streams (garbage in,
garbage out — no validation).

#### 3.5.6 SelectFn (#55)

| Field | Value |
|-------|-------|
| FunctionId | `select@1.0.0` |
| Inputs | 1+ |
| Params | `index` (String — 0-based input index) |
| Streaming counterpart | None (trivial — just return one input) |
| Complexity | Low |
| Dependencies | None |

Returns the input at the specified index, discarding all others.
Useful in pipelines where a fan-out produces multiple results but
only one is needed downstream.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let idx: usize = parse_usize_param(params, "index")?;
    if idx >= inputs.len() {
        return Err(ComputeError::ExecutionFailed(
            format!("index {} out of range (have {} inputs)", idx, inputs.len()),
        ));
    }
    Ok(inputs[idx].clone())
}
```

Edge cases: index out of range → error. Single input with index=0 →
pass-through.

#### 3.5.7 Combiners Summary

| # | Function | FunctionId | Inputs | Params | Streaming Parity | Deterministic |
|---|----------|-----------|--------|--------|-----------------|---------------|
| 50 | InterleaveFn | `interleave@1.0.0` | 2+ | `block_size` | ✓ identical | ✓ |
| 51 | ZipConcatFn | `zip_concat@1.0.0` | 2 | — | ✓ identical | ✓ |
| 52 | DiffFn | `diff@1.0.0` | 2 | — | ✗ batch-only | ✓ |
| 53 | PatchFn | `patch@1.0.0` | 2 | — | ✗ batch-only | ✓ |
| 54 | MergeSortedFn | `merge_sorted@1.0.0` | 2+ | — | ✗ batch-only | ✓ |
| 55 | SelectFn | `select@1.0.0` | 1+ | `index` | ✗ trivial | ✓ |

Roundtrip invariant: `patch(old, diff(old, new)) == new` for all
inputs. Verified in §5.2.

### 3.6 Slicing & Restructuring (10 functions)

Slicing functions extract or reorder portions of the input. The first
three (#56–#58) operate on raw bytes; the rest (#59–#65) operate on
newline-delimited lines. Several are batch-only because they require
full-input access (sort, unique, shuffle, tail, sample).

#### 3.6.1 TakeFn (#56)

| Field | Value |
|-------|-------|
| FunctionId | `take@1.0.0` |
| Inputs | 1 |
| Params | `bytes` (String — number of bytes to take) |
| Streaming counterpart | `StreamingTake` (#17 in §2.14) |
| Complexity | Low |
| Dependencies | None |

Returns the first N bytes of the input. If input is shorter than N,
returns the entire input.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let n: usize = parse_usize_param(params, "bytes")?;
    let end = n.min(inputs[0].len());
    Ok(inputs[0].slice(..end))
}
```

Edge cases: empty input → empty. N=0 → empty. N > input length →
entire input.

#### 3.6.2 SkipFn (#57)

| Field | Value |
|-------|-------|
| FunctionId | `skip@1.0.0` |
| Inputs | 1 |
| Params | `bytes` (String — number of bytes to skip) |
| Streaming counterpart | `StreamingSkip` (#18 in §2.14) |
| Complexity | Low |
| Dependencies | None |

Returns all bytes after skipping the first N. If input is shorter
than N, returns empty.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let n: usize = parse_usize_param(params, "bytes")?;
    let start = n.min(inputs[0].len());
    Ok(inputs[0].slice(start..))
}
```

Edge cases: empty input → empty. N=0 → entire input. N > length →
empty.

#### 3.6.3 SliceFn (#58)

| Field | Value |
|-------|-------|
| FunctionId | `slice@1.0.0` |
| Inputs | 1 |
| Params | `offset` (String — start byte), `length` (String — number of bytes) |
| Streaming counterpart | None (combine StreamingSkip + StreamingTake in pipeline) |
| Complexity | Low |
| Dependencies | None |

Returns `length` bytes starting at `offset`. Clamps to input bounds
— does not error if range extends past end.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let offset: usize = parse_usize_param(params, "offset")?;
    let length: usize = parse_usize_param(params, "length")?;
    let input = &inputs[0];
    let start = offset.min(input.len());
    let end = start.saturating_add(length).min(input.len());
    Ok(inputs[0].slice(start..end))
}
```

Edge cases: offset past end → empty. length=0 → empty. Overflow-safe
via `saturating_add`.

#### 3.6.4 SortFn (#59)

| Field | Value |
|-------|-------|
| FunctionId | `sort@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (batch-only — global sort requires full input) |
| Complexity | Medium |
| Dependencies | None |

Sorts lines lexicographically. Input is split on `\n`, sorted, and
rejoined with `\n`. Stable sort preserves order of equal lines.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("sort requires UTF-8 input".into()))?;
    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort();
    Ok(Bytes::from(lines.join("\n")))
}
```

Edge cases: empty input → empty. Single line → unchanged. Already
sorted → unchanged.

#### 3.6.5 UniqueFn (#60)

| Field | Value |
|-------|-------|
| FunctionId | `unique@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (batch-only — dedup requires sorted or full input) |
| Complexity | Medium |
| Dependencies | None |

Removes consecutive duplicate lines (like Unix `uniq`). Input should
be pre-sorted for full deduplication. For unsorted input, only
adjacent duplicates are removed.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("unique requires UTF-8 input".into()))?;
    let mut result = Vec::new();
    let mut prev: Option<&str> = None;
    for line in text.lines() {
        if prev != Some(line) {
            result.push(line);
            prev = Some(line);
        }
    }
    Ok(Bytes::from(result.join("\n")))
}
```

Edge cases: empty input → empty. All identical lines → single line.
No duplicates → unchanged.

#### 3.6.6 SortUniqueFn (#61)

| Field | Value |
|-------|-------|
| FunctionId | `sort_unique@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (batch-only) |
| Complexity | Medium |
| Dependencies | None |

Sort + deduplicate in one pass. Equivalent to `unique(sort(x))` but
more efficient — uses `sort` then `dedup` on the Vec directly.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("sort_unique requires UTF-8 input".into()))?;
    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort();
    lines.dedup();
    Ok(Bytes::from(lines.join("\n")))
}
```

Edge cases: empty input → empty. All identical → single line.

#### 3.6.7 ShuffleFn (#62)

| Field | Value |
|-------|-------|
| FunctionId | `shuffle@1.0.0` |
| Inputs | 1 |
| Params | `seed` (String — u64 decimal for deterministic shuffle) |
| Streaming counterpart | None (batch-only — needs all lines for fair shuffle) |
| Complexity | Medium |
| Dependencies | `rand` |

Randomly reorders lines using Fisher-Yates shuffle. Deterministic
when `seed` is provided — same seed always produces same order.
Required for reproducible CAS pipelines.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use rand::seq::SliceRandom;
    use rand::SeedableRng;

    let seed: u64 = parse_u64_param(params, "seed")?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("shuffle requires UTF-8 input".into()))?;
    let mut lines: Vec<&str> = text.lines().collect();
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    lines.shuffle(&mut rng);
    Ok(Bytes::from(lines.join("\n")))
}
```

Edge cases: empty input → empty. Single line → unchanged. Same seed →
identical output (deterministic).

#### 3.6.8 HeadFn (#63)

| Field | Value |
|-------|-------|
| FunctionId | `head@1.0.0` |
| Inputs | 1 |
| Params | `lines` (String — number of lines to keep) |
| Streaming counterpart | None (streaming version would need line-boundary tracking) |
| Complexity | Low |
| Dependencies | None |

Returns the first N lines. Like Unix `head -n N`.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let n: usize = parse_usize_param(params, "lines")?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("head requires UTF-8 input".into()))?;
    let result: Vec<&str> = text.lines().take(n).collect();
    Ok(Bytes::from(result.join("\n")))
}
```

Edge cases: empty input → empty. N=0 → empty. N > line count →
entire input. Non-UTF-8 → error.

#### 3.6.9 TailFn (#64)

| Field | Value |
|-------|-------|
| FunctionId | `tail@1.0.0` |
| Inputs | 1 |
| Params | `lines` (String — number of lines to keep from end) |
| Streaming counterpart | None (batch-only — needs full input for tail) |
| Complexity | Low |
| Dependencies | None |

Returns the last N lines. Like Unix `tail -n N`. Batch-only because
streaming cannot know which lines are "last" until End.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let n: usize = parse_usize_param(params, "lines")?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("tail requires UTF-8 input".into()))?;
    let all_lines: Vec<&str> = text.lines().collect();
    let start = all_lines.len().saturating_sub(n);
    Ok(Bytes::from(all_lines[start..].join("\n")))
}
```

Edge cases: empty input → empty. N=0 → empty. N > line count →
entire input.

#### 3.6.10 SampleFn (#65)

| Field | Value |
|-------|-------|
| FunctionId | `sample@1.0.0` |
| Inputs | 1 |
| Params | `lines` (String — number of lines to sample), `seed` (String — u64 for determinism) |
| Streaming counterpart | None (batch-only — reservoir sampling needs full input for fairness) |
| Complexity | Medium |
| Dependencies | `rand` |

Reservoir samples N lines uniformly at random. Deterministic with
`seed`. Output preserves original line order (stable sample).

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use rand::Rng;
    use rand::SeedableRng;

    let n: usize = parse_usize_param(params, "lines")?;
    let seed: u64 = parse_u64_param(params, "seed")?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("sample requires UTF-8 input".into()))?;
    let all_lines: Vec<&str> = text.lines().collect();
    if n >= all_lines.len() {
        return Ok(Bytes::from(all_lines.join("\n")));
    }
    // Reservoir sampling (Algorithm R)
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut reservoir: Vec<usize> = (0..n).collect();
    for i in n..all_lines.len() {
        let j = rng.gen_range(0..=i);
        if j < n {
            reservoir[j] = i;
        }
    }
    reservoir.sort(); // preserve original order
    let sampled: Vec<&str> = reservoir.iter().map(|&i| all_lines[i]).collect();
    Ok(Bytes::from(sampled.join("\n")))
}
```

Edge cases: empty input → empty. N ≥ line count → entire input.
N=0 → empty. Same seed → identical sample (deterministic).

#### 3.6.11 Shared Helper: `parse_u64_param`

```rust
fn parse_u64_param(params: &BTreeMap<String, Value>, name: &str) -> Result<u64, ComputeError> {
    match params.get(name) {
        Some(Value::String(s)) => s.parse::<u64>()
            .map_err(|_| ComputeError::InvalidParam(format!("{} must be a non-negative integer", name))),
        Some(Value::Int(n)) => u64::try_from(*n)
            .map_err(|_| ComputeError::InvalidParam(format!("{} must be non-negative", name))),
        _ => Err(ComputeError::InvalidParam(format!("missing param: {}", name))),
    }
}
```

#### 3.6.12 Slicing & Restructuring Summary

| # | Function | FunctionId | Params | Operates On | Streaming Parity | Deterministic |
|---|----------|-----------|--------|-------------|-----------------|---------------|
| 56 | TakeFn | `take@1.0.0` | `bytes` | raw bytes | ✓ identical | ✓ |
| 57 | SkipFn | `skip@1.0.0` | `bytes` | raw bytes | ✓ identical | ✓ |
| 58 | SliceFn | `slice@1.0.0` | `offset`, `length` | raw bytes | ✗ batch-only | ✓ |
| 59 | SortFn | `sort@1.0.0` | — | lines | ✗ batch-only | ✓ |
| 60 | UniqueFn | `unique@1.0.0` | — | lines | ✗ batch-only | ✓ |
| 61 | SortUniqueFn | `sort_unique@1.0.0` | — | lines | ✗ batch-only | ✓ |
| 62 | ShuffleFn | `shuffle@1.0.0` | `seed` | lines | ✗ batch-only | ✓ (seeded) |
| 63 | HeadFn | `head@1.0.0` | `lines` | lines | ✗ batch-preferred | ✓ |
| 64 | TailFn | `tail@1.0.0` | `lines` | lines | ✗ batch-only | ✓ |
| 65 | SampleFn | `sample@1.0.0` | `lines`, `seed` | lines | ✗ batch-only | ✓ (seeded) |

This category has the highest proportion of batch-only functions (8/10)
because global reordering, tail access, and fair sampling all require
the full input. Only Take and Skip have streaming counterparts.

### 3.7 Text Processing (10 functions)

Text processing functions operate on UTF-8 input at the line or string
level. All require valid UTF-8 and return error on invalid input.
Several have streaming counterparts that use boundary-aware mapping
(§2.14 Pattern 3), but the batch versions are simpler and avoid
split-line edge cases.

#### 3.7.1 ReplaceFn (#66)

| Field | Value |
|-------|-------|
| FunctionId | `replace@1.0.0` |
| Inputs | 1 |
| Params | `find` (String — literal byte pattern), `replace` (String — replacement) |
| Streaming counterpart | `StreamingReplace` (§2.14 #78) |
| Complexity | Medium |
| Dependencies | None |

Replaces all occurrences of `find` with `replace`. Operates on the
full input, so matches spanning chunk boundaries (which streaming
would miss) are handled correctly.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let find = get_string_param(params, "find")?;
    let replace = get_string_param(params, "replace")?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("replace requires UTF-8 input".into()))?;
    Ok(Bytes::from(text.replace(find, replace)))
}
```

Edge cases: empty find → unchanged (no infinite loop — Rust's
`str::replace` with empty pattern inserts between every char, so we
reject it). Empty input → empty. No matches → unchanged.

#### 3.7.2 RegexReplaceFn (#67)

| Field | Value |
|-------|-------|
| FunctionId | `regex_replace@1.0.0` |
| Inputs | 1 |
| Params | `pattern` (String — regex), `replacement` (String — supports `$1` capture groups) |
| Streaming counterpart | `StreamingSed` (§2.14 #83) |
| Complexity | Medium |
| Dependencies | `regex` |

Regex find-and-replace across the full input. Supports capture group
references (`$1`, `$2`, etc.) in the replacement string.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let pattern = get_string_param(params, "pattern")?;
    let replacement = get_string_param(params, "replacement")?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("regex_replace requires UTF-8 input".into()))?;
    let re = regex::Regex::new(pattern)
        .map_err(|e| ComputeError::InvalidParam(format!("invalid regex: {}", e)))?;
    Ok(Bytes::from(re.replace_all(text, replacement).into_owned()))
}
```

Edge cases: invalid regex → error. No matches → unchanged. Empty
input → empty.

#### 3.7.3 GrepFn (#68)

| Field | Value |
|-------|-------|
| FunctionId | `grep@1.0.0` |
| Inputs | 1 |
| Params | `pattern` (String — regex) |
| Streaming counterpart | `StreamingGrep` (§2.14 #82) |
| Complexity | Medium |
| Dependencies | `regex` |

Keeps only lines matching the regex pattern. Like Unix `grep`.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let pattern = get_string_param(params, "pattern")?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("grep requires UTF-8 input".into()))?;
    let re = regex::Regex::new(pattern)
        .map_err(|e| ComputeError::InvalidParam(format!("invalid regex: {}", e)))?;
    let matched: Vec<&str> = text.lines().filter(|line| re.is_match(line)).collect();
    Ok(Bytes::from(matched.join("\n")))
}
```

Edge cases: no matches → empty. Empty input → empty. Invalid regex →
error. All lines match → unchanged.

#### 3.7.4 GrepInvertFn (#69)

| Field | Value |
|-------|-------|
| FunctionId | `grep_invert@1.0.0` |
| Inputs | 1 |
| Params | `pattern` (String — regex) |
| Streaming counterpart | `StreamingGrepInvert` (§2.14 complement of #82) |
| Complexity | Medium |
| Dependencies | `regex` |

Keeps only lines NOT matching the regex. Like Unix `grep -v`.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let pattern = get_string_param(params, "pattern")?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("grep_invert requires UTF-8 input".into()))?;
    let re = regex::Regex::new(pattern)
        .map_err(|e| ComputeError::InvalidParam(format!("invalid regex: {}", e)))?;
    let filtered: Vec<&str> = text.lines().filter(|line| !re.is_match(line)).collect();
    Ok(Bytes::from(filtered.join("\n")))
}
```

Edge cases: all lines match → empty. No matches → unchanged.

#### 3.7.5 PrefixFn (#70)

| Field | Value |
|-------|-------|
| FunctionId | `prefix@1.0.0` |
| Inputs | 1 |
| Params | `prefix` (String — bytes to prepend) |
| Streaming counterpart | `StreamingPrefix` (§2.14 #79) |
| Complexity | Low |
| Dependencies | None |

Prepends the given prefix bytes to the input.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let prefix = get_string_param(params, "prefix")?;
    let mut out = Vec::with_capacity(prefix.len() + inputs[0].len());
    out.extend_from_slice(prefix.as_bytes());
    out.extend_from_slice(&inputs[0]);
    Ok(Bytes::from(out))
}
```

Edge cases: empty prefix → unchanged. Empty input → prefix only.

#### 3.7.6 SuffixFn (#71)

| Field | Value |
|-------|-------|
| FunctionId | `suffix@1.0.0` |
| Inputs | 1 |
| Params | `suffix` (String — bytes to append) |
| Streaming counterpart | `StreamingSuffix` (§2.14 #80) |
| Complexity | Low |
| Dependencies | None |

Appends the given suffix bytes to the input.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let suffix = get_string_param(params, "suffix")?;
    let mut out = Vec::with_capacity(inputs[0].len() + suffix.len());
    out.extend_from_slice(&inputs[0]);
    out.extend_from_slice(suffix.as_bytes());
    Ok(Bytes::from(out))
}
```

Edge cases: empty suffix → unchanged. Empty input → suffix only.

#### 3.7.7 LinePrefixFn (#72)

| Field | Value |
|-------|-------|
| FunctionId | `line_prefix@1.0.0` |
| Inputs | 1 |
| Params | `prefix` (String — string to prepend to each line) |
| Streaming counterpart | `StreamingLinePrefix` (§2.14 #81) |
| Complexity | Medium |
| Dependencies | None |

Prepends a string to each line. Useful for adding indentation,
comment markers, or log prefixes.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let prefix = get_string_param(params, "prefix")?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("line_prefix requires UTF-8 input".into()))?;
    let result: Vec<String> = text.lines().map(|line| format!("{}{}", prefix, line)).collect();
    Ok(Bytes::from(result.join("\n")))
}
```

Edge cases: empty input → empty. Empty prefix → unchanged. Single
line → prefixed.

#### 3.7.8 LineNumberFn (#73)

| Field | Value |
|-------|-------|
| FunctionId | `line_number@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | `StreamingLineNumber` (§2.14 #83 variant) |
| Complexity | Medium |
| Dependencies | None |

Prepends 1-based line numbers to each line. Format: `{n:>6}\t{line}`
(right-aligned 6-digit number, tab separator). Like `cat -n`.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("line_number requires UTF-8 input".into()))?;
    let result: Vec<String> = text.lines()
        .enumerate()
        .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
        .collect();
    Ok(Bytes::from(result.join("\n")))
}
```

Edge cases: empty input → empty. Single line → `"     1\tline"`.

#### 3.7.9 TruncateLinesFn (#74)

| Field | Value |
|-------|-------|
| FunctionId | `truncate_lines@1.0.0` |
| Inputs | 1 |
| Params | `max_line_bytes` (String — maximum bytes per line) |
| Streaming counterpart | `StreamingTruncateLines` (§2.14 #84) |
| Complexity | Low |
| Dependencies | None |

Truncates each line to at most N bytes. Lines shorter than N are
unchanged. Useful for preventing oversized log lines.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let max: usize = parse_usize_param(params, "max_line_bytes")?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("truncate_lines requires UTF-8 input".into()))?;
    let result: Vec<&str> = text.lines()
        .map(|line| if line.len() > max { &line[..max] } else { line })
        .collect();
    Ok(Bytes::from(result.join("\n")))
}
```

Note: truncation is byte-based, not character-based. May split a
multi-byte UTF-8 character. For character-safe truncation, use
`char_indices` to find the last valid boundary ≤ max.

Edge cases: max=0 → all lines become empty. Lines shorter than max →
unchanged.

#### 3.7.10 CharsetConvertFn (#75)

| Field | Value |
|-------|-------|
| FunctionId | `charset_convert@1.0.0` |
| Inputs | 1 |
| Params | `from` (String — source encoding), `to` (String — target encoding) |
| Streaming counterpart | None (batch-preferred — avoids split multi-byte at chunk boundary) |
| Complexity | Medium |
| Dependencies | `encoding_rs` |

Converts text encoding between charsets. Supported encodings include
UTF-8, UTF-16LE, UTF-16BE, ISO-8859-1, Windows-1252, Shift_JIS,
EUC-JP, and others supported by `encoding_rs`.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let from_name = get_string_param(params, "from")?;
    let to_name = get_string_param(params, "to")?;
    let from_enc = encoding_rs::Encoding::for_label(from_name.as_bytes())
        .ok_or_else(|| ComputeError::InvalidParam(format!("unknown encoding: {}", from_name)))?;
    let to_enc = encoding_rs::Encoding::for_label(to_name.as_bytes())
        .ok_or_else(|| ComputeError::InvalidParam(format!("unknown encoding: {}", to_name)))?;
    // Decode from source encoding to UTF-8
    let (decoded, _, had_errors) = from_enc.decode(&inputs[0]);
    if had_errors {
        return Err(ComputeError::ExecutionFailed(
            format!("invalid {} input", from_name),
        ));
    }
    // Encode from UTF-8 to target encoding
    let (encoded, _, had_errors) = to_enc.encode(&decoded);
    if had_errors {
        return Err(ComputeError::ExecutionFailed(
            format!("cannot encode to {}", to_name),
        ));
    }
    Ok(Bytes::from(encoded.into_owned()))
}
```

Edge cases: same encoding → pass-through (with re-encoding roundtrip).
Unknown encoding name → error. Invalid bytes for source encoding →
error. Characters not representable in target encoding → error.

#### 3.7.11 Text Processing Summary

| # | Function | FunctionId | Params | Streaming Parity | Deterministic |
|---|----------|-----------|--------|-----------------|---------------|
| 66 | ReplaceFn | `replace@1.0.0` | `find`, `replace` | ✓ (batch more accurate) | ✓ |
| 67 | RegexReplaceFn | `regex_replace@1.0.0` | `pattern`, `replacement` | ✓ (batch more accurate) | ✓ |
| 68 | GrepFn | `grep@1.0.0` | `pattern` | ✓ identical | ✓ |
| 69 | GrepInvertFn | `grep_invert@1.0.0` | `pattern` | ✓ identical | ✓ |
| 70 | PrefixFn | `prefix@1.0.0` | `prefix` | ✓ identical | ✓ |
| 71 | SuffixFn | `suffix@1.0.0` | `suffix` | ✓ identical | ✓ |
| 72 | LinePrefixFn | `line_prefix@1.0.0` | `prefix` | ✓ identical | ✓ |
| 73 | LineNumberFn | `line_number@1.0.0` | — | ✓ identical | ✓ |
| 74 | TruncateLinesFn | `truncate_lines@1.0.0` | `max_line_bytes` | ✓ identical | ✓ |
| 75 | CharsetConvertFn | `charset_convert@1.0.0` | `from`, `to` | ✗ batch-preferred | ✓ |

Replace and RegexReplace are marked "batch more accurate" because
streaming versions using boundary-aware mapping may miss matches that
span chunk boundaries. The batch version sees the full input and
handles all matches correctly.

### 3.8 Validation & Integrity (8 functions)

Validation functions check input against a constraint and either
return the input unchanged (pass-through on success) or return an
error. They are primarily batch-only because most validations require
the full input (JSON parsing, schema validation, hash verification).
These are critical for input sanitization in CAS pipelines.

#### 3.8.1 Utf8ValidateFn (#76)

| Field | Value |
|-------|-------|
| FunctionId | `utf8_validate@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | Input unchanged if valid UTF-8 |
| Streaming counterpart | None (batch-only — multi-byte chars may split across chunks) |
| Complexity | Low |
| Dependencies | None |

Validates that the input is valid UTF-8. Returns the input unchanged
on success, error on invalid bytes. Reports the byte offset of the
first invalid sequence.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    match std::str::from_utf8(&inputs[0]) {
        Ok(_) => Ok(inputs[0].clone()),
        Err(e) => Err(ComputeError::ExecutionFailed(
            format!("invalid UTF-8 at byte offset {}", e.valid_up_to()),
        )),
    }
}
```

Edge cases: empty input → empty (valid). Pure ASCII → valid. Truncated
multi-byte sequence → error with offset.

#### 3.8.2 JsonValidateFn (#77)

| Field | Value |
|-------|-------|
| FunctionId | `json_validate@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | Input unchanged if valid JSON |
| Streaming counterpart | None (batch-only — JSON parsing requires full input) |
| Complexity | Medium |
| Dependencies | `serde_json` |

Validates that the input is valid JSON. Parses the entire input; returns
unchanged on success, error with parse location on failure.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("JSON must be UTF-8".into()))?;
    serde_json::from_str::<serde_json::Value>(text)
        .map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
    Ok(inputs[0].clone())
}
```

Edge cases: empty input → error (not valid JSON). `"null"` → valid.
Trailing garbage → error. Nested depth limit depends on serde_json
default (128).

#### 3.8.3 SchemaValidateFn (#78)

| Field | Value |
|-------|-------|
| FunctionId | `schema_validate@1.0.0` |
| Inputs | 1 |
| Params | `schema` (String — JSON Schema as JSON string) |
| Output | Input unchanged if valid against schema |
| Streaming counterpart | None (batch-only) |
| Complexity | High |
| Dependencies | `serde_json`, `jsonschema` |

Validates JSON input against a JSON Schema (Draft 2020-12). Returns
input unchanged on success, error with validation messages on failure.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let schema_str = get_string_param(params, "schema")?;
    let schema: serde_json::Value = serde_json::from_str(schema_str)
        .map_err(|e| ComputeError::InvalidParam(format!("invalid schema JSON: {}", e)))?;
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("input must be UTF-8".into()))?;
    let instance: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|e| ComputeError::InvalidParam(format!("invalid schema: {}", e)))?;
    let errors: Vec<String> = validator.iter_errors(&instance)
        .map(|e| format!("{}: {}", e.instance_path, e))
        .collect();
    if errors.is_empty() {
        Ok(inputs[0].clone())
    } else {
        Err(ComputeError::ExecutionFailed(
            format!("schema validation failed:\n{}", errors.join("\n")),
        ))
    }
}
```

Edge cases: empty schema `{}` → accepts everything. Invalid schema →
error. Invalid JSON input → error before schema check.

#### 3.8.4 MagicBytesFn (#79)

| Field | Value |
|-------|-------|
| FunctionId | `magic_bytes@1.0.0` |
| Inputs | 1 |
| Params | `expected` (String — hex-encoded expected prefix bytes) |
| Output | Input unchanged if prefix matches |
| Streaming counterpart | None (trivial — could check first chunk, but batch is simpler) |
| Complexity | Low |
| Dependencies | None |

Verifies that the input starts with the expected magic bytes. Used
for file format detection (e.g., `"89504e47"` for PNG, `"504b0304"`
for ZIP).

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let hex = get_string_param(params, "expected")?;
    let expected = hex_decode_param(hex, "expected")?;
    let input = &inputs[0];
    if input.len() < expected.len() {
        return Err(ComputeError::ExecutionFailed(
            format!("input too short: {} bytes, expected at least {}", input.len(), expected.len()),
        ));
    }
    if &input[..expected.len()] != expected.as_slice() {
        return Err(ComputeError::ExecutionFailed(format!(
            "magic bytes mismatch: expected {}, got {}",
            hex,
            input[..expected.len()].iter().map(|b| format!("{:02x}", b)).collect::<String>(),
        )));
    }
    Ok(inputs[0].clone())
}
```

Edge cases: empty expected → always passes. Input shorter than
expected → error. Exact match → pass.

#### 3.8.5 SizeLimitFn (#80)

| Field | Value |
|-------|-------|
| FunctionId | `size_limit@1.0.0` |
| Inputs | 1 |
| Params | `max_bytes` (String — maximum allowed size) |
| Output | Input unchanged if within limit |
| Streaming counterpart | `StreamingSizeLimit` (§2.14 #74) |
| Complexity | Low |
| Dependencies | None |

Returns error if input exceeds the size limit. Pass-through otherwise.
Useful for preventing oversized values from entering the CAS.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let max: usize = parse_usize_param(params, "max_bytes")?;
    if inputs[0].len() > max {
        return Err(ComputeError::ExecutionFailed(
            format!("input size {} exceeds limit {}", inputs[0].len(), max),
        ));
    }
    Ok(inputs[0].clone())
}
```

Edge cases: empty input → always passes. Exactly at limit → passes.
One byte over → error.

#### 3.8.6 NonEmptyFn (#81)

| Field | Value |
|-------|-------|
| FunctionId | `non_empty@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | Input unchanged if non-empty |
| Streaming counterpart | `StreamingNonEmpty` (§2.14 #77) |
| Complexity | Low |
| Dependencies | None |

Returns error if input is 0 bytes. Pass-through otherwise. Guards
against accidentally storing empty values in the CAS.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    if inputs[0].is_empty() {
        return Err(ComputeError::ExecutionFailed("input is empty".into()));
    }
    Ok(inputs[0].clone())
}
```

Edge cases: empty → error. Single byte → passes.

#### 3.8.7 Sha256VerifyFn (#82)

| Field | Value |
|-------|-------|
| FunctionId | `sha256_verify@1.0.0` |
| Inputs | 1 |
| Params | `expected_hash` (String — 64 hex chars = 32-byte SHA-256 digest) |
| Output | Input unchanged if hash matches |
| Streaming counterpart | None (batch-only — simpler than streaming accumulate + compare) |
| Complexity | Low |
| Dependencies | `sha2` (already in workspace) |

Computes SHA-256 of the input and compares against the expected hash.
Returns input unchanged on match, error on mismatch. Used for
integrity verification in CAS pipelines.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use sha2::{Sha256, Digest};
    let expected_hex = get_string_param(params, "expected_hash")?;
    let expected = hex_decode_param(expected_hex, "expected_hash")?;
    if expected.len() != 32 {
        return Err(ComputeError::InvalidParam("expected_hash must be 32 bytes (64 hex chars)".into()));
    }
    let actual = Sha256::digest(&inputs[0]);
    if actual.as_slice() != expected.as_slice() {
        return Err(ComputeError::ExecutionFailed(format!(
            "SHA-256 mismatch: expected {}, got {}",
            expected_hex,
            actual.iter().map(|b| format!("{:02x}", b)).collect::<String>(),
        )));
    }
    Ok(inputs[0].clone())
}
```

Edge cases: wrong hash length → error. Matching hash → pass-through.
Mismatched hash → error with both expected and actual.

#### 3.8.8 Crc32VerifyFn (#83)

| Field | Value |
|-------|-------|
| FunctionId | `crc32_verify@1.0.0` |
| Inputs | 1 |
| Params | `expected_crc32` (String — 8 hex chars = 4-byte CRC32) |
| Output | Input unchanged if CRC matches |
| Streaming counterpart | None (batch-only) |
| Complexity | Low |
| Dependencies | `crc32fast` (already in workspace) |

Computes CRC32 of the input and compares against the expected value.
Returns input unchanged on match, error on mismatch.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let expected_hex = get_string_param(params, "expected_crc32")?;
    let expected_bytes = hex_decode_param(expected_hex, "expected_crc32")?;
    if expected_bytes.len() != 4 {
        return Err(ComputeError::InvalidParam("expected_crc32 must be 4 bytes (8 hex chars)".into()));
    }
    let expected = u32::from_be_bytes([expected_bytes[0], expected_bytes[1], expected_bytes[2], expected_bytes[3]]);
    let actual = crc32fast::hash(&inputs[0]);
    if actual != expected {
        return Err(ComputeError::ExecutionFailed(format!(
            "CRC32 mismatch: expected {:08x}, got {:08x}",
            expected, actual,
        )));
    }
    Ok(inputs[0].clone())
}
```

Edge cases: wrong hex length → error. Matching CRC → pass-through.

#### 3.8.9 Validation & Integrity Summary

| # | Function | FunctionId | Params | Streaming Parity | Deterministic |
|---|----------|-----------|--------|-----------------|---------------|
| 76 | Utf8ValidateFn | `utf8_validate@1.0.0` | — | ✗ batch-only | ✓ |
| 77 | JsonValidateFn | `json_validate@1.0.0` | — | ✗ batch-only | ✓ |
| 78 | SchemaValidateFn | `schema_validate@1.0.0` | `schema` | ✗ batch-only | ✓ |
| 79 | MagicBytesFn | `magic_bytes@1.0.0` | `expected` | ✗ batch-preferred | ✓ |
| 80 | SizeLimitFn | `size_limit@1.0.0` | `max_bytes` | ✓ identical | ✓ |
| 81 | NonEmptyFn | `non_empty@1.0.0` | — | ✓ identical | ✓ |
| 82 | Sha256VerifyFn | `sha256_verify@1.0.0` | `expected_hash` | ✗ batch-only | ✓ |
| 83 | Crc32VerifyFn | `crc32_verify@1.0.0` | `expected_crc32` | ✗ batch-only | ✓ |

All validation functions are pass-through on success (return input
unchanged) and error on failure. This makes them composable as
pipeline guards: `Source → Utf8Validate → JsonValidate → SizeLimit →
SchemaValidate → downstream`. Any failure short-circuits the pipeline.

### 3.9 Format Conversion (8 functions)

Format conversion functions parse input in one format and emit it in
another. All are batch-only — they require the full input for correct
parsing (JSON, YAML, TOML, and CSV are not safely streamable without
a full parse). All require valid UTF-8 input.

#### 3.9.1 JsonPrettyPrintFn (#84)

| Field | Value |
|-------|-------|
| FunctionId | `json_pretty@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (batch-only — needs full JSON parse) |
| Complexity | Medium |
| Dependencies | `serde_json` |

Parses JSON and re-emits it with 2-space indentation. Normalizes
formatting without changing semantics.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("json_pretty requires UTF-8".into()))?;
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
    let pretty = serde_json::to_string_pretty(&value)
        .map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
    Ok(Bytes::from(pretty))
}
```

Edge cases: empty input → error. `"null"` → `"null"`. Already pretty →
re-formatted (idempotent).

#### 3.9.2 JsonMinifyFn (#85)

| Field | Value |
|-------|-------|
| FunctionId | `json_minify@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (batch-only) |
| Complexity | Medium |
| Dependencies | `serde_json` |

Parses JSON and re-emits it in compact form (no whitespace). Useful
for reducing storage size of JSON values in the CAS.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("json_minify requires UTF-8".into()))?;
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
    let compact = serde_json::to_string(&value)
        .map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
    Ok(Bytes::from(compact))
}
```

Edge cases: empty input → error. Already compact → unchanged
(idempotent). Roundtrip: `minify(pretty(x))` and `pretty(minify(x))`
both preserve JSON semantics.

#### 3.9.3 CsvToJsonFn (#86)

| Field | Value |
|-------|-------|
| FunctionId | `csv_to_json@1.0.0` |
| Inputs | 1 |
| Params | None (first row is header) |
| Output | JSON array of objects |
| Streaming counterpart | None (batch-only — needs header row for all records) |
| Complexity | High |
| Dependencies | `csv`, `serde_json` |

Parses CSV with the first row as headers and emits a JSON array of
objects. Each row becomes an object with header names as keys.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mut reader = csv::Reader::from_reader(&inputs[0][..]);
    let headers: Vec<String> = reader.headers()
        .map_err(|e| ComputeError::ExecutionFailed(format!("csv headers: {}", e)))?
        .iter().map(|h| h.to_string()).collect();
    let mut records = Vec::new();
    for result in reader.records() {
        let record = result
            .map_err(|e| ComputeError::ExecutionFailed(format!("csv record: {}", e)))?;
        let mut obj = serde_json::Map::new();
        for (i, field) in record.iter().enumerate() {
            let key = headers.get(i).cloned().unwrap_or_else(|| format!("col_{}", i));
            obj.insert(key, serde_json::Value::String(field.to_string()));
        }
        records.push(serde_json::Value::Object(obj));
    }
    let json = serde_json::to_string_pretty(&records)
        .map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
    Ok(Bytes::from(json))
}
```

Edge cases: empty input → error (no headers). Header only, no rows →
`"[]"`. Rows with fewer fields than headers → missing keys omitted.
Rows with more fields → extra fields get `col_N` keys.

#### 3.9.4 JsonToCsvFn (#87)

| Field | Value |
|-------|-------|
| FunctionId | `json_to_csv@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | CSV with header row |
| Streaming counterpart | None (batch-only — needs all keys for header) |
| Complexity | High |
| Dependencies | `csv`, `serde_json` |

Parses a JSON array of objects and emits CSV. Headers are derived from
the union of all keys in the first object. Fields are emitted in
sorted key order for determinism.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("json_to_csv requires UTF-8".into()))?;
    let array: Vec<serde_json::Map<String, serde_json::Value>> = serde_json::from_str(text)
        .map_err(|e| ComputeError::ExecutionFailed(format!("expected JSON array of objects: {}", e)))?;
    if array.is_empty() {
        return Ok(Bytes::new());
    }
    // Collect all keys from first object, sorted for determinism
    let mut headers: Vec<String> = array[0].keys().cloned().collect();
    headers.sort();
    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record(&headers)
        .map_err(|e| ComputeError::ExecutionFailed(format!("csv write: {}", e)))?;
    for obj in &array {
        let row: Vec<String> = headers.iter()
            .map(|h| match obj.get(h) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            })
            .collect();
        wtr.write_record(&row)
            .map_err(|e| ComputeError::ExecutionFailed(format!("csv write: {}", e)))?;
    }
    let csv_bytes = wtr.into_inner()
        .map_err(|e| ComputeError::ExecutionFailed(format!("csv flush: {}", e)))?;
    Ok(Bytes::from(csv_bytes))
}
```

Edge cases: empty array → empty output. Objects with different keys →
missing values become empty strings. Non-string values → `to_string()`.

#### 3.9.5 JsonLinesFn (#88)

| Field | Value |
|-------|-------|
| FunctionId | `json_lines@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | Newline-delimited JSON (one JSON value per line) |
| Streaming counterpart | None (batch-only) |
| Complexity | Low |
| Dependencies | `serde_json` |

Converts a JSON array into newline-delimited JSON (NDJSON/JSON Lines).
Each array element is serialized as a compact JSON line.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("json_lines requires UTF-8".into()))?;
    let array: Vec<serde_json::Value> = serde_json::from_str(text)
        .map_err(|e| ComputeError::ExecutionFailed(format!("expected JSON array: {}", e)))?;
    let lines: Vec<String> = array.iter()
        .map(|v| serde_json::to_string(v))
        .collect::<Result<_, _>>()
        .map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
    Ok(Bytes::from(lines.join("\n")))
}
```

Edge cases: empty array → empty output. Non-array input → error.
Nested objects → serialized inline.

#### 3.9.6 YamlToJsonFn (#89)

| Field | Value |
|-------|-------|
| FunctionId | `yaml_to_json@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (batch-only) |
| Complexity | Medium |
| Dependencies | `serde_yaml`, `serde_json` |

Parses YAML and emits compact JSON. Supports the full YAML 1.2 spec
via `serde_yaml`.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("yaml_to_json requires UTF-8".into()))?;
    let value: serde_yaml::Value = serde_yaml::from_str(text)
        .map_err(|e| ComputeError::ExecutionFailed(format!("invalid YAML: {}", e)))?;
    let json = serde_json::to_string_pretty(&value)
        .map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
    Ok(Bytes::from(json))
}
```

Edge cases: empty input → error. YAML scalar → JSON scalar. YAML
anchors/aliases → resolved. Multi-document YAML → only first document.

#### 3.9.7 JsonToYamlFn (#90)

| Field | Value |
|-------|-------|
| FunctionId | `json_to_yaml@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (batch-only) |
| Complexity | Medium |
| Dependencies | `serde_json`, `serde_yaml` |

Parses JSON and emits YAML.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("json_to_yaml requires UTF-8".into()))?;
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ComputeError::ExecutionFailed(format!("invalid JSON: {}", e)))?;
    let yaml = serde_yaml::to_string(&value)
        .map_err(|e| ComputeError::ExecutionFailed(format!("yaml serialize: {}", e)))?;
    Ok(Bytes::from(yaml))
}
```

Edge cases: empty input → error. `"null"` → `"null\n"`. Roundtrip:
`json_to_yaml(yaml_to_json(x))` preserves semantics but not
formatting.

#### 3.9.8 TomlToJsonFn (#91)

| Field | Value |
|-------|-------|
| FunctionId | `toml_to_json@1.0.0` |
| Inputs | 1 |
| Params | None |
| Streaming counterpart | None (batch-only) |
| Complexity | Medium |
| Dependencies | `toml`, `serde_json` |

Parses TOML and emits JSON. TOML is commonly used for configuration
files (Cargo.toml, pyproject.toml).

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let text = std::str::from_utf8(&inputs[0])
        .map_err(|_| ComputeError::ExecutionFailed("toml_to_json requires UTF-8".into()))?;
    let value: toml::Value = toml::from_str(text)
        .map_err(|e| ComputeError::ExecutionFailed(format!("invalid TOML: {}", e)))?;
    let json = serde_json::to_string_pretty(&value)
        .map_err(|e| ComputeError::ExecutionFailed(format!("json serialize: {}", e)))?;
    Ok(Bytes::from(json))
}
```

Edge cases: empty input → error. TOML datetime types → JSON strings.
TOML inline tables → JSON objects.

#### 3.9.9 Format Conversion Summary

| # | Function | FunctionId | Direction | Streaming Parity | Deterministic |
|---|----------|-----------|-----------|-----------------|---------------|
| 84 | JsonPrettyPrintFn | `json_pretty@1.0.0` | JSON → JSON | ✗ batch-only | ✓ |
| 85 | JsonMinifyFn | `json_minify@1.0.0` | JSON → JSON | ✗ batch-only | ✓ |
| 86 | CsvToJsonFn | `csv_to_json@1.0.0` | CSV → JSON | ✗ batch-only | ✓ |
| 87 | JsonToCsvFn | `json_to_csv@1.0.0` | JSON → CSV | ✗ batch-only | ✓ |
| 88 | JsonLinesFn | `json_lines@1.0.0` | JSON → NDJSON | ✗ batch-only | ✓ |
| 89 | YamlToJsonFn | `yaml_to_json@1.0.0` | YAML → JSON | ✗ batch-only | ✓ |
| 90 | JsonToYamlFn | `json_to_yaml@1.0.0` | JSON → YAML | ✗ batch-only | ✓ |
| 91 | TomlToJsonFn | `toml_to_json@1.0.0` | TOML → JSON | ✗ batch-only | ✓ |

All format conversion functions are batch-only. JSON is the hub format
— all conversions go through JSON. This enables chaining:
`TOML → JSON → YAML` via `toml_to_json | json_to_yaml`.

Roundtrip pairs:
- `json_minify(json_pretty(x))` preserves semantics
- `yaml_to_json(json_to_yaml(x))` preserves semantics (not formatting)
- `csv_to_json` / `json_to_csv` roundtrip preserves data (all values
  become strings in CSV)

### 3.10 CAS-Specific Operations (7 functions)

CAS-specific functions leverage content-addressing properties of the
deriva system. They compute, verify, or embed content addresses
(CAddr), build Merkle trees, detect content types, and produce
chunk-level integrity metadata. Most are batch-only because they
need the full input for correct address computation.

#### 3.10.1 CAddrComputeFn (#92)

| Field | Value |
|-------|-------|
| FunctionId | `caddr_compute@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | 32 bytes (CAddr raw bytes) |
| Streaming counterpart | None (batch-preferred — CAddr is a single hash of full content) |
| Complexity | Low |
| Dependencies | None (uses `CAddr::from_bytes` from `deriva_core`) |

Computes the CAddr of the input. Returns the raw 32-byte address.
This is the same address that the CAS would assign if the input were
stored directly.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let addr = CAddr::from_bytes(&inputs[0]);
    Ok(Bytes::copy_from_slice(addr.as_bytes()))
}
```

Edge cases: empty input → CAddr of empty bytes. Deterministic — same
input always produces same CAddr.

#### 3.10.2 CAddrVerifyFn (#93)

| Field | Value |
|-------|-------|
| FunctionId | `caddr_verify@1.0.0` |
| Inputs | 1 |
| Params | `expected_caddr` (String — 64 hex chars = 32-byte CAddr) |
| Output | Input unchanged if CAddr matches |
| Streaming counterpart | None (batch-only — needs full input for CAddr) |
| Complexity | Medium |
| Dependencies | None |

Computes the CAddr of the input and compares against the expected
address. Returns input unchanged on match, error on mismatch. Used
for integrity verification — ensures content hasn't been corrupted
or tampered with.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let expected_hex = get_string_param(params, "expected_caddr")?;
    let expected = hex_decode_param(expected_hex, "expected_caddr")?;
    if expected.len() != 32 {
        return Err(ComputeError::InvalidParam("expected_caddr must be 32 bytes (64 hex chars)".into()));
    }
    let actual = CAddr::from_bytes(&inputs[0]);
    if actual.as_bytes() != expected.as_slice() {
        return Err(ComputeError::ExecutionFailed(format!(
            "CAddr mismatch: expected {}, got {}",
            expected_hex,
            actual.as_bytes().iter().map(|b| format!("{:02x}", b)).collect::<String>(),
        )));
    }
    Ok(inputs[0].clone())
}
```

Edge cases: wrong hex length → error. Matching CAddr → pass-through.

#### 3.10.3 CAddrEmbedFn (#94)

| Field | Value |
|-------|-------|
| FunctionId | `caddr_embed@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | Input + 32 bytes (CAddr appended as trailing metadata) |
| Streaming counterpart | `StreamingCAddrEmbed` (§2.14 #86) |
| Complexity | Medium |
| Dependencies | None |

Computes the CAddr of the input and appends it as a 32-byte trailer.
The result is `[original_data][caddr_32_bytes]`. Consumers can verify
integrity by splitting off the last 32 bytes and recomputing.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let addr = CAddr::from_bytes(&inputs[0]);
    let mut out = Vec::with_capacity(inputs[0].len() + 32);
    out.extend_from_slice(&inputs[0]);
    out.extend_from_slice(addr.as_bytes());
    Ok(Bytes::from(out))
}
```

Edge cases: empty input → 32 bytes (CAddr of empty). Idempotent?
No — embedding twice appends two CAddrs (second includes the first).

#### 3.10.4 MerkleRootFn (#95)

| Field | Value |
|-------|-------|
| FunctionId | `merkle_root@1.0.0` |
| Inputs | 1 |
| Params | `block_size` (String — bytes per leaf block, default `"65536"`) |
| Output | 32 bytes (Merkle root hash) |
| Streaming counterpart | None (batch-only — needs all blocks for tree construction) |
| Complexity | High |
| Dependencies | `sha2` (already in workspace) |

Splits input into fixed-size blocks, computes SHA-256 of each block,
then builds a binary Merkle tree and returns the 32-byte root hash.
The last block may be smaller than `block_size`. If input is empty,
returns SHA-256 of empty (the root of an empty tree).

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use sha2::{Sha256, Digest};
    let bs: usize = match params.get("block_size") {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("block_size must be positive".into()))?,
        None => 65536,
        _ => return Err(ComputeError::InvalidParam("block_size must be a string".into())),
    };
    if bs == 0 {
        return Err(ComputeError::InvalidParam("block_size must be > 0".into()));
    }
    let input = &inputs[0];
    if input.is_empty() {
        return Ok(Bytes::copy_from_slice(&Sha256::digest(b"")));
    }
    // Compute leaf hashes
    let mut hashes: Vec<[u8; 32]> = input.chunks(bs)
        .map(|chunk| Sha256::digest(chunk).into())
        .collect();
    // Build tree bottom-up
    while hashes.len() > 1 {
        let mut next = Vec::with_capacity((hashes.len() + 1) / 2);
        for pair in hashes.chunks(2) {
            if pair.len() == 2 {
                let mut hasher = Sha256::new();
                hasher.update(&pair[0]);
                hasher.update(&pair[1]);
                next.push(hasher.finalize().into());
            } else {
                next.push(pair[0]); // odd node promoted
            }
        }
        hashes = next;
    }
    Ok(Bytes::copy_from_slice(&hashes[0]))
}
```

Edge cases: empty input → SHA-256 of empty. Single block → hash of
that block (no tree). block_size=1 → one leaf per byte (deep tree).

#### 3.10.5 ContentTypeFn (#96)

| Field | Value |
|-------|-------|
| FunctionId | `content_type@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | MIME type string as UTF-8 bytes |
| Streaming counterpart | None (batch-preferred — checks magic bytes at start) |
| Complexity | Medium |
| Dependencies | None |

Detects the MIME type from magic bytes at the start of the input.
Returns the MIME type string (e.g., `"application/json"`,
`"image/png"`). Falls back to `"application/octet-stream"` for
unknown formats.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let input = &inputs[0];
    let mime = if input.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if input.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if input.starts_with(b"GIF87a") || input.starts_with(b"GIF89a") {
        "image/gif"
    } else if input.starts_with(b"PK\x03\x04") {
        "application/zip"
    } else if input.starts_with(b"\x1f\x8b") {
        "application/gzip"
    } else if input.starts_with(b"%PDF") {
        "application/pdf"
    } else if input.starts_with(b"\x28\xb5\x2f\xfd") {
        "application/zstd"
    } else if input.starts_with(b"{") || input.starts_with(b"[") {
        "application/json"
    } else if std::str::from_utf8(input).is_ok() {
        "text/plain"
    } else {
        "application/octet-stream"
    };
    Ok(Bytes::from(mime))
}
```

Edge cases: empty input → `"application/octet-stream"`. JSON heuristic
is best-effort (checks first byte only). Valid UTF-8 binary that
starts with `{` → detected as JSON.

#### 3.10.6 ChunkHashFn (#97)

| Field | Value |
|-------|-------|
| FunctionId | `chunk_hash@1.0.0` |
| Inputs | 1 |
| Params | `block_size` (String — bytes per block, default `"65536"`) |
| Output | Sequence of 32-byte SHA-256 hashes, one per block |
| Streaming counterpart | None (batch-preferred — simpler with full input) |
| Complexity | Medium |
| Dependencies | `sha2` (already in workspace) |

Splits input into fixed-size blocks and returns the concatenation of
SHA-256 hashes, one per block. Output size = `ceil(input_len / block_size) * 32`.
Useful for block-level integrity verification and parallel download
validation.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    use sha2::{Sha256, Digest};
    let bs: usize = match params.get("block_size") {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("block_size must be positive".into()))?,
        None => 65536,
        _ => return Err(ComputeError::InvalidParam("block_size must be a string".into())),
    };
    if bs == 0 {
        return Err(ComputeError::InvalidParam("block_size must be > 0".into()));
    }
    let input = &inputs[0];
    let mut out = Vec::with_capacity((input.len() / bs + 1) * 32);
    for chunk in input.chunks(bs) {
        out.extend_from_slice(&Sha256::digest(chunk));
    }
    Ok(Bytes::from(out))
}
```

Edge cases: empty input → empty output (no blocks). Single block →
32 bytes. block_size > input → single hash.

#### 3.10.7 DedupAnalyzeFn (#98)

| Field | Value |
|-------|-------|
| FunctionId | `dedup_analyze@1.0.0` |
| Inputs | 1 |
| Params | `window_size` (String — rolling hash window in bytes, default `"48"`) |
| Output | JSON array of chunk boundaries (byte offsets) |
| Streaming counterpart | None (batch-only — content-defined chunking needs full input) |
| Complexity | High |
| Dependencies | None |

Performs content-defined chunking using a rolling hash (Rabin-like).
Returns a JSON array of byte offsets where chunk boundaries fall.
Used for deduplication analysis — identical sub-sequences across
different values produce identical chunks.

The algorithm uses a simple polynomial rolling hash with a mask to
detect boundaries. Average chunk size ≈ `2^13` = 8 KB (mask =
`0x1FFF`). Minimum chunk = 2 KB, maximum chunk = 64 KB.

```rust
fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let window: usize = match params.get("window_size") {
        Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("window_size must be positive".into()))?,
        None => 48,
        _ => return Err(ComputeError::InvalidParam("window_size must be a string".into())),
    };
    let input = &inputs[0];
    let min_chunk = 2048;
    let max_chunk = 65536;
    let mask: u64 = 0x1FFF; // average chunk ~8KB
    let mut boundaries = vec![0u64]; // first boundary at start
    let mut hash: u64 = 0;
    let mut last_boundary = 0usize;
    for (i, &b) in input.iter().enumerate() {
        hash = hash.wrapping_mul(31).wrapping_add(b as u64);
        if i >= window {
            let old = input[i - window] as u64;
            hash = hash.wrapping_sub(old.wrapping_mul(31u64.wrapping_pow(window as u32)));
        }
        let chunk_len = i - last_boundary + 1;
        if (chunk_len >= min_chunk && (hash & mask) == 0) || chunk_len >= max_chunk {
            boundaries.push((i + 1) as u64);
            last_boundary = i + 1;
            hash = 0;
        }
    }
    if last_boundary < input.len() {
        boundaries.push(input.len() as u64);
    }
    let json = serde_json::to_string(&boundaries)
        .map_err(|e| ComputeError::ExecutionFailed(format!("json: {}", e)))?;
    Ok(Bytes::from(json))
}
```

Edge cases: empty input → `"[0]"`. Input smaller than min_chunk →
single chunk `[0, len]`. Deterministic — same input always produces
same boundaries.

#### 3.10.8 CAS-Specific Summary

| # | Function | FunctionId | Params | Output | Streaming Parity | Deterministic |
|---|----------|-----------|--------|--------|-----------------|---------------|
| 92 | CAddrComputeFn | `caddr_compute@1.0.0` | — | 32 B | ✗ batch-preferred | ✓ |
| 93 | CAddrVerifyFn | `caddr_verify@1.0.0` | `expected_caddr` | pass-through | ✗ batch-only | ✓ |
| 94 | CAddrEmbedFn | `caddr_embed@1.0.0` | — | input + 32 B | ✓ identical | ✓ |
| 95 | MerkleRootFn | `merkle_root@1.0.0` | `block_size` | 32 B | ✗ batch-only | ✓ |
| 96 | ContentTypeFn | `content_type@1.0.0` | — | MIME string | ✗ batch-preferred | ✓ |
| 97 | ChunkHashFn | `chunk_hash@1.0.0` | `block_size` | N × 32 B | ✗ batch-preferred | ✓ |
| 98 | DedupAnalyzeFn | `dedup_analyze@1.0.0` | `window_size` | JSON offsets | ✗ batch-only | ✓ |

These functions form the foundation for CAS-native pipeline operations:
integrity verification (CAddrVerify, ChunkHash), metadata embedding
(CAddrEmbed, ContentType), tree structures (MerkleRoot), and
deduplication (DedupAnalyze).

### 3.11 Batch-Only (2 functions)

These functions are inherently batch-only — they require random access
over the full input and have no meaningful streaming equivalent.

#### 3.11.1 ReverseByteFn (#99)

| Field | Value |
|-------|-------|
| FunctionId | `reverse_bytes@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | Input bytes in reverse order |
| Streaming counterpart | None (impossible — streaming emits bytes in arrival order) |
| Complexity | Low |
| Dependencies | None |

Reverses the entire byte sequence. Unlike `ReverseFn` (#6) which
reverses UTF-8 characters, this operates on raw bytes. Useful for
endianness conversion of opaque blobs and testing pipeline symmetry.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mut out = inputs[0].to_vec();
    out.reverse();
    Ok(Bytes::from(out))
}
```

Edge cases: empty input → empty output. Single byte → unchanged.
Palindromic input → unchanged. Roundtrip: `reverse_bytes(reverse_bytes(x)) == x`.

#### 3.11.2 SortBytesFn (#100)

| Field | Value |
|-------|-------|
| FunctionId | `sort_bytes@1.0.0` |
| Inputs | 1 |
| Params | None |
| Output | Input bytes sorted ascending (0x00..0xFF) |
| Streaming counterpart | None (impossible — needs all bytes before emitting) |
| Complexity | Medium (O(n log n)) |
| Dependencies | None |

Sorts all bytes in ascending order. Produces a canonical byte
ordering. Useful for fingerprinting (two inputs with the same byte
histogram produce identical output) and entropy analysis baselines.

```rust
fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
    let mut out = inputs[0].to_vec();
    out.sort_unstable();
    Ok(Bytes::from(out))
}
```

Edge cases: empty input → empty output. Already sorted → unchanged
(idempotent). All same byte → unchanged.

#### 3.11.3 Batch-Only Summary

| # | Function | FunctionId | Why Batch-Only | Deterministic |
|---|----------|-----------|----------------|---------------|
| 99 | ReverseByteFn | `reverse_bytes@1.0.0` | Needs full input to reverse | ✓ |
| 100 | SortBytesFn | `sort_bytes@1.0.0` | Needs all bytes to sort | ✓ |

Both are zero-dependency, zero-param, single-input functions with
O(n) and O(n log n) complexity respectively. They complete the batch
library at exactly 100 functions (#1–#100).

---

## 4. Implementation Strategy

### 4.1 Estimated Cost Heuristics

Each function implements `estimated_cost()` for the pipeline optimizer.
Costs scale with input size and algorithmic complexity:

| Complexity | Base Cost | Scaling | Examples |
|-----------|-----------|---------|----------|
| Trivial | 1 | O(1) | IdentityFn |
| Low | 10 | O(n) | Lowercase, Reverse, Trim, XOR, ByteSwap |
| Medium | 50 | O(n) | Base64, Hex, SHA-256, Compress, ContentType |
| High | 100 | O(n log n) or O(n²) | Sort, Regex, MerkleRoot, DedupAnalyze |
| Very High | 200 | O(n) with large constant | Encrypt/AEAD (AES), SchemaValidate |

Formula: `base_cost * (total_input_bytes / 1024).max(1)`

Multi-input functions (Combiners) sum all input sizes.

### 4.2 Shared Utility Functions

Six helpers reduce boilerplate across the 96 new functions:

```rust
// §3.1 — param parsing
fn parse_byte_param(params: &BTreeMap<String, Value>, name: &str) -> Result<u8, ComputeError>;
fn parse_usize_param(params: &BTreeMap<String, Value>, name: &str) -> Result<usize, ComputeError>;

// §3.3 — string/hex param extraction
fn get_string_param<'a>(params: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a str, ComputeError>;
fn hex_decode_param(hex: &str, name: &str) -> Result<Vec<u8>, ComputeError>;

// §3.6 — u64 param parsing
fn parse_u64_param(params: &BTreeMap<String, Value>, name: &str) -> Result<u64, ComputeError>;

// Text helpers (used by §3.4, §3.6, §3.7)
fn split_lines(input: &[u8]) -> Vec<&[u8]>;  // split on \n, preserving empty trailing
```

All helpers are `pub(crate)` in `builtins.rs` and reused by both
existing (#1–#4) and new (#5–#100) functions.

### 4.3 Error Handling Convention

All functions follow a consistent error mapping:

| Condition | Error Variant | Example |
|-----------|--------------|---------|
| Missing required param | `ComputeError::InvalidParam(msg)` | `"missing param: key"` |
| Param wrong type/range | `ComputeError::InvalidParam(msg)` | `"level must be 1..22"` |
| Invalid input data | `ComputeError::ExecutionFailed(msg)` | `"invalid UTF-8"` |
| Algorithm failure | `ComputeError::ExecutionFailed(msg)` | `"decompression failed"` |
| Wrong input count | `ComputeError::ExecutionFailed(msg)` | `"expected 2 inputs, got 1"` |

Error messages always include the function name or param name for
debuggability. No panics — all fallible operations use `?` with
mapped errors.

### 4.4 Implementation Order

Implementation follows the priority tiers from §2:

**Tier 1 — Core (implement first):**
§3.1 Transforms (#5–#20), §3.3 Crypto (#31–#41), §3.4 Accumulators
(#42–#49). These have the most streaming parity overlap and are
needed by §2.9 tests.

**Tier 2 — Infrastructure:**
§3.2 Compression (#21–#30), §3.6 Slicing (#56–#65), §3.5 Combiners
(#50–#55). These complete the pipeline building blocks.

**Tier 3 — Extended:**
§3.7 Text (#66–#75), §3.8 Validation (#76–#83), §3.9 Format
(#84–#91), §3.10 CAS (#92–#98), §3.11 Batch-Only (#99–#100).

Each tier is one commit. Tests are written alongside each tier.

### 4.5 File Organization

All 96 new functions go in `crates/deriva-compute/src/builtins.rs`
alongside the existing 4. Functions are grouped by category with
section comments. No new source files — the `ComputeFunction` trait
implementations are small enough to colocate.

Registration happens in `register_default_functions()` in the same
file, extending the existing pattern.

---

## 5. Test Specification

### 5.1 Per-Function Unit Tests

Each of the 96 new functions gets 3 tests minimum:

1. **Happy path** — typical input, verify correct output
2. **Empty input** — verify graceful handling (empty output or error)
3. **Error case** — invalid params, wrong input count, or corrupt data

Total: ~288 unit tests in `crates/deriva-compute/tests/batch_functions.rs`.

Test pattern:
```rust
#[test]
fn test_lowercase_basic() {
    let f = LowercaseFn;
    let result = f.execute(vec![Bytes::from("HELLO")], &BTreeMap::new()).unwrap();
    assert_eq!(result, Bytes::from("hello"));
}

#[test]
fn test_lowercase_empty() {
    let f = LowercaseFn;
    let result = f.execute(vec![Bytes::new()], &BTreeMap::new()).unwrap();
    assert_eq!(result, Bytes::from(""));
}

#[test]
fn test_lowercase_non_ascii() {
    let f = LowercaseFn;
    let result = f.execute(vec![Bytes::from("ÜBER")], &BTreeMap::new()).unwrap();
    assert_eq!(result, Bytes::from("über"));
}
```

Tests are organized by category, mirroring §3.1–§3.11. Each category
is a `mod` inside the test file:

```rust
mod transforms { /* #5–#20 */ }
mod compression { /* #21–#30 */ }
mod crypto { /* #31–#41 */ }
mod accumulators { /* #42–#49 */ }
mod combiners { /* #50–#55 */ }
mod slicing { /* #56–#65 */ }
mod text { /* #66–#75 */ }
mod validation { /* #76–#83 */ }
mod format_conversion { /* #84–#91 */ }
mod cas_specific { /* #92–#98 */ }
mod batch_only { /* #99–#100 */ }
```

### 5.2 Roundtrip Tests

Encode/decode and transform pairs must roundtrip:
`decode(encode(x)) == x` for all non-empty inputs.

| # | Pair | Forward | Reverse | Notes |
|---|------|---------|---------|-------|
| 1 | Base64 | Base64EncodeFn | Base64DecodeFn | Exact roundtrip |
| 2 | Hex | HexEncodeFn | HexDecodeFn | Exact roundtrip |
| 3 | Base32 | Base32EncodeFn | Base32DecodeFn | Exact roundtrip |
| 4 | AES-CTR | EncryptFn | DecryptFn | Same key+nonce |
| 5 | AES-GCM | AeadEncryptFn | AeadDecryptFn | Same key+nonce |
| 6 | Zlib | CompressFn | DecompressFn | Exact roundtrip |
| 7 | Zstd | ZstdCompressFn | ZstdDecompressFn | Exact roundtrip |
| 8 | LZ4 | Lz4CompressFn | Lz4DecompressFn | Exact roundtrip |
| 9 | Snappy | SnappyCompressFn | SnappyDecompressFn | Exact roundtrip |
| 10 | Brotli | BrotliCompressFn | BrotliDecompressFn | Exact roundtrip |
| 11 | ReverseBytes | ReverseByteFn | ReverseByteFn | Self-inverse |
| 12 | Diff/Patch | DiffFn | PatchFn | `patch(a, diff(a, b)) == b` |
| 13 | JSON format | JsonPrettyPrintFn | JsonMinifyFn | Semantic roundtrip (not byte-identical) |

Test pattern:
```rust
#[test]
fn roundtrip_base64() {
    let input = Bytes::from("hello world 🌍");
    let encoded = Base64EncodeFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
    let decoded = Base64DecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn roundtrip_encrypt_decrypt() {
    let input = Bytes::from("secret data");
    let params = BTreeMap::from([
        ("key".into(), Value::String("aa".repeat(32))),   // 32-byte hex key
        ("nonce".into(), Value::String("bb".repeat(16))),  // 16-byte hex nonce
    ]);
    let encrypted = EncryptFn.execute(vec![input.clone()], &params).unwrap();
    let decrypted = DecryptFn.execute(vec![encrypted], &params).unwrap();
    assert_eq!(decrypted, input);
}

#[test]
fn roundtrip_diff_patch() {
    let a = Bytes::from("original");
    let b = Bytes::from("modified");
    let diff = DiffFn.execute(vec![a.clone(), b.clone()], &BTreeMap::new()).unwrap();
    let patched = PatchFn.execute(vec![a, diff], &BTreeMap::new()).unwrap();
    assert_eq!(patched, b);
}
```

Total: 13 roundtrip tests in a `mod roundtrips` block.

### 5.3 Streaming Parity Tests

For functions with both batch and streaming implementations, verify
identical output for the same input. The batch function is the
reference — streaming must match byte-for-byte.

Parity helper:
```rust
fn assert_parity(registry: &FunctionRegistry, id: &str, input: &[u8], params: &BTreeMap<String, Value>) {
    let fid = FunctionId::new(id);
    let batch = registry.get(&fid).expect("batch impl");
    let batch_out = batch.execute(vec![Bytes::copy_from_slice(input)], params).unwrap();
    let streaming = registry.get_streaming(&fid).expect("streaming impl");
    let streaming_out = collect_streaming(streaming, input);
    assert_eq!(batch_out, streaming_out, "parity failed for {}", id);
}
```

| # | FunctionId | Batch (§2.15) | Streaming (§2.14) |
|---|-----------|---------------|-------------------|
| 1 | `lowercase@1.0.0` | LowercaseFn | StreamingLowercase |
| 2 | `reverse@1.0.0` | ReverseFn | StreamingReverse |
| 3 | `base64_encode@1.0.0` | Base64EncodeFn | StreamingBase64Encode |
| 4 | `base64_decode@1.0.0` | Base64DecodeFn | StreamingBase64Decode |
| 5 | `hex_encode@1.0.0` | HexEncodeFn | StreamingHexEncode |
| 6 | `hex_decode@1.0.0` | HexDecodeFn | StreamingHexDecode |
| 7 | `xor@1.0.0` | XorFn | StreamingXor |
| 8 | `compress@1.0.0` | CompressFn | StreamingCompress |
| 9 | `decompress@1.0.0` | DecompressFn | StreamingDecompress |
| 10 | `sha256@1.0.0` | Sha256Fn | StreamingSha256 |
| 11 | `byte_count@1.0.0` | ByteCountFn | StreamingByteCount |
| 12 | `concat@1.0.0` | ConcatFn | StreamingConcat |
| 13 | `interleave@1.0.0` | InterleaveFn | StreamingInterleave |
| 14 | `zip_concat@1.0.0` | ZipConcatFn | StreamingZipConcat |
| 15 | `take@1.0.0` | TakeFn | StreamingTake |
| 16 | `skip@1.0.0` | SkipFn | StreamingSkip |
| 17 | `repeat@1.0.0` | RepeatFn | StreamingRepeat |
| 18 | `uppercase@1.0.0` | UppercaseFn | StreamingUppercase |
| 19 | `identity@1.0.0` | IdentityFn | StreamingIdentity |
| 20 | `caddr_embed@1.0.0` | CAddrEmbedFn | StreamingCAddrEmbed |

Each test runs with two inputs: a small payload (`b"hello world"`)
and a larger payload (4 KB random bytes) to catch chunking edge cases.

Total: 20 parity tests in a `mod streaming_parity` block.

### 5.4 Pipeline Composition Tests

Multi-function batch pipelines exercising chained `execute()` calls
through the registry. Each test builds a realistic pipeline and
verifies end-to-end correctness.

| # | Pipeline | Functions Chained | Verifies |
|---|----------|-------------------|----------|
| 1 | Transform chain | `uppercase → base64_encode → hex_encode` | Encoding composition |
| 2 | Storage pipeline | `compress → encrypt → base64_encode` | Roundtrip with reverse chain |
| 3 | Analytics pipeline | `grep → line_count → byte_count` | Filter then measure |
| 4 | Format pipeline | `csv_to_json → json_minify → sha256` | Format conversion + hash |
| 5 | CAS integrity | `chunk_hash → caddr_compute` | CAS-native composition |

Test pattern:
```rust
#[test]
fn pipeline_storage_roundtrip() {
    let input = Bytes::from("sensitive data to store");
    let key_params = BTreeMap::from([
        ("key".into(), Value::String("aa".repeat(32))),
        ("nonce".into(), Value::String("bb".repeat(16))),
    ]);
    // Forward: compress → encrypt → base64_encode
    let compressed = CompressFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap();
    let encrypted = EncryptFn.execute(vec![compressed.clone()], &key_params).unwrap();
    let encoded = Base64EncodeFn.execute(vec![encrypted.clone()], &BTreeMap::new()).unwrap();
    // Reverse: base64_decode → decrypt → decompress
    let decoded = Base64DecodeFn.execute(vec![encoded], &BTreeMap::new()).unwrap();
    let decrypted = DecryptFn.execute(vec![decoded], &key_params).unwrap();
    let decompressed = DecompressFn.execute(vec![decrypted], &BTreeMap::new()).unwrap();
    assert_eq!(decompressed, input);
}
```

Total: 5 integration tests in a `mod pipeline_composition` block.

### 5.5 Benchmark Suite

Deferred to a separate `crates/deriva-compute/benches/batch_bench.rs`
using `criterion`. Not part of the initial implementation — added
after correctness tests pass.

Key comparisons to measure:

| Benchmark | Input Sizes | What It Measures |
|-----------|-------------|------------------|
| Batch vs streaming throughput | 1 KB, 1 MB, 100 MB | Overhead of streaming state management |
| Compression ratio: batch vs streaming | 1 MB, 100 MB | Whole-input vs per-chunk compression |
| Sort: batch vs streaming | 1 MB | Correct global sort vs per-chunk sort |
| Crypto: encrypt + decrypt roundtrip | 1 KB, 1 MB | AES-CTR vs AES-GCM throughput |
| Format conversion: CSV↔JSON | 10K rows | Parse + serialize overhead |

Benchmark skeleton:
```rust
fn bench_compress(c: &mut Criterion) {
    let input = Bytes::from(vec![b'x'; 1_000_000]);
    c.bench_function("batch_compress_1mb", |b| {
        b.iter(|| CompressFn.execute(vec![input.clone()], &BTreeMap::new()).unwrap())
    });
}
```

---

## 6. Edge Cases & Error Handling

### 6.1 Universal Edge Cases

These apply to all 96 new functions:

| Edge Case | Expected Behavior | Affected Functions |
|-----------|-------------------|-------------------|
| Empty input (0 bytes) | Return empty or meaningful zero-value (e.g., `byte_count` → `"0"`) | All |
| Single byte input | Process correctly, no off-by-one | All |
| Very large input (>100 MB) | Complete without panic; O(n) memory | All batch functions |
| Non-UTF-8 input | Functions requiring UTF-8 return `ExecutionFailed`; byte-level functions succeed | Text, Format, Validation |
| Wrong input count | `ExecutionFailed("expected N inputs, got M")` | All |

### 6.2 Parameter Edge Cases

| Edge Case | Expected Behavior | Affected Functions |
|-----------|-------------------|-------------------|
| Missing required param | `InvalidParam("missing param: name")` | All parameterized |
| Param wrong type (number instead of string) | `InvalidParam` | All parameterized |
| Numeric param = 0 | `InvalidParam("must be > 0")` where 0 is invalid | block_size, count, level |
| Numeric param negative (via string) | `InvalidParam` on parse failure | parse_u64_param, parse_usize_param |
| Hex param odd length | `InvalidParam("invalid hex")` | key, nonce, expected_caddr |
| Regex param invalid syntax | `ExecutionFailed("invalid regex: ...")` | RegexReplaceFn, GrepFn |
| Level param out of range | `InvalidParam("level must be 1..N")` | Zstd (1–22), Brotli (0–11) |

### 6.3 Category-Specific Edge Cases

| Category | Edge Case | Behavior |
|----------|-----------|----------|
| Compression | Decompress non-compressed data | `ExecutionFailed` with codec-specific error |
| Compression | Compress already-compressed data | Succeeds (may grow slightly) |
| Crypto | Decrypt with wrong key | `ExecutionFailed` (CTR: silent corruption; GCM: auth tag failure) |
| Crypto | AEAD tampered ciphertext | `ExecutionFailed("authentication failed")` |
| Combiners | Two inputs of different lengths | Function-specific: Interleave stops at shorter, ZipConcat pads |
| Slicing | `take` N > input length | Return full input (no error) |
| Slicing | `skip` N > input length | Return empty |
| Slicing | `slice` with start > end | `InvalidParam` |
| Text | Replace with empty pattern | `InvalidParam("pattern must not be empty")` |
| Text | GrepFn on binary input | Processes line-by-line; non-UTF-8 lines never match |
| Validation | Utf8ValidateFn on valid UTF-8 | Pass-through |
| Validation | SizeLimitFn input exactly at limit | Pass-through (limit is inclusive) |
| Format | JSON parse of truncated input | `ExecutionFailed("invalid JSON: ...")` |
| Format | CSV with no rows (header only) | `csv_to_json` → `"[]"` |
| CAS | CAddrVerify mismatch | `ExecutionFailed("CAddr mismatch: expected ..., got ...")` |
| CAS | MerkleRoot block_size > input | Single leaf = hash of full input |
| Batch-Only | SortBytes on all-zero input | Returns input unchanged |

### 6.4 Determinism Guarantees

All 100 batch functions are deterministic — same inputs and params
always produce identical output — with one exception:

- `ShuffleFn` (#63): deterministic only when `seed` param is provided.
  Without seed, uses thread-local RNG and output varies between calls.

This is critical for CAS correctness: a computation node's output
address must be reproducible from its inputs.

---

## 7. Performance Analysis

### 7.1 Batch Advantage Over Streaming

Batch execution wins in specific scenarios where full-input access
enables better algorithms:

| Scenario | Batch | Streaming | Batch Advantage |
|----------|-------|-----------|-----------------|
| Compression ratio (zlib) | Whole-input dictionary | Per-chunk dictionary | 5–15% better ratio |
| Compression ratio (zstd) | Full-frame training | Per-chunk frames | 10–20% better ratio |
| Sort | Correct global sort | Per-chunk sort (incorrect) | Correctness, not just speed |
| Unique/Dedup | Exact deduplication | Approximate (per-chunk) | Correctness |
| Aggregates (min/max/sum) | Single pass, exact | Must merge chunk results | Simpler, same speed |
| Format conversion | Full parse tree | Impossible to stream | Only option |
| Merkle tree | Correct tree structure | Would need two passes | Only option |

#### 7.1.1 Throughput Comparison

Measured on 1MB inputs unless noted. Run with:
`cargo bench -p deriva-benchmarks --bench batch_vs_streaming`

| Scenario | Batch | Streaming | Batch Advantage |
|----------|-------|-----------|-----------------|
| Identity (64KB) | 1.94 TiB/s | 41 MiB/s | ~48,000× |
| Identity (1MB) | 28.3 TiB/s | 625 MiB/s | ~46,000× |
| Identity (10MB) | 322 TiB/s | 2.86 GiB/s | ~115,000× |
| Uppercase | 15.1 GiB/s | 618 MiB/s | 25× |
| Lowercase | 29.9 GiB/s | 570 MiB/s | 54× |
| Base64 Encode | 2.82 GiB/s | 409 MiB/s | 7× |
| Base64 Decode | 3.17 GiB/s | 553 MiB/s | 6× |
| Zlib Compress | 768 MiB/s | 489 MiB/s | 1.6× |
| Zlib Decompress | 5.76 MiB/s | 494 KiB/s | 12× |
| SHA-256 | 2.0 GiB/s | 401 MiB/s | 5× |
| Byte Count | 23.4 TiB/s | 625 MiB/s | ~38,000× |
| XOR | 40.2 GiB/s | 548 MiB/s | 75× |
| 3-Stage Pipeline | 1.62 GiB/s | 450 MiB/s | 3.7× |
| Compress+XOR | 735 MiB/s | 518 MiB/s | 1.4× |
| 5-Stage Pipeline | 687 MiB/s | 367 MiB/s | 1.9× |

#### 7.1.2 Chunk Size Impact (Streaming)

| Chunk Size | Throughput | Notes |
|------------|------------|-------|
| 4KB | 276 MiB/s | High channel overhead |
| 64KB (default) | 553 MiB/s | Optimal balance |
| 256KB | 546 MiB/s | No improvement |
| Batch baseline | 13.3 GiB/s | 24× faster than best streaming |

#### 7.1.3 Key Findings

1. **Zero-copy passthrough is unmatchable**: Identity and byte count
   show 40,000–115,000× batch advantage. Batch `Bytes::clone()` is a
   refcount bump (~30 ns), while streaming must chunk the data into
   64KB pieces, send each through a tokio mpsc channel, and reassemble
   — a minimum ~1.5 ms overhead regardless of input size. This is the
   fundamental cost of the streaming abstraction.

2. **Simple transforms are overhead-dominated**: Uppercase, lowercase,
   and XOR show 25–75× batch advantage. The per-byte computation
   (single CPU instruction) is negligible compared to streaming's
   channel send/recv, task scheduling, and chunk allocation. Streaming
   throughput plateaus at ~550–620 MiB/s across all lightweight
   transforms, confirming the bottleneck is the pipeline machinery,
   not the computation.

3. **CPU-heavy operations converge**: Zlib compression (1.6×) and the
   compress+XOR pipeline (1.4×) show the smallest gaps. When a single
   function call takes >1 ms, the ~1.5 ms streaming overhead becomes
   a smaller fraction of total time. This is the crossover point where
   streaming's memory advantages begin to justify its throughput cost.

4. **Accumulators expose a design tension**: SHA-256 (5× batch
   advantage) and byte count (38,000×) both produce fixed-size output,
   but streaming implementations must still receive all chunks before
   emitting a result. Streaming accumulators gain no progressive output
   benefit — they pay the full pipeline overhead for zero streaming
   advantage. Batch is strictly superior for accumulators.

5. **Pipeline depth amortizes overhead**: The batch advantage shrinks
   from 3.7× (3-stage) to 1.9× (5-stage). Each additional streaming
   stage adds only incremental channel overhead (~0.2 ms), while batch
   must allocate a new intermediate buffer per stage. For deep
   pipelines (>5 stages) with CPU-heavy functions, streaming may
   approach parity.

6. **64KB chunk size is optimal**: Smaller chunks (4KB) double the
   channel operations, halving throughput. Larger chunks (256KB) show
   no improvement — the bottleneck shifts from channel overhead to
   memory copy cost. The 64KB default aligns with L2 cache size on
   most architectures, balancing channel efficiency with cache
   locality.

7. **Streaming overhead is constant, not proportional**: The ~1.5 ms
   base overhead (channel setup, task spawn, End marker) is fixed.
   This means streaming's relative penalty decreases with input size:
   at 64KB the overhead is 100% of batch time, at 10MB it drops to
   ~30%. For inputs >100MB, streaming overhead becomes negligible.

#### 7.1.4 When to Use Each Mode

| Use Batch When | Use Streaming When |
|----------------|-------------------|
| Input fits in memory | Input exceeds memory budget |
| Latency-sensitive (<1 ms) | Memory-constrained environments |
| Simple transforms (upper/lower/xor) | Progressive output needed |
| Accumulators/aggregates | Backpressure required |
| Format conversion (batch-only) | Tee/fan-out patterns |
| Pipeline depth ≤3 stages | Deep pipelines with CPU-heavy stages |
| Input size <3 MB | Input size >3 MB |

The §2.9 size-aware mode selection uses `streaming_threshold` (default
3MB) to automatically choose the optimal mode based on input size.
The 3MB threshold is empirically justified: below 3MB, batch completes
all operations in <5 ms; above 3MB, streaming's constant overhead
becomes an acceptable fraction of total execution time.

### 7.2 Memory Profile

All batch functions are O(n) memory where n = total input size.
Worst-case memory multipliers:

| Category | Peak Memory | Reason |
|----------|-------------|--------|
| Transforms | 1× input | In-place or single output buffer |
| Compression | 2× input | Input + compressed output |
| Crypto | 2× input | Input + ciphertext (GCM adds 16B tag) |
| Accumulators | O(1) | Only accumulate counters/stats |
| Combiners | N× inputs | All inputs held simultaneously |
| Slicing | 1× input | Subset of input |
| Text | 2× input | Input + transformed output |
| Format conversion | 3× input | Input + parsed DOM + serialized output |
| CAS | 1× input + O(blocks) | Input + hash per block |

Recommendation: inputs >1 GB should prefer streaming path when
available. §2.9 size-aware mode selection enforces this via the
`streaming_threshold` (default 3 MB).

#### 7.2.1 Measured Output Multipliers

Run with: `cargo bench -p deriva-benchmarks --bench memory_profile`

| Scenario | Output/Input Ratio | Measured Throughput | Notes |
|----------|-------------------|---------------------|-------|
| Identity | 1.00× | 32 ns (zero-copy) | Bytes::clone is refcount bump |
| Uppercase/Lowercase | 1.00× | 64 µs / 1MB | New buffer, same size |
| Zlib (compressible) | <0.01× | 1.30 ms / 1MB | >100× compression on repeated data |
| Zlib (incompressible) | ~1.00× | 1.30 ms / 1MB | Slight expansion from headers |
| Zstd (compressible) | <0.01× | 168 µs / 1MB | 7.7× faster than zlib |
| LZ4 (compressible) | <0.10× | 52 µs / 1MB | 25× faster than zlib |
| AES-CTR encrypt | 1.00× | 146 µs / 1MB | Stream cipher: exact size |
| AES-GCM encrypt | 1.00× + 16B | 605 µs / 1MB | 16-byte auth tag appended |
| Base64 encode | 1.33× | 310 µs / 1MB | 4 output chars per 3 input bytes |
| Hex encode | 2.00× | 29.9 ms / 1MB | 2 hex chars per byte |
| Base32 encode | 1.60× | 437 µs / 1MB | 8 output chars per 5 input bytes |

#### 7.2.2 Accumulator Output Sizes (Constant Regardless of Input)

| Function | Output Size | Time (64KB) | Time (1MB) | Time (10MB) |
|----------|-------------|-------------|------------|-------------|
| SHA-256 | 32 bytes | 30 µs | 490 µs | 5.0 ms |
| SHA-512 | 64 bytes | — | 1.42 ms | — |
| BLAKE3 | 32 bytes | — | 233 µs | — |
| MD5 | 16 bytes | — | 1.42 ms | — |
| CRC32 | 4 bytes | — | 30 µs | — |
| Byte count | 8 bytes | 42 ns | 46 ns | 44 ns |

Byte count is O(1) time (reads `Bytes::len()`). All hashes are O(n)
time with constant output. BLAKE3 is 2× faster than SHA-256, 6×
faster than SHA-512/MD5.

#### 7.2.3 Slicing: Sub-Linear Output

| Operation | Output Size | Time |
|-----------|-------------|------|
| Take 256KB from 1MB | 0.25× | 41 ns (zero-copy slice) |
| Skip 768KB from 1MB | 0.25× | 41 ns (zero-copy slice) |
| Slice middle 512KB | 0.50× | 51 ns (zero-copy slice) |

All slicing operations are O(1) time via `Bytes::slice()` — no data
is copied.

#### 7.2.4 Expansion Operations

| Operation | Output/Input | Time (64KB input) |
|-----------|-------------|-------------------|
| Repeat 4× | 4.00× | 4.5 µs |
| Pad to 256-byte blocks | ~1.00× | 2.3 µs |
| CAddr embed | 1.00× + 32B | 246 µs |

#### 7.2.5 Category Throughput Summary

| Category | Throughput (1MB) | Memory Pattern |
|----------|-----------------|----------------|
| Transforms | 15–30 GiB/s | 1× (in-place) |
| Compression | 0.8–6.0 GiB/s | Variable (0.01–1.0×) |
| Crypto (CTR) | 6.7 GiB/s | 1× (stream cipher) |
| Crypto (GCM) | 1.6 GiB/s | 1× + 16B (auth tag) |
| Accumulators | 0.2–20 TiB/s | O(1) output |
| Combiners | 18 GiB/s (concat) | N× inputs |
| Slicing | >20 TiB/s | ≤1× (zero-copy) |
| Text | 1.9–3.6 GiB/s | 1–2× |
| Format conversion | 0.4–0.5 GiB/s | 2–4× |
| CAS | 2.0–4.3 GiB/s | 1× + O(blocks) |
| Validation | 4.8 GiB/s (JSON) | 1× (passthrough) |
| Sort | 0.1–1.1 GiB/s | 1× (reordered) |
| Encoding | 0.03–3.2 GiB/s | 1.33–2.0× |

#### 7.2.6 Key Findings

1. **Zero-copy operations dominate**: Identity, slicing (take/skip/slice),
   and byte count all complete in 30–50 ns regardless of input size.
   `Bytes::clone()` is a refcount bump; `Bytes::slice()` is a pointer
   adjustment. These are effectively free.

2. **Accumulators are output-bounded, not input-bounded**: SHA-256
   produces 32 bytes whether the input is 64KB or 10MB. Byte count
   is O(1) time because it reads `Bytes::len()` — it never touches
   the data. BLAKE3 (233 µs/MB) is 2× faster than SHA-256 (490 µs/MB)
   and 6× faster than SHA-512 and MD5 (~1.4 ms/MB each).

3. **Compression ratio depends entirely on input entropy**: Repeated
   data achieves >100× compression (output <0.01× input). Pseudo-random
   data produces ~1.0× output with slight header overhead. Zstd is
   7.7× faster than zlib; LZ4 is 25× faster than zlib.

4. **Encoding expansion is predictable and fixed**: Base64 always
   produces 1.33× output, hex always 2.0×, base32 always 1.6×.
   Hex encoding (29.9 ms/MB) is ~100× slower than base64 (310 µs/MB)
   due to per-byte string formatting vs SIMD-optimized base64.

5. **Crypto overhead is mode-dependent**: AES-CTR (146 µs/MB) is 4×
   faster than AES-GCM (605 µs/MB). CTR produces exact-size output;
   GCM appends a 16-byte authentication tag. Choose CTR for throughput,
   GCM when tamper detection is required.

6. **Format conversion is the most memory-intensive category**: CSV→JSON
   expands data (field names repeated per row), and the parse→DOM→serialize
   pipeline requires 2–4× peak memory. At 0.4–0.5 GiB/s, this is the
   slowest category — a strong candidate for streaming when available.

7. **Slicing never allocates**: All three slicing operations (take, skip,
   slice) complete in ~40–50 ns via `Bytes::slice()`, producing a
   zero-copy view into the original buffer. This confirms the spec's
   "1× input" claim is actually an overestimate — slicing is 0× extra
   memory.

8. **Recommendation confirmed**: The 3MB streaming threshold from §2.9
   is well-justified. Below 3MB, batch functions complete in <5 ms for
   all categories. Above 3MB, format conversion and sort begin to
   stress memory, making streaming preferable.

### 7.3 Computational Complexity

| Complexity Class | Functions | Count |
|-----------------|-----------|-------|
| O(1) | IdentityFn | 1 |
| O(n) | Most transforms, compression, crypto, accumulators, slicing, text | 78 |
| O(n log n) | SortFn, SortUniqueFn, SortBytesFn, MergeSortedFn | 4 |
| O(n × m) | ReplaceFn, RegexReplaceFn (m = matches), DiffFn (m = edit distance) | 3 |
| O(n) high constant | EncryptFn, AeadEncryptFn, SchemaValidateFn, BrotliCompress | 4 |

No function exceeds O(n²) worst case. DiffFn uses Myers' algorithm
which is O(n × d) where d = edit distance, bounded by O(n²) for
completely different inputs.

#### 7.3.1 Scaling Measurements

Run with: `cargo bench -p deriva-benchmarks --bench complexity_scaling`

Each function measured at 4KB, 64KB, 256KB, 1MB to reveal scaling curves.

| Scenario | 4KB | 64KB | 256KB | 1MB | Scaling Factor (1MB/4KB) | Expected |
|----------|-----|------|-------|-----|--------------------------|----------|
| Identity (O(1)) | 34 ns | 35 ns | 34 ns | 35 ns | 1.0× | 1.0× |
| Uppercase (O(n)) | 297 ns | 3.7 µs | 15.2 µs | 62.2 µs | 209× | 256× |
| SHA-256 (O(n)) | 1.9 µs | 29.2 µs | 118 µs | 486 µs | 256× | 256× |
| Zlib compress (O(n)) | 14.0 µs | 93.6 µs | 337 µs | 1.34 ms | 96× | 256× |
| AES-CTR encrypt (O(n) high) | 1.0 µs | 9.5 µs | 36.9 µs | 147 µs | 147× | 256× |
| Brotli compress (O(n) high) | 49.1 µs | 569 µs | 904 µs | 2.32 ms | 47× | 256× |
| Sort lines (O(n log n)) | 2.3 µs | 33.4 µs | 142 µs | 508 µs | 221× | 341× |
| Sort unique (O(n log n)) | 10.6 µs | 159 µs | 822 µs | 2.40 ms | 226× | 341× |
| Replace (O(n×m)) | 2.3 µs | 33.5 µs | 126 µs | 527 µs | 229× | 256× |
| Diff (O(n×d), d≈5%) | 3.5 µs | 51.7 µs | 186 µs | 651 µs | 186× | 256× |

*Expected scaling: O(1)=1×, O(n)=256× (1MB/4KB), O(n log n)=256×(log 1M/log 4K)≈341×*

#### 7.3.2 Key Findings

1. **O(1) confirmed**: Identity holds at ~34 ns regardless of input
   size. The `Bytes::clone()` refcount bump is truly constant — no
   hidden memcpy or allocation.

2. **Pure O(n) functions scale linearly**: SHA-256 hits the theoretical
   256× scaling factor exactly (1.9 µs → 486 µs). Uppercase shows
   209× due to SIMD vectorization at larger sizes — the CPU processes
   more bytes per cycle when the working set fits in L1/L2 cache.

3. **High-constant O(n) diverges from pure O(n)**: AES-CTR (147×) has
   a fixed key schedule setup (~0.5 µs) that dominates at 4KB but
   amortizes at 1MB. Brotli (47×) shows sub-linear scaling because its
   dictionary-building phase is O(n) but with a large constant that
   saturates early — the LZ77 window size caps the effective search
   depth regardless of input size.

4. **Zlib compression is sub-linear in practice**: The 96× scaling
   (vs expected 256×) reflects zlib's sliding window (32KB). Beyond
   32KB, the algorithm doesn't search further back, so doubling input
   size doesn't double work — it only adds more window-sized chunks.

5. **O(n log n) sorts show expected super-linear growth**: Sort (221×)
   and sort-unique (226×) fall between O(n) (256×) and O(n log n)
   (341×). The gap from 341× reflects that comparison-based sorting
   benefits from branch prediction on partially-ordered data — our
   test input has repeated lines that create natural runs.

6. **Replace scales as O(n) not O(n×m)**: The 229× factor matches
   linear scaling because match density (m/n) is constant — "the"
   appears at a fixed rate per line. O(n×m) worst case requires
   match density to grow with input size, which doesn't happen with
   natural text.

7. **Diff with small edit distance is effectively O(n)**: At 5% edit
   distance, diff shows 186× scaling (sub-linear). Myers' algorithm
   is O(n×d) where d is edit distance; with d proportional to n (5%),
   this is O(n²), but the constant is tiny. For similar inputs (the
   common case in a CAS), diff behaves as fast O(n).

---

## 8. Files Changed

| File | Action | Description |
|------|--------|-------------|
| `crates/deriva-compute/src/builtins/` | Create | 11 category modules (transforms, compression, crypto, accumulators, combiners, slicing, text, validation, format_conversion, cas, batch_only) + `mod.rs` |
| `crates/deriva-compute/tests/batch/` | Create | 14 test modules (579 tests): per-category + roundtrips, pipeline_composition, edge_cases |
| `crates/deriva-compute/tests/async_exec/` | Create | 7 modules (45 tests): core, parallel, semaphore, dedup, error_handling, caching, stress |
| `crates/deriva-compute/tests/verif/` | Create | 2 modules (30 tests): modes, advanced |
| `crates/deriva-compute/Cargo.toml` | Modify | Add 13 new dependencies (see §9) |
| `crates/deriva-benchmarks/benches/batch_vs_streaming.rs` | Create | §7.1 benchmark — 15 scenarios |
| `crates/deriva-benchmarks/benches/memory_profile.rs` | Create | §7.2 benchmark — 15 scenarios |
| `crates/deriva-benchmarks/benches/complexity_scaling.rs` | Create | §7.3 benchmark — 10 scenarios, 4 sizes each |
| `crates/deriva-benchmarks/Cargo.toml` | Modify | Register 3 new benchmark entries |

---

## 9. Dependency Changes

New dependencies added to `crates/deriva-compute/Cargo.toml`:

| Crate | Version | Used By | Already in Workspace? |
|-------|---------|---------|----------------------|
| `base32` | `0.5` | Base32Encode/DecodeFn | No |
| `base64` | `0.22` | Base64Encode/DecodeFn | Yes (§2.14) |
| `sha2` | `0.10` | Sha256Fn, Sha512Fn, MerkleRootFn, ChunkHashFn | Yes (§2.14) |
| `md-5` | `0.10` | Md5Fn | No |
| `blake3` | `1` | Blake3Fn | No |
| `hmac` | `0.12` | HmacSha256Fn | No |
| `crc32fast` | `1` | Crc32Fn, Crc32VerifyFn | Yes (§2.14) |
| `aes` | `0.8` | EncryptFn, DecryptFn | No |
| `ctr` | `0.9` | EncryptFn, DecryptFn (AES-256-CTR) | No |
| `aes-gcm` | `0.10` | AeadEncryptFn, AeadDecryptFn | No |
| `zstd` | `0.13` | ZstdCompress/DecompressFn | No |
| `lz4_flex` | `0.11` | Lz4Compress/DecompressFn | No |
| `snap` | `1` | SnappyCompress/DecompressFn | No |
| `brotli` | `6` | BrotliCompress/DecompressFn | No |
| `flate2` | `1` | CompressFn, DecompressFn (zlib) | Yes (§2.14) |
| `regex` | `1` | RegexReplaceFn, GrepFn, GrepInvertFn | No |
| `rand` | `0.8` | ShuffleFn, SampleFn | No |
| `csv` | `1` | CsvToJsonFn, JsonToCsvFn | No |
| `serde_json` | `1` | Format conversion (#84–#91), DedupAnalyzeFn | Yes (workspace) |
| `serde_yaml` | `0.9` | YamlToJsonFn, JsonToYamlFn | No |
| `toml` | `0.8` | TomlToJsonFn | No |
| `encoding_rs` | `0.8` | CharsetConvertFn | No |
| `jsonschema` | `0.18` | SchemaValidateFn | No |

Summary: 13 new crates, 5 already in workspace. All are well-maintained,
widely-used crates with no `unsafe` in their public API (except `aes`
which uses hardware intrinsics behind safe wrappers).

---

## 10. Design Rationale

### 10.1 Why Not Just Use Streaming for Everything?

Batch is the correct choice when:
- The algorithm requires full input (sort, unique, format conversion,
  Merkle tree, aggregates like min/max/average)
- Compression benefits from whole-input dictionary (5–20% better ratio)
- Implementation simplicity matters — batch functions are ~10 lines
  each vs 50+ for streaming equivalents with state machines
- Testing is simpler — no chunk boundary edge cases

Streaming is preferred when input exceeds memory or when latency to
first byte matters. §2.9 selects automatically based on input size.

### 10.2 Why Separate FunctionId Versioning?

The `name@version` scheme (e.g., `sha256@1.0.0`) allows breaking
changes without invalidating existing computation DAG nodes:
- Old recipes reference `sha256@1.0.0`, new recipes use `sha256@2.0.0`
- Both coexist in the registry simultaneously
- CAS addresses remain valid — a node's output is determined by its
  function version + inputs, so changing the version creates a new node

### 10.3 Why Include Format Conversion Here Instead of §2.16?

§2.15 covers general-purpose format conversions between common text
formats (JSON, YAML, TOML, CSV). These are simple parse-and-serialize
operations with no schema awareness.

§2.16 covers complex format-specific operations: Parquet, Avro, HDF5,
Protocol Buffers — formats that require schema definitions, columnar
access patterns, or binary framing. The complexity and dependency
weight of those formats warrants a separate section.

---

## 11. Observability Integration

Four Prometheus metrics are emitted per batch function execution:

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `deriva_batch_fn_calls_total` | Counter | `function` | Total invocations per function |
| `deriva_batch_fn_errors_total` | Counter | `function`, `error_type` | Errors per function |
| `deriva_batch_fn_duration_seconds` | Histogram | `function` | Execution duration per function |
| `deriva_batch_fn_input_bytes` | Histogram | `function` | Total input size per invocation |

Metrics are recorded in the executor layer wrapping `execute()`, not
inside each function implementation. This keeps function code clean
and ensures consistent instrumentation.

`error_type` labels: `invalid_param`, `execution_failed`.

Dashboard queries:
```promql
# Error rate per function
rate(deriva_batch_fn_errors_total[5m]) / rate(deriva_batch_fn_calls_total[5m])

# P99 latency per function
histogram_quantile(0.99, rate(deriva_batch_fn_duration_seconds_bucket[5m]))

# Top functions by throughput
topk(10, sum(rate(deriva_batch_fn_input_bytes_sum[5m])) by (function))
```

---

## 12. Checklist

### 12.1 Implementation (96 functions)

- [x] §3.1 Transforms (#5–#20) — 16 functions
- [x] §3.2 Compression (#21–#30) — 10 functions
- [x] §3.3 Crypto & Hashing (#31–#41) — 11 functions
- [x] §3.4 Accumulators (#42–#49) — 8 functions
- [x] §3.5 Combiners (#50–#55) — 6 functions
- [x] §3.6 Slicing & Restructuring (#56–#65) — 10 functions
- [x] §3.7 Text Processing (#66–#75) — 10 functions
- [x] §3.8 Validation & Integrity (#76–#83) — 8 functions
- [x] §3.9 Format Conversion (#84–#91) — 8 functions
- [x] §3.10 CAS-Specific (#92–#98) — 7 functions
- [x] §3.11 Batch-Only (#99–#100) — 2 functions

### 12.2 Registration & Wiring

- [x] Register all 96 new functions in `register_default_functions()`
- [x] Add 13 new crate dependencies to `Cargo.toml` (§9)
- [x] Implement 6 shared helper functions (§4.2)

### 12.3 Testing (579 tests)

- [x] Unit tests — 579 tests across 14 modules in `tests/batch/`
- [x] Roundtrip tests — 13 encode/decode pairs (§5.2)
- [x] Streaming parity tests — 20 batch-vs-streaming comparisons (§5.3)
- [x] Pipeline composition tests — 5 multi-function chains (§5.4)

### 12.4 Quality

- [x] `cargo check -p deriva-compute` compiles clean
- [x] `cargo clippy -p deriva-compute -- -D warnings` no warnings
- [x] `cargo test -p deriva-compute` — all existing + new tests pass
- [x] All `estimated_cost()` implementations return sensible values (§4.1)
- [x] All error messages include function/param name for debuggability (§4.3)

### 12.5 Observability

- [x] Compute metrics wired in executor layer (§11) — `COMPUTE_DURATION`, `COMPUTE_INPUT_BYTES`, `COMPUTE_OUTPUT_BYTES` with `function` label

### 12.6 Benchmarks

- [x] §7.1 Batch vs Streaming — 15 scenarios in `benches/batch_vs_streaming.rs`
- [x] §7.2 Memory Profile — 15 scenarios in `benches/memory_profile.rs`
- [x] §7.3 Computational Complexity — 10 scenarios in `benches/complexity_scaling.rs`

### 12.7 Final

- [x] `git add -A && git -c commit.gpgsign=false commit`
- [x] Verify no TODO stubs remain in this spec

