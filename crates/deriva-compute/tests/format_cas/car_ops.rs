use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_cas::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- CarCreateFn (#403) ----
#[test]
fn car_create_single_block() {
    let r = CarCreateFn.execute(vec![Bytes::from("hello")], &BTreeMap::new()).unwrap();
    assert!(!r.is_empty());
}
#[test]
fn car_create_multiple_blocks() {
    let r = CarCreateFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &BTreeMap::new()).unwrap();
    let list = CarListFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&list).unwrap();
    assert_eq!(entries.len(), 2);
}
#[test]
fn car_create_empty_data() {
    let r = CarCreateFn.execute(vec![Bytes::from("")], &BTreeMap::new()).unwrap();
    assert!(!r.is_empty());
}
#[test]
fn car_create_verify_roundtrip() {
    let car = CarCreateFn.execute(vec![Bytes::from("test data")], &BTreeMap::new()).unwrap();
    let verified = CarVerifyFn.execute(vec![car], &BTreeMap::new()).unwrap();
    assert!(!verified.is_empty());
}
#[test]
fn car_create_binary_data() {
    let data = vec![0u8, 1, 2, 255, 254, 253];
    let car = CarCreateFn.execute(vec![Bytes::from(data.clone())], &BTreeMap::new()).unwrap();
    let list = CarListFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&list).unwrap();
    assert_eq!(entries[0]["size"], data.len());
}

// ---- CarExtractFn (#404) ----
#[test]
fn car_extract_manifest() {
    let car = CarCreateFn.execute(vec![Bytes::from("hello")], &BTreeMap::new()).unwrap();
    let r = CarExtractFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["roots"].is_array());
    assert!(v["blocks"].is_array());
}
#[test]
fn car_extract_specific_block() {
    let data = Bytes::from("extract me");
    let car = CarCreateFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    // Get the CID from the list
    let list = CarListFn.execute(vec![car.clone()], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&list).unwrap();
    let cid_str = entries[0]["cid"].as_str().unwrap();
    let block = CarExtractFn.execute(vec![car], &p(&[("cid", cid_str)])).unwrap();
    assert_eq!(block, data);
}
#[test]
fn car_extract_missing_block() {
    let car = CarCreateFn.execute(vec![Bytes::from("x")], &BTreeMap::new()).unwrap();
    // Use a CID that won't be in the archive
    let other_cid = CidComputeFn.execute(vec![Bytes::from("other")], &BTreeMap::new()).unwrap();
    let cid_str = std::str::from_utf8(&other_cid).unwrap();
    let r = CarExtractFn.execute(vec![car], &p(&[("cid", cid_str)]));
    assert!(r.is_err());
}
#[test]
fn car_extract_roots_present() {
    let car = CarCreateFn.execute(vec![Bytes::from("data")], &BTreeMap::new()).unwrap();
    let r = CarExtractFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(!v["roots"].as_array().unwrap().is_empty());
}
#[test]
fn car_extract_block_size() {
    let car = CarCreateFn.execute(vec![Bytes::from("12345")], &BTreeMap::new()).unwrap();
    let r = CarExtractFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["blocks"][0]["size"], 5);
}

// ---- CarListFn (#405) ----
#[test]
fn car_list_single() {
    let car = CarCreateFn.execute(vec![Bytes::from("x")], &BTreeMap::new()).unwrap();
    let r = CarListFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(entries.len(), 1);
}
#[test]
fn car_list_has_cid_and_size() {
    let car = CarCreateFn.execute(vec![Bytes::from("data")], &BTreeMap::new()).unwrap();
    let r = CarListFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert!(entries[0]["cid"].is_string());
    assert!(entries[0]["size"].is_number());
}
#[test]
fn car_list_multiple() {
    let car = CarCreateFn.execute(vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")], &BTreeMap::new()).unwrap();
    let r = CarListFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(entries.len(), 3);
}
#[test]
fn car_list_cids_unique() {
    let car = CarCreateFn.execute(vec![Bytes::from("x"), Bytes::from("y")], &BTreeMap::new()).unwrap();
    let r = CarListFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_ne!(entries[0]["cid"], entries[1]["cid"]);
}
#[test]
fn car_list_correct_sizes() {
    let car = CarCreateFn.execute(vec![Bytes::from("abc"), Bytes::from("defgh")], &BTreeMap::new()).unwrap();
    let r = CarListFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    let sizes: Vec<u64> = entries.iter().map(|e| e["size"].as_u64().unwrap()).collect();
    assert!(sizes.contains(&3));
    assert!(sizes.contains(&5));
}

// ---- CarVerifyFn (#406) ----
#[test]
fn car_verify_valid() {
    let car = CarCreateFn.execute(vec![Bytes::from("valid")], &BTreeMap::new()).unwrap();
    let r = CarVerifyFn.execute(vec![car.clone()], &BTreeMap::new()).unwrap();
    assert_eq!(r, car);
}
#[test]
fn car_verify_tampered() {
    let mut car = CarCreateFn.execute(vec![Bytes::from("original")], &BTreeMap::new()).unwrap().to_vec();
    // Tamper with last byte
    if let Some(last) = car.last_mut() { *last ^= 0xFF; }
    let r = CarVerifyFn.execute(vec![Bytes::from(car)], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn car_verify_multiple_blocks() {
    let car = CarCreateFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &BTreeMap::new()).unwrap();
    let r = CarVerifyFn.execute(vec![car], &BTreeMap::new());
    assert!(r.is_ok());
}
#[test]
fn car_verify_empty_data_block() {
    let car = CarCreateFn.execute(vec![Bytes::from("")], &BTreeMap::new()).unwrap();
    let r = CarVerifyFn.execute(vec![car], &BTreeMap::new());
    assert!(r.is_ok());
}
#[test]
fn car_verify_passthrough() {
    let car = CarCreateFn.execute(vec![Bytes::from("pass")], &BTreeMap::new()).unwrap();
    let verified = CarVerifyFn.execute(vec![car.clone()], &BTreeMap::new()).unwrap();
    assert_eq!(car, verified);
}

// ---- UnixFsChunkFn (#407) ----
#[test]
fn unixfs_chunk_small_file() {
    let data = Bytes::from("small file content");
    let car = UnixFsChunkFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let list = CarListFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&list).unwrap();
    assert_eq!(entries.len(), 1); // single chunk = single block
}
#[test]
fn unixfs_chunk_large_file() {
    let data = Bytes::from(vec![0x42u8; 1024 * 300]); // 300KB > 256KB default
    let car = UnixFsChunkFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let list = CarListFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&list).unwrap();
    assert!(entries.len() >= 3); // root + 2 leaves
}
#[test]
fn unixfs_chunk_custom_size() {
    let data = Bytes::from(vec![0x41u8; 200]);
    let car = UnixFsChunkFn.execute(vec![data], &p(&[("chunk_size","100")])).unwrap();
    let list = CarListFn.execute(vec![car], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&list).unwrap();
    assert!(entries.len() >= 3); // root + 2 leaves
}
#[test]
fn unixfs_chunk_assemble_roundtrip() {
    let original = Bytes::from(vec![0x42u8; 1024 * 300]);
    let car = UnixFsChunkFn.execute(vec![original.clone()], &BTreeMap::new()).unwrap();
    let reassembled = UnixFsAssembleFn.execute(vec![car], &BTreeMap::new()).unwrap();
    assert_eq!(original, reassembled);
}
#[test]
fn unixfs_chunk_deterministic() {
    let data = Bytes::from("deterministic test");
    let c1 = UnixFsChunkFn.execute(vec![data.clone()], &BTreeMap::new()).unwrap();
    let c2 = UnixFsChunkFn.execute(vec![data], &BTreeMap::new()).unwrap();
    assert_eq!(c1, c2);
}

// ---- UnixFsAssembleFn (#408) ----
#[test]
fn unixfs_assemble_single_chunk() {
    let original = Bytes::from("single chunk");
    let car = UnixFsChunkFn.execute(vec![original.clone()], &BTreeMap::new()).unwrap();
    let reassembled = UnixFsAssembleFn.execute(vec![car], &BTreeMap::new()).unwrap();
    assert_eq!(original, reassembled);
}
#[test]
fn unixfs_assemble_empty() {
    let original = Bytes::from("");
    let car = UnixFsChunkFn.execute(vec![original.clone()], &BTreeMap::new()).unwrap();
    let reassembled = UnixFsAssembleFn.execute(vec![car], &BTreeMap::new()).unwrap();
    assert_eq!(original, reassembled);
}
#[test]
fn unixfs_assemble_binary() {
    let original = Bytes::from(vec![0u8, 1, 2, 255, 254, 253]);
    let car = UnixFsChunkFn.execute(vec![original.clone()], &BTreeMap::new()).unwrap();
    let reassembled = UnixFsAssembleFn.execute(vec![car], &BTreeMap::new()).unwrap();
    assert_eq!(original, reassembled);
}
#[test]
fn unixfs_assemble_multi_chunk() {
    let original = Bytes::from(vec![0xABu8; 500]);
    let car = UnixFsChunkFn.execute(vec![original.clone()], &p(&[("chunk_size","200")])).unwrap();
    let reassembled = UnixFsAssembleFn.execute(vec![car], &BTreeMap::new()).unwrap();
    assert_eq!(original, reassembled);
}
#[test]
fn unixfs_assemble_preserves_length() {
    let original = Bytes::from(vec![0x42u8; 1024 * 300]);
    let car = UnixFsChunkFn.execute(vec![original.clone()], &BTreeMap::new()).unwrap();
    let reassembled = UnixFsAssembleFn.execute(vec![car], &BTreeMap::new()).unwrap();
    assert_eq!(original.len(), reassembled.len());
}

// ---- MerkleProofGenerateFn (#409) + MerkleProofVerifyFn (#410) ----
#[test]
fn merkle_proof_generate_leaf() {
    let data = Bytes::from(vec![0x42u8; 1024 * 300]);
    let car = UnixFsChunkFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let list = CarListFn.execute(vec![car.clone()], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&list).unwrap();
    let leaf_cid = entries.last().unwrap()["cid"].as_str().unwrap();
    let proof = MerkleProofGenerateFn.execute(vec![car], &p(&[("target_cid", leaf_cid)])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert!(v["path"].as_array().unwrap().len() >= 2);
}
#[test]
fn merkle_proof_generate_root() {
    let data = Bytes::from(vec![0x42u8; 1024 * 300]);
    let car = UnixFsChunkFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let list_json = CarExtractFn.execute(vec![car.clone()], &BTreeMap::new()).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&list_json).unwrap();
    let root_cid = manifest["roots"][0].as_str().unwrap();
    let proof = MerkleProofGenerateFn.execute(vec![car], &p(&[("target_cid", root_cid)])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert_eq!(v["path"].as_array().unwrap().len(), 1);
}
#[test]
fn merkle_proof_generate_missing_target() {
    let car = CarCreateFn.execute(vec![Bytes::from("x")], &BTreeMap::new()).unwrap();
    let other = CidComputeFn.execute(vec![Bytes::from("other")], &BTreeMap::new()).unwrap();
    let r = MerkleProofGenerateFn.execute(vec![car], &p(&[("target_cid", std::str::from_utf8(&other).unwrap())]));
    assert!(r.is_err());
}
#[test]
fn merkle_proof_verify_valid() {
    let data = Bytes::from(vec![0x42u8; 1024 * 300]);
    let car = UnixFsChunkFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let list = CarListFn.execute(vec![car.clone()], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&list).unwrap();
    let leaf_cid = entries.last().unwrap()["cid"].as_str().unwrap();
    let proof = MerkleProofGenerateFn.execute(vec![car.clone()], &p(&[("target_cid", leaf_cid)])).unwrap();
    let proof_v: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    let root_cid = proof_v["root"].as_str().unwrap();
    let r = MerkleProofVerifyFn.execute(vec![proof], &p(&[("root_cid", root_cid)])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["valid"], true);
}
#[test]
fn merkle_proof_verify_wrong_root() {
    let data = Bytes::from(vec![0x42u8; 1024 * 300]);
    let car = UnixFsChunkFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let list = CarListFn.execute(vec![car.clone()], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&list).unwrap();
    let leaf_cid = entries.last().unwrap()["cid"].as_str().unwrap();
    let proof = MerkleProofGenerateFn.execute(vec![car], &p(&[("target_cid", leaf_cid)])).unwrap();
    let other = CidComputeFn.execute(vec![Bytes::from("wrong")], &BTreeMap::new()).unwrap();
    let r = MerkleProofVerifyFn.execute(vec![proof], &p(&[("root_cid", std::str::from_utf8(&other).unwrap())]));
    assert!(r.is_err());
}

#[test]
fn merkle_proof_generate_missing_param() {
    let car = CarCreateFn.execute(vec![Bytes::from("x")], &BTreeMap::new()).unwrap();
    let r = MerkleProofGenerateFn.execute(vec![car], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn merkle_proof_generate_has_root_field() {
    let data = Bytes::from(vec![0x42u8; 1024 * 300]);
    let car = UnixFsChunkFn.execute(vec![data], &BTreeMap::new()).unwrap();
    let list = CarListFn.execute(vec![car.clone()], &BTreeMap::new()).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&list).unwrap();
    let leaf_cid = entries.last().unwrap()["cid"].as_str().unwrap();
    let proof = MerkleProofGenerateFn.execute(vec![car], &p(&[("target_cid", leaf_cid)])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    assert!(v["root"].is_string());
    assert!(v["target"].is_string());
}
#[test]
fn merkle_proof_verify_missing_root_param() {
    let proof = serde_json::json!({"root":"x","target":"y","path":["x","y"]}).to_string();
    let r = MerkleProofVerifyFn.execute(vec![Bytes::from(proof)], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn merkle_proof_verify_empty_path() {
    let proof = serde_json::json!({"root":"x","target":"y","path":[]}).to_string();
    let r = MerkleProofVerifyFn.execute(vec![Bytes::from(proof)], &p(&[("root_cid","x")]));
    assert!(r.is_err());
}
#[test]
fn merkle_proof_verify_invalid_json() {
    let r = MerkleProofVerifyFn.execute(vec![Bytes::from("not json")], &p(&[("root_cid","x")]));
    assert!(r.is_err());
}
