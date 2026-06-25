use deriva_network::message::SwimMessage;
use deriva_network::types::*;

fn test_node(port: u16) -> NodeId {
    NodeId::new(format!("127.0.0.1:{}", port).parse().unwrap(), format!("n{}", port))
}

fn test_metadata() -> NodeMetadata {
    NodeMetadata {
        grpc_addr: "127.0.0.1:50051".parse().unwrap(),
        role: NodeRole::Hybrid,
        cache_bloom: vec![0xAB; 1200],
        cache_entry_count: 1000,
        cache_size_bytes: 50_000_000,
        blob_count: 500,
        blob_size_bytes: 100_000_000,
        recipe_count: 300,
        cpu_load: 0.45,
        memory_used_bytes: 2_000_000_000,
        memory_total_bytes: 8_000_000_000,
        uptime_secs: 3600,
        version: "0.1.0".to_string(),
    }
}

#[test]
fn ping_encode_decode_roundtrip() {
    let msg = SwimMessage::Ping {
        sender: test_node(7947),
        sequence: 42,
        piggyback: vec![],
    };
    let data = msg.encode().unwrap();
    let decoded = SwimMessage::decode(&data).unwrap();
    assert_eq!(decoded.sender().addr, test_node(7947).addr);
    assert!(decoded.piggyback().is_empty());
}

#[test]
fn ack_encode_decode_roundtrip() {
    let msg = SwimMessage::Ack {
        sender: test_node(7948),
        sequence: 99,
        piggyback: vec![MemberUpdate {
            node: test_node(7949),
            state: MemberState::Suspect,
            incarnation: 3,
            metadata: None,
        }],
    };
    let data = msg.encode().unwrap();
    let decoded = SwimMessage::decode(&data).unwrap();
    assert_eq!(decoded.sender().addr, test_node(7948).addr);
    assert_eq!(decoded.piggyback().len(), 1);
    assert_eq!(decoded.piggyback()[0].state, MemberState::Suspect);
}


#[test]
fn ping_req_encode_decode_roundtrip() {
    let msg = SwimMessage::PingReq {
        sender: test_node(7947),
        target: test_node(7948),
        sequence: 7,
        piggyback: vec![],
    };
    let data = msg.encode().unwrap();
    let decoded = SwimMessage::decode(&data).unwrap();
    assert_eq!(decoded.sender().addr, test_node(7947).addr);
    match decoded {
        SwimMessage::PingReq { target, .. } => assert_eq!(target.addr, test_node(7948).addr),
        _ => panic!("expected PingReq"),
    }
}

#[test]
fn sync_encode_decode_roundtrip() {
    let members: Vec<MemberUpdate> = (0..5).map(|i| MemberUpdate {
        node: test_node(8000 + i),
        state: MemberState::Alive,
        incarnation: i as u64,
        metadata: None,
    }).collect();
    let msg = SwimMessage::Sync {
        sender: test_node(7947),
        members,
    };
    let data = msg.encode().unwrap();
    let decoded = SwimMessage::decode(&data).unwrap();
    assert_eq!(decoded.piggyback().len(), 5);
}

#[test]
fn message_with_8_piggyback_and_metadata_fits_udp() {
    let updates: Vec<MemberUpdate> = (0..8).map(|i| MemberUpdate {
        node: test_node(8000 + i),
        state: MemberState::Alive,
        incarnation: i as u64,
        metadata: Some(test_metadata()),
    }).collect();
    let msg = SwimMessage::Ping {
        sender: test_node(7947),
        sequence: 1,
        piggyback: updates,
    };
    let data = msg.encode().unwrap();
    assert!(
        data.len() < 65507,
        "message size {} exceeds UDP max payload",
        data.len()
    );
}

#[test]
fn piggyback_extracts_correct_slice_from_each_variant() {
    let updates = vec![MemberUpdate {
        node: test_node(8000),
        state: MemberState::Dead,
        incarnation: 1,
        metadata: None,
    }];

    let ping = SwimMessage::Ping { sender: test_node(7947), sequence: 0, piggyback: updates.clone() };
    assert_eq!(ping.piggyback().len(), 1);
    assert_eq!(ping.piggyback()[0].state, MemberState::Dead);

    let ack = SwimMessage::Ack { sender: test_node(7947), sequence: 0, piggyback: updates.clone() };
    assert_eq!(ack.piggyback().len(), 1);

    let pr = SwimMessage::PingReq { sender: test_node(7947), target: test_node(7948), sequence: 0, piggyback: updates.clone() };
    assert_eq!(pr.piggyback().len(), 1);

    let sync = SwimMessage::Sync { sender: test_node(7947), members: updates };
    assert_eq!(sync.piggyback().len(), 1);
}

#[test]
fn sender_extracts_correct_node_from_each_variant() {
    let s = test_node(7947);
    let ping = SwimMessage::Ping { sender: s.clone(), sequence: 0, piggyback: vec![] };
    assert_eq!(ping.sender(), &s);
    let ack = SwimMessage::Ack { sender: s.clone(), sequence: 0, piggyback: vec![] };
    assert_eq!(ack.sender(), &s);
    let pr = SwimMessage::PingReq { sender: s.clone(), target: test_node(8000), sequence: 0, piggyback: vec![] };
    assert_eq!(pr.sender(), &s);
    let sync = SwimMessage::Sync { sender: s.clone(), members: vec![] };
    assert_eq!(sync.sender(), &s);
}

#[test]
fn decode_corrupted_data_returns_error_no_panic() {
    let garbage = vec![0xFF, 0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x42];
    let result = SwimMessage::decode(&garbage);
    assert!(result.is_err());
}

#[test]
fn decode_empty_data_returns_error_no_panic() {
    let result = SwimMessage::decode(&[]);
    assert!(result.is_err());
}
