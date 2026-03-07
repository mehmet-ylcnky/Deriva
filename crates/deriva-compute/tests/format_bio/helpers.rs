use bytes::Bytes;
use deriva_core::address::Value;
use std::collections::BTreeMap;

pub fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}
