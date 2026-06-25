use crate::types::NodeRole;
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SwimConfig {
    pub bind_addr: SocketAddr,
    pub grpc_addr: SocketAddr,
    pub seeds: Vec<SocketAddr>,
    pub probe_interval: Duration,
    pub probe_timeout: Duration,
    pub indirect_probes: usize,
    pub suspicion_mult: u32,
    pub dead_cleanup: Duration,
    pub max_piggyback: usize,
    pub metadata_interval: Duration,
    pub bloom_bits: usize,
    pub bloom_hashes: usize,
    pub role: NodeRole,
    pub node_name: Option<String>,
}

impl Default for SwimConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:7947".parse().unwrap(),
            grpc_addr: "127.0.0.1:50051".parse().unwrap(),
            seeds: Vec::new(),
            probe_interval: Duration::from_secs(1),
            probe_timeout: Duration::from_millis(500),
            indirect_probes: 3,
            suspicion_mult: 4,
            dead_cleanup: Duration::from_secs(30),
            max_piggyback: 8,
            metadata_interval: Duration::from_secs(10),
            bloom_bits: 96_000,
            bloom_hashes: 7,
            role: NodeRole::Hybrid,
            node_name: None,
        }
    }
}

impl SwimConfig {
    pub fn suspicion_timeout(&self, member_count: usize) -> Duration {
        let secs = self.suspicion_mult as f64
            * self.probe_interval.as_secs_f64()
            * ((member_count + 1) as f64).ln();
        Duration::from_secs_f64(secs)
    }
}
