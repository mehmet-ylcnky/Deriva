use proptest::prelude::*;
use std::net::SocketAddr;
use deriva_network::memberlist::MemberList;
use deriva_network::types::*;

fn arb_addr() -> impl Strategy<Value = SocketAddr> {
    (1u16..=65534).prop_map(|port| {
        format!("127.0.0.1:{}", port).parse().unwrap()
    })
}

fn arb_node_id() -> impl Strategy<Value = NodeId> {
    (arb_addr(), 0u64..100, "[a-z]{3,6}").prop_map(|(addr, inc, name)| {
        let mut id = NodeId::new(addr, name);
        id.incarnation = inc;
        id
    })
}

fn arb_state() -> impl Strategy<Value = MemberState> {
    prop_oneof![
        Just(MemberState::Alive),
        Just(MemberState::Suspect),
        Just(MemberState::Dead),
    ]
}

fn arb_update() -> impl Strategy<Value = MemberUpdate> {
    (arb_node_id(), arb_state(), 0u64..100).prop_map(|(node, state, inc)| {
        MemberUpdate { node, state, incarnation: inc, metadata: None }
    })
}

fn local_node() -> NodeId {
    NodeId::new("127.0.0.1:9999".parse().unwrap(), "local")
}


// --- 8.1: apply_update is idempotent ---

proptest! {
    #[test]
    fn apply_update_is_idempotent(update in arb_update()) {
        let mut ml1 = MemberList::new(local_node());
        let mut ml2 = MemberList::new(local_node());

        // Apply once
        ml1.apply_update(&update);
        // Apply twice
        ml2.apply_update(&update);
        ml2.apply_update(&update);

        // Both should have same member state
        let state1 = ml1.all_members().into_iter()
            .find(|(id, _)| *id == update.node)
            .map(|(_, s)| s);
        let state2 = ml2.all_members().into_iter()
            .find(|(id, _)| *id == update.node)
            .map(|(_, s)| s);
        prop_assert_eq!(state1, state2);
    }
}

// --- 8.2: incarnation conflict resolution is commutative ---

proptest! {
    #[test]
    fn incarnation_resolution_is_commutative(
        node_id in arb_node_id(),
        state_a in arb_state(),
        state_b in arb_state(),
        inc_a in 0u64..50,
        inc_b in 0u64..50,
    ) {
        let update_a = MemberUpdate {
            node: node_id.clone(), state: state_a, incarnation: inc_a, metadata: None,
        };
        let update_b = MemberUpdate {
            node: node_id.clone(), state: state_b, incarnation: inc_b, metadata: None,
        };

        // Apply A then B
        let mut ml1 = MemberList::new(local_node());
        ml1.apply_update(&update_a);
        ml1.apply_update(&update_b);

        // Apply B then A
        let mut ml2 = MemberList::new(local_node());
        ml2.apply_update(&update_b);
        ml2.apply_update(&update_a);

        // Final state should be the same
        let final1 = ml1.all_members().into_iter()
            .find(|(id, _)| *id == node_id)
            .map(|(_, s)| s);
        let final2 = ml2.all_members().into_iter()
            .find(|(id, _)| *id == node_id)
            .map(|(_, s)| s);
        prop_assert_eq!(final1, final2);
    }
}


// --- 8.3: drain_piggyback never returns more than max_count ---

proptest! {
    #[test]
    fn drain_piggyback_respects_max_count(
        num_updates in 0usize..20,
        max_count in 1usize..10,
    ) {
        let mut ml = MemberList::new(local_node());
        for i in 0..num_updates {
            let node = NodeId::new(
                format!("127.0.0.1:{}", 10000 + i).parse().unwrap(),
                format!("n{}", i),
            );
            ml.queue_update(MemberUpdate {
                node, state: MemberState::Alive, incarnation: 0, metadata: None,
            });
        }
        let drained = ml.drain_piggyback(max_count);
        prop_assert!(drained.len() <= max_count);
    }
}

// --- 8.4: refute always produces higher incarnation ---

proptest! {
    #[test]
    fn refute_always_increases_incarnation(initial_inc in 0u64..1000) {
        let mut node = local_node();
        node.incarnation = initial_inc;
        let mut ml = MemberList::new(node);
        let prev = ml.local_id().incarnation;
        let update = ml.refute();
        prop_assert!(update.incarnation > prev);
        prop_assert_eq!(update.state, MemberState::Alive);
        prop_assert!(ml.local_id().incarnation > prev);
    }
}
