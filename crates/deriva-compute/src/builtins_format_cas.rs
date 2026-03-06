//! Category L: Blockchain & Content-Addressing — CID, DAG-PB, DAG-CBOR, CAR, UnixFS (15 functions, #396–#410)

use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;

fn fid(name: &str) -> FunctionId {
    FunctionId { name: name.to_string(), version: "1.0.0".to_string() }
}
fn param_str(p: &BTreeMap<String, Value>, k: &str) -> Option<String> {
    match p.get(k) { Some(Value::String(s)) => Some(s.clone()), _ => None }
}
fn one(inputs: &[Bytes]) -> Result<&Bytes, ComputeError> {
    if inputs.is_empty() { return Err(ComputeError::InputCount { expected: 1, got: 0 }); }
    Ok(&inputs[0])
}
fn cost(sizes: &[u64]) -> ComputeCost {
    ComputeCost { cpu_ms: sizes.iter().sum::<u64>() / 100 + 50, memory_bytes: sizes.iter().sum::<u64>() * 2 + 1024 }
}

const SHA2_256: u64 = 0x12;
const BLAKE3_CODE: u64 = 0x1e;
const RAW: u64 = 0x55;
const DAG_PB: u64 = 0x70;
const DAG_CBOR: u64 = 0x71;

fn compute_multihash(data: &[u8], hash: &str) -> Result<libipld::multihash::Multihash, ComputeError> {
    match hash {
        "sha2-256" => {
            use sha2::Digest;
            let digest = sha2::Sha256::digest(data);
            libipld::multihash::Multihash::wrap(SHA2_256, &digest)
                .map_err(|e| ComputeError::ExecutionFailed(format!("multihash: {e}")))
        }
        "blake3" => {
            let digest = blake3::hash(data);
            libipld::multihash::Multihash::wrap(BLAKE3_CODE, digest.as_bytes())
                .map_err(|e| ComputeError::ExecutionFailed(format!("multihash: {e}")))
        }
        _ => Err(ComputeError::InvalidParam("hash: sha2-256|blake3".into())),
    }
}

fn codec_code(s: &str) -> Result<u64, ComputeError> {
    match s { "raw" => Ok(RAW), "dag-pb" => Ok(DAG_PB), "dag-cbor" => Ok(DAG_CBOR),
        _ => Err(ComputeError::InvalidParam("codec: raw|dag-pb|dag-cbor".into())) }
}

fn make_cid(data: &[u8], version: u64, codec: u64, hash: &str) -> Result<libipld::cid::Cid, ComputeError> {
    let mh = compute_multihash(data, hash)?;
    if version == 0 {
        if codec != DAG_PB { return Err(ComputeError::ExecutionFailed("CIDv0 requires dag-pb codec".into())); }
        if hash != "sha2-256" { return Err(ComputeError::ExecutionFailed("CIDv0 requires sha2-256".into())); }
        libipld::cid::Cid::new_v0(mh).map_err(|e| ComputeError::ExecutionFailed(format!("cidv0: {e}")))
    } else {
        Ok(libipld::cid::Cid::new_v1(codec, mh))
    }
}

fn ipld_to_json(ipld: &libipld::Ipld) -> serde_json::Value {
    match ipld {
        libipld::Ipld::Null => serde_json::Value::Null,
        libipld::Ipld::Bool(b) => serde_json::Value::Bool(*b),
        libipld::Ipld::Integer(i) => serde_json::json!(*i),
        libipld::Ipld::Float(f) => serde_json::json!(*f),
        libipld::Ipld::String(s) => serde_json::Value::String(s.clone()),
        libipld::Ipld::Bytes(b) => serde_json::Value::String(data_encoding::BASE64.encode(b)),
        libipld::Ipld::List(l) => serde_json::Value::Array(l.iter().map(ipld_to_json).collect()),
        libipld::Ipld::Map(m) => {
            let obj: serde_json::Map<String, serde_json::Value> = m.iter().map(|(k, v)| (k.clone(), ipld_to_json(v))).collect();
            serde_json::Value::Object(obj)
        }
        libipld::Ipld::Link(c) => serde_json::json!({"/": c.to_string()}),
    }
}

fn json_to_ipld(v: &serde_json::Value) -> libipld::Ipld {
    match v {
        serde_json::Value::Null => libipld::Ipld::Null,
        serde_json::Value::Bool(b) => libipld::Ipld::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { libipld::Ipld::Integer(i as i128) }
            else { libipld::Ipld::Float(n.as_f64().unwrap_or(0.0)) }
        }
        serde_json::Value::String(s) => libipld::Ipld::String(s.clone()),
        serde_json::Value::Array(a) => libipld::Ipld::List(a.iter().map(json_to_ipld).collect()),
        serde_json::Value::Object(m) => {
            if let Some(link) = m.get("/").and_then(|v| v.as_str()) {
                if let Ok(c) = link.parse::<libipld::cid::Cid>() {
                    return libipld::Ipld::Link(c);
                }
            }
            libipld::Ipld::Map(m.iter().map(|(k, v)| (k.clone(), json_to_ipld(v))).collect())
        }
    }
}

/// Encode unsigned varint to bytes
fn encode_varint(mut n: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    loop {
        let mut byte = (n & 0x7F) as u8;
        n >>= 7;
        if n != 0 { byte |= 0x80; }
        buf.push(byte);
        if n == 0 { break; }
    }
    buf
}

/// Decode unsigned varint from bytes, returns (value, bytes_consumed)
fn decode_varint(data: &[u8]) -> Result<(u64, usize), ComputeError> {
    let mut n: u64 = 0;
    let mut shift = 0;
    for (i, &byte) in data.iter().enumerate() {
        n |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 { return Ok((n, i + 1)); }
        shift += 7;
        if shift >= 64 { return Err(ComputeError::ExecutionFailed("varint overflow".into())); }
    }
    Err(ComputeError::ExecutionFailed("truncated varint".into()))
}

/// Build a CARv1 archive from blocks: Vec<(Cid, Vec<u8>)> with given roots
fn build_car(roots: &[libipld::cid::Cid], blocks: &[(libipld::cid::Cid, Vec<u8>)]) -> Vec<u8> {
    // Header: DAG-CBOR encoded {version: 1, roots: [...]}
    use libipld::codec::Codec;
    let roots_ipld: Vec<libipld::Ipld> = roots.iter().map(|c| libipld::Ipld::Link(*c)).collect();
    let header_ipld = libipld::Ipld::Map(BTreeMap::from([
        ("roots".into(), libipld::Ipld::List(roots_ipld)),
        ("version".into(), libipld::Ipld::Integer(1)),
    ]));
    let header_bytes = libipld::cbor::DagCborCodec.encode(&header_ipld).unwrap();
    let mut car = Vec::new();
    car.extend_from_slice(&encode_varint(header_bytes.len() as u64));
    car.extend_from_slice(&header_bytes);
    for (cid, data) in blocks {
        let cid_bytes = cid.to_bytes();
        let block_len = cid_bytes.len() + data.len();
        car.extend_from_slice(&encode_varint(block_len as u64));
        car.extend_from_slice(&cid_bytes);
        car.extend_from_slice(data);
    }
    car
}

/// Parse a CAR archive, returns (roots, blocks)
fn parse_car(data: &[u8]) -> Result<(Vec<libipld::cid::Cid>, Vec<(libipld::cid::Cid, Vec<u8>)>), ComputeError> {
    let err = |s: &str| ComputeError::ExecutionFailed(s.into());
    let mut pos = 0;
    // Read header
    let (header_len, consumed) = decode_varint(&data[pos..])?;
    pos += consumed;
    let header_end = pos + header_len as usize;
    if header_end > data.len() { return Err(err("truncated CAR header")); }
    use libipld::codec::Codec;
    let header: libipld::Ipld = libipld::cbor::DagCborCodec.decode(&data[pos..header_end])
        .map_err(|e| err(&format!("CAR header: {e}")))?;
    pos = header_end;
    let roots = if let libipld::Ipld::Map(m) = &header {
        if let Some(libipld::Ipld::List(r)) = m.get("roots") {
            r.iter().filter_map(|i| if let libipld::Ipld::Link(c) = i { Some(*c) } else { None }).collect()
        } else { Vec::new() }
    } else { Vec::new() };
    // Read blocks
    let mut blocks = Vec::new();
    while pos < data.len() {
        let (block_len, consumed) = decode_varint(&data[pos..])?;
        pos += consumed;
        let block_end = pos + block_len as usize;
        if block_end > data.len() { return Err(err("truncated CAR block")); }
        let block_data = &data[pos..block_end];
        let cid = libipld::cid::Cid::read_bytes(std::io::Cursor::new(block_data))
            .map_err(|e| err(&format!("block CID: {e}")))?;
        let cid_len = cid.encoded_len();
        blocks.push((cid, block_data[cid_len..].to_vec()));
        pos = block_end;
    }
    Ok((roots, blocks))
}

// ---- #396 CidComputeFn ----
pub struct CidComputeFn;
impl ComputeFunction for CidComputeFn {
    fn id(&self) -> FunctionId { fid("cid_compute") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let version: u64 = param_str(p, "version").unwrap_or_else(|| "1".into()).parse().unwrap_or(1);
        let codec_str = param_str(p, "codec").unwrap_or_else(|| "raw".into());
        let hash = param_str(p, "hash").unwrap_or_else(|| "sha2-256".into());
        let codec = codec_code(&codec_str)?;
        let cid = make_cid(b, version, codec, &hash)?;
        Ok(Bytes::from(cid.to_string()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #397 CidVerifyFn ----
pub struct CidVerifyFn;
impl ComputeFunction for CidVerifyFn {
    fn id(&self) -> FunctionId { fid("cid_verify") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let expected = param_str(p, "expected_cid").ok_or_else(|| ComputeError::InvalidParam("missing expected_cid".into()))?;
        let expected_cid: libipld::cid::Cid = expected.parse().map_err(|e| ComputeError::InvalidParam(format!("invalid CID: {e}")))?;
        let hash_name = match expected_cid.hash().code() { 0x12 => "sha2-256", 0x1e => "blake3", c => return Err(ComputeError::ExecutionFailed(format!("unsupported hash code: {c}"))) };
        let actual = make_cid(b, expected_cid.version().into(), expected_cid.codec(), hash_name)?;
        if actual != expected_cid {
            return Err(ComputeError::ExecutionFailed(format!("CID mismatch: expected {expected}, got {actual}")));
        }
        Ok(b.clone())
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #398 CidParseFn ----
pub struct CidParseFn;
impl ComputeFunction for CidParseFn {
    fn id(&self) -> FunctionId { fid("cid_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| ComputeError::ExecutionFailed("requires UTF-8".into()))?;
        let cid: libipld::cid::Cid = text.trim().parse().map_err(|e| ComputeError::ExecutionFailed(format!("invalid CID: {e}")))?;
        let codec_name = match cid.codec() { 0x55 => "raw", 0x70 => "dag-pb", 0x71 => "dag-cbor", c => return Ok(Bytes::from(serde_json::json!({"version": u64::from(cid.version()), "codec": c, "hash_code": cid.hash().code(), "digest": data_encoding::HEXLOWER.encode(cid.hash().digest())}).to_string())) };
        let hash_name = match cid.hash().code() { 0x12 => "sha2-256", 0x1e => "blake3", c => &format!("0x{c:x}") };
        let result = serde_json::json!({
            "version": u64::from(cid.version()),
            "codec": codec_name,
            "hash": hash_name,
            "digest": data_encoding::HEXLOWER.encode(cid.hash().digest())
        });
        Ok(Bytes::from(result.to_string()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #399 DagPbEncodeFn ----
pub struct DagPbEncodeFn;
impl ComputeFunction for DagPbEncodeFn {
    fn id(&self) -> FunctionId { fid("dag_pb_encode") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json: {e}")))?;
        let mut links = Vec::new();
        if let Some(arr) = v["links"].as_array() {
            for link in arr {
                let cid_str = link["cid"].as_str().unwrap_or("");
                let cid: libipld::cid::Cid = cid_str.parse().map_err(|e| ComputeError::ExecutionFailed(format!("link CID: {e}")))?;
                links.push(libipld::pb::PbLink {
                    cid,
                    name: link["name"].as_str().map(|s| s.to_string()),
                    size: link["size"].as_u64(),
                });
            }
        }
        let data = if let Some(s) = v["data"].as_str() {
            Some(Bytes::from(data_encoding::BASE64.decode(s.as_bytes()).unwrap_or_else(|_| s.as_bytes().to_vec())))
        } else { None };
        let node = libipld::pb::PbNode { links, data };
        Ok(Bytes::from(node.into_bytes().to_vec()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #400 DagPbDecodeFn ----
pub struct DagPbDecodeFn;
impl ComputeFunction for DagPbDecodeFn {
    fn id(&self) -> FunctionId { fid("dag_pb_decode") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let node = libipld::pb::PbNode::from_bytes(Bytes::copy_from_slice(b))
            .map_err(|e| ComputeError::ExecutionFailed(format!("dag-pb: {e}")))?;
        let links: Vec<serde_json::Value> = node.links.iter().map(|l| {
            serde_json::json!({"cid": l.cid.to_string(), "name": l.name, "size": l.size})
        }).collect();
        let data = node.data.as_ref().map(|d| data_encoding::BASE64.encode(d));
        let result = serde_json::json!({"links": links, "data": data});
        Ok(Bytes::from(result.to_string()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #401 DagCborEncodeFn ----
pub struct DagCborEncodeFn;
impl ComputeFunction for DagCborEncodeFn {
    fn id(&self) -> FunctionId { fid("dag_cbor_encode") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json: {e}")))?;
        let ipld = json_to_ipld(&v);
        use libipld::codec::Codec;
        let encoded = libipld::cbor::DagCborCodec.encode(&ipld)
            .map_err(|e| ComputeError::ExecutionFailed(format!("dag-cbor encode: {e}")))?;
        Ok(Bytes::from(encoded))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #402 DagCborDecodeFn ----
pub struct DagCborDecodeFn;
impl ComputeFunction for DagCborDecodeFn {
    fn id(&self) -> FunctionId { fid("dag_cbor_decode") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        use libipld::codec::Codec;
        let ipld: libipld::Ipld = libipld::cbor::DagCborCodec.decode(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("dag-cbor decode: {e}")))?;
        let json = ipld_to_json(&ipld);
        Ok(Bytes::from(serde_json::to_string(&json).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #403 CarCreateFn ----
pub struct CarCreateFn;
impl ComputeFunction for CarCreateFn {
    fn id(&self) -> FunctionId { fid("car_create") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let roots_str = param_str(p, "roots").unwrap_or_default();
        let roots: Vec<libipld::cid::Cid> = if roots_str.is_empty() { Vec::new() } else {
            roots_str.split(',').map(|s| s.trim().parse::<libipld::cid::Cid>()
                .map_err(|e| ComputeError::InvalidParam(format!("root CID: {e}")))).collect::<Result<_, _>>()?
        };
        let mut blocks = Vec::new();
        for input in &inputs {
            let mh = compute_multihash(input, "sha2-256")?;
            let cid = libipld::cid::Cid::new_v1(RAW, mh);
            blocks.push((cid, input.to_vec()));
        }
        let car_roots = if roots.is_empty() { blocks.iter().map(|(c, _)| *c).collect() } else { roots };
        Ok(Bytes::from(build_car(&car_roots, &blocks)))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #404 CarExtractFn ----
pub struct CarExtractFn;
impl ComputeFunction for CarExtractFn {
    fn id(&self) -> FunctionId { fid("car_extract") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let (roots, blocks) = parse_car(b)?;
        if let Some(target) = param_str(p, "cid") {
            let target_cid: libipld::cid::Cid = target.parse().map_err(|e| ComputeError::InvalidParam(format!("cid: {e}")))?;
            for (cid, data) in &blocks {
                if *cid == target_cid { return Ok(Bytes::from(data.clone())); }
            }
            return Err(ComputeError::ExecutionFailed(format!("block not found: {target}")));
        }
        let manifest: Vec<serde_json::Value> = blocks.iter().map(|(c, d)| {
            serde_json::json!({"cid": c.to_string(), "size": d.len()})
        }).collect();
        let result = serde_json::json!({"roots": roots.iter().map(|c| c.to_string()).collect::<Vec<_>>(), "blocks": manifest});
        Ok(Bytes::from(result.to_string()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #405 CarListFn ----
pub struct CarListFn;
impl ComputeFunction for CarListFn {
    fn id(&self) -> FunctionId { fid("car_list") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let (_roots, blocks) = parse_car(b)?;
        let entries: Vec<serde_json::Value> = blocks.iter().map(|(c, d)| {
            serde_json::json!({"cid": c.to_string(), "size": d.len()})
        }).collect();
        Ok(Bytes::from(serde_json::to_string(&entries).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #406 CarVerifyFn ----
pub struct CarVerifyFn;
impl ComputeFunction for CarVerifyFn {
    fn id(&self) -> FunctionId { fid("car_verify") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let (_roots, blocks) = parse_car(b)?;
        for (cid, data) in &blocks {
            let hash_name = match cid.hash().code() { 0x12 => "sha2-256", 0x1e => "blake3", c => return Err(ComputeError::ExecutionFailed(format!("unsupported hash: 0x{c:x}"))) };
            let actual_mh = compute_multihash(data, hash_name)?;
            if actual_mh.digest() != cid.hash().digest() {
                return Err(ComputeError::ExecutionFailed(format!("CID mismatch for block {cid}")));
            }
        }
        Ok(b.clone())
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #407 UnixFsChunkFn ----
pub struct UnixFsChunkFn;
impl ComputeFunction for UnixFsChunkFn {
    fn id(&self) -> FunctionId { fid("unixfs_chunk") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let chunk_size: usize = param_str(p, "chunk_size").unwrap_or_else(|| "262144".into())
            .parse().unwrap_or(262144);
        // Split into chunks, create DAG-PB leaves
        let chunks: Vec<&[u8]> = b.chunks(chunk_size).collect();
        let mut leaf_blocks: Vec<(libipld::cid::Cid, Vec<u8>)> = Vec::new();
        for chunk in &chunks {
            let node = libipld::pb::PbNode { links: vec![], data: Some(Bytes::copy_from_slice(chunk)) };
            let encoded = node.into_bytes().to_vec();
            let mh = compute_multihash(&encoded, "sha2-256")?;
            let cid = libipld::cid::Cid::new_v1(DAG_PB, mh);
            leaf_blocks.push((cid, encoded));
        }
        // If single chunk, root = leaf
        if leaf_blocks.len() == 1 {
            let root = leaf_blocks[0].0;
            return Ok(Bytes::from(build_car(&[root], &leaf_blocks)));
        }
        // Build root node linking to leaves
        let links: Vec<libipld::pb::PbLink> = leaf_blocks.iter().enumerate().map(|(i, (cid, data))| {
            libipld::pb::PbLink { cid: *cid, name: Some(format!("{i}")), size: Some(data.len() as u64) }
        }).collect();
        let root_node = libipld::pb::PbNode { links, data: None };
        let root_encoded = root_node.into_bytes().to_vec();
        let root_mh = compute_multihash(&root_encoded, "sha2-256")?;
        let root_cid = libipld::cid::Cid::new_v1(DAG_PB, root_mh);
        let mut all_blocks = vec![(root_cid, root_encoded)];
        all_blocks.extend(leaf_blocks);
        Ok(Bytes::from(build_car(&[root_cid], &all_blocks)))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #408 UnixFsAssembleFn ----
pub struct UnixFsAssembleFn;
impl ComputeFunction for UnixFsAssembleFn {
    fn id(&self) -> FunctionId { fid("unixfs_assemble") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let (roots, blocks) = parse_car(b)?;
        let root_cid = roots.first().ok_or_else(|| ComputeError::ExecutionFailed("no root in CAR".into()))?;
        let block_map: BTreeMap<String, Vec<u8>> = blocks.iter().map(|(c, d)| (c.to_string(), d.clone())).collect();
        fn reassemble(cid: &libipld::cid::Cid, block_map: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, ComputeError> {
            let data = block_map.get(&cid.to_string())
                .ok_or_else(|| ComputeError::ExecutionFailed(format!("missing block: {cid}")))?;
            let node = libipld::pb::PbNode::from_bytes(Bytes::copy_from_slice(data))
                .map_err(|e| ComputeError::ExecutionFailed(format!("dag-pb: {e}")))?;
            if node.links.is_empty() {
                return Ok(node.data.map(|d| d.to_vec()).unwrap_or_default());
            }
            let mut result = Vec::new();
            for link in &node.links {
                result.extend(reassemble(&link.cid, block_map)?);
            }
            Ok(result)
        }
        Ok(Bytes::from(reassemble(root_cid, &block_map)?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #409 MerkleProofGenerateFn ----
pub struct MerkleProofGenerateFn;
impl ComputeFunction for MerkleProofGenerateFn {
    fn id(&self) -> FunctionId { fid("merkle_proof_generate") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let target = param_str(p, "target_cid").ok_or_else(|| ComputeError::InvalidParam("missing target_cid".into()))?;
        let target_cid: libipld::cid::Cid = target.parse().map_err(|e| ComputeError::InvalidParam(format!("target_cid: {e}")))?;
        let (roots, blocks) = parse_car(b)?;
        let root_cid = roots.first().ok_or_else(|| ComputeError::ExecutionFailed("no root in CAR".into()))?;
        let block_map: BTreeMap<String, Vec<u8>> = blocks.iter().map(|(c, d)| (c.to_string(), d.clone())).collect();
        fn find_path(current: &libipld::cid::Cid, target: &libipld::cid::Cid, block_map: &BTreeMap<String, Vec<u8>>, path: &mut Vec<String>) -> bool {
            path.push(current.to_string());
            if current == target { return true; }
            if let Some(data) = block_map.get(&current.to_string()) {
                if let Ok(node) = libipld::pb::PbNode::from_bytes(Bytes::copy_from_slice(data)) {
                    for link in &node.links {
                        if find_path(&link.cid, target, block_map, path) { return true; }
                    }
                }
            }
            path.pop();
            false
        }
        let mut path = Vec::new();
        if !find_path(root_cid, &target_cid, &block_map, &mut path) {
            return Err(ComputeError::ExecutionFailed(format!("target CID not in DAG: {target}")));
        }
        let proof = serde_json::json!({"root": root_cid.to_string(), "target": target, "path": path});
        Ok(Bytes::from(proof.to_string()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #410 MerkleProofVerifyFn ----
pub struct MerkleProofVerifyFn;
impl ComputeFunction for MerkleProofVerifyFn {
    fn id(&self) -> FunctionId { fid("merkle_proof_verify") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let proof: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json: {e}")))?;
        let root_param = param_str(p, "root_cid").ok_or_else(|| ComputeError::InvalidParam("missing root_cid".into()))?;
        let proof_root = proof["root"].as_str().unwrap_or("");
        if proof_root != root_param {
            return Err(ComputeError::ExecutionFailed(format!("root mismatch: proof has {proof_root}, expected {root_param}")));
        }
        let path = proof["path"].as_array().ok_or_else(|| ComputeError::ExecutionFailed("missing path".into()))?;
        if path.is_empty() { return Err(ComputeError::ExecutionFailed("empty path".into())); }
        // Verify path starts at root and each CID is parseable
        let first = path[0].as_str().unwrap_or("");
        if first != root_param {
            return Err(ComputeError::ExecutionFailed("path doesn't start at root".into()));
        }
        for cid_str in path {
            let s = cid_str.as_str().unwrap_or("");
            let _: libipld::cid::Cid = s.parse().map_err(|e| ComputeError::ExecutionFailed(format!("invalid CID in path: {e}")))?;
        }
        Ok(Bytes::from(serde_json::json!({"valid": true}).to_string()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
