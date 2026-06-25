//! Checkpoint smoke test: verify two nodes discover each other.

use std::time::Duration;
use deriva_network::config::SwimConfig;
use deriva_network::runtime::SwimRuntime;

#[tokio::test]
async fn two_node_discovery_smoke() {
    let config_a = SwimConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        seeds: vec![],
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        ..Default::default()
    };

    let (runtime_a, _events_a) = SwimRuntime::start(config_a).await.unwrap();
    let a_addr = runtime_a.local_id().addr;

    let config_b = SwimConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        seeds: vec![a_addr],
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        ..Default::default()
    };

    let (runtime_b, _events_b) = SwimRuntime::start(config_b).await.unwrap();

    // Wait for mutual discovery
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert_eq!(runtime_a.alive_count().await, 2, "A should see 2 members");
    assert_eq!(runtime_b.alive_count().await, 2, "B should see 2 members");

    runtime_a.shutdown();
    runtime_b.shutdown();
}

#[tokio::test]
async fn single_node_starts_without_seeds() {
    let config = SwimConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        seeds: vec![],
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        ..Default::default()
    };

    let (runtime, _events) = SwimRuntime::start(config).await.unwrap();

    // Single node — only itself
    assert_eq!(runtime.alive_count().await, 1);

    runtime.shutdown();
}
