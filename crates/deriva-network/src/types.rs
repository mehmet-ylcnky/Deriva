use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId {
    pub addr: SocketAddr,
    pub incarnation: u64,
    pub name: String,
}

impl NodeId {
    pub fn new(addr: SocketAddr, name: impl Into<String>) -> Self {
        Self { addr, incarnation: 0, name: name.into() }
    }

    pub fn next_incarnation(&mut self) {
        self.incarnation += 1;
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.addr, self.incarnation, self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NodeRole {
    Compute,
    Storage,
    #[default]
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberState {
    Alive,
    Suspect,
    Dead,
}

#[derive(Debug, Clone)]
pub struct Member {
    pub id: NodeId,
    pub state: MemberState,
    pub state_change: Instant,
    pub metadata: Option<NodeMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetadata {
    pub grpc_addr: SocketAddr,
    pub role: NodeRole,
    pub cache_bloom: Vec<u8>,
    pub cache_entry_count: u64,
    pub cache_size_bytes: u64,
    pub blob_count: u64,
    pub blob_size_bytes: u64,
    pub recipe_count: u64,
    pub cpu_load: f32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub uptime_secs: u64,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberUpdate {
    pub node: NodeId,
    pub state: MemberState,
    pub incarnation: u64,
    pub metadata: Option<NodeMetadata>,
}

#[derive(Debug, Clone)]
pub enum SwimEvent {
    MemberJoined(NodeId),
    MemberLeft(NodeId),
    MemberSuspect(NodeId),
    MemberAlive(NodeId),
    MetadataUpdated(NodeId, NodeMetadata),
}
