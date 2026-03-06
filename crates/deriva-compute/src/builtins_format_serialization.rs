//! Category C: Serialization Formats (19 functions, #244–#262)

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
fn fail(msg: impl std::fmt::Display) -> ComputeError {
    ComputeError::ExecutionFailed(msg.to_string())
}
fn utf8(b: &[u8]) -> Result<&str, ComputeError> {
    std::str::from_utf8(b).map_err(|_| fail("requires UTF-8"))
}

// ---- #244 AvroReadFn ----
pub struct AvroReadFn;
impl ComputeFunction for AvroReadFn {
    fn id(&self) -> FunctionId { fid("avro_read") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let reader = apache_avro::Reader::new(&b[..]).map_err(|e| fail(format!("avro: {e}")))?;
        let mut records = Vec::new();
        for val in reader {
            let v = val.map_err(|e| fail(format!("avro: {e}")))?;
            let json = serde_json::Value::try_from(v).map_err(|e| fail(format!("json: {e}")))?;
            records.push(json);
        }
        Ok(Bytes::from(serde_json::to_string(&records).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #245 AvroWriteFn ----
pub struct AvroWriteFn;
impl ComputeFunction for AvroWriteFn {
    fn id(&self) -> FunctionId { fid("avro_write") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let schema = apache_avro::Schema::parse_str(utf8(&inputs[1])?).map_err(|e| fail(format!("avro: {e}")))?;
        let codec = match param_str(p, "codec").as_deref() {
            Some("deflate") => apache_avro::Codec::Deflate(Default::default()),
            _ => apache_avro::Codec::Null,
        };
        let records: Vec<serde_json::Value> = serde_json::from_slice(&inputs[0]).map_err(|e| fail(format!("json: {e}")))?;
        let mut writer = apache_avro::Writer::with_codec(&schema, Vec::new(), codec);
        for rec in records {
            let avro_val = apache_avro::types::Value::from(rec).resolve(&schema).map_err(|e| fail(format!("avro: {e}")))?;
            writer.append(avro_val).map_err(|e| fail(format!("avro: {e}")))?;
        }
        Ok(Bytes::from(writer.into_inner().map_err(|e| fail(format!("avro: {e}")))?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #246 AvroSchemaExtractFn ----
pub struct AvroSchemaExtractFn;
impl ComputeFunction for AvroSchemaExtractFn {
    fn id(&self) -> FunctionId { fid("avro_schema_extract") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let reader = apache_avro::Reader::new(&b[..]).map_err(|e| fail(format!("avro: {e}")))?;
        Ok(Bytes::from(reader.writer_schema().canonical_form()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #247 AvroSchemaEvolveFn ----
pub struct AvroSchemaEvolveFn;
impl ComputeFunction for AvroSchemaEvolveFn {
    fn id(&self) -> FunctionId { fid("avro_schema_evolve") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let new_schema = apache_avro::Schema::parse_str(utf8(&inputs[1])?).map_err(|e| fail(format!("avro: {e}")))?;
        let reader = apache_avro::Reader::with_schema(&new_schema, &inputs[0][..]).map_err(|e| fail(format!("avro: {e}")))?;
        let codec = match param_str(p, "codec").as_deref() {
            Some("deflate") => apache_avro::Codec::Deflate(Default::default()),
            _ => apache_avro::Codec::Null,
        };
        let mut writer = apache_avro::Writer::with_codec(&new_schema, Vec::new(), codec);
        for val in reader {
            writer.append(val.map_err(|e| fail(format!("avro: {e}")))?).map_err(|e| fail(format!("avro: {e}")))?;
        }
        Ok(Bytes::from(writer.into_inner().map_err(|e| fail(format!("avro: {e}")))?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #248 AvroToJsonFn ----
pub struct AvroToJsonFn;
impl ComputeFunction for AvroToJsonFn {
    fn id(&self) -> FunctionId { fid("avro_to_json") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let reader = apache_avro::Reader::new(&b[..]).map_err(|e| fail(format!("avro: {e}")))?;
        let mut lines = Vec::new();
        for val in reader {
            let v = val.map_err(|e| fail(format!("avro: {e}")))?;
            let json = serde_json::Value::try_from(v).map_err(|e| fail(format!("json: {e}")))?;
            lines.push(serde_json::to_string(&json).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #249 JsonToAvroFn ----
pub struct JsonToAvroFn;
impl ComputeFunction for JsonToAvroFn {
    fn id(&self) -> FunctionId { fid("json_to_avro") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let schema = apache_avro::Schema::parse_str(utf8(&inputs[1])?).map_err(|e| fail(format!("avro: {e}")))?;
        let text = utf8(&inputs[0])?;
        let mut writer = apache_avro::Writer::new(&schema, Vec::new());
        for line in text.lines() {
            if line.is_empty() { continue; }
            let json: serde_json::Value = serde_json::from_str(line).map_err(|e| fail(format!("json: {e}")))?;
            writer.append(apache_avro::types::Value::from(json).resolve(&schema).map_err(|e| fail(format!("avro: {e}")))?).map_err(|e| fail(format!("avro: {e}")))?;
        }
        Ok(Bytes::from(writer.into_inner().map_err(|e| fail(format!("avro: {e}")))?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #250 ProtobufDecodeFn ----
pub struct ProtobufDecodeFn;
impl ComputeFunction for ProtobufDecodeFn {
    fn id(&self) -> FunctionId { fid("protobuf_decode") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        use base64::Engine;
        let b = one(&inputs)?;
        let desc_b64 = param_str(p, "descriptor").ok_or_else(|| ComputeError::InvalidParam("descriptor required".into()))?;
        let msg_type = param_str(p, "message_type").ok_or_else(|| ComputeError::InvalidParam("message_type required".into()))?;
        let desc_bytes = base64::engine::general_purpose::STANDARD.decode(&desc_b64).map_err(|e| fail(format!("base64: {e}")))?;
        let pool = prost_reflect::DescriptorPool::decode(desc_bytes.as_slice()).map_err(|e| fail(format!("descriptor: {e}")))?;
        let md = pool.get_message_by_name(&msg_type).ok_or_else(|| fail(format!("message type not found: {msg_type}")))?;
        let msg = prost_reflect::DynamicMessage::decode(md, &b[..]).map_err(|e| fail(format!("protobuf: {e}")))?;
        let opts = prost_reflect::SerializeOptions::new().stringify_64_bit_integers(false);
        let json = serde_json::to_value(&msg).map_err(|e| fail(format!("json: {e}")))?;
        Ok(Bytes::from(serde_json::to_string(&json).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #251 ProtobufEncodeFn ----
pub struct ProtobufEncodeFn;
impl ComputeFunction for ProtobufEncodeFn {
    fn id(&self) -> FunctionId { fid("protobuf_encode") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        use base64::Engine;
        use prost::Message;
        let b = one(&inputs)?;
        let desc_b64 = param_str(p, "descriptor").ok_or_else(|| ComputeError::InvalidParam("descriptor required".into()))?;
        let msg_type = param_str(p, "message_type").ok_or_else(|| ComputeError::InvalidParam("message_type required".into()))?;
        let desc_bytes = base64::engine::general_purpose::STANDARD.decode(&desc_b64).map_err(|e| fail(format!("base64: {e}")))?;
        let pool = prost_reflect::DescriptorPool::decode(desc_bytes.as_slice()).map_err(|e| fail(format!("descriptor: {e}")))?;
        let md = pool.get_message_by_name(&msg_type).ok_or_else(|| fail(format!("message type not found: {msg_type}")))?;
        let json_val: serde_json::Value = serde_json::from_slice(b).map_err(|e| fail(format!("json: {e}")))?;
        let opts = prost_reflect::DeserializeOptions::new().deny_unknown_fields(false);
        let msg = prost_reflect::DynamicMessage::deserialize_with_options(md, &json_val, &opts)
            .map_err(|e| fail(format!("protobuf: {e}")))?;
        let mut buf = Vec::new();
        msg.encode(&mut buf).map_err(|e| fail(format!("encode: {e}")))?;
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #252 ProtobufSchemaExtractFn ----
pub struct ProtobufSchemaExtractFn;
impl ComputeFunction for ProtobufSchemaExtractFn {
    fn id(&self) -> FunctionId { fid("protobuf_schema_extract") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        use base64::Engine;
        let b = one(&inputs)?;
        let desc_bytes = base64::engine::general_purpose::STANDARD.decode(utf8(b)?).map_err(|e| fail(format!("base64: {e}")))?;
        let pool = prost_reflect::DescriptorPool::decode(desc_bytes.as_slice()).map_err(|e| fail(format!("descriptor: {e}")))?;
        let mut messages = serde_json::Map::new();
        for msg in pool.all_messages() {
            let mut fields = serde_json::Map::new();
            for field in msg.fields() {
                fields.insert(field.name().to_string(), serde_json::json!({
                    "number": field.number(),
                    "kind": format!("{:?}", field.kind()),
                }));
            }
            messages.insert(msg.full_name().to_string(), serde_json::Value::Object(fields));
        }
        Ok(Bytes::from(serde_json::to_string(&messages).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #253 ThriftDecodeFn (stub) ----
pub struct ThriftDecodeFn;
impl ComputeFunction for ThriftDecodeFn {
    fn id(&self) -> FunctionId { fid("thrift_decode") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("thrift_decode: not yet implemented — no stable schemaless Thrift decoder in Rust"))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #254 ThriftEncodeFn (stub) ----
pub struct ThriftEncodeFn;
impl ComputeFunction for ThriftEncodeFn {
    fn id(&self) -> FunctionId { fid("thrift_encode") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("thrift_encode: not yet implemented — no stable schemaless Thrift encoder in Rust"))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #255 MsgpackDecodeFn ----
pub struct MsgpackDecodeFn;
impl ComputeFunction for MsgpackDecodeFn {
    fn id(&self) -> FunctionId { fid("msgpack_decode") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let val: serde_json::Value = rmp_serde::from_slice(b).map_err(|e| fail(format!("msgpack: {e}")))?;
        Ok(Bytes::from(serde_json::to_string(&val).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #256 MsgpackEncodeFn ----
pub struct MsgpackEncodeFn;
impl ComputeFunction for MsgpackEncodeFn {
    fn id(&self) -> FunctionId { fid("msgpack_encode") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let val: serde_json::Value = serde_json::from_slice(b).map_err(|e| fail(format!("json: {e}")))?;
        Ok(Bytes::from(rmp_serde::to_vec(&val).map_err(|e| fail(format!("msgpack: {e}")))?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #257 CborDecodeFn ----
pub struct CborDecodeFn;
impl ComputeFunction for CborDecodeFn {
    fn id(&self) -> FunctionId { fid("cbor_decode") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let val: ciborium::Value = ciborium::from_reader(&b[..]).map_err(|e| fail(format!("cbor: {e}")))?;
        Ok(Bytes::from(serde_json::to_string(&cbor_to_json(&val)).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn cbor_to_json(v: &ciborium::Value) -> serde_json::Value {
    use base64::Engine;
    match v {
        ciborium::Value::Integer(i) => { let n: i128 = (*i).into(); serde_json::json!(n) }
        ciborium::Value::Float(f) => serde_json::json!(f),
        ciborium::Value::Text(s) => serde_json::json!(s),
        ciborium::Value::Bool(b) => serde_json::json!(b),
        ciborium::Value::Null => serde_json::Value::Null,
        ciborium::Value::Bytes(b) => serde_json::json!({"$bytes": base64::engine::general_purpose::STANDARD.encode(b)}),
        ciborium::Value::Array(arr) => serde_json::Value::Array(arr.iter().map(cbor_to_json).collect()),
        ciborium::Value::Map(m) => {
            let mut map = serde_json::Map::new();
            for (k, val) in m {
                let key = match k {
                    ciborium::Value::Text(s) => s.clone(),
                    other => serde_json::to_string(&cbor_to_json(other)).unwrap(),
                };
                map.insert(key, cbor_to_json(val));
            }
            serde_json::Value::Object(map)
        }
        ciborium::Value::Tag(tag, inner) => serde_json::json!({"$tag": tag, "value": cbor_to_json(inner)}),
        _ => serde_json::Value::Null,
    }
}

// ---- #258 CborEncodeFn ----
pub struct CborEncodeFn;
impl ComputeFunction for CborEncodeFn {
    fn id(&self) -> FunctionId { fid("cbor_encode") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let val: serde_json::Value = serde_json::from_slice(b).map_err(|e| fail(format!("json: {e}")))?;
        let mut buf = Vec::new();
        ciborium::into_writer(&json_to_cbor(&val), &mut buf).map_err(|e| fail(format!("cbor: {e}")))?;
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn json_to_cbor(v: &serde_json::Value) -> ciborium::Value {
    match v {
        serde_json::Value::Null => ciborium::Value::Null,
        serde_json::Value::Bool(b) => ciborium::Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { ciborium::Value::Integer(i.into()) }
            else if let Some(u) = n.as_u64() { ciborium::Value::Integer(u.into()) }
            else { ciborium::Value::Float(n.as_f64().unwrap()) }
        }
        serde_json::Value::String(s) => ciborium::Value::Text(s.clone()),
        serde_json::Value::Array(a) => ciborium::Value::Array(a.iter().map(json_to_cbor).collect()),
        serde_json::Value::Object(m) => ciborium::Value::Map(m.iter().map(|(k, v)| (ciborium::Value::Text(k.clone()), json_to_cbor(v))).collect()),
    }
}

// ---- #259 BsonDecodeFn ----
pub struct BsonDecodeFn;
impl ComputeFunction for BsonDecodeFn {
    fn id(&self) -> FunctionId { fid("bson_decode") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let doc = bson::Document::from_reader(&mut &b[..]).map_err(|e| fail(format!("bson: {e}")))?;
        // Convert via Bson → serde_json::Value using bson's into_relaxed_extjson
        let bson_val = bson::Bson::Document(doc);
        let json = bson_val.into_relaxed_extjson();
        Ok(Bytes::from(serde_json::to_string(&json).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #260 BsonEncodeFn ----
pub struct BsonEncodeFn;
impl ComputeFunction for BsonEncodeFn {
    fn id(&self) -> FunctionId { fid("bson_encode") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let json: serde_json::Value = serde_json::from_slice(b).map_err(|e| fail(format!("json: {e}")))?;
        let obj = json.as_object().ok_or_else(|| fail("input must be a JSON object"))?;
        let mut doc = bson::Document::new();
        for (k, v) in obj {
            doc.insert(k.clone(), json_to_bson(v));
        }
        let mut buf = Vec::new();
        doc.to_writer(&mut buf).map_err(|e| fail(format!("bson: {e}")))?;
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn json_to_bson(v: &serde_json::Value) -> bson::Bson {
    match v {
        serde_json::Value::Null => bson::Bson::Null,
        serde_json::Value::Bool(b) => bson::Bson::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { bson::Bson::Int64(i) }
            else { bson::Bson::Double(n.as_f64().unwrap()) }
        }
        serde_json::Value::String(s) => bson::Bson::String(s.clone()),
        serde_json::Value::Array(a) => bson::Bson::Array(a.iter().map(json_to_bson).collect()),
        serde_json::Value::Object(m) => {
            let mut doc = bson::Document::new();
            for (k, val) in m { doc.insert(k.clone(), json_to_bson(val)); }
            bson::Bson::Document(doc)
        }
    }
}

// ---- #261 AvroToParquetFn (stub) ----
pub struct AvroToParquetFn;
impl ComputeFunction for AvroToParquetFn {
    fn id(&self) -> FunctionId { fid("avro_to_parquet") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("avro_to_parquet: requires parquet feature (Phase 3)"))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #262 ParquetToAvroFn (stub) ----
pub struct ParquetToAvroFn;
impl ComputeFunction for ParquetToAvroFn {
    fn id(&self) -> FunctionId { fid("parquet_to_avro") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("parquet_to_avro: requires parquet feature (Phase 3)"))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
