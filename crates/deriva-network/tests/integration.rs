//! Integration tests for SWIM gossip protocol.
//!
//! All tests use ephemeral ports (bind to 127.0.0.1:0) and short
//! intervals for fast execution (< 10s total).

use std::time::Duration;
use deriva_network::config::SwimConfig;
use deriva_network::runtime::SwimRuntime;
use deriva_network::types::*;

fn fast_config() -> SwimConfig {
    SwimConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(75),
        indirect_probes: 2,
        suspicion_mult: 2,
        dead_cleanup: Duration::from_secs(3),
        ..Default::default()
    }
}

/// 7.1: Two-node discovery via seed.
#[tokio::test]
async fn two_node_discovery() {
    let config_a = SwimConfig {
        seeds: vec![],
        ..fast_config()
    };
    let (runtime_a, _ev_a) = SwimRuntime::start(config_a).await.unwrap();
    let a_addr = runtime_a.local_id().addr;

    let config_b = SwimConfig {
        seeds: vec![a_addr],
        ..fast_config()
    };
    let (runtime_b, _ev_b) = SwimRuntime::start(config_b).await.unwrap();

    // Wait for mutual discovery
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert_eq!(runtime_a.alive_count().await, 2, "A should see 2 members");
    assert_eq!(runtime_b.alive_count().await, 2, "B should see 2 members");

    runtime_a.shutdown();
    runtime_b.shutdown();
}


/// 7.2: Failure detection — shut down B, A detects it as dead.
#[tokio::test]
async fn failure_detection() {
    let config_a = SwimConfig {
        seeds: vec![],
        dead_cleanup: Duration::from_secs(1), // Fast cleanup for test
        suspicion_mult: 2,
        ..fast_config()
    };
    let (runtime_a, _ev_a) = SwimRuntime::start(config_a).await.unwrap();
    let a_addr = runtime_a.local_id().addr;

    let config_b = SwimConfig {
        seeds: vec![a_addr],
        dead_cleanup: Duration::from_secs(1),
        suspicion_mult: 2,
        ..fast_config()
    };
    let (runtime_b, _ev_b) = SwimRuntime::start(config_b).await.unwrap();

    // Wait for mutual discovery
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(runtime_a.alive_count().await, 2);

    // Kill node B — drop the runtime (closes socket, stops tasks)
    runtime_b.shutdown();
    drop(runtime_b);
    drop(_ev_b);

    // Wait for A to detect B as suspect → dead
    // With probe_interval=150ms, probe_timeout=75ms, suspicion_mult=2:
    // suspicion_timeout = 2 * 0.15 * ln(3) ≈ 0.33s
    // Cleanup loop runs every dead_cleanup/2 = 1.5s
    // Need: probe failure + suspicion timeout + cleanup loop tick
    tokio::time::sleep(Duration::from_secs(8)).await;

    // alive_count counts non-Dead members; B should be Dead or Removed by now
    let count = runtime_a.alive_count().await;
    assert!(
        count <= 1,
        "A should only see itself after B dies, but sees {} alive",
        count
    );

    runtime_a.shutdown();
}

/// 7.3: Three-node gossip convergence — C discovers B via gossip from A.
#[tokio::test]
async fn three_node_gossip_convergence() {
    let config_a = SwimConfig {
        seeds: vec![],
        ..fast_config()
    };
    let (runtime_a, _ev_a) = SwimRuntime::start(config_a).await.unwrap();
    let a_addr = runtime_a.local_id().addr;

    let config_b = SwimConfig {
        seeds: vec![a_addr],
        ..fast_config()
    };
    let (runtime_b, _ev_b) = SwimRuntime::start(config_b).await.unwrap();

    let config_c = SwimConfig {
        seeds: vec![a_addr],
        ..fast_config()
    };
    let (runtime_c, _ev_c) = SwimRuntime::start(config_c).await.unwrap();

    // Wait for full convergence: B and C seed A, gossip propagates B↔C
    // Needs: join + multiple probe rounds for gossip to propagate
    // With probe_interval=150ms, convergence should happen in ~O(log3) rounds
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Allow for slight timing variance — at minimum each should see >= 2
    let a_count = runtime_a.alive_count().await;
    let b_count = runtime_b.alive_count().await;
    let c_count = runtime_c.alive_count().await;
    assert_eq!(a_count, 3, "A should see 3 members (sees {})", a_count);
    assert_eq!(b_count, 3, "B should see 3 members (sees {})", b_count);
    assert_eq!(c_count, 3, "C should see 3 members (sees {})", c_count);

    runtime_a.shutdown();
    runtime_b.shutdown();
    runtime_c.shutdown();
}


/// 7.4: Incarnation refutation — B refutes false suspicion.
#[tokio::test]
async fn incarnation_refutation() {
    // Use longer suspicion timeout to give B time to refute before promotion to Dead
    let config_a = SwimConfig {
        seeds: vec![],
        suspicion_mult: 50, // Very long suspicion window for test
        ..fast_config()
    };
    let (runtime_a, _ev_a) = SwimRuntime::start(config_a).await.unwrap();
    let a_addr = runtime_a.local_id().addr;

    let config_b = SwimConfig {
        seeds: vec![a_addr],
        suspicion_mult: 50,
        ..fast_config()
    };
    let (runtime_b, _ev_b) = SwimRuntime::start(config_b).await.unwrap();
    let b_id = runtime_b.local_id().clone();

    // Wait for mutual discovery
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(runtime_a.alive_count().await, 2);

    // Manually inject a false suspect update about B into A's memberlist
    {
        let mut ml = runtime_a.member_list().write().await;
        ml.apply_update(&MemberUpdate {
            node: b_id.clone(),
            state: MemberState::Suspect,
            incarnation: b_id.incarnation,
            metadata: None,
        });
        // Queue it for gossip so B will learn about it
        ml.queue_update(MemberUpdate {
            node: b_id.clone(),
            state: MemberState::Suspect,
            incarnation: b_id.incarnation,
            metadata: None,
        });
    }

    // Verify A now sees B as suspect
    {
        let ml = runtime_a.member_list().read().await;
        let member = ml.get_member(&b_id).unwrap();
        assert_eq!(member.state, MemberState::Suspect);
    }

    // Wait for B to receive the gossip and refute with higher incarnation
    tokio::time::sleep(Duration::from_secs(5)).await;

    // A should now see B as alive again (refutation accepted)
    let members = runtime_a.members().await;
    let b_entry = members.iter().find(|(id, _)| id.addr == b_id.addr);
    assert!(
        b_entry.is_some(),
        "B should still be in A's member list"
    );
    let (b_current_id, b_state) = b_entry.unwrap();
    assert_eq!(
        *b_state, MemberState::Alive,
        "B should be alive after refutation (state={:?}, incarnation={})",
        b_state, b_current_id.incarnation
    );
    // B's incarnation should have increased
    assert!(
        b_current_id.incarnation > b_id.incarnation,
        "B's incarnation should have increased from {} but is {}",
        b_id.incarnation, b_current_id.incarnation
    );

    runtime_a.shutdown();
    runtime_b.shutdown();
}
