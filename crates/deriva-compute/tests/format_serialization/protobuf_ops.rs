use bytes::Bytes;
use deriva_compute::builtins_format_serialization::*;
use deriva_compute::function::ComputeFunction;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

/// Build a FileDescriptorSet for a simple message:
/// message Person { string name = 1; int32 age = 2; }
fn person_descriptor_b64() -> String {
    use base64::Engine;
    use prost::Message;
    let fdp = prost_types::FileDescriptorProto {
        name: Some("test.proto".into()),
        package: Some("test".into()),
        message_type: vec![prost_types::DescriptorProto {
            name: Some("Person".into()),
            field: vec![
                prost_types::FieldDescriptorProto {
                    name: Some("name".into()),
                    number: Some(1),
                    r#type: Some(prost_types::field_descriptor_proto::Type::String.into()),
                    label: Some(prost_types::field_descriptor_proto::Label::Optional.into()),
                    ..Default::default()
                },
                prost_types::FieldDescriptorProto {
                    name: Some("age".into()),
                    number: Some(2),
                    r#type: Some(prost_types::field_descriptor_proto::Type::Int32.into()),
                    label: Some(prost_types::field_descriptor_proto::Label::Optional.into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        syntax: Some("proto3".into()),
        ..Default::default()
    };
    let fds = prost_types::FileDescriptorSet { file: vec![fdp] };
    let mut buf = Vec::new();
    fds.encode(&mut buf).unwrap();
    base64::engine::general_purpose::STANDARD.encode(&buf)
}

// ---- protobuf_decode (5 tests) ----
#[test]
fn protobuf_decode_person() {
    use prost::Message;
    let desc = person_descriptor_b64();
    // Encode a Person manually via prost-reflect
    let pool = prost_reflect::DescriptorPool::decode(
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &desc).unwrap().as_slice()
    ).unwrap();
    let md = pool.get_message_by_name("test.Person").unwrap();
    let mut msg = prost_reflect::DynamicMessage::new(md);
    msg.set_field_by_name("name", prost_reflect::Value::String("Alice".into()));
    msg.set_field_by_name("age", prost_reflect::Value::I32(30));
    let mut wire = Vec::new();
    msg.encode(&mut wire).unwrap();

    let out = ProtobufDecodeFn.execute(
        vec![Bytes::from(wire)],
        &p(&[("descriptor", &desc), ("message_type", "test.Person")]),
    ).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["name"], "Alice");
    assert_eq!(json["age"], 30);
}

#[test]
fn protobuf_decode_empty_message() {
    let desc = person_descriptor_b64();
    let out = ProtobufDecodeFn.execute(
        vec![Bytes::from_static(b"")],
        &p(&[("descriptor", &desc), ("message_type", "test.Person")]),
    ).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    // proto3 default: empty message serializes with default values or omits them
    assert!(json.is_object());
}

#[test]
fn protobuf_decode_missing_descriptor() {
    assert!(ProtobufDecodeFn.execute(
        vec![Bytes::from_static(b"")],
        &p(&[("message_type", "test.Person")]),
    ).is_err());
}

#[test]
fn protobuf_decode_missing_message_type() {
    let desc = person_descriptor_b64();
    assert!(ProtobufDecodeFn.execute(
        vec![Bytes::from_static(b"")],
        &p(&[("descriptor", &desc)]),
    ).is_err());
}

#[test]
fn protobuf_decode_wrong_message_type() {
    let desc = person_descriptor_b64();
    assert!(ProtobufDecodeFn.execute(
        vec![Bytes::from_static(b"")],
        &p(&[("descriptor", &desc), ("message_type", "test.Unknown")]),
    ).is_err());
}

// ---- protobuf_encode (5 tests) ----
#[test]
fn protobuf_encode_roundtrip() {
    let desc = person_descriptor_b64();
    let json = r#"{"name":"Bob","age":25}"#;
    let wire = ProtobufEncodeFn.execute(
        vec![Bytes::from(json)],
        &p(&[("descriptor", &desc), ("message_type", "test.Person")]),
    ).unwrap();
    let back = ProtobufDecodeFn.execute(
        vec![wire],
        &p(&[("descriptor", &desc), ("message_type", "test.Person")]),
    ).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v["name"], "Bob");
    assert_eq!(v["age"], 25);
}

#[test]
fn protobuf_encode_empty_object() {
    let desc = person_descriptor_b64();
    let wire = ProtobufEncodeFn.execute(
        vec![Bytes::from_static(b"{}")],
        &p(&[("descriptor", &desc), ("message_type", "test.Person")]),
    ).unwrap();
    assert!(wire.is_empty()); // proto3 default = empty bytes
}

#[test]
fn protobuf_encode_bad_json() {
    let desc = person_descriptor_b64();
    assert!(ProtobufEncodeFn.execute(
        vec![Bytes::from_static(b"not json")],
        &p(&[("descriptor", &desc), ("message_type", "test.Person")]),
    ).is_err());
}

#[test]
fn protobuf_encode_missing_descriptor() {
    assert!(ProtobufEncodeFn.execute(
        vec![Bytes::from_static(b"{}")],
        &p(&[("message_type", "test.Person")]),
    ).is_err());
}

#[test]
fn protobuf_encode_no_input() {
    let desc = person_descriptor_b64();
    assert!(ProtobufEncodeFn.execute(
        vec![],
        &p(&[("descriptor", &desc), ("message_type", "test.Person")]),
    ).is_err());
}

// ---- protobuf_schema_extract (5 tests) ----
#[test]
fn protobuf_schema_extract_finds_person() {
    let desc = person_descriptor_b64();
    let out = ProtobufSchemaExtractFn.execute(vec![Bytes::from(desc)], &p(&[])).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(schema.get("test.Person").is_some());
}

#[test]
fn protobuf_schema_extract_has_fields() {
    let desc = person_descriptor_b64();
    let out = ProtobufSchemaExtractFn.execute(vec![Bytes::from(desc)], &p(&[])).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let person = &schema["test.Person"];
    assert!(person.get("name").is_some());
    assert!(person.get("age").is_some());
}

#[test]
fn protobuf_schema_extract_field_numbers() {
    let desc = person_descriptor_b64();
    let out = ProtobufSchemaExtractFn.execute(vec![Bytes::from(desc)], &p(&[])).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(schema["test.Person"]["name"]["number"], 1);
    assert_eq!(schema["test.Person"]["age"]["number"], 2);
}

#[test]
fn protobuf_schema_extract_bad_base64() {
    assert!(ProtobufSchemaExtractFn.execute(vec![Bytes::from_static(b"!!!")], &p(&[])).is_err());
}

#[test]
fn protobuf_schema_extract_no_input() {
    assert!(ProtobufSchemaExtractFn.execute(vec![], &p(&[])).is_err());
}
