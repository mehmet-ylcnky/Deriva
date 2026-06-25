use deriva_network::types::*;

#[test]
fn node_id_equality_same_fields() {
    let a = NodeId::new("127.0.0.1:7947".parse().unwrap(), "node-a");
    let b = NodeId::new("127.0.0.1:7947".parse().unwrap(), "node-a");
    assert_eq!(a, b);
}

#[test]
fn node_id_inequality_different_incarnation() {
    let mut a = NodeId::new("127.0.0.1:7947".parse().unwrap(), "node-a");
    let b = a.clone();
    a.next_incarnation();
    assert_ne!(a, b);
}

#[test]
fn node_id_inequality_different_addr() {
    let a = NodeId::new("127.0.0.1:7947".parse().unwrap(), "node-a");
    let b = NodeId::new("127.0.0.1:7948".parse().unwrap(), "node-a");
    assert_ne!(a, b);
}

#[test]
fn node_id_inequality_different_name() {
    let a = NodeId::new("127.0.0.1:7947".parse().unwrap(), "node-a");
    let b = NodeId::new("127.0.0.1:7947".parse().unwrap(), "node-b");
    assert_ne!(a, b);
}

#[test]
fn node_id_next_incarnation_increments() {
    let mut a = NodeId::new("127.0.0.1:7947".parse().unwrap(), "x");
    assert_eq!(a.incarnation, 0);
    a.next_incarnation();
    assert_eq!(a.incarnation, 1);
    a.next_incarnation();
    assert_eq!(a.incarnation, 2);
}

#[test]
fn node_id_display_format() {
    let a = NodeId::new("127.0.0.1:7947".parse().unwrap(), "alpha");
    assert_eq!(a.to_string(), "127.0.0.1:7947:0:alpha");
}

#[test]
fn node_id_display_with_incarnation() {
    let mut a = NodeId::new("10.0.1.5:7947".parse().unwrap(), "beta");
    a.incarnation = 42;
    assert_eq!(a.to_string(), "10.0.1.5:7947:42:beta");
}


#[test]
fn member_state_serialization_roundtrip() {
    for state in [MemberState::Alive, MemberState::Suspect, MemberState::Dead] {
        let bytes = bincode::serialize(&state).unwrap();
        let decoded: MemberState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(state, decoded);
    }
}

#[test]
fn node_id_serialization_roundtrip() {
    let id = NodeId::new("192.168.1.100:7947".parse().unwrap(), "prod-node");
    let bytes = bincode::serialize(&id).unwrap();
    let decoded: NodeId = bincode::deserialize(&bytes).unwrap();
    assert_eq!(id, decoded);
}

#[test]
fn node_metadata_serialization_roundtrip() {
    let meta = NodeMetadata {
        grpc_addr: "10.0.1.5:50051".parse().unwrap(),
        role: NodeRole::Hybrid,
        cache_bloom: vec![0xAA; 100],
        cache_entry_count: 5000,
        cache_size_bytes: 1_000_000,
        blob_count: 200,
        blob_size_bytes: 50_000_000,
        recipe_count: 150,
        cpu_load: 0.45,
        memory_used_bytes: 2_000_000_000,
        memory_total_bytes: 8_000_000_000,
        uptime_secs: 3600,
        version: "0.1.0".to_string(),
    };
    let bytes = bincode::serialize(&meta).unwrap();
    let decoded: NodeMetadata = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded.grpc_addr, meta.grpc_addr);
    assert_eq!(decoded.cache_entry_count, 5000);
    assert_eq!(decoded.cpu_load, 0.45);
    assert_eq!(decoded.version, "0.1.0");
    assert_eq!(decoded.cache_bloom.len(), 100);
}

#[test]
fn node_role_default_is_hybrid() {
    assert_eq!(NodeRole::default(), NodeRole::Hybrid);
}

#[test]
fn member_update_serialization_roundtrip() {
    let update = MemberUpdate {
        node: NodeId::new("127.0.0.1:7947".parse().unwrap(), "test"),
        state: MemberState::Suspect,
        incarnation: 7,
        metadata: None,
    };
    let bytes = bincode::serialize(&update).unwrap();
    let decoded: MemberUpdate = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded.node, update.node);
    assert_eq!(decoded.incarnation, 7);
}
