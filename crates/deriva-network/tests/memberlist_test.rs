use std::time::Duration;
use deriva_network::memberlist::MemberList;
use deriva_network::types::*;

fn addr(port: u16) -> std::net::SocketAddr {
    format!("127.0.0.1:{}", port).parse().unwrap()
}

fn node(port: u16) -> NodeId {
    NodeId::new(addr(port), format!("node-{}", port))
}

fn alive_update(n: &NodeId, inc: u64) -> MemberUpdate {
    MemberUpdate { node: n.clone(), state: MemberState::Alive, incarnation: inc, metadata: None }
}

#[test]
fn new_contains_only_self_as_alive() {
    let local = node(7947);
    let ml = MemberList::new(local.clone());
    assert_eq!(ml.alive_count(), 1);
    assert!(ml.alive_peers().is_empty());
    let members = ml.all_members();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].0, local);
    assert_eq!(members[0].1, MemberState::Alive);
}

#[test]
fn alive_peers_excludes_self() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let peer = node(7948);
    ml.apply_update(&alive_update(&peer, 0));
    let peers = ml.alive_peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].id, peer);
}

#[test]
fn apply_update_new_node_emits_member_joined() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let peer = node(7948);
    let event = ml.apply_update(&alive_update(&peer, 0));
    assert!(matches!(event, Some(SwimEvent::MemberJoined(ref id)) if *id == peer));
    assert_eq!(ml.alive_count(), 2);
}

#[test]
fn apply_update_alive_to_suspect_emits_member_suspect() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let peer = node(7948);
    ml.apply_update(&alive_update(&peer, 0));

    let event = ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Suspect, incarnation: 0, metadata: None,
    });
    assert!(matches!(event, Some(SwimEvent::MemberSuspect(ref id)) if *id == peer));
}


#[test]
fn apply_update_suspect_to_dead_emits_member_left() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let peer = node(7948);
    ml.apply_update(&alive_update(&peer, 0));
    ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Suspect, incarnation: 0, metadata: None,
    });
    let event = ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Dead, incarnation: 0, metadata: None,
    });
    assert!(matches!(event, Some(SwimEvent::MemberLeft(ref id)) if *id == peer));
}

#[test]
fn higher_incarnation_wins_regardless_of_state() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let peer = node(7948);
    // Insert as suspect at incarnation 5
    ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Suspect, incarnation: 5, metadata: None,
    });
    // Alive at incarnation 6 should override
    let event = ml.apply_update(&alive_update(&peer, 6));
    assert!(matches!(event, Some(SwimEvent::MemberAlive(_))));
    let members = ml.all_members();
    let (_, state) = members.iter().find(|(id, _)| id.addr == peer.addr).unwrap();
    assert_eq!(*state, MemberState::Alive);
}

#[test]
fn stale_update_lower_incarnation_discarded() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let peer = node(7948);
    ml.apply_update(&alive_update(&peer, 5));
    // Suspect at incarnation 3 — stale, should be ignored
    let event = ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Suspect, incarnation: 3, metadata: None,
    });
    assert!(event.is_none());
    let members = ml.all_members();
    let (_, state) = members.iter().find(|(id, _)| id.addr == peer.addr).unwrap();
    assert_eq!(*state, MemberState::Alive);
}

#[test]
fn same_incarnation_state_priority_dead_over_suspect_over_alive() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let peer = node(7948);
    ml.apply_update(&alive_update(&peer, 0));

    // Alive → Suspect at same incarnation: should succeed
    let event = ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Suspect, incarnation: 0, metadata: None,
    });
    assert!(event.is_some());

    // Suspect → Alive at same incarnation: should be rejected (lower priority)
    let event = ml.apply_update(&alive_update(&peer, 0));
    assert!(event.is_none());

    // Suspect → Dead at same incarnation: should succeed
    let event = ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Dead, incarnation: 0, metadata: None,
    });
    assert!(matches!(event, Some(SwimEvent::MemberLeft(_))));
}

#[test]
fn refute_increments_incarnation_returns_alive() {
    let local = node(7947);
    let mut ml = MemberList::new(local.clone());
    assert_eq!(ml.local_id().incarnation, 0);
    let update = ml.refute();
    assert_eq!(update.incarnation, 1);
    assert_eq!(update.state, MemberState::Alive);
    assert_eq!(update.node.addr, local.addr);
    assert_eq!(ml.local_id().incarnation, 1);
}


#[test]
fn random_peer_returns_none_when_no_peers() {
    let local = node(7947);
    let ml = MemberList::new(local);
    assert!(ml.random_peer().is_none());
}

#[test]
fn random_peer_returns_a_peer_when_peers_exist() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let peer = node(7948);
    ml.apply_update(&alive_update(&peer, 0));
    let picked = ml.random_peer().unwrap();
    assert_eq!(picked, peer);
}

#[test]
fn random_peers_excluding_excludes_target_and_self() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let p1 = node(7948);
    let p2 = node(7949);
    let p3 = node(7950);
    ml.apply_update(&alive_update(&p1, 0));
    ml.apply_update(&alive_update(&p2, 0));
    ml.apply_update(&alive_update(&p3, 0));

    let result = ml.random_peers_excluding(&p1, 10);
    assert!(!result.contains(&p1));
    assert!(!result.contains(&node(7947))); // self excluded
    assert_eq!(result.len(), 2); // p2 and p3
}

#[test]
fn drain_piggyback_returns_up_to_max_count() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    for i in 0..10 {
        ml.queue_update(MemberUpdate {
            node: node(8000 + i), state: MemberState::Alive, incarnation: 0, metadata: None,
        });
    }
    let drained = ml.drain_piggyback(3);
    assert_eq!(drained.len(), 3);
    assert_eq!(ml.pending_count(), 10); // all still there (retransmit decremented, not exhausted)
}

#[test]
fn drain_piggyback_removes_when_retransmit_exhausted() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    ml.queue_update(MemberUpdate {
        node: node(8000), state: MemberState::Alive, incarnation: 0, metadata: None,
    });
    // Default max_retransmit = 4, drain 4 times should exhaust
    for _ in 0..4 {
        let _ = ml.drain_piggyback(1);
    }
    assert_eq!(ml.pending_count(), 0);
}

#[test]
fn cleanup_dead_removes_dead_past_timeout() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let peer = node(7948);
    ml.apply_update(&alive_update(&peer, 0));
    ml.apply_update(&MemberUpdate {
        node: peer.clone(), state: MemberState::Dead, incarnation: 0, metadata: None,
    });
    // Cleanup with 0 timeout → immediate removal
    let removed = ml.cleanup_dead(Duration::from_secs(0));
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0], peer);
    assert_eq!(ml.alive_count(), 1); // only self
}

#[test]
fn cleanup_dead_does_not_remove_alive_or_suspect() {
    let local = node(7947);
    let mut ml = MemberList::new(local);
    let alive_peer = node(7948);
    let suspect_peer = node(7949);
    ml.apply_update(&alive_update(&alive_peer, 0));
    ml.apply_update(&MemberUpdate {
        node: suspect_peer.clone(), state: MemberState::Suspect, incarnation: 0, metadata: None,
    });
    let removed = ml.cleanup_dead(Duration::from_secs(0));
    assert!(removed.is_empty());
}
