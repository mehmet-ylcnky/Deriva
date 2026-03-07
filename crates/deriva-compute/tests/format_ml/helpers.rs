use bytes::Bytes;
use deriva_core::address::Value;
use std::collections::BTreeMap;

pub fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

pub fn make_safetensors(name: &str, data: &[u8]) -> Bytes {
    let header = serde_json::json!({
        name: {
            "dtype": "F32",
            "shape": [data.len() / 4],
            "data_offsets": [0, data.len()],
        }
    });
    let header_bytes = serde_json::to_vec(&header).unwrap();
    let header_size = header_bytes.len() as u64;
    let mut out = Vec::new();
    out.extend_from_slice(&header_size.to_le_bytes());
    out.extend_from_slice(&header_bytes);
    out.extend_from_slice(data);
    Bytes::from(out)
}
